//! Actual portable Engine and generated DEX; native identities below are a
//! controlled host provider, not claims of Android/Horizon thread execution.
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Throwable, Value, VmConfig, VmExit};
use std::{cell::Cell, rc::Rc};
const PROBE: &str = "Lorg/androswitch/vm/MonitorFixture;";
fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-monitor-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn vm() -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    Engine::new(
        vec![rd_dex::Dex::parse(
            std::fs::read(root().join("dex/classes.dex"))
                .expect("run crates/rd-vm/tests/fixtures/portable-monitor/build.ps1"),
        )
        .unwrap()],
        VmConfig::default(),
    )
}
fn object(vm: &mut Engine) -> ObjRef {
    vm.heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap()
}
fn error(code: &str) -> VmExit {
    rd_vm::err::vm_error(code, "monitor host probe").into()
}
fn assert_code<T: std::fmt::Debug>(result: Result<T, VmExit>, expected: &str) {
    assert!(
        matches!(result,Err(VmExit::Error(error)) if error.code==expected),
        "expected {expected}"
    );
}
fn assert_exception<T: std::fmt::Debug>(result: Result<T, VmExit>, expected: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(error)) if error.class==expected),
        "expected {expected}"
    );
}
struct Identity {
    id: Cell<Option<u64>>,
}
impl NativeHost for Identity {
    fn current_thread_id(&self) -> Option<u64> {
        self.id.get()
    }
    fn load(&self, _vm: &mut Engine, _name: &str, _absolute: bool) -> Result<(), VmExit> {
        Err(error("PROBE_UNUSED"))
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        _descriptor: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value, VmExit> {
        assert!(name == "staticNative" || name == "instanceNative");
        let lock = if is_static {
            vm.device_class_object(class)?
        } else {
            slots[0].as_ref()?.unwrap()
        };
        assert!(vm.monitor_holds_lock(lock)?);
        vm.monitor_enter(lock)?; // Native reentrancy has the same actual provider identity.
        vm.gc();
        assert!(vm.monitor_holds_lock(lock)?);
        vm.monitor_exit(lock)?;
        match slots[usize::from(!is_static)].as_int()? {
            0 => Ok(Value::Long(17)),
            1 => Err(Throwable::new("Ljava/lang/ArithmeticException;", "native probe").into()),
            2 => Err(error("PROBE_ERROR")),
            3 => {
                let held = vm.invoke_static(
                    PROBE,
                    "withLock",
                    "(Ljava/lang/Object;)I",
                    &[Value::Obj(lock)],
                )?;
                assert_eq!(held, Value::Int(1));
                Ok(Value::Long(19))
            }
            _ => unreachable!(),
        }
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
}
struct NoIdentity;
impl NativeHost for NoIdentity {
    fn load(&self, _vm: &mut Engine, _name: &str, _absolute: bool) -> Result<(), VmExit> {
        Err(error("PROBE_UNUSED"))
    }
    fn invoke(
        &self,
        _vm: &mut Engine,
        _class: &str,
        _name: &str,
        _descriptor: &str,
        _is_static: bool,
        _slots: &[Value],
    ) -> Result<Value, VmExit> {
        Err(error("PROBE_UNUSED"))
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
}

#[test]
fn twelve_jvm_cases_match_with_nested_locks_exceptions_gc_and_declared_methods() {
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.txt")).unwrap();
    assert_eq!(oracle.lines().count(), 12);
    let mut vm = vm();
    for _ in 0..2 {
        for row in oracle.lines() {
            let (index, expected) = row.split_once('\t').unwrap();
            assert_eq!(
                vm.invoke_static(PROBE, "run", "(I)I", &[Value::Int(index.parse().unwrap())])
                    .unwrap(),
                Value::Int(expected.parse().unwrap()),
                "{row}"
            );
        }
        vm.gc();
    }
}

#[test]
fn ownership_reentrancy_null_invalid_and_contention_are_real_and_gc_rooted() {
    let mut vm = vm();
    let backend = Rc::new(Identity {
        id: Cell::new(Some(7)),
    });
    vm.bind_native(backend.clone());
    let lock = object(&mut vm);
    assert_exception(vm.monitor_enter(0), "Ljava/lang/NullPointerException;");
    assert_exception(vm.monitor_exit(0), "Ljava/lang/NullPointerException;");
    assert_exception(
        vm.monitor_exit(lock),
        "Ljava/lang/IllegalMonitorStateException;",
    );
    assert!(vm.monitor_enter(u32::MAX).is_err());
    vm.monitor_enter(lock).unwrap();
    vm.monitor_enter(lock).unwrap();
    vm.gc();
    assert_eq!(vm.heap.class_of(lock).unwrap(), "Ljava/lang/Object;");
    backend.id.set(Some(8));
    assert!(!vm.monitor_holds_lock(lock).unwrap());
    assert_code(vm.monitor_enter(lock), "API_UNAVAILABLE");
    assert_exception(
        vm.monitor_exit(lock),
        "Ljava/lang/IllegalMonitorStateException;",
    );
    backend.id.set(Some(7));
    vm.monitor_exit(lock).unwrap();
    assert!(
        vm.monitor_holds_lock(lock).unwrap(),
        "contention and wrong exit preserved recursion count"
    );
    vm.monitor_exit(lock).unwrap();
    assert!(!vm.monitor_holds_lock(lock).unwrap());
    vm.gc();
    assert_eq!(vm.heap.class_of(lock).unwrap(), rd_vm::gc::FREED_CLASS);
    assert_code(vm.monitor_enter(lock), "VM_TYPE_ERROR");
}

#[test]
fn unavailable_thread_identity_never_becomes_a_synthetic_native_owner() {
    let mut vm = vm();
    let lock = object(&mut vm);
    vm.bind_native(Rc::new(NoIdentity));
    assert_code(vm.monitor_enter(lock), "API_UNAVAILABLE");
    assert_code(vm.monitor_holds_lock(lock), "API_UNAVAILABLE");
    let backend = Rc::new(Identity {
        id: Cell::new(Some(0)),
    });
    vm.bind_native(backend.clone());
    assert_code(vm.monitor_enter(lock), "API_UNAVAILABLE");
    backend.id.set(Some(1));
    vm.monitor_enter(lock).unwrap();
    vm.monitor_exit(lock).unwrap();
    let mut host = crate::vm();
    let lock = object(&mut host);
    host.monitor_enter(lock).unwrap(); // Real single-interpreter identity.
    host.bind_native(Rc::new(Identity {
        id: Cell::new(Some(1)),
    }));
    // Native id 1 must not impersonate the single host interpreter.
    assert_code(host.monitor_enter(lock), "API_UNAVAILABLE");
}

#[test]
fn synchronized_native_class_and_instance_locks_release_on_every_exit() {
    let mut vm = vm();
    vm.bind_native(Rc::new(Identity {
        id: Cell::new(Some(41)),
    }));
    for name in ["staticNative", "instanceNative"] {
        let (_, _, method) = vm.cp.resolve_method(PROBE, name, "(I)J").unwrap();
        assert_ne!(
            method.access_flags & 0x20,
            0,
            "fixture must contain actual ACC_SYNCHRONIZED"
        );
    }
    let instance = vm.new_instance(PROBE).unwrap();
    vm.statics
        .insert(("LProbe;".into(), "instance".into()), Value::Obj(instance));
    let class = vm.device_class_object(PROBE).unwrap();
    for behavior in 0..4 {
        for is_static in [true, false] {
            let result = if is_static {
                vm.invoke_static(PROBE, "staticNative", "(I)J", &[Value::Int(behavior)])
            } else {
                vm.invoke_instance(instance, "instanceNative", "(I)J", &[Value::Int(behavior)])
            };
            match behavior {
                0 => assert_eq!(result.unwrap(), Value::Long(17)),
                1 => assert_exception(result, "Ljava/lang/ArithmeticException;"),
                2 => assert_code(result, "PROBE_ERROR"),
                3 => assert_eq!(result.unwrap(), Value::Long(19)),
                _ => unreachable!(),
            }
            assert!(!vm
                .monitor_holds_lock(if is_static { class } else { instance })
                .unwrap());
        }
    }
    // A synchronized callee releases its own acquisition, preserving an outer
    // caller's ownership and its original recursion count.
    vm.monitor_enter(class).unwrap();
    assert_code(
        vm.invoke_static(PROBE, "staticNative", "(I)J", &[Value::Int(2)]),
        "PROBE_ERROR",
    );
    assert!(vm.monitor_holds_lock(class).unwrap());
    vm.monitor_exit(class).unwrap();
    assert!(!vm.monitor_holds_lock(class).unwrap());
}

#[test]
fn wait_set_is_explicitly_unavailable_and_never_releases_the_owned_lock() {
    let mut vm = vm();
    let lock = object(&mut vm);
    for method in ["unsupportedWait", "unsupportedNotify"] {
        assert_exception(
            vm.invoke_static(PROBE, method, "(Ljava/lang/Object;)V", &[Value::Obj(lock)]),
            "Ljava/lang/IllegalMonitorStateException;",
        );
        vm.monitor_enter(lock).unwrap();
        assert_code(
            vm.invoke_static(PROBE, method, "(Ljava/lang/Object;)V", &[Value::Obj(lock)]),
            "API_UNAVAILABLE",
        );
        assert!(vm.monitor_holds_lock(lock).unwrap());
        vm.monitor_exit(lock).unwrap();
    }
}

#[test]
fn monitor_and_recursion_budgets_fail_without_changing_previous_ownership() {
    let mut vm = vm();
    let locks: Vec<_> = (0..1025).map(|_| object(&mut vm)).collect();
    for lock in &locks[..1024] {
        vm.monitor_enter(*lock).unwrap();
    }
    assert_code(vm.monitor_enter(locks[1024]), "MONITOR_BUDGET");
    assert!(!vm.monitor_holds_lock(locks[1024]).unwrap());
    for lock in &locks[..1024] {
        vm.monitor_exit(*lock).unwrap();
    }
    let lock = locks[0];
    for _ in 0..65535 {
        vm.monitor_enter(lock).unwrap();
    }
    assert_code(vm.monitor_enter(lock), "MONITOR_BUDGET");
    for _ in 0..65535 {
        vm.monitor_exit(lock).unwrap();
    }
    assert!(!vm.monitor_holds_lock(lock).unwrap());
}
