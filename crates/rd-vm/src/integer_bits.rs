//! Pure Integer/Long bit and scalar operations extracted from RustyDroid
//! numerics.rs. One implementation for both VM paths; no platform state.
use crate::{err, Throwable, Value, VmExit};
pub(crate) fn metadata(class: &str, name: &str, sig: &str) -> bool {
    matches!(
        (class, name, sig),
        (
            "Ljava/lang/Integer;",
            "bitCount"
                | "numberOfLeadingZeros"
                | "numberOfTrailingZeros"
                | "highestOneBit"
                | "lowestOneBit"
                | "reverse"
                | "reverseBytes"
                | "signum"
                | "hashCode",
            "(I)I"
        ) | (
            "Ljava/lang/Integer;",
            "rotateLeft"
                | "rotateRight"
                | "compare"
                | "compareUnsigned"
                | "divideUnsigned"
                | "remainderUnsigned"
                | "max"
                | "min"
                | "sum",
            "(II)I"
        ) | ("Ljava/lang/Integer;", "toUnsignedLong", "(I)J")
            | (
                "Ljava/lang/Long;",
                "bitCount"
                    | "numberOfLeadingZeros"
                    | "numberOfTrailingZeros"
                    | "signum"
                    | "hashCode",
                "(J)I"
            )
            | (
                "Ljava/lang/Long;",
                "highestOneBit" | "lowestOneBit" | "reverse" | "reverseBytes",
                "(J)J"
            )
            | ("Ljava/lang/Long;", "rotateLeft" | "rotateRight", "(JI)J")
            | ("Ljava/lang/Long;", "compare" | "compareUnsigned", "(JJ)I")
            | (
                "Ljava/lang/Long;",
                "max" | "min" | "sum" | "divideUnsigned" | "remainderUnsigned",
                "(JJ)J"
            )
    )
}
pub(crate) fn call(
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !metadata(class, name, sig) {
        return Ok(None);
    }
    let parameters = sig[1..].split(')').next().unwrap_or("").as_bytes();
    if args.len() != parameters.len() {
        return Err(err::vm_error("VM_TYPE_ERROR", "integer utility argument count").into());
    }
    for (parameter, value) in parameters.iter().zip(args) {
        if *parameter == b'I' {
            value.as_int()?;
        } else {
            value.as_long()?;
        }
    }
    // Arguments have been validated against the exact descriptor above.
    let i = |index: usize| args[index].as_int().unwrap_or(0);
    let j = |index: usize| args[index].as_long().unwrap_or(0);
    let cls = if class == "Ljava/lang/Integer;" {
        "Integer"
    } else {
        "Long"
    };
    let value = match (cls, name, sig) {
        ("Integer", "bitCount", "(I)I") => Value::Int(i(0).count_ones() as i32),
        ("Integer", "numberOfLeadingZeros", "(I)I") => Value::Int(i(0).leading_zeros() as i32),
        ("Integer", "numberOfTrailingZeros", "(I)I") => Value::Int(i(0).trailing_zeros() as i32),
        ("Integer", "highestOneBit", "(I)I") => {
            let x = i(0) as u32;
            Value::Int(if x == 0 {
                0
            } else {
                (1u32 << (31 - x.leading_zeros())) as i32
            })
        }
        ("Integer", "lowestOneBit", "(I)I") => Value::Int(i(0) & i(0).wrapping_neg()),
        ("Integer", "rotateLeft", "(II)I") => Value::Int(i(0).rotate_left((i(1) & 31) as u32)),
        ("Integer", "rotateRight", "(II)I") => Value::Int(i(0).rotate_right((i(1) & 31) as u32)),
        ("Integer", "reverse", "(I)I") => Value::Int(i(0).reverse_bits()),
        ("Integer", "reverseBytes", "(I)I") => Value::Int(i(0).swap_bytes()),
        ("Integer", "signum", "(I)I") => Value::Int(i(0).signum()),
        ("Integer", "hashCode", "(I)I") => Value::Int(i(0)),
        ("Integer", "compare", "(II)I") => Value::Int(i(0).cmp(&i(1)) as i32),
        ("Integer", "compareUnsigned", "(II)I") => {
            Value::Int((i(0) as u32).cmp(&(i(1) as u32)) as i32)
        }
        ("Integer", "divideUnsigned", "(II)I") | ("Integer", "remainderUnsigned", "(II)I") => {
            if i(1) == 0 {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ArithmeticException;",
                    "/ by zero",
                )));
            }
            let (a, c) = (i(0) as u32, i(1) as u32);
            Value::Int(if name == "divideUnsigned" {
                a / c
            } else {
                a % c
            } as i32)
        }
        ("Integer", "toUnsignedLong", "(I)J") => Value::Long(i(0) as u32 as i64),
        ("Integer", "max", "(II)I") => Value::Int(i(0).max(i(1))),
        ("Integer", "min", "(II)I") => Value::Int(i(0).min(i(1))),
        ("Integer", "sum", "(II)I") => Value::Int(i(0).wrapping_add(i(1))),
        ("Long", "bitCount", "(J)I") => Value::Int(j(0).count_ones() as i32),
        ("Long", "numberOfLeadingZeros", "(J)I") => Value::Int(j(0).leading_zeros() as i32),
        ("Long", "numberOfTrailingZeros", "(J)I") => Value::Int(j(0).trailing_zeros() as i32),
        ("Long", "highestOneBit", "(J)J") => {
            let x = j(0) as u64;
            Value::Long(if x == 0 {
                0
            } else {
                (1u64 << (63 - x.leading_zeros())) as i64
            })
        }
        ("Long", "lowestOneBit", "(J)J") => Value::Long(j(0) & j(0).wrapping_neg()),
        ("Long", "rotateLeft", "(JI)J") => Value::Long(j(0).rotate_left((i(1) & 63) as u32)),
        ("Long", "rotateRight", "(JI)J") => Value::Long(j(0).rotate_right((i(1) & 63) as u32)),
        ("Long", "reverse", "(J)J") => Value::Long(j(0).reverse_bits()),
        ("Long", "signum", "(J)I") => Value::Int(j(0).signum() as i32),
        ("Long", "hashCode", "(J)I") => Value::Int((j(0) ^ ((j(0) as u64) >> 32) as i64) as i32),
        ("Long", "compare", "(JJ)I") => Value::Int(j(0).cmp(&j(1)) as i32),
        ("Long", "compareUnsigned", "(JJ)I") => {
            Value::Int((j(0) as u64).cmp(&(j(1) as u64)) as i32)
        }
        ("Long", "max", "(JJ)J") => Value::Long(j(0).max(j(1))),
        ("Long", "min", "(JJ)J") => Value::Long(j(0).min(j(1))),
        ("Long", "sum", "(JJ)J") => Value::Long(j(0).wrapping_add(j(1))),
        ("Long", "reverseBytes", "(J)J") => Value::Long(j(0).swap_bytes()),
        ("Long", "divideUnsigned", "(JJ)J") | ("Long", "remainderUnsigned", "(JJ)J") => {
            let (a, b) = (j(0) as u64, j(1) as u64);
            if b == 0 {
                return Err(Throwable::new("Ljava/lang/ArithmeticException;", "/ by zero").into());
            }
            Value::Long(if name == "divideUnsigned" {
                a / b
            } else {
                a % b
            } as i64)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
