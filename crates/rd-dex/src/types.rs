//! type_id_item — tabela de descritores de tipo (`Ljava/lang/String;`, `[I`, `I`…).
//! Cada entrada é um índice para a tabela de strings; validação básica de forma.

use crate::error::{RdError, RdResult};

/// Tabela de tipos: cada item é um string_idx.
#[derive(Debug, Clone, Default)]
pub struct TypeIds {
    pub string_idx: Vec<u32>,
}

impl TypeIds {
    pub(crate) fn validate_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
        let count = count as usize;
        let off = off as usize;
        let byte_len = count
            .checked_mul(4)
            .ok_or_else(|| RdError::parse("types: type_ids_size overflow"))?;
        let window = data
            .get(off..)
            .ok_or_else(|| RdError::parse(format!("types: table @ {off} além dos dados")))?;
        window.get(..byte_len).ok_or_else(|| {
            RdError::parse(format!(
                "types: {} ids não cabem em {} bytes",
                count,
                window.len()
            ))
        })
    }

    pub fn parse(data: &[u8], count: u32, off: u32) -> RdResult<TypeIds> {
        let window = Self::validate_window(data, count, off)?;
        let string_idx = window
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(TypeIds { string_idx })
    }

    pub fn len(&self) -> usize {
        self.string_idx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.string_idx.is_empty()
    }
}

/// Valida a forma de um descritor de tipo (JVM/JNI notation).
/// Tolerante: aceita formas comuns; usado apenas para heurística, nunca erro fatal.
pub fn looks_like_descriptor(s: &str) -> bool {
    let b = s.as_bytes();
    match b.first() {
        Some(b'V') => b.len() == 1,
        Some(b'Z' | b'B' | b'S' | b'C' | b'I' | b'J' | b'F' | b'D') => b.len() == 1,
        Some(b'L') => s.ends_with(';') && s.len() >= 3 && s.contains('/'),
        Some(b'[') => !s.is_empty(),
        _ => false,
    }
}

/// Converte descritor para notação Java de exibição (`Ljava/lang/String;` →
/// `java.lang.String`, `[I` → `int[]`) — usado em saída humana, não no smali.
pub fn descriptor_to_java(desc: &str) -> String {
    let (dims, rest) = count_dims(desc);
    let base = match rest.as_bytes().first() {
        Some(b'V') => "void".to_string(),
        Some(b'Z') => "boolean".to_string(),
        Some(b'B') => "byte".to_string(),
        Some(b'S') => "short".to_string(),
        Some(b'C') => "char".to_string(),
        Some(b'I') => "int".to_string(),
        Some(b'J') => "long".to_string(),
        Some(b'F') => "float".to_string(),
        Some(b'D') => "double".to_string(),
        Some(b'L') => rest[1..rest.len() - 1].replace('/', "."),
        _ => rest.to_string(),
    };
    let mut out = base;
    for _ in 0..dims {
        out.push_str("[]");
    }
    out
}

fn count_dims(desc: &str) -> (usize, &str) {
    let mut n = 0usize;
    let mut s = desc;
    while let Some(rest) = s.strip_prefix('[') {
        n += 1;
        s = rest;
    }
    (n, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_table() {
        let mut d = vec![0u8; 8];
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        let t = TypeIds::parse(&d, 2, 8).expect("parse");
        assert_eq!(t.string_idx, vec![0, 1]);
        assert!(TypeIds::parse(&d, 3, 8).is_err());
    }

    #[test]
    fn descriptors() {
        assert!(looks_like_descriptor("Ljava/lang/String;"));
        assert!(looks_like_descriptor("[I"));
        assert!(looks_like_descriptor("I"));
        assert!(!looks_like_descriptor("java/lang/String"));

        assert_eq!(descriptor_to_java("Ljava/lang/String;"), "java.lang.String");
        assert_eq!(descriptor_to_java("[I"), "int[]");
        assert_eq!(
            descriptor_to_java("[[Ljava/lang/Object;"),
            "java.lang.Object[][]"
        );
        assert_eq!(descriptor_to_java("V"), "void");
    }
}
