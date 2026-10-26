#![cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
//! M10 (`AG-10`) MCP integration tests for `device_profile` virtual-device multiplexing.

use rd_agent::mcp::{tool_defs, AgentSession, ToolContent};
use serde_json::{json, Value};

fn call(session: &mut AgentSession, name: &str, args: Value) -> (Value, bool) {
    let res = session.call_tool(name, &args);
    let val = match &res.content[0] {
        ToolContent::Text(txt) => serde_json::from_str(txt).unwrap_or_else(|_| json!(txt)),
        ToolContent::Image { mime, .. } => json!({ "image": mime }),
    };
    (val, res.is_error)
}

fn write_temp_apk(tag: &str, bytes: Vec<u8>) -> String {
    let dir = std::env::temp_dir().join(format!("rd-m10-mcp-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("app.apk");
    std::fs::write(&path, bytes).expect("write apk");
    path.to_string_lossy().into_owned()
}

#[test]
fn m10_catalog_preserves_47_tools_and_exposes_all_18_actions() {
    let defs = tool_defs();
    assert_eq!(defs.len(), 47, "MCP tool count contract is locked at 47");

    let dp = defs
        .iter()
        .find(|d| d.name == "device_profile")
        .expect("device_profile registered");
    let actions = dp.schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum");
    let expected = [
        "get",
        "location",
        "sensor",
        "battery",
        "network",
        "telephony",
        "sms_receive",
        "ble_advertisement",
        "nfc_tag",
        "camera_frame",
        "biometric",
        "audio_input",
        "media_action",
        "dark_mode",
        "locale",
        "font_scale",
        "orientation",
        "clipboard",
    ];
    assert_eq!(actions.len(), expected.len());
    for name in expected {
        assert!(
            actions.iter().any(|v| v.as_str() == Some(name)),
            "missing action {name} in device_profile schema"
        );
    }
}

#[test]
fn m10_device_profile_no_app_contract_and_mutation_rejection() {
    let mut s = AgentSession::new();

    // Legacy read-only contract without installed app (`contract-no-app.json`)
    let (v0, err0) = call(&mut s, "device_profile", json!({}));
    assert!(!err0, "{v0}");
    assert_eq!(v0["id"], "moto-e5");
    assert_eq!(v0["hardware_floor"], true);
    assert!(v0["virtual_devices"].is_null(), "{v0}");

    let (v1, err1) = call(&mut s, "device_profile", json!({ "action": "get" }));
    assert!(!err1, "{v1}");
    assert_eq!(v1["id"], "moto-e5");
    assert!(v1["virtual_devices"].is_null(), "{v1}");

    // Mutating action before launch -> NOT_RUNNING
    let (err_no_run, is_err) = call(
        &mut s,
        "device_profile",
        json!({ "action": "location", "latitude": -18.24, "longitude": -43.60 }),
    );
    assert!(is_err, "{err_no_run}");
    assert_eq!(err_no_run["code"], "NOT_RUNNING", "{err_no_run}");

    // Install without launch -> mutating action still returns NOT_RUNNING
    let apk_path = write_temp_apk("not-running", rd_testsupport::m9_app::apk());
    let (inst, inst_err) = call(&mut s, "install", json!({ "apk": apk_path }));
    assert!(!inst_err, "{inst}");

    let (err_not_run2, is_err2) = call(
        &mut s,
        "device_profile",
        json!({ "action": "battery", "level": 50 }),
    );
    assert!(is_err2, "{err_not_run2}");
    assert_eq!(err_not_run2["code"], "NOT_RUNNING", "{err_not_run2}");
}

#[test]
fn m10_all_mutating_actions_drive_live_vm_and_update_snapshot() {
    let mut s = AgentSession::new();
    let apk_path = write_temp_apk("live", rd_testsupport::m9_app::apk());
    let (inst, inst_err) = call(&mut s, "install", json!({ "apk": apk_path }));
    assert!(!inst_err, "{inst}");
    let (lnch, lnch_err) = call(&mut s, "launch", json!({}));
    assert!(!lnch_err, "{lnch}");

    let mutations = vec![
        json!({ "action": "sensor", "sensor": "accelerometer", "values": [1.5, -2.5, 9.5], "accuracy": 3 }),
        json!({ "action": "location", "latitude": -23.5505, "longitude": -46.6333, "altitude": 760.0, "accuracy": 4.0, "speed": 1.2, "bearing": 90.0 }),
        json!({
            "action": "location",
            "route": [
                { "latitude": -23.5505, "longitude": -46.6333, "altitude": 760.0, "speed": 2.0 },
                { "latitude": -23.5605, "longitude": -46.6433, "altitude": 765.0, "speed": 2.5 }
            ]
        }),
        json!({ "action": "location", "step_route": true }),
        json!({ "action": "camera_frame", "camera_id": "0", "width": 64, "height": 64, "format": 256, "bytes_hex": "FFD84D3130FFD9" }),
        json!({ "action": "biometric", "hardware_present": true, "enrolled": true, "outcome": "success", "persist": true }),
        json!({ "action": "audio_input", "pcm_hex": "0102030405060708" }),
        json!({ "action": "media_action", "command": "play" }),
        json!({ "action": "network", "online": true, "transport": "cellular", "metered": true, "airplane_mode": false }),
        json!({ "action": "telephony", "sim_state": 5, "carrier_name": "Vivo", "network_type": 20, "call_state": 0, "roaming": false }),
        json!({ "action": "sms_receive", "sender": "+5511999990000", "text": "OTP 9012" }),
        json!({ "action": "ble_advertisement", "address": "11:22:33:44:55:66", "name": "RD-Tag", "rssi": -48, "payload_hex": "02010603030F18" }),
        json!({ "action": "nfc_tag", "id_hex": "04AABBCCDD", "text": "hello-m10-nfc", "uri": "https://example.org/nfc" }),
        json!({ "action": "battery", "level": 42, "charging": true, "plugged": 2, "temperature_tenths_c": 310, "power_save_mode": true }),
        json!({ "action": "dark_mode", "enabled": true }),
        json!({ "action": "locale", "locale": "pt-BR" }),
        json!({ "action": "font_scale", "scale": 1.25 }),
        json!({ "action": "orientation", "orientation": "landscape" }),
        json!({ "action": "clipboard", "label": "otp", "text": "copied-via-ag10" }),
    ];

    for payload in mutations {
        let act = payload["action"].as_str().unwrap_or("").to_string();
        let (res, is_err) = call(&mut s, "device_profile", payload);
        assert!(!is_err, "mutation {act} failed: {res}");
        assert_eq!(res["action"], act, "mutation {act}: {res}");
        assert!(res["virtual_devices"].is_object(), "mutation {act}: {res}");
    }

    // Verify invalid action and malformed arguments return typed INVALID_ARG
    let (bad_act, bad_err) = call(&mut s, "device_profile", json!({ "action": "teleport" }));
    assert!(bad_err, "{bad_act}");
    assert_eq!(bad_act["code"], "INVALID_ARG");

    let (bad_hex, bad_hex_err) = call(
        &mut s,
        "device_profile",
        json!({ "action": "audio_input", "pcm_hex": "ZZZ" }),
    );
    assert!(bad_hex_err, "{bad_hex}");
    assert_eq!(bad_hex["code"], "INVALID_ARG");

    // Verify full live snapshot reflects every mutated virtual device subsystem
    let (snap, snap_err) = call(&mut s, "device_profile", json!({ "action": "get" }));
    assert!(!snap_err, "{snap}");
    assert_eq!(snap["id"], "moto-e5");
    let vd = &snap["virtual_devices"];
    assert!(vd.is_object(), "expected virtual_devices object: {snap}");
    assert_eq!(vd["location"]["route_remaining"], 0);
    assert!((vd["location"]["latitude"].as_f64().unwrap() - (-23.5605)).abs() < 1e-4);
    assert_eq!(vd["battery"]["level"], 42);
    assert_eq!(vd["battery"]["charging"], true);
    assert_eq!(vd["battery"]["power_save_mode"], true);
    assert_eq!(vd["network"]["transport"], "cellular");
    assert_eq!(vd["network"]["metered"], true);
    assert_eq!(vd["telephony"]["carrier_name"], "Vivo");
    assert_eq!(vd["telephony"]["network_type"], 20);
    assert_eq!(vd["telephony"]["received_sms"], 1);
    assert_eq!(vd["bluetooth"]["advertisements"], 1);
    assert_eq!(vd["biometrics"]["hardware_present"], true);
    assert_eq!(vd["biometrics"]["enrolled"], true);
    assert_eq!(vd["media"]["pcm_input_queued_bytes"], 8);
    assert_eq!(vd["sysconfig"]["dark_mode"], true);
    assert_eq!(vd["sysconfig"]["locale"], "pt-BR");
    assert!((vd["sysconfig"]["font_scale"].as_f64().unwrap() - 1.25).abs() < 1e-4);
    assert_eq!(vd["sysconfig"]["orientation"], "landscape");
    assert_eq!(vd["sysconfig"]["screen"], json!([1440, 720]));
    assert_eq!(vd["sysconfig"]["clipboard"], "copied-via-ag10");
}
