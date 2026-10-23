//! All unsafe address access lives at this documented synchronous FFI boundary.
//! C/libnx owns mappings, executable veneers, thread TLS and machine calls.
use alloc::{
    collections::BTreeMap,
    ffi::CString,
    string::{String, ToString},
};
use as_native::{error, Allocation, CallFrame, Error, NativeBackend, Result};
#[allow(unsafe_code)]
extern "C" {
    fn as_native_allocate(length: usize, address: *mut u64, token: *mut u64) -> i32;
    fn as_native_write(token: u64, offset: usize, input: *const u8, length: usize) -> i32;
    fn as_native_finalize(token: u64, pages: *const u8, length: usize) -> i32;
    fn as_native_release(token: u64);
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
    imports: BTreeMap<String, u64>,
}
fn rc(operation: &str, code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(error(
            "HORIZON_NATIVE_ERROR",
            operation,
            "libnx",
            alloc::format!("result {code:#x}"),
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
        rc("allocate", result)?;
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
    fn import(&mut self, _library: &str, name: &str) -> Result<Option<u64>> {
        if let Some(v) = self.imports.get(name) {
            return Ok(Some(*v));
        }
        let c = CString::new(name).map_err(|_| error("INVALID_SYMBOL", "import", "libnx", name))?;
        // SAFETY: named lookup returns a host function address; no code is executed.
        #[allow(unsafe_code)]
        let target = unsafe { as_native_import(c.as_ptr()) };
        if target == 0 {
            return Ok(None);
        }
        let address = if name == "__stack_chk_guard" || name.starts_with("SL_IID_") {
            target
        } else {
            veneer(target)?
        };
        self.imports.insert(name.to_string(), address);
        Ok(Some(address))
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
