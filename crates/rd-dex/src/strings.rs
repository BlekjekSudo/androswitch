//! Seção de strings do DEX: string_id_item (offsets) + string_data_item
//! (uleb utf16_len + bytes MUTF-8 + NUL). Decode total com bounds-checks;
//! offsets brutos preservados (Lei 2).

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

use crate::error::{RdError, RdResult};
use crate::mutf8;
use crate::read::Reader;

pub const NO_INDEX: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone, Default)]
pub struct Strings {
    /// Offsets brutos de cada string_data_item (Lei 2).
    pub offsets: Vec<u32>,
    /// Conteúdo decodificado (MUTF-8 → String).
    pub values: Vec<String>,
}

impl Strings {
    /// Parse da tabela de strings inteira (eager — barato: offsets + decode).
    pub fn parse(data: &[u8], count: u32, off: u32) -> RdResult<Strings> {
        let count = count as usize;
        let off = off as usize;
        // guard fuzz: cada id ocupa 4 bytes
        if count.checked_mul(4).is_none() {
            return Err(RdError::parse("strings: string_ids_size overflow"));
        }
        let ids = data
            .get(off..)
            .ok_or_else(|| RdError::parse(format!("strings: ids table @ {off} além dos dados")))?;
        if count > ids.len() / 4 {
            return Err(RdError::parse(format!(
                "strings: {} ids não cabem em {} bytes",
                count,
                ids.len()
            )));
        }

        let mut offsets = Vec::with_capacity(count);
        let mut values = Vec::with_capacity(count.min(1 << 20));
        for i in 0..count {
            // string_data_off é ABSOLUTO (offset do início do arquivo, spec dex)
            let abs = crate::read::u32_at(ids, i * 4)?;
            let s = read_string_data(data, abs as usize)
                .map_err(|e| RdError::parse(format!("string #{i} @ 0x{abs:x}: {}", e.cause)))?;
            offsets.push(abs);
            values.push(s);
        }
        Ok(Strings { offsets, values })
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn get(&self, idx: u32) -> Option<&str> {
        if idx as usize >= self.values.len() {
            return None;
        }
        Some(&self.values[idx as usize])
    }

    /// Resolve índice (com NO_INDEX → None) para rendering tolerante.
    pub fn get_or_empty(&self, idx: u32) -> &str {
        if idx == NO_INDEX {
            return "";
        }
        self.get(idx).unwrap_or("")
    }
}

/// Lê um string_data_item: `uleb128 utf16_len` + bytes MUTF-8 + terminador 0x00.
fn read_string_data(data: &[u8], off: usize) -> RdResult<String> {
    let mut r = Reader::at(data, off)?;
    // utf16_len é lido só para avançar o cursor (uleb128 bounds-checked); o
    // valor declarado NÃO é confiável em arquivos corrompidos — ver política
    // no comentário abaixo (issue #12)
    let _utf16_len = r.uleb128()? as usize;
    // procura terminador 0x00 com bound — nunca sai da janela.
    // issue #36: bound de progresso por string — sem ele, uma tabela de ids
    // apontando para região sem NUL faz scan O(n²) (DoS de perf). Cap de
    // 1 MiB: strings reais têm < 64 Ki UTF-16 (≈192 KiB em MUTF-8)
    const MAX_STRING_BYTES: usize = 1 << 20;
    let start = r.pos;
    let mut end = start;
    while end < data.len() && data[end] != 0 && end - start < MAX_STRING_BYTES {
        end += 1;
    }
    if end >= data.len() {
        return Err(RdError::parse("string: terminador NUL ausente"));
    }
    if end - start >= MAX_STRING_BYTES {
        return Err(RdError::parse(
            "string: excede 1 MiB sem terminador NUL (stream malformada)",
        ));
    }
    let s = mutf8::decode(&data[start..end]);
    // spec: utf16_len é o comprimento em unidades de código UTF-16. Um
    // desacerto indica corrupção — mas o decode é LOSSY POR DESIGN (mutf8.rs:
    // "strings malformadas não podem derrubar o parser", Lei 1) e TODA
    // corrupção que aciona o U+FFFD também muda a contagem, então rejeitar o
    // mismatch anularia o decode lossy e derrubaria o DEX inteiro por causa de
    // uma única string (issue #12). Política alinhada com baksmali/dexlib2:
    // a string decodificada vence; o valor declarado é tratado como metadado
    // não-confiável e ignorado.
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_table() {
        // header mínimo: 2 ids em offset 0x0c; dados dos strings em 0x14 e 0x19
        let mut d = Vec::new();
        d.extend_from_slice(&2u32.to_le_bytes()); // count (dummy p/ alinhamento)
        d.extend_from_slice(&[0u8; 8]); // filler
        let ids_at = d.len() as u32; // 12
        d.extend_from_slice(&20u32.to_le_bytes()); // id0 → data @20 (absoluto)
        d.extend_from_slice(&25u32.to_le_bytes()); // id1 → data @25 (absoluto)
                                                   // string 0: utf16_len=3 "abc"
        d.extend_from_slice(&[0x03, b'a', b'b', b'c', 0x00]);
        // string 1: utf16_len=1 "é" (2 bytes em MUTF-8: C3 A9)
        d.extend_from_slice(&[0x01, 0xC3, 0xA9, 0x00]);

        let s = Strings::parse(&d, 2, ids_at).expect("parse");
        assert_eq!(s.get(0), Some("abc"));
        assert_eq!(s.get(1), Some("é"));
        assert_eq!(s.get(2), None);
        // Lei 2: offsets absolutos preservados
        assert_eq!(s.offsets, vec![20, 25]);
    }

    #[test]
    fn utf16_len_mismatch_is_tolerated_lossy() {
        // issue #12: promete utf16_len=3 mas "ab" tem 2 — corrupção não pode
        // derrubar o parse de um DEX inteiro (decode lossy por design, Lei 1;
        // paridade com baksmali/dexlib2)
        let d = [0x03u8, b'a', b'b', 0x00];
        let s = read_string_data(&d, 0).expect("mismatch não pode rejeitar");
        assert_eq!(s, "ab");
    }

    #[test]
    fn malformed_mutf8_bytes_decode_lossy_without_rejecting() {
        // 0xFF é MUTF-8 inválido → U+FFFD (1 unidade) com utf16_len declarado 2;
        // antes isso rejeitava o DEX INTEIRO — agora decodifica lossy
        let d = [0x02u8, 0xFF, 0x00];
        let s = read_string_data(&d, 0).expect("bytes inválidos não podem rejeitar");
        assert_eq!(s, "\u{FFFD}");
    }

    #[test]
    fn missing_terminator_is_typed_error() {
        let d = [0x02, b'a', b'b'];
        assert!(Strings::parse(&d, 1, 0).is_err());
    }

    #[test]
    fn out_of_bounds_table_is_typed_error() {
        let d = [0u8; 8];
        assert!(Strings::parse(&d, 4, 0).is_err());
    }
}
