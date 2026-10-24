use super::*;
pub(super) mod application_lifecycle;
pub(super) mod bitmap;
mod bundle;
mod collections;
mod communication;
mod database;
pub(super) mod executors;
pub(super) mod glsurface;
mod package;
mod permissions;
mod preferences;
mod reflection;
mod resource_names;
pub(super) mod runtime;
mod text_utils;
pub(super) mod timer;
pub(super) struct Services {
    pub(super) lifecycle_observers: application_lifecycle::Observers,
    pub(super) bitmaps: bitmap::Bitmaps,
    collections: collections::Collections,
    communication: communication::Communication,
    preferences: preferences::PreferenceService,
    database: database::DatabaseService,
    pub(super) glsurface: glsurface::GlSurfaces,
    pub(super) timers: timer::Timers,
    pub(super) executors: executors::Executors,
    pub(super) runtime: runtime::Runtime,
    package_metadata: package::PackageService,
    reflection_fields: reflection::Fields,
    package: String,
    fs: Rc<dyn as_platform::fs::FileSystem>,
}
impl Services {
    pub(super) fn method_metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        executors::Executors::metadata(class, name, sig)
            .or_else(|| runtime::Runtime::metadata(class, name, sig))
            .or_else(|| package::PackageService::metadata(class, name, sig))
            .or_else(|| reflection::metadata(class, name, sig))
            .or_else(|| collections::Collections::metadata(class, name, sig))
            .or_else(|| application_lifecycle::Observers::metadata(class, name, sig))
            .or_else(|| {
                matches!(
                    (class, name, sig),
                    ("Ljava/lang/Object;", "equals", "(Ljava/lang/Object;)Z")
                        | ("Ljava/lang/String;", "endsWith", "(Ljava/lang/String;)Z")
                )
                .then_some(rd_vm::framework_host::HostMethodKind::Instance)
            })
    }
    pub(super) fn new(fs: Rc<dyn as_platform::fs::FileSystem>, apk: &Apk) -> Result<Self> {
        let store =
            as_storage::Store::new(fs.clone(), &apk.manifest.package).map_err(storage_error)?;
        Ok(Self {
            lifecycle_observers: Default::default(),
            bitmaps: Default::default(),
            collections: Default::default(),
            communication: Default::default(),
            preferences: preferences::PreferenceService::new(store.clone()),
            database: database::DatabaseService::new(store),
            glsurface: Default::default(),
            timers: Default::default(),
            executors: Default::default(),
            runtime: Default::default(),
            package_metadata: Default::default(),
            reflection_fields: Default::default(),
            package: apk.manifest.package.clone(),
            fs,
        })
    }
    pub(super) fn flush(&self) -> Result<()> {
        self.preferences.flush()
    }
    pub(super) fn pump(&self, android: &Android, vm: &mut Engine) -> Result<()> {
        if let Err(failure) = self.timers.pump() {
            self.executors.forward_worker_exception(vm, &failure);
            return Err(failure);
        }
        if let Err(failure) = self.executors.pump() {
            self.executors.forward_worker_exception(vm, &failure);
            return Err(failure);
        }
        self.communication.pump(android, vm)
    }
    pub(super) fn bind_package_source(
        &self,
        apk: &Apk,
        path: String,
        storage: String,
    ) -> Result<()> {
        self.package_metadata
            .bind_source(self.fs.clone(), apk, path, storage)
    }
    pub(super) fn attach(&self, vm: &mut Engine, class: &str, r: ObjRef) -> Result<bool> {
        if class == "Landroid/content/pm/PackageInfo;" {
            return Ok(true);
        }
        if self.bitmaps.attach(vm, class, r)? {
            return Ok(true);
        }
        if self.executors.attach(vm, class, r)? {
            return Ok(true);
        }
        if self.timers.attach(vm, class, r)? {
            return Ok(true);
        }
        if self.communication.attach(vm, class, r)? {
            return Ok(true);
        }
        if self.glsurface.attach(vm, class, r)? {
            return Ok(true);
        }
        if self.database.attach(vm, class, r) {
            return Ok(true);
        }
        self.collections.attach(vm, class, r)
    }
    pub(super) fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        _descriptor: &str,
    ) -> Result<Option<Value>> {
        if let Some(value) = self.executors.static_field(vm, class, name, _descriptor)? {
            return Ok(Some(value));
        }
        if name == "TYPE" {
            if let Some(descriptor) = collections::primitive(class) {
                return Ok(Some(Value::Obj(vm.device_class_object(descriptor)?)));
            }
        }
        if class == "Landroid/os/Build$VERSION;" && name == "SDK_INT" {
            return Ok(Some(Value::Int(crate::properties::sdk_int().ok_or_else(
                || {
                    error(
                        "RUNTIME_CONFIGURATION",
                        "missing numeric Android SDK property",
                    )
                },
            )?)));
        }
        if class == "Ljava/lang/Boolean;" && matches!(name, "TRUE" | "FALSE") {
            return self
                .collections
                .boxed(vm, class, Value::Int(i32::from(name == "TRUE")))
                .map(Some);
        }
        Ok(None)
    }
    pub(super) fn call_static(
        &self,
        android: &Android,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if let Some(value) = text_utils::call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.runtime.call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .executors
            .call_static(android, vm, class, name, sig, args)?
        {
            return Ok(Some(value));
        }
        if (self.glsurface.in_callback() || self.timers.in_callback(vm)?)
            && class == "Ljava/lang/Thread;"
            && name == "currentThread"
        {
            return Err(error(
                "API_UNAVAILABLE",
                "Java Thread identity for the platform callback worker",
            ));
        }
        if let Some(value) = reflection::call_static(self, vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if class == "Landroid/util/Log;" && name == "isLoggable" && sig == "(Ljava/lang/String;I)Z"
        {
            text(vm, &args[0])?;
            // Android's default tag threshold is INFO. No log.tag property
            // provider is installed; this is shared policy for every tag.
            return Ok(Some(Value::Int(i32::from(
                (4..=7).contains(&int(args, 1)?),
            ))));
        }
        if class == "Landroid/util/Log;"
            && matches!(name, "v" | "d" | "i" | "w" | "e" | "wtf")
            && sig == "(Ljava/lang/String;Ljava/lang/String;)I"
        {
            let tag = text(vm, &args[0])?;
            let message = text(vm, &args[1])?;
            let line = format!("[{name}] {tag}: {message}\n");
            if line.len() > 64 << 10 {
                return Err(error("LOG_BUDGET", "64 KiB per log entry"));
            }
            let directory = "logs/android";
            as_platform::fs::mkdirs(self.fs.as_ref(), directory).map_err(storage_error)?;
            let path = format!("{directory}/{}.log", self.package);
            let offset = self
                .fs
                .stat(&path)
                .map_err(storage_error)?
                .map_or(0, |m| m.length);
            let truncate = offset + line.len() as u64 > 1 << 20;
            let written = self
                .fs
                .write_at(
                    &path,
                    if truncate { 0 } else { offset },
                    line.as_bytes(),
                    truncate,
                )
                .map_err(storage_error)?;
            if written != line.len() {
                return Err(error("SHORT_WRITE", "android Log sink"));
            }
            return Ok(Some(Value::Int(written as i32)));
        }
        if let Some(value) = self.collections.call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if class == "Landroid/preference/PreferenceManager;"
            && name == "getDefaultSharedPreferences"
            && sig == "(Landroid/content/Context;)Landroid/content/SharedPreferences;"
        {
            obj(&args[0])?;
            return self
                .preferences
                .get(vm, &format!("{}_preferences", self.package))
                .map(Some);
        }
        Ok(None)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        android: &Android,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if let Some(value) = self.runtime.call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.lifecycle_observers.call(vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.bitmaps.call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = resource_names::call(android, vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .executors
            .call(android, vm, class, name, sig, r, args)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = self.timers.call(android, vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = bundle::call(android, vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .communication
            .call(self, android, vm, name, sig, r, args)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = reflection::call(self, vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.glsurface.call(android, vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .package_metadata
            .call(self, android, vm, name, sig, r, args)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = permissions::call(android, vm, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.database.call(android, vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .preferences
            .call(self, android, vm, class, name, sig, r, args)?
        {
            return Ok(Some(value));
        }
        self.collections.call(vm, class, name, sig, r, args)
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let mut roots = self.preferences.roots();
        roots.extend(self.lifecycle_observers.roots());
        roots.extend(self.collections.roots());
        roots.extend(self.package_metadata.roots());
        roots.extend(self.reflection_fields.roots());
        roots.extend(self.glsurface.roots());
        roots.extend(self.communication.roots());
        roots.extend(self.timers.roots());
        roots.extend(self.executors.roots());
        roots.extend(self.runtime.roots());
        roots
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        let mut refs = self.collections.references(r);
        refs.extend(self.lifecycle_observers.references(r));
        refs.extend(self.preferences.references(r));
        refs.extend(self.database.references(r));
        refs.extend(self.executors.references(r));
        refs.extend(self.reflection_fields.references(r));
        refs
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.lifecycle_observers.collect(marked);
        self.bitmaps.collect(marked);
        self.communication.collect(marked);
        self.timers.collect(marked);
        self.executors.collect(marked);
        self.glsurface.collect(marked);
        self.collections.collect(marked);
        self.preferences.collect(marked);
        self.database.collect(marked);
        self.reflection_fields.collect(marked);
    }
}
fn storage_error(e: as_platform::PlatformError) -> VmExit {
    error(e.code, e.to_string())
}
