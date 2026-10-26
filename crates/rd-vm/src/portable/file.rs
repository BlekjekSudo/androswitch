//! Persistent java.io primitives. All paths resolve through the same VFS used
//! by native imports; no separate in-memory filesystem or platform globals.
use crate::{intrinsics, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
};
fn exception(class: &str, message: impl Into<String>) -> VmExit {
    Throwable::new(class, message).into()
}
pub(super) fn io(error: as_platform::PlatformError) -> VmExit {
    exception("Ljava/io/IOException;", error.to_string())
}
pub(super) fn object(value: &Value) -> Result<ObjRef, VmExit> {
    value
        .as_ref()?
        .ok_or_else(|| exception("Ljava/lang/NullPointerException;", "null argument"))
}
pub(super) fn text(vm: &Engine, value: &Value) -> Result<String, VmExit> {
    Ok(vm.heap.as_str(object(value)?)?.to_string())
}
pub(super) fn path(vm: &Engine, value: &Value) -> Result<String, VmExit> {
    let r = object(value)?;
    if let Some(file) = vm.portable.files.get(&r) {
        Ok(file.path.clone())
    } else if vm.heap.class_of(r)? == crate::java_file::CLASS {
        Ok(crate::java_file::path_of(vm, r))
    } else {
        text(vm, value)
    }
}
pub(super) fn absolute(vm: &Engine, path: &str) -> Result<String, VmExit> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/data/user/0/{}/files/{path}", vm.portable.package)
    };
    as_platform::vfs::normalize(&path).map_err(io)
}
fn result_text(vm: &mut Engine, text: String) -> Result<Option<Value>, VmExit> {
    Ok(Some(Value::Obj(intrinsics::alloc_string(vm, text)?)))
}
pub(crate) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(value) = super::text_contracts::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    let scalar = match (class, name, sig) {
        ("Ljava/lang/Long;", "toString", "(J)Ljava/lang/String;") => {
            Some(args[0].as_long()?.to_string())
        }
        ("Ljava/lang/Boolean;", "toString", "(Z)Ljava/lang/String;") => Some(
            if args[0].as_int()? == 0 {
                "false"
            } else {
                "true"
            }
            .to_string(),
        ),
        _ => None,
    };
    if let Some(value) = scalar {
        return result_text(vm, value);
    }
    if class == "Ljava/lang/System;"
        && name == "getProperty"
        && matches!(
            sig,
            "(Ljava/lang/String;)Ljava/lang/String;"
                | "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"
        )
    {
        let key = text(vm, &args[0])?;
        let value = match key.as_str() {
            "file.separator" => Some("/".to_string()),
            "path.separator" => Some(":".to_string()),
            "line.separator" => Some("\n".to_string()),
            "java.io.tmpdir" => Some(format!("/data/user/0/{}/cache", vm.portable.package)),
            "user.dir" => Some(format!("/data/user/0/{}/files", vm.portable.package)),
            _ => match vm.framework_host() {
                Some(host) => host.system_property(&key)?,
                None => None,
            },
        };
        return match value {
            Some(value) => result_text(vm, value),
            None => Ok(Some(args.get(1).cloned().unwrap_or(Value::Null))),
        };
    }
    Ok(None)
}
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(value) = super::random_access::call(vm, class, name, sig, recv, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = super::text_contracts::call(vm, class, name, sig, recv, args)? {
        return Ok(Some(value));
    }
    if class == "Ljava/lang/String;" {
        if name == "getBytes" {
            return super::string_encoding::get_bytes(vm, recv, sig, args);
        }
        let s = vm.heap.as_str(recv)?.to_string();
        return match (name, sig) {
            ("isEmpty", "()Z") => Ok(Some(Value::Int(i32::from(s.is_empty())))),
            ("length", "()I") => Ok(Some(Value::Int(s.encode_utf16().count() as i32))),
            ("hashCode", "()I") => Ok(Some(Value::Int(
                s.encode_utf16().fold(0i32, |hash, unit| {
                    hash.wrapping_mul(31).wrapping_add(i32::from(unit))
                }),
            ))),
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = args[0].as_ref()?.and_then(|r| vm.heap.as_str(r).ok());
                Ok(Some(Value::Int((other == Some(s.as_str())) as i32)))
            }
            ("toString", "()Ljava/lang/String;") => Ok(Some(Value::Obj(recv))),
            _ => Ok(None),
        };
    }
    Ok(None)
}
