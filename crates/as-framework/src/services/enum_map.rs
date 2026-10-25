//! EnumMap type policy over the existing Collections map store. No second
//! map engine or detached views: unimplemented view/iterator APIs stay absent.
use super::*;
const CLASS: &str = "Ljava/util/EnumMap;";
const KEY_TYPE: &str = "java.util.EnumMap#keyType";

pub(super) fn metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<rd_vm::framework_host::HostMethodKind> {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance};
    if class != CLASS {
        return None;
    }
    match (name, sig) {
        ("<init>", "(Ljava/lang/Class;)V" | "(Ljava/util/EnumMap;)V" | "(Ljava/util/Map;)V") => {
            Some(Constructor)
        }
        ("size", "()I")
        | ("isEmpty", "()Z")
        | ("clear", "()V")
        | ("get" | "remove", "(Ljava/lang/Object;)Ljava/lang/Object;")
        | ("containsKey" | "containsValue", "(Ljava/lang/Object;)Z")
        | (
            "put",
            "(Ljava/lang/Enum;Ljava/lang/Object;)Ljava/lang/Object;"
            | "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        )
        | ("putAll", "(Ljava/util/Map;)V") => Some(Instance),
        _ => None,
    }
}
fn descriptor(vm: &Engine, class: ObjRef) -> Result<String> {
    let name = obj(&vm.heap.get_field(class, "class_name")?)?;
    Ok(vm.heap.as_str(name)?.to_string())
}
fn valid_key(vm: &Engine, value: &Value, key_type: &str) -> Result<bool> {
    let Some(reference) = value.as_ref()? else {
        return Ok(false);
    };
    let actual = vm.heap.class_of(reference)?;
    Ok(actual == key_type || vm.cp.superclass_of(actual).as_deref() == Some(key_type))
}
impl Collections {
    fn map_snapshot(&self, reference: ObjRef) -> Result<Vec<(Value, Value)>> {
        match self.objects.borrow().get(&reference) {
            Some(Container::Map(entries)) => Ok(entries.clone()),
            _ => Err(error(
                "API_UNAVAILABLE",
                "Map source requires an implemented entry traversal",
            )),
        }
    }
    pub(super) fn enum_map_call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if metadata(CLASS, name, sig).is_none() {
            return Ok(None);
        }
        let pin = self.cow_pin(receiver, args)?;
        if name == "<init>" {
            let source = obj(&args[0])?;
            let (class, entries) = if sig == "(Ljava/lang/Class;)V" {
                (source, Vec::new())
            } else {
                let entries = self.map_snapshot(source)?;
                let class = if vm.cp.is_subtype(vm.heap.class_of(source)?, CLASS) {
                    obj(&vm.heap.get_field(source, KEY_TYPE)?)?
                } else {
                    let key = entries
                        .first()
                        .ok_or_else(|| {
                            Throwable::new(
                                "Ljava/lang/IllegalArgumentException;",
                                "empty Map has no enum key type",
                            )
                        })?
                        .0
                        .clone();
                    let key = obj(&key)?;
                    if !vm.cp.is_subtype(vm.heap.class_of(key)?, "Ljava/lang/Enum;") {
                        return Err(Throwable::new(
                            "Ljava/lang/ClassCastException;",
                            "Map key is not an enum",
                        )
                        .into());
                    }
                    obj(&vm.invoke_instance(
                        key,
                        "getDeclaringClass",
                        "()Ljava/lang/Class;",
                        &[],
                    )?)?
                };
                (class, entries)
            };
            pin.extend(
                core::iter::once(class).chain(
                    entries
                        .iter()
                        .flat_map(|(k, v)| [k, v])
                        .filter_map(|v| v.as_ref().ok().flatten()),
                ),
            )?;
            let desc = descriptor(vm, class)?;
            if !vm
                .cp
                .find_class(&desc)
                .is_some_and(|(_, def)| def.access_flags & 0x4000 != 0)
                || vm.cp.superclass_of(&desc).as_deref() != Some("Ljava/lang/Enum;")
            {
                // Android's EnumMap obtains the enum universe; a non-enum
                // Class has no such universe (same NPE as the JVM).
                return Err(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Class has no enum constants",
                )
                .into());
            }
            vm.ensure_initialized(&desc)?;
            self.budget(entries.len().saturating_mul(2))?;
            write(vm, receiver, KEY_TYPE, Value::Obj(class))?;
            self.objects
                .borrow_mut()
                .insert(receiver, Container::Map(Vec::new()));
            for (key, value) in entries {
                self.enum_map_call(
                    vm,
                    "put",
                    "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                    receiver,
                    &[key, value],
                )?;
            }
            return Ok(Some(Value::Null));
        }
        let class = obj(&vm.heap.get_field(receiver, KEY_TYPE)?)?;
        let desc = descriptor(vm, class)?;
        if name == "putAll" {
            let source = obj(&args[0])?;
            let entries = self.map_snapshot(source)?;
            pin.extend(
                entries
                    .iter()
                    .flat_map(|(k, v)| [k, v])
                    .filter_map(|v| v.as_ref().ok().flatten()),
            )?;
            for (key, value) in entries {
                self.enum_map_call(
                    vm,
                    "put",
                    "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                    receiver,
                    &[key, value],
                )?;
            }
            return Ok(Some(Value::Null));
        }
        if name == "put" {
            obj(&args[0])?;
            if !valid_key(vm, &args[0], &desc)? {
                return Err(Throwable::new(
                    "Ljava/lang/ClassCastException;",
                    "wrong EnumMap key type",
                )
                .into());
            }
        } else if matches!(name, "get" | "remove" | "containsKey")
            && !valid_key(vm, &args[0], &desc)?
        {
            return Ok(Some(if name == "containsKey" {
                Value::Int(0)
            } else {
                Value::Null
            }));
        }
        let sig = if name == "put" {
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
        } else {
            sig
        };
        self.regular_call(vm, name, sig, receiver, args)
    }
}
