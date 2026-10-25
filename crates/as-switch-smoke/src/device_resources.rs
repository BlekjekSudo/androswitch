//! Actual Horizon process capabilities, queried at each Java Runtime call.
use alloc::{format, rc::Rc, string::String};
use as_platform::{PlatformError, RuntimeResources};

#[allow(unsafe_code)]
extern "C" {
    fn as_platform_available_processors(value: *mut u32) -> i32;
    fn as_platform_resource_query(kind: u32, value: *mut u64) -> i32;
    fn as_platform_renderer_name() -> *const core::ffi::c_char;
}
struct HorizonResources;
impl RuntimeResources for HorizonResources {
    fn system_property(&self, key: &str) -> Result<Option<String>, PlatformError> {
        if key == "androswitch.host.os.name" {
            return Ok(Some("Horizon".into()));
        }
        if key == "androswitch.host.renderer.backend" {
            // SAFETY: platform returns a process-owned static NUL-terminated
            // provider identity, or null when the product media backend is absent.
            #[allow(unsafe_code)]
            let name = unsafe {
                let pointer = as_platform_renderer_name();
                if pointer.is_null() {
                    None
                } else {
                    core::ffi::CStr::from_ptr(pointer).to_str().ok()
                }
            };
            return Ok(name.map(String::from));
        }
        let kind = match key {
            "androswitch.host.cpu.allowed_mask" => 0,
            "androswitch.host.cpu.clock_hz" => 1,
            "androswitch.host.gpu.clock_hz" => 2,
            "androswitch.host.memory.clock_hz" => 3,
            "androswitch.host.tick.frequency_hz" => 4,
            "androswitch.host.os.version" => 5,
            _ => return Ok(None),
        };
        let mut value = 0u64;
        // SAFETY: C writes one scalar after a successful read-only query. It
        // closes every acquired service/session and retains no pointer.
        #[allow(unsafe_code)]
        let rc = unsafe { as_platform_resource_query(kind, &mut value) };
        if rc != 0 {
            return Err(PlatformError {
                code: "HORIZON_RESOURCE_ERROR",
                operation: "resource_query",
                cause: format!("{key}: result={rc:#010x}"),
            });
        }
        Ok(Some(if kind == 0 {
            format!("{value:#018x}")
        } else if kind == 5 {
            format!(
                "{}.{}.{}",
                (value >> 16) & 255,
                (value >> 8) & 255,
                value & 255
            )
        } else {
            format!("{value}")
        }))
    }
    fn available_processors(&self) -> Result<u32, PlatformError> {
        let mut count = 0;
        // SAFETY: C writes this scalar only after a successful process CoreMask
        // query and validation. It retains no pointer and invents no fallback.
        #[allow(unsafe_code)]
        let result = unsafe { as_platform_available_processors(&mut count) };
        if result == 0 && count != 0 {
            Ok(count)
        } else {
            Err(PlatformError {
                code: "HORIZON_RESOURCE_ERROR",
                operation: "available_processors",
                cause: format!(
                    "process allowed CoreMask query/count failed: {result:#010x}, count={count}"
                ),
            })
        }
    }
}
pub fn bind(android: &as_framework::Android) {
    android.bind_runtime_resources(Rc::new(HorizonResources));
}
