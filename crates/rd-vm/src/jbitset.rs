//! java.util.BitSet — host JDK-fiel (T9 follow-up da cascata Primftpd:
//! `MaterialShapeDrawable.<init>` faz `new BitSet(4)`; curado após o default
//! `View.onTouchEvent` destravar a cadeia TabView.performClick →
//! ViewPager.setCurrentItem).
//!
//! Semântica JDK real sobre um vetor de words u64 armazenado como heap array
//! `[J` no campo `rd_words` (GC-tracked, mutação in place quando a
//! capacidade não muda). `size()` reflete a capacidade alocada (o JDK nunca
//! encolhe o array de words); `length()`/`isEmpty()`/`cardinality()`/`
//! nextSetBit`/`nextClearBit` descontam words zero à direita virtualmente.
//!
//! **Não-modelado** (sem demanda observada; a cascata aponta se faltar):
//! `stream()`, `and/or/xor/andNot`, `get(int,int)`, `intersects`, `clone`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const BITSET: &str = "Ljava/util/BitSet;";
const WORDS: &str = "rd_words";
const IOOBE: &str = "Ljava/lang/IndexOutOfBoundsException;";
const NPE: &str = "Ljava/lang/NullPointerException;";
const NASE: &str = "Ljava/lang/NegativeArraySizeException;";

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg.into()))
}

fn a_i(args: &[Value], i: usize) -> Result<i32, VmExit> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        other => Err(exc(
            "Ljava/lang/IllegalArgumentException;",
            format!("BitSet: argumento {i} não é int: {other:?}"),
        )),
    }
}

/// Words do bitset lidas do heap (`rd_words` → `[J`). Ausente = vazio.
fn words_of(vm: &Engine, recv: ObjRef) -> Vec<u64> {
    match vm.heap.get_field(recv, WORDS) {
        Ok(Value::Obj(arr)) => match vm.heap.as_array_elems(arr) {
            Ok((elems, ElemKind::Long)) => elems
                .iter()
                .map(|v| match v {
                    Value::Long(l) => *l as u64,
                    _ => 0,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn words_alloc(vm: &mut Engine, recv: ObjRef, words: Vec<u64>) -> Result<(), VmExit> {
    let elems: Vec<Value> = words.iter().map(|w| Value::Long(*w as i64)).collect();
    let arr = crate::arrays::new_array(vm, "[J", ElemKind::Long, elems)?;
    crate::err::put_field_result(vm.heap.put_field(recv, WORDS, arr))
}

/// Mesma capacidade → mutação in place no array do heap; senão realoca.
fn words_store(vm: &mut Engine, recv: ObjRef, words: Vec<u64>) -> Result<(), VmExit> {
    let cur = match vm.heap.get_field(recv, WORDS) {
        Ok(Value::Obj(arr)) => Some(arr),
        _ => None,
    };
    if let Some(arr) = cur {
        let same = vm
            .heap
            .as_array_elems(arr)
            .map(|(e, _)| e.len() == words.len())
            .unwrap_or(false);
        if same {
            if let Ok(slot) = vm.heap.array_elems_mut(arr) {
                for (i, w) in words.iter().enumerate() {
                    slot[i] = Value::Long(*w as i64);
                }
                return Ok(());
            }
        }
    }
    words_alloc(vm, recv, words)
}

// ── operações de bits (JDK: word = 64 bits, índice >> 6, deslocamento & 63) ──

fn get_bit(words: &[u64], i: usize) -> bool {
    matches!(words.get(i >> 6), Some(w) if w & (1u64 << (i & 63)) != 0)
}

/// Última word com bit setado (JDK wordsInUse-1); None = vazio.
fn top_word(words: &[u64]) -> Option<usize> {
    words.iter().rposition(|w| *w != 0)
}

fn is_empty(words: &[u64]) -> bool {
    words.iter().all(|w| *w == 0)
}

fn cardinality(words: &[u64]) -> i32 {
    words.iter().map(|w| w.count_ones()).sum::<u32>() as i32
}

fn length(words: &[u64]) -> i32 {
    match top_word(words) {
        Some(idx) => (idx * 64 + (64 - words[idx].leading_zeros() as usize)) as i32,
        None => 0,
    }
}

fn next_set_bit(words: &[u64], from: usize) -> i32 {
    for i in from..words.len() * 64 {
        if get_bit(words, i) {
            return i as i32;
        }
    }
    -1
}

fn next_clear_bit(words: &[u64], from: usize) -> i32 {
    for i in from..words.len() * 64 {
        if !get_bit(words, i) {
            return i as i32;
        }
    }
    (words.len() * 64) as i32
}

fn check_bit_index(bit: i32) -> Result<usize, VmExit> {
    if bit < 0 {
        return Err(exc(IOOBE, format!("bitIndex < 0: {bit}")));
    }
    Ok(bit as usize)
}

fn check_range(from: i32, to: i32) -> Result<(usize, usize), VmExit> {
    if from < 0 {
        return Err(exc(IOOBE, format!("fromIndex < 0: {from}")));
    }
    if to < from {
        return Err(exc(IOOBE, format!("toIndex < fromIndex: {to}")));
    }
    Ok((from as usize, to as usize))
}

/// JDK hashCode: `h ^= words[i] * (i+1)` do topo para a base, depois
/// `(int)(h >> 32) ^ (int)h`.
fn jdk_hash(words: &[u64]) -> i64 {
    let mut h: u64 = 1234;
    for (i, w) in words.iter().enumerate().rev() {
        h ^= w.wrapping_mul((i + 1) as u64);
    }
    ((h >> 32) ^ h) as i64
}

/// Comparação lógica (trailing zeros equivalentes) — JDK compara até
/// wordsInUse.
fn logical_eq(a: &[u64], b: &[u64]) -> bool {
    let mut x = a;
    let mut y = b;
    while let Some((&last, rest)) = x.split_last() {
        if last == 0 {
            x = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = y.split_last() {
        if last == 0 {
            y = rest;
        } else {
            break;
        }
    }
    x == y
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
    if recv_class != BITSET || mref_class != BITSET {
        return Ok(None);
    }
    let words = words_of(vm, recv);
    Ok(Some(match (name, sig) {
        // ── constructors ────────────────────────────────────────────────
        ("<init>", "()V") => {
            words_alloc(vm, recv, Vec::new())?;
            Value::Null
        }
        ("<init>", "(I)V") => {
            let n = a_i(args, 0)?;
            if n < 0 {
                return Err(exc(NASE, format!("nbits < 0: {n}")));
            }
            // JDK initWords: ((nbits - 1) >> 6) + 1 words (0 → vazio)
            let cap = if n == 0 {
                0
            } else {
                (((n - 1) >> 6) + 1) as usize
            };
            words_alloc(vm, recv, vec![0u64; cap])?;
            Value::Null
        }
        ("<init>", "([J)V") => {
            let Some(Value::Obj(arr)) = args.first() else {
                return Err(exc(NPE, "BitSet(long[]) com null"));
            };
            let (elems, ElemKind::Long) = vm.heap.as_array_elems(*arr)? else {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "BitSet(long[]): array não é [J",
                ));
            };
            let copy: Vec<u64> = elems
                .iter()
                .map(|v| match v {
                    Value::Long(l) => *l as u64,
                    _ => 0,
                })
                .collect();
            words_alloc(vm, recv, copy)?;
            Value::Null
        }
        // ── set / clear / flip ──────────────────────────────────────────
        ("set", "(I)V") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            let mut w = words;
            if w.len() <= i >> 6 {
                w.resize((i >> 6) + 1, 0);
            }
            w[i >> 6] |= 1u64 << (i & 63);
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("set", "(IZ)V") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            let on = matches!(args.get(1), Some(Value::Int(n)) if *n != 0);
            let mut w = words;
            if on {
                if w.len() <= i >> 6 {
                    w.resize((i >> 6) + 1, 0);
                }
                w[i >> 6] |= 1u64 << (i & 63);
            } else if let Some(slot) = w.get_mut(i >> 6) {
                *slot &= !(1u64 << (i & 63));
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("set", "(II)V" | "(IIZ)V") => {
            let (from, to) = check_range(a_i(args, 0)?, a_i(args, 1)?)?;
            let on = match args.get(2) {
                Some(Value::Int(n)) => *n != 0,
                _ => true,
            };
            let mut w = words;
            for i in from..to {
                if on {
                    if w.len() <= i >> 6 {
                        w.resize((i >> 6) + 1, 0);
                    }
                    w[i >> 6] |= 1u64 << (i & 63);
                } else if let Some(slot) = w.get_mut(i >> 6) {
                    *slot &= !(1u64 << (i & 63));
                }
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("clear", "(I)V") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            let mut w = words;
            if let Some(slot) = w.get_mut(i >> 6) {
                *slot &= !(1u64 << (i & 63));
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("clear", "(II)V") => {
            let (from, to) = check_range(a_i(args, 0)?, a_i(args, 1)?)?;
            let mut w = words;
            for i in from..to {
                if let Some(slot) = w.get_mut(i >> 6) {
                    *slot &= !(1u64 << (i & 63));
                }
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("clear", "()V") => {
            let w = vec![0u64; words.len()];
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("flip", "(I)V") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            let mut w = words;
            if w.len() <= i >> 6 {
                w.resize((i >> 6) + 1, 0);
            }
            w[i >> 6] ^= 1u64 << (i & 63);
            words_store(vm, recv, w)?;
            Value::Null
        }
        // ── queries ─────────────────────────────────────────────────────
        ("get", "(I)Z") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            Value::Int(get_bit(&words, i) as i32)
        }
        ("cardinality", "()I") => Value::Int(cardinality(&words)),
        ("nextSetBit", "(I)I") => {
            let from = a_i(args, 0)?;
            if from < 0 {
                return Err(exc(IOOBE, format!("fromIndex < 0: {from}")));
            }
            Value::Int(next_set_bit(&words, from as usize))
        }
        ("nextClearBit", "(I)I") => {
            let from = a_i(args, 0)?;
            if from < 0 {
                return Err(exc(IOOBE, format!("fromIndex < 0: {from}")));
            }
            Value::Int(next_clear_bit(&words, from as usize))
        }
        ("isEmpty", "()Z") => Value::Int(is_empty(&words) as i32),
        ("length", "()I") => Value::Int(length(&words)),
        ("size", "()I") => Value::Int((words.len() * 64) as i32),
        // ── objeto / conversões ─────────────────────────────────────────
        ("toString", "()Ljava/lang/String;") => {
            let mut bits = Vec::new();
            let mut i = next_set_bit(&words, 0);
            while i >= 0 {
                bits.push(i.to_string());
                i = next_set_bit(&words, (i + 1) as usize);
            }
            crate::intrinsics::alloc_string(vm, format!("{{{}}}", bits.join(", ")))
                .map(Value::Obj)?
        }
        ("equals", "(Ljava/lang/Object;)Z") => match args.first() {
            Some(Value::Obj(other)) if *other == recv => Value::Int(1),
            Some(Value::Obj(other)) => {
                let same = vm.heap.class_of(*other).is_ok_and(|c| c == BITSET)
                    && logical_eq(&words, &words_of(vm, *other));
                Value::Int(same as i32)
            }
            _ => Value::Int(0),
        },
        ("hashCode", "()I") => Value::Int(jdk_hash(&words) as i32),
        ("toLongArray", "()[J") => {
            let elems: Vec<Value> = words.iter().map(|w| Value::Long(*w as i64)).collect();
            crate::arrays::new_array(vm, "[J", ElemKind::Long, elems)?
        }
        _ => return Ok(None),
    }))
}
