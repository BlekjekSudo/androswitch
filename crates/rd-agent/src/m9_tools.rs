// Included in mcp.rs: the same session policy, Engine and result envelope.
impl AgentSession {
    fn tool_notifications(&mut self, args: &serde_json::Map<String,serde_json::Value>) -> Result<ToolResult,RdError> {
        #[cfg(feature="advanced-components")]
        {
            let e=self.require_engine()?;
            let action=args.get("action").and_then(|v|v.as_str()).unwrap_or("list");
            let tag=args.get("tag").and_then(|v|v.as_str()).unwrap_or("");
            match action {
                "list"=>{},
                "click"|"cancel"=>{
                    let id=m9_i32(args,"id")?;
                    if action=="click" { let index=args.get("action_index").map(|v|v.as_u64().ok_or_else(||agent_error("INVALID_ARG","action_index must be nonnegative",None))).transpose()?.map(|v|v as usize);e.m9_click_notification(tag,id,index).map_err(vm_exit_to_rd)?; }
                    else {e.m9_cancel_notification(tag,id,2).map_err(vm_exit_to_rd)?;}
                },
                _=>return Err(agent_error("INVALID_ARG","notifications action must be list, click or cancel",None)),
            }
            Ok(ToolResult::json(&serde_json::json!({"notifications":e.m9_notifications(),"channels":e.fw.m9.channels.values().collect::<Vec<_>>(),"permission_granted":e.fw.m9.notifications_allowed})))
        }
        #[cfg(not(feature="advanced-components"))]
        {let _=args;Err(agent_error("NOT_IMPLEMENTED","M9 requires advanced-components",None))}
    }
    fn tool_m9(&mut self,name:&str,args:&serde_json::Map<String,serde_json::Value>)->Result<ToolResult,RdError>{
        if name=="native_state" {
            let e=self.require_engine()?;
            #[cfg(feature="native")]
            {return Ok(ToolResult::json(&e.native_report()));}
            #[cfg(not(feature="native"))]
            {let _=e;return Ok(ToolResult::json(&serde_json::json!({"enabled":false,"hint":"build rd-cli with --features native; install unicorn==2.1.4"})));}
        }
        #[cfg(not(feature="advanced-components"))]
        {let _=(name,args);Err(agent_error("NOT_IMPLEMENTED","M9 requires advanced-components",None))}
        #[cfg(feature="advanced-components")]
        {
            let e=self.require_engine()?;
            let action=args.get("action").and_then(|v|v.as_str()).unwrap_or(match name{"ime"=>"state","accessibility"=>"tree",_=>"list"});
            match name {
                "component_service"=>{
                    let class=args.get("class").and_then(|v|v.as_str()).ok_or_else(||agent_error("INVALID_ARG","component class required",None))?;
                    let role=args.get("role").and_then(|v|v.as_str()).ok_or_else(||agent_error("INVALID_ARG","component role required",None))?;
                    let desc=normalize_activity_desc(class);
                    let dot=desc.trim_start_matches('L').trim_end_matches(';').replace('/',".");
                    let declared=e.resources().map_err(vm_exit_to_rd)?.manifest.application.components.iter().any(|c|c.class_name==dot&&c.enabled!=Some(false)&&if role=="widget"{c.kind==ComponentKind::Receiver}else{c.kind==ComponentKind::Service});
                    if !declared{return Err(agent_error("INVALID_ARG","component is not an enabled service/receiver declared in the active APK",None));}
                    let expected=match role{"ime"=>"Landroid/inputmethodservice/InputMethodService;","accessibility"=>"Landroid/accessibilityservice/AccessibilityService;","notification_listener"=>"Landroid/service/notification/NotificationListenerService;","widget"=>"Landroid/appwidget/AppWidgetProvider;",_=>return Err(agent_error("INVALID_ARG","unknown component role",None))};
                    if !e.cp.is_subtype(&desc,expected){return Err(agent_error("INVALID_ARG",format!("component is not a {expected}"),None));}
                    e.ensure_initialized(&desc).map_err(vm_exit_to_rd)?;
                    let r=e.heap.alloc_instance(desc.clone(),Vec::new()).map_err(|_|agent_error("VM_OOM","component allocation budget exceeded",None))?;
                    e.attach_host_state(&desc,r).map_err(vm_exit_to_rd)?;
                    e.m9_start_component(r,role).map_err(vm_exit_to_rd)?;
                    Ok(ToolResult::json(&serde_json::json!({"object":r,"class":dot,"role":role})))
                },
                "widgets"=>{
                    match action {
                        "list"=>{},
                        "allocate"=>{let host=args.get("host").and_then(|v|v.as_i64()).unwrap_or(1);let host=i32::try_from(host).map_err(|_|agent_error("INVALID_ARG","host outside i32",None))?;let id=e.m9_allocate_widget(host).map_err(vm_exit_to_rd)?;return Ok(ToolResult::json(&serde_json::json!({"id":id})));},
                        "bind"=>{e.m9_bind_widget(m9_i32(args,"id")?,m9_object(args,"provider")?).map_err(vm_exit_to_rd)?;},
                        "delete"=>{e.m9_delete_widget(m9_i32(args,"id")?).map_err(vm_exit_to_rd)?;},
                        "click"=>{e.m9_click_widget(m9_i32(args,"id")?,m9_i32(args,"view_id")?).map_err(vm_exit_to_rd)?;},
                        _=>return Err(agent_error("INVALID_ARG","unknown widgets action",None)),
                    }
                    Ok(ToolResult::json(&serde_json::json!({"widgets":e.fw.m9.widgets.values().collect::<Vec<_>>()})))
                },
                "widget_screenshot"=>{let tree=e.m9_widget_tree(m9_i32(args,"id")?).ok_or_else(||agent_error("INVALID_ARG","widget has no RemoteViews content",None))?;let png=rd_render::render_snapshot(&tree).to_png();Ok(ToolResult{content:vec![ToolContent::Image{data_b64:base64_encode(&png),mime:"image/png"}],is_error:false})},
                "ime"=>{
                    match action {
                        "state"=>{},"focus"=>{e.m9_focus(m9_object(args,"view")?).map_err(vm_exit_to_rd)?;},
                        "select"=>{let start=m9_nonnegative(args,"start")?;let end=m9_nonnegative(args,"end")?;e.m9_set_selection(start,end).map_err(vm_exit_to_rd)?;},
                        "commit"|"compose"=>{let text=args.get("text").and_then(|v|v.as_str()).ok_or_else(||agent_error("INVALID_ARG","text required",None))?;let cursor=if args.contains_key("cursor"){m9_i32(args,"cursor")?}else{1};e.m9_commit_text(text,cursor,action=="compose").map_err(vm_exit_to_rd)?;},
                        "delete"=>{e.m9_delete_surrounding(m9_nonnegative(args,"before")?,m9_nonnegative(args,"after")?,args.get("codepoints").and_then(|v|v.as_bool()).unwrap_or(false)).map_err(vm_exit_to_rd)?;},
                        "finish"=>{if let Some(r)=e.fw.m9.focused{if let Some(editor)=e.fw.m9.editors.get_mut(&r){editor.composing=None;}}},
                        _=>return Err(agent_error("INVALID_ARG","unknown ime action",None)),
                    }
                    let focused=e.fw.m9.focused;let editor=focused.and_then(|r|e.fw.m9.editors.get(&r));let text=focused.and_then(|r|e.fw.objects.get(&r)).and_then(|h|if let rd_vm::framework::HostObj::View{text,..}=h{Some(text)}else{None});
                    Ok(ToolResult::json(&serde_json::json!({"focused":focused,"keyboard_visible":e.fw.m9.keyboard_visible,"editor":editor,"text":text})))
                },
                "accessibility"=>{match action {
                    "tree"=>Ok(ToolResult::json(&serde_json::json!({"nodes":e.m9_accessible_nodes()}))),
                    "events"=>Ok(ToolResult::json(&serde_json::json!({"events":e.fw.m9.accessibility_events}))),
                    "perform"=>{let done=e.m9_accessibility_action(m9_object(args,"node")?,m9_i32(args,"action_id")?,args.get("text").and_then(|v|v.as_str())).map_err(vm_exit_to_rd)?;Ok(ToolResult::json(&serde_json::json!({"performed":done})))},
                    _=>Err(agent_error("INVALID_ARG","unknown accessibility action",None)),
                }},
                "shortcuts"=>{if action=="launch"{let id=args.get("id").and_then(|v|v.as_str()).ok_or_else(||agent_error("INVALID_ARG","shortcut id required",None))?;e.m9_launch_shortcut(id).map_err(vm_exit_to_rd)?;}else if action!="list"{return Err(agent_error("INVALID_ARG","unknown shortcuts action",None));}Ok(ToolResult::json(&serde_json::json!({"shortcuts":e.fw.m9.shortcuts.values().collect::<Vec<_>>()})))},
                _=>Err(agent_error("UNKNOWN_TOOL",name,None)),
            }
        }
    }
}
#[cfg(feature="advanced-components")]
fn m9_i32(args:&serde_json::Map<String,serde_json::Value>,key:&str)->Result<i32,RdError>{args.get(key).and_then(|v|v.as_i64()).and_then(|v|i32::try_from(v).ok()).ok_or_else(||agent_error("INVALID_ARG",format!("{key} must be an i32 integer"),None))}
#[cfg(feature="advanced-components")]
fn m9_object(args:&serde_json::Map<String,serde_json::Value>,key:&str)->Result<u32,RdError>{args.get(key).and_then(|v|v.as_u64()).filter(|v|*v>0).and_then(|v|u32::try_from(v).ok()).ok_or_else(||agent_error("INVALID_ARG",format!("{key} must be a nonzero object id"),None))}
#[cfg(feature="advanced-components")]
fn m9_nonnegative(args:&serde_json::Map<String,serde_json::Value>,key:&str)->Result<usize,RdError>{args.get(key).and_then(|v|v.as_u64()).and_then(|v|usize::try_from(v).ok()).ok_or_else(||agent_error("INVALID_ARG",format!("{key} must be nonnegative"),None))}
