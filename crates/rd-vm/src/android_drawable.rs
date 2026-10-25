//! `android.graphics.drawable.Drawable` base behaviour, `ColorDrawable`,
//! `RippleDrawable` and `android.content.res.ColorStateList`.
//!
//! - **Drawable base.** It applies equally to DEX subclasses (e.g.
//!   Compose's `UnprojectedRipple extends RippleDrawable`). Covered:
//!   - bounds (`setBounds`/`getBounds`/`copyBounds`/`getDirtyBounds`);
//!   - `Callback` with `invalidateSelf` → `invalidateDrawable`;
//!   - visibility, state/level, alpha, tint and colour filter (recorded);
//!   - hotspot calls (accepted);
//!   - intrinsic size −1, opacity TRANSLUCENT, `mutate` → `this`.
//! - **ColorDrawable** fills its bounds when drawn.
//! - **RippleDrawable** keeps colour and radius. The transient ripple
//!   animation is not drawn, so the frame shows the resting state.
//! - **ColorStateList** stores `(states[][], colors[])`.
//!   `getColorForState` follows AOSP: the first entry whose required
//!   states are all present (negative ids must be absent) wins, falling
//!   back to the default colour.
//!
//! State lives in heap fields (`rd_l`, `rd_t`, `rd_r`, `rd_b`, `rd_alpha`,
//! `rd_color`, `rd_cb`, …).

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

pub(crate) const CSL: &str = "Landroid/content/res/ColorStateList;";
const PKG: &str = "Landroid/graphics/drawable/";

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

fn geti(vm: &Engine, r: ObjRef, f: &str, d: i32) -> i32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Int(i)) => i,
        _ => d,
    }
}

fn int(args: &[Value], i: usize) -> i32 {
    match args.get(i) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

fn int_array(vm: &Engine, v: Option<&Value>) -> Vec<i32> {
    match v {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| {
                e.iter()
                    .filter_map(|x| match x {
                        Value::Int(i) => Some(*i),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn new_int_array(vm: &mut Engine, v: &[i32]) -> Result<ObjRef, VmExit> {
    let a = vm.heap.alloc_array(ElemKind::Int, v.len(), "I")?;
    *vm.heap.array_elems_mut(a)? = v.iter().map(|i| Value::Int(*i)).collect();
    Ok(a)
}

// ── ColorStateList ─────────────────────────────────────────────────────

fn csl_new(vm: &mut Engine, states: Vec<Vec<i32>>, colors: Vec<i32>) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_instance(CSL.to_string(), Vec::new())?;
    csl_set(vm, r, states, colors)?;
    Ok(r)
}

fn csl_set(
    vm: &mut Engine,
    r: ObjRef,
    states: Vec<Vec<i32>>,
    colors: Vec<i32>,
) -> Result<(), VmExit> {
    // flattened: rd_states = [n0, s.., n1, s.., …]
    let mut flat = Vec::new();
    for s in &states {
        flat.push(s.len() as i32);
        flat.extend_from_slice(s);
    }
    let fa = new_int_array(vm, &flat)?;
    let ca = new_int_array(vm, &colors)?;
    put(vm, r, "rd_states", Value::Obj(fa))?;
    put(vm, r, "rd_colors", Value::Obj(ca))
}

fn csl_get(vm: &Engine, r: ObjRef) -> (Vec<Vec<i32>>, Vec<i32>) {
    let flat = int_array(vm, vm.heap.get_field(r, "rd_states").ok().as_ref());
    let colors = int_array(vm, vm.heap.get_field(r, "rd_colors").ok().as_ref());
    let mut states = Vec::new();
    let mut i = 0;
    while i < flat.len() {
        let n = flat[i].max(0) as usize;
        states.push(flat[i + 1..(i + 1 + n).min(flat.len())].to_vec());
        i += 1 + n;
    }
    (states, colors)
}

/// AOSP `StateSet.stateSetMatches`.
fn state_matches(spec: &[i32], state: &[i32]) -> bool {
    spec.iter().all(|&s| {
        if s >= 0 {
            state.contains(&s)
        } else {
            !state.contains(&-s)
        }
    })
}

fn csl_default(states: &[Vec<i32>], colors: &[i32]) -> i32 {
    // AOSP onColorsChanged: colors[0], overridden by the LAST empty
    // (wildcard) spec; an empty list defaults to Color.RED.
    states
        .iter()
        .rposition(|s| s.is_empty())
        .and_then(|i| colors.get(i).copied())
        .or_else(|| colors.first().copied())
        .unwrap_or(0xFFFF_0000u32 as i32)
}

fn color_state_list(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let rows: Vec<Vec<i32>> = match args.first() {
                Some(Value::Obj(a)) => {
                    let rows: Vec<Value> = vm
                        .heap
                        .as_array_elems(*a)
                        .map(|(e, _)| e.to_vec())
                        .unwrap_or_default();
                    rows.iter().map(|r| int_array(vm, Some(r))).collect()
                }
                _ => Vec::new(),
            };
            let colors = int_array(vm, args.get(1));
            csl_set(vm, recv, rows, colors)?;
            Value::Null
        }
        "getDefaultColor" => {
            let (s, c) = csl_get(vm, recv);
            Value::Int(csl_default(&s, &c))
        }
        "getColorForState" => {
            let state = int_array(vm, args.first());
            let (s, c) = csl_get(vm, recv);
            let hit = s
                .iter()
                .position(|spec| state_matches(spec, &state))
                .and_then(|i| c.get(i).copied());
            Value::Int(hit.unwrap_or_else(|| int(args, 1)))
        }
        "isStateful" => {
            let (s, _) = csl_get(vm, recv);
            Value::Int(s.iter().any(|x| !x.is_empty()) as i32)
        }
        "isOpaque" => {
            let (_, c) = csl_get(vm, recv);
            Value::Int(c.iter().all(|x| (*x as u32) >> 24 == 0xFF) as i32)
        }
        "withAlpha" => {
            let a = int(args, 0) as u32 & 0xFF;
            let (s, c) = csl_get(vm, recv);
            let c = c
                .iter()
                .map(|x| ((*x as u32 & 0x00FF_FFFF) | (a << 24)) as i32)
                .collect();
            Value::Obj(csl_new(vm, s, c)?)
        }
        "getChangingConfigurations" => Value::Int(0),
        "toString" => {
            let (_, c) = csl_get(vm, recv);
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!("ColorStateList{{mColors={c:?}}}"),
            )?)
        }
        _ => return Ok(None),
    }))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == CSL && name == "valueOf" {
        let c = int(args, 0);
        return Ok(Some(Value::Obj(csl_new(vm, vec![vec![]], vec![c])?)));
    }
    Ok(None)
}

// ── Drawable family ────────────────────────────────────────────────────

/// True for a framework Drawable class name (host-implemented base).
fn is_drawable_class(c: &str) -> bool {
    c.starts_with(PKG) && !c.contains("Icon")
}

fn bounds(vm: &Engine, r: ObjRef) -> [i32; 4] {
    [
        geti(vm, r, "rd_l", 0),
        geti(vm, r, "rd_t", 0),
        geti(vm, r, "rd_r", 0),
        geti(vm, r, "rd_b", 0),
    ]
}

fn set_bounds(vm: &mut Engine, r: ObjRef, b: [i32; 4]) -> Result<bool, VmExit> {
    let changed = bounds(vm, r) != b;
    for (f, v) in ["rd_l", "rd_t", "rd_r", "rd_b"].iter().zip(b) {
        put(vm, r, f, Value::Int(v))?;
    }
    if changed {
        // AOSP: onBoundsChange (overridable)
        let rect = new_rect(vm, b)?;
        crate::jcollections::guest_call(
            vm,
            r,
            "onBoundsChange",
            "(Landroid/graphics/Rect;)V",
            &[Value::Obj(rect)],
        )?;
    }
    Ok(changed)
}

fn new_rect(vm: &mut Engine, b: [i32; 4]) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Landroid/graphics/Rect;".into(), Vec::new())?;
    write_rect(vm, r, b)?;
    Ok(r)
}

fn write_rect(vm: &mut Engine, r: ObjRef, b: [i32; 4]) -> Result<(), VmExit> {
    for (f, v) in ["left", "top", "right", "bottom"].iter().zip(b) {
        put(vm, r, f, Value::Int(v))?;
    }
    Ok(())
}

fn read_rect(vm: &Engine, r: ObjRef) -> [i32; 4] {
    ["left", "top", "right", "bottom"].map(|f| geti(vm, r, f, 0))
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
    if recv_class == CSL || mref_class == CSL {
        return color_state_list(vm, name, recv, args);
    }
    if !is_drawable_class(mref_class) && !is_drawable_class(recv_class) {
        return Ok(None);
    }
    let kind = if is_drawable_class(recv_class) {
        recv_class
    } else {
        mref_class
    };
    let obj = |i: usize| match args.get(i) {
        Some(Value::Obj(o)) => Some(*o),
        _ => None,
    };
    Ok(Some(match (name, sig) {
        ("<init>", _) => {
            put(vm, recv, "rd_alpha", Value::Int(255))?;
            put(vm, recv, "rd_visible", Value::Int(1))?;
            if kind.ends_with("ColorDrawable;") {
                // rd_color mirrors mUseColor, rd_base mBaseColor.
                let c = Value::Int(int(args, 0));
                put(vm, recv, "rd_color", c.clone())?;
                put(vm, recv, "rd_base", c)?;
            }
            if kind.ends_with("RippleDrawable;") {
                if sig.starts_with("(Landroid/content/res/ColorStateList;") {
                    match obj(0) {
                        Some(c) => put(vm, recv, "rd_csl", Value::Obj(c))?,
                        None => {
                            return Err(crate::android_sysservices::iae(
                                "RippleDrawable requires a non-null color",
                            ))
                        }
                    }
                } else if let Some(c) = obj(0) {
                    put(vm, recv, "rd_csl", Value::Obj(c))?;
                }
                put(vm, recv, "rd_radius", Value::Int(-1))?;
            }
            Value::Null
        }
        ("setBounds", "(IIII)V") => {
            set_bounds(
                vm,
                recv,
                [int(args, 0), int(args, 1), int(args, 2), int(args, 3)],
            )?;
            Value::Null
        }
        ("setBounds", _) => {
            if let Some(r) = obj(0) {
                let b = read_rect(vm, r);
                set_bounds(vm, recv, b)?;
            }
            Value::Null
        }
        ("getBounds" | "getDirtyBounds", _) | ("copyBounds", "()Landroid/graphics/Rect;") => {
            let b = bounds(vm, recv);
            Value::Obj(new_rect(vm, b)?)
        }
        ("copyBounds", _) => {
            if let Some(r) = obj(0) {
                let b = bounds(vm, recv);
                write_rect(vm, r, b)?;
            }
            Value::Null
        }
        ("setCallback", _) => {
            put(
                vm,
                recv,
                "rd_cb",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getCallback", _) => vm.heap.get_field(recv, "rd_cb").unwrap_or(Value::Null),
        ("invalidateSelf", _) => {
            if let Ok(Value::Obj(cb)) = vm.heap.get_field(recv, "rd_cb") {
                let _ = crate::jcollections::guest_call(
                    vm,
                    cb,
                    "invalidateDrawable",
                    "(Landroid/graphics/drawable/Drawable;)V",
                    &[Value::Obj(recv)],
                )?;
            }
            vm.gfx.frame_valid = false;
            Value::Null
        }
        ("scheduleSelf" | "unscheduleSelf", _) => Value::Null,
        ("setVisible", _) => {
            let was = geti(vm, recv, "rd_visible", 1);
            put(vm, recv, "rd_visible", Value::Int(int(args, 0)))?;
            Value::Int((was != int(args, 0)) as i32)
        }
        ("isVisible", _) => Value::Int(geti(vm, recv, "rd_visible", 1)),
        ("setState", _) => {
            put(
                vm,
                recv,
                "rd_state",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Int(0)
        }
        ("getState", _) => match vm.heap.get_field(recv, "rd_state") {
            Ok(Value::Obj(a)) => Value::Obj(a),
            _ => Value::Obj(new_int_array(vm, &[])?),
        },
        ("isStateful", _) => Value::Int(kind.ends_with("RippleDrawable;") as i32),
        ("isProjected", _) => Value::Int(0),
        ("setLevel", _) => {
            put(vm, recv, "rd_level", Value::Int(int(args, 0)))?;
            Value::Int(0)
        }
        ("getLevel", _) => Value::Int(geti(vm, recv, "rd_level", 0)),
        ("setAlpha", _) => {
            if kind.ends_with("ColorDrawable;") {
                // AOSP modulates mBaseColor's alpha into mUseColor:
                // alpha += alpha >> 7 (0..256), use = base * a >> 8.
                // Keep the whole calculation in Java's wrapping i32
                // arithmetic so out-of-contract values cannot panic.
                let a = int(args, 0);
                let a = a.wrapping_add(a >> 7);
                let base = geti(vm, recv, "rd_base", geti(vm, recv, "rd_color", 0)) as u32;
                let base_alpha = (base >> 24) as i32;
                let use_alpha = base_alpha.wrapping_mul(a) >> 8;
                let use_color = (base & 0x00FF_FFFF) | ((use_alpha as u32) << 24);
                put(vm, recv, "rd_color", Value::Int(use_color as i32))?;
            } else {
                put(vm, recv, "rd_alpha", Value::Int(int(args, 0) & 0xFF))?;
            }
            Value::Null
        }
        ("getAlpha", _) => {
            if kind.ends_with("ColorDrawable;") {
                // The combined (used) color's alpha, not the setter value.
                Value::Int((geti(vm, recv, "rd_color", 0) as u32 >> 24) as i32)
            } else {
                Value::Int(geti(vm, recv, "rd_alpha", 255))
            }
        }
        ("setColorFilter" | "setTintList" | "setTintMode" | "setTintBlendMode" | "setTint", _) => {
            put(
                vm,
                recv,
                &format!("rd_{name}"),
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getColorFilter", _) => vm
            .heap
            .get_field(recv, "rd_setColorFilter")
            .unwrap_or(Value::Null),
        ("clearColorFilter", _) => {
            put(vm, recv, "rd_setColorFilter", Value::Null)?;
            Value::Null
        }
        (
            "setHotspot" | "setHotspotBounds" | "jumpToCurrentState" | "setDither"
            | "setFilterBitmap",
            _,
        ) => Value::Null,
        ("setAutoMirrored" | "setLayoutDirection", _) => {
            if sig.ends_with('Z') {
                Value::Int(0)
            } else {
                Value::Null
            }
        }
        ("isAutoMirrored", _) => Value::Int(0),
        ("getLayoutDirection", _) => Value::Int(0),
        ("getIntrinsicWidth" | "getIntrinsicHeight", _) => Value::Int(-1),
        ("getMinimumWidth" | "getMinimumHeight", _) => Value::Int(0),
        ("getOpacity", _) => {
            if kind.ends_with("ColorDrawable;") {
                if !matches!(
                    vm.heap.get_field(recv, "rd_setColorFilter"),
                    Ok(Value::Null)
                ) && vm.heap.get_field(recv, "rd_setColorFilter").is_ok()
                {
                    Value::Int(-3) // TRANSLUCENT behind a colour filter
                } else {
                    match geti(vm, recv, "rd_color", 0) as u32 >> 24 {
                        255 => Value::Int(-1), // PixelFormat.OPAQUE
                        0 => Value::Int(-2),   // PixelFormat.TRANSPARENT
                        _ => Value::Int(-3),
                    }
                }
            } else {
                Value::Int(-3) // PixelFormat.TRANSLUCENT
            }
        }
        ("getPadding", _) => Value::Int(0),
        ("getChangingConfigurations", _) => Value::Int(0),
        ("mutate", _) => Value::Obj(recv),
        ("getConstantState", _) => Value::Null,
        ("getCurrent", _) => Value::Obj(recv),
        // ColorDrawable
        ("getColor", _) => Value::Int(geti(vm, recv, "rd_color", 0)),
        ("setColor", "(I)V") => {
            let c = Value::Int(int(args, 0));
            put(vm, recv, "rd_color", c.clone())?;
            if kind.ends_with("ColorDrawable;") {
                put(vm, recv, "rd_base", c)?;
            }
            Value::Null
        }
        // RippleDrawable
        ("setColor", _) => {
            if kind.ends_with("RippleDrawable;") && obj(0).is_none() {
                return Err(crate::android_sysservices::iae("color cannot be null"));
            }
            put(
                vm,
                recv,
                "rd_csl",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setRadius", _) => {
            put(vm, recv, "rd_radius", Value::Int(int(args, 0)))?;
            Value::Null
        }
        ("getRadius", _) => Value::Int(geti(vm, recv, "rd_radius", -1)),
        ("draw", _) => {
            if kind.ends_with("ColorDrawable;") && geti(vm, recv, "rd_visible", 1) != 0 {
                if let Some(c) = obj(0) {
                    let b = bounds(vm, recv);
                    let col = geti(vm, recv, "rd_color", 0) as u32;
                    let a = ((col >> 24) * geti(vm, recv, "rd_alpha", 255) as u32) / 255;
                    vm.frame_fill(
                        c,
                        b[0] as f32,
                        b[1] as f32,
                        (b[2] - b[0]) as f32,
                        (b[3] - b[1]) as f32,
                        (col & 0x00FF_FFFF) | (a << 24),
                    );
                }
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_set_matching_follows_aosp() {
        let pressed = 0x0101_00a7;
        assert!(state_matches(&[pressed], &[pressed, 1]));
        assert!(!state_matches(&[pressed], &[1]));
        assert!(state_matches(&[-pressed], &[1]));
        assert!(state_matches(&[], &[]));
        assert_eq!(csl_default(&[vec![pressed], vec![]], &[1, 2]), 2);
    }
}
