//! Regression tests for issue #262 (`java.util.regex.Pattern` / `Matcher`):
//! 1. `Matcher.toMatchResult()` returns an immutable snapshot independent of subsequent `find()`/`reset()`/`usePattern()`.
//! 2. `Matcher.matches()` / `lookingAt()` work with `Pattern.COMMENTS` (`#` without trailing `\n`) and at the 64 KiB compilation ceiling.
//! 3. Replacement `expand` rejects trailing `\`, unclosed `${name`, empty `${}`, and nonexistent `${group}` with `IllegalArgumentException`.
//! 4. `null` arguments throw `NullPointerException` (and `usePattern(null)` throws `IllegalArgumentException`).

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const PATTERN: &str = "Ljava/util/regex/Pattern;";
const FLAG_COMMENTS: i32 = 4;

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn s(e: &mut Engine, txt: &str) -> Value {
    Value::Obj(e.heap.alloc_string(txt.to_string()).unwrap())
}

fn as_obj(v: Value) -> u32 {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

fn as_string(e: &Engine, v: Value) -> String {
    e.heap.as_str(as_obj(v)).unwrap().to_string()
}

fn assert_exc(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_class,
            "unexpected exception: {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected_class}, got {other:?}"),
    }
}

#[test]
fn issue_262_to_match_result_preserves_immutable_snapshots_across_find_and_reset() {
    let mut e = engine();
    let pat_str = s(&mut e, "(?<lhs>\\d+)-(?<rhs>[a-z]+)");
    let p = as_obj(
        e.invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[pat_str],
        )
        .unwrap(),
    );
    let inp = s(&mut e, "12-ab 345-xyz");
    let m = as_obj(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[inp],
        )
        .unwrap(),
    );

    let mut snapshots = Vec::new();
    while e
        .invoke_instance(m, "find", "()Z", &[])
        .unwrap()
        .as_int()
        .unwrap()
        != 0
    {
        let mr = as_obj(
            e.invoke_instance(m, "toMatchResult", "()Ljava/util/regex/MatchResult;", &[])
                .unwrap(),
        );
        assert_ne!(mr, m, "toMatchResult() must not alias mutable Matcher");
        snapshots.push(mr);
    }
    assert_eq!(snapshots.len(), 2);

    // Exhausted matcher now throws IllegalStateException on group()...
    assert_exc(
        e.invoke_instance(m, "group", "()Ljava/lang/String;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    // ...even after mutating matcher via reset() and usePattern(), snapshots stay intact!
    let other_inp = s(&mut e, "99-qq");
    e.invoke_instance(
        m,
        "reset",
        "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        &[other_inp],
    )
    .unwrap();
    let other_pat_str = s(&mut e, "z+");
    let p2 = e
        .invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[other_pat_str],
        )
        .unwrap();
    e.invoke_instance(
        m,
        "usePattern",
        "(Ljava/util/regex/Pattern;)Ljava/util/regex/Matcher;",
        &[p2],
    )
    .unwrap();

    let rhs_name = s(&mut e, "rhs");
    let g0 = e
        .invoke_instance(snapshots[0], "group", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(as_string(&e, g0), "12-ab");
    let g1 = e
        .invoke_instance(
            snapshots[0],
            "group",
            "(I)Ljava/lang/String;",
            &[Value::Int(1)],
        )
        .unwrap();
    assert_eq!(as_string(&e, g1), "12");
    let g_rhs = e
        .invoke_instance(
            snapshots[0],
            "group",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[rhs_name],
        )
        .unwrap();
    assert_eq!(as_string(&e, g_rhs), "ab");
    assert_eq!(
        e.invoke_instance(snapshots[0], "start", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        0
    );
    assert_eq!(
        e.invoke_instance(snapshots[0], "end", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        5
    );
    assert_eq!(
        e.invoke_instance(snapshots[0], "groupCount", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        2
    );

    let rhs_name2 = s(&mut e, "rhs");
    let g1_0 = e
        .invoke_instance(snapshots[1], "group", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(as_string(&e, g1_0), "345-xyz");
    let g1_rhs = e
        .invoke_instance(
            snapshots[1],
            "group",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[rhs_name2],
        )
        .unwrap();
    assert_eq!(as_string(&e, g1_rhs), "xyz");
    assert_eq!(
        e.invoke_instance(snapshots[1], "start", "(I)I", &[Value::Int(1)])
            .unwrap()
            .as_int()
            .unwrap(),
        6
    );
    assert_eq!(
        e.invoke_instance(snapshots[1], "end", "(I)I", &[Value::Int(2)])
            .unwrap()
            .as_int()
            .unwrap(),
        13
    );
}

#[test]
fn issue_262_comments_mode_without_newline_and_64kib_ceiling_work_in_matches_and_looking_at() {
    let mut e = engine();
    let pat_str = s(&mut e, "abc # trailing comment without newline");
    let p = as_obj(
        e.invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;I)Ljava/util/regex/Pattern;",
            &[pat_str, Value::Int(FLAG_COMMENTS)],
        )
        .unwrap(),
    );

    let exact = s(&mut e, "abc");
    let m1 = as_obj(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[exact],
        )
        .unwrap(),
    );
    assert_eq!(
        e.invoke_instance(m1, "matches", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );

    let prefix = s(&mut e, "abcdef");
    let m2 = as_obj(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[prefix],
        )
        .unwrap(),
    );
    assert_eq!(
        e.invoke_instance(m2, "matches", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        0
    );
    assert_eq!(
        e.invoke_instance(m2, "lookingAt", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );

    // Inline `(?x)` via `Pattern.matches` also works without trailing `\n`.
    let inline_x = s(&mut e, "(?x)foo # inline trailing comment");
    let foo = s(&mut e, "foo");
    assert_eq!(
        e.invoke_static(
            PATTERN,
            "matches",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Z",
            &[inline_x, foo],
        )
        .unwrap()
        .as_int()
        .unwrap(),
        1
    );

    // Exactly 64 KiB (`65_536` bytes) pattern compiled via Pattern.compile must not
    // fail the 64 KiB guard when wrapped by matches() / lookingAt().
    let mut near_cap = String::from("ok #");
    near_cap.push_str(&"x".repeat(64 * 1024 - near_cap.len()));
    assert_eq!(near_cap.len(), 64 * 1024);
    let near_cap_val = s(&mut e, &near_cap);
    let p_cap = as_obj(
        e.invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;I)Ljava/util/regex/Pattern;",
            &[near_cap_val, Value::Int(FLAG_COMMENTS)],
        )
        .unwrap(),
    );
    let ok_in = s(&mut e, "ok");
    let m_cap = as_obj(
        e.invoke_instance(
            p_cap,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[ok_in],
        )
        .unwrap(),
    );
    assert_eq!(
        e.invoke_instance(m_cap, "matches", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );
    assert_eq!(
        e.invoke_instance(m_cap, "lookingAt", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );
}

#[test]
fn issue_262_expand_rejects_trailing_backslash_unclosed_or_unknown_named_groups() {
    let mut e = engine();
    let pat_str = s(&mut e, "(?<opt>x)?(?<word>[a-z]+)");
    let p = as_obj(
        e.invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[pat_str],
        )
        .unwrap(),
    );
    let inp = s(&mut e, "abc");
    let m = as_obj(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[inp],
        )
        .unwrap(),
    );

    for bad in ["\\", "ok\\", "${unclosed", "${}", "${missing}"] {
        let rep = s(&mut e, bad);
        assert_exc(
            e.invoke_instance(
                m,
                "replaceAll",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[rep],
            ),
            "Ljava/lang/IllegalArgumentException;",
        );
    }

    // Valid optional named group `${opt}` that did not participate expands to `""`, not an error.
    let valid_rep = s(&mut e, "[${opt}:${word}]");
    let replaced = e
        .invoke_instance(
            m,
            "replaceAll",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[valid_rep],
        )
        .unwrap();
    assert_eq!(as_string(&e, replaced), "[:abc]");
}

#[test]
fn issue_262_null_arguments_throw_npe_and_use_pattern_null_throws_iae() {
    let mut e = engine();

    assert_exc(
        e.invoke_static(
            PATTERN,
            "quote",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let valid_re = s(&mut e, "a");
    let valid_in = s(&mut e, "a");
    assert_exc(
        e.invoke_static(
            PATTERN,
            "matches",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Z",
            &[Value::Null, valid_in],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_static(
            PATTERN,
            "matches",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Z",
            &[valid_re, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_static(
            PATTERN,
            "matches",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Z",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let pat_str = s(&mut e, "a");
    let p = as_obj(
        e.invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[pat_str],
        )
        .unwrap(),
    );
    assert_exc(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            p,
            "split",
            "(Ljava/lang/CharSequence;)[Ljava/lang/String;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let inp = s(&mut e, "a");
    let m = as_obj(
        e.invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[inp],
        )
        .unwrap(),
    );
    assert_exc(
        e.invoke_instance(
            m,
            "replaceAll",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            m,
            "replaceFirst",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            m,
            "reset",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            m,
            "usePattern",
            "(Ljava/util/regex/Pattern;)Ljava/util/regex/Matcher;",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Zero-arg `reset()` still works without NPE, then `find()` + `appendReplacement(sb, null)` throws NPE.
    e.invoke_instance(m, "reset", "()Ljava/util/regex/Matcher;", &[])
        .unwrap();
    assert_eq!(
        e.invoke_instance(m, "find", "()Z", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );
    let sb = Value::Obj(
        e.heap
            .alloc_instance("Ljava/lang/StringBuilder;".to_string(), Vec::new())
            .unwrap(),
    );
    assert_exc(
        e.invoke_instance(
            m,
            "appendReplacement",
            "(Ljava/lang/StringBuilder;Ljava/lang/String;)Ljava/util/regex/Matcher;",
            &[sb, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
}
