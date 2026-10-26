//! `android.view.MotionEvent`: a single-pointer touch-screen event, enough
//! for the framework touch pipeline (`dispatchTouchEvent` / `onTouchEvent`)
//! and for gesture code (Compose pointer input, GestureDetector,
//! scrolling).
//!
//! - **State** lives in heap fields: `rd_action`, `rd_x`/`rd_y` (view-local)
//!   and `rd_ox`/`rd_oy` (local→raw offset, so `getRawX` = x + ox),
//!   `rd_down`, `rd_time`, `rd_source`, `rd_meta`, `rd_buttons`.
//! - **Single pointer.** Pointer id 0, tool type FINGER, pressure 1, size 1,
//!   no history. `getX(int)`/`getY(int)` accept index 0.
//! - **Supported calls.** The `obtain` overloads (the
//!   PointerProperties/PointerCoords form takes pointer 0),
//!   `obtainNoHistory`, `offsetLocation`, `setLocation`, `setAction`,
//!   `setSource`, `getAxisValue(AXIS_X/AXIS_Y)`, `getPointerCoords`,
//!   `getPointerProperties` and `recycle` (a no-op).
//! - **Not modelled.** `transform(Matrix)` is a no-op.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

pub(crate) const ME: &str = "Landroid/view/MotionEvent;";
const COORDS: &str = "Landroid/view/MotionEvent$PointerCoords;";
const PROPS: &str = "Landroid/view/MotionEvent$PointerProperties;";

pub(crate) const ACTION_DOWN: i32 = 0;
pub(crate) const ACTION_UP: i32 = 1;
pub(crate) const ACTION_MOVE: i32 = 2;
/// InputDevice.SOURCE_TOUCHSCREEN
const SOURCE_TOUCHSCREEN: i32 = 0x1002;

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

fn getf(vm: &Engine, r: ObjRef, f: &str) -> f32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Float(v)) => v,
        Ok(Value::Int(i)) => i as f32,
        Ok(Value::Double(d)) => d as f32,
        _ => 0.0,
    }
}

fn geti(vm: &Engine, r: ObjRef, f: &str) -> i32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Int(i)) => i,
        _ => 0,
    }
}

fn getl(vm: &Engine, r: ObjRef, f: &str) -> i64 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Long(l)) => l,
        Ok(Value::Int(i)) => i as i64,
        _ => 0,
    }
}

fn arg_f(args: &[Value], i: usize) -> f32 {
    match args.get(i) {
        Some(Value::Float(f)) => *f,
        // raw f32 bits in an int slot
        Some(Value::Int(b)) => f32::from_bits(*b as u32),
        Some(Value::Double(d)) => *d as f32,
        _ => 0.0,
    }
}

fn arg_i(args: &[Value], i: usize) -> i32 {
    match args.get(i) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

fn arg_l(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Long(v)) => *v,
        Some(Value::Int(v)) => *v as i64,
        _ => 0,
    }
}

/// Allocates a touch-screen event at view-local `(x, y)` whose raw
/// (screen) position is `(x + ox, y + oy)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_event(
    vm: &mut Engine,
    action: i32,
    x: f32,
    y: f32,
    ox: f32,
    oy: f32,
    down: i64,
    time: i64,
) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_instance(ME.to_string(), Vec::new())?;
    put(vm, r, "rd_action", Value::Int(action))?;
    put(vm, r, "rd_x", Value::Float(x))?;
    put(vm, r, "rd_y", Value::Float(y))?;
    put(vm, r, "rd_ox", Value::Float(ox))?;
    put(vm, r, "rd_oy", Value::Float(oy))?;
    put(vm, r, "rd_down", Value::Long(down))?;
    put(vm, r, "rd_time", Value::Long(time))?;
    put(vm, r, "rd_source", Value::Int(SOURCE_TOUCHSCREEN))?;
    Ok(r)
}

/// A copy of `src` (same action, position and times).
pub(crate) fn copy_event(vm: &mut Engine, src: ObjRef) -> Result<ObjRef, VmExit> {
    let r = new_event(
        vm,
        geti(vm, src, "rd_action"),
        getf(vm, src, "rd_x"),
        getf(vm, src, "rd_y"),
        getf(vm, src, "rd_ox"),
        getf(vm, src, "rd_oy"),
        getl(vm, src, "rd_down"),
        getl(vm, src, "rd_time"),
    )?;
    let s = geti(vm, src, "rd_source");
    put(vm, r, "rd_source", Value::Int(s))?;
    Ok(r)
}

/// `offsetLocation(dx, dy)`: moves the local position, keeping the raw one.
pub(crate) fn offset(vm: &mut Engine, r: ObjRef, dx: f32, dy: f32) -> Result<(), VmExit> {
    let (x, y, ox, oy) = (
        getf(vm, r, "rd_x"),
        getf(vm, r, "rd_y"),
        getf(vm, r, "rd_ox"),
        getf(vm, r, "rd_oy"),
    );
    put(vm, r, "rd_x", Value::Float(x + dx))?;
    put(vm, r, "rd_y", Value::Float(y + dy))?;
    put(vm, r, "rd_ox", Value::Float(ox - dx))?;
    put(vm, r, "rd_oy", Value::Float(oy - dy))
}

pub(crate) fn xy(vm: &Engine, r: ObjRef) -> (f32, f32) {
    (getf(vm, r, "rd_x"), getf(vm, r, "rd_y"))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != ME {
        return Ok(None);
    }
    let r = match (name, sig) {
        ("obtain", "(Landroid/view/MotionEvent;)Landroid/view/MotionEvent;")
        | ("obtainNoHistory", _) => match args.first() {
            Some(Value::Obj(src)) => copy_event(vm, *src)?,
            _ => return Ok(Some(Value::Null)),
        },
        // obtain(downTime, eventTime, action, x, y, metaState)
        ("obtain", "(JJIFFI)Landroid/view/MotionEvent;")
        | ("obtain", "(JJIFFFFIFFII)Landroid/view/MotionEvent;") => {
            let r = new_event(
                vm,
                arg_i(args, 2),
                arg_f(args, 3),
                arg_f(args, 4),
                0.0,
                0.0,
                arg_l(args, 0),
                arg_l(args, 1),
            )?;
            let meta = arg_i(args, args.len() - 1);
            if sig == "(JJIFFI)Landroid/view/MotionEvent;" {
                put(vm, r, "rd_meta", Value::Int(meta))?;
            }
            r
        }
        // obtain(down, time, action, pointerCount, props[], coords[], meta, buttons, xPrec, yPrec, deviceId, edgeFlags, source, flags)
        ("obtain", s) if s.starts_with("(JJII[Landroid/view/MotionEvent$PointerProperties;") => {
            let (x, y) = match args.get(5) {
                Some(Value::Obj(arr)) => {
                    let first = vm
                        .heap
                        .as_array_elems(*arr)
                        .ok()
                        .and_then(|(e, _)| e.first().cloned());
                    match first {
                        Some(Value::Obj(c)) => (getf(vm, c, "x"), getf(vm, c, "y")),
                        _ => (0.0, 0.0),
                    }
                }
                _ => (0.0, 0.0),
            };
            let r = new_event(
                vm,
                arg_i(args, 2),
                x,
                y,
                0.0,
                0.0,
                arg_l(args, 0),
                arg_l(args, 1),
            )?;
            put(vm, r, "rd_meta", Value::Int(arg_i(args, 6)))?;
            put(vm, r, "rd_buttons", Value::Int(arg_i(args, 7)))?;
            if let Some(Value::Int(src)) = args.get(12) {
                put(vm, r, "rd_source", Value::Int(*src))?;
            }
            r
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Obj(r)))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class == COORDS || recv_class == PROPS {
        // value classes with public fields; only the constructors and
        // copy helpers are methods
        return Ok(match name {
            "<init>" => {
                if let Some(Value::Obj(src)) = args.first() {
                    for f in ["x", "y", "pressure", "size", "id", "toolType"] {
                        if let Ok(v) = vm.heap.get_field(*src, f) {
                            put(vm, recv, f, v)?;
                        }
                    }
                }
                Some(Value::Null)
            }
            "clear" => Some(Value::Null),
            "copyFrom" => {
                if let Some(Value::Obj(src)) = args.first() {
                    for f in ["x", "y", "pressure", "size", "id", "toolType"] {
                        if let Ok(v) = vm.heap.get_field(*src, f) {
                            put(vm, recv, f, v)?;
                        }
                    }
                }
                Some(Value::Null)
            }
            _ => None,
        });
    }
    if recv_class != ME {
        return Ok(None);
    }
    let action = geti(vm, recv, "rd_action");
    let x = getf(vm, recv, "rd_x");
    let y = getf(vm, recv, "rd_y");
    Ok(Some(match (name, sig) {
        ("getAction", _) => Value::Int(action),
        ("getActionMasked", _) => Value::Int(action & 0xff),
        ("getActionIndex", _) => Value::Int((action >> 8) & 0xff),
        ("getX", _) | ("getAxisValue", "(I)F") if name == "getX" || arg_i(args, 0) == 0 => {
            Value::Float(x)
        }
        ("getY", _) | ("getAxisValue", _) if name == "getY" || arg_i(args, 0) == 1 => {
            Value::Float(y)
        }
        ("getAxisValue", _) => Value::Float(0.0),
        ("getRawX", _) => Value::Float(x + getf(vm, recv, "rd_ox")),
        ("getRawY", _) => Value::Float(y + getf(vm, recv, "rd_oy")),
        ("getPointerCount", _) => Value::Int(1),
        ("getPointerId", _) => Value::Int(0),
        ("findPointerIndex", _) => Value::Int(if arg_i(args, 0) == 0 { 0 } else { -1 }),
        ("getToolType", _) => Value::Int(1), // TOOL_TYPE_FINGER
        ("getPressure", _) | ("getSize", _) => Value::Float(1.0),
        ("getTouchMajor" | "getTouchMinor" | "getToolMajor" | "getToolMinor", _) => {
            Value::Float(1.0)
        }
        ("getOrientation", _) => Value::Float(0.0),
        ("getXPrecision" | "getYPrecision", _) => Value::Float(1.0),
        ("getSource", _) => Value::Int(geti(vm, recv, "rd_source")),
        ("setSource", _) => {
            put(vm, recv, "rd_source", Value::Int(arg_i(args, 0)))?;
            Value::Null
        }
        ("isFromSource", _) => {
            let s = arg_i(args, 0);
            Value::Int((geti(vm, recv, "rd_source") & s == s) as i32)
        }
        ("getDeviceId", _) | ("getEdgeFlags", _) | ("getFlags", _) => Value::Int(0),
        ("getMetaState", _) => Value::Int(geti(vm, recv, "rd_meta")),
        ("getButtonState", _) => Value::Int(geti(vm, recv, "rd_buttons")),
        ("isButtonPressed", _) => {
            Value::Int((geti(vm, recv, "rd_buttons") & arg_i(args, 0) != 0) as i32)
        }
        ("getDownTime", _) => Value::Long(getl(vm, recv, "rd_down")),
        ("getEventTime", _) => Value::Long(getl(vm, recv, "rd_time")),
        ("getEventTimeNanos", _) => Value::Long(getl(vm, recv, "rd_time") * 1_000_000),
        ("getHistorySize", _) => Value::Int(0),
        ("getHistoricalX" | "getHistoricalY" | "getHistoricalAxisValue", _) => {
            Value::Float(if name.ends_with('Y') { y } else { x })
        }
        ("getHistoricalEventTime", _) => Value::Long(getl(vm, recv, "rd_time")),
        ("setAction", _) => {
            put(vm, recv, "rd_action", Value::Int(arg_i(args, 0)))?;
            Value::Null
        }
        ("offsetLocation", _) => {
            offset(vm, recv, arg_f(args, 0), arg_f(args, 1))?;
            Value::Null
        }
        ("setLocation", _) => {
            let (dx, dy) = (arg_f(args, 0) - x, arg_f(args, 1) - y);
            offset(vm, recv, dx, dy)?;
            Value::Null
        }
        ("transform", _) => Value::Null,
        ("getPointerCoords", _) => {
            if let Some(Value::Obj(c)) = args.get(1) {
                put(vm, *c, "x", Value::Float(x))?;
                put(vm, *c, "y", Value::Float(y))?;
                put(vm, *c, "pressure", Value::Float(1.0))?;
                put(vm, *c, "size", Value::Float(1.0))?;
            }
            Value::Null
        }
        ("getPointerProperties", _) => {
            if let Some(Value::Obj(p)) = args.get(1) {
                put(vm, *p, "id", Value::Int(0))?;
                put(vm, *p, "toolType", Value::Int(1))?;
            }
            Value::Null
        }
        ("recycle", _) => Value::Null,
        ("copy", _) => Value::Obj(copy_event(vm, recv)?),
        ("toString", _) => {
            let s = format!("MotionEvent {{ action={action}, x={x}, y={y} }}");
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    #[test]
    fn action_constants_match_aosp() {
        assert_eq!(
            (super::ACTION_DOWN, super::ACTION_UP, super::ACTION_MOVE),
            (0, 1, 2)
        );
    }
}
