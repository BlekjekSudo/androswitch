//! rd-vm — VM Dalvik (interpretador → JIT Cranelift) + GC arena + threads.
//!
//! M2: interpretador mínimo — opcodes núcleo, invocações, exceções, strings,
//! fields; métodos puros de APK real retornam valores corretos (golden
//! vectors vs execução real). Heap arena com o orçamento do piso moto-e5
//! (heap ≤ 256 MB — PRF-06/A2); OOM é erro tipado, nunca abort (Lei 1).

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
#[cfg(all(test, not(feature = "std")))]
extern crate std;

use alloc::{
    format,
    string::{String, ToString},
};

mod app_output;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
mod collection_clock;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
mod module_tables;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
mod port_env;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
mod port_float;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
mod port_prelude;
pub use app_output::AppOutput;
#[cfg(all(not(feature = "desktop-runtime"), feature = "upstream-runtime"))]
mod reference_filesystem;
mod sdk_types;
mod throwable_message;
#[cfg(all(not(feature = "desktop-runtime"), feature = "upstream-runtime"))]
pub use reference_filesystem::ReferenceFileSystem;
pub mod framework_host;
#[cfg(not(feature = "desktop-runtime"))]
pub mod native_host;
#[cfg(not(feature = "desktop-runtime"))]
mod portable;
/// Internal contracts exposed only to the isolated portable host test target.
#[doc(hidden)]
#[cfg(all(not(feature = "desktop-runtime"), feature = "portable-test-support"))]
pub mod portable_test_support {
    pub use crate::portable::execution::contracts::*;
}
#[cfg(feature = "std")]
pub(crate) mod maps {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub use std::collections::hash_map::Entry;
    pub use std::collections::{HashMap, HashSet};
}
#[cfg(not(feature = "std"))]
pub(crate) mod maps {
    #[cfg(feature = "upstream-runtime")]
    pub use alloc::collections::btree_map::Entry;
    pub use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet};
}

#[cfg(feature = "advanced-components")]
pub mod accessibility_node;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod activity_callbacks;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod activity_extras;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod android_build;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_colorspace;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_display;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_drawable;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_env;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_json;
pub mod android_log;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_motion;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_observable;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_pm;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_region;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_res;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_spannable;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_stub_surfaces;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_sysservices;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_text;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_uri;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_util;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod android_velocity;
/// Apêndice D — shims host AndroidX/Jetpack (data/jetpack.toml).
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod androidx;
pub mod arrays;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod atomics;
pub mod boxes;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod bundle;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod choreographer;
pub mod classpath;
pub mod collection_values;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod collections;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod content;
pub mod engine;
pub mod err;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod framework;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod gc;
#[cfg(all(not(feature = "desktop-runtime"), not(feature = "upstream-runtime")))]
#[path = "portable/gc.rs"]
pub mod gc;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod gms;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod graphics;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod handler_msg;
pub mod heap;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod inflate;
mod integer_bits;
pub mod interp;
#[cfg(feature = "desktop-runtime")]
pub mod intrinsics;
#[cfg(not(feature = "desktop-runtime"))]
#[path = "portable/intrinsics.rs"]
pub mod intrinsics;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jannotations;
mod java_format;
mod java_values;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jbigdecimal;
pub mod jbitset;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jbreakiter;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jcollections;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jfile;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jio_chars;
#[cfg(feature = "jit")]
#[cfg(feature = "desktop-runtime")]
pub mod jit;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jiterator;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jlclass;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jlocks;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jlstring;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jlsystem;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jnormalizer;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jpriorityqueue;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jrandom;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jregex;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jresources;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jsb;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jsecurity;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jsorted;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jsqlite;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jstring_tokenizer;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jtext_format;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jtime;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod juuid;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod jxml;
#[cfg(feature = "virtual-devices")]
pub mod m10;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod m8;
#[cfg(feature = "advanced-components")]
pub mod m9;
#[cfg(feature = "native")]
#[cfg(feature = "desktop-runtime")]
pub mod native;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod numerics;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod parcel;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod perms;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod platform_base;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod prefs;
mod primitive_parse;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod reflect;
pub mod repr;
mod shared_call;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub mod threads;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod throwable;
#[cfg(all(not(feature = "desktop-runtime"), feature = "upstream-runtime"))]
#[path = "intrinsics.rs"]
pub(crate) mod upstream_intrinsics;
pub mod value;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod view_attach;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod view_layout;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod view_props;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod vto;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod window;
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) mod xml_res;

use rd_dex::annotations::EncodedValue;
use rd_dex::Dex;

#[cfg(feature = "desktop-runtime")]
pub(crate) use intrinsics::print_sink as upstream_print_sink;

pub use engine::{ClassCoverage, CoverageSnapshot, Engine, VmConfig};
pub use err::{Throwable, VmExit};
pub use heap::ObjRef;
#[cfg(feature = "desktop-runtime")]
pub use intrinsics::{route_app_stdout_to_stderr, set_app_stdout, HostOut};
pub use value::Value;

/// Módulo do contrato de erro.
pub const MODULE_ID: &str = "rd-vm";
/// Explicit evidence of the linked VM variant (Cargo features are additive).
pub const PORTABLE_CORE: bool = !cfg!(feature = "desktop-runtime");
/// Actual Rust library profile, independent of the execution backend.
pub const RUST_STD_ENABLED: bool = cfg!(feature = "std");

/// Legacy host boundary, retained for callers built against RustyDroid.
/// Object-indexed module tables are now Engine-owned and drop with their heap;
/// constructing a second Engine cannot reset another live Engine's state.
/// Persistent filesystem/session data is still reset through its own adapter.
#[cfg(feature = "desktop-runtime")]
pub fn reset_host_thread_state() {}

/// Issue #230: apaga o armazenamento privado de um pacote no VFS
/// (`/data/user/0/<pkg>/`) — backend de `package_clear`/`package_remove`.
#[cfg(feature = "desktop-runtime")]
pub use jfile::clear_package_data;
/// Issue #230: zera o VFS inteiro — fronteira de sessão nova.
#[cfg(feature = "desktop-runtime")]
pub use jfile::reset_all as reset_vfs;

/// Converte um `EncodedValue` (static_values de class_def) em `Value`,
/// alocando strings no heap da VM. `None` = tipo não representável no M2
/// (arrays/annotations aninhadas em static_values — raras; o campo fica
/// com o default).
pub fn value_from_encoded(ev: &EncodedValue, dex: &Dex) -> Option<Value> {
    Some(match ev {
        EncodedValue::Byte(v) => Value::Int(*v as i32),
        EncodedValue::Short(v) => Value::Int(*v as i32),
        EncodedValue::Char(v) => Value::Int(*v as i32),
        EncodedValue::Int(v) => Value::Int(*v),
        EncodedValue::Long(v) => Value::Long(*v),
        EncodedValue::Float(v) => Value::Float(*v),
        EncodedValue::Double(v) => Value::Double(*v),
        EncodedValue::Boolean(v) => Value::Int(*v as i32),
        EncodedValue::Null => Value::Null,
        EncodedValue::String(idx) => {
            // texto resolvido aqui; quem chama aloca no heap (ver
            // Engine::ensure_initialized) — por isso devolvemos Obj só se
            // o texto for vazio (caso degenerado); caso geral: None e o
            // caller trata String(idx) especificamente
            let _ = dex.string(*idx);
            return None;
        }
        _ => return None,
    })
}

/// Converte um literal de argumento (linha de comando / harness) para `Value`
/// conforme o tipo do parâmetro. Aceita: `42`, `-7`, `1L`, `1.5f`, `2.5`,
/// `'c'`, `"texto"`, `true`/`false`, `null`.
pub fn parse_arg_value(lit: &str, param_type: &str) -> Result<Value, String> {
    let lit = lit.trim();
    let bad = |msg: &str| format!("literal '{lit}' inválido para {param_type}: {msg}");
    match param_type {
        "I" | "B" | "S" | "C" | "Z" => {
            if lit == "true" || lit == "false" {
                return Ok(Value::Int((lit == "true") as i32));
            }
            if param_type == "C" && lit.len() >= 3 && lit.starts_with('\'') && lit.ends_with('\'') {
                let inner = &lit[1..lit.len() - 1];
                let c = match inner {
                    "\\n" => '\n',
                    "\\t" => '\t',
                    "\\\\" => '\\',
                    "\\'" => '\'',
                    s if s.chars().count() == 1 => s.chars().next().unwrap(),
                    _ => return Err(bad("char literal com mais de um caractere")),
                };
                return Ok(Value::Int(c as u32 as i32));
            }
            lit.parse::<i32>()
                .map(Value::Int)
                .map_err(|e| bad(&e.to_string()))
        }
        "J" => {
            let digits = lit.strip_suffix('L').unwrap_or(lit);
            digits
                .parse::<i64>()
                .map(Value::Long)
                .map_err(|e| bad(&e.to_string()))
        }
        "F" => {
            let digits = lit
                .strip_suffix('f')
                .or_else(|| lit.strip_suffix('F'))
                .unwrap_or(lit);
            match digits.to_ascii_lowercase().as_str() {
                "nan" => Ok(Value::Float(f32::NAN)),
                "infinity" | "+infinity" | "inf" => Ok(Value::Float(f32::INFINITY)),
                "-infinity" | "-inf" => Ok(Value::Float(f32::NEG_INFINITY)),
                _ => digits
                    .parse::<f32>()
                    .map(Value::Float)
                    .map_err(|e| bad(&e.to_string())),
            }
        }
        "D" => {
            let digits = lit
                .strip_suffix('d')
                .or_else(|| lit.strip_suffix('D'))
                .unwrap_or(lit);
            match digits.to_ascii_lowercase().as_str() {
                "nan" => Ok(Value::Double(f64::NAN)),
                "infinity" | "+infinity" | "inf" => Ok(Value::Double(f64::INFINITY)),
                "-infinity" | "-inf" => Ok(Value::Double(f64::NEG_INFINITY)),
                _ => digits
                    .parse::<f64>()
                    .map(Value::Double)
                    .map_err(|e| bad(&e.to_string())),
            }
        }
        "Ljava/lang/String;" => {
            if lit == "null" {
                return Ok(Value::Null);
            }
            let unquoted = if lit.starts_with('"') && lit.ends_with('"') && lit.len() >= 2 {
                &lit[1..lit.len() - 1]
            } else {
                lit
            };
            let unescaped = unescaped_java(unquoted);
            Ok(Value::StrPlaceholder(unescaped))
        }
        t if t.starts_with('L') || t.starts_with('[') => {
            if lit == "null" {
                Ok(Value::Null)
            } else {
                Err(format!("tipo {t} só aceita 'null' no M2 (recebi '{lit}')"))
            }
        }
        _ => Err(format!("tipo de parâmetro {param_type} não suportado")),
    }
}

/// Escapes mínimos de string Java (\n, \t, \\, \", \').
pub fn unescaped_java(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    use alloc::vec::Vec;

    #[test]
    fn stub_contract_gone_vm_has_real_api() {
        // o stub do M0 sumiu: a API pública do M2 deve existir
        let e = Engine::new(Vec::new(), VmConfig::default());
        assert_eq!(e.heap_used(), 0);
        assert_eq!(VmConfig::default().heap_budget, heap::DEFAULT_HEAP_BUDGET);
    }

    #[test]
    fn arg_parsing_matches_types() {
        assert_eq!(parse_arg_value("42", "I"), Ok(Value::Int(42)));
        assert_eq!(parse_arg_value("-7", "I"), Ok(Value::Int(-7)));
        assert_eq!(parse_arg_value("true", "Z"), Ok(Value::Int(1)));
        assert_eq!(parse_arg_value("1L", "J"), Ok(Value::Long(1)));
        assert_eq!(parse_arg_value("1.5f", "F"), Ok(Value::Float(1.5)));
        assert_eq!(parse_arg_value("2.5", "D"), Ok(Value::Double(2.5)));
        assert_eq!(parse_arg_value("'A'", "C"), Ok(Value::Int(0x41)));
        assert_eq!(
            parse_arg_value("\"oi\\n\"", "Ljava/lang/String;"),
            Ok(Value::StrPlaceholder("oi\n".into()))
        );
        assert_eq!(
            parse_arg_value("null", "Ljava/lang/String;"),
            Ok(Value::Null)
        );
        assert!(parse_arg_value("x", "I").is_err());
    }
}
