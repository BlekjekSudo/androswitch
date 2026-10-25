//! rd-dex — parser DEX 100% + disassembler smali fiel (M1).
//!
//! Parsing total do formato DEX (header, map list, strings MUTF-8, types,
//! protos, fields, methods, classes, code items, debug info, annotations,
//! call sites, method handles, hiddenapi tolerado) + disassembler textual
//! no formato smali (alvo de fidelidade: baksmali 2.5.2).
//!
//! Lei 1: falha sempre tipada via `RdError {code, cause, suggestion, module_id}`
//! — nunca pânico; todo read é bounds-checked (estilo fuzz-hardened do rd-apk).
//! Lei 2: nada se perde — os bytes brutos ficam preservados em `Dex::data` e os
//! offsets de cada seção ficam expostos junto do modelo tipado.

pub mod annotations;
pub mod classes;
pub mod code;
pub mod debug;
pub mod dex;
pub mod disasm;
pub mod error;
pub mod fields;
pub mod lazy;
pub mod methods;
pub mod mutf8;
pub mod opcode;
pub mod protos;
pub mod read;
pub mod strings;
pub mod types;

pub use dex::{Dex, Header, MapItem, MapType};
pub use error::{RdError, RdResult};
pub use lazy::{DexBacking, LazyDex, RawClassIndex};

pub const MODULE_ID: &str = "rd-dex";
/// Milestone em que este módulo entrou de verdade (spec PARTE 4).
pub const MILESTONE: &str = "M1";
