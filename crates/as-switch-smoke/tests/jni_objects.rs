//! Actual production JNI reference-table and safe invocation dispatch contracts.
//! C pointer/varargs transport and Horizon execution are separate evidence.
use as_framework::App;
use as_native::jni_refs::References;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use as_switch_smoke::{jni_constructor::method_owner, jni_objects as objects};
use rd_jni::Handle;
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
const CASES: &str = "Lorg/androswitch/references/ClassReferenceCases;";
const CLASS: &str = "Ljava/lang/Class;";
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Host {
    refs: Rc<RefCell<References>>,
    thread: Cell<u64>,
}
impl NativeHost for Host {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.thread.get())
    }
    fn roots(&self) -> Vec<ObjRef> {
        self.refs.borrow().roots()
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_NATIVE_LOAD",
            "no machine code in reference fixture",
        )
        .into())
    }
    fn invoke(
        &self,
        _: &mut Engine,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_NATIVE_CALL",
            "no machine code in reference fixture",
        )
        .into())
    }
}
struct Harness {
    app: App,
    refs: Rc<RefCell<References>>,
    host: Rc<Host>,
}
impl Harness {
    #[allow(clippy::assertions_on_constants)]
    fn new(fixture: &str) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-jni-objects-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let fs = Rc::new(HostFileSystem::new(root).unwrap());
        let manager = as_package::PackageManager::new(fs.clone());
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/jni-objects-fixture");
        let installed = manager
            .install(
                std::fs::read(dir.join(format!("{fixture}.apk")))
                    .expect("run fixtures/m9/jni-objects/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let mut app = App::new(
            manager.load_apk(&installed.package, None).unwrap().1,
            fs,
            Rc::new(HostPlatform::default()),
            640,
            360,
            None,
        )
        .unwrap();
        let refs = Rc::new(RefCell::new(References::default()));
        refs.borrow_mut().attach(10);
        refs.borrow_mut().attach(20);
        let host = Rc::new(Host {
            refs: refs.clone(),
            thread: Cell::new(10),
        });
        app.vm.bind_native(host.clone());
        app.android.bind_main_thread(&app.vm).unwrap();
        Self { app, refs, host }
    }
    fn op(&mut self, op: u32, a: u64, b: u64) -> Result<u64, VmExit> {
        objects::dispatch(
            &mut self.app.vm,
            &self.refs,
            self.host.thread.get(),
            op,
            a,
            b,
            None,
        )
    }
    fn find(&mut self, name: &str) -> u64 {
        objects::dispatch(
            &mut self.app.vm,
            &self.refs,
            self.host.thread.get(),
            1,
            0,
            0,
            Some(name),
        )
        .unwrap()
    }
    fn descriptor(&mut self, token: u64) -> String {
        objects::class_descriptor(&mut self.app.vm, &self.refs, self.host.thread.get(), token)
            .unwrap()
    }
    fn object(&self, token: u64) -> ObjRef {
        objects::object(&self.refs, self.host.thread.get(), token).unwrap()
    }
    fn method(&mut self, class: u64, name: &str, sig: &str, is_static: bool) -> u64 {
        let class = self.descriptor(class);
        let declaring = method_owner(&mut self.app.vm, &class, name, sig, is_static).unwrap();
        self.app.vm.ensure_initialized(&class).unwrap();
        self.refs
            .borrow_mut()
            .id(
                self.host.thread.get(),
                Handle::Method {
                    class: declaring,
                    name: name.into(),
                    sig: sig.into(),
                    is_static,
                },
            )
            .unwrap()
    }
    fn call(
        &mut self,
        receiver: u64,
        method: u64,
        is_static: bool,
        args: &[u64],
    ) -> Result<u64, VmExit> {
        let call = objects::Invocation::decode(
            &self.refs,
            self.host.thread.get(),
            receiver,
            method,
            is_static,
            args,
        )?;
        call.invoke(&mut self.app.vm, &self.refs, self.host.thread.get())
    }
    fn name(&mut self, class: u64) -> String {
        // getName is an existing Java intrinsic, outside the bounded JNI
        // method catalog. This label check must not invent a JNI method ID.
        let class = self.object(class);
        let text = self
            .app
            .vm
            .invoke_instance(class, "getName", "()Ljava/lang/String;", &[])
            .unwrap();
        self.app
            .vm
            .heap
            .as_str(text.as_ref().unwrap().unwrap())
            .unwrap()
            .into()
    }
}
#[test]
fn production_class_receiver_uses_canonical_object_and_preserves_declaring_descriptor() {
    for fixture in ["objects", "renamed"] {
        let mut h = Harness::new(fixture);
        let root = h.find("org/androswitch/references/ClassReferenceCases");
        let object = h.object(root);
        assert_eq!(h.app.vm.heap.class_of(object).unwrap(), CLASS);
        assert!(h.refs.borrow().roots().contains(&object));
        assert_eq!(h.descriptor(root), CASES);
        let static_receiver = objects::local_class(&mut h.app.vm, &h.refs, 10, CASES).unwrap();
        assert_eq!(h.op(17, root, static_receiver).unwrap(), 1);
        let class_class = h.find("java/lang/Class");
        let method = h.method(
            class_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            false,
        );
        assert!(
            matches!(h.refs.borrow().get(10,method).unwrap(),Handle::Method{class,is_static:false,..} if class==CLASS)
        );
        let loader = h.call(root, method, false, &[]).unwrap();
        let thread = h
            .app
            .vm
            .invoke_static(
                "Ljava/lang/Thread;",
                "currentThread",
                "()Ljava/lang/Thread;",
                &[],
            )
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        let context = h
            .app
            .vm
            .invoke_instance(
                thread,
                "getContextClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .unwrap();
        assert_eq!(context, Value::Obj(h.object(loader)));
        let actual_type = h.op(5, root, 0).unwrap();
        assert_eq!(h.descriptor(actual_type), CLASS);
        assert_eq!(h.op(17, class_class, actual_type).unwrap(), 1);
        assert_eq!(h.op(17, root, class_class).unwrap(), 0);
        let object_class = h.find("java/lang/Object");
        assert_eq!(h.op(33, root, class_class).unwrap(), 1);
        assert_eq!(h.op(33, root, object_class).unwrap(), 1);
        assert_eq!(
            h.op(33, root, root).unwrap(),
            0,
            "a Class object is not an instance of the class it represents"
        );
    }
}
#[test]
fn production_arguments_returns_gc_primitive_and_array_classes_match_actual_jvm() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/jni-objects-fixture/jvm-oracle.txt"),
    )
    .unwrap();
    for fixture in ["objects", "renamed"] {
        let mut h = Harness::new(fixture);
        let root = h.find("org/androswitch/references/ClassReferenceCases");
        let mut rows = Vec::new();
        for (name, sig, label) in [
            ("echo", "(Ljava/lang/Class;)Ljava/lang/Class;", "echo"),
            (
                "echoObject",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                "object",
            ),
        ] {
            let method = h.method(root, name, sig, true);
            let result = h.call(root, method, true, &[root]).unwrap();
            rows.push(format!("{label}\t{}", h.op(17, root, result).unwrap()));
        }
        let own = h.method(root, "own", "()Ljava/lang/Class;", true);
        let returned = h.call(root, own, true, &[]).unwrap();
        assert_eq!(h.descriptor(returned), CASES);
        rows.push(format!("own\t{}", h.name(returned)));
        let class = h.op(5, returned, 0).unwrap();
        rows.push(format!("class\t{}", h.name(class)));
        for (name, expected) in [("primitive", "I"), ("array", "[[Ljava/lang/String;")] {
            let method = h.method(root, name, "()Ljava/lang/Class;", true);
            let class = h.call(root, method, true, &[]).unwrap();
            assert_eq!(
                h.descriptor(class),
                expected,
                "genuine Java primitive/array Class is a valid jclass"
            );
            rows.push(format!("{name}\t{}", h.name(class)));
            if name == "array" {
                let found = h.find("[[Ljava/lang/String;");
                assert_eq!(h.op(17, class, found).unwrap(), 1);
            }
        }
        let object = h.find("java/lang/Object");
        rows.push(format!("distinct\t{}", 1 - h.op(17, root, object).unwrap()));
        let failure = h.method(root, "fail", "(Ljava/lang/Class;)V", true);
        let error = h.call(root, failure, true, &[root]).unwrap_err();
        let VmExit::Exception(error) = error else {
            panic!("expected real DEX Throwable")
        };
        assert_eq!(error.class, "Ljava/lang/IllegalStateException;");
        // VmExit appends diagnostic call frames; compare the real Throwable
        // payload separately from that VM-only presentation suffix.
        let message = error.message.unwrap();
        rows.push(format!("exception\t{}", message.lines().next().unwrap()));
        assert_eq!(rows, oracle.lines().map(str::to_string).collect::<Vec<_>>());
        assert!(
            matches!(objects::dispatch(&mut h.app.vm,&h.refs,10,1,0,0,Some("I")),Err(VmExit::Exception(e)) if e.class=="Ljava/lang/NoClassDefFoundError;")
        );
    }
}
#[test]
fn production_reference_frames_global_ownership_and_gc_keep_real_objects() {
    let mut h = Harness::new("objects");
    let class = h.find("org/androswitch/references/ClassReferenceCases");
    assert_eq!(h.op(31, 0, 0).unwrap(), 0);
    assert_eq!(h.op(31, class, 0).unwrap(), 1);
    let global = h.op(14, class, 0).unwrap();
    assert_eq!(h.op(31, global, 0).unwrap(), 2);
    assert!(h.op(15, global, 0).is_err());
    assert!(h.op(16, class, 0).is_err());
    h.op(18, 0, 0).unwrap();
    let inner = h.op(13, global, 0).unwrap();
    let escaped = h.op(19, inner, 0).unwrap();
    assert!(h.op(17, inner, class).is_err());
    assert_eq!(h.op(17, escaped, class).unwrap(), 1);
    let ordinary = h
        .app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    let local = h
        .refs
        .borrow_mut()
        .local(10, Handle::Object(ordinary))
        .unwrap();
    let kept = h.op(14, local, 0).unwrap();
    h.app.vm.gc();
    assert_eq!(
        h.app.vm.heap.class_of(ordinary).unwrap(),
        "Ljava/lang/Object;"
    );
    h.host.thread.set(20);
    assert!(matches!(h.op(17,class,global),Err(VmExit::Error(e)) if e.code=="JNI_WRONG_THREAD"));
    assert_eq!(h.descriptor(global), CASES);
    h.refs.borrow_mut().detach(10);
    assert!(h.refs.borrow().get(10, class).is_err());
    assert_eq!(h.descriptor(global), CASES);
    h.app.vm.gc();
    assert_eq!(
        h.app.vm.heap.class_of(ordinary).unwrap(),
        "Ljava/lang/Object;"
    );
    h.op(16, kept, 0).unwrap();
    h.app.vm.gc();
    assert_eq!(
        h.app.vm.heap.class_of(ordinary).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    h.op(16, global, 0).unwrap();
    assert!(h.op(31, global, 0).is_err());
    // Canonical Class metadata remains interned/rooted by the VM itself.
    let object = h.app.vm.device_class_object(CASES).unwrap();
    assert_eq!(h.app.vm.heap.class_of(object).unwrap(), CLASS);
}
#[test]
fn member_ids_deleted_and_wrong_class_objects_never_become_jobjects() {
    let mut h = Harness::new("objects");
    let root = h.find("org/androswitch/references/ClassReferenceCases");
    let method = h.method(root, "own", "()Ljava/lang/Class;", true);
    let field = h
        .refs
        .borrow_mut()
        .id(
            10,
            Handle::Field {
                class: CASES.into(),
                name: "not_an_object".into(),
                sig: "I".into(),
                is_static: true,
            },
        )
        .unwrap();
    for id in [method, field] {
        assert!(objects::object(&h.refs, 10, id).is_err());
        assert!(objects::class_descriptor(&mut h.app.vm, &h.refs, 10, id).is_err());
        assert!(objects::from_bits(&h.refs, 10, id, "Ljava/lang/Object;").is_err());
        for op in [5, 13, 14, 15, 16] {
            assert!(
                h.op(op, id, 0).is_err(),
                "member ID accepted by object op {op}"
            );
        }
        assert_eq!(
            h.op(31, id, 0).unwrap(),
            0,
            "opaque member IDs have JNIInvalidRefType"
        );
        assert!(h.op(17, 0, id).is_err());
        h.op(18, 0, 0).unwrap();
        let live = h.op(13, root, 0).unwrap();
        assert!(h.op(19, id, 0).is_err());
        assert_eq!(h.op(17, live, root).unwrap(), 1);
        h.op(19, 0, 0).unwrap();
    }
    assert!(objects::Invocation::decode(&h.refs, 10, root, method, false, &[]).is_err());
    assert!(objects::Invocation::decode(&h.refs, 10, root, field, true, &[]).is_err());
    let ordinary = h
        .app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    let object = h
        .refs
        .borrow_mut()
        .local(10, Handle::Object(ordinary))
        .unwrap();
    assert!(objects::class_descriptor(&mut h.app.vm, &h.refs, 10, object).is_err());
    let descriptor = h.app.vm.heap.alloc_string(CASES.into()).unwrap();
    let forged = h
        .app
        .vm
        .heap
        .alloc_instance(
            CLASS.into(),
            vec![("class_name".into(), Value::Obj(descriptor))],
        )
        .unwrap();
    let forged = h
        .refs
        .borrow_mut()
        .local(10, Handle::Object(forged))
        .unwrap();
    assert!(
        matches!(objects::class_descriptor(&mut h.app.vm,&h.refs,10,forged),Err(VmExit::Error(e)) if e.code=="JNI_REFERENCE_KIND")
    );
    let deleted = h.op(13, root, 0).unwrap();
    h.op(15, deleted, 0).unwrap();
    let echo = h.method(root, "echo", "(Ljava/lang/Class;)Ljava/lang/Class;", true);
    assert!(
        matches!(objects::Invocation::decode(&h.refs,10,root,echo,true,&[deleted]),Err(VmExit::Error(e)) if e.code=="JNI_INVALID_REFERENCE")
    );
    assert!(objects::class_descriptor(&mut h.app.vm, &h.refs, 10, deleted).is_err());
    assert!(h.op(17, deleted, root).is_err());
    assert_eq!(h.op(17, 0, 0).unwrap(), 1);
}

#[test]
fn production_virtual_object_method_id_honors_host_overrides_and_rejects_unknown_builtin_equals() {
    let mut h = Harness::new("objects");
    let object_class = h.find("java/lang/Object");
    let equals = h.method(object_class, "equals", "(Ljava/lang/Object;)Z", false);
    let local = |object: ObjRef| {
        h.refs
            .borrow_mut()
            .local(10, Handle::Object(object))
            .unwrap()
    };
    let a = h.app.vm.heap.alloc_string("same value".into()).unwrap();
    let b = h.app.vm.heap.alloc_string("same value".into()).unwrap();
    assert_ne!(a, b);
    let a = local(a);
    let b = local(b);
    assert_eq!(h.call(a, equals, false, &[b]).unwrap(), 1);
    assert_eq!(h.call(a, equals, false, &[0]).unwrap(), 0);

    let a = h
        .app
        .vm
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(1000)],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let b = h
        .app
        .vm
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(1000)],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_ne!(a, b);
    let a = h.refs.borrow_mut().local(10, Handle::Object(a)).unwrap();
    let b = h.refs.borrow_mut().local(10, Handle::Object(b)).unwrap();
    assert_eq!(h.call(a, equals, false, &[b]).unwrap(), 1);

    for class in ["Ljava/util/ArrayList;", "Ljava/util/HashMap;"] {
        let a = h.app.vm.new_instance(class).unwrap();
        let b = h.app.vm.new_instance(class).unwrap();
        let a = h.refs.borrow_mut().local(10, Handle::Object(a)).unwrap();
        let b = h.refs.borrow_mut().local(10, Handle::Object(b)).unwrap();
        for other in [a, b] {
            let result = h.call(a, equals, false, &[other]);
            assert!(
                matches!(result, Err(VmExit::Error(ref e)) if e.code=="API_UNAVAILABLE" && e.cause.contains("Object.equals")),
                "{class}: {result:?}"
            );
        }
    }
    // This contract exercises virtual invocation, not the separate constructor
    // path. Object has no instance fields requiring initialization.
    let plain = h
        .app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    let plain = h
        .refs
        .borrow_mut()
        .local(10, Handle::Object(plain))
        .unwrap();
    assert_eq!(h.call(plain, equals, false, &[plain]).unwrap(), 1);
    assert_eq!(h.call(plain, equals, false, &[0]).unwrap(), 0);
    let a = h
        .app
        .vm
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, 1, "[I")
        .unwrap();
    let b = h
        .app
        .vm
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, 1, "[I")
        .unwrap();
    let a = h.refs.borrow_mut().local(10, Handle::Object(a)).unwrap();
    let b = h.refs.borrow_mut().local(10, Handle::Object(b)).unwrap();
    assert_eq!(h.call(a, equals, false, &[a]).unwrap(), 1);
    assert_eq!(h.call(a, equals, false, &[b]).unwrap(), 0);
    assert_eq!(h.call(a, equals, false, &[0]).unwrap(), 0);
}
