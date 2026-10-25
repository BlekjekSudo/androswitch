//! java.io character streams and in-memory byte streams.
//!
//! **Writers**: `StringWriter`, `OutputStreamWriter`, `BufferedWriter`,
//! `PrintWriter`, `FileWriter`.
//! - Text accumulates in the writer and is pushed to its sink on `flush`/
//!   `close`.
//! - Sinks: another Writer (`write(String)`), an OutputStream (bytes in the
//!   writer's charset, `write([B)`), or the StringWriter buffer.
//! - Sinks are called through normal virtual dispatch, so host streams
//!   (`FileOutputStream`, `ByteArrayOutputStream`) and DEX subclasses both
//!   work.
//! - PrintWriter: `print`/`println` for every primitive, String and Object,
//!   plus `printf`/`format`, `append`, `checkError`. It flushes on
//!   `println` when autoflush is set.
//!
//! **Readers**: `StringReader`, `InputStreamReader`, `BufferedReader`,
//! `FileReader`.
//! - The first read drains the source: bytes via `read([BII)` decoded in the
//!   reader's charset, or chars from an inner Reader.
//! - Then `read()`, `read([C[II])`, `readLine` (`\n`, `\r\n`, `\r`),
//!   `ready`, `skip`, `mark`/`reset`, `close`.
//!
//! **Byte streams**: `ByteArrayOutputStream` (`write*`, `toByteArray`,
//! `toString([charset])`, `size`, `reset`, `writeTo`) and
//! `ByteArrayInputStream` (`read*`, `available`, `skip`, `mark`/`reset`).
//!
//! **Abstract bases**: DEX subclasses of `Writer`/`Reader` get the base
//! constructors and the template methods (`Writer.write(String)` →
//! `write(char[],int,int)`, `Reader.read()` → `read(char[],int,int)`).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const WRITER: &str = "Ljava/io/Writer;";
const READER: &str = "Ljava/io/Reader;";
const SW: &str = "Ljava/io/StringWriter;";
const OSW: &str = "Ljava/io/OutputStreamWriter;";
const BW: &str = "Ljava/io/BufferedWriter;";
const PW: &str = "Ljava/io/PrintWriter;";
const PS: &str = "Ljava/io/PrintStream;";
const FW: &str = "Ljava/io/FileWriter;";
const SR: &str = "Ljava/io/StringReader;";
const ISR: &str = "Ljava/io/InputStreamReader;";
const BR: &str = "Ljava/io/BufferedReader;";
const FR: &str = "Ljava/io/FileReader;";
const BAOS: &str = "Ljava/io/ByteArrayOutputStream;";
const OUTPUT_STREAM: &str = "Ljava/io/OutputStream;";
const INPUT_STREAM: &str = "Ljava/io/InputStream;";
const FILTER_OS: &str = "Ljava/io/FilterOutputStream;";
const BOS: &str = "Ljava/io/BufferedOutputStream;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";

const WRITERS: [&str; 6] = [SW, OSW, BW, PW, PS, FW];
const READERS: [&str; 4] = [SR, ISR, BR, FR];

const MAX_STREAM_UNITS: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
enum Sink {
    Buffer,
    Writer(ObjRef),
    Stream(ObjRef, String),
}

#[derive(Debug, Clone)]
struct WState {
    sink: Sink,
    pending: Vec<u16>,
    /// StringWriter content (Sink::Buffer).
    text: Vec<u16>,
    autoflush: bool,
    closed: bool,
}

#[derive(Debug, Clone)]
enum Source {
    Text,
    Reader(ObjRef),
    Stream(ObjRef, String),
}

#[derive(Debug, Clone)]
struct RState {
    source: Source,
    loaded: bool,
    chars: Vec<u16>,
    pos: usize,
    mark: usize,
    closed: bool,
}

#[derive(Debug, Clone, Default)]
struct BState {
    bytes: Vec<u8>,
    pos: usize,
    mark: usize,
}

thread_local! {
    static WRITERS_ST: RefCell<HashMap<ObjRef, WState>> = RefCell::new(HashMap::new());
    static READERS_ST: RefCell<HashMap<ObjRef, RState>> = RefCell::new(HashMap::new());
    static BYTES_ST: RefCell<HashMap<ObjRef, BState>> = RefCell::new(HashMap::new());
}

/// GC: objects that have state here (marking consults `gc_entry` only
/// for these).
pub(crate) fn gc_keys(out: &mut Vec<ObjRef>) {
    WRITERS_ST.with(|m| out.extend(m.borrow().keys().copied()));
    READERS_ST.with(|m| out.extend(m.borrow().keys().copied()));
    BYTES_ST.with(|m| out.extend(m.borrow().keys().copied()));
}

/// GC ephemeron: state of a reachable object (holds heap refs).
pub(crate) fn gc_entry(r: ObjRef, out: &mut impl std::fmt::Write) {
    WRITERS_ST.with(|m| {
        if let Some(v) = m.borrow().get(&r) {
            let _ = write!(out, "|{:?}", v.sink);
        }
    });
    READERS_ST.with(|m| {
        if let Some(v) = m.borrow().get(&r) {
            let _ = write!(out, "|{:?}", v.source);
        }
    });
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    WRITERS_ST.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
    READERS_ST.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
    BYTES_ST.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — estados de streams indexados por
/// `ObjRef` morrem com o heap que os criou.
pub(crate) fn reset_all() {
    WRITERS_ST.with(|m| m.borrow_mut().clear());
    READERS_ST.with(|m| m.borrow_mut().clear());
    BYTES_ST.with(|m| m.borrow_mut().clear());
}

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}
fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}
fn int(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
fn obj(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn class_of(vm: &Engine, r: ObjRef) -> String {
    vm.heap.class_of(r).unwrap_or("").to_string()
}

/// Text of a char sequence / object argument (`null` → "null").
fn text_of(vm: &mut Engine, v: Option<&Value>) -> Result<String, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => match vm.heap.as_str(*r) {
            Ok(s) => s.to_string(),
            Err(_) => crate::intrinsics::object_to_string(vm, *r)?,
        },
        _ => "null".to_string(),
    })
}

fn chars_of(vm: &Engine, v: Option<&Value>) -> Vec<u16> {
    match v {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u16).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn bytes_of(vm: &Engine, v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u8).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn require_char_array(vm: &Engine, v: Option<&Value>) -> Result<Vec<u16>, VmExit> {
    let Some(Value::Obj(a)) = v else {
        return Err(exc("Ljava/lang/NullPointerException;", "cbuf == null"));
    };
    if *a == 0 {
        return Err(exc("Ljava/lang/NullPointerException;", "cbuf == null"));
    }
    Ok(vm
        .heap
        .as_array_elems(*a)
        .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u16).collect())
        .unwrap_or_default())
}

fn require_byte_array(vm: &Engine, v: Option<&Value>) -> Result<Vec<u8>, VmExit> {
    let Some(Value::Obj(a)) = v else {
        return Err(exc("Ljava/lang/NullPointerException;", "b == null"));
    };
    if *a == 0 {
        return Err(exc("Ljava/lang/NullPointerException;", "b == null"));
    }
    Ok(vm
        .heap
        .as_array_elems(*a)
        .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u8).collect())
        .unwrap_or_default())
}

fn require_non_null_string(vm: &mut Engine, v: Option<&Value>) -> Result<String, VmExit> {
    let Some(Value::Obj(r)) = v else {
        return Err(exc("Ljava/lang/NullPointerException;", "str == null"));
    };
    if *r == 0 {
        return Err(exc("Ljava/lang/NullPointerException;", "str == null"));
    }
    match vm.heap.as_str(*r) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => crate::intrinsics::object_to_string(vm, *r),
    }
}

fn check_offset_len(cap: usize, off: i32, len: i32) -> Result<(usize, usize), VmExit> {
    if off < 0 || len < 0 {
        return Err(exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("offset {off}, count {len}, length {cap}"),
        ));
    }
    let (uoff, ulen) = (off as usize, len as usize);
    let end = uoff.checked_add(ulen).ok_or_else(|| {
        exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("offset {off}, count {len}, length {cap}"),
        )
    })?;
    if end > cap {
        return Err(exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("offset {off}, count {len}, length {cap}"),
        ));
    }
    Ok((uoff, ulen))
}

fn check_start_end(cap: usize, start: i32, end: i32) -> Result<(usize, usize), VmExit> {
    if start < 0 || end < start || (end as usize) > cap {
        return Err(exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("start {start}, end {end}, length {cap}"),
        ));
    }
    Ok((start as usize, end as usize))
}

fn slice<T: Clone>(all: &[T], off: i32, len: i32) -> Result<Vec<T>, VmExit> {
    let (uoff, ulen) = check_offset_len(all.len(), off, len)?;
    Ok(all[uoff..uoff + ulen].to_vec())
}

/// Value printed by `print(x)` for the parameter type in `sig`, as UTF-16 units
/// so surrogate `char` arguments survive paired `print(char)` calls.
fn print_units(vm: &mut Engine, sig: &str, v: Option<&Value>) -> Result<Vec<u16>, VmExit> {
    let p = sig.trim_start_matches('(').split(')').next().unwrap_or("");
    Ok(match (p, v) {
        ("Z", Some(v)) => (v.as_int().unwrap_or(0) != 0)
            .to_string()
            .encode_utf16()
            .collect(),
        ("C", Some(v)) => vec![(v.as_int().unwrap_or(0) as u32 & 0xffff) as u16],
        ("I", Some(v)) => v.as_int().unwrap_or(0).to_string().encode_utf16().collect(),
        ("J", Some(Value::Long(l))) => l.to_string().encode_utf16().collect(),
        ("F", Some(Value::Float(f))) => crate::numerics::java_double_to_string(*f as f64, true)
            .encode_utf16()
            .collect(),
        ("D", Some(Value::Double(d))) => crate::numerics::java_double_to_string(*d, false)
            .encode_utf16()
            .collect(),
        ("[C", v) => require_char_array(vm, v)?,
        ("", _) => Vec::new(),
        (_, v) => text_of(vm, v)?.encode_utf16().collect(),
    })
}

// ── writers ─────────────────────────────────────────────────────────────

fn wget(r: ObjRef) -> Option<WState> {
    WRITERS_ST.with(|m| m.borrow().get(&r).cloned())
}
fn wput(r: ObjRef, st: WState) {
    WRITERS_ST.with(|m| m.borrow_mut().insert(r, st));
}

fn flush_writer(
    vm: &mut Engine,
    recv: ObjRef,
    close: bool,
    hold_trailing_high: bool,
) -> Result<(), VmExit> {
    let Some(mut st) = wget(recv) else {
        return Ok(());
    };
    let units = if !close
        && hold_trailing_high
        && st
            .pending
            .last()
            .is_some_and(|u| (0xD800..=0xDBFF).contains(u))
    {
        let keep = st.pending.pop();
        let taken = std::mem::take(&mut st.pending);
        if let Some(hi) = keep {
            st.pending.push(hi);
        }
        taken
    } else {
        std::mem::take(&mut st.pending)
    };
    let data = String::from_utf16_lossy(&units);
    if close {
        st.closed = true;
    }
    wput(recv, st.clone());
    match &st.sink {
        Sink::Buffer => {}
        Sink::Writer(w) => {
            if !data.is_empty() {
                let s = jstr(vm, data)?;
                vm.invoke_instance(*w, "write", "(Ljava/lang/String;)V", &[s])?;
            }
            vm.invoke_instance(*w, if close { "close" } else { "flush" }, "()V", &[])?;
        }
        Sink::Stream(os, cs) => {
            if !data.is_empty() {
                let b = crate::jlstring::byte_array(vm, &crate::jlstring::encode(&data, cs))?;
                vm.invoke_instance(*os, "write", "([B)V", &[b])?;
            }
            vm.invoke_instance(*os, if close { "close" } else { "flush" }, "()V", &[])?;
        }
    }
    Ok(())
}

fn write_units(recv: ObjRef, units: &[u16]) -> Result<(), VmExit> {
    let Some(mut st) = wget(recv) else {
        return Ok(());
    };
    if st.closed {
        return Err(exc("Ljava/io/IOException;", "Stream closed"));
    }
    match st.sink {
        Sink::Buffer => st.text.extend_from_slice(units),
        _ => st.pending.extend_from_slice(units),
    }
    wput(recv, st);
    Ok(())
}

fn write_text(recv: ObjRef, s: &str) -> Result<(), VmExit> {
    let units: Vec<u16> = s.encode_utf16().collect();
    write_units(recv, &units)
}

/// Sink for a writer constructor's first argument.
fn sink_for(vm: &mut Engine, class: &str, args: &[Value], sig: &str) -> Result<Sink, VmExit> {
    let charset = || -> Option<usize> {
        sig.contains("Charset;")
            .then_some(1)
            .or(sig.contains("String;)").then_some(1))
    };
    match class {
        SW => Ok(Sink::Buffer),
        FW => {
            if obj(args, 0).is_none() {
                return Err(exc("Ljava/lang/NullPointerException;", "file == null"));
            }
            // FileWriter(File|String[, boolean append | Charset[, boolean]])
            let fos = vm
                .heap
                .alloc_instance("Ljava/io/FileOutputStream;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            let append = args.iter().skip(1).any(|a| matches!(a, Value::Int(1)));
            let ctor_sig = if sig.starts_with("(Ljava/io/File;") {
                "(Ljava/io/File;Z)V"
            } else {
                "(Ljava/lang/String;Z)V"
            };
            crate::jfile::instance_intrinsic(
                vm,
                "Ljava/io/FileOutputStream;",
                "<init>",
                ctor_sig,
                fos,
                &[
                    args.first().cloned().unwrap_or(Value::Null),
                    Value::Int(append as i32),
                ],
            )?;
            Ok(Sink::Stream(fos, "UTF-8".into()))
        }
        _ => {
            let Some(target) = obj(args, 0) else {
                return Err(exc("Ljava/lang/NullPointerException;", "out == null"));
            };
            if vm.cp.is_subtype(&class_of(vm, target), WRITER)
                || WRITERS.contains(&class_of(vm, target).as_str())
            {
                Ok(Sink::Writer(target))
            } else if vm.heap.as_str(target).is_ok() || class_of(vm, target) == "Ljava/io/File;" {
                // PrintWriter(String fileName | File)
                let fos = vm
                    .heap
                    .alloc_instance("Ljava/io/FileOutputStream;".to_string(), Vec::new())
                    .map_err(crate::err::oom_throwable)?;
                let ctor_sig = if class_of(vm, target) == "Ljava/io/File;" {
                    "(Ljava/io/File;)V"
                } else {
                    "(Ljava/lang/String;)V"
                };
                crate::jfile::instance_intrinsic(
                    vm,
                    "Ljava/io/FileOutputStream;",
                    "<init>",
                    ctor_sig,
                    fos,
                    &[Value::Obj(target)],
                )?;
                Ok(Sink::Stream(fos, "UTF-8".into()))
            } else {
                let cs = match charset() {
                    Some(i) if class == OSW => crate::jlstring::charset_name(vm, args.get(i))?,
                    _ => "UTF-8".into(),
                };
                Ok(Sink::Stream(target, cs))
            }
        }
    }
}

fn writer(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let sink = sink_for(vm, class, args, sig)?;
        let autoflush = matches!(class, PW | PS)
            && sig.contains("Z)V")
            && args.iter().any(|a| matches!(a, Value::Int(1)));
        wput(
            recv,
            WState {
                sink,
                pending: Vec::new(),
                text: Vec::new(),
                autoflush,
                closed: false,
            },
        );
        return Ok(Some(Value::Null));
    }
    if wget(recv).is_none() {
        return Ok(None);
    }
    let ret_self = sig.ends_with(")Ljava/io/Writer;")
        || sig.ends_with(")Ljava/io/PrintWriter;")
        || sig.ends_with(")Ljava/io/PrintStream;")
        || sig.ends_with(")Ljava/io/StringWriter;")
        || sig.ends_with(")Ljava/lang/Appendable;");
    let out = match name {
        "write" if class == PS && (sig == "([B)V" || sig == "([BII)V") => {
            let raw = if sig == "([B)V" {
                require_byte_array(vm, args.first())?
            } else {
                let all = require_byte_array(vm, args.first())?;
                slice(&all, int(args, 1), int(args, 2))?
            };
            flush_writer(vm, recv, false, false)?;
            if let Some(WState {
                sink: Sink::Stream(os, _),
                closed: false,
                ..
            }) = wget(recv)
            {
                let b = crate::jlstring::byte_array(vm, &raw)?;
                vm.invoke_instance(os, "write", "([B)V", &[b])?;
                vm.invoke_instance(os, "flush", "()V", &[])?;
            }
            Value::Null
        }
        "writeBytes" if class == PS => {
            let raw = require_byte_array(vm, args.first())?;
            flush_writer(vm, recv, false, false)?;
            if let Some(WState {
                sink: Sink::Stream(os, _),
                closed: false,
                ..
            }) = wget(recv)
            {
                let b = crate::jlstring::byte_array(vm, &raw)?;
                vm.invoke_instance(os, "write", "([B)V", &[b])?;
                vm.invoke_instance(os, "flush", "()V", &[])?;
            }
            Value::Null
        }
        "write" => {
            let units: Vec<u16> = match sig {
                "(I)V" => vec![(int(args, 0) as u32 & 0xffff) as u16],
                "([C)V" => require_char_array(vm, args.first())?,
                "([CII)V" => {
                    let all = require_char_array(vm, args.first())?;
                    slice(&all, int(args, 1), int(args, 2))?
                }
                "(Ljava/lang/String;II)V" => {
                    let u: Vec<u16> = require_non_null_string(vm, args.first())?
                        .encode_utf16()
                        .collect();
                    slice(&u, int(args, 1), int(args, 2))?
                }
                _ => require_non_null_string(vm, args.first())?
                    .encode_utf16()
                    .collect(),
            };
            write_units(recv, &units)?;
            Value::Null
        }
        "append" => {
            let units: Vec<u16> = match sig {
                "(C)Ljava/io/Writer;"
                | "(C)Ljava/io/PrintWriter;"
                | "(C)Ljava/io/PrintStream;"
                | "(C)Ljava/io/StringWriter;"
                | "(C)Ljava/lang/Appendable;" => vec![(int(args, 0) as u32 & 0xffff) as u16],
                _ if args.len() == 3 => {
                    let u: Vec<u16> = text_of(vm, args.first())?.encode_utf16().collect();
                    let (a, b) = check_start_end(u.len(), int(args, 1), int(args, 2))?;
                    u[a..b].to_vec()
                }
                _ => text_of(vm, args.first())?.encode_utf16().collect(),
            };
            write_units(recv, &units)?;
            Value::Obj(recv)
        }
        "print" | "println" => {
            let mut units = print_units(vm, sig, args.first())?;
            if name == "println" {
                units.push(b'\n' as u16);
            }
            write_units(recv, &units)?;
            Value::Null
        }
        "printf" | "format" => {
            // printf(String, Object[]) or printf(Locale, String, Object[])
            let (fi, ai) = if sig.starts_with("(Ljava/util/Locale;") {
                (1, 2)
            } else {
                (0, 1)
            };
            let fmt = require_non_null_string(vm, args.get(fi))?;
            let fargs: Vec<Value> = match args.get(ai) {
                Some(Value::Obj(a)) => vm
                    .heap
                    .as_array_elems(*a)
                    .map(|(e, _)| e.to_vec())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let s = crate::jlstring::java_format(vm, &fmt, &fargs, ('.', ','))?;
            write_text(recv, &s)?;
            Value::Obj(recv)
        }
        "newLine" => {
            write_text(recv, "\n")?;
            Value::Null
        }
        "flush" => {
            flush_writer(vm, recv, false, false)?;
            Value::Null
        }
        "close" => {
            if class != SW && !wget(recv).is_some_and(|w| w.closed) {
                flush_writer(vm, recv, true, false)?;
            }
            Value::Null
        }
        "checkError" => {
            flush_writer(vm, recv, false, false)?;
            Value::Int(0)
        }
        "toString" if class == SW => {
            let t = wget(recv)
                .map(|w| String::from_utf16_lossy(&w.text))
                .unwrap_or_default();
            jstr(vm, t)?
        }
        "getBuffer" if class == SW => {
            let t = wget(recv)
                .map(|w| String::from_utf16_lossy(&w.text))
                .unwrap_or_default();
            let sb = vm
                .heap
                .alloc_instance("Ljava/lang/StringBuffer;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            let s_val = jstr(vm, t)?;
            let _ = crate::intrinsics::call_instance_intrinsic_mref(
                vm,
                "Ljava/lang/StringBuffer;",
                "Ljava/lang/StringBuffer;",
                "<init>",
                "(Ljava/lang/String;)V",
                sb,
                &[s_val],
            )?;
            Value::Obj(sb)
        }
        "getEncoding" => match wget(recv).map(|w| w.sink) {
            Some(Sink::Stream(_, cs)) => jstr(vm, cs.replace('-', ""))?,
            _ => Value::Null,
        },
        _ => return Ok(None),
    };
    let stream_write = class == PS
        && matches!(
            name,
            "write" | "append" | "print" | "println" | "printf" | "format" | "newLine"
        )
        && !matches!(sig, "([B)V" | "([BII)V");
    let printwriter_autoflush =
        class == PW && name == "println" && wget(recv).is_some_and(|w| w.autoflush);
    if stream_write || printwriter_autoflush {
        flush_writer(vm, recv, false, name != "println" && name != "newLine")?;
    }
    Ok(Some(if ret_self && matches!(out, Value::Null) {
        Value::Obj(recv)
    } else {
        out
    }))
}

// ── readers ─────────────────────────────────────────────────────────────

fn rget(r: ObjRef) -> Option<RState> {
    READERS_ST.with(|m| m.borrow().get(&r).cloned())
}
fn rput(r: ObjRef, st: RState) {
    READERS_ST.with(|m| m.borrow_mut().insert(r, st));
}

/// Drains the reader's source into `chars` on first use.
fn load(vm: &mut Engine, recv: ObjRef) -> Result<RState, VmExit> {
    let mut st =
        rget(recv).ok_or_else(|| exc("Ljava/io/IOException;", "reader not initialised"))?;
    if st.closed {
        return Err(exc("Ljava/io/IOException;", "Stream closed"));
    }
    if st.loaded {
        return Ok(st);
    }
    match st.source.clone() {
        Source::Text => {}
        Source::Reader(inner) => {
            if rget(inner).is_some() {
                let ist = load(vm, inner)?;
                st.chars = ist.chars[ist.pos..].to_vec();
                let mut ist2 = ist;
                ist2.pos = ist2.chars.len();
                rput(inner, ist2);
            } else {
                // DEX Reader: pull through read(char[],int,int) reusing one buffer.
                let arr = vm
                    .heap
                    .alloc_array_zeroed(ElemKind::Int, 4096, "[C")
                    .map_err(crate::err::oom_throwable)?;
                loop {
                    let n = vm
                        .invoke_instance(
                            inner,
                            "read",
                            "([CII)I",
                            &[Value::Obj(arr), Value::Int(0), Value::Int(4096)],
                        )?
                        .as_int()
                        .unwrap_or(-1);
                    if n <= 0 {
                        break;
                    }
                    let got = chars_of(vm, Some(&Value::Obj(arr)));
                    let take = (n as usize).min(got.len());
                    if st
                        .chars
                        .len()
                        .checked_add(take)
                        .map_or(true, |total| total > MAX_STREAM_UNITS)
                    {
                        return Err(exc(
                            "Ljava/io/IOException;",
                            "Reader stream exceeds maximum buffered length",
                        ));
                    }
                    st.chars.extend_from_slice(&got[..take]);
                    if let Ok(elems) = vm.heap.array_elems_mut(arr) {
                        for slot in elems.iter_mut().take(take) {
                            *slot = Value::Int(0);
                        }
                    }
                }
            }
        }
        Source::Stream(is, cs) => {
            let mut bytes = Vec::new();
            let arr = vm
                .heap
                .alloc_array_zeroed(ElemKind::Int, 8192, "[B")
                .map_err(crate::err::oom_throwable)?;
            loop {
                let n = vm
                    .invoke_instance(
                        is,
                        "read",
                        "([BII)I",
                        &[Value::Obj(arr), Value::Int(0), Value::Int(8192)],
                    )?
                    .as_int()
                    .unwrap_or(-1);
                if n <= 0 {
                    break;
                }
                let got = bytes_of(vm, Some(&Value::Obj(arr)));
                let take = (n as usize).min(got.len());
                if bytes
                    .len()
                    .checked_add(take)
                    .map_or(true, |total| total > MAX_STREAM_UNITS)
                {
                    return Err(exc(
                        "Ljava/io/IOException;",
                        "InputStream exceeds maximum buffered length",
                    ));
                }
                bytes.extend_from_slice(&got[..take]);
                if let Ok(elems) = vm.heap.array_elems_mut(arr) {
                    for slot in elems.iter_mut().take(take) {
                        *slot = Value::Int(0);
                    }
                }
            }
            st.chars = crate::jlstring::decode(&bytes, &cs)
                .encode_utf16()
                .collect();
        }
    }
    st.loaded = true;
    rput(recv, st.clone());
    Ok(st)
}

fn reader(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let source = match class {
            SR => Source::Text,
            FR => {
                if obj(args, 0).is_none() {
                    return Err(exc("Ljava/lang/NullPointerException;", "file == null"));
                }
                let fis = vm
                    .heap
                    .alloc_instance("Ljava/io/FileInputStream;".to_string(), Vec::new())
                    .map_err(crate::err::oom_throwable)?;
                let ctor_sig = if sig.starts_with("(Ljava/io/File;") {
                    "(Ljava/io/File;)V"
                } else {
                    "(Ljava/lang/String;)V"
                };
                crate::jfile::instance_intrinsic(
                    vm,
                    "Ljava/io/FileInputStream;",
                    "<init>",
                    ctor_sig,
                    fis,
                    &[args.first().cloned().unwrap_or(Value::Null)],
                )?;
                Source::Stream(fis, "UTF-8".into())
            }
            _ => {
                let Some(src) = obj(args, 0) else {
                    return Err(exc("Ljava/lang/NullPointerException;", "in == null"));
                };
                let sc = class_of(vm, src);
                if class == BR || READERS.contains(&sc.as_str()) || vm.cp.is_subtype(&sc, READER) {
                    Source::Reader(src)
                } else {
                    let cs = if args.len() > 1 {
                        crate::jlstring::charset_name(vm, args.get(1))?
                    } else {
                        "UTF-8".into()
                    };
                    Source::Stream(src, cs)
                }
            }
        };
        let (chars, loaded) = match source {
            Source::Text => (
                require_non_null_string(vm, args.first())?
                    .encode_utf16()
                    .collect(),
                true,
            ),
            _ => (Vec::new(), false),
        };
        rput(
            recv,
            RState {
                source,
                loaded,
                chars,
                pos: 0,
                mark: 0,
                closed: false,
            },
        );
        return Ok(Some(Value::Null));
    }
    if rget(recv).is_none() {
        return Ok(None);
    }
    if name == "close" {
        if let Some(mut st) = rget(recv) {
            if !st.closed {
                st.closed = true;
                let src = st.source.clone();
                rput(recv, st);
                match src {
                    Source::Reader(r) | Source::Stream(r, _) => {
                        vm.invoke_instance(r, "close", "()V", &[])?;
                    }
                    Source::Text => {}
                }
            }
        }
        return Ok(Some(Value::Null));
    }
    if name == "markSupported" {
        return Ok(Some(Value::Int(1)));
    }
    if name == "getEncoding" {
        return match rget(recv) {
            Some(st) if !st.closed => match st.source {
                Source::Stream(_, cs) => Ok(Some(jstr(vm, cs.replace('-', ""))?)),
                _ => Ok(Some(Value::Null)),
            },
            _ => Ok(Some(Value::Null)),
        };
    }
    let mut st = load(vm, recv)?;
    let n = st.chars.len();
    let out = match (name, sig) {
        ("read", "()I") => {
            if st.pos < n {
                st.pos += 1;
                Value::Int(st.chars[st.pos - 1] as i32)
            } else {
                Value::Int(-1)
            }
        }
        ("read", "([C)I") | ("read", "([CII)I") => {
            let Some(arr) = obj(args, 0) else {
                return Err(exc("Ljava/lang/NullPointerException;", "cbuf"));
            };
            let cap = vm
                .heap
                .as_array_elems(arr)
                .map(|(e, _)| e.len())
                .unwrap_or(0);
            let (off, len) = if sig == "([C)I" {
                (0, cap)
            } else {
                check_offset_len(cap, int(args, 1), int(args, 2))?
            };
            if len == 0 {
                Value::Int(0)
            } else if st.pos >= n {
                Value::Int(-1)
            } else {
                let k = len.min(n - st.pos);
                let elems = vm
                    .heap
                    .array_elems_mut(arr)
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
                for i in 0..k {
                    elems[off + i] = Value::Int(st.chars[st.pos + i] as i32);
                }
                st.pos += k;
                Value::Int(k as i32)
            }
        }
        ("readLine", _) => {
            if st.pos >= n {
                Value::Null
            } else {
                let start = st.pos;
                while st.pos < n
                    && st.chars[st.pos] != b'\n' as u16
                    && st.chars[st.pos] != b'\r' as u16
                {
                    st.pos += 1;
                }
                let line = String::from_utf16_lossy(&st.chars[start..st.pos]);
                if st.pos < n && st.chars[st.pos] == b'\r' as u16 {
                    st.pos += 1;
                }
                if st.pos < n
                    && st.chars[st.pos] == b'\n' as u16
                    && (st.pos == 0 || st.chars[st.pos - 1] != b'\n' as u16 || st.pos == start)
                {
                    st.pos += 1;
                }
                jstr(vm, line)?
            }
        }
        ("ready", _) => Value::Int((st.pos < n) as i32),
        ("skip", _) => {
            let want = match args.first() {
                Some(Value::Long(l)) => *l,
                _ => 0,
            };
            if want < 0 {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "skip value is negative",
                ));
            }
            let k = (want as usize).min(n - st.pos);
            st.pos += k;
            Value::Long(k as i64)
        }
        ("mark", _) => {
            if int(args, 0) < 0 {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "Read-ahead limit < 0",
                ));
            }
            st.mark = st.pos;
            Value::Null
        }
        ("reset", _) => {
            st.pos = st.mark;
            Value::Null
        }
        _ => return Ok(None),
    };
    rput(recv, st);
    Ok(Some(out))
}

// ── byte array streams ──────────────────────────────────────────────────

/// A host `ByteArrayInputStream` over `bytes` (raw resources, assets).
pub(crate) fn new_byte_input_stream(vm: &mut Engine, bytes: Vec<u8>) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(BAIS.to_string(), Vec::new())?;
    BYTES_ST.with(|m| {
        m.borrow_mut().insert(
            r,
            BState {
                bytes,
                pos: 0,
                mark: 0,
            },
        )
    });
    Ok(Value::Obj(r))
}

fn bytes_stream(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let get = || BYTES_ST.with(|m| m.borrow().get(&recv).cloned());
    let set = |st: BState| BYTES_ST.with(|m| m.borrow_mut().insert(recv, st));
    if name == "<init>" {
        let st = if class == BAIS {
            let all = require_byte_array(vm, args.first())?;
            let bytes = if args.len() == 3 {
                slice(&all, int(args, 1), int(args, 2))?
            } else {
                all
            };
            BState {
                bytes,
                pos: 0,
                mark: 0,
            }
        } else {
            BState::default()
        };
        set(st);
        return Ok(Some(Value::Null));
    }
    let Some(mut st) = get() else { return Ok(None) };
    let n = st.bytes.len();
    let out = match (class, name) {
        (BAOS, "write") => {
            match sig {
                "(I)V" => st.bytes.push(int(args, 0) as u8),
                "([B)V" => st.bytes.extend(require_byte_array(vm, args.first())?),
                _ => {
                    let all = require_byte_array(vm, args.first())?;
                    st.bytes.extend(slice(&all, int(args, 1), int(args, 2))?);
                }
            }
            Value::Null
        }
        (BAOS, "writeBytes") => {
            st.bytes.extend(require_byte_array(vm, args.first())?);
            Value::Null
        }
        (BAOS, "toByteArray") => crate::jlstring::byte_array(vm, &st.bytes)?,
        (BAOS, "toString") => {
            let cs = if args.is_empty() {
                "UTF-8".to_string()
            } else {
                crate::jlstring::charset_name(vm, args.first())?
            };
            jstr(vm, crate::jlstring::decode(&st.bytes, &cs))?
        }
        (BAOS, "size") => Value::Int(n as i32),
        (BAOS, "reset") => {
            st.bytes.clear();
            Value::Null
        }
        (BAOS, "writeTo") => {
            let Some(os) = obj(args, 0) else {
                return Err(exc("Ljava/lang/NullPointerException;", "out"));
            };
            let b = crate::jlstring::byte_array(vm, &st.bytes)?;
            vm.invoke_instance(os, "write", "([B)V", &[b])?;
            Value::Null
        }
        (_, "flush" | "close") => Value::Null,
        (BAIS, "read") => {
            if sig == "()I" {
                if st.pos < n {
                    st.pos += 1;
                    Value::Int(st.bytes[st.pos - 1] as i32)
                } else {
                    Value::Int(-1)
                }
            } else {
                let Some(arr) = obj(args, 0) else {
                    return Err(exc("Ljava/lang/NullPointerException;", "b"));
                };
                let cap = vm
                    .heap
                    .as_array_elems(arr)
                    .map(|(e, _)| e.len())
                    .unwrap_or(0);
                let (off, len) = if sig == "([B)I" {
                    (0, cap)
                } else {
                    check_offset_len(cap, int(args, 1), int(args, 2))?
                };
                if len == 0 {
                    Value::Int(0)
                } else if st.pos >= n {
                    Value::Int(-1)
                } else {
                    let k = len.min(n - st.pos);
                    let elems = vm
                        .heap
                        .array_elems_mut(arr)
                        .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
                    for i in 0..k {
                        elems[off + i] = Value::Int(st.bytes[st.pos + i] as i8 as i32);
                    }
                    st.pos += k;
                    Value::Int(k as i32)
                }
            }
        }
        (BAIS, "readAllBytes") => {
            let rest = st.bytes[st.pos..].to_vec();
            st.pos = n;
            crate::jlstring::byte_array(vm, &rest)?
        }
        (BAIS, "available") => Value::Int((n - st.pos) as i32),
        (BAIS, "skip") => {
            let want = match args.first() {
                Some(Value::Long(l)) => (*l).max(0) as usize,
                _ => 0,
            };
            let k = want.min(n - st.pos);
            st.pos += k;
            Value::Long(k as i64)
        }
        (BAIS, "markSupported") => Value::Int(1),
        (BAIS, "mark") => {
            st.mark = st.pos;
            Value::Null
        }
        (BAIS, "reset") => {
            st.pos = st.mark;
            Value::Null
        }
        _ => return Ok(None),
    };
    set(st);
    Ok(Some(out))
}

// ── abstract bases for DEX subclasses ───────────────────────────────────

fn dex_base(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (mref_class, name, sig) {
        (WRITER | READER | OUTPUT_STREAM | INPUT_STREAM, "<init>", _) => Ok(Some(Value::Null)),
        (FILTER_OS, "<init>", "(Ljava/io/OutputStream;)V") => {
            if let Some(out) = args.first() {
                let _ = vm.heap.put_field(recv, "out", out.clone());
            }
            Ok(Some(Value::Null))
        }
        (WRITER, "write", "(Ljava/lang/String;)V")
        | (WRITER, "write", "(Ljava/lang/String;II)V")
        | (WRITER, "write", "(I)V")
        | (WRITER, "append", _) => {
            let u: Vec<u16> = if sig == "(I)V" || sig.starts_with("(C)") {
                vec![(int(args, 0) as u32 & 0xffff) as u16]
            } else if name == "write" && sig == "(Ljava/lang/String;II)V" {
                let all: Vec<u16> = require_non_null_string(vm, args.first())?
                    .encode_utf16()
                    .collect();
                slice(&all, int(args, 1), int(args, 2))?
            } else if name == "write" {
                require_non_null_string(vm, args.first())?
                    .encode_utf16()
                    .collect()
            } else if args.len() == 3 {
                let all: Vec<u16> = text_of(vm, args.first())?.encode_utf16().collect();
                let (a, b) = check_start_end(all.len(), int(args, 1), int(args, 2))?;
                all[a..b].to_vec()
            } else {
                text_of(vm, args.first())?.encode_utf16().collect()
            };
            let arr = vm
                .heap
                .alloc_array(ElemKind::Int, u.len(), "[C")
                .map_err(crate::err::oom_throwable)?;
            *vm.heap
                .array_elems_mut(arr)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? =
                u.iter().map(|c| Value::Int(*c as i32)).collect();
            vm.invoke_instance(
                recv,
                "write",
                "([CII)V",
                &[Value::Obj(arr), Value::Int(0), Value::Int(u.len() as i32)],
            )?;
            Ok(Some(if name == "append" {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        (WRITER, "write", "([C)V") => {
            let len = require_char_array(vm, args.first())?.len() as i32;
            vm.invoke_instance(
                recv,
                "write",
                "([CII)V",
                &[args[0].clone(), Value::Int(0), Value::Int(len)],
            )?;
            Ok(Some(Value::Null))
        }
        (READER, "read", "()I") => {
            let arr = vm
                .heap
                .alloc_array_zeroed(ElemKind::Int, 1, "[C")
                .map_err(crate::err::oom_throwable)?;
            let n = vm
                .invoke_instance(
                    recv,
                    "read",
                    "([CII)I",
                    &[Value::Obj(arr), Value::Int(0), Value::Int(1)],
                )?
                .as_int()
                .unwrap_or(-1);
            Ok(Some(Value::Int(if n <= 0 {
                -1
            } else {
                chars_of(vm, Some(&Value::Obj(arr)))
                    .first()
                    .copied()
                    .unwrap_or(0) as i32
            })))
        }
        (READER, "read", "([C)I") => {
            let len = require_char_array(vm, args.first())?.len() as i32;
            Ok(Some(vm.invoke_instance(
                recv,
                "read",
                "([CII)I",
                &[args[0].clone(), Value::Int(0), Value::Int(len)],
            )?))
        }
        _ => Ok(None),
    }
}

fn buffered_output_stream(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let Some(out) = obj(args, 0) else {
            return Err(exc("Ljava/lang/NullPointerException;", "out"));
        };
        let _ = vm.heap.put_field(recv, "out", Value::Obj(out));
        let size = if args.len() > 1 { int(args, 1) } else { 8192 };
        if size <= 0 {
            return Err(exc(
                "Ljava/lang/IllegalArgumentException;",
                "Buffer size <= 0",
            ));
        }
        let _ = vm.heap.put_field(recv, "rd_buf_size", Value::Int(size));
        BYTES_ST.with(|m| m.borrow_mut().insert(recv, BState::default()));
        return Ok(Some(Value::Null));
    }
    let wrapped = match vm.heap.get_field(recv, "out") {
        Ok(Value::Obj(o)) => Some(o),
        _ => None,
    };
    let flush_buffer = |vm: &mut Engine| -> Result<(), VmExit> {
        let pending = BYTES_ST.with(|m| {
            let mut map = m.borrow_mut();
            let st = map.entry(recv).or_default();
            std::mem::take(&mut st.bytes)
        });
        if !pending.is_empty() {
            if let Some(out) = wrapped {
                let b = crate::jlstring::byte_array(vm, &pending)?;
                vm.invoke_instance(out, "write", "([B)V", &[b])?;
            }
        }
        Ok(())
    };
    match name {
        "write" => {
            let data = match sig {
                "(I)V" => vec![int(args, 0) as u8],
                "([B)V" => require_byte_array(vm, args.first())?,
                _ => {
                    let all = require_byte_array(vm, args.first())?;
                    slice(&all, int(args, 1), int(args, 2))?
                }
            };
            BYTES_ST.with(|m| m.borrow_mut().entry(recv).or_default().bytes.extend(data));
            let cap = match vm.heap.get_field(recv, "rd_buf_size") {
                Ok(Value::Int(v)) => v.max(1) as usize,
                _ => 8192,
            };
            let over = BYTES_ST.with(|m| {
                m.borrow()
                    .get(&recv)
                    .map(|st| st.bytes.len() >= cap)
                    .unwrap_or(false)
            });
            if over {
                flush_buffer(vm)?;
            }
            Ok(Some(Value::Null))
        }
        "flush" => {
            flush_buffer(vm)?;
            if let Some(out) = wrapped {
                vm.invoke_instance(out, "flush", "()V", &[])?;
            }
            Ok(Some(Value::Null))
        }
        "close" => {
            let r = flush_buffer(vm);
            if let Some(out) = wrapped {
                let _ = vm.invoke_instance(out, "close", "()V", &[]);
            }
            r?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if WRITERS.contains(&recv_class) {
        return writer(vm, recv_class, name, sig, recv, args);
    }
    if READERS.contains(&recv_class) {
        return reader(vm, recv_class, name, sig, recv, args);
    }
    if recv_class == BAOS || recv_class == BAIS {
        return bytes_stream(vm, recv_class, name, sig, recv, args);
    }
    if recv_class == BOS {
        return buffered_output_stream(vm, name, sig, recv, args);
    }
    // Base-class templates apply when the DEX receiver does not override.
    if matches!(
        mref_class,
        WRITER | READER | OUTPUT_STREAM | INPUT_STREAM | FILTER_OS
    ) && (name == "<init>" || vm.cp.resolve_method(recv_class, name, sig).is_none())
    {
        return dex_base(vm, mref_class, name, sig, recv, args);
    }
    Ok(None)
}
