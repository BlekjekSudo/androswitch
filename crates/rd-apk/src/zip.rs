//! Leitor ZIP especializado em APK — sem alocação desnecessária, ZIP64, CRC-32.
//!
//! Escopo deliberado (M0): leitura apenas (APKs são lidos, nunca reescritos aqui).
//! STORED + DEFLATE; os tamanhos do *central directory* são autoritativos
//! (apps malformados mentem no local header — o CD é o registro de verdade).

use crate::error::{RdError, RdResult};
use crate::paged::PagedZip;
use flate2::read::DeflateDecoder;
use std::io::Read;
use std::sync::Arc;

pub const EOCD_SIG: u32 = 0x0605_4b50;
pub const ZIP64_EOCD_LOCATOR_SIG: u32 = 0x0706_4b50;
pub const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
pub const CD_ENTRY_SIG: u32 = 0x0201_4b50;
pub const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;

const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    pub flags: u16,
}

impl ZipEntry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
    pub fn is_stored(&self) -> bool {
        self.method == METHOD_STORED
    }
}

/// Fonte dos bytes do container (ADR 008 Stage 2): arquivo inteiro em
/// memória (caminho histórico, ≤ `Apk::MAX_INPUT_BYTES`) ou leitura
/// paginada sob demanda para arquivos grandes (até 16 GiB, RSS travado
/// no cache de páginas do [`PagedZip`]). A política de leitura —
/// métodos, orçamentos anti-bomba, CRC — é idêntica nos dois modos.
#[derive(Clone)]
enum ZipSource {
    Whole(Vec<u8>),
    Paged(Arc<PagedZip>),
}

impl std::fmt::Debug for ZipSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipSource::Whole(d) => write!(f, "Whole({} bytes)", d.len()),
            ZipSource::Paged(p) => write!(f, "Paged({} bytes)", p.file_len()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Zip {
    source: ZipSource,
    entries: Vec<ZipEntry>,
    /// Offset do central directory já resolvido (ZIP64-aware) — fonte única
    /// para a detecção de assinatura (issue #35: o re-scan duplicado em
    /// lib.rs tinha off-by-one e perdia o sentinela ZIP64).
    /// issue #84: nomes que aparecem MAIS DE UMA VEZ no central directory
    /// (ambiguidade: qual entrada o sistema instalaria?). O rd segue lendo a
    /// 1ª ocorrência, mas a duplicidade é reportada — nunca silenciosa.
    duplicates: Vec<String>,
    cd_offset: u64,
}

// ─── leitores little-endian com bounds-check ────────────────────────────────
//
// Issue #331 (item 1): offsets ZIP64 são u64 de atacante. `off + N` sem
// checked_add panica em debug (`attempt to add with overflow`) e WRAP
// em release — um container de 42 bytes virava T0. Mesmo padrão `ck`
// que a #245 aplicou em `paged.rs`.

/// `off + k` checado — para sites que somam antes de indexar.
#[inline]
fn off_add(off: usize, k: usize) -> RdResult<usize> {
    off.checked_add(k)
        .ok_or_else(|| RdError::parse(format!("zip: offset overflow ({off} + {k})")))
}

/// `u64 -> usize` checado (sem truncar silenciosamente em 32-bit).
#[inline]
fn usz(v: u64) -> RdResult<usize> {
    usize::try_from(v).map_err(|_| RdError::parse(format!("zip: 64-bit value {v} exceeds usize")))
}

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    let end = off_add(off, 2)?;
    d.get(off..end)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("zip: truncated u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    let end = off_add(off, 4)?;
    d.get(off..end)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("zip: truncated u32 @ {off}")))
}

#[inline]
fn u64_at(d: &[u8], off: usize) -> RdResult<u64> {
    let end = off_add(off, 8)?;
    d.get(off..end)
        .map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
        .ok_or_else(|| RdError::parse(format!("zip: truncated u64 @ {off}")))
}

// ─── CRC-32 (IEEE 802.3, tabela em const fn — zero custo em runtime) ───────

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = make_crc_table();

const fn make_crc_tables8() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    tables[0] = make_crc_table();
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = tables[k - 1][i];
            tables[k][i] = tables[0][(prev & 0xFF) as usize] ^ (prev >> 8);
            i += 1;
        }
        k += 1;
    }
    tables
}

static CRC_TABLES8: [[u32; 256]; 8] = make_crc_tables8();

pub fn crc32(data: &[u8]) -> u32 {
    // Slicing-by-8 (ADR 008 Stage 1b): 8 table lookups per 8 input bytes;
    // bit-identical results to the byte-at-a-time form above.
    let mut c: u32 = 0xFFFF_FFFF;
    let mut chunks = data.chunks_exact(8);
    for ch in &mut chunks {
        let one = u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]) ^ c;
        let two = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
        c = CRC_TABLES8[7][(one & 0xFF) as usize]
            ^ CRC_TABLES8[6][((one >> 8) & 0xFF) as usize]
            ^ CRC_TABLES8[5][((one >> 16) & 0xFF) as usize]
            ^ CRC_TABLES8[4][(one >> 24) as usize]
            ^ CRC_TABLES8[3][(two & 0xFF) as usize]
            ^ CRC_TABLES8[2][((two >> 8) & 0xFF) as usize]
            ^ CRC_TABLES8[1][((two >> 16) & 0xFF) as usize]
            ^ CRC_TABLES8[0][(two >> 24) as usize];
    }
    for &b in chunks.remainder() {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

// ─── ZIP ────────────────────────────────────────────────────────────────────

impl Zip {
    /// Parse de um APK/ZIP já carregado em memória.
    pub fn parse(data: Vec<u8>) -> RdResult<Zip> {
        let (eocd_off, entry_count, cd_size, cd_offset) = Self::find_eocd(&data)?;
        let _ = eocd_off; // (usado apenas pela busca; ZIP64 já resolvido abaixo)

        // Bounds do central directory (#331: sem truncar u64 em 32-bit)
        let cd_start = usz(cd_offset)?;
        let cd_size_u = usz(cd_size)?;
        let cd_end = cd_start
            .checked_add(cd_size_u)
            .ok_or_else(|| RdError::parse("zip: CD size overflow"))?;
        if cd_end > data.len() {
            return Err(RdError::parse(format!(
                "zip: central directory [{cd_start}..{cd_end}) exceeds file (len {})",
                data.len()
            )));
        }

        // Pre-alloc limitada pelo que o CD pode conter (espelha paged.rs):
        // um header ZIP64 minúsculo não pré-aloca 100k entradas.
        let cap = entry_count.min(cd_size_u / 46).min(100_000);
        let mut entries = Vec::with_capacity(cap);
        let mut p = cd_start;
        for i in 0..entry_count {
            // O laço nunca sai do CD: cabeçalho fixo (46 B) inteiro dentro.
            if off_add(p, 46)? > cd_end {
                return Err(RdError::parse(format!(
                    "zip: CD entry #{i} at {p} exceeds central directory end {cd_end}"
                )));
            }
            // strides lidos ANTES do parse (nome é bytes crus — lossy muda o len)
            let name_len = u16_at(&data, off_add(p, 28)?)? as usize;
            let extra_len = extra_len_of(&data, off_add(p, 30)?)?;
            let comment_len = comment_len_of(&data, off_add(p, 32)?)?;
            let entry = Self::parse_cd_entry(&data, p)
                .map_err(|e| e.with_suggestion(format!("parsing central-directory entry #{i}")))?;
            entries.push(entry);
            let stride = off_add(46, name_len + extra_len + comment_len)?;
            p = off_add(p, stride)?;
            if p > cd_end {
                return Err(RdError::parse(format!(
                    "zip: CD entry #{i} stride walks past central directory end {cd_end}"
                )));
            }
        }

        // issue #84: detecção de nomes repetidos (ordem de aparição).
        let duplicates = Self::compute_duplicates(&entries);

        Ok(Zip {
            source: ZipSource::Whole(data),
            entries,
            duplicates,
            cd_offset,
        })
    }

    /// Lê arquivo do disco e faz parse.
    pub fn open(path: &std::path::Path) -> RdResult<Zip> {
        let data = std::fs::read(path)
            .map_err(|e| RdError::io(&e, format!("reading {}", path.display())))?;
        Self::parse(data)
    }

    /// Abre do disco em modo PAGINADO (ADR 008 Stage 2): só o diretório
    /// central é materializado; os payloads leem sob demanda com RSS
    /// travado (cache de 16 MiB do [`PagedZip`]). É a rota de
    /// `Apk::open` acima de `Apk::MAX_INPUT_BYTES` — o teto efetivo
    /// sobe para os 16 GiB do `PagedZip`.
    pub fn open_paged(path: &std::path::Path) -> RdResult<Zip> {
        let paged = PagedZip::open(path)?;
        let entries = paged.entries().to_vec();
        let duplicates = Self::compute_duplicates(&entries);
        let cd_offset = paged.cd_offset();
        Ok(Zip {
            source: ZipSource::Paged(Arc::new(paged)),
            entries,
            duplicates,
            cd_offset,
        })
    }

    /// issue #84: nomes que aparecem mais de uma vez no CD (ordem de
    /// aparição) — mesma regra nos modos whole e paginado.
    fn compute_duplicates(entries: &[ZipEntry]) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut dup_seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for e in entries {
            if !seen.insert(e.name.as_str()) && dup_seen.insert(e.name.as_str()) {
                duplicates.push(e.name.clone());
            }
        }
        duplicates
    }

    /// Varre o fim do arquivo por EOCD; resolve ZIP64 quando presente.
    fn find_eocd(data: &[u8]) -> RdResult<(usize, usize, u64, u64)> {
        if data.len() < 22 {
            return Err(RdError::parse("zip: file shorter than EOCD (22 bytes)"));
        }
        let max_back = 22 + 65_536; // EOCD + comentário máximo
        let start = data.len().saturating_sub(max_back);
        let mut eocd_pos = None;
        let mut i = data.len() - 22;
        loop {
            if u32_at(data, i)? == EOCD_SIG {
                eocd_pos = Some(i);
                break;
            }
            if i == start {
                break;
            }
            i -= 1;
        }
        let eocd = eocd_pos.ok_or_else(|| RdError::parse("zip: EOCD signature not found"))?;

        let mut entry_count = u16_at(data, eocd + 10)? as usize;
        let mut cd_size = u32_at(data, eocd + 12)? as u64;
        let mut cd_offset = u32_at(data, eocd + 16)? as u64;

        // ZIP64: campos sentinel 0xFFFF/0xFFFFFFFF
        if entry_count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
            // locator fica imediatamente antes do EOCD (20 bytes)
            let loc = eocd.checked_sub(20).ok_or_else(|| {
                RdError::parse("zip: ZIP64 sentinel present but locator cannot fit")
            })?;
            if u32_at(data, loc)? != ZIP64_EOCD_LOCATOR_SIG {
                return Err(RdError::parse(
                    "zip: ZIP64 sentinel without locator — corrupt container",
                ));
            }
            let z64_off = usz(u64_at(data, off_add(loc, 8)?)?)?;
            if u32_at(data, z64_off)? != ZIP64_EOCD_SIG {
                return Err(RdError::parse("zip: ZIP64 EOCD signature mismatch"));
            }
            entry_count = usz(u64_at(data, off_add(z64_off, 32)?)?)?;
            cd_size = u64_at(data, off_add(z64_off, 40)?)?;
            cd_offset = u64_at(data, off_add(z64_off, 48)?)?;
        }
        Ok((eocd, entry_count, cd_size, cd_offset))
    }

    fn parse_cd_entry(data: &[u8], off: usize) -> RdResult<ZipEntry> {
        if u32_at(data, off)? != CD_ENTRY_SIG {
            return Err(RdError::parse(format!(
                "zip: bad CD entry signature at {off}"
            )));
        }
        let flags = u16_at(data, off_add(off, 8)?)?;
        let method = u16_at(data, off_add(off, 10)?)?;
        let crc = u32_at(data, off_add(off, 16)?)?;
        let mut csize = u32_at(data, off_add(off, 20)?)? as u64;
        let mut usize_ = u32_at(data, off_add(off, 24)?)? as u64;
        let name_len = u16_at(data, off_add(off, 28)?)? as usize;
        let extra_len = u16_at(data, off_add(off, 30)?)? as usize;
        let _comment_len = u16_at(data, off_add(off, 32)?)? as usize;
        let mut lho = u32_at(data, off_add(off, 42)?)? as u64;

        let var_start = off_add(off, 46)?;
        let name_bytes = data
            .get(var_start..off_add(var_start, name_len)?)
            .ok_or_else(|| RdError::parse("zip: CD entry name out of bounds"))?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();

        // ZIP64 extra field (0x0001) — campos na ordem: usize, csize, lho
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF || lho == 0xFFFF_FFFF {
            let extra_start = off_add(var_start, name_len)?;
            let extra = data
                .get(extra_start..off_add(extra_start, extra_len)?)
                .ok_or_else(|| RdError::parse("zip: CD extra out of bounds"))?;
            let mut q = 0usize;
            while q + 4 <= extra.len() {
                let id = u16_at(extra, q)?;
                let sz = u16_at(extra, q + 2)? as usize;
                if id == 0x0001 {
                    let mut f = q + 4;
                    if usize_ == 0xFFFF_FFFF && f + 8 <= q + 4 + sz {
                        usize_ = u64_at(extra, f)?;
                        f += 8;
                    }
                    if csize == 0xFFFF_FFFF && f + 8 <= q + 4 + sz {
                        csize = u64_at(extra, f)?;
                        f += 8;
                    }
                    if lho == 0xFFFF_FFFF && f + 8 <= q + 4 + sz {
                        lho = u64_at(extra, f)?;
                    }
                    break;
                }
                q += 4 + sz;
            }
        }

        Ok(ZipEntry {
            name,
            method,
            crc32: crc,
            compressed_size: csize,
            uncompressed_size: usize_,
            local_header_offset: lho,
            flags,
        })
    }

    /// Descomprime e verifica CRC da entrada. Retornando `Ok`, os bytes são íntegros.
    pub fn read(&self, name: &str) -> RdResult<Vec<u8>> {
        let entry = self
            .find(name)
            .ok_or_else(|| RdError::missing_entry(name))?;
        self.read_entry(entry)
    }

    pub fn read_entry(&self, entry: &ZipEntry) -> RdResult<Vec<u8>> {
        match &self.source {
            // O PagedZip aplica a MESMA política de leitura (métodos,
            // orçamentos anti-bomba, CRC) — paridade coberta pelo teste
            // `open_paged_matches_whole_parse`.
            ZipSource::Paged(p) => p.read_entry(entry),
            ZipSource::Whole(data) => Self::read_entry_whole(data, entry),
        }
    }

    fn read_entry_whole(data: &[u8], entry: &ZipEntry) -> RdResult<Vec<u8>> {
        // #331: lho vem do extra ZIP64 do CD (u64 de atacante) — checado.
        let off = usz(entry.local_header_offset)?;
        if u32_at(data, off)? != LOCAL_HEADER_SIG {
            return Err(RdError::parse(format!(
                "zip: bad local header for {:?}",
                entry.name
            )));
        }
        // usar extra_len do LOCAL header (pode diferir do CD!)
        let name_len = u16_at(data, off_add(off, 26)?)? as usize;
        let extra_len = u16_at(data, off_add(off, 28)?)? as usize;
        let data_start = off_add(off_add(off, 30)?, name_len + extra_len)?;
        let data_end = data_start
            .checked_add(usz(entry.compressed_size)?)
            .ok_or_else(|| RdError::parse("zip: compressed size overflow"))?;
        let compressed = data.get(data_start..data_end).ok_or_else(|| {
            RdError::parse(format!("zip: payload of {:?} out of bounds", entry.name))
        })?;

        let out: Vec<u8> = match entry.method {
            METHOD_STORED => compressed.to_vec(),
            METHOD_DEFLATE => {
                // Bomba de descompressão (issues #14 e #30): dois tetos valem
                // DURANTE/Antes do inflate —
                // (a) orçamento GLOBAL de 512 MB checado ANTES de inflar: o
                //     `declared` é campo do atacante; 712 KB podiam declarar
                //     600 MB "honestos" e alocar isso na RAM. Piso E5 do spec:
                //     processo ≤ 512 MB. Rejeita sem alocar.
                // (b) o `declared` do CD vale durante o inflate (take + 1 byte
                //     de folga): pega stream que expande além do declarado.
                const MAX_INFLATE_BUDGET: u64 = 512 * 1024 * 1024;
                let declared = entry.uncompressed_size;
                if declared > MAX_INFLATE_BUDGET {
                    return Err(RdError::invalid_format(format!(
                        "zip: {:?} declara {} bytes descomprimidos (> teto global de {} bytes): entrada rejeitada antes do inflate (suspeita de bomba)",
                        entry.name, declared, MAX_INFLATE_BUDGET
                    )));
                }
                let mut decoder = DeflateDecoder::new(compressed).take(declared.saturating_add(1));
                let mut buf = Vec::with_capacity(declared.min(64 * 1024 * 1024) as usize);
                decoder
                    .read_to_end(&mut buf)
                    .map_err(|e| RdError::parse(format!("zip: inflate {:?}: {e}", entry.name)))?;
                if buf.len() as u64 > declared {
                    return Err(RdError::invalid_format(format!(
                        "zip: {:?} expandiu além do declarado no CD (> {} bytes): descompressão limitada (suspeita de bomba)",
                        entry.name, declared
                    )));
                }
                buf
            }
            m => {
                return Err(RdError::parse(format!(
                    "zip: unsupported method {m} for {:?}",
                    entry.name
                ))
                .with_suggestion("only STORED (0) and DEFLATE (8) are valid in APKs"));
            }
        };

        if out.len() as u64 != entry.uncompressed_size {
            return Err(RdError::invalid_format(format!(
                "zip: {:?} inflated to {} bytes, CD says {}",
                entry.name,
                out.len(),
                entry.uncompressed_size
            )));
        }
        let actual = crc32(&out);
        if actual != entry.crc32 {
            return Err(RdError::invalid_format(format!(
                "zip: CRC mismatch for {:?} (stored 0x{:08x}, computed 0x{:08x})",
                entry.name, entry.crc32, actual
            )));
        }
        Ok(out)
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn find(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// issue #84: nomes de entrada repetidos (ordem de aparição no CD).
    /// Vazio na imensa maioria dos APKs — é o sinal de um container ambíguo.
    pub fn duplicates(&self) -> &[String] {
        &self.duplicates
    }

    /// Offset do central directory já resolvido (ZIP64-aware) — o mesmo valor
    /// usado no parse; garante que a detecção de assinatura e o leitor de
    /// entradas nunca divergem (issue #35).
    pub fn cd_offset(&self) -> u64 {
        self.cd_offset
    }

    /// Bytes brutos do container (contrato público estável: a assinatura
    /// não muda com a fonte paginada). No modo paginado devolve um slice
    /// **vazio**, porque o arquivo nunca é materializado. Quem precisa
    /// distinguir os modos usa [`Zip::whole_bytes`] / [`Zip::is_paged`];
    /// janelas sob demanda saem de [`Zip::read_range`].
    pub fn raw_bytes(&self) -> &[u8] {
        self.whole_bytes().unwrap_or(&[])
    }

    /// Bytes brutos quando o container está inteiro em memória; `None` no
    /// modo paginado (a detecção do bloco de assinatura lê só a janela que
    /// termina no CD).
    pub fn whole_bytes(&self) -> Option<&[u8]> {
        match &self.source {
            ZipSource::Whole(d) => Some(d),
            ZipSource::Paged(_) => None,
        }
    }

    /// Modo paginado? (arquivo acima do teto whole-file, lido sob demanda)
    pub fn is_paged(&self) -> bool {
        matches!(self.source, ZipSource::Paged(_))
    }

    /// Tamanho total do container em bytes (sem materializá-lo).
    pub fn file_len(&self) -> u64 {
        match &self.source {
            ZipSource::Whole(d) => d.len() as u64,
            ZipSource::Paged(p) => p.file_len(),
        }
    }

    /// Janela crua `[off, off+len)` do container. No modo whole copia da
    /// memória; no paginado lê sob demanda pelo cache de páginas.
    pub fn read_range(&self, off: u64, len: usize) -> RdResult<Vec<u8>> {
        match &self.source {
            ZipSource::Whole(d) => {
                let start = usz(off)?;
                let end = start
                    .checked_add(len)
                    .ok_or_else(|| RdError::parse("zip: read_range overflow"))?;
                d.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| {
                    RdError::parse(format!(
                        "zip: read_range [{start}..{end}) fora do arquivo ({})",
                        d.len()
                    ))
                })
            }
            ZipSource::Paged(p) => p.read_range(off, len),
        }
    }

    /// Nomes que casam com um prefixo (ex.: "assets/", "lib/arm64-v8a/").
    pub fn names_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a str> {
        self.entries
            .iter()
            .map(|e| e.name.as_str())
            .filter(move |n| n.starts_with(prefix))
    }
}

#[inline]
fn extra_len_of(data: &[u8], off: usize) -> RdResult<usize> {
    Ok(u16_at(data, off)? as usize)
}

#[inline]
fn comment_len_of(data: &[u8], off: usize) -> RdResult<usize> {
    Ok(u16_at(data, off)? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um ZIP mínimo com 1 entrada STORED — estrutura real, byte a byte.
    fn handcrafted_zip(content: &[u8], name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        let crc = crc32(content);
        // local header
        v.extend(LOCAL_HEADER_SIG.to_le_bytes());
        v.extend(20u16.to_le_bytes()); // version
        v.extend(0u16.to_le_bytes()); // flags
        v.extend(METHOD_STORED.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // time
        v.extend(0u16.to_le_bytes()); // date
        v.extend(crc.to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes()); // csize
        v.extend((content.len() as u32).to_le_bytes()); // usize
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes()); // extra
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(content);
        let local_offset = 0u32;
        // central directory
        let cd_start = v.len() as u32;
        v.extend(CD_ENTRY_SIG.to_le_bytes());
        v.extend(20u16.to_le_bytes()); // version made by
        v.extend(20u16.to_le_bytes()); // version needed
        v.extend(0u16.to_le_bytes()); // flags
        v.extend(METHOD_STORED.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // time/date
        v.extend(0u16.to_le_bytes());
        v.extend(crc.to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes()); // extra
        v.extend(0u16.to_le_bytes()); // comment
        v.extend(0u16.to_le_bytes()); // disk
        v.extend(0u16.to_le_bytes()); // internal attrs
        v.extend(0u32.to_le_bytes()); // external attrs
        v.extend(local_offset.to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        let cd_size = v.len() as u32 - cd_start;
        // EOCD
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // disk
        v.extend(0u16.to_le_bytes()); // cd disk
        v.extend(1u16.to_le_bytes()); // entries this disk
        v.extend(1u16.to_le_bytes()); // total
        v.extend(cd_size.to_le_bytes());
        v.extend(cd_start.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // comment len
        v
    }

    /// Monta um ZIP STORED com N entradas (nomes podem repetir — issue #84).
    fn handcrafted_zip_multi(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut v = Vec::new();
        let mut central = Vec::new();
        for (name, content) in entries {
            let crc = crc32(content);
            let local_offset = v.len() as u32;
            v.extend(LOCAL_HEADER_SIG.to_le_bytes());
            v.extend(20u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(METHOD_STORED.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(crc.to_le_bytes());
            v.extend((content.len() as u32).to_le_bytes());
            v.extend((content.len() as u32).to_le_bytes());
            v.extend((name.len() as u16).to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend_from_slice(name.as_bytes());
            v.extend_from_slice(content);

            central.extend(CD_ENTRY_SIG.to_le_bytes());
            central.extend(20u16.to_le_bytes());
            central.extend(20u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(METHOD_STORED.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(crc.to_le_bytes());
            central.extend((content.len() as u32).to_le_bytes());
            central.extend((content.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u32.to_le_bytes());
            central.extend(local_offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_start = v.len() as u32;
        v.extend(&central);
        let cd_size = v.len() as u32 - cd_start;
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend((entries.len() as u16).to_le_bytes());
        v.extend((entries.len() as u16).to_le_bytes());
        v.extend(cd_size.to_le_bytes());
        v.extend(cd_start.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v
    }

    #[test]
    fn crc32_known_vector() {
        // "123456789" → 0xCBF43926 (IEEE 802.3)
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn parses_handcrafted_stored_zip_and_verifies_crc() {
        let content = b"AndroidManifest.xml-contents-123";
        let bytes = handcrafted_zip(content, "AndroidManifest.xml");
        let zip = Zip::parse(bytes).unwrap();
        assert_eq!(zip.entry_count(), 1);
        assert!(zip.contains("AndroidManifest.xml"));
        let out = zip.read("AndroidManifest.xml").unwrap();
        assert_eq!(out, content);
        assert!(zip.read("missing").is_err());
    }

    #[test]
    fn crc_corruption_is_detected() {
        let mut bytes = handcrafted_zip(b"payload", "f.txt");
        // corrompe um byte do payload (offset 30 + 5 do nome "f.txt")
        let payload_off = 30 + 5;
        bytes[payload_off] ^= 0xFF;
        let zip = Zip::parse(bytes).unwrap();
        let err = zip.read("f.txt").unwrap_err();
        assert_eq!(err.code, "INVALID_FORMAT");
        assert!(err.cause.contains("CRC"));
    }

    #[test]
    fn rejects_non_zip() {
        assert!(Zip::parse(b"definitely not a zip file at all".repeat(4)).is_err());
        assert!(Zip::parse(Vec::new()).is_err());
    }

    /// Variante DEFLATE do handcrafted_zip: payload comprimido + campos do CD
    /// controlados pelo teste (para simular CD mentiroso — bomba de compressão).
    fn handcrafted_zip_deflate(
        compressed: &[u8],
        declared_uncompressed: u32,
        crc: u32,
        name: &str,
    ) -> Vec<u8> {
        let mut v = Vec::new();
        // local header
        v.extend(LOCAL_HEADER_SIG.to_le_bytes());
        v.extend(20u16.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // flags
        v.extend(METHOD_DEFLATE.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // time
        v.extend(0u16.to_le_bytes()); // date
        v.extend(crc.to_le_bytes());
        v.extend((compressed.len() as u32).to_le_bytes()); // csize
        v.extend(declared_uncompressed.to_le_bytes()); // usize (declarado)
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes()); // extra
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(compressed);
        let local_offset = 0u32;
        // central directory
        let cd_start = v.len() as u32;
        v.extend(CD_ENTRY_SIG.to_le_bytes());
        v.extend(20u16.to_le_bytes());
        v.extend(20u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(METHOD_DEFLATE.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(crc.to_le_bytes());
        v.extend((compressed.len() as u32).to_le_bytes());
        v.extend(declared_uncompressed.to_le_bytes());
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(local_offset.to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        let cd_size = v.len() as u32 - cd_start;
        // EOCD
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(cd_size.to_le_bytes());
        v.extend(cd_start.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v
    }

    fn deflate_bytes(content: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(content).unwrap();
        enc.finish().unwrap()
    }

    /// Issue #14 + #30: stream deflate minúsculo (5 MB de zeros → ~5 KB
    /// comprimidos) com CD declarando só 1000 bytes descomprimidos. Sem o
    /// `take()`, o `read_to_end` cresceria o buffer sem teto ANTES da checagem
    /// do CD (OOM kill em craft maior); agora corta no limite → erro tipado.
    #[test]
    fn decompression_bomb_is_typed_error_not_oom() {
        let content = vec![0u8; 5 * 1024 * 1024];
        let compressed = deflate_bytes(&content);
        assert!(compressed.len() < 64 * 1024, "stream deve ser minúsculo");
        let bytes = handcrafted_zip_deflate(&compressed, 1000, 0, "bomb.bin");
        let zip = Zip::parse(bytes).unwrap();
        let err = zip.read("bomb.bin").unwrap_err();
        assert_eq!(err.code, "INVALID_FORMAT");
        assert!(
            err.cause.contains("descompressão limitada"),
            "mensagem: {}",
            err.cause
        );
    }

    /// Issue #30: CD "honesto-grande" — declara 600 MB (> teto global de
    /// 512 MB) para um payload de ~75 MB. Rejeitado ANTES do inflate (sem
    /// alocar um byte da entrada) → erro tipado, não OOM.
    #[test]
    fn inflate_is_capped_by_global_budget_not_attacker_declared_size() {
        // 75 MB de zeros comprimem para ~90 KB; CD declara 600 MB
        let content = vec![0u8; 75 * 1024 * 1024];
        let compressed = deflate_bytes(&content);
        let declared: u32 = 600 * 1024 * 1024; // > teto global de 512 MB
        let bytes = handcrafted_zip_deflate(&compressed, declared, 0, "big.bin");
        let zip = Zip::parse(bytes).unwrap();
        let err = zip.read("big.bin").unwrap_err();
        assert_eq!(err.code, "INVALID_FORMAT");
        assert!(err.cause.contains("teto global"), "mensagem: {}", err.cause);
    }

    /// issue #84: dois central-directory entries com o mesmo nome. O rd NÃO
    /// rejeita o container (um APK ambíguo é justamente o que uma ferramenta
    /// de análise precisa descrever), mas expõe a duplicidade.
    #[test]
    fn duplicate_entry_names_are_reported_not_silently_picked() {
        let bytes = handcrafted_zip_multi(&[
            ("classes.dex", b"dex-primeira-ocorrencia"),
            ("classes2.dex", b"dex-2"),
            ("classes.dex", b"DEX-SEGUNDA-OCORRENCIA"),
        ]);
        let zip = Zip::parse(bytes).expect("zip com duplicata parseia");
        assert_eq!(zip.entry_count(), 3, "3 entradas zip (o container é real)");
        assert_eq!(
            zip.duplicates(),
            ["classes.dex"],
            "só o repetido: {:?}",
            zip.duplicates()
        );
        // find/read seguem na 1ª ocorrência — decisão explícita e documentada
        assert_eq!(
            zip.read("classes.dex").unwrap(),
            b"dex-primeira-ocorrencia".to_vec(),
            "leitura usa a 1ª ocorrência"
        );
        // sem duplicata não há sinal
        let clean = handcrafted_zip_multi(&[("a.dex", b"a"), ("b.dex", b"b")]);
        let zip = Zip::parse(clean).unwrap();
        assert!(zip.duplicates().is_empty(), "APK normal: sem aviso");
    }

    /// Entrada DEFLATE legítima (declaração honesta) continua inflando igual.
    #[test]
    fn legit_deflate_entry_still_inflates() {
        let content = b"legit deflate payload ".repeat(100);
        let compressed = deflate_bytes(&content);
        let bytes =
            handcrafted_zip_deflate(&compressed, content.len() as u32, crc32(&content), "a.bin");
        let zip = Zip::parse(bytes).unwrap();
        assert_eq!(zip.read("a.bin").unwrap(), content);
    }

    // ─── ADR 008 Stage 2: ZipSource (whole × paginado) ──────────────────

    fn write_temp_zip(
        tag: &str,
        write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    ) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("rd-zip-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("t.zip");
        let mut f = std::fs::File::create(&path)?;
        write(&mut f)?;
        drop(f);
        Ok(path)
    }

    /// Paridade total whole × paginado sobre o mesmo container: entradas,
    /// duplicatas (#84), leituras STORED e DEFLATE, janelas cruas e os
    /// sinais de modo (`is_paged`/`raw_bytes`).
    #[test]
    fn open_paged_matches_whole_parse() {
        use std::io::Write as _;
        let big = [7u8; 1000];
        let bytes = handcrafted_zip_multi(&[
            ("classes.dex", b"dex-primeira-ocorrencia"),
            ("assets/x.bin", &big),
            ("classes.dex", b"dex-duplicada"),
        ]);
        let whole = Zip::parse(bytes.clone()).unwrap();
        let path = write_temp_zip("parity", |f| f.write_all(&bytes)).unwrap();
        let paged = Zip::open_paged(&path).unwrap();

        assert!(!whole.is_paged() && whole.whole_bytes().is_some());
        assert!(paged.is_paged() && paged.whole_bytes().is_none());
        // contrato estável de `raw_bytes`: inteiro = bytes do arquivo,
        // paginado = vazio (nunca materializa)
        assert_eq!(whole.raw_bytes().len() as u64, whole.file_len());
        assert!(paged.raw_bytes().is_empty());
        assert_eq!(paged.entries(), whole.entries());
        assert_eq!(paged.duplicates(), whole.duplicates());
        assert_eq!(paged.cd_offset(), whole.cd_offset());
        assert_eq!(paged.file_len(), bytes.len() as u64);
        assert_eq!(whole.file_len(), bytes.len() as u64);
        for name in ["classes.dex", "assets/x.bin"] {
            assert_eq!(
                paged.read(name).unwrap(),
                whole.read(name).unwrap(),
                "{name}"
            );
        }
        for (off, len) in [(0u64, 32usize), ((bytes.len() - 22) as u64, 22)] {
            assert_eq!(
                paged.read_range(off, len).unwrap(),
                whole.read_range(off, len).unwrap(),
                "janela [{off}..{})",
                off + len as u64
            );
        }

        // DEFLATE pelos dois caminhos (mesma política de inflate/CRC).
        let content = b"paged deflate parity ".repeat(50);
        let compressed = deflate_bytes(&content);
        let dbytes =
            handcrafted_zip_deflate(&compressed, content.len() as u32, crc32(&content), "a.bin");
        let dpath = write_temp_zip("parity-deflate", |f| f.write_all(&dbytes)).unwrap();
        assert_eq!(
            Zip::open_paged(&dpath).unwrap().read("a.bin").unwrap(),
            content
        );

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
        std::fs::remove_dir_all(dpath.parent().unwrap()).ok();
    }

    /// Arquivo >512 MiB com buraco esparso entre os payloads e o CD: o modo
    /// paginado abre e lê sem materializar o arquivo (custo de disco ~zero
    /// no teste — o buraco nunca é lido, só os offsets importam).
    #[test]
    fn open_paged_sparse_above_wholefile_cap() {
        use std::io::{Seek, SeekFrom, Write as _};
        let bytes = handcrafted_zip_multi(&[
            ("AndroidManifest.xml", b"<axml/>"),
            ("classes.dex", b"dex\n035\0fake"),
        ]);
        let eocd = bytes.len() - 22;
        let rd32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let cd_start = rd32(&bytes, eocd + 16) as usize;
        let (head, tail) = bytes.split_at(cd_start);
        let mut tail = tail.to_vec();
        const TARGET_CD: u64 = 600 * 1024 * 1024;
        let tail_eocd = tail.len() - 22;
        tail[tail_eocd + 16..tail_eocd + 20].copy_from_slice(&(TARGET_CD as u32).to_le_bytes());

        let path = write_temp_zip("sparse", |f| {
            f.write_all(head)?;
            f.seek(SeekFrom::Start(TARGET_CD))?;
            f.write_all(&tail)
        })
        .unwrap();
        let z = Zip::open_paged(&path).unwrap();
        assert!(z.is_paged());
        assert_eq!(z.file_len(), TARGET_CD + tail.len() as u64);
        assert!(z.file_len() > 512 * 1024 * 1024, "acima do teto whole-file");
        assert_eq!(z.cd_offset(), TARGET_CD);
        assert_eq!(z.entry_count(), 2);
        assert_eq!(z.read("AndroidManifest.xml").unwrap(), b"<axml/>");
        assert_eq!(z.read("classes.dex").unwrap(), b"dex\n035\0fake");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    // ─── #331 (item 1): overflow de offsets ZIP64 no caminho Whole ──────────

    /// Container de 42 bytes: locator ZIP64 com `z64_off = u64::MAX - 1`
    /// antes de um EOCD com sentinel. Na base, `u32_at(data, z64_off)`
    /// avaliava `off + 4` e panica (debug) / wrap (release). Fix = Err tipado.
    #[test]
    fn issue_331_zip64_locator_offset_overflow_is_typed_error() {
        let mut v = Vec::new();
        v.extend(0x0706_4b50u32.to_le_bytes()); // ZIP64_EOCD_LOCATOR_SIG
        v.extend(0u32.to_le_bytes()); // disk
        v.extend((u64::MAX - 1).to_le_bytes()); // z64_off (atacante)
        v.extend(1u32.to_le_bytes()); // total disks
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0xFFFFu16.to_le_bytes()); // entries (sentinel)
        v.extend(0xFFFFu16.to_le_bytes());
        v.extend(0u32.to_le_bytes()); // cd size
        v.extend(0u32.to_le_bytes()); // cd offset
        v.extend(0u16.to_le_bytes());
        assert_eq!(v.len(), 42);
        assert!(Zip::parse(v).is_err(), "overflow deve virar erro tipado");
    }

    /// `local_header_offset = u64::MAX` via extra ZIP64 do CD: o parse
    /// aceita a entrada, mas `read` deve falhar tipado em vez de panicar
    /// em `off + 30 + name_len + extra_len`.
    #[test]
    fn issue_331_zip64_lho_max_read_is_typed_error() {
        let name = "a.txt";
        let content = b"hi";
        let crc = crc32(content);
        let mut v = Vec::new();
        v.extend(LOCAL_HEADER_SIG.to_le_bytes());
        v.extend(45u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(METHOD_STORED.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(crc.to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(content);
        let cd_start = v.len() as u32;
        v.extend(CD_ENTRY_SIG.to_le_bytes());
        v.extend(45u16.to_le_bytes());
        v.extend(45u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(METHOD_STORED.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(crc.to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((content.len() as u32).to_le_bytes());
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(12u16.to_le_bytes()); // extra: id+size+8B
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(0xFFFF_FFFFu32.to_le_bytes()); // lho sentinel
        v.extend_from_slice(name.as_bytes());
        v.extend(0x0001u16.to_le_bytes()); // ZIP64 extra
        v.extend(8u16.to_le_bytes());
        v.extend(u64::MAX.to_le_bytes()); // lho real (atacante)
        let cd_size = v.len() as u32 - cd_start;
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(cd_size.to_le_bytes());
        v.extend(cd_start.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        let z = Zip::parse(v).unwrap();
        assert!(
            z.read(name).is_err(),
            "read com lho MAX deve ser erro tipado"
        );
    }

    /// EOCD declara 2 entradas mas o CD só comporta 1: o laço não pode
    /// caminhar para além de `cd_end`.
    #[test]
    fn issue_331_cd_walk_stays_inside_cd_end() {
        let mut v = handcrafted_zip(b"data", "x.bin");
        let eocd = v.len() - 22;
        v[eocd + 8..eocd + 10].copy_from_slice(&2u16.to_le_bytes());
        v[eocd + 10..eocd + 12].copy_from_slice(&2u16.to_le_bytes());
        let err = format!("{:?}", Zip::parse(v).unwrap_err());
        assert!(
            err.contains("central directory end") || err.contains("signature"),
            "deve rejeitar dentro dos bounds do CD: {err}"
        );
    }
}
