//! `android.view.Display`, `android.view.Display$Mode`, and
//! `android.hardware.display.DisplayManager` for the emulated device's
//! single built-in display.
//!
//! - **Getters.** `View.getDisplay()`, `Context.getDisplay()`,
//!   `WindowManager.getDefaultDisplay()`, and `DisplayManager.getDisplay(0)`
//!   all return the default display (`Display.DEFAULT_DISPLAY == 0`).
//!   `DisplayManager.getDisplays()` and `getDisplays(null)` return a
//!   1-element `Display[]` containing that display; non-null categories (such
//!   as `DISPLAY_CATEGORY_PRESENTATION`) return an empty `Display[0]`.
//! - **Listeners.** `DisplayManager.registerDisplayListener` and
//!   `unregisterDisplayListener` follow AOSP `DisplayManagerGlobal` contracts:
//!   `null` listeners throw `IllegalArgumentException("listener must not be null")`,
//!   a zero event mask throws
//!   `IllegalArgumentException("The set of events to listen to must not be empty.")`,
//!   duplicate registrations deduplicate by listener identity (updating the
//!   mask/handler), and registered delegates live in a heap array field on the
//!   `DisplayManager` instance so GC traces them naturally without touching
//!   `HostState`.
//! - **Geometry & refresh rate** follow `android_res::device()` (720×1440 at
//!   280 dpi by default, overridable via `RD_SCREEN_W`/`RD_SCREEN_H`/
//!   `RD_DENSITY_DPI`) at 60 Hz (`Display.Mode` id `1`, `STATE_ON`,
//!   `"Built-in Screen"`), unblocking Flutter's `VsyncWaiter` and Jetpack
//!   display queries.

use crate::engine::Engine;
use crate::err::{oom_throwable, Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

pub(crate) const DISPLAY: &str = "Landroid/view/Display;";
pub(crate) const DISPLAY_MODE: &str = "Landroid/view/Display$Mode;";
pub(crate) const DISPLAY_MANAGER: &str = "Landroid/hardware/display/DisplayManager;";
const LISTENER_DELEGATE: &str =
    "Landroid/hardware/display/DisplayManagerGlobal$DisplayListenerDelegate;";

pub const EVENT_FLAG_DISPLAY_ADDED: i64 = 1 << 0;
pub const EVENT_FLAG_DISPLAY_REMOVED: i64 = 1 << 1;
pub const EVENT_FLAG_DISPLAY_CHANGED: i64 = 1 << 2;
const DEFAULT_EVENTS_MASK: i64 =
    EVENT_FLAG_DISPLAY_ADDED | EVENT_FLAG_DISPLAY_REMOVED | EVENT_FLAG_DISPLAY_CHANGED;

fn iae(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/IllegalArgumentException;", msg))
}

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

fn get(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}

fn alloc_display(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    vm.heap
        .alloc_instance(DISPLAY.to_string(), Vec::new())
        .map_err(oom_throwable)
}

fn alloc_default_mode(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let d = crate::android_res::device();
    let mode = vm
        .heap
        .alloc_instance(
            DISPLAY_MODE.to_string(),
            vec![
                ("mModeId".to_string(), Value::Int(1)),
                ("mWidth".to_string(), Value::Int(d.w as i32)),
                ("mHeight".to_string(), Value::Int(d.h as i32)),
                ("mRefreshRate".to_string(), Value::Float(60.0)),
            ],
        )
        .map_err(oom_throwable)?;
    Ok(mode)
}

fn alloc_obj_array(vm: &mut Engine, elem_desc: &str, items: Vec<Value>) -> Result<ObjRef, VmExit> {
    let arr = vm
        .heap
        .alloc_array(ElemKind::Obj, items.len(), elem_desc)
        .map_err(oom_throwable)?;
    if let Ok(elems) = vm.heap.array_elems_mut(arr) {
        elems.extend(items);
    }
    Ok(arr)
}

fn alloc_float_array(vm: &mut Engine, items: &[f32]) -> Result<ObjRef, VmExit> {
    let arr = vm
        .heap
        .alloc_array(ElemKind::Float, items.len(), "[F")
        .map_err(oom_throwable)?;
    if let Ok(elems) = vm.heap.array_elems_mut(arr) {
        elems.extend(items.iter().copied().map(Value::Float));
    }
    Ok(arr)
}

fn alloc_int_array(vm: &mut Engine, items: &[i32]) -> Result<ObjRef, VmExit> {
    let arr = vm
        .heap
        .alloc_array(ElemKind::Int, items.len(), "[I")
        .map_err(oom_throwable)?;
    if let Ok(elems) = vm.heap.array_elems_mut(arr) {
        elems.extend(items.iter().copied().map(Value::Int));
    }
    Ok(arr)
}

fn read_delegates(vm: &Engine, dm: ObjRef) -> Vec<ObjRef> {
    let Value::Obj(arr) = get(vm, dm, "mDisplayListeners") else {
        return Vec::new();
    };
    let Ok((elems, _)) = vm.heap.as_array_elems(arr) else {
        return Vec::new();
    };
    elems
        .iter()
        .filter_map(|v| match v {
            Value::Obj(r) => Some(*r),
            _ => None,
        })
        .collect()
}

fn write_delegates(vm: &mut Engine, dm: ObjRef, delegates: &[ObjRef]) -> Result<(), VmExit> {
    let items: Vec<Value> = delegates.iter().copied().map(Value::Obj).collect();
    let arr = alloc_obj_array(vm, "Ljava/lang/Object;", items)?;
    put(vm, dm, "mDisplayListeners", Value::Obj(arr))
}

fn register_listener(
    vm: &mut Engine,
    dm: ObjRef,
    listener_val: Option<&Value>,
    dispatcher_val: Value,
    events_mask: i64,
) -> Result<(), VmExit> {
    let Some(Value::Obj(listener)) = listener_val else {
        return Err(iae("listener must not be null"));
    };
    if events_mask == 0 {
        return Err(iae("The set of events to listen to must not be empty."));
    }
    let mut delegates = read_delegates(vm, dm);
    for &del in &delegates {
        if matches!(get(vm, del, "mListener"), Value::Obj(existing) if existing == *listener) {
            put(vm, del, "mHandler", dispatcher_val)?;
            put(vm, del, "mEventsMask", Value::Long(events_mask))?;
            return Ok(());
        }
    }
    let delegate = vm
        .heap
        .alloc_instance(
            LISTENER_DELEGATE.to_string(),
            vec![
                ("mListener".to_string(), Value::Obj(*listener)),
                ("mHandler".to_string(), dispatcher_val),
                ("mEventsMask".to_string(), Value::Long(events_mask)),
            ],
        )
        .map_err(oom_throwable)?;
    delegates.push(delegate);
    write_delegates(vm, dm, &delegates)
}

fn unregister_listener(
    vm: &mut Engine,
    dm: ObjRef,
    listener_val: Option<&Value>,
) -> Result<(), VmExit> {
    let Some(Value::Obj(listener)) = listener_val else {
        return Err(iae("listener must not be null"));
    };
    let delegates = read_delegates(vm, dm);
    let kept: Vec<ObjRef> = delegates
        .into_iter()
        .filter(
            |&del| !matches!(get(vm, del, "mListener"), Value::Obj(existing) if existing == *listener),
        )
        .collect();
    write_delegates(vm, dm, &kept)
}

fn display_manager_intrinsic(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") | ("<init>", "(Landroid/content/Context;)V") => Value::Null,
        ("getDisplay", "(I)Landroid/view/Display;") => {
            if args.first().and_then(|a| a.as_int().ok()) == Some(0) {
                Value::Obj(alloc_display(vm)?)
            } else {
                Value::Null
            }
        }
        ("getDisplays", "()[Landroid/view/Display;") => {
            let disp = alloc_display(vm)?;
            Value::Obj(alloc_obj_array(vm, DISPLAY, vec![Value::Obj(disp)])?)
        }
        ("getDisplays", "(Ljava/lang/String;)[Landroid/view/Display;") => {
            let is_null_category = matches!(args.first(), None | Some(Value::Null));
            let items = if is_null_category {
                vec![Value::Obj(alloc_display(vm)?)]
            } else {
                Vec::new()
            };
            Value::Obj(alloc_obj_array(vm, DISPLAY, items)?)
        }
        (
            "registerDisplayListener",
            "(Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;)V",
        ) => {
            let handler = args.get(1).cloned().unwrap_or(Value::Null);
            register_listener(vm, recv, args.first(), handler, DEFAULT_EVENTS_MASK)?;
            Value::Null
        }
        (
            "registerDisplayListener",
            "(Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;J)V",
        ) => {
            let handler = args.get(1).cloned().unwrap_or(Value::Null);
            let mask = match args.get(2) {
                Some(Value::Long(m)) => *m,
                Some(Value::Int(m)) => i64::from(*m),
                _ => 0,
            };
            register_listener(vm, recv, args.first(), handler, mask)?;
            Value::Null
        }
        (
            "registerDisplayListener",
            "(Ljava/util/concurrent/Executor;JLandroid/hardware/display/DisplayManager$DisplayListener;)V",
        ) => {
            let listener_arg = args.get(2);
            if !matches!(listener_arg, Some(Value::Obj(_))) {
                return Err(iae("listener must not be null"));
            }
            let Some(Value::Obj(exec)) = args.first() else {
                return Err(iae("executor must not be null"));
            };
            let mask = match args.get(1) {
                Some(Value::Long(m)) => *m,
                Some(Value::Int(m)) => i64::from(*m),
                _ => 0,
            };
            register_listener(vm, recv, listener_arg, Value::Obj(*exec), mask)?;
            Value::Null
        }
        (
            "unregisterDisplayListener",
            "(Landroid/hardware/display/DisplayManager$DisplayListener;)V",
        ) => {
            unregister_listener(vm, recv, args.first())?;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

fn display_mode_intrinsic(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let int_field = |f: &str| get(vm, recv, f).as_int().unwrap_or(0);
    let float_field = |f: &str| match get(vm, recv, f) {
        Value::Float(v) => v,
        _ => 60.0,
    };
    Ok(Some(match (name, sig) {
        ("<init>", "(IIIF)V") => {
            let id = args.first().and_then(|v| v.as_int().ok()).unwrap_or(1);
            let w = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
            let h = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0);
            let fps = match args.get(3) {
                Some(Value::Float(v)) => *v,
                _ => 60.0,
            };
            put(vm, recv, "mModeId", Value::Int(id))?;
            put(vm, recv, "mWidth", Value::Int(w))?;
            put(vm, recv, "mHeight", Value::Int(h))?;
            put(vm, recv, "mRefreshRate", Value::Float(fps))?;
            Value::Null
        }
        ("getModeId", "()I") => Value::Int(int_field("mModeId")),
        ("getPhysicalWidth", "()I") => Value::Int(int_field("mWidth")),
        ("getPhysicalHeight", "()I") => Value::Int(int_field("mHeight")),
        ("getRefreshRate", "()F") => Value::Float(float_field("mRefreshRate")),
        ("getAlternativeRefreshRates", "()[F") => Value::Obj(alloc_float_array(vm, &[])?),
        ("getSupportedHdrTypes", "()[I") => Value::Obj(alloc_int_array(vm, &[])?),
        ("describeContents", "()I") => Value::Int(0),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match args.first() {
                Some(Value::Obj(other)) if *other == recv => true,
                Some(Value::Obj(other)) if vm.heap.class_of(*other).ok() == Some(DISPLAY_MODE) => {
                    let other_fps = match get(vm, *other, "mRefreshRate") {
                        Value::Float(v) => v,
                        _ => 60.0,
                    };
                    int_field("mModeId") == get(vm, *other, "mModeId").as_int().unwrap_or(0)
                        && int_field("mWidth") == get(vm, *other, "mWidth").as_int().unwrap_or(0)
                        && int_field("mHeight") == get(vm, *other, "mHeight").as_int().unwrap_or(0)
                        && float_field("mRefreshRate").to_bits() == other_fps.to_bits()
                }
                _ => false,
            };
            Value::Int(i32::from(eq))
        }
        ("hashCode", "()I") => {
            let mut h = 1i32;
            h = h.wrapping_mul(31).wrapping_add(int_field("mModeId"));
            h = h.wrapping_mul(31).wrapping_add(int_field("mWidth"));
            h = h.wrapping_mul(31).wrapping_add(int_field("mHeight"));
            h = h
                .wrapping_mul(31)
                .wrapping_add(float_field("mRefreshRate").to_bits() as i32);
            Value::Int(h)
        }
        ("toString", "()Ljava/lang/String;") => {
            let s = format!(
                "{{id={}, width={}, height={}, fps={:.1}}}",
                int_field("mModeId"),
                int_field("mWidth"),
                int_field("mHeight"),
                float_field("mRefreshRate")
            );
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
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
    if recv_class == DISPLAY_MANAGER {
        return display_manager_intrinsic(vm, recv, name, sig, args);
    }
    if recv_class == DISPLAY_MODE {
        return display_mode_intrinsic(vm, recv, name, sig, args);
    }
    // display getters on View / Context / WindowManager / DisplayManager subclasses
    if recv_class != DISPLAY {
        let is_getter = match name {
            "getDisplay" => sig == "()Landroid/view/Display;" || sig == "(I)Landroid/view/Display;",
            "getDefaultDisplay" => true,
            _ => false,
        };
        if is_getter && mref_class.starts_with("Landroid/") {
            if sig.starts_with("(I)") && args.first().and_then(|a| a.as_int().ok()) != Some(0) {
                return Ok(Some(Value::Null));
            }
            return Ok(Some(Value::Obj(alloc_display(vm)?)));
        }
        return Ok(None);
    }
    let d = crate::android_res::device();
    let (w, h) = (d.w as i32, d.h as i32);
    let density = d.dpi as f32 / 160.0;
    Ok(Some(match name {
        "getDisplayId" | "getRotation" | "getOrientation" | "getFlags" => Value::Int(0),
        "getState" => Value::Int(2), // STATE_ON
        "getRefreshRate" => Value::Float(60.0),
        "getWidth" => Value::Int(w),
        "getHeight" => Value::Int(h),
        "getPixelFormat" => Value::Int(1), // RGBA_8888
        "isValid" => Value::Int(1),
        "isHdr" | "isWideColorGamut" => Value::Int(0),
        "getAppVsyncOffsetNanos" => Value::Long(0),
        "getPresentationDeadlineNanos" => Value::Long(16_666_666),
        "getCutout" | "getRoundedCorner" => Value::Null,
        "getMode" if sig == "()Landroid/view/Display$Mode;" => Value::Obj(alloc_default_mode(vm)?),
        "getSupportedModes" if sig == "()[Landroid/view/Display$Mode;" => {
            let mode = alloc_default_mode(vm)?;
            Value::Obj(alloc_obj_array(vm, DISPLAY_MODE, vec![Value::Obj(mode)])?)
        }
        "getSupportedRefreshRates" if sig == "()[F" => Value::Obj(alloc_float_array(vm, &[60.0])?),
        "getName" => Value::Obj(crate::intrinsics::alloc_string(
            vm,
            "Built-in Screen".into(),
        )?),
        "getSize" | "getRealSize" => {
            if let Some(Value::Obj(p)) = args.first() {
                put(vm, *p, "x", Value::Int(w))?;
                put(vm, *p, "y", Value::Int(h))?;
            }
            Value::Null
        }
        "getMetrics" | "getRealMetrics" => {
            if let Some(Value::Obj(m)) = args.first() {
                put(vm, *m, "widthPixels", Value::Int(w))?;
                put(vm, *m, "heightPixels", Value::Int(h))?;
                put(vm, *m, "density", Value::Float(density))?;
                put(vm, *m, "scaledDensity", Value::Float(density))?;
                put(vm, *m, "densityDpi", Value::Int(d.dpi as i32))?;
                put(vm, *m, "xdpi", Value::Float(d.dpi as f32))?;
                put(vm, *m, "ydpi", Value::Float(d.dpi as f32))?;
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

/// Deliver a display lifecycle event (`EVENT_FLAG_DISPLAY_ADDED`,
/// `EVENT_FLAG_DISPLAY_REMOVED`, or `EVENT_FLAG_DISPLAY_CHANGED`) to all
/// listeners currently registered on the engine's canonical `DisplayManager`
/// whose `mEventsMask` subscribes to `event_flag`.
pub fn dispatch_display_event(
    vm: &mut Engine,
    display_id: i32,
    event_flag: i64,
) -> Result<usize, VmExit> {
    let cb_method = match event_flag {
        EVENT_FLAG_DISPLAY_ADDED => "onDisplayAdded",
        EVENT_FLAG_DISPLAY_REMOVED => "onDisplayRemoved",
        EVENT_FLAG_DISPLAY_CHANGED => "onDisplayChanged",
        _ => return Err(iae("Unknown display event flag")),
    };
    let key = (
        "Lrd/SystemServices;".to_string(),
        DISPLAY_MANAGER.to_string(),
    );
    let Some(Value::Obj(dm)) = vm.statics.get(&key).cloned() else {
        return Ok(0);
    };
    // Snapshot delegates before invoking guest callbacks so re-entrant
    // `unregisterDisplayListener` during a callback cannot corrupt iteration.
    let snapshot: Vec<ObjRef> = read_delegates(vm, dm)
        .into_iter()
        .filter_map(|del| {
            let mask = match get(vm, del, "mEventsMask") {
                Value::Long(m) => m,
                _ => 0,
            };
            if (mask & event_flag) == 0 {
                return None;
            }
            match get(vm, del, "mListener") {
                Value::Obj(listener) => Some(listener),
                _ => None,
            }
        })
        .collect();

    let mut delivered = 0usize;
    for listener in snapshot {
        vm.invoke_instance(listener, cb_method, "(I)V", &[Value::Int(display_id)])?;
        delivered += 1;
    }
    Ok(delivered)
}
