//! `android.text.TextUtils` static helpers with AOSP semantics.
//!
//! A `CharSequence` argument is read through `object_to_string`. That covers
//! `String`, `StringBuilder` and app classes with `toString`. Compose and
//! AndroidX call these on hot paths (`isEmpty`, `equals`, `join`).

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::value::Value;

const TEXT_UTILS: &str = "Landroid/text/TextUtils;";

fn text(vm: &mut Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    match v.map(|v| v.as_ref()).transpose()?.flatten() {
        Some(r) => Ok(Some(crate::intrinsics::object_to_string(vm, r)?)),
        None => Ok(None),
    }
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn s(vm: &mut Engine, v: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, v)?))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != TEXT_UTILS {
        return Ok(None);
    }
    let v = match (name, sig) {
        ("isEmpty", "(Ljava/lang/CharSequence;)Z") => {
            let t = text(vm, args.first())?;
            Value::Int(t.is_none_or(|t| t.is_empty()) as i32)
        }
        ("equals", "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Z") => {
            let a = text(vm, args.first())?;
            let b = text(vm, args.get(1))?;
            Value::Int((a == b) as i32)
        }
        ("getTrimmedLength", "(Ljava/lang/CharSequence;)I") => {
            // AOSP: trims chars <= ' ' at both ends (UTF-16 units)
            let u = utf16(&text(vm, args.first())?.unwrap_or_default());
            let start = u.iter().position(|c| *c > 0x20).unwrap_or(u.len());
            let end = u.iter().rposition(|c| *c > 0x20).map_or(start, |e| e + 1);
            Value::Int((end.saturating_sub(start)) as i32)
        }
        ("substring", "(Ljava/lang/CharSequence;II)Ljava/lang/String;") => {
            let u = utf16(&text(vm, args.first())?.unwrap_or_default());
            let (a, b) = (args[1].as_int()?, args[2].as_int()?);
            if a < 0 || b < a || b as usize > u.len() {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("start {a}, end {b}, length {}", u.len()),
                )));
            }
            s(vm, String::from_utf16_lossy(&u[a as usize..b as usize]))?
        }
        ("concat", "([Ljava/lang/CharSequence;)Ljava/lang/CharSequence;") => {
            let arr = args[0].as_ref()?.ok_or_else(|| {
                VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", "concat(null)"))
            })?;
            let elems = vm.heap.as_array_elems(arr)?.0.to_vec();
            let mut out = String::new();
            for e in &elems {
                out.push_str(&text(vm, Some(e))?.unwrap_or_else(|| "null".into()));
            }
            s(vm, out)?
        }
        ("join", "(Ljava/lang/CharSequence;Ljava/lang/Iterable;)Ljava/lang/String;")
        | ("join", "(Ljava/lang/CharSequence;[Ljava/lang/Object;)Ljava/lang/String;") => {
            let delim = text(vm, args.first())?.unwrap_or_default();
            let src = args[1].as_ref()?.ok_or_else(|| {
                VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", "join(null)"))
            })?;
            let elems = if vm.heap.class_of(src)?.starts_with('[') {
                vm.heap.as_array_elems(src)?.0.to_vec()
            } else {
                match vm.fw.objects.get(&src) {
                    Some(HostObj::List { elems }) => elems.clone(),
                    _ => {
                        return Err(crate::err::not_implemented(
                            "TextUtils.join over a non-host Iterable",
                        )
                        .into())
                    }
                }
            };
            let mut parts = Vec::with_capacity(elems.len());
            for e in &elems {
                parts.push(text(vm, Some(e))?.unwrap_or_else(|| "null".into()));
            }
            s(vm, parts.join(&delim))?
        }
        ("getLayoutDirectionFromLocale", _) => Value::Int(0), // LTR (pt-BR/en)
        ("getCapsMode", "(Ljava/lang/CharSequence;II)I") => {
            // AOSP: CAP_MODE_SENTENCES/WORDS/CHARACTERS masked by reqModes;
            // at the start of the text every requested mode applies.
            let off = args[1].as_int()?;
            let req = args[2].as_int()?;
            if off <= 0 {
                Value::Int(req & 0x7000)
            } else {
                Value::Int(0)
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// `android.util.Log`: logcat-style lines (`D/Tag: message`) written to the
/// app output the MCP `logs` tool returns (same channel as `System.out`).
/// `isLoggable` is true from DEBUG up (the device default is INFO, but a
/// headless test runtime wants the detail). The return value is the number
/// of bytes written, as in AOSP.
pub(crate) fn log_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != "Landroid/util/Log;" {
        return Ok(None);
    }
    let level = match name {
        "v" => 'V',
        "d" => 'D',
        "i" => 'I',
        "w" => 'W',
        "e" => 'E',
        "wtf" => 'A',
        "isLoggable" => {
            let lvl = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(4);
            return Ok(Some(Value::Int((lvl >= 3) as i32)));
        }
        "getStackTraceString" => {
            let t = match args.first().map(|v| v.as_ref()).transpose()?.flatten() {
                Some(r) => crate::intrinsics::object_to_string(vm, r)?,
                None => String::new(),
            };
            return Ok(Some(s(vm, t)?));
        }
        "println" => {
            // println(int priority, String tag, String msg)
            let p = args.first().and_then(|v| v.as_int().ok()).unwrap_or(4);
            let tag = text(vm, args.get(1))?.unwrap_or_default();
            let msg = text(vm, args.get(2))?.unwrap_or_default();
            let c = ['V', 'V', 'V', 'D', 'I', 'W', 'E', 'A'][p.clamp(0, 7) as usize];
            let line = format!("{c}/{tag}: {msg}\n");
            crate::intrinsics::host_print(1, &line);
            return Ok(Some(Value::Int(line.len() as i32)));
        }
        _ => return Ok(None),
    };
    // (tag, msg) | (tag, msg, Throwable) | w/wtf(tag, Throwable)
    let tag = text(vm, args.first())?.unwrap_or_default();
    let mut msg = String::new();
    for a in &args[1.min(args.len())..] {
        if let Some(r) = a.as_ref()? {
            let piece = crate::intrinsics::object_to_string(vm, r)?;
            if !msg.is_empty() {
                msg.push('\n');
            }
            msg.push_str(&piece);
        }
    }
    let _ = sig;
    let line = format!("{level}/{tag}: {msg}\n");
    crate::intrinsics::host_print(1, &line);
    Ok(Some(Value::Int(line.len() as i32)))
}
