//! ResStringPool — string pool do Android (usada por AXML e resources.arsc).
//!
//! Formato: `source.android.com` → "String Pool Resource" / ResStringPool_header.
//! Suporta UTF-16 e UTF-8 (flag `UTF8_FLAG`), com comprimentos prefixados e
//! terminador nulo — decodificação total com bounds-checks (parsers nunca confiam).

use crate::error::{RdError, RdResult};

pub const RES_STRING_POOL_TYPE: u16 = 0x0001;
pub const UTF8_FLAG: u32 = 1 << 8;

#[derive(Debug, Clone, Default)]
pub struct StringPool {
    strings: Vec<String>,
}

/// `off + k` checado (issue #331, item 2) — offsets são do atacante.
#[inline]
fn off_add(off: usize, k: usize) -> RdResult<usize> {
    off.checked_add(k)
        .ok_or_else(|| RdError::parse(format!("string pool: offset overflow ({off} + {k})")))
}

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    let end = off_add(off, 2)?;
    d.get(off..end)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("string pool: truncated read u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    let end = off_add(off, 4)?;
    d.get(off..end)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("string pool: truncated read u32 @ {off}")))
}

impl StringPool {
    /// Parse de um chunk de string pool em `data[chunk_off..]`.
    /// Retorna `(pool, tamanho total do chunk)` — o caller continua nos bytes seguintes.
    ///
    /// Issue #331 (item 2): todas as leituras acontecem dentro da
    /// fatia `data[base..base + chunk_size]` — nada lê bytes de
    /// chunks vizinhos — e o total decodificado é orçado
    /// (`MAX_POOL_DECODED_BYTES`): sem isso, N offsets repetidos
    /// para uma única string grande clonam GiB no heap do host a
    /// partir de um chunk de poucos KB (aliasing).
    pub fn parse(data: &[u8], chunk_off: usize) -> RdResult<(Self, u32)> {
        /// Teto de bytes decodificados por pool (anti-aliasing).
        const MAX_POOL_DECODED_BYTES: usize = 128 * 1024 * 1024;

        let base = chunk_off;
        if off_add(base, 28)? > data.len() {
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
        let header_size = u16_at(data, off_add(base, 2)?)? as usize;
        let chunk_size = u32_at(data, off_add(base, 4)?)? as usize;
        let string_count = u32_at(data, off_add(base, 8)?)? as usize;
        let _style_count = u32_at(data, off_add(base, 12)?)? as usize;
        let flags = u32_at(data, off_add(base, 16)?)?;
        let strings_start = u32_at(data, off_add(base, 20)?)? as usize;
        let _styles_start = u32_at(data, off_add(base, 24)?)? as usize;

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

        // Fatia do chunk: daqui em diante TUDO é relativo a ela.
        let chunk_end = off_add(base, chunk_size)?;
        let chunk = &data[base..chunk_end];
        if header_size < 28 || header_size > chunk.len() {
            return Err(RdError::parse(format!(
                "string pool: implausible header size {header_size}"
            )));
        }
        if strings_start > chunk.len() {
            return Err(RdError::parse(format!(
                "string pool: strings start {strings_start} exceeds chunk"
            )));
        }
        let table_end = off_add(
            header_size,
            string_count
                .checked_mul(4)
                .ok_or_else(|| RdError::parse("string pool: offset table overflow"))?,
        )?;
        if table_end > chunk.len() || (string_count > 0 && table_end > strings_start) {
            return Err(RdError::parse(
                "string pool: offset table exceeds chunk/strings start",
            ));
        }

        let utf8 = flags & UTF8_FLAG != 0;
        let mut strings = Vec::with_capacity(string_count.min(65_536));
        let mut decoded_total = 0usize;
        for i in 0..string_count {
            let rel = u32_at(chunk, off_add(header_size, i * 4)?)? as usize;
            let abs = off_add(strings_start, rel)?;
            let s = if utf8 {
                Self::read_utf8(chunk, abs)?
            } else {
                Self::read_utf16(chunk, abs)?
            };
            decoded_total = off_add(decoded_total, s.len())?;
            if decoded_total > MAX_POOL_DECODED_BYTES {
                return Err(RdError::parse(format!(
                    "string pool: decoded strings exceed budget ({MAX_POOL_DECODED_BYTES} bytes)"
                )));
            }
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
        // Bounds ANTES de alocar (#331 item 2): `len` é em UNIDADES
        // de 16 bits; exigimos `len * 2` bytes restantes na fatia.
        let need = len
            .checked_mul(2)
            .ok_or_else(|| RdError::parse("string pool: utf16 length overflow"))?;
        if off_add(p, need)? > data.len() {
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

    // ─── #331 (item 2): overflow, aliasing e leitura cross-chunk ────────────

    #[test]
    fn issue_331_chunk_off_near_usize_max_is_typed_not_panic() {
        // Na base, `base + 28` na primeira checagem panica (overflow).
        let bytes = handcrafted_utf8_pool();
        assert!(StringPool::parse(&bytes, usize::MAX - 10).is_err());
    }

    /// Pool UTF-8 com `count` offsets TODOS apontando para uma única
    /// string de `slen` bytes (aliasing): chunk pequeno, decode gigante.
    fn aliased_utf8_pool(count: usize, slen: usize) -> Vec<u8> {
        fn varint2(v: usize) -> [u8; 2] {
            [0x80 | ((v >> 8) as u8), (v & 0xFF) as u8]
        }
        let mut v = Vec::new();
        v.extend(RES_STRING_POOL_TYPE.to_le_bytes());
        v.extend(28u16.to_le_bytes());
        v.extend(0u32.to_le_bytes()); // size (patched)
        v.extend((count as u32).to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(UTF8_FLAG.to_le_bytes());
        v.extend((28 + count as u32 * 4).to_le_bytes()); // stringsStart
        v.extend(0u32.to_le_bytes());
        for _ in 0..count {
            v.extend(0u32.to_le_bytes()); // todos -> mesma string
        }
        v.extend(varint2(slen));
        v.extend(varint2(slen));
        v.extend(std::iter::repeat(b'A').take(slen));
        v.push(0);
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        v
    }

    #[test]
    fn issue_331_offset_aliasing_hits_decoded_budget() {
        // 5_000 × 30_000 B = 150 MB decodificados de um chunk de ~50 KB.
        let bytes = aliased_utf8_pool(5_000, 30_000);
        assert!(bytes.len() < 64 * 1024);
        let err = format!("{:?}", StringPool::parse(&bytes, 0).unwrap_err());
        assert!(err.contains("budget"), "esperado orçamento: {err}");
    }

    #[test]
    fn issue_331_utf16_string_cannot_read_past_chunk_end() {
        // String UTF-16 no fim do chunk declara 400k unidades; atrás do
        // chunk há 1 MB de zeros (outro "chunk"). Na base, a checagem
        // `len > data.len()` (unidades vs bytes do CONTAINER) passava e
        // a leitura cruzava o fim do chunk devolvendo 400k NULs.
        let mut v = Vec::new();
        v.extend(RES_STRING_POOL_TYPE.to_le_bytes());
        v.extend(28u16.to_le_bytes());
        v.extend(0u32.to_le_bytes()); // size (patched)
        v.extend(1u32.to_le_bytes()); // stringCount
        v.extend(0u32.to_le_bytes());
        v.extend(0u32.to_le_bytes()); // flags: UTF-16
        v.extend(32u32.to_le_bytes()); // stringsStart = 28 + 4
        v.extend(0u32.to_le_bytes());
        v.extend(0u32.to_le_bytes()); // offset 0
                                      // len UTF-16 (forma longa): 400_000 = (6 << 16) | 0x1A80… exato:
        v.extend((0x8000u16 | 6).to_le_bytes());
        v.extend(0x1A80u16.to_le_bytes());
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        v.extend(std::iter::repeat(0u8).take(1024 * 1024)); // "chunk seguinte"
        assert!(StringPool::parse(&v, 0).is_err());
    }
}
