//! Generic, bounded method-probe protocol, shared by the host and Horizon.
use alloc::{boxed::Box, rc::Rc};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use as_platform::fs::FileSystem;
use rd_vm::{Engine, VmConfig, VmExit};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub session_id: String,
    pub calls: Vec<Invocation>,
    #[serde(default)]
    pub storage: Option<StoreRequest>,
    #[serde(default)]
    pub native: Vec<crate::native_probe::NativeRequest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreRequest {
    pub operation: StoreOperation,
    pub package: Option<String>,
    pub revision: Option<u64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreOperation {
    Install,
    Update,
    Launch,
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
    pub installation: Option<as_package::Record>,
    pub native: Vec<crate::native_probe::NativeResult>,
    pub jni_statistics: Option<JniStatistics>,
}
#[derive(Serialize)]
pub struct JniStatistics {
    pub libraries_loaded: u64,
    pub native_calls: u64,
    pub dex_callbacks: u64,
}

pub fn run(apk: Vec<u8>, request: Request) -> Result<Report, Failure> {
    run_with_fs(apk, request, None)
}
pub fn run_with_fs(
    mut apk: Vec<u8>,
    request: Request,
    filesystem: Option<Rc<dyn FileSystem>>,
) -> Result<Report, Failure> {
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
    let mut installation = None;
    if let Some(store) = &request.storage {
        let manager = as_package::PackageManager::new(
            filesystem
                .clone()
                .ok_or_else(|| failure("STORAGE_UNAVAILABLE", "no filesystem adapter"))?,
        );
        let platform = |e: as_platform::PlatformError| Failure {
            code: e.code.into(),
            module_id: "as-package".into(),
            cause: e.to_string(),
        };
        let record = match store.operation {
            StoreOperation::Install | StoreOperation::Update => manager
                .install(apk, matches!(store.operation, StoreOperation::Update), None)
                .map_err(platform)?,
            StoreOperation::Launch => {
                manager
                    .load(
                        store
                            .package
                            .as_deref()
                            .ok_or_else(|| failure("INVALID_REQUEST", "launch requires package"))?,
                        store.revision,
                    )
                    .map_err(platform)?
                    .0
            }
        };
        if store.package.as_ref().is_some_and(|p| p != &record.package) {
            return Err(failure(
                "PACKAGE_MISMATCH",
                "request package differs from installed APK",
            ));
        }
        apk = manager
            .load(&record.package, Some(record.revision))
            .map_err(platform)?
            .1;
        installation = Some(record);
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
    let mut vm = Box::new(Engine::new(
        dexes,
        VmConfig {
            fuel: 1_000_000,
            max_depth: 16,
            heap_budget: 32 << 20,
            exception_trace: true,
        },
    ));
    if let Some(fs) = filesystem {
        vm.bind_storage(
            &apk.manifest.package,
            Box::new(
                as_platform::vfs::Vfs::new(fs, &apk.manifest.package)
                    .map_err(|e| failure(e.code, e.to_string()))?,
            ),
        )
        .map_err(|e| failure("STORAGE_UNAVAILABLE", e.to_string()))?;
    }
    #[cfg(target_os = "horizon")]
    let native_session = if !apk.native_libs.is_empty() && request.native.is_empty() {
        let session = crate::device_jni::Session::new(&apk)
            .map_err(|e| failure("JNI_INITIALIZATION", e.to_string()))?;
        session.bind_vm(&mut vm);
        vm.bind_native(session.clone());
        Some(session)
    } else {
        None
    };
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
        jni: cfg!(target_os = "horizon")
            && !apk.native_libs.is_empty()
            && request.native.is_empty(),
        calls: Vec::new(),
        installation,
        native: crate::native_probe::run(&apk, &request.native),
        jni_statistics: None,
    };
    for call in request.calls {
        #[cfg(target_os = "horizon")]
        crate::device_jni::lock();
        vm.reset_fuel_window();
        let before = vm.fuel_used();
        let result = invoke(&mut vm, &call);
        #[cfg(target_os = "horizon")]
        crate::device_jni::unlock();
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
    #[cfg(target_os = "horizon")]
    if let Some(session) = native_session {
        crate::device_jni::lock();
        session.close_vm();
        let (libraries_loaded, native_calls, dex_callbacks) = session.statistics();
        report.jni_statistics = Some(JniStatistics {
            libraries_loaded,
            native_calls,
            dex_callbacks,
        });
        report.jni = native_calls != 0;
        crate::device_jni::unlock();
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
    run_json_with_fs(apk, input, None)
}
pub fn run_json_with_fs(apk: Vec<u8>, input: &[u8], fs: Option<Rc<dyn FileSystem>>) -> Vec<u8> {
    let result = serde_json::from_slice::<Request>(input)
        .map_err(|e| failure("INVALID_REQUEST", e.to_string()))
        .and_then(|request| run_with_fs(apk, request, fs));
    match result {
        Ok(report) => serde_json::to_vec(&report).expect("report serialization"),
        Err(error) => serde_json::to_vec(&error).expect("error serialization"),
    }
}
