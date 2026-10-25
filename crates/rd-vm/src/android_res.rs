//! Resources, DisplayMetrics, Configuration, Theme, TypedArray, TypedValue,
//! LocaleList and `java.util.Locale`.
//!
//! **Device model.** The VM models the MCP device profile (moto-e5, the
//! contractual floor): 720×1440 px at 280 dpi. That gives density 1.75 and
//! 411×822 dp, portrait, `uiMode` NORMAL | NIGHT_NO, `fontScale` 1.0.
//! Override with `RD_DENSITY_DPI`, `RD_SCREEN_W`/`RD_SCREEN_H`,
//! `RD_NIGHT_MODE=1` and `RD_LOCALE` (BCP-47, default `en-US`).
//!
//! **DisplayMetrics and Configuration** are host objects with real heap
//! fields (`density`, `densityDpi`, `widthPixels`, …, `screenWidthDp`,
//! `fontScale`, `uiMode`, `locale`, …), so DEX `iget` reads them directly.
//! The `Configuration(Configuration)` copy constructor, `setTo`,
//! `updateFrom`, `diff`, `equals`, `getLocales`, `setLocale(s)` and
//! `isNightModeActive` follow AOSP.
//!
//! **Resources** lookups go to `resources.arsc` (best config): `getString`
//! (± format args), `getText`, `getDimension*` (unit conversion via the
//! model's density/fontScale), `getInteger`, `getBoolean`, `getColor`,
//! `getValue(TypedValue)`, `getIdentifier`, `getResource*Name`. Reference
//! chains are followed. An unknown id throws
//! `Resources$NotFoundException`, as AOSP does.
//!
//! **Themes and styled attributes.** These are not resolved; the platform
//! theme graph is not modelled.
//! - `Theme.resolveAttribute` returns false.
//! - `obtainStyledAttributes` returns a TypedArray where no index has a
//!   value: `hasValue` is false, `getX(i, def)` returns `def`, and getters
//!   without a default return null. This is a documented deviation; code
//!   written against AOSP must already handle missing attributes.
//!
//! **Context family.** `getResources`, `getTheme`, `obtainStyledAttributes`,
//! `getMainLooper`, `getBaseContext`, `isRestricted`, `getAttributionTag`,
//! `getAttributionSource`, `getAssets`, `getApplicationInfo`,
//! `getText` and `getString(I, Object…)` are answered for any receiver that
//! is a `Context` subtype. That covers Activity, Application, Service, and
//! DEX `ContextWrapper`s, whatever class the method ref names. `getResources`
//! also covers Views. `getApplicationInfo` delegates to the same host model
//! `PackageManager` serves for the own package (android_pm.rs).
//!
//! **AssetManager (APK-Tester ladder 12/12).** `Context.getAssets()` and
//! `Resources.getAssets()` return one canonical host `AssetManager` per VM.
//! `open(String[, int])` streams the REAL bytes of `assets/<name>` from the
//! attached APK ZIP (Lei de Ouro nº 2 — compat > elegância) as a host
//! `ByteArrayInputStream`; a missing entry raises the same typed
//! `FileNotFoundException` AOSP raises. `list(path)` reports the immediate
//! entries under `assets/<path>/` (first level, like AOSP). `openFd` always
//! throws `FileNotFoundException` ("probably compressed") — matching an APK
//! whose entries are deflated, which is the common case.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;
use rd_apk::arsc::ResolvedValue;
use rd_apk::axml::ComplexUnit;

const RESOURCES: &str = "Landroid/content/res/Resources;";
const DM: &str = "Landroid/util/DisplayMetrics;";
const CONFIG: &str = "Landroid/content/res/Configuration;";
const THEME: &str = "Landroid/content/res/Resources$Theme;";
const TYPED_ARRAY: &str = "Landroid/content/res/TypedArray;";
const TYPED_VALUE: &str = "Landroid/util/TypedValue;";
const LOCALE: &str = "Ljava/util/Locale;";
const LOCALE_LIST: &str = "Landroid/os/LocaleList;";
const ASSET_MANAGER: &str = "Landroid/content/res/AssetManager;";
const FILE_NOT_FOUND: &str = "Ljava/io/FileNotFoundException;";

/// Configuration fields copied by the copy constructor / setTo / diff.
const CONFIG_FIELDS: &[&str] = &[
    "fontScale",
    "densityDpi",
    "screenWidthDp",
    "screenHeightDp",
    "smallestScreenWidthDp",
    "orientation",
    "uiMode",
    "locale",
    "keyboard",
    "keyboardHidden",
    "hardKeyboardHidden",
    "navigation",
    "navigationHidden",
    "touchscreen",
    "screenLayout",
    "colorMode",
    "mcc",
    "mnc",
    "fontWeightAdjustment",
    "rd_locales",
];

fn env_u32(k: &str, d: u32) -> u32 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

pub(crate) struct Device {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) dpi: u32,
}

pub(crate) fn device() -> Device {
    Device {
        w: env_u32("RD_SCREEN_W", 720),
        h: env_u32("RD_SCREEN_H", 1440),
        dpi: env_u32("RD_DENSITY_DPI", 280),
    }
}

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.into()))
}

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

fn get(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}

fn s(vm: &mut Engine, t: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, t.into())?))
}

fn str_of(vm: &Engine, v: &Value) -> Option<String> {
    match v {
        Value::Obj(r) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

/// Singleton per key, stored in the static table.
fn singleton(
    vm: &mut Engine,
    key: &str,
    class: &str,
    init: fn(&mut Engine, ObjRef) -> Result<(), VmExit>,
) -> Result<ObjRef, VmExit> {
    let k = ("Lrd/Resources;".to_string(), key.to_string());
    if let Some(Value::Obj(r)) = vm.statics.get(&k) {
        return Ok(*r);
    }
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    vm.statics.insert(k, Value::Obj(r));
    init(vm, r)?;
    Ok(r)
}

fn resources(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    singleton(vm, "resources", RESOURCES, |_, _| Ok(()))
}

/// The canonical host `AssetManager` for this VM — what every
/// `Context.getAssets()`/`Resources.getAssets()` call hands back (AOSP has
/// one manager per Resources, and one Resources per process context set).
fn asset_manager(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    singleton(vm, "asset_manager", ASSET_MANAGER, |_, _| Ok(()))
}

// ── assets/ lookups (APK-Tester ladder) ────────────────────────────────

/// Rejects paths the AOSP asset layer never resolves: absolute, empty
/// segments, `.`/`..` traversal and backslashes. Keeping this check on THIS
/// side of the zip lookup means an app can never pivot `assets/…` into an
/// arbitrary ZIP entry.
fn asset_path_is_hostile(file: &str) -> bool {
    file.is_empty()
        || file.starts_with('/')
        || file.contains('\\')
        || file
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
}

/// Reads `assets/<file>` from the attached APK — real bytes, no stub. A
/// missing entry is the typed `FileNotFoundException` AOSP throws (never a
/// silent empty stream — Lei nº 1).
fn open_asset(vm: &mut Engine, file: &str) -> Result<Value, VmExit> {
    let entry = format!("assets/{file}");
    if asset_path_is_hostile(file) {
        return Err(exc(FILE_NOT_FOUND, entry));
    }
    let apk = vm.resources()?;
    let bytes = apk
        .zip
        .read(&entry)
        .map_err(|e| exc(FILE_NOT_FOUND, format!("{entry}: {e}")))?;
    crate::jio_chars::new_byte_input_stream(vm, bytes)
}

/// Immediate entries under `assets/<path>/` — the first level only, which is
/// what AOSP `AssetManager.list` reports (directories show up as bare names,
/// the caller lists them again to descend).
fn list_assets(vm: &mut Engine, path: &str) -> Result<Value, VmExit> {
    if asset_path_is_hostile(path) && !path.is_empty() {
        return Err(exc(FILE_NOT_FOUND, format!("assets/{path}")));
    }
    let apk = vm.resources()?;
    let mut prefix = String::from("assets/");
    if !path.is_empty() {
        prefix.push_str(path.trim_end_matches('/'));
        prefix.push('/');
    }
    let mut names: Vec<String> = Vec::new();
    for entry in apk.zip.names() {
        let Some(rest) = entry.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if rest.is_empty() {
            continue; // a directory entry for the prefix itself
        }
        let head = rest.split('/').next().unwrap_or(rest);
        if !names.iter().any(|n| n == head) {
            names.push(head.to_string());
        }
    }
    let items = names
        .into_iter()
        .map(|n| s(vm, n))
        .collect::<Result<Vec<Value>, VmExit>>()?;
    crate::arrays::new_array(vm, "[Ljava/lang/String;", ElemKind::Obj, items)
}

fn metrics(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    singleton(vm, "metrics", DM, |vm, r| {
        let d = device();
        let density = d.dpi as f32 / 160.0;
        put(vm, r, "density", Value::Float(density))?;
        put(vm, r, "scaledDensity", Value::Float(density))?;
        put(vm, r, "densityDpi", Value::Int(d.dpi as i32))?;
        put(vm, r, "widthPixels", Value::Int(d.w as i32))?;
        put(vm, r, "heightPixels", Value::Int(d.h as i32))?;
        put(vm, r, "xdpi", Value::Float(d.dpi as f32))?;
        put(vm, r, "ydpi", Value::Float(d.dpi as f32))
    })
}

fn configuration(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    singleton(vm, "configuration", CONFIG, |vm, r| {
        let d = device();
        let density = d.dpi as f32 / 160.0;
        let (wdp, hdp) = ((d.w as f32 / density) as i32, (d.h as f32 / density) as i32);
        put(vm, r, "fontScale", Value::Float(1.0))?;
        put(vm, r, "densityDpi", Value::Int(d.dpi as i32))?;
        put(vm, r, "screenWidthDp", Value::Int(wdp))?;
        put(vm, r, "screenHeightDp", Value::Int(hdp))?;
        put(vm, r, "smallestScreenWidthDp", Value::Int(wdp.min(hdp)))?;
        put(
            vm,
            r,
            "orientation",
            Value::Int(if d.h >= d.w { 1 } else { 2 }),
        )?;
        let night = std::env::var("RD_NIGHT_MODE")
            .map(|v| v == "1")
            .unwrap_or(false);
        put(
            vm,
            r,
            "uiMode",
            Value::Int(0x01 | if night { 0x20 } else { 0x10 }),
        )?;
        put(vm, r, "keyboard", Value::Int(1))?; // KEYBOARD_NOKEYS
        put(vm, r, "keyboardHidden", Value::Int(1))?;
        put(vm, r, "hardKeyboardHidden", Value::Int(2))?;
        put(vm, r, "navigation", Value::Int(1))?; // NAVIGATION_NONAV
        put(vm, r, "navigationHidden", Value::Int(1))?;
        put(vm, r, "touchscreen", Value::Int(3))?; // TOUCHSCREEN_FINGER
        put(vm, r, "screenLayout", Value::Int(0x02 | 0x10 | 0x100))?; // NORMAL|LONG_NO|LTR
        put(vm, r, "colorMode", Value::Int(0x05))?;
        put(vm, r, "mcc", Value::Int(0))?;
        put(vm, r, "mnc", Value::Int(0))?;
        put(vm, r, "fontWeightAdjustment", Value::Int(0))?;
        let loc = default_locale(vm)?;
        put(vm, r, "locale", Value::Obj(loc))
    })
}

// ── Locale ─────────────────────────────────────────────────────────────

fn new_locale(vm: &mut Engine, lang: &str, country: &str, variant: &str) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_instance(LOCALE.to_string(), Vec::new())?;
    let (l, c, v) = (
        s(vm, lang.to_lowercase())?,
        s(vm, country.to_uppercase())?,
        s(vm, variant)?,
    );
    put(vm, r, "rd_lang", l)?;
    put(vm, r, "rd_country", c)?;
    put(vm, r, "rd_variant", v)?;
    Ok(r)
}

fn parse_tag(tag: &str) -> (String, String) {
    let mut parts = tag.split(['-', '_']);
    let lang = parts.next().unwrap_or("").to_string();
    let country = parts
        .find(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())))
        .unwrap_or("")
        .to_string();
    (if lang == "und" { String::new() } else { lang }, country)
}

fn default_locale(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let k = ("Lrd/Resources;".to_string(), "locale".to_string());
    if let Some(Value::Obj(r)) = vm.statics.get(&k) {
        return Ok(*r);
    }
    let tag = std::env::var("RD_LOCALE").unwrap_or_else(|_| "en-US".into());
    let (l, c) = parse_tag(&tag);
    let r = new_locale(vm, &l, &c, "")?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(r)
}

fn locale_parts(vm: &Engine, r: ObjRef) -> (String, String, String) {
    let f = |n: &str| str_of(vm, &get(vm, r, n)).unwrap_or_default();
    (f("rd_lang"), f("rd_country"), f("rd_variant"))
}

fn language_tag(vm: &Engine, r: ObjRef) -> String {
    let (l, c, _) = locale_parts(vm, r);
    match (l.is_empty(), c.is_empty()) {
        (true, _) => "und".into(),
        (false, true) => l,
        (false, false) => format!("{l}-{c}"),
    }
}

fn locale_list(vm: &mut Engine, locales: Vec<Value>) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance(LOCALE_LIST.to_string(), Vec::new())?;
    let arr = crate::arrays::new_array(vm, "[Ljava/util/Locale;", ElemKind::Obj, locales)?;
    put(vm, r, "rd_locales", arr)?;
    Ok(r)
}

fn locale_list_items(vm: &Engine, r: ObjRef) -> Vec<Value> {
    match get(vm, r, "rd_locales") {
        Value::Obj(a) => vm
            .heap
            .as_array_elems(a)
            .map(|(e, _)| e.to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

// ── arsc-backed lookups ────────────────────────────────────────────────

fn not_found(id: i32) -> VmExit {
    exc(
        "Landroid/content/res/Resources$NotFoundException;",
        format!("Resource ID #0x{:x}", id as u32),
    )
}

fn value_of(vm: &Engine, id: i32) -> Result<ResolvedValue, VmExit> {
    let mut cur = id as u32;
    for _ in 0..8 {
        match vm.resolve_resource_value(cur) {
            Ok(ResolvedValue::Reference(next)) => cur = next,
            Ok(v) => return Ok(v),
            Err(_) => return Err(not_found(id)),
        }
    }
    Err(not_found(id))
}

fn dimension_px(vm: &Engine, v: &ResolvedValue, id: i32) -> Result<f32, VmExit> {
    let d = device();
    let density = d.dpi as f32 / 160.0;
    Ok(match v {
        ResolvedValue::Dimension { value, unit } => match unit {
            ComplexUnit::Px => *value,
            ComplexUnit::Dip => value * density,
            ComplexUnit::Sp => value * density, // fontScale 1.0
            ComplexUnit::Pt => value * d.dpi as f32 / 72.0,
            ComplexUnit::In => value * d.dpi as f32,
            ComplexUnit::Mm => value * d.dpi as f32 / 25.4,
            _ => return Err(not_found(id)),
        },
        ResolvedValue::Integer(i) => *i as f32,
        ResolvedValue::Float(f) => *f,
        _ => {
            let _ = vm;
            return Err(not_found(id));
        }
    })
}

fn string_res(vm: &Engine, id: i32) -> Result<String, VmExit> {
    match vm
        .fw
        .resources
        .as_ref()
        .and_then(|a| a.arsc.as_ref())
        .and_then(|a| a.resolve_string(id as u32))
    {
        Some(t) => Ok(t),
        None => Err(not_found(id)),
    }
}

fn identifier(vm: &Engine, name: &str, def_type: Option<&str>, def_pkg: Option<&str>) -> i32 {
    let Some(arsc) = vm.fw.resources.as_ref().and_then(|a| a.arsc.as_ref()) else {
        return 0;
    };
    // "pkg:type/entry" | "type/entry" | "entry"
    let (pkg, rest) = match name.split_once(':') {
        Some((p, r)) => (Some(p), r),
        None => (def_pkg, name),
    };
    let (ty, entry) = match rest.split_once('/') {
        Some((t, e)) => (Some(t), e),
        None => (def_type, rest),
    };
    let Some(ty) = ty else { return 0 };
    let want = format!("@{ty}/{entry}");
    for p in &arsc.packages {
        if pkg.is_some_and(|n| n != p.name && n != "android" && !p.name.is_empty())
            && pkg != Some(p.name.as_str())
        {
            continue;
        }
        for id in p.entries.keys() {
            if p.resolve_name(*id).as_deref() == Some(want.as_str()) {
                return *id as i32;
            }
        }
    }
    0
}

fn fill_typed_value(vm: &mut Engine, tv: ObjRef, id: i32, v: &ResolvedValue) -> Result<(), VmExit> {
    // TypedValue.TYPE_*: STRING 3, FLOAT 4, DIMENSION 5, INT_DEC 16, BOOLEAN 18, INT_COLOR_ARGB8 28
    let (ty, data, string) = match v {
        ResolvedValue::Reference(id) => (1, *id as i32, None),
        ResolvedValue::Attribute(id) => (2, *id as i32, None),
        ResolvedValue::Str(t) => (3, 0, Some(t.clone())),
        ResolvedValue::Float(f) => (4, f.to_bits() as i32, None),
        ResolvedValue::Dimension { .. } => (5, dimension_px(vm, v, id)?.round() as i32, None),
        ResolvedValue::Integer(i) => (16, *i, None),
        ResolvedValue::Boolean(b) => (18, *b as i32, None),
    };
    put(vm, tv, "type", Value::Int(ty))?;
    put(vm, tv, "data", Value::Int(data))?;
    put(vm, tv, "resourceId", Value::Int(id))?;
    put(vm, tv, "density", Value::Int(device().dpi as i32))?;
    let sv = match string {
        Some(t) => s(vm, t)?,
        None => Value::Null,
    };
    put(vm, tv, "string", sv)
}

fn typed_array(vm: &mut Engine, attrs: Option<&Value>) -> Result<Value, VmExit> {
    let n = match attrs {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| e.len())
            .unwrap_or(0),
        _ => 0,
    };
    let r = vm
        .heap
        .alloc_instance(TYPED_ARRAY.to_string(), Vec::new())?;
    put(vm, r, "rd_len", Value::Int(n as i32))?;
    Ok(Value::Obj(r))
}

fn parse_theme_id(raw: &str) -> Option<u32> {
    let raw = raw.strip_prefix('@').unwrap_or(raw);
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        raw.parse().ok()
    }
}

fn theme_id_for_context(vm: &Engine, context: ObjRef) -> Option<u32> {
    if let Some(Value::Int(id)) = vm
        .view_props
        .get(&(context, "rd_theme_override".to_string()))
    {
        return (*id != 0).then_some(*id as u32);
    }
    let apk = vm.fw.resources.as_ref()?;
    let class = vm.heap.class_of(context).ok()?;
    if vm.cp.is_subtype(class, "Landroid/app/Activity;") {
        let name = class
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', ".");
        if let Some(id) = apk
            .manifest
            .application
            .components
            .iter()
            .find(|component| component.class_name == name)
            .and_then(|component| component.theme.as_deref())
            .and_then(parse_theme_id)
        {
            return Some(id);
        }
    }
    apk.manifest.application.theme_res
}

fn theme_style_id(vm: &Engine, theme: ObjRef) -> Option<u32> {
    match vm.heap.get_field(theme, "rd_theme_id") {
        Ok(Value::Int(id)) if id != 0 => Some(id as u32),
        _ => None,
    }
}

fn style_attribute(vm: &Engine, style_id: u32, attr_id: u32) -> Option<ResolvedValue> {
    vm.fw
        .resources
        .as_ref()?
        .arsc
        .as_ref()?
        .resolve_style_attribute(style_id, attr_id)
}

fn format_string(vm: &mut Engine, fmt: String, arr: &Value) -> Result<Value, VmExit> {
    let f = s(vm, fmt)?;
    vm.invoke_static(
        "Ljava/lang/String;",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        &[f, arr.clone()],
    )
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (class, name, sig) {
        (TYPED_VALUE, "applyDimension", "(IFLandroid/util/DisplayMetrics;)F") => {
            let unit = args
                .first()
                .and_then(|value| value.as_int().ok())
                .unwrap_or(0);
            let value = args
                .get(1)
                .and_then(|value| value.as_float().ok())
                .unwrap_or(0.0);
            let metrics_ref = args.get(2).and_then(|value| value.as_ref().ok().flatten());
            let metric = |field: &str| {
                metrics_ref
                    .and_then(|metrics| vm.heap.get_field(metrics, field).ok())
                    .and_then(|value| match value {
                        Value::Float(number) => Some(number),
                        Value::Double(number) => Some(number as f32),
                        _ => None,
                    })
                    .unwrap_or_else(|| device().dpi as f32 / 160.0)
            };
            let pixels = match unit {
                0 => value,                     // COMPLEX_UNIT_PX
                1 => value * metric("density"), // COMPLEX_UNIT_DIP
                2 => value * metric("scaledDensity"),
                3 => value * metric("xdpi") / 72.0,
                4 => value * metric("xdpi"),
                5 => value * metric("xdpi") / 25.4,
                _ => 0.0,
            };
            Value::Float(pixels)
        }
        (RESOURCES, "getSystem", _) => Value::Obj(resources(vm)?),
        (LOCALE, "getDefault", "()Ljava/util/Locale;") => Value::Obj(default_locale(vm)?),
        (LOCALE, "setDefault", "(Ljava/util/Locale;)V") => {
            if let Some(Value::Obj(l)) = args.first() {
                vm.statics
                    .insert(("Lrd/Resources;".into(), "locale".into()), Value::Obj(*l));
            }
            Value::Null
        }
        (LOCALE, "forLanguageTag", _) => {
            let t = str_of(vm, args.first().unwrap_or(&Value::Null))
                .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "languageTag"))?;
            let (l, c) = parse_tag(&t);
            Value::Obj(new_locale(vm, &l, &c, "")?)
        }
        (LOCALE_LIST, "getDefault" | "getAdjustedDefault", _) => {
            let l = default_locale(vm)?;
            Value::Obj(locale_list(vm, vec![Value::Obj(l)])?)
        }
        (LOCALE_LIST, "getEmptyLocaleList", _) => Value::Obj(locale_list(vm, Vec::new())?),
        (LOCALE_LIST, "forLanguageTags", _) => {
            let t = str_of(vm, args.first().unwrap_or(&Value::Null)).unwrap_or_default();
            let mut ls = Vec::new();
            for tag in t.split(',').filter(|x| !x.is_empty()) {
                let (l, c) = parse_tag(tag);
                ls.push(Value::Obj(new_locale(vm, &l, &c, "")?));
            }
            Value::Obj(locale_list(vm, ls)?)
        }
        _ => return Ok(None),
    }))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    match mref_class {
        ASSET_MANAGER => {
            return Ok(Some(match (name, sig) {
                ("<init>", "()V") => Value::Null,
                ("open", "(Ljava/lang/String;)Ljava/io/InputStream;")
                | ("open", "(Ljava/lang/String;I)Ljava/io/InputStream;") => {
                    // accessMode (UNKNOWN/RANDOM/STREAMING/BUFFER) all stream
                    // the same bytes; ByteArrayInputStream already supports
                    // mark/reset, which is what ACCESS_BUFFER asks for.
                    let file =
                        str_of(vm, args.first().unwrap_or(&Value::Null)).ok_or_else(|| {
                            exc("Ljava/lang/NullPointerException;", "fileName == null")
                        })?;
                    open_asset(vm, &file)?
                }
                ("openFd", "(Ljava/lang/String;)Landroid/content/res/AssetFileDescriptor;")
                | (
                    "openNonAssetFd",
                    "(Ljava/lang/String;)Landroid/content/res/AssetFileDescriptor;",
                ) => {
                    // Zip entries in a stock APK are deflated: no fd to hand
                    // out. AOSP throws the exact same typed error here.
                    return Err(exc(
                        FILE_NOT_FOUND,
                        "This file can not be opened as a file descriptor; it is probably compressed",
                    ));
                }
                ("list", "(Ljava/lang/String;)[Ljava/lang/String;") => {
                    let path = str_of(vm, args.first().unwrap_or(&Value::Null))
                        .ok_or_else(|| exc("Ljava/lang/NullPointerException;", "path == null"))?;
                    list_assets(vm, &path)?
                }
                ("close", "()V") => Value::Null,
                _ => return Ok(None),
            }));
        }
        RESOURCES => {
            return Ok(Some(match (name, sig) {
                ("getDisplayMetrics", _) => Value::Obj(metrics(vm)?),
                ("getConfiguration", _) => Value::Obj(configuration(vm)?),
                ("getAssets", "()Landroid/content/res/AssetManager;") => {
                    Value::Obj(asset_manager(vm)?)
                }
                ("getString", "(I)Ljava/lang/String;")
                | ("getText", "(I)Ljava/lang/CharSequence;") => {
                    let t = string_res(vm, int(0))?;
                    s(vm, t)?
                }
                ("getText", "(ILjava/lang/CharSequence;)Ljava/lang/CharSequence;") => {
                    match string_res(vm, int(0)) {
                        Ok(t) => s(vm, t)?,
                        Err(_) => args.get(1).cloned().unwrap_or(Value::Null),
                    }
                }
                ("getString", "(I[Ljava/lang/Object;)Ljava/lang/String;") => {
                    let t = string_res(vm, int(0))?;
                    format_string(vm, t, &args[1])?
                }
                ("getDimension", "(I)F") => {
                    let v = value_of(vm, int(0))?;
                    Value::Float(dimension_px(vm, &v, int(0))?)
                }
                ("getDimensionPixelSize", "(I)I") => {
                    // AOSP: round, and never 0 for a non-zero value
                    let v = value_of(vm, int(0))?;
                    let f = dimension_px(vm, &v, int(0))?;
                    let r = (f + 0.5).floor() as i32;
                    Value::Int(if r == 0 && f != 0.0 {
                        if f > 0.0 {
                            1
                        } else {
                            -1
                        }
                    } else {
                        r
                    })
                }
                ("getDimensionPixelOffset", "(I)I") => {
                    let v = value_of(vm, int(0))?;
                    Value::Int(dimension_px(vm, &v, int(0))? as i32)
                }
                ("getInteger", "(I)I") | ("getColor", _) => match value_of(vm, int(0))? {
                    ResolvedValue::Integer(i) => Value::Int(i),
                    _ => return Err(not_found(int(0))),
                },
                ("getBoolean", "(I)Z") => match value_of(vm, int(0))? {
                    ResolvedValue::Boolean(b) => Value::Int(b as i32),
                    ResolvedValue::Integer(i) => Value::Int((i != 0) as i32),
                    _ => return Err(not_found(int(0))),
                },
                ("getFloat", "(I)F") => match value_of(vm, int(0))? {
                    ResolvedValue::Float(f) => Value::Float(f),
                    _ => return Err(not_found(int(0))),
                },
                ("getValue", "(ILandroid/util/TypedValue;Z)V") => {
                    let v = value_of(vm, int(0))?;
                    if let Some(Value::Obj(tv)) = args.get(1) {
                        fill_typed_value(vm, *tv, int(0), &v)?;
                    }
                    Value::Null
                }
                ("getIdentifier", _) => {
                    let n = str_of(vm, &args[0]).unwrap_or_default();
                    let t = str_of(vm, args.get(1).unwrap_or(&Value::Null));
                    let p = str_of(vm, args.get(2).unwrap_or(&Value::Null));
                    Value::Int(identifier(vm, &n, t.as_deref(), p.as_deref()))
                }
                ("getResourceName", _)
                | ("getResourceEntryName", _)
                | ("getResourceTypeName", _)
                | ("getResourcePackageName", _) => {
                    let id = int(0) as u32;
                    let Some(arsc) = vm.fw.resources.as_ref().and_then(|a| a.arsc.as_ref()) else {
                        return Err(not_found(id as i32));
                    };
                    let (Some(nm), Some(p)) = (arsc.resolve_name(id), arsc.package_for(id)) else {
                        return Err(not_found(id as i32));
                    };
                    let pkg = p.name.clone();
                    let (ty, entry) = nm
                        .trim_start_matches('@')
                        .split_once('/')
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .unwrap_or_default();
                    let out = match name {
                        "getResourceName" => format!("{pkg}:{ty}/{entry}"),
                        "getResourceEntryName" => entry,
                        "getResourceTypeName" => ty,
                        _ => pkg,
                    };
                    s(vm, out)?
                }
                ("newTheme", _) => {
                    Value::Obj(vm.heap.alloc_instance(THEME.to_string(), Vec::new())?)
                }
                ("obtainAttributes", _) => {
                    let ta = typed_array(vm, args.get(1))?;
                    if let Value::Obj(t) = ta {
                        crate::xml_res::fill_typed_array(vm, t, args.first(), args.get(1));
                    }
                    ta
                }
                ("obtainTypedArray", _) => typed_array(vm, args.get(1))?,
                ("updateConfiguration", _) | ("flushLayoutCache", _) => Value::Null,
                _ => return Ok(None),
            }));
        }
        THEME => {
            return Ok(Some(match name {
                "resolveAttribute" => {
                    let style_id = theme_style_id(vm, recv);
                    let attr_id = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0) as u32;
                    let value = style_id.and_then(|style| style_attribute(vm, style, attr_id));
                    let Some(mut value) = value else {
                        return Ok(Some(Value::Int(0)));
                    };
                    let Some(Value::Obj(out)) = args.get(1) else {
                        return Err(exc(
                            "Ljava/lang/NullPointerException;",
                            "Theme.resolveAttribute outValue == null",
                        ));
                    };
                    let resolve_refs = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0) != 0;
                    if resolve_refs {
                        if let ResolvedValue::Reference(id) = &value {
                            value =
                                value_of(vm, *id as i32).unwrap_or(ResolvedValue::Reference(*id));
                        }
                    }
                    fill_typed_value(vm, *out, attr_id as i32, &value)?;
                    Value::Int(1)
                }
                "obtainStyledAttributes" => {
                    let attrs = args
                        .iter()
                        .find(|v| matches!(v, Value::Obj(a) if vm.heap.as_array_elems(*a).is_ok()))
                        .cloned();
                    let ta = typed_array(vm, attrs.as_ref())?;
                    if let Value::Obj(t) = ta {
                        if let Some(style) = theme_style_id(vm, recv) {
                            crate::xml_res::fill_theme_typed_array(vm, t, attrs.as_ref(), style);
                        }
                        crate::xml_res::fill_typed_array(vm, t, args.first(), attrs.as_ref());
                    }
                    ta
                }
                "applyStyle" => {
                    let style = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
                    let force = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) != 0;
                    if style != 0 && (force || theme_style_id(vm, recv).is_none()) {
                        put(vm, recv, "rd_theme_id", Value::Int(style))?;
                    }
                    Value::Null
                }
                "setTo" => {
                    if let Some(Value::Obj(source)) = args.first() {
                        let style = get(vm, *source, "rd_theme_id");
                        put(vm, recv, "rd_theme_id", style)?;
                    }
                    Value::Null
                }
                "rebase" => Value::Null,
                "getResources" => Value::Obj(resources(vm)?),
                "getChangingConfigurations" => Value::Int(0),
                _ => return Ok(None),
            }));
        }
        TYPED_ARRAY => {
            let def = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
            return Ok(Some(match name {
                "hasValue" | "hasValueOrEmpty" => Value::Int(0),
                "length" => get(vm, recv, "rd_len"),
                "getIndexCount" | "getChangingConfigurations" | "getType" => Value::Int(0),
                "getIndex" => Value::Int(0),
                "getResources" => Value::Obj(resources(vm)?),
                "getBoolean"
                | "getInt"
                | "getInteger"
                | "getFloat"
                | "getColor"
                | "getResourceId"
                | "getDimensionPixelSize"
                | "getDimensionPixelOffset"
                | "getDimension"
                | "getFraction"
                | "getLayoutDimension"
                | "getSourceResourceId" => {
                    // (index, default) — getFraction(index, base, pbase, default); getLayoutDimension(index, name|default)
                    let last = args.len().saturating_sub(1);
                    if name == "getLayoutDimension" && sig.contains("Ljava/lang/String;") {
                        return Err(exc(
                            "Ljava/lang/UnsupportedOperationException;",
                            "attribute not specified",
                        ));
                    }
                    def(last)
                }
                "getString"
                | "getText"
                | "getNonResourceString"
                | "getDrawable"
                | "getColorStateList"
                | "getFont"
                | "peekValue"
                | "getTextArray"
                | "getPositionDescription" => Value::Null,
                "getValue" => Value::Int(0),
                "recycle" | "close" => Value::Null,
                _ => return Ok(None),
            }));
        }
        CONFIG => {
            return Ok(Some(match (name, sig) {
                ("<init>", "()V") => Value::Null,
                ("<init>", "(Landroid/content/res/Configuration;)V") | ("setTo", _) => {
                    if let Some(Value::Obj(o)) = args.first() {
                        for f in CONFIG_FIELDS {
                            let v = get(vm, *o, f);
                            put(vm, recv, f, v)?;
                        }
                    }
                    Value::Null
                }
                ("updateFrom", _) | ("diff", _) => {
                    let Some(Value::Obj(o)) = args.first().cloned() else {
                        return Ok(Some(Value::Int(0)));
                    };
                    let mut changed = 0i32;
                    for (i, f) in CONFIG_FIELDS.iter().enumerate() {
                        let (a, b) = (get(vm, recv, f), get(vm, o, f));
                        if !crate::arrays::obj_equals(vm, &a, &b)? {
                            changed |= 1 << (i % 31);
                            if name == "updateFrom" {
                                put(vm, recv, f, b)?;
                            }
                        }
                    }
                    Value::Int(changed)
                }
                ("equals", _) => {
                    let Some(Value::Obj(o)) = args.first().cloned() else {
                        return Ok(Some(Value::Int(0)));
                    };
                    let mut eq = true;
                    for f in CONFIG_FIELDS {
                        let (a, b) = (get(vm, recv, f), get(vm, o, f));
                        if !crate::arrays::obj_equals(vm, &a, &b)? {
                            eq = false;
                            break;
                        }
                    }
                    Value::Int(eq as i32)
                }
                ("hashCode", "()I") => {
                    let mut h = 17i32;
                    for f in [
                        "fontScale",
                        "densityDpi",
                        "screenWidthDp",
                        "screenHeightDp",
                        "uiMode",
                        "orientation",
                    ] {
                        let v = match get(vm, recv, f) {
                            Value::Int(i) => i,
                            Value::Float(x) => x.to_bits() as i32,
                            _ => 0,
                        };
                        h = h.wrapping_mul(31).wrapping_add(v);
                    }
                    Value::Int(h)
                }
                ("getLocales", _) => {
                    if let Value::Obj(l) = get(vm, recv, "rd_locales") {
                        Value::Obj(l)
                    } else {
                        let loc = match get(vm, recv, "locale") {
                            Value::Obj(l) => l,
                            _ => default_locale(vm)?,
                        };
                        Value::Obj(locale_list(vm, vec![Value::Obj(loc)])?)
                    }
                }
                ("setLocale", _) => {
                    let l = args.first().cloned().unwrap_or(Value::Null);
                    put(vm, recv, "locale", l)?;
                    put(vm, recv, "rd_locales", Value::Null)?;
                    Value::Null
                }
                ("setLocales", _) => {
                    let ll = args.first().cloned().unwrap_or(Value::Null);
                    if let Value::Obj(l) = ll {
                        let first = locale_list_items(vm, l)
                            .first()
                            .cloned()
                            .unwrap_or(Value::Null);
                        put(vm, recv, "locale", first)?;
                    }
                    put(vm, recv, "rd_locales", ll)?;
                    Value::Null
                }
                ("isNightModeActive", _) => {
                    let ui = get(vm, recv, "uiMode").as_int().unwrap_or(0);
                    Value::Int(((ui & 0x30) == 0x20) as i32)
                }
                ("getLayoutDirection", _) => Value::Int(0),
                ("isScreenRound", _) | ("isScreenWideColorGamut", _) | ("isScreenHdr", _) => {
                    Value::Int(0)
                }
                ("isLayoutSizeAtLeast", _) => Value::Int((int(0) <= 2) as i32),
                _ => return Ok(None),
            }));
        }
        DM => {
            return Ok(match (name, sig) {
                ("<init>", "()V") => Some(Value::Null),
                ("setToDefaults", _) | ("setTo", _) => {
                    let src = match args.first() {
                        Some(Value::Obj(o)) => *o,
                        _ => metrics(vm)?,
                    };
                    for f in [
                        "density",
                        "scaledDensity",
                        "densityDpi",
                        "widthPixels",
                        "heightPixels",
                        "xdpi",
                        "ydpi",
                    ] {
                        let v = get(vm, src, f);
                        put(vm, recv, f, v)?;
                    }
                    Some(Value::Null)
                }
                _ => None,
            });
        }
        TYPED_VALUE => {
            return Ok(match (name, sig) {
                ("<init>", "()V") => Some(Value::Null),
                ("getDimension", "(Landroid/util/DisplayMetrics;)F") => Some(Value::Float(
                    get(vm, recv, "data").as_int().unwrap_or(0) as f32,
                )),
                ("getFloat", "()F") => Some(Value::Float(f32::from_bits(
                    get(vm, recv, "data").as_int().unwrap_or(0) as u32,
                ))),
                ("coerceToString", _) => Some(get(vm, recv, "string")),
                _ => None,
            });
        }
        LOCALE => {
            return Ok(Some(match (name, sig) {
                ("<init>", _) => {
                    let p: Vec<String> = args
                        .iter()
                        .map(|a| str_of(vm, a).unwrap_or_default())
                        .collect();
                    let (l, c, v) = (
                        p.first().cloned().unwrap_or_default(),
                        p.get(1).cloned().unwrap_or_default(),
                        p.get(2).cloned().unwrap_or_default(),
                    );
                    let (l, c) = (s(vm, l.to_lowercase())?, s(vm, c.to_uppercase())?);
                    put(vm, recv, "rd_lang", l)?;
                    put(vm, recv, "rd_country", c)?;
                    let v = s(vm, v)?;
                    put(vm, recv, "rd_variant", v)?;
                    Value::Null
                }
                ("getLanguage", _) => {
                    let (l, _, _) = locale_parts(vm, recv);
                    s(vm, l)?
                }
                ("getCountry", _) => {
                    let (_, c, _) = locale_parts(vm, recv);
                    s(vm, c)?
                }
                ("getVariant", _) => {
                    let (_, _, v) = locale_parts(vm, recv);
                    s(vm, v)?
                }
                ("toLanguageTag", _) => {
                    let t = language_tag(vm, recv);
                    s(vm, t)?
                }
                ("toString", _) => {
                    let (l, c, v) = locale_parts(vm, recv);
                    let mut t = l;
                    if !c.is_empty() || !v.is_empty() {
                        t.push('_');
                        t.push_str(&c);
                    }
                    if !v.is_empty() {
                        t.push('_');
                        t.push_str(&v);
                    }
                    s(vm, t)?
                }
                ("equals", _) => match args.first() {
                    Some(Value::Obj(o))
                        if vm.heap.class_of(*o).map(|c| c == LOCALE).unwrap_or(false) =>
                    {
                        Value::Int((locale_parts(vm, recv) == locale_parts(vm, *o)) as i32)
                    }
                    _ => Value::Int(0),
                },
                ("hashCode", _) => {
                    let t = language_tag(vm, recv);
                    Value::Int(
                        t.encode_utf16()
                            .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)),
                    )
                }
                _ => return Ok(None),
            }));
        }
        LOCALE_LIST => {
            let items = locale_list_items(vm, recv);
            return Ok(Some(match (name, sig) {
                ("<init>", "([Ljava/util/Locale;)V") => {
                    let ls = match args.first() {
                        Some(Value::Obj(a)) => vm.heap.as_array_elems(*a)?.0.to_vec(),
                        _ => Vec::new(),
                    };
                    let arr =
                        crate::arrays::new_array(vm, "[Ljava/util/Locale;", ElemKind::Obj, ls)?;
                    put(vm, recv, "rd_locales", arr)?;
                    Value::Null
                }
                ("get", "(I)Ljava/util/Locale;") => {
                    items.get(int(0) as usize).cloned().unwrap_or(Value::Null)
                }
                ("size", _) => Value::Int(items.len() as i32),
                ("isEmpty", _) => Value::Int(items.is_empty() as i32),
                ("toLanguageTags", _) => {
                    let tags: Vec<String> = items
                        .iter()
                        .filter_map(|v| {
                            if let Value::Obj(l) = v {
                                Some(language_tag(vm, *l))
                            } else {
                                None
                            }
                        })
                        .collect();
                    s(vm, tags.join(","))?
                }
                ("indexOf", _) => {
                    let mut idx = -1;
                    for (i, it) in items.iter().enumerate() {
                        if let (Value::Obj(a), Some(Value::Obj(b))) = (it, args.first()) {
                            if locale_parts(vm, *a) == locale_parts(vm, *b) {
                                idx = i as i32;
                                break;
                            }
                        }
                    }
                    Value::Int(idx)
                }
                _ => return Ok(None),
            }));
        }
        _ => {}
    }
    // View.getResources
    if name == "getResources" && sig == "()Landroid/content/res/Resources;" {
        if vm.cp.is_subtype(recv_class, "Landroid/content/Context;")
            || vm.cp.is_subtype(recv_class, "Landroid/view/View;")
            || matches!(
                vm.fw.get(recv),
                Some(crate::framework::HostObj::View { .. })
            )
        {
            return Ok(Some(Value::Obj(resources(vm)?)));
        }
        return Ok(None);
    }
    // the Context family, whatever class the method ref names
    if !matches!(
        name,
        "getTheme"
            | "setTheme"
            | "obtainStyledAttributes"
            | "getMainLooper"
            | "getBaseContext"
            | "isRestricted"
            | "getAttributionTag"
            | "getAttributionSource"
            | "getAssets"
            | "getApplicationInfo"
            | "getText"
            | "getString"
            | "getColor"
            | "getResources"
    ) || !vm.cp.is_subtype(recv_class, "Landroid/content/Context;")
    {
        return Ok(None);
    }
    Ok(Some(match (name, sig) {
        ("getTheme", _) => {
            let k = ("Lrd/Resources;".to_string(), "theme".to_string());
            let theme = match vm.statics.get(&k) {
                Some(Value::Obj(t)) => *t,
                _ => {
                    let t = vm.heap.alloc_instance(THEME.to_string(), Vec::new())?;
                    vm.statics.insert(k, Value::Obj(t));
                    t
                }
            };
            let style = theme_id_for_context(vm, recv).unwrap_or(0);
            put(vm, theme, "rd_theme_id", Value::Int(style as i32))?;
            Value::Obj(theme)
        }
        ("setTheme", _) => {
            let style = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            vm.view_props
                .insert((recv, "rd_theme_override".to_string()), Value::Int(style));
            let k = ("Lrd/Resources;".to_string(), "theme".to_string());
            if let Some(Value::Obj(theme)) = vm.statics.get(&k).cloned() {
                put(vm, theme, "rd_theme_id", Value::Int(style))?;
            }
            Value::Null
        }
        ("obtainStyledAttributes", _) => {
            let attrs = args
                .iter()
                .find(|v| matches!(v, Value::Obj(a) if vm.heap.as_array_elems(*a).is_ok()))
                .cloned();
            let ta = typed_array(vm, attrs.as_ref())?;
            if let Value::Obj(t) = ta {
                if let Some(style) = theme_id_for_context(vm, recv) {
                    crate::xml_res::fill_theme_typed_array(vm, t, attrs.as_ref(), style);
                }
                crate::xml_res::fill_typed_array(vm, t, args.first(), attrs.as_ref());
            }
            ta
        }
        ("getMainLooper", _) => Value::Obj(vm.main_looper_obj()?),
        ("getAssets", _) => Value::Obj(asset_manager(vm)?),
        // APK-Tester ladder follow-up: the short route the LadderActivity uses
        // (`ApplicationInfo appInfo = getApplicationInfo()`), same object the
        // PackageManager serves for the own package (minSdk/targetSdk come from
        // the attached APK manifest).
        ("getApplicationInfo", "()Landroid/content/pm/ApplicationInfo;") => {
            Value::Obj(crate::android_pm::context_app_info(vm)?)
        }
        // ContextWrapper with no attached base: the platform ContextImpl
        // (not a ContextWrapper, so unwrap loops terminate)
        ("getBaseContext", _) => Value::Obj(crate::intrinsics::context_impl(vm)?),
        ("isRestricted", _) => Value::Int(0),
        ("getAttributionTag", _) => Value::Null,
        ("getAttributionSource", "()Landroid/content/AttributionSource;") => {
            match get(vm, recv, "mAttributionSource") {
                value @ Value::Obj(_) => value,
                _ => {
                    let source = vm.heap.alloc_instance(
                        "Landroid/content/AttributionSource;".to_string(),
                        Vec::new(),
                    )?;
                    put(vm, recv, "mAttributionSource", Value::Obj(source))?;
                    Value::Obj(source)
                }
            }
        }
        ("getText", "(I)Ljava/lang/CharSequence;") | ("getString", "(I)Ljava/lang/String;") => {
            let t = string_res(vm, int(0))?;
            s(vm, t)?
        }
        ("getString", "(I[Ljava/lang/Object;)Ljava/lang/String;") => {
            let t = string_res(vm, int(0))?;
            format_string(vm, t, &args[1])?
        }
        ("getColor", "(I)I") => match value_of(vm, int(0))? {
            ResolvedValue::Integer(i) => Value::Int(i),
            _ => return Err(not_found(int(0))),
        },
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_tag;

    #[test]
    fn tags() {
        assert_eq!(parse_tag("pt-BR"), ("pt".into(), "BR".into()));
        assert_eq!(parse_tag("en"), ("en".into(), String::new()));
        assert_eq!(parse_tag("zh-Hant-TW"), ("zh".into(), "TW".into()));
        assert_eq!(parse_tag("es-419"), ("es".into(), "419".into()));
    }
}
