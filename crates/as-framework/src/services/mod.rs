use super::*;
mod collections;
mod database;
pub(super) mod glsurface;
mod package;
mod permissions;
mod preferences;
mod reflection;
pub(super) struct Services {
    collections: collections::Collections,
    preferences: preferences::PreferenceService,
    database: database::DatabaseService,
    pub(super) glsurface: glsurface::GlSurfaces,
    package_metadata: package::PackageService,
    package: String,
    fs: Rc<dyn as_platform::fs::FileSystem>,
}
impl Services {
    pub(super) fn new(fs: Rc<dyn as_platform::fs::FileSystem>, apk: &Apk) -> Result<Self> {
        let store =
            as_storage::Store::new(fs.clone(), &apk.manifest.package).map_err(storage_error)?;
        Ok(Self {
            collections: Default::default(),
            preferences: preferences::PreferenceService::new(store.clone()),
            database: database::DatabaseService::new(store),
            glsurface: Default::default(),
            package_metadata: Default::default(),
            package: apk.manifest.package.clone(),
            fs,
        })
    }
    pub(super) fn flush(&self) -> Result<()> {
        self.preferences.flush()
    }
    pub(super) fn bind_package_source(&self, package: &str, path: String) -> Result<()> {
        self.package_metadata.bind_source(package, path)
    }
    pub(super) fn attach(&self, vm: &mut Engine, class: &str, r: ObjRef) -> Result<bool> {
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
        if name == "TYPE" {
            if let Some(descriptor) = collections::primitive(class) {
                return Ok(Some(Value::Obj(vm.device_class_object(descriptor)?)));
            }
        }
        if class == "Landroid/os/Build$VERSION;" && name == "SDK_INT" {
            return Ok(Some(Value::Int(26)));
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
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if self.glsurface.in_callback() && class == "Ljava/lang/Thread;" && name == "currentThread"
        {
            return Err(error(
                "API_UNAVAILABLE",
                "Java Thread identity for the platform GL worker",
            ));
        }
        if let Some(value) = reflection::call_static(vm, class, name, sig, args)? {
            return Ok(Some(value));
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
        roots.extend(self.collections.roots());
        roots.extend(self.package_metadata.roots());
        roots.extend(self.glsurface.roots());
        roots
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        let mut refs = self.collections.references(r);
        refs.extend(self.preferences.references(r));
        refs.extend(self.database.references(r));
        refs
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.glsurface.collect(marked);
        self.collections.collect(marked);
        self.preferences.collect(marked);
        self.database.collect(marked);
    }
}
fn storage_error(e: as_platform::PlatformError) -> VmExit {
    error(e.code, e.to_string())
}
