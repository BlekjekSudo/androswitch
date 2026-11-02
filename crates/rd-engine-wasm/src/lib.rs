//! RustyDroid WebAssembly engine (M11 / ADR 013, slice S1).
//!
//! Interpreter for the WebAssembly MVP **integer subset** (i32/i64), written
//! in pure Rust with zero dependencies, `unsafe` forbidden by the workspace
//! lints. It is the runtime behind "WASM hospedado pelo RustyDroid via bridge
//! documentada" (`docs/m11-wasm.md`). It is NOT a general WebAssembly runtime
//! and makes no claim about WASM shipped inside third-party apps.
//!
//! Guarantees:
//! - every module is fully parsed and validated before instantiation; any
//!   structural or type error is a structured [`WasmError`], never a panic;
//! - fuel is charged once per executed instruction, so runs are deterministic;
//! - memory, module size, call depth, value stack and call arguments are bounded
//!   by the constants in [`limits`];
//! - there is no clock, no randomness, no host imports and no I/O in the guest.
//!
//! Out-of-subset features (floats, SIMD, tables / `call_indirect`, imports,
//! multi-value, bulk memory, passive segments) fail with `NOT_IMPLEMENTED`.

#![forbid(unsafe_code)]

mod decode;
mod exec;

use std::fmt;

/// Module id used in every [`WasmError`] produced by this crate.
pub const MODULE_ID: &str = "rd-engine-wasm";

/// Hard limits. Mirrors `native_execution.rs` where the concept exists.
pub mod limits {
    /// Maximum size of a `.wasm` module, in bytes (ADR 013 §5).
    pub const MAX_MODULE_BYTES: usize = 2 * 1024 * 1024;
    /// WebAssembly page size.
    pub const PAGE_SIZE: usize = 64 * 1024;
    /// Maximum linear memory: 256 pages = 16 MiB (ADR 013 §5).
    pub const MAX_MEMORY_PAGES: u32 = 256;
    /// Maximum fuel per invocation (ADR 013 §5: ~1 s on moto-e5 at 0.25×).
    pub const MAX_FUEL: u64 = 10_000_000;
    /// Maximum number of arguments per call (ADR 013 §5).
    pub const MAX_CALL_ARGS: usize = 16;
    /// Maximum call and response payload, in bytes (ADR 013 §5). Enforced by
    /// the bridge at the boundary; exported so both sides share one number.
    pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
    /// Maximum nested call frames.
    pub const MAX_CALL_DEPTH: usize = 256;
    /// Maximum operand-stack depth, in values.
    pub const MAX_VALUE_STACK: usize = 64 * 1024;
    /// Maximum declared locals per function (MVP limit).
    pub const MAX_LOCALS: u64 = 50_000;
    /// Maximum locals alive across the whole call stack, in slots (1 MiB at
    /// 16 bytes per slot). Keeps recursion over large frames inside the budget.
    pub const MAX_TOTAL_LOCALS: usize = 64 * 1024;
    /// Maximum length of an export name, in bytes.
    pub const MAX_NAME_BYTES: usize = 256;
}

/// Value types of the S1 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
}

/// A WebAssembly value of the S1 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    I32(i32),
    I64(i64),
}

impl Value {
    /// The type of this value.
    pub fn ty(&self) -> ValType {
        match self {
            Value::I32(_) => ValType::I32,
            Value::I64(_) => ValType::I64,
        }
    }

    pub(crate) fn zero(ty: ValType) -> Value {
        match ty {
            ValType::I32 => Value::I32(0),
            ValType::I64 => Value::I64(0),
        }
    }
}

/// Structured error. `code` is stable and meant for agents and the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmError {
    /// `PARSE_ERROR`, `INVALID_FORMAT`, `NOT_IMPLEMENTED`, `LIMIT_EXCEEDED`,
    /// `INSTANTIATION_FAILED`, `NOT_FOUND`, `INVALID_ARG`, `TRAP`,
    /// `FUEL_EXHAUSTED`, `STACK_EXHAUSTED`.
    pub code: &'static str,
    /// Technical cause, in English.
    pub cause: String,
    /// Optional actionable hint.
    pub suggestion: Option<String>,
    /// Always [`MODULE_ID`].
    pub module_id: &'static str,
}

impl WasmError {
    pub(crate) fn new(code: &'static str, cause: impl Into<String>) -> Self {
        WasmError {
            code,
            cause: cause.into(),
            suggestion: None,
            module_id: MODULE_ID,
        }
    }

    pub(crate) fn with_suggestion(mut self, s: impl Into<String>) -> Self {
        self.suggestion = Some(s.into());
        self
    }
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.code, self.cause, self.module_id)
    }
}

impl std::error::Error for WasmError {}

/// Kind of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Func,
    Memory,
    Global,
}

/// A parsed and validated module.
#[derive(Debug, Clone)]
pub struct Module {
    def: decode::ModuleDef,
}

impl Module {
    /// Parses and fully validates a binary module.
    pub fn parse(bytes: &[u8]) -> Result<Module, WasmError> {
        Ok(Module {
            def: decode::parse_module(bytes)?,
        })
    }

    /// Exports, in declaration order.
    pub fn exports(&self) -> Vec<(&str, ExportKind)> {
        self.def
            .exports
            .iter()
            .map(|e| {
                let k = match e.kind {
                    decode::ExportKind::Func(_) => ExportKind::Func,
                    decode::ExportKind::Memory => ExportKind::Memory,
                    decode::ExportKind::Global(_) => ExportKind::Global,
                };
                (e.name.as_str(), k)
            })
            .collect()
    }

    /// Number of defined functions.
    pub fn function_count(&self) -> usize {
        self.def.funcs.len()
    }
}

/// Result of a successful [`Instance::invoke`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Returned values (0 or 1 in the S1 subset).
    pub results: Vec<Value>,
    /// Fuel consumed = number of WebAssembly instructions executed (Lei 4).
    pub fuel_used: u64,
}

/// An instantiated module with its own memory and globals.
#[derive(Debug)]
pub struct Instance {
    module: Module,
    state: exec::State,
}

impl Instance {
    /// Instantiates the module: initialises memory, data segments, globals,
    /// then runs the start function, if any, with `start_fuel` budget.
    pub fn new(module: Module, start_fuel: u64) -> Result<Instance, WasmError> {
        check_fuel(start_fuel)?;
        let def = &module.def;
        let (mut memory, max_pages) = match def.memory {
            Some(m) => (
                vec![0u8; m.min as usize * limits::PAGE_SIZE],
                m.max.unwrap_or(limits::MAX_MEMORY_PAGES),
            ),
            None => (Vec::new(), 0),
        };
        for seg in &def.data {
            let off = seg.offset as usize;
            let end = off.checked_add(seg.bytes.len());
            match end {
                Some(e) if e <= memory.len() => memory[off..e].copy_from_slice(&seg.bytes),
                _ => {
                    return Err(WasmError::new(
                        "INSTANTIATION_FAILED",
                        format!(
                            "data segment at {off} does not fit in {} bytes",
                            memory.len()
                        ),
                    ))
                }
            }
        }
        let globals = def.globals.iter().map(|g| g.init).collect();
        let mut inst = Instance {
            state: exec::State {
                memory,
                max_pages,
                globals,
            },
            module,
        };
        if let Some(start) = inst.module.def.start {
            let mut fuel = start_fuel;
            exec::invoke(&inst.module.def, &mut inst.state, start, &[], &mut fuel)?;
        }
        Ok(inst)
    }

    /// Calls an exported function by name. `fuel` bounds the instructions
    /// executed by this call (≤ [`limits::MAX_FUEL`]).
    pub fn invoke(&mut self, name: &str, args: &[Value], fuel: u64) -> Result<Outcome, WasmError> {
        check_fuel(fuel)?;
        if args.len() > limits::MAX_CALL_ARGS {
            return Err(WasmError::new(
                "LIMIT_EXCEEDED",
                format!(
                    "{} arguments exceed the limit of {}",
                    args.len(),
                    limits::MAX_CALL_ARGS
                ),
            ));
        }
        let idx = self
            .module
            .def
            .exports
            .iter()
            .find_map(|e| match (e.name == name, e.kind) {
                (true, decode::ExportKind::Func(i)) => Some(i),
                _ => None,
            })
            .ok_or_else(|| {
                WasmError::new("NOT_FOUND", format!("no exported function named `{name}`"))
            })?;
        let ft = &self.module.def.types[self.module.def.funcs[idx as usize].type_idx as usize];
        if ft.params.len() != args.len() || ft.params.iter().zip(args).any(|(p, a)| *p != a.ty()) {
            return Err(WasmError::new(
                "INVALID_ARG",
                format!(
                    "`{name}` expects {} argument(s) of types {:?}",
                    ft.params.len(),
                    ft.params
                ),
            )
            .with_suggestion("pass values with the exact WebAssembly types of the export"));
        }
        let mut left = fuel;
        let results = exec::invoke(&self.module.def, &mut self.state, idx, args, &mut left)?;
        Ok(Outcome {
            results,
            fuel_used: fuel - left,
        })
    }

    /// Read-only view of linear memory (empty if the module has none).
    pub fn memory(&self) -> &[u8] {
        &self.state.memory
    }

    /// Current global values, in declaration order.
    pub fn globals(&self) -> &[Value] {
        &self.state.globals
    }
}

fn check_fuel(fuel: u64) -> Result<(), WasmError> {
    if fuel > limits::MAX_FUEL {
        Err(WasmError::new(
            "LIMIT_EXCEEDED",
            format!("fuel {fuel} exceeds the limit of {}", limits::MAX_FUEL),
        ))
    } else {
        Ok(())
    }
}
