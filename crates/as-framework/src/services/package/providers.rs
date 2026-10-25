//! API26 authority lookup over committed installations. This returns declaration
//! metadata, not a running provider or a fabricated system-service installation.
use super::*;
use rd_apk::axml::XmlElement;

fn literal(element: &XmlElement, name: &str) -> Result<Option<String>> {
    match element.android_attr(name).map(|a| &a.value) {
        None | Some(AttrValue::Null) => Ok(None),
        Some(AttrValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(error(
            "API_UNAVAILABLE",
            format!("provider manifest nonliteral {name}"),
        )),
    }
}
fn boolean(element: &XmlElement, name: &str, default: bool) -> Result<bool> {
    element.android_attr(name).map_or(Ok(default), |a| {
        a.as_bool().ok_or_else(|| {
            error(
                "API_UNAVAILABLE",
                format!("provider manifest nonliteral {name}"),
            )
        })
    })
}
fn integer(element: &XmlElement, name: &str) -> Result<i32> {
    match element.android_attr(name).map(|a| &a.value) {
        None => Ok(0),
        Some(AttrValue::Int(value)) => Ok(*value),
        _ => Err(error(
            "API_UNAVAILABLE",
            format!("provider manifest noninteger {name}"),
        )),
    }
}
impl PackageService {
    pub(super) fn resolve_provider(
        &self,
        services: &Services,
        vm: &mut Engine,
        args: &[Value],
    ) -> Result<Value> {
        let authority = text(vm, &args[0])?;
        if int(args, 1)? != 0 {
            return Err(error(
                "API_UNAVAILABLE",
                "PackageManager.resolveContentProvider supports flags=0 only",
            ));
        }
        if authority.len() > 4096 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "4096 bytes per provider authority",
            ));
        }
        let records = as_package::PackageManager::new(services.fs.clone())
            .list()
            .map_err(storage_error)?;
        if records.len() > 1024 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "1024 installed packages per provider lookup",
            ));
        }
        let mut found = None;
        for record in records {
            let document = installed::document(services.fs.as_ref(), &record)?;
            let manifest = installed::application_manifest(&document)?;
            // versionName resources are unrelated to authority resolution. Do
            // not force them into a fabricated string, or reject their package.
            if manifest.package != record.package || manifest.version_code != record.version_code {
                return Err(error(
                    "INSTALLATION_CORRUPT",
                    "provider manifest identity/version differs from committed record",
                ));
            }
            if let Some(AttrValue::String(version)) =
                document.root.android_attr("versionName").map(|a| &a.value)
            {
                if record.version_name.as_deref() != Some(version) {
                    return Err(error(
                        "INSTALLATION_CORRUPT",
                        "provider manifest versionName differs from committed record",
                    ));
                }
            }
            let application = document.root.children_named("application").next().unwrap();
            if !boolean(application, "enabled", true)? {
                continue;
            }
            for provider in application.children_named("provider") {
                let Some(authorities) = literal(provider, "authorities")? else {
                    return Err(error("INVALID_APK", "provider has no authorities"));
                };
                if authorities.is_empty() {
                    return Err(error("INVALID_APK", "provider has empty authorities"));
                }
                if !authorities.split(';').any(|a| a == authority)
                    || !boolean(provider, "enabled", true)?
                {
                    continue;
                }
                if found.is_some() {
                    return Err(error(
                        "INSTALLATION_CORRUPT",
                        "duplicate installed provider authority",
                    ));
                }
                let source = self
                    .source
                    .borrow()
                    .as_ref()
                    .filter(|m| m.record == record)
                    .map(|m| m.guest.clone());
                found = Some(Self::provider_info(
                    vm,
                    &manifest,
                    application,
                    provider,
                    authorities,
                    source,
                )?);
            }
        }
        Ok(found.map_or(Value::Null, Value::Obj))
    }
    fn provider_info(
        vm: &mut Engine,
        manifest: &rd_apk::manifest::Manifest,
        application: &XmlElement,
        provider: &XmlElement,
        authorities: String,
        source: Option<String>,
    ) -> Result<ObjRef> {
        // These declarations require PatternMatcher/PathPermission objects.
        // An incomplete ProviderInfo must not silently erase access rules.
        if provider.children_named("path-permission").next().is_some() {
            return Err(error("API_UNAVAILABLE", "ProviderInfo.pathPermissions"));
        }
        let raw_name = literal(provider, "name")?
            .ok_or_else(|| error("INVALID_APK", "provider has no class name"))?;
        let name = if raw_name.starts_with('.') {
            format!("{}{raw_name}", manifest.package)
        } else if !raw_name.contains('.') {
            format!("{}.{raw_name}", manifest.package)
        } else {
            raw_name
        };
        let process = literal(provider, "process")?
            .filter(|p| !p.is_empty())
            .or(literal(application, "process")?.filter(|p| !p.is_empty()))
            .unwrap_or_else(|| manifest.package.clone());
        let process = if process.starts_with(':') {
            format!("{}{process}", manifest.package)
        } else {
            process
        };
        let permission = literal(provider, "permission")?.or(literal(application, "permission")?);
        let read = literal(provider, "readPermission")?
            .or(permission.clone())
            .filter(|p| !p.is_empty());
        let write_permission = literal(provider, "writePermission")?
            .or(permission)
            .filter(|p| !p.is_empty());
        let info = shell(vm, PROVIDER_INFO)?;
        let app = Self::application_info(vm, manifest, source, false)?;
        write(vm, info, "applicationInfo", Value::Obj(app))?;
        for (field, value) in [
            ("packageName", Some(manifest.package.clone())),
            ("name", Some(name)),
            ("authority", Some(authorities)),
            ("processName", Some(process)),
            ("readPermission", read),
            ("writePermission", write_permission),
        ] {
            let value = value.map_or(Ok(Value::Null), |s| string(vm, s))?;
            write(vm, info, field, value)?;
        }
        // API26 suppresses exported for single-user providers in ordinary,
        // nonprivileged APKs. AndroSwitch has no privileged/system APK install.
        let single_user = boolean(provider, "singleUser", false)?;
        let flags = (i32::from(single_user) * (1 << 30))
            | (i32::from(boolean(provider, "visibleToInstantApps", false)?) * (1 << 20));
        for (field, value) in [
            ("enabled", true),
            (
                "exported",
                boolean(provider, "exported", manifest.target_sdk < 17)? && !single_user,
            ),
            (
                "grantUriPermissions",
                boolean(provider, "grantUriPermissions", false)?
                    || provider
                        .children_named("grant-uri-permission")
                        .next()
                        .is_some(),
            ),
            ("multiprocess", boolean(provider, "multiprocess", false)?),
            (
                "directBootAware",
                boolean(provider, "directBootAware", false)?,
            ),
            ("isSyncable", boolean(provider, "syncable", false)?),
        ] {
            write(vm, info, field, Value::Int(i32::from(value)))?;
        }
        write(
            vm,
            info,
            "initOrder",
            Value::Int(integer(provider, "initOrder")?),
        )?;
        write(vm, info, "flags", Value::Int(flags))?;
        // GET_META_DATA / GET_URI_PERMISSION_PATTERNS were not requested.
        write(vm, info, "metaData", Value::Null)?;
        write(vm, info, "uriPermissionPatterns", Value::Null)?;
        write(vm, info, "pathPermissions", Value::Null)?;
        Ok(info)
    }
}
