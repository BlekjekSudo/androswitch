//! `java.text.BreakIterator`: character, word, line and sentence instances.
//!
//! Boundaries are UTF-16 offsets, as in Java. The rules are a compact
//! approximation of the UAX #29 / UAX #14 defaults that the platform's ICU
//! implements:
//! - **character**: grapheme clusters. A surrogate pair, combining marks,
//!   variation selectors, skin-tone modifiers and ZWJ sequences stay together.
//! - **word**: boundaries between runs of letters/digits, runs of
//!   whitespace, and around each other character (punctuation, symbols).
//! - **line**: break opportunities after a run of whitespace and after a
//!   hyphen that follows a non-space.
//! - **sentence**: after `.`, `!` or `?` (and any closing quotes/brackets),
//!   once the following whitespace run ends.
//!
//! The iterator state (kind, text, current position) is kept per object;
//! `setText(CharacterIterator)` reads the text through the interface
//! (`first`/`next` until `DONE`) and offsets boundaries by its begin index.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const BI: &str = "Ljava/text/BreakIterator;";
const DONE: i32 = -1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Character,
    Word,
    Line,
    Sentence,
}

#[derive(Debug, Clone)]
pub(crate) struct BreakState {
    kind: Kind,
    /// boundaries (sorted, include `begin` and `begin + len`)
    bounds: Vec<i32>,
    pos: i32,
}

fn is_extend(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A
        | 0x064B..=0x065F | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F)
}

/// UTF-16 offsets of every char start, plus the total length.
fn utf16_starts(s: &str) -> (Vec<(char, i32)>, i32) {
    let mut out = Vec::with_capacity(s.len());
    let mut off = 0i32;
    for c in s.chars() {
        out.push((c, off));
        off += c.len_utf16() as i32;
    }
    (out, off)
}

fn compute(kind: Kind, text: &str) -> Vec<i32> {
    let (cs, len) = utf16_starts(text);
    let mut b = vec![0];
    let n = cs.len();
    match kind {
        Kind::Character => {
            for i in 1..n {
                let (c, off) = cs[i];
                let prev = cs[i - 1].0;
                let joined = is_extend(c) || c == '\u{200D}' || prev == '\u{200D}';
                // CR LF stays one cluster
                let crlf = prev == '\r' && c == '\n';
                // regional-indicator pairs (flags)
                let ri = |x: char| (0x1F1E6..=0x1F1FF).contains(&(x as u32));
                let flag = ri(prev) && ri(c) && {
                    let mut k = 0;
                    let mut j = i;
                    while j > 0 && ri(cs[j - 1].0) {
                        k += 1;
                        j -= 1;
                    }
                    k % 2 == 1
                };
                if !(joined || crlf || flag) {
                    b.push(off);
                }
            }
        }
        Kind::Word => {
            let class = |c: char| -> u8 {
                if c.is_alphanumeric() || c == '_' || c == '\'' || is_extend(c) {
                    0
                } else if c.is_whitespace() {
                    1
                } else {
                    2
                }
            };
            for i in 1..n {
                let (c, off) = cs[i];
                let (p, pc) = (cs[i - 1].0, class(cs[i - 1].0));
                let cc = class(c);
                // apostrophe/period inside a word ("don't", "3.14") joins
                let mid = cc == 0 && pc == 0;
                let mid_num = (p == '.' || p == ',')
                    && c.is_ascii_digit()
                    && i >= 2
                    && cs[i - 2].0.is_ascii_digit();
                let mid_num2 = (c == '.' || c == ',')
                    && p.is_ascii_digit()
                    && cs.get(i + 1).is_some_and(|x| x.0.is_ascii_digit());
                if mid || mid_num || mid_num2 || (cc == 1 && pc == 1) {
                    continue;
                }
                if (pc == 2 && cc == 2) || cc != pc {
                    b.push(off);
                }
            }
        }
        Kind::Line => {
            for i in 1..n {
                let (c, off) = cs[i];
                let p = cs[i - 1].0;
                let after_space = p.is_whitespace() && !c.is_whitespace();
                let after_hyphen = (p == '-' || p == '\u{2010}')
                    && i >= 2
                    && !cs[i - 2].0.is_whitespace()
                    && !c.is_whitespace();
                let hard = p == '\n' || p == '\u{2028}';
                if after_space || after_hyphen || hard {
                    b.push(off);
                }
            }
        }
        Kind::Sentence => {
            let mut i = 0;
            while i < n {
                if matches!(cs[i].0, '.' | '!' | '?' | '\u{3002}') {
                    let mut j = i + 1;
                    while j < n && matches!(cs[j].0, '.' | '!' | '?' | '"' | '\'' | ')' | ']') {
                        j += 1;
                    }
                    let k0 = j;
                    while j < n && cs[j].0.is_whitespace() {
                        j += 1;
                    }
                    if j < n && j > k0 {
                        b.push(cs[j].1);
                    }
                    i = j.max(i + 1);
                } else {
                    i += 1;
                }
            }
        }
    }
    if len > 0 {
        b.push(len);
    }
    b.dedup();
    b
}

fn new_iter(vm: &mut Engine, kind: Kind) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(BI.to_string(), Vec::new())?;
    vm.break_iters.insert(
        r,
        BreakState {
            kind,
            bounds: vec![0],
            pos: 0,
        },
    );
    Ok(Value::Obj(r))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != BI {
        return Ok(None);
    }
    let kind = match name {
        "getCharacterInstance" => Kind::Character,
        "getWordInstance" => Kind::Word,
        "getLineInstance" => Kind::Line,
        "getSentenceInstance" => Kind::Sentence,
        "getAvailableLocales" => return Ok(None),
        _ => return Ok(None),
    };
    Ok(Some(new_iter(vm, kind)?))
}

/// Text of a `java.text.CharacterIterator` via its interface, with its
/// begin index.
fn char_iter_text(vm: &mut Engine, it: ObjRef) -> Result<(String, i32), VmExit> {
    let begin = vm
        .invoke_instance(it, "getBeginIndex", "()I", &[])?
        .as_int()
        .unwrap_or(0);
    let mut units: Vec<u16> = Vec::new();
    let mut c = vm.invoke_instance(it, "first", "()C", &[])?.as_int()?;
    while c != 0xFFFF && units.len() < 1 << 22 {
        units.push(c as u16);
        c = vm.invoke_instance(it, "next", "()C", &[])?.as_int()?;
    }
    Ok((String::from_utf16_lossy(&units), begin))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let Some(mut st) = vm.break_iters.get(&recv).cloned() else {
        return Ok(None);
    };
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let first = *st.bounds.first().unwrap_or(&0);
    let last = *st.bounds.last().unwrap_or(&0);
    let out = match (name, sig) {
        ("setText", "(Ljava/lang/String;)V") => {
            let s = match args.first() {
                Some(v @ Value::Obj(_)) => crate::intrinsics::string_arg(vm, v)?,
                _ => String::new(),
            };
            st.bounds = compute(st.kind, &s);
            st.pos = 0;
            Value::Null
        }
        ("setText", "(Ljava/text/CharacterIterator;)V") => {
            let (s, begin) = match args.first() {
                Some(Value::Obj(it)) => char_iter_text(vm, *it)?,
                _ => (String::new(), 0),
            };
            st.bounds = compute(st.kind, &s)
                .into_iter()
                .map(|b| b + begin)
                .collect();
            st.pos = begin;
            Value::Null
        }
        ("first", "()I") => {
            st.pos = first;
            Value::Int(first)
        }
        ("last", "()I") => {
            st.pos = last;
            Value::Int(last)
        }
        ("current", "()I") => Value::Int(st.pos),
        ("next", "()I") => match st.bounds.iter().find(|b| **b > st.pos) {
            Some(b) => {
                st.pos = *b;
                Value::Int(*b)
            }
            None => Value::Int(DONE),
        },
        ("next", "(I)I") => {
            let n = int(0);
            let idx = st.bounds.iter().position(|b| *b == st.pos).unwrap_or(0) as i64 + n as i64;
            if idx < 0 || idx as usize >= st.bounds.len() {
                Value::Int(DONE)
            } else {
                st.pos = st.bounds[idx as usize];
                Value::Int(st.pos)
            }
        }
        ("previous", "()I") => match st.bounds.iter().rev().find(|b| **b < st.pos) {
            Some(b) => {
                st.pos = *b;
                Value::Int(*b)
            }
            None => Value::Int(DONE),
        },
        ("following", "(I)I") | ("preceding", "(I)I") | ("isBoundary", "(I)Z") => {
            let off = int(0);
            if off < first || off > last {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("offset out of bounds: {off}"),
                )));
            }
            match name {
                "following" => match st.bounds.iter().find(|b| **b > off) {
                    Some(b) => {
                        st.pos = *b;
                        Value::Int(*b)
                    }
                    None => {
                        st.pos = last;
                        Value::Int(DONE)
                    }
                },
                "preceding" => match st.bounds.iter().rev().find(|b| **b < off) {
                    Some(b) => {
                        st.pos = *b;
                        Value::Int(*b)
                    }
                    None => {
                        st.pos = first;
                        Value::Int(DONE)
                    }
                },
                _ => {
                    if st.bounds.contains(&off) {
                        st.pos = off;
                        Value::Int(1)
                    } else {
                        st.pos = st.bounds.iter().copied().find(|b| *b > off).unwrap_or(last);
                        Value::Int(0)
                    }
                }
            }
        }
        ("clone", "()Ljava/lang/Object;") => {
            let r = vm.heap.alloc_instance(BI.to_string(), Vec::new())?;
            vm.break_iters.insert(r, st.clone());
            Value::Obj(r)
        }
        _ => return Ok(None),
    };
    vm.break_iters.insert(recv, st);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_line_char_boundaries() {
        assert_eq!(compute(Kind::Word, "ola, mundo"), vec![0, 3, 4, 5, 10]);
        assert_eq!(compute(Kind::Line, "Feira do mes"), vec![0, 6, 9, 12]);
        // surrogate pair + combining accent stay one cluster
        assert_eq!(
            compute(Kind::Character, "a\u{1F600}e\u{301}"),
            vec![0, 1, 3, 5]
        );
        assert_eq!(
            compute(Kind::Sentence, "Oi. Tudo bem? Sim"),
            vec![0, 4, 14, 17]
        );
        assert_eq!(compute(Kind::Word, ""), vec![0]);
    }
}
