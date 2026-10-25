//! Persistent java.io primitives. All paths resolve through the same VFS used
//! by native imports; no separate in-memory filesystem or platform globals.
use super::FileState;
use crate::{err, intrinsics, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
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
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = args[0].as_ref()?.and_then(|r| vm.heap.as_str(r).ok());
                Ok(Some(Value::Int((other == Some(s.as_str())) as i32)))
            }
            ("toString", "()Ljava/lang/String;") => Ok(Some(Value::Obj(recv))),
            _ => Ok(None),
        };
    }
    let mode = match class {
        "Ljava/io/File;" => 0,
        "Ljava/io/FileInputStream;" => 1,
        "Ljava/io/FileOutputStream;" => 2,
        _ => return Ok(None),
    };
    if name == "<init>" {
        let mut value = match sig {
            "(Ljava/lang/String;)V" | "(Ljava/io/File;)V" => path(vm, &args[0])?,
            "(Ljava/lang/String;Z)V" | "(Ljava/io/File;Z)V" if mode == 2 => path(vm, &args[0])?,
            "(Ljava/lang/String;Ljava/lang/String;)V" | "(Ljava/io/File;Ljava/lang/String;)V"
                if mode == 0 =>
            {
                let parent = if matches!(args[0], Value::Null) {
                    String::new()
                } else {
                    path(vm, &args[0])?
                };
                let child = text(vm, &args[1])?;
                if parent.is_empty() {
                    child
                } else {
                    format!("{parent}/{child}")
                }
            }
            _ => return Ok(None),
        };
        if value.is_empty() {
            value = ".".to_string();
        }
        let value = absolute(vm, &value)?;
        vm.guest_filesystem()?.resolve(&value).map_err(io)?;
        let mut position = 0;
        if mode == 1
            && vm
                .guest_filesystem()?
                .stat(&value)
                .map_err(io)?
                .map_or(true, |m| m.directory)
        {
            return Err(exception("Ljava/io/FileNotFoundException;", &value));
        }
        if mode == 2 {
            let append = args.get(1).map(|a| a.as_int()).transpose()?.unwrap_or(0) != 0;
            if append {
                position = vm
                    .guest_filesystem()?
                    .stat(&value)
                    .map_err(io)?
                    .map(|m| m.length)
                    .unwrap_or(0);
                vm.guest_filesystem()?
                    .write_at(&value, position, &[], false)
                    .map_err(io)?;
            } else {
                vm.guest_filesystem()?
                    .write_at(&value, 0, &[], true)
                    .map_err(io)?;
            }
        }
        vm.portable.files.insert(
            recv,
            FileState {
                path: value,
                position,
                mode,
                closed: false,
            },
        );
        return Ok(Some(Value::Null));
    }
    let file = vm
        .portable
        .files
        .get(&recv)
        .cloned()
        .ok_or_else(|| err::vm_error("INVALID_FILE_STATE", class))?;
    if mode == 0 {
        return match (name, sig) {
            (
                "getPath" | "getAbsolutePath" | "getCanonicalPath" | "toString",
                "()Ljava/lang/String;",
            ) => result_text(vm, file.path),
            ("getParent", "()Ljava/lang/String;") => match file.path.rsplit_once('/') {
                Some((parent, _)) => result_text(
                    vm,
                    if parent.is_empty() {
                        "/".into()
                    } else {
                        parent.into()
                    },
                ),
                None => Ok(Some(Value::Null)),
            },
            ("getName", "()Ljava/lang/String;") => {
                result_text(vm, file.path.rsplit('/').next().unwrap_or("").to_string())
            }
            ("exists", "()Z") => Ok(Some(Value::Int(
                vm.guest_filesystem()?
                    .stat(&file.path)
                    .map_err(io)?
                    .is_some() as i32,
            ))),
            ("isDirectory", "()Z") => Ok(Some(Value::Int(
                vm.guest_filesystem()?
                    .stat(&file.path)
                    .map_err(io)?
                    .is_some_and(|m| m.directory) as i32,
            ))),
            ("isFile", "()Z") => Ok(Some(Value::Int(
                vm.guest_filesystem()?
                    .stat(&file.path)
                    .map_err(io)?
                    .is_some_and(|m| !m.directory) as i32,
            ))),
            ("length", "()J") => Ok(Some(Value::Long(
                vm.guest_filesystem()?
                    .stat(&file.path)
                    .map_err(io)?
                    .map(|m| m.length)
                    .unwrap_or(0) as i64,
            ))),
            ("mkdirs" | "mkdir", "()Z") => {
                if vm
                    .guest_filesystem()?
                    .stat(&file.path)
                    .map_err(io)?
                    .is_some()
                {
                    return Ok(Some(Value::Int(0)));
                }
                if name == "mkdir" {
                    let parent = file.path.rsplit_once('/').map(|p| p.0).unwrap_or("/");
                    if !vm
                        .guest_filesystem()?
                        .stat(parent)
                        .map_err(io)?
                        .is_some_and(|m| m.directory)
                    {
                        return Ok(Some(Value::Int(0)));
                    }
                }
                vm.guest_filesystem()?.mkdirs(&file.path).map_err(io)?;
                Ok(Some(Value::Int(1)))
            }
            ("delete", "()Z") => {
                let result = vm.guest_filesystem()?.remove(&file.path);
                Ok(Some(Value::Int(result.is_ok() as i32)))
            }
            _ => Ok(None),
        };
    }
    if name == "close" && sig == "()V" {
        vm.portable.files.get_mut(&recv).unwrap().closed = true;
        return Ok(Some(Value::Null));
    }
    if file.closed {
        return Err(exception("Ljava/io/IOException;", "Stream closed"));
    }
    if file.mode == 1 && name == "available" && sig == "()I" {
        let length = vm
            .guest_filesystem()?
            .stat(&file.path)
            .map_err(io)?
            .map(|m| m.length)
            .unwrap_or(0);
        return Ok(Some(Value::Int(
            length.saturating_sub(file.position).min(i32::MAX as u64) as i32,
        )));
    }
    if file.mode == 1 && name == "skip" && sig == "(J)J" {
        let count = args[0].as_long()?.max(0) as u64;
        let length = vm
            .guest_filesystem()?
            .stat(&file.path)
            .map_err(io)?
            .map(|m| m.length)
            .unwrap_or(0);
        let skipped = count.min(length.saturating_sub(file.position));
        vm.portable.files.get_mut(&recv).unwrap().position += skipped;
        return Ok(Some(Value::Long(skipped as i64)));
    }
    if name == "flush" && sig == "()V" && file.mode == 2 {
        return Ok(Some(Value::Null));
    } // Backend writes are synchronously committed.
    if name == "read" && file.mode == 1 {
        if sig == "()I" {
            let mut b = [0];
            let n = vm
                .guest_filesystem()?
                .read_at(&file.path, file.position, &mut b)
                .map_err(io)?;
            vm.portable.files.get_mut(&recv).unwrap().position += n as u64;
            return Ok(Some(Value::Int(if n == 0 { -1 } else { b[0] as i32 })));
        }
        if matches!(sig, "([B)I" | "([BII)I") {
            let array = object(&args[0])?;
            let size = vm.heap.as_array_elems(array)?.0.len();
            let (offset, len) = bounds(args, size)?;
            if len == 0 {
                return Ok(Some(Value::Int(0)));
            }
            let mut data = vec![0; len];
            let n = vm
                .guest_filesystem()?
                .read_at(&file.path, file.position, &mut data)
                .map_err(io)?;
            for (slot, byte) in vm.heap.array_elems_mut(array)?[offset..offset + n]
                .iter_mut()
                .zip(&data)
            {
                *slot = Value::Int(*byte as i8 as i32);
            }
            vm.portable.files.get_mut(&recv).unwrap().position += n as u64;
            return Ok(Some(Value::Int(if n == 0 { -1 } else { n as i32 })));
        }
    }
    if name == "write" && file.mode == 2 {
        let data = if sig == "(I)V" {
            vec![args[0].as_int()? as u8]
        } else if matches!(sig, "([B)V" | "([BII)V") {
            let array = object(&args[0])?;
            let (elems, _) = vm.heap.as_array_elems(array)?;
            let (offset, len) = bounds(args, elems.len())?;
            elems[offset..offset + len]
                .iter()
                .map(|v| v.as_int().map(|v| v as u8))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            return Ok(None);
        };
        let mut offset = 0;
        while offset < data.len() {
            let n = vm
                .guest_filesystem()?
                .write_at(
                    &file.path,
                    file.position + offset as u64,
                    &data[offset..],
                    false,
                )
                .map_err(io)?;
            if n == 0 {
                return Err(exception("Ljava/io/IOException;", "short write"));
            }
            offset += n;
        }
        vm.portable.files.get_mut(&recv).unwrap().position += data.len() as u64;
        return Ok(Some(Value::Null));
    }
    Ok(None)
}
fn bounds(args: &[Value], size: usize) -> Result<(usize, usize), VmExit> {
    let (offset, len) = if args.len() == 3 {
        (args[1].as_int()?, args[2].as_int()?)
    } else {
        (0, size as i32)
    };
    if offset < 0
        || len < 0
        || (offset as usize)
            .checked_add(len as usize)
            .map_or(true, |n| n > size)
    {
        return Err(exception(
            "Ljava/lang/IndexOutOfBoundsException;",
            "invalid array range",
        ));
    }
    Ok((offset as usize, len as usize))
}
