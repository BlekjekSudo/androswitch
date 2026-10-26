//! `android.os.Build` / `Build.VERSION` static fields.
//!
//! Without these, `Build.VERSION.SDK_INT` read as 0 (the unmodelled-static
//! default). Every `if (SDK_INT >= N)` in AndroidX `*Compat` then took the
//! pre-Oreo branch, often reflection hacks (`HandlerCompat.createAsync` →
//! `Handler.class.getDeclaredConstructor(...)`), which a headless runtime
//! cannot and should not emulate.
//!
//! The emulated platform defaults to API 34 (Android 14), the level whose
//! behaviour the runtime models. `RD_SDK_INT` overrides it, which is useful
//! to exercise an app's legacy branches deliberately. Device identity
//! matches the virtual device profile (moto-e5 class), and `SUPPORTED_ABIS`
//! lists the ABIs the NDK emulation executes.

use crate::engine::Engine;
use crate::err::VmExit;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

pub(crate) const BUILD: &str = "Landroid/os/Build;";
pub(crate) const VERSION: &str = "Landroid/os/Build$VERSION;";
pub const DEFAULT_SDK_INT: i32 = 34;

/// Emulated `Build.VERSION.SDK_INT` (env `RD_SDK_INT`, default 34).
pub fn sdk_int() -> i32 {
    crate::port_env::var("RD_SDK_INT")
        .ok()
        .and_then(|v| v.trim().parse::<i32>().ok())
        .filter(|v| (21..=40).contains(v))
        .unwrap_or(DEFAULT_SDK_INT)
}

fn release_for(sdk: i32) -> &'static str {
    match sdk {
        ..=25 => "7.1",
        26 => "8.0.0",
        27 => "8.1.0",
        28 => "9",
        29 => "10",
        30 => "11",
        31 => "12",
        32 => "12L",
        33 => "13",
        34 => "14",
        35 => "15",
        _ => "16",
    }
}

fn s(vm: &mut Engine, v: &str) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(
        vm,
        v.to_string(),
    )?))
}

pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    let sdk = sdk_int();
    if owner == VERSION {
        return Ok(Some(match field {
            "SDK_INT" => Value::Int(sdk),
            "PREVIEW_SDK_INT" => Value::Int(0),
            "RELEASE" | "RELEASE_OR_CODENAME" | "RELEASE_OR_PREVIEW_DISPLAY" => {
                s(vm, release_for(sdk))?
            }
            "CODENAME" => s(vm, "REL")?,
            "INCREMENTAL" => s(vm, "rustydroid")?,
            "SECURITY_PATCH" => s(vm, "2026-01-01")?,
            "BASE_OS" => s(vm, "")?,
            "SDK" => s(vm, &sdk.to_string())?,
            "MEDIA_PERFORMANCE_CLASS" => Value::Int(0),
            _ => return Ok(None),
        }));
    }
    if owner == BUILD {
        return Ok(Some(match field {
            "MANUFACTURER" => s(vm, "RustyDroid")?,
            "BRAND" => s(vm, "rustydroid")?,
            "MODEL" => s(vm, "RustyDroid Virtual (moto-e5 class)")?,
            "DEVICE" | "PRODUCT" | "BOARD" | "HARDWARE" => s(vm, "rustydroid")?,
            "ID" | "DISPLAY" => s(vm, "RD1")?,
            "HOST" => s(vm, "localhost")?,
            "USER" => s(vm, "rustydroid")?,
            "TAGS" => s(vm, "release-keys")?,
            "TYPE" => s(vm, "user")?,
            "BOOTLOADER" | "RADIO" | "SERIAL" => s(vm, "unknown")?,
            "FINGERPRINT" => {
                let f = format!(
                    "rustydroid/rustydroid/rustydroid:{}/RD1/1:user/release-keys",
                    release_for(sdk)
                );
                s(vm, &f)?
            }
            "TIME" => Value::Long(1_767_225_600_000),
            "SOC_MANUFACTURER" | "SOC_MODEL" | "SKU" | "ODM_SKU" => s(vm, "unknown")?,
            "SUPPORTED_ABIS" | "SUPPORTED_64_BIT_ABIS" | "SUPPORTED_32_BIT_ABIS" => {
                let abis: &[&str] = match field {
                    "SUPPORTED_64_BIT_ABIS" => &["x86_64", "arm64-v8a"],
                    "SUPPORTED_32_BIT_ABIS" => &["x86", "armeabi-v7a"],
                    _ => &["x86_64", "arm64-v8a", "x86", "armeabi-v7a"],
                };
                let mut elems = Vec::new();
                for a in abis {
                    elems.push(s(vm, a)?);
                }
                let arr = vm.heap.alloc_array(
                    crate::heap::ElemKind::Obj,
                    elems.len(),
                    "Ljava/lang/String;",
                )?;
                let slot = vm
                    .heap
                    .array_elems_mut(arr)
                    .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
                slot.clear();
                slot.extend(elems);
                Value::Obj(arr)
            }
            "CPU_ABI" => s(vm, "x86_64")?,
            "CPU_ABI2" => s(vm, "")?,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;

    #[test]
    fn release_names() {
        assert_eq!(release_for(34), "14");
        assert_eq!(release_for(26), "8.0.0");
        assert_eq!(DEFAULT_SDK_INT, 34);
    }
}
