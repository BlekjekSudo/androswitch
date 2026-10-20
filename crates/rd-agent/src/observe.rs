// Screen observation without screenshots (round 6): `observe`, `tap_text`,
// MCP resources + subscriptions, and the live viewer (`rd mcp --live`).
//
// Included into `mcp.rs`. The screen model is derived from the recorded
// window display list (`Engine::screen_model`, toolkit-agnostic: Views,
// Compose, custom canvases) merged with the host view tree (clickable host
// views and host-painted texts). It is cached per *frame version*: any tool
// that can run app code marks the screen dirty; the next observer re-records
// one frame and bumps the version only when the visible content changed.

use rd_vm::graphics::{ScreenItem, ScreenModel};

/// URIs published as MCP resources.
const RES_SCREEN: &str = "rustydroid://screen";
const RES_SCREEN_TXT: &str = "rustydroid://screen.txt";
const RES_SCREEN_PNG: &str = "rustydroid://screen.png";
const RES_LOGS: &str = "rustydroid://logs";

/// Tools that never run app code (the screen stays valid across them).
const READ_ONLY_TOOLS: &[&str] = &[
    "observe",
    "ui_tree",
    "screenshot",
    "logs",
    "app_info",
    "package_list",
    "permissions_list",
    "permissions_audit",
    "device_profile",
    "behavior_switches",
    "runtime_health",
    "compat_gaps",
    "get_api_usage",
    "get_coverage",
    "assert_view",
    "assert_text",
    "assert_activity",
    "assert_finished",
];

/// Action tools that accept `observe: true` (append the screen summary).
const OBSERVE_AFTER_TOOLS: &[&str] = &[
    "launch", "tap", "tap_text", "swipe", "type_text", "key", "wait", "wait_for",
];

/// A clickable host view (from the view tree).
#[derive(Clone, Debug)]
struct HostTarget {
    label: String,
    bounds: [i32; 4],
}

struct ScreenSnap {
    model: ScreenModel,
    /// Texts painted by the host renderer (TextView & co.), not in the frame.
    host_texts: Vec<ScreenItem>,
    host_targets: Vec<HostTarget>,
    activity: String,
    dialogs: usize,
    /// The activity finished (BACK/finish()): there is no window to show.
    finished: bool,
    record_ms: f64,
    fingerprint: u64,
}

impl ScreenSnap {
    /// All visible texts (frame + host), reading order.
    fn texts(&self) -> Vec<&ScreenItem> {
        let mut v: Vec<&ScreenItem> = self.model.texts.iter().chain(self.host_texts.iter()).collect();
        v.sort_by_key(|t| ((t.bounds[1] + t.bounds[3]) / 2 / 8, t.bounds[0]));
        v
    }

    /// Tap targets: (label, bounds, source).
    fn targets(&self) -> Vec<(String, [i32; 4], &'static str)> {
        let mut v: Vec<(String, [i32; 4], &'static str)> = self
            .host_targets
            .iter()
            .map(|h| (h.label.clone(), h.bounds, "view"))
            .collect();
        for t in &self.model.targets {
            if !v.iter().any(|(_, b, _)| *b == t.bounds) {
                v.push((t.label.clone(), t.bounds, "frame"));
            }
        }
        v.sort_by_key(|(_, b, _)| ((b[1] + b[3]) / 2 / 8, b[0]));
        v
    }

    fn to_json(&self, version: u64, boxes: bool) -> serde_json::Value {
        let item = |t: &ScreenItem| {
            serde_json::json!({
                "text": t.text,
                "x": t.bounds[0], "y": t.bounds[1],
                "w": t.bounds[2] - t.bounds[0], "h": t.bounds[3] - t.bounds[1],
                "size": (t.size * 10.0).round() / 10.0,
                "color": format!("#{:08X}", t.argb),
            })
        };
        let mut v = serde_json::json!({
            "version": version,
            "activity": self.activity,
            "dialogs": self.dialogs,
            "finished": self.finished,
            "size": [self.model.width, self.model.height],
            "background": self.model.background.map(|c| format!("#{c:08X}")),
            "texts": self.texts().into_iter().map(item).collect::<Vec<_>>(),
            "targets": self.targets().into_iter().map(|(l, b, s)| serde_json::json!({
                "label": l,
                "x": (b[0] + b[2]) / 2, "y": (b[1] + b[3]) / 2,
                "bounds": b,
                "source": s,
            })).collect::<Vec<_>>(),
            "images": self.model.images.iter().map(|i| i.bounds).collect::<Vec<_>>(),
            "ops": self.model.ops,
            "record_ms": (self.record_ms * 10.0).round() / 10.0,
        });
        if boxes {
            v["boxes"] = serde_json::json!(self
                .model
                .boxes
                .iter()
                .map(|b| serde_json::json!({"bounds": b.bounds, "color": format!("#{:08X}", b.argb)}))
                .collect::<Vec<_>>());
        }
        v
    }

    fn to_text(&self, version: u64) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "frame v{version} · {} · {} texts · {} targets · {} images{}",
            if self.activity.is_empty() { "-" } else { &self.activity },
            self.texts().len(),
            self.targets().len(),
            self.model.images.len(),
            if self.dialogs > 0 {
                format!(" · {} dialog(s)", self.dialogs)
            } else {
                String::new()
            }
        );
        if self.finished {
            let _ = writeln!(out, "(activity finished — no window; call launch to start it again)");
        }
        let mut rows: Vec<(i32, Vec<&ScreenItem>)> = Vec::new();
        for t in self.texts() {
            let cy = (t.bounds[1] + t.bounds[3]) / 2;
            match rows.iter_mut().find(|(y, _)| (cy - *y).abs() <= 6) {
                Some((_, v)) => v.push(t),
                None => rows.push((cy, vec![t])),
            }
        }
        rows.sort_by_key(|r| r.0);
        for (y, mut runs) in rows {
            runs.sort_by_key(|r| r.bounds[0]);
            let parts: Vec<String> = runs.iter().map(|r| format!("[x{}] {}", r.bounds[0], r.text)).collect();
            let _ = writeln!(out, "y{y:<5} {}", parts.join("   "));
        }
        // icons / images (no label in the frame): centres, so icon-only
        // buttons (search, overflow, nav icons) are still reachable by tap
        let icons: Vec<String> = self
            .model
            .images
            .iter()
            .filter(|i| i.bounds[2] - i.bounds[0] <= 160 && i.bounds[3] - i.bounds[1] <= 160)
            .map(|i| format!("{},{}", (i.bounds[0] + i.bounds[2]) / 2, (i.bounds[1] + i.bounds[3]) / 2))
            .collect();
        if !icons.is_empty() {
            let _ = writeln!(out, "-- icons @ center: {}", icons.join("  "));
        }
        let targets = self.targets();
        if !targets.is_empty() {
            let _ = writeln!(out, "-- targets (label @ center x,y):");
            for (l, b, s) in targets {
                let _ = writeln!(
                    out,
                    "  {:?} @ {},{}{}",
                    l,
                    (b[0] + b[2]) / 2,
                    (b[1] + b[3]) / 2,
                    if s == "view" { " (view)" } else { "" }
                );
            }
        }
        out
    }
}

/// Shared state between the MCP thread and the live-viewer HTTP thread.
#[derive(Default)]
pub struct LiveShared {
    inner: Mutex<LiveInner>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct LiveInner {
    version: u64,
    state: String,
    png: Arc<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct ScreenState {
    dirty: bool,
    snap: Option<ScreenSnap>,
    version: u64,
    png: Option<(u64, Arc<Vec<u8>>)>,
    observed: Option<(u64, Vec<String>)>,
    notified: u64,
    subs: std::collections::BTreeSet<String>,
    pending: Vec<serde_json::Value>,
    live: Option<Arc<LiveShared>>,
    draw_error: Option<String>,
    last_tool: String,
    last_tool_ms: f64,
}

fn host_collect(n: &UiNode, texts: &mut Vec<ScreenItem>, targets: &mut Vec<HostTarget>) {
    if !matches!(n.visibility, rd_vm::framework::Vis::Visible) {
        return;
    }
    let (x, y, w, h) = n.bounds;
    let b = [x, y, x + w, y + h];
    if w > 0 && h > 0 {
        // AndroidComposeView & co. paint through the frame; host-painted
        // widgets (TextView, Button, EditText) only through the host renderer
        if !n.text.trim().is_empty() {
            texts.push(ScreenItem {
                kind: rd_vm::graphics::ScreenKind::Text,
                text: n.text.trim().to_string(),
                bounds: b,
                argb: 0xff00_0000,
                size: 0.0,
                filled: false,
            });
        }
        if n.clickable && n.enabled {
            let short = n.class.rsplit('.').next().unwrap_or(&n.class).to_string();
            let label = if !n.text.trim().is_empty() {
                n.text.trim().to_string()
            } else if let Some(r) = &n.resource_id {
                r.rsplit('/').next().unwrap_or(r).to_string()
            } else {
                short
            };
            targets.push(HostTarget { label, bounds: b });
        }
    }
    for c in &n.children {
        host_collect(c, texts, targets);
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl AgentSession {
    /// [`AgentSession::call_tool`] with panic isolation: a bug in the runtime
    /// (VM/framework) ends the *app*, never the MCP server — the session,
    /// registry and logs survive and the agent gets a typed error.
    pub fn call_tool_isolated(&mut self, name: &str, args: &serde_json::Value) -> ToolResult {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.call_tool(name, args)));
        match r {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                self.engine = None;
                self.engine_package = None;
                self.activity = None;
                self.screen.snap = None;
                self.screen.dirty = true;
                self.logs
                    .lock()
                    .map(|mut l| l.extend_from_slice(format!("E/RustyDroid: runtime panic in '{name}': {msg}\n").as_bytes()))
                    .ok();
                ToolResult::error(&agent_error(
                    "INTERNAL_PANIC",
                    format!("runtime panic durante '{name}': {msg} — o app foi encerrado; a sessão continua"),
                    Some("relance com `launch` e reporte o bug (logs tem a mensagem)"),
                ))
            }
        }
    }

    /// Re-records one frame when the screen is dirty and rebuilds the model.
    fn screen_refresh(&mut self) -> Result<(), RdError> {
        if !self.screen.dirty && self.screen.snap.is_some() {
            return Ok(());
        }
        let t0 = std::time::Instant::now();
        let activity = self.activity.clone().unwrap_or_default();
        let eng = self.require_engine()?;
        let finished = eng.activity_is_finished();
        let mut draw_error = None;
        let model = if finished {
            // no window any more: never serve the last frame as if it were live
            ScreenModel {
                width: 720,
                height: 1440,
                ..ScreenModel::default()
            }
        } else {
            let _ = eng.ensure_laid_out();
            if let Err(e) = eng.draw_frame() {
                draw_error = Some(format!("{e}{}", eng.exit_trace(&e)));
            }
            eng.screen_model()
        };
        let mut host_texts = Vec::new();
        let mut host_targets = Vec::new();
        let mut dialogs = 0;
        if let Some(tree) = eng.view_tree().filter(|_| !finished) {
            dialogs = tree.children.iter().filter(|c| c.class.contains("Dialog") || c.class.contains("DecorView")).count();
            host_collect(&tree, &mut host_texts, &mut host_targets);
        }
        // host text already drawn in the frame (same text, overlapping) → once
        host_texts.retain(|h| {
            !model.texts.iter().any(|t| {
                t.text == h.text
                    && t.bounds[0] < h.bounds[2]
                    && h.bounds[0] < t.bounds[2]
                    && t.bounds[1] < h.bounds[3]
                    && h.bounds[1] < t.bounds[3]
            })
        });
        let fingerprint = {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            model.fingerprint.hash(&mut hs);
            for t in &host_texts {
                t.text.hash(&mut hs);
                t.bounds.hash(&mut hs);
            }
            for t in &host_targets {
                t.bounds.hash(&mut hs);
            }
            activity.hash(&mut hs);
            finished.hash(&mut hs);
            hs.finish()
        };
        if let Some(msg) = &draw_error {
            if self.screen.draw_error.as_deref() != Some(msg) {
                self.logs
                    .lock()
                    .expect("logs")
                    .extend_from_slice(format!("W/RustyDroid: draw pass failed: {msg}\n").as_bytes());
            }
        }
        self.screen.draw_error = draw_error;
        let changed = self.screen.snap.as_ref().map(|s| s.fingerprint) != Some(fingerprint);
        if changed {
            self.screen.version += 1;
        }
        self.screen.snap = Some(ScreenSnap {
            model,
            host_texts,
            host_targets,
            activity,
            dialogs,
            finished,
            record_ms: t0.elapsed().as_secs_f64() * 1000.0,
            fingerprint,
        });
        self.screen.dirty = false;
        Ok(())
    }

    /// PNG of the current frame, cached per frame version.
    fn screen_png(&mut self) -> Result<Arc<Vec<u8>>, RdError> {
        self.screen_refresh()?;
        let v = self.screen.version;
        if let Some((pv, png)) = &self.screen.png {
            if *pv == v {
                return Ok(png.clone());
            }
        }
        let eng = self.require_engine()?;
        let tree = eng
            .view_tree()
            .ok_or_else(|| agent_error("INVALID_FORMAT", "sem activity/window para screenshot", None))?;
        let mut fb = rd_render::render_snapshot(&tree);
        if eng.has_frame() {
            let (w, h) = (fb.w, fb.h);
            eng.rasterize_frame(&mut fb.px, w, h);
        }
        let png = Arc::new(fb.to_png());
        self.screen.png = Some((v, png.clone()));
        Ok(png)
    }

    /// Called after every tool: app code may have run → screen dirty; the
    /// optional `observe: true` argument appends the screen summary.
    fn screen_post_tool(&mut self, name: &str, args: &serde_json::Map<String, serde_json::Value>, out: &mut ToolResult) {
        if !READ_ONLY_TOOLS.contains(&name) {
            self.screen.dirty = true;
        }
        let wants = args.get("observe").and_then(|v| v.as_bool()).unwrap_or(false);
        if wants && !out.is_error && OBSERVE_AFTER_TOOLS.contains(&name) && self.engine.is_some() {
            match self.screen_refresh() {
                Ok(()) => {
                    let s = self.observe_text(true);
                    out.content.push(ToolContent::Text(s));
                }
                Err(e) => out.content.push(ToolContent::Text(format!("observe: {}", e.cause))),
            }
        }
    }

    /// Text summary + diff against the previous observation.
    fn observe_text(&mut self, diff: bool) -> String {
        let v = self.screen.version;
        let Some(snap) = self.screen.snap.as_ref() else {
            return "sem frame".into();
        };
        let mut s = snap.to_text(v);
        let cur: Vec<String> = snap.texts().iter().map(|t| t.text.clone()).collect();
        if diff {
            s.push_str(&self.diff_line(&cur));
        }
        if let Some(e) = &self.screen.draw_error {
            s.push_str(&format!("!! draw pass failed: {}\n", e.lines().next().unwrap_or("")));
        }
        self.screen.observed = Some((v, cur));
        s
    }

    fn diff_line(&self, cur: &[String]) -> String {
        match &self.screen.observed {
            Some((pv, prev)) if *pv == self.screen.version => "-- unchanged since last observe\n".into(),
            Some((pv, prev)) => {
                let added: Vec<&String> = cur.iter().filter(|t| !prev.contains(t)).collect();
                let removed: Vec<&String> = prev.iter().filter(|t| !cur.contains(t)).collect();
                format!("-- changed since v{pv}: +{added:?} -{removed:?}\n")
            }
            None => String::new(),
        }
    }

    fn tool_observe(&mut self, args: &serde_json::Map<String, serde_json::Value>) -> Result<ToolResult, RdError> {
        let t0 = std::time::Instant::now();
        self.screen_refresh()?;
        let format = args.get("format").and_then(|v| v.as_str()).unwrap_or("text");
        let diff = args.get("diff").and_then(|v| v.as_bool()).unwrap_or(true);
        match format {
            "json" => {
                let boxes = args.get("boxes").and_then(|v| v.as_bool()).unwrap_or(false);
                let v = self.screen.version;
                let snap = self.screen.snap.as_ref().expect("snap");
                let mut j = snap.to_json(v, boxes);
                let cur: Vec<String> = snap.texts().iter().map(|t| t.text.clone()).collect();
                if diff {
                    if let Some((pv, prev)) = &self.screen.observed {
                        j["changed"] = serde_json::json!(*pv != v);
                        j["since"] = serde_json::json!(pv);
                        j["added"] = serde_json::json!(cur.iter().filter(|t| !prev.contains(t)).collect::<Vec<_>>());
                        j["removed"] = serde_json::json!(prev.iter().filter(|t| !cur.contains(t)).collect::<Vec<_>>());
                    }
                }
                if let Some(e) = &self.screen.draw_error {
                    j["draw_error"] = serde_json::json!(e);
                }
                j["observe_ms"] = serde_json::json!((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
                self.screen.observed = Some((v, cur));
                Ok(ToolResult::json(&j))
            }
            "text" => Ok(ToolResult::text(self.observe_text(diff))),
            other => Err(agent_error(
                "INVALID_FORMAT",
                format!("observe: format '{other}' desconhecido"),
                Some("use format \"text\" (compacto, default) ou \"json\""),
            )),
        }
    }

    /// Taps the target (or text) whose visible label matches `text`.
    fn tool_tap_text(&mut self, args: &serde_json::Map<String, serde_json::Value>) -> Result<ToolResult, RdError> {
        let Some(want) = args.get("text").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) else {
            return Err(agent_error(
                "INVALID_FORMAT",
                "tap_text exige text (string não vazia)",
                Some("use {\"text\": \"Continuar\"} — veja `observe` para os rótulos visíveis"),
            ));
        };
        let exact = args.get("exact").and_then(|v| v.as_bool()).unwrap_or(false);
        let nth = args.get("nth").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        self.screen_refresh()?;
        let snap = self.screen.snap.as_ref().expect("snap");
        let w = norm(want);
        let hit = |label: &str| {
            let l = norm(label);
            if exact {
                l == w
            } else {
                l.contains(&w)
            }
        };
        // prefer exact-label targets, then any matching target, then bare texts
        let mut cands: Vec<(String, [i32; 4], &'static str)> = Vec::new();
        let targets = snap.targets();
        for (l, b, s) in &targets {
            if norm(l) == w {
                cands.push((l.clone(), *b, s));
            }
        }
        for (l, b, s) in &targets {
            if hit(l) && !cands.iter().any(|c| c.1 == *b) {
                cands.push((l.clone(), *b, s));
            }
        }
        for t in snap.texts() {
            if hit(&t.text) && !cands.iter().any(|c| c.1[0] <= t.bounds[0] && c.1[1] <= t.bounds[1] && c.1[2] >= t.bounds[2] && c.1[3] >= t.bounds[3]) {
                cands.push((t.text.clone(), t.bounds, "text"));
            }
        }
        let Some((label, b, via)) = cands.get(nth).cloned() else {
            let visible: Vec<String> = snap.texts().iter().take(30).map(|t| t.text.clone()).collect();
            return Err(agent_error(
                "VIEW_NOT_FOUND",
                format!("tap_text: nenhum rótulo visível casa com {want:?} (nth={nth}, candidatos={})", cands.len()),
                Some(&format!("textos visíveis: {visible:?}")),
            ));
        };
        let (x, y) = ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2);
        let hitl = self.require_engine()?.touch_app(x, y).map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "tapped": [x, y],
            "label": label,
            "bounds": b,
            "via": via,
            "candidates": cands.len(),
            "listener_acionado": hitl,
        })))
    }

    // ── MCP resources ───────────────────────────────────────────────────────

    fn resources_list() -> serde_json::Value {
        serde_json::json!({ "resources": [
            {"uri": RES_SCREEN, "name": "screen", "mimeType": "application/json",
             "description": "Modelo semântico da tela atual (textos, alvos de toque, imagens, versão do frame) — derivado da display list, sem screenshot. Assinável: notifications/resources/updated quando o conteúdo visível muda."},
            {"uri": RES_SCREEN_TXT, "name": "screen.txt", "mimeType": "text/plain",
             "description": "A mesma tela em texto compacto (linhas por y, alvos com centro x,y). Assinável."},
            {"uri": RES_SCREEN_PNG, "name": "screen.png", "mimeType": "image/png",
             "description": "PNG 720×1440 do frame atual (cacheado por versão). Assinável."},
            {"uri": RES_LOGS, "name": "logs", "mimeType": "text/plain",
             "description": "System.out/logcat do app na sessão."},
        ]})
    }

    fn resource_read(&mut self, uri: &str) -> Result<serde_json::Value, (i64, String)> {
        let err = |e: RdError| (-32002_i64, format!("{}: {}", e.code, e.cause));
        let content = match uri {
            RES_SCREEN => {
                self.screen_refresh().map_err(err)?;
                let snap = self.screen.snap.as_ref().expect("snap");
                serde_json::json!({"uri": uri, "mimeType": "application/json",
                    "text": snap.to_json(self.screen.version, true).to_string()})
            }
            RES_SCREEN_TXT => {
                self.screen_refresh().map_err(err)?;
                let snap = self.screen.snap.as_ref().expect("snap");
                serde_json::json!({"uri": uri, "mimeType": "text/plain", "text": snap.to_text(self.screen.version)})
            }
            RES_SCREEN_PNG => {
                let png = self.screen_png().map_err(err)?;
                serde_json::json!({"uri": uri, "mimeType": "image/png", "blob": base64_encode(&png)})
            }
            RES_LOGS => {
                let buf = self.logs.lock().expect("logs");
                serde_json::json!({"uri": uri, "mimeType": "text/plain", "text": String::from_utf8_lossy(&buf).to_string()})
            }
            other => return Err((-32002, format!("resource '{other}' não existe (veja resources/list)"))),
        };
        Ok(serde_json::json!({ "contents": [content] }))
    }

    fn resource_subscribe(&mut self, uri: &str, on: bool) -> Result<(), (i64, String)> {
        if ![RES_SCREEN, RES_SCREEN_TXT, RES_SCREEN_PNG].contains(&uri) {
            return Err((-32602, format!("resource '{uri}' não é assinável ({RES_SCREEN}, {RES_SCREEN_TXT}, {RES_SCREEN_PNG})")));
        }
        if on {
            self.screen.subs.insert(uri.to_string());
        } else {
            self.screen.subs.remove(uri);
        }
        Ok(())
    }

    /// After a `tools/call`: when someone watches the screen (subscribers or
    /// the live viewer), re-observe and push updates if the content changed.
    fn screen_after_call(&mut self, tool: &str, ms: f64) {
        self.screen.last_tool = tool.to_string();
        self.screen.last_tool_ms = ms;
        let watched = !self.screen.subs.is_empty() || self.screen.live.is_some();
        if !watched || self.engine.is_none() || self.screen_refresh().is_err() {
            return;
        }
        let v = self.screen.version;
        if v != self.screen.notified {
            self.screen.notified = v;
            for uri in self.screen.subs.clone() {
                self.screen.pending.push(serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/updated",
                    "params": {"uri": uri, "version": v},
                }));
            }
        }
        self.live_publish();
    }

    fn live_publish(&mut self) {
        let Some(live) = self.screen.live.clone() else {
            return;
        };
        let v = self.screen.version;
        let png = if live.inner.lock().map(|g| g.version == v && !g.png.is_empty()).unwrap_or(false) {
            None
        } else {
            self.screen_png().ok()
        };
        let Some(snap) = self.screen.snap.as_ref() else {
            return;
        };
        let mut st = snap.to_json(v, false);
        st["text"] = serde_json::json!(snap.to_text(v));
        st["last_tool"] = serde_json::json!(self.screen.last_tool);
        st["last_tool_ms"] = serde_json::json!((self.screen.last_tool_ms * 10.0).round() / 10.0);
        st["package"] = serde_json::json!(self.engine_package);
        st["draw_error"] = serde_json::json!(self.screen.draw_error);
        if let Ok(mut g) = live.inner.lock() {
            g.version = v;
            g.state = st.to_string();
            if let Some(p) = png {
                g.png = p;
            }
        }
        live.changed.notify_all();
    }

    /// Notifications produced since the last call (serve loop drains them).
    pub fn take_notifications(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.screen.pending)
    }

    /// Starts the live viewer HTTP server on `addr` (e.g. `0.0.0.0:7878`).
    pub fn enable_live(&mut self, addr: &str) -> Result<std::net::SocketAddr, RdError> {
        let listener = std::net::TcpListener::bind(addr)
            .map_err(|e| RdError::new("IO_ERROR", format!("live viewer: bind {addr}: {e}"), MODULE_ID))?;
        let local = listener
            .local_addr()
            .map_err(|e| RdError::new("IO_ERROR", format!("live viewer: {e}"), MODULE_ID))?;
        let shared = Arc::new(LiveShared::default());
        if let Ok(mut g) = shared.inner.lock() {
            g.state = serde_json::json!({"version": 0, "text": "aguardando o primeiro frame (install + launch)…"}).to_string();
        }
        self.screen.live = Some(shared.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let sh = shared.clone();
                std::thread::spawn(move || live_serve_conn(stream, &sh));
            }
        });
        Ok(local)
    }
}

fn live_serve_conn(mut stream: std::net::TcpStream, sh: &LiveShared) {
    use std::io::{Read, Write};
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
    let (route, query) = path.split_once('?').unwrap_or((path.as_str(), ""));
    let (ctype, body): (&str, Vec<u8>) = match route {
        "/" | "/index.html" => ("text/html; charset=utf-8", LIVE_HTML.as_bytes().to_vec()),
        "/state.json" => {
            // long-poll: `?since=N` blocks (≤ 20 s) until the version moves
            let since: u64 = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("since="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(u64::MAX);
            let mut g = match sh.inner.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while since != u64::MAX && g.version <= since {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                g = match sh.changed.wait_timeout(g, left) {
                    Ok((g, _)) => g,
                    Err(_) => return,
                };
            }
            ("application/json", g.state.clone().into_bytes())
        }
        "/frame.png" => {
            let png = sh.inner.lock().map(|g| g.png.clone()).unwrap_or_default();
            if png.is_empty() {
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                return;
            }
            ("image/png", png.to_vec())
        }
        _ => {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

const LIVE_HTML: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>RustyDroid live</title>
<style>
*{box-sizing:border-box}body{margin:0;font:14px/1.4 system-ui,sans-serif;background:#111418;color:#e6e6e6}
header{display:flex;gap:16px;align-items:center;padding:10px 16px;background:#1b2027;border-bottom:1px solid #2a313a}
header b{color:#f74c00}.pill{background:#2a313a;border-radius:10px;padding:2px 8px;font-size:12px}
main{display:flex;gap:16px;padding:16px;align-items:flex-start}
#phone{position:relative;width:360px;height:720px;border-radius:18px;overflow:hidden;background:#000;box-shadow:0 0 0 8px #222,0 8px 30px #000}
#phone img,#phone canvas{position:absolute;left:0;top:0;width:360px;height:720px}
#side{flex:1;min-width:280px}pre{background:#1b2027;border:1px solid #2a313a;border-radius:8px;padding:10px;white-space:pre-wrap;max-height:640px;overflow:auto;font:12px/1.45 ui-monospace,monospace}
label{font-size:12px;color:#9aa4ae}.err{color:#ff6b6b}
</style></head><body>
<header><b>RustyDroid</b> live <span id="pkg" class="pill">-</span><span id="ver" class="pill">frame v0</span><span id="tool" class="pill">-</span>
<label><input type="checkbox" id="ov" checked> tap targets</label></header>
<main><div id="phone"><img id="img" alt=""><canvas id="cv" width="720" height="1440"></canvas></div>
<div id="side"><div id="err" class="err"></div><pre id="txt">connecting…</pre></div></main>
<script>
let v=-1,st=null;const img=document.getElementById('img'),cv=document.getElementById('cv'),cx=cv.getContext('2d');
function draw(){cx.clearRect(0,0,720,1440);if(!st||!document.getElementById('ov').checked)return;
cx.lineWidth=3;cx.font='22px sans-serif';for(const t of (st.targets||[])){const b=t.bounds;cx.strokeStyle=t.source==='view'?'#3fb950':'#f74c00';
cx.strokeRect(b[0],b[1],b[2]-b[0],b[3]-b[1]);}}
document.getElementById('ov').onchange=draw;
async function loop(){for(;;){try{const r=await fetch('state.json?since='+v,{cache:'no-store'});const s=await r.json();
if(s.version!==v){v=s.version;st=s;img.src='frame.png?v='+v;
document.getElementById('ver').textContent='frame v'+v;document.getElementById('pkg').textContent=(s.package||'-')+(s.activity?' · '+s.activity:'');
document.getElementById('tool').textContent=s.last_tool?('last: '+s.last_tool+' '+s.last_tool_ms+' ms'):'-';
document.getElementById('txt').textContent=s.text||'';document.getElementById('err').textContent=s.draw_error?('draw pass failed: '+s.draw_error):'';draw();}}
catch(e){await new Promise(r=>setTimeout(r,1000));}}}
loop();
</script></body></html>
"##;
