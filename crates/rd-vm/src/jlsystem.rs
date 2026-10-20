//! `java.lang.System` essentials that real apps hit on their first frames:
//! `arraycopy`, `identityHashCode`, `nanoTime`, `currentTimeMillis` and
//! `lineSeparator`.
//!
//! **Time is virtual and deterministic.** Like `SystemClock` and the Looper,
//! `nanoTime` is the virtual clock (`HostState.clock_ms`) in nanoseconds.
//! `currentTimeMillis` is a fixed epoch plus the virtual clock. The epoch
//! defaults to 2026-01-01T00:00:00Z and can be overridden with
//! `RD_EPOCH_MS`. Runs are reproducible, and `wait` moves both clocks.
//!
//! **`arraycopy` follows the JDK contract.**
//! - A null source or destination throws NPE.
//! - A non-array argument, or primitive/reference or primitive-kind
//!   mismatch, throws ArrayStoreException.
//! - Negative or out-of-range positions throw
//!   ArrayIndexOutOfBoundsException.
//! - Overlapping copies within one array behave as if through a temporary
//!   buffer.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::value::Value;

const SYSTEM: &str = "Ljava/lang/System;";
/// 2026-01-01T00:00:00Z
pub const DEFAULT_EPOCH_MS: i64 = 1_767_225_600_000;

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}

pub(crate) fn epoch_ms() -> i64 {
    std::env::var("RD_EPOCH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_EPOCH_MS)
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == "Ljava/lang/Class;" && name == "forName" {
        return class_for_name(vm, sig, args);
    }
    if class != SYSTEM {
        return Ok(None);
    }
    Ok(Some(match (name, sig) {
        ("nanoTime", "()J") => Value::Long((vm.fw.clock() as i64).saturating_mul(1_000_000)),
        ("currentTimeMillis", "()J") => {
            Value::Long(epoch_ms().saturating_add(vm.fw.clock() as i64))
        }
        ("identityHashCode", "(Ljava/lang/Object;)I") => match args[0].as_ref()? {
            Some(r) => Value::Int(r as i32),
            None => Value::Int(0),
        },
        ("lineSeparator", "()Ljava/lang/String;") => {
            Value::Obj(crate::intrinsics::alloc_string(vm, "\n".to_string())?)
        }
        ("arraycopy", "(Ljava/lang/Object;ILjava/lang/Object;II)V") => {
            arraycopy(
                vm,
                &args[0],
                args[1].as_int()?,
                &args[2],
                args[3].as_int()?,
                args[4].as_int()?,
            )?;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

fn arraycopy(
    vm: &mut Engine,
    src: &Value,
    sp: i32,
    dst: &Value,
    dp: i32,
    len: i32,
) -> Result<(), VmExit> {
    const NPE: &str = "Ljava/lang/NullPointerException;";
    const ASE: &str = "Ljava/lang/ArrayStoreException;";
    const AIOOBE: &str = "Ljava/lang/ArrayIndexOutOfBoundsException;";
    let s = src.as_ref()?.ok_or_else(|| exc(NPE, "src == null"))?;
    let d = dst.as_ref()?.ok_or_else(|| exc(NPE, "dst == null"))?;
    let sd = vm.heap.class_of(s)?.to_string();
    let dd = vm.heap.class_of(d)?.to_string();
    if !sd.starts_with('[') {
        return Err(exc(ASE, format!("source of type {sd} is not an array")));
    }
    if !dd.starts_with('[') {
        return Err(exc(
            ASE,
            format!("destination of type {dd} is not an array"),
        ));
    }
    let s_prim = !matches!(sd.as_bytes().get(1), Some(b'L') | Some(b'['));
    let d_prim = !matches!(dd.as_bytes().get(1), Some(b'L') | Some(b'['));
    if (s_prim || d_prim) && sd != dd {
        return Err(exc(
            ASE,
            format!("{sd} and {dd} are incompatible array types"),
        ));
    }
    let slen = vm.heap.as_array_elems(s)?.0.len() as i64;
    let dlen = vm.heap.as_array_elems(d)?.0.len() as i64;
    let (sp64, dp64, len64) = (sp as i64, dp as i64, len as i64);
    if sp < 0 || dp < 0 || len < 0 || sp64 + len64 > slen || dp64 + len64 > dlen {
        return Err(exc(
            AIOOBE,
            format!("src.length={slen} srcPos={sp} dst.length={dlen} dstPos={dp} length={len}"),
        ));
    }
    if len == 0 {
        return Ok(());
    }
    let chunk: Vec<Value> = vm.heap.as_array_elems(s)?.0[sp as usize..(sp + len) as usize].to_vec();
    let out = vm.heap.array_elems_mut(d)?;
    out[dp as usize..(dp + len) as usize].clone_from_slice(&chunk);
    Ok(())
}

/// `Class.forName(name[, initialize, loader])`. The class exists if:
/// - the app DEX defines it;
/// - the host runtime models it (`classpath::host_class_known`); or
/// - it is platform API (`android.*`, `dalvik.*`).
///
/// Anything else throws `ClassNotFoundException`, which is exactly what
/// optional-dependency probes expect (kotlin-reflect, OkHttp platforms,
/// Conscrypt, …). With `initialize` (the default), `<clinit>` runs as on ART.
fn class_for_name(vm: &mut Engine, sig: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if sig != "(Ljava/lang/String;)Ljava/lang/Class;"
        && sig != "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;"
    {
        return Ok(None);
    }
    let name = match args.first() {
        Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
        _ => {
            return Err(exc(
                "Ljava/lang/NullPointerException;",
                "Class.forName(null)",
            ))
        }
    };
    let initialize = !matches!(args.get(1), Some(Value::Int(0)));
    let desc = if name.starts_with('[') {
        name.replace('.', "/")
    } else {
        format!("L{};", name.replace('.', "/"))
    };
    let in_dex = vm.cp.find_class(&desc).is_some();
    let known = in_dex
        || desc.starts_with('[')
        || crate::classpath::host_class_known(&desc)
        || desc.starts_with("Landroid/")
        || desc.starts_with("Ldalvik/");
    if !known {
        return Err(exc("Ljava/lang/ClassNotFoundException;", name));
    }
    if in_dex && initialize {
        vm.ensure_initialized(&desc)?;
    }
    Ok(Some(Value::Obj(vm.class_object_for(&desc)?)))
}

/// `ClassLoader.loadClass(name[, resolve])`: same resolution as
/// `Class.forName` (app DEX + host classes; `ClassNotFoundException`
/// otherwise). `getParent` is null (single app loader).
pub(crate) fn class_loader_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Class.getPackage / Package.getName: derived from the descriptor
    // Class annotations come from the DEX (jannotations.rs); host classes
    // and Method/Constructor report "absent".
    if mref_class == "Ljava/lang/Class;" {
        // Runtime-visible class annotations from the DEX (jannotations.rs).
        if let Some(Value::Obj(c)) = args.first() {
            if let Some(v) = crate::jannotations::class_intrinsic(vm, name, *c, &args[1..])? {
                return Ok(Some(v));
            }
        }
        match name {
            "getAnnotation" | "getDeclaredAnnotation" => return Ok(Some(Value::Null)),
            "isAnnotationPresent" => return Ok(Some(Value::Int(0))),
            "getAnnotations"
            | "getDeclaredAnnotations"
            | "getAnnotationsByType"
            | "getDeclaredAnnotationsByType" => {
                let a = crate::arrays::new_array(
                    vm,
                    "[Ljava/lang/annotation/Annotation;",
                    crate::heap::ElemKind::Obj,
                    Vec::new(),
                )?;
                return Ok(Some(a));
            }
            _ => {}
        }
    }
    if mref_class == "Ljava/lang/Class;" && name == "getPackage" {
        let desc = match args.first() {
            Some(Value::Obj(c)) => match vm.heap.get_field(*c, "class_name") {
                Ok(Value::Obj(s)) => vm.heap.as_str(s).map(str::to_string).unwrap_or_default(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let bin = desc
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', ".");
        let pkg = bin
            .rsplit_once('.')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        let r = vm
            .heap
            .alloc_instance("Ljava/lang/Package;".into(), Vec::new())?;
        let n = crate::intrinsics::alloc_string(vm, pkg)?;
        crate::err::put_field_result(vm.heap.put_field(r, "rd_name", Value::Obj(n)))?;
        return Ok(Some(Value::Obj(r)));
    }
    if mref_class == "Ljava/lang/Package;" {
        return Ok(match (name, args.first()) {
            ("getName" | "toString", Some(Value::Obj(p))) => {
                Some(vm.heap.get_field(*p, "rd_name").unwrap_or(Value::Null))
            }
            (
                "getImplementationVersion" | "getSpecificationVersion" | "getImplementationVendor",
                _,
            ) => Some(Value::Null),
            _ => None,
        });
    }
    if !matches!(
        mref_class,
        "Ljava/lang/ClassLoader;"
            | "Ldalvik/system/BaseDexClassLoader;"
            | "Ldalvik/system/PathClassLoader;"
    ) {
        return Ok(None);
    }
    match (name, sig) {
        (
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;" | "(Ljava/lang/String;Z)Ljava/lang/Class;",
        ) => class_for_name(vm, "(Ljava/lang/String;)Ljava/lang/Class;", &args[..1]),
        ("getParent", _) => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}
