// Included in mcp.rs: M10 (`AG-10`) virtual device controls multiplexed on `device_profile`.
fn device_profile_schema_props() -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (k, ty) in [
        ("latitude", "number"),
        ("longitude", "number"),
        ("altitude", "number"),
        ("accuracy", "number"),
        ("speed", "number"),
        ("bearing", "number"),
        ("step_route", "boolean"),
        ("sensor", "string"),
        ("sensor_type", "integer"),
        ("level", "integer"),
        ("charging", "boolean"),
        ("plugged", "integer"),
        ("temperature_tenths_c", "integer"),
        ("power_save_mode", "boolean"),
        ("online", "boolean"),
        ("transport", "string"),
        ("metered", "boolean"),
        ("airplane_mode", "boolean"),
        ("sim_state", "integer"),
        ("carrier_name", "string"),
        ("network_type", "integer"),
        ("call_state", "integer"),
        ("roaming", "boolean"),
        ("sender", "string"),
        ("text", "string"),
        ("label", "string"),
        ("address", "string"),
        ("name", "string"),
        ("rssi", "integer"),
        ("payload_hex", "string"),
        ("id_hex", "string"),
        ("uri", "string"),
        ("camera_id", "string"),
        ("width", "integer"),
        ("height", "integer"),
        ("format", "integer"),
        ("bytes_hex", "string"),
        ("hardware_present", "boolean"),
        ("enrolled", "boolean"),
        ("outcome", "string"),
        ("persist", "boolean"),
        ("pcm_hex", "string"),
        ("command", "string"),
        ("seek_ms", "integer"),
        ("enabled", "boolean"),
        ("locale", "string"),
        ("scale", "number"),
        ("orientation", "string"),
    ] {
        m.insert(k.into(), serde_json::json!({ "type": ty }));
    }
    m.insert(
        "action".into(),
        serde_json::json!({
            "type": "string",
            "enum": [
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
                "clipboard"
            ]
        }),
    );
    m.insert(
        "route".into(),
        serde_json::json!({ "type": "array", "items": { "type": "object" } }),
    );
    m.insert(
        "values".into(),
        serde_json::json!({ "type": "array", "items": { "type": "number" } }),
    );
    serde_json::Value::Object(m)
}

impl AgentSession {
    fn tool_device_profile_m10(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("get");
        if action != "get" {
            return self.exec_m10_action(action, args);
        }
        let d = self.devices.get("moto-e5").cloned().ok_or_else(|| {
            agent_error(
                "INTERNAL",
                "perfil moto-e5 ausente dos dados embutidos",
                None,
            )
        })?;
        #[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
        let virtual_devices = self.m10_snapshot_json();
        #[cfg(not(any(feature = "advanced-components", feature = "virtual-devices")))]
        let virtual_devices = serde_json::Value::Null;

        Ok(ToolResult::json(&serde_json::json!({
            "id": d.id,
            "label": d.label,
            "class": d.class,
            "screen": [d.width_px, d.height_px],
            "density_dpi": d.density_dpi,
            "refresh_hz": d.refresh_hz,
            "cpu_cores": d.cpu_cores,
            "hardware_floor": d.is_hardware_floor(),
            "budgets": d.budget_summary(),
            "virtual_devices": virtual_devices,
        })))
    }

    #[cfg(not(any(feature = "advanced-components", feature = "virtual-devices")))]
    fn exec_m10_action(
        &mut self,
        _action: &str,
        _args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        Err(agent_error(
            "NOT_IMPLEMENTED",
            "M10 virtual device controls require feature `virtual-devices`",
            None,
        ))
    }

    #[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
    fn m10_snapshot_json(&mut self) -> serde_json::Value {
        let Some(e) = self.engine.as_mut() else {
            return serde_json::Value::Null;
        };
        let clip = e.m10_get_clipboard().ok().flatten();
        let st = &e.fw.vdev;
        let sensors_summary: serde_json::Map<String, serde_json::Value> = st
            .sensors
            .iter()
            .map(|(k, s)| {
                (
                    k.to_string(),
                    serde_json::json!({
                        "name": s.name,
                        "values": s.values,
                        "accuracy": s.accuracy,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "location": {
                "provider": st.location.current.provider,
                "latitude": st.location.current.latitude,
                "longitude": st.location.current.longitude,
                "altitude": st.location.current.altitude,
                "accuracy": st.location.current.accuracy,
                "speed": st.location.current.speed,
                "bearing": st.location.current.bearing,
                "listeners": st.location.listeners.len(),
                "route_remaining": st.location.route.len().saturating_sub(st.location.route_cursor),
            },
            "sensors": sensors_summary,
            "sensor_listeners": st.sensor_listeners.len(),
            "battery": {
                "level": st.battery.level,
                "charging": st.battery.charging,
                "plugged": st.battery.plugged,
                "temperature_tenths_c": st.battery.temperature_tenths_c,
                "power_save_mode": st.power.power_save_mode,
                "held_wake_locks": st.wake_locks.values().filter(|w| w.held).count(),
            },
            "network": {
                "online": st.network.online,
                "transport": st.network.transport,
                "metered": st.network.metered,
                "airplane_mode": st.network.airplane_mode,
            },
            "telephony": {
                "sim_state": st.telephony.sim_state,
                "carrier_name": st.telephony.carrier_name,
                "network_type": st.telephony.network_type,
                "call_state": st.telephony.call_state,
                "sent_sms": st.telephony.sent_sms.len(),
                "received_sms": st.telephony.received_sms.len(),
            },
            "bluetooth": {
                "enabled": st.bluetooth_enabled,
                "active_ble_scans": st.ble_scan_callbacks.len(),
                "advertisements": st.ble_advertisements.len(),
            },
            "nfc": {
                "enabled": st.nfc_enabled,
                "reader_mode": st.nfc_reader_callback.is_some(),
            },
            "biometrics": {
                "hardware_present": st.biometrics.hardware_present,
                "enrolled": st.biometrics.enrolled,
                "prompt_active": st.biometrics.active_prompt.is_some(),
                "auth_attempts": st.biometrics.auth_attempts,
            },
            "media": {
                "ringer_mode": st.audio.ringer_mode,
                "active_players": st.media_players.len(),
                "active_sessions": st.media_sessions.values().filter(|s| s.active).count(),
                "active_projections": st.media_projections.values().filter(|p| !p.stopped).count(),
                "pcm_input_queued_bytes": st.audio.pcm_input_queue.len(),
            },
            "sysconfig": {
                "dark_mode": st.sysconfig.dark_mode,
                "locale": st.sysconfig.locale_tag,
                "font_scale": st.sysconfig.font_scale,
                "orientation": if st.sysconfig.orientation == 2 { "landscape" } else { "portrait" },
                "screen": [st.sysconfig.width_px, st.sysconfig.height_px],
                "clipboard": clip,
            },
        })
    }

    #[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
    fn exec_m10_action(
        &mut self,
        action: &str,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let e = self.require_engine()?;
        e.m10_enable_virtual_devices();
        let mut callbacks = 0usize;
        match action {
            "location" => {
                if let Some(route_arr) = args.get("route").and_then(|v| v.as_array()) {
                    let mut wps = Vec::with_capacity(route_arr.len());
                    for item in route_arr {
                        let lat = item
                            .get("latitude")
                            .and_then(|v| v.as_f64())
                            .ok_or_else(|| {
                                agent_error("INVALID_ARG", "route waypoint requires latitude", None)
                            })?;
                        let lon = item
                            .get("longitude")
                            .and_then(|v| v.as_f64())
                            .ok_or_else(|| {
                                agent_error(
                                    "INVALID_ARG",
                                    "route waypoint requires longitude",
                                    None,
                                )
                            })?;
                        let alt = item.get("altitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let spd = item.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        wps.push((lat, lon, alt, spd));
                    }
                    callbacks = e.m10_set_location_route(wps).map_err(vm_exit_to_rd)?;
                } else if args.get("step_route").and_then(|v| v.as_bool()) == Some(true) {
                    callbacks = e.m10_step_location_route().map_err(vm_exit_to_rd)?;
                } else {
                    let lat = m10_f64(args, "latitude")?;
                    let lon = m10_f64(args, "longitude")?;
                    let alt = args.get("altitude").and_then(|v| v.as_f64());
                    let acc = args.get("accuracy").and_then(|v| v.as_f64()).map(|v| v as f32);
                    let spd = args.get("speed").and_then(|v| v.as_f64()).map(|v| v as f32);
                    let brg = args.get("bearing").and_then(|v| v.as_f64()).map(|v| v as f32);
                    callbacks = e
                        .m10_set_location(lat, lon, alt, acc, spd, brg)
                        .map_err(vm_exit_to_rd)?;
                }
            }
            "sensor" => {
                let stype = if let Some(s) = args.get("sensor").and_then(|v| v.as_str()) {
                    rd_vm::m10::sensors::sensor_type_from_name(s).ok_or_else(|| {
                        agent_error("INVALID_ARG", format!("unknown sensor name: {s}"), None)
                    })?
                } else {
                    m10_i32(args, "sensor_type")?
                };
                let vals_json = args
                    .get("values")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        agent_error("INVALID_ARG", "values float array required", None)
                    })?;
                let mut vals = Vec::with_capacity(vals_json.len());
                for v in vals_json {
                    let f = v.as_f64().ok_or_else(|| {
                        agent_error("INVALID_ARG", "sensor values must be numeric", None)
                    })? as f32;
                    vals.push(f);
                }
                let acc = args
                    .get("accuracy")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok());
                callbacks = e
                    .m10_inject_sensor(stype, vals, acc)
                    .map_err(vm_exit_to_rd)?;
            }
            "battery" => {
                let level = m10_i32(args, "level")?;
                let charging = args
                    .get("charging")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let plugged = args
                    .get("plugged")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok());
                let temp = args
                    .get("temperature_tenths_c")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok());
                let psm = args.get("power_save_mode").and_then(|v| v.as_bool());
                callbacks = e
                    .m10_set_battery(level, charging, plugged, temp, psm)
                    .map_err(vm_exit_to_rd)?;
            }
            "network" => {
                let online = args.get("online").and_then(|v| v.as_bool()).unwrap_or(true);
                let transport = args
                    .get("transport")
                    .and_then(|v| v.as_str())
                    .unwrap_or("wifi");
                let metered = args
                    .get("metered")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let airplane = args
                    .get("airplane_mode")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                callbacks = e
                    .m10_set_network(online, transport, metered, airplane)
                    .map_err(vm_exit_to_rd)?;
            }
            "telephony" => {
                if let Some(sim) = args
                    .get("sim_state")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                {
                    e.fw.vdev.telephony.sim_state = sim;
                }
                if let Some(carrier) = args.get("carrier_name").and_then(|v| v.as_str()) {
                    e.fw.vdev.telephony.carrier_name = carrier.to_string();
                }
                if let Some(nt) = args
                    .get("network_type")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                {
                    e.fw.vdev.telephony.network_type = nt;
                }
                if let Some(cs) = args
                    .get("call_state")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                {
                    e.fw.vdev.telephony.call_state = cs;
                }
                if let Some(roam) = args.get("roaming").and_then(|v| v.as_bool()) {
                    e.fw.vdev.telephony.roaming = roam;
                }
            }
            "sms_receive" => {
                let sender = m10_str(args, "sender")?;
                let text = m10_str(args, "text")?;
                callbacks = e.m10_receive_sms(&sender, &text).map_err(vm_exit_to_rd)?;
            }
            "ble_advertisement" => {
                let addr = args
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("AA:BB:CC:DD:EE:01");
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("RD-BLE-Sensor");
                let rssi = args
                    .get("rssi")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(-55);
                let payload = m10_hex_or_utf8(args, "payload_hex", b"\x02\x01\x06")?;
                callbacks = e
                    .m10_inject_ble_advertisement(addr, name, rssi, payload)
                    .map_err(vm_exit_to_rd)?;
            }
            "nfc_tag" => {
                let tag_id = m10_hex_or_utf8(args, "id_hex", &[0x04, 0xA1, 0xB2, 0xC3])?;
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let uri = args.get("uri").and_then(|v| v.as_str()).map(str::to_string);
                callbacks = e
                    .m10_inject_nfc_tag(tag_id, text, uri)
                    .map_err(vm_exit_to_rd)?;
            }
            "camera_frame" => {
                let cam_id = args
                    .get("camera_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0")
                    .to_string();
                let w = args
                    .get("width")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(64);
                let h = args
                    .get("height")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(64);
                let fmt = args
                    .get("format")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(256);
                let bytes = m10_hex_or_utf8(
                    args,
                    "bytes_hex",
                    &[0xFF, 0xD8, b'R', b'D', 0xFF, 0xD9],
                )?;
                callbacks = e
                    .m10_inject_camera_frame(&cam_id, w, h, fmt, bytes)
                    .map_err(vm_exit_to_rd)?;
            }
            "biometric" => {
                if let Some(hw) = args.get("hardware_present").and_then(|v| v.as_bool()) {
                    let enr = args.get("enrolled").and_then(|v| v.as_bool()).unwrap_or(hw);
                    e.m10_set_biometric_hardware(hw, enr);
                }
                let persist = args
                    .get("persist")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if let Some(out_str) = args.get("outcome").and_then(|v| v.as_str()) {
                    let outcome = match out_str {
                        "success" => rd_vm::m10::BiometricOutcome::Success,
                        "fail" | "failed" => rd_vm::m10::BiometricOutcome::Failed,
                        "cancel" | "canceled" => rd_vm::m10::BiometricOutcome::Error {
                            code: 10,
                            message: "User canceled biometric prompt".into(),
                        },
                        "lockout" => rd_vm::m10::BiometricOutcome::Error {
                            code: 7,
                            message: "Too many failed attempts".into(),
                        },
                        "pending" => rd_vm::m10::BiometricOutcome::Pending,
                        _ => {
                            return Err(agent_error(
                                "INVALID_ARG",
                                format!("unknown biometric outcome: {out_str}"),
                                None,
                            ))
                        }
                    };
                    let delivered = e
                        .m10_trigger_biometric(outcome, persist)
                        .map_err(vm_exit_to_rd)?;
                    callbacks = usize::from(delivered);
                }
            }
            "audio_input" => {
                let pcm = m10_hex_or_utf8(args, "pcm_hex", &[0u8; 64])?;
                e.m10_push_audio_input(&pcm);
            }
            "media_action" => {
                let cmd = m10_str(args, "command")?;
                let seek = args.get("seek_ms").and_then(|v| v.as_i64());
                callbacks = e.m10_media_action(&cmd, seek).map_err(vm_exit_to_rd)?;
            }
            "dark_mode" => {
                let enabled = args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| agent_error("INVALID_ARG", "enabled boolean required", None))?;
                e.m10_set_dark_mode(enabled).map_err(vm_exit_to_rd)?;
                callbacks = 1;
            }
            "locale" => {
                let tag = m10_str(args, "locale")?;
                e.m10_set_locale(&tag).map_err(vm_exit_to_rd)?;
                callbacks = 1;
            }
            "font_scale" => {
                let scale = m10_f64(args, "scale")? as f32;
                e.m10_set_font_scale(scale).map_err(vm_exit_to_rd)?;
                callbacks = 1;
            }
            "orientation" => {
                let ori = m10_str(args, "orientation")?;
                e.m10_set_orientation(&ori).map_err(vm_exit_to_rd)?;
                callbacks = 1;
            }
            "clipboard" => {
                let label = args
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("agent");
                let text = m10_str(args, "text")?;
                e.m10_set_clipboard(label, &text).map_err(vm_exit_to_rd)?;
            }
            other => {
                return Err(agent_error(
                    "INVALID_ARG",
                    format!("unknown device_profile action: {other}"),
                    Some(
                        "valid actions: get, location, sensor, battery, network, telephony, sms_receive, ble_advertisement, nfc_tag, camera_frame, biometric, audio_input, media_action, dark_mode, locale, font_scale, orientation, clipboard",
                    ),
                ))
            }
        }
        // `device_profile` is in `READ_ONLY_TOOLS` for `{}`; invalidate cached screen on mutations.
        self.ui_epoch = self.ui_epoch.saturating_add(1);
        self.screen.dirty = true;
        let snap = self.m10_snapshot_json();
        Ok(ToolResult::json(&serde_json::json!({
            "action": action,
            "callbacks_delivered": callbacks,
            "virtual_devices": snap,
        })))
    }
}

#[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
fn m10_i32(args: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<i32, RdError> {
    args.get(key)
        .and_then(|v| v.as_i64())
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| agent_error("INVALID_ARG", format!("{key} must be an i32 integer"), None))
}

#[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
fn m10_f64(args: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<f64, RdError> {
    args.get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| agent_error("INVALID_ARG", format!("{key} must be a number"), None))
}

#[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
fn m10_str(
    args: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, RdError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| agent_error("INVALID_ARG", format!("{key} string required"), None))
}

#[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
fn m10_hex_or_utf8(
    args: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    default_bytes: &[u8],
) -> Result<Vec<u8>, RdError> {
    let Some(s) = args.get(key).and_then(|v| v.as_str()) else {
        return Ok(default_bytes.to_vec());
    };
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return Err(agent_error(
            "INVALID_ARG",
            format!("{key} must have an even number of hex digits"),
            None,
        ));
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push(((h << 4) | l) as u8),
            _ => {
                return Err(agent_error(
                    "INVALID_ARG",
                    format!("{key} contains non-hex characters"),
                    None,
                ))
            }
        }
        i += 2;
    }
    Ok(out)
}
