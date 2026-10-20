#![cfg(feature = "advanced-components")]
use rd_testsupport::m9_app::*;
use rd_vm::m9::{self, Channel, Notification, RemoteOp, RemoteViews};
use rd_vm::{Engine, Value, VmConfig, VmExit};
fn engine() -> Engine {
    let apk = rd_apk::Apk::from_bytes(apk()).unwrap();
    let dex = rd_dex::Dex::parse(apk.zip.read("classes.dex").unwrap()).unwrap();
    let mut e = Engine::new(vec![dex], VmConfig::default());
    e.set_resources(apk);
    e.launch_app(MAIN, PACKAGE, vec![]).unwrap();
    e
}
fn obj(e: &mut Engine, c: &str) -> u32 {
    let r = e.heap.alloc_instance(c.into(), Vec::new()).unwrap();
    e.attach_host_state(c, r).unwrap();
    r
}
fn string(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.into()).unwrap())
}
fn call(e: &mut Engine, c: &str, n: &str, sig: &str, r: u32, args: &[Value]) -> Value {
    m9::call_instance(e, c, n, sig, r, args)
        .unwrap()
        .unwrap_or_else(|| panic!("unhandled {c}->{n}{sig}"))
}
fn views(e: &Engine) -> Vec<u32> {
    e.m9_accessible_nodes()
        .into_iter()
        .filter(|n| n.editable)
        .map(|n| n.id)
        .collect()
}
fn count(e: &Engine, class: &str, name: &str) -> i32 {
    match e.statics.get(&(class.into(), name.into())) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}
fn channel(e: &mut Engine) {
    e.fw.m9.channels.insert(
        "messages".into(),
        Channel {
            id: "messages".into(),
            name: "Messages".into(),
            description: String::new(),
            importance: 3,
            group: None,
            lights: false,
            vibration: false,
            deleted: false,
        },
    );
}
#[test]
fn focused_ime_selection_composition_and_unicode() {
    let mut e = engine();
    let v = views(&e);
    assert_eq!(v.len(), 2);
    e.m9_focus(v[0]).unwrap();
    e.m9_set_selection(1, 3).unwrap();
    assert_eq!(e.m9_commit_text("Olá", 1, true).unwrap(), "AOláB");
    assert_eq!(e.m9_commit_text("🦀", 1, true).unwrap(), "A🦀B");
    assert_eq!(e.m9_commit_text("XY", 1, false).unwrap(), "AXYB");
    e.m9_set_selection(3, 3).unwrap();
    assert_eq!(e.m9_delete_surrounding(1, 0, false).unwrap(), "AXB");
    e.m9_focus(v[1]).unwrap();
    assert_eq!(e.type_text("!", false).unwrap(), "second!");
    e.m9_focus(v[0]).unwrap();
    assert!(e.m9_set_selection(999, 999).is_err());
    e.m9_set_selection(0, 3).unwrap();
    e.m9_commit_text("😀", 1, false).unwrap();
    assert!(e.m9_set_selection(1, 1).is_err());
    e.m9_set_selection(2, 2).unwrap();
    assert_eq!(e.m9_delete_surrounding(1, 0, true).unwrap(), "");
}
#[test]
fn input_connection_and_real_ime_lifecycle() {
    let mut e = engine();
    let v = views(&e)[0];
    let service = obj(&mut e, IME);
    e.m9_start_component(service, "ime").unwrap();
    assert_eq!(count(&e, IME, "onInitializeInterface"), 1);
    let manager = obj(&mut e, "Landroid/view/inputmethod/InputMethodManager;");
    assert_eq!(
        call(
            &mut e,
            "Landroid/view/inputmethod/InputMethodManager;",
            "showSoftInput",
            "(Landroid/view/View;I)Z",
            manager,
            &[Value::Obj(v), Value::Int(0)]
        ),
        Value::Int(1)
    );
    assert_eq!(count(&e, IME, "onStartInput"), 1);
    let Value::Obj(c) = call(
        &mut e,
        "Landroid/inputmethodservice/InputMethodService;",
        "getCurrentInputConnection",
        "()Landroid/view/inputmethod/InputConnection;",
        service,
        &[],
    ) else {
        panic!()
    };
    let s = string(&mut e, "!");
    call(
        &mut e,
        "Landroid/view/inputmethod/InputConnection;",
        "commitText",
        "(Ljava/lang/CharSequence;I)Z",
        c,
        &[s, Value::Int(1)],
    );
    assert_eq!(
        e.m9_accessible_nodes()
            .into_iter()
            .find(|n| n.id == v)
            .unwrap()
            .text,
        "A😀B!"
    );
}
#[test]
fn notifications_permissions_channels_update_timeout_listener() {
    let mut e = engine();
    channel(&mut e);
    let listener = obj(&mut e, LISTENER);
    e.m9_start_component(listener, "notification_listener")
        .unwrap();
    assert_eq!(count(&e, LISTENER, "onListenerConnected"), 1);
    let n = Notification {
        channel: "messages".into(),
        title: "one".into(),
        timeout_ms: 50,
        ..Default::default()
    };
    assert!(!e.m9_notify("tag", 7, n.clone()).unwrap());
    e.m9_set_notification_permission(true);
    assert!(e.m9_notify("tag", 7, n.clone()).unwrap());
    let mut replacement = n;
    replacement.title = "updated".into();
    e.m9_notify("tag", 7, replacement).unwrap();
    assert_eq!(e.m9_notifications().len(), 1);
    assert_eq!(e.m9_notifications()[0].title, "updated");
    assert_eq!(count(&e, LISTENER, "onNotificationPosted"), 2);
    e.advance_clock(49).unwrap();
    assert_eq!(e.m9_notifications().len(), 1);
    e.advance_clock(1).unwrap();
    assert!(e.m9_notifications().is_empty());
    assert_eq!(count(&e, LISTENER, "onNotificationRemoved"), 1);
}
#[test]
fn channel_constructor_is_not_registration_and_importance_is_immutable() {
    let mut e = engine();
    let c = obj(&mut e, "Landroid/app/NotificationChannel;");
    let id = string(&mut e, "news");
    let name = string(&mut e, "News");
    call(
        &mut e,
        "Landroid/app/NotificationChannel;",
        "<init>",
        "(Ljava/lang/String;Ljava/lang/CharSequence;I)V",
        c,
        &[id, name, Value::Int(3)],
    );
    assert!(e.fw.m9.channels.is_empty());
    let manager = obj(&mut e, "Landroid/app/NotificationManager;");
    call(
        &mut e,
        "Landroid/app/NotificationManager;",
        "createNotificationChannel",
        "",
        manager,
        &[Value::Obj(c)],
    );
    e.fw.m9.channel_values.get_mut(&c).unwrap().importance = 5;
    call(
        &mut e,
        "Landroid/app/NotificationManager;",
        "createNotificationChannel",
        "",
        manager,
        &[Value::Obj(c)],
    );
    assert_eq!(e.fw.m9.channels["news"].importance, 3);
}
#[test]
fn remote_views_render_and_widget_provider_dex_lifecycle() {
    let mut e = engine();
    let provider = obj(&mut e, WIDGET);
    let id = e.m9_allocate_widget(3).unwrap();
    e.m9_bind_widget(id, provider).unwrap();
    assert_eq!(count(&e, WIDGET, "onEnabled"), 1);
    assert_eq!(count(&e, WIDGET, "onUpdate"), 1);
    // ArscFix reserves type 1=layout, type 2=id; derive actual IDs from UI/resources.
    let rv = RemoteViews {
        package: PACKAGE.into(),
        layout: 0x7f010000,
        operations: vec![RemoteOp::Text(0x7f020002, "Remote hello".into())],
    };
    e.m9_update_widget(id, rv.clone(), false).unwrap();
    let tree = e.m9_widget_tree(id).unwrap();
    assert_eq!(tree.children[2].text, "Remote hello");
    let png = rd_render::render_snapshot(&tree).to_png();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut partial = rv;
    partial.operations = vec![RemoteOp::Text(0x7f020002, "updated".into())];
    e.m9_update_widget(id, partial, true).unwrap();
    assert_eq!(e.m9_widget_tree(id).unwrap().children[2].text, "updated");
    e.m9_delete_widget(id).unwrap();
    assert_eq!(count(&e, WIDGET, "onDeleted"), 1);
    assert_eq!(count(&e, WIDGET, "onDisabled"), 1);
}
#[test]
fn accessibility_observes_same_editor_and_delivers_dex_events() {
    let mut e = engine();
    let service = obj(&mut e, A11Y);
    e.m9_start_component(service, "accessibility").unwrap();
    assert_eq!(count(&e, A11Y, "onServiceConnected"), 1);
    let v = views(&e)[1];
    assert!(e
        .m9_accessibility_action(v, 2097152, Some("accessible"))
        .unwrap());
    let n = e
        .m9_accessible_nodes()
        .into_iter()
        .find(|n| n.id == v)
        .unwrap();
    assert_eq!(n.text, "accessible");
    assert!(n.focused);
    assert!(count(&e, A11Y, "onAccessibilityEvent") >= 2);
    assert!(e.m9_accessibility_action(v, 64, None).unwrap());
    assert_eq!(e.fw.m9.accessibility_focus, Some(v));
    let err = e.m9_accessibility_action(v, 4096, None).unwrap_err();
    assert!(matches!(err, VmExit::Error(_)));
}
#[test]
fn unknown_component_apis_are_not_fabricated_success() {
    let mut e = engine();
    let r = obj(&mut e, "Landroid/app/Notification$Builder;");
    let ctx = e.fw.current_activity.unwrap();
    let channel = string(&mut e, "messages");
    call(
        &mut e,
        "Landroid/app/Notification$Builder;",
        "<init>",
        "",
        r,
        &[Value::Obj(ctx), channel],
    );
    assert!(m9::call_instance(
        &mut e,
        "Landroid/app/Notification$Builder;",
        "setFakeOption",
        "()V",
        r,
        &[]
    )
    .unwrap()
    .is_none());
}

/// Round 6 (Compose text input): the clipboard keeps one primary clip,
/// EditorInfo stores the initial surrounding text and accepts newer setters,
/// and the IMM accepts a non-EditText IME target (Compose's view edits
/// through its own InputConnection) without touching the EditText model.
#[test]
fn clipboard_editorinfo_and_non_edittext_ime_target() {
    let mut e = engine();
    const CM: &str = "Landroid/content/ClipboardManager;";
    let cm = e.heap.alloc_instance(CM.into(), Vec::new()).unwrap();
    assert_eq!(
        call(&mut e, CM, "hasPrimaryClip", "()Z", cm, &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut e,
            CM,
            "getPrimaryClip",
            "()Landroid/content/ClipData;",
            cm,
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut e,
            CM,
            "getPrimaryClipDescription",
            "()Landroid/content/ClipDescription;",
            cm,
            &[]
        ),
        Value::Null
    );
    let clip = e
        .heap
        .alloc_instance("Landroid/content/ClipData;".into(), Vec::new())
        .unwrap();
    call(
        &mut e,
        CM,
        "setPrimaryClip",
        "(Landroid/content/ClipData;)V",
        cm,
        &[Value::Obj(clip)],
    );
    assert_eq!(
        call(&mut e, CM, "hasPrimaryClip", "()Z", cm, &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut e,
            CM,
            "getPrimaryClip",
            "()Landroid/content/ClipData;",
            cm,
            &[]
        ),
        Value::Obj(clip)
    );
    call(&mut e, CM, "clearPrimaryClip", "()V", cm, &[]);
    assert_eq!(
        call(&mut e, CM, "hasPrimaryClip", "()Z", cm, &[]),
        Value::Int(0)
    );

    const EI: &str = "Landroid/view/inputmethod/EditorInfo;";
    let info = e.heap.alloc_instance(EI.into(), Vec::new()).unwrap();
    let s = string(&mut e, "2 kg");
    call(
        &mut e,
        EI,
        "setInitialSurroundingSubText",
        "(Ljava/lang/CharSequence;I)V",
        info,
        &[s.clone(), Value::Int(0)],
    );
    assert_eq!(
        call(
            &mut e,
            EI,
            "getInitialTextBeforeCursor",
            "(II)Ljava/lang/CharSequence;",
            info,
            &[Value::Int(9), Value::Int(0)]
        ),
        s
    );
    assert_eq!(
        call(
            &mut e,
            EI,
            "setSupportedHandwritingGestures",
            "(Ljava/util/List;)V",
            info,
            &[Value::Null]
        ),
        Value::Null
    );

    const IMM: &str = "Landroid/view/inputmethod/InputMethodManager;";
    let imm = e.heap.alloc_instance(IMM.into(), Vec::new()).unwrap();
    let plain = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), Vec::new())
        .unwrap();
    let before = e.fw.m9.focused;
    assert_eq!(
        call(
            &mut e,
            IMM,
            "showSoftInput",
            "(Landroid/view/View;I)Z",
            imm,
            &[Value::Obj(plain), Value::Int(0)]
        ),
        Value::Int(1)
    );
    assert!(e.fw.m9.keyboard_visible);
    assert_eq!(e.fw.m9.focused, before, "EditText editor model untouched");
    assert_eq!(
        call(
            &mut e,
            IMM,
            "restartInput",
            "(Landroid/view/View;)V",
            imm,
            &[Value::Obj(plain)]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut e,
            IMM,
            "updateSelection",
            "(Landroid/view/View;IIII)V",
            imm,
            &[
                Value::Obj(plain),
                Value::Int(0),
                Value::Int(0),
                Value::Int(-1),
                Value::Int(-1)
            ]
        ),
        Value::Null
    );
}
