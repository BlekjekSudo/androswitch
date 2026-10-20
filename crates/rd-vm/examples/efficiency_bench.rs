//! Deterministic, assertion-bearing optimization workloads. Not a claim of
//! application-wide speedup: run the same release executable before/after.
use rd_testsupport::vm::{CodeBlob, DexBuilder};
use rd_vm::framework::HostObj;
use rd_vm::{Engine, Value};
use std::time::Instant;

fn report(name: &str, start: Instant, iterations: usize, checksum: u64) {
    println!("{{\"workload\":\"{name}\",\"iterations\":{iterations},\"elapsed_ms\":{},\"checksum\":{checksum}}}", start.elapsed().as_secs_f64() * 1000.0);
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "roots".into());
    let mut vm = Engine::new(vec![], Default::default());
    match name.as_str() {
        "roots" | "host" => {
            for _ in 0..64 {
                vm.heap.alloc_instance("LPad;".into(), vec![]).unwrap();
            }
            let leaf = vm.heap.alloc_string("reachable leaf".into()).unwrap();
            let root = vm
                .heap
                .alloc_instance("LBench;".into(), vec![("child".into(), Value::Obj(leaf))])
                .unwrap();
            // Escapes, UTF-8, long zero runs, numeric prefixes and overflow.
            let text =
                "letters αβ🌳 \\\" \n -0000000066 184467440737095516160 3e66 | ".repeat(65536);
            if name == "roots" {
                vm.statics.insert((text, "root".into()), Value::Obj(root));
            } else {
                vm.statics
                    .insert(("LBench;".into(), "root".into()), Value::Obj(root));
                vm.fw.objects.insert(
                    root,
                    HostObj::Intent {
                        component: Some(text),
                        extras: vec![("leaf".into(), Value::Obj(leaf))],
                    },
                );
            }
            vm.gc();
            let start = Instant::now();
            let mut sum = 0;
            for _ in 0..30 {
                let s = vm.gc();
                assert_eq!(s.last_freed_objects, 0);
                assert_eq!(vm.heap.as_str(leaf).unwrap(), "reachable leaf");
                sum += s.last_live_objects as u64;
            }
            report(&name, start, 30, sum);
        }
        "sweep" => {
            let start = Instant::now();
            let mut sum = 0;
            for _ in 0..12 {
                for _ in 0..100000 {
                    vm.heap.alloc_instance("LBench;".into(), vec![]).unwrap();
                }
                let s = vm.gc();
                assert!(s.last_freed_objects > 99000);
                sum += s.last_freed_objects as u64;
            }
            report(&name, start, 12, sum);
        }
        "minor" => {
            let mut roots = Vec::new();
            for _ in 0..20000 {
                let r = vm.heap.alloc_instance("LBench;".into(), vec![]).unwrap();
                roots.push(Value::Obj(r));
                vm.fw.objects.insert(r, HostObj::Bundle { map: vec![] });
            }
            let a = vm
                .heap
                .alloc_array(rd_vm::heap::ElemKind::Obj, roots.len(), "LBench;")
                .unwrap();
            *vm.heap.array_elems_mut(a).unwrap() = roots;
            vm.statics
                .insert(("LBench;".into(), "root".into()), Value::Obj(a));
            vm.gc();
            let start = Instant::now();
            let mut sum = 0;
            for _ in 0..50 {
                let s = vm.gc_minor();
                assert_eq!(s.last_freed_objects, 0);
                assert_eq!(vm.fw.objects.len(), 20000);
                sum += s.last_live_objects as u64;
            }
            report(&name, start, 50, sum);
        }
        "interp" => {
            let mut b = DexBuilder::new();
            let cls = b.class("LBench;", "Ljava/lang/Object;");
            // Sequential fetch mixed with a backward branch and a final fallthrough.
            let mut units = vec![0x0013, 20000, 0x0112];
            let loop_pc = units.len();
            units.extend(vec![0; 256]);
            units.extend([0x01d8, 0x0101]);
            let branch_pc = units.len();
            units.extend([0x0134, (loop_pc as i16 - branch_pc as i16) as u16, 0x010f]);
            b.direct(
                cls,
                "run",
                "I",
                vec![],
                0x9,
                Some(CodeBlob {
                    regs: 2,
                    ins: 0,
                    outs: 0,
                    units,
                    tries: vec![],
                }),
            );
            let mut vm = Engine::new(
                vec![rd_dex::Dex::parse(b.finish()).unwrap()],
                Default::default(),
            );
            let start = Instant::now();
            let result = vm.invoke_static("LBench;", "run", "()I", &[]).unwrap();
            assert!(matches!(result, Value::Int(20000)));
            report(&name, start, 5160000, 20000);
        }
        "resolve" => {
            let mut b = DexBuilder::new();
            let cls = b.class("LBench;", "Ljava/lang/Object;");
            for i in 0..512 {
                b.direct(
                    cls,
                    &format!("method{i:04}"),
                    "V",
                    vec![],
                    0x9,
                    Some(CodeBlob {
                        regs: 0,
                        ins: 0,
                        outs: 0,
                        units: vec![0x000e],
                        tries: vec![],
                    }),
                );
            }
            let mut vm = Engine::new(
                vec![rd_dex::Dex::parse(b.finish()).unwrap()],
                Default::default(),
            );
            assert!(vm
                .cp
                .resolve_method("LBench;", "method0511", "()V")
                .is_some());
            let start = Instant::now();
            let mut sum = 0;
            for _ in 0..50000 {
                sum += vm
                    .cp
                    .resolve_method("LBench;", "method0511", "()V")
                    .unwrap()
                    .2
                    .method_idx as u64;
            }
            report(&name, start, 50000, sum);
        }
        _ => panic!("unknown workload {name}"),
    }
}
