//! AtomicFile backup protocol extracted from RustyDroid jfile; real storage
//! operations and durable finish are supplied by the common mount.
use crate::java_file::{absolute, io_error, new_file, parent_of, path_of, storage};
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
pub const CLASS: &str = "Landroid/util/AtomicFile;";
const FIS: &str = "Ljava/io/FileInputStream;";
const FOS: &str = "Ljava/io/FileOutputStream;";
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != CLASS {
        return Ok(None);
    }
    let mut roots = vec![Value::Obj(recv)];
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| call_inner(vm, name, sig, recv, args))
}
fn atomic_throw(class: &str, message: impl Into<String>) -> VmExit {
    crate::err::Throwable::new(class, message.into()).into()
}

fn atomic_file_ref(vm: &Engine, recv: ObjRef, field: &str) -> Result<ObjRef, VmExit> {
    match vm.heap.get_field(recv, field) {
        Ok(Value::Obj(file)) => Ok(file),
        _ => Err(atomic_throw(
            "Ljava/lang/NullPointerException;",
            format!("{field} == null"),
        )),
    }
}

fn atomic_paths(vm: &Engine, recv: ObjRef) -> Result<(String, String), VmExit> {
    let base = atomic_file_ref(vm, recv, "mBaseFile")?;
    let backup = atomic_file_ref(vm, recv, "mBackupFile")?;
    Ok((
        absolute(vm, &path_of(vm, base))?,
        absolute(vm, &path_of(vm, backup))?,
    ))
}

fn atomic_rename(vm: &Engine, from: &str, to: &str) -> Result<bool, VmExit> {
    storage(vm)?.rename_replace(from, to).map_err(io_error)?;
    Ok(true)
}
fn is_file(vm: &Engine, path: &str) -> Result<bool, VmExit> {
    Ok(storage(vm)?
        .stat(path)
        .map_err(io_error)?
        .is_some_and(|m| !m.directory))
}
fn is_dir(vm: &Engine, path: &str) -> Result<bool, VmExit> {
    Ok(storage(vm)?
        .stat(path)
        .map_err(io_error)?
        .is_some_and(|m| m.directory))
}
fn delete_file(vm: &Engine, path: &str) -> Result<(), VmExit> {
    if storage(vm)?.stat(path).map_err(io_error)?.is_some() {
        storage(vm)?.remove(path).map_err(io_error)?;
    }
    Ok(())
}
fn mkdirs(vm: &Engine, path: &str) -> Result<(), VmExit> {
    storage(vm)?.mkdirs(path).map_err(io_error)
}

fn atomic_create_stream(vm: &mut Engine, class: &str, path: &str) -> Result<Value, VmExit> {
    let stream = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    let path_ref = crate::intrinsics::alloc_string(vm, path.to_string())?;
    if crate::java_stream::call(
        vm,
        class,
        "<init>",
        "(Ljava/lang/String;)V",
        stream,
        &[Value::Obj(path_ref)],
    )?
    .is_none()
    {
        return Err(crate::err::vm_error(
            "VM_TYPE_ERROR",
            format!("{class} constructor was not handled"),
        )
        .into());
    }
    Ok(Value::Obj(stream))
}

fn atomic_close_stream(vm: &mut Engine, args: &[Value]) {
    if let Some(Value::Obj(stream)) = args.first() {
        // FileOutputStream.close() also makes the host VFS state final; the
        // AOSP implementation ignores IOException from this cleanup path.
        let _ = vm.invoke_instance(*stream, "close", "()V", &[]);
    }
}

fn atomic_restore_backup(vm: &Engine, base: &str, backup: &str) -> Result<(), VmExit> {
    if is_file(vm, backup)? {
        delete_file(vm, base)?;
        atomic_rename(vm, backup, base)?;
    }
    Ok(())
}

fn atomic_open_read(vm: &mut Engine, recv: ObjRef) -> Result<Value, VmExit> {
    let (base, backup) = atomic_paths(vm, recv)?;
    atomic_restore_backup(vm, &base, &backup)?;
    atomic_create_stream(vm, FIS, &base)
}

fn call_inner(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(Ljava/io/File;)V") => {
            let Some(base @ Value::Obj(base_ref)) = args.first() else {
                return Err(atomic_throw(
                    "Ljava/lang/NullPointerException;",
                    "baseFile == null",
                ));
            };
            let base_path = path_of(vm, *base_ref);
            let backup = new_file(vm, &format!("{base_path}.bak"))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "mBaseFile", base.clone()))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "mBackupFile", backup))?;
            Ok(Some(Value::Null))
        }
        ("getBaseFile", "()Ljava/io/File;") => {
            Ok(Some(Value::Obj(atomic_file_ref(vm, recv, "mBaseFile")?)))
        }
        ("startWrite", "()Ljava/io/FileOutputStream;") => {
            let (base, backup) = atomic_paths(vm, recv)?;
            if is_file(vm, &base)? || is_dir(vm, &base)? {
                if !is_file(vm, &backup)? && !is_dir(vm, &backup)? {
                    if !is_file(vm, &base)? || !atomic_rename(vm, &base, &backup)? {
                        return Err(atomic_throw(
                            "Ljava/io/IOException;",
                            format!("Couldn't rename file {base} to backup file {backup}"),
                        ));
                    }
                } else {
                    // A pre-existing backup is the older committed state;
                    // the current base is an interrupted replacement.
                    delete_file(vm, &base)?;
                }
            }
            match atomic_create_stream(vm, FOS, &base) {
                Ok(stream) => Ok(Some(stream)),
                Err(error) => {
                    let Some(parent) = parent_of(&base) else {
                        return Err(error);
                    };
                    mkdirs(vm, &parent)?;
                    if !is_dir(vm, &parent)? {
                        return Err(error);
                    }
                    Ok(Some(atomic_create_stream(vm, FOS, &base)?))
                }
            }
        }
        ("finishWrite", "(Ljava/io/FileOutputStream;)V") => {
            let Some(Value::Obj(stream)) = args.first() else {
                return Ok(Some(Value::Null));
            };
            // Do not remove the committed backup unless the replacement has
            // actually reached the storage backend's durability boundary.
            vm.invoke_instance(*stream, "flush", "()V", &[])?;
            atomic_close_stream(vm, args);
            let (_, backup) = atomic_paths(vm, recv)?;
            delete_file(vm, &backup)?;
            Ok(Some(Value::Null))
        }
        ("failWrite", "(Ljava/io/FileOutputStream;)V") => {
            atomic_close_stream(vm, args);
            let (base, backup) = atomic_paths(vm, recv)?;
            delete_file(vm, &base)?;
            if is_file(vm, &backup)? {
                atomic_rename(vm, &backup, &base)?;
            }
            Ok(Some(Value::Null))
        }
        ("openRead", "()Ljava/io/FileInputStream;") => Ok(Some(atomic_open_read(vm, recv)?)),
        ("readFully", "()[B") => {
            let Value::Obj(stream) = atomic_open_read(vm, recv)? else {
                return Err(atomic_throw(
                    "Ljava/io/IOException;",
                    "openRead returned null",
                ));
            };
            let scratch = crate::jlstring::byte_array(vm, &[0; 8192])?;
            let Value::Obj(scratch_ref) = scratch else {
                return Err(atomic_throw("Ljava/io/IOException;", "scratch buffer"));
            };
            let mut contents = Vec::new();
            loop {
                let count = vm
                    .invoke_instance(
                        stream,
                        "read",
                        "([BII)I",
                        &[scratch.clone(), Value::Int(0), Value::Int(8192)],
                    )?
                    .as_int()
                    .unwrap_or(-1);
                if count < 0 {
                    break;
                }
                if count == 0 {
                    continue;
                }
                let bytes = vm.heap.as_array_elems(scratch_ref)?.0;
                if contents.len().saturating_add(count as usize) > vm.heap.budget() {
                    return Err(atomic_throw(
                        "Ljava/lang/OutOfMemoryError;",
                        "AtomicFile.readFully exceeds VM heap budget",
                    ));
                }
                contents.extend(
                    bytes[..(count as usize).min(bytes.len())]
                        .iter()
                        .map(|value| value.as_int().unwrap_or(0) as u8),
                );
            }
            let _ = vm.invoke_instance(stream, "close", "()V", &[]);
            Ok(Some(crate::jlstring::byte_array(vm, &contents)?))
        }
        ("delete", "()V") => {
            let (base, backup) = atomic_paths(vm, recv)?;
            delete_file(vm, &base)?;
            delete_file(vm, &backup)?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}
pub fn method_metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<crate::framework_host::HostMethodKind> {
    use crate::framework_host::HostMethodKind::{Constructor, Instance};
    if class != CLASS {
        return None;
    }
    let implemented = matches!(
        (name, sig),
        ("<init>", "(Ljava/io/File;)V")
            | ("getBaseFile", "()Ljava/io/File;")
            | ("startWrite", "()Ljava/io/FileOutputStream;")
            | ("finishWrite" | "failWrite", "(Ljava/io/FileOutputStream;)V")
            | ("openRead", "()Ljava/io/FileInputStream;")
            | ("readFully", "()[B")
            | ("delete", "()V")
    );
    implemented.then_some(if name == "<init>" {
        Constructor
    } else {
        Instance
    })
}
