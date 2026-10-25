//! dex.rs — header, map list e a orquestração do parse completo (`Dex`).
//!
//! Lei 2: os bytes brutos ficam preservados em `Dex::data` e cada seção guarda
//! seu offset. Toda leitura é bounds-checked; contagens implausíveis falham
//! tipado antes de alocar (estilo fuzz-hardened do rd-apk).

use crate::annotations::EncodedValue;
use crate::classes::{self, ClassData, ClassDef};
use crate::error::{RdError, RdResult};
use crate::fields::FieldId;
use crate::methods::{self, MethodHandle, MethodId};
use crate::protos::{self, ProtoId};
use crate::read;
use crate::strings::Strings;
use crate::{code, debug};

/// Magic DEX: `dex\n0XX\0`.
pub const DEX_MAGIC: [u8; 4] = *b"dex\n";
/// endian_tag little-endian (padrão de produção).
pub const ENDIAN_CONSTANT_LE: u32 = 0x1234_5678;
/// endian_tag big-endian (raro; rejeitado com erro tipado).
pub const ENDIAN_CONSTANT_BE: u32 = 0x7856_3412;

/// Versões de formato aceitas.
pub const KNOWN_VERSIONS: [&str; 5] = ["035", "037", "038", "039", "041"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DexVersion(pub [u8; 3]);

impl DexVersion {
    pub fn as_str(&self) -> &'static str {
        match &self.0 {
            [b'0', b'3', b'5'] => "035",
            [b'0', b'3', b'7'] => "037",
            [b'0', b'3', b'8'] => "038",
            [b'0', b'3', b'9'] => "039",
            [b'0', b'4', b'1'] => "041",
            _ => "?",
        }
    }
}

/// Cabeçalho DEX (112 bytes).
#[derive(Debug, Clone)]
pub struct Header {
    pub magic: [u8; 8],
    pub version: DexVersion,
    pub checksum: u32,
    /// checksum adler32 armazenado no header.
    pub signature: [u8; 20],
    pub file_size: u32,
    pub header_size: u32,
    pub endian_tag: u32,
    pub link_size: u32,
    pub link_off: u32,
    pub map_off: u32,
    pub string_ids_size: u32,
    pub string_ids_off: u32,
    pub type_ids_size: u32,
    pub type_ids_off: u32,
    pub proto_ids_size: u32,
    pub proto_ids_off: u32,
    pub field_ids_size: u32,
    pub field_ids_off: u32,
    pub method_ids_size: u32,
    pub method_ids_off: u32,
    pub class_defs_size: u32,
    pub class_defs_off: u32,
    pub data_size: u32,
    pub data_off: u32,
    /// adler32 recalculado confere? (informacional; parse não falha).
    /// `false` também quando a verificação foi explicitamente deferida
    /// (ADR 009: `Dex::parse_unverified`) — use `Dex::verify_checksum()`
    /// para o valor autoritativo.
    pub checksum_ok: bool,
}

impl Header {
    pub fn version_str(&self) -> &'static str {
        self.version.as_str()
    }
}

/// Tipo de item do map list (nomes do dex-format).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MapType {
    Header,
    StringId,
    TypeId,
    ProtoId,
    FieldId,
    MethodId,
    ClassDef,
    CallSiteId,
    MethodHandle,
    MapList,
    TypeList,
    AnnotationSetRefList,
    AnnotationSetItem,
    ClassData,
    Code,
    StringData,
    DebugInfo,
    Annotation,
    EncodedArray,
    AnnotationsDirectory,
    HiddenapiClassData,
    Unknown(u16),
}

impl MapType {
    pub fn from_code(code: u16) -> MapType {
        match code {
            0x0000 => MapType::Header,
            0x0001 => MapType::StringId,
            0x0002 => MapType::TypeId,
            0x0003 => MapType::ProtoId,
            0x0004 => MapType::FieldId,
            0x0005 => MapType::MethodId,
            0x0006 => MapType::ClassDef,
            0x1000 => MapType::MapList,
            0x1001 => MapType::TypeList,
            0x1002 => MapType::AnnotationSetRefList,
            0x1003 => MapType::AnnotationSetItem,
            0x2000 => MapType::ClassData,
            0x2001 => MapType::Code,
            0x2002 => MapType::StringData,
            0x2003 => MapType::DebugInfo,
            0x2004 => MapType::Annotation,
            0x2005 => MapType::EncodedArray,
            0x2006 => MapType::AnnotationsDirectory,
            // DEX 038+: invokedynamic / method handles (lambdas desugar,
            // string concat, records) — códigos OFICIAIS da spec dex-format
            // (TYPE_CALL_SITE_ID_ITEM=0x0007, TYPE_METHOD_HANDLE_ITEM=0x0008).
            // Sem estes braços o loop de população nunca preenche
            // call_site_offsets/method_handles (issues #10 e #21)
            0x0007 => MapType::CallSiteId,
            0x0008 => MapType::MethodHandle,
            0xF000 => MapType::HiddenapiClassData,
            _ => MapType::Unknown(code),
        }
    }

    pub fn code(&self) -> u16 {
        match self {
            MapType::Header => 0x0000,
            MapType::StringId => 0x0001,
            MapType::TypeId => 0x0002,
            MapType::ProtoId => 0x0003,
            MapType::FieldId => 0x0004,
            MapType::MethodId => 0x0005,
            MapType::ClassDef => 0x0006,
            MapType::MapList => 0x1000,
            MapType::TypeList => 0x1001,
            MapType::AnnotationSetRefList => 0x1002,
            MapType::AnnotationSetItem => 0x1003,
            MapType::ClassData => 0x2000,
            MapType::Code => 0x2001,
            MapType::StringData => 0x2002,
            MapType::DebugInfo => 0x2003,
            MapType::Annotation => 0x2004,
            MapType::EncodedArray => 0x2005,
            MapType::AnnotationsDirectory => 0x2006,
            MapType::HiddenapiClassData => 0xF000,
            MapType::CallSiteId => 0x0007,
            MapType::MethodHandle => 0x0008,
            MapType::Unknown(c) => *c,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            MapType::Header => "header",
            MapType::StringId => "string_id_item",
            MapType::TypeId => "type_id_item",
            MapType::ProtoId => "proto_id_item",
            MapType::FieldId => "field_id_item",
            MapType::MethodId => "method_id_item",
            MapType::ClassDef => "class_def_item",
            MapType::CallSiteId => "call_site_id_item",
            MapType::MethodHandle => "method_handle_item",
            MapType::MapList => "map_list",
            MapType::TypeList => "type_list",
            MapType::AnnotationSetRefList => "annotation_set_ref_list",
            MapType::AnnotationSetItem => "annotation_set_item",
            MapType::ClassData => "class_data_item",
            MapType::Code => "code_item",
            MapType::StringData => "string_data_item",
            MapType::DebugInfo => "debug_info_item",
            MapType::Annotation => "annotation_item",
            MapType::EncodedArray => "encoded_array_item",
            MapType::AnnotationsDirectory => "annotations_directory_item",
            MapType::HiddenapiClassData => "hiddenapi_class_data_item",
            MapType::Unknown(_) => "unknown",
        }
    }
}

/// Entrada do map list.
#[derive(Debug, Clone, Copy)]
pub struct MapItem {
    pub map_type: MapType,
    /// campo `unused` (bruto — Lei 2).
    pub unused: u16,
    pub size: u32,
    pub offset: u32,
    /// código bruto do tipo (para Unknown).
    pub raw_type: u16,
}

/// Adler-32 (verificação do campo checksum do header).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// Modelo completo de um DEX parseado.
#[derive(Debug, Clone)]
pub struct Dex {
    /// Bytes brutos (Lei 2 — nada se perde).
    pub data: Vec<u8>,
    pub header: Header,
    pub map_list: Vec<MapItem>,
    pub strings: Strings,
    pub type_ids: crate::types::TypeIds,
    pub proto_ids: Vec<ProtoId>,
    pub field_ids: Vec<FieldId>,
    pub method_ids: Vec<MethodId>,
    pub class_defs: Vec<ClassDef>,
    /// offsets de call_site_id_item (encoded_array_item).
    pub call_site_offsets: Vec<u32>,
    pub method_handles: Vec<MethodHandle>,
    /// descriptor -> class_defs position, built lazily on first `find_class`
    /// over raw MUTF-8 slices without decoding or allocating `String`s
    /// (ADR 008 Stage 1b `RawClassIndex`).
    pub class_index: std::sync::OnceLock<crate::lazy::RawClassIndex>,
}

impl Dex {
    /// Detecta se os bytes são um DEX (pelo magic).
    pub fn looks_like_dex(data: &[u8]) -> bool {
        data.len() >= 8 && data[0..4] == DEX_MAGIC
    }

    /// Parseia um DEX completo de memória (verifica o adler32 do header e
    /// materializa a tabela de strings inteira para inspeção/fuzzing).
    pub fn parse(data: Vec<u8>) -> RdResult<Dex> {
        Self::parse_inner(data, true, false)
    }

    /// ADR 009 + ADR 008 Stage 1b: como `parse`, mas DEFERE a verificação do
    /// adler32 do header e a decodificação MUTF-8 das strings (apenas os
    /// offsets de `string_ids` são validados no mount; cada string tocada em
    /// `Dex::string` / `Dex::type_str` é decodificada e memoizada sob demanda).
    /// `header.checksum_ok` fica `false` = "não verificado"; `verify_checksum()`
    /// recalcula sob demanda. `rd dex`/inspect/fuzz continuam usando `parse`.
    pub fn parse_unverified(data: Vec<u8>) -> RdResult<Dex> {
        Self::parse_inner(data, false, true)
    }

    /// Alias explícito para o mount com strings sob demanda e adler32 deferido
    /// (ADR 008 Stage 1b).
    pub fn parse_lazy(data: Vec<u8>) -> RdResult<Dex> {
        Self::parse_inner(data, false, true)
    }

    fn parse_inner(data: Vec<u8>, verify_checksum: bool, lazy_strings: bool) -> RdResult<Dex> {
        let header = Self::parse_header_with(&data, verify_checksum)?;
        let map_list = Self::parse_map_list(&data, header.map_off)?;
        let strings = if lazy_strings {
            Strings::parse_lazy(&data, header.string_ids_size, header.string_ids_off)?
        } else {
            Strings::parse(&data, header.string_ids_size, header.string_ids_off)?
        };
        let type_ids =
            crate::types::TypeIds::parse(&data, header.type_ids_size, header.type_ids_off)?;
        let proto_ids =
            protos::parse_proto_ids(&data, header.proto_ids_size, header.proto_ids_off)?;
        let field_ids =
            crate::fields::parse_field_ids(&data, header.field_ids_size, header.field_ids_off)?;
        let method_ids =
            methods::parse_method_ids(&data, header.method_ids_size, header.method_ids_off)?;
        let class_defs =
            classes::parse_class_defs(&data, header.class_defs_size, header.class_defs_off)?;

        // call sites + method handles (via map list — versões 038+)
        let mut call_site_offsets = Vec::new();
        let mut method_handles = Vec::new();
        for item in &map_list {
            match item.map_type {
                MapType::CallSiteId => {
                    let mut r = read::Reader::at(&data, item.offset as usize)?;
                    for _ in 0..item.size {
                        call_site_offsets.push(r.u32()?);
                    }
                }
                MapType::MethodHandle => {
                    method_handles = methods::parse_method_handles(&data, item.size, item.offset)?;
                }
                _ => {}
            }
        }

        Ok(Dex {
            data,
            header,
            map_list,
            strings,
            type_ids,
            proto_ids,
            field_ids,
            method_ids,
            class_defs,
            class_index: std::sync::OnceLock::new(),
            call_site_offsets,
            method_handles,
        })
    }

    /// Recalcula o adler32 sobre os bytes e compara com o header
    /// (autoritativo; independente de como o DEX foi parseado).
    pub fn verify_checksum(&self) -> bool {
        let fs = self.header.file_size as usize;
        fs >= 12 && fs <= self.data.len() && adler32(&self.data[12..fs]) == self.header.checksum
    }

    /// Parse do header com validações fuzz-safe.
    pub fn parse_header(data: &[u8]) -> RdResult<Header> {
        Self::parse_header_with(data, true)
    }

    pub(crate) fn parse_header_checked(data: &[u8], verify_checksum: bool) -> RdResult<Header> {
        Self::parse_header_with(data, verify_checksum)
    }

    fn parse_header_with(data: &[u8], verify_checksum: bool) -> RdResult<Header> {
        if data.len() < 112 {
            return Err(RdError::parse(format!(
                "dex: arquivo de {} bytes < header mínimo (112)",
                data.len()
            )));
        }
        if data[0..4] != DEX_MAGIC {
            return Err(RdError::invalid_format(format!(
                "dex: magic inválido (esperado {:?}, got {:?})",
                std::str::from_utf8(&DEX_MAGIC).unwrap_or("?"),
                String::from_utf8_lossy(&data[0..4])
            )));
        }
        let digits = &data[4..7];
        if !digits.iter().all(|b| b.is_ascii_digit()) || data[7] != 0 {
            return Err(RdError::invalid_format(format!(
                "dex: versão malformada {:?}",
                String::from_utf8_lossy(digits)
            )));
        }
        let version = DexVersion([digits[0], digits[1], digits[2]]);
        let checksum = read::u32_at(data, 8)?;
        let mut signature = [0u8; 20];
        signature.copy_from_slice(&data[12..32]);
        let file_size = read::u32_at(data, 32)?;
        let header_size = read::u32_at(data, 36)?;
        let endian_tag = read::u32_at(data, 40)?;
        if endian_tag == ENDIAN_CONSTANT_BE {
            return Err(RdError::invalid_format(
                "dex: big-endian endian_tag não suportado",
            ));
        }
        if endian_tag != ENDIAN_CONSTANT_LE {
            return Err(RdError::invalid_format(format!(
                "dex: endian_tag desconhecido 0x{endian_tag:08x}"
            )));
        }
        if header_size < 112 {
            return Err(RdError::invalid_format(format!(
                "dex: header_size {header_size} < 112"
            )));
        }
        // file_size não pode exceder o buffer real (APKs podem ter padding)
        if file_size as usize > data.len() {
            return Err(RdError::invalid_format(format!(
                "dex: file_size {file_size} > tamanho real {}",
                data.len()
            )));
        }
        let checksum_ok = verify_checksum
            && file_size >= 12
            && adler32(&data[12..file_size as usize]) == checksum;

        Ok(Header {
            magic: data[0..8].try_into().unwrap_or([0u8; 8]),
            version,
            checksum,
            checksum_ok,
            signature,
            file_size,
            header_size,
            endian_tag,
            link_size: read::u32_at(data, 44)?,
            link_off: read::u32_at(data, 48)?,
            map_off: read::u32_at(data, 52)?,
            string_ids_size: read::u32_at(data, 56)?,
            string_ids_off: read::u32_at(data, 60)?,
            type_ids_size: read::u32_at(data, 64)?,
            type_ids_off: read::u32_at(data, 68)?,
            proto_ids_size: read::u32_at(data, 72)?,
            proto_ids_off: read::u32_at(data, 76)?,
            field_ids_size: read::u32_at(data, 80)?,
            field_ids_off: read::u32_at(data, 84)?,
            method_ids_size: read::u32_at(data, 88)?,
            method_ids_off: read::u32_at(data, 92)?,
            class_defs_size: read::u32_at(data, 96)?,
            class_defs_off: read::u32_at(data, 100)?,
            data_size: read::u32_at(data, 104)?,
            data_off: read::u32_at(data, 108)?,
        })
    }

    /// map_list: `u32 size` + size × (type u16, unused u16, size u32, offset u32).
    pub fn parse_map_list(data: &[u8], map_off: u32) -> RdResult<Vec<MapItem>> {
        if map_off == 0 {
            return Ok(Vec::new());
        }
        let mut r = read::Reader::at(data, map_off as usize)?;
        let size = r.u32()? as usize;
        // cada item tem 12 bytes
        if size.checked_mul(12).is_none() {
            return Err(RdError::parse("map_list: size overflow"));
        }
        if size > r.remaining() / 12 {
            return Err(RdError::parse(format!(
                "map_list: {} itens não cabem em {} bytes",
                size,
                r.remaining()
            )));
        }
        let mut items = Vec::with_capacity(size);
        for _ in 0..size {
            let raw_type = r.u16()?;
            let unused = r.u16()?;
            let size = r.u32()?;
            let offset = r.u32()?;
            items.push(MapItem {
                map_type: MapType::from_code(raw_type),
                unused,
                size,
                offset,
                raw_type,
            });
        }
        Ok(items)
    }

    // ── accessors de índices ────────────────────────────────────────────────

    pub fn string(&self, idx: u32) -> &str {
        self.strings.get_or_empty_with_data(idx, &self.data)
    }

    /// Compara `string_ids[idx]` com `s` sobre MUTF-8 cru sem alocar `String`.
    pub fn string_eq(&self, idx: u32, s: &str) -> RdResult<bool> {
        if let Some(cached) = self.strings.get(idx) {
            return Ok(cached == s);
        }
        match self.strings.raw_slice(idx, &self.data)? {
            Some(raw) => Ok(crate::mutf8::eq_str(raw, s)),
            None => Err(RdError::parse(format!(
                "string idx {idx} fora da tabela ({})",
                self.strings.len()
            ))),
        }
    }

    /// Descritor de tipo por índice (tolerante: índice inválido → "?").
    pub fn type_str(&self, idx: u32) -> &str {
        match self.type_ids.string_idx.get(idx as usize) {
            Some(&sidx) => self.string(sidx),
            None => "?",
        }
    }

    pub fn proto(&self, idx: u16) -> RdResult<ProtoId> {
        self.proto_ids
            .get(idx as usize)
            .copied()
            .ok_or_else(|| RdError::parse(format!("proto idx {idx} fora da tabela")))
    }

    /// Lista de parâmetros (índices de tipo) de um proto.
    pub fn proto_params(&self, proto_idx: u16) -> RdResult<Vec<u32>> {
        let p = self.proto(proto_idx)?;
        protos::type_list(&self.data, p.parameters_off)
    }

    pub fn field(&self, idx: u32) -> RdResult<FieldId> {
        self.field_ids
            .get(idx as usize)
            .copied()
            .ok_or_else(|| RdError::parse(format!("field idx {idx} fora da tabela")))
    }

    pub fn method(&self, idx: u32) -> RdResult<MethodId> {
        self.method_ids
            .get(idx as usize)
            .copied()
            .ok_or_else(|| RdError::parse(format!("method idx {idx} fora da tabela")))
    }

    // ── seções sob demanda ──────────────────────────────────────────────────

    /// class_data_item de uma classe (None se classe sem membros).
    pub fn class_data(&self, def: &ClassDef) -> RdResult<Option<ClassData>> {
        if def.class_data_off == 0 {
            return Ok(None);
        }
        classes::parse_class_data(&self.data, def.class_data_off).map(Some)
    }

    /// code_item em offset (offset 0 → None).
    pub fn code(&self, off: u32) -> RdResult<Option<code::CodeItem>> {
        if off == 0 {
            return Ok(None);
        }
        code::CodeItem::parse(&self.data, off).map(Some)
    }

    /// debug_info_item em offset (offset 0 → None).
    pub fn debug_info(&self, off: u32) -> RdResult<Option<debug::DebugInfo>> {
        if off == 0 {
            return Ok(None);
        }
        debug::parse_debug_info(&self.data, off).map(Some)
    }

    /// static_values encoded_array de uma classe.
    pub fn static_values(&self, def: &ClassDef) -> RdResult<Vec<EncodedValue>> {
        if def.static_values_off == 0 {
            return Ok(Vec::new());
        }
        crate::annotations::parse_encoded_array(&self.data, def.static_values_off)
    }

    /// Interfaces de uma classe (índices de tipo).
    pub fn interfaces(&self, def: &ClassDef) -> RdResult<Vec<u32>> {
        protos::type_list(&self.data, def.interfaces_off)
    }

    /// Class for a descriptor (`Lcom/x/Y;`). O(1) via a lazily built
    /// zero-String-allocation `RawClassIndex`; the first definition wins,
    /// matching the previous linear scan.
    pub fn find_class(&self, descriptor: &str) -> Option<&ClassDef> {
        let idx = self.class_index.get_or_init(|| {
            crate::lazy::RawClassIndex::build_from_dex(
                &self.data,
                &self.strings,
                &self.type_ids,
                &self.class_defs,
            )
        });
        idx.find_in_dex(
            descriptor,
            &self.data,
            &self.strings,
            &self.type_ids,
            &self.class_defs,
        )
        .map(|i| &self.class_defs[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um DEX mínimo sintético: header 035 + map_list vazio.
    fn minimal_dex() -> Vec<u8> {
        let mut d = vec![0u8; 112];
        d[0..4].copy_from_slice(b"dex\n");
        d[4..7].copy_from_slice(b"035");
        d[7] = 0;
        d[36..40].copy_from_slice(&112u32.to_le_bytes()); // header_size
                                                          // endian LE
        d[40..44].copy_from_slice(&ENDIAN_CONSTANT_LE.to_le_bytes());
        // file_size = 116 (header 112 + map list size 0)
        let map_off = 112u32;
        d[52..56].copy_from_slice(&map_off.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // map list size 0
        let file_size = d.len() as u32;
        d[32..36].copy_from_slice(&file_size.to_le_bytes());
        d
    }

    #[test]
    fn parses_minimal_dex() {
        let d = minimal_dex();
        let dex = Dex::parse(d.clone()).expect("parse");
        assert_eq!(dex.header.version_str(), "035");
        assert_eq!(dex.header.file_size as usize, d.len());
        assert!(dex.map_list.is_empty());
        assert!(dex.class_defs.is_empty());
        assert!(dex.strings.is_empty());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut d = minimal_dex();
        d[0] = b'X';
        let e = Dex::parse(d).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
    }

    #[test]
    fn rejects_big_endian() {
        let mut d = minimal_dex();
        d[40..44].copy_from_slice(&ENDIAN_CONSTANT_BE.to_le_bytes());
        assert!(Dex::parse(d).is_err());
    }

    #[test]
    fn rejects_truncated() {
        assert!(Dex::parse(vec![0u8; 64]).is_err());
    }

    #[test]
    fn map_list_roundtrip() {
        let mut d = minimal_dex();
        // espaço p/ 1 item de map (12 bytes) além dos 4 de size já no buffer
        d.extend_from_slice(&[0u8; 12]);
        let file_size = d.len() as u32;
        d[32..36].copy_from_slice(&file_size.to_le_bytes());
        // substitui map list: 1 item (type_list @ 0x80, 2 itens)
        let pos = 112;
        d[pos..pos + 4].copy_from_slice(&1u32.to_le_bytes());
        d[pos + 4..pos + 6].copy_from_slice(&0x1001u16.to_le_bytes());
        d[pos + 6..pos + 8].copy_from_slice(&0u16.to_le_bytes());
        d[pos + 8..pos + 12].copy_from_slice(&2u32.to_le_bytes());
        d[pos + 12..pos + 16].copy_from_slice(&0x80u32.to_le_bytes());
        let dex = Dex::parse(d).expect("parse");
        assert_eq!(dex.map_list.len(), 1);
        assert_eq!(dex.map_list[0].map_type, MapType::TypeList);
        assert_eq!(dex.map_list[0].size, 2);
        assert_eq!(dex.map_list[0].offset, 0x80);
    }

    #[test]
    fn adler32_known_vector() {
        // referência zlib: adler32("Wikipedia") = 0x11E60398
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn map_type_names_and_codes() {
        for code in [
            0x0000u16, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x1000, 0x1001, 0x1002,
            0x1003, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x0007, 0x0008, 0xF000,
        ] {
            let t = MapType::from_code(code);
            assert_eq!(t.code(), code, "roundtrip de 0x{code:04x}");
            assert!(!t.name().is_empty());
        }
        // o roundtrip sozinho passa vazio para Unknown (Unknown(c).code() == c);
        // from_code precisa reconhecer os códigos de verdade (issue #10)
        // códigos oficiais da spec: 0x0007/0x0008 (issue #21 — 0x7000/0x7001
        // não existem na tabela de type codes do dex-format)
        assert!(matches!(MapType::from_code(0x0007), MapType::CallSiteId));
        assert!(matches!(MapType::from_code(0x0008), MapType::MethodHandle));
        assert!(matches!(
            MapType::from_code(0x7000),
            MapType::Unknown(0x7000)
        ));
        assert!(matches!(
            MapType::from_code(0x7001),
            MapType::Unknown(0x7001)
        ));
        assert!(matches!(
            MapType::from_code(0xF000),
            MapType::HiddenapiClassData
        ));
        assert!(matches!(
            MapType::from_code(0x9999),
            MapType::Unknown(0x9999)
        ));
    }
}
