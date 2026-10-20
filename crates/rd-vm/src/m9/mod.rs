//! M9 advanced platform components. One state model for app calls and agent APIs.
mod accessibility;
mod ime;
mod notifications;
mod shortcuts;
mod widgets;
use crate::framework::HostObj;
use crate::{Engine, ObjRef, Throwable, Value, VmExit};
pub use accessibility::{AccessibilityEvent, AccessibleNode};
pub use ime::Editor;
pub use notifications::{Action, Channel, Notification};
pub use shortcuts::Shortcut;
use std::collections::{BTreeMap, BTreeSet};
pub use widgets::{RemoteOp, RemoteViews, Widget};
#[derive(Debug)]
pub struct State {
    pub target_sdk: u32,
    pub notifications_allowed: bool,
    pub channels: BTreeMap<String, Channel>,
    pub channel_objects: BTreeMap<ObjRef, String>,
    pub channel_values: BTreeMap<ObjRef, Channel>,
    pub action_objects: BTreeMap<ObjRef, Action>,
    pub notification_objects: BTreeMap<ObjRef, Notification>,
    pub builders: BTreeMap<ObjRef, Notification>,
    pub active: BTreeMap<(String, i32), Notification>,
    pub listeners: Vec<ObjRef>,
    pub statusbar: BTreeMap<ObjRef, Notification>,
    pub pending: BTreeMap<ObjRef, Pending>,
    pub pending_keys: BTreeMap<String, ObjRef>,
    pub notification_sequence: u64,
    pub focused: Option<ObjRef>,
    pub keyboard_visible: bool,
    pub editors: BTreeMap<ObjRef, Editor>,
    pub connections: BTreeMap<ObjRef, ObjRef>,
    pub ime_service: Option<ObjRef>,
    pub editor_actions: Vec<i32>,
    pub remote_views: BTreeMap<ObjRef, RemoteViews>,
    pub widgets: BTreeMap<i32, Widget>,
    pub next_widget: i32,
    pub widget_hosts: BTreeMap<ObjRef, i32>,
    pub widget_provider: Option<ObjRef>,
    pub accessibility_enabled: bool,
    pub accessibility_services: Vec<ObjRef>,
    pub accessibility_events: Vec<AccessibilityEvent>,
    pub nodes: BTreeMap<ObjRef, ObjRef>,
    pub event_objects: BTreeMap<ObjRef, AccessibilityEvent>,
    pub accessibility_focus: Option<ObjRef>,
    pub descriptions: BTreeMap<ObjRef, String>,
    pub shortcuts: BTreeMap<String, Shortcut>,
    pub shortcut_builders: BTreeMap<ObjRef, Shortcut>,
    pub shortcut_objects: BTreeMap<ObjRef, Shortcut>,
    pub pinned: BTreeSet<String>,
    pub service_objects: BTreeMap<String, ObjRef>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            target_sdk: 26,
            notifications_allowed: true,
            channels: Default::default(),
            channel_objects: Default::default(),
            channel_values: Default::default(),
            action_objects: Default::default(),
            notification_objects: Default::default(),
            builders: Default::default(),
            active: Default::default(),
            listeners: Vec::new(),
            statusbar: Default::default(),
            pending: Default::default(),
            pending_keys: Default::default(),
            notification_sequence: 0,
            focused: None,
            keyboard_visible: false,
            editors: Default::default(),
            connections: Default::default(),
            ime_service: None,
            editor_actions: Vec::new(),
            remote_views: Default::default(),
            widgets: Default::default(),
            next_widget: 1,
            widget_hosts: Default::default(),
            widget_provider: None,
            accessibility_enabled: false,
            accessibility_services: Vec::new(),
            accessibility_events: Vec::new(),
            nodes: Default::default(),
            event_objects: Default::default(),
            accessibility_focus: None,
            descriptions: Default::default(),
            shortcuts: Default::default(),
            shortcut_builders: Default::default(),
            shortcut_objects: Default::default(),
            pinned: Default::default(),
            service_objects: Default::default(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Pending {
    pub kind: String,
    pub request: i32,
    pub intent: ObjRef,
    pub context: ObjRef,
    pub one_shot: bool,
    pub immutable: bool,
    pub cancelled: bool,
}
pub(super) fn error(class: &str, msg: impl Into<String>) -> VmExit {
    Throwable::new(class, msg).into()
}
pub(super) fn illegal(msg: impl Into<String>) -> VmExit {
    error("Ljava/lang/IllegalArgumentException;", msg)
}
pub(super) fn alloc(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.into(), Vec::new())?)
}
pub(super) fn string(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(vm.heap.alloc_string(s.into())?))
}
pub(super) fn obj(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
pub(super) fn int(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
pub(super) fn long(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Long(v)) => *v,
        Some(Value::Int(v)) => i64::from(*v),
        _ => 0,
    }
}
pub(super) fn text(vm: &Engine, args: &[Value], i: usize) -> Result<String, VmExit> {
    obj(args, i)
        .map(|r| vm.heap.as_str(r).map(str::to_string).map_err(VmExit::from))
        .unwrap_or_else(|| Err(error("Ljava/lang/NullPointerException;", "text")))
}
pub(super) fn list(vm: &mut Engine, values: Vec<Value>) -> Result<Value, VmExit> {
    let r = alloc(vm, "Ljava/util/ArrayList;")?;
    vm.fw.objects.insert(r, HostObj::List { elems: values });
    Ok(Value::Obj(r))
}
pub(super) fn array(vm: &mut Engine, values: Vec<Value>, class: &str) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_array(crate::heap::ElemKind::Obj, values.len(), class)?;
    *vm.heap.array_elems_mut(r)? = values;
    Ok(Value::Obj(r))
}
pub(super) fn call_dex(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, VmExit> {
    let c = vm.heap.class_of(recv)?.to_string();
    let Some((d, def, m)) = vm.cp.resolve_method(&c, name, sig) else {
        return Ok(None);
    };
    let mut ins = vec![Value::Obj(recv)];
    ins.extend(args);
    vm.call(d, def, &m, ins).map(Some)
}
pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_static_rooted(vm, class, name, sig, args))
}

fn call_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(v) = notifications::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = widgets::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    accessibility::static_call(vm, class, name, sig, args)
}
pub fn call_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_instance_rooted(vm, class, name, sig, recv, args))
}

fn call_instance_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "getSystemService" {
        let key = if sig.contains("Ljava/lang/Class;") {
            obj(args, 0)
                .and_then(|r| vm.heap.get_field(r, "class_name").ok())
                .and_then(|v| {
                    if let Value::Obj(r) = v {
                        vm.heap.as_str(r).ok().map(str::to_string)
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        } else {
            text(vm, args, 0)?
        };
        let cls = match key.as_str() {
            "notification" | "Landroid/app/NotificationManager;" => {
                Some("Landroid/app/NotificationManager;")
            }
            "input_method" | "Landroid/view/inputmethod/InputMethodManager;" => {
                Some("Landroid/view/inputmethod/InputMethodManager;")
            }
            "accessibility" | "Landroid/view/accessibility/AccessibilityManager;" => {
                Some("Landroid/view/accessibility/AccessibilityManager;")
            }
            "appwidget" | "Landroid/appwidget/AppWidgetManager;" => {
                Some("Landroid/appwidget/AppWidgetManager;")
            }
            "shortcut" | "Landroid/content/pm/ShortcutManager;" => {
                Some("Landroid/content/pm/ShortcutManager;")
            }
            _ => None,
        };
        if let Some(cls) = cls {
            let r = if let Some(r) = vm.fw.m9.service_objects.get(cls) {
                *r
            } else {
                let r = alloc(vm, cls)?;
                vm.fw.m9.service_objects.insert(cls.into(), r);
                r
            };
            return Ok(Some(Value::Obj(r)));
        }
    }
    if let Some(v) = notifications::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = ime::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = widgets::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = accessibility::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    shortcuts::instance_call(vm, class, name, sig, recv, args)
}
impl Engine {
    pub fn m9_set_notification_permission(&mut self, allowed: bool) {
        self.fw.m9.notifications_allowed = allowed;
    }
    pub fn m9_send_pending(&mut self, pending: ObjRef) -> Result<(), VmExit> {
        let p = self
            .fw
            .m9
            .pending
            .get(&pending)
            .cloned()
            .ok_or_else(|| illegal("unknown PendingIntent"))?;
        if p.cancelled {
            return Err(error(
                "Landroid/app/PendingIntent$CanceledException;",
                "cancelled PendingIntent",
            ));
        }
        if p.one_shot {
            self.fw.m9.pending.get_mut(&pending).unwrap().cancelled = true;
        }
        let method = match p.kind.as_str() {
            "activity" => "startActivity",
            "broadcast" => "sendBroadcast",
            _ => "startService",
        };
        if method == "startActivity" {
            crate::framework::call_host_instance(
                self,
                "Landroid/content/Context;",
                method,
                "(Landroid/content/Intent;)V",
                p.context,
                &[Value::Obj(p.intent)],
            )?
            .ok_or_else(|| illegal("PendingIntent activity dispatch unsupported"))?;
        } else {
            crate::m8::call_m8_instance(
                self,
                "Landroid/content/Context;",
                method,
                "(Landroid/content/Intent;)V",
                p.context,
                &[Value::Obj(p.intent)],
            )?
            .ok_or_else(|| illegal("PendingIntent dispatch unsupported"))?;
        }
        Ok(())
    }
}

impl Engine {
    /// Privileged agent activation, after manifest/type checks by the caller.
    pub fn m9_start_component(&mut self, recv: ObjRef, role: &str) -> Result<(), VmExit> {
        call_dex(self, recv, "<init>", "()V", vec![])?;
        call_dex(self, recv, "onCreate", "()V", vec![])?;
        match role {
            "ime" => self.m9_register_ime(recv),
            "accessibility" => self.m9_register_accessibility_service(recv),
            "notification_listener" => self.m9_register_notification_listener(recv),
            "widget" => {
                self.fw.m9.widget_provider = Some(recv);
                Ok(())
            }
            _ => Err(illegal("unknown component role")),
        }
    }
}
