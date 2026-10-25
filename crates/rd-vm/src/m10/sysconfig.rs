//! `SYS-07`: Virtual `BatteryManager` (+ sticky `ACTION_BATTERY_CHANGED` & `IntentFilter`),
//! `PowerManager` (`WakeLock`), `UiModeManager` (dark mode), `LocaleManager`,
//! dynamic `Configuration` / `DisplayMetrics` / `Locale` updates (orientation, font scale,
//! locale), and `ClipData` / `ClipboardManager` agent controls.

use super::{
    alloc, alloc_with, array_len, array_list, bool_arg, call_dex, get_field, get_str_field,
    illegal, int_arg, obj_arg, obj_array, opt_str_arg, put_field, req_str_arg, str_val,
    system_service_singleton,
};
use crate::framework::HostObj;
use crate::{Engine, ObjRef, Value, VmExit};

const BATTERY_MANAGER: &str = "Landroid/os/BatteryManager;";
const POWER_MANAGER: &str = "Landroid/os/PowerManager;";
const WAKE_LOCK: &str = "Landroid/os/PowerManager$WakeLock;";
const UI_MODE_MANAGER: &str = "Landroid/app/UiModeManager;";
const LOCALE_MANAGER: &str = "Landroid/app/LocaleManager;";
const INTENT_CLS: &str = "Landroid/content/Intent;";
const INTENT_FILTER: &str = "Landroid/content/IntentFilter;";
const CONFIG_CLS: &str = "Landroid/content/res/Configuration;";
const DM_CLS: &str = "Landroid/util/DisplayMetrics;";
const LOCALE_CLS: &str = "Ljava/util/Locale;";
const LOCALE_LIST_CLS: &str = "Landroid/os/LocaleList;";
const CLIPBOARD_MANAGER: &str = "Landroid/content/ClipboardManager;";
const CLIP_DATA: &str = "Landroid/content/ClipData;";
const CLIP_ITEM: &str = "Landroid/content/ClipData$Item;";
const CLIP_DESC: &str = "Landroid/content/ClipDescription;";

const ACTION_BATTERY_CHANGED: &str = "android.intent.action.BATTERY_CHANGED";

#[derive(Debug, Clone)]
pub struct BatteryState {
    pub level: i32,                // 0..=100
    pub scale: i32,                // 100
    pub charging: bool,            // true => STATUS_CHARGING(2) or FULL(5); false => DISCHARGING(3)
    pub plugged: i32,              // 0=unplugged, 1=AC, 2=USB, 4=Wireless
    pub health: i32,               // 2=BATTERY_HEALTH_GOOD
    pub temperature_tenths_c: i32, // 250 => 25.0 C
}

impl Default for BatteryState {
    fn default() -> Self {
        Self {
            level: 85,
            scale: 100,
            charging: true,
            plugged: 2,
            health: 2,
            temperature_tenths_c: 250,
        }
    }
}

impl BatteryState {
    pub fn status_code(&self) -> i32 {
        if self.charging {
            if self.level >= 100 {
                5 // BATTERY_STATUS_FULL
            } else {
                2 // BATTERY_STATUS_CHARGING
            }
        } else {
            3 // BATTERY_STATUS_DISCHARGING
        }
    }
}

#[derive(Debug, Clone)]
pub struct WakeLockEntry {
    pub tag: String,
    pub flags: i32,
    pub ref_counted: bool,
    pub count: u32,
    pub held: bool,
}

#[derive(Debug, Clone)]
pub struct PowerState {
    pub interactive: bool,
    pub power_save_mode: bool,
    pub ignoring_battery_optimizations: bool,
}

impl Default for PowerState {
    fn default() -> Self {
        Self {
            interactive: true,
            power_save_mode: false,
            ignoring_battery_optimizations: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemConfigState {
    pub dark_mode: bool,
    pub locale_tag: String,
    pub font_scale: f32,
    pub orientation: i32, // 1 = PORTRAIT, 2 = LANDSCAPE
    pub width_px: i32,
    pub height_px: i32,
    pub dpi: i32,
    pub clipboard_listeners: Vec<ObjRef>,
}

impl Default for SystemConfigState {
    fn default() -> Self {
        Self {
            dark_mode: false,
            locale_tag: "en-US".into(),
            font_scale: 1.0,
            orientation: 1,
            width_px: 720,
            height_px: 1440,
            dpi: 280,
            clipboard_listeners: Vec::new(),
        }
    }
}

pub(crate) fn build_battery_intent(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let bat = vm.fw.vdev.battery.clone();
    let intent = alloc(vm, INTENT_CLS)?;
    vm.attach_host_state(INTENT_CLS, intent)?;
    vm.fw.m8.intents.entry(intent).or_default().action = Some(ACTION_BATTERY_CHANGED.into());
    if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&intent) {
        extras.push(("level".into(), Value::Int(bat.level)));
        extras.push(("scale".into(), Value::Int(bat.scale)));
        extras.push(("status".into(), Value::Int(bat.status_code())));
        extras.push(("plugged".into(), Value::Int(bat.plugged)));
        extras.push(("health".into(), Value::Int(bat.health)));
        extras.push(("temperature".into(), Value::Int(bat.temperature_tenths_c)));
        extras.push(("present".into(), Value::Int(1)));
    }
    Ok(intent)
}

pub(super) fn maybe_sticky_battery_receiver(
    vm: &mut Engine,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Sticky lookup: registerReceiver(null, filter) or non-null receiver matching ACTION_BATTERY_CHANGED
    let recv_arg = obj_arg(args, 0);
    let filter_ref = obj_arg(args, 1);
    let matches_battery = match filter_ref {
        Some(f) => filter_has_action(vm, f, ACTION_BATTERY_CHANGED),
        None => false,
    };
    if !matches_battery {
        return Ok(None);
    }
    if let Some(r) = recv_arg {
        if !vm.fw.m8.receivers.contains(&r) {
            vm.fw.m8.receivers.push(r);
        }
    }
    let sticky = build_battery_intent(vm)?;
    Ok(Some(Value::Obj(sticky)))
}

fn filter_has_action(vm: &Engine, filter: ObjRef, target: &str) -> bool {
    if let Some(Value::Obj(list_ref)) = get_field(vm, filter, "rd_actions") {
        if let Some(HostObj::List { elems }) = vm.fw.objects.get(&list_ref) {
            return elems.iter().any(|v| match v {
                Value::Obj(s) => vm.heap.as_str(*s).ok() == Some(target),
                _ => false,
            });
        }
    }
    false
}

fn ensure_resources_singletons(vm: &mut Engine) -> Result<(ObjRef, ObjRef), VmExit> {
    let sys_res = crate::android_res::static_intrinsic(
        vm,
        "Landroid/content/res/Resources;",
        "getSystem",
        "()Landroid/content/res/Resources;",
        &[],
    )?;
    if let Some(Value::Obj(res_ref)) = sys_res {
        let _ = crate::android_res::instance_intrinsic(
            vm,
            "Landroid/content/res/Resources;",
            "Landroid/content/res/Resources;",
            "getConfiguration",
            "()Landroid/content/res/Configuration;",
            res_ref,
            &[],
        )?;
        let _ = crate::android_res::instance_intrinsic(
            vm,
            "Landroid/content/res/Resources;",
            "Landroid/content/res/Resources;",
            "getDisplayMetrics",
            "()Landroid/util/DisplayMetrics;",
            res_ref,
            &[],
        )?;
    }
    let cfg = match vm
        .statics
        .get(&("Lrd/Resources;".to_string(), "configuration".to_string()))
    {
        Some(Value::Obj(r)) => *r,
        _ => {
            let r = alloc(vm, CONFIG_CLS)?;
            vm.statics.insert(
                ("Lrd/Resources;".to_string(), "configuration".to_string()),
                Value::Obj(r),
            );
            r
        }
    };
    let dm = match vm
        .statics
        .get(&("Lrd/Resources;".to_string(), "metrics".to_string()))
    {
        Some(Value::Obj(r)) => *r,
        _ => {
            let r = alloc(vm, DM_CLS)?;
            vm.statics.insert(
                ("Lrd/Resources;".to_string(), "metrics".to_string()),
                Value::Obj(r),
            );
            r
        }
    };
    Ok((cfg, dm))
}

fn apply_sysconfig_to_vm(vm: &mut Engine, notify_activity: bool) -> Result<(), VmExit> {
    let (cfg, dm) = ensure_resources_singletons(vm)?;
    let st = vm.fw.vdev.sysconfig.clone();

    let (lang, country) = parse_locale_tag(&st.locale_tag);
    let l_val = str_val(vm, lang.to_lowercase())?;
    let c_val = str_val(vm, country.to_uppercase())?;
    let v_val = str_val(vm, "")?;
    let loc_obj = alloc_with(
        vm,
        LOCALE_CLS,
        vec![
            ("rd_lang".into(), l_val),
            ("rd_country".into(), c_val),
            ("rd_variant".into(), v_val),
        ],
    )?;
    vm.statics.insert(
        ("Lrd/Resources;".to_string(), "locale".to_string()),
        Value::Obj(loc_obj),
    );
    let loc_arr = obj_array(vm, "[Ljava/util/Locale;", vec![Value::Obj(loc_obj)])?;
    let loc_list = alloc_with(vm, LOCALE_LIST_CLS, vec![("rd_locales".into(), loc_arr)])?;
    vm.statics.insert(
        ("Lrd/Resources;".to_string(), "locale_list".to_string()),
        Value::Obj(loc_list),
    );

    let density = st.dpi as f32 / 160.0;
    let wdp = (st.width_px as f32 / density) as i32;
    let hdp = (st.height_px as f32 / density) as i32;
    put_field(vm, cfg, "fontScale", Value::Float(st.font_scale))?;
    put_field(vm, cfg, "densityDpi", Value::Int(st.dpi))?;
    put_field(vm, cfg, "screenWidthDp", Value::Int(wdp))?;
    put_field(vm, cfg, "screenHeightDp", Value::Int(hdp))?;
    put_field(vm, cfg, "smallestScreenWidthDp", Value::Int(wdp.min(hdp)))?;
    put_field(vm, cfg, "orientation", Value::Int(st.orientation))?;
    put_field(
        vm,
        cfg,
        "uiMode",
        Value::Int(0x01 | if st.dark_mode { 0x20 } else { 0x10 }),
    )?;
    put_field(vm, cfg, "locale", Value::Obj(loc_obj))?;

    put_field(vm, dm, "density", Value::Float(density))?;
    put_field(
        vm,
        dm,
        "scaledDensity",
        Value::Float(density * st.font_scale),
    )?;
    put_field(vm, dm, "densityDpi", Value::Int(st.dpi))?;
    put_field(vm, dm, "widthPixels", Value::Int(st.width_px))?;
    put_field(vm, dm, "heightPixels", Value::Int(st.height_px))?;
    put_field(vm, dm, "xdpi", Value::Float(st.dpi as f32))?;
    put_field(vm, dm, "ydpi", Value::Float(st.dpi as f32))?;

    if notify_activity {
        if let Some(act) = vm.fw.current_activity {
            let _ = call_dex(
                vm,
                act,
                "onConfigurationChanged",
                "(Landroid/content/res/Configuration;)V",
                vec![Value::Obj(cfg)],
            )?;
        }
    }
    Ok(())
}

fn parse_locale_tag(tag: &str) -> (String, String) {
    let mut parts = tag.split(['-', '_']);
    let lang = parts.next().unwrap_or("en").to_string();
    let country = parts
        .find(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())))
        .unwrap_or("")
        .to_string();
    (lang, country)
}

fn build_plain_clip_data(vm: &mut Engine, label: &str, text: &str) -> Result<ObjRef, VmExit> {
    let lbl = str_val(vm, label)?;
    let txt = str_val(vm, text)?;
    let mime = str_val(vm, "text/plain")?;
    let mimes = obj_array(vm, "[Ljava/lang/String;", vec![mime])?;
    let desc = alloc_with(
        vm,
        CLIP_DESC,
        vec![("rd_label".into(), lbl), ("rd_mimes".into(), mimes)],
    )?;
    let item = alloc_with(vm, CLIP_ITEM, vec![("rd_text".into(), txt)])?;
    let items = array_list(vm, vec![Value::Obj(item)])?;
    alloc_with(
        vm,
        CLIP_DATA,
        vec![
            ("rd_desc".into(), Value::Obj(desc)),
            ("rd_items".into(), items),
        ],
    )
}

pub(super) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (INTENT_CLS, "ACTION_BATTERY_CHANGED") => str_val(vm, ACTION_BATTERY_CHANGED).map(Some),
        (INTENT_CLS, "ACTION_POWER_CONNECTED") => {
            str_val(vm, "android.intent.action.ACTION_POWER_CONNECTED").map(Some)
        }
        (INTENT_CLS, "ACTION_POWER_DISCONNECTED") => {
            str_val(vm, "android.intent.action.ACTION_POWER_DISCONNECTED").map(Some)
        }
        (BATTERY_MANAGER, "EXTRA_LEVEL") => str_val(vm, "level").map(Some),
        (BATTERY_MANAGER, "EXTRA_SCALE") => str_val(vm, "scale").map(Some),
        (BATTERY_MANAGER, "EXTRA_STATUS") => str_val(vm, "status").map(Some),
        (BATTERY_MANAGER, "EXTRA_PLUGGED") => str_val(vm, "plugged").map(Some),
        (BATTERY_MANAGER, "EXTRA_HEALTH") => str_val(vm, "health").map(Some),
        (BATTERY_MANAGER, "EXTRA_TEMPERATURE") => str_val(vm, "temperature").map(Some),
        (BATTERY_MANAGER, "EXTRA_PRESENT") => str_val(vm, "present").map(Some),
        (BATTERY_MANAGER, "BATTERY_PROPERTY_CHARGE_COUNTER") => Ok(Some(Value::Int(1))),
        (BATTERY_MANAGER, "BATTERY_PROPERTY_CURRENT_NOW") => Ok(Some(Value::Int(2))),
        (BATTERY_MANAGER, "BATTERY_PROPERTY_CURRENT_AVERAGE") => Ok(Some(Value::Int(3))),
        (BATTERY_MANAGER, "BATTERY_PROPERTY_CAPACITY") => Ok(Some(Value::Int(4))),
        (BATTERY_MANAGER, "BATTERY_PROPERTY_STATUS") => Ok(Some(Value::Int(6))),
        (BATTERY_MANAGER, "BATTERY_STATUS_UNKNOWN") => Ok(Some(Value::Int(1))),
        (BATTERY_MANAGER, "BATTERY_STATUS_CHARGING") => Ok(Some(Value::Int(2))),
        (BATTERY_MANAGER, "BATTERY_STATUS_DISCHARGING") => Ok(Some(Value::Int(3))),
        (BATTERY_MANAGER, "BATTERY_STATUS_NOT_CHARGING") => Ok(Some(Value::Int(4))),
        (BATTERY_MANAGER, "BATTERY_STATUS_FULL") => Ok(Some(Value::Int(5))),
        (BATTERY_MANAGER, "BATTERY_PLUGGED_AC") => Ok(Some(Value::Int(1))),
        (BATTERY_MANAGER, "BATTERY_PLUGGED_USB") => Ok(Some(Value::Int(2))),
        (BATTERY_MANAGER, "BATTERY_PLUGGED_WIRELESS") => Ok(Some(Value::Int(4))),
        (BATTERY_MANAGER, "BATTERY_HEALTH_GOOD") => Ok(Some(Value::Int(2))),
        (POWER_MANAGER, "PARTIAL_WAKE_LOCK") => Ok(Some(Value::Int(1))),
        (UI_MODE_MANAGER, "MODE_NIGHT_AUTO") => Ok(Some(Value::Int(0))),
        (UI_MODE_MANAGER, "MODE_NIGHT_NO") => Ok(Some(Value::Int(1))),
        (UI_MODE_MANAGER, "MODE_NIGHT_YES") => Ok(Some(Value::Int(2))),
        (CONFIG_CLS, "ORIENTATION_PORTRAIT") => Ok(Some(Value::Int(1))),
        (CONFIG_CLS, "ORIENTATION_LANDSCAPE") => Ok(Some(Value::Int(2))),
        (CONFIG_CLS, "UI_MODE_NIGHT_NO") => Ok(Some(Value::Int(0x10))),
        (CONFIG_CLS, "UI_MODE_NIGHT_YES") => Ok(Some(Value::Int(0x20))),
        (CONFIG_CLS, "UI_MODE_NIGHT_MASK") => Ok(Some(Value::Int(0x30))),
        (CLIP_DESC, "MIMETYPE_TEXT_PLAIN") => str_val(vm, "text/plain").map(Some),
        (CLIP_DESC, "MIMETYPE_TEXT_HTML") => str_val(vm, "text/html").map(Some),
        _ => Ok(None),
    }
}

pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (CLIP_DATA, "newPlainText") => {
            let label = opt_str_arg(vm, args, 0)?.unwrap_or_default();
            let text = opt_str_arg(vm, args, 1)?.unwrap_or_default();
            Ok(Some(Value::Obj(build_plain_clip_data(vm, &label, &text)?)))
        }
        (CLIP_DATA, "newRawUri") => {
            let label = opt_str_arg(vm, args, 0)?.unwrap_or_default();
            let uri = args.get(1).cloned().unwrap_or(Value::Null);
            let clip = build_plain_clip_data(vm, &label, "")?;
            if let Some(Value::Obj(items_ref)) = get_field(vm, clip, "rd_items") {
                if let Some(HostObj::List { elems }) = vm.fw.objects.get(&items_ref).cloned() {
                    if let Some(Value::Obj(item)) = elems.first() {
                        put_field(vm, *item, "rd_uri", uri)?;
                    }
                }
            }
            Ok(Some(Value::Obj(clip)))
        }
        _ => Ok(None),
    }
}

pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        INTENT_FILTER => match name {
            "<init>" => {
                let mut initial = Vec::new();
                if let Some(act) = opt_str_arg(vm, args, 0)? {
                    initial.push(str_val(vm, act)?);
                }
                let list = array_list(vm, initial)?;
                put_field(vm, recv, "rd_actions", list)?;
                Ok(Some(Value::Null))
            }
            "addAction" => {
                let act = req_str_arg(vm, args, 0)?;
                let av = str_val(vm, act)?;
                if let Some(Value::Obj(list_ref)) = get_field(vm, recv, "rd_actions") {
                    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list_ref) {
                        elems.push(av);
                    }
                } else {
                    let list = array_list(vm, vec![av])?;
                    put_field(vm, recv, "rd_actions", list)?;
                }
                Ok(Some(Value::Null))
            }
            "hasAction" => {
                let ok = opt_str_arg(vm, args, 0)?
                    .map(|a| filter_has_action(vm, recv, &a))
                    .unwrap_or(false);
                Ok(Some(Value::Int(i32::from(ok))))
            }
            "countActions" => {
                let n = match get_field(vm, recv, "rd_actions") {
                    Some(Value::Obj(list_ref)) => match vm.fw.objects.get(&list_ref) {
                        Some(HostObj::List { elems }) => elems.len() as i32,
                        _ => 0,
                    },
                    _ => 0,
                };
                Ok(Some(Value::Int(n)))
            }
            "getAction" => {
                let idx = int_arg(args, 0) as usize;
                let v = match get_field(vm, recv, "rd_actions") {
                    Some(Value::Obj(list_ref)) => match vm.fw.objects.get(&list_ref) {
                        Some(HostObj::List { elems }) => {
                            elems.get(idx).cloned().unwrap_or(Value::Null)
                        }
                        _ => Value::Null,
                    },
                    _ => Value::Null,
                };
                Ok(Some(v))
            }
            "addCategory" | "addDataScheme" | "addDataType" | "setPriority" => {
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        BATTERY_MANAGER => match name {
            "getIntProperty" => {
                let prop = int_arg(args, 0);
                let bat = &vm.fw.vdev.battery;
                let v = match prop {
                    1 => bat.level * 30_000, // uAh charge counter
                    2 | 3 => {
                        if bat.charging {
                            500_000
                        } else {
                            -250_000
                        }
                    }
                    4 => bat.level,
                    6 => bat.status_code(),
                    _ => i32::MIN,
                };
                Ok(Some(Value::Int(v)))
            }
            "getLongProperty" => {
                let prop = int_arg(args, 0);
                let v = if prop == 4 {
                    i64::from(vm.fw.vdev.battery.level)
                } else {
                    0
                };
                Ok(Some(Value::Long(v)))
            }
            "isCharging" => Ok(Some(Value::Int(i32::from(vm.fw.vdev.battery.charging)))),
            "computeChargeTimeRemaining" => Ok(Some(Value::Long(if vm.fw.vdev.battery.charging {
                i64::from(100 - vm.fw.vdev.battery.level) * 60_000
            } else {
                -1
            }))),
            _ => Ok(None),
        },
        POWER_MANAGER => match name {
            "isInteractive" | "isScreenOn" => {
                Ok(Some(Value::Int(i32::from(vm.fw.vdev.power.interactive))))
            }
            "isPowerSaveMode" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.power.power_save_mode,
            )))),
            "isDeviceIdleMode" => Ok(Some(Value::Int(0))),
            "isIgnoringBatteryOptimizations" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.power.ignoring_battery_optimizations,
            )))),
            "newWakeLock" => {
                let flags = int_arg(args, 0);
                let tag = req_str_arg(vm, args, 1)?;
                let wl = alloc(vm, WAKE_LOCK)?;
                vm.fw.vdev.wake_locks.insert(
                    wl,
                    WakeLockEntry {
                        tag,
                        flags,
                        ref_counted: true,
                        count: 0,
                        held: false,
                    },
                );
                Ok(Some(Value::Obj(wl)))
            }
            _ => Ok(None),
        },
        WAKE_LOCK => match name {
            "acquire" => {
                if let Some(wl) = vm.fw.vdev.wake_locks.get_mut(&recv) {
                    if wl.ref_counted {
                        wl.count += 1;
                    } else {
                        wl.count = 1;
                    }
                    wl.held = true;
                }
                Ok(Some(Value::Null))
            }
            "release" => {
                if let Some(wl) = vm.fw.vdev.wake_locks.get_mut(&recv) {
                    if wl.ref_counted {
                        if wl.count == 0 {
                            return Err(super::error(
                                "Ljava/lang/RuntimeException;",
                                "WakeLock under-locked",
                            ));
                        }
                        wl.count -= 1;
                        wl.held = wl.count > 0;
                    } else {
                        wl.count = 0;
                        wl.held = false;
                    }
                }
                Ok(Some(Value::Null))
            }
            "isHeld" => Ok(Some(Value::Int(i32::from(
                vm.fw
                    .vdev
                    .wake_locks
                    .get(&recv)
                    .map(|w| w.held)
                    .unwrap_or(false),
            )))),
            "setReferenceCounted" => {
                if let Some(wl) = vm.fw.vdev.wake_locks.get_mut(&recv) {
                    wl.ref_counted = bool_arg(args, 0);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        UI_MODE_MANAGER => match name {
            "getNightMode" => Ok(Some(Value::Int(if vm.fw.vdev.sysconfig.dark_mode {
                2
            } else {
                1
            }))),
            "setNightMode" | "setApplicationNightMode" => {
                let mode = int_arg(args, 0);
                vm.fw.vdev.sysconfig.dark_mode = mode == 2;
                apply_sysconfig_to_vm(vm, true)?;
                Ok(Some(Value::Null))
            }
            "getCurrentModeType" => Ok(Some(Value::Int(1))), // UI_MODE_TYPE_NORMAL
            _ => Ok(None),
        },
        LOCALE_MANAGER => match name {
            "getApplicationLocales" | "getSystemLocales" => {
                apply_sysconfig_to_vm(vm, false)?;
                let ll = vm
                    .statics
                    .get(&("Lrd/Resources;".to_string(), "locale_list".to_string()))
                    .cloned()
                    .unwrap_or(Value::Null);
                Ok(Some(ll))
            }
            "setApplicationLocales" => {
                if let Some(ll) = obj_arg(args, 0) {
                    if let Some(Value::Obj(arr)) = get_field(vm, ll, "rd_locales") {
                        if let Ok((elems, _)) = vm.heap.as_array_elems(arr) {
                            if let Some(Value::Obj(loc)) = elems.first() {
                                let l = get_str_field(vm, *loc, "rd_lang")
                                    .unwrap_or_else(|| "en".into());
                                let c = get_str_field(vm, *loc, "rd_country").unwrap_or_default();
                                vm.fw.vdev.sysconfig.locale_tag =
                                    if c.is_empty() { l } else { format!("{l}-{c}") };
                                apply_sysconfig_to_vm(vm, true)?;
                            }
                        }
                    }
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        CLIPBOARD_MANAGER => match name {
            "addPrimaryClipChangedListener" => {
                if let Some(l) = obj_arg(args, 0) {
                    if !vm.fw.vdev.sysconfig.clipboard_listeners.contains(&l) {
                        vm.fw.vdev.sysconfig.clipboard_listeners.push(l);
                    }
                }
                Ok(Some(Value::Null))
            }
            "removePrimaryClipChangedListener" => {
                if let Some(l) = obj_arg(args, 0) {
                    vm.fw.vdev.sysconfig.clipboard_listeners.retain(|x| *x != l);
                }
                Ok(Some(Value::Null))
            }
            "setPrimaryClip" => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_primary_clip", v)?;
                let listeners = vm.fw.vdev.sysconfig.clipboard_listeners.clone();
                for l in listeners {
                    let _ = call_dex(vm, l, "onPrimaryClipChanged", "()V", Vec::new())?;
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None), // getPrimaryClip/hasPrimaryClip/clearPrimaryClip fall through to m9::ime
        },
        CLIP_DATA => match name {
            "<init>" => {
                // (CharSequence label, String[] mimeTypes, Item item)
                let label = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                let lbl = str_val(vm, label)?;
                let mimes = args.get(1).cloned().unwrap_or(Value::Null);
                let desc = alloc_with(
                    vm,
                    CLIP_DESC,
                    vec![("rd_label".into(), lbl), ("rd_mimes".into(), mimes)],
                )?;
                let first_item = args.get(2).cloned().unwrap_or(Value::Null);
                let items = array_list(vm, vec![first_item])?;
                put_field(vm, recv, "rd_desc", Value::Obj(desc))?;
                put_field(vm, recv, "rd_items", items)?;
                Ok(Some(Value::Null))
            }
            "getDescription" => Ok(Some(get_field(vm, recv, "rd_desc").unwrap_or(Value::Null))),
            "getItemCount" => {
                let n = match get_field(vm, recv, "rd_items") {
                    Some(Value::Obj(list_ref)) => match vm.fw.objects.get(&list_ref) {
                        Some(HostObj::List { elems }) => elems.len() as i32,
                        _ => 0,
                    },
                    _ => 0,
                };
                Ok(Some(Value::Int(n)))
            }
            "getItemAt" => {
                let idx = int_arg(args, 0) as usize;
                let item = match get_field(vm, recv, "rd_items") {
                    Some(Value::Obj(list_ref)) => match vm.fw.objects.get(&list_ref) {
                        Some(HostObj::List { elems }) => {
                            elems.get(idx).cloned().unwrap_or(Value::Null)
                        }
                        _ => Value::Null,
                    },
                    _ => Value::Null,
                };
                Ok(Some(item))
            }
            "addItem" => {
                let item = args.first().cloned().unwrap_or(Value::Null);
                if let Some(Value::Obj(list_ref)) = get_field(vm, recv, "rd_items") {
                    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list_ref) {
                        elems.push(item);
                    }
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        CLIP_ITEM => match name {
            "<init>" => {
                let t = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_text", t)?;
                Ok(Some(Value::Null))
            }
            "getText" | "coerceToText" => {
                Ok(Some(get_field(vm, recv, "rd_text").unwrap_or(Value::Null)))
            }
            "getUri" => Ok(Some(get_field(vm, recv, "rd_uri").unwrap_or(Value::Null))),
            _ => Ok(None),
        },
        CLIP_DESC => match name {
            "getLabel" => Ok(Some(get_field(vm, recv, "rd_label").unwrap_or(Value::Null))),
            "getMimeTypeCount" => {
                let n = match get_field(vm, recv, "rd_mimes") {
                    Some(Value::Obj(arr)) => array_len(vm, arr).unwrap_or(0) as i32,
                    _ => 0,
                };
                Ok(Some(Value::Int(n)))
            }
            "getMimeType" => {
                let idx = int_arg(args, 0) as usize;
                let m = match get_field(vm, recv, "rd_mimes") {
                    Some(Value::Obj(arr)) => vm
                        .heap
                        .as_array_elems(arr)
                        .ok()
                        .and_then(|(e, _)| e.get(idx).cloned())
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                Ok(Some(m))
            }
            "hasMimeType" => {
                let target = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                let has = match get_field(vm, recv, "rd_mimes") {
                    Some(Value::Obj(arr)) => vm
                        .heap
                        .as_array_elems(arr)
                        .ok()
                        .map(|(elems, _)| {
                            elems.iter().any(|v| match v {
                                Value::Obj(s) => {
                                    vm.heap.as_str(*s).ok() == Some(target.as_str())
                                        || target == "*/*"
                                }
                                _ => false,
                            })
                        })
                        .unwrap_or(false),
                    _ => false,
                };
                Ok(Some(Value::Int(i32::from(has))))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

impl Engine {
    /// Update virtual battery state and broadcast `ACTION_BATTERY_CHANGED` to registered receivers.
    pub fn m10_set_battery(
        &mut self,
        level: i32,
        charging: bool,
        plugged: Option<i32>,
        temperature_tenths_c: Option<i32>,
        power_save_mode: Option<bool>,
    ) -> Result<usize, VmExit> {
        if !(0..=100).contains(&level) {
            return Err(illegal("battery level must be in 0..=100"));
        }
        self.fw.vdev.battery.level = level;
        self.fw.vdev.battery.charging = charging;
        self.fw.vdev.battery.plugged = plugged.unwrap_or(if charging { 2 } else { 0 });
        if let Some(t) = temperature_tenths_c {
            self.fw.vdev.battery.temperature_tenths_c = t;
        }
        if let Some(ps) = power_save_mode {
            self.fw.vdev.power.power_save_mode = ps;
        }
        let receivers = self.fw.m8.receivers.clone();
        if receivers.is_empty() {
            return Ok(0);
        }
        self.with_host_roots(|vm| {
            let intent = build_battery_intent(vm)?;
            let ctx = vm
                .fw
                .current_activity
                .map(Value::Obj)
                .unwrap_or(Value::Null);
            let mut fired = 0usize;
            for r in receivers {
                if call_dex(
                    vm,
                    r,
                    "onReceive",
                    "(Landroid/content/Context;Landroid/content/Intent;)V",
                    vec![ctx.clone(), Value::Obj(intent)],
                )?
                .is_some()
                {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }

    pub fn m10_set_dark_mode(&mut self, enabled: bool) -> Result<(), VmExit> {
        self.fw.vdev.sysconfig.dark_mode = enabled;
        self.with_host_roots(|vm| apply_sysconfig_to_vm(vm, true))
    }

    pub fn m10_set_locale(&mut self, language_tag: &str) -> Result<(), VmExit> {
        if language_tag.trim().is_empty() {
            return Err(illegal("locale tag must not be empty"));
        }
        self.fw.vdev.sysconfig.locale_tag = language_tag.trim().to_string();
        self.with_host_roots(|vm| apply_sysconfig_to_vm(vm, true))
    }

    pub fn m10_set_font_scale(&mut self, scale: f32) -> Result<(), VmExit> {
        if !(0.5..=3.0).contains(&scale) {
            return Err(illegal("font_scale must be in 0.5..=3.0"));
        }
        self.fw.vdev.sysconfig.font_scale = scale;
        self.with_host_roots(|vm| apply_sysconfig_to_vm(vm, true))
    }

    pub fn m10_set_orientation(&mut self, orientation: &str) -> Result<(), VmExit> {
        let code = match orientation.trim().to_ascii_lowercase().as_str() {
            "portrait" | "1" => 1,
            "landscape" | "2" => 2,
            _ => return Err(illegal("orientation must be 'portrait' or 'landscape'")),
        };
        let (min_d, max_d) = {
            let a = self.fw.vdev.sysconfig.width_px;
            let b = self.fw.vdev.sysconfig.height_px;
            (a.min(b), a.max(b))
        };
        self.fw.vdev.sysconfig.orientation = code;
        if code == 1 {
            self.fw.vdev.sysconfig.width_px = min_d;
            self.fw.vdev.sysconfig.height_px = max_d;
        } else {
            self.fw.vdev.sysconfig.width_px = max_d;
            self.fw.vdev.sysconfig.height_px = min_d;
        }
        self.with_host_roots(|vm| apply_sysconfig_to_vm(vm, true))
    }

    pub fn m10_set_clipboard(&mut self, label: &str, text: &str) -> Result<(), VmExit> {
        let lbl = label.to_string();
        let txt = text.to_string();
        self.with_host_roots(|vm| {
            let mgr = system_service_singleton(vm, CLIPBOARD_MANAGER)?;
            let clip = build_plain_clip_data(vm, &lbl, &txt)?;
            put_field(vm, mgr, "rd_primary_clip", Value::Obj(clip))?;
            let listeners = vm.fw.vdev.sysconfig.clipboard_listeners.clone();
            for l in listeners {
                let _ = call_dex(vm, l, "onPrimaryClipChanged", "()V", Vec::new())?;
            }
            Ok(())
        })
    }

    pub fn m10_get_clipboard(&mut self) -> Result<Option<String>, VmExit> {
        self.with_host_roots(|vm| {
            let mgr = system_service_singleton(vm, CLIPBOARD_MANAGER)?;
            let Some(Value::Obj(clip)) = get_field(vm, mgr, "rd_primary_clip") else {
                return Ok(None);
            };
            if clip == 0 {
                return Ok(None);
            }
            let Some(Value::Obj(list_ref)) = get_field(vm, clip, "rd_items") else {
                return Ok(None);
            };
            let first = match vm.fw.objects.get(&list_ref) {
                Some(HostObj::List { elems }) => elems.first().cloned(),
                _ => None,
            };
            let Some(Value::Obj(item)) = first else {
                return Ok(None);
            };
            Ok(get_str_field(vm, item, "rd_text"))
        })
    }
}
