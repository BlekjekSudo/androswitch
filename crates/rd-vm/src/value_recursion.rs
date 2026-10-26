//! Detect cycles in Java value callbacks before the native stack can overflow.
//! Keys are scoped to the actual execution owner; equal/hash/string operations
//! and different argument pairs do not interfere. Callers retain GC roots.
use crate::{Engine, ObjRef, Throwable, VmExit};
use alloc::string::ToString;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    String(ObjRef),
    Hash(ObjRef),
    Equals(ObjRef, ObjRef),
}
pub(crate) fn with<T>(
    vm: &mut Engine,
    key: Key,
    operation: impl FnOnce(&mut Engine) -> Result<T, VmExit>,
) -> Result<T, VmExit> {
    #[cfg(feature = "desktop-runtime")]
    let owner = 0;
    #[cfg(not(feature = "desktop-runtime"))]
    let owner = vm.heap.checked_execution_owner()?;
    let (receiver, name, sig) = match key {
        Key::String(r) => (r, "toString", "()Ljava/lang/String;"),
        Key::Hash(r) => (r, "hashCode", "()I"),
        Key::Equals(r, _) => (r, "equals", "(Ljava/lang/Object;)Z"),
    };
    let class = vm.heap.class_of(receiver)?.to_string();
    if vm
        .cp
        .resolve_method(&class, name, sig)
        .is_some_and(|(_, _, method)| method.access_flags & crate::engine::ACC_ABSTRACT == 0)
    {
        // Guest callbacks may legitimately mutate state and reenter a value
        // method a finite number of times. They interrupt the pure built-in
        // collection graph; do not mistake that for an immutable graph cycle.
        let suspended = vm.value_recursion.remove(&owner);
        let result = operation(vm);
        if let Some(suspended) = suspended {
            vm.value_recursion.insert(owner, suspended);
        }
        return result;
    }
    if crate::collection_factories::snapshot(vm, receiver)?.is_none() {
        return operation(vm);
    }
    let calls = vm.value_recursion.entry(owner).or_default();
    if calls.contains(&key) {
        return Err(Throwable::new(
            "Ljava/lang/StackOverflowError;",
            "cyclic Java value callback",
        )
        .into());
    }
    calls.push(key);
    let result = operation(vm);
    if let Some(calls) = vm.value_recursion.get_mut(&owner) {
        calls.pop();
        if calls.is_empty() {
            vm.value_recursion.remove(&owner);
        }
    }
    result
}
