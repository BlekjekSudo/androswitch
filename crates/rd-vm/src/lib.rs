//! rd-vm — VM Dalvik (interpretador → JIT Cranelift) + GC arena + threads.
//!
//! M2: interpretador mínimo — opcodes núcleo, invocações, exceções, strings,
//! fields; métodos puros de APK real retornam valores corretos (golden
//! vectors vs execução real). Heap arena com o orçamento do piso moto-e5
//! (heap ≤ 256 MB — PRF-06/A2); OOM é erro tipado, nunca abort (Lei 1).

#[cfg(feature = "advanced-components")]
pub mod accessibility_node;
pub(crate) mod activity_callbacks;
pub(crate) mod activity_extras;
pub mod android_build;
pub(crate) mod android_colorspace;
pub(crate) mod android_display;
pub(crate) mod android_drawable;
pub(crate) mod android_env;
pub(crate) mod android_json;
pub(crate) mod android_motion;
pub(crate) mod android_observable;
pub(crate) mod android_pm;
pub(crate) mod android_region;
pub(crate) mod android_res;
pub(crate) mod android_spannable;
pub(crate) mod android_stub_surfaces;
pub(crate) mod android_sysservices;
pub(crate) mod android_text;
pub(crate) mod android_uri;
pub(crate) mod android_util;
pub(crate) mod android_velocity;
/// Apêndice D — shims host AndroidX/Jetpack (data/jetpack.toml).
pub mod androidx;
pub(crate) mod arrays;
pub(crate) mod atomics;
pub(crate) mod boxes;
pub(crate) mod bundle;
pub(crate) mod choreographer;
pub mod classpath;
pub(crate) mod collections;
pub(crate) mod content;
pub mod engine;
pub mod err;
pub mod framework;
pub mod gc;
pub mod gms;
pub mod graphics;
pub(crate) mod handler_msg;
pub mod heap;
pub mod inflate;
pub mod interp;
pub mod intrinsics;
pub(crate) mod jannotations;
pub(crate) mod jbigdecimal;
pub(crate) mod jbitset;
pub(crate) mod jbreakiter;
pub(crate) mod jcollections;
pub(crate) mod jfile;
pub(crate) mod jio_chars;
#[cfg(feature = "jit")]
pub mod jit;
pub(crate) mod jiterator;
pub(crate) mod jlclass;
pub(crate) mod jlocks;
pub(crate) mod jlstring;
pub(crate) mod jlsystem;
pub(crate) mod jnormalizer;
pub(crate) mod jpriorityqueue;
pub(crate) mod jrandom;
pub(crate) mod jregex;
pub(crate) mod jresources;
pub(crate) mod jsb;
pub(crate) mod jsecurity;
pub(crate) mod jsorted;
pub(crate) mod jsqlite;
pub(crate) mod jstring_tokenizer;
pub(crate) mod jtext_format;
pub(crate) mod jtime;
pub(crate) mod juuid;
pub(crate) mod jxml;
#[cfg(feature = "virtual-devices")]
pub mod m10;
pub mod m8;
#[cfg(feature = "advanced-components")]
pub mod m9;
#[cfg(feature = "native")]
pub mod native;
pub(crate) mod numerics;
pub(crate) mod parcel;
pub mod perms;
pub(crate) mod platform_base;
pub(crate) mod prefs;
pub(crate) mod reflect;
pub mod repr;
pub mod threads;
pub(crate) mod throwable;
pub mod value;
pub(crate) mod view_attach;
pub(crate) mod view_layout;
pub(crate) mod view_props;
pub(crate) mod vto;
pub(crate) mod window;
pub(crate) mod xml_res;

use rd_dex::annotations::EncodedValue;
use rd_dex::Dex;

pub use engine::{ClassCoverage, CoverageSnapshot, Engine, VmConfig};
pub use err::{Throwable, VmExit};
pub use heap::ObjRef;
pub use intrinsics::{route_app_stdout_to_stderr, set_app_stdout, HostOut};
pub use value::Value;

/// Módulo do contrato de erro.
pub const MODULE_ID: &str = "rd-vm";

/// Issue #230: fronteira de `Engine` para o estado host que vive em
/// estado thread-local (macro do std) fora do `Engine`. Esvazia/reinicia as tabelas
/// indexadas por `ObjRef` (spannable, random, regex, uri, xml,
/// annotations, io-chars) e o RNG de UUID. Sem isto, um `Engine` novo
/// na mesma thread (relaunch, próximo cenário de `rd test`) herdava
/// estado fantasma de um heap morto, porque os `ObjRef`s reiniciam com
/// o `Heap::new`. O VFS de arquivos NÃO entra aqui: dados persistidos
/// sobrevivem a relaunch dentro da sessão (comportamento Android);
/// quem o limpa é `clear_package_data` (clear/remove) e `reset_vfs`
/// (sessão nova).
///
/// Contrato: chamar ao descartar um `Engine` (o MCP faz isso ao matar o
/// engine ativo) ou antes de construir outro na mesma thread.
///
/// Premissa explícita: um `Engine` vivo por vez por thread — o padrão
/// de todas as superfícies (MCP, CLI, suítes). A migração dessas
/// tabelas para dentro de `HostState` segue como follow-up da #230.
pub fn reset_host_thread_state() {
    android_spannable::reset_all();
    jrandom::reset_all();
    jregex::reset_all();
    android_uri::reset_all();
    xml_res::reset_all();
    jannotations::reset_all();
    jio_chars::reset_all();
    juuid::reset_all();
}

/// Issue #230: apaga o armazenamento privado de um pacote no VFS
/// (`/data/user/0/<pkg>/`) — backend de `package_clear`/`package_remove`.
pub use jfile::clear_package_data;
/// Issue #230: zera o VFS inteiro — fronteira de sessão nova.
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
