//! Regression test for the #315 dispatch port (PR #396): a virtual call whose
//! DEX override takes a `long` must pass the raw register list (with the
//! `WideHi` placeholder after the wide value) to the callee. Passing the
//! logical arguments failed with
//! `INVALID_FORMAT frame: 2 argumentos para 3 registradores de entrada`
//! on real APKs (logback `setTotalSizeCap(J)V`).

mod common;

use common::*;
use rd_vm::value::Value;

const BASE: &str = "LBase;";
const CHILD: &str = "LChild;";
const MAIN: &str = "LMain;";

#[test]
fn virtual_call_with_long_argument_reaches_dex_override() {
    let mut b = DexBuilder::new();
    // class Base { void setCap(long) {} }
    let base = b.class(BASE, "Ljava/lang/Object;");
    b.r#virtual(
        base,
        "setCap",
        "V",
        vec!["J"],
        0x0001,
        Some(b.code(3, 3, 0, op10x(0x0E))),
    );
    // class Child extends Base {}
    b.class(CHILD, BASE);
    // static void go(Child c) { c.setCap(5L); }  — regs=4, ins=1 (c in v3)
    let main = b.class(MAIN, "Ljava/lang/Object;");
    let set_cap = {
        let proto = b.proto_idx("V", vec!["J".to_string()]);
        b.method_idx(BASE, proto, "setCap")
    };
    let mut u = Vec::new();
    u.extend([0x0116u16, 5]); // const-wide/16 v1, 5
    u.extend(op35c(0x6E, 3, set_cap, [3, 1, 2, 0, 0])); // invoke-virtual {v3, v1:v2}, Base.setCap(J)V
    u.extend(op10x(0x0E));
    b.direct(
        main,
        "go",
        "V",
        vec![CHILD],
        0x0009,
        Some(b.code(4, 1, 3, u)),
    );

    let mut vm = engine_of(&b);
    let child = vm
        .heap
        .alloc_instance(CHILD.to_string(), Vec::new())
        .unwrap();
    let r = vm.invoke_static(MAIN, "go", "(LChild;)V", &[Value::Obj(child)]);
    assert!(
        r.is_ok(),
        "virtual call with a long argument must run: {r:?}"
    );
}
