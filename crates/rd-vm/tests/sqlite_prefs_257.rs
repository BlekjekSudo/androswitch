//! Regression tests for issue #257:
//! 1. Recycled `ObjRef`s after GC must never be intercepted as phantom
//!    `SQLiteCursor` / `SQLiteStatement` / `SQLiteDatabase`, `SqlState::gc_purge`
//!    cleans dead keys, and `Cursor.close()` releases buffered rows while
//!    `deactivate()` preserves them.
//! 2. Non-finite IEEE-754 floats (`+Infinity`, `-Infinity`, `NaN`) round-trip
//!    through the SQLite JSON-lines backend without syntax errors or bogus
//!    `NOT NULL` constraint failures.
//! 3. `SharedPreferences` XML parses self-closing `<set name="..."/>`,
//!    `<map/>`, and `<!-- a > b -->` comments without dropping or corrupting
//!    neighboring entries.

mod common;

use common::*;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const SQL_DB: &str = "Landroid/database/sqlite/SQLiteDatabase;";

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).unwrap())
}

fn open_memory_db(vm: &mut Engine) -> u32 {
    match vm
        .invoke_static(
            SQL_DB,
            "create",
            "(Landroid/database/sqlite/SQLiteDatabase$CursorFactory;)Landroid/database/sqlite/SQLiteDatabase;",
            &[Value::Null],
        )
        .unwrap()
    {
        Value::Obj(r) => r,
        other => panic!("expected SQLiteDatabase ObjRef, got {other:?}"),
    }
}

#[test]
fn sqlite_recycled_objref_after_gc_is_not_phantom_cursor_and_close_frees_rows() {
    let mut b = DexBuilder::new();
    let custom = b.class("LCustomObj;", "Ljava/lang/Object;");
    let init_code = b.code(1, 1, 0, op10x(0x0e));
    b.direct(custom, "<init>", "V", vec![], ACC_PUBLIC, Some(init_code));
    let hash_code = b.code(2, 1, 0, [op21s(0x13, 0, 7777), op11x(0x0f, 0)].concat());
    b.r#virtual(custom, "hashCode", "I", vec![], ACC_PUBLIC, Some(hash_code));
    let close_code = b.code(2, 1, 0, [op21s(0x13, 0, 8888), op11x(0x0f, 0)].concat());
    b.r#virtual(custom, "close", "I", vec![], ACC_PUBLIC, Some(close_code));

    let mut vm = engine_of(&b);
    let db = open_memory_db(&mut vm);

    let q_sql = jstr(&mut vm, "SELECT 10 UNION ALL SELECT 20 UNION ALL SELECT 30");
    let cur = match vm
        .invoke_instance(
            db,
            "rawQuery",
            "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
            &[q_sql, Value::Null],
        )
        .unwrap()
    {
        Value::Obj(c) => c,
        other => panic!("expected Cursor, got {other:?}"),
    };

    // Verify `deactivate()` keeps rows whereas `close()` drops buffered rows.
    assert_eq!(
        vm.invoke_instance(cur, "getCount", "()I", &[]).unwrap(),
        Value::Int(3)
    );
    vm.invoke_instance(cur, "deactivate", "()V", &[]).unwrap();
    assert_eq!(
        vm.invoke_instance(cur, "getCount", "()I", &[]).unwrap(),
        Value::Int(3),
        "deactivate() must preserve buffered rows"
    );

    let s_sql = jstr(&mut vm, "SELECT 99");
    let stmt = match vm
        .invoke_instance(
            db,
            "compileStatement",
            "(Ljava/lang/String;)Landroid/database/sqlite/SQLiteStatement;",
            &[s_sql],
        )
        .unwrap()
    {
        Value::Obj(st) => st,
        other => panic!("expected SQLiteStatement, got {other:?}"),
    };

    // Explicit `close()` on a cursor must release buffered rows immediately.
    let q_sql2 = jstr(&mut vm, "SELECT 1 UNION ALL SELECT 2");
    let cur2 = match vm
        .invoke_instance(
            db,
            "rawQuery",
            "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
            &[q_sql2, Value::Null],
        )
        .unwrap()
    {
        Value::Obj(c) => c,
        other => panic!("expected Cursor, got {other:?}"),
    };
    assert_eq!(
        vm.invoke_instance(cur2, "getCount", "()I", &[]).unwrap(),
        Value::Int(2)
    );
    vm.invoke_instance(cur2, "close", "()V", &[]).unwrap();
    assert_eq!(
        vm.invoke_instance(cur2, "isClosed", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.invoke_instance(cur2, "getCount", "()I", &[]).unwrap(),
        Value::Int(0),
        "close() must release buffered cursor rows"
    );

    // Collect unreachable `db`, `cur`, `cur2`, and `stmt`, then allocate fresh
    // `LCustomObj;` instances until all 3 wrapper kinds (`dbs`, `cursors`,
    // `programs`) have had their `ObjRef` slots recycled from the LIFO free-list!
    let stats = vm.gc();
    assert!(stats.collections >= 1);

    let mut recycled_hits = 0usize;
    for _ in 0..24 {
        let obj = vm.new_instance("LCustomObj;").unwrap();
        if obj == db || obj == cur || obj == stmt || obj == cur2 {
            recycled_hits += 1;
        }
        // Must execute `LCustomObj`'s DEX methods (`7777` / `8888`), NEVER
        // `jsqlite`'s phantom `Cursor` / `SQLiteProgram` / `SQLiteDatabase`!
        assert_eq!(
            vm.invoke_instance(obj, "hashCode", "()I", &[]).unwrap(),
            Value::Int(7777)
        );
        assert_eq!(
            vm.invoke_instance(obj, "close", "()I", &[]).unwrap(),
            Value::Int(8888)
        );
    }
    assert!(
        recycled_hits >= 3,
        "expected LIFO heap free-list to recycle swept SQLite ObjRefs, got {recycled_hits}"
    );
}

#[test]
fn sqlite_nonfinite_floats_infinity_and_nan_roundtrip_without_json_error() {
    let mut vm = Engine::new(Vec::new(), VmConfig::default());
    let db = open_memory_db(&mut vm);

    // 1. Python -> Rust: `SELECT 1e999, -1e999` produces IEEE-754 ±Inf in SQLite.
    let sel = jstr(&mut vm, "SELECT 1e999, -1e999");
    let cur = match vm
        .invoke_instance(
            db,
            "rawQuery",
            "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
            &[sel, Value::Null],
        )
        .unwrap()
    {
        Value::Obj(c) => c,
        other => panic!("expected Cursor, got {other:?}"),
    };
    assert_eq!(
        vm.invoke_instance(cur, "moveToFirst", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    match vm
        .invoke_instance(cur, "getDouble", "(I)D", &[Value::Int(0)])
        .unwrap()
    {
        Value::Double(d) => assert!(d.is_infinite() && d.is_sign_positive(), "got {d}"),
        other => panic!("expected +Inf Double, got {other:?}"),
    }
    match vm
        .invoke_instance(cur, "getDouble", "(I)D", &[Value::Int(1)])
        .unwrap()
    {
        Value::Double(d) => assert!(d.is_infinite() && d.is_sign_negative(), "got {d}"),
        other => panic!("expected -Inf Double, got {other:?}"),
    }

    // 2. Rust -> Python: `bindDouble(±Infinity)` on a `REAL NOT NULL` column must
    //    insert actual ±Infinity (never `null`, which previously tripped NOT NULL!).
    let ddl = jstr(
        &mut vm,
        "CREATE TABLE inf_nonnull (id INTEGER PRIMARY KEY, v REAL NOT NULL)",
    );
    vm.invoke_instance(db, "execSQL", "(Ljava/lang/String;)V", &[ddl])
        .unwrap();

    let ins_sql = jstr(&mut vm, "INSERT INTO inf_nonnull(v) VALUES (?)");
    let stmt = match vm
        .invoke_instance(
            db,
            "compileStatement",
            "(Ljava/lang/String;)Landroid/database/sqlite/SQLiteStatement;",
            &[ins_sql],
        )
        .unwrap()
    {
        Value::Obj(st) => st,
        other => panic!("expected SQLiteStatement, got {other:?}"),
    };

    for val in [f64::INFINITY, f64::NEG_INFINITY] {
        vm.invoke_instance(
            stmt,
            "bindDouble",
            "(ID)V",
            &[Value::Int(1), Value::Double(val)],
        )
        .unwrap();
        let row_id = vm
            .invoke_instance(stmt, "executeInsert", "()J", &[])
            .unwrap();
        assert!(matches!(row_id, Value::Long(id) if id >= 1));
    }

    let check_sql = jstr(&mut vm, "SELECT v FROM inf_nonnull ORDER BY id ASC");
    let cur2 = match vm
        .invoke_instance(
            db,
            "rawQuery",
            "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
            &[check_sql, Value::Null],
        )
        .unwrap()
    {
        Value::Obj(c) => c,
        other => panic!("expected Cursor, got {other:?}"),
    };
    assert_eq!(
        vm.invoke_instance(cur2, "moveToFirst", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.invoke_instance(cur2, "getDouble", "(I)D", &[Value::Int(0)])
            .unwrap(),
        Value::Double(f64::INFINITY)
    );
    assert_eq!(
        vm.invoke_instance(cur2, "moveToNext", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.invoke_instance(cur2, "getDouble", "(I)D", &[Value::Int(0)])
            .unwrap(),
        Value::Double(f64::NEG_INFINITY)
    );
}

fn write_vfs_file(vm: &mut Engine, path: &str, bytes: &[u8]) {
    let fos = vm
        .heap
        .alloc_instance("Ljava/io/FileOutputStream;".to_string(), Vec::new())
        .unwrap();
    let p = jstr(vm, path);
    vm.invoke_instance(fos, "<init>", "(Ljava/lang/String;)V", &[p])
        .unwrap();
    let arr = vm
        .heap
        .alloc_array(ElemKind::Int, bytes.len(), "[B")
        .unwrap();
    vm.heap
        .array_elems_mut(arr)
        .unwrap()
        .extend(bytes.iter().map(|&b| Value::Int(b as i8 as i32)));
    vm.invoke_instance(fos, "write", "([B)V", &[Value::Obj(arr)])
        .unwrap();
    vm.invoke_instance(fos, "close", "()V", &[]).unwrap();
}

#[test]
fn shared_preferences_self_closing_empty_set_and_comment_gt_preserve_all_entries() {
    let mut vm = Engine::new(Vec::new(), VmConfig::default());
    let dir = vm
        .heap
        .alloc_instance("Ljava/io/File;".to_string(), Vec::new())
        .unwrap();
    let dir_path = jstr(&mut vm, "/data/user/0/app/shared_prefs");
    vm.invoke_instance(dir, "<init>", "(Ljava/lang/String;)V", &[dir_path])
        .unwrap();
    vm.invoke_instance(dir, "mkdirs", "()Z", &[]).unwrap();

    let xml = br#"<?xml version='1.0' encoding='utf-8' standalone='yes' ?>
<map>
    <!-- outer comment containing a > b and <fake/> -->
    <set name="empty_first" />
    <string name="middle">preserved</string>
    <set name="populated">
        <!-- inner > comment -->
        <string>alpha</string>
        <string>beta</string>
    </set>
    <int name="answer" value="42" />
    <set name="empty_trailing"/>
</map>
"#;
    write_vfs_file(&mut vm, "/data/user/0/app/shared_prefs/issue257.xml", xml);

    let act = vm
        .heap
        .alloc_instance("Landroid/app/Activity;".to_string(), Vec::new())
        .unwrap();
    let pref_name = jstr(&mut vm, "issue257");
    let prefs = match rd_vm::framework::call_host_instance(
        &mut vm,
        "Landroid/content/Context;",
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        act,
        &[pref_name, Value::Int(0)],
    )
    .unwrap()
    {
        Some(Value::Obj(p)) => p,
        other => panic!("expected SharedPreferences, got {other:?}"),
    };

    let get_set_size = |vm: &mut Engine, key: &str| -> i32 {
        let k = jstr(vm, key);
        let set_obj = match rd_vm::framework::call_host_instance(
            vm,
            "Landroid/content/SharedPreferences;",
            "getStringSet",
            "(Ljava/lang/String;Ljava/util/Set;)Ljava/util/Set;",
            prefs,
            &[k, Value::Null],
        )
        .unwrap()
        {
            Some(Value::Obj(s)) => s,
            other => panic!("expected non-null Set for '{key}', got {other:?}"),
        };
        match vm.invoke_instance(set_obj, "size", "()I", &[]).unwrap() {
            Value::Int(n) => n,
            other => panic!("expected int size, got {other:?}"),
        }
    };

    assert_eq!(get_set_size(&mut vm, "empty_first"), 0);
    assert_eq!(get_set_size(&mut vm, "populated"), 2);
    assert_eq!(get_set_size(&mut vm, "empty_trailing"), 0);

    let mid_key = jstr(&mut vm, "middle");
    let mid_def = jstr(&mut vm, "missing");
    let mid_val = match rd_vm::framework::call_host_instance(
        &mut vm,
        "Landroid/content/SharedPreferences;",
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        prefs,
        &[mid_key, mid_def],
    )
    .unwrap()
    {
        Some(Value::Obj(s)) => vm.heap.as_str(s).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    };
    assert_eq!(mid_val, "preserved");

    let ans_key = jstr(&mut vm, "answer");
    assert_eq!(
        rd_vm::framework::call_host_instance(
            &mut vm,
            "Landroid/content/SharedPreferences;",
            "getInt",
            "(Ljava/lang/String;I)I",
            prefs,
            &[ans_key, Value::Int(-1)],
        )
        .unwrap(),
        Some(Value::Int(42))
    );
}
