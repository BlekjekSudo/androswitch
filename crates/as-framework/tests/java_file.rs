//! Shared upstream File values with the actual bound product storage provider.
#![cfg(feature = "host")]
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{ObjRef, Value};
use std::{path::PathBuf, rc::Rc};
struct Test {
    app: App,
    root: PathBuf,
}
impl Test {
    fn object(&mut self, class: &str, sig: &str, args: &[Value]) -> ObjRef {
        let r = self
            .app
            .vm
            .heap
            .alloc_instance(class.into(), vec![])
            .unwrap();
        self.app.vm.attach_host_state(class, r).unwrap();
        self.call(r, "<init>", sig, args);
        r
    }
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "as-java-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let apk = rd_apk::Apk::from_bytes(
            std::fs::read(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../local-artifacts/conformance/AndroSwitchConformance.apk"),
            )
            .unwrap(),
        )
        .unwrap();
        let app = App::new(apk, fs, Rc::new(HostPlatform::default()), 640, 360, None).unwrap();
        Self { app, root }
    }
    fn file(&mut self, path: &str) -> ObjRef {
        let r = self
            .app
            .vm
            .heap
            .alloc_instance("Ljava/io/File;".into(), vec![])
            .unwrap();
        self.app.vm.attach_host_state("Ljava/io/File;", r).unwrap();
        let path = Value::Obj(self.app.vm.heap.alloc_string(path.into()).unwrap());
        self.call(r, "<init>", "(Ljava/lang/String;)V", &[path]);
        r
    }
    fn call(&mut self, r: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
        self.app.vm.invoke_instance(r, name, sig, args).unwrap()
    }
    fn text(&self, v: Value) -> String {
        self.app
            .vm
            .heap
            .as_str(v.as_ref().unwrap().unwrap())
            .unwrap()
            .into()
    }
}
#[test]
fn inherited_character_streams_write_persistently_and_keep_their_sink_alive() {
    let mut t = Test::new();
    let dir = t.file("/sdcard/Download");
    t.call(dir, "mkdirs", "()Z", &[]);
    let file = t.file("/sdcard/Download/characters");
    let fw = t.object(
        "Ljava/io/FileWriter;",
        "(Ljava/io/File;)V",
        &[Value::Obj(file)],
    );
    let bw = t.object(
        "Ljava/io/BufferedWriter;",
        "(Ljava/io/Writer;)V",
        &[Value::Obj(fw)],
    );
    t.app
        .vm
        .statics
        .insert(("Ltests/Files;".into(), "writer".into()), Value::Obj(bw));
    t.app.vm.gc_minor();
    t.app.vm.gc_minor();
    let chars = Value::Obj(
        t.app
            .vm
            .heap
            .alloc_string("hello \u{1f642}\nsecond".into())
            .unwrap(),
    );
    t.call(bw, "write", "(Ljava/lang/String;)V", &[chars]);
    t.call(bw, "close", "()V", &[]);
    assert_eq!(
        std::fs::read(t.root.join("Download/characters")).unwrap(),
        "hello \u{1f642}\nsecond".as_bytes()
    );
    let file = t.file("/storage/emulated/0/Download/characters");
    let fr = t.object(
        "Ljava/io/FileReader;",
        "(Ljava/io/File;)V",
        &[Value::Obj(file)],
    );
    let br = t.object(
        "Ljava/io/BufferedReader;",
        "(Ljava/io/Reader;)V",
        &[Value::Obj(fr)],
    );
    t.app
        .vm
        .statics
        .insert(("Ltests/Files;".into(), "reader".into()), Value::Obj(br));
    t.app.vm.gc();
    let line = t.call(br, "readLine", "()Ljava/lang/String;", &[]);
    assert_eq!(t.text(line), "hello \u{1f642}");
    let line = t.call(br, "readLine", "()Ljava/lang/String;", &[]);
    assert_eq!(t.text(line), "second");
    assert_eq!(
        t.call(br, "readLine", "()Ljava/lang/String;", &[]),
        Value::Null
    );
    t.call(br, "close", "()V", &[]);
    assert!(t.app.vm.invoke_instance(br, "read", "()I", &[]).is_err());
}
#[test]
fn channel_position_and_close_share_the_stream_owner_and_file_renames_replace() {
    let mut t = Test::new();
    let dir = t.file("/sdcard/Download");
    t.call(dir, "mkdirs", "()Z", &[]);
    let f = t.file("/sdcard/Download/source");
    let out = t.object(
        "Ljava/io/FileOutputStream;",
        "(Ljava/io/File;)V",
        &[Value::Obj(f)],
    );
    let channel = t
        .call(out, "getChannel", "()Ljava/nio/channels/FileChannel;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.call(out, "write", "(I)V", &[Value::Int(42)]);
    assert_eq!(t.call(channel, "position", "()J", &[]), Value::Long(1));
    t.call(channel, "force", "(Z)V", &[Value::Int(1)]);
    t.call(channel, "close", "()V", &[]);
    assert_eq!(t.call(channel, "isOpen", "()Z", &[]), Value::Int(0));
    assert!(t
        .app
        .vm
        .invoke_instance(out, "write", "(I)V", &[Value::Int(20)])
        .is_err());
    let target = t.file("/sdcard/Download/target");
    t.call(target, "createNewFile", "()Z", &[]);
    assert_eq!(
        t.call(f, "renameTo", "(Ljava/io/File;)Z", &[Value::Obj(target)]),
        Value::Int(1)
    );
    assert_eq!(std::fs::read(t.root.join("Download/target")).unwrap(), [42]);
    assert!(!t.root.join("Download/source").exists());
}
impl Drop for Test {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct FailSync {
    fs: as_platform::vfs::Vfs<HostFileSystem>,
    fail: Rc<std::cell::Cell<bool>>,
}
impl as_platform::vfs::GuestFileSystem for FailSync {
    fn sync_file(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        if self.fail.get() {
            Err(as_platform::fs::error(
                "IO_ERROR",
                "sync_file",
                "injected durability failure",
            ))
        } else {
            self.fs.sync_file(p)
        }
    }
    fn resolve(&self, p: &str) -> Result<String, as_platform::PlatformError> {
        self.fs.resolve(p)
    }
    fn stat(
        &self,
        p: &str,
    ) -> Result<Option<as_platform::fs::Metadata>, as_platform::PlatformError> {
        self.fs.stat(p)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, as_platform::PlatformError> {
        self.fs.read_at(p, o, b)
    }
    fn write_at(
        &self,
        p: &str,
        o: u64,
        b: &[u8],
        t: bool,
    ) -> Result<usize, as_platform::PlatformError> {
        self.fs.write_at(p, o, b, t)
    }
    fn mkdirs(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        self.fs.mkdirs(p)
    }
    fn list(&self, p: &str) -> Result<Vec<as_platform::fs::Entry>, as_platform::PlatformError> {
        self.fs.list(p)
    }
    fn remove(&self, p: &str) -> Result<(), as_platform::PlatformError> {
        self.fs.remove(p)
    }
    fn rename_replace(&self, a: &str, b: &str) -> Result<(), as_platform::PlatformError> {
        self.fs.rename_replace(a, b)
    }
}
#[test]
fn atomic_file_keeps_committed_backup_when_durable_finish_fails() {
    let mut t = Test::new();
    let fail = Rc::new(std::cell::Cell::new(false));
    let package = t.app.android.apk.manifest.package.clone();
    t.app
        .vm
        .bind_device_storage(
            &package,
            Box::new(FailSync {
                fs: as_platform::vfs::Vfs::new(HostFileSystem::new(&t.root).unwrap(), &package)
                    .unwrap(),
                fail: fail.clone(),
            }),
        )
        .unwrap();
    let base = t.file("/sdcard/Download/atomic/base");
    let af = t.object(
        "Landroid/util/AtomicFile;",
        "(Ljava/io/File;)V",
        &[Value::Obj(base)],
    );
    let first = t
        .call(af, "startWrite", "()Ljava/io/FileOutputStream;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.call(first, "write", "(I)V", &[Value::Int(42)]);
    t.call(
        af,
        "finishWrite",
        "(Ljava/io/FileOutputStream;)V",
        &[Value::Obj(first)],
    );
    let next = t
        .call(af, "startWrite", "()Ljava/io/FileOutputStream;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.call(next, "write", "(I)V", &[Value::Int(43)]);
    fail.set(true);
    let error = t
        .app
        .vm
        .invoke_instance(
            af,
            "finishWrite",
            "(Ljava/io/FileOutputStream;)V",
            &[Value::Obj(next)],
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected durability failure"));
    assert_eq!(
        std::fs::read(t.root.join("Download/atomic/base.bak")).unwrap(),
        [42]
    );
    let recovered = t
        .call(af, "readFully", "()[B", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(
        t.app.vm.heap.as_array_elems(recovered).unwrap().0,
        &[Value::Int(42)]
    );
    assert_eq!(
        std::fs::read(t.root.join("Download/atomic/base")).unwrap(),
        [42]
    );
    assert!(!t.root.join("Download/atomic/base.bak").exists());
}
#[test]
fn temporary_files_use_exclusive_creation_and_reject_short_prefixes() {
    let mut t = Test::new();
    let prefix = Value::Obj(t.app.vm.heap.alloc_string("prefix".into()).unwrap());
    let sig = "(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;";
    let a = t
        .app
        .vm
        .invoke_static(
            "Ljava/io/File;",
            "createTempFile",
            sig,
            &[prefix.clone(), Value::Null],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let b = t
        .app
        .vm
        .invoke_static(
            "Ljava/io/File;",
            "createTempFile",
            sig,
            &[prefix, Value::Null],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(t.call(a, "exists", "()Z", &[]), Value::Int(1));
    assert_eq!(
        t.call(a, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(b)]),
        Value::Int(0)
    );
    let short = Value::Obj(t.app.vm.heap.alloc_string("ab".into()).unwrap());
    let e = t
        .app
        .vm
        .invoke_static(
            "Ljava/io/File;",
            "createTempFile",
            sig,
            &[short, Value::Null],
        )
        .unwrap_err();
    assert!(e.to_string().contains("IllegalArgumentException"));
}
#[test]
fn upstream_file_paths_share_the_java_heap_and_persistent_stream_mount() {
    let mut t = Test::new();
    let dir = t.file("/sdcard/Download/family");
    assert_eq!(t.call(dir, "mkdirs", "()Z", &[]), Value::Int(1));
    let f = t.file("/sdcard/Download/family/save.bin");
    assert_eq!(t.call(f, "createNewFile", "()Z", &[]), Value::Int(1));
    assert_eq!(t.call(f, "createNewFile", "()Z", &[]), Value::Int(0));
    let out = t
        .app
        .vm
        .heap
        .alloc_instance("Ljava/io/FileOutputStream;".into(), vec![])
        .unwrap();
    t.app
        .vm
        .attach_host_state("Ljava/io/FileOutputStream;", out)
        .unwrap();
    t.call(out, "<init>", "(Ljava/io/File;)V", &[Value::Obj(f)]);
    t.call(out, "write", "(I)V", &[Value::Int(123)]);
    t.call(out, "close", "()V", &[]);
    assert_eq!(
        std::fs::read(t.root.join("Download/family/save.bin")).unwrap(),
        [123]
    );
    assert_eq!(t.call(f, "length", "()J", &[]), Value::Long(1));
    // A native/platform writer and the /storage alias observe the same file.
    std::fs::write(t.root.join("Download/family/save.bin"), [1, 2, 3]).unwrap();
    let alias = t.file("/storage/emulated/0/Download/family/save.bin");
    assert_eq!(t.call(alias, "length", "()J", &[]), Value::Long(3));
    assert!(t.call(alias, "lastModified", "()J", &[]).as_long().unwrap() > 0);
    let list = t
        .call(dir, "listFiles", "()[Ljava/io/File;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    let values = t.app.vm.heap.as_array_elems(list).unwrap().0.to_vec();
    assert_eq!(values.len(), 1);
    let listed = values[0].as_ref().unwrap().unwrap();
    assert_eq!(t.call(listed, "length", "()J", &[]), Value::Long(3));
    let name = t.call(listed, "getName", "()Ljava/lang/String;", &[]);
    assert_eq!(t.text(name), "save.bin");
    // No writable File side table owns this value; the heap field survives GC.
    t.app
        .vm
        .statics
        .insert(("Ltests/Files;".into(), "file".into()), Value::Obj(listed));
    t.app.vm.gc();
    assert_eq!(t.call(listed, "length", "()J", &[]), Value::Long(3));
}
#[test]
fn relative_file_values_retain_upstream_lexical_paths_and_device_user_directory() {
    let mut t = Test::new();
    let f = t.file("a//b/../save");
    let value = t.call(f, "getPath", "()Ljava/lang/String;", &[]);
    assert_eq!(t.text(value), "a/save");
    let value = t.call(f, "getAbsolutePath", "()Ljava/lang/String;", &[]);
    let absolute = t.text(value);
    assert!(absolute.starts_with("/data/user/0/"));
    assert!(absolute.ends_with("/files/a/save"));
    let parent = t
        .call(f, "getParentFile", "()Ljava/io/File;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    let value = t.call(parent, "getPath", "()Ljava/lang/String;", &[]);
    assert_eq!(t.text(value), "a");
    assert_eq!(t.call(f, "isAbsolute", "()Z", &[]), Value::Int(0));
}
#[test]
fn file_creation_is_exclusive_and_does_not_truncate_existing_native_data() {
    let mut t = Test::new();
    let dir = t.file("/sdcard/Download");
    t.call(dir, "mkdirs", "()Z", &[]);
    std::fs::write(t.root.join("Download/existing"), b"keep").unwrap();
    let file = t.file("/sdcard/Download/existing");
    assert_eq!(t.call(file, "createNewFile", "()Z", &[]), Value::Int(0));
    assert_eq!(
        std::fs::read(t.root.join("Download/existing")).unwrap(),
        b"keep"
    );
    let denied = t.file("/data/user/0/org.other.package/private");
    assert_eq!(t.call(denied, "exists", "()Z", &[]), Value::Int(0));
    assert!(t
        .app
        .vm
        .invoke_instance(denied, "createNewFile", "()Z", &[])
        .is_err());
    assert!(!t.root.join("userdata/org.other.package").exists());
    assert!(t
        .app
        .vm
        .invoke_instance(file, "createNewFile", "()V", &[])
        .is_err());
}
