//! Process-local broadcasts and manifest-backed service discovery. There is no
//! Binder backend: discovery metadata never creates a functioning remote service.
use super::*;
use alloc::collections::BTreeSet;
use core::cell::Cell;
const RECEIVER: &str = "Landroid/content/BroadcastReceiver;";
const FILTER: &str = "Landroid/content/IntentFilter;";
const INTENT: &str = "Landroid/content/Intent;";
const PM: &str = "Landroid/content/pm/PackageManager;";
#[derive(Clone, Default)]
struct Filter {
    actions: Vec<String>,
    categories: BTreeSet<String>,
    priority: i32,
}
#[derive(Clone, Default)]
struct Intent {
    action: Option<String>,
    package: Option<String>,
    categories: BTreeSet<String>,
}
fn filter_bytes(f: &Filter) -> usize {
    f.actions.iter().map(String::len).sum::<usize>()
        + f.categories.iter().map(String::len).sum::<usize>()
}
fn intent_bytes(i: &Intent) -> usize {
    i.action.as_ref().map_or(0, String::len)
        + i.package.as_ref().map_or(0, String::len)
        + i.categories.iter().map(String::len).sum::<usize>()
}
struct PackageManifest {
    manifest: rd_apk::manifest::Manifest,
    permission: Option<String>,
    services: BTreeMap<String, Vec<rd_apk::axml::XmlAttribute>>,
}
fn manifest(apk: &Apk) -> Result<Option<PackageManifest>> {
    let raw = apk.manifest_raw().map_err(|e| error(&e.code, e.cause))?;
    let app = raw
        .root
        .children_named("application")
        .next()
        .ok_or_else(|| error("INVALID_APK", "application manifest element"))?;
    if let Some(enabled) = app.android_attr("enabled") {
        let enabled = enabled
            .as_bool()
            .ok_or_else(|| unavailable("resource-valued application enabled state"))?;
        if !enabled {
            return Ok(None);
        }
    }
    let permission = match app.android_attr("permission").map(|a| &a.value) {
        None | Some(AttrValue::Null) => None,
        Some(AttrValue::String(s)) => Some(s.clone()),
        _ => return Err(unavailable("resource-valued application permission")),
    };
    let mut services = BTreeMap::new();
    for service in app.children_named("service") {
        if services.len() >= 4096 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "4096 raw service declarations",
            ));
        }
        let name = service
            .android_attr("name")
            .and_then(|a| a.value.as_string())
            .ok_or_else(|| unavailable("nonliteral service class name"))?;
        let name = if name.starts_with('.') {
            format!("{}{name}", apk.manifest.package)
        } else if name.contains('.') {
            name.to_string()
        } else {
            format!("{}.{name}", apk.manifest.package)
        };
        let attrs = service
            .attrs
            .iter()
            .filter(|a| {
                a.is_android()
                    && matches!(
                        a.name.as_str(),
                        "enabled" | "exported" | "permission" | "process"
                    )
            })
            .cloned()
            .collect();
        services.insert(name, attrs);
    }
    let mut manifest = apk.manifest.clone();
    if let Some(process) = app.android_attr("process") {
        manifest.application.process = match &process.value {
            AttrValue::String(s) => Some(s.clone()),
            AttrValue::Null => None,
            _ => return Err(unavailable("resource-valued application process")),
        };
    }
    Ok(Some(PackageManifest {
        manifest,
        permission,
        services,
    }))
}
#[derive(Clone)]
struct Registration {
    id: u64,
    context: ObjRef,
    receiver: ObjRef,
    filter: Filter,
}
struct Delivery {
    registration: u64,
    context: ObjRef,
    receiver: ObjRef,
    intent: ObjRef,
}
#[derive(Default)]
pub(super) struct Communication {
    filters: RefCell<BTreeMap<ObjRef, Filter>>,
    intents: RefCell<BTreeMap<ObjRef, Intent>>,
    registrations: RefCell<Vec<Registration>>,
    bindings: RefCell<BTreeSet<(ObjRef, ObjRef)>>,
    pending: RefCell<VecDeque<Delivery>>,
    serial: Cell<u64>,
    manager: Cell<Option<ObjRef>>,
}
fn unavailable(what: &str) -> VmExit {
    error("API_UNAVAILABLE", format!("Android communication: {what}"))
}
fn nullable_text(vm: &Engine, value: &Value) -> Result<Option<String>> {
    let Some(r) = value.as_ref()? else {
        return Ok(None);
    };
    let s = vm.heap.as_str(r)?;
    if s.len() > 4096 {
        return Err(error(
            "INTENT_BUDGET",
            "4096 bytes per action/package/category",
        ));
    }
    Ok(Some(s.to_string()))
}
impl Communication {
    fn budget(&self, extra: usize) -> Result<()> {
        let used = self
            .filters
            .borrow()
            .values()
            .map(filter_bytes)
            .sum::<usize>()
            + self
                .intents
                .borrow()
                .values()
                .map(intent_bytes)
                .sum::<usize>()
            + self
                .registrations
                .borrow()
                .iter()
                .map(|r| filter_bytes(&r.filter))
                .sum::<usize>();
        if used.saturating_add(extra) > 8 << 20 {
            return Err(error(
                "BROADCAST_BUDGET",
                "8 MiB process-local intent/filter metadata",
            ));
        }
        Ok(())
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if vm.cp.is_subtype(class, INTENT) {
            if self.intents.borrow().len() >= 4096 {
                return Err(error("INTENT_BUDGET", "4096 live Intent objects"));
            }
            self.intents.borrow_mut().insert(r, Intent::default());
            return Ok(false); // Android also owns activity targets and Bundle extras.
        }
        if vm.cp.is_subtype(class, FILTER) {
            if self.filters.borrow().len() >= 4096 {
                return Err(error("BROADCAST_BUDGET", "4096 live IntentFilters"));
            }
            self.filters.borrow_mut().insert(r, Filter::default());
            return Ok(true);
        }
        Ok(vm.cp.is_subtype(class, RECEIVER)
            || matches!(
                class,
                PM | "Landroid/content/pm/ServiceInfo;" | "Landroid/content/pm/ResolveInfo;"
            ))
    }
    fn intent(&self, r: ObjRef) -> Result<Intent> {
        self.intents
            .borrow()
            .get(&r)
            .cloned()
            .ok_or_else(|| error("INVALID_INTENT", "unknown Intent"))
    }
    fn copy_intent(&self, android: &Android, vm: &mut Engine, source: ObjRef) -> Result<ObjRef> {
        let metadata = self.intent(source)?;
        self.budget(intent_bytes(&metadata))?;
        let copy = shell(vm, INTENT)?;
        self.intents.borrow_mut().insert(copy, metadata);
        let (target, extras) = {
            let s = android.state.borrow();
            (
                s.intents.get(&source).cloned(),
                s.bundles.get(&source).cloned(),
            )
        };
        let mut s = android.state.borrow_mut();
        if let Some(target) = target {
            s.intents.insert(copy, target);
        }
        if let Some(extras) = extras {
            s.bundles.insert(copy, extras);
        }
        Ok(copy)
    }
    fn matches(filter: &Filter, intent: &Intent) -> bool {
        intent
            .action
            .as_ref()
            .is_none_or(|action| filter.actions.contains(action))
            && intent.categories.is_subset(&filter.categories)
    }
    fn send(&self, android: &Android, vm: &mut Engine, intent: ObjRef) -> Result<()> {
        let metadata = self.intent(intent)?;
        if metadata.package.as_ref() != Some(&android.apk.manifest.package) {
            // The installed/background receiver catalog has not been resolved.
            // Never turn an implicit or external send into partial delivery.
            return Err(unavailable("implicit/external broadcast delivery broker"));
        }
        if android
            .state
            .borrow()
            .intents
            .get(&intent)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(unavailable("manifest/explicit broadcast receivers"));
        }
        for component in &android.apk.manifest.application.components {
            if component.kind != rd_apk::manifest::ComponentKind::Receiver
                || component.enabled == Some(false)
            {
                continue;
            }
            let requires_manifest = component.intent_filters.iter().any(|filter| {
                metadata
                    .action
                    .as_ref()
                    .is_none_or(|action| filter.actions.contains(action))
                    && metadata
                        .categories
                        .iter()
                        .all(|category| filter.categories.contains(category))
            });
            if requires_manifest {
                return Err(unavailable("matching manifest broadcast receiver delivery"));
            }
        }
        let mut recipients: Vec<_> = self
            .registrations
            .borrow()
            .iter()
            .filter(|r| Self::matches(&r.filter, &metadata))
            .cloned()
            .collect();
        recipients.sort_by_key(|r| (core::cmp::Reverse(r.filter.priority), r.id));
        let mut seen = BTreeSet::new();
        recipients.retain(|r| seen.insert((r.context, r.receiver)));
        if self.pending.borrow().len() + recipients.len() > 4096 {
            return Err(error("BROADCAST_BUDGET", "4096 queued receiver callbacks"));
        }
        // A distinct Intent per delivery matches Android's marshalled snapshot;
        // one receiver cannot mutate another receiver's extras accidentally.
        let mut deliveries = Vec::new();
        for r in recipients {
            deliveries.push(Delivery {
                registration: r.id,
                context: r.context,
                receiver: r.receiver,
                intent: self.copy_intent(android, vm, intent)?,
            });
        }
        self.pending.borrow_mut().extend(deliveries);
        Ok(())
    }
    pub(super) fn pump(&self, android: &Android, vm: &mut Engine) -> Result<()> {
        let count = self.pending.borrow().len().min(256);
        for _ in 0..count {
            let Some(delivery) = self.pending.borrow_mut().pop_front() else {
                break;
            };
            if !self
                .registrations
                .borrow()
                .iter()
                .any(|r| r.id == delivery.registration)
            {
                continue;
            }
            android.callback(
                vm,
                delivery.receiver,
                "onReceive",
                "(Landroid/content/Context;Landroid/content/Intent;)V",
                &[Value::Obj(delivery.context), Value::Obj(delivery.intent)],
            )?;
        }
        Ok(())
    }
    fn register(&self, context: ObjRef, receiver: Option<ObjRef>, filter: ObjRef) -> Result<Value> {
        let filter = self
            .filters
            .borrow()
            .get(&filter)
            .cloned()
            .ok_or_else(|| error("INVALID_FILTER", "unknown IntentFilter"))?;
        // No sticky broadcast producers exist; null receiver is a real query,
        // not a newly registered receiver or a fabricated battery/network event.
        if let Some(receiver) = receiver {
            self.budget(filter_bytes(&filter))?;
            let mut registrations = self.registrations.borrow_mut();
            if registrations.iter().any(|r| {
                r.context == context
                    && r.receiver == receiver
                    && r.filter.actions == filter.actions
                    && r.filter.categories == filter.categories
                    && r.filter.priority == filter.priority
            }) {
                return Ok(Value::Null);
            }
            if registrations.len() >= 256 {
                return Err(error("BROADCAST_BUDGET", "256 dynamic registrations"));
            }
            let id = self
                .serial
                .get()
                .checked_add(1)
                .ok_or_else(|| error("BROADCAST_BUDGET", "registration identity overflow"))?;
            self.serial.set(id);
            registrations.push(Registration {
                id,
                context,
                receiver,
                filter,
            });
        }
        Ok(Value::Null)
    }
    fn services(
        &self,
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        intent: ObjRef,
        flags: i32,
    ) -> Result<Value> {
        if flags != 0 {
            return Err(unavailable("PackageManager.queryIntentServices flags"));
        }
        let target = android.state.borrow().intents.get(&intent).cloned();
        let target = target.filter(|s| !s.is_empty());
        let intent = self.intent(intent)?;
        let mut manifests = Vec::new();
        if intent
            .package
            .as_ref()
            .is_none_or(|p| p == &android.apk.manifest.package)
        {
            if let Some(manifest) = manifest(&android.apk)? {
                manifests.push(manifest);
            }
        }
        let pm = as_package::PackageManager::new(services.fs.clone());
        let records = pm.list().map_err(storage_error)?;
        if records.len() > 64 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "64 installed packages per service query",
            ));
        }
        for record in records {
            if record.package == android.apk.manifest.package
                || intent
                    .package
                    .as_ref()
                    .is_some_and(|p| p != &record.package)
            {
                continue;
            }
            // Metadata query uses the immutable installed revision and bounded
            // file-backed ZIP readers. Do not rehash/read the whole APK here.
            let path = format!(
                "installed/{}/{:020}/package.apk",
                record.package, record.revision
            );
            let apk = as_package::open_apk(&services.fs, &path).map_err(storage_error)?;
            if apk.manifest.package != record.package {
                return Err(error("INSTALLATION_CORRUPT", path));
            }
            if let Some(manifest) = manifest(&apk)? {
                manifests.push(manifest);
            }
        }
        let mut matches = Vec::new();
        for package in manifests {
            let manifest = package.manifest;
            let application_permission = package.permission;
            if intent
                .package
                .as_ref()
                .is_some_and(|p| p != &manifest.package)
            {
                continue;
            }
            if manifest.application.components.len() > 4096 {
                return Err(error(
                    "PACKAGE_QUERY_BUDGET",
                    "4096 manifest components per package",
                ));
            }
            let application_process = manifest.application.process.clone();
            for mut component in manifest.application.components {
                if component.kind != rd_apk::manifest::ComponentKind::Service
                    || component.enabled == Some(false)
                {
                    continue;
                }
                let mut priority = None;
                if let Some(target) = &target {
                    if descriptor(&component.class_name) == *target {
                        priority = Some(0);
                    } else {
                        continue;
                    }
                }
                for filter in &component.intent_filters {
                    if target.is_some() {
                        break;
                    }
                    let f = Filter {
                        actions: filter.actions.clone(),
                        categories: filter.categories.iter().cloned().collect(),
                        priority: filter.priority.unwrap_or(0),
                    };
                    if Self::matches(&f, &intent) {
                        if !filter.data.is_empty() {
                            return Err(unavailable("service IntentFilter data matching"));
                        }
                        priority = Some(priority.unwrap_or(i32::MIN).max(f.priority));
                    }
                }
                if let Some(priority) = priority {
                    let attrs = package
                        .services
                        .get(&component.class_name)
                        .ok_or_else(|| error("INVALID_APK", "missing raw service declaration"))?;
                    for a in attrs
                        .iter()
                        .filter(|a| matches!(a.name.as_str(), "enabled" | "exported"))
                    {
                        let value = a.as_bool().ok_or_else(|| {
                            unavailable(&format!("resource-valued service {}", a.name))
                        })?;
                        if a.name == "exported" {
                            component.exported_effective = value;
                        }
                    }
                    let permission = attrs.iter().find(|a| a.name == "permission");
                    component.permission = match permission.map(|a| &a.value) {
                        None => application_permission.clone(),
                        Some(AttrValue::Null) => None,
                        Some(AttrValue::String(s)) => {
                            if s.is_empty() {
                                None
                            } else {
                                Some(s.clone())
                            }
                        }
                        _ => return Err(unavailable("resource-valued service permission")),
                    };
                    if let Some(process) = attrs.iter().find(|a| a.name == "process") {
                        component.process = match &process.value {
                            AttrValue::String(s) => Some(s.clone()),
                            AttrValue::Null => None,
                            _ => return Err(unavailable("resource-valued service process")),
                        };
                    }
                    let process = component
                        .process
                        .or_else(|| application_process.clone())
                        .unwrap_or_else(|| manifest.package.clone());
                    component.process = Some(if process.starts_with(':') {
                        format!("{}{process}", manifest.package)
                    } else {
                        process
                    });
                    matches.push((priority, manifest.package.clone(), component));
                }
                if matches.len() > 256 {
                    return Err(error(
                        "PACKAGE_QUERY_BUDGET",
                        "256 matching service declarations",
                    ));
                }
            }
        }
        matches.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.class_name.cmp(&b.2.class_name))
        });
        let list = shell(vm, "Ljava/util/ArrayList;")?;
        vm.invoke_instance(list, "<init>", "()V", &[])?;
        for (priority, package, component) in matches {
            let info = shell(vm, "Landroid/content/pm/ServiceInfo;")?;
            for (name, value) in [
                ("packageName", Some(package)),
                ("name", Some(component.class_name)),
                ("permission", component.permission),
                ("processName", component.process),
            ] {
                let value = value.map_or(Ok(Value::Null), |v| string(vm, v))?;
                write(vm, info, name, value)?;
            }
            write(vm, info, "enabled", Value::Int(1))?;
            write(
                vm,
                info,
                "exported",
                Value::Int(i32::from(component.exported_effective)),
            )?;
            let resolve = shell(vm, "Landroid/content/pm/ResolveInfo;")?;
            write(vm, resolve, "serviceInfo", Value::Obj(info))?;
            write(vm, resolve, "priority", Value::Int(priority))?;
            vm.invoke_instance(list, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(resolve)])?;
        }
        Ok(Value::Obj(list))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let class = vm.heap.class_of(r)?.to_string();
        let result = if self.filters.borrow().contains_key(&r) {
            match (name, sig) {
                ("<init>", "()V") => Value::Null,
                ("<init>", "(Ljava/lang/String;)V") | ("addAction", "(Ljava/lang/String;)V") => {
                    let action = text(vm, &args[0])?;
                    if action.len() > 4096 {
                        return Err(error("BROADCAST_BUDGET", "4096 bytes per action"));
                    }
                    self.budget(action.len())?;
                    let mut filters = self.filters.borrow_mut();
                    let f = filters
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_FILTER", "missing filter"))?;
                    if !f.actions.contains(&action) {
                        if f.actions.len() >= 64 {
                            return Err(error("BROADCAST_BUDGET", "64 actions per filter"));
                        }
                        f.actions.push(action);
                    }
                    Value::Null
                }
                ("<init>", "(Landroid/content/IntentFilter;)V") => {
                    let copy = self
                        .filters
                        .borrow()
                        .get(&obj(&args[0])?)
                        .cloned()
                        .ok_or_else(|| error("INVALID_FILTER", "copy"))?;
                    self.budget(filter_bytes(&copy))?;
                    self.filters.borrow_mut().insert(r, copy);
                    Value::Null
                }
                ("addCategory", "(Ljava/lang/String;)V") => {
                    let category = text(vm, &args[0])?;
                    if category.len() > 4096 {
                        return Err(error("BROADCAST_BUDGET", "4096 bytes per category"));
                    }
                    self.budget(category.len())?;
                    let mut f = self.filters.borrow_mut();
                    let f = f
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_FILTER", "category"))?;
                    if f.categories.len() >= 64 && !f.categories.contains(&category) {
                        return Err(error("BROADCAST_BUDGET", "64 categories per filter"));
                    }
                    f.categories.insert(category);
                    Value::Null
                }
                ("countActions", "()I") => {
                    Value::Int(self.filters.borrow()[&r].actions.len() as i32)
                }
                ("hasAction", "(Ljava/lang/String;)Z") => Value::Int(i32::from(
                    self.filters.borrow()[&r]
                        .actions
                        .contains(&text(vm, &args[0])?),
                )),
                ("getAction", "(I)Ljava/lang/String;") => {
                    let index = int(args, 0)?;
                    let action = self.filters.borrow()[&r]
                        .actions
                        .get(index as usize)
                        .cloned()
                        .ok_or_else(|| {
                            Throwable::new("Ljava/lang/IndexOutOfBoundsException;", "action index")
                        })?;
                    string(vm, action)?
                }
                ("setPriority", "(I)V") => {
                    self.filters
                        .borrow_mut()
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_FILTER", "priority"))?
                        .priority = int(args, 0)?;
                    Value::Null
                }
                ("getPriority", "()I") => Value::Int(self.filters.borrow()[&r].priority),
                _ => return Err(unavailable(&format!("IntentFilter.{name}{sig}"))),
            }
        } else if self.intents.borrow().contains_key(&r) {
            match (name, sig) {
                ("<init>", "(Ljava/lang/String;)V")
                | ("setAction", "(Ljava/lang/String;)Landroid/content/Intent;") => {
                    let action = nullable_text(vm, &args[0])?;
                    self.budget(action.as_ref().map_or(0, String::len))?;
                    self.intents
                        .borrow_mut()
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_INTENT", "action"))?
                        .action = action;
                    if name == "<init>" {
                        Value::Null
                    } else {
                        Value::Obj(r)
                    }
                }
                ("setPackage", "(Ljava/lang/String;)Landroid/content/Intent;") => {
                    let package = nullable_text(vm, &args[0])?;
                    self.budget(package.as_ref().map_or(0, String::len))?;
                    self.intents
                        .borrow_mut()
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_INTENT", "package"))?
                        .package = package;
                    Value::Obj(r)
                }
                ("getAction", "()Ljava/lang/String;") | ("getPackage", "()Ljava/lang/String;") => {
                    let i = self.intent(r)?;
                    let text = if name == "getAction" {
                        i.action
                    } else {
                        i.package
                    };
                    text.map_or(Ok(Value::Null), |v| string(vm, v))?
                }
                ("addCategory", "(Ljava/lang/String;)Landroid/content/Intent;") => {
                    let category = text(vm, &args[0])?;
                    if category.len() > 4096 {
                        return Err(error("INTENT_BUDGET", "4096 bytes per category"));
                    }
                    self.budget(category.len())?;
                    let mut intents = self.intents.borrow_mut();
                    let i = intents
                        .get_mut(&r)
                        .ok_or_else(|| error("INVALID_INTENT", "category"))?;
                    if i.categories.len() >= 64 && !i.categories.contains(&category) {
                        return Err(error("INTENT_BUDGET", "64 categories"));
                    }
                    i.categories.insert(category);
                    Value::Obj(r)
                }
                ("getExtras", "()Landroid/os/Bundle;") => {
                    let extras = android.state.borrow().bundles.get(&r).cloned();
                    if let Some(extras) = extras {
                        let b = shell(vm, "Landroid/os/Bundle;")?;
                        android.state.borrow_mut().bundles.insert(b, extras);
                        Value::Obj(b)
                    } else {
                        Value::Null
                    }
                }
                _ => return Ok(None),
            }
        } else if vm.cp.is_subtype(&class, RECEIVER) {
            match (name, sig) {
                ("<init>", "()V") => Value::Null,
                ("isInitialStickyBroadcast" | "isOrderedBroadcast", "()Z") => Value::Int(0),
                _ => return Ok(None),
            }
        } else if class == PM {
            match (name, sig) {
                ("queryIntentServices", "(Landroid/content/Intent;I)Ljava/util/List;") => {
                    self.services(services, android, vm, obj(&args[0])?, int(args, 1)?)?
                }
                _ => return Ok(None),
            }
        } else if vm.cp.is_subtype(&class, "Landroid/content/Context;") {
            match(name,sig) {
                ("getPackageManager","()Landroid/content/pm/PackageManager;")=>{let manager=if let Some(r)=self.manager.get(){r}else{let r=shell(vm,PM)?;self.manager.set(Some(r));r};Value::Obj(manager)},
                ("registerReceiver","(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;")=>{if let Some(receiver)=args[0].as_ref()? {if !vm.cp.is_subtype(vm.heap.class_of(receiver)?,RECEIVER){return Err(error("INVALID_RECEIVER","registerReceiver argument"));}}self.register(r,args[0].as_ref()?,obj(&args[1])?)?},
                ("registerReceiver","(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;Ljava/lang/String;Landroid/os/Handler;)Landroid/content/Intent;")=>{if args[2].as_ref()?.is_some()||args[3].as_ref()?.is_some(){return Err(unavailable("receiver permissions/custom scheduler"));}self.register(r,args[0].as_ref()?,obj(&args[1])?)?},
                ("unregisterReceiver","(Landroid/content/BroadcastReceiver;)V")=>{let receiver=obj(&args[0])?;let mut registrations=self.registrations.borrow_mut();let before=registrations.len();registrations.retain(|v|v.context!=r||v.receiver!=receiver);if registrations.len()==before{return Err(Throwable::new("Ljava/lang/IllegalArgumentException;","Receiver not registered for this Context").into());}Value::Null},
                ("sendBroadcast","(Landroid/content/Intent;)V")=>{self.send(android,vm,obj(&args[0])?)?;Value::Null},
                ("sendBroadcast","(Landroid/content/Intent;Ljava/lang/String;)V")=>{if args[1].as_ref()?.is_some(){return Err(unavailable("broadcast receiver permission"));}self.send(android,vm,obj(&args[0])?)?;Value::Null},
                ("bindService","(Landroid/content/Intent;Landroid/content/ServiceConnection;I)Z")=>{
                    let intent=obj(&args[0])?;let connection=obj(&args[1])?;
                    let metadata=self.intent(intent)?;if metadata.package.is_none()&&android.state.borrow().intents.get(&intent).is_none_or(String::is_empty){return Err(Throwable::new("Ljava/lang/IllegalArgumentException;","Service Intent must be explicit").into());}
                    // ContextImpl retains the dispatcher even when binding fails;
                    // callers must still unbind. This is not a Binder connection.
                    let mut bindings=self.bindings.borrow_mut();
                    if bindings.len()>=256&&!bindings.contains(&(r,connection)){return Err(error("SERVICE_BINDING_BUDGET","256 pending connection dispatchers"));}
                    bindings.insert((r,connection));Value::Int(0)
                },
                ("unbindService","(Landroid/content/ServiceConnection;)V")=>{
                    let connection=obj(&args[0])?;
                    if !self.bindings.borrow_mut().remove(&(r,connection)){return Err(Throwable::new("Ljava/lang/IllegalArgumentException;","Service not registered").into());}
                    Value::Null
                },
                _=>return Ok(None),
            }
        } else {
            return Ok(None);
        };
        Ok(Some(result))
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let mut roots: Vec<_> = self.manager.get().into_iter().collect();
        roots.extend(
            self.registrations
                .borrow()
                .iter()
                .flat_map(|r| [r.context, r.receiver]),
        );
        roots.extend(
            self.pending
                .borrow()
                .iter()
                .flat_map(|d| [d.context, d.receiver, d.intent]),
        );
        roots.extend(
            self.bindings
                .borrow()
                .iter()
                .flat_map(|(context, connection)| [*context, *connection]),
        );
        roots
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        self.filters.borrow_mut().retain(|r, _| live(r));
        self.intents.borrow_mut().retain(|r, _| live(r));
    }
}
