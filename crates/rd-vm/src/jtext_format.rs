//! `java.text.DecimalFormat` / `DecimalFormatSymbols` / `NumberFormat` and
//! `android.content.ComponentName`.
//!
//! **DecimalFormat** state lives in heap fields of the host object:
//! - Pattern grammar (positive subpattern): literal prefix/suffix (with
//!   `'quoted'` text), `#`/`0` integer digits, `,` grouping (size = digits
//!   after the last comma), `.` and `0`/`#` fraction digits, and `%`/`‰`
//!   (×100/×1000). An optional `;negative` subpattern supplies the negative
//!   prefix/suffix.
//! - Rounding is HALF_EVEN, as in the JDK, applied to the double's exact
//!   value. `setRoundingMode` accepts HALF_UP, DOWN, FLOOR, CEILING and UP.
//! - Separators come from DecimalFormatSymbols: the locale default (`,`
//!   decimal for pt/de/fr/es/it/nl/ru/tr…, else `.`), overridable with
//!   `setDecimalSeparator`/`setGroupingSeparator`.
//! - Methods: `format(Object|double|long)`, `parse`,
//!   `set/getMaximum/MinimumFractionDigits`, `setMinimumIntegerDigits`,
//!   `setGroupingUsed`, `applyPattern`, `toPattern`, `set/getDecimalFormatSymbols`,
//!   `setRoundingMode`, `setParseBigDecimal` (accepted, no-op).
//! - `NumberFormat.getInstance`/`getNumberInstance`/`getIntegerInstance`/
//!   `getPercentInstance`/`getCurrencyInstance([Locale])` return
//!   DecimalFormats with the JDK default patterns for the locale.
//!
//! **Charset**: `Charset.forName` (validated against the charsets the
//! String codecs implement; others → `UnsupportedCharsetException`),
//! `defaultCharset` (UTF-8), `isSupported`, `name`/`displayName`/`toString`,
//! and the `StandardCharsets` constants.
//!
//! **ComponentName**: `(String,String)`, `(Context,String)`,
//! `(Context,Class)`, `getPackageName`, `getClassName`,
//! `getShortClassName`, `flattenToString`, `flattenToShortString`,
//! `unflattenFromString`, `toShortString`, `equals`, `hashCode`, `toString`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const DF: &str = "Ljava/text/DecimalFormat;";
const DFS: &str = "Ljava/text/DecimalFormatSymbols;";
const NF: &str = "Ljava/text/NumberFormat;";
const CN: &str = "Landroid/content/ComponentName;";
const CHARSET: &str = "Ljava/nio/charset/Charset;";

/// Canonical charset name, if the String codecs implement it.
fn canonical_charset(n: &str) -> Option<&'static str> {
    Some(match n.to_ascii_uppercase().replace('_', "-").as_str() {
        "UTF-8" | "UTF8" => "UTF-8",
        "UTF-16" | "UTF16" => "UTF-16",
        "UTF-16BE" => "UTF-16BE",
        "UTF-16LE" => "UTF-16LE",
        "ISO-8859-1" | "ISO8859-1" | "LATIN1" | "ISO-LATIN-1" => "ISO-8859-1",
        "US-ASCII" | "ASCII" => "US-ASCII",
        _ => return None,
    })
}

fn charset_obj(vm: &mut Engine, name: &str) -> Result<Value, VmExit> {
    let n = jstr(vm, name.to_string())?;
    let r = vm
        .heap
        .alloc_instance(CHARSET.to_string(), vec![("rd_name".to_string(), n)])
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

/// `StandardCharsets.*` constants (sget of a host class).
pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    if owner != "Ljava/nio/charset/StandardCharsets;" {
        return Ok(None);
    }
    let Some(name) = canonical_charset(&field.replace('_', "-")) else {
        return Ok(None);
    };
    Ok(Some(charset_obj(vm, name)?))
}

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}
fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}
fn sval(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}
fn field_str(vm: &Engine, r: ObjRef, f: &str) -> Option<String> {
    let v = vm.heap.get_field(r, f).ok()?;
    sval(vm, Some(&v))
}
fn field_int(vm: &Engine, r: ObjRef, f: &str, d: i32) -> i32 {
    vm.heap
        .get_field(r, f)
        .ok()
        .and_then(|v| v.as_int().ok())
        .unwrap_or(d)
}
fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
fn put_str(vm: &mut Engine, r: ObjRef, f: &str, s: String) -> Result<(), VmExit> {
    let v = jstr(vm, s)?;
    put(vm, r, f, v)
}

/// Parsed DecimalFormat pattern.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pat {
    pub pos_prefix: String,
    pub pos_suffix: String,
    pub neg_prefix: Option<String>,
    pub neg_suffix: Option<String>,
    pub min_int: i32,
    pub min_frac: i32,
    pub max_frac: i32,
    pub grouping: i32,
    pub multiplier: i32,
}

/// JDK `DecimalFormat.DOUBLE_INTEGER_DIGITS` / `DOUBLE_FRACTION_DIGITS`:
/// tetos que `set{Minimum,Maximum}{Integer,Fraction}Digits` aplicam
/// silenciosamente. Sem eles, `setMinimumIntegerDigits(MAX_VALUE)`
/// fazia `format` tentar montar uma String de ~2,1e9 dígitos —
/// hang O(N²) + OOM no host, fora do `heap_budget` (issue #295).
const MAX_INT_DIGITS: i32 = 309;
const MAX_FRAC_DIGITS: i32 = 340;

fn clamp_int_digits(n: i32) -> i32 {
    n.clamp(0, MAX_INT_DIGITS)
}

fn clamp_frac_digits(n: i32) -> i32 {
    n.clamp(0, MAX_FRAC_DIGITS)
}

fn split_affixes(p: &str) -> (String, String, String) {
    // (prefix, number part, suffix); quotes make literal text.
    let chars: Vec<char> = p.chars().collect();
    let is_num = |c: char| matches!(c, '#' | '0'..='9' | ',' | '.' | 'E');
    let mut i = 0;
    let mut prefix = String::new();
    let mut in_q = false;
    while i < chars.len() && (in_q || !is_num(chars[i])) {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                prefix.push('\'');
                i += 2;
                continue;
            }
            in_q = !in_q;
        } else {
            prefix.push(chars[i]);
        }
        i += 1;
    }
    let start = i;
    while i < chars.len() && is_num(chars[i]) {
        i += 1;
    }
    let num: String = chars[start..i].iter().collect();
    let mut suffix = String::new();
    in_q = false;
    while i < chars.len() {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                suffix.push('\'');
                i += 2;
                continue;
            }
            in_q = !in_q;
        } else {
            suffix.push(chars[i]);
        }
        i += 1;
    }
    (prefix, num, suffix)
}

pub(crate) fn parse_pattern(p: &str) -> Pat {
    let (pos, neg) = match p.split_once(';') {
        Some((a, b)) => (a, Some(b)),
        None => (p, None),
    };
    let (pre, num, suf) = split_affixes(pos);
    let (int_part, frac_part) = num.split_once('.').unwrap_or((&num, ""));
    let min_int = int_part.chars().filter(|c| c.is_ascii_digit()).count() as i32;
    let grouping = int_part
        .rfind(',')
        .map(|k| (int_part.len() - k - 1) as i32)
        .unwrap_or(0);
    let min_frac = frac_part.chars().filter(|c| c.is_ascii_digit()).count() as i32;
    let max_frac = frac_part
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '#')
        .count() as i32;
    let multiplier = if pre.contains('%') || suf.contains('%') {
        100
    } else if pre.contains('‰') || suf.contains('‰') {
        1000
    } else {
        1
    };
    let (neg_prefix, neg_suffix) = match neg {
        Some(n) => {
            let (a, _, b) = split_affixes(n);
            (Some(a), Some(b))
        }
        None => (None, None),
    };
    Pat {
        pos_prefix: pre,
        pos_suffix: suf,
        neg_prefix,
        neg_suffix,
        min_int,
        min_frac,
        max_frac,
        grouping,
        multiplier,
    }
}

/// Rounds `x` to `digits` fraction digits with a JDK RoundingMode name.
fn round(x: f64, digits: i32, mode: &str) -> String {
    let digits = digits.clamp(0, 17) as usize;
    if mode == "HALF_EVEN" {
        // Rust's formatter rounds the exact binary value half-to-even.
        return format!("{:.*}", digits, x);
    }
    let scale = 10f64.powi(digits as i32);
    let v = x * scale;
    let r = match mode {
        "HALF_UP" => {
            let f = v.abs().floor();
            let up = v.abs() - f >= 0.5 - 1e-9;
            (f + up as i32 as f64).copysign(v)
        }
        "HALF_DOWN" => {
            let f = v.abs().floor();
            let up = v.abs() - f > 0.5 + 1e-9;
            (f + up as i32 as f64).copysign(v)
        }
        "DOWN" => v.trunc(),
        "UP" => {
            let t = v.trunc();
            if (v - t).abs() > 1e-9 {
                t + v.signum()
            } else {
                t
            }
        }
        "FLOOR" => v.floor(),
        "CEILING" => v.ceil(),
        _ => v.round(),
    };
    format!("{:.*}", digits, r / scale)
}

pub(crate) struct Fmt {
    pub pat: Pat,
    pub dec: char,
    pub group: char,
    pub grouping_used: bool,
    pub rounding: String,
}

pub(crate) fn format_number(f: &Fmt, x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    let neg = x < 0.0;
    let v = x.abs() * f.pat.multiplier as f64;
    if v.is_infinite() {
        return format!("{}∞{}", if neg { "-" } else { "" }, "");
    }
    let s = round(v, f.pat.max_frac, &f.rounding);
    let (ip, fp) = s.split_once('.').unwrap_or((&s, ""));
    assemble_number(f, neg, ip, fp)
}

/// Núcleo EXATO de `NumberFormat.format(long)` (issue #295 item 2):
/// `l as f64` corrompe `|l| > 2^53` (`Long.MAX_VALUE` virava
/// `…808`). O produto `long × multiplier` é inteiro (`i128`), logo
/// os dígitos são exatos e a fração é só zeros (aparados até
/// `min_frac` em `assemble_number`, como no caminho `double`).
pub(crate) fn format_long_exact(f: &Fmt, x: i64) -> String {
    let v = x as i128 * f.pat.multiplier as i128;
    let neg = v < 0;
    let ip = v.unsigned_abs().to_string();
    let fp = "0".repeat(f.pat.max_frac.max(0) as usize);
    assemble_number(f, neg, &ip, &fp)
}

/// Monta `afixos + parte inteira (pad/agrupamento) + fração` a
/// partir de dígitos crus — compartilhado pelos caminhos `f64`
/// (`format_number`, já arredondado) e `long` (`format_long_exact`).
fn assemble_number(f: &Fmt, neg: bool, ip: &str, fp: &str) -> String {
    let mut fp = fp.to_string();
    while fp.len() as i32 > f.pat.min_frac && fp.ends_with('0') {
        fp.pop();
    }
    let mut ip = ip.trim_start_matches('0').to_string();
    // Padding linear (`"0".repeat`): o laço anterior com `insert(0, …)`
    // deslocava a String inteira a cada dígito — O(N²) (issue #295).
    let min_int = f.pat.min_int.max(if fp.is_empty() && f.pat.min_int == 0 {
        1
    } else {
        0
    });
    if (ip.len() as i32) < min_int {
        ip = format!("{}{ip}", "0".repeat(min_int as usize - ip.len()));
    }
    if ip.is_empty() && fp.is_empty() {
        ip.push('0');
    }
    if f.grouping_used && f.pat.grouping > 0 {
        let g = f.pat.grouping as usize;
        let digits: Vec<char> = ip.chars().collect();
        let mut out = String::new();
        for (k, c) in digits.iter().enumerate() {
            if k > 0 && (digits.len() - k) % g == 0 {
                out.push(f.group);
            }
            out.push(*c);
        }
        ip = out;
    }
    let body = if fp.is_empty() {
        ip
    } else {
        format!("{ip}{}{fp}", f.dec)
    };
    let is_zero = body.chars().all(|c| !c.is_ascii_digit() || c == '0');
    if neg && !is_zero {
        let pre = f
            .pat
            .neg_prefix
            .clone()
            .unwrap_or_else(|| format!("-{}", f.pat.pos_prefix));
        let suf = f
            .pat
            .neg_suffix
            .clone()
            .unwrap_or_else(|| f.pat.pos_suffix.clone());
        format!("{pre}{body}{suf}")
    } else {
        format!("{}{body}{}", f.pat.pos_prefix, f.pat.pos_suffix)
    }
}

/// Núcleo puro do `DecimalFormat.parse` (issue #295): reconhece os
/// afixos NEGATIVOS efetivos — os mesmos que `format_number` emite
/// (`neg_prefix ?? "-{pos_prefix}"`, `neg_suffix ?? pos_suffix`) —
/// além dos positivos. Sem isso, `parse("(1.5)")` (subpattern
/// negativo) e `parse("-$9.90")` (moeda) falhavam no round-trip.
fn parse_number(f: &Fmt, s: &str) -> Option<f64> {
    let t = s.trim();
    let neg_pre = f
        .pat
        .neg_prefix
        .clone()
        .unwrap_or_else(|| format!("-{}", f.pat.pos_prefix));
    let neg_suf = f
        .pat
        .neg_suffix
        .clone()
        .unwrap_or_else(|| f.pat.pos_suffix.clone());
    let (t, neg) = if !neg_pre.is_empty() && t.starts_with(neg_pre.as_str()) {
        let t = t.strip_prefix(neg_pre.as_str()).unwrap_or(t);
        let t = t.strip_suffix(neg_suf.as_str()).unwrap_or(t);
        (t, true)
    } else {
        let t = t.strip_prefix(f.pat.pos_prefix.as_str()).unwrap_or(t);
        let t = t.strip_suffix(f.pat.pos_suffix.as_str()).unwrap_or(t);
        (t, false)
    };
    let num: String = t
        .chars()
        .filter(|c| *c != f.group)
        .map(|c| if c == f.dec { '.' } else { c })
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let x: f64 = num.parse().ok()?;
    let x = if neg { -x.abs() } else { x };
    Some(x / f.pat.multiplier as f64)
}

fn locale_tag(vm: &mut Engine, v: Option<&Value>) -> String {
    match v {
        Some(Value::Obj(r)) => crate::intrinsics::object_to_string(vm, *r).unwrap_or_default(),
        _ => "en_US".into(),
    }
}
fn locale_seps(tag: &str) -> (char, char) {
    let lang = tag
        .split(['_', '-'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lang.as_str() {
        "pt" | "de" | "es" | "it" | "nl" | "id" | "tr" | "da" | "el" | "ro" | "hr" | "sl"
        | "sr" => (',', '.'),
        "fr" | "ru" | "pl" | "cs" | "sk" | "uk" | "fi" | "sv" | "nb" | "no" | "hu" | "bg" => {
            (',', '\u{a0}')
        }
        _ => ('.', ','),
    }
}

fn new_symbols(vm: &mut Engine, tag: &str) -> Result<ObjRef, VmExit> {
    let (d, g) = locale_seps(tag);
    let r = vm
        .heap
        .alloc_instance(DFS.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    put(vm, r, "rd_dec", Value::Int(d as i32))?;
    put(vm, r, "rd_group", Value::Int(g as i32))?;
    Ok(r)
}

fn init_df(vm: &mut Engine, r: ObjRef, pattern: &str, symbols: ObjRef) -> Result<(), VmExit> {
    put_str(vm, r, "rd_pattern", pattern.to_string())?;
    put(vm, r, "rd_symbols", Value::Obj(symbols))?;
    let p = parse_pattern(pattern);
    put(vm, r, "rd_min_frac", Value::Int(p.min_frac))?;
    put(vm, r, "rd_max_frac", Value::Int(p.max_frac))?;
    put(vm, r, "rd_min_int", Value::Int(p.min_int))?;
    put(vm, r, "rd_grouping_used", Value::Int(1))?;
    Ok(())
}

fn new_df(vm: &mut Engine, pattern: &str, tag: &str) -> Result<Value, VmExit> {
    let s = new_symbols(vm, tag)?;
    let r = vm
        .heap
        .alloc_instance(DF.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    init_df(vm, r, pattern, s)?;
    Ok(Value::Obj(r))
}

fn fmt_of(vm: &Engine, r: ObjRef) -> Fmt {
    let mut pat =
        parse_pattern(&field_str(vm, r, "rd_pattern").unwrap_or_else(|| "#,##0.###".into()));
    // Defesa em profundidade (issue #295): mesmo que um campo tenha
    // sido gravado por um caminho antigo, os tetos do JDK valem aqui.
    pat.min_frac = clamp_frac_digits(field_int(vm, r, "rd_min_frac", pat.min_frac));
    pat.max_frac =
        clamp_frac_digits(field_int(vm, r, "rd_max_frac", pat.max_frac)).max(pat.min_frac);
    pat.min_int = clamp_int_digits(field_int(vm, r, "rd_min_int", pat.min_int));
    let (dec, group) = match vm.heap.get_field(r, "rd_symbols") {
        Ok(Value::Obj(s)) => (
            char::from_u32(field_int(vm, s, "rd_dec", '.' as i32) as u32).unwrap_or('.'),
            char::from_u32(field_int(vm, s, "rd_group", ',' as i32) as u32).unwrap_or(','),
        ),
        _ => ('.', ','),
    };
    Fmt {
        pat,
        dec,
        group,
        grouping_used: field_int(vm, r, "rd_grouping_used", 1) != 0,
        rounding: field_str(vm, r, "rd_rounding").unwrap_or_else(|| "HALF_EVEN".into()),
    }
}

/// O argumento numérico de `format`, PRESERVANDO a distinção do
/// JDK entre `format(long)` (exato) e `format(double)` — issue
/// #295 item 2: converter `Long` para `f64` corrompia `|l| > 2^53`.
enum NumVal {
    Long(i64),
    Double(f64),
}

fn number_of(vm: &mut Engine, v: Option<&Value>) -> Result<NumVal, VmExit> {
    let not_a_number = || {
        exc(
            "Ljava/lang/IllegalArgumentException;",
            "Cannot format given Object as a Number",
        )
    };
    Ok(match v {
        Some(Value::Int(i)) => NumVal::Long(*i as i64),
        Some(Value::Long(l)) => NumVal::Long(*l),
        Some(Value::Float(f)) => NumVal::Double(*f as f64),
        Some(Value::Double(d)) => NumVal::Double(*d),
        Some(Value::Obj(r)) => match vm.heap.get_field(*r, "value") {
            Ok(Value::Int(i)) => NumVal::Long(i as i64),
            Ok(Value::Long(l)) => NumVal::Long(l),
            Ok(Value::Float(f)) => NumVal::Double(f as f64),
            Ok(Value::Double(d)) => NumVal::Double(d),
            _ => {
                // JDK: `format(Object)` exige `Number` — uma String
                // "123" NÃO é formatável (issue #295 item 2), mesmo
                // que o seu toString parseie como número.
                let is_number = vm
                    .heap
                    .class_of(*r)
                    .is_ok_and(|cls| vm.cp.is_subtype(cls, "Ljava/lang/Number;"));
                if !is_number {
                    return Err(not_a_number());
                }
                // BigDecimal/BigInteger and other Numbers: via toString.
                let s = crate::intrinsics::object_to_string(vm, *r)?;
                NumVal::Double(s.trim().parse::<f64>().map_err(|_| not_a_number())?)
            }
        },
        _ => return Err(not_a_number()),
    })
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (NF | DF, "getInstance" | "getNumberInstance") => {
            let tag = locale_tag(vm, args.first());
            Ok(Some(new_df(vm, "#,##0.###", &tag)?))
        }
        (NF | DF, "getIntegerInstance") => {
            let tag = locale_tag(vm, args.first());
            let df = new_df(vm, "#,##0", &tag)?;
            if let Value::Obj(r) = df {
                put_str(vm, r, "rd_rounding", "HALF_EVEN".into())?;
            }
            Ok(Some(df))
        }
        (NF | DF, "getPercentInstance") => {
            let tag = locale_tag(vm, args.first());
            Ok(Some(new_df(vm, "#,##0%", &tag)?))
        }
        (NF | DF, "getCurrencyInstance") => {
            let tag = locale_tag(vm, args.first());
            let sym = if tag.contains("BR") {
                "R$\u{a0}"
            } else if tag.starts_with("en_GB") {
                "£"
            } else if tag.starts_with("en") || tag.is_empty() {
                "$"
            } else {
                "¤"
            };
            let pat = if tag.starts_with("pt") || tag.starts_with("en") || tag.is_empty() {
                format!("'{sym}'#,##0.00")
            } else {
                format!("#,##0.00\u{a0}'{sym}'")
            };
            Ok(Some(new_df(vm, &pat, &tag)?))
        }
        (DFS, "getInstance") => {
            let tag = locale_tag(vm, args.first());
            Ok(Some(Value::Obj(new_symbols(vm, &tag)?)))
        }
        (CHARSET, "forName") => {
            let n = sval(vm, args.first())
                .ok_or_else(|| exc("Ljava/lang/IllegalArgumentException;", "Null charset name"))?;
            match canonical_charset(&n) {
                Some(c) => Ok(Some(charset_obj(vm, c)?)),
                None => Err(exc("Ljava/nio/charset/UnsupportedCharsetException;", n)),
            }
        }
        (CHARSET, "defaultCharset") => Ok(Some(charset_obj(vm, "UTF-8")?)),
        (CHARSET, "isSupported") => {
            // JDK: `Charset.isSupported(null)` é IAE (issue #295).
            let n = sval(vm, args.first())
                .ok_or_else(|| exc("Ljava/lang/IllegalArgumentException;", "Null charset name"))?;
            Ok(Some(Value::Int(canonical_charset(&n).is_some() as i32)))
        }
        (CN, "unflattenFromString") => {
            let s = sval(vm, args.first()).unwrap_or_default();
            let Some((pkg, cls)) = s.split_once('/') else {
                return Ok(Some(Value::Null));
            };
            let cls = if cls.starts_with('.') {
                format!("{pkg}{cls}")
            } else {
                cls.to_string()
            };
            let r = vm
                .heap
                .alloc_instance(CN.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            put_str(vm, r, "rd_pkg", pkg.to_string())?;
            put_str(vm, r, "rd_cls", cls)?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match recv_class {
        DFS => symbols(vm, name, recv, args),
        DF => decimal_format(vm, name, sig, recv, args),
        CN => component(vm, name, recv, args),
        CHARSET => {
            let n = field_str(vm, recv, "rd_name").unwrap_or_else(|| "UTF-8".into());
            Ok(Some(match name {
                "name" | "displayName" | "toString" => jstr(vm, n)?,
                "hashCode" => Value::Int(java_hash(&n)),
                "equals" => Value::Int(
                    matches!(args.first(), Some(Value::Obj(o)) if field_str(vm, *o, "rd_name").as_deref() == Some(n.as_str()))
                        as i32,
                ),
                "canEncode" | "isRegistered" => Value::Int(1),
                "aliases" => return Ok(None),
                _ => return Ok(None),
            }))
        }
        _ => Ok(None),
    }
}

fn symbols(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let ch = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
    Ok(Some(match name {
        "<init>" => {
            let tag = locale_tag(vm, args.first());
            let (d, g) = locale_seps(&tag);
            put(vm, recv, "rd_dec", Value::Int(d as i32))?;
            put(vm, recv, "rd_group", Value::Int(g as i32))?;
            Value::Null
        }
        "setDecimalSeparator" | "setMonetaryDecimalSeparator" => {
            put(vm, recv, "rd_dec", Value::Int(ch))?;
            Value::Null
        }
        "setGroupingSeparator" | "setMonetaryGroupingSeparator" => {
            put(vm, recv, "rd_group", Value::Int(ch))?;
            Value::Null
        }
        "getDecimalSeparator" | "getMonetaryDecimalSeparator" => {
            Value::Int(field_int(vm, recv, "rd_dec", '.' as i32))
        }
        "getGroupingSeparator" | "getMonetaryGroupingSeparator" => {
            Value::Int(field_int(vm, recv, "rd_group", ',' as i32))
        }
        "getMinusSign" => Value::Int('-' as i32),
        "getZeroDigit" => Value::Int('0' as i32),
        "getPercent" => Value::Int('%' as i32),
        "setMinusSign" | "setZeroDigit" | "setPercent" | "setCurrencySymbol" | "setInfinity"
        | "setNaN" => Value::Null,
        "clone" => {
            let r = vm
                .heap
                .alloc_instance(DFS.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            let (d, g) = (
                field_int(vm, recv, "rd_dec", '.' as i32),
                field_int(vm, recv, "rd_group", ',' as i32),
            );
            put(vm, r, "rd_dec", Value::Int(d))?;
            put(vm, r, "rd_group", Value::Int(g))?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}

fn decimal_format(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let n = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
    Ok(Some(match name {
        "<init>" => {
            // JDK: `new DecimalFormat((String) null)` é NPE; o ctor
            // sem argumentos usa o padrão default (issue #295). Sem
            // ler ambiente do host (determinismo): default "en_US",
            // como em `NumberFormat.getInstance()`.
            let pattern = if sig.contains("Ljava/lang/String;") {
                sval(vm, args.first())
                    .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "pattern"))?
            } else {
                "#,##0.###".into()
            };
            let symbols = match args.get(1) {
                Some(Value::Obj(s)) => *s,
                _ => new_symbols(vm, "en_US")?,
            };
            init_df(vm, recv, &pattern, symbols)?;
            Value::Null
        }
        "format" => {
            let f = fmt_of(vm, recv);
            let s = match number_of(vm, args.first())? {
                NumVal::Long(l) => format_long_exact(&f, l),
                NumVal::Double(d) => format_number(&f, d),
            };
            if sig.contains("StringBuffer") {
                // format(Object|double|long, StringBuffer, FieldPosition)
                if let Some(Value::Obj(sb)) = args.get(1) {
                    let tv = jstr(vm, s)?;
                    vm.invoke_instance(
                        *sb,
                        "append",
                        "(Ljava/lang/String;)Ljava/lang/StringBuffer;",
                        &[tv],
                    )?;
                    return Ok(Some(Value::Obj(*sb)));
                }
            }
            jstr(vm, s)?
        }
        "parse" => {
            // JDK: `parse(null)` é NullPointerException (issue #295).
            let s = sval(vm, args.first())
                .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "source"))?;
            let f = fmt_of(vm, recv);
            let Some(x) = parse_number(&f, &s) else {
                return Err(exc(
                    "Ljava/text/ParseException;",
                    format!("Unparseable number: \"{s}\""),
                ));
            };
            if x.fract() == 0.0 && x.abs() < 9.2e18 {
                Value::Obj(crate::boxes::box_value(
                    vm,
                    "Ljava/lang/Long;",
                    Value::Long(x as i64),
                )?)
            } else {
                Value::Obj(crate::boxes::box_value(
                    vm,
                    "Ljava/lang/Double;",
                    Value::Double(x),
                )?)
            }
        }
        "applyPattern" | "applyLocalizedPattern" => {
            // JDK: `applyPattern(null)` é NullPointerException (issue #295).
            let p = sval(vm, args.first())
                .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "pattern"))?;
            let s = match vm.heap.get_field(recv, "rd_symbols") {
                Ok(Value::Obj(s)) => s,
                _ => new_symbols(vm, "en_US")?,
            };
            init_df(vm, recv, &p, s)?;
            Value::Null
        }
        "toPattern" | "toLocalizedPattern" => {
            let p = field_str(vm, recv, "rd_pattern").unwrap_or_default();
            jstr(vm, p)?
        }
        "setMaximumFractionDigits" => {
            let n = clamp_frac_digits(n);
            put(vm, recv, "rd_max_frac", Value::Int(n))?;
            if field_int(vm, recv, "rd_min_frac", 0) > n {
                put(vm, recv, "rd_min_frac", Value::Int(n))?;
            }
            Value::Null
        }
        "setMinimumFractionDigits" => {
            let n = clamp_frac_digits(n);
            put(vm, recv, "rd_min_frac", Value::Int(n))?;
            if field_int(vm, recv, "rd_max_frac", 3) < n {
                put(vm, recv, "rd_max_frac", Value::Int(n))?;
            }
            Value::Null
        }
        "setMinimumIntegerDigits" => {
            put(vm, recv, "rd_min_int", Value::Int(clamp_int_digits(n)))?;
            Value::Null
        }
        "setMaximumIntegerDigits"
        | "setParseBigDecimal"
        | "setParseIntegerOnly"
        | "setCurrency"
        | "setDecimalSeparatorAlwaysShown"
        | "setPositivePrefix"
        | "setNegativePrefix"
        | "setPositiveSuffix"
        | "setNegativeSuffix"
        | "setMultiplier"
        | "setGroupingSize" => Value::Null,
        "getMaximumFractionDigits" => Value::Int(field_int(vm, recv, "rd_max_frac", 3)),
        "getMinimumFractionDigits" => Value::Int(field_int(vm, recv, "rd_min_frac", 0)),
        "getMinimumIntegerDigits" => Value::Int(field_int(vm, recv, "rd_min_int", 1)),
        "setGroupingUsed" => {
            put(vm, recv, "rd_grouping_used", Value::Int(n))?;
            Value::Null
        }
        "isGroupingUsed" => Value::Int(field_int(vm, recv, "rd_grouping_used", 1)),
        "setRoundingMode" => {
            let mode = match args.first() {
                Some(Value::Obj(r)) => crate::intrinsics::object_to_string(vm, *r)?,
                _ => "HALF_EVEN".into(),
            };
            put_str(vm, recv, "rd_rounding", mode)?;
            Value::Null
        }
        "setDecimalFormatSymbols" => {
            if let Some(v @ Value::Obj(_)) = args.first() {
                put(vm, recv, "rd_symbols", v.clone())?;
            }
            Value::Null
        }
        "getDecimalFormatSymbols" => vm.heap.get_field(recv, "rd_symbols").unwrap_or(Value::Null),
        "clone" => {
            let p = field_str(vm, recv, "rd_pattern").unwrap_or_default();
            let s = match vm.heap.get_field(recv, "rd_symbols") {
                Ok(Value::Obj(s)) => s,
                _ => new_symbols(vm, "en_US")?,
            };
            let r = vm
                .heap
                .alloc_instance(DF.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            init_df(vm, r, &p, s)?;
            for f in [
                "rd_min_frac",
                "rd_max_frac",
                "rd_min_int",
                "rd_grouping_used",
                "rd_rounding",
            ] {
                if let Ok(v) = vm.heap.get_field(recv, f) {
                    put(vm, r, f, v)?;
                }
            }
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}

fn component(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let pkg = field_str(vm, recv, "rd_pkg").unwrap_or_default();
    let cls = field_str(vm, recv, "rd_cls").unwrap_or_default();
    let short = if cls.starts_with(&format!("{pkg}.")) {
        cls[pkg.len()..].to_string()
    } else {
        cls.clone()
    };
    Ok(Some(match name {
        "<init>" => {
            // AOSP: pkg/Context null em `ComponentName.<init>` é NPE —
            // só o caso Context (objeto não-String) cai no pacote da
            // sessão (issue #295).
            if matches!(args.first(), None | Some(Value::Null)) {
                return Err(exc(
                    "Ljava/lang/NullPointerException;",
                    "package name is null",
                ));
            }
            let p = match args.first() {
                Some(Value::Obj(o)) if vm.heap.as_str(*o).is_ok() => {
                    sval(vm, args.first()).unwrap_or_default()
                }
                _ => vm.fw.package_name.clone(),
            };
            let c = match args.get(1) {
                Some(Value::Obj(o)) if vm.heap.as_str(*o).is_ok() => {
                    sval(vm, args.get(1)).unwrap_or_default()
                }
                Some(Value::Obj(o)) => {
                    let desc = field_str(vm, *o, "class_name").unwrap_or_default();
                    desc.trim_start_matches('L')
                        .trim_end_matches(';')
                        .replace('/', ".")
                }
                _ => {
                    return Err(exc(
                        "Ljava/lang/NullPointerException;",
                        "class name is null",
                    ))
                }
            };
            put_str(vm, recv, "rd_pkg", p)?;
            put_str(vm, recv, "rd_cls", c)?;
            Value::Null
        }
        "getPackageName" => jstr(vm, pkg)?,
        "getClassName" => jstr(vm, cls)?,
        "getShortClassName" => jstr(vm, short)?,
        "flattenToString" => jstr(vm, format!("{pkg}/{cls}"))?,
        "flattenToShortString" => jstr(vm, format!("{pkg}/{short}"))?,
        "toShortString" => jstr(vm, format!("{{{pkg}/{short}}}"))?,
        "toString" => jstr(vm, format!("ComponentInfo{{{pkg}/{cls}}}"))?,
        "hashCode" => Value::Int(java_hash(&pkg).wrapping_add(java_hash(&cls))),
        "equals" => {
            let same = match args.first() {
                Some(Value::Obj(o)) if vm.heap.class_of(*o).map(|c| c == CN).unwrap_or(false) => {
                    field_str(vm, *o, "rd_pkg").unwrap_or_default() == pkg
                        && field_str(vm, *o, "rd_cls").unwrap_or_default() == cls
                }
                _ => false,
            };
            Value::Int(same as i32)
        }
        "compareTo" => {
            let (op, oc) = match args.first() {
                Some(Value::Obj(o)) => (
                    field_str(vm, *o, "rd_pkg").unwrap_or_default(),
                    field_str(vm, *o, "rd_cls").unwrap_or_default(),
                ),
                _ => return Err(exc("Ljava/lang/NullPointerException;", "compareTo(null)")),
            };
            Value::Int(match pkg.cmp(&op).then(cls.cmp(&oc)) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        _ => return Ok(None),
    }))
}

fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(p: &str, dec: char, group: char) -> Fmt {
        Fmt {
            pat: parse_pattern(p),
            dec,
            group,
            grouping_used: true,
            rounding: "HALF_EVEN".into(),
        }
    }
    #[test]
    fn pt_br_money() {
        let m = f("#,##0.00", ',', '.');
        assert_eq!(format_number(&m, 1234567.891), "1.234.567,89");
        assert_eq!(format_number(&m, 0.5), "0,50");
        assert_eq!(format_number(&m, -12.0), "-12,00");
        assert_eq!(format_number(&f("#,##0.###", ',', '.'), 2.5), "2,5");
        assert_eq!(format_number(&f("#,##0.###", ',', '.'), 1000.0), "1.000");
        assert_eq!(format_number(&f("#,##0.00##", ',', '.'), 1.23456), "1,2346");
        assert_eq!(format_number(&f("#,##0", ',', '.'), 2.5), "2");
        assert_eq!(format_number(&f("#,##0", ',', '.'), 3.5), "4");
    }
    #[test]
    fn affixes_and_percent() {
        assert_eq!(format_number(&f("'R$ '#,##0.00", ',', '.'), 9.9), "R$ 9,90");
        assert_eq!(format_number(&f("#,##0%", '.', ','), 0.256), "26%");
        assert_eq!(format_number(&f("00.0", '.', ','), 3.04159), "03.0");
        assert_eq!(format_number(&f("#.##;(#.##)", '.', ','), -1.5), "(1.5)");
    }
    #[test]
    fn digit_clamps_are_jdk_bounds() {
        // Issue #295: DOUBLE_INTEGER_DIGITS / DOUBLE_FRACTION_DIGITS.
        assert_eq!(clamp_int_digits(i32::MAX), 309);
        assert_eq!(clamp_int_digits(-1), 0);
        assert_eq!(clamp_int_digits(42), 42);
        assert_eq!(clamp_frac_digits(i32::MAX), 340);
        assert_eq!(clamp_frac_digits(-1), 0);
    }
    #[test]
    fn integer_padding_is_linear_and_exact() {
        // min_int no teto do JDK: o padding antigo (insert(0) em laço)
        // era quadrático; o novo é uma repetição só, exata.
        let mut fmt = f("0", '.', ',');
        fmt.pat.min_int = MAX_INT_DIGITS;
        let s = format_number(&fmt, 7.0);
        assert_eq!(s.len(), 309);
        assert!(s.ends_with('7'));
        assert!(s[..308].chars().all(|c| c == '0'));
    }
    #[test]
    fn parse_round_trips_negatives_and_currency() {
        // Issue #295 (item 3): parse ignorava os afixos negativos.
        let paren = f("#.##;(#.##)", '.', ',');
        assert_eq!(format_number(&paren, -1.5), "(1.5)");
        assert_eq!(parse_number(&paren, "(1.5)"), Some(-1.5));
        assert_eq!(parse_number(&paren, "1.5"), Some(1.5));
        let money = f("$#,##0.00", '.', ',');
        assert_eq!(format_number(&money, -9.9), "-$9.90");
        assert_eq!(parse_number(&money, "-$9.90"), Some(-9.9));
        assert_eq!(parse_number(&money, "$9.90"), Some(9.9));
        assert_eq!(parse_number(&money, "xyz"), None);
    }
    #[test]
    fn long_format_is_exact_beyond_2pow53() {
        // Issue #295 (item 2): via f64, Long.MAX_VALUE virava …808.
        let grouped = f("#,##0", '.', ',');
        assert_eq!(
            format_long_exact(&grouped, i64::MAX),
            "9,223,372,036,854,775,807"
        );
        assert_eq!(
            format_long_exact(&grouped, (1i64 << 53) + 1),
            "9,007,199,254,740,993"
        );
        assert_eq!(
            format_long_exact(&grouped, i64::MIN),
            "-9,223,372,036,854,775,808"
        );
        // Fração mínima e multiplier (%) comportam-se como no double.
        assert_eq!(format_long_exact(&f("#,##0.00", '.', ','), 5), "5.00");
        assert_eq!(format_long_exact(&f("#,##0%", '.', ','), 1), "100%");
        assert_eq!(format_long_exact(&grouped, 0), "0");
        // Consistência com o caminho f64 onde ele é exato (|x| <= 2^53).
        assert_eq!(
            format_long_exact(&f("#,##0.##", '.', ','), -1234567),
            format_number(&f("#,##0.##", '.', ','), -1234567.0)
        );
    }
}
