// Structured host-view selectors. Canvas/Compose labels remain served by observe/tap_text.
const UI_QUERY_NODE_CAP: usize = 10_000;
const UI_QUERY_RESULT_CAP: usize = 200;

#[derive(Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewQuery {
    resource_id: Option<String>,
    text: Option<String>,
    class: Option<String>,
    #[serde(default)]
    contains: bool,
    enabled: Option<bool>,
    clickable: Option<bool>,
    visible: Option<bool>,
    limit: Option<usize>,
    expected_token: Option<String>,
    timeout_ms: Option<u64>,
    step_ms: Option<u64>,
    state: Option<String>,
    observe: Option<bool>,
}

impl ViewQuery {
    fn parse(
        args: &serde_json::Map<String, serde_json::Value>,
        tool: &str,
    ) -> Result<Self, RdError> {
        for (key, value) in args {
            let valid = match key.as_str() {
                "resource_id" | "text" | "class" | "expected_token" | "state" => value.is_string(),
                "contains" | "enabled" | "clickable" | "visible" | "observe" => value.is_boolean(),
                "limit" | "timeout_ms" | "step_ms" => value.as_u64().is_some(),
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
        let q: Self = serde_json::from_value(serde_json::Value::Object(args.clone()))
            .map_err(|e| agent_error("INVALID_FORMAT", format!("{tool}: {e}"), None))?;
        validate_view_criteria(
            tool,
            q.resource_id.as_deref(),
            q.text.as_deref(),
            q.class.as_deref(),
        )?;
        for value in [&q.resource_id, &q.text, &q.class, &q.expected_token]
            .into_iter()
            .flatten()
        {
            if value.len() > 4096 {
                return Err(agent_error(
                    "LIMIT_EXCEEDED",
                    "selector strings exceed 4096 bytes",
                    None,
                ));
            }
        }
        if !(1..=UI_QUERY_RESULT_CAP).contains(&q.limit.unwrap_or(50)) {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                "limit must be in 1..=200",
                None,
            ));
        }
        let allowed: &[&str] = match tool {
            "find_views" => &[
                "resource_id",
                "text",
                "class",
                "contains",
                "enabled",
                "clickable",
                "visible",
                "limit",
            ],
            "tap_view" => &[
                "resource_id",
                "text",
                "class",
                "contains",
                "enabled",
                "clickable",
                "visible",
                "expected_token",
                "observe",
            ],
            "wait_for_view" => &[
                "resource_id",
                "text",
                "class",
                "contains",
                "enabled",
                "clickable",
                "visible",
                "timeout_ms",
                "step_ms",
                "state",
                "observe",
            ],
            _ => &[],
        };
        if args.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(agent_error(
                "INVALID_FORMAT",
                format!("unsupported argument for {tool}"),
                None,
            ));
        }
        let _ = q.observe; // Shared screen_post_tool handles this option.
        Ok(q)
    }
    fn matches(&self, n: &UiNode, visible: bool, enabled: bool) -> bool {
        self.resource_id
            .as_ref()
            .map_or(true, |s| n.resource_id.as_ref() == Some(s))
            && self.text.as_ref().map_or(true, |s| {
                if self.contains {
                    n.text.contains(s)
                } else {
                    n.text == *s
                }
            })
            && self
                .class
                .as_ref()
                .map_or(true, |s| class_matches(&n.class, s))
            && self.enabled.map_or(true, |v| v == enabled)
            && self.clickable.map_or(true, |v| v == n.clickable)
            && self.visible.unwrap_or(true) == visible
    }
}

struct ViewMatches {
    nodes: Vec<UiNode>,
    total: usize,
    truncated: bool,
    token: String,
}

fn node_json(n: &UiNode) -> serde_json::Value {
    let mut end = n.text.len().min(4096);
    while !n.text.is_char_boundary(end) {
        end -= 1;
    }

    serde_json::json!({"class":n.class,"resource_id":n.resource_id,"text":&n.text[..end],"text_bytes":n.text.len(),"text_truncated":end<n.text.len(),
        "bounds_xywh":[n.bounds.0,n.bounds.1,n.bounds.2,n.bounds.3],
        "enabled":n.enabled,"clickable":n.clickable,"visible":n.visibility == rd_vm::framework::Vis::Visible})
}

#[cfg(test)]
fn query_tree(
    tree: &UiNode,
    q: &ViewQuery,
    epoch: u64,
    session: &str,
    package: Option<&str>,
) -> ViewMatches {
    query_roots(std::slice::from_ref(tree), q, epoch, session, package)
}

fn query_roots(
    trees: &[UiNode],
    q: &ViewQuery,
    epoch: u64,
    session: &str,
    package: Option<&str>,
) -> ViewMatches {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session.hash(&mut hasher);
    package.hash(&mut hasher);
    epoch.hash(&mut hasher);
    trees.len().hash(&mut hasher);
    let mut stack: Vec<_> = trees
        .iter()
        .take(UI_QUERY_NODE_CAP)
        .rev()
        .map(|tree| (tree, true))
        .collect();
    let mut result = ViewMatches {
        nodes: Vec::new(),
        total: 0,
        truncated: trees.len() > UI_QUERY_NODE_CAP,
        token: String::new(),
    };
    let mut visited = 0;
    while let Some((n, parent_visible)) = stack.pop() {
        if visited == UI_QUERY_NODE_CAP {
            result.truncated = true;
            break;
        }
        visited += 1;
        let visible = parent_visible && n.visibility == rd_vm::framework::Vis::Visible;
        let enabled = n.enabled;
        n.class.hash(&mut hasher);
        n.resource_id.hash(&mut hasher);
        n.text.hash(&mut hasher);
        n.bounds.hash(&mut hasher);
        visible.hash(&mut hasher);
        enabled.hash(&mut hasher);
        n.clickable.hash(&mut hasher);
        n.truncated.hash(&mut hasher);
        n.children.len().hash(&mut hasher);
        result.truncated |= n.truncated;
        if q.matches(n, visible, enabled) {
            result.total += 1;
            if result.nodes.len() < q.limit.unwrap_or(50) {
                // Return a leaf copy: never copy a complete subtree per match.
                let mut leaf = UiNode {
                    class: n.class.clone(),
                    resource_id: n.resource_id.clone(),
                    text: n.text.clone(),
                    bounds: n.bounds,
                    visibility: n.visibility,
                    enabled: n.enabled,
                    clickable: n.clickable,
                    focusable: n.focusable,
                    truncated: n.truncated,
                    children: Vec::new(),
                };
                leaf.enabled = enabled;
                leaf.visibility = if visible {
                    rd_vm::framework::Vis::Visible
                } else {
                    rd_vm::framework::Vis::Invisible
                };
                result.nodes.push(leaf);
            }
        }
        let room = UI_QUERY_NODE_CAP.saturating_sub(stack.len());
        if n.children.len() > room {
            result.truncated = true;
        }
        for child in n.children.iter().take(room).rev() {
            stack.push((child, visible));
        }
    }
    result.token = format!("ui-{epoch:x}-{:016x}", hasher.finish());
    result
}

fn ui_tool_defs() -> Vec<ToolDef> {
    let common = serde_json::json!({
        "resource_id":{"type":"string","minLength":1},"text":{"type":"string","minLength":1},
        "class":{"type":"string","minLength":1},"contains":{"type":"boolean","default":false},
        "enabled":{"type":"boolean"},"clickable":{"type":"boolean"},"visible":{"type":"boolean","default":true}
    });
    [
        ("find_views", "Find structured host views by AND-combined selectors. Text is exact by default; contains=true opts into substring. Reports bounded matches, total, truncation, xywh bounds and an opaque session/revision token. Snapshots visit at most 10000 nodes; returned text is a UTF-8 prefix of at most 4096 bytes with explicit text_bytes/text_truncated. Tokens expire after potentially mutating tools, including waits and failed attempts. Not a Canvas/Compose query: use observe for those.", serde_json::json!({"limit":{"type":"integer","minimum":1,"maximum":200}})),
        ("tap_view", "Tap a uniquely matched visible enabled host view, rejecting ambiguity, incomplete trees, stale expected_token and occluded centers before injecting input. Coordinates are clipped to the 720x1440 viewport. Does not claim listener success when none fired.", serde_json::json!({"expected_token":{"type":"string"}})),
        ("wait_for_view", "Wait for a host selector to become present or absent, polling virtual time under existing wait/fuel budgets. A timeout is condition_met=false, never a successful assertion. Limited to 1000 time steps. Incomplete trees cannot prove absence.", serde_json::json!({"state":{"type":"string","enum":["present","absent"]},"timeout_ms":{"type":"integer","minimum":0},"step_ms":{"type":"integer","minimum":1}})),
    ].into_iter().map(|(name,description,extra)| {
        let mut props=common.clone();props.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let mut schema=obj(props,&[]);schema["additionalProperties"]=serde_json::json!(false);
        ToolDef{name,description,schema}
    }).collect()
}

impl AgentSession {
    fn view_matches(&mut self, q: &ViewQuery) -> Result<ViewMatches, RdError> {
        self.require_engine()?
            .ensure_laid_out()
            .map_err(vm_exit_to_rd)?;
        let tree = self
            .require_engine()?
            .view_roots_bounded(UI_QUERY_NODE_CAP)
            .ok_or_else(|| {
                agent_error(
                    "VIEW_NOT_FOUND",
                    "no active host view tree",
                    Some("use launch, or observe for Canvas/Compose"),
                )
            })?;
        Ok(query_roots(
            &tree,
            q,
            self.ui_epoch,
            &self.session_id,
            self.engine_package.as_deref(),
        ))
    }
    fn tool_find_views(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let q = ViewQuery::parse(args, "find_views")?;
        let m = self.view_matches(&q)?;
        Ok(ToolResult::json(
            &serde_json::json!({"matches":m.nodes.iter().map(node_json).collect::<Vec<_>>(),"total":m.total,
            "returned":m.nodes.len(),"truncated":m.truncated,"has_more":m.total>m.nodes.len() || m.truncated,"total_is_exact":!m.truncated,"tree_token":m.token,"revision":self.ui_epoch,"source":"host_view_tree"}),
        ))
    }
    fn tool_tap_view(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let q = ViewQuery::parse(args, "tap_view")?;
        let m = self.view_matches(&q)?;
        if q.expected_token.as_ref().is_some_and(|s| s != &m.token) {
            return Err(agent_error(
                "STALE_VIEW",
                "view token no longer describes this session/tree revision",
                Some("call find_views again"),
            ));
        }
        if m.truncated {
            return Err(agent_error(
                "INCOMPLETE_UI_TREE",
                "cannot establish a unique target in a truncated tree",
                None,
            ));
        }
        if m.total == 0 {
            return Err(agent_error(
                "VIEW_NOT_FOUND",
                "selector matched no host view",
                Some("inspect find_views or observe"),
            ));
        }
        if m.total != 1 {
            return Err(agent_error(
                "AMBIGUOUS_VIEW",
                format!("selector matched {} views", m.total),
                Some("add resource_id, exact text, or class"),
            ));
        }
        let n = &m.nodes[0];
        let (x, y, w, h) = n.bounds;
        let left = i64::from(x).max(0);
        let top = i64::from(y).max(0);
        let right = (i64::from(x) + i64::from(w)).min(720);
        let bottom = (i64::from(y) + i64::from(h)).min(1440);
        if !n.enabled
            || n.visibility != rd_vm::framework::Vis::Visible
            || left >= right
            || top >= bottom
        {
            return Err(agent_error(
                "VIEW_NOT_INTERACTABLE",
                "target is disabled, hidden or outside the viewport",
                None,
            ));
        }
        let px = ((left + right) / 2) as i32;
        let py = ((top + bottom) / 2) as i32;
        let actual = self.require_engine()?.view_at_point(px, py);
        if actual.as_ref().map_or(true, |hit| {
            hit.enabled != n.enabled
                || hit.clickable != n.clickable
                || hit.visibility != n.visibility
                || hit.resource_id != n.resource_id
                || hit.class != n.class
                || hit.bounds != n.bounds
                || hit.text != n.text
        }) {
            return Err(agent_error(
                "TARGET_OCCLUDED",
                "target center resolves to another view/window",
                Some("inspect the active dialog or select the foreground view"),
            ));
        }
        let dispatched = self
            .require_engine()?
            .touch_app(px, py)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(
            &serde_json::json!({"tapped":[px,py],"target":node_json(n),"listener_acionado":dispatched}),
        ))
    }
    fn tool_wait_for_view(
        &mut self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolResult, RdError> {
        let q = ViewQuery::parse(args, "wait_for_view")?;
        let state = q.state.as_deref().unwrap_or("present");
        if !matches!(state, "present" | "absent") {
            return Err(agent_error(
                "INVALID_FORMAT",
                "state must be present or absent",
                None,
            ));
        }
        let timeout = q.timeout_ms.unwrap_or(5000);
        let step = q.step_ms.unwrap_or(100);
        if timeout > self.policy.max_wait_ms
            || step == 0
            || (timeout > 0 && step > self.policy.max_wait_ms)
        {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                "timeout/step exceeds wait policy or step is zero",
                None,
            ));
        }
        if timeout / step + u64::from(timeout % step != 0) > 1000 {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                "wait_for_view is limited to 1000 virtual-time steps; increase step_ms",
                None,
            ));
        }
        let mut elapsed = 0;
        let mut polls = 0;
        loop {
            let m = self.view_matches(&q)?;
            if state == "absent" && m.truncated {
                return Err(agent_error(
                    "INCOMPLETE_UI_TREE",
                    "a truncated tree cannot prove absence",
                    None,
                ));
            }
            let found = if state == "present" {
                m.total > 0
            } else {
                m.total == 0
            };
            if found || elapsed == timeout {
                return Ok(ToolResult::json(
                    &serde_json::json!({"condition_met":found,"present":m.total>0,"state":state,"waited_ms":elapsed,"polls":polls+1,"total":m.total,"truncated":m.truncated,"matches":m.nodes.iter().map(node_json).collect::<Vec<_>>()}),
                ));
            }
            let delta = step.min(timeout - elapsed);
            self.require_engine()?
                .advance_clock(delta)
                .map_err(vm_exit_to_rd)?;
            elapsed += delta;
            polls += 1;
        }
    }
}

// Invalid optional values must not silently turn into a different UI action.
// Only enforce types/ranges already advertised by the existing schemas.
fn validate_legacy_tool_arguments(
    name: &str,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RdError> {
    let expected: &[(&str, &str)] = match name {
        "tap_text" => &[
            ("text", "string"),
            ("exact", "boolean"),
            ("nth", "index"),
            ("observe", "boolean"),
        ],
        "type_text" => &[
            ("text", "string"),
            ("replace", "boolean"),
            ("observe", "boolean"),
        ],
        "logs" => &[("clear", "boolean")],
        "runtime_health" => &[("gc", "boolean")],
        "observe" => &[
            ("format", "string"),
            ("diff", "boolean"),
            ("boxes", "boolean"),
        ],
        _ => return Ok(()),
    };
    if matches!(name, "tap_text" | "type_text") && !args.contains_key("text") {
        return Err(agent_error(
            "INVALID_FORMAT",
            format!("{name} requires text"),
            None,
        ));
    }
    for (key, kind) in expected {
        if let Some(value) = args.get(*key) {
            let valid = match *kind {
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "index" => value
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .is_some(),
                _ => false,
            };
            if !valid {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("{name}: {key} must be {kind}"),
                    Some("use the types/ranges advertised in tools/list"),
                ));
            }
        }
    }
    Ok(())
}
