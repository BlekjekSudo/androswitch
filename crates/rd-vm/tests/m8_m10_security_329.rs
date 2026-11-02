//! Regression tests for Issue #329:
//! 1. `AudioTrack.write` (`byte[]`, `short[]`, and `ByteBuffer` with out-of-bounds
//!    `rd_pos` / `rd_limit`) returns `ERROR_BAD_VALUE (-2)` instead of panicking.
//! 2. `AudioRecord.read(ByteBuffer, Integer.MAX_VALUE)` caps by capacity without
//!    2 GiB host `Vec<u8>` allocation.
//! 3. `m8` `SQLiteDatabase.execSQL("CREATE TABLE )t(")` (`RD_SQLITE=subset`) does
//!    not panic on inverted parentheses in `parse_create`.
//! 4. `m10::State` virtual-device maps (`wake_locks`, `audio_tracks`, `audio_records`,
//!    `media_players`) act as GC ephemerons and purge dead entries on `Engine::gc()`,
//!    while active sensor/listener registrations survive GC.
//! 5. `FragmentTransaction.replace(containerId, frag2)` removes prior fragments in
//!    `containerId` upon `commit` / `commitNow`.

mod common;

use common::*;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;

#[cfg(feature = "virtual-devices")]
#[test]
fn audio_track_write_and_audio_record_read_bounds_do_not_panic_or_oom_329() {
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

    let byte4 = vm
        .heap
        .alloc_array(ElemKind::Int, 4, "[B")
        .expect("byte[4]");
    let short4 = vm
        .heap
        .alloc_array(ElemKind::Int, 4, "[S")
        .expect("short[4]");

    // byte[] and short[] out-of-bounds / negative ranges return ERROR_BAD_VALUE (-2).
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioTrack;",
            "write",
            "([BII)I",
            trk,
            &[Value::Obj(byte4), Value::Int(10), Value::Int(2)],
        )
        .expect("write byte[4] off=10"),
        Some(Value::Int(-2))
    );
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioTrack;",
            "write",
            "([BII)I",
            trk,
            &[Value::Obj(byte4), Value::Int(-1), Value::Int(2)],
        )
        .expect("write byte[4] off=-1"),
        Some(Value::Int(-2))
    );
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioTrack;",
            "write",
            "([SII)I",
            trk,
            &[Value::Obj(short4), Value::Int(3), Value::Int(4)],
        )
        .expect("write short[4] 3..7"),
        Some(Value::Int(-2))
    );

    // ByteBuffer with corrupted/out-of-bounds rd_pos returns ERROR_BAD_VALUE (-2) without slice panic.
    let bb = match rd_vm::m10::call_static(
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
    for bad_pos in [-1, 10] {
        vm.heap
            .put_field(bb, "rd_pos", Value::Int(bad_pos))
            .expect("set rd_pos");
        assert_eq!(
            rd_vm::m10::call_instance(
                &mut vm,
                "Landroid/media/AudioTrack;",
                "write",
                "(Ljava/nio/ByteBuffer;I)I",
                trk,
                &[Value::Obj(bb), Value::Int(2)],
            )
            .expect("write ByteBuffer bad rd_pos"),
            Some(Value::Int(-2)),
            "bad_pos={bad_pos}"
        );
    }

    // AudioRecord.read(ByteBuffer.allocate(4), MAX_VALUE) completes in O(cap) without 2 GiB OOM.
    vm.heap
        .put_field(bb, "rd_pos", Value::Int(0))
        .expect("reset rd_pos");
    vm.heap
        .put_field(bb, "rd_limit", Value::Int(4))
        .expect("reset rd_limit");
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
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioRecord;",
            "read",
            "(Ljava/nio/ByteBuffer;I)I",
            rec,
            &[Value::Obj(bb), Value::Int(-1)],
        )
        .expect("read(-1)"),
        Some(Value::Int(-2))
    );
    assert_eq!(
        rd_vm::m10::call_instance(
            &mut vm,
            "Landroid/media/AudioRecord;",
            "read",
            "(Ljava/nio/ByteBuffer;I)I",
            rec,
            &[Value::Obj(bb), Value::Int(i32::MAX)],
        )
        .expect("read(MAX_VALUE)"),
        Some(Value::Int(4))
    );
}

#[test]
fn m8_subset_parse_create_inverted_parens_does_not_panic_329() {
    std::env::set_var("RD_SQLITE", "subset");
    let mut vm = engine_of(&DexBuilder::new());
    let db = vm
        .heap
        .alloc_instance(
            "Landroid/database/sqlite/SQLiteDatabase;".to_string(),
            Vec::new(),
        )
        .expect("alloc m8 SQLiteDatabase");
    vm.fw.m8.db_by_obj.insert(db, ":memory:".to_string());
    for malformed in [
        "CREATE TABLE )t(",
        "CREATE TABLE ()",
        "CREATE TABLE )(id INT)",
    ] {
        let s = Value::Obj(vm.heap.alloc_string(malformed.to_string()).expect("str"));
        let res = rd_vm::m8::call_m8_instance(
            &mut vm,
            "Landroid/database/sqlite/SQLiteDatabase;",
            "execSQL",
            "(Ljava/lang/String;)V",
            db,
            &[s],
        )
        .expect("subset execSQL must not panic");
        assert_eq!(res, Some(Value::Null));
    }
}

#[cfg(feature = "virtual-devices")]
#[test]
fn m10_state_reclaims_unreachable_peripherals_on_gc_329() {
    let mut vm = engine_of(&DexBuilder::new());
    vm.m10_enable_virtual_devices();

    let pm = vm
        .heap
        .alloc_instance("Landroid/os/PowerManager;".to_string(), Vec::new())
        .expect("alloc PowerManager");
    let tag = Value::Obj(vm.heap.alloc_string("rd:leak329".to_string()).expect("tag"));
    let wl = match rd_vm::m10::call_instance(
        &mut vm,
        "Landroid/os/PowerManager;",
        "newWakeLock",
        "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
        pm,
        &[Value::Int(1), tag],
    )
    .expect("newWakeLock")
    {
        Some(Value::Obj(w)) => w,
        other => panic!("expected WakeLock, got {other:?}"),
    };

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

    assert!(vm.fw.vdev.wake_locks.contains_key(&wl));
    assert!(vm.fw.vdev.audio_tracks.contains_key(&trk));
    assert!(vm.fw.vdev.audio_records.contains_key(&rec));

    // With no stack/static/pinned references holding `wl`, `trk`, or `rec`,
    // `Engine::gc()` must reclaim all three slots and purge `vm.fw.vdev` maps.
    let _ = vm.gc();
    assert_eq!(vm.heap.class_of(wl).ok(), Some(rd_vm::gc::FREED_CLASS));
    assert_eq!(vm.heap.class_of(trk).ok(), Some(rd_vm::gc::FREED_CLASS));
    assert_eq!(vm.heap.class_of(rec).ok(), Some(rd_vm::gc::FREED_CLASS));
    assert!(
        !vm.fw.vdev.wake_locks.contains_key(&wl),
        "unreachable WakeLock remained pinned in m10::State::wake_locks"
    );
    assert!(
        !vm.fw.vdev.audio_tracks.contains_key(&trk),
        "unreachable AudioTrack remained pinned in m10::State::audio_tracks"
    );
    assert!(
        !vm.fw.vdev.audio_records.contains_key(&rec),
        "unreachable AudioRecord remained pinned in m10::State::audio_records"
    );
}

#[test]
fn fragment_transaction_replace_replaces_existing_container_fragment_329() {
    let mut vm = engine_of(&DexBuilder::new());
    let act = vm
        .heap
        .alloc_instance(
            "Landroidx/fragment/app/FragmentActivity;".to_string(),
            Vec::new(),
        )
        .expect("act");
    let fm = match rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentActivity;",
        "getSupportFragmentManager",
        "()Landroidx/fragment/app/FragmentManager;",
        act,
        &[],
    )
    .expect("fm")
    {
        Some(Value::Obj(f)) => f,
        other => panic!("expected fm, got {other:?}"),
    };

    let frag1 = vm
        .heap
        .alloc_instance("Landroidx/fragment/app/Fragment;".to_string(), Vec::new())
        .expect("frag1");
    let frag2 = vm
        .heap
        .alloc_instance("Landroidx/fragment/app/Fragment;".to_string(), Vec::new())
        .expect("frag2");
    let tag1 = Value::Obj(vm.heap.alloc_string("first".to_string()).expect("tag1"));
    let tag2 = Value::Obj(vm.heap.alloc_string("second".to_string()).expect("tag2"));

    let tx1 = match rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentManager;",
        "beginTransaction",
        "()Landroidx/fragment/app/FragmentTransaction;",
        fm,
        &[],
    )
    .expect("tx1")
    {
        Some(Value::Obj(t)) => t,
        other => panic!("expected tx1, got {other:?}"),
    };
    rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentTransaction;",
        "add",
        "(ILandroidx/fragment/app/Fragment;Ljava/lang/String;)Landroidx/fragment/app/FragmentTransaction;",
        tx1,
        &[Value::Int(0x7f08_0010), Value::Obj(frag1), tag1],
    )
    .expect("add frag1");
    rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentTransaction;",
        "commitNow",
        "()V",
        tx1,
        &[],
    )
    .expect("commitNow tx1");

    let tx2 = match rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentManager;",
        "beginTransaction",
        "()Landroidx/fragment/app/FragmentTransaction;",
        fm,
        &[],
    )
    .expect("tx2")
    {
        Some(Value::Obj(t)) => t,
        other => panic!("expected tx2, got {other:?}"),
    };
    rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentTransaction;",
        "replace",
        "(ILandroidx/fragment/app/Fragment;Ljava/lang/String;)Landroidx/fragment/app/FragmentTransaction;",
        tx2,
        &[Value::Int(0x7f08_0010), Value::Obj(frag2), tag2],
    )
    .expect("replace frag2");
    rd_vm::m8::call_m8_instance(
        &mut vm,
        "Landroidx/fragment/app/FragmentTransaction;",
        "commitNow",
        "()V",
        tx2,
        &[],
    )
    .expect("commitNow tx2");

    // `frag1` must have been removed from container `0x7f08_0010` and tag lookup.
    assert_eq!(
        rd_vm::m8::call_m8_instance(
            &mut vm,
            "Landroidx/fragment/app/FragmentManager;",
            "findFragmentById",
            "(I)Landroidx/fragment/app/Fragment;",
            fm,
            &[Value::Int(0x7f08_0010)],
        )
        .expect("findFragmentById"),
        Some(Value::Obj(frag2))
    );
    let q_first = Value::Obj(vm.heap.alloc_string("first".to_string()).expect("q_first"));
    assert_eq!(
        rd_vm::m8::call_m8_instance(
            &mut vm,
            "Landroidx/fragment/app/FragmentManager;",
            "findFragmentByTag",
            "(Ljava/lang/String;)Landroidx/fragment/app/Fragment;",
            fm,
            &[q_first],
        )
        .expect("findFragmentByTag(first)"),
        Some(Value::Null)
    );
}
