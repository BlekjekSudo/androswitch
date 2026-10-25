//! T9 — semântica AOSP do `View.onTouchEvent` default para views clicáveis
//! sem override DEX, e value classes `Xfermode` (PorterDuffXfermode /
//! BlendModeXfermode).
//!
//! Reprodução do gap Primftpd: sem o default AOSP, um irmão full-screen
//! (ViewPager atrás do TabLayout) rouba o DOWN/UP e o `performClick` da aba
//! nunca roda — a cadeia real `TabView.performClick` (Material, DEX) →
//! `Tab.select` → `ViewPager.setCurrentItem` exige que o tap seja consumido
//! na própria aba (AOSP: view clicável consome DOWN e faz performClick no UP
//! sobre um press ativo).
#[macro_use]
mod common;
use common::*;
use rd_vm::framework::HostObj;
use rd_vm::{Engine, ObjRef, Value};

const ACC_CONSTRUCTOR: u32 = 0x1_0000;

/// MotionEvent.obtain(downTime, eventTime, action, x, y, metaState) — a
/// rota de produção (estático de plataforma), clock virtual em 0.
fn event_of(e: &mut Engine, action: i32, x: f32, y: f32) -> ObjRef {
    let ev = e
        .invoke_static(
            "Landroid/view/MotionEvent;",
            "obtain",
            "(JJIFFI)Landroid/view/MotionEvent;",
            &[
                Value::Int(0),
                Value::Int(0),
                Value::Int(action),
                Value::Float(x),
                Value::Float(y),
                Value::Int(0),
            ],
        )
        .expect("MotionEvent.obtain");
    match ev {
        Value::Obj(r) => r,
        other => panic!("obtain: {other:?}"),
    }
}

/// View host com estado anexado (padrão inherited_dispatch).
fn view_of(e: &mut Engine, cls: &str) -> ObjRef {
    let v = e.heap.alloc_instance(cls.into(), vec![]).unwrap();
    e.attach_host_state(cls, v).unwrap();
    v
}

fn is_selected(e: &Engine, v: ObjRef) -> bool {
    matches!(e.fw.get(v), Some(HostObj::View { selected: true, .. }))
}

/// dispatchTouchEvent(action, x, y) — rota ViewRootImpl (arm host
/// ViewGroup/View → host_dispatch_touch → default View.onTouchEvent).
fn dispatch(e: &mut Engine, v: ObjRef, action: i32, x: f32, y: f32) -> bool {
    let ev = event_of(e, action, x, y);
    let r = e
        .invoke_instance(
            v,
            "dispatchTouchEvent",
            "(Landroid/view/MotionEvent;)Z",
            &[Value::Obj(ev)],
        )
        .expect("dispatchTouchEvent");
    r == Value::Int(1)
}

/// LTabLike; extends Landroid/view/View; com override DEX de `performClick`
/// que incrementa `LFlags;.clicks` e marca `selected` (o mesmo efeito do
/// `tab.select()` do Material — TabView.setSelected) e retorna true.
fn tab_like_builder() -> (DexBuilder, u16) {
    let mut b = DexBuilder::new();
    let cls = b.class("LTabLike;", "Landroid/view/View;");
    let flags = b.class("LFlags;", "Ljava/lang/Object;");
    b.static_field(flags, "clicks", "I", SVal::Int(0));
    let _freg = b.field_idx("LFlags;", "I", "clicks");
    let f_clicks = b.find_field("LFlags;", "I", "clicks") as u16;
    let p_z_void = b.proto_idx("V", vec!["Z".to_string()]);
    let set_selected = b.method_idx("Landroid/view/View;", p_z_void, "setSelected");
    // performClick()Z: regs=3, ins=1 → this=v2; clicks++; setSelected(true); true
    b.r#virtual(
        cls,
        "performClick",
        "Z",
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op21c(0x60, 0, f_clicks); // sget v0, LFlags;->clicks:I
            u.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
            u.extend(op21c(0x67, 0, f_clicks)); // sput v0, LFlags;->clicks:I
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            u.extend(op35c(0x6E, 2, set_selected, [2, 1, 0, 0, 0]));
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    (b, f_clicks)
}

fn clicks_of(e: &Engine) -> i32 {
    match e
        .statics
        .get(&("LFlags;".to_string(), "clicks".to_string()))
        .cloned()
    {
        Some(Value::Int(n)) => n,
        // None = sput ainda não rodou (nenhum performClick executou)
        None => 0,
        other => panic!("clicks: {other:?}"),
    }
}

#[test]
fn clickable_view_consumes_tap_and_performs_click_on_up() {
    let (b, _f) = tab_like_builder();
    let mut e = engine_of(&b);
    let v = view_of(&mut e, "LTabLike;");
    e.invoke_instance(v, "setClickable", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(clicks_of(&e), 0);
    // DOWN: view clicável consome (AOSP PFLAG_PRESSED)…
    assert!(dispatch(&mut e, v, 0, 10.0, 10.0), "DOWN clicável consome");
    assert_eq!(clicks_of(&e), 0, "click só no UP");
    // …e o UP sobre o press roda o override DEX de performClick.
    assert!(dispatch(&mut e, v, 1, 10.0, 10.0), "UP clicável consome");
    assert_eq!(clicks_of(&e), 1, "performClick DEX rodou no UP");
    assert!(is_selected(&e, v));
    // UP extra sem press: consumido, sem novo clique.
    assert!(dispatch(&mut e, v, 1, 10.0, 10.0));
    assert_eq!(clicks_of(&e), 1, "UP sem press não repete o clique");
}

#[test]
fn unclickable_view_returns_false_and_never_performs_click() {
    let (b, _f) = tab_like_builder();
    let mut e = engine_of(&b);
    let v = view_of(&mut e, "LTabLike;"); // sem setClickable
    assert!(
        !dispatch(&mut e, v, 0, 10.0, 10.0),
        "DOWN não-clicável passa"
    );
    assert!(!dispatch(&mut e, v, 1, 10.0, 10.0), "UP não-clicável passa");
    assert_eq!(clicks_of(&e), 0, "performClick não roda sem press ativo");
}

#[test]
fn cancel_releases_press_so_up_does_not_perform_click() {
    let (b, _f) = tab_like_builder();
    let mut e = engine_of(&b);
    let v = view_of(&mut e, "LTabLike;");
    e.invoke_instance(v, "setClickable", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert!(dispatch(&mut e, v, 0, 10.0, 10.0), "DOWN consome");
    // ACTION_CANCEL (3): gesto interrompido libera o press.
    assert!(dispatch(&mut e, v, 3, 10.0, 10.0), "CANCEL consumido");
    assert!(dispatch(&mut e, v, 1, 10.0, 10.0), "UP consumido");
    assert_eq!(clicks_of(&e), 0, "UP sem press ativo não roda performClick");
}

#[test]
fn up_without_down_does_not_perform_click() {
    let (b, _f) = tab_like_builder();
    let mut e = engine_of(&b);
    let v = view_of(&mut e, "LTabLike;");
    e.invoke_instance(v, "setClickable", "(Z)V", &[Value::Int(1)])
        .unwrap();
    // UP isolado (sem DOWN): consumido, mas sem clique (sem press).
    assert!(dispatch(&mut e, v, 1, 10.0, 10.0));
    assert_eq!(clicks_of(&e), 0, "performClick exige press do DOWN");
}

#[test]
fn host_click_listener_fires_when_no_dex_perform_click_override() {
    let mut b = DexBuilder::new();
    let _view_cls = b.class("LTabLike2;", "Landroid/view/View;");
    let lis_cls = b.class("LClickListener;", "Ljava/lang/Object;");
    let p_z_void = b.proto_idx("V", vec!["Z".to_string()]);
    let set_selected = b.method_idx("Landroid/view/View;", p_z_void, "setSelected");
    // onClick(View)V: regs=3, ins=2 → this=v1, view=v2; v0=1; v2.setSelected(v0)
    b.direct(
        lis_cls,
        "onClick",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, {
            let mut u = op11n(0x12, 0, 1); // const/4 v0, 1
            u.extend(op35c(0x6E, 2, set_selected, [2, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = view_of(&mut e, "LTabLike2;");
    let lis = e
        .heap
        .alloc_instance("LClickListener;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        v,
        "setOnClickListener",
        "(Landroid/view/View$OnClickListener;)V",
        &[Value::Obj(lis)],
    )
    .unwrap();
    // DOWN + UP → performClick (sem override DEX) → super path → listener.
    assert!(dispatch(&mut e, v, 0, 10.0, 10.0));
    assert!(dispatch(&mut e, v, 1, 10.0, 10.0));
    assert!(is_selected(&e, v), "listener host rodou via performClick");
}

/// O tap E2E do agente (touch_app → hit-test → touch_sequence) troca o
/// estado da aba clicável: a rota real do gap Primftpd.
#[test]
fn touch_app_on_clickable_view_performs_click_end_to_end() {
    let mut b = DexBuilder::new();
    let cls = b.class("LTabMain;", "Landroid/app/Activity;");
    let tab_cls = b.class("LTabLike;", "Landroid/view/View;");
    let flags = b.class("LFlags;", "Ljava/lang/Object;");
    b.static_field(flags, "clicks", "I", SVal::Int(0));
    let _freg = b.field_idx("LFlags;", "I", "clicks");
    let f_clicks = b.find_field("LFlags;", "I", "clicks") as u16;
    let p_void = b.proto_idx("V", vec![]);
    let p_view_void = b.proto_idx("V", vec!["Landroid/view/View;".to_string()]);
    let p_z_void = b.proto_idx("V", vec!["Z".to_string()]);
    let tab_init = b.method_idx("LTabLike;", p_void, "<init>");
    let super_init = b.method_idx("Landroid/view/View;", p_void, "<init>");
    let set_clickable = b.method_idx("Landroid/view/View;", p_z_void, "setClickable");
    let set_selected = b.method_idx("Landroid/view/View;", p_z_void, "setSelected");
    let set_content = b.method_idx("Landroid/app/Activity;", p_view_void, "setContentView");
    let t_tab = b.type_idx_cached("LTabLike;");

    // LTabLike.<init>()V: super()
    b.direct(
        tab_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, super_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // LTabLike.performClick()Z: this=v2 (regs=3, ins=1); clicks++;
    // this.setSelected(true); return true
    b.r#virtual(
        tab_cls,
        "performClick",
        "Z",
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op21c(0x60, 0, f_clicks); // sget v0, LFlags;->clicks:I
            u.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
            u.extend(op21c(0x67, 0, f_clicks)); // sput v0, LFlags;->clicks:I
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            u.extend(op35c(0x6E, 2, set_selected, [2, 1, 0, 0, 0]));
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    // LTabMain.<init>()V: super()
    let act_init = b.method_idx("Landroid/app/Activity;", p_void, "<init>");
    b.direct(
        cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, act_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // onCreate(Bundle)V: regs=6, ins=2 → this=v4, bundle=v5 (ins no topo);
    // v0 = new LTabLike; v0.setClickable(true); this.setContentView(v0)
    b.direct(
        cls,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(6, 2, 2, {
            let mut u = op21c(0x22, 0, t_tab); // new-instance v0, LTabLike;
            u.extend(op35c(0x70, 1, tab_init, [0, 0, 0, 0, 0]));
            u.extend(op11n(0x12, 3, 1)); // v3 = 1
            u.extend(op35c(0x6E, 2, set_clickable, [0, 3, 0, 0, 0]));
            u.extend(op35c(0x70, 2, set_content, [4, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    e.launch_app("LTabMain;", "com.exemplo.tab", Vec::new())
        .expect("launch");
    assert_eq!(clicks_of(&e), 0);
    let hit = e.touch_app(10, 10).expect("touch");
    assert!(hit, "aba clicável consome o tap");
    assert_eq!(clicks_of(&e), 1, "tap E2E rodou performClick (tab.select)");
}

#[test]
fn porter_duff_xfermode_stores_the_mode_for_the_rasterizer() {
    let mut b = DexBuilder::new();
    b.class("Landroid/graphics/Xfermode;", "Ljava/lang/Object;");
    b.class(
        "Landroid/graphics/PorterDuffXfermode;",
        "Landroid/graphics/Xfermode;",
    );
    b.class(
        "Landroid/graphics/BlendModeXfermode;",
        "Landroid/graphics/Xfermode;",
    );
    let mut e = engine_of(&b);

    // PorterDuff.Mode.valueOf("DST_IN") → ordinal 6 no catálogo do host
    let name = Value::Obj(e.heap.alloc_string("DST_IN".into()).unwrap());
    let mode = e
        .invoke_static(
            "Landroid/graphics/PorterDuff$Mode;",
            "valueOf",
            "(Ljava/lang/String;)Landroid/graphics/PorterDuff$Mode;",
            &[name],
        )
        .unwrap();
    let xf = e
        .heap
        .alloc_instance("Landroid/graphics/PorterDuffXfermode;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        xf,
        "<init>",
        "(Landroid/graphics/PorterDuff$Mode;)V",
        std::slice::from_ref(&mode),
    )
    .unwrap();
    let Value::Obj(stored) = e.heap.get_field(xf, "rd_mode").unwrap() else {
        panic!("rd_mode não é objeto");
    };
    assert_eq!(
        e.heap.get_field(stored, "rd_ord").unwrap(),
        Value::Int(6),
        "ordinal DST_IN legível pelo snap do rasterizador"
    );

    // BlendModeXfermode(BlendMode.CLEAR) → ordinal 0 (caminho CLEAR)
    let clear = Value::Obj(e.heap.alloc_string("CLEAR".into()).unwrap());
    let bmode = e
        .invoke_static(
            "Landroid/graphics/BlendMode;",
            "valueOf",
            "(Ljava/lang/String;)Landroid/graphics/BlendMode;",
            &[clear],
        )
        .unwrap();
    let xf2 = e
        .heap
        .alloc_instance("Landroid/graphics/BlendModeXfermode;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        xf2,
        "<init>",
        "(Landroid/graphics/BlendMode;)V",
        std::slice::from_ref(&bmode),
    )
    .unwrap();
    let Value::Obj(stored2) = e.heap.get_field(xf2, "rd_mode").unwrap() else {
        panic!("rd_mode não é objeto");
    };
    assert_eq!(
        e.heap.get_field(stored2, "rd_ord").unwrap(),
        Value::Int(0),
        "ordinal CLEAR legível pelo snap do rasterizador"
    );
}
