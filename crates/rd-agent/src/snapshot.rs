// ADR 009 — install-time snapshot capture + snapshot presentation with
// lazy engine materialization (opt-in `RD_SNAPSHOT=on`, default off).
//
// Why: process-cold first frame in <26 ms cannot re-inflate a 100+ MB
// container or re-execute cold guest initialization (measured 303 ms of
// guest execution alone for an 11 MB app). So a first real cold launch
// persists its frame + observe model keyed by a content fingerprint of
// the APK (ZIP central-directory inventory + manifest bytes). Later
// processes install through the ADR 008 `PagedZip` (tail + CD + manifest
// only), present the stored frame immediately, and materialize the real
// engine synchronously on the first tool that needs it
// (`require_engine`). Launch/install results disclose this with
// `snapshot` / `materialized` / `snapshot_cached` fields — the stored
// frame is byte-identical to live execution under Law 4 determinism.
//
// Trust model: the fingerprint is a cache key, not a security boundary.
// The snapshot directory is trusted local state (like Android's oat
// files after dexopt); the freshly paged-parsed manifest is still
// cross-checked against `meta.json` before anything is presented.
//
// This file is `include!`d from `mcp.rs` (same pattern as observe.rs).

/// True when the owner opted in via `RD_SNAPSHOT=on|1` (mirrors `RD_JIT`).
pub fn snapshot_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("RD_SNAPSHOT")
            .map(|v| v == "on" || v == "1")
            .unwrap_or(false)
    })
}

/// Snapshot cache root: `RD_SNAPSHOT_DIR`, else `$XDG_CACHE_HOME` or
/// `$HOME/.cache`, always under `rustydroid/snapshots`.
pub fn snapshot_root() -> PathBuf {
    if let Ok(d) = std::env::var("RD_SNAPSHOT_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("rustydroid").join("snapshots")
}

/// Two-lane 128-bit FNV fingerprint (FNV-1a + FNV-1, distinct seeds).
/// Cache-key grade (see trust model above), stable across processes —
/// unlike `DefaultHasher`, whose keys are randomized per process.
struct Fingerprint {
    a: u64,
    b: u64,
}

impl Fingerprint {
    fn new() -> Self {
        Fingerprint {
            a: 0xcbf2_9ce4_8422_2325,
            b: 0x8422_2325_cbf2_9ce4,
        }
    }
    fn update(&mut self, bytes: &[u8]) {
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        for &x in bytes {
            self.a = (self.a ^ x as u64).wrapping_mul(PRIME);
            self.b = self.b.wrapping_mul(PRIME) ^ x as u64;
        }
    }
    fn u64(&mut self, v: u64) {
        self.update(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.update(s.as_bytes());
        self.update(&[0xff]);
    }
    fn hex(&self) -> String {
        format!("{:016x}{:016x}", self.a, self.b)
    }
}

/// Content fingerprint of an APK from its paged central directory +
/// manifest bytes. The CD inventory carries every entry's name, method,
/// CRC-32 and sizes, so two different containers cannot share a key
/// unless their full inventories (and manifest) agree.
fn apk_fingerprint(pz: &rd_apk::paged::PagedZip, manifest_bytes: &[u8]) -> String {
    let mut f = Fingerprint::new();
    f.str("rustydroid-snapshot-v1");
    f.str(env!("CARGO_PKG_VERSION"));
    f.str("720x1440");
    f.u64(pz.file_len());
    for e in pz.entries() {
        f.str(&e.name);
        f.u64(e.method as u64);
        f.u64(e.crc32 as u64);
        f.u64(e.compressed_size);
        f.u64(e.uncompressed_size);
        f.u64(e.local_header_offset);
    }
    f.u64(manifest_bytes.len() as u64);
    f.update(manifest_bytes);
    f.hex()
}

pub struct LoadedSnapshot {
    pub dir: PathBuf,
    pub package: String,
    pub activity: String,
    pub png: Arc<Vec<u8>>,
    pub observe_text: String,
    pub observe_json: String,
}

/// A snapshot frame that has been presented while the engine is not yet
/// materialized. Cleared by materialization or whenever the engine is
/// killed/replaced.
pub struct PendingSnapshot {
    pub package: String,
    pub activity: String,
    pub key: String,
    pub loaded: LoadedSnapshot,
}

fn snapshot_dir(key: &str) -> PathBuf {
    snapshot_root().join(key)
}

fn load_snapshot(key: &str) -> Option<LoadedSnapshot> {
    let dir = snapshot_dir(key);
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).ok()?).ok()?;
    if meta.get("format").and_then(|v| v.as_u64()) != Some(1) {
        return None;
    }
    if meta.get("runtime").and_then(|v| v.as_str()) != Some(env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let png = std::fs::read(dir.join("frame.png")).ok()?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    Some(LoadedSnapshot {
        package: meta.get("package")?.as_str()?.to_string(),
        activity: meta.get("activity")?.as_str()?.to_string(),
        png: Arc::new(png),
        observe_text: std::fs::read_to_string(dir.join("observe.txt")).unwrap_or_default(),
        observe_json: std::fs::read_to_string(dir.join("observe.json")).unwrap_or_default(),
        dir,
    })
}

/// Store atomically: write a temp sibling dir, then rename into place.
fn store_snapshot(
    key: &str,
    meta: &serde_json::Value,
    png: &[u8],
    observe_text: &str,
    observe_json: &str,
) -> Result<(), RdError> {
    let io = |e: std::io::Error| RdError::new("IO_ERROR", format!("snapshot store: {e}"), MODULE_ID);
    let root = snapshot_root();
    std::fs::create_dir_all(&root).map_err(io)?;
    let tmp = root.join(format!(".{key}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(io)?;
    std::fs::write(tmp.join("meta.json"), meta.to_string()).map_err(io)?;
    std::fs::write(tmp.join("frame.png"), png).map_err(io)?;
    std::fs::write(tmp.join("observe.txt"), observe_text).map_err(io)?;
    std::fs::write(tmp.join("observe.json"), observe_json).map_err(io)?;
    let dir = snapshot_dir(key);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(io)?;
    Ok(())
}

impl AgentSession {
    /// Issue #259: um pacote só pode servir/gravar snapshot enquanto o
    /// estado da sessão para ele é o de um install pristine.
    fn snapshot_pristine(&self, package: &str) -> bool {
        !self.snapshot_dirty.contains(package)
    }

    fn mark_snapshot_dirty(&mut self, package: &str) {
        self.snapshot_dirty.insert(package.to_string());
    }

    /// ADR 009 fast install: paged open (tail+CD), fingerprint, manifest
    /// only. Returns `Ok(Some(result))` on a verified snapshot hit,
    /// `Ok(None)` to fall through to the authoritative whole-file path
    /// (a fast-path failure is never user-visible as an install error).
    fn try_install_snapshot(&mut self, apk_file: &Path) -> Result<Option<ToolResult>, RdError> {
        let t0 = std::time::Instant::now();
        let pz = match rd_apk::paged::PagedZip::open(apk_file) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        phase("snapshot_paged_open", t0);
        let manifest_bytes = match pz.read("AndroidManifest.xml") {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        let key = apk_fingerprint(&pz, &manifest_bytes);
        let loaded = match load_snapshot(&key) {
            Some(l) => l,
            None => return Ok(None),
        };
        phase("snapshot_lookup_hit", t0);
        // Manifest is always parsed fresh and cross-checked against the
        // snapshot meta before anything gets presented later. Labels are
        // left as resource refs (no ARSC on the fast path) — launch and
        // permissions only need the AXML content.
        let doc = match rd_apk::axml::parse(&manifest_bytes) {
            Ok(d) => d,
            Err(_) => return Ok(None),
        };
        let manifest = match rd_apk::manifest::build(&doc, None) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        if manifest.package != loaded.package || manifest.min_sdk < rd_apk::FLOOR_MIN_SDK {
            return Ok(None);
        }
        let package = manifest.package.clone();
        let components = manifest.application.components.len();
        let version = manifest.version_name.clone().unwrap_or_else(|| "?".into());
        let perms = PermissionEngine::install(
            &manifest,
            PermissionTable::embedded()
                .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?,
        );
        let killed = self.kill_engine_if_running(&package);
        let entry = InstalledApp {
            package: package.clone(),
            version_name: manifest.version_name.clone(),
            version_code: manifest.version_code,
            manifest,
            apk: None,
            source_path: Some(apk_file.to_path_buf()),
            snapshot_key: Some(key),
            perms,
        };
        match self.installed.iter().position(|a| a.package == package) {
            Some(i) => self.installed[i] = entry,
            None => self.installed.push(entry),
        }
        self.active = Some(package.clone());
        phase("snapshot_install_total", t0);
        Ok(Some(ToolResult::json(&serde_json::json!({
            "installed": true,
            "session_id": self.session_id,
            "package": package,
            "version_name": version,
            "components": components,
            "killed_running": killed,
            "installed_count": self.installed.len(),
            "snapshot_cached": true,
            "runtime_pending": self.find_installed(&package).map(|a| a.perms.pending_runtime().len()).unwrap_or(0),
            "runtime_special": self.find_installed(&package).map(|a| a.perms.pending_special().len()).unwrap_or(0),
        }))))
    }

    /// Find a presentable snapshot for (package, activity): by the key
    /// recorded at fast install, else by fingerprinting the source APK
    /// (paged: tail + CD + manifest only). A snapshot is presentable
    /// only when BOTH package and activity match what is being launched.
    fn snapshot_for_launch(
        &mut self,
        package: &str,
        activity: &str,
        snap_key: Option<&str>,
        source: Option<&Path>,
    ) -> Option<PendingSnapshot> {
        // Issue #259: grants/estado persistido mudam o primeiro frame
        // e o fingerprint não os enxerga — fora do pristine, o launch
        // real é o único caminho honesto.
        if !self.snapshot_pristine(package) {
            return None;
        }
        let t0 = std::time::Instant::now();
        let key = match snap_key {
            Some(k) => k.to_string(),
            None => {
                let path = source?;
                let pz = rd_apk::paged::PagedZip::open(path).ok()?;
                let mb = pz.read("AndroidManifest.xml").ok()?;
                apk_fingerprint(&pz, &mb)
            }
        };
        let loaded = load_snapshot(&key)?;
        if loaded.package != package || loaded.activity != activity {
            return None;
        }
        phase("snapshot_present_lookup", t0);
        Some(PendingSnapshot {
            package: package.to_string(),
            activity: activity.to_string(),
            key,
            loaded,
        })
    }

    /// Present a stored snapshot frame: live viewer + resource
    /// subscribers + cached PNG for `screenshot`, without an engine.
    fn present_snapshot(&mut self, pending: PendingSnapshot) {
        self.screen.version = self.screen.version.saturating_add(1);
        let v = self.screen.version;
        self.screen.png = Some((v, pending.loaded.png.clone()));
        self.screen.dirty = true;
        if let Some(live) = self.screen.live.clone() {
            let mut st: serde_json::Value = serde_json::from_str(&pending.loaded.observe_json)
                .unwrap_or_else(|_| serde_json::json!({}));
            st["version"] = serde_json::json!(v);
            st["text"] = serde_json::json!(pending.loaded.observe_text);
            st["package"] = serde_json::json!(pending.package);
            st["last_tool"] = serde_json::json!("launch");
            st["last_tool_ms"] = serde_json::json!(0.0);
            st["draw_error"] = serde_json::Value::Null;
            st["snapshot"] = serde_json::json!(true);
            if let Ok(mut g) = live.inner.lock() {
                g.version = v;
                g.state = st.to_string();
                g.png = pending.loaded.png.clone();
            }
            live.changed.notify_all();
        }
        if !self.screen.subs.is_empty() {
            self.screen.notified = v;
            for uri in self.screen.subs.clone() {
                self.screen.pending.push(serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/updated",
                    "params": {"uri": uri, "version": v},
                }));
            }
        }
        self.pending_snapshot = Some(pending);
    }

    /// Capture after a REAL cold launch: first frame PNG + observe model,
    /// persisted under the APK fingerprint. Errors are returned so the
    /// launch result can disclose them (`snapshot_error`); they never
    /// fail the launch itself.
    fn capture_snapshot(&mut self, package: &str, activity: &str) -> Result<(), RdError> {
        // Issue #259: nunca gravar no cache global um frame produzido
        // fora do estado pristine (envenenaria sessões futuras).
        if !self.snapshot_pristine(package) {
            return Ok(());
        }
        let t0 = std::time::Instant::now();
        let (source_path, manifest_bytes) = {
            let app = self.find_installed(package).ok_or_else(|| {
                agent_error("NO_APP", "snapshot capture: package not installed", None)
            })?;
            let sp = app.source_path.clone().ok_or_else(|| {
                agent_error("NO_APP", "snapshot capture: source path unknown", None)
            })?;
            let mb = match &app.apk {
                Some(apk) => apk.zip.read("AndroidManifest.xml").map_err(|e| {
                    RdError::new(&e.code, &e.cause, &e.module_id)
                })?,
                None => {
                    let pz = rd_apk::paged::PagedZip::open(&sp)?;
                    pz.read("AndroidManifest.xml")?
                }
            };
            (sp, mb)
        };
        let pz = rd_apk::paged::PagedZip::open(&source_path)?;
        let key = apk_fingerprint(&pz, &manifest_bytes);
        let png = self.screen_png()?;
        let v = self.screen.version;
        let (observe_json, observe_text) = match self.screen.snap.as_ref() {
            Some(snap) => (snap.to_json(v, false).to_string(), snap.to_text(v)),
            None => (String::new(), String::new()),
        };
        let meta = serde_json::json!({
            "format": 1,
            "runtime": env!("CARGO_PKG_VERSION"),
            "package": package,
            "activity": activity,
            "surface": [720, 1440],
        });
        store_snapshot(&key, &meta, &png, &observe_text, &observe_json)?;
        if let Some(app) = self.find_installed_mut(package) {
            app.snapshot_key = Some(key);
        }
        phase("snapshot_capture", t0);
        Ok(())
    }

    /// Run the REAL cold launch represented by a presented snapshot.
    /// Called by `require_engine` on first engine-requiring tool.
    fn materialize_snapshot(&mut self) -> Result<(), RdError> {
        let t0 = std::time::Instant::now();
        let pending = self
            .pending_snapshot
            .take()
            .ok_or_else(|| agent_error("NOT_RUNNING", "no snapshot to materialize", None))?;
        let r = self.cold_launch(&pending.package, &pending.activity, None)?;
        // Issue #259: o engine real rodou — o app pode ter persistido
        // estado; launches futuros desta sessão não servem snapshot.
        self.mark_snapshot_dirty(&pending.package);
        phase("snapshot_materialize", t0);
        r.map(|_| ()).map_err(vm_exit_to_rd)
    }
}
