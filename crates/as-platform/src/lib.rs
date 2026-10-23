//! Platform contracts usable from the device's `no_std + alloc` core.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt;

#[cfg(feature = "std")]
pub mod host;

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
}

pub trait Logger {
    fn log(&self, module: &str, message: &str);
}
