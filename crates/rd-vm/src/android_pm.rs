//! `android.content.pm.PackageManager`, backed by the installed APK's manifest.
//!
//! - `Context.getPackageManager()` returns an `ApplicationPackageManager`.
//! - **Own package.** `getPackageInfo` and `getApplicationInfo` return
//!   `PackageInfo`/`ApplicationInfo` objects whose public fields
//!   (`packageName`, `versionName`, `versionCode`, `applicationInfo`,
//!   `targetSdkVersion`, `minSdkVersion`, `dataDir`, `sourceDir`, `uid`, …)
//!   come from the manifest. `getLongVersionCode` is also served.
//!   `getActivityInfo`, `getServiceInfo` and `getProviderInfo` return
//!   component infos for classes present in the APK.
//! - **Other packages** throw `PackageManager$NameNotFoundException`, as on a
//!   device without them installed.
//! - `hasSystemFeature`: reports a fixed phone profile (touchscreen,
//!   camera, wifi, portrait screen, …). TV, watch, automotive and PC
//!   features are false.
//! - **Intent queries.** `queryIntent*` return an empty list;
//!   `resolveActivity` and `resolveContentProvider` return null. No other
//!   apps are installed.
//! - `getApplicationLabel`, `getPackagesForUid` and `isInstantApp` are
//!   served; `getDefaultActivityIcon` and `getDrawable` return null.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const PM: &str = "Landroid/content/pm/PackageManager;";
pub(crate) const APM: &str = "Landroid/app/ApplicationPackageManager;";
const PACKAGE_INFO: &str = "Landroid/content/pm/PackageInfo;";
const APP_INFO: &str = "Landroid/content/pm/ApplicationInfo;";
const APP_UID: i32 = 10_123;

/// Features of the emulated device (a phone with a camera).
const FEATURES: &[&str] = &[
    "android.hardware.touchscreen",
    "android.hardware.touchscreen.multitouch",
    "android.hardware.faketouch",
    "android.hardware.camera",
    "android.hardware.camera.any",
    "android.hardware.camera.autofocus",
    "android.hardware.camera.front",
    "android.hardware.screen.portrait",
    "android.hardware.screen.landscape",
    "android.hardware.wifi",
    "android.hardware.audio.output",
    "android.hardware.microphone",
    "android.hardware.sensor.accelerometer",
    "android.hardware.vulkan.version",
    "android.software.app_widgets",
    "android.software.home_screen",
    "android.software.input_methods",
    "android.software.webview",
];

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg.into()))
}

fn jstr(vm: &mut Engine, s: &str) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(
        vm,
        s.to_string(),
    )?))
}

fn sval(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

struct AppMeta {
    package: String,
    version_name: Option<String>,
    version_code: u64,
    min_sdk: u32,
    target_sdk: u32,
    label: String,
}

fn meta(vm: &Engine) -> AppMeta {
    match vm.fw.resources.as_ref() {
        Some(apk) => {
            let m = &apk.manifest;
            AppMeta {
                package: m.package.clone(),
                version_name: m.version_name.clone(),
                version_code: m.version_code.unwrap_or(1),
                min_sdk: m.min_sdk,
                target_sdk: m.target_sdk,
                label: m
                    .application
                    .label
                    .clone()
                    .unwrap_or_else(|| m.package.clone()),
            }
        }
        None => AppMeta {
            package: "app".into(),
            version_name: None,
            version_code: 1,
            min_sdk: 1,
            target_sdk: 1,
            label: "app".into(),
        },
    }
}

fn alloc(vm: &mut Engine, class: &str, fields: Vec<(&str, Value)>) -> Result<ObjRef, VmExit> {
    let fields = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    Ok(vm.heap.alloc_instance(class.to_string(), fields)?)
}

fn not_found(pkg: &str) -> VmExit {
    exc(
        "Landroid/content/pm/PackageManager$NameNotFoundException;",
        pkg,
    )
}

fn app_info(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let m = meta(vm);
    let data_dir = format!("/data/user/0/{}", m.package);
    let source_dir = format!("/data/app/{}/base.apk", m.package);
    let fields = vec![
        ("packageName", jstr(vm, &m.package)?),
        ("processName", jstr(vm, &m.package)?),
        ("className", Value::Null),
        ("dataDir", jstr(vm, &data_dir)?),
        ("sourceDir", jstr(vm, &source_dir)?),
        ("publicSourceDir", jstr(vm, &source_dir)?),
        (
            "nativeLibraryDir",
            jstr(vm, &format!("/data/app/{}/lib", m.package))?,
        ),
        ("uid", Value::Int(APP_UID)),
        ("minSdkVersion", Value::Int(m.min_sdk as i32)),
        ("targetSdkVersion", Value::Int(m.target_sdk as i32)),
        ("enabled", Value::Int(1)),
        ("flags", Value::Int(0x0000_0004 | 0x0000_0080 | 0x0040_0000)), // HAS_CODE | INSTALLED… | SUPPORTS_RTL-ish
        ("metaData", Value::Null),
        ("nonLocalizedLabel", jstr(vm, &m.label)?),
        ("labelRes", Value::Int(0)),
        ("icon", Value::Int(0)),
        ("theme", Value::Int(0)),
        ("category", Value::Int(-1)),
    ];
    alloc(vm, APP_INFO, fields)
}

fn package_info(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let m = meta(vm);
    let ai = app_info(vm)?;
    let vn = match &m.version_name {
        Some(v) => jstr(vm, v)?,
        None => Value::Null,
    };
    let fields = vec![
        ("packageName", jstr(vm, &m.package)?),
        ("versionName", vn),
        ("versionCode", Value::Int(m.version_code as u32 as i32)),
        ("rd_long_version", Value::Long(m.version_code as i64)),
        ("applicationInfo", Value::Obj(ai)),
        ("firstInstallTime", Value::Long(1_700_000_000_000)),
        ("lastUpdateTime", Value::Long(1_700_000_000_000)),
        ("activities", Value::Null),
        ("services", Value::Null),
        ("providers", Value::Null),
        ("receivers", Value::Null),
        ("permissions", Value::Null),
        ("requestedPermissions", Value::Null),
        ("signatures", Value::Null),
        ("signingInfo", Value::Null),
    ];
    alloc(vm, PACKAGE_INFO, fields)
}

fn component_info(
    vm: &mut Engine,
    info_class: &str,
    comp: Option<&Value>,
) -> Result<Value, VmExit> {
    let m = meta(vm);
    let (pkg, cls) = match comp {
        Some(Value::Obj(c)) => {
            let get = |f: &str| match vm.heap.get_field(*c, f) {
                Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
                _ => None,
            };
            (
                get("rd_pkg").unwrap_or_default(),
                get("rd_cls").unwrap_or_default(),
            )
        }
        _ => return Err(exc("Ljava/lang/NullPointerException;", "component == null")),
    };
    let desc = format!("L{};", cls.replace('.', "/"));
    if pkg != m.package || vm.cp.find_class(&desc).is_none() {
        return Err(not_found(&format!("{pkg}/{cls}")));
    }
    let ai = app_info(vm)?;
    let fields = vec![
        ("name", jstr(vm, &cls)?),
        ("packageName", jstr(vm, &pkg)?),
        ("processName", jstr(vm, &pkg)?),
        ("applicationInfo", Value::Obj(ai)),
        ("enabled", Value::Int(1)),
        ("exported", Value::Int(0)),
        ("metaData", Value::Null),
        ("authority", Value::Null),
        ("permission", Value::Null),
        ("theme", Value::Int(0)),
        ("labelRes", Value::Int(0)),
        ("screenOrientation", Value::Int(-1)),
        ("launchMode", Value::Int(0)),
        ("flags", Value::Int(0)),
        ("configChanges", Value::Int(0)),
        ("softInputMode", Value::Int(0)),
    ];
    Ok(Value::Obj(alloc(vm, info_class, fields)?))
}

/// `Context.getPackageManager()` (any Context receiver).
pub fn package_manager(vm: &mut Engine) -> Result<Value, VmExit> {
    Ok(Value::Obj(alloc(vm, APM, Vec::new())?))
}

/// `Context.getApplicationInfo()` — the SHORT route (AOSP: same
/// ApplicationInfo the PackageManager would hand back for the own package;
/// LadderActivity and friends call it directly on the Context, without going
/// through getPackageManager()).
pub(crate) fn context_app_info(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    app_info(vm)
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "getPackageManager" {
        if mref_class.starts_with("Landroid/") && recv_class != APM {
            return Ok(Some(package_manager(vm)?));
        }
        return Ok(None);
    }
    if recv_class == PACKAGE_INFO && name == "getLongVersionCode" {
        return Ok(Some(match vm.heap.get_field(recv, "rd_long_version") {
            Ok(v @ Value::Long(_)) => v,
            _ => Value::Long(0),
        }));
    }
    if recv_class != APM && mref_class != PM && mref_class != APM {
        return Ok(None);
    }
    let own = |vm: &Engine, v: Option<&Value>| -> Result<(), VmExit> {
        let p = sval(vm, v)
            .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "packageName == null"))?;
        if p == meta(vm).package {
            Ok(())
        } else {
            Err(not_found(&p))
        }
    };
    Ok(Some(match name {
        "hasSystemFeature" => {
            let f = sval(vm, args.first()).unwrap_or_default();
            Value::Int(FEATURES.contains(&f.as_str()) as i32)
        }
        "getPackageInfo" => {
            own(vm, args.first())?;
            Value::Obj(package_info(vm)?)
        }
        "getApplicationInfo" => {
            own(vm, args.first())?;
            Value::Obj(app_info(vm)?)
        }
        "getApplicationLabel" => {
            let l = meta(vm).label;
            jstr(vm, &l)?
        }
        "getActivityInfo" => component_info(vm, "Landroid/content/pm/ActivityInfo;", args.first())?,
        "getServiceInfo" => component_info(vm, "Landroid/content/pm/ServiceInfo;", args.first())?,
        "getProviderInfo" => component_info(vm, "Landroid/content/pm/ProviderInfo;", args.first())?,
        "getReceiverInfo" => component_info(vm, "Landroid/content/pm/ActivityInfo;", args.first())?,
        "queryIntentActivities"
        | "queryIntentServices"
        | "queryBroadcastReceivers"
        | "queryIntentContentProviders"
        | "queryIntentActivityOptions"
        | "getInstalledPackages"
        | "getInstalledApplications" => {
            let r = alloc(vm, "Ljava/util/ArrayList;", Vec::new())?;
            vm.fw
                .objects
                .insert(r, crate::framework::HostObj::List { elems: Vec::new() });
            Value::Obj(r)
        }
        "resolveActivity"
        | "resolveService"
        | "resolveContentProvider"
        | "getDefaultActivityIcon"
        | "getDrawable"
        | "getApplicationIcon"
        | "getLaunchIntentForPackage" => Value::Null,
        "getPackagesForUid" => {
            let p = meta(vm).package;
            let s = jstr(vm, &p)?;
            let a = vm
                .heap
                .alloc_array(crate::heap::ElemKind::Obj, 1, "Ljava/lang/String;")?;
            *vm.heap.array_elems_mut(a)? = vec![s];
            Value::Obj(a)
        }
        "getNameForUid" => {
            let p = meta(vm).package;
            jstr(vm, &p)?
        }
        "isInstantApp" | "isSafeMode" => Value::Int(0),
        "checkPermission" => Value::Int(0), // PERMISSION_GRANTED; runtime grants are modelled elsewhere
        "getInstallerPackageName" => Value::Null,
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::FEATURES;
    #[test]
    fn phone_profile_excludes_tv() {
        assert!(FEATURES.contains(&"android.hardware.camera.any"));
        assert!(!FEATURES.contains(&"android.software.leanback"));
    }
}
