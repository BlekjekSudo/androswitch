//! T10 — view state (issue #228): `View.saveHierarchyState` /
//! `restoreHierarchyState` com a caminhada AOSP
//! (`dispatchSaveInstanceState`/`dispatchRestoreInstanceState`), flags
//! `saveEnabled`/`saveFromParentEnabled` e subclasses DEX de
//! `View$BaseSavedState` (`AbsSavedState.EMPTY_STATE`, `getSuperState`).
//!
//! Reprodução do gap Primftpd: `FragmentStateManager.saveViewState` cria um
//! `SparseArray<Parcelable>` e chama `mFragment.mView.saveHierarchyState(...)`
//! — sem o host o método não resolvia (NOT_FOUND) e a troca de abas quebrava.
#![allow(clippy::disallowed_methods)]
mod common;
use common::*;
use rd_vm::{Engine, ObjRef, Value};

const ACC_CONSTRUCTOR: u32 = 0x1_0000;

fn view_of(e: &mut Engine, cls: &str) -> ObjRef {
    let v = e.heap.alloc_instance(cls.into(), vec![]).unwrap();
    e.attach_host_state(cls, v).unwrap();
    v
}

fn container_of(e: &mut Engine) -> ObjRef {
    let sa = e
        .heap
        .alloc_instance("Landroid/util/SparseArray;".into(), vec![])
        .unwrap();
    e.invoke_instance(sa, "<init>", "()V", &[]).unwrap();
    sa
}

fn entries_of(e: &mut Engine, sa: ObjRef) -> Vec<(i32, Value)> {
    let n = match e.invoke_instance(sa, "size", "()I", &[]).unwrap() {
        Value::Int(n) => n,
        _ => 0,
    };
    (0..n)
        .map(|i| {
            let k = e
                .invoke_instance(sa, "keyAt", "(I)I", &[Value::Int(i)])
                .unwrap();
            let v = e
                .invoke_instance(sa, "valueAt", "(I)Ljava/lang/Object;", &[Value::Int(i)])
                .unwrap();
            (
                match k {
                    Value::Int(k) => k,
                    other => panic!("keyAt: {other:?}"),
                },
                v,
            )
        })
        .collect()
}

fn save(e: &mut Engine, v: ObjRef, sa: ObjRef) {
    e.invoke_instance(
        v,
        "saveHierarchyState",
        "(Landroid/util/SparseArray;)V",
        &[Value::Obj(sa)],
    )
    .unwrap();
}

fn restore(e: &mut Engine, v: ObjRef, sa: ObjRef) {
    e.invoke_instance(
        v,
        "restoreHierarchyState",
        "(Landroid/util/SparseArray;)V",
        &[Value::Obj(sa)],
    )
    .unwrap();
}

fn int_static(e: &Engine, cls: &str, name: &str) -> i32 {
    match e.statics.get(&(cls.to_string(), name.to_string())).cloned() {
        Some(Value::Int(n)) => n,
        None => 0,
        other => panic!("{cls}.{name}: {other:?}"),
    }
}

/// LRecView; extends Landroid/view/View; — onSaveInstanceState devolve um
/// LMyState (subclass DEX de BaseSavedState) construído com
/// super.onSaveInstanceState() (que DEVE ser o EMPTY_STATE não-nulo);
/// onRestoreInstanceState marca em qual ramo caiu (estado == EMPTY_STATE ou
/// não) e conta chamadas.
fn rec_view_builder() -> DexBuilder {
    let mut b = DexBuilder::new();
    let cls = b.class("LRecView;", "Landroid/view/View;");
    let state_cls = b.class("LMyState;", "Landroid/view/View$BaseSavedState;");
    let flags = b.class("LFlags;", "Ljava/lang/Object;");
    for name in ["saves", "restores", "rest_empty", "rest_other"] {
        b.static_field(flags, name, "I", SVal::Int(0));
        b.field_idx("LFlags;", "I", name);
    }
    let f = |n: &str| b.find_field("LFlags;", "I", n) as u16;
    let (f_saves, f_restores, f_rest_empty, f_rest_other) =
        (f("saves"), f("restores"), f("rest_empty"), f("rest_other"));
    let p_void = b.proto_idx("V", vec![]);
    let p_state_void = b.proto_idx("V", vec!["Landroid/os/Parcelable;".to_string()]);
    let p_parcelable_ret = b.proto_idx("Landroid/os/Parcelable;", vec![]);
    let super_init = b.method_idx("Landroid/view/View;", p_void, "<init>");
    let super_onsave = b.method_idx(
        "Landroid/view/View;",
        p_parcelable_ret,
        "onSaveInstanceState",
    );
    let state_init = b.method_idx("LMyState;", p_state_void, "<init>");
    let base_state_init =
        b.method_idx("Landroid/view/View$BaseSavedState;", p_state_void, "<init>");
    let f_empty = b.field_idx(
        "Landroid/view/AbsSavedState;",
        "Landroid/os/Parcelable;",
        "EMPTY_STATE",
    );
    let t_state = b.type_idx_cached("LMyState;");

    // LMyState.<init>(Parcelable)V: super(superState)
    b.direct(
        state_cls,
        "<init>",
        "V",
        vec!["Landroid/os/Parcelable;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 2, {
            let mut u = op35c(0x70, 2, base_state_init, [0, 1, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );

    // LRecView.<init>()V: super()
    b.direct(
        cls,
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

    // LRecView.onSaveInstanceState()Landroid/os/Parcelable; (regs=3, ins=1,
    // this=v2): saves++; new LMyState(super.onSaveInstanceState()).
    b.r#virtual(
        cls,
        "onSaveInstanceState",
        "Landroid/os/Parcelable;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op21c(0x60, 0, f_saves);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_saves));
            u.extend(op21c(0x22, 0, t_state)); // new-instance v0, LMyState;
            u.extend(op35c(0x6F, 1, super_onsave, [2, 0, 0, 0, 0]));
            u.extend(op11x(0x0C, 1)); // move-result-object v1
            u.extend(op35c(0x70, 2, state_init, [0, 1, 0, 0, 0]));
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );

    // LRecView.onRestoreInstanceState(Parcelable)V (regs=3, ins=2, this=v2,
    // state=v1): restores++; if state == EMPTY_STATE → rest_empty++ senão
    // rest_other++.
    b.r#virtual(
        cls,
        "onRestoreInstanceState",
        "V",
        vec!["Landroid/os/Parcelable;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, {
            let mut u = op21c(0x60, 0, f_restores);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_restores));
            u.extend(op21c(0x62, 0, f_empty)); // sget-object v0, EMPTY_STATE
            u.extend(op22t(0x32, 0, 1, 5)); // if-eq v0, v1 → pc9
            u.extend(op21c(0x60, 0, f_rest_other));
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_rest_other));
            u.extend(op10x(0x0E));
            u.extend(op21c(0x60, 0, f_rest_empty));
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_rest_empty));
            u.extend(op10x(0x0E));
            u
        })),
    );
    b
}

/// Árvore hospedada: pai com dois filhos DEX (LRecView) e um neto host —
/// todos com ids.
fn tree(e: &mut Engine) -> (ObjRef, ObjRef, ObjRef, ObjRef) {
    let parent = view_of(e, "Landroid/widget/FrameLayout;");
    let a = view_of(e, "LRecView;");
    let b = view_of(e, "LRecView;");
    let leaf = view_of(e, "Landroid/widget/TextView;");
    for (v, id) in [(parent, 11), (a, 12), (b, 13), (leaf, 14)] {
        e.invoke_instance(v, "setId", "(I)V", &[Value::Int(id)])
            .unwrap();
    }
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(a)],
    )
    .unwrap();
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(b)],
    )
    .unwrap();
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(leaf)],
    )
    .unwrap();
    (parent, a, b, leaf)
}

#[test]
fn hierarchy_save_fills_sparse_array_and_restore_round_trips() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    let (parent, _a, _b2, _leaf) = tree(&mut e);
    let sa = container_of(&mut e);
    save(&mut e, parent, sa);

    // 4 entradas (pai + 2 filhos + neto), ids exatos, todos Parcelable.
    let mut entries = entries_of(&mut e, sa);
    entries.sort_by_key(|(id, _)| *id);
    assert_eq!(
        entries.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![11, 12, 13, 14],
        "SparseArray guarda um estado por id"
    );

    // O estado do LRecView é uma subclasse DEX de BaseSavedState.
    for (_, v) in &entries {
        let Value::Obj(r) = v else {
            panic!("estado não-objeto: {v:?}")
        };
        let cls = e.heap.class_of(*r).unwrap();
        assert!(
            cls == "LMyState;" || cls == "Landroid/view/AbsSavedState;",
            "classe de estado inesperada: {cls}"
        );
    }

    // Restore: cada LRecView recebe o MESMO objeto que produziu (não é o
    // EMPTY_STATE → ramo rest_other) e o pai/no-defaults não quebram.
    restore(&mut e, parent, sa);
    assert_eq!(int_static(&e, "LFlags;", "restores"), 2);
    assert_eq!(
        int_static(&e, "LFlags;", "rest_other"),
        2,
        "estado real ≠ EMPTY_STATE"
    );
    assert_eq!(int_static(&e, "LFlags;", "rest_empty"), 0);
    assert_eq!(int_static(&e, "LFlags;", "saves"), 2);

    // Segundo ciclo: novo container, ids estáveis.
    let sa2 = container_of(&mut e);
    save(&mut e, parent, sa2);
    assert_eq!(entries_of(&mut e, sa2).len(), 4);
    assert_eq!(int_static(&e, "LFlags;", "saves"), 4);
    assert_ne!(sa, sa2);
    let _ = _b2;
}

#[test]
fn default_view_stores_abs_saved_state_empty_state_singleton() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    let plain = view_of(&mut e, "Landroid/widget/TextView;");
    e.invoke_instance(plain, "setId", "(I)V", &[Value::Int(7)])
        .unwrap();
    let sa = container_of(&mut e);
    save(&mut e, plain, sa);

    let entries = entries_of(&mut e, sa);
    assert_eq!(entries.len(), 1, "só a view com id grava estado");
    let (id, v) = &entries[0];
    assert_eq!(*id, 7);
    let Value::Obj(r) = v else {
        panic!("estado não-objeto")
    };
    assert_eq!(e.heap.class_of(*r).unwrap(), "Landroid/view/AbsSavedState;");

    // O singleton é o EMPTY_STATE canônico: getSuperState() == null.
    let super_state = e
        .invoke_instance(*r, "getSuperState", "()Landroid/os/Parcelable;", &[])
        .unwrap();
    assert_eq!(super_state, Value::Null, "EMPTY_STATE não tem superState");
}

#[test]
fn save_enabled_false_skips_state_but_children_still_save() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    let parent = view_of(&mut e, "Landroid/widget/FrameLayout;");
    let a = view_of(&mut e, "LRecView;");
    for (v, id) in [(parent, 21), (a, 22)] {
        e.invoke_instance(v, "setId", "(I)V", &[Value::Int(id)])
            .unwrap();
    }
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(a)],
    )
    .unwrap();
    // AOSP: setSaveEnabled(false) desliga o estado PRÓPRIO da view; os filhos
    // continuam sendo caminhados.
    e.invoke_instance(parent, "setSaveEnabled", "(Z)V", &[Value::Int(0)])
        .unwrap();
    let sa = container_of(&mut e);
    save(&mut e, parent, sa);
    let ids = entries_of(&mut e, sa)
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![22],
        "pai com saveEnabled=false não grava; filho sim"
    );

    // isSaveEnabled reflete a flag; o default é true.
    let enabled = e
        .invoke_instance(parent, "isSaveEnabled", "()Z", &[])
        .unwrap();
    assert_eq!(enabled, Value::Int(0));
    let default_on = e.invoke_instance(a, "isSaveEnabled", "()Z", &[]).unwrap();
    assert_eq!(default_on, Value::Int(1));
}

#[test]
fn parent_save_disabled_skips_subtree() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    let parent = view_of(&mut e, "Landroid/widget/FrameLayout;");
    let child = view_of(&mut e, "Landroid/widget/LinearLayout;");
    let grand = view_of(&mut e, "LRecView;");
    for (v, id) in [(parent, 31), (child, 32), (grand, 33)] {
        e.invoke_instance(v, "setId", "(I)V", &[Value::Int(id)])
            .unwrap();
    }
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    )
    .unwrap();
    e.invoke_instance(
        child,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(grand)],
    )
    .unwrap();
    // AOSP: setSaveFromParentEnabled(false) no FILHO corta a subárvore dele
    // (PARENT_SAVE_DISABLED) — o pai ainda grava o próprio estado.
    e.invoke_instance(child, "setSaveFromParentEnabled", "(Z)V", &[Value::Int(0)])
        .unwrap();
    let sa = container_of(&mut e);
    save(&mut e, parent, sa);
    let ids = entries_of(&mut e, sa)
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![31], "subárvore do filho cortada; pai intacto");
}

#[test]
fn base_saved_state_stores_and_reads_super_state() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    // new LMyState(EMPTY_STATE): superState normaliza para null (AOSP).
    let st_empty = e.heap.alloc_instance("LMyState;".into(), vec![]).unwrap();
    let empty = crate_empty_state(&mut e);
    e.invoke_instance(
        st_empty,
        "<init>",
        "(Landroid/os/Parcelable;)V",
        &[Value::Obj(empty)],
    )
    .unwrap();
    let sup = e
        .invoke_instance(st_empty, "getSuperState", "()Landroid/os/Parcelable;", &[])
        .unwrap();
    assert_eq!(
        sup,
        Value::Null,
        "EMPTY_STATE normaliza para superState null"
    );

    // new LMyState(outro): getSuperState() devolve o próprio objeto.
    let other = e
        .heap
        .alloc_instance("Landroid/view/AbsSavedState;".into(), vec![])
        .unwrap();
    let st = e.heap.alloc_instance("LMyState;".into(), vec![]).unwrap();
    e.invoke_instance(
        st,
        "<init>",
        "(Landroid/os/Parcelable;)V",
        &[Value::Obj(other)],
    )
    .unwrap();
    let sup = e
        .invoke_instance(st, "getSuperState", "()Landroid/os/Parcelable;", &[])
        .unwrap();
    assert_eq!(sup, Value::Obj(other), "getSuperState devolve o superState");

    // super(null): AOSP AbsSavedState lança IllegalArgumentException.
    let bad = e.heap.alloc_instance("LMyState;".into(), vec![]).unwrap();
    let err = e
        .invoke_instance(bad, "<init>", "(Landroid/os/Parcelable;)V", &[Value::Null])
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("IllegalArgumentException") || msg.contains("superState"),
        "esperava IllegalArgumentException, veio: {msg}"
    );

    // describeContents() == 0 (AbsSavedState).
    let d = e
        .invoke_instance(st, "describeContents", "()I", &[])
        .unwrap();
    assert_eq!(d, Value::Int(0));
}

/// AbsSavedState.EMPTY_STATE via engine (mesma rota do empty_state host).
fn crate_empty_state(e: &mut Engine) -> ObjRef {
    // Invocado por reflexão da própria suíte: a identidade canônica vive nos
    // statics da classe.
    match e
        .statics
        .get(&(
            "Landroid/view/AbsSavedState;".to_string(),
            "EMPTY_STATE".to_string(),
        ))
        .cloned()
    {
        Some(Value::Obj(r)) => r,
        _ => {
            let r = e
                .heap
                .alloc_instance("Landroid/view/AbsSavedState;".into(), vec![])
                .unwrap();
            e.statics.insert(
                (
                    "Landroid/view/AbsSavedState;".to_string(),
                    "EMPTY_STATE".to_string(),
                ),
                Value::Obj(r),
            );
            r
        }
    }
}

#[test]
fn restore_is_noop_without_entries_and_for_missing_ids() {
    let b = rec_view_builder();
    let mut e = engine_of(&b);
    let (parent, _a, _b2, _leaf) = tree(&mut e);
    // Restore com container vazio: nenhuma view recebe estado (AOSP: get
    // devolve null → sem callback).
    let sa = container_of(&mut e);
    restore(&mut e, parent, sa);
    assert_eq!(int_static(&e, "LFlags;", "restores"), 0);
    assert_eq!(int_static(&e, "LFlags;", "rest_empty"), 0);
    assert_eq!(int_static(&e, "LFlags;", "rest_other"), 0);

    // Restore com container que não tem o id desta view (estado de outra
    // hierarquia): também sem callback.
    let lone = view_of(&mut e, "LRecView;");
    e.invoke_instance(lone, "setId", "(I)V", &[Value::Int(99)])
        .unwrap();
    restore(&mut e, lone, sa);
    assert_eq!(int_static(&e, "LFlags;", "restores"), 0);
}

#[test]
fn dex_dispatch_override_wins_over_host_default() {
    // Um ViewGroup DEX com override de dispatchSaveInstanceState intercepta a
    // caminhada inteira (AOSP: virtual) — e o super landing segue o host.
    let mut b = rec_view_builder();
    let cls = b.class("LDispatchGuard;", "Landroid/view/ViewGroup;");
    let flags = b.class("LFlags2;", "Ljava/lang/Object;");
    b.static_field(flags, "guard", "I", SVal::Int(0));
    b.field_idx("LFlags2;", "I", "guard");
    let f_guard = b.find_field("LFlags2;", "I", "guard") as u16;
    let p_container_void = b.proto_idx("V", vec!["Landroid/util/SparseArray;".to_string()]);
    let super_dispatch = b.method_idx(
        "Landroid/view/ViewGroup;",
        p_container_void,
        "dispatchSaveInstanceState",
    );
    // dispatchSaveInstanceState(SparseArray)V: regs=3, ins=2 → this=v1,
    // container=v2; guard++; super.dispatchSaveInstanceState(container).
    b.r#virtual(
        cls,
        "dispatchSaveInstanceState",
        "V",
        vec!["Landroid/util/SparseArray;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 1, {
            let mut u = op21c(0x60, 0, f_guard);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_guard));
            u.extend(op35c(0x6F, 2, super_dispatch, [1, 2, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    let guard = view_of(&mut e, "LDispatchGuard;");
    let inner = view_of(&mut e, "LRecView;");
    e.invoke_instance(guard, "setId", "(I)V", &[Value::Int(41)])
        .unwrap();
    e.invoke_instance(inner, "setId", "(I)V", &[Value::Int(42)])
        .unwrap();
    e.invoke_instance(
        guard,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(inner)],
    )
    .unwrap();
    let sa = container_of(&mut e);
    save(&mut e, guard, sa);
    // O override DEX rodou e o super (host) gravou os dois ids.
    assert_eq!(int_static(&e, "LFlags2;", "guard"), 1);
    let mut ids = entries_of(&mut e, sa)
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec![41, 42], "super.dispatch gravou pai e filho");
}
