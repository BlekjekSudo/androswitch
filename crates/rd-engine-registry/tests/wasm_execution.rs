#![cfg(feature = "wasm-execution")]
//! M11 S1c — adapter `wasm` (#28) executa o APK de demonstração pela VM + WasmBridge.
use rd_engine_registry::{Artifact, EngineAdapter, EngineEvent, Registry};
use rd_testsupport::wasm_demo;

fn launch_demo(registry: &Registry) -> rd_engine_registry::AdapterProcess {
    let adapter = registry.adapter("wasm").unwrap();
    let apk = wasm_demo::apk();
    let mut process = adapter
        .parse(Artifact {
            name: "wasm-demo.apk",
            bytes: &apk,
        })
        .expect("golden APK parses through the wasm adapter");
    adapter
        .event(
            &mut process,
            EngineEvent {
                name: "launch",
                payload: &[],
            },
        )
        .expect("launch runs onCreate and the WasmBridge call");
    process
}

#[test]
fn wasm_adapter_is_bounded_available_and_registered_as_partial_wasm() {
    let registry = Registry::embedded().unwrap();
    let descriptor = registry.get("wasm").unwrap();
    assert_eq!(
        descriptor.runtime_status,
        rd_engine_registry::RuntimeStatus::PartialWasm
    );
    assert!(rd_engine_registry::wasm_execution_available("wasm"));
    assert!(rd_engine_registry::execution_available("wasm"));
    assert_eq!(
        registry.info(descriptor).unwrap().registry_adapter_status,
        "AVAILABLE_BOUNDED"
    );
    assert!(registry.info(descriptor).unwrap().feature_enabled);
}

#[test]
fn launching_golden_apk_shows_the_wasm_result_and_claims_only_measured_launch() {
    let registry = Registry::embedded().unwrap();
    let process = launch_demo(&registry);
    let report = process.report().unwrap();
    assert_eq!(report["engine"], "wasm");
    assert_eq!(report["package"], wasm_demo::PACKAGE);
    assert_eq!(report["launched"], true);
    assert_eq!(report["execution_claimed"], true);
    assert_eq!(report["wasm"]["bridge"], "io.rustydroid.wasm.WasmBridge");
    let ui = report["ui"].as_str().expect("ui dump is text");
    assert!(
        ui.contains(wasm_demo::EXPECTED_TEXT),
        "ui must show add(2,3)={}: {ui}",
        wasm_demo::EXPECTED_TEXT
    );
}

#[test]
fn wasm_adapter_rejects_non_apk_and_unknown_events_with_typed_errors() {
    let registry = Registry::embedded().unwrap();
    let adapter = registry.adapter("wasm").unwrap();
    let bad = adapter
        .parse(Artifact {
            name: "bad.apk",
            bytes: b"bad",
        })
        .unwrap_err();
    assert_ne!(
        bad.code, "NOT_IMPLEMENTED",
        "real adapter validates the APK"
    );
    // Invalid bytes are rejected by the shared APK parser (rd-apk) before the adapter.
    assert!(!bad.module_id.is_empty());
    assert!(!bad.cause.is_empty());

    let mut process = launch_demo(&registry);
    let unknown = adapter
        .event(
            &mut process,
            EngineEvent {
                name: "resume",
                payload: &[],
            },
        )
        .unwrap_err();
    assert_eq!(unknown.code, "NOT_IMPLEMENTED");
}
