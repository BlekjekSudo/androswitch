//! Isolated native runtime; JSON IPC is private, bounded and watchdog protected.
use crate::elf::{relative_kind, Abi, Elf};
use crate::{RdError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
pub const ENV: u64 = 0x50000000;
pub const JAVA_VM: u64 = ENV + 8;
pub const TRAPS: u64 = 0x70000000;
const ARENA: u64 = 0x40000000;
const ARENA_SIZE: u64 = 64 * 1024 * 1024;
const WORKER: &str = include_str!("../worker/native_worker.py");
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Argument {
    pub kind: char,
    pub bits: u64,
}
impl Argument {
    pub fn word(bits: u64) -> Self {
        Self { kind: 'J', bits }
    }
    pub fn pointer(bits: u64) -> Self {
        Self { kind: 'L', bits }
    }
}
#[derive(Debug, Clone)]
pub struct Trap {
    pub slot: usize,
    pub args: Vec<u64>,
    pub fp: Vec<u64>,
    pub sp: u64,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Return {
    pub bits: u64,
    pub fp_bits: Option<u64>,
}
#[derive(Debug, Clone)]
struct Library {
    base: u64,
    elf: Elf,
}
pub struct Runtime {
    child: Child,
    input: ChildStdin,
    output: Receiver<std::result::Result<Value, String>>,
    pub abi: Abi,
    libraries: BTreeMap<String, Library>,
    imports: BTreeMap<String, usize>,
    import_names: BTreeMap<usize, String>,
    next_base: u64,
    arena_used: u64,
    pub instructions: u64,
    pub logs: Vec<String>,
    pub allocations: u64,
    sizes: BTreeMap<u64, usize>,
    data_imports: BTreeMap<String, u64>,
    errno: u64,
    tls_keys: BTreeMap<u64, u64>,
    /// Imports that were lazily bound but called without an implementation.
    pub missing_imports: BTreeSet<String>,
}
impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRuntime")
            .field("abi", &self.abi)
            .field("libraries", &self.libraries.keys())
            .field("arena_used", &self.arena_used)
            .finish()
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn ioerr(e: impl std::fmt::Display) -> RdError {
    RdError::new("NATIVE_WORKER", e.to_string())
}
fn hex(b: &[u8]) -> String {
    const H: &[u8] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for v in b {
        s.push(H[(v >> 4) as usize] as char);
        s.push(H[(v & 15) as usize] as char);
    }
    s
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 || s.len() > 32 * 1024 * 1024 {
        return Err(ioerr("invalid worker hex length"));
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|b| {
            let c = |v: u8| -> Result<u8> {
                match v {
                    b'0'..=b'9' => Ok(v - b'0'),
                    b'a'..=b'f' => Ok(v - b'a' + 10),
                    _ => Err(ioerr("invalid hex")),
                }
            };
            Ok((c(b[0])? << 4) | c(b[1])?)
        })
        .collect()
}
impl Runtime {
    pub fn new(abi: Abi) -> Result<Self> {
        let python = std::env::var_os("RD_NATIVE_PYTHON").unwrap_or_else(|| "python3".into());
        let mut child = Command::new(python)
            .arg("-u")
            .arg("-c")
            .arg(WORKER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(ioerr)?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| ioerr("worker stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ioerr("worker stdout unavailable"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut b = Vec::new();
                let mut eof = false;
                let result = (|| {
                    while b.len() <= 64 * 1024 * 1024 {
                        let chunk = r.fill_buf().map_err(|e| e.to_string())?;
                        if chunk.is_empty() {
                            eof = true;
                            return Err("native worker exited (install unicorn==2.1.4)".into());
                        }
                        let n = chunk
                            .iter()
                            .position(|c| *c == b'\n')
                            .map(|i| i + 1)
                            .unwrap_or(chunk.len());
                        b.extend_from_slice(&chunk[..n]);
                        r.consume(n);
                        if b.last() == Some(&b'\n') {
                            return serde_json::from_slice(&b).map_err(|e| e.to_string());
                        }
                    }
                    Err("worker response exceeds limit".into())
                })();
                if tx.send(result).is_err() || eof {
                    break;
                }
            }
        });
        let mut r = Self {
            child,
            input,
            output: rx,
            abi,
            libraries: BTreeMap::new(),
            imports: BTreeMap::new(),
            import_names: BTreeMap::new(),
            next_base: 0x10000000,
            arena_used: 0,
            instructions: 0,
            logs: Vec::new(),
            allocations: 0,
            sizes: BTreeMap::new(),
            data_imports: BTreeMap::new(),
            errno: 0,
            tls_keys: BTreeMap::new(),
            missing_imports: BTreeSet::new(),
        };
        let hello = r.request(json!({"op":"init","abi":abi.android_name()}))?;
        if hello["version"] != 1 {
            return Err(ioerr("worker version mismatch"));
        }
        r.map(ARENA, ARENA_SIZE)?;
        r.map(ENV, 0x10000)?;
        let w = abi.pointer_size();
        r.write_word(ENV, ENV + 0x1000)?;
        r.write_word(JAVA_VM, ENV + 0x2000)?;
        for i in 0..234 {
            r.write_word(ENV + 0x1000 + (i * w) as u64, TRAPS + (i * 16) as u64)?;
        }
        for i in 0..8 {
            r.write_word(
                ENV + 0x2000 + (i * w) as u64,
                TRAPS + ((300 + i) * 16) as u64,
            )?;
        }
        Ok(r)
    }
    fn request(&mut self, q: Value) -> Result<Value> {
        let mut b = serde_json::to_vec(&q).map_err(ioerr)?;
        if b.len() > 64 * 1024 * 1024 {
            return Err(ioerr("request exceeds IPC limit"));
        }
        b.push(b'\n');
        self.input.write_all(&b).map_err(ioerr)?;
        self.input.flush().map_err(ioerr)?;
        let v = self
            .output
            .recv_timeout(Duration::from_secs(7))
            .map_err(|e| {
                let _ = self.child.kill();
                ioerr(format!("worker response watchdog: {e}"))
            })?
            .map_err(ioerr)?;
        if v["event"] == "error" {
            return Err(RdError::new(
                v["code"].as_str().unwrap_or("NATIVE_FAULT"),
                v["cause"].as_str().unwrap_or("unknown worker error"),
            ));
        }
        Ok(v)
    }
    pub fn map(&mut self, address: u64, size: u64) -> Result<()> {
        self.request(json!({"op":"map","address":address,"size":size}))?;
        Ok(())
    }
    pub fn read(&mut self, address: u64, size: usize) -> Result<Vec<u8>> {
        if size > 16 * 1024 * 1024 {
            return Err(RdError::new("NATIVE_MEMORY", "read exceeds 16 MiB limit"));
        }
        let v = self.request(json!({"op":"read","address":address,"size":size}))?;
        unhex(
            v["hex"]
                .as_str()
                .ok_or_else(|| ioerr("missing read payload"))?,
        )
    }
    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(RdError::new("NATIVE_MEMORY", "write exceeds 16 MiB limit"));
        }
        self.request(json!({"op":"write","address":address,"hex":hex(bytes)}))?;
        Ok(())
    }
    pub fn read_word(&mut self, address: u64) -> Result<u64> {
        let b = self.read(address, self.abi.pointer_size())?;
        Ok(b.iter()
            .enumerate()
            .fold(0, |v, (i, b)| v | (u64::from(*b) << (i * 8))))
    }
    pub fn write_word(&mut self, address: u64, value: u64) -> Result<()> {
        self.write(address, &value.to_le_bytes()[..self.abi.pointer_size()])
    }
    pub fn c_string(&mut self, address: u64) -> Result<Vec<u8>> {
        self.read_c_string(address, 1024 * 1024, true)
    }
    pub fn c_string_bounded(&mut self, address: u64, max_bytes: usize) -> Result<Vec<u8>> {
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let require_nul = max_bytes > 1024 * 1024;
        self.read_c_string(address, max_bytes.min(1024 * 1024), require_nul)
    }
    fn read_c_string(&mut self, address: u64, limit: usize, require_nul: bool) -> Result<Vec<u8>> {
        if address == 0 {
            return Err(RdError::new("NATIVE_MEMORY", "null C string"));
        }
        let mut s = Vec::new();
        while s.len() < limit {
            let cur = address
                .checked_add(s.len() as u64)
                .ok_or_else(|| RdError::new("NATIVE_MEMORY", "C string address overflow"))?;
            let page_rem = 4096 - (cur as usize % 4096);
            let rem = limit - s.len();
            let chunk = 256usize.min(page_rem).min(rem);
            let b = self.read(cur, chunk)?;
            if let Some(n) = b.iter().position(|c| *c == 0) {
                s.extend_from_slice(&b[..n]);
                return Ok(s);
            }
            s.extend_from_slice(&b);
        }
        if require_nul {
            Err(RdError::new(
                "NATIVE_MEMORY",
                "unterminated C string exceeds 1 MiB",
            ))
        } else {
            Ok(s)
        }
    }
    pub fn allocate(&mut self, size: usize) -> Result<u64> {
        let size = (size.max(1) as u64)
            .checked_add(15)
            .ok_or_else(|| RdError::new("NATIVE_OOM", "allocation overflow"))?
            & !15;
        let end = self
            .arena_used
            .checked_add(size)
            .ok_or_else(|| RdError::new("NATIVE_OOM", "allocation overflow"))?;
        if end > ARENA_SIZE {
            return Err(RdError::new("NATIVE_OOM", "64 MiB native arena exhausted"));
        }
        let p = ARENA + self.arena_used;
        self.arena_used = end;
        self.allocations += 1;
        self.sizes.insert(p, size as usize);
        Ok(p)
    }
    pub fn alloc_bytes(&mut self, b: &[u8]) -> Result<u64> {
        let p = self.allocate(b.len())?;
        self.write(p, b)?;
        Ok(p)
    }
    pub fn symbol(&self, name: &str) -> Option<u64> {
        self.libraries
            .values()
            .find_map(|l| l.elf.symbol(name).and_then(|v| l.base.checked_add(v)))
    }
    pub fn library_symbol(&self, library: &str, name: &str) -> Option<u64> {
        self.libraries
            .get(library)
            .and_then(|l| l.elf.symbol(name).and_then(|v| l.base.checked_add(v)))
    }
    pub fn is_loaded(&self, name: &str) -> bool {
        self.libraries.contains_key(name)
    }
    pub fn load(&mut self, name: &str, available: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u64>> {
        let mut pending = vec![name.to_string()];
        let mut added = Vec::new();
        let mut seen = BTreeSet::new();
        while let Some(n) = pending.pop() {
            if self.is_loaded(&n) || !seen.insert(n.clone()) {
                continue;
            }
            let b = available.get(&n).ok_or_else(|| {
                RdError::new(
                    "NATIVE_LIBRARY_NOT_FOUND",
                    format!("APK lacks {n} for {}", self.abi.android_name()),
                )
            })?;
            let elf = Elf::parse(b)?;
            if elf.abi != self.abi {
                return Err(RdError::new("UNSUPPORTED_ABI", n));
            }
            let base = self.next_base;
            let size = elf.memory_size.max(4096);
            let step = size
                .checked_add(65536)
                .ok_or_else(|| RdError::new("NATIVE_OOM", "ELF base overflow"))?;
            let next = base
                .checked_add(step)
                .ok_or_else(|| RdError::new("NATIVE_OOM", "ELF base overflow"))?;
            if next > 0x20000000 {
                return Err(RdError::new(
                    "NATIVE_OOM",
                    "total ELF mapping budget exceeded",
                ));
            }
            self.map(base, size)?;
            self.next_base = next;
            for s in &elf.segments {
                let seg_base = base
                    .checked_add(s.address)
                    .ok_or_else(|| RdError::new("INVALID_ELF", "segment address overflow"))?;
                for (i, chunk) in s.bytes.chunks(8 * 1024 * 1024).enumerate() {
                    let dst = seg_base
                        .checked_add((i * 8 * 1024 * 1024) as u64)
                        .ok_or_else(|| RdError::new("INVALID_ELF", "segment write overflow"))?;
                    self.write(dst, chunk)?;
                }
            }
            for dep in &elf.needed {
                if !platform_library(dep) {
                    pending.push(dep.clone());
                }
            }
            self.libraries.insert(n.clone(), Library { base, elf });
            added.push(n);
        }
        // All exports exist before relocation, allowing cyclic dependency graphs.
        for n in &added {
            let l = self.libraries[n].clone();
            for r in &l.elf.relocations {
                if r.kind == 0 {
                    continue;
                }
                let target = l
                    .base
                    .checked_add(r.offset)
                    .ok_or_else(|| RdError::new("INVALID_ELF", "relocation offset overflow"))?;
                let add = match r.addend {
                    Some(a) => a as u64,
                    None => self.read_word(target)?,
                };
                let value = if r.kind == relative_kind(self.abi) {
                    l.base.wrapping_add(add)
                } else {
                    let supported = match self.abi {
                        Abi::X86_64 => matches!(r.kind, 1 | 6 | 7),
                        Abi::Arm64 => matches!(r.kind, 257 | 1025 | 1026),
                        Abi::Arm => matches!(r.kind, 2 | 21 | 22),
                    };
                    if !supported {
                        return Err(RdError::new(
                            "UNSUPPORTED_RELOCATION",
                            format!("{} relocation {}", self.abi.android_name(), r.kind),
                        ));
                    }
                    let s =
                        l.elf.symbols.get(r.symbol).ok_or_else(|| {
                            RdError::new("INVALID_ELF", "relocation missing symbol")
                        })?;
                    let resolved = if s.defined {
                        l.base
                            .checked_add(s.value)
                            .ok_or_else(|| RdError::new("INVALID_ELF", "symbol value overflow"))?
                    } else if let Some(v) = self.symbol(&s.name) {
                        v
                    } else if builtin_import(&s.name) {
                        self.import(&s.name)?
                    } else if s.weak {
                        0
                    } else if s.kind == 1 {
                        self.data_import(&s.name, s.size)?
                    } else {
                        // Lazy binding like bionic: an unimplemented import
                        // fails when called at its trap slot.
                        self.import(&s.name)?
                    };
                    // GLOB_DAT/JUMP_SLOT do not add the implicit GOT contents on ARM/x86.
                    if matches!(
                        (self.abi, r.kind),
                        (Abi::Arm, 21 | 22) | (Abi::X86_64, 6 | 7)
                    ) {
                        resolved
                    } else {
                        resolved.wrapping_add(add)
                    }
                };
                self.write_word(target, value)?;
            }
        }
        let mut init = Vec::new();
        for n in added.iter().rev() {
            let l = self.libraries[n].clone();
            if let Some(a) = l.elf.init {
                let fn_ptr = l
                    .base
                    .checked_add(a)
                    .ok_or_else(|| RdError::new("INVALID_ELF", "DT_INIT address overflow"))?;
                init.push(fn_ptr);
            }
            if let Some((a, count)) = l.elf.init_array {
                let arr_base = l
                    .base
                    .checked_add(a)
                    .ok_or_else(|| RdError::new("INVALID_ELF", "init_array address overflow"))?;
                for i in 0..count {
                    let elem_off = (i as u64)
                        .checked_mul(self.abi.pointer_size() as u64)
                        .and_then(|v| arr_base.checked_add(v))
                        .ok_or_else(|| RdError::new("INVALID_ELF", "init_array entry overflow"))?;
                    let p = self.read_word(elem_off)?;
                    if p != 0 && p != u64::MAX && p != u32::MAX as u64 {
                        let p_addr = if self.abi == Abi::Arm { p & !1 } else { p };
                        let valid_target = self.libraries.values().any(|lib| {
                            lib.base
                                .checked_add(lib.elf.memory_size.max(4096))
                                .map(|end| p_addr >= lib.base && p_addr < end)
                                .unwrap_or(false)
                        });
                        if !valid_target {
                            return Err(RdError::new(
                                "INVALID_ELF",
                                "init_array entry points outside loaded ELF segments",
                            ));
                        }
                        init.push(p);
                    }
                }
            }
        }
        Ok(init)
    }
    fn data_import(&mut self, name: &str, size: u64) -> Result<u64> {
        if let Some(p) = self.data_imports.get(name) {
            return Ok(*p);
        }
        let n = size.clamp(64, 64 * 1024) as usize;
        let p = self.allocate(n)?;
        self.write(p, &vec![0; n])?;
        if name == "__stack_chk_guard" {
            self.write_word(p, 0x5244_4e44_4b4d_3921)?;
        }
        self.data_imports.insert(name.into(), p);
        Ok(p)
    }
    fn set_errno(&mut self, value: u32) -> Result<()> {
        if self.errno == 0 {
            self.errno = self.allocate(4)?;
        }
        self.write(self.errno, &value.to_le_bytes())
    }
    pub fn lazily_bound_imports(&self) -> Vec<String> {
        self.imports.keys().cloned().collect()
    }
    fn import(&mut self, name: &str) -> Result<u64> {
        if let Some(i) = self.imports.get(name) {
            return Ok(TRAPS + (*i * 16) as u64);
        }
        let i = 512 + self.imports.len();
        if i >= 4096 {
            return Err(RdError::new(
                "NATIVE_OOM",
                "import trampoline table exhausted",
            ));
        }
        self.imports.insert(name.into(), i);
        self.import_names.insert(i, name.into());
        Ok(TRAPS + (i * 16) as u64)
    }
    pub fn call<F>(
        &mut self,
        address: u64,
        args: &[Argument],
        fuel: u64,
        mut bridge: F,
    ) -> Result<Return>
    where
        F: FnMut(&mut Self, &Trap) -> Result<Return>,
    {
        let mut v = self.request(json!({"op":"call","address":address,"args":args,"fuel":fuel}))?;
        // The worker reports a running per-call total with every event; only
        // the increment since the previous event is new work.
        let mut seen = 0u64;
        loop {
            let total = v["instructions"].as_u64().unwrap_or(seen);
            self.instructions = self.instructions.saturating_add(total.saturating_sub(seen));
            seen = seen.max(total);
            match v["event"].as_str() {
                Some("return") => {
                    return Ok(Return {
                        bits: v["value"].as_u64().unwrap_or(0),
                        fp_bits: v["fp_value"].as_u64(),
                    })
                }
                Some("trap") => {
                    let t = Trap {
                        slot: v["slot"]
                            .as_u64()
                            .ok_or_else(|| ioerr("trap has no slot"))?
                            as usize,
                        args: serde_json::from_value(v["args"].clone()).map_err(ioerr)?,
                        fp: serde_json::from_value(v["fp"].clone()).map_err(ioerr)?,
                        sp: v["sp"].as_u64().unwrap_or(0),
                    };
                    let r = if let Some(n) = self.import_names.get(&t.slot).cloned() {
                        self.libc(&n, &t)?
                    } else {
                        bridge(self, &t)?
                    };
                    let mut q = json!({"op":"resume","value":r.bits});
                    if let Some(fp) = r.fp_bits {
                        q["fp_value"] = json!(fp);
                    }
                    v = self.request(q)?;
                }
                _ => return Err(ioerr("unexpected worker event")),
            }
        }
    }
    pub fn raw_argument(&mut self, t: &Trap, index: usize) -> Result<u64> {
        if let Some(v) = t.args.get(index) {
            Ok(*v)
        } else {
            let extra = index
                .checked_sub(t.args.len())
                .and_then(|d| d.checked_mul(self.abi.pointer_size()))
                .and_then(|b| b.checked_add(if self.abi == Abi::X86_64 { 8 } else { 0 }))
                .ok_or_else(|| RdError::new("NATIVE_MEMORY", "stack argument offset overflow"))?;
            let addr =
                t.sp.checked_add(extra as u64)
                    .ok_or_else(|| RdError::new("NATIVE_MEMORY", "stack pointer overflow"))?;
            self.read_word(addr)
        }
    }
    /// libm subset. x86_64/AArch64 pass floats in XMM/V registers; Android
    /// armeabi-v7a is softfp, so floats travel in core registers (a double
    /// occupies an even/odd pair) and come back in r0 (r0:r1 for doubles).
    fn libm(&mut self, name: &str, t: &Trap) -> Option<Result<Return>> {
        // (arity, f32?, integer result?)
        let (arity, single, int_result) = match name {
            "fmodf" | "atan2f" | "powf" | "hypotf" => (2, true, false),
            "fmod" | "atan2" | "pow" | "hypot" => (2, false, false),
            "sqrtf" | "floorf" | "ceilf" | "roundf" | "fabsf" | "expf" | "logf" | "sinf"
            | "cosf" | "tanf" | "atanf" | "truncf" | "log10f" => (1, true, false),
            "sqrt" | "floor" | "ceil" | "round" | "fabs" | "exp" | "log" | "sin" | "cos"
            | "tan" | "atan" | "trunc" | "log10" => (1, false, false),
            "lroundf" | "llroundf" => (1, true, true),
            "lround" | "llround" => (1, false, true),
            _ => return None,
        };
        let mut x = [0f64; 2];
        for (i, v) in x.iter_mut().enumerate().take(arity) {
            *v = match self.abi {
                Abi::Arm => {
                    if single {
                        f32::from_bits(t.args.get(i).copied().unwrap_or(0) as u32) as f64
                    } else {
                        let lo = t.args.get(2 * i).copied().unwrap_or(0) & 0xffff_ffff;
                        let hi = t.args.get(2 * i + 1).copied().unwrap_or(0) & 0xffff_ffff;
                        f64::from_bits((hi << 32) | lo)
                    }
                }
                _ => {
                    let bits = t.fp.get(i).copied().unwrap_or(0);
                    if single {
                        f32::from_bits(bits as u32) as f64
                    } else {
                        f64::from_bits(bits)
                    }
                }
            };
        }
        let (a, b) = (x[0], x[1]);
        let r = match name.trim_end_matches('f') {
            "fmod" => a % b,
            "atan2" => a.atan2(b),
            "pow" => a.powf(b),
            "hypot" => a.hypot(b),
            "sqrt" => a.sqrt(),
            "floor" => a.floor(),
            "ceil" => a.ceil(),
            "round" | "lround" | "llround" => a.round(),
            "fabs" => a.abs(),
            "exp" => a.exp(),
            "log" => a.ln(),
            "log10" => a.log10(),
            "sin" => a.sin(),
            "cos" => a.cos(),
            "tan" => a.tan(),
            "atan" => a.atan(),
            "trunc" => a.trunc(),
            _ => return None,
        };
        let r = if single && !int_result {
            r as f32 as f64
        } else {
            r
        };
        if int_result {
            let v = r as i64;
            let bits = if self.abi == Abi::Arm && name.starts_with("lround") {
                v as i32 as u32 as u64
            } else {
                v as u64
            };
            return Some(Ok(Return {
                bits,
                fp_bits: None,
            }));
        }
        let bits = if single {
            (r as f32).to_bits() as u64
        } else {
            r.to_bits()
        };
        Some(Ok(match self.abi {
            Abi::Arm => Return {
                bits,
                fp_bits: None,
            },
            _ => Return {
                bits: 0,
                fp_bits: Some(bits),
            },
        }))
    }
    fn libc(&mut self, name: &str, t: &Trap) -> Result<Return> {
        if let Some(r) = self.libm(name, t) {
            return r;
        }
        let mut a = Vec::new();
        for i in 0..6 {
            a.push(self.raw_argument(t, i)?);
        }
        let value = match name {
            "malloc" => {
                let sz = usize::try_from(a[0])
                    .map_err(|_| RdError::new("NATIVE_OOM", "malloc overflow"))?;
                self.allocate(sz)?
            }
            "calloc" => {
                let n = a[0]
                    .checked_mul(a[1])
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| RdError::new("NATIVE_OOM", "calloc overflow"))?;
                let p = self.allocate(n)?;
                self.write(p, &vec![0; n])?;
                p
            }
            "free" | "__cxa_finalize" => 0,
            "memcpy" | "memmove" | "__memcpy_chk" | "__memmove_chk" => {
                let n = usize::try_from(a[2]).map_err(ioerr)?;
                if name.ends_with("_chk") && a[2] > a[3] {
                    return Err(RdError::new("NATIVE_ABORT", "fortify bounds failure"));
                }
                let b = self.read(a[1], n)?;
                self.write(a[0], &b)?;
                a[0]
            }
            "memset" | "__memset_chk" => {
                if a[2] > 16 * 1024 * 1024 || name.ends_with("_chk") && a[2] > a[3] {
                    return Err(RdError::new("NATIVE_MEMORY", "memset bounds exceeded"));
                }
                self.write(a[0], &vec![a[1] as u8; a[2] as usize])?;
                a[0]
            }
            "memcmp" => {
                let n = usize::try_from(a[2])
                    .map_err(|_| RdError::new("NATIVE_MEMORY", "memcmp exceeds 16 MiB limit"))?;
                if n > 16 * 1024 * 1024 {
                    return Err(RdError::new("NATIVE_MEMORY", "memcmp exceeds 16 MiB limit"));
                }
                let x = self.read(a[0], n)?;
                let y = self.read(a[1], n)?;
                x.iter()
                    .zip(y.iter())
                    .find_map(|(x, y)| {
                        if x == y {
                            None
                        } else {
                            Some(i32::from(*x) - i32::from(*y))
                        }
                    })
                    .unwrap_or(0) as u64
            }
            "strlen" => self.c_string(a[0])?.len() as u64,
            "strcmp" => {
                let x = self.c_string(a[0])?;
                let y = self.c_string(a[1])?;
                match x.cmp(&y) {
                    std::cmp::Ordering::Less => (-1i64) as u64,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            }
            "strcpy" => {
                let mut b = self.c_string(a[1])?;
                b.push(0);
                self.write(a[0], &b)?;
                a[0]
            }
            "strdup" => {
                let mut b = self.c_string(a[0])?;
                b.push(0);
                self.alloc_bytes(&b)?
            }
            "__android_log_write" => {
                let tag = String::from_utf8_lossy(&self.c_string(a[1])?).to_string();
                let msg = String::from_utf8_lossy(&self.c_string(a[2])?).to_string();
                if self.logs.len() < 1000 {
                    self.logs.push(format!("{tag}: {msg}"));
                }
                1
            }
            "abort" | "__stack_chk_fail" | "__assert2" => {
                return Err(RdError::new("NATIVE_ABORT", name))
            }
            "__cxa_guard_acquire" => {
                if self.read(a[0], 1)?[0] == 0 {
                    1
                } else {
                    0
                }
            }
            "__cxa_guard_release" => {
                self.write(a[0], &[1])?;
                0
            }
            "__cxa_guard_abort" => 0,
            "__cxa_atexit"
            | "__register_atfork"
            | "pthread_mutex_lock"
            | "pthread_mutex_unlock"
            | "pthread_mutex_init"
            | "pthread_mutex_destroy" => 0,
            "__errno" => {
                if self.errno == 0 {
                    self.errno = self.allocate(4)?;
                }
                self.errno
            }
            "atoi" => {
                let s = self.c_string(a[0])?;
                let t = String::from_utf8_lossy(&s);
                let t = t.trim_start();
                let mut end = 0;
                for (i, c) in t.char_indices() {
                    if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
                        end = i + 1;
                    } else {
                        break;
                    }
                }
                t[..end].parse::<i64>().unwrap_or(0) as i32 as i64 as u64
            }
            "memchr" | "wmemchr" => {
                let w = if name == "wmemchr" { 4usize } else { 1usize };
                let n = usize::try_from(a[2])
                    .map_err(|_| RdError::new("NATIVE_MEMORY", "memchr exceeds limit"))?;
                let byte_len = n
                    .checked_mul(w)
                    .ok_or_else(|| RdError::new("NATIVE_MEMORY", "memchr exceeds limit"))?;
                if byte_len > 16 * 1024 * 1024 {
                    return Err(RdError::new("NATIVE_MEMORY", "memchr exceeds limit"));
                }
                let b = self.read(a[0], byte_len)?;
                let pos = if w == 1 {
                    b.iter().position(|x| *x == a[1] as u8)
                } else {
                    b.chunks_exact(4)
                        .position(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) == a[1] as u32)
                };
                match pos {
                    Some(i) => a[0]
                        .checked_add((i * w) as u64)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "memchr pointer overflow"))?,
                    None => 0,
                }
            }
            "wcslen" => {
                let mut n = 0u64;
                loop {
                    let addr = a[0]
                        .checked_add(n * 4)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "wcslen pointer overflow"))?;
                    if self.read(addr, 4)? == [0, 0, 0, 0] {
                        break;
                    }
                    n += 1;
                    if n > 4 * 1024 * 1024 {
                        return Err(RdError::new("NATIVE_MEMORY", "wcslen exceeds limit"));
                    }
                }
                n
            }
            "strncmp" => {
                let n = usize::try_from(a[2]).unwrap_or(usize::MAX);
                let x = self.c_string_bounded(a[0], n)?;
                let y = self.c_string_bounded(a[1], n)?;
                match x.cmp(&y) {
                    std::cmp::Ordering::Less => (-1i64) as u64,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            }
            "posix_memalign" => {
                let align = a[1];
                let min_align = self.abi.pointer_size() as u64;
                if align < min_align || !align.is_power_of_two() || align > 65536 {
                    22 // EINVAL
                } else {
                    let req = a[2]
                        .checked_add(align)
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| RdError::new("NATIVE_OOM", "posix_memalign overflow"))?;
                    let p = self.allocate(req)?;
                    let aligned = (p + align - 1) & !(align - 1);
                    self.write_word(a[0], aligned)?;
                    0
                }
            }
            // Single-threaded guest: no other thread can hold a lock or wait
            // on a condition, so these are uncontended successes. A wait
            // (`pthread_cond_wait`) would block forever and stays unimplemented.
            "pthread_rwlock_rdlock"
            | "pthread_rwlock_wrlock"
            | "pthread_rwlock_unlock"
            | "pthread_rwlock_init"
            | "pthread_rwlock_destroy"
            | "pthread_cond_broadcast"
            | "pthread_cond_signal"
            | "pthread_cond_init"
            | "pthread_cond_destroy"
            | "pthread_key_delete" => 0,
            // getauxval: page size and the CPU features Unicorn emulates
            // (AArch64 FP+ASIMD; ARMv7 half/thumb/fastmult/VFP/EDSP/NEON/VFPv3/TLS).
            "strstr" => {
                let h = self.c_string(a[0])?;
                let n = self.c_string(a[1])?;
                if n.is_empty() {
                    a[0]
                } else {
                    match h.windows(n.len()).position(|w| w == n.as_slice()) {
                        Some(i) => a[0].checked_add(i as u64).ok_or_else(|| {
                            RdError::new("NATIVE_MEMORY", "strstr pointer overflow")
                        })?,
                        None => 0,
                    }
                }
            }
            "getauxval" => match a[0] & 0xffff_ffff {
                6 => 4096,
                16 => match self.abi {
                    Abi::Arm64 => 0x3,
                    Abi::Arm => 45270,
                    Abi::X86_64 => 0,
                },
                _ => 0,
            },
            "strchr" | "strrchr" | "__strchr_chk" => {
                let s = self.c_string(a[0])?;
                let c = a[1] as u8;
                if c == 0 {
                    a[0].checked_add(s.len() as u64)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "strchr pointer overflow"))?
                } else {
                    let pos = if name != "strrchr" {
                        s.iter().position(|b| *b == c)
                    } else {
                        s.iter().rposition(|b| *b == c)
                    };
                    match pos {
                        Some(i) => a[0].checked_add(i as u64).ok_or_else(|| {
                            RdError::new("NATIVE_MEMORY", "strchr pointer overflow")
                        })?,
                        None => 0,
                    }
                }
            }
            "strncpy" => {
                let n = usize::try_from(a[2])
                    .map_err(|_| RdError::new("NATIVE_MEMORY", "strncpy exceeds limit"))?;
                if n > 16 * 1024 * 1024 {
                    return Err(RdError::new("NATIVE_MEMORY", "strncpy exceeds limit"));
                }
                let mut s = self.c_string_bounded(a[1], n)?;
                s.truncate(n);
                s.resize(n, 0);
                self.write(a[0], &s)?;
                a[0]
            }
            "__strlen_chk" => {
                let n = self.c_string(a[0])?.len() as u64;
                if n >= a[1] {
                    return Err(RdError::new("NATIVE_ABORT", "fortify strlen overflow"));
                }
                n
            }
            "realloc" => {
                let req = usize::try_from(a[1])
                    .map_err(|_| RdError::new("NATIVE_OOM", "realloc overflow"))?;
                let old = if a[0] == 0 {
                    0
                } else {
                    *self.sizes.get(&a[0]).ok_or_else(|| {
                        RdError::new("NATIVE_MEMORY", "realloc of unknown pointer")
                    })?
                };
                let p = self.allocate(req)?;
                let n = old.min(req);
                if n > 0 {
                    let b = self.read(a[0], n)?;
                    self.write(p, &b)?;
                }
                p
            }
            // Deterministic virtual resource limits (RLIMIT_* are per-process).
            "getrlimit" => {
                let w = self.abi.pointer_size();
                let mut b = Vec::new();
                b.extend_from_slice(&1024u64.to_le_bytes()[..w]);
                b.extend_from_slice(&4096u64.to_le_bytes()[..w]);
                self.write(a[1], &b)?;
                0
            }
            "setrlimit" => 0,
            "__system_property_get" => {
                // Deterministic device: system properties are absent.
                self.write(a[1], &[0])?;
                0
            }
            "getenv" => 0,
            "getpid" | "gettid" => 4242,
            "getpagesize" => 4096,
            "pthread_key_create" => {
                let key = self.tls_keys.len() as u64 + 1;
                self.tls_keys.insert(key, 0);
                self.write(a[0], &(key as u32).to_le_bytes())?;
                0
            }
            "pthread_getspecific" => self
                .tls_keys
                .get(&(a[0] & 0xffff_ffff))
                .copied()
                .unwrap_or(0),
            "pthread_setspecific" => {
                self.tls_keys.insert(a[0] & 0xffff_ffff, a[1]);
                0
            }
            // Fail-closed virtual kernel: the guest owns no file descriptors.
            "close" | "ioctl" | "setsockopt" | "getsockopt" | "read" | "write" | "fcntl" => {
                self.set_errno(9)?; // EBADF
                u64::MAX
            }
            "snprintf" | "sprintf" | "__sprintf_chk" | "__snprintf_chk" | "vsnprintf"
            | "vsprintf" | "__vsprintf_chk" | "__vsnprintf_chk" | "asprintf" | "vasprintf" => {
                // (dest, [size], [flag, objsize], fmt, args|va_list)
                let (fmt_i, cap) = match name {
                    "snprintf" | "vsnprintf" => (2, Some(a[1])),
                    "sprintf" | "vsprintf" | "asprintf" | "vasprintf" => (1, None),
                    "__sprintf_chk" | "__vsprintf_chk" => (3, Some(a[2])),
                    _ => (4, Some(a[1].min(a[3]))),
                };
                let fmt = self.c_string(a[fmt_i])?;
                let mut src = if name.contains("printf_chk") && name.starts_with("__v")
                    || matches!(name, "vsnprintf" | "vsprintf" | "vasprintf")
                {
                    VaSource::List(self.va_list(a[fmt_i + 1])?)
                } else {
                    VaSource::Direct(fmt_i + 1, 0)
                };
                let out = self.format(&fmt, t, &mut src)?;
                let mut bytes = out.clone();
                if matches!(name, "asprintf" | "vasprintf") {
                    bytes.push(0);
                    let p = self.alloc_bytes(&bytes)?;
                    self.write_word(a[0], p)?;
                } else {
                    if name.starts_with("__")
                        && name.contains("sprintf_chk")
                        && out.len() as u64 >= cap.unwrap_or(u64::MAX)
                    {
                        return Err(RdError::new("NATIVE_ABORT", "fortify sprintf overflow"));
                    }
                    if let Some(c) = cap {
                        if c == 0 {
                            return Ok(Return {
                                bits: out.len() as u64,
                                fp_bits: None,
                            });
                        }
                        bytes.truncate(c.saturating_sub(1) as usize);
                    }
                    bytes.push(0);
                    self.write(a[0], &bytes)?;
                }
                out.len() as u64
            }
            // stdio is a bounded sink: bytes are recorded (unformatted), never host I/O.
            "fprintf" | "vfprintf" | "fputs" | "puts" | "printf" | "vprintf" => {
                let fmt_arg = if matches!(name, "puts" | "printf" | "vprintf") {
                    a[0]
                } else {
                    a[1]
                };
                let text = String::from_utf8_lossy(&self.c_string(fmt_arg)?).to_string();
                if self.logs.len() < 1000 {
                    self.logs.push(format!("stdio: {text} [unformatted]"));
                }
                text.len() as u64
            }
            "fwrite" => a[2],
            "fputc" | "putchar" => a[0] & 0xff,
            "fflush" => 0,
            "__android_log_print" | "__android_log_vprint" => {
                let tag = String::from_utf8_lossy(&self.c_string(a[1])?).to_string();
                let fmt = self.c_string(a[2])?;
                let mut src = if name == "__android_log_vprint" {
                    VaSource::List(self.va_list(a[3])?)
                } else {
                    VaSource::Direct(3, 0)
                };
                let msg = match self.format(&fmt, t, &mut src) {
                    Ok(m) => String::from_utf8_lossy(&m).to_string(),
                    Err(_) => format!("{} [unformatted]", String::from_utf8_lossy(&fmt)),
                };
                if self.logs.len() < 1000 {
                    self.logs.push(format!("{tag}: {msg}"));
                }
                1
            }
            // C/POSIX locale only: locale handles are opaque tokens.
            "newlocale" | "uselocale" | "duplocale" => 1,
            "freelocale" => 0,
            "syscall" => {
                let futex = match self.abi {
                    Abi::X86_64 => 202,
                    Abi::Arm64 => 98,
                    Abi::Arm => 240,
                };
                if a[0] == futex {
                    0 // single guest thread: FUTEX_WAKE/WAIT never block
                } else {
                    self.set_errno(38)?; // ENOSYS: fail closed
                    u64::MAX
                }
            }
            _ => {
                self.missing_imports.insert(name.to_string());
                return Err(RdError::new(
                    "NOT_IMPLEMENTED",
                    format!("native import {name}"),
                ));
            }
        };
        Ok(Return {
            bits: value,
            fp_bits: None,
        })
    }
}
enum VaSource {
    /// Next general-purpose and floating-point argument register index.
    Direct(usize, usize),
    List(VaList),
}
enum VaList {
    /// x86-64 SysV: gp_offset, overflow_arg_area, reg_save_area.
    X86 {
        gp: u64,
        fp: u64,
        overflow: u64,
        save: u64,
    },
    /// AAPCS64: __stack, __gr_top, __gr_offs.
    Arm64 {
        stack: u64,
        gr_top: u64,
        gr_offs: i64,
        vr_top: u64,
        vr_offs: i64,
    },
    /// AAPCS32: a plain pointer.
    Arm { next: u64 },
}
impl Runtime {
    fn va_list(&mut self, p: u64) -> Result<VaList> {
        Ok(match self.abi {
            Abi::X86_64 => {
                let b = self.read(p, 24)?;
                let w = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                VaList::X86 {
                    gp: u64::from(u32::from_le_bytes(b[0..4].try_into().unwrap())),
                    fp: u64::from(u32::from_le_bytes(b[4..8].try_into().unwrap())),
                    overflow: w(8),
                    save: w(16),
                }
            }
            Abi::Arm64 => {
                let b = self.read(p, 32)?;
                let w = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                VaList::Arm64 {
                    stack: w(0),
                    gr_top: w(8),
                    gr_offs: i64::from(i32::from_le_bytes(b[24..28].try_into().unwrap())),
                    vr_top: w(16),
                    vr_offs: i64::from(i32::from_le_bytes(b[28..32].try_into().unwrap())),
                }
            }
            Abi::Arm => VaList::Arm { next: p },
        })
    }
    fn next_arg(&mut self, t: &Trap, src: &mut VaSource, wide: bool) -> Result<u64> {
        match src {
            VaSource::Direct(i, _) => {
                if self.abi == Abi::Arm && wide {
                    *i += *i & 1;
                    let lo = self.raw_argument(t, *i)?;
                    let hi = self.raw_argument(t, *i + 1)?;
                    *i += 2;
                    return Ok((lo & 0xffff_ffff) | (hi << 32));
                }
                let v = self.raw_argument(t, *i)?;
                *i += 1;
                Ok(v)
            }
            VaSource::List(VaList::X86 {
                gp, overflow, save, ..
            }) => {
                if *gp < 48 {
                    let addr = save
                        .checked_add(*gp)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    let v = self.read_word(addr)?;
                    *gp += 8;
                    Ok(v)
                } else {
                    let v = self.read_word(*overflow)?;
                    *overflow = overflow
                        .checked_add(8)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    Ok(v)
                }
            }
            VaSource::List(VaList::Arm64 {
                stack,
                gr_top,
                gr_offs,
                ..
            }) => {
                if *gr_offs < 0 {
                    let v = self.read_word(gr_top.wrapping_add(*gr_offs as u64))?;
                    *gr_offs += 8;
                    Ok(v)
                } else {
                    let v = self.read_word(*stack)?;
                    *stack = stack
                        .checked_add(8)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    Ok(v)
                }
            }
            VaSource::List(VaList::Arm { next }) => {
                if wide {
                    let aligned = next
                        .checked_add(7)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?
                        & !7;
                    let hi_addr = aligned
                        .checked_add(4)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    let after = aligned
                        .checked_add(8)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    let lo = self.read_word(aligned)?;
                    let hi = self.read_word(hi_addr)?;
                    *next = after;
                    Ok((lo & 0xffff_ffff) | (hi << 32))
                } else {
                    let after = next
                        .checked_add(4)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    let v = self.read_word(*next)?;
                    *next = after;
                    Ok(v)
                }
            }
        }
    }
    /// Reads JNI `Call*MethodV` arguments from a guest `va_list` for the given
    /// descriptor types. C default promotions apply: `float` travels as
    /// `double` (returned here as f32 bits), small integers as `int`.
    /// x86-64 and AArch64 pass a pointer to the `va_list` object; 32-bit ARM
    /// passes the `__ap` pointer itself.
    pub fn va_list_args(&mut self, va: u64, types: &[String]) -> Result<Vec<u64>> {
        if types.len() > 256 {
            return Err(RdError::new("JNI_INVALID", "too many va_list arguments"));
        }
        let t = Trap {
            slot: 0,
            args: Vec::new(),
            fp: Vec::new(),
            sp: 0,
        };
        let mut src = VaSource::List(self.va_list(va)?);
        let mut out = Vec::with_capacity(types.len());
        for ty in types {
            out.push(match ty.as_str() {
                "F" => (self.next_fp_arg(&t, &mut src)? as f32).to_bits() as u64,
                "D" => self.next_fp_arg(&t, &mut src)?.to_bits(),
                "J" => self.next_arg(&t, &mut src, true)?,
                // objects and int-class values: one pointer-sized slot
                _ => self.next_arg(&t, &mut src, false)?,
            });
        }
        Ok(out)
    }
    /// Next `double` vararg (floats are promoted to double by C).
    fn next_fp_arg(&mut self, t: &Trap, src: &mut VaSource) -> Result<f64> {
        let bits = match src {
            // softfp: doubles use an aligned core-register pair / stack slot
            _ if self.abi == Abi::Arm => self.next_arg(t, src, true)?,
            VaSource::Direct(_, f) => {
                let v = t.fp.get(*f).copied().ok_or_else(|| {
                    RdError::new(
                        "NOT_IMPLEMENTED",
                        "printf: more than 8 floating-point register arguments",
                    )
                })?;
                *f += 1;
                v
            }
            VaSource::List(VaList::X86 {
                fp, overflow, save, ..
            }) => {
                if *fp < 176 {
                    let addr = save
                        .checked_add(*fp)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    let v = self.read_word(addr)?;
                    *fp += 16;
                    v
                } else {
                    let v = self.read_word(*overflow)?;
                    *overflow = overflow
                        .checked_add(8)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    v
                }
            }
            VaSource::List(VaList::Arm64 {
                stack,
                vr_top,
                vr_offs,
                ..
            }) => {
                if *vr_offs < 0 {
                    let v = self.read_word(vr_top.wrapping_add(*vr_offs as u64))?;
                    *vr_offs += 16;
                    v
                } else {
                    let v = self.read_word(*stack)?;
                    *stack = stack
                        .checked_add(8)
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "va_list pointer overflow"))?;
                    v
                }
            }
            VaSource::List(VaList::Arm { .. }) => unreachable!("handled by the softfp arm"),
        };
        Ok(f64::from_bits(bits))
    }
    /// printf subset: flags `-0+ #`, width, precision, `hh h l ll z j t`,
    /// conversions `d i u x X o c s p f F e E g G %`.
    fn format(&mut self, fmt: &[u8], t: &Trap, src: &mut VaSource) -> Result<Vec<u8>> {
        const MAX_FORMAT_BYTES: usize = 1024 * 1024;
        let mut out = Vec::new();
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] != b'%' {
                out.push(fmt[i]);
                if out.len() > MAX_FORMAT_BYTES {
                    return Err(RdError::new(
                        "NATIVE_MEMORY",
                        "formatted output exceeds 1 MiB",
                    ));
                }
                i += 1;
                continue;
            }
            i += 1;
            let mut left = false;
            let mut zero = false;
            let mut plus = false;
            let mut space = false;
            let mut alt = false;
            while i < fmt.len() && b"-0+ #".contains(&fmt[i]) {
                match fmt[i] {
                    b'-' => left = true,
                    b'0' => zero = true,
                    b'+' => plus = true,
                    b' ' => space = true,
                    b'#' => alt = true,
                    _ => {}
                }
                i += 1;
            }
            let mut width = 0usize;
            if i < fmt.len() && fmt[i] == b'*' {
                let w = self.next_arg(t, src, false)? as i32;
                if w < 0 {
                    left = true;
                    width = w.unsigned_abs() as usize;
                } else {
                    width = w as usize;
                }
                if width > MAX_FORMAT_BYTES {
                    return Err(RdError::new(
                        "NATIVE_MEMORY",
                        "printf field width exceeds 1 MiB",
                    ));
                }
                i += 1;
            } else {
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    width = width
                        .checked_mul(10)
                        .and_then(|v| v.checked_add(usize::from(fmt[i] - b'0')))
                        .ok_or_else(|| RdError::new("NATIVE_MEMORY", "printf width overflow"))?;
                    if width > MAX_FORMAT_BYTES {
                        return Err(RdError::new(
                            "NATIVE_MEMORY",
                            "printf field width exceeds 1 MiB",
                        ));
                    }
                    i += 1;
                }
            }
            let mut precision = None;
            if i < fmt.len() && fmt[i] == b'.' {
                i += 1;
                if i < fmt.len() && fmt[i] == b'*' {
                    let p = self.next_arg(t, src, false)? as i32;
                    i += 1;
                    if p >= 0 {
                        precision = Some(p as usize);
                    }
                } else {
                    let mut p = 0usize;
                    while i < fmt.len() && fmt[i].is_ascii_digit() {
                        p = p
                            .checked_mul(10)
                            .and_then(|v| v.checked_add(usize::from(fmt[i] - b'0')))
                            .unwrap_or(MAX_FORMAT_BYTES + 1);
                        i += 1;
                    }
                    precision = Some(p);
                }
            }
            let mut l_count = 0usize;
            let mut intmax = false;
            while i < fmt.len() && b"hlzjtL".contains(&fmt[i]) {
                match fmt[i] {
                    b'l' => l_count += 1,
                    b'j' => intmax = true,
                    b'z' | b't' if self.abi != Abi::Arm => l_count = l_count.max(1),
                    _ => {}
                }
                i += 1;
            }
            let Some(&conv) = fmt.get(i) else { break };
            i += 1;
            let wide = intmax || l_count >= 2 || (l_count == 1 && self.abi != Abi::Arm);
            let mut zero_handled = false;
            let body: Vec<u8> = match conv {
                b'%' => {
                    width = 0;
                    b"%".to_vec()
                }
                b'd' | b'i' => {
                    zero_handled = true;
                    let raw = self.next_arg(t, src, wide)?;
                    let v = if wide {
                        raw as i64
                    } else {
                        i64::from(raw as i32)
                    };
                    let sign: &[u8] = if v < 0 {
                        b"-"
                    } else if plus {
                        b"+"
                    } else if space {
                        b" "
                    } else {
                        b""
                    };
                    let mut digits = v.unsigned_abs().to_string().into_bytes();
                    if let Some(prec) = precision {
                        if prec > MAX_FORMAT_BYTES {
                            return Err(RdError::new(
                                "NATIVE_MEMORY",
                                "printf precision exceeds 1 MiB",
                            ));
                        }
                        if prec == 0 && v == 0 {
                            digits.clear();
                        } else if digits.len() < prec {
                            let mut padded = vec![b'0'; prec - digits.len()];
                            padded.extend_from_slice(&digits);
                            digits = padded;
                        }
                    } else if zero && !left && sign.len() + digits.len() < width {
                        let mut padded = vec![b'0'; width - sign.len() - digits.len()];
                        padded.extend_from_slice(&digits);
                        digits = padded;
                    }
                    let mut res = Vec::with_capacity(sign.len() + digits.len());
                    res.extend_from_slice(sign);
                    res.extend_from_slice(&digits);
                    res
                }
                b'u' | b'x' | b'X' | b'o' => {
                    zero_handled = true;
                    let raw = self.next_arg(t, src, wide)?;
                    let v = if wide { raw } else { raw & 0xffff_ffff };
                    let mut digits = match conv {
                        b'u' => v.to_string(),
                        b'x' => format!("{v:x}"),
                        b'X' => format!("{v:X}"),
                        _ => format!("{v:o}"),
                    }
                    .into_bytes();
                    let prefix: &[u8] = if alt && v != 0 {
                        match conv {
                            b'x' => b"0x",
                            b'X' => b"0X",
                            b'o' => b"0",
                            _ => b"",
                        }
                    } else {
                        b""
                    };
                    if let Some(prec) = precision {
                        if prec > MAX_FORMAT_BYTES {
                            return Err(RdError::new(
                                "NATIVE_MEMORY",
                                "printf precision exceeds 1 MiB",
                            ));
                        }
                        if prec == 0 && v == 0 {
                            digits = if conv == b'o' && alt {
                                b"0".to_vec()
                            } else {
                                Vec::new()
                            };
                        } else {
                            let eff_prec = if conv == b'o' && alt {
                                prec.saturating_sub(1)
                            } else {
                                prec
                            };
                            if digits.len() < eff_prec {
                                let mut padded = vec![b'0'; eff_prec - digits.len()];
                                padded.extend_from_slice(&digits);
                                digits = padded;
                            }
                        }
                    } else if zero && !left && prefix.len() + digits.len() < width {
                        let mut padded = vec![b'0'; width - prefix.len() - digits.len()];
                        padded.extend_from_slice(&digits);
                        digits = padded;
                    }
                    let mut res = Vec::with_capacity(prefix.len() + digits.len());
                    res.extend_from_slice(prefix);
                    res.extend_from_slice(&digits);
                    res
                }
                b'p' => {
                    format!("0x{:x}", self.next_arg(t, src, self.abi != Abi::Arm)?).into_bytes()
                }
                b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => {
                    zero_handled = true;
                    let v = self.next_fp_arg(t, src)?;
                    let mut body = c_float(v, conv, precision.unwrap_or(6), alt);
                    if !body.starts_with('-') {
                        if plus {
                            body.insert(0, '+');
                        } else if space {
                            body.insert(0, ' ');
                        }
                    }
                    if zero && !left && v.is_finite() && body.len() < width {
                        let sign = usize::from(body.starts_with(['-', '+', ' ']));
                        let pad = "0".repeat(width - body.len());
                        body.insert_str(sign, &pad);
                    }
                    body.into_bytes()
                }
                b'c' => vec![self.next_arg(t, src, false)? as u8],
                b's' => {
                    let p = self.next_arg(t, src, self.abi != Abi::Arm)?;
                    if p == 0 {
                        let mut s = b"(null)".to_vec();
                        if let Some(n) = precision {
                            s.truncate(n);
                        }
                        s
                    } else if let Some(n) = precision {
                        self.c_string_bounded(p, n)?
                    } else {
                        self.c_string(p)?
                    }
                }
                other => {
                    return Err(RdError::new(
                        "NOT_IMPLEMENTED",
                        format!("printf conversion %{}", other as char),
                    ))
                }
            };
            let field_len = body.len().max(width);
            if out
                .len()
                .checked_add(field_len)
                .map(|total| total > MAX_FORMAT_BYTES)
                .unwrap_or(true)
            {
                return Err(RdError::new(
                    "NATIVE_MEMORY",
                    "formatted output exceeds 1 MiB",
                ));
            }
            if body.len() < width {
                let pad_byte = if !zero_handled && zero && !left && conv != b's' {
                    b'0'
                } else {
                    b' '
                };
                let pad = vec![pad_byte; width - body.len()];
                if left {
                    out.extend(&body);
                    out.extend(pad);
                } else {
                    out.extend(pad);
                    out.extend(&body);
                }
            } else {
                out.extend(&body);
            }
        }
        Ok(out)
    }
}
/// C `%f/%e/%g` rendering (glibc/bionic compatible for finite values).
fn c_float(v: f64, conv: u8, precision: usize, alt: bool) -> String {
    let upper = conv.is_ascii_uppercase();
    if !v.is_finite() {
        let s = if v.is_nan() {
            "nan"
        } else if v < 0.0 {
            "-inf"
        } else {
            "inf"
        };
        return if upper { s.to_uppercase() } else { s.into() };
    }
    let p = precision.min(64);
    let exp_form = |p: usize| {
        let s = format!("{v:.p$e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let e: i32 = e.parse().unwrap_or(0);
        let m = if alt && p == 0 {
            format!("{m}.")
        } else {
            m.to_string()
        };
        format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    };
    let s = match conv.to_ascii_lowercase() {
        b'f' => {
            let s = format!("{v:.p$}");
            if alt && p == 0 {
                s + "."
            } else {
                s
            }
        }
        b'e' => exp_form(p),
        _ => {
            let p = if p == 0 { 1 } else { p };
            let e = if v == 0.0 {
                0
            } else {
                let s = format!("{v:.prec$e}", prec = p - 1);
                s.split_once('e')
                    .and_then(|(_, e)| e.parse::<i32>().ok())
                    .unwrap_or(0)
            };
            let mut s = if e < -4 || e >= p as i32 {
                exp_form(p - 1)
            } else {
                let fp = (p as i32 - 1 - e).max(0) as usize;
                format!("{v:.fp$}")
            };
            if !alt {
                let (mant, exp) = match s.find('e') {
                    Some(k) => (s[..k].to_string(), s[k..].to_string()),
                    None => (s.clone(), String::new()),
                };
                let mant = if mant.contains('.') {
                    mant.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    mant
                };
                s = mant + &exp;
            }
            s
        }
    };
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}
fn platform_library(n: &str) -> bool {
    // Android NDK system libraries: their symbols are host-provided imports.
    if matches!(
        n,
        "libstdc++.so"
            | "libjnigraphics.so"
            | "libOpenSLES.so"
            | "libEGL.so"
            | "libGLESv1_CM.so"
            | "libGLESv2.so"
            | "libGLESv3.so"
            | "libaaudio.so"
            | "libmediandk.so"
            | "libvulkan.so"
            | "libnativewindow.so"
            | "libcamera2ndk.so"
            | "libneuralnetworks.so"
            | "libsync.so"
            | "libbinder_ndk.so"
            | "libicu.so"
    ) {
        return true;
    }
    matches!(
        n,
        "libc.so" | "libm.so" | "libdl.so" | "liblog.so" | "libandroid.so" | "libz.so"
    )
}
fn builtin_import(n: &str) -> bool {
    matches!(
        n,
        "malloc"
            | "calloc"
            | "free"
            | "realloc"
            | "posix_memalign"
            | "memcpy"
            | "memmove"
            | "memset"
            | "memcmp"
            | "memchr"
            | "wmemchr"
            | "strlen"
            | "wcslen"
            | "strcmp"
            | "strncmp"
            | "strcpy"
            | "strncpy"
            | "strdup"
            | "strstr"
            | "strchr"
            | "strrchr"
            | "__strchr_chk"
            | "__strlen_chk"
            | "__memcpy_chk"
            | "__memmove_chk"
            | "__memset_chk"
            | "snprintf"
            | "sprintf"
            | "__sprintf_chk"
            | "__snprintf_chk"
            | "vsnprintf"
            | "vsprintf"
            | "__vsprintf_chk"
            | "__vsnprintf_chk"
            | "asprintf"
            | "vasprintf"
            | "fprintf"
            | "vfprintf"
            | "fputs"
            | "puts"
            | "printf"
            | "vprintf"
            | "fwrite"
            | "fputc"
            | "putchar"
            | "fflush"
            | "__android_log_write"
            | "__android_log_print"
            | "__android_log_vprint"
            | "abort"
            | "__stack_chk_fail"
            | "__assert2"
            | "__cxa_finalize"
            | "__cxa_atexit"
            | "__register_atfork"
            | "__cxa_guard_acquire"
            | "__cxa_guard_release"
            | "__cxa_guard_abort"
            | "__errno"
            | "atoi"
            | "__system_property_get"
            | "getauxval"
            | "getenv"
            | "getpid"
            | "gettid"
            | "getpagesize"
            | "getrlimit"
            | "setrlimit"
            | "pthread_key_create"
            | "pthread_key_delete"
            | "pthread_getspecific"
            | "pthread_setspecific"
            | "pthread_mutex_lock"
            | "pthread_mutex_unlock"
            | "pthread_mutex_init"
            | "pthread_mutex_destroy"
            | "pthread_rwlock_rdlock"
            | "pthread_rwlock_wrlock"
            | "pthread_rwlock_unlock"
            | "pthread_rwlock_init"
            | "pthread_rwlock_destroy"
            | "pthread_cond_broadcast"
            | "pthread_cond_signal"
            | "pthread_cond_init"
            | "pthread_cond_destroy"
            | "newlocale"
            | "uselocale"
            | "duplocale"
            | "freelocale"
            | "close"
            | "ioctl"
            | "setsockopt"
            | "getsockopt"
            | "read"
            | "write"
            | "fcntl"
            | "syscall"
            | "fmodf"
            | "atan2f"
            | "powf"
            | "hypotf"
            | "fmod"
            | "atan2"
            | "pow"
            | "hypot"
            | "sqrtf"
            | "floorf"
            | "ceilf"
            | "roundf"
            | "fabsf"
            | "expf"
            | "logf"
            | "sinf"
            | "cosf"
            | "tanf"
            | "atanf"
            | "truncf"
            | "log10f"
            | "sqrt"
            | "floor"
            | "ceil"
            | "round"
            | "fabs"
            | "exp"
            | "log"
            | "sin"
            | "cos"
            | "tan"
            | "atan"
            | "trunc"
            | "log10"
            | "lroundf"
            | "llroundf"
            | "lround"
            | "llround"
    )
}

impl Runtime {
    pub fn loaded_libraries(&self) -> Vec<String> {
        self.libraries.keys().cloned().collect()
    }
    pub fn arena_bytes(&self) -> u64 {
        self.arena_used
    }
}

#[cfg(test)]
mod float_format_tests {
    use super::c_float;
    #[test]
    fn matches_c_printf() {
        assert_eq!(c_float(1.23456_f64, b'f', 6, false), "1.234560");
        assert_eq!(c_float(1.23456_f64, b'f', 2, false), "1.23");
        assert_eq!(c_float(0.5_f64, b'f', 0, false), "0");
        assert_eq!(c_float(2.5_f64, b'f', 0, false), "2");
        assert_eq!(c_float(-0.0_f64, b'f', 1, false), "-0.0");
        assert_eq!(c_float(1234567.0_f64, b'e', 3, false), "1.235e+06");
        assert_eq!(c_float(0.000123_f64, b'e', 2, false), "1.23e-04");
        assert_eq!(c_float(0.0001_f64, b'g', 6, false), "0.0001");
        assert_eq!(c_float(1e-05_f64, b'g', 6, false), "1e-05");
        assert_eq!(c_float(123456.0_f64, b'g', 6, false), "123456");
        assert_eq!(c_float(1234567.0_f64, b'g', 6, false), "1.23457e+06");
        assert_eq!(c_float(100.0_f64, b'g', 6, false), "100");
        assert_eq!(c_float(0.5_f64, b'g', 1, false), "0.5");
        assert_eq!(c_float(1e-10_f64, b'G', 3, false), "1E-10");
        assert_eq!(c_float(42.0_f64, b'g', 0, false), "4e+01");
        assert_eq!(c_float(12.5_f64, b'f', 1, false), "12.5");
        assert_eq!(c_float(f64::NAN, b'F', 6, false), "NAN");
        assert_eq!(c_float(f64::NEG_INFINITY, b'f', 6, false), "-inf");
    }
}

#[cfg(test)]
mod p0_242_255_tests {
    use super::*;

    const MOCK_WORKER: &str = r#"
import json, sys
mem = {}
mapped = []
pending_trap = None

def is_mapped(addr, size):
    if size == 0:
        return True
    end = addr + size
    cur = addr
    while cur < end:
        ok = False
        for a, s in mapped:
            if a <= cur < a + s:
                cur = min(end, a + s)
                ok = True
                break
        if not ok:
            return False
    return True

def send(v):
    sys.stdout.write(json.dumps(v) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    q = json.loads(raw)
    op = q["op"]
    if op == "init":
        send({"version": 1})
    elif op == "map":
        mapped.append((int(q["address"]), int(q["size"])))
        send({"event": "ok"})
    elif op == "write":
        addr = int(q["address"])
        data = bytes.fromhex(q["hex"])
        if not is_mapped(addr, len(data)):
            send({"event": "error", "code": "NATIVE_FAULT", "cause": "unmapped write"})
        else:
            for idx, b in enumerate(data):
                mem[addr + idx] = b
            send({"event": "ok"})
    elif op == "read":
        addr = int(q["address"])
        sz = int(q["size"])
        if not is_mapped(addr, sz):
            send({"event": "error", "code": "NATIVE_FAULT", "cause": "unmapped read"})
        else:
            out = bytes(mem.get(addr + idx, 0) for idx, _ in enumerate(range(sz)))
            send({"event": "ok", "hex": out.hex()})
"#;

    fn mock_runtime(abi: Abi) -> Result<Runtime> {
        let mut child = Command::new("python3")
            .arg("-u")
            .arg("-c")
            .arg(MOCK_WORKER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(ioerr)?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| ioerr("mock stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ioerr("mock stdout unavailable"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let parsed =
                            serde_json::from_str::<Value>(&line).map_err(|e| e.to_string());
                        if tx.send(parsed).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        break;
                    }
                }
            }
        });
        let mut rt = Runtime {
            child,
            input,
            output: rx,
            abi,
            libraries: BTreeMap::new(),
            imports: BTreeMap::new(),
            import_names: BTreeMap::new(),
            next_base: 0x10000000,
            arena_used: 0,
            instructions: 0,
            logs: Vec::new(),
            allocations: 0,
            sizes: BTreeMap::new(),
            data_imports: BTreeMap::new(),
            errno: 0,
            tls_keys: BTreeMap::new(),
            missing_imports: BTreeSet::new(),
        };
        let hello = rt.request(json!({"op":"init","abi":abi.android_name()}))?;
        assert_eq!(hello["version"], 1);
        rt.map(ARENA, ARENA_SIZE)?;
        rt.map(ENV, 0x10000)?;
        Ok(rt)
    }

    fn build_weak_import_elf(weak_syms: &[&str], bad_init_ptr: Option<u64>) -> Vec<u8> {
        let mut d = vec![0u8; 0x2000];
        d[0..4].copy_from_slice(b"\x7fELF");
        d[4] = 2; // ELF64
        d[5] = 1; // LE
        d[6] = 1; // v1
        d[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        d[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        d[20..24].copy_from_slice(&1u32.to_le_bytes());
        d[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
        d[52..54].copy_from_slice(&64u16.to_le_bytes());
        d[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        d[56..58].copy_from_slice(&2u16.to_le_bytes()); // 2 phdrs: PT_LOAD + PT_DYNAMIC

        // PHDR 0: PT_LOAD covering 0x0..0x2000
        let p0 = 64usize;
        d[p0..p0 + 4].copy_from_slice(&1u32.to_le_bytes());
        d[p0 + 4..p0 + 8].copy_from_slice(&7u32.to_le_bytes());
        d[p0 + 32..p0 + 40].copy_from_slice(&0x2000u64.to_le_bytes());
        d[p0 + 40..p0 + 48].copy_from_slice(&0x2000u64.to_le_bytes());
        d[p0 + 48..p0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        // Layout inside 0x0..0x2000:
        // 0x200: dynstr
        // 0x400: dynsym
        // 0x700: rela.plt (JUMP_SLOT = 7) into GOT at 0x1000 + i*8
        // 0x900: init_array (optional 1 entry)
        // 0xa00: PT_DYNAMIC
        let mut strtab = vec![0u8];
        let mut name_offs = Vec::new();
        for s in weak_syms {
            name_offs.push(strtab.len() as u32);
            strtab.extend_from_slice(s.as_bytes());
            strtab.push(0);
        }
        d[0x200..0x200 + strtab.len()].copy_from_slice(&strtab);

        // Symbol 0 = STN_UNDEF; Symbols 1..=N = STB_WEAK (2<<4) | STT_FUNC (2) = 0x22, shndx = 0
        for (idx, &noff) in name_offs.iter().enumerate() {
            let sym_off = 0x400 + (idx + 1) * 24;
            d[sym_off..sym_off + 4].copy_from_slice(&noff.to_le_bytes());
            d[sym_off + 4] = 0x22;
            let rela_off = 0x700 + idx * 24;
            let got_va = 0x1000u64 + (idx as u64) * 8;
            let r_info = (((idx + 1) as u64) << 32) | 7u64; // R_X86_64_JUMP_SLOT
            d[rela_off..rela_off + 8].copy_from_slice(&got_va.to_le_bytes());
            d[rela_off + 8..rela_off + 16].copy_from_slice(&r_info.to_le_bytes());
        }

        let mut dyn_entries: Vec<(u64, u64)> = vec![
            (5, 0x200),                         // DT_STRTAB
            (10, strtab.len() as u64),          // DT_STRSZ
            (6, 0x400),                         // DT_SYMTAB
            (11, 24),                           // DT_SYMENT
            (7, 0x700),                         // DT_RELA
            (8, (weak_syms.len() * 24) as u64), // DT_RELASZ
            (9, 24),                            // DT_RELAENT
        ];
        if let Some(ptr) = bad_init_ptr {
            d[0x900..0x908].copy_from_slice(&ptr.to_le_bytes());
            dyn_entries.push((25, 0x900)); // DT_INIT_ARRAY
            dyn_entries.push((27, 8)); // DT_INIT_ARRAYSZ
        }
        dyn_entries.push((0, 0));

        let dyn_bytes = dyn_entries.len() * 16;
        for (i, (tag, val)) in dyn_entries.iter().enumerate() {
            let o = 0xa00 + i * 16;
            d[o..o + 8].copy_from_slice(&tag.to_le_bytes());
            d[o + 8..o + 16].copy_from_slice(&val.to_le_bytes());
        }

        let p1 = 64 + 56;
        d[p1..p1 + 4].copy_from_slice(&2u32.to_le_bytes()); // PT_DYNAMIC
        d[p1 + 4..p1 + 8].copy_from_slice(&6u32.to_le_bytes());
        d[p1 + 8..p1 + 16].copy_from_slice(&0xa00u64.to_le_bytes());
        d[p1 + 16..p1 + 24].copy_from_slice(&0xa00u64.to_le_bytes());
        d[p1 + 32..p1 + 40].copy_from_slice(&(dyn_bytes as u64).to_le_bytes());
        d[p1 + 40..p1 + 48].copy_from_slice(&(dyn_bytes as u64).to_le_bytes());
        d
    }

    #[test]
    fn p0_255_printf_width_precision_sign_and_arm32_intmax() -> Result<()> {
        let mut rt = mock_runtime(Abi::X86_64)?;
        let hi = rt.alloc_bytes(b"hi\0")?;

        // Negative dynamic width `%*s` with -8 -> left-justified width 8 (no ~18 EiB OOM abort!)
        let t_neg_w = Trap {
            slot: 0,
            args: vec![(-8i32 as u32) as u64, hi],
            fp: Vec::new(),
            sp: 0,
        };
        let mut src = VaSource::Direct(0, 0);
        assert_eq!(rt.format(b"%*s", &t_neg_w, &mut src)?, b"hi      ");

        // Dynamic width i32::MIN or > 1 MiB -> structured NATIVE_MEMORY error, never OOM
        for bad_w in [i32::MIN, -2_000_000, 2_000_000] {
            let t_bad = Trap {
                slot: 0,
                args: vec![(bad_w as u32) as u64, hi],
                fp: Vec::new(),
                sp: 0,
            };
            let mut s = VaSource::Direct(0, 0);
            let res = rt.format(b"%*s", &t_bad, &mut s);
            assert!(matches!(res, Err(ref e) if e.code == "NATIVE_MEMORY"));
        }

        // Literal huge/overflowing width -> structured NATIVE_MEMORY error
        let t_empty = Trap {
            slot: 0,
            args: vec![hi],
            fp: Vec::new(),
            sp: 0,
        };
        let mut s_ovf = VaSource::Direct(0, 0);
        let res_ovf = rt.format(b"%99999999999999999999999999s", &t_empty, &mut s_ovf);
        assert!(matches!(res_ovf, Err(ref e) if e.code == "NATIVE_MEMORY"));

        // Negative dynamic precision `%.*s` with -1 -> treated as omitted per C99
        let hello = rt.alloc_bytes(b"hello\0")?;
        let t_neg_p = Trap {
            slot: 0,
            args: vec![(-1i32 as u32) as u64, hello],
            fp: Vec::new(),
            sp: 0,
        };
        let mut s_np = VaSource::Direct(0, 0);
        assert_eq!(rt.format(b"%.*s", &t_neg_p, &mut s_np)?, b"hello");

        // Zero-padded negative signed int `%05d` on -42 -> "-0042" (never "00-42")
        let t_sign = Trap {
            slot: 0,
            args: vec![(-42i64) as u64, 42, 0xab],
            fp: Vec::new(),
            sp: 0,
        };
        let mut s_sign = VaSource::Direct(0, 0);
        assert_eq!(
            rt.format(b"%05d|%+05d|%#08x", &t_sign, &mut s_sign)?,
            b"-0042|+0042|0x0000ab"
        );

        // ARM32 (`armeabi-v7a`) `%jd` (`intmax_t` is 64-bit even on ILP32!)
        let mut arm_rt = mock_runtime(Abi::Arm)?;
        let ok_ptr = arm_rt.alloc_bytes(b"ok\0")?;
        let val64: i64 = -0x1234_5678_9abc_def0;
        let lo = (val64 as u64) & 0xffff_ffff;
        let hi_word = ((val64 as u64) >> 32) & 0xffff_ffff;
        // Start Direct at index 1 (odd!) to verify AAPCS32 even-register pair alignment -> r2:r3, then r4 for `%s`!
        let t_arm = Trap {
            slot: 0,
            args: vec![0xdead, 0xbeef, lo, hi_word, ok_ptr],
            fp: Vec::new(),
            sp: 0,
        };
        let mut s_arm = VaSource::Direct(1, 0);
        assert_eq!(
            arm_rt.format(b"%jd|%s", &t_arm, &mut s_arm)?,
            format!("{val64}|ok").into_bytes()
        );
        Ok(())
    }

    #[test]
    fn p0_255_bounded_reads_near_unmapped_page_and_libc_guards() -> Result<()> {
        let mut rt = mock_runtime(Abi::X86_64)?;
        // Map single page 0x3000_0000..0x3000_1000; 0x3000_1000 is unmapped!
        rt.map(0x3000_0000, 0x1000)?;
        let edge = 0x3000_0ffb;
        rt.write(edge, b"ABCDE")?;
        let other = rt.alloc_bytes(b"ABCDE\0")?;
        let dst = rt.allocate(16)?;
        rt.map(0x6000_0000, 0x1000)?; // dummy stack for raw_argument

        // strncmp(edge, other, 5) must NOT read 0x3000_1000
        let t_cmp = Trap {
            slot: 0,
            args: vec![edge, other, 5, 0, 0, 0],
            fp: Vec::new(),
            sp: 0x6000_0100,
        };
        assert_eq!(rt.libc("strncmp", &t_cmp)?.bits, 0);

        // strncpy(dst, edge, 5) must NOT read 0x3000_1000
        let t_cpy = Trap {
            slot: 0,
            args: vec![dst, edge, 5, 0, 0, 0],
            fp: Vec::new(),
            sp: 0x6000_0100,
        };
        assert_eq!(rt.libc("strncpy", &t_cpy)?.bits, dst);
        assert_eq!(rt.read(dst, 5)?, b"ABCDE");

        // snprintf(dst, 16, "%.*s", 5, edge) must NOT read 0x3000_1000
        let fmt_ptr = rt.alloc_bytes(b"%.*s\0")?;
        let t_snp = Trap {
            slot: 0,
            args: vec![dst, 16, fmt_ptr, 5, edge, 0],
            fp: Vec::new(),
            sp: 0x6000_0100,
        };
        assert_eq!(rt.libc("snprintf", &t_snp)?.bits, 5);
        assert_eq!(rt.read(dst, 6)?, b"ABCDE\0");

        // memcmp > 16 MiB rejected cleanly
        let t_memcmp = Trap {
            slot: 0,
            args: vec![other, other, 16 * 1024 * 1024 + 1, 0, 0, 0],
            fp: Vec::new(),
            sp: 0x6000_0100,
        };
        assert!(matches!(
            rt.libc("memcmp", &t_memcmp),
            Err(ref e) if e.code == "NATIVE_MEMORY"
        ));

        // posix_memalign: align < sizeof(void*) -> EINVAL (22); size near u64::MAX -> NATIVE_OOM (no panic)
        let out_ptr = rt.allocate(8)?;
        for bad_align in [1u64, 2, 4] {
            let t_align = Trap {
                slot: 0,
                args: vec![out_ptr, bad_align, 32, 0, 0, 0],
                fp: Vec::new(),
                sp: 0x6000_0100,
            };
            assert_eq!(rt.libc("posix_memalign", &t_align)?.bits, 22);
        }
        let t_ovf = Trap {
            slot: 0,
            args: vec![out_ptr, 16, u64::MAX - 4, 0, 0, 0],
            fp: Vec::new(),
            sp: 0x6000_0100,
        };
        assert!(matches!(
            rt.libc("posix_memalign", &t_ovf),
            Err(ref e) if e.code == "NATIVE_OOM"
        ));
        Ok(())
    }

    #[test]
    fn p0_255_weak_builtin_imports_and_p0_242_pointer_overflow_guards() -> Result<()> {
        let mut rt = mock_runtime(Abi::X86_64)?;
        let syms = [
            "getauxval",
            "posix_memalign",
            "snprintf",
            "fmodf",
            "pthread_rwlock_rdlock",
        ];
        let elf_bytes = build_weak_import_elf(&syms, None);
        let mut avail = BTreeMap::new();
        avail.insert("libweak.so".to_string(), elf_bytes);
        let init = rt.load("libweak.so", &avail)?;
        assert!(init.is_empty());

        // Every weak symbol implemented by libc/libm must bind to a TRAPS trampoline, never 0!
        for (idx, sym) in syms.iter().enumerate() {
            let got_slot = rt.read_word(0x10000000 + 0x1000 + (idx as u64) * 8)?;
            assert!(
                got_slot >= TRAPS,
                "weak builtin {sym} resolved to {got_slot:#x}"
            );
        }

        // #242: c_string and raw_argument near u64::MAX return structured errors without overflow panics
        rt.map(u64::MAX - 7, 8)?;
        rt.write(u64::MAX - 7, b"abcdefgh")?;
        assert!(matches!(
            rt.c_string(u64::MAX - 7),
            Err(ref e) if e.code == "NATIVE_MEMORY"
        ));
        let t_sp = Trap {
            slot: 0,
            args: vec![1, 2],
            fp: Vec::new(),
            sp: u64::MAX - 4,
        };
        assert!(matches!(
            rt.raw_argument(&t_sp, 5),
            Err(ref e) if e.code == "NATIVE_MEMORY"
        ));

        // #242: init_array entry pointing into ARENA (outside ELF segments) is rejected during load()
        let bad_init_elf = build_weak_import_elf(&[], Some(ARENA));
        avail.insert("libbadinit.so".to_string(), bad_init_elf);
        assert!(matches!(
            rt.load("libbadinit.so", &avail),
            Err(ref e) if e.code == "INVALID_ELF"
        ));
        Ok(())
    }
}
