//! JNI references and callbacks target the real portable VM. A C reentrant
//! lease serializes access. No Rust state borrow is held across a machine call.
use crate::device_native::{self, HorizonNative};
use alloc::{
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use as_native::{jni_refs::References, CallFrame, Loader};
use core::{
    cell::{Cell, RefCell},
    ffi::{c_char, c_void},
};
use rd_jni::Handle;
use rd_vm::{heap::ElemKind, native_host::NativeHost, Engine, ObjRef, Throwable, Value, VmExit};
type Result<T> = core::result::Result<T, VmExit>;
fn error(code: &str, cause: impl Into<String>) -> VmExit {
    rd_vm::err::vm_error(code, cause).into()
}
fn jni(e: as_native::Error) -> VmExit {
    device_native::vm_error(e)
}
#[allow(unsafe_code)]
extern "C" {
    fn as_jni_initialize() -> u64;
    fn as_jni_java_vm() -> u64;
    fn as_jni_environment() -> u64;
    fn as_jni_thread() -> u64;
    fn as_native_set_session(session: *const c_void);
    fn as_native_session() -> *const c_void;
    fn as_native_fail(code: i32);
    fn as_jni_vm_lock();
    fn as_jni_vm_unlock();
    fn as_jni_vm_release() -> u32;
    fn as_jni_vm_acquire(depth: u32);
}
#[derive(Default)]
struct ThreadState {
    pending: Option<Throwable>,
}
pub struct Session {
    loader: RefCell<Loader<HorizonNative>>,
    refs: RefCell<References>,
    registrations: RefCell<BTreeMap<(String, String, String), u64>>,
    threads: RefCell<BTreeMap<u64, ThreadState>>,
    vm: Cell<*mut Engine>,
    environment: Cell<u64>,
    fatal: RefCell<Option<VmExit>>,
    loads: Cell<u64>,
    calls: Cell<u64>,
    callbacks: Cell<u64>,
}
pub fn thread() -> u64 {
    // SAFETY: scalar thread identity from libnx, no borrowed data.
    #[allow(unsafe_code)]
    unsafe {
        as_jni_thread()
    }
}
impl Session {
    pub fn new(apk: &rd_apk::Apk) -> Result<Rc<Self>> {
        let session = Rc::new(Self {
            loader: RefCell::new(Loader::new(
                HorizonNative::default(),
                crate::native_probe::sources(apk).map_err(jni)?,
            )),
            refs: RefCell::new(References::default()),
            registrations: RefCell::new(BTreeMap::new()),
            threads: RefCell::new(BTreeMap::new()),
            vm: Cell::new(core::ptr::null_mut()),
            environment: Cell::new(0),
            fatal: RefCell::new(None),
            loads: Cell::new(0),
            calls: Cell::new(0),
            callbacks: Cell::new(0),
        });
        // SAFETY: Rc allocation is stable and retained by the Engine for its
        // entire native session. The C boundary never frees this pointer.
        #[allow(unsafe_code)]
        unsafe {
            as_native_set_session(Rc::as_ptr(&session).cast());
            session.environment.set(as_jni_initialize());
        }
        session.refs.borrow_mut().attach(thread());
        session
            .threads
            .borrow_mut()
            .insert(thread(), ThreadState::default());
        // Native code and pthreads may outlive the last DEX frame. Keep their
        // mappings/session owned by this runner process; M7 replaces the whole
        // process when switching applications. Never unmap live native code.
        let _process_root = Rc::into_raw(session.clone());
        Ok(session)
    }
    pub fn bind_vm(&self, vm: &mut Engine) {
        self.vm.set(vm as *mut Engine);
    }
    pub fn close_vm(&self) {
        self.vm.set(core::ptr::null_mut());
    }
    fn class(&self, token: u64) -> Result<String> {
        match self.refs.borrow().get(thread(), token).map_err(jni)? {
            Handle::Class(name) => Ok(name.clone()),
            _ => Err(error("JNI_REFERENCE_KIND", "expected class reference")),
        }
    }
    fn object(&self, token: u64) -> Result<ObjRef> {
        match self.refs.borrow().get(thread(), token).map_err(jni)? {
            Handle::Object(r) => Ok(*r),
            _ => Err(error("JNI_REFERENCE_KIND", "expected object reference")),
        }
    }
    fn local(&self, value: Handle) -> Result<u64> {
        self.refs.borrow_mut().local(thread(), value).map_err(jni)
    }
    fn pending(&self) -> Option<Throwable> {
        self.threads
            .borrow()
            .get(&thread())
            .and_then(|t| t.pending.clone())
    }
    fn set_pending(&self, t: Throwable) {
        self.threads
            .borrow_mut()
            .entry(thread())
            .or_default()
            .pending = Some(t);
    }
    fn value(&self, bits: u64, kind: &str) -> Result<Value> {
        Ok(match kind {
            "V" => Value::Null,
            "J" => Value::Long(bits as i64),
            "F" => Value::Float(f32::from_bits(bits as u32)),
            "D" => Value::Double(f64::from_bits(bits)),
            t if t.starts_with('L') || t.starts_with('[') => {
                if bits == 0 {
                    Value::Null
                } else {
                    Value::Obj(self.object(bits)?)
                }
            }
            "Z" => Value::Int((bits != 0) as i32),
            "B" => Value::Int(bits as i8 as i32),
            "C" => Value::Int(bits as u16 as i32),
            "S" => Value::Int(bits as i16 as i32),
            _ => Value::Int(bits as i32),
        })
    }
    fn bits(&self, value: &Value, kind: &str) -> Result<u64> {
        if kind.starts_with('L') || kind.starts_with('[') {
            return match value.as_ref()? {
                Some(r) => self.local(Handle::Object(r)),
                None => Ok(0),
            };
        }
        crate::native_probe::bits(value).map_err(jni)
    }
    fn method(&self, token: u64) -> Result<(String, String, String, bool)> {
        match self.refs.borrow().get(thread(), token).map_err(jni)? {
            Handle::Method {
                class,
                name,
                sig,
                is_static,
            } => Ok((class.clone(), name.clone(), sig.clone(), *is_static)),
            _ => Err(error("JNI_REFERENCE_KIND", "expected method ID")),
        }
    }
    fn foreign(&self, vm: &mut Engine, frame: &mut CallFrame) -> Result<()> {
        let previous = self.vm.replace(vm as *mut Engine);
        // SAFETY: C releases only the current thread's VM lease, switches to
        // guest TLS and reacquires the lease before Rust accesses VM state.
        // JNI callbacks temporarily reborrow this pointer under the same lease.
        #[allow(unsafe_code)]
        let depth = unsafe { as_jni_vm_release() };
        let result = device_native::call(frame).map_err(jni);
        #[allow(unsafe_code)]
        unsafe {
            as_jni_vm_acquire(depth)
        };
        self.vm.set(previous);
        if let Some(error) = self.fatal.borrow_mut().take() {
            return Err(error);
        }
        result
    }
    pub fn statistics(&self) -> (u64, u64, u64) {
        (self.loads.get(), self.calls.get(), self.callbacks.get())
    }
    fn handle(
        &self,
        vm: &mut Engine,
        op: u32,
        a: u64,
        b: u64,
        c: u64,
        data: *const u8,
        length: usize,
    ) -> Result<u64> {
        match op {
            1 => {
                let name = text(data, length)?;
                let class = if name.starts_with('[') {
                    name
                } else {
                    format!("L{name};")
                };
                if vm.cp.find_class(&class).is_none() && !platform_class(&class) {
                    return Err(Throwable::new("Ljava/lang/NoClassDefFoundError;", class).into());
                }
                self.local(Handle::Class(class))
            }
            2 => {
                let class = self.class(a)?;
                let name = cstr(b)?;
                let sig = cstr(c)?;
                rd_jni::descriptor(&sig).map_err(|e| error(&e.code, e.cause))?;
                let is_static = length != 0;
                let method = vm.cp.resolve_method(&class, &name, &sig).ok_or_else(|| {
                    Throwable::new(
                        "Ljava/lang/NoSuchMethodError;",
                        format!("{class}->{name}{sig}"),
                    )
                })?;
                if (method.2.access_flags & rd_vm::engine::ACC_STATIC != 0) != is_static {
                    return Err(Throwable::new("Ljava/lang/NoSuchMethodError;", name).into());
                }
                vm.ensure_initialized(&class)?;
                self.refs
                    .borrow_mut()
                    .id(
                        thread(),
                        Handle::Method {
                            class,
                            name,
                            sig,
                            is_static,
                        },
                    )
                    .map_err(jni)
            }
            3 => {
                let class = self.class(a)?;
                if length > 1024 {
                    return Err(error("JNI_ARGUMENT_LIMIT", "RegisterNatives >1024 methods"));
                }
                let methods = read_slice::<NativeMethod>(data, length)?;
                let mut additions = Vec::new();
                for method in methods {
                    let name = cstr(method.name)?;
                    let sig = cstr(method.sig)?;
                    let resolved = vm.cp.resolve_method(&class, &name, &sig).ok_or_else(|| {
                        Throwable::new(
                            "Ljava/lang/NoSuchMethodError;",
                            format!("{class}->{name}{sig}"),
                        )
                    })?;
                    if resolved.2.access_flags & rd_vm::engine::ACC_NATIVE == 0
                        || method.function == 0
                    {
                        return Err(error(
                            "JNI_REGISTER_NATIVES",
                            "method is not native or has null entry",
                        ));
                    }
                    additions.push(((class.clone(), name, sig), method.function));
                }
                self.registrations.borrow_mut().extend(additions);
                Ok(0)
            }
            4 => {
                let (class, name, sig, is_static) = self.method(b)?;
                if is_static != (c != 0) {
                    return Err(error(
                        "JNI_METHOD_KIND",
                        "static/instance invocation mismatch",
                    ));
                }
                let (types, result) =
                    rd_jni::descriptor(&sig).map_err(|e| error(&e.code, e.cause))?;
                if length != types.len() {
                    return Err(error("JNI_SIGNATURE", "callback argument count"));
                }
                let values = read_slice::<u64>(data, length)?
                    .iter()
                    .zip(types.iter())
                    .map(|(bits, ty)| self.value(*bits, ty))
                    .collect::<Result<Vec<_>>>()?;
                self.callbacks.set(self.callbacks.get() + 1);
                let value = if is_static {
                    vm.invoke_static(&class, &name, &sig, &values)
                } else {
                    vm.invoke_instance(self.object(a)?, &name, &sig, &values)
                }?;
                self.bits(&value, &result)
            }
            5 => {
                let object = self.object(a)?;
                self.local(Handle::Class(vm.heap.class_of(object)?.to_string()))
            }
            6 | 28 => {
                let value = if op == 6 {
                    rd_jni::decode_modified_utf8(read_slice::<u8>(data, length)?)
                        .map_err(|e| error(&e.code, e.cause))?
                } else {
                    String::from_utf16(read_slice::<u16>(data, length)?).map_err(|_| {
                        error(
                            "JNI_STRING_UNSUPPORTED",
                            "unpaired UTF-16 surrogate is not representable by this VM",
                        )
                    })?
                };
                let object = vm.heap.alloc_string(value)?;
                self.local(Handle::Object(object))
            }
            7 | 8 | 29 | 30 => {
                let object = self.object(a)?;
                let value = vm.heap.as_str(object)?.to_string();
                if op == 7 {
                    Ok(rd_jni::encode_modified_utf8(&value).len() as u64)
                } else if op == 29 {
                    Ok(value.encode_utf16().count() as u64)
                } else if op == 8 {
                    let bytes = rd_jni::encode_modified_utf8(&value);
                    write_slice(data, length, &bytes)?;
                    Ok(0)
                } else {
                    let units: Vec<_> = value.encode_utf16().collect();
                    write_slice(data, length, &units)?;
                    Ok(0)
                }
            }
            9 => Ok(vm.heap.as_array_elems(self.object(a)?)?.0.len() as u64),
            10 => {
                if b > 1 << 20 {
                    return Err(
                        Throwable::new("Ljava/lang/OutOfMemoryError;", "JNI array limit").into(),
                    );
                }
                let descriptor = format!("[{}", a as u8 as char);
                let kind = ElemKind::from_array_desc(&descriptor)
                    .ok_or_else(|| error("JNI_ARRAY_TYPE", &descriptor))?;
                let object = vm.heap.alloc_array_zeroed(kind, b as usize, &descriptor)?;
                self.local(Handle::Object(object))
            }
            11 | 12 => {
                let object = self.object(a)?;
                let descriptor = vm.heap.class_of(object)?.to_string();
                if descriptor != format!("[{}", c as u8 as char) {
                    return Err(error("JNI_ARRAY_TYPE", descriptor));
                }
                let size = vm.heap.as_array_elems(object)?.0.len();
                let start = b as usize;
                if length > size || start > size - length {
                    return Err(Throwable::new(
                        "Ljava/lang/ArrayIndexOutOfBoundsException;",
                        "JNI array range",
                    )
                    .into());
                }
                let kind = (c as u8 as char).to_string();
                let width = match kind.as_str() {
                    "Z" | "B" => 1,
                    "C" | "S" => 2,
                    "I" | "F" => 4,
                    _ => 8,
                };
                if op == 11 {
                    let values = vm.heap.as_array_elems(object)?.0[start..start + length].to_vec();
                    let mut bytes = Vec::with_capacity(length * width);
                    for value in values {
                        bytes.extend_from_slice(
                            &crate::native_probe::bits(&value)
                                .map_err(jni)?
                                .to_le_bytes()[..width],
                        );
                    }
                    write_slice(data, length * width, &bytes)?;
                } else {
                    let bytes = read_slice::<u8>(data, length * width)?;
                    let mut values = Vec::with_capacity(length);
                    for part in bytes.chunks_exact(width) {
                        let mut bits = [0; 8];
                        bits[..width].copy_from_slice(part);
                        values.push(self.value(u64::from_le_bytes(bits), &kind)?);
                    }
                    vm.heap.array_elems_mut(object)?[start..start + length]
                        .clone_from_slice(&values);
                }
                Ok(0)
            }
            13 | 14 => self
                .refs
                .borrow_mut()
                .duplicate(thread(), a, op == 14)
                .map_err(jni),
            15 | 16 => {
                self.refs
                    .borrow_mut()
                    .delete(thread(), a, op == 16)
                    .map_err(jni)?;
                Ok(0)
            }
            17 => Ok(self.refs.borrow().same(thread(), a, b).map_err(jni)? as u64),
            18 => {
                self.refs.borrow_mut().push(thread()).map_err(jni)?;
                Ok(0)
            }
            19 => self.refs.borrow_mut().pop(thread(), a).map_err(jni),
            20 => Ok(self.pending().is_some() as u64),
            21 => {
                self.threads
                    .borrow_mut()
                    .entry(thread())
                    .or_default()
                    .pending = None;
                Ok(0)
            }
            22 => {
                self.set_pending(Throwable::new(&self.class(a)?, text(data, length)?));
                Ok(0)
            }
            23 => {
                if let Some(mut pending) = self.pending() {
                    let object = if let Some(object) = pending.obj {
                        object
                    } else {
                        let object = vm.heap.alloc_instance(pending.class.clone(), Vec::new())?;
                        pending.obj = Some(object);
                        self.set_pending(pending);
                        object
                    };
                    self.local(Handle::Object(object))
                } else {
                    Ok(0)
                }
            }
            24 => {
                let object = self.object(a)?;
                self.set_pending(Throwable {
                    class: vm.heap.class_of(object)?.to_string(),
                    message: None,
                    obj: Some(object),
                });
                Ok(0)
            }
            25 => {
                let (_, _, sig, _) = self.method(a)?;
                let types = rd_jni::descriptor(&sig)
                    .map_err(|e| error(&e.code, e.cause))?
                    .0;
                let bytes: Vec<u8> = types.iter().map(|t| t.as_bytes()[0]).collect();
                write_slice(data, length, &bytes)?;
                Ok(bytes.len() as u64)
            }
            26 => {
                self.refs.borrow_mut().attach(thread());
                self.threads.borrow_mut().entry(thread()).or_default();
                Ok(0)
            }
            27 => {
                self.refs.borrow_mut().detach(thread());
                self.threads.borrow_mut().remove(&thread());
                Ok(0)
            }
            31 => self.refs.borrow().kind(thread(), a).map_err(jni),
            32 => {
                let class = self.class(a)?;
                vm.ensure_initialized(&class)?;
                let object = vm.heap.alloc_instance(class.clone(), Vec::new())?;
                vm.attach_host_state(&class, object)?;
                self.local(Handle::Object(object))
            }
            33 => {
                if a == 0 {
                    return Ok(1);
                }
                let actual = vm.heap.class_of(self.object(a)?)?.to_string();
                let expected = self.class(b)?;
                let mut next = Some(actual);
                while let Some(class) = next {
                    if class == expected {
                        return Ok(1);
                    }
                    next = vm.cp.superclass_of(&class);
                }
                Ok(0)
            }
            37 => {
                let class = self.class(a)?;
                self.registrations
                    .borrow_mut()
                    .retain(|(c, _, _), _| c != &class);
                Ok(0)
            }
            u32::MAX => Err(error(
                "JNI_API_UNAVAILABLE",
                format!("JNI slot {a}: {}", text(data, length)?),
            )),
            _ => Err(error(
                "JNI_API_UNAVAILABLE",
                format!("JNI bridge operation {op}"),
            )),
        }
    }
}
impl NativeHost for Session {
    fn load(&self, vm: &mut Engine, name: &str, absolute: bool) -> Result<()> {
        let name = if absolute {
            return Err(error("NATIVE_PATH_LOAD_UNAVAILABLE",format!("System.load({name}): loading an arbitrary VFS ELF path is not implemented; APK System.loadLibrary is supported")));
        } else {
            if name.contains('/') || name.is_empty() {
                return Err(error("INVALID_LIBRARY_NAME", name));
            }
            format!("lib{name}.so")
        };
        let old = self.loader.borrow().modules.len();
        self.loader.borrow_mut().load(&name).map_err(jni)?;
        let modules: Vec<_> = self
            .loader
            .borrow()
            .modules
            .iter()
            .enumerate()
            .skip(old)
            .map(|(index, m)| {
                (
                    index,
                    m.constructors.clone(),
                    self.loader.borrow().module_symbol(index, "JNI_OnLoad"),
                )
            })
            .collect();
        for (index, constructors, on_load) in modules {
            for function in constructors {
                self.foreign(
                    vm,
                    &mut CallFrame {
                        function,
                        ..Default::default()
                    },
                )?;
            }
            if let Some(function) = on_load {
                // SAFETY: JavaVM pointer is a persistent C-owned ABI table.
                #[allow(unsafe_code)]
                let java_vm = unsafe { as_jni_java_vm() };
                let mut frame = CallFrame {
                    function,
                    ..Default::default()
                };
                frame.integer[0] = java_vm;
                self.refs.borrow_mut().push(thread()).map_err(jni)?;
                let call = self.foreign(vm, &mut frame);
                self.refs.borrow_mut().pop(thread(), 0).map_err(jni)?;
                call?;
                if frame.result_integer[0] as u32 != 0x00010006 {
                    return Err(error(
                        "JNI_VERSION_UNSUPPORTED",
                        format!("JNI_OnLoad returned {:#x}", frame.result_integer[0]),
                    ));
                }
            }
            self.loader.borrow_mut().modules[index].initialized = true;
            self.loads.set(self.loads.get() + 1);
        }
        Ok(())
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value> {
        let (types, result) = rd_jni::descriptor(sig).map_err(|e| error(&e.code, e.cause))?;
        let registered = self
            .registrations
            .borrow()
            .get(&(class.into(), name.into(), sig.into()))
            .copied();
        let function = registered
            .or_else(|| {
                rd_jni::symbols(class, name, sig)
                    .ok()
                    .and_then(|names| names.iter().find_map(|n| self.loader.borrow().symbol(n)))
            })
            .ok_or_else(|| {
                error(
                    "JNI_NATIVE_METHOD_NOT_FOUND",
                    format!("{class}->{name}{sig}"),
                )
            })?;
        self.refs.borrow_mut().push(thread()).map_err(jni)?;
        let value = (|| {
            let receiver = if is_static {
                self.local(Handle::Class(class.into()))?
            } else {
                let object = slots[0].as_ref()?.ok_or_else(|| {
                    Throwable::new("Ljava/lang/NullPointerException;", "JNI receiver")
                })?;
                self.local(Handle::Object(object))?
            };
            #[allow(unsafe_code)]
            let environment = unsafe { as_jni_environment() };
            let mut values = vec![environment, receiver];
            let mut kinds = vec!["J".to_string(), "J".to_string()];
            let mut slot = if is_static { 0 } else { 1 };
            for ty in &types {
                let value = slots
                    .get(slot)
                    .ok_or_else(|| error("JNI_SIGNATURE", "missing VM slot"))?;
                values.push(self.bits(value, ty)?);
                kinds.push(ty.clone());
                slot += if ty == "J" || ty == "D" { 2 } else { 1 };
            }
            let mut frame = CallFrame {
                function,
                ..Default::default()
            };
            frame.arguments(&kinds, &values).map_err(jni)?;
            self.calls.set(self.calls.get() + 1);
            self.foreign(vm, &mut frame)?;
            if let Some(pending) = self.pending() {
                self.threads
                    .borrow_mut()
                    .entry(thread())
                    .or_default()
                    .pending = None;
                return Err(pending.into());
            }
            let bits = if result == "F" || result == "D" {
                frame.result_float[0]
            } else {
                frame.result_integer[0]
            };
            self.value(bits, &result)
        })();
        self.refs.borrow_mut().pop(thread(), 0).map_err(jni)?;
        value
    }
    fn roots(&self) -> Vec<ObjRef> {
        let mut roots = self.refs.borrow().roots();
        roots.extend(
            self.threads
                .borrow()
                .values()
                .filter_map(|t| t.pending.as_ref().and_then(|e| e.obj)),
        );
        roots
    }
}
fn platform_class(class: &str) -> bool {
    matches!(
        class,
        "Ljava/lang/Object;"
            | "Ljava/lang/String;"
            | "Ljava/lang/Class;"
            | "Ljava/lang/Throwable;"
            | "Ljava/lang/Exception;"
            | "Ljava/lang/RuntimeException;"
            | "Ljava/lang/IllegalArgumentException;"
            | "Ljava/lang/IllegalStateException;"
            | "Ljava/lang/ArithmeticException;"
    )
}
#[repr(C)]
struct NativeMethod {
    name: u64,
    sig: u64,
    function: u64,
}
fn read_slice<'a, T>(data: *const u8, length: usize) -> Result<&'a [T]> {
    if length > 1 << 24
        || length != 0 && (data.is_null() || data as usize % core::mem::align_of::<T>() != 0)
    {
        return Err(error(
            "JNI_INVALID_BUFFER",
            "invalid pointer, alignment or length",
        ));
    }
    if length == 0 {
        return Ok(&[]);
    }
    // SAFETY: native caller supplies the readable region for this synchronous
    // JNI call, as required by the JNI C ABI. Length/alignment checked above.
    #[allow(unsafe_code)]
    Ok(unsafe { core::slice::from_raw_parts(data.cast::<T>(), length) })
}
fn write_slice<T: Copy>(data: *const u8, capacity: usize, bytes: &[T]) -> Result<()> {
    if bytes.len() > capacity || data.is_null() || data as usize % core::mem::align_of::<T>() != 0 {
        return Err(error("JNI_INVALID_BUFFER", "output capacity/alignment"));
    }
    // SAFETY: JNI C adapter supplies a writable, disjoint buffer with capacity.
    #[allow(unsafe_code)]
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut T, bytes.len());
    }
    Ok(())
}
fn text(data: *const u8, length: usize) -> Result<String> {
    String::from_utf8(read_slice::<u8>(data, length)?.to_vec())
        .map_err(|_| error("JNI_INVALID_UTF8", "name is not UTF-8"))
}
fn cstr(address: u64) -> Result<String> {
    if address == 0 {
        return Err(error("JNI_INVALID_BUFFER", "null C string"));
    }
    let mut bytes = Vec::new();
    for i in 0..65536 {
        // SAFETY: caller's JNI string must be readable and NUL-terminated. Bound
        // prevents unbounded scans; native memory is in the runner address space.
        #[allow(unsafe_code)]
        let b = unsafe { *(address as *const u8).add(i) };
        if b == 0 {
            return String::from_utf8(bytes).map_err(|_| error("JNI_INVALID_UTF8", "C string"));
        }
        bytes.push(b);
    }
    Err(error("JNI_STRING_LIMIT", "C string exceeds 64 KiB"))
}
/// C holds the VM lease and has switched back to host TLS before calling.
/// Returned errors are trapped by C after Rust frames have returned.
#[allow(unsafe_code)] // Documented JNI FFI boundary; no unwinding or longjmp here.
#[no_mangle]
pub unsafe extern "C" fn as_jni_dispatch(
    op: u32,
    a: u64,
    b: u64,
    c: u64,
    data: *const u8,
    length: usize,
) -> u64 {
    let pointer = unsafe { as_native_session() }.cast::<Session>();
    if pointer.is_null() {
        unsafe { as_native_fail(-1005) };
        return 0;
    }
    let session = unsafe { &*pointer };
    let vm = session.vm.get();
    if vm.is_null() {
        unsafe { as_native_fail(-1006) };
        return 0;
    }
    match session.handle(unsafe { &mut *vm }, op, a, b, c, data, length) {
        Ok(value) => value,
        Err(VmExit::Exception(exception)) => {
            session.set_pending(exception);
            0
        }
        Err(e) => {
            *session.fatal.borrow_mut() = Some(e);
            unsafe { as_native_fail(-1002) };
            0
        }
    }
}
pub fn lock() {
    // SAFETY: process-local reentrant lease; paired by the probe's execution scope.
    #[allow(unsafe_code)]
    unsafe {
        as_jni_vm_lock()
    }
}
pub fn unlock() {
    // SAFETY: releases only this thread's matching lease.
    #[allow(unsafe_code)]
    unsafe {
        as_jni_vm_unlock()
    }
}
/// # Safety
/// C holds the VM lease; path is NUL-terminated, buffer is readable/writable
/// for length according to operation. No native pointers survive this call.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_guest_fs(
    op: u32,
    path: *const c_char,
    offset: u64,
    buffer: *mut u8,
    length: usize,
    flags: u32,
) -> i64 {
    let pointer = unsafe { as_native_session() }.cast::<Session>();
    if pointer.is_null() {
        return -38;
    }
    let session = unsafe { &*pointer };
    let vm = session.vm.get();
    if vm.is_null() {
        return -38;
    }
    let result = (|| -> Result<i64> {
        let path = cstr(path as u64)?;
        let fs = unsafe { &*vm }.guest_filesystem()?;
        match op {
            100 => {
                let stat = fs.stat(&path).map_err(|e| error(e.code, e.to_string()))?;
                if stat.as_ref().is_some_and(|m| m.directory) {
                    return Ok(-21);
                }
                if flags & 4 != 0 {
                    fs.write_at(&path, 0, &[], true)
                        .map_err(|e| error(e.code, e.to_string()))?;
                } else if stat.is_none() {
                    if flags & 8 != 0 {
                        fs.write_at(&path, 0, &[], false)
                            .map_err(|e| error(e.code, e.to_string()))?;
                    } else {
                        return Ok(-2);
                    }
                }
                Ok(if flags & 8 != 0 {
                    stat.map(|m| m.length).unwrap_or(0) as i64
                } else {
                    0
                })
            }
            101 => {
                if length > 16 << 20 || buffer.is_null() {
                    return Ok(-22);
                }
                let out = unsafe { core::slice::from_raw_parts_mut(buffer, length) };
                fs.read_at(&path, offset, out)
                    .map(|n| n as i64)
                    .map_err(|e| error(e.code, e.to_string()))
            }
            102 => {
                let input = read_slice::<u8>(buffer, length)?;
                fs.write_at(&path, offset, input, false)
                    .map(|n| n as i64)
                    .map_err(|e| error(e.code, e.to_string()))
            }
            103 => Ok(fs
                .stat(&path)
                .map_err(|e| error(e.code, e.to_string()))?
                .map(|m| m.length as i64)
                .unwrap_or(-2)),
            _ => Ok(-38),
        }
    })();
    match result {
        Ok(value) => value,
        Err(_) => -5,
    }
}
