//! All unsafe address access lives at this documented synchronous FFI boundary.
//! C/libnx owns mappings, executable veneers, thread TLS and machine calls.
use alloc::{
    collections::BTreeMap,
    ffi::CString,
    string::{String, ToString},
    vec::Vec,
};
use as_native::{error, Allocation, CallFrame, Error, NativeBackend, Result};
use core::ffi::CStr;
#[allow(unsafe_code)]
extern "C" {
    fn as_native_allocate(length: usize, address: *mut u64, token: *mut u64) -> i32;
    fn as_native_write(token: u64, offset: usize, input: *const u8, length: usize) -> i32;
    fn as_native_finalize(token: u64, pages: *const u8, length: usize) -> i32;
    fn as_native_release(token: u64);
    fn as_native_register_module(
        token: u64,
        name: *const core::ffi::c_char,
        headers: *const u8,
        length: usize,
    ) -> i32;
    fn as_native_missing_veneer(
        library: *const core::ffi::c_char,
        symbol: *const core::ffi::c_char,
    ) -> u64;
    fn as_native_missing_library() -> *const core::ffi::c_char;
    fn as_native_missing_symbol() -> *const core::ffi::c_char;
    fn as_native_data_import(name: *const core::ffi::c_char) -> u64;
    fn as_native_import(name: *const core::ffi::c_char) -> u64;
    fn as_native_library(name: *const core::ffi::c_char) -> i32;
    fn as_native_veneer(function: u64) -> u64;
    fn as_native_tls(
        module: usize,
        bytes: *const u8,
        length: usize,
        size: usize,
        alignment: usize,
    ) -> u64;
    fn as_native_tlsdesc_address() -> u64;
    fn as_native_call(frame: *mut CallFrame) -> i32;
}
#[derive(Default)]
pub struct HorizonNative {
    imports: BTreeMap<(String, u8), u64>,
    deferred: BTreeMap<(String, String), u64>,
}
pub fn platform_result(operation: &str, code: i32) -> Result<()> {
    rc(operation, code)
}
fn rc(operation: &str, code: i32) -> Result<()> {
    if code == -1211 {
        // SAFETY: C owns stable UTF-8 names until post-quiescence resource reset.
        #[allow(unsafe_code)]
        let names = unsafe {
            let library = as_native_missing_library();
            let symbol = as_native_missing_symbol();
            if library.is_null() || symbol.is_null() {
                None
            } else {
                Some((
                    CStr::from_ptr(library).to_string_lossy().into_owned(),
                    CStr::from_ptr(symbol).to_string_lossy().into_owned(),
                ))
            }
        };
        let (library, symbol) =
            names.unwrap_or_else(|| ("unknown ELF".into(), "unknown function".into()));
        return Err(error("NATIVE_API_UNAVAILABLE", operation, &library, symbol));
    }
    if code == 0 {
        Ok(())
    } else {
        let (name, detail) = match code {
            -1201 => (
                "NATIVE_CPP_ALLOCATION_FAILURE",
                "throwing C++ new failed; exception unwinding unavailable",
            ),
            -1202 => (
                "NATIVE_CPP_PURE_VIRTUAL",
                "pure/deleted virtual function called",
            ),
            -1203 => (
                "NATIVE_CPP_GUARD_STATE",
                "invalid static-initialization guard ownership",
            ),
            -1204 => (
                "NATIVE_CPP_RECURSIVE_INITIALIZATION",
                "recursive static initialization",
            ),
            -1205 => ("NATIVE_PROCESS_EXIT", "guest requested process exit"),
            -1206 => ("NATIVE_ABORT", "guest called abort"),
            -2350 => (
                "VULKAN_CALLBACK_UNAVAILABLE",
                "Vulkan guest allocator/pNext callback translation is unavailable",
            ),
            -2351 => (
                "VULKAN_HANDLE_STATE",
                "Vulkan instance/device lifetime or ownership violation",
            ),
            -1210 => (
                "NATIVE_FORTIFY_BOUNDS",
                "Android fortify object bounds exceeded",
            ),
            _ => ("HORIZON_NATIVE_ERROR", "platform call failed"),
        };
        Err(error(
            name,
            operation,
            "libnx",
            alloc::format!("{detail}; result {code:#x}"),
        ))
    }
}
impl NativeBackend for HorizonNative {
    fn system_library(&self, name: &str) -> bool {
        let Ok(c) = CString::new(name) else {
            return false;
        };
        // SAFETY: synchronous bounded NUL-terminated library lookup.
        #[allow(unsafe_code)]
        unsafe {
            as_native_library(c.as_ptr()) != 0
        }
    }
    fn allocate(&mut self, length: usize) -> Result<Allocation> {
        let (mut address, mut token) = (0, 0);
        // SAFETY: writable scalar outputs; C retains its own allocation, not these pointers.
        #[allow(unsafe_code)]
        let result = unsafe { as_native_allocate(length, &mut address, &mut token) };
        if result != 0 {
            return Err(error(
                if result == -12 {
                    "NATIVE_MEMORY_BUDGET"
                } else {
                    "HORIZON_NATIVE_ERROR"
                },
                "allocate",
                "libnx",
                alloc::format!("{length} mapped native bytes; result {result:#x}"),
            ));
        }
        Ok(Allocation {
            address,
            token,
            length,
        })
    }
    fn write(&mut self, memory: Allocation, offset: usize, bytes: &[u8]) -> Result<()> {
        // SAFETY: token was returned by this backend, bytes are live for the call.
        #[allow(unsafe_code)]
        let result = unsafe { as_native_write(memory.token, offset, bytes.as_ptr(), bytes.len()) };
        rc("write", result)
    }
    fn finalize(&mut self, memory: Allocation, pages: &[u8]) -> Result<()> {
        // SAFETY: C validates page count against its owned allocation.
        #[allow(unsafe_code)]
        let result = unsafe { as_native_finalize(memory.token, pages.as_ptr(), pages.len()) };
        rc("finalize", result)
    }
    fn release(&mut self, memory: Allocation) {
        // SAFETY: each owned loader allocation is released exactly once.
        #[allow(unsafe_code)]
        unsafe {
            as_native_release(memory.token)
        };
    }
    fn register_module(
        &mut self,
        name: &str,
        elf: &as_native::Elf,
        memory: Allocation,
    ) -> Result<()> {
        let name = CString::new(name)
            .map_err(|_| error("INVALID_LIBRARY_NAME", "register", "libnx", name))?;
        // SAFETY: metadata is copied by C while both buffers are alive.
        #[allow(unsafe_code)]
        let result = unsafe {
            as_native_register_module(
                memory.token,
                name.as_ptr(),
                elf.program_headers.as_ptr(),
                elf.program_headers.len(),
            )
        };
        rc("register_module", result)
    }
    fn import(&mut self, _library: &str, name: &str, kind: u8) -> Result<Option<u64>> {
        if !matches!(kind, 0..=2) {
            return Err(error(
                "UNSUPPORTED_IMPORT_KIND",
                "import",
                "libnx",
                alloc::format!("{name}: ELF STT_{kind}"),
            ));
        }
        let key = (name.to_string(), kind);
        if let Some(v) = self.imports.get(&key) {
            return Ok(Some(*v));
        }
        let c = CString::new(name).map_err(|_| error("INVALID_SYMBOL", "import", "libnx", name))?;
        // SAFETY: explicit bounded data lookup; never execute a data address.
        #[allow(unsafe_code)]
        let data = unsafe { as_native_data_import(c.as_ptr()) };
        if kind == 1 || kind == 0 && data != 0 {
            if data == 0 {
                return Ok(None);
            }
            self.imports.insert(key, data);
            return Ok(Some(data));
        }
        if data != 0 {
            return Err(error(
                "NATIVE_IMPORT_KIND",
                "import",
                "libnx",
                alloc::format!("{name}: ELF function refers to data symbol"),
            ));
        }
        // SAFETY: named function lookup does not execute the returned pointer.
        #[allow(unsafe_code)]
        let target = unsafe { as_native_import(c.as_ptr()) };
        if target == 0 {
            return Ok(None);
        }
        let address = veneer(target)?;
        self.imports.insert(key, address);
        Ok(Some(address))
    }
    fn defer_function(&mut self, library: &str, symbol: &str) -> Result<Option<u64>> {
        let key = (library.to_string(), symbol.to_string());
        if let Some(address) = self.deferred.get(&key) {
            return Ok(Some(*address));
        }
        let library_c = CString::new(library)
            .map_err(|_| error("INVALID_LIBRARY_NAME", "defer", library, symbol))?;
        let symbol_c =
            CString::new(symbol).map_err(|_| error("INVALID_SYMBOL", "defer", library, symbol))?;
        // SAFETY: C copies both names and returns a non-returning checked trap
        // veneer. It does not call code or register a successful API provider.
        #[allow(unsafe_code)]
        let address = unsafe { as_native_missing_veneer(library_c.as_ptr(), symbol_c.as_ptr()) };
        if address == 0 {
            return Err(error("NATIVE_IMPORT_BUDGET", "defer", library, symbol));
        }
        self.deferred.insert(key, address);
        Ok(Some(address))
    }
    fn deferred_imports(&self) -> Vec<as_native::DeferredImport> {
        self.deferred
            .keys()
            .map(|(library, symbol)| as_native::DeferredImport {
                library: library.clone(),
                symbol: symbol.clone(),
            })
            .collect()
    }
    fn tls(&mut self, module: usize, bytes: &[u8], size: usize, alignment: usize) -> Result<u64> {
        // SAFETY: template is copied by C before the call returns.
        #[allow(unsafe_code)]
        let offset = unsafe { as_native_tls(module, bytes.as_ptr(), bytes.len(), size, alignment) };
        if offset == 0 {
            Err(error(
                "TLS_LIMIT",
                "allocate",
                "libnx",
                "64 KiB guest TLS, alignment <=4096",
            ))
        } else {
            Ok(offset)
        }
    }
    fn tls_descriptor(&mut self, offset: u64) -> Result<(u64, u64)> {
        // SAFETY: assembly resolver preserves TLSDESC ABI; it does not enter host TLS.
        #[allow(unsafe_code)]
        let address = unsafe { as_native_tlsdesc_address() };
        Ok((address, offset))
    }
}
pub fn veneer(target: u64) -> Result<u64> {
    // SAFETY: C generates a bridge without executing the target.
    #[allow(unsafe_code)]
    let address = unsafe { as_native_veneer(target) };
    if address == 0 {
        Err(error(
            "NATIVE_ALLOCATION_FAILED",
            "veneer",
            "libnx",
            "executable mapping",
        ))
    } else {
        Ok(address)
    }
}
pub fn call(frame: &mut CallFrame) -> Result<()> {
    // SAFETY: frame is a valid repr(C) object; function comes from the validated
    // loader or a registered native method. C restores host TLS on every exit.
    #[allow(unsafe_code)]
    let result = unsafe { as_native_call(frame) };
    rc("call", result)
}
pub fn vm_error(error: Error) -> rd_vm::VmExit {
    rd_vm::err::vm_error(
        &error.code,
        alloc::format!("{}: {}", error.operation, error.cause),
    )
    .into()
}
