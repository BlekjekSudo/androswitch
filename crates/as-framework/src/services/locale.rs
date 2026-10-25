//! Base Locale identity and fields. Default language is the declared guest
//! profile, separate from an unimplemented Horizon system-language adapter.
use super::*;
use core::cell::Cell;
const CLASS: &str = "Ljava/util/Locale;";
#[derive(Default)]
pub(super) struct Locales {
    default: Cell<Option<ObjRef>>,
    root: Cell<Option<ObjRef>>,
}
fn assign(
    vm: &mut Engine,
    object: ObjRef,
    language: &str,
    country: &str,
    variant: &str,
) -> Result<()> {
    let language = match language.to_ascii_lowercase().as_str() {
        "he" => "iw".into(),
        "yi" => "ji".into(),
        "id" => "in".into(),
        value => value.to_string(),
    };
    for (name, text) in [
        ("language", language),
        ("country", country.to_ascii_uppercase()),
        ("variant", variant.to_string()),
    ] {
        let value = string(vm, text)?;
        write(vm, object, name, value)?;
    }
    Ok(())
}
fn create(vm: &mut Engine, language: &str, country: &str, variant: &str) -> Result<ObjRef> {
    let object = shell(vm, CLASS)?;
    assign(vm, object, language, country, variant)?;
    Ok(object)
}
impl Locales {
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.default
            .get()
            .into_iter()
            .chain(self.root.get())
            .collect()
    }
    pub(super) fn attach(vm: &mut Engine, class: &str, object: ObjRef) -> Result<bool> {
        if class != CLASS {
            return Ok(false);
        }
        for name in ["language", "country", "variant"] {
            write(vm, object, name, Value::Null)?;
        }
        Ok(true)
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        use rd_vm::framework_host::HostMethodKind::{Constructor, Instance, Static};
        match (class, name, sig) {
            (CLASS, "getDefault", "()Ljava/util/Locale;")
            | (CLASS, "setDefault", "(Ljava/util/Locale;)V") => Some(Static),
            (
                CLASS,
                "<init>",
                "(Ljava/lang/String;)V"
                | "(Ljava/lang/String;Ljava/lang/String;)V"
                | "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            ) => Some(Constructor),
            (
                CLASS,
                "getLanguage" | "getCountry" | "getVariant" | "toString",
                "()Ljava/lang/String;",
            ) => Some(Instance),
            _ => None,
        }
    }
    pub(super) fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        ty: &str,
    ) -> Result<Option<Value>> {
        if (class, name, ty) != (CLASS, "ROOT", CLASS) {
            return Ok(None);
        }
        let value = if let Some(value) = self.root.get() {
            value
        } else {
            let value = create(vm, "", "", "")?;
            self.root.set(Some(value));
            value
        };
        Ok(Some(Value::Obj(value)))
    }
    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class != CLASS {
            return Ok(None);
        }
        match (name, sig) {
            ("getDefault", "()Ljava/util/Locale;") => {
                let locale = if let Some(locale) = self.default.get() {
                    locale
                } else {
                    let tag = crate::properties::get("persist.sys.locale")
                        .ok_or_else(|| error("RUNTIME_CONFIGURATION", "missing guest locale"))?;
                    let (language, country) = tag.split_once('-').unwrap_or((tag, ""));
                    let locale = create(vm, language, country, "")?;
                    self.default.set(Some(locale));
                    locale
                };
                Ok(Some(Value::Obj(locale)))
            }
            ("setDefault", "(Ljava/util/Locale;)V") => {
                let locale = obj(&args[0])?;
                if vm.heap.class_of(locale)? != CLASS {
                    return Err(Throwable::new(
                        "Ljava/lang/ClassCastException;",
                        "expected Locale",
                    )
                    .into());
                }
                self.default.set(Some(locale));
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        }
    }
    pub(super) fn call(
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if vm.heap.class_of(receiver)? != CLASS {
            return Ok(None);
        }
        if class == CLASS
            && name == "<init>"
            && matches!(
                sig,
                "(Ljava/lang/String;)V"
                    | "(Ljava/lang/String;Ljava/lang/String;)V"
                    | "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"
            )
        {
            let language = text(vm, &args[0])?;
            let country = if args.len() > 1 {
                text(vm, &args[1])?
            } else {
                String::new()
            };
            let variant = if args.len() > 2 {
                text(vm, &args[2])?
            } else {
                String::new()
            };
            assign(vm, receiver, &language, &country, &variant)?;
            return Ok(Some(Value::Null));
        }
        if sig != "()Ljava/lang/String;" {
            return Ok(None);
        }
        let field = match name {
            "getLanguage" => "language",
            "getCountry" => "country",
            "getVariant" => "variant",
            _ => "",
        };
        if !field.is_empty() {
            return Ok(Some(vm.heap.get_field(receiver, field)?));
        }
        if name == "toString" {
            let mut language = text(vm, &vm.heap.get_field(receiver, "language")?)?;
            let country = text(vm, &vm.heap.get_field(receiver, "country")?)?;
            let variant = text(vm, &vm.heap.get_field(receiver, "variant")?)?;
            let nonempty = !language.is_empty() || !country.is_empty();
            if !country.is_empty() || (!language.is_empty() && !variant.is_empty()) {
                language.push('_');
                language.push_str(&country);
            }
            if nonempty && !variant.is_empty() {
                language.push('_');
                language.push_str(&variant);
            }
            return string(vm, language).map(Some);
        }
        Ok(None)
    }
}
