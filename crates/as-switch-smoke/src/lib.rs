//! M1 proof: actual upstream APK/DEX parsers and interpreter without `std`.
//! Method execution is not an Android Application/Activity lifecycle runner.
#![cfg_attr(target_os = "horizon", no_std)]
#![cfg_attr(target_os = "horizon", feature(alloc_error_handler))]

extern crate alloc;

#[cfg(target_os = "horizon")]
pub mod device_fs;
#[cfg(target_os = "horizon")]
pub mod device_jni;
#[cfg(target_os = "horizon")]
pub mod device_native;
pub mod native_probe;
pub mod probe;

use alloc::vec::Vec;

#[repr(C)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DexSummary {
    pub version: u32,
    pub file_size: u32,
    pub classes: u32,
    pub methods: u32,
    pub strings: u32,
    pub checksum_ok: u32,
}

pub fn summarize_dex(bytes: Vec<u8>) -> Result<DexSummary, rd_dex::RdError> {
    let dex = rd_dex::Dex::parse(bytes)?;
    Ok(DexSummary {
        version: dex
            .header
            .version
            .as_str()
            .parse()
            .map_err(|_| rd_dex::RdError::parse("invalid DEX version"))?,
        file_size: dex.header.file_size,
        classes: dex.header.class_defs_size,
        methods: dex.header.method_ids_size,
        strings: dex.header.string_ids_size,
        checksum_ok: u32::from(dex.verify_checksum()),
    })
}

pub fn allocator_probe() -> u64 {
    let values: Vec<u128> = (0..256u128).collect();
    values.into_iter().sum::<u128>() as u64
}

/// # Safety
/// `bytes` must reference `length` readable bytes and `report` must reference a
/// writable, aligned `DexSummary`. The C caller owns both buffers for this call.
/// No guest or VM heap pointers are retained beyond the call.
#[allow(unsafe_code)] // ADR 011: bounded C ABI slice conversion and output write.
#[no_mangle]
pub unsafe extern "C" fn as_smoke_parse_dex(
    bytes: *const u8,
    length: usize,
    report: *mut DexSummary,
) -> u32 {
    if bytes.is_null() || report.is_null() || length > 64 * 1024 * 1024 {
        return 1;
    }
    // SAFETY: required by this function's C ABI contract; cloned before parsing.
    let input = unsafe { core::slice::from_raw_parts(bytes, length) }.to_vec();
    match summarize_dex(input) {
        Ok(summary) => {
            // SAFETY: caller provided a writable, correctly aligned output.
            unsafe { report.write(summary) };
            0
        }
        Err(_) => 2,
    }
}

#[allow(unsafe_code)] // ADR 011: exported symbol only, no pointer access.
#[no_mangle]
pub extern "C" fn as_smoke_allocator_probe() -> u64 {
    allocator_probe()
}

/// # Safety
/// Both input pointers reference their stated readable lengths. Output points
/// to `capacity` writable bytes and must not overlap either input. No raw
/// pointers are retained. Returns bytes written; zero means invalid FFI input
/// or an output buffer too small. Parse/VM errors are serialized as JSON.
#[allow(unsafe_code)] // Bounded buffers owned by the libnx C caller; ADR 011.
#[no_mangle]
pub unsafe extern "C" fn as_smoke_run_apk(
    apk: *const u8,
    apk_len: usize,
    request: *const u8,
    request_len: usize,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if (apk.is_null() && apk_len != 0)
        || request.is_null()
        || output.is_null()
        || apk_len > 64 << 20
        || request_len > 64 << 10
        || capacity > 1 << 20
    {
        return 0;
    }
    // SAFETY: caller's documented input buffer contract; APK is owned by Rust.
    let apk = if apk_len == 0 {
        Vec::new()
    } else {
        unsafe { core::slice::from_raw_parts(apk, apk_len) }.to_vec()
    };
    let request = unsafe { core::slice::from_raw_parts(request, request_len) };
    #[cfg(not(target_os = "horizon"))]
    let report = probe::run_json(apk, request);
    #[cfg(target_os = "horizon")]
    let report = match device_fs::LibnxFileSystem::new("sdmc:/switch/androswitch") {
        Ok(fs) => probe::run_json_with_fs(apk, request, Some(alloc::rc::Rc::new(fs))),
        Err(error) => serde_json::to_vec(&probe::Failure {
            code: error.code.into(),
            module_id: "as-platform".into(),
            cause: alloc::string::ToString::to_string(&error),
        })
        .unwrap_or_default(),
    };
    if report.len() > capacity {
        return 0;
    }
    // SAFETY: caller supplies disjoint, writable output of at least capacity.
    unsafe {
        core::ptr::copy_nonoverlapping(report.as_ptr(), output, report.len());
    }
    report.len()
}

#[cfg(target_os = "horizon")]
mod device {
    use core::{
        alloc::{GlobalAlloc, Layout},
        panic::PanicInfo,
    };

    #[allow(unsafe_code)] // ADR 011: functions are provided by the libnx smoke host.
    extern "C" {
        fn as_host_alloc(alignment: usize, size: usize) -> *mut u8;
        fn as_host_free(pointer: *mut u8);
        fn as_host_panic() -> !;
    }

    struct LibnxAllocator;

    #[allow(unsafe_code)] // ADR 011: allocation ownership stays with newlib.
    unsafe impl GlobalAlloc for LibnxAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: Layout provides nonzero size and power-of-two alignment.
            unsafe { as_host_alloc(layout.align(), layout.size()) }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
            // SAFETY: pointer was returned by the matching libnx allocator.
            unsafe { as_host_free(pointer) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: LibnxAllocator = LibnxAllocator;

    #[panic_handler]
    #[allow(unsafe_code)] // ADR 011: the C host logs and aborts this smoke process.
    fn panic(_info: &PanicInfo<'_>) -> ! {
        unsafe { as_host_panic() }
    }

    #[alloc_error_handler]
    fn allocation_error(_layout: Layout) -> ! {
        panic!("allocation failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_checksum_is_stable() {
        assert_eq!(allocator_probe(), 32640);
    }

    #[test]
    fn malformed_dex_returns_a_real_parser_error() {
        assert_eq!(
            summarize_dex(alloc::vec![1, 2, 3]).unwrap_err().module_id,
            "rd-dex"
        );
    }

    #[test]
    #[allow(unsafe_code)] // Exercise the C ABI's null/length validation only.
    fn ffi_rejects_null_buffers_before_reading() {
        let mut report = DexSummary::default();
        assert_eq!(
            unsafe { as_smoke_parse_dex(core::ptr::null(), 1, &mut report) },
            1
        );
    }
}
