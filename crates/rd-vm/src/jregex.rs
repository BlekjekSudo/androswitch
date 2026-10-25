//! `java.util.regex.Pattern` / `Matcher` on bounded `fancy-regex`.
//!
//! - **Pattern** is a host object holding its source (`rd_regex`) and flags
//!   (`rd_flags`). Supported: `compile(regex[, flags])`, `quote`,
//!   `matches(regex, input)`, `matcher`, `split(input[, limit])`, `pattern`,
//!   `toString`, `flags`, `asPredicate`-free subset.
//! - **Matcher** keeps its input and match state per object: `find()`,
//!   `find(int)`, `matches`, `lookingAt`, `group()`/`group(int)`/
//!   `group(String)`, `groupCount`, `start`/`end` (optionally by group),
//!   `replaceAll`, `replaceFirst`, `reset([input])`, `hitEnd`, `pattern`,
//!   `appendReplacement`/`appendTail` (StringBuilder/StringBuffer).
//! - Indices are UTF-16 code units, as in Java.
//! - Java syntax translated: `\Q…\E` quoting, `(?<name>…)`,
//!   `\p{javaWhitespace}` and friends. Lookahead/lookbehind and
//!   backreferences use fancy-regex with a bounded backtracking budget.
//!   Exhaustion is a structured REGEX_RUNTIME error, never a silent miss.
//! - Flags: CASE_INSENSITIVE (2), COMMENTS (4), MULTILINE (8), LITERAL (16),
//!   DOTALL (32); UNICODE_CASE/UNICODE_CHARACTER_CLASS are accepted (the
//!   regex crate is Unicode-aware by default).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const PATTERN: &str = "Ljava/util/regex/Pattern;";
const MATCHER: &str = "Ljava/util/regex/Matcher;";
const MATCH_RESULT: &str = "Ljava/util/regex/MatchResult;";

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Anchor {
    None = 0,
    Start = 1,
    Full = 2,
}

#[derive(Debug, Default, Clone)]
struct MState {
    pattern: ObjRef,
    input: String,
    /// Groups of the last successful match, as byte ranges.
    groups: Vec<Option<(usize, usize)>>,
    /// Byte offset where the next `find()` starts.
    next: usize,
    /// Append position for appendReplacement (bytes).
    append: usize,
    matched: bool,
    hit_end: bool,
}

thread_local! {
    static CACHE: RefCell<HashMap<(String, i32, u8), fancy_regex::Regex>> = RefCell::new(HashMap::new());
    static MATCHERS: RefCell<HashMap<ObjRef, MState>> = RefCell::new(HashMap::new());
}

/// GC: objects that have state here (marking consults `gc_entry` only
/// for these).
pub(crate) fn gc_keys(out: &mut Vec<ObjRef>) {
    MATCHERS.with(|m| out.extend(m.borrow().keys().copied()));
}

/// GC ephemeron: state of a reachable object (holds heap refs).
pub(crate) fn gc_entry(r: ObjRef, out: &mut impl std::fmt::Write) {
    MATCHERS.with(|m| {
        if let Some(v) = m.borrow().get(&r) {
            let _ = write!(out, "|{v:?}");
        }
    });
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    MATCHERS.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — limpa só os matchers (indexados
/// por `ObjRef`); o `CACHE` de regex compiladas é cache puro por
/// (pattern, flags), válido entre engines, e fica.
pub(crate) fn reset_all() {
    MATCHERS.with(|m| m.borrow_mut().clear());
}

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}

/// Java regex → regex crate syntax.
pub(crate) fn translate(src: &str, flags: i32) -> String {
    if flags & 16 != 0 {
        return regex::escape(src);
    }
    let mut out = String::new();
    let mut it = src.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('Q') => {
                    let mut lit = String::new();
                    while let Some(d) = it.next() {
                        if d == '\\' && it.peek() == Some(&'E') {
                            it.next();
                            break;
                        }
                        lit.push(d);
                    }
                    out.push_str(&regex::escape(&lit));
                }
                Some(kind @ ('p' | 'P')) if it.peek() == Some(&'{') => {
                    let mut name = String::new();
                    it.next();
                    for d in it.by_ref() {
                        if d == '}' {
                            break;
                        }
                        name.push(d);
                    }
                    let (pos, neg): (String, String) = match name.as_str() {
                        "javaWhitespace" | "Space" | "IsWhite_Space" => {
                            ("\\s".into(), "\\S".into())
                        }
                        "javaDigit" | "Digit" | "IsDigit" => ("\\d".into(), "\\D".into()),
                        "javaLetterOrDigit" | "Alnum" => (
                            "[\\p{Alphabetic}\\d]".into(),
                            "[^\\p{Alphabetic}\\d]".into(),
                        ),
                        "InCombiningDiacriticalMarks" => (
                            "[\\u{0300}-\\u{036F}]".into(),
                            "[^\\u{0300}-\\u{036F}]".into(),
                        ),
                        n => {
                            let prop = match n {
                                "javaLetter" | "Alpha" | "IsAlphabetic" | "IsLetter" => {
                                    "Alphabetic"
                                }
                                "javaLowerCase" | "Lower" | "IsLowercase" => "Lowercase",
                                "javaUpperCase" | "Upper" | "IsUppercase" => "Uppercase",
                                "Punct" | "IsPunctuation" => "P",
                                n if n.starts_with("Is") || n.starts_with("In") => &n[2..],
                                n => n,
                            };
                            (format!("\\p{{{prop}}}"), format!("\\P{{{prop}}}"))
                        }
                    };
                    out.push_str(if kind == 'p' { &pos } else { &neg });
                }
                Some(d) => {
                    out.push('\\');
                    out.push(d);
                }
                None => out.push('\\'),
            }
        } else if c == '(' && it.peek() == Some(&'?') {
            out.push('(');
            out.push(it.next().unwrap_or('?'));
            // (?<name> → (?P<name>, but keep lookbehind (?<= / (?<! as-is
            if it.peek() == Some(&'<') {
                let mut look = it.clone();
                look.next();
                if look.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
                    out.push('P');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn compile(src: &str, flags: i32) -> Result<fancy_regex::Regex, VmExit> {
    compile_with_anchor(src, flags, Anchor::None)
}

fn compile_with_anchor(
    src: &str,
    flags: i32,
    anchor: Anchor,
) -> Result<fancy_regex::Regex, VmExit> {
    if src.len() > 64 * 1024 {
        return Err(exc(
            "Ljava/util/regex/PatternSyntaxException;",
            "pattern exceeds the 64 KiB host compilation limit",
        ));
    }
    let key = (src.to_string(), flags, anchor as u8);
    if let Some(r) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(r);
    }
    if anchor != Anchor::None {
        // Validate raw pattern syntax first so the newline fallback below only
        // triggers when a valid `(?x)` trailing `#` comment swallowed `)`.
        compile_with_anchor(src, flags, Anchor::None)?;
    }
    let t = translate(src, flags);
    let mut modes = String::new();
    for (bit, mode) in [(2, 'i'), (4, 'x'), (8, 'm'), (32, 's')] {
        if flags & bit != 0 {
            modes.push(mode);
        }
    }
    let body = if modes.is_empty() {
        t
    } else {
        format!("(?{modes}){t}")
    };
    let build = |pat: &str| -> Result<fancy_regex::Regex, Box<fancy_regex::Error>> {
        fancy_regex::RegexBuilder::new(pat)
            .backtrack_limit(100_000)
            .delegate_size_limit(2 << 20)
            .delegate_dfa_size_limit(2 << 20)
            .build()
            .map_err(Box::new)
    };
    let r = match anchor {
        Anchor::None => build(&body),
        Anchor::Start => {
            build(&format!(r"\A(?:{body})")).or_else(|_| build(&format!("\\A(?:{body}\n)")))
        }
        Anchor::Full => {
            build(&format!(r"\A(?:{body})\z")).or_else(|_| build(&format!("\\A(?:{body}\n)\\z")))
        }
    }
    .map_err(|e| {
        exc(
            "Ljava/util/regex/PatternSyntaxException;",
            format!("{src}: not supported by RustyDroid's regex engine ({e})"),
        )
    })?;
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        // Bound retained native compilation state across transient Java Patterns.
        if c.len() >= 128 {
            c.clear();
        }
        c.insert(key, r.clone());
    });
    Ok(r)
}

/// Fallible matching must never become a false match/miss on budget exhaustion.
pub(crate) fn runtime_error(e: fancy_regex::Error) -> VmExit {
    crate::err::with_suggestion(
        crate::err::vm_error(
            "REGEX_RUNTIME",
            format!("bounded regex execution failed: {e}"),
        ),
        "simplify the expression/input; backtracking is capped at 100000 steps",
    )
    .into()
}

fn char_seq(vm: &mut Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    match v {
        Some(Value::Obj(r)) => match vm.heap.as_str(*r) {
            Ok(s) => Ok(Some(s.to_string())),
            Err(_) => Ok(Some(crate::intrinsics::object_to_string(vm, *r)?)),
        },
        _ => Ok(None),
    }
}

fn req_char_seq(vm: &mut Engine, v: Option<&Value>, ctx: &str) -> Result<String, VmExit> {
    char_seq(vm, v)?.ok_or_else(|| exc("Ljava/lang/NullPointerException;", ctx))
}

fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}

fn u16_of(s: &str, byte: usize) -> i32 {
    s[..byte.min(s.len())].encode_utf16().count() as i32
}
fn byte_of(s: &str, u16i: i32) -> usize {
    let mut n = 0i32;
    for (b, ch) in s.char_indices() {
        if n >= u16i {
            return b;
        }
        n += ch.len_utf16() as i32;
    }
    s.len()
}

fn pattern_of(vm: &Engine, p: ObjRef) -> (String, i32) {
    let src = match vm.heap.get_field(p, "rd_regex") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).map(str::to_string).unwrap_or_default(),
        _ => String::new(),
    };
    let flags = vm
        .heap
        .get_field(p, "rd_flags")
        .ok()
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0);
    (src, flags)
}

fn new_pattern(vm: &mut Engine, src: String, flags: i32) -> Result<Value, VmExit> {
    compile(&src, flags)?;
    let s = jstr(vm, src)?;
    let r = vm
        .heap
        .alloc_instance(
            PATTERN.to_string(),
            vec![
                ("rd_regex".to_string(), s),
                ("rd_flags".to_string(), Value::Int(flags)),
            ],
        )
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

fn new_matcher(vm: &mut Engine, pattern: ObjRef, input: String) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(MATCHER.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    MATCHERS.with(|m| {
        m.borrow_mut().insert(
            r,
            MState {
                pattern,
                input,
                ..Default::default()
            },
        )
    });
    Ok(Value::Obj(r))
}

fn new_match_result(vm: &mut Engine, st: &MState) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(MATCH_RESULT.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    let snap = MState {
        pattern: st.pattern,
        input: st.input.clone(),
        groups: st.groups.clone(),
        next: 0,
        append: 0,
        matched: st.matched,
        hit_end: st.hit_end,
    };
    MATCHERS.with(|m| {
        m.borrow_mut().insert(r, snap);
    });
    Ok(Value::Obj(r))
}

/// Java replacement string (`$1`, `${name}`, `\$`) expanded for a match.
fn expand(
    re: &fancy_regex::Regex,
    rep: &str,
    caps: &fancy_regex::Captures<'_>,
) -> Result<String, VmExit> {
    let mut out = String::new();
    let mut it = rep.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                let Some(n) = it.next() else {
                    return Err(exc(
                        "Ljava/lang/IllegalArgumentException;",
                        "character to be escaped is missing",
                    ));
                };
                out.push(n);
            }
            '$' => {
                if it.peek() == Some(&'{') {
                    it.next();
                    let mut name = String::new();
                    let mut closed = false;
                    for d in it.by_ref() {
                        if d == '}' {
                            closed = true;
                            break;
                        }
                        name.push(d);
                    }
                    if !closed {
                        return Err(exc(
                            "Ljava/lang/IllegalArgumentException;",
                            "named capturing group is missing trailing '}'",
                        ));
                    }
                    if name.is_empty() {
                        return Err(exc(
                            "Ljava/lang/IllegalArgumentException;",
                            "named capturing group has 0 length name",
                        ));
                    }
                    let Some(idx) = re.capture_names().position(|c| c == Some(name.as_str()))
                    else {
                        return Err(exc(
                            "Ljava/lang/IllegalArgumentException;",
                            format!("No group with name <{name}>"),
                        ));
                    };
                    out.push_str(caps.get(idx).map(|m| m.as_str()).unwrap_or(""));
                } else {
                    let mut num = String::new();
                    let total_groups = re.captures_len();
                    while let Some(d) = it.peek().filter(|d| d.is_ascii_digit()) {
                        // Java takes the longest group number that exists.
                        let cand = format!("{num}{d}");
                        if num.is_empty()
                            || cand
                                .parse::<usize>()
                                .map(|g| g < total_groups)
                                .unwrap_or(false)
                        {
                            num.push(*d);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    let g: usize = num.parse().map_err(|_| {
                        exc(
                            "Ljava/lang/IllegalArgumentException;",
                            "Illegal group reference",
                        )
                    })?;
                    if g >= total_groups {
                        return Err(exc(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("No group {g}"),
                        ));
                    }
                    out.push_str(caps.get(g).map(|m| m.as_str()).unwrap_or(""));
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

pub(crate) fn replace(
    re: &fancy_regex::Regex,
    input: &str,
    rep: &str,
    all: bool,
) -> Result<String, VmExit> {
    let mut out = String::new();
    let mut last = 0;
    for caps in re.captures_iter(input) {
        let caps = caps.map_err(runtime_error)?;
        let Some(m) = caps.get(0) else {
            continue;
        };
        out.push_str(&input[last..m.start()]);
        out.push_str(&expand(re, rep, &caps)?);
        last = m.end();
        if !all {
            break;
        }
    }
    out.push_str(&input[last..]);
    Ok(out)
}

/// Java `Pattern.split` semantics.
pub(crate) fn split(
    re: &fancy_regex::Regex,
    input: &str,
    limit: i32,
) -> Result<Vec<String>, VmExit> {
    if input.is_empty() {
        return Ok(vec![String::new()]);
    }
    let mut parts = Vec::new();
    let mut last = 0;
    for m in re.find_iter(input) {
        let m = m.map_err(runtime_error)?;
        if limit > 0 && parts.len() as i32 == limit - 1 {
            break;
        }
        if m.end() == 0 {
            // zero-width match at the start never yields a leading empty part
            continue;
        }
        if m.start() == m.end() && m.start() == last && last != 0 && m.start() >= input.len() {
            continue;
        }
        parts.push(input[last..m.start()].to_string());
        last = m.end();
    }
    parts.push(input[last..].to_string());
    if limit == 0 {
        while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
            parts.pop();
        }
        if parts.len() == 1 && parts[0].is_empty() {
            return Ok(vec![String::new()]);
        }
    }
    Ok(parts)
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != PATTERN {
        return Ok(None);
    }
    Ok(Some(match name {
        "compile" => {
            let src = req_char_seq(vm, args.first(), "Pattern.compile(null)")?;
            let flags = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
            new_pattern(vm, src, flags)?
        }
        "quote" => {
            let s = req_char_seq(vm, args.first(), "Pattern.quote(null)")?;
            jstr(vm, format!("\\Q{}\\E", s.replace("\\E", "\\E\\\\E\\Q")))?
        }
        "matches" => {
            let src = req_char_seq(vm, args.first(), "Pattern.matches(null, input)")?;
            let input = req_char_seq(vm, args.get(1), "Pattern.matches(regex, null)")?;
            Value::Int(anchored_full(&src, 0, &input, 0)?.is_some() as i32)
        }
        _ => return Ok(None),
    }))
}

type GroupSpans = Vec<Option<(usize, usize)>>;

fn anchored_full(
    src: &str,
    flags: i32,
    input: &str,
    from: usize,
) -> Result<Option<GroupSpans>, VmExit> {
    // Java matches(): the whole region must match; compile directly from `(src, flags)`
    // so `Pattern.COMMENTS` (`#` without trailing `\n`) and patterns near 64 KiB work.
    let full = compile_with_anchor(src, flags, Anchor::Full)?;
    let Some(caps) = full.captures(&input[from..]).map_err(runtime_error)? else {
        return Ok(None);
    };
    Ok(Some(
        (0..caps.len())
            .map(|i| caps.get(i).map(|m| (m.start() + from, m.end() + from)))
            .collect(),
    ))
}

fn query_match_result(
    vm: &mut Engine,
    st: &MState,
    re: &fancy_regex::Regex,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let no_match = || exc("Ljava/lang/IllegalStateException;", "No match found");
    let group_idx = |vm: &mut Engine, st: &MState, a: Option<&Value>| -> Result<usize, VmExit> {
        match a {
            None => Ok(0),
            Some(Value::Int(i)) => {
                if *i < 0 || *i as usize >= st.groups.len().max(re.captures_len()) {
                    return Err(exc(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        format!("No group {i}"),
                    ));
                }
                Ok(*i as usize)
            }
            other => {
                let n = req_char_seq(vm, other, "Group name")?;
                re.capture_names()
                    .position(|c| c == Some(n.as_str()))
                    .ok_or_else(|| {
                        exc(
                            "Ljava/lang/IllegalArgumentException;",
                            format!("No group with name <{n}>"),
                        )
                    })
            }
        }
    };
    Ok(Some(match name {
        "group" | "start" | "end" => {
            if !st.matched {
                return Err(no_match());
            }
            let g = group_idx(vm, st, args.first())?;
            let span = st.groups.get(g).copied().flatten();
            match (name, span) {
                ("group", Some((s, e))) => jstr(vm, st.input[s..e].to_string())?,
                ("group", None) => Value::Null,
                ("start", Some((s, _))) => Value::Int(u16_of(&st.input, s)),
                ("end", Some((_, e))) => Value::Int(u16_of(&st.input, e)),
                _ => Value::Int(-1),
            }
        }
        "groupCount" => Value::Int(re.captures_len() as i32 - 1),
        "hashCode" => Value::Int(recv as i32),
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
    if recv_class == PATTERN {
        let (src, flags) = pattern_of(vm, recv);
        return Ok(Some(match name {
            "matcher" => {
                let input = req_char_seq(vm, args.first(), "Pattern.matcher(null)")?;
                new_matcher(vm, recv, input)?
            }
            "pattern" | "toString" => jstr(vm, src)?,
            "flags" => Value::Int(flags),
            "split" | "splitAsStream" if name == "split" => {
                let input = req_char_seq(vm, args.first(), "Pattern.split(null)")?;
                let limit = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
                let re = compile(&src, flags)?;
                let parts = split(&re, &input, limit)?;
                let mut vals = Vec::new();
                for p in parts {
                    vals.push(jstr(vm, p)?);
                }
                let arr = vm
                    .heap
                    .alloc_array(
                        crate::heap::ElemKind::Obj,
                        vals.len(),
                        "[Ljava/lang/String;",
                    )
                    .map_err(crate::err::oom_throwable)?;
                *vm.heap
                    .array_elems_mut(arr)
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? = vals;
                Value::Obj(arr)
            }
            "hashCode" => Value::Int(recv as i32),
            _ => return Ok(None),
        }));
    }
    if recv_class == MATCH_RESULT {
        let Some(st) = MATCHERS.with(|m| m.borrow().get(&recv).cloned()) else {
            return Ok(None);
        };
        let (src, flags) = pattern_of(vm, st.pattern);
        let re = compile(&src, flags)?;
        if name == "toString" {
            let last = if st.matched {
                st.groups
                    .first()
                    .copied()
                    .flatten()
                    .map(|(s, e)| &st.input[s..e])
                    .unwrap_or("")
            } else {
                ""
            };
            return Ok(Some(jstr(
                vm,
                format!("java.util.regex.Matcher$ImmutableMatchResult[lastmatch={last}]"),
            )?));
        }
        return query_match_result(vm, &st, &re, name, recv, args);
    }
    if recv_class != MATCHER {
        return Ok(None);
    }
    let Some(mut st) = MATCHERS.with(|m| m.borrow().get(&recv).cloned()) else {
        return Ok(None);
    };
    let (src, flags) = pattern_of(vm, st.pattern);
    let re = compile(&src, flags)?;
    let no_match = || exc("Ljava/lang/IllegalStateException;", "No match found");
    let set_caps = |st: &mut MState, caps: Option<fancy_regex::Captures<'_>>| match caps {
        Some(c) => {
            st.groups = (0..c.len())
                .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                .collect();
            let (s, e) = st.groups[0].unwrap_or((0, 0));
            st.next = if e == s { next_char(&st.input, e) } else { e };
            st.matched = true;
            st.hit_end = e == st.input.len();
        }
        None => {
            st.groups.clear();
            st.matched = false;
            st.hit_end = true;
        }
    };
    if matches!(name, "group" | "start" | "end" | "groupCount") {
        return query_match_result(vm, &st, &re, name, recv, args);
    }
    let out = match name {
        "find" => {
            if let Some(Value::Int(i)) = args.first() {
                if *i < 0 || *i > st.input.encode_utf16().count() as i32 {
                    return Err(exc(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        format!("Illegal start index {i}"),
                    ));
                }
                st.next = byte_of(&st.input, *i);
                st.append = 0;
            }
            if st.next > st.input.len() {
                set_caps(&mut st, None);
            } else {
                let input = st.input.clone();
                let caps = re
                    .captures_from_pos(&input, st.next)
                    .map_err(runtime_error)?;
                set_caps(&mut st, caps);
            }
            Value::Int(st.matched as i32)
        }
        "matches" | "lookingAt" => {
            let input = st.input.clone();
            let groups = if name == "matches" {
                anchored_full(&src, flags, &input, 0)?
            } else {
                let anchored = compile_with_anchor(&src, flags, Anchor::Start)?;
                anchored.captures(&input).map_err(runtime_error)?.map(|c| {
                    (0..c.len())
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect()
                })
            };
            match groups {
                Some(g) => {
                    st.next = g[0].map(|x| x.1).unwrap_or(0);
                    st.groups = g;
                    st.matched = true;
                }
                None => {
                    st.groups.clear();
                    st.matched = false;
                }
            }
            Value::Int(st.matched as i32)
        }
        "hitEnd" => Value::Int(st.hit_end as i32),
        "replaceAll" | "replaceFirst" => {
            let rep = req_char_seq(vm, args.first(), "Matcher.replace(null)")?;
            let r = replace(&re, &st.input, &rep, name == "replaceAll")?;
            st.next = 0;
            st.matched = false;
            jstr(vm, r)?
        }
        "reset" => {
            if let Some(a) = args.first() {
                st.input = req_char_seq(vm, Some(a), "Matcher.reset(null)")?;
            }
            st.next = 0;
            st.append = 0;
            st.matched = false;
            st.groups.clear();
            Value::Obj(recv)
        }
        "pattern" => Value::Obj(st.pattern),
        "usePattern" => {
            let Some(Value::Obj(p)) = args.first() else {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "Pattern cannot be null",
                ));
            };
            st.pattern = *p;
            st.groups.clear();
            st.matched = false;
            Value::Obj(recv)
        }
        "regionStart" => Value::Int(0),
        "regionEnd" => Value::Int(st.input.encode_utf16().count() as i32),
        "appendReplacement" | "appendTail" => {
            let Some(Value::Obj(sb)) = args.first() else {
                return Err(exc(
                    "Ljava/lang/NullPointerException;",
                    "appendReplacement(null)",
                ));
            };
            let text = if name == "appendTail" {
                st.input[st.append..].to_string()
            } else {
                let rep = req_char_seq(vm, args.get(1), "appendReplacement(sb, null)")?;
                if !st.matched {
                    return Err(no_match());
                }
                let (s, e) = st.groups[0].unwrap_or((0, 0));
                let input = st.input.clone();
                let caps = re
                    .captures_from_pos(&input, s)
                    .map_err(runtime_error)?
                    .ok_or_else(no_match)?;
                let t = format!("{}{}", &input[st.append..s], expand(&re, &rep, &caps)?);
                st.append = e;
                t
            };
            let tv = jstr(vm, text)?;
            let cls = vm
                .heap
                .class_of(*sb)
                .unwrap_or("Ljava/lang/StringBuilder;")
                .to_string();
            let sig = format!("(Ljava/lang/String;){cls}");
            vm.invoke_instance(*sb, "append", &sig, &[tv])?;
            if name == "appendTail" {
                Value::Obj(*sb)
            } else {
                Value::Obj(recv)
            }
        }
        "toMatchResult" => new_match_result(vm, &st)?,
        "hashCode" => Value::Int(recv as i32),
        "toString" => jstr(
            vm,
            format!(
                "java.util.regex.Matcher[pattern={src} region=0,{} lastmatch=]",
                st.input.encode_utf16().count()
            ),
        )?,
        _ => return Ok(None),
    };
    MATCHERS.with(|m| m.borrow_mut().insert(recv, st));
    Ok(Some(out))
}

fn next_char(s: &str, b: usize) -> usize {
    s[b.min(s.len())..]
        .chars()
        .next()
        .map(|c| b + c.len_utf8())
        .unwrap_or(s.len() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translates_java_syntax() {
        assert_eq!(translate(r"\Qa.b\E+", 0), r"a\.b+");
        assert_eq!(translate(r"(?<num>\d+)", 0), r"(?P<num>\d+)");
        assert_eq!(translate(r"\p{javaWhitespace}", 0), r"\s");
        assert_eq!(translate("a.b", 16), r"a\.b");
    }
    #[test]
    fn java_split_semantics() {
        let re = compile(",", 0).unwrap();
        assert_eq!(split(&re, "a,b,,", 0).unwrap(), vec!["a", "b"]);
        assert_eq!(split(&re, "a,b,,", -1).unwrap(), vec!["a", "b", "", ""]);
        assert_eq!(split(&re, "a,b,c", 2).unwrap(), vec!["a", "b,c"]);
        assert_eq!(split(&re, "", 0).unwrap(), vec![""]);
    }
    #[test]
    fn replacement_groups() {
        let re = compile(r"(\d+)-(\d+)", 0).unwrap();
        assert_eq!(replace(&re, "1-2 3-4", "$2:$1", true).unwrap(), "2:1 4:3");
        assert_eq!(replace(&re, "1-2 3-4", r"\$", false).unwrap(), "$ 3-4");
    }
    #[test]
    fn utf16_offsets() {
        let s = "aé😀b";
        assert_eq!(u16_of(s, s.find('b').unwrap()), 4);
        assert_eq!(byte_of(s, 4), s.find('b').unwrap());
    }
}
