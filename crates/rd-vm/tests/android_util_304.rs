//! Regression suite for Issue #304:
//! 1. `Engine.sparse` ephemeron tracing + GC sweep (no permanent memory leak,
//!    no scalar key false-pinning, live `SparseArray` entries preserved across
//!    minor/major GC).
//! 2. `Base64InputStream.available()` overflow safety (`>= 715_827_883`) and
//!    chunked streaming without draining unbounded streams into host RAM.
//! 3. `SparseArray*` AOSP `toString()` (including `(this Map)` self-cycle),
//!    polymorphic dispatch through `java.lang.Object` / `java.lang.Number`,
//!    semantic `Pair`/`Range` `equals` & `hashCode`, and `Rational` `MIN_VALUE`
//!    + `±Infinity` / `NaN` semantics.

mod common;

use common::*;
use rd_dex::opcode::info;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const ACC_PUBLIC: u32 = 0x0001;
const ACC_STATIC: u32 = 0x0008;

fn op(name: &str) -> u8 {
    for code in 0u16..=255 {
        let meta = info(code as u8);
        if meta.name == name {
            return code as u8;
        }
    }
    panic!("unknown opcode {name}");
}

fn op21t(opc: u8, a: u8, off: i16) -> Vec<u16> {
    vec![(opc as u16) | ((a as u16) << 8), off as u16]
}

fn alloc_str(vm: &mut Engine, s: &str) -> ObjRef {
    vm.heap.alloc_string(s.to_string()).unwrap()
}

fn as_string(vm: &Engine, v: Value) -> String {
    let Value::Obj(r) = v else {
        panic!("expected String ObjRef, got {v:?}");
    };
    vm.heap.as_str(r).unwrap().to_string()
}

/// 1a. Unreachable `SparseArray` + its referenced objects are reclaimed by GC
///     and purged from `Engine.sparse`, while a live `SparseArray` keeps its
///     values alive across minor + major GC.
/// 1b. Scalar `i64` keys inside a live `SparseIntArray` / `LongSparseArray`
///     must NOT conservatively pin unrelated heap objects whose `ObjRef` numeric
///     ID matches the integer key.
#[test]
fn sparse_array_gc_reclaims_unreachable_and_avoids_scalar_key_pinning_304() {
    let mut vm = engine_of(&DexBuilder::new());
    // Skip low ObjRef IDs (1..=8) that coincide with built-in main thread / config ints
    // in conservative root formatting.
    for _ in 0..430 {
        let _ = alloc_str(&mut vm, "pad");
    }

    let dead_sparse = vm
        .heap
        .alloc_instance("Landroid/util/SparseArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(dead_sparse, "<init>", "()V", &[])
        .unwrap();
    let dead_val = alloc_str(&mut vm, "ephemeral-sparse-payload");
    vm.invoke_instance(
        dead_sparse,
        "put",
        "(ILjava/lang/Object;)V",
        &[Value::Int(7), Value::Obj(dead_val)],
    )
    .unwrap();

    let live_sparse = vm
        .heap
        .alloc_instance("Landroid/util/SparseArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(live_sparse, "<init>", "()V", &[])
        .unwrap();
    let live_val = alloc_str(&mut vm, "kept-alive-by-live-sparse");
    vm.invoke_instance(
        live_sparse,
        "put",
        "(ILjava/lang/Object;)V",
        &[Value::Int(1), Value::Obj(live_val)],
    )
    .unwrap();

    let live_int_sparse = vm
        .heap
        .alloc_instance("Landroid/util/SparseIntArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(live_int_sparse, "<init>", "(I)V", &[Value::Int(4)])
        .unwrap();
    let victim_by_key_id = alloc_str(&mut vm, "must-not-be-pinned-by-int-key");
    vm.invoke_instance(
        live_int_sparse,
        "put",
        "(II)V",
        &[Value::Int(victim_by_key_id as i32), Value::Int(999)],
    )
    .unwrap();

    vm.statics.insert(
        ("LRoot;".to_string(), "liveSparse".to_string()),
        Value::Obj(live_sparse),
    );
    vm.statics.insert(
        ("LRoot;".to_string(), "liveIntSparse".to_string()),
        Value::Obj(live_int_sparse),
    );

    vm.gc();
    assert_eq!(
        vm.heap.class_of(dead_sparse).ok(),
        Some(rd_vm::gc::FREED_CLASS),
        "unreachable SparseArray itself must be reclaimed by GC"
    );
    assert_eq!(
        vm.heap.class_of(dead_val).ok(),
        Some(rd_vm::gc::FREED_CLASS),
        "value inside unreachable SparseArray must be reclaimed by GC"
    );
    assert_eq!(
        vm.heap.class_of(victim_by_key_id).ok(),
        Some(rd_vm::gc::FREED_CLASS),
        "scalar integer key in SparseIntArray must NOT false-pin matching ObjRef"
    );

    let young_val = alloc_str(&mut vm, "young-entry-survives-minor-gc");
    vm.invoke_instance(
        live_sparse,
        "put",
        "(ILjava/lang/Object;)V",
        &[Value::Int(2), Value::Obj(young_val)],
    )
    .unwrap();
    vm.gc_minor();
    assert_eq!(
        vm.heap.as_str(young_val).ok(),
        Some("young-entry-survives-minor-gc"),
        "young object stored in promoted SparseArray must survive minor GC"
    );
    assert_eq!(
        vm.heap.as_str(live_val).ok(),
        Some("kept-alive-by-live-sparse")
    );

    // Allocate SparseArrays until `dead_sparse` slot is recycled and verify its
    // side-table entry in `Engine.sparse` was swept (size() == 0 without <init>).
    let mut recycled = None;
    for _ in 0..512 {
        let cand = vm
            .heap
            .alloc_instance("Landroid/util/SparseArray;".to_string(), Vec::new())
            .unwrap();
        if cand == dead_sparse {
            recycled = Some(cand);
            break;
        }
    }
    let recycled = recycled.expect("freed dead_sparse slot should be recycled");
    assert_eq!(
        vm.invoke_instance(recycled, "size", "()I", &[]).unwrap(),
        Value::Int(0),
        "recycled ObjRef must not inherit stale Engine.sparse map"
    );
}

/// 2a. `Base64InputStream.available()` must not panic or wrap negative when
///     the underlying `InputStream.available()` returns `>= 715_827_883`.
/// 2b. `Base64InputStream.read()` on an infinite guest `InputStream` must
///     decode incrementally in bounded chunks without draining the stream to
///     EOF.
#[test]
fn base64_input_stream_available_overflow_and_infinite_stream_chunking_304() {
    let mut b = DexBuilder::new();
    let cls_idx = b.class("LInfiniteB64Stream;", "Ljava/io/InputStream;");

    // available()I -> return Integer.MAX_VALUE (0x7fff_ffff)
    let mut u_avail = Vec::new();
    u_avail.extend(op31t(op("const"), 0, i32::MAX));
    u_avail.extend(op11x(op("return"), 0));
    let c_avail = b.code(1, 1, 0, u_avail);
    b.r#virtual(cls_idx, "available", "I", vec![], ACC_PUBLIC, Some(c_avail));

    // read([BII)I -> writes "QUJD" ("ABC") at buf[off..off+4] and returns 4 forever!
    // regs: v0..v3 locals, v4=this, v5=buf, v6=off, v7=len
    let mut u_read = Vec::new();
    // if-gtz v7, +5 (jump to write block when len > 0? Wait: if-gtz jumps when v7 > 0!)
    // offset +4 skips const/4 (1 unit) + return (1 unit) -> wait: if-gtz is 2 units,
    // const/4 is 1 unit, return is 1 unit => target is +4!
    u_read.extend(op21t(op("if-gtz"), 7, 4));
    u_read.extend(op11n(op("const/4"), 0, 0));
    u_read.extend(op11x(op("return"), 0));
    for (idx, byte) in b"QUJD".iter().enumerate() {
        u_read.extend(op21s(op("const/16"), 0, i16::from(*byte)));
        u_read.extend(op22b(op("add-int/lit8"), 1, 6, idx as i8));
        u_read.extend(op23x(op("aput-byte"), 0, 5, 1));
    }
    u_read.extend(op11n(op("const/4"), 0, 4));
    u_read.extend(op11x(op("return"), 0));
    let c_read = b.code(8, 4, 0, u_read);
    b.r#virtual(
        cls_idx,
        "read",
        "I",
        vec!["[B", "I", "I"],
        ACC_PUBLIC,
        Some(c_read),
    );

    let cfg = VmConfig {
        fuel: 50_000,
        heap_budget: 2 * 1024 * 1024,
        ..VmConfig::default()
    };
    let dex = rd_dex::Dex::parse(b.finish()).unwrap();
    let mut vm = Engine::new(vec![dex], cfg);

    let inf_stream = vm
        .heap
        .alloc_instance("LInfiniteB64Stream;".to_string(), Vec::new())
        .unwrap();
    let b64_in = vm
        .heap
        .alloc_instance("Landroid/util/Base64InputStream;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(
        b64_in,
        "<init>",
        "(Ljava/io/InputStream;I)V",
        &[Value::Obj(inf_stream), Value::Int(0)],
    )
    .unwrap();

    // 2a: available() with Integer.MAX_VALUE must NOT panic or wrap negative.
    let avail = vm
        .invoke_instance(b64_in, "available", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(avail, 1_610_612_735);

    // 2b: Reading 5 bytes ("ABCAB") from an infinite stream must succeed within
    // tiny fuel & 2 MiB heap budget without draining to EOF!
    let mut got = Vec::new();
    for _ in 0..5 {
        let b_val = vm
            .invoke_instance(b64_in, "read", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap();
        got.push(b_val as u8);
    }
    assert_eq!(&got, b"ABCAB");

    // Multi-chunk read([BII)I also stays bounded:
    let buf = vm.heap.alloc_array_zeroed(ElemKind::Int, 12, "[B").unwrap();
    let n = vm
        .invoke_instance(
            b64_in,
            "read",
            "([BII)I",
            &[Value::Obj(buf), Value::Int(0), Value::Int(12)],
        )
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(n, 12);
    let slice: Vec<u8> = vm
        .heap
        .as_array_elems(buf)
        .unwrap()
        .0
        .iter()
        .map(|v| v.as_int().unwrap() as u8)
        .collect();
    assert_eq!(&slice, b"CABCABCABCAB");
}

/// 3a & 3b. `SparseArray*` format AOSP `toString()` (including `(this Map)`)
///          and dispatch polymorphically via `Ljava/lang/Object;->toString`.
#[test]
fn sparse_arrays_to_string_and_object_polymorphic_dispatch_304() {
    let mut b = DexBuilder::new();
    let cls_idx = b.class("LPolyCall304;", "Ljava/lang/Object;");
    let p_to_str = b.proto_idx("Ljava/lang/String;", vec![]);
    let m_obj_to_str = b.method_idx("Ljava/lang/Object;", p_to_str, "toString");
    let mut u = Vec::new();
    u.extend(op35c(
        op("invoke-virtual"),
        1,
        m_obj_to_str,
        [1, 0, 0, 0, 0],
    ));
    u.extend(op11x(op("move-result-object"), 0));
    u.extend(op11x(op("return-object"), 0));
    let c = b.code(2, 1, 1, u);
    b.direct(
        cls_idx,
        "objToString",
        "Ljava/lang/String;",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(c),
    );

    let mut vm = engine_of(&b);

    let sia = vm
        .heap
        .alloc_instance("Landroid/util/SparseIntArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(sia, "<init>", "()V", &[]).unwrap();
    vm.invoke_instance(sia, "put", "(II)V", &[Value::Int(1), Value::Int(7)])
        .unwrap();
    let s_sia = vm
        .invoke_static(
            "LPolyCall304;",
            "objToString",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(sia)],
        )
        .unwrap();
    assert_eq!(as_string(&vm, s_sia), "{1=7}");

    let sba = vm
        .heap
        .alloc_instance("Landroid/util/SparseBooleanArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(sba, "<init>", "()V", &[]).unwrap();
    vm.invoke_instance(sba, "put", "(IZ)V", &[Value::Int(1), Value::Int(1)])
        .unwrap();
    vm.invoke_instance(sba, "put", "(IZ)V", &[Value::Int(2), Value::Int(0)])
        .unwrap();
    let s_sba = vm
        .invoke_static(
            "LPolyCall304;",
            "objToString",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(sba)],
        )
        .unwrap();
    assert_eq!(as_string(&vm, s_sba), "{1=true, 2=false}");

    let sa = vm
        .heap
        .alloc_instance("Landroid/util/SparseArray;".to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(sa, "<init>", "()V", &[]).unwrap();
    let hello = alloc_str(&mut vm, "ola");
    vm.invoke_instance(
        sa,
        "put",
        "(ILjava/lang/Object;)V",
        &[Value::Int(1), Value::Obj(hello)],
    )
    .unwrap();
    vm.invoke_instance(
        sa,
        "put",
        "(ILjava/lang/Object;)V",
        &[Value::Int(2), Value::Obj(sa)],
    )
    .unwrap();
    let s_sa = vm
        .invoke_static(
            "LPolyCall304;",
            "objToString",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(sa)],
        )
        .unwrap();
    assert_eq!(as_string(&vm, s_sa), "{1=ola, 2=(this Map)}");
}

/// 3b & 3c. `Pair` and `Range` compare/hash semantically (`Objects.equals` and
///          `Objects.hashCode` with `^` for `Pair`) via `Object.equals`/`hashCode`.
#[test]
fn pair_and_range_semantic_equals_hash_and_object_dispatch_304() {
    let mut b = DexBuilder::new();
    let cls_idx = b.class("LObjEqHash304;", "Ljava/lang/Object;");

    let p_eq = b.proto_idx("Z", vec!["Ljava/lang/Object;".to_string()]);
    let m_obj_eq = b.method_idx("Ljava/lang/Object;", p_eq, "equals");
    let mut u_eq = Vec::new();
    u_eq.extend(op35c(op("invoke-virtual"), 2, m_obj_eq, [1, 2, 0, 0, 0]));
    u_eq.extend(op11x(op("move-result"), 0));
    u_eq.extend(op11x(op("return"), 0));
    let c_eq = b.code(3, 2, 2, u_eq);
    b.direct(
        cls_idx,
        "objEquals",
        "Z",
        vec!["Ljava/lang/Object;", "Ljava/lang/Object;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(c_eq),
    );

    let p_hc = b.proto_idx("I", vec![]);
    let m_obj_hc = b.method_idx("Ljava/lang/Object;", p_hc, "hashCode");
    let mut u_hc = Vec::new();
    u_hc.extend(op35c(op("invoke-virtual"), 1, m_obj_hc, [1, 0, 0, 0, 0]));
    u_hc.extend(op11x(op("move-result"), 0));
    u_hc.extend(op11x(op("return"), 0));
    let c_hc = b.code(2, 1, 1, u_hc);
    b.direct(
        cls_idx,
        "objHash",
        "I",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(c_hc),
    );

    let mut vm = engine_of(&b);

    let s1 = alloc_str(&mut vm, "alpha");
    let s2 = alloc_str(&mut vm, "alpha");
    assert_ne!(s1, s2);
    let i1 = vm
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(500)],
        )
        .unwrap();
    let i2 = vm
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(500)],
        )
        .unwrap();

    let p1 = vm
        .invoke_static(
            "Landroid/util/Pair;",
            "create",
            "(Ljava/lang/Object;Ljava/lang/Object;)Landroid/util/Pair;",
            &[Value::Obj(s1), i1],
        )
        .unwrap();
    let p2 = vm
        .invoke_static(
            "Landroid/util/Pair;",
            "create",
            "(Ljava/lang/Object;Ljava/lang/Object;)Landroid/util/Pair;",
            &[Value::Obj(s2), i2],
        )
        .unwrap();

    let eq = vm
        .invoke_static(
            "LObjEqHash304;",
            "objEquals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            &[p1.clone(), p2.clone()],
        )
        .unwrap();
    assert_eq!(eq, Value::Int(1));

    let h1 = vm
        .invoke_static("LObjEqHash304;", "objHash", "(Ljava/lang/Object;)I", &[p1])
        .unwrap()
        .as_int()
        .unwrap();
    let h2 = vm
        .invoke_static("LObjEqHash304;", "objHash", "(Ljava/lang/Object;)I", &[p2])
        .unwrap()
        .as_int()
        .unwrap();
    let alpha_hash = "alpha"
        .encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    assert_eq!(h1, alpha_hash ^ 500);
    assert_eq!(h1, h2);

    let lo1 = alloc_str(&mut vm, "a");
    let hi1 = alloc_str(&mut vm, "z");
    let lo2 = alloc_str(&mut vm, "a");
    let hi2 = alloc_str(&mut vm, "z");
    let r1 = vm
        .invoke_static(
            "Landroid/util/Range;",
            "create",
            "(Ljava/lang/Comparable;Ljava/lang/Comparable;)Landroid/util/Range;",
            &[Value::Obj(lo1), Value::Obj(hi1)],
        )
        .unwrap();
    let r2 = vm
        .invoke_static(
            "Landroid/util/Range;",
            "create",
            "(Ljava/lang/Comparable;Ljava/lang/Comparable;)Landroid/util/Range;",
            &[Value::Obj(lo2), Value::Obj(hi2)],
        )
        .unwrap();
    let req = vm
        .invoke_static(
            "LObjEqHash304;",
            "objEquals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            &[r1, r2],
        )
        .unwrap();
    assert_eq!(req, Value::Int(1));
}

/// 3b & 3d. `Rational` handles `new Rational(Integer.MIN_VALUE, -1)` without
///          wrapping back to `Integer.MIN_VALUE`, returns `±Infinity` for
///          `±1/0` and `NaN` only for `0/0`, and dispatches through
///          `Ljava/lang/Number;->doubleValue()D`.
#[test]
fn rational_min_value_infinities_and_number_polymorphism_304() {
    let mut b = DexBuilder::new();
    let cls_idx = b.class("LNumPoly304;", "Ljava/lang/Object;");
    let p_dbl = b.proto_idx("D", vec![]);
    let m_num_dbl = b.method_idx("Ljava/lang/Number;", p_dbl, "doubleValue");
    let mut u = Vec::new();
    u.extend(op35c(op("invoke-virtual"), 1, m_num_dbl, [2, 0, 0, 0, 0]));
    u.extend(op11x(op("move-result-wide"), 0));
    u.extend(op11x(op("return-wide"), 0));
    let c = b.code(3, 1, 1, u);
    b.direct(
        cls_idx,
        "asDouble",
        "D",
        vec!["Ljava/lang/Number;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(c),
    );

    let mut vm = engine_of(&b);

    let make_rat = |vm: &mut Engine, n: i32, d: i32| -> ObjRef {
        let r = vm
            .heap
            .alloc_instance("Landroid/util/Rational;".to_string(), Vec::new())
            .unwrap();
        vm.invoke_instance(r, "<init>", "(II)V", &[Value::Int(n), Value::Int(d)])
            .unwrap();
        r
    };

    let r_min_neg = make_rat(&mut vm, i32::MIN, -1);
    let num = vm
        .invoke_instance(r_min_neg, "getNumerator", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert!(num > 0, "numerator must be positive, got {num}");
    let dbl = match vm
        .invoke_static(
            "LNumPoly304;",
            "asDouble",
            "(Ljava/lang/Number;)D",
            &[Value::Obj(r_min_neg)],
        )
        .unwrap()
    {
        Value::Double(d) => d,
        other => panic!("expected Double, got {other:?}"),
    };
    assert!(dbl >= 2_147_483_647.0, "expected >= 2^31 - 1, got {dbl}");

    let r_pos_inf = make_rat(&mut vm, 5, 0);
    let pos_d = match vm
        .invoke_static(
            "LNumPoly304;",
            "asDouble",
            "(Ljava/lang/Number;)D",
            &[Value::Obj(r_pos_inf)],
        )
        .unwrap()
    {
        Value::Double(d) => d,
        other => panic!("expected Double, got {other:?}"),
    };
    assert!(pos_d.is_infinite() && pos_d.is_sign_positive());

    let r_neg_inf = make_rat(&mut vm, -5, 0);
    let neg_d = match vm
        .invoke_instance(r_neg_inf, "floatValue", "()F", &[])
        .unwrap()
    {
        Value::Float(f) => f,
        other => panic!("expected Float, got {other:?}"),
    };
    assert!(neg_d.is_infinite() && neg_d.is_sign_negative());

    let r_nan = make_rat(&mut vm, 0, 0);
    let nan_d = match vm
        .invoke_instance(r_nan, "doubleValue", "()D", &[])
        .unwrap()
    {
        Value::Double(d) => d,
        other => panic!("expected Double, got {other:?}"),
    };
    assert!(nan_d.is_nan());
}
