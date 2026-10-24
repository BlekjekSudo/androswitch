//! General String.getBytes contracts. Encode directly into the final managed
//! array's backing Vec; no duplicate source string or encoded scratch buffer.
use super::charset_catalog::{IMPLEMENTED_ALIASES, KNOWN_KEYS};
use crate::{err, heap::ElemKind, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{format, vec::Vec};

// The API 26 ICU alias lookup rejects names at or above this byte length.
const MAX_NAME: usize = 60;

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    Utf8,
    Utf16,
    Utf16Be,
    Utf16Le,
    Latin1,
    Ascii,
}

fn unsupported() -> VmExit {
    // Do not duplicate an arbitrarily large invalid guest string in diagnostics.
    Throwable::new(
        "Ljava/io/UnsupportedEncodingException;",
        "unknown or invalid charset name",
    )
    .into()
}

/// Source-backed ICU recognition only: ignored punctuation resets the numeric
/// run, and only leading zeroes immediately followed by digits are discarded.
/// A fixed stack buffer bounds name processing before any host allocation.
fn normalized<'a>(name: &str, output: &'a mut [u8; MAX_NAME]) -> &'a str {
    let bytes = name.as_bytes();
    let mut len = 0;
    let mut after_digit = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let mapped = if byte.is_ascii_alphabetic() {
            after_digit = false;
            byte.to_ascii_lowercase()
        } else if byte == b'0' {
            if !after_digit && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                continue;
            }
            byte
        } else if byte.is_ascii_digit() {
            after_digit = true;
            byte
        } else {
            after_digit = false;
            continue;
        };
        output[len] = mapped;
        len += 1;
    }
    // Every emitted byte is ASCII and callers enforce input < MAX_NAME.
    core::str::from_utf8(&output[..len]).unwrap_or("")
}

fn known(name: &str) -> bool {
    let mut key = [0; MAX_NAME];
    KNOWN_KEYS
        .binary_search(&normalized(name, &mut key))
        .is_ok()
}

fn encoding(vm: &Engine, value: &Value) -> Result<Encoding, VmExit> {
    let reference = value
        .as_ref()?
        .ok_or_else(|| Throwable::new("Ljava/lang/NullPointerException;", "charsetName is null"))?;
    let name = vm.heap.as_str(reference)?;
    if name.is_empty() || name.len() >= MAX_NAME {
        return Err(unsupported());
    }
    let mut lower = [0; MAX_NAME];
    for (out, byte) in lower.iter_mut().zip(name.bytes()) {
        *out = byte.to_ascii_lowercase();
    }
    let lower = core::str::from_utf8(&lower[..name.len()]).map_err(|_| unsupported())?;
    if let Ok(index) = IMPLEMENTED_ALIASES.binary_search_by_key(&lower, |entry| entry.0) {
        return Ok(IMPLEMENTED_ALIASES[index].1);
    }
    // NativeConverter also tries a literal lower-case "x-" fallback. Recognize
    // that and known converter options as missing coverage, without substituting
    // a different BOM/version encoder or making unknown names successful.
    let recognized = known(name)
        || name.strip_prefix("x-").is_some_and(known)
        || name.split_once(',').is_some_and(|(base, _)| {
            !base.is_empty() && (known(base) || base.strip_prefix("x-").is_some_and(known))
        });
    if recognized {
        return Err(err::vm_error(
            "API_UNAVAILABLE",
            format!("String.getBytes charset {name:?}: encoder/alias form is not implemented"),
        )
        .into());
    }
    Err(unsupported())
}

fn oom(vm: &Engine, requested: usize) -> VmExit {
    err::oom_throwable(crate::heap::OomError {
        requested,
        budget: vm.heap.budget(),
    })
}

fn encoded_len(source: &str, encoding: Encoding) -> Option<usize> {
    match encoding {
        Encoding::Utf8 => Some(source.len()),
        Encoding::Ascii | Encoding::Latin1 => Some(source.encode_utf16().count()),
        Encoding::Utf16 | Encoding::Utf16Be | Encoding::Utf16Le => {
            source.encode_utf16().count().checked_mul(2)?.checked_add(
                usize::from(matches!(encoding, Encoding::Utf16) && !source.is_empty()) * 2,
            )
        }
    }
}

fn encode(vm: &mut Engine, recv: ObjRef, encoding: Encoding) -> Result<Value, VmExit> {
    let source = vm.heap.as_str(recv)?;
    let len = encoded_len(source, encoding).ok_or_else(|| oom(vm, usize::MAX))?;
    // Match Heap::alloc_array's real Value storage charge, not byte[]'s logical
    // byte count. Check before host reserve and use a fallible final allocation.
    let required = len
        .checked_mul(core::mem::size_of::<Value>())
        .and_then(|bytes| bytes.checked_add(16))
        .ok_or_else(|| oom(vm, usize::MAX))?;
    if len > i32::MAX as usize || required > vm.heap.budget().saturating_sub(vm.heap.used()) {
        return Err(oom(vm, required));
    }
    let mut values = Vec::new();
    values.try_reserve_exact(len).map_err(|_| {
        VmExit::from(Throwable::new(
            "Ljava/lang/OutOfMemoryError;",
            format!("String.getBytes backing allocation of {required} bytes failed"),
        ))
    })?;
    let mut byte = |value: u8| values.push(Value::Int(value as i8 as i32));
    match encoding {
        Encoding::Utf8 => {
            for value in source.bytes() {
                byte(value);
            }
        }
        Encoding::Ascii | Encoding::Latin1 => {
            let ceiling = if matches!(encoding, Encoding::Ascii) {
                0x7f
            } else {
                0xff
            };
            // Android 26 ART fast paths replace each UTF-16 unit. This is
            // intentionally two '?' bytes for an unmappable surrogate pair.
            for unit in source.encode_utf16() {
                byte(if unit <= ceiling { unit as u8 } else { b'?' });
            }
        }
        Encoding::Utf16 | Encoding::Utf16Be | Encoding::Utf16Le => {
            if matches!(encoding, Encoding::Utf16) && !source.is_empty() {
                byte(0xfe);
                byte(0xff);
            }
            for unit in source.encode_utf16() {
                let pair = if matches!(encoding, Encoding::Utf16Le) {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                byte(pair[0]);
                byte(pair[1]);
            }
        }
    }
    let reference = vm
        .heap
        .alloc_array(ElemKind::Int, len, "[B")
        .map_err(err::oom_throwable)?;
    *vm.heap.array_elems_mut(reference)? = values;
    Ok(Value::Obj(reference))
}

pub(crate) fn get_bytes(
    vm: &mut Engine,
    recv: ObjRef,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let encoder = match sig {
        "()[B" => Encoding::Utf8,
        "(Ljava/lang/String;)[B" => {
            if args.len() != 1 {
                return Err(
                    err::vm_error("INVALID_ARG", "String.getBytes expects charsetName").into(),
                );
            }
            encoding(vm, &args[0])?
        }
        _ => return Ok(None),
    };
    encode(vm, recv, encoder).map(Some)
}
