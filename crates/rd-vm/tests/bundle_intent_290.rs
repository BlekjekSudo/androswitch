//! Guest-level proofs for issue #290 (Bundle / Intent round):
//! (1) typed getters never leak a mismatched stored value into a register
//!     (`getString` over a boxed Integer is null, `getInt(k, default)`
//!     honors the default on null/mismatch — AOSP's ClassCastException
//!     warning renders as the default);
//! (2) `BaseBundle.dumpStats` survives self-referential Bundles and depth
//!     past the cap with a `<recursion>` marker (no host stack overflow):
//!     the write cap (#261) makes deep values unmeasurable, the walk skips
//!     the size line and keeps indent parity;
//! (3) `Intent.setComponent` and `Intent.setClassName` agree — a class set
//!     by either is observable through `getComponent`;
//! (4) `hasExtra`/`removeExtra(null)` use the same NULL_KEY slot as
//!     `putExtra(null, …)`;
//! (5) `Bundle.deepCopy` copies nested Bundles recursively (AOSP) while
//!     `clone()` stays the shallow copy constructor, and both survive
//!     cyclic graphs;
//! (6) old-format `parseUri` normalizes a relative component class
//!     (leading dot) against the package.

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const BUNDLE: &str = "Landroid/os/Bundle;";
const BASE_BUNDLE: &str = "Landroid/os/BaseBundle;";
const INTENT: &str = "Landroid/content/Intent;";
const COMPONENT: &str = "Landroid/content/ComponentName;";
const WRITER: &str = "Lrd/test/RecordingIndentingPrintWriter;";
const INDENTING_WRITER: &str = "Lcom/android/internal/util/IndentingPrintWriter;";
const ACC_CONSTRUCTOR: u32 = 0x1_0000;
const ACC_PUBLIC: u32 = 0x1;

fn object(v: Value) -> ObjRef {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected object, got {other:?}"),
    }
}

fn s(e: &mut Engine, v: &str) -> Value {
    Value::Obj(rd_vm::intrinsics::alloc_string(e, v.to_string()).unwrap())
}

fn text_of(e: &Engine, v: Value) -> String {
    e.heap.as_str(object(v)).unwrap().to_string()
}

fn inst_text(e: &mut Engine, recv: ObjRef, name: &str, sig: &str) -> String {
    let v = e.invoke_instance(recv, name, sig, &[]).unwrap();
    text_of(e, v)
}

// ---- typed Bundle helpers (each binds the key before the call so the
// borrow of the engine is never nested) --------------------------------------

fn put_int(e: &mut Engine, b: ObjRef, key: &str, v: i32) {
    let k = s(e, key);
    e.invoke_instance(b, "putInt", "(Ljava/lang/String;I)V", &[k, Value::Int(v)])
        .unwrap();
}

fn put_str(e: &mut Engine, b: ObjRef, key: &str, v: Option<&str>) {
    let k = s(e, key);
    let val = v.map(|t| s(e, t)).unwrap_or(Value::Null);
    e.invoke_instance(
        b,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[k, val],
    )
    .unwrap();
}

fn put_bundle(e: &mut Engine, b: ObjRef, key: &str, nested: ObjRef) {
    let k = s(e, key);
    e.invoke_instance(
        b,
        "putBundle",
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        &[k, Value::Obj(nested)],
    )
    .unwrap();
}

fn get_int1(e: &mut Engine, b: ObjRef, key: &str) -> Value {
    let k = s(e, key);
    e.invoke_instance(b, "getInt", "(Ljava/lang/String;)I", &[k])
        .unwrap()
}

fn get_int2(e: &mut Engine, b: ObjRef, key: &str, d: i32) -> Value {
    let k = s(e, key);
    e.invoke_instance(b, "getInt", "(Ljava/lang/String;I)I", &[k, Value::Int(d)])
        .unwrap()
}

fn get_str1(e: &mut Engine, b: ObjRef, key: &str) -> Value {
    let k = s(e, key);
    e.invoke_instance(
        b,
        "getString",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[k],
    )
    .unwrap()
}

fn get_str2(e: &mut Engine, b: ObjRef, key: &str, d: &str) -> Value {
    let k = s(e, key);
    let d = s(e, d);
    e.invoke_instance(
        b,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[k, d],
    )
    .unwrap()
}

fn get_bundle1(e: &mut Engine, b: ObjRef, key: &str) -> Value {
    let k = s(e, key);
    e.invoke_instance(
        b,
        "getBundle",
        "(Ljava/lang/String;)Landroid/os/Bundle;",
        &[k],
    )
    .unwrap()
}

fn clone_shallow(e: &mut Engine, b: ObjRef) -> ObjRef {
    let v = e
        .invoke_instance(b, "clone", "()Ljava/lang/Object;", &[])
        .unwrap();
    object(v)
}

fn deep_copy(e: &mut Engine, b: ObjRef) -> ObjRef {
    let v = e
        .invoke_instance(b, "deepCopy", "()Landroid/os/Bundle;", &[])
        .unwrap();
    object(v)
}

// ---- recording IndentingPrintWriter-shaped sink (same shape as the
// `parcel_binder.rs` dumpStats fixture) --------------------------------------

fn build() -> DexBuilder {
    let mut b = DexBuilder::new();
    let void0 = b.proto_idx("V", vec![]);
    let obj_init = b.method_idx("Ljava/lang/Object;", void0, "<init>");

    let writer = b.class(WRITER, "Ljava/lang/Object;");
    for (name, ftype) in [
        ("indent", "I"),
        ("increases", "I"),
        ("decreases", "I"),
        ("lineCount", "I"),
        ("lastLine", "Ljava/lang/String;"),
    ] {
        b.instance_field(writer, name, ftype);
    }
    let f_indent = b.field_idx(WRITER, "I", "indent");
    let f_increases = b.field_idx(WRITER, "I", "increases");
    let f_decreases = b.field_idx(WRITER, "I", "decreases");
    let f_line_count = b.field_idx(WRITER, "I", "lineCount");
    let f_last_line = b.field_idx(WRITER, "Ljava/lang/String;", "lastLine");

    b.direct(
        writer,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    // increaseIndent(): this=v2, locals v0/v1.
    b.r#virtual(
        writer,
        "increaseIndent",
        INDENTING_WRITER,
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op22c(0x52, 0, 2, f_indent);
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_indent));
            u.extend(op22c(0x52, 0, 2, f_increases));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_increases));
            u.extend(op11x(0x11, 2));
            u
        })),
    );
    // decreaseIndent(): this=v2, locals v0/v1.
    b.r#virtual(
        writer,
        "decreaseIndent",
        INDENTING_WRITER,
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op22c(0x52, 0, 2, f_indent);
            u.extend(op22b(0xd8, 0, 0, -1));
            u.extend(op22c(0x59, 0, 2, f_indent));
            u.extend(op22c(0x52, 0, 2, f_decreases));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_decreases));
            u.extend(op11x(0x11, 2));
            u
        })),
    );
    // println(String;)V: this=v2, line=v3, locals v0/v1.
    b.r#virtual(
        writer,
        "println",
        "V",
        vec!["Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 0, {
            let mut u = op22c(0x5b, 3, 2, f_last_line);
            u.extend(op22c(0x52, 0, 2, f_line_count));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_line_count));
            u.extend(op10x(0x0e));
            u
        })),
    );
    b
}

fn writer_fields(e: &Engine, w: ObjRef) -> (i32, i32, i32, Value) {
    let inc = match e.heap.get_field(w, "increases").unwrap() {
        Value::Int(n) => n,
        other => panic!("increases: {other:?}"),
    };
    let dec = match e.heap.get_field(w, "decreases").unwrap() {
        Value::Int(n) => n,
        other => panic!("decreases: {other:?}"),
    };
    let lines = match e.heap.get_field(w, "lineCount").unwrap() {
        Value::Int(n) => n,
        other => panic!("lineCount: {other:?}"),
    };
    let last = e.heap.get_field(w, "lastLine").unwrap();
    (inc, dec, lines, last)
}

// ---- tests ------------------------------------------------------------------

#[test]
fn typed_getters_never_leak_mismatched_stored_values() {
    let mut e = engine_of(&DexBuilder::new());
    let b = e.new_instance(BUNDLE).unwrap();

    // putInt stores a boxed Integer…
    put_int(&mut e, b, "num", 42);
    // …but getString must NOT hand the raw Integer object to a String
    // register (type corruption): AOSP renders the CCE as null.
    assert_eq!(get_str1(&mut e, b, "num"), Value::Null);
    // The 2-arg form falls back to the supplied default.
    let def_v = get_str2(&mut e, b, "num", "def");
    assert_eq!(text_of(&e, def_v), "def");
    // The positive path is untouched: getInt unboxes the Integer.
    assert_eq!(get_int2(&mut e, b, "num", 9), Value::Int(42));
    // getBundle over the same Integer key is null as well.
    assert_eq!(get_bundle1(&mut e, b, "num"), Value::Null);

    // Reverse mismatch: a stored String under getInt(k, default) yields the
    // default (previously 0), and the 1-arg form yields the typed default.
    put_str(&mut e, b, "txt", Some("hello"));
    assert_eq!(get_int2(&mut e, b, "txt", 7), Value::Int(7));
    assert_eq!(get_int1(&mut e, b, "txt"), Value::Int(0));

    // A stored null value behaves like an absent key for the default form,
    // and getString still reads a real string back (positive control).
    put_str(&mut e, b, "nul", None);
    assert_eq!(get_int2(&mut e, b, "nul", 5), Value::Int(5));
    put_str(&mut e, b, "real", Some("v"));
    let real_v = get_str1(&mut e, b, "real");
    assert_eq!(text_of(&e, real_v), "v");
}

#[test]
fn dump_stats_survives_self_referential_bundle() {
    let b = build();
    let mut e = engine_of(&b);
    let bundle = e.new_instance(BUNDLE).unwrap();
    // A Bundle that contains itself: AOSP's recursion would abort the host.
    // Lazy reference storage (#261) lets the put succeed; serialization of
    // the cycle trips the write cap, so dumpStats must walk it without
    // measuring the unmeasurable value.
    put_bundle(&mut e, bundle, "self", bundle);

    let w = e.new_instance(WRITER).unwrap();
    e.invoke_static(
        BASE_BUNDLE,
        "dumpStats",
        "(Lcom/android/internal/util/IndentingPrintWriter;Landroid/os/BaseBundle;)V",
        &[Value::Obj(w), Value::Obj(bundle)],
    )
    .expect("dumpStats must survive a self-referential Bundle");

    let (inc, dec, _, last) = writer_fields(&e, w);
    assert_eq!(inc, dec, "indent calls must stay balanced");
    // The walk ends on the recursion marker instead of aborting the host.
    assert_eq!(text_of(&e, last), "<recursion>");
}

#[test]
fn dump_stats_caps_deep_nesting() {
    let b = build();
    let mut e = engine_of(&b);
    // A 40-level chain: puts are lazy so the chain builds; dumpStats' size
    // measurement of the deep value trips the write cap and is skipped,
    // the walk prints the marker and unwinds instead of exhausting the
    // host stack.
    let mut cur = e.new_instance(BUNDLE).unwrap();
    put_int(&mut e, cur, "leaf", 1);
    for _ in 0..40 {
        let outer = e.new_instance(BUNDLE).unwrap();
        put_bundle(&mut e, outer, "n", cur);
        cur = outer;
    }
    let w = e.new_instance(WRITER).unwrap();
    e.invoke_static(
        BASE_BUNDLE,
        "dumpStats",
        "(Lcom/android/internal/util/IndentingPrintWriter;Landroid/os/BaseBundle;)V",
        &[Value::Obj(w), Value::Obj(cur)],
    )
    .expect("dumpStats must survive deep nesting");
    let (inc, dec, _, last) = writer_fields(&e, w);
    assert_eq!(inc, dec, "indent calls must stay balanced");
    assert_eq!(text_of(&e, last), "<recursion>");
}

#[test]
fn deep_copy_copies_nested_bundles_and_survives_cycles() {
    let mut e = engine_of(&DexBuilder::new());
    let inner = e.new_instance(BUNDLE).unwrap();
    put_int(&mut e, inner, "x", 1);
    let outer = e.new_instance(BUNDLE).unwrap();
    put_bundle(&mut e, outer, "in", inner);

    // deepCopy: the nested Bundle is a distinct copy — mutating the
    // original inner afterwards must not leak into the copy.
    let cp = deep_copy(&mut e, outer);
    let cp_inner = object(get_bundle1(&mut e, cp, "in"));
    assert_ne!(cp_inner, inner, "deepCopy must copy the nested Bundle");
    put_int(&mut e, inner, "x", 2);
    assert_eq!(get_int1(&mut e, cp_inner, "x"), Value::Int(1));

    // clone(): the shallow copy constructor — the nested Bundle is shared.
    let cl = clone_shallow(&mut e, outer);
    let cl_inner = object(get_bundle1(&mut e, cl, "in"));
    assert_eq!(cl_inner, inner, "clone must keep sharing the nested Bundle");

    // A self-referential deepCopy terminates (the cycle shares the copy).
    let c = e.new_instance(BUNDLE).unwrap();
    put_bundle(&mut e, c, "self", c);
    let ccp = deep_copy(&mut e, c);
    let ccp_inner = object(get_bundle1(&mut e, ccp, "self"));
    let ccp_inner2 = object(get_bundle1(&mut e, ccp_inner, "self"));
    assert_eq!(
        ccp_inner, ccp_inner2,
        "the cycle resolves to the shared copy"
    );
}

#[test]
fn set_component_and_set_class_name_agree_through_get_component() {
    let mut e = engine_of(&DexBuilder::new());
    let make_cn = |e: &mut Engine, pkg: &str, cls: &str| -> ObjRef {
        let cn = e
            .heap
            .alloc_instance(COMPONENT.to_string(), Vec::new())
            .unwrap();
        let p = s(e, pkg);
        let c = s(e, cls);
        e.invoke_instance(
            cn,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[p, c],
        )
        .unwrap();
        cn
    };

    // setComponent keeps exposing the component (regression guard).
    let i1 = e.new_instance(INTENT).unwrap();
    let cn1 = make_cn(&mut e, "com.p", "com.p.A");
    e.invoke_instance(
        i1,
        "setComponent",
        "(Landroid/content/ComponentName;)Landroid/content/Intent;",
        &[Value::Obj(cn1)],
    )
    .unwrap();
    let back = object(
        e.invoke_instance(i1, "getComponent", "()Landroid/content/ComponentName;", &[])
            .unwrap(),
    );
    assert_eq!(
        inst_text(&mut e, back, "getClassName", "()Ljava/lang/String;"),
        "com.p.A"
    );

    // setClassName must install a real ComponentName: getComponent() used to
    // come back null (split-brain with setComponent).
    let i2 = e.new_instance(INTENT).unwrap();
    let pkg_v = s(&mut e, "com.p");
    let cls_v = s(&mut e, "com.p.B");
    e.invoke_instance(
        i2,
        "setClassName",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[pkg_v, cls_v],
    )
    .unwrap();
    let back2 = object(
        e.invoke_instance(i2, "getComponent", "()Landroid/content/ComponentName;", &[])
            .unwrap(),
    );
    assert_eq!(
        inst_text(&mut e, back2, "getClassName", "()Ljava/lang/String;"),
        "com.p.B"
    );
    assert_eq!(
        inst_text(&mut e, back2, "getPackageName", "()Ljava/lang/String;"),
        "com.p"
    );
}

#[test]
fn intent_null_key_uses_the_null_key_slot() {
    let mut e = engine_of(&DexBuilder::new());
    let i = e.new_instance(INTENT).unwrap();

    // putExtra(null, 1) stores under the NULL_KEY sentinel…
    e.invoke_instance(
        i,
        "putExtra",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[Value::Null, Value::Int(1)],
    )
    .unwrap();
    // …and hasExtra(null) must see it (previously probed the "" slot).
    assert_eq!(
        e.invoke_instance(i, "hasExtra", "(Ljava/lang/String;)Z", &[Value::Null])
            .unwrap(),
        Value::Int(1)
    );
    // removeExtra(null) drops that same slot.
    e.invoke_instance(i, "removeExtra", "(Ljava/lang/String;)V", &[Value::Null])
        .unwrap();
    assert_eq!(
        e.invoke_instance(i, "hasExtra", "(Ljava/lang/String;)Z", &[Value::Null])
            .unwrap(),
        Value::Int(0)
    );
    // Named keys keep their own slots (regression guard).
    let key = s(&mut e, "named");
    e.invoke_instance(
        i,
        "putExtra",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[key, Value::Int(3)],
    )
    .unwrap();
    let key2 = s(&mut e, "named");
    assert_eq!(
        e.invoke_instance(i, "hasExtra", "(Ljava/lang/String;)Z", &[key2])
            .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn parse_uri_old_format_normalizes_relative_component_class() {
    let mut e = engine_of(&DexBuilder::new());
    let parse = |e: &mut Engine, uri: &str| -> ObjRef {
        let u = s(e, uri);
        let v = e
            .invoke_static(
                INTENT,
                "parseUri",
                "(Ljava/lang/String;I)Landroid/content/Intent;",
                &[u, Value::Int(0)],
            )
            .unwrap();
        object(v)
    };

    let parsed = parse(&mut e, "intent:#component(com.example.app/.MainActivity)");
    let cn = object(
        e.invoke_instance(
            parsed,
            "getComponent",
            "()Landroid/content/ComponentName;",
            &[],
        )
        .unwrap(),
    );
    // AOSP: the leading-dot class is resolved against the component package.
    assert_eq!(
        inst_text(&mut e, cn, "getClassName", "()Ljava/lang/String;"),
        "com.example.app.MainActivity"
    );
    assert_eq!(
        inst_text(&mut e, cn, "getPackageName", "()Ljava/lang/String;"),
        "com.example.app"
    );

    // A fully qualified class passes through unchanged.
    let parsed2 = parse(
        &mut e,
        "intent:#component(com.example.app/com.example.app.Other)",
    );
    let cn2 = object(
        e.invoke_instance(
            parsed2,
            "getComponent",
            "()Landroid/content/ComponentName;",
            &[],
        )
        .unwrap(),
    );
    assert_eq!(
        inst_text(&mut e, cn2, "getClassName", "()Ljava/lang/String;"),
        "com.example.app.Other"
    );
}
