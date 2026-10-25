//! Apêndice E — testes do runtime GMS/Firebase (`gms.rs`, modo "sem GMS"
//! honesto): Tasks API, disponibilidade, Firebase core/analytics/auth/
//! messaging/crashlytics/firestore/rtdb/remoteconfig/storage, Location,
//! Billing, Ads, Maps, Fit, Cast/Nearby, Integrity/SafetyNet e os
//! programadores `Engine::gms_*`. Nenhuma rede: tudo é estado local
//! programável, e as asserções leem o estado por ambos os lados
//! (convidado via dispatch, hospedeiro via `gms_*`).

#[macro_use]
mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::framework::HostObj;
use rd_vm::gms::*;
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
fn sv(e: &Engine, v: Option<Value>) -> String {
    match v {
        Some(Value::Obj(r)) => e.heap.as_str(r).unwrap_or("").to_string(),
        other => panic!("esperava String Obj, veio {other:?}"),
    }
}
fn ctx(e: &mut Engine) -> Value {
    Value::Obj(obj(e, "Landroid/content/Context;"))
}
fn host_list(e: &mut Engine, elems: Vec<Value>) -> Value {
    let r = obj(e, "Ljava/util/ArrayList;");
    e.fw.objects.insert(r, HostObj::List { elems });
    Value::Obj(r)
}
fn host_map(e: &mut Engine, entries: Vec<(Value, Value)>) -> Value {
    let r = obj(e, "Ljava/util/HashMap;");
    e.fw.objects.insert(r, HostObj::Map { entries });
    Value::Obj(r)
}
fn bundle(e: &mut Engine, entries: Vec<(String, Value)>) -> Value {
    let r = obj(e, "Landroid/os/Bundle;");
    e.fw.objects.insert(r, HostObj::Bundle { map: entries });
    Value::Obj(r)
}
macro_rules! svm {
    ($e:expr, $v:expr) => {{
        let __v = $v;
        sv(&$e, __v)
    }};
}
macro_rules! ub {
    ($e:expr, $v:expr) => {{
        let __v = $v;
        unboxed(&$e, __v)
    }};
}
fn exception_class(err: &VmExit) -> String {
    match err {
        VmExit::Exception(t) => t.class.clone(),
        other => panic!("esperava Exception, veio {other:?}"),
    }
}
fn unboxed(e: &Engine, v: Option<Value>) -> Value {
    match v {
        Some(Value::Obj(r)) => e.heap.get_field(r, "value").unwrap_or(Value::Null),
        other => panic!("esperava boxed Obj, veio {other:?}"),
    }
}

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

/// Classe DEX cujo método incrementa o estático `called` — prova que os
/// callbacks do shim executam bytecode real do convidado.
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
            let mut u = op21c(0x60, 0, f);
            u.extend(op11n(0x12, 1, 1));
            u.extend(op23x(0x90, 0, 0, 1));
            u.extend(op21c(0x67, 0, f));
            u.extend(op10x(0x0E));
            u
        })),
    );
}
fn called(e: &Engine, class: &str) -> i32 {
    match e.statics.get(&(class.to_string(), "called".to_string())) {
        Some(Value::Int(n)) => *n,
        None => 0,
        other => panic!("called de {class}: {other:?}"),
    }
}

/// Task → getResult via dispatch de instância.
fn task_result(mut e: &mut Engine, task: u32) -> Option<Value> {
    ci!(e, TASK, "getResult", "()Ljava/lang/Object;", task, []).unwrap()
}
fn task_flag(mut e: &mut Engine, task: u32, name: &str) -> i32 {
    iv(ci!(e, TASK, name, "()Z", task, []).unwrap())
}

// ── Tasks API ───────────────────────────────────────────────────────────
#[test]
fn tasks_completion_source_roundtrip_with_dex_listener() {
    let mut b = DexBuilder::new();
    counter_observer_class(&mut b, "LOkLis;", "onSuccess", vec!["Ljava/lang/Object;"]);
    let mut e = engine_of(&b);

    let tcs = obj(&mut e, TASK_SOURCE);
    ci!(e, TASK_SOURCE, "<init>", "()V", tcs, []).unwrap();
    let task = ov(ci!(
        e,
        TASK_SOURCE,
        "getTask",
        "()Lcom/google/android/gms/tasks/Task;",
        tcs,
        []
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, task, "isComplete"), 0);
    assert_eq!(e.gms_pending_tasks(), 1);

    // getResult numa Task incompleta é erro, nunca sucesso falso.
    let err = ci!(e, TASK, "getResult", "()Ljava/lang/Object;", task, []).unwrap_err();
    assert!(exception_class(&err).contains("IllegalState"), "{err:?}");

    let lis = obj(&mut e, "LOkLis;");
    ci!(
        e,
        TASK,
        "addOnSuccessListener",
        "(Lcom/google/android/gms/tasks/OnSuccessListener;)Lcom/google/android/gms/tasks/Task;",
        task,
        [Value::Obj(lis)]
    )
    .unwrap();
    assert_eq!(called(&e, "LOkLis;"), 0);

    let val = s(&mut e, "pronto");
    let ok = ci!(
        e,
        TASK_SOURCE,
        "trySetResult",
        "(Ljava/lang/Object;)Z",
        tcs,
        [val]
    )
    .unwrap();
    assert_eq!(iv(ok), 1);
    assert_eq!(called(&e, "LOkLis;"), 1, "listener DEX executou");
    assert_eq!(task_flag(&mut e, task, "isSuccessful"), 1);
    assert_eq!(svm!(e, task_result(&mut e, task)), "pronto");

    // Segunda conclusão é recusada (trySetResult → false).
    let v2 = s(&mut e, "tarde");
    let again = ci!(
        e,
        TASK_SOURCE,
        "trySetResult",
        "(Ljava/lang/Object;)Z",
        tcs,
        [v2]
    )
    .unwrap();
    assert_eq!(iv(again), 0);
    assert_eq!(e.gms_pending_tasks(), 0);
}

#[test]
fn tasks_failure_surfaces_exception() {
    let mut e = engine();
    let tcs = obj(&mut e, TASK_SOURCE);
    ci!(e, TASK_SOURCE, "<init>", "()V", tcs, []).unwrap();
    let task = ov(ci!(
        e,
        TASK_SOURCE,
        "getTask",
        "()Lcom/google/android/gms/tasks/Task;",
        tcs,
        []
    )
    .unwrap());

    let exc = obj(&mut e, "Ljava/lang/IllegalArgumentException;");
    let msg = s(&mut e, "mau argumento");
    e.heap.put_field(exc, "message", msg).unwrap();
    ci!(
        e,
        TASK_SOURCE,
        "setException",
        "(Ljava/lang/Exception;)V",
        tcs,
        [Value::Obj(exc)]
    )
    .unwrap();

    assert_eq!(task_flag(&mut e, task, "isComplete"), 1);
    assert_eq!(task_flag(&mut e, task, "isSuccessful"), 0);
    let got = ov(ci!(e, TASK, "getException", "()Ljava/lang/Exception;", task, []).unwrap());
    assert_eq!(got, exc);
    // getResult envolve a causa em RuntimeExecutionException (upstream).
    let err = ci!(e, TASK, "getResult", "()Ljava/lang/Object;", task, []).unwrap_err();
    assert_eq!(
        exception_class(&err),
        "Lcom/google/android/gms/tasks/RuntimeExecutionException;"
    );
}

// ── Disponibilidade / Common ────────────────────────────────────────────
#[test]
fn availability_programmable_codes() {
    let mut e = engine();
    let avail = ov(cs!(
        e,
        AVAILABILITY,
        "getInstance",
        "()Lcom/google/android/gms/common/GoogleApiAvailability;",
        []
    )
    .unwrap());
    let c = ctx(&mut e);
    let code = ci!(
        e,
        AVAILABILITY,
        "isGooglePlayServicesAvailable",
        "(Landroid/content/Context;)I",
        avail,
        [c]
    )
    .unwrap();
    assert_eq!(iv(code), 0, "SUCCESS por defeito");

    e.gms_set_availability(2);
    let c = ctx(&mut e);
    let code = ci!(
        e,
        AVAILABILITY,
        "isGooglePlayServicesAvailable",
        "(Landroid/content/Context;)I",
        avail,
        [c]
    )
    .unwrap();
    assert_eq!(iv(code), 2);
    assert_eq!(
        iv(ci!(
            e,
            AVAILABILITY,
            "isUserResolvableError",
            "(I)Z",
            avail,
            [Value::Int(2)]
        )
        .unwrap()),
        1
    );
    assert_eq!(
        iv(ci!(
            e,
            AVAILABILITY,
            "isUserResolvableError",
            "(I)Z",
            avail,
            [Value::Int(9)]
        )
        .unwrap()),
        0
    );
    let name = svm!(
        e,
        cs!(
            e,
            AVAILABILITY,
            "getErrorString",
            "(I)Ljava/lang/String;",
            [Value::Int(2)]
        )
        .unwrap()
    );
    assert_eq!(name, "SERVICE_VERSION_UPDATE_REQUIRED");
}

// ── Firebase core + Analytics ───────────────────────────────────────────
#[test]
fn firebase_autoinit_and_strict_mode() {
    let mut e = engine();
    // Auto-init por defeito: getInstance sem initializeApp vende a app falsa.
    let analytics = ov(cs!(
        e,
        FIREBASE_ANALYTICS,
        "getInstance",
        "(Landroid/content/Context;)Lcom/google/firebase/analytics/FirebaseAnalytics;",
        [ctx(&mut e)]
    )
    .unwrap());
    let app = ov(cs!(
        e,
        FIREBASE_APP,
        "getInstance",
        "()Lcom/google/firebase/FirebaseApp;",
        []
    )
    .unwrap());
    let opts = ov(ci!(
        e,
        FIREBASE_APP,
        "getOptions",
        "()Lcom/google/firebase/FirebaseOptions;",
        app,
        []
    )
    .unwrap());
    let proj = svm!(
        e,
        ci!(
            e,
            FIREBASE_OPTIONS,
            "getProjectId",
            "()Ljava/lang/String;",
            opts,
            []
        )
        .unwrap()
    );
    assert_eq!(proj, "rustydroid-fake-project");

    let name = s(&mut e, "select_content");
    let params = {
        let k = "item".to_string();
        let v = s(&mut e, "banana");
        bundle(&mut e, vec![(k, v)])
    };
    ci!(
        e,
        FIREBASE_ANALYTICS,
        "logEvent",
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        analytics,
        [name, params]
    )
    .unwrap();
    assert_eq!(e.gms_analytics_events().len(), 1);
    assert_eq!(e.gms_analytics_events()[0].name, "select_content");
    assert_eq!(
        e.gms_analytics_events()[0].params,
        vec![("item".to_string(), "banana".to_string())]
    );

    // Coleção desligada ⇒ eventos seguintes são largados (honesto).
    ci!(
        e,
        FIREBASE_ANALYTICS,
        "setAnalyticsCollectionEnabled",
        "(Z)V",
        analytics,
        [Value::Int(0)]
    )
    .unwrap();
    let n2 = s(&mut e, "hidden");
    ci!(
        e,
        FIREBASE_ANALYTICS,
        "logEvent",
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        analytics,
        [n2, Value::Null]
    )
    .unwrap();
    assert_eq!(e.gms_analytics_events().len(), 1);

    // Modo estrito: getInstance sem app inicializada lança, como upstream.
    let mut e2 = engine();
    e2.gms_firebase_strict_init(true);
    let err = cs!(
        e2,
        FIREBASE_ANALYTICS,
        "getInstance",
        "(Landroid/content/Context;)Lcom/google/firebase/analytics/FirebaseAnalytics;",
        [ctx(&mut e2)]
    )
    .unwrap_err();
    assert!(exception_class(&err).contains("IllegalState"), "{err:?}");
}

// ── Firebase Auth ───────────────────────────────────────────────────────
#[test]
fn firebase_auth_full_flow() {
    let mut e = engine();
    let auth = ov(cs!(
        e,
        FIREBASE_AUTH,
        "getInstance",
        "()Lcom/google/firebase/auth/FirebaseAuth;",
        []
    )
    .unwrap());
    assert_eq!(
        ci!(
            e,
            FIREBASE_AUTH,
            "getCurrentUser",
            "()Lcom/google/firebase/auth/FirebaseUser;",
            auth,
            []
        )
        .unwrap(),
        Some(Value::Null)
    );

    // Criar conta → AuthResult com o utilizador; fica com sessão iniciada.
    let email = s(&mut e, "user@noreply.github.com");
    let pwd = s(&mut e, "secret1");
    let task = ov(ci!(
        e,
        FIREBASE_AUTH,
        "createUserWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    let result = ov(task_result(&mut e, task));
    let user = ov(ci!(
        e,
        AUTH_RESULT,
        "getUser",
        "()Lcom/google/firebase/auth/FirebaseUser;",
        result,
        []
    )
    .unwrap());
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                FIREBASE_USER,
                "getEmail",
                "()Ljava/lang/String;",
                user,
                []
            )
            .unwrap()
        ),
        "user@noreply.github.com"
    );
    let cur = ov(ci!(
        e,
        FIREBASE_AUTH,
        "getCurrentUser",
        "()Lcom/google/firebase/auth/FirebaseUser;",
        auth,
        []
    )
    .unwrap());
    assert_eq!(cur, user);

    // Duplicado → colisão.
    let email = s(&mut e, "user@noreply.github.com");
    let pwd = s(&mut e, "secret1");
    let dup = ov(ci!(
        e,
        FIREBASE_AUTH,
        "createUserWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, dup, "isSuccessful"), 0);
    let exc = ov(ci!(e, TASK, "getException", "()Ljava/lang/Exception;", dup, []).unwrap());
    assert!(e.heap.class_of(exc).unwrap().contains("UserCollision"));

    // Senha fraca → erro específico.
    let email = s(&mut e, "other@x.pt");
    let pwd = s(&mut e, "123");
    let weak = ov(ci!(
        e,
        FIREBASE_AUTH,
        "createUserWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    let exc = ov(ci!(e, TASK, "getException", "()Ljava/lang/Exception;", weak, []).unwrap());
    assert!(e.heap.class_of(exc).unwrap().contains("WeakPassword"));

    // signOut + login com senha errada → credencial inválida; certa → OK.
    ci!(e, FIREBASE_AUTH, "signOut", "()V", auth, []).unwrap();
    let email = s(&mut e, "user@noreply.github.com");
    let pwd = s(&mut e, "wrong!!");
    let bad = ov(ci!(
        e,
        FIREBASE_AUTH,
        "signInWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    let exc = ov(ci!(e, TASK, "getException", "()Ljava/lang/Exception;", bad, []).unwrap());
    assert!(e.heap.class_of(exc).unwrap().contains("InvalidCredentials"));

    let email = s(&mut e, "user@noreply.github.com");
    let pwd = s(&mut e, "secret1");
    let good = ov(ci!(
        e,
        FIREBASE_AUTH,
        "signInWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, good, "isSuccessful"), 1);

    // Utilizador programado pelo hospedeiro também entra.
    e.gms_program_firebase_user("dono@loja.pt", "segredo");
    let email = s(&mut e, "dono@loja.pt");
    let pwd = s(&mut e, "segredo");
    let prog = ov(ci!(
        e,
        FIREBASE_AUTH,
        "signInWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, prog, "isSuccessful"), 1);
}

// ── Messaging + Crashlytics ─────────────────────────────────────────────
#[test]
fn messaging_token_topics_and_delivery() {
    let mut e = engine();
    let msg = ov(cs!(
        e,
        FIREBASE_MESSAGING,
        "getInstance",
        "()Lcom/google/firebase/messaging/FirebaseMessaging;",
        []
    )
    .unwrap());
    let t = ov(ci!(
        e,
        FIREBASE_MESSAGING,
        "getToken",
        "()Lcom/google/android/gms/tasks/Task;",
        msg,
        []
    )
    .unwrap());
    assert_eq!(svm!(e, task_result(&mut e, t)), "rustydroid-fcm-token-0001");
    e.gms_set_fcm_token("tok-42");
    let t = ov(ci!(
        e,
        FIREBASE_MESSAGING,
        "getToken",
        "()Lcom/google/android/gms/tasks/Task;",
        msg,
        []
    )
    .unwrap());
    assert_eq!(svm!(e, task_result(&mut e, t)), "tok-42");

    let topic = s(&mut e, "noticias");
    let sub = ov(ci!(
        e,
        FIREBASE_MESSAGING,
        "subscribeToTopic",
        "(Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        msg,
        [topic]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, sub, "isSuccessful"), 1);
    assert_eq!(e.gms_fcm_topics(), vec!["noticias".to_string()]);

    let delivered = e
        .gms_deliver_remote_message(
            "srv",
            vec![("k".into(), "v".into())],
            Some(("Ola", "Corpo")),
        )
        .unwrap();
    let rm = match delivered {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        svm!(
            e,
            ci!(e, REMOTE_MESSAGE, "getFrom", "()Ljava/lang/String;", rm, []).unwrap()
        ),
        "srv"
    );
    let notif = ov(ci!(
        e,
        REMOTE_MESSAGE,
        "getNotification",
        "()Lcom/google/firebase/messaging/RemoteMessage$Notification;",
        rm,
        []
    )
    .unwrap());
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                REMOTE_NOTIFICATION,
                "getTitle",
                "()Ljava/lang/String;",
                notif,
                []
            )
            .unwrap()
        ),
        "Ola"
    );
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                REMOTE_NOTIFICATION,
                "getBody",
                "()Ljava/lang/String;",
                notif,
                []
            )
            .unwrap()
        ),
        "Corpo"
    );
}

#[test]
fn crashlytics_records_and_uncaught_bridge() {
    let mut e = engine();
    let cl = ov(cs!(
        e,
        FIREBASE_CRASHLYTICS,
        "getInstance",
        "()Lcom/google/firebase/crashlytics/FirebaseCrashlytics;",
        []
    )
    .unwrap());
    let line = s(&mut e, "arranque ok");
    ci!(
        e,
        FIREBASE_CRASHLYTICS,
        "log",
        "(Ljava/lang/String;)V",
        cl,
        [line]
    )
    .unwrap();
    let exc = obj(&mut e, "Ljava/lang/IllegalStateException;");
    let m = s(&mut e, "estado mau");
    e.heap.put_field(exc, "message", m).unwrap();
    ci!(
        e,
        FIREBASE_CRASHLYTICS,
        "recordException",
        "(Ljava/lang/Throwable;)V",
        cl,
        [Value::Obj(exc)]
    )
    .unwrap();
    assert_eq!(e.gms_crashlytics_log(), vec!["arranque ok".to_string()]);
    assert_eq!(e.gms_crashlytics_crash_summaries().len(), 1);
    assert!(e.gms_crashlytics_crash_summaries()[0].contains("estado mau"));

    // O crash real da VM entra pela ponte do hospedeiro.
    e.gms_report_uncaught_crash("Ljava/lang/NullPointerException;", "npe no onCreate");
    let sums = e.gms_crashlytics_crash_summaries();
    assert_eq!(sums.len(), 2);
    assert!(sums[1].contains("uncaught") && sums[1].contains("npe no onCreate"));

    // Coleção desligada larga registos seguintes.
    ci!(
        e,
        FIREBASE_CRASHLYTICS,
        "setCrashlyticsCollectionEnabled",
        "(Z)V",
        cl,
        [Value::Int(0)]
    )
    .unwrap();
    let exc2 = obj(&mut e, "Ljava/lang/RuntimeException;");
    ci!(
        e,
        FIREBASE_CRASHLYTICS,
        "recordException",
        "(Ljava/lang/Throwable;)V",
        cl,
        [Value::Obj(exc2)]
    )
    .unwrap();
    assert_eq!(e.gms_crashlytics_crash_summaries().len(), 2);
}

// ── Firestore + RTDB ────────────────────────────────────────────────────
#[test]
fn firestore_set_get_query_and_seed() {
    let mut e = engine();
    let fs = ov(cs!(
        e,
        FIRESTORE,
        "getInstance",
        "()Lcom/google/firebase/Firestore/FirebaseFirestore;",
        []
    )
    .unwrap());
    let coll_name = s(&mut e, "users");
    let coll = ov(ci!(
        e,
        FIRESTORE,
        "collection",
        "(Ljava/lang/String;)Lcom/google/firebase/Firestore/CollectionReference;",
        fs,
        [coll_name]
    )
    .unwrap());
    let doc_id = s(&mut e, "u1");
    let doc = ov(ci!(
        e,
        COLLECTION_REF,
        "document",
        "(Ljava/lang/String;)Lcom/google/firebase/Firestore/DocumentReference;",
        coll,
        [doc_id]
    )
    .unwrap());

    let data = {
        let k1 = s(&mut e, "nome");
        let v1 = s(&mut e, "User");
        let k2 = s(&mut e, "idade");
        host_map(&mut e, vec![(k1, v1), (k2, Value::Int(30))])
    };
    let set_task = ov(ci!(
        e,
        DOC_REF,
        "set",
        "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
        doc,
        [data]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, set_task, "isSuccessful"), 1);

    // O hospedeiro lê o documento tal como ficou.
    let stored = e.gms_firestore_doc("users/u1").expect("doc gravado");
    assert!(stored.iter().any(|(k, _)| k == "nome"));

    let get_task = ov(ci!(
        e,
        DOC_REF,
        "get",
        "()Lcom/google/android/gms/tasks/Task;",
        doc,
        []
    )
    .unwrap());
    let snap = ov(task_result(&mut e, get_task));
    assert_eq!(iv(ci!(e, DOC_SNAP, "exists", "()Z", snap, []).unwrap()), 1);
    let field = s(&mut e, "nome");
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                DOC_SNAP,
                "getString",
                "(Ljava/lang/String;)Ljava/lang/String;",
                snap,
                [field]
            )
            .unwrap()
        ),
        "User"
    );

    // Documento inexistente: exists=false, sem sucesso falso.
    let missing_id = s(&mut e, "zzz");
    let missing = ov(ci!(
        e,
        COLLECTION_REF,
        "document",
        "(Ljava/lang/String;)Lcom/google/firebase/Firestore/DocumentReference;",
        coll,
        [missing_id]
    )
    .unwrap());
    let get2 = ov(ci!(
        e,
        DOC_REF,
        "get",
        "()Lcom/google/android/gms/tasks/Task;",
        missing,
        []
    )
    .unwrap());
    let snap2 = ov(task_result(&mut e, get2));
    assert_eq!(iv(ci!(e, DOC_SNAP, "exists", "()Z", snap2, []).unwrap()), 0);
}

#[test]
fn rtdb_set_resolve_and_listener() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LDbLis;",
        "onDataChange",
        vec!["Lcom/google/firebase/database/DataSnapshot;"],
    );
    let mut e = engine_of(&b);
    let db = ov(cs!(
        e,
        FIREBASE_DATABASE,
        "getInstance",
        "()Lcom/google/firebase/database/FirebaseDatabase;",
        []
    )
    .unwrap());
    let path = s(&mut e, "config/tema");
    let reference = ov(ci!(
        e,
        FIREBASE_DATABASE,
        "getReference",
        "(Ljava/lang/String;)Lcom/google/firebase/database/DatabaseReference;",
        db,
        [path]
    )
    .unwrap());
    let v = s(&mut e, "escuro");
    let t = ov(ci!(
        e,
        DATABASE_REF,
        "setValue",
        "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
        reference,
        [v]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    match e.gms_rtdb_value("config/tema") {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "escuro"),
        other => panic!("valor RTDB: {other:?}"),
    }

    let lis = obj(&mut e, "LDbLis;");
    ci!(
        e,
        DATABASE_REF,
        "addListenerForSingleValueEvent",
        "(Lcom/google/firebase/database/ValueEventListener;)V",
        reference,
        [Value::Obj(lis)]
    )
    .unwrap();
    assert_eq!(called(&e, "LDbLis;"), 1);
}

// ── RemoteConfig + Storage ──────────────────────────────────────────────
#[test]
fn remoteconfig_fetch_activate_semantics() {
    let mut e = engine();
    let rc = ov(cs!(
        e,
        REMOTE_CONFIG,
        "getInstance",
        "()Lcom/google/firebase/RemoteConfig/FirebaseRemoteConfig;",
        []
    )
    .unwrap());
    let key = s(&mut e, "banner");
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                REMOTE_CONFIG,
                "getString",
                "(Ljava/lang/String;)Ljava/lang/String;",
                rc,
                [key]
            )
            .unwrap()
        ),
        ""
    );

    e.gms_remoteconfig_program(vec![("banner".into(), "promo".into())]);
    let t = ov(ci!(
        e,
        REMOTE_CONFIG,
        "fetchAndActivate",
        "()Lcom/google/android/gms/tasks/Task;",
        rc,
        []
    )
    .unwrap());
    assert_eq!(
        ub!(e, task_result(&mut e, t)),
        Value::Int(1),
        "valores mudaram"
    );
    let key = s(&mut e, "banner");
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                REMOTE_CONFIG,
                "getString",
                "(Ljava/lang/String;)Ljava/lang/String;",
                rc,
                [key]
            )
            .unwrap()
        ),
        "promo"
    );

    let t2 = ov(ci!(
        e,
        REMOTE_CONFIG,
        "fetchAndActivate",
        "()Lcom/google/android/gms/tasks/Task;",
        rc,
        []
    )
    .unwrap());
    assert_eq!(
        ub!(e, task_result(&mut e, t2)),
        Value::Int(0),
        "sem mudancas"
    );
}

#[test]
fn storage_seed_download_and_missing() {
    let mut e = engine();
    e.gms_storage_seed("docs/a.bin", vec![1, 2, 3, 4]);
    let st = ov(cs!(
        e,
        FIREBASE_STORAGE,
        "getInstance",
        "()Lcom/google/firebase/Storage/FirebaseStorage;",
        []
    )
    .unwrap());
    let root = ov(ci!(
        e,
        FIREBASE_STORAGE,
        "getReference",
        "()Lcom/google/firebase/Storage/StorageReference;",
        st,
        []
    )
    .unwrap());
    let child_name = s(&mut e, "docs/a.bin");
    let reference = ov(ci!(
        e,
        STORAGE_REF,
        "child",
        "(Ljava/lang/String;)Lcom/google/firebase/Storage/StorageReference;",
        root,
        [child_name]
    )
    .unwrap());
    let t = ov(ci!(
        e,
        STORAGE_REF,
        "getBytes",
        "(J)Lcom/google/android/gms/tasks/Task;",
        reference,
        [Value::Long(1024)]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    let url_task = ov(ci!(
        e,
        STORAGE_REF,
        "getDownloadUrl",
        "()Lcom/google/android/gms/tasks/Task;",
        reference,
        []
    )
    .unwrap());
    let uri = ov(task_result(&mut e, url_task));
    assert_eq!(e.heap.class_of(uri).unwrap(), "Landroid/net/Uri;");

    let missing_name = s(&mut e, "nope.bin");
    let missing = ov(ci!(
        e,
        STORAGE_REF,
        "child",
        "(Ljava/lang/String;)Lcom/google/firebase/Storage/StorageReference;",
        root,
        [missing_name]
    )
    .unwrap());
    let t2 = ov(ci!(
        e,
        STORAGE_REF,
        "getBytes",
        "(J)Lcom/google/android/gms/tasks/Task;",
        missing,
        [Value::Long(10)]
    )
    .unwrap());
    assert_eq!(
        task_flag(&mut e, t2, "isSuccessful"),
        0,
        "blob ausente falha honestamente"
    );
}

// ── Location ────────────────────────────────────────────────────────────
#[test]
fn location_fix_and_callback_delivery() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LLocCb;",
        "onLocationResult",
        vec!["Lcom/google/android/gms/location/LocationResult;"],
    );
    let mut e = engine_of(&b);
    let fused =
        ov(cs!(e, LOCATION_SERVICES, "getFusedLocationProviderClient",
        "(Landroid/content/Context;)Lcom/google/android/gms/location/FusedLocationProviderClient;",
        [ctx(&mut e)])
        .unwrap());

    // Sem fix: a Task conclui com null (como um aparelho sem última posição).
    let t = ov(ci!(
        e,
        FUSED_CLIENT,
        "getLastLocation",
        "()Lcom/google/android/gms/tasks/Task;",
        fused,
        []
    )
    .unwrap());
    assert_eq!(task_result(&mut e, t), Some(Value::Null));

    e.gms_set_location_fix(-23.55, -46.63, 8.0);
    let t = ov(ci!(
        e,
        FUSED_CLIENT,
        "getLastLocation",
        "()Lcom/google/android/gms/tasks/Task;",
        fused,
        []
    )
    .unwrap());
    let loc = ov(task_result(&mut e, t));
    assert_eq!(e.heap.class_of(loc).unwrap(), "Landroid/location/Location;");
    match e.heap.get_field(loc, "mLatitudeDegrees").unwrap() {
        Value::Double(d) => assert!((d - (-23.55)).abs() < 1e-9),
        other => panic!("latitude: {other:?}"),
    }

    // requestLocationUpdates entrega já ao registar; push entrega outra vez.
    let req = ov(cs!(
        e,
        LOCATION_REQUEST,
        "create",
        "()Lcom/google/android/gms/location/LocationRequest;",
        []
    )
    .unwrap());
    let cb = obj(&mut e, "LLocCb;");
    let t = ov(ci!(e, FUSED_CLIENT, "requestLocationUpdates",
        "(Lcom/google/android/gms/location/LocationRequest;Lcom/google/android/gms/location/LocationCallback;Landroid/os/Looper;)Lcom/google/android/gms/tasks/Task;",
        fused, [Value::Obj(req), Value::Obj(cb), Value::Null]).unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    assert_eq!(called(&e, "LLocCb;"), 1);
    assert_eq!(e.gms_location_callback_count(), 1);
    e.gms_push_location(-23.56, -46.64, 5.0);
    assert_eq!(called(&e, "LLocCb;"), 2);

    let t =
        ov(ci!(e, FUSED_CLIENT, "removeLocationUpdates",
        "(Lcom/google/android/gms/location/LocationCallback;)Lcom/google/android/gms/tasks/Task;",
        fused, [Value::Obj(cb)])
        .unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    assert_eq!(e.gms_location_callback_count(), 0);
}

// ── Billing ─────────────────────────────────────────────────────────────
fn billing_client_with_listeners(mut e: &mut Engine, purchases_lis: u32) -> u32 {
    let builder = ov(cs!(
        e,
        BILLING_CLIENT,
        "newBuilder",
        "(Landroid/content/Context;)Lcom/android/billingclient/api/BillingClient$Builder;",
        [ctx(e)]
    )
    .unwrap());
    ci!(e, BILLING_CLIENT_BUILDER, "setListener",
        "(Lcom/android/billingclient/api/PurchasesUpdatedListener;)Lcom/android/billingclient/api/BillingClient$Builder;",
        builder, [Value::Obj(purchases_lis)]).unwrap();
    ov(ci!(
        e,
        BILLING_CLIENT_BUILDER,
        "build",
        "()Lcom/android/billingclient/api/BillingClient;",
        builder,
        []
    )
    .unwrap())
}

#[test]
fn billing_purchase_flow_and_cancel() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LPurLis;",
        "onPurchasesUpdated",
        vec![
            "Lcom/android/billingclient/api/BillingResult;",
            "Ljava/util/List;",
        ],
    );
    counter_observer_class(
        &mut b,
        "LStateLis;",
        "onBillingSetupFinished",
        vec!["Lcom/android/billingclient/api/BillingResult;"],
    );
    counter_observer_class(
        &mut b,
        "LSkuLis;",
        "onSkuDetailsResponse",
        vec![
            "Lcom/android/billingclient/api/BillingResult;",
            "Ljava/util/List;",
        ],
    );
    let mut e = engine_of(&b);
    e.gms_billing_add_sku(
        "coins100",
        "inapp",
        "R$ 4,99",
        4_990_000,
        "BRL",
        "100 moedas",
    );

    let purchases_lis = obj(&mut e, "LPurLis;");
    let client = billing_client_with_listeners(&mut e, purchases_lis);
    let state_lis = obj(&mut e, "LStateLis;");
    ci!(
        e,
        BILLING_CLIENT,
        "startConnection",
        "(Lcom/android/billingclient/api/BillingClientStateListener;)V",
        client,
        [Value::Obj(state_lis)]
    )
    .unwrap();
    assert_eq!(called(&e, "LStateLis;"), 1);

    // Catálogo legado (SkuDetails).
    let params_builder = ov(cs!(
        e,
        SKU_PARAMS,
        "newBuilder",
        "()Lcom/android/billingclient/api/SkuDetailsParams$Builder;",
        []
    )
    .unwrap());
    let skus = {
        let id = s(&mut e, "coins100");
        host_list(&mut e, vec![id])
    };
    ci!(
        e,
        SKU_PARAMS_BUILDER,
        "setSkusList",
        "(Ljava/util/List;)Lcom/android/billingclient/api/SkuDetailsParams$Builder;",
        params_builder,
        [skus]
    )
    .unwrap();
    let typ = s(&mut e, "inapp");
    ci!(
        e,
        SKU_PARAMS_BUILDER,
        "setType",
        "(Ljava/lang/String;)Lcom/android/billingclient/api/SkuDetailsParams$Builder;",
        params_builder,
        [typ]
    )
    .unwrap();
    let params = ov(ci!(
        e,
        SKU_PARAMS_BUILDER,
        "build",
        "()Lcom/android/billingclient/api/SkuDetailsParams;",
        params_builder,
        []
    )
    .unwrap());
    let sku_listener = obj(&mut e, "LSkuLis;");
    ci!(e, BILLING_CLIENT, "querySkuDetailsAsync",
        "(Lcom/android/billingclient/api/SkuDetailsParams;Lcom/android/billingclient/api/SkuDetailsResponseListener;)V",
        client, [Value::Obj(params), Value::Obj(sku_listener)]).unwrap();
    assert_eq!(called(&e, "LSkuLis;"), 1);

    // launchBillingFlow precisa dos SkuDetails: obtém via estado do shim.
    let sku_obj = *e
        .fw
        .gms
        .sku_details_objs
        .keys()
        .next()
        .expect("SkuDetails cunhado");
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                SKU_DETAILS,
                "getSku",
                "()Ljava/lang/String;",
                sku_obj,
                []
            )
            .unwrap()
        ),
        "coins100"
    );
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                SKU_DETAILS,
                "getPrice",
                "()Ljava/lang/String;",
                sku_obj,
                []
            )
            .unwrap()
        ),
        "R$ 4,99"
    );

    let flow_builder = ov(cs!(
        e,
        FLOW_PARAMS,
        "newBuilder",
        "()Lcom/android/billingclient/api/BillingFlowParams$Builder;",
        []
    )
    .unwrap());
    ci!(e, FLOW_PARAMS_BUILDER, "setSkuDetails",
        "(Lcom/android/billingclient/api/SkuDetails;)Lcom/android/billingclient/api/BillingFlowParams$Builder;",
        flow_builder, [Value::Obj(sku_obj)]).unwrap();
    let flow = ov(ci!(
        e,
        FLOW_PARAMS_BUILDER,
        "build",
        "()Lcom/android/billingclient/api/BillingFlowParams;",
        flow_builder,
        []
    )
    .unwrap());
    let activity = ctx(&mut e);
    let result = ov(ci!(e, BILLING_CLIENT, "launchBillingFlow",
        "(Landroid/app/Activity;Lcom/android/billingclient/api/BillingFlowParams;)Lcom/android/billingclient/api/BillingResult;",
        client, [activity, Value::Obj(flow)]).unwrap());
    assert_eq!(
        iv(ci!(e, BILLING_RESULT, "getResponseCode", "()I", result, []).unwrap()),
        0
    );
    assert_eq!(
        called(&e, "LPurLis;"),
        1,
        "PurchasesUpdatedListener executou em DEX"
    );
    let purchases = e.gms_billing_purchases();
    assert_eq!(purchases.len(), 1);
    assert!(purchases[0].token.starts_with("rustydroid-purchase-token-"));
    assert_eq!(purchases[0].products, vec!["coins100".to_string()]);

    // queryPurchases devolve a compra vendida.
    let typ = s(&mut e, "inapp");
    let pr = ov(ci!(
        e,
        BILLING_CLIENT,
        "queryPurchases",
        "(Ljava/lang/String;)Lcom/android/billingclient/api/Purchase$PurchasesResult;",
        client,
        [typ]
    )
    .unwrap());
    assert_eq!(
        iv(ci!(e, PURCHASES_RESULT, "getResponseCode", "()I", pr, []).unwrap()),
        0
    );
    let list = ov(ci!(
        e,
        PURCHASES_RESULT,
        "getPurchasesList",
        "()Ljava/util/List;",
        pr,
        []
    )
    .unwrap());
    match e.fw.objects.get(&list) {
        Some(HostObj::List { elems }) => assert_eq!(elems.len(), 1),
        other => panic!("purchases list: {other:?}"),
    }

    // Utilizador cancela: o lançamento continua OK (upstream), mas o
    // listener recebe USER_CANCELED e nenhuma compra nova aparece.
    e.gms_billing_set_outcome(BillingOutcome::Canceled);
    let activity = ctx(&mut e);
    let result = ov(ci!(e, BILLING_CLIENT, "launchBillingFlow",
        "(Landroid/app/Activity;Lcom/android/billingclient/api/BillingFlowParams;)Lcom/android/billingclient/api/BillingResult;",
        client, [activity, Value::Obj(flow)]).unwrap());
    assert_eq!(
        iv(ci!(e, BILLING_RESULT, "getResponseCode", "()I", result, []).unwrap()),
        0
    );
    assert_eq!(called(&e, "LPurLis;"), 2);
    assert_eq!(e.gms_billing_purchases().len(), 1);
}

// ── Ads ─────────────────────────────────────────────────────────────────
#[test]
fn ads_banner_success_failure_and_rewarded() {
    let mut b = DexBuilder::new();
    counter_observer_class(&mut b, "LAdLis;", "onAdLoaded", vec![]);
    counter_observer_class(
        &mut b,
        "LAdFail;",
        "onAdFailedToLoad",
        vec!["Lcom/google/android/gms/ads/LoadAdError;"],
    );
    let mut e = engine_of(&b);

    cs!(
        e,
        MOBILE_ADS,
        "initialize",
        "(Landroid/content/Context;)V",
        [ctx(&mut e)]
    )
    .unwrap();
    assert!(e.fw.gms.ads_initialized);

    let view = obj(&mut e, AD_VIEW);
    ci!(
        e,
        AD_VIEW,
        "<init>",
        "(Landroid/content/Context;)V",
        view,
        [ctx(&mut e)]
    )
    .unwrap();
    let unit = s(&mut e, "ca-app-pub-test/banner");
    ci!(
        e,
        AD_VIEW,
        "setAdUnitId",
        "(Ljava/lang/String;)V",
        view,
        [unit]
    )
    .unwrap();
    let lis = obj(&mut e, "LAdLis;");
    ci!(
        e,
        AD_VIEW,
        "setAdListener",
        "(Lcom/google/android/gms/ads/AdListener;)V",
        view,
        [Value::Obj(lis)]
    )
    .unwrap();

    let req_builder = obj(&mut e, AD_REQUEST_BUILDER);
    ci!(e, AD_REQUEST_BUILDER, "<init>", "()V", req_builder, []).unwrap();
    let req = ov(ci!(
        e,
        AD_REQUEST_BUILDER,
        "build",
        "()Lcom/google/android/gms/ads/AdRequest;",
        req_builder,
        []
    )
    .unwrap());
    ci!(
        e,
        AD_VIEW,
        "loadAd",
        "(Lcom/google/android/gms/ads/AdRequest;)V",
        view,
        [Value::Obj(req)]
    )
    .unwrap();
    assert_eq!(called(&e, "LAdLis;"), 1);
    assert!(e.gms_ads_events().iter().any(|ev| ev.contains("banner")));

    // Sem fill programado: onAdFailedToLoad com código 3.
    e.gms_ads_set_failure(3);
    let fail_lis = obj(&mut e, "LAdFail;");
    ci!(
        e,
        AD_VIEW,
        "setAdListener",
        "(Lcom/google/android/gms/ads/AdListener;)V",
        view,
        [Value::Obj(fail_lis)]
    )
    .unwrap();
    ci!(
        e,
        AD_VIEW,
        "loadAd",
        "(Lcom/google/android/gms/ads/AdRequest;)V",
        view,
        [Value::Obj(req)]
    )
    .unwrap();
    assert_eq!(called(&e, "LAdFail;"), 1);
    e.gms_ads_clear_failure();
}

// ── Maps + Fit ──────────────────────────────────────────────────────────
#[test]
fn maps_ready_camera_and_markers() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LMapCb;",
        "onMapReady",
        vec!["Lcom/google/android/gms/maps/GoogleMap;"],
    );
    let mut e = engine_of(&b);
    let frag = ov(cs!(
        e,
        SUPPORT_MAP_FRAGMENT,
        "newInstance",
        "()Lcom/google/android/gms/maps/SupportMapFragment;",
        []
    )
    .unwrap());
    let cb = obj(&mut e, "LMapCb;");
    ci!(
        e,
        SUPPORT_MAP_FRAGMENT,
        "getMapAsync",
        "(Lcom/google/android/gms/maps/OnMapReadyCallback;)V",
        frag,
        [Value::Obj(cb)]
    )
    .unwrap();
    assert_eq!(called(&e, "LMapCb;"), 1);

    let map = *e.fw.gms.google_maps.keys().next().expect("mapa cunhado");
    let ll = obj(&mut e, LATLNG);
    ci!(
        e,
        LATLNG,
        "<init>",
        "(DD)V",
        ll,
        [Value::Double(-23.55), Value::Double(-46.63)]
    )
    .unwrap();
    let upd =
        ov(cs!(e, CAMERA_UPDATE_FACTORY, "newLatLngZoom",
        "(Lcom/google/android/gms/maps/model/LatLng;F)Lcom/google/android/gms/maps/CameraUpdate;",
        [Value::Obj(ll), Value::Float(12.0)])
        .unwrap());
    ci!(
        e,
        GOOGLE_MAP,
        "moveCamera",
        "(Lcom/google/android/gms/maps/CameraUpdate;)V",
        map,
        [Value::Obj(upd)]
    )
    .unwrap();
    let cams = e.gms_maps_cameras();
    assert_eq!(cams.len(), 1);
    assert!((cams[0].0 - (-23.55)).abs() < 1e-9 && (cams[0].2 - 12.0).abs() < 1e-6);

    let opts = obj(&mut e, MARKER_OPTIONS);
    ci!(e, MARKER_OPTIONS, "<init>", "()V", opts, []).unwrap();
    ci!(e, MARKER_OPTIONS, "position",
        "(Lcom/google/android/gms/maps/model/LatLng;)Lcom/google/android/gms/maps/model/MarkerOptions;",
        opts, [Value::Obj(ll)]).unwrap();
    let title = s(&mut e, "Loja");
    ci!(
        e,
        MARKER_OPTIONS,
        "title",
        "(Ljava/lang/String;)Lcom/google/android/gms/maps/model/MarkerOptions;",
        opts,
        [title]
    )
    .unwrap();
    ci!(e, GOOGLE_MAP, "addMarker",
        "(Lcom/google/android/gms/maps/model/MarkerOptions;)Lcom/google/android/gms/maps/model/Marker;",
        map, [Value::Obj(opts)]).unwrap();
    let markers = e.gms_maps_markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].2, "Loja");
}

#[test]
fn fit_steps_programmable() {
    let mut e = engine();
    e.gms_fit_set_steps(4321);
    let history = ov(cs!(e, FITNESS, "getHistoryClient",
        "(Landroid/content/Context;Lcom/google/android/gms/auth/api/signin/GoogleSignInAccount;)Lcom/google/android/gms/fitness/HistoryClient;",
        [ctx(&mut e), Value::Null]).unwrap());
    let t = ov(ci!(
        e,
        "Lcom/google/android/gms/fitness/HistoryClient;",
        "readDailyTotal",
        "(Lcom/google/android/gms/fitness/data/DataType;)Lcom/google/android/gms/tasks/Task;",
        history,
        [Value::Null]
    )
    .unwrap());
    let data_set = ov(task_result(&mut e, t));
    let points = ov(ci!(
        e,
        DATA_SET,
        "getDataPoints",
        "()Ljava/util/List;",
        data_set,
        []
    )
    .unwrap());
    let point = match e.fw.objects.get(&points).cloned() {
        Some(HostObj::List { elems }) => match elems.first() {
            Some(Value::Obj(r)) => *r,
            other => panic!("datapoint: {other:?}"),
        },
        other => panic!("points: {other:?}"),
    };
    let field = obj(&mut e, "Lcom/google/android/gms/fitness/data/Field;");
    let value =
        ov(ci!(e, DATA_POINT, "getValue",
        "(Lcom/google/android/gms/fitness/data/Field;)Lcom/google/android/gms/fitness/data/Value;",
        point, [Value::Obj(field)])
        .unwrap());
    assert_eq!(
        iv(ci!(e, FIT_VALUE, "asInt", "()I", value, []).unwrap()),
        4321
    );
}

// ── Cast / Nearby / Integridade ─────────────────────────────────────────
#[test]
fn cast_states_follow_programmed_session() {
    let mut e = engine();
    let cc = ov(cs!(
        e,
        CAST_CONTEXT,
        "getSharedInstance",
        "(Landroid/content/Context;)Lcom/google/android/gms/cast/framework/CastContext;",
        [ctx(&mut e)]
    )
    .unwrap());
    assert_eq!(
        iv(ci!(e, CAST_CONTEXT, "getCastState", "()I", cc, []).unwrap()),
        1,
        "NO_DEVICES"
    );
    e.gms_cast_connect("TV da Sala");
    assert_eq!(
        iv(ci!(e, CAST_CONTEXT, "getCastState", "()I", cc, []).unwrap()),
        4,
        "CONNECTED"
    );
    let sm = ov(ci!(
        e,
        CAST_CONTEXT,
        "getSessionManager",
        "()Lcom/google/android/gms/cast/framework/SessionManager;",
        cc,
        []
    )
    .unwrap());
    let session = ci!(
        e,
        SESSION_MANAGER,
        "getCurrentCastSession",
        "()Lcom/google/android/gms/cast/framework/CastSession;",
        sm,
        []
    )
    .unwrap();
    assert!(matches!(session, Some(Value::Obj(_))));
    e.gms_cast_disconnect();
    assert_eq!(
        iv(ci!(e, CAST_CONTEXT, "getCastState", "()I", cc, []).unwrap()),
        1
    );
}

#[test]
fn nearby_discovery_reports_programmed_endpoint() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LEpCb;",
        "onEndpointFound",
        vec![
            "Ljava/lang/String;",
            "Lcom/google/android/gms/nearby/connection/DiscoveredEndpointInfo;",
        ],
    );
    let mut e = engine_of(&b);
    e.gms_nearby_add_endpoint("ep-1", "Tablet do balcao");
    let client =
        ov(cs!(e, NEARBY, "getConnectionsClient",
        "(Landroid/content/Context;)Lcom/google/android/gms/nearby/connection/ConnectionsClient;",
        [ctx(&mut e)])
        .unwrap());
    let service = s(&mut e, "pt.rustydroid.demo");
    let cb = obj(&mut e, "LEpCb;");
    let t = ov(ci!(e, "Lcom/google/android/gms/nearby/connection/ConnectionsClient;", "startDiscovery",
        "(Ljava/lang/String;Lcom/google/android/gms/nearby/connection/EndpointDiscoveryCallback;Lcom/google/android/gms/nearby/connection/DiscoveryOptions;)Lcom/google/android/gms/tasks/Task;",
        client, [service, Value::Obj(cb), Value::Null]).unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    assert_eq!(called(&e, "LEpCb;"), 1);
}

#[test]
fn integrity_and_safetynet_deterministic_tokens() {
    let mut e = engine();
    let manager = ov(cs!(
        e,
        INTEGRITY_FACTORY,
        "create",
        "(Landroid/content/Context;)Lcom/google/android/play/core/integrity/IntegrityManager;",
        [ctx(&mut e)]
    )
    .unwrap());
    let builder = ov(cs!(
        e,
        INTEGRITY_TOKEN_REQUEST,
        "newBuilder",
        "()Lcom/google/android/play/core/integrity/IntegrityTokenRequest$Builder;",
        []
    )
    .unwrap());
    let nonce = s(&mut e, "nonce-123");
    ci!(e, INTEGRITY_TOKEN_BUILDER, "setNonce",
        "(Ljava/lang/String;)Lcom/google/android/play/core/integrity/IntegrityTokenRequest$Builder;",
        builder, [nonce]).unwrap();
    let request = ov(ci!(
        e,
        INTEGRITY_TOKEN_BUILDER,
        "build",
        "()Lcom/google/android/play/core/integrity/IntegrityTokenRequest;",
        builder,
        []
    )
    .unwrap());
    let t = ov(ci!(e, "Lcom/google/android/play/core/integrity/IntegrityManager;", "requestIntegrityToken",
        "(Lcom/google/android/play/core/integrity/IntegrityTokenRequest;)Lcom/google/android/gms/tasks/Task;",
        manager, [Value::Obj(request)]).unwrap());
    let resp = ov(task_result(&mut e, t));
    let tok = svm!(
        e,
        ci!(
            e,
            INTEGRITY_TOKEN_RESPONSE,
            "token",
            "()Ljava/lang/String;",
            resp,
            []
        )
        .unwrap()
    );
    assert_eq!(tok, "rustydroid-fake-integrity-token");
    assert_eq!(
        e.gms_integrity_nonces(),
        vec!["integrity:nonce-123".to_string()]
    );

    e.gms_set_integrity_token("tok-configurado");
    let t2 = ov(ci!(e, "Lcom/google/android/play/core/integrity/IntegrityManager;", "requestIntegrityToken",
        "(Lcom/google/android/play/core/integrity/IntegrityTokenRequest;)Lcom/google/android/gms/tasks/Task;",
        manager, [Value::Obj(request)]).unwrap());
    let resp2 = ov(task_result(&mut e, t2));
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                INTEGRITY_TOKEN_RESPONSE,
                "token",
                "()Ljava/lang/String;",
                resp2,
                []
            )
            .unwrap()
        ),
        "tok-configurado"
    );

    let sn = ov(cs!(
        e,
        SAFETYNET,
        "getClient",
        "(Landroid/content/Context;)Lcom/google/android/gms/safetynet/SafetyNetClient;",
        [ctx(&mut e)]
    )
    .unwrap());
    let key = s(&mut e, "api-key");
    let t3 = ov(ci!(
        e,
        SAFETYNET_CLIENT,
        "attest",
        "([BLjava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        sn,
        [Value::Null, key]
    )
    .unwrap());
    let att = ov(task_result(&mut e, t3));
    let jws = svm!(
        e,
        ci!(
            e,
            ATTESTATION_RESPONSE,
            "getJwsResult",
            "()Ljava/lang/String;",
            att,
            []
        )
        .unwrap()
    );
    assert!(jws.contains("not-a-real-attestation"), "{jws}");
}

// ── Google Sign-In ──────────────────────────────────────────────────────
#[test]
fn google_signin_silent_requires_host_completion() {
    let mut e = engine();
    let client = ov(cs!(e, GOOGLE_SIGNIN, "getClient",
        "(Landroid/content/Context;Lcom/google/android/gms/auth/api/signin/GoogleSignInOptions;)Lcom/google/android/gms/auth/api/signin/GoogleSignInClient;",
        [ctx(&mut e), Value::Null]).unwrap());
    let t = ov(ci!(
        e,
        SIGNIN_CLIENT,
        "silentSignIn",
        "()Lcom/google/android/gms/tasks/Task;",
        client,
        []
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 0);
    let exc = ov(ci!(e, TASK, "getException", "()Ljava/lang/Exception;", t, []).unwrap());
    assert_eq!(
        iv(ci!(e, API_EXCEPTION, "getStatusCode", "()I", exc, []).unwrap()),
        4
    );

    assert_eq!(
        cs!(e, GOOGLE_SIGNIN, "getLastSignedInAccount",
        "(Landroid/content/Context;)Lcom/google/android/gms/auth/api/signin/GoogleSignInAccount;",
        [ctx(&mut e)])
        .unwrap(),
        Some(Value::Null)
    );

    e.gms_complete_signin(Some(("user@noreply.github.com", "User")));
    let t2 = ov(ci!(
        e,
        SIGNIN_CLIENT,
        "silentSignIn",
        "()Lcom/google/android/gms/tasks/Task;",
        client,
        []
    )
    .unwrap());
    let account = ov(task_result(&mut e, t2));
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                SIGNIN_ACCOUNT,
                "getEmail",
                "()Ljava/lang/String;",
                account,
                []
            )
            .unwrap()
        ),
        "user@noreply.github.com"
    );
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                SIGNIN_ACCOUNT,
                "getDisplayName",
                "()Ljava/lang/String;",
                account,
                []
            )
            .unwrap()
        ),
        "User"
    );
    let last =
        cs!(e, GOOGLE_SIGNIN, "getLastSignedInAccount",
        "(Landroid/content/Context;)Lcom/google/android/gms/auth/api/signin/GoogleSignInAccount;",
        [ctx(&mut e)])
        .unwrap();
    assert!(matches!(last, Some(Value::Obj(_))));
}

// ── Invariante honesta: fora do âmbito ⇒ None (DEX/host decidem) ───────
#[test]
fn unknown_gms_surface_returns_none() {
    let mut e = engine();
    // ML Kit ficou fora desta ronda (not_implemented no registo): o shim
    // não intercepta — None propaga para DEX/host, nunca sucesso falso.
    assert_eq!(
        cs!(
            e,
            "Lcom/google/mlkit/vision/barcode/BarcodeScanning;",
            "getClient",
            "()Lcom/google/mlkit/vision/barcode/BarcodeScanner;",
            []
        )
        .unwrap(),
        None
    );
    let alien = obj(&mut e, "Lcom/google/android/gms/wallet/PaymentsClient;");
    assert_eq!(
        ci!(e, "Lcom/google/android/gms/wallet/PaymentsClient;", "loadPaymentData",
            "(Lcom/google/android/gms/wallet/PaymentDataRequest;)Lcom/google/android/gms/tasks/Task;",
            alien, [Value::Null]).unwrap(),
        None
    );
}

// ── Tasks: agregação e continuações (cobertura do subconjunto registado) ─
#[test]
fn tasks_when_all_success_failure_and_complete() {
    let mut e = engine();
    let a_val = s(&mut e, "A");
    let ta = ov(cs!(
        e,
        TASKS,
        "forResult",
        "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
        [a_val]
    )
    .unwrap());
    assert_eq!(svm!(e, task_result(&mut e, ta)), "A");

    // whenAllSuccess com uma Task ainda pendente: só conclui quando todas
    // concluírem, e o resultado é a lista dos resultados por ordem.
    let tcs = obj(&mut e, TASK_SOURCE);
    ci!(e, TASK_SOURCE, "<init>", "()V", tcs, []).unwrap();
    let tb = ov(ci!(
        e,
        TASK_SOURCE,
        "getTask",
        "()Lcom/google/android/gms/tasks/Task;",
        tcs,
        []
    )
    .unwrap());
    let list = host_list(&mut e, vec![Value::Obj(ta), Value::Obj(tb)]);
    let all = ov(cs!(
        e,
        TASKS,
        "whenAllSuccess",
        "(Ljava/util/Collection;)Lcom/google/android/gms/tasks/Task;",
        [list]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, all, "isComplete"), 0);
    let b_val = s(&mut e, "B");
    ci!(
        e,
        TASK_SOURCE,
        "setResult",
        "(Ljava/lang/Object;)V",
        tcs,
        [b_val]
    )
    .unwrap();
    assert_eq!(task_flag(&mut e, all, "isSuccessful"), 1);
    let results = ov(task_result(&mut e, all));
    match e.fw.objects.get(&results) {
        Some(HostObj::List { elems }) => {
            assert_eq!(elems.len(), 2);
            match &elems[1] {
                Value::Obj(r) => assert_eq!(e.heap.as_str(*r).unwrap(), "B"),
                other => panic!("elem[1]: {other:?}"),
            }
        }
        other => panic!("resultados whenAllSuccess: {other:?}"),
    }

    // Uma falha contamina whenAll; whenAllComplete conclui mesmo assim.
    let exc = obj(&mut e, "Ljava/lang/RuntimeException;");
    let m = s(&mut e, "falhou");
    e.heap.put_field(exc, "message", m).unwrap();
    let tf = ov(cs!(
        e,
        TASKS,
        "forException",
        "(Ljava/lang/Exception;)Lcom/google/android/gms/tasks/Task;",
        [Value::Obj(exc)]
    )
    .unwrap());
    let list2 = host_list(&mut e, vec![Value::Obj(ta), Value::Obj(tf)]);
    let wall = ov(cs!(
        e,
        TASKS,
        "whenAll",
        "(Ljava/util/Collection;)Lcom/google/android/gms/tasks/Task;",
        [list2]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, wall, "isComplete"), 1);
    assert_eq!(task_flag(&mut e, wall, "isSuccessful"), 0);
    let list3 = host_list(&mut e, vec![Value::Obj(ta), Value::Obj(tf)]);
    let wcomplete = ov(cs!(
        e,
        TASKS,
        "whenAllComplete",
        "(Ljava/util/Collection;)Lcom/google/android/gms/tasks/Task;",
        [list3]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, wcomplete, "isSuccessful"), 1);
}

#[test]
fn tasks_continue_with_executes_dex_continuation() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCont;", "Ljava/lang/Object;");
    let depois = b.intern("depois") as u16;
    b.r#virtual(
        cls,
        "then",
        "Ljava/lang/Object;",
        vec!["Lcom/google/android/gms/tasks/Task;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, {
            let mut u = op21c(0x1A, 0, depois); // const-string v0, "depois"
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );
    let mut e = engine_of(&b);
    let src_val = s(&mut e, "antes");
    let src = ov(cs!(
        e,
        TASKS,
        "forResult",
        "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
        [src_val]
    )
    .unwrap());
    let cont = obj(&mut e, "LCont;");
    let out = ov(ci!(
        e,
        TASK,
        "continueWith",
        "(Lcom/google/android/gms/tasks/Continuation;)Lcom/google/android/gms/tasks/Task;",
        src,
        [Value::Obj(cont)]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, out, "isSuccessful"), 1);
    assert_eq!(svm!(e, task_result(&mut e, out)), "depois");
}

#[test]
fn firebase_auth_state_listener_fires_on_add_and_signin() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LAuthLis;",
        "onAuthStateChanged",
        vec!["Lcom/google/firebase/auth/FirebaseAuth;"],
    );
    let mut e = engine_of(&b);
    let auth = ov(cs!(
        e,
        FIREBASE_AUTH,
        "getInstance",
        "()Lcom/google/firebase/auth/FirebaseAuth;",
        []
    )
    .unwrap());
    let lis = obj(&mut e, "LAuthLis;");
    ci!(
        e,
        FIREBASE_AUTH,
        "addAuthStateListener",
        "(Lcom/google/firebase/auth/FirebaseAuth$AuthStateListener;)V",
        auth,
        [Value::Obj(lis)]
    )
    .unwrap();
    assert_eq!(
        called(&e, "LAuthLis;"),
        1,
        "dispara já ao registar (upstream)"
    );

    let email = s(&mut e, "user@x.pt");
    let pwd = s(&mut e, "secret1");
    let t = ov(ci!(
        e,
        FIREBASE_AUTH,
        "createUserWithEmailAndPassword",
        "(Ljava/lang/String;Ljava/lang/String;)Lcom/google/android/gms/tasks/Task;",
        auth,
        [email, pwd]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, t, "isSuccessful"), 1);
    assert_eq!(
        called(&e, "LAuthLis;"),
        2,
        "dispara na mudança de utilizador"
    );

    ci!(e, FIREBASE_AUTH, "signOut", "()V", auth, []).unwrap();
    assert_eq!(called(&e, "LAuthLis;"), 3, "dispara no signOut");
}

// ── Firestore: queries e snapshot listeners; Billing: caminho moderno ───
#[test]
fn firestore_query_filter_and_snapshot_listener() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LFsLis;",
        "onEvent",
        vec![
            "Ljava/lang/Object;",
            "Lcom/google/firebase/firestore/FirebaseFirestoreException;",
        ],
    );
    let mut e = engine_of(&b);
    // Semente: dois utilizadores em Lisboa, um no Porto.
    for (id, city, age) in [
        ("u1", "Lisboa", 31),
        ("u2", "Porto", 25),
        ("u3", "Lisboa", 40),
    ] {
        let c = e.heap.alloc_string(city.to_string()).unwrap();
        e.gms_firestore_seed(
            &format!("users/{id}"),
            vec![
                ("cidade".into(), Value::Obj(c)),
                ("idade".into(), Value::Int(age)),
            ],
        );
    }
    let fs = ov(cs!(
        e,
        FIRESTORE,
        "getInstance",
        "()Lcom/google/firebase/firestore/FirebaseFirestore;",
        []
    )
    .unwrap());
    let coll_name = s(&mut e, "users");
    let coll = ov(ci!(
        e,
        FIRESTORE,
        "collection",
        "(Ljava/lang/String;)Lcom/google/firebase/firestore/CollectionReference;",
        fs,
        [coll_name]
    )
    .unwrap());
    let field = s(&mut e, "cidade");
    let wanted = s(&mut e, "Lisboa");
    let query = ov(ci!(
        e,
        COLLECTION_REF,
        "whereEqualTo",
        "(Ljava/lang/String;Ljava/lang/Object;)Lcom/google/firebase/firestore/Query;",
        coll,
        [field, wanted]
    )
    .unwrap());
    let t = ov(ci!(
        e,
        FS_QUERY,
        "get",
        "()Lcom/google/android/gms/tasks/Task;",
        query,
        []
    )
    .unwrap());
    let snap = ov(task_result(&mut e, t));
    assert_eq!(iv(ci!(e, QUERY_SNAP, "size", "()I", snap, []).unwrap()), 2);

    // Snapshot listener no documento: dispara ao registar e em cada set.
    let doc_id = s(&mut e, "u1");
    let doc = ov(ci!(
        e,
        COLLECTION_REF,
        "document",
        "(Ljava/lang/String;)Lcom/google/firebase/firestore/DocumentReference;",
        coll,
        [doc_id]
    )
    .unwrap());
    let lis = obj(&mut e, "LFsLis;");
    ci!(e, DOC_REF, "addSnapshotListener",
        "(Lcom/google/firebase/firestore/EventListener;)Lcom/google/firebase/firestore/ListenerRegistration;",
        doc, [Value::Obj(lis)]).unwrap();
    assert_eq!(called(&e, "LFsLis;"), 1);
    let data = {
        let k = s(&mut e, "cidade");
        let v = s(&mut e, "Braga");
        host_map(&mut e, vec![(k, v)])
    };
    let set_t = ov(ci!(
        e,
        DOC_REF,
        "set",
        "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
        doc,
        [data]
    )
    .unwrap());
    assert_eq!(task_flag(&mut e, set_t, "isSuccessful"), 1);
    assert_eq!(called(&e, "LFsLis;"), 2);
}

#[test]
fn billing_product_details_modern_path() {
    let mut b = DexBuilder::new();
    counter_observer_class(
        &mut b,
        "LPurLis2;",
        "onPurchasesUpdated",
        vec![
            "Lcom/android/billingclient/api/BillingResult;",
            "Ljava/util/List;",
        ],
    );
    counter_observer_class(
        &mut b,
        "LStateLis2;",
        "onBillingSetupFinished",
        vec!["Lcom/android/billingclient/api/BillingResult;"],
    );
    counter_observer_class(
        &mut b,
        "LPdLis;",
        "onProductDetailsResponse",
        vec![
            "Lcom/android/billingclient/api/BillingResult;",
            "Ljava/util/List;",
        ],
    );
    let mut e = engine_of(&b);
    e.gms_billing_add_sku("premium", "inapp", "R$ 9,99", 9_990_000, "BRL", "Premium");

    let purchases_lis = obj(&mut e, "LPurLis2;");
    let client = billing_client_with_listeners(&mut e, purchases_lis);
    let state_lis = obj(&mut e, "LStateLis2;");
    ci!(
        e,
        BILLING_CLIENT,
        "startConnection",
        "(Lcom/android/billingclient/api/BillingClientStateListener;)V",
        client,
        [Value::Obj(state_lis)]
    )
    .unwrap();

    // QueryProductDetailsParams.Product.newBuilder().setProductId.setProductType.build()
    let product_builder = ov(cs!(
        e,
        QP_PRODUCT,
        "newBuilder",
        "()Lcom/android/billingclient/api/QueryProductDetailsParams$Product$Builder;",
        []
    )
    .unwrap());
    let pid = s(&mut e, "premium");
    ci!(e, QP_PRODUCT_BUILDER, "setProductId",
        "(Ljava/lang/String;)Lcom/android/billingclient/api/QueryProductDetailsParams$Product$Builder;",
        product_builder, [pid]).unwrap();
    let ptype = s(&mut e, "inapp");
    ci!(e, QP_PRODUCT_BUILDER, "setProductType",
        "(Ljava/lang/String;)Lcom/android/billingclient/api/QueryProductDetailsParams$Product$Builder;",
        product_builder, [ptype]).unwrap();
    let product = ov(ci!(
        e,
        QP_PRODUCT_BUILDER,
        "build",
        "()Lcom/android/billingclient/api/QueryProductDetailsParams$Product;",
        product_builder,
        []
    )
    .unwrap());
    let params_builder = ov(cs!(
        e,
        QUERY_PRODUCTS_PARAMS,
        "newBuilder",
        "()Lcom/android/billingclient/api/QueryProductDetailsParams$Builder;",
        []
    )
    .unwrap());
    let products = host_list(&mut e, vec![Value::Obj(product)]);
    ci!(
        e,
        QUERY_PRODUCTS_BUILDER,
        "setProductList",
        "(Ljava/util/List;)Lcom/android/billingclient/api/QueryProductDetailsParams$Builder;",
        params_builder,
        [products]
    )
    .unwrap();
    let params = ov(ci!(
        e,
        QUERY_PRODUCTS_BUILDER,
        "build",
        "()Lcom/android/billingclient/api/QueryProductDetailsParams;",
        params_builder,
        []
    )
    .unwrap());
    let pd_listener = obj(&mut e, "LPdLis;");
    ci!(e, BILLING_CLIENT, "queryProductDetailsAsync",
        "(Lcom/android/billingclient/api/QueryProductDetailsParams;Lcom/android/billingclient/api/ProductDetailsResponseListener;)V",
        client, [Value::Obj(params), Value::Obj(pd_listener)]).unwrap();
    assert_eq!(called(&e, "LPdLis;"), 1);

    let pd = *e
        .fw
        .gms
        .product_details_objs
        .keys()
        .next()
        .expect("ProductDetails cunhado");
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                PRODUCT_DETAILS,
                "getProductId",
                "()Ljava/lang/String;",
                pd,
                []
            )
            .unwrap()
        ),
        "premium"
    );
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                PRODUCT_DETAILS,
                "getTitle",
                "()Ljava/lang/String;",
                pd,
                []
            )
            .unwrap()
        ),
        "Premium"
    );
    let offer = ov(ci!(
        e,
        PRODUCT_DETAILS,
        "getOneTimePurchaseOfferDetails",
        "()Lcom/android/billingclient/api/ProductDetails$OneTimePurchaseOfferDetails;",
        pd,
        []
    )
    .unwrap());
    assert_eq!(
        svm!(
            e,
            ci!(
                e,
                "Lcom/android/billingclient/api/ProductDetails$OneTimePurchaseOfferDetails;",
                "getFormattedPrice",
                "()Ljava/lang/String;",
                offer,
                []
            )
            .unwrap()
        ),
        "R$ 9,99"
    );
}

// ── Ponta a ponta: bytecode DEX chama o shim pelo dispatch real ─────────
#[test]
fn dex_bytecode_calls_gms_through_full_dispatch() {
    let mut b = DexBuilder::new();
    let probe = b.class("LProbe;", "Ljava/lang/Object;");
    b.static_field(probe, "result", "I", common::SVal::Int(-1));
    b.static_field(probe, "code", "I", common::SVal::Int(-1));
    let f_result = b.field_idx("LProbe;", "I", "result");
    let f_code = b.field_idx("LProbe;", "I", "code");

    let p_void = b.proto_idx("V", vec![]);
    let p_messaging = b.proto_idx("Lcom/google/firebase/messaging/FirebaseMessaging;", vec![]);
    let p_task = b.proto_idx("Lcom/google/android/gms/tasks/Task;", vec![]);
    let p_bool = b.proto_idx("Z", vec![]);
    let p_avail = b.proto_idx(
        "Lcom/google/android/gms/common/GoogleApiAvailability;",
        vec![],
    );
    let p_avail_check = b.proto_idx("I", vec!["Landroid/content/Context;".to_string()]);

    let m_fm = b.method_idx(
        "Lcom/google/firebase/messaging/FirebaseMessaging;",
        p_messaging,
        "getInstance",
    );
    let m_token = b.method_idx(
        "Lcom/google/firebase/messaging/FirebaseMessaging;",
        p_task,
        "getToken",
    );
    let m_ok = b.method_idx(
        "Lcom/google/android/gms/tasks/Task;",
        p_bool,
        "isSuccessful",
    );
    let m_ga = b.method_idx(
        "Lcom/google/android/gms/common/GoogleApiAvailability;",
        p_avail,
        "getInstance",
    );
    let m_check = b.method_idx(
        "Lcom/google/android/gms/common/GoogleApiAvailability;",
        p_avail_check,
        "isGooglePlayServicesAvailable",
    );

    b.direct(
        probe,
        "run",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 2, {
            let mut u = op35c(0x71, 0, m_fm, [0, 0, 0, 0, 0]); // invoke-static getInstance
            u.extend(op11x(0x0C, 0)); // move-result-object v0
            u.extend(op35c(0x6E, 1, m_token, [0, 0, 0, 0, 0])); // invoke-virtual getToken
            u.extend(op11x(0x0C, 1)); // move-result-object v1
            u.extend(op35c(0x6E, 1, m_ok, [1, 0, 0, 0, 0])); // invoke-virtual isSuccessful
            u.extend(op11x(0x0A, 2)); // move-result v2
            u.extend(op21c(0x67, 2, f_result)); // sput v2, result
            u.extend(op35c(0x71, 0, m_ga, [0, 0, 0, 0, 0])); // invoke-static getInstance
            u.extend(op11x(0x0C, 0)); // move-result-object v0
            u.extend(op11n(0x12, 1, 0)); // const/4 v1, 0 (Context null)
            u.extend(op35c(0x6E, 2, m_check, [0, 1, 0, 0, 0])); // invoke-virtual check
            u.extend(op11x(0x0A, 2)); // move-result v2
            u.extend(op21c(0x67, 2, f_code)); // sput v2, code
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    let _ = p_void;

    let mut e = engine_of(&b);
    e.invoke_static("LProbe;", "run", "()V", &[])
        .expect("Probe.run executa");
    let get = |e: &Engine, name: &str| match e.statics.get(&("LProbe;".into(), name.into())) {
        Some(Value::Int(n)) => *n,
        other => panic!("static {name}: {other:?}"),
    };
    assert_eq!(
        get(&e, "result"),
        1,
        "Task do token concluiu com sucesso via DEX"
    );
    assert_eq!(get(&e, "code"), 0, "disponibilidade SUCCESS via DEX");

    // Programado pelo hospedeiro, o mesmo bytecode vê SERVICE_MISSING.
    e.gms_set_availability(1);
    e.invoke_static("LProbe;", "run", "()V", &[])
        .expect("Probe.run outra vez");
    assert_eq!(get(&e, "code"), 1);
}
