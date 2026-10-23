//! libnx filesystem FFI. All buffers are owned by Rust for each synchronous
//! call. The C adapter commits writes before atomic revision publication.
use alloc::{
    ffi::CString,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use as_platform::{
    fs::{self, Entry, FileSystem, Metadata},
    PlatformError,
};
#[allow(unsafe_code)]
extern "C" {
    fn as_storage_prepare(path: *const core::ffi::c_char) -> i32;
    fn as_storage_stat(
        path: *const core::ffi::c_char,
        length: *mut u64,
        directory: *mut u32,
    ) -> i32;
    fn as_storage_read(
        path: *const core::ffi::c_char,
        offset: u64,
        buffer: *mut u8,
        length: usize,
    ) -> i64;
    fn as_storage_write(
        path: *const core::ffi::c_char,
        offset: u64,
        buffer: *const u8,
        length: usize,
        truncate: u32,
    ) -> i64;
    fn as_storage_mkdir(path: *const core::ffi::c_char, exclusive: u32) -> i32;
    fn as_storage_list(path: *const core::ffi::c_char, buffer: *mut u8, length: usize) -> i64;
    fn as_storage_rename(
        source: *const core::ffi::c_char,
        destination: *const core::ffi::c_char,
    ) -> i32;
    fn as_storage_remove(path: *const core::ffi::c_char, recursive: u32) -> i32;
}
#[derive(Clone)]
pub struct LibnxFileSystem {
    root: String,
}
fn error(op: &'static str, code: i64) -> PlatformError {
    fs::error("IO_ERROR", op, format!("libnx/newlib error {code}"))
}
impl LibnxFileSystem {
    pub fn new(root: &str) -> Result<Self, PlatformError> {
        let path = CString::new(root).map_err(|_| error("root", -1))?;
        // SAFETY: NUL-terminated path is alive for this synchronous call.
        #[allow(unsafe_code)]
        let result = unsafe { as_storage_prepare(path.as_ptr()) };
        if result < 0 {
            return Err(error("root", result as i64));
        }
        Ok(Self {
            root: root.to_string(),
        })
    }
    fn path(&self, p: &str) -> Result<CString, PlatformError> {
        if p.starts_with('/')
            || p.contains(['\\', ':', '\0'])
            || p.split('/').any(|v| matches!(v, "." | ".."))
        {
            return Err(fs::error("INVALID_PATH", "libnx_path", p));
        }
        CString::new(if p.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{p}", self.root)
        })
        .map_err(|_| error("path", -1))
    }
}
impl FileSystem for LibnxFileSystem {
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        let path = self.path(p)?;
        let (mut length, mut directory) = (0, 0);
        // SAFETY: Path and writable scalar outputs live throughout this call.
        #[allow(unsafe_code)]
        let rc = unsafe { as_storage_stat(path.as_ptr(), &mut length, &mut directory) };
        match rc {
            0 => Ok(Some(Metadata {
                directory: directory != 0,
                length,
            })),
            1 => Ok(None),
            _ => Err(error("stat", rc as i64)),
        }
    }
    fn read_at(&self, p: &str, offset: u64, out: &mut [u8]) -> Result<usize, PlatformError> {
        let path = self.path(p)?;
        // SAFETY: output is writable for its stated length, path is terminated.
        #[allow(unsafe_code)]
        let rc = unsafe { as_storage_read(path.as_ptr(), offset, out.as_mut_ptr(), out.len()) };
        if rc < 0 {
            Err(error("read", rc))
        } else {
            Ok(rc as usize)
        }
    }
    fn write_at(
        &self,
        p: &str,
        offset: u64,
        bytes: &[u8],
        truncate: bool,
    ) -> Result<usize, PlatformError> {
        let path = self.path(p)?;
        // SAFETY: input is readable for its stated length; no pointers retained.
        #[allow(unsafe_code)]
        let rc = unsafe {
            as_storage_write(
                path.as_ptr(),
                offset,
                bytes.as_ptr(),
                bytes.len(),
                truncate as u32,
            )
        };
        if rc < 0 {
            Err(error("write", rc))
        } else {
            Ok(rc as usize)
        }
    }
    fn create_dir(&self, p: &str, exclusive: bool) -> Result<bool, PlatformError> {
        let path = self.path(p)?;
        // SAFETY: NUL-terminated path is alive throughout the call.
        #[allow(unsafe_code)]
        let rc = unsafe { as_storage_mkdir(path.as_ptr(), exclusive as u32) };
        if rc < 0 {
            Err(error("mkdir", rc as i64))
        } else {
            Ok(rc == 1)
        }
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        let path = self.path(p)?;
        let mut capacity = 4096;
        loop {
            let mut buffer = vec![0; capacity];
            // SAFETY: allocated buffer is writable for the exact passed length.
            #[allow(unsafe_code)]
            let rc = unsafe { as_storage_list(path.as_ptr(), buffer.as_mut_ptr(), buffer.len()) };
            if rc == -28 && capacity < (4 << 20) {
                capacity *= 2;
                continue;
            }
            if rc < 0 {
                return Err(error("list", rc));
            }
            let mut out = Vec::new();
            for entry in buffer[..rc as usize]
                .split(|b| *b == 0)
                .filter(|e| !e.is_empty())
            {
                let name = core::str::from_utf8(&entry[1..])
                    .map_err(|_| error("list", -1))?
                    .to_string();
                out.push(Entry {
                    name,
                    directory: entry[0] == b'D',
                });
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(out);
        }
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        let (a, b) = (self.path(a)?, self.path(b)?);
        // SAFETY: both paths are valid terminated strings for the call.
        #[allow(unsafe_code)]
        let rc = unsafe { as_storage_rename(a.as_ptr(), b.as_ptr()) };
        if rc < 0 {
            Err(error("rename", rc as i64))
        } else {
            Ok(())
        }
    }
    fn remove(&self, p: &str, recursive: bool) -> Result<(), PlatformError> {
        if p.is_empty() {
            return Err(fs::error("ACCESS_DENIED", "remove", "managed root"));
        }
        let path = self.path(p)?;
        // SAFETY: root-relative path was checked above and remains live.
        #[allow(unsafe_code)]
        let rc = unsafe { as_storage_remove(path.as_ptr(), recursive as u32) };
        if rc < 0 {
            Err(error("remove", rc as i64))
        } else {
            Ok(())
        }
    }
}
