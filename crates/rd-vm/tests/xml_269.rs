//! Regression tests for issue #269 (`org.xmlpull.v1.sax2.Driver` / `AttributesImpl`):
//! 1. Valid CDATA sections containing `]]` (`[[1, 2]]`, `]]]>`) parse without false `"unterminated CDATA section"`.
//! 2. `line_column` advances incrementally in `O(N)` instead of quadratic `O(N * E)`.
//! 3. `xmlns` / `xmlns:prefix` scoping populates `startElement`/`endElement` `uri`+`localName` and `Attributes.getValue(uri, localName)` / `getIndex(uri, localName)` (including `AttributesImpl(Attributes)` copies), while keeping unprefixed attribute `uri == ""`.
//! 4. Odd-byte-count UTF-16LE/BE inputs after BOM fail closed with `SAXException`.

#[macro_use]
mod common;

use std::time::Instant;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_instance_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const DRIVER: &str = "Lorg/xmlpull/v1/sax2/Driver;";
const INPUT_SOURCE: &str = "Lorg/xml/sax/InputSource;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";
const DEFAULT_HANDLER: &str = "Lorg/xml/sax/helpers/DefaultHandler;";
const ATTRIBUTES: &str = "Lorg/xml/sax/Attributes;";
const ATTRIBUTES_IMPL: &str = "Lorg/xml/sax/helpers/AttributesImpl;";
const LOCATOR: &str = "Lorg/xml/sax/Locator;";
const ARRAY_LIST: &str = "Ljava/util/ArrayList;";
const STRING: &str = "Ljava/lang/String;";
const OBJECT: &str = "Ljava/lang/Object;";

/// Builds a DEX `LRecHandler;` extending `DefaultHandler` with:
/// - `locator: Lorg/xml/sax/Locator;`
/// - `starts: Ljava/util/ArrayList;` (pushes `uri`, `local`, `qname`, `new AttributesImpl(atts)` per `startElement`)
/// - `ends: Ljava/util/ArrayList;` (pushes `uri`, `local`, `qname` per `endElement`)
/// - `texts: Ljava/util/ArrayList;` (pushes `new String(ch, start, length)` per `characters`)
fn build_recording_handler_dex() -> rd_dex::Dex {
    let mut b = DexBuilder::new();
    let h = b.class("LRecHandler;", DEFAULT_HANDLER);
    let f_loc = b.field_idx("LRecHandler;", LOCATOR, "locator");
    let f_starts = b.field_idx("LRecHandler;", ARRAY_LIST, "starts");
    let f_ends = b.field_idx("LRecHandler;", ARRAY_LIST, "ends");
    let f_texts = b.field_idx("LRecHandler;", ARRAY_LIST, "texts");

    b.instance_field(h, "locator", LOCATOR);
    b.instance_field(h, "starts", ARRAY_LIST);
    b.instance_field(h, "ends", ARRAY_LIST);
    b.instance_field(h, "texts", ARRAY_LIST);

    let al_type = b.type_idx(ARRAY_LIST);
    let al_init_proto = b.proto_idx("V", vec![]);
    let al_init = b.method_idx(ARRAY_LIST, al_init_proto, "<init>");
    let al_add_proto = b.proto_idx("Z", vec![OBJECT.to_string()]);
    let al_add = b.method_idx(ARRAY_LIST, al_add_proto, "add");

    let dh_init = b.method_idx(DEFAULT_HANDLER, al_init_proto, "<init>");

    let ai_type = b.type_idx(ATTRIBUTES_IMPL);
    let ai_init_proto = b.proto_idx("V", vec![ATTRIBUTES.to_string()]);
    let ai_init = b.method_idx(ATTRIBUTES_IMPL, ai_init_proto, "<init>");

    let str_type = b.type_idx(STRING);
    let str_init_proto = b.proto_idx(
        "V",
        vec!["[C".to_string(), "I".to_string(), "I".to_string()],
    );
    let str_init = b.method_idx(STRING, str_init_proto, "<init>");

    // <init>()V: super.<init>(), allocate starts/ends/texts ArrayLists.
    let mut init_u = Vec::new();
    init_u.extend(op35c(0x70, 1, dh_init, [1, 0, 0, 0, 0])); // invoke-direct {v1}, DefaultHandler.<init>
    for field in [f_starts, f_ends, f_texts] {
        init_u.extend(op21c(0x22, 0, al_type)); // new-instance v0, ArrayList
        init_u.extend(op35c(0x70, 1, al_init, [0, 0, 0, 0, 0])); // invoke-direct {v0}, ArrayList.<init>
        init_u.extend(op22c(0x5b, 0, 1, field)); // iput-object v0, v1, field
    }
    init_u.extend(op10x(0x0e)); // return-void
    let init_code = b.code(2, 1, 1, init_u);
    b.direct(h, "<init>", "V", vec![], 0x10001, Some(init_code));

    // setDocumentLocator(Locator)V: regs=2, ins=2 -> v0=this, v1=locator
    let mut loc_u = Vec::new();
    loc_u.extend(op22c(0x5b, 1, 0, f_loc)); // iput-object v1, v0, locator
    loc_u.extend(op10x(0x0e));
    let loc_code = b.code(2, 2, 0, loc_u);
    b.r#virtual(
        h,
        "setDocumentLocator",
        "V",
        vec![LOCATOR],
        0x0001,
        Some(loc_code),
    );

    // startElement(String uri, String local, String qname, Attributes atts)V
    // regs=7, ins=5 -> v2=this, v3=uri, v4=local, v5=qname, v6=atts
    let mut se_u = Vec::new();
    se_u.extend(op22c(0x54, 0, 2, f_starts)); // iget-object v0, v2, starts
    se_u.extend(op35c(0x6e, 2, al_add, [0, 3, 0, 0, 0])); // starts.add(uri)
    se_u.extend(op35c(0x6e, 2, al_add, [0, 4, 0, 0, 0])); // starts.add(local)
    se_u.extend(op35c(0x6e, 2, al_add, [0, 5, 0, 0, 0])); // starts.add(qname)
    se_u.extend(op21c(0x22, 1, ai_type)); // new-instance v1, AttributesImpl
    se_u.extend(op35c(0x70, 2, ai_init, [1, 6, 0, 0, 0])); // new AttributesImpl(atts)
    se_u.extend(op35c(0x6e, 2, al_add, [0, 1, 0, 0, 0])); // starts.add(copy)
    se_u.extend(op10x(0x0e));
    let se_code = b.code(7, 5, 2, se_u);
    b.r#virtual(
        h,
        "startElement",
        "V",
        vec![STRING, STRING, STRING, ATTRIBUTES],
        0x0001,
        Some(se_code),
    );

    // endElement(String uri, String local, String qname)V
    // regs=5, ins=4 -> v1=this, v2=uri, v3=local, v4=qname
    let mut ee_u = Vec::new();
    ee_u.extend(op22c(0x54, 0, 1, f_ends)); // iget-object v0, v1, ends
    ee_u.extend(op35c(0x6e, 2, al_add, [0, 2, 0, 0, 0]));
    ee_u.extend(op35c(0x6e, 2, al_add, [0, 3, 0, 0, 0]));
    ee_u.extend(op35c(0x6e, 2, al_add, [0, 4, 0, 0, 0]));
    ee_u.extend(op10x(0x0e));
    let ee_code = b.code(5, 4, 2, ee_u);
    b.r#virtual(
        h,
        "endElement",
        "V",
        vec![STRING, STRING, STRING],
        0x0001,
        Some(ee_code),
    );

    // characters([C ch, I start, I len)V
    // regs=6, ins=4 -> v2=this, v3=ch, v4=start, v5=len
    let mut ch_u = Vec::new();
    ch_u.extend(op21c(0x22, 0, str_type)); // new-instance v0, String
    ch_u.extend(op35c(0x70, 4, str_init, [0, 3, 4, 5, 0])); // new String(ch, start, len)
    ch_u.extend(op22c(0x54, 1, 2, f_texts)); // iget-object v1, v2, texts
    ch_u.extend(op35c(0x6e, 2, al_add, [1, 0, 0, 0, 0])); // texts.add(str)
    ch_u.extend(op10x(0x0e));
    let ch_code = b.code(6, 4, 4, ch_u);
    b.r#virtual(
        h,
        "characters",
        "V",
        vec!["[C", "I", "I"],
        0x0001,
        Some(ch_code),
    );

    rd_dex::Dex::parse(b.finish()).unwrap()
}

fn parse_bytes_with_driver(vm: &mut Engine, handler: u32, raw_bytes: &[u8]) -> Result<(), VmExit> {
    let byte_arr = vm
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, raw_bytes.len(), "[B")
        .unwrap();
    {
        let elems = vm.heap.array_elems_mut(byte_arr).unwrap();
        for (slot, b) in elems.iter_mut().zip(raw_bytes.iter()) {
            *slot = Value::Int((*b as i8) as i32);
        }
    }
    let bais = vm
        .heap
        .alloc_instance(BAIS.to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(bais, "<init>", "([B)V", &[Value::Obj(byte_arr)])
        .unwrap();

    let src = vm
        .heap
        .alloc_instance(INPUT_SOURCE.to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(
        src,
        "<init>",
        "(Ljava/io/InputStream;)V",
        &[Value::Obj(bais)],
    )
    .unwrap();

    let driver = vm
        .heap
        .alloc_instance(DRIVER.to_string(), Vec::new())
        .unwrap();
    vm.invoke_instance(driver, "<init>", "()V", &[]).unwrap();
    vm.invoke_instance(
        driver,
        "setContentHandler",
        "(Lorg/xml/sax/ContentHandler;)V",
        &[Value::Obj(handler)],
    )
    .unwrap();
    vm.invoke_instance(
        driver,
        "parse",
        "(Lorg/xml/sax/InputSource;)V",
        &[Value::Obj(src)],
    )
    .map(|_| ())
}

fn list_items(vm: &mut Engine, list_obj: u32) -> Vec<Value> {
    let sz = call_instance_intrinsic(vm, ARRAY_LIST, "size", "()I", list_obj, &[])
        .unwrap()
        .unwrap()
        .as_int()
        .unwrap();
    let mut out = Vec::with_capacity(sz as usize);
    for i in 0..sz {
        let v = call_instance_intrinsic(
            vm,
            ARRAY_LIST,
            "get",
            "(I)Ljava/lang/Object;",
            list_obj,
            &[Value::Int(i)],
        )
        .unwrap()
        .unwrap();
        out.push(v);
    }
    out
}

fn val_str(vm: &Engine, v: Value) -> String {
    match v {
        Value::Obj(r) => vm.heap.as_str(r).unwrap().to_string(),
        other => panic!("expected String Obj, got {other:?}"),
    }
}

#[test]
fn issue_269_cdata_with_double_brackets_parses_cleanly() {
    let dex = build_recording_handler_dex();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let handler = vm.new_instance("LRecHandler;").unwrap();
    vm.invoke_instance(handler, "<init>", "()V", &[]).unwrap();

    let xml = b"<root><a><![CDATA[[[1, 2]]]]></a><b><![CDATA[]]]></b><c><![CDATA[]]]]></c><d><![CDATA[x[y[0]]z]]></d></root>";
    parse_bytes_with_driver(&mut vm, handler, xml).unwrap();

    let texts_ref = match vm.heap.get_field(handler, "texts").unwrap() {
        Value::Obj(r) => r,
        other => panic!("texts: {other:?}"),
    };
    let texts: Vec<String> = list_items(&mut vm, texts_ref)
        .into_iter()
        .map(|v| val_str(&vm, v))
        .collect();
    assert_eq!(texts, vec!["[[1, 2]]", "]", "]]", "x[y[0]]z"]);
}

#[test]
fn issue_269_namespaces_resolve_on_elements_and_attributes_get_value_uri_local() {
    let dex = build_recording_handler_dex();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let handler = vm.new_instance("LRecHandler;").unwrap();
    vm.invoke_instance(handler, "<init>", "()V", &[]).unwrap();

    let xml = br#"<manifest xmlns="urn:default" xmlns:android="http://schemas.android.com/apk/res/android" package="io.rd.demo" android:versionCode="7"><android:application xmlns="urn:inner" android:label="Demo" plain="ok"/></manifest>"#;
    parse_bytes_with_driver(&mut vm, handler, xml).unwrap();

    let starts_ref = match vm.heap.get_field(handler, "starts").unwrap() {
        Value::Obj(r) => r,
        other => panic!("starts: {other:?}"),
    };
    let starts = list_items(&mut vm, starts_ref);
    assert_eq!(starts.len(), 8); // 2 elements * 4 entries (uri, local, qname, AttributesImpl)

    // Element 0: <manifest xmlns="urn:default" ...>
    assert_eq!(val_str(&vm, starts[0].clone()), "urn:default");
    assert_eq!(val_str(&vm, starts[1].clone()), "manifest");
    assert_eq!(val_str(&vm, starts[2].clone()), "manifest");
    let atts0 = match &starts[3] {
        Value::Obj(r) => *r,
        other => panic!("atts0: {other:?}"),
    };

    let ns_android = Value::Obj(
        vm.heap
            .alloc_string("http://schemas.android.com/apk/res/android".into())
            .unwrap(),
    );
    let empty_ns = Value::Obj(vm.heap.alloc_string(String::new()).unwrap());
    let k_vc = Value::Obj(vm.heap.alloc_string("versionCode".into()).unwrap());
    let k_pkg = Value::Obj(vm.heap.alloc_string("package".into()).unwrap());

    let vc_val = vm
        .invoke_instance(
            atts0,
            "getValue",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[ns_android.clone(), k_vc.clone()],
        )
        .unwrap();
    assert_eq!(val_str(&vm, vc_val), "7");
    assert_eq!(
        vm.invoke_instance(
            atts0,
            "getIndex",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[ns_android.clone(), k_vc],
        )
        .unwrap()
        .as_int()
        .unwrap(),
        1
    );

    // Unprefixed attribute `package` has `uri == ""` (NOT default namespace `"urn:default"`).
    let pkg_val = vm
        .invoke_instance(
            atts0,
            "getValue",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[empty_ns.clone(), k_pkg.clone()],
        )
        .unwrap();
    assert_eq!(val_str(&vm, pkg_val), "io.rd.demo");
    let default_ns = Value::Obj(vm.heap.alloc_string("urn:default".into()).unwrap());
    assert_eq!(
        vm.invoke_instance(
            atts0,
            "getValue",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[default_ns, k_pkg],
        )
        .unwrap(),
        Value::Null
    );

    // Element 1: <android:application xmlns="urn:inner" android:label="Demo" plain="ok"/>
    assert_eq!(
        val_str(&vm, starts[4].clone()),
        "http://schemas.android.com/apk/res/android"
    );
    assert_eq!(val_str(&vm, starts[5].clone()), "application");
    assert_eq!(val_str(&vm, starts[6].clone()), "android:application");
    let atts1 = match &starts[7] {
        Value::Obj(r) => *r,
        other => panic!("atts1: {other:?}"),
    };
    let k_label = Value::Obj(vm.heap.alloc_string("label".into()).unwrap());
    let k_plain = Value::Obj(vm.heap.alloc_string("plain".into()).unwrap());
    let lbl_val = vm
        .invoke_instance(
            atts1,
            "getValue",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[ns_android, k_label],
        )
        .unwrap();
    assert_eq!(val_str(&vm, lbl_val), "Demo");
    let plain_val = vm
        .invoke_instance(
            atts1,
            "getValue",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[empty_ns, k_plain],
        )
        .unwrap();
    assert_eq!(val_str(&vm, plain_val), "ok");

    // End elements pop scope cleanly (`android:application` then `manifest` back in `urn:default`).
    let ends_ref = match vm.heap.get_field(handler, "ends").unwrap() {
        Value::Obj(r) => r,
        other => panic!("ends: {other:?}"),
    };
    let ends: Vec<String> = list_items(&mut vm, ends_ref)
        .into_iter()
        .map(|v| val_str(&vm, v))
        .collect();
    assert_eq!(
        ends,
        vec![
            "http://schemas.android.com/apk/res/android",
            "application",
            "android:application",
            "urn:default",
            "manifest",
            "manifest",
        ]
    );
}

#[test]
fn issue_269_incremental_line_column_handles_many_events_linearly() {
    let mut b = DexBuilder::new();
    let h = b.class("LNoopHandler;", DEFAULT_HANDLER);
    let f_loc = b.field_idx("LNoopHandler;", LOCATOR, "locator");
    b.instance_field(h, "locator", LOCATOR);
    let void_proto = b.proto_idx("V", vec![]);
    let dh_init = b.method_idx(DEFAULT_HANDLER, void_proto, "<init>");
    let mut init_u = Vec::new();
    init_u.extend(op35c(0x70, 1, dh_init, [0, 0, 0, 0, 0]));
    init_u.extend(op10x(0x0e));
    let init_code = b.code(1, 1, 1, init_u);
    b.direct(h, "<init>", "V", vec![], 0x10001, Some(init_code));

    let mut loc_u = Vec::new();
    loc_u.extend(op22c(0x5b, 1, 0, f_loc));
    loc_u.extend(op10x(0x0e));
    let loc_code = b.code(2, 2, 0, loc_u);
    b.r#virtual(
        h,
        "setDocumentLocator",
        "V",
        vec![LOCATOR],
        0x0001,
        Some(loc_code),
    );
    b.r#virtual(
        h,
        "startDocument",
        "V",
        vec![],
        0x0001,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    b.r#virtual(
        h,
        "endDocument",
        "V",
        vec![],
        0x0001,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    let noop5 = b.code(5, 5, 0, op10x(0x0e));
    b.r#virtual(
        h,
        "startElement",
        "V",
        vec![STRING, STRING, STRING, ATTRIBUTES],
        0x0001,
        Some(noop5),
    );
    let noop4 = b.code(4, 4, 0, op10x(0x0e));
    b.r#virtual(
        h,
        "endElement",
        "V",
        vec![STRING, STRING, STRING],
        0x0001,
        Some(noop4),
    );
    b.r#virtual(
        h,
        "characters",
        "V",
        vec!["[C", "I", "I"],
        0x0001,
        Some(b.code(4, 4, 0, op10x(0x0e))),
    );
    let dex = rd_dex::Dex::parse(b.finish()).unwrap();

    let cfg = VmConfig {
        fuel: 200_000_000,
        ..VmConfig::default()
    };
    let build_xml = |n: usize| -> String {
        let pad = "x".repeat(120);
        let mut s = String::with_capacity(n * 140 + 32);
        s.push_str("<root>\n");
        for _ in 0..n {
            s.push_str("<!--");
            s.push_str(&pad);
            s.push_str("--><i/>\n");
        }
        s.push_str("</root>");
        s
    };
    // Feed both documents via `apk:/...` systemId so byte-by-byte InputStream.read()
    // dispatch does not mask parser complexity: N=1,500 vs 4N=6,000 lines gives
    // t(4N)/t(N) ~ 4x with the incremental O(N) cursor vs ~ 14-16x with quadratic rescan.
    let xml_n = build_xml(500);
    let xml_4n = build_xml(2_000);
    let apk_bytes = common::apkfix::build_apk(
        "xml.t",
        "LNoopHandler;",
        common::apkfix::ArscFix::new().build(),
        vec![
            ("assets/n.xml", xml_n.into_bytes()),
            ("assets/4n.xml", xml_4n.into_bytes()),
        ],
    );

    let run_once = |entry_url: &str| -> (f64, i32, i32) {
        let mut vm = Engine::new(vec![dex.clone()], cfg.clone());
        vm.set_resources(rd_apk::Apk::from_bytes(apk_bytes.clone()).unwrap());
        let handler = vm.new_instance("LNoopHandler;").unwrap();
        vm.invoke_instance(handler, "<init>", "()V", &[]).unwrap();
        let sys_id = vm.heap.alloc_string(entry_url.to_string()).unwrap();
        let src = vm
            .heap
            .alloc_instance(INPUT_SOURCE.to_string(), Vec::new())
            .unwrap();
        vm.invoke_instance(
            src,
            "<init>",
            "(Ljava/lang/String;)V",
            &[Value::Obj(sys_id)],
        )
        .unwrap();
        let driver = vm
            .heap
            .alloc_instance(DRIVER.to_string(), Vec::new())
            .unwrap();
        vm.invoke_instance(driver, "<init>", "()V", &[]).unwrap();
        vm.invoke_instance(
            driver,
            "setContentHandler",
            "(Lorg/xml/sax/ContentHandler;)V",
            &[Value::Obj(handler)],
        )
        .unwrap();

        let t0 = Instant::now();
        vm.invoke_instance(
            driver,
            "parse",
            "(Lorg/xml/sax/InputSource;)V",
            &[Value::Obj(src)],
        )
        .unwrap();
        let dt = t0.elapsed().as_secs_f64();
        let loc = match vm.heap.get_field(handler, "locator").unwrap() {
            Value::Obj(r) => r,
            other => panic!("locator: {other:?}"),
        };
        let line = vm
            .invoke_instance(loc, "getLineNumber", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap();
        let col = vm
            .invoke_instance(loc, "getColumnNumber", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap();
        (dt, line, col)
    };

    let _ = run_once("apk:/assets/n.xml");
    let mut best_n = f64::INFINITY;
    let mut best_4n = f64::INFINITY;
    let mut last_pos = (0, 0);
    for _ in 0..3 {
        let (tn, _, _) = run_once("apk:/assets/n.xml");
        best_n = best_n.min(tn);
        let (t4, line, col) = run_once("apk:/assets/4n.xml");
        best_4n = best_4n.min(t4);
        last_pos = (line, col);
    }
    let ratio = best_4n / best_n.max(1e-6);
    assert!(
        ratio < 8.0,
        "expected linear scaling t(4N)/t(N) < 8.0 (linear ~4x, quadratic ~16x), got {ratio:.2} (tN={best_n:.4}s, t4N={best_4n:.4}s)"
    );
    assert_eq!(last_pos, (2_002, 1));
}

#[test]
fn issue_269_truncated_odd_byte_utf16_is_rejected_with_sax_exception() {
    let dex = build_recording_handler_dex();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let handler = vm.new_instance("LRecHandler;").unwrap();
    vm.invoke_instance(handler, "<init>", "()V", &[]).unwrap();

    // Valid UTF-16LE `<a/>` + 1 trailing odd byte `0x7F` must fail closed with SAXException!
    let mut utf16le_odd = vec![0xFF, 0xFE];
    for unit in "<a/>".encode_utf16() {
        utf16le_odd.extend_from_slice(&unit.to_le_bytes());
    }
    utf16le_odd.push(0x7F);
    match parse_bytes_with_driver(&mut vm, handler, &utf16le_odd) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Lorg/xml/sax/SAXException;"),
        other => panic!("expected SAXException for odd UTF-16LE, got {other:?}"),
    }

    // Valid UTF-16BE `<a/>` + 1 trailing odd byte `0x7F` must fail closed with SAXException!
    let mut utf16be_odd = vec![0xFE, 0xFF];
    for unit in "<a/>".encode_utf16() {
        utf16be_odd.extend_from_slice(&unit.to_be_bytes());
    }
    utf16be_odd.push(0x7F);
    match parse_bytes_with_driver(&mut vm, handler, &utf16be_odd) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Lorg/xml/sax/SAXException;"),
        other => panic!("expected SAXException for odd UTF-16BE, got {other:?}"),
    }

    // Even-byte valid UTF-16LE `<a/>` still succeeds!
    utf16le_odd.pop();
    parse_bytes_with_driver(&mut vm, handler, &utf16le_odd).unwrap();
}
