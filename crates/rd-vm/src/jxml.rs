//! SAX source objects and a small, fail-closed XML reader for the platform
//! `org.xmlpull.v1.sax2.Driver` path used by headless applications.
//!
//! The reader emits the standard SAX callbacks into the DEX ContentHandler.
//! It accepts well-formed UTF-8/UTF-16 XML, but deliberately rejects DTDs and
//! external entities: the headless VM never fetches network or host-file XML.

use crate::engine::Engine;
use crate::err::{put_field_result, Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const INPUT_SOURCE: &str = "Lorg/xml/sax/InputSource;";
const DEFAULT_HANDLER: &str = "Lorg/xml/sax/helpers/DefaultHandler;";
const DRIVER: &str = "Lorg/xmlpull/v1/sax2/Driver;";
const ATTRIBUTES: &str = "Lorg/xml/sax/Attributes;";
const ATTRIBUTES_IMPL: &str = "Lorg/xml/sax/helpers/AttributesImpl;";
const LOCATOR: &str = "Lorg/xml/sax/Locator;";
const LOCATOR_IMPL: &str = "Lorg/xml/sax/helpers/LocatorImpl;";

const BYTE_STREAM: &str = "rd_input_source_byte_stream";
const CHAR_STREAM: &str = "rd_input_source_char_stream";
const PUBLIC_ID: &str = "rd_input_source_public_id";
const SYSTEM_ID: &str = "rd_input_source_system_id";
const ENCODING: &str = "rd_input_source_encoding";
const CONTENT_HANDLER_FIELD: &str = "rd_sax_content_handler";
const ERROR_HANDLER_FIELD: &str = "rd_sax_error_handler";
const LOCATOR_FIELD: &str = "rd_sax_locator";
const NAMESPACES_FIELD: &str = "rd_sax_namespaces";
const NS_PREFIXES_FIELD: &str = "rd_sax_ns_prefixes";
const FEATURE_NAMESPACES: &str = "http://xml.org/sax/features/namespaces";
const FEATURE_NS_PREFIXES: &str = "http://xml.org/sax/features/namespace-prefixes";
const XML_NS_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";
const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_EVENTS: usize = 100_000;
const MAX_XML_DEPTH: usize = 256;

/// `InputSource`, the platform XML-pull SAX adapter, and SAX helper objects.
pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == DEFAULT_HANDLER
        && matches!(
            (name, sig),
            ("<init>", "()V")
                | ("setDocumentLocator", "(Lorg/xml/sax/Locator;)V")
                | ("startDocument", "()V")
                | ("endDocument", "()V")
                | (
                    "startPrefixMapping",
                    "(Ljava/lang/String;Ljava/lang/String;)V"
                )
                | ("endPrefixMapping", "(Ljava/lang/String;)V")
                | (
                    "startElement",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lorg/xml/sax/Attributes;)V"
                )
                | (
                    "endElement",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"
                )
                | ("characters", "([CII)V")
                | ("ignorableWhitespace", "([CII)V")
                | (
                    "processingInstruction",
                    "(Ljava/lang/String;Ljava/lang/String;)V"
                )
                | ("skippedEntity", "(Ljava/lang/String;)V")
        )
    {
        return Ok(Some(Value::Null));
    }
    if recv_class == DRIVER && mref_class == DRIVER {
        return driver_intrinsic(vm, name, sig, recv, args);
    }
    if recv_class == ATTRIBUTES_IMPL && (mref_class == ATTRIBUTES || mref_class == ATTRIBUTES_IMPL)
    {
        return attributes_intrinsic(vm, name, sig, recv, args);
    }
    if recv_class == LOCATOR_IMPL && (mref_class == LOCATOR || mref_class == LOCATOR_IMPL) {
        return locator_intrinsic(vm, name, sig, recv, args);
    }
    if mref_class != INPUT_SOURCE || recv_class != INPUT_SOURCE {
        return Ok(None);
    }
    input_source_intrinsic(vm, name, sig, recv, args)
}

fn input_source_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "()V") => Ok(Some(Value::Null)),
        ("<init>", "(Ljava/io/InputStream;)V") => set_arg(vm, recv, BYTE_STREAM, args),
        ("<init>", "(Ljava/io/Reader;)V") => set_arg(vm, recv, CHAR_STREAM, args),
        ("<init>", "(Ljava/lang/String;)V") => set_arg(vm, recv, SYSTEM_ID, args),
        ("getByteStream", "()Ljava/io/InputStream;") => get_field(vm, recv, BYTE_STREAM),
        ("setByteStream", "(Ljava/io/InputStream;)V") => set_arg(vm, recv, BYTE_STREAM, args),
        ("getCharacterStream", "()Ljava/io/Reader;") => get_field(vm, recv, CHAR_STREAM),
        ("setCharacterStream", "(Ljava/io/Reader;)V") => set_arg(vm, recv, CHAR_STREAM, args),
        ("getPublicId", "()Ljava/lang/String;") => get_field(vm, recv, PUBLIC_ID),
        ("setPublicId", "(Ljava/lang/String;)V") => set_arg(vm, recv, PUBLIC_ID, args),
        ("getSystemId", "()Ljava/lang/String;") => get_field(vm, recv, SYSTEM_ID),
        ("setSystemId", "(Ljava/lang/String;)V") => set_arg(vm, recv, SYSTEM_ID, args),
        ("getEncoding", "()Ljava/lang/String;") => get_field(vm, recv, ENCODING),
        ("setEncoding", "(Ljava/lang/String;)V") => set_arg(vm, recv, ENCODING, args),
        _ => Ok(None),
    }
}

fn driver_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "()V") => {
            set_field(vm, recv, NAMESPACES_FIELD, Value::Int(1))?;
            set_field(vm, recv, NS_PREFIXES_FIELD, Value::Int(0))?;
            Ok(Some(Value::Null))
        }
        ("setFeature", "(Ljava/lang/String;Z)V") => {
            let feat = arg_string(vm, args, 0)?;
            let enabled = int_arg(args, 1) != 0;
            match feat.as_str() {
                FEATURE_NAMESPACES => {
                    set_field(vm, recv, NAMESPACES_FIELD, Value::Int(i32::from(enabled)))?;
                }
                FEATURE_NS_PREFIXES => {
                    set_field(vm, recv, NS_PREFIXES_FIELD, Value::Int(i32::from(enabled)))?;
                }
                _ => {}
            }
            Ok(Some(Value::Null))
        }
        ("getFeature", "(Ljava/lang/String;)Z") => {
            let feat = arg_string(vm, args, 0)?;
            let val = match feat.as_str() {
                FEATURE_NAMESPACES => bool_field(vm, recv, NAMESPACES_FIELD, true),
                FEATURE_NS_PREFIXES => bool_field(vm, recv, NS_PREFIXES_FIELD, false),
                _ => false,
            };
            Ok(Some(Value::Int(i32::from(val))))
        }
        ("setContentHandler", "(Lorg/xml/sax/ContentHandler;)V") => {
            set_arg(vm, recv, CONTENT_HANDLER_FIELD, args)
        }
        ("setErrorHandler", "(Lorg/xml/sax/ErrorHandler;)V") => {
            set_arg(vm, recv, ERROR_HANDLER_FIELD, args)
        }
        ("parse", "(Lorg/xml/sax/InputSource;)V") => {
            let source = match args.first() {
                Some(Value::Obj(r)) => *r,
                _ => {
                    return Err(exception(
                        "Ljava/lang/NullPointerException;",
                        "XMLReader.parse input source == null",
                    ));
                }
            };
            let handler = match vm.heap.get_field(recv, CONTENT_HANDLER_FIELD) {
                Ok(Value::Obj(r)) => r,
                _ => {
                    return Err(exception(
                        "Lorg/xml/sax/SAXException;",
                        "XMLReader has no ContentHandler",
                    ));
                }
            };
            let xml = input_source_text(vm, source)?;
            parse_to_handler(vm, recv, handler, source, &xml)?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

fn bool_field(vm: &Engine, recv: ObjRef, field: &str, default: bool) -> bool {
    match vm.heap.get_field(recv, field) {
        Ok(Value::Int(v)) => v != 0,
        _ => default,
    }
}

fn input_source_text(vm: &mut Engine, source: ObjRef) -> Result<String, VmExit> {
    let byte_stream = object_field(vm, source, BYTE_STREAM);
    if let Some(stream) = byte_stream {
        let mut bytes = Vec::new();
        loop {
            if bytes.len() >= MAX_XML_BYTES {
                return Err(exception(
                    "Ljava/io/IOException;",
                    "XML input exceeds the configured byte limit",
                ));
            }
            let value = vm.invoke_instance(stream, "read", "()I", &[])?;
            let Value::Int(byte) = value else {
                return Err(exception(
                    "Ljava/io/IOException;",
                    "InputStream.read() returned a non-integer",
                ));
            };
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
        return decode_xml_bytes(&bytes);
    }

    if let Some(reader) = object_field(vm, source, CHAR_STREAM) {
        let mut units = Vec::new();
        loop {
            if units.len() >= MAX_XML_BYTES {
                return Err(exception(
                    "Ljava/io/IOException;",
                    "XML character input exceeds the configured limit",
                ));
            }
            let value = vm.invoke_instance(reader, "read", "()I", &[])?;
            let Value::Int(unit) = value else {
                return Err(exception(
                    "Ljava/io/IOException;",
                    "Reader.read() returned a non-integer",
                ));
            };
            if unit < 0 {
                break;
            }
            units.push(unit as u16);
        }
        return Ok(String::from_utf16_lossy(&units));
    }

    let system_id = match vm.heap.get_field(source, SYSTEM_ID) {
        Ok(Value::Obj(r)) => vm.heap.as_str(r).unwrap_or("").to_string(),
        _ => String::new(),
    };
    let Some(path) = system_id
        .strip_prefix("apk:/")
        .map(|p| p.trim_start_matches('/'))
    else {
        return Err(exception(
            "Ljava/io/IOException;",
            "InputSource has no stream and its system ID is not an APK URL",
        ));
    };
    let Some(apk) = &vm.fw.resources else {
        return Err(exception(
            "Ljava/io/FileNotFoundException;",
            "no APK is attached to the XML InputSource",
        ));
    };
    let Some(entry) = apk.zip.find(path) else {
        return Err(exception(
            "Ljava/io/FileNotFoundException;",
            &format!("XML resource not found: {path}"),
        ));
    };
    let bytes = apk.zip.read_entry(entry).map_err(|e| {
        exception(
            "Ljava/io/IOException;",
            &format!("reading XML resource {path}: {}", e.cause),
        )
    })?;
    decode_xml_bytes(&bytes)
}

fn decode_xml_bytes(bytes: &[u8]) -> Result<String, VmExit> {
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16_xml(body, true);
    }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16_xml(body, false);
    }
    if bytes.starts_with(&[b'<', 0x00]) {
        return decode_utf16_xml(bytes, true);
    }
    if bytes.starts_with(&[0x00, b'<']) {
        return decode_utf16_xml(bytes, false);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(bytes.to_vec()).map_err(|e| {
        exception(
            "Lorg/xml/sax/SAXException;",
            &format!("invalid UTF-8 XML: {e}"),
        )
    })
}

fn decode_utf16_xml(body: &[u8], little_endian: bool) -> Result<String, VmExit> {
    if body.len() % 2 != 0 {
        return Err(exception(
            "Lorg/xml/sax/SAXException;",
            "truncated UTF-16 XML: odd byte count",
        ));
    }
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|e| {
        exception(
            "Lorg/xml/sax/SAXException;",
            &format!("invalid UTF-16 XML: {e}"),
        )
    })
}

#[derive(Clone, Debug)]
struct SaxAttr {
    uri: String,
    local: String,
    qname: String,
    attr_type: String,
    value: String,
}

fn lookup_prefix_uri<'a>(ns_stack: &'a [Vec<(String, String)>], prefix: &str) -> Option<&'a str> {
    match prefix {
        "xml" => return Some(XML_NS_URI),
        "xmlns" => return Some(XMLNS_URI),
        _ => {}
    }
    for scope in ns_stack.iter().rev() {
        for (p, uri) in scope.iter().rev() {
            if p == prefix {
                return if prefix.is_empty() {
                    Some(uri.as_str())
                } else if uri.is_empty() {
                    None
                } else {
                    Some(uri.as_str())
                };
            }
        }
    }
    if prefix.is_empty() {
        Some("")
    } else {
        None
    }
}

fn resolve_element_qname(
    ns_stack: &[Vec<(String, String)>],
    qname: &str,
    namespaces: bool,
) -> Result<(String, String), VmExit> {
    if !namespaces {
        return Ok((String::new(), String::new()));
    }
    if let Some((prefix, local)) = qname.split_once(':') {
        if prefix.is_empty() || local.is_empty() || local.contains(':') {
            return Err(exception(
                "Lorg/xml/sax/SAXException;",
                &format!("malformed QName: {qname}"),
            ));
        }
        let Some(uri) = lookup_prefix_uri(ns_stack, prefix) else {
            return Err(exception(
                "Lorg/xml/sax/SAXException;",
                &format!("unbound namespace prefix '{prefix}' on <{qname}>"),
            ));
        };
        Ok((uri.to_string(), local.to_string()))
    } else {
        let uri = lookup_prefix_uri(ns_stack, "").unwrap_or("");
        Ok((uri.to_string(), qname.to_string()))
    }
}

fn resolve_element_attributes(
    ns_stack: &mut Vec<Vec<(String, String)>>,
    raw_attrs: &[(String, String)],
    namespaces: bool,
    ns_prefixes: bool,
) -> Result<Vec<SaxAttr>, VmExit> {
    if !namespaces {
        ns_stack.push(Vec::new());
        return Ok(raw_attrs
            .iter()
            .map(|(name, value)| SaxAttr {
                uri: String::new(),
                local: local_name(name),
                qname: name.clone(),
                attr_type: "CDATA".to_string(),
                value: value.clone(),
            })
            .collect());
    }

    let mut frame = Vec::new();
    for (name, value) in raw_attrs {
        if name == "xmlns" {
            frame.push((String::new(), value.clone()));
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            if prefix.is_empty() || prefix.contains(':') || prefix == "xml" || prefix == "xmlns" {
                return Err(exception(
                    "Lorg/xml/sax/SAXException;",
                    &format!("invalid namespace declaration '{name}'"),
                ));
            }
            if value.is_empty() {
                return Err(exception(
                    "Lorg/xml/sax/SAXException;",
                    &format!("empty namespace URI for prefix '{prefix}'"),
                ));
            }
            frame.push((prefix.to_string(), value.clone()));
        }
    }
    ns_stack.push(frame);

    let mut resolved = Vec::with_capacity(raw_attrs.len());
    let mut expanded_names = std::collections::HashSet::new();
    for (name, value) in raw_attrs {
        if name == "xmlns" || name.starts_with("xmlns:") {
            if ns_prefixes {
                let local = name.strip_prefix("xmlns:").unwrap_or("xmlns").to_string();
                resolved.push(SaxAttr {
                    uri: XMLNS_URI.to_string(),
                    local,
                    qname: name.clone(),
                    attr_type: "CDATA".to_string(),
                    value: value.clone(),
                });
            }
            continue;
        }
        let (uri, local) = if let Some((prefix, local)) = name.split_once(':') {
            if prefix.is_empty() || local.is_empty() || local.contains(':') {
                return Err(exception(
                    "Lorg/xml/sax/SAXException;",
                    &format!("malformed attribute QName '{name}'"),
                ));
            }
            let Some(uri) = lookup_prefix_uri(ns_stack, prefix) else {
                return Err(exception(
                    "Lorg/xml/sax/SAXException;",
                    &format!("unbound namespace prefix '{prefix}' on attribute '{name}'"),
                ));
            };
            (uri.to_string(), local.to_string())
        } else {
            // W3C Namespaces in XML §6.2: default namespace never applies to unprefixed attributes.
            (String::new(), name.clone())
        };
        if !uri.is_empty() && !expanded_names.insert((uri.clone(), local.clone())) {
            return Err(exception(
                "Lorg/xml/sax/SAXException;",
                &format!("duplicate namespace-qualified attribute {{{uri}}}{local}"),
            ));
        }
        resolved.push(SaxAttr {
            uri,
            local,
            qname: name.clone(),
            attr_type: "CDATA".to_string(),
            value: value.clone(),
        });
    }
    Ok(resolved)
}

fn parse_to_handler(
    vm: &mut Engine,
    driver: ObjRef,
    handler: ObjRef,
    source: ObjRef,
    xml: &str,
) -> Result<(), VmExit> {
    let events = parse_xml(xml).map_err(|e| exception("Lorg/xml/sax/SAXException;", &e))?;
    let namespaces = bool_field(vm, driver, NAMESPACES_FIELD, true);
    let ns_prefixes = bool_field(vm, driver, NS_PREFIXES_FIELD, false);
    let locator = vm
        .heap
        .alloc_instance(LOCATOR_IMPL.to_string(), Vec::new())?;
    set_field(vm, locator, "rd_sax_line", Value::Int(1))?;
    set_field(vm, locator, "rd_sax_column", Value::Int(1))?;
    let public_id = vm.heap.get_field(source, PUBLIC_ID).unwrap_or(Value::Null);
    let system_id = vm.heap.get_field(source, SYSTEM_ID).unwrap_or(Value::Null);
    set_field(vm, locator, "rd_sax_public_id", public_id)?;
    set_field(vm, locator, "rd_sax_system_id", system_id)?;
    set_field(vm, driver, LOCATOR_FIELD, Value::Obj(locator))?;

    let handler_class = vm.heap.class_of(handler)?.to_string();
    let has_start_prefix = vm
        .cp
        .resolve_method(
            &handler_class,
            "startPrefixMapping",
            "(Ljava/lang/String;Ljava/lang/String;)V",
        )
        .is_some();
    let has_end_prefix = vm
        .cp
        .resolve_method(&handler_class, "endPrefixMapping", "(Ljava/lang/String;)V")
        .is_some();

    vm.invoke_instance(
        handler,
        "setDocumentLocator",
        "(Lorg/xml/sax/Locator;)V",
        &[Value::Obj(locator)],
    )?;
    vm.invoke_instance(handler, "startDocument", "()V", &[])?;

    let mut cursor_byte = 0usize;
    let mut line = 1i32;
    let mut column = 1i32;
    let mut ns_stack: Vec<Vec<(String, String)>> = Vec::new();

    for event in events {
        advance_line_column(xml, &mut cursor_byte, &mut line, &mut column, event.offset);
        set_field(vm, locator, "rd_sax_line", Value::Int(line))?;
        set_field(vm, locator, "rd_sax_column", Value::Int(column))?;
        match event.kind {
            XmlEventKind::Start { name, attributes } => {
                let resolved_attrs = resolve_element_attributes(
                    &mut ns_stack,
                    &attributes,
                    namespaces,
                    ns_prefixes,
                )?;
                if namespaces && has_start_prefix {
                    if let Some(frame) = ns_stack.last().cloned() {
                        for (pfx, uri) in frame {
                            let pfx_v = alloc_string(vm, pfx)?;
                            let uri_v = alloc_string(vm, uri)?;
                            vm.invoke_instance(
                                handler,
                                "startPrefixMapping",
                                "(Ljava/lang/String;Ljava/lang/String;)V",
                                &[Value::Obj(pfx_v), Value::Obj(uri_v)],
                            )?;
                        }
                    }
                }
                let (elem_uri, elem_local) = resolve_element_qname(&ns_stack, &name, namespaces)?;
                let uri = alloc_string(vm, elem_uri)?;
                let local = alloc_string(vm, elem_local)?;
                let qname = alloc_string(vm, name)?;
                let attributes = new_sax_attributes(vm, &resolved_attrs)?;
                vm.invoke_instance(
                    handler,
                    "startElement",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lorg/xml/sax/Attributes;)V",
                    &[
                        Value::Obj(uri),
                        Value::Obj(local),
                        Value::Obj(qname),
                        Value::Obj(attributes),
                    ],
                )?;
            }
            XmlEventKind::End { name } => {
                let (elem_uri, elem_local) = resolve_element_qname(&ns_stack, &name, namespaces)?;
                let popped = ns_stack.pop().unwrap_or_default();
                let uri = alloc_string(vm, elem_uri)?;
                let local = alloc_string(vm, elem_local)?;
                let qname = alloc_string(vm, name)?;
                vm.invoke_instance(
                    handler,
                    "endElement",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
                    &[Value::Obj(uri), Value::Obj(local), Value::Obj(qname)],
                )?;
                if namespaces && has_end_prefix {
                    for (pfx, _) in popped.into_iter().rev() {
                        let pfx_v = alloc_string(vm, pfx)?;
                        vm.invoke_instance(
                            handler,
                            "endPrefixMapping",
                            "(Ljava/lang/String;)V",
                            &[Value::Obj(pfx_v)],
                        )?;
                    }
                }
            }
            XmlEventKind::Text(text) if !text.is_empty() => {
                let chars: Vec<u16> = text.encode_utf16().collect();
                let array = vm
                    .heap
                    .alloc_array_zeroed(ElemKind::Int, chars.len(), "[C")?;
                let elems = vm
                    .heap
                    .array_elems_mut(array)
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
                for (dst, unit) in elems.iter_mut().zip(chars.iter()) {
                    *dst = Value::Int(*unit as i32);
                }
                vm.invoke_instance(
                    handler,
                    "characters",
                    "([CII)V",
                    &[
                        Value::Obj(array),
                        Value::Int(0),
                        Value::Int(chars.len() as i32),
                    ],
                )?;
            }
            XmlEventKind::Text(_) => {}
        }
    }
    // `DefaultHandler.endDocument` is a no-op. Invoke an application override
    // when present; otherwise the Java default method has no observable effect.
    let handler_class = vm.heap.class_of(handler)?.to_string();
    if vm
        .cp
        .resolve_method(&handler_class, "endDocument", "()V")
        .is_some()
    {
        vm.invoke_instance(handler, "endDocument", "()V", &[])?;
    }
    Ok(())
}

fn attributes_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        match sig {
            "()V" => {
                set_field(vm, recv, "rd_sax_attr_count", Value::Int(0))?;
                return Ok(Some(Value::Null));
            }
            "(Lorg/xml/sax/Attributes;)V" => {
                let source = match args.first() {
                    Some(Value::Obj(r)) => Some(*r),
                    _ => None,
                };
                let copied = if let Some(source) = source {
                    copy_attributes(vm, source)?
                } else {
                    Vec::new()
                };
                store_sax_attributes(vm, recv, &copied)?;
                return Ok(Some(Value::Null));
            }
            _ => return Ok(None),
        }
    }

    let length = attr_count(vm, recv);
    let value = match (name, sig) {
        ("getLength", "()I") => Value::Int(length as i32),
        ("getURI", "(I)Ljava/lang/String;") => attr_at(vm, recv, int_arg(args, 0), "uri"),
        ("getLocalName", "(I)Ljava/lang/String;") => attr_at(vm, recv, int_arg(args, 0), "local"),
        ("getQName", "(I)Ljava/lang/String;") => attr_at(vm, recv, int_arg(args, 0), "qname"),
        ("getType", "(I)Ljava/lang/String;") => attr_at(vm, recv, int_arg(args, 0), "type"),
        ("getValue", "(I)Ljava/lang/String;") => attr_at(vm, recv, int_arg(args, 0), "value"),
        ("getIndex", "(Ljava/lang/String;)I") => {
            Value::Int(find_attr(vm, recv, None, arg_string(vm, args, 0)?))
        }
        ("getIndex", "(Ljava/lang/String;Ljava/lang/String;)I") => Value::Int(find_attr(
            vm,
            recv,
            Some(arg_string(vm, args, 0)?),
            arg_string(vm, args, 1)?,
        )),
        ("getType", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let index = find_attr(vm, recv, None, arg_string(vm, args, 0)?);
            attr_at(vm, recv, index, "type")
        }
        ("getType", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;") => {
            let index = find_attr(
                vm,
                recv,
                Some(arg_string(vm, args, 0)?),
                arg_string(vm, args, 1)?,
            );
            attr_at(vm, recv, index, "type")
        }
        ("getValue", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let index = find_attr(vm, recv, None, arg_string(vm, args, 0)?);
            attr_at(vm, recv, index, "value")
        }
        ("getValue", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;") => {
            let index = find_attr(
                vm,
                recv,
                Some(arg_string(vm, args, 0)?),
                arg_string(vm, args, 1)?,
            );
            attr_at(vm, recv, index, "value")
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn copy_attributes(vm: &mut Engine, source: ObjRef) -> Result<Vec<SaxAttr>, VmExit> {
    let length = match vm.invoke_instance(source, "getLength", "()I", &[])? {
        Value::Int(n) => n.max(0) as usize,
        _ => 0,
    };
    if length > 4096 {
        return Err(exception(
            "Lorg/xml/sax/SAXException;",
            "XML element has too many attributes",
        ));
    }
    let mut out = Vec::with_capacity(length);
    for index in 0..length {
        let idx = [Value::Int(index as i32)];
        let uri = vm.invoke_instance(source, "getURI", "(I)Ljava/lang/String;", &idx)?;
        let local = vm.invoke_instance(source, "getLocalName", "(I)Ljava/lang/String;", &idx)?;
        let qname = vm.invoke_instance(source, "getQName", "(I)Ljava/lang/String;", &idx)?;
        let attr_type = vm.invoke_instance(source, "getType", "(I)Ljava/lang/String;", &idx)?;
        let value = vm.invoke_instance(source, "getValue", "(I)Ljava/lang/String;", &idx)?;
        let qname_str = value_string(vm, qname);
        let local_str = {
            let s = value_string(vm, local);
            if s.is_empty() {
                local_name(&qname_str)
            } else {
                s
            }
        };
        let type_str = {
            let s = value_string(vm, attr_type);
            if s.is_empty() {
                "CDATA".to_string()
            } else {
                s
            }
        };
        out.push(SaxAttr {
            uri: value_string(vm, uri),
            local: local_str,
            qname: qname_str,
            attr_type: type_str,
            value: value_string(vm, value),
        });
    }
    Ok(out)
}

#[cfg(test)]
fn new_attributes(vm: &mut Engine, pairs: &[(String, String)]) -> Result<ObjRef, VmExit> {
    let attrs: Vec<SaxAttr> = pairs
        .iter()
        .map(|(name, value)| SaxAttr {
            uri: String::new(),
            local: local_name(name),
            qname: name.clone(),
            attr_type: "CDATA".to_string(),
            value: value.clone(),
        })
        .collect();
    new_sax_attributes(vm, &attrs)
}

fn new_sax_attributes(vm: &mut Engine, attrs: &[SaxAttr]) -> Result<ObjRef, VmExit> {
    let object = vm
        .heap
        .alloc_instance(ATTRIBUTES_IMPL.to_string(), Vec::new())?;
    store_sax_attributes(vm, object, attrs)?;
    Ok(object)
}

fn store_sax_attributes(vm: &mut Engine, object: ObjRef, attrs: &[SaxAttr]) -> Result<(), VmExit> {
    if attrs.len() > 4096 {
        return Err(exception(
            "Lorg/xml/sax/SAXException;",
            "XML element has too many attributes",
        ));
    }
    set_field(
        vm,
        object,
        "rd_sax_attr_count",
        Value::Int(attrs.len() as i32),
    )?;
    for (index, attr) in attrs.iter().enumerate() {
        let uri = alloc_string(vm, attr.uri.clone())?;
        let local = alloc_string(vm, attr.local.clone())?;
        let qname = alloc_string(vm, attr.qname.clone())?;
        let attr_type = alloc_string(vm, attr.attr_type.clone())?;
        let value = alloc_string(vm, attr.value.clone())?;
        set_field(vm, object, &attr_field(index, "uri"), Value::Obj(uri))?;
        set_field(vm, object, &attr_field(index, "local"), Value::Obj(local))?;
        set_field(vm, object, &attr_field(index, "qname"), Value::Obj(qname))?;
        set_field(
            vm,
            object,
            &attr_field(index, "type"),
            Value::Obj(attr_type),
        )?;
        set_field(vm, object, &attr_field(index, "value"), Value::Obj(value))?;
    }
    Ok(())
}

fn attr_count(vm: &Engine, object: ObjRef) -> usize {
    match vm.heap.get_field(object, "rd_sax_attr_count") {
        Ok(Value::Int(n)) => n.max(0) as usize,
        _ => 0,
    }
}

fn attr_at(vm: &Engine, object: ObjRef, index: i32, field: &str) -> Value {
    if index < 0 || index as usize >= attr_count(vm, object) {
        return Value::Null;
    }
    vm.heap
        .get_field(object, &attr_field(index as usize, field))
        .unwrap_or(Value::Null)
}

fn find_attr(vm: &Engine, object: ObjRef, uri: Option<String>, key: String) -> i32 {
    for index in 0..attr_count(vm, object) {
        if let Some(ref want_uri) = uri {
            let local = vm.heap.get_field(object, &attr_field(index, "local"));
            let attr_uri = vm.heap.get_field(object, &attr_field(index, "uri"));
            let local_matches = local
                .ok()
                .and_then(|v| value_string_ref(vm, v))
                .is_some_and(|s| s == key);
            let uri_matches = attr_uri
                .ok()
                .and_then(|v| value_string_ref(vm, v))
                .is_some_and(|s| s == want_uri.as_str());
            if uri_matches && local_matches {
                return index as i32;
            }
        } else {
            let qname = vm.heap.get_field(object, &attr_field(index, "qname"));
            let qname_matches = qname
                .ok()
                .and_then(|v| value_string_ref(vm, v))
                .is_some_and(|s| s == key);
            if qname_matches {
                return index as i32;
            }
        }
    }
    -1
}

fn locator_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        match sig {
            "()V" => {
                set_field(vm, recv, "rd_sax_line", Value::Int(-1))?;
                set_field(vm, recv, "rd_sax_column", Value::Int(-1))?;
                return Ok(Some(Value::Null));
            }
            "(Lorg/xml/sax/Locator;)V" => {
                if let Some(Value::Obj(source)) = args.first() {
                    for (method, signature, field) in [
                        ("getPublicId", "()Ljava/lang/String;", "rd_sax_public_id"),
                        ("getSystemId", "()Ljava/lang/String;", "rd_sax_system_id"),
                        ("getLineNumber", "()I", "rd_sax_line"),
                        ("getColumnNumber", "()I", "rd_sax_column"),
                    ] {
                        let value = vm.invoke_instance(*source, method, signature, &[])?;
                        set_field(vm, recv, field, value)?;
                    }
                }
                return Ok(Some(Value::Null));
            }
            _ => return Ok(None),
        }
    }
    match (name, sig) {
        ("getPublicId", "()Ljava/lang/String;") => get_field(vm, recv, "rd_sax_public_id"),
        ("getSystemId", "()Ljava/lang/String;") => get_field(vm, recv, "rd_sax_system_id"),
        ("getLineNumber", "()I") => get_field(vm, recv, "rd_sax_line"),
        ("getColumnNumber", "()I") => get_field(vm, recv, "rd_sax_column"),
        ("setPublicId", "(Ljava/lang/String;)V") => set_arg(vm, recv, "rd_sax_public_id", args),
        ("setSystemId", "(Ljava/lang/String;)V") => set_arg(vm, recv, "rd_sax_system_id", args),
        ("setLineNumber", "(I)V") => set_arg(vm, recv, "rd_sax_line", args),
        ("setColumnNumber", "(I)V") => set_arg(vm, recv, "rd_sax_column", args),
        _ => Ok(None),
    }
}

fn get_field(vm: &Engine, recv: ObjRef, field: &str) -> Result<Option<Value>, VmExit> {
    Ok(Some(vm.heap.get_field(recv, field).unwrap_or(Value::Null)))
}

fn set_arg(
    vm: &mut Engine,
    recv: ObjRef,
    field: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = args.first().cloned().unwrap_or(Value::Null);
    set_field(vm, recv, field, value)?;
    Ok(Some(Value::Null))
}

fn set_field(vm: &mut Engine, recv: ObjRef, field: &str, value: Value) -> Result<(), VmExit> {
    put_field_result(vm.heap.put_field(recv, field, value))?;
    Ok(())
}

fn object_field(vm: &Engine, object: ObjRef, field: &str) -> Option<ObjRef> {
    match vm.heap.get_field(object, field).ok()? {
        Value::Obj(r) => Some(r),
        _ => None,
    }
}

fn alloc_string(vm: &mut Engine, value: impl Into<String>) -> Result<ObjRef, VmExit> {
    crate::intrinsics::alloc_string(vm, value.into())
}

fn attr_field(index: usize, field: &str) -> String {
    format!("rd_sax_attr_{index}_{field}")
}

fn int_arg(args: &[Value], index: usize) -> i32 {
    args.get(index)
        .and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(-1)
}

fn arg_string(vm: &Engine, args: &[Value], index: usize) -> Result<String, VmExit> {
    match args.get(index) {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).map(str::to_string).map_err(|e| {
            crate::err::vm_error("INVALID_ARG", format!("SAX attribute key: {e}")).into()
        }),
        _ => Ok(String::new()),
    }
}

fn value_string(vm: &Engine, value: Value) -> String {
    value_string_ref(vm, value).unwrap_or_default()
}

fn value_string_ref(vm: &Engine, value: Value) -> Option<String> {
    match value {
        Value::Obj(r) => vm.heap.as_str(r).ok().map(str::to_string),
        _ => None,
    }
}

fn local_name(qname: &str) -> String {
    qname.rsplit(':').next().unwrap_or(qname).to_string()
}

fn exception(class: &str, message: &str) -> VmExit {
    VmExit::Exception(Throwable::new(class, message))
}

#[derive(Debug, PartialEq, Eq)]
enum XmlEventKind {
    Start {
        name: String,
        attributes: Vec<(String, String)>,
    },
    End {
        name: String,
    },
    Text(String),
}

#[derive(Debug, PartialEq, Eq)]
struct XmlEvent {
    offset: usize,
    kind: XmlEventKind,
}

fn parse_xml(xml: &str) -> Result<Vec<XmlEvent>, String> {
    let bytes = xml.as_bytes();
    let mut offset = usize::from(bytes.starts_with(&[0xEF, 0xBB, 0xBF])) * 3;
    let mut stack: Vec<String> = Vec::new();
    let mut events = Vec::new();
    let mut root_count = 0usize;
    let mut root_closed = false;

    while offset < bytes.len() {
        if bytes[offset] != b'<' {
            let start = offset;
            while offset < bytes.len() && bytes[offset] != b'<' {
                offset += 1;
            }
            let text = decode_entities(&xml[start..offset])?;
            if stack.is_empty() {
                if !text.trim().is_empty() {
                    return Err("text outside the document element".into());
                }
            } else if !text.is_empty() {
                push_xml_event(
                    &mut events,
                    XmlEvent {
                        offset: start,
                        kind: XmlEventKind::Text(text),
                    },
                )?;
            }
            continue;
        }

        if bytes[offset..].starts_with(b"<!--") {
            offset = find_after(bytes, offset + 4, b"-->")
                .ok_or_else(|| "unterminated XML comment".to_string())?;
            continue;
        }
        if bytes[offset..].starts_with(b"<![CDATA[") {
            if stack.is_empty() {
                return Err("CDATA outside the document element".into());
            }
            let start = offset + 9;
            let end = find_from(bytes, start, b"]]>")
                .ok_or_else(|| "unterminated CDATA section".to_string())?;
            let text = xml[start..end].to_string();
            push_xml_event(
                &mut events,
                XmlEvent {
                    offset,
                    kind: XmlEventKind::Text(text),
                },
            )?;
            offset = end + 3;
            continue;
        }
        if bytes[offset..].starts_with(b"<?") {
            offset = find_after(bytes, offset + 2, b"?>")
                .ok_or_else(|| "unterminated processing instruction".to_string())?;
            continue;
        }
        if bytes[offset..].starts_with(b"<!") {
            return Err("DOCTYPE and XML declarations are disabled".into());
        }

        if bytes[offset..].starts_with(b"</") {
            let event_offset = offset;
            offset += 2;
            let name = read_name(xml, &mut offset)?;
            skip_space(bytes, &mut offset);
            if bytes.get(offset) != Some(&b'>') {
                return Err("malformed end tag".into());
            }
            offset += 1;
            let open = stack
                .pop()
                .ok_or_else(|| format!("unexpected closing tag </{name}>"))?;
            if open != name {
                return Err(format!("closing tag </{name}> does not match <{open}>"));
            }
            if stack.is_empty() {
                root_closed = true;
            }
            push_xml_event(
                &mut events,
                XmlEvent {
                    offset: event_offset,
                    kind: XmlEventKind::End { name },
                },
            )?;
            continue;
        }

        let event_offset = offset;
        offset += 1;
        let name = read_name(xml, &mut offset)?;
        if stack.is_empty() {
            if root_closed || root_count != 0 {
                return Err("multiple document elements".into());
            }
            root_count += 1;
        }
        let mut attributes = Vec::new();
        let mut names = std::collections::HashSet::new();
        let self_closing;
        loop {
            skip_space(bytes, &mut offset);
            match bytes.get(offset) {
                Some(b'>') => {
                    offset += 1;
                    self_closing = false;
                    break;
                }
                Some(b'/') if bytes.get(offset + 1) == Some(&b'>') => {
                    offset += 2;
                    self_closing = true;
                    break;
                }
                Some(_) => {
                    let attr_name = read_name(xml, &mut offset)?;
                    if !names.insert(attr_name.clone()) {
                        return Err(format!("duplicate attribute {attr_name}"));
                    }
                    skip_space(bytes, &mut offset);
                    if bytes.get(offset) != Some(&b'=') {
                        return Err(format!("attribute {attr_name} has no '='"));
                    }
                    offset += 1;
                    skip_space(bytes, &mut offset);
                    let quote = *bytes
                        .get(offset)
                        .ok_or_else(|| "missing attribute value".to_string())?;
                    if quote != b'\'' && quote != b'"' {
                        return Err(format!("attribute {attr_name} is not quoted"));
                    }
                    offset += 1;
                    let start = offset;
                    while offset < bytes.len() && bytes[offset] != quote {
                        if bytes[offset] == b'<' {
                            return Err("'<' is not allowed in an attribute value".into());
                        }
                        offset += 1;
                    }
                    if offset == bytes.len() {
                        return Err(format!("unterminated value for attribute {attr_name}"));
                    }
                    let value = decode_entities(&xml[start..offset])?;
                    offset += 1;
                    attributes.push((attr_name, value));
                }
                None => return Err("unterminated start tag".into()),
            }
        }

        push_xml_event(
            &mut events,
            XmlEvent {
                offset: event_offset,
                kind: XmlEventKind::Start {
                    name: name.clone(),
                    attributes,
                },
            },
        )?;
        if self_closing {
            push_xml_event(
                &mut events,
                XmlEvent {
                    offset: event_offset,
                    kind: XmlEventKind::End { name },
                },
            )?;
            if stack.is_empty() {
                root_closed = true;
            }
        } else {
            stack.push(name);
            if stack.len() > MAX_XML_DEPTH {
                return Err("XML nesting exceeds the configured depth limit".into());
            }
        }
    }

    if let Some(last) = stack.last() {
        return Err(format!("unclosed XML element <{last}>"));
    }
    if root_count != 1 {
        return Err("XML document must have exactly one document element".into());
    }
    Ok(events)
}

fn push_xml_event(events: &mut Vec<XmlEvent>, event: XmlEvent) -> Result<(), String> {
    if events.len() >= MAX_XML_EVENTS {
        return Err("XML event count exceeds the configured limit".into());
    }
    events.push(event);
    Ok(())
}

fn read_name(xml: &str, offset: &mut usize) -> Result<String, String> {
    let bytes = xml.as_bytes();
    let start = *offset;
    while let Some(&byte) = bytes.get(*offset) {
        if byte <= b' ' || matches!(byte, b'/' | b'>' | b'=' | b'?' | b'<' | b'\'' | b'"') {
            break;
        }
        *offset += 1;
    }
    if *offset == start {
        return Err("expected an XML name".into());
    }
    Ok(xml[start..*offset].to_string())
}

fn skip_space(bytes: &[u8], offset: &mut usize) {
    while bytes.get(*offset).is_some_and(|b| *b <= b' ') {
        *offset += 1;
    }
}

fn find_from(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| relative + start)
}

fn find_after(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    find_from(bytes, start, needle).map(|index| index + needle.len())
}

fn decode_entities(input: &str) -> Result<String, String> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let entity_start = index + 1;
        let Some(end) = rest[entity_start..].find(';') else {
            return Err("unterminated XML entity reference".into());
        };
        let entity_end = entity_start + end;
        let entity = &rest[entity_start..entity_end];
        let character = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                let value = u32::from_str_radix(&entity[2..], 16)
                    .map_err(|_| format!("invalid XML character reference &{entity};"))?;
                char::from_u32(value)
                    .ok_or_else(|| format!("invalid XML character reference &{entity};"))?
            }
            _ if entity.starts_with('#') => {
                let value = entity[1..]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid XML character reference &{entity};"))?;
                char::from_u32(value)
                    .ok_or_else(|| format!("invalid XML character reference &{entity};"))?
            }
            _ => return Err(format!("undeclared XML entity &{entity};")),
        };
        out.push(character);
        rest = &rest[entity_end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn advance_line_column(
    xml: &str,
    cursor: &mut usize,
    line: &mut i32,
    column: &mut i32,
    offset: usize,
) {
    let target = offset.min(xml.len());
    if target < *cursor {
        *cursor = 0;
        *line = 1;
        *column = 1;
    }
    for ch in xml[*cursor..target].chars() {
        if ch == '\n' {
            *line = line.saturating_add(1);
            *column = 1;
        } else {
            *column = column.saturating_add(1);
        }
    }
    *cursor = target;
}

#[cfg(test)]
mod tests {
    use super::{
        alloc_string, attributes_intrinsic, locator_intrinsic, new_attributes, parse_xml,
        XmlEventKind, ATTRIBUTES_IMPL, LOCATOR_IMPL,
    };
    use crate::engine::{Engine, VmConfig};
    use crate::value::Value;

    #[test]
    fn attributes_copy_and_lookup_follow_sax_contract() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let attrs = new_attributes(
            &mut vm,
            &[
                ("name".into(), "server".into()),
                ("level".into(), "WARN".into()),
            ],
        )
        .unwrap();
        let key = Value::Obj(alloc_string(&mut vm, "name").unwrap());
        let value = attributes_intrinsic(
            &mut vm,
            "getValue",
            "(Ljava/lang/String;)Ljava/lang/String;",
            attrs,
            &[key],
        )
        .unwrap();
        let value = match value {
            Some(Value::Obj(r)) => r,
            other => panic!("Attributes.getValue: {other:?}"),
        };
        assert_eq!(vm.heap.as_str(value).unwrap(), "server");

        let copy = vm
            .heap
            .alloc_instance(ATTRIBUTES_IMPL.to_string(), Vec::new())
            .unwrap();
        attributes_intrinsic(
            &mut vm,
            "<init>",
            "(Lorg/xml/sax/Attributes;)V",
            copy,
            &[Value::Obj(attrs)],
        )
        .unwrap();
        assert_eq!(
            attributes_intrinsic(&mut vm, "getLength", "()I", copy, &[]).unwrap(),
            Some(Value::Int(2))
        );
    }

    #[test]
    fn locator_impl_copies_standard_locator_values() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let source = vm
            .heap
            .alloc_instance(LOCATOR_IMPL.to_string(), Vec::new())
            .unwrap();
        super::set_field(&mut vm, source, "rd_sax_line", Value::Int(7)).unwrap();
        super::set_field(&mut vm, source, "rd_sax_column", Value::Int(13)).unwrap();
        let destination = vm
            .heap
            .alloc_instance(LOCATOR_IMPL.to_string(), Vec::new())
            .unwrap();
        locator_intrinsic(
            &mut vm,
            "<init>",
            "(Lorg/xml/sax/Locator;)V",
            destination,
            &[Value::Obj(source)],
        )
        .unwrap();
        assert_eq!(
            locator_intrinsic(&mut vm, "getLineNumber", "()I", destination, &[]).unwrap(),
            Some(Value::Int(7))
        );
        assert_eq!(
            locator_intrinsic(&mut vm, "getColumnNumber", "()I", destination, &[],).unwrap(),
            Some(Value::Int(13))
        );
    }

    #[test]
    fn parses_nested_tags_attributes_entities_and_cdata() {
        let events = parse_xml(
            "<configuration><tag name='A&amp;B'>text<![CDATA[<raw>]]></tag><empty /></configuration>",
        )
        .unwrap();
        let kinds: Vec<_> = events.into_iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            vec![
                XmlEventKind::Start {
                    name: "configuration".into(),
                    attributes: vec![],
                },
                XmlEventKind::Start {
                    name: "tag".into(),
                    attributes: vec![("name".into(), "A&B".into())],
                },
                XmlEventKind::Text("text".into()),
                XmlEventKind::Text("<raw>".into()),
                XmlEventKind::End { name: "tag".into() },
                XmlEventKind::Start {
                    name: "empty".into(),
                    attributes: vec![],
                },
                XmlEventKind::End {
                    name: "empty".into(),
                },
                XmlEventKind::End {
                    name: "configuration".into(),
                },
            ]
        );
    }

    #[test]
    fn rejects_external_entities_and_malformed_document_structure() {
        assert!(parse_xml("<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><x/>").is_err());
        assert!(parse_xml("<x><y></x>").is_err());
        assert!(parse_xml("<x/><y/>").is_err());
    }
}
