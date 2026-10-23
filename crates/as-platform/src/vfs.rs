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
}
pub struct Vfs<F> {
    pub fs: F,
    package: String,
}
impl<F: FileSystem> Vfs<F> {
    pub fn new(fs: F, package: &str) -> Result<Self, PlatformError> {
        if !valid_package(package) {
            return Err(fs::error("INVALID_PACKAGE", "mount", package));
        }
        Ok(Self {
            fs,
            package: package.to_string(),
        })
    }
}
impl<F: FileSystem> GuestFileSystem for Vfs<F> {
    fn resolve(&self, path: &str) -> Result<String, PlatformError> {
        let path = normalize(path)?;
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
        self.fs.stat(&self.resolve(p)?)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        self.fs.read_at(&self.resolve(p)?, o, b)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        self.fs.write_at(&self.resolve(p)?, o, b, t)
    }
    fn mkdirs(&self, p: &str) -> Result<(), PlatformError> {
        fs::mkdirs(&self.fs, &self.resolve(p)?)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        let path = self.resolve(p)?;
        let mut entries = self.fs.list(&path)?;
        if path.is_empty() {
            entries.retain(|e| self.resolve(&format!("/sdcard/{}", e.name)).is_ok());
        }
        Ok(entries)
    }
    fn remove(&self, p: &str) -> Result<(), PlatformError> {
        let path = self.resolve(p)?;
        if path.is_empty() || path == format!("userdata/{}", self.package) {
            return Err(fs::error("ACCESS_DENIED", "delete", p));
        }
        self.fs.remove(&path, false)
    }
}
