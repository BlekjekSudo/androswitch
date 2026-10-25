use rd_vm::{android_log, Engine, ObjRef, Value, VmConfig, VmExit};
fn render(vm: &mut Engine, reference: ObjRef) -> Result<String, VmExit> {
    vm.gc();
    Ok(vm.heap.as_str(reference)?.to_string())
}
fn string(vm: &mut Engine, text: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(text.into()).unwrap())
}
fn vm() -> Engine {
    Engine::new(vec![], VmConfig::default())
}
const CLASS: &str = "Landroid/util/Log;";
const TWO: &str = "(Ljava/lang/String;Ljava/lang/String;)I";
const THREE: &str = "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/Throwable;)I";

#[test]
fn shared_levels_overloads_and_policy_emit_actual_utf8_bytes() {
    for (name, level) in [
        ("v", "V"),
        ("d", "D"),
        ("i", "I"),
        ("w", "W"),
        ("e", "E"),
        ("wtf", "A"),
    ] {
        for signature in [TWO, THREE] {
            let mut vm = vm();
            let mut args = vec![string(&mut vm, "tag"), string(&mut vm, "héllo")];
            if signature == THREE {
                args.push(string(&mut vm, "throwable summary"));
            }
            let expected = format!(
                "{level}/tag: héllo{}\n",
                if signature == THREE {
                    "\nthrowable summary"
                } else {
                    ""
                }
            );
            let mut lines = Vec::new();
            let value = android_log::call(
                &mut vm,
                CLASS,
                name,
                signature,
                &args,
                render,
                |p| p >= 3,
                |line| {
                    lines.push(line.to_string());
                    Ok(line.len())
                },
            )
            .unwrap();
            assert_eq!(lines, vec![expected.clone()]);
            assert_eq!(value, Some(Value::Int(expected.len() as i32)));
        }
    }
    let mut vm = vm();
    for level in 2..=7 {
        let args = [Value::Null, Value::Int(level)];
        let result = android_log::call(
            &mut vm,
            CLASS,
            "isLoggable",
            "(Ljava/lang/String;I)Z",
            &args,
            render,
            |p| p >= 4,
            |_| panic!("policy must not emit"),
        )
        .unwrap();
        assert_eq!(result, Some(Value::Int(i32::from(level >= 4))));
    }
}
#[test]
fn shared_callback_gc_roots_exact_arguments_then_releases_them() {
    let mut vm = vm();
    // The inherited collector conservatively scans integer side-table fields;
    // low IDs can alias values such as the default thread ID. Give this probe
    // ordinary higher IDs so it can measure host-pin lifetime on both paths.
    for _ in 0..256 {
        string(&mut vm, "padding");
    }
    let unused = string(&mut vm, "unrelated").as_ref().unwrap().unwrap();
    let args = [
        string(&mut vm, "tag"),
        string(&mut vm, "message"),
        string(&mut vm, "exception"),
    ];
    android_log::call(
        &mut vm,
        CLASS,
        "w",
        THREE,
        &args,
        render,
        |_| true,
        |line| {
            assert_eq!(line, "W/tag: message\nexception\n");
            Ok(line.len())
        },
    )
    .unwrap();
    assert_eq!(vm.heap.class_of(unused).unwrap(), rd_vm::gc::FREED_CLASS);
    vm.gc();
    for value in args {
        assert_eq!(
            vm.heap.class_of(value.as_ref().unwrap().unwrap()).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
}
#[test]
fn shared_log_sink_failures_and_entry_budget_do_not_claim_success() {
    let mut vm = vm();
    let args = [string(&mut vm, "tag"), string(&mut vm, "message")];
    let failure = android_log::call(
        &mut vm,
        CLASS,
        "e",
        TWO,
        &args,
        render,
        |_| true,
        |_| Err(rd_vm::err::vm_error("TEST_IO_FAILURE", "disk failure").into()),
    );
    assert!(matches!(failure,Err(VmExit::Error(ref e)) if e.code=="TEST_IO_FAILURE"));
    let failure = android_log::call(&mut vm, CLASS, "e", TWO, &args, render, |_| true, |_| Ok(0));
    assert!(matches!(failure,Err(VmExit::Error(ref e)) if e.code=="SHORT_WRITE"));
    let args = [Value::Null, string(&mut vm, &"x".repeat(65536))];
    let failure = android_log::call(
        &mut vm,
        CLASS,
        "e",
        TWO,
        &args,
        render,
        |_| true,
        |_| panic!("over budget must not write"),
    );
    assert!(matches!(failure,Err(VmExit::Error(ref e)) if e.code=="LOG_BUDGET"));
}
#[test]
fn shared_log_println_nulls_and_catalog_remain_descriptor_checked() {
    let mut vm = vm();
    let failure = android_log::call(
        &mut vm,
        CLASS,
        "w",
        THREE,
        &[],
        render,
        |_| true,
        |_| panic!("invalid arguments must not emit"),
    );
    assert!(matches!(failure,Err(VmExit::Error(ref e)) if e.code=="VM_TYPE_ERROR"));
    let args = [Value::Int(5), Value::Null, Value::Null];
    assert_eq!(
        android_log::call(
            &mut vm,
            CLASS,
            "println",
            "(ILjava/lang/String;Ljava/lang/String;)I",
            &args,
            render,
            |_| true,
            |line| {
                assert_eq!(line, "W/: \n");
                Ok(line.len())
            }
        )
        .unwrap(),
        Some(Value::Int(5))
    );
    for (name, sig) in [
        ("w", "()I"),
        ("d", "(Ljava/lang/String;Ljava/lang/Throwable;)I"),
        (
            "getStackTraceString",
            "(Ljava/lang/Throwable;)Ljava/lang/String;",
        ),
    ] {
        assert!(!android_log::metadata(CLASS, name, sig));
    }
}
