//! Android `Observable` / `DataSetObservable` host implementation.
//!
//! PagerAdapter and several AndroidX widgets use DataSetObservable to notify
//! DEX-defined observers. Keep the observer collection as a host ArrayList and
//! invoke callbacks through normal virtual dispatch so app overrides run.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

const OBSERVABLE: &str = "Landroid/database/Observable;";
const DATA_SET_OBSERVABLE: &str = "Landroid/database/DataSetObservable;";
const OBSERVER_ARG_SIG: &str = "(Landroid/database/DataSetObserver;)V";
const OBSERVER_ERASED_SIG: &str = "(Ljava/lang/Object;)V";

fn null_observer() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/NullPointerException;",
        "observer must not be null",
    ))
}

fn illegal_observer(message: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/IllegalStateException;", message))
}

fn observer_list(vm: &mut Engine, owner: ObjRef) -> Result<ObjRef, VmExit> {
    if let Ok(Value::Obj(list)) = vm.heap.get_field(owner, "mObservers") {
        if matches!(vm.fw.objects.get(&list), Some(HostObj::List { .. })) {
            return Ok(list);
        }
    }
    let list = vm
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())?;
    vm.fw
        .objects
        .insert(list, HostObj::List { elems: Vec::new() });
    crate::err::put_field_result(vm.heap.put_field(owner, "mObservers", Value::Obj(list)))?;
    Ok(list)
}

fn observer_values(vm: &Engine, owner: ObjRef) -> Vec<ObjRef> {
    let list = match vm.heap.get_field(owner, "mObservers") {
        Ok(Value::Obj(list)) => list,
        _ => return Vec::new(),
    };
    match vm.fw.objects.get(&list) {
        Some(HostObj::List { elems }) => elems
            .iter()
            .filter_map(|value| value.as_ref().ok().flatten())
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(mref_class, OBSERVABLE | DATA_SET_OBSERVABLE) {
        return Ok(None);
    }
    match (name, sig) {
        ("<init>", "()V") => {
            observer_list(vm, recv)?;
            Ok(Some(Value::Null))
        }
        ("registerObserver", OBSERVER_ARG_SIG | OBSERVER_ERASED_SIG) => {
            let Some(observer) = args.first().and_then(|value| value.as_ref().ok().flatten())
            else {
                return Err(null_observer());
            };
            let list = observer_list(vm, recv)?;
            let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) else {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "Observable.mObservers is not a host list",
                )
                .into());
            };
            if elems.contains(&Value::Obj(observer)) {
                return Err(illegal_observer("Observer is already registered."));
            }
            elems.push(Value::Obj(observer));
            Ok(Some(Value::Null))
        }
        ("unregisterObserver", OBSERVER_ARG_SIG | OBSERVER_ERASED_SIG) => {
            let Some(observer) = args.first().and_then(|value| value.as_ref().ok().flatten())
            else {
                return Err(null_observer());
            };
            let list = observer_list(vm, recv)?;
            let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) else {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "Observable.mObservers is not a host list",
                )
                .into());
            };
            let Some(index) = elems
                .iter()
                .position(|value| *value == Value::Obj(observer))
            else {
                return Err(illegal_observer("Observer was not registered."));
            };
            elems.remove(index);
            Ok(Some(Value::Null))
        }
        ("unregisterAll", "()V") => {
            let list = observer_list(vm, recv)?;
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
                elems.clear();
            }
            Ok(Some(Value::Null))
        }
        ("hasObservers", "()Z") => Ok(Some(Value::Int(i32::from(
            !observer_values(vm, recv).is_empty(),
        )))),
        ("notifyChanged", "()V") if mref_class == DATA_SET_OBSERVABLE => {
            notify(vm, recv, "onChanged")?;
            Ok(Some(Value::Null))
        }
        ("notifyInvalidated", "()V") if mref_class == DATA_SET_OBSERVABLE => {
            notify(vm, recv, "onInvalidated")?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

fn notify(vm: &mut Engine, owner: ObjRef, method: &str) -> Result<(), VmExit> {
    // AOSP traverses the observer list in reverse registration order.
    for observer in observer_values(vm, owner).into_iter().rev() {
        vm.invoke_instance(observer, method, "()V", &[])?;
    }
    Ok(())
}
