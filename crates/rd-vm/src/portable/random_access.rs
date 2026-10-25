//! RandomAccessFile scalar data I/O over the shared persistent guest VFS.
use super::{
    file::{absolute, io, object, path, text},
    FileState,
};
use crate::{Engine, ObjRef, Throwable, Value, VmExit};
use alloc::vec::Vec;
type Result<T> = core::result::Result<T, VmExit>;
fn failure(class: &str, message: &str) -> VmExit {
    Throwable::new(class, message).into()
}
fn read(vm: &mut Engine, r: ObjRef, size: usize) -> Result<Vec<u8>> {
    let file = vm.portable.files[&r].clone();
    let mut bytes = alloc::vec![0;size];
    let mut done = 0;
    while done < size {
        let n = vm
            .guest_filesystem()?
            .read_at(&file.path, file.position + done as u64, &mut bytes[done..])
            .map_err(io)?;
        if n == 0 {
            break;
        }
        done += n;
    }
    vm.portable.files.get_mut(&r).unwrap().position += done as u64;
    bytes.truncate(done);
    Ok(bytes)
}
fn write(vm: &mut Engine, r: ObjRef, bytes: &[u8]) -> Result<()> {
    let file = vm.portable.files[&r].clone();
    if file.mode != 4 {
        return Err(failure("Ljava/io/IOException;", "file is read-only"));
    }
    let mut done = 0;
    while done < bytes.len() {
        let n = vm
            .guest_filesystem()?
            .write_at(
                &file.path,
                file.position + done as u64,
                &bytes[done..],
                false,
            )
            .map_err(io)?;
        if n == 0 {
            return Err(failure("Ljava/io/IOException;", "short write"));
        }
        done += n;
        vm.portable.files.get_mut(&r).unwrap().position += n as u64;
    }
    Ok(())
}
pub(super) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    r: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    if class != "Ljava/io/RandomAccessFile;" {
        return Ok(None);
    }
    if name == "<init>"
        && matches!(
            sig,
            "(Ljava/io/File;Ljava/lang/String;)V" | "(Ljava/lang/String;Ljava/lang/String;)V"
        )
    {
        let path = absolute(vm, &path(vm, &args[0])?)?;
        let mode = text(vm, &args[1])?;
        let writable = match mode.as_str() {
            "r" => false,
            "rw" => true,
            "rws" | "rwd" => {
                return Err(crate::err::vm_error(
                    "API_UNAVAILABLE",
                    "synchronous RandomAccessFile modes need VFS fsync",
                )
                .into())
            }
            _ => {
                return Err(failure(
                    "Ljava/lang/IllegalArgumentException;",
                    "invalid mode",
                ))
            }
        };
        let metadata = vm.guest_filesystem()?.stat(&path).map_err(io)?;
        if metadata.as_ref().is_some_and(|m| m.directory) || (!writable && metadata.is_none()) {
            return Err(failure(
                "Ljava/io/FileNotFoundException;",
                "file does not exist or is directory",
            ));
        }
        if writable && metadata.is_none() {
            vm.guest_filesystem()?
                .write_at(&path, 0, &[], false)
                .map_err(io)?;
        }
        vm.portable.files.insert(
            r,
            FileState {
                path,
                position: 0,
                mode: if writable { 4 } else { 3 },
                closed: false,
            },
        );
        return Ok(Some(Value::Null));
    }
    let file = vm
        .portable
        .files
        .get(&r)
        .cloned()
        .ok_or_else(|| failure("Ljava/io/IOException;", "uninitialized file"))?;
    if (name, sig) == ("close", "()V") {
        vm.portable.files.get_mut(&r).unwrap().closed = true;
        return Ok(Some(Value::Null));
    }
    if file.closed {
        return Err(failure("Ljava/io/IOException;", "file closed"));
    }
    let value = match (name, sig) {
        ("seek", "(J)V") => {
            let n = args[0].as_long()?;
            if n < 0 {
                return Err(failure("Ljava/io/IOException;", "negative seek"));
            }
            vm.portable.files.get_mut(&r).unwrap().position = n as u64;
            Value::Null
        }
        ("getFilePointer", "()J") => Value::Long(file.position as i64),
        ("length", "()J") => Value::Long(
            vm.guest_filesystem()?
                .stat(&file.path)
                .map_err(io)?
                .map_or(0, |m| m.length) as i64,
        ),
        ("writeInt", "(I)V") => {
            write(vm, r, &args[0].as_int()?.to_be_bytes())?;
            Value::Null
        }
        ("writeLong", "(J)V") => {
            write(vm, r, &args[0].as_long()?.to_be_bytes())?;
            Value::Null
        }
        ("write" | "writeByte", "(I)V") => {
            write(vm, r, &[args[0].as_int()? as u8])?;
            Value::Null
        }
        ("readInt", "()I") => {
            let b = read(vm, r, 4)?;
            if b.len() != 4 {
                return Err(failure("Ljava/io/EOFException;", "readInt EOF"));
            }
            Value::Int(i32::from_be_bytes(b.try_into().unwrap()))
        }
        ("readLong", "()J") => {
            let b = read(vm, r, 8)?;
            if b.len() != 8 {
                return Err(failure("Ljava/io/EOFException;", "readLong EOF"));
            }
            Value::Long(i64::from_be_bytes(b.try_into().unwrap()))
        }
        ("read", "()I") => {
            let b = read(vm, r, 1)?;
            Value::Int(b.first().map_or(-1, |b| *b as i32))
        }
        ("write", "([B)V") => {
            let b = vm
                .heap
                .as_array_elems(object(&args[0])?)?
                .0
                .iter()
                .map(|v| v.as_int().map(|v| v as u8))
                .collect::<core::result::Result<Vec<_>, _>>()?;
            write(vm, r, &b)?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
