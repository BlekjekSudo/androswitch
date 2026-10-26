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

#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
#[cfg(feature = "desktop-runtime")]
use core::cell::RefCell;

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

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
pub(crate) struct Vfs {
    dirs: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
    meta: BTreeMap<String, VfsMeta>,
    next_ino: i64,
    open_descs: BTreeMap<i64, OpenDesc>,
    fds: BTreeMap<i32, i64>,
    next_desc: i64,
}

#[cfg(feature = "desktop-runtime")]
thread_local! {
    static VFS: RefCell<Vfs> = RefCell::new(Vfs::default());
}

// The desktop oracle retains session-scoped VFS persistence. The isolated
// no_std parity harness owns its memory filesystem explicitly through Engine.
// This backend is not the Horizon persistent VFS and must not be selected by
// the product runner until the filesystem adapter migration is complete.
macro_rules! vfs {
    ($vm:expr) => {{
        #[cfg(feature = "desktop-runtime")]
        {
            &VFS
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            $vm.parity_vfs.clone()
        }
    }};
}

pub(crate) use crate::java_file::new_file;
use crate::java_file::{normalize, parent_of, path_of};

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

fn mkdirs(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) {
    let p = absolute(p);
    vfs!(vm).with(|v| {
        let mut v = v.borrow_mut();
        let mut cur = String::new();
        for seg in p.split('/').filter(|s| !s.is_empty()) {
            cur.push('/');
            cur.push_str(seg);
            v.dirs.insert(cur.clone());
        }
    });
}

#[cfg(test)]
fn is_dir(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) -> bool {
    if p.is_empty() {
        return false;
    }
    let p = absolute(p);
    p == "/" || vfs!(vm).with(|v| v.borrow().dirs.contains(&p))
}
#[cfg(test)]
fn is_file(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) -> bool {
    if p.is_empty() {
        return false;
    }
    let p = absolute(p);
    vfs!(vm).with(|v| v.borrow().files.contains_key(&p))
}

/// Android's deterministic emulated shared-storage root, held only in the
/// VM's virtual filesystem (never mapped to host disk).
pub(crate) fn external_storage_path(vm: &mut Engine, path: &str) -> Result<Value, VmExit> {
    mkdirs(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        "/storage/emulated/0",
    );
    new_file(vm, path)
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
    mkdirs(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        &p,
    );
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
            mkdirs(
                #[cfg(not(feature = "desktop-runtime"))]
                vm,
                &p,
            );
            new_file(vm, &p)?
        }
        ("databaseList", _) | ("fileList", _) => {
            let pkg = vm.fw.package_name.clone();
            let dir = if name == "databaseList" {
                format!("/data/user/0/{pkg}/databases/")
            } else {
                format!("/data/user/0/{pkg}/files/")
            };
            let names: Vec<String> = vfs!(vm).with(|v| {
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
            let deleted = vfs!(vm).with(|v| v.borrow_mut().files.remove(&p).is_some());
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
    let sig = match args.len() {
        2 => "(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;",
        3 => "(Ljava/lang/String;Ljava/lang/String;Ljava/io/File;)Ljava/io/File;",
        _ => return Ok(None),
    };
    crate::java_file::call_static(vm, class, name, sig, args)
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if crate::java_stream::supports(recv_class) {
        return stream_intrinsic(vm, recv_class, name, sig, recv, args);
    }
    if let Some(value) = crate::atomic_file::call(vm, recv_class, name, sig, recv, args)? {
        return Ok(Some(value));
    }
    crate::java_file::call(vm, recv_class, name, sig, recv, args)
}

const FOS: &str = "Ljava/io/FileOutputStream;";
const FIS: &str = "Ljava/io/FileInputStream;";
fn stream_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    crate::java_stream::call(vm, class, name, sig, recv, args)
}

/// Creates `p`'s parent directories (public entry for other host modules).
pub(crate) fn mkdirs_pub(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) {
    mkdirs(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        p,
    );
}

/// Ensures a file exists at `p` (parents included), keeping its content.
pub(crate) fn touch(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) {
    let p = absolute(p);
    if let Some(parent) = parent_of(&p) {
        mkdirs(
            #[cfg(not(feature = "desktop-runtime"))]
            vm,
            &parent,
        );
    }
    vfs!(vm).with(|v| {
        v.borrow_mut().files.entry(p).or_default();
    });
}

/// Reads a virtual file's bytes for another host module (e.g. SQLite loads
/// its serialized database image from the app-private VFS path).
pub(crate) fn read_bytes(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    p: &str,
) -> Option<Vec<u8>> {
    let p = absolute(p);
    vfs!(vm).with(|v| v.borrow().files.get(&p).cloned())
}

/// Replaces a virtual file's bytes, creating parent directories as Android
/// does for app-private storage paths.
pub(crate) fn write_bytes(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    p: &str,
    bytes: Vec<u8>,
) {
    let p = absolute(p);
    if let Some(parent) = parent_of(&p) {
        mkdirs(
            #[cfg(not(feature = "desktop-runtime"))]
            vm,
            &parent,
        );
    }
    vfs!(vm).with(|v| {
        v.borrow_mut().files.insert(p, bytes);
    });
}

/// Deletes a virtual file. Returns whether a file existed.
pub(crate) fn delete_file(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, p: &str) -> bool {
    let p = absolute(p);
    vfs!(vm).with(|v| v.borrow_mut().files.remove(&p).is_some())
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
pub fn clear_package_data(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, package: &str) {
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
    vfs!(vm).with(|cell| {
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
pub fn reset_all(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine) {
    vfs!(vm).with(|cell| *cell.borrow_mut() = Vfs::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_stream_constructor_keeps_existing_reference_bytes() {
        let mut vm = crate::Engine::new(vec![], Default::default());
        let path = Value::Obj(
            vm.heap
                .alloc_string("/fd-constructor-preserve".into())
                .unwrap(),
        );
        let out = vm.heap.alloc_instance(FOS.into(), vec![]).unwrap();
        vm.invoke_instance(out, "<init>", "(Ljava/lang/String;)V", &[path])
            .unwrap();
        vm.invoke_instance(out, "write", "(I)V", &[Value::Int(65)])
            .unwrap();
        let fd = vm
            .invoke_instance(out, "getFD", "()Ljava/io/FileDescriptor;", &[])
            .unwrap();
        let next = vm.heap.alloc_instance(FOS.into(), vec![]).unwrap();
        vm.invoke_instance(next, "<init>", "(Ljava/io/FileDescriptor;)V", &[fd])
            .unwrap();
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/fd-constructor-preserve"
            ),
            Some(vec![65])
        );
    }
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    #[test]
    fn paths_and_vfs() {
        #[cfg(not(feature = "desktop-runtime"))]
        let vm = crate::Engine::new(alloc::vec![], Default::default());

        assert_eq!(normalize("/a/./b/../c/"), "/a/c");
        assert_eq!(parent_of("/a/c").as_deref(), Some("/a"));
        assert_eq!(parent_of("/a").as_deref(), Some("/"));
        mkdirs(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "/x/y/z",
        );
        assert!(
            is_dir(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/x/y"
            ) && is_dir(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/x/y/z"
            ) && !is_file(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/x/y"
            )
        );
    }

    #[test]
    fn clear_package_data_isolates_packages_and_reset_all_clears() {
        #[cfg(not(feature = "desktop-runtime"))]
        let vm = crate::Engine::new(alloc::vec![], Default::default());

        reset_all(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
        );
        write_bytes(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "/data/user/0/com.example.a/files/x.txt",
            b"a".to_vec(),
        );
        write_bytes(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "/storage/emulated/0/Android/data/com.example.a/files/ext.txt",
            b"e".to_vec(),
        );
        write_bytes(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "/data/user/0/com.example.b/files/y.txt",
            b"b".to_vec(),
        );
        write_bytes(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "/data/user/0/com.example.ab/files/z.txt",
            b"z".to_vec(),
        );
        clear_package_data(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
            "com.example.a",
        );
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/data/user/0/com.example.a/files/x.txt"
            ),
            None,
            "#230: clear data apaga os arquivos do pacote"
        );
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/storage/emulated/0/Android/data/com.example.a/files/ext.txt"
            ),
            None,
            "#230: clear data apaga também os externos específicos do app"
        );
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/data/user/0/com.example.ab/files/z.txt"
            ),
            Some(b"z".to_vec()),
            "pacote com prefixo textual comum (com.example.ab) não é tocado"
        );
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/data/user/0/com.example.b/files/y.txt"
            ),
            Some(b"b".to_vec()),
            "pacote vizinho fica intacto"
        );
        reset_all(
            #[cfg(not(feature = "desktop-runtime"))]
            &vm,
        );
        assert_eq!(
            read_bytes(
                #[cfg(not(feature = "desktop-runtime"))]
                &vm,
                "/data/user/0/com.example.b/files/y.txt"
            ),
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

pub(crate) fn os_stat(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
) -> Result<OsStatData, i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| os_stat_in(&mut v.borrow_mut(), &path))
}

pub(crate) fn os_access(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    mode: i32,
) -> Result<bool, i32> {
    if mode & !0o7 != 0 {
        return Err(OS_EINVAL);
    }
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_open(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    flags: i32,
    mode: i32,
) -> Result<i32, i32> {
    if path.is_empty() {
        return Err(OS_ENOENT);
    }
    let path = os_abs(path);
    let access = flags & 0o3;
    if access == 3 {
        return Err(OS_EINVAL);
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_register_path(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    flags: i32,
    pos: u64,
) -> Option<i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_close(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<(), i32> {
    vfs!(vm).with(|v| {
        let mut vfs = v.borrow_mut();
        let desc_id = vfs.fds.remove(&fd).ok_or(OS_EBADF)?;
        os_cleanup_desc(&mut vfs, desc_id);
        Ok(())
    })
}

pub(crate) fn os_dup(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<i32, i32> {
    vfs!(vm).with(|v| {
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

pub(crate) fn os_dup2(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    new_fd: i32,
) -> Result<i32, i32> {
    if new_fd < 0 {
        return Err(OS_EBADF);
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_fd_path(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<String, i32> {
    vfs!(vm).with(|v| {
        let vfs = v.borrow();
        Ok(os_desc_snapshot(&vfs, fd)?.1)
    })
}

pub(crate) fn os_fd_valid(#[cfg(not(feature = "desktop-runtime"))] vm: &Engine, fd: i32) -> bool {
    vfs!(vm).with(|v| v.borrow().fds.contains_key(&fd))
}

pub(crate) fn os_fstat(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<OsStatData, i32> {
    let path = os_fd_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
    )?;
    os_stat(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        &path,
    )
}

fn os_read_at(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    count: usize,
    offset: Option<i64>,
) -> Result<Vec<u8>, i32> {
    if offset.is_some_and(|value| value < 0) {
        return Err(OS_EINVAL);
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_read(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    count: usize,
) -> Result<Vec<u8>, i32> {
    os_read_at(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
        count,
        None,
    )
}

pub(crate) fn os_pread(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    count: usize,
    offset: i64,
) -> Result<Vec<u8>, i32> {
    os_read_at(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
        count,
        Some(offset),
    )
}

fn os_write_at(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    data: &[u8],
    offset: Option<i64>,
) -> Result<usize, i32> {
    if offset.is_some_and(|value| value < 0) {
        return Err(OS_EINVAL);
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_write(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    data: &[u8],
) -> Result<usize, i32> {
    os_write_at(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
        data,
        None,
    )
}

pub(crate) fn os_pwrite(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    data: &[u8],
    offset: i64,
) -> Result<usize, i32> {
    os_write_at(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
        data,
        Some(offset),
    )
}

pub(crate) fn os_lseek(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    offset: i64,
    whence: i32,
) -> Result<i64, i32> {
    vfs!(vm).with(|v| {
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

pub(crate) fn os_ftruncate(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    length: i64,
) -> Result<(), i32> {
    if length < 0 {
        return Err(OS_EINVAL);
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_fsync(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<(), i32> {
    vfs!(vm).with(|v| {
        let vfs = v.borrow();
        os_desc_id(&vfs, fd).map(|_| ())
    })
}

pub(crate) fn os_chmod(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    mode: i32,
) -> Result<(), i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_fchmod(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    mode: i32,
) -> Result<(), i32> {
    let path = os_fd_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
    )?;
    os_chmod(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        &path,
        mode,
    )
}

fn os_chown_path(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    uid: i32,
    gid: i32,
) -> Result<(), i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_chown(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    uid: i32,
    gid: i32,
) -> Result<(), i32> {
    os_chown_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        path,
        uid,
        gid,
    )
}

pub(crate) fn os_fchown(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
    uid: i32,
    gid: i32,
) -> Result<(), i32> {
    let path = os_fd_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
    )?;
    os_chown_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        &path,
        uid,
        gid,
    )
}

pub(crate) fn os_mkdir(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
    mode: i32,
) -> Result<(), i32> {
    if path.is_empty() {
        return Err(OS_ENOENT);
    }
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_remove(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
) -> Result<(), i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_unlink(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
) -> Result<(), i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_rename(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    old_path: &str,
    new_path: &str,
) -> Result<(), i32> {
    if old_path.is_empty() || new_path.is_empty() {
        return Err(OS_ENOENT);
    }
    let old_path = os_abs(old_path);
    let new_path = os_abs(new_path);
    if old_path == new_path {
        return os_stat(
            #[cfg(not(feature = "desktop-runtime"))]
            vm,
            &old_path,
        )
        .map(|_| ());
    }
    vfs!(vm).with(|v| {
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

pub(crate) fn os_statvfs(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
) -> Result<OsStatVfsData, i32> {
    let path = os_abs(path);
    vfs!(vm).with(|v| {
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

pub(crate) fn os_fstatvfs(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    fd: i32,
) -> Result<OsStatVfsData, i32> {
    let path = os_fd_path(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        fd,
    )?;
    os_statvfs(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        &path,
    )
}

pub(crate) fn os_realpath(
    #[cfg(not(feature = "desktop-runtime"))] vm: &Engine,
    path: &str,
) -> Result<String, i32> {
    // The VFS has no symbolic links, so canonicalization is normalization
    // after verifying that the path exists.
    os_stat(
        #[cfg(not(feature = "desktop-runtime"))]
        vm,
        path,
    )?;
    Ok(os_abs(path))
}

/// Explicit memory transport for the original semantic oracle. The shared
/// File algorithms select it only when no product framework/storage is bound.
pub(crate) struct ReferenceIo<'a> {
    #[allow(dead_code)] // Desktop VFS is session thread-local; embedded is Engine-owned.
    vm: &'a Engine,
}
impl<'a> ReferenceIo<'a> {
    pub(crate) fn new(vm: &'a Engine) -> Self {
        Self { vm }
    }
}
fn reference_error(op: &'static str, errno: i32) -> as_platform::PlatformError {
    as_platform::fs::error(
        match errno {
            OS_ENOENT => "NOT_FOUND",
            OS_EEXIST => "ALREADY_EXISTS",
            OS_EACCES | OS_EPERM => "ACCESS_DENIED",
            _ => "IO_ERROR",
        },
        op,
        format!("reference VFS errno {errno}"),
    )
}
impl as_platform::vfs::GuestFileSystem for ReferenceIo<'_> {
    fn resolve(&self, p: &str) -> Result<String, as_platform::PlatformError> {
        Ok(absolute(p))
    }
    fn stat(
        &self,
        p: &str,
    ) -> Result<Option<as_platform::fs::Metadata>, as_platform::PlatformError> {
        let p = absolute(p);
        Ok(vfs!(self.vm).with(|v| {
            let v = v.borrow();
            if p == "/" || v.dirs.contains(&p) {
                Some(as_platform::fs::Metadata {
                    directory: true,
                    length: 0,
                })
            } else {
                v.files.get(&p).map(|b| as_platform::fs::Metadata {
                    directory: false,
                    length: b.len() as u64,
                })
            }
        }))
    }
    fn modified_millis(&self, _p: &str) -> Result<u64, as_platform::PlatformError> {
        Ok(crate::jlsystem::epoch_ms().max(0) as u64)
    }
    fn create_file_new(&self, p: &str) -> Result<bool, as_platform::PlatformError> {
        let p = absolute(p);
        vfs!(self.vm).with(|v| {
            let mut v = v.borrow_mut();
            if v.files.contains_key(&p) || v.dirs.contains(&p) || p == "/" {
                return Ok(false);
            }
            os_parent_status(&v, &p).map_err(|e| reference_error("create_file_new", e))?;
            v.files.insert(p, Vec::new());
            Ok(true)
        })
    }
    fn create_dir(&self, p: &str) -> Result<bool, as_platform::PlatformError> {
        match os_mkdir(
            #[cfg(not(feature = "desktop-runtime"))]
            self.vm,
            p,
            0o777,
        ) {
            Ok(()) => Ok(true),
            Err(OS_EEXIST) => Ok(false),
            Err(e) => Err(reference_error("mkdir", e)),
        }
    }
    fn mkdirs(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        mkdirs(
            #[cfg(not(feature = "desktop-runtime"))]
            self.vm,
            p,
        );
        Ok(())
    }
    fn list(&self, p: &str) -> Result<Vec<as_platform::fs::Entry>, as_platform::PlatformError> {
        let p = absolute(p);
        let prefix = if p == "/" { p } else { format!("{p}/") };
        Ok(vfs!(self.vm).with(|v| {
            let v = v.borrow();
            v.dirs
                .iter()
                .map(|k| (k, true))
                .chain(v.files.keys().map(|k| (k, false)))
                .filter_map(|(k, directory)| {
                    k.strip_prefix(&prefix)
                        .filter(|k| !k.is_empty() && !k.contains('/'))
                        .map(|name| as_platform::fs::Entry {
                            name: name.to_string(),
                            directory,
                        })
                })
                .collect()
        }))
    }
    fn remove(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        os_remove(
            #[cfg(not(feature = "desktop-runtime"))]
            self.vm,
            p,
        )
        .map_err(|e| reference_error("remove", e))
    }
    fn rename_new(
        &self,
        source: &str,
        destination: &str,
    ) -> Result<(), as_platform::PlatformError> {
        if self.stat(destination)?.is_some() {
            return Err(reference_error("rename", OS_EEXIST));
        }
        self.rename_replace(source, destination)
    }
    fn rename_replace(
        &self,
        source: &str,
        destination: &str,
    ) -> Result<(), as_platform::PlatformError> {
        os_rename(
            #[cfg(not(feature = "desktop-runtime"))]
            self.vm,
            source,
            destination,
        )
        .map_err(|e| reference_error("rename", e))
    }
    fn sync_file(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        if self.stat(p)?.is_some_and(|m| !m.directory) {
            Ok(())
        } else {
            Err(reference_error("sync_file", OS_ENOENT))
        }
    }
    fn read_at(
        &self,
        p: &str,
        o: u64,
        out: &mut [u8],
    ) -> Result<usize, as_platform::PlatformError> {
        vfs!(self.vm).with(|v| {
            let v = v.borrow();
            let bytes = v
                .files
                .get(p)
                .ok_or_else(|| reference_error("read", OS_ENOENT))?;
            let o = usize::try_from(o).unwrap_or(usize::MAX).min(bytes.len());
            let n = out.len().min(bytes.len() - o);
            out[..n].copy_from_slice(&bytes[o..o + n]);
            Ok(n)
        })
    }
    fn write_at(
        &self,
        p: &str,
        o: u64,
        input: &[u8],
        truncate: bool,
    ) -> Result<usize, as_platform::PlatformError> {
        vfs!(self.vm).with(|v| {
            let mut v = v.borrow_mut();
            let o = usize::try_from(o).map_err(|_| reference_error("write", OS_EFBIG))?;
            let end = o
                .checked_add(input.len())
                .filter(|n| *n <= MAX_VFS_FILE_BYTES)
                .ok_or_else(|| reference_error("write", OS_EFBIG))?;
            let old = if truncate {
                0
            } else {
                v.files.get(p).map_or(0, Vec::len)
            };
            let size = if input.is_empty() { old } else { old.max(end) };
            if vfs_used_excluding(&v, p).saturating_add(size) > MAX_VFS_TOTAL_BYTES {
                return Err(reference_error("write", OS_ENOSPC));
            }
            os_parent_status(&v, p).map_err(|e| reference_error("write", e))?;
            if v.dirs.contains(p) {
                return Err(reference_error("write", OS_EISDIR));
            }
            let bytes = v.files.entry(p.to_string()).or_default();
            if truncate {
                bytes.clear();
            }
            bytes.resize(size, 0);
            if !input.is_empty() {
                bytes[o..end].copy_from_slice(input);
            }
            Ok(input.len())
        })
    }
}
