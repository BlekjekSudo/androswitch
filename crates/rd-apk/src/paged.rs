//! ADR 008 Stage 2 prototype — paged ZIP reader for very large APKs.
//!
//! Zero whole-file reads: the container is opened with positional I/O
//! (`FileExt::read_at`, i.e. `pread64`) through a bounded user-space LRU
//! page cache (64 KiB pages, 16 MiB cap). No `unsafe`, no `mmap` — so the
//! bytes this reader can ever pin in RSS are capped by the cache size,
//! independent of the APK size (a 15 GiB APK opens in O(tail + CD) reads).
//!
//! Scope: EOCD + ZIP64 locator/EOCD discovery from the file tail,
//! central-directory parse entry-by-entry through the cache, and
//! on-demand entry extraction (STORED/DEFLATE, CRC-checked, same
//! policy as [`crate::zip::Zip::read_entry`]). Stage 2 integration:
//! [`crate::zip::Zip::open_paged`] wraps this reader as a `ZipSource`,
//! and `Apk::open` routes files above the whole-file cap here.

use crate::error::{RdError, RdResult};
use crate::zip::{crc32, ZipEntry, CD_ENTRY_SIG, EOCD_SIG, ZIP64_EOCD_LOCATOR_SIG, ZIP64_EOCD_SIG};
use std::collections::VecDeque;
use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Mutex;

pub const PAGE_BYTES: usize = 64 * 1024;
pub const CACHE_PAGES: usize = 256; // 256 * 64 KiB = 16 MiB hard cap
pub const MAX_APK_BYTES: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB

/// Issue #245: soma de offset vindo de estruturas não confiáveis
/// (EOCD/ZIP64/CD) — overflow vira erro tipado, nunca panic.
fn ck(off: u64, n: u64) -> RdResult<u64> {
    off.checked_add(n)
        .ok_or_else(|| RdError::parse(format!("paged-zip: offset overflow at {off}+{n}")))
}

struct LruPageCache {
    file: File,
    len: u64,
    // (page_index, bytes) in LRU order, most-recently-used last.
    pages: VecDeque<(u64, Vec<u8>)>,
}

impl LruPageCache {
    fn read_exact_at(&mut self, off: u64, buf: &mut [u8]) -> RdResult<()> {
        let mut done = 0usize;
        while done < buf.len() {
            // Issue #245: offsets vêm de CD/ZIP64 não confiáveis — somas
            // sempre checked, erro tipado em vez de panic de overflow.
            let pos = off
                .checked_add(done as u64)
                .ok_or_else(|| RdError::parse(format!("paged-zip: offset overflow at {off}")))?;
            if pos >= self.len {
                return Err(RdError::parse(format!("paged-zip: read past EOF at {pos}")));
            }
            let page_idx = pos / PAGE_BYTES as u64;
            let page_off = (pos % PAGE_BYTES as u64) as usize;
            let take = (PAGE_BYTES - page_off).min(buf.len() - done);
            if let Some((_, page)) = self
                .pages
                .iter()
                .position(|(i, _)| *i == page_idx)
                .and_then(|p| self.pages.remove(p))
            {
                buf[done..done + take].copy_from_slice(&page[page_off..page_off + take]);
                self.pages.push_back((page_idx, page));
            } else {
                let mut page = vec![0u8; PAGE_BYTES];
                let n = self
                    .file
                    .read_at(&mut page, page_idx * PAGE_BYTES as u64)
                    .map_err(|e| RdError::io(&e, "paged-zip read_at"))?;
                page.truncate(n);
                if page_off + take > page.len() {
                    return Err(RdError::parse("paged-zip: short page at EOF"));
                }
                buf[done..done + take].copy_from_slice(&page[page_off..page_off + take]);
                self.pages.push_back((page_idx, page));
                while self.pages.len() > CACHE_PAGES {
                    self.pages.pop_front();
                }
            }
            done += take;
        }
        Ok(())
    }

    fn read_vec(&mut self, off: u64, len: usize) -> RdResult<Vec<u8>> {
        // Issue #245: validar a janela contra o EOF ANTES de alocar — um
        // CD malicioso declarando gigabytes não pode virar uma alocação
        // arbitrária (abort por OOM) antes de qualquer checagem.
        let end = off
            .checked_add(len as u64)
            .filter(|&end| end <= self.len)
            .ok_or_else(|| {
                RdError::parse(format!(
                    "paged-zip: read [{off}..+{len}) beyond EOF ({})",
                    self.len
                ))
            })?;
        let _ = end;
        let mut buf = vec![0u8; len];
        self.read_exact_at(off, &mut buf)?;
        Ok(buf)
    }

    fn u16_at(&mut self, off: u64) -> RdResult<u16> {
        let mut b = [0u8; 2];
        self.read_exact_at(off, &mut b)?;
        Ok(u16::from_le_bytes(b))
    }
    fn u32_at(&mut self, off: u64) -> RdResult<u32> {
        let mut b = [0u8; 4];
        self.read_exact_at(off, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn u64_at(&mut self, off: u64) -> RdResult<u64> {
        let mut b = [0u8; 8];
        self.read_exact_at(off, &mut b)?;
        Ok(u64::from_le_bytes(b))
    }
}

/// Paged ZIP container: metadata in memory, payloads on disk.
/// O cache vive num `Mutex` (não `RefCell`): `PagedZip` é `Send + Sync`,
/// então `Zip`/`Apk` paginados cruzam threads como o caminho whole-file
/// (a `Arc<PagedZip>` do `ZipSource` e o `Arc<Apk>` do agente exigem).
pub struct PagedZip {
    cache: Mutex<LruPageCache>,
    entries: Vec<ZipEntry>,
    cd_offset: u64,
    file_len: u64,
}

impl PagedZip {
    pub fn open(path: &Path) -> RdResult<PagedZip> {
        let file =
            File::open(path).map_err(|e| RdError::io(&e, format!("opening {}", path.display())))?;
        let len = file
            .metadata()
            .map_err(|e| RdError::io(&e, "paged-zip metadata"))?
            .len();
        if len > MAX_APK_BYTES {
            return Err(RdError::invalid_format(format!(
                "paged-zip: {len} bytes exceeds {MAX_APK_BYTES}-byte ceiling"
            )));
        }
        if len < 22 {
            return Err(RdError::parse(
                "paged-zip: file shorter than EOCD (22 bytes)",
            ));
        }
        let mut cache = LruPageCache {
            file,
            len,
            pages: VecDeque::new(),
        };
        // Tail-only EOCD search: last 64 KiB + EOCD record.
        let tail_len = (len as usize).min(22 + 65_536);
        let tail = cache.read_vec(len - tail_len as u64, tail_len)?;
        let mut eocd_rel = None;
        let mut i = tail_len - 22;
        loop {
            if u32::from_le_bytes([tail[i], tail[i + 1], tail[i + 2], tail[i + 3]]) == EOCD_SIG {
                eocd_rel = Some(i);
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        let eocd_rel =
            eocd_rel.ok_or_else(|| RdError::parse("paged-zip: EOCD not found in tail"))?;
        let eocd = (len - tail_len as u64) + eocd_rel as u64;
        let rd16 = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let rd32 = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let mut entry_count = rd16(&tail, eocd_rel + 10) as u64;
        let mut cd_size = rd32(&tail, eocd_rel + 12) as u64;
        let mut cd_offset = rd32(&tail, eocd_rel + 16) as u64;
        if entry_count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
            let loc = eocd
                .checked_sub(20)
                .ok_or_else(|| RdError::parse("paged-zip: ZIP64 locator cannot fit"))?;
            if cache.u32_at(loc)? != ZIP64_EOCD_LOCATOR_SIG {
                return Err(RdError::parse("paged-zip: ZIP64 sentinel without locator"));
            }
            let z64 = cache.u64_at(ck(loc, 8)?)?;
            if cache.u32_at(z64)? != ZIP64_EOCD_SIG {
                return Err(RdError::parse("paged-zip: ZIP64 EOCD signature mismatch"));
            }
            entry_count = cache.u64_at(ck(z64, 32)?)?;
            cd_size = cache.u64_at(ck(z64, 40)?)?;
            cd_offset = cache.u64_at(ck(z64, 48)?)?;
        }
        let _ = cd_size;
        // Stream the central directory entry-by-entry through the cache.
        // Issue #245: a pré-alocação é limitada pelo que cabe fisicamente
        // no CD (cada entrada ocupa ≥46 B), não só pela contagem declarada.
        let cd_room = (len.saturating_sub(cd_offset) / 46) as usize;
        let mut entries = Vec::with_capacity((entry_count as usize).min(cd_room.min(1_000_000)));
        let mut p = cd_offset;
        for n in 0..entry_count {
            let entry = Self::parse_cd_entry(&mut cache, p).map_err(|e: RdError| {
                e.with_suggestion(format!("paged-zip: parsing CD entry #{n}"))
            })?;
            let stride = 46u64
                + cache.u16_at(ck(p, 28)?)? as u64
                + cache.u16_at(ck(p, 30)?)? as u64
                + cache.u16_at(ck(p, 32)?)? as u64;
            entries.push(entry);
            p = ck(p, stride)?;
        }
        Ok(PagedZip {
            cache: Mutex::new(cache),
            entries,
            cd_offset,
            file_len: len,
        })
    }

    fn parse_cd_entry(cache: &mut LruPageCache, off: u64) -> RdResult<ZipEntry> {
        if cache.u32_at(off)? != CD_ENTRY_SIG {
            return Err(RdError::parse(format!(
                "paged-zip: bad CD entry signature at {off}"
            )));
        }
        let flags = cache.u16_at(ck(off, 8)?)?;
        let method = cache.u16_at(ck(off, 10)?)?;
        let crc = cache.u32_at(ck(off, 16)?)?;
        let mut csize = cache.u32_at(ck(off, 20)?)? as u64;
        let mut usize_ = cache.u32_at(ck(off, 24)?)? as u64;
        let name_len = cache.u16_at(ck(off, 28)?)? as usize;
        let extra_len = cache.u16_at(ck(off, 30)?)? as usize;
        let mut lho = cache.u32_at(ck(off, 42)?)? as u64;
        let name_bytes = cache.read_vec(ck(off, 46)?, name_len)?;
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF || lho == 0xFFFF_FFFF {
            let extra = cache.read_vec(ck(ck(off, 46)?, name_len as u64)?, extra_len)?;
            // Issue #245: id/sz e os campos u64 são lidos com bounds-check
            // contra o extra REAL — um bloco 0x0001 truncado (sz declarado
            // maior que o extra presente) é erro tipado, nunca panic de
            // slice. Mesma política do u64_at do zip.rs.
            let u64_in = |b: &[u8], o: usize| -> RdResult<u64> {
                b.get(o..o + 8)
                    .map(|s| {
                        let mut a = [0u8; 8];
                        a.copy_from_slice(s);
                        u64::from_le_bytes(a)
                    })
                    .ok_or_else(|| {
                        RdError::parse(format!("paged-zip: truncated ZIP64 extra @ {o}"))
                    })
            };
            let mut q = 0usize;
            while q + 4 <= extra.len() {
                let id = u16::from_le_bytes([extra[q], extra[q + 1]]);
                let sz = u16::from_le_bytes([extra[q + 2], extra[q + 3]]) as usize;
                let block_end = (q + 4).checked_add(sz).filter(|&e| e <= extra.len());
                if id == 0x0001 {
                    let block_end = block_end
                        .ok_or_else(|| RdError::parse("paged-zip: truncated ZIP64 extra block"))?;
                    let mut f = q + 4;
                    if usize_ == 0xFFFF_FFFF && f + 8 <= block_end {
                        usize_ = u64_in(&extra, f)?;
                        f += 8;
                    }
                    if csize == 0xFFFF_FFFF && f + 8 <= block_end {
                        csize = u64_in(&extra, f)?;
                        f += 8;
                    }
                    if lho == 0xFFFF_FFFF && f + 8 <= block_end {
                        lho = u64_in(&extra, f)?;
                    }
                    break;
                }
                match block_end {
                    Some(e) => q = e,
                    None => break,
                }
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

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }
    pub fn cd_offset(&self) -> u64 {
        self.cd_offset
    }
    pub fn file_len(&self) -> u64 {
        self.file_len
    }
    /// Bytes the page cache currently pins (hard-capped at 16 MiB).
    pub fn cached_bytes(&self) -> usize {
        self.lock_cache().pages.iter().map(|(_, p)| p.len()).sum()
    }

    /// Takes the page-cache lock without panicking on poison. The cache only
    /// memoizes immutable file pages (payloads are CRC-checked again on
    /// extraction), so a thread that panicked mid-read cannot leave
    /// guest-visible state behind; on poison the pages are dropped and the
    /// cache refills from disk.
    fn lock_cache(&self) -> std::sync::MutexGuard<'_, LruPageCache> {
        self.cache.lock().unwrap_or_else(|poisoned| {
            let mut guard = poisoned.into_inner();
            guard.pages.clear();
            guard
        })
    }
    pub fn find(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Janela crua do container (ex.: bloco de assinatura antes do CD) sem
    /// materializar o arquivo — passa pelo cache de páginas (teto 16 MiB).
    pub fn read_range(&self, off: u64, len: usize) -> RdResult<Vec<u8>> {
        self.lock_cache().read_vec(off, len)
    }

    pub fn read(&self, name: &str) -> RdResult<Vec<u8>> {
        let entry = self
            .find(name)
            .ok_or_else(|| RdError::missing_entry(name))?;
        self.read_entry(entry)
    }

    pub fn read_entry(&self, entry: &ZipEntry) -> RdResult<Vec<u8>> {
        // Issue #245: os orçamentos são aplicados ANTES de materializar
        // qualquer byte — nos dois sentidos (comprimido e declarado),
        // também para STORED, cuja "descompressão" é a própria cópia.
        // try_from: em alvos 32-bit um tamanho >4 GiB é erro tipado,
        // nunca truncamento silencioso.
        const MAX_INFLATE_BUDGET: u64 = 512 * 1024 * 1024;
        if entry.uncompressed_size > MAX_INFLATE_BUDGET
            || entry.compressed_size > MAX_INFLATE_BUDGET
        {
            return Err(RdError::invalid_format(format!(
                "paged-zip: {:?} declara {}/{} bytes (comprimido/expandido, teto {MAX_INFLATE_BUDGET})",
                entry.name, entry.compressed_size, entry.uncompressed_size
            )));
        }
        let compressed_len = usize::try_from(entry.compressed_size).map_err(|_| {
            RdError::invalid_format(format!(
                "paged-zip: {:?} compressed_size não cabe em usize",
                entry.name
            ))
        })?;
        let mut cache = self.lock_cache();
        let off = entry.local_header_offset;
        if cache.u32_at(off)? != 0x0403_4b50 {
            return Err(RdError::parse(format!(
                "paged-zip: bad local header for {:?}",
                entry.name
            )));
        }
        let name_len = cache.u16_at(ck(off, 26)?)? as u64;
        let extra_len = cache.u16_at(ck(off, 28)?)? as u64;
        let data_start = ck(ck(off, 30)?, ck(name_len, extra_len)?)?;
        let compressed = cache.read_vec(data_start, compressed_len)?;
        drop(cache);
        let out: Vec<u8> = match entry.method {
            0 => compressed,
            8 => {
                // O teto (MAX_INFLATE_BUDGET) já foi aplicado na entrada da
                // função, para STORED e DEFLATE igualmente (issue #245).
                let declared = entry.uncompressed_size;
                let mut decoder = flate2::read::DeflateDecoder::new(&compressed[..])
                    .take(declared.saturating_add(1));
                let mut buf = Vec::with_capacity(declared.min(64 * 1024 * 1024) as usize);
                decoder.read_to_end(&mut buf).map_err(|e| {
                    RdError::parse(format!("paged-zip: inflate {:?}: {e}", entry.name))
                })?;
                if buf.len() as u64 > declared {
                    return Err(RdError::invalid_format(format!(
                        "paged-zip: {:?} expandiu além do declarado ({declared})",
                        entry.name
                    )));
                }
                buf
            }
            m => {
                return Err(RdError::parse(format!(
                    "paged-zip: unsupported method {m} for {:?}",
                    entry.name
                )))
            }
        };
        if out.len() as u64 != entry.uncompressed_size {
            return Err(RdError::invalid_format(format!(
                "paged-zip: {:?} inflated to {} bytes, CD says {}",
                entry.name,
                out.len(),
                entry.uncompressed_size
            )));
        }
        if crc32(&out) != entry.crc32 {
            return Err(RdError::invalid_format(format!(
                "paged-zip: CRC mismatch for {:?}",
                entry.name
            )));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn stored_zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in entries {
            let lho = out.len() as u32;
            let crc = crc32(data);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            cd.extend_from_slice(&CD_ENTRY_SIG.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&lho.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        let cd_size = cd.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn paged_open_reads_entries_without_whole_file() {
        let dir = std::env::temp_dir().join(format!("rd-paged-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");
        let bytes = stored_zip_bytes(&[
            ("classes.dex", b"dex\n035\0fake"),
            ("assets/x.bin", &[7u8; 1000]),
        ]);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let z = PagedZip::open(&path).unwrap();
        assert_eq!(z.entries().len(), 2);
        assert_eq!(z.read("classes.dex").unwrap(), b"dex\n035\0fake");
        assert_eq!(z.read("assets/x.bin").unwrap().len(), 1000);
        assert!(z.cached_bytes() <= PAGE_BYTES * CACHE_PAGES);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paged_open_handles_sparse_multigigabyte_container() {
        // 600 MiB sparse STORED entry: open + lookup must not read payload.
        let dir = std::env::temp_dir().join(format!("rd-paged-sparse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.zip");
        let payload = 600u64 * 1024 * 1024;
        let mut bytes = stored_zip_bytes(&[("classes.dex", b"dex\n035\0fake")]);
        // Rebuild manually: local header for big entry + sparse payload + CD/EOCD.
        bytes.clear();
        let name = b"assets/model.bin";
        let mut lh = Vec::new();
        lh.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        lh.extend_from_slice(&20u16.to_le_bytes());
        lh.extend_from_slice(&0u16.to_le_bytes());
        lh.extend_from_slice(&0u16.to_le_bytes());
        lh.extend_from_slice(&0u16.to_le_bytes());
        lh.extend_from_slice(&0u16.to_le_bytes());
        lh.extend_from_slice(&0u32.to_le_bytes()); // crc of zeros, not read in this test
        lh.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        lh.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        lh.extend_from_slice(&(name.len() as u16).to_le_bytes());
        // ZIP64 extra: usize, csize
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x0001u16.to_le_bytes());
        extra.extend_from_slice(&16u16.to_le_bytes());
        extra.extend_from_slice(&payload.to_le_bytes());
        extra.extend_from_slice(&payload.to_le_bytes());
        lh.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        lh.extend_from_slice(name);
        lh.extend_from_slice(&extra);
        let cd_off = lh.len() as u64 + payload;
        let mut cd = Vec::new();
        cd.extend_from_slice(&CD_ENTRY_SIG.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        cd.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(name);
        cd.extend_from_slice(&extra);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&lh).unwrap();
        f.set_len(cd_off).unwrap();
        f.write_at(&cd, cd_off).unwrap();
        let mut eocd = Vec::new();
        eocd.extend_from_slice(&EOCD_SIG.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        eocd.extend_from_slice(&1u16.to_le_bytes());
        eocd.extend_from_slice(&1u16.to_le_bytes());
        eocd.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        eocd.extend_from_slice(&(cd_off as u32).to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        f.write_at(&eocd, cd_off + cd.len() as u64).unwrap();
        drop(f);
        let z = PagedZip::open(&path).unwrap();
        let e = z.find("assets/model.bin").unwrap();
        assert_eq!(e.uncompressed_size, payload);
        assert!(z.file_len() > 600 * 1024 * 1024);
        assert!(
            z.cached_bytes() <= PAGE_BYTES * 4,
            "cache pinned {} bytes",
            z.cached_bytes()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ─── Issue #245: hardening contra ZIP/ZIP64 malformado ───────────
    // Estilo Result com `?` (sem unwrap novo), como nos testes do Stage 2.

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn write_tmp(
        tag: &str,
        bytes: &[u8],
    ) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rd-paged245-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("t.zip");
        let mut f = std::fs::File::create(&path)?;
        f.write_all(bytes)?;
        drop(f);
        Ok(path)
    }

    /// ZIP de 1 entrada STORED "a" com campos do CD sob controle do teste.
    fn adversarial_zip(csize: u32, usize_: u32, lho: u32, extra: &[u8]) -> Vec<u8> {
        let name = b"a";
        let mut out = Vec::new();
        // local header mínimo (o payload não é lido nestes testes)
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 22]);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        let cd_off = out.len() as u32;
        out.extend_from_slice(&CD_ENTRY_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method STORED
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // crc
        out.extend_from_slice(&csize.to_le_bytes());
        out.extend_from_slice(&usize_.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out.extend_from_slice(&0u16.to_le_bytes()); // disk start
        out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        out.extend_from_slice(&lho.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(extra);
        let cd_size = (out.len() as u32) - cd_off;
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// Bug 1: extra ZIP64 truncado (sz=24 declarado, 2 bytes presentes)
    /// com usize sentinela — antes: panic de slice; agora: erro tipado.
    #[test]
    fn truncated_zip64_extra_is_typed_error_not_panic() -> TestResult {
        let extra: &[u8] = &[0x01, 0x00, 24, 0x00, 0xAA, 0xBB];
        let bytes = adversarial_zip(3, 0xFFFF_FFFF, 0, extra);
        let path = write_tmp("trunc64", &bytes)?;
        assert!(
            PagedZip::open(&path).is_err(),
            "extra ZIP64 truncado recusa com erro (sem panic)"
        );
        Ok(())
    }

    /// Bug 2: tamanhos declarados acima do teto — recusa ANTES de
    /// alocar/ler (arquivo minúsculo, declaração de 600 MiB).
    #[test]
    fn declared_sizes_above_budget_refused_before_reading() -> TestResult {
        let big = 600 * 1024 * 1024u32;
        let bytes = adversarial_zip(big, big, 0, &[]);
        let path = write_tmp("toobig", &bytes)?;
        let z = PagedZip::open(&path)?;
        assert!(
            z.read("a").is_err(),
            "leitura acima do orçamento recusa com erro tipado"
        );
        Ok(())
    }

    /// Bug 3: local_header_offset via ZIP64 colado em u64::MAX — as
    /// somas de offset são checked: erro tipado, nunca panic de
    /// overflow (o build de teste é debug, onde overflow panica).
    #[test]
    fn near_max_local_header_offset_is_typed_error_not_panic() -> TestResult {
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x0001u16.to_le_bytes());
        extra.extend_from_slice(&8u16.to_le_bytes());
        extra.extend_from_slice(&0xFFFF_FFFF_FFFF_FFF8u64.to_le_bytes());
        let bytes = adversarial_zip(3, 3, 0xFFFF_FFFF, &extra);
        let path = write_tmp("maxlho", &bytes)?;
        let z = PagedZip::open(&path)?;
        assert!(z.read("a").is_err(), "lho ~u64::MAX recusa sem panic");
        Ok(())
    }
}
