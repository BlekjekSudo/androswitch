//! Regression test for issue #321 (bug 3): `SQLiteOpenHelper` must not cache a
//! database whose open lifecycle failed. A failing `onCreate` must leave no
//! `rd_sqlx_db` handle behind, so the next `getWritableDatabase()` retries
//! the open instead of returning a half-initialized database.

mod common;

use common::*;
use rd_vm::m8::call_m8_instance;
use rd_vm::value::Value;
use rd_vm::Engine;

const HELPER: &str = "Landroid/database/sqlite/SQLiteOpenHelper;";
const SQL_DB: &str = "Landroid/database/sqlite/SQLiteDatabase;";
const FAIL_HELPER: &str = "LFailHelper;";

/// `class FailHelper extends SQLiteOpenHelper` whose `onCreate(db)` runs
/// `db.execSQL("NOT VALID SQL")`, which throws.
fn fail_helper_dex() -> DexBuilder {
    let mut b = DexBuilder::new();
    let h = b.class(FAIL_HELPER, HELPER);
    let s_bad = b.intern("NOT VALID SQL") as u16;
    let exec_proto = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
    let exec = b.method_idx(SQL_DB, exec_proto, "execSQL");
    // regs=3, ins=2: v1=this, v2=db
    let mut u = Vec::new();
    u.extend(op21c(0x1A, 0, s_bad)); // const-string v0, "NOT VALID SQL"
    u.extend(op35c(0x6E, 2, exec, [2, 0, 0, 0, 0])); // invoke-virtual {v2, v0}, execSQL
    u.extend(op10x(0x0E)); // return-void
    let code = b.code(3, 2, 2, u);
    b.r#virtual(h, "onCreate", "V", vec![SQL_DB], 0x0001, Some(code));
    b
}

fn text(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).unwrap())
}

#[test]
fn failed_open_does_not_cache_the_database_handle() {
    let mut vm = engine_of(&fail_helper_dex());
    let helper = vm
        .heap
        .alloc_instance(FAIL_HELPER.to_string(), Vec::new())
        .unwrap();
    let name = text(&mut vm, "broken.db");
    call_m8_instance(
        &mut vm,
        HELPER,
        "<init>",
        "(Landroid/content/Context;Ljava/lang/String;Landroid/database/sqlite/SQLiteDatabase$CursorFactory;I)V",
        helper,
        &[Value::Null, name, Value::Null, Value::Int(1)],
    )
    .unwrap();

    let first = call_m8_instance(
        &mut vm,
        HELPER,
        "getWritableDatabase",
        "()Landroid/database/sqlite/SQLiteDatabase;",
        helper,
        &[],
    );
    assert!(
        first.is_err(),
        "onCreate failure must surface to the caller"
    );
    assert_eq!(
        vm.heap.get_field(helper, "rd_sqlx_db").unwrap(),
        Value::Null,
        "a failed open must not leave a cached handle (#321 bug 3)"
    );
}
