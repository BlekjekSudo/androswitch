//! proto_id_item — assinaturas de método: shorty + return type + lista de
//! parâmetros (type_list). A type_list é lida sob demanda via `Dex`.

use crate::error::{RdError, RdResult};
use crate::read;

#[derive(Debug, Clone, Copy)]
pub struct ProtoId {
    /// índice na tabela de strings (ex.: `ILI`).
    pub shorty_idx: u32,
    /// índice na tabela de tipos (retorno).
    pub return_type_idx: u32,
    /// offset da type_list de parâmetros (0 = sem parâmetros).
    pub parameters_off: u32,
}

/// Lê uma type_list (`u32 size` + `size × u16 type_idx`).
/// Retorna os índices de tipo; offset 0 = lista vazia.
pub fn type_list(data: &[u8], off: u32) -> RdResult<Vec<u32>> {
    if off == 0 {
        return Ok(Vec::new());
    }
    let mut r = read::Reader::at(data, off as usize)?;
    let size = r.u32()? as usize;
    // guard fuzz: cada entrada tem 2 bytes
    if size.checked_mul(2).is_none() {
        return Err(RdError::parse("type_list: size overflow"));
    }
    let mut out = Vec::with_capacity(size.min(1 << 16));
    for _ in 0..size {
        out.push(r.u16()? as u32);
    }
    Ok(out)
}

pub(crate) fn validate_proto_window(data: &[u8], count: u32, off: u32) -> RdResult<&[u8]> {
    let count = count as usize;
    let off = off as usize;
    let byte_len = count
        .checked_mul(12)
        .ok_or_else(|| RdError::parse("protos: proto_ids_size overflow"))?;
    let window = data
        .get(off..)
        .ok_or_else(|| RdError::parse(format!("protos: table @ {off} além dos dados")))?;
    window.get(..byte_len).ok_or_else(|| {
        RdError::parse(format!(
            "protos: {} ids não cabem em {} bytes",
            count,
            window.len()
        ))
    })
}

#[inline]
pub(crate) fn decode_proto_id(b: &[u8]) -> ProtoId {
    ProtoId {
        shorty_idx: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        return_type_idx: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        parameters_off: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
    }
}

pub fn parse_proto_ids(data: &[u8], count: u32, off: u32) -> RdResult<Vec<ProtoId>> {
    let window = validate_proto_window(data, count, off)?;
    Ok(window.chunks_exact(12).map(decode_proto_id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protos_and_type_list() {
        // proto @ 4: shorty=0, ret=1, params_off=0 (vazio)
        let mut d = vec![0u8; 4];
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let protos = parse_proto_ids(&d, 1, 4).expect("protos");
        assert_eq!(protos.len(), 1);
        assert_eq!(protos[0].return_type_idx, 1);

        // type_list @ 4 (offset 0 = vazio por convenção do spec)
        let mut tl = 0u32.to_le_bytes().to_vec();
        tl.extend_from_slice(&2u32.to_le_bytes());
        tl.extend_from_slice(&7u16.to_le_bytes());
        tl.extend_from_slice(&9u16.to_le_bytes());
        assert_eq!(type_list(&tl, 4).unwrap(), vec![7, 9]);
        // offset 0 = vazio
        assert_eq!(type_list(&tl, 0).unwrap(), Vec::<u32>::new());
        assert_eq!(type_list(&[], 0).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn truncated_type_list_is_typed_error() {
        let d = [0u8, 0, 0, 0, 2, 0, 0, 0, 7, 0]; // promete 2, só tem 1
        assert!(type_list(&d, 4).is_err());
    }
}
