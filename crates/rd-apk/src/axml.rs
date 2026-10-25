//! AXML — AndroidManifest.xml binário (ResXMLTree) → árvore genérica tipada.
//!
//! Formato: source.android.com → "XML resource" + ResChunk_header chain.
//! O modelo genérico preserva **todos** os elementos e atributos (Lei 2:
//! "manifest é a verdade") — as views tipadas ficam em `manifest.rs`.

use crate::error::{RdError, RdResult};
use crate::pool::{StringPool, RES_STRING_POOL_TYPE};

pub const RES_XML_TYPE: u16 = 0x0003;
pub const RES_XML_RESOURCE_MAP_TYPE: u16 = 0x0180;
pub const RES_XML_START_NAMESPACE_TYPE: u16 = 0x0100;
pub const RES_XML_END_NAMESPACE_TYPE: u16 = 0x0101;
pub const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
pub const RES_XML_END_ELEMENT_TYPE: u16 = 0x0103;
pub const RES_XML_CDATA_TYPE: u16 = 0x0104;

pub const NS_ANDROID: &str = "http://schemas.android.com/apk/res/android";

// typed value data types (Res_value.dataType)
pub const TYPE_NULL: u8 = 0x00;
pub const TYPE_REFERENCE: u8 = 0x01;
pub const TYPE_ATTRIBUTE: u8 = 0x02;
pub const TYPE_STRING: u8 = 0x03;
pub const TYPE_FLOAT: u8 = 0x04;
pub const TYPE_DIMENSION: u8 = 0x05;
pub const TYPE_FRACTION: u8 = 0x06;
pub const TYPE_FIRST_INT: u8 = 0x10;
pub const TYPE_LAST_INT: u8 = 0x1F;
pub const TYPE_BOOLEAN: u8 = 0x12;

pub const NO_INDEX: u32 = 0xFFFF_FFFF;

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    d.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("axml: truncated u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("axml: truncated u32 @ {off}")))
}

/// Unidades de dimensão/fração (Res_value complex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexUnit {
    Px,
    Dip,
    Sp,
    Pt,
    In,
    Mm,
    Percent,
    PercentParent,
    Unknown(u8),
}

impl std::fmt::Display for ComplexUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ComplexUnit::Px => "px",
            ComplexUnit::Dip => "dp",
            ComplexUnit::Sp => "sp",
            ComplexUnit::Pt => "pt",
            ComplexUnit::In => "in",
            ComplexUnit::Mm => "mm",
            ComplexUnit::Percent => "%",
            ComplexUnit::PercentParent => "%p",
            ComplexUnit::Unknown(u) => return write!(f, "unit?{u}"),
        };
        f.write_str(s)
    }
}

fn complex_unit(raw: u8) -> ComplexUnit {
    match raw {
        0 => ComplexUnit::Px,
        1 => ComplexUnit::Dip,
        2 => ComplexUnit::Sp,
        3 => ComplexUnit::Pt,
        4 => ComplexUnit::In,
        5 => ComplexUnit::Mm,
        _ => ComplexUnit::Unknown(raw),
    }
}

fn fraction_unit(raw: u8) -> ComplexUnit {
    match raw {
        0 => ComplexUnit::Percent,
        1 => ComplexUnit::PercentParent,
        _ => ComplexUnit::Unknown(raw),
    }
}

/// complexToFloat (TypedValue): mantissa (bits 8–31, sinal no bit 31) × radix mult.
/// Layout: unit = bits 0–3, radix = bits 4–5, mantissa = bits 8–31.
/// Ex.: "100dp" → data = 0x00006401 → mantissa 0x6400 × 2⁻⁸ = 100.0.
fn complex_to_float(data: u32) -> f32 {
    const RADIX_MULTS: [f32; 4] = [
        0.003_906_25,   // radix 0: 2⁻⁸
        3.051_758e-5,   // radix 1: 2⁻¹⁵
        1.192_092_9e-7, // radix 2: 2⁻²³
        4.656_613e-10,  // radix 3: 2⁻³¹
    ];
    let mantissa = (data & 0xFFFF_FF00) as i32; // bits 8..31 — sinal do bit 31
    mantissa as f32 * RADIX_MULTS[((data >> 4) & 0x3) as usize]
}

/// Valor tipado de um atributo AXML.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Null,
    String(String),
    /// Recurso (ex.: @string/app_name → 0x7f0e0001).
    Reference(u32),
    /// Referência a atributo do mesmo tema.
    Attribute(u32),
    Float(f32),
    Dimension(f32, ComplexUnit),
    Fraction(f32, ComplexUnit),
    Int(i32),
    Bool(bool),
    /// Tipo não tratado explicitamente — preservado cru (Lei 2).
    Raw {
        dtype: u8,
        data: u32,
    },
}

impl AttrValue {
    /// Texto "amigável" (strings cruas/decodificadas; refs como hex).
    pub fn as_text(&self) -> String {
        match self {
            AttrValue::Null => String::new(),
            AttrValue::String(s) => s.clone(),
            AttrValue::Reference(r) | AttrValue::Attribute(r) => format!("@0x{r:08x}"),
            AttrValue::Float(f) => format!("{f}"),
            AttrValue::Dimension(v, u) => format!("{v}{u}"),
            AttrValue::Fraction(v, u) => format!("{v}{u}"),
            AttrValue::Int(i) => format!("{i}"),
            AttrValue::Bool(b) => format!("{b}"),
            AttrValue::Raw { data, .. } => format!("0x{data:08x}"),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            AttrValue::Bool(b) => Some(*b),
            AttrValue::String(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            AttrValue::Int(i) => Some(*i != 0),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            AttrValue::Int(i) => Some(*i),
            AttrValue::Reference(r) | AttrValue::Attribute(r) => Some(*r as i32),
            AttrValue::String(s) => {
                let s = s.trim();
                if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    i32::from_str_radix(hex, 16).ok()
                } else {
                    s.parse::<i32>().ok()
                }
            }
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_i32().and_then(|i| u32::try_from(i).ok())
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            AttrValue::String(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct XmlAttribute {
    /// URI completo do namespace (ex.: NS_ANDROID), se houver.
    pub ns: Option<String>,
    /// Nome local (sem prefixo).
    pub name: String,
    /// String crua declarada (quando o aapt a guardou).
    pub raw: Option<String>,
    /// Valor tipado decodificado.
    pub value: AttrValue,
    /// Resource ID canônico do atributo (via resource map), se conhecido.
    pub res_id: Option<u32>,
}

impl XmlAttribute {
    pub fn is_android(&self) -> bool {
        self.ns.as_deref() == Some(NS_ANDROID)
    }
    pub fn text(&self) -> String {
        self.raw.clone().unwrap_or_else(|| self.value.as_text())
    }
    pub fn as_bool(&self) -> Option<bool> {
        self.value.as_bool()
    }
    pub fn as_u32(&self) -> Option<u32> {
        self.value.as_u32()
    }
    pub fn as_i32(&self) -> Option<i32> {
        self.value.as_i32()
    }
}

const MAX_XML_DEPTH: usize = 1024;

#[derive(Debug, Default)]
pub struct XmlElement {
    /// Nome local do elemento (ex.: "activity", "uses-permission").
    pub name: String,
    pub attrs: Vec<XmlAttribute>,
    pub children: Vec<XmlElement>,
    /// Texto CDATA direto (raro em manifests).
    pub text: Option<String>,
}

// issue #253: `#[derive(Clone)]` recursivo estoura a stack em árvores profundas
// (~200k níveis). Clone iterativo em pré-ordem + montagem bottom-up em O(N).
impl Clone for XmlElement {
    fn clone(&self) -> Self {
        if self.children.is_empty() {
            return Self {
                name: self.name.clone(),
                attrs: self.attrs.clone(),
                children: Vec::new(),
                text: self.text.clone(),
            };
        }
        let mut nodes: Vec<(Option<usize>, XmlElement)> = Vec::new();
        let mut work: Vec<(Option<usize>, &XmlElement)> = vec![(None, self)];
        while let Some((parent, src)) = work.pop() {
            let my_idx = nodes.len();
            nodes.push((
                parent,
                XmlElement {
                    name: src.name.clone(),
                    attrs: src.attrs.clone(),
                    children: Vec::with_capacity(src.children.len()),
                    text: src.text.clone(),
                },
            ));
            for child in &src.children {
                work.push((Some(my_idx), child));
            }
        }
        for idx in (1..nodes.len()).rev() {
            let (parent_opt, child_el) = std::mem::take(&mut nodes[idx]);
            if let Some(pidx) = parent_opt {
                nodes[pidx].1.children.push(child_el);
            }
        }
        std::mem::take(&mut nodes[0].1)
    }
}

// issue #40: o drop recursivo padrão estoura a stack em AXML hostil com
// ~200k níveis de aninhamento (o PARSE é iterativo, o drop não — abort do
// processo). Visita iterativa com stack explícita: tira os filhos de cada
// nó antes de soltá-lo, sem recursão.
impl Drop for XmlElement {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        stack.extend(std::mem::take(&mut self.children));
        while let Some(mut node) = stack.pop() {
            stack.extend(std::mem::take(&mut node.children));
        }
    }
}

impl XmlElement {
    /// Primeiro atributo com `ns==android` e nome local dado.
    pub fn android_attr(&self, name: &str) -> Option<&XmlAttribute> {
        self.attrs.iter().find(|a| a.is_android() && a.name == name)
    }

    /// Atributo com nome local dado (qualquer namespace).
    pub fn attr(&self, name: &str) -> Option<&XmlAttribute> {
        self.attrs.iter().find(|a| a.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlElement> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AxmlDocument {
    pub root: XmlElement,
    /// Resource map: índice do nome de atributo no pool → resource id canônico.
    pub resource_ids: Vec<u32>,
    pub strings: StringPool,
    pub namespaces: Vec<(String, String)>, // (prefix, uri)
}

/// Parse completo de um documento AXML.
pub fn parse(data: &[u8]) -> RdResult<AxmlDocument> {
    if data.len() < 8 {
        return Err(RdError::parse("axml: file shorter than 8 bytes"));
    }
    let file_type = u16_at(data, 0)?;
    if file_type != RES_XML_TYPE {
        return Err(RdError::parse(format!(
            "axml: expected file type 0x{RES_XML_TYPE:04x}, got 0x{file_type:04x} (not binary XML?)"
        ))
        .with_suggestion("this chunk of the APK is not AXML — check the entry name"));
    }
    let file_size = u32_at(data, 4)? as usize;
    if file_size > data.len() {
        return Err(RdError::parse(format!(
            "axml: declared size {file_size} exceeds container {}",
            data.len()
        )));
    }
    let data = &data[..file_size]; // ignora lixo à direita, se houver

    let mut strings = StringPool::default();
    let mut resource_ids: Vec<u32> = Vec::new();
    let mut namespaces: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;
    let mut open_count: usize = 0;

    let mut p = 8usize; // após o header do arquivo
    while p.checked_add(8).is_some_and(|end| end <= data.len()) {
        let ctype = u16_at(data, p)?;
        let cheader = u16_at(data, p + 2)? as usize;
        let csize = u32_at(data, p + 4)? as usize;
        // progresso garantido: chunk mínimo = 8 bytes (endurece contra fuzz e 32-bit)
        let Some(chunk_end) = p.checked_add(csize) else {
            return Err(RdError::parse(format!(
                "axml: chunk 0x{ctype:04x} at {p} overflows address space (size {csize})"
            )));
        };
        if csize < 8 || csize < cheader || chunk_end > data.len() {
            return Err(RdError::parse(format!(
                "axml: chunk 0x{ctype:04x} at {p} declares size {csize} beyond bounds"
            )));
        }
        let payload = p + cheader; // seguro: `cheader <= csize` e `p + csize` não deu overflow
        match ctype {
            RES_STRING_POOL_TYPE => {
                let (pool, used) = StringPool::parse(&data[..chunk_end], p)?;
                if used as usize > csize {
                    return Err(RdError::parse("axml: string pool bigger than its chunk"));
                }
                strings = pool;
            }
            RES_XML_RESOURCE_MAP_TYPE => {
                let n = (csize - cheader) / 4;
                resource_ids = (0..n)
                    .map(|i| u32_at(data, payload + i * 4))
                    .collect::<RdResult<_>>()?;
            }
            RES_XML_START_NAMESPACE_TYPE => {
                if payload.checked_add(8).map_or(true, |end| end > chunk_end) {
                    return Err(RdError::parse("axml: start namespace chunk truncated"));
                }
                let prefix_idx = u32_at(data, payload)?;
                let uri_idx = u32_at(data, payload + 4)?;
                let prefix = strings.get(prefix_idx).unwrap_or("").to_string();
                let uri = strings.get(uri_idx).unwrap_or("").to_string();
                namespaces.push((prefix, uri));
            }
            RES_XML_START_ELEMENT_TYPE => {
                if root.is_some() {
                    return Err(RdError::parse(
                        "axml: trailing start element after root closed (unbalanced document)",
                    ));
                }
                if open_count >= MAX_XML_DEPTH {
                    return Err(RdError::parse(format!(
                        "axml: element nesting exceeds maximum depth ({MAX_XML_DEPTH})"
                    )));
                }
                let el = parse_start_element(data, p, cheader, chunk_end, &strings, &resource_ids)?;
                stack.push(el);
                open_count += 1;
            }
            RES_XML_END_ELEMENT_TYPE => {
                if payload.checked_add(8).map_or(true, |end| end > chunk_end) {
                    return Err(RdError::parse("axml: end element chunk truncated"));
                }
                let _name_idx = u32_at(data, payload + 4)?;
                match (stack.pop(), open_count) {
                    (Some(el), n) if n > 1 => {
                        open_count = n - 1;
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(el);
                        }
                    }
                    (Some(el), _) => {
                        // issue #35: segundo elemento top-level — documento
                        // desbalanceado (Android rejeita). Sobrescrever o root
                        // descartaria o primeiro (com filhos) — viola a Lei 2.
                        if root.is_some() {
                            return Err(RdError::parse(
                                "axml: segundo elemento top-level (documento desbalanceado)",
                            ));
                        }
                        root = Some(el);
                        open_count = 0;
                    }
                    (None, _) => {
                        return Err(RdError::parse("axml: end element without matching start"));
                    }
                }
            }
            RES_XML_CDATA_TYPE => {
                if payload.checked_add(4).map_or(true, |end| end > chunk_end) {
                    return Err(RdError::parse("axml: cdata chunk truncated"));
                }
                let text_idx = u32_at(data, payload)?;
                if let Some(parent) = stack.last_mut() {
                    parent.text = Some(strings.get(text_idx).unwrap_or("").to_string());
                }
            }
            _ => { /* chunk desconhecido: skip por csize (robustez) */ }
        }
        p = chunk_end;
    }

    if !stack.is_empty() || open_count != 0 {
        return Err(RdError::parse(
            "axml: unclosed elements at end of document (unbalanced document)",
        ));
    }

    match root {
        Some(root) => Ok(AxmlDocument {
            root,
            resource_ids,
            strings,
            namespaces,
        }),
        None => Err(RdError::parse(
            "axml: no root element (unbalanced document?)",
        )),
    }
}

fn parse_start_element(
    data: &[u8],
    base: usize,
    cheader: usize,
    chunk_end: usize,
    strings: &StringPool,
    resource_ids: &[u32],
) -> RdResult<XmlElement> {
    // ResXMLTree_attrExt: ns u32 | name u32 | attributeStart u16 | attributeSize u16
    //                     | attributeCount u16 | idIndex u16 | classIndex u16 | styleIndex u16
    let attr_ext = base
        .checked_add(cheader)
        .ok_or_else(|| RdError::parse("axml: start element header overflow"))?;
    if attr_ext.checked_add(20).map_or(true, |end| end > chunk_end) {
        return Err(RdError::parse(
            "axml: start element ResXMLTree_attrExt truncated",
        ));
    }
    let name_idx = u32_at(data, attr_ext + 4)?;
    let attribute_start = u16_at(data, attr_ext + 8)? as usize;
    let attribute_size = u16_at(data, attr_ext + 10)? as usize;
    let attribute_count = u16_at(data, attr_ext + 12)? as usize;

    let name = strings.get(name_idx).unwrap_or("").to_string();
    let attrs_base = attr_ext
        .checked_add(attribute_start)
        .ok_or_else(|| RdError::parse("axml: attributeStart overflow"))?;

    if attribute_count > 0 {
        if attribute_size < 20 {
            return Err(RdError::parse(format!(
                "axml: invalid attributeSize {attribute_size} (< 20) on <{name}>"
            )));
        }
        let attrs_bytes = attribute_count
            .checked_mul(attribute_size)
            .ok_or_else(|| RdError::parse("axml: attribute table size overflow"))?;
        let attrs_end = attrs_base
            .checked_add(attrs_bytes)
            .ok_or_else(|| RdError::parse("axml: attribute table bounds overflow"))?;
        if attrs_end > chunk_end {
            return Err(RdError::parse(format!(
                "axml: <{name}> declares {attribute_count} attributes beyond chunk end"
            )));
        }
    }

    let mut attrs = Vec::with_capacity(attribute_count.min(256));
    for i in 0..attribute_count {
        let a = attrs_base + i * attribute_size;
        if a.checked_add(20).map_or(true, |end| end > chunk_end) {
            return Err(RdError::parse(format!(
                "axml: attribute #{i} of <{name}> out of bounds"
            )));
        }
        // layout do atributo (20 bytes): ns u32 | name u32 | rawValue u32
        // | typedValue: size u16 @12, res0 u8 @14, dataType u8 @15, data u32 @16
        let ns_idx = u32_at(data, a)?;
        let aname_idx = u32_at(data, a + 4)?;
        let raw_idx = u32_at(data, a + 8)?;
        let tv_size = u16_at(data, a + 12)? as usize;
        let dtype = *data
            .get(a + 15)
            .ok_or_else(|| RdError::parse("axml: attribute typed value truncated"))?;
        let ddata = u32_at(data, a + 16)?;

        let ns = if ns_idx == NO_INDEX {
            None
        } else {
            Some(strings.get(ns_idx).unwrap_or("").to_string())
        };
        let aname = strings.get(aname_idx).unwrap_or("").to_string();
        let raw = if raw_idx == NO_INDEX {
            None
        } else {
            strings.get(raw_idx).map(str::to_owned)
        };
        let value = decode_typed_value(strings, dtype, ddata, raw.as_deref());
        let res_id = resource_ids.get(aname_idx as usize).copied();

        attrs.push(XmlAttribute {
            ns,
            name: aname,
            raw,
            value,
            res_id,
        });
        let _ = tv_size; // validação: normalmente 8
    }

    Ok(XmlElement {
        name,
        attrs,
        children: Vec::new(),
        text: None,
    })
}

fn decode_typed_value(strings: &StringPool, dtype: u8, data: u32, raw: Option<&str>) -> AttrValue {
    match dtype {
        TYPE_STRING => {
            // preferir raw (idêntico, já decodificado); cair para o pool
            raw.map(|s| AttrValue::String(s.to_owned()))
                .unwrap_or_else(|| AttrValue::String(strings.get(data).unwrap_or("").to_owned()))
        }
        TYPE_REFERENCE => AttrValue::Reference(data),
        TYPE_ATTRIBUTE => AttrValue::Attribute(data),
        TYPE_FLOAT => AttrValue::Float(f32::from_bits(data)),
        TYPE_DIMENSION => {
            AttrValue::Dimension(complex_to_float(data), complex_unit((data & 0xF) as u8))
        }
        TYPE_FRACTION => {
            AttrValue::Fraction(complex_to_float(data), fraction_unit((data & 0xF) as u8))
        }
        TYPE_BOOLEAN => AttrValue::Bool(data != 0),
        TYPE_NULL => AttrValue::Null,
        t if (TYPE_FIRST_INT..=TYPE_LAST_INT).contains(&t) => AttrValue::Int(data as i32),
        t => AttrValue::Raw { dtype: t, data },
    }
}

// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um AXML mínimo <manifest package="com.t" android:versionCode="7"/>.
    /// String pool: 0:"manifest" 1:"package" 2:"com.t" 3:"versionCode" 4:"android" 5:android-ns
    fn handcrafted_axml() -> Vec<u8> {
        fn utf16(s: &str) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend((s.len() as u16).to_le_bytes());
            for u in s.encode_utf16() {
                v.extend(u.to_le_bytes());
            }
            v.extend(0u16.to_le_bytes());
            v
        }
        let strs = [
            "manifest",
            "package",
            "com.t",
            "versionCode",
            "android",
            NS_ANDROID,
        ];
        let mut pool = Vec::new();
        pool.extend(RES_STRING_POOL_TYPE.to_le_bytes());
        pool.extend(28u16.to_le_bytes());
        pool.extend(0u32.to_le_bytes()); // size patched
        pool.extend((strs.len() as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        pool.extend(0u32.to_le_bytes()); // flags (UTF-16)
        let strings_start = 28 + strs.len() * 4;
        pool.extend((strings_start as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        let mut offs = Vec::new();
        let mut body = Vec::new();
        for s in strs {
            offs.push(body.len() as u32);
            body.extend(utf16(s));
        }
        pool.extend(offs.iter().flat_map(|o| o.to_le_bytes()));
        pool.extend(&body);
        let pool_size = pool.len() as u32;
        pool[4..8].copy_from_slice(&pool_size.to_le_bytes());

        // resource map: versionCode (pool idx 3) → 0x0101021b
        let mut resmap = Vec::new();
        resmap.extend(RES_XML_RESOURCE_MAP_TYPE.to_le_bytes());
        resmap.extend(8u16.to_le_bytes());
        resmap.extend(28u32.to_le_bytes()); // 8 + 5 ids × 4
        resmap.extend(0u32.to_le_bytes()); // idx0 → 0
        resmap.extend(0u32.to_le_bytes());
        resmap.extend(0u32.to_le_bytes());
        resmap.extend(0x0101_021bu32.to_le_bytes()); // idx3 = versionCode
        resmap.extend(0u32.to_le_bytes()); // idx4

        // start element <manifest>
        let mut el = Vec::new();
        el.extend(RES_XML_START_ELEMENT_TYPE.to_le_bytes());
        el.extend(16u16.to_le_bytes()); // header (node) size = 8 + line + comment
        el.extend(0u32.to_le_bytes()); // size patched
        el.extend(1u32.to_le_bytes()); // line
        el.extend(NO_INDEX.to_le_bytes()); // comment
        el.extend(NO_INDEX.to_le_bytes()); // ns
        el.extend(0u32.to_le_bytes()); // name = "manifest"
        el.extend(20u16.to_le_bytes()); // attributeStart (a partir de ns)
        el.extend(20u16.to_le_bytes()); // attributeSize
        el.extend(2u16.to_le_bytes()); // attributeCount
        el.extend(0u16.to_le_bytes()); // idIndex
        el.extend(0u16.to_le_bytes()); // classIndex
        el.extend(0u16.to_le_bytes()); // styleIndex
                                       // attr 0: package="com.t" (string raw, idx 1; raw value idx 2)
        el.extend(NO_INDEX.to_le_bytes()); // ns
        el.extend(1u32.to_le_bytes()); // name "package"
        el.extend(2u32.to_le_bytes()); // raw "com.t"
        el.extend(8u16.to_le_bytes()); // typed value size
        el.push(0u8); // res0
        el.push(TYPE_STRING); // dataType
        el.extend(2u32.to_le_bytes()); // data = pool idx 2
                                       // attr 1: android:versionCode=7 (int, sem raw)
        el.extend(5u32.to_le_bytes()); // ns = android-ns (pool idx 5)
        el.extend(3u32.to_le_bytes()); // name "versionCode"
        el.extend(NO_INDEX.to_le_bytes()); // sem raw
        el.extend(8u16.to_le_bytes());
        el.push(0u8);
        el.push(TYPE_FIRST_INT);
        el.extend(7u32.to_le_bytes());
        let el_size = el.len() as u32;
        el[4..8].copy_from_slice(&el_size.to_le_bytes());

        // end element
        let mut end = Vec::new();
        end.extend(RES_XML_END_ELEMENT_TYPE.to_le_bytes());
        end.extend(16u16.to_le_bytes());
        end.extend(24u32.to_le_bytes());
        end.extend(1u32.to_le_bytes()); // line
        end.extend(NO_INDEX.to_le_bytes());
        end.extend(NO_INDEX.to_le_bytes()); // ns
        end.extend(0u32.to_le_bytes()); // name

        let total = (8 + pool.len() + resmap.len() + el.len() + end.len()) as u32;
        let mut out = Vec::new();
        out.extend(RES_XML_TYPE.to_le_bytes());
        out.extend(8u16.to_le_bytes());
        out.extend(total.to_le_bytes());
        out.extend(pool);
        out.extend(resmap);
        out.extend(el);
        out.extend(end);
        out
    }

    #[test]
    fn parses_handcrafted_manifest_axml() {
        let bytes = handcrafted_axml();
        let doc = parse(&bytes).unwrap();
        assert_eq!(doc.root.name, "manifest");
        assert_eq!(doc.strings.len(), 6);
        assert_eq!(
            doc.root.attr("package").unwrap().value,
            AttrValue::String("com.t".into())
        );
        let vc = doc.root.android_attr("versionCode").unwrap();
        assert_eq!(vc.value, AttrValue::Int(7));
        assert_eq!(vc.res_id, Some(0x0101_021b));
        assert!(vc.is_android());
    }

    #[test]
    fn rejects_non_axml() {
        assert!(parse(b"not axml").is_err());
    }

    #[test]
    fn complex_to_float_matches_aosp_examples() {
        // "100dp" codificado pelo aapt: mantissa 100 @ bits 8+, radix 0, unit DIP(1)
        let dp100 = (100u32 << 8) | 1;
        assert!((complex_to_float(dp100) - 100.0).abs() < 1e-4);
        // "50%": mantissa 50, radix 0, unit 0
        let pct50 = 50u32 << 8;
        assert!((complex_to_float(pct50) - 50.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    /// issue #40: drop iterativo — XmlElement de 200k níveis não pode estourar
    /// a stack ao ser solto (o parse é iterativo; o drop default não era).
    #[test]
    fn deep_xml_element_drops_without_stack_overflow() {
        let mut cur = XmlElement::default();
        for _ in 0..200_000 {
            cur = XmlElement {
                name: "n".to_string(),
                attrs: Vec::new(),
                children: vec![cur],
                text: None,
            };
        }
        // assert implícito: o drop abaixo não aborta o processo
        drop(cur);
    }
}
