//! Java and native use the same mount resolver; private data is not exposed
//! through the SD mount containing the registry and runtime files.
use crate::{
    fs::{self, Entry, FileSystem, Metadata},
    PlatformError,
};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
pub fn valid_package(name: &str) -> bool {
    name.contains('.')
        && name.len() <= 200
        && name.split('.').all(|p| {
            !p.is_empty()
                && p.as_bytes()[0].is_ascii_alphabetic()
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}
pub fn normalize(path: &str) -> Result<String, PlatformError> {
    if !path.starts_with('/') || path.contains(['\\', '\0', ':']) {
        return Err(fs::error("INVALID_PATH", "resolve", path));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(fs::error("ACCESS_DENIED", "resolve", path));
                }
            }
            value => parts.push(value),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}
pub trait GuestFileSystem {
    /// Rename with the platform's replacement semantics. A backend that only
    /// supports publication to absent names must report existing targets.
    fn rename_replace(&self, source: &str, destination: &str) -> Result<(), PlatformError> {
        if self.stat(destination)?.is_some() {
            return Err(fs::error("API_UNAVAILABLE", "rename_replace", destination));
        }
        self.rename_new(source, destination)
    }
    fn create_file_new(&self, path: &str) -> Result<bool, PlatformError> {
        Err(fs::error("API_UNAVAILABLE", "create_file_new", path))
    }
    fn create_dir(&self, path: &str) -> Result<bool, PlatformError> {
        Err(fs::error("API_UNAVAILABLE", "create_dir", path))
    }
    fn modified_millis(&self, path: &str) -> Result<u64, PlatformError> {
        Err(fs::error("API_UNAVAILABLE", "modified_millis", path))
    }
    fn sync_file(&self, path: &str) -> Result<(), PlatformError> {
        Err(fs::error("API_UNAVAILABLE", "sync_file", path))
    }
    fn resolve(&self, path: &str) -> Result<String, PlatformError>;
    fn stat(&self, path: &str) -> Result<Option<Metadata>, PlatformError>;
    fn read_at(&self, path: &str, offset: u64, output: &mut [u8]) -> Result<usize, PlatformError>;
    fn write_at(
        &self,
        path: &str,
        offset: u64,
        input: &[u8],
        truncate: bool,
    ) -> Result<usize, PlatformError>;
    fn mkdirs(&self, path: &str) -> Result<(), PlatformError>;
    fn list(&self, path: &str) -> Result<Vec<Entry>, PlatformError>;
    fn remove(&self, path: &str) -> Result<(), PlatformError>;
    fn rename_new(&self, source: &str, destination: &str) -> Result<(), PlatformError> {
        Err(fs::error(
            "API_UNAVAILABLE",
            "rename",
            format!("{source} -> {destination}"),
        ))
    }
}
pub struct Vfs<F> {
    pub fs: F,
    package: String,
    source_apk: Option<String>,
}
impl<F: FileSystem> Vfs<F> {
    pub fn new(fs: F, package: &str) -> Result<Self, PlatformError> {
        if !valid_package(package) {
            return Err(fs::error("INVALID_PACKAGE", "mount", package));
        }
        Ok(Self {
            fs,
            package: package.to_string(),
            source_apk: None,
        })
    }
    /// Mount an existing immutable installation as a read-only Android APK.
    pub fn bind_apk(&mut self, storage_path: String) -> Result<String, PlatformError> {
        let parts = storage_path.split('/').collect::<Vec<_>>();
        if parts.len() != 4
            || parts[0] != "installed"
            || parts[1] != self.package
            || parts[2].len() != 20
            || !parts[2].bytes().all(|b| b.is_ascii_digit())
            || parts[3] != "package.apk"
        {
            return Err(fs::error("INVALID_APK_MOUNT", "mount", storage_path));
        }
        if !self.fs.stat(&storage_path)?.is_some_and(|m| !m.directory) {
            return Err(fs::error("NOT_FOUND", "mount", storage_path));
        }
        self.source_apk = Some(storage_path);
        Ok(format!("/data/app/{}/base.apk", self.package))
    }
    fn apk_path(&self, path: &str) -> Result<Option<String>, PlatformError> {
        let path = normalize(path)?;
        if path == "/data/app" || path.starts_with("/data/app/") {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
    fn writable(&self, path: &str) -> Result<(), PlatformError> {
        if self.apk_path(path)?.is_some() {
            return Err(fs::error("READ_ONLY_FILESYSTEM", "write", path));
        }
        Ok(())
    }
}
impl<F: FileSystem> GuestFileSystem for Vfs<F> {
    fn rename_replace(&self, source: &str, destination: &str) -> Result<(), PlatformError> {
        self.writable(source)?;
        self.writable(destination)?;
        let source = self.resolve(source)?;
        let destination = self.resolve(destination)?;
        let private = format!("userdata/{}", self.package);
        if source.is_empty()
            || destination.is_empty()
            || source == private
            || destination == private
        {
            return Err(fs::error("ACCESS_DENIED", "rename_replace", source));
        }
        self.fs.rename_replace(&source, &destination)
    }
    fn create_file_new(&self, path: &str) -> Result<bool, PlatformError> {
        self.writable(path)?;
        self.fs.create_file_new(&self.resolve(path)?)
    }
    fn create_dir(&self, path: &str) -> Result<bool, PlatformError> {
        self.writable(path)?;
        self.fs.create_dir(&self.resolve(path)?, true)
    }
    fn modified_millis(&self, path: &str) -> Result<u64, PlatformError> {
        self.fs.modified_millis(&self.resolve(path)?)
    }
    fn sync_file(&self, path: &str) -> Result<(), PlatformError> {
        self.fs.sync_file(&self.resolve(path)?)
    }
    fn resolve(&self, path: &str) -> Result<String, PlatformError> {
        let path = normalize(path)?;
        if let Some(apk) = &self.source_apk {
            if path == format!("/data/app/{}/base.apk", self.package) {
                return Ok(apk.clone());
            }
        }
        if self.apk_path(&path)?.is_some() {
            return Err(fs::error("ACCESS_DENIED", "resolve", path));
        }
        for prefix in ["/data/user/0/", "/data/data/"] {
            if let Some(rest) = path.strip_prefix(prefix) {
                let (package, tail) = rest.split_once('/').unwrap_or((rest, ""));
                if package != self.package {
                    return Err(fs::error("ACCESS_DENIED", "resolve", path));
                }
                return Ok(format!("userdata/{package}/{tail}")
                    .trim_end_matches('/')
                    .to_string());
            }
        }
        let shared = ["/sdcard", "/storage/emulated/0"]
            .into_iter()
            .find_map(|p| {
                if path == p {
                    Some("")
                } else {
                    path.strip_prefix(p).and_then(|s| s.strip_prefix('/'))
                }
            })
            .ok_or_else(|| fs::error("UNMAPPED_PATH", "resolve", &path))?;
        if [
            "installed",
            "userdata",
            "runtime",
            "cache",
            "logs",
            "apks",
            "androswitch.nro",
        ]
        .iter()
        .any(|reserved| {
            shared
                .split('/')
                .next()
                .unwrap_or("")
                .eq_ignore_ascii_case(reserved)
        }) {
            return Err(fs::error("ACCESS_DENIED", "resolve", path));
        }
        Ok(shared.to_string())
    }
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        if self.source_apk.is_some() {
            if let Some(path) = self.apk_path(p)? {
                if path == "/data/app" || path == format!("/data/app/{}", self.package) {
                    return Ok(Some(Metadata {
                        directory: true,
                        length: 0,
                    }));
                }
            }
        }
        self.fs.stat(&self.resolve(p)?)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        self.fs.read_at(&self.resolve(p)?, o, b)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        self.writable(p)?;
        self.fs.write_at(&self.resolve(p)?, o, b, t)
    }
    fn mkdirs(&self, p: &str) -> Result<(), PlatformError> {
        self.writable(p)?;
        fs::mkdirs(&self.fs, &self.resolve(p)?)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        if self.source_apk.is_some() {
            if let Some(path) = self.apk_path(p)? {
                if path == "/data/app" {
                    return Ok(alloc::vec![Entry {
                        name: self.package.clone(),
                        directory: true
                    }]);
                }
                if path == format!("/data/app/{}", self.package) {
                    return Ok(alloc::vec![Entry {
                        name: "base.apk".to_string(),
                        directory: false
                    }]);
                }
            }
        }
        let path = self.resolve(p)?;
        let mut entries = self.fs.list(&path)?;
        if path.is_empty() {
            entries.retain(|e| self.resolve(&format!("/sdcard/{}", e.name)).is_ok());
        }
        Ok(entries)
    }
    fn rename_new(&self, source: &str, destination: &str) -> Result<(), PlatformError> {
        self.writable(source)?;
        self.writable(destination)?;
        let source = self.resolve(source)?;
        let destination = self.resolve(destination)?;
        if source.is_empty()
            || destination.is_empty()
            || source == format!("userdata/{}", self.package)
            || destination == format!("userdata/{}", self.package)
        {
            return Err(fs::error("ACCESS_DENIED", "rename", source));
        }
        if self.fs.stat(&source)?.is_none() {
            return Err(fs::error("NOT_FOUND", "rename", source));
        }
        if source == destination {
            return Ok(());
        }
        if self.fs.stat(&destination)?.is_some() {
            return Err(fs::error(
                "API_UNAVAILABLE",
                "rename",
                "atomic replacement of existing destination",
            ));
        }
        self.fs.rename_new(&source, &destination)
    }
    fn remove(&self, p: &str) -> Result<(), PlatformError> {
        self.writable(p)?;
        let path = self.resolve(p)?;
        if path.is_empty() || path == format!("userdata/{}", self.package) {
            return Err(fs::error("ACCESS_DENIED", "delete", p));
        }
        self.fs.remove(&path, false)
    }
}

#[cfg(all(test, feature = "std"))]
mod native_rename_tests {
    use super::*;
    #[test]
    fn file_sync_uses_the_shared_mount_and_real_host_commit() {
        let root = std::env::temp_dir().join(format!(
            "androswitch-sync-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let host = alloc::rc::Rc::new(crate::host_fs::HostFileSystem::new(&root).unwrap());
        fs::mkdirs(host.as_ref(), "Download").unwrap();
        let vfs = Vfs::new(host, "org.fixture.general").unwrap();
        vfs.write_at("/sdcard/Download/save", 0, b"committed", true)
            .unwrap();
        vfs.sync_file("/storage/emulated/0/Download/save").unwrap();
        let mut bytes = [0; 9];
        assert_eq!(
            vfs.read_at("/sdcard/Download/save", 0, &mut bytes).unwrap(),
            9
        );
        assert_eq!(&bytes, b"committed");
        assert_eq!(
            vfs.sync_file("/sdcard/Download/missing").unwrap_err().code,
            "NOT_FOUND"
        );
        assert_eq!(
            vfs.sync_file("/sdcard/Download").unwrap_err().code,
            "IS_DIRECTORY"
        );
        assert_eq!(
            vfs.sync_file("/data/user/0/org.other/save")
                .unwrap_err()
                .code,
            "ACCESS_DENIED"
        );
        std::fs::remove_file(root.join("Download/save")).unwrap();
        std::fs::remove_dir(root.join("Download")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn virtual_rename_uses_shared_storage_and_preserves_existing_destination() {
        let root = std::env::temp_dir().join(format!(
            "androswitch-native-rename-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let host = crate::host_fs::HostFileSystem::new(&root).unwrap();
        fs::mkdirs(&host, "Download").unwrap();
        let vfs = Vfs::new(host, "org.fixture.general").unwrap();
        vfs.write_at("/sdcard/Download/source", 0, b"save", true)
            .unwrap();
        vfs.rename_new(
            "/sdcard/Download/source",
            "/storage/emulated/0/Download/target",
        )
        .unwrap();
        assert!(vfs.stat("/sdcard/Download/source").unwrap().is_none());
        let mut bytes = [0; 4];
        assert_eq!(
            vfs.read_at("/sdcard/Download/target", 0, &mut bytes)
                .unwrap(),
            4
        );
        assert_eq!(&bytes, b"save");
        vfs.rename_new(
            "/sdcard/Download/target",
            "/storage/emulated/0/Download/target",
        )
        .unwrap();
        vfs.write_at("/sdcard/Download/other", 0, b"keep", true)
            .unwrap();
        assert_eq!(
            vfs.rename_new("/sdcard/Download/target", "/sdcard/Download/other")
                .unwrap_err()
                .code,
            "API_UNAVAILABLE"
        );
        assert_eq!(
            vfs.read_at("/sdcard/Download/other", 0, &mut bytes)
                .unwrap(),
            4
        );
        assert_eq!(&bytes, b"keep");
        assert!(vfs.stat("/sdcard/Download/target").unwrap().is_some());
        assert_eq!(
            vfs.rename_new("/data/user/0/org.other/source", "/sdcard/Download/new")
                .unwrap_err()
                .code,
            "ACCESS_DENIED"
        );
        std::fs::remove_file(root.join("Download/target")).unwrap();
        std::fs::remove_file(root.join("Download/other")).unwrap();
        std::fs::remove_dir(root.join("Download")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
