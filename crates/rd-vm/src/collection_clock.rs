//! GC measurements use the executing platform's clock, independently of the
//! guest's reference/virtual wall clock. Absence remains visible in statistics.
#[cfg(feature = "std")]
pub(crate) struct Stamp(std::time::Instant);
#[cfg(feature = "std")]
impl Stamp {
    pub(crate) fn start(_vm: &crate::Engine) -> Self {
        Self(std::time::Instant::now())
    }
    pub(crate) fn elapsed(&self) -> core::time::Duration {
        self.0.elapsed()
    }
    pub(crate) fn measured(&self) -> bool {
        true
    }
}

#[cfg(not(feature = "std"))]
pub(crate) struct Stamp {
    host: Option<alloc::rc::Rc<dyn crate::framework_host::FrameworkHost>>,
    start: Option<u64>,
}
#[cfg(not(feature = "std"))]
impl Stamp {
    pub(crate) fn start(vm: &crate::Engine) -> Self {
        let host = vm.framework_host();
        let start = host.as_ref().and_then(|h| h.monotonic_micros());
        Self { host, start }
    }
    pub(crate) fn elapsed(&self) -> core::time::Duration {
        let elapsed = self
            .start
            .zip(self.host.as_ref().and_then(|h| h.monotonic_micros()))
            .map_or(0, |(start, now)| now.saturating_sub(start));
        core::time::Duration::from_micros(elapsed)
    }
    pub(crate) fn measured(&self) -> bool {
        self.start.is_some()
    }
}
