//! Portable VM/DEX contracts; host reentry uses real VM owners and monitors.
//! Native OS concurrency and the NRO proof are separate evidence layers.
use as_framework::App;
use as_platform::{host_fs::HostFileSystem, Clock};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{cell::Cell, path::PathBuf, rc::Rc};
struct Time;
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        0
    }
}
struct Native {
    owner: Cell<u64>,
    mode: Cell<i32>,
    calls: Cell<usize>,
    cases: String,
}
impl NativeHost for Native {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.owner.get())
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        panic!("no native ELF in host COW proof")
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(
            (class, name, sig),
            (self.cases.as_str(), "nativeTouch", "()V")
        );
        self.calls.set(self.calls.get() + 1);
        if self.mode.get() == 0 {
            vm.invoke_static(&self.cases, "nativeReentry", "()V", &[])?;
        } else {
            self.owner.set(77);
            let rejected = vm.invoke_static(&self.cases, "lockReentry", "()V", &[]);
            self.owner.set(1);
            assert!(rejected
                .unwrap_err()
                .to_string()
                .contains("contended Java monitor"));
            vm.invoke_static(&self.cases, "lockReentry", "()V", &[])?;
        }
        vm.gc();
        Ok(Value::Null)
    }
}
struct Test {
    app: App,
    native: Rc<Native>,
    cases: String,
    root: PathBuf,
}

#[test]
fn shared_log_dispatch_records_actual_sdk_throwable_callbacks_in_the_file_sink() {
    for variant in ["original", "renamed"] {
        let mut test = Test::new(variant, false);
        test.cases = test.cases.replace("CowCases", "LogCases");
        let bytes = test.call("emit", "()I", &[]).as_int().unwrap();
        let package = format!("org.fixture.cow{variant}");
        let actual =
            std::fs::read_to_string(test.root.join(format!("logs/android/{package}.log"))).unwrap();
        let expected="V/probe: verbose\nD/probe: debug\nI/probe: info\nW/probe: warning\ncallback exception summary\nW/probe: callback exception summary\nE/probe: error\ncallback exception summary\nW/probe: raw\n";
        assert_eq!(actual, expected);
        assert_eq!(bytes, expected.len() as i32);
    }
}

#[test]
fn enum_map_reuses_map_storage_and_matches_jvm_before_and_after_gc() {
    for variant in ["original", "renamed"] {
        let rows = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            format!("../../target/copy-on-write-fixture/{variant}/enum-map-oracle.tsv"),
        ))
        .unwrap();
        let expected: std::collections::BTreeMap<_, _> = rows
            .lines()
            .map(|row| row.split_once('\t').unwrap())
            .collect();
        assert_eq!(expected.len(), 26);
        let mut test = Test::new(variant, false);
        test.cases = test.cases.replace("CowCases", "EnumMapCases");
        for _ in 0..2 {
            for n in 0..26 {
                assert_eq!(
                    test.scenario(n),
                    expected[format!("scenario{n}").as_str()],
                    "{variant}/{n}"
                );
            }
            test.app.vm.gc();
        }
        let vm = &mut test.app.vm;
        let class = "Ljava/util/EnumMap;";
        let receiver = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
        vm.attach_host_state(class, receiver).unwrap();
        // Never advertise a detached snapshot as a Java live Map view.
        for (name, sig) in [
            ("entrySet", "()Ljava/util/Set;"),
            ("keySet", "()Ljava/util/Set;"),
            ("values", "()Ljava/util/Collection;"),
        ] {
            assert!(vm.invoke_instance(receiver, name, sig, &[]).is_err());
        }
    }
}

#[test]
fn builder_scalar_overloads_keep_raw_units_until_a_surrogate_pair_is_complete() {
    let mut test = Test::new("original", false);
    for class in ["Ljava/lang/StringBuilder;", "Ljava/lang/StringBuffer;"] {
        let vm = &mut test.app.vm;
        let builder = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
        vm.attach_host_state(class, builder).unwrap();
        vm.invoke_instance(builder, "<init>", "()V", &[]).unwrap();
        for (parameter, value) in [("C", 44), ("Z", 1), ("Z", 0), ("C", 0), ("C", 233)] {
            let result = vm
                .invoke_instance(
                    builder,
                    "append",
                    &format!("({parameter}){class}"),
                    &[Value::Int(value)],
                )
                .unwrap();
            assert_eq!(result, Value::Obj(builder));
        }
        assert_eq!(
            vm.invoke_instance(builder, "length", "()I", &[]).unwrap(),
            Value::Int(12)
        );
        vm.invoke_instance(
            builder,
            "append",
            &format!("(C){class}"),
            &[Value::Int(0xd800)],
        )
        .unwrap();
        assert_eq!(
            vm.invoke_instance(builder, "length", "()I", &[]).unwrap(),
            Value::Int(13)
        );
        assert_eq!(
            vm.invoke_instance(builder, "charAt", "(I)C", &[Value::Int(12)])
                .unwrap(),
            Value::Int(0xd800)
        );
        assert_eq!(
            vm.invoke_instance(builder, "codePointAt", "(I)I", &[Value::Int(12)])
                .unwrap(),
            Value::Int(0xd800)
        );
        let incomplete = vm.invoke_instance(builder, "toString", "()Ljava/lang/String;", &[]);
        assert!(matches!(incomplete, Err(VmExit::Error(ref e)) if e.code == "API_UNAVAILABLE"));
        vm.statics.insert(
            ("Ltests/Builders;".into(), "pending".into()),
            Value::Obj(builder),
        );
        vm.gc_minor();
        vm.gc_minor();
        vm.invoke_instance(
            builder,
            "append",
            &format!("(C){class}"),
            &[Value::Int(0xdc00)],
        )
        .unwrap();
        let result = vm
            .invoke_instance(builder, "toString", "()Ljava/lang/String;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        assert_eq!(vm.heap.as_str(result).unwrap(), ",truefalse\0é\u{10000}");
    }
}
impl Test {
    fn builder_case(&mut self, class: &str, which: i32) -> String {
        let vm = &mut self.app.vm;
        let r = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
        vm.attach_host_state(class, r).unwrap();
        vm.invoke_instance(r, "<init>", "()V", &[]).unwrap();
        let chain = format!("(C){class}");
        let codepoint = format!("(I){class}");
        let string = format!("(Ljava/lang/String;){class}");
        let call = |vm: &mut Engine, n: &str, s: &str, a: &[Value]| {
            vm.invoke_instance(r, n, s, a).unwrap()
        };
        let number = |vm: &mut Engine, n: &str, i: i32| {
            call(
                vm,
                n,
                if n == "charAt" { "(I)C" } else { "(I)I" },
                &[Value::Int(i)],
            )
            .as_int()
            .unwrap()
        };
        let append = |vm: &mut Engine, u: i32| {
            call(vm, "append", &chain, &[Value::Int(u)]);
        };
        let high = 0xd83d;
        let low = 0xde42;
        match which {
            0 => {
                append(vm, high);
                format!(
                    "{}:{}:{}",
                    call(vm, "length", "()I", &[]).as_int().unwrap(),
                    number(vm, "charAt", 0),
                    number(vm, "codePointAt", 0)
                )
            }
            1 => {
                append(vm, high);
                append(vm, low);
                format!(
                    "{}:{}",
                    call(vm, "length", "()I", &[]).as_int().unwrap(),
                    number(vm, "codePointAt", 0)
                )
            }
            2 => {
                let text = Value::Obj(vm.heap.alloc_string("ab".into()).unwrap());
                call(vm, "append", &string, &[text]);
                call(
                    vm,
                    "insert",
                    &format!("(IC){class}"),
                    &[Value::Int(1), Value::Int(high)],
                );
                call(
                    vm,
                    "insert",
                    &format!("(IC){class}"),
                    &[Value::Int(2), Value::Int(low)],
                );
                format!(
                    "{}:{}:{}",
                    call(vm, "length", "()I", &[]).as_int().unwrap(),
                    number(vm, "codePointAt", 1),
                    number(vm, "charAt", 3)
                )
            }
            3 => {
                let text = Value::Obj(vm.heap.alloc_string("ab".into()).unwrap());
                call(vm, "append", &string, &[text]);
                call(vm, "setCharAt", "(IC)V", &[Value::Int(0), Value::Int(high)]);
                call(vm, "setCharAt", "(IC)V", &[Value::Int(1), Value::Int(low)]);
                number(vm, "codePointAt", 0).to_string()
            }
            4 => {
                append(vm, low);
                append(vm, high);
                call(vm, "reverse", &format!("(){class}"), &[]);
                number(vm, "codePointAt", 0).to_string()
            }
            5 => {
                call(vm, "appendCodePoint", &codepoint, &[Value::Int(high)]);
                call(vm, "appendCodePoint", &codepoint, &[Value::Int(low)]);
                number(vm, "codePointAt", 0).to_string()
            }
            6 => {
                call(vm, "appendCodePoint", &codepoint, &[Value::Int(0x1f642)]);
                call(vm, "setLength", "(I)V", &[Value::Int(1)]);
                append(vm, low);
                number(vm, "codePointAt", 0).to_string()
            }
            7 => {
                append(vm, high);
                call(vm, "setLength", "(I)V", &[Value::Int(3)]);
                let array = vm
                    .heap
                    .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, 3, "[C")
                    .unwrap();
                call(
                    vm,
                    "getChars",
                    "(II[CI)V",
                    &[
                        Value::Int(0),
                        Value::Int(3),
                        Value::Obj(array),
                        Value::Int(0),
                    ],
                );
                vm.heap
                    .as_array_elems(array)
                    .unwrap()
                    .0
                    .iter()
                    .map(|v| v.as_int().unwrap().to_string())
                    .collect::<Vec<_>>()
                    .join(":")
            }
            8 => {
                append(vm, high);
                append(vm, 120);
                call(vm, "deleteCharAt", &codepoint, &[Value::Int(1)]);
                append(vm, low);
                number(vm, "codePointAt", 0).to_string()
            }
            9 => {
                append(vm, high);
                append(vm, 120);
                append(vm, low);
                let empty = Value::Obj(vm.heap.alloc_string("".into()).unwrap());
                call(
                    vm,
                    "replace",
                    &format!("(IILjava/lang/String;){class}"),
                    &[Value::Int(1), Value::Int(2), empty],
                );
                number(vm, "codePointAt", 0).to_string()
            }
            10 => {
                append(vm, 97);
                call(vm, "appendCodePoint", &codepoint, &[Value::Int(0x1f642)]);
                append(vm, 98);
                call(vm, "reverse", &format!("(){class}"), &[]);
                format!(
                    "{}:{}:{}",
                    number(vm, "charAt", 0),
                    number(vm, "codePointAt", 1),
                    number(vm, "charAt", 3)
                )
            }
            _ => unreachable!(),
        }
    }
    #[allow(clippy::assertions_on_constants)]
    fn new(variant: &str, proof: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-cow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/copy-on-write-fixture")
            .join(variant)
            .join(if proof { "cow-proof.apk" } else { "cow.apk" });
        let record = pm
            .install(
                std::fs::read(fixture).expect("run fixtures/m9/copy-on-write/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let mut app = App::new(
            pm.load_apk(&record.package, None).unwrap().1,
            fs,
            Rc::new(Time),
            640,
            360,
            None,
        )
        .unwrap();
        let ns = if variant == "original" {
            "org/androswitch/contracts"
        } else {
            "net/example/snapshots"
        };
        let cases = format!("L{ns}/CowCases;");
        let native = Rc::new(Native {
            owner: Cell::new(1),
            mode: Cell::new(0),
            calls: Cell::new(0),
            cases: cases.clone(),
        });
        app.vm.bind_native(native.clone());
        app.launch().unwrap();
        Self {
            app,
            native,
            cases,
            root,
        }
    }
    fn invoke(&mut self, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
        self.app.vm.invoke_static(&self.cases, name, sig, args)
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.invoke(name, sig, args).unwrap()
    }
    fn text(&mut self, name: &str, sig: &str, args: &[Value]) -> String {
        let r = self.call(name, sig, args).as_ref().unwrap().unwrap();
        self.app.vm.heap.as_str(r).unwrap().to_string()
    }
    fn int(&mut self, name: &str) -> i32 {
        self.call(name, "()I", &[]).as_int().unwrap()
    }
    fn reference(&mut self, name: &str) -> ObjRef {
        self.call(name, "()Ljava/lang/Object;", &[])
            .as_ref()
            .unwrap()
            .unwrap()
    }
    fn scenario(&mut self, value: i32) -> String {
        self.text("scenario", "(I)Ljava/lang/String;", &[Value::Int(value)])
    }
    fn freed(&self, r: ObjRef) -> bool {
        self.app.vm.heap.class_of(r).unwrap() == rd_vm::gc::FREED_CLASS
    }
}
#[test]
fn builder_raw_edits_match_independent_jvm_rows_for_both_shared_classes() {
    let rows = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local-artifacts/builder-utf16/jvm.tsv"),
    )
    .expect("run fixtures/builder-utf16/build.ps1");
    let mut t = Test::new("original", false);
    for class in ["Ljava/lang/StringBuilder;", "Ljava/lang/StringBuffer;"] {
        for row in rows.lines() {
            let (which, expected) = row.split_once('\t').unwrap();
            assert_eq!(
                t.builder_case(class, which.parse().unwrap()),
                expected,
                "{class} case {which}"
            );
        }
    }
}
#[test]
fn sixteen_cases_match_actual_jvm_in_renamed_namespaces_before_and_after_gc() {
    for variant in ["original", "renamed"] {
        let rows = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            format!("../../target/copy-on-write-fixture/{variant}/oracle.tsv"),
        ))
        .unwrap();
        let expected: std::collections::BTreeMap<_, _> = rows
            .lines()
            .map(|row| row.split_once('\t').unwrap())
            .collect();
        assert_eq!(expected.len(), 16);
        let mut t = Test::new(variant, false);
        for _ in 0..2 {
            for case in 0..=13 {
                assert_eq!(
                    t.scenario(case),
                    expected[format!("scenario{case}").as_str()],
                    "{variant}/{case}"
                );
            }
            assert_eq!(
                t.text("errors", "()Ljava/lang/String;", &[]),
                expected["errors"]
            );
            assert_eq!(
                t.int("iteratorMessages").to_string(),
                expected["iteratorMessages"]
            );
            t.app.vm.gc();
        }
    }
}

#[test]
fn cow_set_reuses_list_generations_and_matches_twenty_jvm_cases_in_both_namespaces() {
    for variant in ["original", "renamed"] {
        let rows = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            format!("../../target/copy-on-write-fixture/{variant}/set-oracle.tsv"),
        ))
        .unwrap();
        let expected: std::collections::BTreeMap<_, _> = rows
            .lines()
            .map(|row| row.split_once('\t').unwrap())
            .collect();
        assert_eq!(expected.len(), 20);
        let mut t = Test::new(variant, false);
        t.cases = t.cases.replace("/CowCases;", "/CowSetCases;");
        for _ in 0..2 {
            for case in 0..20 {
                assert_eq!(
                    t.scenario(case),
                    expected[format!("scenario{case}").as_str()],
                    "{variant}/{case}"
                );
            }
            t.app.vm.gc();
        }
        let set = t.reference("setObject");
        let error = t
            .app
            .vm
            .invoke_instance(set, "get", "(I)Ljava/lang/Object;", &[Value::Int(0)])
            .unwrap_err();
        assert!(error.to_string().contains("API_UNAVAILABLE"));
        let host = t.app.vm.framework_host().unwrap();
        assert_eq!(
            host.method_metadata(
                "Ljava/util/concurrent/CopyOnWriteArraySet;",
                "<init>",
                "(I)V"
            ),
            None
        );
        assert_eq!(
            host.method_metadata(
                "Ljava/util/concurrent/CopyOnWriteArraySet;",
                "listIterator",
                "()Ljava/util/ListIterator;"
            ),
            None
        );
    }
}
#[test]
fn virtual_equality_native_reentry_retains_the_snapshot_after_list_clear_and_gc() {
    for variant in ["original", "renamed"] {
        let mut t = Test::new(variant, false);
        assert_eq!(t.scenario(14), "1:77,");
        assert_eq!(t.native.calls.get(), 1);
    }
}
#[test]
fn private_monitor_preserves_owner_on_contention_and_is_reentrant_for_the_same_owner() {
    let mut t = Test::new("original", false);
    t.native.mode.set(1);
    assert_eq!(t.scenario(15), "1:8,");
    assert_eq!(t.native.calls.get(), 1);
    // A later writer proves the exact lock acquisition was released on unwind.
    t.call("lockReentry", "()V", &[]);
    assert_eq!(t.int("size"), 2);
}
#[test]
fn iterator_alone_retains_all_snapshot_values_but_does_not_retain_its_list() {
    let mut t = Test::new("original", false);
    t.call("setupSnapshot", "()V", &[]);
    let owner = t.reference("listObject");
    let old = t.reference("oldValue");
    t.call("dropList", "()V", &[]);
    t.app.vm.gc();
    assert!(t.freed(owner));
    assert!(!t.freed(old));
    assert_eq!(t.app.vm.heap.get_field(old, "id").unwrap(), Value::Int(101));
    t.call("dropIterator", "()V", &[]);
    t.app.vm.gc();
    assert!(t.freed(old));
}
#[test]
fn element_and_retained_generation_budgets_fail_before_publication_and_release_after_gc() {
    let mut t = Test::new("original", false);
    t.call("budgetSetup", "(I)V", &[Value::Int(10000)]);
    let failed = t.invoke("budgetGrow", "()V", &[]).unwrap_err();
    assert!(failed.to_string().contains("COLLECTION_BUDGET"));
    assert_eq!(t.int("size"), 10000);
    t.call("budgetSetup", "(I)V", &[Value::Int(1000)]);
    t.app.vm.gc();
    let failed = t.invoke("retainSnapshots", "()V", &[]).unwrap_err();
    assert!(failed.to_string().contains("COLLECTION_BUDGET"));
    assert_eq!(t.int("size"), 1000);
    t.call("setSameReference", "()V", &[]);
    assert_eq!(t.int("invalidIndex"), 1);
    t.call("releaseSnapshots", "()V", &[]);
    t.app.vm.gc();
    t.call("budgetGrow", "()V", &[]);
    assert_eq!(t.int("size"), 1001);
}
#[test]
fn expired_generation_registry_is_pruned_while_mutation_keeps_advancing() {
    let mut t = Test::new("original", false);
    t.call("budgetSetup", "(I)V", &[Value::Int(1)]);
    t.call("rotateGenerations", "(I)V", &[Value::Int(17000)]);
    assert_eq!(t.int("first"), 16999);
    assert_eq!(t.int("size"), 1);
}
#[test]
fn exact_metadata_and_unavailable_broad_operations_do_not_fake_mutations() {
    let mut t = Test::new("original", false);
    t.call("budgetSetup", "(I)V", &[Value::Int(2)]);
    let failed = t.invoke("unsupported", "()V", &[]).unwrap_err();
    assert!(failed.to_string().contains("API_UNAVAILABLE"));
    assert_eq!(t.int("size"), 2);
    use rd_vm::framework_host::HostMethodKind;
    let host = t.app.vm.framework_host().unwrap();
    assert_eq!(
        host.method_metadata(
            "Ljava/util/concurrent/CopyOnWriteArrayList;",
            "<init>",
            "()V"
        ),
        Some(HostMethodKind::Constructor)
    );
    assert!(host
        .method_metadata(
            "Ljava/util/concurrent/CopyOnWriteArrayList;",
            "removeIf",
            "(Ljava/util/function/Predicate;)Z"
        )
        .is_none());
    assert!(t.app.vm.cp.is_subtype(
        "Ljava/util/concurrent/CopyOnWriteArrayList$COWIterator;",
        "Ljava/util/Iterator;"
    ));
    assert!(t.app.vm.cp.is_subtype(
        "Ljava/util/concurrent/CopyOnWriteArrayList$COWIterator;",
        "Ljava/util/ListIterator;"
    ));
}
#[test]
fn standard_sdk_proof_uses_real_snapshot_mutation_gc_and_shared_vfs() {
    let mut t = Test::new("original", true);
    t.app.draw().unwrap();
    let proof = std::fs::read_to_string(t.root.join("Download/androswitch-cow-proof.txt")).unwrap();
    assert_eq!(proof,"COW_SNAPSHOT_PROOF_PASS\nsnapshot=1,2,/3,-1,\nreentrantRemove=1:1,9,\niterator=2:2:1:0:-1:3:1:1\ngcRetained=101\n");
}
#[test]
fn ordinary_arraylist_iterator_remains_fail_fast_and_is_not_a_cow_alias() {
    let mut t = Test::new("original", false);
    assert_eq!(t.int("regular"), 1);
    assert_eq!(t.scenario(0), "1,2,/3,-1,");
}
