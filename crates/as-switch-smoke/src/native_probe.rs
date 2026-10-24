//! Descriptor-driven native method probe, using one loader on both backends.
use alloc::{collections::BTreeMap, rc::Rc, string::String, vec::Vec};
use as_native::{error, Result};
#[cfg(any(target_os = "horizon", feature = "host-native"))]
use as_native::{CallFrame, Loader};
use rd_vm::Value;
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub library: String,
    pub symbol: String,
    pub descriptor: String,
    pub args: Vec<String>,
}
#[derive(Serialize)]
pub struct NativeResult {
    pub library: String,
    pub symbol: String,
    pub value: Option<String>,
    pub error: Option<as_native::Error>,
}
struct ApkLibraries {
    zip: rd_apk::Zip,
    entries: BTreeMap<String, String>,
}
impl as_native::LibrarySource for ApkLibraries {
    fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let path = self
            .entries
            .get(name)
            .ok_or_else(|| error("LIBRARY_NOT_FOUND", "load", name, "APK ARM64 entry"))?;
        let bytes = self
            .zip
            .find(path)
            .map_or(u64::MAX, |entry| entry.uncompressed_size);
        let limit = 128u64 << 20;
        #[cfg(target_os = "horizon")]
        let limit = {
            #[allow(unsafe_code)]
            extern "C" {
                fn as_host_memory_available() -> u64;
                fn as_memory_stage(stage: *const core::ffi::c_char);
            }
            // SAFETY: scalar allocator snapshot; checkpoint is a static C string.
            #[allow(unsafe_code)]
            let free = unsafe {
                as_memory_stage(c"native_extract_begin".as_ptr());
                as_host_memory_available()
            };
            // Input, parsed segments and relocation image coexist temporarily.
            limit.min(free.saturating_sub(128 << 20) / 3)
        };
        if bytes > limit {
            return Err(error("NATIVE_MEMORY_BUDGET", "extract", name, alloc::format!("native entry needs {bytes} bytes; extraction budget {limit} bytes (128 MiB cap, allocator reserve and staging costs)")));
        }
        self.zip
            .read_bounded(path, limit)
            .map_err(|e| error(&e.code, "extract", name, e.cause))
    }
}
pub fn library_source(apk: &rd_apk::Apk) -> Rc<dyn as_native::LibrarySource> {
    let entries = apk
        .native_libs
        .iter()
        .filter_map(|path| {
            let name = path.strip_prefix("lib/arm64-v8a/")?;
            (!name.contains('/')).then(|| (String::from(name), path.clone()))
        })
        .collect();
    Rc::new(ApkLibraries {
        zip: apk.zip.clone(),
        entries,
    })
}
pub fn sources(apk: &rd_apk::Apk) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut sources = BTreeMap::new();
    for name in &apk.native_libs {
        if let Some(name) = name.strip_prefix("lib/arm64-v8a/") {
            if name.contains('/') {
                continue;
            }
            let bytes = apk
                .zip
                .read(&alloc::format!("lib/arm64-v8a/{name}"))
                .map_err(|e| error(&e.code, "extract", name, e.cause))?;
            sources.insert(name.into(), bytes);
        }
    }
    Ok(sources)
}
pub fn run(apk: &rd_apk::Apk, calls: &[NativeRequest]) -> Vec<NativeResult> {
    #[cfg(target_os = "horizon")]
    let backend: Result<_> = Ok(crate::device_native::HorizonNative::default());
    #[cfg(all(not(target_os = "horizon"), feature = "host-native"))]
    let backend = as_native::oracle::Oracle::new();
    #[cfg(all(not(target_os = "horizon"), not(feature = "host-native")))]
    {
        let _ = apk;
        calls
            .iter()
            .map(|c| NativeResult {
                library: c.library.clone(),
                symbol: c.symbol.clone(),
                value: None,
                error: Some(error(
                    "NATIVE_BACKEND_UNAVAILABLE",
                    "probe",
                    &c.library,
                    "host-native feature or Horizon backend",
                )),
            })
            .collect()
    }
    #[cfg(any(target_os = "horizon", feature = "host-native"))]
    {
        let mut loader = backend.and_then(|backend| Ok(Loader::new(backend, sources(apk)?)));
        calls
            .iter()
            .map(|request| {
                let result = match &mut loader {
                    Ok(loader) => invoke(loader, request),
                    Err(e) => Err(e.clone()),
                };
                let (value, error) = match result {
                    Ok(v) => (Some(v), None),
                    Err(e) => (None, Some(e)),
                };
                NativeResult {
                    library: request.library.clone(),
                    symbol: request.symbol.clone(),
                    value,
                    error,
                }
            })
            .collect()
    }
}
#[cfg(any(target_os = "horizon", feature = "host-native"))]
fn invoke(loader: &mut Loader<Backend>, request: &NativeRequest) -> Result<String> {
    let i = loader.load(&request.library)?;
    for index in 0..loader.modules.len() {
        if !loader.modules[index].initialized {
            for function in loader.modules[index].constructors.clone() {
                let mut frame = CallFrame {
                    function,
                    ..Default::default()
                };
                execute(&mut loader.backend, &mut frame)?;
            }
            loader.modules[index].initialized = true;
        }
    }
    let (types, result) = rd_jni::descriptor(&request.descriptor)
        .map_err(|e| error(&e.code, "descriptor", &request.library, e.cause))?;
    if types.len() != request.args.len() {
        return Err(error(
            "INVALID_ARGUMENT",
            "probe",
            &request.library,
            "descriptor argument count",
        ));
    }
    let values = types
        .iter()
        .zip(&request.args)
        .map(|(ty, text)| {
            let value = rd_vm::parse_arg_value(text, ty)
                .map_err(|e| error("INVALID_ARGUMENT", "parse", &request.library, e))?;
            bits(&value)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut frame = CallFrame {
        function: loader.module_symbol(i, &request.symbol).ok_or_else(|| {
            error(
                "SYMBOL_NOT_FOUND",
                "probe",
                &request.library,
                &request.symbol,
            )
        })?,
        ..Default::default()
    };
    frame.arguments(&types, &values)?;
    execute(&mut loader.backend, &mut frame)?;
    let bits = if result == "F" || result == "D" {
        frame.result_float[0]
    } else {
        frame.result_integer[0]
    };
    let value = match result.as_str() {
        "V" => Value::Null,
        "J" => Value::Long(bits as i64),
        "D" => Value::Double(f64::from_bits(bits)),
        "F" => Value::Float(f32::from_bits(bits as u32)),
        _ => Value::Int(bits as i32),
    };
    Ok(rd_vm::interp::render_result(&value, &result))
}
pub fn bits(v: &Value) -> Result<u64> {
    match v {
        Value::Int(v) => Ok(*v as u32 as u64),
        Value::Long(v) => Ok(*v as u64),
        Value::Float(v) => Ok(v.to_bits() as u64),
        Value::Double(v) => Ok(v.to_bits()),
        Value::Null => Ok(0),
        _ => Err(error(
            "INVALID_ARGUMENT",
            "native",
            "JNI",
            "scalar value or JNI handle",
        )),
    }
}
#[cfg(target_os = "horizon")]
type Backend = crate::device_native::HorizonNative;
#[cfg(all(not(target_os = "horizon"), feature = "host-native"))]
type Backend = as_native::oracle::Oracle;
#[cfg(target_os = "horizon")]
fn execute(_backend: &mut Backend, frame: &mut CallFrame) -> Result<()> {
    crate::device_native::call(frame)
}
#[cfg(all(not(target_os = "horizon"), feature = "host-native"))]
fn execute(backend: &mut Backend, frame: &mut CallFrame) -> Result<()> {
    backend.call(frame)
}
