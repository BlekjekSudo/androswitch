//! Regression tests for Issue #311 (remaining host-allocation & bounds sites on `main`):
//! 1. Item 1 (`jsb.rs`): `StringBuilder.setLength` / `StringBuffer.setLength`
//!    validates `heap_budget` before `Vec<u16>::resize` and throws guest-catchable
//!    `OutOfMemoryError` (+ `NegativeArraySizeException` on `<init>(-1)` and
//!    overflow-safe `[C` offset+len checks).
//! 2. Item 4 (`jsqlite.rs`): `SQLiteProgram.bind*` validates `1 <= index <= numParameters`
//!    (throwing `IllegalArgumentException` on `<= 0`, `> numParameters`, and
//!    `Integer.MAX_VALUE` without 51 GiB `Vec<Sv>::resize`).
//! 3. Item 8 (`m10/media.rs`): `AudioRecord.read(ByteBuffer, sizeInBytes)` caps
//!    allocation by `ByteBuffer` capacity (no 2.14 GiB `vec![0u8; req_bytes]`),
//!    and `AudioTrack.write` rejects negative / out-of-bounds `[B` / `ByteBuffer`
//!    ranges with `ERROR_BAD_VALUE (-2)` instead of panicking on slice indexing.

mod common;

use common::*;
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;

const SB: &str = "Ljava/lang/StringBuilder;";
const SBUF: &str = "Ljava/lang/StringBuffer;";
const SQL_DB: &str = "Landroid/database/sqlite/SQLiteDatabase;";

#[test]
fn string_builder_set_length_enforces_heap_budget_and_throws_catchable_oome_311() {
    let dex = rd_dex::Dex::parse(DexBuilder::new().finish()).expect("dex");
    let cfg = VmConfig {
        heap_budget: 2 * 1024 * 1024, // 2 MiB budget (keeps unpatched Before RAM <= 6 MiB)
        ..VmConfig::default()
    };
    let mut vm = Engine::new(vec![dex], cfg);

    let sb = vm
        .heap
        .alloc_instance(SB.to_string(), Vec::new())
        .expect("alloc StringBuilder");
    vm.invoke_instance(sb, "<init>", "()V", &[])
        .expect("sb.<init>");

    // Stage 1 (RAM-safe in unpatched Before: ~6 MiB): must throw guest-catchable
    // `OutOfMemoryError` BEFORE `Vec<u16>::resize`, not uncatchable `Error(VM_OOM)`.
    match vm.invoke_instance(sb, "setLength", "(I)V", &[Value::Int(3_000_000)]) {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, "Ljava/lang/OutOfMemoryError;");
        }
        other => panic!("expected catchable OutOfMemoryError on 3M setLength, got {other:?}"),
    }

    // Stage 2: `Integer.MAX_VALUE` on both `StringBuilder` and `StringBuffer`
    // rejects immediately in O(1) without attempting 4.29 GiB host allocation.
    for cls in [SB, SBUF] {
        let obj = vm
            .heap
            .alloc_instance(cls.to_string(), Vec::new())
            .expect("alloc builder");
        vm.invoke_instance(obj, "<init>", "()V", &[])
            .expect("<init>");
        match vm.invoke_instance(obj, "setLength", "(I)V", &[Value::Int(i32::MAX)]) {
            Err(VmExit::Exception(ex)) => {
                assert_eq!(ex.class, "Ljava/lang/OutOfMemoryError;");
            }
            other => panic!("expected OutOfMemoryError for {cls}.setLength(MAX), got {other:?}"),
        }
    }

    // Negative capacity constructor & overflow `[C` ranges fail with Java exceptions.
    let sb_neg = vm
        .heap
        .alloc_instance(SB.to_string(), Vec::new())
        .expect("sb_neg");
    match vm.invoke_instance(sb_neg, "<init>", "(I)V", &[Value::Int(-1)]) {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, "Ljava/lang/NegativeArraySizeException;");
        }
        other => panic!("expected NegativeArraySizeException, got {other:?}"),
    }
    let carr = vm
        .heap
        .alloc_array(ElemKind::Int, 4, "[C")
        .expect("alloc char[4]");
    match vm.invoke_instance(
        sb,
        "append",
        "([CII)Ljava/lang/StringBuilder;",
        &[Value::Obj(carr), Value::Int(1), Value::Int(i32::MAX)],
    ) {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, "Ljava/lang/IndexOutOfBoundsException;");
        }
        other => panic!("expected IndexOutOfBoundsException on overflow range, got {other:?}"),
    }
}

#[test]
fn sqlite_program_bind_validates_parameter_range_without_gib_resize_311() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = match vm
        .invoke_static(
            SQL_DB,
            "create",
            "(Landroid/database/sqlite/SQLiteDatabase$CursorFactory;)Landroid/database/sqlite/SQLiteDatabase;",
            &[Value::Null],
        )
        .expect("SQLiteDatabase.create")
    {
        Value::Obj(r) => r,
        other => panic!("expected SQLiteDatabase, got {other:?}"),
    };
    let ddl = Value::Obj(
        vm.heap
            .alloc_string("CREATE TABLE kv (k INTEGER, v TEXT, note TEXT)".to_string())
            .expect("str"),
    );
    vm.invoke_instance(db, "execSQL", "(Ljava/lang/String;)V", &[ddl])
        .expect("create kv");

    // Quoted `'lit?not_a_param'` and `-- ?comment` must NOT inflate parameter count (only 2 `?`).
    let sql = Value::Obj(
        vm.heap
            .alloc_string("INSERT INTO kv VALUES (?, ?, 'lit?not_a_param') -- ?comment".to_string())
            .expect("sql str"),
    );
    let stmt = match vm
        .invoke_instance(
            db,
            "compileStatement",
            "(Ljava/lang/String;)Landroid/database/sqlite/SQLiteStatement;",
            &[sql],
        )
        .expect("compileStatement")
    {
        Value::Obj(s) => s,
        other => panic!("expected SQLiteStatement, got {other:?}"),
    };

    // Stage 1 (0 MB RAM in unpatched Before): index <= 0 and index == 3 (> 2 params)
    // must throw `IllegalArgumentException` instead of clamping to 1 or resizing.
    for bad_idx in [0, -5, 3] {
        match vm.invoke_instance(
            stmt,
            "bindLong",
            "(IJ)V",
            &[Value::Int(bad_idx), Value::Long(99)],
        ) {
            Err(VmExit::Exception(ex)) => {
                assert_eq!(
                    ex.class, "Ljava/lang/IllegalArgumentException;",
                    "bad_idx={bad_idx}"
                );
            }
            other => panic!("expected IllegalArgumentException for index {bad_idx}, got {other:?}"),
        }
    }

    // Stage 2: `Integer.MAX_VALUE` rejects in O(1) without 51 GiB `Vec<Sv>::resize`.
    match vm.invoke_instance(
        stmt,
        "bindLong",
        "(IJ)V",
        &[Value::Int(i32::MAX), Value::Long(99)],
    ) {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, "Ljava/lang/IllegalArgumentException;");
        }
        other => panic!("expected IllegalArgumentException for MAX_VALUE, got {other:?}"),
    }

    // Valid 1-based binds (1 and 2) succeed and insert cleanly.
    vm.invoke_instance(stmt, "bindLong", "(IJ)V", &[Value::Int(1), Value::Long(42)])
        .expect("bind 1");
    let v_str = Value::Obj(vm.heap.alloc_string("ok".to_string()).expect("ok"));
    vm.invoke_instance(
        stmt,
        "bindString",
        "(ILjava/lang/String;)V",
        &[Value::Int(2), v_str],
    )
    .expect("bind 2");
    assert_eq!(
        vm.invoke_instance(stmt, "executeInsert", "()J", &[])
            .expect("executeInsert"),
        Value::Long(1)
    );
}

#[cfg(feature = "virtual-devices")]
#[test]
fn audio_record_read_bytebuffer_caps_to_capacity_without_gib_vec_311() {
    let mut vm = engine_of(&DexBuilder::new());
    let rec = vm
        .heap
        .alloc_instance("Landroid/media/AudioRecord;".to_string(), Vec::new())
        .expect("alloc AudioRecord");
    rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/media/AudioRecord;",
        "<init>",
        "(IIIII)V",
        rec,
        &[
            Value::Int(1),
            Value::Int(16_000),
            Value::Int(16),
            Value::Int(2),
            Value::Int(1024),
        ],
    )
    .expect("AudioRecord.<init>");
    rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/media/AudioRecord;",
        "startRecording",
        "()V",
        rec,
        &[],
    )
    .expect("startRecording");

    let buf4 = match rd_vm::m10::call_static(
        &mut vm,
        "Ljava/nio/ByteBuffer;",
        "allocate",
        "(I)Ljava/nio/ByteBuffer;",
        &[Value::Int(4)],
    )
    .expect("ByteBuffer.allocate(4)")
    {
        Some(Value::Obj(b)) => b,
        other => panic!("expected ByteBuffer, got {other:?}"),
    };

    // Stage 1 (0 MB RAM in unpatched Before): negative `sizeInBytes` must return
    // `ERROR_BAD_VALUE (-2)` instead of `.max(0)` -> `0`.
    let neg_res = rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/media/AudioRecord;",
        "read",
        "(Ljava/nio/ByteBuffer;I)I",
        rec,
        &[Value::Obj(buf4), Value::Int(-4)],
    )
    .expect("read(-4)");
    assert_eq!(
        neg_res,
        Some(Value::Int(-2)),
        "negative sizeInBytes must return ERROR_BAD_VALUE (-2)"
    );

    // Stage 2: `sizeInBytes = Integer.MAX_VALUE` on a 4-byte ByteBuffer caps to 4 bytes
    // with zero 2.14 GiB host `vec![0u8; req_bytes]` allocation.
    vm.fw
        .vdev
        .audio
        .pcm_input_queue
        .extend_from_slice(&[0x12, 0x34]);
    let ok_res = rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/media/AudioRecord;",
        "read",
        "(Ljava/nio/ByteBuffer;I)I",
        rec,
        &[Value::Obj(buf4), Value::Int(i32::MAX)],
    )
    .expect("read(MAX_VALUE)");
    assert_eq!(ok_res, Some(Value::Int(4)));
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Ljava/nio/ByteBuffer;",
            "get",
            "(I)B",
            buf4,
            &[Value::Int(0)]
        )
        .expect("get(0)"),
        Some(Value::Int(0x12))
    );
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Ljava/nio/ByteBuffer;",
            "get",
            "(I)B",
            buf4,
            &[Value::Int(1)]
        )
        .expect("get(1)"),
        Some(Value::Int(0x34))
    );
}

#[cfg(feature = "virtual-devices")]
#[test]
fn audio_track_write_out_of_bounds_returns_bad_value_without_slice_panic_311() {
    let mut vm = engine_of(&DexBuilder::new());
    let trk = vm
        .heap
        .alloc_instance("Landroid/media/AudioTrack;".to_string(), Vec::new())
        .expect("alloc AudioTrack");
    rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/media/AudioTrack;",
        "<init>",
        "(IIIIII)V",
        trk,
        &[
            Value::Int(3),
            Value::Int(44_100),
            Value::Int(4),
            Value::Int(2),
            Value::Int(4096),
            Value::Int(1),
        ],
    )
    .expect("AudioTrack.<init>");

    let pcm = vm
        .heap
        .alloc_array(ElemKind::Int, 4, "[B")
        .expect("alloc byte[4]");

    // On unpatched `main`, `off = 10 > bytes.len() (4)` panicked in `bytes[10..4]`!
    for (off, count) in [(10, 2), (2, i32::MAX), (-1, 2), (0, -2)] {
        let res = rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioTrack;",
            "write",
            "([BII)I",
            trk,
            &[Value::Obj(pcm), Value::Int(off), Value::Int(count)],
        )
        .expect("AudioTrack.write");
        assert_eq!(
            res,
            Some(Value::Int(-2)),
            "expected ERROR_BAD_VALUE (-2) for off={off}, count={count}"
        );
    }
}
