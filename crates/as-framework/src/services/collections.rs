use super::*;
#[path = "copy_on_write.rs"]
mod copy_on_write;
#[path = "enum_map.rs"]
mod enum_map;
#[derive(Clone)]
enum Container {
    List(Vec<Value>),
    Set(Vec<Value>),
    Map(Vec<(Value, Value)>),
    SnapshotIterator {
        elems: Vec<Value>,
        pos: usize,
    },
    Iterator {
        owner: ObjRef,
        position: usize,
        expected: u64,
    },
}
#[derive(Default)]
pub(super) struct Collections {
    cow: RefCell<copy_on_write::Copies>,
    objects: RefCell<BTreeMap<ObjRef, Container>>,
    changes: RefCell<BTreeMap<ObjRef, u64>>,
}
pub(super) fn primitive(class: &str) -> Option<&'static str> {
    rd_vm::boxes::primitive_descriptor(class)
}
fn kind(vm: &Engine, class: &str) -> Option<&'static str> {
    let mut class = class.to_string();
    for _ in 0..64 {
        match class.as_str() {
            "Ljava/util/ArrayList;" | "Ljava/util/LinkedList;" | "Ljava/util/Vector;" => {
                return Some("list")
            }
            "Ljava/util/HashMap;" | "Ljava/util/LinkedHashMap;" | "Ljava/util/EnumMap;" => {
                return Some("map")
            }
            "Ljava/util/HashSet;" | "Ljava/util/LinkedHashSet;" => return Some("set"),
            _ if primitive(&class).is_some() => return Some("box"),
            _ => class = vm.cp.superclass_of(&class)?,
        }
    }
    None
}
pub(super) fn equals(vm: &mut Engine, left: &Value, right: &Value) -> Result<bool> {
    if left == right {
        return Ok(true);
    }
    let (Value::Obj(a), Value::Obj(b)) = (left, right) else {
        return Ok(false);
    };
    if let (Ok(a), Ok(b)) = (vm.heap.as_str(*a), vm.heap.as_str(*b)) {
        return Ok(a == b);
    }
    Ok(vm
        .invoke_instance(
            *a,
            "equals",
            "(Ljava/lang/Object;)Z",
            core::slice::from_ref(right),
        )?
        .as_int()?
        != 0)
}
impl Collections {
    pub(super) fn snapshot(
        &self,
        vm: &Engine,
        object: ObjRef,
    ) -> Result<Option<rd_vm::collection_factories::Contents>> {
        use rd_vm::collection_factories::Contents;
        if let Some(values) = self.cow_items(object) {
            return Ok(Some(Contents::List(values)));
        }
        if vm.heap.class_of(object)? == "Ljava/util/Arrays$ArrayList;" {
            let array = obj(&vm.heap.get_field(object, "backingArray")?)?;
            return Ok(Some(Contents::List(
                vm.heap.as_array_elems(array)?.0.to_vec(),
            )));
        }
        Ok(match self.objects.borrow().get(&object) {
            Some(Container::List(values)) => Some(Contents::List(values.clone())),
            Some(Container::Set(values)) => Some(Contents::Set(values.clone())),
            Some(Container::Map(values)) => Some(Contents::Map(values.clone())),
            Some(Container::SnapshotIterator { elems, pos }) => Some(Contents::Iterator {
                elems: elems.clone(),
                pos: *pos,
            }),
            _ => None,
        })
    }
    pub(super) fn create(
        &self,
        vm: &mut Engine,
        class: &str,
        contents: rd_vm::collection_factories::Contents,
    ) -> Result<ObjRef> {
        use rd_vm::collection_factories::{Contents, LIST, MAP, SET};
        let (slots, container) = match (class, contents) {
            (LIST, Contents::List(values)) => (values.len(), Container::List(values)),
            (SET, Contents::Set(values)) => (values.len(), Container::Set(values)),
            (MAP, Contents::Map(values)) => {
                (values.len().saturating_mul(2), Container::Map(values))
            }
            ("Ljava/util/ArrayList;", Contents::List(values)) => {
                (values.len(), Container::List(values))
            }
            ("Ljava/util/ListIterator;", Contents::Iterator { elems, pos }) => {
                (elems.len(), Container::SnapshotIterator { elems, pos })
            }
            ("Ljava/util/Iterator;", Contents::Iterator { elems, pos }) => {
                (elems.len(), Container::SnapshotIterator { elems, pos })
            }
            ("Ljava/util/LinkedHashSet;", Contents::Set(elems)) => {
                (elems.len(), Container::Set(elems))
            }
            _ => return Err(error("INVALID_COLLECTION_FACTORY", class)),
        };
        self.budget(slots)?;
        let object = vm.heap.alloc_instance(class.into(), vec![])?;
        self.objects.borrow_mut().insert(object, container);
        Ok(object)
    }
    pub(super) fn replace(
        &self,
        vm: &mut Engine,
        object: ObjRef,
        contents: rd_vm::collection_factories::Contents,
    ) -> Result<()> {
        use rd_vm::collection_factories::Contents;
        if self.cow_items(object).is_some() {
            let (Contents::List(values) | Contents::Set(values)) = contents else {
                return Err(error(
                    "INVALID_COLLECTION_STATE",
                    "COW owner needs elements",
                ));
            };
            return self.cow_replace(vm, object, values);
        }
        let previous = self
            .objects
            .borrow()
            .get(&object)
            .cloned()
            .ok_or_else(|| error("API_UNAVAILABLE", "collection update owner"))?;
        let container = match (previous, contents) {
            (Container::List(_), Contents::List(values)) => Container::List(values),
            (Container::Set(_), Contents::Set(values) | Contents::List(values)) => {
                Container::Set(values)
            }
            (Container::Map(_), Contents::Map(values)) => Container::Map(values),
            (Container::SnapshotIterator { .. }, Contents::Iterator { elems, pos }) => {
                Container::SnapshotIterator { elems, pos }
            }
            _ => return Err(error("INVALID_COLLECTION_STATE", "collection update kind")),
        };
        let slots = match &container {
            Container::List(v) | Container::Set(v) => v.len(),
            Container::Map(v) => v.len().saturating_mul(2),
            Container::SnapshotIterator { elems, .. } => elems.len(),
            Container::Iterator { .. } => 1,
        };
        self.budget(slots)?;
        let changed = match (self.objects.borrow().get(&object), &container) {
            (Some(Container::List(a)), Container::List(b))
            | (Some(Container::Set(a)), Container::Set(b)) => a != b,
            (Some(Container::Map(a)), Container::Map(b)) => a != b,
            _ => false,
        };
        self.objects.borrow_mut().insert(object, container);
        if changed {
            self.change(object);
        }
        Ok(())
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        if let Some(kind) = rd_vm::jcollections::method_metadata(class, name, sig) {
            return Some(kind);
        }
        if class == "Ljava/util/AbstractCollection;" {
            use rd_vm::framework_host::HostMethodKind::{Constructor, Instance};
            match (name, sig) {
                ("<init>", "()V") => return Some(Constructor),
                ("isEmpty", "()Z") | ("contains", "(Ljava/lang/Object;)Z") => {
                    return Some(Instance)
                }
                _ => {}
            }
        }
        enum_map::metadata(class, name, sig).or_else(|| copy_on_write::metadata(class, name, sig))
    }
    pub(super) fn array_list_view(&self, vm: &mut Engine, array: ObjRef) -> Result<Value> {
        if vm.heap.as_array_elems(array)?.1 != rd_vm::heap::ElemKind::Obj {
            return Err(
                Throwable::new("Ljava/lang/ClassCastException;", "Object[] required").into(),
            );
        }
        Ok(Value::Obj(vm.heap.alloc_instance(
            "Ljava/util/Arrays$ArrayList;".into(),
            vec![("backingArray".into(), Value::Obj(array))],
        )?))
    }
    fn regular_slots(&self) -> usize {
        self.objects
            .borrow()
            .values()
            .map(|c| match c {
                Container::List(v) | Container::Set(v) => v.len(),
                Container::Map(v) => v.len() * 2,
                Container::SnapshotIterator { elems, .. } => elems.len(),
                Container::Iterator { .. } => 1,
            })
            .sum()
    }
    fn budget(&self, extra: usize) -> Result<()> {
        let objects = self.objects.borrow();
        let slots = objects
            .values()
            .map(|c| match c {
                Container::List(v) | Container::Set(v) => v.len(),
                Container::Map(v) => v.len() * 2,
                Container::SnapshotIterator { elems, .. } => elems.len(),
                Container::Iterator { .. } => 1,
            })
            .sum::<usize>();
        let mut cow = self.cow.borrow_mut();
        let cow_bytes = cow.bytes();
        if objects.len() + cow.objects() >= 8192
            || slots
                .saturating_add(extra)
                .saturating_mul(core::mem::size_of::<Value>())
                .saturating_add(cow_bytes)
                > 8 << 20
        {
            return Err(error(
                "COLLECTION_BUDGET",
                "8192 containers / 8 MiB values per VM",
            ));
        }
        Ok(())
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if self.cow_attach(vm, class, r)? {
            return Ok(true);
        }
        let Some(kind) = kind(vm, class) else {
            return Ok(vm.cp.is_subtype(class, "Ljava/util/AbstractCollection;"));
        };
        let container = match kind {
            "list" => Container::List(Vec::new()),
            "set" => Container::Set(Vec::new()),
            "map" => Container::Map(Vec::new()),
            _ => return Ok(true),
        };
        self.budget(0)?;
        self.objects.borrow_mut().insert(r, container);
        Ok(true)
    }
    /// Framework boxing entry point; identity, narrowing and allocation are
    /// owned by the same upstream module used by both VM feature paths.
    pub(super) fn boxed(&self, vm: &mut Engine, class: &str, value: Value) -> Result<Value> {
        rd_vm::boxes::box_value(vm, class, value).map(Value::Obj)
    }
    pub(super) fn new_list(&self, vm: &mut Engine, values: Vec<Value>, set: bool) -> Result<Value> {
        self.budget(values.len())?;
        let r = vm.heap.alloc_instance(
            if set {
                "Ljava/util/HashSet;"
            } else {
                "Ljava/util/ArrayList;"
            }
            .into(),
            vec![],
        )?;
        self.objects.borrow_mut().insert(
            r,
            if set {
                Container::Set(values)
            } else {
                Container::List(values)
            },
        );
        Ok(Value::Obj(r))
    }
    pub(super) fn new_map(&self, vm: &mut Engine, values: Vec<(Value, Value)>) -> Result<Value> {
        self.budget(values.len().saturating_mul(2))?;
        let r = vm
            .heap
            .alloc_instance("Ljava/util/HashMap;".into(), vec![])?;
        self.objects.borrow_mut().insert(r, Container::Map(values));
        Ok(Value::Obj(r))
    }
    pub(super) fn items(&self, r: ObjRef) -> Result<Vec<Value>> {
        if let Some(items) = self.cow_items(r) {
            return Ok(items);
        }
        match self.objects.borrow().get(&r) {
            Some(Container::List(v) | Container::Set(v)) => Ok(v.clone()),
            _ => Err(error("API_UNAVAILABLE", "collection kind")),
        }
    }
    fn change(&self, r: ObjRef) {
        *self.changes.borrow_mut().entry(r).or_default() += 1;
    }
    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        rd_vm::boxes::static_intrinsic(vm, class, name, sig, args)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        vm: &mut Engine,
        _class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if let Some(value) = self.cow_call(vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        let actual = vm.heap.class_of(r)?.to_string();
        if rd_vm::collection_factories::rejects_mutation(&actual, name, sig) {
            return Err(Throwable::new(
                "Ljava/lang/UnsupportedOperationException;",
                "immutable Collections factory result",
            )
            .into());
        }
        if vm.cp.is_subtype(&actual, "Ljava/util/EnumMap;") {
            return self.enum_map_call(vm, name, sig, r, args);
        }
        if matches!(
            actual.as_str(),
            "Ljava/util/Arrays$ArrayList;" | "Ljava/util/Arrays$ArrayList$Itr;"
        ) {
            let array = obj(&vm.heap.get_field(r, "backingArray")?)?;
            let length = vm.heap.as_array_elems(array)?.0.len();
            let indexed = |index: i32| -> Result<usize> {
                if index < 0 || index as usize >= length {
                    Err(Throwable::new(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "array-backed list index",
                    )
                    .into())
                } else {
                    Ok(index as usize)
                }
            };
            let value = match (name, sig) {
                ("size", "()I") => Value::Int(length as i32),
                ("isEmpty", "()Z") => Value::Int(i32::from(length == 0)),
                ("get", "(I)Ljava/lang/Object;") => {
                    vm.heap.as_array_elems(array)?.0[indexed(int(args, 0)?)?].clone()
                }
                ("set", "(ILjava/lang/Object;)Ljava/lang/Object;") => {
                    let index = indexed(int(args, 0)?)?;
                    let component = vm.heap.class_of(array)?[1..].to_string();
                    if let Some(reference) = args[1].as_ref()? {
                        if !vm.cp.is_subtype(vm.heap.class_of(reference)?, &component) {
                            return Err(Throwable::new(
                                "Ljava/lang/ArrayStoreException;",
                                "array-backed list component",
                            )
                            .into());
                        }
                    }
                    let previous = core::mem::replace(
                        &mut vm.heap.array_elems_mut(array)?[index],
                        args[1].clone(),
                    );
                    previous
                }
                ("iterator", "()Ljava/util/Iterator;") => Value::Obj(vm.heap.alloc_instance(
                    "Ljava/util/Arrays$ArrayList$Itr;".into(),
                    vec![
                        ("backingArray".into(), Value::Obj(array)),
                        ("cursor".into(), Value::Int(0)),
                    ],
                )?),
                ("hasNext", "()Z") => Value::Int(i32::from(
                    (vm.heap.get_field(r, "cursor")?.as_int()? as usize) < length,
                )),
                ("next", "()Ljava/lang/Object;") => {
                    let index = vm.heap.get_field(r, "cursor")?.as_int()?;
                    if index < 0 || index as usize >= length {
                        return Err(Throwable::new(
                            "Ljava/util/NoSuchElementException;",
                            "array iterator exhausted",
                        )
                        .into());
                    }
                    let value = vm.heap.as_array_elems(array)?.0[index as usize].clone();
                    write(vm, r, "cursor", Value::Int(index + 1))?;
                    value
                }
                ("contains", "(Ljava/lang/Object;)Z") => {
                    let pin = self.cow_pin(r, args)?;
                    pin.extend(core::iter::once(array))?;
                    let mut found = false;
                    for index in 0..length {
                        let value = vm.heap.as_array_elems(array)?.0[index].clone();
                        if equals(vm, &args[0], &value)? {
                            found = true;
                            break;
                        }
                    }
                    Value::Int(i32::from(found))
                }
                ("add", "(Ljava/lang/Object;)Z")
                | ("remove", "(I)Ljava/lang/Object;")
                | ("remove", "()V") => {
                    return Err(Throwable::new(
                        "Ljava/lang/UnsupportedOperationException;",
                        "fixed-size list",
                    )
                    .into())
                }
                _ => return Ok(None),
            };
            return Ok(Some(value));
        }
        if primitive(&actual).is_some() {
            return rd_vm::boxes::instance_intrinsic(vm, &actual, name, sig, r, args);
        }
        rd_vm::jcollections::with_product_call(vm, r, args, |vm| {
            if let Some(value) = rd_vm::jcollections::call(vm, _class, name, sig, r, args)? {
                return Ok(Some(value));
            }
            self.regular_call(vm, name, sig, r, args)
        })
    }
    fn regular_call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        self.budget(2)?;
        let container = self.objects.borrow().get(&r).cloned();
        let Some(container) = container else {
            return Ok(None);
        };
        // Guest equals implementations may reenter the VM and collect. Keep
        // this operation's snapshot alive even if they clear its owner.
        let pin = self.cow_pin(r, args)?;
        pin.extend(self.references(r))?;
        let size = match &container {
            Container::List(v) | Container::Set(v) => v.len(),
            Container::Map(v) => v.len(),
            _ => 0,
        };
        let value = match (name, sig) {
            ("<init>", "()V" | "(I)V" | "(IF)V") => {
                if !args.is_empty()
                    && (int(args, 0)? < 0
                        || (args.len() == 2
                            && (args[1].as_float()? <= 0.0 || args[1].as_float()?.is_nan())))
                {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalArgumentException;",
                        "collection capacity/load factor",
                    )
                    .into());
                }
                Value::Null
            }
            ("size", "()I") => Value::Int(size as i32),
            ("isEmpty", "()Z") => Value::Int((size == 0) as i32),
            ("clear", "()V") => {
                self.change(r);
                self.objects.borrow_mut().insert(
                    r,
                    match container {
                        Container::Map(_) => Container::Map(Vec::new()),
                        Container::Set(_) => Container::Set(Vec::new()),
                        _ => Container::List(Vec::new()),
                    },
                );
                Value::Null
            }
            ("get", "(I)Ljava/lang/Object;") => {
                let Container::List(values) = container else {
                    return Ok(None);
                };
                values.get(int(args, 0)? as usize).cloned().ok_or_else(|| {
                    Throwable::new("Ljava/lang/IndexOutOfBoundsException;", "List.get")
                })?
            }
            ("remove", "(I)Ljava/lang/Object;") => {
                let Container::List(mut values) = container else {
                    return Ok(None);
                };
                let index = int(args, 0)?;
                if index < 0 || index as usize >= values.len() {
                    return Err(Throwable::new(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "List.remove",
                    )
                    .into());
                }
                let old = values.remove(index as usize);
                self.change(r);
                self.objects.borrow_mut().insert(r, Container::List(values));
                old
            }
            ("set", "(ILjava/lang/Object;)Ljava/lang/Object;") => {
                let Container::List(mut values) = container else {
                    return Ok(None);
                };
                let slot = values.get_mut(int(args, 0)? as usize).ok_or_else(|| {
                    Throwable::new("Ljava/lang/IndexOutOfBoundsException;", "List.set")
                })?;
                let old = core::mem::replace(slot, args[1].clone());
                self.objects.borrow_mut().insert(r, Container::List(values));
                old
            }
            ("add", "(Ljava/lang/Object;)Z") => {
                let (mut values, set) = match container {
                    Container::List(v) => (v, false),
                    Container::Set(v) => (v, true),
                    _ => return Ok(None),
                };
                let duplicate = if set {
                    let mut found = false;
                    for value in &values {
                        if equals(vm, value, &args[0])? {
                            found = true;
                            break;
                        }
                    }
                    found
                } else {
                    false
                };
                if !duplicate {
                    if values.len() >= 10000 {
                        return Err(error("COLLECTION_BUDGET", "10000 elements"));
                    }
                    values.push(args[0].clone());
                    self.change(r);
                    self.objects.borrow_mut().insert(
                        r,
                        if set {
                            Container::Set(values)
                        } else {
                            Container::List(values)
                        },
                    );
                }
                Value::Int((!duplicate) as i32)
            }
            ("contains", "(Ljava/lang/Object;)Z") => {
                let values = match container {
                    Container::List(v) | Container::Set(v) => v,
                    _ => return Ok(None),
                };
                let mut found = false;
                for value in &values {
                    if equals(vm, value, &args[0])? {
                        found = true;
                        break;
                    }
                }
                Value::Int(found as i32)
            }
            ("put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;") => {
                let Container::Map(mut values) = container else {
                    return Ok(None);
                };
                let mut old = Value::Null;
                let mut found = false;
                for (key, value) in &mut values {
                    if equals(vm, key, &args[0])? {
                        old = core::mem::replace(value, args[1].clone());
                        found = true;
                        break;
                    }
                }
                if !found {
                    if values.len() >= 10000 {
                        return Err(error("COLLECTION_BUDGET", "10000 entries"));
                    }
                    values.push((args[0].clone(), args[1].clone()));
                    self.change(r);
                }
                self.objects.borrow_mut().insert(r, Container::Map(values));
                old
            }
            ("get", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | ("containsKey", "(Ljava/lang/Object;)Z") => {
                let Container::Map(values) = container else {
                    return Ok(None);
                };
                let mut out = Value::Null;
                let mut found = false;
                for (key, value) in values {
                    if equals(vm, &key, &args[0])? {
                        out = value;
                        found = true;
                        break;
                    }
                }
                if name == "containsKey" {
                    Value::Int(found as i32)
                } else {
                    out
                }
            }
            ("containsValue", "(Ljava/lang/Object;)Z") => {
                let Container::Map(values) = container else {
                    return Ok(None);
                };
                let mut found = false;
                for (_, value) in values {
                    if equals(vm, &args[0], &value)? {
                        found = true;
                        break;
                    }
                }
                Value::Int(i32::from(found))
            }
            ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;") => {
                let Container::Map(mut values) = container else {
                    return Ok(None);
                };
                let mut index = None;
                for (i, (key, _)) in values.iter().enumerate() {
                    if equals(vm, key, &args[0])? {
                        index = Some(i);
                        break;
                    }
                }
                let out = index
                    .map(|i| {
                        self.change(r);
                        values.remove(i).1
                    })
                    .unwrap_or(Value::Null);
                self.objects.borrow_mut().insert(r, Container::Map(values));
                out
            }
            ("iterator", "()Ljava/util/Iterator;") => {
                if !matches!(container, Container::List(_) | Container::Set(_)) {
                    return Ok(None);
                }
                let iterator = vm.heap.alloc_instance(
                    "Lorg/androswitch/runtime/CollectionIterator;".into(),
                    vec![],
                )?;
                self.objects.borrow_mut().insert(
                    iterator,
                    Container::Iterator {
                        owner: r,
                        position: 0,
                        expected: *self.changes.borrow().get(&r).unwrap_or(&0),
                    },
                );
                Value::Obj(iterator)
            }
            ("hasNext", "()Z") | ("next", "()Ljava/lang/Object;") => {
                let Container::Iterator {
                    owner,
                    position,
                    expected,
                } = container
                else {
                    return Ok(None);
                };
                if *self.changes.borrow().get(&owner).unwrap_or(&0) != expected {
                    return Err(Throwable::new(
                        "Ljava/util/ConcurrentModificationException;",
                        "iterator owner changed",
                    )
                    .into());
                }
                let values = self.items(owner)?;
                if name == "hasNext" {
                    Value::Int((position < values.len()) as i32)
                } else {
                    let value = values.get(position).cloned().ok_or_else(|| {
                        Throwable::new("Ljava/util/NoSuchElementException;", "iterator exhausted")
                    })?;
                    self.objects.borrow_mut().insert(
                        r,
                        Container::Iterator {
                            owner,
                            position: position + 1,
                            expected,
                        },
                    );
                    value
                }
            }
            ("toArray", "()[Ljava/lang/Object;") => {
                let values = self.items(r)?;
                let array = vm.heap.alloc_array_zeroed(
                    rd_vm::heap::ElemKind::Obj,
                    values.len(),
                    "[Ljava/lang/Object;",
                )?;
                vm.heap
                    .array_elems_mut(array)
                    .map_err(|e| error("INVALID_ARRAY", e))?
                    .clone_from_slice(&values);
                Value::Obj(array)
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        if let Some(roots) = self.cow.borrow().references(r) {
            return roots;
        }
        let values = match self.objects.borrow().get(&r) {
            Some(Container::List(v) | Container::Set(v)) => v.clone(),
            Some(Container::Map(v)) => v.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect(),
            Some(Container::SnapshotIterator { elems, .. }) => elems.clone(),
            Some(Container::Iterator { owner, .. }) => return vec![*owner],
            None => return Vec::new(),
        };
        values
            .iter()
            .filter_map(|v| {
                if let Value::Obj(r) = v {
                    Some(*r)
                } else {
                    None
                }
            })
            .collect()
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.cow.borrow().roots()
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.cow.borrow_mut().collect(marked);
        self.objects
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
        self.changes
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
}
