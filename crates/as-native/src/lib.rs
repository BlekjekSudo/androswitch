//! General ARM64 ELF loading. Memory ownership/execution belongs to a backend;
//! this crate never dereferences guest addresses or selects behavior by APK.
#![no_std]
extern crate alloc;
pub mod jni_refs;
#[cfg(feature = "oracle")]
pub mod oracle;
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use rd_ndk::elf::{Abi, Elf};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: String,
    pub operation: String,
    pub library: String,
    pub required: String,
    pub cause: String,
}
pub type Result<T> = core::result::Result<T, Error>;
pub fn error(code: &str, operation: &str, library: &str, required: impl Into<String>) -> Error {
    let required = required.into();
    Error {
        code: code.into(),
        operation: operation.into(),
        library: library.into(),
        cause: format!("{library}: {operation} requires {required}"),
        required,
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Allocation {
    pub address: u64,
    pub token: u64,
    pub length: usize,
}
pub trait NativeBackend {
    fn system_library(&self, name: &str) -> bool {
        matches!(
            name,
            "libc.so" | "libm.so" | "libdl.so" | "liblog.so" | "libandroid.so"
        )
    }
    fn allocate(&mut self, length: usize) -> Result<Allocation>;
    fn write(&mut self, memory: Allocation, offset: usize, bytes: &[u8]) -> Result<()>;
    fn finalize(&mut self, memory: Allocation, executable_pages: &[u8]) -> Result<()>;
    fn release(&mut self, memory: Allocation);
    fn import(&mut self, library: &str, symbol: &str) -> Result<Option<u64>>;
    fn tls(&mut self, module: usize, template: &[u8], size: usize, alignment: usize)
        -> Result<u64>;
    fn tls_descriptor(&mut self, offset: u64) -> Result<(u64, u64)>;
}
pub struct Module {
    pub name: String,
    pub image: Vec<u8>,
    pub elf: Elf,
    pub memory: Allocation,
    pub constructors: Vec<u64>,
    pub tls_offset: Option<u64>,
    pub initialized: bool,
}
pub struct Loader<B: NativeBackend> {
    pub backend: B,
    pub modules: Vec<Module>,
    sources: BTreeMap<String, Vec<u8>>,
    loading: Vec<String>,
    poisoned: bool,
}
impl<B: NativeBackend> Loader<B> {
    pub fn new(backend: B, sources: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            backend,
            modules: Vec::new(),
            sources,
            loading: Vec::new(),
            poisoned: false,
        }
    }
    pub fn symbol(&self, name: &str) -> Option<u64> {
        self.modules
            .iter()
            .find_map(|m| m.elf.symbol(name).map(|offset| m.memory.address + offset))
    }
    pub fn module_symbol(&self, index: usize, name: &str) -> Option<u64> {
        let m = self.modules.get(index)?;
        m.elf.symbol(name).map(|v| m.memory.address + v)
    }
    pub fn load(&mut self, name: &str) -> Result<usize> {
        if self.poisoned {
            return Err(error(
                "NATIVE_SESSION_POISONED",
                "load",
                name,
                "new runner process after failed mapping",
            ));
        }
        if let Some(i) = self.modules.iter().position(|m| m.name == name) {
            return Ok(i);
        }
        if self.loading.iter().any(|n| n == name) {
            return Err(error(
                "DEPENDENCY_CYCLE",
                "load",
                name,
                "acyclic dependency graph",
            ));
        }
        let bytes = self
            .sources
            .get(name)
            .ok_or_else(|| error("LIBRARY_NOT_FOUND", "load", name, "APK lib/arm64-v8a entry"))?;
        let elf = Elf::parse(bytes).map_err(|e| error(&e.code, "parse", name, e.cause))?;
        if elf.abi != Abi::Arm64 {
            return Err(error("UNSUPPORTED_NATIVE_ABI", "load", name, "ARM64"));
        }
        self.loading.push(name.to_string());
        let result = (|| {
            for dependency in &elf.needed {
                if self.sources.contains_key(dependency) {
                    self.load(dependency)?;
                } else if !self.backend.system_library(dependency) {
                    return Err(error("LIBRARY_NOT_FOUND", "dependency", name, dependency));
                }
            }
            let length = (elf.memory_size as usize + 4095) & !4095;
            let memory = self.backend.allocate(length)?;
            let mut module = Module {
                name: name.to_string(),
                image: vec![0; length],
                elf,
                memory,
                constructors: Vec::new(),
                tls_offset: None,
                initialized: false,
            };
            let result = self.relocate(&mut module);
            if let Err(e) = result {
                self.backend.release(memory);
                self.poisoned = e.operation == "finalize";
                return Err(e);
            }
            let index = self.modules.len();
            self.modules.push(module);
            Ok(index)
        })();
        self.loading.pop();
        result
    }
    fn relocate(&mut self, m: &mut Module) -> Result<()> {
        let mut pages = vec![0; m.image.len() / 4096];
        for s in &m.elf.segments {
            m.image[s.address as usize..s.address as usize + s.bytes.len()]
                .copy_from_slice(&s.bytes);
            if s.flags & 1 != 0 {
                // Linux SVC instructions cannot be allowed to become Horizon SVCs.
                for (i, chunk) in s.bytes.chunks_exact(4).enumerate() {
                    let instruction = u32::from_le_bytes(chunk.try_into().unwrap());
                    if instruction & 0xffe0001f == 0xd4000001 {
                        return Err(error(
                            "LINUX_SYSCALL_UNSUPPORTED",
                            "validate_code",
                            &m.name,
                            format!("svc instruction at {:#x}", s.address + i as u64 * 4),
                        ));
                    }
                }
                let first = s.address as usize / 4096;
                let end = (s.address as usize + s.memory_size as usize).div_ceil(4096);
                pages[first..end].fill(1);
            }
        }
        if let Some(tls) = &m.elf.tls {
            m.tls_offset = Some(self.backend.tls(
                self.modules.len() + 1,
                &tls.bytes,
                tls.memory_size as usize,
                tls.alignment as usize,
            )?);
        }
        for relocation in &m.elf.relocations {
            if relocation.kind == 0 {
                continue;
            }
            let at = relocation.offset as usize;
            let old = m.image.get(at..at + 8).ok_or_else(|| {
                error(
                    "INVALID_RELOCATION",
                    "relocate",
                    &m.name,
                    format!("offset {at:#x}"),
                )
            })?;
            let addend = relocation
                .addend
                .unwrap_or(i64::from_le_bytes(old.try_into().unwrap()));
            let symbol = m.elf.symbols.get(relocation.symbol);
            let address = if relocation.symbol == 0 {
                0
            } else {
                let s = symbol.ok_or_else(|| {
                    error(
                        "INVALID_RELOCATION",
                        "symbol",
                        &m.name,
                        format!("index {}", relocation.symbol),
                    )
                })?;
                if s.defined {
                    m.memory.address + s.value
                } else if let Some(v) = self.symbol(&s.name) {
                    v
                } else if let Some(v) = self.backend.import(&m.name, &s.name)? {
                    v
                } else if s.weak {
                    0
                } else {
                    return Err(error("SYMBOL_NOT_FOUND", "resolve", &m.name, &s.name));
                }
            };
            let value = match relocation.kind {
                1027 => m.memory.address.wrapping_add_signed(addend),
                257 | 1025 | 1026 => address.wrapping_add_signed(addend),
                1028 => self.modules.len() as u64 + 1,
                1029 => symbol
                    .map(|s| s.value)
                    .unwrap_or(0)
                    .wrapping_add_signed(addend),
                1030 => m
                    .tls_offset
                    .ok_or_else(|| error("TLS_UNAVAILABLE", "relocate", &m.name, "PT_TLS"))?
                    .wrapping_add(symbol.map(|s| s.value).unwrap_or(0))
                    .wrapping_add_signed(addend),
                1031 => {
                    let offset = m
                        .tls_offset
                        .ok_or_else(|| error("TLS_UNAVAILABLE", "relocate", &m.name, "PT_TLS"))?
                        .wrapping_add(symbol.map(|s| s.value).unwrap_or(0))
                        .wrapping_add_signed(addend);
                    let (function, argument) = self.backend.tls_descriptor(offset)?;
                    put(&mut m.image, at + 8, argument, &m.name)?;
                    function
                }
                kind => {
                    return Err(error(
                        "UNSUPPORTED_RELOCATION",
                        "relocate",
                        &m.name,
                        format!("R_AARCH64 type {kind} at {at:#x}"),
                    ))
                }
            };
            put(&mut m.image, at, value, &m.name)?;
        }
        if let Some(init) = m.elf.init {
            m.constructors.push(m.memory.address + init);
        }
        if let Some((at, count)) = m.elf.init_array {
            for i in 0..count {
                let at = at as usize + i * 8;
                let bytes = m.image.get(at..at + 8).ok_or_else(|| {
                    error("INVALID_ELF", "init_array", &m.name, "mapped init array")
                })?;
                let function = u64::from_le_bytes(bytes.try_into().unwrap());
                if function != 0 && function != u64::MAX {
                    m.constructors.push(function);
                }
            }
        }
        self.backend.write(m.memory, 0, &m.image)?;
        self.backend.finalize(m.memory, &pages)
    }
}
fn put(bytes: &mut [u8], offset: usize, value: u64, name: &str) -> Result<()> {
    let slot = bytes.get_mut(offset..offset + 8).ok_or_else(|| {
        error(
            "INVALID_RELOCATION",
            "write",
            name,
            format!("offset {offset:#x}"),
        )
    })?;
    slot.copy_from_slice(&value.to_le_bytes());
    Ok(())
}
impl<B: NativeBackend> Drop for Loader<B> {
    fn drop(&mut self) {
        for module in self.modules.iter().rev() {
            self.backend.release(module.memory);
        }
    }
}

/// AAPCS64 independent integer and SIMD argument banks plus bounded spill area.
/// Scalar JNI values do not use homogeneous aggregate calling conventions.
#[repr(C)]
#[derive(Default)]
pub struct CallFrame {
    pub integer: [u64; 8],
    pub float: [u64; 8],
    pub stack: [u64; 32],
    pub result_integer: [u64; 2],
    pub result_float: [u64; 2],
    pub function: u64,
    pub guest_tls: u64,
}
impl CallFrame {
    pub fn arguments(&mut self, types: &[String], values: &[u64]) -> Result<()> {
        if types.len() != values.len() {
            return Err(error(
                "JNI_SIGNATURE",
                "arguments",
                "JNI",
                "matching descriptor/value count",
            ));
        }
        let (mut integer, mut float, mut stack) = (0, 0, 0);
        for (ty, value) in types.iter().zip(values) {
            let floating = ty == "F" || ty == "D";
            let bank = if floating {
                &mut self.float
            } else {
                &mut self.integer
            };
            let index = if floating { &mut float } else { &mut integer };
            if *index < 8 {
                bank[*index] = *value;
                *index += 1;
            } else {
                if stack == self.stack.len() {
                    return Err(error(
                        "NATIVE_ARGUMENT_LIMIT",
                        "arguments",
                        "JNI",
                        "<=32 stack arguments",
                    ));
                }
                self.stack[stack] = *value;
                stack += 1;
            }
        }
        Ok(())
    }
}
