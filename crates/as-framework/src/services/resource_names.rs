//! Resource identity comes from the owning ARSC tables, independently of value
//! aliases, selected configuration, or the APK manifest's application ID.
use super::*;
const RESOURCES: &str = "Landroid/content/res/Resources;";
// API26 public.xml (android-8.0.0_r4): the standard content-view identifier.
// This explicit metadata registry does not provide Android framework assets.
// https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/97e5130234bcdc1c4a705be4e548302934499bd4/core/res/res/values/public.xml
fn system_name(id: u32) -> Option<rd_apk::arsc::ResourceName<'static>> {
    match id {
        0x0102_0002 => Some(rd_apk::arsc::ResourceName {
            package: "android",
            resource_type: "id",
            entry: "content",
        }),
        _ => None,
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn call(
    android: &Android,
    vm: &mut Engine,
    _class: &str,
    name: &str,
    sig: &str,
    r: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    if name == "getIdentifier"
        && sig == "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I"
        && vm.cp.is_subtype(vm.heap.class_of(r)?, RESOURCES)
    {
        return identifier(android, vm, args).map(Some);
    }
    if sig != "(I)Ljava/lang/String;"
        || !matches!(
            name,
            "getResourcePackageName"
                | "getResourceTypeName"
                | "getResourceEntryName"
                | "getResourceName"
        )
        || !vm.cp.is_subtype(vm.heap.class_of(r)?, RESOURCES)
    {
        return Ok(None);
    }
    let id = int(args, 0)? as u32;
    let identity = if id >> 24 == 1 && (id >> 16) & 255 != 0 {
        Some(system_name(id).ok_or_else(|| {
            error(
                "API_UNAVAILABLE",
                format!("Android framework ARSC name metadata for #0x{id:08x} is unavailable"),
            )
        })?)
    } else {
        android.apk.arsc.as_ref().and_then(|a| a.resource_name(id))
    }
    .ok_or_else(|| {
        Throwable::new(
            "Landroid/content/res/Resources$NotFoundException;",
            format!("Resource ID #0x{id:08x}"),
        )
    })?;
    let value = match name {
        "getResourcePackageName" => identity.package.into(),
        "getResourceTypeName" => identity.resource_type.into(),
        "getResourceEntryName" => identity.entry.into(),
        _ => format!(
            "{}:{}/{}",
            identity.package, identity.resource_type, identity.entry
        ),
    };
    Ok(Some(string(vm, value)?))
}

fn identifier(android: &Android, vm: &Engine, args: &[Value]) -> Result<Value> {
    let name = args[0]
        .as_ref()?
        .ok_or_else(|| Throwable::new("Ljava/lang/NullPointerException;", "name is null"))?;
    let name = vm.heap.as_str(name)?;
    if name.len() > 4096 {
        return Err(error(
            "RESOURCE_NAME_BUDGET",
            "4096 bytes per resource name",
        ));
    }
    if let Some(id) = rd_apk::arsc::identifier_number(name) {
        return Ok(Value::Int(id));
    }
    if name.starts_with('^') {
        return Err(error("API_UNAVAILABLE", "internal ResTable identifiers"));
    }
    let default_type = args[1].as_ref()?.map(|r| vm.heap.as_str(r)).transpose()?;
    let default_package = args[2].as_ref()?.map(|r| vm.heap.as_str(r)).transpose()?;
    if default_type.is_some_and(|s| s.len() > 4096)
        || default_package.is_some_and(|s| s.len() > 4096)
    {
        return Err(error(
            "RESOURCE_NAME_BUDGET",
            "4096 bytes per resource default",
        ));
    }
    let Some(query) = rd_apk::arsc::identifier_name(name, default_type, default_package) else {
        return Ok(Value::Int(0));
    };
    if query.package == "android" {
        let known = system_name(0x01020002).unwrap();
        if query == known {
            return Ok(Value::Int(0x01020002));
        }
        return Err(error(
            "API_UNAVAILABLE",
            "Android framework ARSC reverse-name catalog",
        ));
    }
    let id = android
        .apk
        .arsc
        .as_ref()
        .map(|table| table.find_identifier(query))
        .transpose()
        .map_err(|e| error(&e.code, e.cause))?
        .flatten()
        .unwrap_or(0);
    Ok(Value::Int(id as i32))
}
