//! Factories share the product's actual collection state, interfaces and GC.
#![cfg(feature = "host")]
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{framework_host::FrameworkHost, ObjRef, Value, VmExit};
use std::{path::PathBuf, rc::Rc};

struct Test {
    app: App,
    root: PathBuf,
}
impl Test {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "as-collection-factories-{}-{}",
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
            .expect("run fixtures/conformance/build.ps1"),
        )
        .unwrap();
        let app = App::new(apk, fs, Rc::new(HostPlatform::default()), 640, 360, None).unwrap();
        Self { app, root }
    }
    fn factory(&mut self, name: &str, sig: &str, args: &[Value]) -> ObjRef {
        self.app
            .vm
            .invoke_static("Ljava/util/Collections;", name, sig, args)
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap()
    }
    fn call(&mut self, r: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
        self.app.vm.invoke_instance(r, name, sig, args).unwrap()
    }
    fn collection(&mut self, class: &str) -> ObjRef {
        let r = self
            .app
            .vm
            .heap
            .alloc_instance(class.into(), vec![])
            .unwrap();
        self.app.vm.attach_host_state(class, r).unwrap();
        self.call(r, "<init>", "()V", &[]);
        r
    }
    fn keep(&mut self, name: &str, r: ObjRef) {
        self.app
            .vm
            .statics
            .insert(("Ltests/Factories;".into(), name.into()), Value::Obj(r));
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn immutable(result: Result<Value, VmExit>) {
    assert!(
        matches!(result,Err(VmExit::Exception(ref e)) if e.class=="Ljava/lang/UnsupportedOperationException;"),
        "{result:?}"
    );
}
#[test]
fn empty_and_singleton_factories_use_existing_product_collection_methods() {
    let mut t = Test::new();
    for (name, ty) in [
        ("emptyList", "List"),
        ("emptySet", "Set"),
        ("emptyMap", "Map"),
    ] {
        let r = t.factory(name, &format!("()Ljava/util/{ty};"), &[]);
        assert_eq!(t.call(r, "size", "()I", &[]), Value::Int(0));
        assert_eq!(t.call(r, "isEmpty", "()Z", &[]), Value::Int(1));
        let result = if ty == "Map" {
            t.app.vm.invoke_instance(
                r,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[Value::Null, Value::Null],
            )
        } else {
            t.app
                .vm
                .invoke_instance(r, "add", "(Ljava/lang/Object;)Z", &[Value::Null])
        };
        immutable(result);
    }
    let text = Value::Obj(t.app.vm.heap.alloc_string("shared-value".into()).unwrap());
    let list = t.factory(
        "singletonList",
        "(Ljava/lang/Object;)Ljava/util/List;",
        std::slice::from_ref(&text),
    );
    assert_eq!(
        t.call(list, "get", "(I)Ljava/lang/Object;", &[Value::Int(0)]),
        text
    );
    immutable(t.app.vm.invoke_instance(
        list,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        &[Value::Int(0), Value::Null],
    ));
    let set = t.factory(
        "singleton",
        "(Ljava/lang/Object;)Ljava/util/Set;",
        &[Value::Int(0)],
    );
    assert_eq!(
        t.call(set, "contains", "(Ljava/lang/Object;)Z", &[Value::Null]),
        Value::Int(1)
    );
    immutable(
        t.app
            .vm
            .invoke_instance(set, "add", "(Ljava/lang/Object;)Z", &[text]),
    );
}
#[test]
fn wrappers_read_constructor_created_store_and_preserve_inherited_snapshot_scope() {
    let mut t = Test::new();
    let text = Value::Obj(t.app.vm.heap.alloc_string("original".into()).unwrap());
    for (source_class, name, ty) in [
        ("ArrayList", "unmodifiableList", "List"),
        ("ArrayList", "unmodifiableCollection", "Collection"),
        ("HashSet", "unmodifiableSet", "Set"),
    ] {
        let source = t.collection(&format!("Ljava/util/{source_class};"));
        t.call(
            source,
            "add",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(&text),
        );
        let view = t.factory(
            name,
            &format!("(Ljava/util/{ty};)Ljava/util/{ty};"),
            &[Value::Obj(source)],
        );
        assert_eq!(t.call(view, "size", "()I", &[]), Value::Int(1));
        t.call(source, "clear", "()V", &[]);
        assert_eq!(
            t.call(
                view,
                "contains",
                "(Ljava/lang/Object;)Z",
                std::slice::from_ref(&text)
            ),
            Value::Int(1)
        );
        immutable(t.app.vm.invoke_instance(view, "clear", "()V", &[]));
    }
    let source = t.collection("Ljava/util/HashMap;");
    t.call(
        source,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[text.clone(), Value::Null],
    );
    let view = t.factory(
        "unmodifiableMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        &[Value::Obj(source)],
    );
    assert_eq!(
        t.call(
            view,
            "containsKey",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(&text)
        ),
        Value::Int(1)
    );
    immutable(t.app.vm.invoke_instance(
        view,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[text, Value::Null],
    ));
}
#[test]
fn iterator_keeps_factory_owner_and_child_alive_across_real_collection() {
    let mut t = Test::new();
    let child = t
        .app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    let list = t.factory(
        "singletonList",
        "(Ljava/lang/Object;)Ljava/util/List;",
        &[Value::Obj(child)],
    );
    let iterator = t
        .call(list, "iterator", "()Ljava/util/Iterator;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.keep("iterator", iterator);
    let garbage = t
        .app
        .vm
        .heap
        .alloc_instance("Ltests/Garbage;".into(), vec![])
        .unwrap();
    t.app.vm.gc();
    assert_eq!(
        t.app.vm.heap.class_of(garbage).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(t.call(iterator, "hasNext", "()Z", &[]), Value::Int(1));
    assert_eq!(
        t.call(iterator, "next", "()Ljava/lang/Object;", &[]),
        Value::Obj(child)
    );
    assert_eq!(t.call(iterator, "hasNext", "()Z", &[]), Value::Int(0));
    assert!(
        matches!(t.app.vm.invoke_instance(iterator,"next","()Ljava/lang/Object;",&[]),Err(VmExit::Exception(ref e)) if e.class=="Ljava/util/NoSuchElementException;")
    );
}
#[test]
fn field_identity_exact_metadata_and_null_wrapper_errors() {
    let mut t = Test::new();
    let android = t.app.android.clone();
    for (name, ty) in [
        ("EMPTY_LIST", "List"),
        ("EMPTY_SET", "Set"),
        ("EMPTY_MAP", "Map"),
    ] {
        let sig = format!("Ljava/util/{ty};");
        assert!(android.static_field_metadata("Ljava/util/Collections;", name, &sig));
        assert!(!android.static_field_metadata(
            "Ljava/util/Collections;",
            name,
            "Ljava/lang/Object;"
        ));
        let first = android
            .static_field(&mut t.app.vm, "Ljava/util/Collections;", name, &sig)
            .unwrap()
            .unwrap();
        t.app.vm.gc();
        let second = android
            .static_field(&mut t.app.vm, "Ljava/util/Collections;", name, &sig)
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            t.call(first.as_ref().unwrap().unwrap(), "size", "()I", &[]),
            Value::Int(0)
        );
    }
    assert!(android
        .method_metadata("Ljava/util/Collections;", "emptyList", "()Ljava/util/List;")
        .is_some());
    assert!(android
        .method_metadata(
            "Ljava/util/Collections;",
            "emptyList",
            "(I)Ljava/util/List;"
        )
        .is_none());
    assert!(android
        .method_metadata(
            "Ljava/util/Collections;",
            "emptySortedSet",
            "()Ljava/util/SortedSet;"
        )
        .is_none());
    assert!(android
        .method_metadata(
            "Ljava/util/Collections;",
            "emptySortedMap",
            "()Ljava/util/SortedMap;"
        )
        .is_none());
    assert!(android
        .method_metadata(
            "Ljava/util/Collections;",
            "synchronizedList",
            "(Ljava/util/List;)Ljava/util/List;"
        )
        .is_none());
    assert!(
        matches!(t.app.vm.invoke_static("Ljava/util/Collections;","unmodifiableList","(Ljava/util/List;)Ljava/util/List;",&[Value::Null]),Err(VmExit::Exception(ref e)) if e.class=="Ljava/lang/NullPointerException;")
    );
}
