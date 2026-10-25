//! Seção de strings do DEX: string_id_item (offsets) + string_data_item
//! (uleb utf16_len + bytes MUTF-8 + NUL). Suporta modo eager (`Strings::parse`)
//! e modo lazy paginado sob demanda (`Strings::parse_lazy`, ADR 008 Stage 1b);
//! offsets brutos preservados (Lei 2).

use std::sync::OnceLock;

use crate::error::{RdError, RdResult};
use crate::mutf8;
use crate::read::Reader;

pub const NO_INDEX: u32 = 0xFFFF_FFFF;

const CACHE_PAGE_SHIFT: usize = 8;
const CACHE_PAGE_LEN: usize = 1 << CACHE_PAGE_SHIFT; // 256 strings/página
const CACHE_PAGE_MASK: usize = CACHE_PAGE_LEN - 1;

#[derive(Debug)]
struct StringCachePage {
    slots: [OnceLock<Box<str>>; CACHE_PAGE_LEN],
}

impl StringCachePage {
    fn new() -> Box<Self> {
        Box::new(Self {
            slots: std::array::from_fn(|_| OnceLock::new()),
        })
    }
}

impl Clone for StringCachePage {
    fn clone(&self) -> Self {
        Self {
            slots: std::array::from_fn(|i| self.slots[i].clone()),
        }
    }
}

/// Cache paginado de 2 níveis (`256` strings por página) para materialização
/// lock-free sob demanda: um DEX de 65.536 strings aloca apenas ~4 KiB no mount
/// em vez de ~1,5 MiB quando não tocado.
#[derive(Debug, Clone, Default)]
pub(crate) struct PagedStringCache {
    len: usize,
    pages: Box<[OnceLock<Box<StringCachePage>>]>,
}

impl PagedStringCache {
    pub(crate) fn with_len(len: usize) -> Self {
        if len == 0 {
            return Self::default();
        }
        let page_count = (len + CACHE_PAGE_LEN - 1) >> CACHE_PAGE_SHIFT;
        let pages = (0..page_count)
            .map(|_| OnceLock::new())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self { len, pages }
    }

    #[inline]
    pub(crate) fn get(&self, idx: usize) -> Option<&str> {
        if idx >= self.len {
            return None;
        }
        let page = self.pages.get(idx >> CACHE_PAGE_SHIFT)?.get()?;
        page.slots[idx & CACHE_PAGE_MASK].get().map(Box::as_ref)
    }

    #[inline]
    pub(crate) fn cell(&self, idx: usize) -> Option<&OnceLock<Box<str>>> {
        if idx >= self.len {
            return None;
        }
        let page = self
            .pages
            .get(idx >> CACHE_PAGE_SHIFT)?
            .get_or_init(StringCachePage::new);
        Some(&page.slots[idx & CACHE_PAGE_MASK])
    }

    pub(crate) fn decoded_count(&self) -> usize {
        self.pages
            .iter()
            .filter_map(OnceLock::get)
            .map(|p| p.slots.iter().filter(|s| s.get().is_some()).count())
            .sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Strings {
    /// Offsets brutos de cada string_data_item (Lei 2).
    pub offsets: Vec<u32>,
    /// Conteúdo decodificado no modo eager (`Strings::parse` / `Dex::parse`).
    /// No modo lazy (`Strings::parse_lazy` / `Dex::parse_unverified`), fica
    /// vazio e cada entrada tocada é materializada em `lazy`.
    pub values: Vec<String>,
    lazy: PagedStringCache,
}

impl Strings {
    /// Valida a tabela `string_id_item` e retorna o slice bruto de `count * 4` bytes.
    pub(crate) fn validate_ids_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
        let count = count as usize;
        let off = off as usize;
        let byte_len = count
            .checked_mul(4)
            .ok_or_else(|| RdError::parse("strings: string_ids_size overflow"))?;
        let ids = data
            .get(off..)
            .ok_or_else(|| RdError::parse(format!("strings: ids table @ {off} além dos dados")))?;
        ids.get(..byte_len).ok_or_else(|| {
            RdError::parse(format!(
                "strings: {} ids não cabem em {} bytes",
                count,
                ids.len()
            ))
        })
    }

    /// Parse da tabela de strings inteira (modo eager: offsets + decode completo).
    pub fn parse(data: &[u8], count: u32, off: u32) -> RdResult<Strings> {
        let window = Self::validate_ids_window(data, count, off)?;
        let count = count as usize;
        let mut offsets = Vec::with_capacity(count);
        let mut values = Vec::with_capacity(count.min(1 << 20));
        for (i, chunk) in window.chunks_exact(4).enumerate() {
            let abs = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let s = read_string_data(data, abs as usize)
                .map_err(|e| RdError::parse(format!("string #{i} @ 0x{abs:x}: {}", e.cause)))?;
            offsets.push(abs);
            values.push(s);
        }
        Ok(Strings {
            offsets,
            values,
            lazy: PagedStringCache::default(),
        })
    }

    /// ADR 008 Stage 1b: lê e valida os offsets de `string_id_item` sem
    /// decodificar nem alocar `String`s para entradas não tocadas.
    pub fn parse_lazy(data: &[u8], count: u32, off: u32) -> RdResult<Strings> {
        let window = Self::validate_ids_window(data, count, off)?;
        let count = count as usize;
        let data_len = data.len();
        let mut offsets = Vec::with_capacity(count);
        for (i, chunk) in window.chunks_exact(4).enumerate() {
            let abs = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if abs as usize >= data_len {
                return Err(RdError::parse(format!(
                    "string #{i} @ 0x{abs:x}: offset fora dos dados ({data_len} bytes)"
                )));
            }
            offsets.push(abs);
        }
        Ok(Strings {
            offsets,
            values: Vec::new(),
            lazy: PagedStringCache::with_len(count),
        })
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Quantidade de strings já decodificadas em memória (`len()` no modo eager,
    /// apenas o subconjunto tocado no modo lazy).
    pub fn decoded_count(&self) -> usize {
        if !self.values.is_empty() {
            self.values.len()
        } else {
            self.lazy.decoded_count()
        }
    }

    pub fn get(&self, idx: u32) -> Option<&str> {
        let i = idx as usize;
        if let Some(s) = self.values.get(i) {
            return Some(s.as_str());
        }
        self.lazy.get(i)
    }

    /// Resolve índice (com NO_INDEX → "") usando apenas valores já materializados.
    pub fn get_or_empty(&self, idx: u32) -> &str {
        if idx == NO_INDEX {
            return "";
        }
        self.get(idx).unwrap_or("")
    }

    /// Resolve índice sob demanda contra `data` (decodifica e memoiza na 1ª leitura
    /// no modo lazy; retorna erro tipado se o `string_data_item` estiver truncado).
    pub fn try_get_with_data<'a>(&'a self, idx: u32, data: &'a [u8]) -> RdResult<Option<&'a str>> {
        if idx == NO_INDEX {
            return Ok(None);
        }
        let i = idx as usize;
        if let Some(s) = self.values.get(i) {
            return Ok(Some(s.as_str()));
        }
        if let Some(cached) = self.lazy.get(i) {
            return Ok(Some(cached));
        }
        let Some(&abs_u32) = self.offsets.get(i) else {
            return Ok(None);
        };
        let Some(cell) = self.lazy.cell(i) else {
            return Ok(None);
        };
        let abs = abs_u32 as usize;
        let decoded = read_string_data(data, abs)
            .map_err(|e| RdError::parse(format!("string #{i} @ 0x{abs_u32:x}: {}", e.cause)))?
            .into_boxed_str();
        let _ = cell.set(decoded);
        Ok(cell.get().map(Box::as_ref))
    }

    /// Resolve índice sob demanda contra `data`: O(1) nas leituras repetidas via
    /// `PagedStringCache` (decodifica apenas na 1ª leitura da string).
    #[inline]
    pub fn get_with_data<'a>(&'a self, idx: u32, data: &'a [u8]) -> Option<&'a str> {
        if idx == NO_INDEX {
            return None;
        }
        let i = idx as usize;
        if !self.values.is_empty() {
            return self.values.get(i).map(String::as_str);
        }
        if let Some(cached) = self.lazy.get(i) {
            return Some(cached);
        }
        let abs = *self.offsets.get(i)? as usize;
        let cell = self.lazy.cell(i)?;
        Some(
            cell.get_or_init(|| {
                read_string_data(data, abs)
                    .unwrap_or_default()
                    .into_boxed_str()
            })
            .as_ref(),
        )
    }

    /// Resolve índice sob demanda para rendering tolerante (`NO_INDEX`/inválido → `""`).
    #[inline]
    pub fn get_or_empty_with_data<'a>(&'a self, idx: u32, data: &'a [u8]) -> &'a str {
        if idx == NO_INDEX {
            return "";
        }
        self.get_with_data(idx, data).unwrap_or("")
    }

    /// Retorna os bytes MUTF-8 crus (sem o terminador NUL) de `idx` em `data`,
    /// sem alocar `String`.
    pub fn raw_slice<'a>(&self, idx: u32, data: &'a [u8]) -> RdResult<Option<&'a [u8]>> {
        if idx == NO_INDEX {
            return Ok(None);
        }
        let Some(&abs) = self.offsets.get(idx as usize) else {
            return Ok(None);
        };
        read_string_slice(data, abs as usize).map(Some)
    }
}

/// Fast-path O(1) sem alocação nem `OnceLock`: quando `data[off]` é um `uleb128`
/// de 1 byte (`len < 128`), `data[off + 1 + len] == 0` e todos os bytes de
/// `data[off + 1 .. off + 1 + len]` estão em `0x01..=0x7F`, o slice em `data`
/// já é UTF-8 válido e idêntico ao decode MUTF-8.
#[inline]
pub(crate) fn inline_ascii_str(data: &[u8], off: usize) -> Option<&str> {
    let b0 = *data.get(off)?;
    if b0 >= 0x80 {
        return None;
    }
    let len = b0 as usize;
    let win = data.get(off + 1..off + 2 + len)?;
    if win[len] != 0 {
        return None;
    }
    let body = &win[..len];
    if !body.iter().all(|&b| b.wrapping_sub(1) < 0x7F) {
        return None;
    }
    std::str::from_utf8(body).ok()
}

/// Fast-path O(1) para comparar `string_data_item @ off` contra `target` ASCII.
/// Retorna `Some(bool)` quando resolvido diretamente sobre `data`, ou `None`
/// quando `off`/`target` exige o caminho geral MUTF-8.
#[inline]
pub(crate) fn inline_ascii_eq(data: &[u8], off: usize, target: &str) -> Option<bool> {
    let tb = target.as_bytes();
    let len = tb.len();
    if len >= 0x80 || !crate::mutf8::is_pure_ascii_non_nul(tb) {
        return None;
    }
    let b0 = *data.get(off)?;
    if b0 >= 0x80 {
        return None;
    }
    let decl_len = b0 as usize;
    let win = data.get(off + 1..off + 2 + decl_len)?;
    if win[decl_len] != 0 {
        return None;
    }
    if decl_len != len {
        // Se o corpo é ASCII puro sem NUL precoce, comprimentos distintos
        // garantem desigualdade imediata sem percorrer o corpo inteiro em memcmp.
        if crate::mutf8::is_pure_ascii_non_nul(&win[..decl_len]) {
            return Some(false);
        }
        return None;
    }
    // `tb` já foi validado como ASCII puro sem NUL; se `&win[..len] == tb`,
    // então `win[..len]` também é ASCII puro sem NUL!
    if &win[..len] == tb {
        return Some(true);
    }
    if crate::mutf8::is_pure_ascii_non_nul(&win[..len]) {
        return Some(false);
    }
    None
}

/// Lê o slice MUTF-8 cru de um `string_data_item` (`uleb128 utf16_len` + bytes até `0x00`),
/// limitado a 1 MiB (proteção contra DoS em streams sem NUL — issue #36).
pub(crate) fn read_string_slice(data: &[u8], off: usize) -> RdResult<&[u8]> {
    let mut r = Reader::at(data, off)?;
    let _utf16_len = r.uleb128()? as usize;
    const MAX_STRING_BYTES: usize = 1 << 20;
    let start = r.pos;
    let rest = data
        .get(start..)
        .ok_or_else(|| RdError::parse("string: offset após uleb128 fora dos dados"))?;
    let scan_window = &rest[..rest.len().min(MAX_STRING_BYTES)];
    if let Some(nul_rel) = scan_window.iter().position(|&b| b == 0) {
        return Ok(&scan_window[..nul_rel]);
    }
    if rest.len() > MAX_STRING_BYTES {
        Err(RdError::parse(
            "string: excede 1 MiB sem terminador NUL (stream malformada)",
        ))
    } else {
        Err(RdError::parse("string: terminador NUL ausente"))
    }
}

/// Lê um string_data_item: `uleb128 utf16_len` + bytes MUTF-8 + terminador 0x00.
pub(crate) fn read_string_data(data: &[u8], off: usize) -> RdResult<String> {
    let raw = read_string_slice(data, off)?;
    Ok(mutf8::decode(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_table() -> RdResult<()> {
        let mut d = Vec::new();
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&[0u8; 8]);
        let ids_at = d.len() as u32;
        d.extend_from_slice(&20u32.to_le_bytes());
        d.extend_from_slice(&25u32.to_le_bytes());
        d.extend_from_slice(&[0x03, b'a', b'b', b'c', 0x00]);
        d.extend_from_slice(&[0x01, 0xC3, 0xA9, 0x00]);

        let s = Strings::parse(&d, 2, ids_at)?;
        assert_eq!(s.get(0), Some("abc"));
        assert_eq!(s.get(1), Some("é"));
        assert_eq!(s.get(2), None);
        assert_eq!(s.offsets, vec![20, 25]);

        let lazy = Strings::parse_lazy(&d, 2, ids_at)?;
        assert_eq!(lazy.len(), 2);
        assert_eq!(lazy.decoded_count(), 0);
        assert_eq!(lazy.get_with_data(1, &d), Some("é"));
        assert_eq!(lazy.decoded_count(), 1);
        assert_eq!(lazy.get_with_data(0, &d), Some("abc"));
        assert_eq!(lazy.decoded_count(), 2);
        Ok(())
    }

    #[test]
    fn utf16_len_mismatch_is_tolerated_lossy() -> RdResult<()> {
        let d = [0x03u8, b'a', b'b', 0x00];
        let s = read_string_data(&d, 0)?;
        assert_eq!(s, "ab");
        Ok(())
    }

    #[test]
    fn malformed_mutf8_bytes_decode_lossy_without_rejecting() -> RdResult<()> {
        let d = [0x02u8, b'\xff', 0x00];
        let s = read_string_data(&d, 0)?;
        assert_eq!(s, "\u{FFFD}");
        Ok(())
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
        assert!(Strings::parse_lazy(&d, 4, 0).is_err());
    }
}
