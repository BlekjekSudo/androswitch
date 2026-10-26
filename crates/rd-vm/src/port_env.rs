//! Desktop diagnostic overrides are not environment variables on Horizon.
//! Platform observations must be supplied by the bound platform adapter.
#[cfg(feature = "std")]
pub(crate) use std::env::var;

#[cfg(not(feature = "std"))]
pub(crate) fn var(_name: &str) -> Result<alloc::string::String, MissingEnvironment> {
    Err(MissingEnvironment)
}

#[cfg(not(feature = "std"))]
#[derive(Debug)]
pub(crate) struct MissingEnvironment;

#[cfg(feature = "std")]
pub(crate) const ARCH: &str = std::env::consts::ARCH;
#[cfg(not(feature = "std"))]
pub(crate) const ARCH: &str = if cfg!(target_arch = "aarch64") {
    "aarch64"
} else {
    "x86_64"
};
