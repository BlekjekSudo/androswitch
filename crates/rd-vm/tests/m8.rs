//! M8 full-scope integration: RecyclerView (DEX adapter), Dialogs, OkHttp,
//! SQLite/Room, Services/Broadcasts/DeepLinks, WorkManager, prefs stateful.
//! Host dispatch is exercised directly + through real DEX callbacks.

#[macro_use]
mod common;

use common::*;
use rd_vm::m8::*;
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

// ── SQLite: DDL + insert + query + cursor + update + delete ─────────────
#[test]
fn m8_sqlite_crud_cursor() {
    let mut e = engine();
    e.use_sqlite_subset();
    let helper = obj(&mut e, SQLITE_HELPER);
    let name = s(&mut e, "app.db");
    let ctx = Value::Null;
    call_m8_instance(&mut e, SQLITE_HELPER, "<init>", "(Landroid/content/Context;Ljava/lang/String;Landroid/database/sqlite/SQLiteDatabase$CursorFactory;I)V", helper, &[ctx, name, Value::Null, Value::Int(1)]).unwrap();
    let db = match call_m8_instance(
        &mut e,
        SQLITE_HELPER,
        "getWritableDatabase",
        "()Landroid/database/sqlite/SQLiteDatabase;",
        helper,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(d)) => d,
        other => panic!("db: {other:?}"),
    };
    let sql = s(
        &mut e,
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT)",
    );
    call_m8_instance(
        &mut e,
        SQLITE_DB,
        "execSQL",
        "(Ljava/lang/String;)V",
        db,
        &[sql],
    )
    .unwrap();
    // insert duas linhas via ContentValues
    for (id, title) in [(1, "um"), (2, "dois")] {
        let cv = obj(&mut e, CONTENT_VALUES);
        let k1 = s(&mut e, "id");
        let k2 = s(&mut e, "title");
        let t = s(&mut e, title);
        call_m8_instance(
            &mut e,
            CONTENT_VALUES,
            "put",
            "(Ljava/lang/String;Ljava/lang/Integer;)V",
            cv,
            &[k1, Value::Int(id)],
        )
        .unwrap();
        call_m8_instance(
            &mut e,
            CONTENT_VALUES,
            "put",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            cv,
            &[k2, t],
        )
        .unwrap();
        let tbl = s(&mut e, "notes");
        let r = call_m8_instance(
            &mut e,
            SQLITE_DB,
            "insert",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/content/ContentValues;)J",
            db,
            &[tbl, Value::Null, Value::Obj(cv)],
        )
        .unwrap();
        assert!(matches!(r, Some(Value::Long(_))), "{r:?}");
    }
    assert_eq!(
        e.m8_database("app.db").unwrap().tables["notes"].rows.len(),
        2
    );
    let q = s(&mut e, "SELECT id, title FROM notes");
    let cur = match call_m8_instance(
        &mut e,
        SQLITE_DB,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        db,
        &[q, Value::Null],
    )
    .unwrap()
    {
        Some(Value::Obj(c)) => c,
        other => panic!("cursor: {other:?}"),
    };
    assert_eq!(
        call_m8_instance(&mut e, CURSOR, "getCount", "()I", cur, &[]).unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call_m8_instance(&mut e, CURSOR, "moveToNext", "()Z", cur, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_m8_instance(&mut e, CURSOR, "getInt", "(I)I", cur, &[Value::Int(0)]).unwrap(),
        Some(Value::Int(1))
    );
    match call_m8_instance(
        &mut e,
        CURSOR,
        "getString",
        "(I)Ljava/lang/String;",
        cur,
        &[Value::Int(1)],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "um"),
        other => panic!("getString: {other:?}"),
    }
    // delete + contagem final no estado
    let tbl = s(&mut e, "notes");
    let wh = s(&mut e, "id=1");
    call_m8_instance(
        &mut e,
        SQLITE_DB,
        "delete",
        "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)I",
        db,
        &[tbl, wh, Value::Null],
    )
    .unwrap();
    assert_eq!(
        e.m8_database("app.db").unwrap().tables["notes"].rows.len(),
        1
    );
}

fn sqlite_open_helper(e: &mut Engine, name: &str) -> u32 {
    let helper = obj(e, SQLITE_HELPER);
    let dbname = s(e, name);
    call_m8_instance(
        e,
        SQLITE_HELPER,
        "<init>",
        "(Landroid/content/Context;Ljava/lang/String;Landroid/database/sqlite/SQLiteDatabase$CursorFactory;I)V",
        helper,
        &[Value::Null, dbname, Value::Null, Value::Int(1)],
    )
    .unwrap();
    match call_m8_instance(
        e,
        SQLITE_HELPER,
        "getWritableDatabase",
        "()Landroid/database/sqlite/SQLiteDatabase;",
        helper,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(db)) => db,
        other => panic!("db: {other:?}"),
    }
}

fn sqlite_exec(e: &mut Engine, db: u32, sql: &str) {
    let sql = s(e, sql);
    call_m8_instance(e, SQLITE_DB, "execSQL", "(Ljava/lang/String;)V", db, &[sql]).unwrap();
}

fn sqlite_first_string(e: &mut Engine, db: u32, sql: &str) -> String {
    let q = s(e, sql);
    let cur = match call_m8_instance(
        e,
        SQLITE_DB,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        db,
        &[q, Value::Null],
    )
    .unwrap()
    {
        Some(Value::Obj(c)) => c,
        other => panic!("cursor: {other:?}"),
    };
    assert_eq!(
        call_m8_instance(e, CURSOR, "moveToNext", "()Z", cur, &[]).unwrap(),
        Some(Value::Int(1))
    );
    match call_m8_instance(
        e,
        CURSOR,
        "getString",
        "(I)Ljava/lang/String;",
        cur,
        &[Value::Int(0)],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => e.heap.as_str(r).unwrap().to_string(),
        other => panic!("getString: {other:?}"),
    }
}

/// Literal Appendix C Round 1: a named SQLite database is serialized to the
/// app-private VFS image and reopened by a fresh Engine (the cross-launch
/// boundary), instead of living only in the first Engine's backend process.
#[test]
fn sqlite_named_database_persists_across_engine_launches() {
    let dbname = format!(
        "persist-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    {
        let mut e = engine();
        let db = sqlite_open_helper(&mut e, &dbname);
        sqlite_exec(
            &mut e,
            db,
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT)",
        );
        sqlite_exec(
            &mut e,
            db,
            "INSERT INTO notes(id, title) VALUES (7, 'persisted')",
        );
        call_m8_instance(&mut e, SQLITE_DB, "close", "()V", db, &[]).unwrap();
    }
    {
        let mut e = engine();
        let db = sqlite_open_helper(&mut e, &dbname);
        assert_eq!(
            sqlite_first_string(&mut e, db, "SELECT title FROM notes WHERE id = 7"),
            "persisted"
        );
        sqlite_exec(
            &mut e,
            db,
            "INSERT INTO notes(id, title) VALUES (8, 'again')",
        );
    }
    {
        let mut e = engine();
        let db = sqlite_open_helper(&mut e, &dbname);
        assert_eq!(
            sqlite_first_string(&mut e, db, "SELECT CAST(count(*) AS TEXT) FROM notes"),
            "2"
        );
        // Context.deleteDatabase must close/forget the open backend handle as
        // well as remove the VFS image; otherwise the next mutation would
        // resurrect the deleted database.
        if e.fw.package_name.is_empty() {
            e.fw.package_name = "app".into();
        }
        let ctx = obj(&mut e, "Landroid/content/Context;");
        let name = s(&mut e, &dbname);
        assert_eq!(
            rd_vm::intrinsics::call_instance_intrinsic(
                &mut e,
                "Landroid/content/Context;",
                "deleteDatabase",
                "(Ljava/lang/String;)Z",
                ctx,
                &[name],
            )
            .unwrap(),
            Some(Value::Int(1))
        );
    }
    {
        let mut e = engine();
        let db = sqlite_open_helper(&mut e, &dbname);
        // Succeeds only if the deleted image was not resurrected.
        sqlite_exec(
            &mut e,
            db,
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT)",
        );
        sqlite_exec(
            &mut e,
            db,
            "INSERT INTO notes(id, title) VALUES (1, 'fresh')",
        );
        assert_eq!(
            sqlite_first_string(&mut e, db, "SELECT title FROM notes WHERE id = 1"),
            "fresh"
        );
    }
}

// ── OkHttp: canned response determinística + request registrada ─────────
#[test]
fn m8_okhttp_execute_and_enqueue() {
    let mut e = engine();
    e.m8_set_http_response(201, "{\"ok\":true}");
    let builder = obj(&mut e, OK_REQ_BUILDER);
    let url = s(&mut e, "https://example.test/api");
    call_m8_instance(
        &mut e,
        OK_REQ_BUILDER,
        "url",
        "(Ljava/lang/String;)Lokhttp3/Request$Builder;",
        builder,
        &[url],
    )
    .unwrap();
    let req = match call_m8_instance(
        &mut e,
        OK_REQ_BUILDER,
        "build",
        "()Lokhttp3/Request;",
        builder,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("{other:?}"),
    };
    let client = obj(&mut e, OK_CLIENT);
    let call = match call_m8_instance(
        &mut e,
        OK_CLIENT,
        "newCall",
        "(Lokhttp3/Request;)Lokhttp3/Call;",
        client,
        &[Value::Obj(req)],
    )
    .unwrap()
    {
        Some(Value::Obj(c)) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!(e.m8_http_requests(), ["https://example.test/api"]);
    let resp = match call_m8_instance(
        &mut e,
        OK_CALL,
        "execute",
        "()Lokhttp3/Response;",
        call,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        call_m8_instance(&mut e, OK_RESPONSE, "code", "()I", resp, &[]).unwrap(),
        Some(Value::Int(201))
    );
    assert_eq!(
        call_m8_instance(&mut e, OK_RESPONSE, "isSuccessful", "()Z", resp, &[]).unwrap(),
        Some(Value::Int(1))
    );
    let body = match call_m8_instance(
        &mut e,
        OK_RESPONSE,
        "body",
        "()Lokhttp3/ResponseBody;",
        resp,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(b)) => b,
        other => panic!("{other:?}"),
    };
    match call_m8_instance(&mut e, OK_BODY, "string", "()Ljava/lang/String;", body, &[]).unwrap() {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "{\"ok\":true}"),
        other => panic!("{other:?}"),
    }
}

// ── Dialogs: builder -> show -> botão ────────────────────────────────────
#[test]
fn m8_alert_dialog_builder_show() {
    let mut e = engine();
    let b = obj(&mut e, ALERT_BUILDER);
    let title = s(&mut e, "Apagar?");
    call_m8_instance(
        &mut e,
        ALERT_BUILDER,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;",
        b,
        &[title],
    )
    .unwrap();
    let msg = s(&mut e, "Confirma?");
    call_m8_instance(
        &mut e,
        ALERT_BUILDER,
        "setMessage",
        "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;",
        b,
        &[msg],
    )
    .unwrap();
    let ok = s(&mut e, "Sim");
    call_m8_instance(&mut e, ALERT_BUILDER, "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", b, &[ok, Value::Null]).unwrap();
    let d = match call_m8_instance(
        &mut e,
        ALERT_BUILDER,
        "show",
        "()Landroid/app/AlertDialog;",
        b,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(d)) => d,
        other => panic!("{other:?}"),
    };
    assert_eq!(e.m8_last_dialog(), Some(d));
    let st = e.m8_dialog(d).unwrap();
    assert!(st.showing);
    assert_eq!(st.title.as_deref(), Some("Apagar?"));
    assert_eq!(st.message.as_deref(), Some("Confirma?"));
    assert!(st.buttons.contains_key(&-1));
    assert_eq!(
        call_m8_instance(&mut e, ALERT_DIALOG, "isShowing", "()Z", d, &[]).unwrap(),
        Some(Value::Int(1))
    );
    call_m8_instance(&mut e, ALERT_DIALOG, "dismiss", "()V", d, &[]).unwrap();
    assert_eq!(
        call_m8_instance(&mut e, ALERT_DIALOG, "isShowing", "()Z", d, &[]).unwrap(),
        Some(Value::Int(0))
    );
}

// ── Services / broadcasts / deep links / Uri ─────────────────────────────
#[test]
fn m8_services_broadcasts_deep_links() {
    let mut e = engine();
    e.m8_register_deep_link("https", "LWebActivity;");
    let act = obj(&mut e, "Landroid/app/Activity;");
    // Intent ACTION_VIEW + Uri data
    let url_arg = s(&mut e, "https://example.test/item/42");
    let uri = match call_m8_static(
        &mut e,
        URI,
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[url_arg],
    )
    .unwrap()
    {
        Some(Value::Obj(u)) => u,
        other => panic!("{other:?}"),
    };
    match call_m8_instance(&mut e, URI, "getHost", "()Ljava/lang/String;", uri, &[]).unwrap() {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "example.test"),
        other => panic!("host: {other:?}"),
    }
    let intent = obj(&mut e, "Landroid/content/Intent;");
    e.fw.objects.insert(
        intent,
        rd_vm::framework::HostObj::Intent {
            component: None,
            extras: Vec::new(),
        },
    );
    let action = s(&mut e, "android.intent.action.VIEW");
    call_m8_instance(
        &mut e,
        "Landroid/content/Intent;",
        "setAction",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        intent,
        &[action],
    )
    .unwrap();
    call_m8_instance(
        &mut e,
        "Landroid/content/Intent;",
        "setData",
        "(Landroid/net/Uri;)Landroid/content/Intent;",
        intent,
        &[Value::Obj(uri)],
    )
    .unwrap();
    // startActivity implícito (deep link)
    call_m8_instance(
        &mut e,
        "Landroid/app/Activity;",
        "startActivity",
        "(Landroid/content/Intent;)V",
        act,
        &[Value::Obj(intent)],
    )
    .unwrap();
    assert_eq!(e.m8_deep_links(), ["https://example.test/item/42"]);
    // broadcast
    let bintent = obj(&mut e, "Landroid/content/Intent;");
    e.fw.objects.insert(
        bintent,
        rd_vm::framework::HostObj::Intent {
            component: None,
            extras: Vec::new(),
        },
    );
    let act2 = s(&mut e, "com.example.SYNC");
    call_m8_instance(
        &mut e,
        "Landroid/content/Intent;",
        "setAction",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        bintent,
        &[act2],
    )
    .unwrap();
    call_m8_instance(
        &mut e,
        "Landroid/content/Context;",
        "sendBroadcast",
        "(Landroid/content/Intent;)V",
        act,
        &[Value::Obj(bintent)],
    )
    .unwrap();
    assert_eq!(e.m8_broadcasts(), ["com.example.SYNC"]);
    // service start/stop
    call_m8_instance(
        &mut e,
        "Landroid/content/Context;",
        "startService",
        "(Landroid/content/Intent;)Landroid/content/ComponentName;",
        act,
        &[Value::Obj(bintent)],
    )
    .unwrap();
    assert_eq!(e.m8_started_services().len(), 1);
}

// ── WorkManager ─────────────────────────────────────────────────────────
#[test]
fn m8_workmanager_enqueue_cancel() {
    let mut e = engine();
    let wm = match call_m8_static(
        &mut e,
        WORK_MANAGER,
        "getInstance",
        "(Landroid/content/Context;)Landroidx/work/WorkManager;",
        &[Value::Null],
    )
    .unwrap()
    {
        Some(Value::Obj(w)) => w,
        other => panic!("{other:?}"),
    };
    let req = obj(&mut e, "Landroidx/work/WorkRequest;");
    let ws = e
        .heap
        .alloc_string("Lcom/example/SyncWorker;".into())
        .unwrap();
    e.heap.put_field(req, "m8_worker", Value::Obj(ws)).unwrap();
    call_m8_instance(
        &mut e,
        WORK_MANAGER,
        "enqueue",
        "(Landroidx/work/WorkRequest;)Landroidx/work/Operation;",
        wm,
        &[Value::Obj(req)],
    )
    .unwrap();
    assert_eq!(e.m8_work().len(), 1);
    assert_eq!(e.m8_work()[0].worker_class, "Lcom/example/SyncWorker;");
    assert_eq!(e.m8_work()[0].state, "ENQUEUED");
    let id = e.m8_work()[0].id.clone();
    e.m8_complete_work(&id);
    assert_eq!(e.m8_work()[0].state, "SUCCEEDED");
}

// ── Fragments androidx ──────────────────────────────────────────────────
#[test]
fn m8_androidx_fragments() {
    let mut e = engine();
    let act = obj(&mut e, "Landroidx/fragment/app/FragmentActivity;");
    let fm = match call_m8_instance(
        &mut e,
        "Landroidx/fragment/app/FragmentActivity;",
        "getSupportFragmentManager",
        "()Landroidx/fragment/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(f)) => f,
        other => panic!("{other:?}"),
    };
    let tx = match call_m8_instance(
        &mut e,
        "Landroidx/fragment/app/FragmentManager;",
        "beginTransaction",
        "()Landroidx/fragment/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(t)) => t,
        other => panic!("{other:?}"),
    };
    let frag = obj(&mut e, "Landroidx/fragment/app/Fragment;");
    let tag = s(&mut e, "detail");
    call_m8_instance(&mut e, "Landroidx/fragment/app/FragmentTransaction;", "add", "(Landroidx/fragment/app/Fragment;Ljava/lang/String;)Landroidx/fragment/app/FragmentTransaction;", tx, &[Value::Obj(frag), tag]).unwrap();
    call_m8_instance(
        &mut e,
        "Landroidx/fragment/app/FragmentTransaction;",
        "commit",
        "()I",
        tx,
        &[],
    )
    .unwrap();
    let tag2 = s(&mut e, "detail");
    match call_m8_instance(
        &mut e,
        "Landroidx/fragment/app/FragmentManager;",
        "findFragmentByTag",
        "(Ljava/lang/String;)Landroidx/fragment/app/Fragment;",
        fm,
        &[tag2],
    )
    .unwrap()
    {
        Some(Value::Obj(f)) => assert_eq!(f, frag),
        other => panic!("find: {other:?}"),
    }
}

// ── RecyclerView com adapter DEX real ───────────────────────────────────
#[test]
fn m8_recyclerview_populates_from_dex_adapter() {
    let mut b = DexBuilder::new();
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    let p_void = b.proto_idx("V", vec![]);
    let adapter_cls = b.class(
        "LAdapter;",
        "Landroidx/recyclerview/widget/RecyclerView$Adapter;",
    );
    let holder_cls = b.class(
        "LHolder;",
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
    );
    // Adapter.<init>: super()
    let adapter_super_init = b.method_idx(
        "Landroidx/recyclerview/widget/RecyclerView$Adapter;",
        p_void,
        "<init>",
    );
    b.direct(
        adapter_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | 0x1_0000,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, adapter_super_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // getItemCount()I = 3 (virtual)
    let p_int = b.proto_idx("I", vec![]);
    b.r#virtual(
        adapter_cls,
        "getItemCount",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op11n(0x12, 0, 3);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // onCreateViewHolder(ViewGroup,I)ViewHolder: new TextView -> new Holder(tv)
    let p_holder = b.proto_idx(
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
        vec!["Landroid/view/ViewGroup;".into(), "I".into()],
    );
    let tv_type = b.type_idx("Landroid/widget/TextView;");
    let holder_type = b.type_idx("LHolder;");
    let tv_init = b.method_idx("Landroid/widget/TextView;", p_void, "<init>");
    let p_view_void = b.proto_idx("V", vec!["Landroid/view/View;".into()]);
    let holder_init = b.method_idx("LHolder;", p_view_void, "<init>");
    b.r#virtual(
        adapter_cls,
        "onCreateViewHolder",
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
        vec!["Landroid/view/ViewGroup;", "I"],
        ACC_PUBLIC,
        Some(b.code(3, 3, 2, {
            // ins: v0=this, v1=parent, v2=viewType (regs=3, ins=3 -> v0..v2 all ins? regs must >= ins)
            let mut u = op21c(0x22, 0, tv_type);
            u.extend(op35c(0x70, 1, tv_init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x22, 1, holder_type));
            u.extend(op35c(0x70, 2, holder_init, [1, 0, 0, 0, 0]));
            u.extend(op11x(0x11, 1));
            u
        })),
    );
    let _ = p_holder;
    let _ = p_int;
    // onBindViewHolder(Holder,I)V: no-op return
    b.r#virtual(
        adapter_cls,
        "onBindViewHolder",
        "V",
        vec![
            "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
            "I",
        ],
        ACC_PUBLIC,
        Some(b.code(3, 3, 0, op10x(0x0E))),
    );
    // Holder.<init>(View): super(view)
    let holder_super_init = b.method_idx(
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
        p_view_void,
        "<init>",
    );
    b.direct(
        holder_cls,
        "<init>",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC | 0x1_0000,
        Some(b.code(2, 2, 2, {
            let mut u = op35c(0x70, 2, holder_super_init, [0, 1, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut e = engine_of(&b);
    let recycler = e
        .heap
        .alloc_instance(RECYCLER.to_string(), Vec::new())
        .unwrap();
    e.attach_host_state(RECYCLER, recycler).unwrap();
    let adapter = e.new_dex_instance_pub("LAdapter;").unwrap();
    call_m8_instance(
        &mut e,
        RECYCLER,
        "setAdapter",
        "(Landroidx/recyclerview/widget/RecyclerView$Adapter;)V",
        recycler,
        &[Value::Obj(adapter)],
    )
    .unwrap();
    let st = e.m8_recycler(recycler).expect("recycler state");
    assert_eq!(st.item_count, 3, "getItemCount DEX");
    assert_eq!(
        st.bound_positions,
        [0, 1, 2],
        "onBindViewHolder por posição"
    );
    match e.fw.get(recycler) {
        Some(rd_vm::framework::HostObj::View { children, .. }) => {
            assert_eq!(children.len(), 3, "itemViews anexados")
        }
        other => panic!("recycler não é View host: {other:?}"),
    }
    // notifyDataSetChanged repopula e conta
    call_m8_instance(
        &mut e,
        RECYCLER_ADAPTER,
        "notifyDataSetChanged",
        "()V",
        adapter,
        &[],
    )
    .unwrap();
    assert_eq!(e.m8_recycler(recycler).unwrap().notifications, 1);
}

// ── SharedPreferences stateful: edit -> commit -> read ──────────────────
#[test]
fn m8_shared_preferences_stateful() {
    let mut e = engine();
    let act = obj(&mut e, "Landroid/app/Activity;");
    let name = s(&mut e, "prefs");
    let prefs = match rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/Context;",
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        act,
        &[name, Value::Int(0)],
    )
    .unwrap()
    {
        Some(Value::Obj(p)) => p,
        other => panic!("{other:?}"),
    };
    let editor = match rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "edit",
        "()Landroid/content/SharedPreferences$Editor;",
        prefs,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(ed)) => ed,
        other => panic!("edit: {other:?}"),
    };
    let k = s(&mut e, "user");
    let v = s(&mut e, "ana");
    rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences$Editor;",
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        editor,
        &[k, v],
    )
    .unwrap();
    rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences$Editor;",
        "commit",
        "()Z",
        editor,
        &[],
    )
    .unwrap();
    let k2 = s(&mut e, "user");
    let def = s(&mut e, "none");
    match rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        prefs,
        &[k2, def],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "ana"),
        other => panic!("getString: {other:?}"),
    }
    let k3 = s(&mut e, "user");
    assert_eq!(
        rd_vm::framework::call_host_instance(
            &mut e,
            "Landroid/content/SharedPreferences;",
            "contains",
            "(Ljava/lang/String;)Z",
            prefs,
            &[k3]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
}

#[test]
fn m8_shared_preferences_change_listener_notifies_only_changed_keys() {
    let mut b = DexBuilder::new();
    let listener_class = b.class("LPrefsListener;", "Ljava/lang/Object;");
    b.instance_field(listener_class, "changeCount", "I");
    b.instance_field(listener_class, "lastKey", "Ljava/lang/String;");
    let count_field = b.field_idx("LPrefsListener;", "I", "changeCount");
    let key_field = b.field_idx("LPrefsListener;", "Ljava/lang/String;", "lastKey");
    b.r#virtual(
        listener_class,
        "onSharedPreferenceChanged",
        "V",
        vec!["Landroid/content/SharedPreferences;", "Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(5, 3, 0, {
            // regs v0-v1 are temporaries; incoming p0=this, p1=prefs, p2=key
            let mut code = op22c(0x52, 0, 2, count_field); // iget v0, v2, count
            code.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            code.extend(op12x(0xb0, 0, 1)); // add-int/2addr v0, v1
            code.extend(op22c(0x59, 0, 2, count_field)); // iput v0, v2, count
            code.extend(op22c(0x5b, 4, 2, key_field)); // iput-object v4, v2, lastKey
            code.extend(op10x(0x0e));
            code
        })),
    );

    let mut e = engine_of(&b);
    let context = obj(&mut e, "Landroid/app/Activity;");
    let pref_name = s(&mut e, "listener-test");
    let prefs = match rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/Context;",
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        context,
        &[pref_name, Value::Int(0)],
    )
    .unwrap()
    {
        Some(Value::Obj(prefs)) => prefs,
        other => panic!("getSharedPreferences: {other:?}"),
    };
    let listener = e.new_dex_instance_pub("LPrefsListener;").unwrap();
    const LISTENER_SIG: &str =
        "(Landroid/content/SharedPreferences$OnSharedPreferenceChangeListener;)V";
    for _ in 0..2 {
        rd_vm::framework::call_host_instance(
            &mut e,
            "Landroid/content/SharedPreferences;",
            "registerOnSharedPreferenceChangeListener",
            LISTENER_SIG,
            prefs,
            &[Value::Obj(listener)],
        )
        .unwrap();
    }

    let commit_string = |e: &mut Engine, value: &str| {
        let editor = match rd_vm::framework::call_host_instance(
            e,
            "Landroid/content/SharedPreferences;",
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            prefs,
            &[],
        )
        .unwrap()
        {
            Some(Value::Obj(editor)) => editor,
            other => panic!("edit: {other:?}"),
        };
        let key = s(e, "mode");
        let value = s(e, value);
        rd_vm::framework::call_host_instance(
            e,
            "Landroid/content/SharedPreferences$Editor;",
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            editor,
            &[key, value],
        )
        .unwrap();
        rd_vm::framework::call_host_instance(
            e,
            "Landroid/content/SharedPreferences$Editor;",
            "commit",
            "()Z",
            editor,
            &[],
        )
        .unwrap();
    };

    commit_string(&mut e, "dark");
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(1)
    );
    commit_string(&mut e, "dark"); // equal String content is not a change
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(1)
    );
    commit_string(&mut e, "light");
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(2)
    );
    let Value::Obj(last_key) = e.heap.get_field(listener, "lastKey").unwrap() else {
        panic!("listener should receive a String key");
    };
    assert_eq!(e.heap.as_str(last_key).unwrap(), "mode");

    rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "unregisterOnSharedPreferenceChangeListener",
        LISTENER_SIG,
        prefs,
        &[Value::Obj(listener)],
    )
    .unwrap();
    commit_string(&mut e, "bright");
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(2)
    );
}

fn named_prefs(e: &mut Engine, name: &str) -> u32 {
    let context = obj(e, "Landroid/app/Activity;");
    let name = s(e, name);
    match rd_vm::framework::call_host_instance(
        e,
        "Landroid/content/Context;",
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        context,
        &[name, Value::Int(0)],
    )
    .unwrap()
    {
        Some(Value::Obj(prefs)) => prefs,
        other => panic!("getSharedPreferences: {other:?}"),
    }
}

fn prefs_editor(e: &mut Engine, prefs: u32) -> u32 {
    match rd_vm::framework::call_host_instance(
        e,
        "Landroid/content/SharedPreferences;",
        "edit",
        "()Landroid/content/SharedPreferences$Editor;",
        prefs,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(editor)) => editor,
        other => panic!("edit: {other:?}"),
    }
}

fn prefs_put(e: &mut Engine, editor: u32, method: &str, sig: &str, key: &str, value: Value) {
    let key = s(e, key);
    rd_vm::framework::call_host_instance(
        e,
        "Landroid/content/SharedPreferences$Editor;",
        method,
        sig,
        editor,
        &[key, value],
    )
    .unwrap();
}

fn prefs_commit(e: &mut Engine, editor: u32) {
    assert_eq!(
        rd_vm::framework::call_host_instance(
            e,
            "Landroid/content/SharedPreferences$Editor;",
            "commit",
            "()Z",
            editor,
            &[],
        )
        .unwrap(),
        Some(Value::Int(1))
    );
}

fn prefs_get(
    e: &mut Engine,
    prefs: u32,
    method: &str,
    sig: &str,
    key: &str,
    default: Value,
) -> Value {
    let key = s(e, key);
    rd_vm::framework::call_host_instance(
        e,
        "Landroid/content/SharedPreferences;",
        method,
        sig,
        prefs,
        &[key, default],
    )
    .unwrap()
    .unwrap()
}

fn prefs_get_string(e: &mut Engine, prefs: u32, key: &str) -> String {
    let default = s(e, "<missing>");
    match prefs_get(
        e,
        prefs,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        key,
        default,
    ) {
        Value::Obj(text) => e.heap.as_str(text).unwrap().to_string(),
        other => panic!("getString({key}): {other:?}"),
    }
}

#[test]
fn shared_preferences_named_stores_persist_across_engine_launches() {
    const MAIN: &str = "literal-round3-main";
    const OTHER: &str = "literal-round3-other";

    {
        let mut e = engine();
        let main = named_prefs(&mut e, MAIN);
        assert_eq!(named_prefs(&mut e, MAIN), main);
        let other = named_prefs(&mut e, OTHER);
        assert_ne!(other, main);

        let editor = prefs_editor(&mut e, main);
        let user = s(&mut e, "Ana & João");
        prefs_put(
            &mut e,
            editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            "user",
            user,
        );
        prefs_put(
            &mut e,
            editor,
            "putInt",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences$Editor;",
            "count",
            Value::Int(7),
        );
        prefs_put(
            &mut e,
            editor,
            "putLong",
            "(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;",
            "stamp",
            Value::Long(9_000_000_001),
        );
        prefs_put(
            &mut e,
            editor,
            "putFloat",
            "(Ljava/lang/String;F)Landroid/content/SharedPreferences$Editor;",
            "ratio",
            Value::Float(2.5),
        );
        prefs_put(
            &mut e,
            editor,
            "putBoolean",
            "(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;",
            "enabled",
            Value::Int(1),
        );
        let tags = obj(&mut e, "Ljava/util/HashSet;");
        let z = s(&mut e, "z");
        let a = s(&mut e, "a&b");
        e.fw.objects
            .insert(tags, rd_vm::framework::HostObj::List { elems: vec![z, a] });
        prefs_put(
            &mut e,
            editor,
            "putStringSet",
            "(Ljava/lang/String;Ljava/util/Set;)Landroid/content/SharedPreferences$Editor;",
            "tags",
            Value::Obj(tags),
        );
        prefs_commit(&mut e, editor);

        let editor = prefs_editor(&mut e, other);
        let user = s(&mut e, "other-user");
        prefs_put(
            &mut e,
            editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            "user",
            user,
        );
        prefs_commit(&mut e, editor);
    }

    {
        let mut e = engine();
        let main = named_prefs(&mut e, MAIN);
        let other = named_prefs(&mut e, OTHER);
        assert_eq!(prefs_get_string(&mut e, main, "user"), "Ana & João");
        assert_eq!(prefs_get_string(&mut e, other, "user"), "other-user");
        assert_eq!(
            prefs_get(
                &mut e,
                main,
                "getInt",
                "(Ljava/lang/String;I)I",
                "count",
                Value::Int(-1),
            ),
            Value::Int(7)
        );
        assert_eq!(
            prefs_get(
                &mut e,
                main,
                "getLong",
                "(Ljava/lang/String;J)J",
                "stamp",
                Value::Long(-1),
            ),
            Value::Long(9_000_000_001)
        );
        assert_eq!(
            prefs_get(
                &mut e,
                main,
                "getFloat",
                "(Ljava/lang/String;F)F",
                "ratio",
                Value::Float(-1.0),
            ),
            Value::Float(2.5)
        );
        assert_eq!(
            prefs_get(
                &mut e,
                main,
                "getBoolean",
                "(Ljava/lang/String;Z)Z",
                "enabled",
                Value::Int(0),
            ),
            Value::Int(1)
        );
        let tags = prefs_get(
            &mut e,
            main,
            "getStringSet",
            "(Ljava/lang/String;Ljava/util/Set;)Ljava/util/Set;",
            "tags",
            Value::Null,
        );
        let Value::Obj(tags) = tags else {
            panic!("getStringSet: {tags:?}");
        };
        let Some(rd_vm::framework::HostObj::List { elems }) = e.fw.objects.get(&tags) else {
            panic!("string set has no host elements");
        };
        let mut strings = elems
            .iter()
            .map(|value| match value {
                Value::Obj(text) => e.heap.as_str(*text).unwrap().to_string(),
                other => panic!("string set element: {other:?}"),
            })
            .collect::<Vec<_>>();
        strings.sort();
        assert_eq!(strings, ["a&b", "z"]);

        // Named stores are isolated, and typed getters reject wrong types.
        assert_eq!(
            prefs_get(
                &mut e,
                other,
                "getInt",
                "(Ljava/lang/String;I)I",
                "count",
                Value::Int(-1),
            ),
            Value::Int(-1)
        );
        let count = s(&mut e, "count");
        let default = s(&mut e, "wrong");
        assert!(rd_vm::framework::call_host_instance(
            &mut e,
            "Landroid/content/SharedPreferences;",
            "getString",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            main,
            &[count, default],
        )
        .is_err());

        let editor = prefs_editor(&mut e, main);
        let user = s(&mut e, "user");
        rd_vm::framework::call_host_instance(
            &mut e,
            "Landroid/content/SharedPreferences$Editor;",
            "remove",
            "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            editor,
            &[user],
        )
        .unwrap();
        prefs_commit(&mut e, editor);

        let editor = prefs_editor(&mut e, other);
        rd_vm::framework::call_host_instance(
            &mut e,
            "Landroid/content/SharedPreferences$Editor;",
            "clear",
            "()Landroid/content/SharedPreferences$Editor;",
            editor,
            &[],
        )
        .unwrap();
        prefs_commit(&mut e, editor);
    }

    {
        let mut e = engine();
        let main = named_prefs(&mut e, MAIN);
        let other = named_prefs(&mut e, OTHER);
        assert_eq!(prefs_get_string(&mut e, main, "user"), "<missing>");
        assert_eq!(prefs_get_string(&mut e, other, "user"), "<missing>");
        assert_eq!(
            prefs_get(
                &mut e,
                main,
                "getInt",
                "(Ljava/lang/String;I)I",
                "count",
                Value::Int(-1),
            ),
            Value::Int(7)
        );
    }
}

// ── Room builder over SQLite host ────────────────────────────────────────
#[test]
fn m8_room_builder_builds_database() {
    let mut e = engine();
    // Class object para a DB (class_name field como o host de Class usa)
    let cls = obj(&mut e, "Ljava/lang/Class;");
    let cn = e
        .heap
        .alloc_string("Landroidx/room/RoomDatabase;".into())
        .unwrap();
    e.heap.put_field(cls, "class_name", Value::Obj(cn)).unwrap();
    let name_arg = s(&mut e, "room.db");
    let builder = match call_m8_static(&mut e, ROOM, "databaseBuilder", "(Landroid/content/Context;Ljava/lang/Class;Ljava/lang/String;)Landroidx/room/RoomDatabase$Builder;", &[Value::Null, Value::Obj(cls), name_arg]).unwrap() {
        Some(Value::Obj(b)) => b, other => panic!("{other:?}"),
    };
    // allowMainThreadQueries é fluente (devolve this) — Room real chamaria antes do build.
    call_m8_instance(
        &mut e,
        ROOM_BUILDER,
        "allowMainThreadQueries",
        "()Landroidx/room/RoomDatabase$Builder;",
        builder,
        &[],
    )
    .unwrap();
    let db = match call_m8_instance(
        &mut e,
        ROOM_BUILDER,
        "build",
        "()Landroidx/room/RoomDatabase;",
        builder,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(d)) => d,
        other => panic!("{other:?}"),
    };
    let _ = db;
    assert!(e.m8_database("room.db").is_some());
}

// ── Callbacks DEX reais: dialog button, OkHttp enqueue, broadcast ───────
fn callback_class(
    b: &mut DexBuilder,
    class: &str,
    method: &str,
    sig_ret: &str,
    params: Vec<&str>,
    marker: i8,
) -> usize {
    let cls = b.class(class, "Ljava/lang/Object;");
    b.static_field(cls, "called", "I", common::SVal::Int(0));
    let f = b.field_idx(class, "I", "called");
    b.r#virtual(
        cls,
        method,
        sig_ret,
        params,
        ACC_PUBLIC,
        Some(b.code(3, 3, 0, {
            let mut u = op11n(0x12, 0, marker);
            u.extend(op21c(0x67, 0, f));
            u.extend(op10x(0x0E));
            u
        })),
    );
    cls
}

#[test]
fn m8_dex_callbacks_fire() {
    let mut b = DexBuilder::new();
    callback_class(
        &mut b,
        "LDlgListener;",
        "onClick",
        "V",
        vec!["Landroid/content/DialogInterface;", "I"],
        7,
    );
    callback_class(
        &mut b,
        "LHttpCb;",
        "onResponse",
        "V",
        vec!["Lokhttp3/Call;", "Lokhttp3/Response;"],
        6,
    );
    callback_class(
        &mut b,
        "LReceiver;",
        "onReceive",
        "V",
        vec!["Landroid/content/Context;", "Landroid/content/Intent;"],
        5,
    );
    let mut e = engine_of(&b);

    // Dialog: listener DEX dispara e o dialog fecha
    let listener = e.new_dex_instance_pub("LDlgListener;").unwrap_or_else(|_| {
        e.heap
            .alloc_instance("LDlgListener;".into(), Vec::new())
            .unwrap()
    });
    let dlg_builder = obj(&mut e, ALERT_BUILDER);
    let label = s(&mut e, "OK");
    call_m8_instance(&mut e, ALERT_BUILDER, "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", dlg_builder, &[label, Value::Obj(listener)]).unwrap();
    let d = match call_m8_instance(
        &mut e,
        ALERT_BUILDER,
        "show",
        "()Landroid/app/AlertDialog;",
        dlg_builder,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(d)) => d,
        other => panic!("{other:?}"),
    };
    assert!(e.m8_click_dialog_button(d, -1).unwrap());
    assert_eq!(
        e.statics
            .get(&("LDlgListener;".to_string(), "called".to_string())),
        Some(&Value::Int(7))
    );
    assert!(!e.m8_dialog(d).unwrap().showing);

    // OkHttp enqueue: Callback DEX dispara síncrono
    let cb = e
        .heap
        .alloc_instance("LHttpCb;".into(), Vec::new())
        .unwrap();
    let client = obj(&mut e, OK_CLIENT);
    let reqb = obj(&mut e, OK_REQ_BUILDER);
    let url = s(&mut e, "https://example.test/cb");
    call_m8_instance(
        &mut e,
        OK_REQ_BUILDER,
        "url",
        "(Ljava/lang/String;)Lokhttp3/Request$Builder;",
        reqb,
        &[url],
    )
    .unwrap();
    let req = match call_m8_instance(
        &mut e,
        OK_REQ_BUILDER,
        "build",
        "()Lokhttp3/Request;",
        reqb,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("{other:?}"),
    };
    let call = match call_m8_instance(
        &mut e,
        OK_CLIENT,
        "newCall",
        "(Lokhttp3/Request;)Lokhttp3/Call;",
        client,
        &[Value::Obj(req)],
    )
    .unwrap()
    {
        Some(Value::Obj(c)) => c,
        other => panic!("{other:?}"),
    };
    call_m8_instance(
        &mut e,
        OK_CALL,
        "enqueue",
        "(Lokhttp3/Callback;)V",
        call,
        &[Value::Obj(cb)],
    )
    .unwrap();
    assert_eq!(
        e.statics
            .get(&("LHttpCb;".to_string(), "called".to_string())),
        Some(&Value::Int(6))
    );

    // Broadcast: receiver registrado recebe onReceive DEX
    let receiver = e
        .heap
        .alloc_instance("LReceiver;".into(), Vec::new())
        .unwrap();
    let act = obj(&mut e, "Landroid/app/Activity;");
    call_m8_instance(&mut e, "Landroid/content/Context;", "registerReceiver", "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;", act, &[Value::Obj(receiver), Value::Null]).unwrap();
    let intent = obj(&mut e, "Landroid/content/Intent;");
    e.fw.objects.insert(
        intent,
        rd_vm::framework::HostObj::Intent {
            component: None,
            extras: Vec::new(),
        },
    );
    let action = s(&mut e, "com.example.PING");
    call_m8_instance(
        &mut e,
        "Landroid/content/Intent;",
        "setAction",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        intent,
        &[action],
    )
    .unwrap();
    call_m8_instance(
        &mut e,
        "Landroid/content/Context;",
        "sendBroadcast",
        "(Landroid/content/Intent;)V",
        act,
        &[Value::Obj(intent)],
    )
    .unwrap();
    assert_eq!(
        e.statics
            .get(&("LReceiver;".to_string(), "called".to_string())),
        Some(&Value::Int(5))
    );
}

#[test]
fn prefs_clear_notifies_null_once_and_null_keys_tolerated() {
    let mut b = DexBuilder::new();
    let listener_class = b.class("LPrefsClearListener;", "Ljava/lang/Object;");
    b.instance_field(listener_class, "changeCount", "I");
    b.instance_field(listener_class, "nullKeyCount", "I");
    let count_field = b.field_idx("LPrefsClearListener;", "I", "changeCount");
    let null_field = b.field_idx("LPrefsClearListener;", "I", "nullKeyCount");
    b.r#virtual(
        listener_class,
        "onSharedPreferenceChanged",
        "V",
        vec!["Landroid/content/SharedPreferences;", "Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(6, 3, 0, {
            // v0/v1 temps; p0=this(v3), p1=prefs(v4), p2=key(v5)
            let mut code = op22c(0x52, 0, 3, count_field); // iget v0, this, count
            code.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            code.extend(op12x(0xb0, 0, 1)); // add-int/2addr v0, v1
            code.extend(op22c(0x59, 0, 3, count_field)); // iput v0, this, count
                                                         // if (key != null) skip: if-eqz -> offset 6 code units ahead
            code.extend(op21c(0x39, 5, 8)); // if-nez v5, +8 (to return-void)
            code.extend(op22c(0x52, 0, 3, null_field)); // iget v0, this, nullCount
            code.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            code.extend(op12x(0xb0, 0, 1)); // add-int/2addr
            code.extend(op22c(0x59, 0, 3, null_field)); // iput
            code.extend(op10x(0x0e)); // return-void
            code
        })),
    );

    let mut e = engine_of(&b);
    let prefs = named_prefs(&mut e, "clear-test");
    let listener = e.new_dex_instance_pub("LPrefsClearListener;").unwrap();
    e.heap
        .put_field(listener, "changeCount", Value::Int(0))
        .unwrap();
    e.heap
        .put_field(listener, "nullKeyCount", Value::Int(0))
        .unwrap();
    rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "registerOnSharedPreferenceChangeListener",
        "(Landroid/content/SharedPreferences$OnSharedPreferenceChangeListener;)V",
        prefs,
        &[Value::Obj(listener)],
    )
    .unwrap();

    let editor = prefs_editor(&mut e, prefs);
    let value = s(&mut e, "v");
    prefs_put(
        &mut e,
        editor,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        "k1",
        value.clone(),
    );
    prefs_commit(&mut e, editor);
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.heap.get_field(listener, "nullKeyCount").unwrap(),
        Value::Int(0)
    );

    // clear() reports exactly one null-key callback (since targetSdk R).
    let editor = prefs_editor(&mut e, prefs);
    rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences$Editor;",
        "clear",
        "()Landroid/content/SharedPreferences$Editor;",
        editor,
        &[],
    )
    .unwrap();
    prefs_commit(&mut e, editor);
    assert_eq!(
        e.heap.get_field(listener, "changeCount").unwrap(),
        Value::Int(2)
    );
    assert_eq!(
        e.heap.get_field(listener, "nullKeyCount").unwrap(),
        Value::Int(1)
    );
    assert_eq!(prefs_get_string(&mut e, prefs, "k1"), "<missing>");

    // Null keys are tolerated: default back, not an NPE.
    let default = s(&mut e, "d");
    let default_cmp = default.clone();
    let got = rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        prefs,
        &[Value::Null, default],
    )
    .unwrap()
    .unwrap();
    assert_eq!(got, default_cmp);
    let contains = rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/SharedPreferences;",
        "contains",
        "(Ljava/lang/String;)Z",
        prefs,
        &[Value::Null],
    )
    .unwrap()
    .unwrap();
    assert_eq!(contains, Value::Int(0));
}
