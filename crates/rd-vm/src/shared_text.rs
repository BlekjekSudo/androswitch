//! Exact overload catalog and GC scope for the inherited String/regex family.
//! String rows are extracted from jlstring's actual match arms, not SDK class
//! declarations. Algorithms remain in the original jlstring/jregex modules.
use crate::framework_host::HostMethodKind;
#[cfg(not(feature = "desktop-runtime"))]
use crate::{Engine, ObjRef, Value, VmExit};
#[cfg(not(feature = "desktop-runtime"))]
use alloc::vec::Vec;
const STRING_INSTANCE: &[(&str, &str)] = &[
    ("codePointAt", "(I)I"),
    ("compareTo", "(Ljava/lang/Object;)I"),
    ("compareTo", "(Ljava/lang/String;)I"),
    ("compareToIgnoreCase", "(Ljava/lang/String;)I"),
    ("contains", "(Ljava/lang/CharSequence;)Z"),
    ("contentEquals", "(Ljava/lang/CharSequence;)Z"),
    ("contentEquals", "(Ljava/lang/StringBuffer;)Z"),
    ("equalsIgnoreCase", "(Ljava/lang/String;)Z"),
    ("getBytes", "()[B"),
    ("getBytes", "(Ljava/lang/String;)[B"),
    ("getBytes", "(Ljava/nio/charset/Charset;)[B"),
    ("indexOf", "(I)I"),
    ("indexOf", "(II)I"),
    ("indexOf", "(Ljava/lang/String;I)I"),
    ("isBlank", "()Z"),
    ("lastIndexOf", "(I)I"),
    ("lastIndexOf", "(II)I"),
    ("lastIndexOf", "(Ljava/lang/String;)I"),
    ("lastIndexOf", "(Ljava/lang/String;I)I"),
    ("matches", "(Ljava/lang/String;)Z"),
    ("regionMatches", "(ILjava/lang/String;II)Z"),
    ("regionMatches", "(ZILjava/lang/String;II)Z"),
    ("repeat", "(I)Ljava/lang/String;"),
    (
        "replace",
        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;",
    ),
    (
        "replaceAll",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
    ),
    (
        "replaceFirst",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
    ),
    ("split", "(Ljava/lang/String;)[Ljava/lang/String;"),
    ("split", "(Ljava/lang/String;I)[Ljava/lang/String;"),
    ("startsWith", "(Ljava/lang/String;I)Z"),
    ("strip", "()Ljava/lang/String;"),
    ("stripLeading", "()Ljava/lang/String;"),
    ("stripTrailing", "()Ljava/lang/String;"),
    ("subSequence", "(II)Ljava/lang/CharSequence;"),
    ("toLowerCase", "()Ljava/lang/String;"),
    ("toLowerCase", "(Ljava/util/Locale;)Ljava/lang/String;"),
    ("toUpperCase", "()Ljava/lang/String;"),
    ("toUpperCase", "(Ljava/util/Locale;)Ljava/lang/String;"),
    ("trim", "()Ljava/lang/String;"),
    ("<init>", "([B)V"),
    ("<init>", "([BII)V"),
    ("<init>", "([BIILjava/lang/String;)V"),
    ("<init>", "([BIILjava/nio/charset/Charset;)V"),
    ("<init>", "([BLjava/lang/String;)V"),
    ("<init>", "([BLjava/nio/charset/Charset;)V"),
    ("<init>", "([III)V"),
];
const STRING_STATIC: &[(&str, &str)] = &[
    (
        "format",
        "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
    ),
    (
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
    ),
    (
        "join",
        "(Ljava/lang/CharSequence;Ljava/lang/Iterable;)Ljava/lang/String;",
    ),
];

pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance, Static};
    if let Some(kind) = crate::jsb::method_metadata(class, name, sig) {
        return Some(kind);
    }
    if class == "Ljava/lang/String;" {
        if STRING_STATIC.contains(&(name, sig)) {
            return Some(Static);
        }
        return STRING_INSTANCE
            .contains(&(name, sig))
            .then_some(if name == "<init>" {
                Constructor
            } else {
                Instance
            });
    }
    let statics = class == "Ljava/util/regex/Pattern;"
        && matches!(
            (name, sig),
            (
                "compile",
                "(Ljava/lang/String;)Ljava/util/regex/Pattern;"
                    | "(Ljava/lang/String;I)Ljava/util/regex/Pattern;"
            ) | ("quote", "(Ljava/lang/String;)Ljava/lang/String;")
                | ("matches", "(Ljava/lang/String;Ljava/lang/CharSequence;)Z")
        );
    if statics {
        return Some(Static);
    }
    let implemented = match class {
        "Ljava/util/regex/Pattern;" => matches!(
            (name, sig),
            (
                "matcher",
                "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;"
            ) | ("pattern" | "toString", "()Ljava/lang/String;")
                | ("flags" | "hashCode", "()I")
                | (
                    "split",
                    "(Ljava/lang/CharSequence;)[Ljava/lang/String;"
                        | "(Ljava/lang/CharSequence;I)[Ljava/lang/String;"
                )
        ),
        "Ljava/util/regex/Matcher;" | "Ljava/util/regex/MatchResult;" => {
            let result = matches!(
                (name, sig),
                (
                    "group",
                    "()Ljava/lang/String;"
                        | "(I)Ljava/lang/String;"
                        | "(Ljava/lang/String;)Ljava/lang/String;"
                ) | ("start" | "end", "()I" | "(I)I" | "(Ljava/lang/String;)I")
                    | ("groupCount" | "hashCode", "()I")
                    | ("toString", "()Ljava/lang/String;")
            );
            result || (class=="Ljava/util/regex/Matcher;" && matches!((name,sig),
                ("find","()Z" | "(I)Z") | ("matches" | "lookingAt" | "hitEnd","()Z") |
                ("reset","()Ljava/util/regex/Matcher;" | "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;") |
                ("pattern","()Ljava/util/regex/Pattern;") |
                ("usePattern","(Ljava/util/regex/Pattern;)Ljava/util/regex/Matcher;") |
                ("regionStart" | "regionEnd", "()I") |
                ("replaceAll" | "replaceFirst","(Ljava/lang/String;)Ljava/lang/String;") |
                ("appendReplacement","(Ljava/lang/StringBuffer;Ljava/lang/String;)Ljava/util/regex/Matcher;" | "(Ljava/lang/StringBuilder;Ljava/lang/String;)Ljava/util/regex/Matcher;") |
                ("appendTail","(Ljava/lang/StringBuffer;)Ljava/lang/StringBuffer;" | "(Ljava/lang/StringBuilder;)Ljava/lang/StringBuilder;") |
                ("toMatchResult","()Ljava/util/regex/MatchResult;")
            ))
        }
        _ => false,
    };
    implemented.then_some(Instance)
}

#[cfg(not(feature = "desktop-runtime"))]
pub(crate) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if method_metadata(class, name, sig) != Some(HostMethodKind::Static) {
        return Ok(None);
    }
    crate::shared_call::with_values(vm, args, |vm| {
        if class == "Ljava/lang/String;" {
            crate::jlstring::static_intrinsic(vm, class, name, sig, args)
        } else {
            crate::jregex::static_intrinsic(vm, class, name, args)
        }
    })
}
#[cfg(not(feature = "desktop-runtime"))]
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        method_metadata(class, name, sig),
        Some(HostMethodKind::Instance | HostMethodKind::Constructor)
    ) {
        return Ok(None);
    }
    let mut roots = Vec::with_capacity(args.len() + 1);
    roots.push(Value::Obj(recv));
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| {
        if class == "Ljava/lang/String;" {
            // Keep the API26 ICU alias, BOM and pre-reservation/OOM contract
            // already verified by the device/JVM encoding corpus. The
            // inherited general text path must not regress that adapter when
            // its constructors are selected before the portable fallback.
            if name == "getBytes" && matches!(sig, "()[B" | "(Ljava/lang/String;)[B") {
                return crate::portable::file::call(vm, class, name, sig, recv, args);
            }
            crate::jlstring::instance_intrinsic(vm, class, name, sig, recv, args)
        } else if matches!(
            class,
            "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;"
        ) {
            if class == "Ljava/lang/StringBuffer;" {
                vm.monitor_enter(recv)?;
            }
            let result = crate::jsb::instance_intrinsic(vm, class, name, sig, recv, args);
            let released = if class == "Ljava/lang/StringBuffer;" {
                vm.monitor_exit(recv)
            } else {
                Ok(())
            };
            match result {
                Err(error) => Err(error),
                Ok(value) => {
                    released?;
                    Ok(value)
                }
            }
        } else {
            crate::jregex::instance_intrinsic(vm, class, name, recv, args)
        }
    })
}
