//! RustyDroid's immutable Collections factories over the active collection owner.
//!
//! Extracted from intrinsics.rs. Snapshot wrappers preserve the inherited
//! limitation; this is not a claim of live JDK unmodifiable views. A bound
//! framework must use its existing store, never fall back to HostState.
use crate::{framework_host::HostMethodKind, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{string::ToString, vec, vec::Vec};

pub const CLASS: &str = "Ljava/util/Collections;";
pub const LIST: &str = "Ljava/util/Collections$SingletonList;";
pub const SET: &str = "Ljava/util/Collections$UnmodifiableSet;";
pub const MAP: &str = "Ljava/util/Collections$UnmodifiableMap;";

/// An operation-local snapshot, not an additional collection object store.
#[derive(Clone, Debug)]
pub enum Contents {
    List(Vec<Value>),
    Set(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    if class != CLASS {
        return None;
    }
    matches!(
        (name, sig),
        ("singletonList", "(Ljava/lang/Object;)Ljava/util/List;")
            | ("singleton", "(Ljava/lang/Object;)Ljava/util/Set;")
            | ("emptyList", "()Ljava/util/List;")
            | ("emptySet", "()Ljava/util/Set;")
            | ("emptyMap", "()Ljava/util/Map;")
            | ("unmodifiableList", "(Ljava/util/List;)Ljava/util/List;")
            | (
                "unmodifiableCollection",
                "(Ljava/util/Collection;)Ljava/util/Collection;"
            )
            | ("unmodifiableSet", "(Ljava/util/Set;)Ljava/util/Set;")
            | ("unmodifiableMap", "(Ljava/util/Map;)Ljava/util/Map;")
    )
    .then_some(HostMethodKind::Static)
}

pub fn static_field_descriptor(class: &str, name: &str) -> Option<&'static str> {
    if class != CLASS {
        return None;
    }
    match name {
        "EMPTY_LIST" => Some("Ljava/util/List;"),
        "EMPTY_SET" => Some("Ljava/util/Set;"),
        "EMPTY_MAP" => Some("Ljava/util/Map;"),
        _ => None,
    }
}

fn snapshot(vm: &Engine, object: ObjRef) -> Result<Option<Contents>, VmExit> {
    if let Some(host) = vm.framework_host() {
        return host.collection_snapshot(vm, object);
    }
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    {
        use crate::framework::HostObj;
        Ok(match vm.fw.objects.get(&object) {
            Some(HostObj::List { elems }) => Some(Contents::List(elems.clone())),
            Some(HostObj::Map { entries }) => Some(Contents::Map(entries.clone())),
            _ => None,
        })
    }
    #[cfg(not(any(feature = "desktop-runtime", feature = "upstream-runtime")))]
    Ok(None)
}

pub(crate) fn create(vm: &mut Engine, class: &str, contents: Contents) -> Result<ObjRef, VmExit> {
    if let Some(host) = vm.framework_host() {
        return host.collection_create(vm, class, contents);
    }
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    {
        use crate::framework::HostObj;
        let state = match contents {
            Contents::List(elems) | Contents::Set(elems) => HostObj::List { elems },
            Contents::Map(entries) => HostObj::Map { entries },
        };
        let object = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
        vm.fw.objects.insert(object, state);
        Ok(object)
    }
    #[cfg(not(any(feature = "desktop-runtime", feature = "upstream-runtime")))]
    {
        let _ = (class, contents);
        Err(crate::err::vm_error("API_UNAVAILABLE", "collection owner is not bound").into())
    }
}

pub fn static_field(vm: &mut Engine, class: &str, name: &str) -> Result<Option<Value>, VmExit> {
    if static_field_descriptor(class, name).is_none() {
        return Ok(None);
    }
    let key = (class.to_string(), name.to_string());
    if let Some(value) = vm.statics.get(&key) {
        return Ok(Some(value.clone()));
    }
    let (class, contents) = match name {
        "EMPTY_LIST" => (LIST, Contents::List(Vec::new())),
        "EMPTY_SET" => (SET, Contents::Set(Vec::new())),
        _ => (MAP, Contents::Map(Vec::new())),
    };
    let value = Value::Obj(create(vm, class, contents)?);
    vm.statics.insert(key, value.clone());
    Ok(Some(value))
}

pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Preserve the reference's sorted-factory behavior without exposing a
    // plain Set/Map as a functioning SortedSet/SortedMap on the device.
    let reference_sorted = cfg!(any(
        feature = "desktop-runtime",
        feature = "upstream-runtime"
    )) && vm.framework_host().is_none()
        && class == CLASS
        && matches!(
            (name, sig),
            ("emptySortedSet", "()Ljava/util/SortedSet;")
                | ("emptySortedMap", "()Ljava/util/SortedMap;")
        );
    if method_metadata(class, name, sig).is_none() && !reference_sorted {
        return Ok(None);
    }
    let expected = usize::from(!name.starts_with("empty"));
    if args.len() != expected {
        return Err(
            crate::err::vm_error("INVALID_ARG", "Collections factory argument count").into(),
        );
    }
    crate::shared_call::with_values(vm, args, |vm| {
        // DEX represents a null object argument as const/4 zero.
        let element = || match args.first() {
            Some(Value::Int(0)) | Some(Value::Null) | None => Value::Null,
            Some(value) => value.clone(),
        };
        let (class, contents) = match name {
            "singletonList" => (LIST, Contents::List(vec![element()])),
            "singleton" => (SET, Contents::Set(vec![element()])),
            "emptyList" => (LIST, Contents::List(Vec::new())),
            "emptySet" | "emptySortedSet" => (SET, Contents::Set(Vec::new())),
            "emptyMap" | "emptySortedMap" => (MAP, Contents::Map(Vec::new())),
            _ => {
                let Some(Value::Obj(source)) = args.first() else {
                    return Err(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        alloc::format!("Collections.{name} with null collection"),
                    )
                    .into());
                };
                match (name, snapshot(vm, *source)?) {
                    ("unmodifiableMap", Some(Contents::Map(entries))) => {
                        (MAP, Contents::Map(entries))
                    }
                    ("unmodifiableSet", Some(Contents::List(elems) | Contents::Set(elems))) => {
                        (SET, Contents::Set(elems))
                    }
                    (
                        "unmodifiableList" | "unmodifiableCollection",
                        Some(Contents::List(elems) | Contents::Set(elems)),
                    ) => (LIST, Contents::List(elems)),
                    // Preserve the desktop reference's documented DEX fallback.
                    // A device adapter must not claim an immutable wrapper by
                    // returning a mutable, unhandled DEX collection unchanged.
                    ("unmodifiableList" | "unmodifiableCollection", None)
                        if vm.framework_host().is_none() =>
                    {
                        return Ok(Some(Value::Obj(*source)))
                    }
                    _ => {
                        return Err(crate::err::vm_error(
                            "VM_TYPE_ERROR",
                            alloc::format!(
                                "Collections.{name}: source has no compatible collection state"
                            ),
                        )
                        .into())
                    }
                }
            }
        };
        Ok(Some(Value::Obj(create(vm, class, contents)?)))
    })
}

/// Only implemented mutations are catalogued here. Unknown overloads stay
/// unresolved instead of becoming a successful operation on an immutable view.
pub fn rejects_mutation(class: &str, name: &str, sig: &str) -> bool {
    let common = matches!((name, sig), ("clear", "()V"));
    match class {
        LIST | SET => {
            common
                || matches!(
                    (name, sig),
                    ("add", "(Ljava/lang/Object;)Z")
                        | ("set", "(ILjava/lang/Object;)Ljava/lang/Object;")
                        | ("remove", "(I)Ljava/lang/Object;")
                )
        }
        MAP => {
            common
                || matches!(
                    (name, sig),
                    (
                        "put",
                        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
                    ) | ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;")
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    struct NoStore;
    impl crate::framework_host::FrameworkHost for NoStore {
        fn attach(&self, _: &mut Engine, _: &str, _: ObjRef) -> Result<bool, VmExit> {
            Ok(false)
        }
        fn call_static(
            &self,
            _: &mut Engine,
            _: &str,
            _: &str,
            _: &str,
            _: &[Value],
        ) -> Result<Option<Value>, VmExit> {
            Ok(None)
        }
        fn call(
            &self,
            _: &mut Engine,
            _: &str,
            _: &str,
            _: &str,
            _: ObjRef,
            _: &[Value],
        ) -> Result<Option<Value>, VmExit> {
            Ok(None)
        }
        fn roots(&self) -> Vec<ObjRef> {
            Vec::new()
        }
    }
    #[test]
    fn bound_framework_never_falls_back_to_reference_collection_storage() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        vm.bind_framework(Rc::new(NoStore));
        let before = vm.heap.len();
        let error =
            call_static(&mut vm, CLASS, "emptyList", "()Ljava/util/List;", &[]).unwrap_err();
        assert!(alloc::format!("{error:?}").contains("API_UNAVAILABLE"));
        assert_eq!(vm.heap.len(), before);
    }
    #[test]
    fn malformed_factory_call_does_not_index_missing_arguments() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let error = call_static(
            &mut vm,
            CLASS,
            "singletonList",
            "(Ljava/lang/Object;)Ljava/util/List;",
            &[],
        )
        .unwrap_err();
        assert!(alloc::format!("{error:?}").contains("INVALID_ARG"));
    }
}
