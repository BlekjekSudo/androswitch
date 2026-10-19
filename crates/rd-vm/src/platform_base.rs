//! Stateless platform base classes that apps subclass.
//!
//! AOSP ships many abstract or concrete-but-empty base classes whose only
//! job is to be overridden: delegates, callbacks, providers and span bases.
//! A DEX subclass calls `super.<init>()` and, for methods it doesn't
//! override, invokes the base implementation. Here the constructor is a
//! no-op, and an inherited method returns the AOSP base default:
//! - `void` → nothing;
//! - `boolean`/`int`/`long`/`float` → false/0;
//! - objects → null.
//!
//! `View$AccessibilityDelegate.getAccessibilityNodeProvider`, for example,
//! returns null and `performAccessibilityAction` returns false, as in AOSP
//! when no host view is wired to an accessibility service.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::value::Value;

const BASES: &[&str] = &[
    "Landroid/view/View$AccessibilityDelegate;",
    "Landroid/view/accessibility/AccessibilityNodeProvider;",
    "Landroid/view/ViewOutlineProvider;",
    "Landroid/view/ActionMode$Callback2;",
    "Landroid/view/GestureDetector$SimpleOnGestureListener;",
    "Landroid/view/ScaleGestureDetector$SimpleOnScaleGestureListener;",
    "Landroid/view/View$DragShadowBuilder;",
    "Landroid/view/OrientationEventListener;",
    "Landroid/view/translation/ViewTranslationCallback;",
    "Landroid/view/inputmethod/InputConnectionWrapper;",
    "Landroid/text/style/CharacterStyle;",
    "Landroid/text/style/MetricAffectingSpan;",
    "Landroid/text/style/ReplacementSpan;",
    "Landroid/text/style/ClickableSpan;",
    "Landroid/text/method/BaseMovementMethod;",
    "Landroid/graphics/drawable/Animatable2$AnimationCallback;",
    "Landroid/animation/AnimatorListenerAdapter;",
    "Landroid/net/ConnectivityManager$NetworkCallback;",
    "Landroid/os/CancellationSignal;",
    "Landroid/database/DataSetObserver;",
    "Landroid/window/OnBackAnimationCallback;",
    "Landroid/hardware/camera2/CameraDevice$StateCallback;",
    "Landroid/hardware/camera2/CameraCaptureSession$StateCallback;",
    "Landroid/hardware/camera2/CameraCaptureSession$CaptureCallback;",
    "Landroid/hardware/display/DisplayManager$DisplayListener;",
];

/// ViewOutlineProvider statics (BACKGROUND/BOUNDS/PADDED_BOUNDS) are objects
/// of the base class.
pub(crate) fn host_static(vm: &mut Engine, owner: &str, field: &str) -> Result<Option<Value>, VmExit> {
    if owner == "Landroid/view/ViewOutlineProvider;" && matches!(field, "BACKGROUND" | "BOUNDS" | "PADDED_BOUNDS") {
        let r = vm.heap.alloc_instance(owner.to_string(), Vec::new())?;
        return Ok(Some(Value::Obj(r)));
    }
    Ok(None)
}

pub(crate) fn instance_intrinsic(mref_class: &str, name: &str, sig: &str) -> Option<Value> {
    if !BASES.contains(&mref_class) {
        return None;
    }
    if name == "<init>" {
        return Some(Value::Null);
    }
    Some(match sig.rsplit(')').next().unwrap_or("V") {
        "V" => Value::Null,
        "J" => Value::Long(0),
        "F" => Value::Float(0.0),
        "D" => Value::Double(0.0),
        "Z" | "I" | "S" | "B" | "C" => Value::Int(0),
        _ => Value::Null,
    })
}

const A11Y: &str = "Landroid/view/accessibility/AccessibilityManager;";

/// `AccessibilityManager` with no accessibility service enabled (the
/// headless default): `isEnabled`/`isTouchExplorationEnabled` false, empty
/// service lists, listener registration succeeds, events are dropped.
/// MCP-driven accessibility (`a11y_*` tools) works on the view tree and does
/// not go through this manager.
pub(crate) fn a11y_manager(vm: &mut Engine, mref_class: &str, name: &str, sig: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if mref_class == "Landroid/view/autofill/AutofillManager;" {
        // autofill disabled (no autofill service on the headless device)
        return Ok(Some(match sig.rsplit(')').next().unwrap_or("V") {
            "Z" => Value::Int(0),
            "V" => Value::Null,
            _ => Value::Null,
        }));
    }
    if mref_class != A11Y {
        return Ok(None);
    }
    Ok(Some(match name {
        "isEnabled" | "isTouchExplorationEnabled" | "isAudioDescriptionRequested" => Value::Int(0),
        "getEnabledAccessibilityServiceList" | "getInstalledAccessibilityServiceList" => crate::collections::new_list(vm, "Ljava/util/ArrayList;", Vec::new())?,
        "getRecommendedTimeoutMillis" => args.first().cloned().unwrap_or(Value::Int(0)),
        n if n.starts_with("add") || n.starts_with("remove") => {
            if sig.ends_with(")Z") { Value::Int(1) } else { Value::Null }
        }
        "sendAccessibilityEvent" | "interrupt" => Value::Null,
        _ => return Ok(None),
    }))
}

/// Vibrator / VibratorManager / VibrationEffect. The virtual device has a
/// vibrator with amplitude control. Each `vibrate` is written to logcat as
/// `I/RdVibrator: …` so agents can assert haptic feedback via the MCP `logs`
/// tool. `VibrationEffect` objects carry their description in `rd_desc`.
pub(crate) fn vibrator_static(vm: &mut Engine, class: &str, name: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if class != "Landroid/os/VibrationEffect;" {
        return Ok(None);
    }
    let desc = match name {
        "createOneShot" => format!("oneShot {}ms amp={}", long(args.first()), int(args.get(1))),
        "createWaveform" => {
            let timings: Vec<String> = match args.first() {
                Some(Value::Obj(a)) => vm.heap.as_array_elems(*a).map(|(e, _)| e.iter().map(|v| long(Some(v)).to_string()).collect()).unwrap_or_default(),
                _ => Vec::new(),
            };
            format!("waveform [{}] repeat={}", timings.join(","), int(args.last()))
        }
        "createPredefined" => format!("predefined {}", int(args.first())),
        _ => return Ok(None),
    };
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    let d = crate::intrinsics::alloc_string(vm, desc)?;
    crate::err::put_field_result(vm.heap.put_field(r, "rd_desc", Value::Obj(d)))?;
    Ok(Some(Value::Obj(r)))
}

pub(crate) fn vibrator(vm: &mut Engine, mref_class: &str, name: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    match mref_class {
        "Landroid/os/VibratorManager;" => Ok(Some(match name {
            "getDefaultVibrator" | "getVibrator" => {
                let k = ("Lrd/SystemServices;".to_string(), "Landroid/os/Vibrator;".to_string());
                if let Some(v) = vm.statics.get(&k) {
                    v.clone()
                } else {
                    let r = vm.heap.alloc_instance("Landroid/os/Vibrator;".into(), Vec::new())?;
                    vm.statics.insert(k, Value::Obj(r));
                    Value::Obj(r)
                }
            }
            "getVibratorIds" => crate::arrays::new_array(vm, "[I", crate::heap::ElemKind::Int, vec![Value::Int(0)])?,
            "vibrate" | "cancel" => Value::Null,
            _ => return Ok(None),
        })),
        "Landroid/os/Vibrator;" => Ok(Some(match name {
            "hasVibrator" | "hasAmplitudeControl" => Value::Int(1),
            "vibrate" => {
                let desc = match args.first() {
                    Some(Value::Obj(e)) => match vm.heap.get_field(*e, "rd_desc") {
                        Ok(Value::Obj(s)) => vm.heap.as_str(s).map(str::to_string).unwrap_or_default(),
                        _ => "effect".into(),
                    },
                    Some(Value::Long(ms)) => format!("oneShot {ms}ms"),
                    _ => "effect".into(),
                };
                crate::intrinsics::host_print(1, &format!("I/RdVibrator: vibrate {desc}\n"));
                Value::Null
            }
            "cancel" => Value::Null,
            "getId" => Value::Int(0),
            "areAllPrimitivesSupported" | "isPrimitiveSupported" => Value::Int(0),
            _ => return Ok(None),
        })),
        _ => Ok(None),
    }
}

fn long(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(i)) => *i as i64,
        _ => 0,
    }
}
fn int(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Int(i)) => *i,
        _ => 0,
    }
}
