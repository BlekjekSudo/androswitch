//! Testes de integração M3.2 — LayoutInflater de layout XML real (AXML via
//! rd-apk + Resources/ARSC). APK sintético byte a byte (apkfix) + DEX
//! sintético (DexBuilder) — mesma política dos demais testes.
//!
//! Pipeline exercido (ponta a ponta, pelo dispatch real da VM):
//!   onCreate → Activity.setContentView(I) → resid → @layout/key (ARSC) →
//!   res/layout/key.xml (ZIP) → axml::parse → árvore de views host →
//!   findViewById/touch (android:onClick) → setText (String e resid).

#[macro_use]
mod common;

use common::apkfix::{a, build_apk, build_axml, AxNode, AxVal};
use common::*;
use rd_vm::err::VmExit;
use rd_vm::framework::HostObj;
use rd_vm::value::Value;

pub const ACC_CONSTRUCTOR: u32 = 0x1_0000;

// resids do fixture (pkg 0x7f; types na ordem de inserção do ArscFix:
// 1=layout, 2=string, 3=id)
const LAYOUT_MAIN: i32 = 0x7F01_0000;
const LAYOUT_BAD: i32 = 0x7F01_0001;
const S_HELLO: i32 = 0x7F02_0000;
const S_CLICKED: i32 = 0x7F02_0001;
const S_DIN: i32 = 0x7F02_0002;
const ID_TV: i32 = 0x7F03_0000;
const ID_BTN: i32 = 0x7F03_0001;

/// const vAA, +BBBBBBBB (formato 31i) — resid é 32 bits.
fn op31i(op: u8, reg: u8, lit: i32) -> Vec<u16> {
    vec![
        (op as u16) | ((reg as u16) << 8),
        (lit & 0xFFFF) as u16,
        ((lit >> 16) & 0xFFFF) as u16,
    ]
}

fn register_platform_classes(b: &mut DexBuilder) {
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    b.class("Landroid/widget/Button;", "Landroid/widget/TextView;");
    b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/widget/FrameLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
}

struct XmlRefs {
    activity_init: u16,
    set_content_i: u16,
    find_view: u16,
    tv_set_text_cs: u16,
    tv_set_text_i: u16,
    get_string: u16,
}

fn xml_refs(b: &mut DexBuilder) -> XmlRefs {
    let p_void = b.proto_idx("V", vec![]);
    let p_iv = b.proto_idx("V", vec!["I".to_string()]);
    let p_charseq = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".to_string()]);
    let p_ret_view = b.proto_idx("Landroid/view/View;", vec!["I".to_string()]);
    let p_ret_str = b.proto_idx("Ljava/lang/String;", vec!["I".to_string()]);
    XmlRefs {
        activity_init: b.method_idx("Landroid/app/Activity;", p_void, "<init>"),
        set_content_i: b.method_idx("Landroid/app/Activity;", p_iv, "setContentView"),
        find_view: b.method_idx("Landroid/app/Activity;", p_ret_view, "findViewById"),
        tv_set_text_cs: b.method_idx("Landroid/widget/TextView;", p_charseq, "setText"),
        tv_set_text_i: b.method_idx("Landroid/widget/TextView;", p_iv, "setText"),
        get_string: b.method_idx("Landroid/app/Activity;", p_ret_str, "getString"),
    }
}

/// Layout canônico do fixture: LinearLayout(vertical) { TextView, Button }.
/// TextView com 18sp (attr ignorado no modelo headless — não pode falhar) e
/// 80dp de altura (→ 160px no viewport DENSITY 2.0).
fn main_layout() -> AxNode {
    AxNode::new(
        "LinearLayout",
        vec![
            a(true, "orientation", AxVal::Int(1)),
            a(true, "layout_width", AxVal::Int(-1)), // match_parent
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![
            AxNode::new(
                "TextView",
                vec![
                    a(true, "id", AxVal::Ref(ID_TV as u32)),
                    a(true, "text", AxVal::Ref(S_HELLO as u32)), // @string/hello
                    a(true, "textSize", AxVal::Dim { raw: (18 << 8) | 2 }), // 18sp
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Dim { raw: (80 << 8) | 1 }), // 80dp
                ],
                vec![],
            ),
            AxNode::new(
                "Button",
                vec![
                    a(true, "id", AxVal::Ref(ID_BTN as u32)),
                    a(true, "text", AxVal::Str("Clique aqui".into())),
                    a(true, "onClick", AxVal::Str("onBtn".into())),
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Int(-2)), // wrap_content → ROW_H
                ],
                vec![],
            ),
        ],
    )
}

/// APK sintético com layout/arsc/manifest (package com.test.xml, Main).
fn build_fixture(layouts: Vec<(&str, AxNode)>) -> Vec<u8> {
    let mut arsc = common::apkfix::ArscFix::new();
    for (name, _) in &layouts {
        arsc.add_layout(name);
    }
    arsc.add_string("hello", "Olá do XML");
    arsc.add_string("clicked", "clicado via XML");
    arsc.add_string("din", "texto dinâmico");
    arsc.add_id("tv");
    arsc.add_id("btn");
    let extras: Vec<(&str, Vec<u8>)> = layouts
        .iter()
        .map(|(name, node)| {
            (
                format!("res/layout/{name}.xml").leak() as &'static str,
                common::apkfix::build_axml(node),
            )
        })
        .collect();
    build_apk("com.test.xml", "LMain;", arsc.build(), extras)
}

// ═════════════════ M3.2 DoD: XML infla e responde a toque ═════════════════

/// App trivial com layout XML: infla via setContentView(R.layout.main),
/// textos resolvem pelo arsc, ids viram @id/nome no dump e o botão com
/// android:onClick dispara o método da activity no toque.
#[test]
fn layout_xml_inflates_and_touch_fires_onclick() {
    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);

    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs.activity_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // onCreate(Bundle): setContentView(R.layout.activity_main)
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            // regs=4, ins=2 → this=v2, bundle=v3
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    // onBtn(View): findViewById(R.id.tv).setText("clicado via XML")
    let s_clicked = b.intern("clicado via XML") as u16;
    b.direct(
        main,
        "onBtn",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            // this=v3, view=v4; locais v0..v2
            let mut u = op31i(0x14, 1, ID_TV);
            u.extend(op35c(0x6E, 2, refs.find_view, [3, 1, 0, 0, 0]));
            u.extend(op11x(0x0C, 0)); // move-result-object v0
            u.extend(op21c(0x1A, 1, s_clicked));
            u.extend(op35c(0x6E, 2, refs.tv_set_text_cs, [0, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("APK sintético parseia");
    e.set_resources(apk);
    e.launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    // árvore inflada: LinearLayout vertical com os dois filhos, bounds do
    // layout mínimo (TextView 160px de altura = 80dp × DENSITY 2.0; Button 48)
    let dump = e.dump_ui();
    assert!(
        dump.contains("android/widget/LinearLayout @id/0 [0,0 720x208] vertical"),
        "dump: {dump}"
    );
    assert!(
        dump.contains("android/widget/TextView @id/tv [0,0 720x160] text=\"Olá do XML\""),
        "dump: {dump}"
    );
    assert!(
        dump.contains("android/widget/Button @id/btn [0,160 720x48] text=\"Clique aqui\""),
        "dump: {dump}"
    );

    // toque no TextView (sem listener/onClick) → nada
    let hit = e.touch_app(10, 10).expect("touch tv");
    assert!(!hit, "TextView sem onClick não aciona nada");

    // toque no botão ([0,160 720x48]) → android:onClick="onBtn" dispara
    let hit = e.touch_app(10, 170).expect("touch btn");
    assert!(hit, "botão deve acionar android:onClick");

    let dump = e.dump_ui();
    assert!(
        dump.contains("text=\"clicado via XML\""),
        "texto mudou: {dump}"
    );
}

#[test]
fn inflation_supports_custom_layout_params_with_noarg_constructor() {
    let layout = AxNode::new(
        "TextView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-2)),
        ],
        vec![],
    );
    let apk_bytes = build_fixture(vec![("custom_params", layout)]);
    let mut b = DexBuilder::new();
    b.class(
        "Landroidx/viewpager/widget/ViewPager;",
        "Landroid/view/ViewGroup;",
    );
    let params = b.class(
        "Landroidx/viewpager/widget/ViewPager$LayoutParams;",
        "Landroid/view/ViewGroup$LayoutParams;",
    );
    let p_size = b.proto_idx("V", vec!["I".into(), "I".into()]);
    let size_ctor = b.method_idx("Landroid/view/ViewGroup$LayoutParams;", p_size, "<init>");
    let mut code = op11n(0x12, 0, -1);
    code.extend(op11n(0x12, 1, -1));
    code.extend(op35c(0x70, 3, size_ctor, [2, 0, 1, 0, 0]));
    code.extend(op10x(0x0E));
    b.direct(
        params,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(3, 1, 3, code)),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK sintético parseia"));
    let parent = e
        .heap
        .alloc_instance("Landroidx/viewpager/widget/ViewPager;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroidx/viewpager/widget/ViewPager;", parent)
        .unwrap();

    let child =
        rd_vm::inflate::inflate_resource_into(&mut e, LAYOUT_MAIN as u32, Some(parent), false)
            .expect("layout params custom deve inflar");
    let Value::Obj(params) = e.heap.get_field(child, "mLayoutParams").unwrap() else {
        panic!("child must have custom LayoutParams")
    };
    assert_eq!(
        e.heap.class_of(params).unwrap(),
        "Landroidx/viewpager/widget/ViewPager$LayoutParams;"
    );
    assert_eq!(e.heap.get_field(params, "width").unwrap(), Value::Int(-1));
    assert_eq!(e.heap.get_field(params, "height").unwrap(), Value::Int(-2));
}

#[test]
fn scroll_progress_and_radio_shorthands_inflate_as_host_views() {
    let radio_group = AxNode::new(
        "RadioGroup",
        vec![],
        vec![AxNode::new("RadioButton", vec![], vec![])],
    );
    let layout = AxNode::new(
        "ScrollView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![
            AxNode::new("TextView", vec![], vec![]),
            AxNode::new("ProgressBar", vec![], vec![]),
            radio_group,
        ],
    );
    let apk_bytes = build_fixture(vec![("scroll", layout)]);
    let mut e = engine_of(&DexBuilder::new());
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK sintético parseia"));

    let root = rd_vm::inflate::inflate_resource(&mut e, LAYOUT_MAIN as u32)
        .expect("<ScrollView> deve inflar");
    assert_eq!(
        e.heap.class_of(root).unwrap(),
        "Landroid/widget/ScrollView;"
    );
    let children = match e.fw.get(root) {
        Some(HostObj::View { children, .. }) => children,
        other => panic!("ScrollView deve usar o modelo host ViewGroup: {other:?}"),
    };
    assert_eq!(children.len(), 3);
    assert_eq!(
        e.heap.class_of(children[0]).unwrap(),
        "Landroid/widget/TextView;"
    );
    assert_eq!(
        e.heap.class_of(children[1]).unwrap(),
        "Landroid/widget/ProgressBar;"
    );
    let radio_group = children[2];
    assert_eq!(
        e.heap.class_of(radio_group).unwrap(),
        "Landroid/widget/RadioGroup;"
    );
    let radio_buttons = match e.fw.get(radio_group) {
        Some(HostObj::View { children, .. }) => children,
        other => panic!("RadioGroup deve usar o modelo host LinearLayout: {other:?}"),
    };
    assert_eq!(radio_buttons.len(), 1);
    assert_eq!(
        e.heap.class_of(radio_buttons[0]).unwrap(),
        "Landroid/widget/RadioButton;"
    );
}

#[test]
fn inflater_preserves_linear_layout_child_weights() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    let layout = AxNode::new(
        "LinearLayout",
        vec![
            a(true, "orientation", AxVal::Int(0)),
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-2)),
        ],
        [1, 2]
            .into_iter()
            .map(|weight| {
                AxNode::new(
                    "TextView",
                    vec![
                        a(true, "layout_width", AxVal::Int(0)),
                        a(true, "layout_height", AxVal::Int(-1)),
                        a(true, "layout_weight", AxVal::Int(weight)),
                    ],
                    vec![],
                )
            })
            .collect(),
    );
    let apk_bytes = build_fixture(vec![("activity_main", layout)]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));
    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();
    let root = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        "(ILandroid/view/ViewGroup;)Landroid/view/View;",
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Null],
    )
    .unwrap()
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    let children = match e.fw.get(root).unwrap() {
        HostObj::View { children, .. } => children,
        other => panic!("expected LinearLayout host view, got {other:?}"),
    };
    let weights: Vec<_> = children
        .iter()
        .map(|child| {
            let Value::Obj(params) = e.heap.get_field(*child, "mLayoutParams").unwrap() else {
                panic!("inflated child must retain layout params")
            };
            match e.heap.get_field(params, "weight").unwrap() {
                Value::Float(weight) => weight,
                other => panic!("weight must be stored as float, got {other:?}"),
            }
        })
        .collect();
    assert_eq!(weights, vec![1.0, 2.0]);
}

/// LayoutInflater.inflate(resource, root) infla o layout do APK, mantém a
/// árvore host e respeita a semântica do overload explícito attachToRoot.
#[test]
fn layout_inflater_inflate_resource_attaches_only_when_requested() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    const INFLATE_ROOT: &str = "(ILandroid/view/ViewGroup;)Landroid/view/View;";
    const INFLATE_ATTACH: &str = "(ILandroid/view/ViewGroup;Z)Landroid/view/View;";

    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));

    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();
    let detached = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE_ROOT,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Null],
    )
    .unwrap()
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    assert_eq!(
        e.heap.class_of(detached).unwrap(),
        "Landroid/widget/LinearLayout;"
    );
    assert!(
        matches!(e.fw.get(detached), Some(HostObj::View { children, .. }) if children.len() == 2)
    );

    let parent = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout;".to_string(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/FrameLayout;", parent)
        .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "inflate",
            INFLATE_ROOT,
            inflater,
            &[Value::Int(LAYOUT_MAIN), Value::Obj(parent)],
        )
        .unwrap(),
        Some(Value::Obj(parent)),
        "two-argument overload attaches and returns the supplied root"
    );
    let first_inflated = match e.fw.get(parent).unwrap() {
        HostObj::View { children, .. } => children[0],
        _ => panic!("expected host ViewGroup"),
    };
    assert!(
        matches!(e.fw.get(first_inflated), Some(HostObj::View { children, .. }) if children.len() == 2)
    );

    let returned = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE_ATTACH,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Obj(parent), Value::Int(0)],
    )
    .unwrap()
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    assert_ne!(returned, parent, "attachToRoot=false returns the new tree");
    assert!(
        matches!(e.fw.get(parent), Some(HostObj::View { children, .. }) if children.len() == 1)
    );

    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "inflate",
            INFLATE_ATTACH,
            inflater,
            &[Value::Int(LAYOUT_MAIN), Value::Obj(parent), Value::Int(1)],
        )
        .unwrap(),
        Some(Value::Obj(parent)),
        "attachToRoot=true attaches and returns the supplied root"
    );
    assert!(
        matches!(e.fw.get(parent), Some(HostObj::View { children, .. }) if children.len() == 2)
    );
}

/// `<include>` é expandido; roots `<merge>` são anexados diretamente ao
/// ViewGroup, sem criar uma view artificial para a tag de inclusão.
#[test]
fn layout_inflater_expands_include_and_merge_roots() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    const INFLATE_ROOT: &str = "(ILandroid/view/ViewGroup;)Landroid/view/View;";
    const INFLATE_ATTACH: &str = "(ILandroid/view/ViewGroup;Z)Landroid/view/View;";
    const LAYOUT_INCLUDED: i32 = 0x7F01_0001;
    const LAYOUT_ROOT_MERGE: i32 = 0x7F01_0002;

    let include = AxNode::new(
        "include",
        vec![a(false, "layout", AxVal::Ref(LAYOUT_INCLUDED as u32))],
        vec![],
    );
    let host_layout = AxNode::new("FrameLayout", vec![], vec![include]);
    let included_merge = AxNode::new(
        "merge",
        vec![],
        vec![
            AxNode::new("TextView", vec![], vec![]),
            AxNode::new("Button", vec![], vec![]),
        ],
    );
    let root_merge = AxNode::new(
        "merge",
        vec![],
        vec![
            AxNode::new("TextView", vec![], vec![]),
            AxNode::new("Button", vec![], vec![]),
        ],
    );
    let apk_bytes = build_fixture(vec![
        ("activity_main", host_layout),
        ("included", included_merge),
        ("root_merge", root_merge),
    ]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));
    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();

    let inflated = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE_ROOT,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Null],
    )
    .unwrap()
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    let included_children = match e.fw.get(inflated).unwrap() {
        HostObj::View { children, .. } => children.clone(),
        _ => panic!("expected included host ViewGroup"),
    };
    assert_eq!(included_children.len(), 2);
    assert_eq!(
        e.heap.class_of(included_children[0]).unwrap(),
        "Landroid/widget/TextView;"
    );
    assert_eq!(
        e.heap.class_of(included_children[1]).unwrap(),
        "Landroid/widget/Button;"
    );

    let parent = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout;".to_string(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/FrameLayout;", parent)
        .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "inflate",
            INFLATE_ATTACH,
            inflater,
            &[
                Value::Int(LAYOUT_ROOT_MERGE),
                Value::Obj(parent),
                Value::Int(1),
            ],
        )
        .unwrap(),
        Some(Value::Obj(parent))
    );
    assert!(
        matches!(e.fw.get(parent), Some(HostObj::View { children, .. }) if children.len() == 2)
    );
}

/// Sem Resources injetadas (Engine::new direto), setContentView(I) responde
/// RESOURCES_MISSING tipado — nunca silencioso, nunca pânico.
#[test]
fn set_content_view_resid_without_resources_is_typed_error() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b); // SEM set_resources
    let err = e
        .launch_app("LMain;", "com.test.xml", Vec::new())
        .expect_err("deve falhar tipado");
    match err {
        VmExit::Error(rde) => assert_eq!(rde.code, "RESOURCES_MISSING", "{rde}"),
        other => panic!("esperava VmExit::Error, veio {other:?}"),
    }
}

/// findViewById + TextView.setText(I) (resid) + Activity.getString(I)
/// resolvem pelo resources.arsc.
#[test]
fn find_by_id_set_text_resid_and_get_string() {
    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);

    let res_cls = b.class("LRes;", "Ljava/lang/Object;");
    let _f = b.field_idx("LRes;", "Ljava/lang/String;", "s");
    b.static_field(res_cls, "s", "Ljava/lang/String;", SVal::Int(0));
    let f_s = b.find_field("LRes;", "Ljava/lang/String;", "s") as u16;

    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            // this=v4, bundle=v5; locais v0..v3
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [4, 1, 0, 0, 0]));
            // tv = findViewById(R.id.tv)
            u.extend(op31i(0x14, 1, ID_TV));
            u.extend(op35c(0x6E, 2, refs.find_view, [4, 1, 0, 0, 0]));
            u.extend(op11x(0x0C, 0)); // v0 = tv
                                      // tv.setText(R.string.clicked) — setText(int resId)
            u.extend(op31i(0x14, 1, S_CLICKED));
            u.extend(op35c(0x6E, 2, refs.tv_set_text_i, [0, 1, 0, 0, 0]));
            // LRes.s = getString(R.string.din)
            u.extend(op31i(0x14, 1, S_DIN));
            u.extend(op35c(0x6E, 2, refs.get_string, [4, 1, 0, 0, 0]));
            u.extend(op11x(0x0C, 2)); // v2 = string
            u.extend(op21c(0x69, 2, f_s)); // sput-object v2, LRes->s
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e.launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    // setText(I) substituiu o texto do TextView (que era @string/hello)
    let dump = e.dump_ui();
    assert!(
        dump.contains("android/widget/TextView @id/tv [0,0 720x160] text=\"clicado via XML\""),
        "setText(resid) deve resolver no arsc: {dump}"
    );
    // getString(I) devolveu a string do arsc
    let s = e
        .statics
        .get(&("LRes;".to_string(), "s".to_string()))
        .cloned();
    match s {
        Some(Value::Obj(r)) => {
            assert_eq!(e.heap.as_str(r).unwrap(), "texto dinâmico");
        }
        other => panic!("static s deveria ser string, veio {other:?}"),
    }
}

#[test]
fn coordinator_layout_children_receive_coordinator_layout_params() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    const INFLATE_ROOT: &str = "(ILandroid/view/ViewGroup;)Landroid/view/View;";
    const COORDINATOR: &str = "Landroidx/coordinatorlayout/widget/CoordinatorLayout;";
    const COORDINATOR_PARAMS: &str =
        "Landroidx/coordinatorlayout/widget/CoordinatorLayout$LayoutParams;";

    let layout = AxNode::new(
        "androidx.coordinatorlayout.widget.CoordinatorLayout",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "id", AxVal::Ref(ID_TV as u32)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-2)),
            ],
            vec![],
        )],
    );
    let apk_bytes = build_fixture(vec![("activity_main", layout)]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let coordinator = b.class(COORDINATOR, "Landroid/view/ViewGroup;");
    let params = b.class(
        COORDINATOR_PARAMS,
        "Landroid/view/ViewGroup$MarginLayoutParams;",
    );
    b.direct(
        coordinator,
        "<init>",
        "V",
        vec!["Landroid/content/Context;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 0, op10x(0x0e))),
    );
    b.direct(
        params,
        "<init>",
        "V",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(3, 3, 0, op10x(0x0e))),
    );

    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), vec![])
        .unwrap();
    e.fw.current_activity = Some(context);
    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();
    let Value::Obj(root) = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE_ROOT,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Null],
    )
    .unwrap()
    .expect("inflate returns the CoordinatorLayout") else {
        panic!("inflate should return a View");
    };
    assert_eq!(e.heap.class_of(root).unwrap(), COORDINATOR);

    let child = match e.fw.get(root) {
        Some(HostObj::View { children, .. }) if children.len() == 1 => children[0],
        _ => panic!("CoordinatorLayout should have one inflated child"),
    };
    let Value::Obj(params) = e
        .invoke_instance(
            child,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .unwrap()
    else {
        panic!("CoordinatorLayout child should have LayoutParams");
    };
    assert_eq!(e.heap.class_of(params).unwrap(), COORDINATOR_PARAMS);
    assert!(e.cp.is_subtype(
        COORDINATOR_PARAMS,
        "Landroid/view/ViewGroup$MarginLayoutParams;"
    ));
}

#[test]
fn appcompat_viewgroup_children_receive_their_dex_layout_params_subclass() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    const INFLATE_ROOT: &str = "(ILandroid/view/ViewGroup;)Landroid/view/View;";
    const ACTION_BAR: &str = "Landroidx/appcompat/widget/ActionBarOverlayLayout;";
    const ACTION_BAR_PARAMS: &str =
        "Landroidx/appcompat/widget/ActionBarOverlayLayout$LayoutParams;";

    let layout = AxNode::new(
        "androidx.appcompat.widget.ActionBarOverlayLayout",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-2)),
            ],
            vec![],
        )],
    );
    let apk_bytes = build_fixture(vec![("activity_main", layout)]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let action_bar = b.class(ACTION_BAR, "Landroid/view/ViewGroup;");
    let params = b.class(
        ACTION_BAR_PARAMS,
        "Landroid/view/ViewGroup$MarginLayoutParams;",
    );
    b.direct(
        action_bar,
        "<init>",
        "V",
        vec!["Landroid/content/Context;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 0, op10x(0x0e))),
    );
    b.direct(
        params,
        "<init>",
        "V",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(3, 3, 0, op10x(0x0e))),
    );

    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), vec![])
        .unwrap();
    e.fw.current_activity = Some(context);
    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();
    let Value::Obj(root) = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE_ROOT,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Null],
    )
    .unwrap()
    .expect("inflate returns the ActionBarOverlayLayout") else {
        panic!("inflate should return a View");
    };
    let child = match e.fw.get(root) {
        Some(HostObj::View { children, .. }) if children.len() == 1 => children[0],
        _ => panic!("ActionBarOverlayLayout should have one inflated child"),
    };
    let Value::Obj(params) = e
        .invoke_instance(
            child,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .unwrap()
    else {
        panic!("ActionBarOverlayLayout child should have LayoutParams");
    };
    assert_eq!(e.heap.class_of(params).unwrap(), ACTION_BAR_PARAMS);
    assert!(e.cp.is_subtype(
        ACTION_BAR_PARAMS,
        "Landroid/view/ViewGroup$MarginLayoutParams;"
    ));
}

/// The short Android ImageView tag resolves to the built-in platform class and
/// receives the same common View host state as fully-qualified ImageView.
#[test]
fn image_view_short_tag_inflates_as_platform_view() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const LI: &str = "Landroid/view/LayoutInflater;";
    const INFLATE: &str = "(ILandroid/view/ViewGroup;Z)Landroid/view/View;";
    let layout = AxNode::new(
        "ImageView",
        vec![a(true, "layout_width", AxVal::Int(-1))],
        vec![],
    );
    let apk_bytes = build_fixture(vec![("activity_main", layout)]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("APK"));
    let inflater = e.heap.alloc_instance(LI.to_string(), vec![]).unwrap();
    let parent = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("Landroid/widget/FrameLayout;", parent)
        .unwrap();
    let image = call_instance_intrinsic(
        &mut e,
        LI,
        "inflate",
        INFLATE,
        inflater,
        &[Value::Int(LAYOUT_MAIN), Value::Obj(parent), Value::Int(0)],
    )
    .unwrap()
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    assert_eq!(
        e.heap.class_of(image).unwrap(),
        "Landroid/widget/ImageView;"
    );
    assert!(matches!(e.fw.get(image), Some(HostObj::View { .. })));
    let Value::Obj(params) = e
        .invoke_instance(
            image,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .unwrap()
    else {
        panic!("inflate(resource, parent, false) must assign LayoutParams");
    };
    assert_eq!(
        e.heap.class_of(params).unwrap(),
        "Landroid/widget/FrameLayout$LayoutParams;"
    );
    assert!(e.cp.is_subtype(
        "Landroid/widget/FrameLayout$LayoutParams;",
        "Landroid/view/ViewGroup$MarginLayoutParams;"
    ));
    assert_eq!(
        e.heap.get_field(params, "bottomMargin").unwrap(),
        Value::Int(0)
    );
    assert!(matches!(
        e.fw.get(parent),
        Some(HostObj::View { children, .. }) if children.is_empty()
    ));
}

/// A shorthand widget outside the modeled set still fails with a typed error.
#[test]
fn unsupported_view_class_is_typed_error() {
    let bad = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        vec![AxNode::new(
            "SurfaceView",
            vec![a(true, "layout_width", AxVal::Int(-1))],
            vec![],
        )],
    );
    let apk_bytes = build_fixture(vec![("activity_main", main_layout()), ("bad", bad)]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_BAD);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    let err = e
        .launch_app("LMain;", "com.test.xml", Vec::new())
        .expect_err("SurfaceView fora do escopo M3 deve falhar tipado");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("SurfaceView"), "{rde}");
        }
        other => panic!("esperava VmExit::Error, veio {other:?}"),
    }
}

/// Visibilidade/enabled do XML chegam no estado host (hit-test respeita).
#[test]
fn xml_visibility_and_enabled_reach_host_state() {
    // LinearLayout com TextView INVISIBLE e Button DISABLED
    let layout = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        vec![
            AxNode::new(
                "TextView",
                vec![
                    a(true, "id", AxVal::Ref(ID_TV as u32)),
                    a(true, "text", AxVal::Ref(S_HELLO as u32)),
                    a(true, "visibility", AxVal::Int(2)), // GONE
                ],
                vec![],
            ),
            AxNode::new(
                "Button",
                vec![
                    a(true, "id", AxVal::Ref(ID_BTN as u32)),
                    a(true, "onClick", AxVal::Str("onBtn".into())),
                    a(true, "enabled", AxVal::Bool(false)),
                ],
                vec![],
            ),
        ],
    );
    let apk_bytes = build_fixture(vec![("activity_main", layout)]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        main,
        "onBtn",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 0, op10x(0x0E))),
    );

    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e.launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    let dump = e.dump_ui();
    // issue #46: GONE é estado PRÓPRIO (não INVISIBLE) e sai do layout —
    // o Button (irmão seguinte) sobe para y=0
    assert!(dump.contains("GONE"), "GONE é anotado no dump: {dump}");
    assert!(
        !dump.contains("INVISIBLE"),
        "GONE não deve ser confundido com INVISIBLE: {dump}"
    );
    assert!(dump.contains("DISABLED"), "enabled=false marca: {dump}");
    let btn_line = dump
        .lines()
        .find(|l| l.contains("Button"))
        .expect("button no dump");
    assert!(
        btn_line.contains("[0,0"),
        "GONE não ocupa espaço — Button em y=0: {btn_line}"
    );
    assert!(btn_line.contains("DISABLED"), "enabled no dump: {btn_line}");

    // toque no botão desabilitado (agora em y=0, GONE não empurrou):
    // hit_test não devolve view disabled
    let hit = e.touch_app(10, 10).expect("touch");
    assert!(!hit, "botão disabled não deve acionar onClick");
}

/// Acessa o estado host direto (sanidade do fixture): a raiz é um
/// LinearLayout com 2 filhos após a inflação (sem tocar no dispatch).
#[test]
fn inflate_builds_host_tree_shape() {
    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    let act = e
        .launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    let Some(HostObj::Activity { window, .. }) = e.fw.get(act) else {
        panic!("activity host");
    };
    let w = window.expect("window");
    let Some(HostObj::Window { content }) = e.fw.get(w) else {
        panic!("window host");
    };
    let root = content.expect("content");
    let Some(HostObj::View {
        children,
        orientation,
        id,
        ..
    }) = e.fw.get(root)
    else {
        panic!("raiz é view host");
    };
    assert_eq!(children.len(), 2, "LinearLayout com 2 filhos");
    assert_eq!(*orientation, 1, "vertical do XML");
    assert_eq!(*id, 0, "raiz sem id");
    let _ = act;
}

// ── issue #40: cap de profundidade do LayoutInflater (AXML hostil)

/// AXML aninhado além do cap (600 níveis no fixture; cap host = 128) falha
/// TIPADO (INFLATE) — antes, a recursão da stack Rust podia abortar o processo.
#[test]
fn hostile_deep_axml_is_typed_error_not_abort() {
    // cadeia de 600 LinearLayouts com Button na ponta
    let mut cur = AxNode::new("Button", vec![], vec![]);
    for _ in 0..600 {
        cur = AxNode::new(
            "LinearLayout",
            vec![a(true, "orientation", AxVal::Int(1))],
            vec![cur],
        );
    }
    let apk_bytes = build_fixture(vec![("activity_main", main_layout()), ("deep", cur)]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LDeep;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_BAD); // reusa o slot do layout extra
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    let err = e
        .launch_app("LDeep;", "com.test.deep", Vec::new())
        .expect_err("AXML profundo deve falhar tipado (cap 512), nunca abortar");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("níveis"), "{rde}");
        }
        other => panic!("esperava VmExit::Error (INFLATE), veio {other:?}"),
    }
}

// ── issue #45: caps de layout hostil (contagem de views + dimensão)

/// Layout com 10.100 views → INFLATE tipado (cap de contagem) — nunca hang
/// nem abort; o layout pós-inflação é O(N) único.
#[test]
fn hostile_wide_layout_is_typed_error() {
    let mut kids = Vec::new();
    for _ in 0..10_100 {
        kids.push(AxNode::new("Button", vec![], vec![]));
    }
    let wide = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        kids,
    );
    let apk_bytes = build_fixture(vec![("activity_main", main_layout()), ("wide", wide)]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LWide;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_BAD);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    let err = e
        .launch_app("LWide;", "com.test.wide", Vec::new())
        .expect_err("10.100 views deve falhar tipada (cap de contagem)");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("views"), "{rde}");
        }
        other => panic!("esperava VmExit::Error (INFLATE), veio {other:?}"),
    }
}

/// layout_height = 0x7FFFFFFF (i32::MAX) → INFLATE tipado — antes: saturava
/// para i32::MAX e o layout/hit-test estouravam (panic debug, wrap release).
#[test]
fn layout_dimension_overflow_is_typed_error() {
    let bad = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        vec![AxNode::new(
            "Button",
            vec![a(true, "layout_height", AxVal::Int(0x7FFF_FFFF))],
            vec![],
        )],
    );
    let apk_bytes = build_fixture(vec![("activity_main", main_layout()), ("dim", bad)]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LDim;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_BAD);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    let err = e
        .launch_app("LDim;", "com.test.dim", Vec::new())
        .expect_err("dimensão 0x7FFFFFFF deve falhar tipada");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("viewport"), "{rde}");
        }
        other => panic!("esperava VmExit::Error (INFLATE), veio {other:?}"),
    }
}

// ── issue #47: fixture fiel ao aapt (typeSpec, 2 configs, layout-land)

/// resources.arsc com 2 configs (default + locale "pt") e typeSpec: o parser
/// entrega as DUAS variantes no modelo e o resolve sem config-alvo escolhe a
/// default (menos específica — comportamento documentado; a escolha por
/// device-config é M4).
#[test]
fn arsc_two_configs_parse_and_resolve() {
    let mut arsc = common::apkfix::ArscFix::new();
    let _layout = arsc.add_layout("activity_main");
    let greet = arsc.add_string("greet", "Ola default");
    arsc.add_string_lang("greet", "Ola pt", "pt");
    let tv = arsc.add_id("tv");
    let node = AxNode::new(
        "LinearLayout",
        vec![],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "id", AxVal::Ref(tv)),
                a(true, "text", AxVal::Ref(greet)),
            ],
            vec![],
        )],
    );
    let extras = vec![(
        "res/layout/activity_main.xml",
        common::apkfix::build_axml(&node),
    )];
    let apk_bytes = build_apk("com.test.cfg", "LMain;", arsc.build(), extras);

    // parse direto do arsc: o res_id de "greet" tem 2 entradas por config
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("apk parseia (typeSpec não quebra)");
    let a = apk.arsc.as_ref().expect("arsc presente");
    let pkg = a.packages.first().expect("package");
    let entries = pkg.entries.get(&greet).expect("entry greet");
    assert_eq!(
        entries.len(),
        2,
        "2 configs para o mesmo res_id: {entries:?}"
    );
    let default = entries
        .iter()
        .find(|e| e.config.is_default())
        .expect("default");
    let specific = entries.iter().find(|e| !e.config.is_default()).expect("pt");
    assert_eq!(default.string.as_deref(), Some("Ola default"));
    assert_eq!(specific.string.as_deref(), Some("Ola pt"));
    assert_eq!(specific.config.language.as_deref(), Some("pt"));
    // resolve sem config-alvo → default (menos específica)
    assert_eq!(a.resolve_string(greet).as_deref(), Some("Ola default"));
}

/// Fallback de config: SEM res/layout/activity_main.xml, COM
/// res/layout-land/activity_main.xml — o layout_entry varre res/layout-<config>/
/// (e NÃO casa res/layoutfoo/ — issue #46). O layout da land infla e o dump
/// mostra o texto do TextView (prova que a ENTRADA certa foi inflada).
#[test]
fn layout_land_fallback_resolves() {
    let mut arsc = common::apkfix::ArscFix::new();
    let _layout = arsc.add_layout("activity_main");
    let tv = arsc.add_id("tv_land");
    let s_land = arsc.add_string("land_txt", "texto da land");
    let node = AxNode::new(
        "LinearLayout",
        vec![],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "id", AxVal::Ref(tv)),
                a(true, "text", AxVal::Ref(s_land)),
            ],
            vec![],
        )],
    );
    // só a variante land existe (canonical res/layout/ AUSENTE)
    let extras = vec![(
        "res/layout-land/activity_main.xml",
        common::apkfix::build_axml(&node),
    )];
    let apk_bytes = build_apk("com.test.land", "LLand;", arsc.build(), extras);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LLand;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e.launch_app("LLand;", "com.test.land", Vec::new())
        .expect("launch via fallback land");
    let dump = e.dump_ui();
    assert!(
        dump.contains("texto da land"),
        "fallback res/layout-land/ deve resolver: {dump}"
    );
}

// ── M4: view_tree estruturada (fonte do uiautomator dump + screenshot)

/// DoD M4 "get_ui_tree com ids certos": a árvore estruturada da activity tem
/// resource-id no formato uiautomator (package:id/nome), bounds do layout
/// (TextView 80dp → 160px no topo), text resolvido do arsc e classes certas.
#[test]
fn m4_view_tree_has_ids_and_bounds() {
    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);

    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e.launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    let tree = e.view_tree().expect("árvore de UI da activity");
    // raiz: LinearLayout full-window
    assert_eq!(tree.class, "android.widget.LinearLayout");
    assert_eq!(
        tree.bounds,
        (0, 0, 720, 208),
        "raiz vertical: soma dos filhos (160+48)"
    );
    // filho 1: TextView com id resolvido + bounds do layout_height=80dp→160px
    let tv = &tree.children[0];
    assert_eq!(tv.class, "android.widget.TextView");
    assert_eq!(
        tv.resource_id.as_deref(),
        Some("com.test:id/tv"),
        "issue #54: namespace = package do ARSC (fonte da nomeação; o fixture\n         declara com.test no arsc e com.test.xml no manifest)"
    );
    assert_eq!(tv.bounds, (0, 0, 720, 160), "80dp × DENSITY 2.0");
    assert_eq!(tv.text, "Olá do XML", "text resolvido do arsc");
    // filho 2: Button clickable (android:onClick)
    let btn = &tree.children[1];
    assert_eq!(btn.resource_id.as_deref(), Some("com.test:id/btn"));
    assert!(btn.clickable, "onClick no XML → clickable=true");
    assert_eq!(btn.text, "Clique aqui");
}

/// DoD M4 "screenshot correto": o renderer pinta a árvore REAL da activity —
/// TextView branco no bloco superior, Button cinza com borda de clickable no
/// bloco seguinte, fundo fora da raiz; e o XML uiautomator carrega os ids.
/// rd-render aqui via dev-dependency (ciclo dev-dependency é permitido).
#[test]
fn m4_screenshot_renders_real_tree() {
    let apk_bytes = build_fixture(vec![("activity_main", main_layout())]);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e.launch_app("LMain;", "com.test.xml", Vec::new())
        .expect("launch");

    let tree = e.view_tree().expect("árvore");
    let xml = rd_render::uiautomator_xml(&tree, "com.test.xml").expect("dump ok");
    assert!(xml.contains("resource-id=\"com.test:id/tv\""), "{xml}");
    assert!(xml.contains("resource-id=\"com.test:id/btn\""), "{xml}");

    let mut fb = rd_render::render_snapshot(&tree);
    // The VM draw-list is transparent over the host-rendered tree; it must not
    // replace the host snapshot with a white full-screen canvas.
    let ops = e.draw_frame().expect("DEX overlay draw pass");
    if ops > 0 && e.has_frame() {
        let (w, h) = (fb.w, fb.h);
        e.rasterize_frame(&mut fb.px, w, h);
    }
    let px = |x: i32, y: i32| {
        let i = ((y as usize) * fb.w + x as usize) * 3;
        (fb.px[i], fb.px[i + 1], fb.px[i + 2])
    };
    let png = fb.to_png();
    assert_eq!(
        &png[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "PNG válido"
    );
    // TextView (0..720 × 0..160): branco no centro (texto fica à esquerda)
    assert_eq!(px(600, 80), (255, 255, 255), "TextView branco");
    // Button (0..720 × 160..208): cinza de botão + borda de clickable
    assert_eq!(px(600, 180), (0xD6, 0xD7, 0xD8), "Button cinza");
    assert_eq!(px(360, 160), (0x60, 0x60, 0x60), "borda clickable");
    // fora da raiz (208px): fundo da window
    assert_eq!(px(360, 700), (0xF6, 0xF6, 0xF6), "fundo fora da raiz");
}

// ── issue #59: teto de dimensão POR EIXO (largura ≤720, altura ≤1440) ───────

/// Engine com activity que infla `layout` no onCreate (helper local).
fn engine_with_layout(
    apk_bytes: Vec<u8>,
    activity: &str,
    _pkg: &str,
    layout: u32,
) -> rd_vm::Engine {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = xml_refs(&mut b);
    let main = b.class(activity, "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, layout as i32);
            u.extend(op35c(0x6E, 2, refs.set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));
    e
}

/// issue #59: altura legal (400dp = 800px ≤ WINDOW_H 1440) é ACEITA — o teto
/// antigo usava WINDOW_W (720) nos dois eixos e rejeitava com INFLATE
/// falso-positivo; 721dp = 1442px excede POR ALTURA; largura 400dp = 800px
/// excede POR LARGURA (720).
#[test]
fn dimension_cap_is_per_axis() {
    let mut arsc = common::apkfix::ArscFix::new();
    let l_ok = arsc.add_layout("h_ok");
    let l_tall = arsc.add_layout("h_tall");
    let l_wide = arsc.add_layout("w_wide");
    let s_hi = arsc.add_string("hi", "alto");

    let container = |tv: AxNode| {
        AxNode::new(
            "LinearLayout",
            vec![
                a(true, "orientation", AxVal::Int(1)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            vec![tv],
        )
    };
    let tv_h = |raw: u32| {
        container(AxNode::new(
            "TextView",
            vec![
                a(true, "text", AxVal::Ref(s_hi)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Dim { raw }),
            ],
            vec![],
        ))
    };
    let tv_w = container(AxNode::new(
        "TextView",
        vec![
            a(true, "text", AxVal::Ref(s_hi)),
            a(
                true,
                "layout_width",
                AxVal::Dim {
                    raw: (400 << 8) | 1,
                },
            ), // 400dp
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![],
    ));

    let apk_bytes = build_apk(
        "com.test.axis",
        "LH;",
        arsc.build(),
        vec![
            ("res/layout/h_ok.xml", build_axml(&tv_h((400 << 8) | 1))),
            ("res/layout/h_tall.xml", build_axml(&tv_h((721 << 8) | 1))),
            ("res/layout/w_wide.xml", build_axml(&tv_w)),
        ],
    );

    // 400dp = 800px ≤ 1440 (altura): ACEITO (antes: INFLATE falso)
    let mut e = engine_with_layout(apk_bytes.clone(), "LH;", "com.test.axis", l_ok);
    e.launch_app("LH;", "com.test.axis", Vec::new())
        .expect("400dp=800px é altura legal (teto por eixo)");
    let tree = e.view_tree().expect("árvore");
    assert_eq!(tree.children[0].bounds.3, 800, "altura 800px aplicada");

    // 721dp = 1442px > 1440: INFLATE citando a ALTURA
    let mut e = engine_with_layout(apk_bytes.clone(), "LH;", "com.test.axis", l_tall);
    let err = e
        .launch_app("LH;", "com.test.axis", Vec::new())
        .expect_err("1442px excede WINDOW_H");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("1442"), "{rde}");
            assert!(rde.cause.contains("altura"), "{rde}");
        }
        other => panic!("esperava INFLATE, veio {other:?}"),
    }

    // largura 400dp = 800px > 720: INFLATE citando a LARGURA (eixo certo)
    let mut e = engine_with_layout(apk_bytes, "LH;", "com.test.axis", l_wide);
    let err = e
        .launch_app("LH;", "com.test.axis", Vec::new())
        .expect_err("800px de largura excede WINDOW_W");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("largura"), "{rde}");
        }
        other => panic!("esperava INFLATE, veio {other:?}"),
    }
}

// ── issue #61: @dimen/@bool por referência resolvem do ARSC ─────────────────

/// Resíduo da deferral "M4" da #46: layout_height=@dimen/x e enabled=@bool/x
/// RESOLVEM do arsc (typed values). Não resolve → INFLATE tipado — nunca
/// default silencioso.
#[test]
fn dimen_and_bool_references_resolve_from_arsc() {
    let mut arsc = common::apkfix::ArscFix::new();
    let l_main = arsc.add_layout("main");
    let s_hi = arsc.add_string("hi", "texto");
    let d_tall = arsc.add_dimen_dp("tall", 300); // 600px
    let _d_bad = arsc.add_dimen_dp("bad", 800); // 1600px > WINDOW_H
    let _s_str = arsc.add_string("nao_dimen", "ops"); // string não-dimen (não usada aqui)
    let b_off = arsc.add_bool("off", false);
    let d_px = arsc.add_dimen_dp("px", 100); // para width? (200px ≤ 720)

    let container = |kids: Vec<AxNode>| {
        AxNode::new(
            "LinearLayout",
            vec![
                a(true, "orientation", AxVal::Int(1)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            kids,
        )
    };
    // TextView com layout_height=@dimen/tall (REFERENCE) + Button enabled=@bool/off
    let main = container(vec![
        AxNode::new(
            "TextView",
            vec![
                a(true, "text", AxVal::Ref(s_hi)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Ref(d_tall)),
            ],
            vec![],
        ),
        AxNode::new(
            "Button",
            vec![
                a(true, "text", AxVal::Str("b".into())),
                a(true, "enabled", AxVal::Ref(b_off)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-2)),
            ],
            vec![],
        ),
    ]);
    let apk_bytes = build_apk(
        "com.test.refs",
        "LR;",
        arsc.build(),
        vec![("res/layout/main.xml", build_axml(&main))],
    );

    let mut e = engine_with_layout(apk_bytes, "LR;", "com.test.refs", l_main);
    e.launch_app("LR;", "com.test.refs", Vec::new())
        .expect("@dimen/@bool resolvem");
    let tree = e.view_tree().expect("árvore");
    assert_eq!(
        tree.children[0].bounds.3, 600,
        "layout_height=@dimen/tall → 300dp × 2.0 = 600px"
    );
    // enabled do Button: inspeciona o estado host
    let btn_ref = {
        let w =
            e.fw.objects
                .iter()
                .find_map(|(k, v)| matches!(v, HostObj::Window { .. }).then_some(*k))
                .expect("window");
        let content = match e.fw.get(w) {
            Some(HostObj::Window { content }) => content.expect("content"),
            _ => panic!("window"),
        };
        match e.fw.get(content) {
            Some(HostObj::View { children, .. }) => children[1],
            _ => panic!("raiz"),
        }
    };
    match e.fw.get(btn_ref) {
        Some(HostObj::View { enabled, .. }) => {
            assert!(!enabled, "enabled=@bool/off → false (não default true)");
        }
        _ => panic!("btn host"),
    }

    // NÃO resolve → INFLATE tipado:
    // (a) @dimen inexistente
    let mut arsc2 = common::apkfix::ArscFix::new();
    let l2 = arsc2.add_layout("main");
    let d_fantasma = 0x7F99_0001u32; // resid que não existe no arsc
    let bad = container(vec![AxNode::new(
        "TextView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Ref(d_fantasma)),
        ],
        vec![],
    )]);
    let apk2 = build_apk(
        "com.test.refs2",
        "LR2;",
        arsc2.build(),
        vec![("res/layout/main.xml", build_axml(&bad))],
    );
    let mut e = engine_with_layout(apk2, "LR2;", "com.test.refs2", l2);
    let err = e
        .launch_app("LR2;", "com.test.refs2", Vec::new())
        .expect_err("@dimen fantasma não pode virar default silencioso");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("referência não resolvida"), "{rde}");
        }
        other => panic!("esperava INFLATE, veio {other:?}"),
    }

    // (b) tipo errado (@string onde se esperava dimensão)
    let mut arsc3 = common::apkfix::ArscFix::new();
    let l3 = arsc3.add_layout("main");
    let s3 = arsc3.add_string("nao_dimen", "ops");
    let wrong = container(vec![AxNode::new(
        "TextView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Ref(s3)),
        ],
        vec![],
    )]);
    let apk3 = build_apk(
        "com.test.refs3",
        "LR3;",
        arsc3.build(),
        vec![("res/layout/main.xml", build_axml(&wrong))],
    );
    let mut e = engine_with_layout(apk3, "LR3;", "com.test.refs3", l3);
    let err = e
        .launch_app("LR3;", "com.test.refs3", Vec::new())
        .expect_err("@string em layout_height é erro tipado");
    match err {
        VmExit::Error(rde) => {
            assert_eq!(rde.code, "INFLATE", "{rde}");
            assert!(rde.cause.contains("esperado dimensão"), "{rde}");
        }
        other => panic!("esperava INFLATE, veio {other:?}"),
    }
    let _ = d_px;
}

// ── issue #61: seleção de config do layout NÃO é mais cega à ordem do ZIP ───

/// Device virtual é retrato/tema dia: com default + -land disponíveis, o
/// DEFAULT vence (antes: ordem do ZIP arbitrava); só -land → -land usado;
/// res/layoutfoo/ NUNCA é fallback (não é config).
#[test]
fn layout_config_selection_prefers_matching_device() {
    let node = |txt: &str| {
        AxNode::new(
            "LinearLayout",
            vec![a(true, "orientation", AxVal::Int(1))],
            vec![AxNode::new(
                "TextView",
                vec![a(true, "text", AxVal::Str(txt.into()))],
                vec![],
            )],
        )
    };

    // (a) default + land: retrato vence
    let mut arsc = common::apkfix::ArscFix::new();
    let l = arsc.add_layout("main");
    let apk = build_apk(
        "com.test.cfg2",
        "LC;",
        arsc.build(),
        vec![
            ("res/layout/main.xml", build_axml(&node("retrato"))),
            ("res/layout-land/main.xml", build_axml(&node("land"))),
        ],
    );
    let mut e = engine_with_layout(apk, "LC;", "com.test.cfg2", l);
    e.launch_app("LC;", "com.test.cfg2", Vec::new())
        .expect("default existe");
    let tree = e.view_tree().expect("árvore");
    assert_eq!(tree.children[0].text, "retrato", "device retrato → default");

    // (b) só land: land é usado
    let mut arsc = common::apkfix::ArscFix::new();
    let l = arsc.add_layout("main");
    let apk = build_apk(
        "com.test.cfg3",
        "LC;",
        arsc.build(),
        vec![("res/layout-land/main.xml", build_axml(&node("land")))],
    );
    let mut e = engine_with_layout(apk, "LC;", "com.test.cfg3", l);
    e.launch_app("LC;", "com.test.cfg3", Vec::new())
        .expect("land existe");
    let tree = e.view_tree().expect("árvore");
    assert_eq!(tree.children[0].text, "land", "só -land → -land");

    // (c) res/layoutfoo/ NÃO é config — INFLATE tipado (issue #66: probe
    // negativo do fallback promovido a teste)
    let mut arsc = common::apkfix::ArscFix::new();
    let l = arsc.add_layout("main");
    let apk = build_apk(
        "com.test.cfg4",
        "LC;",
        arsc.build(),
        vec![("res/layoutfoo/main.xml", build_axml(&node("foo")))],
    );
    let mut e = engine_with_layout(apk, "LC;", "com.test.cfg4", l);
    let err = e
        .launch_app("LC;", "com.test.cfg4", Vec::new())
        .expect_err("res/layoutfoo não é fallback de config");
    match err {
        VmExit::Error(rde) => assert_eq!(rde.code, "INFLATE", "{rde}"),
        other => panic!("esperava INFLATE, veio {other:?}"),
    }
}

// ── issue #66: attr.res_id populado do resource map (probe promovido) ───────

/// O build_axml do fixture emite resource map (0x0180) — o parser do rd-apk
/// popula XmlAttribute.res_id para attrs android:* (base da resolução de
/// referências tipadas do inflate).
#[test]
fn android_attrs_carry_res_id_from_resource_map() {
    let node = AxNode::new(
        "TextView",
        vec![
            a(true, "text", AxVal::Str("x".into())),
            a(true, "layout_width", AxVal::Int(-1)),
        ],
        vec![],
    );
    let axml = build_axml(&node);
    let doc = rd_apk::axml::parse(&axml).expect("axml parseia");
    let attrs = &doc.root.attrs;
    assert!(attrs.len() >= 2, "2 attrs android:*");
    for at in attrs {
        assert!(at.is_android(), "attr android namespace");
        assert!(
            at.res_id.is_some(),
            "res_id do resource map presente para {:?}",
            at.name
        );
        // ids do fixture são plausíveis (0x0101_0000+n)
        assert!(at.res_id.unwrap() & 0xFFFF_0000 == 0x0101_0000);
    }
}
