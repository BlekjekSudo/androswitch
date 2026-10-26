//! `android.text.SpannableString` and `android.text.SpannableStringBuilder`.
//!
//! - **State.** Text is kept as UTF-16 code units (Java indexing), with a
//!   span list of `(what, start, end, flags)` per object, in a
//!   thread-local side table keyed by the object.
//! - **Spanned API.** `CharSequence` (`length`, `charAt`, `toString`,
//!   `subSequence`, which keeps the spans it covers) and `Spanned`
//!   (`getSpans` filtered by class, `getSpanStart`/`getSpanEnd`/
//!   `getSpanFlags`, `nextSpanTransition`), plus `setSpan`/`removeSpan`.
//! - **Editing.** `SpannableStringBuilder` adds `append` (all overloads),
//!   `insert`, `replace`, `delete` and `clear`.
//! - **Span adjustment on edit, simplified from AOSP.** Spans after the
//!   edited range shift by the length delta. Spans overlapping it are
//!   clamped. Spans emptied by a deletion are dropped unless they are
//!   point spans (`SPAN_POINT_POINT`-style zero length, kept at the edit
//!   start).
//! - **Copying.** Copying a `Spanned` source (constructor, `append`,
//!   `insert`, `replace`) copies its spans.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

pub(crate) const SS: &str = "Landroid/text/SpannableString;";
pub(crate) const SSB: &str = "Landroid/text/SpannableStringBuilder;";

#[derive(Debug, Clone)]
struct Span {
    what: Value,
    start: usize,
    end: usize,
    flags: i32,
}

#[derive(Debug, Clone, Default)]
struct State {
    text: Vec<u16>,
    spans: Vec<Span>,
}

thread_local! {
    static STATES: RefCell<HashMap<ObjRef, State>> = RefCell::new(HashMap::new());
}

/// GC: objects that have state here (marking consults `gc_entry` only
/// for these).
pub(crate) fn gc_keys(out: &mut Vec<ObjRef>) {
    STATES.with(|m| out.extend(m.borrow().keys().copied()));
}

/// GC ephemeron: state of a reachable object (holds heap refs).
pub(crate) fn gc_entry(r: ObjRef, out: &mut impl std::fmt::Write) {
    STATES.with(|m| {
        if let Some(v) = m.borrow().get(&r) {
            let _ = write!(out, "|{v:?}");
        }
    });
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    STATES.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: esvazia a tabela na fronteira de `Engine` — sem isso um
/// `Engine` novo na mesma thread herda estados indexados por `ObjRef`s
/// de um heap morto (os refs reiniciam a cada `Heap::new`).
pub(crate) fn reset_all() {
    STATES.with(|m| m.borrow_mut().clear());
}

pub(crate) fn is_spannable(class: &str) -> bool {
    class == SS || class == SSB
}

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.into()))
}

fn int(args: &[Value], i: usize) -> Result<i32, VmExit> {
    match args.get(i) {
        Some(v) => Ok(v.as_int()?),
        None => Ok(0),
    }
}

/// Text (and spans, when the source is one of ours) of a CharSequence value.
fn source(vm: &mut Engine, v: Option<&Value>) -> Result<State, VmExit> {
    match v {
        Some(Value::Obj(r)) => {
            if let Some(st) = STATES.with(|s| s.borrow().get(r).cloned()) {
                return Ok(st);
            }
            let s = match vm.heap.as_str(*r) {
                Ok(s) => s.to_string(),
                Err(_) => crate::intrinsics::object_to_string(vm, *r)?,
            };
            Ok(State {
                text: s.encode_utf16().collect(),
                spans: Vec::new(),
            })
        }
        Some(Value::Null) => Ok(State {
            text: "null".encode_utf16().collect(),
            spans: Vec::new(),
        }),
        _ => Ok(State::default()),
    }
}

fn get(r: ObjRef) -> State {
    STATES.with(|s| s.borrow().get(&r).cloned().unwrap_or_default())
}

fn put(r: ObjRef, st: State) {
    STATES.with(|s| {
        s.borrow_mut().insert(r, st);
    });
}

fn range(len: usize, a: i32, b: i32, what: &str) -> Result<(usize, usize), VmExit> {
    if a < 0 || b < a || b as usize > len {
        return Err(exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("{what} ({a} ... {b}) has end beyond length {len}"),
        ));
    }
    Ok((a as usize, b as usize))
}

/// Replace `[a, b)` of `st` with `src`, adjusting the spans.
fn splice(st: &mut State, a: usize, b: usize, src: &State) {
    let ins = src.text.len();
    let delta = ins as isize - (b - a) as isize;
    st.text.splice(a..b, src.text.iter().copied());
    let shift = |p: usize| -> usize {
        if p >= b {
            (p as isize + delta) as usize
        } else if p > a {
            // inside the replaced range: clamp to the edit start (or end of insert)
            a + ins.min(p - a)
        } else {
            p
        }
    };
    let mut kept = Vec::with_capacity(st.spans.len() + src.spans.len());
    for sp in st.spans.drain(..) {
        let (s, e) = (shift(sp.start), shift(sp.end));
        let was_point = sp.start == sp.end;
        if s == e && !was_point && b > a {
            continue;
        }
        kept.push(Span {
            start: s,
            end: e.max(s),
            ..sp
        });
    }
    for sp in &src.spans {
        kept.push(Span {
            start: a + sp.start,
            end: a + sp.end,
            ..sp.clone()
        });
    }
    st.spans = kept;
}

fn jstr(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

fn new_obj(vm: &mut Engine, class: &str, st: State) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    put(r, st);
    Ok(Value::Obj(r))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !is_spannable(class) || name != "valueOf" {
        return Ok(None);
    }
    if let Some(Value::Obj(r)) = args.first() {
        if vm.heap.class_of(*r).map(|c| c == class).unwrap_or(false) {
            return Ok(Some(Value::Obj(*r)));
        }
    }
    let st = source(vm, args.first())?;
    Ok(Some(new_obj(vm, class, st)?))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !is_spannable(recv_class) {
        return Ok(None);
    }
    let builder = recv_class == SSB;
    Ok(Some(match name {
        "<init>" => {
            let mut st = if args.is_empty() {
                State::default()
            } else {
                source(vm, args.first())?
            };
            if args.len() >= 3 {
                // (CharSequence, int start, int end)
                let (a, b) = range(st.text.len(), int(args, 1)?, int(args, 2)?, "subSequence")?;
                let mut sub = State {
                    text: st.text[a..b].to_vec(),
                    spans: Vec::new(),
                };
                for sp in &st.spans {
                    if sp.end >= a && sp.start <= b {
                        sub.spans.push(Span {
                            start: sp.start.max(a) - a,
                            end: sp.end.min(b) - a,
                            ..sp.clone()
                        });
                    }
                }
                st = sub;
            }
            put(recv, st);
            Value::Null
        }
        "length" => Value::Int(get(recv).text.len() as i32),
        "charAt" => {
            let st = get(recv);
            let i = int(args, 0)?;
            match st.text.get(i.max(0) as usize) {
                Some(c) if i >= 0 => Value::Int(*c as i32),
                _ => {
                    return Err(exc(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        format!("charAt: {i} >= length {}", st.text.len()),
                    ))
                }
            }
        }
        "toString" => {
            let st = get(recv);
            jstr(vm, String::from_utf16_lossy(&st.text))?
        }
        "hashCode" => {
            let st = get(recv);
            Value::Int(
                st.text
                    .iter()
                    .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(*c as i32)),
            )
        }
        "equals" => {
            let st = get(recv);
            let same = match args.first() {
                Some(Value::Obj(o)) if STATES.with(|s| s.borrow().contains_key(o)) => {
                    get(*o).text == st.text
                }
                _ => false,
            };
            Value::Int(same as i32)
        }
        "subSequence" => {
            let st = get(recv);
            let (a, b) = range(st.text.len(), int(args, 0)?, int(args, 1)?, "subSequence")?;
            let mut sub = State {
                text: st.text[a..b].to_vec(),
                spans: Vec::new(),
            };
            for sp in &st.spans {
                if sp.end >= a && sp.start <= b && !(sp.start == b && sp.end > b) {
                    sub.spans.push(Span {
                        start: sp.start.max(a) - a,
                        end: sp.end.min(b) - a,
                        ..sp.clone()
                    });
                }
            }
            new_obj(vm, recv_class, sub)?
        }
        "getChars" => {
            // getChars(int start, int end, char[] dest, int destoff)
            let st = get(recv);
            let (a, b) = range(st.text.len(), int(args, 0)?, int(args, 1)?, "getChars")?;
            if let Some(Value::Obj(dst)) = args.get(2) {
                let off = int(args, 3)?.max(0) as usize;
                let elems = vm.heap.array_elems_mut(*dst)?;
                for (k, c) in st.text[a..b].iter().enumerate() {
                    if let Some(slot) = elems.get_mut(off + k) {
                        *slot = Value::Int(*c as i32);
                    }
                }
            }
            Value::Null
        }
        "setSpan" => {
            let mut st = get(recv);
            let (a, b) = range(st.text.len(), int(args, 1)?, int(args, 2)?, "setSpan")?;
            let what = args.first().cloned().unwrap_or(Value::Null);
            let flags = int(args, 3)?;
            if let Some(sp) = st.spans.iter_mut().find(|s| s.what == what) {
                sp.start = a;
                sp.end = b;
                sp.flags = flags;
            } else {
                st.spans.push(Span {
                    what,
                    start: a,
                    end: b,
                    flags,
                });
            }
            put(recv, st);
            Value::Null
        }
        "removeSpan" => {
            let mut st = get(recv);
            let what = args.first().cloned().unwrap_or(Value::Null);
            st.spans.retain(|s| s.what != what);
            put(recv, st);
            Value::Null
        }
        "getSpanStart" | "getSpanEnd" | "getSpanFlags" => {
            let st = get(recv);
            let what = args.first().cloned().unwrap_or(Value::Null);
            Value::Int(match st.spans.iter().find(|s| s.what == what) {
                Some(s) if name == "getSpanStart" => s.start as i32,
                Some(s) if name == "getSpanEnd" => s.end as i32,
                Some(s) => s.flags,
                None if name == "getSpanFlags" => 0,
                None => -1,
            })
        }
        "getSpans" => {
            let st = get(recv);
            let (qa, qb) = (int(args, 0)?.max(0) as usize, int(args, 1)?.max(0) as usize);
            let want = match args.get(2) {
                Some(Value::Obj(c)) => crate::reflect::class_desc(vm, *c)
                    .unwrap_or_else(|| "Ljava/lang/Object;".into()),
                _ => "Ljava/lang/Object;".into(),
            };
            let mut out = Vec::new();
            for sp in &st.spans {
                // AOSP: overlap, plus empty spans/queries touching the boundary
                let overlaps = sp.start < qb && sp.end > qa
                    || (sp.start == sp.end && sp.start >= qa && sp.start <= qb)
                    || (qa == qb && sp.start <= qa && sp.end >= qa);
                if !overlaps {
                    continue;
                }
                if let Value::Obj(o) = &sp.what {
                    let oc = vm.heap.class_of(*o)?.to_string();
                    if want == "Ljava/lang/Object;" || crate::jlclass::assignable(vm, &want, &oc) {
                        out.push(sp.what.clone());
                    }
                }
            }
            let a = vm.heap.alloc_array(ElemKind::Obj, out.len(), &want)?;
            *vm.heap.array_elems_mut(a)? = out;
            Value::Obj(a)
        }
        "nextSpanTransition" => {
            // nextSpanTransition(int start, int limit, Class kind)
            let st = get(recv);
            let (start, limit) = (int(args, 0)?.max(0) as usize, int(args, 1)?.max(0) as usize);
            let mut next = limit;
            for sp in &st.spans {
                if sp.start > start && sp.start < next {
                    next = sp.start;
                }
                if sp.end > start && sp.end < next {
                    next = sp.end;
                }
            }
            Value::Int(next as i32)
        }
        // ── SpannableStringBuilder editing ──────────────────────────────
        "append" if builder => {
            let mut st = get(recv);
            let src = match sig {
                "(C)Landroid/text/SpannableStringBuilder;"
                | "(C)Landroid/text/Editable;"
                | "(C)Ljava/lang/Appendable;" => State {
                    text: vec![int(args, 0)? as u16],
                    spans: Vec::new(),
                },
                _ => {
                    let mut s = source(vm, args.first())?;
                    if sig.starts_with("(Ljava/lang/CharSequence;II)") {
                        let (a, b) = range(s.text.len(), int(args, 1)?, int(args, 2)?, "append")?;
                        s = State {
                            text: s.text[a..b].to_vec(),
                            spans: s
                                .spans
                                .iter()
                                .filter(|sp| sp.end >= a && sp.start <= b)
                                .map(|sp| Span {
                                    start: sp.start.max(a) - a,
                                    end: sp.end.min(b) - a,
                                    ..sp.clone()
                                })
                                .collect(),
                        };
                    }
                    s
                }
            };
            let at = st.text.len();
            splice(&mut st, at, at, &src);
            if sig.starts_with("(Ljava/lang/CharSequence;Ljava/lang/Object;I)") {
                // append(text, what, flags)
                st.spans.push(Span {
                    what: args.get(1).cloned().unwrap_or(Value::Null),
                    start: at,
                    end: st.text.len(),
                    flags: int(args, 2)?,
                });
            }
            put(recv, st);
            Value::Obj(recv)
        }
        "insert" if builder => {
            let mut st = get(recv);
            let at = int(args, 0)?;
            let (a, _) = range(st.text.len(), at, at, "insert")?;
            let src = source(vm, args.get(1))?;
            splice(&mut st, a, a, &src);
            put(recv, st);
            Value::Obj(recv)
        }
        "replace" if builder => {
            let mut st = get(recv);
            let (a, b) = range(st.text.len(), int(args, 0)?, int(args, 1)?, "replace")?;
            let mut src = source(vm, args.get(2))?;
            if args.len() >= 5 {
                let (sa, sb) = range(src.text.len(), int(args, 3)?, int(args, 4)?, "replace")?;
                src = State {
                    text: src.text[sa..sb].to_vec(),
                    spans: Vec::new(),
                };
            }
            splice(&mut st, a, b, &src);
            put(recv, st);
            Value::Obj(recv)
        }
        "delete" if builder => {
            let mut st = get(recv);
            let (a, b) = range(st.text.len(), int(args, 0)?, int(args, 1)?, "delete")?;
            splice(&mut st, a, b, &State::default());
            put(recv, st);
            Value::Obj(recv)
        }
        "clear" if builder => {
            put(recv, State::default());
            Value::Null
        }
        "clearSpans" => {
            let mut st = get(recv);
            st.spans.clear();
            put(recv, st);
            Value::Null
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splice_shifts_spans() {
        let mut st = State {
            text: "hello world".encode_utf16().collect(),
            spans: Vec::new(),
        };
        st.spans.push(Span {
            what: Value::Int(1),
            start: 6,
            end: 11,
            flags: 33,
        });
        let ins = State {
            text: "big ".encode_utf16().collect(),
            spans: Vec::new(),
        };
        splice(&mut st, 6, 6, &ins);
        assert_eq!(String::from_utf16_lossy(&st.text), "hello big world");
        assert_eq!((st.spans[0].start, st.spans[0].end), (10, 15));
        splice(&mut st, 0, 6, &State::default());
        assert_eq!((st.spans[0].start, st.spans[0].end), (4, 9));
    }
}
