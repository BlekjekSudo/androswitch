// Included in MCP tests to reuse the session lock and real APK fixtures.
#[test]
fn runtime_diagnostics_validate_objects_fields_ranges_and_cursor_shape() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    for tool in ["session_info", "logs_tail", "last_error"] {
        for args in [
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!(1),
            serde_json::json!({"unknown":true}),
        ] {
            let (v, e) = call(&mut s, tool, args);
            assert!(e, "{tool}: {v}");
            assert_eq!(v["code"], "INVALID_FORMAT");
        }
    }
    for args in [
        serde_json::json!({"max_bytes":null}),
        serde_json::json!({"max_bytes":1.5}),
        serde_json::json!({"cursor":null}),
        serde_json::json!({"cursor":{}}),
        serde_json::json!({"cursor":{"generation":0,"offset":-1}}),
        serde_json::json!({"cursor":{"generation":0,"offset":0,"extra":true}}),
    ] {
        let (v, e) = call(&mut s, "logs_tail", args);
        assert!(e);
        assert_eq!(v["code"], "INVALID_FORMAT");
    }
    for n in [0, u64::MAX, LOG_TAIL_MAX_BYTES + 1] {
        let (v, e) = call(&mut s, "logs_tail", serde_json::json!({"max_bytes":n}));
        assert!(e);
        assert_eq!(v["code"], "LIMIT_EXCEEDED");
    }
    for (tool, args) in [
        ("last_error", serde_json::json!({"clear":null})),
        ("logs", serde_json::json!({"clear":"true"})),
        ("runtime_health", serde_json::json!({"gc":1})),
    ] {
        let (v, e) = call(&mut s, tool, args);
        assert!(e);
        assert_eq!(v["code"], "INVALID_FORMAT");
    }
}

#[test]
fn logs_tail_is_byte_exact_at_every_utf8_boundary_and_rejects_regrown_cursors() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let bytes = b"A\xf0\x9f\x99\x82\xc3\xa9\xff\x00\n";
    s.logs.lock().unwrap().extend_from_slice(bytes);
    let (tail, e) = call(&mut s, "logs_tail", serde_json::json!({"max_bytes":3}));
    assert!(!e);
    assert_eq!(tail["start_offset"], bytes.len() - 3);
    assert_eq!(tail["data_b64"], base64_encode(&bytes[bytes.len() - 3..]));
    let session_id = s.session_id.clone();
    let mut cursor = serde_json::json!({"session_id":session_id,"generation":0,"offset":0});
    for (offset, byte) in bytes.iter().enumerate() {
        let (v, e) = call(
            &mut s,
            "logs_tail",
            serde_json::json!({"max_bytes":1,"cursor":cursor}),
        );
        assert!(!e, "{v}");
        assert_eq!(v["bytes"], 1);
        assert_eq!(v["start_offset"], offset);
        assert_eq!(v["data_b64"], base64_encode(&[*byte]));
        assert_eq!(v["text_lossy"], std::str::from_utf8(&[*byte]).is_err());
        cursor = v["next_cursor"].clone();
    }
    let (empty, e) = call(&mut s, "logs_tail", serde_json::json!({"cursor":cursor}));
    assert!(!e);
    assert_eq!(empty["bytes"], 0);
    assert_eq!(empty["has_more"], false);
    assert_eq!(
        s.logs.lock().unwrap().as_slice(),
        bytes,
        "tail must not clear the legacy buffer"
    );
    s.logs.lock().unwrap().extend_from_slice(b"Z");
    let (appended, e) = call(&mut s, "logs_tail", serde_json::json!({"cursor":cursor}));
    assert!(!e);
    assert_eq!(appended["data_b64"], "Wg==");
    let (_, e) = call(&mut s, "logs", serde_json::json!({"clear":true}));
    assert!(!e);
    s.logs.lock().unwrap().extend_from_slice(&[b'R'; 128]);
    let (stale, e) = call(&mut s, "logs_tail", serde_json::json!({"cursor":cursor}));
    assert!(e);
    assert_eq!(stale["code"], "STALE_LOG_CURSOR");
    let (invalid, e) = call(
        &mut s,
        "logs_tail",
        serde_json::json!({"cursor":{"session_id":session_id,"generation":1,"offset":u64::MAX}}),
    );
    assert!(e);
    assert_eq!(invalid["code"], "INVALID_LOG_CURSOR");
}

#[test]
fn session_info_is_available_without_app_and_does_not_expose_sandbox_paths() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let policy = Policy {
        max_wait_ms: 123,
        max_batch_steps: 2,
        ..Policy::default()
    };
    let mut s = AgentSession::new().with_policy(policy);
    let (v, e) = call(&mut s, "session_info", serde_json::json!({}));
    assert!(!e, "{v}");
    assert_eq!(v["running"], false);
    assert_eq!(v["tool_count"], tool_defs().len());
    assert_eq!(v["limits"]["max_wait_ms"], 123);
    assert_eq!(v["limits"]["max_batch_steps"], 2);
    assert_eq!(
        v["capabilities"]["native_engines_compiled"],
        cfg!(feature = "native-engines")
    );
    assert!(!v
        .to_string()
        .contains(std::env::current_dir().unwrap().to_str().unwrap()));
    assert!(s.engine.is_none());
    assert_eq!(s.ui_epoch, 0);
}

#[test]
fn last_error_captures_early_returns_batch_children_and_survives_success_until_clear() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let (v, e) = call(&mut s, "last_error", serde_json::json!({}));
    assert!(!e);
    assert!(v["error"].is_null());
    let secret = "DO_NOT_RETAIN_THE_INPUT_ARGUMENTS";
    let (original, e) = call(&mut s, "tap", serde_json::json!({"x":secret,"y":0}));
    assert!(e);
    let (_, e) = call(&mut s, "package_list", serde_json::json!({}));
    assert!(!e);
    let (v, e) = call(&mut s, "last_error", serde_json::json!({}));
    assert!(!e);
    assert_eq!(v["error"]["tool"], "tap");
    assert_eq!(v["error"]["code"], original["code"]);
    assert_eq!(v["error"]["cause"], original["cause"]);
    assert!(!v.to_string().contains(secret));
    assert_eq!(v["tool_error_count"], 1);
    let (_, e) = call(&mut s, "launch", serde_json::json!({"engine":true}));
    assert!(e);
    let (v, _) = call(&mut s, "last_error", serde_json::json!({}));
    assert_eq!(v["error"]["tool"], "launch");
    s.call_tool(
        "batch",
        &serde_json::json!({"calls":[{"name":"wait","arguments":{"ms":0}}]}),
    );
    let (v, e) = call(&mut s, "last_error", serde_json::json!({"clear":true}));
    assert!(!e);
    assert_eq!(v["error"]["tool"], "wait");
    assert_eq!(v["error"]["code"], "NOT_RUNNING");
    let count = v["tool_error_count"].clone();
    let (v, _) = call(&mut s, "last_error", serde_json::json!({}));
    assert!(v["error"].is_null());
    assert_eq!(v["tool_error_count"], count);
}

#[test]
fn error_snapshot_bounds_unicode_context_without_changing_the_original_wire_error() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let error = agent_error("TEST_ERROR", "🙂".repeat(2048), None);
    let wire = s.tool_error_result("example", &error);
    let original: serde_json::Value = serde_json::from_str(match &wire.content[0] {
        ToolContent::Text(s) => s,
        _ => unreachable!(),
    })
    .unwrap();
    assert_eq!(original["cause"].as_str().unwrap().len(), 8192);
    let (v, e) = call(&mut s, "last_error", serde_json::json!({}));
    assert!(!e);
    assert_eq!(v["error"]["cause"].as_str().unwrap().len(), 4096);
    assert_eq!(v["error"]["cause_bytes"], 8192);
    assert_eq!(v["error"]["truncated"], true);
}

#[test]
fn isolated_panic_retains_context_and_diagnostics_survive_a_poisoned_log_buffer() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    s.engine = Some(Engine::new(vec![], VmConfig::default()));
    s.engine_package = Some("p".into());
    s.native_engine = Some("c-ndk".into());
    let logs = s.logs.clone();
    let _ = std::thread::spawn(move || {
        let _guard = logs.lock().unwrap();
        panic!("injected log poison");
    })
    .join();
    let result = s.call_tool_isolated("logs", &serde_json::json!({}));
    assert!(result.is_error);
    assert!(s.engine.is_none());
    assert!(s.native_engine.is_none());
    let (v, e) = call(&mut s, "last_error", serde_json::json!({}));
    assert!(!e, "{v}");
    assert_eq!(v["error"]["code"], "INTERNAL_PANIC");
    assert_eq!(v["error"]["running_package"], "p");
    let (info, e) = call(&mut s, "session_info", serde_json::json!({}));
    assert!(!e);
    assert_eq!(info["running"], false);
    assert_eq!(info["logs"]["available"], false);
    let (v, e) = call(&mut s, "logs_tail", serde_json::json!({}));
    assert!(e);
    assert_eq!(v["code"], "LOG_BUFFER_UNAVAILABLE");
}

#[test]
fn batch_isolates_a_panicking_step_and_continues() {
    // Issue #317: o batch chamava `call_tool` cru — um panic em
    // qualquer passo escapava do isolamento e matava o lote inteiro
    // (e o processo). Cada passo agora é isolado: o que explode vira
    // INTERNAL_PANIC no próprio passo e o lote continua. (O helper
    // `call` dos testes chama call_tool CRU: se o isolamento sumir
    // do batch, ESTE teste entra em panic em vez de falhar limpo.)
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    // Mesmo gatilho do teste de isolamento acima: buffer de logs
    // envenenado faz a tool `logs` entrar em panic.
    let logs = s.logs.clone();
    let _ = std::thread::spawn(move || {
        let _guard = logs.lock().unwrap();
        panic!("injected log poison");
    })
    .join();
    let (v, err) = call(
        &mut s,
        "batch",
        serde_json::json!({"stop_on_error": false, "calls": [
            {"name": "logs", "arguments": {}},
            {"name": "session_info", "arguments": {}}
        ]}),
    );
    assert!(!err, "batch: {v}");
    assert_eq!(v["executed"], 2, "{v}");
    assert_eq!(v["steps"][0]["ok"], false, "{v}");
    assert_eq!(v["steps"][0]["result"]["code"], "INTERNAL_PANIC", "{v}");
    assert_eq!(v["steps"][1]["ok"], true, "{v}");
}

#[test]
fn runtime_diagnostics_preserve_ui_revision_clock_fuel_and_gc_state() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut s = AgentSession::new();
    let apk = fixture_apk_path();
    assert!(
        !s.call_tool("install", &serde_json::json!({"apk":apk}))
            .is_error
    );
    assert!(!s.call_tool("launch", &serde_json::json!({})).is_error);
    let (before, e) = call(
        &mut s,
        "find_views",
        serde_json::json!({"text":"Olá do MCP"}),
    );
    assert!(!e);
    let engine = s.engine.as_ref().unwrap();
    let clock = engine.fw.clock();
    let fuel = engine.fuel_used();
    let gc = serde_json::to_value(engine.gc_stats()).unwrap();
    for tool in ["session_info", "logs_tail", "last_error"] {
        let (v, e) = call(&mut s, tool, serde_json::json!({}));
        assert!(!e, "{v}");
    }
    let engine = s.engine.as_ref().unwrap();
    assert_eq!(engine.fw.clock(), clock);
    assert_eq!(engine.fuel_used(), fuel);
    assert_eq!(serde_json::to_value(engine.gc_stats()).unwrap(), gc);
    let (after, e) = call(
        &mut s,
        "find_views",
        serde_json::json!({"text":"Olá do MCP"}),
    );
    assert!(!e);
    assert_eq!(after["tree_token"], before["tree_token"]);
    std::fs::remove_file(apk).unwrap();
}

#[test]
fn log_cursors_cannot_silently_cross_session_boundaries() {
    let _guard = SESSION_LOCK.lock().unwrap();
    let mut a = AgentSession::new();
    let mut b = AgentSession::new();
    a.logs.lock().unwrap().extend_from_slice(b"first");
    b.logs.lock().unwrap().extend_from_slice(b"second-longer");
    let (v, e) = call(&mut a, "logs_tail", serde_json::json!({}));
    assert!(!e);
    let (v, e) = call(
        &mut b,
        "logs_tail",
        serde_json::json!({"cursor":v["next_cursor"]}),
    );
    assert!(e);
    assert_eq!(v["code"], "STALE_LOG_CURSOR");
}
