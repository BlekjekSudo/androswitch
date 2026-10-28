//! StringTokenizer on the shipping VM, without the reference framework.
use rd_vm::{jstring_tokenizer::CLASS, Engine, Value, VmConfig};
fn call(vm: &mut Engine, owner: u32, name: &str, sig: &str, args: &[Value]) -> Value {
    vm.invoke_instance(owner, name, sig, args).unwrap()
}
fn text(vm: &Engine, value: Value) -> String {
    vm.heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
#[test]
fn inherited_returned_delimiters_enumeration_and_delimiter_change_contract() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    let owner = vm.heap.alloc_instance(CLASS.into(), vec![]).unwrap();
    call(
        &mut vm,
        owner,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;Z)V",
        &[
            Value::StrPlaceholder("1,2-4".into()),
            Value::StrPlaceholder(",;-".into()),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(&mut vm, owner, "countTokens", "()I", &[]),
        Value::Int(5)
    );
    for (method, sig, expected) in [
        ("nextToken", "()Ljava/lang/String;", "1"),
        ("nextToken", "()Ljava/lang/String;", ","),
        ("nextElement", "()Ljava/lang/Object;", "2"),
        ("nextToken", "()Ljava/lang/String;", "-"),
        ("nextToken", "()Ljava/lang/String;", "4"),
    ] {
        let value = call(&mut vm, owner, method, sig, &[]);
        assert_eq!(text(&vm, value), expected);
    }
    assert_eq!(
        call(&mut vm, owner, "hasMoreElements", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut vm, owner, "countTokens", "()I", &[]),
        Value::Int(0)
    );
    call(
        &mut vm,
        owner,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[
            Value::StrPlaceholder("a,b;c".into()),
            Value::StrPlaceholder(",".into()),
        ],
    );
    let first = call(&mut vm, owner, "nextToken", "()Ljava/lang/String;", &[]);
    assert_eq!(text(&vm, first), "a");
    let rest = call(
        &mut vm,
        owner,
        "nextToken",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Value::StrPlaceholder("|".into())],
    );
    assert_eq!(text(&vm, rest), ",b;c");
}

#[test]
fn exact_metadata_admits_all_nine_declarations_but_no_invented_overloads() {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance};
    let rows = [
        ("<init>", "(Ljava/lang/String;)V", Constructor),
        (
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            Constructor,
        ),
        (
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Z)V",
            Constructor,
        ),
        ("hasMoreTokens", "()Z", Instance),
        ("hasMoreElements", "()Z", Instance),
        ("countTokens", "()I", Instance),
        ("nextToken", "()Ljava/lang/String;", Instance),
        (
            "nextToken",
            "(Ljava/lang/String;)Ljava/lang/String;",
            Instance,
        ),
        ("nextElement", "()Ljava/lang/Object;", Instance),
    ];
    for (name, sig, kind) in rows {
        assert_eq!(
            rd_vm::jstring_tokenizer::method_metadata(CLASS, name, sig),
            Some(kind)
        );
    }
    for (class, name, sig) in [
        (CLASS, "nextToken", "()Ljava/lang/Object;"),
        (CLASS, "countTokens", "()J"),
        ("Ljava/util/Enumeration;", "countTokens", "()I"),
    ] {
        assert_eq!(
            rd_vm::jstring_tokenizer::method_metadata(class, name, sig),
            None
        );
    }
}
