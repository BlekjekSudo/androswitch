//! Real APK/DEX/ELF/JNI route for C, C++ and Rust. No language guessing.
use crate::{Artifact, EngineEvent, EngineSyscall};
use rd_apk::{Apk, RdError, RdResult};
use rd_vm::{Engine, Value, VmConfig, VmExit};
use serde::Deserialize;

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
        if artifact.bytes.len() > 256 * 1024 * 1024 {
            return Err(error("APK_TOO_LARGE", "native adapter APK exceeds 256 MiB"));
        }
        Self::from_apk(id, Apk::from_bytes(artifact.bytes.to_vec())?)
    }
    /// Prepare an already-parsed APK for the same native route used by MCP.
    pub fn from_apk(id: &str, apk: Apk) -> RdResult<Self> {
        if !crate::native_execution_available(id) {
            return Err(error(
                "NOT_IMPLEMENTED",
                "no executing native adapter for selected ID",
            ));
        }
        if apk.file_size_bytes > 256 * 1024 * 1024 {
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
        let activity = apk
            .manifest
            .application
            .components
            .iter()
            .find(|c| c.is_launcher && c.enabled != Some(false))
            .ok_or_else(|| error("MISSING_ENTRY", "no enabled launcher activity"))?;
        if activity.target_activity.is_some() {
            return Err(error(
                "NOT_IMPLEMENTED",
                "native adapter launch aliases require the MCP application route",
            ));
        }
        let activity = format!("L{};", activity.class_name.replace('.', "/"));
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
        engine.set_resources(apk);
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
        serde_json::json!({"engine":self.id,"package":self.package,"activity":self.activity,"launched":self.launched,"execution_claimed":native["instructions"].as_u64().unwrap_or(0)>0,"language_attribution":"selected explicitly, not inferred from ELF","native":native,"last_result":self.last_result,"ui":self.engine.dump_ui(),"limitations":"bounded DEX/ELF/JNI subset; not universal NDK, STL or Rust std/async"})
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
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Call {
                    class: String,
                    method: String,
                    args: Vec<i32>,
                }
                if event.payload.len() > 16384 {
                    return Err(error("INVALID_FORMAT", "invoke payload too large"));
                }
                let call: Call = serde_json::from_slice(event.payload)
                    .map_err(|e| error("INVALID_FORMAT", e.to_string()))?;
                if call.args.len() > 16 {
                    return Err(error("INVALID_FORMAT", "at most16integer arguments"));
                }
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
