//! Bounded DEX field reflection. Field IDs referenced by bytecode are not
//! declarations. Platform fields and ambiguous name-only instance storage are
//! rejected rather than reported as absent or read from an unrelated slot.
use super::*;
use core::cell::Cell;

const FIELD: &str = "Ljava/lang/reflect/Field;";
const ACCESSIBLE: &str = "Ljava/lang/reflect/AccessibleObject;";
const MAX_FIELDS: usize = 4096;
const MAX_BYTES: usize = 4 << 20;
type Definition = rd_vm::classpath::DeclaredFieldInfo;
#[derive(Clone)]
struct Entry {
    definition: Definition,
    owner_class: ObjRef,
    type_class: ObjRef,
    accessible: bool,
}
impl Entry {
    fn bytes(&self) -> usize {
        core::mem::size_of::<Self>()
            + self.definition.0.len()
            + self.definition.1.len()
            + self.definition.2.len()
    }
}
#[derive(Default)]
pub(in super::super) struct Fields {
    objects: RefCell<BTreeMap<ObjRef, Entry>>,
    scopes: RefCell<BTreeMap<u64, Vec<ObjRef>>>,
    serial: Cell<u64>,
}
struct Scope<'a>(&'a Fields, u64);
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.0.scopes.borrow_mut().remove(&self.1);
    }
}
fn unavailable(message: impl Into<String>) -> VmExit {
    error("API_UNAVAILABLE", message)
}
fn exception(vm: &mut Engine, class: &str, message: impl Into<String>) -> VmExit {
    // Materialize before frame diagnostics append transport stack text, so the
    // actual Java exception's nullable message remains independent of tracing.
    let throwable = Throwable::new(class, message);
    match vm.materialize_throwable(&throwable) {
        Ok(r) => Throwable {
            obj: Some(r),
            ..throwable
        }
        .into(),
        Err(failure) => failure,
    }
}
fn missing(vm: &mut Engine, owner: &str, name: &str) -> VmExit {
    exception(
        vm,
        "Ljava/lang/NoSuchFieldException;",
        format!("{owner}.{name}"),
    )
}
fn own(vm: &mut Engine, class: &str) -> Result<Vec<Definition>> {
    if let Some(fields) = vm.cp.declared_fields_info_checked(class)? {
        return Ok(fields);
    }
    if matches!(class, "V" | "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D")
        || class.starts_with('[')
        || class == "Ljava/lang/Object;"
    {
        return Ok(Vec::new());
    }
    Err(unavailable(format!("declared field catalog: {class}")))
}
fn lookup(
    vm: &mut Engine,
    class: &str,
    name: &str,
    public: bool,
    visited: &mut Vec<String>,
) -> Result<Option<Definition>> {
    if visited.len() >= 128 {
        return Err(error("REFLECTION_BUDGET", "128 field lookup classes"));
    }
    if visited.iter().any(|old| old == class) {
        return Ok(None);
    }
    visited.push(class.into());
    // ART API26 searches instance declarations before static declarations.
    let mut definitions = own(vm, class)?;
    definitions.sort_by_key(|field| field.4);
    if let Some(field) = definitions
        .into_iter()
        .find(|field| field.1 == name && (!public || field.3 & 1 != 0))
    {
        return Ok(Some(field));
    }
    if !public || class.len() == 1 || class.starts_with('[') {
        return Ok(None);
    }
    for interface in vm.cp.interfaces_of(class) {
        if let Some(field) = lookup(vm, &interface, name, true, visited)? {
            return Ok(Some(field));
        }
    }
    if let Some(parent) = vm.cp.superclass_of(class) {
        return lookup(vm, &parent, name, true, visited);
    }
    Ok(None)
}
fn package(class: &str) -> &str {
    class.rsplit_once('/').map_or("", |(package, _)| package)
}
fn accessible(vm: &mut Engine, entry: &Entry, receiver: Option<ObjRef>) -> Result<()> {
    if entry.accessible {
        return Ok(());
    }
    let (owner, _, _, flags, is_static) = &entry.definition;
    let owner_flags = vm
        .cp
        .find_class(owner)
        .map(|(_, def)| def.access_flags)
        .ok_or_else(|| unavailable(format!("field declaring class: {owner}")))?;
    if flags & 1 != 0 && owner_flags & 1 != 0 {
        return Ok(());
    }
    let caller = vm
        .active_declaring_class()?
        .ok_or_else(|| unavailable("nonpublic Field access without an active DEX caller"))?;
    let same_package = package(&caller) == package(owner);
    let class_allowed = owner_flags & 1 != 0 || same_package;
    let field_allowed = if flags & 1 != 0 {
        true
    } else if flags & 2 != 0 {
        caller == *owner
    } else if same_package {
        true
    } else if flags & 4 != 0 && vm.cp.is_subtype(&caller, owner) {
        *is_static
            || receiver.is_some_and(|r| {
                vm.heap
                    .class_of(r)
                    .is_ok_and(|actual| vm.cp.is_subtype(actual, &caller))
            })
    } else {
        false
    };
    if class_allowed && field_allowed {
        Ok(())
    } else {
        Err(exception(
            vm,
            "Ljava/lang/IllegalAccessException;",
            format!("{caller} cannot access {owner}.{}", entry.definition.1),
        ))
    }
}
fn value(
    fields: &Fields,
    field: ObjRef,
    vm: &mut Engine,
    entry: &Entry,
    target: &Value,
) -> Result<Value> {
    let (owner, name, ty, _, is_static) = &entry.definition;
    let receiver = if *is_static {
        // DEX/JVM can declare the same name with distinct descriptors. The
        // existing VM static map omits the descriptor, so admission must fail
        // before an initializer can alias those distinct storage locations.
        if own(vm, owner)?
            .iter()
            .filter(|field| field.4 && field.1 == *name)
            .count()
            > 1
        {
            return Err(unavailable(
                "same-name static fields require descriptor-qualified static storage",
            ));
        }
        // API26 ART does receiver/static-initialization before access checking.
        super::class_for_name::initialize_class(vm, owner)?;
        None
    } else {
        let Some(receiver) = target.as_ref()? else {
            return Err(exception(
                vm,
                "Ljava/lang/NullPointerException;",
                "null field receiver",
            ));
        };
        if !vm.cp.is_subtype(vm.heap.class_of(receiver)?, owner) {
            return Err(exception(
                vm,
                "Ljava/lang/IllegalArgumentException;",
                "field receiver type",
            ));
        }
        Some(receiver)
    };
    // Initialization may reenter Java and change this very Field's override.
    // Read it after CheckReceiver, matching ART, without retaining a borrow.
    let mut access_entry = entry.clone();
    access_entry.accessible = fields
        .objects
        .borrow()
        .get(&field)
        .ok_or_else(|| error("REFLECTION_STATE", "in-flight Field metadata lost"))?
        .accessible;
    accessible(vm, &access_entry, receiver)?;
    if *is_static {
        return vm
            .statics
            .get(&(owner.clone(), name.clone()))
            .cloned()
            .ok_or_else(|| error("REFLECTION_STATE", "initialized static field has no slot"));
    }
    let receiver = receiver.expect("instance receiver validated");
    // Existing portable object storage is name-keyed. Never confuse a hidden
    // base slot with a subclass slot, even if both have the same descriptor.
    let mut cursor = vm.heap.class_of(receiver)?.to_string();
    let mut count = 0;
    let mut done = false;
    for _ in 0..128 {
        if cursor == "Ljava/lang/Object;" {
            done = true;
            break;
        }
        count += own(vm, &cursor)?
            .into_iter()
            .filter(|field| !field.4 && field.1 == *name)
            .count();
        if count > 1 {
            return Err(unavailable(
                "hidden instance fields require owner-qualified object storage",
            ));
        }
        let Some(parent) = vm.cp.superclass_of(&cursor) else {
            done = true;
            break;
        };
        cursor = parent;
    }
    if !done {
        return Err(error("REFLECTION_BUDGET", "128 instance superclass fields"));
    }
    match vm.heap.get(receiver)? {
        rd_vm::heap::HeapObj::Instance { fields, .. } => Ok(fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| rd_vm::engine::default_for(ty))),
        _ => Err(error(
            "REFLECTION_STATE",
            "field receiver is not an instance",
        )),
    }
}
fn widen(vm: &mut Engine, source: &str, target: &str, value: Value) -> Result<Value> {
    let allowed = source == target
        || match source {
            "B" => matches!(target, "S" | "I" | "J" | "F" | "D"),
            "S" | "C" => matches!(target, "I" | "J" | "F" | "D"),
            "I" => matches!(target, "J" | "F" | "D"),
            "J" => matches!(target, "F" | "D"),
            "F" => target == "D",
            _ => false,
        };
    if !allowed {
        return Err(exception(
            vm,
            "Ljava/lang/IllegalArgumentException;",
            "field primitive conversion",
        ));
    }
    let value = match source {
        "Z" => Value::Int(i32::from(value.as_int()? != 0)),
        "B" => Value::Int(value.as_int()? as i8 as i32),
        "S" => Value::Int(value.as_int()? as i16 as i32),
        "C" => Value::Int(value.as_int()? as u16 as i32),
        "I" => Value::Int(value.as_int()?),
        "J" => Value::Long(value.as_long()?),
        "F" => Value::Float(value.as_float()?),
        "D" => Value::Double(value.as_double()?),
        _ => value,
    };
    Ok(match (target, value) {
        ("J", Value::Int(v)) => Value::Long(v as i64),
        ("F", Value::Int(v)) => Value::Float(v as f32),
        ("F", Value::Long(v)) => Value::Float(v as f32),
        ("D", Value::Int(v)) => Value::Double(v as f64),
        ("D", Value::Long(v)) => Value::Double(v as f64),
        ("D", Value::Float(v)) => Value::Double(v as f64),
        (_, v) => v,
    })
}
impl Fields {
    fn protect(&self, roots: Vec<ObjRef>) -> Result<Scope<'_>> {
        if self.scopes.borrow().len() >= 64 {
            return Err(error("REFLECTION_BUDGET", "64 field read scopes"));
        }
        let ticket = self
            .serial
            .get()
            .checked_add(1)
            .ok_or_else(|| error("REFLECTION_BUDGET", "field scope serial exhausted"))?;
        self.serial.set(ticket);
        self.scopes.borrow_mut().insert(ticket, roots);
        Ok(Scope(self, ticket))
    }
    pub(in super::super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        let implemented = (class == CLASS
            && matches!(name, "getDeclaredField" | "getField")
            && sig == "(Ljava/lang/String;)Ljava/lang/reflect/Field;")
            || (matches!(class, FIELD | ACCESSIBLE)
                && matches!(
                    (name, sig),
                    ("setAccessible", "(Z)V") | ("isAccessible", "()Z")
                ))
            || (class == "Ljava/lang/reflect/Member;"
                && matches!(
                    (name, sig),
                    ("getName", "()Ljava/lang/String;")
                        | ("getDeclaringClass", "()Ljava/lang/Class;")
                        | ("getModifiers", "()I")
                ))
            || (class == FIELD
                && matches!(
                    (name, sig),
                    ("getName", "()Ljava/lang/String;")
                        | ("getType" | "getDeclaringClass", "()Ljava/lang/Class;")
                        | ("getModifiers", "()I")
                        | ("get", "(Ljava/lang/Object;)Ljava/lang/Object;")
                        | ("getBoolean", "(Ljava/lang/Object;)Z")
                        | ("getByte", "(Ljava/lang/Object;)B")
                        | ("getShort", "(Ljava/lang/Object;)S")
                        | ("getChar", "(Ljava/lang/Object;)C")
                        | ("getInt", "(Ljava/lang/Object;)I")
                        | ("getLong", "(Ljava/lang/Object;)J")
                        | ("getFloat", "(Ljava/lang/Object;)F")
                        | ("getDouble", "(Ljava/lang/Object;)D")
                ));
        implemented.then_some(rd_vm::framework_host::HostMethodKind::Instance)
    }
    pub(in super::super) fn call(
        &self,
        services: &Services,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let actual = vm.heap.class_of(receiver)?.to_string();
        if actual == CLASS
            && matches!(name, "getField" | "getDeclaredField")
            && sig == "(Ljava/lang/String;)Ljava/lang/reflect/Field;"
        {
            if args[0].as_ref()?.is_none() {
                return Err(exception(
                    vm,
                    "Ljava/lang/NullPointerException;",
                    "name == null",
                ));
            }
            let name_text = text(vm, &args[0])?;
            let class = desc(vm, receiver)?;
            if vm.device_class_object(&class)? != receiver {
                return Err(unavailable(
                    "Class object is not a canonical loaded identity",
                ));
            }
            let definition = lookup(vm, &class, &name_text, name == "getField", &mut Vec::new())?
                .ok_or_else(|| missing(vm, &class, &name_text))?;
            let component = definition.2.trim_start_matches('[');
            if component.len() != 1 {
                link_class(
                    vm,
                    component,
                    super::super::package::LoaderScope::Application,
                )?;
            } else if !matches!(component, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D") {
                return Err(error(
                    "INVALID_CLASS_METADATA",
                    "invalid reflected field primitive descriptor",
                ));
            }
            let owner_class = vm.device_class_object(&definition.0)?;
            let type_class = vm.device_class_object(&definition.2)?;
            let entry = Entry {
                definition,
                owner_class,
                type_class,
                accessible: false,
            };
            let objects = self.objects.borrow();
            if objects.len() >= MAX_FIELDS
                || objects.values().map(Entry::bytes).sum::<usize>() + entry.bytes() > MAX_BYTES
            {
                return Err(error(
                    "REFLECTION_BUDGET",
                    "4096 Fields / 4 MiB field metadata",
                ));
            }
            drop(objects);
            let field = vm.heap.alloc_instance(FIELD.into(), vec![])?;
            self.objects.borrow_mut().insert(field, entry);
            return Ok(Some(Value::Obj(field)));
        }
        if actual != FIELD {
            return Ok(None);
        }
        let entry = self
            .objects
            .borrow()
            .get(&receiver)
            .cloned()
            .ok_or_else(|| unavailable("Field object is not owned by the reflection service"))?;
        let result = match (name, sig) {
            ("getName", "()Ljava/lang/String;") => string(vm, entry.definition.1.clone())?,
            ("getType", "()Ljava/lang/Class;") => Value::Obj(entry.type_class),
            ("getDeclaringClass", "()Ljava/lang/Class;") => Value::Obj(entry.owner_class),
            ("getModifiers", "()I") => Value::Int((entry.definition.3 & 0xffff) as i32),
            ("isAccessible", "()Z") => Value::Int(i32::from(entry.accessible)),
            ("setAccessible", "(Z)V") => {
                self.objects
                    .borrow_mut()
                    .get_mut(&receiver)
                    .expect("checked Field")
                    .accessible = args[0].as_int()? != 0;
                Value::Null
            }
            _ => {
                let target = match (name, sig) {
                    ("get", "(Ljava/lang/Object;)Ljava/lang/Object;") => "",
                    ("getBoolean", "(Ljava/lang/Object;)Z") => "Z",
                    ("getByte", "(Ljava/lang/Object;)B") => "B",
                    ("getShort", "(Ljava/lang/Object;)S") => "S",
                    ("getChar", "(Ljava/lang/Object;)C") => "C",
                    ("getInt", "(Ljava/lang/Object;)I") => "I",
                    ("getLong", "(Ljava/lang/Object;)J") => "J",
                    ("getFloat", "(Ljava/lang/Object;)F") => "F",
                    ("getDouble", "(Ljava/lang/Object;)D") => "D",
                    _ => return Ok(None),
                };
                let mut roots = vec![receiver, entry.owner_class, entry.type_class];
                if let Some(r) = args[0].as_ref()? {
                    roots.push(r);
                }
                let _scope = self.protect(roots)?;
                let raw = value(self, receiver, vm, &entry, &args[0])?;
                let ty = &entry.definition.2;
                if target.is_empty() {
                    if ty.len() == 1 {
                        let normalized = widen(vm, ty, ty, raw)?;
                        let wrapper = [
                            "Ljava/lang/Boolean;",
                            "Ljava/lang/Byte;",
                            "Ljava/lang/Character;",
                            "Ljava/lang/Short;",
                            "Ljava/lang/Integer;",
                            "Ljava/lang/Long;",
                            "Ljava/lang/Float;",
                            "Ljava/lang/Double;",
                        ]
                        .into_iter()
                        .find(|class| collections::primitive(class) == Some(ty.as_str()))
                        .ok_or_else(|| error("REFLECTION_STATE", "field primitive wrapper"))?;
                        services.collections.boxed(vm, wrapper, normalized)?
                    } else {
                        raw
                    }
                } else {
                    widen(vm, ty, target, raw)?
                }
            }
        };
        Ok(Some(result))
    }
    pub(in super::super) fn roots(&self) -> Vec<ObjRef> {
        self.scopes.borrow().values().flatten().copied().collect()
    }
    pub(in super::super) fn references(&self, field: ObjRef) -> Vec<ObjRef> {
        self.objects
            .borrow()
            .get(&field)
            .map_or_else(Vec::new, |entry| vec![entry.owner_class, entry.type_class])
    }
    pub(in super::super) fn collect(&self, marked: &[bool]) {
        self.objects
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
}
