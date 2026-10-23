//! Generic, bounded method-probe protocol, shared by the host and Horizon.
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use rd_vm::{Engine, VmConfig, VmExit};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub session_id: String,
    pub calls: Vec<Invocation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub id: String,
    pub class: String,
    pub method: String,
    pub descriptor: String,
    pub args: Vec<String>,
}
#[derive(Serialize)]
pub struct Failure {
    pub code: String,
    pub module_id: String,
    pub cause: String,
}
impl From<rd_apk::RdError> for Failure {
    fn from(e: rd_apk::RdError) -> Self {
        Self {
            code: e.code,
            module_id: e.module_id,
            cause: e.cause,
        }
    }
}
impl From<rd_dex::RdError> for Failure {
    fn from(e: rd_dex::RdError) -> Self {
        Self {
            code: e.code,
            module_id: e.module_id,
            cause: e.cause,
        }
    }
}
fn failure(code: &str, cause: impl Into<String>) -> Failure {
    Failure {
        code: code.to_string(),
        module_id: "as-switch-smoke".to_string(),
        cause: cause.into(),
    }
}
#[derive(Serialize)]
pub struct CallResult {
    pub id: String,
    pub value: Option<String>,
    pub error: Option<Failure>,
    pub instructions: u64,
}
#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub kind: &'static str,
    pub session_id: String,
    pub package: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub dex_files: usize,
    pub portable_vm: bool,
    pub application_lifecycle: bool,
    pub jni: bool,
    pub calls: Vec<CallResult>,
}

pub fn run(apk: Vec<u8>, request: Request) -> Result<Report, Failure> {
    if request.schema_version != 1
        || request.session_id.is_empty()
        || request.session_id.len() > 128
        || request.calls.len() > 128
    {
        return Err(failure(
            "INVALID_REQUEST",
            "schema_version=1, nonempty session <=128 bytes and <=128 calls required",
        ));
    }
    let apk = rd_apk::Apk::from_bytes(apk)?;
    if apk.manifest.min_sdk > 26 {
        return Err(failure(
            "SDK_UNSUPPORTED",
            format!("minSdk {} exceeds API 26", apk.manifest.min_sdk),
        ));
    }
    let mut dexes = Vec::new();
    for name in &apk.dex_files {
        let dex = rd_dex::Dex::parse(apk.zip.read(name)?)?;
        if !dex.verify_checksum() {
            return Err(failure("INVALID_CHECKSUM", format!("DEX checksum: {name}")));
        }
        dexes.push(dex);
    }
    if dexes.is_empty() {
        return Err(failure("MISSING_DEX", "no DEX entries in APK"));
    }
    let mut vm = Engine::new(
        dexes,
        VmConfig {
            fuel: 1_000_000,
            max_depth: 16,
            heap_budget: 32 << 20,
            exception_trace: true,
        },
    );
    let mut report = Report {
        schema_version: 1,
        kind: "apk_dex_method_probe",
        session_id: request.session_id,
        package: apk.manifest.package.clone(),
        min_sdk: apk.manifest.min_sdk,
        target_sdk: apk.manifest.target_sdk,
        dex_files: apk.dex_files.len(),
        portable_vm: rd_vm::PORTABLE_CORE,
        application_lifecycle: false,
        jni: false,
        calls: Vec::new(),
    };
    for call in request.calls {
        vm.reset_fuel_window();
        let before = vm.fuel_used();
        let result = invoke(&mut vm, &call);
        let (value, error) = match result {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        report.calls.push(CallResult {
            id: call.id,
            value,
            error,
            instructions: vm.fuel_used() - before,
        });
    }
    Ok(report)
}
fn invoke(vm: &mut Engine, call: &Invocation) -> Result<String, Failure> {
    let params = rd_vm::engine::parse_param_types(&call.descriptor)
        .ok_or_else(|| failure("INVALID_DESCRIPTOR", &call.descriptor))?;
    if params.len() != call.args.len() {
        return Err(failure("INVALID_ARGUMENT", &call.descriptor));
    }
    let args = params
        .iter()
        .zip(&call.args)
        .map(|(kind, text)| {
            rd_vm::parse_arg_value(text, kind).map_err(|e| failure("INVALID_ARGUMENT", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let value = vm
        .invoke_static(&call.class, &call.method, &call.descriptor, &args)
        .map_err(|error| match error {
            VmExit::Error(error) => error.into(),
            VmExit::Exception(error) => failure(
                "JAVA_EXCEPTION",
                format!("{}: {}", error.class, error.message.unwrap_or_default()),
            ),
        })?;
    let return_type = call
        .descriptor
        .rsplit_once(')')
        .ok_or_else(|| failure("INVALID_DESCRIPTOR", &call.descriptor))?
        .1;
    Ok(rd_vm::interp::render_result(&value, return_type))
}
pub fn run_json(apk: Vec<u8>, input: &[u8]) -> Vec<u8> {
    let result = serde_json::from_slice::<Request>(input)
        .map_err(|e| failure("INVALID_REQUEST", e.to_string()))
        .and_then(|request| run(apk, request));
    match result {
        Ok(report) => serde_json::to_vec(&report).expect("report serialization"),
        Err(error) => serde_json::to_vec(&error).expect("error serialization"),
    }
}
