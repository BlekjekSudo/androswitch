//! Real host services; these are never linked into a device-only build.
use crate::{Clock, PlatformError, Storage};
use std::{fs::File, io::Read, time::Instant};

pub struct HostPlatform {
    started: Instant,
}

impl Default for HostPlatform {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Clock for HostPlatform {
    fn monotonic_micros(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

impl Storage for HostPlatform {
    fn read_bounded(&self, path: &str, limit: usize) -> Result<Vec<u8>, PlatformError> {
        let io_error = |e: std::io::Error| PlatformError {
            code: "IO_ERROR",
            operation: "read",
            cause: format!("{path}: {e}"),
        };
        let too_large = || PlatformError {
            code: "INPUT_TOO_LARGE",
            operation: "read",
            cause: format!("{path} exceeds the {limit}-byte inspection limit"),
        };
        let file = File::open(path).map_err(io_error)?;
        if file.metadata().map_err(io_error)?.len() > limit as u64 {
            return Err(too_large());
        }
        let read_limit = u64::try_from(limit)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(too_large)?;
        let mut bytes = Vec::new();
        file.take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > limit {
            return Err(too_large());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_accepts_exact_limit_and_rejects_larger_input() {
        let root = std::env::temp_dir().join(format!(
            "as-platform-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("bytes");
        std::fs::write(&path, b"four").unwrap();
        let host = HostPlatform::default();
        let path = path.to_str().unwrap();
        assert_eq!(host.read_bounded(path, 4).unwrap(), b"four");
        assert_eq!(
            host.read_bounded(path, 3).unwrap_err().code,
            "INPUT_TOO_LARGE"
        );
        assert_eq!(
            host.read_bounded(path, 0).unwrap_err().code,
            "INPUT_TOO_LARGE"
        );
        std::fs::remove_file(root.join("bytes")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
