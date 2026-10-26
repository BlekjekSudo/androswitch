//! Unhandled intrinsic lookup returns `None` so the shared interpreter can
//! resolve the actual DEX method or report its exact unresolved descriptor.
//! This is not a successful implementation of any Android service.
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::string::{String, ToString};

pub fn alloc_string(vm: &mut Engine, text: String) -> Result<ObjRef, VmExit> {
    #[cfg(feature = "upstream-runtime")]
    if vm.framework_host().is_none() {
        return crate::upstream_intrinsics::alloc_string(vm, text);
    }
    if let Some(object) = vm.portable.strings.get(&text) {
        return Ok(*object);
    }
    let object = vm.heap.alloc_string(text.clone())?;
    vm.portable.strings.insert(text, object);
    Ok(object)
}

pub fn call_native_loader(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == "Ljava/lang/System;"
        && matches!(name, "loadLibrary" | "load")
        && sig == "(Ljava/lang/String;)V"
    {
        let text = vm
            .heap
            .as_str(args[0].as_ref()?.ok_or_else(|| {
                crate::Throwable::new("Ljava/lang/NullPointerException;", "library name")
            })?)?
            .to_string();
        let backend = vm.native_host().ok_or_else(|| {
            crate::err::vm_error(
                "NATIVE_BACKEND_UNAVAILABLE",
                "System.loadLibrary requires native backend",
            )
        })?;
        backend.load(vm, &text, name == "load")?;
        return Ok(Some(Value::Null));
    }
    Ok(None)
}
pub fn call_static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    #[cfg(feature = "upstream-runtime")]
    if vm.framework_host().is_none() {
        return crate::upstream_intrinsics::call_static_intrinsic(vm, class, name, sig, args);
    }
    if let Some(value) = crate::arrays::static_intrinsic(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::collection_factories::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::collections::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::jbitset::static_intrinsic(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::boxes::static_intrinsic(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    #[cfg(feature = "std")]
    if let Some(value) = crate::numerics::static_intrinsic(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::monitor::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if class == "Ljava/lang/System;" && name == "gc" && sig == "()V" {
        vm.gc();
        return Ok(Some(Value::Null));
    }
    if let Some(value) = crate::portable::enums::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::format::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::file::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::java_file::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    #[cfg(feature = "std")]
    if let Some(value) = crate::shared_text::call_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    if let Some(host) = vm.framework_host() {
        return host.call_static(vm, class, name, sig, args);
    }
    Ok(None)
}
#[allow(clippy::too_many_arguments)]
pub fn call_instance_intrinsic_mref(
    vm: &mut Engine,
    receiver_class: &str,
    declaring_class: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    #[cfg(feature = "upstream-runtime")]
    if vm.framework_host().is_none() {
        return crate::upstream_intrinsics::call_instance_intrinsic_mref(
            vm,
            receiver_class,
            declaring_class,
            name,
            sig,
            receiver,
            args,
        );
    }
    if let Some(value) = crate::jbitset::instance_intrinsic(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    if let Some(value) =
        crate::boxes::instance_intrinsic(vm, receiver_class, name, sig, receiver, args)?
    {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::throwables::call_instance(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    if let Some(value) =
        crate::portable::monitor::call_instance(vm, declaring_class, name, sig, receiver)?
    {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::atomic_reference::call_instance(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::atomic_number::call_instance(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::atomic::call_instance(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    if let Some(value) =
        crate::portable::classes::call_instance(vm, declaring_class, name, sig, receiver)?
    {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::enums::call_instance(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    // These text classes are final. Virtual Object/CharSequence calls must
    // select the same implementation and heap buffer as direct calls.
    // Keep declaring-class dispatch for file constructors and other receivers.
    let value_class = if matches!(
        receiver_class,
        "Ljava/lang/String;" | "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;"
    ) {
        receiver_class
    } else {
        declaring_class
    };
    #[cfg(feature = "std")]
    if let Some(value) = crate::shared_text::call(vm, value_class, name, sig, receiver, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::portable::file::call(vm, value_class, name, sig, receiver, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::java_file::call(vm, value_class, name, sig, receiver, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::java_stream::call(vm, value_class, name, sig, receiver, args)? {
        return Ok(Some(value));
    }
    #[cfg(any(feature = "std", feature = "upstream-runtime"))]
    if let Some(value) = crate::atomic_file::call(vm, value_class, name, sig, receiver, args)? {
        return Ok(Some(value));
    }
    #[cfg(any(feature = "std", feature = "upstream-runtime"))]
    if let Some(value) = crate::java_io::call(
        vm,
        receiver_class,
        declaring_class,
        name,
        sig,
        receiver,
        args,
    )? {
        return Ok(Some(value));
    }
    Ok(None)
}

pub(crate) use crate::java_values::object_to_string;
#[cfg(feature = "upstream-runtime")]
pub use crate::upstream_intrinsics::{
    call_instance_intrinsic, exception, string_arg, values_equal,
};
#[cfg(feature = "upstream-runtime")]
pub(crate) use crate::upstream_intrinsics::{context_impl, host_print, is_throwable_class};

#[cfg(feature = "upstream-runtime")]
pub use crate::upstream_intrinsics::{
    dispatch_display_event, EVENT_FLAG_DISPLAY_ADDED, EVENT_FLAG_DISPLAY_CHANGED,
    EVENT_FLAG_DISPLAY_REMOVED,
};
