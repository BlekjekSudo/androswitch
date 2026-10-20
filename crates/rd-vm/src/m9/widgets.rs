use super::*;
use crate::framework::{HostObj, UiNode, Vis};
use serde::{Deserialize, Serialize};
const REMOTE: &str = "Landroid/widget/RemoteViews;";
const MANAGER: &str = "Landroid/appwidget/AppWidgetManager;";
const HOST: &str = "Landroid/appwidget/AppWidgetHost;";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemoteOp {
    Text(i32, String),
    Visibility(i32, i32),
    Click(i32, ObjRef),
    Enabled(i32, bool),
    Progress(i32, i32, i32, bool),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteViews {
    pub package: String,
    pub layout: u32,
    pub operations: Vec<RemoteOp>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: i32,
    pub host: i32,
    pub provider: Option<ObjRef>,
    pub remote: Option<RemoteViews>,
    pub root: Option<ObjRef>,
    pub clicks: BTreeMap<i32, ObjRef>,
    pub options: BTreeMap<String, i32>,
}
impl Engine {
    pub fn m9_allocate_widget(&mut self, host: i32) -> Result<i32, VmExit> {
        if self.fw.m9.widgets.len() >= 256 {
            return Err(illegal("widget limit exceeded"));
        }
        let id = self.fw.m9.next_widget;
        self.fw.m9.next_widget = self
            .fw
            .m9
            .next_widget
            .checked_add(1)
            .ok_or_else(|| illegal("widget id overflow"))?;
        self.fw.m9.widgets.insert(
            id,
            Widget {
                id,
                host,
                provider: None,
                remote: None,
                root: None,
                clicks: BTreeMap::new(),
                options: BTreeMap::new(),
            },
        );
        Ok(id)
    }
    pub fn m9_bind_widget(&mut self, id: i32, provider: ObjRef) -> Result<(), VmExit> {
        let first = !self
            .fw
            .m9
            .widgets
            .values()
            .any(|w| w.provider == Some(provider));
        self.fw
            .m9
            .widgets
            .get_mut(&id)
            .ok_or_else(|| illegal("widget id not allocated"))?
            .provider = Some(provider);
        if first {
            call_dex(
                self,
                provider,
                "onEnabled",
                "(Landroid/content/Context;)V",
                vec![self
                    .fw
                    .current_activity
                    .map(Value::Obj)
                    .unwrap_or(Value::Null)],
            )?;
        }
        let ids = self.heap.alloc_array(crate::heap::ElemKind::Int, 1, "[I")?;
        self.heap.array_elems_mut(ids)?.push(Value::Int(id));
        let manager = alloc(self, MANAGER)?;
        call_dex(
            self,
            provider,
            "onUpdate",
            "(Landroid/content/Context;Landroid/appwidget/AppWidgetManager;[I)V",
            vec![
                self.fw
                    .current_activity
                    .map(Value::Obj)
                    .unwrap_or(Value::Null),
                Value::Obj(manager),
                Value::Obj(ids),
            ],
        )?;
        Ok(())
    }
    pub fn m9_delete_widget(&mut self, id: i32) -> Result<(), VmExit> {
        let w = self
            .fw
            .m9
            .widgets
            .remove(&id)
            .ok_or_else(|| illegal("unknown widget"))?;
        if let Some(provider) = w.provider {
            let ids = self.heap.alloc_array(crate::heap::ElemKind::Int, 1, "[I")?;
            self.heap.array_elems_mut(ids)?.push(Value::Int(id));
            let context = self
                .fw
                .current_activity
                .map(Value::Obj)
                .unwrap_or(Value::Null);
            call_dex(
                self,
                provider,
                "onDeleted",
                "(Landroid/content/Context;[I)V",
                vec![context.clone(), Value::Obj(ids)],
            )?;
            if !self
                .fw
                .m9
                .widgets
                .values()
                .any(|w| w.provider == Some(provider))
            {
                call_dex(
                    self,
                    provider,
                    "onDisabled",
                    "(Landroid/content/Context;)V",
                    vec![context],
                )?;
            }
        }
        Ok(())
    }
    pub fn m9_update_widget(
        &mut self,
        id: i32,
        remote: RemoteViews,
        partial: bool,
    ) -> Result<(), VmExit> {
        if !remote.package.is_empty()
            && !self.fw.package_name.is_empty()
            && remote.package != self.fw.package_name
        {
            return Err(error(
                "Ljava/lang/SecurityException;",
                "cross-package RemoteViews not installed",
            ));
        }
        let old = self
            .fw
            .m9
            .widgets
            .get(&id)
            .cloned()
            .ok_or_else(|| illegal("unknown widget id"))?;
        let mut effective = if partial {
            old.remote
                .clone()
                .ok_or_else(|| illegal("partial update before initial RemoteViews"))?
        } else {
            RemoteViews {
                operations: Vec::new(),
                ..remote.clone()
            }
        };
        if partial && effective.layout != remote.layout {
            return Err(illegal("partial widget update cannot change layout"));
        }
        if effective.operations.len() + remote.operations.len() > 4096 {
            return Err(illegal("RemoteViews operation limit exceeded"));
        }
        effective.operations.extend(remote.operations);
        let root = crate::inflate::inflate_resource(self, effective.layout)?;
        let mut clicks = BTreeMap::new();
        for op in &effective.operations {
            let id = match op {
                RemoteOp::Text(id, _)
                | RemoteOp::Visibility(id, _)
                | RemoteOp::Click(id, _)
                | RemoteOp::Enabled(id, _)
                | RemoteOp::Progress(id, ..) => *id,
            };
            let target = self
                .find_view_by_id(root, id)
                .ok_or_else(|| illegal(format!("RemoteViews target id {id:#x} absent")))?;
            match op {
                RemoteOp::Text(_, s) => self.set_view_text(target, s.clone())?,
                RemoteOp::Visibility(_, v) => {
                    let vis = match v {
                        0 => Vis::Visible,
                        4 => Vis::Invisible,
                        8 => Vis::Gone,
                        _ => return Err(illegal("invalid RemoteViews visibility")),
                    };
                    if let Some(HostObj::View { vis: v, .. }) = self.fw.objects.get_mut(&target) {
                        *v = vis;
                    }
                }
                RemoteOp::Enabled(_, enabled) => {
                    if let Some(HostObj::View { enabled: e, .. }) = self.fw.objects.get_mut(&target)
                    {
                        *e = *enabled;
                    }
                }
                RemoteOp::Click(id, p) => {
                    clicks.insert(*id, *p);
                }
                RemoteOp::Progress(..) => {
                    return Err(crate::err::vm_error(
                        "NOT_IMPLEMENTED",
                        "RemoteViews ProgressBar renderer not implemented",
                    )
                    .into())
                }
            }
        }
        self.layout(root, 0, 0, crate::framework::WINDOW_W);
        let w = self.fw.m9.widgets.get_mut(&id).unwrap();
        w.remote = Some(effective);
        w.root = Some(root);
        w.clicks = clicks;
        Ok(())
    }
    pub fn m9_widget_tree(&self, id: i32) -> Option<UiNode> {
        let r = self.fw.m9.widgets.get(&id)?.root?;
        Some(self.build_ui_node(r, 0))
    }
    pub fn m9_click_widget(&mut self, id: i32, view_id: i32) -> Result<(), VmExit> {
        let p = *self
            .fw
            .m9
            .widgets
            .get(&id)
            .and_then(|w| w.clicks.get(&view_id))
            .ok_or_else(|| illegal("widget view has no click PendingIntent"))?;
        self.m9_send_pending(p)
    }
}
pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == MANAGER && name == "getInstance" {
        let r = if let Some(r) = vm.fw.m9.service_objects.get(MANAGER) {
            *r
        } else {
            let r = alloc(vm, MANAGER)?;
            vm.fw.m9.service_objects.insert(MANAGER.into(), r);
            r
        };
        return Ok(Some(Value::Obj(r)));
    }
    Ok(None)
}
pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == REMOTE {
        if name == "<init>" {
            if sig.starts_with("(Landroid/widget/RemoteViews;") {
                let from = obj(args, 0).ok_or_else(|| illegal("RemoteViews source null"))?;
                let r = vm
                    .fw
                    .m9
                    .remote_views
                    .get(&from)
                    .cloned()
                    .ok_or_else(|| illegal("RemoteViews source invalid"))?;
                vm.fw.m9.remote_views.insert(recv, r);
            } else {
                let package = text(vm, args, 0)?;
                vm.fw.m9.remote_views.insert(
                    recv,
                    RemoteViews {
                        package,
                        layout: int(args, 1) as u32,
                        operations: Vec::new(),
                    },
                );
            }
            return Ok(Some(Value::Null));
        }
        if name == "getLayoutId" {
            return Ok(Some(Value::Int(
                vm.fw
                    .m9
                    .remote_views
                    .get(&recv)
                    .ok_or_else(|| illegal("uninitialized RemoteViews"))?
                    .layout as i32,
            )));
        }
        let op = match name {
            "setTextViewText" => RemoteOp::Text(int(args, 0), text(vm, args, 1)?),
            "setViewVisibility" => RemoteOp::Visibility(int(args, 0), int(args, 1)),
            "setOnClickPendingIntent" => RemoteOp::Click(
                int(args, 0),
                obj(args, 1).ok_or_else(|| illegal("click PendingIntent null"))?,
            ),
            "setBoolean" => {
                if text(vm, args, 1)? != "setEnabled" {
                    return Err(crate::err::vm_error(
                        "NOT_IMPLEMENTED",
                        "RemoteViews boolean method",
                    )
                    .into());
                }
                RemoteOp::Enabled(int(args, 0), int(args, 2) != 0)
            }
            _ => return Ok(None),
        };
        let r = vm
            .fw
            .m9
            .remote_views
            .get_mut(&recv)
            .ok_or_else(|| illegal("uninitialized RemoteViews"))?;
        if r.operations.len() >= 4096 {
            return Err(illegal("RemoteViews operation cap exceeded"));
        }
        r.operations.push(op);
        return Ok(Some(Value::Null));
    }
    if class == HOST {
        let v = match name {
            "<init>" => {
                vm.fw.m9.widget_hosts.insert(recv, int(args, 1));
                Value::Null
            }
            "allocateAppWidgetId" => {
                let host = *vm
                    .fw
                    .m9
                    .widget_hosts
                    .get(&recv)
                    .ok_or_else(|| illegal("uninitialized AppWidgetHost"))?;
                Value::Int(vm.m9_allocate_widget(host)?)
            }
            "deleteAppWidgetId" => {
                vm.m9_delete_widget(int(args, 0))?;
                Value::Null
            }
            "deleteHost" => {
                let host = *vm
                    .fw
                    .m9
                    .widget_hosts
                    .get(&recv)
                    .ok_or_else(|| illegal("uninitialized widget host"))?;
                let ids: Vec<_> = vm
                    .fw
                    .m9
                    .widgets
                    .values()
                    .filter(|w| w.host == host)
                    .map(|w| w.id)
                    .collect();
                for id in ids {
                    vm.m9_delete_widget(id)?;
                }
                Value::Null
            }
            "startListening" | "stopListening" => Value::Null,
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == MANAGER {
        let v = match name {
            "updateAppWidget" | "partiallyUpdateAppWidget" => {
                let remote = obj(args, 1)
                    .and_then(|r| vm.fw.m9.remote_views.get(&r).cloned())
                    .ok_or_else(|| illegal("updateAppWidget RemoteViews missing"))?;
                let ids = if sig.starts_with("([I") {
                    let r = obj(args, 0).ok_or_else(|| illegal("widget ids null"))?;
                    vm.heap
                        .as_array_elems(r)?
                        .0
                        .iter()
                        .map(|v| v.as_int().unwrap_or(0))
                        .collect::<Vec<_>>()
                } else {
                    vec![int(args, 0)]
                };
                for id in ids {
                    vm.m9_update_widget(id, remote.clone(), name == "partiallyUpdateAppWidget")?;
                }
                Value::Null
            }
            "getAppWidgetIds" => {
                let vals: Vec<_> = vm.fw.m9.widgets.keys().map(|id| Value::Int(*id)).collect();
                let r = vm
                    .heap
                    .alloc_array(crate::heap::ElemKind::Int, vals.len(), "[I")?;
                *vm.heap.array_elems_mut(r)? = vals;
                Value::Obj(r)
            }
            "getAppWidgetOptions" => {
                let w = vm
                    .fw
                    .m9
                    .widgets
                    .get(&int(args, 0))
                    .cloned()
                    .ok_or_else(|| illegal("unknown widget"))?;
                let r = alloc(vm, "Landroid/os/Bundle;")?;
                vm.fw.objects.insert(
                    r,
                    HostObj::Bundle {
                        map: w
                            .options
                            .into_iter()
                            .map(|(k, v)| (k, Value::Int(v)))
                            .collect(),
                    },
                );
                Value::Obj(r)
            }
            "updateAppWidgetOptions" => {
                let r = obj(args, 1).ok_or_else(|| illegal("options null"))?;
                let map = match vm.fw.get(r) {
                    Some(HostObj::Bundle { map }) => map.clone(),
                    _ => return Err(illegal("options not Bundle")),
                };
                let w = vm
                    .fw
                    .m9
                    .widgets
                    .get_mut(&int(args, 0))
                    .ok_or_else(|| illegal("unknown widget"))?;
                for (k, v) in map {
                    w.options.insert(k, v.as_int().unwrap_or(0));
                }
                if let Some(provider) = w.provider {
                    call_dex(vm,provider,"onAppWidgetOptionsChanged","(Landroid/content/Context;Landroid/appwidget/AppWidgetManager;ILandroid/os/Bundle;)V",vec![vm.fw.current_activity.map(Value::Obj).unwrap_or(Value::Null),Value::Obj(recv),Value::Int(int(args,0)),Value::Obj(r)])?;
                }
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }
    if class == "Landroid/appwidget/AppWidgetProvider;"
        && matches!(
            name,
            "<init>"
                | "onReceive"
                | "onUpdate"
                | "onEnabled"
                | "onDisabled"
                | "onDeleted"
                | "onAppWidgetOptionsChanged"
        )
    {
        return Ok(Some(Value::Null));
    }
    Ok(None)
}
