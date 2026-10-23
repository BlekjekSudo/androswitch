//! MUTF-8 (Modified UTF-8, Cesu-8-like) — o encoding das strings do DEX —
//! com decode/encode e o escaping de literais no estilo baksmali.
//!
//! Diferenças vs UTF-8 padrão:
//! - NUL (U+0000) é codificado como `0xC0 0x80` (nunca byte zero).
//! - Não há sequências de 4 bytes: code points suplementares (U+10000+) são
//!   codificados como par de surrogates UTF-16, cada um em 3 bytes.
//!
//! Decode é *lossy* (bytes inválidos → U+FFFD) — nunca pânico, nunca erro
//! (strings malformadas não podem derrubar o parser; Lei 1).

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

/// Decodifica bytes MUTF-8 para `String` (perda com U+FFFD em sequências inválidas).
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            out.push(b as char);
            i += 1;
        } else if (0xC0..=0xDF).contains(&b) {
            // 2 bytes — cobre 0xC0 0x80 → U+0000
            match bytes.get(i + 1) {
                Some(&b1) if b1 & 0xC0 == 0x80 => {
                    let cp = (((b & 0x1F) as u32) << 6) | (b1 & 0x3F) as u32;
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    i += 2;
                }
                _ => {
                    out.push('\u{FFFD}');
                    i += 1;
                }
            }
        } else if (0xE0..=0xEF).contains(&b) {
            // 3 bytes — pode produzir surrogate; combina pares válidos
            match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(&b1), Some(&b2)) if b1 & 0xC0 == 0x80 && b2 & 0xC0 == 0x80 => {
                    let cp = (((b & 0x0F) as u32) << 12)
                        | (((b1 & 0x3F) as u32) << 6)
                        | (b2 & 0x3F) as u32;
                    i += 3;
                    if (0xD800..=0xDBFF).contains(&cp) {
                        // high surrogate — tenta combinar com o próximo trio
                        if let (Some(&b3), Some(&b4), Some(&b5)) =
                            (bytes.get(i), bytes.get(i + 1), bytes.get(i + 2))
                        {
                            if (0xE0..=0xEF).contains(&b3) && b4 & 0xC0 == 0x80 && b5 & 0xC0 == 0x80
                            {
                                let cp2 = (((b3 & 0x0F) as u32) << 12)
                                    | (((b4 & 0x3F) as u32) << 6)
                                    | (b5 & 0x3F) as u32;
                                if (0xDC00..=0xDFFF).contains(&cp2) {
                                    let combined = 0x10000 + ((cp - 0xD800) << 10) + (cp2 - 0xDC00);
                                    if let Some(c) = char::from_u32(combined) {
                                        out.push(c);
                                        i += 3;
                                        continue;
                                    }
                                }
                            }
                        }
                        // surrogate não pareado → replacement
                        out.push('\u{FFFD}');
                    } else if (0xDC00..=0xDFFF).contains(&cp) {
                        out.push('\u{FFFD}');
                    } else {
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                }
                _ => {
                    out.push('\u{FFFD}');
                    i += 1;
                }
            }
        } else {
            // 0x80..0xBF solto ou 0xF0..0xFF (MUTF-8 não tem 4 bytes) → inválido
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
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
            // par de surrogates, cada um em 3 bytes (CESU-8)
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

/// Escaping de literal de string no estilo baksmali/dexlib2 (`StringUtils.escapeString`):
/// ASCII imprimível passa direto; `\n \r \t \\ "` com escape; o resto `\uXXXX`
/// (em unidades de código UTF-16 — suplementares viram par de escapes, como em Java).
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
    // ASCII imprimível que não exige escape
    if (32..127).contains(&unit) && unit as u8 as char != quote && unit != b'\\' as u16 {
        out.push(unit as u8 as char);
        return;
    }
    match unit {
        0x0A => out.push_str("\\n"),
        0x0D => out.push_str("\\r"),
        0x09 => out.push_str("\\t"),
        0x5C => out.push_str("\\\\"),
        // a própria aspa do literal (" no string, ' no char) vira \" / \'
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
    }

    #[test]
    fn ascii_roundtrip() {
        for b in 1u8..0x80 {
            let s = (b as char).to_string();
            assert_eq!(decode(&encode(&s)), s);
        }
        assert_eq!(decode(b"hello world!"), "hello world!");
    }

    #[test]
    fn bmp_roundtrip() {
        for cp in [0xE9u32, 0x4E2D, 0x2603, 0x20AC, 0xFFFD] {
            let s = char::from_u32(cp).unwrap().to_string();
            let enc = encode(&s);
            let want = if cp < 0x800 { 2 } else { 3 };
            assert_eq!(enc.len(), want, "cp U+{cp:04X} deve usar {want} bytes");
            assert_eq!(decode(&enc), s);
        }
    }

    #[test]
    fn supplementary_is_surrogate_pair() {
        let s = "\u{1F600}"; // 😀
        let enc = encode(s);
        assert_eq!(enc.len(), 6, "par de surrogates = 2 × 3 bytes");
        // ED A0 BD ED B8 80
        assert_eq!(enc, vec![0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
        assert_eq!(decode(&enc), s);
    }

    #[test]
    fn invalid_bytes_are_lossy_not_panic() {
        // byte solto de continuação, truncado, 4-byte proibido
        assert_eq!(decode(&[0x80]), "\u{FFFD}");
        // 0xE0 inválido vira replacement e o decode ressincroniza byte a byte
        assert_eq!(decode(&[0xE0, 0x41]), "\u{FFFD}A");
        assert_eq!(
            decode(&[0xF0, 0x9F, 0x98, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        // truncado no fim
        assert_eq!(decode(&[0xC3]), "\u{FFFD}");
        // surrogate não pareado
        assert_eq!(decode(&[0xED, 0xA0, 0xBD]), "\u{FFFD}");
    }

    #[test]
    fn escape_string_baksmali_conventions() {
        assert_eq!(escape_string("abc"), "\"abc\"");
        assert_eq!(escape_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(escape_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(escape_string("a\nb\r\t"), "\"a\\nb\\r\\t\"");
        // não-ASCII vira \uXXXX
        assert_eq!(escape_string("é"), "\"\\u00e9\"");
        // DEL e controle
        assert_eq!(escape_string("\u{7f}"), "\"\\u007f\"");
        assert_eq!(escape_string("\u{8}"), "\"\\u0008\"");
        // suplementar → par de escapes UTF-16 (comportamento Java)
        assert_eq!(escape_string("\u{1F600}"), "\"\\ud83d\\ude00\"");
        // espaço imprimível passa
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
        // "Aé中😀" — verificação cruzada dos comprimentos esperados
        let s = "A\u{E9}\u{4E2D}\u{1F600}";
        assert_eq!(encode(s).len(), 1 + 2 + 3 + 6);
        assert_eq!(decode(&encode(s)), s);
    }
}
