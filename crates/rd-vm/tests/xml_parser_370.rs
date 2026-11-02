//! Regression tests for issue #370: `XmlResourceParser.next()` returns
//! END_DOCUMENT idempotently, and `nextTag()` throws at END_DOCUMENT.

mod common;

use common::apkfix::{build_apk, build_axml, ArscFix, AxNode};
use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const RESOURCES: &str = "Landroid/content/res/Resources;";
const NEXT_I: &str = "()I";

fn resources(engine: &mut Engine) -> ObjRef {
    engine
        .heap
        .alloc_instance(RESOURCES.to_string(), Vec::new())
        .unwrap()
}

/// `<LinearLayout><TextView/></LinearLayout>` as `res/layout/root.xml`.
fn engine_with_layout() -> (Engine, i32) {
    let mut arsc = ArscFix::new();
    let rid = arsc.add_layout_file("root");
    let root = AxNode::new(
        "LinearLayout",
        vec![],
        vec![AxNode::new("TextView", vec![], vec![])],
    );
    let apk = build_apk(
        "com.test.xml370",
        "LMain;",
        arsc.build(),
        vec![("res/layout/root.xml", build_axml(&root))],
    );
    let mut engine = engine_of(&DexBuilder::new());
    engine.set_resources(rd_apk::Apk::from_bytes(apk).expect("APK sintético parseia"));
    (engine, rid as i32)
}

fn open_parser(engine: &mut Engine, id: i32) -> ObjRef {
    let res = resources(engine);
    match engine
        .invoke_instance(
            res,
            "getXml",
            "(I)Landroid/content/res/XmlResourceParser;",
            &[Value::Int(id)],
        )
        .unwrap_or_else(|e| panic!("getXml failed: {e:?}"))
    {
        Value::Obj(p) => p,
        other => panic!("expected parser, got {other:?}"),
    }
}

fn step(engine: &mut Engine, parser: ObjRef, name: &str) -> Value {
    engine.invoke_instance(parser, name, NEXT_I, &[]).unwrap()
}

#[test]
fn next_keeps_returning_end_document_after_the_end() {
    let (mut engine, id) = engine_with_layout();
    let p = open_parser(&mut engine, id);
    // START_TAG, START_TAG, END_TAG, END_TAG, then END_DOCUMENT forever.
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(3));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(3));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(1));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(1));
    assert_eq!(step(&mut engine, p, "nextToken"), Value::Int(1));
}

#[test]
fn next_tag_skips_to_tags_and_throws_at_end_document() {
    let (mut engine, id) = engine_with_layout();
    let p = open_parser(&mut engine, id);
    assert_eq!(step(&mut engine, p, "nextTag"), Value::Int(2));
    assert_eq!(step(&mut engine, p, "nextTag"), Value::Int(2));
    assert_eq!(step(&mut engine, p, "nextTag"), Value::Int(3));
    assert_eq!(step(&mut engine, p, "nextTag"), Value::Int(3));
    // Past the last tag, nextTag() must throw XmlPullParserException.
    assert!(engine.invoke_instance(p, "nextTag", NEXT_I, &[]).is_err());
}

fn engine_with_text_layout() -> (Engine, i32) {
    let mut arsc = ArscFix::new();
    let rid = arsc.add_layout_file("texts");
    // <LinearLayout><TextView/></LinearLayout>. The AXML fixture cannot carry
    // element text, so nextText() is covered on an empty element here.
    let root = AxNode::new(
        "LinearLayout",
        vec![],
        vec![AxNode::new("TextView", vec![], vec![])],
    );
    let apk = build_apk(
        "com.test.xml370b",
        "LMain;",
        arsc.build(),
        vec![("res/layout/texts.xml", build_axml(&root))],
    );
    let mut engine = engine_of(&DexBuilder::new());
    engine.set_resources(rd_apk::Apk::from_bytes(apk).expect("APK sintético parseia"));
    (engine, rid as i32)
}

#[test]
fn close_is_idempotent_and_keeps_answering_after_close() {
    let (mut engine, id) = engine_with_text_layout();
    let p = open_parser(&mut engine, id);
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    engine.invoke_instance(p, "close", "()V", &[]).unwrap();
    engine
        .invoke_instance(p, "close", "()V", &[])
        .expect("second close() must succeed");
    assert_eq!(
        engine
            .invoke_instance(p, "getEventType", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn require_throws_on_mismatch_and_passes_on_match() {
    let (mut engine, id) = engine_with_text_layout();
    let p = open_parser(&mut engine, id);
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    let name =
        |engine: &mut Engine, s: &str| Value::Obj(engine.heap.alloc_string(s.to_string()).unwrap());
    let ok_name = name(&mut engine, "LinearLayout");
    engine
        .invoke_instance(
            p,
            "require",
            "(ILjava/lang/String;Ljava/lang/String;)V",
            &[Value::Int(2), Value::Null, ok_name],
        )
        .expect("matching require() must pass");
    let bad_name = name(&mut engine, "Button");
    assert!(engine
        .invoke_instance(
            p,
            "require",
            "(ILjava/lang/String;Ljava/lang/String;)V",
            &[Value::Int(2), Value::Null, bad_name],
        )
        .is_err());
    assert!(engine
        .invoke_instance(
            p,
            "require",
            "(ILjava/lang/String;Ljava/lang/String;)V",
            &[Value::Int(3), Value::Null, Value::Null],
        )
        .is_err());
}

#[test]
fn next_text_on_element_without_text_returns_empty_and_stops_on_end_tag() {
    let (mut engine, id) = engine_with_text_layout();
    let p = open_parser(&mut engine, id);
    // Position on the root START_TAG, then nextText() on the TextView (child is empty).
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    assert_eq!(step(&mut engine, p, "next"), Value::Int(2));
    let text = engine
        .invoke_instance(p, "nextText", "()Ljava/lang/String;", &[])
        .unwrap();
    let Value::Obj(t) = text else {
        panic!("expected String");
    };
    assert_eq!(engine.heap.as_str(t).unwrap(), "");
    assert_eq!(step(&mut engine, p, "getEventType"), Value::Int(3));
}
