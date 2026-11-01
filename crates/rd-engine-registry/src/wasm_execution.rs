//! Rota APK real para `wasm` (engine #28, LANG-12), M11 S1c (ADR 013).
//!
//! O DEX do APK roda na VM existente e a classe `io.rustydroid.wasm.WasmBridge`
//! hospeda os módulos `assets/*.wasm` (contrato `call(asset, export, a, b)`).
//! Não há loader de `.wasm` standalone, WASI, threads nem JS/JNI bridge: só o
//! subconjunto documentado em `docs/m11-wasm.md`.
use crate::{Artifact, EngineEvent, EngineSyscall};
use rd_apk::manifest::ComponentKind;
use rd_apk::{Apk, RdError, RdResult};
use rd_vm::{Engine, VmConfig, VmExit};
use std::sync::Arc;

const MAX_APK_BYTES: u64 = 256 * 1024 * 1024;
const MODULE_ID: &str = "rd-engine-wasm";
const ENGINE_ID: &str = "wasm";

pub struct WasmProcess {
    package: String,
    activity: String,
    engine: Engine,
    launched: bool,
}

impl std::fmt::Debug for WasmProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmProcess")
            .field("engine", &ENGINE_ID)
            .field("package", &self.package)
            .field("launched", &self.launched)
            .finish()
    }
}

fn error(code: &str, cause: impl Into<String>) -> RdError {
    RdError::new(code, cause, MODULE_ID).with_suggestion(
        "see docs/m11-wasm.md; use an APK with classes.dex and assets/*.wasm, launch it first, and use only the documented WasmBridge subset",
    )
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

impl WasmProcess {
    pub(crate) fn parse(artifact: Artifact<'_>) -> RdResult<Self> {
        if artifact.bytes.len() as u64 > MAX_APK_BYTES {
            return Err(error("APK_TOO_LARGE", "wasm adapter APK exceeds 256 MiB"));
        }
        Self::from_apk(Apk::from_bytes(artifact.bytes.to_vec())?)
    }

    fn from_apk(apk: Apk) -> RdResult<Self> {
        if apk.file_size_bytes > MAX_APK_BYTES {
            return Err(error("APK_TOO_LARGE", "wasm adapter APK exceeds 256 MiB"));
        }
        if apk.manifest.min_sdk < 26 {
            return Err(error(
                "UNSUPPORTED_API_FLOOR",
                "wasm execution requires minSdkVersion 26 or greater",
            ));
        }
        if !apk
            .assets
            .iter()
            .any(|a| a.starts_with("assets/") && a.ends_with(".wasm"))
        {
            return Err(error(
                "MISSING_ENTRY",
                "wasm APK has no assets/*.wasm module",
            ));
        }
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
                "wasm adapter launch aliases are not implemented",
            ));
        }
        let activity = format!("L{};", component.class_name.replace('.', "/"));
        let package = apk.manifest.package.clone();
        let mut dexes = Vec::new();
        for name in &apk.dex_files {
            let bytes = apk.zip.read(name)?;
            dexes.push(rd_dex::Dex::parse(bytes).map_err(|e| error(&e.code, e.cause))?);
        }
        if dexes.is_empty() {
            return Err(error("MISSING_ENTRY", "wasm APK requires DEX glue"));
        }
        let application = apk.manifest.application.name.clone();
        let mut engine = Engine::new(dexes, VmConfig::default());
        engine.set_resources_arc(Arc::new(apk));
        if let Some(a) = application {
            engine.set_application_class(format!("L{};", a.replace('.', "/")));
        }
        Ok(Self {
            package,
            activity,
            engine,
            launched: false,
        })
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "engine": ENGINE_ID,
            "package": self.package,
            "activity": self.activity,
            "launched": self.launched,
            // Medido: a Activity rodou onCreate sem exceção; o resultado visível
            // (TextView/UI) é o que comprova a chamada WasmBridge.
            "execution_claimed": self.launched,
            "wasm": {
                "bridge": "io.rustydroid.wasm.WasmBridge",
                "contract": "static int call(String asset, String export, int a, int b)",
                "fresh_instance_per_call": true,
                "returns": "exactly one i32",
            },
            "ui": self.engine.dump_ui(),
            "limitations": "WasmBridge MVP i32/i64 com fuel por chamada; sem WASI, threads, JS/JNI bridge nem loader standalone de .wasm",
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
            _ => Err(error(
                "NOT_IMPLEMENTED",
                format!("unsupported wasm event {}", event.name),
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
