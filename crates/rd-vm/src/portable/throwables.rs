//! Minimal Throwable detail-message state in a budgeted, GC-scanned heap slot.
//! The slot is not a valid DEX field name, so subclass fields cannot alias it.
use crate::{err, Engine, ObjRef, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
};
const CLASS: &str = "Ljava/lang/Throwable;";
const SLOT: &str = "java.lang.Throwable#detailMessage";

pub(crate) fn is_host_type(vm: &Engine, class: &str) -> bool {
    vm.cp.find_class(class).is_none() && vm.cp.is_subtype(class, CLASS)
}
pub(crate) fn attach(vm: &mut Engine, object: ObjRef) -> Result<(), VmExit> {
    set(vm, object, Value::Null)
}
fn set(vm: &mut Engine, object: ObjRef, value: Value) -> Result<(), VmExit> {
    err::put_field_result(vm.heap.put_field(object, SLOT, value))
}
fn stored(vm: &Engine, object: ObjRef) -> Result<Value, VmExit> {
    let class = vm.heap.class_of(object)?;
    if !vm.cp.is_subtype(class, CLASS) {
        return Err(err::vm_error(
            "VM_TYPE_ERROR",
            "Throwable message receiver is not Throwable",
        )
        .into());
    }
    let value = vm.heap.get_field(object, SLOT)?;
    if let Some(reference) = value.as_ref()? {
        vm.heap.as_str(reference)?;
    }
    Ok(value)
}
pub(crate) fn message(vm: &Engine, object: ObjRef) -> Result<Option<String>, VmExit> {
    stored(vm, object)?
        .as_ref()?
        .map(|r| vm.heap.as_str(r).map(ToString::to_string))
        .transpose()
        .map_err(Into::into)
}
pub(crate) fn materialize(
    vm: &mut Engine,
    class: &str,
    message: Option<&str>,
) -> Result<ObjRef, VmExit> {
    let object = vm
        .heap
        .alloc_instance(class.to_string(), vec![])
        .map_err(err::oom_throwable)?;
    let message = match message {
        Some(text) => Value::Obj(
            vm.heap
                .alloc_string(text.to_string())
                .map_err(err::oom_throwable)?,
        ),
        None => Value::Null,
    };
    set(vm, object, message)?;
    Ok(object)
}

// Constructors are not inherited. Specialized builtin exceptions must not be
// given nonexistent ()/(String) constructors just because they are Throwable.
fn constructor(class: &str, string: bool) -> bool {
    match class {
        "Ljava/lang/Throwable;"
        | "Ljava/lang/Exception;"
        | "Ljava/lang/RuntimeException;"
        | "Ljava/lang/Error;"
        | "Ljava/lang/LinkageError;"
        | "Ljava/lang/VirtualMachineError;"
        | "Ljava/lang/ArithmeticException;"
        | "Ljava/lang/NullPointerException;"
        | "Ljava/lang/ClassCastException;"
        | "Ljava/lang/IndexOutOfBoundsException;"
        | "Ljava/lang/ArrayIndexOutOfBoundsException;"
        | "Ljava/lang/StringIndexOutOfBoundsException;"
        | "Ljava/lang/NegativeArraySizeException;"
        | "Ljava/lang/IllegalStateException;"
        | "Ljava/lang/IllegalArgumentException;"
        | "Ljava/lang/IllegalMonitorStateException;"
        | "Ljava/lang/NumberFormatException;"
        | "Ljava/lang/ArrayStoreException;"
        | "Ljava/lang/UnsupportedOperationException;"
        | "Ljava/lang/SecurityException;"
        | "Ljava/lang/IllegalThreadStateException;"
        | "Ljava/lang/StackOverflowError;"
        | "Ljava/lang/OutOfMemoryError;"
        | "Ljava/lang/InternalError;"
        | "Ljava/lang/UnknownError;"
        | "Ljava/lang/ClassFormatError;"
        | "Ljava/lang/UnsupportedClassVersionError;"
        | "Ljava/lang/NoClassDefFoundError;"
        | "Ljava/lang/VerifyError;"
        | "Ljava/lang/UnsatisfiedLinkError;"
        | "Ljava/lang/NoSuchMethodError;"
        | "Ljava/lang/NoSuchFieldError;"
        | "Ljava/lang/AbstractMethodError;"
        | "Ljava/lang/IncompatibleClassChangeError;"
        | "Ljava/lang/IllegalAccessError;"
        | "Ljava/lang/InstantiationError;"
        | "Ljava/lang/ExceptionInInitializerError;"
        | "Ljava/lang/ClassNotFoundException;"
        | "Ljava/lang/ReflectiveOperationException;"
        | "Ljava/lang/InstantiationException;"
        | "Ljava/lang/IllegalAccessException;"
        | "Ljava/lang/NoSuchMethodException;"
        | "Ljava/lang/NoSuchFieldException;"
        | "Ljava/lang/InterruptedException;"
        | "Ljava/lang/CloneNotSupportedException;"
        | "Ljava/io/IOException;"
        | "Ljava/io/FileNotFoundException;"
        | "Ljava/io/EOFException;"
        | "Ljava/io/UnsupportedEncodingException;"
        | "Ljava/io/UTFDataFormatException;"
        | "Ljava/io/InterruptedIOException;"
        | "Ljava/util/NoSuchElementException;"
        | "Ljava/util/ConcurrentModificationException;"
        | "Ljava/util/concurrent/CancellationException;"
        | "Ljava/util/concurrent/TimeoutException;"
        | "Ljava/util/concurrent/ExecutionException;"
        | "Landroid/util/AndroidException;"
        | "Landroid/database/SQLException;"
        | "Landroid/database/sqlite/SQLiteException;"
        | "Landroid/database/sqlite/SQLiteConstraintException;"
        | "Landroid/database/sqlite/SQLiteCantOpenDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDiskIOException;"
        | "Landroid/database/sqlite/SQLiteFullException;"
        | "Landroid/database/sqlite/SQLiteDatabaseCorruptException;"
        | "Landroid/database/sqlite/SQLiteDatabaseLockedException;"
        | "Landroid/database/sqlite/SQLiteReadOnlyDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDoneException;"
        | "Landroid/database/sqlite/SQLiteBlobTooBigException;" => true,
        "Ljava/lang/ThreadDeath;"
        | "Ljava/lang/AssertionError;"
        | "Ljava/util/EmptyStackException;"
        | "Ljava/util/IllegalFormatException;"
        | "Ljava/nio/BufferOverflowException;"
        | "Ljava/nio/BufferUnderflowException;"
        | "Ljava/nio/InvalidMarkException;" => !string,
        "Ljava/time/DateTimeException;" | "Ljava/util/ServiceConfigurationError;" => string,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn call_instance(
    vm: &mut Engine,
    receiver_class: &str,
    declaring: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let runtime = vm.heap.class_of(receiver)?;
    if !vm.cp.is_subtype(runtime, CLASS) {
        return Ok(None);
    }
    let runtime = runtime.to_string();
    if name == "<init>" {
        if !is_host_type(vm, declaring) {
            return Ok(None);
        }
        let string = sig == "(Ljava/lang/String;)V";
        if (sig != "()V" && !string) || !constructor(declaring, string) {
            return Err(err::vm_error(
                "API_UNAVAILABLE",
                format!("Throwable constructor {declaring}->{name}{sig}"),
            )
            .into());
        }
        let value = if string {
            let value = args
                .first()
                .ok_or_else(|| err::vm_error("VM_TYPE_ERROR", "missing Throwable message"))?;
            match value.as_ref()? {
                Some(r) => {
                    vm.heap.as_str(r)?;
                    Value::Obj(r)
                }
                None => Value::Null,
            }
        } else {
            Value::Null
        };
        set(vm, receiver, value)?;
        return Ok(Some(Value::Null));
    }
    // Normal virtual calls retain real DEX overrides. invoke-super supplies
    // the actual superclass lookup start, preserving its overrides as well.
    if vm.cp.resolve_method(receiver_class, name, sig).is_some() {
        return Ok(None);
    }
    let value = match (name, sig) {
        ("getMessage", "()Ljava/lang/String;") => stored(vm, receiver)?,
        ("getLocalizedMessage", "()Ljava/lang/String;") => {
            vm.invoke_instance(receiver, "getMessage", "()Ljava/lang/String;", &[])?
        }
        ("toString", "()Ljava/lang/String;") => {
            let localized =
                vm.invoke_instance(receiver, "getLocalizedMessage", "()Ljava/lang/String;", &[])?;
            let name = runtime
                .strip_prefix('L')
                .and_then(|name| name.strip_suffix(';'))
                .ok_or_else(|| {
                    err::vm_error("VM_TYPE_ERROR", "invalid Throwable class descriptor")
                })?
                .replace('/', ".");
            let text = match localized.as_ref()? {
                None => name,
                Some(r) => format!("{name}: {}", vm.heap.as_str(r)?),
            };
            Value::Obj(crate::intrinsics::alloc_string(vm, text)?)
        }
        _ if matches!(
            name,
            "getCause"
                | "initCause"
                | "fillInStackTrace"
                | "getStackTrace"
                | "setStackTrace"
                | "printStackTrace"
                | "addSuppressed"
                | "getSuppressed"
        ) =>
        {
            return Err(err::vm_error("API_UNAVAILABLE", format!("Throwable.{name}{sig}")).into());
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
