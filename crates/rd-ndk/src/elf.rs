//! Little-endian ET_DYN ELF32/ELF64, including stripped dynamic metadata.
use crate::{RdError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const MAX_IMAGE: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Abi {
    X86_64,
    Arm64,
    Arm,
}
impl Abi {
    pub fn android_name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Arm64 => "arm64-v8a",
            Self::Arm => "armeabi-v7a",
        }
    }
    pub fn pointer_size(self) -> usize {
        if self == Self::Arm {
            4
        } else {
            8
        }
    }
}
#[derive(Debug, Clone)]
pub struct Segment {
    pub address: u64,
    pub memory_size: u64,
    pub flags: u32,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub defined: bool,
    pub weak: bool,
    /// STT_* (1 = object, 2 = function).
    pub kind: u8,
}
#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: u64,
    pub kind: u32,
    pub symbol: usize,
    pub addend: Option<i64>,
}
#[derive(Debug, Clone)]
pub struct Elf {
    pub abi: Abi,
    pub segments: Vec<Segment>,
    pub symbols: Vec<Symbol>,
    pub relocations: Vec<Relocation>,
    pub needed: Vec<String>,
    pub init: Option<u64>,
    pub init_array: Option<(u64, usize)>,
    pub memory_size: u64,
}
fn bad(s: impl Into<String>) -> RdError {
    RdError::new("INVALID_ELF", s)
}
fn bytes(d: &[u8], off: u64, n: u64) -> Result<&[u8]> {
    let end = off.checked_add(n).ok_or_else(|| bad("offset overflow"))?;
    if end > d.len() as u64 {
        return Err(bad(format!("truncated range {off:#x}..{end:#x}")));
    }
    Ok(&d[off as usize..end as usize])
}
fn num(d: &[u8], off: u64, n: usize) -> Result<u64> {
    let b = bytes(d, off, n as u64)?;
    Ok(b.iter()
        .enumerate()
        .fold(0u64, |v, (i, b)| v | (u64::from(*b) << (i * 8))))
}
fn cstr(d: &[u8], off: u64) -> Result<String> {
    let b = bytes(d, off, d.len() as u64 - off.min(d.len() as u64))?;
    let end = b
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| bad("unterminated ELF string"))?;
    String::from_utf8(b[..end].to_vec()).map_err(|_| bad("ELF symbol not UTF-8"))
}
impl Elf {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.len() as u64 > MAX_IMAGE {
            return Err(bad("ELF exceeds 128 MiB input budget"));
        }
        if bytes(d, 0, 16)?[..4] != *b"\x7fELF" {
            return Err(bad("bad ELF magic"));
        }
        if d[5] != 1 || d[6] != 1 {
            return Err(bad("only ELF version 1 little-endian supported"));
        }
        let wide = match d[4] {
            1 => false,
            2 => true,
            _ => return Err(bad("invalid ELF class")),
        };
        if num(d, 16, 2)? != 3 {
            return Err(bad("native libraries must be ET_DYN"));
        }
        let abi = match (num(d, 18, 2)?, wide) {
            (62, true) => Abi::X86_64,
            (183, true) => Abi::Arm64,
            (40, false) => Abi::Arm,
            _ => {
                return Err(RdError::new(
                    "UNSUPPORTED_ABI",
                    "expected x86_64, AArch64 or ARM ELF",
                ))
            }
        };
        let word = abi.pointer_size();
        let phoff = num(d, if wide { 32 } else { 28 }, word)?;
        let phsize = num(d, if wide { 54 } else { 42 }, 2)?;
        let phcount = num(d, if wide { 56 } else { 44 }, 2)?;
        if phcount == 0 || phcount > 4096 || phsize < if wide { 56 } else { 32 } {
            return Err(bad("invalid program header table"));
        }
        bytes(
            d,
            phoff,
            phsize
                .checked_mul(phcount)
                .ok_or_else(|| bad("phdr overflow"))?,
        )?;
        let mut segments = Vec::new();
        let mut dynamic = None;
        let mut end = 0;
        for i in 0..phcount {
            let p = phoff + i * phsize;
            let ty = num(d, p, 4)?;
            let (flags, off, va, filesz, memsz) = if wide {
                (
                    num(d, p + 4, 4)? as u32,
                    num(d, p + 8, 8)?,
                    num(d, p + 16, 8)?,
                    num(d, p + 32, 8)?,
                    num(d, p + 40, 8)?,
                )
            } else {
                (
                    num(d, p + 24, 4)? as u32,
                    num(d, p + 4, 4)?,
                    num(d, p + 8, 4)?,
                    num(d, p + 16, 4)?,
                    num(d, p + 20, 4)?,
                )
            };
            if ty == 1 {
                if filesz > memsz {
                    return Err(bad("PT_LOAD filesz exceeds memsz"));
                }
                let segend = va
                    .checked_add(memsz)
                    .ok_or_else(|| bad("segment overflow"))?;
                if segend > MAX_IMAGE {
                    return Err(bad("PT_LOAD exceeds 128 MiB address budget"));
                }
                end = end.max(segend);
                segments.push(Segment {
                    address: va,
                    memory_size: memsz,
                    flags,
                    bytes: bytes(d, off, filesz)?.to_vec(),
                });
            } else if ty == 2 {
                dynamic = Some((off, filesz));
            }
        }
        if segments.is_empty() {
            return Err(bad("no loadable segments"));
        }
        let vaoff = |address: u64, len: u64| -> Result<u64> {
            for i in 0..phcount {
                let p = phoff + i * phsize;
                if num(d, p, 4)? != 1 {
                    continue;
                }
                let off = num(d, p + if wide { 8 } else { 4 }, word)?;
                let va = num(d, p + if wide { 16 } else { 8 }, word)?;
                let size = num(d, p + if wide { 32 } else { 16 }, word)?;
                if address >= va
                    && address
                        .checked_sub(va)
                        .and_then(|x| x.checked_add(len))
                        .map(|x| x <= size)
                        .unwrap_or(false)
                {
                    return Ok(off + address - va);
                }
            }
            Err(bad(format!(
                "dynamic address {address:#x} is not file-backed"
            )))
        };
        let mut tags = BTreeMap::new();
        let mut need = Vec::new();
        if let Some((off, size)) = dynamic {
            let entry = (word * 2) as u64;
            if size / entry > MAX_ENTRIES as u64 {
                return Err(bad("too many dynamic entries"));
            }
            bytes(d, off, size)?;
            let mut terminated = false;
            for i in 0..size / entry {
                let tag = num(d, off + i * entry, word)?;
                let val = num(d, off + i * entry + word as u64, word)?;
                if tag == 0 {
                    terminated = true;
                    break;
                }
                if tag == 1 {
                    need.push(val);
                } else {
                    tags.insert(tag, val);
                }
            }
            if !terminated {
                return Err(bad("unterminated PT_DYNAMIC"));
            }
        }
        for tag in [0x6000000f, 0x60000010, 0x60000011, 0x60000012] {
            if tags.get(&tag).copied().unwrap_or(0) != 0 {
                return Err(RdError::new(
                    "UNSUPPORTED_RELOCATION",
                    "Android packed relocations require APS2 decoding",
                ));
            }
        }
        let get = |tag: u64| tags.get(&tag).copied().unwrap_or(0);
        let strings = if get(5) != 0 {
            bytes(d, vaoff(get(5), get(10))?, get(10))?
        } else {
            &[]
        };
        let mut needed = Vec::new();
        for n in need {
            needed.push(cstr(strings, n)?);
        }
        let mut relocations = Vec::new();
        let mut add_rels = |addr: u64, size: u64, rela: bool, entsize: u64| -> Result<()> {
            if size == 0 {
                return Ok(());
            }
            let expected = (word * if rela { 3 } else { 2 }) as u64;
            if entsize != expected || size % entsize != 0 || size / entsize > MAX_ENTRIES as u64 {
                return Err(bad("invalid relocation table size"));
            }
            let off = vaoff(addr, size)?;
            for i in 0..size / entsize {
                let o = off + i * entsize;
                let info = num(d, o + word as u64, word)?;
                relocations.push(Relocation {
                    offset: num(d, o, word)?,
                    kind: if wide {
                        info as u32
                    } else {
                        (info & 255) as u32
                    },
                    symbol: if wide {
                        (info >> 32) as usize
                    } else {
                        (info >> 8) as usize
                    },
                    addend: if rela {
                        Some(if wide {
                            num(d, o + 16, 8)? as i64
                        } else {
                            num(d, o + 8, 4)? as i32 as i64
                        })
                    } else {
                        None
                    },
                });
            }
            Ok(())
        };
        add_rels(
            get(7),
            get(8),
            true,
            if get(9) == 0 {
                (word * 3) as u64
            } else {
                get(9)
            },
        )?;
        add_rels(
            get(17),
            get(18),
            false,
            if get(19) == 0 {
                (word * 2) as u64
            } else {
                get(19)
            },
        )?;
        if get(2) != 0 {
            let rela = get(20) == 7;
            add_rels(
                get(23),
                get(2),
                rela,
                (word * if rela { 3 } else { 2 }) as u64,
            )?;
        }
        // Standard DT_RELR: address entries followed by bitmap words.
        if get(35) != 0 {
            if get(37) != word as u64 || get(35) % (word as u64) != 0 {
                return Err(bad("invalid RELR table"));
            }
            let off = vaoff(get(36), get(35))?;
            let mut address = 0u64;
            for i in 0..get(35) / word as u64 {
                let entry = num(d, off + i * word as u64, word)?;
                if entry & 1 == 0 {
                    address = entry;
                    relocations.push(Relocation {
                        offset: address,
                        kind: relative_kind(abi),
                        symbol: 0,
                        addend: None,
                    });
                    address = address
                        .checked_add(word as u64)
                        .ok_or_else(|| bad("RELR overflow"))?;
                } else {
                    for bit in 1..word * 8 {
                        if entry & (1u64 << bit) != 0 {
                            relocations.push(Relocation {
                                offset: address + (bit - 1) as u64 * word as u64,
                                kind: relative_kind(abi),
                                symbol: 0,
                                addend: None,
                            });
                        }
                    }
                    address = address
                        .checked_add((word * 8 - 1) as u64 * word as u64)
                        .ok_or_else(|| bad("RELR overflow"))?;
                }
                if relocations.len() > MAX_ENTRIES {
                    return Err(bad("too many relocations"));
                }
            }
        }
        let mut count = relocations.iter().map(|r| r.symbol + 1).max().unwrap_or(0);
        if get(4) != 0 {
            count = count.max(num(d, vaoff(get(4), 8)? + 4, 4)? as usize);
        }
        if get(0x6ffffef5) != 0 {
            let h = vaoff(get(0x6ffffef5), 16)?;
            let nb = num(d, h, 4)?;
            let first = num(d, h + 4, 4)?;
            let bloom = num(d, h + 8, 4)?;
            if nb > MAX_ENTRIES as u64 || bloom > MAX_ENTRIES as u64 {
                return Err(bad("GNU hash too large"));
            }
            let buckets = h + 16 + bloom * word as u64;
            let mut last = first;
            for i in 0..nb {
                last = last.max(num(d, buckets + i * 4, 4)?);
            }
            if last >= first && last != 0 {
                let chains = buckets + nb * 4;
                loop {
                    if last >= MAX_ENTRIES as u64 {
                        return Err(bad("GNU hash chain too long"));
                    }
                    let v = num(d, chains + (last - first) * 4, 4)?;
                    last += 1;
                    if v & 1 != 0 {
                        break;
                    }
                }
                count = count.max(last as usize);
            }
        }
        // Section headers are optional, but can give a larger dynsym count.
        let shoff = num(d, if wide { 40 } else { 32 }, word)?;
        let shsize = num(d, if wide { 58 } else { 46 }, 2)?;
        let shcount = num(d, if wide { 60 } else { 48 }, 2)?;
        if shcount > 0 {
            if shsize < if wide { 64 } else { 40 } {
                return Err(bad("invalid section header size"));
            }
            bytes(d, shoff, shcount * shsize)?;
            for i in 0..shcount {
                let p = shoff + i * shsize;
                if num(d, p + 4, 4)? == 11 {
                    let size = num(d, p + if wide { 32 } else { 20 }, word)?;
                    let ent = num(d, p + if wide { 56 } else { 36 }, word)?;
                    if ent == 0 || size % ent != 0 {
                        return Err(bad("invalid dynsym section"));
                    }
                    count = count.max((size / ent) as usize);
                }
            }
        }
        if count > MAX_ENTRIES {
            return Err(bad("symbol count exceeds budget"));
        }
        let mut symbols = Vec::new();
        if count > 0 {
            let entry = if wide { 24 } else { 16 };
            if get(11) != entry {
                return Err(bad("invalid DT_SYMENT"));
            }
            let off = vaoff(get(6), count as u64 * entry)?;
            for i in 0..count {
                let p = off + i as u64 * entry;
                let name = cstr(strings, num(d, p, 4)?)?;
                let info = num(d, p + if wide { 4 } else { 12 }, 1)?;
                let defined = num(d, p + if wide { 6 } else { 14 }, 2)? != 0;
                symbols.push(Symbol {
                    name,
                    value: num(d, p + if wide { 8 } else { 4 }, word)?,
                    size: num(d, p + if wide { 16 } else { 8 }, word)?,
                    defined,
                    weak: info >> 4 == 2,
                    kind: (info & 15) as u8,
                });
            }
        }
        for r in &relocations {
            let valid = segments.iter().any(|s| {
                r.offset >= s.address
                    && r.offset
                        .checked_add(word as u64)
                        .map(|e| e <= s.address + s.memory_size)
                        .unwrap_or(false)
            });
            if !valid || r.symbol >= symbols.len() && r.symbol != 0 {
                return Err(bad("relocation target/symbol outside image"));
            }
        }
        let init_array = if get(25) != 0 {
            if get(27) % (word as u64) != 0 || get(27) > MAX_IMAGE {
                return Err(bad("invalid init_array"));
            }
            Some((get(25), (get(27) / word as u64) as usize))
        } else {
            None
        };
        Ok(Self {
            abi,
            segments,
            symbols,
            relocations,
            needed,
            init: if get(12) == 0 { None } else { Some(get(12)) },
            init_array,
            memory_size: (end + 4095) & !4095,
        })
    }
    pub fn symbol(&self, name: &str) -> Option<u64> {
        self.symbols
            .iter()
            .find(|s| s.defined && s.name == name)
            .map(|s| s.value)
    }
}
pub fn relative_kind(abi: Abi) -> u32 {
    match abi {
        Abi::X86_64 => 8,
        Abi::Arm64 => 1027,
        Abi::Arm => 23,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_and_overflow_without_panics() {
        for n in 0..256 {
            let mut b = vec![0xff; n];
            if n >= 16 {
                b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
            }
            assert!(Elf::parse(&b).is_err());
        }
    }
    #[test]
    fn fuzz_prefixes() {
        let mut x = 7u64;
        for n in 0..20000 {
            let mut b = vec![0; n % 512];
            for v in &mut b {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *v = x as u8;
            }
            let _ = Elf::parse(&b);
        }
    }
}
