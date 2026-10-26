use super::*;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)] // Explicit alloc prelude for the inherited module.
use crate::port_prelude::*;
use serde::{Deserialize, Serialize};
const BUILDER: &str = "Landroid/content/pm/ShortcutInfo$Builder;";
const INFO: &str = "Landroid/content/pm/ShortcutInfo;";
const MANAGER: &str = "Landroid/content/pm/ShortcutManager;";
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Shortcut {
    pub id: String,
    pub short_label: String,
    pub long_label: String,
    pub intent: Option<ObjRef>,
    pub rank: i32,
    pub enabled: bool,
    pub disabled_message: String,
    pub dynamic: bool,
    pub pinned: bool,
}
fn entries(vm: &Engine, r: ObjRef) -> Result<Vec<Value>, VmExit> {
    match vm.fw.get(r) {
        Some(HostObj::List { elems }) => Ok(elems.clone()),
        _ => Err(illegal("shortcut argument must be List")),
    }
}
impl Engine {
    pub fn m9_launch_shortcut(&mut self, id: &str) -> Result<(), VmExit> {
        let s = self
            .fw
            .m9
            .shortcuts
            .get(id)
            .cloned()
            .ok_or_else(|| illegal("shortcut absent"))?;
        if !s.enabled {
            return Err(error(
                "Ljava/lang/IllegalStateException;",
                s.disabled_message,
            ));
        }
        let intent = s.intent.ok_or_else(|| illegal("shortcut intent absent"))?;
        let context = self
            .fw
            .current_activity
            .ok_or_else(|| illegal("shortcut launch requires activity context"))?;
        crate::framework::call_host_instance(
            self,
            "Landroid/content/Context;",
            "startActivity",
            "(Landroid/content/Intent;)V",
            context,
            &[Value::Obj(intent)],
        )?
        .ok_or_else(|| illegal("shortcut Intent cannot be resolved"))?;
        Ok(())
    }
}
pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == BUILDER {
        if name == "<init>" {
            let id = text(vm, args, 1)?;
            if id.is_empty() {
                return Err(illegal("shortcut id empty"));
            }
            vm.fw.m9.shortcut_builders.insert(
                recv,
                Shortcut {
                    id,
                    enabled: true,
                    ..Default::default()
                },
            );
            return Ok(Some(Value::Null));
        }
        if name == "build" {
            let s = vm
                .fw
                .m9
                .shortcut_builders
                .get(&recv)
                .cloned()
                .ok_or_else(|| illegal("uninitialized ShortcutInfo.Builder"))?;
            if s.short_label.is_empty() || s.intent.is_none() {
                return Err(illegal("shortcut requires shortLabel and Intent"));
            }
            let r = alloc(vm, INFO)?;
            vm.fw.m9.shortcut_objects.insert(r, s);
            return Ok(Some(Value::Obj(r)));
        }
        let val = if name == "setShortLabel" || name == "setLongLabel" {
            Some(text(vm, args, 0)?)
        } else {
            None
        };
        let s = vm
            .fw
            .m9
            .shortcut_builders
            .get_mut(&recv)
            .ok_or_else(|| illegal("uninitialized ShortcutInfo.Builder"))?;
        match name {
            "setShortLabel" => s.short_label = val.unwrap(),
            "setLongLabel" => s.long_label = val.unwrap(),
            "setIntent" => s.intent = obj(args, 0),
            "setRank" => {
                if int(args, 0) < 0 {
                    return Err(illegal("negative shortcut rank"));
                }
                s.rank = int(args, 0)
            }
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if class == INFO {
        let s = vm
            .fw
            .m9
            .shortcut_objects
            .get(&recv)
            .cloned()
            .ok_or_else(|| illegal("invalid ShortcutInfo"))?;
        let v = match name {
            "getId" => string(vm, s.id)?,
            "getShortLabel" => string(vm, s.short_label)?,
            "getLongLabel" => string(vm, s.long_label)?,
            "getRank" => Value::Int(s.rank),
            "isEnabled" => Value::Int(s.enabled as i32),
            "isDynamic" => Value::Int(s.dynamic as i32),
            "isPinned" => Value::Int(s.pinned as i32),
            "getIntent" => s.intent.map(Value::Obj).unwrap_or(Value::Null),
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == MANAGER {
        let value = match name {
            "setDynamicShortcuts" | "addDynamicShortcuts" | "updateShortcuts" => {
                let vals = entries(
                    vm,
                    obj(args, 0).ok_or_else(|| illegal("shortcut list null"))?,
                )?;
                let mut incoming = Vec::new();
                for v in vals {
                    let Value::Obj(r) = v else {
                        return Err(illegal("shortcut list item not object"));
                    };
                    incoming.push(
                        vm.fw
                            .m9
                            .shortcut_objects
                            .get(&r)
                            .cloned()
                            .ok_or_else(|| illegal("invalid shortcut"))?,
                    );
                }
                let mut next = vm.fw.m9.shortcuts.clone();
                if name == "setDynamicShortcuts" {
                    for s in next.values_mut() {
                        s.dynamic = false;
                    }
                }
                for mut s in incoming {
                    if name == "updateShortcuts" && !next.contains_key(&s.id) {
                        continue;
                    }
                    if let Some(old) = next.get(&s.id) {
                        s.pinned = old.pinned;
                        s.dynamic = if name == "updateShortcuts" {
                            old.dynamic
                        } else {
                            true
                        };
                    } else {
                        s.dynamic = true;
                    }
                    next.insert(s.id.clone(), s);
                }
                next.retain(|_, s| s.dynamic || s.pinned);
                if next.values().filter(|s| s.dynamic).count() > 5 {
                    return Ok(Some(Value::Int(0)));
                }
                vm.fw.m9.shortcuts = next;
                Value::Int(1)
            }
            "getDynamicShortcuts" | "getPinnedShortcuts" => {
                let mut values = Vec::new();
                let mut shortcuts: Vec<_> = vm
                    .fw
                    .m9
                    .shortcuts
                    .values()
                    .filter(|s| {
                        if name == "getDynamicShortcuts" {
                            s.dynamic
                        } else {
                            s.pinned
                        }
                    })
                    .cloned()
                    .collect();
                shortcuts.sort_by(|a, b| a.rank.cmp(&b.rank).then(a.id.cmp(&b.id)));
                for s in shortcuts {
                    let r = alloc(vm, INFO)?;
                    vm.fw.m9.shortcut_objects.insert(r, s);
                    values.push(Value::Obj(r));
                }
                list(vm, values)?
            }
            "removeAllDynamicShortcuts" => {
                for s in vm.fw.m9.shortcuts.values_mut() {
                    s.dynamic = false;
                }
                vm.fw.m9.shortcuts.retain(|_, s| s.pinned);
                Value::Null
            }
            "removeDynamicShortcuts" | "disableShortcuts" | "enableShortcuts" => {
                let vals = entries(
                    vm,
                    obj(args, 0).ok_or_else(|| illegal("shortcut ids null"))?,
                )?;
                let message = if args.len() > 1 {
                    text(vm, args, 1)?
                } else {
                    String::new()
                };
                for v in vals {
                    let Value::Obj(r) = v else {
                        return Err(illegal("shortcut id not string"));
                    };
                    let id = vm.heap.as_str(r)?.to_string();
                    if let Some(s) = vm.fw.m9.shortcuts.get_mut(&id) {
                        match name {
                            "removeDynamicShortcuts" => s.dynamic = false,
                            "disableShortcuts" => {
                                s.enabled = false;
                                s.disabled_message = message.clone();
                            }
                            _ => s.enabled = true,
                        }
                    }
                }
                vm.fw.m9.shortcuts.retain(|_, s| s.dynamic || s.pinned);
                Value::Null
            }
            "requestPinShortcut" => {
                let r = obj(args, 0).ok_or_else(|| illegal("shortcut null"))?;
                let mut s = vm
                    .fw
                    .m9
                    .shortcut_objects
                    .get(&r)
                    .cloned()
                    .ok_or_else(|| illegal("shortcut invalid"))?;
                if let Some(old) = vm.fw.m9.shortcuts.get(&s.id) {
                    s.dynamic = old.dynamic;
                }
                s.pinned = true;
                vm.fw.m9.pinned.insert(s.id.clone());
                vm.fw.m9.shortcuts.insert(s.id.clone(), s);
                Value::Int(1)
            }
            "isRequestPinShortcutSupported" => Value::Int(1),
            "getMaxShortcutCountPerActivity" => Value::Int(5),
            "isRateLimitingActive" => Value::Int(0),
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    Ok(None)
}
