//! Immutable installation revisions. Publish a durable record by renaming to
//! a *new* commit marker: no rename-overwrite assumption on Horizon's SD FS.
#![no_std]
extern crate alloc;
mod archive;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
pub use archive::open_apk;
use as_platform::{
    fs::{self, FileSystem},
    vfs::{valid_package, Vfs},
    PlatformError,
};
use rd_apk::{axml::XmlElement, Apk};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_APK_BYTES: usize = 512 << 20;
pub fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    use core::fmt::Write;
    for byte in hash {
        write!(&mut text, "{byte:02x}").expect("String write");
    }
    text
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    pub schema_version: u32,
    pub package: String,
    pub revision: u64,
    pub installed: bool,
    pub version_code: Option<u64>,
    pub version_name: Option<String>,
    pub label: Option<String>,
    pub entry_activity: Option<String>,
    pub activities: Vec<String>,
    pub apk_bytes: u64,
    pub apk_sha256: String,
    pub signature_verified: bool,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    record: Record,
    sha256: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct UninstallReport {
    pub package: String,
    pub revision: u64,
    pub data_retained: bool,
    pub cleanup_errors: Vec<String>,
}
pub struct PackageManager<F> {
    pub fs: F,
}
fn error(code: &'static str, cause: impl Into<String>) -> PlatformError {
    fs::error(code, "package", cause)
}
fn json(e: impl core::fmt::Display) -> PlatformError {
    error("REGISTRY_FORMAT", e.to_string())
}
fn package(name: &str) -> Result<(), PlatformError> {
    if valid_package(name) {
        Ok(())
    } else {
        Err(error("INVALID_PACKAGE", name))
    }
}
fn revision_dir(name: &str, revision: u64) -> String {
    format!("installed/{name}/{revision:020}")
}
pub fn requires_splits(root: &XmlElement) -> bool {
    root.android_attr("isSplitRequired")
        .and_then(|a| a.as_bool())
        == Some(true)
        || root.children.iter().any(|child| {
            child.name == "uses-split"
                || child.name == "application"
                    && (child
                        .android_attr("isSplitRequired")
                        .and_then(|a| a.as_bool())
                        == Some(true)
                        || child.children.iter().any(|m| {
                            m.name == "meta-data"
                                && m.android_attr("name").is_some_and(|a| {
                                    a.text() == "com.android.vending.splits.required"
                                })
                                && m.android_attr("value").and_then(|a| a.as_bool()) == Some(true)
                        }))
        })
}
impl<F: FileSystem> PackageManager<F> {
    pub fn new(fs: F) -> Self {
        Self { fs }
    }
    pub fn initialize(&self) -> Result<(), PlatformError> {
        for p in [
            "apks",
            "Android/data",
            "Android/obb",
            "DCIM",
            "Download",
            "installed",
            "userdata",
            "cache",
            "logs",
            "runtime",
        ] {
            fs::mkdirs(&self.fs, p)?;
        }
        Ok(())
    }
    pub fn latest(&self, name: &str) -> Result<Option<Record>, PlatformError> {
        package(name)?;
        if self.fs.stat("installed")?.is_some() {
            for entry in self.fs.list("installed")? {
                if entry.directory && entry.name != name && entry.name.eq_ignore_ascii_case(name) {
                    return Err(error(
                        "PACKAGE_CASE_COLLISION",
                        format!(
                            "{name} aliases existing {} on case-insensitive SD storage",
                            entry.name
                        ),
                    ));
                }
            }
        }
        let root = format!("installed/{name}");
        if self.fs.stat(&root)?.is_none() {
            return Ok(None);
        }
        let mut latest = None;
        for e in self.fs.list(&root)? {
            if !e.directory || e.name.len() != 20 {
                continue;
            }
            let Ok(revision) = e.name.parse::<u64>() else {
                continue;
            };
            let marker = format!("{root}/{}/commit.json", e.name);
            if self.fs.stat(&marker)?.is_none() {
                continue;
            }
            let envelope: Envelope =
                serde_json::from_slice(&fs::read_all(&self.fs, &marker, 64 << 10)?)
                    .map_err(json)?;
            let bytes = serde_json::to_vec(&envelope.record).map_err(json)?;
            let r = envelope.record;
            if envelope.sha256 != digest(&bytes)
                || r.schema_version != 1
                || r.package != name
                || r.revision != revision
            {
                return Err(error("REGISTRY_CORRUPT", marker));
            }
            if latest
                .as_ref()
                .is_none_or(|old: &Record| old.revision < revision)
            {
                latest = Some(r);
            }
        }
        Ok(latest)
    }
    pub fn list(&self) -> Result<Vec<Record>, PlatformError> {
        if self.fs.stat("installed")?.is_none() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for e in self.fs.list("installed")? {
            if e.directory && valid_package(&e.name) {
                if let Some(r) = self.latest(&e.name)? {
                    if r.installed {
                        records.push(r);
                    }
                }
            }
        }
        records.sort_by(|a, b| a.package.cmp(&b.package));
        Ok(records)
    }
    fn reserve(&self, name: &str) -> Result<u64, PlatformError> {
        let root = format!("installed/{name}");
        fs::mkdirs(&self.fs, &root)?;
        let mut revision = 0u64;
        for entry in self.fs.list(&root)? {
            if entry.directory {
                if let Ok(n) = entry.name.parse::<u64>() {
                    revision = revision.max(n);
                }
            }
        }
        for _ in 0..32 {
            revision = revision
                .checked_add(1)
                .ok_or_else(|| error("REVISION_OVERFLOW", name))?;
            if self.fs.create_dir(&revision_dir(name, revision), true)? {
                return Ok(revision);
            }
        }
        Err(error("PACKAGE_BUSY", name))
    }
    fn publish(&self, record: &Record) -> Result<(), PlatformError> {
        let bytes = serde_json::to_vec(record).map_err(json)?;
        let envelope = serde_json::to_vec(&Envelope {
            record: record.clone(),
            sha256: digest(&bytes),
        })
        .map_err(json)?;
        let dir = revision_dir(&record.package, record.revision);
        fs::write_all(&self.fs, &format!("{dir}/commit.pending"), &envelope)?;
        self.fs.rename_new(
            &format!("{dir}/commit.pending"),
            &format!("{dir}/commit.json"),
        )
    }
    pub fn install(
        &self,
        bytes: Vec<u8>,
        update: bool,
        entry: Option<&str>,
    ) -> Result<Record, PlatformError> {
        if bytes.len() > MAX_APK_BYTES {
            return Err(error("INPUT_TOO_LARGE", "APK exceeds 512 MiB"));
        }
        let apk = Apk::from_bytes(bytes).map_err(|e| error("INVALID_APK", e.to_string()))?;
        let hash = digest(apk.zip.raw_bytes().map_err(json)?);
        self.install_parsed(&apk, hash, update, entry, |output| {
            fs::write_all(&self.fs, output, apk.zip.raw_bytes().map_err(json)?)
        })
    }
    /// Copy to a private staging directory before parsing/validating. The source
    /// can be removed after publication; partial imports never replace a revision.
    pub fn install_file(
        &self,
        path: &str,
        update: bool,
        entry: Option<&str>,
    ) -> Result<Record, PlatformError> {
        self.initialize()?;
        let bucket = format!("cache/imports/{}", digest(path.as_bytes()));
        fs::mkdirs(&self.fs, &bucket)?;
        let mut staging = None;
        for serial in 0..1024 {
            let candidate = format!("{bucket}/{serial}");
            if self.fs.create_dir(&candidate, true)? {
                staging = Some(candidate);
                break;
            }
        }
        let staging = staging.ok_or_else(|| error("PACKAGE_BUSY", "import staging"))?;
        let temp = format!("{staging}/package.apk");
        let result = (|| {
            let (length, hash) = archive::transfer(&self.fs, path, Some(&temp))?;
            if archive::transfer(&self.fs, &temp, None)? != (length, hash.clone()) {
                return Err(error("INSTALL_VERIFY_FAILED", &temp));
            }
            let apk = open_apk(&self.fs, &temp)?;
            self.install_parsed(&apk, hash, update, entry, |output| {
                self.fs.rename_new(&temp, output)
            })
        })();
        let _ = self.fs.remove(&staging, true);
        result
    }
    fn install_parsed(
        &self,
        apk: &Apk,
        apk_sha256: String,
        update: bool,
        entry: Option<&str>,
        copy: impl FnOnce(&str) -> Result<(), PlatformError>,
    ) -> Result<Record, PlatformError> {
        package(&apk.manifest.package)?;
        if apk.manifest.min_sdk > 26 {
            return Err(error(
                "SDK_TOO_NEW",
                format!("minSdk {} exceeds API 26", apk.manifest.min_sdk),
            ));
        }
        if apk.manifest.split.is_some() || requires_splits(&apk.manifest_raw().map_err(json)?.root)
        {
            return Err(error("SPLIT_APK_REQUIRED", "single APK installation only"));
        }
        if !apk.native_libs.is_empty() && !apk.abis.iter().any(|a| a == "arm64-v8a") {
            return Err(error(
                "UNSUPPORTED_NATIVE_ABI",
                "native ARM64 libraries required",
            ));
        }
        if !apk.zip_duplicates.is_empty() {
            return Err(error("AMBIGUOUS_APK", "duplicate ZIP entry names"));
        }
        for name in &apk.dex_files {
            let dex = rd_dex::Dex::parse(apk.zip.read(name).map_err(json)?).map_err(json)?;
            if !dex.verify_checksum() {
                return Err(error("INVALID_DEX_CHECKSUM", name));
            }
        }
        let previous = self.latest(&apk.manifest.package)?;
        if let Some(old) = previous.as_ref().filter(|r| r.installed) {
            if !update {
                return Err(error("ALREADY_INSTALLED", "use update"));
            }
            if apk.manifest.version_code < old.version_code {
                return Err(error("VERSION_DOWNGRADE", "new versionCode is older"));
            }
        }
        let raw = apk.manifest_raw().map_err(json)?;
        let enabled = raw
            .root
            .children
            .iter()
            .find(|n| n.name == "application")
            .and_then(|n| n.android_attr("enabled"))
            .and_then(|a| a.as_bool())
            != Some(false);
        let activities: Vec<String> = apk
            .launcher_activities()
            .into_iter()
            .filter(|a| enabled && a.enabled != Some(false))
            .map(|a| a.class_name.clone())
            .collect();
        if entry.is_some_and(|e| !activities.iter().any(|a| a == e)) {
            return Err(error(
                "INVALID_ENTRY_ACTIVITY",
                "entry is not an enabled MAIN/LAUNCHER component",
            ));
        }
        let entry_activity = entry
            .map(str::to_string)
            .or_else(|| activities.first().cloned());
        if let Some(old) = previous.filter(|r| r.installed) {
            if old.apk_sha256 == apk_sha256 && old.entry_activity == entry_activity {
                // Verify the installed copy before accepting a no-op. A damaged
                // or missing copy must not be reported as a healthy install.
                self.verify_record(&old)?;
                return Ok(old);
            }
        }
        self.initialize()?;
        let revision = self.reserve(&apk.manifest.package)?;
        let record = Record {
            schema_version: 1,
            package: apk.manifest.package.clone(),
            revision,
            installed: true,
            version_code: apk.manifest.version_code,
            version_name: apk.manifest.version_name.clone(),
            label: apk.manifest.application.label.clone(),
            entry_activity,
            activities,
            apk_bytes: apk.file_size_bytes,
            apk_sha256,
            signature_verified: false,
        };
        let output = format!("{}/package.apk", revision_dir(&record.package, revision));
        copy(&output)?;
        if archive::transfer(&self.fs, &output, None)?
            != (record.apk_bytes, record.apk_sha256.clone())
        {
            return Err(error("INSTALL_VERIFY_FAILED", output));
        }
        for path in [
            format!("userdata/{}/files", record.package),
            format!("userdata/{}/cache", record.package),
            format!("Android/data/{}", record.package),
            format!("Android/obb/{}", record.package),
        ] {
            fs::mkdirs(&self.fs, &path)?;
        }
        self.publish(&record)?;
        Ok(record)
    }
    fn verify_record(&self, record: &Record) -> Result<(), PlatformError> {
        let path = format!(
            "{}/package.apk",
            revision_dir(&record.package, record.revision)
        );
        if archive::transfer(&self.fs, &path, None)?
            != (record.apk_bytes, record.apk_sha256.clone())
        {
            return Err(error("INSTALLATION_CORRUPT", &record.package));
        }
        Ok(())
    }
    pub fn load_apk(
        &self,
        name: &str,
        revision: Option<u64>,
    ) -> Result<(Record, Apk), PlatformError> {
        let record = self
            .latest(name)?
            .filter(|r| r.installed)
            .ok_or_else(|| error("NOT_INSTALLED", name))?;
        if revision.is_some_and(|r| r != record.revision) {
            return Err(error("STALE_INSTALLATION", name));
        }
        self.verify_record(&record)?;
        let path = format!("{}/package.apk", revision_dir(name, record.revision));
        let apk = open_apk(&self.fs, &path)?;
        Ok((record, apk))
    }
    pub fn load(
        &self,
        name: &str,
        revision: Option<u64>,
    ) -> Result<(Record, Vec<u8>), PlatformError> {
        let r = self
            .latest(name)?
            .filter(|r| r.installed)
            .ok_or_else(|| error("NOT_INSTALLED", name))?;
        if revision.is_some_and(|v| v != r.revision) {
            return Err(error(
                "STALE_INSTALLATION",
                "requested revision is no longer active",
            ));
        }
        let bytes = fs::read_all(
            &self.fs,
            &format!("{}/package.apk", revision_dir(name, r.revision)),
            MAX_APK_BYTES,
        )?;
        if bytes.len() as u64 != r.apk_bytes || digest(&bytes) != r.apk_sha256 {
            return Err(error("INSTALLATION_CORRUPT", name));
        }
        Ok((r, bytes))
    }
    pub fn uninstall(&self, name: &str) -> Result<UninstallReport, PlatformError> {
        let mut r = self
            .latest(name)?
            .filter(|r| r.installed)
            .ok_or_else(|| error("NOT_INSTALLED", name))?;
        r.revision = self.reserve(name)?;
        r.installed = false;
        self.publish(&r)?;
        let mut cleanup_errors = Vec::new();
        for e in self.fs.list(&format!("installed/{name}"))? {
            if e.directory && e.name.len() == 20 && e.name.parse::<u64>().is_ok() {
                let path = format!("installed/{name}/{}/package.apk", e.name);
                if self.fs.stat(&path)?.is_some() {
                    if let Err(error) = self.fs.remove(&path, false) {
                        cleanup_errors.push(error.to_string());
                    }
                }
            }
        }
        Ok(UninstallReport {
            package: name.to_string(),
            revision: r.revision,
            data_retained: true,
            cleanup_errors,
        })
    }
    pub fn clear_data(&self, name: &str) -> Result<(), PlatformError> {
        package(name)?;
        self.latest(name)?;
        let path = format!("userdata/{name}");
        if self.fs.stat(&path)?.is_some() {
            self.fs.remove(&path, true)?;
        }
        fs::mkdirs(&self.fs, &format!("{path}/files"))?;
        fs::mkdirs(&self.fs, &format!("{path}/cache"))
    }
}
impl<F: FileSystem + Clone> PackageManager<F> {
    pub fn vfs(&self, name: &str) -> Result<Vfs<F>, PlatformError> {
        Vfs::new(self.fs.clone(), name)
    }
}
