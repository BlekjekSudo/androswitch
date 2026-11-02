//! Dalvik ↔ isolated native JNI bridge. No unsafe code or host pointers.
use crate::heap::{ElemKind, ObjRef};
use crate::{Engine, Throwable, Value, VmExit};
use rd_jni::{Handle, References};
use rd_ndk::elf::Abi;
use rd_ndk::runtime::{Argument, Return, Runtime, Trap, ENV, JAVA_VM};
use rd_ndk::{RdError, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
struct CachedReport {
    instructions: u64,
    libraries: Vec<String>,
    arena_bytes: u64,
    missing_imports: BTreeSet<String>,
    lazy_imports: usize,
    logs: Vec<String>,
}

impl CachedReport {
    fn sync_from(&mut self, rt: &Runtime) {
        self.instructions = rt.instructions;
        self.libraries = rt.loaded_libraries();
        self.arena_bytes = rt.arena_bytes();
        self.missing_imports = rt.missing_imports.clone();
        self.lazy_imports = rt.lazily_bound_imports().len();
        self.logs = rt.logs.clone();
    }
}

/// GC-aware wrapper around [`rd_jni::References`] tracking strong local/global
/// object handles separately from weak global handles (`NewWeakGlobalRef`).
#[derive(Debug)]
struct TrackedRefs {
    inner: References,
    local_frames: Vec<BTreeSet<u64>>,
    strong_globals: BTreeSet<u64>,
    weak_globals: BTreeMap<u64, Option<ObjRef>>,
    obj_by_handle: BTreeMap<u64, ObjRef>,
    pending: Option<Throwable>,
}

impl Default for TrackedRefs {
    fn default() -> Self {
        Self {
            inner: References::default(),
            local_frames: vec![BTreeSet::new()],
            strong_globals: BTreeSet::new(),
            weak_globals: BTreeMap::new(),
            obj_by_handle: BTreeMap::new(),
            pending: None,
        }
    }
}

impl TrackedRefs {
    fn insert(&mut self, v: Handle) -> rd_jni::Result<u64> {
        let obj = match &v {
            Handle::Object(r) if *r != 0 => Some(*r),
            _ => None,
        };
        let h = self.inner.insert(v)?;
        if let Some(f) = self.local_frames.last_mut() {
            f.insert(h);
        }
        if let Some(r) = obj {
            self.obj_by_handle.insert(h, r);
        }
        Ok(h)
    }

    fn get(&self, h: u64) -> rd_jni::Result<&Handle> {
        if let Some(slot) = self.weak_globals.get(&h) {
            if slot.is_none() {
                return Err(rd_jni::RdError::new(
                    "JNI_INVALID_REFERENCE",
                    format!("weak global JNI reference {h} was cleared by GC"),
                ));
            }
        }
        self.inner.get(h)
    }

    fn is_cleared_weak(&self, h: u64) -> bool {
        matches!(self.weak_globals.get(&h), Some(None))
    }

    fn duplicate(&mut self, h: u64, global: bool) -> rd_jni::Result<u64> {
        if h == 0 || self.is_cleared_weak(h) {
            return Ok(0);
        }
        let v = self.get(h)?.clone();
        let n = self.inner.duplicate(h, global)?;
        if global {
            self.strong_globals.insert(n);
        } else if let Some(f) = self.local_frames.last_mut() {
            f.insert(n);
        }
        if let Handle::Object(r) = v {
            if r != 0 {
                self.obj_by_handle.insert(n, r);
            }
        }
        Ok(n)
    }

    fn new_weak_global(&mut self, h: u64) -> rd_jni::Result<u64> {
        if h == 0 || self.is_cleared_weak(h) {
            return Ok(0);
        }
        let v = self.get(h)?.clone();
        let obj = match v {
            Handle::Object(r) if r != 0 => Some(r),
            _ => None,
        };
        if obj.is_none() && matches!(self.get(h), Ok(Handle::Object(0))) {
            return Ok(0);
        }
        let w = self.inner.duplicate(h, true)?;
        self.weak_globals.insert(w, obj);
        Ok(w)
    }

    fn delete_weak_global(&mut self, h: u64) {
        if self.weak_globals.remove(&h).is_some() {
            self.inner.delete_global(h);
        }
    }

    fn delete_local(&mut self, h: u64) {
        for frame in &mut self.local_frames {
            if frame.remove(&h) {
                self.obj_by_handle.remove(&h);
                self.inner.delete_local(h);
                break;
            }
        }
    }

    fn delete_global(&mut self, h: u64) {
        if self.strong_globals.remove(&h) {
            self.obj_by_handle.remove(&h);
            self.inner.delete_global(h);
        }
    }

    fn push(&mut self) -> rd_jni::Result<()> {
        self.inner.push()?;
        self.local_frames.push(BTreeSet::new());
        Ok(())
    }

    fn pop(&mut self, result: u64) -> rd_jni::Result<u64> {
        let eff = if self.is_cleared_weak(result) {
            0
        } else {
            result
        };
        let kept_obj = if eff == 0 {
            None
        } else {
            match self.get(eff)? {
                Handle::Object(r) if *r != 0 => Some(*r),
                _ => None,
            }
        };
        let out = self.inner.pop(eff)?;
        if let Some(popped) = self.local_frames.pop() {
            for h in popped {
                self.obj_by_handle.remove(&h);
            }
        }
        if out != 0 {
            if let Some(f) = self.local_frames.last_mut() {
                f.insert(out);
            }
            if let Some(r) = kept_obj {
                self.obj_by_handle.insert(out, r);
            }
        }
        Ok(out)
    }

    fn same(&self, a: u64, b: u64) -> rd_jni::Result<bool> {
        let a_null = a == 0 || self.is_cleared_weak(a);
        let b_null = b == 0 || self.is_cleared_weak(b);
        if a_null || b_null {
            return Ok(a_null && b_null);
        }
        self.inner.same(a, b)
    }

    fn ref_type(&self, h: u64) -> u64 {
        if h == 0 {
            0
        } else if self.weak_globals.contains_key(&h) {
            3
        } else if self.strong_globals.contains(&h) {
            2
        } else if self.local_frames.iter().any(|f| f.contains(&h)) {
            1
        } else {
            0
        }
    }

    fn collect_roots(&self, out: &mut Vec<ObjRef>) {
        out.extend(self.obj_by_handle.values().copied());
        if let Some(t) = &self.pending {
            if let Some(r) = t.obj {
                if r != 0 {
                    out.push(r);
                }
            }
        }
    }

    fn purge_dead_weak<F: Fn(&ObjRef) -> bool>(&mut self, alive: &F) -> usize {
        let mut cleared = 0usize;
        for slot in self.weak_globals.values_mut() {
            if let Some(r) = *slot {
                if !alive(&r) {
                    *slot = None;
                    cleared += 1;
                }
            }
        }
        cleared
    }
}

#[derive(Debug)]
pub struct NativeState {
    runtime: Option<Runtime>,
    abi: Abi,
    refs: TrackedRefs,
    registered: BTreeMap<(String, String, String), u64>,
    buffers: BTreeMap<u64, (ObjRef, char)>,
    onloads: BTreeSet<String>,
    extra_roots: Vec<ObjRef>,
    cached: CachedReport,
}

impl NativeState {
    pub(crate) fn collect_gc_roots(&self, out: &mut Vec<ObjRef>) {
        self.refs.collect_roots(out);
        for &(r, _) in self.buffers.values() {
            if r != 0 {
                out.push(r);
            }
        }
        out.extend(self.extra_roots.iter().copied().filter(|r| *r != 0));
    }

    pub(crate) fn purge_dead_weak<F: Fn(&ObjRef) -> bool>(&mut self, alive: &F) -> usize {
        self.refs.purge_dead_weak(alive)
    }
}

fn ndk(e: impl std::fmt::Debug) -> RdError {
    RdError::new("JNI_ERROR", format!("{e:?}"))
}
fn mem_err(msg: &str) -> RdError {
    RdError::new("MEMORY_ACCESS", msg)
}
fn vmerr(e: RdError) -> VmExit {
    VmExit::Error(crate::err::RdError {
        code: e.code,
        cause: e.cause,
        suggestion: e.suggestion,
        module_id: e.module_id,
    })
}
fn jerr(e: rd_jni::RdError) -> RdError {
    RdError::new(&e.code, e.cause)
}
fn state_mut(vm: &mut Engine) -> Result<&mut NativeState> {
    vm.native
        .as_mut()
        .ok_or_else(|| ndk("native state absent during JNI bridge call"))
}
fn state_ref(vm: &Engine) -> Result<&NativeState> {
    vm.native
        .as_ref()
        .ok_or_else(|| ndk("native state absent during JNI bridge call"))
}
fn hobj(refs: &TrackedRefs, h: u64) -> Result<ObjRef> {
    if refs.is_cleared_weak(h) {
        return Ok(0);
    }
    match refs.get(h).map_err(jerr)? {
        Handle::Object(r) => Ok(*r),
        _ => Err(ndk("JNI reference is not an object")),
    }
}
fn hclass(vm: &Engine, refs: &TrackedRefs, h: u64) -> Result<String> {
    match refs.get(h).map_err(jerr)? {
        Handle::Class(c) => Ok(c.clone()),
        Handle::Object(r) => {
            let r = *r;
            if vm.heap.class_of(r).ok() == Some("Ljava/lang/Class;") {
                for field in ["class_name", "name"] {
                    if let Ok(Value::Obj(s)) = vm.heap.get_field(r, field) {
                        if let Ok(desc) = vm.heap.as_str(s) {
                            let d = desc.to_string();
                            return Ok(
                                if d.starts_with('[') || d.starts_with('L') && d.ends_with(';') {
                                    d
                                } else {
                                    format!("L{};", d.replace('.', "/"))
                                },
                            );
                        }
                    }
                }
            }
            Err(ndk("JNI reference is not a class"))
        }
        _ => Err(ndk("JNI reference is not a class")),
    }
}
fn object(refs: &mut TrackedRefs, r: ObjRef) -> Result<u64> {
    if r == 0 {
        return Ok(0);
    }
    refs.insert(Handle::Object(r)).map_err(jerr)
}
fn to_native(refs: &mut TrackedRefs, v: &Value) -> Result<u64> {
    Ok(match v {
        Value::Null => 0,
        Value::Obj(r) => object(refs, *r)?,
        Value::Int(n) => *n as u64,
        Value::Long(n) => *n as u64,
        Value::Float(f) => f.to_bits() as u64,
        Value::Double(f) => f.to_bits(),
        _ => return Err(ndk("unsupported JNI value")),
    })
}
/// Converts a native register/jvalue word to a VM value. `narrow` (32-bit
/// ARM) drops the upper half for every non-64-bit type: a jvalue's unused
/// bytes and r1 of a 32-bit return are not part of the value.
fn from_native(vm: &mut Engine, b: u64, ty: &str, narrow: bool) -> Result<Value> {
    let b = if narrow && !matches!(ty, "J" | "D") {
        b & 0xffff_ffff
    } else {
        b
    };
    Ok(match ty.as_bytes().first() {
        Some(b'V') => Value::Null,
        // jboolean is one byte; the rest of a jvalue is unspecified
        Some(b'Z') => Value::Int((b as u8 != 0) as i32),
        Some(b'B') => Value::Int(b as i8 as i32),
        Some(b'C') => Value::Int(b as u16 as i32),
        Some(b'S') => Value::Int(b as i16 as i32),
        Some(b'I') => Value::Int(b as i32),
        Some(b'J') => Value::Long(b as i64),
        Some(b'F') => Value::Float(f32::from_bits(b as u32)),
        Some(b'D') => Value::Double(f64::from_bits(b)),
        Some(b'L' | b'[') => {
            if b == 0 {
                Value::Null
            } else {
                let class_desc = {
                    let s = state_ref(vm)?;
                    if s.refs.is_cleared_weak(b) {
                        return Ok(Value::Null);
                    }
                    match s.refs.get(b).map_err(jerr)? {
                        Handle::Object(0) => return Ok(Value::Null),
                        Handle::Object(r) => return Ok(Value::Obj(*r)),
                        Handle::Class(c) => c.clone(),
                        _ => return Err(ndk("JNI reference is not an object")),
                    }
                };
                let cls_obj = vm.class_object_for(&class_desc).map_err(ndk)?;
                Value::Obj(cls_obj)
            }
        }
        _ => return Err(ndk("invalid JNI return type")),
    })
}

fn normalize_library_name(name: &str) -> std::result::Result<String, VmExit> {
    if name.ends_with(".so") {
        Ok(name.rsplit('/').next().unwrap_or(name).to_string())
    } else {
        if name.contains('/') || name.is_empty() {
            return Err(
                Throwable::new("Ljava/lang/UnsatisfiedLinkError;", "invalid library name").into(),
            );
        }
        Ok(format!("lib{name}.so"))
    }
}

fn ensure(vm: &mut Engine) -> std::result::Result<(), VmExit> {
    if vm.native_active {
        return Err(vmerr(RdError::new(
            "NATIVE_REENTRANT",
            "nested native invocation from a JNI callback is not yet supported",
        )));
    }
    if vm.native.is_none() {
        let apk = vm.resources()?;
        // RD_NATIVE_ABI (x86_64 | arm64-v8a | armeabi-v7a) pins the ABI, e.g.
        // to run an APK's ARM libraries; default preference is x86_64 first.
        let wanted = std::env::var("RD_NATIVE_ABI")
            .ok()
            .filter(|w| !w.is_empty());
        let abi = [Abi::X86_64, Abi::Arm64, Abi::Arm]
            .into_iter()
            .filter(|a| wanted.as_deref().is_none_or(|w| w == a.android_name()))
            .find(|a| apk.abis.iter().any(|s| s == a.android_name()))
            .ok_or_else(|| {
                vmerr(RdError::new(
                    "UNSUPPORTED_ABI",
                    "APK contains no supported native ABI",
                ))
            })?;
        let rt = Runtime::new(abi).map_err(vmerr)?;
        let mut cached = CachedReport::default();
        cached.sync_from(&rt);
        vm.native = Some(NativeState {
            runtime: Some(rt),
            abi,
            refs: TrackedRefs::default(),
            registered: BTreeMap::new(),
            buffers: BTreeMap::new(),
            onloads: BTreeSet::new(),
            extra_roots: Vec::new(),
            cached,
        });
    }
    Ok(())
}

/// System.loadLibrary and System.load both resolve only APK-owned libraries.
pub fn load(vm: &mut Engine, name: &str) -> std::result::Result<(), VmExit> {
    let library = normalize_library_name(name)?;
    // Idempotent fast path: if this library already completed JNI_OnLoad / load
    // (including from a reentrant JNI → Java callback), succeed immediately.
    if vm
        .native
        .as_ref()
        .is_some_and(|s| s.onloads.contains(&library))
    {
        return Ok(());
    }
    ensure(vm)?;
    let abi_name = vm
        .native
        .as_ref()
        .map(|s| s.abi.android_name())
        .ok_or_else(|| vmerr(ndk("native state missing after ensure")))?;
    let mut available = BTreeMap::new();
    {
        let apk = vm.resources()?;
        let prefix = format!("lib/{abi_name}/");
        for n in &apk.native_libs {
            if let Some(short) = n.strip_prefix(&prefix) {
                available.insert(
                    short.to_string(),
                    apk.zip.read(n).map_err(|e| vmerr(ndk(e)))?,
                );
            }
        }
    }
    vm.native_active = true;
    let result = (|| -> Result<()> {
        let initializers = {
            let s = state_mut(vm)?;
            let rt = s
                .runtime
                .as_mut()
                .ok_or_else(|| ndk("native runtime unavailable during load"))?;
            let inits = rt.load(&library, &available)?;
            s.cached.sync_from(rt);
            // Record in `onloads` before running `.init_array` / `JNI_OnLoad` so
            // a Java `<clinit>` triggered by `JNI_OnLoad` calling
            // `System.loadLibrary` for the same `.so` is a no-op (AOSP ART
            // semantics). On error, `vm.native` is discarded below.
            s.onloads.insert(library.clone());
            inits
        };
        for address in initializers {
            run(vm, address, &[])?;
        }
        let onload_addr = {
            let s = state_ref(vm)?;
            let rt = s
                .runtime
                .as_ref()
                .ok_or_else(|| ndk("native runtime unavailable after initializers"))?;
            rt.library_symbol(&library, "JNI_OnLoad")
        };
        if let Some(address) = onload_addr {
            let version = run(
                vm,
                address,
                &[Argument::pointer(JAVA_VM), Argument::pointer(0)],
            )?
            .bits as u32;
            if !matches!(
                version,
                0x00010001 | 0x00010002 | 0x00010004 | 0x00010006 | 0x00010008
            ) {
                return Err(RdError::new(
                    "JNI_VERSION",
                    format!("JNI_OnLoad returned {version:#x}"),
                ));
            }
        }
        Ok(())
    })();
    vm.native_active = false;
    // A failed loader is discarded: half-relocated images must never be reused.
    if result.is_err() {
        vm.native = None;
    }
    result.map_err(vmerr)
}

pub fn invoke(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    is_static: bool,
    slots: &[Value],
) -> std::result::Result<Value, VmExit> {
    vm.with_host_roots(|vm| invoke_rooted(vm, class, name, sig, is_static, slots))
}

fn invoke_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    is_static: bool,
    slots: &[Value],
) -> std::result::Result<Value, VmExit> {
    ensure(vm)?;
    vm.native_active = true;
    let result = (|| -> Result<Value> {
        let key = (class.to_string(), name.to_string(), sig.to_string());
        let symbols = rd_jni::symbols(class, name, sig).map_err(jerr)?;
        let (address, narrow) = {
            let s = state_mut(vm)?;
            let rt = s
                .runtime
                .as_ref()
                .ok_or_else(|| ndk("native runtime unavailable"))?;
            let addr = s
                .registered
                .get(&key)
                .copied()
                .or_else(|| rt.symbol(&symbols[1]))
                .or_else(|| rt.symbol(&symbols[0]))
                .ok_or_else(|| {
                    RdError::new(
                        "NATIVE_UNRESOLVED_SYMBOL",
                        format!("{class}->{name}{sig}; loadLibrary must run first"),
                    )
                })?;
            s.refs.push().map_err(jerr)?;
            (addr, s.abi == Abi::Arm)
        };
        let inner = (|| -> Result<Value> {
            let (params, ret) = rd_jni::descriptor(sig).map_err(jerr)?;
            let mut args = vec![Argument::pointer(ENV)];
            {
                let s = state_mut(vm)?;
                if is_static {
                    args.push(Argument::pointer(
                        s.refs.insert(Handle::Class(class.into())).map_err(jerr)?,
                    ));
                } else {
                    let recv = slots
                        .first()
                        .ok_or_else(|| ndk("missing native receiver"))?;
                    args.push(Argument::pointer(to_native(&mut s.refs, recv)?));
                }
                let mut slot = if is_static { 0 } else { 1 };
                for ty in params {
                    let v = slots
                        .get(slot)
                        .ok_or_else(|| ndk("native argument missing"))?;
                    let kind = ty
                        .chars()
                        .next()
                        .ok_or_else(|| ndk("empty JNI parameter descriptor"))?;
                    args.push(Argument {
                        kind,
                        bits: to_native(&mut s.refs, v)?,
                    });
                    slot += if ty == "J" || ty == "D" { 2 } else { 1 };
                }
            }
            let r = run(vm, address, &args)?;
            let bits = if ret == "F" || ret == "D" {
                r.fp_bits.unwrap_or(r.bits)
            } else {
                r.bits
            };
            from_native(vm, bits, &ret, narrow)
        })();
        if let Ok(Value::Obj(r)) = &inner {
            vm.heap.gc_pin_return(*r);
        }
        if let Ok(s) = state_mut(vm) {
            let _ = s.refs.pop(0);
        }
        inner
    })();
    let pending = vm.native.as_mut().and_then(|s| s.refs.pending.take());
    vm.native_active = false;
    if let Some(exc) = pending {
        if let Some(r) = exc.obj {
            vm.heap.gc_pin_return(r);
        }
        return Err(VmExit::Exception(exc));
    }
    result.map_err(vmerr)
}

fn jni_default_for_desc(desc: &str) -> Value {
    match desc {
        "Z" | "B" | "C" | "S" | "I" => Value::Int(0),
        "J" => Value::Long(0),
        "F" => Value::Float(0.0),
        "D" => Value::Double(0.0),
        _ => Value::Null,
    }
}

fn jni_default_instance_fields(vm: &mut Engine, class: &str) -> Vec<(String, Value)> {
    let mut lineage = Vec::new();
    let mut cur = class.to_string();
    let mut seen = BTreeSet::new();
    for _ in 0..32 {
        if !seen.insert(cur.clone()) {
            break;
        }
        lineage.push(cur.clone());
        match vm.cp.superclass_of(&cur) {
            Some(sup) if sup != cur && sup != "Ljava/lang/Object;" => cur = sup,
            _ => break,
        }
    }
    let mut fields_map: BTreeMap<String, Value> = BTreeMap::new();
    for cls in lineage.into_iter().rev() {
        for (_owner, fname, ftype, _flags, is_static) in vm.cp.declared_fields_info(&cls) {
            if !is_static {
                fields_map.insert(fname, jni_default_for_desc(&ftype));
            }
        }
    }
    fields_map.into_iter().collect()
}

fn set_jni_pending(vm: &mut Engine, class: &str, msg: Option<String>) -> Result<()> {
    let fields = jni_default_instance_fields(vm, class);
    let obj = match vm.heap.alloc_instance(class.to_string(), fields) {
        Ok(r) => {
            if let Some(m) = &msg {
                if let Ok(ms) = vm.heap.alloc_string(m.clone()) {
                    let _ = vm.heap.put_field(r, "message", Value::Obj(ms));
                    let _ = vm.heap.put_field(r, "detailMessage", Value::Obj(ms));
                }
            }
            Some(r)
        }
        Err(_) => None,
    };
    state_mut(vm)?.refs.pending = Some(Throwable {
        class: class.to_string(),
        message: msg,
        obj,
    });
    Ok(())
}

fn record_vm_exception(vm: &mut Engine, mut exc: Throwable) -> Result<()> {
    if exc.obj.is_none() {
        if let Ok(r) = vm.materialize_throwable(&exc) {
            if let Some(m) = &exc.message {
                if let Ok(ms) = vm.heap.alloc_string(m.clone()) {
                    let _ = vm.heap.put_field(r, "detailMessage", Value::Obj(ms));
                }
            }
            exc.obj = Some(r);
        }
    }
    state_mut(vm)?.refs.pending = Some(exc);
    Ok(())
}

fn require_object_array_component(vm: &Engine, r: ObjRef) -> Result<(usize, String)> {
    let (elems, kind) = vm.heap.as_array_elems(r).map_err(ndk)?;
    let desc = vm.heap.class_of(r).map_err(ndk)?;
    if kind != ElemKind::Obj {
        return Err(ndk(format!("expected object array, got {desc}")));
    }
    let comp = desc
        .strip_prefix('[')
        .filter(|c| c.starts_with('L') || c.starts_with('['))
        .ok_or_else(|| ndk(format!("expected object array descriptor, got {desc}")))?;
    Ok((elems.len(), comp.to_string()))
}

fn require_primitive_array_len(vm: &Engine, r: ObjRef, k: char) -> Result<usize> {
    let (elems, kind) = vm.heap.as_array_elems(r).map_err(ndk)?;
    let desc = vm.heap.class_of(r).map_err(ndk)?;
    let want_desc = format!("[{k}");
    let want_kind = ElemKind::from_array_desc(&want_desc)
        .ok_or_else(|| ndk("invalid primitive array descriptor"))?;
    if kind != want_kind || desc != want_desc {
        return Err(ndk(format!(
            "primitive array type mismatch: expected {want_desc}, got {desc}"
        )));
    }
    Ok(elems.len())
}

fn run(vm: &mut Engine, address: u64, args: &[Argument]) -> Result<Return> {
    let mut rt = state_mut(vm)?
        .runtime
        .take()
        .ok_or_else(|| ndk("native runtime unavailable for call"))?;
    let scope_depth = vm.heap.gc_scope_depth();
    let pushed_sentinel_frame = vm.gc_frames.is_empty();
    if pushed_sentinel_frame {
        vm.gc_frames.push(Vec::new());
    }
    // Native execution tracks every live reference explicitly in `TrackedRefs`,
    // `buffers`, and `extra_roots`, so `DeleteLocalRef` / `PopLocalFrame` can
    // release garbage during long native loops and `JNI_OnLoad`.
    vm.heap.gc_guest_enter();
    let fuel = vm.fuel_remaining().min(5_000_000);
    let call_res = rt.call(address, args, fuel, |rt, t| bridge(vm, rt, t));
    vm.heap.gc_scope_restore(scope_depth);
    if pushed_sentinel_frame {
        vm.gc_frames.pop();
    }
    if let Some(s) = vm.native.as_mut() {
        s.cached.sync_from(&rt);
        s.runtime = Some(rt);
    }
    call_res
}

fn host_jni_package_supported(desc: &str) -> bool {
    desc.starts_with('[')
        || crate::classpath::host_class_known(desc)
        || [
            "Ljava/",
            "Ljavax/",
            "Lsun/",
            "Ljdk/",
            "Ldalvik/",
            "Landroid/",
            "Landroidx/",
            "Lcom/android/",
            "Lcom/google/android/gms/",
            "Lcom/google/firebase/",
            "Lcom/google/common/util/concurrent/",
            "Lorg/json/",
            "Lorg/xml/",
            "Lorg/xmlpull/",
            "Lorg/w3c/",
            "Lkotlin/",
            "Lkotlinx/",
            "Lokhttp3/",
            "Lokio/",
        ]
        .iter()
        .any(|p| desc.starts_with(p))
}

fn is_object_base_method(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("equals", "(Ljava/lang/Object;)Z")
            | ("hashCode", "()I")
            | ("toString", "()Ljava/lang/String;")
            | ("getClass", "()Ljava/lang/Class;")
            | ("clone", "()Ljava/lang/Object;")
            | ("notify", "()V")
            | ("notifyAll", "()V")
            | ("wait", "()V")
            | ("wait", "(J)V")
            | ("wait", "(JI)V")
    )
}

fn can_resolve_host_jni_method(
    cp: &crate::classpath::Classpath,
    class: &str,
    name: &str,
    sig: &str,
    is_static: bool,
) -> bool {
    if !is_static && is_object_base_method(name, sig) {
        return true;
    }
    if host_jni_package_supported(class) {
        return true;
    }
    if !is_static && name != "<init>" {
        let mut cur = cp.superclass_of(class);
        let mut hops = 0usize;
        while let Some(anc) = cur {
            if anc != "Ljava/lang/Object;" && host_jni_package_supported(&anc) {
                return true;
            }
            hops += 1;
            if hops > 32 {
                break;
            }
            cur = cp.superclass_of(&anc);
        }
    }
    false
}

fn default_for_desc(ty: &str) -> Value {
    match ty.as_bytes().first() {
        Some(b'J') => Value::Long(0),
        Some(b'F') => Value::Float(0.0),
        Some(b'D') => Value::Double(0.0),
        Some(b'L' | b'[') => Value::Null,
        _ => Value::Int(0),
    }
}

fn jni_read_static_field(
    vm: &mut Engine,
    class: &str,
    fname: &str,
    ftype: &str,
) -> std::result::Result<Value, VmExit> {
    let owner = match vm.resolve_static_owner(class, fname) {
        Ok(o) => o,
        Err(_) if host_jni_package_supported(class) => class.to_string(),
        Err(e) => return Err(e),
    };
    vm.ensure_initialized(&owner)?;
    if owner == "Ljava/lang/System;" && (fname == "out" || fname == "err") {
        let key = (owner.clone(), fname.to_string());
        if !vm.statics.contains_key(&key) {
            let fd = if fname == "err" { 2 } else { 1 };
            let r = vm
                .heap
                .alloc_instance(
                    "Ljava/io/PrintStream;".to_string(),
                    vec![("fd".to_string(), Value::Int(fd))],
                )
                .map_err(|e| {
                    VmExit::Exception(Throwable::new(
                        "Ljava/lang/OutOfMemoryError;",
                        format!("{e:?}"),
                    ))
                })?;
            vm.statics.insert(key, Value::Obj(r));
        }
    }
    if owner == "Ljava/util/concurrent/TimeUnit;"
        && !vm.statics.contains_key(&(owner.clone(), fname.to_string()))
        && matches!(
            fname,
            "NANOSECONDS"
                | "MICROSECONDS"
                | "MILLISECONDS"
                | "SECONDS"
                | "MINUTES"
                | "HOURS"
                | "DAYS"
        )
    {
        let unit = vm.time_unit_obj(fname)?;
        vm.statics
            .insert((owner.clone(), fname.to_string()), Value::Obj(unit));
    }
    let key = (owner.clone(), fname.to_string());
    if !vm.statics.contains_key(&key) {
        let host_val = if let Some(c) = crate::boxes::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::jnormalizer::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::jbigdecimal::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::jtext_format::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::android_colorspace::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::android_env::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::android_build::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::graphics::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::parcel::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::android_util::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::platform_base::host_static(vm, &owner, fname)? {
            Some(c)
        } else if let Some(c) = crate::collections::host_static(vm, &owner, fname)? {
            Some(c)
        } else {
            crate::jtime::host_static(vm, &owner, fname)?
        };
        if let Some(c) = host_val {
            vm.statics.insert(key.clone(), c);
        }
        #[cfg(feature = "virtual-devices")]
        if !vm.statics.contains_key(&key) {
            if let Some(c) = crate::m10::host_static(vm, &owner, fname)? {
                vm.statics.insert(key.clone(), c);
            }
        }
    }
    Ok(vm
        .statics
        .get(&key)
        .cloned()
        .unwrap_or_else(|| default_for_desc(ftype)))
}

fn bridge(vm: &mut Engine, rt: &mut Runtime, t: &Trap) -> Result<Return> {
    let a = |i: usize| t.args.get(i).copied().unwrap_or(0);
    let text = |rt: &mut Runtime, p: u64| -> Result<String> {
        rd_jni::decode_modified_utf8(&rt.c_string(p)?).map_err(jerr)
    };
    let bits = match t.slot {
        4 => 0x00010006, // GetVersion
        6 => {
            let n = text(rt, a(1))?;
            let desc = if n.starts_with('[') || n.starts_with('L') && n.ends_with(';') {
                n
            } else {
                format!("L{n};")
            };
            state_mut(vm)?
                .refs
                .insert(Handle::Class(desc))
                .map_err(jerr)?
        }
        10 => {
            let c = {
                let s = state_ref(vm)?;
                hclass(vm, &s.refs, a(1))?
            };
            let p = vm.cp.superclass_of(&c);
            match p {
                Some(p) => state_mut(vm)?.refs.insert(Handle::Class(p)).map_err(jerr)?,
                None => 0,
            }
        }
        11 => {
            let (c, d) = {
                let s = state_ref(vm)?;
                (hclass(vm, &s.refs, a(1))?, hclass(vm, &s.refs, a(2))?)
            };
            vm.cp.is_subtype(&c, &d) as u64
        }
        13 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("Throw: null jthrowable".into()),
                )?;
                (-1i64) as u64
            } else {
                let r = hobj(&state_ref(vm)?.refs, a(1))?;
                if r == 0 {
                    set_jni_pending(
                        vm,
                        "Ljava/lang/NullPointerException;",
                        Some("Throw: null jthrowable".into()),
                    )?;
                    (-1i64) as u64
                } else {
                    let c = vm.heap.class_of(r).map_err(ndk)?.to_string();
                    if !crate::intrinsics::is_throwable_class(vm, &c) {
                        return Err(ndk(format!("Throw: {c} is not a Throwable")));
                    }
                    let mut msg = None;
                    for fname in ["message", "detailMessage"] {
                        if let Ok(Value::Obj(s)) = vm.heap.get_field(r, fname) {
                            if s != 0 {
                                if let Ok(txt) = vm.heap.as_str(s) {
                                    msg = Some(txt.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    state_mut(vm)?.refs.pending = Some(Throwable {
                        class: c,
                        message: msg,
                        obj: Some(r),
                    });
                    0
                }
            }
        }
        14 => {
            let c = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            let msg = if a(2) == 0 {
                None
            } else {
                Some(text(rt, a(2))?)
            };
            set_jni_pending(vm, &c, msg)?;
            0
        }
        15 => {
            let pending = state_ref(vm)?.refs.pending.clone();
            if let Some(mut t) = pending {
                let r = match t.obj {
                    Some(existing) if existing != 0 => existing,
                    _ => {
                        let fields = jni_default_instance_fields(vm, &t.class);
                        let created = vm
                            .heap
                            .alloc_instance(t.class.clone(), fields)
                            .map_err(ndk)?;
                        if let Some(m) = &t.message {
                            let ms = vm.heap.alloc_string(m.clone()).map_err(ndk)?;
                            let _ = vm.heap.put_field(created, "message", Value::Obj(ms));
                            let _ = vm.heap.put_field(created, "detailMessage", Value::Obj(ms));
                        }
                        t.obj = Some(created);
                        state_mut(vm)?.refs.pending = Some(t);
                        created
                    }
                };
                object(&mut state_mut(vm)?.refs, r)?
            } else {
                0
            }
        }
        16 => 0, // ExceptionDescribe only writes stderr on Android; no state change.
        17 => {
            state_mut(vm)?.refs.pending = None;
            0
        }
        19 => {
            state_mut(vm)?.refs.push().map_err(jerr)?;
            0
        }
        20 => state_mut(vm)?.refs.pop(a(1)).map_err(jerr)?,
        21 => state_mut(vm)?.refs.duplicate(a(1), true).map_err(jerr)?,
        22 => {
            state_mut(vm)?.refs.delete_global(a(1));
            0
        }
        23 => {
            state_mut(vm)?.refs.delete_local(a(1));
            0
        }
        24 => state_ref(vm)?.refs.same(a(1), a(2)).map_err(jerr)? as u64,
        25 => state_mut(vm)?.refs.duplicate(a(1), false).map_err(jerr)?,
        26 => {
            let cap = a(1) as i32;
            if cap < 0 || (cap as usize) > References::MAX {
                set_jni_pending(
                    vm,
                    "Ljava/lang/OutOfMemoryError;",
                    Some(format!("EnsureLocalCapacity({cap})")),
                )?;
                (-1i64) as u64
            } else {
                0
            }
        }
        27 => {
            let c = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            match vm.ensure_initialized(&c) {
                Ok(()) => {}
                Err(VmExit::Exception(e)) => {
                    record_vm_exception(vm, e)?;
                    return Ok(Return::default());
                }
                Err(e) => return Err(ndk(format!("AllocObject init failed: {e:?}"))),
            }
            let defaults = jni_default_instance_fields(vm, &c);
            let r = vm.heap.alloc_instance(c.clone(), defaults).map_err(ndk)?;
            match vm.attach_host_state(&c, r) {
                Ok(()) => {}
                Err(VmExit::Exception(e)) => {
                    record_vm_exception(vm, e)?;
                    return Ok(Return::default());
                }
                Err(e) => return Err(ndk(format!("AllocObject host init failed: {e:?}"))),
            }
            object(&mut state_mut(vm)?.refs, r)?
        }
        31 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            let c = vm.heap.class_of(r).map_err(ndk)?.to_string();
            state_mut(vm)?.refs.insert(Handle::Class(c)).map_err(jerr)?
        }
        32 => {
            let h1 = a(1);
            if h1 == 0 || state_ref(vm)?.refs.is_cleared_weak(h1) {
                1
            } else {
                let (r, c) = {
                    let s = state_ref(vm)?;
                    (hobj(&s.refs, h1)?, hclass(vm, &s.refs, a(2))?)
                };
                if r == 0 {
                    1
                } else {
                    vm.cp.is_subtype(vm.heap.class_of(r).map_err(ndk)?, &c) as u64
                }
            }
        }
        33 | 113 => {
            let class = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            let name = text(rt, a(2))?;
            let sig = text(rt, a(3))?;
            let want_static = t.slot == 113;
            rd_jni::descriptor(&sig).map_err(jerr)?;
            let dex_ok = match vm.cp.resolve_method(&class, &name, &sig) {
                Some((_, _, m)) => ((m.access_flags & 0x0008) != 0) == want_static,
                None => false,
            };
            let ok =
                dex_ok || can_resolve_host_jni_method(&vm.cp, &class, &name, &sig, want_static);
            if !ok {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NoSuchMethodError;",
                    Some(format!("{class}->{name}{sig}")),
                )?;
                0
            } else {
                state_mut(vm)?
                    .refs
                    .insert(Handle::Method {
                        class,
                        name,
                        sig,
                        is_static: want_static,
                    })
                    .map_err(jerr)?
            }
        }
        94 | 144 => {
            let class = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            let name = text(rt, a(2))?;
            let sig = text(rt, a(3))?;
            let want_static = t.slot == 144;
            if want_static {
                match vm.ensure_initialized(&class) {
                    Ok(()) => {}
                    Err(VmExit::Exception(e)) => {
                        record_vm_exception(vm, e)?;
                        return Ok(Return::default());
                    }
                    Err(e) => return Err(ndk(format!("GetStaticFieldID init failed: {e:?}"))),
                }
            }
            state_mut(vm)?
                .refs
                .insert(Handle::Field {
                    class,
                    name,
                    sig,
                    is_static: want_static,
                })
                .map_err(jerr)?
        }
        95..=103 | 145..=153 => {
            let Handle::Field {
                class,
                name,
                sig,
                is_static,
            } = state_ref(vm)?.refs.get(a(2)).map_err(jerr)?.clone()
            else {
                return Err(ndk("expected jfieldID"));
            };
            let v = if is_static {
                match jni_read_static_field(vm, &class, &name, &sig) {
                    Ok(v) => v,
                    Err(VmExit::Exception(e)) => {
                        record_vm_exception(vm, e)?;
                        return Ok(Return::default());
                    }
                    Err(e) => return Err(ndk(format!("GetStaticField failed: {e:?}"))),
                }
            } else {
                if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                    set_jni_pending(
                        vm,
                        "Ljava/lang/NullPointerException;",
                        Some(format!("GetField '{name}' on null object")),
                    )?;
                    return Ok(Return::default());
                }
                let r = hobj(&state_ref(vm)?.refs, a(1))?;
                if r == 0 {
                    set_jni_pending(
                        vm,
                        "Ljava/lang/NullPointerException;",
                        Some(format!("GetField '{name}' on null object")),
                    )?;
                    return Ok(Return::default());
                }
                let _ = vm.heap.class_of(r).map_err(ndk)?;
                vm.heap
                    .get_field(r, &name)
                    .unwrap_or_else(|_| jni_default_for_desc(&sig))
            };
            let bits = to_native(&mut state_mut(vm)?.refs, &v)?;
            return Ok(Return {
                bits,
                fp_bits: if matches!(v, Value::Float(_) | Value::Double(_)) {
                    Some(bits)
                } else {
                    None
                },
            });
        }
        104..=112 | 154..=162 => {
            let Handle::Field {
                class,
                name,
                sig,
                is_static,
            } = state_ref(vm)?.refs.get(a(2)).map_err(jerr)?.clone()
            else {
                return Err(ndk("expected jfieldID"));
            };
            let b = if rt.abi == Abi::Arm && (sig == "J" || sig == "D") {
                let mut i = 3;
                arm_wide(rt, t, &mut i)?
            } else if (sig == "F" || sig == "D") && rt.abi != Abi::Arm {
                t.fp.first().copied().unwrap_or(a(3))
            } else {
                rt.raw_argument(t, 3)?
            };
            let v = from_native(vm, b, &sig, rt.abi == Abi::Arm)?;
            if is_static {
                let owner = vm.resolve_static_owner(&class, &name).unwrap_or(class);
                vm.statics.insert((owner, name), v);
            } else {
                let r = hobj(&state_ref(vm)?.refs, a(1))?;
                vm.heap
                    .put_field(r, &name, v)
                    .map_err(|e| ndk(format!("{e:?}")))?;
            }
            0
        }
        163 => {
            let n_i32 = a(2) as i32;
            if !(0..=1024 * 1024).contains(&n_i32) {
                return Err(ndk("NewString size exceeds budget"));
            }
            let n = n_i32 as usize;
            let b = rt.read(a(1), n * 2)?;
            let u: Vec<u16> = b
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16_lossy(&u);
            let r = vm.heap.alloc_string(s).map_err(ndk)?;
            object(&mut state_mut(vm)?.refs, r)?
        }
        164 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            vm.heap.as_str(r).map_err(ndk)?.encode_utf16().count() as u64
        }
        165 | 224 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            let s = vm.heap.as_str(r).map_err(ndk)?;
            let mut b = Vec::new();
            for c in s.encode_utf16() {
                b.extend(c.to_le_bytes());
            }
            b.extend([0, 0]);
            let p = rt.alloc_bytes(&b)?;
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        166 | 170 | 225 => 0, // returned string copies live for the bounded native arena.
        167 => {
            let s = text(rt, a(1))?;
            let r = vm.heap.alloc_string(s).map_err(ndk)?;
            object(&mut state_mut(vm)?.refs, r)?
        }
        168 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            rd_jni::encode_modified_utf8(vm.heap.as_str(r).map_err(ndk)?).len() as u64
        }
        169 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            let mut b = rd_jni::encode_modified_utf8(vm.heap.as_str(r).map_err(ndk)?);
            b.push(0);
            let p = rt.alloc_bytes(&b)?;
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        171 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            vm.heap.as_array_elems(r).map_err(ndk)?.0.len() as u64
        }
        172 => {
            let n = a(1) as i32;
            if n < 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/OutOfMemoryError;",
                    Some(format!("NewObjectArray negative size {n}")),
                )?;
                return Ok(Return::default());
            }
            if n > 1_000_000 {
                return Err(ndk("object array length exceeds budget"));
            }
            let c = hclass(vm, &state_ref(vm)?.refs, a(2))?;
            let init = from_native(vm, a(3), &c, rt.abi == Abi::Arm)?;
            if let Value::Obj(ir) = init {
                if ir != 0 {
                    let icls = vm.heap.class_of(ir).map_err(ndk)?;
                    if !vm.cp.is_subtype(icls, &c) {
                        set_jni_pending(
                            vm,
                            "Ljava/lang/ArrayStoreException;",
                            Some(format!("{icls} cannot be stored in [{c}")),
                        )?;
                        return Ok(Return::default());
                    }
                }
            }
            let arr_desc = format!("[{c}");
            let r = vm
                .heap
                .alloc_array(ElemKind::Obj, n as usize, &arr_desc)
                .map_err(ndk)?;
            vm.heap
                .array_elems_mut(r)
                .map_err(ndk)?
                .resize(n as usize, init);
            object(&mut state_mut(vm)?.refs, r)?
        }
        173 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetObjectArrayElement on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetObjectArrayElement on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let (len, _) = require_object_array_component(vm, r)?;
            let idx = a(2) as i32;
            if idx < 0 || (idx as usize) >= len {
                set_jni_pending(
                    vm,
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    Some(format!("index={idx} length={len}")),
                )?;
                return Ok(Return::default());
            }
            let v = vm.heap.as_array_elems(r).map_err(ndk)?.0[idx as usize].clone();
            to_native(&mut state_mut(vm)?.refs, &v)?
        }
        174 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("SetObjectArrayElement on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let (r, v) = {
                let s = state_ref(vm)?;
                let r = hobj(&s.refs, a(1))?;
                let v = if a(3) == 0 || s.refs.is_cleared_weak(a(3)) {
                    Value::Null
                } else {
                    let obj = hobj(&s.refs, a(3))?;
                    if obj == 0 {
                        Value::Null
                    } else {
                        Value::Obj(obj)
                    }
                };
                (r, v)
            };
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("SetObjectArrayElement on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let (len, comp) = require_object_array_component(vm, r)?;
            let idx = a(2) as i32;
            if idx < 0 || (idx as usize) >= len {
                set_jni_pending(
                    vm,
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    Some(format!("index={idx} length={len}")),
                )?;
                return Ok(Return::default());
            }
            if let Value::Obj(val_r) = v {
                let val_cls = vm.heap.class_of(val_r).map_err(ndk)?.to_string();
                if !vm.cp.is_subtype(&val_cls, &comp) {
                    set_jni_pending(
                        vm,
                        "Ljava/lang/ArrayStoreException;",
                        Some(format!("{val_cls} cannot be stored in [{comp}")),
                    )?;
                    return Ok(Return::default());
                }
            }
            let elems = vm.heap.array_elems_mut(r).map_err(ndk)?;
            elems[idx as usize] = v;
            0
        }
        175..=182 => {
            let k = primitive(t.slot - 175);
            let n = a(1) as i32;
            if n < 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/OutOfMemoryError;",
                    Some(format!("New{k}Array negative size {n}")),
                )?;
                return Ok(Return::default());
            }
            if n > 1_000_000 {
                return Err(ndk("primitive array length exceeds budget"));
            }
            let desc = format!("[{k}");
            let kind = ElemKind::from_array_desc(&desc)
                .ok_or_else(|| ndk("invalid primitive array descriptor"))?;
            let r = vm.heap.alloc_array(kind, n as usize, &desc).map_err(ndk)?;
            let zero = match k {
                'J' => Value::Long(0),
                'F' => Value::Float(0.0),
                'D' => Value::Double(0.0),
                _ => Value::Int(0),
            };
            vm.heap
                .array_elems_mut(r)
                .map_err(ndk)?
                .resize(n as usize, zero);
            object(&mut state_mut(vm)?.refs, r)?
        }
        183..=190 => {
            let k = primitive(t.slot - 183);
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            let b = array_bytes(vm, r, k, 0, usize::MAX)?;
            let p = rt.alloc_bytes(&b)?;
            state_mut(vm)?.buffers.insert(p, (r, k));
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        191..=198 => {
            let (r, k, arg_r) = {
                let s = state_ref(vm)?;
                let (r, k) = s
                    .buffers
                    .get(&a(2))
                    .copied()
                    .ok_or_else(|| ndk("array release pointer was not acquired"))?;
                (r, k, hobj(&s.refs, a(1))?)
            };
            if r != arg_r {
                return Err(ndk("array release reference mismatch"));
            }
            if a(3) != 2 {
                let n = vm.heap.as_array_elems(r).map_err(ndk)?.0.len();
                let byte_len = n
                    .checked_mul(width(k))
                    .ok_or_else(|| mem_err("array byte length overflow"))?;
                let b = rt.read(a(2), byte_len)?;
                write_array(vm, r, k, 0, &b)?;
            }
            if a(3) != 1 {
                state_mut(vm)?.buffers.remove(&a(2));
            }
            0
        }
        199..=206 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("Get<Type>ArrayRegion on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let k = primitive(t.slot - 199);
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("Get<Type>ArrayRegion on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let arr_len = require_primitive_array_len(vm, r, k)?;
            let start = a(2) as i32;
            let len = a(3) as i32;
            let valid = start >= 0
                && len >= 0
                && (start as usize)
                    .checked_add(len as usize)
                    .is_some_and(|end| end <= arr_len);
            if !valid {
                set_jni_pending(
                    vm,
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    Some(format!("start={start} len={len} arrayLen={arr_len}")),
                )?;
                return Ok(Return::default());
            }
            if len > 0 {
                let b = array_bytes(vm, r, k, start as usize, len as usize)?;
                let p = rt.raw_argument(t, 4)?;
                rt.write(p, &b)?;
            }
            0
        }
        207..=214 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("Set<Type>ArrayRegion on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let k = primitive(t.slot - 207);
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("Set<Type>ArrayRegion on null array".into()),
                )?;
                return Ok(Return::default());
            }
            let arr_len = require_primitive_array_len(vm, r, k)?;
            let start = a(2) as i32;
            let len = a(3) as i32;
            let valid = start >= 0
                && len >= 0
                && (start as usize)
                    .checked_add(len as usize)
                    .is_some_and(|end| end <= arr_len);
            if !valid {
                set_jni_pending(
                    vm,
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    Some(format!("start={start} len={len} arrayLen={arr_len}")),
                )?;
                return Ok(Return::default());
            }
            let n = len as usize;
            if n > 1_000_000 {
                return Err(ndk("array region exceeds budget"));
            }
            if n > 0 {
                let p = rt.raw_argument(t, 4)?;
                let byte_len = n
                    .checked_mul(width(k))
                    .ok_or_else(|| mem_err("array region byte length overflow"))?;
                let b = rt.read(p, byte_len)?;
                write_array(vm, r, k, start as usize, &b)?;
            }
            0
        }
        215 => {
            let class = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            let n = a(3) as i32;
            if !(0..=4096).contains(&n) {
                return Err(ndk("RegisterNatives count invalid"));
            }
            let w = rt.abi.pointer_size() as u64;
            let stride = w
                .checked_mul(3)
                .ok_or_else(|| mem_err("RegisterNatives stride overflow"))?;
            for i in 0..n as u64 {
                let off = i
                    .checked_mul(stride)
                    .ok_or_else(|| mem_err("RegisterNatives offset overflow"))?;
                let p = a(2)
                    .checked_add(off)
                    .ok_or_else(|| mem_err("RegisterNatives pointer overflow"))?;
                let p_sig = p
                    .checked_add(w)
                    .ok_or_else(|| mem_err("RegisterNatives signature pointer overflow"))?;
                let p_fn = p_sig
                    .checked_add(w)
                    .ok_or_else(|| mem_err("RegisterNatives fn pointer overflow"))?;
                let _ = p_fn
                    .checked_add(w)
                    .ok_or_else(|| mem_err("RegisterNatives struct end pointer overflow"))?;
                let np = rt.read_word(p)?;
                let sp = rt.read_word(p_sig)?;
                let func = rt.read_word(p_fn)?;
                let name = text(rt, np)?;
                let sig = text(rt, sp)?;
                rd_jni::descriptor(&sig).map_err(jerr)?;
                if vm.cp.resolve_method(&class, &name, &sig).is_none() {
                    return Err(ndk(format!(
                        "RegisterNatives method absent {class}->{name}{sig}"
                    )));
                }
                state_mut(vm)?
                    .registered
                    .insert((class.clone(), name, sig), func);
            }
            0
        }
        216 => {
            let class = hclass(vm, &state_ref(vm)?.refs, a(1))?;
            state_mut(vm)?.registered.retain(|(c, _, _), _| c != &class);
            0
        }
        // MonitorEnter/MonitorExit: the VM is single-threaded, so a monitor is
        // always uncontended (JNI_OK).
        217 | 218 => 0,
        219 => {
            rt.write_word(a(1), JAVA_VM)?;
            0
        }
        // GetStringRegion(env, str, start, len, jchar* buf)
        220 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetStringRegion on null string".into()),
                )?;
                return Ok(Return::default());
            }
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetStringRegion on null string".into()),
                )?;
                return Ok(Return::default());
            }
            let u: Vec<u16> = vm.heap.as_str(r).map_err(ndk)?.encode_utf16().collect();
            let start = a(2) as i32;
            let len = a(3) as i32;
            let end = if start >= 0 && len >= 0 {
                (start as usize)
                    .checked_add(len as usize)
                    .filter(|e| *e <= u.len())
            } else {
                None
            };
            let Some(end) = end else {
                set_jni_pending(
                    vm,
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    Some(format!("start={start} len={len} length={}", u.len())),
                )?;
                return Ok(Return::default());
            };
            if len > 0 {
                let b: Vec<u8> = u[start as usize..end]
                    .iter()
                    .flat_map(|c| c.to_le_bytes())
                    .collect();
                let p = rt.raw_argument(t, 4)?;
                rt.write(p, &b)?;
            }
            0
        }
        // GetStringUTFRegion(env, str, start, len, char* buf): `start`/`len`
        // count UTF-16 units; the output is modified UTF-8, NUL-terminated.
        221 => {
            if a(1) == 0 || state_ref(vm)?.refs.is_cleared_weak(a(1)) {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetStringUTFRegion on null string".into()),
                )?;
                return Ok(Return::default());
            }
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            if r == 0 {
                set_jni_pending(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    Some("GetStringUTFRegion on null string".into()),
                )?;
                return Ok(Return::default());
            }
            let u: Vec<u16> = vm.heap.as_str(r).map_err(ndk)?.encode_utf16().collect();
            let start = a(2) as i32;
            let len = a(3) as i32;
            let end = if start >= 0 && len >= 0 {
                (start as usize)
                    .checked_add(len as usize)
                    .filter(|e| *e <= u.len())
            } else {
                None
            };
            let Some(end) = end else {
                set_jni_pending(
                    vm,
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    Some(format!("start={start} len={len} length={}", u.len())),
                )?;
                return Ok(Return::default());
            };
            let sub = String::from_utf16_lossy(&u[start as usize..end]);
            let mut b = rd_jni::encode_modified_utf8(&sub);
            b.push(0);
            let p = rt.raw_argument(t, 4)?;
            rt.write(p, &b)?;
            0
        }
        // GetPrimitiveArrayCritical: a bounded copy like Get<T>ArrayElements;
        // the element width comes from the runtime array descriptor.
        222 => {
            let r = hobj(&state_ref(vm)?.refs, a(1))?;
            let desc = vm.heap.class_of(r).map_err(ndk)?;
            let k = desc
                .strip_prefix('[')
                .and_then(|d| d.chars().next())
                .filter(|c| "ZBCSIJFD".contains(*c) && desc.len() == 2)
                .ok_or_else(|| ndk("GetPrimitiveArrayCritical on a non-primitive array"))?;
            let b = array_bytes(vm, r, k, 0, usize::MAX)?;
            let p = rt.alloc_bytes(&b)?;
            state_mut(vm)?.buffers.insert(p, (r, k));
            if a(2) != 0 {
                rt.write(a(2), &[1])?;
            }
            p
        }
        // ReleasePrimitiveArrayCritical(env, array, ptr, mode)
        223 => {
            let (r, k, arg_r) = {
                let s = state_ref(vm)?;
                let (r, k) = s
                    .buffers
                    .get(&a(2))
                    .copied()
                    .ok_or_else(|| ndk("critical release pointer was not acquired"))?;
                (r, k, hobj(&s.refs, a(1))?)
            };
            if r != arg_r {
                return Err(ndk("critical release reference mismatch"));
            }
            if a(3) != 2 {
                let n = vm.heap.as_array_elems(r).map_err(ndk)?.0.len();
                let byte_len = n
                    .checked_mul(width(k))
                    .ok_or_else(|| mem_err("critical array byte length overflow"))?;
                let b = rt.read(a(2), byte_len)?;
                write_array(vm, r, k, 0, &b)?;
            }
            if a(3) != 1 {
                state_mut(vm)?.buffers.remove(&a(2));
            }
            0
        }
        // NewWeakGlobalRef / DeleteWeakGlobalRef: tracked weakly and cleared on GC sweep.
        226 => state_mut(vm)?.refs.new_weak_global(a(1)).map_err(jerr)?,
        227 => {
            state_mut(vm)?.refs.delete_weak_global(a(1));
            0
        }
        228 => state_ref(vm)?.refs.pending.is_some() as u64,
        232 => state_ref(vm)?.refs.ref_type(a(1)),
        304 | 306 | 307 => {
            rt.write_word(a(1), ENV)?;
            0
        }
        305 => 0,
        34..=93 | 114..=143 | 28..=30 => return call_java(vm, rt, t),
        _ => {
            return Err(RdError::new(
                "NOT_IMPLEMENTED",
                format!("JNI function table slot {}", t.slot),
            ))
        }
    };
    Ok(Return {
        bits,
        fp_bits: None,
    })
}

/// ARM (AAPCS) 64-bit argument at raw index `i`: an even-aligned pair.
fn arm_wide(rt: &mut Runtime, t: &Trap, i: &mut usize) -> Result<u64> {
    *i += *i & 1;
    let lo = rt.raw_argument(t, *i)? & 0xffff_ffff;
    let hi = rt.raw_argument(t, *i + 1)? & 0xffff_ffff;
    *i += 2;
    Ok(lo | (hi << 32))
}
fn primitive(i: usize) -> char {
    ['Z', 'B', 'C', 'S', 'I', 'J', 'F', 'D'][i]
}
fn width(k: char) -> usize {
    match k {
        'Z' | 'B' => 1,
        'C' | 'S' => 2,
        'I' | 'F' => 4,
        _ => 8,
    }
}
fn array_bytes(vm: &Engine, r: ObjRef, k: char, start: usize, n: usize) -> Result<Vec<u8>> {
    let arr_len = require_primitive_array_len(vm, r, k)?;
    let (elems, _) = vm.heap.as_array_elems(r).map_err(ndk)?;
    let end = if n == usize::MAX {
        arr_len
    } else {
        start
            .checked_add(n)
            .ok_or_else(|| ndk("array range overflow"))?
    };
    let slice = elems
        .get(start..end)
        .ok_or_else(|| ndk("array range out of bounds"))?;
    let mut b = Vec::with_capacity(slice.len() * width(k));
    for v in slice {
        let raw = match v {
            Value::Int(v) => *v as u64,
            Value::Long(v) => *v as u64,
            Value::Float(v) => v.to_bits() as u64,
            Value::Double(v) => v.to_bits(),
            _ => return Err(ndk("not a primitive array")),
        };
        b.extend(&raw.to_le_bytes()[..width(k)]);
    }
    Ok(b)
}
fn write_array(vm: &mut Engine, r: ObjRef, k: char, start: usize, b: &[u8]) -> Result<()> {
    let _ = require_primitive_array_len(vm, r, k)?;
    let elems = vm.heap.array_elems_mut(r).map_err(ndk)?;
    let n = b.len() / width(k);
    let end = start
        .checked_add(n)
        .ok_or_else(|| ndk("array range overflow"))?;
    let slice = elems
        .get_mut(start..end)
        .ok_or_else(|| ndk("array range out of bounds"))?;
    for (v, b) in slice.iter_mut().zip(b.chunks_exact(width(k))) {
        let raw = b
            .iter()
            .enumerate()
            .fold(0u64, |v, (i, c)| v | (u64::from(*c) << (i * 8)));
        *v = match k {
            'J' => Value::Long(raw as i64),
            'F' => Value::Float(f32::from_bits(raw as u32)),
            'D' => Value::Double(f64::from_bits(raw)),
            'Z' => Value::Int(((raw as u8) != 0) as i32),
            'B' => Value::Int(raw as i8 as i32),
            'C' => Value::Int((raw as u16) as i32),
            'S' => Value::Int(raw as i16 as i32),
            _ => Value::Int(raw as i32),
        };
    }
    Ok(())
}

fn jni_inherited_android_host(
    vm: &mut Engine,
    start: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> std::result::Result<Option<Value>, VmExit> {
    let mut ancestor = start.to_string();
    let mut property_fallback_class = None;
    let mut visited = BTreeSet::new();
    for _ in 0..16 {
        if !visited.insert(ancestor.clone()) {
            break;
        }
        if ancestor.starts_with("Landroid/") {
            property_fallback_class.get_or_insert_with(|| ancestor.clone());
            #[cfg(feature = "advanced-components")]
            if let Some(value) = crate::m9::call_instance(vm, &ancestor, name, sig, recv, args)? {
                return Ok(Some(value));
            }
            #[cfg(feature = "virtual-devices")]
            if vm.fw.vdev.virtual_hardware_enabled {
                if let Some(value) =
                    crate::m10::call_instance(vm, &ancestor, name, sig, recv, args)?
                {
                    return Ok(Some(value));
                }
            }
            if let Some(value) = crate::m8::call_m8_instance(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) =
                crate::framework::call_host_instance(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) =
                crate::view_attach::instance_intrinsic(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) = crate::platform_base::instance_intrinsic(&ancestor, name, sig) {
                return Ok(Some(value));
            }
        }
        let next = vm.cp.superclass_of(&ancestor).or_else(|| {
            crate::classpath::builtin_hierarchy(&ancestor).and_then(|chain| {
                chain
                    .iter()
                    .copied()
                    .find(|s| *s != ancestor)
                    .map(str::to_string)
            })
        });
        match next {
            Some(next) => ancestor = next,
            None => break,
        }
    }
    if let Some(class) = property_fallback_class {
        if let Some(value) = crate::view_props::fallback(vm, &class, name, sig, recv, args)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn jni_invoke_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> std::result::Result<Value, VmExit> {
    if let Some(v) = crate::intrinsics::call_native_loader(vm, class, name, sig, args)? {
        return Ok(v);
    }
    #[cfg(feature = "advanced-components")]
    if let Some(v) = crate::m9::call_static(vm, class, name, sig, args)? {
        return Ok(v);
    }
    #[cfg(feature = "virtual-devices")]
    if vm.fw.vdev.virtual_hardware_enabled {
        if let Some(v) = crate::m10::call_static(vm, class, name, sig, args)? {
            return Ok(v);
        }
    }
    if let Some(v) = crate::intrinsics::call_static_intrinsic(vm, class, name, sig, args)? {
        return Ok(v);
    }
    if let Some(v) = crate::m8::call_m8_static(vm, class, name, sig, args)? {
        return Ok(v);
    }
    if let Some(v) = crate::androidx::call_static(vm, class, name, sig, args)? {
        return Ok(v);
    }
    if let Some(v) = crate::gms::call_static(vm, class, name, sig, args)? {
        return Ok(v);
    }
    if class.starts_with("Landroid/") {
        if let Some(v) = crate::framework::call_host_static(vm, class, name, sig, args)? {
            return Ok(v);
        }
    }
    vm.invoke_static(class, name, sig, args)
}

struct JniInstanceCall<'a> {
    mref_class: &'a str,
    name: &'a str,
    sig: &'a str,
    params: &'a [String],
    is_direct: bool,
}

fn jni_invoke_instance(
    vm: &mut Engine,
    recv: ObjRef,
    call: &JniInstanceCall<'_>,
    args: &[Value],
) -> std::result::Result<Value, VmExit> {
    let mref_class = call.mref_class;
    let name = call.name;
    let sig = call.sig;
    let params = call.params;
    let is_direct = call.is_direct;
    if mref_class == "Ljava/lang/Object;" && name == "<init>" && sig == "()V" {
        return Ok(Value::Null);
    }
    let recv_class = vm
        .heap
        .class_of(recv)
        .map_err(|e| crate::err::VmExit::Error(crate::err::vm_error("VM_HEAP", format!("{e:?}"))))?
        .to_string();
    let effective_mref = if is_direct { mref_class } else { &recv_class };

    // For virtual calls (`Call*Method`), a DEX override on the runtime class
    // takes precedence unless the target is handled by a host subsystem that
    // intercepts before DEX (matching `interp::do_invoke`).
    if let Some(v) = crate::intrinsics::call_instance_intrinsic_mref(
        vm,
        &recv_class,
        effective_mref,
        name,
        sig,
        recv,
        args,
    )? {
        return Ok(v);
    }
    if is_direct && effective_mref != mref_class {
        if let Some(v) = crate::intrinsics::call_instance_intrinsic_mref(
            vm,
            &recv_class,
            mref_class,
            name,
            sig,
            recv,
            args,
        )? {
            return Ok(v);
        }
    }
    #[cfg(feature = "advanced-components")]
    if let Some(v) = crate::m9::call_instance(vm, effective_mref, name, sig, recv, args)? {
        return Ok(v);
    }
    #[cfg(feature = "virtual-devices")]
    if vm.fw.vdev.virtual_hardware_enabled {
        if let Some(v) = crate::m10::call_instance(vm, effective_mref, name, sig, recv, args)? {
            return Ok(v);
        }
    }
    if let Some(v) = crate::m8::call_m8_instance(vm, effective_mref, name, sig, recv, args)? {
        return Ok(v);
    }
    if let Some(v) = crate::androidx::call_instance(vm, effective_mref, name, sig, recv, args)? {
        return Ok(v);
    }
    if let Some(v) = crate::gms::call_instance(vm, effective_mref, name, sig, recv, args)? {
        return Ok(v);
    }

    let dispatch_target = if is_direct { mref_class } else { &recv_class };
    if let Some((d, def, m)) = vm.cp.resolve_method(dispatch_target, name, sig) {
        let mut values = vec![Value::Obj(recv)];
        values.extend(vm.slot_form_public(args, params));
        return vm.call(d, def, &m, values);
    }

    let host_start = if is_direct || mref_class.starts_with("Landroid/") {
        mref_class
    } else {
        &recv_class
    };
    if let Some(v) = jni_inherited_android_host(vm, host_start, name, sig, recv, args)? {
        return Ok(v);
    }
    if !is_direct && host_start != recv_class {
        if let Some(v) = jni_inherited_android_host(vm, &recv_class, name, sig, recv, args)? {
            return Ok(v);
        }
    }
    Err(crate::classpath::unresolved_method_on(mref_class, &recv_class, name, sig).into())
}

fn call_java(vm: &mut Engine, rt: &mut Runtime, t: &Trap) -> Result<Return> {
    let ctor = matches!(t.slot, 28..=30);
    let static_call = matches!(t.slot, 114..=143);
    let nonvirtual = matches!(t.slot, 64..=93);
    let id_index = if nonvirtual { 3 } else { 2 };
    let id = rt.raw_argument(t, id_index)?;
    let Handle::Method {
        class,
        name,
        sig,
        is_static,
    } = state_ref(vm)?.refs.get(id).map_err(jerr)?.clone()
    else {
        return Err(ndk("expected jmethodID"));
    };
    if is_static != static_call && !ctor {
        return Err(ndk("JNI static/instance method mismatch"));
    }
    let (params, ret) = rd_jni::descriptor(&sig).map_err(jerr)?;
    let base = if ctor {
        28
    } else if static_call {
        114
    } else if nonvirtual {
        64
    } else {
        34
    };
    let variant = (t.slot - base) % 3;
    // Call*MethodV: decode the guest va_list with the ABI's rules (C++ JNI
    // wrappers in jni.h always go through these entry points).
    let va_words = if variant == 1 {
        let va = rt.raw_argument(t, id_index + 1)?;
        Some(rt.va_list_args(va, &params)?)
    } else {
        None
    };
    let mut args = Vec::new();
    let mut raw_index = id_index + 1;
    let array = if variant == 2 {
        rt.raw_argument(t, raw_index)?
    } else {
        0
    };
    let mut fpi = 0;
    for (i, p) in params.iter().enumerate() {
        let b = if let Some(w) = &va_words {
            w.get(i).copied().unwrap_or(0)
        } else if variant == 2 {
            let elem_off = (i as u64)
                .checked_mul(8)
                .and_then(|d| array.checked_add(d))
                .ok_or_else(|| mem_err("JNI jvalue array pointer overflow"))?;
            let _ = elem_off
                .checked_add(8)
                .ok_or_else(|| mem_err("JNI jvalue element end pointer overflow"))?;
            let b = rt.read(elem_off, 8)?;
            let arr8: [u8; 8] = b.try_into().map_err(|_| ndk("short jvalue read"))?;
            u64::from_le_bytes(arr8)
        } else if rt.abi == Abi::Arm && (p == "J" || p == "D" || p == "F") {
            // AAPCS variadic: 64-bit values (and floats, promoted to double)
            // take an even-aligned register pair / 8-byte stack slot.
            let v = arm_wide(rt, t, &mut raw_index)?;
            if p == "F" {
                (f64::from_bits(v) as f32).to_bits() as u64
            } else {
                v
            }
        } else if (p == "F" || p == "D") && rt.abi != Abi::Arm {
            let v = t.fp.get(fpi).copied().unwrap_or(0);
            fpi += 1;
            if p == "F" {
                (f64::from_bits(v) as f32).to_bits() as u64
            } else {
                v
            }
        } else {
            let v = rt.raw_argument(t, raw_index)?;
            raw_index += 1;
            v
        };
        args.push(from_native(vm, b, p, rt.abi == Abi::Arm)?);
    }

    let result = if static_call {
        jni_invoke_static(vm, &class, &name, &sig, &args)
    } else if ctor {
        match vm.ensure_initialized(&class) {
            Ok(()) => {
                let defaults = jni_default_instance_fields(vm, &class);
                let recv = vm
                    .heap
                    .alloc_instance(class.clone(), defaults)
                    .map_err(ndk)?;
                match vm.attach_host_state(&class, recv) {
                    Ok(()) => {
                        state_mut(vm)?.extra_roots.push(recv);
                        let icall = JniInstanceCall {
                            mref_class: &class,
                            name: &name,
                            sig: &sig,
                            params: &params,
                            is_direct: true,
                        };
                        let v = jni_invoke_instance(vm, recv, &icall, &args);
                        if let Ok(s) = state_mut(vm) {
                            if let Some(pos) = s.extra_roots.iter().rposition(|&x| x == recv) {
                                s.extra_roots.remove(pos);
                            }
                        }
                        v.map(|_| Value::Obj(recv))
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    } else {
        let raw_recv = rt.raw_argument(t, 1)?;
        if raw_recv == 0 || state_ref(vm)?.refs.is_cleared_weak(raw_recv) {
            Err(Throwable::new("Ljava/lang/NullPointerException;", "null JNI receiver").into())
        } else {
            let recv = hobj(&state_ref(vm)?.refs, raw_recv)?;
            if recv == 0 {
                Err(Throwable::new("Ljava/lang/NullPointerException;", "null JNI receiver").into())
            } else {
                let icall = JniInstanceCall {
                    mref_class: &class,
                    name: &name,
                    sig: &sig,
                    params: &params,
                    is_direct: nonvirtual,
                };
                jni_invoke_instance(vm, recv, &icall, &args)
            }
        }
    };

    match result {
        Ok(v) => {
            let b = to_native(&mut state_mut(vm)?.refs, &v)?;
            Ok(Return {
                bits: b,
                fp_bits: if ret == "F" || ret == "D" {
                    Some(b)
                } else {
                    None
                },
            })
        }
        Err(VmExit::Exception(e)) => {
            record_vm_exception(vm, e)?;
            Ok(Return::default())
        }
        Err(e) => Err(ndk(format!("Java callback failed: {e:?}"))),
    }
}

impl Engine {
    pub fn native_report(&self) -> serde_json::Value {
        match &self.native {
            Some(s) => match &s.runtime {
                Some(rt) => serde_json::json!({
                    "enabled": true,
                    "abi": s.abi,
                    "instructions": rt.instructions,
                    "libraries": rt.loaded_libraries(),
                    "arena_bytes": rt.arena_bytes(),
                    "registered_methods": s.registered.len(),
                    "missing_imports": rt.missing_imports,
                    "lazy_imports": rt.lazily_bound_imports().len(),
                    "logs": rt.logs,
                }),
                None => serde_json::json!({
                    "enabled": true,
                    "abi": s.abi,
                    "instructions": s.cached.instructions,
                    "libraries": s.cached.libraries,
                    "arena_bytes": s.cached.arena_bytes,
                    "registered_methods": s.registered.len(),
                    "missing_imports": s.cached.missing_imports,
                    "lazy_imports": s.cached.lazy_imports,
                    "logs": s.cached.logs,
                }),
            },
            None => serde_json::json!({"enabled":true,"libraries":[],"instructions":0}),
        }
    }
}
