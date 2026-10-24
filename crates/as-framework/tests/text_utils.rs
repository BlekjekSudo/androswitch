//! Actual portable DEX/CharSequence dispatch compared with a pure-Java oracle.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{cell::Cell, path::PathBuf, rc::Rc, thread::ThreadId};
const FIXTURE: &str = "Lorg/androswitch/contracts/TextFixture;";
const CASES: &str = "Lorg/androswitch/contracts/TextCases;";
const TEXT: &str = "Landroid/text/TextUtils;";
struct Collector {
    owner: ThreadId,
    calls: Cell<usize>,
}
impl NativeHost for Collector {
    fn current_thread_id(&self) -> Option<u64> {
        // Opaque owner1 denotes this VM's checked actual Rust test thread.
        (std::thread::current().id() == self.owner).then_some(1)
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error("API_UNAVAILABLE", "test has no native library loader").into())
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(
            (class, name, sig, is_static),
            (
                "Lorg/androswitch/contracts/TextFixture$NativeCollector;",
                "collect",
                "()V",
                false
            )
        );
        assert_eq!(slots.len(), 1);
        self.calls.set(self.calls.get() + 1);
        vm.gc();
        Ok(Value::Null)
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
}
#[allow(clippy::assertions_on_constants)]
fn app(name: &str) -> (App, Rc<Collector>) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-text-utils-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/text-utils-fixture/{name}.apk"));
    let record = pm
        .install(
            std::fs::read(file).expect("run fixtures/m9/text-utils/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    app.launch().unwrap();
    let collector = Rc::new(Collector {
        owner: std::thread::current().id(),
        calls: Cell::new(0),
    });
    app.vm.bind_native(collector.clone());
    (app, collector)
}
fn call(app: &mut App, name: &str, kind: i32) -> Result<Value, VmExit> {
    app.vm
        .invoke_static(FIXTURE, name, "(I)I", &[Value::Int(kind)])
}
fn counter(app: &App, name: &str) -> i32 {
    app.vm.statics[&(CASES.into(), name.into())]
        .as_int()
        .unwrap()
}
#[test]
fn null_strings_builders_and_custom_virtual_lengths_match_jvm_in_both_packages() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/text-utils-fixture/jvm-oracle.txt"),
    )
    .unwrap();
    assert_eq!(oracle.lines().count(), 19);
    for package in ["text-utils", "renamed"] {
        let (mut app, collector) = app(package);
        for line in oracle.lines() {
            let columns: Vec<_> = line.split('\t').collect();
            let kind = columns[0].parse().unwrap();
            let result = call(&mut app, "run", kind);
            if columns[1] == "VALUE" {
                assert_eq!(
                    result.unwrap(),
                    Value::Int(columns[2].parse().unwrap()),
                    "case{kind} in {package}"
                );
            } else {
                let Err(VmExit::Exception(failure)) = result else {
                    panic!("expected custom length exception")
                };
                assert_eq!(failure.class, "Ljava/lang/IllegalStateException;");
                let diagnostic = failure.message.as_deref().unwrap();
                assert_eq!(diagnostic.lines().next(), Some(columns[3]));
                assert!(diagnostic.contains("TextCases$ThrowingSequence;->length()I"));
                assert_eq!(counter(&app, "calls"), columns[4].parse::<i32>().unwrap());
            }
            assert_eq!(counter(&app, "toStrings"), 0);
            app.vm.gc();
        }
        assert_eq!(collector.calls.get(), 1);
        // Compare the actual Java message, separately from VmExit's optional
        // diagnostic stack suffix on an uncaught exception.
        let caught = app
            .vm
            .invoke_static(FIXTURE, "caught", "()Ljava/lang/String;", &[])
            .unwrap();
        assert_eq!(
            app.vm
                .heap
                .as_str(caught.as_ref().unwrap().unwrap())
                .unwrap(),
            "length failure"
        );
    }
}
#[test]
fn utf16_builder_and_buffer_content_is_counted_and_null_register_zero_is_normalized() {
    let (mut app, _) = app("text-utils");
    for kind in [3, 8, 17, 18] {
        assert_eq!(call(&mut app, "length", kind).unwrap(), Value::Int(2));
    }
    assert_eq!(call(&mut app, "length", 6).unwrap(), Value::Int(0));
    assert_eq!(call(&mut app, "length", 7).unwrap(), Value::Int(1));
    for null in [Value::Null, Value::Int(0)] {
        assert_eq!(
            app.vm
                .invoke_static(TEXT, "isEmpty", "(Ljava/lang/CharSequence;)Z", &[null])
                .unwrap(),
            Value::Int(1)
        );
    }
    assert_eq!(call(&mut app, "run", 13).unwrap(), Value::Int(11));
    assert_eq!(counter(&app, "calls"), 1);
    assert_eq!(call(&mut app, "run", 12).unwrap(), Value::Int(101));
    assert_eq!(counter(&app, "calls"), 10);
}
#[test]
fn callback_gc_keeps_sequence_payload_alive_and_wrong_types_or_unsupported_descriptors_fail() {
    for package in ["text-utils", "renamed"] {
        let (mut app, collector) = app(package);
        assert_eq!(call(&mut app, "run", 15).unwrap(), Value::Int(10));
        assert_eq!(collector.calls.get(), 1);
        assert_eq!(counter(&app, "calls"), 1);
        app.vm.gc();
        let wrong = app
            .vm
            .invoke_static(FIXTURE, "wrong", "()Ljava/lang/Object;", &[])
            .unwrap();
        let result = app
            .vm
            .invoke_static(TEXT, "isEmpty", "(Ljava/lang/CharSequence;)Z", &[wrong]);
        assert!(matches!(result,Err(VmExit::Error(error)) if error.code=="VM_TYPE_ERROR"));
        assert_eq!(counter(&app, "calls"), 1);
        let result = app
            .vm
            .invoke_static(TEXT, "isEmpty", "(Ljava/lang/String;)Z", &[Value::Null]);
        assert!(matches!(result,Err(VmExit::Error(error)) if error.code=="NOT_FOUND"));
        let result = app.vm.invoke_static(
            TEXT,
            "isGraphic",
            "(Ljava/lang/CharSequence;)Z",
            &[Value::Null],
        );
        assert!(matches!(result,Err(VmExit::Error(error)) if error.code=="NOT_FOUND"));
    }
}
