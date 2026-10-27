//! Real DEX literal opcodes, classloader isolation and bounded runtime strings.
use rd_testsupport::vm::{
    op11n, op11x, op21c, op21s, op22b, op22t, op35c, DexBuilder, ACC_PUBLIC, ACC_STATIC,
};
use rd_vm::{Engine, Value, VmConfig};
fn constant(class: &str, jumbo: bool) -> rd_dex::Dex {
    let mut b = DexBuilder::new();
    let owner = b.class(class, "Ljava/lang/Object;");
    let text = b.intern("shared literal \u{1f642}");
    let mut code = if jumbo {
        vec![0x001b, text as u16, (text >> 16) as u16]
    } else {
        op21c(0x1a, 0, text as u16)
    };
    code.extend(op11x(0x11, 0));
    b.direct(
        owner,
        "literal",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, code)),
    );
    rd_dex::Dex::parse(b.finish()).unwrap()
}
#[test]
fn normal_and_jumbo_literals_share_identity_across_dexes_and_survive_gc() {
    let mut vm = Engine::new(
        vec![constant("LFirst;", false), constant("LSecond;", true)],
        VmConfig::default(),
    );
    let a = vm
        .invoke_static("LFirst;", "literal", "()Ljava/lang/String;", &[])
        .unwrap();
    vm.gc_minor();
    vm.gc();
    let b = vm
        .invoke_static("LSecond;", "literal", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(
        vm.heap.as_str(b.as_ref().unwrap().unwrap()).unwrap(),
        "shared literal \u{1f642}"
    );
    let mut other = Engine::new(vec![constant("LFirst;", true)], VmConfig::default());
    other
        .heap
        .alloc_string("other Engine's object at the same ID".into())
        .unwrap();
    let separate = other
        .invoke_static("LFirst;", "literal", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(
        other
            .heap
            .as_str(separate.as_ref().unwrap().unwrap())
            .unwrap(),
        "shared literal \u{1f642}"
    );
}
#[test]
fn one_dex_call_reclaims_many_times_its_heap_budget_in_dynamic_strings() {
    let mut b = DexBuilder::new();
    let owner = b.class("LStringChurn;", "Ljava/lang/Object;");
    let text_proto = b.proto_idx("Ljava/lang/String;", vec!["I".into()]);
    let to_string = b.method_idx("Ljava/lang/Integer;", text_proto, "toString");
    let void_proto = b.proto_idx("V", vec![]);
    let gc = b.method_idx("Ljava/lang/System;", void_proto, "gc");
    let mut code = op11n(0x12, 0, 0);
    code.extend(op21s(0x13, 1, 512));
    let start = code.len();
    code.extend(op35c(0x71, 1, to_string, [0, 0, 0, 0, 0]));
    code.extend(op11x(0x0c, 2));
    code.extend(op35c(0x71, 0, gc, [0, 0, 0, 0, 0]));
    code.extend(op22b(0xd8, 0, 0, 1));
    let branch = code.len();
    code.extend(op22t(0x34, 0, 1, start as i16 - branch as i16));
    code.extend(op11x(0x0f, 0));
    b.direct(
        owner,
        "run",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 1, code)),
    );
    let mut vm = Engine::new(
        vec![rd_dex::Dex::parse(b.finish()).unwrap()],
        VmConfig {
            heap_budget: 4096,
            ..Default::default()
        },
    );
    assert_eq!(
        vm.invoke_static("LStringChurn;", "run", "()I", &[])
            .unwrap(),
        Value::Int(512)
    );
    vm.gc();
    assert!(
        vm.heap.used() < 512,
        "transient strings remain rooted: {} bytes",
        vm.heap.used()
    );
}
