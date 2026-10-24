//! Immutable Android compatibility facts, shared with the native property ABI.
//! Unknown keys stay absent; host environment and device identity are not read.

const SOURCE: &str = include_str!("../../../platform/switch-smoke/runtime_properties.inc");

fn entry(line: &str) -> Option<(&str, &str)> {
    line.trim()
        .strip_prefix("AS_RUNTIME_PROPERTY(\"")?
        .strip_suffix("\")")?
        .split_once("\", \"")
}

/// Values borrow immutable source bytes and require no allocation or OS access.
pub fn get(name: &str) -> Option<&'static str> {
    SOURCE.lines().filter_map(entry).find_map(
        |(key, value)| {
            if key == name {
                Some(value)
            } else {
                None
            }
        },
    )
}

pub fn sdk_int() -> Option<i32> {
    get("ro.build.version.sdk")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_manifest_has_unique_plain_bounded_values() {
        for (index, line) in SOURCE.lines().enumerate() {
            if !line.trim().starts_with("AS_RUNTIME_PROPERTY(") {
                continue;
            }
            let (key, value) = entry(line).expect("invalid shared property declaration");
            assert!(!key.is_empty() && key.is_ascii());
            assert!(value.len() < 92 && value.is_ascii());
            assert!(!key.contains(['"', '\\']) && !value.contains(['"', '\\']));
            assert!(!SOURCE
                .lines()
                .take(index)
                .filter_map(entry)
                .any(|(other, _)| other == key));
        }
    }

    #[test]
    fn java_sdk_and_native_abi_use_same_facts_with_unknown_keys_absent() {
        assert_eq!(sdk_int(), Some(26));
        assert_eq!(get("ro.product.cpu.abi"), Some("arm64-v8a"));
        assert_eq!(get("ro.product.cpu.abilist"), get("ro.product.cpu.abi"));
        assert_eq!(get("ro.product.cpu.abilist64"), get("ro.product.cpu.abi"));
        for missing in [
            "",
            "PATH",
            "ro.hardware",
            "ro.build.fingerprint",
            "ro.build.version.release",
            "RO.BUILD.VERSION.SDK",
        ] {
            assert_eq!(get(missing), None);
        }
    }
}
