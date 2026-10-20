// Included in the existing MCP tests module to share real APK builders.
fn selector_node(text: &str) -> UiNode {
    UiNode {
        class: "android.widget.TextView".into(),
        resource_id: Some("p:id/tv".into()),
        text: text.into(),
        bounds: (0, 0, 100, 48),
        visibility: rd_vm::framework::Vis::Visible,
        enabled: true,
        clickable: false,
        focusable: false,
        truncated: false,
        children: Vec::new(),
    }
}
fn selector_query(v: serde_json::Value) -> ViewQuery {
    ViewQuery::parse(v.as_object().unwrap(), "find_views").unwrap()
}
#[test]
fn view_query_exact_and_criteria_are_conjunctive() {
    let n = selector_node("Olá world");
    for (q, want) in [
        (serde_json::json!({"text":"Olá world"}), 1),
        (serde_json::json!({"text":"Olá"}), 0),
        (serde_json::json!({"text":"Olá","contains":true}), 1),
        (serde_json::json!({"class":"View"}), 0),
        (
            serde_json::json!({"class":"TextView","resource_id":"wrong"}),
            0,
        ),
        (
            serde_json::json!({"class":"TextView","resource_id":"p:id/tv"}),
            1,
        ),
    ] {
        assert_eq!(query_tree(&n, &selector_query(q), 0, "s", None).total, want);
    }
}
#[test]
fn view_query_visibility_inherits_but_enabled_is_per_view() {
    let mut n = selector_node("parent");
    n.children.push(selector_node("child"));
    n.enabled = false;
    let q = selector_query(serde_json::json!({"text":"child","enabled":true}));
    assert_eq!(query_tree(&n, &q, 0, "s", None).total, 1);
    n.visibility = rd_vm::framework::Vis::Gone;
    assert_eq!(query_tree(&n, &q, 0, "s", None).total, 0);
    let q = selector_query(serde_json::json!({"text":"child","visible":false}));
    let m = query_tree(&n, &q, 0, "s", None);
    assert_eq!(m.total, 1);
    assert_ne!(m.nodes[0].visibility, rd_vm::framework::Vis::Visible);
}
#[test]
fn view_query_limits_results_without_fabricating_complete_counts() {
    let mut n = selector_node("parent");
    n.children = vec![selector_node("child"); 300];
    let q = selector_query(serde_json::json!({"text":"child","limit":3}));
    let m = query_tree(&n, &q, 0, "s", None);
    assert_eq!(m.total, 300);
    assert_eq!(m.nodes.len(), 3);
    assert!(!m.truncated);
    assert!(m.nodes.iter().all(|n| n.children.is_empty()));
    n.children = vec![selector_node("child"); UI_QUERY_NODE_CAP + 1];
    let m = query_tree(&n, &q, 0, "s", None);
    assert!(m.truncated);
    assert_eq!(m.nodes.len(), 3);
    n.children.clear();
    n.truncated = true;
    assert!(query_tree(&n, &q, 0, "s", None).truncated);
}
#[test]
fn view_tokens_bind_session_revision_package_and_complete_selector_state() {
    let mut n = selector_node("label");
    let q = selector_query(serde_json::json!({"text":"label"}));
    let a = query_tree(&n, &q, 1, "a", Some("p")).token;
    assert_eq!(a, query_tree(&n, &q, 1, "a", Some("p")).token);
    assert_ne!(a, query_tree(&n, &q, 2, "a", Some("p")).token);
    assert_ne!(a, query_tree(&n, &q, 1, "b", Some("p")).token);
    assert_ne!(a, query_tree(&n, &q, 1, "a", Some("q")).token);
    n.enabled = false;
    assert_ne!(a, query_tree(&n, &q, 1, "a", Some("p")).token);
}
#[test]
fn new_view_tools_reject_unknown_null_wrong_type_and_unbounded_arguments() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    for tool in ["find_views", "tap_view", "wait_for_view"] {
        for args in [
            serde_json::json!({}),
            serde_json::json!({"text":""}),
            serde_json::json!({"text":null}),
            serde_json::json!({"text":1}),
            serde_json::json!({"text":"x","unknown":true}),
            serde_json::json!({"text":"x","visible":null}),
            serde_json::json!({"text":"x","contains":"yes"}),
        ] {
            let (v, err) = call(&mut s, tool, args);
            assert!(err, "{tool}: {v}");
            assert_eq!(v["code"], "INVALID_FORMAT");
        }
    }
    for args in [
        serde_json::json!({"text":"x","limit":0}),
        serde_json::json!({"text":"x","limit":201}),
        serde_json::json!({"text":"x".repeat(4097)}),
    ] {
        let (v, err) = call(&mut s, "find_views", args);
        assert!(err);
        assert_eq!(v["code"], "LIMIT_EXCEEDED");
    }
}
#[test]
fn structured_find_tap_and_stale_token_use_real_apk_views() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let apk = fixture_apk_path();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    let args = serde_json::json!({"resource_id":"com.test:id/tv"});
    let (a, err) = call(&mut s, "find_views", args.clone());
    assert!(!err, "{a}");
    assert_eq!(a["total"], 1);
    let token = a["tree_token"].as_str().unwrap();
    let (b, err) = call(&mut s, "find_views", args.clone());
    assert!(!err);
    assert_eq!(b["tree_token"], token);
    let (tap, err) = call(
        &mut s,
        "tap_view",
        serde_json::json!({"resource_id":"com.test:id/tv","expected_token":token}),
    );
    assert!(!err, "{tap}");
    assert!(tap["listener_acionado"].is_boolean());
    let (stale, err) = call(
        &mut s,
        "tap_view",
        serde_json::json!({"resource_id":"com.test:id/tv","expected_token":token}),
    );
    assert!(err);
    assert_eq!(stale["code"], "STALE_VIEW");
    std::fs::remove_file(apk).unwrap();
}
#[test]
fn structured_wait_checks_immediately_and_does_not_overshoot_virtual_budget() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let apk = fixture_apk_path();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    for (args, want, elapsed) in [
        (
            serde_json::json!({"text":"Olá do MCP","timeout_ms":0}),
            true,
            0,
        ),
        (
            serde_json::json!({"text":"missing","state":"absent","timeout_ms":0}),
            true,
            0,
        ),
        (
            serde_json::json!({"text":"missing","timeout_ms":250,"step_ms":100}),
            false,
            250,
        ),
        (
            serde_json::json!({"text":"Olá do MCP","state":"absent","timeout_ms":0}),
            false,
            0,
        ),
    ] {
        let (v, e) = call(&mut s, "wait_for_view", args);
        assert!(!e, "{v}");
        assert_eq!(v["condition_met"], want);
        assert_eq!(v["waited_ms"], elapsed);
    }
    let clock = s.engine.as_ref().unwrap().fw.clock();
    for args in [
        serde_json::json!({"text":"x","step_ms":0}),
        serde_json::json!({"text":"x","timeout_ms":60001}),
        serde_json::json!({"text":"x","timeout_ms":60000,"step_ms":1}),
    ] {
        let (v, e) = call(&mut s, "wait_for_view", args);
        assert!(e);
        assert_eq!(v["code"], "LIMIT_EXCEEDED");
        assert_eq!(s.engine.as_ref().unwrap().fw.clock(), clock);
    }
    std::fs::remove_file(apk).unwrap();
}

#[test]
fn malformed_legacy_ui_arguments_fail_before_any_app_code() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    for (tool, args) in [
        ("tap_text", serde_json::json!({"text":"x","nth":-1})),
        ("tap_text", serde_json::json!({"text":"x","nth":1.5})),
        ("tap_text", serde_json::json!({"text":"x","exact":"false"})),
        ("type_text", serde_json::json!({"text":123,"replace":true})),
        ("type_text", serde_json::json!({"replace":true})),
        ("type_text", serde_json::json!({"text":"x","replace":null})),
        ("observe", serde_json::json!({"diff":"false"})),
        ("observe", serde_json::json!({"boxes":1})),
        ("observe", serde_json::json!({"format":null})),
    ] {
        let (v, e) = call(&mut s, tool, args);
        assert!(e, "{v}");
        assert_eq!(v["code"], "INVALID_FORMAT");
        assert!(s.engine.is_none());
    }
}

#[test]
fn structured_tap_rejects_ambiguity_same_metadata_occlusion_and_disabled_targets() {
    use rd_vm::framework::{HostObj, Vis};
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let apk = fixture_apk_path();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    assert!(
        !s.call_tool("find_views", &serde_json::json!({"text":"Olá do MCP"}))
            .is_error
    );
    let (original, other, parent) = {
        let e = s.engine.as_mut().unwrap();
        let original =
            e.fw.objects
                .iter()
                .find_map(|(&r, h)| {
                    matches!(h,HostObj::View{text,..} if text=="Olá do MCP").then_some(r)
                })
                .unwrap();
        let state = e.fw.get(original).unwrap().clone();
        let parent = match &state {
            HostObj::View { parent, .. } => parent.unwrap(),
            _ => unreachable!(),
        };
        let other = e
            .heap
            .alloc_instance("Landroid/widget/TextView;".into(), vec![])
            .unwrap();
        e.fw.objects.insert(other, state);
        if let Some(HostObj::View { children, .. }) = e.fw.objects.get_mut(&parent) {
            children.push(other);
        }
        (original, other, parent)
    };
    let (v, error) = call(&mut s, "tap_view", serde_json::json!({"text":"Olá do MCP"}));
    assert!(error);
    assert_eq!(v["code"], "AMBIGUOUS_VIEW");
    // Same id, class, text and bounds are NOT sufficient identity: the
    // selector can distinguish by clickable, so the hit guard must too.
    if let Some(HostObj::View { click_method, .. }) =
        s.engine.as_mut().unwrap().fw.objects.get_mut(&original)
    {
        *click_method = Some("must_not_run".into());
    }
    let (v, error) = call(
        &mut s,
        "tap_view",
        serde_json::json!({"text":"Olá do MCP","clickable":true}),
    );
    assert!(error, "{v}");
    assert_eq!(v["code"], "TARGET_OCCLUDED");
    if let Some(HostObj::View { text, .. }) = s.engine.as_mut().unwrap().fw.objects.get_mut(&other)
    {
        *text = "occluder".into();
    }
    let (v, error) = call(&mut s, "tap_view", serde_json::json!({"text":"Olá do MCP"}));
    assert!(error);
    assert_eq!(v["code"], "TARGET_OCCLUDED");
    if let Some(HostObj::View { vis, .. }) = s.engine.as_mut().unwrap().fw.objects.get_mut(&other) {
        *vis = Vis::Gone;
    }
    if let Some(HostObj::View { enabled, .. }) =
        s.engine.as_mut().unwrap().fw.objects.get_mut(&original)
    {
        *enabled = false;
    }
    let (v, error) = call(&mut s, "tap_view", serde_json::json!({"text":"Olá do MCP"}));
    assert!(error);
    assert_eq!(v["code"], "VIEW_NOT_INTERACTABLE");
    // Keep the manually-added host object reachable while checking its tree.
    assert!(
        matches!(s.engine.as_ref().unwrap().fw.get(parent),Some(HostObj::View{children,..}) if children.contains(&other))
    );
    std::fs::remove_file(apk).unwrap();
}

#[test]
fn structured_zero_timeout_respects_zero_wait_policy_without_advancing_time() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let apk = fixture_apk_path();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    s.policy.max_wait_ms = 0;
    let before = s.engine.as_ref().unwrap().fw.clock();
    let (v, error) = call(
        &mut s,
        "wait_for_view",
        serde_json::json!({"text":"Olá do MCP","timeout_ms":0}),
    );
    assert!(!error, "{v}");
    assert_eq!(v["condition_met"], true);
    assert_eq!(v["polls"], 1);
    assert_eq!(s.engine.as_ref().unwrap().fw.clock(), before);
    std::fs::remove_file(apk).unwrap();
}

#[test]
fn structured_node_text_prefix_is_bounded_without_splitting_utf8_or_changing_matching() {
    let n = selector_node(&"🙂".repeat(2048));
    let v = node_json(&n);
    assert_eq!(v["text"].as_str().unwrap().len(), 4096);
    assert_eq!(v["text_bytes"], 8192);
    assert_eq!(v["text_truncated"], true);
    let q = selector_query(serde_json::json!({"resource_id":"p:id/tv"}));
    let matches = query_tree(&n, &q, 0, "s", None);
    assert_eq!(matches.nodes[0].text, n.text);
}

#[test]
fn separate_window_visibility_does_not_inherit_hidden_activity_content() {
    let mut hidden = selector_node("activity");
    hidden.visibility = rd_vm::framework::Vis::Gone;
    let dialog = selector_node("dialog");
    let q = selector_query(serde_json::json!({"text":"dialog"}));
    let result = query_roots(&[hidden, dialog], &q, 0, "s", None);
    assert_eq!(result.total, 1);
    assert!(!result.truncated);
}

#[test]
fn structured_wait_observes_guest_runnable_visibility_change_on_virtual_clock() {
    use rd_testsupport::vm::op22c;
    const ACC_CONSTRUCTOR: u32 = 0x1_0000;
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut b = DexBuilder::new();
    b.class("Landroid/os/Handler;", "Ljava/lang/Object;");
    let task = b.class("LMcpWaitTask;", "Ljava/lang/Object;");
    b.instance_field(task, "target", "Landroid/view/View;");
    let field = b.field_idx("LMcpWaitTask;", "Landroid/view/View;", "target");
    let void = b.proto_idx("V", vec![]);
    let object_init = b.method_idx("Ljava/lang/Object;", void, "<init>");
    let handler_init = b.method_idx("Landroid/os/Handler;", void, "<init>");
    let with_view = b.proto_idx("V", vec!["Landroid/view/View;".into()]);
    let task_init = b.method_idx("LMcpWaitTask;", with_view, "<init>");
    let delayed = b.proto_idx("Z", vec!["Ljava/lang/Runnable;".into(), "J".into()]);
    let post = b.method_idx("Landroid/os/Handler;", delayed, "postDelayed");
    let int_void = b.proto_idx("V", vec!["I".into()]);
    let visibility = b.method_idx("Landroid/view/View;", int_void, "setVisibility");
    let handler_type = b.type_idx_cached("Landroid/os/Handler;");
    let task_type = b.type_idx_cached("LMcpWaitTask;");
    b.direct(
        task,
        "<init>",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(3, 2, 1, {
            let mut u = op35c(0x70, 1, object_init, [1, 0, 0, 0, 0]);
            u.extend(op22c(0x5b, 2, 1, field));
            u.extend(op10x(0x0e));
            u
        })),
    );
    b.direct(
        task,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 2, {
            let mut u = op22c(0x54, 0, 1, field);
            u.extend(op21s(0x13, 1, 8));
            u.extend(op35c(0x6e, 2, visibility, [0, 1, 0, 0, 0]));
            u.extend(op10x(0x0e));
            u
        })),
    );
    b.direct(
        task,
        "arm",
        "V",
        vec!["Landroid/view/View;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 4, {
            let mut u = op21c(0x22, 0, handler_type);
            u.extend(op35c(0x70, 1, handler_init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x22, 1, task_type));
            u.extend(op35c(0x70, 2, task_init, [1, 4, 0, 0, 0]));
            u.extend(op21s(0x16, 2, 75));
            u.extend(op35c(0x6e, 4, post, [0, 1, 2, 3, 0]));
            u.extend(op10x(0x0e));
            u
        })),
    );
    let (arsc, layout, dex) = fixture_parts();
    let apk = fixture_apk_write(rd_testsupport::apkfix::build_apk_sdk(
        "com.test.mcp",
        "com.test.mcp.Main",
        arsc,
        vec![
            ("res/layout/activity_main.xml", layout),
            ("classes.dex", dex),
            ("classes2.dex", b.finish()),
        ],
        26,
        30,
    ));
    let mut s = AgentSession::new();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    let engine = s.engine.as_mut().unwrap();
    let target = engine
        .fw
        .objects
        .iter()
        .find_map(|(&r, h)| {
            matches!(h,rd_vm::framework::HostObj::View{text,..} if text=="Olá do MCP").then_some(r)
        })
        .unwrap();
    engine
        .invoke_static(
            "LMcpWaitTask;",
            "arm",
            "(Landroid/view/View;)V",
            &[Value::Obj(target)],
        )
        .unwrap();
    assert_eq!(
        s.engine.as_ref().unwrap().looper_pending(),
        1,
        "guest setup must enqueue exactly one Runnable"
    );
    let (v, e) = call(
        &mut s,
        "wait_for_view",
        serde_json::json!({"text":"Olá do MCP","state":"absent","timeout_ms":125,"step_ms":50}),
    );
    assert!(!e, "{v}");
    assert_eq!(v["condition_met"], true, "wait response: {v}");
    assert_eq!(v["present"], false);
    assert_eq!(v["waited_ms"], 100);
    assert_eq!(v["polls"], 3);
    assert_eq!(s.engine.as_ref().unwrap().fw.clock(), 100);
    std::fs::remove_file(apk).unwrap();
}
