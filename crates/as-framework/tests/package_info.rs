//! Real committed APK metadata, normal SDK DEX queries, and shared VFS identity.
use as_framework::{App, AppOptions};
use as_package::{PackageManager, Record};
use as_platform::{
    fs::{Entry, FileSystem, Metadata, ReadOnlyFile, WriteFile},
    host::HostPlatform,
    host_fs::HostFileSystem,
    PlatformError,
};
use rd_vm::{
    framework_host::{FrameworkHost, HostMethodKind},
    gc::FREED_CLASS,
    ObjRef, Value, VmExit,
};
use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
const PROBE: &str = "Lorg/androswitch/contracts/PackageMetadataProbe;";
const INFO_SIG: &str = "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;";
const MISSING: &str = "Landroid/content/pm/PackageManager$NameNotFoundException;";
struct Reader {
    inner: Arc<dyn ReadOnlyFile>,
    bytes: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}
impl ReadOnlyFile for Reader {
    fn len(&self) -> u64 {
        self.inner.len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize, PlatformError> {
        let n = self.inner.read_at(offset, output)?;
        self.bytes.fetch_add(n, Ordering::Relaxed);
        self.maximum.fetch_max(output.len(), Ordering::Relaxed);
        Ok(n)
    }
}
struct CountingFs {
    inner: HostFileSystem,
    bytes: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}
impl FileSystem for CountingFs {
    fn begin_write(&self, p: &str) -> Result<Option<Box<dyn WriteFile>>, PlatformError> {
        self.inner.begin_write(p)
    }
    fn open_read(&self, p: &str) -> Result<Arc<dyn ReadOnlyFile>, PlatformError> {
        let inner = self.inner.open_read(p)?;
        if p.ends_with(".apk") {
            Ok(Arc::new(Reader {
                inner,
                bytes: self.bytes.clone(),
                maximum: self.maximum.clone(),
            }))
        } else {
            Ok(inner)
        }
    }
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        self.inner.stat(p)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        self.inner.read_at(p, o, b)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        self.inner.write_at(p, o, b, t)
    }
    fn create_dir(&self, p: &str, e: bool) -> Result<bool, PlatformError> {
        self.inner.create_dir(p, e)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        self.inner.list(p)
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        self.inner.rename_new(a, b)
    }
    fn remove(&self, p: &str, r: bool) -> Result<(), PlatformError> {
        self.inner.remove(p, r)
    }
}
struct Harness {
    app: App,
    fs: Rc<CountingFs>,
    record: Record,
}
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/package-info-fixture/{name}.apk")),
    )
    .expect("run fixtures/m9/package-info/build.ps1")
}
impl Harness {
    #[allow(clippy::assertions_on_constants)]
    fn new(fixture: &str) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-package-info-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(CountingFs {
            inner: HostFileSystem::new(root).unwrap(),
            bytes: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
        });
        let pm = PackageManager::new(fs.clone());
        let record = pm.install(bytes(fixture), true, None).unwrap();
        let apk = pm.load_apk(&record.package, None).unwrap().1;
        let app = App::new_with_options(
            apk,
            fs.clone(),
            Rc::new(HostPlatform::default()),
            640,
            360,
            None,
            AppOptions {
                source_apk: Some(format!(
                    "installed/{}/{:020}/package.apk",
                    record.package, record.revision
                )),
                ..Default::default()
            },
        )
        .unwrap();
        Self { app, fs, record }
    }
    fn install(&self, fixture: &str) -> Record {
        PackageManager::new(self.fs.clone())
            .install(bytes(fixture), true, None)
            .unwrap()
    }
    fn launch(&mut self) {
        self.app.launch().unwrap();
    }
    fn query(&mut self, name: Value, flags: i32) -> Result<ObjRef, VmExit> {
        self.app
            .vm
            .invoke_static(PROBE, "query", INFO_SIG, &[name, Value::Int(flags)])
            .map(|v| v.as_ref().unwrap().unwrap())
    }
    fn own(&mut self) -> ObjRef {
        self.query(Value::StrPlaceholder(self.record.package.clone()), 0)
            .unwrap()
    }
    fn field(&self, r: ObjRef, name: &str) -> Value {
        self.app.vm.heap.get_field(r, name).unwrap()
    }
    fn text(&self, v: Value) -> String {
        self.app
            .vm
            .heap
            .as_str(v.as_ref().unwrap().unwrap())
            .unwrap()
            .into()
    }
    fn clear(&mut self) {
        self.app
            .vm
            .invoke_static(PROBE, "clear", "()V", &[])
            .unwrap();
    }
    fn missing(&mut self, result: Result<ObjRef, VmExit>, name: Option<&str>) {
        let Err(VmExit::Exception(throwable)) = result else {
            panic!("expected NameNotFoundException: {result:?}");
        };
        assert_eq!(throwable.class, MISSING);
        let object = throwable
            .obj
            .expect("real NameNotFoundException is materialized before transport diagnostics");
        let message = self
            .app
            .vm
            .invoke_instance(object, "getMessage", "()Ljava/lang/String;", &[])
            .unwrap();
        match name {
            Some(name) => assert_eq!(self.text(message), name),
            None => assert_eq!(message, Value::Null),
        }
    }
    fn unsupported(&mut self, method: &str, sig: &str, name: &str) {
        let result =
            self.app
                .vm
                .invoke_static(PROBE, method, sig, &[Value::StrPlaceholder(name.into())]);
        assert!(
            matches!(result,Err(VmExit::Error(ref e)) if e.code=="API_UNAVAILABLE"),
            "{result:?}"
        );
    }
}
#[test]
fn normal_sdk_device_proof_reads_own_versions_real_apk_and_typed_failures_in_both_identities() {
    for fixture in ["package-info", "renamed"] {
        let mut h = Harness::new(fixture);
        h.launch();
        assert!(h
            .app
            .android
            .views()
            .iter()
            .any(|v| v.text.contains("PACKAGE INFO PASS")));
        let mut output = vec![0; 4096];
        let count = h
            .app
            .vm
            .guest_filesystem()
            .unwrap()
            .read_at(
                "/sdcard/Download/androswitch-package-info-proof.txt",
                0,
                &mut output,
            )
            .unwrap();
        let output = String::from_utf8(output[..count].to_vec()).unwrap();
        assert!(output.contains(&format!("package={}\n", h.record.package)));
        assert!(output.contains("versionCode=7\nversionName=2.4-proof\n"));
        assert!(output.contains(
            "apk_header=PK\nunknown=NameNotFoundException\nnull=NameNotFoundException\n"
        ));
        let info = h.own();
        assert_eq!(h.field(info, "versionCode"), Value::Int(7));
        assert_eq!(
            h.app.android.method_metadata(
                "Landroid/content/pm/PackageManager;",
                "getPackageInfo",
                INFO_SIG
            ),
            Some(HostMethodKind::Instance)
        );
        assert_eq!(
            h.app.android.method_metadata(
                "Landroid/content/pm/PackageManager;",
                "getPackageInfo",
                "(Ljava/lang/String;)Landroid/content/pm/PackageInfo;"
            ),
            None
        );
        assert!(h
            .app
            .vm
            .cp
            .is_subtype(MISSING, "Landroid/util/AndroidException;"));
        assert!(h.app.vm.cp.is_subtype(MISSING, "Ljava/lang/Exception;"));
        h.app.android.stop(&mut h.app.vm).unwrap();
    }
}
#[test]
fn cross_package_metadata_is_committed_and_file_backed_without_extracting_large_assets() {
    let mut h = Harness::new("package-info");
    let other = h.install("companion");
    h.launch();
    assert!(other.apk_bytes > 2 << 20);
    h.fs.bytes.store(0, Ordering::Relaxed);
    h.fs.maximum.store(0, Ordering::Relaxed);
    let info = h
        .query(Value::StrPlaceholder(other.package.clone()), 0)
        .unwrap();
    assert_eq!(h.field(info, "versionCode"), Value::Int(42));
    assert_eq!(h.text(h.field(info, "versionName")), "9.2-other");
    assert_eq!(h.text(h.field(info, "packageName")), other.package);
    let app = h.field(info, "applicationInfo").as_ref().unwrap().unwrap();
    assert_eq!(h.text(h.field(app, "packageName")), other.package);
    assert_eq!(h.field(app, "minSdkVersion"), Value::Int(26));
    assert!(h.fs.bytes.load(Ordering::Relaxed) < 256 << 10);
    assert!(h.fs.maximum.load(Ordering::Relaxed) <= 64 << 10);
    h.unsupported(
        "source",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &other.package,
    );
}
#[test]
fn nullable_default_versions_fresh_mutable_snapshots_and_gc_follow_actual_field_edges() {
    let mut h = Harness::new("package-info");
    let other = h.install("defaults");
    h.launch();
    let info = h
        .query(Value::StrPlaceholder(other.package.clone()), 0)
        .unwrap();
    assert_eq!(h.field(info, "versionCode"), Value::Int(0));
    assert_eq!(h.field(info, "versionName"), Value::Null);
    let application = h.field(info, "applicationInfo").as_ref().unwrap().unwrap();
    let package = h.field(info, "packageName").as_ref().unwrap().unwrap();
    h.app.vm.gc();
    assert_ne!(h.app.vm.heap.class_of(info).unwrap(), FREED_CLASS);
    assert_ne!(h.app.vm.heap.class_of(application).unwrap(), FREED_CLASS);
    assert_ne!(h.app.vm.heap.class_of(package).unwrap(), FREED_CLASS);
    h.app
        .vm
        .heap
        .put_field(info, "versionCode", Value::Int(999))
        .unwrap();
    let fresh = h.query(Value::StrPlaceholder(other.package), 0).unwrap();
    assert_ne!(fresh, info);
    assert_eq!(h.field(fresh, "versionCode"), Value::Int(0));
    h.clear();
    h.app.vm.gc();
    for r in [info, application, package, fresh] {
        assert_eq!(h.app.vm.heap.class_of(r).unwrap(), FREED_CLASS);
    }
}
#[test]
fn updated_versions_do_not_reuse_running_old_source_alias_and_uninstalled_packages_are_missing() {
    let mut h = Harness::new("package-info");
    h.launch();
    let old = h.own();
    assert_eq!(h.field(old, "versionCode"), Value::Int(7));
    let updated = h.install("updated");
    assert!(updated.revision > h.record.revision);
    let fresh = h.own();
    assert_eq!(h.field(fresh, "versionCode"), Value::Int(8));
    assert_eq!(h.text(h.field(fresh, "versionName")), "2.5-updated");
    assert_eq!(h.field(old, "versionCode"), Value::Int(7));
    let name = h.record.package.clone();
    h.unsupported("source", "(Ljava/lang/String;)Ljava/lang/String;", &name);
    PackageManager::new(h.fs.clone()).uninstall(&name).unwrap();
    let result = h.query(Value::StrPlaceholder(name.clone()), 0);
    h.missing(result, Some(&name));
}
#[test]
fn missing_null_invalid_and_case_mismatched_names_do_not_create_package_existence() {
    let mut h = Harness::new("package-info");
    h.launch();
    for name in [
        None,
        Some(""),
        Some("../outside"),
        Some("org.fixture.packagemetadata.missing"),
        Some("ORG.fixture.packagemetadata"),
    ] {
        let result = h.query(
            name.map_or(Value::Null, |s| Value::StrPlaceholder(s.into())),
            0,
        );
        h.missing(result, name);
    }
    let ghost = "org.fixture.packagecompanion";
    let apk = rd_apk::Apk::from_bytes(bytes("companion")).unwrap();
    assert_eq!(apk.manifest.package, ghost);
    // Loading/parsing an APK is not installing it in the committed registry.
    assert!(
        matches!(h.query(Value::StrPlaceholder(ghost.into()),0),Err(VmExit::Exception(ref t)) if t.class==MISSING)
    );
}
#[test]
fn unsupported_flags_fields_and_resource_metadata_fail_instead_of_inventing_values() {
    let mut h = Harness::new("package-info");
    let resource = h.install("resource-version");
    let boolean = h.install("resource-boolean");
    h.launch();
    let own = h.record.package.clone();
    for flags in [1, 64, 128, 8192, -1] {
        let result = h.query(Value::StrPlaceholder(own.clone()), flags);
        assert!(
            matches!(result,Err(VmExit::Error(ref e)) if e.code=="API_UNAVAILABLE"),
            "{flags}: {result:?}"
        );
    }
    for (method, sig) in [
        ("installTime", "(Ljava/lang/String;)J"),
        ("signatures", "(Ljava/lang/String;)Ljava/lang/Object;"),
        ("uid", "(Ljava/lang/String;)I"),
        ("flags", "(Ljava/lang/String;)I"),
    ] {
        h.unsupported(method, sig, &own);
    }
    h.query(Value::StrPlaceholder(boolean.package.clone()), 0)
        .unwrap();
    h.unsupported("flags", "(Ljava/lang/String;)I", &boolean.package);
    let result = h.query(Value::StrPlaceholder(resource.package), 0);
    assert!(
        matches!(result,Err(VmExit::Error(ref e)) if e.code=="API_UNAVAILABLE"),
        "{result:?}"
    );
}
#[test]
fn mismatched_installed_manifest_is_corruption_not_fabricated_metadata() {
    let mut h = Harness::new("package-info");
    let other = h.install("defaults");
    h.launch();
    let path = format!(
        "installed/{}/{:020}/package.apk",
        other.package, other.revision
    );
    let replacement = bytes("renamed");
    as_platform::fs::write_all(h.fs.as_ref(), &path, &replacement).unwrap();
    let result = h.query(Value::StrPlaceholder(other.package), 0);
    assert!(
        matches!(result,Err(VmExit::Error(ref e)) if e.code=="INSTALLATION_CORRUPT"),
        "{result:?}"
    );
}

#[test]
fn min_only_empty_and_absent_uses_sdk_use_actual_android_manifest_defaults() {
    let mut h = Harness::new("package-info");
    let min_only = h.install("min-only");
    let empty = h.install("empty-sdk");
    let absent = h.install("no-sdk");
    h.launch();
    for (record, expected) in [(min_only, 26), (empty, 1), (absent, 1)] {
        let info = h.query(Value::StrPlaceholder(record.package), 0).unwrap();
        let application = h.field(info, "applicationInfo").as_ref().unwrap().unwrap();
        assert_eq!(h.field(application, "minSdkVersion"), Value::Int(expected));
        assert_eq!(
            h.field(application, "targetSdkVersion"),
            Value::Int(expected)
        );
    }
}
