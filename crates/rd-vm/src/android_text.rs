//! `android.text.TextUtils` static helpers with AOSP semantics.
//!
//! A `CharSequence` argument is read through `object_to_string`. That covers
//! `String`, `StringBuilder` and app classes with `toString`. Compose and
//! AndroidX call these on hot paths (`isEmpty`, `equals`, `join`).

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
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
            Value::Int(t.map_or(true, |t| t.is_empty()) as i32)
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
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "concat(null)",
                ))
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
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "join(null)",
                ))
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

/// Legacy stack-summary API remains separate until real stack traces exist.
pub(crate) fn log_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if (class, name, sig)
        == (
            "Landroid/util/Log;",
            "getStackTraceString",
            "(Ljava/lang/Throwable;)Ljava/lang/String;",
        )
    {
        let value = match args[0].as_ref()? {
            Some(reference) => crate::intrinsics::object_to_string(vm, reference)?,
            None => String::new(),
        };
        return Ok(Some(s(vm, value)?));
    }
    let log_host = vm.framework_host();
    let output = vm.output.clone();
    crate::android_log::call(
        vm,
        class,
        name,
        sig,
        args,
        crate::intrinsics::object_to_string,
        |priority| priority >= 3,
        |line| output.write(log_host.as_deref(), 1, line),
    )
}
