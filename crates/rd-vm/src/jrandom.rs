//! `java.util.Random` and `java.util.concurrent.ThreadLocalRandom`.
//!
//! The JDK 48-bit LCG bit-for-bit (`setSeed` scrambling, `next(bits)`,
//! `nextInt(bound)` rejection loop, `nextLong`/`nextDouble`/`nextFloat`), so
//! `new Random(seed)` reproduces a JVM's sequence. Unseeded generators use a
//! deterministic per-process seed sequence (`RD_RANDOM_SEED` overrides the
//! base) so agent runs are reproducible.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const RANDOM: &str = "Ljava/util/Random;";
pub(crate) const TLR: &str = "Ljava/util/concurrent/ThreadLocalRandom;";
const MULT: u64 = 0x5_DEEC_E66D;
const MASK: u64 = (1 << 48) - 1;

thread_local! {
    static SEEDS: RefCell<HashMap<ObjRef, u64>> = RefCell::new(HashMap::new());
    static UNIQ: RefCell<u64> = const { RefCell::new(0) };
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    SEEDS.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — limpa os seeds por `ObjRef` e
/// reinicia o uniquifier, para a sequência de `Random()` sem seed ser
/// função só deste `Engine` (determinismo, PRF-04).
pub(crate) fn reset_all() {
    SEEDS.with(|m| m.borrow_mut().clear());
    UNIQ.with(|u| *u.borrow_mut() = 0);
}

fn scramble(seed: i64) -> u64 {
    (seed as u64 ^ MULT) & MASK
}

fn default_seed() -> i64 {
    let base: i64 = std::env::var("RD_RANDOM_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x2545_F491_4F6C_DD1D);
    UNIQ.with(|u| {
        let mut u = u.borrow_mut();
        *u += 1;
        base ^ (*u as i64).wrapping_mul(1_181_783_497_276_652_981)
    })
}

fn next(r: ObjRef, bits: u32) -> i32 {
    SEEDS.with(|m| {
        let mut m = m.borrow_mut();
        let s = m.entry(r).or_insert_with(|| scramble(default_seed()));
        *s = (s.wrapping_mul(MULT).wrapping_add(0xB)) & MASK;
        (*s >> (48 - bits)) as i64 as i32
    })
}

fn next_int_bound(r: ObjRef, bound: i32) -> i32 {
    if bound & bound.wrapping_neg() == bound {
        return ((bound as i64 * next(r, 31) as i64) >> 31) as i32;
    }
    loop {
        let bits = next(r, 31);
        let val = bits % bound;
        if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
            return val;
        }
    }
}

fn next_long(r: ObjRef) -> i64 {
    ((next(r, 32) as i64) << 32).wrapping_add(next(r, 32) as i64)
}

fn next_double(r: ObjRef) -> f64 {
    (((next(r, 26) as i64) << 27) + next(r, 27) as i64) as f64 * (1.0 / (1u64 << 53) as f64)
}

fn iarg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}

fn larg(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(v)) => *v as i64,
        _ => 0,
    }
}

fn bad_bound(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/IllegalArgumentException;", msg))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if class != TLR || name != "current" {
        return Ok(None);
    }
    let key = (TLR.to_string(), "rd_instance".to_string());
    if let Some(v @ Value::Obj(_)) = vm.statics.get(&key) {
        return Ok(Some(v.clone()));
    }
    let r = vm.heap.alloc_instance(TLR.to_string(), Vec::new())?;
    vm.statics.insert(key, Value::Obj(r));
    Ok(Some(Value::Obj(r)))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let ours = recv_class == RANDOM || recv_class == TLR;
    if !(ours || (mref_class == RANDOM && name == "<init>")) {
        return Ok(None);
    }
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            SEEDS.with(|m| m.borrow_mut().insert(recv, scramble(default_seed())));
            Value::Null
        }
        ("<init>", "(J)V") | ("setSeed", "(J)V") => {
            let s = scramble(larg(args, 0));
            SEEDS.with(|m| m.borrow_mut().insert(recv, s));
            Value::Null
        }
        ("nextInt", "()I") => Value::Int(next(recv, 32)),
        ("nextInt", "(I)I") => {
            let b = iarg(args, 0);
            if b <= 0 {
                return Err(bad_bound("bound must be positive"));
            }
            Value::Int(next_int_bound(recv, b))
        }
        ("nextInt", "(II)I") => {
            let (lo, hi) = (iarg(args, 0), iarg(args, 1));
            if lo >= hi {
                return Err(bad_bound("bound must be greater than origin"));
            }
            let n = hi.wrapping_sub(lo);
            if n > 0 {
                Value::Int(lo + next_int_bound(recv, n))
            } else {
                loop {
                    let v = next(recv, 32);
                    if v >= lo && v < hi {
                        break Value::Int(v);
                    }
                }
            }
        }
        ("nextLong", "()J") => Value::Long(next_long(recv)),
        ("nextLong", "(J)J") => {
            let b = larg(args, 0);
            if b <= 0 {
                return Err(bad_bound("bound must be positive"));
            }
            Value::Long(next_long(recv).rem_euclid(b))
        }
        ("nextBoolean", "()Z") => Value::Int((next(recv, 1) != 0) as i32),
        ("nextFloat", "()F") => Value::Float(next(recv, 24) as f32 / (1u32 << 24) as f32),
        ("nextDouble", "()D") => Value::Double(next_double(recv)),
        ("nextGaussian", "()D") => {
            // polar Box–Muller as in the JDK (no cached second value)
            loop {
                let v1 = 2.0 * next_double(recv) - 1.0;
                let v2 = 2.0 * next_double(recv) - 1.0;
                let s = v1 * v1 + v2 * v2;
                if s < 1.0 && s != 0.0 {
                    break Value::Double(v1 * (-2.0 * s.ln() / s).sqrt());
                }
            }
        }
        ("nextBytes", "([B)V") => {
            if let Some(Value::Obj(arr)) = args.first() {
                let len = vm
                    .heap
                    .as_array_elems(*arr)
                    .map(|(e, _)| e.len())
                    .unwrap_or(0);
                let mut out = Vec::with_capacity(len);
                while out.len() < len {
                    let mut rnd = next(recv, 32);
                    for _ in 0..4.min(len - out.len()) {
                        out.push(Value::Int(rnd as i8 as i32));
                        rnd >>= 8;
                    }
                }
                if let Ok(e) = vm.heap.array_elems_mut(*arr) {
                    *e = out;
                }
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_jdk_sequence() {
        // new java.util.Random(42).nextInt() == -1170105035
        // new java.util.Random(42).nextInt(10) == 0 (first draw)
        let r: ObjRef = 7;
        SEEDS.with(|m| m.borrow_mut().insert(r, scramble(42)));
        assert_eq!(next(r, 32), -1170105035);
        SEEDS.with(|m| m.borrow_mut().insert(r, scramble(42)));
        assert_eq!(next_int_bound(r, 10), 0);
    }
}
