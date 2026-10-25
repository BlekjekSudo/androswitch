//! Leitura bounds-checked de bytes/LEB128 — base de todo parser do rd-dex.
//!
//! Parsers nunca confiam (mesmo estilo fuzz-hardened do rd-apk): todo acesso
//! passa por fatias verificadas; LEB128 tem limite de bytes (anti-loop/anti-DoS).

use crate::error::{RdError, RdResult};

/// Cursor bounds-checked sobre uma fatia de bytes.
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> RdResult<Self> {
        if pos > data.len() {
            return Err(RdError::parse(format!(
                "reader: start offset {pos} beyond data (len {})",
                data.len()
            )));
        }
        Ok(Reader { data, pos })
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn take(&mut self, n: usize) -> RdResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| RdError::parse("reader: offset overflow"))?;
        let s = self.data.get(self.pos..end).ok_or_else(|| {
            RdError::parse(format!(
                "reader: truncated read of {n} byte(s) @ {} (len {})",
                self.pos,
                self.data.len()
            ))
        })?;
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> RdResult<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> RdResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> RdResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> RdResult<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn i32v(&mut self) -> RdResult<i32> {
        Ok(self.u32()? as i32)
    }

    pub fn i64v(&mut self) -> RdResult<i64> {
        Ok(self.u64()? as i64)
    }

    /// uleb128 — LEB128 sem sinal (máx. 5 bytes para u32; guard contra fluxo malformado).
    pub fn uleb128(&mut self) -> RdResult<u32> {
        let mut result: u32 = 0;
        let mut shift = 0u32;
        for i in 0..5 {
            let b = self.u8()?;
            let payload = (b & 0x7F) as u32;
            if i == 4 && b > 0x0F {
                return Err(RdError::parse(format!(
                    "uleb128: value overflows u32 @ {}",
                    self.pos - 1
                )));
            }
            result |= payload << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
        }
        Err(RdError::parse(format!(
            "uleb128: continuation beyond 5 bytes @ {}",
            self.pos
        )))
    }

    /// uleb128p1 — como uleb128, mas 0 representa -1 (NO_INDEX).
    pub fn uleb128p1(&mut self) -> RdResult<i64> {
        let v = self.uleb128()?;
        Ok(v as i64 - 1)
    }

    /// sleb128 — LEB128 com sinal (máx. 5 bytes para i32).
    pub fn sleb128(&mut self) -> RdResult<i32> {
        let mut result: u32 = 0;
        for i in 0..4 {
            let b = self.u8()?;
            let shift = i * 7;
            result |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                let next_shift = shift + 7;
                if b & 0x40 != 0 {
                    result |= !0u32 << next_shift;
                }
                return Ok(result as i32);
            }
        }
        let b = self.u8()?;
        if b & 0x80 != 0 {
            return Err(RdError::parse(format!(
                "sleb128: continuation beyond 5 bytes @ {}",
                self.pos
            )));
        }
        // Em 32 bits com sinal, os bits 6..=3 do 5º byte devem estender o bit 3
        // de sinal (todos 0 para >= 0, todos 1 para < 0 — AOSP DecodeSignedLeb128).
        let top = b & 0x78;
        if top != 0x00 && top != 0x78 {
            return Err(RdError::parse("sleb128: value overflows i32"));
        }
        result |= ((b & 0x0F) as u32) << 28;
        Ok(result as i32)
    }

    /// Fatia crua de `n` bytes (para Law 2 — janelas de bytes preservadas).
    pub fn bytes(&mut self, n: usize) -> RdResult<&'a [u8]> {
        self.take(n)
    }
}

#[inline]
pub fn u8_at(d: &[u8], off: usize) -> RdResult<u8> {
    d.get(off)
        .copied()
        .ok_or_else(|| RdError::parse(format!("truncated read u8 @ {off}")))
}

#[inline]
pub fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    d.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("truncated read u16 @ {off}")))
}

#[inline]
pub fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("truncated read u32 @ {off}")))
}

#[inline]
pub fn u64_at(d: &[u8], off: usize) -> RdResult<u64> {
    d.get(off..off + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| RdError::parse(format!("truncated read u64 @ {off}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uleb128_vectors() {
        let cases: &[(&[u8], u32)] = &[
            (&[0x00], 0),
            (&[0x7f], 127),
            (&[0x80, 0x01], 128),
            (&[0xff, 0x7f], 16_383),
            (&[0x80, 0x80, 0x01], 16_384),
            (&[0xff, 0xff, 0xff, 0xff, 0x0f], 0xFFFF_FFFF),
        ];
        for (bytes, want) in cases {
            let mut r = Reader::new(bytes);
            assert_eq!(r.uleb128().unwrap(), *want, "bytes {bytes:02x?}");
        }
    }

    #[test]
    fn uleb128_rejects_overflow_and_loop() {
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x1f]);
        assert!(r.uleb128().is_err(), "overflowing u32 deve falhar tipado");
        let mut r = Reader::new(&[0xff; 8]);
        assert!(r.uleb128().is_err(), "continuação infinita deve falhar");
    }

    #[test]
    fn sleb128_vectors() {
        let cases: &[(&[u8], i32)] = &[
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x7f], -1),
            (&[0x7e], -2),
            (&[0x40], -64),
            (&[0x3f], 63),
            (&[0x80, 0x7f], -128),
            (&[0xc0, 0x7f], -64),
            (&[0x80, 0x80, 0x7f], -16_384),
            (&[0xff, 0x00], 127),
        ];
        for (bytes, want) in cases {
            let mut r = Reader::new(bytes);
            assert_eq!(r.sleb128().unwrap(), *want, "bytes {bytes:02x?}");
        }
    }

    #[test]
    fn uleb128p1_maps_zero_to_no_index() {
        let mut r = Reader::new(&[0x00]);
        assert_eq!(r.uleb128p1().unwrap(), -1);
        let mut r = Reader::new(&[0x05]);
        assert_eq!(r.uleb128p1().unwrap(), 4);
    }

    #[test]
    fn bounds_checked_reads() {
        assert!(u16_at(&[0x01], 0).is_err());
        assert!(u32_at(&[0x01, 0x02, 0x03], 0).is_err());
        assert_eq!(u32_at(&[0x78, 0x56, 0x34, 0x12], 0).unwrap(), 0x1234_5678);
    }
}
