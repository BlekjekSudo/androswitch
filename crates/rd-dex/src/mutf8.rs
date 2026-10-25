//! MUTF-8 (Modified UTF-8, Cesu-8-like) — o encoding das strings do DEX —
//! com decode/encode, comparação/hashing zero-alloc sobre bytes crus e o
//! escaping de literais no estilo baksmali.
//!
//! Diferenças vs UTF-8 padrão:
//! - NUL (U+0000) é codificado como `0xC0 0x80` (nunca byte zero).
//! - Não há sequências de 4 bytes: code points suplementares (U+10000+) são
//!   codificados como par de surrogates UTF-16, cada um em 3 bytes.
//!
//! Decode é *lossy* (bytes inválidos → U+FFFD) — nunca pânico, nunca erro
//! (strings malformadas não podem derrubar o parser; Lei 1).

const HASH_SEED: u64 = 0x517c_c1b7_2722_0a95;

/// Verifica em blocos de 8 bytes (SWAR) se `bytes` contém apenas ASCII `0x01..=0x7F`.
#[inline]
pub(crate) fn is_pure_ascii_non_nul(bytes: &[u8]) -> bool {
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let w = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        if (w.wrapping_sub(0x0101_0101_0101_0101) | w) & 0x8080_8080_8080_8080 != 0 {
            return false;
        }
    }
    chunks.remainder().iter().all(|&b| b.wrapping_sub(1) < 0x7F)
}

/// Hash determinístico 64-bit processando 8 bytes por iteração sobre bytes
/// MUTF-8 canônicos. Nunca retorna `0` (`0` é reservado como sentinela de
/// slot vazio em `RawClassIndex`).
#[inline]
pub fn hash_bytes_nonzero(bytes: &[u8]) -> u64 {
    let mut h = HASH_SEED ^ (bytes.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let w = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        h = (h ^ w).wrapping_mul(HASH_SEED);
        h ^= h >> 32;
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        let mut tail = 0u64;
        for (shift, &b) in rem.iter().enumerate() {
            tail |= (b as u64) << (shift * 8);
        }
        h = (h ^ tail).wrapping_mul(HASH_SEED);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    if h == 0 {
        1
    } else {
        h
    }
}

/// Decodifica um único escalar Unicode de `bytes[*i..]` em MUTF-8 lossy e
/// avança `*i`.
#[inline]
pub fn next_char(bytes: &[u8], i: &mut usize) -> char {
    let b = bytes[*i];
    if b < 0x80 {
        *i += 1;
        return b as char;
    }
    if (0xC0..=0xDF).contains(&b) {
        if let Some(&b1) = bytes.get(*i + 1) {
            if b1 & 0xC0 == 0x80 {
                let cp = (((b & 0x1F) as u32) << 6) | (b1 & 0x3F) as u32;
                *i += 2;
                return char::from_u32(cp).unwrap_or('\u{FFFD}');
            }
        }
        *i += 1;
        return '\u{FFFD}';
    }
    if (0xE0..=0xEF).contains(&b) {
        if let (Some(&b1), Some(&b2)) = (bytes.get(*i + 1), bytes.get(*i + 2)) {
            if b1 & 0xC0 == 0x80 && b2 & 0xC0 == 0x80 {
                let cp =
                    (((b & 0x0F) as u32) << 12) | (((b1 & 0x3F) as u32) << 6) | (b2 & 0x3F) as u32;
                *i += 3;
                if (0xD800..=0xDBFF).contains(&cp) {
                    if let (Some(&b3), Some(&b4), Some(&b5)) =
                        (bytes.get(*i), bytes.get(*i + 1), bytes.get(*i + 2))
                    {
                        if (0xE0..=0xEF).contains(&b3) && b4 & 0xC0 == 0x80 && b5 & 0xC0 == 0x80 {
                            let cp2 = (((b3 & 0x0F) as u32) << 12)
                                | (((b4 & 0x3F) as u32) << 6)
                                | (b5 & 0x3F) as u32;
                            if (0xDC00..=0xDFFF).contains(&cp2) {
                                let combined = 0x10000 + ((cp - 0xD800) << 10) + (cp2 - 0xDC00);
                                if let Some(c) = char::from_u32(combined) {
                                    *i += 3;
                                    return c;
                                }
                            }
                        }
                    }
                    return '\u{FFFD}';
                }
                if (0xDC00..=0xDFFF).contains(&cp) {
                    return '\u{FFFD}';
                }
                return char::from_u32(cp).unwrap_or('\u{FFFD}');
            }
        }
        *i += 1;
        return '\u{FFFD}';
    }
    *i += 1;
    '\u{FFFD}'
}

/// Decodifica bytes MUTF-8 para `String` (perda com U+FFFD em sequências inválidas).
pub fn decode(bytes: &[u8]) -> String {
    if is_pure_ascii_non_nul(bytes) {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_owned();
        }
    }
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        out.push(next_char(bytes, &mut i));
    }
    out
}

/// Compara bytes MUTF-8 crus (sem o terminador NUL final) contra `target`
/// sem alocar `String`. Produz exatamente o mesmo resultado booleano que
/// `decode(raw_mutf8) == target`.
#[inline]
pub fn eq_str(raw_mutf8: &[u8], target: &str) -> bool {
    let tb = target.as_bytes();
    if is_pure_ascii_non_nul(raw_mutf8) && is_pure_ascii_non_nul(tb) {
        return raw_mutf8 == tb;
    }
    let mut i = 0usize;
    let mut chars = target.chars();
    while i < raw_mutf8.len() {
        let ch = next_char(raw_mutf8, &mut i);
        if chars.next() != Some(ch) {
            return false;
        }
    }
    chars.next().is_none()
}

/// Hash 64-bit não-zero sobre bytes MUTF-8 crus, canônico em relação a `hash_str`:
/// para ASCII puro (>99,99% dos descritores), opera direto em `u64` (8 bytes/vez)
/// sem decodificar; para sequências não-ASCII, normaliza via MUTF-8 canônico.
#[inline]
pub fn hash_mutf8(raw_mutf8: &[u8]) -> u64 {
    if is_pure_ascii_non_nul(raw_mutf8) {
        return hash_bytes_nonzero(raw_mutf8);
    }
    // Caminho raro (descritor Unicode ou MUTF-8 malformado): normaliza para o
    // MUTF-8 canônico dos escalares decodificados para coincidir 100% com hash_str.
    let decoded = decode(raw_mutf8);
    let canonical = encode(&decoded);
    hash_bytes_nonzero(&canonical)
}

/// Hash 64-bit não-zero sobre `s`, idêntico bit a bit a `hash_mutf8` em 8 bytes/vez.
#[inline]
pub fn hash_str(s: &str) -> u64 {
    let tb = s.as_bytes();
    if is_pure_ascii_non_nul(tb) {
        return hash_bytes_nonzero(tb);
    }
    let canonical = encode(s);
    hash_bytes_nonzero(&canonical)
}

/// Codifica `s` em MUTF-8 (NUL → C0 80; suplementares → par de surrogates 3-byte).
pub fn encode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 1);
    push_mutf8(s, &mut out);
    out
}

pub fn push_mutf8(s: &str, out: &mut Vec<u8>) {
    for c in s.chars() {
        let cp = c as u32;
        if cp == 0 {
            out.push(0xC0);
            out.push(0x80);
        } else if cp < 0x80 {
            out.push(cp as u8);
        } else if cp < 0x800 {
            out.push(0xC0 | (cp >> 6) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else if cp < 0x10000 {
            out.push(0xE0 | (cp >> 12) as u8);
            out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else {
            let v = cp - 0x10000;
            let hi = 0xD800 + (v >> 10);
            let lo = 0xDC00 + (v & 0x3FF);
            for unit in [hi, lo] {
                out.push(0xE0 | (unit >> 12) as u8);
                out.push(0x80 | ((unit >> 6) & 0x3F) as u8);
                out.push(0x80 | (unit & 0x3F) as u8);
            }
        }
    }
}

/// Escaping de literal de string no estilo baksmali/dexlib2 (`StringUtils.escapeString`).
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for unit in s.encode_utf16() {
        escape_unit(unit, '"', &mut out);
    }
    out.push('"');
    out
}

/// Escaping de literal de char no estilo baksmali (aspas simples).
pub fn escape_char(c: u16) -> String {
    let mut out = String::new();
    out.push('\'');
    escape_unit(c, '\'', &mut out);
    out.push('\'');
    out
}

fn escape_unit(unit: u16, quote: char, out: &mut String) {
    if (32..127).contains(&unit) && unit as u8 as char != quote && unit != b'\\' as u16 {
        out.push(unit as u8 as char);
        return;
    }
    match unit {
        0x0A => out.push_str("\\n"),
        0x0D => out.push_str("\\r"),
        0x09 => out.push_str("\\t"),
        0x5C => out.push_str("\\\\"),
        u if u == quote as u16 => {
            out.push('\\');
            out.push(quote);
        }
        _ => out.push_str(&format!("\\u{unit:04x}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_is_c0_80() {
        assert_eq!(encode("\0"), vec![0xC0, 0x80]);
        assert_eq!(decode(&[0xC0, 0x80]), "\0");
        assert!(eq_str(&[0xC0, 0x80], "\0"));
        assert_eq!(hash_mutf8(&[0xC0, 0x80]), hash_str("\0"));
    }

    #[test]
    fn ascii_roundtrip() {
        for b in 1u8..0x80 {
            let s = (b as char).to_string();
            let enc = encode(&s);
            assert_eq!(decode(&enc), s);
            assert!(eq_str(&enc, &s));
            assert_eq!(hash_mutf8(&enc), hash_str(&s));
        }
        assert_eq!(decode(b"hello world!"), "hello world!");
    }

    #[test]
    fn bmp_roundtrip() {
        for cp in [0xE9u32, 0x4E2D, 0x2603, 0x20AC, 0xFFFD] {
            let s = match char::from_u32(cp) {
                Some(c) => c.to_string(),
                None => continue,
            };
            let enc = encode(&s);
            let want = if cp < 0x800 { 2 } else { 3 };
            assert_eq!(enc.len(), want, "cp U+{cp:04X} deve usar {want} bytes");
            assert_eq!(decode(&enc), s);
            assert!(eq_str(&enc, &s));
            assert_eq!(hash_mutf8(&enc), hash_str(&s));
        }
    }

    #[test]
    fn supplementary_is_surrogate_pair() {
        let s = "\u{1F600}";
        let enc = encode(s);
        assert_eq!(enc.len(), 6, "par de surrogates = 2 × 3 bytes");
        assert_eq!(enc, vec![0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
        assert_eq!(decode(&enc), s);
        assert!(eq_str(&enc, s));
        assert_eq!(hash_mutf8(&enc), hash_str(s));
    }

    #[test]
    fn invalid_bytes_are_lossy_not_panic() {
        assert_eq!(decode(&[0x80]), "\u{FFFD}");
        assert!(eq_str(&[0x80], "\u{FFFD}"));
        assert_eq!(hash_mutf8(&[0x80]), hash_str("\u{FFFD}"));
        assert_eq!(decode(&[0xE0, 0x41]), "\u{FFFD}A");
        assert!(eq_str(&[0xE0, 0x41], "\u{FFFD}A"));
        assert_eq!(
            decode(&[0xF0, 0x9F, 0x98, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        assert_eq!(decode(&[0xC3]), "\u{FFFD}");
        assert_eq!(decode(&[0xED, 0xA0, 0xBD]), "\u{FFFD}");
    }

    #[test]
    fn escape_string_baksmali_conventions() {
        assert_eq!(escape_string("abc"), "\"abc\"");
        assert_eq!(escape_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(escape_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(escape_string("a\nb\r\t"), "\"a\\nb\\r\\t\"");
        assert_eq!(escape_string("é"), "\"\\u00e9\"");
        assert_eq!(escape_string("\u{7f}"), "\"\\u007f\"");
        assert_eq!(escape_string("\u{8}"), "\"\\u0008\"");
        assert_eq!(escape_string("\u{1F600}"), "\"\\ud83d\\ude00\"");
        assert_eq!(escape_string("a b"), "\"a b\"");
    }

    #[test]
    fn escape_char_uses_single_quotes() {
        assert_eq!(escape_char(b'a' as u16), "'a'");
        assert_eq!(escape_char(b'\'' as u16), "'\\''");
        assert_eq!(escape_char(0x2028), "'\\u2028'");
    }

    #[test]
    fn encode_matches_java_mutf8_reference() {
        let s = "A\u{E9}\u{4E2D}\u{1F600}";
        let enc = encode(s);
        assert_eq!(enc.len(), 1 + 2 + 3 + 6);
        assert_eq!(decode(&enc), s);
        assert!(eq_str(&enc, s));
        assert!(!eq_str(&enc, "A\u{E9}\u{4E2D}"));
        assert_eq!(hash_mutf8(&enc), hash_str(s));
    }
}
