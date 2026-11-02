//! Parser-only cache synchronization. Device std uses the real platform locks;
//! the retained core/alloc profile uses spin locks and never executes guest code
//! while a cache is locked or being initialized.
#[cfg(feature = "std")]
pub(crate) use std::sync::{Mutex, OnceLock};

#[cfg(not(feature = "std"))]
#[derive(Debug, Default)]
pub(crate) struct OnceLock<T>(spin::Once<T>);

#[cfg(not(feature = "std"))]
impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        Self(spin::Once::new())
    }
    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        self.0.call_once(init)
    }
}

#[cfg(not(feature = "std"))]
#[derive(Debug)]
pub(crate) struct Mutex<T>(spin::Mutex<T>);

#[cfg(not(feature = "std"))]
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(spin::Mutex::new(value))
    }
    pub fn lock(&self) -> Result<spin::MutexGuard<'_, T>, core::convert::Infallible> {
        Ok(self.0.lock())
    }
}
