//! Production JNI object/reference operations shared with host contracts.
//! A jclass is a reference to the VM's canonical java.lang.Class object;
//! member IDs remain opaque metadata and never become Java object references.
use alloc::{format, string::String, vec::Vec};
use as_native::jni_refs::References;
use core::cell::RefCell;
use rd_jni::Handle;
use rd_vm::{Engine, ObjRef, Throwable, Value, VmExit};

type Result<T> = core::result::Result<T, VmExit>;
fn error(code: &str, cause: impl Into<String>) -> VmExit {
    rd_vm::err::vm_error(code, cause).into()
}
fn table_error(e: as_native::Error) -> VmExit {
    error(&e.code, format!("{}: {}", e.operation, e.cause))
}
pub fn object(refs: &RefCell<References>, thread: u64, token: u64) -> Result<ObjRef> {
    match refs.borrow().get(thread, token).map_err(table_error)? {
        Handle::Object(object) => Ok(*object),
        _ => Err(error("JNI_REFERENCE_KIND", "expected object reference")),
    }
}
pub fn local_class(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    descriptor: &str,
) -> Result<u64> {
    // Release every reference-table borrow before touching the VM heap.
    let class = vm.device_class_object(descriptor)?;
    refs.borrow_mut()
        .local(thread, Handle::Object(class))
        .map_err(table_error)
}
pub fn class_descriptor(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    token: u64,
) -> Result<String> {
    let class = object(refs, thread, token)?;
    if vm.heap.class_of(class)? != "Ljava/lang/Class;" {
        return Err(error(
            "JNI_REFERENCE_KIND",
            "expected java.lang.Class reference",
        ));
    }
    let name = vm
        .heap
        .get_field(class, "class_name")?
        .as_ref()?
        .ok_or_else(|| {
            error(
                "JNI_REFERENCE_KIND",
                "Class reference lacks represented descriptor",
            )
        })?;
    let descriptor: String = vm.heap.as_str(name)?.into();
    if vm.device_class_object(&descriptor)? != class {
        return Err(error(
            "JNI_REFERENCE_KIND",
            "expected canonical Class object",
        ));
    }
    Ok(descriptor)
}
pub fn from_bits(refs: &RefCell<References>, thread: u64, bits: u64, kind: &str) -> Result<Value> {
    Ok(match kind {
        "V" => Value::Null,
        "J" => Value::Long(bits as i64),
        "F" => Value::Float(f32::from_bits(bits as u32)),
        "D" => Value::Double(f64::from_bits(bits)),
        t if t.starts_with('L') || t.starts_with('[') => {
            if bits == 0 {
                Value::Null
            } else {
                Value::Obj(object(refs, thread, bits)?)
            }
        }
        "Z" => Value::Int((bits != 0) as i32),
        "B" => Value::Int(bits as i8 as i32),
        "C" => Value::Int(bits as u16 as i32),
        "S" => Value::Int(bits as i16 as i32),
        _ => Value::Int(bits as i32),
    })
}
pub fn to_bits(refs: &RefCell<References>, thread: u64, value: &Value, kind: &str) -> Result<u64> {
    if kind.starts_with('L') || kind.starts_with('[') {
        return match value.as_ref()? {
            Some(object) => refs
                .borrow_mut()
                .local(thread, Handle::Object(object))
                .map_err(table_error),
            None => Ok(0),
        };
    }
    crate::native_probe::bits(value).map_err(table_error)
}

/// Scalar reference operations use the same opcode numbers as the C bridge.
/// `name` is decoded before this safe boundary; raw pointer transport stays C/FFI.
pub fn dispatch(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    op: u32,
    a: u64,
    b: u64,
    name: Option<&str>,
) -> Result<u64> {
    match op {
        1 => {
            let name = name.ok_or_else(|| error("JNI_ARGUMENT", "FindClass name"))?;
            let class = if name.starts_with('[') {
                name.into()
            } else {
                format!("L{name};")
            };
            if !crate::jni_constructor::class_known(vm, &class) {
                return Err(Throwable::new("Ljava/lang/NoClassDefFoundError;", class).into());
            }
            local_class(vm, refs, thread, &class)
        }
        5 => {
            let class: String = vm.heap.class_of(object(refs, thread, a)?)?.into();
            local_class(vm, refs, thread, &class)
        }
        13 | 14 => {
            if a != 0 {
                object(refs, thread, a)?;
            }
            refs.borrow_mut()
                .duplicate(thread, a, op == 14)
                .map_err(table_error)
        }
        15 | 16 => {
            if a != 0 {
                object(refs, thread, a)?;
            }
            refs.borrow_mut()
                .delete(thread, a, op == 16)
                .map_err(table_error)?;
            Ok(0)
        }
        17 => {
            let a = if a == 0 {
                None
            } else {
                Some(object(refs, thread, a)?)
            };
            let b = if b == 0 {
                None
            } else {
                Some(object(refs, thread, b)?)
            };
            Ok((a == b) as u64)
        }
        18 => {
            refs.borrow_mut().push(thread).map_err(table_error)?;
            Ok(0)
        }
        19 => {
            if a != 0 {
                object(refs, thread, a)?;
            }
            refs.borrow_mut().pop(thread, a).map_err(table_error)
        }
        31 => {
            if a == 0 {
                return Ok(0);
            }
            let table = refs.borrow();
            if !matches!(
                table.get(thread, a).map_err(table_error)?,
                Handle::Object(_)
            ) {
                // A member ID has never been an object reference. Report
                // JNIInvalidRefType rather than treating it as global.
                return Ok(0);
            }
            table.kind(thread, a).map_err(table_error)
        }
        33 => {
            let class = class_descriptor(vm, refs, thread, b)?;
            if a == 0 {
                return Ok(1);
            }
            let actual = vm.heap.class_of(object(refs, thread, a)?)?;
            Ok(vm.cp.is_subtype(actual, &class) as u64)
        }
        _ => Err(error("JNI_REFERENCE_OPERATION", format!("opcode {op}"))),
    }
}

/// Decoded invocation owns metadata/values, never a reference-table borrow.
/// The real Session and host contracts execute this same safe dispatch path.
pub struct Invocation {
    class: String,
    name: String,
    signature: String,
    result: String,
    receiver: Option<ObjRef>,
    arguments: Vec<Value>,
}
impl Invocation {
    pub fn decode(
        refs: &RefCell<References>,
        thread: u64,
        receiver: u64,
        method: u64,
        is_static: bool,
        arguments: &[u64],
    ) -> Result<Self> {
        let (class, name, signature, method_static) =
            match refs.borrow().get(thread, method).map_err(table_error)? {
                Handle::Method {
                    class,
                    name,
                    sig,
                    is_static,
                } => (class.clone(), name.clone(), sig.clone(), *is_static),
                _ => return Err(error("JNI_REFERENCE_KIND", "expected method ID")),
            };
        if is_static != method_static {
            return Err(error(
                "JNI_METHOD_KIND",
                "static/instance invocation mismatch",
            ));
        }
        let (types, result) =
            rd_jni::descriptor(&signature).map_err(|e| error(&e.code, e.cause))?;
        if arguments.len() != types.len() {
            return Err(error("JNI_SIGNATURE", "callback argument count"));
        }
        let arguments = arguments
            .iter()
            .zip(&types)
            .map(|(bits, ty)| from_bits(refs, thread, *bits, ty))
            .collect::<Result<Vec<_>>>()?;
        let receiver = if is_static {
            None
        } else {
            Some(object(refs, thread, receiver)?)
        };
        Ok(Self {
            class,
            name,
            signature,
            result,
            receiver,
            arguments,
        })
    }
    pub fn invoke(self, vm: &mut Engine, refs: &RefCell<References>, thread: u64) -> Result<u64> {
        let value = if let Some(receiver) = self.receiver {
            vm.invoke_instance(receiver, &self.name, &self.signature, &self.arguments)?
        } else {
            vm.invoke_static(&self.class, &self.name, &self.signature, &self.arguments)?
        };
        to_bits(refs, thread, &value, &self.result)
    }
}
