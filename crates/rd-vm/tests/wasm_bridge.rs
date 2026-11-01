//! M11 S1b — `io.rustydroid.wasm.WasmBridge` ponta a ponta na VM (ADR 013).
//!
//! Golden de VM: o APK é montado em memória a partir de fontes (bytes WASM
//! escritos à mão + DEX sintético), sem binário commitado. O DEX chama
//! `invoke-static WasmBridge.call(String,String,int,int)I` pelo caminho de
//! execução real; o módulo WASM é lido de `assets/` do APK injetado.
//!
//! Requer `--features wasm-engine`.
#![cfg(feature = "wasm-engine")]

mod common;

use common::*;
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::value::Value;

const BRIDGE: &str = "Lio/rustydroid/wasm/WasmBridge;";
const ACC_PUBLIC: u32 = 0x1;
const ACC_PUBLIC_STATIC: u32 = 0x9;

fn uleb(mut v: usize, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

/// Módulo com tipo `(i32,i32)->i32`, uma função exportada `export` e corpo
/// `body` (sem o byte de contagem de locals, que é 0 aqui).
fn module(export: &str, body: &[u8]) -> Vec<u8> {
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    let section = |m: &mut Vec<u8>, id: u8, contents: &[u8]| {
        m.push(id);
        uleb(contents.len(), m);
        m.extend_from_slice(contents);
    };
    section(&mut m, 1, &[1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f]);
    section(&mut m, 3, &[1, 0]);
    let mut exp = vec![1, export.len() as u8];
    exp.extend_from_slice(export.as_bytes());
    exp.extend_from_slice(&[0, 0]);
    section(&mut m, 7, &exp);
    let mut code = vec![1];
    let mut func = vec![0u8]; // zero declarações de locals
    func.extend_from_slice(body);
    uleb(func.len(), &mut code);
    code.extend_from_slice(&func);
    section(&mut m, 10, &code);
    m
}

/// `add(a,b) = a + b`.
fn add_module() -> Vec<u8> {
    module("add", &[0x20, 0, 0x20, 1, 0x6a, 0x0b])
}

/// Laço infinito: esgota o fuel por construção.
fn spin_module() -> Vec<u8> {
    module("spin", &[0x03, 0x40, 0x0c, 0, 0x0b, 0x41, 0, 0x0b])
}

/// DEX com `LCaso;->run()I` que chama o bridge com (asset, export, a, b).
fn dex_run(asset: &str, export: &str, a: i16, b: i16) -> DexBuilder {
    let mut d = DexBuilder::new();
    let cls = d.class("LCaso;", "Ljava/lang/Object;");
    let proto = d.proto_idx(
        "I",
        vec![
            "Ljava/lang/String;".into(),
            "Ljava/lang/String;".into(),
            "I".into(),
            "I".into(),
        ],
    );
    let call = d.method_idx(BRIDGE, proto, "call");
    let s_asset = d.intern(asset) as u16;
    let s_export = d.intern(export) as u16;
    let mut u = Vec::new();
    u.extend(op21c(0x1a, 0, s_asset)); // const-string v0, asset
    u.extend(op21c(0x1a, 1, s_export)); // const-string v1, export
    u.extend(op21s(0x13, 2, a)); // const/16 v2, a
    u.extend(op21s(0x13, 3, b)); // const/16 v3, b
    u.extend(op35c(0x71, 4, call, [0, 1, 2, 3, 0])); // invoke-static
    u.extend(op11x(0x0a, 0)); // move-result v0
    u.extend(op11x(0x0f, 0)); // return v0
    let code = d.code(4, 0, 4, u);
    d.direct(cls, "run", "I", vec![], ACC_PUBLIC_STATIC, Some(code));
    d
}

/// APK com `assets/<name>` = `bytes` (o `build_apk` grava o nome literal).
fn apk_with_asset(name: &str, bytes: Vec<u8>) -> rd_apk::Apk {
    let entry = format!("assets/{name}");
    let apk_bytes = apkfix::build_apk(
        "com.test.wasm",
        "LMain;",
        apkfix::ArscFix::new().build(),
        vec![(entry.as_str(), bytes)],
    );
    rd_apk::Apk::from_bytes(apk_bytes).expect("APK parseia")
}

fn vm_with(asset_name: &str, asset: Vec<u8>, export: &str, a: i16, b: i16) -> Engine {
    let dex = rd_dex::Dex::parse(dex_run(asset_name, export, a, b).finish()).expect("DEX parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk_with_asset(asset_name, asset));
    vm
}

fn run(vm: &mut Engine) -> Result<Value, VmExit> {
    invoke(vm, "run", "()I", &[])
}

/// Exige uma exceção de classe `class` cuja mensagem começa por `code`
/// (código estável do `rd-engine-wasm`, ou o caminho do asset).
fn expect_exc(r: Result<Value, VmExit>, class: &str, code: &str) {
    match r {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, class, "classe da exceção");
            let msg = t.message.unwrap_or_default();
            assert!(
                msg.starts_with(code),
                "mensagem {msg:?} deve começar por {code:?}"
            );
        }
        other => panic!("esperava exceção {class}, veio {other:?}"),
    }
}

const ISE: &str = "Ljava/lang/IllegalStateException;";
const FNF: &str = "Ljava/io/FileNotFoundException;";

#[test]
fn bridge_returns_the_export_result() {
    // assets/math/add.wasm — caminho aninhado, como um app real.
    let mut vm = vm_with("math/add.wasm", add_module(), "add", 20, 22);
    assert_eq!(run(&mut vm).expect("bridge ok"), Value::Int(42));
}

#[test]
fn bridge_wraps_i32_arithmetic() {
    let mut vm = vm_with("math/add.wasm", add_module(), "add", i16::MAX, i16::MAX);
    assert_eq!(run(&mut vm).expect("bridge ok"), Value::Int(65534));
}

#[test]
fn missing_asset_is_a_typed_exception() {
    // O DEX pede `math/absent.wasm`; o APK só tem `math/add.wasm`.
    let dex = rd_dex::Dex::parse(dex_run("math/absent.wasm", "add", 1, 2).finish()).unwrap();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk_with_asset("math/add.wasm", add_module()));
    expect_exc(run(&mut vm), FNF, "assets/math/absent.wasm");
}

#[test]
fn hostile_asset_path_is_rejected_before_zip_lookup() {
    // `../math/add.wasm` existiria se o caminho não fosse validado.
    let dex = rd_dex::Dex::parse(dex_run("../math/add.wasm", "add", 1, 2).finish()).unwrap();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk_with_asset("math/add.wasm", add_module()));
    expect_exc(run(&mut vm), FNF, "assets/../math/add.wasm");
}

#[test]
fn unknown_export_is_a_typed_exception() {
    let mut vm = vm_with("math/add.wasm", add_module(), "nope", 1, 2);
    expect_exc(run(&mut vm), ISE, "NOT_FOUND");
}

#[test]
fn garbage_bytes_are_a_typed_exception() {
    let mut vm = vm_with("math/add.wasm", b"not wasm at all".to_vec(), "add", 1, 2);
    let r = run(&mut vm);
    match &r {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, ISE),
        other => panic!("esperava exceção, veio {other:?}"),
    }
    assert!(
        matches!(&r, Err(VmExit::Exception(t)) if t.message.as_deref().unwrap_or("").starts_with("PARSE_ERROR")
            || t.message.as_deref().unwrap_or("").starts_with("INVALID_FORMAT")),
        "código de parse esperado: {r:?}"
    );
}

#[test]
fn infinite_loop_exhausts_fuel() {
    let mut vm = vm_with("math/spin.wasm", spin_module(), "spin", 0, 0);
    expect_exc(run(&mut vm), ISE, "FUEL_EXHAUSTED");
}

/// Golden de ciclo de vida: `LMain` (Activity) chama o bridge dentro de
/// `onCreate` — caminho real de `launch_app` — e grava o resultado em um
/// campo de instância, que o teste lê de volta do heap.
#[test]
fn activity_on_create_calls_the_bridge_and_stores_the_result() {
    let mut b = DexBuilder::new();
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.instance_field(main, "r", "I");
    let f_r = b.field_idx("LMain;", "I", "r");
    let proto = b.proto_idx(
        "I",
        vec![
            "Ljava/lang/String;".into(),
            "Ljava/lang/String;".into(),
            "I".into(),
            "I".into(),
        ],
    );
    let call = b.method_idx(BRIDGE, proto, "call");
    let s_asset = b.intern("math/add.wasm") as u16;
    let s_export = b.intern("add") as u16;
    let mut u = Vec::new();
    u.extend(op21c(0x1a, 0, s_asset)); // const-string v0
    u.extend(op21c(0x1a, 1, s_export)); // const-string v1
    u.extend(op21s(0x13, 2, 20)); // const/16 v2, 20
    u.extend(op21s(0x13, 3, 22)); // const/16 v3, 22
    u.extend(op35c(0x71, 4, call, [0, 1, 2, 3, 0])); // invoke-static
    u.extend(op11x(0x0a, 0)); // move-result v0
    u.extend(op22c(0x59, 0, 4, f_r)); // iput v0, v4 (this), r
    u.extend(op10x(0x0e)); // return-void
    b.r#virtual(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC, // método de instância público
        Some(b.code(6, 2, 4, u)),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX parseia");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    e.set_resources(apk_with_asset("math/add.wasm", add_module()));
    let act = e
        .launch_app("LMain;", "com.test.wasm", vec![])
        .expect("launch_app");
    assert_eq!(e.heap.get_field(act, "r").expect("campo r"), Value::Int(42));
}
