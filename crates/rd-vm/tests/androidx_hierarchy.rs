//! S7 (Apêndice D) — hierarquias builtin AndroidX/Material no classpath
//! (hook mínimo em `builtin_hierarchy`/`builtin_interfaces`) e o efeito
//! ponta a ponta: layouts AXML com tags AndroidX inflam como views host
//! mesmo SEM o DEX das bibliotecas no APK, e os LayoutParams gerados para
//! filhos de containers AndroidX têm o subtipo que o código das libs
//! espera encontrar nos casts.

#[macro_use]
mod common;

use common::apkfix::{a, build_apk, AxNode, AxVal};
use common::*;
use rd_vm::framework::HostObj;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

pub const ACC_CONSTRUCTOR: u32 = 0x1_0000;

const LAYOUT_MAIN: i32 = 0x7F01_0000;
const ID_TV: i32 = 0x7F03_0000;
const ID_BTN: i32 = 0x7F03_0001;

const APPCOMPAT_TV: &str = "Landroidx/appcompat/widget/AppCompatTextView;";
const MATERIAL_BUTTON: &str = "Lcom/google/android/material/button/MaterialButton;";
const CONSTRAINT: &str = "Landroidx/constraintlayout/widget/ConstraintLayout;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}
fn obj(e: &mut Engine, class: &str) -> u32 {
    e.heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}

// ── Hierarquias: subtipagem sem DEX das libs ────────────────────────────
#[test]
fn androidx_widgets_are_framework_subtypes() {
    let e = engine();
    let cases = [
        (APPCOMPAT_TV, "Landroid/widget/TextView;"),
        (APPCOMPAT_TV, "Landroid/view/View;"),
        (
            "Landroidx/appcompat/widget/AppCompatEditText;",
            "Landroid/widget/EditText;",
        ),
        (
            "Landroidx/appcompat/widget/AppCompatButton;",
            "Landroid/widget/Button;",
        ),
        (
            "Landroidx/appcompat/widget/AppCompatCheckBox;",
            "Landroid/widget/CompoundButton;",
        ),
        (
            "Landroidx/appcompat/widget/Toolbar;",
            "Landroid/view/ViewGroup;",
        ),
        (CONSTRAINT, "Landroid/view/ViewGroup;"),
        (
            "Landroidx/cardview/widget/CardView;",
            "Landroid/widget/FrameLayout;",
        ),
        (
            "Landroidx/viewpager2/widget/ViewPager2;",
            "Landroid/view/ViewGroup;",
        ),
        (
            MATERIAL_BUTTON,
            "Landroidx/appcompat/widget/AppCompatButton;",
        ),
        (MATERIAL_BUTTON, "Landroid/widget/Button;"),
        (
            "Lcom/google/android/material/card/MaterialCardView;",
            "Landroidx/cardview/widget/CardView;",
        ),
        (
            "Lcom/google/android/material/textfield/TextInputLayout;",
            "Landroid/widget/LinearLayout;",
        ),
        (
            "Lcom/google/android/material/chip/Chip;",
            "Landroid/widget/CheckBox;",
        ),
        (
            "Landroidx/constraintlayout/widget/ConstraintLayout$LayoutParams;",
            "Landroid/view/ViewGroup$MarginLayoutParams;",
        ),
        (
            "Landroidx/viewpager/widget/ViewPager$LayoutParams;",
            "Landroid/view/ViewGroup$LayoutParams;",
        ),
    ];
    for (sub, sup) in cases {
        assert!(
            e.cp.is_subtype(sub, sup),
            "{sub} deveria ser subtipo de {sup}"
        );
    }
    // Negativos: a tabela não pode inventar relações.
    assert!(!e.cp.is_subtype(APPCOMPAT_TV, "Landroid/widget/Button;"));
    assert!(!e.cp.is_subtype(CONSTRAINT, "Landroid/widget/LinearLayout;"));
    assert!(!e.cp.is_subtype(
        "Lcom/google/android/material/slider/Slider;",
        "Landroid/widget/ProgressBar;"
    ));
}

#[test]
fn appcompat_activity_chain_and_lifecycle_interfaces() {
    let e = engine();
    let act = "Landroidx/appcompat/app/AppCompatActivity;";
    for sup in [
        "Landroidx/fragment/app/FragmentActivity;",
        "Landroidx/activity/ComponentActivity;",
        "Landroid/app/Activity;",
        "Landroid/content/Context;",
        "Landroidx/lifecycle/LifecycleOwner;",
        "Landroidx/lifecycle/ViewModelStoreOwner;",
        "Landroidx/savedstate/SavedStateRegistryOwner;",
    ] {
        assert!(e.cp.is_subtype(act, sup), "AppCompatActivity <: {sup}");
    }
    assert!(e.cp.is_subtype(
        "Landroidx/fragment/app/Fragment;",
        "Landroidx/lifecycle/LifecycleOwner;"
    ));
    // A cadeia não inverte: Activity NÃO é uma AppCompatActivity.
    assert!(!e.cp.is_subtype("Landroid/app/Activity;", act));
}

// ── attach_host_state é subtype-driven ─────────────────────────────────
#[test]
fn attach_host_state_recognizes_androidx_views_and_activities() {
    let mut e = engine();
    let tv = obj(&mut e, APPCOMPAT_TV);
    e.attach_host_state(APPCOMPAT_TV, tv).unwrap();
    match e.fw.objects.get(&tv) {
        Some(HostObj::View { .. }) => {}
        other => panic!("AppCompatTextView devia virar View host: {other:?}"),
    }
    let root = obj(&mut e, CONSTRAINT);
    e.attach_host_state(CONSTRAINT, root).unwrap();
    assert!(matches!(
        e.fw.objects.get(&root),
        Some(HostObj::View { .. })
    ));

    let act = obj(&mut e, "Landroidx/appcompat/app/AppCompatActivity;");
    e.attach_host_state("Landroidx/appcompat/app/AppCompatActivity;", act)
        .unwrap();
    assert!(
        matches!(e.fw.objects.get(&act), Some(HostObj::Activity { .. })),
        "AppCompatActivity devia ganhar estado de Activity"
    );
}

// ── Inflate ponta a ponta com tags AndroidX (sem DEX das libs) ──────────
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
    b.class("Landroid/app/Activity;", "Ljava/lang/Object;");
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
}

fn build_fixture(layout: AxNode) -> Vec<u8> {
    let mut arsc = common::apkfix::ArscFix::new();
    arsc.add_layout("activity_main");
    arsc.add_string("hello", "Olá AndroidX");
    arsc.add_id("tv");
    arsc.add_id("btn");
    let extras: Vec<(&str, Vec<u8>)> = vec![(
        "res/layout/activity_main.xml",
        common::apkfix::build_axml(&layout),
    )];
    build_apk("com.test.ax", "LMain;", arsc.build(), extras)
}

/// DEX mínimo: Main extends Activity, onCreate faz setContentView e
/// onBtn(View) troca o texto via findViewById — o mesmo padrão de app_xml.
fn launch_with_layout(layout: AxNode) -> Engine {
    let apk_bytes = build_fixture(layout);
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let p_void = b.proto_idx("V", vec![]);
    let p_iv = b.proto_idx("V", vec!["I".to_string()]);
    let p_charseq = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".to_string()]);
    let p_ret_view = b.proto_idx("Landroid/view/View;", vec!["I".to_string()]);
    let activity_init = b.method_idx("Landroid/app/Activity;", p_void, "<init>");
    let set_content_i = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
    let find_view = b.method_idx("Landroid/app/Activity;", p_ret_view, "findViewById");
    let tv_set_text = b.method_idx("Landroid/widget/TextView;", p_charseq, "setText");

    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, activity_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, LAYOUT_MAIN);
            u.extend(op35c(0x6E, 2, set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let s_clicked = b.intern("clicado AndroidX") as u16;
    b.direct(
        main,
        "onBtn",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            let mut u = op31i(0x14, 1, ID_TV);
            u.extend(op35c(0x6E, 2, find_view, [3, 1, 0, 0, 0]));
            u.extend(op11x(0x0C, 0));
            u.extend(op21c(0x1A, 1, s_clicked));
            u.extend(op35c(0x6E, 2, tv_set_text, [0, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("APK sintético parseia");
    e.set_resources(apk);
    e.launch_app("LMain;", "com.test.ax", Vec::new())
        .expect("launch");
    e
}

#[test]
fn inflate_androidx_widgets_and_onclick() {
    // Raiz platform (LinearLayout vertical) com filhos AndroidX: as tags
    // FQCN só inflam porque is_subtype resolve pela tabela builtin — o APK
    // NÃO contém classe alguma de appcompat/material no DEX.
    let layout = AxNode::new(
        "LinearLayout",
        vec![
            a(true, "orientation", AxVal::Int(1)),
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![
            AxNode::new(
                "androidx.appcompat.widget.AppCompatTextView",
                vec![
                    a(true, "id", AxVal::Ref(ID_TV as u32)),
                    a(true, "text", AxVal::Str("Olá AndroidX".into())),
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Int(-2)),
                ],
                vec![],
            ),
            AxNode::new(
                "com.google.android.material.button.MaterialButton",
                vec![
                    a(true, "id", AxVal::Ref(ID_BTN as u32)),
                    a(true, "text", AxVal::Str("Toque".into())),
                    a(true, "onClick", AxVal::Str("onBtn".into())),
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Int(-2)),
                ],
                vec![],
            ),
        ],
    );
    let mut e = launch_with_layout(layout);
    let dump = e.dump_ui();
    assert!(
        dump.contains("androidx/appcompat/widget/AppCompatTextView"),
        "dump: {dump}"
    );
    assert!(dump.contains("text=\"Olá AndroidX\""), "dump: {dump}");
    assert!(
        dump.contains("com/google/android/material/button/MaterialButton"),
        "dump: {dump}"
    );

    // O MaterialButton é o segundo filho (abaixo do TextView de 48px).
    let hit = e.touch_app(10, 60).expect("touch");
    assert!(hit, "MaterialButton devia disparar android:onClick");
    let dump = e.dump_ui();
    assert!(dump.contains("text=\"clicado AndroidX\""), "dump: {dump}");
}

#[test]
fn inflate_constraint_root_generates_androidx_layout_params() {
    let layout = AxNode::new(
        "androidx.constraintlayout.widget.ConstraintLayout",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "androidx.appcompat.widget.AppCompatTextView",
            vec![
                a(true, "id", AxVal::Ref(ID_TV as u32)),
                a(true, "text", AxVal::Str("dentro do Constraint".into())),
                a(true, "layout_width", AxVal::Int(-2)),
                a(true, "layout_height", AxVal::Int(-2)),
            ],
            vec![],
        )],
    );
    let e = launch_with_layout(layout);
    let dump = e.dump_ui();
    assert!(
        dump.contains("androidx/constraintlayout/widget/ConstraintLayout"),
        "dump: {dump}"
    );

    // O filho recebe ConstraintLayout$LayoutParams (não MarginLayoutParams
    // genérico): é o que o cast do código da lib espera.
    let child =
        e.fw.objects
            .keys()
            .find(|r| e.heap.class_of(**r) == Ok(APPCOMPAT_TV))
            .copied()
            .expect("AppCompatTextView inflada");
    let params = match e.heap.get_field(child, "mLayoutParams") {
        Ok(Value::Obj(r)) => r,
        other => panic!("mLayoutParams: {other:?}"),
    };
    assert_eq!(
        e.heap.class_of(params).unwrap(),
        "Landroidx/constraintlayout/widget/ConstraintLayout$LayoutParams;"
    );
    // …e esse LayoutParams continua um MarginLayoutParams válido.
    assert!(e.cp.is_subtype(
        "Landroidx/constraintlayout/widget/ConstraintLayout$LayoutParams;",
        "Landroid/view/ViewGroup$MarginLayoutParams;"
    ));
}
