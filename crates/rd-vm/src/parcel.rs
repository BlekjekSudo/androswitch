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
const VAL_OBJECTARRAY: i32 = 16;
const VAL_INTARRAY: i32 = 17;
const VAL_LONGARRAY: i32 = 18;
const VAL_BYTE: i32 = 19;
const VAL_SPARSEBOOLEANARRAY: i32 = 22;
const VAL_BOOLEANARRAY: i32 = 23;
const VAL_CHARARRAY: i32 = 24;
const VAL_DOUBLEARRAY: i32 = 25;
const VAL_FLOATARRAY: i32 = 27;

fn bad(msg: impl Into<String>) -> VmExit {
    Throwable::new("Landroid/os/BadParcelableException;", msg).into()
}
fn npe(msg: impl Into<String>) -> VmExit {
    Throwable::new("Ljava/lang/NullPointerException;", msg).into()
}
fn iae(msg: impl Into<String>) -> VmExit {
    Throwable::new("Ljava/lang/IllegalArgumentException;", msg).into()
}

#[derive(Debug, Clone, Default)]
struct PState {
    data: Vec<u8>,
    pos: usize,
    objects: Vec<Value>,
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

impl PState {
    fn align_write(&mut self) {
        let a = align4(self.pos);
        if a > self.data.len() {
            self.data.resize(a, 0);
        }
        self.pos = a;
    }
    fn write_raw(&mut self, bytes: &[u8]) {
        if self.pos > self.data.len() {
            self.data.resize(self.pos, 0);
        }
        let end = self.pos + bytes.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
    }
    fn write_padded(&mut self, bytes: &[u8]) {
        self.write_raw(bytes);
        let pad = align4(bytes.len()) - bytes.len();
        if pad > 0 {
            self.write_raw(&[0, 0, 0][..pad]);
        }
    }
    fn write_i32(&mut self, v: i32) {
        self.align_write();
        self.write_raw(&v.to_le_bytes());
    }
    fn write_i64(&mut self, v: i64) {
        self.align_write();
        self.write_raw(&v.to_le_bytes());
    }
    fn patch_i32(&mut self, offset: usize, v: i32) {
        if let Some(bytes) = self.data.get_mut(offset..offset + 4) {
            bytes.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn write_string(&mut self, s: Option<&str>) {
        match s {
            None => self.write_i32(-1),
            Some(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                self.write_i32(units.len() as i32);
                let mut bytes = Vec::with_capacity((units.len() + 1) * 2);
                for u in &units {
                    bytes.extend_from_slice(&u.to_le_bytes());
                }
                bytes.extend_from_slice(&0u16.to_le_bytes());
                self.write_padded(&bytes);
            }
        }
    }
    fn write_string8(&mut self, s: Option<&str>) {
        match s {
            None => self.write_i32(-1),
            Some(s) => {
                self.write_i32(s.len() as i32);
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0);
                self.write_padded(&bytes);
            }
        }
    }
    fn write_byte_blob(&mut self, b: Option<&[u8]>) {
        match b {
            None => self.write_i32(-1),
            Some(b) => {
                self.write_i32(b.len() as i32);
                self.write_padded(b);
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
        let out = self.read_raw(n, what)?;
        let pad = align4(n) - n;
        if pad > 0 {
            if self.pos + pad > self.data.len() {
                return Err(bad(format!("Parcel under-run reading {what} padding")));
            }
            self.pos += pad;
        }
        Ok(out)
    }
    fn read_i32(&mut self) -> Result<i32, VmExit> {
        self.pos = align4(self.pos);
        let b = self.read_raw(4, "int")?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn read_i64(&mut self) -> Result<i64, VmExit> {
        self.pos = align4(self.pos);
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
        let bytes = self.read_padded((n + 1) * 2, "string")?;
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
        let bytes = self.read_padded(n + 1, "string8")?;
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
    let mut out = Vec::with_capacity(n.min(1_000_000));
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

fn write_string_to(vm: &mut Engine, parcel: ObjRef, s: Option<&str>) {
    let mut st = load(vm, parcel);
    st.write_string(s);
    store(vm, parcel, st);
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

fn creator_create(
    vm: &mut Engine,
    creator: ObjRef,
    parcel: ObjRef,
    target_desc: Option<&str>,
) -> Result<Value, VmExit> {
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
}

fn creator_new_array(vm: &mut Engine, creator: ObjRef, n: i32) -> Result<Value, VmExit> {
    vm.invoke_instance(
        creator,
        "newArray",
        "(I)[Ljava/lang/Object;",
        &[Value::Int(n)],
    )
}

fn invoke_write_to_parcel(
    vm: &mut Engine,
    obj: ObjRef,
    parcel: ObjRef,
    flags: i32,
) -> Result<(), VmExit> {
    let class = vm.heap.class_of(obj)?.to_string();
    if class == BUNDLE || class == PERSISTABLE_BUNDLE {
        // Bundle is a host value, not DEX: write its AOSP
        // length/magic/ArrayMap frame directly.
        return write_bundle_map(vm, parcel, obj);
    }
    vm.invoke_instance(
        obj,
        "writeToParcel",
        "(Landroid/os/Parcel;I)V",
        &[Value::Obj(parcel), Value::Int(flags)],
    )?;
    Ok(())
}

fn bundle_map(vm: &Engine, r: ObjRef) -> Vec<(String, Value)> {
    match vm.fw.get(r) {
        Some(HostObj::Bundle { map }) => map.clone(),
        _ => Vec::new(),
    }
}

fn write_bundle_entries(
    vm: &mut Engine,
    parcel: ObjRef,
    entries: Vec<(String, Value)>,
) -> Result<(), VmExit> {
    let mut st = load(vm, parcel);
    st.write_i32(entries.len() as i32);
    store(vm, parcel, st);
    for (k, v) in entries {
        write_string_to(vm, parcel, Some(&k));
        write_value(vm, parcel, &v)?;
    }
    Ok(())
}

fn read_bundle_entries(vm: &mut Engine, parcel: ObjRef) -> Result<Vec<(String, Value)>, VmExit> {
    let mut st = load(vm, parcel);
    let n = st.read_i32()?;
    store(vm, parcel, st);
    if n < 0 {
        return Err(bad(format!("Parcel: negative bundle size {n}")));
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut st = load(vm, parcel);
        let k = st.read_string()?.unwrap_or_default();
        store(vm, parcel, st);
        let v = read_value(vm, parcel)?;
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
    let entries = bundle_map(vm, bundle);
    let (length_pos, start_pos) = {
        let mut st = load(vm, parcel);
        st.pos = align4(st.pos);
        let length_pos = st.pos;
        st.write_i32(0); // Backpatched after the map is written.
        st.write_i32(BUNDLE_MAGIC);
        let start_pos = st.pos;
        store(vm, parcel, st);
        (length_pos, start_pos)
    };
    write_bundle_entries(vm, parcel, entries)?;
    let mut st = load(vm, parcel);
    let end_pos = st.pos;
    let length = i32::try_from(end_pos - start_pos)
        .map_err(|_| bad("Parcel: Bundle exceeds the maximum parcelable size"))?;
    st.patch_i32(length_pos, length);
    st.pos = end_pos;
    store(vm, parcel, st);
    Ok(())
}

/// Read one AOSP-framed Bundle. The declared length bounds the map and is
/// honored even if a forward-compatible producer leaves unread bytes inside
/// the Bundle frame.
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
    let entries = read_bundle_entries(vm, parcel)?;
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

fn write_nullable_bundle(
    vm: &mut Engine,
    parcel: ObjRef,
    bundle: Option<ObjRef>,
) -> Result<(), VmExit> {
    let mut st = load(vm, parcel);
    st.write_i32(bundle.is_some() as i32);
    store(vm, parcel, st);
    if let Some(bundle) = bundle {
        write_bundle_map(vm, parcel, bundle)?;
    }
    Ok(())
}

fn read_nullable_bundle(vm: &mut Engine, parcel: ObjRef, class: &str) -> Result<Value, VmExit> {
    let mut st = load(vm, parcel);
    let present = st.read_i32()?;
    store(vm, parcel, st);
    match present {
        0 => Ok(Value::Null),
        1 => {
            let entries = read_bundle_map(vm, parcel)?;
            new_bundle(vm, class, entries)
        }
        other => Err(bad(format!(
            "Parcel: invalid nullable Bundle marker {other}"
        ))),
    }
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
    let mut entries = Vec::with_capacity(n.min(1_000_000));
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

fn write_sparse_array(
    vm: &mut Engine,
    parcel: ObjRef,
    sparse: Option<ObjRef>,
) -> Result<(), VmExit> {
    let Some(sparse) = sparse else {
        let mut st = load(vm, parcel);
        st.write_i32(-1);
        store(vm, parcel, st);
        return Ok(());
    };
    let entries = sparse_entries(vm, sparse)?;
    let mut st = load(vm, parcel);
    st.write_i32(entries.len() as i32);
    store(vm, parcel, st);
    for (key, value) in entries {
        let mut st = load(vm, parcel);
        st.write_i32(key);
        store(vm, parcel, st);
        write_value(vm, parcel, &value)?;
    }
    Ok(())
}

fn read_sparse_array(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let Some(n) = read_typed_len(vm, parcel, "SparseArray")? else {
        return Ok(Value::Null);
    };
    let mut entries = Vec::with_capacity(n.min(1_000_000));
    for _ in 0..n {
        let key = read_i32_from(vm, parcel)?;
        entries.push((key, read_value(vm, parcel)?));
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
        st.write_i32(-1);
        store(vm, parcel, st);
        return Ok(());
    };
    let entries = sparse_entries(vm, sparse)?;
    let mut st = load(vm, parcel);
    st.write_i32(entries.len() as i32);
    for (key, value) in entries {
        st.write_i32(key);
        // AOSP writes each boolean with writeByte, whose native aligned
        // representation is one 4-byte word in the Parcel stream.
        st.write_i32((value.as_int().unwrap_or(0) != 0) as i32);
    }
    store(vm, parcel, st);
    Ok(())
}

fn read_sparse_boolean_array(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let Some(n) = read_typed_len(vm, parcel, "SparseBooleanArray")? else {
        return Ok(Value::Null);
    };
    let mut st = load(vm, parcel);
    let mut entries = Vec::with_capacity(n.min(1_000_000));
    for _ in 0..n {
        let key = st.read_i32()?;
        let value = st.read_i32()?;
        entries.push((key, Value::Int((value != 0) as i32)));
    }
    store(vm, parcel, st);
    new_sparse(vm, SPARSE_BOOLEAN_ARRAY, entries)
}

fn new_bundle(vm: &mut Engine, class: &str, map: Vec<(String, Value)>) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.objects.insert(r, HostObj::Bundle { map });
    Ok(Value::Obj(r))
}

fn write_parcelable(vm: &mut Engine, parcel: ObjRef, v: &Value, flags: i32) -> Result<(), VmExit> {
    let Some(r) = v.as_ref()? else {
        write_string_to(vm, parcel, None);
        return Ok(());
    };
    let class = vm.heap.class_of(r)?.to_string();
    let name = java_name(&class);
    write_string_to(vm, parcel, Some(&name));
    invoke_write_to_parcel(vm, r, parcel, flags)
}

fn read_parcelable(vm: &mut Engine, parcel: ObjRef) -> Result<Value, VmExit> {
    let mut st = load(vm, parcel);
    let name = st.read_string()?;
    store(vm, parcel, st);
    let Some(name) = name else {
        return Ok(Value::Null);
    };
    let desc = descriptor_of(&name);
    if desc == BUNDLE || desc == PERSISTABLE_BUNDLE {
        let map = read_bundle_map(vm, parcel)?;
        return new_bundle(vm, &desc, map);
    }
    let creator = creator_for(vm, &desc)?;
    creator_create(vm, creator, parcel, Some(&desc))
}

fn boxed(vm: &mut Engine, class: &str, v: Value) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(vm, class, v)?))
}

fn write_value(vm: &mut Engine, parcel: ObjRef, v: &Value) -> Result<(), VmExit> {
    // Raw primitive Values can arrive from host-stored Bundles.
    match v {
        Value::Null | Value::WideHi => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_NULL);
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Int(i) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_INTEGER);
            st.write_i32(*i);
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Long(l) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_LONG);
            st.write_i64(*l);
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Float(f) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_FLOAT);
            st.write_i32(f.to_bits() as i32);
            store(vm, parcel, st);
            return Ok(());
        }
        Value::Double(d) => {
            let mut st = load(vm, parcel);
            st.write_i32(VAL_DOUBLE);
            st.write_i64(d.to_bits() as i64);
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
        st.write_i32(VAL_NULL);
        store(vm, parcel, st);
        return Ok(());
    };
    if let Ok(s) = vm.heap.as_str(r) {
        let s = s.to_string();
        let mut st = load(vm, parcel);
        st.write_i32(VAL_STRING);
        st.write_string(Some(&s));
        store(vm, parcel, st);
        return Ok(());
    }
    let class = vm.heap.class_of(r)?.to_string();
    if let Some(kind) = crate::boxes::kind(&class) {
        let inner = vm.heap.get_field(r, "value").unwrap_or(Value::Null);
        let mut st = load(vm, parcel);
        match kind {
            'Z' => {
                st.write_i32(VAL_BOOLEAN);
                st.write_i32(inner.as_int().unwrap_or(0));
            }
            'B' => {
                st.write_i32(VAL_BYTE);
                st.write_i32(inner.as_int().unwrap_or(0));
            }
            'S' => {
                st.write_i32(VAL_SHORT);
                st.write_i32(inner.as_int().unwrap_or(0));
            }
            'C' => {
                st.write_i32(VAL_CHARSEQUENCE);
                let c = char::from_u32(inner.as_int().unwrap_or(0) as u32).unwrap_or('\0');
                st.write_string(Some(&c.to_string()));
            }
            'I' => {
                st.write_i32(VAL_INTEGER);
                st.write_i32(inner.as_int().unwrap_or(0));
            }
            'J' => {
                st.write_i32(VAL_LONG);
                st.write_i64(inner.as_long().unwrap_or(0));
            }
            'F' => {
                st.write_i32(VAL_FLOAT);
                st.write_i32(inner.as_float().unwrap_or(0.0).to_bits() as i32);
            }
            'D' => {
                st.write_i32(VAL_DOUBLE);
                st.write_i64(inner.as_double().unwrap_or(0.0).to_bits() as i64);
            }
            _ => st.write_i32(VAL_NULL),
        }
        store(vm, parcel, st);
        return Ok(());
    }
    if class == BUNDLE || class == PERSISTABLE_BUNDLE {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_BUNDLE);
        store(vm, parcel, st);
        return write_bundle_map(vm, parcel, r);
    }
    if class == SPARSE_BOOLEAN_ARRAY {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_SPARSEBOOLEANARRAY);
        store(vm, parcel, st);
        return write_sparse_boolean_array(vm, parcel, Some(r));
    }
    if class == SPARSE_ARRAY || vm.cp.is_subtype(&class, SPARSE_ARRAY) {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_SPARSEARRAY);
        store(vm, parcel, st);
        return write_sparse_array(vm, parcel, Some(r));
    }
    if class.starts_with('[') {
        let elems = vm.heap.as_array_elems(r)?.0.to_vec();
        let mut st = load(vm, parcel);
        match class.as_str() {
            "[B" => {
                st.write_i32(VAL_BYTEARRAY);
                let bytes: Vec<u8> = elems
                    .iter()
                    .map(|e| e.as_int().unwrap_or(0) as u8)
                    .collect();
                st.write_byte_blob(Some(&bytes));
            }
            "[I" | "[S" => {
                st.write_i32(VAL_INTARRAY);
                st.write_i32(elems.len() as i32);
                for e in &elems {
                    st.write_i32(e.as_int().unwrap_or(0));
                }
            }
            "[Z" => {
                st.write_i32(VAL_BOOLEANARRAY);
                st.write_i32(elems.len() as i32);
                let bytes: Vec<u8> = elems
                    .iter()
                    .map(|e| (e.as_int().unwrap_or(0) != 0) as u8)
                    .collect();
                st.write_padded(&bytes);
            }
            "[C" => {
                st.write_i32(VAL_CHARARRAY);
                st.write_i32(elems.len() as i32);
                for e in &elems {
                    st.write_i32(e.as_int().unwrap_or(0));
                }
            }
            "[J" => {
                st.write_i32(VAL_LONGARRAY);
                st.write_i32(elems.len() as i32);
                for e in &elems {
                    st.write_i64(e.as_long().unwrap_or(0));
                }
            }
            "[F" => {
                st.write_i32(VAL_FLOATARRAY);
                st.write_i32(elems.len() as i32);
                for e in &elems {
                    st.write_i32(e.as_float().unwrap_or(0.0).to_bits() as i32);
                }
            }
            "[D" => {
                st.write_i32(VAL_DOUBLEARRAY);
                st.write_i32(elems.len() as i32);
                for e in &elems {
                    st.write_i64(e.as_double().unwrap_or(0.0).to_bits() as i64);
                }
            }
            "[Ljava/lang/String;" => {
                // Strings need heap access after dropping the parcel state.
                drop(st);
                let mut st = load(vm, parcel);
                st.write_i32(VAL_STRINGARRAY);
                st.write_i32(elems.len() as i32);
                store(vm, parcel, st);
                for e in &elems {
                    let s = str_of(vm, Some(e))?;
                    write_string_to(vm, parcel, s.as_deref());
                }
                return Ok(());
            }
            _ => {
                st.write_i32(VAL_OBJECTARRAY);
                st.write_i32(elems.len() as i32);
                store(vm, parcel, st);
                for e in &elems {
                    write_value(vm, parcel, e)?;
                }
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
        st.write_i32(VAL_MAP);
        st.write_i32(entries.len() as i32);
        store(vm, parcel, st);
        for (k, val) in &entries {
            write_value(vm, parcel, k)?;
            write_value(vm, parcel, val)?;
        }
        return Ok(());
    }
    if let Some(elems) = list_values(vm, v)? {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_LIST);
        st.write_i32(elems.len() as i32);
        store(vm, parcel, st);
        for e in &elems {
            write_value(vm, parcel, e)?;
        }
        return Ok(());
    }
    if class == BINDER || vm.cp.is_subtype(&class, BINDER) || vm.cp.is_subtype(&class, IBINDER) {
        let h = binder_handle(vm, v);
        let mut st = load(vm, parcel);
        st.write_i32(VAL_IBINDER);
        st.write_i32(h);
        store(vm, parcel, st);
        return Ok(());
    }
    if vm.cp.is_subtype(&class, "Ljava/lang/CharSequence;") {
        let s = char_seq_of(vm, Some(v))?;
        let mut st = load(vm, parcel);
        st.write_i32(VAL_CHARSEQUENCE);
        st.write_string(s.as_deref());
        store(vm, parcel, st);
        return Ok(());
    }
    if vm.cp.is_subtype(&class, "Landroid/os/Parcelable;") {
        let mut st = load(vm, parcel);
        st.write_i32(VAL_PARCELABLE);
        store(vm, parcel, st);
        return write_parcelable(vm, parcel, v, 0);
    }
    Err(bad(format!(
        "Parcel.writeValue: unsupported value class {class}"
    )))
}

fn read_typed_len(vm: &mut Engine, parcel: ObjRef, what: &str) -> Result<Option<usize>, VmExit> {
    let mut st = load(vm, parcel);
    let n = st.read_len(what)?;
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
    let mut st = load(vm, parcel);
    let tag = st.read_i32()?;
    store(vm, parcel, st);
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
        VAL_INTARRAY | VAL_CHARARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "int array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Int(st.read_i32()?));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(
                vm,
                if tag == VAL_CHARARRAY { "[C" } else { "[I" },
                ElemKind::Int,
                vals,
            )?
        }
        VAL_LONGARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "long array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Long(st.read_i64()?));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[J", ElemKind::Long, vals)?
        }
        VAL_FLOATARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "float array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Float(f32::from_bits(st.read_i32()? as u32)));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[F", ElemKind::Float, vals)?
        }
        VAL_DOUBLEARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "double array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            let mut st = load(vm, parcel);
            for _ in 0..n {
                vals.push(Value::Double(f64::from_bits(st.read_i64()? as u64)));
            }
            store(vm, parcel, st);
            crate::arrays::new_array(vm, "[D", ElemKind::Double, vals)?
        }
        VAL_BOOLEANARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "boolean array")? else {
                return Ok(Value::Null);
            };
            let mut st = load(vm, parcel);
            let bytes = st.read_padded(n, "boolean array")?;
            store(vm, parcel, st);
            let vals = bytes
                .into_iter()
                .map(|b| Value::Int((b != 0) as i32))
                .collect();
            crate::arrays::new_array(vm, "[Z", ElemKind::Int, vals)?
        }
        VAL_STRINGARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "string array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                let mut st = load(vm, parcel);
                let s = st.read_string()?;
                store(vm, parcel, st);
                vals.push(jstring(vm, s)?);
            }
            crate::arrays::new_array(vm, "[Ljava/lang/String;", ElemKind::Obj, vals)?
        }
        VAL_OBJECTARRAY => {
            let Some(n) = read_typed_len(vm, parcel, "object array")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(read_value(vm, parcel)?);
            }
            crate::arrays::new_array(vm, "[Ljava/lang/Object;", ElemKind::Obj, vals)?
        }
        VAL_LIST => {
            let Some(n) = read_typed_len(vm, parcel, "list")? else {
                return Ok(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(read_value(vm, parcel)?);
            }
            new_list(vm, vals)?
        }
        VAL_MAP => {
            let Some(n) = read_typed_len(vm, parcel, "map")? else {
                return Ok(Value::Null);
            };
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let k = read_value(vm, parcel)?;
                let v = read_value(vm, parcel)?;
                entries.push((k, v));
            }
            new_map(vm, entries)?
        }
        VAL_BUNDLE => {
            let map = read_bundle_map(vm, parcel)?;
            new_bundle(vm, BUNDLE, map)?
        }
        VAL_SPARSEARRAY => read_sparse_array(vm, parcel)?,
        VAL_SPARSEBOOLEANARRAY => read_sparse_boolean_array(vm, parcel)?,
        VAL_IBINDER => {
            let mut st = load(vm, parcel);
            let h = st.read_i32()?;
            store(vm, parcel, st);
            binder_for_handle(vm, h)?
        }
        VAL_PARCELABLE => read_parcelable(vm, parcel)?,
        other => return Err(bad(format!("Parcel.readValue: unsupported tag {other}"))),
    })
}

fn write_present_typed(
    vm: &mut Engine,
    parcel: ObjRef,
    elems: &[Value],
    flags: i32,
) -> Result<(), VmExit> {
    for e in elems {
        match e.as_ref()? {
            Some(r) => {
                let mut st = load(vm, parcel);
                st.write_i32(1);
                store(vm, parcel, st);
                invoke_write_to_parcel(vm, r, parcel, flags)?;
            }
            None => {
                let mut st = load(vm, parcel);
                st.write_i32(0);
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
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let present = {
            let mut st = load(vm, parcel);
            let p = st.read_i32()?;
            store(vm, parcel, st);
            p != 0
        };
        out.push(if present {
            creator_create(vm, creator, parcel, None)?
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
            let mut st = load(vm, recv);
            st.data.resize(n as usize, 0);
            st.pos = st.pos.min(n as usize);
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("setDataCapacity", "(I)V") => {
            if int_arg(args, 0) < 0 {
                return Err(iae("Parcel.setDataCapacity: negative capacity"));
            }
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
            st.write_raw(&src[off as usize..off as usize + len as usize]);
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("hasFileDescriptors", "()Z") => return some(Value::Int(0)),
        ("writeInterfaceToken", "(Ljava/lang/String;)V") => {
            let token = str_of(vm, args.first())?;
            let mut st = load(vm, recv);
            st.write_i32(0); // local strict-mode header word
            st.write_string(token.as_deref());
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
            st.write_i32(0);
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readException", "()V") => {
            let mut st = load(vm, recv);
            let code = st.read_i32()?;
            let msg = if code != 0 { st.read_string()? } else { None };
            store(vm, recv, st);
            if code != 0 {
                let class = match code {
                    -1 => "Ljava/lang/SecurityException;",
                    -3 => "Ljava/lang/IllegalArgumentException;",
                    -4 => "Ljava/lang/NullPointerException;",
                    -5 => "Ljava/lang/IllegalStateException;",
                    -7 => "Landroid/os/BadParcelableException;",
                    _ => "Ljava/lang/RuntimeException;",
                };
                return Err(Throwable::new(
                    class,
                    msg.unwrap_or_else(|| format!("remote exception code {code}")),
                )
                .into());
            }
            return some(Value::Null);
        }
        ("writeException", "(Ljava/lang/Exception;)V") => {
            let (code, msg) = match obj_arg(args, 0) {
                Some(e) => {
                    let class = vm.heap.class_of(e)?.to_string();
                    let code = match class.as_str() {
                        "Ljava/lang/SecurityException;" => -1,
                        "Landroid/os/BadParcelableException;" => -7,
                        "Ljava/lang/IllegalArgumentException;" => -3,
                        "Ljava/lang/NullPointerException;" => -4,
                        "Ljava/lang/IllegalStateException;" => -5,
                        _ => -128,
                    };
                    let msg = match vm.heap.get_field(e, "detailMessage") {
                        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
                        _ => None,
                    };
                    (code, msg)
                }
                None => (0, None),
            };
            let mut st = load(vm, recv);
            st.write_i32(code);
            if code != 0 {
                st.write_string(msg.as_deref());
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("writeInt", "(I)V") => {
            let mut st = load(vm, recv);
            st.write_i32(int_arg(args, 0));
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
            st.write_i64(args.first().and_then(|v| v.as_long().ok()).unwrap_or(0));
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
            );
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
            );
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
            st.write_i32(int_arg(args, 0) as i8 as i32);
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
            st.write_i32((int_arg(args, 0) != 0) as i32);
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
            write_string_to(vm, recv, s.as_deref());
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
            st.write_string8(s.as_deref());
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
            write_string_to(vm, recv, s.as_deref());
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
            st.write_byte_blob(b.as_deref());
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
            st.write_byte_blob(Some(&b[off as usize..off as usize + len as usize]));
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
            let h = binder_handle(
                vm,
                args.first().cloned().unwrap_or(Value::Null).as_ref_value(),
            );
            let mut st = load(vm, recv);
            st.write_i32(h);
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("readStrongBinder", "()Landroid/os/IBinder;") => {
            let mut st = load(vm, recv);
            let h = st.read_i32()?;
            store(vm, recv, st);
            return some(binder_for_handle(vm, h)?);
        }
        ("writeStrongInterface", "(Landroid/os/IInterface;)V") => {
            let binder = match obj_arg(args, 0) {
                None => Value::Null,
                Some(r) => {
                    let class = vm.heap.class_of(r)?.to_string();
                    if class == BINDER || vm.cp.is_subtype(&class, BINDER) {
                        Value::Obj(r)
                    } else {
                        vm.invoke_instance(r, "asBinder", "()Landroid/os/IBinder;", &[])?
                    }
                }
            };
            let h = binder_handle(vm, &binder);
            let mut st = load(vm, recv);
            st.write_i32(h);
            store(vm, recv, st);
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
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
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
            let Some(n) = read_typed_len(vm, recv, "list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(read_value(vm, recv)?);
            }
            fill_list(vm, dest, vals)?;
            return some(Value::Null);
        }
        ("readArrayList", "(Ljava/lang/ClassLoader;)Ljava/util/ArrayList;")
        | ("readArrayList", "(Ljava/lang/ClassLoader;Ljava/lang/Class;)Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, "list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(read_value(vm, recv)?);
            }
            return some(new_list(vm, vals)?);
        }
        ("writeArray", "([Ljava/lang/Object;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
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
            let Some(n) = read_typed_len(vm, recv, "array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(entries) => {
                    let mut st = load(vm, recv);
                    st.write_i32(entries.len() as i32);
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
            let Some(n) = read_typed_len(vm, recv, "map")? else {
                return some(Value::Null);
            };
            let mut entries = Vec::with_capacity(n);
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
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
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
            let Some(n) = read_typed_len(vm, recv, "typed list")? else {
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
            let Some(n) = read_typed_len(vm, recv, "typed list")? else {
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
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
                    store(vm, recv, st);
                    write_present_typed(vm, recv, &elems, int_arg(args, 1))?;
                }
            }
            return some(Value::Null);
        }
        ("createTypedArray", "(Landroid/os/Parcelable$Creator;)[Ljava/lang/Object;") => {
            let Some(creator) = obj_arg(args, 0) else {
                return Err(npe("Parcel.createTypedArray: creator is null"));
            };
            let Some(n) = read_typed_len(vm, recv, "typed array")? else {
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
            let Some(n) = read_typed_len(vm, recv, "typed array")? else {
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
        ("writeIntArray", "([I)V") | ("writeCharArray", "([C)V") => {
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1),
                Some(es) => {
                    st.write_i32(es.len() as i32);
                    for e in &es {
                        st.write_i32(e.as_int().unwrap_or(0));
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createIntArray", "()[I") | ("createCharArray", "()[C") => {
            let Some(n) = read_typed_len(vm, recv, "int array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(Value::Int(st.read_i32()?));
            }
            store(vm, recv, st);
            return some(crate::arrays::new_array(
                vm,
                if name == "createCharArray" {
                    "[C"
                } else {
                    "[I"
                },
                ElemKind::Int,
                vals,
            )?);
        }
        ("readIntArray", "([I)V") | ("readCharArray", "([C)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readIntArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, "int array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
                None => st.write_i32(-1),
                Some(es) => {
                    st.write_i32(es.len() as i32);
                    for e in &es {
                        st.write_i64(e.as_long().unwrap_or(0));
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createLongArray", "()[J") => {
            let Some(n) = read_typed_len(vm, recv, "long array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "long array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
                None => st.write_i32(-1),
                Some(es) => {
                    st.write_i32(es.len() as i32);
                    for e in &es {
                        st.write_i32(e.as_float().unwrap_or(0.0).to_bits() as i32);
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createFloatArray", "()[F") => {
            let Some(n) = read_typed_len(vm, recv, "float array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "float array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
                None => st.write_i32(-1),
                Some(es) => {
                    st.write_i32(es.len() as i32);
                    for e in &es {
                        st.write_i64(e.as_double().unwrap_or(0.0).to_bits() as i64);
                    }
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createDoubleArray", "()[D") => {
            let Some(n) = read_typed_len(vm, recv, "double array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "double array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let mut vals = Vec::with_capacity(n);
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
            let mut st = load(vm, recv);
            match array_elems(vm, args.first())? {
                None => st.write_i32(-1),
                Some(es) => {
                    st.write_i32(es.len() as i32);
                    let bytes: Vec<u8> = es
                        .iter()
                        .map(|e| (e.as_int().unwrap_or(0) != 0) as u8)
                        .collect();
                    st.write_padded(&bytes);
                }
            }
            store(vm, recv, st);
            return some(Value::Null);
        }
        ("createBooleanArray", "()[Z") => {
            let Some(n) = read_typed_len(vm, recv, "boolean array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let bytes = st.read_padded(n, "boolean array")?;
            store(vm, recv, st);
            let vals = bytes
                .into_iter()
                .map(|b| Value::Int((b != 0) as i32))
                .collect();
            return some(crate::arrays::new_array(vm, "[Z", ElemKind::Int, vals)?);
        }
        ("readBooleanArray", "([Z)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readBooleanArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, "boolean array")? else {
                return some(Value::Null);
            };
            let mut st = load(vm, recv);
            let bytes = st.read_padded(n, "boolean array")?;
            store(vm, recv, st);
            let vals: Vec<Value> = bytes
                .into_iter()
                .map(|b| Value::Int((b != 0) as i32))
                .collect();
            let slot = vm.heap.array_elems_mut(dest)?;
            if slot.len() != vals.len() {
                return Err(bad("Parcel.readBooleanArray: destination length mismatch"));
            }
            slot.clone_from(&vals);
            return some(Value::Null);
        }
        ("writeStringArray", "([Ljava/lang/String;)V")
        | ("writeCharSequenceArray", "([Ljava/lang/CharSequence;)V") => {
            match array_elems(vm, args.first())? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
                    store(vm, recv, st);
                    for e in &elems {
                        let s = char_seq_of(vm, Some(e))?;
                        write_string_to(vm, recv, s.as_deref());
                    }
                }
            }
            return some(Value::Null);
        }
        ("createStringArray", "()[Ljava/lang/String;")
        | ("createCharSequenceArray", "()[Ljava/lang/CharSequence;") => {
            let Some(n) = read_typed_len(vm, recv, "string array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
        | ("readCharSequenceArray", "([Ljava/lang/CharSequence;)V") => {
            let Some(dest) = obj_arg(args, 0) else {
                return Err(npe("Parcel.readStringArray: destination is null"));
            };
            let Some(n) = read_typed_len(vm, recv, "string array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
        ("writeStringList", "(Ljava/util/List;)V")
        | ("writeCharSequenceList", "(Ljava/util/List;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
                    store(vm, recv, st);
                    for e in &elems {
                        let s = char_seq_of(vm, Some(e))?;
                        write_string_to(vm, recv, s.as_deref());
                    }
                }
            }
            return some(Value::Null);
        }
        ("createStringArrayList", "()Ljava/util/ArrayList;")
        | ("createCharSequenceArrayList", "()Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, "string list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "string list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
                    store(vm, recv, st);
                    for e in &elems {
                        let h = binder_handle(vm, e);
                        let mut st = load(vm, recv);
                        st.write_i32(h);
                        store(vm, recv, st);
                    }
                }
            }
            return some(Value::Null);
        }
        ("createIBinderArray", "()[Landroid/os/IBinder;")
        | ("createBinderArray", "()[Landroid/os/IBinder;") => {
            let Some(n) = read_typed_len(vm, recv, "binder array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "binder array")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
        ("writeBinderList", "(Ljava/util/List;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            match list_values(vm, &v)? {
                None => {
                    let mut st = load(vm, recv);
                    st.write_i32(-1);
                    store(vm, recv, st);
                }
                Some(elems) => {
                    let mut st = load(vm, recv);
                    st.write_i32(elems.len() as i32);
                    store(vm, recv, st);
                    for e in &elems {
                        let h = binder_handle(vm, e);
                        let mut st = load(vm, recv);
                        st.write_i32(h);
                        store(vm, recv, st);
                    }
                }
            }
            return some(Value::Null);
        }
        ("createBinderArrayList", "()Ljava/util/ArrayList;") => {
            let Some(n) = read_typed_len(vm, recv, "binder list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
            let Some(n) = read_typed_len(vm, recv, "binder list")? else {
                return some(Value::Null);
            };
            let mut vals = Vec::with_capacity(n);
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
        write_string_to(vm, *r, d.as_deref());
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
        st.write_i32(0x0102_0304);
        st.write_string(Some("Aé"));
        st.write_byte_blob(Some(&[1, 2, 3, 4, 5]));
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
        st.write_i32(7);
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
    fn bundle_parcel_uses_aosp_length_magic_and_nullable_marker() {
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

        let p2 = obj(new_parcel(&mut vm).unwrap());
        callp(
            &mut vm,
            p2,
            "writeBundle",
            "(Landroid/os/Bundle;)V",
            &[Value::Null],
        );
        assert_eq!(load(&vm, p2).data, vec![0, 0, 0, 0]);
        callp(&mut vm, p2, "setDataPosition", "(I)V", &[Value::Int(0)]);
        assert_eq!(
            callp(
                &mut vm,
                p2,
                "readBundle",
                "(Ljava/lang/ClassLoader;)Landroid/os/Bundle;",
                &[Value::Null],
            ),
            Value::Null
        );

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
        let map = bundle_map(&vm, target);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].0, "answer");
        let got = obj(map[0].1.clone());
        assert_eq!(vm.heap.get_field(got, "value").unwrap(), Value::Int(12));
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
        assert_eq!(i32::from_le_bytes(framed.data[0..4].try_into().unwrap()), 1);
        assert_eq!(
            i32::from_le_bytes(framed.data[4..8].try_into().unwrap()) as usize,
            framed.data.len() - 12
        );
        assert_eq!(&framed.data[8..12], b"BNDL");

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
