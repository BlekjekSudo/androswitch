// Incremental, bounded diagnostics. None of these tools runs guest code.
const LOG_TAIL_MAX_BYTES: u64 = 65_536;

#[derive(Clone, serde::Serialize)]
struct LastToolError {
    sequence: u64,
    tool: String,
    code: String,
    cause: String,
    cause_bytes: usize,
    suggestion: Option<String>,
    module_id: String,
    running_package: Option<String>,
    active_package: Option<String>,
    clock_ms: Option<u64>,
    truncated: bool,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LogCursor {
    session_id: String,
    generation: u64,
    offset: u64,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeOptions {
    max_bytes: Option<u64>,
    cursor: Option<LogCursor>,
    clear: Option<bool>,
}

fn runtime_options(
    args: &serde_json::Map<String, serde_json::Value>,
    tool: &str,
) -> Result<RuntimeOptions, RdError> {
    let allowed: &[&str] = match tool {
        "logs_tail" => &["max_bytes", "cursor"],
        "last_error" => &["clear"],
        _ => &[],
    };
    for (key, value) in args {
        let valid = allowed.contains(&key.as_str())
            && match key.as_str() {
                "max_bytes" => value.as_u64().is_some(),
                "cursor" => value.is_object(),
                "clear" => value.is_boolean(),
                _ => false,
            };
        if !valid {
            return Err(agent_error(
                "INVALID_FORMAT",
                format!("{tool}: invalid or unknown argument {key:.64}"),
                None,
            ));
        }
    }
    let options: RuntimeOptions =
        serde_json::from_value(serde_json::Value::Object(args.clone()))
            .map_err(|e| agent_error("INVALID_FORMAT", format!("{tool}: {e}"), None))?;
    if options
        .max_bytes
        .is_some_and(|n| !(1..=LOG_TAIL_MAX_BYTES).contains(&n))
    {
        return Err(agent_error(
            "LIMIT_EXCEEDED",
            "max_bytes must be in 1..=65536",
            None,
        ));
    }
    Ok(options)
}

fn bounded_diagnostic(s: &str, cap: usize, truncated: &mut bool) -> String {
    let mut end = s.len().min(cap);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    *truncated |= end < s.len();
    s[..end].to_owned()
}

fn runtime_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "session_info",
            description: "Inspect session identity, active versus running package, compiled capabilities and policy limits without launching an app or collecting garbage. Sandbox roots are reported only as a count, never paths. This complements runtime_health; compiled capability flags do not promise runtime availability.",
            schema: serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
        },
        ToolDef {
            name: "logs_tail",
            description: "Read at most max_bytes raw log bytes (default 4096, cap 65536). Without cursor, returns the newest bytes; next_cursor supports lossless incremental reads. Cursors bind this session, a clear-generation and byte offset; logs(clear=true) invalidates old cursors even after the buffer regrows. data_b64 is authoritative across UTF-8 boundaries; text is explicitly lossy when needed. Read-only; does not clear or cap the underlying legacy log buffer.",
            schema: serde_json::json!({"type":"object","properties":{
                "max_bytes":{"type":"integer","minimum":1,"maximum":65536,"default":4096},
                "cursor":{"type":"object","properties":{"session_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"offset":{"type":"integer","minimum":0}},"required":["session_id","generation","offset"],"additionalProperties":false}
            },"additionalProperties":false}),
        },
        ToolDef {
            name: "last_error",
            description: "Inspect the latest typed tool failure, including early validation failures, batch child failures and isolated runtime panics. Retains bounded UTF-8 error context, not input arguments; does not separately capture logs or stack traces. Original error messages may contain app-provided text. Successful tools do not erase it. clear=true clears only the snapshot, not the cumulative error count. Protocol parse/method errors are not tool failures.",
            schema: serde_json::json!({"type":"object","properties":{"clear":{"type":"boolean","default":false}},"additionalProperties":false}),
        },
    ]
}

impl AgentSession {
    fn tool_error_result(&mut self, name: &str, error: &RdError) -> ToolResult {
        self.tool_error_count = self.tool_error_count.saturating_add(1);
        let mut truncated = false;
        let snapshot = LastToolError {
            sequence: self.tool_error_count,
            tool: bounded_diagnostic(name, 128, &mut truncated),
            code: bounded_diagnostic(&error.code, 128, &mut truncated),
            cause: bounded_diagnostic(&error.cause, 4096, &mut truncated),
            cause_bytes: error.cause.len(),
            suggestion: error
                .suggestion
                .as_deref()
                .map(|s| bounded_diagnostic(s, 4096, &mut truncated)),
            module_id: bounded_diagnostic(&error.module_id, 128, &mut truncated),
            running_package: self
                .engine_package
                .as_deref()
                .map(|s| bounded_diagnostic(s, 256, &mut truncated)),
            active_package: self
                .active
                .as_deref()
                .map(|s| bounded_diagnostic(s, 256, &mut truncated)),
            clock_ms: self.engine.as_ref().map(|e| e.fw.clock()),
            truncated,
        };
        self.last_tool_error = Some(snapshot);
        ToolResult::error(error)
    }

    fn tool_session_info(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        runtime_options(args, "session_info")?;
        let log_bytes = self.logs.lock().ok().map(|buf| buf.len());
        Ok(ToolResult::json(&serde_json::json!({
            "session_id":self.session_id,"server":SERVER_NAME,"version":SERVER_VERSION,
            "protocol_version":PROTOCOL_VERSION,"transport":"stdio","tool_count":tool_defs().len(),
            "installed_count":self.installed.len(),"active_package":self.active,
            "running":self.engine.is_some(),"running_package":self.engine_package,
            "native_engine":self.native_engine,"activity":self.current_activity(),
            "clock_ms":self.engine.as_ref().map(|e|e.fw.clock()),
            "capabilities":{"host_view_selectors":true,"incremental_logs":true,"typed_error_snapshot":true,
                "native_engines_compiled":cfg!(feature="native-engines"),"jit_compiled":cfg!(feature="jit"),
                "advanced_components_compiled":cfg!(feature="advanced-components")},
            "limits":{"max_apk_bytes":self.policy.max_apk_bytes,"max_wait_ms":self.policy.max_wait_ms,
                "max_batch_steps":self.policy.max_batch_steps,"sandbox_root_count":self.roots.len(),
                "view_snapshot_nodes":UI_QUERY_NODE_CAP,"find_result_count":UI_QUERY_RESULT_CAP,"view_wait_steps":1000,
                "log_tail_bytes":LOG_TAIL_MAX_BYTES},
            "logs":{"generation":self.log_generation,"bytes":log_bytes,"available":log_bytes.is_some()},
            "tool_error_count":self.tool_error_count
        })))
    }

    fn tool_logs_tail(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let options = runtime_options(args, "logs_tail")?;
        if options
            .cursor
            .as_ref()
            .is_some_and(|c| c.session_id != self.session_id || c.generation != self.log_generation)
        {
            return Err(agent_error(
                "STALE_LOG_CURSOR",
                "cursor belongs to another session or the log buffer was cleared",
                Some("read logs_tail without a cursor to restart"),
            ));
        }
        let maximum = usize::try_from(options.max_bytes.unwrap_or(4096)).map_err(|_| {
            agent_error("LIMIT_EXCEEDED", "max_bytes cannot fit this platform", None)
        })?;
        let (data, start, end, total) = {
            let buf = self.logs.lock().map_err(|_| {
                agent_error(
                    "LOG_BUFFER_UNAVAILABLE",
                    "log buffer lock is poisoned",
                    None,
                )
            })?;
            let start = match options.cursor {
                None => buf.len().saturating_sub(maximum),
                Some(c) => usize::try_from(c.offset)
                    .ok()
                    .filter(|n| *n <= buf.len())
                    .ok_or_else(|| {
                        agent_error(
                            "INVALID_LOG_CURSOR",
                            "cursor offset exceeds the current log buffer",
                            None,
                        )
                    })?,
            };
            let end = start.saturating_add(maximum).min(buf.len());
            (buf[start..end].to_vec(), start, end, buf.len())
        };
        Ok(ToolResult::json(&serde_json::json!({
            "data_b64":base64_encode(&data),"text":String::from_utf8_lossy(&data),
            "text_lossy":std::str::from_utf8(&data).is_err(),"bytes":data.len(),
            "start_offset":start,"end_offset":end,"total_bytes":total,"has_more":end<total,
            "next_cursor":LogCursor{session_id:self.session_id.clone(),generation:self.log_generation,offset:end as u64}
        })))
    }

    fn tool_last_error(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let options = runtime_options(args, "last_error")?;
        let clear = options.clear.unwrap_or(false);
        let error = self.last_tool_error.clone();
        if clear {
            self.last_tool_error = None;
        }
        Ok(ToolResult::json(
            &serde_json::json!({"error":error,"tool_error_count":self.tool_error_count,"cleared":clear}),
        ))
    }
}
