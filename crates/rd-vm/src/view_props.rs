//! General View property store: the last-resort fallback for `android.view.*`
//! and `android.widget.*` instance methods on host views that no specific
//! handler models.
//!
//! Real view constructors call dozens of property setters: AndroidX,
//! Compose's `AbstractComposeView`/`AndroidComposeView`, and Material
//! (`setClipChildren`, `setFocusable`, `setWillNotDraw`,
//! `setImportantForAccessibility`, `setLayerType`, `setElevation`,
//! `setTranslationX`, …). Modelling each by hand does not scale, and
//! failing on them blocks every modern app. This module handles them as
//! follows:
//! - **`setX(v)`** records `v` under `X` for that view. `setTag(key, v)` is
//!   recorded per key.
//! - **`getX()` / `isX()` / `hasX()`** return the recorded value, else the
//!   AOSP default: `alpha`/`scale` 1.0, `visibility` VISIBLE, primitives 0 or
//!   false.
//! - **Geometry getters** (`getWidth`, `getLeft`, `getMeasuredHeight`, …)
//!   read the host layout.
//! - **No-arg void actions** such as `invalidate`, `requestLayout`,
//!   `bringToFront` and `jumpDrawablesToCurrentState` are no-ops. There is
//!   no retained render node headless; the renderer re-reads the tree.
//! - **An object-typed getter with nothing recorded** still fails with
//!   NOT_FOUND, unless Android's default for it is null (`getTag`,
//!   `getBackground`, …). A real gap stays visible and is never silently
//!   null.
//!
//! The recorded properties are inspectable state; they are not rendered.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

/// Getters that need real modelling elsewhere. Never answered from the store.
const EXCLUDED: &[&str] = &[
    "getContext",
    "getResources",
    "getParent",
    "getRootView",
    "isAttachedToWindow",
    "getViewTreeObserver",
    "getHandler",
    "getWindowToken",
    "getApplicationWindowToken",
    "getDisplay",
    "isLaidOut",
    "findViewById",
    "getChildAt",
    "getChildCount",
];

/// Object getters whose Android default is null.
const NULLABLE: &[&str] = &[
    "getTag",
    "getBackground",
    "getForeground",
    "getLayoutParams",
    "getOnFocusChangeListener",
    "getContentDescription",
    "getAnimation",
    "getOutlineProvider",
    "getTouchDelegate",
    "getStateListAnimator",
    "getTooltipText",
    "getAccessibilityDelegate",
    "getLayoutTransition",
    "getFocusedChild",
    "findFocus",
    "getBackgroundTintList",
    "getForegroundTintList",
    "getKeyboardNavigationClusterSearch",
    "getTransitionName",
    "getClipBounds",
];

const NOOP_ACTIONS: &[&str] = &[
    "invalidate",
    "postInvalidate",
    "postInvalidateOnAnimation",
    "requestLayout",
    "forceLayout",
    "refreshDrawableState",
    "jumpDrawablesToCurrentState",
    "cancelLongPress",
    "bringToFront",
    "invalidateOutline",
    "clearAnimation",
    "requestApplyInsets",
    "dispatchApplyWindowInsets",
    "drawableStateChanged",
    "cancelPendingInputEvents",
    "resetPivot",
    "clearFocusNoop",
    "invalidateDrawable",
    "requestFitSystemWindows",
];

fn ret_type(sig: &str) -> &str {
    sig.rsplit_once(')').map_or("V", |(_, r)| r)
}

fn default_for(prop: &str, ret: &str) -> Option<Value> {
    Some(match ret {
        "F" => Value::Float(match prop {
            "Alpha" | "ScaleX" | "ScaleY" | "TransitionAlpha" => 1.0,
            _ => 0.0,
        }),
        "D" => Value::Double(0.0),
        "J" => Value::Long(0),
        "Z" => Value::Int(match prop {
            // AOSP defaults that are true
            "Enabled"
            | "SaveEnabled"
            | "SoundEffectsEnabled"
            | "HapticFeedbackEnabled"
            | "ClipChildren"
            | "ClipToPadding"
            | "ShowingLayoutBounds"
            | "DrawingCacheEnabled"
            | "MotionEventSplittingEnabled"
            | "Shown" => {
                (prop != "DrawingCacheEnabled" && prop != "ShowingLayoutBounds" && prop != "Shown")
                    as i32
            }
            _ => 0,
        }),
        "I" | "S" | "B" | "C" => Value::Int(match prop {
            "LayerType" | "Visibility" | "ImportantForAccessibility" | "LayoutDirection" => 0,
            "DescendantFocusability" => 0x40000, // FOCUS_AFTER_DESCENDANTS
            "Id" | "NextFocusDownId" | "NextFocusUpId" | "LabelFor" => -1,
            _ => 0,
        }),
        _ => return None,
    })
}

pub(crate) fn fallback(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !(mref_class.starts_with("Landroid/view/") || mref_class.starts_with("Landroid/widget/")) {
        return Ok(None);
    }
    let Some(HostObj::View { x, y, w, h, .. }) = vm.fw.get(recv).cloned() else {
        return Ok(None);
    };
    if EXCLUDED.contains(&name) {
        return Ok(None);
    }
    let ret = ret_type(sig);
    // geometry from the host layout
    let geo = match (name, sig) {
        ("getWidth" | "getMeasuredWidth", "()I") => Some(w),
        ("getHeight" | "getMeasuredHeight", "()I") => Some(h),
        ("getLeft", "()I") => Some(x),
        ("getTop", "()I") => Some(y),
        ("getRight", "()I") => Some(x + w),
        ("getBottom", "()I") => Some(y + h),
        _ => None,
    };
    if let Some(g) = geo {
        return Ok(Some(Value::Int(g)));
    }
    // super.onXxx(...) from a subclass: the AOSP View/ViewGroup callback
    // hooks have no observable base behaviour headless (onAttachedToWindow,
    // onDetachedFromWindow, onSizeChanged, onDraw, onLayout, onFinishInflate,
    // onVisibilityChanged, …). Z-returning ones (onTouchEvent, onKeyDown, …)
    // report "not consumed".
    if name.len() > 2
        && name.starts_with("on")
        && name[2..].starts_with(|c: char| c.is_ascii_uppercase())
    {
        match ret {
            "V" if name == "onMeasure" => {
                // View.onMeasure default: setMeasuredDimension(getDefaultSize)
                for (i, k) in ["MeasuredWidth", "MeasuredHeight"].iter().enumerate() {
                    let spec = args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
                    vm.view_props
                        .insert((recv, k.to_string()), Value::Int(spec & 0x3fff_ffff));
                }
                return Ok(Some(Value::Null));
            }
            "V" => return Ok(Some(Value::Null)),
            "Z" => return Ok(Some(Value::Int(0))),
            "Landroid/os/Parcelable;" if name == "onSaveInstanceState" => {
                return Ok(Some(Value::Null))
            }
            _ => {}
        }
    }
    if ret == "V" && matches!(name, "requestLayout" | "forceLayout") {
        // requestLayout → ViewRootImpl schedules a layout traversal. Plain
        // invalidate() only dirties the display list: headless there is
        // nothing to draw until a screenshot records a frame, so it does
        // not schedule work (and avoids per-vsync relayout churn).
        vm.fw.traversal_pending = true;
    }
    if ret == "V" && args.is_empty() && NOOP_ACTIONS.contains(&name) {
        return Ok(Some(Value::Null));
    }
    // setTag(int, Object) / getTag(int)
    if name == "setTag" && sig == "(ILjava/lang/Object;)V" {
        let key = format!("Tag#{}", args[0].as_int()?);
        vm.view_props.insert((recv, key), args[1].clone());
        return Ok(Some(Value::Null));
    }
    if name == "getTag" && sig == "(I)Ljava/lang/Object;" {
        let key = format!("Tag#{}", args[0].as_int()?);
        return Ok(Some(
            vm.view_props
                .get(&(recv, key))
                .cloned()
                .unwrap_or(Value::Null),
        ));
    }
    // clearXxx() undoes setXxx(..) (clearViewTranslationCallback,
    // clearAnimation-style resets of stored properties)
    if let Some(prop) = name.strip_prefix("clear") {
        if ret == "V" && args.is_empty() && prop.starts_with(|c: char| c.is_ascii_uppercase()) {
            vm.view_props.remove(&(recv, prop.to_string()));
            return Ok(Some(Value::Null));
        }
    }
    // add/removeXxxListener|Callback(l): registrations with no headless
    // event source (layout-change, unhandled-key, insets-animation, …)
    if ret == "V"
        && args.len() == 1
        && (name.starts_with("add") || name.starts_with("remove"))
        && (name.ends_with("Listener") || name.ends_with("Callback"))
    {
        return Ok(Some(Value::Null));
    }
    match (name, sig) {
        ("scrollTo", "(II)V") | ("scrollBy", "(II)V") => {
            for (i, k) in ["ScrollX", "ScrollY"].iter().enumerate() {
                let d = args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
                let key = (recv, k.to_string());
                let base = if name == "scrollBy" {
                    vm.view_props
                        .get(&key)
                        .and_then(|v| v.as_int().ok())
                        .unwrap_or(0)
                } else {
                    0
                };
                vm.view_props.insert(key, Value::Int(base.wrapping_add(d)));
            }
            return Ok(Some(Value::Null));
        }
        ("suppressLayout", "(Z)V") => {
            let v = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            vm.view_props
                .insert((recv, "LayoutSuppressed".to_string()), Value::Int(v));
            return Ok(Some(Value::Null));
        }
        ("setPadding", "(IIII)V") | ("setPaddingRelative", "(IIII)V") => {
            for (i, k) in ["PaddingLeft", "PaddingTop", "PaddingRight", "PaddingBottom"]
                .iter()
                .enumerate()
            {
                let v = args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
                vm.view_props.insert((recv, k.to_string()), Value::Int(v));
            }
            return Ok(Some(Value::Null));
        }
        ("playSoundEffect", "(I)V") | ("drawableHotspotChanged", "(FF)V") => {
            return Ok(Some(Value::Null))
        }
        ("performHapticFeedback", _) => return Ok(Some(Value::Int(0))),
        _ => {}
    }
    if name == "willNotDraw" && sig == "()Z" && args.is_empty() {
        if let Some(value) = vm.view_props.get(&(recv, "WillNotDraw".to_string())) {
            return Ok(Some(value.clone()));
        }
        // Android ViewGroup defaults to skipping its own onDraw; plain View
        // does not. An explicit setWillNotDraw call is stored above/below.
        return Ok(Some(Value::Int(
            vm.cp.is_subtype(mref_class, "Landroid/view/ViewGroup;") as i32,
        )));
    }
    if let Some(prop) = name.strip_prefix("set") {
        if ret == "V" && args.len() == 1 && prop.starts_with(|c: char| c.is_ascii_uppercase()) {
            vm.view_props
                .insert((recv, prop.to_string()), args[0].clone());
            return Ok(Some(Value::Null));
        }
        return Ok(None);
    }
    let prop = ["get", "is", "has", "can", "should"]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .filter(|p| p.starts_with(|c: char| c.is_ascii_uppercase()));
    if let (Some(prop), true) = (prop, args.is_empty()) {
        if let Some(v) = vm.view_props.get(&(recv, prop.to_string())) {
            return Ok(Some(v.clone()));
        }
        if let Some(d) = default_for(prop, ret) {
            return Ok(Some(d));
        }
        if NULLABLE.contains(&name) {
            return Ok(Some(Value::Null));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_aosp() {
        assert!(matches!(default_for("Alpha", "F"), Some(Value::Float(f)) if f == 1.0));
        assert!(matches!(
            default_for("ClipChildren", "Z"),
            Some(Value::Int(1))
        ));
        assert!(matches!(default_for("Focusable", "Z"), Some(Value::Int(0))));
        assert!(matches!(
            default_for("DescendantFocusability", "I"),
            Some(Value::Int(0x40000))
        ));
        assert!(default_for("Background", "Landroid/graphics/drawable/Drawable;").is_none());
    }
}
