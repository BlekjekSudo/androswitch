//! Shared String/Pattern/Matcher lookaround contracts, including UTF-16.
use rd_vm::{Engine, Value, VmConfig};
fn s(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.into()).unwrap())
}
fn obj(v: Value) -> u32 {
    if let Value::Obj(r) = v {
        r
    } else {
        panic!("{v:?}")
    }
}
fn matcher(e: &mut Engine, pattern: &str, input: &str) -> u32 {
    let p = s(e, pattern);
    let p = obj(e
        .invoke_static(
            "Ljava/util/regex/Pattern;",
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[p],
        )
        .unwrap());
    let input = s(e, input);
    obj(e
        .invoke_instance(
            p,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            &[input],
        )
        .unwrap())
}
#[test]
fn positive_negative_lookahead_and_lookbehind() {
    let mut e = Engine::new(vec![], VmConfig::default());
    for (pattern, input, expected) in [
        (r"\d+(?=\s*kg)", "2 kg de arroz", "2"),
        (r"(?<=de )\w+", "2 kg de arroz", "arroz"),
        (r"\b\w+\b(?! kg)", "2 kg de arroz", "kg"),
        (r"(?<!\d)kg", "2kg kg", "kg"),
        (r"(?<unit>kg)(?= de)", "2 kg de arroz", "kg"),
    ] {
        let m = matcher(&mut e, pattern, input);
        assert_eq!(
            e.invoke_instance(m, "find", "()Z", &[]).unwrap(),
            Value::Int(1),
            "{pattern}"
        );
        let group = obj(e
            .invoke_instance(m, "group", "()Ljava/lang/String;", &[])
            .unwrap());
        assert_eq!(e.heap.as_str(group).unwrap(), expected);
    }
}
#[test]
fn utf16_offsets_repeated_find_and_reset() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let m = matcher(&mut e, r"(?<=😀)a", "😀a 😀a");
    for start in [2, 6] {
        assert_eq!(
            e.invoke_instance(m, "find", "()Z", &[]).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            e.invoke_instance(m, "start", "()I", &[]).unwrap(),
            Value::Int(start)
        );
    }
    assert_eq!(
        e.invoke_instance(m, "find", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(m, "reset", "()Ljava/util/regex/Matcher;", &[])
        .unwrap();
    assert_eq!(
        e.invoke_instance(m, "find", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
}
#[test]
fn string_replacement_and_whole_match_use_same_engine() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let text = obj(s(&mut e, "2 kg de arroz"));
    let pattern = s(&mut e, r"(?<=\d) (?=kg)");
    let replacement = s(&mut e, "");
    let result = obj(e
        .invoke_instance(
            text,
            "replaceAll",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[pattern, replacement],
        )
        .unwrap());
    assert_eq!(e.heap.as_str(result).unwrap(), "2kg de arroz");
    let text = obj(s(&mut e, "ab"));
    let pattern = s(&mut e, "a|ab");
    assert_eq!(
        e.invoke_instance(text, "matches", "(Ljava/lang/String;)Z", &[pattern])
            .unwrap(),
        Value::Int(1)
    );
}
#[test]
fn pathological_backtracking_is_a_structured_error_not_false() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let m = matcher(&mut e, r"(?=a)(a+)+b", &"a".repeat(2000));
    let err = e.invoke_instance(m, "find", "()Z", &[]).unwrap_err();
    assert!(
        matches!(err, rd_vm::err::VmExit::Error(ref err) if err.code == "REGEX_RUNTIME"),
        "{err:?}"
    );
}
