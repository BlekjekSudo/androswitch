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

/// issue #293: o array de words e um `[J` do heap - a contabilidade da arena
/// cobra 32 B por slot (tamanho de `Value`). Falha ANTES de qualquer alocacao
/// no host (OutOfMemoryError capturavel, mesma conta que o alloc_array faz
/// depois) - `new BitSet(Integer.MAX_VALUE)` nao pode mais gastar 256 MiB +
/// 536 MiB de `Vec<Value>` transitorio fora do orcamento.
fn check_words_budget(vm: &Engine, n_words: usize) -> Result<(), VmExit> {
    let bytes = n_words
        .saturating_mul(std::mem::size_of::<Value>())
        .saturating_add(16);
    if bytes > vm.config.heap_budget {
        return Err(exc(
            "Ljava/lang/OutOfMemoryError;",
            format!(
                "alocacao de {bytes} bytes excede o heap de {} bytes",
                vm.config.heap_budget
            ),
        ));
    }
    Ok(())
}

/// JDK wordsInUse: indice da ultima word nao nula + 1 (0 = vazio).
fn words_in_use(words: &[u64]) -> usize {
    top_word(words).map_or(0, |t| t + 1)
}

/// Instancia um BitSet novo no heap (fabrica interna de valueOf/clone/get) -
/// o `<init>([J)V` e private no JDK e nunca despacha.
fn new_bitset(vm: &mut Engine, words: Vec<u64>) -> Result<ObjRef, VmExit> {
    let obj = vm
        .heap
        .alloc_instance(BITSET.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    words_alloc(vm, obj, words)?;
    Ok(obj)
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
    // issue #293: saturação honesta (o orçamento torna i32::MAX bits
    // inalcançável; com budgets maiores o JDK usaria wrap — nós saturamos).
    let total = words.iter().map(|w| w.count_ones() as u64).sum::<u64>();
    i32::try_from(total).unwrap_or(i32::MAX)
}

fn length(words: &[u64]) -> i32 {
    match top_word(words) {
        Some(idx) => i32::try_from(idx as i64 * 64 + (64 - words[idx].leading_zeros() as i64))
            .unwrap_or(i32::MAX),
        None => 0,
    }
}

fn size_words(words: &[u64]) -> i32 {
    i32::try_from(words.len() as i64 * 64).unwrap_or(i32::MAX)
}

fn next_set_bit(words: &[u64], from: usize) -> i32 {
    // issue #293: varredura por word (trailing_zeros), O(words) — no tamanho
    // do orçamento a varredura bit a bit antiga era um hang de ~536 M bits.
    let mut u = from >> 6;
    if u >= words.len() {
        return -1;
    }
    let mut word = words[u] & (u64::MAX << (from & 63));
    loop {
        if word != 0 {
            return (u * 64 + word.trailing_zeros() as usize) as i32;
        }
        u += 1;
        if u == words.len() {
            return -1;
        }
        word = words[u];
    }
}

fn next_clear_bit(words: &[u64], from: usize) -> i32 {
    // JDK: se (fromIndex >> 6) >= wordsInUse, a resposta é o PRÓPRIO
    // fromIndex (bits além do uso são clear virtuais) — o código antigo
    // devolvia words.len()*64 (menor que fromIndex) ou 0 no BitSet vazio.
    let wiu = words_in_use(words);
    let mut u = from >> 6;
    if u >= wiu {
        return i32::try_from(from).unwrap_or(i32::MAX);
    }
    let mut word = !words[u] & (u64::MAX << (from & 63));
    loop {
        if word != 0 {
            return (u * 64 + word.trailing_zeros() as usize) as i32;
        }
        u += 1;
        if u == wiu {
            return i32::try_from(wiu * 64).unwrap_or(i32::MAX);
        }
        word = !words[u];
    }
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

// ── faixas por máscara (issue #293: O(words), nunca bit a bit) ─────────────
/// Bits [from&63, 64) — primeira word da faixa.
fn mask_from(from_mod: u32) -> u64 {
    u64::MAX << from_mod
}
/// Bits [0, to&63) — última word da faixa (to&63 == 0 → word cheia).
fn mask_to(to_mod: u32) -> u64 {
    if to_mod == 0 {
        u64::MAX
    } else {
        (1u64 << to_mod) - 1
    }
}

/// Outro BitSet recebido como argumento (and/or/xor/andNot/intersects):
/// null → NPE; não-BitSet → IAE (o verificador JVM barraria, o host é defesa).
fn bitset_arg(vm: &Engine, args: &[Value], op: &str) -> Result<Vec<u64>, VmExit> {
    match args.first() {
        Some(Value::Obj(other)) => {
            if vm
                .heap
                .class_of(*other)
                .is_ok_and(|c| vm.cp.is_subtype(c, BITSET))
            {
                Ok(words_of(vm, *other))
            } else {
                Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("BitSet.{op}: argumento não é BitSet"),
                ))
            }
        }
        _ => Err(exc(NPE, format!("BitSet.{op} com null"))),
    }
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
    // issue #293: BitSet NÃO é final no JDK — subclasses guest despacham para
    // o host (o gate antigo por igualdade textual rejeitava qualquer subclasse
    // com NOT_FOUND).
    if !vm.cp.is_subtype(recv_class, BITSET) || !vm.cp.is_subtype(mref_class, BITSET) {
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
            check_words_budget(vm, cap)?;
            words_alloc(vm, recv, vec![0u64; cap])?;
            Value::Null
        }
        ("<init>", "([J)V") => {
            // issue #293: BitSet(long[]) é PRIVATE no JDK — o javac jamais
            // emite invoke-direct para ele; a rota pública é valueOf([J)/([B).
            return Err(exc(
                "Ljava/lang/IllegalAccessError;",
                "BitSet(long[]) is private in the JDK; use BitSet.valueOf(long[])",
            ));
        }
        // ── set / clear / flip ──────────────────────────────────────────
        ("set", "(I)V") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            let mut w = words;
            if w.len() <= i >> 6 {
                check_words_budget(vm, (i >> 6) + 1)?;
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
                    check_words_budget(vm, (i >> 6) + 1)?;
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
            if from != to {
                if on {
                    // JDK set(from,to): cresce até wordIndex(to-1) e opera por
                    // máscara — a versão antiga fazia um loop bit a bit de
                    // até 2,1 bi iterações (hang) mesmo com BitSet vazio.
                    let first_word = from >> 6;
                    let last_word = (to - 1) >> 6;
                    check_words_budget(vm, last_word + 1)?;
                    if w.len() <= last_word {
                        w.resize(last_word + 1, 0);
                    }
                    let fm = mask_from((from & 63) as u32);
                    let tm = mask_to((to & 63) as u32);
                    if first_word == last_word {
                        // JDK: mesma word → interseção das duas máscaras
                        w[first_word] |= fm & tm;
                    } else {
                        w[first_word] |= fm;
                        w[last_word] |= tm;
                        for word in &mut w[first_word + 1..last_word] {
                            *word = u64::MAX;
                        }
                    }
                } else {
                    // JDK: set(from,to,false) delega para clear(from,to).
                    clear_range(&mut w, from, to);
                }
                words_store(vm, recv, w)?;
            }
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
            if from != to {
                clear_range(&mut w, from, to);
                words_store(vm, recv, w)?;
            }
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
                check_words_budget(vm, (i >> 6) + 1)?;
                w.resize((i >> 6) + 1, 0);
            }
            w[i >> 6] ^= 1u64 << (i & 63);
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("flip", "(II)V") => {
            let (from, to) = check_range(a_i(args, 0)?, a_i(args, 1)?)?;
            let mut w = words;
            if from != to {
                let first_word = from >> 6;
                let last_word = (to - 1) >> 6;
                check_words_budget(vm, last_word + 1)?;
                if w.len() <= last_word {
                    w.resize(last_word + 1, 0);
                }
                let fm = mask_from((from & 63) as u32);
                let tm = mask_to((to & 63) as u32);
                if first_word == last_word {
                    // JDK: mesma word → interseção das duas máscaras
                    w[first_word] ^= fm & tm;
                } else {
                    w[first_word] ^= fm;
                    w[last_word] ^= tm;
                    for word in &mut w[first_word + 1..last_word] {
                        *word ^= u64::MAX;
                    }
                }
                words_store(vm, recv, w)?;
            }
            Value::Null
        }
        // ── queries ─────────────────────────────────────────────────────
        ("get", "(I)Z") => {
            let i = check_bit_index(a_i(args, 0)?)?;
            Value::Int(get_bit(&words, i) as i32)
        }
        ("get", "(II)Ljava/util/BitSet;") => {
            // JDK: bits [from,to) deslocados para a base 0, toIndex clampado
            // ao length() (bits após o último set são descartados).
            let (from, to) = check_range(a_i(args, 0)?, a_i(args, 1)?)?;
            if from == to {
                return new_bitset(vm, Vec::new()).map(Value::Obj).map(Some);
            }
            let len = length(&words) as i64;
            if len <= from as i64 {
                return new_bitset(vm, Vec::new()).map(Value::Obj).map(Some);
            }
            let to = (to as i64).min(len) as usize;
            let from_bit = (from & 63) as u32;
            let first_word = from >> 6;
            let last_word = (to - 1) >> 6;
            let n = last_word - first_word + 1;
            check_words_budget(vm, n)?;
            let mut out = vec![0u64; n];
            for (j, i) in (first_word..=last_word).enumerate() {
                let mut shifted = words[i] >> from_bit;
                if from_bit != 0 && i + 1 < words.len() {
                    shifted |= words[i + 1] << (64 - from_bit);
                }
                out[j] = shifted;
            }
            let last_bits = (to - from) & 63;
            if last_bits != 0 {
                out[n - 1] &= (1u64 << last_bits) - 1;
            }
            new_bitset(vm, out).map(Value::Obj)?
        }
        ("and", "(Ljava/util/BitSet;)V") => {
            let other = bitset_arg(vm, args, "and")?;
            let mut w = words;
            for (i, ow) in other.iter().enumerate().take(w.len()) {
                w[i] &= ow;
            }
            for slot in w.iter_mut().skip(other.len()) {
                *slot = 0;
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("or", "(Ljava/util/BitSet;)V") => {
            let other = bitset_arg(vm, args, "or")?;
            let mut w = words;
            let o_wiu = words_in_use(&other);
            if w.len() < o_wiu {
                check_words_budget(vm, o_wiu)?;
                w.resize(o_wiu, 0);
            }
            for (i, ow) in other.iter().enumerate().take(o_wiu.min(w.len())) {
                w[i] |= ow;
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("xor", "(Ljava/util/BitSet;)V") => {
            let other = bitset_arg(vm, args, "xor")?;
            let mut w = words;
            let o_wiu = words_in_use(&other);
            if w.len() < o_wiu {
                check_words_budget(vm, o_wiu)?;
                w.resize(o_wiu, 0);
            }
            for (i, ow) in other.iter().enumerate().take(o_wiu.min(w.len())) {
                w[i] ^= ow;
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("andNot", "(Ljava/util/BitSet;)V") => {
            let other = bitset_arg(vm, args, "andNot")?;
            let mut w = words;
            let o_wiu = words_in_use(&other);
            for (i, ow) in other.iter().enumerate().take(o_wiu.min(w.len())) {
                w[i] &= !ow;
            }
            words_store(vm, recv, w)?;
            Value::Null
        }
        ("intersects", "(Ljava/util/BitSet;)Z") => {
            let other = bitset_arg(vm, args, "intersects")?;
            let hit = words.iter().zip(other.iter()).any(|(a, b)| a & b != 0);
            Value::Int(hit as i32)
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
        ("size", "()I") => Value::Int(size_words(&words)),
        // ── objeto / conversões ─────────────────────────────────────────
        ("toString", "()Ljava/lang/String;") => {
            let mut bits = Vec::new();
            let mut i = next_set_bit(&words, 0);
            while i >= 0 {
                bits.push(i.to_string());
                // issue #293: i + 1 em i32 estourava no bit MAX_VALUE
                // (panic de overflow em debug); o orçamento torna esse bit
                // inalcançável, e o guardo é defesa em profundidade.
                if i == i32::MAX {
                    break;
                }
                i = next_set_bit(&words, (i + 1) as usize);
            }
            crate::intrinsics::alloc_string(vm, format!("{{{}}}", bits.join(", ")))
                .map(Value::Obj)?
        }
        ("equals", "(Ljava/lang/Object;)Z") => match args.first() {
            Some(Value::Obj(other)) if *other == recv => Value::Int(1),
            Some(Value::Obj(other)) => {
                // issue #293: instanceof (is_subtype), não igualdade de classe
                // — subclasses guest comparam logicamente igual ao JDK.
                let same = vm
                    .heap
                    .class_of(*other)
                    .is_ok_and(|c| vm.cp.is_subtype(c, BITSET))
                    && logical_eq(&words, &words_of(vm, *other));
                Value::Int(same as i32)
            }
            _ => Value::Int(0),
        },
        ("hashCode", "()I") => Value::Int(jdk_hash(&words) as i32),
        ("clone", "()Ljava/lang/Object;") => {
            // JDK clone(): cópia com a MESMA capacidade de words.
            new_bitset(vm, words).map(Value::Obj)?
        }
        ("toLongArray", "()[J") => {
            // JDK: Arrays.copyOf(words, wordsInUse) — sem words zero à direita
            // (o código antigo devolvia long[2] de [0,0] para new BitSet(128)).
            let wiu = words_in_use(&words);
            let elems: Vec<Value> = words[..wiu]
                .iter()
                .map(|w| Value::Long(*w as i64))
                .collect();
            crate::arrays::new_array(vm, "[J", ElemKind::Long, elems)?
        }
        _ => return Ok(None),
    }))
}

/// clear(from,to) JDK: não cresce, limita às words em uso, máscara por word.
fn clear_range(w: &mut [u64], from: usize, to: usize) {
    let wiu = words_in_use(w);
    let first_word = from >> 6;
    if first_word >= wiu {
        return;
    }
    let last_word = ((to - 1) >> 6).min(wiu - 1);
    if first_word == last_word {
        let mask = mask_from((from & 63) as u32) & mask_to((to & 63) as u32);
        w[first_word] &= !mask;
    } else {
        w[first_word] &= !mask_from((from & 63) as u32);
        for slot in &mut w[first_word + 1..last_word] {
            *slot = 0;
        }
        w[last_word] &= !mask_to((to & 63) as u32);
    }
}

/// issue #293: fábricas públicas estáticas do JDK — `valueOf(long[])` copia
/// as words; `valueOf(byte[])` empacota little-endian, 8 bytes por word.
pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != BITSET {
        return Ok(None);
    }
    match (name, sig) {
        ("valueOf", "([J)Ljava/util/BitSet;") => {
            let Some(Value::Obj(arr)) = args.first() else {
                return Err(exc(NPE, "BitSet.valueOf(long[]) com null"));
            };
            let (elems, ElemKind::Long) = vm.heap.as_array_elems(*arr)? else {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "BitSet.valueOf: array não é [J",
                ));
            };
            let words: Vec<u64> = elems
                .iter()
                .map(|v| match v {
                    Value::Long(l) => *l as u64,
                    _ => 0,
                })
                .collect();
            new_bitset(vm, words).map(Value::Obj).map(Some)
        }
        ("valueOf", "([B)Ljava/util/BitSet;") => {
            let Some(Value::Obj(arr)) = args.first() else {
                return Err(exc(NPE, "BitSet.valueOf(byte[]) com null"));
            };
            let (elems, ElemKind::Int) = vm.heap.as_array_elems(*arr)? else {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "BitSet.valueOf: array não é [B",
                ));
            };
            let n_words = elems.len().div_ceil(8);
            check_words_budget(vm, n_words)?;
            let mut words = vec![0u64; n_words];
            for (i, b) in elems.iter().enumerate() {
                let byte = match b {
                    Value::Int(n) => *n as u8 as u64,
                    _ => 0,
                };
                words[i >> 3] |= byte << ((i & 7) * 8);
            }
            new_bitset(vm, words).map(Value::Obj).map(Some)
        }
        _ => Ok(None),
    }
}
