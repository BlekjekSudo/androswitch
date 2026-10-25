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
const MAX_VFS_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_VFS_TOTAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
struct VfsMeta {
    mode: i32,
    uid: i32,
    gid: i32,
    ino: i64,
    atime_ms: i64,
    mtime_ms: i64,
    ctime_ms: i64,
}

struct OpenDesc {
    path: String,
    flags: i32,
    pos: u64,
}

#[derive(Default)]
struct Vfs {
    dirs: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
    meta: BTreeMap<String, VfsMeta>,
    next_ino: i64,
    open_descs: BTreeMap<i64, OpenDesc>,
    fds: BTreeMap<i32, i64>,
    next_desc: i64,
}

thread_local! {
    static VFS: RefCell<Vfs> = RefCell::new(Vfs::default());
}

fn normalize(p: &str) -> String {
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

fn absolute(p: &str) -> String {
    if p.starts_with('/') {
        normalize(p)
    } else {
        normalize(&format!("/{p}"))
    }
}

fn vfs_used_excluding(vfs: &Vfs, exclude: &str) -> usize {
    vfs.files
        .iter()
        .filter(|(k, _)| k.as_str() != exclude)
        .fold(0usize, |acc, (_, b)| acc.saturating_add(b.len()))
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
    if p.is_empty() {
        return false;
    }
    let p = absolute(p);
    p == "/" || VFS.with(|v| v.borrow().dirs.contains(&p))
}
fn is_file(p: &str) -> bool {
    if p.is_empty() {
        return false;
    }
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
            let Some(Value::Obj(pre_ref)) = args.first() else {
                return Err(atomic_throw(
                    "Ljava/lang/NullPointerException;",
                    "prefix == null",
                ));
            };
            let pre = vm
                .heap
                .as_str(*pre_ref)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                .to_string();
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
            "(Ljava/lang/String;)V" => {
                let Some(Value::Obj(r)) = args.first() else {
                    return Err(atomic_throw(
                        "Ljava/lang/NullPointerException;",
                        "pathname == null",
                    ));
                };
                vm.heap
                    .as_str(*r)
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                    .to_string()
            }
            "(Ljava/io/File;Ljava/lang/String;)V" | "(Ljava/lang/String;Ljava/lang/String;)V" => {
                let Some(Value::Obj(child_ref)) = args.get(1) else {
                    return Err(atomic_throw(
                        "Ljava/lang/NullPointerException;",
                        "child == null",
                    ));
                };
                let child = vm
                    .heap
                    .as_str(*child_ref)
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                    .to_string();
                let parent = match args.first() {
                    None | Some(Value::Null) => None,
                    Some(Value::Obj(p)) if sig.starts_with("(Ljava/io/File;") => {
                        Some(path_of(vm, *p))
                    }
                    Some(Value::Obj(p)) => Some(
                        vm.heap
                            .as_str(*p)
                            .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                            .to_string(),
                    ),
                    _ => None,
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
        "isHidden" => b(!p.is_empty()
            && p != "."
            && p != ".."
            && p.rsplit('/').next().is_some_and(|n| n.starts_with('.'))),
        "canRead" | "canWrite" | "canExecute" => b(is_dir(&p) || is_file(&p)),
        "length" => Value::Long(if p.is_empty() {
            0
        } else {
            VFS.with(|v| {
                v.borrow()
                    .files
                    .get(&abs)
                    .map(|f| f.len() as i64)
                    .unwrap_or(0)
            })
        }),
        "lastModified" => Value::Long(if is_dir(&p) || is_file(&p) {
            crate::jlsystem::epoch_ms()
        } else {
            0
        }),
        "mkdir" => {
            let ok = !p.is_empty()
                && !is_dir(&p)
                && !is_file(&p)
                && parent_of(&abs).is_some_and(|x| is_dir(&x));
            if ok {
                VFS.with(|v| v.borrow_mut().dirs.insert(abs.clone()));
            }
            b(ok)
        }
        "mkdirs" => {
            let ok = !p.is_empty() && !is_dir(&p) && !is_file(&p);
            if ok {
                mkdirs(&abs);
            }
            b(ok)
        }
        "createNewFile" => {
            if p.is_empty() {
                return Err(atomic_throw(
                    "Ljava/io/IOException;",
                    "No such file or directory",
                ));
            }
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
            if p.is_empty() {
                return Ok(Some(b(false)));
            }
            let ok = VFS.with(|v| {
                let mut v = v.borrow_mut();
                if v.files.remove(&abs).is_some() {
                    v.meta.remove(&abs);
                    return true;
                }
                let prefix = format!("{abs}/");
                let empty = !v.dirs.iter().any(|d| d.starts_with(&prefix))
                    && !v.files.keys().any(|f| f.starts_with(&prefix));
                if empty && v.dirs.remove(&abs) {
                    v.meta.remove(&abs);
                    true
                } else {
                    false
                }
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
            let Some(Value::Obj(d)) = args.first() else {
                return Err(atomic_throw(
                    "Ljava/lang/NullPointerException;",
                    "dest == null",
                ));
            };
            let dst_raw = path_of(vm, *d);
            if p.is_empty() || dst_raw.is_empty() {
                return Ok(Some(b(false)));
            }
            let dst = absolute(&dst_raw);
            b(os_rename(&abs, &dst).is_ok())
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
            let Some(Value::Obj(o)) = args.first() else {
                return Err(atomic_throw(
                    "Ljava/lang/NullPointerException;",
                    "pathname == null",
                ));
            };
            let o = path_of(vm, *o);
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
            let new_len = VFS.with(|v| -> Result<usize, VmExit> {
                let mut vfs = v.borrow_mut();
                let cur_len = vfs.files.get(&p).map_or(0, Vec::len);
                let target_len = cur_len
                    .checked_add(data.len())
                    .ok_or_else(|| atomic_throw("Ljava/io/IOException;", "File too large"))?;
                if target_len > MAX_VFS_FILE_BYTES {
                    return Err(atomic_throw("Ljava/io/IOException;", "File too large"));
                }
                let other = vfs_used_excluding(&vfs, &p);
                if other.saturating_add(target_len) > MAX_VFS_TOTAL_BYTES {
                    return Err(atomic_throw(
                        "Ljava/io/IOException;",
                        "No space left on device",
                    ));
                }
                let entry = vfs.files.entry(p.clone()).or_default();
                entry.extend_from_slice(&data);
                Ok(entry.len())
            })?;
            set_pos(vm, new_len)?;
            Value::Null
        }
        (FOS | FIS, "flush" | "close")
        | (CHANNEL, "close" | "force")
        | (FLOCK, "release" | "close") => Value::Null,
        (FOS | FIS, "getChannel") => with_path(vm, CHANNEL, &p)?,
        (FOS | FIS, "getFD") => {
            let value = with_path(vm, FD, &p)?;
            if let Value::Obj(fd_obj) = value {
                let flags = if class == FOS {
                    OS_O_WRONLY | OS_O_APPEND
                } else {
                    OS_O_RDONLY
                };
                if let Some(fd) = os_register_path(&p, flags, pos as u64) {
                    crate::err::put_field_result(vm.heap.put_field(
                        fd_obj,
                        "descriptor",
                        Value::Int(fd),
                    ))?;
                }
                Value::Obj(fd_obj)
            } else {
                value
            }
        }
        (FD, "valid") => match vm.heap.get_field(recv, "descriptor") {
            Ok(Value::Int(fd)) => Value::Int(os_fd_valid(fd) as i32),
            _ => Value::Int(1),
        },
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
                let chunk: Vec<u8> = VFS.with(|v| {
                    let v = v.borrow();
                    let Some(f) = v.files.get(&p) else {
                        return Vec::new();
                    };
                    if pos >= f.len() {
                        return Vec::new();
                    }
                    let end = pos.saturating_add(want).min(f.len());
                    f[pos..end].to_vec()
                });
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

/// Issue #230: Android "clear data" / uninstall — apaga todo o
/// armazenamento do pacote e fecha os descritores abertos nesses
/// caminhos. Raízes (como no AOSP): o diretório privado
/// (`/data/user/0/<pkg>/`: files, databases SQLite, shared_prefs), o
/// alias legado dele (`/data/data/<pkg>/`) e os arquivos externos
/// específicos do app (`/storage/emulated/0/Android/data/<pkg>/`,
/// getExternalFilesDir/getExternalCacheDir). Sem isto,
/// `package_clear`/`package_remove` no MCP só resetavam permissões e
/// os dados "apagados" continuavam legíveis.
pub fn clear_package_data(package: &str) {
    let roots = [
        format!("/data/user/0/{package}"),
        format!("/data/data/{package}"),
        format!("/storage/emulated/0/Android/data/{package}"),
    ];
    let in_pkg = |p: &str| {
        roots
            .iter()
            .any(|root| p == root || p.strip_prefix(root).is_some_and(|s| s.starts_with('/')))
    };
    VFS.with(|cell| {
        let mut v = cell.borrow_mut();
        let dead_descs: Vec<i64> = v
            .open_descs
            .iter()
            .filter(|(_, d)| in_pkg(&d.path))
            .map(|(k, _)| *k)
            .collect();
        for k in &dead_descs {
            v.open_descs.remove(k);
        }
        v.fds.retain(|_, desc| !dead_descs.contains(desc));
        v.files.retain(|p, _| !in_pkg(p));
        v.meta.retain(|p, _| !in_pkg(p));
        v.dirs.retain(|p| !in_pkg(p));
    });
}

/// Issue #230: sessão nova = dispositivo limpo. Zera o VFS inteiro
/// (arquivos, metadados e descritores) — chamado na criação de cada
/// `AgentSession`, de onde nascem os cenários isolados do `rd test`.
pub fn reset_all() {
    VFS.with(|cell| *cell.borrow_mut() = Vfs::default());
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

    #[test]
    fn clear_package_data_isolates_packages_and_reset_all_clears() {
        reset_all();
        write_bytes("/data/user/0/com.example.a/files/x.txt", b"a".to_vec());
        write_bytes(
            "/storage/emulated/0/Android/data/com.example.a/files/ext.txt",
            b"e".to_vec(),
        );
        write_bytes("/data/user/0/com.example.b/files/y.txt", b"b".to_vec());
        write_bytes("/data/user/0/com.example.ab/files/z.txt", b"z".to_vec());
        clear_package_data("com.example.a");
        assert_eq!(
            read_bytes("/data/user/0/com.example.a/files/x.txt"),
            None,
            "#230: clear data apaga os arquivos do pacote"
        );
        assert_eq!(
            read_bytes("/storage/emulated/0/Android/data/com.example.a/files/ext.txt"),
            None,
            "#230: clear data apaga também os externos específicos do app"
        );
        assert_eq!(
            read_bytes("/data/user/0/com.example.ab/files/z.txt"),
            Some(b"z".to_vec()),
            "pacote com prefixo textual comum (com.example.ab) não é tocado"
        );
        assert_eq!(
            read_bytes("/data/user/0/com.example.b/files/y.txt"),
            Some(b"b".to_vec()),
            "pacote vizinho fica intacto"
        );
        reset_all();
        assert_eq!(
            read_bytes("/data/user/0/com.example.b/files/y.txt"),
            None,
            "#230: sessão nova zera o VFS inteiro"
        );
    }
}

// ---------------------------------------------------------------------------
// Bounded android.system.Os support over the same in-memory VFS.
//
// This is deliberately *not* a host-syscall bridge: paths are normalized into
// the VM's virtual filesystem, descriptors are synthetic entries in the VFS
// descriptor table, and file contents / metadata live only in VM memory.
// ---------------------------------------------------------------------------

pub(crate) const OS_EPERM: i32 = 1;
pub(crate) const OS_ENOENT: i32 = 2;
pub(crate) const OS_EBADF: i32 = 9;
pub(crate) const OS_EACCES: i32 = 13;
pub(crate) const OS_EEXIST: i32 = 17;
pub(crate) const OS_ENOTDIR: i32 = 20;
pub(crate) const OS_EISDIR: i32 = 21;
pub(crate) const OS_EINVAL: i32 = 22;
pub(crate) const OS_EFBIG: i32 = 27;
pub(crate) const OS_ENOSPC: i32 = 28;
pub(crate) const OS_ENOTEMPTY: i32 = 39;

const OS_O_RDONLY: i32 = 0;
const OS_O_WRONLY: i32 = 1;
const OS_O_RDWR: i32 = 2;
const OS_O_CREAT: i32 = 0o100;
const OS_O_EXCL: i32 = 0o200;
const OS_O_TRUNC: i32 = 0o1000;
const OS_O_APPEND: i32 = 0o2000;
const OS_O_DIRECTORY: i32 = 0o200000;
const OS_O_PATH: i32 = 0o10000000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OsPathKind {
    File,
    Dir,
    Missing,
    NotDir,
}

#[derive(Clone)]
pub(crate) struct OsStatData {
    pub dev: i64,
    pub ino: i64,
    pub mode: i32,
    pub nlink: i64,
    pub uid: i32,
    pub gid: i32,
    pub rdev: i64,
    pub size: i64,
    pub atime_ms: i64,
    pub mtime_ms: i64,
    pub ctime_ms: i64,
    pub blksize: i64,
    pub blocks: i64,
}

#[derive(Clone)]
pub(crate) struct OsStatVfsData {
    pub bsize: i64,
    pub frsize: i64,
    pub blocks: i64,
    pub bfree: i64,
    pub bavail: i64,
    pub files: i64,
    pub ffree: i64,
    pub favail: i64,
    pub fsid: i64,
    pub flag: i64,
    pub namemax: i64,
}

fn os_abs(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        absolute(path)
    }
}

fn os_classify(vfs: &Vfs, path: &str) -> OsPathKind {
    if path.is_empty() {
        return OsPathKind::Missing;
    }
    if vfs.files.contains_key(path) {
        return OsPathKind::File;
    }
    if path == "/" || vfs.dirs.contains(path) {
        return OsPathKind::Dir;
    }
    let mut current = String::new();
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        current.push('/');
        current.push_str(part);
        if vfs.files.contains_key(&current) {
            return OsPathKind::NotDir;
        }
    }
    OsPathKind::Missing
}

fn os_meta_mut(vfs: &mut Vfs, path: &str, kind: OsPathKind) -> VfsMeta {
    if let Some(meta) = vfs.meta.get(path) {
        return meta.clone();
    }
    if vfs.next_ino < 2 {
        vfs.next_ino = 2;
    }
    let ino = vfs.next_ino;
    vfs.next_ino += 1;
    let now = crate::jlsystem::epoch_ms();
    let meta = VfsMeta {
        mode: if kind == OsPathKind::Dir {
            0o040000 | 0o755
        } else {
            0o100000 | 0o644
        },
        uid: 10000,
        gid: 10000,
        ino,
        atime_ms: now,
        mtime_ms: now,
        ctime_ms: now,
    };
    vfs.meta.insert(path.to_string(), meta.clone());
    meta
}

fn os_parent_status(vfs: &Vfs, path: &str) -> Result<(), i32> {
    let Some(parent) = parent_of(path) else {
        return Ok(());
    };
    match os_classify(vfs, &parent) {
        OsPathKind::Dir => Ok(()),
        OsPathKind::File | OsPathKind::NotDir => Err(OS_ENOTDIR),
        OsPathKind::Missing => Err(OS_ENOENT),
    }
}

fn os_owner_perm(meta: &VfsMeta, read: bool, write: bool, exec: bool) -> bool {
    (!read || meta.mode & 0o400 != 0)
        && (!write || meta.mode & 0o200 != 0)
        && (!exec || meta.mode & 0o100 != 0)
}

fn os_stat_in(vfs: &mut Vfs, path: &str) -> Result<OsStatData, i32> {
    let kind = os_classify(vfs, path);
    match kind {
        OsPathKind::Missing => return Err(OS_ENOENT),
        OsPathKind::NotDir => return Err(OS_ENOTDIR),
        _ => {}
    }
    let meta = os_meta_mut(vfs, path, kind);
    let (size, nlink) = match kind {
        OsPathKind::File => (vfs.files.get(path).map_or(0, |bytes| bytes.len() as i64), 1),
        OsPathKind::Dir => {
            let prefix = if path == "/" {
                "/".to_string()
            } else {
                format!("{path}/")
            };
            let subdirs = vfs
                .dirs
                .iter()
                .filter(|dir| {
                    dir.strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
                })
                .count() as i64;
            (4096, 2 + subdirs)
        }
        _ => unreachable!(),
    };
    Ok(OsStatData {
        dev: 64769,
        ino: meta.ino,
        mode: meta.mode,
        nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: 0,
        size,
        atime_ms: meta.atime_ms,
        mtime_ms: meta.mtime_ms,
        ctime_ms: meta.ctime_ms,
        blksize: 4096,
        blocks: (size + 511) / 512,
    })
}

pub(crate) fn os_stat(path: &str) -> Result<OsStatData, i32> {
    let path = os_abs(path);
    VFS.with(|v| os_stat_in(&mut v.borrow_mut(), &path))
}

pub(crate) fn os_access(path: &str, mode: i32) -> Result<bool, i32> {
    if mode & !0o7 != 0 {
        return Err(OS_EINVAL);
    }
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let kind = os_classify(&vfs, &path);
        if !matches!(kind, OsPathKind::File | OsPathKind::Dir) {
            return Ok(false);
        }
        let meta = os_meta_mut(&mut vfs, &path, kind);
        Ok(os_owner_perm(
            &meta,
            mode & 4 != 0,
            mode & 2 != 0,
            mode & 1 != 0,
        ))
    })
}

fn os_alloc_desc(vfs: &mut Vfs, path: String, flags: i32, pos: u64) -> i32 {
    if vfs.next_desc < 1 {
        vfs.next_desc = 1;
    }
    let desc_id = vfs.next_desc;
    vfs.next_desc += 1;
    vfs.open_descs
        .insert(desc_id, OpenDesc { path, flags, pos });
    let mut fd = 3;
    while vfs.fds.contains_key(&fd) {
        fd += 1;
    }
    vfs.fds.insert(fd, desc_id);
    fd
}

pub(crate) fn os_open(path: &str, flags: i32, mode: i32) -> Result<i32, i32> {
    if path.is_empty() {
        return Err(OS_ENOENT);
    }
    let path = os_abs(path);
    let access = flags & 0o3;
    if access == 3 {
        return Err(OS_EINVAL);
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let kind = os_classify(&vfs, &path);
        match kind {
            OsPathKind::NotDir => return Err(OS_ENOTDIR),
            OsPathKind::Dir => {
                if flags & OS_O_CREAT != 0 && flags & OS_O_EXCL != 0 {
                    return Err(OS_EEXIST);
                }
                if access != OS_O_RDONLY || flags & OS_O_TRUNC != 0 {
                    return Err(OS_EISDIR);
                }
                let meta = os_meta_mut(&mut vfs, &path, kind);
                if !os_owner_perm(&meta, true, false, true) {
                    return Err(OS_EACCES);
                }
            }
            OsPathKind::File => {
                if flags & OS_O_CREAT != 0 && flags & OS_O_EXCL != 0 {
                    return Err(OS_EEXIST);
                }
                if flags & OS_O_DIRECTORY != 0 {
                    return Err(OS_ENOTDIR);
                }
                let meta = os_meta_mut(&mut vfs, &path, kind);
                let readable = access == OS_O_RDONLY || access == OS_O_RDWR;
                let writable = access == OS_O_WRONLY || access == OS_O_RDWR;
                if !os_owner_perm(&meta, readable, writable, false) {
                    return Err(OS_EACCES);
                }
                if flags & OS_O_TRUNC != 0 && writable {
                    if let Some(bytes) = vfs.files.get_mut(&path) {
                        bytes.clear();
                    }
                }
            }
            OsPathKind::Missing => {
                os_parent_status(&vfs, &path)?;
                if flags & OS_O_CREAT == 0 {
                    return Err(OS_ENOENT);
                }
                if flags & OS_O_DIRECTORY != 0 {
                    return Err(OS_ENOENT);
                }
                vfs.files.insert(path.clone(), Vec::new());
                let mut meta = os_meta_mut(&mut vfs, &path, OsPathKind::File);
                meta.mode = 0o100000 | (mode & 0o777 & !0o022);
                vfs.meta.insert(path.clone(), meta);
            }
        }
        Ok(os_alloc_desc(&mut vfs, path, flags, 0))
    })
}

pub(crate) fn os_register_path(path: &str, flags: i32, pos: u64) -> Option<i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        if os_classify(&vfs, &path) != OsPathKind::File {
            return None;
        }
        Some(os_alloc_desc(&mut vfs, path, flags, pos))
    })
}

fn os_desc_id(vfs: &Vfs, fd: i32) -> Result<i64, i32> {
    vfs.fds.get(&fd).copied().ok_or(OS_EBADF)
}

fn os_desc_snapshot(vfs: &Vfs, fd: i32) -> Result<(i64, String, i32, u64), i32> {
    let id = os_desc_id(vfs, fd)?;
    let desc = vfs.open_descs.get(&id).ok_or(OS_EBADF)?;
    Ok((id, desc.path.clone(), desc.flags, desc.pos))
}

fn os_cleanup_desc(vfs: &mut Vfs, desc_id: i64) {
    if !vfs.fds.values().any(|id| *id == desc_id) {
        vfs.open_descs.remove(&desc_id);
    }
}

pub(crate) fn os_close(fd: i32) -> Result<(), i32> {
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let desc_id = vfs.fds.remove(&fd).ok_or(OS_EBADF)?;
        os_cleanup_desc(&mut vfs, desc_id);
        Ok(())
    })
}

pub(crate) fn os_dup(fd: i32) -> Result<i32, i32> {
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let desc_id = os_desc_id(&vfs, fd)?;
        let mut new_fd = 3;
        while vfs.fds.contains_key(&new_fd) {
            new_fd += 1;
        }
        vfs.fds.insert(new_fd, desc_id);
        Ok(new_fd)
    })
}

pub(crate) fn os_dup2(fd: i32, new_fd: i32) -> Result<i32, i32> {
    if new_fd < 0 {
        return Err(OS_EBADF);
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let desc_id = os_desc_id(&vfs, fd)?;
        if fd == new_fd {
            return Ok(new_fd);
        }
        if let Some(old_desc) = vfs.fds.remove(&new_fd) {
            os_cleanup_desc(&mut vfs, old_desc);
        }
        vfs.fds.insert(new_fd, desc_id);
        Ok(new_fd)
    })
}

pub(crate) fn os_fd_path(fd: i32) -> Result<String, i32> {
    VFS.with(|v| {
        let vfs = v.borrow();
        Ok(os_desc_snapshot(&vfs, fd)?.1)
    })
}

pub(crate) fn os_fd_valid(fd: i32) -> bool {
    VFS.with(|v| v.borrow().fds.contains_key(&fd))
}

pub(crate) fn os_fstat(fd: i32) -> Result<OsStatData, i32> {
    let path = os_fd_path(fd)?;
    os_stat(&path)
}

fn os_read_at(fd: i32, count: usize, offset: Option<i64>) -> Result<Vec<u8>, i32> {
    if offset.is_some_and(|value| value < 0) {
        return Err(OS_EINVAL);
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let (desc_id, path, flags, pos) = os_desc_snapshot(&vfs, fd)?;
        let access = flags & 0o3;
        if flags & OS_O_PATH != 0 || access == OS_O_WRONLY {
            return Err(OS_EBADF);
        }
        if os_classify(&vfs, &path) == OsPathKind::Dir {
            return Err(OS_EISDIR);
        }
        let bytes = vfs.files.get(&path).ok_or(OS_ENOENT)?;
        let raw_start = offset.map_or(pos, |value| value as u64);
        let Ok(start) = usize::try_from(raw_start) else {
            return Ok(Vec::new());
        };
        if start >= bytes.len() || count == 0 {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(count).min(bytes.len());
        let out = bytes[start..end].to_vec();
        if offset.is_none() {
            if let Some(desc) = vfs.open_descs.get_mut(&desc_id) {
                desc.pos = end as u64;
            }
        }
        Ok(out)
    })
}

pub(crate) fn os_read(fd: i32, count: usize) -> Result<Vec<u8>, i32> {
    os_read_at(fd, count, None)
}

pub(crate) fn os_pread(fd: i32, count: usize, offset: i64) -> Result<Vec<u8>, i32> {
    os_read_at(fd, count, Some(offset))
}

fn os_write_at(fd: i32, data: &[u8], offset: Option<i64>) -> Result<usize, i32> {
    if offset.is_some_and(|value| value < 0) {
        return Err(OS_EINVAL);
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let (desc_id, path, flags, pos) = os_desc_snapshot(&vfs, fd)?;
        let access = flags & 0o3;
        if flags & OS_O_PATH != 0 || access == OS_O_RDONLY {
            return Err(OS_EBADF);
        }
        if os_classify(&vfs, &path) != OsPathKind::File {
            return Err(OS_EISDIR);
        }
        let append = flags & OS_O_APPEND != 0;
        let cur_len = vfs.files.get(&path).map_or(0, Vec::len);
        let start_u64 = if append {
            cur_len as u64
        } else {
            offset.map_or(pos, |value| value as u64)
        };
        if start_u64 > MAX_VFS_FILE_BYTES as u64 {
            return Err(OS_EFBIG);
        }
        let start = start_u64 as usize;
        let end = start.checked_add(data.len()).ok_or(OS_EFBIG)?;
        if end > MAX_VFS_FILE_BYTES {
            return Err(OS_EFBIG);
        }
        let target_len = cur_len.max(end);
        let other = vfs_used_excluding(&vfs, &path);
        if other.saturating_add(target_len) > MAX_VFS_TOTAL_BYTES {
            return Err(OS_ENOSPC);
        }
        let bytes = vfs.files.entry(path).or_default();
        if start > bytes.len() {
            bytes.resize(start, 0);
        }
        if end > bytes.len() {
            bytes.resize(end, 0);
        }
        bytes[start..end].copy_from_slice(data);
        if let Some(desc) = vfs.open_descs.get_mut(&desc_id) {
            if offset.is_none() || append {
                desc.pos = end as u64;
            }
        }
        Ok(data.len())
    })
}

pub(crate) fn os_write(fd: i32, data: &[u8]) -> Result<usize, i32> {
    os_write_at(fd, data, None)
}

pub(crate) fn os_pwrite(fd: i32, data: &[u8], offset: i64) -> Result<usize, i32> {
    os_write_at(fd, data, Some(offset))
}

pub(crate) fn os_lseek(fd: i32, offset: i64, whence: i32) -> Result<i64, i32> {
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let (desc_id, path, flags, pos) = os_desc_snapshot(&vfs, fd)?;
        if flags & OS_O_PATH != 0 {
            return Err(OS_EBADF);
        }
        let len = match os_classify(&vfs, &path) {
            OsPathKind::File => vfs.files.get(&path).map_or(0, |bytes| bytes.len() as i64),
            OsPathKind::Dir => 0,
            _ => return Err(OS_ENOENT),
        };
        let base = match whence {
            0 => 0,
            1 => pos as i64,
            2 => len,
            _ => return Err(OS_EINVAL),
        };
        let next = base.checked_add(offset).ok_or(OS_EINVAL)?;
        if next < 0 {
            return Err(OS_EINVAL);
        }
        if let Some(desc) = vfs.open_descs.get_mut(&desc_id) {
            desc.pos = next as u64;
        }
        Ok(next)
    })
}

pub(crate) fn os_ftruncate(fd: i32, length: i64) -> Result<(), i32> {
    if length < 0 {
        return Err(OS_EINVAL);
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let (_id, path, flags, _pos) = os_desc_snapshot(&vfs, fd)?;
        if flags & OS_O_PATH != 0 || flags & 0o3 == OS_O_RDONLY {
            return Err(OS_EINVAL);
        }
        if os_classify(&vfs, &path) != OsPathKind::File {
            return Err(OS_EINVAL);
        }
        if length > MAX_VFS_FILE_BYTES as i64 {
            return Err(OS_EFBIG);
        }
        let new_len = length as usize;
        let old_len = vfs.files.get(&path).map_or(0, Vec::len);
        if new_len > old_len {
            let other = vfs_used_excluding(&vfs, &path);
            if other.saturating_add(new_len) > MAX_VFS_TOTAL_BYTES {
                return Err(OS_ENOSPC);
            }
        }
        let bytes = vfs.files.entry(path).or_default();
        bytes.resize(new_len, 0);
        Ok(())
    })
}

pub(crate) fn os_fsync(fd: i32) -> Result<(), i32> {
    VFS.with(|v| {
        let vfs = v.borrow();
        os_desc_id(&vfs, fd).map(|_| ())
    })
}

pub(crate) fn os_chmod(path: &str, mode: i32) -> Result<(), i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let kind = os_classify(&vfs, &path);
        if !matches!(kind, OsPathKind::File | OsPathKind::Dir) {
            return Err(match kind {
                OsPathKind::NotDir => OS_ENOTDIR,
                _ => OS_ENOENT,
            });
        }
        let mut meta = os_meta_mut(&mut vfs, &path, kind);
        meta.mode = (meta.mode & 0o170000) | (mode & 0o7777);
        meta.ctime_ms = crate::jlsystem::epoch_ms();
        vfs.meta.insert(path, meta);
        Ok(())
    })
}

pub(crate) fn os_fchmod(fd: i32, mode: i32) -> Result<(), i32> {
    let path = os_fd_path(fd)?;
    os_chmod(&path, mode)
}

fn os_chown_path(path: &str, uid: i32, gid: i32) -> Result<(), i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let kind = os_classify(&vfs, &path);
        if !matches!(kind, OsPathKind::File | OsPathKind::Dir) {
            return Err(match kind {
                OsPathKind::NotDir => OS_ENOTDIR,
                _ => OS_ENOENT,
            });
        }
        let meta = os_meta_mut(&mut vfs, &path, kind);
        // The virtual app process is uid/gid 10000 and is not privileged;
        // changing either owner away from the current one is EPERM, as for a
        // normal Android app process.
        if (uid != -1 && uid != meta.uid) || (gid != -1 && gid != meta.gid) {
            return Err(OS_EPERM);
        }
        Ok(())
    })
}

pub(crate) fn os_chown(path: &str, uid: i32, gid: i32) -> Result<(), i32> {
    os_chown_path(path, uid, gid)
}

pub(crate) fn os_fchown(fd: i32, uid: i32, gid: i32) -> Result<(), i32> {
    let path = os_fd_path(fd)?;
    os_chown_path(&path, uid, gid)
}

pub(crate) fn os_mkdir(path: &str, mode: i32) -> Result<(), i32> {
    if path.is_empty() {
        return Err(OS_ENOENT);
    }
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        match os_classify(&vfs, &path) {
            OsPathKind::File | OsPathKind::Dir => Err(OS_EEXIST),
            OsPathKind::NotDir => Err(OS_ENOTDIR),
            OsPathKind::Missing => {
                os_parent_status(&vfs, &path)?;
                vfs.dirs.insert(path.clone());
                let mut meta = os_meta_mut(&mut vfs, &path, OsPathKind::Dir);
                meta.mode = 0o040000 | (mode & 0o777 & !0o022);
                vfs.meta.insert(path, meta);
                Ok(())
            }
        }
    })
}

fn os_dir_empty(vfs: &Vfs, path: &str) -> bool {
    let prefix = if path == "/" {
        "/".to_string()
    } else {
        format!("{path}/")
    };
    !vfs.dirs.iter().any(|dir| dir.starts_with(&prefix))
        && !vfs.files.keys().any(|file| file.starts_with(&prefix))
}

pub(crate) fn os_remove(path: &str) -> Result<(), i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        match os_classify(&vfs, &path) {
            OsPathKind::File => {
                vfs.files.remove(&path);
                vfs.meta.remove(&path);
                Ok(())
            }
            OsPathKind::Dir => {
                if path == "/" {
                    Err(OS_EINVAL)
                } else if !os_dir_empty(&vfs, &path) {
                    Err(OS_ENOTEMPTY)
                } else {
                    vfs.dirs.remove(&path);
                    vfs.meta.remove(&path);
                    Ok(())
                }
            }
            OsPathKind::NotDir => Err(OS_ENOTDIR),
            OsPathKind::Missing => Err(OS_ENOENT),
        }
    })
}

pub(crate) fn os_unlink(path: &str) -> Result<(), i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        match os_classify(&vfs, &path) {
            OsPathKind::File => {
                vfs.files.remove(&path);
                vfs.meta.remove(&path);
                Ok(())
            }
            OsPathKind::Dir => Err(OS_EISDIR),
            OsPathKind::NotDir => Err(OS_ENOTDIR),
            OsPathKind::Missing => Err(OS_ENOENT),
        }
    })
}

fn os_move_prefix(value: &str, old: &str, new: &str) -> String {
    if value == old {
        new.to_string()
    } else if let Some(rest) = value.strip_prefix(&format!("{old}/")) {
        format!("{new}/{rest}")
    } else {
        value.to_string()
    }
}

pub(crate) fn os_rename(old_path: &str, new_path: &str) -> Result<(), i32> {
    if old_path.is_empty() || new_path.is_empty() {
        return Err(OS_ENOENT);
    }
    let old_path = os_abs(old_path);
    let new_path = os_abs(new_path);
    if old_path == new_path {
        return os_stat(&old_path).map(|_| ());
    }
    VFS.with(|v| {
        let mut vfs = v.borrow_mut();
        let old_kind = os_classify(&vfs, &old_path);
        let new_kind = os_classify(&vfs, &new_path);
        match (old_kind, new_kind) {
            (OsPathKind::Missing, _) => return Err(OS_ENOENT),
            (OsPathKind::NotDir, _) | (_, OsPathKind::NotDir) => return Err(OS_ENOTDIR),
            (OsPathKind::File, OsPathKind::Dir) => return Err(OS_EISDIR),
            (OsPathKind::Dir, OsPathKind::File) => return Err(OS_ENOTDIR),
            (OsPathKind::Dir, OsPathKind::Dir) if !os_dir_empty(&vfs, &new_path) => {
                return Err(OS_ENOTEMPTY)
            }
            _ => {}
        }
        os_parent_status(&vfs, &new_path)?;
        if old_kind == OsPathKind::Dir && new_path.starts_with(&format!("{old_path}/")) {
            return Err(OS_EINVAL);
        }
        if new_kind == OsPathKind::File {
            vfs.files.remove(&new_path);
            vfs.meta.remove(&new_path);
        } else if new_kind == OsPathKind::Dir {
            vfs.dirs.remove(&new_path);
            vfs.meta.remove(&new_path);
        }
        if old_kind == OsPathKind::File {
            if let Some(bytes) = vfs.files.remove(&old_path) {
                vfs.files.insert(new_path.clone(), bytes);
            }
            if let Some(meta) = vfs.meta.remove(&old_path) {
                vfs.meta.insert(new_path.clone(), meta);
            }
        } else {
            let dirs: Vec<String> = vfs
                .dirs
                .iter()
                .filter(|dir| *dir == &old_path || dir.starts_with(&format!("{old_path}/")))
                .cloned()
                .collect();
            for dir in &dirs {
                vfs.dirs.remove(dir);
            }
            for dir in dirs {
                vfs.dirs.insert(os_move_prefix(&dir, &old_path, &new_path));
            }
            let files: Vec<String> = vfs
                .files
                .keys()
                .filter(|file| file.starts_with(&format!("{old_path}/")))
                .cloned()
                .collect();
            for file in files {
                if let Some(bytes) = vfs.files.remove(&file) {
                    vfs.files
                        .insert(os_move_prefix(&file, &old_path, &new_path), bytes);
                }
            }
            let metas: Vec<String> = vfs
                .meta
                .keys()
                .filter(|path| *path == &old_path || path.starts_with(&format!("{old_path}/")))
                .cloned()
                .collect();
            for path in metas {
                if let Some(meta) = vfs.meta.remove(&path) {
                    vfs.meta
                        .insert(os_move_prefix(&path, &old_path, &new_path), meta);
                }
            }
        }
        for desc in vfs.open_descs.values_mut() {
            desc.path = os_move_prefix(&desc.path, &old_path, &new_path);
        }
        Ok(())
    })
}

pub(crate) fn os_statvfs(path: &str) -> Result<OsStatVfsData, i32> {
    let path = os_abs(path);
    VFS.with(|v| {
        let vfs = v.borrow();
        match os_classify(&vfs, &path) {
            OsPathKind::File | OsPathKind::Dir => {}
            OsPathKind::NotDir => return Err(OS_ENOTDIR),
            OsPathKind::Missing => return Err(OS_ENOENT),
        }
        let used_bytes: i64 = vfs.files.values().map(|bytes| bytes.len() as i64).sum();
        let blocks = (8_i64 << 30) / 4096;
        let used_blocks = ((used_bytes + 4095) / 4096).min(blocks);
        let file_count = (vfs.files.len() + vfs.dirs.len()) as i64;
        Ok(OsStatVfsData {
            bsize: 4096,
            frsize: 4096,
            blocks,
            bfree: blocks - used_blocks,
            bavail: blocks - used_blocks,
            files: 1_000_000,
            ffree: 1_000_000 - file_count,
            favail: 1_000_000 - file_count,
            fsid: 0x5255_5354,
            flag: 0,
            namemax: 255,
        })
    })
}

pub(crate) fn os_fstatvfs(fd: i32) -> Result<OsStatVfsData, i32> {
    let path = os_fd_path(fd)?;
    os_statvfs(&path)
}

pub(crate) fn os_realpath(path: &str) -> Result<String, i32> {
    // The VFS has no symbolic links, so canonicalization is normalization
    // after verifying that the path exists.
    os_stat(path)?;
    Ok(os_abs(path))
}
