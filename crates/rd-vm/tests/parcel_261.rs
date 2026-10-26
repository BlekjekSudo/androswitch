//! Regression tests for issue #261 (`android.os.Parcel` host OOM via unchecked
//! element counts, `setDataSize` without cap, recursion/cycle stack overflow,
//! checked arithmetic on 32-bit wrap hazards, and non-finite `VAL_SIZEF`).

use rd_vm::engine::Engine;
use rd_vm::err::VmExit;
use rd_vm::heap::{ElemKind, ObjRef};
use rd_vm::intrinsics::{alloc_string, call_instance_intrinsic, call_static_intrinsic};
use rd_vm::value::Value;
use rd_vm::VmConfig;

const PARCEL: &str = "Landroid/os/Parcel;";
const BUNDLE: &str = "Landroid/os/Bundle;";
const ARRAY_LIST: &str = "Ljava/util/ArrayList;";
const HASH_MAP: &str = "Ljava/util/HashMap;";
const ARRAY_MAP: &str = "Landroid/util/ArrayMap;";

const BUNDLE_MAGIC: i32 = 0x4c44_4e42;

const VAL_MAP: i32 = 2;
const VAL_LIST: i32 = 11;
const VAL_SPARSEARRAY: i32 = 12;
const VAL_STRINGARRAY: i32 = 14;
const VAL_OBJECTARRAY: i32 = 17;
const VAL_INTARRAY: i32 = 18;
const VAL_LONGARRAY: i32 = 19;
const VAL_SPARSEBOOLEANARRAY: i32 = 22;
const VAL_SIZEF: i32 = 27;
const VAL_DOUBLEARRAY: i32 = 28;
const VAL_FLOATARRAY: i32 = 32;

fn make_vm() -> Engine {
    let cfg = VmConfig {
        heap_budget: 8 * 1024 * 1024,
        ..VmConfig::default()
    };
    Engine::new(Vec::new(), cfg)
}

fn obtain_parcel(vm: &mut Engine) -> ObjRef {
    let res = call_static_intrinsic(vm, PARCEL, "obtain", "()Landroid/os/Parcel;", &[]);
    assert!(matches!(res, Ok(Some(Value::Obj(_)))));
    match res {
        Ok(Some(Value::Obj(r))) => r,
        _ => 0,
    }
}

fn unmarshall_words(vm: &mut Engine, words: &[i32]) -> ObjRef {
    let p = obtain_parcel(vm);
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for w in words {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    let arr_res = vm.heap.alloc_array(ElemKind::Int, bytes.len(), "[B");
    assert!(arr_res.is_ok());
    let arr = arr_res.unwrap_or(0);
    if let Ok(slot) = vm.heap.array_elems_mut(arr) {
        slot.extend(bytes.iter().map(|&b| Value::Int(b as i8 as i32)));
    }
    let res = call_instance_intrinsic(
        vm,
        PARCEL,
        "unmarshall",
        "([BII)V",
        p,
        &[
            Value::Obj(arr),
            Value::Int(0),
            Value::Int(bytes.len() as i32),
        ],
    );
    assert!(matches!(res, Ok(Some(Value::Null))));
    p
}

fn assert_thrown(res: Result<Option<Value>, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, expected_class,
                "expected {expected_class}, got {} ({:?})",
                t.class, t.message
            );
        }
        other => {
            let got = format!("{other:?}");
            assert_eq!(got, expected_class, "expected thrown {expected_class}");
        }
    }
}

fn new_host_instance(vm: &mut Engine, desc: &str) -> ObjRef {
    let r = vm.heap.alloc_instance(desc.to_string(), Vec::new());
    assert!(r.is_ok());
    let obj = r.unwrap_or(0);
    let init = call_instance_intrinsic(vm, desc, "<init>", "()V", obj, &[]);
    assert!(matches!(init, Ok(Some(Value::Null))));
    obj
}

#[test]
fn huge_element_counts_fail_fast_on_data_avail_without_host_oom() {
    let mut vm = make_vm();

    for (name, sig) in [
        ("createIntArray", "()[I"),
        ("createCharArray", "()[C"),
        ("createShortArray", "()[S"),
        ("createLongArray", "()[J"),
        ("createFloatArray", "()[F"),
        ("createDoubleArray", "()[D"),
        ("createBooleanArray", "()[Z"),
        ("createStringArray", "()[Ljava/lang/String;"),
        ("createString8Array", "()[Ljava/lang/String;"),
        ("createStringArrayList", "()Ljava/util/ArrayList;"),
        ("createIBinderArray", "()[Landroid/os/IBinder;"),
        ("createBinderArrayList", "()Ljava/util/ArrayList;"),
        ("readSparseIntArray", "()Landroid/util/SparseIntArray;"),
        (
            "readSparseBooleanArray",
            "()Landroid/util/SparseBooleanArray;",
        ),
    ] {
        let p = unmarshall_words(&mut vm, &[i32::MAX]);
        let res = call_instance_intrinsic(&mut vm, PARCEL, name, sig, p, &[]);
        assert_thrown(res, "Landroid/os/BadParcelableException;");
    }

    for (name, sig) in [
        (
            "readArrayList",
            "(Ljava/lang/ClassLoader;)Ljava/util/ArrayList;",
        ),
        ("readArray", "(Ljava/lang/ClassLoader;)[Ljava/lang/Object;"),
        (
            "readHashMap",
            "(Ljava/lang/ClassLoader;)Ljava/util/HashMap;",
        ),
        (
            "readArraySet",
            "(Ljava/lang/ClassLoader;)Landroid/util/ArraySet;",
        ),
        (
            "readSparseArray",
            "(Ljava/lang/ClassLoader;)Landroid/util/SparseArray;",
        ),
    ] {
        let p = unmarshall_words(&mut vm, &[i32::MAX]);
        let res = call_instance_intrinsic(&mut vm, PARCEL, name, sig, p, &[Value::Null]);
        assert_thrown(res, "Landroid/os/BadParcelableException;");
    }

    let array_map = new_host_instance(&mut vm, ARRAY_MAP);
    let p_am = unmarshall_words(&mut vm, &[i32::MAX]);
    let res_am = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "readArrayMap",
        "(Landroid/util/ArrayMap;Ljava/lang/ClassLoader;)V",
        p_am,
        &[Value::Obj(array_map), Value::Null],
    );
    assert_thrown(res_am, "Landroid/os/BadParcelableException;");

    // Length-prefixed and non-length-prefixed `readValue` array/container tags.
    for tag in [
        VAL_INTARRAY,
        VAL_LONGARRAY,
        VAL_FLOATARRAY,
        VAL_DOUBLEARRAY,
        VAL_STRINGARRAY,
        VAL_SPARSEBOOLEANARRAY,
    ] {
        let p = unmarshall_words(&mut vm, &[tag, i32::MAX]);
        let res = call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            p,
            &[Value::Null],
        );
        assert_thrown(res, "Landroid/os/BadParcelableException;");
    }

    for tag in [VAL_LIST, VAL_MAP, VAL_OBJECTARRAY, VAL_SPARSEARRAY] {
        // Length-prefixed tag: [tag, payload_len=4, count=i32::MAX]
        let p = unmarshall_words(&mut vm, &[tag, 4, i32::MAX]);
        let res = call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            p,
            &[Value::Null],
        );
        assert_thrown(res, "Landroid/os/BadParcelableException;");
    }

    // Hostile Bundle frame claiming i32::MAX entries inside a 4-byte payload:
    // lazy `readBundle` succeeds, and `Bundle.size()` (unparcel) rejects via `dataAvail`.
    let p_bundle = unmarshall_words(&mut vm, &[4, BUNDLE_MAGIC, i32::MAX]);
    let b_res = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "readBundle",
        "()Landroid/os/Bundle;",
        p_bundle,
        &[],
    );
    assert!(matches!(b_res, Ok(Some(Value::Obj(_)))));
    let b_obj = match b_res {
        Ok(Some(Value::Obj(r))) => r,
        _ => 0,
    };
    let size_res = call_instance_intrinsic(&mut vm, BUNDLE, "size", "()I", b_obj, &[]);
    assert_thrown(size_res, "Landroid/os/BadParcelableException;");
}

#[test]
fn set_data_size_and_parcel_growth_enforce_max_parcel_bytes() {
    let mut vm = make_vm();
    let p = obtain_parcel(&mut vm);

    // Reasonable size works.
    let ok = call_instance_intrinsic(&mut vm, PARCEL, "setDataSize", "(I)V", p, &[Value::Int(64)]);
    assert!(matches!(ok, Ok(Some(Value::Null))));
    let sz = call_instance_intrinsic(&mut vm, PARCEL, "dataSize", "()I", p, &[]);
    assert_eq!(sz.ok().flatten(), Some(Value::Int(64)));

    // 2 GiB `setDataSize(i32::MAX)` and `16 MiB + 1` fail with BadParcelableException.
    let too_big_max = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "setDataSize",
        "(I)V",
        p,
        &[Value::Int(i32::MAX)],
    );
    assert_thrown(too_big_max, "Landroid/os/BadParcelableException;");

    let too_big_edge = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "setDataSize",
        "(I)V",
        p,
        &[Value::Int(16 * 1024 * 1024 + 1)],
    );
    assert_thrown(too_big_edge, "Landroid/os/BadParcelableException;");

    let cap_max = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "setDataCapacity",
        "(I)V",
        p,
        &[Value::Int(i32::MAX)],
    );
    assert_thrown(cap_max, "Landroid/os/BadParcelableException;");

    // Repeated `appendFrom` beyond MAX_PARCEL_BYTES also fails via `write_raw`.
    let a = obtain_parcel(&mut vm);
    let b = obtain_parcel(&mut vm);
    let seed = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "setDataSize",
        "(I)V",
        a,
        &[Value::Int(9 * 1024 * 1024)],
    );
    assert!(matches!(seed, Ok(Some(Value::Null))));
    let first = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "appendFrom",
        "(Landroid/os/Parcel;II)V",
        b,
        &[Value::Obj(a), Value::Int(0), Value::Int(9 * 1024 * 1024)],
    );
    assert!(matches!(first, Ok(Some(Value::Null))));
    let second = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "appendFrom",
        "(Landroid/os/Parcel;II)V",
        b,
        &[Value::Obj(a), Value::Int(0), Value::Int(9 * 1024 * 1024)],
    );
    assert_thrown(second, "Landroid/os/BadParcelableException;");
}

#[test]
fn cyclic_and_deeply_nested_containers_throw_instead_of_overflowing_stack() {
    let mut vm = make_vm();

    // 1. Self-referential ArrayList (`list.add(list)`).
    let list = new_host_instance(&mut vm, ARRAY_LIST);
    let add_res = call_instance_intrinsic(
        &mut vm,
        ARRAY_LIST,
        "add",
        "(Ljava/lang/Object;)Z",
        list,
        &[Value::Obj(list)],
    );
    assert!(add_res.is_ok());
    let p1 = obtain_parcel(&mut vm);
    let w_list = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "writeValue",
        "(Ljava/lang/Object;)V",
        p1,
        &[Value::Obj(list)],
    );
    assert_thrown(w_list, "Landroid/os/BadParcelableException;");

    // 2. Self-referential HashMap (`map.put("self", map)`).
    let map = new_host_instance(&mut vm, HASH_MAP);
    let key_res = alloc_string(&mut vm, "self".to_string());
    assert!(key_res.is_ok());
    let key = key_res.unwrap_or(0);
    let put_res = call_instance_intrinsic(
        &mut vm,
        HASH_MAP,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        map,
        &[Value::Obj(key), Value::Obj(map)],
    );
    assert!(put_res.is_ok());
    let p2 = obtain_parcel(&mut vm);
    let w_map = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "writeValue",
        "(Ljava/lang/Object;)V",
        p2,
        &[Value::Obj(map)],
    );
    assert_thrown(w_map, "Landroid/os/BadParcelableException;");

    // 3. Self-referential Bundle (`bundle.putBundle("self", bundle)`).
    let bundle = new_host_instance(&mut vm, BUNDLE);
    let put_b = call_instance_intrinsic(
        &mut vm,
        BUNDLE,
        "putBundle",
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        bundle,
        &[Value::Obj(key), Value::Obj(bundle)],
    );
    assert!(put_b.is_ok());
    let p3 = obtain_parcel(&mut vm);
    let w_bundle = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "writeBundle",
        "(Landroid/os/Bundle;)V",
        p3,
        &[Value::Obj(bundle)],
    );
    assert_thrown(w_bundle, "Landroid/os/BadParcelableException;");

    // 4. Self-referential Object[] (`arr[0] = arr`).
    let arr_res = vm.heap.alloc_array(ElemKind::Obj, 1, "[Ljava/lang/Object;");
    assert!(arr_res.is_ok());
    let arr = arr_res.unwrap_or(0);
    if let Ok(slot) = vm.heap.array_elems_mut(arr) {
        slot.push(Value::Obj(arr));
    }
    let p4 = obtain_parcel(&mut vm);
    let w_arr = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "writeValue",
        "(Ljava/lang/Object;)V",
        p4,
        &[Value::Obj(arr)],
    );
    assert_thrown(w_arr, "Landroid/os/BadParcelableException;");

    // 5. Deeply nested readValue stream (150 nested VAL_LIST frames ending in VAL_NULL).
    let levels = 150usize;
    let mut words = Vec::with_capacity(levels * 3 + 1);
    for rem in (1..=levels).rev() {
        // Each VAL_LIST header adds 3 words (tag, payload_len, count=1); leaf is 1 word (VAL_NULL = -1).
        let payload_bytes = ((rem - 1) * 12 + 8) as i32;
        words.push(VAL_LIST);
        words.push(payload_bytes);
        words.push(1);
    }
    words.push(-1);
    let p5 = unmarshall_words(&mut vm, &words);
    let r_deep = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "readValue",
        "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
        p5,
        &[Value::Null],
    );
    assert_thrown(r_deep, "Landroid/os/BadParcelableException;");
}

#[test]
fn checked_arithmetic_and_val_sizef_nan_infinity_validation() {
    let mut vm = make_vm();

    // `readString` and `readString8` with n = i32::MAX (32-bit wraparound hazard `(n + 1) * 2`).
    let p_str = unmarshall_words(&mut vm, &[i32::MAX]);
    assert_thrown(
        call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readString",
            "()Ljava/lang/String;",
            p_str,
            &[],
        ),
        "Landroid/os/BadParcelableException;",
    );

    let p_str8 = unmarshall_words(&mut vm, &[i32::MAX]);
    assert_thrown(
        call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readString8",
            "()Ljava/lang/String;",
            p_str8,
            &[],
        ),
        "Landroid/os/BadParcelableException;",
    );

    // `readExceptionCode` with StrictMode header (-128) and header_size = i32::MAX.
    let p_ex = unmarshall_words(&mut vm, &[-128, i32::MAX]);
    assert_thrown(
        call_instance_intrinsic(&mut vm, PARCEL, "readExceptionCode", "()I", p_ex, &[]),
        "Landroid/os/BadParcelableException;",
    );

    // `readValue` length-prefixed tag with length = i32::MAX.
    let p_lp = unmarshall_words(&mut vm, &[VAL_LIST, i32::MAX, 0]);
    assert_thrown(
        call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            p_lp,
            &[Value::Null],
        ),
        "Landroid/os/BadParcelableException;",
    );

    // `VAL_SIZEF` in `readValue` rejects NaN and Infinity with IllegalArgumentException.
    let p_nan = unmarshall_words(
        &mut vm,
        &[
            VAL_SIZEF,
            f32::NAN.to_bits() as i32,
            1.0f32.to_bits() as i32,
        ],
    );
    assert_thrown(
        call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            p_nan,
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let p_inf = unmarshall_words(
        &mut vm,
        &[
            VAL_SIZEF,
            2.0f32.to_bits() as i32,
            f32::NEG_INFINITY.to_bits() as i32,
        ],
    );
    assert_thrown(
        call_instance_intrinsic(
            &mut vm,
            PARCEL,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            p_inf,
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Finite `VAL_SIZEF` succeeds in `readValue`.
    let p_ok = unmarshall_words(
        &mut vm,
        &[VAL_SIZEF, 3.5f32.to_bits() as i32, 7.25f32.to_bits() as i32],
    );
    let ok_res = call_instance_intrinsic(
        &mut vm,
        PARCEL,
        "readValue",
        "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
        p_ok,
        &[Value::Null],
    );
    assert!(matches!(ok_res, Ok(Some(Value::Obj(_)))));
}
