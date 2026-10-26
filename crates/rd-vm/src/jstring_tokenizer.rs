//! `java.util.StringTokenizer` host support.
//!
//! Tokenization uses Java's delimiter *set* semantics, including optional
//! delimiter tokens. State stores UTF-16 code units so token boundaries match
//! `StringTokenizer`'s `char`-based rules.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

const TOKENIZER: &str = "Ljava/util/StringTokenizer;";
const ENUMERATION: &str = "Ljava/util/Enumeration;";

fn exception(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}

fn string_units(vm: &Engine, value: &Value, argument: &str) -> Result<Vec<u16>, VmExit> {
    match value {
        Value::Obj(r) => Ok(vm.heap.as_str(*r)?.encode_utf16().collect()),
        Value::Null | Value::Int(0) => Err(exception(
            "Ljava/lang/NullPointerException;",
            format!("StringTokenizer {argument} == null"),
        )),
        _ => Err(exception(
            "Ljava/lang/ClassCastException;",
            format!("StringTokenizer {argument} is not a String"),
        )),
    }
}

fn delimiter(ch: u16, delimiters: &[u16]) -> bool {
    delimiters.contains(&ch)
}

/// Returns `(token_start, token_end, next_cursor)` for the next token.
fn token_at(
    source: &[u16],
    delimiters: &[u16],
    return_delimiters: bool,
    cursor: usize,
) -> Option<(usize, usize, usize)> {
    let mut at = cursor.min(source.len());
    if return_delimiters {
        if at == source.len() {
            return None;
        }
        if delimiter(source[at], delimiters) {
            return Some((at, at + 1, at + 1));
        }
        let start = at;
        while at < source.len() && !delimiter(source[at], delimiters) {
            at += 1;
        }
        return Some((start, at, at));
    }

    while at < source.len() && delimiter(source[at], delimiters) {
        at += 1;
    }
    if at == source.len() {
        return None;
    }
    let start = at;
    while at < source.len() && !delimiter(source[at], delimiters) {
        at += 1;
    }
    Some((start, at, at))
}

fn tokenizer(vm: &Engine, recv: ObjRef) -> Option<(Vec<u16>, Vec<u16>, bool, usize)> {
    match vm.fw.objects.get(&recv) {
        Some(HostObj::StringTokenizer {
            source,
            delimiters,
            return_delimiters,
            cursor,
        }) => Some((
            source.clone(),
            delimiters.clone(),
            *return_delimiters,
            *cursor,
        )),
        _ => None,
    }
}

fn next_token(vm: &mut Engine, recv: ObjRef) -> Result<Value, VmExit> {
    let Some((source, delimiters, return_delimiters, cursor)) = tokenizer(vm, recv) else {
        return Ok(Value::Null);
    };
    let Some((start, end, next_cursor)) = token_at(&source, &delimiters, return_delimiters, cursor)
    else {
        return Err(exception(
            "Ljava/util/NoSuchElementException;",
            "StringTokenizer",
        ));
    };
    if let Some(HostObj::StringTokenizer { cursor, .. }) = vm.fw.objects.get_mut(&recv) {
        *cursor = next_cursor;
    }
    let token = String::from_utf16_lossy(&source[start..end]);
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, token)?))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" && mref_class == TOKENIZER {
        let Some(text) = args.first() else {
            return Ok(None);
        };
        let source = string_units(vm, text, "input")?;
        let (delimiters, return_delimiters) = match sig {
            "(Ljava/lang/String;)V" => (" \t\n\r\u{000c}".encode_utf16().collect(), false),
            "(Ljava/lang/String;Ljava/lang/String;)V" => {
                let Some(value) = args.get(1) else {
                    return Ok(None);
                };
                (string_units(vm, value, "delimiters")?, false)
            }
            "(Ljava/lang/String;Ljava/lang/String;Z)V" => {
                let (Some(value), Some(return_value)) = (args.get(1), args.get(2)) else {
                    return Ok(None);
                };
                (
                    string_units(vm, value, "delimiters")?,
                    return_value.as_int()? != 0,
                )
            }
            _ => return Ok(None),
        };
        vm.fw.objects.insert(
            recv,
            HostObj::StringTokenizer {
                source,
                delimiters,
                return_delimiters,
                cursor: 0,
            },
        );
        return Ok(Some(Value::Null));
    }

    if !matches!(mref_class, TOKENIZER | ENUMERATION) && tokenizer(vm, recv).is_none() {
        return Ok(None);
    }
    if tokenizer(vm, recv).is_none() {
        return Ok(None);
    }

    match (name, sig) {
        ("hasMoreTokens", "()Z") | ("hasMoreElements", "()Z") => {
            let Some((source, delimiters, return_delimiters, cursor)) = tokenizer(vm, recv) else {
                return Ok(None);
            };
            Ok(Some(Value::Int(
                token_at(&source, &delimiters, return_delimiters, cursor).is_some() as i32,
            )))
        }
        ("countTokens", "()I") => {
            let Some((source, delimiters, return_delimiters, mut cursor)) = tokenizer(vm, recv)
            else {
                return Ok(None);
            };
            let mut count = 0i32;
            while let Some((_, _, next)) = token_at(&source, &delimiters, return_delimiters, cursor)
            {
                count += 1;
                cursor = next;
            }
            Ok(Some(Value::Int(count)))
        }
        ("nextToken", "()Ljava/lang/String;") => Ok(Some(next_token(vm, recv)?)),
        ("nextToken", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let Some(value) = args.first() else {
                return Ok(None);
            };
            let delimiters = string_units(vm, value, "delimiters")?;
            if let Some(HostObj::StringTokenizer {
                delimiters: current,
                ..
            }) = vm.fw.objects.get_mut(&recv)
            {
                *current = delimiters;
            }
            Ok(Some(next_token(vm, recv)?))
        }
        ("nextElement", "()Ljava/lang/Object;") => Ok(Some(next_token(vm, recv)?)),
        _ => Ok(None),
    }
}
