// Included in mcp.rs: Appendix E (GMS/Firebase) agent programming,
// multiplexed on `device_profile` via actions `gms` and `gms_state`.
//
// The spec (Appendix E) promises *agent-programmable* structured mock
// responses; until this, programming was only possible from Rust tests
// through `Engine::gms_*`. Everything here mutates/reads that same local
// mock state ("no-gms-honest"): no network, no real Google backend, and
// the results say so (`mode`/`network`) instead of pretending otherwise.

fn appendix_gms_schema_props() -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    for (k, ty) in [
        ("gms_availability", "integer"),
        ("gms_account_email", "string"),
        ("gms_account_name", "string"),
        ("gms_signin", "boolean"),
        ("firebase_strict_init", "boolean"),
        ("fcm_token", "string"),
        ("fcm_from", "string"),
        ("fcm_title", "string"),
        ("fcm_body", "string"),
        ("billing_outcome", "string"),
        ("billing_error_code", "integer"),
        ("ads_failure_code", "integer"),
        ("ads_clear_failure", "boolean"),
        ("integrity_token", "string"),
        ("attestation_jws", "string"),
        ("fit_steps", "integer"),
        ("cast_device", "string"),
        ("cast_disconnect", "boolean"),
    ] {
        m.insert(k.into(), serde_json::json!({ "type": ty }));
    }
    m.insert(
        "fcm_data".into(),
        serde_json::json!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        }),
    );
    m.insert(
        "billing_sku".into(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "type": { "type": "string", "enum": ["inapp", "subs"] },
                "price": { "type": "string" },
                "price_micros": { "type": "integer" },
                "currency": { "type": "string" },
                "title": { "type": "string" }
            },
            "required": ["id"]
        }),
    );
    m.insert(
        "billing_outcome".into(),
        serde_json::json!({
            "type": "string",
            "enum": ["purchase", "canceled", "error"]
        }),
    );
    m
}

/// `device_profile` schema = M10 props (AG-10) + Appendix E props, with
/// the two GMS actions appended to the action enum.
fn device_profile_all_props() -> serde_json::Value {
    let serde_json::Value::Object(mut props) = device_profile_schema_props() else {
        return device_profile_schema_props();
    };
    for (k, v) in appendix_gms_schema_props() {
        props.insert(k, v);
    }
    if let Some(enum_vals) = props
        .get_mut("action")
        .and_then(|a| a.get_mut("enum"))
        .and_then(|e| e.as_array_mut())
    {
        enum_vals.push(serde_json::json!("gms"));
        enum_vals.push(serde_json::json!("gms_state"));
    }
    serde_json::Value::Object(props)
}

impl AgentSession {
    /// `device_profile` entry point: GMS actions are handled here, all
    /// other actions keep delegating to the M10 implementation untouched.
    fn tool_device_profile(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        match args.get("action").and_then(|v| v.as_str()).unwrap_or("get") {
            "gms" => self.exec_gms_program(args),
            "gms_state" => self.gms_state_snapshot(),
            _ => self.tool_device_profile_m10(args),
        }
    }

    /// action=gms: apply the given programming fields to the running
    /// engine's Appendix E mock state. Only present fields are applied;
    /// the response lists exactly what was applied.
    fn exec_gms_program(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let e = self.require_engine()?;
        let mut applied: Vec<&'static str> = Vec::new();
        let get_str = |k: &str| args.get(k).and_then(|v| v.as_str());
        let get_i64 = |k: &str| args.get(k).and_then(|v| v.as_i64());
        let get_bool = |k: &str| args.get(k).and_then(|v| v.as_bool());

        if let Some(code) = get_i64("gms_availability") {
            e.gms_set_availability(code as i32);
            applied.push("gms_availability");
        }
        if let Some(email) = get_str("gms_account_email") {
            let name = get_str("gms_account_name").unwrap_or("");
            e.gms_program_google_account(email, name);
            applied.push("gms_account");
        }
        if let Some(signin) = get_bool("gms_signin") {
            if signin {
                let account = get_str("gms_account_email")
                    .map(|email| (email, get_str("gms_account_name").unwrap_or("")));
                e.gms_complete_signin(account);
                applied.push("gms_signin");
            } else {
                e.gms_sign_out_google();
                applied.push("gms_signout");
            }
        }
        if let Some(strict) = get_bool("firebase_strict_init") {
            e.gms_firebase_strict_init(strict);
            applied.push("firebase_strict_init");
        }
        if let Some(token) = get_str("fcm_token") {
            e.gms_set_fcm_token(token);
            applied.push("fcm_token");
        }
        if args.contains_key("fcm_data") || args.contains_key("fcm_title") {
            let data: Vec<(String, String)> = args
                .get("fcm_data")
                .and_then(|v| v.as_object())
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let notification = match (get_str("fcm_title"), get_str("fcm_body")) {
                (None, None) => None,
                (t, b) => Some((t.unwrap_or(""), b.unwrap_or(""))),
            };
            e.gms_deliver_remote_message(
                get_str("fcm_from").unwrap_or("rustydroid-agent"),
                data,
                notification,
            )
            .map_err(vm_exit_to_rd)?;
            applied.push("fcm_message");
        }
        if let Some(sku) = args.get("billing_sku").and_then(|v| v.as_object()) {
            let s = |k: &str| sku.get(k).and_then(|v| v.as_str()).unwrap_or("");
            let id = s("id");
            if id.is_empty() {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    "billing_sku exige \"id\" (ex.: coins_100)",
                    Some("veja o schema da tool device_profile"),
                ));
            }
            e.gms_billing_add_sku(
                id,
                if s("type").is_empty() {
                    "inapp"
                } else {
                    s("type")
                },
                s("price"),
                sku.get("price_micros")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                if s("currency").is_empty() {
                    "BRL"
                } else {
                    s("currency")
                },
                s("title"),
            );
            applied.push("billing_sku");
        }
        if let Some(outcome) = get_str("billing_outcome") {
            use rd_vm::gms::BillingOutcome;
            let o = match outcome {
                "purchase" => BillingOutcome::Purchase,
                "canceled" => BillingOutcome::Canceled,
                "error" => BillingOutcome::Error(get_i64("billing_error_code").unwrap_or(6) as i32),
                other => {
                    return Err(agent_error(
                        "INVALID_FORMAT",
                        format!("billing_outcome {other:?} inválido (purchase|canceled|error)"),
                        None,
                    ))
                }
            };
            e.gms_billing_set_outcome(o);
            applied.push("billing_outcome");
        }
        if let Some(code) = get_i64("ads_failure_code") {
            e.gms_ads_set_failure(code as i32);
            applied.push("ads_failure");
        }
        if get_bool("ads_clear_failure") == Some(true) {
            e.gms_ads_clear_failure();
            applied.push("ads_clear_failure");
        }
        if let Some(token) = get_str("integrity_token") {
            e.gms_set_integrity_token(token);
            applied.push("integrity_token");
        }
        if let Some(jws) = get_str("attestation_jws") {
            e.gms_set_attestation_jws(jws);
            applied.push("attestation_jws");
        }
        if let Some(steps) = get_i64("fit_steps") {
            e.gms_fit_set_steps(steps as i32);
            applied.push("fit_steps");
        }
        if let Some(name) = get_str("cast_device") {
            e.gms_cast_connect(name);
            applied.push("cast_connect");
        }
        if get_bool("cast_disconnect") == Some(true) {
            e.gms_cast_disconnect();
            applied.push("cast_disconnect");
        }

        Ok(ToolResult::json(&serde_json::json!({
            "applied": applied,
            "mode": "no-gms-honest",
            "network": "none",
            "hint": "action=gms_state devolve o estado capturado/programado; nada disto é um backend Google real",
        })))
    }

    /// action=gms_state: captured + programmed Appendix E state of the
    /// running engine (events are real captures of what the app did).
    fn gms_state_snapshot(&mut self) -> Result<ToolResult, RdError> {
        let e = self.require_engine()?;
        let analytics: Vec<serde_json::Value> = e
            .gms_analytics_events()
            .iter()
            .map(|ev| {
                serde_json::json!({
                    "name": ev.name,
                    "params": ev.params.iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>(),
                })
            })
            .collect();
        let purchases: Vec<serde_json::Value> = e
            .gms_billing_purchases()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "token": p.token,
                    "products": p.products,
                    "state": p.state,
                    "acknowledged": p.acknowledged,
                    "consumed": p.consumed,
                    "order_id": p.order_id,
                })
            })
            .collect();
        let cameras: Vec<serde_json::Value> = e
            .gms_maps_cameras()
            .iter()
            .map(|(lat, lng, zoom)| serde_json::json!({ "lat": lat, "lng": lng, "zoom": zoom }))
            .collect();
        let markers: Vec<serde_json::Value> = e
            .gms_maps_markers()
            .iter()
            .map(|(lat, lng, title)| serde_json::json!({ "lat": lat, "lng": lng, "title": title }))
            .collect();
        Ok(ToolResult::json(&serde_json::json!({
            "mode": "no-gms-honest",
            "network": "none",
            "availability_code": e.fw.gms.availability_code,
            "analytics_events": analytics,
            "analytics_user_props": e.gms_analytics_user_props().iter()
                .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                .collect::<serde_json::Map<String, serde_json::Value>>(),
            "fcm_topics": e.gms_fcm_topics(),
            "billing_purchases": purchases,
            "ads_events": e.gms_ads_events(),
            "crash_summaries": e.gms_crashlytics_crash_summaries(),
            "maps_cameras": cameras,
            "maps_markers": markers,
            "integrity_nonces": e.gms_integrity_nonces(),
            "location_callbacks": e.gms_location_callback_count(),
            "pending_tasks": e.gms_pending_tasks(),
        })))
    }
}
