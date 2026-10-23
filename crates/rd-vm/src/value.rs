//! Valores da VM — registradores Dalvik são sem tipo, então o modelo usa um
//! superset: inteiros de 32 bits (incl. byte/short/char/boolean via conversões)
//! + wide (long/double ocupam 2 registradores) + referências de heap.

use crate::heap::ObjRef;
use alloc::{format, string::String};

/// Valor de registrador/campo/elemento. `Lower` marca a metade alta de um wide
/// (o par vN/vN+1 do Dalvik); nunca é lido diretamente — ler wide lê vN.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Obj(ObjRef),
    Null,
    /// metade alta (vN+1) de um long/double — placeholder não-legível
    WideHi,
    /// texto de string ainda não alocado no heap (entrada de `parse_arg_value`);
    /// o Engine converte para `Obj` no momento da invocação
    StrPlaceholder(String),
}

impl Value {
    pub fn as_int(&self) -> Result<i32, String> {
        match self {
            Value::Int(v) => Ok(*v),
            // Dalvik registers are untyped: move instructions preserve bits,
            // and the consuming opcode determines whether those bits are int
            // or float data.
            Value::Float(v) => Ok(v.to_bits() as i32),
            other => Err(format!("esperado int, got {other:?}")),
        }
    }

    pub fn as_long(&self) -> Result<i64, String> {
        match self {
            Value::Long(v) => Ok(*v),
            Value::Double(v) => Ok(v.to_bits() as i64),
            other => Err(format!("esperado long, got {other:?}")),
        }
    }

    pub fn as_float(&self) -> Result<f32, String> {
        match self {
            Value::Float(v) => Ok(*v),
            Value::Int(v) => Ok(f32::from_bits(*v as u32)),
            other => Err(format!("esperado float, got {other:?}")),
        }
    }

    pub fn as_double(&self) -> Result<f64, String> {
        match self {
            Value::Double(v) => Ok(*v),
            Value::Long(v) => Ok(f64::from_bits(*v as u64)),
            other => Err(format!("esperado double, got {other:?}")),
        }
    }

    /// Referência (Null → None). Obj(0) nunca existe (índice 0 é reservado).
    ///
    /// Semântica Dalvik: registradores são SEM tipo — o javac/d8 emite
    /// `const/4 v0, 0` para o literal `null`, e no ponto de USO como
    /// referência o padrão de bits 0 É null (bug real pego no golden M2:
    /// caso nullStringLen do harness). Int(0) usado como referência = null.
    pub fn as_ref(&self) -> Result<Option<ObjRef>, String> {
        match self {
            Value::Obj(r) => Ok(Some(*r)),
            Value::Int(0) => Ok(None),
            Value::Null => Ok(None),
            other => Err(format!("esperado referência, got {other:?}")),
        }
    }

    /// `true` se ocupa 2 registradores.
    pub fn is_wide(&self) -> bool {
        matches!(self, Value::Long(_) | Value::Double(_))
    }

    /// Representação curta para mensagens de erro (sem alocar String bonita).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Long(_) => "long",
            Value::Float(_) => "float",
            Value::Double(_) => "double",
            Value::Obj(_) => "object",
            Value::Null => "null",
            Value::WideHi => "wide-hi",
            Value::StrPlaceholder(_) => "string",
        }
    }
}

/// Converte um i32 cru para o tipo declarado (`B`/`C`/`S`/`I`/`Z`) — as
/// conversões de saída do interpretador (retornos, aget, iget) aplicam o
/// truncamento/extensão do tipo Dalvik.
pub fn narrow_i32(raw: i32, desc: &str) -> i32 {
    match desc {
        "B" => raw as i8 as i32,
        "C" => raw as u16 as i32,
        "S" => raw as i16 as i32,
        "Z" => (raw != 0) as i32,
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_are_typed() {
        assert_eq!(Value::Int(7).as_int(), Ok(7));
        assert!(Value::Long(7).as_int().is_err());
        assert_eq!(Value::Null.as_ref(), Ok(None));
        assert_eq!(Value::Obj(3).as_ref(), Ok(Some(3)));
        assert!(!Value::WideHi.is_wide());
        assert!(Value::Long(0).is_wide());
    }

    #[test]
    fn primitive_accessors_reinterpret_untyped_dalvik_bits() {
        let f = 0.75f32;
        assert_eq!(Value::Int(f.to_bits() as i32).as_float(), Ok(f));
        assert_eq!(Value::Float(f).as_int(), Ok(f.to_bits() as i32));
        let d = -12.5f64;
        assert_eq!(Value::Long(d.to_bits() as i64).as_double(), Ok(d));
        assert_eq!(Value::Double(d).as_long(), Ok(d.to_bits() as i64));
    }

    #[test]
    fn narrowing_follows_dalvik() {
        assert_eq!(narrow_i32(0x1FF, "B"), -1);
        assert_eq!(narrow_i32(0x1_0041, "C"), 0x41);
        assert_eq!(narrow_i32(-1, "S"), -1);
        assert_eq!(narrow_i32(7, "Z"), 1);
        assert_eq!(narrow_i32(-7, "I"), -7);
    }
}
