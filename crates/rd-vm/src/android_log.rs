//! Log dispatch extracted from RustyDroid's android_text::log_intrinsic.
//! Both VM configurations use this formatter; output and tag policy belong to
//! their adapters. Throwable rendering retains upstream's summary-only scope,
//! not complete Android stack traces or wtf termination/handler semantics.
use crate::{err, Engine, ObjRef, Value, VmExit};
use alloc::{format, string::String};
const CLASS: &str = "Landroid/util/Log;";
pub fn metadata(class: &str, name: &str, sig: &str) -> bool {
    class == CLASS
        && matches!(
            (name, sig),
            (
                "v" | "d" | "i" | "w" | "e" | "wtf",
                "(Ljava/lang/String;Ljava/lang/String;)I"
                    | "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/Throwable;)I",
            ) | ("w" | "wtf", "(Ljava/lang/String;Ljava/lang/Throwable;)I")
                | ("println", "(ILjava/lang/String;Ljava/lang/String;)I")
                | ("isLoggable", "(Ljava/lang/String;I)Z")
        )
}
/// Platform adapters supply actual output and object rendering. Arguments are
/// rooted for nested DEX toString callbacks, including callback GC/exception.
#[allow(clippy::too_many_arguments)]
pub fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
    render: fn(&mut Engine, ObjRef) -> Result<String, VmExit>,
    loggable: fn(i32) -> bool,
    mut sink: impl FnMut(&str) -> Result<usize, VmExit>,
) -> Result<Option<Value>, VmExit> {
    if !metadata(class, name, sig) {
        return Ok(None);
    }
    let expected = if name == "println"
        || sig == "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/Throwable;)I"
    {
        3
    } else {
        2
    };
    if args.len() != expected {
        return Err(err::vm_error("VM_TYPE_ERROR", "android Log argument count").into());
    }
    let mark = vm.heap.gc_scope_depth();
    vm.heap.gc_host_enter(false);
    for value in args {
        if let Some(reference) = value.as_ref().ok().flatten() {
            vm.heap.pin_allocation(reference);
        }
    }
    let result = (|| {
        fn text(
            vm: &mut Engine,
            value: &Value,
            render: fn(&mut Engine, ObjRef) -> Result<String, VmExit>,
        ) -> Result<String, VmExit> {
            match value.as_ref()? {
                Some(reference) => render(vm, reference),
                None => Ok(String::new()),
            }
        }
        if name == "isLoggable" {
            text(vm, &args[0], render)?;
            return Ok(Some(Value::Int(i32::from(loggable(args[1].as_int()?)))));
        }
        let line = if name == "println" {
            let priority = args[0].as_int()?;
            let tag = text(vm, &args[1], render)?;
            let message = text(vm, &args[2], render)?;
            let level = ['V', 'V', 'V', 'D', 'I', 'W', 'E', 'A'][priority.clamp(0, 7) as usize];
            format!("{level}/{tag}: {message}\n")
        } else {
            let level = match name {
                "v" => 'V',
                "d" => 'D',
                "i" => 'I',
                "w" => 'W',
                "e" => 'E',
                _ => 'A',
            };
            let tag = text(vm, &args[0], render)?;
            let mut message = String::new();
            for value in &args[1..] {
                if value.as_ref()?.is_some() {
                    let piece = text(vm, value, render)?;
                    if !message.is_empty() {
                        message.push('\n');
                    }
                    message.push_str(&piece);
                }
            }
            format!("{level}/{tag}: {message}\n")
        };
        if line.len() > 64 << 10 {
            return Err(err::vm_error("LOG_BUDGET", "64 KiB per log entry").into());
        }
        let written = sink(&line)?;
        if written != line.len() {
            return Err(err::vm_error("SHORT_WRITE", "android Log sink").into());
        }
        Ok(Some(Value::Int(written as i32)))
    })();
    let returned = match &result {
        Err(VmExit::Exception(exception)) => exception.obj,
        _ => None,
    };
    #[cfg(feature = "desktop-runtime")]
    {
        vm.heap.gc_scope_restore(mark);
        if let Some(reference) = returned {
            vm.heap.gc_pin_return(reference);
        }
    }
    #[cfg(not(feature = "desktop-runtime"))]
    vm.heap.gc_scope_restore_return(mark, returned);
    result
}
