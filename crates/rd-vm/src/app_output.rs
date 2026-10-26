//! Per-Engine output transport. Java API algorithms do not own stdout globals.
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
use crate::framework_host::FrameworkHost;
use crate::{Engine, VmExit};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};

pub trait AppOutput {
    fn write(&self, fd: i32, text: &str) -> Result<usize, VmExit>;
}

#[derive(Default)]
pub(crate) struct Output {
    sink: RefCell<Option<Rc<dyn AppOutput>>>,
    records: RefCell<Vec<(i32, String)>>,
    bytes: Cell<usize>,
}
impl Output {
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) fn write(
        &self,
        host: Option<&dyn FrameworkHost>,
        fd: i32,
        text: &str,
    ) -> Result<usize, VmExit> {
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            return sink.write(fd, text);
        }
        if let Some(host) = host {
            return host.write_log(fd, text);
        }
        #[cfg(feature = "desktop-runtime")]
        {
            crate::upstream_print_sink(fd, text);
            Ok(text.len())
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            // A standalone reference VM retains real output for inspection.
            // A bound Horizon framework writes through its persistent log sink.
            const LIMIT: usize = 1 << 20;
            if text.len() > LIMIT {
                return Err(crate::err::vm_error("LOG_BUDGET", "1 MiB output record").into());
            }
            let mut records = self.records.borrow_mut();
            if self.bytes.get() + text.len() > LIMIT {
                records.clear();
                self.bytes.set(0);
            }
            records.push((fd, String::from(text)));
            self.bytes.set(self.bytes.get() + text.len());
            Ok(text.len())
        }
    }
}
impl Engine {
    pub fn bind_app_output(&mut self, sink: Rc<dyn AppOutput>) {
        self.output.sink.replace(Some(sink));
    }
    pub fn take_app_output(&mut self) -> Vec<(i32, String)> {
        self.output.bytes.set(0);
        core::mem::take(&mut *self.output.records.borrow_mut())
    }
}
