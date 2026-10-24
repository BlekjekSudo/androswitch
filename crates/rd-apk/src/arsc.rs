//! resources.arsc — parse mínimo porém correto (ResTable) + resolução de recursos.
//!
//! Escopo M0: resolver **strings** (labels) e **nomes** (`@type/key`) por resource id,
//! com preferência pela config default. Valores complexos são marcados (`complex`)
//! sem desdobrar bag/maps — suficiente para o inspect e para o motor de permissões.

use crate::axml::ComplexUnit;
use crate::error::{RdError, RdResult};
use crate::pool::StringPool;
use alloc::{borrow::ToOwned, collections::BTreeMap, format, string::String, vec::Vec};

pub const RES_TABLE_TYPE: u16 = 0x0002;
pub const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
pub const RES_TABLE_TYPE_SPEC: u16 = 0x0202;
pub const RES_TABLE_TYPE_CHUNK: u16 = 0x0201;

const FLAG_SPARSE: u8 = 0x01;
const FLAG_OFFSET16: u8 = 0x02;
const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
#[path = "arsc_identifier.rs"]
mod identifier;
pub use identifier::{identifier_name, identifier_number};

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    d.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("arsc: truncated u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("arsc: truncated u32 @ {off}")))
}

/// Config simplificada — só o que o M0 precisa para escolher a variante certa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResConfigInfo {
    pub language: Option<String>,
    pub country: Option<String>,
    pub density: Option<u16>,
    pub api_level: Option<u16>,
    /// Raw configuration contains qualifiers beyond locale, density and SDK.
    /// Existing inspection selectors retain their behavior; consumers requiring
    /// exact supported configuration matching can explicitly decline these.
    pub unsupported_qualifiers: bool,
}

impl ResConfigInfo {
    pub fn is_default(&self) -> bool {
        self.language.is_none() && self.density.map_or(true, |d| d == 0 || d == 160)
    }
    fn specificity(&self) -> u8 {
        let mut s = 0;
        if self.language.is_some() {
            s += 2;
        }
        if self.country.is_some() {
            s += 1;
        }
        s
    }
}

/// Uma entrada de um bag/mapa de recurso (por exemplo, um atributo de style).
#[derive(Debug, Clone, PartialEq)]
pub struct ResMapEntry {
    /// Nome/resource id do atributo dentro do bag.
    pub name: u32,
    pub data_type: u8,
    pub data: u32,
    /// Pré-resolvida quando `data_type == 3` (STRING → pool global).
    pub string: Option<String>,
}

/// Uma entrada de recurso resolvida para uma config.
#[derive(Debug, Clone)]
pub struct ResEntry {
    pub config: ResConfigInfo,
    pub data_type: u8,
    pub data: u32,
    /// Pré-resolvida quando `data_type == 3` (STRING → pool global).
    pub string: Option<String>,
    pub complex: bool,
    /// Parent style resource id, when this entry is a complex bag.
    pub parent: Option<u32>,
    /// Resource map values when this entry is a complex bag (style/array/etc.).
    pub bag: Option<Vec<ResMapEntry>>,
    /// Índice no key string pool (para `resolve_name`).
    pub key_idx: u32,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub id: u8,
    pub name: String,
    /// Added to each on-disk type id; absent in legacy 284-byte headers.
    pub type_id_offset: u8,
    pub type_strings: StringPool,
    pub key_strings: StringPool,
    /// resid completo → entradas por config.
    pub entries: BTreeMap<u32, Vec<ResEntry>>,
}

/// The identity of an entry in its owning table, independent of value/config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceName<'a> {
    pub package: &'a str,
    pub resource_type: &'a str,
    pub entry: &'a str,
}

#[derive(Debug, Clone, Default)]
pub struct Arsc {
    pub global_strings: StringPool,
    pub packages: Vec<Package>,
}

impl Package {
    pub fn resource_name(&self, res_id: u32) -> Option<ResourceName<'_>> {
        if self.id == 0 || (res_id >> 24) != u32::from(self.id) {
            return None;
        }
        let raw_type = ((res_id >> 16) as u8)
            .checked_sub(self.type_id_offset)?
            .checked_sub(1)?;
        let entries = self.entries.get(&res_id)?;
        let first = entries.first()?;
        // Configuration variants describe the same entry. Conflicting key
        // indexes are malformed metadata, not a reason to guess an identity.
        if entries.iter().any(|e| e.key_idx != first.key_idx) {
            return None;
        }
        let resource_type = self.type_strings.get(u32::from(raw_type))?;
        let entry = self.key_strings.get(first.key_idx)?;
        if self.name.is_empty() || resource_type.is_empty() || entry.is_empty() {
            return None;
        }
        Some(ResourceName {
            package: &self.name,
            resource_type,
            entry,
        })
    }
    /// Nome qualificado `@type/key` de um resid (sem pacote).
    pub fn resolve_name(&self, res_id: u32) -> Option<String> {
        let name = self.resource_name(res_id)?;
        Some(format!("@{}/{}", name.resource_type, name.entry))
    }
}

/// issue #61: valor TIPADO de uma entrada de recurso (melhor config) — o que
/// o LayoutInflater precisa para resolver `@dimen`/`@bool` por referência.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedValue {
    Str(String),
    Boolean(bool),
    Integer(i32),
    Float(f32),
    /// Valor complexo (mantissa×radix) com unidade — px/dp/sp/…
    Dimension {
        value: f32,
        unit: ComplexUnit,
    },
    /// Referência a outro recurso (resid) — o caller decide se segue a cadeia.
    Reference(u32),
    /// Referência a um atributo de tema (`?attr/...`) inside a style bag.
    Attribute(u32),
}

/// complexToFloat (TypedValue) — mesma matemática do axml (mantissa bits
/// 8–31 com sinal no bit 31, radix bits 4–5).
fn complex_to_float(data: u32) -> f32 {
    const RADIX_MULTS: [f32; 4] = [
        0.003_906_25,   // radix 0: 2⁻⁸
        3.051_758e-5,   // radix 1: 2⁻¹⁵
        1.192_092_9e-7, // radix 2: 2⁻²³
        4.656_613e-10,  // radix 3: 2⁻³¹
    ];
    let mantissa = (data & 0xFFFF_FF00) as i32;
    mantissa as f32 * RADIX_MULTS[((data >> 4) & 0x3) as usize]
}

/// Unidade de um Res_value complexo (bits 0–3 — mesma tabela do axml).
fn complex_unit_of(data: u32) -> ComplexUnit {
    match (data & 0xF) as u8 {
        0 => ComplexUnit::Px,
        1 => ComplexUnit::Dip,
        2 => ComplexUnit::Sp,
        3 => ComplexUnit::Pt,
        4 => ComplexUnit::In,
        5 => ComplexUnit::Mm,
        other => ComplexUnit::Unknown(other),
    }
}

impl Arsc {
    pub fn parse(data: &[u8]) -> RdResult<Arsc> {
        if data.len() < 12 {
            return Err(RdError::parse("arsc: shorter than ResTable header"));
        }
        if u16_at(data, 0)? != RES_TABLE_TYPE {
            return Err(RdError::parse(format!(
                "arsc: expected type 0x{RES_TABLE_TYPE:04x}, got 0x{:04x}",
                u16_at(data, 0)?
            )));
        }
        let _header_size = u16_at(data, 2)? as usize;
        let file_size = u32_at(data, 4)? as usize;
        let data = &data[..file_size.min(data.len())];
        let package_count = u32_at(data, 8)?;

        // pool global logo após o header (12 bytes)
        let (global_strings, pool_used) = StringPool::parse(data, 12)?;
        let mut p = 12 + pool_used as usize;

        let mut packages = Vec::new();
        let mut seen_packages = 0u32;
        while p + 8 <= data.len() && seen_packages < package_count {
            let ctype = u16_at(data, p)?;
            let cheader = u16_at(data, p + 2)? as usize;
            let csize = u32_at(data, p + 4)? as usize;
            // progresso garantido: chunk mínimo = 8 bytes (mesmo guard dos
            // irmãos em axml.rs e no loop interno de parse_package — sem ele,
            // csize=0 com packageCount≥1 gira eternamente; issue #9)
            if csize < 8 {
                return Err(RdError::parse(format!(
                    "arsc: chunk at {p} declares size {csize} < 8 (no progress possible)"
                )));
            }
            if csize < cheader || p + csize > data.len() {
                return Err(RdError::parse(format!(
                    "arsc: package chunk at {p} declares {csize} beyond bounds"
                )));
            }
            if ctype == RES_TABLE_PACKAGE_TYPE {
                let pkg_header_size = u16_at(data, p + 2)? as usize;
                packages.push(parse_package(
                    data,
                    p,
                    csize,
                    pkg_header_size,
                    &global_strings,
                )?);
                seen_packages += 1;
            }
            p += csize;
        }
        if seen_packages != package_count {
            return Err(RdError::invalid_format(format!(
                "arsc: header announces {package_count} packages, walked {seen_packages}"
            )));
        }

        Ok(Arsc {
            global_strings,
            packages,
        })
    }

    /// Resolve o resource id para string, seguindo cadeias de referência
    /// (ex.: `@string/app_name` → alias → texto). Limite de 8 saltos contra ciclos.
    pub fn resolve_string(&self, res_id: u32) -> Option<String> {
        self.resolve_string_depth(res_id, 0)
    }

    fn resolve_string_depth(&self, res_id: u32, depth: usize) -> Option<String> {
        if depth > 8 {
            return None;
        }
        let pkg = self.package_for(res_id)?;
        let entries = pkg.entries.get(&res_id)?;
        let best = entries
            .iter()
            .min_by_key(|e| (e.config.specificity(), !e.config.is_default()))?;
        match best.data_type {
            3 => best.string.clone(),
            1 => self.resolve_string_depth(best.data, depth + 1), // REFERENCE
            _ => None,
        }
    }

    /// `@type/key` do recurso (primeiro pacote que o conhece).
    pub fn resolve_name(&self, res_id: u32) -> Option<String> {
        let name = self.resource_name(res_id)?;
        Some(format!("@{}/{}", name.resource_type, name.entry))
    }

    /// Lookup does not follow aliases or select a value configuration. Table
    /// fragments must agree on the identity; conflicting owners are ambiguous.
    pub fn resource_name(&self, res_id: u32) -> Option<ResourceName<'_>> {
        let mut found = None;
        for package in self
            .packages
            .iter()
            .filter(|p| p.entries.contains_key(&res_id))
        {
            let candidate = package.resource_name(res_id)?;
            if found.is_some_and(|name| name != candidate) {
                return None;
            }
            found = Some(candidate);
        }
        found
    }

    /// issue #61: valor TIPADO da melhor config de um resid. `None` = resid
    /// desconhecido ou tipo não representável (bag/complex — o caller falha
    /// tipado; nunca default silencioso).
    pub fn resolve_value(&self, res_id: u32) -> Option<ResolvedValue> {
        self.resolve_value_version(res_id, None)
    }
    /// Runtime selection: retain locale/density precedence, choose the newest
    /// eligible SDK variant, and never use an entry requiring a newer SDK.
    /// This does not implement Android's remaining configuration qualifiers.
    pub fn resolve_value_for_api(&self, res_id: u32, api: u16) -> Option<ResolvedValue> {
        self.resolve_value_version(res_id, Some(api))
    }
    fn resolve_value_version(&self, res_id: u32, api: Option<u16>) -> Option<ResolvedValue> {
        let pkg = self.package_for(res_id)?;
        let entries = pkg.entries.get(&res_id)?;
        let best = entries
            .iter()
            .filter(|e| api.map_or(true, |api| e.config.api_level.unwrap_or(0) <= api))
            .min_by_key(|e| {
                (
                    e.config.specificity(),
                    !e.config.is_default(),
                    core::cmp::Reverse(api.map_or(0, |_| e.config.api_level.unwrap_or(0))),
                )
            })?;
        match best.data_type {
            0x03 => best.string.clone().map(ResolvedValue::Str),
            0x01 => Some(ResolvedValue::Reference(best.data)),
            0x02 => Some(ResolvedValue::Attribute(best.data)),
            // INT_DEC (u32 com semântica i32 — TypedValue.data é cru)
            0x10 => Some(ResolvedValue::Integer(best.data as i32)),
            0x11 => Some(ResolvedValue::Integer(best.data as i32)),
            0x12 => Some(ResolvedValue::Boolean(best.data != 0)),
            // INT_COLOR_ARGB8/RGB8/ARGB4/RGB4: aapt2 stores the expanded ARGB
            0x1c..=0x1f => Some(ResolvedValue::Integer(best.data as i32)),
            0x04 => Some(ResolvedValue::Float(f32::from_bits(best.data))),
            0x05 => Some(ResolvedValue::Dimension {
                value: complex_to_float(best.data),
                unit: complex_unit_of(best.data),
            }),
            _ => None,
        }
    }

    /// Resolve one attribute from a style, including its parent style and
    /// `?attr/…` aliases. Resource references remain explicit so the VM can
    /// dereference them using the APK's normal resource policy.
    pub fn resolve_style_attribute(&self, style_id: u32, attr_id: u32) -> Option<ResolvedValue> {
        self.resolve_style_attribute_inner(style_id, attr_id, 0)
    }

    fn resolve_style_attribute_inner(
        &self,
        style_id: u32,
        attr_id: u32,
        depth: usize,
    ) -> Option<ResolvedValue> {
        if depth >= 64 {
            return None;
        }
        let pkg = self.package_for(style_id)?;
        let entries = pkg.entries.get(&style_id)?;
        let style = entries
            .iter()
            .min_by_key(|e| (e.config.specificity(), !e.config.is_default()))?;
        if let Some(value) = style
            .bag
            .as_ref()
            .and_then(|bag| bag.iter().find(|value| value.name == attr_id))
        {
            let resolved = match value.data_type {
                0x01 => ResolvedValue::Reference(value.data),
                0x02 => ResolvedValue::Attribute(value.data),
                0x03 => ResolvedValue::Str(value.string.clone()?),
                0x04 => ResolvedValue::Float(f32::from_bits(value.data)),
                0x05 => ResolvedValue::Dimension {
                    value: complex_to_float(value.data),
                    unit: complex_unit_of(value.data),
                },
                0x10 | 0x11 | 0x1c..=0x1f => ResolvedValue::Integer(value.data as i32),
                0x12 => ResolvedValue::Boolean(value.data != 0),
                _ => return None,
            };
            return match resolved {
                ResolvedValue::Attribute(next) => {
                    self.resolve_style_attribute_inner(style_id, next, depth + 1)
                }
                other => Some(other),
            };
        }
        let parent = style.parent?;
        self.resolve_style_attribute_inner(parent, attr_id, depth + 1)
    }

    pub fn package_for(&self, res_id: u32) -> Option<&Package> {
        let pid = ((res_id >> 24) & 0xFF) as u8;
        // issue #54: SEM fallback para o primeiro package — um arsc hostil
        // não pode nomear ids de package estranho (ex.: 0x01 = framework
        // android:id/…). Package id inexistente → resolução honesta falha.
        self.packages.iter().find(|p| p.id == pid)
    }
}

fn parse_package(
    data: &[u8],
    base: usize,
    size: usize,
    header_size: usize,
    global: &StringPool,
) -> RdResult<Package> {
    let end = base
        .checked_add(size)
        .ok_or_else(|| RdError::parse("arsc: package size overflow"))?;
    if header_size < 284 || header_size > size || end > data.len() {
        return Err(RdError::parse("arsc: package chunk truncated (header)"));
    }
    // Every read below is confined to the package that owns the metadata.
    let data = &data[..end];
    let pkg_id = u8::try_from(u32_at(data, base + 8)?)
        .map_err(|_| RdError::parse("arsc: package id exceeds 8 bits"))?;
    let type_id_offset = if header_size >= 288 {
        u8::try_from(u32_at(data, base + 284)?)
            .map_err(|_| RdError::parse("arsc: typeIdOffset exceeds 8 bits"))?
    } else {
        0
    };
    let type_strings_off = u32_at(data, base + 268)? as usize;
    let key_strings_off = u32_at(data, base + 276)? as usize;

    // name: 128 UTF-16 units @12
    let mut name_units = Vec::with_capacity(128);
    for i in 0..128 {
        let u = u16_at(data, base + 12 + i * 2)?;
        if u == 0 {
            break;
        }
        name_units.push(u);
    }
    let name = String::from_utf16_lossy(&name_units);

    let symbol_pool = |offset: usize| -> RdResult<StringPool> {
        // Zero denotes inherited overlay pools. Overlay/dynamic package
        // composition is not implemented, so do not invent owning names.
        if offset < header_size || offset > size.saturating_sub(8) {
            return Err(RdError::parse("arsc: symbol pool outside owning package"));
        }
        let (pool, used) = StringPool::parse(data, base + offset)?;
        if used as usize > size - offset {
            return Err(RdError::parse("arsc: symbol pool exceeds owning package"));
        }
        Ok(pool)
    };
    let type_strings = symbol_pool(type_strings_off)?;
    let key_strings = symbol_pool(key_strings_off)?;

    let mut entries: BTreeMap<u32, Vec<ResEntry>> = BTreeMap::new();

    // caminhar chunks internos a partir do fim do header real do pacote
    let mut p = base + header_size;
    while p + 8 <= end {
        let ctype = u16_at(data, p)?;
        let cheader = u16_at(data, p + 2)? as usize;
        let csize = u32_at(data, p + 4)? as usize;
        // progresso garantido (chunk mínimo = 8 bytes) — endurece contra fuzz
        if csize < 8 || csize < cheader || p + csize > end {
            break; // chunk truncado: não fatal para o M0 — o que veio antes fica
        }
        if ctype == RES_TABLE_TYPE_CHUNK {
            if cheader < 20 {
                return Err(RdError::parse("arsc: type header shorter than 20 bytes"));
            }
            let type_id = data.get(p + 8).copied().unwrap_or(0);
            let effective_type = type_id
                .checked_add(type_id_offset)
                .filter(|_| type_id != 0)
                .ok_or_else(|| RdError::parse("arsc: invalid or overflowing type id"))?;
            let flags = data.get(p + 9).copied().unwrap_or(0);
            let entry_count = u32_at(data, p + 12)? as usize;
            if entry_count > 65_536 {
                return Err(RdError::parse(
                    "arsc: entry count exceeds 16-bit resource ids",
                ));
            }
            let entries_start = u32_at(data, p + 16)? as usize;

            // issue #29: ResTable_type tem no mínimo 20 bytes de header
            // (8 do ResChunk_header + id/res0/res1 + entryCount + entriesStart);
            // cheader < 20 causava underflow `cheader - 20` (pânico em
            // overflow-checks, config garbage em release)
            if cheader < 20 {
                break; // chunk malformado: não fatal — o que veio antes fica
            }
            let config = parse_config(data, p + 20, cheader - 20)?;

            let entries_abs = p + entries_start;
            let mut collect = |idx: usize, rel_off: u32| -> RdResult<()> {
                if rel_off == 0xFFFF_FFFF {
                    return Ok(());
                }
                if idx > u16::MAX as usize {
                    return Err(RdError::parse("arsc: resource entry index exceeds 16 bits"));
                }
                let eoff = entries_abs + rel_off as usize;
                if eoff + 8 > p + csize {
                    return Ok(()); // entrada truncada: skip
                }
                let e_size = u16_at(data, eoff)? as usize;
                let e_flags = u16_at(data, eoff + 2)?;
                let key_idx = u32_at(data, eoff + 4)?;
                let (data_type, ddata, complex, parent, bag) = if e_flags & ENTRY_FLAG_COMPLEX != 0
                {
                    let parent = if e_size >= 16 && eoff + 16 <= p + csize {
                        let parent = u32_at(data, eoff + 8)?;
                        (parent != 0).then_some(parent)
                    } else {
                        None
                    };
                    let map_count = if e_size >= 16 && eoff + 16 <= p + csize {
                        u32_at(data, eoff + 12)? as usize
                    } else {
                        usize::MAX
                    };
                    let maps_start = eoff.saturating_add(e_size);
                    let available = (p + csize).saturating_sub(maps_start) / 12;
                    let bag = if map_count <= 65_536 && map_count <= available {
                        let mut bag = Vec::with_capacity(map_count);
                        for i in 0..map_count {
                            let m = maps_start + i * 12;
                            let value_size = u16_at(data, m + 4)?;
                            if value_size < 8 {
                                bag.clear();
                                break;
                            }
                            let name = u32_at(data, m)?;
                            let dtype = *data.get(m + 7).unwrap_or(&0);
                            let value = u32_at(data, m + 8)?;
                            bag.push(ResMapEntry {
                                name,
                                data_type: dtype,
                                data: value,
                                string: (dtype == 3)
                                    .then(|| global.get(value).map(str::to_owned))
                                    .flatten(),
                            });
                        }
                        (bag.len() == map_count).then_some(bag)
                    } else {
                        None
                    };
                    (0u8, 0u32, true, parent, bag)
                } else {
                    // Res_value após ResTable_entry (e_size bytes)
                    let Some(v) = eoff.checked_add(e_size) else {
                        return Ok(());
                    };
                    if v + 8 > p + csize || v + 8 > data.len() {
                        return Ok(());
                    }
                    let dtype = *data.get(v + 3).unwrap_or(&0);
                    let d = u32_at(data, v + 4)?;
                    (dtype, d, false, None, None)
                };
                let string = if data_type == 3 && !complex {
                    global.get(ddata).map(str::to_owned)
                } else {
                    None
                };
                let resid = ((pkg_id as u32) << 24) | ((effective_type as u32) << 16) | idx as u32;
                entries.entry(resid).or_default().push(ResEntry {
                    config: config.clone(),
                    data_type,
                    data: ddata,
                    string,
                    complex,
                    parent,
                    bag,
                    key_idx,
                });
                Ok(())
            };

            if flags & FLAG_SPARSE != 0 {
                // ResTable_sparseTypeEntry: {idx u16, offset u16} — offset/4
                let pairs = entry_count;
                for i in 0..pairs {
                    let o = p + cheader + i * 4;
                    if o + 4 > data.len() {
                        break;
                    }
                    let idx = u16_at(data, o)? as usize;
                    let off4 = u16_at(data, o + 2)? as u32;
                    collect(idx, off4 * 4)?;
                }
            } else if flags & FLAG_OFFSET16 != 0 {
                for i in 0..entry_count {
                    let o = p + cheader + i * 2;
                    if o + 2 > data.len() {
                        break;
                    }
                    let offset = u16_at(data, o)?;
                    if offset != u16::MAX {
                        collect(i, u32::from(offset) * 4)?;
                    }
                }
            } else {
                for i in 0..entry_count {
                    let o = p + cheader + i * 4;
                    if o + 4 > data.len() {
                        break;
                    }
                    collect(i, u32_at(data, o)?)?;
                }
            }
        }
        p += csize;
    }

    Ok(Package {
        id: pkg_id,
        name,
        type_id_offset,
        type_strings,
        key_strings,
        entries,
    })
}

/// ResTable_config — extrai locale/densidade/api sem reclamar de variações de tamanho.
///
/// Layout do `ResTable_config` (frameworks/base/libs/androidfw/include/androidfw/
/// ResourceTypes.h) — offsets conferidos campo a campo:
///
/// ```text
/// size:u32@0 | mcc:u16@4 mnc:u16@6 | language[2]@8 country[2]@10
/// orientation:u8@12 touchscreen:u8@13 density:u16@14
/// keyboard:u8@16 navigation:u8@17 inputFlags:u8@18 pad0:u8@19
/// screenWidth:u16@20 screenHeight:u16@22
/// sdkVersion:u16@24 minorVersion:u16@26
/// screenLayout:u8@28 uiMode:u8@29 smallestScreenWidthDp:u16@30 (se size ≥ 32)
/// ```
///
/// `api_level` é o `sdkVersion` (u16 @24 — o mesmo campo que dá nome às configs
/// "v26" do aapt2). NÃO é `screenHeight` (u16 @22); o bloco [20..24) é o par
/// screenWidth/screenHeight de 2+2 bytes, não dois u32 (verificado empiricamente
/// contra resources.arsc reais — configs "vNN" decodificam com o valor da API).
fn parse_config(data: &[u8], cfg_base: usize, cfg_max: usize) -> RdResult<ResConfigInfo> {
    let mut cfg = ResConfigInfo::default();
    if cfg_max < 4 || cfg_base + 4 > data.len() {
        cfg.unsupported_qualifiers = true;
        return Ok(cfg);
    }
    let cfg_size = u32_at(data, cfg_base)? as usize;
    let readable = cfg_size
        .min(cfg_max)
        .min(data.len().saturating_sub(cfg_base));
    cfg.unsupported_qualifiers = cfg_size < 4
        || cfg_size > readable
        || (4..readable).any(|offset| {
            !matches!(offset, 8..=11 | 14..=15 | 24..=25) && data[cfg_base + offset] != 0
        });
    let lang = |off: usize| -> Option<String> {
        let b = data.get(cfg_base + off..cfg_base + off + 2)?;
        if b[0] == 0 && b[1] == 0 {
            None
        } else {
            // estilo AOSP: 0 se byte < 128, senão 'a' + (b - 128) (packed unicode é raro)
            let chars: Vec<char> = b
                .iter()
                .filter(|&&c| c != 0)
                .map(|&c| {
                    if c < 128 {
                        c as char
                    } else {
                        char::from_u32('a' as u32 + (c - 128) as u32).unwrap_or('?')
                    }
                })
                .collect();
            Some(chars.into_iter().collect())
        }
    };
    if readable >= 12 {
        cfg.language = lang(8);
        cfg.country = lang(10);
    }
    if readable >= 16 {
        let d = u16_at(data, cfg_base + 14)?;
        cfg.density = Some(d);
    }
    if readable >= 26 {
        // sdkVersion:u16 @24 (minorVersion:u16 @26 — não lido)
        cfg.api_level = Some(u16_at(data, cfg_base + 24)?);
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_consumers_can_detect_unmodeled_raw_resource_qualifiers() {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&64u32.to_le_bytes());
        data[14..16].copy_from_slice(&320u16.to_le_bytes());
        data[24..26].copy_from_slice(&26u16.to_le_bytes());
        assert!(!parse_config(&data, 0, 64).unwrap().unsupported_qualifiers);
        for offset in [4, 12, 13, 16, 20, 26, 28, 36, 60] {
            data[offset] = 1;
            assert!(parse_config(&data, 0, 64).unwrap().unsupported_qualifiers);
            data[offset] = 0;
        }
        assert!(parse_config(&data, 0, 32).unwrap().unsupported_qualifiers);
        data[8] = b'f';
        data[9] = b'r';
        let config = parse_config(&data, 0, 64).unwrap();
        assert_eq!(config.language.as_deref(), Some("fr"));
        assert!(!config.unsupported_qualifiers);
    }

    fn empty_global_pool() -> Vec<u8> {
        // ResStringPool vazio válido: 28 bytes de header, 0 strings
        let mut p = Vec::new();
        p.extend_from_slice(&0x0001u16.to_le_bytes()); // type
        p.extend_from_slice(&28u16.to_le_bytes()); // headerSize
        p.extend_from_slice(&28u32.to_le_bytes()); // size
        p.extend_from_slice(&0u32.to_le_bytes()); // stringCount
        p.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        p.extend_from_slice(&0u32.to_le_bytes()); // flags (UTF-16)
        p.extend_from_slice(&28u32.to_le_bytes()); // stringsStart
        p.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        p
    }

    /// PoC da issue #9: 48 bytes = header ResTable (packageCount=1) + pool
    /// global vazia + chunk {type=0x0202, headerSize=0, size=0}. Sem o guard
    /// `csize < 8` o loop externo não progride (`p += 0`) e trava para sempre
    /// (hang, não crash). Deve retornar erro tipado em tempo finito.
    #[test]
    fn zero_size_chunk_is_typed_error_not_hang() {
        let mut d = Vec::new();
        d.extend_from_slice(&0x0002u16.to_le_bytes()); // RES_TABLE_TYPE
        d.extend_from_slice(&12u16.to_le_bytes()); // headerSize
        d.extend_from_slice(&48u32.to_le_bytes()); // file size
        d.extend_from_slice(&1u32.to_le_bytes()); // packageCount = 1
        assert_eq!(d.len(), 12);
        d.extend(empty_global_pool());
        assert_eq!(d.len(), 40);
        d.extend_from_slice(&0x0202u16.to_le_bytes()); // tipo ≠ package
        d.extend_from_slice(&0u16.to_le_bytes()); // headerSize = 0
        d.extend_from_slice(&0u32.to_le_bytes()); // size = 0 → sem progresso
        assert_eq!(d.len(), 48);

        let e = Arsc::parse(&d).expect_err("csize=0 deve ser rejeitado");
        assert_eq!(e.code, "PARSE_ERROR");
        assert!(e.cause.contains("< 8"), "mensagem: {}", e.cause);
    }

    /// Issue #16: `api_level` é o `sdkVersion` u16 @24 do ResTable_config —
    /// NÃO é screenHeight (@22). screenHeight=1440 (0x5A0) NÃO pode vazar
    /// para api_level; sdkVersion=26 deve decodificar como 26 (config "v26").
    #[test]
    fn config_api_level_is_sdk_version_u16_at_offset_24() {
        let mut cfg = vec![0u8; 28];
        cfg[0..4].copy_from_slice(&28u32.to_le_bytes()); // size
        cfg[8..10].copy_from_slice(b"en"); // language
        cfg[10..12].copy_from_slice(b"US"); // country
        cfg[14..16].copy_from_slice(&256u16.to_le_bytes()); // density
        cfg[20..22].copy_from_slice(&720u16.to_le_bytes()); // screenWidth
        cfg[22..24].copy_from_slice(&1440u16.to_le_bytes()); // screenHeight
        cfg[24..26].copy_from_slice(&26u16.to_le_bytes()); // sdkVersion

        let info = parse_config(&cfg, 0, cfg.len()).expect("config");
        assert_eq!(
            info.api_level,
            Some(26),
            "sdkVersion@24, não screenHeight@22"
        );
        assert_eq!(info.density, Some(256));
        assert_eq!(info.language.as_deref(), Some("en"));
        assert_eq!(info.country.as_deref(), Some("US"));
    }
    #[test]
    fn runtime_resource_selects_newest_eligible_sdk_without_changing_inspection() {
        let id = 0x7f020000;
        let variant = |api, label: &str| ResEntry {
            config: ResConfigInfo {
                api_level: api,
                ..Default::default()
            },
            data_type: 3,
            data: 0,
            string: Some(label.into()),
            complex: false,
            parent: None,
            bag: None,
            key_idx: 0,
        };
        let mut entries = BTreeMap::new();
        entries.insert(
            id,
            vec![
                variant(None, "base"),
                variant(Some(1), "v1"),
                variant(Some(26), "v26"),
                variant(Some(27), "future"),
            ],
        );
        let arsc = Arsc {
            global_strings: Default::default(),
            packages: vec![Package {
                id: 0x7f,
                name: "test".into(),
                type_id_offset: 0,
                type_strings: Default::default(),
                key_strings: Default::default(),
                entries,
            }],
        };
        assert_eq!(
            arsc.resolve_value(id),
            Some(ResolvedValue::Str("base".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 25),
            Some(ResolvedValue::Str("v1".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 26),
            Some(ResolvedValue::Str("v26".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 27),
            Some(ResolvedValue::Str("future".into()))
        );
    }

    #[test]
    fn style_attributes_follow_parent_and_theme_attribute_aliases() {
        let parent = 0x7f12_0001;
        let child = 0x7f12_0002;
        let primary = 0x7f04_0001;
        let alias = 0x7f04_0002;
        let inherited = 0x7f04_0003;
        let absent = 0x7f04_0004;
        let entry = |parent, bag| ResEntry {
            config: ResConfigInfo::default(),
            data_type: 0,
            data: 0,
            string: None,
            complex: true,
            parent,
            bag: Some(bag),
            key_idx: 0,
        };
        let mut entries = BTreeMap::new();
        entries.insert(
            parent,
            vec![entry(
                None,
                vec![
                    ResMapEntry {
                        name: primary,
                        data_type: 0x12,
                        data: 1,
                        string: None,
                    },
                    ResMapEntry {
                        name: inherited,
                        data_type: 0x12,
                        data: 1,
                        string: None,
                    },
                ],
            )],
        );
        entries.insert(
            child,
            vec![entry(
                Some(parent),
                vec![
                    ResMapEntry {
                        name: primary,
                        data_type: 0x12,
                        data: 0,
                        string: None,
                    },
                    ResMapEntry {
                        name: alias,
                        data_type: 0x02,
                        data: primary,
                        string: None,
                    },
                ],
            )],
        );
        let arsc = Arsc {
            global_strings: StringPool::default(),
            packages: vec![Package {
                id: 0x7f,
                name: "test".into(),
                type_id_offset: 0,
                type_strings: StringPool::default(),
                key_strings: StringPool::default(),
                entries,
            }],
        };
        assert_eq!(
            arsc.resolve_style_attribute(child, primary),
            Some(ResolvedValue::Boolean(false)),
            "the child style overrides its parent"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, alias),
            Some(ResolvedValue::Boolean(false)),
            "?attr alias resolves through the child's effective theme"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, inherited),
            Some(ResolvedValue::Boolean(true)),
            "a missing child attribute is inherited from the parent"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, absent),
            None,
            "absent attributes stay absent"
        );
    }
}
