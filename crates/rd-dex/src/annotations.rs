//! annotations.rs — `encoded_value` / `encoded_array` / itens de anotação.
//!
//! Implementação exata do §`encoded_value` do dex-format (códigos e semântica
//! conferidos com o leitor de referência do ART `EncodedValue::Read` e com o
//! dexlib2/smali):
//!
//! - O primeiro byte carrega `value_type` nos 5 bits baixos e `value_arg` nos
//!   3 bits altos. Para inteiros e índices, `value_arg + 1` é a largura em
//!   bytes; para float/double, a largura armazenada é `value_arg + 1` bytes de
//!   ALTA ordem (o complemento é zero-pad à direita do padrão de bits, i.e.
//!   `bits << (width - n) * 8`); para array/annotation é a contagem; para
//!   boolean, o próprio valor; para null, 0.
//! - Inteiros (byte/short/int/long): little-endian com extensão de sinal — o
//!   encoding "right-shifted" do spec (o valor é truncado aos bytes de menor
//!   ordem, nunca zigzag — zigzag é protobuf, não DEX).
//! - char: zero-extendido (u16), nunca sinal.
//! - Índices (string/type/field/enum/method/method_handle/method_type):
//!   little-endian sem sinal, largura `value_arg + 1` (máx. 4 bytes).
//!
//! APIs expostas (consumo do disassembler M1-A2): `EncodedValue`,
//! `parse_encoded_array` (static_values de class_def e call_site_item),
//! `parse_annotation_item` (visibility + `encoded_annotation` com name_idx +
//! elements), `parse_annotation_set_item`, `parse_annotation_set_ref_list` e
//! `parse_annotations_directory`. Lei 2: os offsets de cada item ficam
//! expostos; os bytes brutos permanecem em `Dex::data`. Lei 1: toda leitura
//! é bounds-checked, falha sempre tipada, sem pânico.

use crate::error::{RdError, RdResult};
use crate::read::Reader;

/// Códigos `value_type` do §encoded_value (dex-format).
pub mod value_type {
    pub const BYTE: u8 = 0x00;
    pub const SHORT: u8 = 0x02;
    pub const CHAR: u8 = 0x03;
    pub const INT: u8 = 0x04;
    pub const LONG: u8 = 0x06;
    pub const FLOAT: u8 = 0x10;
    pub const DOUBLE: u8 = 0x11;
    pub const METHOD_TYPE: u8 = 0x15;
    pub const METHOD_HANDLE: u8 = 0x16;
    pub const STRING: u8 = 0x17;
    pub const TYPE: u8 = 0x18;
    pub const FIELD: u8 = 0x19;
    /// spec AOSP: VALUE_METHOD = 0x1a (method_ids), VALUE_ENUM = 0x1b (field_ids).
    pub const ENUM: u8 = 0x1b;
    pub const METHOD: u8 = 0x1a;
    pub const ARRAY: u8 = 0x1c;
    pub const ANNOTATION: u8 = 0x1d;
    pub const NULL: u8 = 0x1e;
    pub const BOOLEAN: u8 = 0x1f;
}

/// Valor codificado do DEX — todos os tipos do §encoded_value.
#[derive(Debug, Clone, PartialEq)]
pub enum EncodedValue {
    /// 0x00 — inteiro de 1 byte com sinal.
    Byte(i8),
    /// 0x02 — sinal, largura `value_arg + 1`.
    Short(i16),
    /// 0x03 — zero-extendido (u16).
    Char(u16),
    /// 0x04 — sinal, largura `value_arg + 1`.
    Int(i32),
    /// 0x06 — sinal, largura `value_arg + 1`.
    Long(i64),
    /// 0x10 — bits de alta ordem armazenados, zero-pad à direita.
    Float(f32),
    /// 0x11 — idem, 64 bits.
    Double(f64),
    /// 0x15 — índice em proto_ids.
    MethodType(u32),
    /// 0x16 — índice em method_handles.
    MethodHandle(u32),
    /// 0x17 — índice em string_ids.
    String(u32),
    /// 0x18 — índice em type_ids.
    Type(u32),
    /// 0x19 — índice em field_ids.
    Field(u32),
    /// 0x1b — índice em field_ids (constante de enum).
    Enum(u32),
    /// 0x1a — índice em method_ids.
    Method(u32),
    /// 0x1c — `value_arg` = contagem de elementos.
    Array(Vec<EncodedValue>),
    /// 0x1d — anotação embutida (`value_arg` = contagem de elementos).
    Annotation(EncodedAnnotation),
    /// 0x1e — referência nula.
    Null,
    /// 0x1f — `value_arg` É o valor (0/1); nenhum byte de payload.
    Boolean(bool),
}

impl EncodedValue {
    /// Código `value_type` correspondente (útil p/ renderização e diagnóstico).
    pub fn type_code(&self) -> u8 {
        match self {
            EncodedValue::Byte(_) => value_type::BYTE,
            EncodedValue::Short(_) => value_type::SHORT,
            EncodedValue::Char(_) => value_type::CHAR,
            EncodedValue::Int(_) => value_type::INT,
            EncodedValue::Long(_) => value_type::LONG,
            EncodedValue::Float(_) => value_type::FLOAT,
            EncodedValue::Double(_) => value_type::DOUBLE,
            EncodedValue::MethodType(_) => value_type::METHOD_TYPE,
            EncodedValue::MethodHandle(_) => value_type::METHOD_HANDLE,
            EncodedValue::String(_) => value_type::STRING,
            EncodedValue::Type(_) => value_type::TYPE,
            EncodedValue::Field(_) => value_type::FIELD,
            EncodedValue::Enum(_) => value_type::ENUM,
            EncodedValue::Method(_) => value_type::METHOD,
            EncodedValue::Array(_) => value_type::ARRAY,
            EncodedValue::Annotation(_) => value_type::ANNOTATION,
            EncodedValue::Null => value_type::NULL,
            EncodedValue::Boolean(_) => value_type::BOOLEAN,
        }
    }
}

/// `encoded_annotation` — tipo + pares nome → valor.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedAnnotation {
    /// índice em type_ids (o tipo da anotação, ex.: `Landroid/x/Y;`).
    pub type_idx: u32,
    /// pares (name_idx em string_ids, valor) — ordem do arquivo.
    pub elements: Vec<AnnotationElement>,
}

/// Elemento de `encoded_annotation`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationElement {
    /// índice em string_ids (nome do campo da anotação).
    pub name_idx: u32,
    pub value: EncodedValue,
}

/// Visibilidade de um `annotation_item` (dex-format §annotation_item).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// 0 — visível só em build (RetentionPolicy.CLASS).
    Build,
    /// 1 — visível em runtime (RetentionPolicy.RUNTIME).
    Runtime,
    /// 2 — visível ao sistema (ex.: @android.annotation.TestApi).
    System,
}

impl Visibility {
    pub fn from_code(code: u8) -> RdResult<Visibility> {
        match code {
            0 => Ok(Visibility::Build),
            1 => Ok(Visibility::Runtime),
            2 => Ok(Visibility::System),
            _ => Err(RdError::invalid_format(format!(
                "annotation_item: visibility {code} inválida (0–2)"
            ))),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Visibility::Build => 0,
            Visibility::Runtime => 1,
            Visibility::System => 2,
        }
    }

    /// Prefixo smali do baksmali (`.annotation build` / `runtime` / `system`).
    pub fn name(self) -> &'static str {
        match self {
            Visibility::Build => "build",
            Visibility::Runtime => "runtime",
            Visibility::System => "system",
        }
    }
}

/// `annotation_item` completo (Lei 2: offset bruto preservado).
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationItem {
    /// offset bruto do item no dex.
    pub offset: usize,
    pub visibility: Visibility,
    pub annotation: EncodedAnnotation,
}

// ── leitura de encoded_value ────────────────────────────────────────────────

/// Lê `value_arg + 1` bytes little-endian; com `sign_extend`, estende o bit de
/// sinal do último byte (encoding right-shifted do spec — NÃO é zigzag).
fn var_int(r: &mut Reader, value_arg: u8, sign_extend: bool) -> RdResult<i64> {
    let n = value_arg as usize + 1;
    if n > 8 {
        return Err(RdError::invalid_format(format!(
            "encoded_value: value_arg {value_arg} excede 8 bytes de payload"
        )));
    }
    let mut v: u64 = 0;
    for i in 0..n {
        let b = r.u8()?;
        v |= (b as u64) << (8 * i);
    }
    if sign_extend {
        let bits = 8 * n;
        // n == 8 preenche o u64 inteiro — nada a estender (guard anti-shift-overflow)
        if bits < 64 && v & (1 << (bits - 1)) != 0 {
            v |= u64::MAX << bits;
        }
    }
    Ok(v as i64)
}

/// Índice sem sinal de largura `value_arg + 1` (string/type/field/enum/method/
/// method_handle/method_type); spec limita a 4 bytes.
fn var_index(r: &mut Reader, value_arg: u8) -> RdResult<u32> {
    if value_arg > 3 {
        return Err(RdError::invalid_format(format!(
            "encoded_value: índice com value_arg {value_arg} > 3"
        )));
    }
    Ok(var_int(r, value_arg, false)? as u32)
}

/// Teto de aninhamento de `encoded_value` (array/annotation aninhados).
///
/// A spec não limita formalmente, mas DEX reais ficam abaixo de poucas dezenas
/// de níveis; 64 é generoso. Sem esse teto, cada nível de aninhamento custa
/// apenas 2 bytes de input (`0x1C 0x01`) e a recursão estoura a stack em
/// ~50 mil níveis (~100 KB de input) — abort do processo inteiro, sem chance
/// de unwinding (Lei 1: parser não pode morrer em input hostil).
const MAX_ENCODED_VALUE_DEPTH: usize = 64;
/// Teto total de nós `EncodedValue` decodificados por chamada top-level (#332).
const MAX_ENCODED_VALUE_NODES: usize = 262_144;
/// Pré-alocação máxima de `Vec::with_capacity` em `ARRAY`/`ANNOTATION` (#332).
const MAX_ENCODED_PREALLOC: usize = 4096;

/// Lê um `encoded_value` completo (recursivo p/ array/annotation).
pub fn read_encoded_value(r: &mut Reader) -> RdResult<EncodedValue> {
    let mut budget = MAX_ENCODED_VALUE_NODES;
    read_encoded_value_depth(r, 0, &mut budget)
}

/// Implementação recursiva com orçamento de profundidade e contagem total de nós.
fn read_encoded_value_depth(
    r: &mut Reader,
    depth: usize,
    budget: &mut usize,
) -> RdResult<EncodedValue> {
    if depth > MAX_ENCODED_VALUE_DEPTH {
        return Err(RdError::invalid_format(format!(
            "encoded_value: aninhamento além de {MAX_ENCODED_VALUE_DEPTH} níveis (recursão limitada contra stack overflow)"
        )));
    }
    if *budget == 0 {
        return Err(RdError::parse(format!(
            "encoded_value: número de nós excede limite ({MAX_ENCODED_VALUE_NODES})"
        )));
    }
    *budget -= 1;
    let header = r.u8()?;
    let vt = header & 0x1F;
    let arg = (header >> 5) & 0x07;
    match vt {
        value_type::BYTE => Ok(EncodedValue::Byte(r.u8()? as i8)),
        value_type::SHORT => {
            if arg > 1 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: short com value_arg {arg} > 1"
                )));
            }
            Ok(EncodedValue::Short(var_int(r, arg, true)? as i16))
        }
        value_type::CHAR => {
            if arg > 1 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: char com value_arg {arg} > 1"
                )));
            }
            Ok(EncodedValue::Char(var_int(r, arg, false)? as u16))
        }
        value_type::INT => {
            if arg > 3 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: int com value_arg {arg} > 3"
                )));
            }
            Ok(EncodedValue::Int(var_int(r, arg, true)? as i32))
        }
        value_type::LONG => Ok(EncodedValue::Long(var_int(r, arg, true)?)),
        value_type::FLOAT => {
            // bytes armazenados = alta ordem do padrão de 32 bits; zero-pad à
            // direita (menos significativos = 0)
            if arg > 3 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: float com value_arg {arg} > 3"
                )));
            }
            let bits = (var_int(r, arg, false)? as u64 as u32) << ((3 - arg) as u32 * 8);
            Ok(EncodedValue::Float(f32::from_bits(bits)))
        }
        value_type::DOUBLE => {
            if arg > 7 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: double com value_arg {arg} > 7"
                )));
            }
            let bits = (var_int(r, arg, false)? as u64) << ((7 - arg) as u32 * 8);
            Ok(EncodedValue::Double(f64::from_bits(bits)))
        }
        value_type::METHOD_TYPE => Ok(EncodedValue::MethodType(var_index(r, arg)?)),
        value_type::METHOD_HANDLE => Ok(EncodedValue::MethodHandle(var_index(r, arg)?)),
        value_type::STRING => Ok(EncodedValue::String(var_index(r, arg)?)),
        value_type::TYPE => Ok(EncodedValue::Type(var_index(r, arg)?)),
        value_type::FIELD => Ok(EncodedValue::Field(var_index(r, arg)?)),
        value_type::ENUM => Ok(EncodedValue::Enum(var_index(r, arg)?)),
        value_type::METHOD => Ok(EncodedValue::Method(var_index(r, arg)?)),
        value_type::ARRAY => {
            // spec ("encoded_array format"): value_arg deve ser 0; o número de
            // elementos segue como uleb128 após o header.
            if arg != 0 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: array com value_arg {arg} != 0"
                )));
            }
            let count = r.uleb128()? as usize;
            if count > r.remaining() {
                return Err(RdError::parse(format!(
                    "encoded_value: array de {count} elementos > {} bytes restantes",
                    r.remaining()
                )));
            }
            if count > *budget {
                return Err(RdError::parse(format!(
                    "encoded_value: array de {count} elementos excede orçamento restante ({})",
                    *budget
                )));
            }
            let mut elements = Vec::with_capacity(count.min(MAX_ENCODED_PREALLOC));
            for _ in 0..count {
                elements.push(read_encoded_value_depth(r, depth + 1, budget)?);
            }
            Ok(EncodedValue::Array(elements))
        }
        value_type::ANNOTATION => {
            let type_idx = r.uleb128()?;
            // spec ("encoded_annotation format"): value_arg deve ser 0; o número
            // de elementos segue como uleb128.
            if arg != 0 {
                return Err(RdError::invalid_format(format!(
                    "encoded_value: annotation com value_arg {arg} != 0"
                )));
            }
            let count = r.uleb128()? as usize;
            if count > r.remaining() {
                return Err(RdError::parse(format!(
                    "encoded_value: annotation com {count} elementos > {} bytes restantes",
                    r.remaining()
                )));
            }
            if count > *budget {
                return Err(RdError::parse(format!(
                    "encoded_value: annotation com {count} elementos excede orçamento restante ({})",
                    *budget
                )));
            }
            let mut elements = Vec::with_capacity(count.min(MAX_ENCODED_PREALLOC));
            for _ in 0..count {
                let name_idx = r.uleb128()?;
                let value = read_encoded_value_depth(r, depth + 1, budget)?;
                elements.push(AnnotationElement { name_idx, value });
            }
            Ok(EncodedValue::Annotation(EncodedAnnotation {
                type_idx,
                elements,
            }))
        }
        value_type::NULL => Ok(EncodedValue::Null),
        value_type::BOOLEAN => Ok(EncodedValue::Boolean(arg != 0)),
        other => Err(RdError::invalid_format(format!(
            "encoded_value: value_type 0x{other:02x} desconhecido @ {}",
            r.pos
        ))),
    }
}

// ── encoded_array / itens de anotação ───────────────────────────────────────

/// `encoded_array`: `uleb128 size` + size × `encoded_value`. É o formato de
/// `static_values` (class_def_item) e de `call_site_item`.
pub fn parse_encoded_array(data: &[u8], offset: u32) -> RdResult<Vec<EncodedValue>> {
    let mut r = Reader::at(data, offset as usize)?;
    let size = r.uleb128()? as usize;
    // guard fuzz: cada elemento consome no mínimo 1 byte (header)
    if size > r.remaining() {
        return Err(RdError::parse(format!(
            "encoded_array: {size} elementos > {} bytes restantes @ 0x{offset:x}",
            r.remaining()
        )));
    }
    let mut budget = MAX_ENCODED_VALUE_NODES;
    if size > budget {
        return Err(RdError::parse(format!(
            "encoded_array: {size} elementos excedem limite ({MAX_ENCODED_VALUE_NODES}) @ 0x{offset:x}"
        )));
    }
    let mut out = Vec::with_capacity(size.min(MAX_ENCODED_PREALLOC));
    for _ in 0..size {
        out.push(read_encoded_value_depth(&mut r, 0, &mut budget)?);
    }
    Ok(out)
}

/// `annotation_item`: visibility (1 byte) + `encoded_annotation`.
pub fn parse_annotation_item(data: &[u8], off: u32) -> RdResult<AnnotationItem> {
    let mut r = Reader::at(data, off as usize)?;
    let visibility = Visibility::from_code(r.u8()?)?;
    let type_idx = r.uleb128()?;
    // encoded_annotation: size é uleb128 (diferente do encoded_value embutido,
    // onde a contagem vive no value_arg)
    let size = r.uleb128()? as usize;
    if size > r.remaining() {
        return Err(RdError::parse(format!(
            "annotation_item: {size} elementos > {} bytes restantes @ 0x{off:x}",
            r.remaining()
        )));
    }
    let mut budget = MAX_ENCODED_VALUE_NODES;
    if size > budget {
        return Err(RdError::parse(format!(
            "annotation_item: {size} elementos excedem limite ({MAX_ENCODED_VALUE_NODES}) @ 0x{off:x}"
        )));
    }
    let mut elements = Vec::with_capacity(size.min(MAX_ENCODED_PREALLOC));
    for _ in 0..size {
        let name_idx = r.uleb128()?;
        let value = read_encoded_value_depth(&mut r, 0, &mut budget)?;
        elements.push(AnnotationElement { name_idx, value });
    }
    Ok(AnnotationItem {
        offset: off as usize,
        visibility,
        annotation: EncodedAnnotation { type_idx, elements },
    })
}

/// `annotation_set_item`: `u32 size` + size × `u32 annotation_off`.
/// Retorna os offsets brutos dos `annotation_item` (Lei 2).
pub fn parse_annotation_set_item(data: &[u8], off: u32) -> RdResult<Vec<u32>> {
    if off == 0 {
        return Ok(Vec::new());
    }
    let mut r = Reader::at(data, off as usize)?;
    let size = r.u32()? as usize;
    if size
        .checked_mul(4)
        .map(|n| n > r.remaining())
        .unwrap_or(true)
    {
        return Err(RdError::parse(format!(
            "annotation_set_item: {size} entradas não cabem em {} bytes @ 0x{off:x}",
            r.remaining()
        )));
    }
    let mut out = Vec::with_capacity(size);
    for _ in 0..size {
        out.push(r.u32()?);
    }
    Ok(out)
}

/// `annotation_set_ref_list`: `u32 size` + size × `u32 annotation_set_off`
/// (0 = entrada ausente). Retorna os offsets brutos (Lei 2).
pub fn parse_annotation_set_ref_list(data: &[u8], off: u32) -> RdResult<Vec<u32>> {
    if off == 0 {
        return Ok(Vec::new());
    }
    let mut r = Reader::at(data, off as usize)?;
    let size = r.u32()? as usize;
    if size
        .checked_mul(4)
        .map(|n| n > r.remaining())
        .unwrap_or(true)
    {
        return Err(RdError::parse(format!(
            "annotation_set_ref_list: {size} entradas não cabem em {} bytes @ 0x{off:x}",
            r.remaining()
        )));
    }
    let mut out = Vec::with_capacity(size);
    for _ in 0..size {
        out.push(r.u32()?);
    }
    Ok(out)
}

/// `annotations_directory_item` de um class_def (Lei 2: offsets brutos).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnnotationsDirectory {
    /// offset bruto do item.
    pub offset: usize,
    /// annotation_set_item com as anotações da classe (0 = nenhuma).
    pub class_annotations_off: u32,
    /// pares (field_idx, annotation_set_item off).
    pub field_annotations: Vec<(u32, u32)>,
    /// pares (method_idx, annotation_set_item off).
    pub method_annotations: Vec<(u32, u32)>,
    /// pares (method_idx, annotation_set_ref_list off).
    pub parameter_annotations: Vec<(u32, u32)>,
}

/// `annotations_directory_item`: 4 offsets u32 + três listas de pares
/// (idx u32, off u32).
pub fn parse_annotations_directory(data: &[u8], off: u32) -> RdResult<AnnotationsDirectory> {
    let mut r = Reader::at(data, off as usize)?;
    let mut dir = AnnotationsDirectory {
        offset: off as usize,
        class_annotations_off: r.u32()?,
        ..Default::default()
    };
    let fields = r.u32()? as usize;
    let methods = r.u32()? as usize;
    let params = r.u32()? as usize;
    let total = fields
        .checked_mul(8)
        .and_then(|v| v.checked_add(methods.saturating_mul(8)))
        .and_then(|v| v.checked_add(params.saturating_mul(8)));
    if total.map(|n| n > r.remaining()).unwrap_or(true) {
        return Err(RdError::parse(format!(
            "annotations_directory: listas ({fields}+{methods}+{params}) não cabem \
             em {} bytes @ 0x{off:x}",
            r.remaining()
        )));
    }
    for _ in 0..fields {
        let idx = r.u32()?;
        dir.field_annotations.push((idx, r.u32()?));
    }
    for _ in 0..methods {
        let idx = r.u32()?;
        dir.method_annotations.push((idx, r.u32()?));
    }
    for _ in 0..params {
        let idx = r.u32()?;
        dir.parameter_annotations.push((idx, r.u32()?));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um encoded_value: header + payload.
    fn ev(header: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![header];
        v.extend_from_slice(payload);
        v
    }

    fn one(bytes: &[u8]) -> EncodedValue {
        let mut r = Reader::new(bytes);
        let v = read_encoded_value(&mut r).expect("encoded_value");
        assert_eq!(r.pos, bytes.len(), "payload consumido por completo");
        v
    }

    #[test]
    fn integers_are_right_shifted_sign_extended_not_zigzag() {
        // -1 em 1 byte (right-shift; zigzag de -1 seria 0x01)
        assert_eq!(one(&ev(0x04, &[0xFF])), EncodedValue::Int(-1));
        assert_eq!(one(&ev(0x06, &[0xFE])), EncodedValue::Long(-2));
        // -129 precisa de 2 bytes: 0x7F 0xFF (LE) sign-extend → -129
        assert_eq!(
            one(&ev(0x02 | (1 << 5), &[0x7F, 0xFF])),
            EncodedValue::Short(-129)
        );
        // largura total: int 0x01020304 com value_arg=3
        assert_eq!(
            one(&ev(0x04 | (3 << 5), &[0x04, 0x03, 0x02, 0x01])),
            EncodedValue::Int(0x0102_0304)
        );
        // long 0x1234 com value_arg=1
        assert_eq!(
            one(&ev(0x06 | (1 << 5), &[0x34, 0x12])),
            EncodedValue::Long(0x1234)
        );
        // byte é sempre signed
        assert_eq!(one(&ev(0x00, &[0x80])), EncodedValue::Byte(-128));
    }

    #[test]
    fn char_is_zero_extended_u16() {
        // 1 byte: 'A'
        assert_eq!(one(&ev(0x03, &[0x41])), EncodedValue::Char(0x41));
        // 2 bytes: 0x2028 (LINE SEPARATOR) — sem extensão de sinal
        assert_eq!(
            one(&ev(0x03 | (1 << 5), &[0x28, 0x20])),
            EncodedValue::Char(0x2028)
        );
    }

    #[test]
    fn float_double_zero_pad_low_order_bytes() {
        // dx codifica 2.0f (0x40000000) como byte único 0x40 (alta ordem) —
        // o resto é zero-pad no lado menos significativo
        let v = one(&ev(0x10, &[0x40]));
        assert_eq!(v, EncodedValue::Float(2.0));
        // full-width float 1.0f (0x3F800000)
        assert_eq!(
            one(&ev(0x10 | (3 << 5), &[0x00, 0x00, 0x80, 0x3F])),
            EncodedValue::Float(1.0)
        );
        // 1.5f (0x3FC00000) truncado p/ 2 bytes 0x3F 0xC0
        assert_eq!(
            one(&ev(0x10 | (1 << 5), &[0xC0, 0x3F])),
            EncodedValue::Float(1.5)
        );
        // double 2.0 (0x40000000_00000000) → byte único 0x40
        assert_eq!(one(&ev(0x11, &[0x40])), EncodedValue::Double(2.0));
    }

    #[test]
    fn index_sized_types() {
        assert_eq!(one(&ev(0x17, &[0x05])), EncodedValue::String(5));
        assert_eq!(one(&ev(0x18, &[0x03])), EncodedValue::Type(3));
        assert_eq!(one(&ev(0x19, &[0x07])), EncodedValue::Field(7));
        // spec: 0x1a = METHOD (method_ids), 0x1b = ENUM (field_ids)
        assert_eq!(one(&ev(0x1a, &[0x09])), EncodedValue::Method(9));
        assert_eq!(one(&ev(0x1b, &[0x0B])), EncodedValue::Enum(11));
        assert_eq!(one(&ev(0x15, &[0x0D])), EncodedValue::MethodType(13));
        assert_eq!(one(&ev(0x16, &[0x0F])), EncodedValue::MethodHandle(15));
        // índice de 2 bytes
        assert_eq!(
            one(&ev(0x17 | (1 << 5), &[0x34, 0x12])),
            EncodedValue::String(0x1234)
        );
    }

    #[test]
    fn null_and_boolean_use_value_arg() {
        assert_eq!(one(&ev(0x1E, &[])), EncodedValue::Null);
        assert_eq!(one(&ev(0x1F, &[])), EncodedValue::Boolean(false));
        // boolean true: value_arg = 1 (0x1F | 0x20 = 0x3F), sem payload
        assert_eq!(one(&ev(0x3F, &[])), EncodedValue::Boolean(true));
    }

    #[test]
    fn array_with_mixed_elements() {
        // spec: 0x1c (arg 0) + size uleb + valores — array de 2: [null, byte 7]
        let bytes = ev(0x1C, &[0x02, 0x1E, 0x00, 0x07]);
        assert_eq!(
            one(&bytes),
            EncodedValue::Array(vec![EncodedValue::Null, EncodedValue::Byte(7)])
        );
        // array vazio: 0x1c + size 0
        assert_eq!(one(&ev(0x1C, &[0x00])), EncodedValue::Array(vec![]));
    }

    #[test]
    fn nested_annotation_with_name_idx() {
        // annotation de 1 elemento: type_idx=10, { name=2, value=String(7) }
        let bytes = ev(
            0x1D,
            &[
                0x0A, // type_idx uleb
                0x01, // size uleb (spec: encoded_annotation format)
                0x02, // name_idx uleb
                0x17, 0x07, // String(7)
            ],
        );
        match one(&bytes) {
            EncodedValue::Annotation(a) => {
                assert_eq!(a.type_idx, 10);
                assert_eq!(a.elements.len(), 1);
                assert_eq!(a.elements[0].name_idx, 2);
                assert_eq!(a.elements[0].value, EncodedValue::String(7));
            }
            other => panic!("esperava annotation, got {other:?}"),
        }
        // annotation dentro de array (caso real: values de static_values)
        let bytes = ev(
            0x1C,
            &[
                0x01, 0x1D, 0x0A, 0x01, 0x02, 0x17, 0x07, // size 1 + annotation (arg 0)
            ],
        );
        match one(&bytes) {
            EncodedValue::Array(vs) => {
                assert!(matches!(vs[0], EncodedValue::Annotation(_)));
            }
            other => panic!("esperava array, got {other:?}"),
        }
    }

    #[test]
    fn malformed_values_are_typed_errors() {
        // value_type 0x01 é reservado
        let e = one_err(&[0x01, 0x00]);
        assert_eq!(e.code, "INVALID_FORMAT");
        // float com value_arg 4 → shift impossível
        assert_eq!(one_err(&[0x10 | (4 << 5), 0x00]).code, "INVALID_FORMAT");
        // índice com value_arg 4 (> 3 bytes)
        assert_eq!(
            one_err(&[0x17 | (4 << 5), 0, 0, 0, 0, 0]).code,
            "INVALID_FORMAT"
        );
        // array truncado (promete 2 via uleb, só tem 1 valor)
        assert_eq!(one_err(&[0x1C, 0x02, 0x1E]).code, "PARSE_ERROR");
    }

    fn one_err(bytes: &[u8]) -> crate::error::RdError {
        let mut r = Reader::new(bytes);
        read_encoded_value(&mut r).unwrap_err()
    }

    #[test]
    fn deep_nesting_is_typed_error_not_stack_overflow() {
        // PoC da issue #8: N níveis de EncodedValue::Array aninhados — cada
        // nível custa 2 bytes (0x1C array + 0x01 count-uleb), NULL no fundo.
        // 10_000 níveis (~20 KB) antes abortavam o processo com stack overflow
        // (não-catchable); agora deve retornar erro TIPADO em tempo finito.
        let mut bytes = Vec::new();
        for _ in 0..10_000 {
            bytes.push(0x1C); // ARRAY
            bytes.push(0x01); // count = 1
        }
        bytes.push(0x1E); // NULL no fundo
        let e = one_err(&bytes);
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("64 níveis"), "mensagem: {}", e.cause);
        // profundidade dentro do teto continua funcionando (10 níveis ok)
        let mut ok = Vec::new();
        for _ in 0..10 {
            ok.push(0x1C);
            ok.push(0x01);
        }
        ok.push(0x1E);
        assert!(matches!(one(&ok), EncodedValue::Array(_)));
    }

    #[test]
    fn parse_encoded_array_static_values_shape() {
        // size=2: [Null, Boolean(true)]
        let data = [0x02, 0x1E, 0x3F];
        let vals = parse_encoded_array(&data, 0).expect("array");
        assert_eq!(vals, vec![EncodedValue::Null, EncodedValue::Boolean(true)]);
        // contagem maior que os dados → erro tipado (guard fuzz)
        let data = [0x7F, 0x1E];
        assert!(parse_encoded_array(&data, 0).is_err());
    }

    #[test]
    fn annotation_item_visibility_and_elements() {
        // visibility=runtime(1), type_idx=4, 2 elementos: a=Int(1), b=Null
        let mut d = Vec::new();
        d.push(0x01); // visibility runtime
        d.extend_from_slice(&[0x04]); // type_idx uleb
        d.extend_from_slice(&[0x02]); // size uleb
        d.extend_from_slice(&[0x01, 0x04, 0x01]); // name=1, Int(1)
        d.extend_from_slice(&[0x02, 0x1E]); // name=2, Null
        let item = parse_annotation_item(&d, 0).expect("annotation_item");
        assert_eq!(item.visibility, Visibility::Runtime);
        assert_eq!(item.annotation.type_idx, 4);
        assert_eq!(item.annotation.elements.len(), 2);
        assert_eq!(item.annotation.elements[0].name_idx, 1);
        assert_eq!(item.annotation.elements[0].value, EncodedValue::Int(1));
        assert_eq!(item.annotation.elements[1].value, EncodedValue::Null);
        // visibility inválida → erro tipado
        let mut bad = d.clone();
        bad[0] = 0x09;
        assert!(parse_annotation_item(&bad, 0).is_err());
    }

    #[test]
    fn annotation_set_and_directory_walk() {
        // annotation_set_item com 1 offset (prefixo dummy: offset 0 = vazio)
        let mut set = vec![0u8; 4];
        set.extend_from_slice(&1u32.to_le_bytes());
        set.extend_from_slice(&0x40u32.to_le_bytes());
        assert_eq!(parse_annotation_set_item(&set, 4).unwrap(), vec![0x40]);
        assert!(parse_annotation_set_item(&set, 0x100).is_err()); // além dos dados
        assert!(parse_annotation_set_item(&[], 0).unwrap().is_empty()); // off=0 → vazio

        // directory: class_ann=0, 1 field, 1 method, 1 param
        let mut d = Vec::new();
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&3u32.to_le_bytes()); // field_idx
        d.extend_from_slice(&0x50u32.to_le_bytes());
        d.extend_from_slice(&5u32.to_le_bytes()); // method_idx
        d.extend_from_slice(&0x60u32.to_le_bytes());
        d.extend_from_slice(&5u32.to_le_bytes()); // method_idx
        d.extend_from_slice(&0x70u32.to_le_bytes());
        let dir = parse_annotations_directory(&d, 0).expect("directory");
        assert_eq!(dir.offset, 0);
        assert_eq!(dir.field_annotations, vec![(3, 0x50)]);
        assert_eq!(dir.method_annotations, vec![(5, 0x60)]);
        assert_eq!(dir.parameter_annotations, vec![(5, 0x70)]);
        // listas maiores que os dados → erro tipado
        let mut big = d.clone();
        big[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(parse_annotations_directory(&big, 0).is_err());
    }

    #[test]
    fn visibility_roundtrip_and_names() {
        for (code, name) in [(0u8, "build"), (1, "runtime"), (2, "system")] {
            let v = Visibility::from_code(code).unwrap();
            assert_eq!(v.code(), code);
            assert_eq!(v.name(), name);
        }
        assert_eq!(Visibility::from_code(3).unwrap_err().code, "INVALID_FORMAT");
    }

    #[test]
    fn type_code_covers_all_variants() {
        let all = [
            EncodedValue::Byte(0),
            EncodedValue::Short(0),
            EncodedValue::Char(0),
            EncodedValue::Int(0),
            EncodedValue::Long(0),
            EncodedValue::Float(0.0),
            EncodedValue::Double(0.0),
            EncodedValue::MethodType(0),
            EncodedValue::MethodHandle(0),
            EncodedValue::String(0),
            EncodedValue::Type(0),
            EncodedValue::Field(0),
            EncodedValue::Method(0),
            EncodedValue::Enum(0),
            EncodedValue::Array(vec![]),
            EncodedValue::Annotation(EncodedAnnotation {
                type_idx: 0,
                elements: vec![],
            }),
            EncodedValue::Null,
            EncodedValue::Boolean(false),
        ];
        for (i, v) in all.iter().enumerate() {
            // todos os códigos conhecidos, na ordem do spec
            let expected = [
                0x00, 0x02, 0x03, 0x04, 0x06, 0x10, 0x11, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
                0x1c, 0x1d, 0x1e, 0x1f,
            ][i];
            assert_eq!(v.type_code(), expected, "type_code de {v:?}");
        }
    }
}
