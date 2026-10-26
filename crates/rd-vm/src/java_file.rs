//! java.io.File algorithms extracted from RustyDroid's jfile. Paths live in
//! the Java heap; platform and reference transports never own another File map.
use crate::{err, framework_host::HostMethodKind, Engine, ObjRef, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use as_platform::{fs::Metadata, vfs::GuestFileSystem, PlatformError};

pub const CLASS: &str = "Ljava/io/File;";

/// Original lexical normalization, shared by the reference VFS and File values.
pub(crate) fn normalize(p: &str) -> String {
    if p == "." {
        return ".".to_string();
    }
    let abs = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if abs || parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if abs {
        format!("/{joined}")
    } else {
        joined
    }
}
pub(crate) fn parent_of(p: &str) -> Option<String> {
    let i = p.rfind('/')?;
    if i == 0 {
        if p.len() > 1 {
            Some("/".into())
        } else {
            None
        }
    } else {
        Some(p[..i].to_string())
    }
}
pub(crate) fn path_of(vm: &Engine, r: ObjRef) -> String {
    match vm.heap.get_field(r, "rd_path") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).map(str::to_string).unwrap_or_default(),
        _ => String::new(),
    }
}
pub(crate) fn new_file(vm: &mut Engine, path: &str) -> Result<Value, VmExit> {
    let s = crate::intrinsics::alloc_string(vm, normalize(path))?;
    let r = vm
        .heap
        .alloc_instance(
            CLASS.to_string(),
            vec![("rd_path".to_string(), Value::Obj(s))],
        )
        .map_err(err::oom_throwable)?;
    Ok(Value::Obj(r))
}
pub(crate) fn absolute(vm: &Engine, path: &str) -> Result<String, VmExit> {
    #[cfg(not(feature = "desktop-runtime"))]
    if vm.portable.filesystem.is_some() {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/data/user/0/{}/files/{path}", vm.portable.package)
        };
        return as_platform::vfs::normalize(&path).map_err(io_error);
    }
    let _ = vm;
    Ok(normalize(&if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }))
}
pub(crate) enum Io<'a> {
    #[cfg(not(feature = "desktop-runtime"))]
    Platform(&'a dyn GuestFileSystem),
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    Reference(crate::jfile::ReferenceIo<'a>),
}
impl<'a> core::ops::Deref for Io<'a> {
    type Target = dyn GuestFileSystem + 'a;
    fn deref(&self) -> &Self::Target {
        match self {
            #[cfg(not(feature = "desktop-runtime"))]
            Self::Platform(fs) => *fs,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            Self::Reference(fs) => fs,
        }
    }
}
pub(crate) fn storage(vm: &Engine) -> Result<Io<'_>, VmExit> {
    #[cfg(not(feature = "desktop-runtime"))]
    if let Some(fs) = vm.portable.filesystem.as_deref() {
        return Ok(Io::Platform(fs));
    }
    if vm.framework_host().is_some() {
        return Err(err::vm_error(
            "API_UNAVAILABLE",
            "Java File requires the bound persistent VFS",
        )
        .into());
    }
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    {
        Ok(Io::Reference(crate::jfile::ReferenceIo::new(vm)))
    }
    #[cfg(not(any(feature = "desktop-runtime", feature = "upstream-runtime")))]
    {
        Err(err::vm_error("API_UNAVAILABLE", "Java File storage is not bound").into())
    }
}
pub(crate) fn io_error(e: PlatformError) -> VmExit {
    if e.code == "API_UNAVAILABLE" {
        err::vm_error("API_UNAVAILABLE", e.to_string()).into()
    } else {
        crate::Throwable::new("Ljava/io/IOException;", e.to_string()).into()
    }
}
fn text(vm: &Engine, v: Option<&Value>, what: &str) -> Result<String, VmExit> {
    let r = v
        .and_then(|v| v.as_ref().ok().flatten())
        .ok_or_else(|| crate::Throwable::new("Ljava/lang/NullPointerException;", what))?;
    Ok(vm.heap.as_str(r)?.to_string())
}
fn s(vm: &mut Engine, value: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, value)?))
}
fn stat(vm: &Engine, path: &str) -> Result<Option<Metadata>, VmExit> {
    if path.is_empty() {
        return Ok(None);
    }
    let path = absolute(vm, path)?;
    match storage(vm)?.stat(&path) {
        Ok(meta) => Ok(meta),
        Err(e) if matches!(e.code, "ACCESS_DENIED" | "UNMAPPED_PATH" | "INVALID_PATH") => Ok(None),
        Err(e) => Err(io_error(e)),
    }
}
pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance, Static};
    if class != CLASS {
        return None;
    }
    if name == "<init>"
        && matches!(
            sig,
            "(Ljava/lang/String;)V"
                | "(Ljava/lang/String;Ljava/lang/String;)V"
                | "(Ljava/io/File;Ljava/lang/String;)V"
        )
    {
        return Some(Constructor);
    }
    if name == "createTempFile"
        && matches!(
            sig,
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;"
                | "(Ljava/lang/String;Ljava/lang/String;Ljava/io/File;)Ljava/io/File;"
        )
    {
        return Some(Static);
    }
    let implemented = match name {
        "getPath" | "toString" | "getAbsolutePath" | "getCanonicalPath" | "getName"
        | "getParent" => sig == "()Ljava/lang/String;",
        "getAbsoluteFile" | "getCanonicalFile" | "getParentFile" => sig == "()Ljava/io/File;",
        "exists" | "isDirectory" | "isFile" | "isAbsolute" | "isHidden" | "canRead"
        | "canWrite" | "canExecute" | "mkdir" | "mkdirs" | "createNewFile" | "delete"
        | "setReadOnly" => sig == "()Z",
        "length" | "lastModified" | "getFreeSpace" | "getUsableSpace" | "getTotalSpace" => {
            sig == "()J"
        }
        "list" => sig == "()[Ljava/lang/String;",
        "listFiles" => sig == "()[Ljava/io/File;",
        "equals" => sig == "(Ljava/lang/Object;)Z",
        "hashCode" => sig == "()I",
        "compareTo" => matches!(sig, "(Ljava/io/File;)I" | "(Ljava/lang/Object;)I"),
        "renameTo" => sig == "(Ljava/io/File;)Z",
        "deleteOnExit" => sig == "()V",
        "setLastModified" => sig == "(J)Z",
        "setReadable" | "setWritable" | "setExecutable" => matches!(sig, "(Z)Z" | "(ZZ)Z"),
        _ => false,
    };
    implemented.then_some(Instance)
}
pub(crate) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if method_metadata(class, name, sig) != Some(HostMethodKind::Static) {
        return Ok(None);
    }
    crate::shared_call::with_values(vm, args, |vm| {
        let prefix = text(vm, args.first(), "prefix == null")?;
        if prefix.encode_utf16().count() < 3 {
            return Err(crate::Throwable::new(
                "Ljava/lang/IllegalArgumentException;",
                "prefix must contain at least 3 characters",
            )
            .into());
        }
        let suffix = if matches!(args.get(1), None | Some(Value::Null)) {
            ".tmp".to_string()
        } else {
            text(vm, args.get(1), "suffix")?
        };
        if prefix.contains(['/', '\\', '\0']) || suffix.contains(['/', '\\', '\0']) {
            return Err(crate::Throwable::new(
                "Ljava/io/IOException;",
                "invalid temporary file name",
            )
            .into());
        }
        let dir = match args.get(2) {
            Some(Value::Obj(r)) => path_of(vm, *r),
            _ => {
                #[cfg(not(feature = "desktop-runtime"))]
                if vm.portable.filesystem.is_some() {
                    let dir = format!("/data/user/0/{}/cache", vm.portable.package);
                    storage(vm)?.mkdirs(&dir).map_err(io_error)?;
                    return temp_in(vm, &dir, &prefix, &suffix);
                }
                let dir = "/data/local/tmp".to_string();
                storage(vm)?.mkdirs(&dir).map_err(io_error)?;
                dir
            }
        };
        temp_in(vm, &absolute(vm, &dir)?, &prefix, &suffix)
    })
}
fn temp_in(
    vm: &mut Engine,
    dir: &str,
    prefix: &str,
    suffix: &str,
) -> Result<Option<Value>, VmExit> {
    for _ in 0..4096 {
        vm.file_temp_sequence = vm.file_temp_sequence.wrapping_add(1);
        let path = format!("{dir}/{prefix}{}{suffix}", vm.file_temp_sequence);
        if storage(vm)?.create_file_new(&path).map_err(io_error)? {
            return Ok(Some(new_file(vm, &path)?));
        }
    }
    Err(crate::Throwable::new(
        "Ljava/io/IOException;",
        "temporary file name space exhausted",
    )
    .into())
}
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        method_metadata(class, name, sig),
        Some(HostMethodKind::Constructor | HostMethodKind::Instance)
    ) {
        return Ok(None);
    }
    let mut roots = vec![Value::Obj(recv)];
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| call_inner(vm, name, sig, recv, args))
}
fn call_inner(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let path = match sig {
            "(Ljava/lang/String;)V" => text(vm, args.first(), "pathname == null")?,
            "(Ljava/io/File;Ljava/lang/String;)V" | "(Ljava/lang/String;Ljava/lang/String;)V" => {
                let child = text(vm, args.get(1), "child == null")?;
                let parent = match args.first() {
                    None | Some(Value::Null) => None,
                    Some(Value::Obj(p)) if sig.starts_with("(Ljava/io/File;") => {
                        Some(path_of(vm, *p))
                    }
                    _ => Some(text(vm, args.first(), "parent")?),
                };
                match parent {
                    Some(p) if p.is_empty() => format!("/{child}"),
                    Some(p) => format!("{p}/{child}"),
                    None => child,
                }
            }
            _ => return Ok(None),
        };
        let v = crate::intrinsics::alloc_string(vm, normalize(&path))?;
        err::put_field_result(vm.heap.put_field(recv, "rd_path", Value::Obj(v)))?;
        return Ok(Some(Value::Null));
    }
    let p = path_of(vm, recv);
    let abs = absolute(vm, &p)?;
    let b = |value: bool| Value::Int(i32::from(value));
    let value = match name {
        "getPath" | "toString" => s(vm, p)?,
        "getAbsolutePath" | "getCanonicalPath" => s(vm, abs)?,
        "getAbsoluteFile" | "getCanonicalFile" => new_file(vm, &abs)?,
        "getName" => s(vm, p.rsplit('/').next().unwrap_or("").to_string())?,
        "getParent" => match parent_of(&p) {
            Some(p) => s(vm, p)?,
            None => Value::Null,
        },
        "getParentFile" => match parent_of(&p) {
            Some(p) => new_file(vm, &p)?,
            None => Value::Null,
        },
        "exists" => b(stat(vm, &p)?.is_some()),
        "isDirectory" => b(stat(vm, &p)?.is_some_and(|m| m.directory)),
        "isFile" => b(stat(vm, &p)?.is_some_and(|m| !m.directory)),
        "isAbsolute" => b(p.starts_with('/')),
        "isHidden" => b(!p.is_empty()
            && p != "."
            && p != ".."
            && p.rsplit('/').next().is_some_and(|n| n.starts_with('.'))),
        "length" => Value::Long(stat(vm, &p)?.map_or(0, |m| {
            if m.directory {
                0
            } else {
                m.length.min(i64::MAX as u64) as i64
            }
        })),
        "lastModified" => Value::Long(if stat(vm, &p)?.is_some() {
            storage(vm)?
                .modified_millis(&abs)
                .map_err(io_error)?
                .min(i64::MAX as u64) as i64
        } else {
            0
        }),
        "mkdir" | "mkdirs" => {
            if p.is_empty() || stat(vm, &p)?.is_some() {
                return Ok(Some(b(false)));
            }
            let io = storage(vm)?;
            let result = if name == "mkdir" {
                io.create_dir(&abs)
            } else {
                io.mkdirs(&abs).map(|()| true)
            };
            b(bool_result(result)?)
        }
        "createNewFile" => {
            if p.is_empty() {
                return Err(crate::Throwable::new("Ljava/io/IOException;", "empty path").into());
            }
            b(storage(vm)?.create_file_new(&abs).map_err(io_error)?)
        }
        "delete" => {
            if p.is_empty() {
                b(false)
            } else {
                b(bool_result(storage(vm)?.remove(&abs).map(|()| true))?)
            }
        }
        "renameTo" => {
            let dst = args
                .first()
                .and_then(|v| v.as_ref().ok().flatten())
                .ok_or_else(|| {
                    crate::Throwable::new("Ljava/lang/NullPointerException;", "dest == null")
                })?;
            let dst = path_of(vm, dst);
            if p.is_empty() || dst.is_empty() {
                b(false)
            } else {
                b(bool_result(
                    storage(vm)?
                        .rename_replace(&abs, &absolute(vm, &dst)?)
                        .map(|()| true),
                )?)
            }
        }
        "list" | "listFiles" => {
            if !stat(vm, &p)?.is_some_and(|m| m.directory) {
                return Ok(Some(Value::Null));
            }
            let entries = storage(vm)?.list(&abs).map_err(io_error)?;
            let prefix = if abs == "/" { abs } else { format!("{abs}/") };
            let mut vals = Vec::new();
            for entry in entries {
                let v = if name == "list" {
                    s(vm, entry.name)?
                } else {
                    new_file(vm, &format!("{prefix}{}", entry.name))?
                };
                if let Value::Obj(r) = v {
                    vm.heap.pin_allocation(r);
                }
                vals.push(v);
            }
            crate::arrays::new_array(
                vm,
                if name == "list" {
                    "[Ljava/lang/String;"
                } else {
                    "[Ljava/io/File;"
                },
                crate::heap::ElemKind::Obj,
                vals,
            )?
        }
        "equals" => {
            let other = match args.first() {
                Some(Value::Obj(o)) if vm.heap.class_of(*o).is_ok_and(|c| c == CLASS) => {
                    path_of(vm, *o)
                }
                _ => return Ok(Some(b(false))),
            };
            b(other == p)
        }
        "hashCode" => Value::Int(
            p.encode_utf16()
                .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
                ^ 1234321,
        ),
        "compareTo" => {
            let o = args
                .first()
                .and_then(|v| v.as_ref().ok().flatten())
                .ok_or_else(|| {
                    crate::Throwable::new("Ljava/lang/NullPointerException;", "pathname == null")
                })?;
            Value::Int(p.cmp(&path_of(vm, o)) as i32)
        }
        _ => return legacy_attributes(vm, name, sig, &p),
    };
    Ok(Some(value))
}
fn bool_result(value: Result<bool, PlatformError>) -> Result<bool, VmExit> {
    match value {
        Ok(value) => Ok(value),
        Err(e) if e.code == "API_UNAVAILABLE" => Err(io_error(e)),
        Err(_) => Ok(false),
    }
}
fn legacy_attributes(
    vm: &Engine,
    name: &str,
    sig: &str,
    path: &str,
) -> Result<Option<Value>, VmExit> {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    if matches!(storage(vm)?, Io::Reference(_)) {
        return Ok(Some(match name {
            "canRead" | "canWrite" | "canExecute" => {
                Value::Int(i32::from(stat(vm, path)?.is_some()))
            }
            "getFreeSpace" | "getUsableSpace" => Value::Long(4 << 30),
            "getTotalSpace" => Value::Long(8 << 30),
            _ if sig.ends_with('V') => Value::Null,
            _ => Value::Int(1),
        }));
    }
    let _ = (vm, sig);
    Err(err::vm_error(
        "API_UNAVAILABLE",
        format!("java.io.File.{name}: mounted filesystem attribute contract for {path}"),
    )
    .into())
}
