//! lazy.rs — `LazyDex`, `DexBacking` e `RawClassIndex` (ADR 008 Stage 1b).
//!
//! Monta um arquivo DEX em O(1) validando apenas o header (112 B), `map_list`
//! e os limites aritméticos das tabelas de stride fixo (`string_ids`,
//! `type_ids`, `proto_ids`, `field_ids`, `method_ids`, `class_defs`), sem
//! materializar `Vec<String>` nem `Vec<MethodId>`/`Vec<FieldId>`/`Vec<ClassDef>`.
//!
//! Lei 1: toda leitura por índice valida bounds e retorna `RdError` tipado.
//! Lei 2: offsets brutos preservados sobre o slice subjacente.
//! Lei 4: determinismo total (memoização imutável por índice; busca de classe
//! determinística preservando a regra "primeira definição vence").

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use crate::annotations::{self, EncodedValue};
use crate::classes::{self, ClassData, ClassDef};
use crate::code::CodeItem;
use crate::debug::{self, DebugInfo};
use crate::dex::{adler32, Dex, Header, MapItem};
use crate::error::{RdError, RdResult};
use crate::fields::{self, FieldId};
use crate::methods::{self, MethodId};
use crate::mutf8;
use crate::protos::{self, ProtoId};
use crate::strings::{self, PagedStringCache, Strings, NO_INDEX};
use crate::types::TypeIds;

const STR_OFF_QUESTION: u32 = u32::MAX;
const STR_OFF_EMPTY: u32 = u32::MAX - 1;

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
struct ClassSlot {
    /// Hash 64-bit não-zero (`0` = slot vazio).
    hash: u64,
    /// Offset direto do `string_data_item` em `data` (elimina 4 indireções de cache).
    str_off: u32,
    /// Índice em `class_defs`.
    class_def_idx: u32,
}

/// Índice flat em endereçamento aberto (`16 B` por slot, 4 slots por cache line L1)
/// construído diretamente sobre os slices MUTF-8 crus do DEX, sem decodificar
/// nem alocar `String`s para classes não tocadas.
#[derive(Debug, Clone, Default)]
pub struct RawClassIndex {
    slots: Box<[ClassSlot]>,
}

impl RawClassIndex {
    #[inline]
    fn slot_matches(data: &[u8], str_off: u32, descriptor: &str) -> bool {
        if str_off == STR_OFF_QUESTION {
            return descriptor == "?";
        }
        if str_off == STR_OFF_EMPTY {
            return descriptor.is_empty();
        }
        let abs = str_off as usize;
        if let Some(eq) = strings::inline_ascii_eq(data, abs, descriptor) {
            return eq;
        }
        match strings::read_string_slice(data, abs) {
            Ok(raw) => mutf8::eq_str(raw, descriptor),
            Err(_) => descriptor.is_empty(),
        }
    }

    #[inline]
    fn hash_at_str_off(data: &[u8], str_off: u32) -> u64 {
        if str_off == STR_OFF_QUESTION {
            return mutf8::hash_str("?");
        }
        if str_off == STR_OFF_EMPTY {
            return mutf8::hash_str("");
        }
        let abs = str_off as usize;
        if let Some(ascii) = strings::inline_ascii_str(data, abs) {
            return mutf8::hash_bytes_nonzero(ascii.as_bytes());
        }
        match strings::read_string_slice(data, abs) {
            Ok(raw) => mutf8::hash_mutf8(raw),
            Err(_) => mutf8::hash_str(""),
        }
    }

    fn insert_first_wins(
        slots: &mut [ClassSlot],
        mask: usize,
        data: &[u8],
        hash: u64,
        str_off: u32,
        class_def_idx: u32,
    ) {
        let mut pos = (hash as usize) & mask;
        loop {
            let cur = slots[pos];
            if cur.hash == 0 {
                slots[pos] = ClassSlot {
                    hash,
                    str_off,
                    class_def_idx,
                };
                return;
            }
            if cur.hash == hash && Self::same_str_off(data, cur.str_off, str_off) {
                // Primeira definição vence: descarta duplicata posterior.
                return;
            }
            pos = (pos + 1) & mask;
        }
    }

    fn same_str_off(data: &[u8], a: u32, b: u32) -> bool {
        if a == b {
            return true;
        }
        if a >= STR_OFF_EMPTY || b >= STR_OFF_EMPTY {
            return false;
        }
        match (
            strings::read_string_slice(data, a as usize),
            strings::read_string_slice(data, b as usize),
        ) {
            (Ok(ra), Ok(rb)) => {
                if ra == rb {
                    true
                } else {
                    let sb = mutf8::decode(rb);
                    mutf8::eq_str(ra, &sb)
                }
            }
            (Err(_), Err(_)) => true,
            _ => false,
        }
    }

    pub(crate) fn build_from_dex(
        data: &[u8],
        strings: &Strings,
        type_ids: &TypeIds,
        class_defs: &[ClassDef],
    ) -> Self {
        let n = class_defs.len();
        if n == 0 {
            return Self::default();
        }
        let cap = n.saturating_mul(2).max(16).next_power_of_two();
        let mask = cap - 1;
        let mut slots = vec![ClassSlot::default(); cap].into_boxed_slice();
        let mut seen_str_off = std::collections::HashSet::with_capacity(n.min(1 << 16));

        for (i, def) in class_defs.iter().enumerate() {
            let str_off = match type_ids.string_idx.get(def.class_idx as usize) {
                Some(&sidx) => match strings.offsets.get(sidx as usize) {
                    Some(&off) => off,
                    None => STR_OFF_EMPTY,
                },
                None => STR_OFF_QUESTION,
            };
            if !seen_str_off.insert(str_off) {
                continue;
            }
            let h = Self::hash_at_str_off(data, str_off);
            Self::insert_first_wins(&mut slots, mask, data, h, str_off, i as u32);
        }
        Self { slots }
    }

    #[inline]
    pub(crate) fn find_in_dex(
        &self,
        descriptor: &str,
        data: &[u8],
        _strings: &Strings,
        _type_ids: &TypeIds,
        _class_defs: &[ClassDef],
    ) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let h = mutf8::hash_str(descriptor);
        let mask = self.slots.len() - 1;
        let mut pos = (h as usize) & mask;
        loop {
            let slot = self.slots[pos];
            if slot.hash == 0 {
                return None;
            }
            if slot.hash == h && Self::slot_matches(data, slot.str_off, descriptor) {
                return Some(slot.class_def_idx as usize);
            }
            pos = (pos + 1) & mask;
        }
    }

    fn build_from_lazy(dex: &LazyDex) -> Self {
        let n = dex.hdr.class_defs_size as usize;
        if n == 0 {
            return Self::default();
        }
        let data = dex.as_bytes();
        let cap = n.saturating_mul(2).max(16).next_power_of_two();
        let mask = cap - 1;
        let mut slots = vec![ClassSlot::default(); cap].into_boxed_slice();
        let mut seen_str_off = std::collections::BTreeSet::new();

        for i in 0..n {
            let str_off = match dex
                .class_type_idx_unchecked_bounds(i)
                .and_then(|tidx| dex.type_string_idx(tidx).ok())
            {
                Some(sidx) => match dex.string_data_offset(sidx) {
                    Ok(o) => o as u32,
                    Err(_) => STR_OFF_EMPTY,
                },
                None => STR_OFF_QUESTION,
            };
            if !seen_str_off.insert(str_off) {
                continue;
            }
            let h = Self::hash_at_str_off(data, str_off);
            Self::insert_first_wins(&mut slots, mask, data, h, str_off, i as u32);
        }
        Self { slots }
    }

    #[inline]
    fn find_in_lazy(&self, descriptor: &str, dex: &LazyDex) -> Option<ClassDef> {
        if self.slots.is_empty() {
            return None;
        }
        let data = dex.as_bytes();
        let h = mutf8::hash_str(descriptor);
        let mask = self.slots.len() - 1;
        let mut pos = (h as usize) & mask;
        loop {
            let slot = self.slots[pos];
            if slot.hash == 0 {
                return None;
            }
            if slot.hash == h && Self::slot_matches(data, slot.str_off, descriptor) {
                return dex.class_def(slot.class_def_idx).ok();
            }
            pos = (pos + 1) & mask;
        }
    }
}

/// Armazenamento subjacente de um `LazyDex` (ADR 008 §5).
#[derive(Debug, Clone)]
pub enum DexBacking {
    /// `Vec<u8>` compartilhado em O(1) sem realocar/copiar o buffer (`From<Vec<u8>>`).
    OwnedVec(Arc<Vec<u8>>),
    /// Slice imutável compartilhado via `Arc<[u8]>`.
    Owned(Arc<[u8]>),
    /// Janela posicional sobre um buffer compartilhado (ex.: DEX STORED no APK).
    Slice {
        bytes: Arc<[u8]>,
        off: usize,
        len: usize,
    },
}

impl DexBacking {
    pub fn from_slice_window(bytes: Arc<[u8]>, off: usize, len: usize) -> RdResult<Self> {
        let end = off
            .checked_add(len)
            .ok_or_else(|| RdError::parse("dex backing: slice off+len overflow"))?;
        if end > bytes.len() {
            return Err(RdError::parse(format!(
                "dex backing: janela {off}..{end} excede buffer de {} bytes",
                bytes.len()
            )));
        }
        Ok(DexBacking::Slice { bytes, off, len })
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            DexBacking::OwnedVec(v) => v.as_slice(),
            DexBacking::Owned(b) => b.as_ref(),
            DexBacking::Slice { bytes, off, len } => &bytes[*off..*off + *len],
        }
    }
}

impl From<Vec<u8>> for DexBacking {
    fn from(v: Vec<u8>) -> Self {
        DexBacking::OwnedVec(Arc::new(v))
    }
}

impl From<Arc<Vec<u8>>> for DexBacking {
    fn from(v: Arc<Vec<u8>>) -> Self {
        DexBacking::OwnedVec(v)
    }
}

impl From<Arc<[u8]>> for DexBacking {
    fn from(a: Arc<[u8]>) -> Self {
        DexBacking::Owned(a)
    }
}

/// Leitor DEX sob demanda em O(1) no mount (ADR 008 §5 Stage 1b).
#[derive(Debug, Clone)]
pub struct LazyDex {
    src: DexBacking,
    hdr: Header,
    map: Vec<MapItem>,
    str_cache: PagedStringCache,
    class_index: OnceLock<RawClassIndex>,
}

impl LazyDex {
    /// Monta o DEX validando header, `map_list` e limites de todas as tabelas
    /// de stride fixo em O(1), deferindo o adler32 (ADR 009) e toda decodificação.
    pub fn mount(backing: DexBacking) -> RdResult<Self> {
        Self::mount_inner(backing, false)
    }

    /// Como `mount`, mas verifica também o checksum adler32 do header.
    pub fn mount_verified(backing: DexBacking) -> RdResult<Self> {
        Self::mount_inner(backing, true)
    }

    fn mount_inner(backing: DexBacking, verify_checksum: bool) -> RdResult<Self> {
        let raw_data = backing.as_bytes();
        let hdr = Dex::parse_header_checked(raw_data, verify_checksum)?;
        let data = &raw_data[..hdr.file_size as usize];
        let map = Dex::parse_map_list(data, hdr.map_off)?;

        // Valida os limites aritméticos de todas as seções fixas em O(1).
        let _ = Strings::validate_ids_window(data, hdr.string_ids_size, hdr.string_ids_off)?;
        let _ = TypeIds::validate_window(data, hdr.type_ids_size, hdr.type_ids_off)?;
        let _ = protos::validate_proto_window(data, hdr.proto_ids_size, hdr.proto_ids_off)?;
        let _ = fields::validate_field_window(data, hdr.field_ids_size, hdr.field_ids_off)?;
        let _ = methods::validate_method_window(data, hdr.method_ids_size, hdr.method_ids_off)?;
        let _ = classes::validate_class_defs_window(data, hdr.class_defs_size, hdr.class_defs_off)?;

        let str_cache = PagedStringCache::with_len_and_budget(
            hdr.string_ids_size as usize,
            strings::string_byte_budget(data.len()),
        );

        Ok(Self {
            src: backing,
            hdr,
            map,
            str_cache,
            class_index: OnceLock::new(),
        })
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.src.as_bytes()[..self.hdr.file_size as usize]
    }

    #[inline]
    pub fn header(&self) -> &Header {
        &self.hdr
    }

    #[inline]
    pub fn map_list(&self) -> &[MapItem] {
        &self.map
    }

    pub fn verify_checksum(&self) -> bool {
        let data = self.as_bytes();
        let fs = self.hdr.file_size as usize;
        fs >= 12 && fs <= data.len() && adler32(&data[12..fs]) == self.hdr.checksum
    }

    #[inline]
    pub fn strings_len(&self) -> u32 {
        self.hdr.string_ids_size
    }

    #[inline]
    pub fn class_defs_len(&self) -> u32 {
        self.hdr.class_defs_size
    }

    /// Quantidade de entradas em `str_cache` efetivamente decodificadas até agora.
    pub fn decoded_strings_count(&self) -> usize {
        self.str_cache.decoded_count()
    }

    #[inline]
    fn string_data_offset(&self, idx: u32) -> RdResult<usize> {
        if idx >= self.hdr.string_ids_size {
            return Err(RdError::parse(format!(
                "string idx {idx} fora da tabela ({})",
                self.hdr.string_ids_size
            )));
        }
        let pos = (self.hdr.string_ids_off as usize) + (idx as usize) * 4;
        let b = &self.as_bytes()[pos..pos + 4];
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    /// Retorna o slice MUTF-8 cru (sem o NUL final) de `idx`, zero-alloc.
    pub fn raw_string_bytes(&self, idx: u32) -> RdResult<&[u8]> {
        let abs = self.string_data_offset(idx)?;
        strings::read_string_slice(self.as_bytes(), abs)
            .map_err(|e| RdError::parse(format!("string #{idx} @ 0x{abs:x}: {}", e.cause)))
    }

    /// Decodifica `idx` sob demanda e memoiza em `str_cache` (ADR 008 §5).
    /// Para strings ASCII puras `< 128` bytes, empresta direto do buffer DEX sem alocar.
    pub fn string(&self, idx: u32) -> RdResult<Cow<'_, str>> {
        let i = idx as usize;
        if let Some(s) = self.str_cache.get(i) {
            return Ok(Cow::Borrowed(s));
        }
        let abs = self.string_data_offset(idx)?;
        if let Some(ascii) = strings::inline_ascii_str(self.as_bytes(), abs) {
            return Ok(Cow::Borrowed(ascii));
        }
        if let Some(cell) = self.str_cache.cell(i) {
            let raw = self.raw_string_bytes(idx)?;
            if raw.len() > self.str_cache.remaining_budget() {
                return Err(RdError::parse(format!(
                    "lazy_dex: string #{idx} ({} bytes) excede orçamento restante (limite {} bytes)",
                    raw.len(),
                    self.str_cache.byte_budget()
                )));
            }
            let decoded_str = mutf8::decode(raw);
            if !self.str_cache.try_charge(decoded_str.len()) {
                return Err(RdError::parse(format!(
                    "lazy_dex: volume decodificado acumulado excede orçamento ({} bytes)",
                    self.str_cache.byte_budget()
                )));
            }
            let _ = cell.set(decoded_str.into_boxed_str());
            if let Some(s) = cell.get() {
                return Ok(Cow::Borrowed(s.as_ref()));
            }
        }
        Err(RdError::parse(format!(
            "string idx {idx} fora da tabela ({})",
            self.hdr.string_ids_size
        )))
    }

    /// Acesso tolerante a string (`NO_INDEX` ou erro → `""`).
    #[inline]
    pub fn string_or_empty(&self, idx: u32) -> &str {
        if idx == NO_INDEX {
            return "";
        }
        let i = idx as usize;
        if let Some(s) = self.str_cache.get(i) {
            return s;
        }
        if let Ok(abs) = self.string_data_offset(idx) {
            if let Some(ascii) = strings::inline_ascii_str(self.as_bytes(), abs) {
                return ascii;
            }
        }
        let Ok(raw) = self.raw_string_bytes(idx) else {
            return "";
        };
        if raw.len() > self.str_cache.remaining_budget() {
            return "";
        }
        let decoded_str = mutf8::decode(raw);
        if !self.str_cache.try_charge(decoded_str.len()) {
            return "";
        }
        let Some(cell) = self.str_cache.cell(i) else {
            return "";
        };
        let _ = cell.set(decoded_str.into_boxed_str());
        match cell.get() {
            Some(s) => s.as_ref(),
            None => "",
        }
    }

    /// Compara `string_ids[idx]` com `s` sobre MUTF-8 cru sem alocar `String`
    /// (ADR 008 §5).
    #[inline]
    pub fn string_eq(&self, idx: u32, s: &str) -> RdResult<bool> {
        let abs = self.string_data_offset(idx)?;
        if let Some(eq) = strings::inline_ascii_eq(self.as_bytes(), abs, s) {
            return Ok(eq);
        }
        let i = idx as usize;
        if let Some(cached) = self.str_cache.get(i) {
            return Ok(cached == s);
        }
        let raw = self.raw_string_bytes(idx)?;
        Ok(mutf8::eq_str(raw, s))
    }

    /// Lê `type_ids[idx]` (`descriptor_idx` na tabela de strings) em O(1).
    pub fn type_string_idx(&self, idx: u32) -> RdResult<u32> {
        if idx >= self.hdr.type_ids_size {
            return Err(RdError::parse(format!(
                "type idx {idx} fora da tabela ({})",
                self.hdr.type_ids_size
            )));
        }
        let pos = (self.hdr.type_ids_off as usize) + (idx as usize) * 4;
        let b = &self.as_bytes()[pos..pos + 4];
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Descritor de tipo por índice (tolerante: índice inválido → `"?"`).
    pub fn type_str(&self, idx: u32) -> &str {
        match self.type_string_idx(idx) {
            Ok(sidx) => self.string_or_empty(sidx),
            Err(_) => "?",
        }
    }

    pub fn proto_id(&self, idx: u16) -> RdResult<ProtoId> {
        let idx_u32 = idx as u32;
        if idx_u32 >= self.hdr.proto_ids_size {
            return Err(RdError::parse(format!("proto idx {idx} fora da tabela")));
        }
        let pos = (self.hdr.proto_ids_off as usize) + (idx as usize) * 12;
        Ok(protos::decode_proto_id(&self.as_bytes()[pos..pos + 12]))
    }

    pub fn proto_params(&self, proto_idx: u16) -> RdResult<Vec<u32>> {
        let p = self.proto_id(proto_idx)?;
        protos::type_list(self.as_bytes(), p.parameters_off)
    }

    pub fn field_id(&self, idx: u32) -> RdResult<FieldId> {
        if idx >= self.hdr.field_ids_size {
            return Err(RdError::parse(format!("field idx {idx} fora da tabela")));
        }
        let pos = (self.hdr.field_ids_off as usize) + (idx as usize) * 8;
        Ok(fields::decode_field_id(&self.as_bytes()[pos..pos + 8]))
    }

    pub fn method_id(&self, idx: u32) -> RdResult<MethodId> {
        if idx >= self.hdr.method_ids_size {
            return Err(RdError::parse(format!("method idx {idx} fora da tabela")));
        }
        let pos = (self.hdr.method_ids_off as usize) + (idx as usize) * 8;
        Ok(methods::decode_method_id(&self.as_bytes()[pos..pos + 8]))
    }

    #[inline]
    fn class_type_idx_unchecked_bounds(&self, idx: usize) -> Option<u32> {
        if idx >= self.hdr.class_defs_size as usize {
            return None;
        }
        let pos = (self.hdr.class_defs_off as usize) + idx * 32;
        let b = self.as_bytes().get(pos..pos + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn class_def(&self, idx: u32) -> RdResult<ClassDef> {
        if idx >= self.hdr.class_defs_size {
            return Err(RdError::parse(format!(
                "class_def idx {idx} fora da tabela ({})",
                self.hdr.class_defs_size
            )));
        }
        let i = idx as usize;
        let pos = (self.hdr.class_defs_off as usize) + i * 32;
        Ok(classes::decode_class_def(
            i,
            &self.as_bytes()[pos..pos + 32],
        ))
    }

    /// Busca `ClassDef` por descritor (`Lcom/x/Y;`).
    ///
    /// Fast-path O(log N) sem construir índice quando `string_ids`, `type_ids`
    /// e `class_defs` estão ordenados segundo a spec DEX; caso contrário (ex.:
    /// `class_defs` em ordem topológica superclass-antes-de-subclass, padrão do
    /// `d8`), constrói uma única vez o `RawClassIndex` zero-alloc em O(N) e
    /// responde em O(1) para hits e misses entre múltiplos DEXs.
    pub fn find_class(&self, desc: &str) -> Option<ClassDef> {
        let idx = self
            .class_index
            .get_or_init(|| RawClassIndex::build_from_lazy(self));
        idx.find_in_lazy(desc, self)
    }

    pub fn class_data(&self, def: &ClassDef) -> RdResult<Option<ClassData>> {
        if def.class_data_off == 0 {
            return Ok(None);
        }
        classes::parse_class_data(self.as_bytes(), def.class_data_off).map(Some)
    }

    pub fn code(&self, off: u32) -> RdResult<Option<CodeItem>> {
        if off == 0 {
            return Ok(None);
        }
        CodeItem::parse(self.as_bytes(), off).map(Some)
    }

    pub fn debug_info(&self, off: u32) -> RdResult<Option<DebugInfo>> {
        if off == 0 {
            return Ok(None);
        }
        debug::parse_debug_info(self.as_bytes(), off).map(Some)
    }

    pub fn static_values(&self, def: &ClassDef) -> RdResult<Vec<EncodedValue>> {
        if def.static_values_off == 0 {
            return Ok(Vec::new());
        }
        annotations::parse_encoded_array(self.as_bytes(), def.static_values_off)
    }

    pub fn interfaces(&self, def: &ClassDef) -> RdResult<Vec<u32>> {
        protos::type_list(self.as_bytes(), def.interfaces_off)
    }
}
