//! Regression tests for issue #374 (`crates/rd-vm/src/jregex.rs`):
//! 1. `Matcher.replaceAll` / `replaceFirst` respect the heap budget before building the host string.
//! 2. `Matcher.quoteReplacement(String)` exists and escapes `\` and `$`; `null` throws NPE.
//! 3. `Matcher.appendReplacement` expands the groups stored by the last match
//!    (`a|ab` on "ab" must keep "ab", not re-match "a").

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const PATTERN: &str = "Ljava/util/regex/Pattern;";
const MATCHER: &str = "Ljava/util/regex/Matcher;";

fn engine(heap_budget: usize) -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    )
}

fn s(e: &mut Engine, txt: &str) -> Value {
    Value::Obj(e.heap.alloc_string(txt.to_string()).unwrap())
}

fn obj(v: Value) -> u32 {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

fn text(e: &Engine, v: Value) -> String {
    e.heap.as_str(obj(v)).unwrap().to_string()
}

fn exception_class(res: Result<Value, VmExit>) -> String {
    match res {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("expected an exception, got {other:?}"),
    }
}

fn matcher(e: &mut Engine, regex: &str, input: &str) -> u32 {
    let src = s(e, regex);
    let p = obj(e
        .invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[src],
        )
        .unwrap());
    let inp = s(e, input);
    obj(e
        .invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[inp],
        )
        .unwrap())
}

#[test]
fn issue_374_replace_all_over_budget_throws_oome_before_building_the_string() {
    // 2 MiB heap: 2000 matches of a 2000-char replacement is a 4 MB host string.
    let mut e = engine(2 * 1024 * 1024);
    let m = matcher(&mut e, ".", &"x".repeat(2000));
    let rep = s(&mut e, &"y".repeat(2000));
    let res = e.invoke_instance(
        m,
        "replaceAll",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[rep],
    );
    assert_eq!(exception_class(res), "Ljava/lang/OutOfMemoryError;");
}

#[test]
fn issue_374_replace_all_small_result_still_expands_groups() {
    let mut e = engine(64 * 1024 * 1024);
    let m = matcher(&mut e, r"(\d)", "a1b2");
    let rep = s(&mut e, "<$1>");
    let out = e
        .invoke_instance(
            m,
            "replaceAll",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[rep],
        )
        .unwrap();
    assert_eq!(text(&e, out), "a<1>b<2>");
}

#[test]
fn issue_374_quote_replacement_escapes_backslash_and_dollar() {
    let mut e = engine(64 * 1024 * 1024);
    let input = s(&mut e, r"a$b\c");
    let out = e
        .invoke_static(
            MATCHER,
            "quoteReplacement",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[input],
        )
        .unwrap();
    assert_eq!(text(&e, out), r"a\$b\\c");
}

#[test]
fn issue_374_quote_replacement_null_throws_npe() {
    let mut e = engine(64 * 1024 * 1024);
    let res = e.invoke_static(
        MATCHER,
        "quoteReplacement",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Value::Null],
    );
    assert_eq!(exception_class(res), "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_374_append_replacement_uses_stored_groups_not_a_new_search() {
    let mut e = engine(64 * 1024 * 1024);
    // `a|ab` on "ab": matches() stores group 0 = "ab". A new unanchored
    // search would find "a" instead.
    let m = matcher(&mut e, "a|ab", "ab");
    assert_eq!(
        e.invoke_instance(m, "matches", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    let sb = e.new_instance("Ljava/lang/StringBuilder;").unwrap();
    let rep = s(&mut e, "[$0]");
    e.invoke_instance(
        m,
        "appendReplacement",
        "(Ljava/lang/StringBuilder;Ljava/lang/String;)Ljava/util/regex/Matcher;",
        &[Value::Obj(sb), rep],
    )
    .unwrap();
    let out = e
        .invoke_instance(sb, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(text(&e, out), "[ab]");
}
