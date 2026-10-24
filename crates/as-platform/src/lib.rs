//! Platform contracts usable from the device's `no_std + alloc` core.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt;

pub mod fs;
#[cfg(feature = "std")]
pub mod host;
#[cfg(feature = "std")]
pub mod host_fs;
pub mod vfs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError {
    pub code: &'static str,
    pub operation: &'static str,
    pub cause: String,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.operation, self.cause)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PlatformError {}

pub trait Storage {
    /// Read at most `limit` bytes, including if a file grows after metadata lookup.
    fn read_bounded(&self, path: &str, limit: usize) -> Result<Vec<u8>, PlatformError>;
}

pub trait Clock {
    fn monotonic_micros(&self) -> u64;
    /// Actual Unix epoch time, distinct from the elapsed-time sleep clock.
    /// Providers without a wall clock must fail before an absolute-time task
    /// is committed; inventing epoch time would silently change scheduling.
    fn wall_time_millis(&self) -> Result<i64, PlatformError> {
        Err(PlatformError {
            code: "API_UNAVAILABLE",
            operation: "wall_time_millis",
            cause: "platform wall clock is unavailable".into(),
        })
    }
}

/// Resources actually available to the executing runtime. Missing providers
/// must report unavailable; machine-wide specifications are not process limits.
pub trait RuntimeResources {
    /// Positive count of processors available to this process/VM. Query on
    /// each call because platform affinity/resource restrictions may change.
    fn available_processors(&self) -> Result<u32, PlatformError>;
}

/// One real, dedicated serial callback thread per positive opaque token.
/// The caller keeps callback owners and VM roots alive until platform joins
/// prove quiescence. Successful start creates the thread immediately; wake
/// cannot resurrect a terminal worker. Cancel only signals and never joins
/// or interrupts a callback that is currently executing.
pub trait BackgroundWorkers {
    fn start(&self, token: u64) -> Result<(), PlatformError>;
    fn wake(&self, token: u64) -> Result<(), PlatformError>;
    fn cancel(&self, token: u64) -> Result<(), PlatformError>;
}

pub trait Logger {
    fn log(&self, module: &str, message: &str);
}
