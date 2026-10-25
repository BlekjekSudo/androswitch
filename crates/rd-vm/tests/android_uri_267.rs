//! Regression tests for Issue #267: `android_uri.rs` UTF-8 safe byte-nibble
//! percent-decoding, `Uri.withAppendedPath` query/fragment preservation + NPE,
//! `Uri.fromParts` NPE contracts, and Unicode `allow` characters in `Uri.encode`.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_static_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const URI: &str = "Landroid/net/Uri;";

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn str_of(vm: &Engine, v: Value) -> String {
    let Value::Obj(r) = v else {
        panic!("expected String ObjRef, got {v:?}");
    };
    vm.heap.as_str(r).expect("as_str").to_string()
}

fn parse_uri(vm: &mut Engine, s: &str) -> ObjRef {
    let arg = jstr(vm, s);
    let out = call_static_intrinsic(
        vm,
        URI,
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[arg],
    )
    .expect("Uri.parse ok")
    .expect("Uri.parse handled");
    let Value::Obj(r) = out else {
        panic!("expected Uri ObjRef");
    };
    r
}

fn assert_static_npe(res: Result<Option<Value>, VmExit>) {
    match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, "Ljava/lang/NullPointerException;",
                "expected NullPointerException, got {} ({:?})",
                t.class, t.message
            );
        }
        other => panic!("expected NullPointerException, got {other:?}"),
    }
}

#[test]
fn uri_decode_and_getters_never_panic_on_percent_before_multibyte_utf8() {
    let mut vm = engine_of(&DexBuilder::new());

    for (raw, expected) in [
        ("%😀", "%😀"),
        ("%1é", "%1é"),
        ("%é1", "%é1"),
        ("%", "%"),
        ("%A", "%A"),
        ("%F0%9F%98%80", "😀"),
        ("a%20b%😀%C3%A9", "a b%😀é"),
    ] {
        let arg = jstr(&mut vm, raw);
        let decoded = call_static_intrinsic(
            &mut vm,
            URI,
            "decode",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[arg],
        )
        .expect("Uri.decode must not panic")
        .expect("Uri.decode handled");
        assert_eq!(str_of(&vm, decoded), expected, "input={raw:?}");
    }

    // Exercise getPath, getQueryParameter, getFragment, and getPathSegments on multibyte `%` escapes.
    let u = parse_uri(&mut vm, "https://ex.com/dir/%😀?q=%1é+ok#frag%😀");
    let path = vm
        .invoke_instance(u, "getPath", "()Ljava/lang/String;", &[])
        .expect("getPath");
    assert_eq!(str_of(&vm, path), "/dir/%😀");

    let q_key = jstr(&mut vm, "q");
    let q_val = vm
        .invoke_instance(
            u,
            "getQueryParameter",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[q_key],
        )
        .expect("getQueryParameter");
    assert_eq!(str_of(&vm, q_val), "%1é ok");

    let frag = vm
        .invoke_instance(u, "getFragment", "()Ljava/lang/String;", &[])
        .expect("getFragment");
    assert_eq!(str_of(&vm, frag), "frag%😀");
}

#[test]
fn with_appended_path_preserves_query_and_fragment_and_enforces_npe() {
    let mut vm = engine_of(&DexBuilder::new());

    let base = parse_uri(&mut vm, "content://auth/items?limit=10#top");
    let seg = jstr(&mut vm, "42");
    let out = call_static_intrinsic(
        &mut vm,
        URI,
        "withAppendedPath",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
        &[Value::Obj(base), seg],
    )
    .expect("withAppendedPath ok")
    .expect("withAppendedPath handled");
    let Value::Obj(appended) = out else {
        panic!("expected Uri");
    };

    let full = vm
        .invoke_instance(appended, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(str_of(&vm, full), "content://auth/items/42?limit=10#top");

    let path = vm
        .invoke_instance(appended, "getPath", "()Ljava/lang/String;", &[])
        .expect("getPath");
    assert_eq!(str_of(&vm, path), "/items/42");

    let limit_k = jstr(&mut vm, "limit");
    let limit_v = vm
        .invoke_instance(
            appended,
            "getQueryParameter",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[limit_k],
        )
        .expect("getQueryParameter");
    assert_eq!(str_of(&vm, limit_v), "10");

    let frag = vm
        .invoke_instance(appended, "getFragment", "()Ljava/lang/String;", &[])
        .expect("getFragment");
    assert_eq!(str_of(&vm, frag), "top");

    // Trailing slash on base + leading slash on segment must join with a single `/`.
    let base_slash = parse_uri(&mut vm, "content://auth/items/?a=1");
    let seg_slash = jstr(&mut vm, "/42");
    let Value::Obj(joined) = call_static_intrinsic(
        &mut vm,
        URI,
        "withAppendedPath",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
        &[Value::Obj(base_slash), seg_slash],
    )
    .expect("withAppendedPath slash ok")
    .expect("handled") else {
        panic!("expected Uri");
    };
    let joined_s = vm
        .invoke_instance(joined, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(str_of(&vm, joined_s), "content://auth/items/42?a=1");

    // Null baseUri or null pathSegment must throw NullPointerException.
    let seg_ok = jstr(&mut vm, "42");
    assert_static_npe(call_static_intrinsic(
        &mut vm,
        URI,
        "withAppendedPath",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
        &[Value::Null, seg_ok],
    ));
    assert_static_npe(call_static_intrinsic(
        &mut vm,
        URI,
        "withAppendedPath",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
        &[Value::Obj(base), Value::Null],
    ));
}

#[test]
fn from_parts_null_checks_and_encode_unicode_allow_match_aosp() {
    let mut vm = engine_of(&DexBuilder::new());

    let mailto = jstr(&mut vm, "mailto");
    let ssp = jstr(&mut vm, "user@example.com");

    // Null scheme -> NPE; null ssp -> NPE; null fragment -> allowed.
    assert_static_npe(call_static_intrinsic(
        &mut vm,
        URI,
        "fromParts",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
        &[Value::Null, ssp.clone(), Value::Null],
    ));
    assert_static_npe(call_static_intrinsic(
        &mut vm,
        URI,
        "fromParts",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
        &[mailto.clone(), Value::Null, Value::Null],
    ));

    let Value::Obj(ok_uri) = call_static_intrinsic(
        &mut vm,
        URI,
        "fromParts",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
        &[mailto, ssp, Value::Null],
    )
    .expect("fromParts valid")
    .expect("handled") else {
        panic!("expected Uri");
    };
    let ok_s = vm
        .invoke_instance(ok_uri, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(str_of(&vm, ok_s), "mailto:user%40example.com");

    // Uri.encode(s, allow) preserves Unicode characters explicitly listed in `allow`.
    let raw = jstr(&mut vm, "a/é/😀");
    let allow = jstr(&mut vm, "/é");
    let encoded = call_static_intrinsic(
        &mut vm,
        URI,
        "encode",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[raw, allow],
    )
    .expect("Uri.encode ok")
    .expect("handled");
    assert_eq!(str_of(&vm, encoded), "a/é/%F0%9F%98%80");
}
