//! Regression tests for Issue #313:
//! 1. `VmConfig::default().max_depth` bounded safely for 2 MiB Rust threads AND
//!    dynamic per-entry native stack byte guard (`MAX_NATIVE_STACK_BYTES`) so even
//!    `VmConfig { max_depth: 512, .. }` or heavy DEX<->Host reentrancy throws a
//!    catchable `StackOverflowError` instead of aborting the host with `SIGABRT`.
//! 2. Exceeding stack depth throws guest-catchable `Ljava/lang/StackOverflowError;`
//!    (`VmExit::Exception`) instead of uncatchable `VmExit::Error("VM_STACK_OVERFLOW")`.
//! 3. Host-to-host `Engine::invoke_instance` cycles also track `host_depth` and
//!    unwind cleanly with `StackOverflowError`.

mod common;

use common::*;
use rd_vm::engine::VmConfig;
use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::Engine;

fn build_recursive_dex() -> Vec<u8> {
    let mut b = DexBuilder::new();
    let p_i = b.proto_idx("I", vec![]);
    let p_z_obj = b.proto_idx("Z", vec!["Ljava/lang/Object;".into()]);
    let m_rec = b.method_idx("LRec313;", p_i, "recurse");
    let m_catch = b.method_idx("LRec313;", p_i, "catchRecurse");
    let m_list_eq = b.method_idx("Ljava/util/ArrayList;", p_z_obj, "equals");
    b.type_idx("Ljava/lang/StackOverflowError;");

    let cls = b.class("LRec313;", "Ljava/lang/Object;");
    b.instance_field(cls, "peer", "Ljava/util/ArrayList;");

    // public static int recurse() { return recurse() + 1; }
    let mut u_rec = Vec::new();
    u_rec.extend(op35c(0x71, 0, m_rec, [0, 0, 0, 0, 0])); // invoke-static {}, recurse
    u_rec.extend(op11x(0x0A, 0)); // move-result v0
    u_rec.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
    u_rec.extend(op11x(0x0F, 0)); // return v0
    b.direct(
        cls,
        "recurse",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, u_rec)),
    );

    // public static int catchRecurse() {
    //   try { return recurse(); } catch (StackOverflowError e) { return 42; }
    // }
    let _ = m_catch;
    let mut u_catch = Vec::new();
    u_catch.extend(op35c(0x71, 0, m_rec, [0, 0, 0, 0, 0])); // 0..3
    u_catch.extend(op11x(0x0A, 0)); // 3..4
    u_catch.extend(op11x(0x0F, 0)); // 4..5
                                    // catch @ 5:
    u_catch.extend(op11x(0x0D, 0)); // 5..6 move-exception v0
    u_catch.extend(op21s(0x13, 0, 42)); // 6..8 const/16 v0, 42
    u_catch.extend(op11x(0x0F, 0)); // 8..9 return v0
    let mut c_catch = b.code(1, 0, 0, u_catch);
    c_catch.tries.push(TryBlob {
        start: 0,
        count: 5,
        typed: vec![("Ljava/lang/StackOverflowError;".into(), 5)],
        catch_all: None,
    });
    b.direct(
        cls,
        "catchRecurse",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(c_catch),
    );

    // Heavy DEX<->Host reentrancy: `LRec313.equals(Object)` calls `this.peer.equals(this.peer)`
    // where `peer` is a host `ArrayList` containing two distinct `LRec313` instances.
    let f_peer = b.field_idx("LRec313;", "Ljava/util/ArrayList;", "peer");
    let mut u_eq = Vec::new();
    u_eq.extend(op22c(0x54, 0, 1, f_peer)); // iget-object v0, p0(v1), peer
    u_eq.extend(op35c(0x6E, 2, m_list_eq, [0, 0, 0, 0, 0])); // invoke-virtual {v0, v0} — wait: pass p1's peer so ArrayList doesn't fast-return `recv == other`!
    u_eq.clear();
    // v0..v15 local padding + p0=v16, p1=v17 -> exercises heavy host+DEX stack frames:
    // v0 = p0.peer; v1 = ((LRec313) p1).peer; return v0.equals(v1);
    let rec_ty = b.type_idx("LRec313;");
    u_eq.extend(op22c(0x54, 0, 6, f_peer)); // iget-object v0, p0(v6), peer (2 units: 0..2)
    u_eq.extend(op21c(0x1F, 7, rec_ty)); // check-cast p1(v7), LRec313; (2 units: 2..4)
    u_eq.extend(op22c(0x54, 1, 7, f_peer)); // iget-object v1, p1(v7), peer (2 units: 4..6)
    u_eq.extend(op35c(0x6E, 2, m_list_eq, [0, 1, 0, 0, 0])); // invoke-virtual {v0, v1}, ArrayList.equals (3 units: 6..9)
    u_eq.extend(op11x(0x0A, 0)); // move-result v0 (1 unit: 9..10)
    u_eq.extend(op11x(0x0F, 0)); // return v0 (1 unit: 10..11)
    b.r#virtual(
        cls,
        "equals",
        "Z",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(8, 2, 2, u_eq)),
    );

    b.finish()
}

#[test]
fn stack_overflow_is_catchable_java_error_in_guest_313() {
    let dex_bytes = build_recursive_dex();

    // Stage 1: With small max_depth=16, unpatched code returned uncatchable
    // `VmExit::Error("VM_STACK_OVERFLOW")` instead of catching `StackOverflowError`.
    let dex = rd_dex::Dex::parse(dex_bytes.clone()).expect("DEX");
    let mut vm16 = Engine::new(
        vec![dex],
        VmConfig {
            max_depth: 16,
            ..VmConfig::default()
        },
    );
    let caught16 = vm16
        .invoke_static("LRec313;", "catchRecurse", "()I", &[])
        .expect("guest catch (StackOverflowError) must succeed");
    assert_eq!(caught16, Value::Int(42));

    match vm16.invoke_static("LRec313;", "recurse", "()I", &[]) {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, "Ljava/lang/StackOverflowError;");
        }
        other => panic!("expected uncaught StackOverflowError exception, got {other:?}"),
    }

    // Stage 2: Default VmConfig inside a strict 2 MiB thread (`std::thread::Builder`)
    // catches StackOverflowError cleanly without overflowing the OS thread stack.
    let res = std::thread::Builder::new()
        .name("vm-2mib-default-313".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let dex = rd_dex::Dex::parse(dex_bytes).expect("DEX");
            let mut vm = Engine::new(vec![dex], VmConfig::default());
            let v = vm
                .invoke_static("LRec313;", "catchRecurse", "()I", &[])
                .expect("catchRecurse on 2 MiB thread");
            assert_eq!(v, Value::Int(42));
            // Engine remains healthy for subsequent invocations after unwinding.
            let v2 = vm
                .invoke_static("LRec313;", "catchRecurse", "()I", &[])
                .expect("second catchRecurse on 2 MiB thread");
            assert_eq!(v2, Value::Int(42));
        })
        .expect("spawn 2 MiB thread")
        .join();
    assert!(res.is_ok(), "2 MiB thread panicked");
}

#[test]
fn explicit_max_depth_512_on_2mib_thread_trips_native_stack_budget_without_sigabrt_313() {
    // Stage 1: Verify VmConfig::default().max_depth was lowered from 512 to a safe ceiling
    // (unpatched code fails this assertion immediately without SIGABRT-killing the test binary).
    assert!(
        VmConfig::default().max_depth <= 128,
        "VmConfig::default().max_depth={} exceeds safe 2 MiB thread ceiling",
        VmConfig::default().max_depth
    );

    // Stage 2: Even when a caller explicitly configures `max_depth: 512` on a 2 MiB thread
    // and triggers heavy DEX <-> Host (`ArrayList.equals` -> `LRec313.equals` -> `ArrayList.equals`)
    // mutual reentrancy, `MAX_NATIVE_STACK_BYTES` fires before the 2 MiB OS guard page and
    // unwinds cleanly with `Ljava/lang/StackOverflowError;`.
    let dex_bytes = build_recursive_dex();
    let res = std::thread::Builder::new()
        .name("vm-2mib-512-reentrant-313".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let dex = rd_dex::Dex::parse(dex_bytes).expect("DEX");
            let mut vm = Engine::new(
                vec![dex],
                VmConfig {
                    max_depth: 512,
                    ..VmConfig::default()
                },
            );
            let list_a = vm
                .heap
                .alloc_instance("Ljava/util/ArrayList;".into(), Vec::new())
                .expect("list_a");
            vm.invoke_instance(list_a, "<init>", "()V", &[])
                .expect("init list_a");
            let list_b = vm
                .heap
                .alloc_instance("Ljava/util/ArrayList;".into(), Vec::new())
                .expect("list_b");
            vm.invoke_instance(list_b, "<init>", "()V", &[])
                .expect("init list_b");

            let node_a = vm
                .heap
                .alloc_instance("LRec313;".into(), vec![("peer".into(), Value::Obj(list_a))])
                .expect("node_a");
            let node_b = vm
                .heap
                .alloc_instance("LRec313;".into(), vec![("peer".into(), Value::Obj(list_b))])
                .expect("node_b");
            vm.invoke_instance(
                list_a,
                "add",
                "(Ljava/lang/Object;)Z",
                &[Value::Obj(node_a)],
            )
            .expect("list_a.add");
            vm.invoke_instance(
                list_b,
                "add",
                "(Ljava/lang/Object;)Z",
                &[Value::Obj(node_b)],
            )
            .expect("list_b.add");

            let r = vm.invoke_instance(
                list_a,
                "equals",
                "(Ljava/lang/Object;)Z",
                &[Value::Obj(list_b)],
            );
            match r {
                Err(VmExit::Exception(ex)) => {
                    assert_eq!(ex.class, "Ljava/lang/StackOverflowError;");
                }
                other => panic!("expected StackOverflowError on DEX<->Host cycle, got {other:?}"),
            }
        })
        .expect("spawn 2 MiB thread")
        .join();
    assert!(res.is_ok(), "2 MiB reentrant thread panicked");
}

#[test]
fn host_to_host_mutual_collection_cycle_throws_soe_and_recovers_313() {
    // Guard against unpatched SIGABRT during Before verification:
    assert!(
        VmConfig::default().max_depth <= 128,
        "unpatched max_depth lacks host_depth guard"
    );

    let res = std::thread::Builder::new()
        .name("vm-2mib-host-cycle-313".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut vm = engine_of(&DexBuilder::new());
            let b1 = vm
                .heap
                .alloc_instance("Landroid/util/Base64InputStream;".into(), Vec::new())
                .expect("b1");
            let b2 = vm
                .heap
                .alloc_instance("Landroid/util/Base64InputStream;".into(), Vec::new())
                .expect("b2");
            vm.invoke_instance(
                b1,
                "<init>",
                "(Ljava/io/InputStream;I)V",
                &[Value::Obj(b2), Value::Int(0)],
            )
            .expect("init b1(b2)");
            vm.invoke_instance(
                b2,
                "<init>",
                "(Ljava/io/InputStream;I)V",
                &[Value::Obj(b1), Value::Int(0)],
            )
            .expect("init b2(b1)");

            // Pure host-to-host `invoke_instance` cycle (`b1.available()` -> `b2.available()` -> ...)
            let av = vm.invoke_instance(b1, "available", "()I", &[]);
            match av {
                Err(VmExit::Exception(ex)) => {
                    assert_eq!(ex.class, "Ljava/lang/StackOverflowError;");
                }
                other => panic!("expected StackOverflowError on host cycle, got {other:?}"),
            }

            // Engine state cleanly unwinds host_depth & entry_sp back to 0.
            let list = vm
                .heap
                .alloc_instance("Ljava/util/ArrayList;".into(), Vec::new())
                .expect("list");
            vm.invoke_instance(list, "<init>", "()V", &[])
                .expect("init list");
            let sz = vm
                .invoke_instance(list, "size", "()I", &[])
                .expect("size() after recovery");
            assert_eq!(sz, Value::Int(0));
        })
        .expect("spawn 2 MiB thread")
        .join();
    assert!(res.is_ok(), "host-to-host cycle thread panicked");
}
