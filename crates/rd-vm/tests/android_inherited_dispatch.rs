//! D8 may declare an inherited Android method on an app subclass.
#[macro_use]
mod common;
use common::*;
use rd_vm::{framework::HostObj, heap::HeapObj, Value, VmExit};
fn fixture(override_id: bool) -> rd_vm::Engine {
    let mut b = DexBuilder::new();
    let custom = b.class("LCustomEdit;", "Landroid/widget/EditText;");
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    let pi = b.proto_idx("V", vec!["I".into()]);
    let pt = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".into()]);
    let setid = b.method_idx(
        if override_id {
            "Landroid/view/View;"
        } else {
            "LCustomEdit;"
        },
        pi,
        "setId",
    );
    let settext = b.method_idx("LCustomEdit;", pt, "setText");
    if override_id {
        let super_id = b.method_idx("Landroid/view/View;", pi, "setId");
        let mut code = op22b(0xd8, 2, 2, 1);
        code.extend(op35c(0x6f, 2, super_id, [1, 2, 0, 0, 0]));
        code.extend(op10x(0x0e));
        b.r#virtual(
            custom,
            "setId",
            "V",
            vec!["I"],
            ACC_PUBLIC,
            Some(b.code(3, 2, 2, code)),
        );
    }
    let text = b.intern("actual text") as u16;
    let mut code = op11n(0x12, 0, 7);
    code.extend(op35c(0x6e, 2, setid, [2, 0, 0, 0, 0]));
    code.extend(op21c(0x1a, 1, text));
    code.extend(op35c(0x6e, 2, settext, [2, 1, 0, 0, 0]));
    code.extend(op10x(0x0e));
    b.direct(
        caller,
        "run",
        "V",
        vec!["LCustomEdit;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 1, 2, code)),
    );
    engine_of(&b)
}
fn run(override_id: bool) {
    let mut e = fixture(override_id);
    let view = e
        .heap
        .alloc_instance("LCustomEdit;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCustomEdit;", view).unwrap();
    e.invoke_static("LCaso;", "run", "(LCustomEdit;)V", &[Value::Obj(view)])
        .unwrap();
    let expected = if override_id { 8 } else { 7 };
    assert!(matches!(e.fw.get(view),Some(HostObj::View{id,..}) if *id==expected));
    assert_eq!(
        e.invoke_instance(view, "getId", "()I", &[]).unwrap(),
        Value::Int(expected)
    );
    let text = e
        .invoke_instance(view, "getText", "()Ljava/lang/CharSequence;", &[])
        .unwrap();
    assert!(
        matches!(text,Value::Obj(r) if matches!(e.heap.get(r),Ok(HeapObj::Str(s)) if s=="actual text"))
    );
    let error = e
        .invoke_instance(view, "unrecognizedCall", "()V", &[])
        .unwrap_err();
    assert!(matches!(error,VmExit::Error(e) if e.code=="NOT_FOUND"));
}
#[test]
fn inherited_ids_and_text_use_concrete_models_not_generic_properties() {
    run(false);
}
#[test]
fn virtual_dex_override_wins_and_invoke_super_does_not_recurse() {
    run(true);
}
#[test]
fn cyclic_nonplatform_hierarchy_fails_closed() {
    let mut b = DexBuilder::new();
    b.class("LA;", "LB;");
    b.class("LB;", "LA;");
    let mut e = engine_of(&b);
    let r = e.heap.alloc_instance("LA;".into(), vec![]).unwrap();
    assert!(
        matches!(e.invoke_instance(r,"unrecognizedCall","()V",&[]),Err(VmExit::Error(e)) if e.code=="NOT_FOUND")
    );
}
