//! AndroSwitch inspection facade. Inspection never claims application execution.
use as_platform::{host::HostPlatform, Storage};
use rd_apk::{axml::XmlElement, Apk};
use serde::Serialize;

pub const UPSTREAM_REVISION: &str = "6388281f66d254c8e01a0fdc7a5a9a30239bc762";
pub const RUNTIME_API: u32 = 26;
pub const MAX_INSPECT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_DEX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeError {
    pub code: String,
    pub operation: String,
    pub module_id: String,
    pub cause: String,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}:{}]: {}",
            self.code, self.module_id, self.operation, self.cause
        )
    }
}
impl std::error::Error for RuntimeError {}

impl From<rd_apk::RdError> for RuntimeError {
    fn from(error: rd_apk::RdError) -> Self {
        Self {
            code: error.code,
            operation: "inspect".into(),
            module_id: error.module_id,
            cause: error.cause,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompatibilityGap {
    pub code: &'static str,
    pub cause: String,
    pub milestone: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Preflight {
    pub runtime_api: u32,
    pub sdk_allowed: bool,
    pub native_abi_allowed: bool,
    pub single_apk: bool,
    /// Only SDK/ABI/package-form checks, not an Android API coverage claim.
    pub static_requirements_met: bool,
    pub blockers: Vec<CompatibilityGap>,
    pub install_implemented: bool,
    pub execution_implemented: bool,
    pub host_execution_verified: bool,
    pub horizon_emulator_verified: bool,
    pub device_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub file_size_bytes: u64,
    pub zip_entries: usize,
    pub dex_files: Vec<String>,
    pub native_libraries: Vec<String>,
    pub abis: Vec<String>,
    pub asset_count: usize,
    pub resource_count: usize,
    pub duplicate_entries: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Signing {
    pub detected_schemes: Vec<&'static str>,
    pub cryptographically_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct InspectReport {
    pub schema_version: u32,
    pub status: &'static str,
    pub operation: &'static str,
    pub project: &'static str,
    pub upstream_revision: &'static str,
    pub manifest: rd_apk::Manifest,
    pub launcher_activities: Vec<String>,
    pub inventory: Inventory,
    pub signing: Signing,
    pub preflight: Preflight,
}

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub schema_version: u32,
    pub project: &'static str,
    pub version: &'static str,
    pub upstream_revision: &'static str,
    pub runtime_api: u32,
    pub native_abis: [&'static str; 1],
    pub offline: bool,
    pub apk_inspection: bool,
    pub install: bool,
    pub dex_execution: bool,
    pub native_execution: bool,
    pub horizon_emulator_verified: bool,
    pub device_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct DexReport {
    pub schema_version: u32,
    pub status: &'static str,
    pub operation: &'static str,
    pub version: u32,
    pub file_size: u32,
    pub classes: u32,
    pub methods: u32,
    pub strings: u32,
    pub checksum_ok: u32,
    pub bytecode_executed: bool,
}

pub struct Runtime<P = HostPlatform> {
    platform: P,
}

impl Default for Runtime<HostPlatform> {
    fn default() -> Self {
        Self::new(HostPlatform::default())
    }
}

impl<P: Storage> Runtime<P> {
    pub fn new(platform: P) -> Self {
        Self { platform }
    }

    pub fn inspect(&self, path: &str) -> Result<InspectReport, RuntimeError> {
        let bytes = self
            .platform
            .read_bounded(path, MAX_INSPECT_BYTES)
            .map_err(|error| RuntimeError {
                code: error.code.into(),
                operation: "inspect".into(),
                module_id: "as-platform".into(),
                cause: error.cause,
            })?;
        Self::inspect_bytes(bytes)
    }

    pub fn inspect_dex(&self, path: &str) -> Result<DexReport, RuntimeError> {
        let bytes = self
            .platform
            .read_bounded(path, MAX_DEX_BYTES)
            .map_err(|error| RuntimeError {
                code: error.code.into(),
                operation: "inspect_dex".into(),
                module_id: "as-platform".into(),
                cause: error.cause,
            })?;
        let dex = rd_dex::Dex::parse(bytes).map_err(|error| RuntimeError {
            code: error.code,
            operation: "inspect_dex".into(),
            module_id: error.module_id,
            cause: error.cause,
        })?;
        Ok(DexReport {
            schema_version: 1,
            status: "ok",
            operation: "inspect_dex",
            version: dex
                .header
                .version
                .as_str()
                .parse()
                .map_err(|_| RuntimeError {
                    code: "PARSE_ERROR".into(),
                    operation: "inspect_dex".into(),
                    module_id: "rd-dex".into(),
                    cause: "invalid DEX version".into(),
                })?,
            file_size: dex.header.file_size,
            classes: dex.header.class_defs_size,
            methods: dex.header.method_ids_size,
            strings: dex.header.string_ids_size,
            checksum_ok: u32::from(dex.verify_checksum()),
            bytecode_executed: false,
        })
    }

    pub fn inspect_bytes(bytes: Vec<u8>) -> Result<InspectReport, RuntimeError> {
        if bytes.len() > MAX_INSPECT_BYTES {
            return Err(RuntimeError {
                code: "INPUT_TOO_LARGE".into(),
                operation: "inspect".into(),
                module_id: "as-runtime".into(),
                cause: "inspection input exceeds 512 MiB".into(),
            });
        }
        let apk = Apk::from_bytes(bytes)?;
        let raw = apk.manifest_raw()?;
        let single_apk = apk.manifest.split.is_none() && !requires_splits(&raw.root);
        let sdk_allowed = apk.manifest.min_sdk <= RUNTIME_API;
        let native_abi_allowed =
            apk.native_libs.is_empty() || apk.abis.iter().any(|abi| abi == "arm64-v8a");
        let mut blockers = Vec::new();
        if !sdk_allowed {
            blockers.push(CompatibilityGap {
                code: "SDK_TOO_NEW",
                milestone: "M11",
                cause: format!(
                    "minSdk {} requires an API newer than runtime API {RUNTIME_API}",
                    apk.manifest.min_sdk
                ),
            });
        }
        if !native_abi_allowed {
            blockers.push(CompatibilityGap {
                code: "UNSUPPORTED_NATIVE_ABI",
                milestone: "M11",
                cause: format!(
                    "ARM64 libraries are required; APK provides {}",
                    apk.abis.join(", ")
                ),
            });
        }
        if !single_apk {
            blockers.push(CompatibilityGap {
                code: "SPLIT_APK_REQUIRED",
                milestone: "M10",
                cause: "this package is a split or declares required splits".into(),
            });
        }
        let application_enabled = raw
            .root
            .children
            .iter()
            .find(|node| node.name == "application")
            .and_then(|node| node.android_attr("enabled"))
            .and_then(|attr| attr.as_bool())
            != Some(false);
        let launcher_activities = apk
            .launcher_activities()
            .iter()
            .filter(|component| component.enabled != Some(false) && application_enabled)
            .map(|component| component.class_name.clone())
            .collect();
        let signing = Signing {
            detected_schemes: apk.signing.schemes(),
            cryptographically_verified: false,
        };
        let inventory = Inventory {
            file_size_bytes: apk.file_size_bytes,
            zip_entries: apk.zip_entry_count,
            dex_files: apk.dex_files,
            native_libraries: apk.native_libs,
            abis: apk.abis,
            asset_count: apk.assets.len(),
            resource_count: apk.res_count,
            duplicate_entries: apk.zip_duplicates,
        };
        Ok(InspectReport {
            schema_version: 1,
            status: "ok",
            operation: "inspect",
            project: "AndroSwitch",
            upstream_revision: UPSTREAM_REVISION,
            manifest: apk.manifest,
            launcher_activities,
            inventory,
            signing,
            preflight: Preflight {
                runtime_api: RUNTIME_API,
                sdk_allowed,
                native_abi_allowed,
                single_apk,
                static_requirements_met: blockers.is_empty(),
                blockers,
                install_implemented: false,
                execution_implemented: false,
                host_execution_verified: false,
                horizon_emulator_verified: false,
                device_verified: false,
            },
        })
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            schema_version: 1,
            project: "AndroSwitch",
            version: env!("CARGO_PKG_VERSION"),
            upstream_revision: UPSTREAM_REVISION,
            runtime_api: RUNTIME_API,
            native_abis: ["arm64-v8a"],
            offline: true,
            apk_inspection: true,
            install: false,
            dex_execution: false,
            native_execution: false,
            horizon_emulator_verified: false,
            device_verified: false,
        }
    }
}

fn requires_splits(root: &XmlElement) -> bool {
    if root
        .android_attr("isSplitRequired")
        .and_then(|a| a.as_bool())
        == Some(true)
    {
        return true;
    }
    for child in &root.children {
        if child.name == "uses-split" {
            return true;
        }
        if child.name == "application" {
            if child
                .android_attr("isSplitRequired")
                .and_then(|a| a.as_bool())
                == Some(true)
            {
                return true;
            }
            for metadata in &child.children {
                if metadata.name == "meta-data"
                    && metadata
                        .android_attr("name")
                        .is_some_and(|a| a.text() == "com.android.vending.splits.required")
                    && metadata.android_attr("value").and_then(|a| a.as_bool()) == Some(true)
                {
                    return true;
                }
            }
        }
    }
    false
}
