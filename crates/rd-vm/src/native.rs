//! Dalvik ↔ isolated native JNI bridge. No unsafe code or host pointers.
use crate::heap::{ElemKind, ObjRef};
use crate::{Engine, Throwable, Value, VmExit};
use rd_jni::{Handle, References};
use rd_ndk::elf::Abi;
use rd_ndk::runtime::{Argument, Return, Runtime, Trap, ENV, JAVA_VM};
use rd_ndk::{RdError, Result};
use std::collections::BTreeMap;
#[derive(Debug)]
pub struct NativeState {
    runtime: Runtime,
    refs: References,
    registered: BTreeMap<(String, String, String), u64>,
    buffers: BTreeMap<u64, (ObjRef, char)>,
    onloads: std::collections::BTreeSet<String>,
}
fn ndk(e: impl std::fmt::Debug) -> RdError {
    RdError::new("JNI_ERROR", format!("{e:?}"))
}
fn vmerr(e: RdError) -> VmExit {
    VmExit::Error(crate::err::RdError {
        code: e.code,
        cause: e.cause,
        suggestion: e.suggestion,
        module_id: e.module_id,
    })
}
fn jerr(e: rd_jni::RdError) -> RdError {
    RdError::new(&e.code, e.cause)
}
fn hobj(refs: &References, h: u64) -> Result<ObjRef> {
    match refs.get(h).map_err(jerr)? {
        Handle::Object(r) => Ok(*r),
        _ => Err(ndk("JNI reference is not an object")),
    }
}
fn hclass(refs: &References, h: u64) -> Result<String> {
    match refs.get(h).map_err(jerr)? {
        Handle::Class(c) => Ok(c.clone()),
        _ => Err(ndk("JNI reference is not a class")),
    }
}
fn object(refs: &mut References, r: ObjRef) -> Result<u64> {
    refs.insert(Handle::Object(r)).map_err(jerr)
}
fn to_native(refs: &mut References, v: &Value) -> Result<u64> {
    Ok(match v {
        Value::Null => 0,
        Value::Obj(r) => object(refs, *r)?,
        Value::Int(n) => *n as u64,
        Value::Long(n) => *n as u64,
        Value::Float(f) => f.to_bits() as u64,
        Value::Double(f) => f.to_bits(),
        _ => return Err(ndk("unsupported JNI value")),
    })
}
/// Converts a native register/jvalue word to a VM value. `narrow` (32-bit
/// ARM) drops the upper half for every non-64-bit type: a jvalue's unused
/// bytes and r1 of a 32-bit return are not part of the value.
fn from_native(refs: &References, b: u64, ty: &str, narrow: bool) -> Result<Value> {
    let b = if narrow && !matches!(ty, "J" | "D") {
        b & 0xffff_ffff
    } else {
        b
    };
    Ok(match ty.as_bytes().first() {
        Some(b'V') => Value::Null,
        // jboolean is one byte; the rest of a jvalue is unspecified
        Some(b'Z') => Value::Int((b as u8 != 0) as i32),
        Some(b'B') => Value::Int(b as i8 as i32),
        Some(b'C') => Value::Int(b as u16 as i32),
        Some(b'S') => Value::Int(b as i16 as i32),
        Some(b'I') => Value::Int(b as i32),
        Some(b'J') => Value::Long(b as i64),
        Some(b'F') => Value::Float(f32::from_bits(b as u32)),
        Some(b'D') => Value::Double(f64::from_bits(b)),
        Some(b'L' | b'[') => {
            if b == 0 {
                Value::Null
            } else {
                Value::Obj(hobj(refs, b)?)
            }
        }
        _ => return Err(ndk("invalid JNI return type")),
    })
}
fn ensure(vm: &mut Engine) -> std::result::Result<(), VmExit> {
    if vm.native_active {
        return Err(vmerr(RdError::new(
            "NATIVE_REENTRANT",
            "nested native invocation from a JNI callback is not yet supported",
        )));
    }
    if vm.native.is_none() {
        let apk = vm.resources()?;
        // RD_NATIVE_ABI (x86_64 | arm64-v8a | armeabi-v7a) pins the ABI, e.g.
        // to run an APK's ARM libraries; default preference is x86_64 first.
        let wanted = std::env::var("RD_NATIVE_ABI")
            .ok()
            .filter(|w| !w.is_empty());
        let abi = [Abi::X86_64, Abi::Arm64, Abi::Arm]
            .into_iter()
            .filter(|a| wanted.as_deref().map_or(true, |w| w == a.android_name()))
            .find(|a| apk.abis.iter().any(|s| s == a.android_name()))
            .ok_or_else(|| {
                vmerr(RdError::new(
                    "UNSUPPORTED_ABI",
                    "APK contains no supported native ABI",
                ))
            })?;
        vm.native = Some(NativeState {
            runtime: Runtime::new(abi).map_err(vmerr)?,
            refs: References::default(),
            registered: BTreeMap::new(),
            buffers: BTreeMap::new(),
            onloads: Default::default(),
        });
    }
    Ok(())
}
/// System.loadLibrary and System.load both resolve only APK-owned libraries.
pub fn load(vm: &mut Engine, name: &str) -> std::result::Result<(), VmExit> {
    ensure(vm)?;
    let library = if name.ends_with(".so") {
        name.rsplit('/').next().unwrap_or(name).to_string()
    } else {
        if name.contains('/') || name.is_empty() {
            return Err(
                Throwable::new("Ljava/lang/UnsatisfiedLinkError;", "invalid library name").into(),
            );
        }
        format!("lib{name}.so")
    };
    let mut state = vm.native.take().unwrap();
    let mut available = BTreeMap::new();
    let apk = vm.resources()?;
    let prefix = format!("lib/{}/", state.runtime.abi.android_name());
    for n in &apk.native_libs {
        if let Some(short) = n.strip_prefix(&prefix) {
            available.insert(
                short.to_string(),
                apk.zip.read(n).map_err(|e| vmerr(ndk(e)))?,
            );
        }
    }
    vm.native_active = true;
    let result = (|| -> Result<()> {
        let initializers = state.runtime.load(&library, &available)?;
        for address in initializers {
            run(vm, &mut state, address, &[])?;
        }
        if state.onloads.insert(library.clone()) {
            if let Some(address) = state.runtime.library_symbol(&library, "JNI_OnLoad") {
                let version = run(
                    vm,
                    &mut state,
                    address,
                    &[Argument::pointer(JAVA_VM), Argument::pointer(0)],
                )?
                .bits as u32;
                if !matches!(
                    version,
                    0x00010001 | 0x00010002 | 0x00010004 | 0x00010006 | 0x00010008
                ) {
                    return Err(RdError::new(
                        "JNI_VERSION",
                        format!("JNI_OnLoad returned {version:#x}"),
                    ));
                }
            }
        }
        Ok(())
    })();
    vm.native_active = false;
    // A failed loader is discarded: half-relocated images must never be reused.
    if result.is_ok() {
        vm.native = Some(state);
    }
    result.map_err(vmerr)
}
pub fn invoke(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    is_static: bool,
    slots: &[Value],
) -> std::result::Result<Value, VmExit> {
    vm.with_host_roots(|vm| invoke_rooted(vm, class, name, sig, is_static, slots))
}

fn invoke_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    is_static: bool,
    slots: &[Value],
) -> std::result::Result<Value, VmExit> {
    ensure(vm)?;
    let mut state = vm.native.take().unwrap();
    vm.native_active = true;
    let result = (|| -> Result<Value> {
        let key = (class.to_string(), name.to_string(), sig.to_string());
        let symbols = rd_jni::symbols(class, name, sig).map_err(jerr)?;
        let address = state
            .registered
            .get(&key)
            .copied()
            .or_else(|| state.runtime.symbol(&symbols[1]))
            .or_else(|| state.runtime.symbol(&symbols[0]))
            .ok_or_else(|| {
                RdError::new(
                    "NATIVE_UNRESOLVED_SYMBOL",
                    format!("{class}->{name}{sig}; loadLibrary must run first"),
                )
            })?;
        state.refs.push().map_err(jerr)?;
        let result = (|| {
            let (params, ret) = rd_jni::descriptor(sig).map_err(jerr)?;
            let mut args = vec![Argument::pointer(ENV)];
            if is_static {
                args.push(Argument::pointer(
                    state
                        .refs
                        .insert(Handle::Class(class.into()))
                        .map_err(jerr)?,
                ));
            } else {
                let recv = slots
                    .first()
                    .ok_or_else(|| ndk("missing native receiver"))?;
                args.push(Argument::pointer(to_native(&mut state.refs, recv)?));
            }
            let mut slot = if is_static { 0 } else { 1 };
            for ty in params {
                let v = slots
                    .get(slot)
                    .ok_or_else(|| ndk("native argument missing"))?;
                args.push(Argument {
                    kind: ty.chars().next().unwrap(),
                    bits: to_native(&mut state.refs, v)?,
                });
                slot += if ty == "J" || ty == "D" { 2 } else { 1 };
            }
            let r = run(vm, &mut state, address, &args)?;
            let bits = if ret == "F" || ret == "D" {
                r.fp_bits.unwrap_or(r.bits)
            } else {
                r.bits
            };
            let narrow = state.runtime.abi == Abi::Arm;
            from_native(&state.refs, bits, &ret, narrow)
        })();
        let _ = state.refs.pop(0);
        result
    })();
    let pending = state.refs.pending.take();
    vm.native_active = false;
    vm.native = Some(state);
    if let Some((class, msg)) = pending {
        return Err(Throwable::new(&class, msg).into());
    }
    result.map_err(vmerr)
}
fn run(vm: &mut Engine, s: &mut NativeState, address: u64, args: &[Argument]) -> Result<Return> {
    let NativeState {
        runtime,
        refs,
        registered,
        buffers,
        ..
    } = s;
    runtime.call(
        address,
        args,
        vm.fuel_remaining().min(5_000_000),
        |rt, t| bridge(vm, rt, refs, registered, buffers, t),
    )
}
fn bridge(
    vm: &mut Engine,
    rt: &mut Runtime,
    refs: &mut References,
    registered: &mut BTreeMap<(String, String, String), u64>,
    buffers: &mut BTreeMap<u64, (ObjRef, char)>,
    t: &Trap,
) -> Result<Return> {
    let a = |i: usize| t.args.get(i).copied().unwrap_or(0);
    let text = |rt: &mut Runtime, p: u64| -> Result<String> {
        rd_jni::decode_modified_utf8(&rt.c_string(p)?).map_err(jerr)
    };
    let bits = match t.slot {
        4 => 0x00010006, // GetVersion
        6 => {
            let n = text(rt, a(1))?;
            refs.insert(Handle::Class(if n.starts_with('[') {
                n
            } else {
                format!("L{n};")
            }))
            .map_err(jerr)?
        }
        10 => {
            let c = hclass(refs, a(1))?;
            let p = vm.cp.superclass_of(&c);
            match p {
                Some(p) => refs.insert(Handle::Class(p)).map_err(jerr)?,
                None => 0,
            }
        }
        11 => {
            let c = hclass(refs, a(1))?;
            let d = hclass(refs, a(2))?;
            vm.cp.is_subtype(&c, &d) as u64
        }
        13 => {
            let r = hobj(refs, a(1))?;
            let c = vm.heap.class_of(r).map_err(ndk)?.to_string();
            refs.pending = Some((c, String::new()));
            0
        }
        14 => {
            refs.pending = Some((hclass(refs, a(1))?, text(rt, a(2))?));
            0
        }
        15 => {
            if let Some((c, msg)) = refs.pending.clone() {
                let m = vm.heap.alloc_string(msg).map_err(ndk)?;
                let r = vm
                    .heap
                    .alloc_instance(c, vec![("detailMessage".into(), Value::Obj(m))])
                    .map_err(ndk)?;
                object(refs, r)?
            } else {
                0
            }
        }
        16 => 0, // ExceptionDescribe only writes stderr on Android; no state change.
        17 => {
            refs.pending = None;
            0
        }
        19 => {
            refs.push().map_err(jerr)?;
            0
        }
        20 => refs.pop(a(1)).map_err(jerr)?,
        21 => refs.duplicate(a(1), true).map_err(jerr)?,
        22 => {
            refs.delete_global(a(1));
            0
        }
        23 => {
            refs.delete_local(a(1));
            0
        }
        24 => refs.same(a(1), a(2)).map_err(jerr)? as u64,
        25 => refs.duplicate(a(1), false).map_err(jerr)?,
        26 => {
            if a(1) as usize > References::MAX {
                (-1i64) as u64
            } else {
                0
            }
        }
        27 => {
            let c = hclass(refs, a(1))?;
            let r = vm.heap.alloc_instance(c, Vec::new()).map_err(ndk)?;
            object(refs, r)?
        }
        31 => {
            let r = hobj(refs, a(1))?;
            refs.insert(Handle::Class(vm.heap.class_of(r).map_err(ndk)?.into()))
                .map_err(jerr)?
        }
        32 => {
            if a(1) == 0 {
                1
            } else {
                let r = hobj(refs, a(1))?;
                let c = hclass(refs, a(2))?;
                vm.cp.is_subtype(vm.heap.class_of(r).map_err(ndk)?, &c) as u64
            }
        }
        33 | 113 => {
            let class = hclass(refs, a(1))?;
            let name = text(rt, a(2))?;
            let sig = text(rt, a(3))?;
            rd_jni::descriptor(&sig).map_err(jerr)?;
            if vm.cp.resolve_method(&class, &name, &sig).is_none() {
                refs.pending = Some((
                    "Ljava/lang/NoSuchMethodError;".into(),
                    format!("{class}->{name}{sig}"),
                ));
                0
            } else {
                refs.insert(Handle::Method {
                    class,
                    name,
                    sig,
                    is_static: t.slot == 113,
                })
                .map_err(jerr)?
            }
        }
        94 | 144 => {
            let class = hclass(refs, a(1))?;
            let name = text(rt, a(2))?;
            let sig = text(rt, a(3))?;
            refs.insert(Handle::Field {
                class,
                name,
                sig,
                is_static: t.slot == 144,
            })
            .map_err(jerr)?
        }
        95..=103 | 145..=153 => {
            let Handle::Field {
                class,
                name,
                is_static,
                ..
            } = refs.get(a(2)).map_err(jerr)?.clone()
            else {
                return Err(ndk("expected jfieldID"));
            };
            let v = if is_static {
                vm.statics
                    .get(&(class, name))
                    .cloned()
                    .unwrap_or(Value::Int(0))
            } else {
                vm.heap.get_field(hobj(refs, a(1))?, &name).map_err(ndk)?
            };
            let bits = to_native(refs, &v)?;
            return Ok(Return {
                bits,
                fp_bits: if matches!(v, Value::Float(_) | Value::Double(_)) {
                    Some(bits)
                } else {
                    None
                },
            });
        }
        104..=112 | 154..=162 => {
            let Handle::Field {
                class,
                name,
                sig,
                is_static,
            } = refs.get(a(2)).map_err(jerr)?.clone()
            else {
                return Err(ndk("expected jfieldID"));
            };
            let b = if rt.abi == Abi::Arm && (sig == "J" || sig == "D") {
                let mut i = 3;
                arm_wide(rt, t, &mut i)?
            } else if (sig == "F" || sig == "D") && rt.abi != Abi::Arm {
                t.fp.first().copied().unwrap_or(a(3))
            } else {
                rt.raw_argument(t, 3)?
            };
            let v = from_native(refs, b, &sig, rt.abi == Abi::Arm)?;
            if is_static {
                vm.statics.insert((class, name), v);
            } else {
                vm.heap
                    .put_field(hobj(refs, a(1))?, &name, v)
                    .map_err(|e| ndk(format!("{e:?}")))?;
            }
            0
        }
        163 => {
            let n = a(2) as usize;
            if n > 1024 * 1024 {
                return Err(ndk("NewString size exceeds budget"));
            }
            let b = rt.read(a(1), n * 2)?;
            let u: Vec<u16> = b
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16(&u).map_err(ndk)?;
            object(refs, vm.heap.alloc_string(s).map_err(ndk)?)?
        }
        164 => vm
            .heap
            .as_str(hobj(refs, a(1))?)
            .map_err(ndk)?
            .encode_utf16()
            .count() as u64,
        165 | 224 => {
            let r = hobj(refs, a(1))?;
            let s = vm.heap.as_str(r).map_err(ndk)?;
            let mut b = Vec::new();
            for c in s.encode_utf16() {
                b.extend(c.to_le_bytes());
            }
            b.extend([0, 0]);
            let p = rt.alloc_bytes(&b)?;
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        166 | 170 | 225 => 0, // returned string copies live for the bounded native arena.
        167 => {
            let s = text(rt, a(1))?;
            object(refs, vm.heap.alloc_string(s).map_err(ndk)?)?
        }
        168 => rd_jni::encode_modified_utf8(vm.heap.as_str(hobj(refs, a(1))?).map_err(ndk)?).len()
            as u64,
        169 => {
            let mut b =
                rd_jni::encode_modified_utf8(vm.heap.as_str(hobj(refs, a(1))?).map_err(ndk)?);
            b.push(0);
            let p = rt.alloc_bytes(&b)?;
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        171 => vm
            .heap
            .as_array_elems(hobj(refs, a(1))?)
            .map_err(ndk)?
            .0
            .len() as u64,
        172 => {
            let n = a(1) as i32;
            if !(0..=1_000_000).contains(&n) {
                return Err(ndk("object array length exceeds budget"));
            }
            let c = hclass(refs, a(2))?;
            let init = from_native(refs, a(3), &c, rt.abi == Abi::Arm)?;
            let r = vm
                .heap
                .alloc_array(ElemKind::Obj, n as usize, &c)
                .map_err(ndk)?;
            vm.heap
                .array_elems_mut(r)
                .map_err(ndk)?
                .resize(n as usize, init);
            object(refs, r)?
        }
        173 => {
            let r = hobj(refs, a(1))?;
            let v = vm
                .heap
                .as_array_elems(r)
                .map_err(ndk)?
                .0
                .get(a(2) as usize)
                .ok_or_else(|| ndk("object array index out of bounds"))?
                .clone();
            to_native(refs, &v)?
        }
        174 => {
            let r = hobj(refs, a(1))?;
            let v = if a(3) == 0 {
                Value::Null
            } else {
                Value::Obj(hobj(refs, a(3))?)
            };
            let elems = vm.heap.array_elems_mut(r).map_err(ndk)?;
            *elems
                .get_mut(a(2) as usize)
                .ok_or_else(|| ndk("object array index out of bounds"))? = v;
            0
        }
        175..=182 => {
            let k = primitive(t.slot - 175);
            let n = a(1) as i32;
            if !(0..=1_000_000).contains(&n) {
                return Err(ndk("primitive array length exceeds budget"));
            }
            let desc = format!("[{k}");
            let r = vm
                .heap
                .alloc_array(ElemKind::from_array_desc(&desc).unwrap(), n as usize, &desc)
                .map_err(ndk)?;
            let zero = match k {
                'J' => Value::Long(0),
                'F' => Value::Float(0.0),
                'D' => Value::Double(0.0),
                _ => Value::Int(0),
            };
            vm.heap
                .array_elems_mut(r)
                .map_err(ndk)?
                .resize(n as usize, zero);
            object(refs, r)?
        }
        183..=190 => {
            let k = primitive(t.slot - 183);
            let r = hobj(refs, a(1))?;
            let b = array_bytes(vm, r, k, 0, usize::MAX)?;
            let p = rt.alloc_bytes(&b)?;
            buffers.insert(p, (r, k));
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        191..=198 => {
            let (r, k) = buffers
                .get(&a(2))
                .copied()
                .ok_or_else(|| ndk("array release pointer was not acquired"))?;
            if r != hobj(refs, a(1))? {
                return Err(ndk("array release reference mismatch"));
            }
            if a(3) != 2 {
                let n = vm.heap.as_array_elems(r).map_err(ndk)?.0.len();
                let b = rt.read(a(2), n * width(k))?;
                write_array(vm, r, k, 0, &b)?;
            }
            if a(3) != 1 {
                buffers.remove(&a(2));
            }
            0
        }
        199..=206 => {
            let k = primitive(t.slot - 199);
            let r = hobj(refs, a(1))?;
            let b = array_bytes(vm, r, k, a(2) as usize, a(3) as usize)?;
            let p = rt.raw_argument(t, 4)?;
            rt.write(p, &b)?;
            0
        }
        207..=214 => {
            let k = primitive(t.slot - 207);
            let n = a(3) as usize;
            if n > 1_000_000 {
                return Err(ndk("array region exceeds budget"));
            }
            let p = rt.raw_argument(t, 4)?;
            let b = rt.read(p, n * width(k))?;
            write_array(vm, hobj(refs, a(1))?, k, a(2) as usize, &b)?;
            0
        }
        215 => {
            let class = hclass(refs, a(1))?;
            let n = a(3) as i32;
            if !(0..=4096).contains(&n) {
                return Err(ndk("RegisterNatives count invalid"));
            }
            let w = rt.abi.pointer_size() as u64;
            for i in 0..n as u64 {
                let p = a(2) + i * w * 3;
                let np = rt.read_word(p)?;
                let sp = rt.read_word(p + w)?;
                let func = rt.read_word(p + 2 * w)?;
                let name = text(rt, np)?;
                let sig = text(rt, sp)?;
                rd_jni::descriptor(&sig).map_err(jerr)?;
                if vm.cp.resolve_method(&class, &name, &sig).is_none() {
                    return Err(ndk(format!(
                        "RegisterNatives method absent {class}->{name}{sig}"
                    )));
                }
                registered.insert((class.clone(), name, sig), func);
            }
            0
        }
        216 => {
            let class = hclass(refs, a(1))?;
            registered.retain(|(c, _, _), _| c != &class);
            0
        }
        // MonitorEnter/MonitorExit: the VM is single-threaded, so a monitor is
        // always uncontended (JNI_OK).
        217 | 218 => 0,
        219 => {
            rt.write_word(a(1), JAVA_VM)?;
            0
        }
        // GetStringRegion(env, str, start, len, jchar* buf)
        220 => {
            let u: Vec<u16> = vm
                .heap
                .as_str(hobj(refs, a(1))?)
                .map_err(ndk)?
                .encode_utf16()
                .collect();
            let (start, n) = (a(2) as usize, a(3) as usize);
            let end = start
                .checked_add(n)
                .filter(|e| *e <= u.len())
                .ok_or_else(|| ndk("StringIndexOutOfBoundsException: string region"))?;
            let b: Vec<u8> = u[start..end].iter().flat_map(|c| c.to_le_bytes()).collect();
            let p = rt.raw_argument(t, 4)?;
            rt.write(p, &b)?;
            0
        }
        // GetStringUTFRegion(env, str, start, len, char* buf): `start`/`len`
        // count UTF-16 units; the output is modified UTF-8, NUL-terminated.
        221 => {
            let u: Vec<u16> = vm
                .heap
                .as_str(hobj(refs, a(1))?)
                .map_err(ndk)?
                .encode_utf16()
                .collect();
            let (start, n) = (a(2) as usize, a(3) as usize);
            let end = start
                .checked_add(n)
                .filter(|e| *e <= u.len())
                .ok_or_else(|| ndk("StringIndexOutOfBoundsException: string UTF region"))?;
            let sub = String::from_utf16_lossy(&u[start..end]);
            let mut b = rd_jni::encode_modified_utf8(&sub);
            b.push(0);
            let p = rt.raw_argument(t, 4)?;
            rt.write(p, &b)?;
            0
        }
        // GetPrimitiveArrayCritical: a bounded copy like Get<T>ArrayElements;
        // the element width comes from the runtime array descriptor.
        222 => {
            let r = hobj(refs, a(1))?;
            let desc = vm.heap.class_of(r).map_err(ndk)?;
            let k = desc
                .strip_prefix('[')
                .and_then(|d| d.chars().next())
                .filter(|c| "ZBCSIJFD".contains(*c) && desc.len() == 2)
                .ok_or_else(|| ndk("GetPrimitiveArrayCritical on a non-primitive array"))?;
            let b = array_bytes(vm, r, k, 0, usize::MAX)?;
            let p = rt.alloc_bytes(&b)?;
            buffers.insert(p, (r, k));
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        // ReleasePrimitiveArrayCritical(env, array, ptr, mode)
        223 => {
            let (r, k) = buffers
                .get(&a(2))
                .copied()
                .ok_or_else(|| ndk("critical release pointer was not acquired"))?;
            if r != hobj(refs, a(1))? {
                return Err(ndk("critical release reference mismatch"));
            }
            if a(3) != 2 {
                let n = vm.heap.as_array_elems(r).map_err(ndk)?.0.len();
                let b = rt.read(a(2), n * width(k))?;
                write_array(vm, r, k, 0, &b)?;
            }
            if a(3) != 1 {
                buffers.remove(&a(2));
            }
            0
        }
        // NewWeakGlobalRef / DeleteWeakGlobalRef: no GC, so a weak global is
        // a global that is never cleared.
        226 => refs.duplicate(a(1), true).map_err(jerr)?,
        227 => {
            refs.delete_global(a(1));
            0
        }
        228 => refs.pending.is_some() as u64,
        304 | 306 | 307 => {
            rt.write_word(a(1), ENV)?;
            0
        }
        305 => 0,
        34..=93 | 114..=143 | 28..=30 => return call_java(vm, rt, refs, t),
        _ => {
            return Err(RdError::new(
                "NOT_IMPLEMENTED",
                format!("JNI function table slot {}", t.slot),
            ))
        }
    };
    Ok(Return {
        bits,
        fp_bits: None,
    })
}
/// ARM (AAPCS) 64-bit argument at raw index `i`: an even-aligned pair.
fn arm_wide(rt: &mut Runtime, t: &Trap, i: &mut usize) -> Result<u64> {
    *i += *i & 1;
    let lo = rt.raw_argument(t, *i)? & 0xffff_ffff;
    let hi = rt.raw_argument(t, *i + 1)? & 0xffff_ffff;
    *i += 2;
    Ok(lo | hi << 32)
}
fn primitive(i: usize) -> char {
    ['Z', 'B', 'C', 'S', 'I', 'J', 'F', 'D'][i]
}
fn width(k: char) -> usize {
    match k {
        'Z' | 'B' => 1,
        'C' | 'S' => 2,
        'I' | 'F' => 4,
        _ => 8,
    }
}
fn array_bytes(vm: &Engine, r: ObjRef, k: char, start: usize, n: usize) -> Result<Vec<u8>> {
    let (elems, _) = vm.heap.as_array_elems(r).map_err(ndk)?;
    let end = if n == usize::MAX {
        elems.len()
    } else {
        start
            .checked_add(n)
            .ok_or_else(|| ndk("array range overflow"))?
    };
    let slice = elems
        .get(start..end)
        .ok_or_else(|| ndk("array range out of bounds"))?;
    let mut b = Vec::new();
    for v in slice {
        let raw = match v {
            Value::Int(v) => *v as u64,
            Value::Long(v) => *v as u64,
            Value::Float(v) => v.to_bits() as u64,
            Value::Double(v) => v.to_bits(),
            _ => return Err(ndk("not a primitive array")),
        };
        b.extend(&raw.to_le_bytes()[..width(k)]);
    }
    Ok(b)
}
fn write_array(vm: &mut Engine, r: ObjRef, k: char, start: usize, b: &[u8]) -> Result<()> {
    let elems = vm.heap.array_elems_mut(r).map_err(ndk)?;
    let n = b.len() / width(k);
    let end = start
        .checked_add(n)
        .ok_or_else(|| ndk("array range overflow"))?;
    let slice = elems
        .get_mut(start..end)
        .ok_or_else(|| ndk("array range out of bounds"))?;
    for (v, b) in slice.iter_mut().zip(b.chunks_exact(width(k))) {
        let raw = b
            .iter()
            .enumerate()
            .fold(0u64, |v, (i, c)| v | u64::from(*c) << (i * 8));
        *v = match k {
            'J' => Value::Long(raw as i64),
            'F' => Value::Float(f32::from_bits(raw as u32)),
            'D' => Value::Double(f64::from_bits(raw)),
            'B' => Value::Int(raw as i8 as i32),
            'S' => Value::Int(raw as i16 as i32),
            _ => Value::Int(raw as i32),
        };
    }
    Ok(())
}
fn call_java(vm: &mut Engine, rt: &mut Runtime, refs: &mut References, t: &Trap) -> Result<Return> {
    let ctor = matches!(t.slot, 28..=30);
    let static_call = matches!(t.slot, 114..=143);
    let nonvirtual = matches!(t.slot, 64..=93);
    let id_index = if nonvirtual { 3 } else { 2 };
    let id = rt.raw_argument(t, id_index)?;
    let Handle::Method {
        class,
        name,
        sig,
        is_static,
    } = refs.get(id).map_err(jerr)?.clone()
    else {
        return Err(ndk("expected jmethodID"));
    };
    if is_static != static_call && !ctor {
        return Err(ndk("JNI static/instance method mismatch"));
    }
    let (params, ret) = rd_jni::descriptor(&sig).map_err(jerr)?;
    let base = if ctor {
        28
    } else if static_call {
        114
    } else if nonvirtual {
        64
    } else {
        34
    };
    let variant = (t.slot - base) % 3;
    // Call*MethodV: decode the guest va_list with the ABI's rules (C++ JNI
    // wrappers in jni.h always go through these entry points).
    let va_words = if variant == 1 {
        let va = rt.raw_argument(t, id_index + 1)?;
        Some(rt.va_list_args(va, &params)?)
    } else {
        None
    };
    let mut args = Vec::new();
    let mut raw_index = id_index + 1;
    let array = if variant == 2 {
        rt.raw_argument(t, raw_index)?
    } else {
        0
    };
    let mut fpi = 0;
    for (i, p) in params.iter().enumerate() {
        let b = if let Some(w) = &va_words {
            w[i]
        } else if variant == 2 {
            let b = rt.read(array + i as u64 * 8, 8)?;
            u64::from_le_bytes(b.try_into().unwrap())
        } else if rt.abi == Abi::Arm && (p == "J" || p == "D" || p == "F") {
            // AAPCS variadic: 64-bit values (and floats, promoted to double)
            // take an even-aligned register pair / 8-byte stack slot.
            let v = arm_wide(rt, t, &mut raw_index)?;
            if p == "F" {
                (f64::from_bits(v) as f32).to_bits() as u64
            } else {
                v
            }
        } else if (p == "F" || p == "D") && rt.abi != Abi::Arm {
            let v = t.fp.get(fpi).copied().unwrap_or(0);
            fpi += 1;
            if p == "F" {
                (f64::from_bits(v) as f32).to_bits() as u64
            } else {
                v
            }
        } else {
            let v = rt.raw_argument(t, raw_index)?;
            raw_index += 1;
            v
        };
        args.push(from_native(refs, b, p, rt.abi == Abi::Arm)?);
    }
    let result = if static_call {
        vm.invoke_static(&class, &name, &sig, &args)
    } else {
        let recv = if ctor {
            vm.heap
                .alloc_instance(class.clone(), Vec::new())
                .map_err(ndk)?
        } else {
            hobj(refs, rt.raw_argument(t, 1)?)?
        };
        let dispatch = if ctor || nonvirtual {
            class.clone()
        } else {
            vm.heap.class_of(recv).map_err(ndk)?.to_string()
        };
        let mut values = vec![Value::Obj(recv)];
        values.extend(vm.slot_form_public(&args, &params));
        let (d, def, m) = vm
            .cp
            .resolve_method(&dispatch, &name, &sig)
            .ok_or_else(|| ndk("callback method absent"))?;
        let v = vm.call(d, def, &m, values);
        if ctor {
            v.map(|_| Value::Obj(recv))
        } else {
            v
        }
    };
    match result {
        Ok(v) => {
            let b = to_native(refs, &v)?;
            Ok(Return {
                bits: b,
                fp_bits: if ret == "F" || ret == "D" {
                    Some(b)
                } else {
                    None
                },
            })
        }
        Err(VmExit::Exception(e)) => {
            refs.pending = Some((e.class, e.message.unwrap_or_default()));
            Ok(Return::default())
        }
        Err(e) => Err(ndk(format!("Java callback failed: {e:?}"))),
    }
}

impl Engine {
    pub fn native_report(&self) -> serde_json::Value {
        match &self.native {
            Some(s) => {
                serde_json::json!({"enabled":true,"abi":s.runtime.abi,"instructions":s.runtime.instructions,"libraries":s.runtime.loaded_libraries(),"arena_bytes":s.runtime.arena_bytes(),"registered_methods":s.registered.len(),"missing_imports":s.runtime.missing_imports,"lazy_imports":s.runtime.lazily_bound_imports().len(),"logs":s.runtime.logs})
            }
            None => serde_json::json!({"enabled":true,"libraries":[],"instructions":0}),
        }
    }
}
