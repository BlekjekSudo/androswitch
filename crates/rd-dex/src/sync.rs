//! Immutable upstream parser caches retain synchronization on every target.
//! Device builds have no std; spin's Once publishes initialized data without
//! introducing an OS thread/lock dependency. Initializers only parse bytes and
//! never execute guest code or recursively initialize the same cache.
#[cfg(feature = "std")]
pub use std::sync::OnceLock;

#[cfg(not(feature = "std"))]
#[derive(Debug, Default)]
pub struct OnceLock<T>(spin::Once<T>);

#[cfg(not(feature = "std"))]
impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        Self(spin::Once::new())
    }
    pub fn get(&self) -> Option<&T> {
        self.0.get()
    }
    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        self.0.call_once(init)
    }
    pub fn set(&self, value: T) -> Result<(), T> {
        let mut value = Some(value);
        self.0
            .call_once(|| value.take().expect("initializer runs once"));
        match value {
            None => Ok(()),
            Some(value) => Err(value),
        }
    }
}

#[cfg(not(feature = "std"))]
impl<T: Clone> Clone for OnceLock<T> {
    fn clone(&self) -> Self {
        let result = Self::new();
        if let Some(value) = self.get() {
            let _ = result.set(value.clone());
        }
        result
    }
}
