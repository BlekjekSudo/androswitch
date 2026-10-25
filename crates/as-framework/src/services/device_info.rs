//! Declared Android guest profile, kept separate from real Horizon queries.
use super::*;
#[derive(Default)]
pub(super) struct DeviceInfo {
    fields: RefCell<BTreeMap<(String, String, String), Value>>,
}
impl DeviceInfo {
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.fields
            .borrow()
            .values()
            .filter_map(|v| v.as_ref().ok().flatten())
            .collect()
    }
    pub(super) fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        ty: &str,
    ) -> Result<Option<Value>> {
        let key = (class.to_string(), name.to_string(), ty.to_string());
        if let Some(value) = self.fields.borrow().get(&key) {
            return Ok(Some(value.clone()));
        }
        let property = match (class, name, ty) {
            ("Landroid/os/Build;", "MODEL", "Ljava/lang/String;") => "ro.product.model",
            ("Landroid/os/Build;", "MANUFACTURER", "Ljava/lang/String;") => {
                "ro.product.manufacturer"
            }
            ("Landroid/os/Build;", "BRAND", "Ljava/lang/String;") => "ro.product.brand",
            ("Landroid/os/Build;", "PRODUCT", "Ljava/lang/String;") => "ro.product.name",
            ("Landroid/os/Build;", "DEVICE", "Ljava/lang/String;") => "ro.product.device",
            ("Landroid/os/Build;", "BOARD", "Ljava/lang/String;") => "ro.product.board",
            ("Landroid/os/Build;", "HARDWARE", "Ljava/lang/String;") => "ro.hardware",
            ("Landroid/os/Build;", "ID", "Ljava/lang/String;") => "ro.build.id",
            ("Landroid/os/Build;", "DISPLAY", "Ljava/lang/String;") => "ro.build.display.id",
            ("Landroid/os/Build;", "FINGERPRINT", "Ljava/lang/String;") => "ro.build.fingerprint",
            ("Landroid/os/Build;", "TYPE", "Ljava/lang/String;") => "ro.build.type",
            ("Landroid/os/Build;", "TAGS", "Ljava/lang/String;") => "ro.build.tags",
            ("Landroid/os/Build;", "USER", "Ljava/lang/String;") => "ro.build.user",
            ("Landroid/os/Build;", "HOST", "Ljava/lang/String;") => "ro.build.host",
            ("Landroid/os/Build$VERSION;", "RELEASE", "Ljava/lang/String;") => {
                "ro.build.version.release"
            }
            ("Landroid/os/Build$VERSION;", "INCREMENTAL", "Ljava/lang/String;") => {
                "ro.build.version.incremental"
            }
            ("Landroid/os/Build$VERSION;", "CODENAME", "Ljava/lang/String;") => {
                "ro.build.version.codename"
            }
            ("Landroid/os/Build$VERSION;", "BASE_OS", "Ljava/lang/String;") => {
                "ro.build.version.base_os"
            }
            ("Landroid/os/Build$VERSION;", "SECURITY_PATCH", "Ljava/lang/String;") => {
                "ro.build.version.security_patch"
            }
            ("Landroid/os/Build$VERSION;", "SDK", "Ljava/lang/String;") => "ro.build.version.sdk",
            ("Landroid/os/Build$VERSION;", "PREVIEW_SDK_INT", "I") => {
                "ro.build.version.preview_sdk"
            }
            ("Landroid/os/Build;", "TIME", "J") => "ro.build.date.utc",
            (
                "Landroid/os/Build;",
                "SUPPORTED_ABIS" | "SUPPORTED_64_BIT_ABIS" | "SUPPORTED_32_BIT_ABIS",
                "[Ljava/lang/String;",
            ) => {
                let values = if name == "SUPPORTED_32_BIT_ABIS" {
                    vec![]
                } else {
                    vec![string(vm, "arm64-v8a")?]
                };
                let array = vm.heap.alloc_array_zeroed(
                    rd_vm::heap::ElemKind::Obj,
                    values.len(),
                    "[Ljava/lang/String;",
                )?;
                vm.heap.array_elems_mut(array)?.clone_from_slice(&values);
                let value = Value::Obj(array);
                self.fields.borrow_mut().insert(key, value.clone());
                return Ok(Some(value));
            }
            _ => return Ok(None),
        };
        let raw = crate::properties::get(property).ok_or_else(|| {
            error(
                "RUNTIME_CONFIGURATION",
                format!("missing declared profile key {property}"),
            )
        })?;
        let value = match ty {
            "I" => Value::Int(
                raw.parse()
                    .map_err(|_| error("RUNTIME_CONFIGURATION", property))?,
            ),
            "J" => Value::Long(
                raw.parse::<i64>()
                    .map_err(|_| error("RUNTIME_CONFIGURATION", property))?
                    .checked_mul(1000)
                    .ok_or_else(|| error("RUNTIME_CONFIGURATION", property))?,
            ),
            _ => string(vm, raw)?,
        };
        self.fields.borrow_mut().insert(key, value.clone());
        Ok(Some(value))
    }
}
