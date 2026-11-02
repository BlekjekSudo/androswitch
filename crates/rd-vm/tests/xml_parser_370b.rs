//! Regression test for issue #370 item 4 (`crates/rd-vm/src/xml_res.rs`):
//! `XmlResourceParser` event trees live on the host, outside `heap_budget`.
//! Opening a parser whose tree exceeds the budget must throw
//! `OutOfMemoryError`; a small tree must still open under the same budget.

mod common;

use common::apkfix::{build_apk, build_axml, ArscFix, AxNode};
use common::*;
use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const RESOURCES: &str = "Landroid/content/res/Resources;";
const GET_XML: &str = "(I)Landroid/content/res/XmlResourceParser;";

/// `<LinearLayout>` with `children` `<TextView/>` children, as `res/layout/root.xml`.
fn engine_with_children(children: usize, heap_budget: usize) -> (Engine, i32) {
    let mut arsc = ArscFix::new();
    let rid = arsc.add_layout_file("root");
    let kids = (0..children)
        .map(|_| AxNode::new("TextView", vec![], vec![]))
        .collect();
    let root = AxNode::new("LinearLayout", vec![], kids);
    let apk = build_apk(
        "com.test.xml370b",
        "LMain;",
        arsc.build(),
        vec![("res/layout/root.xml", build_axml(&root))],
    );
    let dex = rd_dex::Dex::parse(DexBuilder::new().finish()).expect("DEX sintético parseia");
    let mut engine = Engine::new(
        vec![dex],
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    );
    engine.set_resources(rd_apk::Apk::from_bytes(apk).expect("APK sintético parseia"));
    (engine, rid as i32)
}

fn open_parser(engine: &mut Engine, id: i32) -> Result<Value, VmExit> {
    let res = engine
        .heap
        .alloc_instance(RESOURCES.to_string(), Vec::new())
        .expect("alloc Resources");
    engine.invoke_instance(res, "getXml", GET_XML, &[Value::Int(id)])
}

#[test]
fn issue_370_parser_tree_is_charged_to_heap_budget() {
    // ~4000 elements → ~8000 host events, several hundred KiB, against a 256 KiB budget.
    let (mut engine, id) = engine_with_children(4000, 256 * 1024);
    match open_parser(&mut engine, id) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;", "wrong exception");
            assert!(
                t.message
                    .as_deref()
                    .is_some_and(|m| m.contains("host bytes")),
                "OOM must come from the parser charge, got {:?}",
                t.message
            );
        }
        other => panic!("oversized parser tree must throw OOM, got {other:?}"),
    }
}

#[test]
fn issue_370_small_parser_tree_still_opens_under_small_budget() {
    let (mut engine, id) = engine_with_children(10, 256 * 1024);
    let parser = open_parser(&mut engine, id);
    assert!(
        matches!(parser, Ok(Value::Obj(_))),
        "small tree must open, got {parser:?}"
    );
}

#[test]
fn issue_370_large_parser_tree_opens_under_default_budget() {
    let (mut engine, id) = engine_with_children(4000, VmConfig::default().heap_budget);
    let parser = open_parser(&mut engine, id);
    assert!(
        matches!(parser, Ok(Value::Obj(_))),
        "large tree must open under the default budget, got {parser:?}"
    );
}
