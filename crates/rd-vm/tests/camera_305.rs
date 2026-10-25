#![cfg(feature = "virtual-devices")]
//! Regression tests for Issue #305 (`java.nio.ByteBuffer` in `m10/camera.rs`):
//! 1. `ByteBuffer.put(byte[], int offset, int length)` negative `offset`/`length`
//!    (`usize::MAX` wrap) and `pos + len` `i32` overflow panics.
//! 2. `ByteBuffer.wrap(byte[], int offset, int length)` and
//!    `ByteBuffer.get(byte[], int offset, int length)` `i32` `off + len` / `pos + len`
//!    overflow panics, plus `get(null)` / `put(null)` `NullPointerException`.
//! 3. `ByteBuffer.allocate(Integer.MAX_VALUE)` / `allocateDirect(Integer.MAX_VALUE)`
//!    enforcing `VmConfig::heap_budget` directly without allocating an intermediate
//!    2 GiB native `Vec<u8>`, throwing guest-catchable `OutOfMemoryError`.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::m10;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const BB: &str = "Ljava/nio/ByteBuffer;";

fn bb_static(vm: &mut Engine, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
    Ok(m10::call_static(vm, BB, name, sig, args)?
        .unwrap_or_else(|| panic!("unhandled ByteBuffer.{name}{sig}")))
}

fn bb_inst(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Value, VmExit> {
    Ok(m10::call_instance(vm, BB, name, sig, recv, args)?
        .unwrap_or_else(|| panic!("unhandled ByteBuffer.{name}{sig}")))
}

fn byte_arr(vm: &mut Engine, vals: &[u8]) -> ObjRef {
    let a = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, vals.len(), "[B")
        .expect("alloc [B");
    let slots = vm.heap.array_elems_mut(a).expect("[B elems");
    for (dst, &v) in slots.iter_mut().zip(vals) {
        *dst = Value::Int(i32::from(v as i8));
    }
    a
}

fn assert_exc(res: Result<Value, VmExit>, expected_desc: &str) {
    match res {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, expected_desc, "unexpected exception descriptor");
        }
        other => panic!("expected exception {expected_desc}, got {other:?}"),
    }
}

#[test]
fn byte_buffer_put_negative_and_overflow_bounds_305() {
    let mut vm = engine_of(&DexBuilder::new());
    let Value::Obj(bb) = bb_static(
        &mut vm,
        "allocate",
        "(I)Ljava/nio/ByteBuffer;",
        &[Value::Int(4)],
    )
    .expect("allocate(4)") else {
        panic!("expected ByteBuffer");
    };
    let one_byte = byte_arr(&mut vm, &[0x41]);

    // Exact #305 class-1 PoC: ByteBuffer.allocate(4).put(new byte[1], 1, -1)
    // previously wrapped `1usize + (-1 as usize) == 0 <= 1` and panicked on host slice!
    let r1 = bb_inst(
        &mut vm,
        bb,
        "put",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(one_byte), Value::Int(1), Value::Int(-1)],
    );
    assert_exc(r1, "Ljava/lang/IndexOutOfBoundsException;");

    for (off, len) in [(-1, 1), (0, -1), (1, i32::MAX), (i32::MAX, 1), (0, 2)] {
        let r = bb_inst(
            &mut vm,
            bb,
            "put",
            "([BII)Ljava/nio/ByteBuffer;",
            &[Value::Obj(one_byte), Value::Int(off), Value::Int(len)],
        );
        assert_exc(r, "Ljava/lang/IndexOutOfBoundsException;");
    }

    // Null array must throw NPE instead of falling into single-byte put(byte).
    let r_null = bb_inst(
        &mut vm,
        bb,
        "put",
        "([B)Ljava/nio/ByteBuffer;",
        &[Value::Null],
    );
    assert_exc(r_null, "Ljava/lang/NullPointerException;");

    // Putting more bytes than remaining throws BufferOverflowException without corrupting pos.
    let five_bytes = byte_arr(&mut vm, &[1, 2, 3, 4, 5]);
    let r_over = bb_inst(
        &mut vm,
        bb,
        "put",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(five_bytes), Value::Int(0), Value::Int(5)],
    );
    assert_exc(r_over, "Ljava/nio/BufferOverflowException;");
    assert_eq!(
        bb_inst(&mut vm, bb, "position", "()I", &[]).expect("position"),
        Value::Int(0)
    );
}

#[test]
fn byte_buffer_wrap_and_get_overflow_bounds_305() {
    let mut vm = engine_of(&DexBuilder::new());
    let backing = byte_arr(&mut vm, &[10, 20, 30, 40]);

    // Exact #305 class-2 PoC on wrap: off = Integer.MAX_VALUE, len = 1 wrapped i32 to MIN.
    for (off, len) in [
        (i32::MAX, 1),
        (1, i32::MAX),
        (-1, 2),
        (2, -1),
        (3, 2),
        (5, 0),
    ] {
        let r = bb_static(
            &mut vm,
            "wrap",
            "([BII)Ljava/nio/ByteBuffer;",
            &[Value::Obj(backing), Value::Int(off), Value::Int(len)],
        );
        assert_exc(r, "Ljava/lang/IndexOutOfBoundsException;");
    }

    let Value::Obj(bb) = bb_static(
        &mut vm,
        "wrap",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(backing), Value::Int(1), Value::Int(2)],
    )
    .expect("valid wrap(backing, 1, 2)") else {
        panic!("expected ByteBuffer");
    };
    assert_eq!(
        bb_inst(&mut vm, bb, "position", "()I", &[]).expect("pos"),
        Value::Int(1)
    );
    assert_eq!(
        bb_inst(&mut vm, bb, "limit", "()I", &[]).expect("limit"),
        Value::Int(3)
    );

    // Exact #305 class-2 PoC on get: off = Integer.MAX_VALUE, len = 1 wrapped i32 to MIN.
    let dst = byte_arr(&mut vm, &[0; 4]);
    for (off, len) in [(i32::MAX, 1), (1, i32::MAX), (-1, 1), (0, -1), (3, 2)] {
        let r = bb_inst(
            &mut vm,
            bb,
            "get",
            "([BII)Ljava/nio/ByteBuffer;",
            &[Value::Obj(dst), Value::Int(off), Value::Int(len)],
        );
        assert_exc(r, "Ljava/lang/IndexOutOfBoundsException;");
    }

    // Null destination array must throw NPE instead of falling into get(int index).
    let r_null = bb_inst(
        &mut vm,
        bb,
        "get",
        "([B)Ljava/nio/ByteBuffer;",
        &[Value::Null],
    );
    assert_exc(r_null, "Ljava/lang/NullPointerException;");

    // Valid in-bounds get reads [20, 30] into dst[1..3] and advances position to 3.
    bb_inst(
        &mut vm,
        bb,
        "get",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(dst), Value::Int(1), Value::Int(2)],
    )
    .expect("valid get");
    let got: Vec<i32> = vm
        .heap
        .as_array_elems(dst)
        .expect("dst elems")
        .0
        .iter()
        .map(|v| v.as_int().expect("int"))
        .collect();
    assert_eq!(got, vec![0, 20, 30, 0]);
    assert_eq!(
        bb_inst(&mut vm, bb, "remaining", "()I", &[]).expect("remaining"),
        Value::Int(0)
    );

    // Reading 1 more byte now throws BufferUnderflowException.
    let r_under = bb_inst(
        &mut vm,
        bb,
        "get",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(dst), Value::Int(0), Value::Int(1)],
    );
    assert_exc(r_under, "Ljava/nio/BufferUnderflowException;");
}

#[test]
fn byte_buffer_allocate_respects_heap_budget_and_self_aliasing_305() {
    let dex = rd_dex::Dex::parse(DexBuilder::new().finish()).expect("DEX");
    let mut vm = Engine::new(
        vec![dex],
        rd_vm::engine::VmConfig {
            heap_budget: 2 * 1024 * 1024,
            ..rd_vm::engine::VmConfig::default()
        },
    );

    // Stage 1: 200_000 bytes exceeds the 2 MiB VM heap_budget (200k * 32B = 6.4 MiB > 2 MiB)
    // without allocating 2 GiB on an unpatched host: unpatched code allocated an
    // unaccounted native Vec<u8> and returned an uncatchable `VmExit::Error(VM_OOM)`
    // instead of a guest-catchable `OutOfMemoryError`.
    let over_budget = bb_static(
        &mut vm,
        "allocate",
        "(I)Ljava/nio/ByteBuffer;",
        &[Value::Int(200_000)],
    );
    assert_exc(over_budget, "Ljava/lang/OutOfMemoryError;");

    // Stage 2: Exact #305 class-3 PoC: ByteBuffer.allocate(Integer.MAX_VALUE) and
    // allocateDirect(Integer.MAX_VALUE) reject immediately via VM heap_budget
    // as a guest-catchable OutOfMemoryError with zero 2 GiB native Vec<u8> allocation.
    for method in ["allocate", "allocateDirect"] {
        let res = bb_static(
            &mut vm,
            method,
            "(I)Ljava/nio/ByteBuffer;",
            &[Value::Int(i32::MAX)],
        );
        assert_exc(res, "Ljava/lang/OutOfMemoryError;");

        let neg = bb_static(
            &mut vm,
            method,
            "(I)Ljava/nio/ByteBuffer;",
            &[Value::Int(-1)],
        );
        assert_exc(neg, "Ljava/lang/IllegalArgumentException;");
    }

    // Valid allocation + self-aliasing put(bb.array(), ...) roundtrip works cleanly.
    let Value::Obj(bb) = bb_static(
        &mut vm,
        "allocate",
        "(I)Ljava/nio/ByteBuffer;",
        &[Value::Int(6)],
    )
    .expect("allocate(6)") else {
        panic!("expected ByteBuffer");
    };
    let src = byte_arr(&mut vm, &[7, 8, 9]);
    bb_inst(
        &mut vm,
        bb,
        "put",
        "([B)Ljava/nio/ByteBuffer;",
        &[Value::Obj(src)],
    )
    .expect("put 3 bytes");
    let Value::Obj(backing) = bb_inst(&mut vm, bb, "array", "()[B", &[]).expect("bb.array()")
    else {
        panic!("expected backing [B");
    };
    bb_inst(
        &mut vm,
        bb,
        "put",
        "([BII)Ljava/nio/ByteBuffer;",
        &[Value::Obj(backing), Value::Int(0), Value::Int(3)],
    )
    .expect("self-aliased put");
    let final_bytes: Vec<i32> = vm
        .heap
        .as_array_elems(backing)
        .expect("backing")
        .0
        .iter()
        .map(|v| v.as_int().expect("int"))
        .collect();
    assert_eq!(final_bytes, vec![7, 8, 9, 7, 8, 9]);
}

#[test]
fn bytecode_byte_buffer_oom_and_put_bounds_catchable_in_guest_305() {
    let mut b = DexBuilder::new();
    let p_bb_i = b.proto_idx("Ljava/nio/ByteBuffer;", vec!["I".into()]);
    let p_bb_bii = b.proto_idx(
        "Ljava/nio/ByteBuffer;",
        vec!["[B".into(), "I".into(), "I".into()],
    );
    let m_alloc = b.method_idx(BB, p_bb_i, "allocate");
    let m_put = b.method_idx(BB, p_bb_bii, "put");
    let b_arr_ty = b.type_idx("[B");
    b.type_idx("Ljava/lang/OutOfMemoryError;");
    b.type_idx("Ljava/lang/IndexOutOfBoundsException;");

    let cls = b.class("LTest305;", "Ljava/lang/Object;");
    // static int run():
    //   try { ByteBuffer.allocate(200_000); return 1; } catch (OutOfMemoryError e) {}
    //   try { ByteBuffer.allocate(4).put(new byte[1], 1, -1); return 2; }
    //   catch (IndexOutOfBoundsException e) { return 77; }
    let mut u = Vec::new();
    // try 1 start = 0
    u.extend(op31t(0x14, 0, 200_000)); // const v0, 200_000 (3 units: 0..3)
    u.extend(op35c(0x71, 1, m_alloc, [0, 0, 0, 0, 0])); // invoke-static {v0}, allocate (3 units: 3..6)
    u.extend(op11n(0x12, 0, 1)); // const/4 v0, 1 (1 unit: 6..7)
    u.extend(op11x(0x0F, 0)); // return v0 (1 unit: 7..8)
                              // catch 1 @ 8:
    u.extend(op11x(0x0D, 0)); // move-exception v0 (1 unit: 8..9)
                              // try 2 start = 9:
    u.extend(op11n(0x12, 0, 4)); // const/4 v0, 4 (1 unit: 9..10)
    u.extend(op35c(0x71, 1, m_alloc, [0, 0, 0, 0, 0])); // invoke-static {v0}, allocate (3 units: 10..13)
    u.extend(op11x(0x0C, 0)); // move-result-object v0 (1 unit: 13..14)
    u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1 (1 unit: 14..15)
    u.extend(op22c(0x23, 2, 1, b_arr_ty)); // new-array v2, v1, [B (2 units: 15..17)
    u.extend(op11n(0x12, 3, -1)); // const/4 v3, -1 (1 unit: 17..18)
    u.extend(op35c(0x6E, 4, m_put, [0, 2, 1, 3, 0])); // invoke-virtual {v0,v2,v1,v3}, put (3 units: 18..21)
    u.extend(op11n(0x12, 0, 2)); // const/4 v0, 2 (1 unit: 21..22)
    u.extend(op11x(0x0F, 0)); // return v0 (1 unit: 22..23)
                              // catch 2 @ 23:
    u.extend(op11x(0x0D, 0)); // move-exception v0 (1 unit: 23..24)
    u.extend(op21s(0x13, 0, 77)); // const/16 v0, 77 (2 units: 24..26)
    u.extend(op11x(0x0F, 0)); // return v0 (1 unit: 26..27)

    let mut code = b.code(4, 0, 4, u);
    code.tries.push(TryBlob {
        start: 0,
        count: 8,
        typed: vec![("Ljava/lang/OutOfMemoryError;".into(), 8)],
        catch_all: None,
    });
    code.tries.push(TryBlob {
        start: 9,
        count: 14,
        typed: vec![("Ljava/lang/IndexOutOfBoundsException;".into(), 23)],
        catch_all: None,
    });
    b.direct(cls, "run", "I", vec![], ACC_PUBLIC | ACC_STATIC, Some(code));

    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX");
    let mut vm = Engine::new(
        vec![dex],
        rd_vm::engine::VmConfig {
            heap_budget: 2 * 1024 * 1024,
            ..rd_vm::engine::VmConfig::default()
        },
    );
    let ret = vm
        .invoke_static("LTest305;", "run", "()I", &[])
        .expect("guest bytecode caught both OOM and IOOBE");
    assert_eq!(ret, Value::Int(77));
}
