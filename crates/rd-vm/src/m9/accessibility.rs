use super::*;
use crate::framework::{HostObj, Vis};
#[cfg(not(feature = "std"))]
#[allow(unused_imports)] // Explicit alloc prelude for the inherited module.
use crate::port_prelude::*;
use serde::{Deserialize, Serialize};
const NODE: &str = "Landroid/view/accessibility/AccessibilityNodeInfo;";
const EVENT: &str = "Landroid/view/accessibility/AccessibilityEvent;";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityEvent {
    pub event_type: i32,
    pub source: ObjRef,
    pub text: String,
    pub time_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleNode {
    pub id: ObjRef,
    pub class: String,
    pub text: String,
    pub description: String,
    pub bounds: (i32, i32, i32, i32),
    pub enabled: bool,
    pub clickable: bool,
    pub editable: bool,
    pub focused: bool,
    pub accessibility_focused: bool,
    pub children: Vec<ObjRef>,
}
fn root(vm: &Engine) -> Option<ObjRef> {
    let a = vm.fw.current_activity?;
    let w = match vm.fw.get(a)? {
        HostObj::Activity { window, .. } => (*window)?,
        _ => return None,
    };
    match vm.fw.get(w)? {
        HostObj::Window { content } => *content,
        _ => None,
    }
}
fn node(vm: &mut Engine, view: ObjRef) -> Result<Value, VmExit> {
    let r = alloc(vm, NODE)?;
    vm.fw.m9.nodes.insert(r, view);
    Ok(Value::Obj(r))
}
impl Engine {
    pub fn m9_accessible_nodes(&self) -> Vec<AccessibleNode> {
        let mut out = Vec::new();
        let Some(r) = root(self) else {
            return out;
        };
        let mut stack = vec![(r, 0)];
        let mut seen = BTreeSet::new();
        while let Some((v, depth)) = stack.pop() {
            if depth > 256 || !seen.insert(v) {
                continue;
            }
            if let Some(HostObj::View {
                text,
                vis: Vis::Visible,
                enabled,
                click_listener,
                click_method,
                x,
                y,
                w,
                h,
                children,
                ..
            }) = self.fw.get(v)
            {
                let class = self
                    .heap
                    .class_of(v)
                    .unwrap_or("Landroid/view/View;")
                    .to_string();
                let editable = self.cp.is_subtype(&class, "Landroid/widget/EditText;");
                let visible_children = children
                    .iter()
                    .copied()
                    .filter(|r| {
                        matches!(
                            self.fw.get(*r),
                            Some(HostObj::View {
                                vis: Vis::Visible,
                                ..
                            })
                        )
                    })
                    .collect::<Vec<_>>();
                out.push(AccessibleNode {
                    id: v,
                    class,
                    text: text.clone(),
                    description: self.fw.m9.descriptions.get(&v).cloned().unwrap_or_default(),
                    bounds: (*x, *y, *w, *h),
                    enabled: *enabled,
                    clickable: click_listener.is_some() || click_method.is_some(),
                    editable,
                    focused: self.fw.m9.focused == Some(v),
                    accessibility_focused: self.fw.m9.accessibility_focus == Some(v),
                    children: visible_children.clone(),
                });
                for c in visible_children.into_iter().rev() {
                    stack.push((c, depth + 1));
                }
            }
        }
        out
    }
    pub fn m9_register_accessibility_service(&mut self, r: ObjRef) -> Result<(), VmExit> {
        self.fw.m9.accessibility_enabled = true;
        if !self.fw.m9.accessibility_services.contains(&r) {
            self.fw.m9.accessibility_services.push(r);
            call_dex(self, r, "onServiceConnected", "()V", vec![])?;
        }
        Ok(())
    }
    pub fn m9_accessibility_event(
        &mut self,
        source: ObjRef,
        event_type: i32,
    ) -> Result<(), VmExit> {
        let e = AccessibilityEvent {
            source,
            event_type,
            text: self.view_text(source).unwrap_or_default(),
            time_ms: self.fw.clock_ms,
        };
        if self.fw.m9.accessibility_events.len() >= 4096 {
            self.fw.m9.accessibility_events.remove(0);
        }
        self.fw.m9.accessibility_events.push(e.clone());
        for service in self.fw.m9.accessibility_services.clone() {
            let r = alloc(self, EVENT)?;
            self.fw.m9.event_objects.insert(r, e.clone());
            call_dex(
                self,
                service,
                "onAccessibilityEvent",
                "(Landroid/view/accessibility/AccessibilityEvent;)V",
                vec![Value::Obj(r)],
            )?;
        }
        Ok(())
    }
    pub fn m9_accessibility_action(
        &mut self,
        id: ObjRef,
        action: i32,
        text: Option<&str>,
    ) -> Result<bool, VmExit> {
        let n = self
            .m9_accessible_nodes()
            .into_iter()
            .find(|n| n.id == id)
            .ok_or_else(|| illegal("accessibility node not visible in active window"))?;
        if !n.enabled {
            return Ok(false);
        }
        match action {
            16 => {
                let clicked = self.dispatch_click(id)?;
                if clicked {
                    self.m9_accessibility_event(id, 1)?;
                }
                Ok(clicked)
            }
            1 => {
                if !n.editable {
                    return Ok(false);
                }
                self.m9_focus(id)?;
                Ok(true)
            }
            2 => {
                if self.fw.m9.focused == Some(id) {
                    self.fw.m9.focused = None;
                }
                Ok(true)
            }
            64 => {
                self.fw.m9.accessibility_focus = Some(id);
                self.m9_accessibility_event(id, 32768)?;
                Ok(true)
            }
            128 => {
                if self.fw.m9.accessibility_focus == Some(id) {
                    self.fw.m9.accessibility_focus = None;
                    self.m9_accessibility_event(id, 65536)?;
                }
                Ok(true)
            }
            2097152 => {
                if !n.editable {
                    return Ok(false);
                }
                let s = text.ok_or_else(|| illegal("ACTION_SET_TEXT requires text"))?;
                self.m9_focus(id)?;
                self.m9_set_selection(0, n.text.encode_utf16().count())?;
                self.m9_commit_text(s, 1, false)?;
                Ok(true)
            }
            _ => Err(crate::err::vm_error(
                "NOT_IMPLEMENTED",
                format!("accessibility action {action}"),
            )
            .into()),
        }
    }
}
pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == NODE && name == "obtain" {
        return crate::accessibility_node::obtain(vm, args).map(Some);
    }
    if class.starts_with("Landroid/view/accessibility/AccessibilityNodeInfo$") && name == "obtain" {
        let r = alloc(vm, class)?;
        for (i, v) in args.iter().enumerate() {
            crate::err::put_field_result(vm.heap.put_field(r, &format!("arg{i}"), v.clone()))?;
        }
        return Ok(Some(Value::Obj(r)));
    }
    if class == EVENT && name == "obtain" {
        let r = alloc(vm, EVENT)?;
        vm.fw.m9.event_objects.insert(
            r,
            AccessibilityEvent {
                event_type: int(args, 0),
                source: 0,
                text: String::new(),
                time_ms: vm.fw.clock_ms,
            },
        );
        return Ok(Some(Value::Obj(r)));
    }
    Ok(None)
}
pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == "Landroid/view/accessibility/AccessibilityNodeInfo$AccessibilityAction;" {
        let v = match name {
            "<init>" => {
                for (key, value) in ["Id", "Label"].into_iter().zip(args) {
                    crate::err::put_field_result(vm.heap.put_field(recv, key, value.clone()))?;
                }
                Value::Null
            }
            "getId" => vm.heap.get_field(recv, "Id").unwrap_or(Value::Int(0)),
            "getLabel" => vm.heap.get_field(recv, "Label").unwrap_or(Value::Null),
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == NODE && !vm.fw.m9.nodes.contains_key(&recv) {
        return crate::accessibility_node::call(vm, recv, name, _sig, args);
    }
    if name == "onInitializeAccessibilityNodeInfo"
        && matches!(vm.fw.get(recv), Some(HostObj::View { .. }))
    {
        if let Some(Value::Obj(n)) = args.first() {
            crate::accessibility_node::initialize_view(vm, recv, *n)?;
        }
        return Ok(Some(Value::Null));
    }
    if name == "getParentForAccessibility" {
        if let Some(HostObj::View { parent, .. }) = vm.fw.get(recv) {
            return Ok(Some(parent.map(Value::Obj).unwrap_or(Value::Null)));
        }
    }
    if name == "setContentDescription" && matches!(vm.fw.get(recv), Some(HostObj::View { .. })) {
        let s = if obj(args, 0).is_some() {
            text(vm, args, 0)?
        } else {
            String::new()
        };
        vm.fw.m9.descriptions.insert(recv, s);
        return Ok(Some(Value::Null));
    }
    if name == "getContentDescription"
        && class != NODE
        && class != EVENT
        && matches!(vm.fw.get(recv), Some(HostObj::View { .. }))
    {
        return Ok(Some(string(
            vm,
            vm.fw
                .m9
                .descriptions
                .get(&recv)
                .cloned()
                .unwrap_or_default(),
        )?));
    }
    if class == "Landroid/accessibilityservice/AccessibilityService;" {
        let v = match name {
            "<init>" => Value::Null,
            "onCreate" => {
                vm.m9_register_accessibility_service(recv)?;
                Value::Null
            }
            "onDestroy" => {
                vm.fw.m9.accessibility_services.retain(|r| *r != recv);
                vm.fw.m9.accessibility_enabled = !vm.fw.m9.accessibility_services.is_empty();
                Value::Null
            }
            "onServiceConnected" | "onInterrupt" => Value::Null,
            "getRootInActiveWindow" => {
                if let Some(r) = root(vm) {
                    node(vm, r)?
                } else {
                    Value::Null
                }
            }
            "performGlobalAction" => {
                if int(args, 0) != 1 {
                    return Err(crate::err::vm_error(
                        "NOT_IMPLEMENTED",
                        "accessibility global action other than BACK",
                    )
                    .into());
                }
                vm.key_event("BACK")?;
                Value::Int(1)
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == "Landroid/view/accessibility/AccessibilityManager;" {
        let v = match name {
            "isEnabled" | "isTouchExplorationEnabled" => {
                Value::Int(vm.fw.m9.accessibility_enabled as i32)
            }
            "sendAccessibilityEvent" => {
                let e = obj(args, 0)
                    .and_then(|r| vm.fw.m9.event_objects.get(&r).cloned())
                    .ok_or_else(|| illegal("event absent"))?;
                vm.m9_accessibility_event(e.source, e.event_type)?;
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == NODE {
        let view = *vm
            .fw
            .m9
            .nodes
            .get(&recv)
            .ok_or_else(|| illegal("recycled/invalid AccessibilityNodeInfo"))?;
        let n = vm
            .m9_accessible_nodes()
            .into_iter()
            .find(|n| n.id == view)
            .ok_or_else(|| illegal("stale accessibility node"))?;
        let v = match name {
            "getText" => string(vm, n.text)?,
            "getClassName" => string(
                vm,
                n.class
                    .trim_start_matches('L')
                    .trim_end_matches(';')
                    .replace('/', "."),
            )?,
            "getPackageName" => string(vm, vm.fw.package_name.clone())?,
            "getContentDescription" => string(vm, n.description)?,
            "getChildCount" => Value::Int(n.children.len() as i32),
            "getChild" => {
                if let Some(c) = n.children.get(int(args, 0) as usize) {
                    node(vm, *c)?
                } else {
                    Value::Null
                }
            }
            "isEnabled" => Value::Int(n.enabled as i32),
            "isClickable" => Value::Int(n.clickable as i32),
            "isEditable" | "isFocusable" => Value::Int(n.editable as i32),
            "isFocused" => Value::Int(n.focused as i32),
            "isAccessibilityFocused" => Value::Int(n.accessibility_focused as i32),
            "isVisibleToUser" => Value::Int(1),
            "refresh" => Value::Int(1),
            "getViewIdResourceName" => {
                let tree = vm.build_ui_node(view, 0);
                if let Some(s) = tree.resource_id {
                    string(vm, s)?
                } else {
                    Value::Null
                }
            }
            "getBoundsInScreen" => {
                let r = obj(args, 0).ok_or_else(|| illegal("Rect null"))?;
                let (x, y, w, h) = n.bounds;
                for (k, v) in [
                    ("left", x),
                    ("top", y),
                    ("right", x.saturating_add(w)),
                    ("bottom", y.saturating_add(h)),
                ] {
                    vm.heap
                        .put_field(r, k, Value::Int(v))
                        .map_err(|e| format!("{e:?}"))?;
                }
                Value::Null
            }
            "performAction" => {
                let arg = obj(args, 1)
                    .and_then(|r| match vm.fw.get(r) {
                        Some(HostObj::Bundle { map }) => map
                            .iter()
                            .find(|(k, _)| k == "ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE")
                            .map(|(_, v)| v.clone()),
                        _ => None,
                    })
                    .and_then(|v| {
                        if let Value::Obj(r) = v {
                            vm.heap.as_str(r).ok().map(str::to_string)
                        } else {
                            None
                        }
                    });
                Value::Int(vm.m9_accessibility_action(view, int(args, 0), arg.as_deref())? as i32)
            }
            "findAccessibilityNodeInfosByText" => {
                let search = text(vm, args, 0)?.to_lowercase();
                let mut vals = Vec::new();
                for n in vm.m9_accessible_nodes() {
                    if n.text.to_lowercase().contains(&search) {
                        vals.push(node(vm, n.id)?);
                    }
                }
                list(vm, vals)?
            }
            "recycle" => {
                vm.fw.m9.nodes.remove(&recv);
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == EVENT {
        let e = vm
            .fw
            .m9
            .event_objects
            .get(&recv)
            .cloned()
            .ok_or_else(|| illegal("invalid AccessibilityEvent"))?;
        let v = match name {
            "getEventType" => Value::Int(e.event_type),
            "getEventTime" => Value::Long(e.time_ms as i64),
            "getSource" => {
                if e.source == 0 {
                    Value::Null
                } else {
                    node(vm, e.source)?
                }
            }
            "getText" => {
                let s = string(vm, e.text)?;
                list(vm, vec![s])?
            }
            "setSource" => {
                vm.fw.m9.event_objects.get_mut(&recv).unwrap().source = obj(args, 0).unwrap_or(0);
                Value::Null
            }
            "recycle" => {
                vm.fw.m9.event_objects.remove(&recv);
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    Ok(None)
}
