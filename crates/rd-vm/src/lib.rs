//! rd-vm — VM Dalvik (interpretador → JIT Cranelift) + GC arena + threads.
//!
//! M2: interpretador mínimo — opcodes núcleo, invocações, exceções, strings,
//! fields; métodos puros de APK real retornam valores corretos (golden
//! vectors vs execução real). Heap arena com o orçamento do piso moto-e5
//! (heap ≤ 256 MB — PRF-06/A2); OOM é erro tipado, nunca abort (Lei 1).

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
};

pub mod framework_host;
#[cfg(not(feature = "std"))]
pub mod native_host;
#[cfg(not(feature = "std"))]
mod portable;
/// Internal contracts exposed only to the isolated portable host test target.
#[doc(hidden)]
#[cfg(all(not(feature = "std"), feature = "portable-test-support"))]
pub mod portable_test_support {
    pub use crate::portable::execution::contracts::*;
}
#[cfg(feature = "std")]
pub(crate) mod maps {
    pub use std::collections::{hash_map::Entry, HashMap, HashSet};
}
#[cfg(not(feature = "std"))]
pub(crate) mod maps {
    pub use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet};
}

#[cfg(feature = "advanced-components")]
#[cfg(feature = "std")]
pub mod accessibility_node;
#[cfg(feature = "std")]
pub(crate) mod activity_callbacks;
#[cfg(feature = "std")]
pub(crate) mod activity_extras;
#[cfg(feature = "std")]
pub mod android_build;
#[cfg(feature = "std")]
pub(crate) mod android_colorspace;
#[cfg(feature = "std")]
pub(crate) mod android_display;
#[cfg(feature = "std")]
pub(crate) mod android_drawable;
#[cfg(feature = "std")]
pub(crate) mod android_env;
pub mod android_log;
#[cfg(feature = "std")]
pub(crate) mod android_motion;
#[cfg(feature = "std")]
pub(crate) mod android_observable;
#[cfg(feature = "std")]
pub(crate) mod android_pm;
#[cfg(feature = "std")]
pub(crate) mod android_region;
#[cfg(feature = "std")]
pub(crate) mod android_res;
#[cfg(feature = "std")]
pub(crate) mod android_spannable;
#[cfg(feature = "std")]
pub(crate) mod android_text;
#[cfg(feature = "std")]
pub(crate) mod android_uri;
#[cfg(feature = "std")]
pub(crate) mod android_util;
#[cfg(feature = "std")]
pub(crate) mod arrays;
#[cfg(feature = "std")]
pub(crate) mod atomics;
pub mod boxes;
#[cfg(feature = "std")]
pub(crate) mod bundle;
#[cfg(feature = "std")]
pub(crate) mod choreographer;
pub mod classpath;
pub mod collection_values;
#[cfg(feature = "std")]
pub(crate) mod collections;
#[cfg(feature = "std")]
pub(crate) mod content;
pub mod engine;
pub mod err;
#[cfg(feature = "std")]
pub mod framework;
#[cfg(feature = "std")]
pub mod gc;
#[cfg(not(feature = "std"))]
#[path = "portable/gc.rs"]
pub mod gc;
#[cfg(feature = "std")]
pub mod graphics;
#[cfg(feature = "std")]
pub(crate) mod handler_msg;
pub mod heap;
#[cfg(feature = "std")]
pub mod inflate;
pub mod interp;
#[cfg(feature = "std")]
pub mod intrinsics;
#[cfg(not(feature = "std"))]
#[path = "portable/intrinsics.rs"]
pub mod intrinsics;
#[cfg(feature = "std")]
pub(crate) mod jannotations;
#[cfg(feature = "std")]
pub(crate) mod jbigdecimal;
#[cfg(feature = "std")]
pub(crate) mod jbreakiter;
#[cfg(feature = "std")]
pub(crate) mod jcollections;
#[cfg(feature = "std")]
pub(crate) mod jfile;
#[cfg(feature = "std")]
pub(crate) mod jio_chars;
#[cfg(feature = "jit")]
#[cfg(feature = "std")]
pub mod jit;
#[cfg(feature = "std")]
pub(crate) mod jiterator;
#[cfg(feature = "std")]
pub(crate) mod jlclass;
#[cfg(feature = "std")]
pub(crate) mod jlocks;
#[cfg(feature = "std")]
pub(crate) mod jlstring;
#[cfg(feature = "std")]
pub(crate) mod jlsystem;
#[cfg(feature = "std")]
pub(crate) mod jnormalizer;
#[cfg(feature = "std")]
pub(crate) mod jpriorityqueue;
#[cfg(feature = "std")]
pub(crate) mod jrandom;
#[cfg(feature = "std")]
pub(crate) mod jregex;
#[cfg(feature = "std")]
pub(crate) mod jresources;
#[cfg(feature = "std")]
pub(crate) mod jsb;
#[cfg(feature = "std")]
pub(crate) mod jsecurity;
#[cfg(feature = "std")]
pub(crate) mod jsorted;
#[cfg(feature = "std")]
pub(crate) mod jsqlite;
#[cfg(feature = "std")]
pub(crate) mod jstring_tokenizer;
#[cfg(feature = "std")]
pub(crate) mod jtext_format;
#[cfg(feature = "std")]
pub(crate) mod jtime;
#[cfg(feature = "std")]
pub(crate) mod juuid;
#[cfg(feature = "std")]
pub(crate) mod jxml;
#[cfg(feature = "std")]
pub mod m8;
#[cfg(feature = "advanced-components")]
#[cfg(feature = "std")]
pub mod m9;
#[cfg(feature = "native")]
#[cfg(feature = "std")]
pub mod native;
#[cfg(feature = "std")]
pub(crate) mod numerics;
#[cfg(feature = "std")]
pub mod perms;
#[cfg(feature = "std")]
pub(crate) mod platform_base;
mod primitive_parse;
#[cfg(feature = "std")]
pub(crate) mod reflect;
pub mod repr;
#[cfg(feature = "std")]
pub mod threads;
#[cfg(feature = "std")]
pub(crate) mod throwable;
pub mod value;
#[cfg(feature = "std")]
pub(crate) mod view_attach;
#[cfg(feature = "std")]
pub(crate) mod view_layout;
#[cfg(feature = "std")]
pub(crate) mod view_props;
#[cfg(feature = "std")]
pub(crate) mod vto;
#[cfg(feature = "std")]
pub(crate) mod window;
#[cfg(feature = "std")]
pub(crate) mod xml_res;

use rd_dex::annotations::EncodedValue;
use rd_dex::Dex;

pub use engine::{ClassCoverage, CoverageSnapshot, Engine, VmConfig};
pub use err::{Throwable, VmExit};
pub use heap::ObjRef;
#[cfg(feature = "std")]
pub use intrinsics::{route_app_stdout_to_stderr, set_app_stdout, HostOut};
pub use value::Value;

/// Módulo do contrato de erro.
pub const MODULE_ID: &str = "rd-vm";
/// Explicit evidence of the linked VM variant (Cargo features are additive).
pub const PORTABLE_CORE: bool = !cfg!(feature = "std");

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
