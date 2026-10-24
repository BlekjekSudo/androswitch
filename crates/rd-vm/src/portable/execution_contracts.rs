//! Real Engine/DEX invocation regressions. Controlled native identities model
//! VM-lease suspension; these are host contracts, not device thread evidence.
use super::*;
use crate::{Engine, Throwable, Value, VmConfig};
use alloc::{format, vec};
use core::cell::Cell;

fn class(prefix: &str, nested: &str) -> String {
    format!("L{prefix}/ExecutionFixture{nested};")
}
fn check_error<T: core::fmt::Debug>(result: Result<T, VmExit>, code: &str) {
    assert!(
        matches!(result, Err(VmExit::Error(ref error)) if error.code == code),
        "{result:?}"
    );
}
fn check_exception<T: core::fmt::Debug>(result: Result<T, VmExit>, name: &str) {
    assert!(
        matches!(result, Err(VmExit::Exception(ref error)) if error.class == name),
        "{result:?}"
    );
}

struct LocalHost {
    id: Cell<u64>,
    fixture: String,
    payload: Cell<ObjRef>,
    scratch: Cell<ObjRef>,
    error_object: Cell<ObjRef>,
    garbage: Cell<ObjRef>,
}
impl NativeHost for LocalHost {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.id.get())
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        unreachable!()
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        owner: &str,
        name: &str,
        _: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(owner, self.fixture);
        assert_eq!(name, "nativeLocal");
        assert!(is_static);
        assert_eq!(vm.active_declaring_class()?, Some(self.fixture.clone()));
        let mode = slots[0].as_int()?;
        let payload = vm.heap.alloc_instance(
            "LLocalPayload;".into(),
            vec![("value".into(), Value::Int(1234))],
        )?;
        let scratch = vm.heap.alloc_instance("LNativeScratch;".into(), vec![])?;
        self.payload.set(payload);
        self.scratch.set(scratch);
        let exception = if mode == 1 {
            let throwable = vm.heap.alloc_instance(
                "Ljava/lang/IllegalStateException;".into(),
                vec![("payload".into(), Value::Obj(payload))],
            )?;
            self.error_object.set(throwable);
            Some(Throwable {
                class: "Ljava/lang/IllegalStateException;".into(),
                message: Some("native failure".into()),
                obj: Some(throwable),
            })
        } else {
            None
        };

        // Only a Rust local holds payload: inputs/native.roots() contain none.
        // The real DEX callback calls Engine::gc through System.gc().
        vm.invoke_static(&self.fixture, "collect", "()V", &[])?;
        assert_eq!(
            vm.heap.class_of(self.garbage.get()),
            Ok(crate::gc::FREED_CLASS)
        );
        assert_eq!(vm.heap.class_of(payload), Ok("LLocalPayload;"));
        assert_eq!(vm.heap.get_field(payload, "value"), Ok(Value::Int(1234)));
        assert_eq!(vm.heap.class_of(scratch), Ok("LNativeScratch;"));
        match mode {
            0 => Ok(Value::Obj(payload)),
            1 => Err(exception.unwrap().into()),
            2 => Err(err::vm_error("NATIVE_TEST_ERROR", "original infrastructure failure").into()),
            3 => {
                let result = vm.invoke_static(&self.fixture, "collectAndThrow", "()V", &[]);
                let Err(VmExit::Exception(throwable)) = result else {
                    panic!("callback must throw")
                };
                let object = throwable.obj.expect("DEX callback throws a real object");
                self.error_object.set(object);
                // The callback-return pin must protect its Throwable while
                // this still-active native host scope enters another DEX call.
                vm.invoke_static(&self.fixture, "collect", "()V", &[])?;
                assert_eq!(
                    vm.heap.class_of(object),
                    Ok("Ljava/lang/IllegalStateException;")
                );
                Err(throwable.into())
            }
            _ => unreachable!(),
        }
    }
}

pub fn native_host_allocations_survive_callback_gc_then_release(dex: rd_dex::Dex, prefix: &str) {
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let host = Rc::new(LocalHost {
        id: Cell::new(11),
        fixture: class(prefix, ""),
        payload: Cell::new(0),
        scratch: Cell::new(0),
        error_object: Cell::new(0),
        garbage: Cell::new(0),
    });
    vm.bind_native(host.clone());
    // Initialize before testing allocation lifetime; no conservative outer DEX
    // scope or global existing-object cutoff is permitted to hide this bug.
    vm.ensure_initialized(&host.fixture).unwrap();
    for mode in 0..4 {
        let garbage = vm
            .heap
            .alloc_instance("LUnrelatedGarbage;".into(), vec![])
            .unwrap();
        host.garbage.set(garbage);
        let outer = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(false);
        let result = vm.invoke_static(
            &host.fixture,
            "nativeLocal",
            "(I)Ljava/lang/Object;",
            &[Value::Int(mode)],
        );
        let returned = match result {
            Ok(Value::Obj(object)) if mode == 0 => Some(object),
            Err(VmExit::Exception(throwable)) if mode == 1 || mode == 3 => {
                assert_eq!(throwable.class, "Ljava/lang/IllegalStateException;");
                assert_eq!(throwable.obj, Some(host.error_object.get()));
                throwable.obj
            }
            Err(VmExit::Error(error)) if mode == 2 => {
                assert_eq!(error.code, "NATIVE_TEST_ERROR");
                assert_eq!(error.cause, "original infrastructure failure");
                None
            }
            other => panic!("unexpected native result: {other:?}"),
        };
        vm.gc();
        assert_eq!(
            vm.heap.class_of(host.scratch.get()),
            Ok(crate::gc::FREED_CLASS)
        );
        if let Some(object) = returned {
            assert_ne!(vm.heap.class_of(object), Ok(crate::gc::FREED_CLASS));
        }
        if mode <= 1 {
            assert_eq!(
                vm.heap.get_field(host.payload.get(), "value"),
                Ok(Value::Int(1234))
            );
        } else {
            assert_eq!(
                vm.heap.class_of(host.payload.get()),
                Ok(crate::gc::FREED_CLASS)
            );
        }
        vm.heap.gc_scope_restore(outer);
        vm.gc();
        if let Some(object) = returned {
            assert_eq!(vm.heap.class_of(object), Ok(crate::gc::FREED_CLASS));
        }
        assert!(vm.execution_frames.roots().next().is_none());
        assert!(vm.heap.gc_pinned_roots().next().is_none());
    }
}

struct InitializerHost {
    id: Cell<Option<u64>>,
    target: String,
    child: Option<String>,
    calls: Cell<u32>,
    fail: bool,
}
impl NativeHost for InitializerHost {
    fn current_thread_id(&self) -> Option<u64> {
        self.id.get()
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        unreachable!()
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        _: &str,
        name: &str,
        _: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            assert_eq!(name, "initFailure");
            return Err(err::vm_error(
                "API_UNAVAILABLE",
                "initializer native transport unavailable",
            )
            .into());
        }
        assert_eq!(name, "initHook");
        let partial = vm.invoke_static(&self.target, "read", "()I", &[])?;
        assert!(partial == Value::Int(7) || partial == Value::Int(0));
        self.id.set(Some(22));
        check_error(
            vm.invoke_static(&self.target, "read", "()I", &[]),
            "API_UNAVAILABLE",
        );
        assert!(!vm.device_class_is_initialized(&self.target));
        self.id.set(None);
        check_error(vm.ensure_initialized(&self.target), "API_UNAVAILABLE");
        self.id.set(Some(22));
        if let Some(child) = &self.child {
            check_error(
                vm.invoke_static(child, "readChild", "()I", &[]),
                "API_UNAVAILABLE",
            );
            check_exception(
                vm.invoke_static(child, "readChild", "()I", &[]),
                "Ljava/lang/NoClassDefFoundError;",
            );
        }
        self.id.set(Some(11));
        assert_eq!(vm.invoke_static(&self.target, "read", "()I", &[])?, partial);
        Ok(Value::Int(7))
    }
}

pub fn cross_owner_initialization_rejects_partial_reads_without_poisoning_owner(
    dex: rd_dex::Dex,
    prefix: &str,
) {
    for suffix in ["$Contended", "$InitializingParent"] {
        let mut vm = Engine::new(vec![dex.clone()], VmConfig::default());
        let target = class(prefix, suffix);
        let host = Rc::new(InitializerHost {
            id: Cell::new(Some(11)),
            target: target.clone(),
            child: (suffix == "$InitializingParent").then(|| class(prefix, "$ContendingChild")),
            calls: Cell::new(0),
            fail: false,
        });
        vm.bind_native(host.clone());
        assert_eq!(
            vm.invoke_static(&target, "read", "()I", &[]).unwrap(),
            Value::Int(42)
        );
        assert_eq!(host.calls.get(), 1);
        host.id.set(Some(22));
        assert_eq!(
            vm.invoke_static(&target, "read", "()I", &[]).unwrap(),
            Value::Int(42)
        );
        assert!(vm.device_class_is_initialized(&target));
        assert_eq!(host.calls.get(), 1);
        assert!(vm.execution_frames.roots().next().is_none());
        assert!(vm.heap.gc_pinned_roots().next().is_none());
    }
}

pub fn initialization_identity_failure_leaves_unstarted_class_retryable(
    dex: rd_dex::Dex,
    prefix: &str,
) {
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let target = class(prefix, "$Recursive");
    let host = Rc::new(InitializerHost {
        id: Cell::new(None),
        target: target.clone(),
        child: None,
        calls: Cell::new(0),
        fail: false,
    });
    vm.bind_native(host.clone());
    for identity in [None, Some(0)] {
        host.id.set(identity);
        check_error(
            vm.invoke_static(&target, "read", "()I", &[]),
            "API_UNAVAILABLE",
        );
        assert!(
            vm.statics.is_empty(),
            "unknown owner must not publish partial defaults"
        );
        assert!(!vm.device_class_is_initialized(&target));
    }
    host.id.set(Some(11));
    assert_eq!(
        vm.invoke_static(&target, "read", "()I", &[]).unwrap(),
        Value::Int(7)
    );
    assert_eq!(
        vm.statics.get(&(target.clone(), "visits".into())),
        Some(&Value::Int(1))
    );
    host.id.set(Some(22));
    assert_eq!(
        vm.invoke_static(&target, "read", "()I", &[]).unwrap(),
        Value::Int(7)
    );
    assert_eq!(host.calls.get(), 0);
}

pub fn initializer_infrastructure_failure_stays_typed_and_cleans_running_owner(
    dex: rd_dex::Dex,
    prefix: &str,
) {
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let target = class(prefix, "$NativeFailure");
    let host = Rc::new(InitializerHost {
        id: Cell::new(Some(11)),
        target: target.clone(),
        child: None,
        calls: Cell::new(0),
        fail: true,
    });
    vm.bind_native(host.clone());
    check_error(
        vm.invoke_static(&target, "read", "()I", &[]),
        "API_UNAVAILABLE",
    );
    for owner in [11, 22] {
        host.id.set(Some(owner));
        check_exception(
            vm.invoke_static(&target, "read", "()I", &[]),
            "Ljava/lang/NoClassDefFoundError;",
        );
    }
    assert_eq!(host.calls.get(), 1, "partial initializer must never rerun");
    assert!(!vm.device_class_is_initialized(&target));
    vm.gc();
    assert!(vm.execution_frames.roots().next().is_none());
    assert!(vm.heap.gc_pinned_roots().next().is_none());
}

pub fn captured_scope_return_never_pins_into_another_owner_or_guest() {
    let mut scopes = OwnerScopes::default();
    let outer_a = scopes.mark(11);
    scopes.host_enter(11, None);
    let native_a = scopes.mark(11);
    scopes.host_enter(11, None);
    scopes.pin(11, 101);
    scopes.pin(11, 102);
    let outer_b = scopes.mark(22);
    scopes.host_enter(22, None);
    scopes.pin(22, 201);
    scopes.restore_return(native_a, Some(101));
    assert_eq!(scopes.roots().collect::<Vec<_>>(), vec![101, 201]);
    scopes.restore(outer_a);
    assert_eq!(scopes.roots().collect::<Vec<_>>(), vec![201]);
    scopes.guest_enter(22);
    let native_b = scopes.mark(22);
    scopes.host_enter(22, None);
    scopes.pin(22, 202);
    scopes.restore_return(native_b, Some(202));
    assert_eq!(scopes.roots().collect::<Vec<_>>(), vec![201]);
    scopes.restore(outer_b);
    assert!(scopes.owners.is_empty());
    assert_eq!(scopes.cutoff(), 0);
}
