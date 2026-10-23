//! Detecção de esquemas de assinatura de APK (v1 / v2 / v3 / v3.1).
//!
//! Escopo M0: presença + versões (não validação criptográfica — chega em fase
//! posterior). v2/v3 = "APK Sig Block 42" antes do central directory (docs
//! source.android.com → APK Signature Scheme v2/v3). v1 = JAR signing (META-INF).

use crate::zip::Zip;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SigningInfo {
    /// JAR signing (META-INF/*.RSA|.DSA|.EC + MANIFEST.MF).
    pub v1: bool,
    /// APK Signature Scheme v2.
    pub v2: bool,
    /// v3 (rotação de chaves).
    pub v3: bool,
    /// v3.1 (rotação com lineage duplo).
    pub v3_1: bool,
}

impl SigningInfo {
    pub fn schemes(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.v1 {
            v.push("v1");
        }
        if self.v2 {
            v.push("v2");
        }
        if self.v3 {
            v.push("v3");
        }
        if self.v3_1 {
            v.push("v3.1");
        }
        v
    }

    pub fn is_signed(&self) -> bool {
        self.v1 || self.v2 || self.v3 || self.v3_1
    }
}

pub const APK_SIG_BLOCK_MAGIC: &[u8; 16] = b"APK Sig Block 42";
pub const BLOCK_ID_V2: u32 = 0x7109_871a;
pub const BLOCK_ID_V3: u32 = 0xf053_68c0;
pub const BLOCK_ID_V3_1: u32 = 0x3ba0_6f8c;

/// Inspect the signing footer/block using bounded positional reads. Presence
/// detection is identical to the resident parser; signatures are not verified.
pub fn detect_zip(zip: &Zip) -> crate::RdResult<SigningInfo> {
    if let Ok(bytes) = zip.raw_bytes() {
        return Ok(detect(bytes, zip.cd_offset(), zip));
    }
    let end = zip.cd_offset();
    if end < 24 {
        return Ok(detect(&[], 0, zip));
    }
    let footer = zip.read_range(end - 24, 24)?;
    if &footer[8..] != APK_SIG_BLOCK_MAGIC {
        return Ok(detect(&[], 0, zip));
    }
    let size = u64_at(&footer, 0).unwrap_or(0);
    let total = size
        .checked_add(8)
        .filter(|n| *n <= end && *n <= 16 << 20)
        .ok_or_else(|| {
            crate::RdError::invalid_format("APK signing block exceeds extent/16 MiB budget")
        })?;
    let block = zip.read_range(end - total, total as usize)?;
    Ok(detect(&block, total, zip))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
fn u64_at(d: &[u8], off: usize) -> Option<u64> {
    d.get(off..off + 8).map(|b| {
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        u64::from_le_bytes(a)
    })
}

/// `data` = bytes completos do APK; `cd_offset` = offset do central directory (do EOCD).
pub fn detect(data: &[u8], cd_offset: u64, zip: &Zip) -> SigningInfo {
    let mut info = SigningInfo {
        v1: detect_v1(zip),
        ..Default::default()
    };

    // bloco de assinatura termina exatamente onde o CD começa.
    // issue #18: `cd_offset` vem do EOCD (campo do atacante) — bound-check
    // obrigatório antes de qualquer slice; o sentinela ZIP64 (0xFFFFFFFF)
    // nunca aponta para um bloco real (o Zip::parse resolve ZIP64 e expõe o
    // offset verdadeiro via `cd_offset()`)
    if cd_offset >= 32 && cd_offset != 0xFFFF_FFFF && cd_offset <= data.len() as u64 {
        let cd = cd_offset as usize;
        if &data[cd - 16..cd] == APK_SIG_BLOCK_MAGIC {
            // issue #18: block_size lido do arquivo — toda aritmética checked
            // (a soma não-checada `block_size + 8` envolvia em release)
            if let Some(block_size) = u64_at(data, cd - 24).map(|v| v as usize) {
                if block_size.checked_add(8).is_some_and(|v| v <= cd) {
                    let block_start = cd - block_size - 8;
                    if block_start + 8 <= data.len() {
                        walk_pairs(data, block_start + 8, cd - 24, &mut info);
                    }
                }
            }
        }
    }
    info
}

fn walk_pairs(data: &[u8], mut p: usize, end: usize, info: &mut SigningInfo) {
    // issue #18: aritmética checked em toda etapa — pair_len do atacante
    // (2^64-8) envolvia em release e pendurava o processo em loop infinito;
    // em debug pânica. Par malformado apenas encerra a varredura.
    while let Some(q) = p.checked_add(12) {
        if q > end || q > data.len() {
            break;
        }
        let Some(pair_len) = u64_at(data, p) else {
            break;
        };
        // fim do par: p + 8 + pair_len (u64 para não truncar/envolver)
        let Some(pair_end) = (p as u64)
            .checked_add(8)
            .and_then(|v| v.checked_add(pair_len))
        else {
            break;
        };
        if pair_len < 4 || pair_end > (end as u64).saturating_add(8) || pair_end > data.len() as u64
        {
            break; // par malformado: não travamos a detecção
        }
        let Some(id) = u32_at(data, p + 8) else { break };
        match id {
            BLOCK_ID_V2 => info.v2 = true,
            BLOCK_ID_V3 => info.v3 = true,
            BLOCK_ID_V3_1 => info.v3_1 = true,
            _ => {}
        }
        p = pair_end as usize;
    }
}

fn detect_v1(zip: &Zip) -> bool {
    let mut has_manifest_mf = false;
    let mut has_sig = false;
    for name in zip.names() {
        if name == "META-INF/MANIFEST.MF" {
            has_manifest_mf = true;
        }
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("meta-inf/")
            && (lower.ends_with(".rsa") || lower.ends_with(".dsa") || lower.ends_with(".ec"))
        {
            has_sig = true;
        }
    }
    has_manifest_mf && has_sig
}
