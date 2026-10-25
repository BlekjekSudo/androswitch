//! Installed ApplicationInfo queries, including typed manifest metadata.
use super::*;

impl PackageService {
    pub(super) fn installed_application_info(
        &self,
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        args: &[Value],
    ) -> Result<Value> {
        let flags = int(args, 1)?;
        let record = Self::lookup(services, vm, &args[0], flags & !128 == 0)?;
        if flags & !128 != 0 {
            return Err(error(
                "API_UNAVAILABLE",
                "PackageManager.getApplicationInfo supports flags=0 or GET_META_DATA",
            ));
        }
        let document = installed::document(services.fs.as_ref(), &record)?;
        let manifest = installed::application_manifest(&document)?;
        if manifest.package != record.package || manifest.version_code != record.version_code {
            return Err(error(
                "INSTALLATION_CORRUPT",
                "application manifest identity/version differs from committed record",
            ));
        }
        let source = self.source_for(&record);
        let own_resources = source.is_some();
        let info = Self::application_info(vm, &manifest, source, false)?;
        let application = document.root.children_named("application").next().unwrap();
        let enabled = match application.android_attr("enabled") {
            None => true,
            Some(a) => a
                .as_bool()
                .ok_or_else(|| error("API_UNAVAILABLE", "nonliteral application enabled"))?,
        };
        write(vm, info, "enabled", Value::Int(i32::from(enabled)))?;
        let metadata: Vec<_> = application.children_named("meta-data").collect();
        if metadata.len() > 1024 {
            return Err(error(
                "PACKAGE_QUERY_BUDGET",
                "1024 application metadata entries",
            ));
        }
        let bundle = if flags & 128 != 0 && !metadata.is_empty() {
            let bundle = shell(vm, "Landroid/os/Bundle;")?;
            for entry in metadata {
                let name = match entry.android_attr("name").map(|a| &a.value) {
                    Some(AttrValue::String(name)) if !name.is_empty() => name.clone(),
                    _ => {
                        return Err(error(
                            "API_UNAVAILABLE",
                            "nonliteral or missing application metadata name",
                        ))
                    }
                };
                // android:resource stores the ID; android:value dereferences it.
                let (suffix, sig, value) = if let Some(resource) = entry.android_attr("resource") {
                    match resource.value {
                        AttrValue::Reference(id) => ("Int", "I", Value::Int(id as i32)),
                        _ => {
                            return Err(error(
                                "API_UNAVAILABLE",
                                "nonresource application metadata resource",
                            ))
                        }
                    }
                } else {
                    match entry.android_attr("value").map(|a| &a.value) {
                        Some(AttrValue::String(value)) => {
                            ("String", "Ljava/lang/String;", string(vm, value.clone())?)
                        }
                        Some(AttrValue::Bool(value)) => {
                            ("Boolean", "Z", Value::Int(i32::from(*value)))
                        }
                        Some(AttrValue::Int(value)) => ("Int", "I", Value::Int(*value)),
                        Some(AttrValue::Float(value)) => ("Float", "F", Value::Float(*value)),
                        Some(AttrValue::Reference(id)) if own_resources => {
                            match android.resource(*id, 0)? {
                                ResolvedValue::Str(value) => {
                                    ("String", "Ljava/lang/String;", string(vm, value)?)
                                }
                                ResolvedValue::Integer(value) => ("Int", "I", Value::Int(value)),
                                ResolvedValue::Boolean(value) => {
                                    ("Boolean", "Z", Value::Int(i32::from(value)))
                                }
                                ResolvedValue::Float(value) => ("Float", "F", Value::Float(value)),
                                _ => {
                                    return Err(error(
                                        "API_UNAVAILABLE",
                                        "unsupported application metadata resource type",
                                    ))
                                }
                            }
                        }
                        Some(AttrValue::Reference(_)) => {
                            return Err(error(
                                "API_UNAVAILABLE",
                                "cross-installation application metadata resource resolution",
                            ))
                        }
                        _ => {
                            return Err(error(
                                "API_UNAVAILABLE",
                                "unsupported or missing application metadata value",
                            ))
                        }
                    }
                };
                vm.invoke_instance(
                    bundle,
                    &format!("put{suffix}"),
                    &format!("(Ljava/lang/String;{sig})V"),
                    &[Value::StrPlaceholder(name), value],
                )?;
            }
            Value::Obj(bundle)
        } else {
            Value::Null
        };
        write(vm, info, "metaData", bundle)?;
        Ok(Value::Obj(info))
    }
}
