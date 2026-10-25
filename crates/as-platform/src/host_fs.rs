use crate::{
    fs::{self, Entry, FileSystem, Metadata},
    PlatformError,
};
use std::{
    fs as disk,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone)]
pub struct HostFileSystem {
    root: Arc<PathBuf>,
}
fn io(op: &'static str, path: &str, e: std::io::Error) -> PlatformError {
    fs::error(
        if e.kind() == std::io::ErrorKind::NotFound {
            "NOT_FOUND"
        } else {
            "IO_ERROR"
        },
        op,
        format!("{path}: {e}"),
    )
}
fn reparse(meta: &disk::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
impl HostFileSystem {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, PlatformError> {
        disk::create_dir_all(root.as_ref()).map_err(|e| io("root", "root", e))?;
        if reparse(&disk::symlink_metadata(root.as_ref()).map_err(|e| io("root", "root", e))?) {
            return Err(fs::error(
                "ACCESS_DENIED",
                "root",
                "root cannot be a link/reparse point",
            ));
        }
        Ok(Self {
            root: Arc::new(disk::canonicalize(root).map_err(|e| io("root", "root", e))?),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn path(&self, relative: &str) -> Result<PathBuf, PlatformError> {
        let mut path = self.root.as_ref().clone();
        if relative.contains(['\\', ':', '\0']) || relative.starts_with('/') {
            return Err(fs::error("INVALID_PATH", "host_path", relative));
        }
        for part in relative.split('/').filter(|p| !p.is_empty()) {
            if matches!(part, "." | "..") || part.ends_with(['.', ' ']) {
                return Err(fs::error("INVALID_PATH", "host_path", relative));
            }
            path.push(part);
            match disk::symlink_metadata(&path) {
                Ok(meta) if reparse(&meta) => {
                    return Err(fs::error(
                        "ACCESS_DENIED",
                        "host_path",
                        "link/reparse point in managed path",
                    ))
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io("host_path", relative, e)),
            }
        }
        if !path.starts_with(self.root.as_ref()) {
            return Err(fs::error("ACCESS_DENIED", "host_path", relative));
        }
        Ok(path)
    }
}
impl FileSystem for HostFileSystem {
    fn sync_file(&self, p: &str) -> Result<(), PlatformError> {
        let meta = self
            .stat(p)?
            .ok_or_else(|| fs::error("NOT_FOUND", "sync_file", p))?;
        if meta.directory {
            return Err(fs::error("IS_DIRECTORY", "sync_file", p));
        }
        // FlushFileBuffers on Windows requires a writable native handle.
        // This operation never changes content or the guest's descriptor mode.
        disk::OpenOptions::new()
            .write(true)
            .open(self.path(p)?)
            .map_err(|e| io("sync_file", p, e))?
            .sync_all()
            .map_err(|e| io("sync_file", p, e))
    }
    fn begin_write(&self, p: &str) -> Result<Option<Box<dyn fs::WriteFile>>, PlatformError> {
        let file = disk::File::create(self.path(p)?).map_err(|e| io("begin_write", p, e))?;
        Ok(Some(Box::new(HostWriteFile(file))))
    }
    fn open_read(&self, p: &str) -> Result<Arc<dyn fs::ReadOnlyFile>, PlatformError> {
        let file = disk::File::open(self.path(p)?).map_err(|e| io("open_read", p, e))?;
        let length = file.metadata().map_err(|e| io("stat", p, e))?.len();
        Ok(Arc::new(HostReadFile { file, length }))
    }
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        match disk::metadata(self.path(p)?) {
            Ok(m) => Ok(Some(Metadata {
                directory: m.is_dir(),
                length: m.len(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io("stat", p, e)),
        }
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        let mut f = disk::File::open(self.path(p)?).map_err(|e| io("read", p, e))?;
        f.seek(SeekFrom::Start(o)).map_err(|e| io("seek", p, e))?;
        f.read(b).map_err(|e| io("read", p, e))
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        let mut f = disk::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(t)
            .open(self.path(p)?)
            .map_err(|e| io("write", p, e))?;
        f.seek(SeekFrom::Start(o)).map_err(|e| io("seek", p, e))?;
        f.write_all(b).map_err(|e| io("write", p, e))?;
        f.sync_all().map_err(|e| io("sync", p, e))?;
        Ok(b.len())
    }
    fn create_dir(&self, p: &str, exclusive: bool) -> Result<bool, PlatformError> {
        match disk::create_dir(self.path(p)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if exclusive || self.stat(p)?.is_some_and(|m| m.directory) {
                    Ok(false)
                } else {
                    Err(io("mkdir", p, e))
                }
            }
            Err(e) => Err(io("mkdir", p, e)),
        }
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        let mut out = Vec::new();
        for entry in disk::read_dir(self.path(p)?).map_err(|e| io("list", p, e))? {
            let entry = entry.map_err(|e| io("list", p, e))?;
            if reparse(&disk::symlink_metadata(entry.path()).map_err(|e| io("list", p, e))?) {
                return Err(fs::error("ACCESS_DENIED", "list", "link/reparse point"));
            }
            out.push(Entry {
                name: entry
                    .file_name()
                    .into_string()
                    .map_err(|_| fs::error("INVALID_PATH", "list", "non-Unicode filename"))?,
                directory: entry.file_type().map_err(|e| io("list", p, e))?.is_dir(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        if self.stat(b)?.is_some() {
            return Err(fs::error("ALREADY_EXISTS", "rename", b));
        }
        disk::rename(self.path(a)?, self.path(b)?).map_err(|e| io("rename", a, e))
    }
    fn remove(&self, p: &str, recursive: bool) -> Result<(), PlatformError> {
        let target = self.path(p)?;
        if target == *self.root {
            return Err(fs::error("ACCESS_DENIED", "remove", "managed root"));
        }
        let m = disk::metadata(&target).map_err(|e| io("remove", p, e))?;
        if m.is_dir() {
            if recursive {
                disk::remove_dir_all(&target)
            } else {
                disk::remove_dir(&target)
            }
        } else {
            disk::remove_file(&target)
        }
        .map_err(|e| io("remove", p, e))
    }
}

struct HostReadFile {
    file: disk::File,
    length: u64,
}
struct HostWriteFile(disk::File);
impl fs::WriteFile for HostWriteFile {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, PlatformError> {
        self.0
            .write_all(bytes)
            .map_err(|e| io("write_stream", "APK staging", e))?;
        Ok(bytes.len())
    }
    fn finish(&mut self) -> Result<(), PlatformError> {
        self.0.sync_all().map_err(|e| io("sync", "APK staging", e))
    }
}
impl fs::ReadOnlyFile for HostReadFile {
    fn len(&self) -> u64 {
        self.length
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize, PlatformError> {
        #[cfg(windows)]
        let result = {
            use std::os::windows::fs::FileExt;
            self.file.seek_read(output, offset)
        };
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::FileExt;
            self.file.read_at(output, offset)
        };
        result.map_err(|e| io("read_at", "owned APK", e))
    }
}
