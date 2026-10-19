//! `android.view.Display`, the emulated device's single built-in display.
//!
//! - **Getters.** `View.getDisplay()`, `Context.getDisplay()`,
//!   `WindowManager.getDefaultDisplay()` and `DisplayManager.getDisplay(0)`
//!   all return the default display, id 0.
//! - **Geometry** follows `android_res::device()` (720×1440 at 280 dpi by
//!   default, overridable via `RD_SCREEN_W`/`RD_SCREEN_H`/`RD_DENSITY_DPI`).
//!   It is served by `getSize`/`getRealSize(Point)`, `getMetrics` and
//!   `getRealMetrics(DisplayMetrics)`, and `getWidth`/`getHeight`.
//! - **Fixed values.** `getRefreshRate` is 60 Hz, `getRotation` is
//!   `ROTATION_0`, `getState` is `STATE_ON` and `getName` is
//!   "Built-in Screen".

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const DISPLAY: &str = "Landroid/view/Display;";

fn display(vm: &mut Engine) -> Result<Value, VmExit> {
    Ok(Value::Obj(
        vm.heap.alloc_instance(DISPLAY.to_string(), Vec::new())?,
    ))
}

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // display getters on View / Context / WindowManager / DisplayManager
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
            return Ok(Some(display(vm)?));
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
