//! Installed metadata needs the ZIP index and manifest, never DEX/resources or
//! the complete APK container. Registry publication remains the identity source.
use super::*;
use alloc::sync::Arc;
use as_platform::fs::{FileSystem, ReadOnlyFile};
use rd_apk::{
    axml::AxmlDocument,
    manifest::Manifest,
    zip::{ReadAt, Zip, ZipLimits},
};

struct Source(Arc<dyn ReadOnlyFile>);
impl core::fmt::Debug for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PackageManifestSource")
            .field("bytes", &self.0.len())
            .finish()
    }
}
impl ReadAt for Source {
    fn len(&self) -> u64 {
        self.0.len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> rd_apk::RdResult<usize> {
        self.0
            .read_at(offset, output)
            .map_err(|e| rd_apk::RdError::parse(e.to_string()))
    }
}
pub(super) struct Metadata {
    pub manifest: Manifest,
    pub version_name: Option<String>,
}
pub(super) fn path(record: &as_package::Record) -> String {
    format!(
        "installed/{}/{:020}/package.apk",
        record.package, record.revision
    )
}
pub(super) fn read(fs: &dyn FileSystem, record: &as_package::Record) -> Result<Metadata> {
    let path = path(record);
    let reader = fs.open_read(&path).map_err(storage_error)?;
    if reader.len() != record.apk_bytes {
        return Err(error(
            "INSTALLATION_CORRUPT",
            "package metadata APK size differs from committed record",
        ));
    }
    let zip = Zip::from_source(
        Arc::new(Source(reader)),
        ZipLimits {
            max_directory_bytes: 4 << 20,
            max_entries: 32_768,
            max_entry_bytes: 4 << 20,
            ..ZipLimits::default()
        },
    )
    .map_err(|e| error(&e.code, e.cause))?;
    if zip
        .duplicates()
        .iter()
        .any(|name| name == "AndroidManifest.xml")
    {
        return Err(error("INSTALLATION_CORRUPT", "ambiguous package manifest"));
    }
    let bytes = zip
        .read("AndroidManifest.xml")
        .map_err(|e| error(&e.code, e.cause))?;
    let document = rd_apk::axml::parse(&bytes).map_err(|e| error(&e.code, e.cause))?;
    let metadata = parse(&document)?;
    if metadata.manifest.package != record.package
        || metadata.manifest.version_code != record.version_code
        || metadata.manifest.version_name != record.version_name
    {
        return Err(error(
            "INSTALLATION_CORRUPT",
            "package manifest identity/version differs from committed record",
        ));
    }
    Ok(metadata)
}
pub(super) fn parse(document: &AxmlDocument) -> Result<Metadata> {
    let root = &document.root;
    if let Some(value) = root.android_attr("versionCode") {
        if value.as_u32().is_none() {
            return Err(error(
                "API_UNAVAILABLE",
                "resource-valued/noninteger manifest versionCode",
            ));
        }
    }
    let version_name = match root.android_attr("versionName").map(|a| &a.value) {
        None | Some(AttrValue::Null) => None,
        Some(AttrValue::String(value)) => Some(value.clone()),
        _ => return Err(error("API_UNAVAILABLE", "nonliteral manifest versionName")),
    };
    Ok(Metadata {
        manifest: application_manifest(document)?,
        version_name,
    })
}
pub(super) fn application_manifest(document: &AxmlDocument) -> Result<Manifest> {
    let root = &document.root;
    if let Some(sdk) = root.children_named("uses-sdk").next() {
        for field in ["minSdkVersion", "targetSdkVersion"] {
            if sdk
                .android_attr(field)
                .is_some_and(|a| a.as_u32().is_none())
            {
                return Err(error(
                    "API_UNAVAILABLE",
                    format!("noninteger uses-sdk {field}"),
                ));
            }
        }
    }
    let application = root
        .children_named("application")
        .next()
        .ok_or_else(|| error("INVALID_APK", "package has no application manifest element"))?;
    if application
        .android_attr("name")
        .is_some_and(|a| !matches!(a.value, AttrValue::String(_) | AttrValue::Null))
    {
        return Err(error(
            "API_UNAVAILABLE",
            "nonliteral application class name",
        ));
    }
    let mut manifest =
        rd_apk::manifest::build(document, None).map_err(|e| error(&e.code, e.cause))?;
    if application
        .android_attr("name")
        .is_none_or(|a| matches!(a.value, AttrValue::Null))
    {
        manifest.application.name = None;
    }
    Ok(manifest)
}
