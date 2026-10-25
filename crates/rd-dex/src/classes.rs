//! class_def_item + class_data_item — classes, campos e métodos por classe.

use crate::error::{RdError, RdResult};
use crate::fields::{self, EncodedField};
use crate::methods::EncodedMethod;
use crate::read::Reader;

/// class_def_item (20 bytes fixos).
#[derive(Debug, Clone, Copy)]
pub struct ClassDef {
    pub index: usize,
    /// índice na tabela de tipos.
    pub class_idx: u32,
    pub access_flags: u32,
    /// índice de tipo da superclasse (NO_INDEX = Object raiz... só para Object).
    pub superclass_idx: u32,
    /// offset da type_list de interfaces (0 = nenhuma).
    pub interfaces_off: u32,
    /// índice de string do arquivo-fonte (NO_INDEX = ausente).
    pub source_file_idx: u32,
    /// offset do annotations_directory_item (0 = nenhum).
    pub annotations_off: u32,
    /// offset do class_data_item (0 = classe sem membros).
    pub class_data_off: u32,
    /// offset do encoded_array de valores estáticos iniciais.
    pub static_values_off: u32,
}

pub(crate) fn validate_class_defs_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
    let count = count as usize;
    let off = off as usize;
    let byte_len = count
        .checked_mul(32)
        .ok_or_else(|| RdError::parse("classes: class_defs_size overflow"))?;
    let window = data
        .get(off..)
        .ok_or_else(|| RdError::parse(format!("classes: table @ {off} além dos dados")))?;
    window.get(..byte_len).ok_or_else(|| {
        RdError::parse(format!(
            "classes: {} defs não cabem em {} bytes",
            count,
            window.len()
        ))
    })
}

#[inline]
pub(crate) fn decode_class_def(index: usize, b: &[u8]) -> ClassDef {
    ClassDef {
        index,
        class_idx: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        access_flags: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        superclass_idx: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        interfaces_off: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
        source_file_idx: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
        annotations_off: u32::from_le_bytes([b[20], b[21], b[22], b[23]]),
        class_data_off: u32::from_le_bytes([b[24], b[25], b[26], b[27]]),
        static_values_off: u32::from_le_bytes([b[28], b[29], b[30], b[31]]),
    }
}

pub fn parse_class_defs(data: &[u8], count: u32, off: u32) -> RdResult<Vec<ClassDef>> {
    let window = validate_class_defs_window(data, count, off)?;
    Ok(window
        .chunks_exact(32)
        .enumerate()
        .map(|(i, b)| decode_class_def(i, b))
        .collect())
}

/// class_data_item — membros da classe em uleb128 com diffs acumulados.
#[derive(Debug, Clone, Default)]
pub struct ClassData {
    pub static_fields: Vec<EncodedField>,
    pub instance_fields: Vec<EncodedField>,
    pub direct_methods: Vec<EncodedMethod>,
    pub virtual_methods: Vec<EncodedMethod>,
    /// offset bruto do class_data_item (Lei 2).
    pub offset: usize,
}

impl ClassData {
    pub fn total_fields(&self) -> usize {
        self.static_fields.len() + self.instance_fields.len()
    }

    pub fn total_methods(&self) -> usize {
        self.direct_methods.len() + self.virtual_methods.len()
    }
}

/// Parse do class_data_item em `off` (count 0 / off 0 → vazio).
pub fn parse_class_data(data: &[u8], off: u32) -> RdResult<ClassData> {
    if off == 0 {
        return Ok(ClassData::default());
    }
    let mut r = Reader::at(data, off as usize)?;
    let static_n = r.uleb128()?;
    let instance_n = r.uleb128()?;
    let direct_n = r.uleb128()?;
    let virtual_n = r.uleb128()?;

    // guards fuzz: cada entrada tem no mínimo 1 byte (uleb); limita pelo tamanho
    let total = static_n
        .checked_add(instance_n)
        .and_then(|v| v.checked_add(direct_n))
        .and_then(|v| v.checked_add(virtual_n));
    match total {
        Some(t) if t as usize > r.remaining() => {
            return Err(RdError::parse(format!(
                "class_data: {} entradas > {} bytes restantes @ 0x{:x}",
                t,
                r.remaining(),
                off
            )));
        }
        None => return Err(RdError::parse("class_data: contagem overflow")),
        Some(_) => {}
    }

    let mut cd = ClassData {
        offset: off as usize,
        ..Default::default()
    };

    let mut idx: u32 = 0;
    for _ in 0..static_n {
        let mut f = fields::read_encoded_field(&mut r)?;
        // issue #36: delta overflow falha tipado (não wrapping silencioso)
        idx = idx.checked_add(f.field_idx).ok_or_else(|| {
            RdError::invalid_format("class_data: delta overflow nos índices de campo")
        })?;
        f.field_idx = idx;
        cd.static_fields.push(f);
    }
    idx = 0;
    for _ in 0..instance_n {
        let mut f = fields::read_encoded_field(&mut r)?;
        idx = idx.checked_add(f.field_idx).ok_or_else(|| {
            RdError::invalid_format("class_data: delta overflow nos índices de campo")
        })?;
        f.field_idx = idx;
        cd.instance_fields.push(f);
    }
    idx = 0;
    for _ in 0..direct_n {
        let mut m = read_method(&mut r)?;
        idx = idx.checked_add(m.method_idx).ok_or_else(|| {
            RdError::invalid_format("class_data: delta overflow nos índices de método")
        })?;
        m.method_idx = idx;
        cd.direct_methods.push(m);
    }
    idx = 0;
    for _ in 0..virtual_n {
        let mut m = read_method(&mut r)?;
        idx = idx.checked_add(m.method_idx).ok_or_else(|| {
            RdError::invalid_format("class_data: delta overflow nos índices de método")
        })?;
        m.method_idx = idx;
        cd.virtual_methods.push(m);
    }
    Ok(cd)
}

fn read_method(r: &mut Reader) -> RdResult<EncodedMethod> {
    crate::methods::read_encoded_method(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_class_data() {
        // 1 static field, 1 instance field, 1 direct, 1 virtual
        // (prefixo dummy: offset 0 é a convenção "sem class_data")
        let mut d = vec![0u8];
        d.extend_from_slice(&[0x01]); // static_fields_size
        d.extend_from_slice(&[0x01]); // instance_fields_size
        d.extend_from_slice(&[0x01]); // direct_methods_size
        d.extend_from_slice(&[0x02]); // virtual_methods_size
                                      // static field: diff 0x05, flags 0x09
        d.extend_from_slice(&[0x05, 0x09]);
        // instance field: diff 0x03, flags 0x02
        d.extend_from_slice(&[0x03, 0x02]);
        // direct method: diff 0x07, flags 0x09, code_off 0x100
        d.extend_from_slice(&[0x07, 0x09, 0x80, 0x02]);
        // virtual methods (2): diff 1 flags 1 code 0; diff 2 flags 1 code 0x10
        d.extend_from_slice(&[0x01, 0x01, 0x00]);
        d.extend_from_slice(&[0x02, 0x01, 0x10]);

        let cd = parse_class_data(&d, 1).expect("class_data");
        assert_eq!(cd.static_fields[0].field_idx, 5);
        assert_eq!(cd.instance_fields[0].field_idx, 3);
        assert_eq!(cd.direct_methods[0].method_idx, 7);
        assert_eq!(cd.direct_methods[0].code_off, 0x100);
        assert_eq!(cd.virtual_methods.len(), 2);
        // diffs acumulam: primeiro = 0+1, segundo = 1+2
        assert_eq!(cd.virtual_methods[1].method_idx, 3);
    }

    #[test]
    fn count_overflow_is_typed_error() {
        // promete 2^32-1 static fields (prefixo dummy: offset 0 = vazio)
        let d = [0u8, 0xff, 0xff, 0xff, 0xff, 0x0f];
        assert!(parse_class_data(&d, 1).is_err());
    }

    #[test]
    fn zero_offset_is_empty() {
        let cd = parse_class_data(&[], 0).expect("vazio");
        assert!(cd.is_empty_class() || cd.total_methods() == 0);
    }

    impl ClassData {
        fn is_empty_class(&self) -> bool {
            self.total_fields() == 0
        }
    }
}
