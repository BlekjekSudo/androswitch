//! `SharedPreferences` persistent value model and Android XML file format.
//!
//! Android's `SharedPreferencesImpl` stores each named preference set in
//! `<shared_prefs>/<name>.xml`, as a `<map>` of typed entries. RustyDroid's
//! filesystem is the app-private virtual filesystem, but the file name and
//! XML value forms follow AOSP so a later Engine launch reads the same bytes
//! a previous launch wrote.
//!
//! This module is deliberately pure: it owns the typed value representation
//! and the XML codec, while `framework.rs` owns guest-object materialization
//! and listener notification.

use std::collections::{BTreeMap, BTreeSet};

/// A `SharedPreferences` value. Floats are stored by IEEE-754 bits so
/// equality and persistence preserve `-0.0`, infinities and NaN sign/payload
/// instead of inheriting Rust's `f32 ==` corner cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefValue {
    String(String),
    Int(i32),
    Long(i64),
    Float(u32),
    Bool(bool),
    StringSet(BTreeSet<Option<String>>),
}

impl PrefValue {
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            PrefValue::String(_) => "String",
            PrefValue::Int(_) => "Integer",
            PrefValue::Long(_) => "Long",
            PrefValue::Float(_) => "Float",
            PrefValue::Bool(_) => "Boolean",
            PrefValue::StringSet(_) => "Set<String>",
        }
    }
}

pub(crate) type PrefStore = BTreeMap<String, PrefValue>;

/// The AOSP package-private XML file for a preference set.
pub(crate) fn path(package: &str, name: &str) -> String {
    let package = if package.is_empty() { "app" } else { package };
    format!(
        "/data/user/0/{package}/shared_prefs/{}.xml",
        file_component(name)
    )
}

/// `PreferenceManager`'s default file name for a package.
pub(crate) fn default_name(package: &str) -> String {
    let package = if package.is_empty() { "app" } else { package };
    format!("{package}_preferences")
}

/// Keep a preference name inside its `shared_prefs` directory. Normal names
/// are unchanged; separators and control characters are percent-encoded by
/// UTF-8 byte instead of becoming path segments.
fn file_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '%' || ch == '/' || ch == '\\' || ch.is_control() {
            let mut buf = [0u8; 4];
            for byte in ch.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            ch if ch.is_control() && ch != '\u{85}' => out.push_str(&format!("&#{};", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            ch if ch.is_control() && ch != '\t' && ch != '\n' && ch != '\u{85}' => {
                out.push_str(&format!("&#{};", ch as u32))
            }
            ch => out.push(ch),
        }
    }
    out
}

fn float_text(bits: u32) -> String {
    let value = f32::from_bits(bits);
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f32::INFINITY {
        "Infinity".to_string()
    } else if value == f32::NEG_INFINITY {
        "-Infinity".to_string()
    } else if bits == (-0.0f32).to_bits() {
        "-0.0".to_string()
    } else {
        // Rust's `f32` formatting is a shortest round-trip representation.
        value.to_string()
    }
}

/// Serialize one preference store in AOSP's `XmlUtils.writeMapXml` shape.
pub(crate) fn serialize(store: &PrefStore) -> Vec<u8> {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8' standalone='yes' ?>\n<map>\n");
    for (key, value) in store {
        let key = escape_attr(key);
        match value {
            PrefValue::String(value) => {
                out.push_str(&format!(
                    "<string name=\"{key}\">{}</string>\n",
                    escape_text(value)
                ));
            }
            PrefValue::Int(value) => {
                out.push_str(&format!("<int name=\"{key}\" value=\"{value}\" />\n"));
            }
            PrefValue::Long(value) => {
                out.push_str(&format!("<long name=\"{key}\" value=\"{value}\" />\n"));
            }
            PrefValue::Float(bits) => {
                out.push_str(&format!(
                    "<float name=\"{key}\" value=\"{}\" />\n",
                    float_text(*bits)
                ));
            }
            PrefValue::Bool(value) => {
                out.push_str(&format!("<boolean name=\"{key}\" value=\"{value}\" />\n"));
            }
            PrefValue::StringSet(values) => {
                out.push_str(&format!("<set name=\"{key}\">\n"));
                for value in values {
                    match value {
                        Some(value) => {
                            out.push_str(&format!("<string>{}</string>\n", escape_text(value)))
                        }
                        None => out.push_str("<null />\n"),
                    }
                }
                out.push_str("</set>\n");
            }
        }
    }
    out.push_str("</map>\n");
    out.into_bytes()
}

fn decode_entities(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after.find(';')?;
        let entity = &after[..end];
        let decoded = match entity {
            "amp" => '&'.to_string(),
            "lt" => '<'.to_string(),
            "gt" => '>'.to_string(),
            "quot" => '"'.to_string(),
            "apos" => '\''.to_string(),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                char::from_u32(u32::from_str_radix(&entity[2..], 16).ok()?)?.to_string()
            }
            _ if entity.starts_with('#') => {
                char::from_u32(entity[1..].parse::<u32>().ok()?)?.to_string()
            }
            _ => return None,
        };
        out.push_str(&decoded);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn tag_name(tag: &str) -> &str {
    let tag = tag.trim_start();
    let end = tag
        .find(|ch: char| ch.is_whitespace() || ch == '/')
        .unwrap_or(tag.len());
    &tag[..end]
}

fn attributes(tag: &str) -> Option<Vec<(String, String)>> {
    let mut attrs = Vec::new();
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let before = &rest[..eq];
        let name = before
            .rsplit(|ch: char| ch.is_whitespace())
            .next()
            .filter(|name| !name.is_empty())?
            .to_string();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let value_and_rest = &after[quote.len_utf8()..];
        let end = value_and_rest.find(quote)?;
        attrs.push((name, decode_entities(&value_and_rest[..end])?));
        rest = &value_and_rest[end + quote.len_utf8()..];
    }
    Some(attrs)
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.as_str())
}

/// Parse an AOSP SharedPreferences XML map. Malformed input is `None`; the
/// caller follows `SharedPreferencesImpl`'s recover-to-empty behavior rather
/// than turning a corrupt app-private file into a VM failure.
pub(crate) fn parse(bytes: &[u8]) -> Option<PrefStore> {
    // SharedPreferences XML is small key/value data; refuse a pathological
    // app-private file instead of spending unbounded memory on recovery.
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let map_start = text.find("<map")?;
    let mut rest = &text[map_start..];
    let mut store = PrefStore::new();

    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        if let Some(after_bang) = rest.strip_prefix("!--") {
            let close = after_bang.find("-->")?;
            rest = &after_bang[close + 3..];
            continue;
        }
        if rest.starts_with('?') || rest.starts_with('!') {
            let close = rest.find('>')?;
            rest = &rest[close + 1..];
            continue;
        }
        if let Some(close_tag) = rest.strip_prefix('/') {
            if tag_name(close_tag) == "map" {
                return Some(store);
            }
            let close = rest.find('>')?;
            rest = &rest[close + 1..];
            continue;
        }
        let close = rest.find('>')?;
        let tag = &rest[..close];
        let name_of_tag = tag_name(tag).to_string();
        let self_closing = tag.trim_end().ends_with('/');
        rest = &rest[close + 1..];
        if name_of_tag == "map" {
            if self_closing {
                return Some(store);
            }
            continue;
        }
        let attrs = attributes(tag)?;
        let Some(key) = attr(&attrs, "name").map(str::to_string) else {
            continue;
        };
        match name_of_tag.as_str() {
            "string" => {
                if self_closing {
                    store.insert(key, PrefValue::String(String::new()));
                } else {
                    let end = rest.find("</string>")?;
                    store.insert(key, PrefValue::String(decode_entities(&rest[..end])?));
                    rest = &rest[end + "</string>".len()..];
                }
            }
            "int" => {
                store.insert(key, PrefValue::Int(attr(&attrs, "value")?.parse().ok()?));
            }
            "long" => {
                store.insert(key, PrefValue::Long(attr(&attrs, "value")?.parse().ok()?));
            }
            "float" => {
                let raw = attr(&attrs, "value")?;
                let value = match raw {
                    "Infinity" => f32::INFINITY,
                    "-Infinity" => f32::NEG_INFINITY,
                    "NaN" => f32::NAN,
                    _ => raw.parse::<f32>().ok()?,
                };
                store.insert(key, PrefValue::Float(value.to_bits()));
            }
            "boolean" => {
                store.insert(
                    key,
                    PrefValue::Bool(matches!(attr(&attrs, "value")?, "true" | "1")),
                );
            }
            "set" => {
                if self_closing {
                    store.insert(key, PrefValue::StringSet(BTreeSet::new()));
                } else {
                    let end = rest.find("</set>")?;
                    let body = &rest[..end];
                    let mut values = BTreeSet::new();
                    let mut set_rest = body;
                    while let Some(start) = set_rest.find('<') {
                        set_rest = &set_rest[start + 1..];
                        if let Some(after_bang) = set_rest.strip_prefix("!--") {
                            let close = after_bang.find("-->")?;
                            set_rest = &after_bang[close + 3..];
                            continue;
                        }
                        if set_rest.starts_with('?') || set_rest.starts_with('!') {
                            let close = set_rest.find('>')?;
                            set_rest = &set_rest[close + 1..];
                            continue;
                        }
                        if set_rest.starts_with('/') {
                            let close = set_rest.find('>')?;
                            set_rest = &set_rest[close + 1..];
                            continue;
                        }
                        let close = set_rest.find('>')?;
                        let inner_tag = &set_rest[..close];
                        set_rest = &set_rest[close + 1..];
                        match tag_name(inner_tag) {
                            "string" => {
                                if inner_tag.trim_end().ends_with('/') {
                                    values.insert(Some(String::new()));
                                } else {
                                    let string_end = set_rest.find("</string>")?;
                                    values.insert(Some(decode_entities(&set_rest[..string_end])?));
                                    set_rest = &set_rest[string_end + "</string>".len()..];
                                }
                            }
                            "null" => {
                                values.insert(None);
                            }
                            _ => {}
                        }
                    }
                    store.insert(key, PrefValue::StringSet(values));
                    rest = &rest[end + "</set>".len()..];
                }
            }
            "null" => {
                // `XmlUtils` can represent a null map value. A `putString`
                // null is an editor removal on Android, so there is no live
                // SharedPreferences value to expose for this entry.
            }
            _ => {
                if !self_closing {
                    let close_tag = format!("</{name_of_tag}>");
                    if let Some(end) = rest.find(&close_tag) {
                        rest = &rest[end + close_tag.len()..];
                    }
                }
            }
        }
    }
    Some(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_roundtrip_preserves_types_entities_and_sets() {
        let mut store = PrefStore::new();
        store.insert(
            "user<&\"".to_string(),
            PrefValue::String("Ana & <João>\n".to_string()),
        );
        store.insert("count".to_string(), PrefValue::Int(-7));
        store.insert("stamp".to_string(), PrefValue::Long(9_000_000_001));
        store.insert("ratio".to_string(), PrefValue::Float((-0.0f32).to_bits()));
        store.insert("enabled".to_string(), PrefValue::Bool(true));
        store.insert(
            "tags".to_string(),
            PrefValue::StringSet(BTreeSet::from([
                None,
                Some("a&b".to_string()),
                Some("z".to_string()),
            ])),
        );
        let bytes = serialize(&store);
        assert_eq!(parse(&bytes), Some(store));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("<map>"));
        assert!(text.contains("<boolean name=\"enabled\" value=\"true\" />"));
    }

    #[test]
    fn malformed_and_oversized_files_recover_as_none() {
        assert_eq!(parse(b"not xml"), None);
        assert_eq!(parse(&vec![b'x'; 16 * 1024 * 1024 + 1]), None);
    }

    #[test]
    fn preference_names_cannot_escape_shared_prefs() {
        assert_eq!(
            path("com.example", "../secret"),
            "/data/user/0/com.example/shared_prefs/..%2Fsecret.xml"
        );
        assert_eq!(default_name(""), "app_preferences");
    }
}
