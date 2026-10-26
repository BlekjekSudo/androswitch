//! RustyDroid FileInputStream/FileOutputStream algorithms over the selected
//! transport. Java stream/channel ownership is represented by heap edges.
use crate::java_file::{absolute, io_error, parent_of, path_of, storage, CLASS as FILE};
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
fn atomic_throw(class: &str, message: impl Into<String>) -> VmExit {
    crate::Throwable::new(class, message.into()).into()
}
fn put(vm: &mut Engine, r: ObjRef, field: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, field, v))
}
pub(crate) fn supports(class: &str) -> bool {
    STREAMS.contains(&class)
}
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !supports(class) {
        return Ok(None);
    }
    let reference_surface = matches!(
        (class, name, sig),
        (FIS | FOS, "getFD", "()Ljava/io/FileDescriptor;")
            | (FIS | FOS, "<init>", "(Ljava/io/FileDescriptor;)V")
            | (FD, "valid", "()Z")
            | (FD, "sync", "()V")
            | (
                CHANNEL,
                "lock" | "tryLock",
                "()Ljava/nio/channels/FileLock;"
            )
            | (FLOCK, "release" | "close", "()V")
            | (FLOCK, "isValid" | "isShared", "()Z")
    );
    if method_metadata(class, name, sig).is_none() && !reference_surface {
        return Ok(None);
    }
    if reference_surface {
        #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
        let reference = matches!(storage(vm)?, crate::java_file::Io::Reference(_));
        #[cfg(not(any(feature = "desktop-runtime", feature = "upstream-runtime")))]
        let reference = false;
        if !reference {
            return Err(crate::err::vm_error(
                "API_UNAVAILABLE",
                format!(
                    "{class}->{name}{sig}: shared native descriptor/lock adapter is not connected"
                ),
            )
            .into());
        }
    }
    let mut roots = vec![Value::Obj(recv)];
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| {
        call_inner(vm, class, name, sig, recv, args)
    })
}
const FOS: &str = "Ljava/io/FileOutputStream;";
const FIS: &str = "Ljava/io/FileInputStream;";
const CHANNEL: &str = "Ljava/nio/channels/FileChannel;";
const FLOCK: &str = "Ljava/nio/channels/FileLock;";
const FD: &str = "Ljava/io/FileDescriptor;";
const STREAMS: [&str; 5] = [FOS, FIS, CHANNEL, FLOCK, FD];

fn with_path(vm: &mut Engine, class: &str, path: &str) -> Result<Value, VmExit> {
    let sv = crate::intrinsics::alloc_string(vm, path.to_string())?;
    let r = vm
        .heap
        .alloc_instance(
            class.to_string(),
            vec![("rd_path".to_string(), Value::Obj(sv))],
        )
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

fn bytes_of(vm: &Engine, v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| {
                e.iter()
                    .map(|x| match x {
                        Value::Int(i) => *i as u8,
                        _ => 0,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn int_arg(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Int(v)) => *v as i64,
        Some(Value::Long(v)) => *v,
        _ => 0,
    }
}

fn call_inner(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let fnf = |p: &str| -> VmExit {
        crate::err::Throwable::new(
            "Ljava/io/FileNotFoundException;",
            format!("{p}: open failed: ENOENT (No such file or directory)"),
        )
        .into()
    };
    if name == "<init>" {
        if sig.contains("FileDescriptor") {
            #[cfg(not(any(feature = "desktop-runtime", feature = "upstream-runtime")))]
            return Err(crate::err::vm_error(
                "API_UNAVAILABLE",
                "stream constructor: native descriptor ownership is not bound",
            )
            .into());
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            if !matches!(storage(vm)?, crate::java_file::Io::Reference(_)) {
                return Err(crate::err::vm_error(
                    "API_UNAVAILABLE",
                    "stream constructor: native descriptor ownership is not bound",
                )
                .into());
            }
        }
        let path = match args.first() {
            Some(Value::Obj(o)) => {
                let c = vm.heap.class_of(*o).map(str::to_string).unwrap_or_default();
                if c == FILE || c == FD {
                    path_of(vm, *o)
                } else {
                    vm.heap.as_str(*o).map(str::to_string).unwrap_or_default()
                }
            }
            _ => {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "file".to_string(),
                )
                .into())
            }
        };
        let abs = absolute(vm, &path)?;
        if class == FOS {
            let append = matches!(args.get(1), Some(Value::Int(1)));
            let io = storage(vm)?;
            let parent_exists = match parent_of(&abs) {
                Some(parent) => io
                    .stat(&parent)
                    .map_err(io_error)?
                    .is_some_and(|m| m.directory),
                None => false,
            };
            if !parent_exists
                || io
                    .stat(&abs)
                    .map_err(io_error)?
                    .is_some_and(|m| m.directory)
            {
                return Err(fnf(&abs));
            }
            io.write_at(&abs, 0, &[], !append && !sig.contains("FileDescriptor"))
                .map_err(io_error)?;
            put(vm, recv, "rd_append", Value::Int(i32::from(append)))?;
        } else if class == FIS
            && !storage(vm)?
                .stat(&abs)
                .map_err(io_error)?
                .is_some_and(|m| !m.directory)
        {
            return Err(fnf(&abs));
        }
        put(vm, recv, "rd_pos", Value::Long(0))?;
        put(vm, recv, "rd_closed", Value::Int(0))?;
        let sv = crate::intrinsics::alloc_string(vm, abs)?;
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_path", Value::Obj(sv)))?;
        return Ok(Some(Value::Null));
    }
    let owner = match vm.heap.get_field(recv, "rd_stream") {
        Ok(Value::Obj(r)) => r,
        _ => recv,
    };
    let p = path_of(vm, owner);
    let closed = matches!(vm.heap.get_field(owner, "rd_closed"), Ok(Value::Int(1)));
    if matches!(name, "close" | "isOpen") {
        if name == "isOpen" {
            return Ok(Some(Value::Int(i32::from(!closed))));
        }
        put(vm, owner, "rd_closed", Value::Int(1))?;
        return Ok(Some(Value::Null));
    }
    if closed {
        return Err(atomic_throw("Ljava/io/IOException;", "Stream closed"));
    }

    let pos = match vm.heap.get_field(owner, "rd_pos") {
        Ok(Value::Long(x)) => x as usize,
        _ => 0,
    };
    let set_pos = |vm: &mut Engine, x: usize| {
        crate::err::put_field_result(vm.heap.put_field(owner, "rd_pos", Value::Long(x as i64)))
    };
    let len = storage(vm)?
        .stat(&p)
        .map_err(io_error)?
        .map_or(0, |m| m.length.min(usize::MAX as u64) as usize);
    Ok(Some(match (class, name) {
        (FOS, "write") => {
            let data = match sig {
                "(I)V" => vec![int_arg(args, 0) as u8],
                "([B)V" => {
                    let Some(Value::Obj(_)) = args.first() else {
                        return Err(atomic_throw(
                            "Ljava/lang/NullPointerException;",
                            "b == null",
                        ));
                    };
                    bytes_of(vm, args.first())
                }
                _ => {
                    let Some(Value::Obj(_)) = args.first() else {
                        return Err(atomic_throw(
                            "Ljava/lang/NullPointerException;",
                            "b == null",
                        ));
                    };
                    let all = bytes_of(vm, args.first());
                    let cap = all.len();
                    let off_i = int_arg(args, 1);
                    let len_i = int_arg(args, 2);
                    if off_i < 0 || len_i < 0 {
                        return Err(atomic_throw(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("offset={off_i}, count={len_i}, length={cap}"),
                        ));
                    }
                    let (off, n) = (off_i as usize, len_i as usize);
                    if off > cap || n > cap - off {
                        return Err(atomic_throw(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("offset={off_i}, count={len_i}, length={cap}"),
                        ));
                    }
                    all[off..off + n].to_vec()
                }
            };
            let append = matches!(vm.heap.get_field(owner, "rd_append"), Ok(Value::Int(1)));
            let pos = if append { len } else { pos };
            let io = storage(vm)?;
            let mut done = 0;
            while done < data.len() {
                let end = (done + 65536).min(data.len());
                let n = io
                    .write_at(
                        &p,
                        (pos as u64).saturating_add(done as u64),
                        &data[done..end],
                        false,
                    )
                    .map_err(io_error)?;
                if n == 0 || n > end - done {
                    return Err(atomic_throw("Ljava/io/IOException;", "invalid short write"));
                }
                done += n;
            }
            set_pos(vm, pos + done)?;
            Value::Null
        }
        (FOS | FIS, "flush") | (CHANNEL, "force") | (FD, "sync") => {
            storage(vm)?.sync_file(&p).map_err(io_error)?;
            Value::Null
        }
        (FOS | FIS, "getChannel") => {
            let value = with_path(vm, CHANNEL, &p)?;
            if let Value::Obj(r) = value {
                put(vm, r, "rd_stream", Value::Obj(owner))?;
            }
            value
        }
        (FOS | FIS, "getFD") => return reference_fd(vm, class, owner, &p, pos),
        (FD, "valid") => return reference_fd_valid(vm, recv),
        (FIS, "available") => Value::Int(len.saturating_sub(pos) as i32),
        (FIS, "skip") => {
            let n = (int_arg(args, 0).max(0) as usize).min(len.saturating_sub(pos));
            set_pos(vm, pos + n)?;
            Value::Long(n as i64)
        }
        (FIS, "read") => {
            if sig == "()I" {
                let mut byte = [0];
                let n = storage(vm)?
                    .read_at(&p, pos as u64, &mut byte)
                    .map_err(io_error)?;
                let b = if n == 0 { None } else { Some(byte[0]) };
                match b {
                    Some(b) => {
                        set_pos(vm, pos + 1)?;
                        Value::Int(b as i32)
                    }
                    None => Value::Int(-1),
                }
            } else {
                let Some(Value::Obj(arr)) = args.first().cloned() else {
                    return Err(atomic_throw(
                        "Ljava/lang/NullPointerException;",
                        "b == null",
                    ));
                };
                let cap = vm
                    .heap
                    .as_array_elems(arr)
                    .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_STATE", e)))?
                    .0
                    .len();
                let (off, want) = if sig == "([B)I" {
                    (0usize, cap)
                } else {
                    let off_i = int_arg(args, 1);
                    let len_i = int_arg(args, 2);
                    if off_i < 0 || len_i < 0 {
                        return Err(atomic_throw(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("offset={off_i}, count={len_i}, length={cap}"),
                        ));
                    }
                    let (o, w) = (off_i as usize, len_i as usize);
                    if o > cap || w > cap - o {
                        return Err(atomic_throw(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("offset={off_i}, count={len_i}, length={cap}"),
                        ));
                    }
                    (o, w)
                };
                if want == 0 {
                    return Ok(Some(Value::Int(0)));
                }
                let want = want.min(len.saturating_sub(pos));
                let mut chunk = vec![0; want];
                let n = storage(vm)?
                    .read_at(&p, pos as u64, &mut chunk)
                    .map_err(io_error)?;
                if n > chunk.len() {
                    return Err(atomic_throw("Ljava/io/IOException;", "invalid read count"));
                }
                chunk.truncate(n);
                if chunk.is_empty() {
                    return Ok(Some(Value::Int(-1)));
                }
                let elems = vm
                    .heap
                    .array_elems_mut(arr)
                    .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_STATE", e)))?;
                for (i, b) in chunk.iter().enumerate() {
                    elems[off + i] = Value::Int(*b as i8 as i32);
                }
                set_pos(vm, pos + chunk.len())?;
                Value::Int(chunk.len() as i32)
            }
        }
        (CHANNEL, "lock" | "tryLock") => return reference_lock(vm, &p),
        (FLOCK, "release") => Value::Null,
        (CHANNEL, "size") => Value::Long(len as i64),
        (CHANNEL, "position") if sig.starts_with("()") => Value::Long(pos as i64),
        (CHANNEL, "isOpen") | (FLOCK, "isValid") => Value::Int(1),
        (FLOCK, "isShared") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn reference_fd(
    vm: &mut Engine,
    class: &str,
    owner: ObjRef,
    p: &str,
    pos: usize,
) -> Result<Option<Value>, VmExit> {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    if matches!(storage(vm)?, crate::java_file::Io::Reference(_)) {
        let value = with_path(vm, FD, p)?;
        if let Value::Obj(r) = value {
            let flags = if class == FOS { 1 | 0o2000 } else { 0 };
            if let Some(fd) = crate::jfile::os_register_path(
                #[cfg(not(feature = "desktop-runtime"))]
                vm,
                p,
                flags,
                pos as u64,
            ) {
                put(vm, r, "descriptor", Value::Int(fd))?;
            }
            put(vm, r, "rd_stream", Value::Obj(owner))?;
        }
        return Ok(Some(value));
    }
    let _ = (vm, class, owner, p, pos);
    Err(crate::err::vm_error(
        "API_UNAVAILABLE",
        "FileDescriptor: shared native handle adapter is not connected",
    )
    .into())
}
fn reference_fd_valid(vm: &Engine, recv: ObjRef) -> Result<Option<Value>, VmExit> {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    if matches!(storage(vm)?, crate::java_file::Io::Reference(_)) {
        let valid = match vm.heap.get_field(recv, "descriptor") {
            Ok(Value::Int(fd)) => crate::jfile::os_fd_valid(
                #[cfg(not(feature = "desktop-runtime"))]
                vm,
                fd,
            ),
            _ => true,
        };
        return Ok(Some(Value::Int(i32::from(valid))));
    }
    let _ = (vm, recv);
    Err(crate::err::vm_error(
        "API_UNAVAILABLE",
        "FileDescriptor.valid: native handle adapter is not connected",
    )
    .into())
}
fn reference_lock(vm: &mut Engine, p: &str) -> Result<Option<Value>, VmExit> {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    if matches!(storage(vm)?, crate::java_file::Io::Reference(_)) {
        return Ok(Some(with_path(vm, FLOCK, p)?));
    }
    let _ = (vm, p);
    Err(crate::err::vm_error(
        "API_UNAVAILABLE",
        "FileChannel.lock: shared native lock adapter is not connected",
    )
    .into())
}
pub fn method_metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<crate::framework_host::HostMethodKind> {
    use crate::framework_host::HostMethodKind::{Constructor, Instance};
    let supported = match class {
        FIS => matches!(
            (name, sig),
            ("<init>", "(Ljava/lang/String;)V" | "(Ljava/io/File;)V")
                | ("read", "()I" | "([B)I" | "([BII)I")
                | ("available", "()I")
                | ("skip", "(J)J")
                | ("close", "()V")
                | ("getChannel", "()Ljava/nio/channels/FileChannel;")
        ),
        FOS => matches!(
            (name, sig),
            (
                "<init>",
                "(Ljava/lang/String;)V"
                    | "(Ljava/io/File;)V"
                    | "(Ljava/lang/String;Z)V"
                    | "(Ljava/io/File;Z)V"
            ) | ("write", "(I)V" | "([B)V" | "([BII)V")
                | ("close" | "flush", "()V")
                | ("getChannel", "()Ljava/nio/channels/FileChannel;")
        ),
        CHANNEL => matches!(
            (name, sig),
            ("close", "()V") | ("force", "(Z)V") | ("size" | "position", "()J") | ("isOpen", "()Z")
        ),
        _ => false,
    };
    supported.then_some(if name == "<init>" {
        Constructor
    } else {
        Instance
    })
}
