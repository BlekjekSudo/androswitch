//! Regression suite for Issue #232, item 2: the base `androidx.lifecycle.ViewModel`
//! host shim. A DEX subclass that does not define `getTag`, `setTagIfAbsent`,
//! `addCloseable` or `clear` must reach the host implementation instead of
//! failing with `UNRESOLVED_METHOD`.

use rd_testsupport::vm::*;
use rd_vm::{Engine, ObjRef, Value};

const GET_TAG_SIG: &str = "(Ljava/lang/String;)Ljava/lang/Object;";
const SET_TAG_SIG: &str = "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;";

fn dex_obj(e: &mut Engine, cls: &str) -> ObjRef {
    e.ensure_initialized(cls).expect("init cls");
    e.new_dex_instance_pub(cls).expect("alloc dex cls")
}

fn static_int(e: &Engine, cls: &str, field: &str) -> i32 {
    match e.statics.get(&(cls.to_string(), field.to_string())) {
        Some(Value::Int(n)) => *n,
        other => panic!("expected static int {cls}.{field}, got {other:?}"),
    }
}

fn key(s: &str) -> Value {
    Value::StrPlaceholder(s.to_string())
}

/// Bytecode fixture:
/// - `LVMProbe;` extends `ViewModel`, overrides `onCleared()V` (counts `cleared`).
/// - `LVMCloser;` is a plain object with `close()V` (counts `closed`).
/// - `LVMCaller;->probe(LVMProbe;)Ljava/lang/Object;` does `const-string "k"`
///   then `invoke-virtual LVMProbe;->getTag` (the exact call shape from the issue).
fn fixture() -> DexBuilder {
    let mut b = DexBuilder::new();

    let probe = b.class("LVMProbe;", "Landroidx/lifecycle/ViewModel;");
    b.static_field(probe, "cleared", "I", SVal::Int(0));
    let f_cleared = b.field_idx("LVMProbe;", "I", "cleared");
    let mut oc = op21c(0x60, 0, f_cleared);
    oc.extend(op22b(0xd8, 0, 0, 1));
    oc.extend(op21c(0x67, 0, f_cleared));
    oc.extend(op10x(0x0e));
    b.r#virtual(
        probe,
        "onCleared",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, oc)),
    );

    let closer = b.class("LVMCloser;", "Ljava/lang/Object;");
    b.static_field(closer, "closed", "I", SVal::Int(0));
    let f_closed = b.field_idx("LVMCloser;", "I", "closed");
    let mut cc = op21c(0x60, 0, f_closed);
    cc.extend(op22b(0xd8, 0, 0, 1));
    cc.extend(op21c(0x67, 0, f_closed));
    cc.extend(op10x(0x0e));
    b.r#virtual(
        closer,
        "close",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, cc)),
    );

    let p_get_tag = b.proto_idx("Ljava/lang/Object;", vec!["Ljava/lang/String;".into()]);
    let m_get_tag = b.method_idx("LVMProbe;", p_get_tag, "getTag");
    let s_k = b.intern("k") as u16;
    let caller = b.class("LVMCaller;", "Ljava/lang/Object;");
    // registers=3, ins=1: p0 = v2 (probe). const-string v0 "k";
    // invoke-virtual {v2, v0}; move-result-object v0; return-object v0
    let mut pc = op21c(0x1a, 0, s_k);
    pc.extend(op35c(0x6e, 2, m_get_tag, [2, 0, 0, 0, 0]));
    pc.extend(op11x(0x0c, 0));
    pc.extend(op11x(0x11, 0));
    b.direct(
        caller,
        "probe",
        "Ljava/lang/Object;",
        vec!["LVMProbe;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 1, 2, pc)),
    );

    b
}

#[test]
fn bytecode_get_tag_on_dex_viewmodel_subclass_resolves_to_host_shim() {
    let b = fixture();
    let mut e = engine_of(&b);
    let probe = dex_obj(&mut e, "LVMProbe;");

    let got = e
        .invoke_static(
            "LVMCaller;",
            "probe",
            "(LVMProbe;)Ljava/lang/Object;",
            &[Value::Obj(probe)],
        )
        .expect("bytecode invoke-virtual getTag must not be UNRESOLVED_METHOD");
    assert_eq!(got, Value::Null, "absent tag reads as null");
}

#[test]
fn set_tag_if_absent_keeps_first_value_and_clear_runs_on_cleared_and_closes() {
    let b = fixture();
    let mut e = engine_of(&b);
    let probe = dex_obj(&mut e, "LVMProbe;");

    let first = dex_obj(&mut e, "LVMCloser;");
    let got = e
        .invoke_instance(
            probe,
            "setTagIfAbsent",
            SET_TAG_SIG,
            &[key("k"), Value::Obj(first)],
        )
        .expect("setTagIfAbsent");
    assert_eq!(got, Value::Obj(first));

    let second = dex_obj(&mut e, "LVMCloser;");
    let again = e
        .invoke_instance(
            probe,
            "setTagIfAbsent",
            SET_TAG_SIG,
            &[key("k"), Value::Obj(second)],
        )
        .expect("setTagIfAbsent again");
    assert_eq!(again, Value::Obj(first), "existing tag wins");

    let read = e
        .invoke_instance(probe, "getTag", GET_TAG_SIG, &[key("k")])
        .expect("getTag");
    assert_eq!(read, Value::Obj(first));

    // addCloseable(Closeable) registers `second`; clear() must close it.
    e.invoke_instance(
        probe,
        "addCloseable",
        "(Ljava/io/Closeable;)V",
        &[Value::Obj(second)],
    )
    .expect("addCloseable");
    assert_eq!(static_int(&e, "LVMCloser;", "closed"), 0);

    e.invoke_instance(probe, "clear", "()V", &[])
        .expect("clear");
    assert_eq!(
        static_int(&e, "LVMProbe;", "cleared"),
        1,
        "DEX onCleared ran"
    );
    assert_eq!(
        static_int(&e, "LVMCloser;", "closed"),
        1,
        "closeable closed once"
    );
}
