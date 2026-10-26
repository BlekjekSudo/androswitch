use super::*;
pub(super) mod application_lifecycle;
mod binder;
pub(super) mod bitmap;
mod bundle;
mod collections;
mod communication;
mod concurrent_map;
mod database;
mod device_info;
pub(super) mod executors;
pub(super) mod glsurface;
mod input_connection;
mod locale;
mod package;
mod pair;
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
    input_connections: input_connection::InputConnections,
    device_info: device_info::DeviceInfo,
    locales: locale::Locales,
    package_metadata: package::PackageService,
    reflection_fields: reflection::Fields,
    package: String,
    fs: Rc<dyn as_platform::fs::FileSystem>,
}
impl Services {
    pub(super) fn collection_snapshot(
        &self,
        vm: &Engine,
        object: ObjRef,
    ) -> Result<Option<rd_vm::collection_factories::Contents>> {
        self.collections.snapshot(vm, object)
    }
    pub(super) fn collection_create(
        &self,
        vm: &mut Engine,
        class: &str,
        contents: rd_vm::collection_factories::Contents,
    ) -> Result<ObjRef> {
        self.collections.create(vm, class, contents)
    }
    pub(super) fn write_log(&self, text: &str) -> Result<usize> {
        let directory = "logs/android";
        as_platform::fs::mkdirs(self.fs.as_ref(), directory).map_err(storage_error)?;
        let path = format!("{directory}/{}.log", self.package);
        let offset = self
            .fs
            .stat(&path)
            .map_err(storage_error)?
            .map_or(0, |m| m.length);
        let truncate = offset + text.len() as u64 > 1 << 20;
        self.fs
            .write_at(
                &path,
                if truncate { 0 } else { offset },
                text.as_bytes(),
                truncate,
            )
            .map_err(storage_error)
    }
    pub(super) fn initialize_runtime_class(vm: &mut Engine, class: &str) -> Result<()> {
        reflection::initialize_class(vm, class)
    }
    pub(super) fn static_field_metadata(class: &str, name: &str, descriptor: &str) -> bool {
        matches!(
            (class, name, descriptor),
            (
                "Landroid/os/Build;",
                "MODEL"
                    | "MANUFACTURER"
                    | "BRAND"
                    | "PRODUCT"
                    | "DEVICE"
                    | "BOARD"
                    | "HARDWARE"
                    | "ID"
                    | "DISPLAY"
                    | "FINGERPRINT"
                    | "TYPE"
                    | "TAGS"
                    | "USER"
                    | "HOST",
                "Ljava/lang/String;"
            ) | (
                "Landroid/os/Build;",
                "SUPPORTED_ABIS" | "SUPPORTED_64_BIT_ABIS" | "SUPPORTED_32_BIT_ABIS",
                "[Ljava/lang/String;"
            ) | ("Landroid/os/Build;", "TIME", "J")
                | (
                    "Landroid/os/Build$VERSION;",
                    "RELEASE" | "INCREMENTAL" | "CODENAME" | "BASE_OS" | "SECURITY_PATCH" | "SDK",
                    "Ljava/lang/String;"
                )
                | (
                    "Landroid/os/Build$VERSION;",
                    "SDK_INT" | "PREVIEW_SDK_INT",
                    "I"
                )
                | ("Ljava/util/Locale;", "ROOT", "Ljava/util/Locale;")
                | (
                    "Ljava/util/concurrent/TimeUnit;",
                    "NANOSECONDS"
                        | "MICROSECONDS"
                        | "MILLISECONDS"
                        | "SECONDS"
                        | "MINUTES"
                        | "HOURS"
                        | "DAYS",
                    "Ljava/util/concurrent/TimeUnit;"
                )
        )
    }
    pub(super) fn method_metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        if rd_vm::android_log::metadata(class, name, sig) {
            return Some(rd_vm::framework_host::HostMethodKind::Static);
        }
        if let Some(kind) = concurrent_map::metadata(class, name, sig) {
            return Some(kind);
        }
        if let Some(kind) = pair::metadata(class, name, sig) {
            return Some(kind);
        }
        if let Some(kind) = locale::Locales::metadata(class, name, sig) {
            return Some(kind);
        }
        if let Some(kind) = binder::metadata(class, name, sig) {
            return Some(kind);
        }
        if class == "Landroid/os/Debug;"
            && matches!(name, "isDebuggerConnected" | "waitingForDebugger")
            && sig == "()Z"
        {
            return Some(rd_vm::framework_host::HostMethodKind::Static);
        }
        executors::Executors::metadata(class, name, sig)
            .or_else(|| input_connection::InputConnections::metadata(class, name, sig))
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
                        | (
                            "Landroid/view/View;",
                            "setDefaultFocusHighlightEnabled",
                            "(Z)V"
                        )
                        | (
                            "Landroid/view/View;",
                            "getDefaultFocusHighlightEnabled",
                            "()Z"
                        )
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
            input_connections: Default::default(),
            device_info: Default::default(),
            locales: Default::default(),
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
        if concurrent_map::attach(vm, class, r)? {
            return Ok(true);
        }
        if pair::attach(vm, class, r)? {
            return Ok(true);
        }
        if locale::Locales::attach(vm, class, r)? {
            return Ok(true);
        }
        if binder::attach(vm, class, r)? {
            return Ok(true);
        }
        if self.input_connections.attach(vm, class, r)? {
            return Ok(true);
        }
        if matches!(
            class,
            "Landroid/content/pm/PackageInfo;" | "Landroid/content/pm/ProviderInfo;"
        ) {
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
        if rd_vm::boxes::static_field_descriptor(class, name) == Some(_descriptor) {
            return rd_vm::boxes::host_static(vm, class, name);
        }
        if let Some(value) = self.locales.static_field(vm, class, name, _descriptor)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .device_info
            .static_field(vm, class, name, _descriptor)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = self.executors.static_field(vm, class, name, _descriptor)? {
            return Ok(Some(value));
        }
        if class == "Landroid/os/Build$VERSION;" && name == "SDK_INT" && _descriptor == "I" {
            return Ok(Some(Value::Int(crate::properties::sdk_int().ok_or_else(
                || {
                    error(
                        "RUNTIME_CONFIGURATION",
                        "missing numeric Android SDK property",
                    )
                },
            )?)));
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
        if let Some(value) = pair::create(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.package_metadata.call_static(vm, class, name, sig)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.locales.call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if class == "Landroid/os/Debug;"
            && matches!(name, "isDebuggerConnected" | "waitingForDebugger")
            && sig == "()Z"
        {
            // This VM has no JDWP/debugger attachment or wait backend. Native
            // call tracing is diagnostics, not a connected Java debugger.
            return Ok(Some(Value::Int(0)));
        }
        if (class, name, sig)
            == (
                "Ljava/util/Arrays;",
                "asList",
                "([Ljava/lang/Object;)Ljava/util/List;",
            )
        {
            // Arrays.asList requires a fixed-size, array-backed view, not a detached ArrayList.
            return self
                .collections
                .array_list_view(vm, obj(&args[0])?)
                .map(Some);
        }
        if let Some(value) = text_utils::call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.runtime.call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .input_connections
            .call_static(vm, class, name, sig, args)?
        {
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
        if let Some(value) = rd_vm::android_log::call(
            vm,
            class,
            name,
            sig,
            args,
            |vm, reference| {
                if let Ok(text) = vm.heap.as_str(reference) {
                    return Ok(text.to_string());
                }
                let value =
                    vm.invoke_instance(reference, "toString", "()Ljava/lang/String;", &[])?;
                let text = obj(&value)?;
                Ok(vm.heap.as_str(text)?.to_string())
            },
            |priority| (4..=7).contains(&priority),
            |line| {
                let directory = "logs/android";
                as_platform::fs::mkdirs(self.fs.as_ref(), directory).map_err(storage_error)?;
                let path = format!("{directory}/{}.log", self.package);
                let offset = self
                    .fs
                    .stat(&path)
                    .map_err(storage_error)?
                    .map_or(0, |m| m.length);
                let truncate = offset + line.len() as u64 > 1 << 20;
                self.fs
                    .write_at(
                        &path,
                        if truncate { 0 } else { offset },
                        line.as_bytes(),
                        truncate,
                    )
                    .map_err(storage_error)
            },
        )? {
            return Ok(Some(value));
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
        if let Some(value) = concurrent_map::call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = pair::call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.runtime.call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = locale::Locales::call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = binder::call(vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .input_connections
            .call(android, vm, class, name, sig, r, args)?
        {
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
        roots.extend(self.input_connections.roots());
        roots.extend(self.device_info.roots());
        roots.extend(self.locales.roots());
        roots
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        let mut refs = self.collections.references(r);
        refs.extend(self.lifecycle_observers.references(r));
        refs.extend(self.preferences.references(r));
        refs.extend(self.database.references(r));
        refs.extend(self.executors.references(r));
        refs.extend(self.reflection_fields.references(r));
        refs.extend(self.input_connections.references(r));
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
        self.input_connections.collect(marked);
    }
}
fn storage_error(e: as_platform::PlatformError) -> VmExit {
    error(e.code, e.to_string())
}
