//! ResStringPool — string pool do Android (usada por AXML e resources.arsc).
//!
//! Formato: `source.android.com` → "String Pool Resource" / ResStringPool_header.
//! Suporta UTF-16 e UTF-8 (flag `UTF8_FLAG`), com comprimentos prefixados e
//! terminador nulo — decodificação total com bounds-checks (parsers nunca confiam).
//!
//! **Lazy (ADR 014):** o parse valida o header e os limites ESTRUTURAIS do
//! chunk (issue #331 item 2) e retém os bytes crus; strings individuais são
//! decodificadas NA PRIMEIRA leitura (`get`) e memoizadas por slot (`OnceLock`).
//! Toda leitura fica dentro da fatia `[chunk_off, chunk_off + chunk_size)`
//! e do limite da região (`end`), nunca dos bytes de um chunk vizinho.
//! Decodificação total é orçada (`MAX_POOL_DECODED_BYTES`, anti-aliasing):
//! ao estourar, as strings seguintes viram `None` no acesso. Strings
//! malformadas individuais também viram `None`. Erros estruturais do pool
//! continuam `RdError` no parse (Lei 1).

use crate::arsc::try_add_capped;
use crate::error::{RdError, RdResult};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, OnceLock};

pub const RES_STRING_POOL_TYPE: u16 = 0x0001;
pub const UTF8_FLAG: u32 = 1 << 8;

/// Teto de bytes decodificados por pool (anti-aliasing, issue #331 item 2).
const MAX_POOL_DECODED_BYTES: usize = 128 * 1024 * 1024;

/// `off + k` checado — offsets são do atacante.
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

/// Pool lazy (ADR 014): bytes crus compartilhados + cache por slot.
///
/// `raw` é o documento INTEIRO (ARSC ou AXML) atrás de um `Arc` — o pool é uma
/// REGIÃO lógica `[chunk_off..chunk_off+chunk_size]` dele (Lei 2: bytes/offsets
/// crus preservados; clones compartilham o buffer). `end` é o limite absoluto
/// da região legível (o chunk do pool, ou o fim do pacote que o contém).
#[derive(Debug, Default)]
pub struct StringPool {
    raw: Arc<Vec<u8>>,
    /// Início absoluto do chunk do pool dentro de `raw`.
    chunk_off: usize,
    /// Fim absoluto do CHUNK do pool (`chunk_off + chunkSize`). Toda leitura
    /// de string fica confinada a `[..chunk_end]` — nunca cruza para o chunk vizinho.
    chunk_end: usize,
    count: usize,
    /// Relativos a `chunk_off`.
    offsets_base: usize,
    strings_base: usize,
    utf8: bool,
    /// Bytes decodificados até agora (compartilhado entre clones).
    decoded: Arc<AtomicUsize>,
    /// Cache por slot (ALOCAÇÃO PREGUIÇOSA — ADR 014): `Vec<OnceLock<..>>`
    /// só existe após o primeiro `get`. Decodifica 1x, memoiza; `None`
    /// interno = string malformada ou orçamento estourado (falha memoizada).
    cache: OnceLock<Vec<OnceLock<Option<String>>>>,
}

impl Clone for StringPool {
    /// Clone = mesma região (Arc compartilhado); cache renasce vazio
    /// (decodifica de novo por instância — determinístico). O orçamento
    /// de decodificação é compartilhado, para que clones não multipliquem o teto.
    fn clone(&self) -> Self {
        Self {
            raw: Arc::clone(&self.raw),
            chunk_off: self.chunk_off,
            chunk_end: self.chunk_end,
            count: self.count,
            offsets_base: self.offsets_base,
            strings_base: self.strings_base,
            utf8: self.utf8,
            decoded: Arc::clone(&self.decoded),
            cache: OnceLock::new(),
        }
    }
}

impl StringPool {
    /// Parse de um chunk de string pool em `data[chunk_off..]` (caminho
    /// standalone — AXML): retém UMA cópia do documento via `Arc`.
    /// Retorna `(pool, tamanho total do chunk)` — o caller continua nos bytes seguintes.
    pub fn parse(data: &[u8], chunk_off: usize) -> RdResult<(Self, u32)> {
        let raw = Arc::new(data.to_vec());
        let end = raw.len();
        Self::parse_in(&raw, chunk_off, end)
    }

    /// Parse compartilhado (caminho ARSC): o documento inteiro já vive num
    /// `Arc` — o pool é só uma região lógica dele, ZERO cópias extras.
    /// `region_end` limita TODAS as leituras (ex.: fim do pacote que contém o pool).
    pub fn parse_in(
        raw: &Arc<Vec<u8>>,
        chunk_off: usize,
        region_end: usize,
    ) -> RdResult<(Self, u32)> {
        let data: &[u8] = &raw[..region_end.min(raw.len())];
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
        let chunk_end = off_add(base, chunk_size)?;
        if chunk_size == 0 || chunk_end > data.len() {
            return Err(RdError::parse(format!(
                "string pool: chunk size {chunk_size} exceeds container"
            )));
        }
        if string_count > chunk_size {
            return Err(RdError::parse(format!(
                "string pool: implausible string count {string_count}"
            )));
        }
        // Fatia do chunk: daqui em diante os limites são relativos a ela.
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
        // Validação O(1): a tabela de offsets cabe no chunk e antes de strings_start.
        // Offsets individuais ruins viram `None` no acesso (ADR 014).
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

        Ok((
            StringPool {
                raw: Arc::clone(raw),
                chunk_off: base,
                chunk_end,
                count: string_count,
                offsets_base: header_size,
                strings_base: strings_start,
                utf8: flags & UTF8_FLAG != 0,
                decoded: Arc::new(AtomicUsize::new(0)),
                cache: OnceLock::new(),
            },
            chunk_size as u32,
        ))
    }

    /// UTF-16: [len:u16 (ou 2×u16 se bit alto)] [len × u16 units] [0x0000].
    fn read_utf16(data: &[u8], off: usize) -> RdResult<String> {
        let mut p = off;
        let mut len = u16_at(data, p)? as usize;
        p = off_add(p, 2)?;
        if len & 0x8000 != 0 {
            len = ((len & 0x7FFF) << 16) | u16_at(data, p)? as usize;
            p = off_add(p, 2)?;
        }
        // Limites em BYTES antes de alocar (issue #331 item 2).
        let bytes = len
            .checked_mul(2)
            .ok_or_else(|| RdError::parse("string pool: utf16 length overflow"))?;
        let end = off_add(p, bytes)?;
        let raw = data
            .get(p..end)
            .ok_or_else(|| RdError::parse("string pool: utf16 length out of bounds"))?;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// UTF-8: [declared chars: varint] [byte len: varint] [bytes] [0x00].
    fn read_utf8(data: &[u8], off: usize) -> RdResult<String> {
        let mut p = off;
        // 1º varint: comprimento em caracteres — decodificado mas ignorado
        let _decl_chars = Self::read_varint(data, &mut p)?;
        // 2º varint: comprimento em bytes (o que usamos de fato)
        let byte_len = Self::read_varint(data, &mut p)?;
        let end = off_add(p, byte_len)?;
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

    /// String por índice, decodificando NA PRIMEIRA leitura e memoizando.
    /// `None` = índice fora da pool, string malformada ou orçamento estourado.
    /// Strings vazias legítimas permanecem `Some("")`.
    pub fn get(&self, idx: u32) -> Option<&str> {
        let idx = idx as usize;
        if idx >= self.count {
            return None;
        }
        let slots = self
            .cache
            .get_or_init(|| (0..self.count).map(|_| OnceLock::new()).collect::<Vec<_>>());
        let slot = slots.get(idx)?;
        // get_or_init é &self: primeira leitura decodifica; demais retornam o cache.
        slot.get_or_init(|| self.decode(idx)).as_deref()
    }

    /// Decodifica a string `idx` dos bytes crus, limitado à região do pool.
    fn decode(&self, idx: usize) -> Option<String> {
        let data: &[u8] = &self.raw[..self.chunk_end];
        let slot = off_add(self.chunk_off, off_add(self.offsets_base, idx * 4).ok()?).ok()?;
        let rel = u32_at(data, slot).ok()? as usize;
        let abs = off_add(off_add(self.chunk_off, self.strings_base).ok()?, rel).ok()?;
        let s = if self.utf8 {
            Self::read_utf8(data, abs).ok()?
        } else {
            Self::read_utf16(data, abs).ok()?
        };
        // Orçamento de decodificação (anti-aliasing): ao estourar, falha memoizada.
        if !try_add_capped(&self.decoded, s.len(), MAX_POOL_DECODED_BYTES) {
            return None;
        }
        Some(s)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Itera decodificando sob demanda (mesma ordem/valores do eager quando
    /// todas as strings são sãs; malformadas são puladas — sem consumidores
    /// externos hoje, mantido por compatibilidade de API).
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.count).filter_map(move |i| self.get(i as u32))
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

    #[test]
    fn lazy_decodes_once_and_memoizes() {
        let bytes = handcrafted_utf8_pool();
        let (pool, _) = StringPool::parse(&bytes, 0).unwrap();
        let a = pool.get(0);
        let b = pool.get(0);
        // Mesmo endereço → memoizado (OnceLock devolve a mesma String).
        assert!(std::ptr::eq(a.unwrap(), b.unwrap()));
        assert_eq!(a, Some("hello"));
    }

    #[test]
    fn clone_resets_cache_but_keeps_content() {
        let bytes = handcrafted_utf8_pool();
        let (pool, _) = StringPool::parse(&bytes, 0).unwrap();
        let _ = pool.get(0); // aquece o cache
        let cloned = pool.clone();
        // Clone decodifica de novo (cache fresco) com o MESMO resultado.
        assert_eq!(cloned.get(0), Some("hello"));
        assert_eq!(cloned.get(1), Some("world"));
        // E os ponteiros são independentes (caches distintos).
        assert!(!std::ptr::eq(pool.get(0).unwrap(), cloned.get(0).unwrap()));
    }

    #[test]
    fn empty_string_is_legitimate() {
        // Pool com uma string vazia UTF-8: [0][0][\0]
        let mut v = Vec::new();
        v.extend(RES_STRING_POOL_TYPE.to_le_bytes());
        v.extend(28u16.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(1u32.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(UTF8_FLAG.to_le_bytes());
        v.extend(32u32.to_le_bytes()); // stringsStart = 28 + 1*4 (1 offset)
        v.extend(0u32.to_le_bytes()); // stylesStart
        v.extend(0u32.to_le_bytes()); // offset da string 0 (rel 0)
        v.push(0); // decl chars
        v.push(0); // byte len
        v.push(0); // terminador
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        let (pool, _) = StringPool::parse(&v, 0).unwrap();
        assert_eq!(pool.get(0), Some(""));
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn malformed_string_yields_none_not_panic() {
        // ADR 014: offsets individuais ruins são rejeitados NO ACESSO —
        // o parse aceita a estrutura (Lei 1 segue no nível do pool/header).
        // string 0 → offset gigante (fora do documento): get → None.
        let mut v = handcrafted_utf8_pool();
        v[28..32].copy_from_slice(&0xFFFFu32.to_le_bytes());
        let (pool, _) = StringPool::parse(&v, 0).unwrap();
        assert_eq!(pool.get(0), None);
        assert_eq!(pool.get(1), Some("world"));

        // offset DENTRO do documento, mas em bytes que não formam string
        // válida: get → None (falha memoizada, não pânico, não "").
        let mut w = handcrafted_utf8_pool();
        // string 1 → rel 11 → abs 47 = byte 'd': decl varint 100, depois
        // byte_len varint fora do documento → Err → None.
        w[32..36].copy_from_slice(&11u32.to_le_bytes());
        let (pool, _) = StringPool::parse(&w, 0).unwrap();
        assert_eq!(pool.get(1), None);
        assert_eq!(pool.get(0), Some("hello"));
    }

    #[test]
    fn iter_decodes_on_demand() {
        let bytes = handcrafted_utf8_pool();
        let (pool, _) = StringPool::parse(&bytes, 0).unwrap();
        let collected: Vec<&str> = pool.iter().collect();
        assert_eq!(collected, vec!["hello", "world"]);
    }

    // ─── #331 (item 2) — portados para o modelo lazy ─────────────────────────

    #[test]
    fn issue_331_chunk_off_near_usize_max_is_typed_not_panic() {
        // Na base, `base + 28` panicaria (overflow). Deve ser erro tipado.
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
        v.extend(std::iter::repeat_n(b'A', slen));
        v.push(0);
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        v
    }

    #[test]
    fn issue_331_offset_aliasing_hits_decoded_budget() {
        // 5_000 × 30_000 B = 150 MB decodificados de um chunk de ~50 KB.
        // Lazy: o parse é estrutural e passa; o orçamento aparece no ACESSO —
        // após ~128 MiB, as leituras seguintes viram None (falha memoizada).
        let bytes = aliased_utf8_pool(5_000, 30_000);
        assert!(bytes.len() < 64 * 1024);
        let (pool, _) = StringPool::parse(&bytes, 0).expect("structural parse ok");
        let mut ok = 0usize;
        let mut none = 0usize;
        for i in 0..5_000u32 {
            match pool.get(i) {
                Some(s) => {
                    assert_eq!(s.len(), 30_000);
                    ok += 1;
                }
                None => none += 1,
            }
        }
        assert!(none > 0, "budget never tripped");
        assert!(ok * 30_000 <= 128 * 1024 * 1024, "decoded past the budget");
        // Falha memoizada: reler um slot negado continua None.
        assert!(pool.get(4_999).is_none());
    }

    #[test]
    fn issue_331_utf16_string_cannot_read_past_chunk_end() {
        // String UTF-16 no fim do chunk declara 400k unidades; atrás do
        // chunk há 1 MB de zeros (outro "chunk"). A leitura NÃO pode cruzar
        // o fim do chunk: o acesso devolve None (e o parse não aloca 800 KB).
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
        v.extend((0x8000u16 | 6).to_le_bytes());
        v.extend(0x1A80u16.to_le_bytes());
        let size = v.len() as u32;
        v[4..8].copy_from_slice(&size.to_le_bytes());
        v.extend(std::iter::repeat_n(0u8, 1024 * 1024)); // "chunk seguinte"
        let (pool, _) = StringPool::parse(&v, 0).expect("structural parse ok");
        assert_eq!(pool.get(0), None);
    }
}
