//! `java.util.UUID`.
//!
//! - `randomUUID`: version-4 UUIDs from a per-thread xorshift generator.
//!   It is seeded from `RD_UUID_SEED` (default fixed), so runs are
//!   reproducible.
//! - `fromString`, `nameUUIDFromBytes` (version 3, MD5) and `new UUID(msb, lsb)`.
//! - `toString`, `getMostSignificantBits`/`getLeastSignificantBits`,
//!   `version`, `variant`, `equals`, `hashCode` (JDK formula), `compareTo`
//!   (signed, as in the JDK).

use std::cell::Cell;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const UUID: &str = "Ljava/util/UUID;";

fn initial_seed() -> u64 {
    std::env::var("RD_UUID_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

thread_local! {
    static RNG: Cell<u64> = Cell::new(initial_seed());
}

/// Issue #230: fronteira de `Engine` — a sequência de
/// `UUID.randomUUID()` volta à semente (`RD_UUID_SEED`), em vez de
/// depender de quantos UUIDs engines anteriores geraram na thread
/// (determinismo, PRF-04).
pub(crate) fn reset_all() {
    RNG.with(|r| r.set(initial_seed()));
}

fn next_u64() -> u64 {
    RNG.with(|r| {
        let mut x = r.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        r.set(x);
        x
    })
}

fn bits(vm: &Engine, r: ObjRef) -> (i64, i64) {
    let g = |f: &str| match vm.heap.get_field(r, f) {
        Ok(Value::Long(l)) => l,
        _ => 0,
    };
    (g("rd_msb"), g("rd_lsb"))
}

fn new_uuid(vm: &mut Engine, msb: i64, lsb: i64) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(
            UUID.to_string(),
            vec![
                ("rd_msb".into(), Value::Long(msb)),
                ("rd_lsb".into(), Value::Long(lsb)),
            ],
        )
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

pub(crate) fn format_uuid(msb: i64, lsb: i64) -> String {
    let (m, l) = (msb as u64, lsb as u64);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        m >> 32,
        (m >> 16) & 0xffff,
        m & 0xffff,
        l >> 48,
        l & 0xffff_ffff_ffff
    )
}

pub(crate) fn parse_uuid(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    let p: Vec<u64> = parts
        .iter()
        .map(|x| u64::from_str_radix(x, 16).ok())
        .collect::<Option<_>>()?;
    let msb = (p[0] << 32) | ((p[1] & 0xffff) << 16) | (p[2] & 0xffff);
    let lsb = ((p[3] & 0xffff) << 48) | (p[4] & 0xffff_ffff_ffff);
    Some((msb as i64, lsb as i64))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != UUID {
        return Ok(None);
    }
    Ok(Some(match name {
        "randomUUID" => {
            let msb = (next_u64() & !0xf000) | 0x4000;
            let lsb = (next_u64() & !(0xc000_0000_0000_0000)) | 0x8000_0000_0000_0000;
            new_uuid(vm, msb as i64, lsb as i64)?
        }
        "fromString" => {
            let s = match args.first() {
                Some(Value::Obj(r)) => vm.heap.as_str(*r).map(str::to_string).unwrap_or_default(),
                _ => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "name",
                    )))
                }
            };
            let (m, l) = parse_uuid(&s).ok_or_else(|| {
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("Invalid UUID string: {s}"),
                ))
            })?;
            new_uuid(vm, m, l)?
        }
        "nameUUIDFromBytes" => {
            let bytes: Vec<u8> = match args.first() {
                Some(Value::Obj(a)) => vm
                    .heap
                    .as_array_elems(*a)
                    .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u8).collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let mut h = md5(&bytes);
            h[6] = (h[6] & 0x0f) | 0x30;
            h[8] = (h[8] & 0x3f) | 0x80;
            let msb = u64::from_be_bytes(h[..8].try_into().unwrap_or([0; 8]));
            let lsb = u64::from_be_bytes(h[8..].try_into().unwrap_or([0; 8]));
            new_uuid(vm, msb as i64, lsb as i64)?
        }
        _ => return Ok(None),
    }))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class != UUID {
        return Ok(None);
    }
    if name == "<init>" {
        let l = |i: usize| match args.get(i) {
            Some(Value::Long(v)) => *v,
            _ => 0,
        };
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_msb", Value::Long(l(0))))?;
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_lsb", Value::Long(l(1))))?;
        return Ok(Some(Value::Null));
    }
    let (m, l) = bits(vm, recv);
    let other = |vm: &Engine| match args.first() {
        Some(Value::Obj(o)) if vm.heap.class_of(*o).map(|c| c == UUID).unwrap_or(false) => {
            Some(bits(vm, *o))
        }
        _ => None,
    };
    Ok(Some(match name {
        "toString" => Value::Obj(crate::intrinsics::alloc_string(vm, format_uuid(m, l))?),
        "getMostSignificantBits" => Value::Long(m),
        "getLeastSignificantBits" => Value::Long(l),
        "version" => Value::Int(((m >> 12) & 0xf) as i32),
        "variant" => Value::Int((((l as u64) >> 62) & 0x3) as i32),
        "hashCode" => {
            let h = m ^ l;
            Value::Int(((h >> 32) as i32) ^ (h as i32))
        }
        "equals" => Value::Int((other(vm) == Some((m, l))) as i32),
        "compareTo" => {
            let Some(o) = other(vm) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "compareTo(null)",
                )));
            };
            Value::Int(match (m, l).cmp(&o) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        _ => return Ok(None),
    }))
}

/// MD5 (RFC 1321), for `nameUUIDFromBytes`.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bitlen = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    #[test]
    fn roundtrip_and_md5() {
        let s = "123e4567-e89b-12d3-a456-426614174000";
        let (m, l) = parse_uuid(s).unwrap();
        assert_eq!(format_uuid(m, l), s);
        let h = md5(b"");
        let mut hex = String::with_capacity(h.len() * 2);
        for byte in h {
            write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
        }
        assert_eq!(hex, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn reset_all_restarts_sequence() {
        reset_all();
        let first = next_u64();
        let _ = next_u64();
        reset_all();
        assert_eq!(
            next_u64(),
            first,
            "#230: RNG de UUID reinicia na fronteira de Engine"
        );
    }
}
