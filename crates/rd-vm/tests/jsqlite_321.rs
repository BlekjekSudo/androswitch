//! Regression tests for issue #321: guest SQL must stay inside the in-memory
//! database. `ATTACH` / `VACUUM INTO` must not reach the host filesystem,
//! `writable_schema` must be refused, and a runaway recursive CTE must be
//! interrupted instead of hanging the VM.

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::Engine;

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

fn exec(vm: &mut Engine, db: u32, sql: &str) -> Result<Value, String> {
    let s = jstr(vm, sql);
    vm.invoke_instance(db, "execSQL", "(Ljava/lang/String;)V", &[s])
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn attach_database_is_denied_and_creates_no_host_file() {
    let path = std::env::temp_dir().join("rd_jsqlite_321_attach.db");
    let _ = std::fs::remove_file(&path);
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    let sql = format!("ATTACH DATABASE '{}' AS ext", path.display());
    assert!(exec(&mut vm, db, &sql).is_err(), "ATTACH must be refused");
    assert!(!path.exists(), "ATTACH must not create a host file");
}

#[test]
fn vacuum_into_host_path_is_denied() {
    let path = std::env::temp_dir().join("rd_jsqlite_321_vacuum.db");
    let _ = std::fs::remove_file(&path);
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    exec(&mut vm, db, "CREATE TABLE t(a)").unwrap();
    exec(&mut vm, db, "INSERT INTO t VALUES (1)").unwrap();
    let sql = format!("VACUUM INTO '{}'", path.display());
    assert!(
        exec(&mut vm, db, &sql).is_err(),
        "VACUUM INTO must be refused"
    );
    assert!(!path.exists(), "VACUUM INTO must not write a host file");
}

#[test]
fn writable_schema_pragma_is_denied() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    assert!(exec(&mut vm, db, "PRAGMA writable_schema=ON").is_err());
}

#[test]
fn runaway_recursive_cte_is_interrupted_and_vm_stays_usable() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    let started = std::time::Instant::now();
    let runaway = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c) SELECT x FROM c";
    assert!(
        exec(&mut vm, db, runaway).is_err(),
        "runaway CTE must be interrupted"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "runaway CTE must not hang"
    );
    exec(&mut vm, db, "CREATE TABLE ok(a)").expect("normal SQL still works after interrupt");
}

/// Issue #321 bug 2: a single value above the 16 MiB SQLITE_LIMIT_LENGTH is refused
/// inside SQLite, before the host encodes it into a JSON line. The default limit
/// (1e9 bytes) allowed `zeroblob(1000000000)` to reach the host pipe.
#[test]
fn oversized_blob_result_is_refused_by_length_limit() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    let q = jstr(&mut vm, "SELECT length(zeroblob(20000000))");
    let res = vm.invoke_instance(
        db,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        &[q, Value::Null],
    );
    assert!(res.is_err(), "a 20 MB blob must exceed the value limit");
}

#[test]
fn small_blob_result_still_works() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    let q = jstr(&mut vm, "SELECT length(zeroblob(4096))");
    let res = vm.invoke_instance(
        db,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        &[q, Value::Null],
    );
    assert!(res.is_ok(), "a 4 KiB blob must keep working: {res:?}");
}

/// Issue #321 bug 2: a SELECT whose rows exceed the per-statement result budget
/// is refused with a catchable SQLiteException, and the VM stays usable.
#[test]
fn oversized_result_set_is_refused_by_statement_budget() {
    let mut vm = engine_of(&DexBuilder::new());
    let db = open_memory_db(&mut vm);
    // 20 000 rows x 1000 bytes = 20 MB, above the 16 MiB per-statement budget.
    let big = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 20000) \
               SELECT zeroblob(1000) FROM c";
    let q = jstr(&mut vm, big);
    let res = vm.invoke_instance(
        db,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        &[q, Value::Null],
    );
    assert!(res.is_err(), "a 20 MB result set must exceed the budget");
    // A small result still works afterwards.
    let small = jstr(&mut vm, "SELECT 1");
    let ok = vm.invoke_instance(
        db,
        "rawQuery",
        "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
        &[small, Value::Null],
    );
    assert!(
        ok.is_ok(),
        "the VM must stay usable after the refusal: {ok:?}"
    );
}
