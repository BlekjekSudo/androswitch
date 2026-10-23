use alloc::{format, string::String, sync::Arc};
use as_platform::fs::{FileSystem, ReadOnlyFile};
use rd_apk::{
    zip::{ReadAt, ZipLimits},
    Apk, RdError, RdResult,
};

struct Source(Arc<dyn ReadOnlyFile>);
impl core::fmt::Debug for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StorageApk")
            .field("bytes", &self.0.len())
            .finish()
    }
}
impl ReadAt for Source {
    fn len(&self) -> u64 {
        self.0.len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> RdResult<usize> {
        self.0
            .read_at(offset, output)
            .map_err(|e| RdError::parse(format!("APK storage: {e}")))
    }
}
pub fn open_apk(fs: &dyn FileSystem, path: &str) -> Result<Apk, as_platform::PlatformError> {
    Apk::from_source(Arc::new(Source(fs.open_read(path)?)), ZipLimits::default())
        .map_err(|e| super::error("INVALID_APK", e.to_string()))
}
use alloc::string::ToString;

/// SHA-256 and optional copying share bounded positional I/O. No container-sized
/// allocation is permitted, including installation verification and no-op updates.
pub(super) fn transfer(
    fs: &dyn FileSystem,
    source: &str,
    destination: Option<&str>,
) -> Result<(u64, String), as_platform::PlatformError> {
    use sha2::{Digest, Sha256};
    let meta = fs
        .stat(source)?
        .ok_or_else(|| super::error("NOT_FOUND", source))?;
    if meta.directory || meta.length > super::MAX_APK_BYTES as u64 {
        return Err(super::error(
            "INPUT_TOO_LARGE",
            "APK exceeds 512 MiB or is a directory",
        ));
    }
    let mut hash = Sha256::new();
    let reader = match fs.open_read(source) {
        Ok(reader) => Some(reader),
        Err(e) if e.code == "API_UNAVAILABLE" => None,
        Err(e) => return Err(e),
    };
    let mut writer = destination
        .map(|dest| fs.begin_write(dest))
        .transpose()?
        .flatten();
    let mut buffer = alloc::vec![0; 64 << 10];
    let mut position = 0;
    while position < meta.length {
        let count = (meta.length - position).min(buffer.len() as u64) as usize;
        let n = match &reader {
            Some(reader) => reader.read_at(position, &mut buffer[..count])?,
            None => fs.read_at(source, position, &mut buffer[..count])?,
        };
        if n == 0 || n > count {
            return Err(super::error("SHORT_READ", source));
        }
        hash.update(&buffer[..n]);
        if let Some(dest) = destination {
            let written = match &mut writer {
                Some(writer) => writer.write(&buffer[..n])?,
                None => fs.write_at(dest, position, &buffer[..n], position == 0)?,
            };
            if written != n {
                return Err(super::error("SHORT_WRITE", dest));
            }
        }
        position += n as u64;
    }
    if let Some(writer) = &mut writer {
        writer.finish()?;
    } else if position == 0 {
        if let Some(dest) = destination {
            fs.write_at(dest, 0, &[], true)?;
        }
    }
    if fs.stat(source)? != Some(meta) {
        return Err(super::error("INPUT_CHANGED", source));
    }
    use core::fmt::Write;
    let mut text = String::with_capacity(64);
    for byte in hash.finalize() {
        write!(&mut text, "{byte:02x}").expect("String write");
    }
    Ok((position, text))
}
