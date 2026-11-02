//! Regression tests for issue #367 (`crates/rd-vm/src/android_uri.rs`):
//! 1. `Uri.getPort()` does not read a port from an unclosed IPv6 `[`, and only
//!    accepts digits (no sign).
//! 2. `Uri.Builder.appendQueryParameter(key, value)` throws `NullPointerException`
//!    for a `null` key or value (AOSP `Objects.requireNonNull`).

use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_static_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const URI: &str = "Landroid/net/Uri;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
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
    match out {
        Value::Obj(r) => r,
        other => panic!("expected Uri ObjRef, got {other:?}"),
    }
}

fn port_of(vm: &mut Engine, s: &str) -> i32 {
    let uri = parse_uri(vm, s);
    match vm
        .invoke_instance(uri, "getPort", "()I", &[])
        .expect("getPort")
    {
        Value::Int(p) => p,
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn issue_367_get_port_reads_explicit_and_ipv6_ports() {
    let mut vm = engine();
    assert_eq!(port_of(&mut vm, "http://host:80"), 80);
    assert_eq!(port_of(&mut vm, "http://[::1]:8080"), 8080);
    assert_eq!(port_of(&mut vm, "http://[::1]"), -1);
}

#[test]
fn issue_367_get_port_unclosed_ipv6_bracket_has_no_port() {
    // Before the fix this returned 1 (the last hextet of "[::1").
    let mut vm = engine();
    assert_eq!(port_of(&mut vm, "http://[::1"), -1);
}

#[test]
fn issue_367_get_port_rejects_signed_port() {
    let mut vm = engine();
    assert_eq!(port_of(&mut vm, "http://host:+80"), -1);
}

#[test]
fn issue_367_append_query_parameter_null_key_or_value_throws_npe() {
    let mut vm = engine();
    let uri = parse_uri(&mut vm, "http://host/path");
    let builder = match vm
        .invoke_instance(uri, "buildUpon", "()Landroid/net/Uri$Builder;", &[])
        .expect("buildUpon")
    {
        Value::Obj(r) => r,
        other => panic!("expected Builder, got {other:?}"),
    };
    let sig = "(Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri$Builder;";
    let v = jstr(&mut vm, "v");
    let null_key = vm.invoke_instance(builder, "appendQueryParameter", sig, &[Value::Null, v]);
    match null_key {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("null key must throw NPE, got {other:?}"),
    }
    let k = jstr(&mut vm, "k");
    let null_value = vm.invoke_instance(builder, "appendQueryParameter", sig, &[k, Value::Null]);
    match null_value {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("null value must throw NPE, got {other:?}"),
    }
}
