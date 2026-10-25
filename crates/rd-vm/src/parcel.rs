//! `android.os.Parcel`, `Parcelable.Creator` materialization and local
//! `android.os.Binder` transactions.
//!
//! Parcel is a real byte buffer, not an object-reference side table:
//! integers, longs, floats, doubles, strings and primitive arrays are
//! marshalled little-endian with the 4-byte alignment Android's native
//! Parcel uses, so `marshall()` byte arrays can be copied, stored, sliced
//! with `appendFrom` and `unmarshall`ed into another Parcel in the same
//! Engine. Strings use Android's UTF-16 form (int char count, UTF-16LE
//! units plus terminator, padded to 4 bytes); null is length -1.
//!
//! Local Binder transport is Engine-local by design: a strong Binder is
//! written as a handle into `HostState.binder_refs`, and `Binder.transact`
//! dispatches to the receiver's DEX `onTransact`. That is deliberately
//! *not* a kernel Binder handle, a cross-process reference, or byte
//! compatibility for `flat_binder_object`; those remain Appendix C gaps
//! for `android.os`. File descriptors, Java-serialized objects and
//! cross-process death recipients are likewise not faked: unsupported
//! Parcel methods fall through to the VM's typed NOT_IMPLEMENTED path.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

pub(crate) const PARCEL: &str = "Landroid/os/Parcel;";
const BINDER: &str = "Landroid/os/Binder;";
const IBINDER: &str = "Landroid/os/IBinder;";
const BUNDLE: &str = "Landroid/os/Bundle;";
const PERSISTABLE_BUNDLE: &str = "Landroid/os/PersistableBundle;";
const SPARSE_ARRAY: &str = "Landroid/util/SparseArray;";
const SPARSE_BOOLEAN_ARRAY: &str = "Landroid/util/SparseBooleanArray;";
const ON_TRANSACT_SIG: &str = "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z";

const INTERFACE_TRANSACTION: i32 = 0x5f4e_5446; // '_NTF'
const DUMP_TRANSACTION: i32 = 0x5f44_4d50; // '_DMP'
/// `BaseBundle.BUNDLE_MAGIC`; as little-endian bytes this is `BNDL`.
const BUNDLE_MAGIC: i32 = 0x4c44_4e42;

/// Heap field holding a Bundle's retained, not-yet-materialized parcelled
/// data: an ObjRef to a hidden host `Parcel` whose buffer is the Bundle
/// frame's ArrayMap payload (frame length and magic excluded). This is the
/// analogue of AOSP `BaseBundle.mParcelledData`; presence of the field is
/// exactly `isParcelled()`.
pub(crate) const PARCELLED_FIELD: &str = "rd$parcelled_data";

// `Parcel` value type tags, exactly as AOSP `Parcel.java` defines them
// (kept in sync with libbinder's ParcelValTypes.h).
const VAL_NULL: i32 = -1;
const VAL_STRING: i32 = 0;
const VAL_INTEGER: i32 = 1;
const VAL_MAP: i32 = 2;
const VAL_BUNDLE: i32 = 3;
const VAL_PARCELABLE: i32 = 4;
const VAL_SHORT: i32 = 5;
const VAL_LONG: i32 = 6;
const VAL_FLOAT: i32 = 7;
const VAL_DOUBLE: i32 = 8;
const VAL_BOOLEAN: i32 = 9;
const VAL_CHARSEQUENCE: i32 = 10;
const VAL_LIST: i32 = 11;
const VAL_SPARSEARRAY: i32 = 12;
const VAL_BYTEARRAY: i32 = 13;
const VAL_STRINGARRAY: i32 = 14;
const VAL_IBINDER: i32 = 15;
const VAL_PARCELABLEARRAY: i32 = 16;
const VAL_OBJECTARRAY: i32 = 17;
const VAL_INTARRAY: i32 = 18;
const VAL_LONGARRAY: i32 = 19;
const VAL_BYTE: i32 = 20;
const VAL_SERIALIZABLE: i32 = 21;
const VAL_SPARSEBOOLEANARRAY: i32 = 22;
const VAL_BOOLEANARRAY: i32 = 23;
const VAL_CHARSEQUENCEARRAY: i32 = 24;
const VAL_PERSISTABLEBUNDLE: i32 = 25;
const VAL_SIZE: i32 = 26;
const VAL_SIZEF: i32 = 27;
const VAL_DOUBLEARRAY: i32 = 28;
const VAL_CHAR: i32 = 29;
const VAL_SHORTARRAY: i32 = 30;
const VAL_CHARARRAY: i32 = 31;
const VAL_FLOATARRAY: i32 = 32;

/// Defensive ceiling on a single host `Parcel` byte buffer (16 MiB, well above
/// Android's ~1 MiB Binder transaction buffer) so `setDataSize` or repeated
/// writes cannot exhaust host memory outside `VmConfig::heap_budget`.
const MAX_PARCEL_BYTES: usize = 16 * 1024 * 1024;

/// Maximum nesting depth for recursive `Parcel` value/container read & write
/// paths so cyclic or adversarially nested graphs throw a typed exception
/// instead of overflowing the host call stack (sized to fit safely inside
/// Rust's 2 MiB default worker-thread stack even in unoptimized debug builds).
const MAX_PARCEL_DEPTH: usize = 32;

/// Upper bound on initial `Vec::with_capacity` reservations before elements
/// are incrementally parsed from the stream.
const MAX_INITIAL_CAPACITY: usize = 4096;

/// Hidden heap field tracking active guest-callback parcel recursion depth.
const DEPTH_FIELD: &str = "rd$parcel_depth";

fn bad(msg: impl Into<String>) -> VmExit {
    Throwable::new("Landroid/os/BadParcelableException;", msg).into()
}
fn npe(msg: impl Into<String>) -> VmExit {
    Throwable::new("Ljava/lang/NullPointerException;", msg).into()
}
fn iae(msg: impl Into<String>) -> VmExit {
    Throwable::new("Ljava/lang/IllegalArgumentException;", msg).into()
}

fn bounded_cap(n: usize) -> usize {
    n.min(MAX_INITIAL_CAPACITY)
}

fn checked_align4(n: usize) -> Result<usize, VmExit> {
    n.checked_add(3)
        .map(|v| v & !3)
        .ok_or_else(|| bad("Parcel: alignment overflow"))
}

fn check_avail_elems(
    st: &PState,
    n: usize,
    min_elem_bytes: usize,
    what: &str,
) -> Result<(), VmExit> {
    let avail = st.data.len().saturating_sub(st.pos);
    let need = n.checked_mul(min_elem_bytes).ok_or_else(|| {
        bad(format!(
            "Parcel under-run reading {what}: count {n} overflows byte size"
        ))
    })?;
    if need > avail {
        return Err(bad(format!(
            "Parcel under-run reading {what}: count {n} needs at least {need} byte(s), avail {avail}"
        )));
    }
    Ok(())
}

fn ensure_parcel_size(target: usize, what: &str) -> Result<(), VmExit> {
    if target > MAX_PARCEL_BYTES {
        return Err(bad(format!(
            "Parcel: {what} size {target} exceeds maximum parcel size {MAX_PARCEL_BYTES}"
        )));
    }
    Ok(())
}

fn current_parcel_depth(vm: &Engine, parcel: ObjRef) -> usize {
    match vm.heap.get_field(parcel, DEPTH_FIELD) {
        Ok(Value::Int(d)) if d > 0 => d as usize,
        _ => 0,
    }
}

fn check_depth(depth: usize) -> Result<usize, VmExit> {
    let next = depth
        .checked_add(1)
        .ok_or_else(|| bad("Parcel: maximum nesting depth exceeded"))?;
    if next > MAX_PARCEL_DEPTH {
        return Err(bad(format!(
            "Parcel: maximum nesting depth ({MAX_PARCEL_DEPTH}) exceeded"
        )));
    }
    Ok(next)
}

#[derive(Debug, Clone, Default)]
struct PState {
    data: Vec<u8>,
    pos: usize,
    objects: Vec<Value>,
}

impl PState {
    fn align_write(&mut self) -> Result<(), VmExit> {
        let a = checked_align4(self.pos)?;
        ensure_parcel_size(a, "aligned write")?;
        if a > self.data.len() {
            self.data.resize(a, 0);
        }
        self.pos = a;
        Ok(())
    }
    fn write_raw(&mut self, bytes: &[u8]) -> Result<(), VmExit> {
        ensure_parcel_size(self.pos, "write cursor")?;
        let end = self
            .pos
            .checked_add(bytes.len())
            .ok_or_else(|| bad("Parcel: write size overflow"))?;
        ensure_parcel_size(end, "write")?;
        if self.pos > self.data.len() {
            self.data.resize(self.pos, 0);
        }
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }
    fn write_padded(&mut self, bytes: &[u8]) -> Result<(), VmExit> {
        self.write_raw(bytes)?;
        let pad = checked_align4(bytes.len())? - bytes.len();
        if pad > 0 {
            self.write_raw(&[0, 0, 0][..pad])?;
        }
        Ok(())
    }
    fn write_i32(&mut self, v: i32) -> Result<(), VmExit> {
        self.align_write()?;
        self.write_raw(&v.to_le_bytes())
    }
    fn write_i64(&mut self, v: i64) -> Result<(), VmExit> {
        self.align_write()?;
        self.write_raw(&v.to_le_bytes())
    }
    fn patch_i32(&mut self, offset: usize, v: i32) {
        if let Some(end) = offset.checked_add(4) {
            if let Some(bytes) = self.data.get_mut(offset..end) {
                bytes.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    fn write_string(&mut self, s: Option<&str>) -> Result<(), VmExit> {
        match s {
            None => self.write_i32(-1),
            Some(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                let len_i32 =
                    i32::try_from(units.len()).map_err(|_| bad("Parcel: string too long"))?;
                let byte_len = units
                    .len()
                    .checked_add(1)
                    .and_then(|u| u.checked_mul(2))
                    .ok_or_else(|| bad("Parcel: string byte length overflow"))?;
                ensure_parcel_size(byte_len, "string")?;
                self.write_i32(len_i32)?;
                let mut bytes = Vec::with_capacity(byte_len);
                for u in &units {
                    bytes.extend_from_slice(&u.to_le_bytes());
                }
                bytes.extend_from_slice(&0u16.to_le_bytes());
                self.write_padded(&bytes)
            }
        }
    }
    fn write_string8(&mut self, s: Option<&str>) -> Result<(), VmExit> {
        match s {
            None => self.write_i32(-1),
            Some(s) => {
                let len_i32 =
                    i32::try_from(s.len()).map_err(|_| bad("Parcel: string8 too long"))?;
                let byte_len = s
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| bad("Parcel: string8 byte length overflow"))?;
                ensure_parcel_size(byte_len, "string8")?;
                self.write_i32(len_i32)?;
                let mut bytes = Vec::with_capacity(byte_len);
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(0);
                self.write_padded(&bytes)
            }
        }
    }
    fn write_byte_blob(&mut self, b: Option<&[u8]>) -> Result<(), VmExit> {
        match b {
            None => self.write_i32(-1),
            Some(b) => {
                let len_i32 =
                    i32::try_from(b.len()).map_err(|_| bad("Parcel: byte array too long"))?;
                ensure_parcel_size(b.len(), "byte array")?;
                self.write_i32(len_i32)?;
                self.write_padded(b)
            }
        }
    }
    fn read_raw(&mut self, n: usize, what: &str) -> Result<Vec<u8>, VmExit> {
        if self.pos > self.data.len() || n > self.data.len() - self.pos {
            return Err(bad(format!(
                "Parcel under-run reading {what}: need {n} byte(s) at {}, size {}",
                self.pos,
                self.data.len()
            )));
        }
        let out = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(out)
    }
    fn read_padded(&mut self, n: usize, what: &str) -> Result<Vec<u8>, VmExit> {
        let aligned = checked_align4(n)?;
        let out = self.read_raw(n, what)?;
        let pad = aligned - n;
        if pad > 0 {
            let next_pos = self
                .pos
                .checked_add(pad)
                .filter(|&end| end <= self.data.len())
                .ok_or_else(|| bad(format!("Parcel under-run reading {what} padding")))?;
            self.pos = next_pos;
        }
        Ok(out)
    }
    fn read_i32(&mut self) -> Result<i32, VmExit> {
        self.pos = checked_align4(self.pos)?;
        let b = self.read_raw(4, "int")?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn read_i64(&mut self) -> Result<i64, VmExit> {
        self.pos = checked_align4(self.pos)?;
        let b = self.read_raw(8, "long")?;
        Ok(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn read_len(&mut self, what: &str) -> Result<Option<usize>, VmExit> {
        let n = self.read_i32()?;
        if n < -1 {
            return Err(bad(format!("Parcel: negative {what} length {n}")));
        }
        Ok(if n == -1 { None } else { Some(n as usize) })
    }
    fn read_string(&mut self) -> Result<Option<String>, VmExit> {
        let Some(n) = self.read_len("string")? else {
            return Ok(None);
        };
        let byte_len = n
            .checked_add(1)
            .and_then(|u| u.checked_mul(2))
            .ok_or_else(|| bad("Parcel under-run reading string: length overflow"))?;
        let bytes = self.read_padded(byte_len, "string")?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .take(n)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(Some(String::from_utf16_lossy(&units)))
    }
    fn read_string8(&mut self) -> Result<Option<String>, VmExit> {
        let Some(n) = self.read_len("string8")? else {
            return Ok(None);
        };
        let byte_len = n
            .checked_add(1)
            .ok_or_else(|| bad("Parcel under-run reading string8: length overflow"))?;
        let bytes = self.read_padded(byte_len, "string8")?;
        Ok(Some(String::from_utf8_lossy(&bytes[..n]).into_owned()))
    }
    fn read_byte_blob(&mut self) -> Result<Option<Vec<u8>>, VmExit> {
        let Some(n) = self.read_len("byte array")? else {
            return Ok(None);
        };
        Ok(Some(self.read_padded(n, "byte array")?))
    }
}

fn load(vm: &Engine, recv: ObjRef) -> PState {
    match vm.fw.get(recv) {
        Some(HostObj::Parcel { data, pos, objects }) => PState {
            data: data.clone(),
            pos: *pos,
            objects: objects.clone(),
        },
        _ => PState::default(),
    }
}

fn store(vm: &mut Engine, recv: ObjRef, st: PState) {
    vm.fw.objects.insert(
        recv,
        HostObj::Parcel {
            data: st.data,
            pos: st.pos,
            objects: st.objects,
        },
    );
}

pub(crate) fn new_parcel(vm: &mut Engine) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(PARCEL.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    store(vm, r, PState::default());
    Ok(Value::Obj(r))
}

/// Size of `v` under `Parcel.writeValue`, measured on a scratch Parcel.
/// AOSP `BaseBundle.dumpStats` reports entries by exactly this measure
/// (it parcels each value and prints those over its 1 KiB threshold).
pub(crate) fn value_size(vm: &mut Engine, v: &Value) -> Result<usize, VmExit> {
    let scratch = match new_parcel(vm)? {
        Value::Obj(r) => r,
        _ => return Ok(0),
    };
    write_value(vm, scratch, v)?;
    Ok(load(vm, scratch).data.len())
}

fn obj_arg(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn int_arg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
fn str_of(vm: &Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => Some(vm.heap.as_str(*r)?.to_string()),
        _ => None,
    })
}
fn char_seq_of(vm: &mut Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => {
            if let Ok(s) = vm.heap.as_str(*r) {
                Some(s.to_string())
            } else {
                match vm.invoke_instance(*r, "toString", "()Ljava/lang/String;", &[])? {
                    Value::Obj(s) => Some(vm.heap.as_str(s)?.to_string()),
                    _ => None,
                }
            }
        }
        _ => None,
    })
}
fn jstring(vm: &mut Engine, s: Option<String>) -> Result<Value, VmExit> {
    Ok(match s {
        Some(s) => Value::Obj(crate::intrinsics::alloc_string(vm, s)?),
        None => Value::Null,
    })
}
fn array_elems(vm: &Engine, v: Option<&Value>) -> Result<Option<Vec<Value>>, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => Some(vm.heap.as_array_elems(*r)?.0.to_vec()),
        _ => None,
    })
}
fn bytes_of(vm: &Engine, v: Option<&Value>) -> Result<Option<Vec<u8>>, VmExit> {
    Ok(array_elems(vm, v)?.map(|es| {
        es.iter()
            .map(|e| e.as_int().unwrap_or(0) as u8)
            .collect::<Vec<u8>>()
    }))
}

fn java_name(desc: &str) -> String {
    desc.trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".")
}
fn descriptor_of(name: &str) -> String {
    if name.starts_with('L') && name.ends_with(';') {
        name.to_string()
    } else {
        format!("L{};", name.replace('.', "/"))
    }
}

fn list_values(vm: &mut Engine, v: &Value) -> Result<Option<Vec<Value>>, VmExit> {
    let Some(r) = v.as_ref()? else {
        return Ok(None);
    };
    match vm.fw.get(r) {
        Some(HostObj::List { elems }) => return Ok(Some(elems.clone())),
        Some(HostObj::Bundle { .. }) | Some(HostObj::Map { .. }) => {
            return Err(bad("Parcel: expected List, got a map-like object"));
        }
        _ => {}
    }
    if let Ok((elems, _)) = vm.heap.as_array_elems(r) {
        return Ok(Some(elems.to_vec()));
    }
    // A DEX List implementation: use its own size()/get(i).
    let n = match vm.invoke_instance(r, "size", "()I", &[])? {
        Value::Int(n) if n >= 0 => n as usize,
        _ => return Err(bad("Parcel: List.size() did not return a count")),
    };
    let mut out = Vec::with_capacity(bounded_cap(n));
    for i in 0..n {
        out.push(vm.invoke_instance(r, "get", "(I)Ljava/lang/Object;", &[Value::Int(i as i32)])?);
    }
    Ok(Some(out))
}

fn new_list(vm: &mut Engine, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}

fn fill_list(vm: &mut Engine, dest: ObjRef, elems: Vec<Value>) -> Result<(), VmExit> {
    if matches!(vm.fw.get(dest), Some(HostObj::List { .. })) || vm.fw.get(dest).is_none() {
        vm.fw.objects.insert(dest, HostObj::List { elems });
        return Ok(());
    }
    let _ = vm.invoke_instance(dest, "clear", "()V", &[]);
    for e in elems {
        vm.invoke_instance(dest, "add", "(Ljava/lang/Object;)Z", &[e])?;
    }
    Ok(())
}

fn map_entries(vm: &Engine, v: &Value) -> Result<Option<Vec<(Value, Value)>>, VmExit> {
    let Some(r) = v.as_ref()? else {
        return Ok(None);
    };
    Ok(match vm.fw.get(r) {
        Some(HostObj::Map { entries }) => Some(entries.clone()),
        _ => {
            return Err(bad(
                "Parcel: only host Map implementations can be written with writeMap",
            ))
        }
    })
}

fn new_map(vm: &mut Engine, entries: Vec<(Value, Value)>) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Ljava/util/HashMap;".to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.objects.insert(r, HostObj::Map { entries });
    Ok(Value::Obj(r))
}

fn fill_map(vm: &mut Engine, dest: ObjRef, entries: Vec<(Value, Value)>) -> Result<(), VmExit> {
    if matches!(vm.fw.get(dest), Some(HostObj::Map { .. })) || vm.fw.get(dest).is_none() {
        vm.fw.objects.insert(dest, HostObj::Map { entries });
        return Ok(());
    }
    let _ = vm.invoke_instance(dest, "clear", "()V", &[]);
    for (k, val) in entries {
        vm.invoke_instance(
            dest,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[k, val],
        )?;
    }
    Ok(())
}

fn binder_handle(vm: &mut Engine, v: &Value) -> i32 {
    let Some(r) = (match v {
        Value::Obj(r) => Some(*r),
        _ => None,
    }) else {
        return 0;
    };
    if let Some(i) = vm
        .fw
        .binder_refs
        .iter()
        .position(|x| matches!(x, Value::Obj(o) if *o == r))
    {
        return i as i32 + 1;
    }
    vm.fw.binder_refs.push(Value::Obj(r));
    vm.fw.binder_refs.len() as i32
}

fn binder_for_handle(vm: &Engine, h: i32) -> Result<Value, VmExit> {
    if h == 0 {
        return Ok(Value::Null);
    }
    if h < 0 {
        return Err(bad(format!("Parcel: negative Binder handle {h}")));
    }
    vm.fw
        .binder_refs
        .get(h as usize - 1)
        .cloned()
        .ok_or_else(|| bad(format!("Parcel: unknown local Binder handle {h}")))
}

/// Write one strong Binder handle (the body of `writeStrongBinder`).
/// The binder is also recorded in the parcel's object table, mirroring the
/// native flat-binder-object list that `hasBinders` inspects.
fn write_strong_binder_to(vm: &mut Engine, parcel: ObjRef, v: &Value) -> Result<(), VmExit> {
    let h = binder_handle(vm, v);
    let mut st = load(vm, parcel);
    st.write_i32(h)?;
    if matches!(v, Value::Obj(_)) {
        st.objects.push(v.clone());
    }
    store(vm, parcel, st);
    Ok(())
}

/// Heap field holding a Parcel's class-cookie `ArrayMap<Class, Object>`
/// (AOSP `Parcel.mClassCookies`, created lazily by `setClassCookie`).
const CLASS_COOKIES_FIELD: &str = "rd$class_cookies";

fn class_cookies(vm: &mut Engine, parcel: ObjRef, create: bool) -> Result<Option<ObjRef>, VmExit> {
    if let Ok(Value::Obj(r)) = vm.heap.get_field(parcel, CLASS_COOKIES_FIELD) {
        return Ok(Some(r));
    }
    if !create {
        return Ok(None);
    }
    let r = vm
        .heap
        .alloc_instance("Landroid/util/ArrayMap;".to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.objects.insert(
        r,
        HostObj::Map {
            entries: Vec::new(),
        },
    );
    crate::err::put_field_result(
        vm.heap
            .put_field(parcel, CLASS_COOKIES_FIELD, Value::Obj(r)),
    )?;
    Ok(Some(r))
}

fn cookie_entries(vm: &Engine, cookies: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.get(cookies) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    }
}

/// libbinder `Parcel::compareData` ordering over two byte ranges:
/// lexicographic `memcmp` over the common prefix, then length.
fn compare_ranges(a: &[u8], b: &[u8]) -> i32 {
    for (x, y) in a.iter().zip(b.iter()) {
        if x != y {
            return *x as i32 - *y as i32;
        }
    }
    a.len() as i32 - b.len() as i32
}

/// Read one strong Binder handle (the body of `readStrongBinder`).
fn read_strong_binder_from(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let mut st = load(vm, parcel);
    let h = st.read_i32()?;
    store(vm, parcel, st);
    binder_for_handle(vm, h)
}

/// Write one `IInterface` as its strong Binder (the body of
/// `writeStrongInterface`): null writes a null Binder, a `Binder` subclass
/// writes itself, and any other interface implementation is asked for its
/// `asBinder()`, exactly as AOSP does.
fn write_strong_interface_to(vm: &mut Engine, parcel: ObjRef, v: &Value) -> Result<(), VmExit> {
    let binder = match v {
        Value::Obj(r) => {
            let class = vm.heap.class_of(*r)?.to_string();
            if class == BINDER || vm.cp.is_subtype(&class, BINDER) {
                Value::Obj(*r)
            } else {
                vm.invoke_instance(*r, "asBinder", "()Landroid/os/IBinder;", &[])?
            }
        }
        _ => Value::Null,
    };
    write_strong_binder_to(vm, parcel, &binder)?;
    Ok(())
}

/// Read one strong Binder and convert it with a guest
/// `java.util.function.Function<IBinder, T>.apply`, the `asInterface`
/// callback of the `*Interface*` container readers. The callback is invoked
/// even for a null Binder, matching AOSP.
fn read_strong_interface_with(
    vm: &mut Engine,
    parcel: ObjRef,
    as_interface: ObjRef,
) -> Result<Value, VmExit> {
    let binder = read_strong_binder_from(vm, parcel)?;
    vm.invoke_instance(
        as_interface,
        "apply",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[binder],
    )
}

// Reply-header exception codes (AOSP `Parcel` EX_* constants, in sync with
// libbinder's binder/Status.h).
const EX_SECURITY: i32 = -1;
const EX_BAD_PARCELABLE: i32 = -2;
const EX_ILLEGAL_ARGUMENT: i32 = -3;
const EX_NULL_POINTER: i32 = -4;
const EX_ILLEGAL_STATE: i32 = -5;
const EX_NETWORK_MAIN_THREAD: i32 = -6;
const EX_UNSUPPORTED_OPERATION: i32 = -7;
const EX_SERVICE_SPECIFIC: i32 = -8;
const EX_PARCELABLE: i32 = -9;
const EX_HAS_STRICTMODE_REPLY_HEADER: i32 = -128;

/// AOSP `Parcel.readExceptionCode`: consume any fat reply header and return
/// the real exception code (0 = no exception). The StrictMode header's
/// payload is skipped by its declared size (violations are not modeled);
/// AppOps noted-op headers are never produced locally.
fn read_exception_code(vm: &mut Engine, parcel: ObjRef) -> Result<i32, VmExit> {
    let mut st = load(vm, parcel);
    let mut code = st.read_i32()?;
    if code == EX_HAS_STRICTMODE_REPLY_HEADER {
        let header_size = st.read_i32()?;
        if header_size > 4 {
            let skip = (header_size - 4) as usize;
            let next_pos = st
                .pos
                .checked_add(skip)
                .filter(|&end| end <= st.data.len())
                .ok_or_else(|| bad("Parcel under-run reading StrictMode reply header"))?;
            st.pos = next_pos;
        }
        code = 0;
    }
    store(vm, parcel, st);
    Ok(code)
}

/// AOSP `Parcel.readException`: rethrow the exception written by
/// `writeException` (code, message, remote-stack-trace header, then any
/// code-specific payload).
fn read_exception(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let code = read_exception_code(vm, parcel)?;
    if code == 0 {
        return Ok(Value::Null);
    }
    let msg = read_string_from(vm, parcel)?.unwrap_or_default();
    {
        let mut st = load(vm, parcel);
        let stack_payload_size = st.read_i32()?;
        if stack_payload_size > 0 {
            // Remote stack trace string: consumed (AOSP attaches it as a
            // RemoteException cause; the thrown type/message are what
            // guest catch blocks observe).
            let _ = st.read_string()?;
        }
        store(vm, parcel, st);
    }
    let class = match code {
        EX_SECURITY => "Ljava/lang/SecurityException;",
        EX_BAD_PARCELABLE => "Landroid/os/BadParcelableException;",
        EX_ILLEGAL_ARGUMENT => "Ljava/lang/IllegalArgumentException;",
        EX_NULL_POINTER => "Ljava/lang/NullPointerException;",
        EX_ILLEGAL_STATE => "Ljava/lang/IllegalStateException;",
        EX_NETWORK_MAIN_THREAD => "Landroid/os/NetworkOnMainThreadException;",
        EX_UNSUPPORTED_OPERATION => "Ljava/lang/UnsupportedOperationException;",
        EX_SERVICE_SPECIFIC => {
            let _error_code = read_i32_from(vm, parcel)?;
            "Landroid/os/ServiceSpecificException;"
        }
        EX_PARCELABLE => {
            let size = read_i32_from(vm, parcel)?;
            if size > 0 {
                if let Value::Obj(e) = read_parcelable(vm, parcel)? {
                    let class = vm.heap.class_of(e)?.to_string();
                    let detail = match vm.heap.get_field(e, "detailMessage") {
                        Ok(Value::Obj(s)) => {
                            vm.heap.as_str(s).ok().map(str::to_string).unwrap_or(msg)
                        }
                        _ => msg,
                    };
                    return Err(Throwable::new(&class, detail).into());
                }
            }
            return Err(Throwable::new(
                "Ljava/lang/RuntimeException;",
                format!("{msg} [missing Parcelable]"),
            )
            .into());
        }
        _ => {
            return Err(Throwable::new(
                "Ljava/lang/RuntimeException;",
                format!("Unknown exception code: {code} msg {msg}"),
            )
            .into());
        }
    };
    Err(Throwable::new(class, msg).into())
}

fn write_string_to(vm: &mut Engine, parcel: ObjRef, s: Option<&str>) -> Result<(), VmExit> {
    let mut st = load(vm, parcel);
    st.write_string(s)?;
    store(vm, parcel, st);
    Ok(())
}

fn with_parcel_depth<T>(
    vm: &mut Engine,
    parcel: ObjRef,
    depth: usize,
    f: impl FnOnce(&mut Engine) -> Result<T, VmExit>,
) -> Result<T, VmExit> {
    let next = check_depth(depth)?;
    let prev = current_parcel_depth(vm, parcel);
    let _ = vm
        .heap
        .put_field(parcel, DEPTH_FIELD, Value::Int(next as i32));
    let res = f(vm);
    let _ = vm
        .heap
        .put_field(parcel, DEPTH_FIELD, Value::Int(prev as i32));
    res
}

fn creator_for(vm: &mut Engine, desc: &str) -> Result<ObjRef, VmExit> {
    vm.ensure_initialized(desc)?;
    let owner = vm
        .resolve_static_owner(desc, "CREATOR")
        .unwrap_or_else(|_| desc.to_string());
    match vm.statics.get(&(owner, "CREATOR".to_string())) {
        Some(Value::Obj(r)) => Ok(*r),
        _ => Err(bad(format!(
            "Parcel: {desc} has no Parcelable.Creator CREATOR field"
        ))),
    }
}

fn creator_create_depth(
    vm: &mut Engine,
    creator: ObjRef,
    parcel: ObjRef,
    target_desc: Option<&str>,
    depth: usize,
) -> Result<Value, VmExit> {
    with_parcel_depth(vm, parcel, depth, |vm| {
        let args = [Value::Obj(parcel)];
        // The erasure bridge every Creator<T> gets from javac/kotlinc.
        let class = vm.heap.class_of(creator)?.to_string();
        if vm
            .cp
            .resolve_method(
                &class,
                "createFromParcel",
                "(Landroid/os/Parcel;)Ljava/lang/Object;",
            )
            .is_some()
        {
            return vm.invoke_instance(
                creator,
                "createFromParcel",
                "(Landroid/os/Parcel;)Ljava/lang/Object;",
                &args,
            );
        }
        if let Some(desc) = target_desc {
            let sig = format!("(Landroid/os/Parcel;){desc}");
            if vm
                .cp
                .resolve_method(&class, "createFromParcel", &sig)
                .is_some()
            {
                return vm.invoke_instance(creator, "createFromParcel", &sig, &args);
            }
        }
        // Host-implemented creators (or a bridge resolution miss): go through the
        // normal invoke path, which reports a typed unresolved-method error.
        vm.invoke_instance(
            creator,
            "createFromParcel",
            "(Landroid/os/Parcel;)Ljava/lang/Object;",
            &args,
        )
    })
}

fn creator_new_array(vm: &mut Engine, creator: ObjRef, n: i32) -> Result<Value, VmExit> {
    vm.invoke_instance(
        creator,
        "newArray",
        "(I)[Ljava/lang/Object;",
        &[Value::Int(n)],
    )
}

fn invoke_write_to_parcel_depth(
    vm: &mut Engine,
    obj: ObjRef,
    parcel: ObjRef,
    flags: i32,
    depth: usize,
) -> Result<(), VmExit> {
    let class = vm.heap.class_of(obj)?.to_string();
    if class == BUNDLE || class == PERSISTABLE_BUNDLE {
        // Bundle is a host value, not DEX: write its AOSP
        // length/magic/ArrayMap frame directly.
        return write_bundle_map_depth(vm, parcel, obj, depth);
    }
    with_parcel_depth(vm, parcel, depth, |vm| {
        vm.invoke_instance(
            obj,
            "writeToParcel",
            "(Landroid/os/Parcel;I)V",
            &[Value::Obj(parcel), Value::Int(flags)],
        )?;
        Ok(())
    })
}

fn bundle_map(vm: &Engine, r: ObjRef) -> Vec<(String, Value)> {
    match vm.fw.get(r) {
        Some(HostObj::Bundle { map }) => map.clone(),
        _ => Vec::new(),
    }
}

fn write_bundle_entries_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    entries: Vec<(String, Value)>,
    depth: usize,
) -> Result<(), VmExit> {
    let len_i32 =
        i32::try_from(entries.len()).map_err(|_| bad("Parcel: bundle entry count overflow"))?;
    let mut st = load(vm, parcel);
    st.write_i32(len_i32)?;
    store(vm, parcel, st);
    for (k, v) in entries {
        write_string_to(vm, parcel, Some(&k))?;
        write_value_depth(vm, parcel, &v, depth)?;
    }
    Ok(())
}

fn read_bundle_entries_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    depth: usize,
) -> Result<Vec<(String, Value)>, VmExit> {
    let next_depth = check_depth(depth)?;
    let n = {
        let mut st = load(vm, parcel);
        let n = st.read_i32()?;
        if n < 0 {
            return Err(bad(format!("Parcel: negative bundle size {n}")));
        }
        let n = n as usize;
        check_avail_elems(&st, n, 8, "bundle")?;
        store(vm, parcel, st);
        n
    };
    let mut out = Vec::with_capacity(bounded_cap(n));
    for _ in 0..n {
        let mut st = load(vm, parcel);
        let k = st.read_string()?.unwrap_or_default();
        store(vm, parcel, st);
        let v = read_value_depth(vm, parcel, next_depth)?;
        out.push((k, v));
    }
    Ok(out)
}

/// Write a Bundle in AOSP `BaseBundle.writeToParcelInner` framing:
/// `length | BUNDLE_MAGIC | ArrayMap`. Despite the historical function
/// name, this is now the complete Bundle parcel representation used by
/// `Bundle.writeToParcel`, `Parcel.writeBundle`, `writeValue` and
/// `writeParcelable`; there is no longer a second internal-only format.
pub(crate) fn write_bundle_map(
    vm: &mut Engine,
    parcel: ObjRef,
    bundle: ObjRef,
) -> Result<(), VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    write_bundle_map_depth(vm, parcel, bundle, depth)
}

#[inline(never)]
fn write_bundle_map_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    bundle: ObjRef,
    depth: usize,
) -> Result<(), VmExit> {
    let next_depth = check_depth(depth)?;
    // A still-parcelled Bundle is written by appending its retained frame
    // payload verbatim (AOSP `Parcel.appendFrom` path in
    // `BaseBundle.writeToParcelInner`); it must not be materialized first.
    if let Some(payload) = parcelled_payload(vm, bundle) {
        let mut st = load(vm, parcel);
        st.pos = checked_align4(st.pos)?;
        st.write_i32(
            i32::try_from(payload.len())
                .map_err(|_| bad("Parcel: Bundle exceeds the maximum parcelable size"))?,
        )?;
        st.write_i32(BUNDLE_MAGIC)?;
        st.write_raw(&payload)?;
        store(vm, parcel, st);
        return Ok(());
    }
    let entries = bundle_map(vm, bundle);
    let (length_pos, start_pos) = {
        let mut st = load(vm, parcel);
        st.pos = checked_align4(st.pos)?;
        let length_pos = st.pos;
        st.write_i32(0)?; // Backpatched after the map is written.
        st.write_i32(BUNDLE_MAGIC)?;
        let start_pos = st.pos;
        store(vm, parcel, st);
        (length_pos, start_pos)
    };
    write_bundle_entries_depth(vm, parcel, entries, next_depth)?;
    let mut st = load(vm, parcel);
    let end_pos = st.pos;
    let length = i32::try_from(end_pos - start_pos)
        .map_err(|_| bad("Parcel: Bundle exceeds the maximum parcelable size"))?;
    st.patch_i32(length_pos, length);
    st.pos = end_pos;
    store(vm, parcel, st);
    Ok(())
}

/// Read one AOSP-framed Bundle eagerly. The declared length bounds the map
/// and is honored even if a forward-compatible producer leaves unread bytes
/// inside the Bundle frame. Production Bundle reads are lazy
/// (`new_lazy_bundle`); this eager form remains for the parcel unit tests.
#[cfg(test)]
pub(crate) fn read_bundle_map(
    vm: &mut Engine,
    parcel: ObjRef,
) -> Result<Vec<(String, Value)>, VmExit> {
    let end_pos = {
        let mut st = load(vm, parcel);
        let length = st.read_i32()?;
        if length < 0 {
            return Err(bad(format!("Parcel: negative Bundle length {length}")));
        }
        let magic = st.read_i32()?;
        if magic != BUNDLE_MAGIC {
            return Err(bad(format!(
                "Parcel: Bundle magic mismatch: expected {BUNDLE_MAGIC:#010x}, got {magic:#010x}"
            )));
        }
        let end_pos = st
            .pos
            .checked_add(length as usize)
            .filter(|end| *end <= st.data.len())
            .ok_or_else(|| bad("Parcel under-run reading Bundle: length exceeds parcel size"))?;
        store(vm, parcel, st);
        end_pos
    };
    let depth = current_parcel_depth(vm, parcel);
    let entries = read_bundle_entries_depth(vm, parcel, depth)?;
    let mut st = load(vm, parcel);
    if st.pos > end_pos {
        return Err(bad(
            "Parcel: Bundle entries exceed the declared Bundle length",
        ));
    }
    st.pos = end_pos;
    store(vm, parcel, st);
    Ok(entries)
}

/// The retained parcelled-data Parcel of a lazy Bundle, if it is still
/// parcelled (AOSP `BaseBundle.mParcelledData != null`).
pub(crate) fn parcelled_data(vm: &Engine, bundle: ObjRef) -> Option<ObjRef> {
    match vm.heap.get_field(bundle, PARCELLED_FIELD) {
        Ok(Value::Obj(r)) => Some(r),
        _ => None,
    }
}

/// The retained ArrayMap payload bytes of a lazy Bundle (frame length and
/// magic excluded), or `None` when the Bundle is materialized.
pub(crate) fn parcelled_payload(vm: &Engine, bundle: ObjRef) -> Option<Vec<u8>> {
    let data = parcelled_data(vm, bundle)?;
    match vm.fw.get(data) {
        Some(HostObj::Parcel { data, .. }) => Some(data.clone()),
        _ => None,
    }
}

/// Read one Bundle frame (`length | magic | payload`) without parsing the
/// payload: the payload bytes are copied into a fresh hidden Parcel and the
/// source cursor advances past the whole frame. This mirrors the AOSP
/// `Bundle(Parcel, length)` constructor used by `Parcel.readBundle`.
fn read_bundle_frame_retained(vm: &mut Engine, parcel: ObjRef) -> Result<ObjRef, VmExit> {
    let length = {
        let mut st = load(vm, parcel);
        let length = st.read_i32()?;
        store(vm, parcel, st);
        length
    };
    if length < 0 {
        return Err(bad(format!("Parcel: negative Bundle length {length}")));
    }
    read_bundle_frame_after_length(vm, parcel, length)
}

/// Second half of a Bundle frame read, for callers (like
/// `Parcel.readBundle`) that have already consumed the length int because
/// it doubles as the null marker.
fn read_bundle_frame_after_length(
    vm: &mut Engine,
    parcel: ObjRef,
    length: i32,
) -> Result<ObjRef, VmExit> {
    let (payload, objects) = {
        let mut st = load(vm, parcel);
        let magic = st.read_i32()?;
        if magic != BUNDLE_MAGIC {
            return Err(bad(format!(
                "Parcel: Bundle magic mismatch: expected {BUNDLE_MAGIC:#010x}, got {magic:#010x}"
            )));
        }
        let end_pos = st
            .pos
            .checked_add(length as usize)
            .filter(|end| *end <= st.data.len())
            .ok_or_else(|| bad("Parcel under-run reading Bundle: length exceeds parcel size"))?;
        let payload = st.data[st.pos..end_pos].to_vec();
        let objects = st.objects.clone();
        st.pos = end_pos;
        store(vm, parcel, st);
        (payload, objects)
    };
    let retained = match new_parcel(vm)? {
        Value::Obj(r) => r,
        _ => return Err(bad("Parcel: could not allocate retained Bundle data")),
    };
    store(
        vm,
        retained,
        PState {
            data: payload,
            pos: 0,
            objects,
        },
    );
    Ok(retained)
}

/// Attach retained parcelled data to `bundle`, marking it parcelled. Any
/// existing (materialized) map is dropped, matching AOSP
/// `BaseBundle.readFromParcelInner`, which replaces the map state with the
/// retained Parcel.
fn set_parcelled_data(vm: &mut Engine, bundle: ObjRef, retained: ObjRef) -> Result<(), VmExit> {
    vm.fw
        .objects
        .insert(bundle, HostObj::Bundle { map: Vec::new() });
    crate::err::put_field_result(
        vm.heap
            .put_field(bundle, PARCELLED_FIELD, Value::Obj(retained)),
    )
}

/// Create a new lazy Bundle of `class` from retained parcelled data.
fn new_bundle_with_retained(
    vm: &mut Engine,
    class: &str,
    retained: ObjRef,
) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    set_parcelled_data(vm, r, retained)?;
    Ok(Value::Obj(r))
}

/// Create a new lazy Bundle of `class` from the next Bundle frame in
/// `parcel` (no nullable marker). Used by `readValue` and `readParcelable`,
/// which are lazy in AOSP just like `readBundle`.
fn new_lazy_bundle(vm: &mut Engine, class: &str, parcel: ObjRef) -> Result<Value, VmExit> {
    let retained = read_bundle_frame_retained(vm, parcel)?;
    new_bundle_with_retained(vm, class, retained)
}

/// `Bundle.readFromParcel` adoption: read the next frame from `parcel` and
/// install it as `bundle`'s retained data without materializing it.
pub(crate) fn adopt_bundle_frame(
    vm: &mut Engine,
    parcel: ObjRef,
    bundle: ObjRef,
) -> Result<(), VmExit> {
    let retained = read_bundle_frame_retained(vm, parcel)?;
    set_parcelled_data(vm, bundle, retained)
}

/// Materialize a lazy Bundle in place (AOSP `BaseBundle.unparcel`): parse
/// the retained ArrayMap payload into the host map and drop the retained
/// Parcel. A no-op for an already-materialized Bundle.
pub(crate) fn unparcel_bundle(vm: &mut Engine, bundle: ObjRef) -> Result<(), VmExit> {
    let Some(retained) = parcelled_data(vm, bundle) else {
        return Ok(());
    };
    let depth = current_parcel_depth(vm, retained);
    let entries = read_bundle_entries_depth(vm, retained, depth)?;
    crate::err::put_field_result(vm.heap.put_field(bundle, PARCELLED_FIELD, Value::Null))?;
    vm.fw
        .objects
        .insert(bundle, HostObj::Bundle { map: entries });
    Ok(())
}

/// AOSP `Parcel.writeBundle` / `writePersistableBundle`: a null Bundle is
/// a lone int `-1` (where the frame length would be); a non-null Bundle is
/// its `writeToParcel` frame with no extra marker.
fn write_nullable_bundle(
    vm: &mut Engine,
    parcel: ObjRef,
    bundle: Option<ObjRef>,
) -> Result<(), VmExit> {
    match bundle {
        None => {
            let mut st = load(vm, parcel);
            st.write_i32(-1)?;
            store(vm, parcel, st);
            Ok(())
        }
        Some(bundle) => write_bundle_map(vm, parcel, bundle),
    }
}

/// AOSP `Parcel.readBundle` / `readPersistableBundle`: read the length int;
/// negative means the written Bundle was null. Otherwise construct the
/// Bundle lazily over the following frame (`new Bundle(Parcel, length)`),
/// retaining the payload until first real access (`unparcel`).
fn read_nullable_bundle(vm: &mut Engine, parcel: ObjRef, class: &str) -> Result<Value, VmExit> {
    let length = {
        let mut st = load(vm, parcel);
        let length = st.read_i32()?;
        store(vm, parcel, st);
        length
    };
    if length < 0 {
        return Ok(Value::Null);
    }
    let retained = read_bundle_frame_after_length(vm, parcel, length)?;
    new_bundle_with_retained(vm, class, retained)
}

fn sparse_entries(vm: &mut Engine, sparse: ObjRef) -> Result<Vec<(i32, Value)>, VmExit> {
    if let Some(entries) = vm.sparse.get(&sparse) {
        return entries
            .iter()
            .map(|(key, value)| {
                i32::try_from(*key)
                    .map(|key| (key, value.clone()))
                    .map_err(|_| {
                        bad(format!(
                            "Parcel: SparseArray key {key} is outside int range"
                        ))
                    })
            })
            .collect();
    }
    // A DEX SparseArray subclass can carry its contents in guest methods
    // rather than the host map. Parcel's own algorithm reads it through
    // size()/keyAt()/valueAt(), so do the same before giving up.
    let n = match vm.invoke_instance(sparse, "size", "()I", &[])? {
        Value::Int(n) if n >= 0 => n as usize,
        _ => return Err(bad("Parcel: SparseArray.size() did not return a count")),
    };
    let mut entries = Vec::with_capacity(bounded_cap(n));
    for i in 0..n {
        let index = Value::Int(i as i32);
        let key = vm
            .invoke_instance(sparse, "keyAt", "(I)I", std::slice::from_ref(&index))?
            .as_int()?;
        let value = vm.invoke_instance(
            sparse,
            "valueAt",
            "(I)Ljava/lang/Object;",
            std::slice::from_ref(&index),
        )?;
        entries.push((key, value));
    }
    Ok(entries)
}

fn new_sparse(vm: &mut Engine, class: &str, entries: Vec<(i32, Value)>) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.sparse.insert(
        r,
        entries
            .into_iter()
            .map(|(key, value)| (i64::from(key), value))
            .collect(),
    );
    Ok(Value::Obj(r))
}

/// Entries of a `SparseIntArray`: the host sparse map when present, else
/// the guest `size()/keyAt()/valueAt()` surface (Parcel's own algorithm).
fn sparse_int_entries(vm: &mut Engine, sparse: ObjRef) -> Result<Vec<(i32, i32)>, VmExit> {
    if let Some(entries) = vm.sparse.get(&sparse) {
        return entries
            .iter()
            .map(|(key, value)| {
                let key = i32::try_from(*key).map_err(|_| {
                    bad(format!(
                        "Parcel: SparseIntArray key {key} is outside int range"
                    ))
                })?;
                Ok((key, value.as_int().unwrap_or(0)))
            })
            .collect();
    }
    let n = match vm.invoke_instance(sparse, "size", "()I", &[])? {
        Value::Int(n) if n >= 0 => n as usize,
        _ => return Err(bad("Parcel: SparseIntArray.size() did not return a count")),
    };
    let mut entries = Vec::with_capacity(bounded_cap(n));
    for i in 0..n {
        let index = Value::Int(i as i32);
        let key = vm
            .invoke_instance(sparse, "keyAt", "(I)I", std::slice::from_ref(&index))?
            .as_int()?;
        let value = vm
            .invoke_instance(sparse, "valueAt", "(I)I", std::slice::from_ref(&index))?
            .as_int()?;
        entries.push((key, value));
    }
    Ok(entries)
}

fn write_sparse_array(
    vm: &mut Engine,
    parcel: ObjRef,
    sparse: Option<ObjRef>,
) -> Result<(), VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    write_sparse_array_depth(vm, parcel, sparse, depth)
}

fn write_sparse_array_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    sparse: Option<ObjRef>,
    depth: usize,
) -> Result<(), VmExit> {
    let next_depth = check_depth(depth)?;
    let Some(sparse) = sparse else {
        let mut st = load(vm, parcel);
        st.write_i32(-1)?;
        store(vm, parcel, st);
        return Ok(());
    };
    let entries = sparse_entries(vm, sparse)?;
    let len_i32 =
        i32::try_from(entries.len()).map_err(|_| bad("Parcel: SparseArray count overflow"))?;
    let mut st = load(vm, parcel);
    st.write_i32(len_i32)?;
    store(vm, parcel, st);
    for (key, value) in entries {
        let mut st = load(vm, parcel);
        st.write_i32(key)?;
        store(vm, parcel, st);
        write_value_depth(vm, parcel, &value, next_depth)?;
    }
    Ok(())
}

fn read_sparse_array(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    read_sparse_array_depth(vm, parcel, depth)
}

fn read_sparse_array_depth(vm: &mut Engine, parcel: ObjRef, depth: usize) -> Result<Value, VmExit> {
    let next_depth = check_depth(depth)?;
    let Some(n) = read_typed_len(vm, parcel, 8, "SparseArray")? else {
        return Ok(Value::Null);
    };
    let mut entries = Vec::with_capacity(bounded_cap(n));
    for _ in 0..n {
        let key = read_i32_from(vm, parcel)?;
        entries.push((key, read_value_depth(vm, parcel, next_depth)?));
    }
    new_sparse(vm, SPARSE_ARRAY, entries)
}

fn write_sparse_boolean_array(
    vm: &mut Engine,
    parcel: ObjRef,
    sparse: Option<ObjRef>,
) -> Result<(), VmExit> {
    let Some(sparse) = sparse else {
        let mut st = load(vm, parcel);
        st.write_i32(-1)?;
        store(vm, parcel, st);
        return Ok(());
    };
    let entries = sparse_entries(vm, sparse)?;
    let len_i32 = i32::try_from(entries.len())
        .map_err(|_| bad("Parcel: SparseBooleanArray count overflow"))?;
    let mut st = load(vm, parcel);
    st.write_i32(len_i32)?;
    for (key, value) in entries {
        st.write_i32(key)?;
        // AOSP writes each boolean with writeByte, whose native aligned
        // representation is one 4-byte word in the Parcel stream.
        st.write_i32((value.as_int().unwrap_or(0) != 0) as i32)?;
    }
    store(vm, parcel, st);
    Ok(())
}

fn read_sparse_boolean_array(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let Some(n) = read_typed_len(vm, parcel, 8, "SparseBooleanArray")? else {
        return Ok(Value::Null);
    };
    let mut st = load(vm, parcel);
    let mut entries = Vec::with_capacity(bounded_cap(n));
    for _ in 0..n {
        let key = st.read_i32()?;
        let value = st.read_i32()?;
        entries.push((key, Value::Int((value != 0) as i32)));
    }
    store(vm, parcel, st);
    new_sparse(vm, SPARSE_BOOLEAN_ARRAY, entries)
}

fn write_parcelable(vm: &mut Engine, parcel: ObjRef, v: &Value, flags: i32) -> Result<(), VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    write_parcelable_depth(vm, parcel, v, flags, depth)
}

fn write_parcelable_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    v: &Value,
    flags: i32,
    depth: usize,
) -> Result<(), VmExit> {
    let Some(r) = v.as_ref()? else {
        write_string_to(vm, parcel, None)?;
        return Ok(());
    };
    let class = vm.heap.class_of(r)?.to_string();
    let name = java_name(&class);
    write_string_to(vm, parcel, Some(&name))?;
    invoke_write_to_parcel_depth(vm, r, parcel, flags, depth)
}

fn read_parcelable(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    read_parcelable_depth(vm, parcel, depth)
}

fn read_parcelable_depth(vm: &mut Engine, parcel: ObjRef, depth: usize) -> Result<Value, VmExit> {
    let mut st = load(vm, parcel);
    let name = st.read_string()?;
    store(vm, parcel, st);
    let Some(name) = name else {
        return Ok(Value::Null);
    };
    let desc = descriptor_of(&name);
    if desc == BUNDLE || desc == PERSISTABLE_BUNDLE {
        // `Bundle.CREATOR.createFromParcel` constructs `Bundle(Parcel)`,
        // which retains the frame lazily just like `readBundle`.
        return new_lazy_bundle(vm, &desc, parcel);
    }
    let creator = creator_for(vm, &desc)?;
    creator_create_depth(vm, creator, parcel, Some(&desc), depth)
}

fn boxed(vm: &mut Engine, class: &str, v: Value) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(vm, class, v)?))
}

/// The length-prefixed value tags (AOSP `Parcel.isLengthPrefixed`):
/// `writeValue` writes tag, a placeholder length, the payload, then
/// backpatches the payload byte count — letting readers (notably lazy
/// Bundle values) skip a value without parsing it.
fn is_length_prefixed(tag: i32) -> bool {
    matches!(
        tag,
        VAL_MAP
            | VAL_PARCELABLE
            | VAL_LIST
            | VAL_SPARSEARRAY
            | VAL_PARCELABLEARRAY
            | VAL_OBJECTARRAY
            | VAL_SERIALIZABLE
    )
}

/// Write the `-1` length placeholder after a length-prefixed tag and
/// return its absolute position for `backpatch_value_length`.
fn write_length_placeholder(vm: &mut Engine, parcel: ObjRef) -> Result<usize, VmExit> {
    let mut st = load(vm, parcel);
    st.pos = checked_align4(st.pos)?;
    let length_pos = st.pos;
    st.write_i32(-1)?;
    store(vm, parcel, st);
    Ok(length_pos)
}

/// Backpatch a length placeholder with the payload byte count written
/// since (AOSP `writeValue` performs the same seek-and-patch).
fn backpatch_value_length(
    vm: &mut Engine,
    parcel: ObjRef,
    length_pos: usize,
) -> Result<(), VmExit> {
    let mut st = load(vm, parcel);
    let end = st.pos;
    let payload_len = end
        .checked_sub(length_pos.saturating_add(4))
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| bad("Parcel: length-prefixed value exceeds maximum size"))?;
    st.patch_i32(length_pos, payload_len);
    st.pos = end;
    store(vm, parcel, st);
    Ok(())
}

fn write_value(vm: &mut Engine, parcel: ObjRef, v: &Value) -> Result<(), VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    write_value_depth(vm, parcel, v, depth)
}

#[inline(never)]
fn write_value_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    v: &Value,
    depth: usize,
) -> Result<(), VmExit> {
    let next_depth = check_depth(depth)?;
    // Raw primitive Values can arrive from host-stored Bundles.
    match v {
        Value::Null | Value::WideHi => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_NULL)?;
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Int(i) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_INTEGER)?;
            st.write_i32(*i)?;
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Long(l) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_LONG)?;
            st.write_i64(*l)?;
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Float(f) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_FLOAT)?;
            st.write_i32(f.to_bits() as i32)?;
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Double(d) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_DOUBLE)?;
            st.write_i64(d.to_bits() as i64)?;
            store(vm, parcel, st);
            return Ok(());
        }
        Value::StrPlaceholder(_) | Value::Obj(_) => {}
    }
    let Some(r) = (match v {
        Value::Obj(r) => Some(*r),
        _ => None,
    }) else {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_NULL)?;
        store(vm, parcel, st);
        return Ok(());
    };
    write_value_obj_depth(vm, parcel, v, r, next_depth)
}

#[inline(never)]
fn write_value_obj_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    v: &Value,
    r: ObjRef,
    next_depth: usize,
) -> Result<(), VmExit> {
    if let Ok(s) = vm.heap.as_str(r) {
        let s = s.to_string();
        let mut st = load(vm, parcel);
        st.write_i32(VAL_STRING)?;
        st.write_string(Some(&s))?;
        store(vm, parcel, st);
        return Ok(());
    }
    let class = vm.heap.class_of(r)?.to_string();
    if let Some(kind) = crate::boxes::kind(&class) {
        let inner = vm.heap.get_field(r, "value").unwrap_or(Value::Null);
        let mut st = load(vm, parcel);
        match kind {
            'Z' => {
                st.write_i32(VAL_BOOLEAN)?;
                st.write_i32(inner.as_int().unwrap_or(0))?;
            }
            'B' => {
                st.write_i32(VAL_BYTE)?;
                st.write_i32(inner.as_int().unwrap_or(0))?;
            }
            'S' => {
                st.write_i32(VAL_SHORT)?;
                st.write_i32(inner.as_int().unwrap_or(0))?;
            }
            'C' => {
                // AOSP writes Character as VAL_CHAR + writeInt(char), not
                // as a one-character CharSequence.
                st.write_i32(VAL_CHAR)?;
                st.write_i32(inner.as_int().unwrap_or(0))?;
            }
            'I' => {
                st.write_i32(VAL_INTEGER)?;
                st.write_i32(inner.as_int().unwrap_or(0))?;
            }
            'J' => {
                st.write_i32(VAL_LONG)?;
                st.write_i64(inner.as_long().unwrap_or(0))?;
            }
            'F' => {
                st.write_i32(VAL_FLOAT)?;
                st.write_i32(inner.as_float().unwrap_or(0.0).to_bits() as i32)?;
            }
            'D' => {
                st.write_i32(VAL_DOUBLE)?;
                st.write_i64(inner.as_double().unwrap_or(0.0).to_bits() as i64)?;
            }
            _ => st.write_i32(VAL_NULL)?,
        }
        store(vm, parcel, st);
        return Ok(());
    }
    if class == BUNDLE || class == PERSISTABLE_BUNDLE {
        // Distinct value tags in AOSP: a PersistableBundle must not decode
        // back as a plain Bundle.
        let mut st = load(vm, parcel);
        st.write_i32(if class == BUNDLE {
            VAL_BUNDLE
        } else {
            VAL_PERSISTABLEBUNDLE
        })?;
        store(vm, parcel, st);
        return write_bundle_map_depth(vm, parcel, r, next_depth);
    }
    if class == "Landroid/util/Size;" || class == "Landroid/util/SizeF;" {
        // `writeSize` writes two ints, `writeSizeF` two floats; the host
        // representation keeps them in the `rd_w`/`rd_h` fields.
        let w = vm.heap.get_field(r, "rd_w").unwrap_or(Value::Null);
        let h = vm.heap.get_field(r, "rd_h").unwrap_or(Value::Null);
        let mut st = load(vm, parcel);
        if class == "Landroid/util/Size;" {
            st.write_i32(VAL_SIZE)?;
            st.write_i32(w.as_int().unwrap_or(0))?;
            st.write_i32(h.as_int().unwrap_or(0))?;
        } else {
            st.write_i32(VAL_SIZEF)?;
            st.write_i32(w.as_float().unwrap_or(0.0).to_bits() as i32)?;
            st.write_i32(h.as_float().unwrap_or(0.0).to_bits() as i32)?;
        }
        store(vm, parcel, st);
        return Ok(());
    }
    if class == SPARSE_BOOLEAN_ARRAY {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_SPARSEBOOLEANARRAY)?;
        store(vm, parcel, st);
        return write_sparse_boolean_array(vm, parcel, Some(r));
    }
    if class == SPARSE_ARRAY || vm.cp.is_subtype(&class, SPARSE_ARRAY) {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_SPARSEARRAY)?;
        store(vm, parcel, st);
        let length_pos = write_length_placeholder(vm, parcel)?;
        write_sparse_array_depth(vm, parcel, Some(r), next_depth)?;
        backpatch_value_length(vm, parcel, length_pos)?;
        return Ok(());
    }
    if class.starts_with('[') {
        let elems = vm.heap.as_array_elems(r)?.0.to_vec();
        let mut st = load(vm, parcel);
        match class.as_str() {
            "[B" => {
                st.write_i32(VAL_BYTEARRAY)?;
                let bytes: Vec<u8> = elems
                    .iter()
                    .map(|e| e.as_int().unwrap_or(0) as u8)
                    .collect();
                st.write_byte_blob(Some(&bytes))?;
            }
            "[I" | "[S" => {
                // short[] has its own tag in AOSP (same int-per-element
                // encoding as int[]); it must not decode back as int[].
                st.write_i32(if class == "[S" {
                    VAL_SHORTARRAY
                } else {
                    VAL_INTARRAY
                })?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i32(e.as_int().unwrap_or(0))?;
                }
            }
            "[Z" => {
                st.write_i32(VAL_BOOLEANARRAY)?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i32((e.as_int().unwrap_or(0) != 0) as i32)?;
                }
            }
            "[C" => {
                st.write_i32(VAL_CHARARRAY)?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i32(e.as_int().unwrap_or(0))?;
                }
            }
            "[J" => {
                st.write_i32(VAL_LONGARRAY)?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i64(e.as_long().unwrap_or(0))?;
                }
            }
            "[F" => {
                st.write_i32(VAL_FLOATARRAY)?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i32(e.as_float().unwrap_or(0.0).to_bits() as i32)?;
                }
            }
            "[D" => {
                st.write_i32(VAL_DOUBLEARRAY)?;
                st.write_i32(elems.len() as i32)?;
                for e in &elems {
                    st.write_i64(e.as_double().unwrap_or(0.0).to_bits() as i64)?;
                }
            }
            "[Ljava/lang/String;" => {
                // Strings need heap access after dropping the parcel state.
                drop(st);
                let mut st = load(vm, parcel);
                st.write_i32(VAL_STRINGARRAY)?;
                st.write_i32(elems.len() as i32)?;
                store(vm, parcel, st);
                for e in &elems {
                    let s = str_of(vm, Some(e))?;
                    write_string_to(vm, parcel, s.as_deref())?;
                }
                return Ok(());
            }
            "[Ljava/lang/CharSequence;" => {
                drop(st);
                let mut st = load(vm, parcel);
                st.write_i32(VAL_CHARSEQUENCEARRAY)?;
                st.write_i32(elems.len() as i32)?;
                store(vm, parcel, st);
                for e in &elems {
                    let s = char_seq_of(vm, Some(e))?;
                    write_string_to(vm, parcel, s.as_deref())?;
                }
                return Ok(());
            }
            _ => {
                // A Parcelable[] uses the dedicated VAL_PARCELABLEARRAY
                // form (count, then one writeParcelable per element, a
                // null class name marking a null element). Any other
                // object array keeps the Object[] value form; AOSP only
                // writes pure Object[] this way and serializes the rest,
                // which remains an Appendix C gap.
                let component = class.trim_start_matches('[');
                let parcelable_array = component.starts_with('L')
                    && (component == "Landroid/os/Parcelable;"
                        || vm.cp.is_subtype(component, "Landroid/os/Parcelable;"));
                st.write_i32(if parcelable_array {
                    VAL_PARCELABLEARRAY
                } else {
                    VAL_OBJECTARRAY
                })?;
                store(vm, parcel, st);
                let length_pos = write_length_placeholder(vm, parcel)?;
                let mut st = load(vm, parcel);
                st.write_i32(elems.len() as i32)?;
                store(vm, parcel, st);
                if parcelable_array {
                    for e in &elems {
                        write_parcelable_depth(vm, parcel, e, 0, next_depth)?;
                    }
                } else {
                    for e in &elems {
                        write_value_depth(vm, parcel, e, next_depth)?;
                    }
                }
                backpatch_value_length(vm, parcel, length_pos)?;
                return Ok(());
            }
        }
        store(vm, parcel, st);
        return Ok(());
    }
    if let Some(entries) = match vm.fw.get(r) {
        Some(HostObj::Map { entries }) => Some(entries.clone()),
        _ => None,
    } {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_MAP)?;
        store(vm, parcel, st);
        let length_pos = write_length_placeholder(vm, parcel)?;
        let mut st = load(vm, parcel);
        st.write_i32(entries.len() as i32)?;
        store(vm, parcel, st);
        for (k, val) in &entries {
            write_value_depth(vm, parcel, k, next_depth)?;
            write_value_depth(vm, parcel, val, next_depth)?;
        }
        backpatch_value_length(vm, parcel, length_pos)?;
        return Ok(());
    }
    if let Some(elems) = list_values(vm, v)? {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_LIST)?;
        store(vm, parcel, st);
        let length_pos = write_length_placeholder(vm, parcel)?;
        let mut st = load(vm, parcel);
        st.write_i32(elems.len() as i32)?;
        store(vm, parcel, st);
        for e in &elems {
            write_value_depth(vm, parcel, e, next_depth)?;
        }
        backpatch_value_length(vm, parcel, length_pos)?;
        return Ok(());
    }
    if class == BINDER || vm.cp.is_subtype(&class, BINDER) || vm.cp.is_subtype(&class, IBINDER) {
        let h = binder_handle(vm, v);
        let mut st = load(vm, parcel);
        st.write_i32(VAL_IBINDER)?;
        st.write_i32(h)?;
        store(vm, parcel, st);
        return Ok(());
    }
    if vm.cp.is_subtype(&class, "Ljava/lang/CharSequence;") {
        let s = char_seq_of(vm, Some(v))?;
        let mut st = load(vm, parcel);
        st.write_i32(VAL_CHARSEQUENCE)?;
        st.write_string(s.as_deref())?;
        store(vm, parcel, st);
        return Ok(());
    }
    if vm.cp.is_subtype(&class, "Landroid/os/Parcelable;") {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_PARCELABLE)?;
        store(vm, parcel, st);
        let length_pos = write_length_placeholder(vm, parcel)?;
        write_parcelable_depth(vm, parcel, v, 0, next_depth)?;
        backpatch_value_length(vm, parcel, length_pos)?;
        return Ok(());
    }
    Err(bad(format!(
        "Parcel.writeValue: unsupported value class {class}"
    )))
}

/// AOSP boolean arrays are one int per element (`writeBooleanArray`
/// loops `writeInt(v ? 1 : 0)`), like short/char arrays — not one byte.
fn write_bool_array_to(
    vm: &mut Engine,
    parcel: ObjRef,
    elems: Option<&[Value]>,
) -> Result<(), VmExit> {
    let mut st = load(vm, parcel);
    match elems {
        None => st.write_i32(-1)?,
        Some(es) => {
            st.write_i32(es.len() as i32)?;
            for e in es {
                st.write_i32((e.as_int().unwrap_or(0) != 0) as i32)?;
            }
        }
    }
    store(vm, parcel, st);
    Ok(())
}

fn read_bool_array_from(vm: &mut Engine, parcel: ObjRef) -> Result<Option<Vec<Value>>, VmExit> {
    let Some(n) = read_typed_len(vm, parcel, 4, "boolean array")? else {
        return Ok(None);
    };
    let mut vals = Vec::with_capacity(bounded_cap(n));
    let mut st = load(vm, parcel);
    for _ in 0..n {
        vals.push(Value::Int((st.read_i32()? != 0) as i32));
    }
    store(vm, parcel, st);
    Ok(Some(vals))
}

fn read_typed_len(
    vm: &mut Engine,
    parcel: ObjRef,
    min_elem_bytes: usize,
    what: &str,
) -> Result<Option<usize>, VmExit> {
    let mut st = load(vm, parcel);
    let n = st.read_len(what)?;
    if let Some(count) = n {
        check_avail_elems(&st, count, min_elem_bytes, what)?;
    }
    store(vm, parcel, st);
    Ok(n)
}

fn read_i32_from(vm: &mut Engine, parcel: ObjRef) -> Result<i32, VmExit> {
    let mut st = load(vm, parcel);
    let v = st.read_i32()?;
    store(vm, parcel, st);
    Ok(v)
}

fn read_i64_from(vm: &mut Engine, parcel: ObjRef) -> Result<i64, VmExit> {
    let mut st = load(vm, parcel);
    let v = st.read_i64()?;
    store(vm, parcel, st);
    Ok(v)
}

fn read_string_from(vm: &mut Engine, parcel: ObjRef) -> Result<Option<String>, VmExit> {
    let mut st = load(vm, parcel);
    let v = st.read_string()?;
    store(vm, parcel, st);
    Ok(v)
}

fn read_blob_from(vm: &mut Engine, parcel: ObjRef) -> Result<Option<Vec<u8>>, VmExit> {
    let mut st = load(vm, parcel);
    let v = st.read_byte_blob()?;
    store(vm, parcel, st);
    Ok(v)
}

fn read_value(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    read_value_depth(vm, parcel, depth)
}

#[inline(never)]
fn read_value_depth(vm: &mut Engine, parcel: ObjRef, depth: usize) -> Result<Value, VmExit> {
    let next_depth = check_depth(depth)?;
    let mut st = load(vm, parcel);
    let tag = st.read_i32()?;
    store(vm, parcel, st);
    if is_length_prefixed(tag) {
        // Consume the backpatched payload length. AOSP only logs (wtf) if
        // the parser does not consume exactly that many bytes and leaves
        // the cursor where parsing stopped; we keep the same cursor
        // behavior but reject a length that runs past the parcel here.
        let mut st = load(vm, parcel);
        let length = st.read_i32()?;
        let ok_end = usize::try_from(length)
            .ok()
            .and_then(|len| st.pos.checked_add(len))
            .filter(|&end| end <= st.data.len());
        if ok_end.is_none() {
            return Err(bad(format!(
                "Parcel.readValue: invalid length-prefixed size {length} for tag {tag}"
            )));
        }
        store(vm, parcel, st);
    }
    read_value_body_depth(vm, parcel, tag, next_depth)
}

#[inline(never)]
fn read_value_body_depth(
    vm: &mut Engine,
    parcel: ObjRef,
    tag: i32,
    next_depth: usize,
) -> Result<Value, VmExit> {
    Ok(match tag {
        VAL_NULL => Value::Null,
        VAL_STRING | VAL_CHARSEQUENCE => {
            let s = read_string_from(vm, parcel)?;
            jstring(vm, s)?
        }
        VAL_INTEGER => {
            let v = read_i32_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Integer;", Value::Int(v))?
        }
        VAL_SHORT => {
            let v = read_i32_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Short;", Value::Int(v))?
        }
        VAL_BYTE => {
            let v = read_i32_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Byte;", Value::Int(v))?
        }
        VAL_CHAR => {
            let v = read_i32_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Character;", Value::Int(v))?
        }
        VAL_SIZE | VAL_SIZEF => {
            let (w, h) = {
                let mut st = load(vm, parcel);
                let w = st.read_i32()?;
                let h = st.read_i32()?;
                store(vm, parcel, st);
                (w, h)
            };
            let (class, w, h) = if tag == VAL_SIZE {
                ("Landroid/util/Size;", Value::Int(w), Value::Int(h))
            } else {
                let (wf, hf) = (f32::from_bits(w as u32), f32::from_bits(h as u32));
                if !wf.is_finite() || !hf.is_finite() {
                    return Err(iae("Parcel.readValue: non-finite SizeF dimension"));
                }
                ("Landroid/util/SizeF;", Value::Float(wf), Value::Float(hf))
            };
            let r = vm
                .heap
                .alloc_instance(class.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_w", w))?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_h", h))?;
            Value::Obj(r)
        }
        VAL_BOOLEAN => {
            let v = read_i32_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Boolean;", Value::Int((v != 0) as i32))?
        }
        VAL_LONG => {
            let v = read_i64_from(vm, parcel)?;
            boxed(vm, "Ljava/lang/Long;", Value::Long(v))?
        }
        VAL_FLOAT => {
            let v = read_i32_from(vm, parcel)?;
            boxed(
                vm,
                "Ljava/lang/Float;",
                Value::Float(f32::from_bits(v as u32)),
            )?
        }
        VAL_DOUBLE => {
            let v = read_i64_from(vm, parcel)?;
            boxed(
                vm,
                "Ljava/lang/Double;",
                Value::Double(f64::from_bits(v as u64)),
            )?
        }
        VAL_BYTEARRAY => match read_blob_from(vm, parcel)? {
            Some(b) => crate::jlstring::byte_array(vm, &b)?,
            None => Value::Null,
        },
        VAL_INTARRAY | VAL_SHORTARRAY | VAL_CHARARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 4, "int array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Int(st.read_i32()?));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(
                vm,
                match tag {
                    VAL_CHARARRAY => "[C",
                    VAL_SHORTARRAY => "[S",
                    _ => "[I",
                },
                ElemKind::Int,
                vals,
            )?
        }
        VAL_LONGARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 8, "long array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Long(st.read_i64()?));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[J", ElemKind::Long, vals)?
        }
        VAL_FLOATARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 4, "float array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Float(f32::from_bits(st.read_i32()? as u32)));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[F", ElemKind::Float, vals)?
        }
        VAL_DOUBLEARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 8, "double array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Double(f64::from_bits(st.read_i64()? as u64)));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[D", ElemKind::Double, vals)?
        }
        VAL_BOOLEANARRAY => match read_bool_array_from(vm, parcel)? {
            Some(vals) => crate::arrays::new_array(vm, "[Z", ElemKind::Int, vals)?,
            None => Value::Null,
        },
        VAL_STRINGARRAY | VAL_CHARSEQUENCEARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 4, "string array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, parcel);
                let s = st.read_string()?;
                store(vm, parcel, st);
                vals.push(jstring(vm, s)?);
            }
            crate::arrays::new_array(
                vm,
                if tag == VAL_CHARSEQUENCEARRAY {
                    "[Ljava/lang/CharSequence;"
                } else {
                    "[Ljava/lang/String;"
                },
                ElemKind::Obj,
                vals,
            )?
        }
        VAL_PARCELABLEARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 4, "parcelable array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_parcelable_depth(vm, parcel, next_depth)?);
            }
            crate::arrays::new_array(vm, "[Landroid/os/Parcelable;", ElemKind::Obj, vals)?
        }
        VAL_OBJECTARRAY => {
            let Some(n) = read_typed_len(vm, parcel, 4, "object array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_value_depth(vm, parcel, next_depth)?);
            }
            crate::arrays::new_array(vm, "[Ljava/lang/Object;", ElemKind::Obj, vals)?
        }
        VAL_LIST => {
            let Some(n) = read_typed_len(vm, parcel, 4, "list")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_value_depth(vm, parcel, next_depth)?);
            }
            new_list(vm, vals)?
        }
        VAL_MAP => {
            let Some(n) = read_typed_len(vm, parcel, 8, "map")? else {
                return Ok(Value::Null);
            };
            let mut entries = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let k = read_value_depth(vm, parcel, next_depth)?;
                let v = read_value_depth(vm, parcel, next_depth)?;
                entries.push((k, v));
            }
            new_map(vm, entries)?
        }
        VAL_BUNDLE => new_lazy_bundle(vm, BUNDLE, parcel)?,
        VAL_PERSISTABLEBUNDLE => new_lazy_bundle(vm, PERSISTABLE_BUNDLE, parcel)?,
        VAL_SPARSEARRAY => read_sparse_array_depth(vm, parcel, next_depth)?,
        VAL_SPARSEBOOLEANARRAY => read_sparse_boolean_array(vm, parcel)?,
        VAL_IBINDER => {
            let mut st = load(vm, parcel);
            let h = st.read_i32()?;
            store(vm, parcel, st);
            binder_for_handle(vm, h)?
        }
        VAL_PARCELABLE => read_parcelable_depth(vm, parcel, next_depth)?,
        VAL_SERIALIZABLE => {
            return Err(bad(
                "Parcel.readValue: Java-serialized values are not supported",
            ))
        }
        other => return Err(bad(format!("Parcel.readValue: unsupported tag {other}"))),
    })
}

fn write_present_typed(
    vm: &mut Engine,
    parcel: ObjRef,
    elems: &[Value],
    flags: i32,
) -> Result<(), VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    for e in elems {
        match e.as_ref()? {
            Some(r) => {
                let mut st = load(vm, parcel);
                st.write_i32(1)?;
                store(vm, parcel, st);
                invoke_write_to_parcel_depth(vm, r, parcel, flags, depth)?;
            }
            None => {
                let mut st = load(vm, parcel);
                st.write_i32(0)?;
                store(vm, parcel, st);
            }
        }
    }
    Ok(())
}

fn read_present_typed(
    vm: &mut Engine,
    parcel: ObjRef,
    creator: ObjRef,
    n: usize,
) -> Result<Vec<Value>, VmExit> {
    let depth = current_parcel_depth(vm, parcel);
    check_avail_elems(&load(vm, parcel), n, 4, "typed elements")?;
    let mut out = Vec::with_capacity(bounded_cap(n));
    for _ in 0..n {
        let present = {
            let mut st = load(vm, parcel);
            let p = st.read_i32()?;
            store(vm, parcel, st);
            p != 0
        };
        out.push(if present {
            creator_create_depth(vm, creator, parcel, None, depth)?
        } else {
            Value::Null
        });
    }
    Ok(out)
}

fn parcel_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let some = |v: Value| Ok(Some(v));
    Ok(match (name, sig) {
        ("<init>", _) => {
            store(vm, recv, PState::default());
            return some(Value::Null);
        }
        ("recycle", "()V") => {
            store(vm, recv, PState::default());
            return some(Value::Null);
        }
        ("dataSize", "()I") => return some(Value::Int(load(vm, recv).data.len() as i32)),
        ("dataAvail", "()I") => {
            let st = load(vm, recv);
            return some(Value::Int((st.data.len().saturating_sub(st.pos)) as i32));
        }
        ("dataPosition", "()I") => return some(Value::Int(load(vm, recv).pos as i32)),
        ("dataCapacity", "()I") => return some(Value::Int(load(vm, recv).data.len() as i32)),
        ("setDataPosition", "(I)V") => {
            let p = int_arg(args, 0);
            if p < 0 {
                return Err(iae("Parcel.setDataPosition: negative position"));
            }
            let mut st = load(vm, recv);
            if p as usize > st.data.len() {
                return Err(iae("Parcel.setDataPosition: position beyond data size"));
            }
            st.pos = p as usize;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("setDataSize", "(I)V") => {
            let n = int_arg(args, 0);
            if n < 0 {
                return Err(iae("Parcel.setDataSize: negative size"));
            }
            let n = n as usize;
            ensure_parcel_size(n, "setDataSize")?;
            let mut st = load(vm, recv);
            st.data.resize(n, 0);
            st.pos = st.pos.min(n);
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("setDataCapacity", "(I)V") => {
            let cap = int_arg(args, 0);
            if cap < 0 {
                return Err(iae("Parcel.setDataCapacity: negative capacity"));
            }
            ensure_parcel_size(cap as usize, "setDataCapacity")?;
            return some(Value::Null);
        }
        ("marshall", "()[B") => {
            let data = load(vm, recv).data;
            return some(crate::jlstring::byte_array(vm, &data)?);
        }
        ("unmarshall", "([BII)V") => {
            let Some(bytes) = bytes_of(vm, args.first())? else {
                return Err(npe("Parcel.unmarshall: data is null"));
            };
            let off = int_arg(args, 1);
            let len = int_arg(args, 2);
            if off < 0
                || len < 0
                || off as usize > bytes.len()
                || len as usize > bytes.len() - off as usize
            {
                return Err(iae("Parcel.unmarshall: offset/length out of bounds"));
            }
            ensure_parcel_size(len as usize, "unmarshall")?;
            store(
                vm,
                recv,
                PState {
                    data: bytes[off as usize..off as usize + len as usize].to_vec(),
                    pos: 0,
                    objects: Vec::new(),
                },
            );
            return some(Value::Null);
        }
        ("appendFrom", "(Landroid/os/Parcel;II)V") => {
            let Some(other) = obj_arg(args, 0) else {
                return Err(npe("Parcel.appendFrom: source is null"));
            };
            let src = load(vm, other).data;
            let off = int_arg(args, 1);
            let len = int_arg(args, 2);
            if off < 0
                || len < 0
                || off as usize > src.len()
                || len as usize > src.len() - off as usize
            {
                return Err(iae("Parcel.appendFrom: offset/length out of bounds"));
            }
            let mut st = load(vm, recv);
            st.write_raw(&src[off as usize..off as usize + len as usize])?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("hasFileDescriptors", "()Z") | ("hasFileDescriptors", "(II)Z") => {
            // No file-descriptor object can ever be written into a host
            // parcel (FD forms are typed-unsupported), so this is exact.
            return some(Value::Int(0));
        }
        ("setClassCookie", "(Ljava/lang/Class;Ljava/lang/Object;)V") => {
            let Some(cookies) = class_cookies(vm, recv, true)? else {
                return some(Value::Null);
            };
            let key = args.first().cloned().unwrap_or(Value::Null);
            let value = args.get(1).cloned().unwrap_or(Value::Null);
            let mut entries = cookie_entries(vm, cookies);
            if let Some(e) = entries.iter_mut().find(|(k, _)| *k == key) {
                e.1 = value;
            } else {
                entries.push((key, value));
            }
            vm.fw.objects.insert(cookies, HostObj::Map { entries });
            return some(Value::Null);
        }
        ("getClassCookie", "(Ljava/lang/Class;)Ljava/lang/Object;") => {
            let found = match class_cookies(vm, recv, false)? {
                Some(cookies) => cookie_entries(vm, cookies)
                    .into_iter()
                    .find(|(k, _)| Some(k) == args.first())
                    .map(|(_, v)| v),
                None => None,
            };
            return some(found.unwrap_or(Value::Null));
        }
        ("hasClassCookie", "(Ljava/lang/Class;)Z") => {
            let found = match class_cookies(vm, recv, false)? {
                Some(cookies) => cookie_entries(vm, cookies)
                    .iter()
                    .any(|(k, _)| Some(k) == args.first()),
                None => false,
            };
            return some(Value::Int(found as i32));
        }
        ("removeClassCookie", "(Ljava/lang/Class;Ljava/lang/Object;)V") => {
            if let Some(cookies) = class_cookies(vm, recv, false)? {
                let mut entries = cookie_entries(vm, cookies);
                entries.retain(|(k, _)| Some(k) != args.first());
                vm.fw.objects.insert(cookies, HostObj::Map { entries });
            }
            return some(Value::Null);
        }
        ("adoptClassCookies", "(Landroid/os/Parcel;)V") => {
            // AOSP adopts the source's cookie map reference itself.
            let Some(from) = obj_arg(args, 0) else {
                return Err(npe("Parcel.adoptClassCookies: source is null"));
            };
            let adopted = vm
                .heap
                .get_field(from, CLASS_COOKIES_FIELD)
                .unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(recv, CLASS_COOKIES_FIELD, adopted))?;
            return some(Value::Null);
        }
        ("copyClassCookies", "()Ljava/util/Map;") => {
            // AOSP `new ArrayMap<>(mClassCookies)` — a copy even when the
            // parcel has no cookies (empty map, tolerating null).
            let entries = match class_cookies(vm, recv, false)? {
                Some(cookies) => cookie_entries(vm, cookies),
                None => Vec::new(),
            };
            let copy = vm
                .heap
                .alloc_instance("Landroid/util/ArrayMap;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            vm.fw.objects.insert(copy, HostObj::Map { entries });
            return some(Value::Obj(copy));
        }
        ("putClassCookies", "(Ljava/util/Map;)V") => {
            if let Some(Value::Obj(source)) = args.first() {
                if let Some(entries) = match vm.fw.get(*source) {
                    Some(HostObj::Map { entries }) => Some(entries.clone()),
                    _ => None,
                } {
                    if let Some(cookies) = class_cookies(vm, recv, true)? {
                        let mut mine = cookie_entries(vm, cookies);
                        for (k, v) in entries {
                            if let Some(e) = mine.iter_mut().find(|(ek, _)| *ek == k) {
                                e.1 = v;
                            } else {
                                mine.push((k, v));
                            }
                        }
                        vm.fw.objects.insert(cookies, HostObj::Map { entries: mine });
                    }
                }
            }
            return some(Value::Null);
        }
        ("hasBinders", "()Z") => {
            let st = load(vm, recv);
            return some(Value::Int(
                st.objects.iter().any(|o| matches!(o, Value::Obj(_))) as i32,
            ));
        }
        ("enforceNoDataAvail", "()V") => {
            let st = load(vm, recv);
            let unread = st.data.len().saturating_sub(st.pos);
            if unread > 0 {
                return Err(bad(format!(
                    "Parcel data not fully consumed, unread size: {unread}"
                )));
            }
            return some(Value::Null);
        }
        ("compareData", "(Landroid/os/Parcel;)I") => {
            let Some(other) = obj_arg(args, 0) else {
                return Err(npe("Parcel.compareData: other is null"));
            };
            let a = load(vm, recv).data;
            let b = load(vm, other).data;
            return some(Value::Int(compare_ranges(&a, &b)));
        }
        ("writeSize", "(Landroid/util/Size;)V") => {
            let Some(r) = obj_arg(args, 0) else {
                return Err(npe("Parcel.writeSize: size is null"));
            };
            let w = vm.heap.get_field(r, "rd_w").unwrap_or(Value::Null);
            let h = vm.heap.get_field(r, "rd_h").unwrap_or(Value::Null);
            let mut st = load(vm, recv);
            st.write_i32(w.as_int().unwrap_or(0))?;
            st.write_i32(h.as_int().unwrap_or(0))?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readSize", "()Landroid/util/Size;") => {
            let (w, h) = {
                let mut st = load(vm, recv);
                let pair = (st.read_i32()?, st.read_i32()?);
                store(vm, recv, st);
                pair
            };
            let r = vm
                .heap
                .alloc_instance("Landroid/util/Size;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_w", Value::Int(w)))?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_h", Value::Int(h)))?;
            return some(Value::Obj(r));
        }
        ("writeSizeF", "(Landroid/util/SizeF;)V") => {
            let Some(r) = obj_arg(args, 0) else {
                return Err(npe("Parcel.writeSizeF: size is null"));
            };
            let w = vm.heap.get_field(r, "rd_w").unwrap_or(Value::Null);
            let h = vm.heap.get_field(r, "rd_h").unwrap_or(Value::Null);
            let mut st = load(vm, recv);
            st.write_i32(w.as_float().unwrap_or(0.0).to_bits() as i32)?;
            st.write_i32(h.as_float().unwrap_or(0.0).to_bits() as i32)?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readSizeF", "()Landroid/util/SizeF;") => {
            let (w, h) = {
                let mut st = load(vm, recv);
                let pair = (st.read_i32()?, st.read_i32()?);
                store(vm, recv, st);
                pair
            };
            // `SizeF`'s constructor rejects NaN/infinite dimensions; the
            // same check applies when unmarshalling one.
            let (wf, hf) = (f32::from_bits(w as u32), f32::from_bits(h as u32));
            if !wf.is_finite() || !hf.is_finite() {
                return Err(iae("Parcel.readSizeF: non-finite dimension"));
            }
            let r = vm
                .heap
                .alloc_instance("Landroid/util/SizeF;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_w", Value::Float(wf)))?;
            crate::err::put_field_result(vm.heap.put_field(r, "rd_h", Value::Float(hf)))?;
            return some(Value::Obj(r));
        }
        ("writeInterfaceToken", "(Ljava/lang/String;)V") => {
            let token = str_of(vm, args.first())?;
            let mut st = load(vm, recv);
            st.write_i32(0)?; // local strict-mode header word
            st.write_string(token.as_deref())?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("enforceInterface", "(Ljava/lang/String;)V") => {
            let want = str_of(vm, args.first())?;
            let mut st = load(vm, recv);
            let _header = st.read_i32()?;
            let got = st.read_string()?;
            store(vm, recv, st);
            if got != want {
                return Err(Throwable::new(
                    "Ljava/lang/SecurityException;",
                    format!(
                        "Binder invocation to an incorrect interface (want {:?}, got {:?})",
                        want, got
                    ),
                )
                .into());
            }
            return some(Value::Null);
        }
        ("writeNoException", "()V") => {
            let mut st = load(vm, recv);
            st.write_i32(0)?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readExceptionCode", "()I") => {
            return some(Value::Int(read_exception_code(vm, recv)?));
        }
        ("readException", "()V") => {
            return some(read_exception(vm, recv)?);
        }
        ("writeException", "(Ljava/lang/Exception;)V") => {
            let Some(e) = obj_arg(args, 0) else {
                let mut st = load(vm, recv);
                st.write_i32(0)?;
                store(vm, recv, st);
                return some(Value::Null);
            };
            let class = vm.heap.class_of(e)?.to_string();
            // AOSP `getExceptionCode` (instanceof order matters):
            // Parcelable-boot exceptions first, then by exception class.
            let is = |target: &str| class == target || vm.cp.is_subtype(&class, target);
            let code = if is("Ljava/lang/SecurityException;") {
                EX_SECURITY
            } else if is("Landroid/os/BadParcelableException;") {
                EX_BAD_PARCELABLE
            } else if is("Ljava/lang/IllegalArgumentException;") {
                EX_ILLEGAL_ARGUMENT
            } else if is("Ljava/lang/NullPointerException;") {
                EX_NULL_POINTER
            } else if is("Ljava/lang/IllegalStateException;") {
                EX_ILLEGAL_STATE
            } else if is("Landroid/os/NetworkOnMainThreadException;") {
                EX_NETWORK_MAIN_THREAD
            } else if is("Ljava/lang/UnsupportedOperationException;") {
                EX_UNSUPPORTED_OPERATION
            } else if is("Landroid/os/ServiceSpecificException;") {
                EX_SERVICE_SPECIFIC
            } else {
                0
            };
            let msg = match vm.heap.get_field(e, "detailMessage") {
                Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
                _ => None,
            };
            let mut st = load(vm, recv);
            st.write_i32(code)?;
            if code == 0 {
                // AOSP writes the 0 code and then rethrows exceptions it
                // has no code for instead of marshalling them.
                store(vm, recv, st);
                return Err(Throwable::new(&class, msg.unwrap_or_default()).into());
            }
            st.write_string(msg.as_deref())?;
            st.write_i32(0)?; // no remote stack-trace payload
            if code == EX_SERVICE_SPECIFIC {
                let error_code = match vm.heap.get_field(e, "errorCode") {
                    Ok(v) => v.as_int().unwrap_or(0),
                    Err(_) => 0,
                };
                st.write_i32(error_code)?;
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("writeInt", "(I)V") => {
            let mut st = load(vm, recv);
            st.write_i32(int_arg(args, 0))?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readInt", "()I") => {
            let mut st = load(vm, recv);
            let v = st.read_i32()?;
            store(vm, recv, st);
            return some(Value::Int(v));
        }
        ("writeLong", "(J)V") => {
            let mut st = load(vm, recv);
            st.write_i64(args.first().and_then(|v| v.as_long().ok()).unwrap_or(0))?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readLong", "()J") => {
            let mut st = load(vm, recv);
            let v = st.read_i64()?;
            store(vm, recv, st);
            return some(Value::Long(v));
        }
        ("writeFloat", "(F)V") => {
            let mut st = load(vm, recv);
            st.write_i32(
                args.first()
                    .and_then(|v| v.as_float().ok())
                    .unwrap_or(0.0)
                    .to_bits() as i32,
            )?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readFloat", "()F") => {
            let mut st = load(vm, recv);
            let v = st.read_i32()?;
            store(vm, recv, st);
            return some(Value::Float(f32::from_bits(v as u32)));
        }
        ("writeDouble", "(D)V") => {
            let mut st = load(vm, recv);
            st.write_i64(
                args.first()
                    .and_then(|v| v.as_double().ok())
                    .unwrap_or(0.0)
                    .to_bits() as i64,
            )?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readDouble", "()D") => {
            let mut st = load(vm, recv);
            let v = st.read_i64()?;
            store(vm, recv, st);
            return some(Value::Double(f64::from_bits(v as u64)));
        }
        ("writeByte", "(B)V") => {
            let mut st = load(vm, recv);
            st.write_i32(int_arg(args, 0) as i8 as i32)?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readByte", "()B") => {
            let mut st = load(vm, recv);
            let v = st.read_i32()?;
            store(vm, recv, st);
            return some(Value::Int(v as i8 as i32));
        }
        ("writeBoolean", "(Z)V") => {
            let mut st = load(vm, recv);
            st.write_i32((int_arg(args, 0) != 0) as i32)?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readBoolean", "()Z") => {
            let mut st = load(vm, recv);
            let v = st.read_i32()?;
            store(vm, recv, st);
            return some(Value::Int((v != 0) as i32));
        }
        ("writeString", "(Ljava/lang/String;)V") => {
            let s = str_of(vm, args.first())?;
            write_string_to(vm, recv, s.as_deref())?;
            return some(Value::Null);
        }
        ("readString", "()Ljava/lang/String;") => {
            let mut st = load(vm, recv);
            let s = st.read_string()?;
            store(vm, recv, st);
            return some(jstring(vm, s)?);
        }
        ("writeString8", "(Ljava/lang/String;)V") => {
            let s = str_of(vm, args.first())?;
            let mut st = load(vm, recv);
            st.write_string8(s.as_deref())?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readString8", "()Ljava/lang/String;") => {
            let mut st = load(vm, recv);
            let s = st.read_string8()?;
            store(vm, recv, st);
            return some(jstring(vm, s)?);
        }
        ("writeCharSequence", "(Ljava/lang/CharSequence;)V") => {
            let s = char_seq_of(vm, args.first())?;
            write_string_to(vm, recv, s.as_deref())?;
            return some(Value::Null);
        }
        ("readCharSequence", "()Ljava/lang/CharSequence;") => {
            let mut st = load(vm, recv);
            let s = st.read_string()?;
            store(vm, recv, st);
            return some(jstring(vm, s)?);
        }
        ("writeByteArray", "([B)V") | ("writeBlob", "([B)V") => {
            let b = bytes_of(vm, args.first())?;
            let mut st = load(vm, recv);
            st.write_byte_blob(b.as_deref())?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("writeByteArray", "([BII)V") => {
            let Some(b) = bytes_of(vm, args.first())? else {
                return Err(npe("Parcel.writeByteArray: array is null"));
            };
            let off = int_arg(args, 1);
            let len = int_arg(args, 2);
            if off < 0 || len < 0 || off as usize > b.len() || len as usize > b.len() - off as usize
            {
                return Err(iae("Parcel.writeByteArray: offset/length out of bounds"));
            }
            let mut st = load(vm, recv);
            st.write_byte_blob(Some(&b[off as usize..off as usize + len as usize]))?;
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createByteArray", "()[B") | ("readBlob", "()[B") => {
            let mut st = load(vm, recv);
            let b = st.read_byte_blob()?;
            store(vm, recv, st);
            return some(match b {
                Some(b) => crate::jlstring::byte_array(vm, &b)?,
                None => Value::Null,
            });
        }
        ("readByteArray", "([B)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readByteArray: destination is null"));
            };
            let mut st = load(vm, recv);
            let b = st.read_byte_blob()?.unwrap_or_default();
            store(vm, recv, st);
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != b.len() {
                return Err(bad(format!(
                    "Parcel.readByteArray: destination length {} != parcel length {}",
                    slot.len(),
                    b.len()
                )));
            }
            for (i, byte) in b.iter().enumerate() {
                slot[i] = Value::Int(*byte as i8 as i32);
            }
            return some(Value::Null);
        }
        ("writeStrongBinder", "(Landroid/os/IBinder;)V") => {
            write_strong_binder_to(
                vm,
                recv,
                args.first().cloned().unwrap_or(Value::Null).as_ref_value(),
            )?;
            return some(Value::Null);
        }
        ("readStrongBinder", "()Landroid/os/IBinder;") => {
            return some(read_strong_binder_from(vm, recv)?);
        }
        ("writeStrongInterface", "(Landroid/os/IInterface;)V") => {
            write_strong_interface_to(
                vm,
                recv,
                args.first().cloned().unwrap_or(Value::Null).as_ref_value(),
            )?;
            return some(Value::Null);
        }
        ("writeValue", "(Ljava/lang/Object;)V") => {
            write_value(
                vm,
                recv,
                args.first().cloned().unwrap_or(Value::Null).as_ref_value(),
            )?;
            return some(Value::Null);
        }
        ("readValue", "(Ljava/lang/ClassLoader;)Ljava/lang/Object;") => {
            return some(read_value(vm, recv)?);
        }
        ("writeParcelable", "(Landroid/os/Parcelable;I)V") => {
            write_parcelable(
                vm,
                recv,
                args.first().cloned().unwrap_or(Value::Null).as_ref_value(),
                int_arg(args, 1),
            )?;
            return some(Value::Null);
        }
        ("readParcelable", "(Ljava/lang/ClassLoader;)Landroid/os/Parcelable;")
        | ("readParcelable", "(Ljava/lang/ClassLoader;Ljava/lang/Class;)Ljava/lang/Object;") => {
            return some(read_parcelable(vm, recv)?);
        }
        ("writeBundle", "(Landroid/os/Bundle;)V") => {
            write_nullable_bundle(vm, recv, obj_arg(args, 0))?;
            return some(Value::Null);
        }
        ("readBundle", "()Landroid/os/Bundle;")
        | ("readBundle", "(Ljava/lang/ClassLoader;)Landroid/os/Bundle;") => {
            return some(read_nullable_bundle(vm, recv, BUNDLE)?);
        }
        ("writeSparseArray", "(Landroid/util/SparseArray;)V") => {
            write_sparse_array(vm, recv, obj_arg(args, 0))?;
            return some(Value::Null);
        }
        ("readSparseArray", "(Ljava/lang/ClassLoader;)Landroid/util/SparseArray;")
        | ("readSparseArray", "(Ljava/lang/ClassLoader;Ljava/lang/Class;)Ljava/lang/Object;") => {
            return some(read_sparse_array(vm, recv)?);
        }
        ("writeSparseIntArray", "(Landroid/util/SparseIntArray;)V") => {
            // AOSP: -1 for null, else count then key/value int pairs in
            // ascending-key order.
            match obj_arg(args, 0) {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(sparse) => {
                    let entries = sparse_int_entries(vm, sparse)?;
                    let mut st = load(vm, recv);
                    st.write_i32(entries.len() as i32)?;
                    for (key, value) in entries {
                        st.write_i32(key)?;
                        st.write_i32(value)?;
                    }
                    store(vm, recv, st);
                }
            }
            return some(Value::Null);
        }
        ("readSparseIntArray", "()Landroid/util/SparseIntArray;") => {
            let Some(n) = read_typed_len(vm, recv, 8, "SparseIntArray")? else {
                return some(Value::Null);
            };
            let mut entries = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, recv);
            for _ in 0..n {
                let key = st.read_i32()?;
                let value = st.read_i32()?;
                entries.push((key, Value::Int(value)));
            }
            store(vm, recv, st);
            return some(new_sparse(vm, "Landroid/util/SparseIntArray;", entries)?);
        }
        ("writeSparseBooleanArray", "(Landroid/util/SparseBooleanArray;)V") => {
            write_sparse_boolean_array(vm, recv, obj_arg(args, 0))?;
            return some(Value::Null);
        }
        ("readSparseBooleanArray", "()Landroid/util/SparseBooleanArray;")
        | (
            "readSparseBooleanArray",
            "(Ljava/lang/ClassLoader;)Landroid/util/SparseBooleanArray;",
        ) => {
            return some(read_sparse_boolean_array(vm, recv)?);
        }
        ("writePersistableBundle", "(Landroid/os/PersistableBundle;)V") => {
            write_nullable_bundle(vm, recv, obj_arg(args, 0))?;
            return some(Value::Null);
        }
        ("readPersistableBundle", "()Landroid/os/PersistableBundle;")
        | ("readPersistableBundle", "(Ljava/lang/ClassLoader;)Landroid/os/PersistableBundle;") => {
            return some(read_nullable_bundle(vm, recv, PERSISTABLE_BUNDLE)?);
        }
        ("writeList", "(Ljava/util/List;)V") | ("writeArrayList", "(Ljava/util/ArrayList;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        write_value(vm, recv, e)?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("readList", "(Ljava/util/List;Ljava/lang/ClassLoader;)V")
        | ("readList", "(Ljava/util/List;Ljava/lang/ClassLoader;Ljava/lang/Class;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readList: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_value(vm, recv)?);
            }
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        ("readArrayList", "(Ljava/lang/ClassLoader;)Ljava/util/ArrayList;")
        | ("readArrayList", "(Ljava/lang/ClassLoader;Ljava/lang/Class;)Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_value(vm, recv)?);
            }
            return some(new_list(vm, vals)?);
        }
        ("writeArray", "([Ljava/lang/Object;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        write_value(vm, recv, e)?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("readArray", "(Ljava/lang/ClassLoader;)[Ljava/lang/Object;")
        | ("readArray", "(Ljava/lang/ClassLoader;Ljava/lang/Class;)[Ljava/lang/Object;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_value(vm, recv)?);
            }
            return some(crate::arrays::new_array(
                vm,
                "[Ljava/lang/Object;",
                ElemKind::Obj,
                vals,
            )?);
        }
        ("writeMap", "(Ljava/util/Map;)V") | ("writeHashMap", "(Ljava/util/HashMap;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match map_entries(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(entries) => {
                    let mut st = load(vm, recv);
                    st.write_i32(entries.len() as i32)?;
                    store(vm, recv, st);
                    for (k, val) in &entries {
                        write_value(vm, recv, k)?;
                        write_value(vm, recv, val)?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("readMap", "(Ljava/util/Map;Ljava/lang/ClassLoader;)V")
        | (
            "readMap",
            "(Ljava/util/Map;Ljava/lang/ClassLoader;Ljava/lang/Class;Ljava/lang/Class;)V",
        )
        | ("readHashMap", "(Ljava/lang/ClassLoader;)Ljava/util/HashMap;") => {
            let dest = if name == "readHashMap" {
                None
            } else {
                Some(obj_arg(args, 0).ok_or_else(|| npe("Parcel.readMap: destination is null"))?)
            };
            let Some(n) = read_typed_len(vm, recv, 8, "map")? else {
                return some(Value::Null);
            };
            let mut entries = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let k = read_value(vm, recv)?;
                let v = read_value(vm, recv)?;
                entries.push((k, v));
            }
            return some(match dest {
                Some(d) => {
                    fill_map(vm, d, entries)?;
                    Value::Null
                }
                None => new_map(vm, entries)?,
            });
        }
        ("writeTypedList", "(Ljava/util/List;)V") | ("writeTypedList", "(Ljava/util/List;I)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            let flags = if sig == "(Ljava/util/List;I)V" {
                int_arg(args, 1)
            } else {
                0
            };
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    write_present_typed(vm, recv, &elems, flags)?;
                }
            }
            return some(Value::Null);
        }
        ("createTypedArrayList", "(Landroid/os/Parcelable$Creator;)Ljava/util/ArrayList;") => {
            let Some(creator) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createTypedArrayList: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "typed list")? else {
                return some(Value::Null);
            };
            let vals = read_present_typed(vm, recv, creator, n)?;
            return some(new_list(vm, vals)?);
        }
        ("readTypedList", "(Ljava/util/List;Landroid/os/Parcelable$Creator;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readTypedList: destination is null"));
            };
            let Some(creator) = obj_arg(args, 1) else {
                return Err(npe("Parcel.readTypedList: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "typed list")? else {
                return some(Value::Null);
            };
            let vals = read_present_typed(vm, recv, creator, n)?;
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        ("writeTypedArray", "([Ljava/lang/Object;I)V")
        | ("writeTypedArray", "([Landroid/os/Parcelable;I)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    write_present_typed(vm, recv, &elems, int_arg(args, 1))?;
                }
            }
            return some(Value::Null);
        }
        ("writeArrayMap", "(Landroid/util/ArrayMap;)V") => {
            // AOSP `writeArrayMapInternal`: -1 for null, else the entry
            // count followed by String key / writeValue pairs in the map's
            // (hash-sorted) iteration order.
            match obj_arg(args, 0) {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(map) => {
                    let entries = match vm.fw.get(map) {
                        Some(HostObj::Map { entries }) => entries.clone(),
                        _ => {
                            return Err(bad(
                                "Parcel.writeArrayMap: only host ArrayMap state can be written",
                            ))
                        }
                    };
                    let mut st = load(vm, recv);
                    st.write_i32(entries.len() as i32)?;
                    store(vm, recv, st);
                    for (k, v) in &entries {
                        let key = match k {
                            Value::Obj(o) => vm.heap.as_str(*o).ok().map(str::to_string),
                            _ => None,
                        };
                        write_string_to(vm, recv, key.as_deref())?;
                        write_value(vm, recv, v)?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("readArrayMap", "(Landroid/util/ArrayMap;Ljava/lang/ClassLoader;)V") => {
            // AOSP appends the parcelled entries to the destination, in
            // stream order (the producer wrote them hash-sorted).
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readArrayMap: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 8, "ArrayMap")? else {
                return some(Value::Null);
            };
            let mut pairs = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let key = read_string_from(vm, recv)?;
                let value = read_value(vm, recv)?;
                pairs.push((jstring(vm, key)?, value));
            }
            if matches!(vm.fw.get(dest), Some(HostObj::Map { .. }) | None) {
                let mut entries = match vm.fw.get(dest) {
                    Some(HostObj::Map { entries }) => entries.clone(),
                    _ => Vec::new(),
                };
                entries.extend(pairs);
                vm.fw.objects.insert(dest, HostObj::Map { entries });
            } else {
                for (k, v) in pairs {
                    vm.invoke_instance(
                        dest,
                        "append",
                        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                        &[k, v],
                    )?;
                }
            }
            return some(Value::Null);
        }
        ("writeTypedArrayMap", "(Landroid/util/ArrayMap;I)V") => {
            match obj_arg(args, 0) {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(map) => {
                    let entries = match vm.fw.get(map) {
                        Some(HostObj::Map { entries }) => entries.clone(),
                        _ => {
                            return Err(bad(
                                "Parcel.writeTypedArrayMap: only host ArrayMap state can be written",
                            ))
                        }
                    };
                    let mut st = load(vm, recv);
                    st.write_i32(entries.len() as i32)?;
                    store(vm, recv, st);
                    for (k, v) in &entries {
                        let key = match k {
                            Value::Obj(o) => vm.heap.as_str(*o).ok().map(str::to_string),
                            _ => None,
                        };
                        write_string_to(vm, recv, key.as_deref())?;
                        write_present_typed(vm, recv, std::slice::from_ref(v), int_arg(args, 1))?;
                    }
                }
            }
            return some(Value::Null);
        }
        (
            "createTypedArrayMap",
            "(Landroid/os/Parcelable$Creator;)Landroid/util/ArrayMap;",
        ) => {
            let Some(creator) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createTypedArrayMap: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 8, "typed ArrayMap")? else {
                return some(Value::Null);
            };
            let mut entries = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let key = read_string_from(vm, recv)?;
                let mut value = read_present_typed(vm, recv, creator, 1)?;
                entries.push((jstring(vm, key)?, value.pop().unwrap_or(Value::Null)));
            }
            let map = vm
                .heap
                .alloc_instance("Landroid/util/ArrayMap;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            vm.fw.objects.insert(map, HostObj::Map { entries });
            return some(Value::Obj(map));
        }
        ("writeArraySet", "(Landroid/util/ArraySet;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        write_value(vm, recv, e)?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("readArraySet", "(Ljava/lang/ClassLoader;)Landroid/util/ArraySet;") => {
            // AOSP `readArraySet` appends in stream order; the producer
            // wrote the set's hash-sorted order, which the host list
            // representation preserves verbatim.
            let Some(n) = read_typed_len(vm, recv, 4, "ArraySet")? else {
                return some(Value::Null);
            };
            let mut elems = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                elems.push(read_value(vm, recv)?);
            }
            let set = vm
                .heap
                .alloc_instance("Landroid/util/ArraySet;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            vm.fw.objects.insert(set, HostObj::List { elems });
            return some(Value::Obj(set));
        }
        ("createTypedArray", "(Landroid/os/Parcelable$Creator;)[Ljava/lang/Object;") => {
            let Some(creator) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createTypedArray: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "typed array")? else {
                return some(Value::Null);
            };
            let vals = read_present_typed(vm, recv, creator, n)?;
            let arr = creator_new_array(vm, creator, n as i32).unwrap_or(Value::Null);
            if let Value::Obj(a) = arr {
                if let Ok(slot) = vm.heap.array_elems_mut(a) {
                    slot.clear();
                    slot.extend(vals);
                    return some(Value::Obj(a));
                }
            }
            return some(crate::arrays::new_array(
                vm,
                "[Ljava/lang/Object;",
                ElemKind::Obj,
                vals,
            )?);
        }
        ("readTypedArray", "([Ljava/lang/Object;Landroid/os/Parcelable$Creator;)V")
        | ("readTypedArray", "([Landroid/os/Parcelable;Landroid/os/Parcelable$Creator;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readTypedArray: destination is null"));
            };
            let Some(creator) = obj_arg(args, 1) else {
                return Err(npe("Parcel.readTypedArray: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "typed array")? else {
                return some(Value::Null);
            };
            let vals = read_present_typed(vm, recv, creator, n)?;
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad(format!(
                    "Parcel.readTypedArray: destination length {} != parcel length {}",
                    slot.len(),
                    vals.len()
                )));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeIntArray", "([I)V") | ("writeCharArray", "([C)V") | ("writeShortArray", "([S)V") => {
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1)?,
                Some(es) => {
                    st.write_i32(es.len() as i32)?;
                    for e in &es {
                        st.write_i32(e.as_int().unwrap_or(0))?;
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createIntArray", "()[I") | ("createCharArray", "()[C") | ("createShortArray", "()[S") => {
            let Some(n) = read_typed_len(vm, recv, 4, "int array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Int(st.read_i32()?));
            }
            store(vm, recv, st);
            return some(crate::arrays::new_array(
                vm,
                match name {
                    "createCharArray" => "[C",
                    "createShortArray" => "[S",
                    _ => "[I",
                },
                ElemKind::Int,
                vals,
            )?);
        }
        ("readIntArray", "([I)V") | ("readCharArray", "([C)V") | ("readShortArray", "([S)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readIntArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "int array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Int(st.read_i32()?));
            }
            store(vm, recv, st);
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readIntArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeLongArray", "([J)V") => {
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1)?,
                Some(es) => {
                    st.write_i32(es.len() as i32)?;
                    for e in &es {
                        st.write_i64(e.as_long().unwrap_or(0))?;
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createLongArray", "()[J") => {
            let Some(n) = read_typed_len(vm, recv, 8, "long array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Long(st.read_i64()?));
            }
            store(vm, recv, st);
            return some(crate::arrays::new_array(vm, "[J", ElemKind::Long, vals)?);
        }
        ("readLongArray", "([J)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readLongArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 8, "long array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Long(st.read_i64()?));
            }
            store(vm, recv, st);
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readLongArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeFloatArray", "([F)V") => {
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1)?,
                Some(es) => {
                    st.write_i32(es.len() as i32)?;
                    for e in &es {
                        st.write_i32(e.as_float().unwrap_or(0.0).to_bits() as i32)?;
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createFloatArray", "()[F") => {
            let Some(n) = read_typed_len(vm, recv, 4, "float array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Float(f32::from_bits(st.read_i32()? as u32)));
            }
            store(vm, recv, st);
            return some(crate::arrays::new_array(vm, "[F", ElemKind::Float, vals)?);
        }
        ("readFloatArray", "([F)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readFloatArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "float array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Float(f32::from_bits(st.read_i32()? as u32)));
            }
            store(vm, recv, st);
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readFloatArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeDoubleArray", "([D)V") => {
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1)?,
                Some(es) => {
                    st.write_i32(es.len() as i32)?;
                    for e in &es {
                        st.write_i64(e.as_double().unwrap_or(0.0).to_bits() as i64)?;
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createDoubleArray", "()[D") => {
            let Some(n) = read_typed_len(vm, recv, 8, "double array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Double(f64::from_bits(st.read_i64()? as u64)));
            }
            store(vm, recv, st);
            return some(crate::arrays::new_array(vm, "[D", ElemKind::Double, vals)?);
        }
        ("readDoubleArray", "([D)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readDoubleArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 8, "double array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(Value::Double(f64::from_bits(st.read_i64()? as u64)));
            }
            store(vm, recv, st);
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readDoubleArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeBooleanArray", "([Z)V") => {
            let elems = array_elems(vm, args.first())?;
            write_bool_array_to(vm, recv, elems.as_deref())?;
            return some(Value::Null);
        }
        ("createBooleanArray", "()[Z") => {
            let Some(vals) = read_bool_array_from(vm, recv)? else {
                return some(Value::Null);
            };
            return some(crate::arrays::new_array(vm, "[Z", ElemKind::Int, vals)?);
        }
        ("readBooleanArray", "([Z)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readBooleanArray: destination is null"));
            };
            let Some(vals) = read_bool_array_from(vm, recv)? else {
                return some(Value::Null);
            };
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readBooleanArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeStringArray", "([Ljava/lang/String;)V")
        | ("writeString16Array", "([Ljava/lang/String;)V")
        | ("writeCharSequenceArray", "([Ljava/lang/CharSequence;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        let s = char_seq_of(vm, Some(e))?;
                        write_string_to(vm, recv, s.as_deref())?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("createStringArray", "()[Ljava/lang/String;")
        | ("createString16Array", "()[Ljava/lang/String;")
        | ("createCharSequenceArray", "()[Ljava/lang/CharSequence;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "string array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let s = st.read_string()?;
                store(vm, recv, st);
                vals.push(jstring(vm, s)?);
            }
            return some(crate::arrays::new_array(
                vm,
                if name == "createCharSequenceArray" {
                    "[Ljava/lang/CharSequence;"
                } else {
                    "[Ljava/lang/String;"
                },
                ElemKind::Obj,
                vals,
            )?);
        }
        ("readStringArray", "([Ljava/lang/String;)V")
        | ("readString16Array", "([Ljava/lang/String;)V")
        | ("readCharSequenceArray", "([Ljava/lang/CharSequence;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readStringArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "string array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let s = st.read_string()?;
                store(vm, recv, st);
                vals.push(jstring(vm, s)?);
            }
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readStringArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeString8Array", "([Ljava/lang/String;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut strings = Vec::with_capacity(bounded_cap(elems.len()));
                    for e in &elems {
                        strings.push(str_of(vm, Some(e))?);
                    }
                    let mut st = load(vm, recv);
                    st.write_i32(strings.len() as i32)?;
                    for s in &strings {
                        st.write_string8(s.as_deref())?;
                    }
                    store(vm, recv, st);
                }
            }
            return some(Value::Null);
        }
        ("createString8Array", "()[Ljava/lang/String;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "string8 array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, recv);
            for _ in 0..n {
                vals.push(st.read_string8()?);
            }
            store(vm, recv, st);
            let mut out = Vec::with_capacity(bounded_cap(n));
            for s in vals {
                out.push(jstring(vm, s)?);
            }
            return some(crate::arrays::new_array(
                vm,
                "[Ljava/lang/String;",
                ElemKind::Obj,
                out,
            )?);
        }
        ("readString8Array", "([Ljava/lang/String;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readString8Array: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "string8 array")? else {
                return some(Value::Null);
            };
            let mut strs = Vec::with_capacity(bounded_cap(n));
            let mut st = load(vm, recv);
            for _ in 0..n {
                strs.push(st.read_string8()?);
            }
            store(vm, recv, st);
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for s in strs {
                vals.push(jstring(vm, s)?);
            }
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readString8Array: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeStringList", "(Ljava/util/List;)V")
        | ("writeCharSequenceList", "(Ljava/util/List;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        let s = char_seq_of(vm, Some(e))?;
                        write_string_to(vm, recv, s.as_deref())?;
                    }
                }
            }
            return some(Value::Null);
        }
        ("createStringArrayList", "()Ljava/util/ArrayList;")
        | ("createCharSequenceArrayList", "()Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "string list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let s = st.read_string()?;
                store(vm, recv, st);
                vals.push(jstring(vm, s)?);
            }
            return some(new_list(vm, vals)?);
        }
        ("readStringList", "(Ljava/util/List;)V")
        | ("readCharSequenceList", "(Ljava/util/List;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readStringList: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "string list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let s = st.read_string()?;
                store(vm, recv, st);
                vals.push(jstring(vm, s)?);
            }
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        ("writeIBinderArray", "([Landroid/os/IBinder;)V")
        | ("writeBinderArray", "([Landroid/os/IBinder;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        let h = binder_handle(vm, e);
                        let mut st = load(vm, recv);
                        st.write_i32(h)?;
                        store(vm, recv, st);
                    }
                }
            }
            return some(Value::Null);
        }
        ("createIBinderArray", "()[Landroid/os/IBinder;")
        | ("createBinderArray", "()[Landroid/os/IBinder;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "binder array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let h = st.read_i32()?;
                store(vm, recv, st);
                vals.push(binder_for_handle(vm, h)?);
            }
            return some(crate::arrays::new_array(
                vm,
                "[Landroid/os/IBinder;",
                ElemKind::Obj,
                vals,
            )?);
        }
        ("readIBinderArray", "([Landroid/os/IBinder;)V")
        | ("readBinderArray", "([Landroid/os/IBinder;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readIBinderArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "binder array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let h = st.read_i32()?;
                store(vm, recv, st);
                vals.push(binder_for_handle(vm, h)?);
            }
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readIBinderArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeInterfaceArray", "([Landroid/os/IInterface;)V") => {
            // AOSP: null writes -1, otherwise the length followed by one
            // strong Binder per element (`writeStrongInterface`).
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        write_strong_interface_to(vm, recv, e)?;
                    }
                }
            }
            return some(Value::Null);
        }
        (
            "createInterfaceArray",
            "(Ljava/util/function/IntFunction;Ljava/util/function/Function;)[Landroid/os/IInterface;",
        ) => {
            // AOSP: a negative count returns null; otherwise the guest
            // IntFunction allocates the typed array and each element is
            // `asInterface.apply(readStrongBinder())`.
            let Some(factory) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createInterfaceArray: factory is null"));
            };
            let Some(as_interface) = obj_arg(args, 1) else {
                return Err(npe("Parcel.createInterfaceArray: asInterface is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "interface array")? else {
                return some(Value::Null);
            };
            let array = match vm.invoke_instance(
                factory,
                "apply",
                "(I)Ljava/lang/Object;",
                &[Value::Int(n as i32)],
            )? {
                Value::Obj(r) => r,
                _ => {
                    return Err(bad(
                        "Parcel.createInterfaceArray: factory did not return an array",
                    ))
                }
            };
            if vm.heap.as_array_elems(array)?.0.len() != n {
                return Err(bad(
                    "Parcel.createInterfaceArray: factory returned an array of the wrong length",
                ));
            }
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_strong_interface_with(vm, recv, as_interface)?);
            }
            vm.heap.array_elems_mut(array)?.clone_from(&vals);
            return some(Value::Obj(array));
        }
        (
            "readInterfaceArray",
            "([Landroid/os/IInterface;Ljava/util/function/Function;)V",
        ) => {
            // AOSP fills the destination in place and rejects a parcelled
            // count larger than the destination array.
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readInterfaceArray: destination is null"));
            };
            let Some(as_interface) = obj_arg(args, 1) else {
                return Err(npe("Parcel.readInterfaceArray: asInterface is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "interface array")? else {
                return some(Value::Null);
            };
            if n > vm.heap.as_array_elems(dest)?.0.len() {
                return Err(bad(
                    "Parcel.readInterfaceArray: parcelled count exceeds destination length",
                ));
            }
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_strong_interface_with(vm, recv, as_interface)?);
            }
            vm.heap.array_elems_mut(dest)?[..n].clone_from_slice(&vals);
            return some(Value::Null);
        }
        ("writeInterfaceList", "(Ljava/util/List;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        write_strong_interface_to(vm, recv, e)?;
                    }
                }
            }
            return some(Value::Null);
        }
        (
            "createInterfaceArrayList",
            "(Ljava/util/function/Function;)Ljava/util/ArrayList;",
        ) => {
            let Some(as_interface) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createInterfaceArrayList: asInterface is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "interface list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_strong_interface_with(vm, recv, as_interface)?);
            }
            return some(new_list(vm, vals)?);
        }
        ("readInterfaceList", "(Ljava/util/List;Ljava/util/function/Function;)V") => {
            // AOSP reuses destination slots (`set`), appends (`add`) or
            // truncates (`remove`) so the destination ends with exactly the
            // parcelled elements; replacing the host contents has the same
            // observable result while keeping the same list object.
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readInterfaceList: destination is null"));
            };
            let Some(as_interface) = obj_arg(args, 1) else {
                return Err(npe("Parcel.readInterfaceList: asInterface is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "interface list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                vals.push(read_strong_interface_with(vm, recv, as_interface)?);
            }
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        ("writeBinderList", "(Ljava/util/List;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1)?;
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32)?;
                    store(vm, recv, st);
                    for e in &elems {
                        let h = binder_handle(vm, e);
                        let mut st = load(vm, recv);
                        st.write_i32(h)?;
                        store(vm, recv, st);
                    }
                }
            }
            return some(Value::Null);
        }
        ("createBinderArrayList", "()Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, 4, "binder list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let h = st.read_i32()?;
                store(vm, recv, st);
                vals.push(binder_for_handle(vm, h)?);
            }
            return some(new_list(vm, vals)?);
        }
        ("readBinderList", "(Ljava/util/List;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readBinderList: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, 4, "binder list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(bounded_cap(n));
            for _ in 0..n {
                let mut st = load(vm, recv);
                let h = st.read_i32()?;
                store(vm, recv, st);
                vals.push(binder_for_handle(vm, h)?);
            }
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        _ => None,
    })
}

// Small extension used above to keep `args.first().cloned()` ergonomic when a
// helper wants `&Value` after the argument vector has been consumed by a
// nested guest call. It is just identity on an owned clone's reference in
// the call expression; helpers clone what they need before invoking guest
// code.
trait AsRefValue {
    fn as_ref_value(&self) -> &Value;
}
impl AsRefValue for Value {
    fn as_ref_value(&self) -> &Value {
        self
    }
}

fn binder_descriptor(vm: &Engine, recv: ObjRef) -> Option<String> {
    match vm.heap.get_field(recv, "rd$binderDescriptor") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}
fn binder_owner(vm: &Engine, recv: ObjRef) -> Value {
    vm.heap
        .get_field(recv, "rd$binderOwner")
        .unwrap_or(Value::Null)
}
fn put_field(vm: &mut Engine, recv: ObjRef, name: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(recv, name, v))
}

fn write_descriptor_reply(
    vm: &mut Engine,
    recv: ObjRef,
    reply: Option<&Value>,
) -> Result<bool, VmExit> {
    if let Some(Value::Obj(r)) = reply {
        let d = binder_descriptor(vm, recv);
        write_string_to(vm, *r, d.as_deref())?;
    }
    Ok(true)
}

fn binder_instance(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_binder = recv_class == BINDER || vm.cp.is_subtype(recv_class, BINDER);
    if !is_binder {
        return Ok(None);
    }
    // Virtual guest overrides (Stub.onTransact, a custom dump) must win over
    // the Binder base behaviour when the call is not an explicit super call.
    if matches!(name, "onTransact" | "dump" | "dumpAsync")
        && mref_class != BINDER
        && vm.cp.resolve_method(recv_class, name, sig).is_some()
    {
        return Ok(None);
    }
    Ok(match (name, sig) {
        ("<init>", "()V") => Some(Value::Null),
        ("<init>", "(Ljava/lang/String;)V") => {
            if let Some(s) = args.first().cloned() {
                put_field(vm, recv, "rd$binderDescriptor", s)?;
            }
            Some(Value::Null)
        }
        ("attachInterface", "(Landroid/os/IInterface;Ljava/lang/String;)V") => {
            put_field(
                vm,
                recv,
                "rd$binderOwner",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put_field(
                vm,
                recv,
                "rd$binderDescriptor",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Some(Value::Null)
        }
        ("queryLocalInterface", "(Ljava/lang/String;)Landroid/os/IInterface;") => {
            let want = str_of(vm, args.first())?;
            Some(if want.is_some() && binder_descriptor(vm, recv) == want {
                binder_owner(vm, recv)
            } else {
                Value::Null
            })
        }
        ("getInterfaceDescriptor", "()Ljava/lang/String;") => {
            Some(jstring(vm, binder_descriptor(vm, recv))?)
        }
        ("isBinderAlive", "()Z") | ("pingBinder", "()Z") => Some(Value::Int(1)),
        // AOSP's local `Binder.linkToDeath` is documented as a no-op (a
        // local object cannot die), and `unlinkToDeath` returns true.
        ("linkToDeath", "(Landroid/os/IBinder$DeathRecipient;I)V") => Some(Value::Null),
        ("unlinkToDeath", "(Landroid/os/IBinder$DeathRecipient;I)Z") => Some(Value::Int(1)),
        ("transact", "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z") => {
            let code = int_arg(args, 0);
            if code == INTERFACE_TRANSACTION {
                return Ok(Some(Value::Int(
                    write_descriptor_reply(vm, recv, args.get(2))? as i32,
                )));
            }
            if code == DUMP_TRANSACTION {
                return Ok(Some(Value::Int(0)));
            }
            if vm
                .cp
                .resolve_method(recv_class, "onTransact", ON_TRANSACT_SIG)
                .is_some()
            {
                let r = vm.invoke_instance(recv, "onTransact", ON_TRANSACT_SIG, args)?;
                return Ok(Some(r));
            }
            Some(Value::Int(0))
        }
        ("onTransact", ON_TRANSACT_SIG) => {
            let code = int_arg(args, 0);
            if code == INTERFACE_TRANSACTION {
                Some(Value::Int(
                    write_descriptor_reply(vm, recv, args.get(2))? as i32
                ))
            } else {
                Some(Value::Int(0))
            }
        }
        ("dump", "(Ljava/io/FileDescriptor;[Ljava/lang/String;)V")
        | ("dumpAsync", "(Ljava/io/FileDescriptor;[Ljava/lang/String;)V") => Some(Value::Null),
        _ => None,
    })
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Parcel/Binder dispatch is consulted before the core java.lang handler;
    // avoid a host-object map lookup for ordinary non-Android receivers.
    // Host Parcels are always allocated with the Parcel descriptor, and
    // Binder calls name Binder/IBinder (or the Binder receiver class).
    if !mref_class.starts_with("Landroid/os/") && !recv_class.starts_with("Landroid/os/") {
        return Ok(None);
    }
    if mref_class == PARCEL
        || recv_class == PARCEL
        || matches!(vm.fw.get(recv), Some(HostObj::Parcel { .. }))
    {
        return parcel_instance(vm, name, sig, recv, args);
    }
    if mref_class == BINDER || mref_class == IBINDER || recv_class == BINDER {
        return binder_instance(vm, recv_class, mref_class, name, sig, recv, args);
    }
    Ok(None)
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(match (class, name, sig) {
        (PARCEL, "obtain", "()Landroid/os/Parcel;") => Some(new_parcel(vm)?),
        (PARCEL, "compareData", "(Landroid/os/Parcel;ILandroid/os/Parcel;II)Z") => {
            let (Some(a), Some(b)) = (obj_arg(args, 0), obj_arg(args, 2)) else {
                return Err(npe("Parcel.compareData: parcel is null"));
            };
            let (off_a, off_b, len) = (int_arg(args, 1), int_arg(args, 3), int_arg(args, 4));
            let da = load(vm, a).data;
            let db = load(vm, b).data;
            let slice = |d: &Vec<u8>, off: i32| -> Option<Vec<u8>> {
                if off < 0 || len < 0 {
                    return None;
                }
                let (off, len) = (off as usize, len as usize);
                let end = off.checked_add(len)?;
                d.get(off..end).map(<[u8]>::to_vec)
            };
            let equal = match (slice(&da, off_a), slice(&db, off_b)) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            };
            Some(Value::Int(equal as i32))
        }
        (BINDER, "getCallingPid", "()I") => Some(Value::Int(10000)),
        (BINDER, "getCallingUid", "()I") => Some(Value::Int(10000)),
        (BINDER, "getCallingUidOrThrow", "()I") => Some(Value::Int(10000)),
        (BINDER, "clearCallingIdentity", "()J") => Some(Value::Long(0)),
        (BINDER, "restoreCallingIdentity", "(J)V") => {
            let _ = args;
            Some(Value::Null)
        }
        (BINDER, "flushPendingCommands", "()V") => Some(Value::Null),
        (BINDER, "joinThreadPool", "()V") => Some(Value::Null),
        _ => None,
    })
}

/// `sget` materialization for IBinder/Binder transaction constants. Most
/// compilers inline these `static final int`s, but reflective or hand-built
/// DEX may still read them.
pub(crate) fn host_static(
    _vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    if owner != IBINDER && owner != BINDER {
        return Ok(None);
    }
    Ok(match field {
        "FIRST_CALL_TRANSACTION" => Some(Value::Int(1)),
        "LAST_CALL_TRANSACTION" => Some(Value::Int(0x00ff_ffff)),
        "INTERFACE_TRANSACTION" => Some(Value::Int(INTERFACE_TRANSACTION)),
        "DUMP_TRANSACTION" => Some(Value::Int(DUMP_TRANSACTION)),
        "FLAG_ONEWAY" => Some(Value::Int(1)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::intrinsics::{alloc_string, call_instance_intrinsic};

    fn engine() -> Engine {
        Engine::new(vec![], Default::default())
    }
    fn obj(v: Value) -> ObjRef {
        match v {
            Value::Obj(r) => r,
            other => panic!("expected object, got {other:?}"),
        }
    }
    fn s(vm: &mut Engine, text: &str) -> Value {
        Value::Obj(alloc_string(vm, text.to_string()).unwrap())
    }
    fn callp(vm: &mut Engine, p: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
        call_instance_intrinsic(vm, PARCEL, name, sig, p, args)
            .unwrap()
            .unwrap_or_else(|| panic!("Parcel.{name}{sig} not handled"))
    }
    fn callb(vm: &mut Engine, b: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
        call_instance_intrinsic(vm, BINDER, name, sig, b, args)
            .unwrap()
            .unwrap_or_else(|| panic!("Binder.{name}{sig} not handled"))
    }

    #[test]
    fn primitive_and_string_layout_is_android_shaped() {
        let mut st = PState::default();
        assert!(st.write_i32(0x0102_0304).is_ok());
        assert!(st.write_string(Some("Aé")).is_ok());
        assert!(st.write_byte_blob(Some(&[1, 2, 3, 4, 5])).is_ok());
        // int LE; string: len=2 UTF-16 chars; 'A' 41 00; 'é' e9 00; term 00 00;
        // byte array len=5 then 5 bytes + 3 pad.
        assert_eq!(&st.data[0..4], &[4, 3, 2, 1]);
        assert_eq!(&st.data[4..8], &[2, 0, 0, 0]);
        assert_eq!(&st.data[8..14], &[0x41, 0, 0xe9, 0, 0, 0]);
        assert_eq!(st.data.len(), 4 + 4 + 8 + 4 + 8);
        st.pos = 0;
        assert_eq!(st.read_i32().unwrap(), 0x0102_0304);
        assert_eq!(st.read_string().unwrap().as_deref(), Some("Aé"));
        assert_eq!(st.read_byte_blob().unwrap().unwrap(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn underrun_is_typed_not_a_panic() {
        let mut st = PState::default();
        assert!(st.write_i32(7).is_ok());
        st.pos = 0;
        assert_eq!(st.read_i32().unwrap(), 7);
        assert!(st.read_string().is_err());
    }

    #[test]
    fn parcel_public_surface_roundtrips_through_marshalled_bytes() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());
        let text = s(&mut vm, "héllo 📦");
        let blob = crate::jlstring::byte_array(&mut vm, &[9, 8, 7, 6, 5]).unwrap();
        let boxed = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(4242)).unwrap(),
        );
        let list_text = s(&mut vm, "inside-list");
        let list = new_list(&mut vm, vec![boxed.clone(), list_text]).unwrap();

        callp(&mut vm, p, "writeInt", "(I)V", &[Value::Int(-7)]);
        callp(
            &mut vm,
            p,
            "writeLong",
            "(J)V",
            &[Value::Long(9_876_543_210)],
        );
        callp(
            &mut vm,
            p,
            "writeString",
            "(Ljava/lang/String;)V",
            std::slice::from_ref(&text),
        );
        callp(
            &mut vm,
            p,
            "writeByteArray",
            "([B)V",
            std::slice::from_ref(&blob),
        );
        callp(
            &mut vm,
            p,
            "writeValue",
            "(Ljava/lang/Object;)V",
            std::slice::from_ref(&list),
        );

        let size = callp(&mut vm, p, "dataSize", "()I", &[]);
        let bytes = callp(&mut vm, p, "marshall", "()[B", &[]);
        let p2 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p2,
            "unmarshall",
            "([BII)V",
            &[bytes.clone(), Value::Int(0), size],
        );
        assert_eq!(callp(&mut vm, p2, "readInt", "()I", &[]), Value::Int(-7));
        assert_eq!(
            callp(&mut vm, p2, "readLong", "()J", &[]),
            Value::Long(9_876_543_210)
        );
        let got = obj(callp(
            &mut vm,
            p2,
            "readString",
            "()Ljava/lang/String;",
            &[],
        ));
        assert_eq!(vm.heap.as_str(got).unwrap(), "héllo 📦");
        let arr = obj(callp(&mut vm, p2, "createByteArray", "()[B", &[]));
        let elems = vm.heap.as_array_elems(arr).unwrap().0.to_vec();
        assert_eq!(
            elems,
            vec![
                Value::Int(9),
                Value::Int(8),
                Value::Int(7),
                Value::Int(6),
                Value::Int(5)
            ]
        );
        let value = obj(callp(
            &mut vm,
            p2,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            &[Value::Null],
        ));
        let items = match vm.fw.get(value) {
            Some(HostObj::List { elems }) => elems.clone(),
            other => panic!("readValue list: {other:?}"),
        };
        assert_eq!(items.len(), 2);
        let first = obj(items[0].clone());
        assert_eq!(vm.heap.get_field(first, "value").unwrap(), Value::Int(4242));
        let second = obj(items[1].clone());
        assert_eq!(vm.heap.as_str(second).unwrap(), "inside-list");
    }

    #[test]
    fn sparse_arrays_roundtrip_through_value_and_direct_methods() {
        let mut vm = engine();

        let sparse = vm
            .heap
            .alloc_instance(SPARSE_ARRAY.to_string(), Vec::new())
            .unwrap();
        let number = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(77)).unwrap(),
        );
        let label = s(&mut vm, "sparse-label");
        vm.sparse.insert(
            sparse,
            std::collections::BTreeMap::from([
                (-3_i64, Value::Null),
                (2_i64, label),
                (10_i64, number),
            ]),
        );
        let p = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p,
            "writeValue",
            "(Ljava/lang/Object;)V",
            &[Value::Obj(sparse)],
        );
        let p2 = obj(new_parcel(&mut vm).unwrap());
        let size = callp(&mut vm, p, "dataSize", "()I", &[]);
        let bytes = callp(&mut vm, p, "marshall", "()[B", &[]);
        callp(
            &mut vm,
            p2,
            "unmarshall",
            "([BII)V",
            &[bytes, Value::Int(0), size],
        );
        let got_sparse = obj(callp(
            &mut vm,
            p2,
            "readValue",
            "(Ljava/lang/ClassLoader;)Ljava/lang/Object;",
            &[Value::Null],
        ));
        let entries = vm.sparse.get(&got_sparse).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[&-3], Value::Null);
        let got_label = obj(entries[&2].clone());
        assert_eq!(vm.heap.as_str(got_label).unwrap(), "sparse-label");
        let got_number = obj(entries[&10].clone());
        assert_eq!(
            vm.heap.get_field(got_number, "value").unwrap(),
            Value::Int(77)
        );

        let booleans = vm
            .heap
            .alloc_instance(SPARSE_BOOLEAN_ARRAY.to_string(), Vec::new())
            .unwrap();
        vm.sparse.insert(
            booleans,
            std::collections::BTreeMap::from([
                (-1_i64, Value::Int(0)),
                (5_i64, Value::Int(1)),
                (9_i64, Value::Int(1)),
            ]),
        );
        let p3 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p3,
            "writeSparseBooleanArray",
            "(Landroid/util/SparseBooleanArray;)V",
            &[Value::Obj(booleans)],
        );
        callp(&mut vm, p3, "setDataPosition", "(I)V", &[Value::Int(0)]);
        let got_booleans = obj(callp(
            &mut vm,
            p3,
            "readSparseBooleanArray",
            "()Landroid/util/SparseBooleanArray;",
            &[],
        ));
        assert_eq!(
            vm.sparse.get(&got_booleans).unwrap(),
            &std::collections::BTreeMap::from([
                (-1_i64, Value::Int(0)),
                (5_i64, Value::Int(1)),
                (9_i64, Value::Int(1)),
            ])
        );

        let p4 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p4,
            "writeSparseArray",
            "(Landroid/util/SparseArray;)V",
            &[Value::Null],
        );
        assert_eq!(load(&vm, p4).data, vec![0xff, 0xff, 0xff, 0xff]);
        callp(&mut vm, p4, "setDataPosition", "(I)V", &[Value::Int(0)]);
        assert_eq!(
            callp(
                &mut vm,
                p4,
                "readSparseArray",
                "(Ljava/lang/ClassLoader;)Landroid/util/SparseArray;",
                &[Value::Null],
            ),
            Value::Null
        );
    }

    #[test]
    fn value_tags_and_primitive_array_forms_match_aosp() {
        fn tag_of(st: &PState) -> i32 {
            i32::from_le_bytes(st.data[0..4].try_into().unwrap())
        }
        fn roundtrip(vm: &mut Engine, v: &Value) -> (PState, Value) {
            let p = obj(new_parcel(vm).unwrap());
            write_value(vm, p, v).unwrap();
            let st = load(vm, p);
            let mut reset = st.clone();
            reset.pos = 0;
            store(vm, p, reset);
            let got = read_value(vm, p).unwrap();
            (st, got)
        }
        let mut vm = engine();

        // Character: VAL_CHAR (29) + writeInt, not a CharSequence string.
        let ch = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Character;", Value::Int(120)).unwrap(),
        );
        let (st, got) = roundtrip(&mut vm, &ch);
        assert_eq!(tag_of(&st), VAL_CHAR);
        assert_eq!(i32::from_le_bytes(st.data[4..8].try_into().unwrap()), 120);
        let got = obj(got);
        assert_eq!(vm.heap.class_of(got).unwrap(), "Ljava/lang/Character;");
        assert_eq!(vm.heap.get_field(got, "value").unwrap(), Value::Int(120));

        // boolean[]: VAL_BOOLEANARRAY (23), count, then one int per element.
        let bools = crate::arrays::new_array(
            &mut vm,
            "[Z",
            ElemKind::Int,
            vec![Value::Int(1), Value::Int(0), Value::Int(1)],
        )
        .unwrap();
        let (st, got) = roundtrip(&mut vm, &bools);
        assert_eq!(tag_of(&st), VAL_BOOLEANARRAY);
        assert_eq!(st.data.len(), 4 + 4 + 3 * 4);
        assert_eq!(i32::from_le_bytes(st.data[8..12].try_into().unwrap()), 1);
        assert_eq!(i32::from_le_bytes(st.data[12..16].try_into().unwrap()), 0);
        let got = obj(got);
        assert_eq!(vm.heap.class_of(got).unwrap(), "[Z");
        assert_eq!(
            vm.heap.as_array_elems(got).unwrap().0,
            &[Value::Int(1), Value::Int(0), Value::Int(1)]
        );

        // short[]: VAL_SHORTARRAY (30), decodes back as short[] not int[].
        let shorts = crate::arrays::new_array(
            &mut vm,
            "[S",
            ElemKind::Int,
            vec![Value::Int(7), Value::Int(-2)],
        )
        .unwrap();
        let (st, got) = roundtrip(&mut vm, &shorts);
        assert_eq!(tag_of(&st), VAL_SHORTARRAY);
        assert_eq!(vm.heap.class_of(obj(got)).unwrap(), "[S");

        // PersistableBundle: VAL_PERSISTABLEBUNDLE (25), lazy on read.
        let pb = vm
            .heap
            .alloc_instance(PERSISTABLE_BUNDLE.to_string(), Vec::new())
            .unwrap();
        vm.fw
            .objects
            .insert(pb, HostObj::Bundle { map: Vec::new() });
        let (st, got) = roundtrip(&mut vm, &Value::Obj(pb));
        assert_eq!(tag_of(&st), VAL_PERSISTABLEBUNDLE);
        let got = obj(got);
        assert_eq!(vm.heap.class_of(got).unwrap(), PERSISTABLE_BUNDLE);
        assert!(parcelled_data(&vm, got).is_some());

        // Size: VAL_SIZE (26) + two ints.
        let size = vm
            .heap
            .alloc_instance("Landroid/util/Size;".to_string(), Vec::new())
            .unwrap();
        vm.heap.put_field(size, "rd_w", Value::Int(3)).unwrap();
        vm.heap.put_field(size, "rd_h", Value::Int(4)).unwrap();
        let (st, got) = roundtrip(&mut vm, &Value::Obj(size));
        assert_eq!(tag_of(&st), VAL_SIZE);
        assert_eq!(i32::from_le_bytes(st.data[4..8].try_into().unwrap()), 3);
        assert_eq!(i32::from_le_bytes(st.data[8..12].try_into().unwrap()), 4);
        let got = obj(got);
        assert_eq!(vm.heap.get_field(got, "rd_w").unwrap(), Value::Int(3));
        assert_eq!(vm.heap.get_field(got, "rd_h").unwrap(), Value::Int(4));
    }

    #[test]
    fn length_prefixed_values_backpatch_payload_length() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());
        let item = s(&mut vm, "inside");
        let list = new_list(&mut vm, vec![item]).unwrap();
        write_value(&mut vm, p, &list).unwrap();
        let st = load(&vm, p);
        assert_eq!(
            i32::from_le_bytes(st.data[0..4].try_into().unwrap()),
            VAL_LIST
        );
        assert_eq!(
            i32::from_le_bytes(st.data[4..8].try_into().unwrap()) as usize,
            st.data.len() - 8,
            "length covers only the payload after tag+length"
        );
        assert_eq!(i32::from_le_bytes(st.data[8..12].try_into().unwrap()), 1);

        // And it reads back through the same length-prefixed form.
        let mut reset = load(&vm, p);
        reset.pos = 0;
        store(&mut vm, p, reset);
        let got = obj(read_value(&mut vm, p).unwrap());
        match vm.fw.get(got) {
            Some(HostObj::List { elems }) => {
                assert_eq!(elems.len(), 1);
                assert_eq!(vm.heap.as_str(obj(elems[0].clone())).unwrap(), "inside");
            }
            _ => panic!("list value did not roundtrip"),
        }
        assert_eq!(load(&vm, p).pos, st.data.len());
    }

    #[test]
    fn bundle_parcel_uses_aosp_length_magic_and_null_length() {
        let mut vm = engine();
        let empty = vm
            .heap
            .alloc_instance(BUNDLE.to_string(), Vec::new())
            .unwrap();
        vm.fw
            .objects
            .insert(empty, HostObj::Bundle { map: Vec::new() });
        let p = obj(new_parcel(&mut vm).unwrap());
        write_bundle_map(&mut vm, p, empty).unwrap();
        let st = load(&vm, p);
        assert_eq!(st.data.len(), 12);
        assert_eq!(
            i32::from_le_bytes(st.data[0..4].try_into().unwrap()),
            4,
            "length covers only the ArrayMap after the magic"
        );
        assert_eq!(&st.data[4..8], b"BNDL");
        assert_eq!(i32::from_le_bytes(st.data[8..12].try_into().unwrap()), 0);

        let mut st = load(&vm, p);
        st.pos = 0;
        store(&mut vm, p, st);
        assert!(read_bundle_map(&mut vm, p).unwrap().is_empty());
        assert_eq!(load(&vm, p).pos, 12);

        let mut st = load(&vm, p);
        st.pos = 0;
        st.data[4] = b'X';
        store(&mut vm, p, st);
        assert!(read_bundle_map(&mut vm, p).is_err());

        // The null-Bundle form has its own test below
        // (`bundle_parcel_uses_aosp_length_magic_and_nullable_marker`).

        // The guest-visible Bundle methods use the same frame, not a
        // shortcut stream private to Bundle.writeToParcel.
        let source = vm
            .heap
            .alloc_instance(BUNDLE.to_string(), Vec::new())
            .unwrap();
        let number = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(12)).unwrap(),
        );
        vm.fw.objects.insert(
            source,
            HostObj::Bundle {
                map: vec![("answer".into(), number)],
            },
        );
        let p3 = obj(new_parcel(&mut vm).unwrap());
        vm.invoke_instance(
            source,
            "writeToParcel",
            "(Landroid/os/Parcel;I)V",
            &[Value::Obj(p3), Value::Int(0)],
        )
        .unwrap();
        let framed = load(&vm, p3);
        assert_eq!(
            i32::from_le_bytes(framed.data[0..4].try_into().unwrap()) as usize,
            framed.data.len() - 8
        );
        assert_eq!(&framed.data[4..8], b"BNDL");
        callp(&mut vm, p3, "setDataPosition", "(I)V", &[Value::Int(0)]);

        let target = vm
            .heap
            .alloc_instance(BUNDLE.to_string(), Vec::new())
            .unwrap();
        vm.fw
            .objects
            .insert(target, HostObj::Bundle { map: Vec::new() });
        vm.invoke_instance(
            target,
            "readFromParcel",
            "(Landroid/os/Parcel;)V",
            &[Value::Obj(p3)],
        )
        .unwrap();
        // `readFromParcel` adopts the frame lazily; the map materializes
        // only on unparcel (first real access in guest code).
        assert!(parcelled_data(&vm, target).is_some());
        unparcel_bundle(&mut vm, target).unwrap();
        assert!(parcelled_data(&vm, target).is_none());
        let map = bundle_map(&vm, target);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].0, "answer");
        let got = obj(map[0].1.clone());
        assert_eq!(vm.heap.get_field(got, "value").unwrap(), Value::Int(12));
    }

    // issue #193: RD Sentinel flagged this historical test name as removed
    // when f321917 renamed it to `..._and_null_length` while correcting the
    // wire semantics to AOSP. The coverage was never lost — it moved into
    // that test — but Sentinel inventories tests by name, so the null form
    // is split back out under its historical name. Both tests now coexist;
    // no assertion was dropped.
    #[test]
    fn bundle_parcel_uses_aosp_length_magic_and_nullable_marker() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p,
            "writeBundle",
            "(Landroid/os/Bundle;)V",
            &[Value::Null],
        );
        // AOSP: a null Bundle is a lone int -1 where the frame length
        // would be — the -1 length itself is the nullable marker, there is
        // no separate presence marker (and no BNDL magic after it).
        assert_eq!(load(&vm, p).data, vec![255, 255, 255, 255]);
        callp(&mut vm, p, "setDataPosition", "(I)V", &[Value::Int(0)]);
        assert_eq!(
            callp(
                &mut vm,
                p,
                "readBundle",
                "(Ljava/lang/ClassLoader;)Landroid/os/Bundle;",
                &[Value::Null],
            ),
            Value::Null
        );
    }

    #[test]
    fn array_map_and_array_set_roundtrip_through_parcel() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());

        // ArrayMap<String, Integer> in the host's signed-hash order
        // ("a" = 97 before "b" = 98).
        let a = s(&mut vm, "a");
        let one = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(1)).unwrap(),
        );
        let b = s(&mut vm, "b");
        let two = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(2)).unwrap(),
        );
        let map = vm
            .heap
            .alloc_instance("Landroid/util/ArrayMap;".to_string(), Vec::new())
            .unwrap();
        vm.fw.objects.insert(
            map,
            HostObj::Map {
                entries: vec![(a, one), (b, two)],
            },
        );
        callp(
            &mut vm,
            p,
            "writeArrayMap",
            "(Landroid/util/ArrayMap;)V",
            &[Value::Obj(map)],
        );
        let map_end = load(&vm, p).data.len();

        // ArraySet with Integer 9 and "x": hash order is 9 (9) then "x"
        // (120); the reader preserves the written stream order.
        let nine = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(9)).unwrap(),
        );
        let x = s(&mut vm, "x");
        let set = vm
            .heap
            .alloc_instance("Landroid/util/ArraySet;".to_string(), Vec::new())
            .unwrap();
        vm.fw.objects.insert(
            set,
            HostObj::List {
                elems: vec![nine, x],
            },
        );
        callp(
            &mut vm,
            p,
            "writeArraySet",
            "(Landroid/util/ArraySet;)V",
            &[Value::Obj(set)],
        );

        callp(&mut vm, p, "setDataPosition", "(I)V", &[Value::Int(0)]);
        let dest = vm
            .heap
            .alloc_instance("Landroid/util/ArrayMap;".to_string(), Vec::new())
            .unwrap();
        vm.fw.objects.insert(
            dest,
            HostObj::Map {
                entries: Vec::new(),
            },
        );
        callp(
            &mut vm,
            p,
            "readArrayMap",
            "(Landroid/util/ArrayMap;Ljava/lang/ClassLoader;)V",
            &[Value::Obj(dest), Value::Null],
        );
        assert_eq!(load(&vm, p).pos, map_end);
        let entries = match vm.fw.get(dest) {
            Some(HostObj::Map { entries }) => entries.clone(),
            _ => panic!("destination ArrayMap has no host state"),
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(vm.heap.as_str(obj(entries[0].0.clone())).unwrap(), "a");
        assert_eq!(
            vm.heap
                .get_field(obj(entries[0].1.clone()), "value")
                .unwrap(),
            Value::Int(1)
        );
        assert_eq!(vm.heap.as_str(obj(entries[1].0.clone())).unwrap(), "b");

        let got_set = obj(callp(
            &mut vm,
            p,
            "readArraySet",
            "(Ljava/lang/ClassLoader;)Landroid/util/ArraySet;",
            &[Value::Null],
        ));
        assert_eq!(
            vm.heap.class_of(got_set).unwrap(),
            "Landroid/util/ArraySet;"
        );
        let elems = match vm.fw.get(got_set) {
            Some(HostObj::List { elems }) => elems.clone(),
            _ => panic!("read ArraySet has no host state"),
        };
        assert_eq!(elems.len(), 2);
        assert_eq!(
            vm.heap.get_field(obj(elems[0].clone()), "value").unwrap(),
            Value::Int(9),
            "Integer 9 (hash 9) precedes \"x\" (hash 120)"
        );
        assert_eq!(vm.heap.as_str(obj(elems[1].clone())).unwrap(), "x");
    }

    #[test]
    fn parcel_compare_enforce_binders_sparse_int_and_string8() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());
        let q = obj(new_parcel(&mut vm).unwrap());
        for parcel in [p, q] {
            callp(&mut vm, parcel, "writeInt", "(I)V", &[Value::Int(7)]);
        }
        assert_eq!(
            callp(
                &mut vm,
                p,
                "compareData",
                "(Landroid/os/Parcel;)I",
                &[Value::Obj(q)],
            ),
            Value::Int(0)
        );
        callp(&mut vm, q, "writeInt", "(I)V", &[Value::Int(1)]);
        assert!(
            callp(
                &mut vm,
                p,
                "compareData",
                "(Landroid/os/Parcel;)I",
                &[Value::Obj(q)],
            )
            .as_int()
            .unwrap()
                < 0
        );
        assert_eq!(
            vm.invoke_static(
                PARCEL,
                "compareData",
                "(Landroid/os/Parcel;ILandroid/os/Parcel;II)Z",
                &[
                    Value::Obj(p),
                    Value::Int(0),
                    Value::Obj(q),
                    Value::Int(0),
                    Value::Int(4),
                ],
            )
            .unwrap(),
            Value::Int(1)
        );

        // enforceNoDataAvail: unread bytes remain until consumed.
        callp(&mut vm, p, "setDataPosition", "(I)V", &[Value::Int(0)]);
        assert!(
            call_instance_intrinsic(&mut vm, PARCEL, "enforceNoDataAvail", "()V", p, &[],).is_err()
        );
        assert_eq!(callp(&mut vm, p, "readInt", "()I", &[]), Value::Int(7));
        callp(&mut vm, p, "enforceNoDataAvail", "()V", &[]);

        // hasBinders tracks strong-binder writes through the object table.
        assert_eq!(callp(&mut vm, p, "hasBinders", "()Z", &[]), Value::Int(0));
        let binder = vm
            .heap
            .alloc_instance(BINDER.to_string(), Vec::new())
            .unwrap();
        callp(
            &mut vm,
            p,
            "writeStrongBinder",
            "(Landroid/os/IBinder;)V",
            &[Value::Obj(binder)],
        );
        assert_eq!(callp(&mut vm, p, "hasBinders", "()Z", &[]), Value::Int(1));

        // SparseIntArray form: count then ascending key/value int pairs.
        let sp = obj(new_parcel(&mut vm).unwrap());
        let sia = vm
            .heap
            .alloc_instance("Landroid/util/SparseIntArray;".to_string(), Vec::new())
            .unwrap();
        vm.sparse.insert(
            sia,
            [(2i64, Value::Int(20)), (1i64, Value::Int(10))]
                .into_iter()
                .collect(),
        );
        callp(
            &mut vm,
            sp,
            "writeSparseIntArray",
            "(Landroid/util/SparseIntArray;)V",
            &[Value::Obj(sia)],
        );
        callp(&mut vm, sp, "setDataPosition", "(I)V", &[Value::Int(0)]);
        let got = obj(callp(
            &mut vm,
            sp,
            "readSparseIntArray",
            "()Landroid/util/SparseIntArray;",
            &[],
        ));
        assert_eq!(
            vm.sparse.get(&got).cloned().unwrap_or_default(),
            [(1i64, Value::Int(10)), (2i64, Value::Int(20))]
                .into_iter()
                .collect()
        );

        // String8 array form uses the UTF-8 codec, not UTF-16.
        let hello = s(&mut vm, "hé");
        let arr = crate::arrays::new_array(
            &mut vm,
            "[Ljava/lang/String;",
            ElemKind::Obj,
            vec![hello, Value::Null],
        )
        .unwrap();
        let sp8 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            sp8,
            "writeString8Array",
            "([Ljava/lang/String;)V",
            std::slice::from_ref(&arr),
        );
        callp(&mut vm, sp8, "setDataPosition", "(I)V", &[Value::Int(0)]);
        let got = obj(callp(
            &mut vm,
            sp8,
            "createString8Array",
            "()[Ljava/lang/String;",
            &[],
        ));
        let elems = vm.heap.as_array_elems(got).unwrap().0.to_vec();
        assert_eq!(vm.heap.as_str(obj(elems[0].clone())).unwrap(), "hé");
        assert_eq!(elems[1], Value::Null);

        // Class cookies live in a lazily-created host ArrayMap.
        let cookie = s(&mut vm, "cookie");
        let clazz = s(&mut vm, "not-a-class-but-a-key");
        callp(
            &mut vm,
            p,
            "setClassCookie",
            "(Ljava/lang/Class;Ljava/lang/Object;)V",
            &[clazz.clone(), cookie.clone()],
        );
        assert_eq!(
            callp(
                &mut vm,
                p,
                "hasClassCookie",
                "(Ljava/lang/Class;)Z",
                std::slice::from_ref(&clazz),
            ),
            Value::Int(1)
        );
        assert_eq!(
            callp(
                &mut vm,
                p,
                "getClassCookie",
                "(Ljava/lang/Class;)Ljava/lang/Object;",
                std::slice::from_ref(&clazz),
            ),
            cookie
        );
        let copy = obj(callp(
            &mut vm,
            p,
            "copyClassCookies",
            "()Ljava/util/Map;",
            &[],
        ));
        assert_eq!(vm.heap.class_of(copy).unwrap(), "Landroid/util/ArrayMap;");
        callp(
            &mut vm,
            p,
            "removeClassCookie",
            "(Ljava/lang/Class;Ljava/lang/Object;)V",
            &[clazz.clone(), cookie],
        );
        assert_eq!(
            callp(
                &mut vm,
                p,
                "getClassCookie",
                "(Ljava/lang/Class;)Ljava/lang/Object;",
                std::slice::from_ref(&clazz),
            ),
            Value::Null
        );
    }

    #[test]
    fn exception_codes_match_aosp_status_table() {
        let mut vm = engine();
        for (class, code) in [
            ("Ljava/lang/SecurityException;", EX_SECURITY),
            ("Landroid/os/BadParcelableException;", EX_BAD_PARCELABLE),
            ("Ljava/lang/IllegalArgumentException;", EX_ILLEGAL_ARGUMENT),
            ("Ljava/lang/NullPointerException;", EX_NULL_POINTER),
            ("Ljava/lang/IllegalStateException;", EX_ILLEGAL_STATE),
            (
                "Ljava/lang/UnsupportedOperationException;",
                EX_UNSUPPORTED_OPERATION,
            ),
        ] {
            let p = obj(new_parcel(&mut vm).unwrap());
            let e = vm
                .heap
                .alloc_instance(class.to_string(), Vec::new())
                .unwrap();
            let message = s(&mut vm, "boom");
            vm.heap.put_field(e, "detailMessage", message).unwrap();
            callp(
                &mut vm,
                p,
                "writeException",
                "(Ljava/lang/Exception;)V",
                &[Value::Obj(e)],
            );
            let st = load(&vm, p);
            assert_eq!(
                i32::from_le_bytes(st.data[0..4].try_into().unwrap()),
                code,
                "{class} must use its AOSP status code"
            );
            // Full frame: code, message string, zero stack-trace header.
            callp(&mut vm, p, "setDataPosition", "(I)V", &[Value::Int(0)]);
            assert_eq!(
                callp(&mut vm, p, "readExceptionCode", "()I", &[]),
                Value::Int(code)
            );
            callp(&mut vm, p, "setDataPosition", "(I)V", &[Value::Int(0)]);
            match call_instance_intrinsic(&mut vm, PARCEL, "readException", "()V", p, &[]) {
                Err(VmExit::Exception(t)) => {
                    assert_eq!(t.class, class);
                    assert_eq!(t.message.as_deref(), Some("boom"));
                }
                other => panic!("readException: {other:?}"),
            }
            assert_eq!(
                load(&vm, p).pos,
                load(&vm, p).data.len(),
                "exception frame must be fully consumed"
            );
        }
    }

    #[test]
    fn bundle_and_binder_handles_survive_parcel_bytes() {
        let mut vm = engine();
        let p = obj(new_parcel(&mut vm).unwrap());

        let bundle = vm
            .heap
            .alloc_instance(BUNDLE.to_string(), Vec::new())
            .unwrap();
        let n = Value::Obj(
            crate::boxes::box_value(&mut vm, "Ljava/lang/Integer;", Value::Int(99)).unwrap(),
        );
        let label = s(&mut vm, "roundtrip");
        vm.fw.objects.insert(
            bundle,
            HostObj::Bundle {
                map: vec![("n".into(), n), ("label".into(), label)],
            },
        );
        callp(
            &mut vm,
            p,
            "writeBundle",
            "(Landroid/os/Bundle;)V",
            &[Value::Obj(bundle)],
        );
        let framed = load(&vm, p);
        // AOSP `writeBundle`: the frame starts directly with its length —
        // no presence marker int before it.
        assert_eq!(
            i32::from_le_bytes(framed.data[0..4].try_into().unwrap()) as usize,
            framed.data.len() - 8
        );
        assert_eq!(&framed.data[4..8], b"BNDL");

        let binder = vm
            .heap
            .alloc_instance(BINDER.to_string(), Vec::new())
            .unwrap();
        let owner = vm
            .heap
            .alloc_instance("Ljava/lang/Object;".to_string(), Vec::new())
            .unwrap();
        let descriptor = s(&mut vm, "rd.test.IEcho");
        callb(
            &mut vm,
            binder,
            "attachInterface",
            "(Landroid/os/IInterface;Ljava/lang/String;)V",
            &[Value::Obj(owner), descriptor.clone()],
        );
        assert_eq!(
            callb(
                &mut vm,
                binder,
                "queryLocalInterface",
                "(Ljava/lang/String;)Landroid/os/IInterface;",
                std::slice::from_ref(&descriptor),
            ),
            Value::Obj(owner)
        );
        callp(
            &mut vm,
            p,
            "writeStrongBinder",
            "(Landroid/os/IBinder;)V",
            &[Value::Obj(binder)],
        );

        let size = callp(&mut vm, p, "dataSize", "()I", &[]);
        let bytes = callp(&mut vm, p, "marshall", "()[B", &[]);
        let p2 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p2,
            "unmarshall",
            "([BII)V",
            &[bytes, Value::Int(0), size],
        );
        let got_bundle = obj(callp(
            &mut vm,
            p2,
            "readBundle",
            "(Ljava/lang/ClassLoader;)Landroid/os/Bundle;",
            &[Value::Null],
        ));
        // `readBundle` is lazy: unmarshalled bytes stay retained until the
        // Bundle is actually accessed.
        assert!(parcelled_data(&vm, got_bundle).is_some());
        unparcel_bundle(&mut vm, got_bundle).unwrap();
        let map = bundle_map(&vm, got_bundle);
        assert_eq!(map.len(), 2);
        let got_n = obj(map[0].1.clone());
        assert_eq!(vm.heap.get_field(got_n, "value").unwrap(), Value::Int(99));
        let got_label = obj(map[1].1.clone());
        assert_eq!(vm.heap.as_str(got_label).unwrap(), "roundtrip");
        assert_eq!(
            callp(
                &mut vm,
                p2,
                "readStrongBinder",
                "()Landroid/os/IBinder;",
                &[],
            ),
            Value::Obj(binder)
        );

        // IBinder.INTERFACE_TRANSACTION is answered by the Binder base.
        let data = obj(new_parcel(&mut vm).unwrap());
        let reply = obj(new_parcel(&mut vm).unwrap());
        assert_eq!(
            callb(
                &mut vm,
                binder,
                "transact",
                "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z",
                &[
                    Value::Int(INTERFACE_TRANSACTION),
                    Value::Obj(data),
                    Value::Obj(reply),
                    Value::Int(0),
                ],
            ),
            Value::Int(1)
        );
        callp(&mut vm, reply, "setDataPosition", "(I)V", &[Value::Int(0)]);
        let reply_text = obj(callp(
            &mut vm,
            reply,
            "readString",
            "()Ljava/lang/String;",
            &[],
        ));
        assert_eq!(vm.heap.as_str(reply_text).unwrap(), "rd.test.IEcho");
    }
}
