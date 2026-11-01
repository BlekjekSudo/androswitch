//! M11 S1b — golden do APK de demonstração (`rd_testsupport::wasm_demo`).
//!
//! Diferente de `wasm_bridge.rs` (DEX e módulo montados no teste), aqui o APK
//! é o artefato real: `classes.dex` e `assets/demo.wasm` são lidos do ZIP, a
//! Activity roda por `launch_app` e o texto visível do `TextView` precisa ser o
//! resultado do export WASM (`add(2, 3) = 5`).
//!
//! Requer `--features wasm-engine`.
#![cfg(feature = "wasm-engine")]

use rd_testsupport::wasm_demo;
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::value::Value;

#[test]
fn demo_apk_shows_the_wasm_result_in_its_text_view() {
    let apk = rd_apk::Apk::from_bytes(wasm_demo::apk()).expect("APK parseia");
    let entry = apk.zip.find("classes.dex").expect("classes.dex no APK");
    let dex_bytes = apk.zip.read_entry(entry).expect("lê classes.dex");
    let dex = rd_dex::Dex::parse(dex_bytes).expect("DEX parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk);

    let act = vm
        .launch_app(wasm_demo::MAIN, wasm_demo::PACKAGE, vec![])
        .expect("launch_app");
    let view = vm
        .invoke_instance(
            act,
            "findViewById",
            "(I)Landroid/view/View;",
            &[Value::Int(wasm_demo::result_resource_id() as i32)],
        )
        .expect("findViewById(result)");
    let Value::Obj(view_ref) = view else {
        panic!("findViewById devolveu {view:?}");
    };
    let text = vm
        .invoke_instance(view_ref, "getText", "()Ljava/lang/CharSequence;", &[])
        .expect("getText");
    let Value::Obj(r) = text else {
        panic!("getText devolveu {text:?}");
    };
    assert_eq!(vm.heap.as_str(r).expect("texto"), wasm_demo::EXPECTED_TEXT);
}

/// O APK é determinístico: mesmos bytes a cada chamada (reprodutível sem
/// toolchain externa, ADR 013 §9).
#[test]
fn demo_apk_bytes_are_deterministic() {
    assert_eq!(wasm_demo::apk(), wasm_demo::apk());
    assert_eq!(wasm_demo::wasm_module(), wasm_demo::wasm_module());
}
