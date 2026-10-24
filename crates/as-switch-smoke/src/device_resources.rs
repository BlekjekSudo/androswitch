//! Actual Horizon process capabilities, queried at each Java Runtime call.
use alloc::{format, rc::Rc};
use as_platform::{PlatformError, RuntimeResources};

#[allow(unsafe_code)]
extern "C" {
    fn as_platform_available_processors(value: *mut u32) -> i32;
}
struct HorizonResources;
impl RuntimeResources for HorizonResources {
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
