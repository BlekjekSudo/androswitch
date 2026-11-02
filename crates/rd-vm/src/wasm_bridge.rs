//! Bridge `io.rustydroid.wasm.WasmBridge` — M11 S1b (ADR 013 §3).
//!
//! Contract (frozen for S1b, documented in `docs/m11-wasm.md`):
//!
//! ```text
//! static int call(String asset, String export, int a, int b)
//! ```
//!
//! Reads `assets/<asset>` from the attached APK (same hostile-path rule as
//! `AssetManager`), instantiates the module with a fresh instance — nothing
//! is carried between calls — and invokes `export(a, b)` under the
//! per-invocation fuel budget. The export must return exactly one `i32`.
//!
//! Every failure becomes an `IllegalStateException` whose message starts with
//! the stable `rd-engine-wasm` error code (for example `FUEL_EXHAUSTED: ...`).

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::value::Value;
use rd_engine_wasm::{limits, Instance, Module, Value as WValue, WasmError};

/// Internal class name of the bridge, as the DEX refers to it.
pub const BRIDGE_CLASS: &str = "Lio/rustydroid/wasm/WasmBridge;";
/// Method descriptor of `WasmBridge.call`.
pub const CALL_SIG: &str = "(Ljava/lang/String;Ljava/lang/String;II)I";

const ILLEGAL_ARG: &str = "Ljava/lang/IllegalArgumentException;";
const ILLEGAL_STATE: &str = "Ljava/lang/IllegalStateException;";
const NPE: &str = "Ljava/lang/NullPointerException;";

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg.into()))
}

fn wasm_exc(e: WasmError) -> VmExit {
    exc(ILLEGAL_STATE, format!("{}: {}", e.code, e.cause))
}

/// Static dispatch hook. Returns `Ok(None)` for anything that is not the
/// bridge's `call`, so the rest of the host chain keeps its order.
pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != BRIDGE_CLASS || name != "call" || sig != CALL_SIG {
        return Ok(None);
    }
    let [asset, export, a, b] = args else {
        return Err(exc(ILLEGAL_ARG, "WasmBridge.call expects 4 arguments"));
    };
    let asset = string_arg(vm, asset, "asset")?;
    let export = string_arg(vm, export, "export")?;
    let (Value::Int(a), Value::Int(b)) = (a, b) else {
        return Err(exc(
            ILLEGAL_ARG,
            "WasmBridge.call expects int arguments a, b",
        ));
    };
    let bytes = read_asset(vm, &asset)?;
    let module = Module::parse(&bytes).map_err(wasm_exc)?;
    let mut instance = Instance::new(module, limits::MAX_FUEL).map_err(wasm_exc)?;
    let outcome = instance
        .invoke(
            &export,
            &[WValue::I32(*a), WValue::I32(*b)],
            limits::MAX_FUEL,
        )
        .map_err(wasm_exc)?;
    match outcome.results.as_slice() {
        [WValue::I32(v)] => Ok(Some(Value::Int(*v))),
        _ => Err(exc(
            ILLEGAL_STATE,
            format!("INVALID_ARG: export '{export}' must return exactly one i32"),
        )),
    }
}

fn string_arg(vm: &Engine, value: &Value, what: &str) -> Result<String, VmExit> {
    match value {
        Value::Obj(r) => vm
            .heap
            .as_str(*r)
            .map(str::to_string)
            .map_err(|e| exc(ILLEGAL_ARG, format!("{what}: {e}"))),
        Value::Null => Err(exc(NPE, format!("WasmBridge.call: {what} is null"))),
        _ => Err(exc(
            ILLEGAL_ARG,
            format!("WasmBridge.call: {what} must be a String"),
        )),
    }
}

fn read_asset(vm: &mut Engine, file: &str) -> Result<Vec<u8>, VmExit> {
    let entry = format!("assets/{file}");
    if crate::android_res::asset_path_is_hostile(file) {
        return Err(exc("Ljava/io/FileNotFoundException;", entry));
    }
    let apk = vm.resources()?;
    apk.zip
        .read(&entry)
        .map(|b| b.to_vec())
        .map_err(|e| exc("Ljava/io/FileNotFoundException;", format!("{entry}: {e}")))
}
