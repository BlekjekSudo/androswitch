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
    fn as_native_jni_failure(
        operation: u32,
        a: u64,
        b: u64,
        c: u64,
        detail: *const u8,
        length: usize,
    );
    fn as_native_finalize_callbacks() -> i32;
    fn as_native_finalize_thread_callbacks() -> i32;
    fn as_native_pending_callbacks() -> u32;
    fn as_jni_vm_lock();
    fn as_jni_vm_unlock();
    fn as_jni_vm_release() -> u32;
    fn as_jni_vm_acquire(depth: u32);
}
fn jni_diagnostic(op: u32, a: u64, b: u64, c: u64, detail: &str) {
    // SAFETY: C copies at most 4 KiB synchronously, retaining no Rust pointer.
    // Only API identifiers/error classes are supplied; Java string contents
    // and arbitrary application exception messages are deliberately omitted.
    #[allow(unsafe_code)]
    unsafe {
        as_native_jni_failure(op, a, b, c, detail.as_ptr(), detail.len());
    }
}
use crate::jni_exceptions::PendingException as ThreadState;
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
    rooted: Cell<bool>,
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
            loader: RefCell::new(Loader::with_source(
                HorizonNative::default(),
                crate::native_probe::library_source(apk),
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
            rooted: Cell::new(true),
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
        // mappings/session rooted until quiescence is proved during shutdown.
        let _process_root = Rc::into_raw(session.clone());
        Ok(session)
    }
    pub fn bind_vm(&self, vm: &mut Engine) {
        self.vm.set(vm as *mut Engine);
    }
    pub fn close_vm(&self) {
        self.vm.set(core::ptr::null_mut());
    }
    /// Caller has joined every guest/platform callback thread before releasing
    /// the retained root. Engine/Running still own Rc references during this call.
    pub fn release_root(self: &Rc<Self>) {
        if self.rooted.replace(false) {
            // SAFETY: balances exactly one Rc::into_raw in Session::new, after quiescence.
            #[allow(unsafe_code)]
            unsafe {
                Rc::decrement_strong_count(Rc::as_ptr(self));
            }
        }
    }
    pub fn native_entry(&self, vm: &mut Engine, library: &str, symbol: &str) -> Result<u64> {
        NativeHost::load(self, vm, library, false)?;
        self.loader
            .borrow()
            .symbol(symbol)
            .ok_or_else(|| error("SYMBOL_NOT_FOUND", symbol))
    }
    pub fn native_call(&self, vm: &mut Engine, function: u64, args: &[u64]) -> Result<u64> {
        let mut frame = CallFrame {
            function,
            ..Default::default()
        };
        if args.len() > 8 {
            return Err(error("NATIVE_ARGUMENT_LIMIT", "callback integer args"));
        }
        frame.integer[..args.len()].copy_from_slice(args);
        self.foreign(vm, &mut frame)?;
        Ok(frame.result_integer[0])
    }
    /// Caller holds the VM lease and has quiesced guest/platform workers.
    /// Never retain a loader/VM borrow across a guest destructor callback.
    pub fn finalize_libraries(&self, vm: &mut Engine) -> Result<()> {
        self.finalize_callback_registry(vm, true)?;
        for _ in 0..4096 {
            let functions = {
                let mut loader = self.loader.borrow_mut();
                let module = loader
                    .modules
                    .iter_mut()
                    .rev()
                    .find(|module| module.initialized && !module.finalized);
                module.map(|module| {
                    module.finalized = true;
                    module.destructors.clone()
                })
            };
            if let Some(functions) = functions {
                for function in functions {
                    self.native_call(vm, function, &[])?;
                }
            } else {
                self.finalize_callback_registry(vm, false)?;
                // A global/TLS callback can load another library. Its fini
                // functions must run before its executable memory is dropped.
                if !self.finalization_pending() {
                    return Ok(());
                }
            }
        }
        Err(error(
            "NATIVE_FINALIZE_LIMIT",
            "too many libraries or callback batches during finalization",
        ))
    }
    /// Caller holds the VM lease. Join guest/platform workers before using a
    /// false result to release roots, TLS, or executable mappings.
    pub fn finalization_pending(&self) -> bool {
        let modules_pending = self
            .loader
            .borrow()
            .modules
            .iter()
            .any(|module| module.initialized && !module.finalized);
        // SAFETY: C reads host-owned callback registries; no guest addresses
        // are dereferenced or callbacks executed. Current host thread owns TLS.
        #[allow(unsafe_code)]
        let callbacks_pending = unsafe { as_native_pending_callbacks() } != 0;
        modules_pending || callbacks_pending
    }
    fn finalize_callback_registry(&self, vm: &mut Engine, thread_local: bool) -> Result<()> {
        let previous = self.vm.replace(vm as *mut Engine);
        // SAFETY: callback registry is owned by the current application. Release
        // the reentrant lease so a destructor can create/join a JNI thread.
        #[allow(unsafe_code)]
        let depth = unsafe { as_jni_vm_release() };
        #[allow(unsafe_code)]
        let result = unsafe {
            if thread_local {
                as_native_finalize_thread_callbacks()
            } else {
                as_native_finalize_callbacks()
            }
        };
        #[allow(unsafe_code)]
        unsafe {
            as_jni_vm_acquire(depth)
        };
        self.vm.set(previous);
        if let Some(error) = self.fatal.borrow_mut().take() {
            return Err(error);
        }
        device_native::platform_result("finalize_callbacks", result).map_err(jni)
    }
    /// Platform render/media worker calls these under the reentrant VM lease.
    /// C++/pthread TLS destructors must finish before detaching its JNI roots.
    pub fn attach_runtime_thread(&self) {
        self.refs.borrow_mut().attach(thread());
        self.threads.borrow_mut().entry(thread()).or_default();
    }
    pub fn detach_runtime_thread(&self) {
        // SAFETY: platform detach holds the VM lease, after TLS callbacks;
        // close_vm clears this pointer before the Engine can be dropped.
        #[allow(unsafe_code)]
        if let Some(vm) = unsafe { self.vm.get().as_ref() } {
            if let Some(host) = vm.framework_host() {
                host.thread_detached(thread());
            }
        }
        self.refs.borrow_mut().detach(thread());
        self.threads.borrow_mut().remove(&thread());
    }
    pub fn record_runtime_error(&self, error: VmExit) {
        let mut fatal = self.fatal.borrow_mut();
        if fatal.is_none() {
            *fatal = Some(error);
        }
    }
    pub fn runtime_error(&self) -> Option<String> {
        self.fatal.borrow().as_ref().map(ToString::to_string)
    }
    /// Acknowledge an execution failure after every worker/callback has joined,
    /// while holding the VM lease and before entering any native finalizer.
    /// New failures raised by a destructor remain fatal finalization failures.
    pub fn take_runtime_error(&self) -> Option<String> {
        self.fatal
            .borrow_mut()
            .take()
            .map(|error| error.to_string())
    }
    pub fn activity_reference(&self, object: ObjRef) -> Result<u64> {
        let local = self.local(Handle::Object(object))?;
        let global = self
            .refs
            .borrow_mut()
            .duplicate(thread(), local, true)
            .map_err(jni)?;
        self.refs
            .borrow_mut()
            .delete(thread(), local, false)
            .map_err(jni)?;
        Ok(global)
    }
    fn class(&self, vm: &mut Engine, token: u64) -> Result<String> {
        crate::jni_objects::class_descriptor(vm, &self.refs, thread(), token)
    }
    fn object(&self, token: u64) -> Result<ObjRef> {
        crate::jni_objects::object(&self.refs, thread(), token)
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
    fn set_pending(&self, vm: &Engine, t: Throwable) {
        self.threads
            .borrow_mut()
            .entry(thread())
            .or_default()
            .set(vm, t);
    }
    fn value(&self, bits: u64, kind: &str) -> Result<Value> {
        crate::jni_objects::from_bits(&self.refs, thread(), bits, kind)
    }
    fn bits(&self, value: &Value, kind: &str) -> Result<u64> {
        crate::jni_objects::to_bits(&self.refs, thread(), value, kind)
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
    pub fn deferred_imports(&self) -> Vec<as_native::DeferredImport> {
        use as_native::NativeBackend;
        self.loader.borrow().backend.deferred_imports()
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
                    name.clone()
                } else {
                    format!("L{name};")
                };
                let result =
                    crate::jni_objects::dispatch(vm, &self.refs, thread(), op, a, b, Some(&name));
                if matches!(&result, Err(VmExit::Exception(e)) if e.class == "Ljava/lang/NoClassDefFoundError;")
                {
                    jni_diagnostic(op, a, b, c, &format!("missing_class={class}"));
                }
                result
            }
            2 => {
                let class = self.class(vm, a)?;
                let name = cstr(b)?;
                let sig = cstr(c)?;
                rd_jni::descriptor(&sig).map_err(|e| error(&e.code, e.cause))?;
                let is_static = length != 0;
                let declaring =
                    crate::jni_constructor::method_owner(vm, &class, &name, &sig, is_static)
                        .map_err(|e| {
                            jni_diagnostic(
                                op,
                                a,
                                b,
                                c,
                                &format!("missing_method={class}->{name}{sig} static={is_static}"),
                            );
                            e
                        })?;
                vm.ensure_initialized(&class)?;
                self.refs
                    .borrow_mut()
                    .id(
                        thread(),
                        Handle::Method {
                            class: declaring,
                            name,
                            sig,
                            is_static,
                        },
                    )
                    .map_err(jni)
            }
            34 => {
                let class = self.class(vm, a)?;
                let name = cstr(b)?;
                let signature = cstr(c)?;
                if length != 1 {
                    return Err(error("API_UNAVAILABLE", "JNI instance field lookup"));
                }
                crate::jni_static_fields::id(vm, &self.refs, thread(), &class, &name, &signature)
            }
            35 => {
                let class = self.class(vm, a)?;
                crate::jni_static_fields::get(vm, &self.refs, thread(), &class, b, c as u8)
            }
            36 => {
                if data.is_null() || length != 8 {
                    return Err(error("JNI_FIELD_ARGUMENT", "expected scalar field bits"));
                }
                let class = self.class(vm, a)?;
                let bytes = read_slice::<u8>(data, 8)?;
                let bits = u64::from_le_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| error("JNI_FIELD_ARGUMENT", "field width"))?,
                );
                crate::jni_static_fields::set(vm, &self.refs, thread(), &class, b, c as u8, bits)?;
                Ok(0)
            }
            3 => {
                let class = self.class(vm, a)?;
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
                let invocation = crate::jni_objects::Invocation::decode(
                    &self.refs,
                    thread(),
                    a,
                    b,
                    c != 0,
                    read_slice::<u64>(data, length)?,
                )?;
                self.callbacks.set(self.callbacks.get() + 1);
                invocation.invoke(vm, &self.refs, thread())
            }
            5 => crate::jni_objects::dispatch(vm, &self.refs, thread(), op, a, b, None),
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
            13..=19 => crate::jni_objects::dispatch(vm, &self.refs, thread(), op, a, b, None),
            20 => Ok(self.pending().is_some() as u64),
            21 => {
                self.threads.borrow_mut().remove(&thread());
                Ok(0)
            }
            39 => {
                // Release the RefCell borrow before any virtual Java callback.
                let mut pending = self
                    .threads
                    .borrow_mut()
                    .remove(&thread())
                    .unwrap_or_default();
                pending.describe(vm)?;
                Ok(0)
            }
            22 => {
                let exception = Throwable::new(&self.class(vm, a)?, text(data, length)?);
                self.set_pending(vm, exception);
                Ok(0)
            }
            23 => {
                if let Some(mut pending) = self.pending() {
                    let object = if let Some(object) = pending.obj {
                        object
                    } else {
                        let object = vm.heap.alloc_instance(pending.class.clone(), Vec::new())?;
                        pending.obj = Some(object);
                        self.threads
                            .borrow_mut()
                            .entry(thread())
                            .or_default()
                            .pending = Some(pending);
                        object
                    };
                    self.local(Handle::Object(object))
                } else {
                    Ok(0)
                }
            }
            24 => {
                let object = self.object(a)?;
                self.set_pending(
                    vm,
                    Throwable {
                        class: vm.heap.class_of(object)?.to_string(),
                        message: None,
                        obj: Some(object),
                    },
                );
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
                let name = if data.is_null() {
                    None
                } else {
                    Some(
                        rd_jni::decode_modified_utf8(read_slice::<u8>(data, length)?)
                            .map_err(|e| error(&e.code, e.cause))?,
                    )
                };
                if let Some(host) = vm.framework_host() {
                    host.thread_attached(thread(), name.as_deref())?;
                }
                self.refs.borrow_mut().attach(thread());
                self.threads.borrow_mut().entry(thread()).or_default();
                Ok(0)
            }
            27 => {
                if let Some(host) = vm.framework_host() {
                    host.thread_detached(thread());
                }
                self.refs.borrow_mut().detach(thread());
                self.threads.borrow_mut().remove(&thread());
                Ok(0)
            }
            31 => crate::jni_objects::dispatch(vm, &self.refs, thread(), op, a, b, None),
            32 => {
                let class = self.class(vm, a)?;
                vm.ensure_initialized(&class)?;
                let object = vm.heap.alloc_instance(class.clone(), Vec::new())?;
                vm.attach_host_state(&class, object)?;
                self.local(Handle::Object(object))
            }
            33 => crate::jni_objects::dispatch(vm, &self.refs, thread(), op, a, b, None),
            37 => {
                let class = self.class(vm, a)?;
                self.registrations
                    .borrow_mut()
                    .retain(|(c, _, _), _| c != &class);
                Ok(0)
            }
            38 => {
                // Pending Java exceptions are preserved; construction does
                // not clear them or expose an uninitialized object to native.
                if self.pending().is_some() {
                    return Ok(0);
                }
                let class = self.class(vm, a)?;
                let (declaring, name, sig, is_static) = self.method(b)?;
                if declaring != class || name != "<init>" || is_static {
                    jni_diagnostic(op, a, b, c, &format!("constructor_kind={declaring}->{name}{sig} allocated={class} static={is_static}"));
                    return Err(error(
                        "JNI_CONSTRUCTOR_METHOD",
                        "NewObject requires the allocated class's instance constructor",
                    ));
                }
                let (types, _) = rd_jni::descriptor(&sig).map_err(|e| error(&e.code, e.cause))?;
                if length != types.len() {
                    return Err(error("JNI_SIGNATURE", "constructor argument count"));
                }
                let values = read_slice::<u64>(data, length)?
                    .iter()
                    .zip(&types)
                    .map(|(bits, ty)| self.value(*bits, ty))
                    .collect::<Result<Vec<_>>>()?;
                let mut local = None;
                let result =
                    crate::jni_constructor::construct(vm, &class, &sig, &values, |object| {
                        let handle = self.local(Handle::Object(object))?;
                        local = Some(handle);
                        Ok(handle)
                    });
                if result.is_err() {
                    jni_diagnostic(op, a, b, c, &format!("constructor={class}-><init>{sig}"));
                    if let Some(handle) = local {
                        self.refs
                            .borrow_mut()
                            .delete(thread(), handle, false)
                            .map_err(jni)?;
                    }
                }
                result
            }
            u32::MAX => {
                let name = text(data, length)?;
                jni_diagnostic(op, a, b, c, &format!("unavailable_jni_slot={a} api={name}"));
                Err(error(
                    "JNI_API_UNAVAILABLE",
                    format!("JNI slot {a}: {name}"),
                ))
            }
            _ => Err(error(
                "JNI_API_UNAVAILABLE",
                format!("JNI bridge operation {op}"),
            )),
        }
    }
}
impl NativeHost for Session {
    fn current_thread_id(&self) -> Option<u64> {
        Some(thread())
    }
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
                crate::jni_objects::local_class(vm, &self.refs, thread(), class)?
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
        jni_diagnostic(op, a, b, c, "error=JNI_SESSION_UNAVAILABLE");
        unsafe { as_native_fail(-1005) };
        return 0;
    }
    let session = unsafe { &*pointer };
    let vm = session.vm.get();
    if vm.is_null() {
        jni_diagnostic(op, a, b, c, "error=JNI_VM_UNAVAILABLE");
        unsafe { as_native_fail(-1006) };
        return 0;
    }
    match session.handle(unsafe { &mut *vm }, op, a, b, c, data, length) {
        Ok(value) => value,
        Err(VmExit::Exception(exception)) => {
            jni_diagnostic(op, a, b, c, &format!("exception={}", exception.class));
            session.set_pending(unsafe { &*vm }, exception);
            0
        }
        Err(e) => {
            if let VmExit::Error(ref error) = e {
                jni_diagnostic(
                    op,
                    a,
                    b,
                    c,
                    &format!("error={} module={}", error.code, error.module_id),
                );
            }
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
/// C holds the VM lease. Buffer/name are valid for this synchronous operation.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_guest_asset(
    op: u32,
    id: u64,
    name: *const c_char,
    offset: i64,
    origin: i32,
    output: *mut u8,
    length: usize,
) -> i64 {
    let pointer = unsafe { as_native_session() }.cast::<Session>();
    if pointer.is_null() {
        return -38;
    }
    let session = unsafe { &*pointer };
    if session.vm.get().is_null() {
        return -38;
    }
    let vm = unsafe { &mut *session.vm.get() };
    let Some(framework) = vm.framework_host() else {
        return -38;
    };
    let result = (|| -> Result<i64> {
        Ok(match op {
            5 => {
                (vm.heap.class_of(session.object(id)?)? == "Landroid/content/res/AssetManager;")
                    as i64
            }
            0 => framework.asset_open(&cstr(name as u64)?)? as i64,
            1 => {
                if length == 0 {
                    return Ok(0);
                }
                if length > 1 << 24 || output.is_null() {
                    return Err(error("INVALID_ASSET_BUFFER", "read"));
                }
                framework.asset_read(id, unsafe {
                    core::slice::from_raw_parts_mut(output, length)
                })? as i64
            }
            2 => framework.asset_seek(id, offset, origin)? as i64,
            3 => framework.asset_len(id)? as i64,
            4 => {
                framework.asset_close(id);
                0
            }
            _ => return Err(error("API_UNAVAILABLE", "asset operation")),
        })
    })();
    match result {
        Ok(n) => n,
        Err(VmExit::Error(e))
            if matches!(
                e.code.as_str(),
                "MISSING_ENTRY"
                    | "INVALID_ASSET_PATH"
                    | "INVALID_SEEK"
                    | "ASSET_CLOSED"
                    | "INVALID_ASSET_BUFFER"
            ) =>
        {
            match e.code.as_str() {
                "MISSING_ENTRY" => -2,
                "ASSET_CLOSED" => -9,
                _ => -22,
            }
        }
        Err(e) => {
            *session.fatal.borrow_mut() = Some(e);
            unsafe { as_native_fail(-1010) };
            -5
        }
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
            104 => {
                if length < 24 || buffer.is_null() {
                    return Ok(-22);
                }
                let Some(meta) = fs.stat(&path).map_err(|e| error(e.code, e.to_string()))? else {
                    return Ok(-2);
                };
                let resolved = match fs.resolve(&path) {
                    Ok(resolved) => resolved,
                    Err(_) if meta.directory && path.starts_with("/data/app") => path.clone(),
                    Err(error) => return Err(self::error(error.code, error.to_string())),
                };
                let readonly =
                    resolved.starts_with("installed/") || resolved.starts_with("/data/app");
                let inode = resolved
                    .bytes()
                    .fold(14695981039346656037u64, |hash, byte| {
                        (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
                    });
                let output = unsafe { core::slice::from_raw_parts_mut(buffer, 24) };
                output[..8].copy_from_slice(&meta.length.to_le_bytes());
                let flags = u64::from(meta.directory) | (u64::from(readonly) << 1);
                output[8..16].copy_from_slice(&flags.to_le_bytes());
                output[16..24].copy_from_slice(&inode.max(1).to_le_bytes());
                Ok(0)
            }
            105 => {
                let Some(meta) = fs.stat(&path).map_err(|e| error(e.code, e.to_string()))? else {
                    return Ok(-2);
                };
                if meta.directory
                    && !fs
                        .list(&path)
                        .map_err(|e| error(e.code, e.to_string()))?
                        .is_empty()
                {
                    return Ok(-39);
                }
                fs.remove(&path)
                    .map(|_| 0)
                    .map_err(|e| error(e.code, e.to_string()))
            }
            106 => {
                if length < 257 || buffer.is_null() || flags > 4096 {
                    return Ok(-22);
                }
                let entries = fs.list(&path).map_err(|e| error(e.code, e.to_string()))?;
                if entries.len() > 4096 {
                    return Ok(-75);
                }
                let Some(entry) = entries.get(flags as usize) else {
                    return Ok(0);
                };
                if entry.name.len() > 255 {
                    return Ok(-36);
                }
                let output = unsafe { core::slice::from_raw_parts_mut(buffer, 257) };
                output.fill(0);
                output[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
                output[256] = u8::from(entry.directory);
                Ok(1)
            }
            107 => {
                if buffer.is_null() || length > 65536 {
                    return Ok(-22);
                }
                let destination = cstr(buffer as u64)?;
                fs.rename_new(&path, &destination)
                    .map(|_| 0)
                    .map_err(|e| error(e.code, e.to_string()))
            }
            _ => Ok(-38),
        }
    })();
    match result {
        Ok(value) => value,
        Err(VmExit::Error(error)) => match error.code.as_str() {
            "NOT_FOUND" => -2,
            "ACCESS_DENIED" | "UNMAPPED_PATH" => -13,
            "READ_ONLY_FILESYSTEM" => -30,
            "API_UNAVAILABLE" => -38,
            "ALREADY_EXISTS" => -17,
            "INVALID_PATH" => -22,
            "NOT_DIRECTORY" => -20,
            "DIRECTORY_NOT_EMPTY" => -39,
            _ => -5,
        },
        Err(_) => -5,
    }
}

/// Synchronous native log bridge. C holds the VM lease; strings are live and
/// bounded NUL-terminated input. Native and Java logging share one service.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_guest_log(
    priority: i32,
    tag: *const c_char,
    message: *const c_char,
) -> i32 {
    let pointer = unsafe { as_native_session() }.cast::<Session>();
    if pointer.is_null() {
        return -38;
    }
    let session = unsafe { &*pointer };
    let vm = session.vm.get();
    if vm.is_null() {
        return -38;
    }
    let result = (|| -> Result<i32> {
        let tag = cstr(tag as u64)?;
        let message = cstr(message as u64)?;
        let method = match priority {
            2 => "v",
            3 => "d",
            5 => "w",
            6 => "e",
            7 => "wtf",
            _ => "i",
        };
        let result = unsafe { &mut *vm }.invoke_static(
            "Landroid/util/Log;",
            method,
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[Value::StrPlaceholder(tag), Value::StrPlaceholder(message)],
        )?;
        Ok(result.as_int()?)
    })();
    match result {
        Ok(length) => length,
        Err(error) => {
            *session.fatal.borrow_mut() = Some(error);
            unsafe { as_native_fail(-1002) };
            -5
        }
    }
}

/// Preserve native worker errors before its host/guest TLS is released. C calls
/// this with host TLS; the VM lease serializes access to Session's fatal state.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_native_record_worker_error(code: i32) {
    let pointer = unsafe { as_native_session() }.cast::<Session>();
    if pointer.is_null() {
        return;
    }
    if let Err(error) = device_native::platform_result("native worker", code) {
        lock();
        unsafe { &*pointer }.record_runtime_error(device_native::vm_error(error));
        unlock();
    }
}
