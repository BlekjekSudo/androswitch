//! Shared RustyDroid `java.util.StringTokenizer` implementation.
//!
//! The original delimiter-set/cursor algorithm uses the actual immutable Java
//! Strings. Typed ephemeron edges retain those inputs only while their tokenizer
//! is live; no reference framework or duplicated permanent text buffer is used.
use crate::{framework_host::HostMethodKind, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{string::String, vec, vec::Vec};

pub const CLASS: &str = "Ljava/util/StringTokenizer;";
const ENUMERATION: &str = "Ljava/util/Enumeration;";

#[derive(Clone, Copy)]
pub(crate) struct State {
    source: ObjRef,
    delimiters: Option<ObjRef>,
    return_delimiters: bool,
    // Byte offsets are internal: every boundary is a Unicode scalar boundary.
    // This preserves supplementary delimiters without splitting surrogate pairs.
    cursor: usize,
}

pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance};
    if class == CLASS && name == "<init>" {
        return matches!(
            sig,
            "(Ljava/lang/String;)V"
                | "(Ljava/lang/String;Ljava/lang/String;)V"
                | "(Ljava/lang/String;Ljava/lang/String;Z)V"
        )
        .then_some(Constructor);
    }
    let enumeration = matches!(
        (name, sig),
        ("hasMoreElements", "()Z") | ("nextElement", "()Ljava/lang/Object;")
    );
    let tokenizer = enumeration
        || matches!(
            (name, sig),
            ("hasMoreTokens", "()Z")
                | ("countTokens", "()I")
                | (
                    "nextToken",
                    "()Ljava/lang/String;" | "(Ljava/lang/String;)Ljava/lang/String;"
                )
        );
    ((class == CLASS && tokenizer) || (class == ENUMERATION && enumeration)).then_some(Instance)
}

pub(crate) fn gc_keys(vm: &Engine, out: &mut Vec<ObjRef>) {
    out.extend(vm.tokenizers.keys().copied());
}
pub(crate) fn references(vm: &Engine, owner: ObjRef) -> Vec<ObjRef> {
    let Some(state) = vm.tokenizers.get(&owner) else {
        return Vec::new();
    };
    let mut refs = vec![state.source];
    refs.extend(state.delimiters);
    refs
}
pub(crate) fn gc_purge(vm: &mut Engine, alive: &dyn Fn(ObjRef) -> bool) {
    vm.tokenizers.retain(|owner, _| alive(*owner));
}

fn exception(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}
fn string_ref(vm: &Engine, value: &Value) -> Result<Option<ObjRef>, VmExit> {
    match value {
        Value::Obj(r) => {
            vm.heap.as_str(*r)?;
            Ok(Some(*r))
        }
        Value::Null | Value::Int(0) => Ok(None),
        _ => Err(exception(
            "Ljava/lang/ClassCastException;",
            "StringTokenizer requires a String",
        )),
    }
}
fn null_input() -> VmExit {
    exception(
        "Ljava/lang/NullPointerException;",
        "StringTokenizer input == null",
    )
}
fn delimiters(vm: &Engine, state: State) -> Result<&str, VmExit> {
    let reference = state.delimiters.ok_or_else(|| {
        exception(
            "Ljava/lang/NullPointerException;",
            "StringTokenizer delimiters == null",
        )
    })?;
    Ok(vm.heap.as_str(reference)?)
}

/// Original maximal-token/delimiter-set algorithm, with code-point boundaries.
/// Returns `(token_start, token_end, next_cursor)` without changing owner state.
fn token_at(
    source: &str,
    delimiters: &str,
    return_delimiters: bool,
    cursor: usize,
) -> Option<(usize, usize, usize)> {
    let mut at = cursor.min(source.len());
    if !return_delimiters {
        for ch in source[at..].chars() {
            if !delimiters.contains(ch) {
                break;
            }
            at += ch.len_utf8();
        }
    }
    if at == source.len() {
        return None;
    }
    let start = at;
    for ch in source[start..].chars() {
        if delimiters.contains(ch) {
            if return_delimiters && at == start {
                at += ch.len_utf8();
            }
            break;
        }
        at += ch.len_utf8();
    }
    Some((start, at, at))
}

fn state(vm: &Engine, receiver: ObjRef) -> Result<State, VmExit> {
    vm.tokenizers.get(&receiver).copied().ok_or_else(|| {
        crate::err::vm_error(
            "INVALID_STATE",
            "StringTokenizer constructor has not initialized this receiver",
        )
        .into()
    })
}
fn next_token(vm: &mut Engine, receiver: ObjRef) -> Result<Value, VmExit> {
    let state = state(vm, receiver)?;
    let source = vm.heap.as_str(state.source)?;
    let length = source.len();
    let range = token_at(
        source,
        delimiters(vm, state)?,
        state.return_delimiters,
        state.cursor,
    );
    let Some((start, end, next_cursor)) = range else {
        vm.tokenizers
            .get_mut(&receiver)
            .expect("validated tokenizer")
            .cursor = length;
        return Err(exception(
            "Ljava/util/NoSuchElementException;",
            "StringTokenizer",
        ));
    };
    // substring(0, length) uses the same immutable source String.
    let whole = start == 0 && end == length;
    let token = (!whole).then(|| String::from(&source[start..end]));
    vm.tokenizers
        .get_mut(&receiver)
        .expect("validated tokenizer")
        .cursor = next_cursor;
    Ok(Value::Obj(match token {
        Some(text) => crate::intrinsics::alloc_string(vm, text)?,
        None => state.source,
    }))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    actual: &str,
    declaring: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if method_metadata(declaring, name, sig) != Some(HostMethodKind::Constructor) {
            return Ok(None);
        }
    } else {
        if !vm.cp.is_subtype(actual, CLASS) {
            return Ok(None);
        }
        // DEX overrides execute first. invoke-super supplies its platform lookup
        // start as `actual`, so an explicit base call still executes this body.
        if vm.cp.resolve_method(actual, name, sig).is_some() {
            return Ok(None);
        }
        if method_metadata(CLASS, name, sig).is_none() {
            return Ok(None);
        }
    }
    let mut roots = vec![Value::Obj(receiver)];
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| {
        if name == "<init>" {
            let source =
                string_ref(vm, args.first().ok_or_else(null_input)?)?.ok_or_else(null_input)?;
            let delim = if sig == "(Ljava/lang/String;)V" {
                Some(crate::intrinsics::alloc_string(
                    vm,
                    " \t\n\r\u{000c}".into(),
                )?)
            } else {
                string_ref(vm, args.get(1).ok_or_else(null_input)?)?
            };
            let return_delimiters = sig == "(Ljava/lang/String;Ljava/lang/String;Z)V"
                && args.get(2).ok_or_else(null_input)?.as_int()? != 0;
            vm.tokenizers.insert(
                receiver,
                State {
                    source,
                    delimiters: delim,
                    return_delimiters,
                    cursor: 0,
                },
            );
            return Ok(Some(Value::Null));
        }
        match (name, sig) {
            ("hasMoreTokens", "()Z") => {
                let s = state(vm, receiver)?;
                Ok(Some(Value::Int(
                    token_at(
                        vm.heap.as_str(s.source)?,
                        delimiters(vm, s)?,
                        s.return_delimiters,
                        s.cursor,
                    )
                    .is_some() as i32,
                )))
            }
            ("countTokens", "()I") => {
                let s = state(vm, receiver)?;
                let source = vm.heap.as_str(s.source)?;
                let mut cursor = s.cursor;
                let mut count = 0;
                // An exhausted tokenizer counts zero even with null delimiters.
                while cursor < source.len() {
                    let Some((_, _, next)) =
                        token_at(source, delimiters(vm, s)?, s.return_delimiters, cursor)
                    else {
                        break;
                    };
                    count += 1;
                    cursor = next;
                }
                Ok(Some(Value::Int(count)))
            }
            ("nextToken", "()Ljava/lang/String;") => Ok(Some(next_token(vm, receiver)?)),
            ("nextToken", "(Ljava/lang/String;)Ljava/lang/String;") => {
                state(vm, receiver)?;
                let delim = string_ref(vm, args.first().ok_or_else(null_input)?)?;
                vm.tokenizers
                    .get_mut(&receiver)
                    .expect("validated tokenizer")
                    .delimiters = delim;
                Ok(Some(vm.invoke_instance(
                    receiver,
                    "nextToken",
                    "()Ljava/lang/String;",
                    &[],
                )?))
            }
            ("hasMoreElements", "()Z") => Ok(Some(vm.invoke_instance(
                receiver,
                "hasMoreTokens",
                "()Z",
                &[],
            )?)),
            ("nextElement", "()Ljava/lang/Object;") => Ok(Some(vm.invoke_instance(
                receiver,
                "nextToken",
                "()Ljava/lang/String;",
                &[],
            )?)),
            _ => Ok(None),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VmConfig;

    fn tokenizer(vm: &mut Engine, source: &str, delim: &str) -> ObjRef {
        let owner = vm.heap.alloc_instance(CLASS.into(), vec![]).unwrap();
        let source = vm.heap.alloc_string(source.into()).unwrap();
        let delim = vm.heap.alloc_string(delim.into()).unwrap();
        instance_intrinsic(
            vm,
            CLASS,
            CLASS,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            owner,
            &[Value::Obj(source), Value::Obj(delim)],
        )
        .unwrap();
        owner
    }

    #[test]
    fn live_owner_traces_inputs_and_dead_owner_releases_table_before_slot_reuse() {
        let mut vm = Engine::new(vec![], VmConfig::default());
        let owner = tokenizer(&mut vm, "retained input", " ");
        let state = state(&vm, owner).unwrap();
        vm.statics
            .insert(("LRoot;".into(), "tokenizer".into()), Value::Obj(owner));
        vm.gc();
        assert_eq!(vm.heap.as_str(state.source).unwrap(), "retained input");
        assert_eq!(vm.heap.as_str(state.delimiters.unwrap()).unwrap(), " ");
        vm.statics.clear();
        vm.gc();
        assert!(vm.tokenizers.is_empty());
        assert_eq!(
            vm.heap.class_of(state.source).unwrap(),
            crate::gc::FREED_CLASS
        );
        assert_eq!(
            vm.heap.class_of(state.delimiters.unwrap()).unwrap(),
            crate::gc::FREED_CLASS
        );
        let reused = vm.heap.alloc_instance(CLASS.into(), vec![]).unwrap();
        assert!(!vm.tokenizers.contains_key(&reused));
        assert!(next_token(&mut vm, reused)
            .unwrap_err()
            .to_string()
            .contains("INVALID_STATE"));
    }

    #[test]
    fn old_tokenizer_retains_young_delimiter_and_numbers_are_never_gc_edges() {
        let mut vm = Engine::new(vec![], VmConfig::default());
        let owner = tokenizer(&mut vm, "a,b;c", ",");
        vm.statics
            .insert(("LRoot;".into(), "tokenizer".into()), Value::Obj(owner));
        vm.gc();
        let delim = vm.heap.alloc_string(";".into()).unwrap();
        let garbage = vm
            .heap
            .alloc_string("not a tokenizer reference".into())
            .unwrap();
        let state = vm.tokenizers.get_mut(&owner).unwrap();
        state.delimiters = Some(delim);
        state.cursor = garbage as usize;
        assert_eq!(vm.gc_minor().last_kind, "minor");
        assert_eq!(vm.heap.as_str(delim).unwrap(), ";");
        assert_eq!(vm.heap.class_of(garbage).unwrap(), crate::gc::FREED_CLASS);
    }

    #[test]
    fn tokenizer_state_is_engine_local() {
        let mut a = Engine::new(vec![], VmConfig::default());
        let owner = tokenizer(&mut a, &"input".repeat(4096), ",");
        let source = state(&a, owner).unwrap().source;
        assert_eq!(references(&a, owner)[0], source);
        let mut b = Engine::new(vec![], VmConfig::default());
        assert!(state(&b, owner).is_err());
        b.gc();
        assert!(state(&a, owner).is_ok());
    }
}
