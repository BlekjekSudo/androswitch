//! method_id_item + encoded_method + method_handle_item + call_site offsets.

use crate::error::{RdError, RdResult};
use crate::read::{self, Reader};

#[derive(Debug, Clone, Copy)]
pub struct MethodId {
    /// índice na tabela de tipos (classe declarante).
    pub class_idx: u16,
    /// índice na tabela de protos.
    pub proto_idx: u16,
    /// índice na tabela de strings (nome).
    pub name_idx: u32,
}

pub(crate) fn validate_method_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
    let count = count as usize;
    let off = off as usize;
    let byte_len = count
        .checked_mul(8)
        .ok_or_else(|| RdError::parse("methods: method_ids_size overflow"))?;
    let window = data
        .get(off..)
        .ok_or_else(|| RdError::parse(format!("methods: table @ {off} além dos dados")))?;
    window.get(..byte_len).ok_or_else(|| {
        RdError::parse(format!(
            "methods: {} ids não cabem em {} bytes",
            count,
            window.len()
        ))
    })
}

#[inline]
pub(crate) fn decode_method_id(b: &[u8]) -> MethodId {
    MethodId {
        class_idx: u16::from_le_bytes([b[0], b[1]]),
        proto_idx: u16::from_le_bytes([b[2], b[3]]),
        name_idx: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
    }
}

pub fn parse_method_ids(data: &[u8], count: u32, off: u32) -> RdResult<Vec<MethodId>> {
    let window = validate_method_window(data, count, off)?;
    Ok(window.chunks_exact(8).map(decode_method_id).collect())
}

/// encoded_method (class_data): method_idx_diff + access_flags + code_off.
#[derive(Debug, Clone)]
pub struct EncodedMethod {
    /// índice absoluto na tabela de method_ids.
    pub method_idx: u32,
    pub access_flags: u32,
    /// offset do code_item (0 = sem corpo: abstract/native).
    pub code_off: u32,
    /// offset bruto do encoded_method (Lei 2).
    pub offset: usize,
}

pub(crate) fn read_encoded_method(r: &mut Reader) -> RdResult<EncodedMethod> {
    let offset = r.pos;
    let diff = r.uleb128()?;
    let flags = r.uleb128()?;
    let code_off = r.uleb128()?;
    Ok(EncodedMethod {
        method_idx: diff,
        access_flags: flags,
        code_off,
        offset,
    })
}

/// method_handle_item — usado por invoke-custom / const-method-handle.
#[derive(Debug, Clone, Copy)]
pub struct MethodHandle {
    pub method_handle_type: u16,
    /// índice para field_ids (static/instance get/put) ou method_ids (invoke-*).
    pub field_or_method_idx: u32,
}

/// Tipos de method handle (dex-format 038+).
pub mod method_handle_type {
    pub const STATIC_PUT: u16 = 0x00;
    pub const STATIC_GET: u16 = 0x01;
    pub const INSTANCE_PUT: u16 = 0x02;
    pub const INSTANCE_GET: u16 = 0x03;
    pub const INVOKE_STATIC: u16 = 0x04;
    pub const INVOKE_INSTANCE: u16 = 0x05;
    pub const INVOKE_CONSTRUCTOR: u16 = 0x06;
    pub const INVOKE_DIRECT: u16 = 0x07;
    pub const INVOKE_INTERFACE: u16 = 0x08;

    /// Nome smali do prefixo (`invoke-static@…`, `static-put@…`).
    pub fn name(t: u16) -> &'static str {
        match t {
            STATIC_PUT => "static-put",
            STATIC_GET => "static-get",
            INSTANCE_PUT => "instance-put",
            INSTANCE_GET => "instance-get",
            INVOKE_STATIC => "invoke-static",
            INVOKE_INSTANCE => "invoke-instance",
            INVOKE_CONSTRUCTOR => "invoke-constructor",
            INVOKE_DIRECT => "invoke-direct",
            INVOKE_INTERFACE => "invoke-interface",
            _ => "unknown",
        }
    }

    /// true se o handle referencia method_ids (invoke-*), false se field_ids.
    pub fn is_invoke(t: u16) -> bool {
        matches!(
            t,
            INVOKE_STATIC | INVOKE_INSTANCE | INVOKE_CONSTRUCTOR | INVOKE_DIRECT | INVOKE_INTERFACE
        )
    }
}

pub fn parse_method_handles(data: &[u8], count: u32, off: u32) -> RdResult<Vec<MethodHandle>> {
    let count = count as usize;
    let off = off as usize;
    if count.checked_mul(8).is_none() {
        return Err(RdError::parse("method_handles: count overflow"));
    }
    let window = data
        .get(off..)
        .ok_or_else(|| RdError::parse(format!("method_handles: table @ {off} além dos dados")))?;
    if count > window.len() / 8 {
        return Err(RdError::parse(format!(
            "method_handles: {} itens não cabem em {} bytes",
            count,
            window.len()
        )));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * 8;
        out.push(MethodHandle {
            method_handle_type: read::u16_at(window, base)?,
            // issue #36: layout do method_handle_item é u2 type, u2 unused,
            // u2 idx, u2 unused — ler u16 no offset 4 (o u32 antigo mesclava
            // o idx com o campo unused seguinte em DEX malformado)
            field_or_method_idx: read::u16_at(window, base + 4)? as u32,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_method_ids_table() {
        let mut d = Vec::new();
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&3u32.to_le_bytes());
        let m = parse_method_ids(&d, 1, 0).expect("methods");
        assert_eq!(m[0].proto_idx, 2);
        assert!(parse_method_ids(&d, 9, 0).is_err());
    }

    #[test]
    fn method_handle_names() {
        assert_eq!(method_handle_type::name(0x04), "invoke-static");
        assert_eq!(method_handle_type::name(0x00), "static-put");
        assert!(method_handle_type::is_invoke(0x06));
        assert!(!method_handle_type::is_invoke(0x01));
    }

    #[test]
    fn parses_method_handles() {
        let mut d = Vec::new();
        d.extend_from_slice(&0x04u16.to_le_bytes());
        d.extend_from_slice(&[0, 0]); // unused
        d.extend_from_slice(&7u32.to_le_bytes());
        let h = parse_method_handles(&d, 1, 0).expect("handles");
        assert_eq!(h[0].method_handle_type, 4);
        assert_eq!(h[0].field_or_method_idx, 7);
    }
}
