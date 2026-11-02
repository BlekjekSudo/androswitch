//! Regression tests for issue #367 (`crates/rd-vm/src/android_uri.rs`): the
//! text of `Uri` objects and of `Uri.Builder` contents is charged to the guest
//! heap budget, so a guest loop cannot grow host memory without limit.

use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_static_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const URI: &str = "Landroid/net/Uri;";
const BUILDER_SIG: &str = "(Ljava/lang/String;)Landroid/net/Uri$Builder;";
const BUDGET: usize = 8 * 1024 * 1024;
const CHUNK: usize = 256 * 1024;
const MAX_ITERATIONS: usize = 64;

fn engine() -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget: BUDGET,
            ..VmConfig::default()
        },
    )
}

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn parse(e: &mut Engine, text: &str) -> ObjRef {
    let arg = jstr(e, text);
    match call_static_intrinsic(
        e,
        URI,
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[arg],
    )
    .expect("parse")
    .expect("handled")
    {
        Value::Obj(r) => r,
        other => panic!("expected Uri, got {other:?}"),
    }
}

fn is_oome(e: &VmExit) -> bool {
    matches!(e, VmExit::Exception(t) if t.class == "Ljava/lang/OutOfMemoryError;")
}

fn text_of(vm: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => vm.heap.as_str(*r).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn issue_367_uri_parse_text_is_charged_to_heap_budget() {
    let mut e = engine();
    let big = "a".repeat(CHUNK);
    let mut hit_oome = false;
    for _ in 0..MAX_ITERATIONS {
        let arg = jstr(&mut e, &format!("https://ex.com/{big}"));
        match call_static_intrinsic(
            &mut e,
            URI,
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[arg],
        ) {
            Ok(_) => {}
            Err(err) => {
                assert!(is_oome(&err), "expected OutOfMemoryError, got {err:?}");
                hit_oome = true;
                break;
            }
        }
    }
    assert!(
        hit_oome,
        "{MAX_ITERATIONS} Uri.parse calls of {CHUNK} bytes never hit the {BUDGET}-byte budget"
    );
}

#[test]
fn issue_367_builder_content_is_charged_to_heap_budget() {
    let mut e = engine();
    let uri = parse(&mut e, "https://ex.com");
    let builder = match e
        .invoke_instance(uri, "buildUpon", "()Landroid/net/Uri$Builder;", &[])
        .expect("buildUpon")
    {
        Value::Obj(r) => r,
        other => panic!("expected Builder, got {other:?}"),
    };
    let big = "b".repeat(CHUNK);
    let mut hit_oome = false;
    for _ in 0..MAX_ITERATIONS {
        let seg = jstr(&mut e, &big);
        match e.invoke_instance(builder, "appendEncodedPath", BUILDER_SIG, &[seg]) {
            Ok(_) => {}
            Err(err) => {
                assert!(is_oome(&err), "expected OutOfMemoryError, got {err:?}");
                hit_oome = true;
                break;
            }
        }
    }
    assert!(
        hit_oome,
        "{MAX_ITERATIONS} builder appends of {CHUNK} bytes never hit the budget"
    );
}

#[test]
fn issue_367_small_uri_and_builder_still_work() {
    let mut e = engine();
    let uri = parse(&mut e, "https://ex.com/a?x=1");
    let s = e
        .invoke_instance(uri, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(text_of(&e, &s), "https://ex.com/a?x=1");
    let builder = match e
        .invoke_instance(uri, "buildUpon", "()Landroid/net/Uri$Builder;", &[])
        .expect("buildUpon")
    {
        Value::Obj(r) => r,
        other => panic!("expected Builder, got {other:?}"),
    };
    let seg = jstr(&mut e, "p");
    e.invoke_instance(builder, "appendEncodedPath", BUILDER_SIG, &[seg])
        .expect("appendEncodedPath");
    let built = e
        .invoke_instance(builder, "build", "()Landroid/net/Uri;", &[])
        .expect("build");
    let Value::Obj(b_ref) = built else {
        panic!("expected Uri")
    };
    let text = e
        .invoke_instance(b_ref, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(text_of(&e, &text), "https://ex.com/a/p?x=1");
}
