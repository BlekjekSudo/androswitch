//! Apêndice D — testes do runtime AndroidX (`androidx.rs`): lifecycle,
//! livedata (+ callbacks DEX reais), viewmodel, savedstate, collection,
//! datastore (valores), work Data, navigation, startup, core
//! (FileProvider/permissões/cancellation), biometria programável,
//! custom tabs, futures — e a invariante DEX-first (corpo DEX presente
//! ⇒ o host devolve `None` e a biblioteca real executa).

#[macro_use]
mod common;

use common::*;
use rd_vm::androidx::*;
use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}
fn s(e: &mut Engine, txt: &str) -> Value {
    Value::Obj(e.heap.alloc_string(txt.to_string()).unwrap())
}
fn obj(e: &mut Engine, class: &str) -> u32 {
    e.heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}
fn iv(v: Option<Value>) -> i32 {
    match v {
        Some(Value::Int(n)) => n,
        other => panic!("esperava Int, veio {other:?}"),
    }
}
fn ov(v: Option<Value>) -> u32 {
    match v {
        Some(Value::Obj(r)) => r,
        other => panic!("esperava Obj, veio {other:?}"),
    }
}
fn class_obj(e: &mut Engine, desc: &str) -> u32 {
    let c = obj(e, "Ljava/lang/Class;");
    let name = s(e, desc);
    e.heap.put_field(c, "class_name", name).unwrap();
    c
}
fn exception_class(err: &VmExit) -> String {
    match err {
        VmExit::Exception(t) => t.class.clone(),
        other => panic!("esperava Exception, veio {other:?}"),
    }
}

/// ci!(e, CLASS, "name", "sig", recv, [args]) — avalia os args ANTES do
/// borrow mutável da engine, como os fixtures do m8 fazem com locais.
macro_rules! ci {
    ($e:expr, $cls:expr, $name:expr, $sig:expr, $recv:expr, [$($a:expr),* $(,)?]) => {{
        let __args: Vec<Value> = vec![$($a),*];
        call_instance(&mut $e, $cls, $name, $sig, $recv, &__args)
    }};
}
macro_rules! cs {
    ($e:expr, $cls:expr, $name:expr, $sig:expr, [$($a:expr),* $(,)?]) => {{
        let __args: Vec<Value> = vec![$($a),*];
        call_static(&mut $e, $cls, $name, $sig, &__args)
    }};
}
fn state_obj_of(e: &mut Engine, name: &str) -> u32 {
    let n = s(e, name);
    ov(call_static(
        e,
        LIFECYCLE_STATE,
        "valueOf",
        "(Ljava/lang/String;)Landroidx/lifecycle/Lifecycle$State;",
        &[n],
    )
    .unwrap())
}
fn event_obj_of(e: &mut Engine, name: &str) -> u32 {
    let n = s(e, name);
    ov(call_static(
        e,
        LIFECYCLE_EVENT,
        "valueOf",
        "(Ljava/lang/String;)Landroidx/lifecycle/Lifecycle$Event;",
        &[n],
    )
    .unwrap())
}
fn make_registry(mut e: &mut Engine, owner: u32) -> u32 {
    let reg = obj(e, LIFECYCLE_REGISTRY);
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "<init>",
        "(Landroidx/lifecycle/LifecycleOwner;)V",
        reg,
        [Value::Obj(owner)]
    )
    .unwrap();
    reg
}
fn counter_observer_class(b: &mut DexBuilder, class: &str, method: &str, params: Vec<&str>) {
    let cls = b.class(class, "Ljava/lang/Object;");
    b.static_field(cls, "called", "I", common::SVal::Int(0));
    let f = b.field_idx(class, "I", "called");
    let ins = (params.len() + 1) as u16;
    b.r#virtual(
        cls,
        method,
        "V",
        params,
        ACC_PUBLIC,
        Some(b.code(ins + 2, ins, 0, {
            let mut u = op21c(0x60, 0, f); // sget v0, called
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            u.extend(op23x(0x90, 0, 0, 1)); // add-int v0, v0, v1
            u.extend(op21c(0x67, 0, f)); // sput v0, called
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
}
fn called(e: &Engine, class: &str) -> i32 {
    // Estático nunca escrito pelo DEX ainda não foi materializado = 0.
    match e.statics.get(&(class.to_string(), "called".to_string())) {
        Some(Value::Int(n)) => *n,
        None => 0,
        other => panic!("called de {class}: {other:?}"),
    }
}

// ── Lifecycle ───────────────────────────────────────────────────────────
#[test]
fn dex_lifecycle_state_machine() {
    let mut e = engine();
    let owner = obj(&mut e, "LFakeOwner;");
    let reg = make_registry(&mut e, owner);
    assert_eq!(
        e.androidx_lifecycle_state(reg).as_deref(),
        Some("INITIALIZED")
    );

    let resumed = state_obj_of(&mut e, "RESUMED");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "setCurrentState",
        "(Landroidx/lifecycle/Lifecycle$State;)V",
        reg,
        [Value::Obj(resumed)]
    )
    .unwrap();
    assert_eq!(e.androidx_lifecycle_state(reg).as_deref(), Some("RESUMED"));

    // getCurrentState devolve o MESMO objeto canônico do enum.
    let cur = ov(ci!(
        e,
        LIFECYCLE,
        "getCurrentState",
        "()Landroidx/lifecycle/Lifecycle$State;",
        reg,
        []
    )
    .unwrap());
    assert_eq!(cur, resumed);

    let started = state_obj_of(&mut e, "STARTED");
    assert_eq!(
        iv(ci!(
            e,
            LIFECYCLE_STATE,
            "isAtLeast",
            "(Landroidx/lifecycle/Lifecycle$State;)Z",
            cur,
            [Value::Obj(started)]
        )
        .unwrap()),
        1
    );
    let on_pause = event_obj_of(&mut e, "ON_PAUSE");
    assert_eq!(
        ov(ci!(
            e,
            LIFECYCLE_EVENT,
            "getTargetState",
            "()Landroidx/lifecycle/Lifecycle$State;",
            on_pause,
            []
        )
        .unwrap()),
        started
    );
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "handleLifecycleEvent",
        "(Landroidx/lifecycle/Lifecycle$Event;)V",
        reg,
        [Value::Obj(on_pause)]
    )
    .unwrap();
    assert_eq!(e.androidx_lifecycle_state(reg).as_deref(), Some("STARTED"));

    let obs = obj(&mut e, "LPlainObserver;");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "addObserver",
        "(Landroidx/lifecycle/LifecycleObserver;)V",
        reg,
        [Value::Obj(obs)]
    )
    .unwrap();
    assert_eq!(
        iv(ci!(e, LIFECYCLE_REGISTRY, "getObserverCount", "()I", reg, []).unwrap()),
        1
    );
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "removeObserver",
        "(Landroidx/lifecycle/LifecycleObserver;)V",
        reg,
        [Value::Obj(obs)]
    )
    .unwrap();
    assert_eq!(
        iv(ci!(e, LIFECYCLE_REGISTRY, "getObserverCount", "()I", reg, []).unwrap()),
        0
    );

    // valueOf inválido → IllegalArgumentException (não sucesso falso)
    let err = cs!(
        e,
        LIFECYCLE_STATE,
        "valueOf",
        "(Ljava/lang/String;)Landroidx/lifecycle/Lifecycle$State;",
        [s(&mut e, "VOANDO")]
    )
    .unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Ljava/lang/IllegalArgumentException;"
    );

    // DESTROYED é terminal.
    let on_destroy = event_obj_of(&mut e, "ON_DESTROY");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "handleLifecycleEvent",
        "(Landroidx/lifecycle/Lifecycle$Event;)V",
        reg,
        [Value::Obj(on_destroy)]
    )
    .unwrap();
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "setCurrentState",
        "(Landroidx/lifecycle/Lifecycle$State;)V",
        reg,
        [Value::Obj(resumed)]
    )
    .unwrap();
    assert_eq!(
        e.androidx_lifecycle_state(reg).as_deref(),
        Some("DESTROYED")
    );
}

#[test]
fn dex_lifecycle_observer_dex_callback_counts_events() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LLcObs;",
        "onStateChanged",
        vec![
            "Landroidx/lifecycle/LifecycleOwner;",
            "Landroidx/lifecycle/Lifecycle$Event;",
        ],
    );
    let mut e = engine_of(&b);
    let owner = obj(&mut e, "LFakeOwner;");
    let reg = make_registry(&mut e, owner);
    let obs = obj(&mut e, "LLcObs;");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "addObserver",
        "(Landroidx/lifecycle/LifecycleObserver;)V",
        reg,
        [Value::Obj(obs)]
    )
    .unwrap();
    let resumed = state_obj_of(&mut e, "RESUMED");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "setCurrentState",
        "(Landroidx/lifecycle/Lifecycle$State;)V",
        reg,
        [Value::Obj(resumed)]
    )
    .unwrap();
    // INITIALIZED → RESUMED dispara ON_CREATE, ON_START, ON_RESUME no DEX.
    assert_eq!(called(&e, "LLcObs;"), 3);
}

// ── LiveData ────────────────────────────────────────────────────────────
#[test]
fn dex_livedata_forever_and_inactive_observers() {
    let mut b = DexBuilder::new();
    counter_observer_class(&mut b, "LValObs;", "onChanged", vec!["Ljava/lang/Object;"]);
    let mut e = engine_of(&b);
    let ld = obj(&mut e, MUTABLE_LIVEDATA);
    ci!(e, MUTABLE_LIVEDATA, "<init>", "()V", ld, []).unwrap();
    assert_eq!(
        ci!(e, LIVEDATA, "getValue", "()Ljava/lang/Object;", ld, []).unwrap(),
        Some(Value::Null)
    );

    let obs = obj(&mut e, "LValObs;");
    ci!(
        e,
        LIVEDATA,
        "observeForever",
        "(Landroidx/lifecycle/Observer;)V",
        ld,
        [Value::Obj(obs)]
    )
    .unwrap();
    let v1 = s(&mut e, "um");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "setValue",
        "(Ljava/lang/Object;)V",
        ld,
        [v1.clone()]
    )
    .unwrap();
    assert_eq!(called(&e, "LValObs;"), 1);
    assert_eq!(e.androidx_livedata_value(ld), Some(v1));
    assert_eq!(
        iv(ci!(e, LIVEDATA, "hasActiveObservers", "()Z", ld, []).unwrap()),
        1
    );

    // Sticky: observer novo recebe o valor corrente no registro.
    let obs2 = obj(&mut e, "LValObs;");
    ci!(
        e,
        LIVEDATA,
        "observeForever",
        "(Landroidx/lifecycle/Observer;)V",
        ld,
        [Value::Obj(obs2)]
    )
    .unwrap();
    assert_eq!(called(&e, "LValObs;"), 2);

    // Owner sem lifecycle resolvível ⇒ observer inativo: não entrega.
    let ld2 = obj(&mut e, MUTABLE_LIVEDATA);
    ci!(e, MUTABLE_LIVEDATA, "<init>", "()V", ld2, []).unwrap();
    let owner = obj(&mut e, "LOwnerSemDex;");
    let obs3 = obj(&mut e, "LValObs;");
    ci!(
        e,
        LIVEDATA,
        "observe",
        "(Landroidx/lifecycle/LifecycleOwner;Landroidx/lifecycle/Observer;)V",
        ld2,
        [Value::Obj(owner), Value::Obj(obs3)]
    )
    .unwrap();
    let v2 = s(&mut e, "dois");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "setValue",
        "(Ljava/lang/Object;)V",
        ld2,
        [v2]
    )
    .unwrap();
    assert_eq!(called(&e, "LValObs;"), 2, "observer inativo não recebe");
    assert_eq!(
        iv(ci!(e, LIVEDATA, "hasObservers", "()Z", ld2, []).unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(e, LIVEDATA, "hasActiveObservers", "()Z", ld2, []).unwrap()),
        0
    );

    ci!(
        e,
        LIVEDATA,
        "removeObserver",
        "(Landroidx/lifecycle/Observer;)V",
        ld,
        [Value::Obj(obs)]
    )
    .unwrap();
    let v3 = s(&mut e, "tres");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "postValue",
        "(Ljava/lang/Object;)V",
        ld,
        [v3]
    )
    .unwrap();
    assert_eq!(
        called(&e, "LValObs;"),
        3,
        "só o obs2 (forever) recebe o postValue"
    );
}

#[test]
fn dex_livedata_owner_lifecycle_gates_delivery() {
    let mut b = DexBuilder::new();
    counter_observer_class(&mut b, "LValObs;", "onChanged", vec!["Ljava/lang/Object;"]);
    // Owner DEX: getLifecycle() devolve o campo `lifecycle`.
    let owner_cls = b.class("LDexOwner;", "Ljava/lang/Object;");
    b.instance_field(owner_cls, "lifecycle", "Landroidx/lifecycle/Lifecycle;");
    let f = b.field_idx("LDexOwner;", "Landroidx/lifecycle/Lifecycle;", "lifecycle");
    b.r#virtual(
        owner_cls,
        "getLifecycle",
        "Landroidx/lifecycle/Lifecycle;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op22c(0x54, 0, 1, f); // iget-object v0, v1(this), lifecycle
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );
    let mut e = engine_of(&b);
    let owner = obj(&mut e, "LDexOwner;");
    let reg = make_registry(&mut e, owner);
    e.heap
        .put_field(owner, "lifecycle", Value::Obj(reg))
        .unwrap();

    let ld = obj(&mut e, MUTABLE_LIVEDATA);
    ci!(e, MUTABLE_LIVEDATA, "<init>", "()V", ld, []).unwrap();
    let obs = obj(&mut e, "LValObs;");
    ci!(
        e,
        LIVEDATA,
        "observe",
        "(Landroidx/lifecycle/LifecycleOwner;Landroidx/lifecycle/Observer;)V",
        ld,
        [Value::Obj(owner), Value::Obj(obs)]
    )
    .unwrap();
    // INITIALIZED (< STARTED): não entrega.
    let v1 = s(&mut e, "cedo");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "setValue",
        "(Ljava/lang/Object;)V",
        ld,
        [v1]
    )
    .unwrap();
    assert_eq!(called(&e, "LValObs;"), 0);
    // Sobe para RESUMED; próximo setValue entrega.
    let resumed = state_obj_of(&mut e, "RESUMED");
    ci!(
        e,
        LIFECYCLE_REGISTRY,
        "setCurrentState",
        "(Landroidx/lifecycle/Lifecycle$State;)V",
        reg,
        [Value::Obj(resumed)]
    )
    .unwrap();
    let v2 = s(&mut e, "agora");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "setValue",
        "(Ljava/lang/Object;)V",
        ld,
        [v2]
    )
    .unwrap();
    assert_eq!(called(&e, "LValObs;"), 1);
}

// ── ViewModel / SavedStateHandle ────────────────────────────────────────
#[test]
fn dex_viewmodel_store_provider_and_saved_state() {
    let mut e = engine();
    let store = obj(&mut e, VIEW_MODEL_STORE);
    ci!(e, VIEW_MODEL_STORE, "<init>", "()V", store, []).unwrap();
    let provider = obj(&mut e, VIEW_MODEL_PROVIDER);
    ci!(
        e,
        VIEW_MODEL_PROVIDER,
        "<init>",
        "(Landroidx/lifecycle/ViewModelStore;Landroidx/lifecycle/ViewModelProvider$Factory;)V",
        provider,
        [Value::Obj(store), Value::Null]
    )
    .unwrap();
    let cls = class_obj(&mut e, "LFakeVm;");
    let a = ov(ci!(
        e,
        VIEW_MODEL_PROVIDER,
        "get",
        "(Ljava/lang/Class;)Landroidx/lifecycle/ViewModel;",
        provider,
        [Value::Obj(cls)]
    )
    .unwrap());
    let b2 = ov(ci!(
        e,
        VIEW_MODEL_PROVIDER,
        "get",
        "(Ljava/lang/Class;)Landroidx/lifecycle/ViewModel;",
        provider,
        [Value::Obj(cls)]
    )
    .unwrap());
    assert_eq!(a, b2, "mesma instância persistida na store");
    let keys = e.androidx_vm_store_keys(store);
    assert_eq!(keys.len(), 1);
    assert!(
        keys[0].contains("FakeVm"),
        "chave default canônica: {keys:?}"
    );

    let vm2 = obj(&mut e, "LOutroVm;");
    ci!(
        e,
        VIEW_MODEL_STORE,
        "put",
        "(Ljava/lang/String;Landroidx/lifecycle/ViewModel;)V",
        store,
        [s(&mut e, "k2"), Value::Obj(vm2)]
    )
    .unwrap();
    assert_eq!(
        ov(ci!(
            e,
            VIEW_MODEL_STORE,
            "get",
            "(Ljava/lang/String;)Landroidx/lifecycle/ViewModel;",
            store,
            [s(&mut e, "k2")]
        )
        .unwrap()),
        vm2
    );
    let set = ov(ci!(e, VIEW_MODEL_STORE, "keys", "()Ljava/util/Set;", store, []).unwrap());
    match e.fw.get(set) {
        Some(rd_vm::framework::HostObj::List { elems }) => assert_eq!(elems.len(), 2),
        other => panic!("keys não é Set host: {other:?}"),
    }
    ci!(e, VIEW_MODEL_STORE, "clear", "()V", store, []).unwrap();
    assert!(e.androidx_vm_store_keys(store).is_empty());

    let h = obj(&mut e, SAVED_STATE_HANDLE);
    ci!(e, SAVED_STATE_HANDLE, "<init>", "()V", h, []).unwrap();
    ci!(
        e,
        SAVED_STATE_HANDLE,
        "set",
        "(Ljava/lang/String;Ljava/lang/Object;)V",
        h,
        [s(&mut e, "n"), Value::Int(42)]
    )
    .unwrap();
    assert_eq!(
        ci!(
            e,
            SAVED_STATE_HANDLE,
            "get",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            h,
            [s(&mut e, "n")]
        )
        .unwrap(),
        Some(Value::Int(42))
    );
    assert_eq!(
        iv(ci!(
            e,
            SAVED_STATE_HANDLE,
            "contains",
            "(Ljava/lang/String;)Z",
            h,
            [s(&mut e, "n")]
        )
        .unwrap()),
        1
    );
    assert_eq!(
        ci!(
            e,
            SAVED_STATE_HANDLE,
            "remove",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            h,
            [s(&mut e, "n")]
        )
        .unwrap(),
        Some(Value::Int(42))
    );
    assert_eq!(
        ci!(
            e,
            SAVED_STATE_HANDLE,
            "get",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            h,
            [s(&mut e, "n")]
        )
        .unwrap(),
        Some(Value::Null)
    );
}

// ── collection ──────────────────────────────────────────────────────────
#[test]
fn dex_collection_array_map_set_sparse_lru() {
    let mut e = engine();
    let m = obj(&mut e, ARRAY_MAP);
    ci!(e, ARRAY_MAP, "<init>", "()V", m, []).unwrap();
    let (k1, v1) = (s(&mut e, "a"), s(&mut e, "1"));
    let (k2, v2) = (s(&mut e, "b"), s(&mut e, "2"));
    ci!(
        e,
        ARRAY_MAP,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        m,
        [k1.clone(), v1.clone()]
    )
    .unwrap();
    ci!(
        e,
        ARRAY_MAP,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        m,
        [k2.clone(), v2.clone()]
    )
    .unwrap();
    assert_eq!(iv(ci!(e, ARRAY_MAP, "size", "()I", m, []).unwrap()), 2);
    assert_eq!(
        ci!(
            e,
            ARRAY_MAP,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            m,
            [s(&mut e, "a")]
        )
        .unwrap(),
        Some(v1.clone())
    );
    assert_eq!(
        ci!(
            e,
            ARRAY_MAP,
            "keyAt",
            "(I)Ljava/lang/Object;",
            m,
            [Value::Int(1)]
        )
        .unwrap(),
        Some(k2.clone())
    );
    assert_eq!(
        ci!(
            e,
            ARRAY_MAP,
            "remove",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            m,
            [s(&mut e, "a")]
        )
        .unwrap(),
        Some(v1)
    );
    let err = ci!(
        e,
        ARRAY_MAP,
        "keyAt",
        "(I)Ljava/lang/Object;",
        m,
        [Value::Int(9)]
    )
    .unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Ljava/lang/ArrayIndexOutOfBoundsException;"
    );

    let set = obj(&mut e, ARRAY_SET);
    ci!(e, ARRAY_SET, "<init>", "()V", set, []).unwrap();
    assert_eq!(
        iv(ci!(
            e,
            ARRAY_SET,
            "add",
            "(Ljava/lang/Object;)Z",
            set,
            [v2.clone()]
        )
        .unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(
            e,
            ARRAY_SET,
            "add",
            "(Ljava/lang/Object;)Z",
            set,
            [v2.clone()]
        )
        .unwrap()),
        0
    );
    assert_eq!(iv(ci!(e, ARRAY_SET, "size", "()I", set, []).unwrap()), 1);

    let sp = obj(&mut e, SPARSE_ARRAY_COMPAT);
    ci!(e, SPARSE_ARRAY_COMPAT, "<init>", "()V", sp, []).unwrap();
    let sv = s(&mut e, "x");
    ci!(
        e,
        SPARSE_ARRAY_COMPAT,
        "put",
        "(ILjava/lang/Object;)V",
        sp,
        [Value::Int(5), sv.clone()]
    )
    .unwrap();
    ci!(
        e,
        SPARSE_ARRAY_COMPAT,
        "put",
        "(ILjava/lang/Object;)V",
        sp,
        [Value::Int(1), v2.clone()]
    )
    .unwrap();
    assert_eq!(
        iv(ci!(e, SPARSE_ARRAY_COMPAT, "keyAt", "(I)I", sp, [Value::Int(0)]).unwrap()),
        1
    );
    assert_eq!(
        ci!(
            e,
            SPARSE_ARRAY_COMPAT,
            "get",
            "(I)Ljava/lang/Object;",
            sp,
            [Value::Int(5)]
        )
        .unwrap(),
        Some(sv)
    );

    let ls = obj(&mut e, LONG_SPARSE_ARRAY);
    ci!(e, LONG_SPARSE_ARRAY, "<init>", "()V", ls, []).unwrap();
    ci!(
        e,
        LONG_SPARSE_ARRAY,
        "put",
        "(JLjava/lang/Object;)V",
        ls,
        [Value::Long(9_000_000_000), v2.clone()]
    )
    .unwrap();
    match ci!(e, LONG_SPARSE_ARRAY, "keyAt", "(I)J", ls, [Value::Int(0)]).unwrap() {
        Some(Value::Long(n)) => assert_eq!(n, 9_000_000_000),
        other => panic!("{other:?}"),
    }

    // LruCache(max=2): evicção do menos recente + contadores
    let lru = obj(&mut e, LRU_CACHE);
    ci!(e, LRU_CACHE, "<init>", "(I)V", lru, [Value::Int(2)]).unwrap();
    for (k, v) in [("a", "1"), ("b", "2"), ("c", "3")] {
        let (kk, vv) = (s(&mut e, k), s(&mut e, v));
        ci!(
            e,
            LRU_CACHE,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            lru,
            [kk, vv]
        )
        .unwrap();
    }
    assert_eq!(iv(ci!(e, LRU_CACHE, "size", "()I", lru, []).unwrap()), 2);
    assert_eq!(
        iv(ci!(e, LRU_CACHE, "evictionCount", "()I", lru, []).unwrap()),
        1
    );
    assert_eq!(
        ci!(
            e,
            LRU_CACHE,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            lru,
            [s(&mut e, "a")]
        )
        .unwrap(),
        Some(Value::Null),
        "'a' foi despejado"
    );
    assert_eq!(
        iv(ci!(e, LRU_CACHE, "missCount", "()I", lru, []).unwrap()),
        1
    );
    let err = ci!(
        e,
        LRU_CACHE,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        lru,
        [Value::Null, v2]
    )
    .unwrap_err();
    assert_eq!(exception_class(&err), "Ljava/lang/NullPointerException;");
}

// ── DataStore (camada de valores) ───────────────────────────────────────
#[test]
fn dex_datastore_preferences_value_layer() {
    let mut e = engine();
    let key = ov(cs!(
        e,
        "Landroidx/datastore/preferences/core/PreferencesKeysKt;",
        "stringPreferencesKey",
        "(Ljava/lang/String;)Landroidx/datastore/preferences/core/Preferences$Key;",
        [s(&mut e, "tema")]
    )
    .unwrap());
    let name = ov(ci!(
        e,
        PREFERENCES_KEY,
        "getName",
        "()Ljava/lang/String;",
        key,
        []
    )
    .unwrap());
    assert_eq!(e.heap.as_str(name).unwrap(), "tema");

    let mp = obj(&mut e, MUTABLE_PREFERENCES);
    ci!(e, MUTABLE_PREFERENCES, "<init>", "()V", mp, []).unwrap();
    let dark = s(&mut e, "dark");
    ci!(
        e,
        MUTABLE_PREFERENCES,
        "set",
        "(Landroidx/datastore/preferences/core/Preferences$Key;Ljava/lang/Object;)V",
        mp,
        [Value::Obj(key), dark.clone()]
    )
    .unwrap();
    assert_eq!(
        ci!(
            e,
            PREFERENCES,
            "get",
            "(Landroidx/datastore/preferences/core/Preferences$Key;)Ljava/lang/Object;",
            mp,
            [Value::Obj(key)]
        )
        .unwrap(),
        Some(dark)
    );
    assert_eq!(
        iv(ci!(
            e,
            PREFERENCES,
            "contains",
            "(Landroidx/datastore/preferences/core/Preferences$Key;)Z",
            mp,
            [Value::Obj(key)]
        )
        .unwrap()),
        1
    );
    let snap = ov(ci!(
        e,
        MUTABLE_PREFERENCES,
        "toPreferences",
        "()Landroidx/datastore/preferences/core/Preferences;",
        mp,
        []
    )
    .unwrap());
    let entries = e.androidx_preferences(snap).expect("snapshot de prefs");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "tema");

    let empty = ov(cs!(
        e,
        "Landroidx/datastore/preferences/core/PreferencesFactoryKt;",
        "emptyPreferences",
        "()Landroidx/datastore/preferences/core/Preferences;",
        []
    )
    .unwrap());
    assert_eq!(e.androidx_preferences(empty), Some(Vec::new()));
}

// ── Work Data ───────────────────────────────────────────────────────────
#[test]
fn dex_work_data_builder_roundtrip() {
    let mut e = engine();
    let b = obj(&mut e, WORK_DATA_BUILDER);
    ci!(e, WORK_DATA_BUILDER, "<init>", "()V", b, []).unwrap();
    ci!(
        e,
        WORK_DATA_BUILDER,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroidx/work/Data$Builder;",
        b,
        [s(&mut e, "in"), s(&mut e, "valor")]
    )
    .unwrap();
    ci!(
        e,
        WORK_DATA_BUILDER,
        "putInt",
        "(Ljava/lang/String;I)Landroidx/work/Data$Builder;",
        b,
        [s(&mut e, "n"), Value::Int(7)]
    )
    .unwrap();
    let data = ov(ci!(
        e,
        WORK_DATA_BUILDER,
        "build",
        "()Landroidx/work/Data;",
        b,
        []
    )
    .unwrap());
    let got = ov(ci!(
        e,
        WORK_DATA,
        "getString",
        "(Ljava/lang/String;)Ljava/lang/String;",
        data,
        [s(&mut e, "in")]
    )
    .unwrap());
    assert_eq!(e.heap.as_str(got).unwrap(), "valor");
    assert_eq!(
        iv(ci!(
            e,
            WORK_DATA,
            "getInt",
            "(Ljava/lang/String;I)I",
            data,
            [s(&mut e, "n"), Value::Int(-1)]
        )
        .unwrap()),
        7
    );
    assert_eq!(
        iv(ci!(
            e,
            WORK_DATA,
            "getInt",
            "(Ljava/lang/String;I)I",
            data,
            [s(&mut e, "ausente"), Value::Int(-3)]
        )
        .unwrap()),
        -3
    );
    let map = ov(ci!(
        e,
        WORK_DATA,
        "getKeyValueMap",
        "()Ljava/util/Map;",
        data,
        []
    )
    .unwrap());
    match e.fw.get(map) {
        Some(rd_vm::framework::HostObj::Map { entries }) => assert_eq!(entries.len(), 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(e.androidx_work_data(data).map(|m| m.len()), Some(2));
}

// ── Navigation / Startup / core ─────────────────────────────────────────
#[test]
fn dex_navigation_stack_model() {
    let mut e = engine();
    let nav = obj(&mut e, NAV_CONTROLLER);
    ci!(
        e,
        NAV_CONTROLLER,
        "<init>",
        "(Landroid/content/Context;)V",
        nav,
        [Value::Null]
    )
    .unwrap();
    // getGraph sem grafo: IllegalState honesto, não sucesso falso.
    let err = ci!(
        e,
        NAV_CONTROLLER,
        "getGraph",
        "()Landroidx/navigation/NavGraph;",
        nav,
        []
    )
    .unwrap_err();
    assert_eq!(exception_class(&err), "Ljava/lang/IllegalStateException;");
    ci!(e, NAV_CONTROLLER, "setGraph", "(I)V", nav, [Value::Int(7)]).unwrap();
    ci!(e, NAV_CONTROLLER, "navigate", "(I)V", nav, [Value::Int(11)]).unwrap();
    ci!(e, NAV_CONTROLLER, "navigate", "(I)V", nav, [Value::Int(12)]).unwrap();
    assert_eq!(e.androidx_nav_stack(nav), Some(vec![7, 11, 12]));
    let dest = ov(ci!(
        e,
        NAV_CONTROLLER,
        "getCurrentDestination",
        "()Landroidx/navigation/NavDestination;",
        nav,
        []
    )
    .unwrap());
    assert_eq!(
        iv(ci!(e, NAV_DESTINATION, "getId", "()I", dest, []).unwrap()),
        12
    );
    assert_eq!(
        iv(ci!(e, NAV_CONTROLLER, "popBackStack", "()Z", nav, []).unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(e, NAV_CONTROLLER, "navigateUp", "()Z", nav, []).unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(e, NAV_CONTROLLER, "navigateUp", "()Z", nav, []).unwrap()),
        0,
        "raiz não desempilha"
    );
    assert_eq!(e.androidx_nav_stack(nav), Some(vec![7]));
    assert_eq!(e.androidx_last_nav_controller(), Some(nav));
}

#[test]
fn dex_startup_multidex_fileprovider_permissions_cancellation() {
    let mut e = engine();
    let ai1 = ov(cs!(
        e,
        APP_INITIALIZER,
        "getInstance",
        "(Landroid/content/Context;)Landroidx/startup/AppInitializer;",
        [Value::Null]
    )
    .unwrap());
    let ai2 = ov(cs!(
        e,
        APP_INITIALIZER,
        "getInstance",
        "(Landroid/content/Context;)Landroidx/startup/AppInitializer;",
        [Value::Null]
    )
    .unwrap());
    assert_eq!(ai1, ai2);
    let cls = class_obj(&mut e, "Lcom/app/MeuInitializer;");
    ci!(
        e,
        APP_INITIALIZER,
        "initializeComponent",
        "(Ljava/lang/Class;)Ljava/lang/Object;",
        ai1,
        [Value::Obj(cls)]
    )
    .unwrap();
    assert_eq!(
        e.androidx_startup_initialized(),
        ["Lcom/app/MeuInitializer;"]
    );

    cs!(
        e,
        MULTIDEX,
        "install",
        "(Landroid/content/Context;)V",
        [Value::Null]
    )
    .unwrap();
    assert_eq!(e.androidx_multidex_installs(), 1);

    let file = obj(&mut e, "Ljava/io/File;");
    let p = s(&mut e, "/data/user/0/com.app/files/foto.jpg");
    e.heap.put_field(file, "path", p).unwrap();
    cs!(
        e,
        FILE_PROVIDER,
        "getUriForFile",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
        [Value::Null, s(&mut e, "com.app.provider"), Value::Obj(file)]
    )
    .unwrap();
    assert_eq!(
        e.androidx_file_provider_uris(),
        ["content://com.app.provider/rustydroid/data/user/0/com.app/files/foto.jpg"]
    );

    assert_eq!(
        iv(cs!(
            e,
            CONTEXT_COMPAT,
            "checkSelfPermission",
            "(Landroid/content/Context;Ljava/lang/String;)I",
            [Value::Null, s(&mut e, "android.permission.CAMERA")]
        )
        .unwrap()),
        0,
        "perfil default: concedido"
    );
    e.granted_permissions = Some(std::collections::HashSet::new());
    assert_eq!(
        iv(cs!(
            e,
            CONTEXT_COMPAT,
            "checkSelfPermission",
            "(Landroid/content/Context;Ljava/lang/String;)I",
            [Value::Null, s(&mut e, "android.permission.CAMERA")]
        )
        .unwrap()),
        -1,
        "perfil vazio: negado"
    );

    let sig = obj(&mut e, CANCELLATION_SIGNAL);
    ci!(e, CANCELLATION_SIGNAL, "<init>", "()V", sig, []).unwrap();
    ci!(e, CANCELLATION_SIGNAL, "throwIfCanceled", "()V", sig, []).unwrap();
    ci!(e, CANCELLATION_SIGNAL, "cancel", "()V", sig, []).unwrap();
    assert_eq!(
        iv(ci!(e, CANCELLATION_SIGNAL, "isCanceled", "()Z", sig, []).unwrap()),
        1
    );
    let err = ci!(e, CANCELLATION_SIGNAL, "throwIfCanceled", "()V", sig, []).unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Landroid/os/OperationCanceledException;"
    );
}

// ── Biometric / Custom Tabs / Futures ───────────────────────────────────
#[test]
fn dex_biometric_programmable_and_custom_tabs() {
    let mut e = engine();
    let bm = ov(cs!(
        e,
        BIOMETRIC_MANAGER,
        "from",
        "(Landroid/content/Context;)Landroidx/biometric/BiometricManager;",
        [Value::Null]
    )
    .unwrap());
    assert_eq!(
        iv(ci!(e, BIOMETRIC_MANAGER, "canAuthenticate", "()I", bm, []).unwrap()),
        BIOMETRIC_ERROR_NO_HARDWARE,
        "default honesto: sem hardware biométrico"
    );
    e.androidx_set_biometric(BIOMETRIC_SUCCESS, BiometricOutcome::Success);
    assert_eq!(
        iv(ci!(
            e,
            BIOMETRIC_MANAGER,
            "canAuthenticate",
            "(I)I",
            bm,
            [Value::Int(255)]
        )
        .unwrap()),
        0
    );

    let pb = obj(&mut e, PROMPT_INFO_BUILDER);
    ci!(e, PROMPT_INFO_BUILDER, "<init>", "()V", pb, []).unwrap();
    let err = ci!(
        e,
        PROMPT_INFO_BUILDER,
        "build",
        "()Landroidx/biometric/BiometricPrompt$PromptInfo;",
        pb,
        []
    )
    .unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Ljava/lang/IllegalArgumentException;"
    );
    ci!(
        e,
        PROMPT_INFO_BUILDER,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroidx/biometric/BiometricPrompt$PromptInfo$Builder;",
        pb,
        [s(&mut e, "Desbloquear")]
    )
    .unwrap();
    let info = ov(ci!(
        e,
        PROMPT_INFO_BUILDER,
        "build",
        "()Landroidx/biometric/BiometricPrompt$PromptInfo;",
        pb,
        []
    )
    .unwrap());

    let cb = obj(
        &mut e,
        "Landroidx/biometric/BiometricPrompt$AuthenticationCallback;",
    );
    let prompt = obj(&mut e, BIOMETRIC_PROMPT);
    ci!(e, BIOMETRIC_PROMPT, "<init>", "(Landroidx/fragment/app/FragmentActivity;Ljava/util/concurrent/Executor;Landroidx/biometric/BiometricPrompt$AuthenticationCallback;)V", prompt, [Value::Null, Value::Null, Value::Obj(cb)]).unwrap();
    ci!(
        e,
        BIOMETRIC_PROMPT,
        "authenticate",
        "(Landroidx/biometric/BiometricPrompt$PromptInfo;)V",
        prompt,
        [Value::Obj(info)]
    )
    .unwrap();
    assert_eq!(e.androidx_biometric_requests(), ["Desbloquear"]);

    // Custom Tabs: launchUrl vira deep link consultável.
    let url = s(&mut e, "https://example.test/ajuda");
    let uri = ov(rd_vm::m8::call_m8_static(
        &mut e,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[url],
    )
    .unwrap());
    let tab_builder = obj(&mut e, CUSTOM_TABS_BUILDER);
    let tab = ov(ci!(
        e,
        CUSTOM_TABS_BUILDER,
        "build",
        "()Landroidx/browser/customtabs/CustomTabsIntent;",
        tab_builder,
        []
    )
    .unwrap());
    ci!(
        e,
        CUSTOM_TABS_INTENT,
        "launchUrl",
        "(Landroid/content/Context;Landroid/net/Uri;)V",
        tab,
        [Value::Null, Value::Obj(uri)]
    )
    .unwrap();
    assert_eq!(
        e.androidx_custom_tab_launches(),
        ["https://example.test/ajuda"]
    );
    assert!(e
        .m8_deep_links()
        .iter()
        .any(|u| u == "https://example.test/ajuda"));
}

#[test]
fn dex_listenable_future_state_machine() {
    let mut e = engine();
    let f = e.androidx_new_future().unwrap();
    assert_eq!(
        iv(ci!(e, LISTENABLE_FUTURE, "isDone", "()Z", f, []).unwrap()),
        0
    );
    let err = ci!(e, LISTENABLE_FUTURE, "get", "()Ljava/lang/Object;", f, []).unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Ljava/util/concurrent/TimeoutException;"
    );
    let result = s(&mut e, "pronto");
    assert!(e.androidx_complete_future(f, result.clone()).unwrap());
    assert_eq!(
        ci!(e, LISTENABLE_FUTURE, "get", "()Ljava/lang/Object;", f, []).unwrap(),
        Some(result)
    );
    // Segunda conclusão é recusada (estado terminal).
    assert!(!e.androidx_complete_future(f, Value::Null).unwrap());

    let f2 = e.androidx_new_future().unwrap();
    assert_eq!(
        iv(ci!(e, LISTENABLE_FUTURE, "cancel", "(Z)Z", f2, [Value::Int(1)]).unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(e, LISTENABLE_FUTURE, "isCancelled", "()Z", f2, []).unwrap()),
        1
    );
    let err = ci!(e, LISTENABLE_FUTURE, "get", "()Ljava/lang/Object;", f2, []).unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Ljava/util/concurrent/CancellationException;"
    );

    // Future "pelado" alocado por outro shim (Operation do WorkManager):
    // comporta-se como concluído com null — nunca trava o guest.
    let bare = obj(
        &mut e,
        "Lcom/google/common/util/concurrent/ListenableFuture;",
    );
    assert_eq!(
        iv(ci!(e, LISTENABLE_FUTURE, "isDone", "()Z", bare, []).unwrap()),
        1
    );
    assert_eq!(
        ci!(
            e,
            LISTENABLE_FUTURE,
            "get",
            "()Ljava/lang/Object;",
            bare,
            []
        )
        .unwrap(),
        Some(Value::Null)
    );

    // SettableFuture.set pelo dispatch do guest.
    let sf = obj(&mut e, SETTABLE_FUTURE);
    ci!(e, SETTABLE_FUTURE, "<init>", "()V", sf, []).unwrap();
    let v = s(&mut e, "via guest");
    assert_eq!(
        iv(ci!(
            e,
            SETTABLE_FUTURE,
            "set",
            "(Ljava/lang/Object;)Z",
            sf,
            [v.clone()]
        )
        .unwrap()),
        1
    );
    assert_eq!(
        ci!(e, LISTENABLE_FUTURE, "get", "()Ljava/lang/Object;", sf, []).unwrap(),
        Some(v)
    );
}

// ── Invariante DEX-first ────────────────────────────────────────────────
#[test]
fn dex_body_wins_over_host_shim() {
    let mut b = DexBuilder::new();
    let cls = b.class("Landroidx/collection/ArrayMap;", "Ljava/lang/Object;");
    // <init>()V com corpo (só return-void basta para resolver).
    b.direct(
        cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | 0x1_0000,
        Some(b.code(1, 1, 0, op10x(0x0E))),
    );
    b.r#virtual(
        cls,
        "size",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op11n(0x12, 0, 7);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    let m = obj(&mut e, ARRAY_MAP);
    // Com corpo DEX, o host NÃO sequestra: None ⇒ o interpretador roda o DEX.
    assert_eq!(ci!(e, ARRAY_MAP, "<init>", "()V", m, []).unwrap(), None);
    assert_eq!(ci!(e, ARRAY_MAP, "size", "()I", m, []).unwrap(), None);
}

// ── Regressão (auditoria de estabilização): um Observer que faz
// setValue dentro de onChanged não pode recursar o despacho até ao
// SIGABRT do hospedeiro. Antes das flags dispatching/invalidated
// (paridade com o AOSP), este cenário abortava o processo.
#[test]
fn livedata_observer_setting_value_in_callback_is_bounded() {
    let mut b = DexBuilder::new();
    let cls = b.class("LLdLoop;", "Ljava/lang/Object;");
    b.static_field(cls, "calls", "I", common::SVal::Int(0));
    let f_calls = b.field_idx("LLdLoop;", "I", "calls");
    let p_set = b.proto_idx("V", vec!["Ljava/lang/Object;".into()]);
    let m_set = b.method_idx("Landroidx/lifecycle/MutableLiveData;", p_set, "setValue");
    b.r#virtual(
        cls,
        "onChanged",
        "V",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            let mut u = op21c(0x60, 0, f_calls);
            u.extend(op11n(0x12, 1, 1));
            u.extend(op23x(0x90, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_calls));
            // O argumento é a própria LiveData: arg.setValue(arg).
            u.extend(op35c(0x6E, 2, m_set, [4, 4, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    let ld = obj(&mut e, MUTABLE_LIVEDATA);
    ci!(e, MUTABLE_LIVEDATA, "<init>", "()V", ld, []).unwrap();
    let obs = obj(&mut e, "LLdLoop;");
    ci!(
        e,
        MUTABLE_LIVEDATA,
        "observeForever",
        "(Landroidx/lifecycle/Observer;)V",
        ld,
        [Value::Obj(obs)]
    )
    .unwrap();
    let r = ci!(
        e,
        MUTABLE_LIVEDATA,
        "setValue",
        "(Ljava/lang/Object;)V",
        ld,
        [Value::Obj(ld)]
    );
    assert!(r.is_ok(), "o setValue exterior conclui sem crash: {r:?}");
    let calls = match e
        .statics
        .get(&("LLdLoop;".to_string(), "calls".to_string()))
    {
        Some(Value::Int(n)) => *n,
        _ => 0,
    };
    assert!(calls > 1, "houve re-despacho iterativo (calls={calls})");
    assert!(calls <= 256, "rondas limitadas pelo teto (calls={calls})");
}
