//! field_id_item + encoded_field — referências de campo e entradas de class_data.

use crate::error::{RdError, RdResult};
use crate::read::Reader;

#[derive(Debug, Clone, Copy)]
pub struct FieldId {
    /// índice na tabela de tipos (classe declarante).
    pub class_idx: u16,
    /// índice na tabela de tipos (tipo do campo).
    pub type_idx: u16,
    /// índice na tabela de strings (nome).
    pub name_idx: u32,
}

pub(crate) fn validate_field_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
    let count = count as usize;
    let off = off as usize;
    let byte_len = count
        .checked_mul(8)
        .ok_or_else(|| RdError::parse("fields: field_ids_size overflow"))?;
    let window = data
        .get(off..)
        .ok_or_else(|| RdError::parse(format!("fields: table @ {off} além dos dados")))?;
    window.get(..byte_len).ok_or_else(|| {
        RdError::parse(format!(
            "fields: {} ids não cabem em {} bytes",
            count,
            window.len()
        ))
    })
}

#[inline]
pub(crate) fn decode_field_id(b: &[u8]) -> FieldId {
    FieldId {
        class_idx: u16::from_le_bytes([b[0], b[1]]),
        type_idx: u16::from_le_bytes([b[2], b[3]]),
        name_idx: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
    }
}

pub fn parse_field_ids(data: &[u8], count: u32, off: u32) -> RdResult<Vec<FieldId>> {
    let window = validate_field_window(data, count, off)?;
    Ok(window.chunks_exact(8).map(decode_field_id).collect())
}

/// encoded_field (class_data): field_idx_diff uleb + access_flags uleb.
#[derive(Debug, Clone)]
pub struct EncodedField {
    /// índice absoluto na tabela de field_ids (diff acumulado).
    pub field_idx: u32,
    pub access_flags: u32,
    /// offset bruto do encoded_field (Lei 2).
    pub offset: usize,
}

pub(crate) fn read_encoded_field(r: &mut Reader) -> RdResult<EncodedField> {
    let offset = r.pos;
    let diff = r.uleb128()?;
    let flags = r.uleb128()?;
    Ok(EncodedField {
        field_idx: diff,
        access_flags: flags,
        offset,
    })
}

/// Acessos de flags de campo (ordem dexlib2 — usada no rendering smali).
pub fn format_field_flags(flags: u32) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for &(bit, name) in FIELD_FLAG_ORDER {
        if flags & bit != 0 {
            parts.push(name);
        }
    }
    parts.join(" ")
}

pub const ACC_PUBLIC: u32 = 0x1;
pub const ACC_PRIVATE: u32 = 0x2;
pub const ACC_PROTECTED: u32 = 0x4;
pub const ACC_STATIC: u32 = 0x8;
pub const ACC_FINAL: u32 = 0x10;
pub const ACC_SYNCHRONIZED_SUPER: u32 = 0x20;
pub const ACC_VOLATILE_BRIDGE: u32 = 0x40;
pub const ACC_TRANSIENT_VARARGS: u32 = 0x80;
pub const ACC_NATIVE: u32 = 0x100;
pub const ACC_INTERFACE: u32 = 0x200;
pub const ACC_ABSTRACT: u32 = 0x400;
pub const ACC_STRICTFP: u32 = 0x800;
pub const ACC_SYNTHETIC: u32 = 0x1000;
pub const ACC_ANNOTATION: u32 = 0x2000;
pub const ACC_ENUM: u32 = 0x4000;
pub const ACC_CONSTRUCTOR: u32 = 0x1_0000;
pub const ACC_DECLARED_SYNCHRONIZED: u32 = 0x2_0000;

/// Ordem de renderização de flags de campo (igual à enum AccessFlags do dexlib2).
pub const FIELD_FLAG_ORDER: &[(u32, &str)] = &[
    (ACC_PUBLIC, "public"),
    (ACC_PRIVATE, "private"),
    (ACC_PROTECTED, "protected"),
    (ACC_STATIC, "static"),
    (ACC_FINAL, "final"),
    (ACC_VOLATILE_BRIDGE, "volatile"),
    (ACC_TRANSIENT_VARARGS, "transient"),
    (ACC_SYNTHETIC, "synthetic"),
    (ACC_ENUM, "enum"),
];

/// Ordem para flags de classe (0x20/super não é renderizado, como no baksmali).
pub const CLASS_FLAG_ORDER: &[(u32, &str)] = &[
    (ACC_PUBLIC, "public"),
    (ACC_PRIVATE, "private"),
    (ACC_PROTECTED, "protected"),
    (ACC_STATIC, "static"),
    (ACC_FINAL, "final"),
    (ACC_INTERFACE, "interface"),
    (ACC_ABSTRACT, "abstract"),
    (ACC_SYNTHETIC, "synthetic"),
    (ACC_ANNOTATION, "annotation"),
    (ACC_ENUM, "enum"),
];

/// Ordem para flags de método.
pub const METHOD_FLAG_ORDER: &[(u32, &str)] = &[
    (ACC_PUBLIC, "public"),
    (ACC_PRIVATE, "private"),
    (ACC_PROTECTED, "protected"),
    (ACC_STATIC, "static"),
    (ACC_FINAL, "final"),
    (ACC_SYNCHRONIZED_SUPER, "synchronized"),
    (ACC_VOLATILE_BRIDGE, "bridge"),
    (ACC_TRANSIENT_VARARGS, "varargs"),
    (ACC_NATIVE, "native"),
    (ACC_ABSTRACT, "abstract"),
    (ACC_STRICTFP, "strictfp"),
    (ACC_SYNTHETIC, "synthetic"),
    (ACC_CONSTRUCTOR, "constructor"),
    (ACC_DECLARED_SYNCHRONIZED, "declared-synchronized"),
];

pub fn format_flags(flags: u32, order: &[(u32, &str)]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for &(bit, name) in order {
        if flags & bit != 0 {
            parts.push(name);
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_field_ids_table() {
        let mut d = Vec::new();
        d.extend_from_slice(&1u16.to_le_bytes()); // class_idx
        d.extend_from_slice(&2u16.to_le_bytes()); // type_idx
        d.extend_from_slice(&3u32.to_le_bytes()); // name_idx
        let f = parse_field_ids(&d, 1, 0).expect("fields");
        assert_eq!(f[0].class_idx, 1);
        assert_eq!(f[0].type_idx, 2);
        assert_eq!(f[0].name_idx, 3);
        assert!(parse_field_ids(&d, 2, 0).is_err());
    }

    #[test]
    fn flag_ordering_matches_dexlib2() {
        assert_eq!(
            format_field_flags(ACC_PUBLIC | ACC_STATIC | ACC_FINAL),
            "public static final"
        );
        assert_eq!(
            format_field_flags(ACC_PRIVATE | ACC_VOLATILE_BRIDGE | ACC_SYNTHETIC),
            "private volatile synthetic"
        );
        assert_eq!(format_flags(0, FIELD_FLAG_ORDER), "");
    }
}
