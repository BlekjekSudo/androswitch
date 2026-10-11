//! Testes de integração M3 — app trivial headless (lifecycle, views, touch,
//! Handler/Looper com clock virtual, navegação por Intent).
//! DEX sintético byte a byte (mesma política do vm_exec.rs).

#[macro_use]
mod common;

use common::*;
use rd_vm::value::Value;
pub const ACC_CONSTRUCTOR: u32 = 0x1_0000;

// ═════════════════ M3: app trivial headless (DoD) ═════════════════

/// Registra as classes de plataforma com suas superclasses (a herança de
/// nomes no DEX é o que alimenta is_subtype/attach no framework host).
fn register_platform_classes(b: &mut DexBuilder) {
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    b.class("Landroid/widget/Button;", "Landroid/widget/TextView;");
    b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/widget/FrameLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/os/Handler;", "Ljava/lang/Object;");
    b.class("Landroid/os/Looper;", "Ljava/lang/Object;");
    b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    b.class("Ljava/lang/Class;", "Ljava/lang/Object;");
}

fn platform_method_refs(b: &mut DexBuilder) -> PlatRefs {
    let p_void = b.proto_idx("V", vec![]);
    let p_i = b.proto_idx("V", vec!["I".to_string()]);
    let p_charseq = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".to_string()]);
    let p_view = b.proto_idx("V", vec!["Landroid/view/View;".to_string()]);
    let _p_view_ret_view = b.proto_idx("Landroid/view/View;", vec!["I".to_string()]);
    let p_listener = b.proto_idx("V", vec!["Landroid/view/View$OnClickListener;".to_string()]);
    let _p_bundle = b.proto_idx("V", vec!["Landroid/os/Bundle;".to_string()]);
    let p_runnable_j_z = b.proto_idx(
        "Z",
        vec!["Ljava/lang/Runnable;".to_string(), "J".to_string()],
    );
    let p_intent = b.proto_idx("V", vec!["Landroid/content/Intent;".to_string()]);
    let p_ctx_class = b.proto_idx(
        "V",
        vec![
            "Landroid/content/Context;".to_string(),
            "Ljava/lang/Class;".to_string(),
        ],
    );
    PlatRefs {
        ll_init: b.method_idx("Landroid/widget/LinearLayout;", p_void, "<init>"),
        set_orientation: b.method_idx("Landroid/widget/LinearLayout;", p_i, "setOrientation"),
        tv_init: b.method_idx("Landroid/widget/TextView;", p_void, "<init>"),
        btn_init: b.method_idx("Landroid/widget/Button;", p_void, "<init>"),
        set_text: b.method_idx("Landroid/widget/TextView;", p_charseq, "setText"),
        add_view: b.method_idx("Landroid/widget/LinearLayout;", p_view, "addView"),
        set_click: b.method_idx("Landroid/view/View;", p_listener, "setOnClickListener"),
        set_content: b.method_idx("Landroid/app/Activity;", p_view, "setContentView"),
        set_visibility: b.method_idx("Landroid/view/View;", p_i, "setVisibility"),
        handler_init: b.method_idx("Landroid/os/Handler;", p_void, "<init>"),
        post_delayed: b.method_idx("Landroid/os/Handler;", p_runnable_j_z, "postDelayed"),
        obj_init: b.method_idx("Ljava/lang/Object;", p_void, "<init>"),
        intent_init: b.method_idx("Landroid/content/Intent;", p_ctx_class, "<init>"),
        start_activity: b.method_idx("Landroid/app/Activity;", p_intent, "startActivity"),
        activity_init: b.method_idx("Landroid/app/Activity;", p_void, "<init>"),
    }
}

struct PlatRefs {
    ll_init: u16,
    set_orientation: u16,
    tv_init: u16,
    btn_init: u16,
    set_text: u16,
    add_view: u16,
    set_click: u16,
    set_content: u16,
    set_visibility: u16,
    handler_init: u16,
    post_delayed: u16,
    obj_init: u16,
    intent_init: u16,
    start_activity: u16,
    activity_init: u16,
}

/// DoD M3: app trivial (activity + botão + texto) roda ponta a ponta e
/// responde a toque simulado — o texto do TextView muda via onClick.
#[test]
fn app_trivial_lifecycle_and_touch() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);
    b.type_idx("Landroid/widget/LinearLayout;");
    b.type_idx("Landroid/widget/TextView;");
    b.type_idx("Landroid/widget/Button;");

    let cls = b.class("LMain;", "Landroid/app/Activity;");
    let _f_ll = b.field_idx("LMain;", "Landroid/widget/LinearLayout;", "ll");
    let _f_tv = b.field_idx("LMain;", "Landroid/widget/TextView;", "tv");
    b.instance_field(cls, "ll", "Landroid/widget/LinearLayout;");
    b.instance_field(cls, "tv", "Landroid/widget/TextView;");

    let s_ola = b.intern("Olá") as u16;
    let s_clique = b.intern("Clique") as u16;
    let s_clicado = b.intern("clicado") as u16;
    let t_ll = b.type_idx_cached("Landroid/widget/LinearLayout;");
    let t_tv = b.type_idx_cached("Landroid/widget/TextView;");
    let t_btn = b.type_idx_cached("Landroid/widget/Button;");
    let f_ll = b.find_field("LMain;", "Landroid/widget/LinearLayout;", "ll") as u16;
    let f_tv = b.find_field("LMain;", "Landroid/widget/TextView;", "tv") as u16;

    // <init>()V: super() → return
    b.direct(
        cls,
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

    // onCreate(Bundle)V: monta LinearLayout(vertical){TextView "Olá",
    // Button "Clique" com listener=this}, guarda refs de campo, setContentView
    b.direct(
        cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, t_ll); // v0 = new LinearLayout
            u.extend(op35c(0x70, 1, refs.ll_init, [0, 0, 0, 0, 0]));
            u.extend(op21s(0x13, 1, 1)); // v1 = 1 (vertical)
            u.extend(op35c(0x6E, 2, refs.set_orientation, [0, 1, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tv)); // v1 = new TextView
            u.extend(op35c(0x70, 1, refs.tv_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_ola)); // v2 = "Olá"
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op22c(0x5B, 1, 4, f_tv)); // this.tv = v1 (antes de reusar v1)
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0]));
            u.extend(op22c(0x5B, 0, 4, f_ll)); // this.ll = v0
            u.extend(op21c(0x22, 1, t_btn)); // v1 = new Button
            u.extend(op35c(0x70, 1, refs.btn_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_clique)); // v2 = "Clique"
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.set_click, [1, 4, 0, 0, 0])); // listener = this (v4)
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.set_content, [4, 0, 0, 0, 0])); // setContentView(this, ll)
            u.extend(op10x(0x0E));
            u
        })),
    );

    // onClick(View)V: ((TextView) this.tv).setText("clicado")
    b.direct(
        cls,
        "onClick",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 1, {
            // regs=3, ins=2 → this=v1, view=v2 (convenção ABI: ins no topo)
            let mut u = op22c(0x54, 1, 1, f_tv); // iget-object v1, v1(this), tv
            u.extend(op21c(0x1A, 2, s_clicado)); // v2 = "clicado"
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    let act = e
        .launch_app("LMain;", "com.exemplo.trivial", Vec::new())
        .expect("launch");

    // dump inicial: árvore com os dois widgets e bounds do layout vertical
    let dump = e.dump_ui();
    assert!(dump.contains("text=\"Olá\""), "dump: {dump}");
    assert!(dump.contains("text=\"Clique\""), "dump: {dump}");
    assert!(dump.contains("vertical"), "dump: {dump}");
    // TextView na linha 0, Button na linha 48 (48px de touch target)
    assert!(dump.contains("[0,0 720x48]"), "dump: {dump}");
    assert!(dump.contains("[0,48 720x48]"), "dump: {dump}");

    // toque no TextView (sem listener) → nada
    let hit = e.touch_app(10, 10).expect("touch tv");
    assert!(!hit, "TextView não tem listener");

    // toque no botão (10, 60 dentro de [0,48 720x48]) → onClick roda
    let hit = e.touch_app(10, 60).expect("touch btn");
    assert!(hit, "botão deve acionar o listener");

    let dump = e.dump_ui();
    assert!(dump.contains("text=\"clicado\""), "texto mudou: {dump}");
    let _ = act;
}

/// M3: Handler.postDelayed + clock virtual dispara o Runnable
#[test]
fn app_handler_post_delayed_runs_on_clock() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);

    let tick_cls = b.class("LTick;", "Ljava/lang/Object;");
    let _ft = b.field_idx("LTick;", "I", "t");
    b.static_field(tick_cls, "t", "I", SVal::Int(0));
    let f_t = b.find_field("LTick;", "I", "t") as u16;

    b.direct(
        tick_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs.obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // run(): t++
    b.direct(
        tick_cls,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x60, 0, f_t); // sget v0, LTick->t
            u.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1 (0xDB é div!)
            u.extend(op21c(0x67, 0, f_t)); // sput v0, LTick->t
            u.extend(op10x(0x0E));
            u
        })),
    );

    let main_cls = b.class("LMain2;", "Landroid/app/Activity;");
    b.type_idx("LTick;");
    let t_tick = b.type_idx_cached("LTick;");
    let p_void = b.proto_idx("V", vec![]);
    let m_tick_init = b.method_idx("LTick;", p_void, "<init>");
    b.direct(
        main_cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, b.type_idx_cached("Landroid/os/Handler;"));
            u.extend(op35c(0x70, 1, refs.handler_init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tick));
            u.extend(op35c(0x70, 1, m_tick_init, [1, 0, 0, 0, 0]));
            u.extend(op21s(0x16, 2, 100)); // const-wide/16 v2, 100L
            u.extend(op35c(0x6E, 4, refs.post_delayed, [0, 1, 2, 3, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.launch_app("LMain2;", "com.exemplo.tick", Vec::new())
        .expect("launch");

    let t_before = e
        .statics
        .get(&("LTick;".to_string(), "t".to_string()))
        .cloned();
    assert_eq!(t_before, Some(Value::Int(0)), "run ainda não disparou");
    // 50ms: ainda não venceu
    let ran = e.advance_clock(50).expect("clock");
    assert_eq!(ran, 0);
    // +50ms: vence o postDelayed(100)
    let ran = e.advance_clock(50).expect("clock");
    assert_eq!(ran, 1, "runnable deve disparar em t=100ms");
    let t_after = e
        .statics
        .get(&("LTick;".to_string(), "t".to_string()))
        .cloned();
    assert_eq!(t_after, Some(Value::Int(1)));
}

/// M3: navegação — Intent(Context, Class) via const-class + startActivity
#[test]
fn app_start_activity_navigates() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);

    // LSecond; — onCreate grava flag estática
    let second = b.class("LSecond;", "Landroid/app/Activity;");
    let _fs = b.field_idx("LSecond;", "I", "started");
    b.static_field(second, "started", "I", SVal::Int(0));
    let f_started = b.find_field("LSecond;", "I", "started") as u16;
    let _p_bundle = b.proto_idx("V", vec!["Landroid/os/Bundle;".to_string()]);
    b.direct(
        second,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 0, {
            let mut u = op11n(0x12, 0, 1); // const/4 v0, 1
            u.extend(op21c(0x67, 0, f_started)); // sput v0, LSecond->started
            u.extend(op10x(0x0E));
            u
        })),
    );

    // LNav; — botão que navega
    let refs2 = refs;
    let nav = b.class("LNav;", "Landroid/app/Activity;");
    let t_btn = b.type_idx("Landroid/widget/Button;");
    b.type_idx("LSecond;");
    let t_second = b.type_idx_cached("LSecond;");
    let s_go = b.intern("Go") as u16;
    let f_btn = {
        let _ = b.field_idx("LNav;", "Landroid/widget/Button;", "bt");
        b.instance_field(nav, "bt", "Landroid/widget/Button;");
        b.find_field("LNav;", "Landroid/widget/Button;", "bt") as u16
    };

    b.direct(
        nav,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs2.activity_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // onCreate: new Button "Go" + listener=this + (sem ContentView — o teste
    // usa performClick via touch? sem window não há hit-test → toca via
    // performClick path do touch_app? touch_app exige window; aqui setamos
    // o botão direto num FrameLayout para ter hit-test)
    let t_frame = b.type_idx("Landroid/widget/FrameLayout;");
    let p_void_nav = b.proto_idx("V", vec![]);
    let frame_init2 = b.method_idx("Landroid/widget/FrameLayout;", p_void_nav, "<init>");
    let p_frame_av = b.proto_idx("V", vec!["Landroid/view/View;".to_string()]);
    let frame_addview2 = b.method_idx("Landroid/widget/FrameLayout;", p_frame_av, "addView");
    b.direct(
        nav,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, t_frame); // v0 = FrameLayout
            u.extend(op35c(0x70, 1, frame_init2, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_btn));
            u.extend(op35c(0x70, 1, refs2.btn_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_go));
            u.extend(op35c(0x6E, 2, refs2.set_text, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs2.set_click, [1, 4, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, frame_addview2, [0, 1, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs2.set_content, [4, 0, 0, 0, 0]));
            u.extend(op22c(0x5B, 1, 4, f_btn)); // this.bt = v1
            u.extend(op10x(0x0E));
            u
        })),
    );
    // onClick: startActivity(new Intent(this, LSecond;))
    b.direct(
        nav,
        "onClick",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 1, {
            // regs=4, ins=2 → this=v2, view=v3; locais v0,v1
            let mut u = op21c(0x22, 0, b.type_idx_cached("Landroid/content/Intent;"));
            u.extend(op21c(0x1C, 1, t_second)); // const-class v1, LSecond;
            u.extend(op35c(0x70, 3, refs2.intent_init, [0, 2, 1, 0, 0]));
            u.extend(op35c(0x6E, 2, refs2.start_activity, [2, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.launch_app("LNav;", "com.exemplo.nav", Vec::new())
        .expect("launch");
    let hit = e.touch_app(10, 10).expect("touch"); // frame ocupa a window toda
    assert!(hit, "botão deve receber o toque");
    let started = e
        .statics
        .get(&("LSecond;".to_string(), "started".to_string()))
        .cloned();
    assert_eq!(started, Some(Value::Int(1)), "LSecond.onCreate deve rodar");
    assert!(!e.activity_is_finished());
}

/// issue #50: drain executa o menor deadline primeiro (não ordem de post) —
/// postDelayed(A, 1000) seguido de postDelayed(B, 500) → B roda primeiro.
#[test]
fn looper_drains_earliest_deadline_first() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);

    // LOrd; — contadores: ord (sequência global), aAt/bAt (ordem de cada run)
    let ord_cls = b.class("LOrd;", "Ljava/lang/Object;");
    b.static_field(ord_cls, "ord", "I", SVal::Int(0));
    b.static_field(ord_cls, "aAt", "I", SVal::Int(-1));
    b.static_field(ord_cls, "bAt", "I", SVal::Int(-1));
    let _fo = b.field_idx("LOrd;", "I", "ord");
    let _fa = b.field_idx("LOrd;", "I", "aAt");
    let _fb = b.field_idx("LOrd;", "I", "bAt");
    let f_ord = b.find_field("LOrd;", "I", "ord") as u16;
    let f_aat = b.find_field("LOrd;", "I", "aAt") as u16;
    let f_bat = b.find_field("LOrd;", "I", "bAt") as u16;

    let p_void = b.proto_idx("V", vec![]);
    // run() genérico: sget ord → sput <slot> → ord++ → sput ord
    let run_body = |slot: u16| {
        let mut u = op21c(0x60, 0, f_ord); // sget v0, ord
        u.extend(op21c(0x67, 0, slot)); // sput v0, <slot>
        u.extend(op22b(0xD8, 0, 0, 1)); // v0 += 1
        u.extend(op21c(0x67, 0, f_ord)); // sput v0, ord
        u.extend(op10x(0x0E));
        u
    };

    let ra = b.class("LRA;", "Ljava/lang/Object;");
    b.direct(
        ra,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs.obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        ra,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, run_body(f_aat))),
    );
    let rb = b.class("LRB;", "Ljava/lang/Object;");
    b.direct(
        rb,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs.obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        rb,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, run_body(f_bat))),
    );

    b.type_idx("LRA;");
    b.type_idx("LRB;");
    let t_ra = b.type_idx_cached("LRA;");
    let t_rb = b.type_idx_cached("LRB;");
    let m_ra_init = b.method_idx("LRA;", p_void, "<init>");
    let m_rb_init = b.method_idx("LRB;", p_void, "<init>");

    let main_cls = b.class("LMainOrd;", "Landroid/app/Activity;");
    b.direct(
        main_cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, b.type_idx_cached("Landroid/os/Handler;"));
            u.extend(op35c(0x70, 1, refs.handler_init, [0, 0, 0, 0, 0]));
            // postDelayed(RA, 1000) — postado PRIMEIRO, vence DEPOIS
            u.extend(op21c(0x22, 1, t_ra));
            u.extend(op35c(0x70, 1, m_ra_init, [1, 0, 0, 0, 0]));
            u.extend(op21s(0x16, 2, 1000)); // const-wide/16 v2, 1000L
            u.extend(op35c(0x6E, 4, refs.post_delayed, [0, 1, 2, 3, 0]));
            // postDelayed(RB, 500) — postado DEPOIS, vence ANTES
            u.extend(op21c(0x22, 1, t_rb));
            u.extend(op35c(0x70, 1, m_rb_init, [1, 0, 0, 0, 0]));
            u.extend(op21s(0x16, 2, 500)); // const-wide/16 v2, 500L
            u.extend(op35c(0x6E, 4, refs.post_delayed, [0, 1, 2, 3, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.launch_app("LMainOrd;", "com.exemplo.ord", Vec::new())
        .expect("launch");
    let ran = e.advance_clock(1000).expect("clock");
    assert_eq!(ran, 2, "ambos vencidos em t=1000");
    let get = |n: &str| {
        e.statics
            .get(&("LOrd;".to_string(), n.to_string()))
            .cloned()
            .unwrap()
    };
    assert_eq!(get("bAt"), Value::Int(0), "B (500ms) deve rodar PRIMEIRO");
    assert_eq!(
        get("aAt"),
        Value::Int(1),
        "A (1000ms) deve rodar em seguida"
    );
    assert_eq!(get("ord"), Value::Int(2));
}

/// issue #50: postDelayed(r, Long.MAX_VALUE) não estoura o clock — a mensagem
/// nunca vence (clamp) em vez de panic (debug) ou vencer imediatamente (wrap).
#[test]
fn post_delayed_i64_max_does_not_wrap() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);

    let tick_cls = b.class("LTMax;", "Ljava/lang/Object;");
    b.static_field(tick_cls, "t", "I", SVal::Int(0));
    let _ft = b.field_idx("LTMax;", "I", "t");
    let f_t = b.find_field("LTMax;", "I", "t") as u16;
    let p_void = b.proto_idx("V", vec![]);
    b.direct(
        tick_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, refs.obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        tick_cls,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x60, 0, f_t);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_t));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let main_cls = b.class("LMainTMax;", "Landroid/app/Activity;");
    b.type_idx("LTMax;");
    let t_tick = b.type_idx_cached("LTMax;");
    let m_tick_init = b.method_idx("LTMax;", p_void, "<init>");
    b.direct(
        main_cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, b.type_idx_cached("Landroid/os/Handler;"));
            u.extend(op35c(0x70, 1, refs.handler_init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tick));
            u.extend(op35c(0x70, 1, m_tick_init, [1, 0, 0, 0, 0]));
            u.extend(op51l(0x18, 2, i64::MAX)); // const-wide v2, Long.MAX_VALUE
            u.extend(op35c(0x6E, 4, refs.post_delayed, [0, 1, 2, 3, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.launch_app("LMainTMax;", "com.exemplo.tmax", Vec::new())
        .expect("launch");
    let t0 = e
        .statics
        .get(&("LTMax;".to_string(), "t".to_string()))
        .cloned();
    assert_eq!(t0, Some(Value::Int(0)));
    let ran = e.advance_clock(10_000).expect("clock não pode panicar");
    assert_eq!(ran, 0, "delay=i64::MAX nunca vence (clamp)");
    let t1 = e
        .statics
        .get(&("LTMax;".to_string(), "t".to_string()))
        .cloned();
    assert_eq!(t1, Some(Value::Int(0)), "run não pode ter disparado");
}

// ── issue #60: divisor horizontal NÃO conta filhos GONE ────────────────────

/// LinearLayout horizontal [GONE, Button]: o Button ocupa a largura INTEIRA
/// (720). Antes: o divisor `w / children.len()` contava o GONE e o visível
/// recebia metade (360) com slot vazio.
#[test]
fn linear_horizontal_gone_child_does_not_consume_slot() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);
    b.type_idx("Landroid/widget/LinearLayout;");
    b.type_idx("Landroid/widget/TextView;");
    b.type_idx("Landroid/widget/Button;");
    let cls = b.class("LMain;", "Landroid/app/Activity;");
    let t_ll = b.type_idx_cached("Landroid/widget/LinearLayout;");
    let t_tv = b.type_idx_cached("Landroid/widget/TextView;");
    let t_btn = b.type_idx_cached("Landroid/widget/Button;");
    let s_x = b.intern("x") as u16;
    let s_ok = b.intern("ok") as u16;

    b.direct(
        cls,
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
    // onCreate: ll HORIZONTAL; tv1 → GONE antes do addView; btn visível
    b.direct(
        cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, t_ll); // v0 = ll
            u.extend(op35c(0x70, 1, refs.ll_init, [0, 0, 0, 0, 0]));
            u.extend(op11n(0x12, 1, 0)); // v1 = 0 (HORIZONTAL)
            u.extend(op35c(0x6E, 2, refs.set_orientation, [0, 1, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tv)); // v1 = tv1
            u.extend(op35c(0x70, 1, refs.tv_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_x)); // v2 = "x"
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op11n(0x12, 2, 2)); // v2 = 2 (GONE)
            u.extend(op35c(0x6E, 2, refs.set_visibility, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0])); // addView(tv1 GONE)
            u.extend(op21c(0x22, 1, t_btn)); // v1 = btn
            u.extend(op35c(0x70, 1, refs.btn_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_ok)); // v2 = "ok"
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0])); // addView(btn)
            u.extend(op35c(0x6E, 2, refs.set_content, [4, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.launch_app("LMain;", "com.exemplo.gone", Vec::new())
        .expect("launch");
    let dump = e.dump_ui();
    // Button único visível: largura inteira, posição 0 (antes: [0,0 360x48])
    assert!(
        dump.contains("Button @id/0 [0,0 720x48]"),
        "issue #60: Button ocupa 720 com GONE no slot: {dump}"
    );
    assert!(dump.contains("GONE"), "tv1 segue GONE no dump: {dump}");
    assert!(!dump.contains("360x48"), "nenhum slot pela metade: {dump}");
}

/// [A, GONE, B] horizontais: A e B dividem igualmente (360 cada) — o GONE
/// não entra no divisor nem deixa buraco.
#[test]
fn linear_horizontal_two_visible_split_with_gone_middle() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let refs = platform_method_refs(&mut b);
    b.type_idx("Landroid/widget/LinearLayout;");
    b.type_idx("Landroid/widget/TextView;");
    let cls = b.class("LMain;", "Landroid/app/Activity;");
    let t_ll = b.type_idx_cached("Landroid/widget/LinearLayout;");
    let t_tv = b.type_idx_cached("Landroid/widget/TextView;");
    let s_a = b.intern("a") as u16;
    let s_m = b.intern("m") as u16;
    let s_b = b.intern("b") as u16;

    b.direct(
        cls,
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
    b.direct(
        cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, t_ll); // v0 = ll
            u.extend(op35c(0x70, 1, refs.ll_init, [0, 0, 0, 0, 0]));
            u.extend(op11n(0x12, 1, 0)); // horizontal
            u.extend(op35c(0x6E, 2, refs.set_orientation, [0, 1, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tv)); // v1 = tvA
            u.extend(op35c(0x70, 1, refs.tv_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_a));
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tv)); // v1 = tvM
            u.extend(op35c(0x70, 1, refs.tv_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_m));
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op11n(0x12, 2, 2)); // GONE
            u.extend(op35c(0x6E, 2, refs.set_visibility, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0]));
            u.extend(op21c(0x22, 1, t_tv)); // v1 = tvB
            u.extend(op35c(0x70, 1, refs.tv_init, [1, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 2, s_b));
            u.extend(op35c(0x6E, 2, refs.set_text, [1, 2, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.add_view, [0, 1, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, refs.set_content, [4, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.launch_app("LMain;", "com.exemplo.gone2", Vec::new())
        .expect("launch");
    let dump = e.dump_ui();
    assert!(
        dump.contains("[0,0 360x48]"),
        "A com metade da largura: {dump}"
    );
    assert!(
        dump.contains("[360,0 360x48]"),
        "B colado em A (sem buraco do GONE): {dump}"
    );
}

// ═════════════════ issue #82: launch de classe ausente do DEX ═════════════════

/// `launch_app` de uma classe que não está no DEX precisa FALHAR (no Android,
/// `am start` de classe inexistente lança ActivityNotFoundException). Antes a
/// VM alocava a instância vazia e devolvia sucesso — `rd mcp launch` relatava
/// `launched: true` para um estado que nunca existiu (falso sucesso).
#[test]
fn launch_app_rejects_activity_missing_from_dex() {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    b.class("LMain;", "Landroid/app/Activity;");
    let mut e = engine_of(&b);

    let err = e
        .launch_app("LNope;", "com.exemplo.trivial", Vec::new())
        .expect_err("classe ausente do DEX não pode lançar");
    let rd = match err {
        rd_vm::err::VmExit::Error(rd) => rd,
        other => panic!("esperava erro estruturado da VM, veio {other:?}"),
    };
    assert_eq!(rd.code, "MISSING_ENTRY", "{rd:?}");
    assert!(rd.cause.contains("LNope;"), "cause cita a classe: {rd:?}");
    assert!(
        rd.suggestion.is_some(),
        "erro de launch traz suggestion: {rd:?}"
    );

    // a classe que EXISTE no DEX continua lançando (sem regressão)
    e.launch_app("LMain;", "com.exemplo.trivial", Vec::new())
        .expect("LMain; está no DEX");
}
