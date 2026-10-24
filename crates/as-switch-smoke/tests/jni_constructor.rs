//! Actual portable DEX constructors and GC; native machine ABI has its own
//! cross-compiler CPU oracle. Backend here models only the nested native entry.
use as_switch_smoke::jni_constructor::{class_known, construct};
use rd_vm::{heap::ElemKind, native_host::NativeHost, Engine, ObjRef, Value, VmConfig, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
const PREFIX: &str = "Lorg/androswitch/contracts/";
const SIG: &str = "(IZBCSJFDLjava/lang/Object;[IDDDDDDDDI)V";
#[derive(Default)]
struct Native {
    roots: RefCell<Vec<ObjRef>>,
    nested: Cell<usize>,
}
impl Native {
    fn keep(&self, object: ObjRef) -> Result<ObjRef, VmExit> {
        self.roots.borrow_mut().push(object);
        Ok(object)
    }
    fn release(&self, object: ObjRef) {
        self.roots.borrow_mut().retain(|r| *r != object);
    }
}
impl NativeHost for Native {
    fn current_thread_id(&self) -> Option<u64> {
        Some(1)
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Ok(())
    }
    fn roots(&self) -> Vec<ObjRef> {
        self.roots.borrow().clone()
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(
            (class, name, sig),
            (
                "Lorg/androswitch/contracts/ConstructorCases;",
                "nested",
                "()I"
            )
        );
        self.nested.set(self.nested.get() + 1);
        vm.gc();
        let leaf = construct(
            vm,
            &format!("{PREFIX}Leaf;"),
            "(I)V",
            &[Value::Int(37)],
            |r| self.keep(r),
        )?;
        let value = vm.invoke_instance(leaf, "value", "()I", &[])?;
        self.release(leaf);
        vm.gc();
        Ok(value)
    }
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/jni-constructor-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn vm() -> (Engine, Rc<Native>) {
    assert!(rd_vm::PORTABLE_CORE);
    let bytes = std::fs::read(root().join("dex/classes.dex"))
        .expect("run fixtures/m9/jni-construct/build.ps1");
    let mut vm = Engine::new(
        vec![rd_dex::Dex::parse(bytes).unwrap()],
        VmConfig::default(),
    );
    let native = Rc::new(Native::default());
    vm.bind_native(native.clone());
    (vm, native)
}
fn args(vm: &mut Engine, native: &Native, mode: i32) -> Vec<Value> {
    let object = vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    native.keep(object).unwrap();
    let array = vm.heap.alloc_array_zeroed(ElemKind::Int, 2, "[I").unwrap();
    native.keep(array).unwrap();
    vm.heap
        .array_elems_mut(array)
        .unwrap()
        .clone_from_slice(&[Value::Int(3), Value::Int(5)]);
    let mut args = vec![
        Value::Int(-17),
        Value::Int(1),
        Value::Int(-113),
        Value::Int(65530),
        Value::Int(-23456),
        Value::Long(0x123456789abcdef),
        Value::Float(1.25),
        Value::Double(-3.5),
        Value::Obj(object),
        Value::Obj(array),
    ];
    args.extend((0..8).map(|i| Value::Double(0.5 + f64::from(i))));
    args.push(Value::Int(mode));
    args
}
#[test]
fn real_dex_constructor_arguments_match_independent_jvm_jni_oracle_before_and_after_gc() {
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.txt")).unwrap();
    assert_eq!(oracle.lines().count(), 20);
    for (mode, kind) in [(0, "value"), (2, "reentry")] {
        let (mut vm, native) = vm();
        let args = args(&mut vm, &native, mode);
        let object = construct(&mut vm, &format!("{PREFIX}Payload;"), SIG, &args, |r| {
            native.keep(r)
        })
        .unwrap();
        for _ in 0..2 {
            vm.gc();
            assert_eq!(vm.heap.get_field(object, "identity").unwrap(), args[8]);
            assert_eq!(vm.heap.get_field(object, "array").unwrap(), args[9]);
            let actual = vm
                .invoke_instance(object, "checksum", "()I", &[])
                .unwrap()
                .as_int()
                .unwrap()
                .to_string();
            for form in 0..3 {
                assert!(oracle
                    .lines()
                    .any(|row| row == format!("{kind}\t{form}\t{actual}")));
            }
        }
        assert_eq!(native.nested.get(), usize::from(mode == 2));
        native.release(object);
        vm.gc();
        assert_eq!(vm.heap.class_of(object).unwrap(), rd_vm::gc::FREED_CLASS);
    }
}
#[test]
fn constructor_exception_is_real_and_partial_object_is_collectible_after_local_release() {
    let (mut vm, native) = vm();
    let args = args(&mut vm, &native, 1);
    let allocated = Cell::new(None);
    let result = construct(&mut vm, &format!("{PREFIX}Payload;"), SIG, &args, |r| {
        allocated.set(Some(r));
        native.keep(r)
    });
    assert!(
        matches!(result,Err(VmExit::Exception(ref e)) if e.class=="Ljava/lang/IllegalStateException;" && e.message.as_deref()==Some("constructor failure"))
    );
    let object = allocated.get().unwrap();
    native.release(object);
    vm.gc();
    assert_eq!(vm.heap.class_of(object).unwrap(), rd_vm::gc::FREED_CLASS);
}
#[test]
fn unsupported_or_invalid_constructor_contracts_fail_before_allocation() {
    let (mut vm, native) = vm();
    let retained = Cell::new(0);
    for (class, sig, args, code) in [
        (
            format!("{PREFIX}AbstractPayload;"),
            "()V",
            vec![],
            "Ljava/lang/InstantiationException;",
        ),
        (
            format!("{PREFIX}ConstructorInterface;"),
            "()V",
            vec![],
            "Ljava/lang/InstantiationException;",
        ),
        (
            "[I".into(),
            "()V",
            vec![],
            "Ljava/lang/InstantiationException;",
        ),
        (
            format!("{PREFIX}ConstructorChild;"),
            "(I)V",
            vec![Value::Int(7)],
            "Ljava/lang/NoSuchMethodError;",
        ),
        (
            "Ljava/lang/Thread;".into(),
            "()V",
            vec![],
            "JNI_API_UNAVAILABLE",
        ),
        (
            format!("{PREFIX}Leaf;"),
            "(I)I",
            vec![Value::Int(7)],
            "JNI_SIGNATURE",
        ),
        (format!("{PREFIX}Leaf;"), "(I)V", vec![], "JNI_SIGNATURE"),
    ] {
        let error = construct(&mut vm, &class, sig, &args, |r| {
            retained.set(retained.get() + 1);
            native.keep(r)
        })
        .unwrap_err();
        match error {
            VmExit::Exception(e) => assert_eq!(e.class, code),
            VmExit::Error(e) => assert_eq!(e.code, code),
        }
    }
    assert_eq!(retained.get(), 0);
}
#[test]
fn jni_class_lookup_reuses_real_vm_types_and_validates_array_components() {
    let (vm, _) = vm();
    for name in [
        "Ljava/lang/Object;",
        "Ljava/lang/Thread;",
        "Ljava/util/Timer;",
        "Ljava/lang/Runnable;",
        "[I",
        "[[Ljava/lang/String;",
        "Lorg/androswitch/contracts/Payload;",
    ] {
        assert!(class_known(&vm, name), "{name}");
    }
    for name in [
        "Lmade/up/Missing;",
        "[Lmade/up/Missing;",
        "[V",
        "[",
        "[[",
        "Ljava/lang/Thread",
        "[Ljava/lang/Thread",
    ] {
        assert!(!class_known(&vm, name), "{name}");
    }
    assert!(class_known(&vm, &format!("{}I", "[".repeat(255))));
    assert!(!class_known(&vm, &format!("{}I", "[".repeat(256))));
}

#[test]
fn class_initialization_precedes_allocation_and_failed_initialization_stays_failed() {
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.txt")).unwrap();
    let (mut vm, native) = vm();
    for label in ["value", "again"] {
        let object = construct(&mut vm, &format!("{PREFIX}Initialized;"), "()V", &[], |r| {
            native.keep(r)
        })
        .unwrap();
        let value = vm
            .invoke_instance(object, "value", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap();
        assert!(oracle
            .lines()
            .any(|row| row == format!("init\t{label}\t{value}")));
        native.release(object);
    }
    let retained = Cell::new(0);
    for (label, class) in [
        ("failure", "Ljava/lang/ExceptionInInitializerError;"),
        ("poisoned", "Ljava/lang/NoClassDefFoundError;"),
    ] {
        let error = construct(
            &mut vm,
            &format!("{PREFIX}BadInitializer;"),
            "()V",
            &[],
            |r| {
                retained.set(retained.get() + 1);
                native.keep(r)
            },
        )
        .unwrap_err();
        assert!(matches!(error,VmExit::Exception(ref e) if e.class==class));
        let jvm_name = class
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', ".");
        assert!(oracle
            .lines()
            .any(|row| row == format!("init\t{label}\t{jvm_name}")));
    }
    assert_eq!(
        retained.get(),
        0,
        "class initialization failed before allocation/local retention"
    );
}

#[test]
fn default_framework_catalog_never_admits_a_method_without_a_provider() {
    struct Empty;
    impl rd_vm::framework_host::FrameworkHost for Empty {
        fn attach(&self, _: &mut Engine, _: &str, _: ObjRef) -> Result<bool, VmExit> {
            Ok(false)
        }
        fn call_static(
            &self,
            _: &mut Engine,
            _: &str,
            _: &str,
            _: &str,
            _: &[Value],
        ) -> Result<Option<Value>, VmExit> {
            Ok(None)
        }
        fn call(
            &self,
            _: &mut Engine,
            _: &str,
            _: &str,
            _: &str,
            _: ObjRef,
            _: &[Value],
        ) -> Result<Option<Value>, VmExit> {
            Ok(None)
        }
        fn roots(&self) -> Vec<ObjRef> {
            vec![]
        }
    }
    let (mut vm, _) = vm();
    vm.bind_framework(Rc::new(Empty));
    assert!(
        matches!(as_switch_smoke::jni_constructor::method_owner(&mut vm,"Ljava/lang/Thread;","currentThread","()Ljava/lang/Thread;",true),Err(VmExit::Exception(e)) if e.class=="Ljava/lang/NoSuchMethodError;")
    );
}
