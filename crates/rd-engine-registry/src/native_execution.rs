//! Real APK/DEX/ELF/JNI route for C, C++ and Rust. No language guessing.
use crate::{Artifact, EngineEvent, EngineSyscall};
use rd_apk::manifest::ComponentKind;
use rd_apk::{Apk, RdError, RdResult};
use rd_vm::{Engine, Value, VmConfig, VmExit};
use serde::Deserialize;
use std::sync::Arc;

// Keep parsing limits explicit and validate before invoking any guest code.
const MAX_APK_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INVOKE_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_INVOKE_ARGS: usize = 16;

// Retain the original request type name to preserve Serde diagnostics.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    class: String,
    method: String,
    args: Vec<i32>,
}

impl Call {
    fn parse(payload: &[u8]) -> RdResult<Self> {
        if payload.len() > MAX_INVOKE_PAYLOAD_BYTES {
            return Err(error("INVALID_FORMAT", "invoke payload too large"));
        }
        let call: Self =
            serde_json::from_slice(payload).map_err(|e| error("INVALID_FORMAT", e.to_string()))?;
        if call.args.len() > MAX_INVOKE_ARGS {
            return Err(error("INVALID_FORMAT", "at most16integer arguments"));
        }
        Ok(call)
    }
}

pub struct NativeProcess {
    id: String,
    package: String,
    activity: String,
    engine: Engine,
    last_result: serde_json::Value,
    launched: bool,
}
impl std::fmt::Debug for NativeProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeProcess")
            .field("engine", &self.id)
            .field("package", &self.package)
            .field("launched", &self.launched)
            .finish()
    }
}
fn error(code: &str, cause: impl Into<String>) -> RdError {
    RdError::new(code, cause, "rd-native-engines").with_suggestion("see docs/native-languages.md; enable native-engines, launch first, and use only the documented bounded API")
}
// Issue #260: the descriptor must name a class the way Dalvik expects
// (Lpkg/Name;). The MCP pre-resolves it, but this constructor is public —
// reject shapes that cannot be a class before the heavy engine mount.
fn valid_activity_desc(desc: &str) -> RdResult<String> {
    let inner = desc
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
        .ok_or_else(|| {
            error(
                "INVALID_FORMAT",
                format!("activity descriptor {desc:?} must be a Dalvik Lpkg/Class; name"),
            )
        })?;
    if inner.is_empty()
        || inner
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(error(
            "INVALID_FORMAT",
            format!("activity descriptor {desc:?} has an empty or hostile package segment"),
        ));
    }
    Ok(desc.to_string())
}
fn vm_error(e: VmExit) -> RdError {
    match e {
        VmExit::Error(e) => RdError {
            code: e.code,
            cause: e.cause,
            suggestion: e.suggestion,
            module_id: e.module_id,
        },
        VmExit::Exception(t) => error(
            "JAVA_EXCEPTION",
            format!("{}: {}", t.class, t.message.unwrap_or_default()),
        ),
    }
}
impl NativeProcess {
    pub(crate) fn parse(id: &str, artifact: Artifact<'_>) -> RdResult<Self> {
        if artifact.bytes.len() as u64 > MAX_APK_BYTES {
            return Err(error("APK_TOO_LARGE", "native adapter APK exceeds 256 MiB"));
        }
        Self::from_apk(id, Apk::from_bytes(artifact.bytes.to_vec())?)
    }
    /// Prepare an already-parsed APK for the same native route used by MCP.
    ///
    /// Issue #260: only a real `<activity>` may satisfy the launcher probe —
    /// `<service>`/`<receiver>`/`<provider>` with an injected MAIN/LAUNCHER
    /// intent filter no longer hijack the entry selection. `<activity-alias>`
    /// still requires the MCP application route (which resolves the alias
    /// target per issue #109 before any launch).
    pub fn from_apk(id: &str, apk: Apk) -> RdResult<Self> {
        Self::from_apk_shared(id, Arc::new(apk), None)
    }
    /// MCP route (issue #260): the agent already resolved and validated the
    /// entry component — an explicit non-launcher `<activity>`, an
    /// `<activity-alias>` target (`launch_desc_of_component`, issue #109) or
    /// the default launcher — so no launcher probe runs here and the VM still
    /// enforces that the resolved class exists in the mounted DEX (issue #82).
    /// The `Arc<Apk>` is shared with the engine instead of cloned per launch.
    pub fn from_apk_with_activity(id: &str, apk: Arc<Apk>, activity_desc: &str) -> RdResult<Self> {
        Self::from_apk_shared(id, apk, Some(activity_desc))
    }
    fn from_apk_shared(id: &str, apk: Arc<Apk>, activity: Option<&str>) -> RdResult<Self> {
        if !crate::native_execution_available(id) {
            return Err(error(
                "NOT_IMPLEMENTED",
                "no executing native adapter for selected ID",
            ));
        }
        if apk.file_size_bytes > MAX_APK_BYTES {
            return Err(error("APK_TOO_LARGE", "native adapter APK exceeds 256 MiB"));
        }
        if apk.manifest.min_sdk < 26 {
            return Err(error(
                "UNSUPPORTED_API_FLOOR",
                "native execution requires minSdkVersion26 or greater",
            ));
        }
        if apk.native_libs.is_empty() {
            return Err(error("MISSING_ENTRY", "native APK has no lib/<abi>/*.so"));
        }
        let activity = match activity {
            Some(desc) => valid_activity_desc(desc)?,
            None => {
                let component = apk
                    .manifest
                    .application
                    .components
                    .iter()
                    .find(|c| {
                        matches!(c.kind, ComponentKind::Activity)
                            && c.is_launcher
                            && c.enabled != Some(false)
                    })
                    .ok_or_else(|| error("MISSING_ENTRY", "no enabled launcher activity"))?;
                if component.target_activity.is_some() {
                    return Err(error(
                        "NOT_IMPLEMENTED",
                        "native adapter launch aliases require the MCP application route",
                    ));
                }
                format!("L{};", component.class_name.replace('.', "/"))
            }
        };
        let package = apk.manifest.package.clone();
        let mut dexes = Vec::new();
        for name in &apk.dex_files {
            let bytes = apk.zip.read(name)?;
            dexes.push(rd_dex::Dex::parse(bytes).map_err(|e| error(&e.code, e.cause))?);
        }
        if dexes.is_empty() {
            return Err(error("MISSING_ENTRY", "native APK requires DEX glue"));
        }
        let application = apk.manifest.application.name.clone();
        let mut engine = Engine::new(dexes, VmConfig::default());
        engine.set_resources_arc(apk);
        if let Some(a) = application {
            engine.set_application_class(format!("L{};", a.replace('.', "/")));
        }
        Ok(Self {
            id: id.into(),
            package,
            activity,
            engine,
            last_result: serde_json::Value::Null,
            launched: false,
        })
    }
    pub fn into_engine(self) -> Engine {
        self.engine
    }
    pub fn engine_id(&self) -> &str {
        &self.id
    }
    pub fn report(&self) -> serde_json::Value {
        let native = self.engine.native_report();
        serde_json::json!({
            "engine": self.id,
            "package": self.package,
            "activity": self.activity,
            "launched": self.launched,
            "execution_claimed": native["instructions"].as_u64().unwrap_or(0) > 0,
            "language_attribution": "selected explicitly, not inferred from ELF",
            "native": native,
            "last_result": self.last_result,
            "ui": self.engine.dump_ui(),
            "limitations": "bounded DEX/ELF/JNI subset; not universal NDK, STL or Rust std/async",
        })
    }
    pub(crate) fn event(&mut self, event: EngineEvent<'_>) -> RdResult<()> {
        match event.name {
            "launch" => {
                if !event.payload.is_empty() {
                    return Err(error("INVALID_FORMAT", "launch payload must be empty"));
                }
                self.engine
                    .launch_app(&self.activity, &self.package, Vec::new())
                    .map_err(vm_error)?;
                self.launched = true;
                Ok(())
            }
            "invoke" => {
                if !self.launched {
                    return Err(error("INVALID_STATE", "launch before invoking methods"));
                }
                let call = Call::parse(event.payload)?;
                let sig = format!("({})I", "I".repeat(call.args.len()));
                let args: Vec<Value> = call.args.into_iter().map(Value::Int).collect();
                let value = self
                    .engine
                    .invoke_static(&call.class, &call.method, &sig, &args)
                    .map_err(vm_error)?;
                self.last_result = match value {
                    Value::Int(v) => serde_json::json!(v),
                    _ => return Err(error("INVALID_RETURN", "integer invocation expected")),
                };
                Ok(())
            }
            _ => Err(error(
                "NOT_IMPLEMENTED",
                format!("unsupported native event {}", event.name),
            )),
        }
    }
    pub(crate) fn syscall(&mut self, call: EngineSyscall<'_>) -> RdResult<()> {
        match (call.name, call.args) {
            ("advance_clock", [ms]) if *ms <= 60000 => {
                self.engine.advance_clock(*ms).map_err(vm_error)?;
                Ok(())
            }
            _ => Err(error(
                "NOT_IMPLEMENTED",
                "arbitrary host syscalls are not exposed; advance_clock accepts one value <=60000",
            )),
        }
    }
}

#[cfg(test)]
mod invocation_tests {
    use super::*;

    fn invalid(payload: &[u8]) -> RdError {
        let error = Call::parse(payload).unwrap_err();
        assert_eq!(error.code, "INVALID_FORMAT");
        assert_eq!(error.module_id, "rd-native-engines");
        assert!(!error.cause.is_empty());
        assert!(error.suggestion.as_deref().is_some_and(|s| !s.is_empty()));
        error
    }

    #[test]
    fn invocation_accepts_zero_and_sixteen_signed_arguments() {
        for args in [vec![], [i32::MIN, i32::MAX].repeat(8)] {
            let payload = serde_json::to_vec(&serde_json::json!({
                "class": "Ldemo/Native;", "method": "digest", "args": args,
            }))
            .unwrap();
            let call = Call::parse(&payload).unwrap();
            assert_eq!(call.class, "Ldemo/Native;");
            assert_eq!(call.method, "digest");
            assert_eq!(call.args, args);
        }
    }

    #[test]
    fn invocation_enforces_exact_payload_byte_boundary_before_json_parsing() {
        let mut payload = br#"{"class":"Ldemo/Native;","method":"digest","args":[]}"#.to_vec();
        payload.resize(MAX_INVOKE_PAYLOAD_BYTES, b' ');
        assert!(Call::parse(&payload).is_ok());
        payload.push(b' ');
        assert_eq!(invalid(&payload).cause, "invoke payload too large");
        payload.fill(0xff);
        assert_eq!(invalid(&payload).cause, "invoke payload too large");
    }

    #[test]
    fn invocation_rejects_seventeen_arguments() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "class": "Ldemo/Native;", "method": "digest", "args": vec![0; 17],
        }))
        .unwrap();
        assert_eq!(invalid(&payload).cause, "at most16integer arguments");
    }

    #[test]
    fn invocation_rejects_malformed_unknown_duplicate_and_non_integer_fields() {
        for payload in [
            b"".as_slice(),
            b"\xff",
            br#"null"#,
            br#"{}"#,
            br#"{"class":"Ldemo/Native;","args":[]}"#,
            br#"{"class":"Ldemo/Native;","method":"digest","args":[],"unknown":1}"#,
            br#"{"class":"a","class":"b","method":"digest","args":[]}"#,
            br#"{"class":"a","method":"digest","args":[2147483648]}"#,
            br#"{"class":"a","method":"digest","args":[-2147483649]}"#,
            br#"{"class":"a","method":"digest","args":[1.0]}"#,
            br#"{"class":"a","method":"digest","args":[true]}"#,
            br#"{"class":"a","method":"digest","args":["1"]}"#,
            br#"{"class":"a","method":"digest","args":[]}{}"#,
        ] {
            invalid(payload);
        }
        // Moving the DTO out of event() must not rename existing wire errors.
        assert_eq!(
            invalid(b"null").cause,
            "invalid type: null, expected struct Call at line 1 column 4"
        );
        assert_eq!(
            invalid(b"[]").cause,
            "invalid length 0, expected struct Call with 3 elements at line 1 column 2"
        );
    }
}
