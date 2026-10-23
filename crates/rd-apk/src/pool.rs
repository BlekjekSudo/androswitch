//! ResStringPool — string pool do Android (usada por AXML e resources.arsc).
//!
//! Formato: `source.android.com` → "String Pool Resource" / ResStringPool_header.
//! Suporta UTF-16 e UTF-8 (flag `UTF8_FLAG`), com comprimentos prefixados e
//! terminador nulo — decodificação total com bounds-checks (parsers nunca confiam).

use crate::error::{RdError, RdResult};
use alloc::{format, string::String, vec::Vec};

pub const RES_STRING_POOL_TYPE: u16 = 0x0001;
pub const UTF8_FLAG: u32 = 1 << 8;

#[derive(Debug, Clone, Default)]
pub struct StringPool {
    strings: Vec<String>,
}

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    d.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("string pool: truncated read u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("string pool: truncated read u32 @ {off}")))
}

impl StringPool {
    /// Parse de um chunk de string pool em `data[chunk_off..]`.
    /// Retorna `(pool, tamanho total do chunk)` — o caller continua nos bytes seguintes.
    pub fn parse(data: &[u8], chunk_off: usize) -> RdResult<(Self, u32)> {
        let base = chunk_off;
        if data.len() < base + 28 {
            return Err(RdError::parse(
                "string pool: chunk shorter than header (28)",
            ));
        }
        let chunk_type = u16_at(data, base)?;
        if chunk_type != RES_STRING_POOL_TYPE {
            return Err(RdError::parse(format!(
                "string pool: expected type 0x{RES_STRING_POOL_TYPE:04x}, got 0x{chunk_type:04x}"
            )));
        }
        let header_size = u16_at(data, base + 2)? as usize;
        let chunk_size = u32_at(data, base + 4)? as usize;
        let string_count = u32_at(data, base + 8)? as usize;
        let _style_count = u32_at(data, base + 12)? as usize;
        let flags = u32_at(data, base + 16)?;
        let strings_start = u32_at(data, base + 20)? as usize;
        let _styles_start = u32_at(data, base + 24)? as usize;

        // Heurística de sanidade: pools reais têm headerSize 28 e chunk limitado.
        if chunk_size == 0 || chunk_size > data.len() - base {
            return Err(RdError::parse(format!(
                "string pool: chunk size {chunk_size} exceeds container"
            )));
        }
        if string_count > chunk_size {
            return Err(RdError::parse(format!(
                "string pool: implausible string count {string_count}"
            )));
        }

        let offsets_base = base + header_size;
        let strings_base = base + strings_start;
        let utf8 = flags & UTF8_FLAG != 0;

        let mut strings = Vec::with_capacity(string_count.min(65_536));
        for i in 0..string_count {
            let rel = u32_at(data, offsets_base + i * 4)? as usize;
            let abs = strings_base + rel;
            let s = if utf8 {
                Self::read_utf8(data, abs)?
            } else {
                Self::read_utf16(data, abs)?
            };
            strings.push(s);
        }

        Ok((StringPool { strings }, chunk_size as u32))
    }

    /// UTF-16: [len:u16 (ou 2×u16 se bit alto)] [len × u16 units] [0x0000].
    fn read_utf16(data: &[u8], off: usize) -> RdResult<String> {
        let mut p = off;
        let mut len = u16_at(data, p)? as usize;
        p += 2;
        if len & 0x8000 != 0 {
            len = ((len & 0x7FFF) << 16) | u16_at(data, p)? as usize;
            p += 2;
        }
        if len > data.len() {
            return Err(RdError::parse("string pool: utf16 length out of bounds"));
        }
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            units.push(u16_at(data, p)?);
            p += 2;
        }
        Ok(String::from_utf16_lossy(&units))
    }

    /// UTF-8: [declared chars: varint] [byte len: varint] [bytes] [0x00].
    fn read_utf8(data: &[u8], off: usize) -> RdResult<String> {
        let mut p = off;
        // 1º varint: comprimento em caracteres — decodificado mas ignorado
        let _decl_chars = Self::read_varint(data, &mut p)?;
        // 2º varint: comprimento em bytes (o que usamos de fato)
        let byte_len = Self::read_varint(data, &mut p)?;
        let end = p
            .checked_add(byte_len)
            .ok_or_else(|| RdError::parse("string pool: utf8 overflow"))?;
        let bytes = data
            .get(p..end)
            .ok_or_else(|| RdError::parse("string pool: utf8 string out of bounds"))?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Varint do ResStringPool UTF-8: 1 byte, ou 2 se o bit alto do 1º estiver setado
    /// (nesse caso o valor é `((b0 & 0x7F) << 8) | b1`).
    fn read_varint(data: &[u8], p: &mut usize) -> RdResult<usize> {
        let b0 = *data
            .get(*p)
            .ok_or_else(|| RdError::parse("string pool: utf8 truncated (varint)"))?;
        *p += 1;
        if b0 & 0x80 == 0 {
            return Ok(b0 as usize);
        }
        let b1 = *data
            .get(*p)
            .ok_or_else(|| RdError::parse("string pool: utf8 truncated (varint hi)"))?;
        *p += 1;
        Ok((((b0 & 0x7F) as usize) << 8) | b1 as usize)
    }

    pub fn get(&self, idx: u32) -> Option<&str> {
        self.strings.get(idx as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.strings.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constrói um pool UTF-8 mínimo à mão: ["hello", "world"]
    fn handcrafted_utf8_pool() -> Vec<u8> {
        // header (28 bytes)
        let mut v = Vec::new();
        v.extend(RES_STRING_POOL_TYPE.to_le_bytes()); // type
        v.extend(28u16.to_le_bytes()); // headerSize
        v.extend(0u32.to_le_bytes()); // size (patched later)
        v.extend(2u32.to_le_bytes()); // stringCount
        v.extend(0u32.to_le_bytes()); // styleCount
        v.extend(UTF8_FLAG.to_le_bytes()); // flags
        v.extend(36u32.to_le_bytes()); // stringsStart = 28 + 2*4 offsets
        v.extend(0u32.to_le_bytes()); // stylesStart
                                      // offsets (string 2 começa em 8: [5][5]"hello"[0] = 8 bytes)
        v.extend(0u32.to_le_bytes());
        v.extend(8u32.to_le_bytes());
        // strings: [decl chars][byte len][bytes][\0]
        v.push(5);
        v.push(5);
        v.extend_from_slice(b"hello");
        v.push(0);
        v.push(5);
        v.push(5);
        v.extend_from_slice(b"world");
        v.push(0);
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        v
    }

    #[test]
    fn parses_handcrafted_utf8_pool() {
        let bytes = handcrafted_utf8_pool();
        let (pool, size) = StringPool::parse(&bytes, 0).unwrap();
        assert_eq!(size as usize, bytes.len());
        assert_eq!(pool.get(0), Some("hello"));
        assert_eq!(pool.get(1), Some("world"));
        assert_eq!(pool.get(2), None);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn rejects_garbage_type() {
        let mut bytes = handcrafted_utf8_pool();
        bytes[0..2].copy_from_slice(&0xBEEFu16.to_le_bytes());
        assert!(StringPool::parse(&bytes, 0).is_err());
    }

    #[test]
    fn rejects_out_of_bounds_chunk() {
        let bytes = handcrafted_utf8_pool();
        assert!(StringPool::parse(&bytes, usize::MAX / 2).is_err());
    }
}
