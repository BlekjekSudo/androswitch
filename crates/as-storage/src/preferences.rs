use super::*;
use alloc::collections::BTreeSet;
use xmlparser::{ElementEnd, Token, Tokenizer};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preference {
    String(String),
    Int(i32),
    Long(i64),
    Float(u32),
    Boolean(bool),
    StringSet(BTreeSet<String>),
}
pub type Preferences = BTreeMap<String, Preference>;
fn malformed(cause: impl Into<String>) -> PlatformError {
    error("PREFERENCES_FORMAT", cause)
}
fn escape(text: &str) -> Result<String, PlatformError> {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && !matches!(c, '\n' | '\r' | '\t') => {
                return Err(malformed("invalid XML character"))
            }
            c => out.push(c),
        }
        if out.len() > 1 << 20 {
            return Err(malformed("escaped value exceeds 1 MiB"));
        }
    }
    Ok(out)
}
fn unescape(mut text: &str) -> Result<String, PlatformError> {
    let mut out = String::new();
    while let Some(pos) = text.find('&') {
        out.push_str(&text[..pos]);
        let rest = &text[pos + 1..];
        let end = rest
            .find(';')
            .ok_or_else(|| malformed("unterminated entity"))?;
        let entity = &rest[..end];
        let c = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let value = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    entity
                        .strip_prefix('#')
                        .and_then(|value| value.parse::<u32>().ok())
                };
                value
                    .and_then(char::from_u32)
                    .ok_or_else(|| malformed("unknown/invalid entity"))?
            }
        };
        if !(matches!(c, '\t' | '\n' | '\r')
            || ('\u{20}'..='\u{d7ff}').contains(&c)
            || ('\u{e000}'..='\u{fffd}').contains(&c)
            || ('\u{10000}'..='\u{10ffff}').contains(&c))
        {
            return Err(malformed("invalid XML entity character"));
        }
        out.push(c);
        text = &rest[end + 1..];
    }
    out.push_str(text);
    Ok(out)
}
pub fn encode_preferences(values: &Preferences) -> Result<String, PlatformError> {
    if values.len() > 8192 {
        return Err(malformed("preference count exceeds 8192"));
    }
    let mut xml = String::from("<?xml version='1.0' encoding='utf-8' standalone='yes' ?>\n<map>\n");
    for (key, value) in values {
        let name = escape(key)?;
        match value {
            Preference::String(value) => xml.push_str(&format!(
                "<string name=\"{name}\">{}</string>\n",
                escape(value)?
            )),
            Preference::StringSet(values) => {
                xml.push_str(&format!("<set name=\"{name}\">"));
                for value in values {
                    xml.push_str(&format!("<string>{}</string>", escape(value)?));
                    if xml.len() > 1 << 20 {
                        return Err(malformed("preferences exceed 1 MiB"));
                    }
                }
                xml.push_str("</set>\n");
            }
            _ => {
                let (kind, text) = match value {
                    Preference::Int(value) => ("int", value.to_string()),
                    Preference::Long(value) => ("long", value.to_string()),
                    Preference::Float(value) => ("float", f32::from_bits(*value).to_string()),
                    Preference::Boolean(value) => ("boolean", value.to_string()),
                    _ => unreachable!(),
                };
                xml.push_str(&format!("<{kind} name=\"{name}\" value=\"{text}\" />\n"));
            }
        }
        if xml.len() > 1 << 20 {
            return Err(malformed("preferences exceed 1 MiB"));
        }
    }
    xml.push_str("</map>\n");
    if xml.len() > 1 << 20 {
        return Err(malformed("preferences exceed 1 MiB"));
    }
    Ok(xml)
}
#[derive(Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
fn finish(node: Node, stack: &mut [Node], root: &mut Option<Node>) -> Result<(), PlatformError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(malformed("multiple roots"));
    }
    Ok(())
}
pub fn decode_preferences(xml: &str) -> Result<Preferences, PlatformError> {
    if xml.len() > 1 << 20 {
        return Err(malformed("preferences exceed 1 MiB"));
    }
    let mut stack = Vec::<Node>::new();
    let mut pending: Option<Node> = None;
    let mut root = None;
    let mut count = 0;
    for token in Tokenizer::from(xml) {
        match token.map_err(|e| malformed(e.to_string()))? {
            Token::ElementStart { prefix, local, .. } => {
                count += 1;
                if !prefix.is_empty() || count > 16384 || stack.len() > 3 || pending.is_some() {
                    return Err(malformed("element/depth budget"));
                }
                pending = Some(Node {
                    name: local.as_str().into(),
                    ..Default::default()
                });
            }
            Token::Attribute {
                prefix,
                local,
                value,
                ..
            } => {
                let node = pending
                    .as_mut()
                    .ok_or_else(|| malformed("attribute outside start"))?;
                if !prefix.is_empty()
                    || node
                        .attrs
                        .insert(local.as_str().into(), unescape(value.as_str())?)
                        .is_some()
                {
                    return Err(malformed("namespace/duplicate attribute"));
                }
            }
            Token::ElementEnd { end, .. } => match end {
                ElementEnd::Open => {
                    stack.push(pending.take().ok_or_else(|| malformed("missing start"))?)
                }
                ElementEnd::Empty => finish(
                    pending.take().ok_or_else(|| malformed("missing start"))?,
                    &mut stack,
                    &mut root,
                )?,
                ElementEnd::Close(prefix, local) => {
                    let node = stack.pop().ok_or_else(|| malformed("unpaired close"))?;
                    if !prefix.is_empty() || node.name != local.as_str() || pending.is_some() {
                        return Err(malformed("mismatched close"));
                    }
                    finish(node, &mut stack, &mut root)?;
                }
            },
            Token::Text { text } => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&unescape(text.as_str())?);
                } else if !text.as_str().trim().is_empty() {
                    return Err(malformed("text outside map"));
                }
            }
            Token::Cdata { text, .. } => stack
                .last_mut()
                .ok_or_else(|| malformed("CDATA outside element"))?
                .text
                .push_str(text.as_str()),
            Token::Declaration { .. } | Token::Comment { .. } => {}
            _ => {
                return Err(malformed(
                    "DTD/entities/processing instructions unsupported",
                ))
            }
        }
    }
    if !stack.is_empty() || pending.is_some() {
        return Err(malformed("incomplete XML"));
    }
    let root = root.ok_or_else(|| malformed("missing map"))?;
    if root.name != "map" || !root.attrs.is_empty() || !root.text.trim().is_empty() {
        return Err(malformed("expected map"));
    }
    let mut values = Preferences::new();
    for mut node in root.children {
        let name = node
            .attrs
            .remove("name")
            .ok_or_else(|| malformed("missing preference name"))?;
        let value = match node.name.as_str() {
            "string" if node.children.is_empty() && node.attrs.is_empty() => {
                Preference::String(node.text)
            }
            "set" if node.attrs.is_empty() && node.text.trim().is_empty() => {
                let mut set = BTreeSet::new();
                for child in node.children {
                    if child.name != "string"
                        || !child.attrs.is_empty()
                        || !child.children.is_empty()
                    {
                        return Err(malformed("invalid string set"));
                    }
                    set.insert(child.text);
                }
                Preference::StringSet(set)
            }
            kind if node.children.is_empty()
                && node.text.trim().is_empty()
                && node.attrs.len() == 1 =>
            {
                let text = node
                    .attrs
                    .remove("value")
                    .ok_or_else(|| malformed("missing value"))?;
                match kind {
                    "int" => Preference::Int(text.parse().map_err(|_| malformed("int value"))?),
                    "long" => Preference::Long(text.parse().map_err(|_| malformed("long value"))?),
                    "float" => Preference::Float(
                        text.parse::<f32>()
                            .map_err(|_| malformed("float value"))?
                            .to_bits(),
                    ),
                    "boolean" => {
                        Preference::Boolean(text.parse().map_err(|_| malformed("boolean value"))?)
                    }
                    _ => return Err(malformed("unknown preference type")),
                }
            }
            _ => return Err(malformed("invalid preference entry")),
        };
        if values.insert(name, value).is_some() || values.len() > 8192 {
            return Err(malformed("duplicate/count budget"));
        }
    }
    Ok(values)
}
