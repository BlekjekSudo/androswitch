//! Pending Java exception diagnostics shared with the device JNI table.
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use rd_vm::{Engine, Throwable, Value, VmExit};

#[derive(Default)]
pub struct PendingException {
    pub pending: Option<Throwable>,
    pub trace: Vec<String>,
}
impl PendingException {
    pub fn set(&mut self, vm: &Engine, exception: Throwable) {
        self.trace = vm.diagnostic_stack();
        self.pending = Some(exception);
    }
    /// The caller removes this record from the live thread map first: native
    /// callbacks during virtual toString/logging must see no old pending error.
    /// Actual captured DEX frames are printed; unavailable Java cause/suppressed
    /// and StackTraceElement contracts are not synthesized here.
    pub fn describe(&mut self, vm: &mut Engine) -> Result<(), VmExit> {
        let Some(exception) = self.pending.take() else {
            return Ok(());
        };
        let trace = core::mem::take(&mut self.trace);
        let object = vm.materialize_throwable(&exception)?;
        let text = vm.invoke_instance(object, "toString", "()Ljava/lang/String;", &[])?;
        let mut text = vm
            .heap
            .as_str(text.as_ref()?.ok_or_else(|| {
                rd_vm::err::vm_error(
                    "JNI_EXCEPTION_DESCRIPTION",
                    "Throwable.toString returned null",
                )
            })?)?
            .to_string();
        for frame in trace {
            text.push_str(&format!("\n  at {frame}"));
        }
        vm.invoke_static(
            "Landroid/util/Log;",
            "w",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[
                Value::StrPlaceholder("System.err".into()),
                Value::StrPlaceholder(text),
            ],
        )?;
        Ok(())
    }
}
