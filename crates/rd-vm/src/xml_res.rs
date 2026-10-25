//! Compiled XML resources: `Resources.getXml`/`getLayout`/`getAnimation`
//! return an `XmlResourceParser` over the APK's binary XML (AXML). It is
//! both an `XmlPullParser` and an `AttributeSet`.
//!
//! `Resources.obtainAttributes(set, attrs)` and
//! `Theme/Context.obtainStyledAttributes(set, attrs, ..)` on such a set
//! return a TypedArray whose indexes are filled from the current start tag's
//! attributes, matched by attribute resource id.
//!
//! Event model: START_DOCUMENT, then START_TAG/TEXT/END_TAG in document
//! order, then END_DOCUMENT. Parser and TypedArray side state lives in a
//! per-thread table keyed by object.
//!
//! **Deviations:**
//! - Theme attribute references (`?attr/x`) are reported as absent: themes
//!   are not resolved.
//! - Default styles (`defStyleAttr`/`defStyleRes`) are ignored.
//! - AXML decoding folds `TYPE_INT_COLOR_*` into plain ints, so
//!   `TypedArray.getValue` reports an int as a color type when the attribute
//!   name contains "color" or "tint".

use std::cell::RefCell;
use std::collections::HashMap;

use rd_apk::axml::{AttrValue, XmlAttribute, XmlElement};

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const PARSER: &str = "Landroid/content/res/XmlResourceParser;";
const RESOURCES: &str = "Landroid/content/res/Resources;";
const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";

#[derive(Clone)]
enum Ev {
    Start {
        name: String,
        attrs: Vec<XmlAttribute>,
        depth: i32,
    },
    Text(String, i32),
    End {
        name: String,
        depth: i32,
    },
}

struct ParserState {
    file: String,
    events: Vec<Ev>,
    /// -1 = START_DOCUMENT, events.len() = END_DOCUMENT.
    pos: isize,
}

thread_local! {
    static PARSERS: RefCell<HashMap<ObjRef, ParserState>> = RefCell::new(HashMap::new());
    static ARRAYS: RefCell<HashMap<ObjRef, Vec<Option<XmlAttribute>>>> = RefCell::new(HashMap::new());
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    PARSERS.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
    ARRAYS.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — parsers/atributos pendurados em
/// `ObjRef`s de um heap morto não podem vazar para o próximo `Engine`.
pub(crate) fn reset_all() {
    PARSERS.with(|m| m.borrow_mut().clear());
    ARRAYS.with(|m| m.borrow_mut().clear());
}

fn flatten(e: &XmlElement, depth: i32, out: &mut Vec<Ev>) {
    out.push(Ev::Start {
        name: e.name.clone(),
        attrs: e.attrs.clone(),
        depth,
    });
    if let Some(t) = &e.text {
        if !t.trim().is_empty() {
            out.push(Ev::Text(t.clone(), depth));
        }
    }
    for c in &e.children {
        flatten(c, depth + 1, out);
    }
    out.push(Ev::End {
        name: e.name.clone(),
        depth,
    });
}

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    Throwable::new(class, msg.into()).into()
}

fn not_found(id: i32) -> VmExit {
    exc(
        "Landroid/content/res/Resources$NotFoundException;",
        format!("Resource ID #0x{:x}", id as u32),
    )
}

/// Opens the compiled XML behind resource `id` as a parser object.
/// Bytes of the APK file a file-based resource id resolves to
/// (res/raw/*, res/xml/*, …), following resource references.
fn resource_file_bytes(vm: &mut Engine, id: i32) -> Result<Vec<u8>, VmExit> {
    let mut rid = id as u32;
    let mut path = None;
    for _ in 0..8 {
        match vm.resolve_resource_value(rid).map_err(|_| not_found(id))? {
            rd_apk::arsc::ResolvedValue::Str(s) => {
                path = Some(s);
                break;
            }
            rd_apk::arsc::ResolvedValue::Reference(r) => rid = r,
            _ => break,
        }
    }
    let path = path
        .filter(|p| p.starts_with("res/"))
        .ok_or_else(|| not_found(id))?;
    let apk = vm.fw.resources.as_ref().ok_or_else(|| not_found(id))?;
    let entry = apk.zip.find(&path).ok_or_else(|| not_found(id))?;
    apk.zip.read_entry(entry).map_err(|_| not_found(id))
}

fn open_xml(vm: &mut Engine, id: i32) -> Result<Value, VmExit> {
    let mut rid = id as u32;
    let mut path = None;
    for _ in 0..8 {
        match vm.resolve_resource_value(rid).map_err(|_| not_found(id))? {
            rd_apk::arsc::ResolvedValue::Str(s) => {
                path = Some(s);
                break;
            }
            rd_apk::arsc::ResolvedValue::Reference(r) => rid = r,
            _ => break,
        }
    }
    let path = path
        .filter(|p| p.ends_with(".xml"))
        .ok_or_else(|| not_found(id))?;
    let bytes = {
        let apk = vm.fw.resources.as_ref().ok_or_else(|| not_found(id))?;
        let entry = apk.zip.find(&path).ok_or_else(|| not_found(id))?;
        apk.zip.read_entry(entry).map_err(|_| not_found(id))?
    };
    let doc = rd_apk::axml::parse(&bytes).map_err(|e| {
        exc(
            "Landroid/content/res/Resources$NotFoundException;",
            format!("{path}: {}", e.cause),
        )
    })?;
    let mut events = Vec::new();
    flatten(&doc.root, 1, &mut events);
    let r = vm
        .heap
        .alloc_instance(PARSER.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    PARSERS.with(|p| {
        p.borrow_mut().insert(
            r,
            ParserState {
                file: path,
                events,
                pos: -1,
            },
        )
    });
    Ok(Value::Obj(r))
}

fn is_parser(r: ObjRef) -> bool {
    PARSERS.with(|p| p.borrow().contains_key(&r))
}

/// Attributes of the parser's current start tag.
fn current_attrs(r: ObjRef) -> Option<Vec<XmlAttribute>> {
    PARSERS.with(|p| {
        let p = p.borrow();
        let st = p.get(&r)?;
        match st.events.get(usize::try_from(st.pos).ok()?) {
            Some(Ev::Start { attrs, .. }) => Some(attrs.clone()),
            _ => None,
        }
    })
}

/// Fills TypedArray `ta` from AttributeSet `set` for the `attrs` id array.
/// No-op unless `set` is a parser created here.
pub(crate) fn fill_typed_array(
    vm: &Engine,
    ta: ObjRef,
    set: Option<&Value>,
    attrs: Option<&Value>,
) {
    let (Some(Value::Obj(s)), Some(Value::Obj(a))) = (set, attrs) else {
        return;
    };
    let Some(cur) = current_attrs(*s) else { return };
    let Ok((ids, _)) = vm.heap.as_array_elems(*a) else {
        return;
    };
    let vals: Vec<Option<XmlAttribute>> = ids
        .iter()
        .map(|id| {
            let id = match id {
                Value::Int(i) => *i as u32,
                _ => return None,
            };
            cur.iter()
                .find(|x| {
                    x.res_id == Some(id)
                        && !matches!(x.value, AttrValue::Attribute(_) | AttrValue::Null)
                })
                .cloned()
        })
        .collect();
    ARRAYS.with(|m| {
        let mut arrays = m.borrow_mut();
        if let Some(existing) = arrays.get(&ta) {
            let mut merged = existing.clone();
            merged.resize(vals.len(), None);
            for (i, value) in vals.into_iter().enumerate() {
                if value.is_some() {
                    merged[i] = value;
                }
            }
            arrays.insert(ta, merged);
        } else {
            arrays.insert(ta, vals);
        }
    });
}

/// Fill a TypedArray from a resolved style bag. Missing attributes remain
/// `None`, allowing the normal TypedArray default-value path to apply.
pub(crate) fn fill_theme_typed_array(
    vm: &Engine,
    ta: ObjRef,
    attrs: Option<&Value>,
    style_id: u32,
) {
    let Some(Value::Obj(a)) = attrs else { return };
    let Ok((ids, _)) = vm.heap.as_array_elems(*a) else {
        return;
    };
    let Some(arsc) = vm.fw.resources.as_ref().and_then(|apk| apk.arsc.as_ref()) else {
        return;
    };
    let values = ids
        .iter()
        .map(|id| {
            let Value::Int(attr_id) = id else {
                return None;
            };
            let attr_id = *attr_id as u32;
            let value = arsc.resolve_style_attribute(style_id, attr_id)?;
            let value = match value {
                rd_apk::arsc::ResolvedValue::Str(s) => AttrValue::String(s),
                rd_apk::arsc::ResolvedValue::Boolean(b) => AttrValue::Bool(b),
                rd_apk::arsc::ResolvedValue::Integer(i) => AttrValue::Int(i),
                rd_apk::arsc::ResolvedValue::Float(f) => AttrValue::Float(f),
                rd_apk::arsc::ResolvedValue::Dimension { value, unit } => {
                    AttrValue::Dimension(value, unit)
                }
                rd_apk::arsc::ResolvedValue::Reference(r) => AttrValue::Reference(r),
                rd_apk::arsc::ResolvedValue::Attribute(a) => AttrValue::Attribute(a),
            };
            Some(XmlAttribute {
                ns: None,
                name: arsc.resolve_name(attr_id).unwrap_or_default(),
                raw: None,
                value,
                res_id: Some(attr_id),
            })
        })
        .collect();
    ARRAYS.with(|m| m.borrow_mut().insert(ta, values));
}

fn str_val(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

fn str_arg(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

fn density() -> f32 {
    crate::android_res::device().dpi as f32 / 160.0
}

/// Follows resource references to a final value.
fn deref(vm: &Engine, v: &AttrValue) -> Option<rd_apk::arsc::ResolvedValue> {
    use rd_apk::arsc::ResolvedValue as R;
    let mut cur = match v {
        AttrValue::Reference(id) => R::Reference(*id),
        AttrValue::String(s) => return Some(R::Str(s.clone())),
        AttrValue::Int(i) => return Some(R::Integer(*i)),
        AttrValue::Bool(b) => return Some(R::Boolean(*b)),
        AttrValue::Float(f) => return Some(R::Float(*f)),
        AttrValue::Dimension(f, u) => {
            return Some(R::Dimension {
                value: *f,
                unit: *u,
            })
        }
        _ => return None,
    };
    for _ in 0..8 {
        match cur {
            R::Reference(id) => cur = vm.resolve_resource_value(id).ok()?,
            other => return Some(other),
        }
    }
    None
}

fn to_px(value: f32, unit: rd_apk::axml::ComplexUnit) -> f32 {
    let name = format!("{unit:?}").to_ascii_lowercase();
    let d = density();
    match name.as_str() {
        "dip" | "dp" | "sp" => value * d,
        "pt" => value * 280.0 / 72.0,
        "in" => value * 280.0,
        "mm" => value * 280.0 / 25.4,
        _ => value,
    }
}

fn as_f32(vm: &Engine, v: &AttrValue) -> Option<f32> {
    use rd_apk::arsc::ResolvedValue as R;
    Some(match deref(vm, v)? {
        R::Float(f) => f,
        R::Integer(i) => i as f32,
        R::Dimension { value, unit } => to_px(value, unit),
        R::Str(s) => s.parse().ok()?,
        R::Boolean(b) => b as i32 as f32,
        R::Reference(_) | R::Attribute(_) => return None,
    })
}

fn as_i32(vm: &Engine, v: &AttrValue) -> Option<i32> {
    use rd_apk::arsc::ResolvedValue as R;
    Some(match deref(vm, v)? {
        R::Integer(i) => i,
        R::Boolean(b) => b as i32,
        R::Float(f) => f as i32,
        R::Dimension { value, unit } => to_px(value, unit) as i32,
        R::Str(s) => {
            let t = s.trim();
            if let Some(h) = t.strip_prefix('#') {
                let n = u32::from_str_radix(h, 16).ok()?;
                (if h.len() <= 6 { n | 0xFF00_0000 } else { n }) as i32
            } else {
                t.parse().ok()?
            }
        }
        R::Reference(_) | R::Attribute(_) => return None,
    })
}

/// TypedArray reads backed by a filled index table. `None` = not ours.
fn typed_array_get(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let Some(vals) = ARRAYS.with(|m| m.borrow().get(&recv).cloned()) else {
        return Ok(None);
    };
    let idx = match args.first() {
        Some(Value::Int(i)) => *i as usize,
        _ => usize::MAX,
    };
    let attr = vals.get(idx).cloned().flatten();
    let def = args.last().cloned().unwrap_or(Value::Null);
    let v = match name {
        "length" => Value::Int(vals.len() as i32),
        "getIndexCount" => Value::Int(vals.iter().filter(|v| v.is_some()).count() as i32),
        "getIndex" => {
            let n = vals
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_some())
                .nth(idx)
                .map(|(i, _)| i as i32);
            Value::Int(n.unwrap_or(0))
        }
        "recycle" | "close" => {
            ARRAYS.with(|m| m.borrow_mut().remove(&recv));
            Value::Null
        }
        _ => {
            let Some(a) = attr else { return Ok(None) }; // absent → legacy default path
            match name {
                "hasValue" | "hasValueOrEmpty" => Value::Int(1),
                "getFloat" => as_f32(vm, &a.value).map(Value::Float).unwrap_or(def),
                "getDimension" => as_f32(vm, &a.value).map(Value::Float).unwrap_or(def),
                "getDimensionPixelSize" => as_f32(vm, &a.value)
                    .map(|f| {
                        Value::Int(if f > 0.0 {
                            (f + 0.5).max(1.0) as i32
                        } else {
                            f.round() as i32
                        })
                    })
                    .unwrap_or(def),
                "getDimensionPixelOffset" | "getLayoutDimension" => as_f32(vm, &a.value)
                    .map(|f| Value::Int(f as i32))
                    .unwrap_or(def),
                "getInt" | "getInteger" | "getColor" => {
                    as_i32(vm, &a.value).map(Value::Int).unwrap_or(def)
                }
                "getBoolean" => as_i32(vm, &a.value)
                    .map(|i| Value::Int((i != 0) as i32))
                    .unwrap_or(def),
                "getResourceId" | "getSourceResourceId" => match a.value {
                    AttrValue::Reference(id) => Value::Int(id as i32),
                    _ => def,
                },
                "getString" | "getText" | "getNonResourceString" => match deref(vm, &a.value) {
                    Some(rd_apk::arsc::ResolvedValue::Str(s)) => str_val(vm, s)?,
                    Some(_) if name != "getNonResourceString" => str_val(vm, a.text())?,
                    _ => Value::Null,
                },
                "getType" => Value::Int(type_of(&a)),
                "getValue" if sig.contains("TypedValue") => {
                    let Some(Value::Obj(tv)) = args.get(1).cloned() else {
                        return Ok(Some(Value::Int(0)));
                    };
                    let ty = type_of(&a);
                    let data = match &a.value {
                        AttrValue::Reference(id) => *id as i32,
                        AttrValue::Float(f) => f.to_bits() as i32,
                        AttrValue::Bool(b) => *b as i32,
                        other => as_i32(vm, other).unwrap_or(0),
                    };
                    let _ = vm.heap.put_field(tv, "type", Value::Int(ty));
                    let _ = vm.heap.put_field(tv, "data", Value::Int(data));
                    let _ = vm.heap.put_field(
                        tv,
                        "resourceId",
                        Value::Int(if let AttrValue::Reference(id) = a.value {
                            id as i32
                        } else {
                            0
                        }),
                    );
                    if let AttrValue::String(s) = &a.value {
                        let s = str_val(vm, s.clone())?;
                        let _ = vm.heap.put_field(tv, "string", s);
                    }
                    Value::Int(1)
                }
                // Complex colors / drawables need inflation that is not modelled.
                "getColorStateList" | "getDrawable" | "getFont" | "getTextArray" => Value::Null,
                _ => return Ok(None),
            }
        }
    };
    Ok(Some(v))
}

fn type_of(a: &XmlAttribute) -> i32 {
    match &a.value {
        AttrValue::Null => 0,
        AttrValue::Reference(_) => 1,
        AttrValue::Attribute(_) => 2,
        AttrValue::String(_) => 3,
        AttrValue::Float(_) => 4,
        AttrValue::Dimension(..) => 5,
        AttrValue::Fraction(..) => 6,
        AttrValue::Bool(_) => 0x12,
        AttrValue::Int(_) => {
            let n = a.name.to_ascii_lowercase();
            if n.contains("color") || n.contains("tint") {
                0x1c
            } else {
                0x10
            }
        }
        AttrValue::Raw { dtype, .. } => *dtype as i32,
    }
}

fn find_attr(vm: &Engine, recv: ObjRef, args: &[Value], indexed: bool) -> Option<XmlAttribute> {
    let attrs = current_attrs(recv)?;
    if indexed {
        let i = match args.first() {
            Some(Value::Int(i)) => *i as usize,
            _ => return None,
        };
        return attrs.get(i).cloned();
    }
    let ns = str_arg(vm, args.first());
    let name = str_arg(vm, args.get(1))?;
    attrs.into_iter().find(|a| {
        a.name == name
            && match ns.as_deref() {
                None => true,
                Some(ANDROID_NS) => a.is_android(),
                Some(other) => a.ns.as_deref() == Some(other),
            }
    })
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class == RESOURCES || mref_class == RESOURCES {
        return Ok(match (name, sig) {
            (
                "getXml" | "getLayout" | "getAnimation",
                "(I)Landroid/content/res/XmlResourceParser;",
            ) => {
                let id = match args.first() {
                    Some(Value::Int(i)) => *i,
                    _ => 0,
                };
                Some(open_xml(vm, id)?)
            }
            ("openRawResource", "(I)Ljava/io/InputStream;")
            | ("openRawResource", "(ILandroid/util/TypedValue;)Ljava/io/InputStream;") => {
                let id = match args.first() {
                    Some(Value::Int(i)) => *i,
                    _ => 0,
                };
                let bytes = resource_file_bytes(vm, id)?;
                Some(crate::jio_chars::new_byte_input_stream(vm, bytes)?)
            }
            _ => None,
        });
    }
    if recv_class == "Landroid/content/res/TypedArray;" {
        return typed_array_get(vm, recv, name, sig, args);
    }
    if recv_class != PARSER || !is_parser(recv) {
        return Ok(None);
    }
    // Pull-parser state machine.
    let state = PARSERS.with(|p| {
        let p = p.borrow();
        let st = &p[&recv];
        let ev = usize::try_from(st.pos)
            .ok()
            .and_then(|i| st.events.get(i).cloned());
        (st.pos, st.events.len() as isize, ev, st.file.clone())
    });
    let (pos, len, ev, file) = state;
    let event_type = |pos: isize, ev: &Option<Ev>| -> i32 {
        if pos < 0 {
            0
        } else if pos >= len {
            1
        } else {
            match ev {
                Some(Ev::Start { .. }) => 2,
                Some(Ev::End { .. }) => 3,
                _ => 4,
            }
        }
    };
    let opt_str = |vm: &mut Engine, s: Option<String>| -> Result<Value, VmExit> {
        match s {
            Some(s) => str_val(vm, s),
            None => Ok(Value::Null),
        }
    };
    let v = match name {
        "next" | "nextToken" | "nextTag" => {
            if pos >= len {
                return Err(exc(
                    "Lorg/xmlpull/v1/XmlPullParserException;",
                    "END_DOCUMENT reached",
                ));
            }
            let mut np = pos + 1;
            if name == "nextTag" {
                while np < len
                    && matches!(
                        PARSERS.with(|p| p.borrow()[&recv].events.get(np as usize).cloned()),
                        Some(Ev::Text(..))
                    )
                {
                    np += 1;
                }
            }
            PARSERS.with(|p| p.borrow_mut().get_mut(&recv).map(|s| s.pos = np));
            let nev = PARSERS.with(|p| p.borrow()[&recv].events.get(np as usize).cloned());
            Value::Int(event_type(np, &nev))
        }
        "getEventType" => Value::Int(event_type(pos, &ev)),
        "getName" => {
            let n = match &ev {
                Some(Ev::Start { name, .. }) | Some(Ev::End { name, .. }) => Some(name.clone()),
                _ => None,
            };
            opt_str(vm, n)?
        }
        "getText" => {
            let t = match &ev {
                Some(Ev::Text(t, _)) => Some(t.clone()),
                _ => None,
            };
            opt_str(vm, t)?
        }
        "isEmptyElementWhitespace" | "isWhitespace" => Value::Int(0),
        "getDepth" => Value::Int(match &ev {
            Some(Ev::Start { depth, .. })
            | Some(Ev::End { depth, .. })
            | Some(Ev::Text(_, depth)) => *depth,
            _ => 0,
        }),
        "getNamespace" => {
            if sig == "()Ljava/lang/String;" {
                str_val(vm, String::new())?
            } else {
                Value::Null
            }
        }
        "getPositionDescription" => {
            str_val(vm, format!("Binary XML file {file} event #{}", pos + 1))?
        }
        "getLineNumber" => Value::Int((pos + 1) as i32),
        "getColumnNumber" => Value::Int(-1),
        "close" => {
            PARSERS.with(|p| p.borrow_mut().remove(&recv));
            Value::Null
        }
        "require" => Value::Null,
        "getAttributeCount" => {
            Value::Int(current_attrs(recv).map(|a| a.len() as i32).unwrap_or(-1))
        }
        "getAttributeName" => {
            let a = find_attr(vm, recv, args, true).map(|a| a.name);
            opt_str(vm, a)?
        }
        "getAttributeNamespace" => {
            let a = find_attr(vm, recv, args, true).map(|a| {
                if a.is_android() {
                    ANDROID_NS.to_string()
                } else {
                    a.ns.unwrap_or_default()
                }
            });
            opt_str(vm, a)?
        }
        "getAttributeNameResource" => Value::Int(
            find_attr(vm, recv, args, true)
                .and_then(|a| a.res_id)
                .unwrap_or(0) as i32,
        ),
        "getAttributeValue" => {
            let indexed = sig == "(I)Ljava/lang/String;";
            let a = find_attr(vm, recv, args, indexed).map(|a| match a.value {
                AttrValue::Reference(id) => format!("@{}", id as i32),
                AttrValue::Attribute(id) => format!("?{}", id as i32),
                _ => a.text(),
            });
            opt_str(vm, a)?
        }
        "getAttributeResourceValue"
        | "getAttributeIntValue"
        | "getAttributeUnsignedIntValue"
        | "getAttributeBooleanValue"
        | "getAttributeFloatValue" => {
            let indexed = sig.starts_with("(I");
            let def = args.last().cloned().unwrap_or(Value::Int(0));
            match find_attr(vm, recv, args, indexed) {
                None => def,
                Some(a) => match name {
                    "getAttributeResourceValue" => match a.value {
                        AttrValue::Reference(id) => Value::Int(id as i32),
                        _ => def,
                    },
                    "getAttributeFloatValue" => {
                        as_f32(vm, &a.value).map(Value::Float).unwrap_or(def)
                    }
                    "getAttributeBooleanValue" => {
                        a.as_bool().map(|b| Value::Int(b as i32)).unwrap_or(def)
                    }
                    _ => a.as_i32().map(Value::Int).unwrap_or(def),
                },
            }
        }
        "getIdAttribute" | "getClassAttribute" => {
            let n = if name == "getIdAttribute" {
                "id"
            } else {
                "class"
            };
            let a = current_attrs(recv)
                .and_then(|v| v.into_iter().find(|a| a.name == n))
                .map(|a| a.text());
            opt_str(vm, a)?
        }
        "getIdAttributeResourceValue" => {
            let d = args.first().cloned().unwrap_or(Value::Int(0));
            match current_attrs(recv)
                .and_then(|v| v.into_iter().find(|a| a.name == "id"))
                .map(|a| a.value)
            {
                Some(AttrValue::Reference(id)) => Value::Int(id as i32),
                _ => d,
            }
        }
        "getStyleAttribute" => Value::Int(0),
        "getFeature" => Value::Int(0),
        "setFeature" | "setInput" | "defineEntityReplacementText" => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// `android.util.Xml.asAttributeSet(parser)`: a compiled-resource parser is
/// its own AttributeSet, as `XmlBlock.Parser` is in AOSP.
pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let _ = vm;
    if class != "Landroid/util/Xml;" || name != "asAttributeSet" {
        return Ok(None);
    }
    Ok(Some(args.first().cloned().unwrap_or(Value::Null)))
}
