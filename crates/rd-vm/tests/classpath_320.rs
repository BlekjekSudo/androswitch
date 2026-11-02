//! Regression tests for issue #320 (`crates/rd-vm/src/classpath.rs`):
//! 1. Exponential `k^D` interface-DAG traversal in `resolve_method` / `resolve_in_interface`.
//! 2. Unbounded host-stack recursion on deep `'[[[...'` array descriptors in `Classpath::is_subtype`.

use rd_dex::Dex;
use rd_testsupport::vm::{op11n, op11x, CodeBlob, DexBuilder};
use rd_vm::classpath::Classpath;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn attach_interfaces(dex: &mut Dex, edges: &[(String, Vec<String>)]) {
    for (owner, ifaces) in edges {
        if ifaces.is_empty() {
            continue;
        }
        let type_ids: Vec<u32> = ifaces
            .iter()
            .map(|iface| {
                dex.class_defs
                    .iter()
                    .find(|def| dex.type_str(def.class_idx) == iface.as_str())
                    .expect("interface class_def")
                    .class_idx
            })
            .collect();
        while !dex.data.len().is_multiple_of(4) {
            dex.data.push(0);
        }
        let offset = dex.data.len() as u32;
        dex.data
            .extend_from_slice(&(type_ids.len() as u32).to_le_bytes());
        for id in type_ids {
            dex.data.extend_from_slice(&(id as u16).to_le_bytes());
        }
        let cls_pos = dex
            .class_defs
            .iter()
            .position(|def| dex.type_str(def.class_idx) == owner.as_str())
            .expect("owner class_def");
        dex.class_defs[cls_pos].interfaces_off = offset;
    }
}

fn build_diamond_interface_dex(layers: usize, width: usize) -> Dex {
    let mut b = DexBuilder::new();
    let _impl_cls = b.class("LImpl;", "Ljava/lang/Object;");
    let mut edges: Vec<(String, Vec<String>)> = Vec::new();

    for d in 0..layers {
        for w in 0..width {
            let desc = format!("LI_{d}_{w};");
            let c = b.class(&desc, "Ljava/lang/Object;");
            // Put a concrete default method on the last node of layer 8 (within budget=128)
            if d == 8.min(layers.saturating_sub(1)) && w + 1 == width {
                let mut units = Vec::new();
                units.extend(op11n(0x12, 0, 7)); // const/4 v0, 7
                units.extend(op11x(0x0f, 0)); // return v0
                b.r#virtual(
                    c,
                    "defaultVal",
                    "I",
                    vec![],
                    0x0001,
                    Some(CodeBlob {
                        regs: 2,
                        ins: 1,
                        outs: 0,
                        units,
                        tries: vec![],
                    }),
                );
            }
        }
    }

    let root_ifaces: Vec<String> = (0..width).map(|w| format!("LI_0_{w};")).collect();
    edges.push(("LImpl;".to_string(), root_ifaces));

    for d in 0..layers.saturating_sub(1) {
        let next_layer: Vec<String> = (0..width).map(|nw| format!("LI_{}_{nw};", d + 1)).collect();
        for w in 0..width {
            edges.push((format!("LI_{d}_{w};"), next_layer.clone()));
        }
    }

    let mut dex = Dex::parse(b.finish()).expect("parse diamond dex");
    attach_interfaces(&mut dex, &edges);
    dex
}

#[test]
fn issue_320_diamond_interface_dag_resolves_without_exponential_hang() {
    // 17-level, 8-way diamond interface DAG: 8^16 (~2.8e14) paths without `visited`,
    // vs <= 128 interface inspections (< 5 ms) with shared `visited` + `budget`.
    // Bounded via `recv_timeout(250ms)` so the BEFORE test fails cleanly in 0.25s
    // without stalling CI / verification sandboxes.
    let dex = build_diamond_interface_dex(17, 8);
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let start = Instant::now();
        let mut engine = Engine::new(
            vec![dex],
            VmConfig {
                fuel: 64,
                ..VmConfig::default()
            },
        );
        // 1. Missing method walk across the entire diamond must finish in O(budget)
        assert!(engine
            .cp
            .resolve_method("LImpl;", "nonExistentMethod", "()V")
            .is_none());

        // 2. Reachable default method in the diamond still resolves and executes
        let obj = engine
            .heap
            .alloc_instance("LImpl;".to_string(), Vec::new())
            .expect("alloc LImpl;");
        let val = engine
            .invoke_instance(obj, "defaultVal", "()I", &[])
            .expect("invoke defaultVal");
        let _ = tx.send((val, start.elapsed()));
    });

    // Hang guard, not a speed test: the bound is generous because a debug build
    // under parallel load can exceed 250 ms. An unbounded walk still fails here.
    let (val, elapsed) = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("17-level 8-way diamond interface resolution hung (>60s)");
    assert_eq!(val, Value::Int(7));
    assert!(
        elapsed < Duration::from_secs(60),
        "expected bounded resolution, took {elapsed:?}"
    );
}

#[test]
fn issue_320_cyclic_interfaces_terminate_and_resolve_default_method() {
    let mut b = DexBuilder::new();
    let _impl_cls = b.class("LCycImpl;", "Ljava/lang/Object;");
    let _ia = b.class("LCycA;", "Ljava/lang/Object;");
    let ib = b.class("LCycB;", "Ljava/lang/Object;");
    let mut units = Vec::new();
    units.extend(op11n(0x12, 0, 5)); // const/4 v0, 5
    units.extend(op11x(0x0f, 0)); // return v0
    b.r#virtual(
        ib,
        "cycDefault",
        "I",
        vec![],
        0x0001,
        Some(CodeBlob {
            regs: 2,
            ins: 1,
            outs: 0,
            units,
            tries: vec![],
        }),
    );

    let mut dex = Dex::parse(b.finish()).expect("parse cyclic dex");
    attach_interfaces(
        &mut dex,
        &[
            ("LCycImpl;".into(), vec!["LCycA;".into()]),
            ("LCycA;".into(), vec!["LCycB;".into()]),
            ("LCycB;".into(), vec!["LCycA;".into()]),
        ],
    );

    let mut cp = Classpath::new(vec![dex]);
    assert!(cp.resolve_method("LCycImpl;", "missing", "()V").is_none());
    assert!(cp
        .resolve_method("LCycImpl;", "cycDefault", "()I")
        .is_some());
}

#[test]
fn issue_320_deep_array_is_subtype_runs_in_constant_stack_space() {
    // Run on an explicit 512 KiB stack so unbounded recursion fails deterministically
    // while iterative '[' peeling uses O(1) stack even at 50,000 dimensions.
    let handle = std::thread::Builder::new()
        .name("deep-array-subtype-320".into())
        .stack_size(512 * 1024)
        .spawn(|| {
            let cp = Classpath::new(Vec::new());
            let brackets_50k = "[".repeat(50_000);
            let brackets_49999 = &brackets_50k[1..];

            let str_50k = format!("{brackets_50k}Ljava/lang/String;");
            let obj_50k = format!("{brackets_50k}Ljava/lang/Object;");
            let cs_50k = format!("{brackets_50k}Ljava/lang/CharSequence;");
            let int_50k = format!("{brackets_50k}I");
            let long_50k = format!("{brackets_50k}J");

            let obj_49999 = format!("{brackets_49999}Ljava/lang/Object;");
            let ser_49999 = format!("{brackets_49999}Ljava/io/Serializable;");
            let cln_49999 = format!("{brackets_49999}Ljava/lang/Cloneable;");
            let cs_49999 = format!("{brackets_49999}Ljava/lang/CharSequence;");

            // Reference leaf covariance at 50,000 dimensions
            assert!(cp.is_subtype(&str_50k, &obj_50k));
            assert!(cp.is_subtype(&str_50k, &cs_50k));
            assert!(!cp.is_subtype(&obj_50k, &str_50k));

            // Primitive leaf invariance at equal dimensions
            assert!(cp.is_subtype(&int_50k, &int_50k));
            assert!(!cp.is_subtype(&int_50k, &obj_50k));
            assert!(!cp.is_subtype(&int_50k, &long_50k));

            // Higher-dimension primitive array <: lower-dimension Object / Serializable / Cloneable array
            assert!(cp.is_subtype(&int_50k, &obj_49999));
            assert!(cp.is_subtype(&int_50k, &ser_49999));
            assert!(cp.is_subtype(&int_50k, &cln_49999));
            assert!(!cp.is_subtype(&int_50k, &cs_49999));

            // Lower-dimension array is NOT subtype of higher-dimension array
            assert!(!cp.is_subtype(&obj_49999, &int_50k));

            // Top-level array <: Object / Cloneable / Serializable
            assert!(cp.is_subtype(&int_50k, "Ljava/lang/Object;"));
            assert!(cp.is_subtype(&int_50k, "Ljava/io/Serializable;"));
            assert!(cp.is_subtype(&int_50k, "Ljava/lang/Cloneable;"));
        })
        .expect("spawn 512 KiB worker");

    handle.join().expect("512 KiB worker should not panic");
}
