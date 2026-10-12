//! Fixture do app trivial (activity + TextView) — o mesmo APK usado pelos
//! testes do MCP e pelo gate de performance do M7 (`rd perf`).

use crate::apkfix::{a, build_axml, ArscFix, AxNode, AxVal};
use crate::vm::{op10x, op21c, op35c, DexBuilder, ACC_PUBLIC};

fn op31i_local(op: u8, reg: u8, lit: u32) -> Vec<u16> {
    let mut u = vec![op as u16 | ((reg as u16) << 8)];
    u.push((lit & 0xFFFF) as u16);
    u.push((lit >> 16) as u16);
    u
}

/// APK fixture do app trivial (package `com.test.mcp`, min 26): onCreate faz
/// println + setContentView(activity_main) com TextView id/tv "Olá via MCP".
/// Determinístico: mesmos bytes a cada chamada. Usado por testes e pelo `rd perf`.
pub fn trivial_app_apk() -> Vec<u8> {
    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("activity_main");
    let s_hello = arsc.add_string("hello", "Olá via MCP");
    let id_tv = arsc.add_id("tv");
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
                a(true, "id", AxVal::Ref(id_tv)),
                a(true, "text", AxVal::Ref(s_hello)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(96)),
            ],
            vec![],
        )],
    );
    let mut b = DexBuilder::new();
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
    let p_iv = b.proto_idx("V", vec!["I".to_string()]);
    let p_v_str = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
    let f_out = b.field_idx("Ljava/lang/System;", "Ljava/io/PrintStream;", "out");
    let ps_println = b.method_idx("Ljava/io/PrintStream;", p_v_str, "println");
    let set_content = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
    let s_hi = b.intern("log do app") as u16;
    let main = b.class("Lcom/test/mcp/Main;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            let mut u = op21c(0x1A, 0, s_hi);
            u.extend(op21c(0x60, 1, f_out));
            u.extend(op35c(0x6E, 2, ps_println, [1, 0, 0, 0, 0]));
            u.extend(op31i_local(0x14, 2, layout));
            u.extend(op35c(0x6E, 2, set_content, [3, 2, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    crate::apkfix::build_apk_sdk(
        "com.test.mcp",
        "com.test.mcp.Main",
        arsc.build(),
        vec![
            ("res/layout/activity_main.xml", build_axml(&layout_xml)),
            ("classes.dex", b.finish()),
        ],
        26,
        30,
    )
}
