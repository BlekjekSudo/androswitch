//! Real host services; these are never linked into a device-only build.
use crate::{Clock, PlatformError, RuntimeResources, Storage};
use std::{
    fs::File,
    io::Read,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

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
    fn wall_time_millis(&self) -> Result<i64, PlatformError> {
        let (magnitude, negative) = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(value) => (value.as_millis(), false),
            Err(error) => (error.duration().as_millis(), true),
        };
        let value = i64::try_from(magnitude).map_err(|_| PlatformError {
            code: "CLOCK_RANGE",
            operation: "wall_time_millis",
            cause: "host epoch time exceeds signed milliseconds".into(),
        })?;
        Ok(if negative { -value } else { value })
    }
}

impl RuntimeResources for HostPlatform {
    fn available_processors(&self) -> Result<u32, PlatformError> {
        let actual = std::thread::available_parallelism().map_err(|failure| PlatformError {
            code: "HOST_RESOURCE_ERROR",
            operation: "available_processors",
            cause: failure.to_string(),
        })?;
        u32::try_from(actual.get()).map_err(|_| PlatformError {
            code: "RESOURCE_INFO_RANGE",
            operation: "available_processors",
            cause: "host available processor count exceeds u32".into(),
        })
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
    fn resource_provider_reads_actual_host_parallelism() {
        let actual = HostPlatform::default().available_processors().unwrap();
        assert!(actual > 0);
        let reference = std::thread::available_parallelism().unwrap().get();
        assert_eq!(actual as usize, reference);
    }

    #[test]
    fn wall_clock_requires_provider_and_host_reports_actual_epoch() {
        struct ElapsedOnly;
        impl Clock for ElapsedOnly {
            fn monotonic_micros(&self) -> u64 {
                123
            }
        }
        let unavailable = ElapsedOnly.wall_time_millis().unwrap_err();
        assert_eq!(unavailable.code, "API_UNAVAILABLE");
        assert_eq!(unavailable.operation, "wall_time_millis");
        let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let actual = HostPlatform::default().wall_time_millis().unwrap();
        let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        assert!(i128::from(actual) >= before.as_millis() as i128);
        assert!(i128::from(actual) <= after.as_millis() as i128);
    }

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
