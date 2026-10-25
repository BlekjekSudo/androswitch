//! Package metadata and directories describe the same installation/VFS as the
//! application executes. Loaded DEX classes share one canonical class loader.
use super::*;
mod application;
mod features;
mod installed;
mod providers;

const CONTEXT: &str = "Landroid/content/Context;";
const CLASS_LOADER: &str = "Ljava/lang/ClassLoader;";
const BOOT_LOADER: &str = "Ljava/lang/BootClassLoader;";
const PATH_LOADER: &str = "Ldalvik/system/PathClassLoader;";
const APPLICATION_INFO: &str = "Landroid/content/pm/ApplicationInfo;";
const PACKAGE_INFO: &str = "Landroid/content/pm/PackageInfo;";
const PROVIDER_INFO: &str = "Landroid/content/pm/ProviderInfo;";
const PACKAGE_MANAGER: &str = "Landroid/content/pm/PackageManager;";
const NAME_NOT_FOUND: &str = "Landroid/content/pm/PackageManager$NameNotFoundException;";
struct MountedSource {
    guest: String,
    record: as_package::Record,
}

/// Provenance of the only loaders backed by this runtime's classpath.
/// A Java ClassLoader-shaped object alone does not confer loading behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoaderScope {
    Application,
    System,
    Bootstrap,
}

#[derive(Default)]
pub(super) struct PackageService {
    source: RefCell<Option<MountedSource>>,
    application_info: RefCell<Option<ObjRef>>,
    class_loader: RefCell<Option<ObjRef>>,
    system_loader: RefCell<Option<ObjRef>>,
    boot_loader: RefCell<Option<ObjRef>>,
}
impl PackageService {
    pub(super) fn bind_source(
        &self,
        fs: Rc<dyn as_platform::fs::FileSystem>,
        apk: &Apk,
        path: String,
        storage: String,
    ) -> Result<()> {
        let package = &apk.manifest.package;
        if path != format!("/data/app/{package}/base.apk") {
            return Err(error(
                "INVALID_PACKAGE_SOURCE",
                "expected mounted installed APK alias",
            ));
        }
        let record = as_package::PackageManager::new(fs.clone())
            .latest(package)
            .map_err(storage_error)?
            .filter(|r| r.installed)
            .ok_or_else(|| {
                error(
                    "INVALID_PACKAGE_SOURCE",
                    "mounted APK has no installed record",
                )
            })?;
        if storage != installed::path(&record)
            || apk.manifest.version_code != record.version_code
            || apk.manifest.version_name != record.version_name
            || !fs
                .stat(&storage)
                .map_err(storage_error)?
                .is_some_and(|m| !m.directory && m.length == record.apk_bytes)
        {
            return Err(error(
                "INVALID_PACKAGE_SOURCE",
                "mounted APK differs from current immutable installation",
            ));
        }
        let mut source = self.source.borrow_mut();
        if source
            .as_ref()
            .is_some_and(|old| old.guest != path || old.record != record)
        {
            return Err(error("PACKAGE_STATE", "APK source is already bound"));
        }
        *source = Some(MountedSource {
            guest: path,
            record,
        });
        Ok(())
    }
    fn source(&self) -> Result<String> {
        self.source
            .borrow()
            .as_ref()
            .map(|source| source.guest.clone())
            .ok_or_else(|| {
                error(
                    "API_UNAVAILABLE",
                    "installed APK source path is not bound to the VFS",
                )
            })
    }
    pub(super) fn app_loader(&self, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(loader) = *self.class_loader.borrow() {
            return Ok(loader);
        }
        let loader = shell(vm, PATH_LOADER)?;
        *self.class_loader.borrow_mut() = Some(loader);
        Ok(loader)
    }
    pub(super) fn bootstrap_loader(&self, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(loader) = *self.boot_loader.borrow() {
            return Ok(loader);
        }
        let loader = shell(vm, BOOT_LOADER)?;
        *self.boot_loader.borrow_mut() = Some(loader);
        Ok(loader)
    }
    fn system_loader(&self, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(loader) = *self.system_loader.borrow() {
            return Ok(loader);
        }
        // API26 has a system PathClassLoader between the app and bootstrap
        // loaders. Its boot/framework classpath does not include APK DEX.
        let loader = shell(vm, PATH_LOADER)?;
        *self.system_loader.borrow_mut() = Some(loader);
        Ok(loader)
    }
    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
    ) -> Result<Option<Value>> {
        if (class, name, sig)
            == (
                CLASS_LOADER,
                "getSystemClassLoader",
                "()Ljava/lang/ClassLoader;",
            )
        {
            return Ok(Some(Value::Obj(self.system_loader(vm)?)));
        }
        Ok(None)
    }
    pub(super) fn resolve_loader(
        &self,
        vm: &Engine,
        loader: Option<ObjRef>,
    ) -> Result<LoaderScope> {
        let Some(loader) = loader else {
            return Ok(LoaderScope::Bootstrap);
        };
        if *self.class_loader.borrow() == Some(loader) {
            return Ok(LoaderScope::Application);
        }
        if *self.system_loader.borrow() == Some(loader) {
            return Ok(LoaderScope::System);
        }
        if *self.boot_loader.borrow() == Some(loader) {
            return Ok(LoaderScope::Bootstrap);
        }
        if !vm.cp.is_subtype(vm.heap.class_of(loader)?, CLASS_LOADER) {
            return Err(
                Throwable::new("Ljava/lang/ClassCastException;", "expected ClassLoader").into(),
            );
        }
        Err(error(
            "API_UNAVAILABLE",
            "custom or unregistered ClassLoader has no loading backend",
        ))
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        use rd_vm::framework_host::HostMethodKind::{Instance, Static};
        match (class, name, sig) {
            (
                PACKAGE_MANAGER,
                "hasSystemFeature",
                "(Ljava/lang/String;)Z" | "(Ljava/lang/String;I)Z",
            ) => Some(Instance),
            (CLASS_LOADER, "getSystemClassLoader", "()Ljava/lang/ClassLoader;") => Some(Static),
            (CLASS_LOADER, "getParent", "()Ljava/lang/ClassLoader;") => Some(Instance),
            (CONTEXT | "Ljava/lang/Class;", "getClassLoader", "()Ljava/lang/ClassLoader;")
            | (CLASS_LOADER, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;")
            | (CONTEXT, "getPackageManager", "()Landroid/content/pm/PackageManager;")
            | (
                PACKAGE_MANAGER,
                "getPackageInfo",
                "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
            )
            | (
                PACKAGE_MANAGER,
                "getApplicationInfo",
                "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;",
            )
            | (
                PACKAGE_MANAGER,
                "resolveContentProvider",
                "(Ljava/lang/String;I)Landroid/content/pm/ProviderInfo;",
            ) => Some(Instance),
            _ => None,
        }
    }
    fn info(&self, android: &Android, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(info) = *self.application_info.borrow() {
            return Ok(info);
        }
        let source = self.source()?;
        let info = Self::application_info(vm, &android.apk.manifest, Some(source), true)?;
        *self.application_info.borrow_mut() = Some(info);
        Ok(info)
    }
    fn application_info(
        vm: &mut Engine,
        manifest: &rd_apk::manifest::Manifest,
        source: Option<String>,
        legacy_context_flags: bool,
    ) -> Result<ObjRef> {
        let application = &manifest.application;
        let info = shell(vm, APPLICATION_INFO)?;
        for (name, value) in [
            ("packageName", Some(manifest.package.clone())),
            ("name", application.name.clone()),
            ("className", application.name.clone()),
            (
                "dataDir",
                Some(format!("/data/user/0/{}", manifest.package)),
            ),
            (
                "nonLocalizedLabel",
                application
                    .label_res
                    .is_none()
                    .then(|| application.label.clone())
                    .flatten(),
            ),
        ] {
            let value = value.map_or(Ok(Value::Null), |v| string(vm, v))?;
            write(vm, info, name, value)?;
        }
        if let Some(source) = source {
            let value = string(vm, source)?;
            write(vm, info, "sourceDir", value.clone())?;
            write(vm, info, "publicSourceDir", value)?;
        }
        if legacy_context_flags {
            // Preserve the preexisting Context subset separately. A fresh
            // PackageInfo must not advertise this incomplete full bitset.
            let flags = (i32::from(application.debuggable == Some(true)) * 2)
                | (i32::from(application.has_code != Some(false)) * 4)
                | (i32::from(application.allow_backup != Some(false)) * (1 << 15))
                | (i32::from(application.extract_native_libs != Some(false)) * (1 << 28))
                | (i32::from(
                    application
                        .hardware_accelerated
                        .unwrap_or(manifest.target_sdk >= 14),
                ) * (1 << 29))
                | (1 << 23);
            write(vm, info, "flags", Value::Int(flags))?;
        }
        for (name, value) in [
            ("minSdkVersion", manifest.min_sdk as i32),
            ("targetSdkVersion", manifest.target_sdk as i32),
            ("labelRes", application.label_res.unwrap_or(0) as i32),
            ("icon", application.icon_res.unwrap_or(0) as i32),
            ("theme", application.theme_res.unwrap_or(0) as i32),
        ] {
            write(vm, info, name, Value::Int(value))?;
        }
        // nativeLibraryDir/uid/process and package-manager services are not
        // fabricated: their backing platform contracts are not implemented.
        Ok(info)
    }
    fn lookup(
        services: &Services,
        vm: &mut Engine,
        name: &Value,
        installed_only: bool,
    ) -> Result<as_package::Record> {
        let missing = |vm: &mut Engine, name: Option<String>| {
            let mut throwable = Throwable {
                class: NAME_NOT_FOUND.into(),
                message: name,
                obj: None,
            };
            // Materialize before diagnostic frame strings are attached to the
            // transport error. Java getMessage retains the actual nullable
            // package argument through an intermediate DEX call/catch.
            match vm.materialize_throwable(&throwable) {
                Ok(object) => {
                    throwable.obj = Some(object);
                    VmExit::Exception(throwable)
                }
                Err(error) => error,
            }
        };
        let Some(name) = name.as_ref()? else {
            return Err(missing(vm, None));
        };
        let name = vm.heap.as_str(name)?;
        if name.len() > 4096 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "4096 bytes per queried package name",
            ));
        }
        let name = name.to_string();
        if !as_platform::vfs::valid_package(&name) {
            return Err(missing(vm, Some(name)));
        }
        match as_package::PackageManager::new(services.fs.clone()).latest(&name) {
            Ok(Some(record)) if record.installed || !installed_only => Ok(record),
            Ok(_) => Err(missing(vm, Some(name))),
            Err(e) if e.code == "PACKAGE_CASE_COLLISION" => Err(missing(vm, Some(name))),
            Err(e) => Err(storage_error(e)),
        }
    }
    fn source_for(&self, record: &as_package::Record) -> Option<String> {
        self.source
            .borrow()
            .as_ref()
            .filter(|mounted| {
                mounted.record.package == record.package
                    && mounted.record.revision == record.revision
                    && mounted.record.apk_sha256 == record.apk_sha256
                    && mounted.record.apk_bytes == record.apk_bytes
            })
            .map(|mounted| mounted.guest.clone())
    }
    fn package_info(&self, services: &Services, vm: &mut Engine, args: &[Value]) -> Result<Value> {
        let flags = int(args, 1)?;
        // A package with no registry history is absent regardless of requested
        // fields. Retained/uninstalled records still require the unsupported
        // visibility flags to be implemented before answering those queries.
        let record = Self::lookup(services, vm, &args[0], flags == 0)?;
        if flags != 0 {
            return Err(error(
                "API_UNAVAILABLE",
                "PackageManager.getPackageInfo supports flags=0 only",
            ));
        }
        let metadata = installed::read(services.fs.as_ref(), &record)?;
        let source = self.source_for(&record);
        let info = shell(vm, PACKAGE_INFO)?;
        let application = Self::application_info(vm, &metadata.manifest, source, false)?;
        let package = string(vm, record.package)?;
        write(vm, info, "packageName", package)?;
        write(
            vm,
            info,
            "versionCode",
            Value::Int(metadata.manifest.version_code.unwrap_or(0) as u32 as i32),
        )?;
        let version = metadata
            .version_name
            .map_or(Ok(Value::Null), |name| string(vm, name))?;
        write(vm, info, "versionName", version)?;
        write(vm, info, "applicationInfo", Value::Obj(application))?;
        Ok(Value::Obj(info))
    }
    fn directory(
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        tail: &str,
    ) -> Result<Value> {
        let package = &android.apk.manifest.package;
        as_platform::fs::mkdirs(services.fs.as_ref(), &format!("userdata/{package}/{tail}"))
            .map_err(storage_error)?;
        let file = shell(vm, "Ljava/io/File;")?;
        vm.invoke_instance(
            file,
            "<init>",
            "(Ljava/lang/String;)V",
            &[Value::StrPlaceholder(format!(
                "/data/user/0/{package}/{tail}"
            ))],
        )?;
        Ok(Value::Obj(file))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let actual = vm.heap.class_of(receiver)?.to_string();
        if vm.cp.is_subtype(&actual, PACKAGE_MANAGER)
            && name == "hasSystemFeature"
            && matches!(sig, "(Ljava/lang/String;)Z" | "(Ljava/lang/String;I)Z")
        {
            return features::has(vm, args).map(Some);
        }
        if vm.cp.is_subtype(&actual, PACKAGE_MANAGER)
            && name == "getApplicationInfo"
            && sig == "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;"
        {
            return self
                .installed_application_info(services, android, vm, args)
                .map(Some);
        }
        if vm.cp.is_subtype(&actual, PACKAGE_MANAGER)
            && name == "resolveContentProvider"
            && sig == "(Ljava/lang/String;I)Landroid/content/pm/ProviderInfo;"
        {
            return self.resolve_provider(services, vm, args).map(Some);
        }
        if vm.cp.is_subtype(&actual, PACKAGE_MANAGER)
            && name == "getPackageInfo"
            && sig == "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;"
        {
            return self.package_info(services, vm, args).map(Some);
        }
        if vm.cp.is_subtype(&actual, CONTEXT) {
            let value = match (name, sig) {
                ("getApplicationInfo", "()Landroid/content/pm/ApplicationInfo;") => {
                    Value::Obj(self.info(android, vm)?)
                }
                ("getClassLoader", "()Ljava/lang/ClassLoader;") => Value::Obj(self.app_loader(vm)?),
                ("getPackageCodePath" | "getPackageResourcePath", "()Ljava/lang/String;") => {
                    string(vm, self.source()?)?
                }
                ("getFilesDir", "()Ljava/io/File;") => {
                    Self::directory(services, android, vm, "files")?
                }
                ("getCacheDir", "()Ljava/io/File;") => {
                    Self::directory(services, android, vm, "cache")?
                }
                ("getCodeCacheDir", "()Ljava/io/File;") => {
                    Self::directory(services, android, vm, "code_cache")?
                }
                ("getNoBackupFilesDir", "()Ljava/io/File;") => {
                    Self::directory(services, android, vm, "no_backup")?
                }
                ("getDataDir", "()Ljava/io/File;") => Self::directory(services, android, vm, "")?,
                ("getDir", "(Ljava/lang/String;I)Ljava/io/File;") => {
                    let directory = text(vm, &args[0])?;
                    if directory.contains(['/', '\0']) {
                        return Err(Throwable::new(
                            "Ljava/lang/IllegalArgumentException;",
                            "invalid private directory name",
                        )
                        .into());
                    }
                    if directory.contains(['\\', ':']) {
                        return Err(error(
                            "API_UNAVAILABLE",
                            "directory name is unsupported by the mounted filesystem",
                        ));
                    }
                    if int(args, 1)? != 0 {
                        return Err(error(
                            "API_UNAVAILABLE",
                            "Context.getDir supports MODE_PRIVATE only",
                        ));
                    }
                    Self::directory(services, android, vm, &format!("app_{directory}"))?
                }
                _ => return Ok(None),
            };
            return Ok(Some(value));
        }
        let app_loader = *self.class_loader.borrow() == Some(receiver);
        let system_loader = *self.system_loader.borrow() == Some(receiver);
        let boot_loader = *self.boot_loader.borrow() == Some(receiver);
        if vm.cp.is_subtype(&actual, CLASS_LOADER)
            && name == "getParent"
            && sig == "()Ljava/lang/ClassLoader;"
        {
            self.resolve_loader(vm, Some(receiver))?;
            return Ok(Some(if app_loader {
                Value::Obj(self.system_loader(vm)?)
            } else if system_loader {
                Value::Obj(self.bootstrap_loader(vm)?)
            } else {
                Value::Null
            }));
        }
        if vm.cp.is_subtype(&actual, CLASS_LOADER)
            && name == "loadClass"
            && sig == "(Ljava/lang/String;)Ljava/lang/Class;"
        {
            self.resolve_loader(vm, Some(receiver))?;
        }
        if (app_loader || system_loader || boot_loader)
            && name == "loadClass"
            && sig == "(Ljava/lang/String;)Ljava/lang/Class;"
        {
            let name = text(vm, &args[0])?;
            let class = descriptor(&name);
            if name.is_empty()
                || name.contains(['[', '/', ';', '\0'])
                || (!rd_vm::classpath::host_class_known(&class)
                    && (!app_loader || vm.cp.find_class(&class).is_none()))
            {
                return Err(Throwable::new("Ljava/lang/ClassNotFoundException;", name).into());
            }
            // ClassLoader.loadClass resolves metadata; it must not invoke
            // the class initializer (Class.forName normally does).
            super::reflection::link_class(vm, &class, self.resolve_loader(vm, Some(receiver))?)?;
            return Ok(Some(Value::Obj(vm.device_class_object(&class)?)));
        }
        if actual == "Ljava/lang/Class;"
            && name == "getClassLoader"
            && sig == "()Ljava/lang/ClassLoader;"
        {
            let class = text(vm, &vm.heap.get_field(receiver, "class_name")?)?;
            let element = class.trim_start_matches('[');
            return Ok(Some(if class.len() == 1 {
                Value::Null // Android returns null only for primitive Class objects.
            } else if vm.cp.find_class(element).is_some() {
                Value::Obj(self.app_loader(vm)?)
            } else {
                Value::Obj(self.bootstrap_loader(vm)?)
            }));
        }
        Ok(None)
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.application_info
            .borrow()
            .iter()
            .copied()
            .chain(self.class_loader.borrow().iter().copied())
            .chain(self.system_loader.borrow().iter().copied())
            .chain(self.boot_loader.borrow().iter().copied())
            .collect()
    }
}
