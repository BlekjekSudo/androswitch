//! Portable Throwable message semantics; cause/stack APIs remain unsupported.
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Throwable, Value, VmConfig, VmExit};
use std::rc::Rc;
const PROBE: &str = "LLorg/androswitch/vm/ThrowableFixture;";
fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-throwable-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn configured(config: VmConfig) -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    Engine::new(
        vec![rd_dex::Dex::parse(
            std::fs::read(root().join("dex/classes.dex"))
                .expect("run crates/rd-vm/tests/fixtures/portable-throwable/build.ps1"),
        )
        .unwrap()],
        config,
    )
}
fn vm() -> Engine {
    configured(VmConfig::default())
}
fn text(vm: &Engine, value: Value) -> Option<String> {
    value
        .as_ref()
        .unwrap()
        .map(|r| vm.heap.as_str(r).unwrap().into())
}
fn throwable(result: Result<Value, VmExit>) -> Throwable {
    match result {
        Err(VmExit::Exception(value)) => value,
        other => panic!("expected exception: {other:?}"),
    }
}
fn unavailable(result: Result<Value, VmExit>) {
    assert!(matches!(result,Err(VmExit::Error(error)) if error.code=="API_UNAVAILABLE"));
}

#[test]
fn fifty_one_jvm_cases_preserve_nullable_messages_overrides_and_super_dispatch() {
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.txt")).unwrap();
    assert_eq!(oracle.lines().count(), 51);
    let mut vm = vm();
    for _ in 0..2 {
        for row in oracle.lines() {
            let parts: Vec<_> = row.split('\t').collect();
            let value = vm
                .invoke_static(
                    PROBE,
                    "probe",
                    "(II)Ljava/lang/String;",
                    &[
                        Value::Int(parts[0].parse().unwrap()),
                        Value::Int(parts[1].parse().unwrap()),
                    ],
                )
                .unwrap();
            assert_eq!(
                text(&vm, value).as_deref().unwrap_or("<null>"),
                parts[2],
                "{row}"
            );
        }
        vm.gc();
    }
}

#[test]
fn dex_constructors_shadow_fields_and_gc_do_not_alias_private_throwable_state() {
    let mut vm = vm();
    let message = vm
        .heap
        .alloc_string("message owned only by Throwable internals".into())
        .unwrap();
    let object = vm
        .invoke_static(
            PROBE,
            "shadow",
            "(Ljava/lang/String;)Ljava/lang/Throwable;",
            &[Value::Obj(message)],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(
        vm.statics.get(&(PROBE.into(), "constructors".into())),
        Some(&Value::Int(1)),
        "real DEX constructor side effect must execute"
    );
    assert_eq!(
        text(&vm, vm.heap.get_field(object, "message").unwrap()).as_deref(),
        Some("shadow")
    );
    assert_eq!(
        text(&vm, vm.heap.get_field(object, "detailMessage").unwrap()).as_deref(),
        Some("shadow detail")
    );
    vm.statics
        .insert(("LRoot;".into(), "throwable".into()), Value::Obj(object));
    vm.invoke_static(PROBE, "tick", "()V", &[]).unwrap();
    vm.gc();
    assert_eq!(
        vm.invoke_instance(object, "getMessage", "()Ljava/lang/String;", &[])
            .unwrap(),
        Value::Obj(message)
    );
    let thrown = throwable(vm.invoke_static(
        PROBE,
        "throwShadow",
        "(Ljava/lang/String;)V",
        &[Value::StrPlaceholder("original detail".into())],
    ));
    assert_eq!(thrown.message.as_deref(), Some("original detail"));
    assert!(thrown.class.ends_with("ThrowableFixture$Shadow;"));
    vm.statics.remove(&("LRoot;".into(), "throwable".into()));
    vm.invoke_static(PROBE, "tick", "()V", &[]).unwrap();
    vm.gc();
    assert_eq!(vm.heap.class_of(object).unwrap(), rd_vm::gc::FREED_CLASS);
    assert_eq!(vm.heap.class_of(message).unwrap(), rd_vm::gc::FREED_CLASS);
}

struct Failure;
impl NativeHost for Failure {
    fn current_thread_id(&self) -> Option<u64> {
        Some(1) // Controlled single-threaded host provider, not a device ID.
    }
    fn load(&self, _vm: &mut Engine, _name: &str, _absolute: bool) -> Result<(), VmExit> {
        unreachable!()
    }
    fn invoke(
        &self,
        _vm: &mut Engine,
        _class: &str,
        name: &str,
        _descriptor: &str,
        _is_static: bool,
        _slots: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(name, "nativeFailure");
        Err(Throwable::new("Ljava/lang/IllegalStateException;", "native detail").into())
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
}
#[test]
fn ordinary_vm_generated_and_native_materialized_errors_keep_real_message_state() {
    let mut vm = vm();
    let failure = throwable(vm.invoke_static(PROBE, "fail", "()V", &[]));
    assert_eq!(failure.class, "Ljava/lang/IllegalStateException;");
    assert_eq!(failure.message.as_deref(), Some("timer task failed"));
    let value = vm
        .invoke_instance(
            failure.obj.unwrap(),
            "getMessage",
            "()Ljava/lang/String;",
            &[],
        )
        .unwrap();
    assert_eq!(text(&vm, value).as_deref(), Some("timer task failed"));
    let generated = vm
        .invoke_static(PROBE, "generatedMessage", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(text(&vm, generated).as_deref(), Some("divide by zero"));
    let generated = throwable(vm.invoke_static(PROBE, "rethrowGenerated", "()V", &[]));
    assert_eq!(generated.class, "Ljava/lang/ArithmeticException;");
    assert_eq!(generated.message.as_deref(), Some("divide by zero"));
    vm.bind_native(Rc::new(Failure));
    let value = vm
        .invoke_static(PROBE, "caughtNative", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(text(&vm, value).as_deref(), Some("native detail"));
}

#[test]
fn unsupported_constructor_cause_and_stack_contracts_never_succeed_silently() {
    let mut vm = vm();
    for (name, sig) in [
        ("causeConstructor", "()Ljava/lang/Throwable;"),
        ("pairConstructor", "()Ljava/lang/Throwable;"),
        ("cause", "()Ljava/lang/Throwable;"),
        ("stack", "()[Ljava/lang/StackTraceElement;"),
    ] {
        unavailable(vm.invoke_static(PROBE, name, sig, &[]));
    }
    let class = "Ljava/util/IllegalFormatConversionException;";
    let object = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
    vm.attach_host_state(class, object).unwrap();
    unavailable(vm.invoke_instance(object, "<init>", "()V", &[]));
    unavailable(vm.invoke_instance(
        object,
        "<init>",
        "(Ljava/lang/String;)V",
        &[Value::StrPlaceholder("fake".into())],
    ));
}

#[test]
fn throwable_private_slot_uses_the_real_heap_budget() {
    let mut vm = configured(VmConfig {
        heap_budget: 64,
        ..Default::default()
    });
    let class = "Ljava/lang/IllegalStateException;";
    let object = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
    let result = vm.attach_host_state(class, object);
    assert!(
        matches!(result,Err(VmExit::Exception(error)) if error.class=="Ljava/lang/OutOfMemoryError;")
    );
    assert_eq!(
        vm.heap.used(),
        32,
        "failed initialization cannot silently overrun the heap budget"
    );
}
