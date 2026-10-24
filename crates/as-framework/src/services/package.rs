//! Package metadata and directories describe the same installation/VFS as the
//! application executes. Loaded DEX classes share one canonical class loader.
use super::*;

const CONTEXT: &str = "Landroid/content/Context;";
const CLASS_LOADER: &str = "Ljava/lang/ClassLoader;";
const BOOT_LOADER: &str = "Ljava/lang/BootClassLoader;";
const APPLICATION_INFO: &str = "Landroid/content/pm/ApplicationInfo;";

/// Provenance of the only loaders backed by this runtime's classpath.
/// A Java ClassLoader-shaped object alone does not confer loading behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoaderScope {
    Application,
    Bootstrap,
}

#[derive(Default)]
pub(super) struct PackageService {
    source: RefCell<Option<String>>,
    application_info: RefCell<Option<ObjRef>>,
    class_loader: RefCell<Option<ObjRef>>,
    boot_loader: RefCell<Option<ObjRef>>,
}
impl PackageService {
    pub(super) fn bind_source(&self, package: &str, path: String) -> Result<()> {
        if path != format!("/data/app/{package}/base.apk") {
            return Err(error(
                "INVALID_PACKAGE_SOURCE",
                "expected mounted installed APK alias",
            ));
        }
        let mut source = self.source.borrow_mut();
        if source.as_ref().is_some_and(|old| old != &path) {
            return Err(error("PACKAGE_STATE", "APK source is already bound"));
        }
        *source = Some(path);
        Ok(())
    }
    fn source(&self) -> Result<String> {
        self.source.borrow().clone().ok_or_else(|| {
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
        let loader = shell(vm, CLASS_LOADER)?;
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
        use rd_vm::framework_host::HostMethodKind::Instance;
        match (class, name, sig) {
            (CONTEXT | "Ljava/lang/Class;", "getClassLoader", "()Ljava/lang/ClassLoader;")
            | (CLASS_LOADER, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;") => {
                Some(Instance)
            }
            _ => None,
        }
    }
    fn info(&self, android: &Android, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(info) = *self.application_info.borrow() {
            return Ok(info);
        }
        let source = self.source()?;
        let manifest = &android.apk.manifest;
        let application = &manifest.application;
        let info = shell(vm, APPLICATION_INFO)?;
        for (name, value) in [
            ("packageName", Some(manifest.package.clone())),
            ("name", application.name.clone()),
            ("className", application.name.clone()),
            ("sourceDir", Some(source.clone())),
            ("publicSourceDir", Some(source)),
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
        let flags = (i32::from(application.debuggable == Some(true)) * 2)
            | (i32::from(application.has_code != Some(false)) * 4)
            | (i32::from(application.allow_backup != Some(false)) * (1 << 15))
            | (i32::from(application.extract_native_libs != Some(false)) * (1 << 28))
            | (i32::from(
                application
                    .hardware_accelerated
                    .unwrap_or(manifest.target_sdk >= 14),
            ) * (1 << 29))
            | (1 << 23); // FLAG_INSTALLED: source is bound to an installed revision.
        for (name, value) in [
            ("minSdkVersion", manifest.min_sdk as i32),
            ("targetSdkVersion", manifest.target_sdk as i32),
            ("labelRes", application.label_res.unwrap_or(0) as i32),
            ("icon", application.icon_res.unwrap_or(0) as i32),
            ("theme", application.theme_res.unwrap_or(0) as i32),
            ("flags", flags),
        ] {
            write(vm, info, name, Value::Int(value))?;
        }
        // nativeLibraryDir/uid/process and package-manager services are not
        // fabricated: their backing platform contracts are not implemented.
        *self.application_info.borrow_mut() = Some(info);
        Ok(info)
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
        let boot_loader = *self.boot_loader.borrow() == Some(receiver);
        if vm.cp.is_subtype(&actual, CLASS_LOADER)
            && name == "loadClass"
            && sig == "(Ljava/lang/String;)Ljava/lang/Class;"
        {
            self.resolve_loader(vm, Some(receiver))?;
        }
        if (app_loader || boot_loader)
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
            .chain(self.boot_loader.borrow().iter().copied())
            .collect()
    }
}
