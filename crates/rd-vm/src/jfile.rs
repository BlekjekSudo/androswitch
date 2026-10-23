//! `java.io.File` over a per-VM virtual filesystem, plus the `Context`
//! directory getters (`getFilesDir`, `getCacheDir`, `getDatabasePath`,
//! `getDir`, …).
//!
//! A File is a host object holding its normalized path in `rd_path`. The
//! virtual filesystem is a set of directories plus a map from file path to
//! byte length. App directories under `/data/user/0/<pkg>/` are created on
//! first request, as Android does. Host disk is never touched, so runs are
//! sandboxed and deterministic.
//!
//! `FileOutputStream`/`FileInputStream` read and write the virtual file
//! contents. `getChannel()` returns a `FileChannel` whose `lock`/`tryLock`
//! always succeed: there is one process, so no other process can hold an
//! advisory lock. `FileChannel.map` is not supported.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const FILE: &str = "Ljava/io/File;";
const ATOMIC_FILE: &str = "Landroid/util/AtomicFile;";

#[derive(Default)]
struct Vfs {
    dirs: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
}

thread_local! {
    static VFS: RefCell<Vfs> = RefCell::new(Vfs::default());
}

fn normalize(p: &str) -> String {
    let abs = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if abs {
        format!("/{joined}")
    } else if joined.is_empty() {
        String::new()
    } else {
        joined
    }
}

fn absolute(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        normalize(&format!("/{p}"))
    }
}

fn parent_of(p: &str) -> Option<String> {
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

fn mkdirs(p: &str) {
    let p = absolute(p);
    VFS.with(|v| {
        let mut v = v.borrow_mut();
        let mut cur = String::new();
        for seg in p.split('/').filter(|s| !s.is_empty()) {
            cur.push('/');
            cur.push_str(seg);
            v.dirs.insert(cur.clone());
        }
    });
}

fn is_dir(p: &str) -> bool {
    let p = absolute(p);
    p == "/" || VFS.with(|v| v.borrow().dirs.contains(&p))
}
fn is_file(p: &str) -> bool {
    let p = absolute(p);
    VFS.with(|v| v.borrow().files.contains_key(&p))
}

pub(crate) fn new_file(vm: &mut Engine, path: &str) -> Result<Value, VmExit> {
    let s = crate::intrinsics::alloc_string(vm, normalize(path))?;
    let r = vm
        .heap
        .alloc_instance(
            FILE.to_string(),
            vec![("rd_path".to_string(), Value::Obj(s))],
        )
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

/// Android's deterministic emulated shared-storage root, held only in the
/// VM's virtual filesystem (never mapped to host disk).
pub(crate) fn external_storage_path(vm: &mut Engine, path: &str) -> Result<Value, VmExit> {
    mkdirs("/storage/emulated/0");
    new_file(vm, path)
}

fn path_of(vm: &Engine, r: ObjRef) -> String {
    match vm.heap.get_field(r, "rd_path") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).map(str::to_string).unwrap_or_default(),
        _ => String::new(),
    }
}

fn str_arg(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

fn s(vm: &mut Engine, x: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, x)?))
}

/// App-private directory `/data/user/0/<pkg>/<sub>`, created on demand.
fn app_dir(vm: &mut Engine, sub: &str) -> Result<Value, VmExit> {
    let pkg = if vm.fw.package_name.is_empty() {
        "app".to_string()
    } else {
        vm.fw.package_name.clone()
    };
    let p = if sub.is_empty() {
        format!("/data/user/0/{pkg}")
    } else {
        format!("/data/user/0/{pkg}/{sub}")
    };
    mkdirs(&p);
    new_file(vm, &p)
}

fn app_file_path(vm: &mut Engine, args: &[Value]) -> Result<String, VmExit> {
    let name = match args.first() {
        Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
        _ => {
            return Err(
                crate::err::Throwable::new("Ljava/lang/NullPointerException;", "name").into(),
            )
        }
    };
    if name.contains('/') || name.contains('\\') || matches!(name.as_str(), "." | "..") {
        return Err(crate::err::Throwable::new(
            "Ljava/lang/IllegalArgumentException;",
            format!("File {name} contains a path separator"),
        )
        .into());
    }
    let Value::Obj(dir) = app_dir(vm, "files")? else {
        return Err(crate::err::vm_error("VM_TYPE_ERROR", "files dir is not a File").into());
    };
    Ok(format!("{}/{name}", path_of(vm, dir)))
}

/// Context directory getters. `None` when `name` is not one of them.
pub fn context_dirs(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("getFilesDir", "()Ljava/io/File;") => app_dir(vm, "files")?,
        ("getCacheDir", "()Ljava/io/File;") => app_dir(vm, "cache")?,
        ("getCodeCacheDir", "()Ljava/io/File;") => app_dir(vm, "code_cache")?,
        ("getNoBackupFilesDir", "()Ljava/io/File;") => app_dir(vm, "no_backup")?,
        ("getDataDir", "()Ljava/io/File;") => app_dir(vm, "")?,
        ("getDatabasePath", "(Ljava/lang/String;)Ljava/io/File;") => {
            let n = str_arg(vm, args.first()).unwrap_or_default();
            if n.starts_with('/') {
                new_file(vm, &n)?
            } else {
                app_dir(vm, "databases")?;
                let pkg = vm.fw.package_name.clone();
                new_file(vm, &format!("/data/user/0/{pkg}/databases/{n}"))?
            }
        }
        ("getDir", "(Ljava/lang/String;I)Ljava/io/File;") => {
            let n = str_arg(vm, args.first()).unwrap_or_default();
            app_dir(vm, &format!("app_{n}"))?
        }
        ("openFileInput", "(Ljava/lang/String;)Ljava/io/FileInputStream;") => {
            let path = app_file_path(vm, args)?;
            let stream = vm
                .heap
                .alloc_instance(FIS.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            let path_ref = crate::intrinsics::alloc_string(vm, path)?;
            if stream_intrinsic(
                vm,
                FIS,
                "<init>",
                "(Ljava/lang/String;)V",
                stream,
                &[Value::Obj(path_ref)],
            )?
            .is_none()
            {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "FileInputStream constructor was not handled",
                )
                .into());
            }
            Value::Obj(stream)
        }
        ("openFileOutput", "(Ljava/lang/String;I)Ljava/io/FileOutputStream;") => {
            let path = app_file_path(vm, args)?;
            let mode = args
                .get(1)
                .and_then(|value| value.as_int().ok())
                .unwrap_or(0);
            let append = Value::Int(i32::from(mode & 0x8000 != 0));
            let stream = vm
                .heap
                .alloc_instance(FOS.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            let path_ref = crate::intrinsics::alloc_string(vm, path)?;
            if stream_intrinsic(
                vm,
                FOS,
                "<init>",
                "(Ljava/lang/String;Z)V",
                stream,
                &[Value::Obj(path_ref), append],
            )?
            .is_none()
            {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "FileOutputStream constructor was not handled",
                )
                .into());
            }
            Value::Obj(stream)
        }
        ("getExternalFilesDir", _) | ("getExternalCacheDir", _) => {
            let pkg = vm.fw.package_name.clone();
            let sub = if name == "getExternalCacheDir" {
                "cache".to_string()
            } else {
                match str_arg(vm, args.first()) {
                    Some(t) => format!("files/{t}"),
                    None => "files".into(),
                }
            };
            let p = format!("/storage/emulated/0/Android/data/{pkg}/{sub}");
            mkdirs(&p);
            new_file(vm, &p)?
        }
        ("databaseList", _) | ("fileList", _) => {
            let pkg = vm.fw.package_name.clone();
            let dir = if name == "databaseList" {
                format!("/data/user/0/{pkg}/databases/")
            } else {
                format!("/data/user/0/{pkg}/files/")
            };
            let names: Vec<String> = VFS.with(|v| {
                v.borrow()
                    .files
                    .keys()
                    .filter_map(|k| k.strip_prefix(&dir))
                    .filter(|k| !k.contains('/'))
                    .map(str::to_string)
                    .collect()
            });
            let mut vals = Vec::new();
            for n in names {
                vals.push(s(vm, n)?);
            }
            crate::arrays::new_array(vm, "[Ljava/lang/String;", ElemKind::Obj, vals)?
        }
        ("deleteDatabase", "(Ljava/lang/String;)Z") => {
            let n = str_arg(vm, args.first()).unwrap_or_default();
            let pkg = vm.fw.package_name.clone();
            let p = format!("/data/user/0/{pkg}/databases/{n}");
            // Close/forget any open SQLite handle first; otherwise its next
            // committed mutation would rewrite (resurrect) the deleted image.
            let was_open = crate::jsqlite::forget_database(vm, &p);
            let deleted = VFS.with(|v| v.borrow_mut().files.remove(&p).is_some());
            Value::Int((was_open || deleted) as i32)
        }
        _ => return Ok(None),
    }))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != FILE {
        return Ok(None);
    }
    Ok(Some(match name {
        "createTempFile" => {
            let pre = str_arg(vm, args.first()).unwrap_or_default();
            let suf = str_arg(vm, args.get(1)).unwrap_or_else(|| ".tmp".into());
            let dir = match args.get(2) {
                Some(Value::Obj(d)) => path_of(vm, *d),
                _ => "/data/local/tmp".into(),
            };
            mkdirs(&dir);
            let n = VFS.with(|v| v.borrow().files.len());
            let p = format!("{dir}/{pre}{n}{suf}");
            VFS.with(|v| v.borrow_mut().files.insert(absolute(&p), Vec::new()));
            new_file(vm, &p)?
        }
        _ => return Ok(None),
    }))
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
    Ok((absolute(&path_of(vm, base)), absolute(&path_of(vm, backup))))
}

fn atomic_rename(from: &str, to: &str) -> bool {
    VFS.with(|v| {
        let mut v = v.borrow_mut();
        let Some(bytes) = v.files.remove(from) else {
            return false;
        };
        v.files.insert(to.to_string(), bytes);
        true
    })
}

fn atomic_create_stream(vm: &mut Engine, class: &str, path: &str) -> Result<Value, VmExit> {
    let stream = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    let path_ref = crate::intrinsics::alloc_string(vm, path.to_string())?;
    if stream_intrinsic(
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

fn atomic_restore_backup(base: &str, backup: &str) {
    if is_file(backup) {
        delete_file(base);
        atomic_rename(backup, base);
    }
}

fn atomic_open_read(vm: &mut Engine, recv: ObjRef) -> Result<Value, VmExit> {
    let (base, backup) = atomic_paths(vm, recv)?;
    atomic_restore_backup(&base, &backup);
    atomic_create_stream(vm, FIS, &base)
}

fn atomic_file_intrinsic(
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
            if is_file(&base) || is_dir(&base) {
                if !is_file(&backup) && !is_dir(&backup) {
                    if !is_file(&base) || !atomic_rename(&base, &backup) {
                        return Err(atomic_throw(
                            "Ljava/io/IOException;",
                            format!("Couldn't rename file {base} to backup file {backup}"),
                        ));
                    }
                } else {
                    // A pre-existing backup is the older committed state;
                    // the current base is an interrupted replacement.
                    delete_file(&base);
                }
            }
            match atomic_create_stream(vm, FOS, &base) {
                Ok(stream) => Ok(Some(stream)),
                Err(error) => {
                    let Some(parent) = parent_of(&base) else {
                        return Err(error);
                    };
                    mkdirs(&parent);
                    if !is_dir(&parent) {
                        return Err(error);
                    }
                    Ok(Some(atomic_create_stream(vm, FOS, &base)?))
                }
            }
        }
        ("finishWrite", "(Ljava/io/FileOutputStream;)V") => {
            atomic_close_stream(vm, args);
            let (_, backup) = atomic_paths(vm, recv)?;
            delete_file(&backup);
            Ok(Some(Value::Null))
        }
        ("failWrite", "(Ljava/io/FileOutputStream;)V") => {
            atomic_close_stream(vm, args);
            let (base, backup) = atomic_paths(vm, recv)?;
            delete_file(&base);
            if is_file(&backup) {
                atomic_rename(&backup, &base);
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
            delete_file(&base);
            delete_file(&backup);
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if STREAMS.contains(&recv_class) {
        return stream_intrinsic(vm, recv_class, name, sig, recv, args);
    }
    if recv_class == ATOMIC_FILE {
        return atomic_file_intrinsic(vm, name, sig, recv, args);
    }
    if recv_class != FILE {
        return Ok(None);
    }
    if name == "<init>" {
        let path = match sig {
            "(Ljava/lang/String;)V" => str_arg(vm, args.first()).unwrap_or_default(),
            "(Ljava/io/File;Ljava/lang/String;)V" | "(Ljava/lang/String;Ljava/lang/String;)V" => {
                let parent = match args.first() {
                    Some(Value::Obj(p)) if sig.starts_with("(Ljava/io/File;") => {
                        Some(path_of(vm, *p))
                    }
                    other => str_arg(vm, other),
                };
                let child = str_arg(vm, args.get(1)).unwrap_or_default();
                match parent {
                    Some(p) if !p.is_empty() => format!("{p}/{child}"),
                    _ => child,
                }
            }
            _ => return Ok(None),
        };
        let v = crate::intrinsics::alloc_string(vm, normalize(&path))?;
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_path", Value::Obj(v)))?;
        return Ok(Some(Value::Null));
    }
    let p = path_of(vm, recv);
    let abs = absolute(&p);
    let b = |x: bool| Value::Int(x as i32);
    Ok(Some(match name {
        "getPath" | "toString" => s(vm, p)?,
        "getAbsolutePath" | "getCanonicalPath" => s(vm, abs)?,
        "getAbsoluteFile" | "getCanonicalFile" => new_file(vm, &abs)?,
        "getName" => {
            let n = p.rsplit('/').next().unwrap_or("").to_string();
            s(vm, n)?
        }
        "getParent" => match parent_of(&p) {
            Some(x) => s(vm, x)?,
            None => Value::Null,
        },
        "getParentFile" => match parent_of(&p) {
            Some(x) => new_file(vm, &x)?,
            None => Value::Null,
        },
        "exists" => b(is_dir(&p) || is_file(&p)),
        "isDirectory" => b(is_dir(&p)),
        "isFile" => b(is_file(&p)),
        "isAbsolute" => b(p.starts_with('/')),
        "isHidden" => b(p.rsplit('/').next().is_some_and(|n| n.starts_with('.'))),
        "canRead" | "canWrite" | "canExecute" => b(is_dir(&p) || is_file(&p)),
        "length" => Value::Long(VFS.with(|v| {
            v.borrow()
                .files
                .get(&abs)
                .map(|f| f.len() as i64)
                .unwrap_or(0)
        })),
        "lastModified" => Value::Long(if is_dir(&p) || is_file(&p) {
            crate::jlsystem::epoch_ms()
        } else {
            0
        }),
        "mkdir" => {
            let ok = !is_dir(&p) && !is_file(&p) && parent_of(&abs).is_some_and(|x| is_dir(&x));
            if ok {
                VFS.with(|v| v.borrow_mut().dirs.insert(abs.clone()));
            }
            b(ok)
        }
        "mkdirs" => {
            let ok = !is_dir(&p) && !is_file(&p);
            if ok {
                mkdirs(&abs);
            }
            b(ok)
        }
        "createNewFile" => {
            let ok = !is_dir(&p) && !is_file(&p);
            if ok {
                if let Some(par) = parent_of(&abs) {
                    if !is_dir(&par) {
                        return Err(crate::err::Throwable::new(
                            "Ljava/io/IOException;",
                            "No such file or directory".to_string(),
                        )
                        .into());
                    }
                }
                VFS.with(|v| v.borrow_mut().files.insert(abs.clone(), Vec::new()));
            }
            b(ok)
        }
        "delete" => {
            let ok = VFS.with(|v| {
                let mut v = v.borrow_mut();
                if v.files.remove(&abs).is_some() {
                    return true;
                }
                let prefix = format!("{abs}/");
                let empty = !v.dirs.iter().any(|d| d.starts_with(&prefix))
                    && !v.files.keys().any(|f| f.starts_with(&prefix));
                empty && v.dirs.remove(&abs)
            });
            b(ok)
        }
        "deleteOnExit" | "setLastModified" | "setReadable" | "setWritable" | "setExecutable"
        | "setReadOnly" => {
            if sig.ends_with('V') {
                Value::Null
            } else {
                b(true)
            }
        }
        "renameTo" => {
            let dst = match args.first() {
                Some(Value::Obj(d)) => absolute(&path_of(vm, *d)),
                _ => return Ok(Some(b(false))),
            };
            let ok = VFS.with(|v| {
                let mut v = v.borrow_mut();
                if let Some(len) = v.files.remove(&abs) {
                    v.files.insert(dst.clone(), len);
                    true
                } else if v.dirs.remove(&abs) {
                    v.dirs.insert(dst.clone());
                    true
                } else {
                    false
                }
            });
            b(ok)
        }
        "list" | "listFiles" if sig.starts_with("()") => {
            if !is_dir(&p) {
                return Ok(Some(Value::Null));
            }
            let prefix = if abs == "/" {
                "/".to_string()
            } else {
                format!("{abs}/")
            };
            let names: Vec<String> = VFS.with(|v| {
                let v = v.borrow();
                v.dirs
                    .iter()
                    .chain(v.files.keys())
                    .filter_map(|k| k.strip_prefix(&prefix))
                    .filter(|k| !k.is_empty() && !k.contains('/'))
                    .map(str::to_string)
                    .collect()
            });
            let mut vals = Vec::new();
            for n in names {
                vals.push(if name == "list" {
                    s(vm, n)?
                } else {
                    new_file(vm, &format!("{prefix}{n}"))?
                });
            }
            let desc = if name == "list" {
                "[Ljava/lang/String;"
            } else {
                "[Ljava/io/File;"
            };
            crate::arrays::new_array(vm, desc, ElemKind::Obj, vals)?
        }
        "equals" => {
            let o = match args.first() {
                Some(Value::Obj(o)) if vm.heap.class_of(*o).map(|c| c == FILE).unwrap_or(false) => {
                    path_of(vm, *o)
                }
                _ => return Ok(Some(b(false))),
            };
            b(o == p)
        }
        "hashCode" => {
            let h = p
                .encode_utf16()
                .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
            Value::Int(h ^ 1234321)
        }
        "compareTo" => {
            let o = match args.first() {
                Some(Value::Obj(o)) => path_of(vm, *o),
                _ => String::new(),
            };
            Value::Int(p.cmp(&o) as i32)
        }
        "getFreeSpace" | "getUsableSpace" => Value::Long(4 << 30),
        "getTotalSpace" => Value::Long(8 << 30),
        _ => return Ok(None),
    }))
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

fn stream_intrinsic(
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
        let abs = absolute(&path);
        if class == FOS {
            let append = matches!(args.get(1), Some(Value::Int(1)));
            if parent_of(&abs).is_some_and(|p| !is_dir(&p)) || is_dir(&abs) {
                return Err(fnf(&abs));
            }
            VFS.with(|v| {
                let mut v = v.borrow_mut();
                let e = v.files.entry(abs.clone()).or_default();
                if !append && !sig.contains("FileDescriptor") {
                    e.clear();
                }
            });
        } else if class == FIS && !is_file(&abs) {
            return Err(fnf(&abs));
        }
        let sv = crate::intrinsics::alloc_string(vm, abs)?;
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_path", Value::Obj(sv)))?;
        return Ok(Some(Value::Null));
    }
    let p = path_of(vm, recv);
    let pos = match vm.heap.get_field(recv, "rd_pos") {
        Ok(Value::Long(x)) => x as usize,
        _ => 0,
    };
    let set_pos = |vm: &mut Engine, x: usize| {
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_pos", Value::Long(x as i64)))
    };
    let len = VFS.with(|v| v.borrow().files.get(&p).map(Vec::len).unwrap_or(0));
    Ok(Some(match (class, name) {
        (FOS, "write") => {
            let data = match sig {
                "(I)V" => vec![int_arg(args, 0) as u8],
                "([B)V" => bytes_of(vm, args.first()),
                _ => {
                    let all = bytes_of(vm, args.first());
                    let (off, n) = (int_arg(args, 1) as usize, int_arg(args, 2) as usize);
                    all.get(off..off.saturating_add(n))
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default()
                }
            };
            VFS.with(|v| {
                v.borrow_mut()
                    .files
                    .entry(p.clone())
                    .or_default()
                    .extend_from_slice(&data)
            });
            Value::Null
        }
        (FOS | FIS, "flush" | "close")
        | (CHANNEL, "close" | "force")
        | (FLOCK, "release" | "close") => Value::Null,
        (FOS | FIS, "getChannel") => with_path(vm, CHANNEL, &p)?,
        (FOS | FIS, "getFD") => with_path(vm, FD, &p)?,
        (FD, "valid") => Value::Int(1),
        (FD, "sync") => Value::Null,
        (FIS, "available") => Value::Int(len.saturating_sub(pos) as i32),
        (FIS, "skip") => {
            let n = (int_arg(args, 0).max(0) as usize).min(len.saturating_sub(pos));
            set_pos(vm, pos + n)?;
            Value::Long(n as i64)
        }
        (FIS, "read") => {
            if sig == "()I" {
                let b = VFS.with(|v| v.borrow().files.get(&p).and_then(|f| f.get(pos).copied()));
                match b {
                    Some(b) => {
                        set_pos(vm, pos + 1)?;
                        Value::Int(b as i32)
                    }
                    None => Value::Int(-1),
                }
            } else {
                let Some(Value::Obj(arr)) = args.first().cloned() else {
                    return Ok(None);
                };
                let cap = vm
                    .heap
                    .as_array_elems(arr)
                    .map(|(e, _)| e.len())
                    .unwrap_or(0);
                let (off, want) = if sig == "([B)I" {
                    (0, cap)
                } else {
                    (int_arg(args, 1) as usize, int_arg(args, 2) as usize)
                };
                if want == 0 {
                    return Ok(Some(Value::Int(0)));
                }
                let chunk: Vec<u8> = VFS.with(|v| {
                    v.borrow()
                        .files
                        .get(&p)
                        .map(|f| f[pos.min(f.len())..(pos + want).min(f.len())].to_vec())
                        .unwrap_or_default()
                });
                if chunk.is_empty() {
                    return Ok(Some(Value::Int(-1)));
                }
                let elems = vm
                    .heap
                    .array_elems_mut(arr)
                    .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_STATE", e)))?;
                for (i, b) in chunk.iter().enumerate() {
                    if let Some(slot) = elems.get_mut(off + i) {
                        *slot = Value::Int(*b as i8 as i32);
                    }
                }
                set_pos(vm, pos + chunk.len())?;
                Value::Int(chunk.len() as i32)
            }
        }
        (CHANNEL, "lock" | "tryLock") => with_path(vm, FLOCK, &p)?,
        (CHANNEL, "size") => Value::Long(len as i64),
        (CHANNEL, "position") if sig.starts_with("()") => Value::Long(pos as i64),
        (CHANNEL, "isOpen") | (FLOCK, "isValid") => Value::Int(1),
        (FLOCK, "isShared") => Value::Int(0),
        _ => return Ok(None),
    }))
}

/// Creates `p`'s parent directories (public entry for other host modules).
pub(crate) fn mkdirs_pub(p: &str) {
    mkdirs(p);
}

/// Ensures a file exists at `p` (parents included), keeping its content.
pub(crate) fn touch(p: &str) {
    let p = absolute(p);
    if let Some(parent) = parent_of(&p) {
        mkdirs(&parent);
    }
    VFS.with(|v| {
        v.borrow_mut().files.entry(p).or_default();
    });
}

/// Reads a virtual file's bytes for another host module (e.g. SQLite loads
/// its serialized database image from the app-private VFS path).
pub(crate) fn read_bytes(p: &str) -> Option<Vec<u8>> {
    let p = absolute(p);
    VFS.with(|v| v.borrow().files.get(&p).cloned())
}

/// Replaces a virtual file's bytes, creating parent directories as Android
/// does for app-private storage paths.
pub(crate) fn write_bytes(p: &str, bytes: Vec<u8>) {
    let p = absolute(p);
    if let Some(parent) = parent_of(&p) {
        mkdirs(&parent);
    }
    VFS.with(|v| {
        v.borrow_mut().files.insert(p, bytes);
    });
}

/// Deletes a virtual file. Returns whether a file existed.
pub(crate) fn delete_file(p: &str) -> bool {
    let p = absolute(p);
    VFS.with(|v| v.borrow_mut().files.remove(&p).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_vfs() {
        assert_eq!(normalize("/a/./b/../c/"), "/a/c");
        assert_eq!(parent_of("/a/c").as_deref(), Some("/a"));
        assert_eq!(parent_of("/a").as_deref(), Some("/"));
        mkdirs("/x/y/z");
        assert!(is_dir("/x/y") && is_dir("/x/y/z") && !is_file("/x/y"));
    }
}
