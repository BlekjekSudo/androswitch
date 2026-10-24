//! Reverse resource identity lookup. Values, aliases and API/density selection
//! do not change the owning package/type/key or the ID returned by this query.
use super::*;

/// API26 AssetManager name splitting (last ':' and '/', optional @ or @*).
/// A missing component requires an explicit default, not an APK-name guess.
pub fn identifier_name<'a>(
    name: &'a str,
    default_type: Option<&'a str>,
    default_package: Option<&'a str>,
) -> Option<ResourceName<'a>> {
    let name = name
        .strip_prefix("@*")
        .or_else(|| name.strip_prefix('@'))
        .unwrap_or(name);
    if name.is_empty() {
        return None;
    }
    let (package, rest) = match name.rsplit_once(':') {
        Some((package, rest)) => (package, rest),
        None => (default_package?, name),
    };
    let (resource_type, entry) = match rest.rsplit_once('/') {
        Some((kind, entry)) => (kind, entry),
        None => (default_type?, rest),
    };
    if package.is_empty() || resource_type.is_empty() || entry.is_empty() {
        return None;
    }
    Some(ResourceName {
        package,
        resource_type,
        entry,
    })
}

/// API26 ResourcesImpl first tries Java Integer.parseInt, radix10, over UTF16
/// chars. ICU58/Unicode9 BMP decimal ranges; astral digits are surrogate pairs
/// and are not accepted by Character.digit(char,10).
/// Source data: unicode-org/icu release-58-2 UnicodeData.txt (Nd, decimal0).
pub fn identifier_number(name: &str) -> Option<i32> {
    const STARTS: [u16; 37] = [
        0x0030, 0x0660, 0x06f0, 0x07c0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0be6, 0x0c66,
        0x0ce6, 0x0d66, 0x0de6, 0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946,
        0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0,
        0xa9f0, 0xaa50, 0xabf0, 0xff10,
    ];
    let (negative, digits) = if let Some(digits) = name.strip_prefix('-') {
        (true, digits)
    } else {
        (false, name.strip_prefix('+').unwrap_or(name))
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0i32;
    // Accumulate negatively to represent Integer.MIN_VALUE without overflow.
    for unit in digits.encode_utf16() {
        let digit = STARTS
            .iter()
            .find_map(|start| unit.checked_sub(*start).filter(|n| *n < 10))?
            as i32;
        value = value.checked_mul(10)?.checked_sub(digit)?;
    }
    if negative {
        Some(value)
    } else {
        value.checked_neg()
    }
}

impl Arsc {
    /// Exact identity search across actual package fragments. Conflicting IDs
    /// or malformed candidate metadata are explicit errors, never false absence.
    pub fn find_identifier(&self, query: ResourceName<'_>) -> RdResult<Option<u32>> {
        let mut found = None;
        for package in self
            .packages
            .iter()
            .filter(|package| package.name == query.package)
        {
            for &id in package.entries.keys() {
                let identity = package.resource_name(id).ok_or_else(|| {
                    RdError::invalid_format(format!("resource identity metadata for {id:#010x}"))
                })?;
                if identity.resource_type != query.resource_type || identity.entry != query.entry {
                    continue;
                }
                if self.resource_name(id) != Some(identity) {
                    return Err(RdError::new(
                        "AMBIGUOUS_RESOURCE_NAME",
                        format!("conflicting identity for {id:#010x}"),
                        crate::MODULE_ID,
                    ));
                }
                if found.is_some_and(|prior| prior != id) {
                    return Err(RdError::new(
                        "AMBIGUOUS_RESOURCE_NAME",
                        format!(
                            "{}:{}/{} has multiple IDs",
                            query.package, query.resource_type, query.entry
                        ),
                        crate::MODULE_ID,
                    ));
                }
                found = Some(id);
            }
        }
        if found.is_none() && query.resource_type == "attr" {
            return self.find_identifier(ResourceName {
                resource_type: "^attr-private",
                ..query
            });
        }
        Ok(found)
    }
}
