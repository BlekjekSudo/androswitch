//! Run in isolation: cargo test -p as-switch-smoke --features portable-tests.
//! A workspace invocation can enable rd-vm/std through upstream dev-deps;
//! assert the actual linked variant rather than silently testing the wrong VM.
use rd_vm::{Engine, Value, VmConfig};

#[allow(clippy::assertions_on_constants)] // Detect additive Cargo feature unification.
fn vm() -> Engine {
    assert!(
        rd_vm::PORTABLE_CORE,
        "run this test with only -p as-switch-smoke"
    );
    Engine::new(Vec::new(), VmConfig::default())
}

#[test]
fn collector_preserves_transitive_roots_and_recycles_unreachable_slots() {
    let mut vm = vm();
    let child = vm.heap.alloc_string("alive".to_string()).unwrap();
    let parent = vm
        .heap
        .alloc_instance(
            "LObject;".to_string(),
            vec![("child".to_string(), Value::Obj(child))],
        )
        .unwrap();
    let dead = vm.heap.alloc_string("garbage".to_string()).unwrap();
    vm.statics.insert(
        ("LRoot;".to_string(), "root".to_string()),
        Value::Obj(parent),
    );
    let before = vm.heap.used();
    assert_eq!(vm.gc().last_freed_objects, 1);
    assert!(vm.heap.used() < before);
    assert_eq!(vm.heap.as_str(child).unwrap(), "alive");
    assert_eq!(
        vm.heap.get_field(parent, "child").unwrap(),
        Value::Obj(child)
    );
    assert_eq!(vm.heap.alloc_string("reused".to_string()).unwrap(), dead);
    vm.statics.clear();
    assert_eq!(vm.gc().last_freed_objects, 3);
}

#[test]
fn dynamic_strings_have_distinct_identity_and_follow_actual_roots() {
    let mut vm = vm();
    let first = rd_vm::intrinsics::alloc_string(&mut vm, "same".to_string()).unwrap();
    let second = rd_vm::intrinsics::alloc_string(&mut vm, "same".to_string()).unwrap();
    assert_ne!(first, second);
    vm.statics.insert(
        ("LRoot;".to_string(), "text".to_string()),
        Value::Obj(first),
    );
    assert_eq!(vm.gc().last_freed_objects, 1);
    assert_eq!(vm.heap.as_str(first).unwrap(), "same");
    vm.statics.clear();
    assert_eq!(vm.gc().last_freed_objects, 1);
}

#[test]
fn missing_platform_objects_fail_instead_of_attaching_fake_state() {
    let mut vm = vm();
    assert!(vm.attach_host_state("Ljava/lang/Object;", 0).is_ok());
    let error = vm
        .attach_host_state("Landroid/app/Activity;", 1)
        .unwrap_err()
        .to_string();
    assert!(error.contains("API_UNAVAILABLE"));
    assert!(error.contains("Landroid/app/Activity;"));
}

#[test]
fn malformed_request_and_apk_have_structured_errors() {
    for (apk, request, expected) in [
        (Vec::new(), b"{}".as_slice(), "INVALID_REQUEST"),
        (
            vec![1, 2, 3],
            br#"{"schema_version":1,"session_id":"bad-apk","calls":[]}"#.as_slice(),
            "PARSE_ERROR",
        ),
    ] {
        let json = as_switch_smoke::probe::run_json(apk, request);
        let error: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(error["code"], expected);
        assert!(error["module_id"].is_string());
        assert!(error["cause"].is_string());
    }
}

#[test]
#[ignore = "requires scripts/build-m1-fixture.ps1"]
fn real_apk_results_match_jvm_and_guard_missing_services() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m1/fixture");
    let json = as_switch_smoke::probe::run_json(
        std::fs::read(root.join("vm-semantics.apk")).unwrap(),
        &std::fs::read(root.join("request.json")).unwrap(),
    );
    let report: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("jvm-expected.json")).unwrap()).unwrap();
    assert_eq!(report["portable_vm"], true);
    for call in report["calls"].as_array().unwrap() {
        let id = call["id"].as_str().unwrap();
        if let Some(value) = oracle.get(id) {
            assert!(call["error"].is_null(), "{id}: {call}");
            assert_eq!(&call["value"], value, "{id}");
        } else {
            assert!(call["value"].is_null(), "{id}: {call}");
            assert!(call["error"]["code"].is_string(), "{id}");
        }
    }
}
