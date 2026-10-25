//! Normal SDK DEX against API26 source-derived selection and JDK UTF16 inputs.
//! This harness does not claim an Android reference OS or an IME backend.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{
    framework_host::{FrameworkHost, HostMethodKind},
    gc::FREED_CLASS,
    ObjRef, Value, VmExit,
};
use std::{path::PathBuf, rc::Rc};
fn output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/input-connection-fixture")
}
fn namespace(variant: &str) -> &'static str {
    if variant == "renamed" {
        "net/example/editorcontracts"
    } else {
        "org/androswitch/inputcontracts"
    }
}
fn probe(variant: &str) -> String {
    format!("L{}/InputCases;", namespace(variant))
}
#[allow(clippy::assertions_on_constants)]
fn app(variant: &str) -> (App, PathBuf) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-input-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let record = pm
        .install(
            std::fs::read(output().join(format!("{variant}.apk")))
                .expect("run fixtures/m9/input-connection/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    app.launch().unwrap();
    (app, root)
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn invoke(
    app: &mut App,
    object: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Value, VmExit> {
    app.vm.invoke_instance(object, name, sig, args)
}
fn make(app: &mut App, variant: &str, full: bool) -> ObjRef {
    app.vm
        .invoke_static(
            &probe(variant),
            "make",
            "(Z)Landroid/view/inputmethod/BaseInputConnection;",
            &[Value::Int(i32::from(full))],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap()
}
fn exception(result: Result<Value, VmExit>, class: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(ref error)) if error.class==class),
        "{result:?}"
    );
}
fn unavailable(result: Result<Value, VmExit>) {
    assert!(
        matches!(result,Err(VmExit::Error(ref error)) if matches!(error.code.as_str(),"API_UNAVAILABLE"|"NOT_FOUND")),
        "{result:?}"
    );
}
#[test]
fn original_sdk_text_queries_selection_and_utf16_match_source_reference_in_two_namespaces() {
    for variant in ["input-connection", "renamed"] {
        let (mut app, root) = app(variant);
        let oracle =
            std::fs::read_to_string(output().join(variant).join("source-reference-oracle.txt"))
                .unwrap();
        assert_eq!(oracle.lines().count(), 27);
        for line in oracle.lines() {
            let (index, expected) = line.split_once('\t').unwrap();
            let result = app
                .vm
                .invoke_static(
                    &probe(variant),
                    "run",
                    "(I)Ljava/lang/String;",
                    &[Value::Int(index.parse().unwrap())],
                )
                .unwrap_or_else(|error| panic!("{variant} case{index}: {error:?}"));
            assert_eq!(text(&app, result), expected, "{variant} case{index}");
        }
        let proof =
            std::fs::read_to_string(root.join("Download/androswitch-input-connection-proof.txt"))
                .unwrap();
        assert!(proof.contains("status=PASS\n"));
        assert!(proof.contains("ime_delivery=unavailable\n"));
    }
}
#[test]
fn actual_virtual_context_editable_replace_callbacks_and_exceptions_are_preserved() {
    let variant = "renamed";
    let (mut app, _) = app(variant);
    assert_eq!(
        app.vm
            .invoke_static(&probe(variant), "callbackProof", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    let context = app
        .vm
        .invoke_static(
            &probe(variant),
            "overrideContext",
            "()Landroid/view/inputmethod/BaseInputConnection;",
            &[],
        )
        .unwrap();
    assert!(context.as_ref().unwrap().is_some());
    exception(
        app.vm.invoke_static(
            &probe(variant),
            "throwingContext",
            "()Landroid/view/inputmethod/BaseInputConnection;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    exception(
        app.vm.invoke_static(
            &probe(variant),
            "nullTarget",
            "()Landroid/view/inputmethod/BaseInputConnection;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let tracked = app
        .vm
        .invoke_static(
            &probe(variant),
            "tracking",
            &format!("(Z)L{}/InputCases$TrackingConnection;", namespace(variant)),
            &[Value::Int(1)],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let editable = app
        .vm
        .heap
        .get_field(tracked, "text")
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    app.vm
        .heap
        .put_field(editable, "fail", Value::Int(1))
        .unwrap();
    exception(
        invoke(
            &mut app,
            tracked,
            "commitText",
            "(Ljava/lang/CharSequence;I)Z",
            &[Value::StrPlaceholder("fail".into()), Value::Int(1)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_eq!(
        app.vm.heap.get_field(tracked, "begins").unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        app.vm
            .heap
            .get_field(tracked, "ends")
            .unwrap_or(Value::Int(0)),
        Value::Int(0)
    );
    assert_eq!(
        app.vm.heap.get_field(editable, "fail").unwrap(),
        Value::Int(1)
    );
}
#[test]
fn connection_view_context_and_editable_edges_survive_gc_then_are_reclaimed() {
    let variant = "input-connection";
    let (mut app, _) = app(variant);
    let result = app
        .vm
        .invoke_static(&probe(variant), "retention", "()[Ljava/lang/Object;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let values = app.vm.heap.as_array_elems(result).unwrap().0.to_vec();
    let input = values[0].as_ref().unwrap().unwrap();
    let view = values[1].as_ref().unwrap().unwrap();
    let context = values[2].as_ref().unwrap().unwrap();
    let editable = invoke(
        &mut app,
        input,
        "getEditable",
        "()Landroid/text/Editable;",
        &[],
    )
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    app.vm
        .statics
        .insert((probe(variant), "kept".into()), Value::Obj(input));
    app.vm.gc();
    assert_ne!(app.vm.heap.class_of(context).unwrap(), FREED_CLASS);
    assert_eq!(
        invoke(
            &mut app,
            view,
            "getContext",
            "()Landroid/content/Context;",
            &[]
        )
        .unwrap(),
        Value::Obj(context)
    );
    assert_eq!(
        invoke(
            &mut app,
            input,
            "getEditable",
            "()Landroid/text/Editable;",
            &[]
        )
        .unwrap(),
        Value::Obj(editable)
    );
    app.vm
        .statics
        .insert((probe(variant), "kept".into()), Value::Null);
    app.vm.gc();
    assert_eq!(app.vm.heap.class_of(input).unwrap(), FREED_CLASS);
    assert_eq!(app.vm.heap.class_of(context).unwrap(), FREED_CLASS);
    assert_eq!(app.vm.heap.class_of(editable).unwrap(), FREED_CLASS);
}
#[test]
fn missing_ime_and_unrepresentable_strings_fail_without_fabricated_delivery() {
    let variant = "input-connection";
    let (mut app, _) = app(variant);
    let input = make(&mut app, variant, false);
    unavailable(invoke(
        &mut app,
        input,
        "commitText",
        "(Ljava/lang/CharSequence;I)Z",
        &[Value::StrPlaceholder("text".into()), Value::Int(1)],
    ));
    let editable = invoke(
        &mut app,
        input,
        "getEditable",
        "()Landroid/text/Editable;",
        &[],
    )
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    assert_eq!(
        invoke(&mut app, editable, "length", "()I", &[]).unwrap(),
        Value::Int(0)
    );
    unavailable(invoke(
        &mut app,
        input,
        "sendKeyEvent",
        "(Landroid/view/KeyEvent;)Z",
        &[Value::Null],
    ));
    let input = make(&mut app, variant, true);
    unavailable(invoke(
        &mut app,
        input,
        "getTextBeforeCursor",
        "(II)Ljava/lang/CharSequence;",
        &[Value::Int(1), Value::Int(1)],
    ));
    unavailable(invoke(
        &mut app,
        input,
        "setComposingText",
        "(Ljava/lang/CharSequence;I)Z",
        &[Value::StrPlaceholder("styled".into()), Value::Int(1)],
    ));
    let result = app
        .vm
        .invoke_static(&probe(variant), "unpaired", "()[Ljava/lang/Object;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let input = app.vm.heap.as_array_elems(result).unwrap().0[0]
        .as_ref()
        .unwrap()
        .unwrap();
    let editable = invoke(
        &mut app,
        input,
        "getEditable",
        "()Landroid/text/Editable;",
        &[],
    )
    .unwrap()
    .as_ref()
    .unwrap()
    .unwrap();
    assert_eq!(
        invoke(&mut app, editable, "charAt", "(I)C", &[Value::Int(0)]).unwrap(),
        Value::Int(0xd800)
    );
    unavailable(invoke(
        &mut app,
        editable,
        "toString",
        "()Ljava/lang/String;",
        &[],
    ));
    assert_eq!(
        invoke(&mut app, editable, "length", "()I", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        app.android.method_metadata(
            "Landroid/view/inputmethod/InputConnection;",
            "getEditable",
            "()Landroid/text/Editable;"
        ),
        None
    );
    assert_eq!(
        app.android.method_metadata(
            "Landroid/view/inputmethod/BaseInputConnection;",
            "commitText",
            "(Ljava/lang/CharSequence;I)Z"
        ),
        Some(HostMethodKind::Instance)
    );
}
