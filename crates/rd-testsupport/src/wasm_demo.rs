//! Golden de demonstração do M11 S1b (ADR 013): APK determinístico com
//! `assets/demo.wasm` real e uma Activity que chama
//! `io.rustydroid.wasm.WasmBridge.call("demo.wasm", "add", 2, 3)` em `onCreate`
//! e mostra o resultado no `TextView` de id `result`.
//!
//! Construído do fonte, sem toolchain externa: mesmos bytes a cada chamada.
//! O módulo WASM é `(i32, i32) -> i32` com `add(a, b) = a + b` (bytes à mão).

use crate::apkfix::{a, build_apk_sdk, build_axml, ArscFix, AxNode, AxVal};
use crate::vm::{op10x, op11x, op21c, op21s, op35c, DexBuilder, ACC_PUBLIC};

/// Nome do pacote do APK de demonstração.
pub const PACKAGE: &str = "io.rustydroid.wasmdemo";
/// Descritor da Activity principal.
pub const MAIN: &str = "Lio/rustydroid/wasmdemo/Main;";
/// Caminho do módulo dentro de `assets/`.
pub const ASSET: &str = "demo.wasm";
/// Função exportada chamada pela Activity.
pub const EXPORT: &str = "add";
/// Texto esperado em `com.rustydroid.wasmdemo:id/result` após `onCreate`.
pub const EXPECTED_TEXT: &str = "5";
/// Nome do recurso (`id`) do `TextView` que recebe o resultado.
pub const RESULT_ID: &str = "result";

/// Valor numérico do id `RESULT_ID` na tabela de recursos do APK (ordem fixa:
/// layout primeiro, id depois, como em [`apk`]).
pub fn result_resource_id() -> u32 {
    let mut arsc = ArscFix::with_package(PACKAGE);
    let _layout = arsc.add_layout("activity_main");
    arsc.add_id(RESULT_ID)
}

const BRIDGE: &str = "Lio/rustydroid/wasm/WasmBridge;";

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

/// Módulo WASM mínimo: um tipo `(i32, i32) -> i32` e a função `add` exportada.
pub fn wasm_module() -> Vec<u8> {
    let body: [u8; 6] = [0x20, 0, 0x20, 1, 0x6a, 0x0b]; // local.get 0; local.get 1; i32.add; end
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    let section = |m: &mut Vec<u8>, id: u8, contents: &[u8]| {
        m.push(id);
        uleb(contents.len(), m);
        m.extend_from_slice(contents);
    };
    section(&mut m, 1, &[1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f]); // type section
    section(&mut m, 3, &[1, 0]); // function section: uma função do tipo 0
    let mut exp = vec![1, EXPORT.len() as u8];
    exp.extend_from_slice(EXPORT.as_bytes());
    exp.extend_from_slice(&[0, 0]); // exporta a função 0
    section(&mut m, 7, &exp); // export section
    let mut func = vec![0u8]; // zero declarações de locals
    func.extend_from_slice(&body);
    let mut code = vec![1];
    uleb(func.len(), &mut code);
    code.extend_from_slice(&func);
    section(&mut m, 10, &code); // code section
    m
}

/// `const v<reg>, #lit` de 32 bits (formato 31i).
fn op31i(op: u8, reg: u8, lit: u32) -> Vec<u16> {
    vec![
        op as u16 | ((reg as u16) << 8),
        (lit & 0xFFFF) as u16,
        (lit >> 16) as u16,
    ]
}

/// `classes.dex`: a Activity chama o bridge e exibe o resultado.
pub fn dex() -> Vec<u8> {
    let mut b = DexBuilder::new();
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    let main = b.class(MAIN, "Landroid/app/Activity;");

    let p_v_i = b.proto_idx("V", vec!["I".into()]);
    let p_view_i = b.proto_idx("Landroid/view/View;", vec!["I".into()]);
    let p_str_i = b.proto_idx("Ljava/lang/String;", vec!["I".into()]);
    let p_v_cs = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".into()]);
    let p_bridge = b.proto_idx(
        "I",
        vec![
            "Ljava/lang/String;".into(),
            "Ljava/lang/String;".into(),
            "I".into(),
            "I".into(),
        ],
    );
    let set_content = b.method_idx("Landroid/app/Activity;", p_v_i, "setContentView");
    let find_view = b.method_idx("Landroid/app/Activity;", p_view_i, "findViewById");
    let value_of = b.method_idx("Ljava/lang/String;", p_str_i, "valueOf");
    let set_text = b.method_idx("Landroid/widget/TextView;", p_v_cs, "setText");
    let call = b.method_idx(BRIDGE, p_bridge, "call");
    let tv_type = b.type_idx("Landroid/widget/TextView;");
    let s_asset = b.intern(ASSET) as u16;
    let s_export = b.intern(EXPORT) as u16;

    // Layout e ids são os mesmos montados no APK (ver `apk`).
    let mut arsc = ArscFix::with_package(PACKAGE);
    let layout_id = arsc.add_layout("activity_main");
    let result_id = arsc.add_id(RESULT_ID);

    // Registradores: v0..v5 locais; p0 (this) = v6; p1 (Bundle) = v7.
    let mut u = Vec::new();
    u.extend(op31i(0x14, 0, layout_id)); // const v0, R.layout.activity_main
    u.extend(op35c(0x6E, 2, set_content, [6, 0, 0, 0, 0])); // invoke-virtual {p0, v0}
    u.extend(op21c(0x1A, 1, s_asset)); // const-string v1, "demo.wasm"
    u.extend(op21c(0x1A, 2, s_export)); // const-string v2, "add"
    u.extend(op21s(0x13, 3, 2)); // const/16 v3, 2
    u.extend(op21s(0x13, 4, 3)); // const/16 v4, 3
    u.extend(op35c(0x71, 4, call, [1, 2, 3, 4, 0])); // invoke-static WasmBridge.call
    u.extend(op11x(0x0A, 5)); // move-result v5
    u.extend(op35c(0x71, 1, value_of, [5, 0, 0, 0, 0])); // invoke-static String.valueOf(I)
    u.extend(op11x(0x0C, 5)); // move-result-object v5
    u.extend(op31i(0x14, 0, result_id)); // const v0, R.id.result
    u.extend(op35c(0x6E, 2, find_view, [6, 0, 0, 0, 0])); // invoke-virtual {p0, v0} findViewById
    u.extend(op11x(0x0C, 0)); // move-result-object v0
    u.extend(op21c(0x1F, 0, tv_type)); // check-cast v0, TextView
    u.extend(op35c(0x6E, 2, set_text, [0, 5, 0, 0, 0])); // invoke-virtual {v0, v5} setText
    u.extend(op10x(0x0E)); // return-void
    let code = b.code(8, 2, 5, u);
    b.r#virtual(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(code),
    );
    b.finish()
}

/// APK de demonstração completo: manifest, layout, `classes.dex` e
/// `assets/demo.wasm`. Determinístico.
pub fn apk() -> Vec<u8> {
    let mut arsc = ArscFix::with_package(PACKAGE);
    let _layout = arsc.add_layout("activity_main");
    let _result = arsc.add_id(RESULT_ID);
    let layout_xml = AxNode::new(
        "LinearLayout",
        vec![
            a(true, "orientation", AxVal::Int(1)),
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "id", AxVal::Ref(_result)),
                a(true, "text", AxVal::Str("aguardando".into())),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(96)),
            ],
            vec![],
        )],
    );
    let asset_entry = format!("assets/{ASSET}");
    build_apk_sdk(
        PACKAGE,
        "io.rustydroid.wasmdemo.Main",
        arsc.build(),
        vec![
            ("res/layout/activity_main.xml", build_axml(&layout_xml)),
            ("classes.dex", dex()),
            (asset_entry.as_str(), wasm_module()),
        ],
        26,
        30,
    )
}
