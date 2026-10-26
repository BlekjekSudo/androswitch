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

fn values(t: &mut Test, r: ObjRef) -> Vec<Value> {
    let array = t
        .call(r, "toArray", "()[Ljava/lang/Object;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.app.vm.heap.as_array_elems(array).unwrap().0.to_vec()
}
fn add(t: &mut Test, r: ObjRef, values: &[Value]) {
    for value in values {
        t.call(
            r,
            "add",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(value),
        );
    }
}
fn copied(t: &mut Test, class: &str, source: ObjRef) -> ObjRef {
    let r = t.app.vm.heap.alloc_instance(class.into(), vec![]).unwrap();
    t.app.vm.attach_host_state(class, r).unwrap();
    t.call(
        r,
        "<init>",
        "(Ljava/util/Collection;)V",
        &[Value::Obj(source)],
    );
    r
}
#[test]
fn inherited_copy_constructors_and_bulk_operations_share_the_product_store() {
    let mut t = Test::new();
    let source = t.collection("Ljava/util/ArrayList;");
    let a = Value::Obj(t.app.vm.heap.alloc_string("equal".into()).unwrap());
    let b = Value::Obj(t.app.vm.heap.alloc_string("equal".into()).unwrap());
    add(&mut t, source, &[a.clone(), b.clone(), Value::Null]);
    for class in [
        "Ljava/util/ArrayList;",
        "Ljava/util/LinkedList;",
        "Ljava/util/Vector;",
        "Ljava/util/HashSet;",
        "Ljava/util/LinkedHashSet;",
    ] {
        let destination = copied(&mut t, class, source);
        let expected = if class.contains("Set") {
            vec![a.clone(), Value::Null]
        } else {
            vec![a.clone(), b.clone(), Value::Null]
        };
        assert_eq!(values(&mut t, destination), expected, "{class}");
        assert_eq!(
            t.call(
                destination,
                "containsAll",
                "(Ljava/util/Collection;)Z",
                &[Value::Obj(source)]
            ),
            Value::Int(1)
        );
        assert_eq!(
            t.call(
                destination,
                "removeAll",
                "(Ljava/util/Collection;)Z",
                &[Value::Obj(source)]
            ),
            Value::Int(1)
        );
        assert_eq!(t.call(destination, "size", "()I", &[]), Value::Int(0));
        assert_eq!(
            t.call(
                destination,
                "addAll",
                "(Ljava/util/Collection;)Z",
                &[Value::Obj(source)]
            ),
            Value::Int(1)
        );
        assert_eq!(values(&mut t, destination), expected);
        assert_eq!(
            t.call(
                destination,
                "retainAll",
                "(Ljava/util/Collection;)Z",
                &[Value::Obj(source)]
            ),
            Value::Int(0)
        );
    }
    assert_eq!(values(&mut t, source), vec![a, b, Value::Null]);
}
#[test]
fn inherited_sublist_and_nested_sublist_write_through_and_retain_parent_during_gc() {
    let mut t = Test::new();
    let list = t.collection("Ljava/util/ArrayList;");
    add(
        &mut t,
        list,
        &[Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
    );
    let view = t
        .call(
            list,
            "subList",
            "(II)Ljava/util/List;",
            &[Value::Int(1), Value::Int(4)],
        )
        .as_ref()
        .unwrap()
        .unwrap();
    let nested = t
        .call(
            view,
            "subList",
            "(II)Ljava/util/List;",
            &[Value::Int(1), Value::Int(2)],
        )
        .as_ref()
        .unwrap()
        .unwrap();
    t.keep("nested", nested);
    t.app.vm.gc();
    assert_eq!(values(&mut t, nested), vec![Value::Int(3)]);
    t.call(nested, "clear", "()V", &[]);
    assert_eq!(
        values(&mut t, list),
        vec![Value::Int(1), Value::Int(2), Value::Int(4)]
    );
    assert_eq!(values(&mut t, view), vec![Value::Int(2), Value::Int(4)]);
    t.call(
        list,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        &[Value::Int(1), Value::Int(9)],
    );
    assert_eq!(values(&mut t, view), vec![Value::Int(9), Value::Int(4)]);
}
#[test]
fn inherited_list_iterator_navigation_removal_exhaustion_and_gc_use_one_owner() {
    let mut t = Test::new();
    let list = t.collection("Ljava/util/ArrayList;");
    add(&mut t, list, &[Value::Int(1), Value::Int(2), Value::Int(3)]);
    let iterator = t
        .call(
            list,
            "listIterator",
            "(I)Ljava/util/ListIterator;",
            &[Value::Int(1)],
        )
        .as_ref()
        .unwrap()
        .unwrap();
    t.keep("iterator", iterator);
    t.app.vm.gc();
    assert_eq!(
        t.call(iterator, "previous", "()Ljava/lang/Object;", &[]),
        Value::Int(1)
    );
    t.call(iterator, "remove", "()V", &[]);
    assert_eq!(values(&mut t, list), vec![Value::Int(2), Value::Int(3)]);
    assert_eq!(t.call(iterator, "nextIndex", "()I", &[]), Value::Int(0));
    assert_eq!(
        t.call(iterator, "next", "()Ljava/lang/Object;", &[]),
        Value::Int(2)
    );
    assert_eq!(
        t.call(iterator, "next", "()Ljava/lang/Object;", &[]),
        Value::Int(3)
    );
    assert_eq!(t.call(iterator, "hasNext", "()Z", &[]), Value::Int(0));
    assert!(
        matches!(t.app.vm.invoke_instance(iterator,"next","()Ljava/lang/Object;",&[]), Err(VmExit::Exception(e)) if e.class=="Ljava/util/NoSuchElementException;")
    );
    immutable(
        t.app
            .vm
            .invoke_instance(iterator, "add", "(Ljava/lang/Object;)V", &[Value::Null]),
    );
    immutable(
        t.app
            .vm
            .invoke_instance(iterator, "set", "(Ljava/lang/Object;)V", &[Value::Null]),
    );
}
#[test]
fn inherited_typed_array_reuses_the_destination_and_null_terminates() {
    let mut t = Test::new();
    let list = t.collection("Ljava/util/ArrayList;");
    let text = Value::Obj(t.app.vm.heap.alloc_string("element".into()).unwrap());
    add(&mut t, list, std::slice::from_ref(&text));
    let array = t
        .app
        .vm
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Obj, 3, "[Ljava/lang/String;")
        .unwrap();
    t.app
        .vm
        .heap
        .array_elems_mut(array)
        .unwrap()
        .fill(text.clone());
    assert_eq!(
        t.call(
            list,
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            &[Value::Obj(array)]
        ),
        Value::Obj(array)
    );
    assert_eq!(
        t.app.vm.heap.as_array_elems(array).unwrap().0,
        &[text.clone(), Value::Null, text]
    );
}
#[test]
fn shared_bulk_mutations_do_not_bypass_readonly_factories() {
    let mut t = Test::new();
    let source = t.collection("Ljava/util/ArrayList;");
    add(&mut t, source, &[Value::Null]);
    for (name, ty) in [("emptyList", "List"), ("emptySet", "Set")] {
        let value = t.factory(name, &format!("()Ljava/util/{ty};"), &[]);
        for operation in ["addAll", "removeAll", "retainAll"] {
            immutable(t.app.vm.invoke_instance(
                value,
                operation,
                "(Ljava/util/Collection;)Z",
                &[Value::Obj(source)],
            ));
        }
        assert_eq!(values(&mut t, value), Vec::<Value>::new());
    }
}
#[test]
fn shared_collections_utilities_sort_reverse_minmax_and_preserve_readonly_maps() {
    let mut t = Test::new();
    let list = t.collection("Ljava/util/ArrayList;");
    let b = Value::Obj(t.app.vm.heap.alloc_string("b".into()).unwrap());
    let a = Value::Obj(t.app.vm.heap.alloc_string("a".into()).unwrap());
    add(&mut t, list, &[b.clone(), a.clone()]);
    let class = "Ljava/util/Collections;";
    assert_eq!(
        t.app
            .vm
            .invoke_static(
                class,
                "max",
                "(Ljava/util/Collection;)Ljava/lang/Object;",
                &[Value::Obj(list)]
            )
            .unwrap(),
        b
    );
    assert_eq!(
        t.app
            .vm
            .invoke_static(
                class,
                "min",
                "(Ljava/util/Collection;)Ljava/lang/Object;",
                &[Value::Obj(list)]
            )
            .unwrap(),
        a
    );
    t.app
        .vm
        .invoke_static(class, "sort", "(Ljava/util/List;)V", &[Value::Obj(list)])
        .unwrap();
    assert_eq!(values(&mut t, list), vec![a.clone(), b.clone()]);
    t.app
        .vm
        .invoke_static(class, "reverse", "(Ljava/util/List;)V", &[Value::Obj(list)])
        .unwrap();
    assert_eq!(values(&mut t, list), vec![b.clone(), a.clone()]);
    let map = t.factory(
        "singletonMap",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;",
        &[a.clone(), b.clone()],
    );
    assert_eq!(
        t.call(map, "get", "(Ljava/lang/Object;)Ljava/lang/Object;", &[a]),
        b
    );
    immutable(t.app.vm.invoke_instance(map, "clear", "()V", &[]));
    let iterator = t.factory("emptyIterator", "()Ljava/util/Iterator;", &[]);
    assert_eq!(t.call(iterator, "hasNext", "()Z", &[]), Value::Int(0));
    assert!(
        matches!(t.app.vm.invoke_instance(iterator,"next","()Ljava/lang/Object;",&[]),Err(VmExit::Exception(e)) if e.class=="Ljava/util/NoSuchElementException;")
    );
    let set = t.factory("emptySet", "()Ljava/util/Set;", &[]);
    immutable(t.app.vm.invoke_static(
        class,
        "addAll",
        "(Ljava/util/Collection;[Ljava/lang/Object;)Z",
        &[Value::Obj(set), Value::Null],
    ));
}

#[test]
fn cyclic_product_values_throw_java_errors_and_release_the_guard_after_unwind() {
    let mut t = Test::new();
    let a = t.collection("Ljava/util/ArrayList;");
    let b = t.collection("Ljava/util/ArrayList;");
    add(&mut t, a, &[Value::Obj(b)]);
    add(&mut t, b, &[Value::Obj(a)]);
    for (method, sig, args) in [
        ("toString", "()Ljava/lang/String;", vec![]),
        ("hashCode", "()I", vec![]),
        ("equals", "(Ljava/lang/Object;)Z", vec![Value::Obj(b)]),
    ] {
        assert!(
            matches!(t.app.vm.invoke_instance(a,method,sig,&args), Err(VmExit::Exception(e)) if e.class=="Ljava/lang/StackOverflowError;")
        );
    }
    t.call(a, "clear", "()V", &[]);
    t.call(b, "clear", "()V", &[]);
    let result = t
        .call(a, "toString", "()Ljava/lang/String;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(t.app.vm.heap.as_str(result).unwrap(), "[]");
    assert_eq!(
        t.call(a, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(b)]),
        Value::Int(1)
    );
    assert_eq!(t.call(a, "hashCode", "()I", &[]), Value::Int(1));
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
