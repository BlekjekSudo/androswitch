use as_package::PackageManager;
use as_platform::{
    fs::{self, Entry, FileSystem, Metadata},
    host_fs::HostFileSystem,
    vfs::{GuestFileSystem, Vfs},
    PlatformError,
};
use as_testsupport::apkfix::{a, build_axml, AxNode, AxVal, ZipFix};
use std::{cell::Cell, path::PathBuf, rc::Rc};
fn root(label: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/package-tests")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn apk(package: &str, version: i32) -> Vec<u8> {
    let root = AxNode {
        name: "manifest".into(),
        attrs: vec![
            a(false, "package", AxVal::Str(package.into())),
            a(true, "versionCode", AxVal::Int(version)),
        ],
        children: vec![
            AxNode {
                name: "uses-sdk".into(),
                attrs: vec![
                    a(true, "minSdkVersion", AxVal::Int(24)),
                    a(true, "targetSdkVersion", AxVal::Int(35)),
                ],
                children: vec![],
            },
            AxNode {
                name: "application".into(),
                attrs: vec![a(true, "label", AxVal::Str("Package fixture".into()))],
                children: vec![],
            },
        ],
    };
    let mut zip = ZipFix::new();
    zip.add("AndroidManifest.xml", build_axml(&root));
    zip.build()
}
#[derive(Clone)]
struct FaultFs {
    fs: HostFileSystem,
    fail: Rc<Cell<Option<usize>>>,
    count: Rc<Cell<usize>>,
}
impl FaultFs {
    fn tick(&self) -> Result<(), PlatformError> {
        let n = self.count.get();
        self.count.set(n + 1);
        if self.fail.get() == Some(n) {
            Err(fs::error(
                "INJECTED_IO",
                "test",
                "power loss before mutation",
            ))
        } else {
            Ok(())
        }
    }
}
impl FileSystem for FaultFs {
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        self.fs.stat(p)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        self.fs.read_at(p, o, b)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        self.tick()?;
        self.fs.write_at(p, o, b, t)
    }
    fn create_dir(&self, p: &str, e: bool) -> Result<bool, PlatformError> {
        self.tick()?;
        self.fs.create_dir(p, e)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        self.fs.list(p)
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        self.tick()?;
        self.fs.rename_new(a, b)
    }
    fn remove(&self, p: &str, r: bool) -> Result<(), PlatformError> {
        self.tick()?;
        self.fs.remove(p, r)
    }
}
#[test]
fn failed_updates_preserve_previous_committed_apk_and_data() {
    let mut failures = 0;
    let mut successes = 0;
    for boundary in 0..40 {
        let fs = FaultFs {
            fs: HostFileSystem::new(root("fault")).unwrap(),
            fail: Rc::new(Cell::new(None)),
            count: Rc::new(Cell::new(0)),
        };
        let pm = PackageManager::new(fs.clone());
        let first = pm.install(apk("org.fixture.one", 1), false, None).unwrap();
        let vfs = pm.vfs("org.fixture.one").unwrap();
        vfs.write_at("/data/user/0/org.fixture.one/files/save", 0, b"keep", true)
            .unwrap();
        fs.count.set(0);
        fs.fail.set(Some(boundary));
        let result = pm.install(apk("org.fixture.one", 2), true, None);
        fs.fail.set(None);
        let reopened = PackageManager::new(fs.clone());
        let (active, _) = reopened.load("org.fixture.one", None).unwrap();
        match result {
            Ok(r) => {
                assert_eq!(active, r);
                successes += 1;
            }
            Err(_) => {
                assert_eq!(active, first);
                failures += 1;
            }
        }
        let mut save = [0; 4];
        vfs.read_at("/data/user/0/org.fixture.one/files/save", 0, &mut save)
            .unwrap();
        assert_eq!(&save, b"keep");
    }
    assert!(failures > 20);
    assert!(successes > 0);
}
#[test]
fn identical_updates_reuse_verified_installation_without_writes() {
    let fs = FaultFs {
        fs: HostFileSystem::new(root("identical")).unwrap(),
        fail: Rc::new(Cell::new(None)),
        count: Rc::new(Cell::new(0)),
    };
    let pm = PackageManager::new(fs.clone());
    let bytes = apk("org.fixture.one", 1);
    let first = pm.install(bytes.clone(), false, None).unwrap();
    fs.count.set(0);
    fs.fail.set(Some(0));
    assert_eq!(pm.install(bytes.clone(), true, None).unwrap(), first);
    assert_eq!(
        fs.count.get(),
        0,
        "identical update must not write SD storage"
    );
    assert_eq!(
        pm.install(bytes.clone(), false, None).unwrap_err().code,
        "ALREADY_INSTALLED"
    );
    fs.fail.set(None);
    let path = format!(
        "installed/{}/{:020}/package.apk",
        first.package, first.revision
    );
    fs::write_all(&fs, &path, b"damaged").unwrap();
    assert_eq!(
        pm.install(bytes.clone(), true, None).unwrap_err().code,
        "INSTALLATION_CORRUPT"
    );
    assert_eq!(pm.latest(&first.package).unwrap().unwrap(), first);
    fs::write_all(&fs, &path, &bytes).unwrap();
    pm.uninstall(&first.package).unwrap();
    let reinstalled = pm.install(bytes, true, None).unwrap();
    assert!(reinstalled.revision > first.revision);
    assert_eq!(pm.load(&first.package, None).unwrap().0, reinstalled);
}
#[test]
fn copied_apks_restart_isolation_aliases_and_uninstall_retention() {
    let path = root("persist");
    let fs = HostFileSystem::new(&path).unwrap();
    let pm = PackageManager::new(fs.clone());
    let original = apk("org.fixture.one", 1);
    let source = path.join("apks/source.apk");
    pm.initialize().unwrap();
    std::fs::write(&source, &original).unwrap();
    let a = pm
        .install(std::fs::read(&source).unwrap(), false, None)
        .unwrap();
    std::fs::remove_file(&source).unwrap();
    pm.install(apk("org.fixture.two", 1), false, None).unwrap();
    let vfs = pm.vfs(&a.package).unwrap();
    vfs.write_at("/data/user/0/org.fixture.one/files/value", 0, b"one", true)
        .unwrap();
    assert!(vfs
        .resolve("/data/user/0/org.fixture.two/files/value")
        .is_err());
    assert!(vfs
        .resolve("/sdcard/userdata/org.fixture.two/value")
        .is_err());
    vfs.write_at(
        "/sdcard/Android/obb/org.fixture.one/main.obb",
        0,
        b"opaque obb",
        true,
    )
    .unwrap();
    let reopened = PackageManager::new(HostFileSystem::new(&path).unwrap());
    assert_eq!(reopened.list().unwrap().len(), 2);
    assert_eq!(
        reopened.load(&a.package, Some(a.revision)).unwrap().1,
        original
    );
    let mounted = reopened.vfs(&a.package).unwrap();
    let mut obb = [0; 10];
    assert_eq!(
        mounted
            .read_at(
                "/storage/emulated/0/Android/obb/org.fixture.one/main.obb",
                0,
                &mut obb
            )
            .unwrap(),
        10
    );
    assert_eq!(&obb, b"opaque obb");
    let update = reopened.install(apk(&a.package, 2), true, None).unwrap();
    assert!(update.revision > a.revision);
    assert!(reopened.load(&a.package, Some(a.revision)).is_err());
    let removed = reopened.uninstall(&a.package).unwrap();
    assert!(removed.data_retained && removed.cleanup_errors.is_empty());
    assert_eq!(reopened.list().unwrap().len(), 1);
    let mut value = [0; 3];
    mounted
        .read_at("/data/user/0/org.fixture.one/files/value", 0, &mut value)
        .unwrap();
    assert_eq!(&value, b"one");
    reopened.clear_data(&a.package).unwrap();
    assert!(mounted
        .stat("/data/user/0/org.fixture.one/files/value")
        .unwrap()
        .is_none());
}
#[test]
fn traversal_and_unknown_root_paths_are_rejected() {
    let fs = HostFileSystem::new(root("paths")).unwrap();
    let vfs = Vfs::new(fs.clone(), "org.fixture.one").unwrap();
    for path in [
        "/sdcard/../userdata/org.fixture.other/save",
        "/data/user/0/org.fixture.one/../org.fixture.other/save",
        "/sdcard/installed/x",
        "/sdcard\\userdata",
        "/storage/emulated/0/userdata",
        "/sdcard/C:/secret",
    ] {
        assert!(vfs.resolve(path).is_err(), "{path}");
    }
    assert_eq!(
        vfs.resolve("/sdcard/Android/data/org.fixture.one/../org.fixture.one/save")
            .unwrap(),
        "Android/data/org.fixture.one/save"
    );
    assert!(fs.write_at("../escape", 0, b"bad", true).is_err());
}
