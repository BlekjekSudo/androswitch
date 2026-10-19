use super::*;
use serde::{Deserialize, Serialize};
const MANAGER: &str = "Landroid/app/NotificationManager;";
const CHANNEL: &str = "Landroid/app/NotificationChannel;";
const NOTIFICATION: &str = "Landroid/app/Notification;";
const PENDING: &str = "Landroid/app/PendingIntent;";
const STATUS: &str = "Landroid/service/notification/StatusBarNotification;";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub importance: i32,
    pub group: Option<String>,
    pub lights: bool,
    pub vibration: bool,
    pub deleted: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Action {
    pub title: String,
    pub icon: i32,
    pub pending: Option<ObjRef>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Notification {
    pub tag: String,
    pub id: i32,
    pub channel: String,
    pub title: String,
    pub text: String,
    pub big_text: String,
    pub subtext: String,
    pub group: Option<String>,
    pub group_summary: bool,
    pub category: String,
    pub small_icon: i32,
    pub ongoing: bool,
    pub auto_cancel: bool,
    pub only_alert_once: bool,
    pub visibility: i32,
    pub priority: i32,
    pub number: i32,
    pub progress: (i32, i32, bool),
    pub content_intent: Option<ObjRef>,
    pub delete_intent: Option<ObjRef>,
    pub actions: Vec<Action>,
    pub timeout_ms: u64,
    pub posted_ms: u64,
    pub expires_ms: Option<u64>,
    pub sequence: u64,
}
fn builder(class: &str) -> bool {
    matches!(
        class,
        "Landroid/app/Notification$Builder;" | "Landroidx/core/app/NotificationCompat$Builder;"
    )
}
fn status(vm: &mut Engine, n: Notification) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, STATUS)?;
    vm.fw.m9.statusbar.insert(r, n);
    Ok(r)
}
impl Engine {
    pub fn m9_notifications(&self) -> Vec<Notification> {
        let mut n: Vec<_> = self.fw.m9.active.values().cloned().collect();
        n.sort_by_key(|n| n.sequence);
        n
    }
    pub fn m9_notify(&mut self, tag: &str, id: i32, mut n: Notification) -> Result<bool, VmExit> {
        if !self.fw.m9.notifications_allowed {
            return Ok(false);
        }
        let c = self.fw.m9.channels.get(&n.channel).ok_or_else(|| {
            illegal(format!(
                "notification channel {:?} does not exist",
                n.channel
            ))
        })?;
        if c.deleted || c.importance == 0 {
            return Ok(false);
        }
        if self.fw.m9.active.len() >= 1024 && !self.fw.m9.active.contains_key(&(tag.into(), id)) {
            return Err(illegal("notification cap exceeded"));
        }
        n.tag = tag.into();
        n.id = id;
        n.posted_ms = self.fw.clock_ms;
        self.fw.m9.notification_sequence = self.fw.m9.notification_sequence.saturating_add(1);
        n.sequence = self.fw.m9.notification_sequence;
        n.expires_ms = if n.timeout_ms == 0 {
            None
        } else {
            Some(self.fw.clock_ms.saturating_add(n.timeout_ms))
        };
        self.fw.m9.active.insert((tag.into(), id), n.clone());
        for l in self.fw.m9.listeners.clone() {
            let sb = status(self, n.clone())?;
            call_dex(
                self,
                l,
                "onNotificationPosted",
                "(Landroid/service/notification/StatusBarNotification;)V",
                vec![Value::Obj(sb)],
            )?;
        }
        Ok(true)
    }
    pub fn m9_cancel_notification(
        &mut self,
        tag: &str,
        id: i32,
        reason: i32,
    ) -> Result<bool, VmExit> {
        let Some(n) = self.fw.m9.active.remove(&(tag.into(), id)) else {
            return Ok(false);
        };
        for l in self.fw.m9.listeners.clone() {
            let sb = status(self, n.clone())?;
            let r=call_dex(self,l,"onNotificationRemoved","(Landroid/service/notification/StatusBarNotification;Landroid/service/notification/NotificationListenerService$RankingMap;I)V",vec![Value::Obj(sb),Value::Null,Value::Int(reason)])?;
            if r.is_none() {
                call_dex(
                    self,
                    l,
                    "onNotificationRemoved",
                    "(Landroid/service/notification/StatusBarNotification;)V",
                    vec![Value::Obj(sb)],
                )?;
            }
        }
        Ok(true)
    }
    pub fn m9_click_notification(
        &mut self,
        tag: &str,
        id: i32,
        action: Option<usize>,
    ) -> Result<(), VmExit> {
        let n = self
            .fw
            .m9
            .active
            .get(&(tag.into(), id))
            .cloned()
            .ok_or_else(|| illegal("notification not found"))?;
        let p = if let Some(a) = action {
            n.actions
                .get(a)
                .ok_or_else(|| illegal("notification action index invalid"))?
                .pending
        } else {
            n.content_intent
        };
        if let Some(p) = p {
            self.m9_send_pending(p)?;
        } else {
            return Err(illegal("notification has no PendingIntent"));
        }
        if action.is_none() && n.auto_cancel {
            self.m9_cancel_notification(tag, id, 1)?;
        }
        Ok(())
    }
    pub fn m9_expire_notifications(&mut self) -> Result<(), VmExit> {
        let due: Vec<_> = self
            .fw
            .m9
            .active
            .iter()
            .filter(|(_, n)| n.expires_ms.map(|t| t <= self.fw.clock_ms).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect();
        for (tag, id) in due {
            self.m9_cancel_notification(&tag, id, 19)?;
        }
        Ok(())
    }
    pub fn m9_register_notification_listener(&mut self, r: ObjRef) -> Result<(), VmExit> {
        if !self.fw.m9.listeners.contains(&r) {
            self.fw.m9.listeners.push(r);
            call_dex(self, r, "onListenerConnected", "()V", vec![])?;
        }
        Ok(())
    }
}
pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != PENDING {
        return Ok(None);
    }
    let kind = match name {
        "getActivity" => "activity",
        "getBroadcast" => "broadcast",
        "getService" | "getForegroundService" => "service",
        _ => return Ok(None),
    };
    let context = obj(args, 0).ok_or_else(|| illegal("PendingIntent context null"))?;
    let request = int(args, 1);
    let intent = obj(args, 2).ok_or_else(|| illegal("PendingIntent intent null"))?;
    let flags = int(args, 3) as u32;
    if flags & 0x06000000 == 0x06000000 {
        return Err(illegal(
            "PendingIntent cannot be both mutable and immutable",
        ));
    }
    if vm.fw.m9.target_sdk >= 31 && flags & 0x06000000 == 0 {
        return Err(illegal(
            "targetSdk >=31 requires PendingIntent mutability flag",
        ));
    }
    let component = match vm.fw.get(intent) {
        Some(HostObj::Intent { component, .. }) => component.clone().unwrap_or_default(),
        _ => String::new(),
    };
    let action = vm
        .fw
        .m8
        .intents
        .get(&intent)
        .and_then(|s| s.action.clone())
        .unwrap_or_default();
    let data = vm
        .fw
        .m8
        .intents
        .get(&intent)
        .and_then(|s| s.data)
        .and_then(|r| vm.fw.m8.uris.get(&r))
        .cloned()
        .unwrap_or_default();
    let key = format!("{kind}|{request}|{component}|{action}|{data}");
    let existing = vm
        .fw
        .m9
        .pending_keys
        .get(&key)
        .copied()
        .filter(|r| !vm.fw.m9.pending[r].cancelled);
    if let Some(r) = existing {
        if flags & 0x10000000 != 0 {
            vm.fw.m9.pending.get_mut(&r).unwrap().cancelled = true;
        } else {
            if flags & 0x08000000 != 0 {
                vm.fw.m9.pending.get_mut(&r).unwrap().intent = intent;
            }
            return Ok(Some(Value::Obj(r)));
        }
    }
    if flags & 0x20000000 != 0 {
        return Ok(Some(Value::Null));
    }
    let r = alloc(vm, PENDING)?;
    vm.fw.m9.pending.insert(
        r,
        Pending {
            kind: kind.into(),
            request,
            intent,
            context,
            one_shot: flags & 0x40000000 != 0,
            immutable: flags & 0x04000000 != 0,
            cancelled: false,
        },
    );
    vm.fw.m9.pending_keys.insert(key, r);
    Ok(Some(Value::Obj(r)))
}
pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == CHANNEL {
        if name == "<init>" {
            let id = text(vm, args, 0)?;
            let n = text(vm, args, 1)?;
            let importance = int(args, 2);
            if id.is_empty() || !(0..=5).contains(&importance) {
                return Err(illegal("invalid notification channel id/importance"));
            }
            vm.fw.m9.channel_objects.insert(recv, id.clone());
            vm.fw.m9.channel_values.insert(
                recv,
                Channel {
                    id,
                    name: n,
                    description: String::new(),
                    importance,
                    group: None,
                    lights: false,
                    vibration: false,
                    deleted: false,
                },
            );
            return Ok(Some(Value::Null));
        }
        let c = vm
            .fw
            .m9
            .channel_values
            .get(&recv)
            .cloned()
            .ok_or_else(|| illegal("uninitialized NotificationChannel"))?;
        let result = match name {
            "getId" => string(vm, c.id)?,
            "getName" => string(vm, c.name)?,
            "getDescription" => string(vm, c.description)?,
            "getImportance" => Value::Int(c.importance),
            "getGroup" => {
                if let Some(g) = c.group {
                    string(vm, g)?
                } else {
                    Value::Null
                }
            }
            "shouldShowLights" => Value::Int(c.lights as i32),
            "shouldVibrate" => Value::Int(c.vibration as i32),
            "setDescription" => {
                let s = text(vm, args, 0)?;
                vm.fw.m9.channel_values.get_mut(&recv).unwrap().description = s;
                Value::Null
            }
            "setName" => {
                let s = text(vm, args, 0)?;
                vm.fw.m9.channel_values.get_mut(&recv).unwrap().name = s;
                Value::Null
            }
            "setGroup" => {
                let g = text(vm, args, 0)?;
                vm.fw.m9.channel_values.get_mut(&recv).unwrap().group = Some(g);
                Value::Null
            }
            "enableLights" => {
                vm.fw.m9.channel_values.get_mut(&recv).unwrap().lights = int(args, 0) != 0;
                Value::Null
            }
            "enableVibration" => {
                vm.fw.m9.channel_values.get_mut(&recv).unwrap().vibration = int(args, 0) != 0;
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(result));
    }
    if builder(class) {
        if name == "<init>" {
            let channel = if args.len() > 1 {
                text(vm, args, 1)?
            } else {
                String::new()
            };
            vm.fw.m9.builders.insert(
                recv,
                Notification {
                    channel,
                    ..Default::default()
                },
            );
            return Ok(Some(Value::Null));
        }
        if matches!(name, "build" | "getNotification") {
            let n = vm
                .fw
                .m9
                .builders
                .get(&recv)
                .cloned()
                .ok_or_else(|| illegal("uninitialized Notification.Builder"))?;
            let r = alloc(vm, NOTIFICATION)?;
            vm.fw.m9.notification_objects.insert(r, n);
            return Ok(Some(Value::Obj(r)));
        }
        if name == "setStyle" {
            if let Some(r) = obj(args, 0) {
                if let Some(style) = vm.fw.m9.builders.get(&r).cloned() {
                    vm.fw
                        .m9
                        .builders
                        .get_mut(&recv)
                        .ok_or_else(|| illegal("uninitialized builder"))?
                        .big_text = style.big_text;
                }
            }
            return Ok(Some(Value::Obj(recv)));
        }
        let val = if matches!(
            name,
            "setContentTitle"
                | "setContentText"
                | "setSubText"
                | "setChannelId"
                | "setGroup"
                | "setCategory"
        ) {
            Some(text(vm, args, 0)?)
        } else {
            None
        };
        let new_action = if name == "addAction" {
            if sig.starts_with("(I") {
                Some(Action {
                    icon: int(args, 0),
                    title: text(vm, args, 1)?,
                    pending: obj(args, 2),
                })
            } else {
                obj(args, 0).and_then(|r| vm.fw.m9.action_objects.get(&r).cloned())
            }
        } else {
            None
        };
        let n = vm
            .fw
            .m9
            .builders
            .get_mut(&recv)
            .ok_or_else(|| illegal("uninitialized Notification.Builder"))?;
        match name {
            "setContentTitle" => n.title = val.unwrap(),
            "setContentText" => n.text = val.unwrap(),
            "setSubText" => n.subtext = val.unwrap(),
            "setChannelId" => n.channel = val.unwrap(),
            "setGroup" => n.group = val,
            "setGroupSummary" => n.group_summary = int(args, 0) != 0,
            "setCategory" => n.category = val.unwrap(),
            "setSmallIcon" => n.small_icon = int(args, 0),
            "setOngoing" => n.ongoing = int(args, 0) != 0,
            "setAutoCancel" => n.auto_cancel = int(args, 0) != 0,
            "setOnlyAlertOnce" => n.only_alert_once = int(args, 0) != 0,
            "setVisibility" => n.visibility = int(args, 0),
            "setPriority" => n.priority = int(args, 0),
            "setNumber" => n.number = int(args, 0),
            "setProgress" => n.progress = (int(args, 0), int(args, 1), int(args, 2) != 0),
            "setContentIntent" => n.content_intent = obj(args, 0),
            "setDeleteIntent" => n.delete_intent = obj(args, 0),
            "setTimeoutAfter" => n.timeout_ms = long(args, 0).max(0) as u64,
            "addAction" => {
                let action = new_action.ok_or_else(|| illegal("invalid Notification action"))?;
                if n.actions.len() >= 32 {
                    return Err(illegal("notification action limit"));
                }
                n.actions.push(action);
            }
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if matches!(
        class,
        "Landroid/app/Notification$BigTextStyle;"
            | "Landroidx/core/app/NotificationCompat$BigTextStyle;"
    ) {
        match name {
            "<init>" => {
                vm.fw.m9.builders.insert(recv, Notification::default());
                return Ok(Some(Value::Null));
            }
            "bigText" => {
                let s = text(vm, args, 0)?;
                vm.fw.m9.builders.entry(recv).or_default().big_text = s;
                return Ok(Some(Value::Obj(recv)));
            }
            _ => return Ok(None),
        }
    }
    if matches!(
        class,
        "Landroid/app/Notification$Action;"
            | "Landroid/app/Notification$Action$Builder;"
            | "Landroidx/core/app/NotificationCompat$Action$Builder;"
    ) {
        match name {
            "<init>" => {
                let a = Action {
                    icon: int(args, 0),
                    title: text(vm, args, 1)?,
                    pending: obj(args, 2),
                };
                vm.fw.m9.action_objects.insert(recv, a);
                return Ok(Some(Value::Null));
            }
            "build" => {
                let a = vm
                    .fw
                    .m9
                    .action_objects
                    .get(&recv)
                    .cloned()
                    .ok_or_else(|| illegal("uninitialized Action.Builder"))?;
                let r = alloc(vm, "Landroid/app/Notification$Action;")?;
                vm.fw.m9.action_objects.insert(r, a);
                return Ok(Some(Value::Obj(r)));
            }
            _ => return Ok(None),
        }
    }
    if class == MANAGER || class == "Landroidx/core/app/NotificationManagerCompat;" {
        let result = match name {
            "createNotificationChannel" => {
                let r = obj(args, 0).ok_or_else(|| illegal("channel null"))?;
                let c = vm
                    .fw
                    .m9
                    .channel_values
                    .get(&r)
                    .cloned()
                    .ok_or_else(|| illegal("invalid channel"))?;
                if let Some(old) = vm.fw.m9.channels.get_mut(&c.id) {
                    old.name = c.name;
                    old.description = c.description;
                    old.deleted = false;
                } else {
                    vm.fw.m9.channels.insert(c.id.clone(), c);
                }
                Value::Null
            }
            "createNotificationChannels" => {
                let r = obj(args, 0).ok_or_else(|| illegal("channels null"))?;
                let vals = match vm.fw.get(r) {
                    Some(HostObj::List { elems }) => elems.clone(),
                    _ => return Err(illegal("channels must be List")),
                };
                for v in vals {
                    instance_call(vm, MANAGER, "createNotificationChannel", "", recv, &[v])?;
                }
                Value::Null
            }
            "getNotificationChannel" => {
                let id = text(vm, args, 0)?;
                if let Some(c) = vm.fw.m9.channels.get(&id).filter(|c| !c.deleted).cloned() {
                    let r = alloc(vm, CHANNEL)?;
                    vm.fw.m9.channel_values.insert(r, c);
                    Value::Obj(r)
                } else {
                    Value::Null
                }
            }
            "getNotificationChannels" => {
                let mut vals = Vec::new();
                for c in vm
                    .fw
                    .m9
                    .channels
                    .values()
                    .filter(|c| !c.deleted)
                    .cloned()
                    .collect::<Vec<_>>()
                {
                    let r = alloc(vm, CHANNEL)?;
                    vm.fw.m9.channel_values.insert(r, c);
                    vals.push(Value::Obj(r));
                }
                list(vm, vals)?
            }
            "deleteNotificationChannel" => {
                let id = text(vm, args, 0)?;
                if let Some(c) = vm.fw.m9.channels.get_mut(&id) {
                    c.deleted = true;
                }
                let keys: Vec<_> = vm
                    .fw
                    .m9
                    .active
                    .iter()
                    .filter(|(_, n)| n.channel == id)
                    .map(|(k, _)| k.clone())
                    .collect();
                for (tag, id) in keys {
                    vm.m9_cancel_notification(&tag, id, 17)?;
                }
                Value::Null
            }
            "areNotificationsEnabled" => Value::Int(vm.fw.m9.notifications_allowed as i32),
            "notify" => {
                let (tag, id, nidx) = if sig.starts_with("(Ljava/lang/String;") {
                    (text(vm, args, 0)?, int(args, 1), 2)
                } else {
                    (String::new(), int(args, 0), 1)
                };
                let r = obj(args, nidx).ok_or_else(|| illegal("notification null"))?;
                let n = vm
                    .fw
                    .m9
                    .notification_objects
                    .get(&r)
                    .cloned()
                    .ok_or_else(|| illegal("invalid notification"))?;
                vm.m9_notify(&tag, id, n)?;
                Value::Null
            }
            "cancel" => {
                let (tag, id) = if sig.starts_with("(Ljava/lang/String;") {
                    (text(vm, args, 0)?, int(args, 1))
                } else {
                    (String::new(), int(args, 0))
                };
                vm.m9_cancel_notification(&tag, id, 8)?;
                Value::Null
            }
            "cancelAll" => {
                let keys: Vec<_> = vm.fw.m9.active.keys().cloned().collect();
                for (tag, id) in keys {
                    vm.m9_cancel_notification(&tag, id, 9)?;
                }
                Value::Null
            }
            "getActiveNotifications" => {
                let mut vals = Vec::new();
                for n in vm.m9_notifications() {
                    vals.push(Value::Obj(status(vm, n)?));
                }
                array(vm, vals, STATUS)?
            }
            _ => return Ok(None),
        };
        return Ok(Some(result));
    }
    if class == STATUS {
        let n = vm
            .fw
            .m9
            .statusbar
            .get(&recv)
            .cloned()
            .ok_or_else(|| illegal("invalid StatusBarNotification"))?;
        let v = match name {
            "getId" => Value::Int(n.id),
            "getTag" => string(vm, n.tag)?,
            "getPackageName" => string(vm, vm.fw.package_name.clone())?,
            "getKey" => string(vm, format!("{}|{}|{}", vm.fw.package_name, n.id, n.tag))?,
            "getPostTime" => Value::Long(n.posted_ms as i64),
            "isOngoing" => Value::Int(n.ongoing as i32),
            "getNotification" => {
                let r = alloc(vm, NOTIFICATION)?;
                vm.fw.m9.notification_objects.insert(r, n);
                Value::Obj(r)
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == PENDING {
        let v = match name {
            "cancel" => {
                vm.fw
                    .m9
                    .pending
                    .get_mut(&recv)
                    .ok_or_else(|| illegal("unknown pending intent"))?
                    .cancelled = true;
                Value::Null
            }
            "send" => {
                vm.m9_send_pending(recv)?;
                Value::Null
            }
            "isImmutable" => Value::Int(
                vm.fw
                    .m9
                    .pending
                    .get(&recv)
                    .map(|p| p.immutable)
                    .unwrap_or(false) as i32,
            ),
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == "Landroid/service/notification/NotificationListenerService;" {
        match name {
            "onCreate" => {
                vm.m9_register_notification_listener(recv)?;
                return Ok(Some(Value::Null));
            }
            "onDestroy" => {
                vm.fw.m9.listeners.retain(|r| *r != recv);
                return Ok(Some(Value::Null));
            }
            _ => {}
        }
    }
    Ok(None)
}
