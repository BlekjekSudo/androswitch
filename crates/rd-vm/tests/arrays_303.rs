//! Regression tests for issue #303 (`arrays.rs`, `jcollections.rs`, `intrinsics.rs`).

use rd_vm::err::VmExit;
use rd_vm::heap::{ElemKind, ObjRef};
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const ARRAYS: &str = "Ljava/util/Arrays;";
const ARRAY_LIST: &str = "Ljava/util/ArrayList;";
const HASH_SET: &str = "Ljava/util/HashSet;";
const HASH_MAP: &str = "Ljava/util/HashMap;";
const VECTOR: &str = "Ljava/util/Vector;";
const ARRAY_MAP: &str = "Landroid/util/ArrayMap;";
const URI: &str = "Landroid/net/Uri;";
const BIG_DECIMAL: &str = "Ljava/math/BigDecimal;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn jstr(e: &mut Engine, txt: &str) -> Value {
    Value::Obj(e.heap.alloc_string(txt.to_string()).expect("alloc string"))
}

fn as_obj(v: Value) -> ObjRef {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

fn as_str(e: &Engine, v: Value) -> String {
    e.heap.as_str(as_obj(v)).expect("string").to_string()
}

fn new_inst(e: &mut Engine, cls: &str) -> ObjRef {
    let r = e
        .heap
        .alloc_instance(cls.to_string(), Vec::new())
        .expect("alloc instance");
    e.invoke_instance(r, "<init>", "()V", &[]).expect("<init>");
    r
}

fn assert_exc(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_class,
            "unexpected exception {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected_class}, got {other:?}"),
    }
}

#[test]
fn issue_303_copy_of_copy_of_range_and_vector_set_size_enforce_heap_budget() {
    let mut e = engine();
    let empty_ints = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, 0, "[I")
        .expect("alloc int[0]");

    // Arrays.copyOf(new int[0], Integer.MAX_VALUE) -> catchable OutOfMemoryError (never 68.7 GiB host abort)
    assert_exc(
        e.invoke_static(
            ARRAYS,
            "copyOf",
            "([II)[I",
            &[Value::Obj(empty_ints), Value::Int(i32::MAX)],
        ),
        "Ljava/lang/OutOfMemoryError;",
    );

    // Arrays.copyOfRange(new int[0], 0, Integer.MAX_VALUE) -> catchable OutOfMemoryError
    assert_exc(
        e.invoke_static(
            ARRAYS,
            "copyOfRange",
            "([III)[I",
            &[Value::Obj(empty_ints), Value::Int(0), Value::Int(i32::MAX)],
        ),
        "Ljava/lang/OutOfMemoryError;",
    );

    // 3-arg copyOf and 4-arg copyOfRange with null Class newType -> NullPointerException
    let obj_arr = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .expect("alloc Object[1]");
    assert_exc(
        e.invoke_static(
            ARRAYS,
            "copyOf",
            "([Ljava/lang/Object;ILjava/lang/Class;)[Ljava/lang/Object;",
            &[Value::Obj(obj_arr), Value::Int(1), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_static(
            ARRAYS,
            "copyOfRange",
            "([Ljava/lang/Object;IILjava/lang/Class;)[Ljava/lang/Object;",
            &[
                Value::Obj(obj_arr),
                Value::Int(0),
                Value::Int(1),
                Value::Null,
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // Vector.setSize(Integer.MAX_VALUE) -> catchable OutOfMemoryError
    let vec_obj = new_inst(&mut e, VECTOR);
    assert_exc(
        e.invoke_instance(vec_obj, "setSize", "(I)V", &[Value::Int(i32::MAX)]),
        "Ljava/lang/OutOfMemoryError;",
    );
}

#[test]
fn issue_303_arrays_deep_to_string_handles_self_and_mutual_cycles_and_shared_siblings() {
    let mut e = engine();

    // 1. Direct self-cycle: Object[] a = new Object[1]; a[0] = a; -> "[[...]]"
    let a = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .expect("alloc Object[1]");
    e.heap.array_elems_mut(a).expect("elems")[0] = Value::Obj(a);
    let s_self = e
        .invoke_static(
            ARRAYS,
            "deepToString",
            "([Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(a)],
        )
        .expect("deepToString self");
    assert_eq!(as_str(&e, s_self), "[[...]]");

    // 2. Mutual 2-array cycle: a[0] = b; b[0] = a; -> "[[[...]]]"
    let b = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .expect("alloc Object[1]");
    e.heap.array_elems_mut(a).expect("elems")[0] = Value::Obj(b);
    e.heap.array_elems_mut(b).expect("elems")[0] = Value::Obj(a);
    let s_mutual = e
        .invoke_static(
            ARRAYS,
            "deepToString",
            "([Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(a)],
        )
        .expect("deepToString mutual");
    assert_eq!(as_str(&e, s_mutual), "[[[...]]]");

    // 3. Shared non-cyclic sibling sub-array: top = {sub, sub} -> "[[7], [7]]" (NOT "[[7], [...]]")
    let sub = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, 1, "[I")
        .expect("alloc int[1]");
    e.heap.array_elems_mut(sub).expect("elems")[0] = Value::Int(7);
    let top = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 2, "Ljava/lang/Object;")
        .expect("alloc Object[2]");
    {
        let slots = e.heap.array_elems_mut(top).expect("elems");
        slots[0] = Value::Obj(sub);
        slots[1] = Value::Obj(sub);
    }
    let s_sib = e
        .invoke_static(
            ARRAYS,
            "deepToString",
            "([Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(top)],
        )
        .expect("deepToString siblings");
    assert_eq!(as_str(&e, s_sib), "[[7], [7]]");
}

#[test]
fn issue_303_indirect_collection_and_map_cycles_throw_catchable_stack_overflow() {
    let mut e = engine();

    // Mutual ArrayList cycle: a = [b], b = [a]
    let a = new_inst(&mut e, ARRAY_LIST);
    let b = new_inst(&mut e, ARRAY_LIST);
    e.invoke_instance(a, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(b)])
        .expect("a.add(b)");
    e.invoke_instance(b, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(a)])
        .expect("b.add(a)");

    assert_exc(
        e.invoke_instance(a, "toString", "()Ljava/lang/String;", &[]),
        "Ljava/lang/StackOverflowError;",
    );
    assert_exc(
        e.invoke_instance(a, "hashCode", "()I", &[]),
        "Ljava/lang/StackOverflowError;",
    );
    assert_exc(
        e.invoke_instance(a, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(b)]),
        "Ljava/lang/StackOverflowError;",
    );

    // Mutual ArrayMap cycle: m1 = {"k": m2}, m2 = {"k": m1}
    let m1 = new_inst(&mut e, ARRAY_MAP);
    let m2 = new_inst(&mut e, ARRAY_MAP);
    let k = jstr(&mut e, "k");
    e.invoke_instance(
        m1,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[k.clone(), Value::Obj(m2)],
    )
    .expect("m1.put");
    e.invoke_instance(
        m2,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[k, Value::Obj(m1)],
    )
    .expect("m2.put");

    assert_exc(
        e.invoke_instance(m1, "toString", "()Ljava/lang/String;", &[]),
        "Ljava/lang/StackOverflowError;",
    );
}

#[test]
fn issue_303_host_classes_equals_and_hash_code_work_across_arrays_lists_sets_and_maps() {
    let mut e = engine();

    let s1 = jstr(&mut e, "https://example.com/item?id=1");
    let s2 = jstr(&mut e, "https://example.com/item?id=1");
    let u1 = as_obj(
        e.invoke_static(URI, "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[s1])
            .expect("Uri.parse 1"),
    );
    let u2 = as_obj(
        e.invoke_static(URI, "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[s2])
            .expect("Uri.parse 2"),
    );
    assert_ne!(u1, u2, "must test distinct ObjRefs with equal value");

    // 1. Arrays.equals & Arrays.hashCode on Uri[]
    let arr1 = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, URI)
        .expect("Uri[1]");
    let arr2 = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, URI)
        .expect("Uri[1]");
    e.heap.array_elems_mut(arr1).expect("elems")[0] = Value::Obj(u1);
    e.heap.array_elems_mut(arr2).expect("elems")[0] = Value::Obj(u2);

    assert_eq!(
        e.invoke_static(
            ARRAYS,
            "equals",
            "([Ljava/lang/Object;[Ljava/lang/Object;)Z",
            &[Value::Obj(arr1), Value::Obj(arr2)],
        )
        .expect("Arrays.equals"),
        Value::Int(1)
    );
    let h1 = e
        .invoke_static(
            ARRAYS,
            "hashCode",
            "([Ljava/lang/Object;)I",
            &[Value::Obj(arr1)],
        )
        .expect("Arrays.hashCode 1");
    let h2 = e
        .invoke_static(
            ARRAYS,
            "hashCode",
            "([Ljava/lang/Object;)I",
            &[Value::Obj(arr2)],
        )
        .expect("Arrays.hashCode 2");
    assert_eq!(h1, h2);

    // 2. HashSet<Uri>: add, contains, deduplication
    let set = new_inst(&mut e, HASH_SET);
    assert_eq!(
        e.invoke_instance(set, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(u1)])
            .expect("set.add u1"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(set, "contains", "(Ljava/lang/Object;)Z", &[Value::Obj(u2)])
            .expect("set.contains u2"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(set, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(u2)])
            .expect("set.add u2 dup"),
        Value::Int(0)
    );

    // 3. Nested ArrayList<ArrayList<Uri>> equality & hashCode
    let inner1 = new_inst(&mut e, ARRAY_LIST);
    let inner2 = new_inst(&mut e, ARRAY_LIST);
    e.invoke_instance(inner1, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(u1)])
        .expect("inner1.add");
    e.invoke_instance(inner2, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(u2)])
        .expect("inner2.add");
    let outer1 = new_inst(&mut e, ARRAY_LIST);
    let outer2 = new_inst(&mut e, ARRAY_LIST);
    e.invoke_instance(
        outer1,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(inner1)],
    )
    .expect("outer1.add");
    e.invoke_instance(
        outer2,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(inner2)],
    )
    .expect("outer2.add");
    assert_eq!(
        e.invoke_instance(
            outer1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(outer2)],
        )
        .expect("nested ArrayList equals"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(outer1, "hashCode", "()I", &[])
            .expect("outer1.hashCode"),
        e.invoke_instance(outer2, "hashCode", "()I", &[])
            .expect("outer2.hashCode")
    );

    // 4. HashMap<Uri, BigDecimal> key lookup, containsValue, equals, hashCode
    let bd1 = e
        .heap
        .alloc_instance(BIG_DECIMAL.to_string(), Vec::new())
        .expect("alloc BigDecimal");
    let bd_arg1 = jstr(&mut e, "42.5");
    e.invoke_instance(bd1, "<init>", "(Ljava/lang/String;)V", &[bd_arg1])
        .expect("BigDecimal.<init>");
    let bd2 = e
        .heap
        .alloc_instance(BIG_DECIMAL.to_string(), Vec::new())
        .expect("alloc BigDecimal");
    let bd_arg2 = jstr(&mut e, "42.5");
    e.invoke_instance(bd2, "<init>", "(Ljava/lang/String;)V", &[bd_arg2])
        .expect("BigDecimal.<init>");

    let map1 = new_inst(&mut e, HASH_MAP);
    e.invoke_instance(
        map1,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Obj(u1), Value::Obj(bd1)],
    )
    .expect("map1.put");
    assert_eq!(
        e.invoke_instance(
            map1,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[Value::Obj(u2)],
        )
        .expect("map1.get(u2)"),
        Value::Obj(bd1)
    );
    assert_eq!(
        e.invoke_instance(
            map1,
            "containsValue",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(bd2)],
        )
        .expect("map1.containsValue(bd2)"),
        Value::Int(1)
    );

    let map2 = new_inst(&mut e, HASH_MAP);
    e.invoke_instance(
        map2,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Obj(u2), Value::Obj(bd2)],
    )
    .expect("map2.put");
    assert_eq!(
        e.invoke_instance(map1, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(map2)])
            .expect("map1.equals(map2)"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(map1, "hashCode", "()I", &[])
            .expect("map1.hashCode"),
        e.invoke_instance(map2, "hashCode", "()I", &[])
            .expect("map2.hashCode")
    );

    // 5. Arrays.fill(boolean[], non01) normalizes stored element to 0 | 1
    let bools = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, 2, "[Z")
        .expect("alloc boolean[2]");
    e.invoke_static(
        ARRAYS,
        "fill",
        "([ZZ)V",
        &[Value::Obj(bools), Value::Int(7)],
    )
    .expect("Arrays.fill([Z, 7)");
    assert_eq!(
        e.heap.as_array_elems(bools).expect("elems").0[0],
        Value::Int(1)
    );
}
