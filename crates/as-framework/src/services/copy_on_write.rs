//! Immutable array generations with snapshot iterators; no ArrayList aliasing.
use super::*;
use alloc::rc::Weak;
use rd_vm::framework_host::HostMethodKind;
const CLASS: &str = "Ljava/util/concurrent/CopyOnWriteArrayList;";
const ITER: &str = "Ljava/util/concurrent/CopyOnWriteArrayList$COWIterator;";
const LIMIT: usize = 10000;
const PIN_LIMIT: usize = 32768;
#[derive(Clone)]
struct Snapshot {
    id: u64,
    values: Rc<Vec<Value>>,
}
#[derive(Default)]
struct List {
    current: Option<Snapshot>,
    lock: Option<ObjRef>,
}
struct SnapshotIterator {
    snapshot: Snapshot,
    cursor: usize,
}
#[derive(Default)]
pub(super) struct Copies {
    lists: BTreeMap<ObjRef, List>,
    iterators: BTreeMap<ObjRef, SnapshotIterator>,
    generations: BTreeMap<u64, Weak<Vec<Value>>>,
    serial: u64,
    pins: BTreeMap<u64, Vec<ObjRef>>,
    tickets: u64,
}
impl Copies {
    pub(super) fn bytes(&mut self) -> usize {
        self.generations
            .retain(|_, snapshot| snapshot.strong_count() != 0);
        self.generations
            .values()
            .filter_map(Weak::upgrade)
            .map(|values| {
                values
                    .capacity()
                    .saturating_mul(core::mem::size_of::<Value>())
            })
            .sum()
    }
    pub(super) fn objects(&self) -> usize {
        self.lists.len() + self.iterators.len()
    }
    pub(super) fn references(&self, r: ObjRef) -> Option<Vec<ObjRef>> {
        if let Some(list) = self.lists.get(&r) {
            let mut roots: Vec<_> = list.lock.into_iter().collect();
            if let Some(snapshot) = &list.current {
                roots.extend(refs(&snapshot.values));
            }
            return Some(roots);
        }
        self.iterators
            .get(&r)
            .map(|iterator| refs(&iterator.snapshot.values).collect())
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.pins.values().flatten().copied().collect()
    }
    pub(super) fn collect(&mut self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        self.lists.retain(|r, _| live(r));
        self.iterators.retain(|r, _| live(r));
        self.bytes();
    }
}
fn refs(values: &[Value]) -> impl Iterator<Item = ObjRef> + '_ {
    values.iter().filter_map(|value| match value {
        Value::Obj(r) => Some(*r),
        _ => None,
    })
}
fn failure(class: &str, cause: &str) -> VmExit {
    Throwable::new(class, cause).into()
}
fn no_message(class: &str) -> VmExit {
    Throwable {
        class: class.into(),
        message: None,
        obj: None,
    }
    .into()
}
fn budget() -> VmExit {
    error(
        "COLLECTION_BUDGET",
        "8192 containers / 8 MiB retained values / 10000 elements / bounded COW roots",
    )
}
fn nullable(value: &Value) -> Result<Value> {
    Ok(value.as_ref()?.map_or(Value::Null, Value::Obj))
}
fn index(value: i32, len: usize, insert: bool) -> Result<usize> {
    if value < 0
        || if insert {
            value as usize > len
        } else {
            value as usize >= len
        }
    {
        return Err(failure(
            if insert {
                "Ljava/lang/IndexOutOfBoundsException;"
            } else {
                "Ljava/lang/ArrayIndexOutOfBoundsException;"
            },
            "CopyOnWriteArrayList index",
        ));
    }
    Ok(value as usize)
}
struct Pin<'a> {
    state: &'a RefCell<Copies>,
    ticket: u64,
}
impl Pin<'_> {
    fn extend(&self, values: impl IntoIterator<Item = ObjRef>) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let mut roots = Vec::new();
        roots.extend(values);
        let count = state.pins.values().map(Vec::len).sum::<usize>();
        if count.saturating_add(roots.len()) > PIN_LIMIT {
            return Err(budget());
        }
        state
            .pins
            .get_mut(&self.ticket)
            .ok_or_else(|| error("COW_ROOT_SCOPE_LOST", "root ticket disappeared"))?
            .extend(roots);
        Ok(())
    }
    fn snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        self.extend(refs(&snapshot.values))
    }
}
impl Drop for Pin<'_> {
    fn drop(&mut self) {
        self.state.borrow_mut().pins.remove(&self.ticket);
    }
}
pub(super) fn metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance};
    if class == CLASS {
        return match (name, sig) {
            ("<init>", "()V" | "([Ljava/lang/Object;)V" | "(Ljava/util/Collection;)V") => {
                Some(Constructor)
            }
            ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
            | ("get", "(I)Ljava/lang/Object;")
            | ("set", "(ILjava/lang/Object;)Ljava/lang/Object;")
            | ("add" | "addIfAbsent" | "remove" | "contains", "(Ljava/lang/Object;)Z")
            | ("add", "(ILjava/lang/Object;)V")
            | ("remove", "(I)Ljava/lang/Object;")
            | ("indexOf" | "lastIndexOf", "(Ljava/lang/Object;)I")
            | ("iterator", "()Ljava/util/Iterator;")
            | ("listIterator", "()Ljava/util/ListIterator;" | "(I)Ljava/util/ListIterator;")
            | ("toArray", "()[Ljava/lang/Object;" | "([Ljava/lang/Object;)[Ljava/lang/Object;")
            | ("addAll", "(Ljava/util/Collection;)Z" | "(ILjava/util/Collection;)Z") => {
                Some(Instance)
            }
            _ => None,
        };
    }
    if class == ITER
        && matches!(
            (name, sig),
            ("hasNext" | "hasPrevious", "()Z")
                | ("next" | "previous", "()Ljava/lang/Object;")
                | ("nextIndex" | "previousIndex", "()I")
                | ("remove", "()V")
                | ("set" | "add", "(Ljava/lang/Object;)V")
        )
    {
        return Some(Instance);
    }
    None
}
impl Collections {
    pub(super) fn cow_attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if !vm.cp.is_subtype(class, CLASS) {
            return Ok(false);
        }
        self.budget(0)?;
        self.cow.borrow_mut().lists.entry(r).or_default();
        Ok(true)
    }
    pub(super) fn cow_items(&self, r: ObjRef) -> Option<Vec<Value>> {
        self.cow
            .borrow()
            .lists
            .get(&r)?
            .current
            .as_ref()
            .map(|snapshot| snapshot.values.as_ref().clone())
    }
    fn cow_pin(&self, r: ObjRef, args: &[Value]) -> Result<Pin<'_>> {
        let ticket = {
            let mut state = self.cow.borrow_mut();
            if state.pins.len() >= 128 {
                return Err(budget());
            }
            let next = state.tickets.checked_add(1).ok_or_else(budget)?;
            state.tickets = next;
            state.pins.insert(next, Vec::new());
            next
        };
        let pin = Pin {
            state: &self.cow,
            ticket,
        };
        pin.extend(core::iter::once(r).chain(refs(args)))?;
        Ok(pin)
    }
    fn cow_current(&self, r: ObjRef) -> Result<Snapshot> {
        self.cow
            .borrow()
            .lists
            .get(&r)
            .and_then(|list| list.current.clone())
            .ok_or_else(|| error("COW_STATE", "base constructor has not completed"))
    }
    fn cow_capacity(&self, capacity: usize) -> Result<()> {
        let regular = self
            .regular_slots()
            .saturating_mul(core::mem::size_of::<Value>());
        let mut state = self.cow.borrow_mut();
        let allocated = state.bytes();
        if capacity > LIMIT
            || state.generations.len() >= 16384
            || regular
                .saturating_add(allocated)
                .saturating_add(capacity.saturating_mul(core::mem::size_of::<Value>()))
                > 8 << 20
        {
            return Err(budget());
        }
        Ok(())
    }
    fn cow_copy(&self, values: &[Value], capacity: usize) -> Result<Vec<Value>> {
        self.cow_capacity(capacity)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(capacity).map_err(|_| budget())?;
        self.cow_capacity(copy.capacity())?;
        copy.extend_from_slice(values);
        Ok(copy)
    }
    fn cow_snapshot(&self, values: Vec<Value>) -> Result<Snapshot> {
        self.cow_capacity(values.capacity())?;
        let mut state = self.cow.borrow_mut();
        let serial = state.serial.checked_add(1).ok_or_else(budget)?;
        state.serial = serial;
        let values = Rc::new(values);
        state.generations.insert(serial, Rc::downgrade(&values));
        Ok(Snapshot { id: serial, values })
    }
    fn cow_publish(&self, r: ObjRef, values: Vec<Value>) -> Result<()> {
        let snapshot = self.cow_snapshot(values)?;
        self.cow
            .borrow_mut()
            .lists
            .get_mut(&r)
            .ok_or_else(|| error("COW_STATE", "list disappeared"))?
            .current = Some(snapshot);
        Ok(())
    }
    fn cow_locked<T>(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        work: impl FnOnce(&mut Engine) -> Result<T>,
    ) -> Result<T> {
        let lock = self
            .cow
            .borrow()
            .lists
            .get(&r)
            .and_then(|list| list.lock)
            .ok_or_else(|| error("COW_STATE", "private lock is unavailable"))?;
        vm.monitor_enter(lock)?;
        let result = work(vm);
        let released = vm.monitor_exit(lock);
        match result {
            Err(error) => Err(error),
            Ok(value) => {
                released?;
                Ok(value)
            }
        }
    }
    fn cow_array_values(&self, vm: &Engine, array: ObjRef) -> Result<Vec<Value>> {
        let descriptor = vm.heap.class_of(array)?;
        if !descriptor.starts_with("[L") && !descriptor.starts_with("[[") {
            return Err(failure(
                "Ljava/lang/ClassCastException;",
                "Object[] required",
            ));
        }
        let values = vm
            .heap
            .as_array_elems(array)
            .map_err(|e| error("VM_TYPE_ERROR", e))?;
        let mut result = self.cow_copy(&[], values.0.len())?;
        for value in values.0 {
            result.push(nullable(value)?);
        }
        Ok(result)
    }
    fn cow_collection_values(
        &self,
        vm: &mut Engine,
        source: &Value,
        pin: &Pin<'_>,
    ) -> Result<Vec<Value>> {
        let source = source
            .as_ref()?
            .ok_or_else(|| failure("Ljava/lang/NullPointerException;", "collection is null"))?;
        pin.extend([source])?;
        let array = vm
            .invoke_instance(source, "toArray", "()[Ljava/lang/Object;", &[])?
            .as_ref()?
            .ok_or_else(|| {
                failure(
                    "Ljava/lang/NullPointerException;",
                    "Collection.toArray returned null",
                )
            })?;
        pin.extend([array])?;
        self.cow_array_values(vm, array)
    }
    fn cow_find(
        &self,
        vm: &mut Engine,
        query: &Value,
        snapshot: &Snapshot,
        start: usize,
    ) -> Result<Option<usize>> {
        for (index, value) in snapshot.values.iter().enumerate().skip(start) {
            if equals(vm, query, value)? {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }
    fn cow_to_array(
        &self,
        vm: &mut Engine,
        snapshot: &Snapshot,
        target: Option<&Value>,
        pin: &Pin<'_>,
    ) -> Result<Value> {
        let (array, descriptor) = if let Some(target) = target {
            let array = target.as_ref()?.ok_or_else(|| {
                failure("Ljava/lang/NullPointerException;", "toArray target null")
            })?;
            let descriptor = vm.heap.class_of(array)?.to_string();
            if !descriptor.starts_with("[L") && !descriptor.starts_with("[[") {
                return Err(failure(
                    "Ljava/lang/ArrayStoreException;",
                    "reference array required",
                ));
            }
            let length = vm
                .heap
                .as_array_elems(array)
                .map_err(|e| error("VM_TYPE_ERROR", e))?
                .0
                .len();
            if length >= snapshot.values.len() {
                (array, descriptor)
            } else {
                (
                    vm.heap.alloc_array_zeroed(
                        rd_vm::heap::ElemKind::Obj,
                        snapshot.values.len(),
                        &descriptor,
                    )?,
                    descriptor,
                )
            }
        } else {
            (
                vm.heap.alloc_array_zeroed(
                    rd_vm::heap::ElemKind::Obj,
                    snapshot.values.len(),
                    "[Ljava/lang/Object;",
                )?,
                "[Ljava/lang/Object;".into(),
            )
        };
        pin.extend([array])?;
        for (index, value) in snapshot.values.iter().enumerate() {
            if let Value::Obj(r) = value {
                if !vm.cp.is_subtype(vm.heap.class_of(*r)?, &descriptor[1..]) {
                    return Err(failure(
                        "Ljava/lang/ArrayStoreException;",
                        "CopyOnWriteArrayList.toArray element",
                    ));
                }
            }
            vm.heap
                .array_elems_mut(array)
                .map_err(|e| error("VM_TYPE_ERROR", e))?[index] = value.clone();
        }
        let length = vm
            .heap
            .as_array_elems(array)
            .map_err(|e| error("VM_TYPE_ERROR", e))?
            .0
            .len();
        if length > snapshot.values.len() {
            vm.heap
                .array_elems_mut(array)
                .map_err(|e| error("VM_TYPE_ERROR", e))?[snapshot.values.len()] = Value::Null;
        }
        Ok(Value::Obj(array))
    }
    fn cow_iterator(&self, vm: &mut Engine, snapshot: Snapshot, cursor: usize) -> Result<Value> {
        self.budget(0)?;
        let iterator = vm.heap.alloc_instance(ITER.into(), vec![])?;
        self.cow
            .borrow_mut()
            .iterators
            .insert(iterator, SnapshotIterator { snapshot, cursor });
        Ok(Value::Obj(iterator))
    }
    fn cow_iterator_call(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        name: &str,
        sig: &str,
    ) -> Result<Option<Value>> {
        // Materialize before interpreter diagnostic frames can affect the
        // transport wrapper. No iterator-state borrow crosses allocation.
        match self.cow_iterator_call_inner(r, name, sig) {
            Err(VmExit::Exception(mut failure)) => {
                failure.obj = Some(vm.materialize_throwable(&failure)?);
                Err(failure.into())
            }
            result => result,
        }
    }
    fn cow_iterator_call_inner(&self, r: ObjRef, name: &str, sig: &str) -> Result<Option<Value>> {
        let mut state = self.cow.borrow_mut();
        let Some(iterator) = state.iterators.get_mut(&r) else {
            return Ok(None);
        };
        let len = iterator.snapshot.values.len();
        let value = match (name, sig) {
            ("hasNext", "()Z") => Value::Int((iterator.cursor < len) as i32),
            ("hasPrevious", "()Z") => Value::Int((iterator.cursor > 0) as i32),
            ("nextIndex", "()I") => Value::Int(iterator.cursor as i32),
            ("previousIndex", "()I") => Value::Int(iterator.cursor as i32 - 1),
            ("next", "()Ljava/lang/Object;") => {
                let value = iterator
                    .snapshot
                    .values
                    .get(iterator.cursor)
                    .cloned()
                    .ok_or_else(|| no_message("Ljava/util/NoSuchElementException;"))?;
                iterator.cursor += 1;
                value
            }
            ("previous", "()Ljava/lang/Object;") => {
                if iterator.cursor == 0 {
                    return Err(no_message("Ljava/util/NoSuchElementException;"));
                }
                iterator.cursor -= 1;
                iterator.snapshot.values[iterator.cursor].clone()
            }
            ("remove", "()V") | ("set" | "add", "(Ljava/lang/Object;)V") => {
                return Err(no_message("Ljava/lang/UnsupportedOperationException;"))
            }
            _ => {
                return Err(error(
                    "API_UNAVAILABLE",
                    format!("COW snapshot iterator.{name}{sig}"),
                ))
            }
        };
        Ok(Some(value))
    }
    pub(super) fn cow_call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if name == "getClass" {
            return Ok(None);
        }
        if self.cow.borrow().iterators.contains_key(&r) {
            return self.cow_iterator_call(vm, r, name, sig);
        }
        if !self.cow.borrow().lists.contains_key(&r) {
            return Ok(None);
        }
        if metadata(CLASS, name, sig).is_none() {
            return Err(error(
                "API_UNAVAILABLE",
                format!("CopyOnWriteArrayList.{name}{sig}"),
            ));
        }
        let pin = self.cow_pin(r, args)?;
        if name == "<init>" {
            let values = match sig {
                "()V" => self.cow_copy(&[], 0)?,
                "([Ljava/lang/Object;)V" => {
                    let array = args[0].as_ref()?.ok_or_else(|| {
                        failure("Ljava/lang/NullPointerException;", "constructor array null")
                    })?;
                    self.cow_array_values(vm, array)?
                }
                _ => self.cow_collection_values(vm, &args[0], &pin)?,
            };
            let lock = vm
                .heap
                .alloc_instance("Ljava/lang/Object;".into(), vec![])?;
            pin.extend([lock])?;
            self.cow_publish(r, values)?;
            self.cow.borrow_mut().lists.get_mut(&r).unwrap().lock = Some(lock);
            return Ok(Some(Value::Null));
        }
        let snapshot = self.cow_current(r)?;
        pin.snapshot(&snapshot)?;
        let len = snapshot.values.len();
        let value = match (name, sig) {
            ("size", "()I") => Value::Int(len as i32),
            ("isEmpty", "()Z") => Value::Int((len == 0) as i32),
            ("get", "(I)Ljava/lang/Object;") => {
                snapshot.values[index(int(args, 0)?, len, false)?].clone()
            }
            ("contains", "(Ljava/lang/Object;)Z") => Value::Int(
                self.cow_find(vm, &nullable(&args[0])?, &snapshot, 0)?
                    .is_some() as i32,
            ),
            ("indexOf", "(Ljava/lang/Object;)I") => Value::Int(
                self.cow_find(vm, &nullable(&args[0])?, &snapshot, 0)?
                    .map_or(-1, |i| i as i32),
            ),
            ("lastIndexOf", "(Ljava/lang/Object;)I") => {
                let query = nullable(&args[0])?;
                let mut found = -1;
                for (index, value) in snapshot.values.iter().enumerate().rev() {
                    if equals(vm, &query, value)? {
                        found = index as i32;
                        break;
                    }
                }
                Value::Int(found)
            }
            ("iterator", "()Ljava/util/Iterator;")
            | ("listIterator", "()Ljava/util/ListIterator;") => {
                self.cow_iterator(vm, snapshot, 0)?
            }
            ("listIterator", "(I)Ljava/util/ListIterator;") => {
                let cursor = index(int(args, 0)?, len, true)?;
                self.cow_iterator(vm, snapshot, cursor)?
            }
            ("toArray", "()[Ljava/lang/Object;") => self.cow_to_array(vm, &snapshot, None, &pin)?,
            ("toArray", "([Ljava/lang/Object;)[Ljava/lang/Object;") => {
                self.cow_to_array(vm, &snapshot, Some(&args[0]), &pin)?
            }
            ("remove", "(Ljava/lang/Object;)Z") => {
                let query = nullable(&args[0])?;
                if let Some(position) = self.cow_find(vm, &query, &snapshot, 0)? {
                    self.cow_locked(vm, r, |vm| {
                        let current = self.cow_current(r)?;
                        pin.snapshot(&current)?;
                        let len = current.values.len();
                        let mut position = position;
                        if snapshot.id != current.id {
                            let mut changed = None;
                            for i in 0..position.min(len) {
                                if current.values[i] != snapshot.values[i]
                                    && equals(vm, &query, &current.values[i])?
                                {
                                    changed = Some(i);
                                    break;
                                }
                            }
                            if let Some(index) = changed {
                                position = index;
                            } else {
                                if position >= len {
                                    return Ok(Value::Int(0));
                                }
                                if current.values[position] != query {
                                    let Some(found) =
                                        self.cow_find(vm, &query, &current, position)?
                                    else {
                                        return Ok(Value::Int(0));
                                    };
                                    position = found;
                                }
                            }
                        }
                        let mut values = self.cow_copy(&current.values, len)?;
                        values.remove(position);
                        self.cow_publish(r, values)?;
                        Ok(Value::Int(1))
                    })?
                } else {
                    Value::Int(0)
                }
            }
            ("addIfAbsent", "(Ljava/lang/Object;)Z") => {
                let query = nullable(&args[0])?;
                if self.cow_find(vm, &query, &snapshot, 0)?.is_some() {
                    Value::Int(0)
                } else {
                    self.cow_locked(vm, r, |vm| {
                        let current = self.cow_current(r)?;
                        pin.snapshot(&current)?;
                        let len = current.values.len();
                        if snapshot.id != current.id {
                            let common = len.min(snapshot.values.len());
                            for i in 0..common {
                                if current.values[i] != snapshot.values[i]
                                    && equals(vm, &query, &current.values[i])?
                                {
                                    return Ok(Value::Int(0));
                                }
                            }
                            if self.cow_find(vm, &query, &current, common)?.is_some() {
                                return Ok(Value::Int(0));
                            }
                        }
                        let mut values = self.cow_copy(&current.values, len.saturating_add(1))?;
                        values.push(query);
                        self.cow_publish(r, values)?;
                        Ok(Value::Int(1))
                    })?
                }
            }
            ("addAll", "(Ljava/util/Collection;)Z" | "(ILjava/util/Collection;)Z") => {
                let incoming = self.cow_snapshot(self.cow_collection_values(
                    vm,
                    args.last().unwrap(),
                    &pin,
                )?)?;
                pin.snapshot(&incoming)?;
                self.cow_locked(vm, r, |_| {
                    let current = self.cow_current(r)?;
                    let len = current.values.len();
                    let position = if args.len() == 2 {
                        index(int(args, 0)?, len, true)?
                    } else {
                        len
                    };
                    if incoming.values.is_empty() {
                        return Ok(Value::Int(0));
                    }
                    let mut values =
                        self.cow_copy(&[], len.saturating_add(incoming.values.len()))?;
                    values.extend_from_slice(&current.values[..position]);
                    values.extend_from_slice(&incoming.values);
                    values.extend_from_slice(&current.values[position..]);
                    self.cow_publish(r, values)?;
                    Ok(Value::Int(1))
                })?
            }
            _ => self.cow_locked(vm, r, |_| {
                let current = self.cow_current(r)?;
                let len = current.values.len();
                let position = match (name, sig) {
                    ("set", _) | ("remove", "(I)Ljava/lang/Object;") => {
                        Some(index(int(args, 0)?, len, false)?)
                    }
                    ("add", "(ILjava/lang/Object;)V") => Some(index(int(args, 0)?, len, true)?),
                    _ => None,
                };
                let supplied = if matches!(name, "add" | "set") {
                    Some(nullable(args.last().unwrap())?)
                } else {
                    None
                };
                if name == "set" && current.values[position.unwrap()] == *supplied.as_ref().unwrap()
                {
                    // API26 republishes the same array for the volatile-write
                    // contract. The monitor fences already publish this owner;
                    // do not create an unnecessary retained generation.
                    return Ok(current.values[position.unwrap()].clone());
                }
                let mut values = match name {
                    "clear" => self.cow_copy(&[], 0)?,
                    "add" => self.cow_copy(&current.values, len.saturating_add(1))?,
                    _ => self.cow_copy(&current.values, len)?,
                };
                let result = match (name, sig) {
                    ("clear", _) => Value::Null,
                    ("add", "(Ljava/lang/Object;)Z") => {
                        values.push(supplied.unwrap());
                        Value::Int(1)
                    }
                    ("add", "(ILjava/lang/Object;)V") => {
                        values.insert(position.unwrap(), supplied.unwrap());
                        Value::Null
                    }
                    ("set", _) => {
                        core::mem::replace(&mut values[position.unwrap()], supplied.unwrap())
                    }
                    ("remove", "(I)Ljava/lang/Object;") => values.remove(position.unwrap()),
                    _ => {
                        return Err(error(
                            "API_UNAVAILABLE",
                            format!("CopyOnWriteArrayList.{name}{sig}"),
                        ))
                    }
                };
                self.cow_publish(r, values)?;
                Ok(result)
            })?,
        };
        Ok(Some(value))
    }
}
