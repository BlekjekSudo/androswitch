//! Root-relative storage shared by package management and the guest VFS.
use crate::PlatformError;
use alloc::{string::String, vec::Vec};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub directory: bool,
    pub length: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}
pub trait FileSystem {
    fn stat(&self, path: &str) -> Result<Option<Metadata>, PlatformError>;
    fn read_at(&self, path: &str, offset: u64, output: &mut [u8]) -> Result<usize, PlatformError>;
    fn write_at(
        &self,
        path: &str,
        offset: u64,
        input: &[u8],
        truncate: bool,
    ) -> Result<usize, PlatformError>;
    fn create_dir(&self, path: &str, exclusive: bool) -> Result<bool, PlatformError>;
    fn list(&self, path: &str) -> Result<Vec<Entry>, PlatformError>;
    /// Atomically publish to a new, absent destination on the same filesystem.
    fn rename_new(&self, source: &str, destination: &str) -> Result<(), PlatformError>;
    fn remove(&self, path: &str, recursive: bool) -> Result<(), PlatformError>;
}
impl<T: FileSystem + ?Sized> FileSystem for alloc::rc::Rc<T> {
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        (**self).stat(p)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        (**self).read_at(p, o, b)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        (**self).write_at(p, o, b, t)
    }
    fn create_dir(&self, p: &str, e: bool) -> Result<bool, PlatformError> {
        (**self).create_dir(p, e)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        (**self).list(p)
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        (**self).rename_new(a, b)
    }
    fn remove(&self, p: &str, r: bool) -> Result<(), PlatformError> {
        (**self).remove(p, r)
    }
}
pub fn error(
    code: &'static str,
    operation: &'static str,
    cause: impl Into<String>,
) -> PlatformError {
    PlatformError {
        code,
        operation,
        cause: cause.into(),
    }
}
pub fn read_all(fs: &dyn FileSystem, path: &str, limit: usize) -> Result<Vec<u8>, PlatformError> {
    let meta = fs
        .stat(path)?
        .ok_or_else(|| error("NOT_FOUND", "read", path))?;
    if meta.directory || meta.length > limit as u64 {
        return Err(error("INPUT_TOO_LARGE", "read", path));
    }
    let mut out = alloc::vec![0; meta.length as usize];
    let mut offset = 0;
    while offset < out.len() {
        let n = fs.read_at(path, offset as u64, &mut out[offset..])?;
        if n == 0 || n > out.len() - offset {
            return Err(error("SHORT_READ", "read", path));
        }
        offset += n;
    }
    Ok(out)
}
pub fn write_all(fs: &dyn FileSystem, path: &str, input: &[u8]) -> Result<(), PlatformError> {
    let mut offset = 0;
    loop {
        let end = (offset + (64 << 10)).min(input.len());
        if fs.write_at(path, offset as u64, &input[offset..end], offset == 0)? != end - offset {
            return Err(error("SHORT_WRITE", "write", path));
        }
        offset = end;
        if offset == input.len() {
            return Ok(());
        }
    }
}
pub fn mkdirs(fs: &dyn FileSystem, path: &str) -> Result<(), PlatformError> {
    let mut prefix = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        fs.create_dir(&prefix, false)?;
    }
    Ok(())
}
