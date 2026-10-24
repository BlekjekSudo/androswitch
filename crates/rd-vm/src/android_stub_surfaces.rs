//! Bounded, guest-visible implementations for five surfaces that were
//! previously only registry `stub`s.
//!
//! The implementations in this module deliberately model small, complete
//! slices of the AOSP contracts:
//!
//! * `android.webkit.WebView` navigation history and `WebSettings` state.
//!   There is no renderer, network fetch, JavaScript, DOM, or WebViewClient
//!   callback delivery.
//! * `android.provider.DocumentsContract` URI construction/parsing, query
//!   argument matching, and `Path` values.  No DocumentsProvider/ContentResolver
//!   data access is modelled.
//! * `android.hardware.camera2` on a device profile with no camera service,
//!   plus metadata keys and explicitly-populated metadata containers.
//! * `android.hardware.camera2.params` value objects and stream-map
//!   calculations over caller-supplied configurations.
//! * deterministic no-key `android.security.NetworkSecurityPolicy` and
//!   `KeyChain` behavior, without a Keystore/KeyChain service.
//! * `android.view.textclassifier` value builders/results and the exact
//!   `TextClassifier.NO_OP` default behavior, without an ML model.
//! * `android.service.autofill` Dataset/FillResponse/SaveInfo/UserData value
//!   objects, validators/sanitizers, requests, and one-shot callbacks. There
//!   is no AutofillManager session, AssistStructure capture, credential UI,
//!   or cross-process service binding in this slice.
//! * `android.transition` capture/matching plus `Visibility`, `Fade`,
//!   `ChangeBounds`, `TransitionSet`, `Scene`, and delayed transitions on
//!   the deterministic frame clock. Overlay snapshot copies, shared-element
//!   window handoff, explode/slide/arc motions, and XML inflation are out.
//! * `android.system` file/stat/descriptor operations over the VM's
//!   in-memory VFS, plus `ErrnoException` and struct value objects. There is
//!   no host syscall passthrough, socket/process/memory operation, symlink,
//!   hard-link, pipe, or ByteBuffer overload in this slice.
//! * `android.animation` `ValueAnimator`/`ObjectAnimator` timing on the
//!   deterministic virtual Choreographer clock, with keyframes, evaluators,
//!   listeners and JavaBeans/Property application. AnimatorSet, Path and
//!   multi-parameter holders are outside this slice.

use crate::android_sysservices::*;
use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::HostObj;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const WEBVIEW: &str = "Landroid/webkit/WebView;";
const WEB_SETTINGS: &str = "Landroid/webkit/WebSettings;";
const DOCUMENTS_CONTRACT: &str = "Landroid/provider/DocumentsContract;";
const DOCUMENTS_PATH: &str = "Landroid/provider/DocumentsContract$Path;";
const NETWORK_SECURITY_POLICY: &str = "Landroid/security/NetworkSecurityPolicy;";
const KEY_CHAIN: &str = "Landroid/security/KeyChain;";
const KEY_CHAIN_CALLBACK: &str = "Landroid/security/KeyChainAliasCallback;";
const CAMERA_MANAGER: &str = "Landroid/hardware/camera2/CameraManager;";
const CAMERA_ACCESS_EXCEPTION: &str = "Landroid/hardware/camera2/CameraAccessException;";
const CAMERA_METADATA_NATIVE: &str = "Landroid/hardware/camera2/impl/CameraMetadataNative;";
const CAMERA_METADATA_NATIVE_KEY: &str = "Landroid/hardware/camera2/impl/CameraMetadataNative$Key;";
const TYPE_REFERENCE: &str = "Landroid/hardware/camera2/utils/TypeReference;";
const CAMERA_CHARACTERISTICS: &str = "Landroid/hardware/camera2/CameraCharacteristics;";
const CAMERA_CHARACTERISTICS_KEY: &str = "Landroid/hardware/camera2/CameraCharacteristics$Key;";
const CAPTURE_REQUEST: &str = "Landroid/hardware/camera2/CaptureRequest;";
const CAPTURE_REQUEST_KEY: &str = "Landroid/hardware/camera2/CaptureRequest$Key;";
const CAPTURE_REQUEST_BUILDER: &str = "Landroid/hardware/camera2/CaptureRequest$Builder;";
const CAPTURE_RESULT: &str = "Landroid/hardware/camera2/CaptureResult;";
const CAPTURE_RESULT_KEY: &str = "Landroid/hardware/camera2/CaptureResult$Key;";
const TOTAL_CAPTURE_RESULT: &str = "Landroid/hardware/camera2/TotalCaptureResult;";
const STREAM_CONFIGURATION: &str = "Landroid/hardware/camera2/params/StreamConfiguration;";
const STREAM_CONFIGURATION_DURATION: &str =
    "Landroid/hardware/camera2/params/StreamConfigurationDuration;";
const STREAM_CONFIGURATION_MAP: &str = "Landroid/hardware/camera2/params/StreamConfigurationMap;";
const HIGH_SPEED_VIDEO_CONFIGURATION: &str =
    "Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;";
const REPROCESS_FORMATS_MAP: &str = "Landroid/hardware/camera2/params/ReprocessFormatsMap;";
const METERING_RECTANGLE: &str = "Landroid/hardware/camera2/params/MeteringRectangle;";
const RGGB_CHANNEL_VECTOR: &str = "Landroid/hardware/camera2/params/RggbChannelVector;";
const BLACK_LEVEL_PATTERN: &str = "Landroid/hardware/camera2/params/BlackLevelPattern;";
const COLOR_SPACE_TRANSFORM: &str = "Landroid/hardware/camera2/params/ColorSpaceTransform;";
const HASH_CODE_HELPERS: &str = "Landroid/hardware/camera2/utils/HashCodeHelpers;";
const TEXT_CLASSIFIER: &str = "Landroid/view/textclassifier/TextClassifier;";
const TEXT_CLASSIFIER_ENTITY_CONFIG: &str =
    "Landroid/view/textclassifier/TextClassifier$EntityConfig;";
const TEXT_CLASSIFIER_ENTITY_CONFIG_BUILDER: &str =
    "Landroid/view/textclassifier/TextClassifier$EntityConfig$Builder;";
const TEXT_CLASSIFICATION: &str = "Landroid/view/textclassifier/TextClassification;";
const TEXT_CLASSIFICATION_BUILDER: &str =
    "Landroid/view/textclassifier/TextClassification$Builder;";
const TEXT_CLASSIFICATION_REQUEST: &str =
    "Landroid/view/textclassifier/TextClassification$Request;";
const TEXT_CLASSIFICATION_REQUEST_BUILDER: &str =
    "Landroid/view/textclassifier/TextClassification$Request$Builder;";
const TEXT_SELECTION: &str = "Landroid/view/textclassifier/TextSelection;";
const TEXT_SELECTION_BUILDER: &str = "Landroid/view/textclassifier/TextSelection$Builder;";
const TEXT_SELECTION_REQUEST: &str = "Landroid/view/textclassifier/TextSelection$Request;";
const TEXT_SELECTION_REQUEST_BUILDER: &str =
    "Landroid/view/textclassifier/TextSelection$Request$Builder;";
const TEXT_LANGUAGE: &str = "Landroid/view/textclassifier/TextLanguage;";
const TEXT_LANGUAGE_BUILDER: &str = "Landroid/view/textclassifier/TextLanguage$Builder;";
const TEXT_LANGUAGE_REQUEST: &str = "Landroid/view/textclassifier/TextLanguage$Request;";
const TEXT_LANGUAGE_REQUEST_BUILDER: &str =
    "Landroid/view/textclassifier/TextLanguage$Request$Builder;";
const TEXT_LINKS: &str = "Landroid/view/textclassifier/TextLinks;";
const TEXT_LINKS_BUILDER: &str = "Landroid/view/textclassifier/TextLinks$Builder;";
const TEXT_LINK: &str = "Landroid/view/textclassifier/TextLinks$TextLink;";
const TEXT_LINKS_REQUEST: &str = "Landroid/view/textclassifier/TextLinks$Request;";
const TEXT_LINKS_REQUEST_BUILDER: &str = "Landroid/view/textclassifier/TextLinks$Request$Builder;";
const CONVERSATION_ACTIONS: &str = "Landroid/view/textclassifier/ConversationActions;";
const CONVERSATION_ACTION: &str = "Landroid/view/textclassifier/ConversationAction;";
const CONVERSATION_ACTION_BUILDER: &str =
    "Landroid/view/textclassifier/ConversationAction$Builder;";
const CONVERSATION_MESSAGE: &str = "Landroid/view/textclassifier/ConversationActions$Message;";
const CONVERSATION_MESSAGE_BUILDER: &str =
    "Landroid/view/textclassifier/ConversationActions$Message$Builder;";
const CONVERSATION_REQUEST: &str = "Landroid/view/textclassifier/ConversationActions$Request;";
const CONVERSATION_REQUEST_BUILDER: &str =
    "Landroid/view/textclassifier/ConversationActions$Request$Builder;";
const TEXT_CLASSIFICATION_MANAGER: &str = "Landroid/view/textclassifier/TextClassificationManager;";
const ULOCALE: &str = "Landroid/icu/util/ULocale;";
const SYSTEM_TEXT_CLASSIFIER: &str = TEXT_CLASSIFIER;
const ENTITY_CONFIDENCE: &str = "Landroid/view/textclassifier/EntityConfidence;";

const ANIMATOR: &str = "Landroid/animation/Animator;";
const VALUE_ANIMATOR: &str = "Landroid/animation/ValueAnimator;";
const OBJECT_ANIMATOR: &str = "Landroid/animation/ObjectAnimator;";
const PROPERTY_VALUES_HOLDER: &str = "Landroid/animation/PropertyValuesHolder;";
const KEYFRAME: &str = "Landroid/animation/Keyframe;";
const KEYFRAME_OBJECT: &str = "Landroid/animation/Keyframe$ObjectKeyframe;";
const KEYFRAME_INT: &str = "Landroid/animation/Keyframe$IntKeyframe;";
const KEYFRAME_FLOAT: &str = "Landroid/animation/Keyframe$FloatKeyframe;";
const ANIMATION_HANDLER: &str = "Landroid/animation/AnimationHandler;";
const INT_EVALUATOR: &str = "Landroid/animation/IntEvaluator;";
const FLOAT_EVALUATOR: &str = "Landroid/animation/FloatEvaluator;";
const ARGB_EVALUATOR: &str = "Landroid/animation/ArgbEvaluator;";
const ANIMATOR_LISTENER: &str = "Landroid/animation/Animator$AnimatorListener;";
const ANIMATOR_PAUSE_LISTENER: &str = "Landroid/animation/Animator$AnimatorPauseListener;";
const ANIMATOR_UPDATE_LISTENER: &str = "Landroid/animation/ValueAnimator$AnimatorUpdateListener;";
const DURATION_SCALE_LISTENER: &str =
    "Landroid/animation/ValueAnimator$DurationScaleChangeListener;";
const ANIMATOR_LISTENER_ADAPTER: &str = "Landroid/animation/AnimatorListenerAdapter;";
const ANDROID_PROPERTY: &str = "Landroid/util/Property;";
const ANDROID_INT_PROPERTY: &str = "Landroid/util/IntProperty;";
const ANDROID_FLOAT_PROPERTY: &str = "Landroid/util/FloatProperty;";

const AUTOFILL_ID: &str = "Landroid/view/autofill/AutofillId;";
const AUTOFILL_VALUE: &str = "Landroid/view/autofill/AutofillValue;";
const AUTOFILL_SERVICE: &str = "Landroid/service/autofill/AutofillService;";
const AF_DATASET: &str = "Landroid/service/autofill/Dataset;";
const AF_DATASET_BUILDER: &str = "Landroid/service/autofill/Dataset$Builder;";
const AF_DATASET_FILTER: &str = "Landroid/service/autofill/Dataset$DatasetFieldFilter;";
const AF_FIELD: &str = "Landroid/service/autofill/Field;";
const AF_FIELD_BUILDER: &str = "Landroid/service/autofill/Field$Builder;";
const AF_PRESENTATIONS: &str = "Landroid/service/autofill/Presentations;";
const AF_PRESENTATIONS_BUILDER: &str = "Landroid/service/autofill/Presentations$Builder;";
const AF_INLINE_PRESENTATION: &str = "Landroid/service/autofill/InlinePresentation;";
const AF_FILL_RESPONSE: &str = "Landroid/service/autofill/FillResponse;";
const AF_FILL_RESPONSE_BUILDER: &str = "Landroid/service/autofill/FillResponse$Builder;";
const AF_SAVE_INFO: &str = "Landroid/service/autofill/SaveInfo;";
const AF_SAVE_INFO_BUILDER: &str = "Landroid/service/autofill/SaveInfo$Builder;";
const AF_USER_DATA: &str = "Landroid/service/autofill/UserData;";
const AF_USER_DATA_BUILDER: &str = "Landroid/service/autofill/UserData$Builder;";
const AF_VALIDATORS: &str = "Landroid/service/autofill/Validators;";
const AF_INTERNAL_VALIDATOR: &str = "Landroid/service/autofill/InternalValidator;";
const AF_REGEX_VALIDATOR: &str = "Landroid/service/autofill/RegexValidator;";
const AF_REQUIRED_VALIDATORS: &str = "Landroid/service/autofill/RequiredValidators;";
const AF_OPTIONAL_VALIDATORS: &str = "Landroid/service/autofill/OptionalValidators;";
const AF_NEGATION_VALIDATOR: &str = "Landroid/service/autofill/NegationValidator;";
const AF_INTERNAL_SANITIZER: &str = "Landroid/service/autofill/InternalSanitizer;";
const AF_TEXT_SANITIZER: &str = "Landroid/service/autofill/TextValueSanitizer;";
const AF_DATE_SANITIZER: &str = "Landroid/service/autofill/DateValueSanitizer;";
const AF_FILL_REQUEST: &str = "Landroid/service/autofill/FillRequest;";
const AF_SAVE_REQUEST: &str = "Landroid/service/autofill/SaveRequest;";
const AF_FILL_CONTEXT: &str = "Landroid/service/autofill/FillContext;";
const AF_FILL_CALLBACK: &str = "Landroid/service/autofill/FillCallback;";
const AF_SAVE_CALLBACK: &str = "Landroid/service/autofill/SaveCallback;";
const AF_FIELD_CLASSIFICATION: &str = "Landroid/service/autofill/FieldClassification;";
const AF_FIELD_CLASSIFICATION_MATCH: &str = "Landroid/service/autofill/FieldClassification$Match;";
const AF_SAVED_DATASETS_INFO: &str = "Landroid/service/autofill/SavedDatasetsInfo;";
const AF_SAVED_DATASETS_CALLBACK: &str = "Landroid/service/autofill/SavedDatasetsInfoCallback;";

const TRANSITION: &str = "Landroid/transition/Transition;";
const TRANSITION_LISTENER: &str = "Landroid/transition/Transition$TransitionListener;";
const TRANSITION_EPICENTER_CALLBACK: &str = "Landroid/transition/Transition$EpicenterCallback;";
const TRANSITION_ANIMATION_INFO: &str = "Landroid/transition/Transition$AnimationInfo;";
const TRANSITION_VALUES: &str = "Landroid/transition/TransitionValues;";
const TRANSITION_VALUES_MAPS: &str = "Landroid/transition/TransitionValuesMaps;";
const TRANSITION_VISIBILITY: &str = "Landroid/transition/Visibility;";
const TRANSITION_FADE: &str = "Landroid/transition/Fade;";
const TRANSITION_CHANGE_BOUNDS: &str = "Landroid/transition/ChangeBounds;";
const TRANSITION_SET: &str = "Landroid/transition/TransitionSet;";
const TRANSITION_AUTO: &str = "Landroid/transition/AutoTransition;";
const TRANSITION_MANAGER: &str = "Landroid/transition/TransitionManager;";
const TRANSITION_SCENE: &str = "Landroid/transition/Scene;";
const TRANSITION_PROPAGATION: &str = "Landroid/transition/TransitionPropagation;";
const TRANSITION_PATH_MOTION: &str = "Landroid/transition/PathMotion;";
const TRANSITION_LISTENER_ADAPTER: &str = "Landroid/transition/TransitionListenerAdapter;";
const TRANSITION_STRAIGHT_PATH: &str = "Landroid/transition/Transition$1;";
const TRANSITION_RUN_LISTENER: &str = "Landroid/transition/Transition$2;";
const TRANSITION_ANIMATE_LISTENER: &str = "Landroid/transition/Transition$3;";
const TRANSITION_SET_LISTENER: &str = "Landroid/transition/TransitionSet$TransitionSetListener;";
const TRANSITION_SET_NEXT_LISTENER: &str = "Landroid/transition/TransitionSet$1;";
const TRANSITION_MANAGER_MULTI: &str = "Landroid/transition/TransitionManager$MultiListener;";
const TRANSITION_MANAGER_END_LISTENER: &str = "Landroid/transition/TransitionManager$1;";
const TRANSITION_FADE_LISTENER: &str = "Landroid/transition/Fade$1;";
const TRANSITION_FADE_ANIM_LISTENER: &str = "Landroid/transition/Fade$FadeAnimatorListener;";
const TRANSITION_DISAPPEAR_LISTENER: &str = "Landroid/transition/Visibility$DisappearListener;";
const TRANSITION_VISIBILITY_OVERLAY_LISTENER: &str = "Landroid/transition/Visibility$1;";
const TRANSITION_CHANGE_BOUNDS_LISTENER: &str = "Landroid/transition/ChangeBounds$1;";

const ANDROID_OS: &str = "Landroid/system/Os;";
const ANDROID_OS_CONSTANTS: &str = "Landroid/system/OsConstants;";
const ANDROID_ERRNO: &str = "Landroid/system/ErrnoException;";
const ANDROID_STRUCT_STAT: &str = "Landroid/system/StructStat;";
const ANDROID_STRUCT_STAT_VFS: &str = "Landroid/system/StructStatVfs;";
const ANDROID_STRUCT_TIMESPEC: &str = "Landroid/system/StructTimespec;";
const ANDROID_STRUCT_UTSNAME: &str = "Landroid/system/StructUtsname;";
const JAVA_FILE_DESCRIPTOR: &str = "Ljava/io/FileDescriptor;";

fn ise(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/IllegalStateException;", msg)
}
fn oob(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/ArrayIndexOutOfBoundsException;", msg)
}
fn idx_oob(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/IndexOutOfBoundsException;", msg)
}
fn assertion(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/AssertionError;", msg)
}
fn class_of(vm: &Engine, r: ObjRef) -> String {
    vm.heap.class_of(r).unwrap_or("").to_string()
}
fn obj(v: Option<&Value>) -> Option<ObjRef> {
    match v {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn req_obj(args: &[Value], i: usize, what: &str) -> Result<ObjRef, VmExit> {
    oarg(args, i).ok_or_else(|| npe(what))
}
fn req_str(vm: &Engine, args: &[Value], i: usize, what: &str) -> Result<String, VmExit> {
    opt_str(vm, args.get(i)).ok_or_else(|| npe(what))
}
fn opt_str_arg(vm: &Engine, args: &[Value], i: usize) -> Option<String> {
    opt_str(vm, args.get(i))
}
fn field_int(vm: &Engine, r: ObjRef, f: &str) -> i32 {
    get(vm, r, f).as_int().unwrap_or(0)
}
fn field_long(vm: &Engine, r: ObjRef, f: &str) -> i64 {
    get(vm, r, f).as_long().unwrap_or(0)
}
fn field_float(vm: &Engine, r: ObjRef, f: &str) -> f32 {
    get(vm, r, f).as_float().unwrap_or(0.0)
}
fn field_bool(vm: &Engine, r: ObjRef, f: &str) -> bool {
    field_int(vm, r, f) != 0
}
fn field_obj(vm: &Engine, r: ObjRef, f: &str) -> Option<ObjRef> {
    match get(vm, r, f) {
        Value::Obj(o) => Some(o),
        _ => None,
    }
}
fn array_elems(vm: &Engine, v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Obj(r)) => vm
            .heap
            .as_array_elems(*r)
            .map(|(e, _)| e.to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn array_is_obj(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Obj(_)))
}
fn int_values(vm: &Engine, v: Option<&Value>) -> Vec<i32> {
    array_elems(vm, v)
        .iter()
        .map(|x| x.as_int().unwrap_or(0))
        .collect()
}
fn float_values(vm: &Engine, v: Option<&Value>) -> Vec<f32> {
    array_elems(vm, v)
        .iter()
        .map(|x| x.as_float().unwrap_or(0.0))
        .collect()
}
fn new_int_array(vm: &mut Engine, vals: &[i32]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[I",
        ElemKind::Int,
        vals.iter().copied().map(Value::Int).collect(),
    )
}
fn new_float_array(vm: &mut Engine, vals: &[f32]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[F",
        ElemKind::Float,
        vals.iter().copied().map(Value::Float).collect(),
    )
}
fn new_collection(vm: &mut Engine, class: &str, elems: Vec<Value>) -> Result<ObjRef, VmExit> {
    let r = alloc_obj(vm, class, Vec::new())?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(r)
}
fn collection_items(vm: &Engine, v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Obj(r)) => match vm.fw.objects.get(r) {
            Some(HostObj::List { elems }) => elems.clone(),
            Some(HostObj::Map { entries }) => entries.iter().map(|(k, _)| k.clone()).collect(),
            _ => {
                let arr = array_elems(vm, v);
                if !arr.is_empty() {
                    arr
                } else if let Some(items) = field_obj(vm, *r, "rd_items") {
                    array_elems(vm, Some(&Value::Obj(items)))
                } else {
                    Vec::new()
                }
            }
        },
        _ => Vec::new(),
    }
}
fn map_entries(vm: &Engine, v: Option<&Value>) -> Vec<(Value, Value)> {
    match v {
        Some(Value::Obj(r)) => match vm.fw.objects.get(r) {
            Some(HostObj::Map { entries }) => entries.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}
fn empty_bundle(vm: &mut Engine) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Landroid/os/Bundle;", Vec::new())?;
    vm.fw.objects.insert(r, HostObj::Bundle { map: Vec::new() });
    Ok(Value::Obj(r))
}
fn bundle_or_empty(vm: &mut Engine, v: Option<&Value>) -> Result<Value, VmExit> {
    match v {
        Some(Value::Obj(_)) => Ok(v.cloned().unwrap_or(Value::Null)),
        _ => empty_bundle(vm),
    }
}
fn identity_hash(r: ObjRef) -> i32 {
    (r as i32).wrapping_mul(0x9E37_79B1u32 as i32)
}
fn host_hash(vm: &Engine, v: &Value) -> i32 {
    match v {
        Value::Int(i) => *i,
        Value::Long(l) => ((*l as u64) ^ ((*l as u64) >> 32)) as i32,
        Value::Float(f) => f.to_bits() as i32,
        Value::Double(d) => {
            let b = d.to_bits();
            ((b ^ (b >> 32)) & 0xffff_ffff) as i32
        }
        Value::Obj(r) => {
            if let Ok(s) = vm.heap.as_str(*r) {
                java_string_hash(s)
            } else {
                identity_hash(*r)
            }
        }
        Value::Null => 0,
        _ => 0,
    }
}
fn bernstein_ints(vals: &[i32]) -> i32 {
    let mut h: i32 = 1;
    for x in vals {
        h = ((h << 5).wrapping_sub(h)) ^ *x;
    }
    h
}
fn objects_equal(vm: &Engine, a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Obj(x), Value::Obj(y)) => {
            if x == y {
                return true;
            }
            match (vm.heap.as_str(*x), vm.heap.as_str(*y)) {
                (Ok(s), Ok(t)) => s == t,
                _ => false,
            }
        }
        _ => a == b,
    }
}

fn value_string(vm: &mut Engine, v: &Value) -> Result<String, VmExit> {
    Ok(match v {
        Value::Null => "null".to_string(),
        Value::Obj(r) => crate::intrinsics::object_to_string(vm, *r)?,
        Value::Int(i) => i.to_string(),
        Value::Long(l) => l.to_string(),
        Value::Float(f) => crate::repr::java_float(*f),
        Value::Double(d) => crate::repr::java_double(*d),
        _ => "null".to_string(),
    })
}

pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    if owner == ANDROID_OS_CONSTANTS {
        if let Some(value) = android_os_constant(field) {
            return Ok(Some(Value::Int(value)));
        }
    }
    if let Some(value) = partial57_host_static(vm, owner, field)? {
        return Ok(Some(value));
    }
    let v = match (owner, field) {
        (WEB_SETTINGS, "LOAD_DEFAULT") => Value::Int(-1),
        (WEB_SETTINGS, "LOAD_NORMAL") => Value::Int(0),
        (WEB_SETTINGS, "LOAD_CACHE_ELSE_NETWORK") => Value::Int(1),
        (WEB_SETTINGS, "LOAD_NO_CACHE") => Value::Int(2),
        (WEB_SETTINGS, "LOAD_CACHE_ONLY") => Value::Int(3),
        (WEB_SETTINGS, "MIXED_CONTENT_ALWAYS_ALLOW") => Value::Int(0),
        (WEB_SETTINGS, "MIXED_CONTENT_NEVER_ALLOW") => Value::Int(1),
        (WEB_SETTINGS, "MIXED_CONTENT_COMPATIBILITY_MODE") => Value::Int(2),
        (WEB_SETTINGS, "FORCE_DARK_OFF") => Value::Int(0),
        (WEB_SETTINGS, "FORCE_DARK_AUTO") => Value::Int(1),
        (WEB_SETTINGS, "FORCE_DARK_ON") => Value::Int(2),
        (WEB_SETTINGS, "MENU_ITEM_NONE") => Value::Int(0),
        (WEB_SETTINGS, "MENU_ITEM_SHARE") => Value::Int(1),
        (WEB_SETTINGS, "MENU_ITEM_WEB_SEARCH") => Value::Int(2),
        (WEB_SETTINGS, "MENU_ITEM_PROCESS_TEXT") => Value::Int(4),
        (CAMERA_ACCESS_EXCEPTION, "CAMERA_DISABLED") => Value::Int(1),
        (CAMERA_ACCESS_EXCEPTION, "CAMERA_DISCONNECTED") => Value::Int(2),
        (CAMERA_ACCESS_EXCEPTION, "CAMERA_ERROR") => Value::Int(3),
        (CAMERA_ACCESS_EXCEPTION, "CAMERA_IN_USE") => Value::Int(4),
        (CAMERA_ACCESS_EXCEPTION, "MAX_CAMERAS_IN_USE") => Value::Int(5),
        (CAMERA_ACCESS_EXCEPTION, "CAMERA_DEPRECATED_HAL") => Value::Int(1000),
        (METERING_RECTANGLE, "METERING_WEIGHT_MIN") => Value::Int(0),
        (METERING_RECTANGLE, "METERING_WEIGHT_MAX") => Value::Int(1000),
        (METERING_RECTANGLE, "METERING_WEIGHT_DONT_CARE") => Value::Int(0),
        (RGGB_CHANNEL_VECTOR, "COUNT") => Value::Int(4),
        (RGGB_CHANNEL_VECTOR, "RED") => Value::Int(0),
        (RGGB_CHANNEL_VECTOR, "GREEN_EVEN") => Value::Int(1),
        (RGGB_CHANNEL_VECTOR, "GREEN_ODD") => Value::Int(2),
        (RGGB_CHANNEL_VECTOR, "BLUE") => Value::Int(3),
        (BLACK_LEVEL_PATTERN, "COUNT") => Value::Int(4),
        (STREAM_CONFIGURATION_MAP, "HAL_PIXEL_FORMAT_BLOB") => Value::Int(0x21),
        (STREAM_CONFIGURATION_MAP, "HAL_DATASPACE_V0_JFIF") => {
            Value::Int((2 << 16) | (3 << 22) | (1 << 27))
        }
        (STREAM_CONFIGURATION_MAP, "HAL_DATASPACE_DEPTH") => Value::Int(0x1000),
        (STREAM_CONFIGURATION_MAP, "HAL_DATASPACE_DYNAMIC_DEPTH") => Value::Int(0x1002),
        (STREAM_CONFIGURATION_MAP, "HAL_DATASPACE_HEIF") => Value::Int(0x1004),
        (STREAM_CONFIGURATION_MAP, "HAL_DATASPACE_JPEG_R") => Value::Int(0x1005),
        (TEXT_CLASSIFIER, "TYPE_UNKNOWN") => return jstr(vm, "unknown").map(Some),
        (TEXT_CLASSIFIER, "TYPE_OTHER") => return jstr(vm, "other").map(Some),
        (TEXT_CLASSIFIER, "TYPE_EMAIL") => return jstr(vm, "email").map(Some),
        (TEXT_CLASSIFIER, "TYPE_PHONE") => return jstr(vm, "phone").map(Some),
        (TEXT_CLASSIFIER, "TYPE_ADDRESS") => return jstr(vm, "address").map(Some),
        (TEXT_CLASSIFIER, "TYPE_URL") => return jstr(vm, "url").map(Some),
        (TEXT_CLASSIFIER, "TYPE_DATE") => return jstr(vm, "date").map(Some),
        (TEXT_CLASSIFIER, "TYPE_DATE_TIME") => return jstr(vm, "datetime").map(Some),
        (TEXT_CLASSIFIER, "TYPE_FLIGHT_NUMBER") => return jstr(vm, "flight").map(Some),
        (TEXT_CLASSIFIER, "EXTRA_FROM_TEXT_CLASSIFIER") => {
            return jstr(vm, "android.view.textclassifier.extra.FROM_TEXT_CLASSIFIER").map(Some)
        }
        (TEXT_CLASSIFIER, "NO_OP") => return text_no_op(vm).map(Some),
        (ANIMATOR | VALUE_ANIMATOR | OBJECT_ANIMATOR, "DURATION_INFINITE") => Value::Long(-1),
        (VALUE_ANIMATOR | OBJECT_ANIMATOR, "INFINITE") => Value::Int(-1),
        (VALUE_ANIMATOR | OBJECT_ANIMATOR, "RESTART") => Value::Int(1),
        (VALUE_ANIMATOR | OBJECT_ANIMATOR, "REVERSE") => Value::Int(2),
        (AUTOFILL_ID, "NO_SESSION") => Value::Int(0),
        (AUTOFILL_ID, "NO_AUTOFILL_ID") => return af_no_autofill_id(vm).map(Some),
        (AF_DATASET, "PICK_REASON_UNKNOWN") => Value::Int(0),
        (AF_DATASET, "PICK_REASON_NO_PCC") => Value::Int(1),
        (AF_DATASET, "PICK_REASON_PROVIDER_DETECTION_ONLY") => Value::Int(2),
        (AF_DATASET, "PICK_REASON_PROVIDER_DETECTION_PREFERRED_WITH_PCC") => Value::Int(3),
        (AF_DATASET, "PICK_REASON_PCC_DETECTION_ONLY") => Value::Int(4),
        (AF_DATASET, "PICK_REASON_PCC_DETECTION_PREFERRED_WITH_PROVIDER") => Value::Int(5),
        (AF_FILL_RESPONSE, "FLAG_TRACK_CONTEXT_COMMITED") => Value::Int(0x1),
        (AF_FILL_RESPONSE, "FLAG_DISABLE_ACTIVITY_ONLY") => Value::Int(0x2),
        (AF_FILL_RESPONSE, "FLAG_DELAY_FILL") => Value::Int(0x4),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_GENERIC") => Value::Int(0),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_PASSWORD") => Value::Int(0x01),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_ADDRESS") => Value::Int(0x02),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_CREDIT_CARD") => Value::Int(0x04),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_USERNAME") => Value::Int(0x08),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_EMAIL_ADDRESS") => Value::Int(0x10),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_DEBIT_CARD") => Value::Int(0x20),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_PAYMENT_CARD") => Value::Int(0x40),
        (AF_SAVE_INFO, "SAVE_DATA_TYPE_GENERIC_CARD") => Value::Int(0x80),
        (AF_SAVE_INFO, "NEGATIVE_BUTTON_STYLE_CANCEL") => Value::Int(0),
        (AF_SAVE_INFO, "NEGATIVE_BUTTON_STYLE_REJECT") => Value::Int(1),
        (AF_SAVE_INFO, "NEGATIVE_BUTTON_STYLE_NEVER") => Value::Int(2),
        (AF_SAVE_INFO, "POSITIVE_BUTTON_STYLE_SAVE") => Value::Int(0),
        (AF_SAVE_INFO, "POSITIVE_BUTTON_STYLE_CONTINUE") => Value::Int(1),
        (AF_SAVE_INFO, "FLAG_SAVE_ON_ALL_VIEWS_INVISIBLE") => Value::Int(0x1),
        (AF_SAVE_INFO, "FLAG_DONT_SAVE_ON_FINISH") => Value::Int(0x2),
        (AF_SAVE_INFO, "FLAG_DELAY_SAVE") => Value::Int(0x4),
        (AF_FILL_REQUEST, "FLAG_MANUAL_REQUEST") => Value::Int(0x1),
        (AF_FILL_REQUEST, "FLAG_COMPATIBILITY_MODE_REQUEST") => Value::Int(0x2),
        (AF_FILL_REQUEST, "FLAG_PASSWORD_INPUT_TYPE") => Value::Int(0x4),
        (AF_FILL_REQUEST, "FLAG_VIEW_NOT_FOCUSED") => Value::Int(0x10),
        (AF_FILL_REQUEST, "FLAG_SUPPORTS_FILL_DIALOG") => Value::Int(0x40),
        (AF_FILL_REQUEST, "FLAG_IME_SHOWING") => Value::Int(0x80),
        (AF_FILL_REQUEST, "FLAG_RESET_FILL_DIALOG_STATE") => Value::Int(0x100),
        (AF_FILL_REQUEST, "FLAG_PCC_DETECTION") => Value::Int(0x200),
        (AF_FILL_REQUEST, "FLAG_SCREEN_HAS_CREDMAN_FIELD") => Value::Int(0x400),
        (AF_FILL_REQUEST, "INVALID_REQUEST_ID") => Value::Int(i32::MIN),
        (AUTOFILL_SERVICE, "SERVICE_INTERFACE") => {
            return jstr(vm, "android.service.autofill.AutofillService").map(Some)
        }
        (AUTOFILL_SERVICE, "SERVICE_META_DATA") => return jstr(vm, "android.autofill").map(Some),
        (AUTOFILL_SERVICE, "EXTRA_FILL_RESPONSE") => {
            return jstr(vm, "android.service.autofill.extra.FILL_RESPONSE").map(Some)
        }
        (AUTOFILL_SERVICE, "EXTRA_RESULT") => return jstr(vm, "result").map(Some),
        (AUTOFILL_SERVICE, "EXTRA_ERROR") => return jstr(vm, "error").map(Some),
        (AF_SAVED_DATASETS_INFO, "TYPE_OTHER") => return jstr(vm, "other").map(Some),
        (AF_SAVED_DATASETS_INFO, "TYPE_PASSWORDS") => return jstr(vm, "passwords").map(Some),
        (AF_SAVED_DATASETS_CALLBACK, "ERROR_OTHER") => Value::Int(0),
        (AF_SAVED_DATASETS_CALLBACK, "ERROR_UNSUPPORTED") => Value::Int(1),
        (AF_SAVED_DATASETS_CALLBACK, "ERROR_NEEDS_USER_ACTION") => Value::Int(2),
        (TRANSITION, "MATCH_INSTANCE") => Value::Int(0x1),
        (TRANSITION, "MATCH_NAME") => Value::Int(0x2),
        (TRANSITION, "MATCH_ID") => Value::Int(0x3),
        (TRANSITION, "MATCH_ITEM_ID") => Value::Int(0x4),
        (TRANSITION_VISIBILITY, "MODE_IN") => Value::Int(0x1),
        (TRANSITION_VISIBILITY, "MODE_OUT") => Value::Int(0x2),
        (TRANSITION_FADE, "IN") => Value::Int(0x1),
        (TRANSITION_FADE, "OUT") => Value::Int(0x2),
        (TRANSITION_SET, "ORDERING_TOGETHER") => Value::Int(0),
        (TRANSITION_SET, "ORDERING_SEQUENTIAL") => Value::Int(1),
        (TEXT_CLASSIFICATION, "EMPTY") => return empty_text_classification(vm).map(Some),
        (TEXT_LANGUAGE, "EMPTY") => return empty_text_language(vm).map(Some),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(value) = partial57_static(vm, class, name, sig, args)? {
        return Ok(Some(value));
    }
    match class {
        DOCUMENTS_CONTRACT => documents_static(vm, name, sig, args),
        NETWORK_SECURITY_POLICY => network_policy_static(vm, name, sig, args),
        KEY_CHAIN => keychain_static(vm, name, sig, args),
        CAMERA_ACCESS_EXCEPTION if name == "getDefaultMessage" => {
            let msg = camera_default_message(iarg(args, 0));
            match msg {
                Some(s) => jstr(vm, s).map(Some),
                None => Ok(Some(Value::Null)),
            }
        }
        STREAM_CONFIGURATION_MAP => stream_map_static(vm, name, sig, args),
        CAMERA_METADATA_NATIVE => camera_metadata_native_static(vm, name, sig, args),
        HASH_CODE_HELPERS => hash_code_helpers_static(vm, name, sig, args),
        TYPE_REFERENCE => type_reference_static(vm, name, sig, args),
        TEXT_CLASSIFIER_ENTITY_CONFIG => entity_config_static(vm, name, sig, args),
        TEXT_CLASSIFICATION => text_classification_static(vm, name, sig, args),
        VALUE_ANIMATOR => value_animator_static(vm, name, sig, args),
        OBJECT_ANIMATOR => object_animator_static(vm, name, sig, args),
        PROPERTY_VALUES_HOLDER => property_values_holder_static(vm, name, sig, args),
        KEYFRAME => keyframe_static(vm, name, sig, args),
        ANIMATION_HANDLER => animation_handler_static(vm, name, sig, args),
        ARGB_EVALUATOR if name == "getInstance" => argb_evaluator_instance(vm).map(Some),
        AUTOFILL_ID => autofill_id_static(vm, name, sig, args),
        AUTOFILL_VALUE => autofill_value_static(vm, name, sig, args),
        AF_VALIDATORS => af_validators_static(vm, name, sig, args),
        AF_INLINE_PRESENTATION => af_inline_presentation_static(vm, name, sig, args),
        AF_USER_DATA => af_user_data_static(vm, name, sig, args),
        AF_SAVE_INFO if name == "copy" => af_save_info_copy_static(vm, args).map(Some),
        AF_FILL_RESPONSE if name == "shallowCopy" => {
            af_fill_response_copy_static(vm, args).map(Some)
        }
        ANDROID_OS => android_os_static(vm, name, sig, args),
        ANDROID_OS_CONSTANTS => android_os_constants_static(vm, name, sig, args),
        TRANSITION_MANAGER => transition_manager_static(vm, name, sig, args),
        TRANSITION_SCENE => transition_scene_static(vm, name, sig, args),
        WEBVIEW => webview_static(vm, name, sig, args),
        WEB_SETTINGS if name == "getDefaultUserAgent" => {
            // A deterministic user agent for the no-renderer profile.  Callers
            // normally use it as an opaque string; construction/parsing of a
            // real Chromium UA is outside the registered slice.
            jstr(vm, "Mozilla/5.0 (Linux; Android 14; RustyDroid) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36").map(Some)
        }
        _ => Ok(None),
    }
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
    // TextClassifier is an interface; its NO_OP object is represented by the
    // interface descriptor itself in the host profile.  Only dispatch it when
    // the receiver is that singleton (or explicitly marked), so a guest DEX
    // implementation class is not shadowed when invoked through the interface.
    if mref_class == TEXT_CLASSIFIER
        && (recv_class == TEXT_CLASSIFIER || field_bool(vm, recv, "rd_noop"))
    {
        return text_classifier_instance(vm, name, sig, recv, args);
    }
    if let Some(value) = partial57_instance(vm, recv_class, mref_class, name, sig, recv, args)? {
        return Ok(Some(value));
    }
    let class = if is_stub_class(recv_class) {
        recv_class
    } else if is_stub_class(mref_class) {
        mref_class
    } else {
        return Ok(None);
    };
    match class {
        WEBVIEW => webview_instance(vm, name, sig, recv, args),
        WEB_SETTINGS => web_settings_instance(vm, name, sig, recv, args),
        DOCUMENTS_PATH => documents_path_instance(vm, name, sig, recv, args),
        NETWORK_SECURITY_POLICY => network_policy_instance(vm, name, sig, recv, args),
        KEY_CHAIN_CALLBACK => keychain_callback_instance(vm, name, sig, recv, args),
        CAMERA_MANAGER => camera_manager_instance(vm, name, sig, recv, args),
        CAMERA_ACCESS_EXCEPTION => camera_access_exception_instance(vm, name, sig, recv, args),
        CAMERA_METADATA_NATIVE => camera_metadata_native_instance(vm, name, sig, recv, args),
        CAMERA_METADATA_NATIVE_KEY => native_key_instance(vm, name, sig, recv, args),
        TYPE_REFERENCE => type_reference_instance(vm, name, sig, recv, args),
        CAMERA_CHARACTERISTICS => camera_characteristics_instance(vm, name, sig, recv, args),
        CAMERA_CHARACTERISTICS_KEY | CAPTURE_REQUEST_KEY | CAPTURE_RESULT_KEY => {
            wrapper_key_instance(vm, class, name, sig, recv, args)
        }
        CAPTURE_REQUEST => capture_request_instance(vm, name, sig, recv, args),
        CAPTURE_REQUEST_BUILDER => capture_request_builder_instance(vm, name, sig, recv, args),
        CAPTURE_RESULT | TOTAL_CAPTURE_RESULT => capture_result_instance(vm, name, sig, recv, args),
        STREAM_CONFIGURATION => stream_configuration_instance(vm, name, sig, recv, args),
        STREAM_CONFIGURATION_DURATION => {
            stream_configuration_duration_instance(vm, name, sig, recv, args)
        }
        STREAM_CONFIGURATION_MAP => stream_map_instance(vm, name, sig, recv, args),
        HIGH_SPEED_VIDEO_CONFIGURATION => {
            high_speed_video_configuration_instance(vm, name, sig, recv, args)
        }
        REPROCESS_FORMATS_MAP => reprocess_formats_map_instance(vm, name, sig, recv, args),
        METERING_RECTANGLE => metering_rectangle_instance(vm, name, sig, recv, args),
        RGGB_CHANNEL_VECTOR => rggb_channel_vector_instance(vm, name, sig, recv, args),
        BLACK_LEVEL_PATTERN => black_level_pattern_instance(vm, name, sig, recv, args),
        COLOR_SPACE_TRANSFORM => color_space_transform_instance(vm, name, sig, recv, args),
        TEXT_CLASSIFIER_ENTITY_CONFIG => entity_config_instance(vm, name, sig, recv, args),
        TEXT_CLASSIFIER_ENTITY_CONFIG_BUILDER => {
            entity_config_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_CLASSIFICATION => text_classification_instance(vm, name, sig, recv, args),
        TEXT_CLASSIFICATION_BUILDER => {
            text_classification_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_CLASSIFICATION_REQUEST => {
            text_classification_request_instance(vm, name, sig, recv, args)
        }
        TEXT_CLASSIFICATION_REQUEST_BUILDER => {
            text_classification_request_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_SELECTION => text_selection_instance(vm, name, sig, recv, args),
        TEXT_SELECTION_BUILDER => text_selection_builder_instance(vm, name, sig, recv, args),
        TEXT_SELECTION_REQUEST => text_selection_request_instance(vm, name, sig, recv, args),
        TEXT_SELECTION_REQUEST_BUILDER => {
            text_selection_request_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_LANGUAGE => text_language_instance(vm, name, sig, recv, args),
        TEXT_LANGUAGE_BUILDER => text_language_builder_instance(vm, name, sig, recv, args),
        TEXT_LANGUAGE_REQUEST => text_language_request_instance(vm, name, sig, recv, args),
        TEXT_LANGUAGE_REQUEST_BUILDER => {
            text_language_request_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_LINKS => text_links_instance(vm, name, sig, recv, args),
        TEXT_LINKS_BUILDER => text_links_builder_instance(vm, name, sig, recv, args),
        TEXT_LINK => text_link_instance(vm, name, sig, recv, args),
        TEXT_LINKS_REQUEST => text_links_request_instance(vm, name, sig, recv, args),
        TEXT_LINKS_REQUEST_BUILDER => {
            text_links_request_builder_instance(vm, name, sig, recv, args)
        }
        CONVERSATION_ACTIONS => conversation_actions_instance(vm, name, sig, recv, args),
        CONVERSATION_ACTION => conversation_action_instance(vm, name, sig, recv, args),
        CONVERSATION_ACTION_BUILDER => {
            conversation_action_builder_instance(vm, name, sig, recv, args)
        }
        CONVERSATION_MESSAGE => conversation_message_instance(vm, name, sig, recv, args),
        CONVERSATION_MESSAGE_BUILDER => {
            conversation_message_builder_instance(vm, name, sig, recv, args)
        }
        CONVERSATION_REQUEST => conversation_request_instance(vm, name, sig, recv, args),
        CONVERSATION_REQUEST_BUILDER => {
            conversation_request_builder_instance(vm, name, sig, recv, args)
        }
        TEXT_CLASSIFICATION_MANAGER => {
            system_text_classifier_manager_instance(vm, name, sig, recv, args)
        }
        ULOCALE => ulocale_instance(vm, name, sig, recv, args),
        ENTITY_CONFIDENCE => entity_confidence_instance(vm, name, sig, recv, args),
        ANIMATOR => animator_base_instance(vm, name, sig, recv, args),
        VALUE_ANIMATOR => value_animator_instance(vm, name, sig, recv, args),
        OBJECT_ANIMATOR => object_animator_instance(vm, name, sig, recv, args),
        PROPERTY_VALUES_HOLDER => property_values_holder_instance(vm, name, sig, recv, args),
        KEYFRAME | KEYFRAME_OBJECT | KEYFRAME_INT | KEYFRAME_FLOAT => {
            keyframe_instance(vm, class, name, sig, recv, args)
        }
        ANIMATION_HANDLER => animation_handler_instance(vm, name, sig, recv, args),
        INT_EVALUATOR | FLOAT_EVALUATOR | ARGB_EVALUATOR => {
            animation_evaluator_instance(vm, class, name, sig, recv, args)
        }
        ANDROID_PROPERTY | ANDROID_INT_PROPERTY | ANDROID_FLOAT_PROPERTY => {
            android_property_instance(vm, class, name, sig, recv, args)
        }
        AUTOFILL_ID => autofill_id_instance(vm, name, sig, recv, args),
        AUTOFILL_VALUE => autofill_value_instance(vm, name, sig, recv, args),
        AF_DATASET => af_dataset_instance(vm, name, sig, recv, args),
        AF_DATASET_BUILDER => af_dataset_builder_instance(vm, name, sig, recv, args),
        AF_DATASET_FILTER => af_dataset_filter_instance(vm, name, sig, recv, args),
        AF_FIELD => af_field_instance(vm, name, sig, recv, args),
        AF_FIELD_BUILDER => af_field_builder_instance(vm, name, sig, recv, args),
        AF_PRESENTATIONS => af_presentations_instance(vm, name, sig, recv, args),
        AF_PRESENTATIONS_BUILDER => af_presentations_builder_instance(vm, name, sig, recv, args),
        AF_INLINE_PRESENTATION => af_inline_presentation_instance(vm, name, sig, recv, args),
        AF_FILL_RESPONSE => af_fill_response_instance(vm, name, sig, recv, args),
        AF_FILL_RESPONSE_BUILDER => af_fill_response_builder_instance(vm, name, sig, recv, args),
        AF_SAVE_INFO => af_save_info_instance(vm, name, sig, recv, args),
        AF_SAVE_INFO_BUILDER => af_save_info_builder_instance(vm, name, sig, recv, args),
        AF_USER_DATA => af_user_data_instance(vm, name, sig, recv, args),
        AF_USER_DATA_BUILDER => af_user_data_builder_instance(vm, name, sig, recv, args),
        AF_REGEX_VALIDATOR
        | AF_REQUIRED_VALIDATORS
        | AF_OPTIONAL_VALIDATORS
        | AF_NEGATION_VALIDATOR => af_validator_instance(vm, class, name, sig, recv, args),
        AF_TEXT_SANITIZER | AF_DATE_SANITIZER => {
            af_sanitizer_instance(vm, class, name, sig, recv, args)
        }
        AF_FILL_REQUEST => af_fill_request_instance(vm, name, sig, recv, args),
        AF_SAVE_REQUEST => af_save_request_instance(vm, name, sig, recv, args),
        AF_FILL_CONTEXT => af_fill_context_instance(vm, name, sig, recv, args),
        AF_FILL_CALLBACK => af_fill_callback_instance(vm, name, sig, recv, args),
        AF_SAVE_CALLBACK => af_save_callback_instance(vm, name, sig, recv, args),
        AF_FIELD_CLASSIFICATION => af_field_classification_instance(vm, name, sig, recv, args),
        AF_FIELD_CLASSIFICATION_MATCH => {
            af_field_classification_match_instance(vm, name, sig, recv, args)
        }
        AF_SAVED_DATASETS_INFO => af_saved_datasets_info_instance(vm, name, sig, recv, args),
        AUTOFILL_SERVICE => af_autofill_service_instance(vm, name, sig, recv, args),
        TRANSITION
        | TRANSITION_VISIBILITY
        | TRANSITION_FADE
        | TRANSITION_CHANGE_BOUNDS
        | TRANSITION_SET
        | TRANSITION_AUTO => transition_instance(vm, name, sig, recv, args),
        TRANSITION_VALUES => transition_values_instance(vm, name, sig, recv, args),
        TRANSITION_ANIMATION_INFO => transition_animation_info_instance(vm, name, sig, recv, args),
        TRANSITION_MANAGER => transition_manager_instance(vm, name, sig, recv, args),
        TRANSITION_SCENE => transition_scene_instance(vm, name, sig, recv, args),
        TRANSITION_LISTENER_ADAPTER
        | TRANSITION_EPICENTER_CALLBACK
        | TRANSITION_PROPAGATION
        | TRANSITION_PATH_MOTION
        | TRANSITION_STRAIGHT_PATH
        | TRANSITION_RUN_LISTENER
        | TRANSITION_ANIMATE_LISTENER
        | TRANSITION_SET_LISTENER
        | TRANSITION_SET_NEXT_LISTENER
        | TRANSITION_LISTENER
        | TRANSITION_MANAGER_MULTI
        | TRANSITION_MANAGER_END_LISTENER
        | TRANSITION_FADE_LISTENER
        | TRANSITION_FADE_ANIM_LISTENER
        | TRANSITION_DISAPPEAR_LISTENER
        | TRANSITION_VISIBILITY_OVERLAY_LISTENER
        | TRANSITION_CHANGE_BOUNDS_LISTENER => {
            transition_callback_instance(vm, class, name, sig, recv, args)
        }
        ANDROID_ERRNO
        | ANDROID_STRUCT_STAT
        | ANDROID_STRUCT_STAT_VFS
        | ANDROID_STRUCT_TIMESPEC
        | ANDROID_STRUCT_UTSNAME => android_system_instance(vm, class, name, sig, recv, args),
        _ => Ok(None),
    }
}

fn is_stub_class(c: &str) -> bool {
    matches!(
        c,
        WEBVIEW
            | WEB_SETTINGS
            | DOCUMENTS_PATH
            | NETWORK_SECURITY_POLICY
            | KEY_CHAIN_CALLBACK
            | CAMERA_MANAGER
            | CAMERA_ACCESS_EXCEPTION
            | CAMERA_METADATA_NATIVE
            | CAMERA_METADATA_NATIVE_KEY
            | TYPE_REFERENCE
            | CAMERA_CHARACTERISTICS
            | CAMERA_CHARACTERISTICS_KEY
            | CAPTURE_REQUEST
            | CAPTURE_REQUEST_KEY
            | CAPTURE_REQUEST_BUILDER
            | CAPTURE_RESULT
            | CAPTURE_RESULT_KEY
            | TOTAL_CAPTURE_RESULT
            | STREAM_CONFIGURATION
            | STREAM_CONFIGURATION_DURATION
            | STREAM_CONFIGURATION_MAP
            | HIGH_SPEED_VIDEO_CONFIGURATION
            | REPROCESS_FORMATS_MAP
            | METERING_RECTANGLE
            | RGGB_CHANNEL_VECTOR
            | BLACK_LEVEL_PATTERN
            | COLOR_SPACE_TRANSFORM
            | TEXT_CLASSIFIER_ENTITY_CONFIG
            | TEXT_CLASSIFIER_ENTITY_CONFIG_BUILDER
            | TEXT_CLASSIFICATION
            | TEXT_CLASSIFICATION_BUILDER
            | TEXT_CLASSIFICATION_REQUEST
            | TEXT_CLASSIFICATION_REQUEST_BUILDER
            | TEXT_SELECTION
            | TEXT_SELECTION_BUILDER
            | TEXT_SELECTION_REQUEST
            | TEXT_SELECTION_REQUEST_BUILDER
            | TEXT_LANGUAGE
            | TEXT_LANGUAGE_BUILDER
            | TEXT_LANGUAGE_REQUEST
            | TEXT_LANGUAGE_REQUEST_BUILDER
            | TEXT_LINKS
            | TEXT_LINKS_BUILDER
            | TEXT_LINK
            | TEXT_LINKS_REQUEST
            | TEXT_LINKS_REQUEST_BUILDER
            | CONVERSATION_ACTIONS
            | CONVERSATION_ACTION
            | CONVERSATION_ACTION_BUILDER
            | CONVERSATION_MESSAGE
            | CONVERSATION_MESSAGE_BUILDER
            | CONVERSATION_REQUEST
            | CONVERSATION_REQUEST_BUILDER
            | TEXT_CLASSIFICATION_MANAGER
            | ULOCALE
            | ENTITY_CONFIDENCE
            | ANIMATOR
            | VALUE_ANIMATOR
            | OBJECT_ANIMATOR
            | PROPERTY_VALUES_HOLDER
            | KEYFRAME
            | KEYFRAME_OBJECT
            | KEYFRAME_INT
            | KEYFRAME_FLOAT
            | ANIMATION_HANDLER
            | INT_EVALUATOR
            | FLOAT_EVALUATOR
            | ARGB_EVALUATOR
            | ANDROID_PROPERTY
            | ANDROID_INT_PROPERTY
            | ANDROID_FLOAT_PROPERTY
            | AUTOFILL_ID
            | AUTOFILL_VALUE
            | AUTOFILL_SERVICE
            | AF_DATASET
            | AF_DATASET_BUILDER
            | AF_DATASET_FILTER
            | AF_FIELD
            | AF_FIELD_BUILDER
            | AF_PRESENTATIONS
            | AF_PRESENTATIONS_BUILDER
            | AF_INLINE_PRESENTATION
            | AF_FILL_RESPONSE
            | AF_FILL_RESPONSE_BUILDER
            | AF_SAVE_INFO
            | AF_SAVE_INFO_BUILDER
            | AF_USER_DATA
            | AF_USER_DATA_BUILDER
            | AF_INTERNAL_VALIDATOR
            | AF_REGEX_VALIDATOR
            | AF_REQUIRED_VALIDATORS
            | AF_OPTIONAL_VALIDATORS
            | AF_NEGATION_VALIDATOR
            | AF_INTERNAL_SANITIZER
            | AF_TEXT_SANITIZER
            | AF_DATE_SANITIZER
            | AF_FILL_REQUEST
            | AF_SAVE_REQUEST
            | AF_FILL_CONTEXT
            | AF_FILL_CALLBACK
            | AF_SAVE_CALLBACK
            | AF_FIELD_CLASSIFICATION
            | AF_FIELD_CLASSIFICATION_MATCH
            | AF_SAVED_DATASETS_INFO
            | TRANSITION
            | TRANSITION_LISTENER
            | TRANSITION_EPICENTER_CALLBACK
            | TRANSITION_ANIMATION_INFO
            | TRANSITION_VALUES
            | TRANSITION_VALUES_MAPS
            | TRANSITION_VISIBILITY
            | TRANSITION_FADE
            | TRANSITION_CHANGE_BOUNDS
            | TRANSITION_SET
            | TRANSITION_AUTO
            | TRANSITION_MANAGER
            | TRANSITION_SCENE
            | TRANSITION_PROPAGATION
            | TRANSITION_PATH_MOTION
            | TRANSITION_LISTENER_ADAPTER
            | TRANSITION_STRAIGHT_PATH
            | TRANSITION_RUN_LISTENER
            | TRANSITION_ANIMATE_LISTENER
            | TRANSITION_SET_LISTENER
            | TRANSITION_SET_NEXT_LISTENER
            | TRANSITION_MANAGER_MULTI
            | TRANSITION_MANAGER_END_LISTENER
            | TRANSITION_FADE_LISTENER
            | TRANSITION_FADE_ANIM_LISTENER
            | TRANSITION_DISAPPEAR_LISTENER
            | TRANSITION_VISIBILITY_OVERLAY_LISTENER
            | TRANSITION_CHANGE_BOUNDS_LISTENER
            | ANDROID_OS
            | ANDROID_OS_CONSTANTS
            | ANDROID_ERRNO
            | ANDROID_STRUCT_STAT
            | ANDROID_STRUCT_STAT_VFS
            | ANDROID_STRUCT_TIMESPEC
            | ANDROID_STRUCT_UTSNAME
    )
}

// ---------------------------------------------------------------------------
// android.provider.DocumentsContract
// ---------------------------------------------------------------------------

const QUERY_ARG_DISPLAY_NAME: &str = "android:query-arg-display-name";
const QUERY_ARG_MIME_TYPES: &str = "android:query-arg-mime-types";
const QUERY_ARG_FILE_SIZE_OVER: &str = "android:query-arg-file-size-over";
const QUERY_ARG_LAST_MODIFIED_AFTER: &str = "android:query-arg-last-modified-after";
const QUERY_ARG_EXCLUDE_MEDIA: &str = "android:query-arg-exclude-media";

fn uri_string(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.fw.m8.uris.get(r).cloned(),
        _ => None,
    }
}
fn req_uri_string(vm: &Engine, args: &[Value], i: usize) -> Result<String, VmExit> {
    // AOSP dereferences the Uri (getPathSegments/getAuthority/...), so a null
    // Uri is an NPE on the paths that do not explicitly null-check it.
    uri_string(vm, args.get(i)).ok_or_else(|| npe("uri"))
}
fn build_content_uri(
    vm: &mut Engine,
    authority: Option<String>,
    segments: &[&str],
) -> Result<Value, VmExit> {
    build_content_uri_query(vm, authority, segments, None)
}
fn build_content_uri_query(
    vm: &mut Engine,
    authority: Option<String>,
    segments: &[&str],
    query: Option<(&str, Option<String>)>,
) -> Result<Value, VmExit> {
    let auth = authority
        .map(|a| crate::android_uri::encode(&a, "@:[]"))
        .unwrap_or_default();
    let mut s = format!("content://{auth}");
    for seg in segments {
        s.push('/');
        s.push_str(&crate::android_uri::encode(seg, ""));
    }
    if let Some((k, v)) = query {
        s.push('?');
        s.push_str(&crate::android_uri::encode(k, ""));
        s.push('=');
        s.push_str(&crate::android_uri::encode(v.as_deref().unwrap_or(""), ""));
    }
    crate::android_uri::new_uri(vm, s)
}
fn uri_authority(raw: &str) -> Option<String> {
    crate::android_uri::parse_parts(raw)
        .authority
        .map(|a| crate::android_uri::decode(&a, false))
}
fn uri_segments(raw: &str) -> Vec<String> {
    let p = crate::android_uri::parse_parts(raw);
    if p.opaque {
        return Vec::new();
    }
    p.path
        .split('/')
        .filter(|x| !x.is_empty())
        .map(|x| crate::android_uri::decode(x, false))
        .collect()
}
fn uri_query_param(raw: &str, key: &str) -> Option<String> {
    let p = crate::android_uri::parse_parts(raw);
    let q = p.query?;
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        if crate::android_uri::decode(k, true) == key {
            return Some(crate::android_uri::decode(v, true));
        }
    }
    None
}
fn root_id_from(raw: &str) -> Result<String, VmExit> {
    let p = uri_segments(raw);
    if p.len() >= 2 && p[0] == "root" {
        Ok(p[1].clone())
    } else {
        Err(iae(format!("Invalid URI: {raw}")))
    }
}
fn document_id_from(raw: &str) -> Result<String, VmExit> {
    let p = uri_segments(raw);
    if p.len() >= 2 && p[0] == "document" {
        Ok(p[1].clone())
    } else if p.len() >= 4 && p[0] == "tree" && p[2] == "document" {
        Ok(p[3].clone())
    } else {
        Err(iae(format!("Invalid URI: {raw}")))
    }
}
fn tree_document_id_from(raw: &str) -> Result<String, VmExit> {
    let p = uri_segments(raw);
    if p.len() >= 2 && p[0] == "tree" {
        Ok(p[1].clone())
    } else {
        Err(iae(format!("Invalid URI: {raw}")))
    }
}
fn bundle_entries(vm: &Engine, v: Option<&Value>) -> Option<Vec<(String, Value)>> {
    match v {
        Some(Value::Obj(r)) => match vm.fw.objects.get(r) {
            Some(HostObj::Bundle { map }) => Some(map.clone()),
            _ => Some(Vec::new()),
        },
        _ => None,
    }
}
fn bundle_get(vm: &Engine, v: Option<&Value>, key: &str) -> Option<Value> {
    bundle_entries(vm, v)?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, x)| x)
}
fn bundle_string(vm: &Engine, v: Option<&Value>, key: &str, default: &str) -> String {
    match bundle_get(vm, v, key) {
        Some(x) => opt_str(vm, Some(&x)).unwrap_or_else(|| default.to_string()),
        None => default.to_string(),
    }
}
fn bundle_long(vm: &Engine, v: Option<&Value>, key: &str, default: i64) -> i64 {
    match bundle_get(vm, v, key) {
        Some(Value::Long(x)) => x,
        Some(Value::Int(x)) => x as i64,
        _ => default,
    }
}
fn normalize_mime(s: &str) -> String {
    // Intent.normalizeMimeType lower-cases and strips parameters.
    s.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}
fn mime_matches(mime: &str, filter: &str) -> bool {
    // MimeTypeFilter.matches: "*/*", "type/*", or exact type/subtype.
    if filter == "*/*" {
        return mime.contains('/');
    }
    let (ft, fs) = match filter.split_once('/') {
        Some(x) => x,
        None => return false,
    };
    let (mt, ms) = match mime.split_once('/') {
        Some(x) => x,
        None => return false,
    };
    (ft == "*" || ft == mt) && (fs == "*" || fs == ms)
}

fn documents_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let out = match name {
        "buildRootsUri" => build_content_uri(vm, opt_str_arg(vm, args, 0), &["root"])?,
        "buildRootUri" => {
            let root = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri(vm, opt_str_arg(vm, args, 0), &["root", &root])?
        }
        "buildRecentDocumentsUri" => {
            let root = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri(vm, opt_str_arg(vm, args, 0), &["root", &root, "recent"])?
        }
        "buildTreeDocumentUri" => {
            let doc = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri(vm, opt_str_arg(vm, args, 0), &["tree", &doc])?
        }
        "buildDocumentUri" if sig.starts_with("(Ljava/lang/String;Ljava/lang/String;)") => {
            let doc = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri(vm, opt_str_arg(vm, args, 0), &["document", &doc])?
        }
        "buildBaseDocumentUri" => build_content_uri(vm, opt_str_arg(vm, args, 0), &["document"])?,
        "buildChildDocumentsUri" if sig.starts_with("(Ljava/lang/String;Ljava/lang/String;)") => {
            let doc = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri(
                vm,
                opt_str_arg(vm, args, 0),
                &["document", &doc, "children"],
            )?
        }
        "buildSearchDocumentsUri" => {
            let root = opt_str_arg(vm, args, 1).unwrap_or_default();
            build_content_uri_query(
                vm,
                opt_str_arg(vm, args, 0),
                &["root", &root, "search"],
                Some(("query", opt_str_arg(vm, args, 2))),
            )?
        }
        "buildDocumentUriUsingTree" | "buildChildDocumentsUriUsingTree" => {
            let raw = req_uri_string(vm, args, 0)?;
            let tree = tree_document_id_from(&raw)?;
            let doc = opt_str_arg(vm, args, 1).unwrap_or_default();
            if name == "buildDocumentUriUsingTree" {
                build_content_uri(vm, uri_authority(&raw), &["tree", &tree, "document", &doc])?
            } else {
                build_content_uri(
                    vm,
                    uri_authority(&raw),
                    &["tree", &tree, "document", &doc, "children"],
                )?
            }
        }
        "buildDocumentUriMaybeUsingTree" => {
            let raw = req_uri_string(vm, args, 0)?;
            let doc = opt_str_arg(vm, args, 1).unwrap_or_default();
            let segs = uri_segments(&raw);
            if segs.len() >= 2 && segs[0] == "tree" {
                build_content_uri(
                    vm,
                    uri_authority(&raw),
                    &["tree", &segs[1], "document", &doc],
                )?
            } else {
                build_content_uri(vm, uri_authority(&raw), &["document", &doc])?
            }
        }
        "getRootId" => jstr(vm, root_id_from(&req_uri_string(vm, args, 0)?)?)?,
        "getDocumentId" => jstr(vm, document_id_from(&req_uri_string(vm, args, 0)?)?)?,
        "getTreeDocumentId" => jstr(vm, tree_document_id_from(&req_uri_string(vm, args, 0)?)?)?,
        "getSearchDocumentsQuery" => {
            if sig.contains("Landroid/os/Bundle;") {
                if !array_is_obj(args.first()) {
                    return Err(npe("bundle can not be null"));
                }
                jstr(
                    vm,
                    bundle_string(vm, args.first(), QUERY_ARG_DISPLAY_NAME, ""),
                )?
            } else {
                let raw = req_uri_string(vm, args, 0)?;
                match uri_query_param(&raw, "query") {
                    Some(q) => jstr(vm, q)?,
                    None => Value::Null,
                }
            }
        }
        "isContentUri" => {
            let ok = match uri_string(vm, args.first()) {
                Some(raw) => {
                    crate::android_uri::parse_parts(&raw).scheme.as_deref() == Some("content")
                }
                None => false,
            };
            bool_value(ok)
        }
        "isTreeUri" => {
            let raw = req_uri_string(vm, args, 0)?;
            let p = uri_segments(&raw);
            bool_value(p.len() >= 2 && p[0] == "tree")
        }
        // A DocumentsProvider is a content provider that answers
        // DocumentsContract.PROVIDER_INTERFACE.  This guest profile has no
        // installed providers, so the AOSP provider lookup finds none and the
        // document/root predicates are false even for syntactically valid URIs.
        "isDocumentUri" => bool_value(false),
        "isRootsUri" | "isRootUri" => {
            if !array_is_obj(args.first()) {
                return Err(npe("context can not be null"));
            }
            bool_value(false)
        }
        "setManageMode" => {
            if !array_is_obj(args.first()) {
                return Err(npe("uri can not be null"));
            }
            let raw = req_uri_string(vm, args, 0)?;
            let (before_fragment, fragment) = match raw.split_once('#') {
                Some((a, b)) => (a.to_string(), Some(b.to_string())),
                None => (raw, None),
            };
            let mut out = before_fragment;
            out.push(if out.contains('?') { '&' } else { '?' });
            out.push_str("manage=true");
            if let Some(f) = fragment {
                out.push('#');
                out.push_str(&f);
            }
            crate::android_uri::new_uri(vm, out)?
        }
        "isManageMode" => {
            if !array_is_obj(args.first()) {
                return Err(npe("uri can not be null"));
            }
            let raw = req_uri_string(vm, args, 0)?;
            let v = uri_query_param(&raw, "manage").unwrap_or_default();
            bool_value(!(v.eq_ignore_ascii_case("false") || v == "0" || v.is_empty()))
        }
        "getHandledQueryArguments" => {
            let Some(entries) = bundle_entries(vm, args.first()) else {
                return obj_array(vm, "[Ljava/lang/String;", Vec::new()).map(Some);
            };
            let mut out = Vec::new();
            for key in [
                QUERY_ARG_EXCLUDE_MEDIA,
                QUERY_ARG_DISPLAY_NAME,
                QUERY_ARG_FILE_SIZE_OVER,
                QUERY_ARG_LAST_MODIFIED_AFTER,
                QUERY_ARG_MIME_TYPES,
            ] {
                if entries.iter().any(|(k, _)| k == key) {
                    out.push(jstr(vm, key)?);
                }
            }
            obj_array(vm, "[Ljava/lang/String;", out)?
        }
        "matchSearchQueryArguments" => {
            if bundle_entries(vm, args.first()).is_none() {
                bool_value(true)
            } else {
                let query = bundle_string(vm, args.first(), QUERY_ARG_DISPLAY_NAME, "");
                if !query.is_empty() {
                    let display = req_str(vm, args, 1, "displayName")?;
                    if !display.to_lowercase().contains(&query.to_lowercase()) {
                        return Ok(Some(bool_value(false)));
                    }
                }
                let min_size = bundle_long(vm, args.first(), QUERY_ARG_FILE_SIZE_OVER, -1);
                if min_size != -1 && larg(args, 4) < min_size {
                    return Ok(Some(bool_value(false)));
                }
                let min_modified = bundle_long(vm, args.first(), QUERY_ARG_LAST_MODIFIED_AFTER, -1);
                if min_modified != -1 && larg(args, 3) < min_modified {
                    return Ok(Some(bool_value(false)));
                }
                let types = bundle_get(vm, args.first(), QUERY_ARG_MIME_TYPES);
                let type_vals = array_elems(vm, types.as_ref());
                if !type_vals.is_empty() {
                    let mime = normalize_mime(&opt_str_arg(vm, args, 2).unwrap_or_default());
                    let mut any = false;
                    for t in &type_vals {
                        let f = normalize_mime(&opt_str(vm, Some(t)).unwrap_or_default());
                        if mime_matches(&mime, &f) {
                            any = true;
                            break;
                        }
                    }
                    bool_value(any)
                } else {
                    bool_value(true)
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn documents_path_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let path = args.get(1).cloned().unwrap_or(Value::Null);
            let items = collection_items(vm, Some(&path));
            // DocumentsContract uses Preconditions.checkCollectionNotEmpty and
            // checkCollectionElementsNotNull, both IllegalArgumentException.
            if !matches!(path, Value::Obj(_)) || items.is_empty() {
                return Err(iae("path must not be empty"));
            }
            if items.iter().any(|v| matches!(v, Value::Null)) {
                return Err(iae("path must not contain null elements"));
            }
            put(
                vm,
                recv,
                "mRootId",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            // AOSP retains the caller's List reference (it does not copy it).
            put(vm, recv, "mPath", path)?;
            Value::Null
        }
        "getRootId" => get(vm, recv, "mRootId"),
        "getPath" => get(vm, recv, "mPath"),
        "equals" => {
            let eq = match args.first() {
                Some(Value::Obj(o)) if class_of(vm, *o) == DOCUMENTS_PATH => {
                    let a_root = get(vm, recv, "mRootId");
                    let b_root = get(vm, *o, "mRootId");
                    let a_items = collection_items(vm, Some(&get(vm, recv, "mPath")));
                    let b_items = collection_items(vm, Some(&get(vm, *o, "mPath")));
                    objects_equal(vm, &a_root, &b_root)
                        && a_items.len() == b_items.len()
                        && a_items
                            .iter()
                            .zip(b_items.iter())
                            .all(|(a, b)| objects_equal(vm, a, b))
                }
                _ => false,
            };
            bool_value(eq)
        }
        "hashCode" => {
            // Objects.hash(rootId, path): List.hashCode is 1 + 31*elem hashes.
            let root_h = host_hash(vm, &get(vm, recv, "mRootId"));
            let items = collection_items(vm, Some(&get(vm, recv, "mPath")));
            let mut list_h: i32 = 1;
            for it in &items {
                list_h = list_h.wrapping_mul(31).wrapping_add(host_hash(vm, it));
            }
            Value::Int(
                (31i32.wrapping_add(root_h))
                    .wrapping_mul(31)
                    .wrapping_add(list_h),
            )
        }
        "toString" => {
            let root =
                opt_str(vm, Some(&get(vm, recv, "mRootId"))).unwrap_or_else(|| "null".into());
            let items = collection_items(vm, Some(&get(vm, recv, "mPath")));
            let parts: Vec<String> = items
                .iter()
                .map(|v| opt_str(vm, Some(v)).unwrap_or_else(|| "null".into()))
                .collect();
            jstr(
                vm,
                format!(
                    "DocumentsContract.Path{{rootId={}, path=[{}]}}",
                    root,
                    parts.join(", ")
                ),
            )?
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------------
// android.security.NetworkSecurityPolicy / KeyChain (no Keystore service)
// ---------------------------------------------------------------------------

fn policy_singleton(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (NETWORK_SECURITY_POLICY.to_string(), "INSTANCE".to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let r = alloc_obj(vm, NETWORK_SECURITY_POLICY, Vec::new())?;
    // libcore's DefaultNetworkSecurityPolicy permits cleartext until the
    // framework installs a manifest-derived policy.
    put(vm, r, "rd_cleartext", Value::Int(1))?;
    let v = Value::Obj(r);
    vm.statics.insert(key, v.clone());
    Ok(v)
}
fn policy_cleartext(vm: &mut Engine) -> Result<bool, VmExit> {
    // NetworkSecurityPolicy.setCleartextTrafficPermitted replaces the libcore
    // process policy; the framework singleton instance itself is unchanged.
    let key = (
        NETWORK_SECURITY_POLICY.to_string(),
        "rd_libcore_cleartext".to_string(),
    );
    Ok(match vm.statics.get(&key) {
        Some(Value::Int(i)) => *i != 0,
        _ => true,
    })
}
fn network_policy_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getInstance" => policy_singleton(vm)?,
        _ => return Ok(None),
    }))
}
fn network_policy_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => Value::Null,
        "isCleartextTrafficPermitted" => bool_value(policy_cleartext(vm)?),
        "setCleartextTrafficPermitted" => {
            vm.statics.insert(
                (
                    NETWORK_SECURITY_POLICY.to_string(),
                    "rd_libcore_cleartext".to_string(),
                ),
                Value::Int(iarg(args, 0)),
            );
            Value::Null
        }
        "handleTrustStorageUpdate" => Value::Null,
        _ => return Ok(None),
    }))
}

fn make_install_intent(vm: &mut Engine) -> Result<Value, VmExit> {
    let intent = alloc_obj(vm, "Landroid/content/Intent;", Vec::new())?;
    let action = jstr(vm, "android.credentials.INSTALL")?;
    vm.invoke_instance(intent, "<init>", "(Ljava/lang/String;)V", &[action])?;
    // AOSP uses CERT_INSTALLER_PACKAGE = "com.android.certinstaller" and
    // class "com.android.certinstaller.CertInstallerMain".
    let cn = alloc_obj(vm, "Landroid/content/ComponentName;", Vec::new())?;
    let package = jstr(vm, "com.android.certinstaller")?;
    let klass = jstr(vm, "com.android.certinstaller.CertInstallerMain")?;
    vm.invoke_instance(
        cn,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[package, klass],
    )?;
    vm.invoke_instance(
        intent,
        "setComponent",
        "(Landroid/content/ComponentName;)Landroid/content/Intent;",
        &[Value::Obj(cn)],
    )?;
    Ok(Value::Obj(intent))
}

fn keychain_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let out = match name {
        "createInstallIntent" => make_install_intent(vm)?,
        "isKeyAlgorithmSupported" => {
            let alg = req_str(vm, args, 0, "algorithm")?;
            bool_value(matches!(alg.to_ascii_uppercase().as_str(), "EC" | "RSA"))
        }
        // AOSP: deprecated, and the current implementation returns true for
        // every input (its argument is only dereferenced by requireNonNull in
        // some platform builds; Android 14 returns true literally).
        "isBoundKeyAlgorithm" => bool_value(true),
        "getPrivateKey" | "getKeyPair" => {
            // The no-KeyChain-service profile has no granted key IDs, which is
            // exactly AOSP's null-keyId branch.  Argument checks happen first.
            if !array_is_obj(args.get(1)) {
                return Err(npe("alias == null"));
            }
            if !array_is_obj(args.first()) {
                return Err(npe("context == null"));
            }
            Value::Null
        }
        "getCertificateChain" => {
            if !array_is_obj(args.get(1)) {
                return Err(npe("alias == null"));
            }
            // AOSP next dereferences context.getApplicationContext() while
            // binding; null context therefore NPEs before any service result.
            if !array_is_obj(args.first()) {
                return Err(npe("context == null"));
            }
            Value::Null
        }
        "choosePrivateKeyAlias" => {
            // Signature overloads differ in the URI/host/port parameters.  In
            // every public overload the first two arguments are activity and
            // callback, and issuers are the fourth argument.
            let activity = req_obj(args, 0, "activity == null")?;
            let callback = req_obj(args, 1, "response == null")?;
            let _ = activity;
            if let Some(issuers) = args.get(3) {
                for issuer in array_elems(vm, Some(issuers)) {
                    if let Value::Obj(r) = issuer {
                        let c = class_of(vm, r);
                        if c != "Ljavax/security/auth/x500/X500Principal;" {
                            let shown = crate::intrinsics::object_to_string(vm, r)?;
                            return Err(iae(format!(
                                "Issuer {shown} is of type class {}, not X500Principal",
                                c.trim_start_matches('L')
                                    .trim_end_matches(';')
                                    .replace('/', ".")
                            )));
                        }
                    } else if matches!(issuer, Value::Null) {
                        // AOSP evaluates issuer instanceof X500Principal, then
                        // issuer.toString() in the error message: null NPEs.
                        return Err(npe("issuer"));
                    }
                }
            }
            let _ = sig;
            // No client certificates are installed in this profile: the
            // KeyChain contract reports that outcome as alias(null).
            vm.invoke_instance(callback, "alias", "(Ljava/lang/String;)V", &[Value::Null])?;
            Value::Null
        }
        "isCredentialManagementApp" => {
            // With no KeyChain service/registered credential-management app,
            // the caller is not that app.
            bool_value(false)
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn keychain_callback_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "alias" => {
            put(vm, recv, "rd_alias_called", Value::Int(1))?;
            put(
                vm,
                recv,
                "rd_alias",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------------
// android.webkit.WebView / WebSettings (state/history, no renderer)
// ---------------------------------------------------------------------------

fn enum_value(vm: &mut Engine, class: &str, name: &str, ordinal: i32) -> Result<Value, VmExit> {
    let key = (class.to_string(), name.to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let r = alloc_obj(vm, class, Vec::new())?;
    let name_val = jstr(vm, name)?;
    put(vm, r, "rd_name", name_val)?;
    put(vm, r, "rd_ord", Value::Int(ordinal))?;
    put(vm, r, "name", get(vm, r, "rd_name"))?;
    put(vm, r, "ordinal", Value::Int(ordinal))?;
    let v = Value::Obj(r);
    vm.statics.insert(key, v.clone());
    Ok(v)
}
fn web_enum_default(vm: &mut Engine, class_short: &str, name: &str) -> Result<Value, VmExit> {
    let (class, ord) = match (class_short, name) {
        ("LayoutAlgorithm", "NORMAL") => ("Landroid/webkit/WebSettings$LayoutAlgorithm;", 0),
        ("LayoutAlgorithm", "SINGLE_COLUMN") => ("Landroid/webkit/WebSettings$LayoutAlgorithm;", 1),
        ("LayoutAlgorithm", "NARROW_COLUMNS") => {
            ("Landroid/webkit/WebSettings$LayoutAlgorithm;", 2)
        }
        ("LayoutAlgorithm", "TEXT_AUTOSIZING") => {
            ("Landroid/webkit/WebSettings$LayoutAlgorithm;", 3)
        }
        ("ZoomDensity", "FAR") => ("Landroid/webkit/WebSettings$ZoomDensity;", 0),
        ("ZoomDensity", "MEDIUM") => ("Landroid/webkit/WebSettings$ZoomDensity;", 1),
        ("ZoomDensity", "CLOSE") => ("Landroid/webkit/WebSettings$ZoomDensity;", 2),
        ("RenderPriority", "NORMAL") => ("Landroid/webkit/WebSettings$RenderPriority;", 0),
        ("RenderPriority", "HIGH") => ("Landroid/webkit/WebSettings$RenderPriority;", 1),
        ("RenderPriority", "LOW") => ("Landroid/webkit/WebSettings$RenderPriority;", 2),
        ("PluginState", "ON") => ("Landroid/webkit/WebSettings$PluginState;", 0),
        ("PluginState", "ON_DEMAND") => ("Landroid/webkit/WebSettings$PluginState;", 1),
        ("PluginState", "OFF") => ("Landroid/webkit/WebSettings$PluginState;", 2),
        _ => ("Landroid/webkit/WebSettings$LayoutAlgorithm;", 0),
    };
    let v = enum_value(vm, class, name, ord)?;
    if class_short == "ZoomDensity" {
        if let Value::Obj(r) = v.clone() {
            let value = match name {
                "FAR" => 150,
                "CLOSE" => 75,
                _ => 100,
            };
            put(vm, r, "value", Value::Int(value))?;
        }
    }
    Ok(v)
}

fn web_settings_default(vm: &mut Engine, prop: &str) -> Result<Value, VmExit> {
    Ok(match prop {
        // Boolean settings whose AOSP/provider default is true.
        "SupportZoom"
        | "MediaPlaybackRequiresUserGesture"
        | "DisplayZoomControls"
        | "AllowContentAccess"
        | "UseWebViewBackgroundForOverscrollBackground"
        | "SaveFormData"
        | "SavePassword"
        | "LoadsImagesAutomatically"
        | "GeolocationEnabled"
        | "NeedInitialFocus"
        | "SafeBrowsingEnabled" => Value::Int(1),
        // Boolean settings whose default is false in this profile.  This VM
        // targets Android 14, so AllowFileAccess /
        // Allow{Universal,File}AccessFromFileURLs use the post-Q defaults, and
        // it has INTERNET permission manifest state not modelled separately;
        // BlockNetworkLoads therefore also defaults to false.
        "NavDump"
        | "BuiltInZoomControls"
        | "AllowFileAccess"
        | "LoadWithOverviewMode"
        | "EnableSmoothTransition"
        | "AcceptThirdPartyCookies"
        | "LightTouchEnabled"
        | "UseWideViewPort"
        | "SupportMultipleWindows"
        | "BlockNetworkImage"
        | "BlockNetworkLoads"
        | "JavaScriptEnabled"
        | "AllowUniversalAccessFromFileURLs"
        | "AllowFileAccessFromFileURLs"
        | "PluginsEnabled"
        | "DatabaseEnabled"
        | "DomStorageEnabled"
        | "JavaScriptCanOpenWindowsAutomatically"
        | "VideoOverlayForEmbeddedEncryptedVideoEnabled"
        | "OffscreenPreRaster"
        | "AlgorithmicDarkeningAllowed" => Value::Int(0),
        "TextZoom" => Value::Int(100),
        "MinimumFontSize" | "MinimumLogicalFontSize" => Value::Int(8),
        "DefaultFontSize" | "DefaultFixedFontSize" => Value::Int(16),
        "UserAgent" => Value::Int(0),
        "CacheMode" => Value::Int(-1),
        // Target Lollipop+ default.
        "MixedContentMode" => Value::Int(1),
        "ForceDark" => Value::Int(1),
        "DisabledActionModeMenuItems" => Value::Int(0),
        "DefaultZoom" => web_enum_default(vm, "ZoomDensity", "MEDIUM")?,
        "LayoutAlgorithm" => web_enum_default(vm, "LayoutAlgorithm", "NARROW_COLUMNS")?,
        "RenderPriority" => web_enum_default(vm, "RenderPriority", "NORMAL")?,
        "PluginState" => web_enum_default(vm, "PluginState", "OFF")?,
        "DefaultTextEncodingName" => jstr(vm, "UTF-8")?,
        "StandardFontFamily" | "SansSerifFontFamily" => jstr(vm, "sans-serif")?,
        "FixedFontFamily" => jstr(vm, "monospace")?,
        "SerifFontFamily" => jstr(vm, "serif")?,
        "CursiveFontFamily" => jstr(vm, "cursive")?,
        "FantasyFontFamily" => jstr(vm, "fantasy")?,
        "DatabasePath" | "GeolocationDatabasePath" => jstr(vm, "")?,
        "UserAgentString" => {
            // Defer to the same deterministic value as getDefaultUserAgent.
            jstr(vm, "Mozilla/5.0 (Linux; Android 14; RustyDroid) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")?
        }
        _ => Value::Null,
    })
}

fn web_settings_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(owner) = field_obj(vm, recv, "rd_owner") {
        if field_bool(vm, owner, "rd_destroyed") {
            return Err(ise(
                "Calling WebSettings method after WebView has been destroyed",
            ));
        }
    }
    if name == "<init>" {
        return Ok(Some(Value::Null));
    }
    // Enum accessors that may be invoked through the WebSettings class in
    // host-created enum objects are handled by graphics.rs normally.  A
    // WebSettings property is identified by stripping set/get/is/support
    // prefixes according to AOSP's (occasionally irregular) method names.
    let (prop, is_setter) = if let Some(p) = name.strip_prefix("set") {
        (p.to_string(), true)
    } else if let Some(p) = name.strip_prefix("get") {
        (p.to_string(), false)
    } else if name == "supportZoom" {
        ("SupportZoom".to_string(), false)
    } else if name == "supportMultipleWindows" {
        ("SupportMultipleWindows".to_string(), false)
    } else if name == "enableSmoothTransition" {
        ("EnableSmoothTransition".to_string(), false)
    } else if name == "isAlgorithmicDarkeningAllowed" {
        ("AlgorithmicDarkeningAllowed".to_string(), false)
    } else if name == "setAlgorithmicDarkeningAllowed" {
        ("AlgorithmicDarkeningAllowed".to_string(), true)
    } else {
        return Ok(None);
    };
    // Reject property names not modeled above so unrelated WebSettings calls
    // retain their ordinary unresolved behavior.
    let default = web_settings_default(vm, &prop)?;
    let known = !matches!(default, Value::Null)
        || matches!(
            prop.as_str(),
            "DatabasePath" | "GeolocationDatabasePath" | "UserAgentString"
        );
    if !known {
        return Ok(None);
    }
    let field = format!("rd_{prop}");
    if is_setter {
        let mut value = args.first().cloned().unwrap_or(Value::Null);
        match prop.as_str() {
            // AOSP documentation: font sizes outside 1..72 are pinned.
            "MinimumFontSize"
            | "MinimumLogicalFontSize"
            | "DefaultFontSize"
            | "DefaultFixedFontSize" => {
                value = Value::Int(iarg(args, 0).clamp(1, 72));
            }
            "BlockNetworkLoads" if iarg(args, 0) == 0 => {
                // This profile grants INTERNET, so setting false is allowed.
            }
            _ => {}
        }
        put(vm, recv, &field, value)?;
        Ok(Some(Value::Null))
    } else {
        let current = get(vm, recv, &field);
        Ok(Some(if matches!(current, Value::Null) {
            default
        } else {
            current
        }))
    }
}

fn web_history(vm: &Engine, recv: ObjRef) -> Vec<Value> {
    collection_items(vm, Some(&get(vm, recv, "rd_history")))
}
fn web_set_history(vm: &mut Engine, recv: ObjRef, items: Vec<Value>) -> Result<(), VmExit> {
    match field_obj(vm, recv, "rd_history") {
        Some(list) => {
            vm.fw.objects.insert(list, HostObj::List { elems: items });
            Ok(())
        }
        None => {
            let list = new_list(vm, items)?;
            put(vm, recv, "rd_history", Value::Obj(list))
        }
    }
}
fn web_current_url(vm: &Engine, recv: ObjRef) -> Value {
    let hist = web_history(vm, recv);
    let idx = field_int(vm, recv, "rd_history_index");
    if idx < 0 {
        Value::Null
    } else {
        hist.get(idx as usize).cloned().unwrap_or(Value::Null)
    }
}
fn web_navigate(vm: &mut Engine, recv: ObjRef, url: String) -> Result<(), VmExit> {
    let url_value = jstr(vm, &url)?;
    let mut hist = web_history(vm, recv);
    let mut idx = field_int(vm, recv, "rd_history_index");
    if idx < -1 {
        idx = -1;
    }
    if idx >= 0 && (idx as usize) < hist.len() {
        hist.truncate((idx as usize) + 1);
    } else if idx >= hist.len() as i32 {
        idx = hist.len() as i32 - 1;
        hist.truncate((idx + 1).max(0) as usize);
    }
    hist.push(url_value.clone());
    let new_idx = hist.len() as i32 - 1;
    web_set_history(vm, recv, hist)?;
    put(vm, recv, "rd_history_index", Value::Int(new_idx))?;
    put(vm, recv, "rd_original_url", url_value)?;
    put(vm, recv, "rd_progress", Value::Int(100))?;
    Ok(())
}
fn web_check_alive(vm: &Engine, recv: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, recv, "rd_destroyed") {
        Err(ise("Calling WebView method after destroy"))
    } else {
        Ok(())
    }
}

fn webview_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        web_set_history(vm, recv, Vec::new())?;
        put(vm, recv, "rd_history_index", Value::Int(-1))?;
        put(vm, recv, "rd_progress", Value::Int(0))?;
        put(vm, recv, "rd_destroyed", Value::Int(0))?;
        put(vm, recv, "rd_paused", Value::Int(0))?;
        return Ok(Some(Value::Null));
    }
    if name == "destroy" {
        put(vm, recv, "rd_destroyed", Value::Int(1))?;
        return Ok(Some(Value::Null));
    }
    // Let ordinary View methods flow to the existing View implementation.
    if !matches!(
        name,
        "getSettings"
            | "loadUrl"
            | "postUrl"
            | "loadData"
            | "loadDataWithBaseURL"
            | "stopLoading"
            | "reload"
            | "canGoBack"
            | "goBack"
            | "canGoForward"
            | "goForward"
            | "canGoBackOrForward"
            | "goBackOrForward"
            | "clearHistory"
            | "getUrl"
            | "getOriginalUrl"
            | "getTitle"
            | "getFavicon"
            | "getProgress"
            | "getContentHeight"
            | "getContentWidth"
            | "pauseTimers"
            | "resumeTimers"
            | "onPause"
            | "onResume"
            | "isPaused"
            | "freeMemory"
            | "clearCache"
            | "clearFormData"
            | "clearSslPreferences"
    ) {
        return Ok(None);
    }
    web_check_alive(vm, recv)?;
    let out = match name {
        "getSettings" => {
            if let Some(s) = field_obj(vm, recv, "rd_settings") {
                Value::Obj(s)
            } else {
                let s = alloc_obj(vm, WEB_SETTINGS, Vec::new())?;
                put(vm, s, "rd_owner", Value::Obj(recv))?;
                put(vm, recv, "rd_settings", Value::Obj(s))?;
                Value::Obj(s)
            }
        }
        "loadUrl" => {
            let url = req_str(vm, args, 0, "url")?;
            if args.len() > 1 {
                req_obj(args, 1, "additionalHttpHeaders")?;
                put(
                    vm,
                    recv,
                    "rd_headers",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
            }
            web_navigate(vm, recv, url)?;
            Value::Null
        }
        "postUrl" => {
            let url = req_str(vm, args, 0, "url")?;
            req_obj(args, 1, "postData")?;
            put(
                vm,
                recv,
                "rd_post_data",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            web_navigate(vm, recv, url)?;
            Value::Null
        }
        "loadData" => {
            let data = req_str(vm, args, 0, "data")?;
            let mime = opt_str_arg(vm, args, 1).unwrap_or_else(|| "text/html".into());
            let enc = opt_str_arg(vm, args, 2);
            let url = if enc.as_deref() == Some("base64") {
                format!("data:{mime};base64,{data}")
            } else {
                format!("data:{mime},{data}")
            };
            let stored = jstr(vm, data)?;
            put(vm, recv, "rd_data", stored)?;
            web_navigate(vm, recv, url)?;
            Value::Null
        }
        "loadDataWithBaseURL" => {
            let base = opt_str_arg(vm, args, 0).unwrap_or_else(|| "about:blank".into());
            let data = req_str(vm, args, 1, "data")?;
            let mime = opt_str_arg(vm, args, 2).unwrap_or_else(|| "text/html".into());
            let enc = opt_str_arg(vm, args, 3);
            let history = opt_str_arg(vm, args, 4);
            let stored = jstr(vm, &data)?;
            put(vm, recv, "rd_data", stored)?;
            let stored = jstr(vm, &base)?;
            put(vm, recv, "rd_base_url", stored)?;
            let url = if base.starts_with("data:") {
                // AOSP: a data: base behaves exactly like loadData and the
                // history URL is ignored.
                if enc.as_deref() == Some("base64") {
                    format!("data:{mime};base64,{data}")
                } else {
                    format!("data:{mime},{data}")
                }
            } else {
                history.unwrap_or(base)
            };
            web_navigate(vm, recv, url)?;
            Value::Null
        }
        "stopLoading"
        | "reload"
        | "pauseTimers"
        | "resumeTimers"
        | "freeMemory"
        | "clearCache"
        | "clearFormData"
        | "clearSslPreferences" => Value::Null,
        "onPause" => {
            put(vm, recv, "rd_paused", Value::Int(1))?;
            Value::Null
        }
        "onResume" => {
            put(vm, recv, "rd_paused", Value::Int(0))?;
            Value::Null
        }
        "isPaused" => bool_value(field_bool(vm, recv, "rd_paused")),
        "canGoBack" => bool_value(field_int(vm, recv, "rd_history_index") > 0),
        "goBack" => {
            let idx = field_int(vm, recv, "rd_history_index");
            if idx > 0 {
                put(vm, recv, "rd_history_index", Value::Int(idx - 1))?;
            }
            Value::Null
        }
        "canGoForward" => {
            let idx = field_int(vm, recv, "rd_history_index");
            bool_value(idx >= 0 && (idx as usize) + 1 < web_history(vm, recv).len())
        }
        "goForward" => {
            let idx = field_int(vm, recv, "rd_history_index");
            if idx >= 0 && (idx as usize) + 1 < web_history(vm, recv).len() {
                put(vm, recv, "rd_history_index", Value::Int(idx + 1))?;
            }
            Value::Null
        }
        "canGoBackOrForward" => {
            let idx = field_int(vm, recv, "rd_history_index");
            let n = web_history(vm, recv).len() as i32;
            let target = idx.wrapping_add(iarg(args, 0));
            bool_value(idx >= 0 && target >= 0 && target < n)
        }
        "goBackOrForward" => {
            let idx = field_int(vm, recv, "rd_history_index");
            let n = web_history(vm, recv).len() as i32;
            let target = idx.wrapping_add(iarg(args, 0));
            if idx >= 0 && target >= 0 && target < n {
                put(vm, recv, "rd_history_index", Value::Int(target))?;
            }
            Value::Null
        }
        "clearHistory" => {
            let cur = web_current_url(vm, recv);
            if matches!(cur, Value::Null) {
                web_set_history(vm, recv, Vec::new())?;
                put(vm, recv, "rd_history_index", Value::Int(-1))?;
            } else {
                web_set_history(vm, recv, vec![cur])?;
                put(vm, recv, "rd_history_index", Value::Int(0))?;
            }
            Value::Null
        }
        "getUrl" => web_current_url(vm, recv),
        "getOriginalUrl" => {
            let cur = web_current_url(vm, recv);
            if matches!(cur, Value::Null) {
                Value::Null
            } else {
                // No redirects are modelled; original == current for every
                // navigation performed by the registered slice.
                cur
            }
        }
        "getTitle" | "getFavicon" => Value::Null,
        "getProgress" => Value::Int(field_int(vm, recv, "rd_progress")),
        "getContentHeight" | "getContentWidth" => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn webview_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "setWebContentsDebuggingEnabled" => {
            vm.statics.insert(
                (WEBVIEW.to_string(), "rd_debugging".to_string()),
                Value::Int(iarg(args, 0)),
            );
            Value::Null
        }
        "enablePlatformNotifications" | "disablePlatformNotifications" => Value::Null,
        "clearClientCertPreferences" => {
            if let Some(r) = oarg(args, 0) {
                // The interface is java.lang.Runnable; a guest implementation
                // runs, while a merely host-allocated Runnable is unresolved.
                vm.invoke_instance(r, "run", "()V", &[])?;
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------------
// android.view.textclassifier (values + NO_OP classifier)
// ---------------------------------------------------------------------------

fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}
fn check_text_range(
    vm: &mut Engine,
    text_v: Option<&Value>,
    start: i32,
    end: i32,
) -> Result<Value, VmExit> {
    let text = charseq(vm, text_v)?.ok_or_else(|| iae(""))?;
    if start < 0 || end > utf16_len(&text) || end <= start {
        return Err(iae(""));
    }
    Ok(text_v.cloned().unwrap_or(Value::Null))
}
fn numeric_float(vm: &Engine, v: &Value) -> Option<f32> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Double(d) => Some(*d as f32),
        Value::Int(i) => Some(*i as f32),
        Value::Long(l) => Some(*l as f32),
        Value::Obj(r) => match get(vm, *r, "value") {
            Value::Float(f) => Some(f),
            Value::Double(d) => Some(d as f32),
            Value::Int(i) => Some(i as f32),
            Value::Long(l) => Some(l as f32),
            _ => None,
        },
        _ => None,
    }
}
fn entity_pairs(vm: &Engine, r: ObjRef) -> Vec<(Value, f32)> {
    let names = array_elems(vm, Some(&get(vm, r, "rd_entity_names")));
    let scores = float_values(vm, Some(&get(vm, r, "rd_entity_scores")));
    names
        .into_iter()
        .zip(scores.into_iter().chain(std::iter::repeat(0.0)))
        .collect()
}
fn set_entity_pairs(vm: &mut Engine, r: ObjRef, pairs: Vec<(Value, f32)>) -> Result<(), VmExit> {
    let names = obj_array(
        vm,
        "[Ljava/lang/String;",
        pairs.iter().map(|(n, _)| n.clone()).collect(),
    )?;
    let scores = new_float_array(vm, &pairs.iter().map(|(_, s)| *s).collect::<Vec<_>>())?;
    put(vm, r, "rd_entity_names", names)?;
    put(vm, r, "rd_entity_scores", scores)
}
fn normalized_entities(pairs: Vec<(Value, f32)>) -> Vec<(Value, f32)> {
    let mut out: Vec<(Value, f32)> = pairs
        .into_iter()
        .filter(|(_, s)| *s > 0.0 || s.is_nan())
        .map(|(n, s)| (n, if s > 1.0 { 1.0 } else { s }))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}
fn entities_from_map(vm: &Engine, map: Option<&Value>) -> Vec<(Value, f32)> {
    map_entries(vm, map)
        .into_iter()
        .filter_map(|(k, v)| numeric_float(vm, &v).map(|s| (k, s)))
        .collect()
}
fn builder_set_entity(vm: &mut Engine, r: ObjRef, entity: Value, score: f32) -> Result<(), VmExit> {
    let mut pairs = entity_pairs(vm, r);
    if let Some(p) = pairs
        .iter_mut()
        .find(|(n, _)| objects_equal(vm, n, &entity))
    {
        p.1 = score;
    } else {
        pairs.push((entity, score));
    }
    set_entity_pairs(vm, r, pairs)
}
fn entity_at(vm: &Engine, r: ObjRef, idx: i32) -> Result<Value, VmExit> {
    let pairs = entity_pairs(vm, r);
    if idx < 0 || idx as usize >= pairs.len() {
        return Err(idx_oob(format!(
            "Index {idx} out of bounds for length {}",
            pairs.len()
        )));
    }
    Ok(pairs[idx as usize].0.clone())
}
fn entity_score(vm: &Engine, r: ObjRef, entity: Option<&Value>) -> f32 {
    let Some(entity) = entity else { return 0.0 };
    entity_pairs(vm, r)
        .iter()
        .find(|(n, _)| objects_equal(vm, n, entity))
        .map(|(_, s)| *s)
        .unwrap_or(0.0)
}
fn entity_map_string(vm: &Engine, r: ObjRef) -> String {
    let parts: Vec<String> = entity_pairs(vm, r)
        .iter()
        .map(|(n, s)| {
            format!(
                "{}={}",
                opt_str(vm, Some(n)).unwrap_or_else(|| "null".into()),
                crate::repr::java_float(*s)
            )
        })
        .collect();
    format!("{{{}}}", parts.join(", "))
}

fn entity_confidence_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            if sig.contains("Ljava/util/Map;") {
                if !array_is_obj(args.first()) {
                    return Err(npe("source"));
                }
                let pairs = normalized_entities(entities_from_map(vm, args.first()));
                set_entity_pairs(vm, recv, pairs)?;
            } else if sig.contains("EntityConfidence") {
                let src = req_obj(args, 0, "source")?;
                let pairs = entity_pairs(vm, src);
                set_entity_pairs(vm, recv, pairs)?;
            } else {
                set_entity_pairs(vm, recv, Vec::new())?;
            }
            Value::Null
        }
        "getEntities" => {
            let names: Vec<Value> = entity_pairs(vm, recv).into_iter().map(|(n, _)| n).collect();
            Value::Obj(new_list(vm, names)?)
        }
        "getConfidenceScore" => Value::Float(entity_score(vm, recv, args.first())),
        "toMap" => {
            let m = alloc_obj(vm, "Landroid/util/ArrayMap;", Vec::new())?;
            let mut entries = Vec::new();
            for (n, s) in entity_pairs(vm, recv) {
                let boxed = crate::boxes::box_value(vm, "Ljava/lang/Float;", Value::Float(s))?;
                entries.push((n, Value::Obj(boxed)));
            }
            vm.fw.objects.insert(m, HostObj::Map { entries });
            Value::Obj(m)
        }
        "toString" => jstr(vm, entity_map_string(vm, recv))?,
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn make_entity_config(
    vm: &mut Engine,
    included: Vec<Value>,
    excluded: Vec<Value>,
    hints: Vec<Value>,
    include_from_tc: bool,
) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, TEXT_CLASSIFIER_ENTITY_CONFIG, Vec::new())?;
    let inc = new_list(vm, included)?;
    let exc = new_list(vm, excluded)?;
    let hin = new_list(vm, hints)?;
    put(vm, r, "rd_included", Value::Obj(inc))?;
    put(vm, r, "rd_excluded", Value::Obj(exc))?;
    put(vm, r, "rd_hints", Value::Obj(hin))?;
    put(
        vm,
        r,
        "rd_include_from_tc",
        Value::Int(include_from_tc as i32),
    )?;
    Ok(Value::Obj(r))
}
fn entity_config_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "createWithHints" => make_entity_config(
            vm,
            Vec::new(),
            Vec::new(),
            collection_items(vm, args.first()),
            true,
        )?,
        "create" => make_entity_config(
            vm,
            collection_items(vm, args.get(1)),
            collection_items(vm, args.get(2)),
            collection_items(vm, args.first()),
            true,
        )?,
        "createWithExplicitEntityList" => make_entity_config(
            vm,
            collection_items(vm, args.first()),
            Vec::new(),
            Vec::new(),
            false,
        )?,
        _ => return Ok(None),
    }))
}
fn entity_config_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            put(vm, recv, "rd_include_from_tc", Value::Int(1))?;
            Value::Null
        }
        "setIncludedTypes" => {
            put(
                vm,
                recv,
                "rd_included",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Obj(recv)
        }
        "setExcludedTypes" => {
            put(
                vm,
                recv,
                "rd_excluded",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Obj(recv)
        }
        "setHints" => {
            put(
                vm,
                recv,
                "rd_hints",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Obj(recv)
        }
        "includeTypesFromTextClassifier" => {
            put(vm, recv, "rd_include_from_tc", Value::Int(iarg(args, 0)))?;
            Value::Obj(recv)
        }
        "build" => {
            let include = if matches!(get(vm, recv, "rd_include_from_tc"), Value::Null) {
                true
            } else {
                field_bool(vm, recv, "rd_include_from_tc")
            };
            make_entity_config(
                vm,
                collection_items(vm, Some(&get(vm, recv, "rd_included"))),
                collection_items(vm, Some(&get(vm, recv, "rd_excluded"))),
                collection_items(vm, Some(&get(vm, recv, "rd_hints"))),
                include,
            )?
        }
        _ => return Ok(None),
    }))
}
fn entity_config_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "resolveEntityListModifications" => {
            if !array_is_obj(args.first()) {
                return Err(npe("entityTypes"));
            }
            let mut out: Vec<Value> = Vec::new();
            let mut push_unique = |v: Value| {
                if !out.iter().any(|x| objects_equal(vm, x, &v)) {
                    out.push(v);
                }
            };
            if field_bool(vm, recv, "rd_include_from_tc") {
                for v in collection_items(vm, args.first()) {
                    push_unique(v);
                }
            }
            for v in collection_items(vm, Some(&get(vm, recv, "rd_included"))) {
                push_unique(v);
            }
            let excluded = collection_items(vm, Some(&get(vm, recv, "rd_excluded")));
            out.retain(|v| !excluded.iter().any(|x| objects_equal(vm, x, v)));
            // AOSP returns a HashSet; order is unspecified.
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", out)?)
        }
        "getHints" => {
            let h = get(vm, recv, "rd_hints");
            if matches!(h, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                h
            }
        }
        "shouldIncludeTypesFromTextClassifier" => {
            bool_value(field_bool(vm, recv, "rd_include_from_tc"))
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn empty_text_classification(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (TEXT_CLASSIFICATION.to_string(), "EMPTY".to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let r = alloc_obj(vm, TEXT_CLASSIFICATION, Vec::new())?;
    set_entity_pairs(vm, r, Vec::new())?;
    let actions = new_list(vm, Vec::new())?;
    put(vm, r, "rd_actions", Value::Obj(actions))?;
    let extras = empty_bundle(vm)?;
    put(vm, r, "rd_extras", extras)?;
    let v = Value::Obj(r);
    vm.statics.insert(key, v.clone());
    Ok(v)
}
fn text_classification_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => Value::Null,
        "setText" => {
            put(
                vm,
                recv,
                "rd_text",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setEntityType" => {
            // Unlike TextSelection.Builder, this AOSP method does not call
            // Objects.requireNonNull(type); ArrayMap accepts a null key.
            builder_set_entity(
                vm,
                recv,
                args.first().cloned().unwrap_or(Value::Null),
                farg(args, 1),
            )?;
            this
        }
        "setEntityConfidence" => {
            let src = req_obj(args, 0, "scores")?;
            set_entity_pairs(vm, recv, entity_pairs(vm, src))?;
            this
        }
        "clearEntityTypes" => {
            set_entity_pairs(vm, recv, Vec::new())?;
            this
        }
        "addAction" => {
            if !array_is_obj(args.first()) {
                return Err(iae(""));
            }
            let mut actions = collection_items(vm, Some(&get(vm, recv, "rd_actions")));
            actions.push(args.first().cloned().unwrap_or(Value::Null));
            let list = new_list(vm, actions)?;
            put(vm, recv, "rd_actions", Value::Obj(list))?;
            this
        }
        "addActions" => {
            if !array_is_obj(args.first()) {
                return Err(npe("actions"));
            }
            let mut actions = collection_items(vm, Some(&get(vm, recv, "rd_actions")));
            actions.extend(collection_items(vm, args.first()));
            let list = new_list(vm, actions)?;
            put(vm, recv, "rd_actions", Value::Obj(list))?;
            this
        }
        "clearActions" => {
            let list = new_list(vm, Vec::new())?;
            put(vm, recv, "rd_actions", Value::Obj(list))?;
            this
        }
        "setIcon" => {
            put(
                vm,
                recv,
                "rd_icon",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setLabel" => {
            put(
                vm,
                recv,
                "rd_label",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setIntent" => {
            put(
                vm,
                recv,
                "rd_intent",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setOnClickListener" => {
            put(
                vm,
                recv,
                "rd_onclick",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setId" => {
            put(
                vm,
                recv,
                "rd_id",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, TEXT_CLASSIFICATION, Vec::new())?;
            for f in [
                "rd_text",
                "rd_icon",
                "rd_label",
                "rd_intent",
                "rd_onclick",
                "rd_id",
            ] {
                put(vm, r, f, get(vm, recv, f))?;
            }
            let actions = collection_items(vm, Some(&get(vm, recv, "rd_actions")));
            // TextClassification's constructor wraps the builder's list with
            // Collections.unmodifiableList; the host List is a snapshot here.
            let copied = new_list(vm, actions)?;
            put(vm, r, "rd_actions", Value::Obj(copied))?;
            set_entity_pairs(vm, r, normalized_entities(entity_pairs(vm, recv)))?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_classification_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getText" => get(vm, recv, "rd_text"),
        "getEntityCount" => Value::Int(entity_pairs(vm, recv).len() as i32),
        "getEntity" => entity_at(vm, recv, iarg(args, 0))?,
        "getConfidenceScore" => Value::Float(entity_score(vm, recv, args.first())),
        "getActions" => {
            let a = get(vm, recv, "rd_actions");
            if matches!(a, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                a
            }
        }
        "getIcon" => get(vm, recv, "rd_icon"),
        "getLabel" => get(vm, recv, "rd_label"),
        "getIntent" => get(vm, recv, "rd_intent"),
        "getOnClickListener" => get(vm, recv, "rd_onclick"),
        "getId" => get(vm, recv, "rd_id"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "toBuilder" => {
            let b = alloc_obj(vm, TEXT_CLASSIFICATION_BUILDER, Vec::new())?;
            for f in [
                "rd_text",
                "rd_icon",
                "rd_label",
                "rd_intent",
                "rd_onclick",
                "rd_id",
                "rd_extras",
                "rd_actions",
            ] {
                put(vm, b, f, get(vm, recv, f))?;
            }
            set_entity_pairs(vm, b, entity_pairs(vm, recv))?;
            Value::Obj(b)
        }
        "toString" => {
            let text_v = get(vm, recv, "rd_text");
            let text = value_string(vm, &text_v)?;
            let id_v = get(vm, recv, "rd_id");
            let id = value_string(vm, &id_v)?;
            let actions_v = get(vm, recv, "rd_actions");
            let actions = if matches!(actions_v, Value::Null) {
                "[]".to_string()
            } else {
                value_string(vm, &actions_v)?
            };
            let extras_v = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            let extras = value_string(vm, &extras_v)?;
            jstr(vm, format!(
                "TextClassification {{text={text}, entities={}, actions={actions}, id={id}, extras={extras}}}",
                entity_map_string(vm, recv)
            ))?
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn text_classification_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        // The empty result is the important public value; intent/PendingIntent
        // factories depend on framework services outside this slice.
        "createIntentOnClickListener" => {
            req_obj(args, 0, "intent")?;
            let r = alloc_obj(vm, "Landroid/view/View$OnClickListener;", Vec::new())?;
            put(
                vm,
                r,
                "rd_pending_intent",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}

fn text_selection_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            let start = iarg(args, 0);
            let end = iarg(args, 1);
            if start < 0 || end <= start {
                return Err(iae(""));
            }
            put(vm, recv, "rd_start", Value::Int(start))?;
            put(vm, recv, "rd_end", Value::Int(end))?;
            Value::Null
        }
        "setEntityType" => {
            let entity = args.first().cloned().unwrap_or(Value::Null);
            if matches!(entity, Value::Null) {
                return Err(npe("type"));
            }
            builder_set_entity(vm, recv, entity, farg(args, 1))?;
            this
        }
        "setEntityConfidence" => {
            let src = req_obj(args, 0, "scores")?;
            set_entity_pairs(vm, recv, entity_pairs(vm, src))?;
            this
        }
        "setId" => {
            put(
                vm,
                recv,
                "rd_id",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setTextClassification" => {
            put(
                vm,
                recv,
                "rd_classification",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let start = field_int(vm, recv, "rd_start");
            let end = field_int(vm, recv, "rd_end");
            if start < 0 || end <= start {
                return Err(iae(""));
            }
            let r = alloc_obj(vm, TEXT_SELECTION, Vec::new())?;
            put(vm, r, "rd_start", Value::Int(start))?;
            put(vm, r, "rd_end", Value::Int(end))?;
            put(vm, r, "rd_id", get(vm, recv, "rd_id"))?;
            put(
                vm,
                r,
                "rd_classification",
                get(vm, recv, "rd_classification"),
            )?;
            set_entity_pairs(vm, r, normalized_entities(entity_pairs(vm, recv)))?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_selection_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getSelectionStartIndex" => Value::Int(field_int(vm, recv, "rd_start")),
        "getSelectionEndIndex" => Value::Int(field_int(vm, recv, "rd_end")),
        "getEntityCount" => Value::Int(entity_pairs(vm, recv).len() as i32),
        "getEntity" => entity_at(vm, recv, iarg(args, 0))?,
        "getConfidenceScore" => Value::Float(entity_score(vm, recv, args.first())),
        "getId" => get(vm, recv, "rd_id"),
        "getTextClassification" => get(vm, recv, "rd_classification"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "toBuilder" => {
            let b = alloc_obj(vm, TEXT_SELECTION_BUILDER, Vec::new())?;
            put(
                vm,
                b,
                "rd_start",
                Value::Int(field_int(vm, recv, "rd_start")),
            )?;
            put(vm, b, "rd_end", Value::Int(field_int(vm, recv, "rd_end")))?;
            put(vm, b, "rd_id", get(vm, recv, "rd_id"))?;
            put(
                vm,
                b,
                "rd_classification",
                get(vm, recv, "rd_classification"),
            )?;
            put(vm, b, "rd_extras", get(vm, recv, "rd_extras"))?;
            set_entity_pairs(vm, b, entity_pairs(vm, recv))?;
            Value::Obj(b)
        }
        "toString" => {
            let id = opt_str(vm, Some(&get(vm, recv, "rd_id"))).unwrap_or_else(|| "null".into());
            jstr(
                vm,
                format!(
                    "TextSelection {{id={}, startIndex={}, endIndex={}, entities={}}}",
                    id,
                    field_int(vm, recv, "rd_start"),
                    field_int(vm, recv, "rd_end"),
                    entity_map_string(vm, recv)
                ),
            )?
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn spanned_text_value(vm: &mut Engine, v: Option<&Value>) -> Result<Value, VmExit> {
    // Request builders wrap text in SpannedString.  The VM's CharSequence
    // handling keys length/content off Strings; retain the original value
    // when it is already a String (the overwhelming guest case), otherwise
    // materialize its toString form.
    match v {
        Some(Value::Obj(r)) => {
            if vm.heap.as_str(*r).is_ok() {
                Ok(Value::Obj(*r))
            } else {
                let s = crate::intrinsics::object_to_string(vm, *r)?;
                jstr(vm, s)
            }
        }
        _ => Ok(Value::Null),
    }
}
fn request_common_get(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getText" => get(vm, recv, "rd_text"),
        "getStartIndex" => Value::Int(field_int(vm, recv, "rd_start")),
        "getEndIndex" => Value::Int(field_int(vm, recv, "rd_end")),
        "getDefaultLocales" => get(vm, recv, "rd_locales"),
        "getReferenceTime" => get(vm, recv, "rd_ref_time"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "getCallingPackageName" => Value::Null,
        "setSystemTextClassifierMetadata" => {
            put(
                vm,
                recv,
                "rd_system_metadata",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "getSystemTextClassifierMetadata" => get(vm, recv, "rd_system_metadata"),
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn range_request_builder(vm: &mut Engine, recv: ObjRef, args: &[Value]) -> Result<(), VmExit> {
    let text = check_text_range(vm, args.first(), iarg(args, 1), iarg(args, 2))?;
    put(vm, recv, "rd_text", text)?;
    put(vm, recv, "rd_start", Value::Int(iarg(args, 1)))?;
    put(vm, recv, "rd_end", Value::Int(iarg(args, 2)))
}
fn ranged_request_build(
    vm: &mut Engine,
    class: &str,
    recv: ObjRef,
    extra_fields: &[&str],
) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, class, Vec::new())?;
    let text = spanned_text_value(vm, Some(&get(vm, recv, "rd_text")))?;
    put(vm, r, "rd_text", text)?;
    put(
        vm,
        r,
        "rd_start",
        Value::Int(field_int(vm, recv, "rd_start")),
    )?;
    put(vm, r, "rd_end", Value::Int(field_int(vm, recv, "rd_end")))?;
    for f in extra_fields {
        put(vm, r, f, get(vm, recv, f))?;
    }
    let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
    put(vm, r, "rd_extras", extras)?;
    Ok(Value::Obj(r))
}
fn text_classification_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            range_request_builder(vm, recv, args)?;
            Value::Null
        }
        "setDefaultLocales" => {
            put(
                vm,
                recv,
                "rd_locales",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setReferenceTime" => {
            put(
                vm,
                recv,
                "rd_ref_time",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => ranged_request_build(
            vm,
            TEXT_CLASSIFICATION_REQUEST,
            recv,
            &["rd_locales", "rd_ref_time"],
        )?,
        _ => return Ok(None),
    }))
}
fn text_classification_request_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    request_common_get(vm, name, recv, args)
}
fn text_selection_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            range_request_builder(vm, recv, args)?;
            Value::Null
        }
        "setDefaultLocales" => {
            put(
                vm,
                recv,
                "rd_locales",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setDarkLaunchAllowed" => {
            put(vm, recv, "rd_dark", Value::Int(iarg(args, 0)))?;
            this
        }
        "setIncludeTextClassification" => {
            put(vm, recv, "rd_include_tc", Value::Int(iarg(args, 0)))?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => ranged_request_build(
            vm,
            TEXT_SELECTION_REQUEST,
            recv,
            &["rd_locales", "rd_dark", "rd_include_tc"],
        )?,
        _ => return Ok(None),
    }))
}
fn text_selection_request_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "isDarkLaunchAllowed" => Ok(Some(bool_value(field_bool(vm, recv, "rd_dark")))),
        "shouldIncludeTextClassification" => {
            Ok(Some(bool_value(field_bool(vm, recv, "rd_include_tc"))))
        }
        _ => request_common_get(vm, name, recv, args),
    }
}

fn ulocale_tag_from_parts(lang: &str, country: &str, variant: &str) -> String {
    let mut parts = vec![lang.to_string()];
    if !country.is_empty() {
        parts.push(country.to_string());
    }
    if !variant.is_empty() {
        if country.is_empty() {
            parts.push(String::new());
        }
        parts.push(variant.to_string());
    }
    parts.join("-")
}
fn ulocale_from_tag(vm: &mut Engine, tag: &str) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, ULOCALE, Vec::new())?;
    let normalized = tag.replace('_', "-");
    let pieces: Vec<&str> = normalized.split('-').collect();
    let lang = pieces.first().copied().unwrap_or("");
    let country = pieces.get(1).copied().unwrap_or("");
    let variant = if pieces.len() > 2 {
        pieces[2..].join("-")
    } else {
        String::new()
    };
    let v = jstr(vm, lang)?;
    put(vm, r, "rd_lang", v)?;
    let v = jstr(vm, country)?;
    put(vm, r, "rd_country", v)?;
    let v = jstr(vm, variant)?;
    put(vm, r, "rd_variant", v)?;
    let v = jstr(vm, normalized)?;
    put(vm, r, "rd_tag", v)?;
    Ok(Value::Obj(r))
}
fn ulocale_tag(vm: &mut Engine, r: ObjRef) -> Result<String, VmExit> {
    if let Some(tag) = field_str(vm, r, "rd_tag") {
        if !tag.is_empty() {
            return Ok(tag);
        }
    }
    // A guest ULocale subclass/sibling implementation supplies its own
    // toLanguageTag.  Avoid recursion for the host class itself.
    if class_of(vm, r) != ULOCALE {
        let v = vm.invoke_instance(r, "toLanguageTag", "()Ljava/lang/String;", &[])?;
        return Ok(opt_str(vm, Some(&v)).unwrap_or_default());
    }
    Ok(ulocale_tag_from_parts(
        &field_str(vm, r, "rd_lang").unwrap_or_default(),
        &field_str(vm, r, "rd_country").unwrap_or_default(),
        &field_str(vm, r, "rd_variant").unwrap_or_default(),
    ))
}
fn ulocale_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            // ULocale(String), (String,String), (String,String,String).
            // The one-argument form also accepts a full locale name such as
            // "en_US"; splitting it gives the same language/country answers
            // for the ordinary tags used by TextLanguage.
            if args.len() == 1 {
                let s = opt_str_arg(vm, args, 0).unwrap_or_default();
                let normalized = s.replace('_', "-");
                let pieces: Vec<&str> = normalized.split('-').collect();
                let v = jstr(vm, pieces.first().copied().unwrap_or(""))?;
                put(vm, recv, "rd_lang", v)?;
                let v = jstr(vm, pieces.get(1).copied().unwrap_or(""))?;
                put(vm, recv, "rd_country", v)?;
                let variant = if pieces.len() > 2 {
                    pieces[2..].join("-")
                } else {
                    String::new()
                };
                let v = jstr(vm, variant)?;
                put(vm, recv, "rd_variant", v)?;
                let v = jstr(vm, normalized)?;
                put(vm, recv, "rd_tag", v)?;
            } else {
                let lang = opt_str_arg(vm, args, 0).unwrap_or_default();
                let country = opt_str_arg(vm, args, 1).unwrap_or_default();
                let variant = opt_str_arg(vm, args, 2).unwrap_or_default();
                let v = jstr(vm, &lang)?;
                put(vm, recv, "rd_lang", v)?;
                let v = jstr(vm, &country)?;
                put(vm, recv, "rd_country", v)?;
                let v = jstr(vm, &variant)?;
                put(vm, recv, "rd_variant", v)?;
                let tag = ulocale_tag_from_parts(&lang, &country, &variant);
                let v = jstr(vm, tag)?;
                put(vm, recv, "rd_tag", v)?;
            }
            Value::Null
        }
        "getLanguage" => jstr(vm, field_str(vm, recv, "rd_lang").unwrap_or_default())?,
        "getCountry" => jstr(vm, field_str(vm, recv, "rd_country").unwrap_or_default())?,
        "getVariant" => jstr(vm, field_str(vm, recv, "rd_variant").unwrap_or_default())?,
        "toLanguageTag" => {
            let tag = ulocale_tag(vm, recv)?;
            jstr(vm, tag)?
        }
        "toString" => {
            let tag = ulocale_tag(vm, recv)?.replace('-', "_");
            jstr(vm, tag)?
        }
        "equals" => {
            let mine = ulocale_tag(vm, recv)?;
            let eq = match oarg(args, 0) {
                Some(o) => ulocale_tag(vm, o).map(|t| t == mine).unwrap_or(false),
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(java_string_hash(&ulocale_tag(vm, recv)?)),
        _ => return Ok(None),
    }))
}
fn empty_text_language(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (TEXT_LANGUAGE.to_string(), "EMPTY".to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let r = alloc_obj(vm, TEXT_LANGUAGE, Vec::new())?;
    set_entity_pairs(vm, r, Vec::new())?;
    let extras = empty_bundle(vm)?;
    put(vm, r, "rd_extras", extras)?;
    let v = Value::Obj(r);
    vm.statics.insert(key, v.clone());
    Ok(v)
}
fn text_language_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => Value::Null,
        "putLocale" => {
            let locale = req_obj(args, 0, "locale")?;
            let tag = ulocale_tag(vm, locale)?;
            let tag_v = jstr(vm, tag)?;
            builder_set_entity(vm, recv, tag_v, farg(args, 1))?;
            this
        }
        "setId" => {
            put(
                vm,
                recv,
                "rd_id",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setExtras" => {
            req_obj(args, 0, "bundle")?;
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, TEXT_LANGUAGE, Vec::new())?;
            put(vm, r, "rd_id", get(vm, recv, "rd_id"))?;
            set_entity_pairs(vm, r, normalized_entities(entity_pairs(vm, recv)))?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            // Builder.build mutates mBundle to Bundle.EMPTY if unset.
            put(vm, recv, "rd_extras", extras.clone())?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_language_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getId" => get(vm, recv, "rd_id"),
        "getLocaleHypothesisCount" => Value::Int(entity_pairs(vm, recv).len() as i32),
        "getLocale" => {
            let tag_v = entity_at(vm, recv, iarg(args, 0))?;
            ulocale_from_tag(vm, &opt_str(vm, Some(&tag_v)).unwrap_or_default())?
        }
        "getConfidenceScore" => {
            let locale = req_obj(args, 0, "locale")?;
            let tag = ulocale_tag(vm, locale)?;
            let tag_v = jstr(vm, tag)?;
            Value::Float(entity_score(vm, recv, Some(&tag_v)))
        }
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "toString" => {
            let id = opt_str(vm, Some(&get(vm, recv, "rd_id"))).unwrap_or_else(|| "null".into());
            jstr(
                vm,
                format!(
                    "TextLanguage {{id={}, locales={}, bundle=Bundle[mParcelledData.dataSize=0]}}",
                    id,
                    entity_map_string(vm, recv)
                ),
            )?
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn text_language_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            if !array_is_obj(args.first()) {
                return Err(npe("text"));
            }
            put(
                vm,
                recv,
                "rd_text",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "setExtras" => {
            req_obj(args, 0, "bundle")?;
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, TEXT_LANGUAGE_REQUEST, Vec::new())?;
            // The request constructor receives mText.toString().
            let text = spanned_text_value(vm, Some(&get(vm, recv, "rd_text")))?;
            put(vm, r, "rd_text", text)?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_language_request_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    request_common_get(vm, name, recv, args)
}

fn make_text_link(
    vm: &mut Engine,
    start: i32,
    end: i32,
    pairs: Vec<(Value, f32)>,
    extras: Value,
    url_span: Value,
) -> Result<Value, VmExit> {
    let normalized = normalized_entities(pairs);
    // TextLink's private constructor checks, in order: confidence non-null
    // (already normalized here), non-empty, start <= end, extras non-null.
    if normalized.is_empty() {
        return Err(iae(""));
    }
    if start > end {
        return Err(iae(""));
    }
    if matches!(extras, Value::Null) {
        return Err(npe("extras"));
    }
    let r = alloc_obj(vm, TEXT_LINK, Vec::new())?;
    put(vm, r, "rd_start", Value::Int(start))?;
    put(vm, r, "rd_end", Value::Int(end))?;
    put(vm, r, "rd_extras", extras)?;
    put(vm, r, "rd_url_span", url_span)?;
    set_entity_pairs(vm, r, normalized)?;
    Ok(Value::Obj(r))
}
fn text_links_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            req_obj(args, 0, "fullText")?;
            put(
                vm,
                recv,
                "rd_text",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            let links = new_list(vm, Vec::new())?;
            put(vm, recv, "rd_links", Value::Obj(links))?;
            Value::Null
        }
        "addLink" => {
            let pairs = entities_from_map(vm, args.get(2));
            let (extras, url_span) = if sig.contains("Landroid/os/Bundle;") {
                (args.get(3).cloned().unwrap_or(Value::Null), Value::Null)
            } else if sig.contains("URLSpan") {
                (
                    empty_bundle(vm)?,
                    args.get(3).cloned().unwrap_or(Value::Null),
                )
            } else {
                (empty_bundle(vm)?, Value::Null)
            };
            let link = make_text_link(vm, iarg(args, 0), iarg(args, 1), pairs, extras, url_span)?;
            let mut links = collection_items(vm, Some(&get(vm, recv, "rd_links")));
            links.push(link);
            match field_obj(vm, recv, "rd_links") {
                Some(list) => {
                    vm.fw.objects.insert(list, HostObj::List { elems: links });
                }
                None => {
                    let list = new_list(vm, links)?;
                    put(vm, recv, "rd_links", Value::Obj(list))?;
                }
            }
            this
        }
        "clearTextLinks" => {
            if let Some(list) = field_obj(vm, recv, "rd_links") {
                vm.fw
                    .objects
                    .insert(list, HostObj::List { elems: Vec::new() });
            } else {
                let list = new_list(vm, Vec::new())?;
                put(vm, recv, "rd_links", Value::Obj(list))?;
            }
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, TEXT_LINKS, Vec::new())?;
            put(vm, r, "rd_text", get(vm, recv, "rd_text"))?;
            let links_field = get(vm, recv, "rd_links");
            // AOSP wraps the same ArrayList in an unmodifiable view rather
            // than copying it.  Retain the list reference likewise.
            if matches!(links_field, Value::Null) {
                let list = new_list(vm, Vec::new())?;
                put(vm, r, "rd_links", Value::Obj(list))?;
            } else {
                put(vm, r, "rd_links", links_field)?;
            }
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_links_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getText" => get(vm, recv, "rd_text"),
        "getLinks" => {
            let l = get(vm, recv, "rd_links");
            if matches!(l, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                l
            }
        }
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "apply" => {
            req_obj(args, 0, "text")?;
            // Applying spans mutates a Spannable, which is outside the
            // registered slice.  The no-link case has an exact observable
            // result we can model: no links (TextLinksParams returns
            // STATUS_NO_LINKS_FOUND = 1 for an empty link list).  For a
            // non-empty list defer to unresolved behavior.
            if collection_items(vm, Some(&get(vm, recv, "rd_links"))).is_empty() {
                Value::Int(1)
            } else {
                return Ok(None);
            }
        }
        "toString" => {
            let text =
                opt_str(vm, Some(&get(vm, recv, "rd_text"))).unwrap_or_else(|| "null".into());
            let links = collection_items(vm, Some(&get(vm, recv, "rd_links")));
            jstr(
                vm,
                format!(
                    "TextLinks{{fullText={text}, links=[{} links]}}",
                    links.len()
                ),
            )?
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn text_link_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getStart" => Value::Int(field_int(vm, recv, "rd_start")),
        "getEnd" => Value::Int(field_int(vm, recv, "rd_end")),
        "getEntityCount" => Value::Int(entity_pairs(vm, recv).len() as i32),
        "getEntity" => entity_at(vm, recv, iarg(args, 0))?,
        "getConfidenceScore" => Value::Float(entity_score(vm, recv, args.first())),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "toString" => jstr(
            vm,
            format!(
                "TextLink{{start={}, end={}, entityScores={}, urlSpan=null}}",
                field_int(vm, recv, "rd_start"),
                field_int(vm, recv, "rd_end"),
                entity_map_string(vm, recv)
            ),
        )?,
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn text_links_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            req_obj(args, 0, "text")?;
            put(
                vm,
                recv,
                "rd_text",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "rd_legacy", Value::Int(1))?;
            Value::Null
        }
        "setDefaultLocales" => {
            put(
                vm,
                recv,
                "rd_locales",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setEntityConfig" => {
            put(
                vm,
                recv,
                "rd_config",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setLegacyFallback" => {
            put(vm, recv, "rd_legacy", Value::Int(iarg(args, 0)))?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setReferenceTime" => {
            put(
                vm,
                recv,
                "rd_ref_time",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, TEXT_LINKS_REQUEST, Vec::new())?;
            for f in [
                "rd_text",
                "rd_locales",
                "rd_config",
                "rd_legacy",
                "rd_ref_time",
            ] {
                put(vm, r, f, get(vm, recv, f))?;
            }
            if matches!(get(vm, r, "rd_legacy"), Value::Null) {
                put(vm, r, "rd_legacy", Value::Int(1))?;
            }
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn text_links_request_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "getEntityConfig" => Ok(Some(get(vm, recv, "rd_config"))),
        "isLegacyFallback" => Ok(Some(bool_value(field_bool(vm, recv, "rd_legacy")))),
        _ => request_common_get(vm, name, recv, args),
    }
}

fn conversation_actions_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            if !array_is_obj(args.first()) {
                return Err(npe("conversationActions"));
            }
            // AOSP stores Collections.unmodifiableList(new ArrayList<>(...)).
            let copied = new_list(vm, collection_items(vm, args.first()))?;
            put(vm, recv, "rd_actions", Value::Obj(copied))?;
            put(
                vm,
                recv,
                "rd_id",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "getConversationActions" => {
            let a = get(vm, recv, "rd_actions");
            if matches!(a, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                a
            }
        }
        "getId" => get(vm, recv, "rd_id"),
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn conversation_action_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            req_obj(args, 0, "actionType")?;
            put(
                vm,
                recv,
                "rd_type",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "setAction" => {
            put(
                vm,
                recv,
                "rd_action",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setTextReply" => {
            put(
                vm,
                recv,
                "rd_reply",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setConfidenceScore" => {
            put(vm, recv, "rd_score", Value::Float(farg(args, 0)))?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let ty = get(vm, recv, "rd_type");
            if matches!(ty, Value::Null) {
                return Err(npe("type"));
            }
            let r = alloc_obj(vm, CONVERSATION_ACTION, Vec::new())?;
            put(vm, r, "rd_type", ty)?;
            put(vm, r, "rd_action", get(vm, recv, "rd_action"))?;
            put(vm, r, "rd_reply", get(vm, recv, "rd_reply"))?;
            put(vm, r, "rd_score", get(vm, recv, "rd_score"))?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn conversation_action_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getType" => get(vm, recv, "rd_type"),
        "getAction" => get(vm, recv, "rd_action"),
        "getConfidenceScore" => Value::Float(field_float(vm, recv, "rd_score")),
        "getTextReply" => get(vm, recv, "rd_reply"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "toBuilder" => {
            let b = alloc_obj(vm, CONVERSATION_ACTION_BUILDER, Vec::new())?;
            for f in ["rd_type", "rd_action", "rd_reply", "rd_score", "rd_extras"] {
                put(vm, b, f, get(vm, recv, f))?;
            }
            Value::Obj(b)
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn conversation_message_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            req_obj(args, 0, "author")?;
            put(
                vm,
                recv,
                "rd_author",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "setText" => {
            put(
                vm,
                recv,
                "rd_text",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setReferenceTime" => {
            put(
                vm,
                recv,
                "rd_ref_time",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, CONVERSATION_MESSAGE, Vec::new())?;
            put(vm, r, "rd_author", get(vm, recv, "rd_author"))?;
            put(vm, r, "rd_ref_time", get(vm, recv, "rd_ref_time"))?;
            let text = get(vm, recv, "rd_text");
            let wrapped = if matches!(text, Value::Null) {
                Value::Null
            } else {
                spanned_text_value(vm, Some(&text))?
            };
            put(vm, r, "rd_text", wrapped)?;
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn conversation_message_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getAuthor" => get(vm, recv, "rd_author"),
        "getReferenceTime" => get(vm, recv, "rd_ref_time"),
        "getText" => get(vm, recv, "rd_text"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn conversation_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let this = Value::Obj(recv);
    Ok(Some(match name {
        "<init>" => {
            req_obj(args, 0, "conversation")?;
            put(
                vm,
                recv,
                "rd_conversation",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "rd_max", Value::Int(-1))?;
            Value::Null
        }
        "setHints" => {
            put(
                vm,
                recv,
                "rd_hints",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setTypeConfig" => {
            put(
                vm,
                recv,
                "rd_config",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "setMaxSuggestions" => {
            let max = iarg(args, 0);
            if max < -1 {
                return Err(iae("maxSuggestions has to be greater than or equal to -1."));
            }
            put(vm, recv, "rd_max", Value::Int(max))?;
            this
        }
        "setExtras" => {
            put(
                vm,
                recv,
                "rd_extras",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            this
        }
        "build" => {
            let r = alloc_obj(vm, CONVERSATION_REQUEST, Vec::new())?;
            // AOSP wraps the caller's list in an unmodifiable view (no copy).
            put(vm, r, "rd_conversation", get(vm, recv, "rd_conversation"))?;
            let config = get(vm, recv, "rd_config");
            if matches!(config, Value::Null) {
                let default_config =
                    make_entity_config(vm, Vec::new(), Vec::new(), Vec::new(), true)?;
                put(vm, r, "rd_config", default_config)?;
            } else {
                put(vm, r, "rd_config", config)?;
            }
            let max = get(vm, recv, "rd_max");
            put(
                vm,
                r,
                "rd_max",
                if matches!(max, Value::Null) {
                    Value::Int(-1)
                } else {
                    max
                },
            )?;
            let hints = get(vm, recv, "rd_hints");
            if matches!(hints, Value::Null) {
                let empty = new_list(vm, Vec::new())?;
                put(vm, r, "rd_hints", Value::Obj(empty))?;
            } else {
                put(vm, r, "rd_hints", hints)?;
            }
            let extras = bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?;
            put(vm, r, "rd_extras", extras)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
fn conversation_request_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "getTypeConfig" => get(vm, recv, "rd_config"),
        "getConversation" => get(vm, recv, "rd_conversation"),
        "getMaxSuggestions" => Value::Int(field_int(vm, recv, "rd_max")),
        "getHints" => {
            let h = get(vm, recv, "rd_hints");
            if matches!(h, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                h
            }
        }
        "getCallingPackageName" => Value::Null,
        "setSystemTextClassifierMetadata" => {
            put(
                vm,
                recv,
                "rd_system_metadata",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "getSystemTextClassifierMetadata" => get(vm, recv, "rd_system_metadata"),
        "getExtras" => bundle_or_empty(vm, Some(&get(vm, recv, "rd_extras")))?,
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn default_text_selection(vm: &mut Engine, request: ObjRef) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, TEXT_SELECTION, Vec::new())?;
    put(
        vm,
        r,
        "rd_start",
        Value::Int(field_int(vm, request, "rd_start")),
    )?;
    put(
        vm,
        r,
        "rd_end",
        Value::Int(field_int(vm, request, "rd_end")),
    )?;
    set_entity_pairs(vm, r, Vec::new())?;
    let extras = empty_bundle(vm)?;
    put(vm, r, "rd_extras", extras)?;
    Ok(Value::Obj(r))
}
fn text_classifier_default(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // The default-interface/inherited behavior: checkMainThread only logs.
    Ok(Some(match name {
        "suggestSelection" => {
            if sig.contains("TextSelection$Request") {
                let req = req_obj(args, 0, "request")?;
                default_text_selection(vm, req)?
            } else {
                // Legacy overload builds a default request, which validates.
                let _text = check_text_range(vm, args.first(), iarg(args, 1), iarg(args, 2))?;
                let r = alloc_obj(vm, TEXT_SELECTION, Vec::new())?;
                put(vm, r, "rd_start", Value::Int(iarg(args, 1)))?;
                put(vm, r, "rd_end", Value::Int(iarg(args, 2)))?;
                set_entity_pairs(vm, r, Vec::new())?;
                let extras = empty_bundle(vm)?;
                put(vm, r, "rd_extras", extras)?;
                Value::Obj(r)
            }
        }
        "classifyText" => {
            if sig.contains("TextClassification$Request") {
                req_obj(args, 0, "request")?;
            } else {
                // Legacy overload builds and validates a request internally.
                let _text = check_text_range(vm, args.first(), iarg(args, 1), iarg(args, 2))?;
            }
            empty_text_classification(vm)?
        }
        "generateLinks" => {
            if sig.contains("TextLinks$Request") {
                let req = req_obj(args, 0, "request")?;
                let text = get(vm, req, "rd_text");
                // The interface default delegates to the legacy overload
                // with (request text, request config); its legacy result is
                // an empty TextLinks over the same CharSequence.
                let r = alloc_obj(vm, TEXT_LINKS, Vec::new())?;
                put(vm, r, "rd_text", text)?;
                let links = new_list(vm, Vec::new())?;
                put(vm, r, "rd_links", Value::Obj(links))?;
                let extras = empty_bundle(vm)?;
                put(vm, r, "rd_extras", extras)?;
                Value::Obj(r)
            } else {
                req_obj(args, 0, "text")?;
                let r = alloc_obj(vm, TEXT_LINKS, Vec::new())?;
                put(
                    vm,
                    r,
                    "rd_text",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                let links = new_list(vm, Vec::new())?;
                put(vm, r, "rd_links", Value::Obj(links))?;
                let extras = empty_bundle(vm)?;
                put(vm, r, "rd_extras", extras)?;
                Value::Obj(r)
            }
        }
        "applyLinks" => {
            // The default TextLinks has no links, so TextLinks.apply returns
            // STATUS_NO_LINKS_FOUND (= 1).  A null text is rejected by
            // the inner request builder/text check; require it.
            req_obj(args, 0, "text")?;
            Value::Int(1)
        }
        "getMaxGenerateLinksTextLength" => Value::Int(i32::MAX),
        "detectLanguage" => {
            if sig.contains("TextLanguage$Request") {
                req_obj(args, 0, "request")?;
            }
            empty_text_language(vm)?
        }
        "suggestConversationActions" => {
            req_obj(args, 0, "request")?;
            let r = alloc_obj(vm, CONVERSATION_ACTIONS, Vec::new())?;
            let actions = new_list(vm, Vec::new())?;
            put(vm, r, "rd_actions", Value::Obj(actions))?;
            Value::Obj(r)
        }
        "onSelectionEvent" | "onTextClassifierEvent" | "destroy" => Value::Null,
        "dump" => Value::Null,
        _ => return Ok(None),
    }))
}
fn system_text_classifier_manager_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => Value::Null,
        "setTextClassifier" => {
            put(
                vm,
                recv,
                "rd_custom_classifier",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "getTextClassifier" => {
            let custom = get(vm, recv, "rd_custom_classifier");
            if !matches!(custom, Value::Null) {
                custom
            } else {
                let system = get(vm, recv, "rd_system_classifier");
                if matches!(system, Value::Null) {
                    let c = alloc_obj(vm, SYSTEM_TEXT_CLASSIFIER, Vec::new())?;
                    put(vm, recv, "rd_system_classifier", Value::Obj(c))?;
                    Value::Obj(c)
                } else {
                    system
                }
            }
        }
        "getSystemTextClassifier" => {
            let system = get(vm, recv, "rd_system_classifier");
            if matches!(system, Value::Null) {
                let c = alloc_obj(vm, SYSTEM_TEXT_CLASSIFIER, Vec::new())?;
                put(vm, recv, "rd_system_classifier", Value::Obj(c))?;
                Value::Obj(c)
            } else {
                system
            }
        }
        _ => return Ok(None),
    }))
}
// ---------------------------------------------------------------------------
// android.hardware.camera2(.params): value classes and a no-device manager
// ---------------------------------------------------------------------------

fn hash_code_helpers_ints(xs: &[i32]) -> i32 {
    bernstein_ints(xs)
}
fn arrays_hash_code_ints(xs: &[i32]) -> i32 {
    let mut h = 1i32;
    for &x in xs {
        h = h.wrapping_mul(31).wrapping_add(x);
    }
    h
}
fn size_parts(vm: &Engine, v: Option<&Value>) -> Option<(i32, i32)> {
    let r = obj(v)?;
    Some((field_int(vm, r, "rd_w"), field_int(vm, r, "rd_h")))
}
fn point_parts(vm: &Engine, v: Option<&Value>) -> Option<(i32, i32)> {
    let r = obj(v)?;
    Some((field_int(vm, r, "x"), field_int(vm, r, "y")))
}
fn rect_parts(vm: &Engine, v: Option<&Value>) -> Option<(i32, i32, i32, i32)> {
    let r = obj(v)?;
    Some((
        field_int(vm, r, "left"),
        field_int(vm, r, "top"),
        field_int(vm, r, "right"),
        field_int(vm, r, "bottom"),
    ))
}
fn new_size(vm: &mut Engine, w: i32, h: i32) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Landroid/util/Size;", Vec::new())?;
    put(vm, r, "rd_w", Value::Int(w))?;
    put(vm, r, "rd_h", Value::Int(h))?;
    Ok(Value::Obj(r))
}
fn new_point(vm: &mut Engine, x: i32, y: i32) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Landroid/graphics/Point;", Vec::new())?;
    put(vm, r, "x", Value::Int(x))?;
    put(vm, r, "y", Value::Int(y))?;
    Ok(Value::Obj(r))
}
fn new_rect(vm: &mut Engine, l: i32, t: i32, rgt: i32, btm: i32) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Landroid/graphics/Rect;", Vec::new())?;
    put(vm, r, "left", Value::Int(l))?;
    put(vm, r, "top", Value::Int(t))?;
    put(vm, r, "right", Value::Int(rgt))?;
    put(vm, r, "bottom", Value::Int(btm))?;
    Ok(Value::Obj(r))
}
fn new_rational(vm: &mut Engine, n: i32, d: i32) -> Result<Value, VmExit> {
    // Mirror the android_util Rational normalization.
    let (mut n64, mut d64) = (n as i64, d as i64);
    if d64 < 0 {
        n64 = -n64;
        d64 = -d64;
    }
    fn gcd(mut a: i64, mut b: i64) -> i64 {
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        a
    }
    let g = gcd(n64.abs(), d64.abs()).max(1);
    let r = alloc_obj(vm, "Landroid/util/Rational;", Vec::new())?;
    put(vm, r, "rd_num", Value::Int((n64 / g) as i32))?;
    put(vm, r, "rd_den", Value::Int((d64 / g) as i32))?;
    Ok(Value::Obj(r))
}
fn nonnegative(value: i32, what: &str) -> Result<i32, VmExit> {
    if value < 0 {
        Err(iae(format!("{what} must be nonnegative")))
    } else {
        Ok(value)
    }
}
fn metering_rectangle_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let (x, y, w, h, weight) = if sig == "(IIIII)V" {
                let weight = iarg(args, 4);
                if !(0..=1000).contains(&weight) {
                    return Err(iae("meteringWeight out of range"));
                }
                (
                    nonnegative(iarg(args, 0), "x")?,
                    nonnegative(iarg(args, 1), "y")?,
                    nonnegative(iarg(args, 2), "width")?,
                    nonnegative(iarg(args, 3), "height")?,
                    weight,
                )
            } else if sig.contains("Landroid/graphics/Point;") {
                let p = point_parts(vm, args.first()).ok_or_else(|| npe("xy must not be null"))?;
                let s = size_parts(vm, args.get(1))
                    .ok_or_else(|| npe("dimensions must not be null"))?;
                (
                    nonnegative(p.0, "x")?,
                    nonnegative(p.1, "y")?,
                    nonnegative(s.0, "width")?,
                    nonnegative(s.1, "height")?,
                    nonnegative(iarg(args, 2), "meteringWeight")?,
                )
            } else {
                let r = rect_parts(vm, args.first()).ok_or_else(|| npe("rect must not be null"))?;
                (
                    nonnegative(r.0, "rect.left")?,
                    nonnegative(r.1, "rect.top")?,
                    nonnegative(r.2 - r.0, "rect.width")?,
                    nonnegative(r.3 - r.1, "rect.height")?,
                    nonnegative(iarg(args, 1), "meteringWeight")?,
                )
            };
            put(vm, recv, "rd_x", Value::Int(x))?;
            put(vm, recv, "rd_y", Value::Int(y))?;
            put(vm, recv, "rd_w", Value::Int(w))?;
            put(vm, recv, "rd_h", Value::Int(h))?;
            put(vm, recv, "rd_weight", Value::Int(weight))?;
            Value::Null
        }
        "getX" => Value::Int(field_int(vm, recv, "rd_x")),
        "getY" => Value::Int(field_int(vm, recv, "rd_y")),
        "getWidth" => Value::Int(field_int(vm, recv, "rd_w")),
        "getHeight" => Value::Int(field_int(vm, recv, "rd_h")),
        "getMeteringWeight" => Value::Int(field_int(vm, recv, "rd_weight")),
        "getPoint" => new_point(vm, field_int(vm, recv, "rd_x"), field_int(vm, recv, "rd_y"))?,
        "getSize" => new_size(vm, field_int(vm, recv, "rd_w"), field_int(vm, recv, "rd_h"))?,
        "getRect" => new_rect(
            vm,
            field_int(vm, recv, "rd_x"),
            field_int(vm, recv, "rd_y"),
            field_int(vm, recv, "rd_x") + field_int(vm, recv, "rd_w"),
            field_int(vm, recv, "rd_y") + field_int(vm, recv, "rd_h"),
        )?,
        "equals" => {
            let vals = ["rd_x", "rd_y", "rd_w", "rd_h", "rd_weight"];
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == METERING_RECTANGLE
                        && vals
                            .iter()
                            .all(|f| field_int(vm, o, f) == field_int(vm, recv, f))
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(hash_code_helpers_ints(&[
            field_int(vm, recv, "rd_x"),
            field_int(vm, recv, "rd_y"),
            field_int(vm, recv, "rd_w"),
            field_int(vm, recv, "rd_h"),
            field_int(vm, recv, "rd_weight"),
        ])),
        "toString" => jstr(
            vm,
            format!(
                "(x:{}, y:{}, w:{}, h:{}, wt:{})",
                field_int(vm, recv, "rd_x"),
                field_int(vm, recv, "rd_y"),
                field_int(vm, recv, "rd_w"),
                field_int(vm, recv, "rd_h"),
                field_int(vm, recv, "rd_weight")
            ),
        )?,
        _ => return Ok(None),
    }))
}
fn black_level_pattern_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let offsets =
        |vm: &Engine, r: ObjRef| -> Vec<i32> { int_values(vm, Some(&get(vm, r, "rd_offsets"))) };
    Ok(Some(match name {
        "<init>" => {
            if !array_is_obj(args.first()) {
                return Err(npe("Null offsets array passed to constructor"));
            }
            let mut xs = int_values(vm, args.first());
            if xs.len() < 4 {
                return Err(iae("Invalid offsets array length"));
            }
            xs.truncate(4);
            let stored = new_int_array(vm, &xs)?;
            put(vm, recv, "rd_offsets", stored)?;
            Value::Null
        }
        "getOffsetForIndex" => {
            let (column, row) = (iarg(args, 0), iarg(args, 1));
            if row < 0 || column < 0 {
                return Err(iae("column, row arguments must be positive"));
            }
            let xs = offsets(vm, recv);
            let idx = (((row & 1) << 1) | (column & 1)) as usize;
            Value::Int(*xs.get(idx).unwrap_or(&0))
        }
        "copyTo" => {
            let dest = match oarg(args, 0) {
                Some(d) => d,
                None => return Err(npe("destination must not be null")),
            };
            let offset = iarg(args, 1);
            if offset < 0 {
                return Err(iae("Null offset passed to copyTo"));
            }
            let values = offsets(vm, recv);
            let valid = {
                let (elems, _) = vm.heap.as_array_elems(dest)?;
                elems.len() as i32 - offset >= 4
            };
            if !valid {
                return Err(oob("destination too small to fit elements"));
            }
            let elems = vm.heap.array_elems_mut(dest)?;
            for i in 0..4 {
                elems[(offset as usize) + i] = Value::Int(values[i]);
            }
            Value::Null
        }
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == BLACK_LEVEL_PATTERN && offsets(vm, o) == offsets(vm, recv)
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(arrays_hash_code_ints(&offsets(vm, recv))),
        "toString" => {
            let x = offsets(vm, recv);
            jstr(
                vm,
                format!(
                    "BlackLevelPattern([{}, {}], [{}, {}])",
                    x.first().unwrap_or(&0),
                    x.get(1).unwrap_or(&0),
                    x.get(2).unwrap_or(&0),
                    x.get(3).unwrap_or(&0)
                ),
            )?
        }
        _ => return Ok(None),
    }))
}
fn rggb_channel_vector_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let channels = |vm: &Engine, r: ObjRef| -> [f32; 4] {
        [
            field_float(vm, r, "rd_red"),
            field_float(vm, r, "rd_green_even"),
            field_float(vm, r, "rd_green_odd"),
            field_float(vm, r, "rd_blue"),
        ]
    };
    Ok(Some(match name {
        "<init>" => {
            for i in 0..4 {
                if !farg(args, i).is_finite() {
                    return Err(iae(""));
                }
            }
            put(vm, recv, "rd_red", Value::Float(farg(args, 0)))?;
            put(vm, recv, "rd_green_even", Value::Float(farg(args, 1)))?;
            put(vm, recv, "rd_green_odd", Value::Float(farg(args, 2)))?;
            put(vm, recv, "rd_blue", Value::Float(farg(args, 3)))?;
            Value::Null
        }
        "getRed" => Value::Float(field_float(vm, recv, "rd_red")),
        "getGreenEven" => Value::Float(field_float(vm, recv, "rd_green_even")),
        "getGreenOdd" => Value::Float(field_float(vm, recv, "rd_green_odd")),
        "getBlue" => Value::Float(field_float(vm, recv, "rd_blue")),
        "getComponent" => {
            let i = iarg(args, 0);
            let cs = channels(vm, recv);
            match cs.get(i as usize) {
                Some(v) if i >= 0 => Value::Float(*v),
                _ => return Err(iae("")),
            }
        }
        "copyTo" => {
            let dest = match oarg(args, 0) {
                Some(d) => d,
                None => return Err(npe("destination must not be null")),
            };
            let offset = iarg(args, 1);
            let cs = channels(vm, recv);
            let valid = {
                let (elems, _) = vm.heap.as_array_elems(dest)?;
                offset >= 0 && elems.len() as i32 - offset >= 4
            };
            if !valid {
                return Err(oob("destination too small to fit elements"));
            }
            let elems = vm.heap.array_elems_mut(dest)?;
            for i in 0..4 {
                elems[(offset as usize) + i] = Value::Float(cs[i]);
            }
            Value::Null
        }
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == RGGB_CHANNEL_VECTOR && channels(vm, o) == channels(vm, recv)
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => {
            let c = channels(vm, recv);
            Value::Int(
                (c[0].to_bits() as i32)
                    ^ (c[1].to_bits() as i32)
                    ^ (c[2].to_bits() as i32)
                    ^ (c[3].to_bits() as i32),
            )
        }
        "toString" => {
            let c = channels(vm, recv);
            jstr(
                vm,
                format!(
                    "RggbChannelVector{{R:{}, G_even:{}, G_odd:{}, B:{}}}",
                    c[0], c[1], c[2], c[3]
                ),
            )?
        }
        _ => return Ok(None),
    }))
}
fn normalized_rational_pair(n: i32, d: i32) -> (i32, i32) {
    let (mut n64, mut d64) = (n as i64, d as i64);
    if d64 < 0 {
        n64 = -n64;
        d64 = -d64;
    }
    let mut a = n64.abs();
    let mut b = d64.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    let g = a.max(1);
    ((n64 / g) as i32, (d64 / g) as i32)
}
fn color_space_transform_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let elements =
        |vm: &Engine, r: ObjRef| -> Vec<i32> { int_values(vm, Some(&get(vm, r, "rd_elements"))) };
    Ok(Some(match name {
        "<init>" => {
            if sig.contains("[Landroid/util/Rational;") {
                let rats = match args.first() {
                    Some(v) if array_is_obj(Some(v)) => array_elems(vm, Some(v)),
                    _ => return Err(npe("elements must not be null")),
                };
                if rats.len() != 9 {
                    return Err(iae("elements must be 9 length"));
                }
                let mut xs = Vec::new();
                for v in &rats {
                    let r = obj(Some(v)).ok_or_else(|| npe("element must not be null"))?;
                    xs.push(field_int(vm, r, "rd_num"));
                    xs.push(field_int(vm, r, "rd_den"));
                }
                let stored = new_int_array(vm, &xs)?;
                put(vm, recv, "rd_elements", stored)?;
            } else {
                if !array_is_obj(args.first()) {
                    return Err(npe("elements must not be null"));
                }
                let xs = int_values(vm, args.first());
                if xs.len() != 18 {
                    return Err(iae("elements must be 18 length"));
                }
                let stored = new_int_array(vm, &xs)?;
                put(vm, recv, "rd_elements", stored)?;
            }
            Value::Null
        }
        "getElement" => {
            let (column, row) = (iarg(args, 0), iarg(args, 1));
            if !(0..3).contains(&column) {
                return Err(iae("column out of range"));
            }
            if !(0..3).contains(&row) {
                return Err(iae("row out of range"));
            }
            let xs = elements(vm, recv);
            let i = ((row * 3 + column) * 2) as usize;
            new_rational(vm, *xs.get(i).unwrap_or(&0), *xs.get(i + 1).unwrap_or(&0))?
        }
        "copyElements" => {
            let offset = iarg(args, 1);
            nonnegative(offset, "offset must not be negative")?;
            let dest = match oarg(args, 0) {
                Some(d) => d,
                None => return Err(npe("destination must not be null")),
            };
            let xs = elements(vm, recv);
            let needed = if sig.contains("[Landroid/util/Rational;") {
                9
            } else {
                18
            };
            let valid = {
                let (elems, _) = vm.heap.as_array_elems(dest)?;
                elems.len() as i32 - offset >= needed
            };
            if !valid {
                return Err(oob("destination too small to fit elements"));
            }
            if sig.contains("[Landroid/util/Rational;") {
                let mut vals = Vec::new();
                for i in 0..9 {
                    vals.push(new_rational(vm, xs[i * 2], xs[i * 2 + 1])?);
                }
                let elems = vm.heap.array_elems_mut(dest)?;
                for (i, v) in vals.into_iter().enumerate() {
                    elems[(offset as usize) + i] = v;
                }
            } else {
                let elems = vm.heap.array_elems_mut(dest)?;
                for i in 0..18 {
                    elems[(offset as usize) + i] = Value::Int(xs[i]);
                }
            }
            Value::Null
        }
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) if class_of(vm, o) == COLOR_SPACE_TRANSFORM => {
                    let a = elements(vm, recv);
                    let b = elements(vm, o);
                    a.len() == b.len()
                        && (0..a.len()).step_by(2).all(|i| {
                            normalized_rational_pair(a[i], *a.get(i + 1).unwrap_or(&0))
                                == normalized_rational_pair(b[i], *b.get(i + 1).unwrap_or(&0))
                        })
                }
                _ => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(hash_code_helpers_ints(&elements(vm, recv))),
        "toString" => {
            let xs = elements(vm, recv);
            let mut rows = Vec::new();
            for row in 0..3 {
                let mut cells = Vec::new();
                for col in 0..3 {
                    let i = (row * 3 + col) * 2;
                    cells.push(format!(
                        "{}/{}",
                        xs.get(i).unwrap_or(&0),
                        xs.get(i + 1).unwrap_or(&0)
                    ));
                }
                rows.push(format!("[{}]", cells.join(", ")));
            }
            jstr(vm, format!("ColorSpaceTransform({})", rows.join(", ")))?
        }
        _ => return Ok(None),
    }))
}

const HAL_FORMAT_RAW16: i32 = 0x20;
const HAL_FORMAT_BLOB: i32 = 0x21;
const HAL_FORMAT_IMPLEMENTATION_DEFINED: i32 = 0x22;
const HAL_FORMAT_YCBCR_420_888: i32 = 0x23;
const HAL_FORMAT_RAW_OPAQUE: i32 = 0x24;
const HAL_FORMAT_RAW10: i32 = 0x25;
const HAL_FORMAT_RAW12: i32 = 0x26;
const HAL_FORMAT_Y16: i32 = 0x2036_3159;
const IMAGE_JPEG: i32 = 0x100;
const IMAGE_DEPTH_JPEG: i32 = 0x6965_6963;
const IMAGE_JPEG_R: i32 = 0x1005;
const IMAGE_DEPTH16: i32 = 0x4436_3159;
const IMAGE_DEPTH_POINT_CLOUD: i32 = 0x101;
const IMAGE_RAW_DEPTH: i32 = 0x1002;
const IMAGE_RAW_DEPTH10: i32 = 0x1003;
const IMAGE_PRIVATE: i32 = 0x22;
const IMAGE_HEIC: i32 = 0x4845_4946;
const DATASPACE_UNKNOWN: i32 = 0;
const DATASPACE_DEPTH: i32 = 0x1000;
const DATASOURCE_DYNAMIC_DEPTH: i32 = 0x1002;
const DATASOURCE_HEIF: i32 = 0x1004;
const DATASOURCE_JPEG_R: i32 = 0x1005;
const DATASOURCE_V0_JFIF: i32 = (2 << 16) | (3 << 22) | (1 << 27);
fn is_public_format(format: i32) -> bool {
    matches!(
        format,
        // ImageFormat public formats.
        4 | 0x10 | 0x14 | 0x3231_5659 | IMAGE_JPEG | 0x11 | HAL_FORMAT_YCBCR_420_888 | 0x27 | 0x28 | 0x36
            | 0x29 | 0x2a | HAL_FORMAT_RAW16 | HAL_FORMAT_RAW_OPAQUE | HAL_FORMAT_RAW10
            | HAL_FORMAT_RAW12 | IMAGE_DEPTH16 | IMAGE_DEPTH_POINT_CLOUD | IMAGE_PRIVATE
            | IMAGE_RAW_DEPTH | IMAGE_RAW_DEPTH10 | 0x2020_3859 | IMAGE_DEPTH_JPEG | IMAGE_HEIC
            | IMAGE_JPEG_R
            // PixelFormat public formats.
            | 1 | 2 | 3 | 0x16 | 0x2b
    )
}
fn check_argument_format(format: i32) -> Result<i32, VmExit> {
    if is_public_format(format) {
        Ok(format)
    } else {
        Err(iae(format!(
            "format 0x{format:x} was not defined in either ImageFormat or PixelFormat"
        )))
    }
}
fn check_argument_format_internal(format: i32) -> Result<i32, VmExit> {
    match format {
        HAL_FORMAT_IMPLEMENTATION_DEFINED
        | HAL_FORMAT_BLOB
        | HAL_FORMAT_RAW_OPAQUE
        | HAL_FORMAT_Y16 => Ok(format),
        IMAGE_JPEG | IMAGE_HEIC => Err(iae(format!("An unknown internal format: {format}"))),
        _ => check_argument_format(format),
    }
}
fn image_format_to_public(format: i32) -> Result<i32, VmExit> {
    match format {
        HAL_FORMAT_BLOB => Ok(IMAGE_JPEG),
        IMAGE_JPEG => Err(iae("ImageFormat.JPEG is an unknown internal format")),
        _ => Ok(format),
    }
}
fn depth_format_to_public(format: i32) -> Result<i32, VmExit> {
    match format {
        HAL_FORMAT_BLOB => Ok(IMAGE_DEPTH_POINT_CLOUD),
        HAL_FORMAT_Y16 => Ok(IMAGE_DEPTH16),
        HAL_FORMAT_RAW16 => Ok(IMAGE_RAW_DEPTH),
        HAL_FORMAT_RAW10 => Ok(IMAGE_RAW_DEPTH10),
        IMAGE_JPEG => Err(iae("ImageFormat.JPEG is an unknown internal format")),
        HAL_FORMAT_IMPLEMENTATION_DEFINED => {
            Err(iae("IMPLEMENTATION_DEFINED must not leak to public API"))
        }
        _ => Err(iae(format!("Unknown DATASPACE_DEPTH format {format}"))),
    }
}
fn image_format_to_internal(format: i32) -> i32 {
    match format {
        IMAGE_JPEG | IMAGE_DEPTH_POINT_CLOUD | IMAGE_DEPTH_JPEG | IMAGE_HEIC | IMAGE_JPEG_R => {
            HAL_FORMAT_BLOB
        }
        IMAGE_DEPTH16 => HAL_FORMAT_Y16,
        IMAGE_RAW_DEPTH => HAL_FORMAT_RAW16,
        IMAGE_RAW_DEPTH10 => HAL_FORMAT_RAW10,
        _ => format,
    }
}
fn image_format_to_dataspace(format: i32) -> i32 {
    match format {
        IMAGE_JPEG => DATASOURCE_V0_JFIF,
        IMAGE_DEPTH_POINT_CLOUD | IMAGE_DEPTH16 | IMAGE_RAW_DEPTH | IMAGE_RAW_DEPTH10 => {
            DATASPACE_DEPTH
        }
        IMAGE_DEPTH_JPEG => DATASOURCE_DYNAMIC_DEPTH,
        IMAGE_HEIC => DATASOURCE_HEIF,
        IMAGE_JPEG_R => DATASOURCE_JPEG_R,
        _ => DATASPACE_UNKNOWN,
    }
}
fn positive(value: i32, what: &str) -> Result<i32, VmExit> {
    if value <= 0 {
        Err(iae(format!("{what} must be positive")))
    } else {
        Ok(value)
    }
}
fn stream_configuration_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            put(
                vm,
                recv,
                "rd_format",
                Value::Int(check_argument_format_internal(iarg(args, 0))?),
            )?;
            put(
                vm,
                recv,
                "rd_w",
                Value::Int(positive(iarg(args, 1), "width")?),
            )?;
            put(
                vm,
                recv,
                "rd_h",
                Value::Int(positive(iarg(args, 2), "height")?),
            )?;
            put(vm, recv, "rd_input", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        "getFormat" => Value::Int(field_int(vm, recv, "rd_format")),
        "getWidth" => Value::Int(field_int(vm, recv, "rd_w")),
        "getHeight" => Value::Int(field_int(vm, recv, "rd_h")),
        "getSize" => new_size(vm, field_int(vm, recv, "rd_w"), field_int(vm, recv, "rd_h"))?,
        "isInput" => bool_value(field_bool(vm, recv, "rd_input")),
        "isOutput" => bool_value(!field_bool(vm, recv, "rd_input")),
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == STREAM_CONFIGURATION
                        && ["rd_format", "rd_w", "rd_h", "rd_input"]
                            .iter()
                            .all(|f| field_int(vm, o, f) == field_int(vm, recv, f))
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(hash_code_helpers_ints(&[
            field_int(vm, recv, "rd_format"),
            field_int(vm, recv, "rd_w"),
            field_int(vm, recv, "rd_h"),
            field_bool(vm, recv, "rd_input") as i32,
        ])),
        _ => return Ok(None),
    }))
}
fn stream_configuration_duration_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let duration = args
                .get(3)
                .map(|v| match v {
                    Value::Long(l) => *l,
                    Value::Int(i) => *i as i64,
                    _ => 0,
                })
                .unwrap_or(0);
            if duration < 0 {
                return Err(iae("durationNs must be non-negative"));
            }
            put(
                vm,
                recv,
                "rd_format",
                Value::Int(check_argument_format_internal(iarg(args, 0))?),
            )?;
            put(
                vm,
                recv,
                "rd_w",
                Value::Int(positive(iarg(args, 1), "width")?),
            )?;
            put(
                vm,
                recv,
                "rd_h",
                Value::Int(positive(iarg(args, 2), "height")?),
            )?;
            put(vm, recv, "rd_duration", Value::Long(duration))?;
            Value::Null
        }
        "getFormat" => Value::Int(field_int(vm, recv, "rd_format")),
        "getWidth" => Value::Int(field_int(vm, recv, "rd_w")),
        "getHeight" => Value::Int(field_int(vm, recv, "rd_h")),
        "getSize" => new_size(vm, field_int(vm, recv, "rd_w"), field_int(vm, recv, "rd_h"))?,
        "getDuration" => match get(vm, recv, "rd_duration") {
            Value::Long(l) => Value::Long(l),
            Value::Int(i) => Value::Long(i as i64),
            _ => Value::Long(0),
        },
        "equals" => {
            let duration = |r: ObjRef| match get(vm, r, "rd_duration") {
                Value::Long(l) => l,
                Value::Int(i) => i as i64,
                _ => 0,
            };
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == STREAM_CONFIGURATION_DURATION
                        && ["rd_format", "rd_w", "rd_h"]
                            .iter()
                            .all(|f| field_int(vm, o, f) == field_int(vm, recv, f))
                        && duration(o) == duration(recv)
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => {
            let d = match get(vm, recv, "rd_duration") {
                Value::Long(l) => l,
                _ => 0,
            };
            Value::Int(hash_code_helpers_ints(&[
                field_int(vm, recv, "rd_format"),
                field_int(vm, recv, "rd_w"),
                field_int(vm, recv, "rd_h"),
                d as i32,
                ((d as u64) >> 32) as i32,
            ]))
        }
        _ => return Ok(None),
    }))
}
fn high_speed_video_configuration_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let fps_max = iarg(args, 3);
            if fps_max < 120 {
                return Err(iae("fpsMax must be at least 120"));
            }
            put(vm, recv, "rd_fps_max", Value::Int(fps_max))?;
            put(
                vm,
                recv,
                "rd_w",
                Value::Int(positive(iarg(args, 0), "width")?),
            )?;
            put(
                vm,
                recv,
                "rd_h",
                Value::Int(positive(iarg(args, 1), "height")?),
            )?;
            put(
                vm,
                recv,
                "rd_fps_min",
                Value::Int(positive(iarg(args, 2), "fpsMin")?),
            )?;
            put(
                vm,
                recv,
                "rd_batch_size_max",
                Value::Int(positive(iarg(args, 4), "batchSizeMax")?),
            )?;
            let size = new_size(vm, field_int(vm, recv, "rd_w"), field_int(vm, recv, "rd_h"))?;
            put(vm, recv, "rd_size", size)?;
            let range_r = alloc_obj(vm, "Landroid/util/Range;", Vec::new())?;
            let lo = crate::boxes::box_value(
                vm,
                "Ljava/lang/Integer;",
                Value::Int(field_int(vm, recv, "rd_fps_min")),
            )?;
            let hi = crate::boxes::box_value(vm, "Ljava/lang/Integer;", Value::Int(fps_max))?;
            put(vm, range_r, "rd_lower", Value::Obj(lo))?;
            put(vm, range_r, "rd_upper", Value::Obj(hi))?;
            put(vm, recv, "rd_fps_range", Value::Obj(range_r))?;
            Value::Null
        }
        "getWidth" => Value::Int(field_int(vm, recv, "rd_w")),
        "getHeight" => Value::Int(field_int(vm, recv, "rd_h")),
        "getFpsMin" => Value::Int(field_int(vm, recv, "rd_fps_min")),
        "getFpsMax" => Value::Int(field_int(vm, recv, "rd_fps_max")),
        "getBatchSizeMax" => Value::Int(field_int(vm, recv, "rd_batch_size_max")),
        "getSize" => {
            let s = get(vm, recv, "rd_size");
            if matches!(s, Value::Null) {
                new_size(vm, field_int(vm, recv, "rd_w"), field_int(vm, recv, "rd_h"))?
            } else {
                s
            }
        }
        "getFpsRange" => get(vm, recv, "rd_fps_range"),
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == HIGH_SPEED_VIDEO_CONFIGURATION
                        && [
                            "rd_w",
                            "rd_h",
                            "rd_fps_min",
                            "rd_fps_max",
                            "rd_batch_size_max",
                        ]
                        .iter()
                        .all(|f| field_int(vm, o, f) == field_int(vm, recv, f))
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(hash_code_helpers_ints(&[
            field_int(vm, recv, "rd_w"),
            field_int(vm, recv, "rd_h"),
            field_int(vm, recv, "rd_fps_min"),
            field_int(vm, recv, "rd_fps_max"),
        ])),
        _ => return Ok(None),
    }))
}
fn reprocess_formats_map_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let entry_of = |vm: &Engine, r: ObjRef| int_values(vm, Some(&get(vm, r, "rd_entry")));
    Ok(Some(match name {
        "<init>" => {
            if !array_is_obj(args.first()) {
                return Err(npe("entry must not be null"));
            }
            let entry = int_values(vm, args.first());
            let mut inputs = 0usize;
            let mut i = 0usize;
            let mut left = entry.len() as i32;
            while i < entry.len() {
                let input = check_argument_format_internal(entry[i])?;
                left -= 1;
                i += 1;
                if left < 1 {
                    return Err(iae(format!(
                        "Input {input:x} had no output format length listed"
                    )));
                }
                let length = entry[i];
                left -= 1;
                i += 1;
                for j in 0..length.max(0) {
                    // AOSP indexes first and validates; an overrun is the
                    // VM's guest array-index failure rather than the later
                    // descriptive IAE in the source.  Keep the descriptive
                    // IAE for the ordinary (reachable) too-few case.
                    if let Some(&out) = entry.get(i + j as usize) {
                        check_argument_format_internal(out)?;
                    }
                }
                if length > 0 {
                    if left < length {
                        return Err(iae(format!(
                            "Input {input:x} had too few output formats listed (actual: {left}, expected: {length})"
                        )));
                    }
                    i += length as usize;
                    left -= length;
                }
                inputs += 1;
            }
            // mEntry retains the caller's array reference.
            put(
                vm,
                recv,
                "rd_entry",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "rd_input_count", Value::Int(inputs as i32))?;
            Value::Null
        }
        "getInputs" => {
            let entry = entry_of(vm, recv);
            let mut inputs = Vec::new();
            let mut i = 0usize;
            while i < entry.len() {
                inputs.push(image_format_to_public(entry[i])?);
                let length = entry.get(i + 1).copied().unwrap_or(0).max(0) as usize;
                i += 2 + length;
            }
            new_int_array(vm, &inputs)?
        }
        "getOutputs" => {
            let format = iarg(args, 0);
            let entry = entry_of(vm, recv);
            let mut i = 0usize;
            let mut found = None;
            while i < entry.len() {
                let input = entry[i];
                let length = entry.get(i + 1).copied().unwrap_or(0);
                i += 2;
                if input == format {
                    let mut outputs = Vec::new();
                    for k in 0..length.max(0) as usize {
                        outputs.push(image_format_to_public(entry[i + k])?);
                    }
                    found = Some(outputs);
                    break;
                }
                i += length.max(0) as usize;
            }
            match found {
                Some(outputs) => new_int_array(vm, &outputs)?,
                None => {
                    return Err(iae(format!(
                        "Input format {format:x} was not one in #getInputs"
                    )))
                }
            }
        }
        "getInputCount" => Value::Int(field_int(vm, recv, "rd_input_count")),
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == REPROCESS_FORMATS_MAP
                        && entry_of(vm, o) == entry_of(vm, recv)
                }
                None => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(hash_code_helpers_ints(&entry_of(vm, recv))),
        _ => return Ok(None),
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StreamCfg {
    format: i32,
    width: i32,
    height: i32,
    input: bool,
}
#[derive(Clone, Copy)]
struct StreamDur {
    format: i32,
    width: i32,
    height: i32,
    duration: i64,
}
struct StreamGroup {
    configs: Vec<StreamCfg>,
    min_frame: Vec<StreamDur>,
    stall: Vec<StreamDur>,
}
const STREAM_GROUP_FIELDS: [(&str, &str, &str); 5] = [
    ("rd_cfg", "rd_min_frame", "rd_stall"),
    ("rd_depth_cfg", "rd_depth_min_frame", "rd_depth_stall"),
    ("rd_dynamic_cfg", "rd_dynamic_min_frame", "rd_dynamic_stall"),
    ("rd_heic_cfg", "rd_heic_min_frame", "rd_heic_stall"),
    ("rd_jpegr_cfg", "rd_jpegr_min_frame", "rd_jpegr_stall"),
];
fn stream_cfgs(vm: &Engine, v: &Value) -> Vec<StreamCfg> {
    array_elems(vm, Some(v))
        .iter()
        .filter_map(|x| match x {
            Value::Obj(r) => Some(StreamCfg {
                format: field_int(vm, *r, "rd_format"),
                width: field_int(vm, *r, "rd_w"),
                height: field_int(vm, *r, "rd_h"),
                input: field_bool(vm, *r, "rd_input"),
            }),
            _ => None,
        })
        .collect()
}
fn stream_durs(vm: &Engine, v: &Value) -> Vec<StreamDur> {
    array_elems(vm, Some(v))
        .iter()
        .filter_map(|x| match x {
            Value::Obj(r) => Some(StreamDur {
                format: field_int(vm, *r, "rd_format"),
                width: field_int(vm, *r, "rd_w"),
                height: field_int(vm, *r, "rd_h"),
                duration: field_long(vm, *r, "rd_duration"),
            }),
            _ => None,
        })
        .collect()
}
fn stream_group(vm: &Engine, recv: ObjRef, idx: usize) -> StreamGroup {
    let (cf, mf, sf) = STREAM_GROUP_FIELDS[idx];
    StreamGroup {
        configs: stream_cfgs(vm, &get(vm, recv, cf)),
        min_frame: stream_durs(vm, &get(vm, recv, mf)),
        stall: stream_durs(vm, &get(vm, recv, sf)),
    }
}
fn stream_group_index_for_dataspace(dataspace: i32) -> usize {
    match dataspace {
        DATASPACE_DEPTH => 1,
        DATASOURCE_DYNAMIC_DEPTH => 2,
        DATASOURCE_HEIF => 3,
        DATASOURCE_JPEG_R => 4,
        _ => 0,
    }
}
fn stream_min_duration(group: &StreamGroup, cfg: &StreamCfg) -> i64 {
    group
        .min_frame
        .iter()
        .find(|d| d.format == cfg.format && d.width == cfg.width && d.height == cfg.height)
        .map(|d| d.duration)
        .unwrap_or(0)
}
fn stream_is_high_res(vm: &Engine, recv: ObjRef, group: &StreamGroup, cfg: &StreamCfg) -> bool {
    field_bool(vm, recv, "rd_list_high_res") && stream_min_duration(group, cfg) > 50_000_000
}
fn stream_check_array(vm: &Engine, args: &[Value], idx: usize, name: &str) -> Result<(), VmExit> {
    match args.get(idx) {
        Some(Value::Obj(_)) => {
            if array_elems(vm, args.get(idx))
                .iter()
                .any(|v| matches!(v, Value::Null))
            {
                Err(npe(name))
            } else {
                Ok(())
            }
        }
        _ => Err(npe(name)),
    }
}
fn stream_map_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "imageFormatToPublic" if sig == "(I)I" => {
            Value::Int(image_format_to_public(iarg(args, 0))?)
        }
        "imageFormatToPublic" if sig == "([I)[I" => {
            let arr = match oarg(args, 0) {
                Some(a) => a,
                None => return Ok(Some(Value::Null)),
            };
            let xs = int_values(vm, args.first());
            let mut out = Vec::new();
            for x in xs {
                out.push(Value::Int(image_format_to_public(x)?));
            }
            *vm.heap.array_elems_mut(arr)? = out;
            Value::Obj(arr)
        }
        "depthFormatToPublic" => Value::Int(depth_format_to_public(iarg(args, 0))?),
        "imageFormatToInternal" if sig == "([I)[I" => {
            let arr = match oarg(args, 0) {
                Some(a) => a,
                None => return Ok(Some(Value::Null)),
            };
            let out: Vec<Value> = int_values(vm, args.first())
                .into_iter()
                .map(|x| Value::Int(image_format_to_internal(x)))
                .collect();
            *vm.heap.array_elems_mut(arr)? = out;
            Value::Obj(arr)
        }
        "isOutputSupportedFor" => {
            let class_obj = req_obj(args, 0, "klass must not be null")?;
            let desc = field_str(vm, class_obj, "class_name").unwrap_or_default();
            bool_value(matches!(
                desc.as_str(),
                "Landroid/media/ImageReader;"
                    | "Landroid/media/MediaRecorder;"
                    | "Landroid/media/MediaCodec;"
                    | "Landroid/renderscript/Allocation;"
                    | "Landroid/view/SurfaceHolder;"
                    | "Landroid/graphics/SurfaceTexture;"
            ))
        }
        _ => return Ok(None),
    }))
}
fn stream_sizes_value(vm: &mut Engine, sizes: Vec<(i32, i32)>) -> Result<Value, VmExit> {
    let mut values = Vec::new();
    for (w, h) in sizes {
        values.push(new_size(vm, w, h)?);
    }
    obj_array(vm, "[Landroid/util/Size;", values)
}
fn stream_supported_format(
    vm: &Engine,
    recv: ObjRef,
    format: i32,
    output: bool,
) -> Result<(), VmExit> {
    check_argument_format(format)?;
    let internal = image_format_to_internal(format);
    let dataspace = image_format_to_dataspace(format);
    let supported = if output {
        let group = stream_group(vm, recv, stream_group_index_for_dataspace(dataspace));
        group
            .configs
            .iter()
            .any(|c| c.format == internal && !c.input)
    } else {
        let group = stream_group(vm, recv, 0);
        group
            .configs
            .iter()
            .any(|c| c.format == internal && c.input)
    };
    if supported {
        Ok(())
    } else {
        Err(iae(format!(
            "format {format:x} is not supported by this stream configuration map"
        )))
    }
}
fn stream_public_sizes(
    vm: &mut Engine,
    recv: ObjRef,
    format: i32,
    output: bool,
    high_res: bool,
) -> Result<Value, VmExit> {
    if stream_supported_format(vm, recv, format, output).is_err() {
        return Ok(Value::Null);
    }
    let internal = image_format_to_internal(format);
    let dataspace = image_format_to_dataspace(format);
    stream_internal_sizes(vm, recv, internal, dataspace, output, high_res)
}
fn stream_internal_sizes(
    vm: &mut Engine,
    recv: ObjRef,
    format: i32,
    dataspace: i32,
    output: bool,
    high_res: bool,
) -> Result<Value, VmExit> {
    if dataspace == DATASPACE_DEPTH && high_res {
        return stream_sizes_value(vm, Vec::new());
    }
    let group_idx = stream_group_index_for_dataspace(dataspace);
    let group = stream_group(vm, recv, group_idx);
    let all: Vec<&StreamCfg> = group
        .configs
        .iter()
        .filter(|c| c.format == format && c.input != output)
        .collect();
    if all.is_empty() {
        return Ok(Value::Null);
    }
    // Keep the source's high-resolution split for color and for the
    // dynamic-depth/HEIF/JPEG-R groups.  Depth sizes are never high-res.
    let selected: Vec<(i32, i32)> = if output && field_bool(vm, recv, "rd_list_high_res") {
        all.iter()
            .filter(|c| {
                dataspace == DATASPACE_DEPTH || stream_is_high_res(vm, recv, &group, c) == high_res
            })
            .map(|c| (c.width, c.height))
            .collect()
    } else {
        all.iter().map(|c| (c.width, c.height)).collect()
    };
    stream_sizes_value(vm, selected)
}
fn stream_duration(
    vm: &Engine,
    recv: ObjRef,
    internal: i32,
    dataspace: i32,
    size: (i32, i32),
    stall: bool,
) -> Result<i64, VmExit> {
    let group = stream_group(vm, recv, stream_group_index_for_dataspace(dataspace));
    // Per the source, duration support checks format/size in the matching
    // configuration array without requiring that entry to be an output.
    if !group
        .configs
        .iter()
        .any(|c| c.format == internal && c.width == size.0 && c.height == size.1)
    {
        return Err(iae("size was not supported"));
    }
    let durations = if stall {
        &group.stall
    } else {
        &group.min_frame
    };
    Ok(durations
        .iter()
        .find(|d| d.format == internal && d.width == size.0 && d.height == size.1)
        .map(|d| d.duration)
        .unwrap_or(0))
}
fn stream_public_formats(vm: &mut Engine, recv: ObjRef, output: bool) -> Result<Value, VmExit> {
    let color = stream_group(vm, recv, 0);
    let mut internal: Vec<i32> = Vec::new();
    for cfg in &color.configs {
        if cfg.input != output && !internal.contains(&cfg.format) {
            internal.push(cfg.format);
        }
    }
    internal.sort_unstable(); // SparseIntArray key order.
    let mut formats = Vec::new();
    for format in internal {
        formats.push(image_format_to_public(format)?);
    }
    if output {
        let depth = stream_group(vm, recv, 1);
        let mut depth_formats: Vec<i32> = Vec::new();
        for cfg in &depth.configs {
            if !cfg.input && !depth_formats.contains(&cfg.format) {
                depth_formats.push(cfg.format);
            }
        }
        depth_formats.sort_unstable();
        for format in depth_formats {
            let public = depth_format_to_public(format)?;
            if !formats.contains(&public) {
                formats.push(public);
            }
        }
        for (idx, public) in [
            (2usize, IMAGE_DEPTH_JPEG),
            (3usize, IMAGE_HEIC),
            (4usize, IMAGE_JPEG_R),
        ] {
            let group = stream_group(vm, recv, idx);
            if group.configs.iter().any(|c| !c.input) && !formats.contains(&public) {
                formats.push(public);
            }
        }
    }
    new_int_array(vm, &formats)
}
fn stream_map_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            if !array_is_obj(args.first())
                && !array_is_obj(args.get(3))
                && !array_is_obj(args.get(9))
            {
                return Err(npe(
                    "At least one of color/depth/heic configurations must not be null",
                ));
            }
            let group_names = [
                ("configurations", "minFrameDurations", "stallDurations"),
                (
                    "depthConfigurations",
                    "depthMinFrameDurations",
                    "depthStallDurations",
                ),
                (
                    "dynamicDepthConfigurations",
                    "dynamicDepthMinFrameDurations",
                    "dynamicDepthStallDurations",
                ),
                (
                    "heicConfigurations",
                    "heicMinFrameDurations",
                    "heicStallDurations",
                ),
                (
                    "jpegRConfigurations",
                    "jpegRFrameDurations",
                    "jpegRStallDurations",
                ),
            ];
            for g in 0..5 {
                let base = g * 3;
                let (cf, mf, sf) = STREAM_GROUP_FIELDS[g];
                if array_is_obj(args.get(base)) {
                    stream_check_array(vm, args, base, group_names[g].0)?;
                    stream_check_array(vm, args, base + 1, group_names[g].1)?;
                    stream_check_array(vm, args, base + 2, group_names[g].2)?;
                    put(vm, recv, cf, args.get(base).cloned().unwrap_or(Value::Null))?;
                    put(
                        vm,
                        recv,
                        mf,
                        args.get(base + 1).cloned().unwrap_or(Value::Null),
                    )?;
                    put(
                        vm,
                        recv,
                        sf,
                        args.get(base + 2).cloned().unwrap_or(Value::Null),
                    )?;
                } else {
                    let empty = obj_array(
                        vm,
                        "[Landroid/hardware/camera2/params/StreamConfiguration;",
                        Vec::new(),
                    )?;
                    put(vm, recv, cf, empty)?;
                    let empty = obj_array(
                        vm,
                        "[Landroid/hardware/camera2/params/StreamConfigurationDuration;",
                        Vec::new(),
                    )?;
                    put(vm, recv, mf, empty)?;
                    let empty = obj_array(
                        vm,
                        "[Landroid/hardware/camera2/params/StreamConfigurationDuration;",
                        Vec::new(),
                    )?;
                    put(vm, recv, sf, empty)?;
                }
            }
            if array_is_obj(args.get(15)) {
                stream_check_array(vm, args, 15, "highSpeedVideoConfigurations")?;
                put(
                    vm,
                    recv,
                    "rd_high_speed",
                    args.get(15).cloned().unwrap_or(Value::Null),
                )?;
            } else {
                let empty = obj_array(
                    vm,
                    "[Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;",
                    Vec::new(),
                )?;
                put(vm, recv, "rd_high_speed", empty)?;
            }
            put(
                vm,
                recv,
                "rd_io_map",
                args.get(16).cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "rd_list_high_res", Value::Int(iarg(args, 17)))?;
            let enforce = if args.len() > 18 {
                iarg(args, 18) != 0
            } else {
                true
            };
            if array_is_obj(args.first()) && enforce {
                let group = stream_group(vm, recv, 0);
                let has_impl_defined = group.configs.iter().any(|c| {
                    c.format == HAL_FORMAT_IMPLEMENTATION_DEFINED
                        && !c.input
                        && !stream_is_high_res(vm, recv, &group, c)
                });
                if !has_impl_defined {
                    return Err(assertion(
                        "At least one stream configuration for IMPLEMENTATION_DEFINED must exist",
                    ));
                }
            }
            Value::Null
        }
        "getOutputFormats" => stream_public_formats(vm, recv, true)?,
        "getInputFormats" => stream_public_formats(vm, recv, false)?,
        "getValidOutputFormatsForInput" => {
            let io_map = get(vm, recv, "rd_io_map");
            let io_ref = match io_map {
                Value::Obj(r) => r,
                _ => return new_int_array(vm, &[]).map(Some),
            };
            let outputs_v = reprocess_formats_map_instance(
                vm,
                "getOutputs",
                "(I)[I",
                io_ref,
                &[Value::Int(iarg(args, 0))],
            )?
            .unwrap_or(Value::Null);
            let mut outputs = int_values(vm, Some(&outputs_v));
            let heic = stream_group(vm, recv, 3);
            if heic.configs.iter().any(|c| !c.input) {
                outputs.push(IMAGE_HEIC);
            }
            new_int_array(vm, &outputs)?
        }
        "getInputSizes" => stream_public_sizes(vm, recv, iarg(args, 0), false, false)?,
        "getOutputSizes" if sig == "(I)[Landroid/util/Size;" => {
            stream_public_sizes(vm, recv, iarg(args, 0), true, false)?
        }
        "getOutputSizes" if sig.contains("Ljava/lang/Class;") => {
            let supported =
                stream_map_static(vm, "isOutputSupportedFor", "(Ljava/lang/Class;)Z", args)?
                    .unwrap_or(Value::Int(0));
            if !matches!(supported, Value::Int(1)) {
                Value::Null
            } else {
                stream_internal_sizes(
                    vm,
                    recv,
                    HAL_FORMAT_IMPLEMENTATION_DEFINED,
                    DATASPACE_UNKNOWN,
                    true,
                    false,
                )?
            }
        }
        "getHighResolutionOutputSizes" => {
            if !field_bool(vm, recv, "rd_list_high_res") {
                Value::Null
            } else {
                stream_public_sizes(vm, recv, iarg(args, 0), true, true)?
            }
        }
        "isOutputSupportedFor" if sig == "(I)Z" => {
            let format = iarg(args, 0);
            check_argument_format(format)?;
            bool_value(stream_supported_format(vm, recv, format, true).is_ok())
        }
        "isOutputSupportedFor" if sig.contains("Landroid/util/Size;") => {
            let size = size_parts(vm, args.first()).unwrap_or((0, 0));
            let format = iarg(args, 1);
            let internal = image_format_to_internal(format);
            let group = stream_group(
                vm,
                recv,
                stream_group_index_for_dataspace(image_format_to_dataspace(format)),
            );
            bool_value(group.configs.iter().any(|c| {
                c.format == internal && !c.input && c.width == size.0 && c.height == size.1
            }))
        }
        "isOutputSupportedFor" if sig.contains("Landroid/view/Surface;") => {
            req_obj(args, 0, "surface must not be null")?;
            return Ok(None);
        }
        "getOutputMinFrameDuration" if sig.starts_with("(I") => {
            let format = iarg(args, 0);
            stream_supported_format(vm, recv, format, true)?;
            let size = size_parts(vm, args.get(1)).ok_or_else(|| npe("size"))?;
            Value::Long(stream_duration(
                vm,
                recv,
                image_format_to_internal(format),
                image_format_to_dataspace(format),
                size,
                false,
            )?)
        }
        "getOutputMinFrameDuration" if sig.contains("Ljava/lang/Class;") => {
            let supported = stream_map_static(
                vm,
                "isOutputSupportedFor",
                "(Ljava/lang/Class;)Z",
                &args[..1.min(args.len())],
            )?
            .unwrap_or(Value::Int(0));
            if !matches!(supported, Value::Int(1)) {
                return Err(iae("klass was not supported"));
            }
            let size = size_parts(vm, args.get(1)).ok_or_else(|| npe("size"))?;
            Value::Long(stream_duration(
                vm,
                recv,
                HAL_FORMAT_IMPLEMENTATION_DEFINED,
                DATASPACE_UNKNOWN,
                size,
                false,
            )?)
        }
        "getOutputStallDuration" if sig.starts_with("(I") => {
            let format = iarg(args, 0);
            stream_supported_format(vm, recv, format, true)?;
            let size = size_parts(vm, args.get(1)).ok_or_else(|| npe("size"))?;
            Value::Long(stream_duration(
                vm,
                recv,
                image_format_to_internal(format),
                image_format_to_dataspace(format),
                size,
                true,
            )?)
        }
        "getOutputStallDuration" if sig.contains("Ljava/lang/Class;") => {
            let supported = stream_map_static(
                vm,
                "isOutputSupportedFor",
                "(Ljava/lang/Class;)Z",
                &args[..1.min(args.len())],
            )?
            .unwrap_or(Value::Int(0));
            if !matches!(supported, Value::Int(1)) {
                return Err(iae("klass was not supported"));
            }
            let size = size_parts(vm, args.get(1)).ok_or_else(|| npe("size"))?;
            Value::Long(stream_duration(
                vm,
                recv,
                HAL_FORMAT_IMPLEMENTATION_DEFINED,
                DATASPACE_UNKNOWN,
                size,
                true,
            )?)
        }
        "getHighSpeedVideoSizes" => {
            let mut sizes: Vec<(i32, i32)> = Vec::new();
            for v in array_elems(vm, Some(&get(vm, recv, "rd_high_speed"))) {
                if let Value::Obj(r) = v {
                    let s = (field_int(vm, r, "rd_w"), field_int(vm, r, "rd_h"));
                    if !sizes.contains(&s) {
                        sizes.push(s);
                    }
                }
            }
            stream_sizes_value(vm, sizes)?
        }
        "getHighSpeedVideoFpsRangesFor" => {
            let size = size_parts(vm, args.first()).unwrap_or((0, 0));
            let mut ranges = Vec::new();
            for v in array_elems(vm, Some(&get(vm, recv, "rd_high_speed"))) {
                if let Value::Obj(r) = v {
                    if (field_int(vm, r, "rd_w"), field_int(vm, r, "rd_h")) == size {
                        ranges.push(get(vm, r, "rd_fps_range"));
                    }
                }
            }
            if ranges.is_empty() {
                return Err(iae(format!(
                    "Size {}x{} does not support high speed video recording",
                    size.0, size.1
                )));
            }
            obj_array(vm, "[Landroid/util/Range;", ranges)?
        }
        "getHighSpeedVideoFpsRanges" => {
            let mut ranges = Vec::new();
            let mut seen: Vec<(i32, i32)> = Vec::new();
            for v in array_elems(vm, Some(&get(vm, recv, "rd_high_speed"))) {
                if let Value::Obj(r) = v {
                    let key = (
                        field_int(vm, r, "rd_fps_min"),
                        field_int(vm, r, "rd_fps_max"),
                    );
                    if !seen.contains(&key) {
                        seen.push(key);
                        ranges.push(get(vm, r, "rd_fps_range"));
                    }
                }
            }
            obj_array(vm, "[Landroid/util/Range;", ranges)?
        }
        "getHighSpeedVideoSizesFor" => {
            let range = match oarg(args, 0) {
                Some(r) => r,
                None => {
                    return Err(iae(
                        "FpsRange null does not support high speed video recording",
                    ))
                }
            };
            let lo = boxed_int_value(vm, &get(vm, range, "rd_lower"));
            let hi = boxed_int_value(vm, &get(vm, range, "rd_upper"));
            let mut sizes = Vec::new();
            for v in array_elems(vm, Some(&get(vm, recv, "rd_high_speed"))) {
                if let Value::Obj(r) = v {
                    if field_int(vm, r, "rd_fps_min") == lo && field_int(vm, r, "rd_fps_max") == hi
                    {
                        sizes.push((field_int(vm, r, "rd_w"), field_int(vm, r, "rd_h")));
                    }
                }
            }
            if sizes.is_empty() {
                return Err(iae(format!(
                    "FpsRange [{lo}, {hi}] does not support high speed video recording"
                )));
            }
            stream_sizes_value(vm, sizes)?
        }
        "equals" => {
            // AOSP does not override equals: identity.
            bool_value(matches!(args.first(), Some(Value::Obj(o)) if *o == recv))
        }
        "hashCode" => Value::Int(identity_hash(recv)),
        "toString" => {
            // AOSP emits a long diagnostic string beginning with this prefix.
            // The full per-configuration formatting is outside the tested
            // slice, so do not fabricate a lookalike diagnostic.
            return Ok(None);
        }
        _ => return Ok(None),
    }))
}
fn boxed_int_value(vm: &Engine, v: &Value) -> i32 {
    match v {
        Value::Int(i) => *i,
        Value::Obj(r) => match get(vm, *r, "value") {
            Value::Int(i) => i,
            _ => 0,
        },
        _ => 0,
    }
}

fn text_no_op(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (TEXT_CLASSIFIER.to_string(), "NO_OP".to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let r = alloc_obj(vm, TEXT_CLASSIFIER, Vec::new())?;
    put(vm, r, "rd_noop", Value::Int(1))?;
    let v = Value::Obj(r);
    vm.statics.insert(key, v.clone());
    Ok(v)
}
fn text_classifier_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    text_classifier_default(vm, name, sig, args)
}

fn class_descriptor_of(vm: &Engine, v: &Value) -> Option<String> {
    match v {
        Value::Obj(r) => field_str(vm, *r, "class_name"),
        _ => None,
    }
}
fn type_hash_of(vm: &Engine, v: &Value) -> i32 {
    match v {
        // Class.hashCode is its identity hash in this VM.
        Value::Obj(r) => *r as i32,
        _ => {
            let _ = vm;
            0
        }
    }
}
fn make_type_reference(vm: &mut Engine, type_value: Value) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, TYPE_REFERENCE, Vec::new())?;
    put(vm, r, "rd_type", type_value.clone())?;
    // For a Class type, the raw type and the type itself are both that Class.
    // Generic/ParameterizedType objects are outside this slice; storing the
    // supplied Type preserves explicit guest-provided equality data.
    put(vm, r, "rd_raw_type", type_value.clone())?;
    if let Some(desc) = class_descriptor_of(vm, &type_value) {
        let d = jstr(vm, desc)?;
        put(vm, r, "rd_type_desc", d)?;
    }
    put(vm, r, "rd_hash", Value::Int(type_hash_of(vm, &type_value)))?;
    Ok(Value::Obj(r))
}
fn type_reference_hash(vm: &Engine, r: ObjRef) -> i32 {
    let stored = field_int(vm, r, "rd_hash");
    if stored != 0 {
        stored
    } else {
        type_hash_of(vm, &get(vm, r, "rd_type"))
    }
}
fn type_references_equal(vm: &Engine, a: &Value, b: &Value) -> bool {
    let (Some(ar), Some(br)) = (obj(Some(a)), obj(Some(b))) else {
        return false;
    };
    if ar == br {
        return true;
    }
    let at = get(vm, ar, "rd_type");
    let bt = get(vm, br, "rd_type");
    match (class_descriptor_of(vm, &at), class_descriptor_of(vm, &bt)) {
        (Some(x), Some(y)) => x == y,
        _ => at == bt,
    }
}
fn type_reference_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "createSpecializedTypeReference" => {
            // The private Type constructor calls containsTypeVariable(type),
            // whose checkNotNull rejects a null type before anything else.
            let type_value = args.first().cloned().unwrap_or(Value::Null);
            if matches!(type_value, Value::Null) {
                return Err(npe("type must not be null"));
            }
            make_type_reference(vm, type_value)?
        }
        _ => return Ok(None),
    }))
}
fn type_reference_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            // Anonymous-subclass generic-superclass capture needs reflection
            // signature data that this VM slice does not model.
            return Ok(None);
        }
        "getType" => get(vm, recv, "rd_type"),
        "getRawType" => get(vm, recv, "rd_raw_type"),
        "getComponentType" => {
            let desc = field_str(vm, recv, "rd_type_desc").unwrap_or_default();
            if let Some(component) = desc.strip_prefix('[') {
                let class = vm.class_object_for(component)?;
                make_type_reference(vm, Value::Obj(class))?
            } else {
                Value::Null
            }
        }
        "equals" => bool_value(match oarg(args, 0) {
            Some(o) => {
                (vm.cp.is_subtype(&class_of(vm, o), TYPE_REFERENCE)
                    || class_of(vm, o) == TYPE_REFERENCE)
                    && type_references_equal(vm, &Value::Obj(recv), &Value::Obj(o))
            }
            None => false,
        }),
        "hashCode" => Value::Int(type_reference_hash(vm, recv)),
        _ => return Ok(None),
    }))
}
fn make_native_key(
    vm: &mut Engine,
    name: Value,
    fallback: Value,
    type_value: Value,
    type_ref: Value,
    vendor_id: i64,
) -> Result<Value, VmExit> {
    let name_s = opt_str(vm, Some(&name)).ok_or_else(|| npe("Key needs a valid name"))?;
    if matches!(type_value, Value::Null) && matches!(type_ref, Value::Null) {
        return Err(npe("Type needs to be non-null"));
    }
    let type_ref = if matches!(type_ref, Value::Null) {
        make_type_reference(vm, type_value.clone())?
    } else {
        type_ref
    };
    let type_value = if matches!(type_value, Value::Null) {
        match &type_ref {
            Value::Obj(r) => get(vm, *r, "rd_raw_type"),
            _ => Value::Null,
        }
    } else {
        type_value
    };
    let th = match &type_ref {
        Value::Obj(r) => type_reference_hash(vm, *r),
        _ => 0,
    };
    let r = alloc_obj(vm, CAMERA_METADATA_NATIVE_KEY, Vec::new())?;
    put(vm, r, "rd_name", name)?;
    put(vm, r, "rd_fallback", fallback)?;
    put(vm, r, "rd_type", type_value)?;
    put(vm, r, "rd_type_ref", type_ref)?;
    put(vm, r, "rd_vendor_id", Value::Long(vendor_id))?;
    put(vm, r, "rd_hash", Value::Int(java_string_hash(&name_s) ^ th))?;
    Ok(Value::Obj(r))
}
fn native_key_of(vm: &Engine, v: &Value) -> Option<ObjRef> {
    let r = obj(Some(v))?;
    let class = class_of(vm, r);
    if class == CAMERA_METADATA_NATIVE_KEY {
        Some(r)
    } else if matches!(
        class.as_str(),
        CAMERA_CHARACTERISTICS_KEY | CAPTURE_REQUEST_KEY | CAPTURE_RESULT_KEY
    ) {
        field_obj(vm, r, "rd_native_key")
    } else {
        None
    }
}
fn native_key_name(vm: &Engine, native: ObjRef) -> String {
    field_str(vm, native, "rd_name").unwrap_or_default()
}
fn native_key_hash(vm: &Engine, native: ObjRef) -> i32 {
    let stored = field_int(vm, native, "rd_hash");
    if stored != 0 {
        stored
    } else {
        let th = match get(vm, native, "rd_type_ref") {
            Value::Obj(r) => type_reference_hash(vm, r),
            _ => 0,
        };
        java_string_hash(&native_key_name(vm, native)) ^ th
    }
}
fn native_keys_equal(vm: &Engine, a: ObjRef, b: ObjRef) -> bool {
    native_key_name(vm, a) == native_key_name(vm, b)
        && type_references_equal(vm, &get(vm, a, "rd_type_ref"), &get(vm, b, "rd_type_ref"))
}
fn init_native_key(vm: &mut Engine, recv: ObjRef, sig: &str, args: &[Value]) -> Result<(), VmExit> {
    let made = if sig.contains("Ljava/lang/String;Ljava/lang/Class;J") {
        let vendor = match args.get(2) {
            Some(Value::Long(l)) => *l,
            Some(Value::Int(i)) => *i as i64,
            _ => i64::MAX,
        };
        make_native_key(
            vm,
            args.first().cloned().unwrap_or(Value::Null),
            Value::Null,
            args.get(1).cloned().unwrap_or(Value::Null),
            Value::Null,
            vendor,
        )?
    } else if sig.contains("Ljava/lang/String;Ljava/lang/String;Ljava/lang/Class;") {
        make_native_key(
            vm,
            args.first().cloned().unwrap_or(Value::Null),
            args.get(1).cloned().unwrap_or(Value::Null),
            args.get(2).cloned().unwrap_or(Value::Null),
            Value::Null,
            i64::MAX,
        )?
    } else if sig.contains(TYPE_REFERENCE) {
        if matches!(args.get(1), None | Some(Value::Null)) {
            // Checked in make_native_key by type nulls, but keep AOSP's
            // specific message for this overload.
            if !args.is_empty() && !matches!(args.first(), Some(Value::Null)) {
                return Err(npe("TypeReference needs to be non-null"));
            }
        }
        make_native_key(
            vm,
            args.first().cloned().unwrap_or(Value::Null),
            Value::Null,
            Value::Null,
            args.get(1).cloned().unwrap_or(Value::Null),
            i64::MAX,
        )?
    } else {
        make_native_key(
            vm,
            args.first().cloned().unwrap_or(Value::Null),
            Value::Null,
            args.get(1).cloned().unwrap_or(Value::Null),
            Value::Null,
            i64::MAX,
        )?
    };
    let source = obj(Some(&made)).unwrap_or(recv);
    for f in [
        "rd_name",
        "rd_fallback",
        "rd_type",
        "rd_type_ref",
        "rd_vendor_id",
        "rd_hash",
    ] {
        put(vm, recv, f, get(vm, source, f))?;
    }
    Ok(())
}
fn native_key_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            init_native_key(vm, recv, sig, args)?;
            Value::Null
        }
        "getName" => get(vm, recv, "rd_name"),
        "hashCode" => Value::Int(native_key_hash(vm, recv)),
        "equals" => {
            let eq = match args.first().and_then(|v| native_key_of(vm, v)) {
                Some(other) => {
                    // AOSP first compares hashCode values.  Wrapper hashes
                    // delegate to their native key, so this is equivalent.
                    native_key_hash(vm, recv) == native_key_hash(vm, other)
                        && native_keys_equal(vm, recv, other)
                }
                None => false,
            };
            bool_value(eq)
        }
        "getTag" => {
            if field_bool(vm, recv, "rd_has_tag") {
                Value::Int(field_int(vm, recv, "rd_tag"))
            } else {
                // No native camera-metadata tag table exists in this
                // profile; do not invent a numeric tag.
                return Ok(None);
            }
        }
        "hasTag" => bool_value(field_bool(vm, recv, "rd_has_tag")),
        "cacheTag" => {
            put(vm, recv, "rd_has_tag", Value::Int(1))?;
            put(vm, recv, "rd_tag", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        "getType" => get(vm, recv, "rd_type"),
        "getVendorId" => Value::Long(match get(vm, recv, "rd_vendor_id") {
            Value::Long(l) => l,
            Value::Int(i) => i as i64,
            _ => i64::MAX,
        }),
        "getTypeReference" => get(vm, recv, "rd_type_ref"),
        _ => return Ok(None),
    }))
}
fn wrapper_key_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let native = if sig.contains(CAMERA_METADATA_NATIVE_KEY) {
                args.first().cloned().unwrap_or(Value::Null)
            } else {
                // Build a temporary native key through the same validation
                // path, then adopt it as this wrapper's mKey.
                let temp = alloc_obj(vm, CAMERA_METADATA_NATIVE_KEY, Vec::new())?;
                init_native_key(vm, temp, sig, args)?;
                Value::Obj(temp)
            };
            put(vm, recv, "rd_native_key", native)?;
            Value::Null
        }
        "getName" => match native_key_of(vm, &Value::Obj(recv)) {
            Some(n) => get(vm, n, "rd_name"),
            None => Value::Null,
        },
        "getVendorId" => match native_key_of(vm, &Value::Obj(recv)) {
            Some(n) => Value::Long(match get(vm, n, "rd_vendor_id") {
                Value::Long(l) => l,
                _ => i64::MAX,
            }),
            None => Value::Long(i64::MAX),
        },
        "hashCode" => Value::Int(match native_key_of(vm, &Value::Obj(recv)) {
            Some(n) => native_key_hash(vm, n),
            None => 0,
        }),
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) => {
                    class_of(vm, o) == class
                        && match (
                            native_key_of(vm, &Value::Obj(recv)),
                            native_key_of(vm, &Value::Obj(o)),
                        ) {
                            (Some(a), Some(b)) => native_keys_equal(vm, a, b),
                            _ => false,
                        }
                }
                None => false,
            };
            bool_value(eq)
        }
        "toString" => {
            let prefix = match class {
                CAMERA_CHARACTERISTICS_KEY => "CameraCharacteristics.Key",
                CAPTURE_REQUEST_KEY => "CaptureRequest.Key",
                _ => "CaptureResult.Key",
            };
            let n = match native_key_of(vm, &Value::Obj(recv)) {
                Some(n) => native_key_name(vm, n),
                None => "null".to_string(),
            };
            jstr(vm, format!("{prefix}({n})"))?
        }
        "getNativeKey" => get(vm, recv, "rd_native_key"),
        _ => return Ok(None),
    }))
}
fn wrap_native_key(vm: &mut Engine, wrapper_class: &str, native: ObjRef) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, wrapper_class, Vec::new())?;
    put(vm, r, "rd_native_key", Value::Obj(native))?;
    Ok(Value::Obj(r))
}
fn metadata_entries(vm: &Engine, metadata: ObjRef) -> Vec<(Value, Value)> {
    let keys = collection_items(vm, Some(&get(vm, metadata, "rd_keys")));
    let values = collection_items(vm, Some(&get(vm, metadata, "rd_values")));
    keys.into_iter()
        .zip(values.into_iter().chain(std::iter::repeat(Value::Null)))
        .collect()
}
fn metadata_set_entries(
    vm: &mut Engine,
    metadata: ObjRef,
    entries: Vec<(Value, Value)>,
) -> Result<(), VmExit> {
    let (keys, values): (Vec<Value>, Vec<Value>) = entries.into_iter().unzip();
    let key_list = new_list(vm, keys)?;
    let value_list = new_list(vm, values)?;
    put(vm, metadata, "rd_keys", Value::Obj(key_list))?;
    put(vm, metadata, "rd_values", Value::Obj(value_list))
}
fn metadata_get(vm: &Engine, metadata: ObjRef, key: &Value) -> Value {
    let Some(wanted) = native_key_of(vm, key) else {
        return Value::Null;
    };
    let entries = metadata_entries(vm, metadata);
    for (k, v) in &entries {
        if let Some(n) = native_key_of(vm, k) {
            if native_keys_equal(vm, n, wanted) {
                return v.clone();
            }
        }
    }
    // Fallback names let a renamed key read data written under its old name.
    if let Some(fallback) = field_str(vm, wanted, "rd_fallback") {
        for (k, v) in &entries {
            if let Some(n) = native_key_of(vm, k) {
                if native_key_name(vm, n) == fallback
                    && type_references_equal(
                        vm,
                        &get(vm, n, "rd_type_ref"),
                        &get(vm, wanted, "rd_type_ref"),
                    )
                {
                    return v.clone();
                }
            }
        }
    }
    Value::Null
}
fn metadata_set(vm: &mut Engine, metadata: ObjRef, key: Value, value: Value) -> Result<(), VmExit> {
    let Some(wanted) = native_key_of(vm, &key) else {
        return Err(npe("key must not be null"));
    };
    let mut entries = metadata_entries(vm, metadata);
    entries.retain(|(k, _)| match native_key_of(vm, k) {
        Some(n) => !native_keys_equal(vm, n, wanted),
        None => true,
    });
    // set(key, null) deletes the tag in native metadata.
    if !matches!(value, Value::Null) {
        entries.push((Value::Obj(wanted), value));
    }
    metadata_set_entries(vm, metadata, entries)
}
fn metadata_copy_to(vm: &mut Engine, source: ObjRef, dest: ObjRef) -> Result<(), VmExit> {
    let entries = metadata_entries(vm, source);
    metadata_set_entries(vm, dest, entries)
}
fn new_metadata_native(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = alloc_obj(vm, CAMERA_METADATA_NATIVE, Vec::new())?;
    metadata_set_entries(vm, r, Vec::new())?;
    Ok(r)
}
fn metadata_move(vm: &mut Engine, source: ObjRef) -> Result<ObjRef, VmExit> {
    let dest = new_metadata_native(vm)?;
    metadata_copy_to(vm, source, dest)?;
    metadata_set_entries(vm, source, Vec::new())?;
    Ok(dest)
}
fn metadata_contents_equal(vm: &Engine, a: ObjRef, b: ObjRef) -> bool {
    if a == b {
        return true;
    }
    let ae = metadata_entries(vm, a);
    ae.len() == metadata_entries(vm, b).len()
        && ae.iter().all(|(k, v)| {
            // Stored metadata values are never Null (set(key, null) deletes),
            // so Null from metadata_get unambiguously means "absent" here.
            native_key_of(vm, k).is_some() && metadata_get(vm, b, k) == *v
        })
}
fn metadata_content_hash(vm: &mut Engine, metadata: ObjRef) -> Result<i32, VmExit> {
    // Order-independent Map-style hash over the explicitly-populated host
    // entries.  This is not AOSP's native serialized-buffer hash; it exists
    // to keep this module's content-equality model internally consistent.
    let mut h = 0i32;
    for (k, v) in metadata_entries(vm, metadata) {
        let kh = match native_key_of(vm, &k) {
            Some(n) => native_key_hash(vm, n),
            None => object_hash_code(vm, &k)?,
        };
        let vh = if matches!(v, Value::Null) {
            0
        } else {
            object_hash_code(vm, &v)?
        };
        h = h.wrapping_add(kh ^ vh);
    }
    Ok(h)
}
fn capture_request_physical_entries(vm: &Engine, recv: ObjRef) -> Vec<(Value, Option<ObjRef>)> {
    let ids = collection_items(vm, Some(&get(vm, recv, "rd_physical_ids")));
    let settings = collection_items(vm, Some(&get(vm, recv, "rd_physical_settings")));
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| {
            let setting = settings.get(i).and_then(|v| match v {
                Value::Obj(r) => Some(*r),
                _ => None,
            });
            (id, setting)
        })
        .collect()
}
fn capture_request_physical_equal(vm: &Engine, a: ObjRef, b: ObjRef) -> bool {
    let ae = capture_request_physical_entries(vm, a);
    let be = capture_request_physical_entries(vm, b);
    ae.len() == be.len()
        && ae.iter().all(|(id, setting)| {
            let id_s = opt_str(vm, Some(id));
            match be.iter().find(|(bid, _)| opt_str(vm, Some(bid)) == id_s) {
                Some((_, other)) => match (*setting, *other) {
                    (Some(x), Some(y)) => metadata_contents_equal(vm, x, y),
                    (None, None) => true,
                    _ => false,
                },
                None => false,
            }
        })
}
fn capture_request_hash(vm: &mut Engine, recv: ObjRef) -> Result<i32, VmExit> {
    // AOSP: HashCodeHelpers.hashCodeGeneric(mPhysicalCameraSettings,
    // mSurfaceSet, mUserTag), using Java Map/Set hash definitions.
    let mut physical_hash = 0i32;
    for (id, setting) in capture_request_physical_entries(vm, recv) {
        let vh = match setting {
            Some(m) => metadata_content_hash(vm, m)?,
            None => 0,
        };
        physical_hash = physical_hash.wrapping_add(host_hash(vm, &id) ^ vh);
    }
    let mut surface_hash = 0i32;
    for surface in collection_items(vm, Some(&get(vm, recv, "rd_surfaces"))) {
        let h = if matches!(surface, Value::Null) {
            0
        } else {
            object_hash_code(vm, &surface)?
        };
        surface_hash = surface_hash.wrapping_add(h);
    }
    let tag = get(vm, recv, "rd_tag");
    let tag_hash = if matches!(tag, Value::Null) {
        0
    } else {
        object_hash_code(vm, &tag)?
    };
    Ok(hash_code_helpers_ints(&[
        physical_hash,
        surface_hash,
        tag_hash,
    ]))
}
fn camera_metadata_native_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            metadata_set_entries(vm, recv, Vec::new())?;
            if let Some(other) = oarg(args, 0) {
                metadata_copy_to(vm, other, recv)?;
            } else if !args.is_empty() {
                return Err(npe("other"));
            }
            Value::Null
        }
        "get" | "getBase" => {
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            metadata_get(vm, recv, &key)
        }
        "set" | "setBase" => {
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            metadata_set(vm, recv, key, args.get(1).cloned().unwrap_or(Value::Null))?;
            Value::Null
        }
        "isEmpty" => bool_value(metadata_entries(vm, recv).is_empty()),
        "getEntryCount" => Value::Int(metadata_entries(vm, recv).len() as i32),
        "swap" => {
            let other = req_obj(args, 0, "other")?;
            let mine = metadata_entries(vm, recv);
            let theirs = metadata_entries(vm, other);
            metadata_set_entries(vm, recv, theirs)?;
            metadata_set_entries(vm, other, mine)?;
            Value::Null
        }
        "describeContents" => Value::Int(0),
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) if class_of(vm, o) == CAMERA_METADATA_NATIVE => {
                    metadata_contents_equal(vm, recv, o)
                }
                _ => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(metadata_content_hash(vm, recv)?),
        _ => return Ok(None),
    }))
}
fn camera_metadata_native_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "move" => {
            let other = req_obj(args, 0, "other")?;
            Value::Obj(metadata_move(vm, other)?)
        }
        "update" => {
            let dst = req_obj(args, 0, "dst")?;
            let src = req_obj(args, 1, "src")?;
            for (k, v) in metadata_entries(vm, src) {
                metadata_set(vm, dst, k, v)?;
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}
fn metadata_keys_as(
    vm: &mut Engine,
    metadata: Option<ObjRef>,
    wrapper_class: &str,
) -> Result<Value, VmExit> {
    let mut keys = Vec::new();
    if let Some(m) = metadata {
        for (k, _) in metadata_entries(vm, m) {
            if let Some(n) = native_key_of(vm, &k) {
                keys.push(wrap_native_key(vm, wrapper_class, n)?);
            }
        }
    }
    Ok(Value::Obj(new_list(vm, keys)?))
}
fn hash_code_helpers_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "hashCode" if sig == "([I)I" => {
            if !array_is_obj(args.first()) {
                Value::Int(0)
            } else {
                Value::Int(hash_code_helpers_ints(&int_values(vm, args.first())))
            }
        }
        "hashCode" if sig == "([F)I" => {
            if !array_is_obj(args.first()) {
                Value::Int(0)
            } else {
                let xs: Vec<i32> = float_values(vm, args.first())
                    .iter()
                    .map(|f| f.to_bits() as i32)
                    .collect();
                Value::Int(hash_code_helpers_ints(&xs))
            }
        }
        "hashCodeGeneric" => {
            if !array_is_obj(args.first()) {
                Value::Int(0)
            } else {
                let mut hashes = Vec::new();
                for v in array_elems(vm, args.first()) {
                    hashes.push(if matches!(v, Value::Null) {
                        0
                    } else {
                        object_hash_code(vm, &v)?
                    });
                }
                Value::Int(hash_code_helpers_ints(&hashes))
            }
        }
        _ => return Ok(None),
    }))
}
fn object_hash_code(vm: &mut Engine, v: &Value) -> Result<i32, VmExit> {
    match v {
        Value::Obj(r) => {
            let result = vm.invoke_instance(*r, "hashCode", "()I", &[])?;
            Ok(match result {
                Value::Int(i) => i,
                _ => host_hash(vm, v),
            })
        }
        _ => Ok(host_hash(vm, v)),
    }
}

fn camera_characteristics_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let properties = req_obj(args, 0, "properties")?;
            let moved = metadata_move(vm, properties)?;
            put(vm, recv, "rd_properties", Value::Obj(moved))?;
            Value::Null
        }
        "get" => {
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            match field_obj(vm, recv, "rd_properties") {
                Some(m) => metadata_get(vm, m, &key),
                None => Value::Null,
            }
        }
        "getNativeCopy" => {
            let copy = new_metadata_native(vm)?;
            if let Some(m) = field_obj(vm, recv, "rd_properties") {
                metadata_copy_to(vm, m, copy)?;
            }
            Value::Obj(copy)
        }
        "getKeys" => metadata_keys_as(
            vm,
            field_obj(vm, recv, "rd_properties"),
            CAMERA_CHARACTERISTICS_KEY,
        )?,
        "getKeysNeedingPermission" => Value::Obj(new_list(vm, Vec::new())?),
        "getAvailableCaptureRequestKeys"
        | "getAvailableSessionKeys"
        | "getAvailablePhysicalCameraRequestKeys" => metadata_keys_as(
            vm,
            field_obj(vm, recv, "rd_properties"),
            CAPTURE_REQUEST_KEY,
        )?,
        "getAvailableCaptureResultKeys" => {
            metadata_keys_as(vm, field_obj(vm, recv, "rd_properties"), CAPTURE_RESULT_KEY)?
        }
        "getPhysicalCameraIds" => {
            // No logical multi-camera composition is described by an
            // explicitly-populated characteristics object in this slice.
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", Vec::new())?)
        }
        _ => return Ok(None),
    }))
}
fn capture_request_init_empty(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    let surfaces = new_list(vm, Vec::new())?;
    put(vm, recv, "rd_surfaces", Value::Obj(surfaces))?;
    put(vm, recv, "rd_reprocess", Value::Int(0))?;
    put(vm, recv, "rd_session_id", Value::Int(-1))?;
    put(vm, recv, "rd_request_type", Value::Int(-1))
}
fn capture_request_settings(vm: &Engine, recv: ObjRef) -> Option<ObjRef> {
    field_obj(vm, recv, "rd_settings")
}
fn capture_request_copy_into(vm: &mut Engine, source: ObjRef, dest: ObjRef) -> Result<(), VmExit> {
    capture_request_init_empty(vm, dest)?;
    put(vm, dest, "rd_logical_id", get(vm, source, "rd_logical_id"))?;
    // Copy logical settings.
    if let Some(settings) = capture_request_settings(vm, source) {
        let copy = new_metadata_native(vm)?;
        metadata_copy_to(vm, settings, copy)?;
        put(vm, dest, "rd_settings", Value::Obj(copy))?;
    }
    // Copy parallel physical settings.
    let ids = collection_items(vm, Some(&get(vm, source, "rd_physical_ids")));
    let settings = collection_items(vm, Some(&get(vm, source, "rd_physical_settings")));
    let id_list = new_list(vm, ids)?;
    put(vm, dest, "rd_physical_ids", Value::Obj(id_list))?;
    let mut copied_settings = Vec::new();
    for s in settings {
        if let Value::Obj(m) = s {
            let copy = new_metadata_native(vm)?;
            metadata_copy_to(vm, m, copy)?;
            copied_settings.push(Value::Obj(copy));
        }
    }
    let settings_list = new_list(vm, copied_settings)?;
    put(vm, dest, "rd_physical_settings", Value::Obj(settings_list))?;
    let surfaces = new_list(
        vm,
        collection_items(vm, Some(&get(vm, source, "rd_surfaces"))),
    )?;
    put(vm, dest, "rd_surfaces", Value::Obj(surfaces))?;
    put(vm, dest, "rd_reprocess", get(vm, source, "rd_reprocess"))?;
    put(vm, dest, "rd_session_id", get(vm, source, "rd_session_id"))?;
    put(vm, dest, "rd_part_chs", get(vm, source, "rd_part_chs"))?;
    put(vm, dest, "rd_tag", get(vm, source, "rd_tag"))?;
    put(vm, dest, "rd_request_type", Value::Int(-1))?;
    Ok(())
}
fn capture_request_init_from_template(
    vm: &mut Engine,
    recv: ObjRef,
    args: &[Value],
) -> Result<(), VmExit> {
    capture_request_init_empty(vm, recv)?;
    let template = req_obj(args, 0, "settings")?;
    let is_reprocess = iarg(args, 1) != 0;
    let session_id = iarg(args, 2);
    let logical_id = args.get(3).cloned().unwrap_or(Value::Null);
    let physical_set = args.get(4).cloned().unwrap_or(Value::Null);
    if !matches!(physical_set, Value::Null) && is_reprocess {
        return Err(iae(
            "Create a reprocess capture request with with more than one physical camera is not supported!",
        ));
    }
    let moved = metadata_move(vm, template)?;
    put(vm, recv, "rd_logical_id", logical_id.clone())?;
    put(vm, recv, "rd_settings", Value::Obj(moved))?;
    let mut ids = vec![logical_id];
    let mut settings = vec![Value::Obj(moved)];
    if !matches!(physical_set, Value::Null) {
        for id in collection_items(vm, Some(&physical_set)) {
            // The logical ID is already present as the map's first key.
            if opt_str(vm, Some(&id)) != opt_str(vm, ids.first()) {
                let copy = new_metadata_native(vm)?;
                metadata_copy_to(vm, moved, copy)?;
                ids.push(id);
                settings.push(Value::Obj(copy));
            }
        }
    }
    let id_list = new_list(vm, ids)?;
    let settings_list = new_list(vm, settings)?;
    put(vm, recv, "rd_physical_ids", Value::Obj(id_list))?;
    put(vm, recv, "rd_physical_settings", Value::Obj(settings_list))?;
    put(vm, recv, "rd_reprocess", Value::Int(is_reprocess as i32))?;
    if is_reprocess {
        if session_id == -1 {
            return Err(iae(format!(
                "Create a reprocess capture request with an invalid session ID: {session_id}"
            )));
        }
        put(vm, recv, "rd_session_id", Value::Int(session_id))?;
    } else {
        put(vm, recv, "rd_session_id", Value::Int(-1))?;
    }
    Ok(())
}
fn capture_request_physical_settings(
    vm: &Engine,
    recv: ObjRef,
    physical_id: &Value,
) -> Result<ObjRef, VmExit> {
    let wanted = opt_str(vm, Some(physical_id));
    let ids = collection_items(vm, Some(&get(vm, recv, "rd_physical_ids")));
    let settings = collection_items(vm, Some(&get(vm, recv, "rd_physical_settings")));
    for (id, setting) in ids.iter().zip(settings.iter()) {
        if opt_str(vm, Some(id)) == wanted {
            if let Value::Obj(m) = setting {
                return Ok(*m);
            }
        }
    }
    Err(iae(format!(
        "Physical camera id: {} is not valid!",
        wanted.as_deref().unwrap_or("null")
    )))
}
fn capture_request_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            if sig == "()V" {
                capture_request_init_empty(vm, recv)?;
            } else if sig.contains("Landroid/hardware/camera2/CaptureRequest;") {
                let source = req_obj(args, 0, "source")?;
                capture_request_copy_into(vm, source, recv)?;
            } else {
                capture_request_init_from_template(vm, recv, args)?;
            }
            Value::Null
        }
        "get" => {
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            match capture_request_settings(vm, recv) {
                Some(m) => metadata_get(vm, m, &key),
                None => Value::Null,
            }
        }
        "getKeys" => metadata_keys_as(vm, capture_request_settings(vm, recv), CAPTURE_REQUEST_KEY)?,
        "getTag" => get(vm, recv, "rd_tag"),
        "isReprocess" => bool_value(field_bool(vm, recv, "rd_reprocess")),
        "isPartOfCRequestList" => bool_value(field_bool(vm, recv, "rd_part_chs")),
        "getNativeCopy" => {
            let copy = new_metadata_native(vm)?;
            if let Some(m) = capture_request_settings(vm, recv) {
                metadata_copy_to(vm, m, copy)?;
            }
            Value::Obj(copy)
        }
        "getReprocessableSessionId" => {
            let session = field_int(vm, recv, "rd_session_id");
            if !field_bool(vm, recv, "rd_reprocess") || session == -1 {
                return Err(ise(
                    "Getting the reprocessable session ID for a non-reprocess capture request is illegal.",
                ));
            }
            Value::Int(session)
        }
        "containsTarget" => {
            let target = args.first().cloned().unwrap_or(Value::Null);
            bool_value(collection_items(vm, Some(&get(vm, recv, "rd_surfaces"))).contains(&target))
        }
        "getTargets" => get(vm, recv, "rd_surfaces"),
        "getLogicalCameraId" => get(vm, recv, "rd_logical_id"),
        "getRequestType" => {
            let cached = field_int(vm, recv, "rd_request_type");
            if cached != -1 {
                Value::Int(cached)
            } else if field_bool(vm, recv, "rd_reprocess") {
                put(vm, recv, "rd_request_type", Value::Int(1))?;
                Value::Int(1)
            } else {
                // The ZSL branch reads generated CaptureRequest keys, which
                // are not materialized as host statics in this slice; an
                // explicitly built regular request is type REGULAR unless a
                // caller has stored an equivalent type cache.  Do not claim
                // ZSL detection.
                put(vm, recv, "rd_request_type", Value::Int(0))?;
                Value::Int(0)
            }
        }
        "equals" => {
            let eq = match oarg(args, 0) {
                Some(o) if class_of(vm, o) == CAPTURE_REQUEST => {
                    let same_surfaces = {
                        let a = collection_items(vm, Some(&get(vm, recv, "rd_surfaces")));
                        let b = collection_items(vm, Some(&get(vm, o, "rd_surfaces")));
                        a.len() == b.len() && a.iter().all(|x| b.contains(x))
                    };
                    let same_settings = match (
                        capture_request_settings(vm, recv),
                        capture_request_settings(vm, o),
                    ) {
                        (Some(a), Some(b)) => metadata_contents_equal(vm, a, b),
                        (None, None) => true,
                        _ => false,
                    };
                    let same_physical = capture_request_physical_equal(vm, recv, o);
                    same_surfaces
                        && same_physical
                        && same_settings
                        && opt_str(vm, Some(&get(vm, recv, "rd_logical_id")))
                            == opt_str(vm, Some(&get(vm, o, "rd_logical_id")))
                        && field_bool(vm, recv, "rd_reprocess") == field_bool(vm, o, "rd_reprocess")
                        && field_int(vm, recv, "rd_session_id") == field_int(vm, o, "rd_session_id")
                        && objects_equal(vm, &get(vm, recv, "rd_tag"), &get(vm, o, "rd_tag"))
                }
                _ => false,
            };
            bool_value(eq)
        }
        "hashCode" => Value::Int(capture_request_hash(vm, recv)?),
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}
fn capture_request_builder_instance(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let request_of = |vm: &mut Engine, b: ObjRef| -> Result<ObjRef, VmExit> {
        if let Some(r) = field_obj(vm, b, "rd_request") {
            Ok(r)
        } else {
            let r = alloc_obj(vm, CAPTURE_REQUEST, Vec::new())?;
            capture_request_init_empty(vm, r)?;
            let settings = new_metadata_native(vm)?;
            put(vm, r, "rd_settings", Value::Obj(settings))?;
            put(vm, b, "rd_request", Value::Obj(r))?;
            Ok(r)
        }
    };
    Ok(Some(match name {
        "<init>" => {
            let request = alloc_obj(vm, CAPTURE_REQUEST, Vec::new())?;
            capture_request_init_from_template(vm, request, args)?;
            put(vm, recv, "rd_request", Value::Obj(request))?;
            Value::Null
        }
        "addTarget" => {
            let request = request_of(vm, recv)?;
            let target = args.first().cloned().unwrap_or(Value::Null);
            // ArraySet.add(null) is permitted by the collection itself even
            // though the parameter is annotated @NonNull.
            let mut surfaces = collection_items(vm, Some(&get(vm, request, "rd_surfaces")));
            if !surfaces.contains(&target) {
                surfaces.push(target);
                let list = new_list(vm, surfaces)?;
                put(vm, request, "rd_surfaces", Value::Obj(list))?;
            }
            Value::Null
        }
        "removeTarget" => {
            let request = request_of(vm, recv)?;
            let target = args.first().cloned().unwrap_or(Value::Null);
            let mut surfaces = collection_items(vm, Some(&get(vm, request, "rd_surfaces")));
            surfaces.retain(|s| *s != target);
            let list = new_list(vm, surfaces)?;
            put(vm, request, "rd_surfaces", Value::Obj(list))?;
            Value::Null
        }
        "set" => {
            let request = request_of(vm, recv)?;
            let settings = match capture_request_settings(vm, request) {
                Some(m) => m,
                None => {
                    let m = new_metadata_native(vm)?;
                    put(vm, request, "rd_settings", Value::Obj(m))?;
                    m
                }
            };
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            metadata_set(
                vm,
                settings,
                key,
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "get" => {
            let request = request_of(vm, recv)?;
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            match capture_request_settings(vm, request) {
                Some(m) => metadata_get(vm, m, &key),
                None => Value::Null,
            }
        }
        "setPhysicalCameraKey" => {
            let request = request_of(vm, recv)?;
            let key = args.first().cloned().unwrap_or(Value::Null);
            let physical_id = args.get(2).cloned().unwrap_or(Value::Null);
            let settings = capture_request_physical_settings(vm, request, &physical_id)?;
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            metadata_set(
                vm,
                settings,
                key,
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Obj(recv)
        }
        "getPhysicalCameraKey" => {
            let request = request_of(vm, recv)?;
            let key = args.first().cloned().unwrap_or(Value::Null);
            let physical_id = args.get(1).cloned().unwrap_or(Value::Null);
            let settings = capture_request_physical_settings(vm, request, &physical_id)?;
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            metadata_get(vm, settings, &key)
        }
        "setTag" => {
            let request = request_of(vm, recv)?;
            put(
                vm,
                request,
                "rd_tag",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "setPartOfCHSRequestList" => {
            let request = request_of(vm, recv)?;
            put(vm, request, "rd_part_chs", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        "build" => {
            let request = request_of(vm, recv)?;
            let built = alloc_obj(vm, CAPTURE_REQUEST, Vec::new())?;
            capture_request_copy_into(vm, request, built)?;
            Value::Obj(built)
        }
        "isEmpty" => {
            let request = request_of(vm, recv)?;
            bool_value(match capture_request_settings(vm, request) {
                Some(m) => metadata_entries(vm, m).is_empty(),
                None => true,
            })
        }
        _ => return Ok(None),
    }))
}
fn capture_result_extras_field(vm: &Engine, extras: &Value, field: &str) -> Value {
    match extras {
        Value::Obj(r) => get(vm, *r, field),
        _ => Value::Null,
    }
}
fn capture_result_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_total = class_of(vm, recv) == TOTAL_CAPTURE_RESULT;
    Ok(Some(match name {
        "<init>" => {
            // Identify overloads by signature/arity.
            let (camera_id, results_arg, request, sequence, frame) =
                if sig.contains("CaptureResultExtras") {
                    let extras = args.get(3).cloned().unwrap_or(Value::Null);
                    if matches!(args.get(2), None | Some(Value::Null)) {
                        return Err(iae("parent was null"));
                    }
                    if matches!(extras, Value::Null) && !is_total {
                        return Err(iae("extras was null"));
                    }
                    (
                        args.first().cloned().unwrap_or(Value::Null),
                        args.get(1).cloned().unwrap_or(Value::Null),
                        args.get(2).cloned().unwrap_or(Value::Null),
                        match capture_result_extras_field(vm, &extras, "rd_request_id") {
                            Value::Int(i) => i,
                            _ => field_int(vm, obj(Some(&extras)).unwrap_or(recv), "mRequestId"),
                        },
                        match capture_result_extras_field(vm, &extras, "rd_frame_number") {
                            Value::Long(l) => l,
                            Value::Int(i) => i as i64,
                            _ => 0,
                        },
                    )
                } else if args.len() >= 5 && sig.contains("Ljava/lang/String;") {
                    if matches!(args.get(2), None | Some(Value::Null)) {
                        return Err(iae("parent was null"));
                    }
                    (
                        args.first().cloned().unwrap_or(Value::Null),
                        args.get(1).cloned().unwrap_or(Value::Null),
                        args.get(2).cloned().unwrap_or(Value::Null),
                        iarg(args, 3),
                        match args.get(4) {
                            Some(Value::Long(l)) => *l,
                            Some(Value::Int(i)) => *i as i64,
                            _ => 0,
                        },
                    )
                } else {
                    (
                        jstr(vm, "none")?,
                        args.first().cloned().unwrap_or(Value::Null),
                        Value::Null,
                        iarg(args, 1),
                        -1i64,
                    )
                };
            let results = match results_arg {
                Value::Obj(r) => r,
                _ => return Err(iae("results was null")),
            };
            let moved = metadata_move(vm, results)?;
            if metadata_entries(vm, moved).is_empty() {
                return Err(assertion("Results must not be empty"));
            }
            put(vm, recv, "rd_camera_id", camera_id)?;
            put(vm, recv, "rd_results", Value::Obj(moved))?;
            put(vm, recv, "rd_request", request)?;
            put(vm, recv, "rd_sequence_id", Value::Int(sequence))?;
            put(vm, recv, "rd_frame_number", Value::Long(frame))?;
            if is_total {
                // TotalCaptureResult overloads append partials/session at
                // different indexes; find a List argument after the base
                // constructor arguments and retain it (null => empty list).
                let partials = args
                    .iter()
                    .find(|v| matches!(v, Value::Obj(r) if matches!(vm.fw.objects.get(r), Some(HostObj::List { .. }))))
                    .cloned()
                    .unwrap_or(Value::Null);
                if matches!(partials, Value::Null) {
                    let empty = new_list(vm, Vec::new())?;
                    put(vm, recv, "rd_partial_results", Value::Obj(empty))?;
                } else {
                    put(vm, recv, "rd_partial_results", partials)?;
                }
                // The request-less Total constructor uses SESSION_ID_NONE.
                // For the full constructors, the session id is the int
                // immediately following the partials list; locating it by
                // position would be brittle across the two overloads, so a
                // stored test-provided field takes precedence and -1 is the
                // conservative default.
                if matches!(get(vm, recv, "rd_session_id"), Value::Null) {
                    put(vm, recv, "rd_session_id", Value::Int(-1))?;
                }
                let physical = alloc_obj(vm, "Ljava/util/HashMap;", Vec::new())?;
                vm.fw.objects.insert(
                    physical,
                    HostObj::Map {
                        entries: Vec::new(),
                    },
                );
                put(vm, recv, "rd_physical_results", Value::Obj(physical))?;
            }
            Value::Null
        }
        "getNativeCopy" => {
            let copy = new_metadata_native(vm)?;
            if let Some(m) = field_obj(vm, recv, "rd_results") {
                metadata_copy_to(vm, m, copy)?;
            }
            Value::Obj(copy)
        }
        "getCameraId" => get(vm, recv, "rd_camera_id"),
        "get" => {
            let key = args.first().cloned().unwrap_or(Value::Null);
            if matches!(key, Value::Null) {
                return Err(npe("key must not be null"));
            }
            match field_obj(vm, recv, "rd_results") {
                Some(m) => metadata_get(vm, m, &key),
                None => Value::Null,
            }
        }
        "getKeys" => metadata_keys_as(vm, field_obj(vm, recv, "rd_results"), CAPTURE_RESULT_KEY)?,
        "getRequest" => get(vm, recv, "rd_request"),
        "getFrameNumber" => match get(vm, recv, "rd_frame_number") {
            Value::Long(l) => Value::Long(l),
            Value::Int(i) => Value::Long(i as i64),
            _ => Value::Long(0),
        },
        "getSequenceId" => Value::Int(field_int(vm, recv, "rd_sequence_id")),
        "getPartialResults" if is_total => {
            let p = get(vm, recv, "rd_partial_results");
            if matches!(p, Value::Null) {
                Value::Obj(new_list(vm, Vec::new())?)
            } else {
                p
            }
        }
        "getSessionId" if is_total => Value::Int(field_int(vm, recv, "rd_session_id")),
        "getPhysicalCameraResults" | "getPhysicalCameraTotalResults" if is_total => {
            let p = get(vm, recv, "rd_physical_results");
            if matches!(p, Value::Null) {
                let empty = alloc_obj(vm, "Ljava/util/HashMap;", Vec::new())?;
                vm.fw.objects.insert(
                    empty,
                    HostObj::Map {
                        entries: Vec::new(),
                    },
                );
                Value::Obj(empty)
            } else {
                p
            }
        }
        "dumpToLog" => Value::Null,
        _ => return Ok(None),
    }))
}
fn camera_default_message(problem: i32) -> Option<&'static str> {
    Some(match problem {
        4 => "The camera device is in use already",
        5 => "The system-wide limit for number of open cameras has been reached, and more camera devices cannot be opened until previous instances are closed.",
        2 => "The camera device is removable and has been disconnected from the Android device, or the camera service has shut down the connection due to a higher-priority access request for the camera device.",
        1 => "The camera is disabled due to a device policy, and cannot be opened.",
        3 => "The camera device is currently in the error state; no further calls to it will succeed.",
        _ => return None,
    })
}
fn camera_problem_string(problem: i32) -> &'static str {
    match problem {
        4 => "CAMERA_IN_USE",
        5 => "MAX_CAMERAS_IN_USE",
        2 => "CAMERA_DISCONNECTED",
        1 => "CAMERA_DISABLED",
        3 => "CAMERA_ERROR",
        1000 => "CAMERA_DEPRECATED_HAL",
        _ => "<UNKNOWN ERROR>",
    }
}
fn camera_access_exception_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            let problem = iarg(args, 0);
            put(vm, recv, "rd_reason", Value::Int(problem))?;
            let (message, cause) = if sig == "(I)V" {
                (camera_default_message(problem).map(|s| s.to_string()), None)
            } else if sig == "(ILjava/lang/String;)V" {
                let raw = opt_str(vm, args.get(1));
                (
                    Some(format!(
                        "{} ({problem}): {}",
                        camera_problem_string(problem),
                        raw.as_deref().unwrap_or("null")
                    )),
                    None,
                )
            } else if sig.contains("Ljava/lang/String;Ljava/lang/Throwable;") {
                let raw = opt_str(vm, args.get(1));
                (
                    Some(format!(
                        "{} ({problem}): {}",
                        camera_problem_string(problem),
                        raw.as_deref().unwrap_or("null")
                    )),
                    args.get(2).cloned(),
                )
            } else {
                (
                    camera_default_message(problem).map(|s| s.to_string()),
                    args.get(1).cloned(),
                )
            };
            match message {
                Some(m) => {
                    let stored = jstr(vm, m)?;
                    put(vm, recv, "message", stored)?
                }
                None => put(vm, recv, "message", Value::Null)?,
            }
            match cause {
                Some(c) => {
                    put(vm, recv, "cause", c)?;
                    put(vm, recv, "rd_cause_set", Value::Int(1))?;
                }
                None => {
                    put(vm, recv, "cause", Value::Null)?;
                    put(vm, recv, "rd_cause_set", Value::Int(0))?;
                }
            }
            Value::Null
        }
        "getReason" => Value::Int(field_int(vm, recv, "rd_reason")),
        _ => return Ok(None),
    }))
}
fn camera_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => {
            put(
                vm,
                recv,
                "rd_context",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        "getCameraIdList" | "getCameraIdListNoLazy" => {
            obj_array(vm, "[Ljava/lang/String;", Vec::new())?
        }
        "getConcurrentCameraIds" => {
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", Vec::new())?)
        }
        "isConcurrentSessionConfigurationSupported" => {
            // With no concurrent combinations, every map is unsupported;
            // the public contract also requires a non-empty subset, so an
            // empty map is unsupported as well.  A null map dereferences in
            // the global implementation.
            req_obj(args, 0, "cameraIdAndSessionConfig")?;
            Value::Int(0)
        }
        "registerAvailabilityCallback" | "registerTorchCallback" => {
            // Callbacks are retained but never fire: there are no cameras
            // and therefore no initial availability/torch events to send.
            let callback = if sig.starts_with("(Ljava/util/concurrent/Executor;") {
                args.get(1).cloned().unwrap_or(Value::Null)
            } else {
                args.first().cloned().unwrap_or(Value::Null)
            };
            if matches!(callback, Value::Null) {
                return Err(npe("callback"));
            }
            put(vm, recv, "rd_callback", callback)?;
            Value::Null
        }
        "unregisterAvailabilityCallback" | "unregisterTorchCallback" => {
            req_obj(args, 0, "callback")?;
            Value::Null
        }
        "getCameraCharacteristics" => {
            // Service-disabled profile: this check precedes cameraId use.
            return Err(iae("No cameras available on device"));
        }
        "getCameraExtensionCharacteristics" => {
            return Err(iae("No cameras available on device"));
        }
        "openCamera" | "openCameraForUid" => {
            // Mirror the overload-specific null checks before the
            // service-disabled failure.
            if sig.contains("Ljava/util/concurrent/Executor;") {
                let executor_idx = if sig
                    .starts_with("(Ljava/lang/String;Ljava/util/concurrent/Executor;")
                {
                    1
                } else if sig.starts_with("(Ljava/lang/String;ILjava/util/concurrent/Executor;") {
                    2
                } else {
                    usize::MAX
                };
                if executor_idx != usize::MAX && oarg(args, executor_idx).is_none() {
                    return Err(iae("executor was null"));
                }
                if sig.starts_with("(Ljava/lang/String;I")
                    && name == "openCamera"
                    && iarg(args, 1) < 0
                {
                    return Err(iae(
                        "oomScoreOffset < 0, cannot increase priority of camera client",
                    ));
                }
            }
            if opt_str(vm, args.first()).is_none() {
                return Err(iae("cameraId was null"));
            }
            let callback_idx = if sig.contains("ZLandroid/os/Handler;")
                || sig.contains("ILjava/util/concurrent/Executor;")
            {
                3
            } else if sig.contains("Ljava/util/concurrent/Executor;") {
                // (String, Executor, Callback), and openCameraForUid also
                // has (String, Callback, Executor, ...).  Distinguish by the
                // argument right after the id.
                if sig.starts_with("(Ljava/lang/String;Ljava/util/concurrent/Executor;") {
                    2
                } else {
                    1
                }
            } else {
                1
            };
            if oarg(args, callback_idx).is_none() {
                return Err(iae("callback was null"));
            }
            return Err(iae("No cameras available on device"));
        }
        "setTorchMode" => {
            return Err(iae("No cameras available on device"));
        }
        "turnOnTorchWithStrengthLevel" => {
            return Err(iae("No camera available on device"));
        }
        "getTorchStrengthLevel" => {
            return Err(iae("No camera available on device."));
        }
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------------
// android.animation: ValueAnimator/ObjectAnimator on the virtual frame clock
// ---------------------------------------------------------------------------

const ANIM_KIND_INT: i32 = 1;
const ANIM_KIND_FLOAT: i32 = 2;
const ANIM_KIND_OBJECT: i32 = 3;

fn anim_class_cast(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/ClassCastException;", msg)
}
fn anim_put_bool(vm: &mut Engine, r: ObjRef, field: &str, value: bool) -> Result<(), VmExit> {
    put(vm, r, field, bool_value(value))
}
fn anim_list_items(vm: &Engine, r: ObjRef, field: &str) -> Vec<Value> {
    collection_items(vm, Some(&get(vm, r, field)))
}
fn anim_set_list_items(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    items: Vec<Value>,
) -> Result<(), VmExit> {
    if let Some(list) = field_obj(vm, r, field) {
        if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
            *elems = items;
            return Ok(());
        }
    }
    let list = new_collection(vm, "Ljava/util/ArrayList;", items)?;
    put(vm, r, field, Value::Obj(list))
}
fn anim_append_listener(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    listener: Value,
) -> Result<(), VmExit> {
    let mut items = anim_list_items(vm, r, field);
    items.push(listener);
    anim_set_list_items(vm, r, field, items)
}
fn anim_remove_listener(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    listener: &Value,
) -> Result<(), VmExit> {
    if field_obj(vm, r, field).is_none() {
        return Ok(());
    }
    let mut items = anim_list_items(vm, r, field);
    if let Some(pos) = items.iter().position(|v| v == listener) {
        items.remove(pos);
    }
    if items.is_empty() {
        put(vm, r, field, Value::Null)
    } else {
        anim_set_list_items(vm, r, field, items)
    }
}
fn anim_copy_list_field(
    vm: &mut Engine,
    source: ObjRef,
    dest: ObjRef,
    field: &str,
) -> Result<(), VmExit> {
    if field_obj(vm, source, field).is_some() {
        let items = anim_list_items(vm, source, field);
        let list = new_collection(vm, "Ljava/util/ArrayList;", items)?;
        put(vm, dest, field, Value::Obj(list))
    } else {
        put(vm, dest, field, Value::Null)
    }
}
fn anim_number_int(vm: &Engine, value: &Value) -> Option<i32> {
    match value {
        Value::Int(v) => Some(*v),
        Value::Long(v) => Some(*v as i32),
        Value::Float(v) => Some(*v as i32),
        Value::Double(v) => Some(*v as i32),
        Value::Obj(r) => match get(vm, *r, "value") {
            Value::Int(v) => Some(v),
            Value::Long(v) => Some(v as i32),
            Value::Float(v) => Some(v as i32),
            Value::Double(v) => Some(v as i32),
            _ => None,
        },
        _ => None,
    }
}
fn anim_number_float(vm: &Engine, value: &Value) -> Option<f32> {
    match value {
        Value::Int(v) => Some(*v as f32),
        Value::Long(v) => Some(*v as f32),
        Value::Float(v) => Some(*v),
        Value::Double(v) => Some(*v as f32),
        Value::Obj(r) => match get(vm, *r, "value") {
            Value::Int(v) => Some(v as f32),
            Value::Long(v) => Some(v as f32),
            Value::Float(v) => Some(v),
            Value::Double(v) => Some(v as f32),
            _ => None,
        },
        _ => None,
    }
}
fn anim_boxed(vm: &mut Engine, kind: i32, value: &Value) -> Result<Value, VmExit> {
    match value {
        Value::Obj(_) | Value::Null => Ok(value.clone()),
        Value::Int(v) => Ok(Value::Obj(crate::boxes::box_value(
            vm,
            "Ljava/lang/Integer;",
            Value::Int(*v),
        )?)),
        Value::Float(v) => Ok(Value::Obj(crate::boxes::box_value(
            vm,
            "Ljava/lang/Float;",
            Value::Float(*v),
        )?)),
        Value::Long(v) => Ok(Value::Obj(crate::boxes::box_value(
            vm,
            "Ljava/lang/Long;",
            Value::Long(*v),
        )?)),
        Value::Double(v) => Ok(Value::Obj(crate::boxes::box_value(
            vm,
            "Ljava/lang/Double;",
            Value::Double(*v),
        )?)),
        _ => {
            let _ = kind;
            Ok(Value::Null)
        }
    }
}
fn anim_internal_from_object(vm: &Engine, kind: i32, value: &Value) -> Result<Value, VmExit> {
    match kind {
        ANIM_KIND_INT => anim_number_int(vm, value)
            .map(Value::Int)
            .ok_or_else(|| anim_class_cast("value is not an Integer")),
        ANIM_KIND_FLOAT => anim_number_float(vm, value)
            .map(Value::Float)
            .ok_or_else(|| anim_class_cast("value is not a Float")),
        _ => Ok(value.clone()),
    }
}
fn anim_type_desc(vm: &Engine, value: &Value) -> String {
    match value {
        Value::Int(_) => "I".to_string(),
        Value::Float(_) => "F".to_string(),
        Value::Long(_) => "J".to_string(),
        Value::Double(_) => "D".to_string(),
        Value::Obj(r) => class_of(vm, *r),
        _ => "Ljava/lang/Object;".to_string(),
    }
}
fn anim_class_value(vm: &mut Engine, desc: &str) -> Result<Value, VmExit> {
    Ok(Value::Obj(vm.class_object_for(desc)?))
}
fn anim_property_name(vm: &mut Engine, property: &Value) -> Option<String> {
    let Value::Obj(r) = property else {
        return None;
    };
    for field in ["rd_name", "mName", "name"] {
        if let Some(s) = field_str(vm, *r, field) {
            return Some(s);
        }
    }
    match vm.invoke_instance(*r, "getName", "()Ljava/lang/String;", &[]) {
        Ok(v) => opt_str(vm, Some(&v)),
        Err(_) => None,
    }
}

fn anim_new_keyframe(
    vm: &mut Engine,
    kind: i32,
    fraction: f32,
    value: Value,
    has_value: bool,
    value_type: &str,
) -> Result<ObjRef, VmExit> {
    let class = match kind {
        ANIM_KIND_INT => KEYFRAME_INT,
        ANIM_KIND_FLOAT => KEYFRAME_FLOAT,
        _ => KEYFRAME_OBJECT,
    };
    let r = alloc_obj(vm, class, Vec::new())?;
    put(vm, r, "rd_kind", Value::Int(kind))?;
    put(vm, r, "rd_fraction", Value::Float(fraction))?;
    put(vm, r, "rd_value", value)?;
    anim_put_bool(vm, r, "rd_has_value", has_value)?;
    anim_put_bool(vm, r, "rd_value_set_on_start", false)?;
    put(vm, r, "rd_interpolator", Value::Null)?;
    let type_name = jstr(vm, value_type)?;
    put(vm, r, "rd_value_type", type_name)?;
    Ok(r)
}
fn anim_keyframes_from_ints(vm: &mut Engine, values: &[i32]) -> Result<Vec<ObjRef>, VmExit> {
    let mut out = Vec::new();
    if values.len() == 1 {
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_INT,
            0.0,
            Value::Int(0),
            false,
            "I",
        )?);
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_INT,
            1.0,
            Value::Int(values[0]),
            true,
            "I",
        )?);
    } else {
        for (i, v) in values.iter().enumerate() {
            let fraction = if values.len() <= 1 {
                0.0
            } else {
                i as f32 / (values.len() - 1) as f32
            };
            out.push(anim_new_keyframe(
                vm,
                ANIM_KIND_INT,
                fraction,
                Value::Int(*v),
                true,
                "I",
            )?);
        }
    }
    Ok(out)
}
fn anim_keyframes_from_floats(vm: &mut Engine, values: &[f32]) -> Result<Vec<ObjRef>, VmExit> {
    let mut out = Vec::new();
    if values.len() == 1 {
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_FLOAT,
            0.0,
            Value::Float(0.0),
            false,
            "F",
        )?);
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_FLOAT,
            1.0,
            Value::Float(values[0]),
            true,
            "F",
        )?);
    } else {
        for (i, v) in values.iter().enumerate() {
            let fraction = if values.len() <= 1 {
                0.0
            } else {
                i as f32 / (values.len() - 1) as f32
            };
            out.push(anim_new_keyframe(
                vm,
                ANIM_KIND_FLOAT,
                fraction,
                Value::Float(*v),
                true,
                "F",
            )?);
        }
    }
    Ok(out)
}
fn anim_keyframes_from_objects(vm: &mut Engine, values: &[Value]) -> Result<Vec<ObjRef>, VmExit> {
    let mut out = Vec::new();
    if values.len() == 1 {
        let value_type = anim_type_desc(vm, &values[0]);
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_OBJECT,
            0.0,
            Value::Null,
            false,
            "Ljava/lang/Object;",
        )?);
        out.push(anim_new_keyframe(
            vm,
            ANIM_KIND_OBJECT,
            1.0,
            values[0].clone(),
            !matches!(values[0], Value::Null),
            &value_type,
        )?);
    } else {
        for (i, v) in values.iter().enumerate() {
            let fraction = if values.len() <= 1 {
                0.0
            } else {
                i as f32 / (values.len() - 1) as f32
            };
            let value_type = anim_type_desc(vm, v);
            out.push(anim_new_keyframe(
                vm,
                ANIM_KIND_OBJECT,
                fraction,
                v.clone(),
                !matches!(v, Value::Null),
                &value_type,
            )?);
        }
    }
    Ok(out)
}
fn anim_keyframe_kind(vm: &Engine, kf: ObjRef) -> i32 {
    let kind = field_int(vm, kf, "rd_kind");
    if kind != 0 {
        return kind;
    }
    match class_of(vm, kf).as_str() {
        KEYFRAME_INT => ANIM_KIND_INT,
        KEYFRAME_FLOAT => ANIM_KIND_FLOAT,
        _ => ANIM_KIND_OBJECT,
    }
}
fn anim_keyframe_internal(vm: &Engine, kf: ObjRef) -> Value {
    match anim_keyframe_kind(vm, kf) {
        ANIM_KIND_INT => Value::Int(field_int(vm, kf, "rd_value")),
        ANIM_KIND_FLOAT => Value::Float(field_float(vm, kf, "rd_value")),
        _ => get(vm, kf, "rd_value"),
    }
}
fn anim_clone_keyframe(vm: &mut Engine, kf: ObjRef) -> Result<ObjRef, VmExit> {
    let kind = anim_keyframe_kind(vm, kf);
    let has = field_bool(vm, kf, "rd_has_value");
    let value = if has {
        anim_keyframe_internal(vm, kf)
    } else {
        match kind {
            ANIM_KIND_INT => Value::Int(0),
            ANIM_KIND_FLOAT => Value::Float(0.0),
            _ => Value::Null,
        }
    };
    let value_type =
        field_str(vm, kf, "rd_value_type").unwrap_or_else(|| anim_type_desc(vm, &value));
    let clone = anim_new_keyframe(
        vm,
        kind,
        field_float(vm, kf, "rd_fraction"),
        value,
        has,
        &value_type,
    )?;
    let interpolator = get(vm, kf, "rd_interpolator");
    put(vm, clone, "rd_interpolator", interpolator)?;
    anim_put_bool(
        vm,
        clone,
        "rd_value_set_on_start",
        field_bool(vm, kf, "rd_value_set_on_start"),
    )?;
    Ok(clone)
}
fn anim_set_keyframe_internal(
    vm: &mut Engine,
    kf: ObjRef,
    value: &Value,
    mark_set_on_start: bool,
) -> Result<(), VmExit> {
    let kind = anim_keyframe_kind(vm, kf);
    let internal = anim_internal_from_object(vm, kind, value)?;
    put(vm, kf, "rd_value", internal)?;
    let has = kind == ANIM_KIND_OBJECT && matches!(value, Value::Null);
    anim_put_bool(vm, kf, "rd_has_value", !has)?;
    if mark_set_on_start {
        anim_put_bool(vm, kf, "rd_value_set_on_start", true)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // one argument per persisted holder field
fn anim_new_pvh(
    vm: &mut Engine,
    property_name: Option<String>,
    property: Value,
    kind: i32,
    keyframes: Vec<ObjRef>,
    evaluator: Value,
    converter: Value,
    value_type: &str,
    generic_keyframes: bool,
) -> Result<ObjRef, VmExit> {
    let r = alloc_obj(vm, PROPERTY_VALUES_HOLDER, Vec::new())?;
    put_opt_str(vm, r, "rd_property_name", property_name)?;
    put(vm, r, "rd_property", property)?;
    put(vm, r, "rd_kind", Value::Int(kind))?;
    put(vm, r, "rd_evaluator", evaluator)?;
    put(vm, r, "rd_converter", converter)?;
    let type_name = jstr(vm, value_type)?;
    put(vm, r, "rd_value_type", type_name)?;
    anim_put_bool(vm, r, "rd_generic_keyframes", generic_keyframes)?;
    let animated = match kind {
        ANIM_KIND_INT => Value::Int(0),
        ANIM_KIND_FLOAT => Value::Float(0.0),
        _ => Value::Null,
    };
    put(vm, r, "rd_animated", animated)?;
    let items = keyframes.into_iter().map(Value::Obj).collect();
    anim_set_list_items(vm, r, "rd_keyframes", items)?;
    Ok(r)
}
fn anim_pvh_keyframes(vm: &Engine, holder: ObjRef) -> Vec<ObjRef> {
    anim_list_items(vm, holder, "rd_keyframes")
        .iter()
        .filter_map(|v| obj(Some(v)))
        .collect()
}
fn anim_pvh_replace_keyframes(
    vm: &mut Engine,
    holder: ObjRef,
    kind: i32,
    keyframes: Vec<ObjRef>,
    value_type: &str,
    generic_keyframes: bool,
) -> Result<(), VmExit> {
    put(vm, holder, "rd_kind", Value::Int(kind))?;
    let type_name = jstr(vm, value_type)?;
    put(vm, holder, "rd_value_type", type_name)?;
    anim_put_bool(vm, holder, "rd_generic_keyframes", generic_keyframes)?;
    let items = keyframes.into_iter().map(Value::Obj).collect();
    anim_set_list_items(vm, holder, "rd_keyframes", items)
}
fn anim_pvh_set_int_values(vm: &mut Engine, holder: ObjRef, values: &[i32]) -> Result<(), VmExit> {
    let keyframes = anim_keyframes_from_ints(vm, values)?;
    anim_pvh_replace_keyframes(vm, holder, ANIM_KIND_INT, keyframes, "I", false)
}
fn anim_pvh_set_float_values(
    vm: &mut Engine,
    holder: ObjRef,
    values: &[f32],
) -> Result<(), VmExit> {
    let keyframes = anim_keyframes_from_floats(vm, values)?;
    anim_pvh_replace_keyframes(vm, holder, ANIM_KIND_FLOAT, keyframes, "F", false)
}
fn anim_pvh_set_object_values(
    vm: &mut Engine,
    holder: ObjRef,
    values: &[Value],
) -> Result<(), VmExit> {
    let first = values.first().ok_or_else(|| oob("0"))?;
    if matches!(first, Value::Null) {
        return Err(npe("values[0]"));
    }
    let value_type = anim_type_desc(vm, first);
    let keyframes = anim_keyframes_from_objects(vm, values)?;
    anim_pvh_replace_keyframes(vm, holder, ANIM_KIND_OBJECT, keyframes, &value_type, false)
}
fn anim_pvh_kind_from_keyframes(vm: &Engine, keyframes: &[ObjRef]) -> i32 {
    let mut has_int = false;
    let mut has_float = false;
    let mut has_object = false;
    for kf in keyframes {
        match anim_keyframe_kind(vm, *kf) {
            ANIM_KIND_INT => has_int = true,
            ANIM_KIND_FLOAT => has_float = true,
            _ => has_object = true,
        }
    }
    if has_int && !has_float && !has_object {
        ANIM_KIND_INT
    } else if has_float && !has_int && !has_object {
        ANIM_KIND_FLOAT
    } else {
        ANIM_KIND_OBJECT
    }
}
fn anim_clone_pvh(vm: &mut Engine, holder: ObjRef) -> Result<ObjRef, VmExit> {
    let mut keyframes = Vec::new();
    for kf in anim_pvh_keyframes(vm, holder) {
        keyframes.push(anim_clone_keyframe(vm, kf)?);
    }
    let clone = anim_new_pvh(
        vm,
        field_str(vm, holder, "rd_property_name"),
        get(vm, holder, "rd_property"),
        field_int(vm, holder, "rd_kind"),
        keyframes,
        get(vm, holder, "rd_evaluator"),
        get(vm, holder, "rd_converter"),
        &field_str(vm, holder, "rd_value_type").unwrap_or_else(|| "Ljava/lang/Object;".to_string()),
        field_bool(vm, holder, "rd_generic_keyframes"),
    )?;
    let animated = get(vm, holder, "rd_animated");
    put(vm, clone, "rd_animated", animated)?;
    Ok(clone)
}
fn anim_pvh_animated_value(vm: &mut Engine, holder: ObjRef) -> Result<Value, VmExit> {
    let kind = field_int(vm, holder, "rd_kind");
    let value = get(vm, holder, "rd_animated");
    anim_boxed(vm, kind, &value)
}
fn anim_int_lerp(fraction: f32, start: i32, end: i32) -> i32 {
    start.wrapping_add((fraction * end.wrapping_sub(start) as f32) as i32)
}
fn anim_float_lerp(fraction: f32, start: f32, end: f32) -> f32 {
    start + fraction * (end - start)
}
fn anim_java_round(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else if value >= i32::MAX as f32 {
        i32::MAX
    } else if value <= i32::MIN as f32 {
        i32::MIN
    } else {
        (value + 0.5).floor() as i32
    }
}
fn anim_argb_evaluate(fraction: f32, start: i32, end: i32) -> i32 {
    let channel = |v: i32, shift: u32| ((v >> shift) & 0xff) as f32 / 255.0;
    let start_a = channel(start, 24);
    let end_a = channel(end, 24);
    let start_r = channel(start, 16).powf(2.2);
    let start_g = channel(start, 8).powf(2.2);
    let start_b = channel(start, 0).powf(2.2);
    let end_r = channel(end, 16).powf(2.2);
    let end_g = channel(end, 8).powf(2.2);
    let end_b = channel(end, 0).powf(2.2);
    let a = anim_float_lerp(fraction, start_a, end_a) * 255.0;
    let r = anim_float_lerp(fraction, start_r, end_r).powf(1.0 / 2.2) * 255.0;
    let g = anim_float_lerp(fraction, start_g, end_g).powf(1.0 / 2.2) * 255.0;
    let b = anim_float_lerp(fraction, start_b, end_b).powf(1.0 / 2.2) * 255.0;
    (((anim_java_round(a) as u32) << 24)
        | ((anim_java_round(r) as u32) << 16)
        | ((anim_java_round(g) as u32) << 8)
        | (anim_java_round(b) as u32)) as i32
}
fn anim_evaluate_interval(
    vm: &mut Engine,
    holder: ObjRef,
    fraction: f32,
    start: &Value,
    end: &Value,
) -> Result<Value, VmExit> {
    let kind = field_int(vm, holder, "rd_kind");
    let evaluator = get(vm, holder, "rd_evaluator");
    if let Value::Obj(evaluator_ref) = evaluator {
        let evaluator_class = class_of(vm, evaluator_ref);
        let result = match evaluator_class.as_str() {
            INT_EVALUATOR => {
                let s = anim_number_int(vm, start)
                    .ok_or_else(|| anim_class_cast("startValue is not an Integer"))?;
                let e = anim_number_int(vm, end)
                    .ok_or_else(|| anim_class_cast("endValue is not an Integer"))?;
                Value::Int(anim_int_lerp(fraction, s, e))
            }
            FLOAT_EVALUATOR => {
                let s = anim_number_float(vm, start)
                    .ok_or_else(|| anim_class_cast("startValue is not a Number"))?;
                let e = anim_number_float(vm, end)
                    .ok_or_else(|| anim_class_cast("endValue is not a Number"))?;
                Value::Float(anim_float_lerp(fraction, s, e))
            }
            ARGB_EVALUATOR => {
                let s = anim_number_int(vm, start)
                    .ok_or_else(|| anim_class_cast("startValue is not an Integer"))?;
                let e = anim_number_int(vm, end)
                    .ok_or_else(|| anim_class_cast("endValue is not an Integer"))?;
                Value::Int(anim_argb_evaluate(fraction, s, e))
            }
            _ => {
                let start_boxed = anim_boxed(vm, kind, start)?;
                let end_boxed = anim_boxed(vm, kind, end)?;
                let evaluated = vm.invoke_instance(
                    evaluator_ref,
                    "evaluate",
                    "(FLjava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                    &[Value::Float(fraction), start_boxed, end_boxed],
                )?;
                return anim_internal_from_object(vm, kind, &evaluated)
                    .or_else(|_| anim_boxed(vm, kind, &evaluated));
            }
        };
        return anim_internal_from_object(vm, kind, &result)
            .or_else(|_| anim_boxed(vm, kind, &result));
    }
    match kind {
        ANIM_KIND_INT => {
            let s = anim_number_int(vm, start).unwrap_or(0);
            let e = anim_number_int(vm, end).unwrap_or(0);
            Ok(Value::Int(anim_int_lerp(fraction, s, e)))
        }
        ANIM_KIND_FLOAT => {
            let s = anim_number_float(vm, start).unwrap_or(0.0);
            let e = anim_number_float(vm, end).unwrap_or(0.0);
            Ok(Value::Float(anim_float_lerp(fraction, s, e)))
        }
        _ => {
            // PropertyValuesHolder.init() supplies Int/Float evaluators for
            // Integer/Float object values. Other object types without an
            // evaluator dereference a null evaluator in AOSP.
            let start_class = anim_type_desc(vm, start);
            let end_class = anim_type_desc(vm, end);
            if start_class == "Ljava/lang/Integer;" && end_class == "Ljava/lang/Integer;" {
                let s = anim_number_int(vm, start).unwrap_or(0);
                let e = anim_number_int(vm, end).unwrap_or(0);
                anim_boxed(
                    vm,
                    ANIM_KIND_INT,
                    &Value::Int(anim_int_lerp(fraction, s, e)),
                )
            } else if matches!(
                start_class.as_str(),
                "Ljava/lang/Float;" | "Ljava/lang/Double;"
            ) && matches!(
                end_class.as_str(),
                "Ljava/lang/Float;" | "Ljava/lang/Double;"
            ) {
                let s = anim_number_float(vm, start).unwrap_or(0.0);
                let e = anim_number_float(vm, end).unwrap_or(0.0);
                anim_boxed(
                    vm,
                    ANIM_KIND_FLOAT,
                    &Value::Float(anim_float_lerp(fraction, s, e)),
                )
            } else {
                Err(npe("mEvaluator"))
            }
        }
    }
}
fn anim_pvh_calculate(vm: &mut Engine, holder: ObjRef, fraction: f32) -> Result<Value, VmExit> {
    let keyframes = anim_pvh_keyframes(vm, holder);
    if keyframes.is_empty() {
        return Ok(Value::Null);
    }
    let mut data = Vec::with_capacity(keyframes.len());
    for kf in keyframes {
        data.push((
            field_float(vm, kf, "rd_fraction"),
            anim_keyframe_internal(vm, kf),
            get(vm, kf, "rd_interpolator"),
        ));
    }
    let result = if data.len() == 1 {
        data[0].1.clone()
    } else if field_bool(vm, holder, "rd_generic_keyframes") && data.len() == 2 {
        let f = anim_apply_interpolator(vm, &data[1].2, fraction)?;
        anim_evaluate_interval(vm, holder, f, &data[0].1, &data[1].1)?
    } else {
        let (prev_idx, next_idx, whole_fraction) = if fraction <= 0.0 {
            (0, 1, true)
        } else if fraction >= 1.0 {
            (data.len() - 2, data.len() - 1, true)
        } else {
            let mut found = None;
            for (i, keyframe) in data.iter().enumerate().skip(1) {
                if fraction < keyframe.0 {
                    found = Some((i - 1, i, false));
                    break;
                }
            }
            match found {
                Some(v) => v,
                None => {
                    let last = data.last().map(|d| d.1.clone()).unwrap_or(Value::Null);
                    put(vm, holder, "rd_animated", last.clone())?;
                    return Ok(last);
                }
            }
        };
        let f = if whole_fraction {
            anim_apply_interpolator(vm, &data[next_idx].2, fraction)?
        } else {
            fraction
        };
        let mut interval = (f - data[prev_idx].0) / (data[next_idx].0 - data[prev_idx].0);
        if !whole_fraction {
            interval = anim_apply_interpolator(vm, &data[next_idx].2, interval)?;
        }
        anim_evaluate_interval(vm, holder, interval, &data[prev_idx].1, &data[next_idx].1)?
    };
    let kind = field_int(vm, holder, "rd_kind");
    let converter = get(vm, holder, "rd_converter");
    let result = if kind == ANIM_KIND_OBJECT {
        if let Value::Obj(converter_ref) = converter {
            let input = anim_boxed(vm, kind, &result)?;
            vm.invoke_instance(
                converter_ref,
                "convert",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                &[input],
            )?
        } else {
            result
        }
    } else {
        result
    };
    put(vm, holder, "rd_animated", result.clone())?;
    Ok(result)
}

fn anim_apply_interpolator(
    vm: &mut Engine,
    interpolator: &Value,
    fraction: f32,
) -> Result<f32, VmExit> {
    let Value::Obj(r) = interpolator else {
        return Ok(fraction);
    };
    let value = vm.invoke_instance(*r, "getInterpolation", "(F)F", &[Value::Float(fraction)])?;
    value.as_float().map_err(VmExit::from)
}
fn anim_global_duration_scale(vm: &Engine) -> f32 {
    match vm
        .statics
        .get(&(VALUE_ANIMATOR.to_string(), "rd_duration_scale".to_string()))
    {
        Some(Value::Float(v)) => *v,
        _ => 1.0,
    }
}
fn anim_set_global_duration_scale(vm: &mut Engine, scale: f32) {
    vm.statics.insert(
        (VALUE_ANIMATOR.to_string(), "rd_duration_scale".to_string()),
        Value::Float(scale),
    );
}
fn anim_frame_delay(vm: &Engine) -> i64 {
    match vm
        .statics
        .get(&(VALUE_ANIMATOR.to_string(), "rd_frame_delay".to_string()))
    {
        Some(Value::Long(v)) => *v,
        _ => 10,
    }
}
fn anim_set_frame_delay(vm: &mut Engine, delay: i64) {
    vm.statics.insert(
        (VALUE_ANIMATOR.to_string(), "rd_frame_delay".to_string()),
        Value::Long(delay),
    );
}
fn anim_handler_instance(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Some(handler) = vm.frames.animation_handler {
        return Ok(handler);
    }
    let handler = alloc_obj(vm, ANIMATION_HANDLER, Vec::new())?;
    vm.frames.animation_handler = Some(handler);
    Ok(handler)
}
fn anim_ensure_handler_posted(vm: &mut Engine) -> Result<(), VmExit> {
    let handler = anim_handler_instance(vm)?;
    if !vm
        .frames
        .callbacks
        .iter()
        .any(|(callback, _)| *callback == handler)
    {
        let now = vm.fw.clock();
        vm.frames.callbacks.push((handler, now));
    }
    Ok(())
}
fn anim_register_callback(vm: &mut Engine, callback: ObjRef, delay: i64) -> Result<(), VmExit> {
    let due = if delay > 0 {
        Some(vm.fw.clock().saturating_add(delay as u64))
    } else {
        None
    };
    if let Some(entry) = vm
        .frames
        .animation_callbacks
        .iter_mut()
        .find(|(callback_ref, _)| *callback_ref == callback)
    {
        if due.is_some() {
            entry.1 = due;
        }
    } else {
        vm.frames.animation_callbacks.push((callback, due));
    }
    anim_ensure_handler_posted(vm)
}
fn anim_unregister_callback(vm: &mut Engine, callback: ObjRef) {
    vm.frames
        .animation_callbacks
        .retain(|(callback_ref, _)| *callback_ref != callback);
}
fn anim_handler_process(vm: &mut Engine, frame_time_ms: i64) -> Result<(), VmExit> {
    let snapshot = vm.frames.animation_callbacks.clone();
    for (callback, due) in snapshot {
        if !vm
            .frames
            .animation_callbacks
            .iter()
            .any(|(callback_ref, _)| *callback_ref == callback)
        {
            continue;
        }
        if let Some(due_ms) = due {
            if due_ms < frame_time_ms.max(0) as u64 {
                if let Some(entry) = vm
                    .frames
                    .animation_callbacks
                    .iter_mut()
                    .find(|(callback_ref, _)| *callback_ref == callback)
                {
                    entry.1 = None;
                }
            } else {
                continue;
            }
        }
        let callback_class = class_of(vm, callback);
        if vm.cp.is_subtype(&callback_class, VALUE_ANIMATOR) {
            // ValueAnimator.doAnimationFrame is final. Calling the host
            // state machine directly also works for DEX subclasses, whose
            // runtime class is not itself a platform stub class.
            anim_do_animation_frame(vm, callback, frame_time_ms)?;
        } else {
            vm.invoke_instance(
                callback,
                "doAnimationFrame",
                "(J)Z",
                &[Value::Long(frame_time_ms)],
            )?;
        }
    }
    if !vm.frames.animation_callbacks.is_empty() {
        anim_ensure_handler_posted(vm)?;
    }
    Ok(())
}

fn anim_record_listener(
    vm: &mut Engine,
    listener: ObjRef,
    method: &str,
    animator: ObjRef,
    reverse: Option<bool>,
) -> Result<(), VmExit> {
    let (count_field, reverse_field) = match method {
        "onAnimationStart" => ("rd_on_start_count", Some("rd_last_start_reverse")),
        "onAnimationEnd" => ("rd_on_end_count", Some("rd_last_end_reverse")),
        "onAnimationCancel" => ("rd_on_cancel_count", None),
        "onAnimationRepeat" => ("rd_on_repeat_count", None),
        "onAnimationPause" => ("rd_on_pause_count", None),
        "onAnimationResume" => ("rd_on_resume_count", None),
        "onAnimationUpdate" => ("rd_on_update_count", None),
        "onChanged" => ("rd_on_changed_count", None),
        _ => return Ok(()),
    };
    let count = field_int(vm, listener, count_field).wrapping_add(1);
    put(vm, listener, count_field, Value::Int(count))?;
    put(vm, listener, "rd_last_animation", Value::Obj(animator))?;
    if let (Some(field), Some(is_reverse)) = (reverse_field, reverse) {
        anim_put_bool(vm, listener, field, is_reverse)?;
    }
    Ok(())
}
fn anim_invoke_lifecycle_listener(
    vm: &mut Engine,
    listener: ObjRef,
    method: &str,
    animator: ObjRef,
    reverse: Option<bool>,
) -> Result<(), VmExit> {
    let class = class_of(vm, listener);
    if matches!(
        class.as_str(),
        ANIMATOR_LISTENER | ANIMATOR_PAUSE_LISTENER | ANIMATOR_LISTENER_ADAPTER
    ) {
        return anim_record_listener(vm, listener, method, animator, reverse);
    }
    if let Some(is_reverse) = reverse {
        if vm
            .cp
            .resolve_method(&class, method, "(Landroid/animation/Animator;Z)V")
            .is_some()
        {
            vm.invoke_instance(
                listener,
                method,
                "(Landroid/animation/Animator;Z)V",
                &[Value::Obj(animator), bool_value(is_reverse)],
            )?;
            return Ok(());
        }
    }
    match vm.invoke_instance(
        listener,
        method,
        "(Landroid/animation/Animator;)V",
        &[Value::Obj(animator)],
    ) {
        Ok(_) => Ok(()),
        Err(VmExit::Error(error))
            if error.code == "NOT_FOUND" && vm.cp.is_subtype(&class, ANIMATOR_LISTENER_ADAPTER) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}
fn anim_notify_listener_field(
    vm: &mut Engine,
    animator: ObjRef,
    field: &str,
    method: &str,
    reverse: Option<bool>,
) -> Result<(), VmExit> {
    let listeners = anim_list_items(vm, animator, field);
    for listener in listeners.iter().filter_map(|value| obj(Some(value))) {
        anim_invoke_lifecycle_listener(vm, listener, method, animator, reverse)?;
    }
    Ok(())
}
fn anim_notify_start_listeners(
    vm: &mut Engine,
    animator: ObjRef,
    reverse: bool,
) -> Result<(), VmExit> {
    let was_called = field_bool(vm, animator, "rd_start_listeners_called");
    anim_put_bool(vm, animator, "rd_start_listeners_called", true)?;
    if !was_called && field_obj(vm, animator, "rd_listeners").is_some() {
        anim_notify_listener_field(
            vm,
            animator,
            "rd_listeners",
            "onAnimationStart",
            Some(reverse),
        )?;
    }
    Ok(())
}
fn anim_notify_end_listeners(
    vm: &mut Engine,
    animator: ObjRef,
    reverse: bool,
) -> Result<(), VmExit> {
    let was_called = field_bool(vm, animator, "rd_start_listeners_called");
    anim_put_bool(vm, animator, "rd_start_listeners_called", false)?;
    if was_called && field_obj(vm, animator, "rd_listeners").is_some() {
        anim_notify_listener_field(
            vm,
            animator,
            "rd_listeners",
            "onAnimationEnd",
            Some(reverse),
        )?;
    }
    Ok(())
}
fn anim_notify_update_listeners(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    let listeners = anim_list_items(vm, animator, "rd_update_listeners");
    for listener in listeners.iter().filter_map(|value| obj(Some(value))) {
        if class_of(vm, listener) == ANIMATOR_UPDATE_LISTENER {
            anim_record_listener(vm, listener, "onAnimationUpdate", animator, None)?;
        } else {
            vm.invoke_instance(
                listener,
                "onAnimationUpdate",
                "(Landroid/animation/ValueAnimator;)V",
                &[Value::Obj(animator)],
            )?;
        }
    }
    Ok(())
}

fn anim_long_or(vm: &Engine, r: ObjRef, field: &str, default: i64) -> i64 {
    match get(vm, r, field) {
        Value::Long(v) => v,
        Value::Int(v) => v as i64,
        _ => default,
    }
}
fn anim_float_or(vm: &Engine, r: ObjRef, field: &str, default: f32) -> f32 {
    match get(vm, r, field) {
        Value::Float(v) => v,
        Value::Double(v) => v as f32,
        _ => default,
    }
}
fn anim_duration(vm: &Engine, r: ObjRef) -> i64 {
    anim_long_or(vm, r, "rd_duration", 300)
}
fn anim_resolve_duration_scale(vm: &Engine, r: ObjRef) -> f32 {
    let override_scale = anim_float_or(vm, r, "rd_duration_scale", -1.0);
    if override_scale >= 0.0 {
        override_scale
    } else {
        anim_global_duration_scale(vm)
    }
}
fn anim_scaled_duration(vm: &Engine, r: ObjRef) -> i64 {
    (anim_duration(vm, r) as f32 * anim_resolve_duration_scale(vm, r)) as i64
}
fn anim_values(vm: &Engine, r: ObjRef) -> Vec<ObjRef> {
    collection_items(vm, Some(&get(vm, r, "rd_values")))
        .iter()
        .filter_map(|value| obj(Some(value)))
        .collect()
}
fn anim_set_values(vm: &mut Engine, animator: ObjRef, holders: Vec<ObjRef>) -> Result<(), VmExit> {
    let values = holders.into_iter().map(Value::Obj).collect();
    let array = obj_array(vm, "[Landroid/animation/PropertyValuesHolder;", values)?;
    put(vm, animator, "rd_values", array)?;
    anim_put_bool(vm, animator, "rd_initialized", false)
}
fn anim_init_animator(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    let interpolator = alloc_obj(
        vm,
        "Landroid/view/animation/AccelerateDecelerateInterpolator;",
        Vec::new(),
    )?;
    put(vm, r, "rd_duration", Value::Long(300))?;
    put(vm, r, "rd_start_delay", Value::Long(0))?;
    put(vm, r, "rd_repeat_count", Value::Int(0))?;
    put(vm, r, "rd_repeat_mode", Value::Int(1))?;
    put(vm, r, "rd_duration_scale", Value::Float(-1.0))?;
    put(vm, r, "rd_interpolator", Value::Obj(interpolator))?;
    put(vm, r, "rd_values", Value::Null)?;
    put(vm, r, "rd_listeners", Value::Null)?;
    put(vm, r, "rd_pause_listeners", Value::Null)?;
    put(vm, r, "rd_update_listeners", Value::Null)?;
    put(vm, r, "rd_changing_configurations", Value::Int(0))?;
    put(vm, r, "rd_seek_fraction", Value::Float(-1.0))?;
    put(vm, r, "rd_overall_fraction", Value::Float(0.0))?;
    put(vm, r, "rd_current_fraction", Value::Float(0.0))?;
    put(vm, r, "rd_start_time", Value::Long(-1))?;
    put(vm, r, "rd_last_frame_time", Value::Long(-1))?;
    put(vm, r, "rd_first_frame_time", Value::Long(-1))?;
    put(vm, r, "rd_pause_time", Value::Long(-1))?;
    anim_put_bool(vm, r, "rd_started", false)?;
    anim_put_bool(vm, r, "rd_running", false)?;
    anim_put_bool(vm, r, "rd_paused", false)?;
    anim_put_bool(vm, r, "rd_resumed", false)?;
    anim_put_bool(vm, r, "rd_reversing", false)?;
    anim_put_bool(vm, r, "rd_initialized", false)?;
    anim_put_bool(vm, r, "rd_start_listeners_called", false)?;
    anim_put_bool(vm, r, "rd_animation_end_requested", false)?;
    anim_put_bool(vm, r, "rd_start_time_committed", false)?;
    anim_put_bool(vm, r, "rd_self_pulse", true)?;
    anim_put_bool(vm, r, "rd_suppress_self_pulse", false)?;
    put(vm, r, "rd_target", Value::Null)?;
    put(vm, r, "rd_property_name", Value::Null)?;
    put(vm, r, "rd_property", Value::Null)?;
    anim_put_bool(vm, r, "rd_auto_cancel", false)
}
fn anim_is_object_animator(vm: &Engine, r: ObjRef) -> bool {
    let class = class_of(vm, r);
    class == OBJECT_ANIMATOR || vm.cp.is_subtype(&class, OBJECT_ANIMATOR)
}
fn anim_method_name(prefix: &str, property_name: &str) -> Option<String> {
    let mut chars = property_name.chars();
    let first = chars.next()?;
    let mut out = prefix.to_string();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    Some(out)
}
fn anim_property_get(
    vm: &mut Engine,
    property: ObjRef,
    target: ObjRef,
    kind: i32,
) -> Result<Value, VmExit> {
    let _ = kind;
    // IntProperty/FloatProperty optimize only the setter; their getter is
    // still Property.get(T) returning Integer/Float.
    vm.invoke_instance(
        property,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Obj(target)],
    )
}

fn anim_convert_back(vm: &mut Engine, holder: ObjRef, value: Value) -> Result<Value, VmExit> {
    let converter = get(vm, holder, "rd_converter");
    let Value::Obj(converter_ref) = converter else {
        return Ok(value);
    };
    let kind = field_int(vm, holder, "rd_kind");
    let boxed = anim_boxed(vm, kind, &value)?;
    match vm.invoke_instance(
        converter_ref,
        "convertBack",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[boxed],
    ) {
        Ok(value) => Ok(value),
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {
            let class = class_of(vm, converter_ref)
                .trim_start_matches('L')
                .trim_end_matches(';')
                .replace('/', ".");
            Err(iae(format!(
                "Converter {class} must be a BidirectionalTypeConverter"
            )))
        }
        Err(error) => Err(error),
    }
}
fn anim_target_getter_value(
    vm: &mut Engine,
    holder: ObjRef,
    target: ObjRef,
) -> Result<Option<Value>, VmExit> {
    let Some(property_name) = field_str(vm, holder, "rd_property_name") else {
        return Ok(None);
    };
    let Some(method) = anim_method_name("get", &property_name) else {
        return Ok(None);
    };
    let kind = field_int(vm, holder, "rd_kind");
    let value_type =
        field_str(vm, holder, "rd_value_type").unwrap_or_else(|| "Ljava/lang/Object;".to_string());
    let mut signatures: Vec<String> = match kind {
        ANIM_KIND_INT => vec!["()I".to_string(), "()Ljava/lang/Integer;".to_string()],
        ANIM_KIND_FLOAT => vec!["()F".to_string(), "()Ljava/lang/Float;".to_string()],
        _ => {
            let mut values = Vec::new();
            if value_type.starts_with('L') || value_type.starts_with('[') {
                values.push(format!("(){value_type}"));
            }
            values.push("()Ljava/lang/Object;".to_string());
            values.push("()Ljava/lang/Integer;".to_string());
            values.push("()Ljava/lang/Float;".to_string());
            values.push("()I".to_string());
            values.push("()F".to_string());
            values
        }
    };
    signatures.dedup();
    for sig in signatures {
        match vm.invoke_instance(target, &method, &sig, &[]) {
            Ok(value) => return Ok(Some(value)),
            Err(VmExit::Exception(_)) => return Ok(None),
            Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}
fn anim_pvh_setup_value(
    vm: &mut Engine,
    holder: ObjRef,
    target: ObjRef,
    keyframe: ObjRef,
    mark_set_on_start: bool,
) -> Result<(), VmExit> {
    let kind = field_int(vm, holder, "rd_kind");
    let property = get(vm, holder, "rd_property");
    let value = if let Value::Obj(property_ref) = property {
        match anim_property_get(vm, property_ref, target, kind) {
            Ok(value) => Some(value),
            Err(VmExit::Exception(_)) => None,
            Err(error) => return Err(error),
        }
    } else {
        anim_target_getter_value(vm, holder, target)?
    };
    if let Some(value) = value {
        let value = anim_convert_back(vm, holder, value)?;
        anim_set_keyframe_internal(vm, keyframe, &value, mark_set_on_start)?;
    }
    Ok(())
}
fn anim_pvh_setup_setter_and_getter(
    vm: &mut Engine,
    holder: ObjRef,
    target: ObjRef,
) -> Result<(), VmExit> {
    for keyframe in anim_pvh_keyframes(vm, holder) {
        if !field_bool(vm, keyframe, "rd_has_value")
            || field_bool(vm, keyframe, "rd_value_set_on_start")
        {
            anim_pvh_setup_value(vm, holder, target, keyframe, true)?;
        }
    }
    Ok(())
}
fn anim_try_setter_candidates(
    vm: &mut Engine,
    target: ObjRef,
    method: &str,
    candidates: Vec<(String, Value)>,
) -> Result<bool, VmExit> {
    for (sig, value) in candidates {
        match vm.invoke_instance(target, method, &sig, &[value]) {
            Ok(_) => return Ok(true),
            // Reflection invokes a resolved setter and logs an exception
            // thrown by it; it does not make the animation frame fail.
            Err(VmExit::Exception(_)) => return Ok(true),
            Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}
fn anim_pvh_apply(vm: &mut Engine, holder: ObjRef, target: ObjRef) -> Result<(), VmExit> {
    let kind = field_int(vm, holder, "rd_kind");
    let animated = get(vm, holder, "rd_animated");
    let property = get(vm, holder, "rd_property");
    if let Value::Obj(property_ref) = property {
        let class = class_of(vm, property_ref);
        if kind == ANIM_KIND_INT && vm.cp.is_subtype(&class, ANDROID_INT_PROPERTY) {
            if let Some(value) = anim_number_int(vm, &animated) {
                match vm.invoke_instance(
                    property_ref,
                    "setValue",
                    "(Ljava/lang/Object;I)V",
                    &[Value::Obj(target), Value::Int(value)],
                ) {
                    Ok(_) => return Ok(()),
                    Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
                    Err(error) => return Err(error),
                }
            }
        }
        if kind == ANIM_KIND_FLOAT && vm.cp.is_subtype(&class, ANDROID_FLOAT_PROPERTY) {
            if let Some(value) = anim_number_float(vm, &animated) {
                match vm.invoke_instance(
                    property_ref,
                    "setValue",
                    "(Ljava/lang/Object;F)V",
                    &[Value::Obj(target), Value::Float(value)],
                ) {
                    Ok(_) => return Ok(()),
                    Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let boxed = anim_boxed(vm, kind, &animated)?;
        vm.invoke_instance(
            property_ref,
            "set",
            "(Ljava/lang/Object;Ljava/lang/Object;)V",
            &[Value::Obj(target), boxed],
        )?;
        return Ok(());
    }
    let Some(property_name) = field_str(vm, holder, "rd_property_name") else {
        return Ok(());
    };
    let Some(method) = anim_method_name("set", &property_name) else {
        return Ok(());
    };
    let mut candidates: Vec<(String, Value)> = Vec::new();
    match kind {
        ANIM_KIND_INT => {
            if let Some(value) = anim_number_int(vm, &animated) {
                candidates.push(("(I)V".to_string(), Value::Int(value)));
            }
        }
        ANIM_KIND_FLOAT => {
            if let Some(value) = anim_number_float(vm, &animated) {
                candidates.push(("(F)V".to_string(), Value::Float(value)));
            }
        }
        _ => {
            let boxed = anim_boxed(vm, kind, &animated)?;
            let actual_type = anim_type_desc(vm, &boxed);
            if actual_type.starts_with('L') || actual_type.starts_with('[') {
                candidates.push((format!("({actual_type})V"), boxed.clone()));
            }
            if let Some(value) = anim_number_int(vm, &animated) {
                candidates.push(("(I)V".to_string(), Value::Int(value)));
            }
            if let Some(value) = anim_number_float(vm, &animated) {
                candidates.push(("(F)V".to_string(), Value::Float(value)));
                candidates.push(("(D)V".to_string(), Value::Double(value as f64)));
            }
            candidates.push(("(Ljava/lang/Object;)V".to_string(), boxed));
        }
    }
    candidates.dedup_by(|a, b| a.0 == b.0);
    anim_try_setter_candidates(vm, target, &method, candidates)?;
    Ok(())
}

fn anim_init_animation(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, animator, "rd_initialized") {
        return Ok(());
    }
    if anim_is_object_animator(vm, animator) {
        if let Some(target) = field_obj(vm, animator, "rd_target") {
            for holder in anim_values(vm, animator) {
                anim_pvh_setup_setter_and_getter(vm, holder, target)?;
            }
        }
    }
    anim_put_bool(vm, animator, "rd_initialized", true)
}
fn anim_clamp_fraction(vm: &Engine, animator: ObjRef, fraction: f32) -> f32 {
    if fraction < 0.0 {
        0.0
    } else {
        let repeat = field_int(vm, animator, "rd_repeat_count");
        if repeat == -1 {
            fraction
        } else {
            let max = repeat.wrapping_add(1) as f32;
            if fraction > max {
                max
            } else {
                fraction
            }
        }
    }
}
fn anim_current_iteration(vm: &Engine, animator: ObjRef, fraction: f32) -> i32 {
    let fraction = anim_clamp_fraction(vm, animator, fraction);
    let mut iteration = fraction.floor();
    if fraction == iteration && fraction > 0.0 {
        iteration -= 1.0;
    }
    iteration as i32
}
fn anim_should_play_backward(
    vm: &Engine,
    animator: ObjRef,
    iteration: i32,
    reversing: bool,
) -> bool {
    let repeat = field_int(vm, animator, "rd_repeat_count");
    let mode = field_int(vm, animator, "rd_repeat_mode");
    if iteration > 0 && mode == 2 && (iteration < repeat.wrapping_add(1) || repeat == -1) {
        if reversing {
            iteration % 2 == 0
        } else {
            iteration % 2 != 0
        }
    } else {
        reversing
    }
}
fn anim_current_iteration_fraction(
    vm: &Engine,
    animator: ObjRef,
    fraction: f32,
    reversing: bool,
) -> f32 {
    let fraction = anim_clamp_fraction(vm, animator, fraction);
    let iteration = anim_current_iteration(vm, animator, fraction);
    let current = fraction - iteration as f32;
    if anim_should_play_backward(vm, animator, iteration, reversing) {
        1.0 - current
    } else {
        current
    }
}
fn anim_animate_value(vm: &mut Engine, animator: ObjRef, fraction: f32) -> Result<(), VmExit> {
    if matches!(get(vm, animator, "rd_values"), Value::Null) {
        return Ok(());
    }
    let holders = anim_values(vm, animator);
    let target = if anim_is_object_animator(vm, animator) {
        field_obj(vm, animator, "rd_target")
    } else {
        None
    };
    let mut interpolator = get(vm, animator, "rd_interpolator");
    if !matches!(interpolator, Value::Obj(_)) {
        let default = alloc_obj(
            vm,
            "Landroid/view/animation/AccelerateDecelerateInterpolator;",
            Vec::new(),
        )?;
        interpolator = Value::Obj(default);
        put(vm, animator, "rd_interpolator", interpolator.clone())?;
    }
    let interpolated = anim_apply_interpolator(vm, &interpolator, fraction)?;
    put(
        vm,
        animator,
        "rd_current_fraction",
        Value::Float(interpolated),
    )?;
    for holder in &holders {
        anim_pvh_calculate(vm, *holder, interpolated)?;
    }
    if anim_float_or(vm, animator, "rd_seek_fraction", -1.0) >= 0.0
        || field_bool(vm, animator, "rd_start_listeners_called")
    {
        anim_notify_update_listeners(vm, animator)?;
    }
    if let Some(target) = target {
        for holder in holders {
            anim_pvh_apply(vm, holder, target)?;
        }
    }
    Ok(())
}
fn anim_start_animation(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    anim_put_bool(vm, animator, "rd_animation_end_requested", false)?;
    anim_init_animation(vm, animator)?;
    anim_put_bool(vm, animator, "rd_running", true)?;
    let seek = anim_float_or(vm, animator, "rd_seek_fraction", -1.0);
    let overall = if seek >= 0.0 { seek } else { 0.0 };
    put(vm, animator, "rd_overall_fraction", Value::Float(overall))?;
    let reversing = field_bool(vm, animator, "rd_reversing");
    anim_notify_start_listeners(vm, animator, reversing)
}
fn anim_end_animation(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, animator, "rd_animation_end_requested") {
        return Ok(());
    }
    if field_bool(vm, animator, "rd_self_pulse") {
        anim_unregister_callback(vm, animator);
    }
    anim_put_bool(vm, animator, "rd_animation_end_requested", true)?;
    anim_put_bool(vm, animator, "rd_paused", false)?;
    let notify = (field_bool(vm, animator, "rd_started") || field_bool(vm, animator, "rd_running"))
        && field_obj(vm, animator, "rd_listeners").is_some();
    let reversing = field_bool(vm, animator, "rd_reversing");
    if notify && !field_bool(vm, animator, "rd_running") {
        anim_notify_start_listeners(vm, animator, reversing)?;
    }
    put(vm, animator, "rd_last_frame_time", Value::Long(-1))?;
    put(vm, animator, "rd_first_frame_time", Value::Long(-1))?;
    put(vm, animator, "rd_start_time", Value::Long(-1))?;
    anim_put_bool(vm, animator, "rd_running", false)?;
    anim_put_bool(vm, animator, "rd_started", false)?;
    anim_notify_end_listeners(vm, animator, reversing)?;
    anim_put_bool(vm, animator, "rd_reversing", false)
}
fn anim_animate_based_on_time(
    vm: &mut Engine,
    animator: ObjRef,
    current_time: i64,
) -> Result<bool, VmExit> {
    if !field_bool(vm, animator, "rd_running") {
        return Ok(false);
    }
    let scaled_duration = anim_scaled_duration(vm, animator);
    let start_time = anim_long_or(vm, animator, "rd_start_time", -1);
    let fraction = if scaled_duration > 0 {
        (current_time - start_time) as f32 / scaled_duration as f32
    } else {
        1.0
    };
    let last_fraction = anim_float_or(vm, animator, "rd_overall_fraction", 0.0);
    let repeat = field_int(vm, animator, "rd_repeat_count");
    let new_iteration = fraction as i32 > last_fraction as i32;
    let last_iteration_finished = fraction >= repeat.wrapping_add(1) as f32 && repeat != -1;
    let mut done = false;
    if scaled_duration == 0 {
        done = true;
    } else if new_iteration && !last_iteration_finished {
        anim_notify_listener_field(vm, animator, "rd_listeners", "onAnimationRepeat", None)?;
    } else if last_iteration_finished {
        done = true;
    }
    let overall = anim_clamp_fraction(vm, animator, fraction);
    put(vm, animator, "rd_overall_fraction", Value::Float(overall))?;
    let reversing = field_bool(vm, animator, "rd_reversing");
    let current = anim_current_iteration_fraction(vm, animator, overall, reversing);
    anim_animate_value(vm, animator, current)?;
    Ok(done)
}
fn anim_do_animation_frame(
    vm: &mut Engine,
    animator: ObjRef,
    frame_time: i64,
) -> Result<bool, VmExit> {
    let mut start_time = anim_long_or(vm, animator, "rd_start_time", -1);
    if start_time < 0 {
        start_time = if field_bool(vm, animator, "rd_reversing") {
            frame_time
        } else {
            frame_time
                + (anim_long_or(vm, animator, "rd_start_delay", 0) as f32
                    * anim_resolve_duration_scale(vm, animator)) as i64
        };
        put(vm, animator, "rd_start_time", Value::Long(start_time))?;
    }
    if field_bool(vm, animator, "rd_paused") {
        put(vm, animator, "rd_pause_time", Value::Long(frame_time))?;
        if field_bool(vm, animator, "rd_self_pulse") {
            anim_unregister_callback(vm, animator);
        }
        return Ok(false);
    } else if field_bool(vm, animator, "rd_resumed") {
        anim_put_bool(vm, animator, "rd_resumed", false)?;
        let pause_time = anim_long_or(vm, animator, "rd_pause_time", -1);
        if pause_time > 0 {
            start_time += frame_time - pause_time;
            put(vm, animator, "rd_start_time", Value::Long(start_time))?;
        }
    }
    if !field_bool(vm, animator, "rd_running") {
        let seek = anim_float_or(vm, animator, "rd_seek_fraction", -1.0);
        if start_time > frame_time && seek == -1.0 {
            return Ok(false);
        }
        anim_put_bool(vm, animator, "rd_running", true)?;
        anim_start_animation(vm, animator)?;
    }
    if anim_long_or(vm, animator, "rd_last_frame_time", -1) < 0 {
        let seek = anim_float_or(vm, animator, "rd_seek_fraction", -1.0);
        if seek >= 0.0 {
            let seek_time = (anim_scaled_duration(vm, animator) as f32 * seek) as i64;
            start_time = frame_time - seek_time;
            put(vm, animator, "rd_start_time", Value::Long(start_time))?;
            put(vm, animator, "rd_seek_fraction", Value::Float(-1.0))?;
        }
        anim_put_bool(vm, animator, "rd_start_time_committed", false)?;
    }
    put(vm, animator, "rd_last_frame_time", Value::Long(frame_time))?;
    let current_time = frame_time.max(start_time);
    let finished = anim_animate_based_on_time(vm, animator, current_time)?;
    if finished {
        anim_end_animation(vm, animator)?;
    }
    Ok(finished)
}
fn anim_set_current_fraction(
    vm: &mut Engine,
    animator: ObjRef,
    fraction: f32,
) -> Result<(), VmExit> {
    anim_init_animation(vm, animator)?;
    let fraction = anim_clamp_fraction(vm, animator, fraction);
    anim_put_bool(vm, animator, "rd_start_time_committed", true)?;
    if anim_long_or(vm, animator, "rd_last_frame_time", -1) >= 0 {
        let seek_time = (anim_scaled_duration(vm, animator) as f32 * fraction) as i64;
        let now = vm.fw.clock() as i64;
        put(vm, animator, "rd_start_time", Value::Long(now - seek_time))?;
    } else {
        put(vm, animator, "rd_seek_fraction", Value::Float(fraction))?;
    }
    put(vm, animator, "rd_overall_fraction", Value::Float(fraction))?;
    let reversing = field_bool(vm, animator, "rd_reversing");
    let current = anim_current_iteration_fraction(vm, animator, fraction, reversing);
    anim_animate_value(vm, animator, current)
}
fn anim_start(vm: &mut Engine, animator: ObjRef, backwards: bool) -> Result<(), VmExit> {
    anim_put_bool(vm, animator, "rd_reversing", backwards)?;
    let suppress = field_bool(vm, animator, "rd_suppress_self_pulse");
    anim_put_bool(vm, animator, "rd_self_pulse", !suppress)?;
    let mut seek = anim_float_or(vm, animator, "rd_seek_fraction", -1.0);
    if backwards && seek != -1.0 && seek != 0.0 {
        let repeat = field_int(vm, animator, "rd_repeat_count");
        seek = if repeat == -1 {
            1.0 - (seek - seek.floor())
        } else {
            1.0 + repeat as f32 - seek
        };
        put(vm, animator, "rd_seek_fraction", Value::Float(seek))?;
    }
    anim_put_bool(vm, animator, "rd_started", true)?;
    anim_put_bool(vm, animator, "rd_paused", false)?;
    anim_put_bool(vm, animator, "rd_running", false)?;
    anim_put_bool(vm, animator, "rd_animation_end_requested", false)?;
    put(vm, animator, "rd_last_frame_time", Value::Long(-1))?;
    put(vm, animator, "rd_first_frame_time", Value::Long(-1))?;
    put(vm, animator, "rd_start_time", Value::Long(-1))?;
    if !suppress {
        anim_register_callback(vm, animator, 0)?;
    }
    if anim_long_or(vm, animator, "rd_start_delay", 0) == 0 || seek >= 0.0 || backwards {
        anim_start_animation(vm, animator)?;
        if anim_float_or(vm, animator, "rd_seek_fraction", -1.0) == -1.0 {
            let fraction = if anim_duration(vm, animator) > 0 {
                0.0
            } else {
                1.0
            };
            anim_set_current_fraction(vm, animator, fraction)?;
        } else {
            let seek_now = anim_float_or(vm, animator, "rd_seek_fraction", -1.0);
            anim_set_current_fraction(vm, animator, seek_now)?;
        }
    }
    Ok(())
}
fn anim_cancel(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, animator, "rd_animation_end_requested") {
        return Ok(());
    }
    let should_notify = (field_bool(vm, animator, "rd_started")
        || field_bool(vm, animator, "rd_running")
        || field_bool(vm, animator, "rd_start_listeners_called"))
        && field_obj(vm, animator, "rd_listeners").is_some();
    if should_notify {
        if !field_bool(vm, animator, "rd_running") {
            let reversing = field_bool(vm, animator, "rd_reversing");
            anim_notify_start_listeners(vm, animator, reversing)?;
        }
        anim_notify_listener_field(vm, animator, "rd_listeners", "onAnimationCancel", None)?;
    }
    anim_end_animation(vm, animator)
}
fn anim_end(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    if !field_bool(vm, animator, "rd_running") {
        anim_start_animation(vm, animator)?;
        anim_put_bool(vm, animator, "rd_started", true)?;
    } else if !field_bool(vm, animator, "rd_initialized") {
        anim_init_animation(vm, animator)?;
    }
    let repeat = field_int(vm, animator, "rd_repeat_count");
    let reversing = field_bool(vm, animator, "rd_reversing");
    let fraction = if anim_should_play_backward(vm, animator, repeat, reversing) {
        0.0
    } else {
        1.0
    };
    anim_animate_value(vm, animator, fraction)?;
    anim_end_animation(vm, animator)
}
fn anim_pause(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    let previously_paused = field_bool(vm, animator, "rd_paused");
    if (field_bool(vm, animator, "rd_started")
        || field_bool(vm, animator, "rd_start_listeners_called"))
        && !previously_paused
    {
        anim_put_bool(vm, animator, "rd_paused", true)?;
        anim_notify_listener_field(vm, animator, "rd_pause_listeners", "onAnimationPause", None)?;
    }
    if !previously_paused && field_bool(vm, animator, "rd_paused") {
        put(vm, animator, "rd_pause_time", Value::Long(-1))?;
        anim_put_bool(vm, animator, "rd_resumed", false)?;
    }
    Ok(())
}
fn anim_resume(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, animator, "rd_paused") && !field_bool(vm, animator, "rd_resumed") {
        anim_put_bool(vm, animator, "rd_resumed", true)?;
        if anim_long_or(vm, animator, "rd_pause_time", -1) > 0
            && field_bool(vm, animator, "rd_self_pulse")
        {
            anim_register_callback(vm, animator, 0)?;
        }
    }
    if field_bool(vm, animator, "rd_paused") {
        anim_put_bool(vm, animator, "rd_paused", false)?;
        anim_notify_listener_field(
            vm,
            animator,
            "rd_pause_listeners",
            "onAnimationResume",
            None,
        )?;
    }
    Ok(())
}
fn anim_same_target_and_properties(vm: &Engine, left: ObjRef, right: ObjRef) -> bool {
    if get(vm, left, "rd_target") != get(vm, right, "rd_target") {
        return false;
    }
    let left_values = anim_values(vm, left);
    let right_values = anim_values(vm, right);
    if left_values.len() != right_values.len() {
        return false;
    }
    left_values.iter().zip(right_values.iter()).all(|(a, b)| {
        let left_name = field_str(vm, *a, "rd_property_name");
        left_name.is_some() && left_name == field_str(vm, *b, "rd_property_name")
    })
}
fn anim_auto_cancel_based_on(vm: &mut Engine, animator: ObjRef) -> Result<(), VmExit> {
    let callbacks: Vec<ObjRef> = vm
        .frames
        .animation_callbacks
        .iter()
        .map(|(callback, _)| *callback)
        .collect();
    for callback in callbacks.into_iter().rev() {
        if callback != animator
            && anim_is_object_animator(vm, callback)
            && field_bool(vm, callback, "rd_auto_cancel")
            && anim_same_target_and_properties(vm, animator, callback)
        {
            vm.invoke_instance(callback, "cancel", "()V", &[])?;
        }
    }
    Ok(())
}
fn anim_clone_animator(vm: &mut Engine, animator: ObjRef) -> Result<ObjRef, VmExit> {
    let class = class_of(vm, animator);
    let clone = alloc_obj(vm, &class, Vec::new())?;
    anim_init_animator(vm, clone)?;
    for field in [
        "rd_duration",
        "rd_start_delay",
        "rd_repeat_count",
        "rd_repeat_mode",
        "rd_duration_scale",
        "rd_interpolator",
        "rd_changing_configurations",
        "rd_target",
        "rd_property_name",
        "rd_property",
        "rd_auto_cancel",
    ] {
        let value = get(vm, animator, field);
        put(vm, clone, field, value)?;
    }
    anim_copy_list_field(vm, animator, clone, "rd_listeners")?;
    anim_copy_list_field(vm, animator, clone, "rd_pause_listeners")?;
    anim_copy_list_field(vm, animator, clone, "rd_update_listeners")?;
    if matches!(get(vm, animator, "rd_values"), Value::Null) {
        put(vm, clone, "rd_values", Value::Null)?;
    } else {
        let mut holders = Vec::new();
        for holder in anim_values(vm, animator) {
            holders.push(anim_clone_pvh(vm, holder)?);
        }
        anim_set_values(vm, clone, holders)?;
    }
    put(vm, clone, "rd_seek_fraction", Value::Float(-1.0))?;
    put(vm, clone, "rd_overall_fraction", Value::Float(0.0))?;
    put(vm, clone, "rd_current_fraction", Value::Float(0.0))?;
    put(vm, clone, "rd_start_time", Value::Long(-1))?;
    put(vm, clone, "rd_last_frame_time", Value::Long(-1))?;
    put(vm, clone, "rd_first_frame_time", Value::Long(-1))?;
    put(vm, clone, "rd_pause_time", Value::Long(-1))?;
    anim_put_bool(vm, clone, "rd_reversing", false)?;
    anim_put_bool(vm, clone, "rd_initialized", false)?;
    anim_put_bool(vm, clone, "rd_started", false)?;
    anim_put_bool(vm, clone, "rd_running", false)?;
    anim_put_bool(vm, clone, "rd_paused", false)?;
    anim_put_bool(vm, clone, "rd_resumed", false)?;
    anim_put_bool(vm, clone, "rd_animation_end_requested", false)?;
    anim_put_bool(vm, clone, "rd_start_time_committed", false)?;
    anim_put_bool(vm, clone, "rd_start_listeners_called", false)?;
    anim_put_bool(vm, clone, "rd_self_pulse", true)?;
    anim_put_bool(vm, clone, "rd_suppress_self_pulse", false)?;
    Ok(clone)
}

fn anim_new_animator(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    let animator = alloc_obj(vm, class, Vec::new())?;
    anim_init_animator(vm, animator)?;
    Ok(animator)
}
fn anim_make_int_pvh(
    vm: &mut Engine,
    property_name: Option<String>,
    property: Value,
    values: &[i32],
) -> Result<ObjRef, VmExit> {
    let property_name = if property_name.is_none() {
        anim_property_name(vm, &property)
    } else {
        property_name
    };
    let keyframes = anim_keyframes_from_ints(vm, values)?;
    anim_new_pvh(
        vm,
        property_name,
        property,
        ANIM_KIND_INT,
        keyframes,
        Value::Null,
        Value::Null,
        "I",
        false,
    )
}
fn anim_make_float_pvh(
    vm: &mut Engine,
    property_name: Option<String>,
    property: Value,
    values: &[f32],
) -> Result<ObjRef, VmExit> {
    let property_name = if property_name.is_none() {
        anim_property_name(vm, &property)
    } else {
        property_name
    };
    let keyframes = anim_keyframes_from_floats(vm, values)?;
    anim_new_pvh(
        vm,
        property_name,
        property,
        ANIM_KIND_FLOAT,
        keyframes,
        Value::Null,
        Value::Null,
        "F",
        false,
    )
}
fn anim_make_object_pvh(
    vm: &mut Engine,
    property_name: Option<String>,
    property: Value,
    evaluator: Value,
    converter: Value,
    values: &[Value],
) -> Result<ObjRef, VmExit> {
    let property_name = if property_name.is_none() {
        anim_property_name(vm, &property)
    } else {
        property_name
    };
    let holder = anim_new_pvh(
        vm,
        property_name,
        property,
        ANIM_KIND_OBJECT,
        Vec::new(),
        evaluator,
        converter,
        "Ljava/lang/Object;",
        false,
    )?;
    anim_pvh_set_object_values(vm, holder, values)?;
    Ok(holder)
}
fn anim_make_keyframe_pvh(
    vm: &mut Engine,
    property_name: Option<String>,
    property: Value,
    keyframes: Vec<ObjRef>,
) -> Result<ObjRef, VmExit> {
    let property_name = if property_name.is_none() {
        anim_property_name(vm, &property)
    } else {
        property_name
    };
    let kind = anim_pvh_kind_from_keyframes(vm, &keyframes);
    let value_type = keyframes
        .first()
        .and_then(|kf| field_str(vm, *kf, "rd_value_type"))
        .unwrap_or_else(|| "Ljava/lang/Object;".to_string());
    anim_new_pvh(
        vm,
        property_name,
        property,
        kind,
        keyframes,
        Value::Null,
        Value::Null,
        &value_type,
        false,
    )
}
fn anim_animator_set_int_values(
    vm: &mut Engine,
    animator: ObjRef,
    values: &[i32],
    object_animator: bool,
) -> Result<(), VmExit> {
    if values.is_empty() {
        return Ok(());
    }
    let holders = anim_values(vm, animator);
    if holders.is_empty() {
        let holder = if object_animator {
            let property = get(vm, animator, "rd_property");
            if matches!(property, Value::Obj(_)) {
                anim_make_int_pvh(vm, None, property, values)?
            } else {
                anim_make_int_pvh(
                    vm,
                    field_str(vm, animator, "rd_property_name"),
                    Value::Null,
                    values,
                )?
            }
        } else {
            anim_make_int_pvh(vm, Some(String::new()), Value::Null, values)?
        };
        anim_set_values(vm, animator, vec![holder])
    } else {
        anim_pvh_set_int_values(vm, holders[0], values)?;
        anim_put_bool(vm, animator, "rd_initialized", false)
    }
}
fn anim_animator_set_float_values(
    vm: &mut Engine,
    animator: ObjRef,
    values: &[f32],
    object_animator: bool,
) -> Result<(), VmExit> {
    if values.is_empty() {
        return Ok(());
    }
    let holders = anim_values(vm, animator);
    if holders.is_empty() {
        let holder = if object_animator {
            let property = get(vm, animator, "rd_property");
            if matches!(property, Value::Obj(_)) {
                anim_make_float_pvh(vm, None, property, values)?
            } else {
                anim_make_float_pvh(
                    vm,
                    field_str(vm, animator, "rd_property_name"),
                    Value::Null,
                    values,
                )?
            }
        } else {
            anim_make_float_pvh(vm, Some(String::new()), Value::Null, values)?
        };
        anim_set_values(vm, animator, vec![holder])
    } else {
        anim_pvh_set_float_values(vm, holders[0], values)?;
        anim_put_bool(vm, animator, "rd_initialized", false)
    }
}
fn anim_animator_set_object_values(
    vm: &mut Engine,
    animator: ObjRef,
    values: &[Value],
    object_animator: bool,
) -> Result<(), VmExit> {
    if values.is_empty() {
        return Ok(());
    }
    let holders = anim_values(vm, animator);
    if holders.is_empty() {
        let holder = if object_animator {
            let property = get(vm, animator, "rd_property");
            if matches!(property, Value::Obj(_)) {
                anim_make_object_pvh(vm, None, property, Value::Null, Value::Null, values)?
            } else {
                anim_make_object_pvh(
                    vm,
                    field_str(vm, animator, "rd_property_name"),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    values,
                )?
            }
        } else {
            anim_make_object_pvh(
                vm,
                Some(String::new()),
                Value::Null,
                Value::Null,
                Value::Null,
                values,
            )?
        };
        anim_set_values(vm, animator, vec![holder])
    } else {
        anim_pvh_set_object_values(vm, holders[0], values)?;
        anim_put_bool(vm, animator, "rd_initialized", false)
    }
}
fn anim_animator_set_evaluator(
    vm: &mut Engine,
    animator: ObjRef,
    evaluator: Value,
) -> Result<(), VmExit> {
    if matches!(evaluator, Value::Obj(_)) {
        if let Some(holder) = anim_values(vm, animator).first().copied() {
            put(vm, holder, "rd_evaluator", evaluator)?;
        }
    }
    Ok(())
}
fn argb_evaluator_instance(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (ARGB_EVALUATOR.to_string(), "sInstance".to_string());
    if let Some(value) = vm.statics.get(&key) {
        return Ok(value.clone());
    }
    let evaluator = alloc_obj(vm, ARGB_EVALUATOR, Vec::new())?;
    let value = Value::Obj(evaluator);
    vm.statics.insert(key, value.clone());
    Ok(value)
}
fn anim_duration_scale_listeners(vm: &Engine) -> Vec<Value> {
    match vm.statics.get(&(
        VALUE_ANIMATOR.to_string(),
        "rd_duration_scale_listeners".to_string(),
    )) {
        Some(value) => collection_items(vm, Some(value)),
        None => Vec::new(),
    }
}
fn anim_set_duration_scale_listeners(vm: &mut Engine, listeners: Vec<Value>) -> Result<(), VmExit> {
    let key = (
        VALUE_ANIMATOR.to_string(),
        "rd_duration_scale_listeners".to_string(),
    );
    if let Some(Value::Obj(list)) = vm.statics.get(&key).cloned() {
        if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
            *elems = listeners;
            return Ok(());
        }
    }
    let list = new_collection(vm, "Ljava/util/ArrayList;", listeners)?;
    vm.statics.insert(key, Value::Obj(list));
    Ok(())
}

fn value_animator_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("ofInt" | "ofArgb", "([I)Landroid/animation/ValueAnimator;") => {
            let animator = anim_new_animator(vm, VALUE_ANIMATOR)?;
            let values = int_values(vm, args.first());
            anim_animator_set_int_values(vm, animator, &values, false)?;
            if name == "ofArgb" {
                let evaluator = argb_evaluator_instance(vm)?;
                anim_animator_set_evaluator(vm, animator, evaluator)?;
            }
            Value::Obj(animator)
        }
        ("ofFloat", "([F)Landroid/animation/ValueAnimator;") => {
            let animator = anim_new_animator(vm, VALUE_ANIMATOR)?;
            let values = float_values(vm, args.first());
            anim_animator_set_float_values(vm, animator, &values, false)?;
            Value::Obj(animator)
        }
        (
            "ofPropertyValuesHolder",
            "([Landroid/animation/PropertyValuesHolder;)Landroid/animation/ValueAnimator;",
        ) => {
            let animator = anim_new_animator(vm, VALUE_ANIMATOR)?;
            let values = args.first().cloned().unwrap_or(Value::Null);
            put(vm, animator, "rd_values", values)?;
            anim_put_bool(vm, animator, "rd_initialized", false)?;
            Value::Obj(animator)
        }
        (
            "ofObject",
            "(Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/ValueAnimator;",
        ) => {
            let animator = anim_new_animator(vm, VALUE_ANIMATOR)?;
            let values = array_elems(vm, args.get(1));
            anim_animator_set_object_values(vm, animator, &values, false)?;
            let evaluator = args.first().cloned().unwrap_or(Value::Null);
            anim_animator_set_evaluator(vm, animator, evaluator)?;
            Value::Obj(animator)
        }
        ("setDurationScale", "(F)V") => {
            let scale = farg(args, 0);
            anim_set_global_duration_scale(vm, scale);
            for listener in anim_duration_scale_listeners(vm)
                .iter()
                .filter_map(|value| obj(Some(value)))
            {
                if class_of(vm, listener) == DURATION_SCALE_LISTENER {
                    let count = field_int(vm, listener, "rd_on_changed_count").wrapping_add(1);
                    put(vm, listener, "rd_on_changed_count", Value::Int(count))?;
                    put(
                        vm,
                        listener,
                        "rd_last_duration_scale",
                        Value::Float(scale),
                    )?;
                } else {
                    vm.invoke_instance(listener, "onChanged", "(F)V", &[Value::Float(scale)])?;
                }
            }
            Value::Null
        }
        ("getDurationScale", "()F") => Value::Float(anim_global_duration_scale(vm)),
        (
            "registerDurationScaleChangeListener",
            "(Landroid/animation/ValueAnimator$DurationScaleChangeListener;)Z",
        ) => {
            let listener = args.first().cloned().unwrap_or(Value::Null);
            let mut listeners = anim_duration_scale_listeners(vm);
            if listeners.contains(&listener) {
                Value::Int(0)
            } else {
                listeners.push(listener);
                anim_set_duration_scale_listeners(vm, listeners)?;
                Value::Int(1)
            }
        }
        (
            "unregisterDurationScaleChangeListener",
            "(Landroid/animation/ValueAnimator$DurationScaleChangeListener;)Z",
        ) => {
            let listener = args.first().cloned().unwrap_or(Value::Null);
            let mut listeners = anim_duration_scale_listeners(vm);
            let before = listeners.len();
            listeners.retain(|value| value != &listener);
            let removed = listeners.len() != before;
            anim_set_duration_scale_listeners(vm, listeners)?;
            bool_value(removed)
        }
        ("areAnimatorsEnabled", "()Z") => bool_value(anim_global_duration_scale(vm) != 0.0),
        ("getFrameDelay", "()J") => Value::Long(anim_frame_delay(vm)),
        ("setFrameDelay", "(J)V") => {
            anim_set_frame_delay(vm, larg(args, 0));
            Value::Null
        }
        ("getCurrentAnimationsCount", "()I") => {
            Value::Int(vm.frames.animation_callbacks.len() as i32)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn anim_new_object_animator(
    vm: &mut Engine,
    target: Value,
    property_name: Option<String>,
    property: Value,
) -> Result<ObjRef, VmExit> {
    let animator = anim_new_animator(vm, OBJECT_ANIMATOR)?;
    put(vm, animator, "rd_target", target)?;
    put(vm, animator, "rd_property", property)?;
    put_opt_str(vm, animator, "rd_property_name", property_name)?;
    Ok(animator)
}
fn object_animator_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if matches!(
        name,
        "setDurationScale"
            | "getDurationScale"
            | "registerDurationScaleChangeListener"
            | "unregisterDurationScaleChangeListener"
            | "areAnimatorsEnabled"
            | "getFrameDelay"
            | "setFrameDelay"
            | "getCurrentAnimationsCount"
    ) {
        return value_animator_static(vm, name, sig, args);
    }
    let value = match (name, sig) {
        (
            "ofInt" | "ofArgb",
            "(Ljava/lang/Object;Ljava/lang/String;[I)Landroid/animation/ObjectAnimator;",
        )
        | (
            "ofInt" | "ofArgb",
            "(Ljava/lang/Object;Landroid/util/Property;[I)Landroid/animation/ObjectAnimator;",
        ) => {
            let by_property = sig.contains("Landroid/util/Property;");
            let animator = if by_property {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    None,
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?
            } else {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    opt_str_arg(vm, args, 1),
                    Value::Null,
                )?
            };
            let values = int_values(vm, args.get(2));
            anim_animator_set_int_values(vm, animator, &values, true)?;
            if name == "ofArgb" {
                let evaluator = argb_evaluator_instance(vm)?;
                anim_animator_set_evaluator(vm, animator, evaluator)?;
            }
            Value::Obj(animator)
        }
        (
            "ofFloat",
            "(Ljava/lang/Object;Ljava/lang/String;[F)Landroid/animation/ObjectAnimator;",
        )
        | (
            "ofFloat",
            "(Ljava/lang/Object;Landroid/util/Property;[F)Landroid/animation/ObjectAnimator;",
        ) => {
            let by_property = sig.contains("Landroid/util/Property;");
            let animator = if by_property {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    None,
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?
            } else {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    opt_str_arg(vm, args, 1),
                    Value::Null,
                )?
            };
            let values = float_values(vm, args.get(2));
            anim_animator_set_float_values(vm, animator, &values, true)?;
            Value::Obj(animator)
        }
        (
            "ofObject",
            "(Ljava/lang/Object;Ljava/lang/String;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/ObjectAnimator;",
        )
        | (
            "ofObject",
            "(Ljava/lang/Object;Landroid/util/Property;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/ObjectAnimator;",
        ) => {
            let by_property = sig.contains("Landroid/util/Property;");
            let animator = if by_property {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    None,
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?
            } else {
                anim_new_object_animator(
                    vm,
                    args.first().cloned().unwrap_or(Value::Null),
                    opt_str_arg(vm, args, 1),
                    Value::Null,
                )?
            };
            let values = array_elems(vm, args.get(3));
            anim_animator_set_object_values(vm, animator, &values, true)?;
            let evaluator = args.get(2).cloned().unwrap_or(Value::Null);
            anim_animator_set_evaluator(vm, animator, evaluator)?;
            Value::Obj(animator)
        }
        (
            "ofObject",
            "(Ljava/lang/Object;Landroid/util/Property;Landroid/animation/TypeConverter;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/ObjectAnimator;",
        ) => {
            let animator = anim_new_animator(vm, OBJECT_ANIMATOR)?;
            put(
                vm,
                animator,
                "rd_target",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            let values = array_elems(vm, args.get(4));
            let holder = anim_make_object_pvh(
                vm,
                None,
                args.get(1).cloned().unwrap_or(Value::Null),
                args.get(3).cloned().unwrap_or(Value::Null),
                args.get(2).cloned().unwrap_or(Value::Null),
                &values,
            )?;
            anim_set_values(vm, animator, vec![holder])?;
            Value::Obj(animator)
        }
        (
            "ofPropertyValuesHolder",
            "(Ljava/lang/Object;[Landroid/animation/PropertyValuesHolder;)Landroid/animation/ObjectAnimator;",
        ) => {
            let animator = anim_new_animator(vm, OBJECT_ANIMATOR)?;
            put(
                vm,
                animator,
                "rd_target",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                animator,
                "rd_values",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            anim_put_bool(vm, animator, "rd_initialized", false)?;
            Value::Obj(animator)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn property_values_holder_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        (
            "ofInt",
            "(Ljava/lang/String;[I)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = int_values(vm, args.get(1));
            Value::Obj(anim_make_int_pvh(
                vm,
                opt_str_arg(vm, args, 0),
                Value::Null,
                &values,
            )?)
        }
        (
            "ofInt",
            "(Landroid/util/Property;[I)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = int_values(vm, args.get(1));
            Value::Obj(anim_make_int_pvh(
                vm,
                None,
                args.first().cloned().unwrap_or(Value::Null),
                &values,
            )?)
        }
        (
            "ofFloat",
            "(Ljava/lang/String;[F)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = float_values(vm, args.get(1));
            Value::Obj(anim_make_float_pvh(
                vm,
                opt_str_arg(vm, args, 0),
                Value::Null,
                &values,
            )?)
        }
        (
            "ofFloat",
            "(Landroid/util/Property;[F)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = float_values(vm, args.get(1));
            Value::Obj(anim_make_float_pvh(
                vm,
                None,
                args.first().cloned().unwrap_or(Value::Null),
                &values,
            )?)
        }
        (
            "ofObject",
            "(Ljava/lang/String;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = array_elems(vm, args.get(2));
            Value::Obj(anim_make_object_pvh(
                vm,
                opt_str_arg(vm, args, 0),
                Value::Null,
                args.get(1).cloned().unwrap_or(Value::Null),
                Value::Null,
                &values,
            )?)
        }
        (
            "ofObject",
            "(Landroid/util/Property;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = array_elems(vm, args.get(2));
            Value::Obj(anim_make_object_pvh(
                vm,
                None,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
                Value::Null,
                &values,
            )?)
        }
        (
            "ofObject",
            "(Landroid/util/Property;Landroid/animation/TypeConverter;Landroid/animation/TypeEvaluator;[Ljava/lang/Object;)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let values = array_elems(vm, args.get(3));
            Value::Obj(anim_make_object_pvh(
                vm,
                None,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(2).cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
                &values,
            )?)
        }
        (
            "ofKeyframe",
            "(Ljava/lang/String;[Landroid/animation/Keyframe;)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let keyframes = array_elems(vm, args.get(1))
                .iter()
                .filter_map(|value| obj(Some(value)))
                .collect();
            Value::Obj(anim_make_keyframe_pvh(
                vm,
                opt_str_arg(vm, args, 0),
                Value::Null,
                keyframes,
            )?)
        }
        (
            "ofKeyframe",
            "(Landroid/util/Property;[Landroid/animation/Keyframe;)Landroid/animation/PropertyValuesHolder;",
        ) => {
            let keyframes = array_elems(vm, args.get(1))
                .iter()
                .filter_map(|value| obj(Some(value)))
                .collect();
            Value::Obj(anim_make_keyframe_pvh(
                vm,
                None,
                args.first().cloned().unwrap_or(Value::Null),
                keyframes,
            )?)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn keyframe_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("ofInt", "(FI)Landroid/animation/Keyframe;") => Value::Obj(anim_new_keyframe(
            vm,
            ANIM_KIND_INT,
            farg(args, 0),
            Value::Int(iarg(args, 1)),
            true,
            "I",
        )?),
        ("ofInt", "(F)Landroid/animation/Keyframe;") => Value::Obj(anim_new_keyframe(
            vm,
            ANIM_KIND_INT,
            farg(args, 0),
            Value::Int(0),
            false,
            "I",
        )?),
        ("ofFloat", "(FF)Landroid/animation/Keyframe;") => Value::Obj(anim_new_keyframe(
            vm,
            ANIM_KIND_FLOAT,
            farg(args, 0),
            Value::Float(farg(args, 1)),
            true,
            "F",
        )?),
        ("ofFloat", "(F)Landroid/animation/Keyframe;") => Value::Obj(anim_new_keyframe(
            vm,
            ANIM_KIND_FLOAT,
            farg(args, 0),
            Value::Float(0.0),
            false,
            "F",
        )?),
        ("ofObject", "(FLjava/lang/Object;)Landroid/animation/Keyframe;") => {
            let object_value = args.get(1).cloned().unwrap_or(Value::Null);
            let value_type = anim_type_desc(vm, &object_value);
            Value::Obj(anim_new_keyframe(
                vm,
                ANIM_KIND_OBJECT,
                farg(args, 0),
                object_value.clone(),
                !matches!(object_value, Value::Null),
                &value_type,
            )?)
        }
        ("ofObject", "(F)Landroid/animation/Keyframe;") => Value::Obj(anim_new_keyframe(
            vm,
            ANIM_KIND_OBJECT,
            farg(args, 0),
            Value::Null,
            false,
            "Ljava/lang/Object;",
        )?),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn animation_handler_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("getInstance", "()Landroid/animation/AnimationHandler;") => {
            Value::Obj(anim_handler_instance(vm)?)
        }
        ("getAnimationCount", "()I") => Value::Int(vm.frames.animation_callbacks.len() as i32),
        ("getFrameDelay", "()J") => Value::Long(anim_frame_delay(vm)),
        ("setFrameDelay", "(J)V") => {
            anim_set_frame_delay(vm, larg(args, 0));
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn property_values_holder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "(Ljava/lang/String;)V") => {
            put_opt_str(vm, recv, "rd_property_name", opt_str_arg(vm, args, 0))?;
            put(vm, recv, "rd_property", Value::Null)?;
            put(vm, recv, "rd_kind", Value::Int(ANIM_KIND_OBJECT))?;
            put(vm, recv, "rd_evaluator", Value::Null)?;
            put(vm, recv, "rd_converter", Value::Null)?;
            let type_name = jstr(vm, "Ljava/lang/Object;")?;
            put(vm, recv, "rd_value_type", type_name)?;
            put(vm, recv, "rd_animated", Value::Null)?;
            anim_set_list_items(vm, recv, "rd_keyframes", Vec::new())?;
            Value::Null
        }
        ("<init>", "(Landroid/util/Property;)V") => {
            let property = args.first().cloned().unwrap_or(Value::Null);
            let property_name = anim_property_name(vm, &property);
            put_opt_str(vm, recv, "rd_property_name", property_name)?;
            put(vm, recv, "rd_property", property)?;
            put(vm, recv, "rd_kind", Value::Int(ANIM_KIND_OBJECT))?;
            put(vm, recv, "rd_evaluator", Value::Null)?;
            put(vm, recv, "rd_converter", Value::Null)?;
            let type_name = jstr(vm, "Ljava/lang/Object;")?;
            put(vm, recv, "rd_value_type", type_name)?;
            put(vm, recv, "rd_animated", Value::Null)?;
            anim_set_list_items(vm, recv, "rd_keyframes", Vec::new())?;
            Value::Null
        }
        ("setIntValues", "([I)V") => {
            let values = int_values(vm, args.first());
            anim_pvh_set_int_values(vm, recv, &values)?;
            Value::Null
        }
        ("setFloatValues", "([F)V") => {
            let values = float_values(vm, args.first());
            anim_pvh_set_float_values(vm, recv, &values)?;
            Value::Null
        }
        ("setObjectValues", "([Ljava/lang/Object;)V") => {
            let values = array_elems(vm, args.first());
            anim_pvh_set_object_values(vm, recv, &values)?;
            Value::Null
        }
        ("setKeyframes", "([Landroid/animation/Keyframe;)V") => {
            let keyframes: Vec<ObjRef> = array_elems(vm, args.first())
                .iter()
                .filter_map(|value| obj(Some(value)))
                .collect();
            let first = keyframes.first().copied().ok_or_else(|| oob("0"))?;
            let value_type = field_str(vm, first, "rd_value_type")
                .unwrap_or_else(|| "Ljava/lang/Object;".to_string());
            let kind = anim_pvh_kind_from_keyframes(vm, &keyframes);
            anim_pvh_replace_keyframes(vm, recv, kind, keyframes, &value_type, true)?;
            Value::Null
        }
        ("setEvaluator", "(Landroid/animation/TypeEvaluator;)V") => {
            put(
                vm,
                recv,
                "rd_evaluator",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setConverter", "(Landroid/animation/TypeConverter;)V") => {
            put(
                vm,
                recv,
                "rd_converter",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setPropertyName", "(Ljava/lang/String;)V") => {
            put_opt_str(vm, recv, "rd_property_name", opt_str_arg(vm, args, 0))?;
            Value::Null
        }
        ("getPropertyName", "()Ljava/lang/String;") => get(vm, recv, "rd_property_name"),
        ("setProperty", "(Landroid/util/Property;)V") => {
            put(
                vm,
                recv,
                "rd_property",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getValueType", "()Ljava/lang/Class;") => {
            let desc = field_str(vm, recv, "rd_value_type")
                .unwrap_or_else(|| "Ljava/lang/Object;".to_string());
            anim_class_value(vm, &desc)?
        }
        ("clone", "()Landroid/animation/PropertyValuesHolder;")
        | ("clone", "()Ljava/lang/Object;") => Value::Obj(anim_clone_pvh(vm, recv)?),
        ("toString", "()Ljava/lang/String;") => {
            let property_name =
                field_str(vm, recv, "rd_property_name").unwrap_or_else(|| "null".to_string());
            let mut out = format!("{property_name}: ");
            for keyframe in anim_pvh_keyframes(vm, recv) {
                let internal = anim_keyframe_internal(vm, keyframe);
                let boxed = anim_boxed(vm, anim_keyframe_kind(vm, keyframe), &internal)?;
                out.push_str(&value_string(vm, &boxed)?);
                out.push_str("  ");
            }
            jstr(vm, out)?
        }
        ("calculateValue", "(F)V") => {
            anim_pvh_calculate(vm, recv, farg(args, 0))?;
            Value::Null
        }
        ("getAnimatedValue", "()Ljava/lang/Object;") => anim_pvh_animated_value(vm, recv)?,
        ("setAnimatedValue", "(Ljava/lang/Object;)V") => {
            if let Some(target) = oarg(args, 0) {
                anim_pvh_apply(vm, recv, target)?;
            }
            Value::Null
        }
        ("setupSetterAndGetter", "(Ljava/lang/Object;)V") => {
            let target = req_obj(args, 0, "target")?;
            anim_pvh_setup_setter_and_getter(vm, recv, target)?;
            Value::Null
        }
        ("setupStartValue", "(Ljava/lang/Object;)V") => {
            let target = req_obj(args, 0, "target")?;
            if let Some(keyframe) = anim_pvh_keyframes(vm, recv).first().copied() {
                anim_pvh_setup_value(vm, recv, target, keyframe, false)?;
            }
            Value::Null
        }
        ("setupEndValue", "(Ljava/lang/Object;)V") => {
            let target = req_obj(args, 0, "target")?;
            if let Some(keyframe) = anim_pvh_keyframes(vm, recv).last().copied() {
                anim_pvh_setup_value(vm, recv, target, keyframe, false)?;
            }
            Value::Null
        }
        ("init", "()V") => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn keyframe_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "(FI)V") if class == KEYFRAME_INT => {
            put(vm, recv, "rd_kind", Value::Int(ANIM_KIND_INT))?;
            put(vm, recv, "rd_fraction", Value::Float(farg(args, 0)))?;
            put(vm, recv, "rd_value", Value::Int(iarg(args, 1)))?;
            anim_put_bool(vm, recv, "rd_has_value", true)?;
            let type_name = jstr(vm, "I")?;
            put(vm, recv, "rd_value_type", type_name)?;
            Value::Null
        }
        ("<init>", "(FF)V") if class == KEYFRAME_FLOAT => {
            put(vm, recv, "rd_kind", Value::Int(ANIM_KIND_FLOAT))?;
            put(vm, recv, "rd_fraction", Value::Float(farg(args, 0)))?;
            put(vm, recv, "rd_value", Value::Float(farg(args, 1)))?;
            anim_put_bool(vm, recv, "rd_has_value", true)?;
            let type_name = jstr(vm, "F")?;
            put(vm, recv, "rd_value_type", type_name)?;
            Value::Null
        }
        ("<init>", "(FLjava/lang/Object;)V") if class == KEYFRAME_OBJECT => {
            let object_value = args.get(1).cloned().unwrap_or(Value::Null);
            let value_type = anim_type_desc(vm, &object_value);
            put(vm, recv, "rd_kind", Value::Int(ANIM_KIND_OBJECT))?;
            put(vm, recv, "rd_fraction", Value::Float(farg(args, 0)))?;
            anim_put_bool(
                vm,
                recv,
                "rd_has_value",
                !matches!(object_value, Value::Null),
            )?;
            put(vm, recv, "rd_value", object_value)?;
            let type_name = jstr(vm, value_type)?;
            put(vm, recv, "rd_value_type", type_name)?;
            Value::Null
        }
        ("<init>", "(F)V") => {
            let kind = match class {
                KEYFRAME_INT => ANIM_KIND_INT,
                KEYFRAME_FLOAT => ANIM_KIND_FLOAT,
                _ => ANIM_KIND_OBJECT,
            };
            put(vm, recv, "rd_kind", Value::Int(kind))?;
            put(vm, recv, "rd_fraction", Value::Float(farg(args, 0)))?;
            anim_put_bool(vm, recv, "rd_has_value", false)?;
            Value::Null
        }
        ("hasValue", "()Z") => bool_value(field_bool(vm, recv, "rd_has_value")),
        ("valueWasSetOnStart", "()Z") => bool_value(field_bool(vm, recv, "rd_value_set_on_start")),
        ("setValueWasSetOnStart", "(Z)V") => {
            anim_put_bool(vm, recv, "rd_value_set_on_start", iarg(args, 0) != 0)?;
            Value::Null
        }
        ("getValue", "()Ljava/lang/Object;") => {
            let internal = anim_keyframe_internal(vm, recv);
            anim_boxed(vm, anim_keyframe_kind(vm, recv), &internal)?
        }
        ("setValue", "(Ljava/lang/Object;)V") => {
            let new_value = args.first().cloned().unwrap_or(Value::Null);
            match anim_keyframe_kind(vm, recv) {
                ANIM_KIND_INT => {
                    let valid = matches!(&new_value, Value::Int(_))
                        || matches!(&new_value, Value::Obj(r) if class_of(vm, *r) == "Ljava/lang/Integer;");
                    if valid {
                        if let Some(v) = anim_number_int(vm, &new_value) {
                            put(vm, recv, "rd_value", Value::Int(v))?;
                            anim_put_bool(vm, recv, "rd_has_value", true)?;
                        }
                    }
                }
                ANIM_KIND_FLOAT => {
                    let valid = matches!(&new_value, Value::Float(_))
                        || matches!(&new_value, Value::Obj(r) if class_of(vm, *r) == "Ljava/lang/Float;");
                    if valid {
                        if let Some(v) = anim_number_float(vm, &new_value) {
                            put(vm, recv, "rd_value", Value::Float(v))?;
                            anim_put_bool(vm, recv, "rd_has_value", true)?;
                        }
                    }
                }
                _ => {
                    anim_put_bool(vm, recv, "rd_has_value", !matches!(new_value, Value::Null))?;
                    put(vm, recv, "rd_value", new_value)?;
                }
            }
            Value::Null
        }
        ("getFraction", "()F") => Value::Float(field_float(vm, recv, "rd_fraction")),
        ("setFraction", "(F)V") => {
            put(vm, recv, "rd_fraction", Value::Float(farg(args, 0)))?;
            Value::Null
        }
        ("getInterpolator", "()Landroid/view/animation/Interpolator;")
        | ("getInterpolator", "()Landroid/animation/TimeInterpolator;") => {
            get(vm, recv, "rd_interpolator")
        }
        ("setInterpolator", "(Landroid/view/animation/Interpolator;)V")
        | ("setInterpolator", "(Landroid/animation/TimeInterpolator;)V") => {
            put(
                vm,
                recv,
                "rd_interpolator",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getType", "()Ljava/lang/Class;") => {
            let desc = field_str(vm, recv, "rd_value_type").unwrap_or_else(|| {
                match anim_keyframe_kind(vm, recv) {
                    ANIM_KIND_INT => "I".to_string(),
                    ANIM_KIND_FLOAT => "F".to_string(),
                    _ => "Ljava/lang/Object;".to_string(),
                }
            });
            anim_class_value(vm, &desc)?
        }
        ("getIntValue", "()I") => Value::Int(field_int(vm, recv, "rd_value")),
        ("getFloatValue", "()F") => Value::Float(field_float(vm, recv, "rd_value")),
        ("clone", "()Landroid/animation/Keyframe;") | ("clone", "()Ljava/lang/Object;") => {
            Value::Obj(anim_clone_keyframe(vm, recv)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn animation_evaluator_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let _ = recv;
    let value = match name {
        "<init>" => Value::Null,
        "evaluate" => {
            let fraction = farg(args, 0);
            let start = args.get(1).cloned().unwrap_or(Value::Null);
            let end = args.get(2).cloned().unwrap_or(Value::Null);
            match class {
                INT_EVALUATOR => {
                    let s = anim_number_int(vm, &start)
                        .ok_or_else(|| anim_class_cast("startValue is not an Integer"))?;
                    let e = anim_number_int(vm, &end)
                        .ok_or_else(|| anim_class_cast("endValue is not an Integer"))?;
                    anim_boxed(
                        vm,
                        ANIM_KIND_INT,
                        &Value::Int(anim_int_lerp(fraction, s, e)),
                    )?
                }
                FLOAT_EVALUATOR => {
                    let s = anim_number_float(vm, &start)
                        .ok_or_else(|| anim_class_cast("startValue is not a Number"))?;
                    let e = anim_number_float(vm, &end)
                        .ok_or_else(|| anim_class_cast("endValue is not a Number"))?;
                    anim_boxed(
                        vm,
                        ANIM_KIND_FLOAT,
                        &Value::Float(anim_float_lerp(fraction, s, e)),
                    )?
                }
                ARGB_EVALUATOR => {
                    let s = anim_number_int(vm, &start)
                        .ok_or_else(|| anim_class_cast("startValue is not an Integer"))?;
                    let e = anim_number_int(vm, &end)
                        .ok_or_else(|| anim_class_cast("endValue is not an Integer"))?;
                    anim_boxed(
                        vm,
                        ANIM_KIND_INT,
                        &Value::Int(anim_argb_evaluate(fraction, s, e)),
                    )?
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn android_property_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "(Ljava/lang/Class;Ljava/lang/String;)V") => {
            put(
                vm,
                recv,
                "rd_type",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "rd_name",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;)V") if class == ANDROID_INT_PROPERTY => {
            let type_value = anim_class_value(vm, "Ljava/lang/Integer;")?;
            put(vm, recv, "rd_type", type_value)?;
            put(
                vm,
                recv,
                "rd_name",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;)V") if class == ANDROID_FLOAT_PROPERTY => {
            let type_value = anim_class_value(vm, "Ljava/lang/Float;")?;
            put(vm, recv, "rd_type", type_value)?;
            put(
                vm,
                recv,
                "rd_name",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "rd_name"),
        ("getType", "()Ljava/lang/Class;") => get(vm, recv, "rd_type"),
        ("isReadOnly", "()Z") => Value::Int(0),
        ("set", "(Ljava/lang/Object;Ljava/lang/Object;)V") => {
            let runtime_class = class_of(vm, recv);
            let target = args.first().cloned().unwrap_or(Value::Null);
            let new_value = args.get(1).cloned().unwrap_or(Value::Null);
            if vm.cp.is_subtype(&runtime_class, ANDROID_INT_PROPERTY) {
                let value = anim_number_int(vm, &new_value).ok_or_else(|| npe("value"))?;
                vm.invoke_instance(
                    recv,
                    "setValue",
                    "(Ljava/lang/Object;I)V",
                    &[target, Value::Int(value)],
                )?;
            } else if vm.cp.is_subtype(&runtime_class, ANDROID_FLOAT_PROPERTY) {
                let value = anim_number_float(vm, &new_value).ok_or_else(|| npe("value"))?;
                vm.invoke_instance(
                    recv,
                    "setValue",
                    "(Ljava/lang/Object;F)V",
                    &[target, Value::Float(value)],
                )?;
            } else {
                let property_name =
                    field_str(vm, recv, "rd_name").unwrap_or_else(|| "null".to_string());
                return Err(throw(
                    "Ljava/lang/UnsupportedOperationException;",
                    format!("Property {property_name} is read-only"),
                ));
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn animation_handler_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("doFrame", "(J)V") => {
            let _ = recv;
            let frame_time = vm.fw.clock() as i64;
            anim_handler_process(vm, frame_time)?;
            Value::Null
        }
        ("doAnimationFrame", "(J)V") => {
            anim_handler_process(vm, larg(args, 0))?;
            Value::Null
        }
        (
            "addAnimationFrameCallback",
            "(Landroid/animation/AnimationHandler$AnimationFrameCallback;J)V",
        ) => {
            let callback = req_obj(args, 0, "callback")?;
            anim_register_callback(vm, callback, larg(args, 1))?;
            Value::Null
        }
        ("removeCallback", "(Landroid/animation/AnimationHandler$AnimationFrameCallback;)V") => {
            if let Some(callback) = oarg(args, 0) {
                anim_unregister_callback(vm, callback);
            }
            Value::Null
        }
        ("getFrameDelay", "()J") => Value::Long(anim_frame_delay(vm)),
        ("setFrameDelay", "(J)V") => {
            anim_set_frame_delay(vm, larg(args, 0));
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn animator_base_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => {
            if matches!(get(vm, recv, "rd_duration"), Value::Null) {
                anim_init_animator(vm, recv)?;
            }
            Value::Null
        }
        ("addListener", "(Landroid/animation/Animator$AnimatorListener;)V") => {
            anim_append_listener(
                vm,
                recv,
                "rd_listeners",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("removeListener", "(Landroid/animation/Animator$AnimatorListener;)V") => {
            let listener = args.first().cloned().unwrap_or(Value::Null);
            anim_remove_listener(vm, recv, "rd_listeners", &listener)?;
            Value::Null
        }
        ("getListeners", "()Ljava/util/ArrayList;") => get(vm, recv, "rd_listeners"),
        ("addPauseListener", "(Landroid/animation/Animator$AnimatorPauseListener;)V") => {
            anim_append_listener(
                vm,
                recv,
                "rd_pause_listeners",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("removePauseListener", "(Landroid/animation/Animator$AnimatorPauseListener;)V") => {
            let listener = args.first().cloned().unwrap_or(Value::Null);
            anim_remove_listener(vm, recv, "rd_pause_listeners", &listener)?;
            Value::Null
        }
        ("removeAllListeners", "()V") => {
            put(vm, recv, "rd_listeners", Value::Null)?;
            put(vm, recv, "rd_pause_listeners", Value::Null)?;
            Value::Null
        }
        ("isPaused", "()Z") => bool_value(field_bool(vm, recv, "rd_paused")),
        ("pause", "()V") => {
            anim_pause(vm, recv)?;
            Value::Null
        }
        ("resume", "()V") => {
            anim_resume(vm, recv)?;
            Value::Null
        }
        ("getChangingConfigurations", "()I") => {
            Value::Int(field_int(vm, recv, "rd_changing_configurations"))
        }
        ("setChangingConfigurations", "(I)V") => {
            put(
                vm,
                recv,
                "rd_changing_configurations",
                Value::Int(iarg(args, 0)),
            )?;
            Value::Null
        }
        ("appendChangingConfigurations", "(I)V") => {
            let value = field_int(vm, recv, "rd_changing_configurations") | iarg(args, 0);
            put(vm, recv, "rd_changing_configurations", Value::Int(value))?;
            Value::Null
        }
        ("start" | "cancel" | "end" | "setupStartValues" | "setupEndValues", "()V") => Value::Null,
        ("setTarget", "(Ljava/lang/Object;)V") => Value::Null,
        ("canReverse", "()Z") => Value::Int(0),
        ("reverse", "()V") => return Err(ise("Reverse is not supported")),
        ("isStarted", "()Z") => vm.invoke_instance(recv, "isRunning", "()Z", &[])?,
        ("getTotalDuration", "()J") => {
            let duration = vm
                .invoke_instance(recv, "getDuration", "()J", &[])?
                .as_long()?;
            if duration == -1 {
                Value::Long(-1)
            } else {
                let delay = vm
                    .invoke_instance(recv, "getStartDelay", "()J", &[])?
                    .as_long()?;
                Value::Long(delay.wrapping_add(duration))
            }
        }
        ("getInterpolator", "()Landroid/animation/TimeInterpolator;")
        | ("getInterpolator", "()Landroid/view/animation/Interpolator;") => Value::Null,
        ("clone", "()Landroid/animation/Animator;") | ("clone", "()Ljava/lang/Object;") => {
            Value::Obj(anim_clone_animator(vm, recv)?)
        }
        ("setAllowRunningAsynchronously", "(Z)V") => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn value_animator_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => {
            anim_init_animator(vm, recv)?;
            Value::Null
        }
        ("setIntValues", "([I)V") => {
            let values = int_values(vm, args.first());
            anim_animator_set_int_values(vm, recv, &values, false)?;
            Value::Null
        }
        ("setFloatValues", "([F)V") => {
            let values = float_values(vm, args.first());
            anim_animator_set_float_values(vm, recv, &values, false)?;
            Value::Null
        }
        ("setObjectValues", "([Ljava/lang/Object;)V") => {
            let values = array_elems(vm, args.first());
            anim_animator_set_object_values(vm, recv, &values, false)?;
            Value::Null
        }
        ("setValues", "([Landroid/animation/PropertyValuesHolder;)V") => {
            let values = args.first().cloned().unwrap_or(Value::Null);
            if matches!(values, Value::Null) {
                return Err(npe("values"));
            }
            put(vm, recv, "rd_values", values)?;
            anim_put_bool(vm, recv, "rd_initialized", false)?;
            Value::Null
        }
        ("getValues", "()[Landroid/animation/PropertyValuesHolder;") => get(vm, recv, "rd_values"),
        ("setDuration", "(J)Landroid/animation/ValueAnimator;") => {
            let duration = larg(args, 0);
            if duration < 0 {
                return Err(iae(format!(
                    "Animators cannot have negative duration: {duration}"
                )));
            }
            put(vm, recv, "rd_duration", Value::Long(duration))?;
            Value::Obj(recv)
        }
        ("getDuration", "()J") => Value::Long(anim_duration(vm, recv)),
        ("getTotalDuration", "()J") => {
            let repeat = field_int(vm, recv, "rd_repeat_count");
            if repeat == -1 {
                Value::Long(-1)
            } else {
                let total = anim_duration(vm, recv).wrapping_mul(repeat.wrapping_add(1) as i64);
                Value::Long(anim_long_or(vm, recv, "rd_start_delay", 0).wrapping_add(total))
            }
        }
        ("overrideDurationScale", "(F)V") => {
            put(vm, recv, "rd_duration_scale", Value::Float(farg(args, 0)))?;
            Value::Null
        }
        ("setCurrentPlayTime", "(J)V") => {
            let duration = anim_duration(vm, recv);
            let fraction = if duration > 0 {
                larg(args, 0) as f32 / duration as f32
            } else {
                1.0
            };
            anim_set_current_fraction(vm, recv, fraction)?;
            Value::Null
        }
        ("setCurrentFraction", "(F)V") => {
            anim_set_current_fraction(vm, recv, farg(args, 0))?;
            Value::Null
        }
        ("getCurrentPlayTime", "()J") => {
            let seek = anim_float_or(vm, recv, "rd_seek_fraction", -1.0);
            if !field_bool(vm, recv, "rd_initialized")
                || (!field_bool(vm, recv, "rd_started") && seek < 0.0)
            {
                Value::Long(0)
            } else if seek >= 0.0 {
                Value::Long((anim_duration(vm, recv) as f32 * seek) as i64)
            } else {
                let mut scale = anim_resolve_duration_scale(vm, recv);
                if scale == 0.0 {
                    scale = 1.0;
                }
                let elapsed = vm.fw.clock() as i64 - anim_long_or(vm, recv, "rd_start_time", -1);
                Value::Long((elapsed as f32 / scale) as i64)
            }
        }
        ("getStartDelay", "()J") => Value::Long(anim_long_or(vm, recv, "rd_start_delay", 0)),
        ("setStartDelay", "(J)V") => {
            put(
                vm,
                recv,
                "rd_start_delay",
                Value::Long(larg(args, 0).max(0)),
            )?;
            Value::Null
        }
        ("getAnimatedValue", "()Ljava/lang/Object;") => {
            if let Some(holder) = anim_values(vm, recv).first().copied() {
                anim_pvh_animated_value(vm, holder)?
            } else {
                Value::Null
            }
        }
        ("getAnimatedValue", "(Ljava/lang/String;)Ljava/lang/Object;") => {
            let property_name = opt_str_arg(vm, args, 0);
            let mut out = Value::Null;
            for holder in anim_values(vm, recv) {
                if field_str(vm, holder, "rd_property_name") == property_name {
                    out = anim_pvh_animated_value(vm, holder)?;
                    break;
                }
            }
            out
        }
        ("setRepeatCount", "(I)V") => {
            put(vm, recv, "rd_repeat_count", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        ("getRepeatCount", "()I") => Value::Int(field_int(vm, recv, "rd_repeat_count")),
        ("setRepeatMode", "(I)V") => {
            put(vm, recv, "rd_repeat_mode", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        ("getRepeatMode", "()I") => {
            let mode = field_int(vm, recv, "rd_repeat_mode");
            Value::Int(if mode == 0 { 1 } else { mode })
        }
        ("addUpdateListener", "(Landroid/animation/ValueAnimator$AnimatorUpdateListener;)V") => {
            anim_append_listener(
                vm,
                recv,
                "rd_update_listeners",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("removeUpdateListener", "(Landroid/animation/ValueAnimator$AnimatorUpdateListener;)V") => {
            let listener = args.first().cloned().unwrap_or(Value::Null);
            anim_remove_listener(vm, recv, "rd_update_listeners", &listener)?;
            Value::Null
        }
        ("removeAllUpdateListeners", "()V") => {
            put(vm, recv, "rd_update_listeners", Value::Null)?;
            Value::Null
        }
        ("setInterpolator", "(Landroid/animation/TimeInterpolator;)V")
        | ("setInterpolator", "(Landroid/view/animation/Interpolator;)V") => {
            let interpolator = match args.first() {
                Some(Value::Obj(_)) => args.first().cloned().unwrap_or(Value::Null),
                _ => {
                    let linear = alloc_obj(
                        vm,
                        "Landroid/view/animation/LinearInterpolator;",
                        Vec::new(),
                    )?;
                    Value::Obj(linear)
                }
            };
            put(vm, recv, "rd_interpolator", interpolator)?;
            Value::Null
        }
        ("getInterpolator", "()Landroid/animation/TimeInterpolator;")
        | ("getInterpolator", "()Landroid/view/animation/Interpolator;") => {
            let current = get(vm, recv, "rd_interpolator");
            if matches!(current, Value::Obj(_)) {
                current
            } else {
                let default = alloc_obj(
                    vm,
                    "Landroid/view/animation/AccelerateDecelerateInterpolator;",
                    Vec::new(),
                )?;
                put(vm, recv, "rd_interpolator", Value::Obj(default))?;
                Value::Obj(default)
            }
        }
        ("setEvaluator", "(Landroid/animation/TypeEvaluator;)V") => {
            let evaluator = args.first().cloned().unwrap_or(Value::Null);
            anim_animator_set_evaluator(vm, recv, evaluator)?;
            Value::Null
        }
        ("start", "()V") => {
            anim_start(vm, recv, false)?;
            Value::Null
        }
        ("start", "(Z)V") => {
            anim_start(vm, recv, iarg(args, 0) != 0)?;
            Value::Null
        }
        ("startWithoutPulsing", "(Z)V") => {
            anim_put_bool(vm, recv, "rd_suppress_self_pulse", true)?;
            let result = if iarg(args, 0) != 0 {
                // ValueAnimator.reverse() delegates to start(true) when it
                // has not started; the already-pulsing branch cannot occur
                // for an animator being started without pulsing.
                anim_start(vm, recv, true)
            } else {
                anim_start(vm, recv, false)
            };
            anim_put_bool(vm, recv, "rd_suppress_self_pulse", false)?;
            result?;
            Value::Null
        }
        ("cancel", "()V") => {
            anim_cancel(vm, recv)?;
            Value::Null
        }
        ("end", "()V") => {
            anim_end(vm, recv)?;
            Value::Null
        }
        ("pause", "()V") => {
            anim_pause(vm, recv)?;
            Value::Null
        }
        ("resume", "()V") => {
            anim_resume(vm, recv)?;
            Value::Null
        }
        ("isRunning", "()Z") => bool_value(field_bool(vm, recv, "rd_running")),
        ("isStarted", "()Z") => bool_value(field_bool(vm, recv, "rd_started")),
        ("reverse", "()V") => {
            if anim_long_or(vm, recv, "rd_last_frame_time", -1) >= 0 {
                let now = vm.fw.clock() as i64;
                let start_time = anim_long_or(vm, recv, "rd_start_time", -1);
                let current_play_time = now - start_time;
                let time_left = anim_scaled_duration(vm, recv) - current_play_time;
                put(vm, recv, "rd_start_time", Value::Long(now - time_left))?;
                anim_put_bool(vm, recv, "rd_start_time_committed", true)?;
                let reversing = field_bool(vm, recv, "rd_reversing");
                anim_put_bool(vm, recv, "rd_reversing", !reversing)?;
            } else if field_bool(vm, recv, "rd_started") {
                let reversing = field_bool(vm, recv, "rd_reversing");
                anim_put_bool(vm, recv, "rd_reversing", !reversing)?;
                anim_end(vm, recv)?;
            } else {
                anim_start(vm, recv, true)?;
            }
            Value::Null
        }
        ("canReverse", "()Z") => Value::Int(1),
        ("getAnimatedFraction", "()F") => {
            Value::Float(anim_float_or(vm, recv, "rd_current_fraction", 0.0))
        }
        ("doAnimationFrame", "(J)Z") => {
            bool_value(anim_do_animation_frame(vm, recv, larg(args, 0))?)
        }
        ("pulseAnimationFrame", "(J)Z") => {
            if field_bool(vm, recv, "rd_self_pulse") {
                Value::Int(0)
            } else {
                bool_value(anim_do_animation_frame(vm, recv, larg(args, 0))?)
            }
        }
        ("commitAnimationFrame", "(J)V") => {
            if !field_bool(vm, recv, "rd_start_time_committed") {
                anim_put_bool(vm, recv, "rd_start_time_committed", true)?;
                let adjustment = larg(args, 0) - anim_long_or(vm, recv, "rd_last_frame_time", -1);
                if adjustment > 0 {
                    let start = anim_long_or(vm, recv, "rd_start_time", -1) + adjustment;
                    put(vm, recv, "rd_start_time", Value::Long(start))?;
                }
            }
            Value::Null
        }
        ("animateBasedOnTime", "(J)Z") => {
            bool_value(anim_animate_based_on_time(vm, recv, larg(args, 0))?)
        }
        ("animateValue", "(F)V") => {
            anim_animate_value(vm, recv, farg(args, 0))?;
            Value::Null
        }
        ("initAnimation", "()V") => {
            anim_init_animation(vm, recv)?;
            Value::Null
        }
        ("isInitialized", "()Z") => bool_value(field_bool(vm, recv, "rd_initialized")),
        ("skipToEndValue", "(Z)V") => {
            anim_init_animation(vm, recv)?;
            let repeat = field_int(vm, recv, "rd_repeat_count");
            let mode = field_int(vm, recv, "rd_repeat_mode");
            let mut fraction = if iarg(args, 0) != 0 { 0.0 } else { 1.0 };
            if repeat % 2 == 1 && mode == 2 {
                fraction = 0.0;
            }
            anim_animate_value(vm, recv, fraction)?;
            Value::Null
        }
        ("clone", "()Landroid/animation/ValueAnimator;")
        | ("clone", "()Landroid/animation/Animator;")
        | ("clone", "()Ljava/lang/Object;") => Value::Obj(anim_clone_animator(vm, recv)?),
        ("toString", "()Ljava/lang/String;") => {
            let mut out = format!("ValueAnimator@{recv:x}");
            if !matches!(get(vm, recv, "rd_values"), Value::Null) {
                for holder in anim_values(vm, recv) {
                    let text =
                        vm.invoke_instance(holder, "toString", "()Ljava/lang/String;", &[])?;
                    out.push_str("\n    ");
                    out.push_str(&opt_str(vm, Some(&text)).unwrap_or_default());
                }
            }
            jstr(vm, out)?
        }
        _ => return animator_base_instance(vm, name, sig, recv, args),
    };
    Ok(Some(value))
}

fn object_animator_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => {
            anim_init_animator(vm, recv)?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/Object;Ljava/lang/String;)V") => {
            anim_init_animator(vm, recv)?;
            put(
                vm,
                recv,
                "rd_target",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put_opt_str(vm, recv, "rd_property_name", opt_str_arg(vm, args, 1))?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/Object;Landroid/util/Property;)V") => {
            anim_init_animator(vm, recv)?;
            put(
                vm,
                recv,
                "rd_target",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "rd_property",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setIntValues", "([I)V") => {
            let values = int_values(vm, args.first());
            anim_animator_set_int_values(vm, recv, &values, true)?;
            Value::Null
        }
        ("setFloatValues", "([F)V") => {
            let values = float_values(vm, args.first());
            anim_animator_set_float_values(vm, recv, &values, true)?;
            Value::Null
        }
        ("setObjectValues", "([Ljava/lang/Object;)V") => {
            let values = array_elems(vm, args.first());
            anim_animator_set_object_values(vm, recv, &values, true)?;
            Value::Null
        }
        ("setDuration", "(J)Landroid/animation/ObjectAnimator;") => {
            let duration = larg(args, 0);
            if duration < 0 {
                return Err(iae(format!(
                    "Animators cannot have negative duration: {duration}"
                )));
            }
            put(vm, recv, "rd_duration", Value::Long(duration))?;
            Value::Obj(recv)
        }
        ("setPropertyName", "(Ljava/lang/String;)V") => {
            let property_name = opt_str_arg(vm, args, 0);
            if !matches!(get(vm, recv, "rd_values"), Value::Null) {
                if let Some(holder) = anim_values(vm, recv).first().copied() {
                    put_opt_str(vm, holder, "rd_property_name", property_name.clone())?;
                }
            }
            put_opt_str(vm, recv, "rd_property_name", property_name)?;
            anim_put_bool(vm, recv, "rd_initialized", false)?;
            Value::Null
        }
        ("setProperty", "(Landroid/util/Property;)V") => {
            let property = args.first().cloned().unwrap_or(Value::Null);
            if !matches!(get(vm, recv, "rd_values"), Value::Null) {
                if let Some(holder) = anim_values(vm, recv).first().copied() {
                    put(vm, holder, "rd_property", property.clone())?;
                }
            }
            if matches!(get(vm, recv, "rd_property"), Value::Obj(_)) {
                let property_name = anim_property_name(vm, &property);
                put_opt_str(vm, recv, "rd_property_name", property_name)?;
            }
            put(vm, recv, "rd_property", property)?;
            anim_put_bool(vm, recv, "rd_initialized", false)?;
            Value::Null
        }
        ("getPropertyName", "()Ljava/lang/String;") => {
            if let Some(property_name) = field_str(vm, recv, "rd_property_name") {
                jstr(vm, property_name)?
            } else if matches!(get(vm, recv, "rd_property"), Value::Obj(_)) {
                let property = get(vm, recv, "rd_property");
                match anim_property_name(vm, &property) {
                    Some(name) => jstr(vm, name)?,
                    None => Value::Null,
                }
            } else if !matches!(get(vm, recv, "rd_values"), Value::Null) {
                let holders = anim_values(vm, recv);
                if holders.is_empty() {
                    Value::Null
                } else {
                    let names: Vec<String> = holders
                        .iter()
                        .map(|holder| {
                            field_str(vm, *holder, "rd_property_name")
                                .unwrap_or_else(|| "null".to_string())
                        })
                        .collect();
                    jstr(vm, names.join(","))?
                }
            } else {
                Value::Null
            }
        }
        ("getTarget", "()Ljava/lang/Object;") => get(vm, recv, "rd_target"),
        ("setTarget", "(Ljava/lang/Object;)V") => {
            let target = args.first().cloned().unwrap_or(Value::Null);
            if get(vm, recv, "rd_target") != target {
                if field_bool(vm, recv, "rd_started") {
                    anim_cancel(vm, recv)?;
                }
                put(vm, recv, "rd_target", target)?;
                anim_put_bool(vm, recv, "rd_initialized", false)?;
            }
            Value::Null
        }
        ("setAutoCancel", "(Z)V") => {
            anim_put_bool(vm, recv, "rd_auto_cancel", iarg(args, 0) != 0)?;
            Value::Null
        }
        ("shouldAutoCancel", "(Landroid/animation/AnimationHandler$AnimationFrameCallback;)Z") => {
            let other = oarg(args, 0);
            bool_value(match other {
                Some(other_ref) => {
                    anim_is_object_animator(vm, other_ref)
                        && field_bool(vm, other_ref, "rd_auto_cancel")
                        && anim_same_target_and_properties(vm, recv, other_ref)
                }
                None => false,
            })
        }
        ("start", "()V") => {
            anim_auto_cancel_based_on(vm, recv)?;
            anim_start(vm, recv, false)?;
            Value::Null
        }
        ("setupStartValues", "()V") => {
            anim_init_animation(vm, recv)?;
            if let Some(target) = field_obj(vm, recv, "rd_target") {
                for holder in anim_values(vm, recv) {
                    if let Some(keyframe) = anim_pvh_keyframes(vm, holder).first().copied() {
                        anim_pvh_setup_value(vm, holder, target, keyframe, false)?;
                    }
                }
            }
            Value::Null
        }
        ("setupEndValues", "()V") => {
            anim_init_animation(vm, recv)?;
            if let Some(target) = field_obj(vm, recv, "rd_target") {
                for holder in anim_values(vm, recv) {
                    if let Some(keyframe) = anim_pvh_keyframes(vm, holder).last().copied() {
                        anim_pvh_setup_value(vm, holder, target, keyframe, false)?;
                    }
                }
            }
            Value::Null
        }
        ("clone", "()Landroid/animation/ObjectAnimator;")
        | ("clone", "()Landroid/animation/ValueAnimator;")
        | ("clone", "()Landroid/animation/Animator;")
        | ("clone", "()Ljava/lang/Object;") => Value::Obj(anim_clone_animator(vm, recv)?),
        ("toString", "()Ljava/lang/String;") => {
            let target = get(vm, recv, "rd_target");
            let target_text = value_string(vm, &target)?;
            let mut out = format!("ObjectAnimator@{recv:x}, target {target_text}");
            if !matches!(get(vm, recv, "rd_values"), Value::Null) {
                for holder in anim_values(vm, recv) {
                    let text =
                        vm.invoke_instance(holder, "toString", "()Ljava/lang/String;", &[])?;
                    out.push_str("\n    ");
                    out.push_str(&opt_str(vm, Some(&text)).unwrap_or_default());
                }
            }
            jstr(vm, out)?
        }
        _ => return value_animator_instance(vm, name, sig, recv, args),
    };
    Ok(Some(value))
}

// ---------------------------------------------------------------------------
// android.service.autofill: service-side value objects and callbacks
// ---------------------------------------------------------------------------

const AF_TYPE_TEXT: i32 = 1;
const AF_TYPE_TOGGLE: i32 = 2;
const AF_TYPE_LIST: i32 = 3;
const AF_TYPE_DATE: i32 = 4;
const AF_ID_FLAG_VIRTUAL_INT: i32 = 0x1;
const AF_ID_FLAG_VIRTUAL_LONG: i32 = 0x2;
const AF_ID_FLAG_HAS_SESSION: i32 = 0x4;

fn af_destroyed(vm: &Engine, r: ObjRef) -> bool {
    field_bool(vm, r, "destroyed")
}
fn af_throw_if_destroyed(vm: &Engine, r: ObjRef) -> Result<(), VmExit> {
    if af_destroyed(vm, r) {
        Err(ise("Already called #build()"))
    } else {
        Ok(())
    }
}
fn af_check_not_used(vm: &Engine, r: ObjRef) -> Result<(), VmExit> {
    if af_destroyed(vm, r) {
        Err(ise(
            "This Builder should not be reused. Use a new Builder instance instead",
        ))
    } else {
        Ok(())
    }
}
fn af_check_flags(value: i32, valid: i32) -> Result<i32, VmExit> {
    if value & !valid != 0 {
        Err(iae(format!(
            "Requested flags 0x{value:x}, but only 0x{valid:x} are allowed"
        )))
    } else {
        Ok(value)
    }
}
fn af_check_range(value: i32, min: i32, max: i32, name: &str) -> Result<i32, VmExit> {
    if value < min || value > max {
        Err(iae(format!("{name} is out of range [{min}, {max}]")))
    } else {
        Ok(value)
    }
}
fn af_arg_value(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}
fn af_req_arg_obj(args: &[Value], i: usize, what: &str) -> Result<ObjRef, VmExit> {
    oarg(args, i).ok_or_else(|| npe(what))
}
fn af_new_list(vm: &mut Engine, items: Vec<Value>) -> Result<ObjRef, VmExit> {
    new_collection(vm, "Ljava/util/ArrayList;", items)
}
fn af_ensure_list_field(vm: &mut Engine, r: ObjRef, field: &str) -> Result<ObjRef, VmExit> {
    if let Some(list) = field_obj(vm, r, field) {
        if matches!(vm.fw.objects.get(&list), Some(HostObj::List { .. })) {
            return Ok(list);
        }
        // A list supplied through an explicit constructor may be represented
        // by another host collection; normalize its visible items once.
        let items = collection_items(vm, Some(&Value::Obj(list)));
        let normalized = af_new_list(vm, items)?;
        put(vm, r, field, Value::Obj(normalized))?;
        return Ok(normalized);
    }
    let list = af_new_list(vm, Vec::new())?;
    put(vm, r, field, Value::Obj(list))?;
    Ok(list)
}
fn af_field_items(vm: &mut Engine, r: ObjRef, field: &str) -> Result<Vec<Value>, VmExit> {
    let list = af_ensure_list_field(vm, r, field)?;
    Ok(match vm.fw.objects.get(&list) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => Vec::new(),
    })
}
fn af_set_field_items(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    items: Vec<Value>,
) -> Result<(), VmExit> {
    let list = af_ensure_list_field(vm, r, field)?;
    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
        *elems = items;
    }
    Ok(())
}
fn af_existing_field_items(vm: &Engine, r: ObjRef, field: &str) -> Vec<Value> {
    collection_items(vm, Some(&get(vm, r, field)))
}
fn af_init_empty_list(vm: &mut Engine, r: ObjRef, field: &str) -> Result<(), VmExit> {
    let list = af_new_list(vm, Vec::new())?;
    put(vm, r, field, Value::Obj(list))
}
fn af_list_at(items: &[Value], index: i32) -> Result<Value, VmExit> {
    if index < 0 || index as usize >= items.len() {
        Err(idx_oob(format!(
            "Index {index} out of bounds for length {}",
            items.len()
        )))
    } else {
        Ok(items[index as usize].clone())
    }
}
fn af_assert_valid_ids(vm: &Engine, value: &Value) -> Result<Value, VmExit> {
    let Some(array_ref) = (match value {
        Value::Obj(r) => Some(*r),
        _ => None,
    }) else {
        return Err(iae("must have at least one id"));
    };
    let Ok((items, _)) = vm.heap.as_array_elems(array_ref) else {
        return Err(iae("must have at least one id"));
    };
    let items = items.to_vec();
    if items.is_empty() {
        return Err(iae("must have at least one id"));
    }
    for (i, item) in items.iter().enumerate() {
        if matches!(item, Value::Null) {
            return Err(iae(format!("ids[{i}] must not be null")));
        }
    }
    Ok(value.clone())
}
fn af_check_array_elements_not_null(
    vm: &Engine,
    value: &Value,
    name: &str,
) -> Result<Vec<Value>, VmExit> {
    let Some(array_ref) = (match value {
        Value::Obj(r) => Some(*r),
        _ => None,
    }) else {
        return Err(npe(&format!("{name} is null")));
    };
    let Ok((items, _)) = vm.heap.as_array_elems(array_ref) else {
        return Err(npe(&format!("{name} is null")));
    };
    let items = items.to_vec();
    for item in &items {
        if matches!(item, Value::Null) {
            return Err(npe(&format!("{name} has null element")));
        }
    }
    Ok(items)
}
fn af_objects_equal(vm: &mut Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    match (a, b) {
        (Value::Null, Value::Null) => Ok(true),
        (Value::Obj(x), Value::Obj(y)) => {
            if x == y {
                return Ok(true);
            }
            if let (Ok(s), Ok(t)) = (vm.heap.as_str(*x), vm.heap.as_str(*y)) {
                return Ok(s == t);
            }
            let result = vm.invoke_instance(
                *x,
                "equals",
                "(Ljava/lang/Object;)Z",
                std::slice::from_ref(b),
            )?;
            Ok(result.as_int().unwrap_or(0) != 0)
        }
        _ => Ok(a == b),
    }
}
fn af_objects_hash(vm: &mut Engine, value: &Value) -> Result<i32, VmExit> {
    if matches!(value, Value::Null) {
        Ok(0)
    } else {
        object_hash_code(vm, value)
    }
}
fn af_string_array(vm: &mut Engine, items: Vec<Value>) -> Result<Value, VmExit> {
    obj_array(vm, "[Ljava/lang/String;", items)
}
fn af_id_set(
    vm: &mut Engine,
    r: ObjRef,
    flags: i32,
    view_id: i32,
    virtual_child: i64,
    session_id: i32,
) -> Result<(), VmExit> {
    put(vm, r, "flags", Value::Int(flags))?;
    put(vm, r, "viewId", Value::Int(view_id))?;
    put(
        vm,
        r,
        "virtualIntId",
        Value::Int(if flags & AF_ID_FLAG_VIRTUAL_INT != 0 {
            virtual_child as i32
        } else {
            -1
        }),
    )?;
    put(
        vm,
        r,
        "virtualLongId",
        Value::Long(if flags & AF_ID_FLAG_VIRTUAL_LONG != 0 {
            virtual_child
        } else {
            -1
        }),
    )?;
    put(vm, r, "sessionId", Value::Int(session_id))
}
fn af_id_new(
    vm: &mut Engine,
    flags: i32,
    view_id: i32,
    virtual_child: i64,
    session_id: i32,
) -> Result<ObjRef, VmExit> {
    let r = alloc_obj(vm, AUTOFILL_ID, Vec::new())?;
    af_id_set(vm, r, flags, view_id, virtual_child, session_id)?;
    Ok(r)
}
fn af_no_autofill_id(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (AUTOFILL_ID.to_string(), "NO_AUTOFILL_ID".to_string());
    if let Some(value) = vm.statics.get(&key) {
        return Ok(value.clone());
    }
    let value = Value::Obj(af_id_new(vm, 0, 0, -1, 0)?);
    vm.statics.insert(key, value.clone());
    Ok(value)
}
fn af_id_equals(vm: &Engine, a: ObjRef, b: ObjRef, ignore_session: bool) -> bool {
    field_int(vm, a, "viewId") == field_int(vm, b, "viewId")
        && field_int(vm, a, "virtualIntId") == field_int(vm, b, "virtualIntId")
        && field_long(vm, a, "virtualLongId") == field_long(vm, b, "virtualLongId")
        && (ignore_session || field_int(vm, a, "sessionId") == field_int(vm, b, "sessionId"))
}

fn autofill_id_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "create" => {
            let host = af_req_arg_obj(args, 0, "host")?;
            let host_id_value = vm.invoke_instance(
                host,
                "getAutofillId",
                "()Landroid/view/autofill/AutofillId;",
                &[],
            )?;
            let host_id = match host_id_value {
                Value::Obj(r) => r,
                _ => return Err(npe("host.getAutofillId()")),
            };
            Value::Obj(af_id_new(
                vm,
                AF_ID_FLAG_VIRTUAL_INT,
                field_int(vm, host_id, "viewId"),
                iarg(args, 1) as i64,
                0,
            )?)
        }
        "withoutSession" => {
            let id = af_req_arg_obj(args, 0, "id")?;
            let old_flags = field_int(vm, id, "flags");
            let child = if old_flags & AF_ID_FLAG_VIRTUAL_LONG != 0 {
                field_long(vm, id, "virtualLongId")
            } else {
                field_int(vm, id, "virtualIntId") as i64
            };
            Value::Obj(af_id_new(
                vm,
                old_flags & !AF_ID_FLAG_HAS_SESSION,
                field_int(vm, id, "viewId"),
                child,
                0,
            )?)
        }
        _ => return Ok(None),
    }))
}

fn autofill_id_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(I)V") => {
            af_id_set(vm, recv, 0, iarg(args, 0), -1, 0)?;
            Value::Null
        }
        ("<init>", "(Landroid/view/autofill/AutofillId;I)V") => {
            let host = af_req_arg_obj(args, 0, "hostId")?;
            af_id_set(
                vm,
                recv,
                AF_ID_FLAG_VIRTUAL_INT,
                field_int(vm, host, "viewId"),
                iarg(args, 1) as i64,
                0,
            )?;
            Value::Null
        }
        ("<init>", "(II)V") => {
            af_id_set(
                vm,
                recv,
                AF_ID_FLAG_VIRTUAL_INT,
                iarg(args, 0),
                iarg(args, 1) as i64,
                0,
            )?;
            Value::Null
        }
        ("<init>", "(Landroid/view/autofill/AutofillId;JI)V") => {
            let host = af_req_arg_obj(args, 0, "hostId")?;
            af_id_set(
                vm,
                recv,
                AF_ID_FLAG_VIRTUAL_LONG | AF_ID_FLAG_HAS_SESSION,
                field_int(vm, host, "viewId"),
                larg(args, 1),
                iarg(args, 2),
            )?;
            Value::Null
        }
        ("<init>", "(IIJI)V") => {
            af_id_set(
                vm,
                recv,
                iarg(args, 0),
                iarg(args, 1),
                larg(args, 2),
                iarg(args, 3),
            )?;
            Value::Null
        }
        ("getViewId", "()I") => Value::Int(field_int(vm, recv, "viewId")),
        ("getVirtualChildIntId", "()I") => Value::Int(field_int(vm, recv, "virtualIntId")),
        ("getVirtualChildLongId", "()J") => Value::Long(field_long(vm, recv, "virtualLongId")),
        ("isVirtualInt", "()Z") => {
            bool_value(field_int(vm, recv, "flags") & AF_ID_FLAG_VIRTUAL_INT != 0)
        }
        ("isVirtualLong", "()Z") => {
            bool_value(field_int(vm, recv, "flags") & AF_ID_FLAG_VIRTUAL_LONG != 0)
        }
        ("isNonVirtual", "()Z") => {
            let flags = field_int(vm, recv, "flags");
            bool_value(flags & (AF_ID_FLAG_VIRTUAL_INT | AF_ID_FLAG_VIRTUAL_LONG) == 0)
        }
        ("hasSession", "()Z") => {
            bool_value(field_int(vm, recv, "flags") & AF_ID_FLAG_HAS_SESSION != 0)
        }
        ("getSessionId", "()I") => Value::Int(field_int(vm, recv, "sessionId")),
        ("setSessionId", "(I)V") => {
            let flags = field_int(vm, recv, "flags") | AF_ID_FLAG_HAS_SESSION;
            put(vm, recv, "flags", Value::Int(flags))?;
            put(vm, recv, "sessionId", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        ("resetSessionId", "()V") => {
            let flags = field_int(vm, recv, "flags") & !AF_ID_FLAG_HAS_SESSION;
            put(vm, recv, "flags", Value::Int(flags))?;
            put(vm, recv, "sessionId", Value::Int(0))?;
            Value::Null
        }
        ("equalsIgnoreSession", "(Landroid/view/autofill/AutofillId;)Z") => {
            bool_value(match oarg(args, 0) {
                Some(other) => other == recv || af_id_equals(vm, recv, other, true),
                None => false,
            })
        }
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(match oarg(args, 0) {
            Some(other) => {
                other == recv
                    || (class_of(vm, other) == AUTOFILL_ID && af_id_equals(vm, recv, other, false))
            }
            None => false,
        }),
        ("hashCode", "()I") => {
            let virtual_long = field_long(vm, recv, "virtualLongId") as u64;
            let long_hash = (virtual_long ^ (virtual_long >> 32)) as i32;
            let mut result: i32 = 1;
            result = result
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "viewId"));
            result = result
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "virtualIntId"));
            result = result.wrapping_mul(31).wrapping_add(long_hash);
            result = result
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "sessionId"));
            Value::Int(result)
        }
        ("toString", "()Ljava/lang/String;") => {
            let flags = field_int(vm, recv, "flags");
            let mut text = field_int(vm, recv, "viewId").to_string();
            if flags & AF_ID_FLAG_VIRTUAL_INT != 0 {
                text.push(':');
                text.push_str(&field_int(vm, recv, "virtualIntId").to_string());
            } else if flags & AF_ID_FLAG_VIRTUAL_LONG != 0 {
                text.push(':');
                text.push_str(&field_long(vm, recv, "virtualLongId").to_string());
            }
            if flags & AF_ID_FLAG_HAS_SESSION != 0 {
                text.push('@');
                text.push_str(&field_int(vm, recv, "sessionId").to_string());
            }
            jstr(vm, text)?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_value_new(vm: &mut Engine, kind: i32, value: Value) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, AUTOFILL_VALUE, Vec::new())?;
    put(vm, r, "type", Value::Int(kind))?;
    put(vm, r, "value", value)?;
    Ok(Value::Obj(r))
}

fn autofill_value_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "forText" => {
            if oarg(args, 0).is_none() {
                Value::Null
            } else {
                af_value_new(vm, AF_TYPE_TEXT, af_arg_value(args, 0))?
            }
        }
        "forToggle" => {
            let boxed =
                crate::boxes::box_value(vm, "Ljava/lang/Boolean;", Value::Int(iarg(args, 0)))?;
            af_value_new(vm, AF_TYPE_TOGGLE, Value::Obj(boxed))?
        }
        "forList" => {
            let boxed =
                crate::boxes::box_value(vm, "Ljava/lang/Integer;", Value::Int(iarg(args, 0)))?;
            af_value_new(vm, AF_TYPE_LIST, Value::Obj(boxed))?
        }
        "forDate" => {
            let boxed =
                crate::boxes::box_value(vm, "Ljava/lang/Long;", Value::Long(larg(args, 0)))?;
            af_value_new(vm, AF_TYPE_DATE, Value::Obj(boxed))?
        }
        _ => return Ok(None),
    }))
}

fn af_value_check(vm: &Engine, r: ObjRef, expected: i32, label: &str) -> Result<Value, VmExit> {
    let kind = field_int(vm, r, "type");
    if kind != expected {
        Err(ise(format!(
            "value must be a {label} value, not type={kind}"
        )))
    } else {
        Ok(get(vm, r, "value"))
    }
}
fn af_value_unbox(vm: &Engine, value: &Value) -> Value {
    match value {
        Value::Obj(r) => match get(vm, *r, "value") {
            Value::Null => value.clone(),
            primitive => primitive,
        },
        _ => value.clone(),
    }
}

fn autofill_value_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let kind = field_int(vm, recv, "type");
    let v = match (name, sig) {
        ("<init>", "(ILjava/lang/Object;)V") => {
            put(vm, recv, "type", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "value", af_arg_value(args, 1))?;
            Value::Null
        }
        ("getTextValue", "()Ljava/lang/CharSequence;") => {
            af_value_check(vm, recv, AF_TYPE_TEXT, "text")?
        }
        ("isText", "()Z") => bool_value(kind == AF_TYPE_TEXT),
        ("getToggleValue", "()Z") => {
            let value = af_value_check(vm, recv, AF_TYPE_TOGGLE, "toggle")?;
            bool_value(af_value_unbox(vm, &value).as_int().unwrap_or(0) != 0)
        }
        ("isToggle", "()Z") => bool_value(kind == AF_TYPE_TOGGLE),
        ("getListValue", "()I") => {
            let value = af_value_check(vm, recv, AF_TYPE_LIST, "list")?;
            Value::Int(af_value_unbox(vm, &value).as_int().unwrap_or(0))
        }
        ("isList", "()Z") => bool_value(kind == AF_TYPE_LIST),
        ("getDateValue", "()J") => {
            let value = af_value_check(vm, recv, AF_TYPE_DATE, "date")?;
            Value::Long(af_value_unbox(vm, &value).as_long().unwrap_or(0))
        }
        ("isDate", "()Z") => bool_value(kind == AF_TYPE_DATE),
        ("isEmpty", "()Z") => {
            if kind != AF_TYPE_TEXT {
                Value::Int(0)
            } else {
                let value = get(vm, recv, "value");
                let text = charseq(vm, Some(&value))?.ok_or_else(|| npe("text value"))?;
                bool_value(text.encode_utf16().count() == 0)
            }
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let result = match oarg(args, 0) {
                Some(other) if other == recv => true,
                Some(other)
                    if class_of(vm, other) == AUTOFILL_VALUE
                        && field_int(vm, other, "type") == kind =>
                {
                    let left = get(vm, recv, "value");
                    let right = get(vm, other, "value");
                    if kind == AF_TYPE_TEXT {
                        value_string(vm, &left)? == value_string(vm, &right)?
                    } else {
                        af_objects_equal(vm, &left, &right)?
                    }
                }
                _ => false,
            };
            bool_value(result)
        }
        ("hashCode", "()I") => {
            let value = get(vm, recv, "value");
            if matches!(value, Value::Null) {
                return Err(npe("mValue.hashCode()"));
            }
            Value::Int(kind.wrapping_add(af_objects_hash(vm, &value)?))
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_new_dataset_filter(vm: &mut Engine, pattern: Value) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, AF_DATASET_FILTER, Vec::new())?;
    put(vm, r, "pattern", pattern)?;
    Ok(Value::Obj(r))
}

fn af_dataset_filter_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/util/regex/Pattern;)V") => {
            put(vm, recv, "pattern", af_arg_value(args, 0))?;
            Value::Null
        }
        ("getPattern", "()Ljava/util/regex/Pattern;") => get(vm, recv, "pattern"),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn af_field_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        (
            "<init>",
            "(Landroid/view/autofill/AutofillValue;Landroid/service/autofill/Dataset$DatasetFieldFilter;Landroid/service/autofill/Presentations;)V",
        ) => {
            put(vm, recv, "value", af_arg_value(args, 0))?;
            put(vm, recv, "filter", af_arg_value(args, 1))?;
            put(vm, recv, "presentations", af_arg_value(args, 2))?;
            Value::Null
        }
        ("getValue", "()Landroid/view/autofill/AutofillValue;") => get(vm, recv, "value"),
        (
            "getDatasetFieldFilter",
            "()Landroid/service/autofill/Dataset$DatasetFieldFilter;",
        ) => get(vm, recv, "filter"),
        ("getFilter", "()Ljava/util/regex/Pattern;") => {
            match field_obj(vm, recv, "filter") {
                Some(filter) => get(vm, filter, "pattern"),
                None => Value::Null,
            }
        }
        ("getPresentations", "()Landroid/service/autofill/Presentations;") => {
            get(vm, recv, "presentations")
        }
        _ => return Ok(None),
    }))
}

fn af_field_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            put(vm, recv, "destroyed", Value::Int(0))?;
            put(vm, recv, "value", Value::Null)?;
            put(vm, recv, "filter", Value::Null)?;
            put(vm, recv, "presentations", Value::Null)?;
            Value::Null
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Field$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "value", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        ("setFilter", "(Ljava/util/regex/Pattern;)Landroid/service/autofill/Field$Builder;") => {
            af_check_not_used(vm, recv)?;
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 0))?;
            put(vm, recv, "filter", filter)?;
            Value::Obj(recv)
        }
        (
            "setPresentations",
            "(Landroid/service/autofill/Presentations;)Landroid/service/autofill/Field$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "presentations", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        ("build", "()Landroid/service/autofill/Field;") => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "destroyed", Value::Int(1))?;
            let field = alloc_obj(vm, AF_FIELD, Vec::new())?;
            put(vm, field, "value", get(vm, recv, "value"))?;
            put(vm, field, "filter", get(vm, recv, "filter"))?;
            put(vm, field, "presentations", get(vm, recv, "presentations"))?;
            Value::Obj(field)
        }
        _ => return Ok(None),
    }))
}

fn af_presentations_validate(
    menu: &Value,
    inline: &Value,
    dialog: &Value,
    tooltip: &Value,
) -> Result<(), VmExit> {
    if matches!(menu, Value::Null) && matches!(inline, Value::Null) && matches!(dialog, Value::Null)
    {
        return Err(ise("All presentations are null."));
    }
    if !matches!(tooltip, Value::Null) && matches!(inline, Value::Null) {
        return Err(ise(
            "The inline presentation is required for mInlineTooltipPresentation.",
        ));
    }
    Ok(())
}

fn af_presentations_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        (
            "<init>",
            "(Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;)V",
        ) => {
            let menu = af_arg_value(args, 0);
            let inline = af_arg_value(args, 1);
            let dialog = af_arg_value(args, 2);
            let tooltip = af_arg_value(args, 3);
            af_presentations_validate(&menu, &inline, &dialog, &tooltip)?;
            put(vm, recv, "menuPresentation", menu)?;
            put(vm, recv, "inlinePresentation", inline)?;
            put(vm, recv, "dialogPresentation", dialog)?;
            put(vm, recv, "inlineTooltipPresentation", tooltip)?;
            Value::Null
        }
        ("getMenuPresentation", "()Landroid/widget/RemoteViews;") => {
            get(vm, recv, "menuPresentation")
        }
        (
            "getInlinePresentation",
            "()Landroid/service/autofill/InlinePresentation;",
        ) => get(vm, recv, "inlinePresentation"),
        ("getDialogPresentation", "()Landroid/widget/RemoteViews;") => {
            get(vm, recv, "dialogPresentation")
        }
        (
            "getInlineTooltipPresentation",
            "()Landroid/service/autofill/InlinePresentation;",
        ) => get(vm, recv, "inlineTooltipPresentation"),
        _ => return Ok(None),
    }))
}

fn af_presentations_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            put(vm, recv, "destroyed", Value::Int(0))?;
            for field in [
                "menuPresentation",
                "inlinePresentation",
                "dialogPresentation",
                "inlineTooltipPresentation",
            ] {
                put(vm, recv, field, Value::Null)?;
            }
            Value::Null
        }
        (
            "setMenuPresentation",
            "(Landroid/widget/RemoteViews;)Landroid/service/autofill/Presentations$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "menuPresentation", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        (
            "setInlinePresentation",
            "(Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Presentations$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "inlinePresentation", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        (
            "setDialogPresentation",
            "(Landroid/widget/RemoteViews;)Landroid/service/autofill/Presentations$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "dialogPresentation", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        (
            "setInlineTooltipPresentation",
            "(Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Presentations$Builder;",
        ) => {
            af_check_not_used(vm, recv)?;
            put(vm, recv, "inlineTooltipPresentation", af_arg_value(args, 0))?;
            Value::Obj(recv)
        }
        ("build", "()Landroid/service/autofill/Presentations;") => {
            af_check_not_used(vm, recv)?;
            // DataClass marks the builder used before invoking the
            // Presentations constructor, including when validation fails.
            put(vm, recv, "destroyed", Value::Int(1))?;
            let menu = get(vm, recv, "menuPresentation");
            let inline = get(vm, recv, "inlinePresentation");
            let dialog = get(vm, recv, "dialogPresentation");
            let tooltip = get(vm, recv, "inlineTooltipPresentation");
            af_presentations_validate(&menu, &inline, &dialog, &tooltip)?;
            let presentations = alloc_obj(vm, AF_PRESENTATIONS, Vec::new())?;
            put(vm, presentations, "menuPresentation", menu)?;
            put(vm, presentations, "inlinePresentation", inline)?;
            put(vm, presentations, "dialogPresentation", dialog)?;
            put(vm, presentations, "inlineTooltipPresentation", tooltip)?;
            Value::Obj(presentations)
        }
        _ => return Ok(None),
    }))
}

fn af_inline_new(
    vm: &mut Engine,
    slice: Value,
    spec: Value,
    pinned: bool,
) -> Result<Value, VmExit> {
    if matches!(slice, Value::Null) {
        return Err(npe("slice must not be null"));
    }
    if matches!(spec, Value::Null) {
        return Err(npe("inlinePresentationSpec must not be null"));
    }
    let r = alloc_obj(vm, AF_INLINE_PRESENTATION, Vec::new())?;
    put(vm, r, "slice", slice)?;
    put(vm, r, "inlinePresentationSpec", spec)?;
    put(vm, r, "pinned", bool_value(pinned))?;
    Ok(Value::Obj(r))
}

fn af_inline_presentation_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "createTooltipPresentation" => {
            af_inline_new(vm, af_arg_value(args, 0), af_arg_value(args, 1), false)?
        }
        _ => return Ok(None),
    }))
}

fn af_inline_presentation_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        (
            "<init>",
            "(Landroid/app/slice/Slice;Landroid/widget/inline/InlinePresentationSpec;Z)V",
        ) => {
            let slice = af_arg_value(args, 0);
            let spec = af_arg_value(args, 1);
            if matches!(slice, Value::Null) {
                return Err(npe("slice must not be null"));
            }
            if matches!(spec, Value::Null) {
                return Err(npe("inlinePresentationSpec must not be null"));
            }
            put(vm, recv, "slice", slice)?;
            put(vm, recv, "inlinePresentationSpec", spec)?;
            put(vm, recv, "pinned", bool_value(iarg(args, 2) != 0))?;
            Value::Null
        }
        ("getSlice", "()Landroid/app/slice/Slice;") => get(vm, recv, "slice"),
        ("getInlinePresentationSpec", "()Landroid/widget/inline/InlinePresentationSpec;") => {
            get(vm, recv, "inlinePresentationSpec")
        }
        ("isPinned", "()Z") => bool_value(field_bool(vm, recv, "pinned")),
        ("getAutofillHints", "()[Ljava/lang/String;") => {
            let slice = field_obj(vm, recv, "slice").ok_or_else(|| npe("slice"))?;
            let hints = vm.invoke_instance(slice, "getHints", "()Ljava/util/List;", &[])?;
            if matches!(hints, Value::Null) {
                return Err(npe("slice.getHints()"));
            }
            af_string_array(vm, collection_items(vm, Some(&hints)))?
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let result = match oarg(args, 0) {
                Some(other) if other == recv => true,
                Some(other) if class_of(vm, other) == AF_INLINE_PRESENTATION => {
                    af_objects_equal(vm, &get(vm, recv, "slice"), &get(vm, other, "slice"))?
                        && af_objects_equal(
                            vm,
                            &get(vm, recv, "inlinePresentationSpec"),
                            &get(vm, other, "inlinePresentationSpec"),
                        )?
                        && field_bool(vm, recv, "pinned") == field_bool(vm, other, "pinned")
                }
                _ => false,
            };
            bool_value(result)
        }
        ("hashCode", "()I") => {
            let mut hash: i32 = 1;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(af_objects_hash(vm, &get(vm, recv, "slice"))?);
            hash = hash.wrapping_mul(31).wrapping_add(af_objects_hash(
                vm,
                &get(vm, recv, "inlinePresentationSpec"),
            )?);
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(if field_bool(vm, recv, "pinned") {
                    1231
                } else {
                    1237
                });
            Value::Int(hash)
        }
        ("toString", "()Ljava/lang/String;") => {
            let slice = value_string(vm, &get(vm, recv, "slice"))?;
            let spec = value_string(vm, &get(vm, recv, "inlinePresentationSpec"))?;
            jstr(
                vm,
                format!(
                    "InlinePresentation {{ slice = {slice}, inlinePresentationSpec = {spec}, pinned = {} }}",
                    field_bool(vm, recv, "pinned")
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

#[derive(Default)]
struct AfDatasetLists {
    ids: Vec<Value>,
    values: Vec<Value>,
    presentations: Vec<Value>,
    dialog_presentations: Vec<Value>,
    inline_presentations: Vec<Value>,
    inline_tooltips: Vec<Value>,
    filters: Vec<Value>,
    datatypes: Vec<Value>,
}

const AF_DATASET_LIST_FIELDS: [&str; 8] = [
    "fieldIds",
    "fieldValues",
    "fieldPresentations",
    "fieldDialogPresentations",
    "fieldInlinePresentations",
    "fieldInlineTooltipPresentations",
    "fieldFilters",
    "autofillDatatypes",
];

fn af_dataset_load_lists(vm: &mut Engine, r: ObjRef) -> Result<AfDatasetLists, VmExit> {
    Ok(AfDatasetLists {
        ids: af_field_items(vm, r, "fieldIds")?,
        values: af_field_items(vm, r, "fieldValues")?,
        presentations: af_field_items(vm, r, "fieldPresentations")?,
        dialog_presentations: af_field_items(vm, r, "fieldDialogPresentations")?,
        inline_presentations: af_field_items(vm, r, "fieldInlinePresentations")?,
        inline_tooltips: af_field_items(vm, r, "fieldInlineTooltipPresentations")?,
        filters: af_field_items(vm, r, "fieldFilters")?,
        datatypes: af_field_items(vm, r, "autofillDatatypes")?,
    })
}
fn af_dataset_store_lists(vm: &mut Engine, r: ObjRef, lists: AfDatasetLists) -> Result<(), VmExit> {
    af_set_field_items(vm, r, "fieldIds", lists.ids)?;
    af_set_field_items(vm, r, "fieldValues", lists.values)?;
    af_set_field_items(vm, r, "fieldPresentations", lists.presentations)?;
    af_set_field_items(
        vm,
        r,
        "fieldDialogPresentations",
        lists.dialog_presentations,
    )?;
    af_set_field_items(
        vm,
        r,
        "fieldInlinePresentations",
        lists.inline_presentations,
    )?;
    af_set_field_items(
        vm,
        r,
        "fieldInlineTooltipPresentations",
        lists.inline_tooltips,
    )?;
    af_set_field_items(vm, r, "fieldFilters", lists.filters)?;
    af_set_field_items(vm, r, "autofillDatatypes", lists.datatypes)
}
fn af_dataset_set_at(items: &mut Vec<Value>, index: usize, value: Value) {
    if index >= items.len() {
        items.resize(index + 1, Value::Null);
    }
    items[index] = value;
}
#[allow(clippy::too_many_arguments)]
fn af_dataset_set_id(
    vm: &mut Engine,
    builder: ObjRef,
    id: Value,
    value: Value,
    presentation: Value,
    inline: Value,
    tooltip: Value,
    filter: Value,
    dialog: Value,
) -> Result<usize, VmExit> {
    let id_ref = match id {
        Value::Obj(r) => r,
        _ => return Err(npe("id cannot be null")),
    };
    let mut lists = af_dataset_load_lists(vm, builder)?;
    let existing = lists.ids.iter().position(|candidate| match candidate {
        Value::Obj(other) => {
            class_of(vm, *other) == AUTOFILL_ID
                && class_of(vm, id_ref) == AUTOFILL_ID
                && af_id_equals(vm, id_ref, *other, false)
        }
        _ => false,
    });
    if let Some(index) = existing {
        af_dataset_set_at(&mut lists.values, index, value);
        af_dataset_set_at(&mut lists.presentations, index, presentation);
        af_dataset_set_at(&mut lists.dialog_presentations, index, dialog);
        af_dataset_set_at(&mut lists.inline_presentations, index, inline);
        af_dataset_set_at(&mut lists.inline_tooltips, index, tooltip);
        af_dataset_set_at(&mut lists.filters, index, filter);
        af_dataset_store_lists(vm, builder, lists)?;
        return Ok(index);
    }
    lists.ids.push(Value::Obj(id_ref));
    lists.datatypes.push(Value::Null);
    lists.values.push(value);
    lists.presentations.push(presentation);
    lists.dialog_presentations.push(dialog);
    lists.inline_presentations.push(inline);
    lists.inline_tooltips.push(tooltip);
    lists.filters.push(filter);
    let index = lists.ids.len() - 1;
    af_dataset_store_lists(vm, builder, lists)?;
    Ok(index)
}
#[allow(clippy::too_many_arguments)]
fn af_dataset_set_datatype(
    vm: &mut Engine,
    builder: ObjRef,
    datatype: Value,
    value: Value,
    presentation: Value,
    inline: Value,
    tooltip: Value,
    filter: Value,
    dialog: Value,
) -> Result<usize, VmExit> {
    let datatype_text =
        opt_str(vm, Some(&datatype)).ok_or_else(|| npe("datatype cannot be null"))?;
    let mut lists = af_dataset_load_lists(vm, builder)?;
    let existing = lists
        .datatypes
        .iter()
        .position(|candidate| opt_str(vm, Some(candidate)).as_deref() == Some(&datatype_text));
    if let Some(index) = existing {
        // AOSP appends the duplicate datatype rather than replacing it in
        // this branch. Preserve that observable list-length quirk.
        lists.datatypes.push(datatype);
        af_dataset_set_at(&mut lists.values, index, value);
        af_dataset_set_at(&mut lists.presentations, index, presentation);
        af_dataset_set_at(&mut lists.dialog_presentations, index, dialog);
        af_dataset_set_at(&mut lists.inline_presentations, index, inline);
        af_dataset_set_at(&mut lists.inline_tooltips, index, tooltip);
        af_dataset_set_at(&mut lists.filters, index, filter);
        af_dataset_store_lists(vm, builder, lists)?;
        return Ok(index);
    }
    lists.ids.push(Value::Null);
    lists.datatypes.push(datatype);
    lists.values.push(value);
    lists.presentations.push(presentation);
    lists.dialog_presentations.push(dialog);
    lists.inline_presentations.push(inline);
    lists.inline_tooltips.push(tooltip);
    lists.filters.push(filter);
    let index = lists.ids.len() - 1;
    af_dataset_store_lists(vm, builder, lists)?;
    Ok(index)
}
fn af_dataset_field_index(
    vm: &mut Engine,
    builder: ObjRef,
    field: &Value,
) -> Result<Option<usize>, VmExit> {
    let keys = af_field_items(vm, builder, "fieldMapKeys")?;
    let indexes = af_field_items(vm, builder, "fieldMapIndexes")?;
    Ok(keys.iter().position(|key| key == field).and_then(|pos| {
        indexes
            .get(pos)
            .and_then(|value| value.as_int().ok())
            .map(|index| index as usize)
    }))
}
fn af_dataset_put_field_index(
    vm: &mut Engine,
    builder: ObjRef,
    field: Value,
    index: usize,
) -> Result<(), VmExit> {
    let mut keys = af_field_items(vm, builder, "fieldMapKeys")?;
    let mut indexes = af_field_items(vm, builder, "fieldMapIndexes")?;
    if let Some(pos) = keys.iter().position(|key| key == &field) {
        af_dataset_set_at(&mut indexes, pos, Value::Int(index as i32));
    } else {
        keys.push(field);
        indexes.push(Value::Int(index as i32));
    }
    af_set_field_items(vm, builder, "fieldMapKeys", keys)?;
    af_set_field_items(vm, builder, "fieldMapIndexes", indexes)
}
fn af_dataset_builder_init(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    put(vm, r, "destroyed", Value::Int(0))?;
    for field in AF_DATASET_LIST_FIELDS {
        af_init_empty_list(vm, r, field)?;
    }
    af_init_empty_list(vm, r, "fieldMapKeys")?;
    af_init_empty_list(vm, r, "fieldMapIndexes")?;
    for field in [
        "fieldContent",
        "presentation",
        "dialogPresentation",
        "inlinePresentation",
        "inlineTooltipPresentation",
        "authentication",
        "id",
    ] {
        put(vm, r, field, Value::Null)?;
    }
    Ok(())
}
fn af_dataset_from_builder(vm: &mut Engine, builder: ObjRef) -> Result<Value, VmExit> {
    let dataset = alloc_obj(vm, AF_DATASET, Vec::new())?;
    for field in AF_DATASET_LIST_FIELDS {
        put(vm, dataset, field, get(vm, builder, field))?;
    }
    for field in [
        "fieldContent",
        "presentation",
        "dialogPresentation",
        "inlinePresentation",
        "inlineTooltipPresentation",
        "authentication",
        "id",
    ] {
        put(vm, dataset, field, get(vm, builder, field))?;
    }
    put(vm, dataset, "eligibleReason", Value::Int(0))?;
    Ok(Value::Obj(dataset))
}
fn af_dataset_copy_fields(
    vm: &mut Engine,
    dataset: ObjRef,
    source: ObjRef,
    include_ids: bool,
) -> Result<(), VmExit> {
    for field in AF_DATASET_LIST_FIELDS {
        if field == "fieldIds" && !include_ids {
            continue;
        }
        put(vm, dataset, field, get(vm, source, field))?;
    }
    for field in [
        "fieldContent",
        "presentation",
        "dialogPresentation",
        "inlinePresentation",
        "inlineTooltipPresentation",
        "authentication",
        "id",
    ] {
        put(vm, dataset, field, get(vm, source, field))?;
    }
    Ok(())
}
fn af_dataset_indexed(
    vm: &Engine,
    dataset: ObjRef,
    field: &str,
    fallback_field: Option<&str>,
    index: i32,
) -> Result<Value, VmExit> {
    if field_obj(vm, dataset, field).is_none() {
        return Err(npe(&format!("{field}.get()")));
    }
    let items = af_existing_field_items(vm, dataset, field);
    let value = af_list_at(&items, index)?;
    if matches!(value, Value::Null) {
        if let Some(fallback) = fallback_field {
            return Ok(get(vm, dataset, fallback));
        }
    }
    Ok(value)
}

fn af_dataset_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/Dataset$Builder;)V") => {
            let builder = af_req_arg_obj(args, 0, "builder")?;
            af_dataset_copy_fields(vm, recv, builder, true)?;
            put(vm, recv, "eligibleReason", Value::Int(0))?;
            Value::Null
        }
        ("<init>", "(Landroid/service/autofill/Dataset;Ljava/util/ArrayList;)V") => {
            let source = af_req_arg_obj(args, 0, "dataset")?;
            af_dataset_copy_fields(vm, recv, source, false)?;
            put(vm, recv, "fieldIds", af_arg_value(args, 1))?;
            put(vm, recv, "eligibleReason", Value::Int(0))?;
            Value::Null
        }
        (
            "<init>",
            "(Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Ljava/util/ArrayList;Landroid/content/ClipData;Landroid/widget/RemoteViews;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;Landroid/service/autofill/InlinePresentation;Ljava/lang/String;Landroid/content/IntentSender;)V",
        ) => {
            let list_fields = [
                "fieldIds",
                "fieldValues",
                "fieldPresentations",
                "fieldDialogPresentations",
                "fieldInlinePresentations",
                "fieldInlineTooltipPresentations",
                "fieldFilters",
                "autofillDatatypes",
            ];
            for (i, field) in list_fields.iter().enumerate() {
                put(vm, recv, field, af_arg_value(args, i))?;
            }
            put(vm, recv, "fieldContent", af_arg_value(args, 8))?;
            put(vm, recv, "presentation", af_arg_value(args, 9))?;
            put(vm, recv, "dialogPresentation", af_arg_value(args, 10))?;
            put(vm, recv, "inlinePresentation", af_arg_value(args, 11))?;
            put(vm, recv, "inlineTooltipPresentation", af_arg_value(args, 12))?;
            put(vm, recv, "id", af_arg_value(args, 13))?;
            put(vm, recv, "authentication", af_arg_value(args, 14))?;
            put(vm, recv, "eligibleReason", Value::Int(0))?;
            Value::Null
        }
        ("getAutofillDatatypes", "()Ljava/util/ArrayList;") => {
            get(vm, recv, "autofillDatatypes")
        }
        ("getFieldIds", "()Ljava/util/ArrayList;") => get(vm, recv, "fieldIds"),
        ("getFieldValues", "()Ljava/util/ArrayList;") => get(vm, recv, "fieldValues"),
        ("getFieldPresentation", "(I)Landroid/widget/RemoteViews;") => {
            af_dataset_indexed(vm, recv, "fieldPresentations", Some("presentation"), iarg(args, 0))?
        }
        ("getFieldDialogPresentation", "(I)Landroid/widget/RemoteViews;") => af_dataset_indexed(
            vm,
            recv,
            "fieldDialogPresentations",
            Some("dialogPresentation"),
            iarg(args, 0),
        )?,
        (
            "getFieldInlinePresentation",
            "(I)Landroid/service/autofill/InlinePresentation;",
        ) => af_dataset_indexed(
            vm,
            recv,
            "fieldInlinePresentations",
            Some("inlinePresentation"),
            iarg(args, 0),
        )?,
        (
            "getFieldInlineTooltipPresentation",
            "(I)Landroid/service/autofill/InlinePresentation;",
        ) => af_dataset_indexed(
            vm,
            recv,
            "fieldInlineTooltipPresentations",
            Some("inlineTooltipPresentation"),
            iarg(args, 0),
        )?,
        (
            "getFilter",
            "(I)Landroid/service/autofill/Dataset$DatasetFieldFilter;",
        ) => af_dataset_indexed(vm, recv, "fieldFilters", None, iarg(args, 0))?,
        ("getFieldContent", "()Landroid/content/ClipData;") => get(vm, recv, "fieldContent"),
        ("getAuthentication", "()Landroid/content/IntentSender;") => {
            get(vm, recv, "authentication")
        }
        ("isEmpty", "()Z") => bool_value(
            field_obj(vm, recv, "fieldIds").is_none()
                || af_existing_field_items(vm, recv, "fieldIds").is_empty(),
        ),
        ("getId", "()Ljava/lang/String;") => get(vm, recv, "id"),
        ("setEligibleReasonReason", "(I)V") => {
            put(vm, recv, "eligibleReason", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        ("getEligibleReason", "()I") => Value::Int(field_int(vm, recv, "eligibleReason")),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_dataset_set_field_by_id(
    vm: &mut Engine,
    builder: ObjRef,
    id: Value,
    field: Value,
) -> Result<(), VmExit> {
    if let Some(index) = af_dataset_field_index(vm, builder, &field)? {
        let mut ids = af_field_items(vm, builder, "fieldIds")?;
        if matches!(af_list_at(&ids, index as i32)?, Value::Null) {
            af_dataset_set_at(&mut ids, index, id);
            af_set_field_items(vm, builder, "fieldIds", ids)?;
            return Ok(());
        }
    }
    let index = if matches!(field, Value::Null) {
        af_dataset_set_id(
            vm,
            builder,
            id,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        )?
    } else {
        let field_ref = match field {
            Value::Obj(r) => r,
            _ => return Err(npe("field")),
        };
        let value = get(vm, field_ref, "value");
        let filter = get(vm, field_ref, "filter");
        match field_obj(vm, field_ref, "presentations") {
            None => af_dataset_set_id(
                vm,
                builder,
                id,
                value,
                Value::Null,
                Value::Null,
                Value::Null,
                filter,
                Value::Null,
            )?,
            Some(presentations) => af_dataset_set_id(
                vm,
                builder,
                id,
                value,
                get(vm, presentations, "menuPresentation"),
                get(vm, presentations, "inlinePresentation"),
                get(vm, presentations, "inlineTooltipPresentation"),
                filter,
                get(vm, presentations, "dialogPresentation"),
            )?,
        }
    };
    af_dataset_put_field_index(vm, builder, field, index)
}
fn af_dataset_set_field_by_hint(
    vm: &mut Engine,
    builder: ObjRef,
    hint: Value,
    field: Value,
) -> Result<(), VmExit> {
    if let Some(index) = af_dataset_field_index(vm, builder, &field)? {
        let mut datatypes = af_field_items(vm, builder, "autofillDatatypes")?;
        if matches!(af_list_at(&datatypes, index as i32)?, Value::Null) {
            af_dataset_set_at(&mut datatypes, index, hint);
            af_set_field_items(vm, builder, "autofillDatatypes", datatypes)?;
            return Ok(());
        }
    }
    let field_ref = match field {
        Value::Obj(r) => r,
        _ => return Err(npe("field.getDatasetFieldFilter()")),
    };
    let value = get(vm, field_ref, "value");
    let filter = get(vm, field_ref, "filter");
    let index = match field_obj(vm, field_ref, "presentations") {
        None => af_dataset_set_datatype(
            vm,
            builder,
            hint,
            value,
            Value::Null,
            Value::Null,
            Value::Null,
            filter,
            Value::Null,
        )?,
        Some(presentations) => af_dataset_set_datatype(
            vm,
            builder,
            hint,
            value,
            get(vm, presentations, "menuPresentation"),
            get(vm, presentations, "inlinePresentation"),
            get(vm, presentations, "inlineTooltipPresentation"),
            filter,
            get(vm, presentations, "dialogPresentation"),
        )?,
    };
    af_dataset_put_field_index(vm, builder, Value::Obj(field_ref), index)
}

fn af_dataset_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let builder_ret = Value::Obj(recv);
    let v = match (name, sig) {
        ("<init>", "()V") => {
            af_dataset_builder_init(vm, recv)?;
            Value::Null
        }
        ("<init>", "(Landroid/widget/RemoteViews;)V") => {
            let presentation = af_arg_value(args, 0);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation must be non-null"));
            }
            af_dataset_builder_init(vm, recv)?;
            put(vm, recv, "presentation", presentation)?;
            Value::Null
        }
        ("<init>", "(Landroid/service/autofill/InlinePresentation;)V") => {
            let inline = af_arg_value(args, 0);
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation must be non-null"));
            }
            af_dataset_builder_init(vm, recv)?;
            put(vm, recv, "inlinePresentation", inline)?;
            Value::Null
        }
        ("<init>", "(Landroid/service/autofill/Presentations;)V") => {
            let presentations = af_req_arg_obj(args, 0, "presentations must be non-null")?;
            af_dataset_builder_init(vm, recv)?;
            put(
                vm,
                recv,
                "presentation",
                get(vm, presentations, "menuPresentation"),
            )?;
            put(
                vm,
                recv,
                "inlinePresentation",
                get(vm, presentations, "inlinePresentation"),
            )?;
            put(
                vm,
                recv,
                "inlineTooltipPresentation",
                get(vm, presentations, "inlineTooltipPresentation"),
            )?;
            put(
                vm,
                recv,
                "dialogPresentation",
                get(vm, presentations, "dialogPresentation"),
            )?;
            Value::Null
        }
        (
            "setInlinePresentation",
            "(Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let inline = af_arg_value(args, 0);
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation must be non-null"));
            }
            put(vm, recv, "inlinePresentation", inline)?;
            builder_ret
        }
        (
            "setInlinePresentation",
            "(Landroid/service/autofill/InlinePresentation;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let inline = af_arg_value(args, 0);
            let tooltip = af_arg_value(args, 1);
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation must be non-null"));
            }
            if matches!(tooltip, Value::Null) {
                return Err(npe("inlineTooltipPresentation must be non-null"));
            }
            put(vm, recv, "inlinePresentation", inline)?;
            put(vm, recv, "inlineTooltipPresentation", tooltip)?;
            builder_ret
        }
        (
            "setAuthentication",
            "(Landroid/content/IntentSender;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "authentication", af_arg_value(args, 0))?;
            builder_ret
        }
        ("setId", "(Ljava/lang/String;)Landroid/service/autofill/Dataset$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "id", af_arg_value(args, 0))?;
            builder_ret
        }
        (
            "setContent",
            "(Landroid/view/autofill/AutofillId;Landroid/content/ClipData;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let content_value = af_arg_value(args, 1);
            if let Value::Obj(content) = content_value {
                let count = vm
                    .invoke_instance(content, "getItemCount", "()I", &[])?
                    .as_int()
                    .unwrap_or(0);
                for i in 0..count {
                    let item_value = vm.invoke_instance(
                        content,
                        "getItemAt",
                        "(I)Landroid/content/ClipData$Item;",
                        &[Value::Int(i)],
                    )?;
                    let item = match item_value {
                        Value::Obj(r) => r,
                        _ => return Err(npe("content.getItemAt().getIntent()")),
                    };
                    let intent = vm.invoke_instance(
                        item,
                        "getIntent",
                        "()Landroid/content/Intent;",
                        &[],
                    )?;
                    if !matches!(intent, Value::Null) {
                        let content_text = value_string(vm, &Value::Obj(content))?;
                        return Err(iae(format!(
                            "Content items cannot contain an Intent: content={content_text}"
                        )));
                    }
                }
            }
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            )?;
            put(vm, recv, "fieldContent", content_value)?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Landroid/widget/RemoteViews;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 2);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            if field_obj(vm, recv, "presentation").is_none() {
                return Err(ise("Dataset presentation not set on constructor"));
            }
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 2))?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                Value::Null,
                Value::Null,
                Value::Null,
                filter,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;Landroid/widget/RemoteViews;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 3);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 2))?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                Value::Null,
                Value::Null,
                filter,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 2);
            let inline = af_arg_value(args, 3);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation cannot be null"));
            }
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                inline,
                Value::Null,
                Value::Null,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 2);
            let inline = af_arg_value(args, 3);
            let tooltip = af_arg_value(args, 4);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation cannot be null"));
            }
            if matches!(tooltip, Value::Null) {
                return Err(npe("inlineTooltipPresentation cannot be null"));
            }
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                inline,
                tooltip,
                Value::Null,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 3);
            let inline = af_arg_value(args, 4);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation cannot be null"));
            }
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 2))?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                inline,
                Value::Null,
                filter,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setValue",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let presentation = af_arg_value(args, 3);
            let inline = af_arg_value(args, 4);
            let tooltip = af_arg_value(args, 5);
            if matches!(presentation, Value::Null) {
                return Err(npe("presentation cannot be null"));
            }
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation cannot be null"));
            }
            if matches!(tooltip, Value::Null) {
                return Err(npe("inlineTooltipPresentation cannot be null"));
            }
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 2))?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                presentation,
                inline,
                tooltip,
                filter,
                Value::Null,
            )?;
            builder_ret
        }
        (
            "setField",
            "(Landroid/view/autofill/AutofillId;Landroid/service/autofill/Field;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_dataset_set_field_by_id(vm, recv, af_arg_value(args, 0), af_arg_value(args, 1))?;
            builder_ret
        }
        (
            "setField",
            "(Ljava/lang/String;Landroid/service/autofill/Field;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_dataset_set_field_by_hint(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
            )?;
            builder_ret
        }
        (
            "setFieldForAllHints",
            "(Landroid/service/autofill/Field;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let any = jstr(vm, "any")?;
            af_dataset_set_field_by_hint(vm, recv, any, af_arg_value(args, 0))?;
            builder_ret
        }
        (
            "setFieldInlinePresentation",
            "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/Dataset$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let inline = af_arg_value(args, 3);
            if matches!(inline, Value::Null) {
                return Err(npe("inlinePresentation cannot be null"));
            }
            let filter = af_new_dataset_filter(vm, af_arg_value(args, 2))?;
            af_dataset_set_id(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                Value::Null,
                inline,
                Value::Null,
                filter,
                Value::Null,
            )?;
            builder_ret
        }
        ("build", "()Landroid/service/autofill/Dataset;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "destroyed", Value::Int(1))?;
            let lists = af_dataset_load_lists(vm, recv)?;
            if lists.ids.is_empty() && lists.datatypes.is_empty() {
                return Err(ise("at least one of field or datatype must be set"));
            }
            if field_obj(vm, recv, "fieldContent").is_some() {
                if lists.ids.len() > 1 {
                    return Err(ise("when filling content, only one field can be filled"));
                }
                let first_value = af_list_at(&lists.values, 0)?;
                if !matches!(first_value, Value::Null) {
                    return Err(ise("cannot fill both content and values"));
                }
            }
            af_dataset_from_builder(vm, recv)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

const AF_FILL_RESPONSE_FIELDS: [&str; 27] = [
    "datasets",
    "saveInfo",
    "clientState",
    "presentation",
    "inlinePresentation",
    "inlineTooltipPresentation",
    "dialogPresentation",
    "dialogHeader",
    "header",
    "footer",
    "authentication",
    "authenticationIds",
    "ignoredIds",
    "fillDialogTriggerIds",
    "disableDuration",
    "fieldClassificationIds",
    "flags",
    "requestId",
    "userData",
    "cancelIds",
    "supportsInlineSuggestions",
    "iconResourceId",
    "serviceDisplayNameResourceId",
    "showFillDialogIcon",
    "showSaveDialogIcon",
    "detectedFieldTypes",
    "destroyed",
];

fn af_fill_response_from_builder(vm: &mut Engine, builder: ObjRef) -> Result<Value, VmExit> {
    let response = alloc_obj(vm, AF_FILL_RESPONSE, Vec::new())?;
    for field in AF_FILL_RESPONSE_FIELDS {
        if field == "destroyed" {
            continue;
        }
        let value = if field == "requestId" {
            Value::Int(i32::MIN)
        } else {
            get(vm, builder, field)
        };
        put(vm, response, field, value)?;
    }
    Ok(Value::Obj(response))
}
fn af_fill_response_copy_static(vm: &mut Engine, args: &[Value]) -> Result<Value, VmExit> {
    let source = af_req_arg_obj(args, 0, "response")?;
    let response = alloc_obj(vm, AF_FILL_RESPONSE, Vec::new())?;
    for field in AF_FILL_RESPONSE_FIELDS {
        if field == "destroyed" {
            continue;
        }
        let value = match field {
            "datasets" => af_arg_value(args, 1),
            "saveInfo" => af_arg_value(args, 2),
            _ => get(vm, source, field),
        };
        put(vm, response, field, value)?;
    }
    Ok(Value::Obj(response))
}
fn af_fill_response_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/FillResponse$Builder;)V") => {
            let builder = af_req_arg_obj(args, 0, "builder")?;
            for field in AF_FILL_RESPONSE_FIELDS {
                if field == "destroyed" {
                    continue;
                }
                let value = if field == "requestId" {
                    Value::Int(i32::MIN)
                } else {
                    get(vm, builder, field)
                };
                put(vm, recv, field, value)?;
            }
            Value::Null
        }
        ("getDetectedFieldClassifications", "()Ljava/util/Set;") => {
            let detected = get(vm, recv, "detectedFieldTypes");
            let items = af_check_array_elements_not_null(vm, &detected, "elements")?;
            let mut unique: Vec<Value> = Vec::new();
            for item in items {
                let mut duplicate = false;
                for existing in &unique {
                    if af_objects_equal(vm, existing, &item)? {
                        duplicate = true;
                        break;
                    }
                }
                if duplicate {
                    return Err(iae("duplicate element"));
                }
                unique.push(item);
            }
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", unique)?)
        }
        ("getClientState", "()Landroid/os/Bundle;") => get(vm, recv, "clientState"),
        ("getDatasets", "()Ljava/util/List;") => get(vm, recv, "datasets"),
        ("getSaveInfo", "()Landroid/service/autofill/SaveInfo;") => get(vm, recv, "saveInfo"),
        ("getPresentation", "()Landroid/widget/RemoteViews;") => get(vm, recv, "presentation"),
        ("getInlinePresentation", "()Landroid/service/autofill/InlinePresentation;") => {
            get(vm, recv, "inlinePresentation")
        }
        ("getInlineTooltipPresentation", "()Landroid/service/autofill/InlinePresentation;") => {
            get(vm, recv, "inlineTooltipPresentation")
        }
        ("getDialogPresentation", "()Landroid/widget/RemoteViews;") => {
            get(vm, recv, "dialogPresentation")
        }
        ("getDialogHeader", "()Landroid/widget/RemoteViews;") => get(vm, recv, "dialogHeader"),
        ("getHeader", "()Landroid/widget/RemoteViews;") => get(vm, recv, "header"),
        ("getFooter", "()Landroid/widget/RemoteViews;") => get(vm, recv, "footer"),
        ("getAuthentication", "()Landroid/content/IntentSender;") => {
            get(vm, recv, "authentication")
        }
        ("getAuthenticationIds", "()[Landroid/view/autofill/AutofillId;") => {
            get(vm, recv, "authenticationIds")
        }
        ("getFillDialogTriggerIds", "()[Landroid/view/autofill/AutofillId;") => {
            get(vm, recv, "fillDialogTriggerIds")
        }
        ("getIgnoredIds", "()[Landroid/view/autofill/AutofillId;") => get(vm, recv, "ignoredIds"),
        ("getDisableDuration", "()J") => Value::Long(field_long(vm, recv, "disableDuration")),
        ("getFieldClassificationIds", "()[Landroid/view/autofill/AutofillId;") => {
            get(vm, recv, "fieldClassificationIds")
        }
        ("getUserData", "()Landroid/service/autofill/UserData;") => get(vm, recv, "userData"),
        ("getIconResourceId", "()I") => Value::Int(field_int(vm, recv, "iconResourceId")),
        ("getServiceDisplayNameResourceId", "()I") => {
            Value::Int(field_int(vm, recv, "serviceDisplayNameResourceId"))
        }
        ("getShowFillDialogIcon", "()Z") => bool_value(field_bool(vm, recv, "showFillDialogIcon")),
        ("getShowSaveDialogIcon", "()Z") => bool_value(field_bool(vm, recv, "showSaveDialogIcon")),
        ("getFlags", "()I") => Value::Int(field_int(vm, recv, "flags")),
        ("setRequestId", "(I)V") => {
            put(vm, recv, "requestId", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        ("getRequestId", "()I") => Value::Int(field_int(vm, recv, "requestId")),
        ("getCancelIds", "()[I") => get(vm, recv, "cancelIds"),
        ("supportsInlineSuggestions", "()Z") => {
            bool_value(field_bool(vm, recv, "supportsInlineSuggestions"))
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_fill_response_builder_init(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    for field in AF_FILL_RESPONSE_FIELDS {
        let value = match field {
            "showFillDialogIcon" | "showSaveDialogIcon" => Value::Int(1),
            "disableDuration" => Value::Long(0),
            "flags"
            | "iconResourceId"
            | "serviceDisplayNameResourceId"
            | "supportsInlineSuggestions"
            | "destroyed" => Value::Int(0),
            "requestId" => Value::Int(i32::MIN),
            _ => Value::Null,
        };
        put(vm, r, field, value)?;
    }
    Ok(())
}
fn af_fill_response_throw_if_disabled(vm: &Engine, r: ObjRef) -> Result<(), VmExit> {
    if field_long(vm, r, "disableDuration") > 0 {
        Err(ise("Already called #disableAutofill()"))
    } else {
        Ok(())
    }
}
fn af_fill_response_throw_if_authentication(vm: &Engine, r: ObjRef) -> Result<(), VmExit> {
    if field_obj(vm, r, "authentication").is_some() {
        Err(ise("Already called #setAuthentication()"))
    } else {
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
fn af_fill_response_set_authentication(
    vm: &mut Engine,
    builder: ObjRef,
    ids: Value,
    authentication: Value,
    presentation: Value,
    inline: Value,
    tooltip: Value,
    dialog: Value,
    dropdown_or_inline: bool,
) -> Result<(), VmExit> {
    af_throw_if_destroyed(vm, builder)?;
    af_fill_response_throw_if_disabled(vm, builder)?;
    if field_obj(vm, builder, "header").is_some() || field_obj(vm, builder, "footer").is_some() {
        return Err(ise("Already called #setHeader() or #setFooter()"));
    }
    let no_presentation = if dropdown_or_inline {
        matches!(presentation, Value::Null) && matches!(inline, Value::Null)
    } else {
        matches!(presentation, Value::Null)
    };
    if matches!(authentication, Value::Null) ^ no_presentation {
        return Err(iae(if dropdown_or_inline {
            "authentication and presentation (dropdown or inline), must be both non-null or null"
        } else {
            "authentication and presentation must be both non-null or null"
        }));
    }
    put(vm, builder, "authentication", authentication)?;
    put(vm, builder, "presentation", presentation)?;
    if dropdown_or_inline {
        put(vm, builder, "inlinePresentation", inline)?;
        put(vm, builder, "inlineTooltipPresentation", tooltip)?;
        put(vm, builder, "dialogPresentation", dialog)?;
    }
    let valid_ids = af_assert_valid_ids(vm, &ids)?;
    put(vm, builder, "authenticationIds", valid_ids)
}

fn af_fill_response_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let ret = Value::Obj(recv);
    let v = match (name, sig) {
        ("<init>", "()V") => {
            af_fill_response_builder_init(vm, recv)?;
            Value::Null
        }
        (
            "setDetectedFieldClassifications",
            "(Ljava/util/Set;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            if oarg(args, 0).is_none() {
                return Err(npe("fieldInfos.toArray()"));
            }
            let items = collection_items(vm, args.first());
            let array = obj_array(
                vm,
                "[Landroid/service/autofill/FieldClassification;",
                items,
            )?;
            put(vm, recv, "detectedFieldTypes", array)?;
            ret
        }
        (
            "setAuthentication",
            "([Landroid/view/autofill/AutofillId;Landroid/content/IntentSender;Landroid/widget/RemoteViews;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_fill_response_set_authentication(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                af_arg_value(args, 2),
                Value::Null,
                Value::Null,
                Value::Null,
                false,
            )?;
            ret
        }
        (
            "setAuthentication",
            "([Landroid/view/autofill/AutofillId;Landroid/content/IntentSender;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_fill_response_set_authentication(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                af_arg_value(args, 2),
                af_arg_value(args, 3),
                Value::Null,
                Value::Null,
                true,
            )?;
            ret
        }
        (
            "setAuthentication",
            "([Landroid/view/autofill/AutofillId;Landroid/content/IntentSender;Landroid/widget/RemoteViews;Landroid/service/autofill/InlinePresentation;Landroid/service/autofill/InlinePresentation;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_fill_response_set_authentication(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                af_arg_value(args, 2),
                af_arg_value(args, 3),
                af_arg_value(args, 4),
                Value::Null,
                true,
            )?;
            ret
        }
        (
            "setAuthentication",
            "([Landroid/view/autofill/AutofillId;Landroid/content/IntentSender;Landroid/service/autofill/Presentations;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            let (menu, inline, tooltip, dialog) = match oarg(args, 2) {
                Some(presentations) => (
                    get(vm, presentations, "menuPresentation"),
                    get(vm, presentations, "inlinePresentation"),
                    get(vm, presentations, "inlineTooltipPresentation"),
                    get(vm, presentations, "dialogPresentation"),
                ),
                None => (Value::Null, Value::Null, Value::Null, Value::Null),
            };
            af_fill_response_set_authentication(
                vm,
                recv,
                af_arg_value(args, 0),
                af_arg_value(args, 1),
                menu,
                inline,
                tooltip,
                dialog,
                true,
            )?;
            ret
        }
        (
            "setIgnoredIds",
            "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "ignoredIds", af_arg_value(args, 0))?;
            ret
        }
        (
            "addDataset",
            "(Landroid/service/autofill/Dataset;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            if oarg(args, 0).is_some() {
                let mut datasets = if field_obj(vm, recv, "datasets").is_some() {
                    af_field_items(vm, recv, "datasets")?
                } else {
                    Vec::new()
                };
                datasets.push(af_arg_value(args, 0));
                af_set_field_items(vm, recv, "datasets", datasets)?;
            }
            ret
        }
        (
            "setDatasets",
            "(Ljava/util/ArrayList;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            // AOSP deliberately performs neither destroyed nor disable check
            // in this hidden setter.
            put(vm, recv, "datasets", af_arg_value(args, 0))?;
            ret
        }
        (
            "setSaveInfo",
            "(Landroid/service/autofill/SaveInfo;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            put(vm, recv, "saveInfo", af_arg_value(args, 0))?;
            ret
        }
        (
            "setClientState",
            "(Landroid/os/Bundle;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            put(vm, recv, "clientState", af_arg_value(args, 0))?;
            ret
        }
        (
            "setFieldClassificationIds",
            "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_disabled(vm, recv)?;
            let ids = af_check_array_elements_not_null(vm, &af_arg_value(args, 0), "ids")?;
            af_check_range(ids.len() as i32, 1, 10, "ids length")?;
            put(vm, recv, "fieldClassificationIds", af_arg_value(args, 0))?;
            let flags = field_int(vm, recv, "flags") | 0x1;
            put(vm, recv, "flags", Value::Int(flags))?;
            ret
        }
        ("setFlags", "(I)Landroid/service/autofill/FillResponse$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            let flags = af_check_flags(iarg(args, 0), 0x1 | 0x2 | 0x4)?;
            put(vm, recv, "flags", Value::Int(flags))?;
            ret
        }
        ("disableAutofill", "(J)Landroid/service/autofill/FillResponse$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            let duration = larg(args, 0);
            if duration <= 0 {
                return Err(iae("duration must be greater than 0"));
            }
            if field_obj(vm, recv, "authentication").is_some()
                || field_obj(vm, recv, "datasets").is_some()
                || field_obj(vm, recv, "saveInfo").is_some()
                || field_obj(vm, recv, "fieldClassificationIds").is_some()
                || field_obj(vm, recv, "clientState").is_some()
            {
                return Err(ise("disableAutofill() must be the only method called"));
            }
            put(vm, recv, "disableDuration", Value::Long(duration))?;
            ret
        }
        ("setIconResourceId", "(I)Landroid/service/autofill/FillResponse$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "iconResourceId", Value::Int(iarg(args, 0)))?;
            ret
        }
        (
            "setServiceDisplayNameResourceId",
            "(I)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            put(
                vm,
                recv,
                "serviceDisplayNameResourceId",
                Value::Int(iarg(args, 0)),
            )?;
            ret
        }
        ("setShowFillDialogIcon", "(Z)Landroid/service/autofill/FillResponse$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(
                vm,
                recv,
                "showFillDialogIcon",
                bool_value(iarg(args, 0) != 0),
            )?;
            ret
        }
        ("setShowSaveDialogIcon", "(Z)Landroid/service/autofill/FillResponse$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(
                vm,
                recv,
                "showSaveDialogIcon",
                bool_value(iarg(args, 0) != 0),
            )?;
            ret
        }
        (
            "setHeader",
            "(Landroid/widget/RemoteViews;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_authentication(vm, recv)?;
            let header = af_req_arg_obj(args, 0, "header")?;
            put(vm, recv, "header", Value::Obj(header))?;
            ret
        }
        (
            "setFooter",
            "(Landroid/widget/RemoteViews;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_authentication(vm, recv)?;
            let footer = af_req_arg_obj(args, 0, "footer")?;
            put(vm, recv, "footer", Value::Obj(footer))?;
            ret
        }
        (
            "setUserData",
            "(Landroid/service/autofill/UserData;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_fill_response_throw_if_authentication(vm, recv)?;
            let user_data = af_req_arg_obj(args, 0, "userData")?;
            put(vm, recv, "userData", Value::Obj(user_data))?;
            ret
        }
        (
            "setPresentationCancelIds",
            "([I)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "cancelIds", af_arg_value(args, 0))?;
            ret
        }
        (
            "setDialogHeader",
            "(Landroid/widget/RemoteViews;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let header = af_req_arg_obj(args, 0, "header")?;
            put(vm, recv, "dialogHeader", Value::Obj(header))?;
            ret
        }
        (
            "setFillDialogTriggerIds",
            "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/FillResponse$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            af_check_array_elements_not_null(vm, &af_arg_value(args, 0), "ids")?;
            put(vm, recv, "fillDialogTriggerIds", af_arg_value(args, 0))?;
            ret
        }
        ("build", "()Landroid/service/autofill/FillResponse;") => {
            af_throw_if_destroyed(vm, recv)?;
            if field_obj(vm, recv, "authentication").is_none()
                && field_obj(vm, recv, "datasets").is_none()
                && field_obj(vm, recv, "saveInfo").is_none()
                && field_long(vm, recv, "disableDuration") == 0
                && field_obj(vm, recv, "fieldClassificationIds").is_none()
                && field_obj(vm, recv, "clientState").is_none()
            {
                return Err(ise(
                    "need to provide: at least one DataSet, or a SaveInfo, or an authentication with a presentation, or a FieldsDetection, or a client state, or disable autofill",
                ));
            }
            if field_obj(vm, recv, "datasets").is_none()
                && (field_obj(vm, recv, "header").is_some()
                    || field_obj(vm, recv, "footer").is_some())
            {
                return Err(ise("must add at least 1 dataset when using header or footer"));
            }
            let supports_inline = if field_obj(vm, recv, "datasets").is_some() {
                let datasets = af_existing_field_items(vm, recv, "datasets");
                let mut supported = false;
                for dataset in datasets {
                    if let Value::Obj(dataset_ref) = dataset {
                        let inline = vm.invoke_instance(
                            dataset_ref,
                            "getFieldInlinePresentation",
                            "(I)Landroid/service/autofill/InlinePresentation;",
                            &[Value::Int(0)],
                        )?;
                        if !matches!(inline, Value::Null) {
                            supported = true;
                            break;
                        }
                    }
                }
                supported
            } else {
                field_obj(vm, recv, "inlinePresentation").is_some()
            };
            put(
                vm,
                recv,
                "supportsInlineSuggestions",
                bool_value(supports_inline),
            )?;
            put(vm, recv, "destroyed", Value::Int(1))?;
            af_fill_response_from_builder(vm, recv)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_is_subtype(vm: &Engine, value: &Value, expected: &str) -> bool {
    match value {
        Value::Obj(r) => vm.cp.is_subtype(&class_of(vm, *r), expected),
        _ => false,
    }
}
fn af_save_info_from_builder(vm: &mut Engine, builder: ObjRef) -> Result<Value, VmExit> {
    let info = alloc_obj(vm, AF_SAVE_INFO, Vec::new())?;
    for field in [
        "type",
        "negativeButtonStyle",
        "positiveButtonStyle",
        "negativeActionListener",
        "requiredIds",
        "optionalIds",
        "description",
        "flags",
        "customDescription",
        "validator",
        "triggerId",
    ] {
        put(vm, info, field, get(vm, builder, field))?;
    }
    if field_obj(vm, builder, "sanitizerKeyList").is_none() {
        put(vm, info, "sanitizerKeys", Value::Null)?;
        put(vm, info, "sanitizerValues", Value::Null)?;
    } else {
        let keys = af_existing_field_items(vm, builder, "sanitizerKeyList");
        let values = af_existing_field_items(vm, builder, "sanitizerValueList");
        let key_array = obj_array(vm, "[Landroid/service/autofill/InternalSanitizer;", keys)?;
        put(vm, info, "sanitizerKeys", key_array)?;
        let value_array = obj_array(vm, "[[Landroid/view/autofill/AutofillId;", values)?;
        put(vm, info, "sanitizerValues", value_array)?;
    }
    Ok(Value::Obj(info))
}
fn af_save_info_copy_static(vm: &mut Engine, args: &[Value]) -> Result<Value, VmExit> {
    let source = af_req_arg_obj(args, 0, "saveInfo")?;
    let optional = af_assert_valid_ids(vm, &af_arg_value(args, 1))?;
    let info = alloc_obj(vm, AF_SAVE_INFO, Vec::new())?;
    for field in [
        "type",
        "negativeButtonStyle",
        "positiveButtonStyle",
        "negativeActionListener",
        "requiredIds",
        "description",
        "flags",
        "customDescription",
        "validator",
        "sanitizerKeys",
        "sanitizerValues",
        "triggerId",
    ] {
        put(vm, info, field, get(vm, source, field))?;
    }
    put(vm, info, "optionalIds", optional)?;
    Ok(Value::Obj(info))
}
fn af_save_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/SaveInfo$Builder;)V") => {
            let builder = af_req_arg_obj(args, 0, "builder")?;
            let built = match af_save_info_from_builder(vm, builder)? {
                Value::Obj(r) => r,
                _ => return Err(ise("SaveInfo construction failed")),
            };
            for field in [
                "type",
                "negativeButtonStyle",
                "positiveButtonStyle",
                "negativeActionListener",
                "requiredIds",
                "optionalIds",
                "description",
                "flags",
                "customDescription",
                "validator",
                "sanitizerKeys",
                "sanitizerValues",
                "triggerId",
            ] {
                put(vm, recv, field, get(vm, built, field))?;
            }
            Value::Null
        }
        ("getNegativeActionStyle", "()I") => Value::Int(field_int(vm, recv, "negativeButtonStyle")),
        ("getNegativeActionListener", "()Landroid/content/IntentSender;") => {
            get(vm, recv, "negativeActionListener")
        }
        ("getPositiveActionStyle", "()I") => Value::Int(field_int(vm, recv, "positiveButtonStyle")),
        ("getRequiredIds", "()[Landroid/view/autofill/AutofillId;") => get(vm, recv, "requiredIds"),
        ("getOptionalIds", "()[Landroid/view/autofill/AutofillId;") => get(vm, recv, "optionalIds"),
        ("getType", "()I") => Value::Int(field_int(vm, recv, "type")),
        ("getFlags", "()I") => Value::Int(field_int(vm, recv, "flags")),
        ("getDescription", "()Ljava/lang/CharSequence;") => get(vm, recv, "description"),
        ("getCustomDescription", "()Landroid/service/autofill/CustomDescription;") => {
            get(vm, recv, "customDescription")
        }
        ("getValidator", "()Landroid/service/autofill/InternalValidator;") => {
            get(vm, recv, "validator")
        }
        ("getSanitizerKeys", "()[Landroid/service/autofill/InternalSanitizer;") => {
            get(vm, recv, "sanitizerKeys")
        }
        ("getSanitizerValues", "()[[Landroid/view/autofill/AutofillId;") => {
            get(vm, recv, "sanitizerValues")
        }
        ("getTriggerId", "()Landroid/view/autofill/AutofillId;") => get(vm, recv, "triggerId"),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_save_info_builder_init(
    vm: &mut Engine,
    r: ObjRef,
    kind: i32,
    required_ids: Value,
) -> Result<(), VmExit> {
    put(vm, r, "type", Value::Int(kind))?;
    put(vm, r, "negativeButtonStyle", Value::Int(0))?;
    put(vm, r, "positiveButtonStyle", Value::Int(0))?;
    put(vm, r, "negativeActionListener", Value::Null)?;
    put(vm, r, "requiredIds", required_ids)?;
    put(vm, r, "optionalIds", Value::Null)?;
    put(vm, r, "description", Value::Null)?;
    put(vm, r, "destroyed", Value::Int(0))?;
    put(vm, r, "flags", Value::Int(0))?;
    put(vm, r, "customDescription", Value::Null)?;
    put(vm, r, "validator", Value::Null)?;
    put(vm, r, "sanitizerKeyList", Value::Null)?;
    put(vm, r, "sanitizerValueList", Value::Null)?;
    put(vm, r, "sanitizerIdList", Value::Null)?;
    put(vm, r, "triggerId", Value::Null)
}
fn af_save_info_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let ret = Value::Obj(recv);
    let v = match (name, sig) {
        ("<init>", "(I[Landroid/view/autofill/AutofillId;)V") => {
            let required = af_assert_valid_ids(vm, &af_arg_value(args, 1))?;
            af_save_info_builder_init(vm, recv, iarg(args, 0), required)?;
            Value::Null
        }
        ("<init>", "(I)V") => {
            af_save_info_builder_init(vm, recv, iarg(args, 0), Value::Null)?;
            Value::Null
        }
        ("setFlags", "(I)Landroid/service/autofill/SaveInfo$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            let flags = af_check_flags(iarg(args, 0), 0x1 | 0x2 | 0x4)?;
            put(vm, recv, "flags", Value::Int(flags))?;
            ret
        }
        (
            "setOptionalIds",
            "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let ids = af_assert_valid_ids(vm, &af_arg_value(args, 0))?;
            put(vm, recv, "optionalIds", ids)?;
            ret
        }
        (
            "setDescription",
            "(Ljava/lang/CharSequence;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            if field_obj(vm, recv, "customDescription").is_some() {
                return Err(ise(
                    "Can call setDescription() or setCustomDescription(), but not both",
                ));
            }
            put(vm, recv, "description", af_arg_value(args, 0))?;
            ret
        }
        (
            "setCustomDescription",
            "(Landroid/service/autofill/CustomDescription;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            if field_obj(vm, recv, "description").is_some() {
                return Err(ise(
                    "Can call setDescription() or setCustomDescription(), but not both",
                ));
            }
            put(vm, recv, "customDescription", af_arg_value(args, 0))?;
            ret
        }
        (
            "setNegativeAction",
            "(ILandroid/content/IntentSender;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let style = af_check_range(iarg(args, 0), 0, 2, "style")?;
            put(vm, recv, "negativeButtonStyle", Value::Int(style))?;
            put(vm, recv, "negativeActionListener", af_arg_value(args, 1))?;
            ret
        }
        ("setPositiveAction", "(I)Landroid/service/autofill/SaveInfo$Builder;") => {
            af_throw_if_destroyed(vm, recv)?;
            let style = af_check_range(iarg(args, 0), 0, 1, "style")?;
            put(vm, recv, "positiveButtonStyle", Value::Int(style))?;
            ret
        }
        (
            "setValidator",
            "(Landroid/service/autofill/Validator;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let validator = af_arg_value(args, 0);
            if !af_is_subtype(vm, &validator, AF_INTERNAL_VALIDATOR) {
                let text = value_string(vm, &validator)?;
                return Err(iae(format!("not provided by Android System: {text}")));
            }
            put(vm, recv, "validator", validator)?;
            ret
        }
        (
            "addSanitizer",
            "(Landroid/service/autofill/Sanitizer;[Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let ids_value = af_arg_value(args, 1);
            let ids = array_elems(vm, Some(&ids_value));
            if ids.is_empty() {
                return Err(iae("ids cannot be empty or null"));
            }
            let sanitizer = af_arg_value(args, 0);
            if !af_is_subtype(vm, &sanitizer, AF_INTERNAL_SANITIZER) {
                let text = value_string(vm, &sanitizer)?;
                return Err(iae(format!("not provided by Android System: {text}")));
            }
            if field_obj(vm, recv, "sanitizerKeyList").is_none() {
                af_init_empty_list(vm, recv, "sanitizerKeyList")?;
                af_init_empty_list(vm, recv, "sanitizerValueList")?;
                af_init_empty_list(vm, recv, "sanitizerIdList")?;
            }
            for id in &ids {
                let mut seen = af_field_items(vm, recv, "sanitizerIdList")?;
                let duplicate = match id {
                    Value::Obj(candidate) if class_of(vm, *candidate) == AUTOFILL_ID => {
                        seen.iter().any(|existing| match existing {
                            Value::Obj(other) if class_of(vm, *other) == AUTOFILL_ID => {
                                af_id_equals(vm, *candidate, *other, false)
                            }
                            _ => false,
                        })
                    }
                    _ => seen.contains(id),
                };
                if duplicate {
                    let text = value_string(vm, id)?;
                    return Err(iae(format!("already added {text}")));
                }
                seen.push(id.clone());
                af_set_field_items(vm, recv, "sanitizerIdList", seen)?;
            }
            let mut keys = af_field_items(vm, recv, "sanitizerKeyList")?;
            let mut values = af_field_items(vm, recv, "sanitizerValueList")?;
            if let Some(pos) = keys.iter().position(|key| key == &sanitizer) {
                af_dataset_set_at(&mut values, pos, ids_value);
            } else {
                keys.push(sanitizer);
                values.push(ids_value);
            }
            af_set_field_items(vm, recv, "sanitizerKeyList", keys)?;
            af_set_field_items(vm, recv, "sanitizerValueList", values)?;
            ret
        }
        (
            "setTriggerId",
            "(Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let id = af_req_arg_obj(args, 0, "id")?;
            put(vm, recv, "triggerId", Value::Obj(id))?;
            ret
        }
        ("build", "()Landroid/service/autofill/SaveInfo;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "destroyed", Value::Int(1))?;
            af_save_info_from_builder(vm, recv)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_new_array_map(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = alloc_obj(vm, "Landroid/util/ArrayMap;", Vec::new())?;
    vm.fw.objects.insert(
        r,
        HostObj::Map {
            entries: Vec::new(),
        },
    );
    Ok(r)
}
fn af_map_put(
    vm: &mut Engine,
    owner: ObjRef,
    field: &str,
    key: Value,
    value: Value,
) -> Result<(), VmExit> {
    let map = match field_obj(vm, owner, field) {
        Some(map) => map,
        None => {
            let map = af_new_array_map(vm)?;
            put(vm, owner, field, Value::Obj(map))?;
            map
        }
    };
    let mut entries = match vm.fw.objects.get(&map) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    };
    if let Some(pos) = entries
        .iter()
        .position(|(existing, _)| objects_equal(vm, existing, &key))
    {
        entries[pos].1 = value;
    } else {
        entries.push((key, value));
    }
    vm.fw.objects.insert(map, HostObj::Map { entries });
    Ok(())
}
fn af_map_get(vm: &Engine, map_value: &Value, key: &Value) -> Option<Value> {
    let map = match map_value {
        Value::Obj(r) => *r,
        _ => return None,
    };
    match vm.fw.objects.get(&map) {
        Some(HostObj::Map { entries }) => entries
            .iter()
            .find(|(existing, _)| objects_equal(vm, existing, key))
            .map(|(_, value)| value.clone()),
        _ => None,
    }
}
fn af_user_data_check_not_empty(
    vm: &Engine,
    value: Option<&Value>,
    name: &str,
) -> Result<(Value, String), VmExit> {
    let value = value.cloned().unwrap_or(Value::Null);
    let text = opt_str(vm, Some(&value)).ok_or_else(|| npe(name))?;
    if text.is_empty() {
        return Err(iae(format!("{name} cannot be empty")));
    }
    Ok((value, text))
}
fn af_user_data_check_value(vm: &Engine, value: Option<&Value>) -> Result<(Value, String), VmExit> {
    let value = value.cloned().unwrap_or(Value::Null);
    let text = opt_str(vm, Some(&value)).ok_or_else(|| npe("value"))?;
    let length = text.encode_utf16().count() as i32;
    af_check_range(length, 3, 100, &format!("value length ({length})"))?;
    Ok((value, text))
}
fn af_user_data_add_mapping(
    vm: &mut Engine,
    builder: ObjRef,
    value: Value,
    value_text: &str,
    category: Value,
    category_text: &str,
) -> Result<(), VmExit> {
    let pair = format!("{value_text}:{category_text}");
    let pairs = af_field_items(vm, builder, "uniqueValueCategoryPairs")?;
    if pairs
        .iter()
        .any(|existing| opt_str(vm, Some(existing)).as_deref() == Some(&pair))
    {
        return Ok(());
    }
    let mut categories = af_field_items(vm, builder, "categoryIds")?;
    categories.push(category);
    af_set_field_items(vm, builder, "categoryIds", categories)?;
    let mut values = af_field_items(vm, builder, "values")?;
    values.push(value);
    af_set_field_items(vm, builder, "values", values)?;
    let mut unique_categories = af_field_items(vm, builder, "uniqueCategoryIds")?;
    if !unique_categories
        .iter()
        .any(|existing| opt_str(vm, Some(existing)).as_deref() == Some(category_text))
    {
        let category_value = jstr(vm, category_text.to_string())?;
        unique_categories.push(category_value);
        af_set_field_items(vm, builder, "uniqueCategoryIds", unique_categories)?;
    }
    let mut unique_pairs = pairs;
    unique_pairs.push(jstr(vm, pair)?);
    af_set_field_items(vm, builder, "uniqueValueCategoryPairs", unique_pairs)
}
fn af_user_data_from_builder(vm: &mut Engine, builder: ObjRef) -> Result<Value, VmExit> {
    let user_data = alloc_obj(vm, AF_USER_DATA, Vec::new())?;
    put(vm, user_data, "id", get(vm, builder, "id"))?;
    let categories = af_existing_field_items(vm, builder, "categoryIds");
    let values = af_existing_field_items(vm, builder, "values");
    let category_array = af_string_array(vm, categories)?;
    put(vm, user_data, "categoryIds", category_array)?;
    let value_array = af_string_array(vm, values)?;
    put(vm, user_data, "values", value_array)?;
    put(
        vm,
        user_data,
        "defaultAlgorithm",
        get(vm, builder, "defaultAlgorithm"),
    )?;
    put(
        vm,
        user_data,
        "defaultArgs",
        get(vm, builder, "defaultArgs"),
    )?;
    put(
        vm,
        user_data,
        "categoryAlgorithms",
        get(vm, builder, "categoryAlgorithms"),
    )?;
    put(
        vm,
        user_data,
        "categoryArgs",
        get(vm, builder, "categoryArgs"),
    )?;
    Ok(Value::Obj(user_data))
}
fn af_user_data_static(
    _vm: &mut Engine,
    name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(Value::Int(match name {
        "getMaxUserDataSize" => 50,
        "getMaxFieldClassificationIdsSize" => 10,
        "getMaxCategoryCount" => 10,
        "getMinValueLength" => 3,
        "getMaxValueLength" => 100,
        _ => return Ok(None),
    })))
}
fn af_user_data_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/UserData$Builder;)V") => {
            let builder = af_req_arg_obj(args, 0, "builder")?;
            let built = match af_user_data_from_builder(vm, builder)? {
                Value::Obj(r) => r,
                _ => return Err(ise("UserData construction failed")),
            };
            for field in [
                "id",
                "categoryIds",
                "values",
                "defaultAlgorithm",
                "defaultArgs",
                "categoryAlgorithms",
                "categoryArgs",
            ] {
                put(vm, recv, field, get(vm, built, field))?;
            }
            Value::Null
        }
        ("getFieldClassificationAlgorithm", "()Ljava/lang/String;") => {
            get(vm, recv, "defaultAlgorithm")
        }
        ("getDefaultFieldClassificationArgs", "()Landroid/os/Bundle;") => {
            get(vm, recv, "defaultArgs")
        }
        (
            "getFieldClassificationAlgorithmForCategory",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ) => {
            if oarg(args, 0).is_none() {
                return Err(npe("categoryId"));
            }
            af_map_get(
                vm,
                &get(vm, recv, "categoryAlgorithms"),
                &af_arg_value(args, 0),
            )
            .unwrap_or(Value::Null)
        }
        ("getId", "()Ljava/lang/String;") => get(vm, recv, "id"),
        ("getCategoryIds", "()[Ljava/lang/String;") => get(vm, recv, "categoryIds"),
        ("getValues", "()[Ljava/lang/String;") => get(vm, recv, "values"),
        ("getFieldClassificationAlgorithms", "()Landroid/util/ArrayMap;") => {
            get(vm, recv, "categoryAlgorithms")
        }
        ("getFieldClassificationArgs", "()Landroid/util/ArrayMap;") => {
            get(vm, recv, "categoryArgs")
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_user_data_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let ret = Value::Obj(recv);
    let v = match (name, sig) {
        (
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        ) => {
            let (id, _id_text) = af_user_data_check_not_empty(vm, args.first(), "id")?;
            let (category, category_text) =
                af_user_data_check_not_empty(vm, args.get(2), "categoryId")?;
            let (value, value_text) = af_user_data_check_value(vm, args.get(1))?;
            put(vm, recv, "id", id)?;
            put(vm, recv, "destroyed", Value::Int(0))?;
            put(vm, recv, "defaultAlgorithm", Value::Null)?;
            put(vm, recv, "defaultArgs", Value::Null)?;
            put(vm, recv, "categoryAlgorithms", Value::Null)?;
            put(vm, recv, "categoryArgs", Value::Null)?;
            for field in [
                "categoryIds",
                "values",
                "uniqueCategoryIds",
                "uniqueValueCategoryPairs",
            ] {
                af_init_empty_list(vm, recv, field)?;
            }
            af_user_data_add_mapping(vm, recv, value, &value_text, category, &category_text)?;
            Value::Null
        }
        (
            "setFieldClassificationAlgorithm",
            "(Ljava/lang/String;Landroid/os/Bundle;)Landroid/service/autofill/UserData$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "defaultAlgorithm", af_arg_value(args, 0))?;
            put(vm, recv, "defaultArgs", af_arg_value(args, 1))?;
            ret
        }
        (
            "setFieldClassificationAlgorithmForCategory",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)Landroid/service/autofill/UserData$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            if oarg(args, 0).is_none() {
                return Err(npe("categoryId"));
            }
            let category = af_arg_value(args, 0);
            af_map_put(
                vm,
                recv,
                "categoryAlgorithms",
                category.clone(),
                af_arg_value(args, 1),
            )?;
            af_map_put(vm, recv, "categoryArgs", category, af_arg_value(args, 2))?;
            ret
        }
        (
            "add",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/service/autofill/UserData$Builder;",
        ) => {
            af_throw_if_destroyed(vm, recv)?;
            let (category, category_text) =
                af_user_data_check_not_empty(vm, args.get(1), "categoryId")?;
            let (value, value_text) = af_user_data_check_value(vm, args.first())?;
            let unique_categories = af_field_items(vm, recv, "uniqueCategoryIds")?;
            if !unique_categories
                .iter()
                .any(|existing| opt_str(vm, Some(existing)).as_deref() == Some(&category_text))
                && unique_categories.len() >= 10
            {
                return Err(ise(format!(
                    "already added {} unique category ids",
                    unique_categories.len()
                )));
            }
            let values = af_field_items(vm, recv, "values")?;
            if values.len() >= 50 {
                return Err(ise(format!("already added {} elements", values.len())));
            }
            af_user_data_add_mapping(vm, recv, value, &value_text, category, &category_text)?;
            ret
        }
        ("build", "()Landroid/service/autofill/UserData;") => {
            af_throw_if_destroyed(vm, recv)?;
            put(vm, recv, "destroyed", Value::Int(1))?;
            af_user_data_from_builder(vm, recv)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_validators_static(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match name {
        "and" | "or" => {
            let validators =
                af_check_array_elements_not_null(vm, &af_arg_value(args, 0), "validators")?;
            for (i, validator) in validators.iter().enumerate() {
                if !af_is_subtype(vm, validator, AF_INTERNAL_VALIDATOR) {
                    let text = value_string(vm, validator)?;
                    return Err(iae(format!(
                        "element {i} not provided by Android System: {text}"
                    )));
                }
            }
            let class = if name == "and" {
                AF_REQUIRED_VALIDATORS
            } else {
                AF_OPTIONAL_VALIDATORS
            };
            let r = alloc_obj(vm, class, Vec::new())?;
            put(vm, r, "validators", af_arg_value(args, 0))?;
            Value::Obj(r)
        }
        "not" => {
            let validator = af_arg_value(args, 0);
            if !af_is_subtype(vm, &validator, AF_INTERNAL_VALIDATOR) {
                let text = value_string(vm, &validator)?;
                return Err(iae(format!(
                    "validator not provided by Android System: {text}"
                )));
            }
            let r = alloc_obj(vm, AF_NEGATION_VALIDATOR, Vec::new())?;
            put(vm, r, "validator", validator)?;
            Value::Obj(r)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_finder_find(vm: &mut Engine, finder: ObjRef, id: Value) -> Result<Option<String>, VmExit> {
    match vm.invoke_instance(
        finder,
        "findByAutofillId",
        "(Landroid/view/autofill/AutofillId;)Ljava/lang/String;",
        std::slice::from_ref(&id),
    ) {
        Ok(value) => Ok(opt_str(vm, Some(&value))),
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {
            let raw = vm.invoke_instance(
                finder,
                "findRawValueByAutofillId",
                "(Landroid/view/autofill/AutofillId;)Landroid/view/autofill/AutofillValue;",
                &[id],
            )?;
            match raw {
                Value::Obj(value_ref) if field_int(vm, value_ref, "type") == AF_TYPE_TEXT => {
                    charseq(vm, Some(&get(vm, value_ref, "value")))
                }
                _ => Ok(None),
            }
        }
        Err(error) => Err(error),
    }
}
fn af_pattern_matches(vm: &mut Engine, pattern: ObjRef, text: &str) -> Result<bool, VmExit> {
    let text_value = jstr(vm, text.to_string())?;
    let matcher = vm.invoke_instance(
        pattern,
        "matcher",
        "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        &[text_value],
    )?;
    let matcher = match matcher {
        Value::Obj(r) => r,
        _ => return Err(npe("pattern.matcher()")),
    };
    Ok(vm
        .invoke_instance(matcher, "matches", "()Z", &[])?
        .as_int()
        .unwrap_or(0)
        != 0)
}
fn af_validator_is_valid(
    vm: &mut Engine,
    class: &str,
    recv: ObjRef,
    finder_value: Value,
) -> Result<bool, VmExit> {
    match class {
        AF_REGEX_VALIDATOR => {
            let finder = match finder_value {
                Value::Obj(r) => r,
                _ => return Err(npe("finder.findByAutofillId()")),
            };
            let Some(text) = af_finder_find(vm, finder, get(vm, recv, "id"))? else {
                return Ok(false);
            };
            let pattern = field_obj(vm, recv, "regex").ok_or_else(|| npe("regex"))?;
            af_pattern_matches(vm, pattern, &text)
        }
        AF_REQUIRED_VALIDATORS => {
            for validator in array_elems(vm, Some(&get(vm, recv, "validators"))) {
                let validator_ref = match validator {
                    Value::Obj(r) => r,
                    _ => return Err(npe("validator.isValid()")),
                };
                let valid = vm.invoke_instance(
                    validator_ref,
                    "isValid",
                    "(Landroid/service/autofill/ValueFinder;)Z",
                    std::slice::from_ref(&finder_value),
                )?;
                if valid.as_int().unwrap_or(0) == 0 {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        AF_OPTIONAL_VALIDATORS => {
            for validator in array_elems(vm, Some(&get(vm, recv, "validators"))) {
                let validator_ref = match validator {
                    Value::Obj(r) => r,
                    _ => return Err(npe("validator.isValid()")),
                };
                let valid = vm.invoke_instance(
                    validator_ref,
                    "isValid",
                    "(Landroid/service/autofill/ValueFinder;)Z",
                    std::slice::from_ref(&finder_value),
                )?;
                if valid.as_int().unwrap_or(0) != 0 {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        AF_NEGATION_VALIDATOR => {
            let validator = field_obj(vm, recv, "validator").ok_or_else(|| npe("validator"))?;
            let valid = vm.invoke_instance(
                validator,
                "isValid",
                "(Landroid/service/autofill/ValueFinder;)Z",
                &[finder_value],
            )?;
            Ok(valid.as_int().unwrap_or(0) == 0)
        }
        _ => Ok(false),
    }
}
fn af_validator_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (class, name, sig) {
        (
            AF_REGEX_VALIDATOR,
            "<init>",
            "(Landroid/view/autofill/AutofillId;Ljava/util/regex/Pattern;)V",
        ) => {
            let id = af_req_arg_obj(args, 0, "id")?;
            let regex = af_req_arg_obj(args, 1, "regex")?;
            put(vm, recv, "id", Value::Obj(id))?;
            put(vm, recv, "regex", Value::Obj(regex))?;
            Value::Null
        }
        (
            AF_REQUIRED_VALIDATORS | AF_OPTIONAL_VALIDATORS,
            "<init>",
            "([Landroid/service/autofill/InternalValidator;)V",
        ) => {
            af_check_array_elements_not_null(vm, &af_arg_value(args, 0), "validators")?;
            put(vm, recv, "validators", af_arg_value(args, 0))?;
            Value::Null
        }
        (AF_NEGATION_VALIDATOR, "<init>", "(Landroid/service/autofill/InternalValidator;)V") => {
            let validator = af_req_arg_obj(args, 0, "validator")?;
            put(vm, recv, "validator", Value::Obj(validator))?;
            Value::Null
        }
        (_, "isValid", "(Landroid/service/autofill/ValueFinder;)Z") => bool_value(
            af_validator_is_valid(vm, class, recv, af_arg_value(args, 0))?,
        ),
        (_, "describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_sanitizer_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (class, name, sig) {
        (AF_TEXT_SANITIZER, "<init>", "(Ljava/util/regex/Pattern;Ljava/lang/String;)V") => {
            let regex = af_req_arg_obj(args, 0, "regex")?;
            let subst = af_req_arg_obj(args, 1, "subst")?;
            put(vm, recv, "regex", Value::Obj(regex))?;
            put(vm, recv, "subst", Value::Obj(subst))?;
            Value::Null
        }
        (AF_DATE_SANITIZER, "<init>", "(Landroid/icu/text/DateFormat;)V") => {
            let format = af_req_arg_obj(args, 0, "dateFormat")?;
            put(vm, recv, "dateFormat", Value::Obj(format))?;
            Value::Null
        }
        (
            AF_TEXT_SANITIZER,
            "sanitize",
            "(Landroid/view/autofill/AutofillValue;)Landroid/view/autofill/AutofillValue;",
        ) => {
            let Some(value_ref) = oarg(args, 0) else {
                return Ok(Some(Value::Null));
            };
            if class_of(vm, value_ref) != AUTOFILL_VALUE
                || field_int(vm, value_ref, "type") != AF_TYPE_TEXT
            {
                return Ok(Some(Value::Null));
            }
            let Some(text) = charseq(vm, Some(&get(vm, value_ref, "value")))? else {
                return Ok(Some(Value::Null));
            };
            let Some(pattern) = field_obj(vm, recv, "regex") else {
                return Ok(Some(Value::Null));
            };
            let subst = get(vm, recv, "subst");
            let result = (|| -> Result<Value, VmExit> {
                let text_value = jstr(vm, text.clone())?;
                let matcher_value = vm.invoke_instance(
                    pattern,
                    "matcher",
                    "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
                    &[text_value],
                )?;
                let matcher = match matcher_value {
                    Value::Obj(r) => r,
                    _ => return Ok(Value::Null),
                };
                if vm
                    .invoke_instance(matcher, "matches", "()Z", &[])?
                    .as_int()
                    .unwrap_or(0)
                    == 0
                {
                    return Ok(Value::Null);
                }
                let sanitized = vm.invoke_instance(
                    matcher,
                    "replaceAll",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[subst],
                )?;
                if matches!(sanitized, Value::Null) {
                    Ok(Value::Null)
                } else {
                    af_value_new(vm, AF_TYPE_TEXT, sanitized)
                }
            })();
            match result {
                Ok(value) => value,
                Err(VmExit::Exception(_)) => Value::Null,
                Err(error) => return Err(error),
            }
        }
        (
            AF_DATE_SANITIZER,
            "sanitize",
            "(Landroid/view/autofill/AutofillValue;)Landroid/view/autofill/AutofillValue;",
        ) => {
            let Some(value_ref) = oarg(args, 0) else {
                return Ok(Some(Value::Null));
            };
            if class_of(vm, value_ref) != AUTOFILL_VALUE
                || field_int(vm, value_ref, "type") != AF_TYPE_DATE
            {
                return Ok(Some(Value::Null));
            }
            let Some(format) = field_obj(vm, recv, "dateFormat") else {
                return Ok(Some(Value::Null));
            };
            let millis = af_value_unbox(vm, &get(vm, value_ref, "value"))
                .as_long()
                .unwrap_or(0);
            let result = (|| -> Result<Value, VmExit> {
                let date = alloc_obj(vm, "Ljava/util/Date;", Vec::new())?;
                vm.invoke_instance(date, "<init>", "(J)V", &[Value::Long(millis)])?;
                let converted = vm.invoke_instance(
                    format,
                    "format",
                    "(Ljava/util/Date;)Ljava/lang/String;",
                    &[Value::Obj(date)],
                )?;
                let parsed_value = vm.invoke_instance(
                    format,
                    "parse",
                    "(Ljava/lang/String;)Ljava/util/Date;",
                    &[converted],
                )?;
                let parsed = match parsed_value {
                    Value::Obj(r) => r,
                    _ => return Err(npe("dateFormat.parse()")),
                };
                let sanitized = vm
                    .invoke_instance(parsed, "getTime", "()J", &[])?
                    .as_long()
                    .unwrap_or(0);
                let boxed =
                    crate::boxes::box_value(vm, "Ljava/lang/Long;", Value::Long(sanitized))?;
                af_value_new(vm, AF_TYPE_DATE, Value::Obj(boxed))
            })();
            match result {
                Ok(value) => value,
                Err(VmExit::Exception(_)) => Value::Null,
                Err(error) => return Err(error),
            }
        }
        (_, "describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn af_fill_request_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        (
            "<init>",
            "(ILjava/util/List;Ljava/util/List;Landroid/os/Bundle;ILandroid/view/inputmethod/InlineSuggestionsRequest;Landroid/content/IntentSender;)V",
        ) => {
            if oarg(args, 1).is_none() {
                return Err(npe("fillContexts must not be null"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("hints must not be null"));
            }
            let flags = af_check_flags(
                iarg(args, 4),
                0x1 | 0x2 | 0x4 | 0x10 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400,
            )?;
            put(vm, recv, "id", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "fillContexts", af_arg_value(args, 1))?;
            put(vm, recv, "hints", af_arg_value(args, 2))?;
            put(vm, recv, "clientState", af_arg_value(args, 3))?;
            put(vm, recv, "flags", Value::Int(flags))?;
            put(vm, recv, "inlineSuggestionsRequest", af_arg_value(args, 5))?;
            put(vm, recv, "delayedFillIntentSender", af_arg_value(args, 6))?;
            Value::Null
        }
        ("getId", "()I") => Value::Int(field_int(vm, recv, "id")),
        ("getFillContexts", "()Ljava/util/List;") => get(vm, recv, "fillContexts"),
        ("getHints", "()Ljava/util/List;") => get(vm, recv, "hints"),
        ("getClientState", "()Landroid/os/Bundle;") => get(vm, recv, "clientState"),
        ("getFlags", "()I") => Value::Int(field_int(vm, recv, "flags")),
        (
            "getInlineSuggestionsRequest",
            "()Landroid/view/inputmethod/InlineSuggestionsRequest;",
        ) => get(vm, recv, "inlineSuggestionsRequest"),
        ("getDelayedFillIntentSender", "()Landroid/content/IntentSender;") => {
            get(vm, recv, "delayedFillIntentSender")
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_save_request_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/util/ArrayList;Landroid/os/Bundle;Ljava/util/ArrayList;)V") => {
            let contexts = af_req_arg_obj(args, 0, "fillContexts")?;
            put(vm, recv, "fillContexts", Value::Obj(contexts))?;
            put(vm, recv, "clientState", af_arg_value(args, 1))?;
            put(vm, recv, "datasetIds", af_arg_value(args, 2))?;
            Value::Null
        }
        ("getFillContexts", "()Ljava/util/List;") => get(vm, recv, "fillContexts"),
        ("getClientState", "()Landroid/os/Bundle;") => get(vm, recv, "clientState"),
        ("getDatasetIds", "()Ljava/util/List;") => get(vm, recv, "datasetIds"),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_fill_context_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        (
            "<init>",
            "(ILandroid/app/assist/AssistStructure;Landroid/view/autofill/AutofillId;)V",
        ) => {
            if oarg(args, 1).is_none() {
                return Err(npe("structure must not be null"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("focusedId must not be null"));
            }
            put(vm, recv, "requestId", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "structure", af_arg_value(args, 1))?;
            put(vm, recv, "focusedId", af_arg_value(args, 2))?;
            Value::Null
        }
        ("getRequestId", "()I") => Value::Int(field_int(vm, recv, "requestId")),
        ("getStructure", "()Landroid/app/assist/AssistStructure;") => get(vm, recv, "structure"),
        ("getFocusedId", "()Landroid/view/autofill/AutofillId;") => get(vm, recv, "focusedId"),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_callback_assert_not_called(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, r, "called") {
        return Err(ise("Already called"));
    }
    put(vm, r, "called", Value::Int(1))
}
fn af_fill_callback_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/IFillCallback;I)V") => {
            put(vm, recv, "callback", af_arg_value(args, 0))?;
            put(vm, recv, "requestId", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "called", Value::Int(0))?;
            put(vm, recv, "response", Value::Null)?;
            put(vm, recv, "failure", Value::Null)?;
            Value::Null
        }
        ("onSuccess", "(Landroid/service/autofill/FillResponse;)V") => {
            af_callback_assert_not_called(vm, recv)?;
            let response = af_arg_value(args, 0);
            if let Value::Obj(response_ref) = response {
                put(
                    vm,
                    response_ref,
                    "requestId",
                    Value::Int(field_int(vm, recv, "requestId")),
                )?;
            }
            put(vm, recv, "response", response.clone())?;
            if let Some(callback) = field_obj(vm, recv, "callback") {
                vm.invoke_instance(
                    callback,
                    "onSuccess",
                    "(Landroid/service/autofill/FillResponse;)V",
                    &[response],
                )?;
            }
            Value::Null
        }
        ("onFailure", "(Ljava/lang/CharSequence;)V") => {
            af_callback_assert_not_called(vm, recv)?;
            let message = af_arg_value(args, 0);
            put(vm, recv, "failure", message.clone())?;
            if let Some(callback) = field_obj(vm, recv, "callback") {
                vm.invoke_instance(
                    callback,
                    "onFailure",
                    "(ILjava/lang/CharSequence;)V",
                    &[Value::Int(field_int(vm, recv, "requestId")), message],
                )?;
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_save_callback_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/service/autofill/ISaveCallback;)V") => {
            put(vm, recv, "callback", af_arg_value(args, 0))?;
            put(vm, recv, "called", Value::Int(0))?;
            put(vm, recv, "intentSender", Value::Null)?;
            put(vm, recv, "failure", Value::Null)?;
            Value::Null
        }
        ("onSuccess", "()V") => {
            af_callback_assert_not_called(vm, recv)?;
            if let Some(callback) = field_obj(vm, recv, "callback") {
                vm.invoke_instance(
                    callback,
                    "onSuccess",
                    "(Landroid/content/IntentSender;)V",
                    &[Value::Null],
                )?;
            }
            Value::Null
        }
        ("onSuccess", "(Landroid/content/IntentSender;)V") => {
            // AOSP evaluates Objects.requireNonNull before the one-shot check.
            let sender = af_req_arg_obj(args, 0, "intentSender")?;
            af_callback_assert_not_called(vm, recv)?;
            put(vm, recv, "intentSender", Value::Obj(sender))?;
            if let Some(callback) = field_obj(vm, recv, "callback") {
                vm.invoke_instance(
                    callback,
                    "onSuccess",
                    "(Landroid/content/IntentSender;)V",
                    &[Value::Obj(sender)],
                )?;
            }
            Value::Null
        }
        ("onFailure", "(Ljava/lang/CharSequence;)V") => {
            af_callback_assert_not_called(vm, recv)?;
            let message = af_arg_value(args, 0);
            put(vm, recv, "failure", message.clone())?;
            if let Some(callback) = field_obj(vm, recv, "callback") {
                vm.invoke_instance(
                    callback,
                    "onFailure",
                    "(Ljava/lang/CharSequence;)V",
                    &[message],
                )?;
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_field_classification_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/util/ArrayList;)V") => {
            let matches_value = af_arg_value(args, 0);
            let matches_ref = af_req_arg_obj(args, 0, "matches")?;
            let mut matches = collection_items(vm, Some(&matches_value));
            matches.sort_by(|a, b| {
                let score = |value: &Value| match value {
                    Value::Obj(r) => field_float(vm, *r, "score"),
                    _ => 0.0,
                };
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&matches_ref) {
                *elems = matches;
            }
            put(vm, recv, "matches", matches_value)?;
            Value::Null
        }
        ("getMatches", "()Ljava/util/List;") => get(vm, recv, "matches"),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_field_classification_match_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/lang/String;F)V") => {
            let category = af_req_arg_obj(args, 0, "categoryId")?;
            put(vm, recv, "categoryId", Value::Obj(category))?;
            put(vm, recv, "score", Value::Float(farg(args, 1)))?;
            Value::Null
        }
        ("getCategoryId", "()Ljava/lang/String;") => get(vm, recv, "categoryId"),
        ("getScore", "()F") => Value::Float(field_float(vm, recv, "score")),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_saved_datasets_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/lang/String;I)V") => {
            let kind = opt_str(vm, args.first());
            if kind.as_deref() != Some("other") && kind.as_deref() != Some("passwords") {
                return Err(iae(format!(
                    "type was {} but must be one of: TYPE_OTHER(other), TYPE_PASSWORDS(passwords)",
                    kind.as_deref().unwrap_or("null")
                )));
            }
            let count = iarg(args, 1);
            if count < 0 {
                return Err(iae("count must be at least 0"));
            }
            put(vm, recv, "type", af_arg_value(args, 0))?;
            put(vm, recv, "count", Value::Int(count))?;
            Value::Null
        }
        ("getType", "()Ljava/lang/String;") => get(vm, recv, "type"),
        ("getCount", "()I") => Value::Int(field_int(vm, recv, "count")),
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(match oarg(args, 0) {
            Some(other) if other == recv => true,
            Some(other) if class_of(vm, other) == AF_SAVED_DATASETS_INFO => {
                objects_equal(vm, &get(vm, recv, "type"), &get(vm, other, "type"))
                    && field_int(vm, recv, "count") == field_int(vm, other, "count")
            }
            _ => false,
        }),
        ("hashCode", "()I") => {
            let mut hash: i32 = 1;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(af_objects_hash(vm, &get(vm, recv, "type"))?);
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "count"));
            Value::Int(hash)
        }
        ("toString", "()Ljava/lang/String;") => {
            let kind = field_str(vm, recv, "type").unwrap_or_else(|| "null".to_string());
            jstr(
                vm,
                format!(
                    "SavedDatasetsInfo {{ type = {kind}, count = {} }}",
                    field_int(vm, recv, "count")
                ),
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
fn af_autofill_service_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("onCreate", "()V") | ("onConnected", "()V") | ("onDisconnected", "()V") => Value::Null,
        ("onBind", "(Landroid/content/Intent;)Landroid/os/IBinder;") => {
            let intent = af_req_arg_obj(args, 0, "intent.getAction()")?;
            let action_value = vm.invoke_instance(
                intent,
                "getAction",
                "()Ljava/lang/String;",
                &[],
            )?;
            if opt_str(vm, Some(&action_value)).as_deref()
                == Some("android.service.autofill.AutofillService")
            {
                if let Some(existing) = field_obj(vm, recv, "binder") {
                    Value::Obj(existing)
                } else {
                    let binder = alloc_obj(vm, "Landroid/os/Binder;", Vec::new())?;
                    put(vm, recv, "binder", Value::Obj(binder))?;
                    Value::Obj(binder)
                }
            } else {
                Value::Null
            }
        }
        (
            "onFillRequest",
            "(Landroid/service/autofill/FillRequest;Landroid/os/CancellationSignal;Landroid/service/autofill/FillCallback;)V",
        )
        | (
            "onSaveRequest",
            "(Landroid/service/autofill/SaveRequest;Landroid/service/autofill/SaveCallback;)V",
        ) => {
            return Err(throw(
                "Ljava/lang/AbstractMethodError;",
                "AutofillService request callback",
            ));
        }
        (
            "onSavedDatasetsInfoRequest",
            "(Landroid/service/autofill/SavedDatasetsInfoCallback;)V",
        ) => {
            let callback = af_req_arg_obj(args, 0, "callback.onError()")?;
            vm.invoke_instance(callback, "onError", "(I)V", &[Value::Int(1)])?;
            Value::Null
        }
        ("getFillEventHistory", "()Landroid/service/autofill/FillEventHistory;") => {
            // The headless profile has no AutofillManager session/history.
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

// ---------------------------------------------------------------------------
// android.transition: deterministic capture/match/play model
// ---------------------------------------------------------------------------

const TR_PROP_VISIBILITY: &str = "android:visibility:visibility";
const TR_PROP_VISIBILITY_PARENT: &str = "android:visibility:parent";
const TR_PROP_VISIBILITY_SCREEN: &str = "android:visibility:screenLocation";
const TR_PROP_FADE_ALPHA: &str = "android:fade:transitionAlpha";
const TR_PROP_BOUNDS: &str = "android:changeBounds:bounds";
const TR_PROP_CLIP: &str = "android:changeBounds:clip";
const TR_PROP_BOUNDS_PARENT: &str = "android:changeBounds:parent";
const TR_PROP_WINDOW_X: &str = "android:changeBounds:windowX";
const TR_PROP_WINDOW_Y: &str = "android:changeBounds:windowY";

fn tr_new_list(vm: &mut Engine, items: Vec<Value>) -> Result<ObjRef, VmExit> {
    new_collection(vm, "Ljava/util/ArrayList;", items)
}
fn tr_new_map(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    af_new_array_map(vm)
}
fn tr_list_value(items: Vec<Value>, vm: &mut Engine) -> Result<Value, VmExit> {
    Ok(Value::Obj(tr_new_list(vm, items)?))
}
fn tr_items(vm: &Engine, value: &Value) -> Vec<Value> {
    collection_items(vm, Some(value))
}
fn tr_field_items(vm: &Engine, r: ObjRef, field: &str) -> Vec<Value> {
    tr_items(vm, &get(vm, r, field))
}
fn tr_set_field_list(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    items: Vec<Value>,
) -> Result<Value, VmExit> {
    let value = tr_list_value(items, vm)?;
    put(vm, r, field, value.clone())?;
    Ok(value)
}
fn tr_append_field_list(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    item: Value,
) -> Result<(), VmExit> {
    let mut items = tr_field_items(vm, r, field);
    items.push(item);
    tr_set_field_list(vm, r, field, items)?;
    Ok(())
}
fn tr_remove_field_list(
    vm: &mut Engine,
    r: ObjRef,
    field: &str,
    item: &Value,
) -> Result<bool, VmExit> {
    let mut items = tr_field_items(vm, r, field);
    let mut found = None;
    for (index, existing) in items.iter().enumerate() {
        if crate::intrinsics::values_equal(&vm.heap, existing, item)? {
            found = Some(index);
            break;
        }
    }
    if let Some(index) = found {
        items.remove(index);
        tr_set_field_list(vm, r, field, items)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
fn tr_list_contains(vm: &Engine, value: &Value, item: &Value) -> Result<bool, VmExit> {
    for existing in tr_items(vm, value) {
        if crate::intrinsics::values_equal(&vm.heap, &existing, item)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn tr_map_put(vm: &mut Engine, map: ObjRef, key: Value, value: Value) -> Result<Value, VmExit> {
    let mut entries = match vm.fw.objects.get(&map) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    };
    let mut old = Value::Null;
    let mut replaced = false;
    for entry in &mut entries {
        if crate::intrinsics::values_equal(&vm.heap, &entry.0, &key)? {
            old = entry.1.clone();
            entry.1 = value.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        entries.push((key, value));
    }
    vm.fw.objects.insert(map, HostObj::Map { entries });
    Ok(old)
}
fn tr_map_get(vm: &Engine, map_value: &Value, key: &Value) -> Option<Value> {
    let Value::Obj(map) = map_value else {
        return None;
    };
    match vm.fw.objects.get(map) {
        Some(HostObj::Map { entries }) => entries
            .iter()
            .find(|(existing, _)| {
                crate::intrinsics::values_equal(&vm.heap, existing, key).unwrap_or(false)
            })
            .map(|(_, value)| value.clone()),
        _ => None,
    }
}
fn tr_map_contains(vm: &Engine, map_value: &Value, key: &Value) -> bool {
    let Value::Obj(map) = map_value else {
        return false;
    };
    match vm.fw.objects.get(map) {
        Some(HostObj::Map { entries }) => entries.iter().any(|(existing, _)| {
            crate::intrinsics::values_equal(&vm.heap, existing, key).unwrap_or(false)
        }),
        _ => false,
    }
}
fn tr_map_remove(vm: &mut Engine, map: ObjRef, key: &Value) -> Result<Value, VmExit> {
    let mut entries = match vm.fw.objects.get(&map) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    };
    let mut old = Value::Null;
    let mut removed = false;
    let mut retained = Vec::with_capacity(entries.len());
    for entry in entries.drain(..) {
        if !removed && crate::intrinsics::values_equal(&vm.heap, &entry.0, key)? {
            old = entry.1;
            removed = true;
        } else {
            retained.push(entry);
        }
    }
    vm.fw
        .objects
        .insert(map, HostObj::Map { entries: retained });
    Ok(old)
}
fn tr_map_entries(vm: &Engine, map_value: &Value) -> Vec<(Value, Value)> {
    map_entries(vm, Some(map_value))
}
fn tr_box_int(vm: &mut Engine, value: i32) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Integer;",
        Value::Int(value),
    )?))
}
fn tr_box_float(vm: &mut Engine, value: f32) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Float;",
        Value::Float(value),
    )?))
}
fn tr_unbox_int(vm: &Engine, value: &Value) -> Option<i32> {
    anim_number_int(vm, value)
}
fn tr_unbox_float(vm: &Engine, value: &Value) -> Option<f32> {
    anim_number_float(vm, value)
}
fn tr_static_get(vm: &Engine, owner: &str, field: &str) -> Value {
    vm.statics
        .get(&(owner.to_string(), field.to_string()))
        .cloned()
        .unwrap_or(Value::Null)
}
fn tr_static_set(vm: &mut Engine, owner: &str, field: &str, value: Value) {
    vm.statics
        .insert((owner.to_string(), field.to_string()), value);
}
fn tr_binary_name(descriptor: &str) -> String {
    descriptor
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".")
}
fn tr_simple_name(descriptor: &str) -> String {
    let binary = tr_binary_name(descriptor);
    let class = binary.rsplit('.').next().unwrap_or(&binary);
    class.rsplit('$').next().unwrap_or(class).to_string()
}
fn tr_new_values_maps(vm: &mut Engine) -> Result<Value, VmExit> {
    let maps = alloc_obj(vm, TRANSITION_VALUES_MAPS, Vec::new())?;
    let view_values = tr_new_map(vm)?;
    put(vm, maps, "viewValues", Value::Obj(view_values))?;
    let id_values = tr_new_map(vm)?;
    put(vm, maps, "idValues", Value::Obj(id_values))?;
    let item_values = tr_new_map(vm)?;
    put(vm, maps, "itemIdValues", Value::Obj(item_values))?;
    let name_values = tr_new_map(vm)?;
    put(vm, maps, "nameValues", Value::Obj(name_values))?;
    Ok(Value::Obj(maps))
}
fn tr_ensure_transition(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, r, "rdTransitionInit") {
        return Ok(());
    }
    let class = class_of(vm, r);
    let name = tr_binary_name(&class);
    put(vm, r, "rdTransitionInit", Value::Int(1))?;
    let name_value = jstr(vm, name)?;
    put(vm, r, "mName", name_value)?;
    put(vm, r, "mStartDelay", Value::Long(-1))?;
    put(vm, r, "mDuration", Value::Long(-1))?;
    put(vm, r, "mInterpolator", Value::Null)?;
    let target_ids = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, r, "mTargetIds", target_ids)?;
    let targets = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, r, "mTargets", targets)?;
    for field in [
        "mTargetNames",
        "mTargetTypes",
        "mTargetIdExcludes",
        "mTargetExcludes",
        "mTargetTypeExcludes",
        "mTargetNameExcludes",
        "mTargetIdChildExcludes",
        "mTargetChildExcludes",
        "mTargetTypeChildExcludes",
        "mParent",
        "mStartValuesList",
        "mEndValuesList",
        "mSceneRoot",
        "mListeners",
        "mPropagation",
        "mEpicenterCallback",
        "mNameOverrides",
    ] {
        put(vm, r, field, Value::Null)?;
    }
    let start_maps = tr_new_values_maps(vm)?;
    put(vm, r, "mStartValues", start_maps)?;
    let end_maps = tr_new_values_maps(vm)?;
    put(vm, r, "mEndValues", end_maps)?;
    let order = new_int_array(vm, &[2, 1, 3, 4])?;
    put(vm, r, "mMatchOrder", order)?;
    put(vm, r, "mCanRemoveViews", Value::Int(0))?;
    let current = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, r, "mCurrentAnimators", current)?;
    put(vm, r, "mNumInstances", Value::Int(0))?;
    put(vm, r, "mPaused", Value::Int(0))?;
    put(vm, r, "mEnded", Value::Int(0))?;
    let animators = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, r, "mAnimators", animators)?;
    let path = alloc_obj(vm, TRANSITION_STRAIGHT_PATH, Vec::new())?;
    put(vm, r, "mPathMotion", Value::Obj(path))?;
    if vm.cp.is_subtype(&class, TRANSITION_VISIBILITY) {
        put(vm, r, "mMode", Value::Int(3))?;
        put(vm, r, "mSuppressLayout", Value::Int(1))?;
    }
    if vm.cp.is_subtype(&class, TRANSITION_SET) {
        let children = Value::Obj(tr_new_list(vm, Vec::new())?);
        put(vm, r, "mTransitions", children)?;
        put(vm, r, "mPlayTogether", Value::Int(1))?;
        put(vm, r, "mCurrentListeners", Value::Int(0))?;
        put(vm, r, "mStarted", Value::Int(0))?;
        put(vm, r, "mChangeFlags", Value::Int(0))?;
    }
    if class == TRANSITION_CHANGE_BOUNDS || vm.cp.is_subtype(&class, TRANSITION_CHANGE_BOUNDS) {
        put(vm, r, "mResizeClip", Value::Int(0))?;
        put(vm, r, "mReparent", Value::Int(0))?;
    }
    Ok(())
}
fn tr_ensure_values(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, r, "rdValuesInit") {
        return Ok(());
    }
    put(vm, r, "rdValuesInit", Value::Int(1))?;
    put(vm, r, "view", Value::Null)?;
    let values = tr_new_map(vm)?;
    put(vm, r, "values", Value::Obj(values))?;
    let targeted = tr_new_list(vm, Vec::new())?;
    put(vm, r, "targetedTransitions", Value::Obj(targeted))
}
fn tr_new_transition_values(vm: &mut Engine, view: Option<ObjRef>) -> Result<ObjRef, VmExit> {
    let values = alloc_obj(vm, TRANSITION_VALUES, Vec::new())?;
    tr_ensure_values(vm, values)?;
    put(vm, values, "view", view.map_or(Value::Null, Value::Obj))?;
    Ok(values)
}
fn tr_values_view(vm: &Engine, values: ObjRef) -> Option<ObjRef> {
    field_obj(vm, values, "view")
}
fn tr_values_map(vm: &Engine, values: ObjRef) -> Value {
    get(vm, values, "values")
}
fn tr_values_get_with_key(vm: &Engine, values: Option<ObjRef>, key_value: &Value) -> Option<Value> {
    tr_map_get(vm, &tr_values_map(vm, values?), key_value)
}
fn tr_values_put(vm: &mut Engine, values: ObjRef, key: &str, value: Value) -> Result<(), VmExit> {
    let map = field_obj(vm, values, "values").ok_or_else(|| npe("values"))?;
    let key = jstr(vm, key)?;
    tr_map_put(vm, map, key, value)?;
    Ok(())
}
fn tr_values_map_equal(vm: &mut Engine, left: &Value, right: &Value) -> Result<bool, VmExit> {
    let left_entries = tr_map_entries(vm, left);
    let right_entries = tr_map_entries(vm, right);
    if left_entries.len() != right_entries.len() {
        return Ok(false);
    }
    for (key, value) in left_entries {
        if !tr_map_contains(vm, right, &key) {
            return Ok(false);
        }
        let other = tr_map_get(vm, right, &key).unwrap_or(Value::Null);
        if crate::intrinsics::values_equal(&vm.heap, &value, &other)? {
            continue;
        }
        if let Value::Obj(a) = value {
            let equal = vm.invoke_instance(
                a,
                "equals",
                "(Ljava/lang/Object;)Z",
                std::slice::from_ref(&other),
            )?;
            if equal != Value::Int(1) {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
    }
    Ok(true)
}
fn tr_map_hash(vm: &mut Engine, map_value: &Value) -> Result<i32, VmExit> {
    let mut hash = 0i32;
    for (key, value) in tr_map_entries(vm, map_value) {
        hash = hash.wrapping_add(object_hash_code(vm, &key)? ^ object_hash_code(vm, &value)?);
    }
    Ok(hash)
}

fn transition_values_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => {
            tr_ensure_values(vm, recv)?;
            Value::Null
        }
        ("<init>", "(Landroid/view/View;)V") => {
            tr_ensure_values(vm, recv)?;
            put(
                vm,
                recv,
                "view",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            tr_ensure_values(vm, recv)?;
            let Some(other) = obj(args.first()) else {
                return Ok(Some(Value::Int(0)));
            };
            if !vm.cp.is_subtype(&class_of(vm, other), TRANSITION_VALUES) {
                return Ok(Some(Value::Int(0)));
            }
            tr_ensure_values(vm, other)?;
            if get(vm, recv, "view") != get(vm, other, "view") {
                return Ok(Some(Value::Int(0)));
            }
            let left = tr_values_map(vm, recv);
            let right = tr_values_map(vm, other);
            Value::Int(tr_values_map_equal(vm, &left, &right)? as i32)
        }
        ("hashCode", "()I") => {
            tr_ensure_values(vm, recv)?;
            let view = get(vm, recv, "view");
            let view_hash = object_hash_code(vm, &view)?;
            let map = tr_values_map(vm, recv);
            Value::Int(
                view_hash
                    .wrapping_mul(31)
                    .wrapping_add(tr_map_hash(vm, &map)?),
            )
        }
        ("toString", "()Ljava/lang/String;") => {
            tr_ensure_values(vm, recv)?;
            let view = get(vm, recv, "view");
            let view_hash = object_hash_code(vm, &view)?;
            let map = tr_values_map(vm, recv);
            let hash = view_hash
                .wrapping_mul(31)
                .wrapping_add(tr_map_hash(vm, &map)?);
            let mut out = format!(
                "TransitionValues@{:x}:\n    view = {}\n    values:",
                hash as u32,
                value_string(vm, &view)?
            );
            for (key, value) in tr_map_entries(vm, &map) {
                out.push_str(&format!(
                    "    {}: {}\n",
                    value_string(vm, &key)?,
                    value_string(vm, &value)?
                ));
            }
            jstr(vm, out)?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn transition_animation_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name != "<init>"
        || sig != "(Landroid/view/View;Ljava/lang/String;Landroid/transition/Transition;Landroid/view/WindowId;Landroid/transition/TransitionValues;)V"
    {
        return Ok(None);
    }
    put(
        vm,
        recv,
        "view",
        args.first().cloned().unwrap_or(Value::Null),
    )?;
    put(
        vm,
        recv,
        "name",
        args.get(1).cloned().unwrap_or(Value::Null),
    )?;
    put(
        vm,
        recv,
        "transition",
        args.get(2).cloned().unwrap_or(Value::Null),
    )?;
    put(
        vm,
        recv,
        "windowId",
        args.get(3).cloned().unwrap_or(Value::Null),
    )?;
    put(
        vm,
        recv,
        "values",
        args.get(4).cloned().unwrap_or(Value::Null),
    )?;
    Ok(Some(Value::Null))
}

fn tr_invoke(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Value, VmExit> {
    vm.invoke_instance(recv, name, sig, args)
}
fn tr_view_id(vm: &mut Engine, view: ObjRef) -> Result<i32, VmExit> {
    Ok(match tr_invoke(vm, view, "getId", "()I", &[])? {
        Value::Int(v) => v,
        _ => -1,
    })
}
fn tr_view_parent(vm: &mut Engine, view: ObjRef) -> Result<Value, VmExit> {
    tr_invoke(vm, view, "getParent", "()Landroid/view/ViewParent;", &[])
}
fn tr_view_transition_name(vm: &mut Engine, view: ObjRef) -> Result<Value, VmExit> {
    match tr_invoke(vm, view, "getTransitionName", "()Ljava/lang/String;", &[]) {
        Ok(v) => Ok(v),
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => Ok(Value::Null),
        Err(error) => Err(error),
    }
}
fn tr_class_is_instance(
    vm: &mut Engine,
    class_value: &Value,
    target: ObjRef,
) -> Result<bool, VmExit> {
    let Some(class_obj) = obj(Some(class_value)) else {
        return Ok(false);
    };
    match vm.invoke_instance(
        class_obj,
        "isInstance",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(target)],
    ) {
        Ok(Value::Int(v)) => Ok(v != 0),
        Ok(_) => Ok(false),
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => Ok(false),
        Err(error) => Err(error),
    }
}
fn tr_ensure_field_list(vm: &mut Engine, r: ObjRef, field: &str) -> Result<Vec<Value>, VmExit> {
    if field_obj(vm, r, field).is_none() {
        tr_set_field_list(vm, r, field, Vec::new())?;
    }
    Ok(tr_field_items(vm, r, field))
}
fn tr_is_valid_target(
    vm: &mut Engine,
    transition: ObjRef,
    view: Option<ObjRef>,
) -> Result<bool, VmExit> {
    tr_ensure_transition(vm, transition)?;
    let Some(view) = view else {
        return Ok(false);
    };
    let id = tr_view_id(vm, view)?;
    let boxed_id = tr_box_int(vm, id)?;
    if tr_list_contains(vm, &get(vm, transition, "mTargetIdExcludes"), &boxed_id)? {
        return Ok(false);
    }
    if tr_list_contains(
        vm,
        &get(vm, transition, "mTargetExcludes"),
        &Value::Obj(view),
    )? {
        return Ok(false);
    }
    for class_value in tr_field_items(vm, transition, "mTargetTypeExcludes") {
        if tr_class_is_instance(vm, &class_value, view)? {
            return Ok(false);
        }
    }
    let name_value = tr_view_transition_name(vm, view)?;
    if !matches!(name_value, Value::Null)
        && tr_list_contains(vm, &get(vm, transition, "mTargetNameExcludes"), &name_value)?
    {
        return Ok(false);
    }
    let target_ids = tr_field_items(vm, transition, "mTargetIds");
    let targets = tr_field_items(vm, transition, "mTargets");
    let target_names = tr_field_items(vm, transition, "mTargetNames");
    let target_types = tr_field_items(vm, transition, "mTargetTypes");
    if target_ids.is_empty()
        && targets.is_empty()
        && target_names.is_empty()
        && target_types.is_empty()
    {
        return Ok(true);
    }
    if tr_list_contains(vm, &get(vm, transition, "mTargetIds"), &boxed_id)?
        || tr_list_contains(vm, &get(vm, transition, "mTargets"), &Value::Obj(view))?
    {
        return Ok(true);
    }
    if !matches!(name_value, Value::Null)
        && tr_list_contains(vm, &get(vm, transition, "mTargetNames"), &name_value)?
    {
        return Ok(true);
    }
    for class_value in target_types {
        if tr_class_is_instance(vm, &class_value, view)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn tr_maps_obj(vm: &Engine, transition: ObjRef, start: bool) -> Option<ObjRef> {
    field_obj(
        vm,
        transition,
        if start { "mStartValues" } else { "mEndValues" },
    )
}
fn tr_maps_field(vm: &Engine, maps: ObjRef, field: &str) -> Option<ObjRef> {
    field_obj(vm, maps, field)
}
fn tr_clear_values(vm: &mut Engine, transition: ObjRef, start: bool) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let maps = tr_new_values_maps(vm)?;
    put(
        vm,
        transition,
        if start { "mStartValues" } else { "mEndValues" },
        maps,
    )?;
    put(
        vm,
        transition,
        if start {
            "mStartValuesList"
        } else {
            "mEndValuesList"
        },
        Value::Null,
    )
}
fn tr_capture_propagation_values(
    vm: &mut Engine,
    transition: ObjRef,
    values: ObjRef,
) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let is_set = {
        let class = class_of(vm, transition);
        vm.cp.is_subtype(&class, TRANSITION_SET)
    };
    if let Some(propagation) = field_obj(vm, transition, "mPropagation") {
        let map = tr_values_map(vm, values);
        if !tr_map_entries(vm, &map).is_empty() {
            let properties = tr_invoke(
                vm,
                propagation,
                "getPropagationProperties",
                "()[Ljava/lang/String;",
                &[],
            )?;
            if matches!(properties, Value::Obj(_)) {
                let mut contains_all = true;
                for property in array_elems(vm, Some(&properties)) {
                    if !tr_map_contains(vm, &map, &property) {
                        contains_all = false;
                        break;
                    }
                }
                if !contains_all {
                    tr_invoke(
                        vm,
                        propagation,
                        "captureValues",
                        "(Landroid/transition/TransitionValues;)V",
                        &[Value::Obj(values)],
                    )?;
                }
            }
        }
    }
    if is_set {
        for child in tr_field_items(vm, transition, "mTransitions")
            .iter()
            .filter_map(|value| obj(Some(value)))
        {
            tr_capture_propagation_values(vm, child, values)?;
        }
    }
    Ok(())
}
fn tr_add_view_values(
    vm: &mut Engine,
    maps: ObjRef,
    view: ObjRef,
    values: ObjRef,
) -> Result<(), VmExit> {
    let view_values = tr_maps_field(vm, maps, "viewValues").ok_or_else(|| npe("viewValues"))?;
    tr_map_put(vm, view_values, Value::Obj(view), Value::Obj(values))?;
    let id = tr_view_id(vm, view)?;
    if id >= 0 {
        let id_values = tr_maps_field(vm, maps, "idValues").ok_or_else(|| npe("idValues"))?;
        let key = tr_box_int(vm, id)?;
        let value = if tr_map_contains(vm, &Value::Obj(id_values), &key) {
            Value::Null
        } else {
            Value::Obj(view)
        };
        tr_map_put(vm, id_values, key, value)?;
    }
    let name = tr_view_transition_name(vm, view)?;
    if !matches!(name, Value::Null) {
        let name_values = tr_maps_field(vm, maps, "nameValues").ok_or_else(|| npe("nameValues"))?;
        let value = if tr_map_contains(vm, &Value::Obj(name_values), &name) {
            Value::Null
        } else {
            Value::Obj(view)
        };
        tr_map_put(vm, name_values, name, value)?;
    }
    Ok(())
}
fn tr_capture_one(
    vm: &mut Engine,
    transition: ObjRef,
    view: ObjRef,
    start: bool,
    maps: ObjRef,
) -> Result<(), VmExit> {
    let values = tr_new_transition_values(vm, Some(view))?;
    tr_invoke(
        vm,
        transition,
        if start {
            "captureStartValues"
        } else {
            "captureEndValues"
        },
        "(Landroid/transition/TransitionValues;)V",
        &[Value::Obj(values)],
    )?;
    tr_append_field_list(vm, values, "targetedTransitions", Value::Obj(transition))?;
    tr_capture_propagation_values(vm, transition, values)?;
    tr_add_view_values(vm, maps, view, values)
}
fn tr_capture_hierarchy(
    vm: &mut Engine,
    transition: ObjRef,
    view: ObjRef,
    start: bool,
    maps: ObjRef,
    depth: usize,
) -> Result<(), VmExit> {
    if depth > 256 {
        return Ok(());
    }
    let id = tr_view_id(vm, view)?;
    let boxed_id = tr_box_int(vm, id)?;
    if tr_list_contains(vm, &get(vm, transition, "mTargetIdExcludes"), &boxed_id)?
        || tr_list_contains(
            vm,
            &get(vm, transition, "mTargetExcludes"),
            &Value::Obj(view),
        )?
    {
        return Ok(());
    }
    for class_value in tr_field_items(vm, transition, "mTargetTypeExcludes") {
        if tr_class_is_instance(vm, &class_value, view)? {
            return Ok(());
        }
    }
    let parent = tr_view_parent(vm, view)?;
    if let Some(parent_ref) = obj(Some(&parent)) {
        let parent_class = class_of(vm, parent_ref);
        if vm.cp.is_subtype(&parent_class, "Landroid/view/ViewGroup;") {
            tr_capture_one(vm, transition, view, start, maps)?;
        }
    }
    let view_class = class_of(vm, view);
    if vm.cp.is_subtype(&view_class, "Landroid/view/ViewGroup;") {
        if tr_list_contains(
            vm,
            &get(vm, transition, "mTargetIdChildExcludes"),
            &boxed_id,
        )? || tr_list_contains(
            vm,
            &get(vm, transition, "mTargetChildExcludes"),
            &Value::Obj(view),
        )? {
            return Ok(());
        }
        for class_value in tr_field_items(vm, transition, "mTargetTypeChildExcludes") {
            if tr_class_is_instance(vm, &class_value, view)? {
                return Ok(());
            }
        }
        let count = match tr_invoke(vm, view, "getChildCount", "()I", &[])? {
            Value::Int(v) => v.max(0),
            _ => 0,
        };
        for index in 0..count {
            let child = tr_invoke(
                vm,
                view,
                "getChildAt",
                "(I)Landroid/view/View;",
                &[Value::Int(index)],
            )?;
            if let Some(child) = obj(Some(&child)) {
                tr_capture_hierarchy(vm, transition, child, start, maps, depth + 1)?;
            }
        }
    }
    Ok(())
}
fn tr_capture_values(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: ObjRef,
    start: bool,
) -> Result<(), VmExit> {
    tr_clear_values(vm, transition, start)?;
    let maps = tr_maps_obj(vm, transition, start).ok_or_else(|| npe("transition values"))?;
    let target_ids = tr_field_items(vm, transition, "mTargetIds");
    let targets = tr_field_items(vm, transition, "mTargets");
    let has_names_or_types = !tr_field_items(vm, transition, "mTargetNames").is_empty()
        || !tr_field_items(vm, transition, "mTargetTypes").is_empty();
    if (!target_ids.is_empty() || !targets.is_empty()) && !has_names_or_types {
        for id_value in target_ids {
            let Some(id) = tr_unbox_int(vm, &id_value) else {
                continue;
            };
            let view = tr_invoke(
                vm,
                scene_root,
                "findViewById",
                "(I)Landroid/view/View;",
                &[Value::Int(id)],
            )?;
            if let Some(view) = obj(Some(&view)) {
                tr_capture_one(vm, transition, view, start, maps)?;
            }
        }
        for target in targets.iter().filter_map(|value| obj(Some(value))) {
            tr_capture_one(vm, transition, target, start, maps)?;
        }
    } else {
        tr_capture_hierarchy(vm, transition, scene_root, start, maps, 0)?;
    }
    if !start {
        let overrides = get(vm, transition, "mNameOverrides");
        if matches!(overrides, Value::Obj(_)) {
            let start_maps =
                tr_maps_obj(vm, transition, true).ok_or_else(|| npe("start values"))?;
            let name_values =
                tr_maps_field(vm, start_maps, "nameValues").ok_or_else(|| npe("nameValues"))?;
            let entries = tr_map_entries(vm, &overrides);
            let mut removed = Vec::new();
            for (from, _) in &entries {
                removed.push(tr_map_remove(vm, name_values, from)?);
            }
            for ((_, to), view) in entries.into_iter().zip(removed) {
                if !matches!(view, Value::Null) {
                    tr_map_put(vm, name_values, to, view)?;
                }
            }
        }
    }
    Ok(())
}
fn tr_view_values_entries(vm: &Engine, maps: ObjRef) -> Vec<(ObjRef, ObjRef)> {
    let Some(view_values) = tr_maps_field(vm, maps, "viewValues") else {
        return Vec::new();
    };
    tr_map_entries(vm, &Value::Obj(view_values))
        .into_iter()
        .filter_map(|(view, values)| Some((obj(Some(&view))?, obj(Some(&values))?)))
        .collect()
}
fn tr_match_start_and_end(vm: &mut Engine, transition: ObjRef) -> Result<(), VmExit> {
    let start_maps = tr_maps_obj(vm, transition, true).ok_or_else(|| npe("start values"))?;
    let end_maps = tr_maps_obj(vm, transition, false).ok_or_else(|| npe("end values"))?;
    let mut unmatched_start = tr_view_values_entries(vm, start_maps);
    let mut unmatched_end = tr_view_values_entries(vm, end_maps);
    let mut pairs: Vec<(Option<ObjRef>, Option<ObjRef>)> = Vec::new();
    let order = int_values(vm, Some(&get(vm, transition, "mMatchOrder")));
    let order = if order.is_empty() {
        vec![2, 1, 3, 4]
    } else {
        order
    };
    for kind in order {
        match kind {
            1 => {
                let mut index = unmatched_start.len();
                while index > 0 {
                    index -= 1;
                    let (view, _) = unmatched_start[index];
                    if !tr_is_valid_target(vm, transition, Some(view))? {
                        continue;
                    }
                    if let Some(end_index) = unmatched_end
                        .iter()
                        .position(|(candidate, _)| *candidate == view)
                    {
                        let (end_view, end_values) = unmatched_end[end_index];
                        if tr_is_valid_target(vm, transition, Some(end_view))? {
                            let (_, start_values) = unmatched_start.remove(index);
                            unmatched_end.remove(end_index);
                            pairs.push((Some(start_values), Some(end_values)));
                        }
                    }
                }
            }
            2..=4 => {
                let field = match kind {
                    2 => "nameValues",
                    3 => "idValues",
                    _ => "itemIdValues",
                };
                let Some(start_map) = tr_maps_field(vm, start_maps, field) else {
                    continue;
                };
                let Some(end_map) = tr_maps_field(vm, end_maps, field) else {
                    continue;
                };
                for (key, start_view_value) in tr_map_entries(vm, &Value::Obj(start_map)) {
                    let Some(start_view) = obj(Some(&start_view_value)) else {
                        continue;
                    };
                    if !tr_is_valid_target(vm, transition, Some(start_view))? {
                        continue;
                    }
                    let Some(end_view_value) = tr_map_get(vm, &Value::Obj(end_map), &key) else {
                        continue;
                    };
                    let Some(end_view) = obj(Some(&end_view_value)) else {
                        continue;
                    };
                    if !tr_is_valid_target(vm, transition, Some(end_view))? {
                        continue;
                    }
                    let Some(start_index) = unmatched_start
                        .iter()
                        .position(|(view, _)| *view == start_view)
                    else {
                        continue;
                    };
                    let Some(end_index) =
                        unmatched_end.iter().position(|(view, _)| *view == end_view)
                    else {
                        continue;
                    };
                    let (_, start_values) = unmatched_start.remove(start_index);
                    let (_, end_values) = unmatched_end.remove(end_index);
                    pairs.push((Some(start_values), Some(end_values)));
                }
            }
            _ => {}
        }
    }
    for (view, values) in unmatched_start {
        if tr_is_valid_target(vm, transition, Some(view))? {
            pairs.push((Some(values), None));
        }
    }
    for (view, values) in unmatched_end {
        if tr_is_valid_target(vm, transition, Some(view))? {
            pairs.push((None, Some(values)));
        }
    }
    let starts = pairs
        .iter()
        .map(|(start, _)| start.map_or(Value::Null, Value::Obj))
        .collect();
    let ends = pairs
        .iter()
        .map(|(_, end)| end.map_or(Value::Null, Value::Obj))
        .collect();
    tr_set_field_list(vm, transition, "mStartValuesList", starts)?;
    tr_set_field_list(vm, transition, "mEndValuesList", ends)?;
    Ok(())
}
fn tr_get_transition_values(
    vm: &mut Engine,
    transition: ObjRef,
    view: Option<ObjRef>,
    start: bool,
) -> Result<Value, VmExit> {
    tr_ensure_transition(vm, transition)?;
    if let Some(parent) = field_obj(vm, transition, "mParent") {
        return tr_get_transition_values(vm, parent, view, start);
    }
    let Some(view) = view else {
        return Ok(Value::Null);
    };
    let Some(maps) = tr_maps_obj(vm, transition, start) else {
        return Ok(Value::Null);
    };
    let Some(view_values) = tr_maps_field(vm, maps, "viewValues") else {
        return Ok(Value::Null);
    };
    Ok(tr_map_get(vm, &Value::Obj(view_values), &Value::Obj(view)).unwrap_or(Value::Null))
}
fn tr_get_matched_transition_values(
    vm: &mut Engine,
    transition: ObjRef,
    view: Option<ObjRef>,
    view_in_start: bool,
) -> Result<Value, VmExit> {
    tr_ensure_transition(vm, transition)?;
    if let Some(parent) = field_obj(vm, transition, "mParent") {
        return tr_get_matched_transition_values(vm, parent, view, view_in_start);
    }
    let look_field = if view_in_start {
        "mStartValuesList"
    } else {
        "mEndValuesList"
    };
    if field_obj(vm, transition, look_field).is_none() {
        return Ok(Value::Null);
    }
    let look = tr_field_items(vm, transition, look_field);
    let mut found = None;
    for (index, value) in look.iter().enumerate() {
        let Some(values) = obj(Some(value)) else {
            return Ok(Value::Null);
        };
        if tr_values_view(vm, values) == view && view.is_some() {
            found = Some(index);
            break;
        }
    }
    let Some(index) = found else {
        return Ok(Value::Null);
    };
    let match_field = if view_in_start {
        "mEndValuesList"
    } else {
        "mStartValuesList"
    };
    Ok(tr_field_items(vm, transition, match_field)
        .get(index)
        .cloned()
        .unwrap_or(Value::Null))
}

fn tr_running_animators(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Value::Obj(map) = tr_static_get(vm, TRANSITION, "rdRunningAnimators") {
        return Ok(map);
    }
    let map = tr_new_map(vm)?;
    tr_static_set(vm, TRANSITION, "rdRunningAnimators", Value::Obj(map));
    Ok(map)
}
fn tr_notify_listeners(vm: &mut Engine, transition: ObjRef, method: &str) -> Result<(), VmExit> {
    let listeners = tr_field_items(vm, transition, "mListeners");
    for listener in listeners.iter().filter_map(|value| obj(Some(value))) {
        tr_invoke(
            vm,
            listener,
            method,
            "(Landroid/transition/Transition;)V",
            &[Value::Obj(transition)],
        )?;
    }
    Ok(())
}
fn tr_start(vm: &mut Engine, transition: ObjRef) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let count = field_int(vm, transition, "mNumInstances");
    if count == 0 {
        tr_notify_listeners(vm, transition, "onTransitionStart")?;
        put(vm, transition, "mEnded", Value::Int(0))?;
    }
    put(
        vm,
        transition,
        "mNumInstances",
        Value::Int(count.wrapping_add(1)),
    )
}
fn tr_end(vm: &mut Engine, transition: ObjRef) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let count = field_int(vm, transition, "mNumInstances").wrapping_sub(1);
    put(vm, transition, "mNumInstances", Value::Int(count))?;
    if count == 0 {
        tr_notify_listeners(vm, transition, "onTransitionEnd")?;
        put(vm, transition, "mEnded", Value::Int(1))?;
    }
    Ok(())
}
fn tr_animator_set_duration(
    vm: &mut Engine,
    animator: ObjRef,
    duration: i64,
) -> Result<(), VmExit> {
    let class = class_of(vm, animator);
    if vm.cp.is_subtype(&class, VALUE_ANIMATOR) {
        if duration < 0 {
            return Err(iae(format!(
                "Animators cannot have negative duration: {duration}"
            )));
        }
        put(vm, animator, "rd_duration", Value::Long(duration))?;
        return Ok(());
    }
    let sig = if vm.cp.is_subtype(&class, OBJECT_ANIMATOR) {
        "(J)Landroid/animation/ObjectAnimator;"
    } else {
        "(J)Landroid/animation/Animator;"
    };
    tr_invoke(vm, animator, "setDuration", sig, &[Value::Long(duration)])?;
    Ok(())
}
fn tr_animator_get_start_delay(vm: &mut Engine, animator: ObjRef) -> Result<i64, VmExit> {
    let class = class_of(vm, animator);
    if vm.cp.is_subtype(&class, VALUE_ANIMATOR) {
        return Ok(anim_long_or(vm, animator, "rd_start_delay", 0));
    }
    Ok(
        match tr_invoke(vm, animator, "getStartDelay", "()J", &[])? {
            Value::Long(v) => v,
            Value::Int(v) => v as i64,
            _ => 0,
        },
    )
}
fn tr_animator_set_start_delay(
    vm: &mut Engine,
    animator: ObjRef,
    delay: i64,
) -> Result<(), VmExit> {
    let class = class_of(vm, animator);
    if vm.cp.is_subtype(&class, VALUE_ANIMATOR) {
        put(vm, animator, "rd_start_delay", Value::Long(delay.max(0)))?;
        return Ok(());
    }
    tr_invoke(vm, animator, "setStartDelay", "(J)V", &[Value::Long(delay)])?;
    Ok(())
}
fn tr_animator_set_interpolator(
    vm: &mut Engine,
    animator: ObjRef,
    interpolator: Value,
) -> Result<(), VmExit> {
    let class = class_of(vm, animator);
    if vm.cp.is_subtype(&class, VALUE_ANIMATOR) {
        put(vm, animator, "rd_interpolator", interpolator)?;
        return Ok(());
    }
    tr_invoke(
        vm,
        animator,
        "setInterpolator",
        "(Landroid/animation/TimeInterpolator;)V",
        &[interpolator],
    )?;
    Ok(())
}
fn tr_animator_add_listener(
    vm: &mut Engine,
    animator: ObjRef,
    listener: ObjRef,
    pause: bool,
) -> Result<(), VmExit> {
    if pause {
        tr_invoke(
            vm,
            animator,
            "addPauseListener",
            "(Landroid/animation/Animator$AnimatorPauseListener;)V",
            &[Value::Obj(listener)],
        )?;
    } else {
        tr_invoke(
            vm,
            animator,
            "addListener",
            "(Landroid/animation/Animator$AnimatorListener;)V",
            &[Value::Obj(listener)],
        )?;
    }
    Ok(())
}
fn tr_animator_bool(vm: &mut Engine, animator: ObjRef, method: &str) -> Result<bool, VmExit> {
    Ok(tr_invoke(vm, animator, method, "()Z", &[])? == Value::Int(1))
}
fn tr_animate(vm: &mut Engine, transition: ObjRef, animator: Option<ObjRef>) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let Some(animator) = animator else {
        return tr_end(vm, transition);
    };
    let duration = field_long(vm, transition, "mDuration");
    if duration >= 0 {
        tr_animator_set_duration(vm, animator, duration)?;
    }
    let delay = field_long(vm, transition, "mStartDelay");
    if delay >= 0 {
        let existing = tr_animator_get_start_delay(vm, animator)?;
        tr_animator_set_start_delay(vm, animator, delay.saturating_add(existing))?;
    }
    let interpolator = get(vm, transition, "mInterpolator");
    if !matches!(interpolator, Value::Null) {
        tr_animator_set_interpolator(vm, animator, interpolator)?;
    }
    let listener = alloc_obj(vm, TRANSITION_ANIMATE_LISTENER, Vec::new())?;
    put(vm, listener, "this$0", Value::Obj(transition))?;
    tr_animator_add_listener(vm, animator, listener, false)?;
    tr_invoke(vm, animator, "start", "()V", &[])?;
    Ok(())
}
fn tr_run_animator(vm: &mut Engine, transition: ObjRef, animator: ObjRef) -> Result<(), VmExit> {
    let running = tr_running_animators(vm)?;
    let listener = alloc_obj(vm, TRANSITION_RUN_LISTENER, Vec::new())?;
    put(vm, listener, "this$0", Value::Obj(transition))?;
    put(vm, listener, "val$runningAnimators", Value::Obj(running))?;
    tr_animator_add_listener(vm, animator, listener, false)?;
    tr_animate(vm, transition, Some(animator))
}
fn tr_set_children(vm: &Engine, transition: ObjRef) -> Vec<ObjRef> {
    tr_field_items(vm, transition, "mTransitions")
        .iter()
        .filter_map(|value| obj(Some(value)))
        .collect()
}
fn tr_is_set(vm: &Engine, transition: ObjRef) -> bool {
    let class = class_of(vm, transition);
    vm.cp.is_subtype(&class, TRANSITION_SET)
}
fn tr_run_animators(vm: &mut Engine, transition: ObjRef) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    if tr_is_set(vm, transition) {
        let children = tr_set_children(vm, transition);
        if children.is_empty() {
            tr_start(vm, transition)?;
            return tr_end(vm, transition);
        }
        let listener = alloc_obj(vm, TRANSITION_SET_LISTENER, Vec::new())?;
        put(vm, listener, "mTransitionSet", Value::Obj(transition))?;
        for child in &children {
            tr_invoke(
                vm,
                *child,
                "addListener",
                "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
                &[Value::Obj(listener)],
            )?;
        }
        put(
            vm,
            transition,
            "mCurrentListeners",
            Value::Int(children.len() as i32),
        )?;
        if !field_bool(vm, transition, "mPlayTogether") {
            for pair in children.windows(2) {
                let next_listener = alloc_obj(vm, TRANSITION_SET_NEXT_LISTENER, Vec::new())?;
                put(vm, next_listener, "this$0", Value::Obj(transition))?;
                put(vm, next_listener, "val$nextTransition", Value::Obj(pair[1]))?;
                tr_invoke(
                    vm,
                    pair[0],
                    "addListener",
                    "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
                    &[Value::Obj(next_listener)],
                )?;
            }
            if let Some(first) = children.first() {
                tr_invoke(vm, *first, "runAnimators", "()V", &[])?;
            }
        } else {
            for child in children {
                tr_invoke(vm, child, "runAnimators", "()V", &[])?;
            }
        }
        return Ok(());
    }
    tr_start(vm, transition)?;
    let running = tr_running_animators(vm)?;
    let animators = tr_field_items(vm, transition, "mAnimators")
        .iter()
        .filter_map(|value| obj(Some(value)))
        .collect::<Vec<_>>();
    for animator in animators {
        if tr_map_contains(vm, &Value::Obj(running), &Value::Obj(animator)) {
            tr_start(vm, transition)?;
            tr_run_animator(vm, transition, animator)?;
        }
    }
    let empty = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, transition, "mAnimators", empty)?;
    tr_end(vm, transition)
}
fn tr_transition_properties_value(vm: &mut Engine, transition: ObjRef) -> Result<Value, VmExit> {
    tr_invoke(
        vm,
        transition,
        "getTransitionProperties",
        "()[Ljava/lang/String;",
        &[],
    )
}
fn tr_transition_properties(vm: &mut Engine, transition: ObjRef) -> Result<Vec<Value>, VmExit> {
    let properties = tr_transition_properties_value(vm, transition)?;
    Ok(array_elems(vm, Some(&properties)))
}
fn tr_is_transition_required(
    vm: &mut Engine,
    transition: ObjRef,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<bool, VmExit> {
    let class = class_of(vm, transition);
    if vm.cp.is_subtype(&class, TRANSITION_VISIBILITY) && !tr_is_set(vm, transition) {
        return tr_visibility_is_required(vm, start, end);
    }
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(false);
    };
    let properties = tr_transition_properties_value(vm, transition)?;
    let keys = if matches!(properties, Value::Null) {
        tr_map_entries(vm, &tr_values_map(vm, start))
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>()
    } else {
        array_elems(vm, Some(&properties))
    };
    for key in keys {
        if tr_value_changed(vm, start, end, &key)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn tr_call_is_transition_required(
    vm: &mut Engine,
    transition: ObjRef,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<bool, VmExit> {
    Ok(tr_invoke(
        vm,
        transition,
        "isTransitionRequired",
        "(Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Z",
        &[
            start.map_or(Value::Null, Value::Obj),
            end.map_or(Value::Null, Value::Obj),
        ],
    )? == Value::Int(1))
}
fn tr_value_changed(
    vm: &mut Engine,
    start: ObjRef,
    end: ObjRef,
    key: &Value,
) -> Result<bool, VmExit> {
    let start_map = tr_values_map(vm, start);
    let end_map = tr_values_map(vm, end);
    if tr_map_contains(vm, &start_map, key) != tr_map_contains(vm, &end_map, key) {
        return Ok(false);
    }
    let old = tr_map_get(vm, &start_map, key).unwrap_or(Value::Null);
    let new = tr_map_get(vm, &end_map, key).unwrap_or(Value::Null);
    if old == Value::Null || new == Value::Null {
        return Ok(old != new);
    }
    if crate::intrinsics::values_equal(&vm.heap, &old, &new)? {
        return Ok(false);
    }
    if let Value::Obj(old_ref) = old {
        let equal = vm.invoke_instance(
            old_ref,
            "equals",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(&new),
        )?;
        return Ok(equal != Value::Int(1));
    }
    Ok(old != new)
}
fn tr_create_animators(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: ObjRef,
    start_maps: ObjRef,
    end_maps: ObjRef,
    start_list: Value,
    end_list: Value,
) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    if tr_is_set(vm, transition) {
        let delay = field_long(vm, transition, "mStartDelay");
        let play_together = field_bool(vm, transition, "mPlayTogether");
        for (index, child) in tr_set_children(vm, transition).into_iter().enumerate() {
            if delay > 0 && (play_together || index == 0) {
                let child_delay = match tr_invoke(vm, child, "getStartDelay", "()J", &[])? {
                    Value::Long(v) => v,
                    _ => -1,
                };
                let combined = if child_delay > 0 {
                    delay + child_delay
                } else {
                    delay
                };
                tr_invoke(
                    vm,
                    child,
                    "setStartDelay",
                    "(J)Landroid/transition/Transition;",
                    &[Value::Long(combined)],
                )?;
            }
            tr_invoke(
                vm,
                child,
                "createAnimators",
                "(Landroid/view/ViewGroup;Landroid/transition/TransitionValuesMaps;Landroid/transition/TransitionValuesMaps;Ljava/util/ArrayList;Ljava/util/ArrayList;)V",
                &[
                    Value::Obj(scene_root),
                    Value::Obj(start_maps),
                    Value::Obj(end_maps),
                    start_list.clone(),
                    end_list.clone(),
                ],
            )?;
        }
        return Ok(());
    }
    let starts = tr_items(vm, &start_list);
    let ends = tr_items(vm, &end_list);
    let running = tr_running_animators(vm)?;
    let mut delays: Vec<(usize, i64)> = Vec::new();
    let mut min_delay = i64::MAX;
    for index in 0..starts.len().min(ends.len()) {
        let mut start = obj(starts.get(index));
        let mut end = obj(ends.get(index));
        if let Some(values) = start {
            let targeted = get(vm, values, "targetedTransitions");
            if !tr_list_contains(vm, &targeted, &Value::Obj(transition))? {
                start = None;
            }
        }
        if let Some(values) = end {
            let targeted = get(vm, values, "targetedTransitions");
            if !tr_list_contains(vm, &targeted, &Value::Obj(transition))? {
                end = None;
            }
        }
        if start.is_none() && end.is_none() {
            continue;
        }
        let changed = start.is_none()
            || end.is_none()
            || tr_call_is_transition_required(vm, transition, start, end)?;
        if !changed {
            continue;
        }
        let animator_value = tr_invoke(
            vm,
            transition,
            "createAnimator",
            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
            &[
                Value::Obj(scene_root),
                start.map_or(Value::Null, Value::Obj),
                end.map_or(Value::Null, Value::Obj),
            ],
        )?;
        let Some(mut animator) = obj(Some(&animator_value)) else {
            continue;
        };
        let mut view = None;
        let mut info_values = Value::Null;
        if let Some(end_values) = end {
            view = tr_values_view(vm, end_values);
            let properties = tr_transition_properties(vm, transition)?;
            if !properties.is_empty() {
                if let Some(view_ref) = view {
                    let info = tr_new_transition_values(vm, Some(view_ref))?;
                    let current = tr_map_get(
                        vm,
                        &Value::Obj(
                            tr_maps_field(vm, end_maps, "viewValues")
                                .ok_or_else(|| npe("viewValues"))?,
                        ),
                        &Value::Obj(view_ref),
                    )
                    .and_then(|value| obj(Some(&value)));
                    if let Some(current) = current {
                        let info_map =
                            field_obj(vm, info, "values").ok_or_else(|| npe("values"))?;
                        for property in properties {
                            let value = tr_values_get_with_key(vm, Some(current), &property)
                                .unwrap_or(Value::Null);
                            tr_map_put(vm, info_map, property, value)?;
                        }
                    }
                    info_values = Value::Obj(info);
                    let name = get(vm, transition, "mName");
                    for (existing_animator, existing_info) in
                        tr_map_entries(vm, &Value::Obj(running))
                    {
                        let Some(existing_ref) = obj(Some(&existing_info)) else {
                            continue;
                        };
                        let _ = existing_animator;
                        if get(vm, existing_ref, "view") == Value::Obj(view_ref)
                            && get(vm, existing_ref, "name") == name
                        {
                            let existing_values = get(vm, existing_ref, "values");
                            if let Some(existing_values_ref) = obj(Some(&existing_values)) {
                                let equal = tr_invoke(
                                    vm,
                                    existing_values_ref,
                                    "equals",
                                    "(Ljava/lang/Object;)Z",
                                    &[info_values.clone()],
                                )?;
                                if equal == Value::Int(1) {
                                    animator = 0;
                                    break;
                                }
                            }
                        }
                    }
                    if animator == 0 {
                        continue;
                    }
                }
            }
        } else if let Some(start_values) = start {
            view = tr_values_view(vm, start_values);
        }
        if let Some(propagation) = field_obj(vm, transition, "mPropagation") {
            let delay = match tr_invoke(
                vm,
                propagation,
                "getStartDelay",
                "(Landroid/view/ViewGroup;Landroid/transition/Transition;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)J",
                &[
                    Value::Obj(scene_root),
                    Value::Obj(transition),
                    start.map_or(Value::Null, Value::Obj),
                    end.map_or(Value::Null, Value::Obj),
                ],
            )? {
                Value::Long(v) => v,
                Value::Int(v) => v as i64,
                _ => 0,
            };
            let animator_index = tr_field_items(vm, transition, "mAnimators").len();
            delays.push((animator_index, delay));
            min_delay = min_delay.min(delay);
        }
        let info = alloc_obj(vm, TRANSITION_ANIMATION_INFO, Vec::new())?;
        put(vm, info, "view", view.map_or(Value::Null, Value::Obj))?;
        put(vm, info, "name", get(vm, transition, "mName"))?;
        put(vm, info, "transition", Value::Obj(transition))?;
        put(vm, info, "windowId", Value::Null)?;
        put(vm, info, "values", info_values)?;
        put(vm, info, "rdSceneRoot", Value::Obj(scene_root))?;
        tr_map_put(vm, running, Value::Obj(animator), Value::Obj(info))?;
        tr_append_field_list(vm, transition, "mAnimators", Value::Obj(animator))?;
    }
    if !delays.is_empty() && min_delay != i64::MAX {
        let animators = tr_field_items(vm, transition, "mAnimators");
        for (index, delay) in delays {
            if let Some(animator) = animators.get(index).and_then(|value| obj(Some(value))) {
                let existing = tr_animator_get_start_delay(vm, animator)?;
                tr_animator_set_start_delay(
                    vm,
                    animator,
                    delay.saturating_sub(min_delay).saturating_add(existing),
                )?;
            }
        }
    }
    Ok(())
}
fn tr_play_transition(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: ObjRef,
) -> Result<(), VmExit> {
    tr_match_start_and_end(vm, transition)?;
    let running = tr_running_animators(vm)?;
    let entries = tr_map_entries(vm, &Value::Obj(running));
    for (animator_value, info_value) in entries.into_iter().rev() {
        let Some(animator) = obj(Some(&animator_value)) else {
            continue;
        };
        let Some(info) = obj(Some(&info_value)) else {
            continue;
        };
        if get(vm, info, "rdSceneRoot") != Value::Obj(scene_root) {
            continue;
        }
        let Some(old_view) = field_obj(vm, info, "view") else {
            continue;
        };
        let old_values = field_obj(vm, info, "values");
        let start_values = tr_get_transition_values(vm, transition, Some(old_view), true)?;
        let mut end_values =
            tr_get_matched_transition_values(vm, transition, Some(old_view), true)?;
        if matches!(start_values, Value::Null) && matches!(end_values, Value::Null) {
            if let Some(maps) = tr_maps_obj(vm, transition, false) {
                if let Some(view_values) = tr_maps_field(vm, maps, "viewValues") {
                    end_values = tr_map_get(vm, &Value::Obj(view_values), &Value::Obj(old_view))
                        .unwrap_or(Value::Null);
                }
            }
        }
        let Some(old_transition) = field_obj(vm, info, "transition") else {
            continue;
        };
        let cancel = (!matches!(start_values, Value::Null) || !matches!(end_values, Value::Null))
            && tr_call_is_transition_required(
                vm,
                old_transition,
                old_values,
                obj(Some(&end_values)),
            )?;
        if cancel {
            if tr_animator_bool(vm, animator, "isRunning")?
                || tr_animator_bool(vm, animator, "isStarted")?
            {
                tr_invoke(vm, animator, "cancel", "()V", &[])?;
            } else {
                tr_map_remove(vm, running, &Value::Obj(animator))?;
            }
        }
    }
    let start_maps = tr_maps_obj(vm, transition, true).ok_or_else(|| npe("start values"))?;
    let end_maps = tr_maps_obj(vm, transition, false).ok_or_else(|| npe("end values"))?;
    let start_list = get(vm, transition, "mStartValuesList");
    let end_list = get(vm, transition, "mEndValuesList");
    tr_invoke(
        vm,
        transition,
        "createAnimators",
        "(Landroid/view/ViewGroup;Landroid/transition/TransitionValuesMaps;Landroid/transition/TransitionValuesMaps;Ljava/util/ArrayList;Ljava/util/ArrayList;)V",
        &[
            Value::Obj(scene_root),
            Value::Obj(start_maps),
            Value::Obj(end_maps),
            start_list,
            end_list,
        ],
    )?;
    tr_invoke(vm, transition, "runAnimators", "()V", &[])?;
    Ok(())
}
fn tr_force_to_end(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: Option<ObjRef>,
) -> Result<(), VmExit> {
    let Some(scene_root) = scene_root else {
        return Ok(());
    };
    let running = tr_running_animators(vm)?;
    let entries = tr_map_entries(vm, &Value::Obj(running));
    let mut animators = Vec::new();
    for (animator, info) in &entries {
        if let (Some(animator), Some(info)) = (obj(Some(animator)), obj(Some(info))) {
            if get(vm, info, "rdSceneRoot") == Value::Obj(scene_root) {
                animators.push(animator);
            }
        }
    }
    for animator in &animators {
        tr_map_remove(vm, running, &Value::Obj(*animator))?;
    }
    for animator in animators.into_iter().rev() {
        tr_invoke(vm, animator, "end", "()V", &[])?;
    }
    if tr_is_set(vm, transition) {
        for child in tr_set_children(vm, transition) {
            tr_invoke(
                vm,
                child,
                "forceToEnd",
                "(Landroid/view/ViewGroup;)V",
                &[Value::Obj(scene_root)],
            )?;
        }
    }
    Ok(())
}
fn tr_pause(vm: &mut Engine, transition: ObjRef, scene_root: Option<ObjRef>) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    if !field_bool(vm, transition, "mEnded") {
        if let Some(root) = scene_root {
            let running = tr_running_animators(vm)?;
            for (animator, info) in tr_map_entries(vm, &Value::Obj(running)).into_iter().rev() {
                if let (Some(animator), Some(info)) = (obj(Some(&animator)), obj(Some(&info))) {
                    if field_obj(vm, info, "view").is_some()
                        && get(vm, info, "rdSceneRoot") == Value::Obj(root)
                    {
                        tr_invoke(vm, animator, "pause", "()V", &[])?;
                    }
                }
            }
        }
        tr_notify_listeners(vm, transition, "onTransitionPause")?;
        put(vm, transition, "mPaused", Value::Int(1))?;
    }
    if tr_is_set(vm, transition) {
        for child in tr_set_children(vm, transition) {
            tr_invoke(
                vm,
                child,
                "pause",
                "(Landroid/view/View;)V",
                &[scene_root.map_or(Value::Null, Value::Obj)],
            )?;
        }
    }
    Ok(())
}
fn tr_resume(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: Option<ObjRef>,
) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    if field_bool(vm, transition, "mPaused") {
        if !field_bool(vm, transition, "mEnded") {
            let root = scene_root.ok_or_else(|| npe("sceneRoot"))?;
            let running = tr_running_animators(vm)?;
            for (animator, info) in tr_map_entries(vm, &Value::Obj(running)).into_iter().rev() {
                if let (Some(animator), Some(info)) = (obj(Some(&animator)), obj(Some(&info))) {
                    if field_obj(vm, info, "view").is_some()
                        && get(vm, info, "rdSceneRoot") == Value::Obj(root)
                    {
                        tr_invoke(vm, animator, "resume", "()V", &[])?;
                    }
                }
            }
            tr_notify_listeners(vm, transition, "onTransitionResume")?;
        }
        put(vm, transition, "mPaused", Value::Int(0))?;
    }
    if tr_is_set(vm, transition) {
        for child in tr_set_children(vm, transition) {
            tr_invoke(
                vm,
                child,
                "resume",
                "(Landroid/view/View;)V",
                &[scene_root.map_or(Value::Null, Value::Obj)],
            )?;
        }
    }
    Ok(())
}
fn tr_cancel(vm: &mut Engine, transition: ObjRef) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let animators = tr_field_items(vm, transition, "mCurrentAnimators")
        .iter()
        .filter_map(|value| obj(Some(value)))
        .collect::<Vec<_>>();
    for animator in animators.into_iter().rev() {
        tr_invoke(vm, animator, "cancel", "()V", &[])?;
    }
    tr_notify_listeners(vm, transition, "onTransitionCancel")?;
    if tr_is_set(vm, transition) {
        for child in tr_set_children(vm, transition) {
            tr_invoke(vm, child, "cancel", "()V", &[])?;
        }
    }
    Ok(())
}
fn tr_clone_transition(vm: &mut Engine, transition: ObjRef) -> Result<ObjRef, VmExit> {
    tr_ensure_transition(vm, transition)?;
    let (class, fields) = {
        let (class, fields) = vm
            .heap
            .as_instance(transition)
            .map_err(|message| crate::err::vm_error("INVALID_FORMAT", message))?;
        (class, fields)
    };
    let class = class.to_string();
    let fields = fields.to_vec();
    let clone = alloc_obj(vm, &class, fields)?;
    let animators = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, clone, "mAnimators", animators)?;
    let current = Value::Obj(tr_new_list(vm, Vec::new())?);
    put(vm, clone, "mCurrentAnimators", current)?;
    let start_maps = tr_new_values_maps(vm)?;
    put(vm, clone, "mStartValues", start_maps)?;
    let end_maps = tr_new_values_maps(vm)?;
    put(vm, clone, "mEndValues", end_maps)?;
    put(vm, clone, "mStartValuesList", Value::Null)?;
    put(vm, clone, "mEndValuesList", Value::Null)?;
    put(vm, clone, "mNumInstances", Value::Int(0))?;
    put(vm, clone, "mEnded", Value::Int(0))?;
    if tr_is_set(vm, transition) {
        let empty = Value::Obj(tr_new_list(vm, Vec::new())?);
        put(vm, clone, "mTransitions", empty)?;
        for child in tr_set_children(vm, transition) {
            let child_clone = tr_clone_transition(vm, child)?;
            put(vm, child_clone, "mParent", Value::Obj(clone))?;
            tr_append_field_list(vm, clone, "mTransitions", Value::Obj(child_clone))?;
        }
    }
    Ok(clone)
}

fn tr_values_get_key(
    vm: &mut Engine,
    values: Option<ObjRef>,
    key: &str,
) -> Result<Option<Value>, VmExit> {
    let Some(values) = values else {
        return Ok(None);
    };
    let key = jstr(vm, key)?;
    Ok(tr_values_get_with_key(vm, Some(values), &key))
}
fn tr_values_contains_key(
    vm: &mut Engine,
    values: Option<ObjRef>,
    key: &str,
) -> Result<bool, VmExit> {
    let Some(values) = values else {
        return Ok(false);
    };
    let key = jstr(vm, key)?;
    Ok(tr_map_contains(vm, &tr_values_map(vm, values), &key))
}
#[derive(Clone, Copy)]
struct TrVisibilityInfo {
    changed: bool,
    fade_in: bool,
    start_visibility: i32,
    end_visibility: i32,
    start_parent: Option<ObjRef>,
    end_parent: Option<ObjRef>,
}
fn tr_visibility_info(
    vm: &mut Engine,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<TrVisibilityInfo, VmExit> {
    let mut info = TrVisibilityInfo {
        changed: false,
        fade_in: false,
        start_visibility: -1,
        end_visibility: -1,
        start_parent: None,
        end_parent: None,
    };
    if tr_values_contains_key(vm, start, TR_PROP_VISIBILITY)? {
        info.start_visibility = tr_values_get_key(vm, start, TR_PROP_VISIBILITY)?
            .and_then(|value| tr_unbox_int(vm, &value))
            .ok_or_else(|| npe("visibility"))?;
        info.start_parent = tr_values_get_key(vm, start, TR_PROP_VISIBILITY_PARENT)?
            .and_then(|value| obj(Some(&value)));
    }
    if tr_values_contains_key(vm, end, TR_PROP_VISIBILITY)? {
        info.end_visibility = tr_values_get_key(vm, end, TR_PROP_VISIBILITY)?
            .and_then(|value| tr_unbox_int(vm, &value))
            .ok_or_else(|| npe("visibility"))?;
        info.end_parent = tr_values_get_key(vm, end, TR_PROP_VISIBILITY_PARENT)?
            .and_then(|value| obj(Some(&value)));
    }
    if start.is_some() && end.is_some() {
        if info.start_visibility == info.end_visibility && info.start_parent == info.end_parent {
            return Ok(info);
        }
        if info.start_visibility != info.end_visibility {
            if info.start_visibility == 0 {
                info.changed = true;
            } else if info.end_visibility == 0 {
                info.fade_in = true;
                info.changed = true;
            }
        } else if info.start_parent != info.end_parent {
            if info.end_parent.is_none() {
                info.changed = true;
            } else if info.start_parent.is_none() {
                info.fade_in = true;
                info.changed = true;
            }
        }
    } else if start.is_none() && info.end_visibility == 0 {
        info.fade_in = true;
        info.changed = true;
    } else if end.is_none() && info.start_visibility == 0 {
        info.changed = true;
    }
    Ok(info)
}
fn tr_visibility_is_required(
    vm: &mut Engine,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<bool, VmExit> {
    if start.is_none() && end.is_none() {
        return Ok(false);
    }
    if start.is_some() && end.is_some() {
        let start_has = tr_values_contains_key(vm, start, TR_PROP_VISIBILITY)?;
        let end_has = tr_values_contains_key(vm, end, TR_PROP_VISIBILITY)?;
        if start_has != end_has {
            return Ok(false);
        }
    }
    let info = tr_visibility_info(vm, start, end)?;
    Ok(info.changed && (info.start_visibility == 0 || info.end_visibility == 0))
}
fn tr_capture_visibility_values(vm: &mut Engine, values: ObjRef) -> Result<(), VmExit> {
    let view = tr_values_view(vm, values).ok_or_else(|| npe("view"))?;
    let visibility = match tr_invoke(vm, view, "getVisibility", "()I", &[])? {
        Value::Int(v) => v,
        _ => 0,
    };
    let boxed = tr_box_int(vm, visibility)?;
    tr_values_put(vm, values, TR_PROP_VISIBILITY, boxed)?;
    let parent = tr_view_parent(vm, view)?;
    tr_values_put(vm, values, TR_PROP_VISIBILITY_PARENT, parent)?;
    let location = new_int_array(vm, &[0, 0])?;
    match tr_invoke(
        vm,
        view,
        "getLocationOnScreen",
        "([I)V",
        std::slice::from_ref(&location),
    ) {
        Ok(_) => {}
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
        Err(error) => return Err(error),
    }
    tr_values_put(vm, values, TR_PROP_VISIBILITY_SCREEN, location)
}
fn tr_visibility_on_appear5(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: ObjRef,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<Value, VmExit> {
    tr_ensure_transition(vm, transition)?;
    if field_int(vm, transition, "mMode") & 1 == 0 {
        return Ok(Value::Null);
    }
    let Some(end) = end else {
        return Ok(Value::Null);
    };
    if start.is_none() {
        let end_view = tr_values_view(vm, end).ok_or_else(|| npe("view"))?;
        let parent = tr_view_parent(vm, end_view)?;
        let parent_ref = obj(Some(&parent));
        let start_parent_values =
            tr_get_matched_transition_values(vm, transition, parent_ref, false)?;
        let end_parent_values = tr_get_transition_values(vm, transition, parent_ref, false)?;
        let info = tr_visibility_info(
            vm,
            obj(Some(&start_parent_values)),
            obj(Some(&end_parent_values)),
        )?;
        if info.changed {
            return Ok(Value::Null);
        }
    }
    let view = tr_values_view(vm, end).ok_or_else(|| npe("view"))?;
    tr_invoke(
        vm,
        transition,
        "onAppear",
        "(Landroid/view/ViewGroup;Landroid/view/View;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        &[
            Value::Obj(scene_root),
            Value::Obj(view),
            start.map_or(Value::Null, Value::Obj),
            Value::Obj(end),
        ],
    )
}
fn tr_set_transition_visibility(
    vm: &mut Engine,
    view: ObjRef,
    visibility: i32,
) -> Result<(), VmExit> {
    match tr_invoke(
        vm,
        view,
        "setTransitionVisibility",
        "(I)V",
        &[Value::Int(visibility)],
    ) {
        Ok(_) => Ok(()),
        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => Ok(()),
        Err(error) => Err(error),
    }
}
fn tr_visibility_on_disappear5(
    vm: &mut Engine,
    transition: ObjRef,
    scene_root: ObjRef,
    start: Option<ObjRef>,
    start_visibility: i32,
    end: Option<ObjRef>,
    end_visibility: i32,
) -> Result<Value, VmExit> {
    let _ = start_visibility;
    tr_ensure_transition(vm, transition)?;
    if field_int(vm, transition, "mMode") & 2 == 0 {
        return Ok(Value::Null);
    }
    let Some(start) = start else {
        return Ok(Value::Null);
    };
    let start_view = tr_values_view(vm, start).ok_or_else(|| npe("view"))?;
    let end_view = end.and_then(|values| tr_values_view(vm, values));
    let mut view_to_keep = None;
    let mut direct_view = None;
    if let Some(end_view_ref) = end_view {
        let parent = tr_view_parent(vm, end_view_ref)?;
        if matches!(parent, Value::Obj(_)) {
            if end_visibility == 4 || start_view == end_view_ref {
                view_to_keep = Some(end_view_ref);
            }
        } else {
            direct_view = Some(end_view_ref);
        }
    } else if matches!(tr_view_parent(vm, start_view)?, Value::Null) {
        direct_view = Some(start_view);
    }
    if let Some(view) = direct_view {
        return tr_invoke(
            vm,
            transition,
            "onDisappear",
            "(Landroid/view/ViewGroup;Landroid/view/View;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
            &[
                Value::Obj(scene_root),
                Value::Obj(view),
                Value::Obj(start),
                end.map_or(Value::Null, Value::Obj),
            ],
        );
    }
    let Some(view) = view_to_keep else {
        // Snapshot/overlay copies for removed, still-parented hierarchies are
        // outside this bounded slice.
        return Ok(Value::Null);
    };
    let original = match tr_invoke(vm, view, "getVisibility", "()I", &[])? {
        Value::Int(v) => v,
        _ => end_visibility,
    };
    tr_set_transition_visibility(vm, view, 0)?;
    let animator = tr_invoke(
        vm,
        transition,
        "onDisappear",
        "(Landroid/view/ViewGroup;Landroid/view/View;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        &[
            Value::Obj(scene_root),
            Value::Obj(view),
            Value::Obj(start),
            end.map_or(Value::Null, Value::Obj),
        ],
    )?;
    if let Some(animator_ref) = obj(Some(&animator)) {
        let listener = alloc_obj(vm, TRANSITION_DISAPPEAR_LISTENER, Vec::new())?;
        tr_invoke(
            vm,
            listener,
            "<init>",
            "(Landroid/view/View;IZ)V",
            &[
                Value::Obj(view),
                Value::Int(end_visibility),
                bool_value(field_bool(vm, transition, "mSuppressLayout")),
            ],
        )?;
        tr_animator_add_listener(vm, animator_ref, listener, false)?;
        tr_animator_add_listener(vm, animator_ref, listener, true)?;
        tr_invoke(
            vm,
            transition,
            "addListener",
            "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
            &[Value::Obj(listener)],
        )?;
    } else {
        tr_set_transition_visibility(vm, view, original)?;
    }
    Ok(animator)
}
fn tr_fade_start_alpha(
    vm: &mut Engine,
    start: Option<ObjRef>,
    fallback: f32,
) -> Result<f32, VmExit> {
    Ok(tr_values_get_key(vm, start, TR_PROP_FADE_ALPHA)?
        .and_then(|value| tr_unbox_float(vm, &value))
        .unwrap_or(fallback))
}
fn tr_fade_create_animation(
    vm: &mut Engine,
    transition: ObjRef,
    view: ObjRef,
    start_alpha: f32,
    end_alpha: f32,
) -> Result<Value, VmExit> {
    if start_alpha == end_alpha {
        return Ok(Value::Null);
    }
    tr_invoke(
        vm,
        view,
        "setTransitionAlpha",
        "(F)V",
        &[Value::Float(start_alpha)],
    )?;
    let animator = anim_new_object_animator(
        vm,
        Value::Obj(view),
        Some("transitionAlpha".to_string()),
        Value::Null,
    )?;
    anim_animator_set_float_values(vm, animator, &[end_alpha], true)?;
    let listener = alloc_obj(vm, TRANSITION_FADE_ANIM_LISTENER, Vec::new())?;
    put(vm, listener, "mView", Value::Obj(view))?;
    put(vm, listener, "mLayerTypeChanged", Value::Int(0))?;
    tr_animator_add_listener(vm, animator, listener, false)?;
    let transition_listener = alloc_obj(vm, TRANSITION_FADE_LISTENER, Vec::new())?;
    put(vm, transition_listener, "this$0", Value::Obj(transition))?;
    put(vm, transition_listener, "val$view", Value::Obj(view))?;
    tr_invoke(
        vm,
        transition,
        "addListener",
        "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        &[Value::Obj(transition_listener)],
    )?;
    Ok(Value::Obj(animator))
}
fn tr_new_rect(vm: &mut Engine, bounds: [i32; 4]) -> Result<Value, VmExit> {
    let rect = alloc_obj(vm, "Landroid/graphics/Rect;", Vec::new())?;
    put(vm, rect, "left", Value::Int(bounds[0]))?;
    put(vm, rect, "top", Value::Int(bounds[1]))?;
    put(vm, rect, "right", Value::Int(bounds[2]))?;
    put(vm, rect, "bottom", Value::Int(bounds[3]))?;
    Ok(Value::Obj(rect))
}
fn tr_rect_values(vm: &Engine, value: &Value) -> Option<[i32; 4]> {
    let rect = obj(Some(value))?;
    if !vm
        .cp
        .is_subtype(&class_of(vm, rect), "Landroid/graphics/Rect;")
    {
        return None;
    }
    Some([
        field_int(vm, rect, "left"),
        field_int(vm, rect, "top"),
        field_int(vm, rect, "right"),
        field_int(vm, rect, "bottom"),
    ])
}
fn tr_view_int(vm: &mut Engine, view: ObjRef, method: &str) -> Result<i32, VmExit> {
    Ok(match tr_invoke(vm, view, method, "()I", &[])? {
        Value::Int(v) => v,
        _ => 0,
    })
}
fn tr_capture_change_bounds_values(
    vm: &mut Engine,
    transition: ObjRef,
    values: ObjRef,
) -> Result<(), VmExit> {
    tr_ensure_transition(vm, transition)?;
    let view = tr_values_view(vm, values).ok_or_else(|| npe("view"))?;
    let laid_out = tr_invoke(vm, view, "isLaidOut", "()Z", &[])? == Value::Int(1);
    let width = tr_view_int(vm, view, "getWidth")?;
    let height = tr_view_int(vm, view, "getHeight")?;
    if laid_out || width != 0 || height != 0 {
        let left = tr_view_int(vm, view, "getLeft")?;
        let top = tr_view_int(vm, view, "getTop")?;
        let right = tr_view_int(vm, view, "getRight")?;
        let bottom = tr_view_int(vm, view, "getBottom")?;
        let bounds = tr_new_rect(vm, [left, top, right, bottom])?;
        tr_values_put(vm, values, TR_PROP_BOUNDS, bounds)?;
        let parent = tr_view_parent(vm, view)?;
        tr_values_put(vm, values, TR_PROP_BOUNDS_PARENT, parent)?;
        if field_bool(vm, transition, "mReparent") {
            let location = new_int_array(vm, &[0, 0])?;
            match tr_invoke(
                vm,
                view,
                "getLocationInWindow",
                "([I)V",
                std::slice::from_ref(&location),
            ) {
                Ok(_) => {}
                Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
                Err(error) => return Err(error),
            }
            let coords = int_values(vm, Some(&location));
            let x = tr_box_int(vm, coords.first().copied().unwrap_or(0))?;
            tr_values_put(vm, values, TR_PROP_WINDOW_X, x)?;
            let y = tr_box_int(vm, coords.get(1).copied().unwrap_or(0))?;
            tr_values_put(vm, values, TR_PROP_WINDOW_Y, y)?;
        }
        if field_bool(vm, transition, "mResizeClip") {
            let clip = match tr_invoke(vm, view, "getClipBounds", "()Landroid/graphics/Rect;", &[])
            {
                Ok(v) => v,
                Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => Value::Null,
                Err(error) => return Err(error),
            };
            tr_values_put(vm, values, TR_PROP_CLIP, clip)?;
        }
    }
    Ok(())
}
fn tr_view_layout(vm: &mut Engine, view: ObjRef, bounds: [i32; 4]) -> Result<(), VmExit> {
    tr_invoke(
        vm,
        view,
        "layout",
        "(IIII)V",
        &[
            Value::Int(bounds[0]),
            Value::Int(bounds[1]),
            Value::Int(bounds[2]),
            Value::Int(bounds[3]),
        ],
    )?;
    Ok(())
}
fn tr_change_bounds_create_animator(
    vm: &mut Engine,
    transition: ObjRef,
    start: Option<ObjRef>,
    end: Option<ObjRef>,
) -> Result<Value, VmExit> {
    tr_ensure_transition(vm, transition)?;
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(Value::Null);
    };
    let start_parent = tr_values_get_key(vm, Some(start), TR_PROP_BOUNDS_PARENT)?
        .and_then(|value| obj(Some(&value)));
    let end_parent = tr_values_get_key(vm, Some(end), TR_PROP_BOUNDS_PARENT)?
        .and_then(|value| obj(Some(&value)));
    let (Some(start_parent), Some(end_parent)) = (start_parent, end_parent) else {
        return Ok(Value::Null);
    };
    if field_bool(vm, transition, "mReparent") {
        let matched = tr_get_matched_transition_values(vm, transition, Some(start_parent), true)?;
        let matches = if matches!(matched, Value::Null) {
            start_parent == end_parent
        } else {
            obj(Some(&matched)).and_then(|values| tr_values_view(vm, values)) == Some(end_parent)
        };
        if !matches {
            // Cross-parent overlay rendering is outside this bounded slice.
            return Ok(Value::Null);
        }
    }
    let start_bounds = tr_values_get_key(vm, Some(start), TR_PROP_BOUNDS)?
        .and_then(|value| tr_rect_values(vm, &value))
        .ok_or_else(|| npe("startBounds"))?;
    let end_bounds = tr_values_get_key(vm, Some(end), TR_PROP_BOUNDS)?
        .and_then(|value| tr_rect_values(vm, &value))
        .ok_or_else(|| npe("endBounds"))?;
    let start_clip_value = tr_values_get_key(vm, Some(start), TR_PROP_CLIP)?.unwrap_or(Value::Null);
    let end_clip_value = tr_values_get_key(vm, Some(end), TR_PROP_CLIP)?.unwrap_or(Value::Null);
    let start_clip = tr_rect_values(vm, &start_clip_value);
    let end_clip = tr_rect_values(vm, &end_clip_value);
    let start_width = start_bounds[2] - start_bounds[0];
    let start_height = start_bounds[3] - start_bounds[1];
    let end_width = end_bounds[2] - end_bounds[0];
    let end_height = end_bounds[3] - end_bounds[1];
    let mut changes = 0;
    if (start_width != 0 && start_height != 0) || (end_width != 0 && end_height != 0) {
        if start_bounds[0] != end_bounds[0] || start_bounds[1] != end_bounds[1] {
            changes += 1;
        }
        if start_bounds[2] != end_bounds[2] || start_bounds[3] != end_bounds[3] {
            changes += 1;
        }
    }
    if start_clip != end_clip {
        changes += 1;
    }
    if changes == 0 {
        return Ok(Value::Null);
    }
    let view = tr_values_view(vm, end).ok_or_else(|| npe("view"))?;
    let resize_clip = field_bool(vm, transition, "mResizeClip");
    let max_width = start_width.max(end_width);
    let max_height = start_height.max(end_height);
    let effective_start_clip = start_clip.unwrap_or([0, 0, start_width, start_height]);
    let effective_end_clip = end_clip.unwrap_or([0, 0, end_width, end_height]);
    if resize_clip {
        tr_view_layout(
            vm,
            view,
            [
                start_bounds[0],
                start_bounds[1],
                start_bounds[0] + max_width,
                start_bounds[1] + max_height,
            ],
        )?;
        if effective_start_clip != effective_end_clip {
            let clip = tr_new_rect(vm, effective_start_clip)?;
            tr_invoke(
                vm,
                view,
                "setClipBounds",
                "(Landroid/graphics/Rect;)V",
                &[clip],
            )?;
        }
    } else {
        tr_view_layout(vm, view, start_bounds)?;
    }
    let animator = anim_new_animator(vm, VALUE_ANIMATOR)?;
    anim_animator_set_float_values(vm, animator, &[0.0, 1.0], false)?;
    let listener = alloc_obj(vm, TRANSITION_CHANGE_BOUNDS_LISTENER, Vec::new())?;
    put(vm, listener, "mView", Value::Obj(view))?;
    put(vm, listener, "mResizeClip", bool_value(resize_clip))?;
    put(vm, listener, "mMaxWidth", Value::Int(max_width))?;
    put(vm, listener, "mMaxHeight", Value::Int(max_height))?;
    for (field, value) in [
        ("mStartLeft", start_bounds[0]),
        ("mStartTop", start_bounds[1]),
        ("mStartRight", start_bounds[2]),
        ("mStartBottom", start_bounds[3]),
        ("mEndLeft", end_bounds[0]),
        ("mEndTop", end_bounds[1]),
        ("mEndRight", end_bounds[2]),
        ("mEndBottom", end_bounds[3]),
        ("mStartClipLeft", effective_start_clip[0]),
        ("mStartClipTop", effective_start_clip[1]),
        ("mStartClipRight", effective_start_clip[2]),
        ("mStartClipBottom", effective_start_clip[3]),
        ("mEndClipLeft", effective_end_clip[0]),
        ("mEndClipTop", effective_end_clip[1]),
        ("mEndClipRight", effective_end_clip[2]),
        ("mEndClipBottom", effective_end_clip[3]),
    ] {
        put(vm, listener, field, Value::Int(value))?;
    }
    put(vm, listener, "mFinalClip", end_clip_value)?;
    tr_invoke(
        vm,
        animator,
        "addUpdateListener",
        "(Landroid/animation/ValueAnimator$AnimatorUpdateListener;)V",
        &[Value::Obj(listener)],
    )?;
    Ok(Value::Obj(animator))
}
fn tr_interpolate_int(start: i32, end: i32, fraction: f32) -> i32 {
    (start as f32 + (end - start) as f32 * fraction).round() as i32
}
fn tr_change_bounds_update(
    vm: &mut Engine,
    listener: ObjRef,
    animator: ObjRef,
) -> Result<(), VmExit> {
    let fraction = match tr_invoke(vm, animator, "getAnimatedFraction", "()F", &[])? {
        Value::Float(v) => v.clamp(0.0, 1.0),
        _ => 0.0,
    };
    let view = field_obj(vm, listener, "mView").ok_or_else(|| npe("view"))?;
    let left = tr_interpolate_int(
        field_int(vm, listener, "mStartLeft"),
        field_int(vm, listener, "mEndLeft"),
        fraction,
    );
    let top = tr_interpolate_int(
        field_int(vm, listener, "mStartTop"),
        field_int(vm, listener, "mEndTop"),
        fraction,
    );
    let right = tr_interpolate_int(
        field_int(vm, listener, "mStartRight"),
        field_int(vm, listener, "mEndRight"),
        fraction,
    );
    let bottom = tr_interpolate_int(
        field_int(vm, listener, "mStartBottom"),
        field_int(vm, listener, "mEndBottom"),
        fraction,
    );
    if field_bool(vm, listener, "mResizeClip") {
        if fraction >= 1.0 {
            let clip = get(vm, listener, "mFinalClip");
            tr_invoke(
                vm,
                view,
                "setClipBounds",
                "(Landroid/graphics/Rect;)V",
                &[clip],
            )?;
            tr_view_layout(vm, view, [left, top, right, bottom])?;
        } else {
            tr_view_layout(
                vm,
                view,
                [
                    left,
                    top,
                    left + field_int(vm, listener, "mMaxWidth"),
                    top + field_int(vm, listener, "mMaxHeight"),
                ],
            )?;
            let clip = tr_new_rect(
                vm,
                [
                    tr_interpolate_int(
                        field_int(vm, listener, "mStartClipLeft"),
                        field_int(vm, listener, "mEndClipLeft"),
                        fraction,
                    ),
                    tr_interpolate_int(
                        field_int(vm, listener, "mStartClipTop"),
                        field_int(vm, listener, "mEndClipTop"),
                        fraction,
                    ),
                    tr_interpolate_int(
                        field_int(vm, listener, "mStartClipRight"),
                        field_int(vm, listener, "mEndClipRight"),
                        fraction,
                    ),
                    tr_interpolate_int(
                        field_int(vm, listener, "mStartClipBottom"),
                        field_int(vm, listener, "mEndClipBottom"),
                        fraction,
                    ),
                ],
            )?;
            tr_invoke(
                vm,
                view,
                "setClipBounds",
                "(Landroid/graphics/Rect;)V",
                &[clip],
            )?;
        }
    } else {
        tr_view_layout(vm, view, [left, top, right, bottom])?;
    }
    Ok(())
}

fn tr_record_transition_listener(
    vm: &mut Engine,
    listener: ObjRef,
    method: &str,
    transition: ObjRef,
) -> Result<(), VmExit> {
    let field = match method {
        "onTransitionStart" => "rd_on_transition_start_count",
        "onTransitionEnd" => "rd_on_transition_end_count",
        "onTransitionCancel" => "rd_on_transition_cancel_count",
        "onTransitionPause" => "rd_on_transition_pause_count",
        "onTransitionResume" => "rd_on_transition_resume_count",
        _ => return Ok(()),
    };
    let count = field_int(vm, listener, field).wrapping_add(1);
    put(vm, listener, field, Value::Int(count))?;
    put(vm, listener, "rd_last_transition", Value::Obj(transition))
}
fn tr_remove_transition_listener(
    vm: &mut Engine,
    transition: ObjRef,
    listener: ObjRef,
) -> Result<(), VmExit> {
    tr_invoke(
        vm,
        transition,
        "removeListener",
        "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        &[Value::Obj(listener)],
    )?;
    Ok(())
}
fn tr_disappear_suppress(vm: &mut Engine, listener: ObjRef, suppress: bool) -> Result<(), VmExit> {
    if field_bool(vm, listener, "mSuppressLayout")
        && field_bool(vm, listener, "mLayoutSuppressed") != suppress
    {
        if let Some(parent) = field_obj(vm, listener, "mParent") {
            put(vm, listener, "mLayoutSuppressed", bool_value(suppress))?;
            match tr_invoke(
                vm,
                parent,
                "suppressLayout",
                "(Z)V",
                &[bool_value(suppress)],
            ) {
                Ok(_) => {}
                Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}
fn tr_disappear_hide(vm: &mut Engine, listener: ObjRef) -> Result<(), VmExit> {
    if !field_bool(vm, listener, "mCanceled") {
        let view = field_obj(vm, listener, "mView").ok_or_else(|| npe("view"))?;
        tr_set_transition_visibility(vm, view, field_int(vm, listener, "mFinalVisibility"))?;
        if let Some(parent) = field_obj(vm, listener, "mParent") {
            match tr_invoke(vm, parent, "invalidate", "()V", &[]) {
                Ok(_) => {}
                Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => {}
                Err(error) => return Err(error),
            }
        }
    }
    tr_disappear_suppress(vm, listener, false)
}

fn transition_callback_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        match class {
            TRANSITION_RUN_LISTENER | TRANSITION_ANIMATE_LISTENER if !args.is_empty() => {
                put(
                    vm,
                    recv,
                    "this$0",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                if let Some(map) = args.get(1) {
                    put(vm, recv, "val$runningAnimators", map.clone())?;
                }
            }
            TRANSITION_SET_LISTENER => {
                put(
                    vm,
                    recv,
                    "mTransitionSet",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
            }
            TRANSITION_SET_NEXT_LISTENER => {
                put(
                    vm,
                    recv,
                    "this$0",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(
                    vm,
                    recv,
                    "val$nextTransition",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
            }
            TRANSITION_MANAGER_MULTI => {
                put(
                    vm,
                    recv,
                    "mTransition",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                let root = args.get(1).cloned().unwrap_or(Value::Null);
                put(vm, recv, "mSceneRoot", root.clone())?;
                if let Some(root_ref) = obj(Some(&root)) {
                    let observer = match tr_invoke(
                        vm,
                        root_ref,
                        "getViewTreeObserver",
                        "()Landroid/view/ViewTreeObserver;",
                        &[],
                    ) {
                        Ok(v) => v,
                        Err(VmExit::Error(error)) if error.code == "NOT_FOUND" => Value::Null,
                        Err(error) => return Err(error),
                    };
                    put(vm, recv, "mViewTreeObserver", observer)?;
                }
            }
            TRANSITION_MANAGER_END_LISTENER => {
                put(
                    vm,
                    recv,
                    "val$sceneRoot",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(
                    vm,
                    recv,
                    "val$runningTransitions",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
            }
            TRANSITION_FADE_LISTENER => {
                put(
                    vm,
                    recv,
                    "this$0",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(
                    vm,
                    recv,
                    "val$view",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
            }
            TRANSITION_FADE_ANIM_LISTENER => {
                put(
                    vm,
                    recv,
                    "mView",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(vm, recv, "mLayerTypeChanged", Value::Int(0))?;
            }
            TRANSITION_DISAPPEAR_LISTENER => {
                let view = req_obj(args, 0, "view")?;
                put(vm, recv, "mView", Value::Obj(view))?;
                put(vm, recv, "mFinalVisibility", Value::Int(iarg(args, 1)))?;
                let parent = tr_view_parent(vm, view)?;
                put(vm, recv, "mParent", parent)?;
                put(
                    vm,
                    recv,
                    "mSuppressLayout",
                    Value::Int(i32::from(iarg(args, 2) != 0)),
                )?;
                put(vm, recv, "mLayoutSuppressed", Value::Int(0))?;
                put(vm, recv, "mCanceled", Value::Int(0))?;
                tr_disappear_suppress(vm, recv, true)?;
            }
            _ => {}
        }
        return Ok(Some(Value::Null));
    }
    match class {
        TRANSITION_STRAIGHT_PATH if name == "getPath" && sig == "(FFFF)Landroid/graphics/Path;" => {
            let path = alloc_obj(vm, "Landroid/graphics/Path;", Vec::new())?;
            let _ = tr_invoke(vm, path, "<init>", "()V", &[]);
            tr_invoke(
                vm,
                path,
                "moveTo",
                "(FF)V",
                &[Value::Float(farg(args, 0)), Value::Float(farg(args, 1))],
            )?;
            tr_invoke(
                vm,
                path,
                "lineTo",
                "(FF)V",
                &[Value::Float(farg(args, 2)), Value::Float(farg(args, 3))],
            )?;
            Ok(Some(Value::Obj(path)))
        }
        TRANSITION_PATH_MOTION if name == "getPath" => Ok(Some(Value::Null)),
        TRANSITION_EPICENTER_CALLBACK if name == "onGetEpicenter" => Ok(Some(Value::Null)),
        TRANSITION_PROPAGATION => Ok(Some(match name {
            "getStartDelay" => Value::Long(0),
            "getPropagationProperties" => Value::Null,
            _ => Value::Null,
        })),
        TRANSITION_RUN_LISTENER => {
            let Some(transition) = field_obj(vm, recv, "this$0") else {
                return Ok(Some(Value::Null));
            };
            let animator = args.first().cloned().unwrap_or(Value::Null);
            match name {
                "onAnimationStart" => {
                    tr_append_field_list(vm, transition, "mCurrentAnimators", animator)?;
                }
                "onAnimationEnd" => {
                    if let Some(map) = field_obj(vm, recv, "val$runningAnimators") {
                        tr_map_remove(vm, map, &animator)?;
                    }
                    tr_remove_field_list(vm, transition, "mCurrentAnimators", &animator)?;
                }
                _ => {}
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_ANIMATE_LISTENER => {
            if name == "onAnimationEnd" {
                let transition = field_obj(vm, recv, "this$0").ok_or_else(|| npe("transition"))?;
                tr_end(vm, transition)?;
                if let Some(animator) = obj(args.first()) {
                    tr_invoke(
                        vm,
                        animator,
                        "removeListener",
                        "(Landroid/animation/Animator$AnimatorListener;)V",
                        &[Value::Obj(recv)],
                    )?;
                }
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_SET_LISTENER => {
            let set = field_obj(vm, recv, "mTransitionSet").ok_or_else(|| npe("transitionSet"))?;
            match name {
                "onTransitionStart" => {
                    if !field_bool(vm, set, "mStarted") {
                        tr_start(vm, set)?;
                        put(vm, set, "mStarted", Value::Int(1))?;
                    }
                }
                "onTransitionEnd" => {
                    let count = field_int(vm, set, "mCurrentListeners").saturating_sub(1);
                    put(vm, set, "mCurrentListeners", Value::Int(count))?;
                    if count == 0 {
                        put(vm, set, "mStarted", Value::Int(0))?;
                        tr_end(vm, set)?;
                    }
                    if let Some(child) = obj(args.first()) {
                        tr_remove_transition_listener(vm, child, recv)?;
                    }
                }
                _ => {}
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_SET_NEXT_LISTENER => {
            if name == "onTransitionEnd" {
                let next = field_obj(vm, recv, "val$nextTransition")
                    .ok_or_else(|| npe("nextTransition"))?;
                tr_invoke(vm, next, "runAnimators", "()V", &[])?;
                if let Some(previous) = obj(args.first()) {
                    tr_remove_transition_listener(vm, previous, recv)?;
                }
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_MANAGER_MULTI => match (name, sig) {
            ("onPreDraw", "()Z") => tr_manager_on_pre_draw(vm, recv).map(Some),
            ("onViewDetachedFromWindow", "(Landroid/view/View;)V") => {
                tr_manager_on_detached(vm, recv)?;
                Ok(Some(Value::Null))
            }
            ("onViewAttachedToWindow", "(Landroid/view/View;)V") | ("removeListeners", "()V") => {
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        TRANSITION_MANAGER_END_LISTENER => {
            if name == "onTransitionEnd" {
                if let (Some(root), Some(transition)) =
                    (field_obj(vm, recv, "val$sceneRoot"), obj(args.first()))
                {
                    let running = tr_manager_running_transitions(vm)?;
                    if let Some(list_value) =
                        tr_map_get(vm, &Value::Obj(running), &Value::Obj(root))
                    {
                        if let Some(list) = obj(Some(&list_value)) {
                            let mut items = tr_items(vm, &list_value);
                            if let Some(index) = items
                                .iter()
                                .position(|value| *value == Value::Obj(transition))
                            {
                                items.remove(index);
                            }
                            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
                                *elems = items;
                            }
                        }
                    }
                    tr_remove_transition_listener(vm, transition, recv)?;
                }
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_FADE_LISTENER => {
            if name == "onTransitionEnd" {
                if let Some(view) = field_obj(vm, recv, "val$view") {
                    tr_invoke(vm, view, "setTransitionAlpha", "(F)V", &[Value::Float(1.0)])?;
                }
                if let Some(transition) = obj(args.first()) {
                    tr_remove_transition_listener(vm, transition, recv)?;
                }
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_FADE_ANIM_LISTENER => {
            let view = field_obj(vm, recv, "mView").ok_or_else(|| npe("view"))?;
            match name {
                "onAnimationStart" => {
                    let overlapping = tr_invoke(vm, view, "hasOverlappingRendering", "()Z", &[])?
                        == Value::Int(1);
                    let layer = tr_view_int(vm, view, "getLayerType")?;
                    if overlapping && layer == 0 {
                        put(vm, recv, "mLayerTypeChanged", Value::Int(1))?;
                        tr_invoke(
                            vm,
                            view,
                            "setLayerType",
                            "(ILandroid/graphics/Paint;)V",
                            &[Value::Int(2), Value::Null],
                        )?;
                    }
                }
                "onAnimationEnd" => {
                    tr_invoke(vm, view, "setTransitionAlpha", "(F)V", &[Value::Float(1.0)])?;
                    if field_bool(vm, recv, "mLayerTypeChanged") {
                        tr_invoke(
                            vm,
                            view,
                            "setLayerType",
                            "(ILandroid/graphics/Paint;)V",
                            &[Value::Int(0), Value::Null],
                        )?;
                    }
                }
                _ => {}
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_DISAPPEAR_LISTENER => {
            match name {
                "onAnimationPause" => {
                    if !field_bool(vm, recv, "mCanceled") {
                        let view = field_obj(vm, recv, "mView").ok_or_else(|| npe("view"))?;
                        tr_set_transition_visibility(
                            vm,
                            view,
                            field_int(vm, recv, "mFinalVisibility"),
                        )?;
                    }
                }
                "onAnimationResume" => {
                    if !field_bool(vm, recv, "mCanceled") {
                        let view = field_obj(vm, recv, "mView").ok_or_else(|| npe("view"))?;
                        tr_set_transition_visibility(vm, view, 0)?;
                    }
                }
                "onAnimationCancel" => put(vm, recv, "mCanceled", Value::Int(1))?,
                "onAnimationEnd" => tr_disappear_hide(vm, recv)?,
                "onTransitionEnd" => {
                    tr_disappear_hide(vm, recv)?;
                    if let Some(transition) = obj(args.first()) {
                        tr_remove_transition_listener(vm, transition, recv)?;
                    }
                }
                "onTransitionPause" => tr_disappear_suppress(vm, recv, false)?,
                "onTransitionResume" => tr_disappear_suppress(vm, recv, true)?,
                _ => {}
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_CHANGE_BOUNDS_LISTENER => {
            if name == "onAnimationUpdate" {
                let animator = req_obj(args, 0, "animation")?;
                tr_change_bounds_update(vm, recv, animator)?;
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_LISTENER | TRANSITION_LISTENER_ADAPTER => {
            if sig == "(Landroid/transition/Transition;)V" {
                if let Some(transition) = obj(args.first()) {
                    tr_record_transition_listener(vm, recv, name, transition)?;
                }
            }
            Ok(Some(Value::Null))
        }
        TRANSITION_VISIBILITY_OVERLAY_LISTENER => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}

fn tr_scene_ensure(vm: &mut Engine, scene: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, scene, "rdSceneInit") {
        return Ok(());
    }
    put(vm, scene, "rdSceneInit", Value::Int(1))?;
    put(vm, scene, "mContext", Value::Null)?;
    put(vm, scene, "mLayoutId", Value::Int(-1))?;
    put(vm, scene, "mSceneRoot", Value::Null)?;
    put(vm, scene, "mLayout", Value::Null)?;
    put(vm, scene, "mEnterAction", Value::Null)?;
    put(vm, scene, "mExitAction", Value::Null)
}
fn tr_scene_current(vm: &Engine, root: ObjRef) -> Value {
    get(vm, root, "rdCurrentScene")
}
fn tr_scene_set_current(vm: &mut Engine, root: ObjRef, scene: Value) -> Result<(), VmExit> {
    put(vm, root, "rdCurrentScene", scene)
}
fn tr_scene_enter(vm: &mut Engine, scene: ObjRef) -> Result<(), VmExit> {
    tr_scene_ensure(vm, scene)?;
    let root = field_obj(vm, scene, "mSceneRoot").ok_or_else(|| npe("sceneRoot"))?;
    let layout_id = field_int(vm, scene, "mLayoutId");
    let layout = get(vm, scene, "mLayout");
    if layout_id > 0 || !matches!(layout, Value::Null) {
        tr_invoke(vm, root, "removeAllViews", "()V", &[])?;
        if layout_id > 0 {
            let context = get(vm, scene, "mContext");
            let inflater = vm.invoke_static(
                "Landroid/view/LayoutInflater;",
                "from",
                "(Landroid/content/Context;)Landroid/view/LayoutInflater;",
                &[context],
            )?;
            let inflater_ref = obj(Some(&inflater)).ok_or_else(|| npe("LayoutInflater"))?;
            tr_invoke(
                vm,
                inflater_ref,
                "inflate",
                "(ILandroid/view/ViewGroup;)Landroid/view/View;",
                &[Value::Int(layout_id), Value::Obj(root)],
            )?;
        } else {
            tr_invoke(vm, root, "addView", "(Landroid/view/View;)V", &[layout])?;
        }
    }
    if let Some(action) = field_obj(vm, scene, "mEnterAction") {
        tr_invoke(vm, action, "run", "()V", &[])?;
    }
    tr_scene_set_current(vm, root, Value::Obj(scene))
}
fn tr_scene_exit(vm: &mut Engine, scene: ObjRef) -> Result<(), VmExit> {
    tr_scene_ensure(vm, scene)?;
    let root = field_obj(vm, scene, "mSceneRoot").ok_or_else(|| npe("sceneRoot"))?;
    if tr_scene_current(vm, root) == Value::Obj(scene) {
        if let Some(action) = field_obj(vm, scene, "mExitAction") {
            tr_invoke(vm, action, "run", "()V", &[])?;
        }
    }
    Ok(())
}

fn transition_scene_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "(Landroid/view/ViewGroup;)V") => {
            tr_scene_ensure(vm, recv)?;
            put(
                vm,
                recv,
                "mSceneRoot",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("<init>", "(Landroid/view/ViewGroup;Landroid/view/View;)V")
        | ("<init>", "(Landroid/view/ViewGroup;Landroid/view/ViewGroup;)V") => {
            tr_scene_ensure(vm, recv)?;
            put(
                vm,
                recv,
                "mSceneRoot",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "mLayout",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("<init>", "(Landroid/view/ViewGroup;ILandroid/content/Context;)V") => {
            tr_scene_ensure(vm, recv)?;
            put(
                vm,
                recv,
                "mContext",
                args.get(2).cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "mSceneRoot",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "mLayoutId", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("getSceneRoot", "()Landroid/view/ViewGroup;") => {
            tr_scene_ensure(vm, recv)?;
            get(vm, recv, "mSceneRoot")
        }
        ("enter", "()V") => {
            tr_scene_enter(vm, recv)?;
            Value::Null
        }
        ("exit", "()V") => {
            tr_scene_exit(vm, recv)?;
            Value::Null
        }
        ("setEnterAction", "(Ljava/lang/Runnable;)V") => {
            tr_scene_ensure(vm, recv)?;
            put(
                vm,
                recv,
                "mEnterAction",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setExitAction", "(Ljava/lang/Runnable;)V") => {
            tr_scene_ensure(vm, recv)?;
            put(
                vm,
                recv,
                "mExitAction",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("isCreatedFromLayoutResource", "()Z") => {
            tr_scene_ensure(vm, recv)?;
            bool_value(field_int(vm, recv, "mLayoutId") > 0)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
fn transition_scene_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        (
            "getSceneForLayout",
            "(Landroid/view/ViewGroup;ILandroid/content/Context;)Landroid/transition/Scene;",
        ) => {
            let root = req_obj(args, 0, "sceneRoot")?;
            let cache = match field_obj(vm, root, "rdSceneLayoutCache") {
                Some(cache) => cache,
                None => {
                    let cache = tr_new_map(vm)?;
                    put(vm, root, "rdSceneLayoutCache", Value::Obj(cache))?;
                    cache
                }
            };
            let key = tr_box_int(vm, iarg(args, 1))?;
            if let Some(existing) = tr_map_get(vm, &Value::Obj(cache), &key) {
                if !matches!(existing, Value::Null) {
                    return Ok(Some(existing));
                }
            }
            let scene = alloc_obj(vm, TRANSITION_SCENE, Vec::new())?;
            tr_invoke(
                vm,
                scene,
                "<init>",
                "(Landroid/view/ViewGroup;ILandroid/content/Context;)V",
                &[
                    Value::Obj(root),
                    Value::Int(iarg(args, 1)),
                    args.get(2).cloned().unwrap_or(Value::Null),
                ],
            )?;
            tr_map_put(vm, cache, key, Value::Obj(scene))?;
            Value::Obj(scene)
        }
        ("getCurrentScene", "(Landroid/view/ViewGroup;)Landroid/transition/Scene;") => {
            let root = req_obj(args, 0, "sceneRoot")?;
            tr_scene_current(vm, root)
        }
        ("setCurrentScene", "(Landroid/view/ViewGroup;Landroid/transition/Scene;)V") => {
            let root = req_obj(args, 0, "sceneRoot")?;
            tr_scene_set_current(vm, root, args.get(1).cloned().unwrap_or(Value::Null))?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn tr_manager_ensure(vm: &mut Engine, manager: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, manager, "rdManagerInit") {
        return Ok(());
    }
    put(vm, manager, "rdManagerInit", Value::Int(1))?;
    let scenes = tr_new_map(vm)?;
    put(vm, manager, "mSceneTransitions", Value::Obj(scenes))?;
    let pairs = tr_new_map(vm)?;
    put(vm, manager, "mScenePairTransitions", Value::Obj(pairs))
}
fn tr_manager_default_transition(vm: &mut Engine) -> Result<Value, VmExit> {
    let key = (
        TRANSITION_MANAGER.to_string(),
        "sDefaultTransition".to_string(),
    );
    if let Some(value) = vm.statics.get(&key).cloned() {
        return Ok(value);
    }
    let auto = alloc_obj(vm, TRANSITION_AUTO, Vec::new())?;
    tr_invoke(vm, auto, "<init>", "()V", &[])?;
    let value = Value::Obj(auto);
    vm.statics.insert(key, value.clone());
    Ok(value)
}
fn tr_manager_pending(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Value::Obj(list) = tr_static_get(vm, TRANSITION_MANAGER, "sPendingTransitions") {
        return Ok(list);
    }
    let list = tr_new_list(vm, Vec::new())?;
    tr_static_set(
        vm,
        TRANSITION_MANAGER,
        "sPendingTransitions",
        Value::Obj(list),
    );
    Ok(list)
}
fn tr_manager_running_transitions(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Value::Obj(map) = tr_static_get(vm, TRANSITION_MANAGER, "rdRunningTransitions") {
        return Ok(map);
    }
    let map = tr_new_map(vm)?;
    tr_static_set(
        vm,
        TRANSITION_MANAGER,
        "rdRunningTransitions",
        Value::Obj(map),
    );
    Ok(map)
}
fn tr_manager_pending_contains(vm: &mut Engine, root_value: &Value) -> Result<bool, VmExit> {
    let pending = tr_manager_pending(vm)?;
    tr_list_contains(vm, &Value::Obj(pending), root_value)
}
fn tr_manager_pending_add(vm: &mut Engine, root: ObjRef) -> Result<(), VmExit> {
    let pending = tr_manager_pending(vm)?;
    let items = tr_items(vm, &Value::Obj(pending));
    let mut items = items;
    items.push(Value::Obj(root));
    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&pending) {
        *elems = items;
    }
    Ok(())
}
fn tr_manager_pending_remove(vm: &mut Engine, root_value: &Value) -> Result<bool, VmExit> {
    let pending = tr_manager_pending(vm)?;
    let mut items = tr_items(vm, &Value::Obj(pending));
    let mut found = None;
    for (index, existing) in items.iter().enumerate() {
        if crate::intrinsics::values_equal(&vm.heap, existing, root_value)? {
            found = Some(index);
            break;
        }
    }
    if let Some(index) = found {
        items.remove(index);
        if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&pending) {
            *elems = items;
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
fn tr_manager_running_list(
    vm: &mut Engine,
    root: ObjRef,
    create: bool,
) -> Result<Option<Value>, VmExit> {
    let map = tr_manager_running_transitions(vm)?;
    if let Some(list) = tr_map_get(vm, &Value::Obj(map), &Value::Obj(root)) {
        return Ok(Some(list));
    }
    if create {
        let list = Value::Obj(tr_new_list(vm, Vec::new())?);
        tr_map_put(vm, map, Value::Obj(root), list.clone())?;
        Ok(Some(list))
    } else {
        Ok(None)
    }
}
fn tr_manager_setup(
    vm: &mut Engine,
    root: ObjRef,
    transition: Option<ObjRef>,
) -> Result<(), VmExit> {
    if let Some(list) = tr_manager_running_list(vm, root, false)? {
        for running in tr_items(vm, &list)
            .iter()
            .filter_map(|value| obj(Some(value)))
        {
            tr_invoke(
                vm,
                running,
                "pause",
                "(Landroid/view/View;)V",
                &[Value::Obj(root)],
            )?;
        }
    }
    if let Some(transition) = transition {
        tr_invoke(
            vm,
            transition,
            "captureValues",
            "(Landroid/view/ViewGroup;Z)V",
            &[Value::Obj(root), Value::Int(1)],
        )?;
    }
    if let Some(previous) = obj(Some(&tr_scene_current(vm, root))) {
        tr_invoke(vm, previous, "exit", "()V", &[])?;
    }
    Ok(())
}
fn tr_manager_schedule(vm: &mut Engine, root: ObjRef, transition: ObjRef) -> Result<(), VmExit> {
    let listener = alloc_obj(vm, TRANSITION_MANAGER_MULTI, Vec::new())?;
    tr_invoke(
        vm,
        listener,
        "<init>",
        "(Landroid/transition/Transition;Landroid/view/ViewGroup;)V",
        &[Value::Obj(transition), Value::Obj(root)],
    )?;
    let _ = tr_invoke(
        vm,
        root,
        "addOnAttachStateChangeListener",
        "(Landroid/view/View$OnAttachStateChangeListener;)V",
        &[Value::Obj(listener)],
    );
    if let Ok(observer) = tr_invoke(
        vm,
        root,
        "getViewTreeObserver",
        "()Landroid/view/ViewTreeObserver;",
        &[],
    ) {
        if let Some(observer) = obj(Some(&observer)) {
            let _ = tr_invoke(
                vm,
                observer,
                "addOnPreDrawListener",
                "(Landroid/view/ViewTreeObserver$OnPreDrawListener;)V",
                &[Value::Obj(listener)],
            );
        }
    }
    let now = vm.fw.clock();
    vm.frames.callbacks.push((listener, now));
    Ok(())
}
fn tr_manager_change_scene(
    vm: &mut Engine,
    scene_value: Value,
    transition_value: Value,
) -> Result<(), VmExit> {
    let scene = obj(Some(&scene_value)).ok_or_else(|| npe("scene"))?;
    let root_value = tr_invoke(vm, scene, "getSceneRoot", "()Landroid/view/ViewGroup;", &[])?;
    let root = obj(Some(&root_value)).ok_or_else(|| npe("sceneRoot"))?;
    if tr_manager_pending_contains(vm, &Value::Obj(root))? {
        return Ok(());
    }
    let old_scene = tr_scene_current(vm, root);
    if matches!(transition_value, Value::Null) {
        if let Some(old) = obj(Some(&old_scene)) {
            tr_invoke(vm, old, "exit", "()V", &[])?;
        }
        tr_invoke(vm, scene, "enter", "()V", &[])?;
        return Ok(());
    }
    tr_manager_pending_add(vm, root)?;
    let transition = obj(Some(&transition_value)).ok_or_else(|| npe("transition"))?;
    let clone_value = tr_invoke(
        vm,
        transition,
        "clone",
        "()Landroid/transition/Transition;",
        &[],
    )?;
    let clone = obj(Some(&clone_value)).ok_or_else(|| npe("transition clone"))?;
    tr_invoke(
        vm,
        clone,
        "setSceneRoot",
        "(Landroid/view/ViewGroup;)Landroid/transition/Transition;",
        &[Value::Obj(root)],
    )?;
    if let Some(old) = obj(Some(&old_scene)) {
        let from_layout = tr_invoke(vm, old, "isCreatedFromLayoutResource", "()Z", &[])?;
        if from_layout == Value::Int(1) {
            tr_invoke(vm, clone, "setCanRemoveViews", "(Z)V", &[Value::Int(1)])?;
        }
    }
    tr_manager_setup(vm, root, Some(clone))?;
    tr_invoke(vm, scene, "enter", "()V", &[])?;
    tr_manager_schedule(vm, root, clone)
}
fn tr_manager_begin_delayed(
    vm: &mut Engine,
    root_value: Value,
    transition_value: Value,
) -> Result<(), VmExit> {
    let root = obj(Some(&root_value)).ok_or_else(|| npe("sceneRoot"))?;
    if tr_manager_pending_contains(vm, &Value::Obj(root))? {
        return Ok(());
    }
    if tr_invoke(vm, root, "isLaidOut", "()Z", &[])? != Value::Int(1) {
        return Ok(());
    }
    tr_manager_pending_add(vm, root)?;
    let transition_value = if matches!(transition_value, Value::Null) {
        tr_manager_default_transition(vm)?
    } else {
        transition_value
    };
    let transition = obj(Some(&transition_value)).ok_or_else(|| npe("transition"))?;
    let clone_value = tr_invoke(
        vm,
        transition,
        "clone",
        "()Landroid/transition/Transition;",
        &[],
    )?;
    let clone = obj(Some(&clone_value)).ok_or_else(|| npe("transition clone"))?;
    tr_manager_setup(vm, root, Some(clone))?;
    tr_scene_set_current(vm, root, Value::Null)?;
    tr_manager_schedule(vm, root, clone)
}
fn tr_manager_on_pre_draw(vm: &mut Engine, listener: ObjRef) -> Result<Value, VmExit> {
    let root = field_obj(vm, listener, "mSceneRoot").ok_or_else(|| npe("sceneRoot"))?;
    let transition = field_obj(vm, listener, "mTransition").ok_or_else(|| npe("transition"))?;
    if !tr_manager_pending_remove(vm, &Value::Obj(root))? {
        return Ok(Value::Int(1));
    }
    let current_list = tr_manager_running_list(vm, root, true)?.ok_or_else(|| npe("running"))?;
    let previous_items = tr_items(vm, &current_list);
    let previous = if previous_items.is_empty() {
        Vec::new()
    } else {
        previous_items
    };
    if let Some(list_ref) = obj(Some(&current_list)) {
        if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list_ref) {
            elems.push(Value::Obj(transition));
        }
    }
    let end_listener = alloc_obj(vm, TRANSITION_MANAGER_END_LISTENER, Vec::new())?;
    put(vm, end_listener, "val$sceneRoot", Value::Obj(root))?;
    let running_map = tr_manager_running_transitions(vm)?;
    put(
        vm,
        end_listener,
        "val$runningTransitions",
        Value::Obj(running_map),
    )?;
    tr_invoke(
        vm,
        transition,
        "addListener",
        "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        &[Value::Obj(end_listener)],
    )?;
    tr_invoke(
        vm,
        transition,
        "captureValues",
        "(Landroid/view/ViewGroup;Z)V",
        &[Value::Obj(root), Value::Int(0)],
    )?;
    for running in previous.iter().filter_map(|value| obj(Some(value))) {
        tr_invoke(
            vm,
            running,
            "resume",
            "(Landroid/view/View;)V",
            &[Value::Obj(root)],
        )?;
    }
    tr_invoke(
        vm,
        transition,
        "playTransition",
        "(Landroid/view/ViewGroup;)V",
        &[Value::Obj(root)],
    )?;
    Ok(Value::Int(1))
}
fn tr_manager_on_detached(vm: &mut Engine, listener: ObjRef) -> Result<(), VmExit> {
    let root = field_obj(vm, listener, "mSceneRoot").ok_or_else(|| npe("sceneRoot"))?;
    tr_manager_pending_remove(vm, &Value::Obj(root))?;
    if let Some(list) = tr_manager_running_list(vm, root, false)? {
        for running in tr_items(vm, &list)
            .iter()
            .filter_map(|value| obj(Some(value)))
        {
            tr_invoke(
                vm,
                running,
                "resume",
                "(Landroid/view/View;)V",
                &[Value::Obj(root)],
            )?;
        }
    }
    let transition = field_obj(vm, listener, "mTransition").ok_or_else(|| npe("transition"))?;
    tr_invoke(vm, transition, "clearValues", "(Z)V", &[Value::Int(1)])?;
    Ok(())
}
fn tr_manager_get_transition(
    vm: &mut Engine,
    manager: ObjRef,
    scene: ObjRef,
) -> Result<Value, VmExit> {
    tr_manager_ensure(vm, manager)?;
    let root_value = tr_invoke(vm, scene, "getSceneRoot", "()Landroid/view/ViewGroup;", &[])?;
    if let Some(root) = obj(Some(&root_value)) {
        let current = tr_scene_current(vm, root);
        if !matches!(current, Value::Null) {
            let pair_map_value = get(vm, manager, "mScenePairTransitions");
            if let Some(scene_map) = tr_map_get(vm, &pair_map_value, &Value::Obj(scene)) {
                if let Some(transition) = tr_map_get(vm, &scene_map, &current) {
                    if !matches!(transition, Value::Null) {
                        return Ok(transition);
                    }
                }
            }
        }
    }
    let scenes = get(vm, manager, "mSceneTransitions");
    let transition = tr_map_get(vm, &scenes, &Value::Obj(scene)).unwrap_or(Value::Null);
    if matches!(transition, Value::Null) {
        tr_manager_default_transition(vm)
    } else {
        Ok(transition)
    }
}

fn transition_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("<init>", "()V") => {
            tr_manager_ensure(vm, recv)?;
            Value::Null
        }
        ("setDefaultTransition", "(Landroid/transition/Transition;)V") => {
            tr_static_set(
                vm,
                TRANSITION_MANAGER,
                "sDefaultTransition",
                args.first().cloned().unwrap_or(Value::Null),
            );
            Value::Null
        }
        (
            "setTransition",
            "(Landroid/transition/Scene;Landroid/transition/Transition;)V",
        ) => {
            tr_manager_ensure(vm, recv)?;
            let map = field_obj(vm, recv, "mSceneTransitions").ok_or_else(|| npe("map"))?;
            tr_map_put(
                vm,
                map,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        (
            "setTransition",
            "(Landroid/transition/Scene;Landroid/transition/Scene;Landroid/transition/Transition;)V",
        ) => {
            tr_manager_ensure(vm, recv)?;
            let pair_map = field_obj(vm, recv, "mScenePairTransitions")
                .ok_or_else(|| npe("map"))?;
            let to_scene = args.get(1).cloned().unwrap_or(Value::Null);
            let inner = match tr_map_get(vm, &Value::Obj(pair_map), &to_scene)
                .and_then(|value| obj(Some(&value)))
            {
                Some(inner) => inner,
                None => {
                    let inner = tr_new_map(vm)?;
                    tr_map_put(vm, pair_map, to_scene, Value::Obj(inner))?;
                    inner
                }
            };
            tr_map_put(
                vm,
                inner,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(2).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        (
            "getTransition",
            "(Landroid/transition/Scene;)Landroid/transition/Transition;",
        ) => {
            let scene = req_obj(args, 0, "scene")?;
            tr_manager_get_transition(vm, recv, scene)?
        }
        ("transitionTo", "(Landroid/transition/Scene;)V") => {
            let scene = req_obj(args, 0, "scene")?;
            let transition = tr_manager_get_transition(vm, recv, scene)?;
            tr_manager_change_scene(vm, Value::Obj(scene), transition)?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
fn transition_manager_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("getDefaultTransition", "()Landroid/transition/Transition;") => {
            tr_manager_default_transition(vm)?
        }
        ("go", "(Landroid/transition/Scene;)V") => {
            let transition = tr_manager_default_transition(vm)?;
            tr_manager_change_scene(vm, args.first().cloned().unwrap_or(Value::Null), transition)?;
            Value::Null
        }
        ("go", "(Landroid/transition/Scene;Landroid/transition/Transition;)V") => {
            tr_manager_change_scene(
                vm,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("beginDelayedTransition", "(Landroid/view/ViewGroup;)V") => {
            tr_manager_begin_delayed(
                vm,
                args.first().cloned().unwrap_or(Value::Null),
                Value::Null,
            )?;
            Value::Null
        }
        (
            "beginDelayedTransition",
            "(Landroid/view/ViewGroup;Landroid/transition/Transition;)V",
        ) => {
            tr_manager_begin_delayed(
                vm,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("endTransitions", "(Landroid/view/ViewGroup;)V") => {
            let root = obj(args.first());
            if let Some(root_ref) = root {
                tr_manager_pending_remove(vm, &Value::Obj(root_ref))?;
                if let Some(list) = tr_manager_running_list(vm, root_ref, false)? {
                    let transitions = tr_items(vm, &list)
                        .iter()
                        .filter_map(|value| obj(Some(value)))
                        .collect::<Vec<_>>();
                    for transition in transitions.into_iter().rev() {
                        tr_invoke(
                            vm,
                            transition,
                            "forceToEnd",
                            "(Landroid/view/ViewGroup;)V",
                            &[Value::Obj(root_ref)],
                        )?;
                    }
                }
            } else {
                tr_manager_pending_remove(vm, &Value::Null)?;
            }
            Value::Null
        }
        ("changeScene", "(Landroid/transition/Scene;Landroid/transition/Transition;)V") => {
            tr_manager_change_scene(
                vm,
                args.first().cloned().unwrap_or(Value::Null),
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("sceneChangeSetup", "(Landroid/view/ViewGroup;Landroid/transition/Transition;)V") => {
            let root = req_obj(args, 0, "sceneRoot")?;
            tr_manager_setup(vm, root, obj(args.get(1)))?;
            Value::Null
        }
        (
            "sceneChangeRunTransition",
            "(Landroid/view/ViewGroup;Landroid/transition/Transition;)V",
        ) => {
            if let (Some(root), Some(transition)) = (obj(args.first()), obj(args.get(1))) {
                tr_manager_schedule(vm, root, transition)?;
            }
            Value::Null
        }
        ("getRunningTransitions", "()Landroid/util/ArrayMap;") => {
            Value::Obj(tr_manager_running_transitions(vm)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn tr_string_array(vm: &mut Engine, values: &[&str]) -> Result<Value, VmExit> {
    let mut items = Vec::new();
    for value in values {
        items.push(jstr(vm, *value)?);
    }
    obj_array(vm, "[Ljava/lang/String;", items)
}
fn tr_auto_init(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, recv, "rdAutoInit") {
        return Ok(());
    }
    put(vm, recv, "rdAutoInit", Value::Int(1))?;
    put(vm, recv, "mPlayTogether", Value::Int(0))?;
    for (class, mode) in [
        (TRANSITION_FADE, Some(2)),
        (TRANSITION_CHANGE_BOUNDS, None),
        (TRANSITION_FADE, Some(1)),
    ] {
        let child = alloc_obj(vm, class, Vec::new())?;
        if let Some(mode) = mode {
            tr_invoke(vm, child, "<init>", "(I)V", &[Value::Int(mode)])?;
        } else {
            tr_invoke(vm, child, "<init>", "()V", &[])?;
        }
        tr_invoke(
            vm,
            recv,
            "addTransition",
            "(Landroid/transition/Transition;)Landroid/transition/TransitionSet;",
            &[Value::Obj(child)],
        )?;
    }
    Ok(())
}

fn transition_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    tr_ensure_transition(vm, recv)?;
    let class = class_of(vm, recv);
    let is_set = vm.cp.is_subtype(&class, TRANSITION_SET);
    let is_visibility = vm.cp.is_subtype(&class, TRANSITION_VISIBILITY);
    let is_fade = vm.cp.is_subtype(&class, TRANSITION_FADE);
    let is_change_bounds = vm.cp.is_subtype(&class, TRANSITION_CHANGE_BOUNDS);
    if is_set
        && matches!(
            name,
            "addTarget" | "removeTarget" | "excludeTarget" | "excludeChildren"
        )
    {
        let base_sig = sig.replace(
            "Landroid/transition/TransitionSet;",
            "Landroid/transition/Transition;",
        );
        for child in tr_set_children(vm, recv) {
            tr_invoke(vm, child, name, &base_sig, args)?;
        }
    }
    let self_value = Value::Obj(recv);
    let value = match (name, sig) {
        ("<init>", "()V") | ("<init>", "(Landroid/content/Context;Landroid/util/AttributeSet;)V") => {
            if class == TRANSITION_AUTO || vm.cp.is_subtype(&class, TRANSITION_AUTO) {
                tr_auto_init(vm, recv)?;
            }
            Value::Null
        }
        ("<init>", "(I)V") if is_fade => {
            let mode = iarg(args, 0);
            if mode & !3 != 0 {
                return Err(iae("Only MODE_IN and MODE_OUT flags are allowed"));
            }
            put(vm, recv, "mMode", Value::Int(mode))?;
            Value::Null
        }
        ("setDuration", "(J)Landroid/transition/Transition;")
        | ("setDuration", "(J)Landroid/transition/TransitionSet;") => {
            put(vm, recv, "mDuration", Value::Long(larg(args, 0)))?;
            if is_set && larg(args, 0) >= 0 {
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setDuration",
                        "(J)Landroid/transition/Transition;",
                        &[Value::Long(larg(args, 0))],
                    )?;
                }
            }
            self_value.clone()
        }
        ("getDuration", "()J") => Value::Long(field_long(vm, recv, "mDuration")),
        ("setStartDelay", "(J)Landroid/transition/Transition;")
        | ("setStartDelay", "(J)Landroid/transition/TransitionSet;") => {
            put(vm, recv, "mStartDelay", Value::Long(larg(args, 0)))?;
            self_value.clone()
        }
        ("getStartDelay", "()J") => Value::Long(field_long(vm, recv, "mStartDelay")),
        (
            "setInterpolator",
            "(Landroid/animation/TimeInterpolator;)Landroid/transition/Transition;",
        )
        | (
            "setInterpolator",
            "(Landroid/animation/TimeInterpolator;)Landroid/transition/TransitionSet;",
        ) => {
            let interpolator = args.first().cloned().unwrap_or(Value::Null);
            if is_set {
                let flags = field_int(vm, recv, "mChangeFlags") | 1;
                put(vm, recv, "mChangeFlags", Value::Int(flags))?;
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setInterpolator",
                        "(Landroid/animation/TimeInterpolator;)Landroid/transition/Transition;",
                        std::slice::from_ref(&interpolator),
                    )?;
                }
            }
            put(vm, recv, "mInterpolator", interpolator)?;
            self_value.clone()
        }
        ("getInterpolator", "()Landroid/animation/TimeInterpolator;") => {
            get(vm, recv, "mInterpolator")
        }
        ("getTransitionProperties", "()[Ljava/lang/String;") => {
            if is_change_bounds {
                tr_string_array(
                    vm,
                    &[
                        TR_PROP_BOUNDS,
                        TR_PROP_CLIP,
                        TR_PROP_BOUNDS_PARENT,
                        TR_PROP_WINDOW_X,
                        TR_PROP_WINDOW_Y,
                    ],
                )?
            } else if is_visibility {
                tr_string_array(vm, &[TR_PROP_VISIBILITY, TR_PROP_VISIBILITY_PARENT])?
            } else {
                Value::Null
            }
        }
        (
            "createAnimator",
            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        ) => {
            let scene_root = req_obj(args, 0, "sceneRoot")?;
            let start = obj(args.get(1));
            let end = obj(args.get(2));
            if is_change_bounds {
                tr_change_bounds_create_animator(vm, recv, start, end)?
            } else if is_visibility {
                let info = tr_visibility_info(vm, start, end)?;
                if info.changed && (info.start_parent.is_some() || info.end_parent.is_some()) {
                    if info.fade_in {
                        tr_invoke(
                            vm,
                            recv,
                            "onAppear",
                            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;ILandroid/transition/TransitionValues;I)Landroid/animation/Animator;",
                            &[
                                Value::Obj(scene_root),
                                start.map_or(Value::Null, Value::Obj),
                                Value::Int(info.start_visibility),
                                end.map_or(Value::Null, Value::Obj),
                                Value::Int(info.end_visibility),
                            ],
                        )?
                    } else {
                        tr_invoke(
                            vm,
                            recv,
                            "onDisappear",
                            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;ILandroid/transition/TransitionValues;I)Landroid/animation/Animator;",
                            &[
                                Value::Obj(scene_root),
                                start.map_or(Value::Null, Value::Obj),
                                Value::Int(info.start_visibility),
                                end.map_or(Value::Null, Value::Obj),
                                Value::Int(info.end_visibility),
                            ],
                        )?
                    }
                } else {
                    Value::Null
                }
            } else {
                Value::Null
            }
        }
        ("setMatchOrder", "([I)V") => {
            let matches = int_values(vm, args.first());
            if matches!(args.first(), None | Some(Value::Null)) || matches.is_empty() {
                let order = new_int_array(vm, &[2, 1, 3, 4])?;
                put(vm, recv, "mMatchOrder", order)?;
            } else {
                for (index, value) in matches.iter().enumerate() {
                    if !(1..=4).contains(value) {
                        return Err(iae("matches contains invalid value"));
                    }
                    if matches[..index].contains(value) {
                        return Err(iae("matches contains a duplicate value"));
                    }
                }
                let order = new_int_array(vm, &matches)?;
                put(vm, recv, "mMatchOrder", order)?;
            }
            Value::Null
        }
        ("isValidTarget", "(Landroid/view/View;)Z") => {
            bool_value(tr_is_valid_target(vm, recv, obj(args.first()))?)
        }
        ("captureStartValues", "(Landroid/transition/TransitionValues;)V")
        | ("captureEndValues", "(Landroid/transition/TransitionValues;)V") => {
            let start = name == "captureStartValues";
            let values = req_obj(args, 0, "transitionValues")?;
            tr_ensure_values(vm, values)?;
            if is_set {
                let view = tr_values_view(vm, values);
                if tr_is_valid_target(vm, recv, view)? {
                    for child in tr_set_children(vm, recv) {
                        if tr_is_valid_target(vm, child, view)? {
                            tr_invoke(vm, child, name, sig, &[Value::Obj(values)])?;
                            tr_append_field_list(
                                vm,
                                values,
                                "targetedTransitions",
                                Value::Obj(child),
                            )?;
                        }
                    }
                }
            } else if is_change_bounds {
                tr_capture_change_bounds_values(vm, recv, values)?;
            } else if is_visibility {
                tr_capture_visibility_values(vm, values)?;
                if is_fade && start {
                    let view = tr_values_view(vm, values).ok_or_else(|| npe("view"))?;
                    let alpha = match tr_invoke(vm, view, "getTransitionAlpha", "()F", &[])? {
                        Value::Float(v) => v,
                        _ => 1.0,
                    };
                    let boxed = tr_box_float(vm, alpha)?;
                    tr_values_put(vm, values, TR_PROP_FADE_ALPHA, boxed)?;
                }
            }
            Value::Null
        }
        ("captureValues", "(Landroid/view/ViewGroup;Z)V") => {
            let root = req_obj(args, 0, "sceneRoot")?;
            tr_capture_values(vm, recv, root, iarg(args, 1) != 0)?;
            Value::Null
        }
        ("clearValues", "(Z)V") => {
            tr_clear_values(vm, recv, iarg(args, 0) != 0)?;
            Value::Null
        }
        (
            "createAnimators",
            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValuesMaps;Landroid/transition/TransitionValuesMaps;Ljava/util/ArrayList;Ljava/util/ArrayList;)V",
        ) => {
            tr_create_animators(
                vm,
                recv,
                req_obj(args, 0, "sceneRoot")?,
                req_obj(args, 1, "startValues")?,
                req_obj(args, 2, "endValues")?,
                args.get(3).cloned().unwrap_or(Value::Null),
                args.get(4).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("runAnimators", "()V") => {
            tr_run_animators(vm, recv)?;
            Value::Null
        }
        ("playTransition", "(Landroid/view/ViewGroup;)V") => {
            tr_play_transition(vm, recv, req_obj(args, 0, "sceneRoot")?)?;
            Value::Null
        }
        (
            "getTransitionValues",
            "(Landroid/view/View;Z)Landroid/transition/TransitionValues;",
        ) => tr_get_transition_values(vm, recv, obj(args.first()), iarg(args, 1) != 0)?,
        (
            "getMatchedTransitionValues",
            "(Landroid/view/View;Z)Landroid/transition/TransitionValues;",
        ) => {
            tr_get_matched_transition_values(vm, recv, obj(args.first()), iarg(args, 1) != 0)?
        }
        (
            "isTransitionRequired",
            "(Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Z",
        ) => bool_value(tr_is_transition_required(
            vm,
            recv,
            obj(args.first()),
            obj(args.get(1)),
        )?),
        ("animate", "(Landroid/animation/Animator;)V") => {
            tr_animate(vm, recv, obj(args.first()))?;
            Value::Null
        }
        ("start", "()V") => {
            tr_start(vm, recv)?;
            Value::Null
        }
        ("end", "()V") => {
            tr_end(vm, recv)?;
            Value::Null
        }
        ("cancel", "()V") => {
            tr_cancel(vm, recv)?;
            Value::Null
        }
        ("pause", "(Landroid/view/View;)V") => {
            tr_pause(vm, recv, obj(args.first()))?;
            Value::Null
        }
        ("resume", "(Landroid/view/View;)V") => {
            tr_resume(vm, recv, obj(args.first()))?;
            Value::Null
        }
        ("forceToEnd", "(Landroid/view/ViewGroup;)V") => {
            tr_force_to_end(vm, recv, obj(args.first()))?;
            Value::Null
        }
        (
            "addListener",
            "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        )
        | (
            "addListener",
            "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/TransitionSet;",
        ) => {
            tr_ensure_field_list(vm, recv, "mListeners")?;
            tr_append_field_list(
                vm,
                recv,
                "mListeners",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            self_value.clone()
        }
        (
            "removeListener",
            "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        )
        | (
            "removeListener",
            "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/TransitionSet;",
        ) => {
            if field_obj(vm, recv, "mListeners").is_some() {
                tr_remove_field_list(
                    vm,
                    recv,
                    "mListeners",
                    &args.first().cloned().unwrap_or(Value::Null),
                )?;
                if tr_field_items(vm, recv, "mListeners").is_empty() {
                    put(vm, recv, "mListeners", Value::Null)?;
                }
            }
            self_value.clone()
        }
        (
            "setEpicenterCallback",
            "(Landroid/transition/Transition$EpicenterCallback;)V",
        ) => {
            let callback = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mEpicenterCallback", callback.clone())?;
            if is_set {
                let flags = field_int(vm, recv, "mChangeFlags") | 8;
                put(vm, recv, "mChangeFlags", Value::Int(flags))?;
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setEpicenterCallback",
                        "(Landroid/transition/Transition$EpicenterCallback;)V",
                        std::slice::from_ref(&callback),
                    )?;
                }
            }
            Value::Null
        }
        (
            "getEpicenterCallback",
            "()Landroid/transition/Transition$EpicenterCallback;",
        ) => get(vm, recv, "mEpicenterCallback"),
        ("getEpicenter", "()Landroid/graphics/Rect;") => {
            if let Some(callback) = field_obj(vm, recv, "mEpicenterCallback") {
                tr_invoke(
                    vm,
                    callback,
                    "onGetEpicenter",
                    "(Landroid/transition/Transition;)Landroid/graphics/Rect;",
                    std::slice::from_ref(&self_value),
                )?
            } else {
                Value::Null
            }
        }
        ("setPathMotion", "(Landroid/transition/PathMotion;)V") => {
            let original = args.first().cloned().unwrap_or(Value::Null);
            let stored = if matches!(original, Value::Null) {
                Value::Obj(alloc_obj(vm, TRANSITION_STRAIGHT_PATH, Vec::new())?)
            } else {
                original.clone()
            };
            put(vm, recv, "mPathMotion", stored)?;
            if is_set {
                let flags = field_int(vm, recv, "mChangeFlags") | 4;
                put(vm, recv, "mChangeFlags", Value::Int(flags))?;
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setPathMotion",
                        "(Landroid/transition/PathMotion;)V",
                        std::slice::from_ref(&original),
                    )?;
                }
            }
            Value::Null
        }
        ("getPathMotion", "()Landroid/transition/PathMotion;") => get(vm, recv, "mPathMotion"),
        (
            "setPropagation",
            "(Landroid/transition/TransitionPropagation;)V",
        ) => {
            let propagation = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mPropagation", propagation.clone())?;
            if is_set {
                let flags = field_int(vm, recv, "mChangeFlags") | 2;
                put(vm, recv, "mChangeFlags", Value::Int(flags))?;
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setPropagation",
                        "(Landroid/transition/TransitionPropagation;)V",
                        std::slice::from_ref(&propagation),
                    )?;
                }
            }
            Value::Null
        }
        (
            "getPropagation",
            "()Landroid/transition/TransitionPropagation;",
        ) => get(vm, recv, "mPropagation"),
        ("capturePropagationValues", "(Landroid/transition/TransitionValues;)V") => {
            let values = req_obj(args, 0, "transitionValues")?;
            tr_capture_propagation_values(vm, recv, values)?;
            Value::Null
        }
        (
            "setSceneRoot",
            "(Landroid/view/ViewGroup;)Landroid/transition/Transition;",
        )
        | (
            "setSceneRoot",
            "(Landroid/view/ViewGroup;)Landroid/transition/TransitionSet;",
        ) => {
            let root = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mSceneRoot", root.clone())?;
            if is_set {
                for child in tr_set_children(vm, recv) {
                    tr_invoke(
                        vm,
                        child,
                        "setSceneRoot",
                        "(Landroid/view/ViewGroup;)Landroid/transition/Transition;",
                        std::slice::from_ref(&root),
                    )?;
                }
            }
            self_value.clone()
        }
        ("setCanRemoveViews", "(Z)V") => {
            put(vm, recv, "mCanRemoveViews", Value::Int(i32::from(iarg(args, 0) != 0)))?;
            if is_set {
                for child in tr_set_children(vm, recv) {
                    tr_invoke(vm, child, "setCanRemoveViews", "(Z)V", &[Value::Int(iarg(args, 0))])?;
                }
            }
            Value::Null
        }
        ("canRemoveViews", "()Z") => bool_value(field_bool(vm, recv, "mCanRemoveViews")),
        ("setNameOverrides", "(Landroid/util/ArrayMap;)V") => {
            put(
                vm,
                recv,
                "mNameOverrides",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getNameOverrides", "()Landroid/util/ArrayMap;") => get(vm, recv, "mNameOverrides"),
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("clone", "()Landroid/transition/Transition;")
        | ("clone", "()Landroid/transition/TransitionSet;")
        | ("clone", "()Ljava/lang/Object;") => Value::Obj(tr_clone_transition(vm, recv)?),
        ("toString", "()Ljava/lang/String;") => {
            let mut out = format!(
                "{}@{:x}: ",
                tr_simple_name(&class),
                identity_hash(recv) as u32
            );
            let duration = field_long(vm, recv, "mDuration");
            if duration != -1 {
                out.push_str(&format!("dur({duration}) "));
            }
            let delay = field_long(vm, recv, "mStartDelay");
            if delay != -1 {
                out.push_str(&format!("dly({delay}) "));
            }
            let interpolator = get(vm, recv, "mInterpolator");
            if !matches!(interpolator, Value::Null) {
                out.push_str(&format!("interp({}) ", value_string(vm, &interpolator)?));
            }
            let ids = tr_field_items(vm, recv, "mTargetIds");
            let targets = tr_field_items(vm, recv, "mTargets");
            if !ids.is_empty() || !targets.is_empty() {
                out.push_str("tgts(");
                let mut rendered = Vec::new();
                for id in &ids {
                    rendered.push(tr_unbox_int(vm, id).unwrap_or(0).to_string());
                }
                for target in &targets {
                    rendered.push(value_string(vm, target)?);
                }
                out.push_str(&rendered.join(", "));
                out.push(')');
            }
            if is_set {
                for child in tr_set_children(vm, recv) {
                    let child_text = tr_invoke(vm, child, "toString", "()Ljava/lang/String;", &[])?;
                    out.push_str(&format!("\n  {}", value_string(vm, &child_text)?));
                }
            }
            jstr(vm, out)?
        }
        _ => return tr_transition_specific_instance(vm, &class, name, sig, recv, args),
    };
    Ok(Some(value))
}

#[allow(clippy::too_many_lines)]
fn tr_transition_specific_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_set = vm.cp.is_subtype(class, TRANSITION_SET);
    let is_visibility = vm.cp.is_subtype(class, TRANSITION_VISIBILITY);
    let is_fade = vm.cp.is_subtype(class, TRANSITION_FADE);
    let self_value = Value::Obj(recv);
    let value = match (name, sig) {
        ("addTarget", "(I)Landroid/transition/Transition;")
        | ("addTarget", "(I)Landroid/transition/TransitionSet;") => {
            if iarg(args, 0) > 0 {
                let boxed = tr_box_int(vm, iarg(args, 0))?;
                tr_append_field_list(vm, recv, "mTargetIds", boxed)?;
            }
            self_value
        }
        ("addTarget", "(Ljava/lang/String;)Landroid/transition/Transition;")
        | ("addTarget", "(Ljava/lang/String;)Landroid/transition/TransitionSet;") => {
            if let Some(target) = obj(args.first()) {
                tr_ensure_field_list(vm, recv, "mTargetNames")?;
                tr_append_field_list(vm, recv, "mTargetNames", Value::Obj(target))?;
            }
            self_value
        }
        ("addTarget", "(Ljava/lang/Class;)Landroid/transition/Transition;")
        | ("addTarget", "(Ljava/lang/Class;)Landroid/transition/TransitionSet;") => {
            if let Some(target) = obj(args.first()) {
                tr_ensure_field_list(vm, recv, "mTargetTypes")?;
                tr_append_field_list(vm, recv, "mTargetTypes", Value::Obj(target))?;
            }
            self_value
        }
        ("addTarget", "(Landroid/view/View;)Landroid/transition/Transition;")
        | ("addTarget", "(Landroid/view/View;)Landroid/transition/TransitionSet;") => {
            tr_append_field_list(
                vm,
                recv,
                "mTargets",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            self_value
        }
        ("removeTarget", "(I)Landroid/transition/Transition;")
        | ("removeTarget", "(I)Landroid/transition/TransitionSet;") => {
            if iarg(args, 0) > 0 {
                let boxed = tr_box_int(vm, iarg(args, 0))?;
                tr_remove_field_list(vm, recv, "mTargetIds", &boxed)?;
            }
            self_value
        }
        ("removeTarget", "(Ljava/lang/String;)Landroid/transition/Transition;")
        | ("removeTarget", "(Ljava/lang/String;)Landroid/transition/TransitionSet;") => {
            if let Some(target) = obj(args.first()) {
                if field_obj(vm, recv, "mTargetNames").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetNames", &Value::Obj(target))?;
                }
            }
            self_value
        }
        ("removeTarget", "(Ljava/lang/Class;)Landroid/transition/Transition;")
        | ("removeTarget", "(Ljava/lang/Class;)Landroid/transition/TransitionSet;") => {
            if let Some(target) = obj(args.first()) {
                if field_obj(vm, recv, "mTargetTypes").is_none() {
                    return Err(npe("target types"));
                }
                tr_remove_field_list(vm, recv, "mTargetTypes", &Value::Obj(target))?;
            }
            self_value
        }
        ("removeTarget", "(Landroid/view/View;)Landroid/transition/Transition;")
        | ("removeTarget", "(Landroid/view/View;)Landroid/transition/TransitionSet;") => {
            if let Some(target) = obj(args.first()) {
                tr_remove_field_list(vm, recv, "mTargets", &Value::Obj(target))?;
            }
            self_value
        }
        ("getTargets", "()Ljava/util/List;") => get(vm, recv, "mTargets"),
        ("getTargetIds", "()Ljava/util/List;") => get(vm, recv, "mTargetIds"),
        ("getTargetNames", "()Ljava/util/List;") => get(vm, recv, "mTargetNames"),
        ("getTargetTypes", "()Ljava/util/List;") => get(vm, recv, "mTargetTypes"),
        ("excludeTarget", "(IZ)Landroid/transition/Transition;") => {
            if iarg(args, 0) >= 0 {
                let boxed = tr_box_int(vm, iarg(args, 0))?;
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetIdExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetIdExcludes", boxed)?;
                } else if field_obj(vm, recv, "mTargetIdExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetIdExcludes", &boxed)?;
                }
            }
            self_value
        }
        ("excludeTarget", "(Ljava/lang/String;Z)Landroid/transition/Transition;") => {
            if let Some(target) = obj(args.first()) {
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetNameExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetNameExcludes", Value::Obj(target))?;
                } else if field_obj(vm, recv, "mTargetNameExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetNameExcludes", &Value::Obj(target))?;
                }
            }
            self_value
        }
        ("excludeTarget", "(Ljava/lang/Class;Z)Landroid/transition/Transition;") => {
            if let Some(target) = obj(args.first()) {
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetTypeExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetTypeExcludes", Value::Obj(target))?;
                } else if field_obj(vm, recv, "mTargetTypeExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetTypeExcludes", &Value::Obj(target))?;
                }
            }
            self_value
        }
        ("excludeTarget", "(Landroid/view/View;Z)Landroid/transition/Transition;") => {
            if let Some(target) = obj(args.first()) {
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetExcludes", Value::Obj(target))?;
                } else if field_obj(vm, recv, "mTargetExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetExcludes", &Value::Obj(target))?;
                }
            }
            self_value
        }
        ("excludeChildren", "(IZ)Landroid/transition/Transition;") => {
            if iarg(args, 0) >= 0 {
                let boxed = tr_box_int(vm, iarg(args, 0))?;
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetIdChildExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetIdChildExcludes", boxed)?;
                } else if field_obj(vm, recv, "mTargetIdChildExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetIdChildExcludes", &boxed)?;
                }
            }
            self_value
        }
        ("excludeChildren", "(Ljava/lang/Class;Z)Landroid/transition/Transition;") => {
            if let Some(target) = obj(args.first()) {
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetTypeChildExcludes")?;
                    tr_append_field_list(
                        vm,
                        recv,
                        "mTargetTypeChildExcludes",
                        Value::Obj(target),
                    )?;
                } else if field_obj(vm, recv, "mTargetTypeChildExcludes").is_some() {
                    tr_remove_field_list(
                        vm,
                        recv,
                        "mTargetTypeChildExcludes",
                        &Value::Obj(target),
                    )?;
                }
            }
            self_value
        }
        ("excludeChildren", "(Landroid/view/View;Z)Landroid/transition/Transition;") => {
            if let Some(target) = obj(args.first()) {
                if iarg(args, 1) != 0 {
                    tr_ensure_field_list(vm, recv, "mTargetChildExcludes")?;
                    tr_append_field_list(vm, recv, "mTargetChildExcludes", Value::Obj(target))?;
                } else if field_obj(vm, recv, "mTargetChildExcludes").is_some() {
                    tr_remove_field_list(vm, recv, "mTargetChildExcludes", &Value::Obj(target))?;
                }
            }
            self_value
        }
        ("setOrdering", "(I)Landroid/transition/TransitionSet;") if is_set => {
            match iarg(args, 0) {
                0 => put(vm, recv, "mPlayTogether", Value::Int(1))?,
                1 => put(vm, recv, "mPlayTogether", Value::Int(0))?,
                _ => {
                    return Err(throw(
                        "Landroid/util/AndroidRuntimeException;",
                        format!("Invalid parameter for TransitionSet ordering: {}", iarg(args, 0)),
                    ))
                }
            }
            self_value
        }
        ("getOrdering", "()I") if is_set => {
            Value::Int(if field_bool(vm, recv, "mPlayTogether") {
                0
            } else {
                1
            })
        }
        (
            "addTransition",
            "(Landroid/transition/Transition;)Landroid/transition/TransitionSet;",
        ) if is_set => {
            if let Some(child) = obj(args.first()) {
                tr_append_field_list(vm, recv, "mTransitions", Value::Obj(child))?;
                put(vm, child, "mParent", self_value.clone())?;
                let duration = field_long(vm, recv, "mDuration");
                if duration >= 0 {
                    tr_invoke(
                        vm,
                        child,
                        "setDuration",
                        "(J)Landroid/transition/Transition;",
                        &[Value::Long(duration)],
                    )?;
                }
                let flags = field_int(vm, recv, "mChangeFlags");
                if flags & 1 != 0 {
                    let interpolator = get(vm, recv, "mInterpolator");
                    tr_invoke(
                        vm,
                        child,
                        "setInterpolator",
                        "(Landroid/animation/TimeInterpolator;)Landroid/transition/Transition;",
                        std::slice::from_ref(&interpolator),
                    )?;
                }
                if flags & 2 != 0 {
                    let propagation = get(vm, recv, "mPropagation");
                    tr_invoke(
                        vm,
                        child,
                        "setPropagation",
                        "(Landroid/transition/TransitionPropagation;)V",
                        std::slice::from_ref(&propagation),
                    )?;
                }
                if flags & 4 != 0 {
                    let path_motion = get(vm, recv, "mPathMotion");
                    tr_invoke(
                        vm,
                        child,
                        "setPathMotion",
                        "(Landroid/transition/PathMotion;)V",
                        std::slice::from_ref(&path_motion),
                    )?;
                }
                if flags & 8 != 0 {
                    let callback = get(vm, recv, "mEpicenterCallback");
                    tr_invoke(
                        vm,
                        child,
                        "setEpicenterCallback",
                        "(Landroid/transition/Transition$EpicenterCallback;)V",
                        std::slice::from_ref(&callback),
                    )?;
                }
            }
            self_value
        }
        ("addTransitionInternal", "(Landroid/transition/Transition;)V") if is_set => {
            let child = req_obj(args, 0, "transition")?;
            tr_append_field_list(vm, recv, "mTransitions", Value::Obj(child))?;
            put(vm, child, "mParent", self_value.clone())?;
            Value::Null
        }
        ("getTransitionCount", "()I") if is_set => {
            Value::Int(tr_field_items(vm, recv, "mTransitions").len() as i32)
        }
        ("getTransitionAt", "(I)Landroid/transition/Transition;") if is_set => {
            let index = iarg(args, 0);
            let children = tr_field_items(vm, recv, "mTransitions");
            children
                .get(index as usize)
                .cloned()
                .ok_or_else(|| idx_oob(format!("Index {index} out of bounds for length {}", children.len())))?
        }
        (
            "removeTransition",
            "(Landroid/transition/Transition;)Landroid/transition/TransitionSet;",
        ) if is_set => {
            let child_value = args.first().cloned().unwrap_or(Value::Null);
            tr_remove_field_list(vm, recv, "mTransitions", &child_value)?;
            let child = req_obj(args, 0, "transition")?;
            put(vm, child, "mParent", Value::Null)?;
            self_value
        }
        ("setupStartEndListeners", "()V") if is_set => {
            let listener = alloc_obj(vm, TRANSITION_SET_LISTENER, Vec::new())?;
            put(vm, listener, "mTransitionSet", self_value)?;
            let children = tr_set_children(vm, recv);
            for child in &children {
                tr_invoke(
                    vm,
                    *child,
                    "addListener",
                    "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
                    &[Value::Obj(listener)],
                )?;
            }
            put(
                vm,
                recv,
                "mCurrentListeners",
                Value::Int(children.len() as i32),
            )?;
            Value::Null
        }
        ("setMode", "(I)Landroid/transition/Visibility;") if is_visibility => {
            let mode = iarg(args, 0);
            if mode & !3 != 0 {
                return Err(iae("Only MODE_IN and MODE_OUT flags are allowed"));
            }
            put(vm, recv, "mMode", Value::Int(mode))?;
            self_value
        }
        ("getMode", "()I") if is_visibility => Value::Int(field_int(vm, recv, "mMode")),
        ("setSuppressLayout", "(Z)V") if is_visibility => {
            put(
                vm,
                recv,
                "mSuppressLayout",
                Value::Int(i32::from(iarg(args, 0) != 0)),
            )?;
            Value::Null
        }
        ("isVisible", "(Landroid/transition/TransitionValues;)Z") if is_visibility => {
            let Some(values) = obj(args.first()) else {
                return Ok(Some(Value::Int(0)));
            };
            let visibility = tr_values_get_key(vm, Some(values), TR_PROP_VISIBILITY)?
                .and_then(|value| tr_unbox_int(vm, &value))
                .ok_or_else(|| npe("visibility"))?;
            bool_value(visibility == 0)
        }
        (
            "onAppear",
            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;ILandroid/transition/TransitionValues;I)Landroid/animation/Animator;",
        ) if is_visibility => tr_visibility_on_appear5(
            vm,
            recv,
            req_obj(args, 0, "sceneRoot")?,
            obj(args.get(1)),
            obj(args.get(3)),
        )?,
        (
            "onDisappear",
            "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;ILandroid/transition/TransitionValues;I)Landroid/animation/Animator;",
        ) if is_visibility => tr_visibility_on_disappear5(
            vm,
            recv,
            req_obj(args, 0, "sceneRoot")?,
            obj(args.get(1)),
            iarg(args, 2),
            obj(args.get(3)),
            iarg(args, 4),
        )?,
        (
            "onAppear",
            "(Landroid/view/ViewGroup;Landroid/view/View;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        ) if is_visibility => {
            if is_fade {
                let view = req_obj(args, 1, "view")?;
                {
                    let captured_alpha = tr_fade_start_alpha(vm, obj(args.get(2)), 0.0)?;
                    let start_alpha = if captured_alpha == 1.0 { 0.0 } else { captured_alpha };
                    tr_fade_create_animation(vm, recv, view, start_alpha, 1.0)?
                }
            } else {
                Value::Null
            }
        }
        (
            "onDisappear",
            "(Landroid/view/ViewGroup;Landroid/view/View;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        ) if is_visibility => {
            if is_fade {
                let view = req_obj(args, 1, "view")?;
                {
                    let start_alpha = tr_fade_start_alpha(vm, obj(args.get(2)), 1.0)?;
                    tr_fade_create_animation(vm, recv, view, start_alpha, 0.0)?
                }
            } else {
                Value::Null
            }
        }
        ("setResizeClip", "(Z)V") if vm.cp.is_subtype(class, TRANSITION_CHANGE_BOUNDS) => {
            put(
                vm,
                recv,
                "mResizeClip",
                Value::Int(i32::from(iarg(args, 0) != 0)),
            )?;
            Value::Null
        }
        ("getResizeClip", "()Z") if vm.cp.is_subtype(class, TRANSITION_CHANGE_BOUNDS) => {
            bool_value(field_bool(vm, recv, "mResizeClip"))
        }
        ("setReparent", "(Z)V") if vm.cp.is_subtype(class, TRANSITION_CHANGE_BOUNDS) => {
            put(
                vm,
                recv,
                "mReparent",
                Value::Int(i32::from(iarg(args, 0) != 0)),
            )?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

// ---------------------------------------------------------------------------
// android.system: a VFS-backed Os file/descriptor slice
// ---------------------------------------------------------------------------

const SYSEO_ENOENT: i32 = 2;
const SYSEO_EINVAL: i32 = 22;

const SYSOS_ERRNOS: &[(&str, i32, &str)] = &[
    ("EPERM", 1, "Operation not permitted"),
    ("ENOENT", 2, "No such file or directory"),
    ("ESRCH", 3, "No such process"),
    ("EINTR", 4, "Interrupted system call"),
    ("EIO", 5, "Input/output error"),
    ("ENXIO", 6, "No such device or address"),
    ("E2BIG", 7, "Argument list too long"),
    ("ENOEXEC", 8, "Exec format error"),
    ("EBADF", 9, "Bad file descriptor"),
    ("ECHILD", 10, "No child processes"),
    ("EAGAIN", 11, "Resource temporarily unavailable"),
    ("EWOULDBLOCK", 11, "Resource temporarily unavailable"),
    ("ENOMEM", 12, "Cannot allocate memory"),
    ("EACCES", 13, "Permission denied"),
    ("EFAULT", 14, "Bad address"),
    ("ENOTBLK", 15, "Block device required"),
    ("EBUSY", 16, "Device or resource busy"),
    ("EEXIST", 17, "File exists"),
    ("EXDEV", 18, "Invalid cross-device link"),
    ("ENODEV", 19, "No such device"),
    ("ENOTDIR", 20, "Not a directory"),
    ("EISDIR", 21, "Is a directory"),
    ("EINVAL", 22, "Invalid argument"),
    ("ENFILE", 23, "Too many open files in system"),
    ("EMFILE", 24, "Too many open files"),
    ("ENOTTY", 25, "Inappropriate ioctl for device"),
    ("ETXTBSY", 26, "Text file busy"),
    ("EFBIG", 27, "File too large"),
    ("ENOSPC", 28, "No space left on device"),
    ("ESPIPE", 29, "Illegal seek"),
    ("EROFS", 30, "Read-only file system"),
    ("EMLINK", 31, "Too many links"),
    ("EPIPE", 32, "Broken pipe"),
    ("EDOM", 33, "Numerical argument out of domain"),
    ("ERANGE", 34, "Numerical result out of range"),
    ("EDEADLK", 35, "Resource deadlock avoided"),
    ("EDEADLOCK", 35, "Resource deadlock avoided"),
    ("ENAMETOOLONG", 36, "File name too long"),
    ("ENOLCK", 37, "No locks available"),
    ("ENOSYS", 38, "Function not implemented"),
    ("ENOTEMPTY", 39, "Directory not empty"),
    ("ELOOP", 40, "Too many levels of symbolic links"),
    ("ENOMSG", 42, "No message of desired type"),
    ("EIDRM", 43, "Identifier removed"),
    ("EPROTO", 71, "Protocol error"),
    ("EBADMSG", 74, "Bad message"),
    ("EOVERFLOW", 75, "Value too large for defined data type"),
    (
        "EILSEQ",
        84,
        "Invalid or incomplete multibyte or wide character",
    ),
    ("EUSERS", 87, "Too many users"),
    ("ENOTSOCK", 88, "Socket operation on non-socket"),
    ("EDESTADDRREQ", 89, "Destination address required"),
    ("EMSGSIZE", 90, "Message too long"),
    ("EPROTOTYPE", 91, "Protocol wrong type for socket"),
    ("ENOPROTOOPT", 92, "Protocol not available"),
    ("EPROTONOSUPPORT", 93, "Protocol not supported"),
    ("ENOTSUP", 95, "Operation not supported"),
    ("EOPNOTSUPP", 95, "Operation not supported"),
    (
        "EAFNOSUPPORT",
        97,
        "Address family not supported by protocol",
    ),
    ("EADDRINUSE", 98, "Address already in use"),
    ("EADDRNOTAVAIL", 99, "Cannot assign requested address"),
    ("ECONNABORTED", 103, "Software caused connection abort"),
    ("ECONNRESET", 104, "Connection reset by peer"),
    ("ENOTCONN", 107, "Transport endpoint is not connected"),
    ("ETIMEDOUT", 110, "Connection timed out"),
    ("ECONNREFUSED", 111, "Connection refused"),
    ("EALREADY", 114, "Operation already in progress"),
    ("EINPROGRESS", 115, "Operation now in progress"),
    ("ESTALE", 116, "Stale file handle"),
    ("ECANCELED", 125, "Operation canceled"),
];

fn android_os_constant(field: &str) -> Option<i32> {
    if let Some((_, value, _)) = SYSOS_ERRNOS.iter().find(|(name, _, _)| *name == field) {
        return Some(*value);
    }
    Some(match field {
        "F_OK" => 0,
        "X_OK" => 1,
        "W_OK" => 2,
        "R_OK" => 4,
        "SEEK_SET" => 0,
        "SEEK_CUR" => 1,
        "SEEK_END" => 2,
        "SEEK_DATA" => 3,
        "SEEK_HOLE" => 4,
        "O_RDONLY" => 0,
        "O_WRONLY" => 1,
        "O_RDWR" => 2,
        "O_ACCMODE" => 0o3,
        "O_CREAT" => 0o100,
        "O_EXCL" => 0o200,
        "O_NOCTTY" => 0o400,
        "O_TRUNC" => 0o1000,
        "O_APPEND" => 0o2000,
        "O_NONBLOCK" => 0o4000,
        "O_DSYNC" => 0o10000,
        "O_DIRECT" => 0o40000,
        "O_DIRECTORY" => 0o200000,
        "O_NOFOLLOW" => 0o400000,
        "O_NOATIME" => 0o1000000,
        "O_CLOEXEC" => 0o2000000,
        "O_SYNC" => 0o4010000,
        "O_PATH" => 0o10000000,
        "S_IFMT" => 0o170000,
        "S_IFSOCK" => 0o140000,
        "S_IFLNK" => 0o120000,
        "S_IFREG" => 0o100000,
        "S_IFBLK" => 0o060000,
        "S_IFDIR" => 0o040000,
        "S_IFCHR" => 0o020000,
        "S_IFIFO" => 0o010000,
        "S_ISUID" => 0o4000,
        "S_ISGID" => 0o2000,
        "S_ISVTX" => 0o1000,
        "S_IRUSR" => 0o400,
        "S_IWUSR" => 0o200,
        "S_IXUSR" => 0o100,
        "S_IRWXU" => 0o700,
        "S_IRGRP" => 0o040,
        "S_IWGRP" => 0o020,
        "S_IXGRP" => 0o010,
        "S_IRWXG" => 0o070,
        "S_IROTH" => 0o004,
        "S_IWOTH" => 0o002,
        "S_IXOTH" => 0o001,
        "S_IRWXO" => 0o007,
        "ST_RDONLY" => 0x0001,
        "ST_NOSUID" => 0x0002,
        "ST_NODEV" => 0x0004,
        "ST_NOEXEC" => 0x0008,
        "ST_SYNCHRONOUS" => 0x0010,
        "ST_NOATIME" => 0x0400,
        "ST_NODIRATIME" => 0x0800,
        "ST_RELATIME" => 0x1000,
        "AF_UNSPEC" => 0,
        "AF_UNIX" | "AF_LOCAL" => 1,
        "AF_INET" => 2,
        "AF_INET6" => 10,
        "AF_NETLINK" => 16,
        "AF_PACKET" => 17,
        "AF_VSOCK" => 40,
        "EAI_BADFLAGS" => -1,
        "EAI_NONAME" => -2,
        "EAI_AGAIN" => -3,
        "EAI_FAIL" => -4,
        "EAI_NODATA" => -5,
        "EAI_FAMILY" => -6,
        "EAI_SOCKTYPE" => -7,
        "EAI_SERVICE" => -8,
        "EAI_MEMORY" => -10,
        "EAI_SYSTEM" => -11,
        "EAI_OVERFLOW" => -12,
        _ => return None,
    })
}

fn sysos_errno_entry(errno: i32) -> Option<(&'static str, &'static str)> {
    SYSOS_ERRNOS
        .iter()
        .find(|(_, value, _)| *value == errno)
        .map(|(name, _, description)| (*name, *description))
}
fn sysos_errno_name(errno: i32) -> Option<&'static str> {
    sysos_errno_entry(errno).map(|(name, _)| name)
}
fn sysos_strerror(errno: i32) -> String {
    sysos_errno_entry(errno).map_or_else(
        || format!("Unknown error {errno}"),
        |(_, description)| description.to_string(),
    )
}
fn sysos_errno_message(function: &str, errno: i32) -> String {
    let name = sysos_errno_name(errno)
        .map(str::to_string)
        .unwrap_or_else(|| format!("errno {errno}"));
    format!("{function} failed: {name} ({})", sysos_strerror(errno))
}
fn sysos_new_errno_obj(vm: &mut Engine, function: &str, errno: i32) -> Result<ObjRef, VmExit> {
    let function_value = jstr(vm, function)?;
    let message = sysos_errno_message(function, errno);
    let message_value = jstr(vm, &message)?;
    alloc_obj(
        vm,
        ANDROID_ERRNO,
        vec![
            ("functionName".to_string(), function_value),
            ("errno".to_string(), Value::Int(errno)),
            ("message".to_string(), message_value),
            ("cause".to_string(), Value::Null),
            ("rd_cause_set".to_string(), Value::Int(0)),
        ],
    )
}
fn sysos_errno_exit(vm: &mut Engine, function: &str, errno: i32) -> VmExit {
    let message = sysos_errno_message(function, errno);
    match sysos_new_errno_obj(vm, function, errno) {
        Ok(obj) => VmExit::Exception(crate::err::Throwable {
            class: ANDROID_ERRNO.to_string(),
            message: Some(message),
            obj: Some(obj),
        }),
        Err(error) => error,
    }
}
fn sysos_map<T>(vm: &mut Engine, function: &str, result: Result<T, i32>) -> Result<T, VmExit> {
    match result {
        Ok(value) => Ok(value),
        Err(errno) => Err(sysos_errno_exit(vm, function, errno)),
    }
}
fn sysos_path_arg(
    vm: &mut Engine,
    args: &[Value],
    index: usize,
    function: &str,
) -> Result<String, VmExit> {
    let path = req_str(vm, args, index, "path")?;
    if path.is_empty() {
        return Err(sysos_errno_exit(vm, function, SYSEO_ENOENT));
    }
    Ok(path)
}
fn sysos_fd_arg(vm: &mut Engine, args: &[Value], index: usize) -> Result<(ObjRef, i32), VmExit> {
    let fd_obj = req_obj(args, index, "fd")?;
    let fd = match get(vm, fd_obj, "descriptor") {
        Value::Int(value) => value,
        _ => {
            // Compatibility with path-only FileDescriptor host objects made
            // by older stream paths: lazily place them in the VFS table.
            let path = match get(vm, fd_obj, "rd_path") {
                Value::Obj(path_ref) => vm.heap.as_str(path_ref).unwrap_or("").to_string(),
                _ => String::new(),
            };
            if path.is_empty() {
                -1
            } else if let Some(registered) = crate::jfile::os_register_path(&path, 2, 0) {
                put(vm, fd_obj, "descriptor", Value::Int(registered))?;
                registered
            } else {
                -1
            }
        }
    };
    Ok((fd_obj, fd))
}
fn sysos_new_fd_obj(vm: &mut Engine, fd: i32) -> Result<Value, VmExit> {
    let path = crate::jfile::os_fd_path(fd).unwrap_or_default();
    let path_value = jstr(vm, path)?;
    let obj = alloc_obj(
        vm,
        JAVA_FILE_DESCRIPTOR,
        vec![
            ("descriptor".to_string(), Value::Int(fd)),
            ("rd_path".to_string(), path_value),
        ],
    )?;
    Ok(Value::Obj(obj))
}
fn sysos_check_bytes(
    vm: &Engine,
    args: &[Value],
    array_index: usize,
    offset_index: usize,
    count_index: usize,
) -> Result<(ObjRef, usize, usize, Vec<u8>), VmExit> {
    let array = req_obj(args, array_index, "bytes")?;
    let elems = vm
        .heap
        .as_array_elems(array)
        .map_err(VmExit::from)?
        .0
        .to_vec();
    let offset = iarg(args, offset_index);
    let count = iarg(args, count_index);
    if offset < 0
        || count < 0
        || offset as usize > elems.len()
        || count as usize > elems.len() - offset as usize
    {
        return Err(idx_oob(format!(
            "length={}; offset={offset}; count={count}",
            elems.len()
        )));
    }
    let bytes = elems[offset as usize..offset as usize + count as usize]
        .iter()
        .map(|value| value.as_int().unwrap_or(0) as u8)
        .collect();
    Ok((array, offset as usize, count as usize, bytes))
}
fn sysos_write_bytes_to_array(
    vm: &mut Engine,
    array: ObjRef,
    offset: usize,
    bytes: &[u8],
) -> Result<(), VmExit> {
    let elems = vm.heap.array_elems_mut(array).map_err(VmExit::from)?;
    for (index, byte) in bytes.iter().enumerate() {
        if let Some(slot) = elems.get_mut(offset + index) {
            *slot = Value::Int(*byte as i8 as i32);
        }
    }
    Ok(())
}
fn sysos_timespec_obj(vm: &mut Engine, sec: i64, nsec: i64) -> Result<Value, VmExit> {
    let obj = alloc_obj(
        vm,
        ANDROID_STRUCT_TIMESPEC,
        vec![
            ("tv_sec".to_string(), Value::Long(sec)),
            ("tv_nsec".to_string(), Value::Long(nsec)),
        ],
    )?;
    Ok(Value::Obj(obj))
}
fn sysos_timespec_from_ms(vm: &mut Engine, millis: i64) -> Result<Value, VmExit> {
    sysos_timespec_obj(
        vm,
        millis.div_euclid(1000),
        millis.rem_euclid(1000) * 1_000_000,
    )
}
fn sysos_stat_obj(vm: &mut Engine, stat: &crate::jfile::OsStatData) -> Result<Value, VmExit> {
    let atim = sysos_timespec_from_ms(vm, stat.atime_ms)?;
    let mtim = sysos_timespec_from_ms(vm, stat.mtime_ms)?;
    let ctim = sysos_timespec_from_ms(vm, stat.ctime_ms)?;
    let obj = alloc_obj(
        vm,
        ANDROID_STRUCT_STAT,
        vec![
            ("st_dev".to_string(), Value::Long(stat.dev)),
            ("st_ino".to_string(), Value::Long(stat.ino)),
            ("st_mode".to_string(), Value::Int(stat.mode)),
            ("st_nlink".to_string(), Value::Long(stat.nlink)),
            ("st_uid".to_string(), Value::Int(stat.uid)),
            ("st_gid".to_string(), Value::Int(stat.gid)),
            ("st_rdev".to_string(), Value::Long(stat.rdev)),
            ("st_size".to_string(), Value::Long(stat.size)),
            (
                "st_atime".to_string(),
                Value::Long(stat.atime_ms.div_euclid(1000)),
            ),
            ("st_atim".to_string(), atim),
            (
                "st_mtime".to_string(),
                Value::Long(stat.mtime_ms.div_euclid(1000)),
            ),
            ("st_mtim".to_string(), mtim),
            (
                "st_ctime".to_string(),
                Value::Long(stat.ctime_ms.div_euclid(1000)),
            ),
            ("st_ctim".to_string(), ctim),
            ("st_blksize".to_string(), Value::Long(stat.blksize)),
            ("st_blocks".to_string(), Value::Long(stat.blocks)),
        ],
    )?;
    Ok(Value::Obj(obj))
}
fn sysos_statvfs_obj(vm: &mut Engine, stat: &crate::jfile::OsStatVfsData) -> Result<Value, VmExit> {
    let obj = alloc_obj(
        vm,
        ANDROID_STRUCT_STAT_VFS,
        vec![
            ("f_bsize".to_string(), Value::Long(stat.bsize)),
            ("f_frsize".to_string(), Value::Long(stat.frsize)),
            ("f_blocks".to_string(), Value::Long(stat.blocks)),
            ("f_bfree".to_string(), Value::Long(stat.bfree)),
            ("f_bavail".to_string(), Value::Long(stat.bavail)),
            ("f_files".to_string(), Value::Long(stat.files)),
            ("f_ffree".to_string(), Value::Long(stat.ffree)),
            ("f_favail".to_string(), Value::Long(stat.favail)),
            ("f_fsid".to_string(), Value::Long(stat.fsid)),
            ("f_flag".to_string(), Value::Long(stat.flag)),
            ("f_namemax".to_string(), Value::Long(stat.namemax)),
        ],
    )?;
    Ok(Value::Obj(obj))
}
fn sysos_utsname_obj(vm: &mut Engine, fields: [&str; 5]) -> Result<Value, VmExit> {
    let mut values = Vec::new();
    for field in fields {
        values.push(jstr(vm, field)?);
    }
    let obj = alloc_obj(
        vm,
        ANDROID_STRUCT_UTSNAME,
        vec![
            ("sysname".to_string(), values[0].clone()),
            ("nodename".to_string(), values[1].clone()),
            ("release".to_string(), values[2].clone()),
            ("version".to_string(), values[3].clone()),
            ("machine".to_string(), values[4].clone()),
        ],
    )?;
    Ok(Value::Obj(obj))
}
fn sysos_long_hash(value: i64) -> i32 {
    ((value as u64) ^ ((value as u64) >> 32)) as u32 as i32
}
fn sysos_struct_string(
    vm: &mut Engine,
    class: &str,
    recv: ObjRef,
    fields: &[&str],
) -> Result<Value, VmExit> {
    let simple = class
        .trim_start_matches('L')
        .trim_end_matches(';')
        .rsplit('/')
        .next()
        .unwrap_or("Object");
    let mut rendered = Vec::new();
    for field in fields {
        rendered.push(format!(
            "{field}={}",
            value_string(vm, &get(vm, recv, field))?
        ));
    }
    jstr(vm, format!("{simple}{{{}}}", rendered.join(", ")))
}

fn android_system_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match class {
        ANDROID_ERRNO => match (name, sig) {
            ("<init>", "(Ljava/lang/String;I)V") => {
                put(
                    vm,
                    recv,
                    "functionName",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(vm, recv, "errno", Value::Int(iarg(args, 1)))?;
                Value::Null
            }
            ("<init>", "(Ljava/lang/String;ILjava/lang/Throwable;)V") => {
                put(
                    vm,
                    recv,
                    "functionName",
                    args.first().cloned().unwrap_or(Value::Null),
                )?;
                put(vm, recv, "errno", Value::Int(iarg(args, 1)))?;
                put(
                    vm,
                    recv,
                    "cause",
                    args.get(2).cloned().unwrap_or(Value::Null),
                )?;
                put(vm, recv, "rd_cause_set", Value::Int(1))?;
                Value::Null
            }
            ("getMessage", "()Ljava/lang/String;") => {
                let function = opt_str(vm, Some(&get(vm, recv, "functionName")))
                    .unwrap_or_else(|| "null".to_string());
                let message = sysos_errno_message(&function, field_int(vm, recv, "errno"));
                jstr(vm, message)?
            }
            ("rethrowAsIOException", "()Ljava/io/IOException;") => {
                let function = opt_str(vm, Some(&get(vm, recv, "functionName")))
                    .unwrap_or_else(|| "null".to_string());
                let message = sysos_errno_message(&function, field_int(vm, recv, "errno"));
                let message_value = jstr(vm, &message)?;
                let obj = alloc_obj(
                    vm,
                    "Ljava/io/IOException;",
                    vec![
                        ("message".to_string(), message_value),
                        ("cause".to_string(), Value::Obj(recv)),
                        ("rd_cause_set".to_string(), Value::Int(1)),
                    ],
                )?;
                return Err(VmExit::Exception(crate::err::Throwable {
                    class: "Ljava/io/IOException;".to_string(),
                    message: Some(message),
                    obj: Some(obj),
                }));
            }
            ("rethrowAsSocketException", "()Ljava/net/SocketException;") => {
                let function = opt_str(vm, Some(&get(vm, recv, "functionName")))
                    .unwrap_or_else(|| "null".to_string());
                let message = sysos_errno_message(&function, field_int(vm, recv, "errno"));
                let message_value = jstr(vm, &message)?;
                let obj = alloc_obj(
                    vm,
                    "Ljava/net/SocketException;",
                    vec![
                        ("message".to_string(), message_value),
                        ("cause".to_string(), Value::Obj(recv)),
                        ("rd_cause_set".to_string(), Value::Int(1)),
                    ],
                )?;
                return Err(VmExit::Exception(crate::err::Throwable {
                    class: "Ljava/net/SocketException;".to_string(),
                    message: Some(message),
                    obj: Some(obj),
                }));
            }
            _ => return Ok(None),
        },
        ANDROID_STRUCT_TIMESPEC => match (name, sig) {
            ("<init>", "(JJ)V") => {
                let sec = larg(args, 0);
                let nsec = larg(args, 1);
                put(vm, recv, "tv_sec", Value::Long(sec))?;
                put(vm, recv, "tv_nsec", Value::Long(nsec))?;
                if !(0..=999_999_999).contains(&nsec) {
                    return Err(iae(format!(
                        "tv_nsec value {nsec} is not in [0, 999999999]"
                    )));
                }
                Value::Null
            }
            ("compareTo", "(Landroid/system/StructTimespec;)I")
            | ("compareTo", "(Ljava/lang/Object;)I") => {
                let other = req_obj(args, 0, "other")?;
                if class_of(vm, other) != ANDROID_STRUCT_TIMESPEC {
                    return Err(throw(
                        "Ljava/lang/ClassCastException;",
                        "StructTimespec",
                    ));
                }
                let sec = field_long(vm, recv, "tv_sec");
                let other_sec = field_long(vm, other, "tv_sec");
                let nsec = field_long(vm, recv, "tv_nsec");
                let other_nsec = field_long(vm, other, "tv_nsec");
                Value::Int(match sec.cmp(&other_sec) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Equal => match nsec.cmp(&other_nsec) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Greater => 1,
                        std::cmp::Ordering::Equal => 0,
                    },
                })
            }
            ("equals", "(Ljava/lang/Object;)Z") => {
                let equal = match obj(args.first()) {
                    Some(other) if other == recv => true,
                    Some(other) if class_of(vm, other) == ANDROID_STRUCT_TIMESPEC => {
                        field_long(vm, recv, "tv_sec") == field_long(vm, other, "tv_sec")
                            && field_long(vm, recv, "tv_nsec") == field_long(vm, other, "tv_nsec")
                    }
                    _ => false,
                };
                bool_value(equal)
            }
            ("hashCode", "()I") => Value::Int(
                sysos_long_hash(field_long(vm, recv, "tv_sec"))
                    .wrapping_mul(31)
                    .wrapping_add(sysos_long_hash(field_long(vm, recv, "tv_nsec"))),
            ),
            ("toString", "()Ljava/lang/String;") => {
                sysos_struct_string(vm, class, recv, &["tv_sec", "tv_nsec"])?
            }
            _ => return Ok(None),
        },
        ANDROID_STRUCT_STAT => match (name, sig) {
            ("<init>", "(JJIJIIJJJJJJJ)V") => {
                put(vm, recv, "st_dev", Value::Long(larg(args, 0)))?;
                put(vm, recv, "st_ino", Value::Long(larg(args, 1)))?;
                put(vm, recv, "st_mode", Value::Int(iarg(args, 2)))?;
                put(vm, recv, "st_nlink", Value::Long(larg(args, 3)))?;
                put(vm, recv, "st_uid", Value::Int(iarg(args, 4)))?;
                put(vm, recv, "st_gid", Value::Int(iarg(args, 5)))?;
                put(vm, recv, "st_rdev", Value::Long(larg(args, 6)))?;
                put(vm, recv, "st_size", Value::Long(larg(args, 7)))?;
                for (index, sec_field, tim_field) in [
                    (8, "st_atime", "st_atim"),
                    (9, "st_mtime", "st_mtim"),
                    (10, "st_ctime", "st_ctim"),
                ] {
                    put(vm, recv, sec_field, Value::Long(larg(args, index)))?;
                    let tim = sysos_timespec_obj(vm, larg(args, index), 0)?;
                    put(vm, recv, tim_field, tim)?;
                }
                put(vm, recv, "st_blksize", Value::Long(larg(args, 11)))?;
                put(vm, recv, "st_blocks", Value::Long(larg(args, 12)))?;
                Value::Null
            }
            (
                "<init>",
                "(JJIJIIJJLandroid/system/StructTimespec;Landroid/system/StructTimespec;Landroid/system/StructTimespec;JJ)V",
            ) => {
                put(vm, recv, "st_dev", Value::Long(larg(args, 0)))?;
                put(vm, recv, "st_ino", Value::Long(larg(args, 1)))?;
                put(vm, recv, "st_mode", Value::Int(iarg(args, 2)))?;
                put(vm, recv, "st_nlink", Value::Long(larg(args, 3)))?;
                put(vm, recv, "st_uid", Value::Int(iarg(args, 4)))?;
                put(vm, recv, "st_gid", Value::Int(iarg(args, 5)))?;
                put(vm, recv, "st_rdev", Value::Long(larg(args, 6)))?;
                put(vm, recv, "st_size", Value::Long(larg(args, 7)))?;
                for (index, sec_field, tim_field) in [
                    (8, "st_atime", "st_atim"),
                    (9, "st_mtime", "st_mtim"),
                    (10, "st_ctime", "st_ctim"),
                ] {
                    let tim = req_obj(args, index, tim_field)?;
                    put(
                        vm,
                        recv,
                        sec_field,
                        Value::Long(field_long(vm, tim, "tv_sec")),
                    )?;
                    put(vm, recv, tim_field, Value::Obj(tim))?;
                }
                put(vm, recv, "st_blksize", Value::Long(larg(args, 11)))?;
                put(vm, recv, "st_blocks", Value::Long(larg(args, 12)))?;
                Value::Null
            }
            ("toString", "()Ljava/lang/String;") => sysos_struct_string(
                vm,
                class,
                recv,
                &[
                    "st_dev", "st_ino", "st_mode", "st_nlink", "st_uid", "st_gid", "st_rdev",
                    "st_size", "st_atim", "st_mtim", "st_ctim", "st_blksize", "st_blocks",
                ],
            )?,
            _ => return Ok(None),
        },
        ANDROID_STRUCT_STAT_VFS => match (name, sig) {
            ("<init>", "(JJJJJJJJJJJ)V") => {
                for (index, field) in [
                    "f_bsize", "f_frsize", "f_blocks", "f_bfree", "f_bavail", "f_files",
                    "f_ffree", "f_favail", "f_fsid", "f_flag", "f_namemax",
                ]
                .into_iter()
                .enumerate()
                {
                    put(vm, recv, field, Value::Long(larg(args, index)))?;
                }
                Value::Null
            }
            ("toString", "()Ljava/lang/String;") => sysos_struct_string(
                vm,
                class,
                recv,
                &[
                    "f_bsize", "f_frsize", "f_blocks", "f_bfree", "f_bavail", "f_files",
                    "f_ffree", "f_favail", "f_fsid", "f_flag", "f_namemax",
                ],
            )?,
            _ => return Ok(None),
        },
        ANDROID_STRUCT_UTSNAME => match (name, sig) {
            (
                "<init>",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            ) => {
                for (index, field) in ["sysname", "nodename", "release", "version", "machine"]
                    .into_iter()
                    .enumerate()
                {
                    put(
                        vm,
                        recv,
                        field,
                        args.get(index).cloned().unwrap_or(Value::Null),
                    )?;
                }
                Value::Null
            }
            ("toString", "()Ljava/lang/String;") => sysos_struct_string(
                vm,
                class,
                recv,
                &["sysname", "nodename", "release", "version", "machine"],
            )?,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn android_os_constants_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let mode = iarg(args, 0);
    let value = match (name, sig) {
        ("errnoName", "(I)Ljava/lang/String;") => match sysos_errno_name(mode) {
            Some(name) => jstr(vm, name)?,
            None => Value::Null,
        },
        ("gaiName", "(I)Ljava/lang/String;") => {
            let found = [
                ("EAI_AGAIN", -3),
                ("EAI_BADFLAGS", -1),
                ("EAI_FAIL", -4),
                ("EAI_FAMILY", -6),
                ("EAI_MEMORY", -10),
                ("EAI_NODATA", -5),
                ("EAI_NONAME", -2),
                ("EAI_OVERFLOW", -12),
                ("EAI_SERVICE", -8),
                ("EAI_SOCKTYPE", -7),
                ("EAI_SYSTEM", -11),
            ]
            .into_iter()
            .find(|(_, value)| *value == mode)
            .map(|(name, _)| name);
            match found {
                Some(name) => jstr(vm, name)?,
                None => Value::Null,
            }
        }
        ("S_ISBLK", "(I)Z") => bool_value(mode & 0o170000 == 0o060000),
        ("S_ISCHR", "(I)Z") => bool_value(mode & 0o170000 == 0o020000),
        ("S_ISDIR", "(I)Z") => bool_value(mode & 0o170000 == 0o040000),
        ("S_ISFIFO", "(I)Z") => bool_value(mode & 0o170000 == 0o010000),
        ("S_ISREG", "(I)Z") => bool_value(mode & 0o170000 == 0o100000),
        ("S_ISLNK", "(I)Z") => bool_value(mode & 0o170000 == 0o120000),
        ("S_ISSOCK", "(I)Z") => bool_value(mode & 0o170000 == 0o140000),
        ("CAP_TO_MASK", "(I)I") => Value::Int(1 << (mode & 31)),
        ("WEXITSTATUS", "(I)I") | ("WSTOPSIG", "(I)I") => Value::Int((mode & 0xff00) >> 8),
        ("WTERMSIG", "(I)I") => Value::Int(mode & 0x7f),
        ("WCOREDUMP", "(I)Z") => bool_value(mode & 0x80 != 0),
        ("WIFEXITED", "(I)Z") => bool_value(mode & 0x7f == 0),
        ("WIFSTOPPED", "(I)Z") => bool_value(mode & 0x7f == 0x7f),
        ("WIFSIGNALED", "(I)Z") => bool_value((mode.wrapping_add(1) & 0x7f) >= 2),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn sysos_environment_map(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Value::Obj(map) = tr_static_get(vm, ANDROID_OS, "rdEnvironment") {
        return Ok(map);
    }
    let map = tr_new_map(vm)?;
    let package = if vm.fw.package_name.is_empty() {
        "app".to_string()
    } else {
        vm.fw.package_name.clone()
    };
    for (key, value) in [
        ("ANDROID_DATA", "/data".to_string()),
        ("ANDROID_ROOT", "/system".to_string()),
        ("ANDROID_STORAGE", "/storage".to_string()),
        ("EXTERNAL_STORAGE", "/storage/emulated/0".to_string()),
        ("HOME", format!("/data/user/0/{package}")),
        ("PATH", "/system/bin:/system/xbin".to_string()),
        ("TMPDIR", "/data/local/tmp".to_string()),
    ] {
        let key = jstr(vm, key)?;
        let value = jstr(vm, &value)?;
        tr_map_put(vm, map, key, value)?;
    }
    tr_static_set(vm, ANDROID_OS, "rdEnvironment", Value::Obj(map));
    Ok(map)
}
fn sysos_valid_env_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('=') && !name.contains('\0')
}

#[allow(clippy::too_many_lines)]
fn android_os_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("access", "(Ljava/lang/String;I)Z") => {
            let path = sysos_path_arg(vm, args, 0, "access")?;
            bool_value(sysos_map(
                vm,
                "access",
                crate::jfile::os_access(&path, iarg(args, 1)),
            )?)
        }
        ("chmod", "(Ljava/lang/String;I)V") => {
            let path = sysos_path_arg(vm, args, 0, "chmod")?;
            sysos_map(vm, "chmod", crate::jfile::os_chmod(&path, iarg(args, 1)))?;
            Value::Null
        }
        ("chown", "(Ljava/lang/String;II)V") | ("lchown", "(Ljava/lang/String;II)V") => {
            let path = sysos_path_arg(vm, args, 0, name)?;
            sysos_map(
                vm,
                name,
                crate::jfile::os_chown(&path, iarg(args, 1), iarg(args, 2)),
            )?;
            Value::Null
        }
        ("close", "(Ljava/io/FileDescriptor;)V") => {
            let (fd_obj, fd) = sysos_fd_arg(vm, args, 0)?;
            sysos_map(vm, "close", crate::jfile::os_close(fd))?;
            put(vm, fd_obj, "descriptor", Value::Int(-1))?;
            Value::Null
        }
        ("dup", "(Ljava/io/FileDescriptor;)Ljava/io/FileDescriptor;") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let new_fd = sysos_map(vm, "dup", crate::jfile::os_dup(fd))?;
            sysos_new_fd_obj(vm, new_fd)?
        }
        ("dup2", "(Ljava/io/FileDescriptor;I)Ljava/io/FileDescriptor;") => {
            let (fd_obj, fd) = sysos_fd_arg(vm, args, 0)?;
            let new_fd = sysos_map(vm, "dup2", crate::jfile::os_dup2(fd, iarg(args, 1)))?;
            if new_fd == fd {
                Value::Obj(fd_obj)
            } else {
                sysos_new_fd_obj(vm, new_fd)?
            }
        }
        ("environ", "()[Ljava/lang/String;") => {
            let map = sysos_environment_map(vm)?;
            let mut values = Vec::new();
            for (key, value) in tr_map_entries(vm, &Value::Obj(map)) {
                let entry = format!("{}={}", value_string(vm, &key)?, value_string(vm, &value)?);
                values.push(jstr(vm, entry)?);
            }
            obj_array(vm, "[Ljava/lang/String;", values)?
        }
        ("fchmod", "(Ljava/io/FileDescriptor;I)V") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            sysos_map(vm, "fchmod", crate::jfile::os_fchmod(fd, iarg(args, 1)))?;
            Value::Null
        }
        ("fchown", "(Ljava/io/FileDescriptor;II)V") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            sysos_map(
                vm,
                "fchown",
                crate::jfile::os_fchown(fd, iarg(args, 1), iarg(args, 2)),
            )?;
            Value::Null
        }
        ("fdatasync", "(Ljava/io/FileDescriptor;)V") | ("fsync", "(Ljava/io/FileDescriptor;)V") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            sysos_map(vm, name, crate::jfile::os_fsync(fd))?;
            Value::Null
        }
        ("fstat", "(Ljava/io/FileDescriptor;)Landroid/system/StructStat;") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let stat = sysos_map(vm, "fstat", crate::jfile::os_fstat(fd))?;
            sysos_stat_obj(vm, &stat)?
        }
        ("fstatvfs", "(Ljava/io/FileDescriptor;)Landroid/system/StructStatVfs;") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let stat = sysos_map(vm, "fstatvfs", crate::jfile::os_fstatvfs(fd))?;
            sysos_statvfs_obj(vm, &stat)?
        }
        ("ftruncate", "(Ljava/io/FileDescriptor;J)V") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            sysos_map(
                vm,
                "ftruncate",
                crate::jfile::os_ftruncate(fd, larg(args, 1)),
            )?;
            Value::Null
        }
        ("getenv", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let name_arg = req_str(vm, args, 0, "name")?;
            let map = sysos_environment_map(vm)?;
            let key = jstr(vm, &name_arg)?;
            tr_map_get(vm, &Value::Obj(map), &key).unwrap_or(Value::Null)
        }
        ("setenv", "(Ljava/lang/String;Ljava/lang/String;Z)V") => {
            let env_name = req_str(vm, args, 0, "name")?;
            let env_value = req_str(vm, args, 1, "value")?;
            if !sysos_valid_env_name(&env_name) {
                return Err(sysos_errno_exit(vm, "setenv", SYSEO_EINVAL));
            }
            let map = sysos_environment_map(vm)?;
            let key = jstr(vm, &env_name)?;
            if iarg(args, 2) != 0 || tr_map_get(vm, &Value::Obj(map), &key).is_none() {
                let value = jstr(vm, &env_value)?;
                tr_map_put(vm, map, key, value)?;
            }
            Value::Null
        }
        ("unsetenv", "(Ljava/lang/String;)V") => {
            let env_name = req_str(vm, args, 0, "name")?;
            if !sysos_valid_env_name(&env_name) {
                return Err(sysos_errno_exit(vm, "unsetenv", SYSEO_EINVAL));
            }
            let map = sysos_environment_map(vm)?;
            let key = jstr(vm, &env_name)?;
            tr_map_remove(vm, map, &key)?;
            Value::Null
        }
        ("getpid", "()I")
        | ("gettid", "()I")
        | ("getuid", "()I")
        | ("geteuid", "()I")
        | ("getgid", "()I")
        | ("getegid", "()I") => Value::Int(10000),
        ("getppid", "()I") => Value::Int(1),
        ("isatty", "(Ljava/io/FileDescriptor;)Z") => {
            if obj(args.first()).is_none() {
                Value::Int(0)
            } else {
                // All descriptors in this slice are regular VFS files or
                // directories, never terminals.
                let _ = sysos_fd_arg(vm, args, 0)?;
                Value::Int(0)
            }
        }
        ("lseek", "(Ljava/io/FileDescriptor;JI)J") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            Value::Long(sysos_map(
                vm,
                "lseek",
                crate::jfile::os_lseek(fd, larg(args, 1), iarg(args, 2)),
            )?)
        }
        ("stat", "(Ljava/lang/String;)Landroid/system/StructStat;")
        | ("lstat", "(Ljava/lang/String;)Landroid/system/StructStat;") => {
            let path = sysos_path_arg(vm, args, 0, name)?;
            let stat = sysos_map(vm, name, crate::jfile::os_stat(&path))?;
            sysos_stat_obj(vm, &stat)?
        }
        ("statvfs", "(Ljava/lang/String;)Landroid/system/StructStatVfs;") => {
            let path = sysos_path_arg(vm, args, 0, "statvfs")?;
            let stat = sysos_map(vm, "statvfs", crate::jfile::os_statvfs(&path))?;
            sysos_statvfs_obj(vm, &stat)?
        }
        ("mkdir", "(Ljava/lang/String;I)V") => {
            let path = sysos_path_arg(vm, args, 0, "mkdir")?;
            sysos_map(vm, "mkdir", crate::jfile::os_mkdir(&path, iarg(args, 1)))?;
            Value::Null
        }
        ("open", "(Ljava/lang/String;II)Ljava/io/FileDescriptor;") => {
            let path = sysos_path_arg(vm, args, 0, "open")?;
            let fd = sysos_map(
                vm,
                "open",
                crate::jfile::os_open(&path, iarg(args, 1), iarg(args, 2)),
            )?;
            sysos_new_fd_obj(vm, fd)?
        }
        ("read", "(Ljava/io/FileDescriptor;[BII)I") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let (array, offset, count, _) = sysos_check_bytes(vm, args, 1, 2, 3)?;
            if count == 0 {
                Value::Int(0)
            } else {
                let bytes = sysos_map(vm, "read", crate::jfile::os_read(fd, count))?;
                if bytes.is_empty() {
                    Value::Int(-1)
                } else {
                    let read_count = bytes.len() as i32;
                    sysos_write_bytes_to_array(vm, array, offset, &bytes)?;
                    Value::Int(read_count)
                }
            }
        }
        ("pread", "(Ljava/io/FileDescriptor;[BIIJ)I") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let (array, offset, count, _) = sysos_check_bytes(vm, args, 1, 2, 3)?;
            if count == 0 {
                Value::Int(0)
            } else {
                let bytes = sysos_map(
                    vm,
                    "pread",
                    crate::jfile::os_pread(fd, count, larg(args, 4)),
                )?;
                if bytes.is_empty() {
                    Value::Int(-1)
                } else {
                    let read_count = bytes.len() as i32;
                    sysos_write_bytes_to_array(vm, array, offset, &bytes)?;
                    Value::Int(read_count)
                }
            }
        }
        ("write", "(Ljava/io/FileDescriptor;[BII)I") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let (_, _, count, bytes) = sysos_check_bytes(vm, args, 1, 2, 3)?;
            if count == 0 {
                Value::Int(0)
            } else {
                Value::Int(sysos_map(vm, "write", crate::jfile::os_write(fd, &bytes))? as i32)
            }
        }
        ("pwrite", "(Ljava/io/FileDescriptor;[BIIJ)I") => {
            let (_, fd) = sysos_fd_arg(vm, args, 0)?;
            let (_, _, count, bytes) = sysos_check_bytes(vm, args, 1, 2, 3)?;
            if count == 0 {
                Value::Int(0)
            } else {
                Value::Int(sysos_map(
                    vm,
                    "pwrite",
                    crate::jfile::os_pwrite(fd, &bytes, larg(args, 4)),
                )? as i32)
            }
        }
        ("readlink", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let path = sysos_path_arg(vm, args, 0, "readlink")?;
            // The VFS has no links: a path that exists is not a symbolic
            // link (EINVAL), and all other failures keep their errno.
            sysos_map(vm, "readlink", crate::jfile::os_stat(&path))?;
            return Err(sysos_errno_exit(vm, "readlink", SYSEO_EINVAL));
        }
        ("realpath", "(Ljava/lang/String;)Ljava/lang/String;") => {
            if obj(args.first()).is_none() {
                Value::Null
            } else {
                let path = sysos_path_arg(vm, args, 0, "realpath")?;
                let canonical = sysos_map(vm, "realpath", crate::jfile::os_realpath(&path))?;
                jstr(vm, canonical)?
            }
        }
        ("remove", "(Ljava/lang/String;)V") => {
            let path = sysos_path_arg(vm, args, 0, "remove")?;
            sysos_map(vm, "remove", crate::jfile::os_remove(&path))?;
            Value::Null
        }
        ("unlink", "(Ljava/lang/String;)V") => {
            let path = sysos_path_arg(vm, args, 0, "unlink")?;
            sysos_map(vm, "unlink", crate::jfile::os_unlink(&path))?;
            Value::Null
        }
        ("rename", "(Ljava/lang/String;Ljava/lang/String;)V") => {
            let old_path = sysos_path_arg(vm, args, 0, "rename")?;
            let new_path = sysos_path_arg(vm, args, 1, "rename")?;
            sysos_map(vm, "rename", crate::jfile::os_rename(&old_path, &new_path))?;
            Value::Null
        }
        ("strerror", "(I)Ljava/lang/String;") => jstr(vm, sysos_strerror(iarg(args, 0)))?,
        ("uname", "()Landroid/system/StructUtsname;") => {
            let machine = match std::env::consts::ARCH {
                "aarch64" => "aarch64",
                "arm" => "armv7l",
                _ => "x86_64",
            };
            sysos_utsname_obj(
                vm,
                [
                    "Linux",
                    "localhost",
                    "6.1.0-rustydroid",
                    "#1 SMP PREEMPT_DYNAMIC",
                    machine,
                ],
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

// ---------------------------------------------------------------------------
// Appendix C Batch 57-A: bounded no-service/value subsets for eight surfaces
// that were previously `not_implemented`:
//   android.view.textservice, android.view.translation,
//   android.hardware.usb, android.telephony.euicc, android.app.usage,
//   android.print, android.security.keystore and android.mtp.
//
// These handlers model only the documented subsets in data/api-coverage.toml.
// In particular there is no spell-check or translation service, no USB/MTP
// device, no eUICC controller, no usage-history database, no print spooler or
// printer, and no AndroidKeyStore provider/key material below.
// ---------------------------------------------------------------------------

const P57_TEXT_SERVICES_MANAGER: &str = "Landroid/view/textservice/TextServicesManager;";
const P57_SPELL_SESSION: &str = "Landroid/view/textservice/SpellCheckerSession;";
const P57_SPELL_PARAMS: &str =
    "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;";
const P57_SPELL_PARAMS_BUILDER: &str =
    "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;";
const P57_TEXT_INFO: &str = "Landroid/view/textservice/TextInfo;";
const P57_SUGGESTIONS_INFO: &str = "Landroid/view/textservice/SuggestionsInfo;";
const P57_SENTENCE_SUGGESTIONS_INFO: &str = "Landroid/view/textservice/SentenceSuggestionsInfo;";
const P57_SPELL_SUBTYPE: &str = "Landroid/view/textservice/SpellCheckerSubtype;";

const P57_TRANSLATION_MANAGER: &str = "Landroid/view/translation/TranslationManager;";
const P57_TRANSLATION_SPEC: &str = "Landroid/view/translation/TranslationSpec;";
const P57_TRANSLATION_REQUEST_VALUE: &str = "Landroid/view/translation/TranslationRequestValue;";
const P57_VIEW_TRANSLATION_REQUEST: &str = "Landroid/view/translation/ViewTranslationRequest;";
const P57_VIEW_TRANSLATION_REQUEST_BUILDER: &str =
    "Landroid/view/translation/ViewTranslationRequest$Builder;";
const P57_TRANSLATION_CONTEXT: &str = "Landroid/view/translation/TranslationContext;";
const P57_TRANSLATION_CONTEXT_BUILDER: &str =
    "Landroid/view/translation/TranslationContext$Builder;";
const P57_TRANSLATION_CAPABILITY: &str = "Landroid/view/translation/TranslationCapability;";

const P57_USB_MANAGER: &str = "Landroid/hardware/usb/UsbManager;";
const P57_USB_CONSTANTS: &str = "Landroid/hardware/usb/UsbConstants;";
const P57_USB_DEVICE: &str = "Landroid/hardware/usb/UsbDevice;";
const P57_USB_DEVICE_BUILDER: &str = "Landroid/hardware/usb/UsbDevice$Builder;";
const P57_USB_CONFIGURATION: &str = "Landroid/hardware/usb/UsbConfiguration;";
const P57_USB_INTERFACE: &str = "Landroid/hardware/usb/UsbInterface;";
const P57_USB_ENDPOINT: &str = "Landroid/hardware/usb/UsbEndpoint;";

const P57_EUICC_MANAGER: &str = "Landroid/telephony/euicc/EuiccManager;";
const P57_EUICC_INFO: &str = "Landroid/telephony/euicc/EuiccInfo;";

const P57_USAGE_MANAGER: &str = "Landroid/app/usage/UsageStatsManager;";
const P57_USAGE_STATS: &str = "Landroid/app/usage/UsageStats;";
const P57_USAGE_EVENTS: &str = "Landroid/app/usage/UsageEvents;";
const P57_USAGE_EVENT: &str = "Landroid/app/usage/UsageEvents$Event;";

const P57_PRINT_MANAGER: &str = "Landroid/print/PrintManager;";
const P57_PRINT_ATTRIBUTES: &str = "Landroid/print/PrintAttributes;";
const P57_PRINT_ATTRIBUTES_BUILDER: &str = "Landroid/print/PrintAttributes$Builder;";
const P57_MEDIA_SIZE: &str = "Landroid/print/PrintAttributes$MediaSize;";
const P57_RESOLUTION: &str = "Landroid/print/PrintAttributes$Resolution;";
const P57_MARGINS: &str = "Landroid/print/PrintAttributes$Margins;";
const P57_PAGE_RANGE: &str = "Landroid/print/PageRange;";
const P57_PRINTER_ID: &str = "Landroid/print/PrinterId;";

const P57_KEY_PROPERTIES: &str = "Landroid/security/keystore/KeyProperties;";
const P57_KEY_GEN_SPEC: &str = "Landroid/security/keystore/KeyGenParameterSpec;";
const P57_KEY_GEN_SPEC_BUILDER: &str = "Landroid/security/keystore/KeyGenParameterSpec$Builder;";
const P57_KEY_PROTECTION: &str = "Landroid/security/keystore/KeyProtection;";
const P57_KEY_PROTECTION_BUILDER: &str = "Landroid/security/keystore/KeyProtection$Builder;";
const P57_KEY_INFO: &str = "Landroid/security/keystore/KeyInfo;";

const P57_MTP_CONSTANTS: &str = "Landroid/mtp/MtpConstants;";
const P57_MTP_DEVICE_INFO: &str = "Landroid/mtp/MtpDeviceInfo;";
const P57_MTP_STORAGE_INFO: &str = "Landroid/mtp/MtpStorageInfo;";
const P57_MTP_OBJECT_INFO: &str = "Landroid/mtp/MtpObjectInfo;";
const P57_MTP_OBJECT_INFO_BUILDER: &str = "Landroid/mtp/MtpObjectInfo$Builder;";
const P57_MTP_DEVICE: &str = "Landroid/mtp/MtpDevice;";

fn p57_class(c: &str) -> bool {
    matches!(
        c,
        P57_TEXT_SERVICES_MANAGER
            | P57_SPELL_SESSION
            | P57_SPELL_PARAMS
            | P57_SPELL_PARAMS_BUILDER
            | P57_TEXT_INFO
            | P57_SUGGESTIONS_INFO
            | P57_SENTENCE_SUGGESTIONS_INFO
            | P57_SPELL_SUBTYPE
            | P57_TRANSLATION_MANAGER
            | P57_TRANSLATION_SPEC
            | P57_TRANSLATION_REQUEST_VALUE
            | P57_VIEW_TRANSLATION_REQUEST
            | P57_VIEW_TRANSLATION_REQUEST_BUILDER
            | P57_TRANSLATION_CONTEXT
            | P57_TRANSLATION_CONTEXT_BUILDER
            | P57_TRANSLATION_CAPABILITY
            | P57_USB_MANAGER
            | P57_USB_DEVICE
            | P57_USB_DEVICE_BUILDER
            | P57_USB_CONFIGURATION
            | P57_USB_INTERFACE
            | P57_USB_ENDPOINT
            | P57_EUICC_MANAGER
            | P57_EUICC_INFO
            | P57_USAGE_MANAGER
            | P57_USAGE_STATS
            | P57_USAGE_EVENTS
            | P57_USAGE_EVENT
            | P57_PRINT_MANAGER
            | P57_PRINT_ATTRIBUTES
            | P57_PRINT_ATTRIBUTES_BUILDER
            | P57_MEDIA_SIZE
            | P57_RESOLUTION
            | P57_MARGINS
            | P57_PAGE_RANGE
            | P57_PRINTER_ID
            | P57_KEY_GEN_SPEC
            | P57_KEY_GEN_SPEC_BUILDER
            | P57_KEY_PROTECTION
            | P57_KEY_PROTECTION_BUILDER
            | P57_KEY_INFO
            | P57_MTP_DEVICE_INFO
            | P57_MTP_STORAGE_INFO
            | P57_MTP_OBJECT_INFO
            | P57_MTP_OBJECT_INFO_BUILDER
            | P57_MTP_DEVICE
    )
}

fn p57_opt_string(vm: &mut Engine, s: Option<String>) -> Result<Value, VmExit> {
    match s {
        Some(s) => jstr(vm, s),
        None => Ok(Value::Null),
    }
}

fn p57_clone_array(
    vm: &mut Engine,
    value: &Value,
    desc: &str,
    kind: ElemKind,
) -> Result<Value, VmExit> {
    let elems = array_elems(vm, Some(value));
    crate::arrays::new_array(vm, desc, kind, elems)
}

fn p57_string_values(vm: &Engine, value: &Value) -> Vec<Option<String>> {
    array_elems(vm, Some(value))
        .iter()
        .map(|v| opt_str(vm, Some(v)))
        .collect()
}

fn p57_new_string_array(vm: &mut Engine, values: &[Option<String>]) -> Result<Value, VmExit> {
    let mut elems = Vec::with_capacity(values.len());
    for value in values {
        elems.push(p57_opt_string(vm, value.clone())?);
    }
    obj_array(vm, "[Ljava/lang/String;", elems)
}

fn p57_new_map(vm: &mut Engine, entries: Vec<(Value, Value)>) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Ljava/util/HashMap;", Vec::new())?;
    vm.fw.objects.insert(r, HostObj::Map { entries });
    Ok(Value::Obj(r))
}

fn p57_map_entries(vm: &Engine, recv: ObjRef, field: &str) -> Vec<(Value, Value)> {
    match field_obj(vm, recv, field) {
        Some(map) => match vm.fw.objects.get(&map) {
            Some(HostObj::Map { entries }) => entries.clone(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    }
}

fn p57_ensure_map(vm: &mut Engine, recv: ObjRef, field: &str) -> Result<ObjRef, VmExit> {
    if let Some(map) = field_obj(vm, recv, field) {
        if matches!(vm.fw.objects.get(&map), Some(HostObj::Map { .. })) {
            return Ok(map);
        }
    }
    let map = match p57_new_map(vm, Vec::new())? {
        Value::Obj(r) => r,
        _ => unreachable!(),
    };
    put(vm, recv, field, Value::Obj(map))?;
    Ok(map)
}

fn p57_map_put_string(
    vm: &mut Engine,
    recv: ObjRef,
    field: &str,
    key: String,
    value: Value,
) -> Result<(), VmExit> {
    let map = p57_ensure_map(vm, recv, field)?;
    let key_value = jstr(vm, key.clone())?;
    if let Some(HostObj::Map { entries }) = vm.fw.objects.get_mut(&map) {
        let mut replaced = false;
        for (k, v) in entries.iter_mut() {
            if let Value::Obj(kr) = k {
                if vm.heap.as_str(*kr).ok() == Some(key.as_str()) {
                    *v = value.clone();
                    replaced = true;
                    break;
                }
            }
        }
        if !replaced {
            entries.push((key_value, value));
        }
    }
    Ok(())
}

fn p57_date_ms(vm: &Engine, value: &Value) -> Option<i64> {
    let Value::Obj(r) = value else {
        return None;
    };
    if class_of(vm, *r) != "Ljava/util/Date;" {
        return None;
    }
    match get(vm, *r, "rd_ms") {
        Value::Long(ms) => Some(ms),
        Value::Int(ms) => Some(ms as i64),
        _ => Some(0),
    }
}

fn p57_new_date(vm: &mut Engine, ms: i64) -> Result<Value, VmExit> {
    let r = alloc_obj(
        vm,
        "Ljava/util/Date;",
        vec![("rd_ms".to_string(), Value::Long(ms))],
    )?;
    Ok(Value::Obj(r))
}

fn p57_clone_date(vm: &mut Engine, value: &Value) -> Result<Value, VmExit> {
    match p57_date_ms(vm, value) {
        Some(ms) => p57_new_date(vm, ms),
        None => Ok(value.clone()),
    }
}

fn p57_utf16_slice(text: &str, start: i32, end: i32) -> Result<String, VmExit> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if start < 0 || end < start || end as usize > units.len() {
        return Err(idx_oob(format!(
            "start {start}, end {end}, length {}",
            units.len()
        )));
    }
    Ok(String::from_utf16_lossy(
        &units[start as usize..end as usize],
    ))
}

fn p57_check_builder_unused(vm: &Engine, recv: ObjRef) -> Result<(), VmExit> {
    if field_bool(vm, recv, "rd_used") {
        return Err(ise("Builder should not be touched after build()"));
    }
    Ok(())
}

// --- android.view.textservice ------------------------------------------------

fn p57_text_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;)V") => {
            let text = opt_str_arg(vm, args, 0);
            if text.as_deref().map(str::is_empty).unwrap_or(true) {
                return Err(iae("charSequence is empty"));
            }
            let value = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mCharSequence", value)?;
            put(vm, recv, "mCookie", Value::Int(0))?;
            put(vm, recv, "mSequenceNumber", Value::Int(0))?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;II)V") => {
            let text = opt_str_arg(vm, args, 0);
            if text.as_deref().map(str::is_empty).unwrap_or(true) {
                return Err(iae("charSequence is empty"));
            }
            let value = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mCharSequence", value)?;
            put(vm, recv, "mCookie", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "mSequenceNumber", Value::Int(iarg(args, 2)))?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/CharSequence;IIII)V") => {
            let text = charseq(vm, args.first())?;
            let Some(text) = text.filter(|s| !s.is_empty()) else {
                return Err(iae("charSequence is empty"));
            };
            let slice = p57_utf16_slice(&text, iarg(args, 1), iarg(args, 2))?;
            let value = jstr(vm, slice)?;
            put(vm, recv, "mCharSequence", value)?;
            put(vm, recv, "mCookie", Value::Int(iarg(args, 3)))?;
            put(vm, recv, "mSequenceNumber", Value::Int(iarg(args, 4)))?;
            Value::Null
        }
        ("getText", "()Ljava/lang/String;") => {
            let value = get(vm, recv, "mCharSequence");
            match charseq(vm, Some(&value))? {
                Some(s) => jstr(vm, s)?,
                None => Value::Null,
            }
        }
        ("getCharSequence", "()Ljava/lang/CharSequence;") => get(vm, recv, "mCharSequence"),
        ("getCookie", "()I") => Value::Int(field_int(vm, recv, "mCookie")),
        ("getSequence", "()I") => Value::Int(field_int(vm, recv, "mSequenceNumber")),
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_suggestions_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(I[Ljava/lang/String;)V") | ("<init>", "(I[Ljava/lang/String;II)V") => {
            let available = oarg(args, 1).is_some();
            let suggestions = if available {
                args.get(1).cloned().unwrap_or(Value::Null)
            } else {
                obj_array(vm, "[Ljava/lang/String;", Vec::new())?
            };
            put(
                vm,
                recv,
                "mSuggestionsAttributes",
                Value::Int(iarg(args, 0)),
            )?;
            put(vm, recv, "mSuggestions", suggestions)?;
            put(vm, recv, "mSuggestionsAvailable", bool_value(available))?;
            put(vm, recv, "mCookie", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mSequence", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        ("setCookieAndSequence", "(II)V") => {
            put(vm, recv, "mCookie", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mSequence", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("getCookie", "()I") => Value::Int(field_int(vm, recv, "mCookie")),
        ("getSequence", "()I") => Value::Int(field_int(vm, recv, "mSequence")),
        ("getSuggestionsAttributes", "()I") => {
            Value::Int(field_int(vm, recv, "mSuggestionsAttributes"))
        }
        ("getSuggestionsCount", "()I") => {
            if !field_bool(vm, recv, "mSuggestionsAvailable") {
                Value::Int(-1)
            } else {
                let arr = get(vm, recv, "mSuggestions");
                Value::Int(array_elems(vm, Some(&arr)).len() as i32)
            }
        }
        ("getSuggestionAt", "(I)Ljava/lang/String;") => {
            let arr = get(vm, recv, "mSuggestions");
            let elems = array_elems(vm, Some(&arr));
            let index = iarg(args, 0);
            if index < 0 || index as usize >= elems.len() {
                return Err(oob(format!(
                    "Index {index} out of bounds for length {}",
                    elems.len()
                )));
            }
            elems[index as usize].clone()
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_sentence_suggestions_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "([Landroid/view/textservice/SuggestionsInfo;[I[I)V") => {
            let Some(infos) = args.first().filter(|_| oarg(args, 0).is_some()) else {
                return Err(npe("suggestionsInfos"));
            };
            let Some(offsets) = args.get(1).filter(|_| oarg(args, 1).is_some()) else {
                return Err(npe("offsets"));
            };
            let Some(lengths) = args.get(2).filter(|_| oarg(args, 2).is_some()) else {
                return Err(npe("lengths"));
            };
            let infos = infos.clone();
            let offsets = offsets.clone();
            let lengths = lengths.clone();
            let n = array_elems(vm, Some(&infos)).len();
            if array_elems(vm, Some(&offsets)).len() != n
                || array_elems(vm, Some(&lengths)).len() != n
            {
                return Err(iae("array lengths differ"));
            }
            let infos = p57_clone_array(
                vm,
                &infos,
                "[Landroid/view/textservice/SuggestionsInfo;",
                ElemKind::Obj,
            )?;
            let offsets = p57_clone_array(vm, &offsets, "[I", ElemKind::Int)?;
            let lengths = p57_clone_array(vm, &lengths, "[I", ElemKind::Int)?;
            put(vm, recv, "mSuggestionsInfos", infos)?;
            put(vm, recv, "mOffsets", offsets)?;
            put(vm, recv, "mLengths", lengths)?;
            Value::Null
        }
        ("getSuggestionsCount", "()I") => {
            let arr = get(vm, recv, "mSuggestionsInfos");
            Value::Int(array_elems(vm, Some(&arr)).len() as i32)
        }
        ("getSuggestionsInfoAt", "(I)Landroid/view/textservice/SuggestionsInfo;") => {
            let arr = get(vm, recv, "mSuggestionsInfos");
            let elems = array_elems(vm, Some(&arr));
            let i = iarg(args, 0);
            if i < 0 || i as usize >= elems.len() {
                Value::Null
            } else {
                elems[i as usize].clone()
            }
        }
        ("getOffsetAt", "(I)I") => {
            let values = int_values(vm, Some(&get(vm, recv, "mOffsets")));
            let i = iarg(args, 0);
            Value::Int(if i < 0 || i as usize >= values.len() {
                -1
            } else {
                values[i as usize]
            })
        }
        ("getLengthAt", "(I)I") => {
            let values = int_values(vm, Some(&get(vm, recv, "mLengths")));
            let i = iarg(args, 0);
            Value::Int(if i < 0 || i as usize >= values.len() {
                -1
            } else {
                values[i as usize]
            })
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_spell_subtype_hash(locale: &str, extra: &str) -> i32 {
    let mut result = 1i32;
    result = result
        .wrapping_mul(31)
        .wrapping_add(java_string_hash(locale));
    result = result
        .wrapping_mul(31)
        .wrapping_add(java_string_hash(extra));
    result
}

fn p57_spell_subtype_extra_map(extra: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for pair in extra.split(',') {
        let mut parts = pair.split('=');
        let Some(key) = parts.next() else { continue };
        let value = parts.next().map(str::to_string);
        if let Some(existing) = out.iter_mut().find(|(k, _)| k == key) {
            existing.1 = value;
        } else {
            out.push((key.to_string(), value));
        }
    }
    out
}

fn p57_spell_subtype_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(ILjava/lang/String;Ljava/lang/String;)V") => {
            let locale = opt_str_arg(vm, args, 1).unwrap_or_default();
            let extra = opt_str_arg(vm, args, 2).unwrap_or_default();
            let lv = jstr(vm, &locale)?;
            let ev = jstr(vm, &extra)?;
            let empty = jstr(vm, "")?;
            put(vm, recv, "mSubtypeNameResId", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mSubtypeLocale", lv)?;
            put(vm, recv, "mSubtypeLanguageTag", empty)?;
            put(vm, recv, "mSubtypeExtraValue", ev)?;
            put(vm, recv, "mSubtypeId", Value::Int(0))?;
            put(
                vm,
                recv,
                "mSubtypeHashCode",
                Value::Int(p57_spell_subtype_hash(&locale, &extra)),
            )?;
            Value::Null
        }
        ("<init>", "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V") => {
            let locale = opt_str_arg(vm, args, 1).unwrap_or_default();
            let tag = opt_str_arg(vm, args, 2).unwrap_or_default();
            let extra = opt_str_arg(vm, args, 3).unwrap_or_default();
            let subtype_id = iarg(args, 4);
            let lv = jstr(vm, &locale)?;
            let tv = jstr(vm, tag)?;
            let ev = jstr(vm, &extra)?;
            let hash = if subtype_id != 0 {
                subtype_id
            } else {
                p57_spell_subtype_hash(&locale, &extra)
            };
            put(vm, recv, "mSubtypeNameResId", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mSubtypeLocale", lv)?;
            put(vm, recv, "mSubtypeLanguageTag", tv)?;
            put(vm, recv, "mSubtypeExtraValue", ev)?;
            put(vm, recv, "mSubtypeId", Value::Int(subtype_id))?;
            put(vm, recv, "mSubtypeHashCode", Value::Int(hash))?;
            Value::Null
        }
        ("getNameResId", "()I") => Value::Int(field_int(vm, recv, "mSubtypeNameResId")),
        ("getLocale", "()Ljava/lang/String;") => jstr(
            vm,
            field_str(vm, recv, "mSubtypeLocale").unwrap_or_default(),
        )?,
        ("getLanguageTag", "()Ljava/lang/String;") => jstr(
            vm,
            field_str(vm, recv, "mSubtypeLanguageTag").unwrap_or_default(),
        )?,
        ("getExtraValue", "()Ljava/lang/String;") => jstr(
            vm,
            field_str(vm, recv, "mSubtypeExtraValue").unwrap_or_default(),
        )?,
        ("containsExtraValueKey", "(Ljava/lang/String;)Z") => {
            let key = opt_str_arg(vm, args, 0);
            let extra = field_str(vm, recv, "mSubtypeExtraValue").unwrap_or_default();
            let found = key
                .as_ref()
                .map(|k| {
                    p57_spell_subtype_extra_map(&extra)
                        .iter()
                        .any(|(ek, _)| ek == k)
                })
                .unwrap_or(false);
            bool_value(found)
        }
        ("getExtraValueOf", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let key = opt_str_arg(vm, args, 0).unwrap_or_default();
            let extra = field_str(vm, recv, "mSubtypeExtraValue").unwrap_or_default();
            let value = p57_spell_subtype_extra_map(&extra)
                .into_iter()
                .find(|(k, _)| *k == key)
                .and_then(|(_, v)| v);
            p57_opt_string(vm, value)?
        }
        ("getLocaleObject", "()Ljava/util/Locale;") => {
            let tag = field_str(vm, recv, "mSubtypeLanguageTag").unwrap_or_default();
            let locale = if !tag.is_empty() {
                tag
            } else {
                field_str(vm, recv, "mSubtypeLocale").unwrap_or_default()
            };
            if locale.is_empty() {
                Value::Null
            } else {
                // java.util.Locale is already hosted; construct through its
                // one-argument constructor with an AOSP locale string form.
                let r = alloc_obj(vm, "Ljava/util/Locale;", Vec::new())?;
                let s = jstr(vm, locale)?;
                vm.invoke_instance(r, "<init>", "(Ljava/lang/String;)V", &[s])?;
                Value::Obj(r)
            }
        }
        ("hashCode", "()I") => Value::Int(field_int(vm, recv, "mSubtypeHashCode")),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_SPELL_SUBTYPE => {
                    let my_id = field_int(vm, recv, "mSubtypeId");
                    let other_id = field_int(vm, other, "mSubtypeId");
                    if my_id != 0 || other_id != 0 {
                        field_int(vm, recv, "mSubtypeHashCode")
                            == field_int(vm, other, "mSubtypeHashCode")
                    } else {
                        field_int(vm, recv, "mSubtypeHashCode")
                            == field_int(vm, other, "mSubtypeHashCode")
                            && field_int(vm, recv, "mSubtypeNameResId")
                                == field_int(vm, other, "mSubtypeNameResId")
                            && field_str(vm, recv, "mSubtypeLocale")
                                == field_str(vm, other, "mSubtypeLocale")
                            && field_str(vm, recv, "mSubtypeLanguageTag")
                                == field_str(vm, other, "mSubtypeLanguageTag")
                            && field_str(vm, recv, "mSubtypeExtraValue")
                                == field_str(vm, other, "mSubtypeExtraValue")
                    }
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_spell_params_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == P57_SPELL_PARAMS_BUILDER {
        return Ok(Some(match (name, sig) {
            ("<init>", "()V") => {
                put(vm, recv, "mLocale", Value::Null)?;
                put(
                    vm,
                    recv,
                    "mShouldReferToSpellCheckerLanguageSettings",
                    Value::Int(0),
                )?;
                put(vm, recv, "mSupportedAttributes", Value::Int(0))?;
                let extras = empty_bundle(vm)?;
                put(vm, recv, "mExtras", extras)?;
                Value::Null
            }
            ("setLocale", "(Ljava/util/Locale;)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;") => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                put(vm, recv, "mLocale", v)?;
                Value::Obj(recv)
            }
            ("setShouldReferToSpellCheckerLanguageSettings", "(Z)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;") => {
                put(
                    vm,
                    recv,
                    "mShouldReferToSpellCheckerLanguageSettings",
                    Value::Int(iarg(args, 0)),
                )?;
                Value::Obj(recv)
            }
            ("setSupportedAttributes", "(I)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;") => {
                put(vm, recv, "mSupportedAttributes", Value::Int(iarg(args, 0)))?;
                Value::Obj(recv)
            }
            ("setExtras", "(Landroid/os/Bundle;)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;") => {
                let v = match args.first() {
                    Some(Value::Obj(_)) => args.first().cloned().unwrap(),
                    _ => return Err(npe("extras")),
                };
                put(vm, recv, "mExtras", v)?;
                Value::Obj(recv)
            }
            ("build", "()Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;") => {
                if field_obj(vm, recv, "mLocale").is_none()
                    && !field_bool(vm, recv, "mShouldReferToSpellCheckerLanguageSettings")
                {
                    return Err(iae(
                        "mLocale should not be null if  mShouldReferToSpellCheckerLanguageSettings is false.",
                    ));
                }
                let out = alloc_obj(vm, P57_SPELL_PARAMS, Vec::new())?;
                for f in [
                    "mLocale",
                    "mShouldReferToSpellCheckerLanguageSettings",
                    "mSupportedAttributes",
                    "mExtras",
                ] {
                    let v = get(vm, recv, f);
                    put(vm, out, f, v)?;
                }
                Value::Obj(out)
            }
            _ => return Ok(None),
        }));
    }
    Ok(Some(match (name, sig) {
        ("getLocale", "()Ljava/util/Locale;") => get(vm, recv, "mLocale"),
        ("shouldReferToSpellCheckerLanguageSettings", "()Z") => bool_value(field_bool(
            vm,
            recv,
            "mShouldReferToSpellCheckerLanguageSettings",
        )),
        ("getSupportedAttributes", "()I") => {
            Value::Int(field_int(vm, recv, "mSupportedAttributes"))
        }
        ("getExtras", "()Landroid/os/Bundle;") => {
            let v = get(vm, recv, "mExtras");
            if matches!(v, Value::Obj(_)) {
                v
            } else {
                empty_bundle(vm)?
            }
        }
        _ => return Ok(None),
    }))
}

fn p57_text_services_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // The host profile intentionally has no installed/enabled spell checker.
    // Manager objects obtained from Context have no constructor-run fields.
    let _ = args;
    Ok(Some(match (name, sig) {
        ("newSpellCheckerSession", "(Landroid/os/Bundle;Ljava/util/Locale;Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;Z)Landroid/view/textservice/SpellCheckerSession;") => {
            if oarg(args, 2).is_none() {
                return Err(npe("listener"));
            }
            Value::Null
        }
        ("newSpellCheckerSession", "(Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;Ljava/util/concurrent/Executor;Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;)Landroid/view/textservice/SpellCheckerSession;") => {
            let Some(params) = oarg(args, 0) else {
                return Err(npe("params"));
            };
            if oarg(args, 1).is_none() {
                return Err(npe("executor"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("listener"));
            }
            if !field_bool(vm, params, "mShouldReferToSpellCheckerLanguageSettings")
                && field_obj(vm, params, "mLocale").is_none()
            {
                return Err(iae(
                    "Locale should not be null if you don't refer settings.",
                ));
            }
            Value::Null
        }
        ("getEnabledSpellCheckers", "()[Landroid/view/textservice/SpellCheckerInfo;") => {
            obj_array(
                vm,
                "[Landroid/view/textservice/SpellCheckerInfo;",
                Vec::new(),
            )?
        }
        ("getEnabledSpellCheckerInfos", "()Ljava/util/List;") => {
            Value::Obj(new_collection(vm, "Ljava/util/ArrayList;", Vec::new())?)
        }
        ("getCurrentSpellCheckerInfo", "()Landroid/view/textservice/SpellCheckerInfo;")
        | ("getCurrentSpellChecker", "()Landroid/view/textservice/SpellCheckerInfo;")
        | ("getCurrentSpellCheckerSubtype", "(Z)Landroid/view/textservice/SpellCheckerSubtype;") => {
            Value::Null
        }
        ("isSpellCheckerEnabled", "()Z") => Value::Int(0),
        ("getInputMethodManager", "()Landroid/view/inputmethod/InputMethodManager;") => {
            let _ = recv;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

fn p57_spell_session_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/view/textservice/SpellCheckerInfo;Landroid/view/textservice/TextServicesManager;Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;Ljava/util/concurrent/Executor;)V") => {
            if oarg(args, 0).is_none() || oarg(args, 1).is_none() || oarg(args, 2).is_none() {
                return Err(npe("SpellCheckerSession arguments"));
            }
            let info = args.first().cloned().unwrap();
            let manager = args.get(1).cloned().unwrap();
            let listener = args.get(2).cloned().unwrap();
            let executor = args.get(3).cloned().unwrap_or(Value::Null);
            put(vm, recv, "mSpellCheckerInfo", info)?;
            put(vm, recv, "mTextServicesManager", manager)?;
            put(vm, recv, "mSpellCheckerSessionListener", listener)?;
            put(vm, recv, "mExecutor", executor)?;
            put(vm, recv, "rd_closed", Value::Int(0))?;
            Value::Null
        }
        ("isSessionDisconnected", "()Z") => Value::Int(1),
        ("getSpellChecker", "()Landroid/view/textservice/SpellCheckerInfo;") => {
            get(vm, recv, "mSpellCheckerInfo")
        }
        ("cancel", "()V") => Value::Null,
        ("close", "()V") => {
            put(vm, recv, "rd_closed", Value::Int(1))?;
            Value::Null
        }
        // A manager in this profile never returns a live session; a directly
        // constructed session remains in AOSP's wait-for-connection state and
        // queues work rather than fabricating suggestions.
        ("getSentenceSuggestions", "([Landroid/view/textservice/TextInfo;I)V")
        | ("getSuggestions", "(Landroid/view/textservice/TextInfo;I)V")
        | ("getSuggestions", "([Landroid/view/textservice/TextInfo;IZ)V") => Value::Null,
        _ => return Ok(None),
    }))
}

// --- android.view.translation ---------------------------------------------

fn p57_ulocale_language(vm: &mut Engine, locale: ObjRef) -> Result<String, VmExit> {
    if let Some(lang) = field_str(vm, locale, "rd_lang") {
        return Ok(lang);
    }
    let tag = ulocale_tag(vm, locale)?;
    Ok(tag.split(['-', '_']).next().unwrap_or_default().to_string())
}

fn p57_translation_spec_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;I)V") => {
            let Some(language) = opt_str_arg(vm, args, 0) else {
                return Err(npe("language"));
            };
            // AOSP's ULocale.Builder accepts this as its language subtag.
            // Keep the deprecated language field verbatim.
            let locale = ulocale_from_tag(vm, &language)?;
            let lang = jstr(vm, language)?;
            put(vm, recv, "mLanguage", lang)?;
            put(vm, recv, "mLocale", locale)?;
            put(vm, recv, "mDataFormat", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("<init>", "(Landroid/icu/util/ULocale;I)V") => {
            let Some(locale) = oarg(args, 0) else {
                return Err(npe("locale"));
            };
            let language = p57_ulocale_language(vm, locale)?;
            let lang = jstr(vm, language)?;
            let locale_value = args.first().cloned().unwrap();
            put(vm, recv, "mLanguage", lang)?;
            put(vm, recv, "mLocale", locale_value)?;
            put(vm, recv, "mDataFormat", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("getLanguage", "()Ljava/lang/String;") => {
            p57_opt_string(vm, field_str(vm, recv, "mLanguage"))?
        }
        ("getLocale", "()Landroid/icu/util/ULocale;") => get(vm, recv, "mLocale"),
        ("getDataFormat", "()I") => Value::Int(field_int(vm, recv, "mDataFormat")),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_TRANSLATION_SPEC => {
                    if other == recv {
                        true
                    } else {
                        let my_locale = field_obj(vm, recv, "mLocale");
                        let other_locale = field_obj(vm, other, "mLocale");
                        let locale_eq = match (my_locale, other_locale) {
                            (Some(a), Some(b)) => ulocale_tag(vm, a)? == ulocale_tag(vm, b)?,
                            (None, None) => true,
                            _ => false,
                        };
                        field_str(vm, recv, "mLanguage") == field_str(vm, other, "mLanguage")
                            && locale_eq
                            && field_int(vm, recv, "mDataFormat")
                                == field_int(vm, other, "mDataFormat")
                    }
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => {
            let language_hash = field_str(vm, recv, "mLanguage")
                .map(|s| java_string_hash(&s))
                .unwrap_or(0);
            let locale_hash = match field_obj(vm, recv, "mLocale") {
                Some(locale) => java_string_hash(&ulocale_tag(vm, locale)?),
                None => 0,
            };
            let mut hash = 1i32;
            hash = hash.wrapping_mul(31).wrapping_add(language_hash);
            hash = hash.wrapping_mul(31).wrapping_add(locale_hash);
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mDataFormat"));
            Value::Int(hash)
        }
        ("toString", "()Ljava/lang/String;") => {
            let language = field_str(vm, recv, "mLanguage").unwrap_or_else(|| "null".into());
            let locale = match field_obj(vm, recv, "mLocale") {
                Some(r) => crate::intrinsics::object_to_string(vm, r)?,
                None => "null".into(),
            };
            jstr(
                vm,
                format!(
                    "TranslationSpec {{ language = {language}, locale = {locale}, dataFormat = {} }}",
                    field_int(vm, recv, "mDataFormat")
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_translation_request_value_hash(vm: &Engine, recv: ObjRef) -> Result<i32, VmExit> {
    let text = get(vm, recv, "mText");
    let hash = match text {
        Value::Obj(r) => match vm.heap.as_str(r) {
            Ok(s) => java_string_hash(s),
            Err(_) => identity_hash(r),
        },
        _ => 0,
    };
    Ok(31i32.wrapping_add(hash))
}

fn p57_translation_request_value_equal(
    vm: &mut Engine,
    a: &Value,
    b: &Value,
) -> Result<bool, VmExit> {
    let (Some(a), Some(b)) = (obj(Some(a)), obj(Some(b))) else {
        return Ok(matches!((a, b), (Value::Null, Value::Null)));
    };
    if a == b {
        return Ok(true);
    }
    if class_of(vm, a) != P57_TRANSLATION_REQUEST_VALUE
        || class_of(vm, b) != P57_TRANSLATION_REQUEST_VALUE
    {
        return Ok(false);
    }
    let av = get(vm, a, "mText");
    let bv = get(vm, b, "mText");
    Ok(charseq(vm, Some(&av))? == charseq(vm, Some(&bv))?)
}

fn p57_translation_request_value_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/CharSequence;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mText", v)?;
            Value::Null
        }
        ("getText", "()Ljava/lang/CharSequence;") => get(vm, recv, "mText"),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let other = args.first().cloned().unwrap_or(Value::Null);
            bool_value(p57_translation_request_value_equal(
                vm,
                &Value::Obj(recv),
                &other,
            )?)
        }
        ("hashCode", "()I") => Value::Int(p57_translation_request_value_hash(vm, recv)?),
        ("toString", "()Ljava/lang/String;") => {
            let v = get(vm, recv, "mText");
            let text = value_string(vm, &v)?;
            jstr(vm, format!("TranslationRequestValue {{ text = {text} }}"))?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_view_translation_map_hash(
    vm: &mut Engine,
    entries: &[(Value, Value)],
) -> Result<i32, VmExit> {
    let mut hash = 0i32;
    for (k, v) in entries {
        let kh = match k {
            Value::Obj(r) => vm
                .heap
                .as_str(*r)
                .map(java_string_hash)
                .unwrap_or_else(|_| identity_hash(*r)),
            _ => 0,
        };
        let vh = match v {
            Value::Obj(r) => p57_translation_request_value_hash(vm, *r)?,
            _ => 0,
        };
        hash = hash.wrapping_add(kh ^ vh);
    }
    Ok(hash)
}

fn p57_view_translation_request_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/view/autofill/AutofillId;Ljava/util/Map;)V") => {
            let Some(id) = oarg(args, 0) else {
                return Err(npe("autofillId"));
            };
            let Some(map) = oarg(args, 1) else {
                return Err(npe("translationRequestValues"));
            };
            let _ = id;
            let _ = map;
            let idv = args.first().cloned().unwrap();
            let mapv = args.get(1).cloned().unwrap();
            put(vm, recv, "mAutofillId", idv)?;
            put(vm, recv, "mTranslationRequestValues", mapv)?;
            Value::Null
        }
        ("getValue", "(Ljava/lang/String;)Landroid/view/translation/TranslationRequestValue;") => {
            let Some(key) = opt_str_arg(vm, args, 0) else {
                return Err(npe("key should not be null"));
            };
            let entries = p57_map_entries(vm, recv, "mTranslationRequestValues");
            let mut found = None;
            for (k, v) in entries {
                if opt_str(vm, Some(&k)).as_deref() == Some(key.as_str()) {
                    found = Some(v);
                    break;
                }
            }
            match found {
                Some(v) => v,
                None => return Err(iae(format!("Request does not contain value for key={key}"))),
            }
        }
        ("getKeys", "()Ljava/util/Set;") => {
            let entries = p57_map_entries(vm, recv, "mTranslationRequestValues");
            let keys: Vec<Value> = entries.into_iter().map(|(k, _)| k).collect();
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", keys)?)
        }
        ("getAutofillId", "()Landroid/view/autofill/AutofillId;") => get(vm, recv, "mAutofillId"),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_VIEW_TRANSLATION_REQUEST => {
                    if other == recv {
                        true
                    } else {
                        let my_id = get(vm, recv, "mAutofillId");
                        let other_id = get(vm, other, "mAutofillId");
                        let id_eq = match (obj(Some(&my_id)), obj(Some(&other_id))) {
                            (Some(a), Some(b)) => {
                                a == b
                                    || vm
                                        .invoke_instance(
                                            a,
                                            "equals",
                                            "(Ljava/lang/Object;)Z",
                                            &[Value::Obj(b)],
                                        )
                                        .map(|v| v == Value::Int(1))?
                            }
                            _ => my_id == other_id,
                        };
                        if !id_eq {
                            false
                        } else {
                            let mine = p57_map_entries(vm, recv, "mTranslationRequestValues");
                            let theirs = p57_map_entries(vm, other, "mTranslationRequestValues");
                            if mine.len() != theirs.len() {
                                false
                            } else {
                                let mut all = true;
                                for (k, v) in &mine {
                                    let key = opt_str(vm, Some(k));
                                    let found =
                                        theirs.iter().find(|(ok, _)| opt_str(vm, Some(ok)) == key);
                                    match found {
                                        Some((_, ov))
                                            if p57_translation_request_value_equal(vm, v, ov)? => {}
                                        _ => {
                                            all = false;
                                            break;
                                        }
                                    }
                                }
                                all
                            }
                        }
                    }
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => {
            let id_hash = match field_obj(vm, recv, "mAutofillId") {
                Some(id) => vm
                    .invoke_instance(id, "hashCode", "()I", &[])
                    .map(|v| v.as_int().unwrap_or(0))?,
                None => 0,
            };
            let entries = p57_map_entries(vm, recv, "mTranslationRequestValues");
            let map_hash = p57_view_translation_map_hash(vm, &entries)?;
            let mut hash = 1i32;
            hash = hash.wrapping_mul(31).wrapping_add(id_hash);
            hash = hash.wrapping_mul(31).wrapping_add(map_hash);
            Value::Int(hash)
        }
        ("toString", "()Ljava/lang/String;") => {
            let id = get(vm, recv, "mAutofillId");
            let id_string = value_string(vm, &id)?;
            let entries = p57_map_entries(vm, recv, "mTranslationRequestValues");
            let mut parts = Vec::new();
            for (k, v) in entries {
                parts.push(format!(
                    "{}={}",
                    value_string(vm, &k)?,
                    value_string(vm, &v)?
                ));
            }
            jstr(
                vm,
                format!(
                    "ViewTranslationRequest {{ autofillId = {id_string}, translationRequestValues = {{{}}} }}",
                    parts.join(", ")
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_view_translation_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/view/autofill/AutofillId;)V") => {
            let Some(_) = oarg(args, 0) else {
                return Err(npe("autofillId"));
            };
            let v = args.first().cloned().unwrap();
            put(vm, recv, "mAutofillId", v)?;
            put(vm, recv, "rd_used", Value::Int(0))?;
            Value::Null
        }
        ("<init>", "(Landroid/view/autofill/AutofillId;J)V") => {
            let Some(host) = oarg(args, 0) else {
                return Err(npe("autofillId"));
            };
            let child = alloc_obj(vm, AUTOFILL_ID, Vec::new())?;
            vm.invoke_instance(
                child,
                "<init>",
                "(Landroid/view/autofill/AutofillId;JI)V",
                &[
                    Value::Obj(host),
                    Value::Long(larg(args, 1)),
                    Value::Int(0),
                ],
            )?;
            put(vm, recv, "mAutofillId", Value::Obj(child))?;
            put(vm, recv, "rd_used", Value::Int(0))?;
            Value::Null
        }
        ("setValue", "(Ljava/lang/String;Landroid/view/translation/TranslationRequestValue;)Landroid/view/translation/ViewTranslationRequest$Builder;") => {
            p57_check_builder_unused(vm, recv)?;
            let Some(key) = opt_str_arg(vm, args, 0) else {
                return Err(npe("key"));
            };
            let Some(_) = oarg(args, 1) else {
                return Err(npe("value"));
            };
            let v = args.get(1).cloned().unwrap();
            p57_map_put_string(vm, recv, "mTranslationRequestValues", key, v)?;
            Value::Obj(recv)
        }
        ("build", "()Landroid/view/translation/ViewTranslationRequest;") => {
            p57_check_builder_unused(vm, recv)?;
            put(vm, recv, "rd_used", Value::Int(1))?;
            let map = p57_ensure_map(vm, recv, "mTranslationRequestValues")?;
            let out = alloc_obj(vm, P57_VIEW_TRANSLATION_REQUEST, Vec::new())?;
            let id = get(vm, recv, "mAutofillId");
            put(vm, out, "mAutofillId", id)?;
            put(
                vm,
                out,
                "mTranslationRequestValues",
                Value::Obj(map),
            )?;
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

fn p57_translation_flag_string(flags: i32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if flags & 1 != 0 {
        parts.push("FLAG_LOW_LATENCY".to_string());
    }
    if flags & 2 != 0 {
        parts.push("FLAG_TRANSLITERATION".to_string());
    }
    if flags & 4 != 0 {
        parts.push("FLAG_DEFINITIONS".to_string());
    }
    let unknown = flags & !7;
    if unknown != 0 {
        parts.push(format!("{unknown:x}"));
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.join("|")
    }
}

fn p57_translation_context_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == P57_TRANSLATION_CONTEXT_BUILDER {
        return Ok(Some(match (name, sig) {
            ("<init>", "(Landroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;)V") => {
                if oarg(args, 0).is_none() {
                    return Err(npe("sourceSpec"));
                }
                if oarg(args, 1).is_none() {
                    return Err(npe("targetSpec"));
                }
                let a = args.first().cloned().unwrap();
                let b = args.get(1).cloned().unwrap();
                put(vm, recv, "mSourceSpec", a)?;
                put(vm, recv, "mTargetSpec", b)?;
                put(vm, recv, "mTranslationFlags", Value::Int(0))?;
                put(vm, recv, "mActivityId", Value::Null)?;
                put(vm, recv, "rd_used", Value::Int(0))?;
                Value::Null
            }
            ("setTranslationFlags", "(I)Landroid/view/translation/TranslationContext$Builder;") => {
                p57_check_builder_unused(vm, recv)?;
                put(vm, recv, "mTranslationFlags", Value::Int(iarg(args, 0)))?;
                Value::Obj(recv)
            }
            ("setActivityId", "(Landroid/app/assist/ActivityId;)Landroid/view/translation/TranslationContext$Builder;") => {
                p57_check_builder_unused(vm, recv)?;
                let Some(_) = oarg(args, 0) else {
                    return Err(npe("activityId"));
                };
                let v = args.first().cloned().unwrap();
                put(vm, recv, "mActivityId", v)?;
                Value::Obj(recv)
            }
            ("build", "()Landroid/view/translation/TranslationContext;") => {
                p57_check_builder_unused(vm, recv)?;
                let flags = field_int(vm, recv, "mTranslationFlags");
                if flags & !7 != 0 {
                    return Err(iae(format!("translation flags invalid: {flags}")));
                }
                put(vm, recv, "rd_used", Value::Int(1))?;
                let out = alloc_obj(vm, P57_TRANSLATION_CONTEXT, Vec::new())?;
                for f in [
                    "mSourceSpec",
                    "mTargetSpec",
                    "mTranslationFlags",
                    "mActivityId",
                ] {
                    let v = get(vm, recv, f);
                    put(vm, out, f, v)?;
                }
                Value::Obj(out)
            }
            _ => return Ok(None),
        }));
    }
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;ILandroid/app/assist/ActivityId;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("sourceSpec"));
            }
            if oarg(args, 1).is_none() {
                return Err(npe("targetSpec"));
            }
            let flags = iarg(args, 2);
            if flags & !7 != 0 {
                return Err(iae(format!("translation flags invalid: {flags}")));
            }
            let a = args.first().cloned().unwrap();
            let b = args.get(1).cloned().unwrap();
            let activity = args.get(3).cloned().unwrap_or(Value::Null);
            put(vm, recv, "mSourceSpec", a)?;
            put(vm, recv, "mTargetSpec", b)?;
            put(vm, recv, "mTranslationFlags", Value::Int(flags))?;
            put(vm, recv, "mActivityId", activity)?;
            Value::Null
        }
        ("getSourceSpec", "()Landroid/view/translation/TranslationSpec;") => {
            get(vm, recv, "mSourceSpec")
        }
        ("getTargetSpec", "()Landroid/view/translation/TranslationSpec;") => {
            get(vm, recv, "mTargetSpec")
        }
        ("getTranslationFlags", "()I") => {
            Value::Int(field_int(vm, recv, "mTranslationFlags"))
        }
        ("getActivityId", "()Landroid/app/assist/ActivityId;") => get(vm, recv, "mActivityId"),
        ("toString", "()Ljava/lang/String;") => {
            let source = get(vm, recv, "mSourceSpec");
            let target = get(vm, recv, "mTargetSpec");
            let activity = get(vm, recv, "mActivityId");
            let source_s = value_string(vm, &source)?;
            let target_s = value_string(vm, &target)?;
            let activity_s = value_string(vm, &activity)?;
            jstr(
                vm,
                format!(
                    "TranslationContext {{ sourceSpec = {source_s}, targetSpec = {target_s}, translationFlags = {}, activityId = {activity_s} }}",
                    p57_translation_flag_string(field_int(vm, recv, "mTranslationFlags"))
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_translation_capability_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(ILandroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;ZI)V") => {
            if oarg(args, 1).is_none() {
                return Err(npe("sourceSpec should not be null"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("targetSpec should not be null"));
            }
            let source = args.get(1).cloned().unwrap();
            let target = args.get(2).cloned().unwrap();
            put(vm, recv, "mState", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mSourceSpec", source)?;
            put(vm, recv, "mTargetSpec", target)?;
            put(vm, recv, "mUiTranslationEnabled", Value::Int(iarg(args, 3)))?;
            put(
                vm,
                recv,
                "mSupportedTranslationFlags",
                Value::Int(iarg(args, 4)),
            )?;
            Value::Null
        }
        ("getState", "()I") => Value::Int(field_int(vm, recv, "mState")),
        ("getSourceSpec", "()Landroid/view/translation/TranslationSpec;") => {
            get(vm, recv, "mSourceSpec")
        }
        ("getTargetSpec", "()Landroid/view/translation/TranslationSpec;") => {
            get(vm, recv, "mTargetSpec")
        }
        ("isUiTranslationEnabled", "()Z") => {
            bool_value(field_bool(vm, recv, "mUiTranslationEnabled"))
        }
        ("getSupportedTranslationFlags", "()I") => {
            Value::Int(field_int(vm, recv, "mSupportedTranslationFlags"))
        }
        ("toString", "()Ljava/lang/String;") => {
            let source = get(vm, recv, "mSourceSpec");
            let target = get(vm, recv, "mTargetSpec");
            let source_s = value_string(vm, &source)?;
            let target_s = value_string(vm, &target)?;
            jstr(
                vm,
                format!(
                    "TranslationCapability {{ state = {}, sourceSpec = {source_s}, targetSpec = {target_s}, uiTranslationEnabled = {}, supportedTranslationFlags = {} }}",
                    p57_translation_model_state(field_int(vm, recv, "mState")),
                    field_bool(vm, recv, "mUiTranslationEnabled"),
                    field_int(vm, recv, "mSupportedTranslationFlags")
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_translation_model_state(state: i32) -> String {
    match state {
        1 => "STATE_AVAILABLE_TO_DOWNLOAD".into(),
        2 => "STATE_DOWNLOADING".into(),
        3 => "STATE_ON_DEVICE".into(),
        4 => "STATE_NOT_AVAILABLE".into(),
        1000 => "STATE_REMOVED_AND_AVAILABLE".into(),
        _ => format!("{:x}", state as u32),
    }
}

fn p57_translation_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // No on-device translation service is installed in this profile.
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/Context;Landroid/view/translation/ITranslationManager;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("context cannot be null"));
            }
            let v = args.first().cloned().unwrap();
            put(vm, recv, "mContext", v)?;
            Value::Null
        }
        ("createOnDeviceTranslator", "(Landroid/view/translation/TranslationContext;Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("translationContext cannot be null"));
            }
            if oarg(args, 1).is_none() {
                return Err(npe("executor cannot be null"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("callback cannot be null"));
            }
            // No service exists to create a session. The synchronous API's
            // documented null result is modeled below; async callback delivery
            // through a guest Executor remains outside this partial slice.
            Value::Null
        }
        ("createOnDeviceTranslator", "(Landroid/view/translation/TranslationContext;)Landroid/view/translation/Translator;")
        | ("createTranslator", "(Landroid/view/translation/TranslationContext;)Landroid/view/translation/Translator;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("translationContext cannot be null"));
            }
            Value::Null
        }
        ("getOnDeviceTranslationCapabilities", "(II)Ljava/util/Set;")
        | ("getTranslationCapabilities", "(II)Ljava/util/Set;") => {
            Value::Obj(new_collection(vm, "Ljava/util/HashSet;", Vec::new())?)
        }
        ("addOnDeviceTranslationCapabilityUpdateListener", "(Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V")
        | ("addTranslationCapabilityUpdateListener", "(Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("executor should not be null"));
            }
            if oarg(args, 1).is_none() {
                return Err(npe("capability listener should not be null"));
            }
            Value::Null
        }
        ("removeOnDeviceTranslationCapabilityUpdateListener", "(Ljava/util/function/Consumer;)V")
        | ("removeTranslationCapabilityUpdateListener", "(Ljava/util/function/Consumer;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("capability listener should not be null"));
            }
            Value::Null
        }
        ("getOnDeviceTranslationSettingsActivityIntent", "()Landroid/app/PendingIntent;")
        | ("getTranslationSettingsActivityIntent", "()Landroid/app/PendingIntent;") => {
            let _ = recv;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// --- android.hardware.usb ---------------------------------------------------

fn p57_usb_endpoint_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(IIII)V") => {
            put(vm, recv, "mAddress", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mAttributes", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "mMaxPacketSize", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mInterval", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        ("getAddress", "()I") => Value::Int(field_int(vm, recv, "mAddress")),
        ("getEndpointNumber", "()I") => Value::Int(field_int(vm, recv, "mAddress") & 15),
        ("getDirection", "()I") => Value::Int(field_int(vm, recv, "mAddress") & 128),
        ("getAttributes", "()I") => Value::Int(field_int(vm, recv, "mAttributes")),
        ("getType", "()I") => Value::Int(field_int(vm, recv, "mAttributes") & 3),
        ("getMaxPacketSize", "()I") => Value::Int(field_int(vm, recv, "mMaxPacketSize")),
        ("getInterval", "()I") => Value::Int(field_int(vm, recv, "mInterval")),
        ("toString", "()Ljava/lang/String;") => jstr(
            vm,
            format!(
                "UsbEndpoint[mAddress={},mAttributes={},mMaxPacketSize={},mInterval={}]",
                field_int(vm, recv, "mAddress"),
                field_int(vm, recv, "mAttributes"),
                field_int(vm, recv, "mMaxPacketSize"),
                field_int(vm, recv, "mInterval")
            ),
        )?,
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_usb_interface_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(IIILjava/lang/String;II[Landroid/hardware/usb/UsbEndpoint;)V") => {
            let name_v = args.get(3).cloned().unwrap_or(Value::Null);
            let Some(endpoints) = oarg(args, 6) else {
                return Err(npe("endpoints"));
            };
            let _ = endpoints;
            let endpoints = args.get(6).cloned().unwrap();
            let endpoints = p57_clone_array(
                vm,
                &endpoints,
                "[Landroid/hardware/usb/UsbEndpoint;",
                ElemKind::Obj,
            )?;
            put(vm, recv, "mId", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mAlternateSetting", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "mClass", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mName", name_v)?;
            put(vm, recv, "mSubclass", Value::Int(iarg(args, 4)))?;
            put(vm, recv, "mProtocol", Value::Int(iarg(args, 5)))?;
            put(vm, recv, "mEndpoints", endpoints)?;
            Value::Null
        }
        ("getId", "()I") => Value::Int(field_int(vm, recv, "mId")),
        ("getAlternateSetting", "()I") => Value::Int(field_int(vm, recv, "mAlternateSetting")),
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("getInterfaceClass", "()I") => Value::Int(field_int(vm, recv, "mClass")),
        ("getInterfaceSubclass", "()I") => Value::Int(field_int(vm, recv, "mSubclass")),
        ("getInterfaceProtocol", "()I") => Value::Int(field_int(vm, recv, "mProtocol")),
        ("getEndpointCount", "()I") => {
            let arr = get(vm, recv, "mEndpoints");
            Value::Int(array_elems(vm, Some(&arr)).len() as i32)
        }
        ("getEndpoint", "(I)Landroid/hardware/usb/UsbEndpoint;") => {
            let arr = get(vm, recv, "mEndpoints");
            let elems = array_elems(vm, Some(&arr));
            let i = iarg(args, 0);
            if i < 0 || i as usize >= elems.len() {
                return Err(oob(format!(
                    "Index {i} out of bounds for length {}",
                    elems.len()
                )));
            }
            elems[i as usize].clone()
        }
        ("toString", "()Ljava/lang/String;") => {
            let arr = get(vm, recv, "mEndpoints");
            let mut endpoint_text = String::new();
            for ep in array_elems(vm, Some(&arr)) {
                endpoint_text.push(' ');
                endpoint_text.push_str(&value_string(vm, &ep)?);
            }
            jstr(
                vm,
                format!(
                    "UsbInterface[mId={},mAlternateSetting={},mName={},mClass={},mSubclass={},mProtocol={},mEndpoints=[]{}]",
                    field_int(vm, recv, "mId"),
                    field_int(vm, recv, "mAlternateSetting"),
                    field_str(vm, recv, "mName").unwrap_or_else(|| "null".into()),
                    field_int(vm, recv, "mClass"),
                    field_int(vm, recv, "mSubclass"),
                    field_int(vm, recv, "mProtocol"),
                    endpoint_text
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_usb_configuration_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(ILjava/lang/String;II[Landroid/hardware/usb/UsbInterface;)V") => {
            let name_v = args.get(1).cloned().unwrap_or(Value::Null);
            let Some(_) = oarg(args, 4) else {
                return Err(npe("interfaces"));
            };
            let interfaces = args.get(4).cloned().unwrap();
            let interfaces = p57_clone_array(
                vm,
                &interfaces,
                "[Landroid/hardware/usb/UsbInterface;",
                ElemKind::Obj,
            )?;
            put(vm, recv, "mId", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mName", name_v)?;
            put(vm, recv, "mAttributes", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mMaxPower", Value::Int(iarg(args, 3)))?;
            put(vm, recv, "mInterfaces", interfaces)?;
            Value::Null
        }
        ("getId", "()I") => Value::Int(field_int(vm, recv, "mId")),
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("isSelfPowered", "()Z") => bool_value(field_int(vm, recv, "mAttributes") & 64 != 0),
        ("isRemoteWakeup", "()Z") => bool_value(field_int(vm, recv, "mAttributes") & 32 != 0),
        ("getMaxPower", "()I") => Value::Int(field_int(vm, recv, "mMaxPower") * 2),
        ("getInterfaceCount", "()I") => {
            let arr = get(vm, recv, "mInterfaces");
            Value::Int(array_elems(vm, Some(&arr)).len() as i32)
        }
        ("getInterface", "(I)Landroid/hardware/usb/UsbInterface;") => {
            let arr = get(vm, recv, "mInterfaces");
            let elems = array_elems(vm, Some(&arr));
            let i = iarg(args, 0);
            if i < 0 || i as usize >= elems.len() {
                return Err(oob(format!(
                    "Index {i} out of bounds for length {}",
                    elems.len()
                )));
            }
            elems[i as usize].clone()
        }
        ("toString", "()Ljava/lang/String;") => {
            let arr = get(vm, recv, "mInterfaces");
            let mut interface_text = String::new();
            for item in array_elems(vm, Some(&arr)) {
                interface_text.push('\n');
                interface_text.push_str(&value_string(vm, &item)?);
            }
            jstr(
                vm,
                format!(
                    "UsbConfiguration[mId={},mName={},mAttributes={},mMaxPower={},mInterfaces=[]{}]",
                    field_int(vm, recv, "mId"),
                    field_str(vm, recv, "mName").unwrap_or_else(|| "null".into()),
                    field_int(vm, recv, "mAttributes"),
                    field_int(vm, recv, "mMaxPower"),
                    interface_text
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_usb_device_all_interfaces(vm: &Engine, recv: ObjRef) -> Vec<Value> {
    let cached = get(vm, recv, "mInterfaces");
    if matches!(cached, Value::Obj(_)) {
        return array_elems(vm, Some(&cached));
    }
    let mut out = Vec::new();
    let configs = get(vm, recv, "mConfigurations");
    for config in array_elems(vm, Some(&configs)) {
        if let Value::Obj(config_ref) = config {
            let interfaces = get(vm, config_ref, "mInterfaces");
            out.extend(array_elems(vm, Some(&interfaces)));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn p57_usb_device_set_fields(
    vm: &mut Engine,
    recv: ObjRef,
    name: Value,
    vendor: i32,
    product: i32,
    class: i32,
    subclass: i32,
    protocol: i32,
    manufacturer: Value,
    product_name: Value,
    version: Value,
    configurations: Value,
    serial: Value,
    flags: [i32; 5],
) -> Result<(), VmExit> {
    put(vm, recv, "mName", name)?;
    put(vm, recv, "mVendorId", Value::Int(vendor))?;
    put(vm, recv, "mProductId", Value::Int(product))?;
    put(vm, recv, "mClass", Value::Int(class))?;
    put(vm, recv, "mSubclass", Value::Int(subclass))?;
    put(vm, recv, "mProtocol", Value::Int(protocol))?;
    put(vm, recv, "mManufacturerName", manufacturer)?;
    put(vm, recv, "mProductName", product_name)?;
    put(vm, recv, "mVersion", version)?;
    put(vm, recv, "mConfigurations", configurations)?;
    put(vm, recv, "rd_serial", serial)?;
    put(vm, recv, "mHasAudioPlayback", Value::Int(flags[0]))?;
    put(vm, recv, "mHasAudioCapture", Value::Int(flags[1]))?;
    put(vm, recv, "mHasMidi", Value::Int(flags[2]))?;
    put(vm, recv, "mHasVideoPlayback", Value::Int(flags[3]))?;
    put(vm, recv, "mHasVideoCapture", Value::Int(flags[4]))?;
    Ok(())
}

fn p57_usb_device_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;IIIIILjava/lang/String;Ljava/lang/String;Ljava/lang/String;[Landroid/hardware/usb/UsbConfiguration;Landroid/hardware/usb/IUsbSerialReader;ZZZZZ)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("name"));
            }
            if opt_str_arg(vm, args, 8)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(iae("version must not be empty"));
            }
            let Some(_) = oarg(args, 9) else {
                return Err(npe("configurations"));
            };
            if array_elems(vm, args.get(9)).iter().any(|v| obj(Some(v)).is_none()) {
                return Err(npe("configurations contains null"));
            }
            if oarg(args, 10).is_none() {
                return Err(npe("serialNumberReader"));
            }
            let flags = [
                iarg(args, 11),
                iarg(args, 12),
                iarg(args, 13),
                iarg(args, 14),
                iarg(args, 15),
            ];
            p57_usb_device_set_fields(
                vm,
                recv,
                args[0].clone(),
                iarg(args, 1),
                iarg(args, 2),
                iarg(args, 3),
                iarg(args, 4),
                iarg(args, 5),
                args[6].clone(),
                args[7].clone(),
                args[8].clone(),
                args[9].clone(),
                Value::Null,
                flags,
            )?;
            Value::Null
        }
        ("getDeviceName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("getManufacturerName", "()Ljava/lang/String;") => get(vm, recv, "mManufacturerName"),
        ("getProductName", "()Ljava/lang/String;") => get(vm, recv, "mProductName"),
        ("getVersion", "()Ljava/lang/String;") => get(vm, recv, "mVersion"),
        ("getSerialNumber", "()Ljava/lang/String;") => get(vm, recv, "rd_serial"),
        ("getDeviceId", "()I") => {
            let name = field_str(vm, recv, "mName").unwrap_or_default();
            Value::Int(p57_usb_device_id_from_name(&name))
        }
        ("getVendorId", "()I") => Value::Int(field_int(vm, recv, "mVendorId")),
        ("getProductId", "()I") => Value::Int(field_int(vm, recv, "mProductId")),
        ("getDeviceClass", "()I") => Value::Int(field_int(vm, recv, "mClass")),
        ("getDeviceSubclass", "()I") => Value::Int(field_int(vm, recv, "mSubclass")),
        ("getDeviceProtocol", "()I") => Value::Int(field_int(vm, recv, "mProtocol")),
        ("getConfigurationCount", "()I") => {
            let arr = get(vm, recv, "mConfigurations");
            Value::Int(array_elems(vm, Some(&arr)).len() as i32)
        }
        ("getConfiguration", "(I)Landroid/hardware/usb/UsbConfiguration;") => {
            let arr = get(vm, recv, "mConfigurations");
            let elems = array_elems(vm, Some(&arr));
            let i = iarg(args, 0);
            if i < 0 || i as usize >= elems.len() {
                return Err(oob(format!(
                    "Index {i} out of bounds for length {}",
                    elems.len()
                )));
            }
            elems[i as usize].clone()
        }
        ("getInterfaceCount", "()I") => {
            Value::Int(p57_usb_device_all_interfaces(vm, recv).len() as i32)
        }
        ("getInterface", "(I)Landroid/hardware/usb/UsbInterface;") => {
            let interfaces = p57_usb_device_all_interfaces(vm, recv);
            let i = iarg(args, 0);
            if i < 0 || i as usize >= interfaces.len() {
                return Err(oob(format!(
                    "Index {i} out of bounds for length {}",
                    interfaces.len()
                )));
            }
            interfaces[i as usize].clone()
        }
        ("hasAudioPlayback", "()Z") => {
            bool_value(field_bool(vm, recv, "mHasAudioPlayback"))
        }
        ("hasAudioCapture", "()Z") => {
            bool_value(field_bool(vm, recv, "mHasAudioCapture"))
        }
        ("hasMidi", "()Z") => bool_value(field_bool(vm, recv, "mHasMidi")),
        ("hasVideoPlayback", "()Z") => {
            bool_value(field_bool(vm, recv, "mHasVideoPlayback"))
        }
        ("hasVideoCapture", "()Z") => {
            bool_value(field_bool(vm, recv, "mHasVideoCapture"))
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_USB_DEVICE => {
                    field_str(vm, recv, "mName") == field_str(vm, other, "mName")
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => Value::Int(
            field_str(vm, recv, "mName")
                .map(|s| java_string_hash(&s))
                .unwrap_or(0),
        ),
        ("toString", "()Ljava/lang/String;") => {
            let configs = get(vm, recv, "mConfigurations");
            let mut config_text = String::new();
            for config in array_elems(vm, Some(&configs)) {
                config_text.push('\n');
                config_text.push_str(&value_string(vm, &config)?);
            }
            jstr(
                vm,
                format!(
                    "UsbDevice[mName={},mVendorId={},mProductId={},mClass={},mSubclass={},mProtocol={},mManufacturerName={},mProductName={},mVersion={},mConfigs=[]{}]",
                    field_str(vm, recv, "mName").unwrap_or_else(|| "null".into()),
                    field_int(vm, recv, "mVendorId"),
                    field_int(vm, recv, "mProductId"),
                    field_int(vm, recv, "mClass"),
                    field_int(vm, recv, "mSubclass"),
                    field_int(vm, recv, "mProtocol"),
                    field_str(vm, recv, "mManufacturerName")
                        .unwrap_or_else(|| "null".into()),
                    field_str(vm, recv, "mProductName")
                        .unwrap_or_else(|| "null".into()),
                    field_str(vm, recv, "mVersion").unwrap_or_else(|| "null".into()),
                    config_text
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_usb_device_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;IIIIILjava/lang/String;Ljava/lang/String;Ljava/lang/String;[Landroid/hardware/usb/UsbConfiguration;Ljava/lang/String;ZZZZZ)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("name"));
            }
            if opt_str_arg(vm, args, 8)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(iae("version must not be empty"));
            }
            let serial = args.get(10).cloned().unwrap_or(Value::Null);
            let flags = [
                iarg(args, 11),
                iarg(args, 12),
                iarg(args, 13),
                iarg(args, 14),
                iarg(args, 15),
            ];
            p57_usb_device_set_fields(
                vm,
                recv,
                args[0].clone(),
                iarg(args, 1),
                iarg(args, 2),
                iarg(args, 3),
                iarg(args, 4),
                iarg(args, 5),
                args[6].clone(),
                args[7].clone(),
                args[8].clone(),
                args[9].clone(),
                serial,
                flags,
            )?;
            // AOSP exposes this temporary public field on Builder.
            let serial = get(vm, recv, "rd_serial");
            put(vm, recv, "serialNumber", serial)?;
            Value::Null
        }
        ("build", "(Landroid/hardware/usb/IUsbSerialReader;)Landroid/hardware/usb/UsbDevice;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("serialReader"));
            }
            let configs_value = get(vm, recv, "mConfigurations");
            if obj(Some(&configs_value)).is_none() {
                return Err(npe("configurations"));
            }
            if array_elems(vm, Some(&configs_value))
                .iter()
                .any(|v| obj(Some(v)).is_none())
            {
                return Err(npe("configurations contains null"));
            }
            let out = alloc_obj(vm, P57_USB_DEVICE, Vec::new())?;
            for f in [
                "mName",
                "mVendorId",
                "mProductId",
                "mClass",
                "mSubclass",
                "mProtocol",
                "mManufacturerName",
                "mProductName",
                "mVersion",
                "mConfigurations",
                "rd_serial",
                "mHasAudioPlayback",
                "mHasAudioCapture",
                "mHasMidi",
                "mHasVideoPlayback",
                "mHasVideoCapture",
            ] {
                let v = get(vm, recv, f);
                put(vm, out, f, v)?;
            }
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

fn p57_usb_device_id_from_name(name: &str) -> i32 {
    // libusbhost's usb_device_get_unique_id_from_name scans
    // "/dev/bus/usb/%d/%d" and returns bus * 1000 + device (with zeroes for
    // fields which do not scan).
    let Some(rest) = name.strip_prefix("/dev/bus/usb/") else {
        return 0;
    };
    let mut parts = rest.split('/');
    let bus: i32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let device: i32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    bus.wrapping_mul(1000).wrapping_add(device)
}

fn p57_usb_device_name_from_id(id: i32) -> String {
    format!("/dev/bus/usb/{:03}/{:03}", id / 1000, id % 1000)
}

fn p57_usb_functions_to_string(functions: i64) -> String {
    let mut parts = Vec::new();
    for (mask, name) in [
        (1_i64 << 2, "mtp"),
        (1_i64 << 4, "ptp"),
        (1_i64 << 5, "rndis"),
        (1_i64 << 3, "midi"),
        (1_i64 << 1, "accessory"),
        (1_i64 << 6, "audio_source"),
        (1_i64 << 10, "ncm"),
        (1_i64 << 7, "uvc"),
        (1_i64, "adb"),
    ] {
        if functions & mask != 0 {
            parts.push(name);
        }
    }
    parts.join(",")
}

fn p57_usb_functions_from_string(functions: Option<String>) -> Result<i64, VmExit> {
    let Some(functions) = functions else {
        return Ok(0);
    };
    if functions == "none" {
        return Ok(0);
    }
    let map = [
        ("mtp", 1_i64 << 2),
        ("ptp", 1_i64 << 4),
        ("rndis", 1_i64 << 5),
        ("midi", 1_i64 << 3),
        ("accessory", 1_i64 << 1),
        ("audio_source", 1_i64 << 6),
        ("adb", 1_i64),
        ("ncm", 1_i64 << 10),
        ("uvc", 1_i64 << 7),
    ];
    let mut out = 0i64;
    for function in functions.split(',') {
        if function.is_empty() {
            continue;
        }
        match map.iter().find(|(name, _)| *name == function) {
            Some((_, code)) => out |= code,
            None => {
                return Err(iae(format!("Invalid usb function {functions}")));
            }
        }
    }
    Ok(out)
}

fn p57_usb_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // This profile exposes no USB host/accessory service or devices.
    let _ = recv;
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/Context;Landroid/hardware/usb/IUsbManager;)V") => {
            let context = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mContext", context)?;
            Value::Null
        }
        ("getDeviceList", "()Ljava/util/HashMap;") => p57_new_map(vm, Vec::new())?,
        (
            "openDevice",
            "(Landroid/hardware/usb/UsbDevice;)Landroid/hardware/usb/UsbDeviceConnection;",
        ) => Value::Null,
        ("getAccessoryList", "()[Landroid/hardware/usb/UsbAccessory;") => Value::Null,
        (
            "openAccessory",
            "(Landroid/hardware/usb/UsbAccessory;)Landroid/os/ParcelFileDescriptor;",
        ) => Value::Null,
        ("hasPermission", "(Landroid/hardware/usb/UsbDevice;)Z")
        | ("hasPermission", "(Landroid/hardware/usb/UsbDevice;Ljava/lang/String;II)Z")
        | ("hasPermission", "(Landroid/hardware/usb/UsbAccessory;)Z")
        | ("hasPermission", "(Landroid/hardware/usb/UsbAccessory;II)Z") => Value::Int(0),
        ("requestPermission", "(Landroid/hardware/usb/UsbDevice;Landroid/app/PendingIntent;)V")
        | (
            "requestPermission",
            "(Landroid/hardware/usb/UsbAccessory;Landroid/app/PendingIntent;)V",
        ) => Value::Null,
        ("isFunctionEnabled", "(Ljava/lang/String;)Z") => Value::Int(0),
        ("getCurrentFunctions", "()J") => Value::Long(0),
        ("getHashForMkFs", "(Ljava/lang/String;)Ljava/lang/String;") => Value::Null,
        _ => return Ok(None),
    }))
}

// --- android.telephony.euicc ----------------------------------------------

fn p57_euicc_send_unavailable(vm: &mut Engine, callback: Option<&Value>) -> Result<(), VmExit> {
    let Some(pi) = obj(callback) else {
        return Err(npe("callbackIntent"));
    };
    // AOSP catches PendingIntent.CanceledException. Host-created/fake pending
    // intents have no delivery target in this no-controller profile.
    let _ = vm.invoke_instance(pi, "send", "(I)V", &[Value::Int(2)]);
    Ok(())
}

fn p57_euicc_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == P57_EUICC_INFO {
        return Ok(Some(match (name, sig) {
            ("<init>", "(Ljava/lang/String;)V") => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                put(vm, recv, "osVersion", v)?;
                Value::Null
            }
            ("getOsVersion", "()Ljava/lang/String;") => get(vm, recv, "osVersion"),
            ("describeContents", "()I") => Value::Int(0),
            _ => return Ok(None),
        }));
    }

    // No IEuiccController is present, so AOSP's isEnabled() is always false.
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/Context;)V") => {
            let context = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mContext", context)?;
            put(vm, recv, "mCardId", Value::Int(-1))?;
            Value::Null
        }
        ("<init>", "(Landroid/content/Context;I)V") => {
            let context = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mContext", context)?;
            put(vm, recv, "mCardId", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("createForCardId", "(I)Landroid/telephony/euicc/EuiccManager;") => {
            let out = alloc_obj(vm, P57_EUICC_MANAGER, Vec::new())?;
            let context = get(vm, recv, "mContext");
            put(vm, out, "mContext", context)?;
            put(vm, out, "mCardId", Value::Int(iarg(args, 0)))?;
            Value::Obj(out)
        }
        ("isEnabled", "()Z") => Value::Int(0),
        ("getEid", "()Ljava/lang/String;") => Value::Null,
        ("getOtaStatus", "()I") => Value::Int(5),
        ("getEuiccInfo", "()Landroid/telephony/euicc/EuiccInfo;") => Value::Null,
        (
            "downloadSubscription",
            "(Landroid/telephony/euicc/DownloadableSubscription;ZLandroid/app/PendingIntent;)V",
        ) => {
            p57_euicc_send_unavailable(vm, args.get(2))?;
            Value::Null
        }
        ("continueOperation", "(Landroid/content/Intent;Landroid/os/Bundle;)V") => {
            // AOSP first extracts the callback PendingIntent from the
            // resolution intent. Parcelable Intent extras are not modeled in
            // this slice, so a null intent is the faithful failure and any
            // other intent has no controller operation to continue.
            if oarg(args, 0).is_none() {
                return Err(npe("resolutionIntent"));
            }
            Value::Null
        }
        (
            "getDownloadableSubscriptionMetadata",
            "(Landroid/telephony/euicc/DownloadableSubscription;Landroid/app/PendingIntent;)V",
        ) => {
            p57_euicc_send_unavailable(vm, args.get(1))?;
            Value::Null
        }
        ("getDefaultDownloadableSubscriptionList", "(Landroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.first())?;
            Value::Null
        }
        ("deleteSubscription", "(ILandroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.get(1))?;
            Value::Null
        }
        ("switchToSubscription", "(ILandroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.get(1))?;
            Value::Null
        }
        ("switchToSubscription", "(IILandroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.get(2))?;
            Value::Null
        }
        ("updateSubscriptionNickname", "(ILjava/lang/String;Landroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.get(2))?;
            Value::Null
        }
        ("eraseSubscriptions", "(Landroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.first())?;
            Value::Null
        }
        (
            "eraseSubscriptions",
            "(Landroid/app/PendingIntent;Landroid/telephony/euicc/EraseEuiccOptions;)V",
        ) => {
            p57_euicc_send_unavailable(vm, args.first())?;
            Value::Null
        }
        (
            "eraseSubscriptions",
            "(Landroid/telephony/euicc/EraseEuiccOptions;Landroid/app/PendingIntent;)V",
        ) => {
            p57_euicc_send_unavailable(vm, args.get(1))?;
            Value::Null
        }
        ("retainSubscriptionsForFactoryReset", "(Landroid/app/PendingIntent;)V") => {
            p57_euicc_send_unavailable(vm, args.first())?;
            Value::Null
        }
        ("setSupportedCountries", "(Ljava/util/List;)V")
        | ("setUnsupportedCountries", "(Ljava/util/List;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("countries"));
            }
            Value::Null
        }
        ("getSupportedCountries", "()Ljava/util/List;")
        | ("getUnsupportedCountries", "()Ljava/util/List;") => {
            Value::Obj(new_collection(vm, "Ljava/util/ArrayList;", Vec::new())?)
        }
        ("isSupportedCountry", "(Ljava/lang/String;)Z") => {
            if oarg(args, 0).is_none() {
                return Err(npe("countryIso"));
            }
            Value::Int(0)
        }
        ("isSimPortAvailable", "(I)Z") => Value::Int(0),
        (
            "startResolutionActivity",
            "(Landroid/app/Activity;ILandroid/content/Intent;Landroid/app/PendingIntent;)V",
        ) => {
            if oarg(args, 2).is_none() {
                return Err(npe("resultIntent"));
            }
            // The hidden resolution PendingIntent is carried as a Parcelable
            // Intent extra, which this slice does not model.
            return Err(iae("Invalid result intent"));
        }
        _ => return Ok(None),
    }))
}

// --- android.app.usage -------------------------------------------------------

const P57_USAGE_STATS_FIELDS: &[&str] = &[
    "mPackageName",
    "mBeginTimeStamp",
    "mEndTimeStamp",
    "mLastTimeUsed",
    "mLastTimeVisible",
    "mLastTimeComponentUsed",
    "mLastTimeForegroundServiceUsed",
    "mTotalTimeInForeground",
    "mTotalTimeVisible",
    "mTotalTimeForegroundServiceUsed",
    "mLaunchCount",
    "mAppLaunchCount",
    "mLastEvent",
];

fn p57_usage_stats_copy(vm: &mut Engine, recv: ObjRef, other: ObjRef) -> Result<(), VmExit> {
    for f in P57_USAGE_STATS_FIELDS {
        let v = get(vm, other, f);
        put(vm, recv, f, v)?;
    }
    Ok(())
}

fn p57_usage_stats_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("<init>", "(Landroid/app/usage/UsageStats;)V") => {
            let Some(other) = oarg(args, 0) else {
                return Err(npe("stats"));
            };
            p57_usage_stats_copy(vm, recv, other)?;
            Value::Null
        }
        ("getObfuscatedForInstantApp", "()Landroid/app/usage/UsageStats;") => {
            let out = alloc_obj(vm, P57_USAGE_STATS, Vec::new())?;
            p57_usage_stats_copy(vm, out, recv)?;
            let package = jstr(vm, "android.instant_app")?;
            put(vm, out, "mPackageName", package)?;
            Value::Obj(out)
        }
        ("getPackageName", "()Ljava/lang/String;") => get(vm, recv, "mPackageName"),
        ("getFirstTimeStamp", "()J") => Value::Long(field_long(vm, recv, "mBeginTimeStamp")),
        ("getLastTimeStamp", "()J") => Value::Long(field_long(vm, recv, "mEndTimeStamp")),
        ("getLastTimeUsed", "()J") => Value::Long(field_long(vm, recv, "mLastTimeUsed")),
        ("getLastTimeVisible", "()J") => Value::Long(field_long(vm, recv, "mLastTimeVisible")),
        ("getTotalTimeInForeground", "()J") => {
            Value::Long(field_long(vm, recv, "mTotalTimeInForeground"))
        }
        ("getTotalTimeVisible", "()J") => Value::Long(field_long(vm, recv, "mTotalTimeVisible")),
        ("getLastTimeForegroundServiceUsed", "()J") => {
            Value::Long(field_long(vm, recv, "mLastTimeForegroundServiceUsed"))
        }
        ("getTotalTimeForegroundServiceUsed", "()J") => {
            Value::Long(field_long(vm, recv, "mTotalTimeForegroundServiceUsed"))
        }
        ("getLastTimeAnyComponentUsed", "()J") => {
            Value::Long(field_long(vm, recv, "mLastTimeComponentUsed"))
        }
        ("getLastTimePackageUsed", "()J") => Value::Long(
            field_long(vm, recv, "mLastTimeUsed")
                .max(field_long(vm, recv, "mLastTimeVisible"))
                .max(field_long(vm, recv, "mLastTimeForegroundServiceUsed"))
                .max(field_long(vm, recv, "mLastTimeComponentUsed")),
        ),
        ("getAppLaunchCount", "()I") => Value::Int(field_int(vm, recv, "mAppLaunchCount")),
        ("add", "(Landroid/app/usage/UsageStats;)V") => {
            let Some(right) = oarg(args, 0) else {
                return Err(npe("right"));
            };
            let my_package = field_str(vm, recv, "mPackageName");
            let right_package = field_str(vm, right, "mPackageName");
            if my_package.is_none() {
                return Err(npe("mPackageName"));
            }
            if my_package != right_package {
                return Err(iae(format!(
                    "Can't merge UsageStats for package '{}' with UsageStats for package '{}'.",
                    my_package.unwrap_or_default(),
                    right_package.unwrap_or_else(|| "null".into())
                )));
            }
            let my_begin = field_long(vm, recv, "mBeginTimeStamp");
            let right_begin = field_long(vm, right, "mBeginTimeStamp");
            if right_begin > my_begin {
                for f in [
                    "mLastTimeUsed",
                    "mLastTimeVisible",
                    "mLastTimeComponentUsed",
                    "mLastTimeForegroundServiceUsed",
                ] {
                    let value = field_long(vm, recv, f).max(field_long(vm, right, f));
                    put(vm, recv, f, Value::Long(value))?;
                }
            }
            put(
                vm,
                recv,
                "mBeginTimeStamp",
                Value::Long(my_begin.min(right_begin)),
            )?;
            let end =
                field_long(vm, recv, "mEndTimeStamp").max(field_long(vm, right, "mEndTimeStamp"));
            put(vm, recv, "mEndTimeStamp", Value::Long(end))?;
            for f in [
                "mTotalTimeInForeground",
                "mTotalTimeVisible",
                "mTotalTimeForegroundServiceUsed",
            ] {
                let value = field_long(vm, recv, f).wrapping_add(field_long(vm, right, f));
                put(vm, recv, f, Value::Long(value))?;
            }
            for f in ["mLaunchCount", "mAppLaunchCount"] {
                let value = field_int(vm, recv, f).wrapping_add(field_int(vm, right, f));
                put(vm, recv, f, Value::Int(value))?;
            }
            Value::Null
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

const P57_USAGE_EVENT_FIELDS: &[&str] = &[
    "mPackage",
    "mClass",
    "mInstanceId",
    "mTaskRootPackage",
    "mTaskRootClass",
    "mTimeStamp",
    "mEventType",
    "mConfiguration",
    "mShortcutId",
    "mAction",
    "mContentType",
    "mContentAnnotations",
    "mFlags",
    "mBucketAndReason",
    "mNotificationChannelId",
    "mLocusId",
];

fn p57_usage_event_copy(vm: &mut Engine, recv: ObjRef, other: ObjRef) -> Result<(), VmExit> {
    for f in P57_USAGE_EVENT_FIELDS {
        let v = get(vm, other, f);
        put(vm, recv, f, v)?;
    }
    Ok(())
}

fn p57_usage_event_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("<init>", "(IJ)V") => {
            put(vm, recv, "mEventType", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mTimeStamp", Value::Long(larg(args, 1)))?;
            Value::Null
        }
        ("<init>", "(Landroid/app/usage/UsageEvents$Event;)V") => {
            let Some(other) = oarg(args, 0) else {
                return Err(npe("orig"));
            };
            p57_usage_event_copy(vm, recv, other)?;
            Value::Null
        }
        ("copyFrom", "(Landroid/app/usage/UsageEvents$Event;)V") => {
            let Some(other) = oarg(args, 0) else {
                return Err(npe("orig"));
            };
            p57_usage_event_copy(vm, recv, other)?;
            Value::Null
        }
        ("getPackageName", "()Ljava/lang/String;") => get(vm, recv, "mPackage"),
        ("isInstantApp", "()Z") => bool_value(field_int(vm, recv, "mFlags") & 1 == 1),
        ("getClassName", "()Ljava/lang/String;") => get(vm, recv, "mClass"),
        ("getInstanceId", "()I") => Value::Int(field_int(vm, recv, "mInstanceId")),
        ("getTaskRootPackageName", "()Ljava/lang/String;") => get(vm, recv, "mTaskRootPackage"),
        ("getTaskRootClassName", "()Ljava/lang/String;") => get(vm, recv, "mTaskRootClass"),
        ("getTimeStamp", "()J") => Value::Long(field_long(vm, recv, "mTimeStamp")),
        ("getEventType", "()I") => Value::Int(field_int(vm, recv, "mEventType")),
        ("getConfiguration", "()Landroid/content/res/Configuration;") => {
            get(vm, recv, "mConfiguration")
        }
        ("getShortcutId", "()Ljava/lang/String;") => get(vm, recv, "mShortcutId"),
        ("getAppStandbyBucket", "()I") => Value::Int(
            ((field_int(vm, recv, "mBucketAndReason") as u32 & 0xffff_0000) >> 16) as i32,
        ),
        ("getStandbyReason", "()I") => {
            Value::Int(field_int(vm, recv, "mBucketAndReason") & 0x0000_ffff)
        }
        ("getNotificationChannelId", "()Ljava/lang/String;") => {
            get(vm, recv, "mNotificationChannelId")
        }
        ("getLocusId", "()Ljava/lang/String;") => get(vm, recv, "mLocusId"),
        ("getObfuscatedIfInstantApp", "()Landroid/app/usage/UsageEvents$Event;") => {
            if field_int(vm, recv, "mFlags") & 1 != 1 {
                Value::Obj(recv)
            } else {
                let out = alloc_obj(vm, P57_USAGE_EVENT, Vec::new())?;
                p57_usage_event_copy(vm, out, recv)?;
                let package = jstr(vm, "android.instant_app")?;
                let class = jstr(vm, "android.instant_app")?;
                put(vm, out, "mPackage", package)?;
                put(vm, out, "mClass", class)?;
                Value::Obj(out)
            }
        }
        ("getObfuscatedNotificationEvent", "()Landroid/app/usage/UsageEvents$Event;") => {
            let out = alloc_obj(vm, P57_USAGE_EVENT, Vec::new())?;
            p57_usage_event_copy(vm, out, recv)?;
            let channel = jstr(vm, "unknown_channel_id")?;
            put(vm, out, "mNotificationChannelId", channel)?;
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

fn p57_usage_events_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            put(vm, recv, "mEventCount", Value::Int(0))?;
            put(vm, recv, "mIndex", Value::Int(0))?;
            Value::Null
        }
        ("<init>", "(Ljava/util/List;[Ljava/lang/String;)V")
        | ("<init>", "(Ljava/util/List;[Ljava/lang/String;Z)V") => {
            let Some(_) = oarg(args, 0) else {
                return Err(npe("events"));
            };
            let items = collection_items(vm, args.first());
            let events = obj_array(vm, "[Landroid/app/usage/UsageEvents$Event;", items.clone())?;
            let pool = args.get(1).cloned().unwrap_or(Value::Null);
            put(vm, recv, "rd_events", events)?;
            put(vm, recv, "mStringPool", pool)?;
            put(vm, recv, "mEventCount", Value::Int(items.len() as i32))?;
            put(vm, recv, "mIndex", Value::Int(0))?;
            put(
                vm,
                recv,
                "mIncludeTaskRoots",
                Value::Int(if sig.ends_with("Z)V") {
                    iarg(args, 2)
                } else {
                    0
                }),
            )?;
            Value::Null
        }
        ("hasNextEvent", "()Z") => {
            bool_value(field_int(vm, recv, "mIndex") < field_int(vm, recv, "mEventCount"))
        }
        ("getNextEvent", "(Landroid/app/usage/UsageEvents$Event;)Z") => {
            let Some(out) = oarg(args, 0) else {
                return Err(iae("Given eventOut must not be null"));
            };
            let index = field_int(vm, recv, "mIndex");
            let count = field_int(vm, recv, "mEventCount");
            if index >= count {
                Value::Int(0)
            } else {
                let arr = get(vm, recv, "rd_events");
                let elems = array_elems(vm, Some(&arr));
                match elems.get(index as usize) {
                    Some(Value::Obj(source_event)) => {
                        p57_usage_event_copy(vm, out, *source_event)?;
                        put(vm, recv, "mIndex", Value::Int(index + 1))?;
                        Value::Int(1)
                    }
                    _ => Value::Int(0),
                }
            }
        }
        ("resetToStart", "()V") => {
            put(vm, recv, "mIndex", Value::Int(0))?;
            Value::Null
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_new_empty_usage_events(vm: &mut Engine) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, P57_USAGE_EVENTS, Vec::new())?;
    put(vm, r, "mEventCount", Value::Int(0))?;
    put(vm, r, "mIndex", Value::Int(0))?;
    Ok(Value::Obj(r))
}

fn p57_usage_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Bounded profile: no persisted usage history or standby controller.
    let empty_list = |vm: &mut Engine| -> Result<Value, VmExit> {
        Ok(Value::Obj(new_collection(
            vm,
            "Ljava/util/ArrayList;",
            Vec::new(),
        )?))
    };
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/Context;Landroid/app/usage/IUsageStatsManager;)V") => {
            let context = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mContext", context)?;
            Value::Null
        }
        ("queryUsageStats", "(IJJ)Ljava/util/List;")
        | ("queryConfigurations", "(IJJ)Ljava/util/List;")
        | ("queryEventStats", "(IJJ)Ljava/util/List;") => empty_list(vm)?,
        ("queryEvents", "(JJ)Landroid/app/usage/UsageEvents;")
        | ("queryEventsForSelf", "(JJ)Landroid/app/usage/UsageEvents;") => {
            p57_new_empty_usage_events(vm)?
        }
        ("queryAndAggregateUsageStats", "(JJ)Ljava/util/Map;") => {
            p57_new_map(vm, Vec::new())?
        }
        ("isAppStandbyEnabled", "()Z") => Value::Int(0),
        ("isAppInactive", "(Ljava/lang/String;)Z") => Value::Int(0),
        ("setAppInactive", "(Ljava/lang/String;Z)V") => Value::Null,
        ("getAppStandbyBucket", "()I") | ("getAppStandbyBucket", "(Ljava/lang/String;)I") => {
            Value::Int(10)
        }
        ("setAppStandbyBucket", "(Ljava/lang/String;I)V") => Value::Null,
        ("getAppStandbyBuckets", "()Ljava/util/Map;") => p57_new_map(vm, Vec::new())?,
        ("setAppStandbyBuckets", "(Ljava/util/Map;)V") => Value::Null,
        ("getAppMinStandbyBucket", "(Ljava/lang/String;)I") => Value::Int(10),
        ("registerAppUsageObserver", "(I[Ljava/lang/String;JLjava/util/concurrent/TimeUnit;Landroid/app/PendingIntent;)V") => {
            if oarg(args, 1).is_none() {
                return Err(npe("observedEntities"));
            }
            if oarg(args, 3).is_none() {
                return Err(npe("timeUnit"));
            }
            if oarg(args, 4).is_none() {
                return Err(npe("callbackIntent"));
            }
            Value::Null
        }
        ("unregisterAppUsageObserver", "(I)V")
        | ("unregisterUsageSessionObserver", "(I)V")
        | ("unregisterAppUsageLimitObserver", "(I)V") => Value::Null,
        ("registerUsageSessionObserver", "(I[Ljava/lang/String;Ljava/time/Duration;Ljava/time/Duration;Landroid/app/PendingIntent;Landroid/app/PendingIntent;)V") => {
            if oarg(args, 1).is_none() {
                return Err(npe("observedEntities"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("timeLimit"));
            }
            if oarg(args, 3).is_none() {
                return Err(npe("sessionThresholdTime"));
            }
            if oarg(args, 4).is_none() {
                return Err(npe("limitReachedCallbackIntent"));
            }
            Value::Null
        }
        ("registerAppUsageLimitObserver", "(I[Ljava/lang/String;Ljava/time/Duration;Landroid/app/PendingIntent;)V") => {
            if oarg(args, 1).is_none() {
                return Err(npe("observedEntities"));
            }
            if oarg(args, 2).is_none() {
                return Err(npe("timeLimit"));
            }
            if oarg(args, 3).is_none() {
                return Err(npe("callbackIntent"));
            }
            Value::Null
        }
        ("queryBroadcastResponseStats", "(Landroid/content/ComponentName;J)Ljava/util/List;") => {
            empty_list(vm)?
        }
        ("getUsageSource", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

// --- android.print ---------------------------------------------------------

fn p57_valid_single_flag(value: i32, valid: i32) -> bool {
    value & valid != 0 && value.count_ones() == 1 && value & !valid == 0
}

fn p57_media_size_new(
    vm: &mut Engine,
    id: Value,
    label: Value,
    package: Value,
    width: i32,
    height: i32,
    res_id: i32,
) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, P57_MEDIA_SIZE, Vec::new())?;
    put(vm, r, "mId", id)?;
    put(vm, r, "mLabel", label)?;
    put(vm, r, "mPackageName", package)?;
    put(vm, r, "mWidthMils", Value::Int(width))?;
    put(vm, r, "mHeightMils", Value::Int(height))?;
    put(vm, r, "mLabelResId", Value::Int(res_id))?;
    Ok(Value::Obj(r))
}

fn p57_media_size_standard(vm: &mut Engine, id: &str) -> Result<Option<Value>, VmExit> {
    let dims = match id {
        "UNKNOWN_PORTRAIT" => (1, i32::MAX),
        "UNKNOWN_LANDSCAPE" => (i32::MAX, 1),
        "ISO_A0" => (33110, 46810),
        "ISO_A1" => (23390, 33110),
        "ISO_A2" => (16540, 23390),
        "ISO_A3" => (11690, 16540),
        "ISO_A4" => (8270, 11690),
        "ISO_A5" => (5830, 8270),
        "ISO_A6" => (4130, 5830),
        "ISO_A7" => (2910, 4130),
        "ISO_A8" => (2050, 2910),
        "ISO_A9" => (1460, 2050),
        "ISO_A10" => (1020, 1460),
        "ISO_B0" => (39370, 55670),
        "ISO_B1" => (27830, 39370),
        "ISO_B2" => (19690, 27830),
        "ISO_B3" => (13900, 19690),
        "ISO_B4" => (9840, 13900),
        "ISO_B5" => (6930, 9840),
        "ISO_B6" => (4920, 6930),
        "ISO_B7" => (3460, 4920),
        "ISO_B8" => (2440, 3460),
        "ISO_B9" => (1730, 2440),
        "ISO_B10" => (1220, 1730),
        "ISO_C0" => (36100, 51060),
        "ISO_C1" => (25510, 36100),
        "ISO_C2" => (18030, 25510),
        "ISO_C3" => (12760, 18030),
        "ISO_C4" => (9020, 12760),
        "ISO_C5" => (6380, 9020),
        "ISO_C6" => (4490, 6380),
        "ISO_C7" => (3190, 4490),
        "ISO_C8" => (2240, 3190),
        "ISO_C9" => (1570, 2240),
        "ISO_C10" => (1100, 1570),
        "NA_LETTER" => (8500, 11000),
        "NA_GOVT_LETTER" => (8000, 10500),
        "NA_LEGAL" => (8500, 14000),
        "NA_JUNIOR_LEGAL" => (8000, 5000),
        "NA_LEDGER" => (17000, 11000),
        "NA_TABLOID" => (11000, 17000),
        _ => return Ok(None),
    };
    let idv = jstr(vm, id)?;
    let label = jstr(vm, id)?;
    let pkg = jstr(vm, "android")?;
    Ok(Some(p57_media_size_new(
        vm, idv, label, pkg, dims.0, dims.1, 0,
    )?))
}

fn p57_media_size_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;II)V") => {
            let id = opt_str_arg(vm, args, 0);
            let label = opt_str_arg(vm, args, 1);
            if id.as_deref().map(str::is_empty).unwrap_or(true) {
                return Err(iae("id cannot be empty."));
            }
            if label.as_deref().map(str::is_empty).unwrap_or(true) {
                return Err(iae("label cannot be empty."));
            }
            if iarg(args, 2) <= 0 {
                return Err(iae("widthMils cannot be less than or equal to zero."));
            }
            if iarg(args, 3) <= 0 {
                return Err(iae("heightMils cannot be less than or equal to zero."));
            }
            put(vm, recv, "mId", args[0].clone())?;
            put(vm, recv, "mLabel", args[1].clone())?;
            put(vm, recv, "mPackageName", Value::Null)?;
            put(vm, recv, "mWidthMils", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mHeightMils", Value::Int(iarg(args, 3)))?;
            put(vm, recv, "mLabelResId", Value::Int(0))?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;III)V") => {
            // Hidden predefined-size constructor: (id, package, res, w, h).
            if opt_str_arg(vm, args, 0)
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
            {
                return Err(iae("id cannot be empty."));
            }
            if opt_str_arg(vm, args, 1)
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
                || iarg(args, 2) == 0
            {
                return Err(iae("label cannot be empty."));
            }
            if iarg(args, 3) <= 0 {
                return Err(iae("widthMils cannot be less than or equal to zero."));
            }
            if iarg(args, 4) <= 0 {
                return Err(iae("heightMils cannot be less than or equal to zero."));
            }
            put(vm, recv, "mId", args[0].clone())?;
            put(vm, recv, "mLabel", Value::Null)?;
            put(vm, recv, "mPackageName", args[1].clone())?;
            put(vm, recv, "mWidthMils", Value::Int(iarg(args, 3)))?;
            put(vm, recv, "mHeightMils", Value::Int(iarg(args, 4)))?;
            put(vm, recv, "mLabelResId", Value::Int(iarg(args, 2)))?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;III)V") => {
            if opt_str_arg(vm, args, 0)
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
            {
                return Err(iae("id cannot be empty."));
            }
            if iarg(args, 3) <= 0 {
                return Err(iae("widthMils cannot be less than or equal to zero."));
            }
            if iarg(args, 4) <= 0 {
                return Err(iae("heightMils cannot be less than or equal to zero."));
            }
            let has_label = opt_str_arg(vm, args, 1)
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            let has_res = opt_str_arg(vm, args, 2)
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
                && iarg(args, 5) != 0;
            if has_label == has_res {
                return Err(iae("label cannot be empty."));
            }
            put(vm, recv, "mId", args[0].clone())?;
            put(vm, recv, "mLabel", args[1].clone())?;
            put(vm, recv, "mPackageName", args[2].clone())?;
            put(vm, recv, "mWidthMils", Value::Int(iarg(args, 3)))?;
            put(vm, recv, "mHeightMils", Value::Int(iarg(args, 4)))?;
            put(vm, recv, "mLabelResId", Value::Int(iarg(args, 5)))?;
            Value::Null
        }
        ("getId", "()Ljava/lang/String;") => get(vm, recv, "mId"),
        ("getLabel", "(Landroid/content/pm/PackageManager;)Ljava/lang/String;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("packageManager"));
            }
            let label = get(vm, recv, "mLabel");
            if matches!(label, Value::Obj(_)) {
                label
            } else {
                // Framework media-size string resources are not modeled; IDs
                // remain the stable human-readable fallback for predefined
                // sizes created through the hidden constructor.
                get(vm, recv, "mId")
            }
        }
        ("getWidthMils", "()I") => Value::Int(field_int(vm, recv, "mWidthMils")),
        ("getHeightMils", "()I") => Value::Int(field_int(vm, recv, "mHeightMils")),
        ("isPortrait", "()Z") => bool_value(
            field_int(vm, recv, "mHeightMils") >= field_int(vm, recv, "mWidthMils"),
        ),
        ("asPortrait", "()Landroid/print/PrintAttributes$MediaSize;") => {
            let w = field_int(vm, recv, "mWidthMils");
            let h = field_int(vm, recv, "mHeightMils");
            if h >= w {
                Value::Obj(recv)
            } else {
                p57_media_size_new(
                    vm,
                    get(vm, recv, "mId"),
                    get(vm, recv, "mLabel"),
                    get(vm, recv, "mPackageName"),
                    w.min(h),
                    w.max(h),
                    field_int(vm, recv, "mLabelResId"),
                )?
            }
        }
        ("asLandscape", "()Landroid/print/PrintAttributes$MediaSize;") => {
            let w = field_int(vm, recv, "mWidthMils");
            let h = field_int(vm, recv, "mHeightMils");
            if h < w {
                Value::Obj(recv)
            } else {
                p57_media_size_new(
                    vm,
                    get(vm, recv, "mId"),
                    get(vm, recv, "mLabel"),
                    get(vm, recv, "mPackageName"),
                    w.max(h),
                    w.min(h),
                    field_int(vm, recv, "mLabelResId"),
                )?
            }
        }
        ("hashCode", "()I") => {
            let mut hash = 1i32;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mWidthMils"));
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mHeightMils"));
            Value::Int(hash)
        }
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(matches!(
            oarg(args, 0),
            Some(other)
                if class_of(vm, other) == P57_MEDIA_SIZE
                    && field_int(vm, recv, "mWidthMils") == field_int(vm, other, "mWidthMils")
                    && field_int(vm, recv, "mHeightMils") == field_int(vm, other, "mHeightMils")
        )),
        ("toString", "()Ljava/lang/String;") => jstr(
            vm,
            format!(
                "MediaSize{{id: {}, label: {}, packageName: {}, heightMils: {}, widthMils: {}, labelResId: {}}}",
                field_str(vm, recv, "mId").unwrap_or_else(|| "null".into()),
                field_str(vm, recv, "mLabel").unwrap_or_else(|| "null".into()),
                field_str(vm, recv, "mPackageName").unwrap_or_else(|| "null".into()),
                field_int(vm, recv, "mHeightMils"),
                field_int(vm, recv, "mWidthMils"),
                field_int(vm, recv, "mLabelResId")
            ),
        )?,
        _ => return Ok(None),
    }))
}

fn p57_resolution_new(
    vm: &mut Engine,
    id: Value,
    label: Value,
    horizontal: i32,
    vertical: i32,
) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, P57_RESOLUTION, Vec::new())?;
    put(vm, r, "mId", id)?;
    put(vm, r, "mLabel", label)?;
    put(vm, r, "mHorizontalDpi", Value::Int(horizontal))?;
    put(vm, r, "mVerticalDpi", Value::Int(vertical))?;
    Ok(Value::Obj(r))
}

fn p57_resolution_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;II)V") => {
            if opt_str_arg(vm, args, 0)
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
            {
                return Err(iae("id cannot be empty."));
            }
            if opt_str_arg(vm, args, 1)
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
            {
                return Err(iae("label cannot be empty."));
            }
            if iarg(args, 2) <= 0 {
                return Err(iae("horizontalDpi cannot be less than or equal to zero."));
            }
            if iarg(args, 3) <= 0 {
                return Err(iae("verticalDpi cannot be less than or equal to zero."));
            }
            put(vm, recv, "mId", args[0].clone())?;
            put(vm, recv, "mLabel", args[1].clone())?;
            put(vm, recv, "mHorizontalDpi", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mVerticalDpi", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        ("getId", "()Ljava/lang/String;") => get(vm, recv, "mId"),
        ("getLabel", "()Ljava/lang/String;") => get(vm, recv, "mLabel"),
        ("getHorizontalDpi", "()I") => Value::Int(field_int(vm, recv, "mHorizontalDpi")),
        ("getVerticalDpi", "()I") => Value::Int(field_int(vm, recv, "mVerticalDpi")),
        ("hashCode", "()I") => {
            let mut hash = 1i32;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mHorizontalDpi"));
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mVerticalDpi"));
            Value::Int(hash)
        }
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(matches!(
            oarg(args, 0),
            Some(other)
                if class_of(vm, other) == P57_RESOLUTION
                    && field_int(vm, recv, "mHorizontalDpi")
                        == field_int(vm, other, "mHorizontalDpi")
                    && field_int(vm, recv, "mVerticalDpi")
                        == field_int(vm, other, "mVerticalDpi")
        )),
        ("toString", "()Ljava/lang/String;") => jstr(
            vm,
            format!(
                "Resolution{{id: {}, label: {}, horizontalDpi: {}, verticalDpi: {}}}",
                field_str(vm, recv, "mId").unwrap_or_else(|| "null".into()),
                field_str(vm, recv, "mLabel").unwrap_or_else(|| "null".into()),
                field_int(vm, recv, "mHorizontalDpi"),
                field_int(vm, recv, "mVerticalDpi")
            ),
        )?,
        _ => return Ok(None),
    }))
}

fn p57_margins_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(IIII)V") => {
            put(vm, recv, "mLeftMils", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mTopMils", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "mRightMils", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mBottomMils", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        ("getLeftMils", "()I") => Value::Int(field_int(vm, recv, "mLeftMils")),
        ("getTopMils", "()I") => Value::Int(field_int(vm, recv, "mTopMils")),
        ("getRightMils", "()I") => Value::Int(field_int(vm, recv, "mRightMils")),
        ("getBottomMils", "()I") => Value::Int(field_int(vm, recv, "mBottomMils")),
        ("hashCode", "()I") => {
            let mut hash = 1i32;
            for f in ["mBottomMils", "mLeftMils", "mRightMils", "mTopMils"] {
                hash = hash.wrapping_mul(31).wrapping_add(field_int(vm, recv, f));
            }
            Value::Int(hash)
        }
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(matches!(
            oarg(args, 0),
            Some(other)
                if class_of(vm, other) == P57_MARGINS
                    && ["mLeftMils", "mTopMils", "mRightMils", "mBottomMils"]
                        .iter()
                        .all(|f| field_int(vm, recv, f) == field_int(vm, other, f))
        )),
        ("toString", "()Ljava/lang/String;") => jstr(
            vm,
            format!(
                "Margins{{leftMils: {}, topMils: {}, rightMils: {}, bottomMils: {}}}",
                field_int(vm, recv, "mLeftMils"),
                field_int(vm, recv, "mTopMils"),
                field_int(vm, recv, "mRightMils"),
                field_int(vm, recv, "mBottomMils")
            ),
        )?,
        _ => return Ok(None),
    }))
}

fn p57_print_attributes_copy(vm: &mut Engine, recv: ObjRef, other: ObjRef) -> Result<(), VmExit> {
    for f in [
        "mMediaSize",
        "mResolution",
        "mMinMargins",
        "mColorMode",
        "mDuplexMode",
    ] {
        let v = get(vm, other, f);
        put(vm, recv, f, v)?;
    }
    Ok(())
}

fn p57_print_value_equal(vm: &mut Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    match (obj(Some(a)), obj(Some(b))) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => {
            if a == b {
                Ok(true)
            } else {
                Ok(
                    vm.invoke_instance(a, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(b)])?
                        == Value::Int(1),
                )
            }
        }
        _ => Ok(false),
    }
}

fn p57_print_value_hash(vm: &mut Engine, v: &Value) -> Result<i32, VmExit> {
    match obj(Some(v)) {
        Some(r) => Ok(vm
            .invoke_instance(r, "hashCode", "()I", &[])?
            .as_int()
            .unwrap_or(0)),
        None => Ok(0),
    }
}

fn p57_print_attributes_rotated(
    vm: &mut Engine,
    recv: ObjRef,
    portrait: bool,
) -> Result<Value, VmExit> {
    let Some(media) = field_obj(vm, recv, "mMediaSize") else {
        return Err(npe("mMediaSize"));
    };
    let is_portrait = field_int(vm, media, "mHeightMils") >= field_int(vm, media, "mWidthMils");
    if is_portrait == portrait {
        return Ok(Value::Obj(recv));
    }
    let Some(resolution) = field_obj(vm, recv, "mResolution") else {
        return Err(npe("mResolution"));
    };
    let color = field_int(vm, recv, "mColorMode");
    let duplex = field_int(vm, recv, "mDuplexMode");
    if !p57_valid_single_flag(color, 3) {
        return Err(iae(format!("invalid color mode: {color}")));
    }
    if !p57_valid_single_flag(duplex, 7) {
        return Err(iae(format!("invalid duplex mode: {duplex}")));
    }
    let new_media = vm.invoke_instance(
        media,
        if portrait {
            "asPortrait"
        } else {
            "asLandscape"
        },
        "()Landroid/print/PrintAttributes$MediaSize;",
        &[],
    )?;
    let new_resolution = p57_resolution_new(
        vm,
        get(vm, resolution, "mId"),
        get(vm, resolution, "mLabel"),
        field_int(vm, resolution, "mVerticalDpi"),
        field_int(vm, resolution, "mHorizontalDpi"),
    )?;
    let margins = get(vm, recv, "mMinMargins");
    let out = alloc_obj(vm, P57_PRINT_ATTRIBUTES, Vec::new())?;
    put(vm, out, "mMediaSize", new_media)?;
    put(vm, out, "mResolution", new_resolution)?;
    put(vm, out, "mMinMargins", margins)?;
    put(vm, out, "mColorMode", Value::Int(color))?;
    put(vm, out, "mDuplexMode", Value::Int(duplex))?;
    Ok(Value::Obj(out))
}

fn p57_print_attributes_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("getMediaSize", "()Landroid/print/PrintAttributes$MediaSize;") => {
            get(vm, recv, "mMediaSize")
        }
        ("setMediaSize", "(Landroid/print/PrintAttributes$MediaSize;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mMediaSize", v)?;
            Value::Null
        }
        ("getResolution", "()Landroid/print/PrintAttributes$Resolution;") => {
            get(vm, recv, "mResolution")
        }
        ("setResolution", "(Landroid/print/PrintAttributes$Resolution;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mResolution", v)?;
            Value::Null
        }
        ("getMinMargins", "()Landroid/print/PrintAttributes$Margins;") => {
            get(vm, recv, "mMinMargins")
        }
        ("setMinMargins", "(Landroid/print/PrintAttributes$Margins;)V") => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mMinMargins", v)?;
            Value::Null
        }
        ("getColorMode", "()I") => Value::Int(field_int(vm, recv, "mColorMode")),
        ("setColorMode", "(I)V") => {
            let mode = iarg(args, 0);
            if !p57_valid_single_flag(mode, 3) {
                return Err(iae(format!("invalid color mode: {mode}")));
            }
            put(vm, recv, "mColorMode", Value::Int(mode))?;
            Value::Null
        }
        ("getDuplexMode", "()I") => Value::Int(field_int(vm, recv, "mDuplexMode")),
        ("setDuplexMode", "(I)V") => {
            let mode = iarg(args, 0);
            if !p57_valid_single_flag(mode, 7) {
                return Err(iae(format!("invalid duplex mode: {mode}")));
            }
            put(vm, recv, "mDuplexMode", Value::Int(mode))?;
            Value::Null
        }
        ("isPortrait", "()Z") => {
            let Some(media) = field_obj(vm, recv, "mMediaSize") else {
                return Err(npe("mMediaSize"));
            };
            bool_value(field_int(vm, media, "mHeightMils") >= field_int(vm, media, "mWidthMils"))
        }
        ("asPortrait", "()Landroid/print/PrintAttributes;") => {
            p57_print_attributes_rotated(vm, recv, true)?
        }
        ("asLandscape", "()Landroid/print/PrintAttributes;") => {
            p57_print_attributes_rotated(vm, recv, false)?
        }
        ("clear", "()V") => {
            put(vm, recv, "mMediaSize", Value::Null)?;
            put(vm, recv, "mResolution", Value::Null)?;
            put(vm, recv, "mMinMargins", Value::Null)?;
            put(vm, recv, "mColorMode", Value::Int(0))?;
            put(vm, recv, "mDuplexMode", Value::Int(0))?;
            Value::Null
        }
        ("copyFrom", "(Landroid/print/PrintAttributes;)V") => {
            let Some(other) = oarg(args, 0) else {
                return Err(npe("other"));
            };
            p57_print_attributes_copy(vm, recv, other)?;
            Value::Null
        }
        ("hashCode", "()I") => {
            let mut hash = 1i32;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mColorMode"));
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mDuplexMode"));
            for f in ["mMinMargins", "mMediaSize", "mResolution"] {
                let v = get(vm, recv, f);
                let h = p57_print_value_hash(vm, &v)?;
                hash = hash.wrapping_mul(31).wrapping_add(h);
            }
            Value::Int(hash)
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_PRINT_ATTRIBUTES => {
                    if other == recv {
                        true
                    } else if field_int(vm, recv, "mColorMode")
                        != field_int(vm, other, "mColorMode")
                        || field_int(vm, recv, "mDuplexMode") != field_int(vm, other, "mDuplexMode")
                    {
                        false
                    } else {
                        let mut all = true;
                        for f in ["mMinMargins", "mMediaSize", "mResolution"] {
                            let a = get(vm, recv, f);
                            let b = get(vm, other, f);
                            if !p57_print_value_equal(vm, &a, &b)? {
                                all = false;
                                break;
                            }
                        }
                        all
                    }
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("toString", "()Ljava/lang/String;") => {
            let media = get(vm, recv, "mMediaSize");
            let resolution = get(vm, recv, "mResolution");
            let margins = get(vm, recv, "mMinMargins");
            let media_s = value_string(vm, &media)?;
            let orientation = match obj(Some(&media)) {
                Some(m) => {
                    if field_int(vm, m, "mHeightMils") >= field_int(vm, m, "mWidthMils") {
                        "portrait"
                    } else {
                        "landscape"
                    }
                }
                None => "null",
            };
            let resolution_s = value_string(vm, &resolution)?;
            let margins_s = value_string(vm, &margins)?;
            let color = match field_int(vm, recv, "mColorMode") {
                1 => "COLOR_MODE_MONOCHROME",
                2 => "COLOR_MODE_COLOR",
                _ => "COLOR_MODE_UNKNOWN",
            };
            let duplex = match field_int(vm, recv, "mDuplexMode") {
                1 => "DUPLEX_MODE_NONE",
                2 => "DUPLEX_MODE_LONG_EDGE",
                4 => "DUPLEX_MODE_SHORT_EDGE",
                _ => "DUPLEX_MODE_UNKNOWN",
            };
            jstr(
                vm,
                format!(
                    "PrintAttributes{{mediaSize: {media_s}, orientation: {orientation}, resolution: {resolution_s}, minMargins: {margins_s}, colorMode: {color}, duplexMode: {duplex}}}"
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_print_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Builder delegates directly to its PrintAttributes instance. Store the
    // attributes fields on the builder and materialize the same field set.
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        (
            "setMediaSize",
            "(Landroid/print/PrintAttributes$MediaSize;)Landroid/print/PrintAttributes$Builder;",
        ) => {
            let Some(_) = oarg(args, 0) else {
                return Err(npe("mediaSize"));
            };
            put(vm, recv, "mMediaSize", args[0].clone())?;
            Value::Obj(recv)
        }
        (
            "setResolution",
            "(Landroid/print/PrintAttributes$Resolution;)Landroid/print/PrintAttributes$Builder;",
        ) => {
            let Some(_) = oarg(args, 0) else {
                return Err(npe("resolution"));
            };
            put(vm, recv, "mResolution", args[0].clone())?;
            Value::Obj(recv)
        }
        (
            "setMinMargins",
            "(Landroid/print/PrintAttributes$Margins;)Landroid/print/PrintAttributes$Builder;",
        ) => {
            let Some(_) = oarg(args, 0) else {
                return Err(npe("margins"));
            };
            put(vm, recv, "mMinMargins", args[0].clone())?;
            Value::Obj(recv)
        }
        ("setColorMode", "(I)Landroid/print/PrintAttributes$Builder;") => {
            let mode = iarg(args, 0);
            if !p57_valid_single_flag(mode, 3) {
                return Err(iae(format!("invalid color mode: {mode}")));
            }
            put(vm, recv, "mColorMode", Value::Int(mode))?;
            Value::Obj(recv)
        }
        ("setDuplexMode", "(I)Landroid/print/PrintAttributes$Builder;") => {
            let mode = iarg(args, 0);
            if !p57_valid_single_flag(mode, 7) {
                return Err(iae(format!("invalid duplex mode: {mode}")));
            }
            put(vm, recv, "mDuplexMode", Value::Int(mode))?;
            Value::Obj(recv)
        }
        ("build", "()Landroid/print/PrintAttributes;") => {
            if let Some(existing) = field_obj(vm, recv, "rd_attributes") {
                p57_print_attributes_copy(vm, existing, recv)?;
                Value::Obj(existing)
            } else {
                let out = alloc_obj(vm, P57_PRINT_ATTRIBUTES, Vec::new())?;
                p57_print_attributes_copy(vm, out, recv)?;
                put(vm, recv, "rd_attributes", Value::Obj(out))?;
                Value::Obj(out)
            }
        }
        _ => return Ok(None),
    }))
}

fn p57_page_range_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(II)V") => {
            let start = iarg(args, 0);
            let end = iarg(args, 1);
            if start < 0 {
                return Err(iae("start cannot be less than zero."));
            }
            if end < 0 {
                return Err(iae("end cannot be less than zero."));
            }
            if start > end {
                return Err(iae("start must be lesser than end."));
            }
            put(vm, recv, "mStart", Value::Int(start))?;
            put(vm, recv, "mEnd", Value::Int(end))?;
            Value::Null
        }
        ("getStart", "()I") => Value::Int(field_int(vm, recv, "mStart")),
        ("getEnd", "()I") => Value::Int(field_int(vm, recv, "mEnd")),
        ("contains", "(I)Z") => bool_value(
            iarg(args, 0) >= field_int(vm, recv, "mStart")
                && iarg(args, 0) <= field_int(vm, recv, "mEnd"),
        ),
        ("getSize", "()I") => Value::Int(
            field_int(vm, recv, "mEnd")
                .wrapping_sub(field_int(vm, recv, "mStart"))
                .wrapping_add(1),
        ),
        ("hashCode", "()I") => {
            let mut hash = 1i32;
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mEnd"));
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(field_int(vm, recv, "mStart"));
            Value::Int(hash)
        }
        ("equals", "(Ljava/lang/Object;)Z") => bool_value(matches!(
            oarg(args, 0),
            Some(other)
                if class_of(vm, other) == P57_PAGE_RANGE
                    && field_int(vm, recv, "mStart") == field_int(vm, other, "mStart")
                    && field_int(vm, recv, "mEnd") == field_int(vm, other, "mEnd")
        )),
        ("toString", "()Ljava/lang/String;") => {
            let start = field_int(vm, recv, "mStart");
            let end = field_int(vm, recv, "mEnd");
            jstr(
                vm,
                if start == 0 && end == i32::MAX {
                    "PageRange[<all pages>]".into()
                } else {
                    format!("PageRange[{start} - {end}]")
                },
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_component_equal(vm: &mut Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    match (obj(Some(a)), obj(Some(b))) {
        (Some(a), Some(b)) => {
            if a == b {
                Ok(true)
            } else {
                Ok(
                    vm.invoke_instance(a, "equals", "(Ljava/lang/Object;)Z", &[Value::Obj(b)])?
                        == Value::Int(1),
                )
            }
        }
        _ => Ok(matches!((a, b), (Value::Null, Value::Null))),
    }
}

fn p57_printer_id_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/ComponentName;Ljava/lang/String;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("serviceName"));
            }
            if oarg(args, 1).is_none() {
                return Err(npe("localId"));
            }
            put(vm, recv, "mServiceName", args[0].clone())?;
            put(vm, recv, "mLocalId", args[1].clone())?;
            Value::Null
        }
        ("getServiceName", "()Landroid/content/ComponentName;") => get(vm, recv, "mServiceName"),
        ("getLocalId", "()Ljava/lang/String;") => get(vm, recv, "mLocalId"),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if class_of(vm, other) == P57_PRINTER_ID => {
                    let a = get(vm, recv, "mServiceName");
                    let b = get(vm, other, "mServiceName");
                    p57_component_equal(vm, &a, &b)?
                        && field_str(vm, recv, "mLocalId") == field_str(vm, other, "mLocalId")
                }
                _ => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => {
            let service = get(vm, recv, "mServiceName");
            let service_hash = p57_print_value_hash(vm, &service)?;
            let local_hash = field_str(vm, recv, "mLocalId")
                .map(|s| java_string_hash(&s))
                .unwrap_or(0);
            let mut hash = 1i32;
            hash = hash.wrapping_mul(31).wrapping_add(service_hash);
            hash = hash.wrapping_mul(31).wrapping_add(local_hash);
            Value::Int(hash)
        }
        ("toString", "()Ljava/lang/String;") => {
            let service = get(vm, recv, "mServiceName");
            let flattened = match obj(Some(&service)) {
                Some(r) => {
                    let v =
                        vm.invoke_instance(r, "flattenToString", "()Ljava/lang/String;", &[])?;
                    opt_str(vm, Some(&v)).unwrap_or_else(|| "null".into())
                }
                None => "null".into(),
            };
            jstr(
                vm,
                format!(
                    "PrinterId{{serviceName={flattened}, localId={}}}",
                    field_str(vm, recv, "mLocalId").unwrap_or_else(|| "null".into())
                ),
            )?
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn p57_print_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // The print feature/service is absent, matching PrintManager's null
    // IPrintManager branches.
    let empty_list = |vm: &mut Engine| -> Result<Value, VmExit> {
        Ok(Value::Obj(new_collection(
            vm,
            "Ljava/util/ArrayList;",
            Vec::new(),
        )?))
    };
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/content/Context;Landroid/print/IPrintManager;II)V") => {
            let context = args.first().cloned().unwrap_or(Value::Null);
            put(vm, recv, "mContext", context)?;
            put(vm, recv, "mUserId", Value::Int(iarg(args, 2)))?;
            put(vm, recv, "mAppId", Value::Int(iarg(args, 3)))?;
            Value::Null
        }
        ("getGlobalPrintManagerForUser", "(I)Landroid/print/PrintManager;") => Value::Null,
        ("addPrintJobStateChangeListener", "(Landroid/print/PrintManager$PrintJobStateChangeListener;)V")
        | ("removePrintJobStateChangeListener", "(Landroid/print/PrintManager$PrintJobStateChangeListener;)V") => Value::Null,
        ("getPrintJob", "(Landroid/print/PrintJobId;)Landroid/print/PrintJob;") => Value::Null,
        ("getCustomPrinterIcon", "(Landroid/print/PrinterId;)Landroid/graphics/drawable/Icon;") => Value::Null,
        ("getPrintJobs", "()Ljava/util/List;") => empty_list(vm)?,
        ("print", "(Ljava/lang/String;Landroid/print/PrintDocumentAdapter;Landroid/print/PrintAttributes;)Landroid/print/PrintJob;") => {
            // AOSP checks the null service before validating Activity/name.
            Value::Null
        }
        ("addPrintServicesChangeListener", "(Landroid/print/PrintManager$PrintServicesChangeListener;Landroid/os/Handler;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("listener"));
            }
            Value::Null
        }
        ("removePrintServicesChangeListener", "(Landroid/print/PrintManager$PrintServicesChangeListener;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("listener"));
            }
            Value::Null
        }
        ("getPrintServices", "(I)Ljava/util/List;") => {
            let flags = iarg(args, 0);
            if flags & !3 != 0 {
                return Err(iae(format!(
                    "selectionFlags contains invalid bits: {flags}"
                )));
            }
            empty_list(vm)?
        }
        ("addPrintServiceRecommendationsChangeListener", "(Landroid/print/PrintManager$PrintServiceRecommendationsChangeListener;Landroid/os/Handler;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("listener"));
            }
            Value::Null
        }
        ("removePrintServiceRecommendationsChangeListener", "(Landroid/print/PrintManager$PrintServiceRecommendationsChangeListener;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("listener"));
            }
            Value::Null
        }
        ("getPrintServiceRecommendations", "()Ljava/util/List;") => empty_list(vm)?,
        ("createPrinterDiscoverySession", "()Landroid/print/PrinterDiscoverySession;") => {
            Value::Null
        }
        ("setPrintServiceEnabled", "(Landroid/content/ComponentName;Z)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("service"));
            }
            Value::Null
        }
        ("isPrintServiceEnabled", "(Landroid/content/ComponentName;)Z") => {
            if oarg(args, 0).is_none() {
                return Err(npe("service"));
            }
            Value::Int(0)
        }
        _ => return Ok(None),
    }))
}

// --- android.security.keystore --------------------------------------------

fn p57_clone_string_array_or_null(vm: &mut Engine, value: &Value) -> Result<Value, VmExit> {
    if obj(Some(value)).is_none() {
        return Ok(Value::Null);
    }
    let values = p57_string_values(vm, value);
    p57_new_string_array(vm, &values)
}

fn p57_clone_string_array_or_empty(vm: &mut Engine, value: &Value) -> Result<Value, VmExit> {
    if obj(Some(value)).is_none() {
        return obj_array(vm, "[Ljava/lang/String;", Vec::new());
    }
    p57_clone_string_array_or_null(vm, value)
}

fn p57_clone_byte_array_or_null(vm: &mut Engine, value: &Value) -> Result<Value, VmExit> {
    if obj(Some(value)).is_none() {
        return Ok(Value::Null);
    }
    p57_clone_array(vm, value, "[B", ElemKind::Int)
}

fn p57_new_x500_principal(vm: &mut Engine, name: &str) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Ljavax/security/auth/x500/X500Principal;", Vec::new())?;
    let s = jstr(vm, name)?;
    // No complete X.500 name parser is part of this key-parameters slice;
    // retain the exact RFC 1779 input for host stringification paths.
    put(vm, r, "rd_name", s.clone())?;
    put(vm, r, "name", s)?;
    Ok(Value::Obj(r))
}

fn p57_new_big_integer_one(vm: &mut Engine) -> Result<Value, VmExit> {
    let r = alloc_obj(vm, "Ljava/math/BigInteger;", Vec::new())?;
    let s = jstr(vm, "1")?;
    vm.invoke_instance(r, "<init>", "(Ljava/lang/String;)V", &[s])?;
    Ok(Value::Obj(r))
}

const P57_KEYGEN_FIELDS: &[&str] = &[
    "mKeystoreAlias",
    "mNamespace",
    "mKeySize",
    "mSpec",
    "mCertificateSubject",
    "mCertificateSerialNumber",
    "mCertificateNotBefore",
    "mCertificateNotAfter",
    "mKeyValidityStart",
    "mKeyValidityForOriginationEnd",
    "mKeyValidityForConsumptionEnd",
    "mPurposes",
    "mDigests",
    "mEncryptionPaddings",
    "mSignaturePaddings",
    "mBlockModes",
    "mRandomizedEncryptionRequired",
    "mUserAuthenticationRequired",
    "mUserAuthenticationValidityDurationSeconds",
    "mUserAuthenticationType",
    "mUserPresenceRequired",
    "mAttestationChallenge",
    "mDevicePropertiesAttestationIncluded",
    "mAttestationIds",
    "mUniqueIdIncluded",
    "mUserAuthenticationValidWhileOnBody",
    "mInvalidatedByBiometricEnrollment",
    "mIsStrongBoxBacked",
    "mUserConfirmationRequired",
    "mUnlockedDeviceRequired",
    "mCriticalToDeviceEncryption",
    "mMaxUsageCount",
    "mAttestKeyAlias",
    "mBoundToSecureUserId",
];

fn p57_keygen_defaults(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    put(vm, recv, "mNamespace", Value::Int(-1))?;
    put(vm, recv, "mKeySize", Value::Int(-1))?;
    put(vm, recv, "mRandomizedEncryptionRequired", Value::Int(1))?;
    put(
        vm,
        recv,
        "mUserAuthenticationValidityDurationSeconds",
        Value::Int(0),
    )?;
    put(vm, recv, "mUserAuthenticationType", Value::Int(2))?;
    let ids = crate::arrays::new_array(vm, "[I", ElemKind::Int, Vec::new())?;
    put(vm, recv, "mAttestationIds", ids)?;
    put(vm, recv, "mInvalidatedByBiometricEnrollment", Value::Int(1))?;
    put(vm, recv, "mMaxUsageCount", Value::Int(-1))?;
    put(vm, recv, "mBoundToSecureUserId", Value::Long(0))?;
    Ok(())
}

fn p57_keygen_copy(
    vm: &mut Engine,
    recv: ObjRef,
    other: ObjRef,
    defensive: bool,
) -> Result<(), VmExit> {
    for f in P57_KEYGEN_FIELDS {
        let mut v = get(vm, other, f);
        if defensive {
            v = match *f {
                "mCertificateNotBefore"
                | "mCertificateNotAfter"
                | "mKeyValidityStart"
                | "mKeyValidityForOriginationEnd"
                | "mKeyValidityForConsumptionEnd" => p57_clone_date(vm, &v)?,
                "mDigests" => p57_clone_string_array_or_null(vm, &v)?,
                "mEncryptionPaddings" | "mSignaturePaddings" | "mBlockModes" => {
                    p57_clone_string_array_or_empty(vm, &v)?
                }
                "mAttestationChallenge" => p57_clone_byte_array_or_null(vm, &v)?,
                "mAttestationIds" => {
                    if obj(Some(&v)).is_none() {
                        crate::arrays::new_array(vm, "[I", ElemKind::Int, Vec::new())?
                    } else {
                        p57_clone_array(vm, &v, "[I", ElemKind::Int)?
                    }
                }
                _ => v,
            };
        }
        put(vm, recv, f, v)?;
    }
    Ok(())
}

fn p57_keygen_finish(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    let alias = field_str(vm, recv, "mKeystoreAlias");
    if alias.as_deref().map(str::is_empty).unwrap_or(true) {
        return Err(iae("keyStoreAlias must not be empty"));
    }
    if field_obj(vm, recv, "mCertificateSubject").is_none() {
        let subject = if field_obj(vm, recv, "mAttestationChallenge").is_none() {
            p57_new_x500_principal(vm, "CN=Fake")?
        } else {
            p57_new_x500_principal(vm, "CN=Android Keystore Key")?
        };
        put(vm, recv, "mCertificateSubject", subject)?;
    }
    if field_obj(vm, recv, "mCertificateSerialNumber").is_none() {
        let one = p57_new_big_integer_one(vm)?;
        put(vm, recv, "mCertificateSerialNumber", one)?;
    }
    if field_obj(vm, recv, "mCertificateNotBefore").is_none() {
        let d = p57_new_date(vm, 0)?;
        put(vm, recv, "mCertificateNotBefore", d)?;
    }
    if field_obj(vm, recv, "mCertificateNotAfter").is_none() {
        let d = p57_new_date(vm, 2_461_449_600_000)?;
        put(vm, recv, "mCertificateNotAfter", d)?;
    }
    let before_value = get(vm, recv, "mCertificateNotBefore");
    let before = p57_date_ms(vm, &before_value).unwrap_or(0);
    let after_value = get(vm, recv, "mCertificateNotAfter");
    let after = p57_date_ms(vm, &after_value).unwrap_or(0);
    if after < before {
        return Err(iae("certificateNotAfter < certificateNotBefore"));
    }
    for f in [
        "mCertificateNotBefore",
        "mCertificateNotAfter",
        "mKeyValidityStart",
        "mKeyValidityForOriginationEnd",
        "mKeyValidityForConsumptionEnd",
    ] {
        let v = get(vm, recv, f);
        let cloned = p57_clone_date(vm, &v)?;
        put(vm, recv, f, cloned)?;
    }
    let digests = get(vm, recv, "mDigests");
    let digests = p57_clone_string_array_or_null(vm, &digests)?;
    put(vm, recv, "mDigests", digests)?;
    for f in ["mEncryptionPaddings", "mSignaturePaddings", "mBlockModes"] {
        let v = get(vm, recv, f);
        let cloned = p57_clone_string_array_or_empty(vm, &v)?;
        put(vm, recv, f, cloned)?;
    }
    let challenge = get(vm, recv, "mAttestationChallenge");
    let challenge = p57_clone_byte_array_or_null(vm, &challenge)?;
    put(vm, recv, "mAttestationChallenge", challenge)?;
    if field_obj(vm, recv, "mAttestationIds").is_none() {
        let ids = crate::arrays::new_array(vm, "[I", ElemKind::Int, Vec::new())?;
        put(vm, recv, "mAttestationIds", ids)?;
    }
    Ok(())
}

fn p57_keygen_get_digests(vm: &mut Engine, recv: ObjRef) -> Result<Value, VmExit> {
    let v = get(vm, recv, "mDigests");
    if obj(Some(&v)).is_none() {
        return Err(ise("Digests not specified"));
    }
    p57_clone_string_array_or_null(vm, &v)
}

fn p57_keygen_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == P57_KEY_GEN_SPEC {
        return Ok(Some(match (name, sig) {
            ("<init>", "(Ljava/lang/String;IILjava/security/spec/AlgorithmParameterSpec;Ljavax/security/auth/x500/X500Principal;Ljava/math/BigInteger;Ljava/util/Date;Ljava/util/Date;Ljava/util/Date;Ljava/util/Date;Ljava/util/Date;I[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;ZZIIZ[BZ[IZZZZZZZILjava/lang/String;J)V") => {
                // AOSP's hidden canonical constructor uses the same field order
                // as P57_KEYGEN_FIELDS.
                for (i, f) in P57_KEYGEN_FIELDS.iter().enumerate() {
                    let v = args.get(i).cloned().unwrap_or(Value::Null);
                    put(vm, recv, f, v)?;
                }
                p57_keygen_finish(vm, recv)?;
                Value::Null
            }
            ("getKeystoreAlias", "()Ljava/lang/String;") => get(vm, recv, "mKeystoreAlias"),
            ("getUid", "()I") => match field_int(vm, recv, "mNamespace") {
                -1 => Value::Int(-1),
                102 => Value::Int(1010),
                ns => {
                    return Err(iae(format!(
                        "No UID corresponding to namespace {ns}"
                    )))
                }
            },
            ("getNamespace", "()I") => Value::Int(field_int(vm, recv, "mNamespace")),
            ("getKeySize", "()I") => Value::Int(field_int(vm, recv, "mKeySize")),
            ("getAlgorithmParameterSpec", "()Ljava/security/spec/AlgorithmParameterSpec;") => {
                get(vm, recv, "mSpec")
            }
            ("getCertificateSubject", "()Ljavax/security/auth/x500/X500Principal;") => {
                get(vm, recv, "mCertificateSubject")
            }
            ("getCertificateSerialNumber", "()Ljava/math/BigInteger;") => {
                get(vm, recv, "mCertificateSerialNumber")
            }
            ("getCertificateNotBefore", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mCertificateNotBefore");
                p57_clone_date(vm, &v)?
            }
            ("getCertificateNotAfter", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mCertificateNotAfter");
                p57_clone_date(vm, &v)?
            }
            ("getKeyValidityStart", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityStart");
                p57_clone_date(vm, &v)?
            }
            ("getKeyValidityForConsumptionEnd", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityForConsumptionEnd");
                p57_clone_date(vm, &v)?
            }
            ("getKeyValidityForOriginationEnd", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityForOriginationEnd");
                p57_clone_date(vm, &v)?
            }
            ("getPurposes", "()I") => Value::Int(field_int(vm, recv, "mPurposes")),
            ("getDigests", "()[Ljava/lang/String;") => p57_keygen_get_digests(vm, recv)?,
            ("isDigestsSpecified", "()Z") => {
                bool_value(field_obj(vm, recv, "mDigests").is_some())
            }
            ("getEncryptionPaddings", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mEncryptionPaddings");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("getSignaturePaddings", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mSignaturePaddings");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("getBlockModes", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mBlockModes");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("isRandomizedEncryptionRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mRandomizedEncryptionRequired"))
            }
            ("isUserAuthenticationRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserAuthenticationRequired"))
            }
            ("isUserConfirmationRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserConfirmationRequired"))
            }
            ("getUserAuthenticationValidityDurationSeconds", "()I") => Value::Int(field_int(
                vm,
                recv,
                "mUserAuthenticationValidityDurationSeconds",
            )),
            ("getUserAuthenticationType", "()I") => {
                Value::Int(field_int(vm, recv, "mUserAuthenticationType"))
            }
            ("isUserPresenceRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserPresenceRequired"))
            }
            ("getAttestationChallenge", "()[B") => {
                let v = get(vm, recv, "mAttestationChallenge");
                p57_clone_byte_array_or_null(vm, &v)?
            }
            ("isDevicePropertiesAttestationIncluded", "()Z") => bool_value(field_bool(
                vm,
                recv,
                "mDevicePropertiesAttestationIncluded",
            )),
            ("getAttestationIds", "()[I") => {
                let v = get(vm, recv, "mAttestationIds");
                if obj(Some(&v)).is_none() {
                    return Err(npe("mAttestationIds"));
                }
                p57_clone_array(vm, &v, "[I", ElemKind::Int)?
            }
            ("isUniqueIdIncluded", "()Z") => {
                bool_value(field_bool(vm, recv, "mUniqueIdIncluded"))
            }
            ("isUserAuthenticationValidWhileOnBody", "()Z") => bool_value(field_bool(
                vm,
                recv,
                "mUserAuthenticationValidWhileOnBody",
            )),
            ("isInvalidatedByBiometricEnrollment", "()Z") => bool_value(field_bool(
                vm,
                recv,
                "mInvalidatedByBiometricEnrollment",
            )),
            ("isStrongBoxBacked", "()Z") => {
                bool_value(field_bool(vm, recv, "mIsStrongBoxBacked"))
            }
            ("isUnlockedDeviceRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUnlockedDeviceRequired"))
            }
            ("getBoundToSpecificSecureUserId", "()J") => {
                Value::Long(field_long(vm, recv, "mBoundToSecureUserId"))
            }
            ("isCriticalToDeviceEncryption", "()Z") => {
                bool_value(field_bool(vm, recv, "mCriticalToDeviceEncryption"))
            }
            ("getMaxUsageCount", "()I") => {
                Value::Int(field_int(vm, recv, "mMaxUsageCount"))
            }
            ("getAttestKeyAlias", "()Ljava/lang/String;") => get(vm, recv, "mAttestKeyAlias"),
            _ => return Ok(None),
        }));
    }

    // KeyGenParameterSpec.Builder.
    let self_value = Value::Obj(recv);
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;I)V") => {
            let Some(alias) = opt_str_arg(vm, args, 0) else {
                return Err(npe("keystoreAlias == null"));
            };
            if alias.is_empty() {
                return Err(iae("keystoreAlias must not be empty"));
            }
            p57_keygen_defaults(vm, recv)?;
            put(vm, recv, "mKeystoreAlias", args[0].clone())?;
            put(vm, recv, "mPurposes", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("<init>", "(Landroid/security/keystore/KeyGenParameterSpec;)V") => {
            let Some(source) = oarg(args, 0) else {
                return Err(npe("sourceSpec"));
            };
            p57_keygen_copy(vm, recv, source, true)?;
            Value::Null
        }
        ("setUid", "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let ns = match iarg(args, 0) {
                -1 => -1,
                1010 => 102,
                uid => {
                    return Err(iae(format!(
                        "No namespace corresponding to uid {uid}"
                    )))
                }
            };
            put(vm, recv, "mNamespace", Value::Int(ns))?;
            self_value
        }
        ("setNamespace", "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mNamespace", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setKeySize", "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if iarg(args, 0) < 0 {
                return Err(iae("keySize < 0"));
            }
            put(vm, recv, "mKeySize", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setAlgorithmParameterSpec", "(Ljava/security/spec/AlgorithmParameterSpec;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("spec == null"));
            }
            put(vm, recv, "mSpec", args[0].clone())?;
            self_value
        }
        ("setCertificateSubject", "(Ljavax/security/auth/x500/X500Principal;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("subject == null"));
            }
            put(vm, recv, "mCertificateSubject", args[0].clone())?;
            self_value
        }
        ("setCertificateSerialNumber", "(Ljava/math/BigInteger;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("serialNumber == null"));
            }
            put(vm, recv, "mCertificateSerialNumber", args[0].clone())?;
            self_value
        }
        ("setCertificateNotBefore", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("date == null"));
            }
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mCertificateNotBefore", v)?;
            self_value
        }
        ("setCertificateNotAfter", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("date == null"));
            }
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mCertificateNotAfter", v)?;
            self_value
        }
        ("setKeyValidityStart", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityStart", v)?;
            self_value
        }
        ("setKeyValidityEnd", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let a = p57_clone_date(vm, &args[0])?;
            let b = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForOriginationEnd", a)?;
            put(vm, recv, "mKeyValidityForConsumptionEnd", b)?;
            self_value
        }
        ("setKeyValidityForOriginationEnd", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForOriginationEnd", v)?;
            self_value
        }
        ("setKeyValidityForConsumptionEnd", "(Ljava/util/Date;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForConsumptionEnd", v)?;
            self_value
        }
        ("setDigests", "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_string_array_or_null(vm, &args[0])?;
            put(vm, recv, "mDigests", v)?;
            self_value
        }
        ("setEncryptionPaddings", "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mEncryptionPaddings", v)?;
            self_value
        }
        ("setSignaturePaddings", "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mSignaturePaddings", v)?;
            self_value
        }
        ("setBlockModes", "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mBlockModes", v)?;
            self_value
        }
        ("setRandomizedEncryptionRequired", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mRandomizedEncryptionRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setUserAuthenticationRequired", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUserAuthenticationRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setUserConfirmationRequired", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUserConfirmationRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setUserAuthenticationValidityDurationSeconds", "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let seconds = iarg(args, 0);
            if seconds < -1 {
                return Err(iae("seconds must be -1 or larger"));
            }
            if seconds == -1 {
                put(vm, recv, "mUserAuthenticationValidityDurationSeconds", Value::Int(0))?;
                put(vm, recv, "mUserAuthenticationType", Value::Int(2))?;
            } else {
                put(vm, recv, "mUserAuthenticationValidityDurationSeconds", Value::Int(seconds))?;
                put(vm, recv, "mUserAuthenticationType", Value::Int(1 | 2))?;
            }
            self_value
        }
        ("setUserAuthenticationParameters", "(II)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            if iarg(args, 0) < 0 {
                return Err(iae("timeout must be 0 or larger"));
            }
            put(vm, recv, "mUserAuthenticationValidityDurationSeconds", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mUserAuthenticationType", Value::Int(iarg(args, 1)))?;
            self_value
        }
        ("setUserPresenceRequired", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUserPresenceRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setAttestationChallenge", "([B)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            // AOSP Builder stores the reference; the spec constructor clones.
            put(vm, recv, "mAttestationChallenge", args[0].clone())?;
            self_value
        }
        ("setDevicePropertiesAttestationIncluded", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mDevicePropertiesAttestationIncluded", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setAttestationIds", "([I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mAttestationIds", args[0].clone())?;
            self_value
        }
        ("setUniqueIdIncluded", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUniqueIdIncluded", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setUserAuthenticationValidWhileOnBody", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUserAuthenticationValidWhileOnBody", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setInvalidatedByBiometricEnrollment", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mInvalidatedByBiometricEnrollment", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setIsStrongBoxBacked", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mIsStrongBoxBacked", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setUnlockedDeviceRequired", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mUnlockedDeviceRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setCriticalToDeviceEncryption", "(Z)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mCriticalToDeviceEncryption", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setMaxUsageCount", "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            let count = iarg(args, 0);
            if count == -1 || count > 0 {
                put(vm, recv, "mMaxUsageCount", Value::Int(count))?;
                self_value
            } else {
                return Err(iae(
                    "maxUsageCount must be either UNRESTRICTED_USAGE_COUNT or a positive value",
                ));
            }
        }
        ("setAttestKeyAlias", "(Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mAttestKeyAlias", args[0].clone())?;
            self_value
        }
        ("setBoundToSpecificSecureUserId", "(J)Landroid/security/keystore/KeyGenParameterSpec$Builder;") => {
            put(vm, recv, "mBoundToSecureUserId", Value::Long(larg(args, 0)))?;
            self_value
        }
        ("build", "()Landroid/security/keystore/KeyGenParameterSpec;") => {
            let out = alloc_obj(vm, P57_KEY_GEN_SPEC, Vec::new())?;
            p57_keygen_copy(vm, out, recv, false)?;
            p57_keygen_finish(vm, out)?;
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

const P57_KEY_PROTECTION_FIELDS: &[&str] = &[
    "mKeyValidityStart",
    "mKeyValidityForOriginationEnd",
    "mKeyValidityForConsumptionEnd",
    "mPurposes",
    "mEncryptionPaddings",
    "mSignaturePaddings",
    "mDigests",
    "mBlockModes",
    "mRandomizedEncryptionRequired",
    "mUserAuthenticationRequired",
    "mUserAuthenticationType",
    "mUserAuthenticationValidityDurationSeconds",
    "mUserPresenceRequired",
    "mUserAuthenticationValidWhileOnBody",
    "mInvalidatedByBiometricEnrollment",
    "mBoundToSecureUserId",
    "mCriticalToDeviceEncryption",
    "mUserConfirmationRequired",
    "mUnlockedDeviceRequired",
    "mIsStrongBoxBacked",
    "mMaxUsageCount",
    "mRollbackResistant",
];

fn p57_key_protection_defaults(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    put(vm, recv, "mRandomizedEncryptionRequired", Value::Int(1))?;
    put(
        vm,
        recv,
        "mUserAuthenticationValidityDurationSeconds",
        Value::Int(0),
    )?;
    put(vm, recv, "mUserAuthenticationType", Value::Int(2))?;
    put(vm, recv, "mInvalidatedByBiometricEnrollment", Value::Int(1))?;
    put(vm, recv, "mBoundToSecureUserId", Value::Long(0))?;
    put(vm, recv, "mMaxUsageCount", Value::Int(-1))?;
    Ok(())
}

fn p57_key_protection_finish(vm: &mut Engine, recv: ObjRef, other: ObjRef) -> Result<(), VmExit> {
    for f in P57_KEY_PROTECTION_FIELDS {
        let mut v = get(vm, other, f);
        v = match *f {
            "mKeyValidityStart"
            | "mKeyValidityForOriginationEnd"
            | "mKeyValidityForConsumptionEnd" => p57_clone_date(vm, &v)?,
            "mDigests" => p57_clone_string_array_or_null(vm, &v)?,
            "mEncryptionPaddings" | "mSignaturePaddings" | "mBlockModes" => {
                p57_clone_string_array_or_empty(vm, &v)?
            }
            _ => v,
        };
        put(vm, recv, f, v)?;
    }
    Ok(())
}

fn p57_key_protection_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == P57_KEY_PROTECTION {
        return Ok(Some(match (name, sig) {
            ("getKeyValidityStart", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityStart");
                p57_clone_date(vm, &v)?
            }
            ("getKeyValidityForConsumptionEnd", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityForConsumptionEnd");
                p57_clone_date(vm, &v)?
            }
            ("getKeyValidityForOriginationEnd", "()Ljava/util/Date;") => {
                let v = get(vm, recv, "mKeyValidityForOriginationEnd");
                p57_clone_date(vm, &v)?
            }
            ("getPurposes", "()I") => Value::Int(field_int(vm, recv, "mPurposes")),
            ("getEncryptionPaddings", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mEncryptionPaddings");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("getSignaturePaddings", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mSignaturePaddings");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("getDigests", "()[Ljava/lang/String;") => p57_keygen_get_digests(vm, recv)?,
            ("isDigestsSpecified", "()Z") => bool_value(field_obj(vm, recv, "mDigests").is_some()),
            ("getBlockModes", "()[Ljava/lang/String;") => {
                let v = get(vm, recv, "mBlockModes");
                p57_clone_string_array_or_empty(vm, &v)?
            }
            ("isRandomizedEncryptionRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mRandomizedEncryptionRequired"))
            }
            ("isUserAuthenticationRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserAuthenticationRequired"))
            }
            ("isUserConfirmationRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserConfirmationRequired"))
            }
            ("getUserAuthenticationType", "()I") => {
                Value::Int(field_int(vm, recv, "mUserAuthenticationType"))
            }
            ("getUserAuthenticationValidityDurationSeconds", "()I") => Value::Int(field_int(
                vm,
                recv,
                "mUserAuthenticationValidityDurationSeconds",
            )),
            ("isUserPresenceRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserPresenceRequired"))
            }
            ("isUserAuthenticationValidWhileOnBody", "()Z") => {
                bool_value(field_bool(vm, recv, "mUserAuthenticationValidWhileOnBody"))
            }
            ("isInvalidatedByBiometricEnrollment", "()Z") => {
                bool_value(field_bool(vm, recv, "mInvalidatedByBiometricEnrollment"))
            }
            ("getBoundToSpecificSecureUserId", "()J") => {
                Value::Long(field_long(vm, recv, "mBoundToSecureUserId"))
            }
            ("isCriticalToDeviceEncryption", "()Z") => {
                bool_value(field_bool(vm, recv, "mCriticalToDeviceEncryption"))
            }
            ("isUnlockedDeviceRequired", "()Z") => {
                bool_value(field_bool(vm, recv, "mUnlockedDeviceRequired"))
            }
            ("isStrongBoxBacked", "()Z") => bool_value(field_bool(vm, recv, "mIsStrongBoxBacked")),
            ("getMaxUsageCount", "()I") => Value::Int(field_int(vm, recv, "mMaxUsageCount")),
            ("isRollbackResistant", "()Z") => {
                bool_value(field_bool(vm, recv, "mRollbackResistant"))
            }
            _ => return Ok(None),
        }));
    }

    let self_value = Value::Obj(recv);
    Ok(Some(match (name, sig) {
        ("<init>", "(I)V") => {
            p57_key_protection_defaults(vm, recv)?;
            put(vm, recv, "mPurposes", Value::Int(iarg(args, 0)))?;
            Value::Null
        }
        (
            "setKeyValidityStart",
            "(Ljava/util/Date;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityStart", v)?;
            self_value
        }
        (
            "setKeyValidityEnd",
            "(Ljava/util/Date;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let a = p57_clone_date(vm, &args[0])?;
            let b = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForOriginationEnd", a)?;
            put(vm, recv, "mKeyValidityForConsumptionEnd", b)?;
            self_value
        }
        (
            "setKeyValidityForOriginationEnd",
            "(Ljava/util/Date;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForOriginationEnd", v)?;
            self_value
        }
        (
            "setKeyValidityForConsumptionEnd",
            "(Ljava/util/Date;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_date(vm, &args[0])?;
            put(vm, recv, "mKeyValidityForConsumptionEnd", v)?;
            self_value
        }
        (
            "setEncryptionPaddings",
            "([Ljava/lang/String;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mEncryptionPaddings", v)?;
            self_value
        }
        (
            "setSignaturePaddings",
            "([Ljava/lang/String;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mSignaturePaddings", v)?;
            self_value
        }
        (
            "setDigests",
            "([Ljava/lang/String;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_string_array_or_null(vm, &args[0])?;
            put(vm, recv, "mDigests", v)?;
            self_value
        }
        (
            "setBlockModes",
            "([Ljava/lang/String;)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let v = p57_clone_string_array_or_empty(vm, &args[0])?;
            put(vm, recv, "mBlockModes", v)?;
            self_value
        }
        (
            "setRandomizedEncryptionRequired",
            "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(
                vm,
                recv,
                "mRandomizedEncryptionRequired",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        (
            "setUserAuthenticationRequired",
            "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(
                vm,
                recv,
                "mUserAuthenticationRequired",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        ("setUserConfirmationRequired", "(Z)Landroid/security/keystore/KeyProtection$Builder;") => {
            put(
                vm,
                recv,
                "mUserConfirmationRequired",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        (
            "setUserAuthenticationValidityDurationSeconds",
            "(I)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            let seconds = iarg(args, 0);
            if seconds < -1 {
                return Err(iae("seconds must be -1 or larger"));
            }
            if seconds == -1 {
                put(
                    vm,
                    recv,
                    "mUserAuthenticationValidityDurationSeconds",
                    Value::Int(0),
                )?;
                put(vm, recv, "mUserAuthenticationType", Value::Int(2))?;
            } else {
                put(
                    vm,
                    recv,
                    "mUserAuthenticationValidityDurationSeconds",
                    Value::Int(seconds),
                )?;
                put(vm, recv, "mUserAuthenticationType", Value::Int(3))?;
            }
            self_value
        }
        (
            "setUserAuthenticationParameters",
            "(II)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            if iarg(args, 0) < 0 {
                return Err(iae("timeout must be 0 or larger"));
            }
            put(
                vm,
                recv,
                "mUserAuthenticationValidityDurationSeconds",
                Value::Int(iarg(args, 0)),
            )?;
            put(
                vm,
                recv,
                "mUserAuthenticationType",
                Value::Int(iarg(args, 1)),
            )?;
            self_value
        }
        ("setUserPresenceRequired", "(Z)Landroid/security/keystore/KeyProtection$Builder;") => {
            put(vm, recv, "mUserPresenceRequired", Value::Int(iarg(args, 0)))?;
            self_value
        }
        (
            "setUserAuthenticationValidWhileOnBody",
            "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(
                vm,
                recv,
                "mUserAuthenticationValidWhileOnBody",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        (
            "setInvalidatedByBiometricEnrollment",
            "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(
                vm,
                recv,
                "mInvalidatedByBiometricEnrollment",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        (
            "setBoundToSpecificSecureUserId",
            "(J)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(vm, recv, "mBoundToSecureUserId", Value::Long(larg(args, 0)))?;
            self_value
        }
        (
            "setCriticalToDeviceEncryption",
            "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        ) => {
            put(
                vm,
                recv,
                "mCriticalToDeviceEncryption",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        ("setUnlockedDeviceRequired", "(Z)Landroid/security/keystore/KeyProtection$Builder;") => {
            put(
                vm,
                recv,
                "mUnlockedDeviceRequired",
                Value::Int(iarg(args, 0)),
            )?;
            self_value
        }
        ("setIsStrongBoxBacked", "(Z)Landroid/security/keystore/KeyProtection$Builder;") => {
            put(vm, recv, "mIsStrongBoxBacked", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setMaxUsageCount", "(I)Landroid/security/keystore/KeyProtection$Builder;") => {
            let count = iarg(args, 0);
            if count == -1 || count > 0 {
                put(vm, recv, "mMaxUsageCount", Value::Int(count))?;
                self_value
            } else {
                return Err(iae(
                    "maxUsageCount must be either UNRESTRICTED_USAGE_COUNT or a positive value",
                ));
            }
        }
        ("setRollbackResistant", "(Z)Landroid/security/keystore/KeyProtection$Builder;") => {
            put(vm, recv, "mRollbackResistant", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("build", "()Landroid/security/keystore/KeyProtection;") => {
            let out = alloc_obj(vm, P57_KEY_PROTECTION, Vec::new())?;
            p57_key_protection_finish(vm, out, recv)?;
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

const P57_KEY_INFO_FIELDS: &[&str] = &[
    "mKeystoreAlias",
    "mInsideSecureHardware",
    "mOrigin",
    "mKeySize",
    "mKeyValidityStart",
    "mKeyValidityForOriginationEnd",
    "mKeyValidityForConsumptionEnd",
    "mPurposes",
    "mEncryptionPaddings",
    "mSignaturePaddings",
    "mDigests",
    "mBlockModes",
    "mUserAuthenticationRequired",
    "mUserAuthenticationValidityDurationSeconds",
    "mUserAuthenticationType",
    "mUserAuthenticationRequirementEnforcedBySecureHardware",
    "mUserAuthenticationValidWhileOnBody",
    "mTrustedUserPresenceRequired",
    "mInvalidatedByBiometricEnrollment",
    "mUserConfirmationRequired",
    "mSecurityLevel",
    "mRemainingUsageCount",
];

fn p57_key_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Ljava/lang/String;ZIILjava/util/Date;Ljava/util/Date;Ljava/util/Date;I[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;ZIIZZZZZII)V") => {
            for (i, f) in P57_KEY_INFO_FIELDS.iter().enumerate() {
                let mut v = args.get(i).cloned().unwrap_or(Value::Null);
                v = match *f {
                    "mKeyValidityStart" | "mKeyValidityForOriginationEnd"
                    | "mKeyValidityForConsumptionEnd" => p57_clone_date(vm, &v)?,
                    "mEncryptionPaddings" | "mSignaturePaddings" | "mDigests"
                    | "mBlockModes" => p57_clone_string_array_or_empty(vm, &v)?,
                    _ => v,
                };
                put(vm, recv, f, v)?;
            }
            Value::Null
        }
        ("getKeystoreAlias", "()Ljava/lang/String;") => get(vm, recv, "mKeystoreAlias"),
        ("isInsideSecureHardware", "()Z") => {
            bool_value(field_bool(vm, recv, "mInsideSecureHardware"))
        }
        ("getOrigin", "()I") => Value::Int(field_int(vm, recv, "mOrigin")),
        ("getKeySize", "()I") => Value::Int(field_int(vm, recv, "mKeySize")),
        ("getKeyValidityStart", "()Ljava/util/Date;") => {
            let v = get(vm, recv, "mKeyValidityStart");
            p57_clone_date(vm, &v)?
        }
        ("getKeyValidityForConsumptionEnd", "()Ljava/util/Date;") => {
            let v = get(vm, recv, "mKeyValidityForConsumptionEnd");
            p57_clone_date(vm, &v)?
        }
        ("getKeyValidityForOriginationEnd", "()Ljava/util/Date;") => {
            let v = get(vm, recv, "mKeyValidityForOriginationEnd");
            p57_clone_date(vm, &v)?
        }
        ("getPurposes", "()I") => Value::Int(field_int(vm, recv, "mPurposes")),
        ("getBlockModes", "()[Ljava/lang/String;") => {
            let v = get(vm, recv, "mBlockModes");
            p57_clone_string_array_or_empty(vm, &v)?
        }
        ("getEncryptionPaddings", "()[Ljava/lang/String;") => {
            let v = get(vm, recv, "mEncryptionPaddings");
            p57_clone_string_array_or_empty(vm, &v)?
        }
        ("getSignaturePaddings", "()[Ljava/lang/String;") => {
            let v = get(vm, recv, "mSignaturePaddings");
            p57_clone_string_array_or_empty(vm, &v)?
        }
        ("getDigests", "()[Ljava/lang/String;") => {
            let v = get(vm, recv, "mDigests");
            p57_clone_string_array_or_empty(vm, &v)?
        }
        ("isUserAuthenticationRequired", "()Z") => {
            bool_value(field_bool(vm, recv, "mUserAuthenticationRequired"))
        }
        ("isUserConfirmationRequired", "()Z") => {
            bool_value(field_bool(vm, recv, "mUserConfirmationRequired"))
        }
        ("getUserAuthenticationValidityDurationSeconds", "()I") => Value::Int(field_int(
            vm,
            recv,
            "mUserAuthenticationValidityDurationSeconds",
        )),
        ("getUserAuthenticationType", "()I") => {
            Value::Int(field_int(vm, recv, "mUserAuthenticationType"))
        }
        ("isUserAuthenticationRequirementEnforcedBySecureHardware", "()Z") => bool_value(
            field_bool(
                vm,
                recv,
                "mUserAuthenticationRequirementEnforcedBySecureHardware",
            ),
        ),
        ("isUserAuthenticationValidWhileOnBody", "()Z") => bool_value(field_bool(
            vm,
            recv,
            "mUserAuthenticationValidWhileOnBody",
        )),
        ("isInvalidatedByBiometricEnrollment", "()Z") => bool_value(field_bool(
            vm,
            recv,
            "mInvalidatedByBiometricEnrollment",
        )),
        ("isTrustedUserPresenceRequired", "()Z") => {
            bool_value(field_bool(vm, recv, "mTrustedUserPresenceRequired"))
        }
        ("getSecurityLevel", "()I") => Value::Int(field_int(vm, recv, "mSecurityLevel")),
        ("getRemainingUsageCount", "()I") => {
            Value::Int(field_int(vm, recv, "mRemainingUsageCount"))
        }
        _ => return Ok(None),
    }))
}

// --- android.mtp -----------------------------------------------------------

fn p57_mtp_device_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let supported = |field: &str| -> Result<Value, VmExit> {
        let v = get(vm, recv, field);
        if obj(Some(&v)).is_none() {
            return Err(npe(field));
        }
        Ok(bool_value(
            int_values(vm, Some(&v)).contains(&iarg(args, 0)),
        ))
    };
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("getManufacturer", "()Ljava/lang/String;") => get(vm, recv, "mManufacturer"),
        ("getModel", "()Ljava/lang/String;") => get(vm, recv, "mModel"),
        ("getVersion", "()Ljava/lang/String;") => get(vm, recv, "mVersion"),
        ("getSerialNumber", "()Ljava/lang/String;") => get(vm, recv, "mSerialNumber"),
        ("getOperationsSupported", "()[I") => get(vm, recv, "mOperationsSupported"),
        ("getEventsSupported", "()[I") => get(vm, recv, "mEventsSupported"),
        ("getDevicePropertySupported", "()[I") => get(vm, recv, "mDevicePropertySupported"),
        ("isOperationSupported", "(I)Z") => supported("mOperationsSupported")?,
        ("isEventSupported", "(I)Z") => supported("mEventsSupported")?,
        ("isDevicePropertySupported", "(I)Z") => supported("mDevicePropertySupported")?,
        _ => return Ok(None),
    }))
}

fn p57_mtp_storage_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("getStorageId", "()I") => Value::Int(field_int(vm, recv, "mStorageId")),
        ("getMaxCapacity", "()J") => Value::Long(field_long(vm, recv, "mMaxCapacity")),
        ("getFreeSpace", "()J") => Value::Long(field_long(vm, recv, "mFreeSpace")),
        ("getDescription", "()Ljava/lang/String;") => get(vm, recv, "mDescription"),
        ("getVolumeIdentifier", "()Ljava/lang/String;") => get(vm, recv, "mVolumeIdentifier"),
        _ => return Ok(None),
    }))
}

const P57_MTP_OBJECT_INFO_FIELDS: &[&str] = &[
    "mHandle",
    "mStorageId",
    "mFormat",
    "mProtectionStatus",
    "mCompressedSize",
    "mThumbFormat",
    "mThumbCompressedSize",
    "mThumbPixWidth",
    "mThumbPixHeight",
    "mImagePixWidth",
    "mImagePixHeight",
    "mImagePixDepth",
    "mParent",
    "mAssociationType",
    "mAssociationDesc",
    "mSequenceNumber",
    "mName",
    "mDateCreated",
    "mDateModified",
    "mKeywords",
];

fn p57_mtp_uint32(value: i32) -> i64 {
    value as u32 as i64
}

fn p57_mtp_object_info_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            let empty = jstr(vm, "")?;
            let empty2 = jstr(vm, "")?;
            put(vm, recv, "mName", empty)?;
            put(vm, recv, "mKeywords", empty2)?;
            Value::Null
        }
        ("getObjectHandle", "()I") => Value::Int(field_int(vm, recv, "mHandle")),
        ("getStorageId", "()I") => Value::Int(field_int(vm, recv, "mStorageId")),
        ("getFormat", "()I") => Value::Int(field_int(vm, recv, "mFormat")),
        ("getProtectionStatus", "()I") => Value::Int(field_int(vm, recv, "mProtectionStatus")),
        ("getCompressedSize", "()I") => Value::Int(field_int(vm, recv, "mCompressedSize")),
        ("getCompressedSizeLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mCompressedSize")))
        }
        ("getThumbFormat", "()I") => Value::Int(field_int(vm, recv, "mThumbFormat")),
        ("getThumbCompressedSize", "()I") => {
            Value::Int(field_int(vm, recv, "mThumbCompressedSize"))
        }
        ("getThumbCompressedSizeLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mThumbCompressedSize")))
        }
        ("getThumbPixWidth", "()I") => Value::Int(field_int(vm, recv, "mThumbPixWidth")),
        ("getThumbPixWidthLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mThumbPixWidth")))
        }
        ("getThumbPixHeight", "()I") => Value::Int(field_int(vm, recv, "mThumbPixHeight")),
        ("getThumbPixHeightLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mThumbPixHeight")))
        }
        ("getImagePixWidth", "()I") => Value::Int(field_int(vm, recv, "mImagePixWidth")),
        ("getImagePixWidthLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mImagePixWidth")))
        }
        ("getImagePixHeight", "()I") => Value::Int(field_int(vm, recv, "mImagePixHeight")),
        ("getImagePixHeightLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mImagePixHeight")))
        }
        ("getImagePixDepth", "()I") => Value::Int(field_int(vm, recv, "mImagePixDepth")),
        ("getImagePixDepthLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mImagePixDepth")))
        }
        ("getParent", "()I") => Value::Int(field_int(vm, recv, "mParent")),
        ("getAssociationType", "()I") => Value::Int(field_int(vm, recv, "mAssociationType")),
        ("getAssociationDesc", "()I") => Value::Int(field_int(vm, recv, "mAssociationDesc")),
        ("getSequenceNumber", "()I") => Value::Int(field_int(vm, recv, "mSequenceNumber")),
        ("getSequenceNumberLong", "()J") => {
            Value::Long(p57_mtp_uint32(field_int(vm, recv, "mSequenceNumber")))
        }
        ("getName", "()Ljava/lang/String;") => {
            let v = get(vm, recv, "mName");
            if matches!(v, Value::Obj(_)) {
                v
            } else {
                jstr(vm, "")?
            }
        }
        ("getDateCreated", "()J") => Value::Long(field_long(vm, recv, "mDateCreated")),
        ("getDateModified", "()J") => Value::Long(field_long(vm, recv, "mDateModified")),
        ("getKeywords", "()Ljava/lang/String;") => {
            let v = get(vm, recv, "mKeywords");
            if matches!(v, Value::Obj(_)) {
                v
            } else {
                jstr(vm, "")?
            }
        }
        _ => return Ok(None),
    }))
}

fn p57_mtp_builder_info(vm: &Engine, recv: ObjRef) -> Result<ObjRef, VmExit> {
    field_obj(vm, recv, "mObjectInfo").ok_or_else(|| npe("mObjectInfo"))
}

fn p57_mtp_long_to_uint32(value: i64) -> Result<i64, VmExit> {
    if !(0..=0xffff_ffff).contains(&value) {
        return Err(iae(format!(
            "value should be in range [0, 4294967295], but is {value}"
        )));
    }
    Ok(value)
}

fn p57_mtp_object_builder_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let self_value = Value::Obj(recv);
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            let info = alloc_obj(vm, P57_MTP_OBJECT_INFO, Vec::new())?;
            let empty = jstr(vm, "")?;
            let empty2 = jstr(vm, "")?;
            put(vm, info, "mName", empty)?;
            put(vm, info, "mKeywords", empty2)?;
            put(vm, info, "mHandle", Value::Int(-1))?;
            put(vm, recv, "mObjectInfo", Value::Obj(info))?;
            Value::Null
        }
        ("<init>", "(Landroid/mtp/MtpObjectInfo;)V") => {
            let Some(source) = oarg(args, 0) else {
                return Err(npe("objectInfo"));
            };
            let info = alloc_obj(vm, P57_MTP_OBJECT_INFO, Vec::new())?;
            for f in P57_MTP_OBJECT_INFO_FIELDS {
                if *f == "mHandle" {
                    continue;
                }
                let v = get(vm, source, f);
                put(vm, info, f, v)?;
            }
            put(vm, info, "mHandle", Value::Int(-1))?;
            put(vm, recv, "mObjectInfo", Value::Obj(info))?;
            Value::Null
        }
        ("setObjectHandle", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mHandle", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setAssociationDesc", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mAssociationDesc", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setAssociationType", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mAssociationType", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setCompressedSize", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mCompressedSize", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setDateCreated", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mDateCreated", Value::Long(larg(args, 0)))?;
            self_value
        }
        ("setDateModified", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mDateModified", Value::Long(larg(args, 0)))?;
            self_value
        }
        ("setFormat", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mFormat", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setImagePixDepth", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mImagePixDepth", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setImagePixHeight", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mImagePixHeight", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setImagePixWidth", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mImagePixWidth", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setKeywords", "(Ljava/lang/String;)Landroid/mtp/MtpObjectInfo$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("value"));
            }
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mKeywords", args[0].clone())?;
            self_value
        }
        ("setName", "(Ljava/lang/String;)Landroid/mtp/MtpObjectInfo$Builder;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("value"));
            }
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mName", args[0].clone())?;
            self_value
        }
        ("setParent", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mParent", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setProtectionStatus", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mProtectionStatus", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setSequenceNumber", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mSequenceNumber", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setStorageId", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mStorageId", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setThumbCompressedSize", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(
                vm,
                info,
                "mThumbCompressedSize",
                Value::Int(v as u32 as i32),
            )?;
            self_value
        }
        ("setThumbFormat", "(I)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mThumbFormat", Value::Int(iarg(args, 0)))?;
            self_value
        }
        ("setThumbPixHeight", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mThumbPixHeight", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("setThumbPixWidth", "(J)Landroid/mtp/MtpObjectInfo$Builder;") => {
            let v = p57_mtp_long_to_uint32(larg(args, 0))?;
            let info = p57_mtp_builder_info(vm, recv)?;
            put(vm, info, "mThumbPixWidth", Value::Int(v as u32 as i32))?;
            self_value
        }
        ("build", "()Landroid/mtp/MtpObjectInfo;") => {
            let info = get(vm, recv, "mObjectInfo");
            put(vm, recv, "mObjectInfo", Value::Null)?;
            info
        }
        _ => return Ok(None),
    }))
}

fn p57_ioexception(message: &str) -> VmExit {
    throw("Ljava/io/IOException;", message)
}

fn p57_mtp_device_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/hardware/usb/UsbDevice;)V") => {
            if oarg(args, 0).is_none() {
                return Err(npe("device"));
            }
            put(vm, recv, "mDevice", args[0].clone())?;
            put(vm, recv, "rd_open", Value::Int(0))?;
            Value::Null
        }
        ("open", "(Landroid/hardware/usb/UsbDeviceConnection;)Z") => {
            let Some(connection) = oarg(args, 0) else {
                return Err(npe("connection"));
            };
            // There is no native MTP/USB transport. AOSP closes the supplied
            // connection when opening fails.
            let _ = vm.invoke_instance(connection, "close", "()V", &[]);
            put(vm, recv, "rd_open", Value::Int(0))?;
            Value::Int(0)
        }
        ("close", "()V") => {
            put(vm, recv, "rd_open", Value::Int(0))?;
            Value::Null
        }
        ("getDeviceName", "()Ljava/lang/String;") => {
            let Some(device) = field_obj(vm, recv, "mDevice") else {
                return Err(npe("mDevice"));
            };
            get(vm, device, "mName")
        }
        ("getDeviceId", "()I") => {
            let Some(device) = field_obj(vm, recv, "mDevice") else {
                return Err(npe("mDevice"));
            };
            let device_name = field_str(vm, device, "mName").unwrap_or_default();
            Value::Int(p57_usb_device_id_from_name(&device_name))
        }
        ("toString", "()Ljava/lang/String;") => {
            let Some(device) = field_obj(vm, recv, "mDevice") else {
                return Err(npe("mDevice"));
            };
            get(vm, device, "mName")
        }
        ("getDeviceInfo", "()Landroid/mtp/MtpDeviceInfo;") => Value::Null,
        ("setDevicePropertyInitVersion", "(Ljava/lang/String;)I") => Value::Int(-1),
        ("getStorageIds", "()[I") => Value::Null,
        ("getObjectHandles", "(III)[I") => Value::Null,
        ("getObject", "(II)[B") => {
            if iarg(args, 1) < 0 {
                return Err(iae("objectSize should not be negative"));
            }
            Value::Null
        }
        ("getPartialObject", "(IJJ[B)J") | ("getPartialObject64", "(IJJ[B)J") => {
            if oarg(args, 3).is_none() {
                return Err(npe("buffer"));
            }
            return Err(p57_ioexception("Failed to obtain object bytes"));
        }
        ("getThumbnail", "(I)[B") => Value::Null,
        ("getStorageInfo", "(I)Landroid/mtp/MtpStorageInfo;") => Value::Null,
        ("getObjectInfo", "(I)Landroid/mtp/MtpObjectInfo;") => Value::Null,
        ("deleteObject", "(I)Z") => Value::Int(0),
        ("getParent", "(I)J") => Value::Long(-1),
        ("getStorageId", "(I)J") => Value::Long(-1),
        ("importFile", "(ILjava/lang/String;)Z") => {
            if oarg(args, 1).is_none() {
                return Err(npe("destPath"));
            }
            Value::Int(0)
        }
        ("importFile", "(ILandroid/os/ParcelFileDescriptor;)Z") => {
            if oarg(args, 1).is_none() {
                return Err(npe("descriptor"));
            }
            Value::Int(0)
        }
        ("sendObject", "(IJLandroid/os/ParcelFileDescriptor;)Z") => {
            if oarg(args, 2).is_none() {
                return Err(npe("descriptor"));
            }
            Value::Int(0)
        }
        ("sendObjectInfo", "(Landroid/mtp/MtpObjectInfo;)Landroid/mtp/MtpObjectInfo;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("info"));
            }
            Value::Null
        }
        ("readEvent", "(Landroid/os/CancellationSignal;)Landroid/mtp/MtpEvent;") => {
            return Err(p57_ioexception("Failed to submit event request"));
        }
        ("getObjectSizeLong", "(II)J") => {
            return Err(p57_ioexception("Failed to obtain object size"));
        }
        _ => return Ok(None),
    }))
}

// --- Batch 57 routers --------------------------------------------------------

fn partial57_instance_for_class(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        P57_TEXT_SERVICES_MANAGER => p57_text_services_manager_instance(vm, name, sig, recv, args),
        P57_SPELL_SESSION => p57_spell_session_instance(vm, name, sig, recv, args),
        P57_SPELL_PARAMS | P57_SPELL_PARAMS_BUILDER => {
            p57_spell_params_instance(vm, class, name, sig, recv, args)
        }
        P57_TEXT_INFO => p57_text_info_instance(vm, name, sig, recv, args),
        P57_SUGGESTIONS_INFO => p57_suggestions_info_instance(vm, name, sig, recv, args),
        P57_SENTENCE_SUGGESTIONS_INFO => {
            p57_sentence_suggestions_instance(vm, name, sig, recv, args)
        }
        P57_SPELL_SUBTYPE => p57_spell_subtype_instance(vm, name, sig, recv, args),
        P57_TRANSLATION_MANAGER => p57_translation_manager_instance(vm, name, sig, recv, args),
        P57_TRANSLATION_SPEC => p57_translation_spec_instance(vm, name, sig, recv, args),
        P57_TRANSLATION_REQUEST_VALUE => {
            p57_translation_request_value_instance(vm, name, sig, recv, args)
        }
        P57_VIEW_TRANSLATION_REQUEST => {
            p57_view_translation_request_instance(vm, name, sig, recv, args)
        }
        P57_VIEW_TRANSLATION_REQUEST_BUILDER => {
            p57_view_translation_builder_instance(vm, name, sig, recv, args)
        }
        P57_TRANSLATION_CONTEXT | P57_TRANSLATION_CONTEXT_BUILDER => {
            p57_translation_context_instance(vm, class, name, sig, recv, args)
        }
        P57_TRANSLATION_CAPABILITY => {
            p57_translation_capability_instance(vm, name, sig, recv, args)
        }
        P57_USB_MANAGER => p57_usb_manager_instance(vm, name, sig, recv, args),
        P57_USB_DEVICE => p57_usb_device_instance(vm, name, sig, recv, args),
        P57_USB_DEVICE_BUILDER => p57_usb_device_builder_instance(vm, name, sig, recv, args),
        P57_USB_CONFIGURATION => p57_usb_configuration_instance(vm, name, sig, recv, args),
        P57_USB_INTERFACE => p57_usb_interface_instance(vm, name, sig, recv, args),
        P57_USB_ENDPOINT => p57_usb_endpoint_instance(vm, name, sig, recv, args),
        P57_EUICC_MANAGER | P57_EUICC_INFO => p57_euicc_instance(vm, class, name, sig, recv, args),
        P57_USAGE_MANAGER => p57_usage_manager_instance(vm, name, sig, recv, args),
        P57_USAGE_STATS => p57_usage_stats_instance(vm, name, sig, recv, args),
        P57_USAGE_EVENTS => p57_usage_events_instance(vm, name, sig, recv, args),
        P57_USAGE_EVENT => p57_usage_event_instance(vm, name, sig, recv, args),
        P57_PRINT_MANAGER => p57_print_manager_instance(vm, name, sig, recv, args),
        P57_PRINT_ATTRIBUTES => p57_print_attributes_instance(vm, name, sig, recv, args),
        P57_PRINT_ATTRIBUTES_BUILDER => p57_print_builder_instance(vm, name, sig, recv, args),
        P57_MEDIA_SIZE => p57_media_size_instance(vm, name, sig, recv, args),
        P57_RESOLUTION => p57_resolution_instance(vm, name, sig, recv, args),
        P57_MARGINS => p57_margins_instance(vm, name, sig, recv, args),
        P57_PAGE_RANGE => p57_page_range_instance(vm, name, sig, recv, args),
        P57_PRINTER_ID => p57_printer_id_instance(vm, name, sig, recv, args),
        P57_KEY_GEN_SPEC | P57_KEY_GEN_SPEC_BUILDER => {
            p57_keygen_instance(vm, class, name, sig, recv, args)
        }
        P57_KEY_PROTECTION | P57_KEY_PROTECTION_BUILDER => {
            p57_key_protection_instance(vm, class, name, sig, recv, args)
        }
        P57_KEY_INFO => p57_key_info_instance(vm, name, sig, recv, args),
        P57_MTP_DEVICE_INFO => p57_mtp_device_info_instance(vm, name, sig, recv, args),
        P57_MTP_STORAGE_INFO => p57_mtp_storage_info_instance(vm, name, sig, recv, args),
        P57_MTP_OBJECT_INFO => p57_mtp_object_info_instance(vm, name, sig, recv, args),
        P57_MTP_OBJECT_INFO_BUILDER => p57_mtp_object_builder_instance(vm, name, sig, recv, args),
        P57_MTP_DEVICE => p57_mtp_device_instance(vm, name, sig, recv, args),
        _ => Ok(None),
    }
}

pub(crate) fn partial57_instance(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    for class in [recv_class, mref_class] {
        if p57_class(class) {
            let v = partial57_instance_for_class(vm, class, name, sig, recv, args)?;
            if v.is_some() {
                return Ok(v);
            }
        }
    }

    // Managers returned by Context may be created for a framework subclass or
    // through a test harness class label that is not one of the exact class
    // constants above. Fall back based on the receiver's actual class when it
    // is one of this batch's classes.
    let actual = class_of(vm, recv);
    if actual != recv_class && actual != mref_class && p57_class(&actual) {
        return partial57_instance_for_class(vm, &actual, name, sig, recv, args);
    }
    Ok(None)
}

pub(crate) fn partial57_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (class, name, sig) {
        (
            P57_TRANSLATION_REQUEST_VALUE,
            "forText",
            "(Ljava/lang/CharSequence;)Landroid/view/translation/TranslationRequestValue;",
        ) => {
            if oarg(args, 0).is_none() {
                return Err(npe("text should not be null"));
            }
            let r = alloc_obj(vm, P57_TRANSLATION_REQUEST_VALUE, Vec::new())?;
            put(vm, r, "mText", args[0].clone())?;
            Value::Obj(r)
        }
        (P57_USB_DEVICE, "getDeviceId", "(Ljava/lang/String;)I") => {
            let Some(device_name) = opt_str_arg(vm, args, 0) else {
                return Err(npe("deviceName"));
            };
            Value::Int(p57_usb_device_id_from_name(&device_name))
        }
        (P57_USB_DEVICE, "getDeviceName", "(I)Ljava/lang/String;") => {
            jstr(vm, p57_usb_device_name_from_id(iarg(args, 0)))?
        }
        (
            P57_USB_DEVICE,
            "getFileDescriptorForDevice",
            "(Landroid/hardware/usb/UsbDevice;)Landroid/os/ParcelFileDescriptor;",
        ) => Value::Null,
        (P57_USB_MANAGER, "areSettableFunctions", "(J)Z") => {
            let functions = larg(args, 0);
            const SETTABLE: i64 = (1 << 2) | (1 << 4) | (1 << 5) | (1 << 3) | (1 << 10) | (1 << 7);
            bool_value(
                functions == 0
                    || ((functions & !SETTABLE) == 0
                        && ((functions as u64).count_ones() == 1
                            || functions == ((1 << 5) | (1 << 10)))),
            )
        }
        (P57_USB_MANAGER, "usbFunctionsToString", "(J)Ljava/lang/String;") => {
            jstr(vm, p57_usb_functions_to_string(larg(args, 0)))?
        }
        (P57_USB_MANAGER, "usbFunctionsFromString", "(Ljava/lang/String;)J") => {
            Value::Long(p57_usb_functions_from_string(opt_str_arg(vm, args, 0))?)
        }
        (
            P57_USB_MANAGER,
            "getGlobalUsbAlsaJackDetector",
            "()Landroid/hardware/usb/UsbAlsaJackDetector;",
        )
        | (
            P57_USB_MANAGER,
            "checkAndGetGlobalUsbAlsaJackDetector",
            "()Landroid/hardware/usb/UsbAlsaJackDetector;",
        ) => Value::Null,
        (P57_USB_CONSTANTS, "usbClassName", "(I)Ljava/lang/String;") => {
            let text = match iarg(args, 0) {
                0 => "USB_CLASS_PER_INTERFACE",
                1 => "USB_CLASS_AUDIO",
                2 => "USB_CLASS_COMM",
                3 => "USB_CLASS_HID",
                5 => "USB_CLASS_PHYSICA",
                6 => "USB_CLASS_STILL_IMAGE",
                7 => "USB_CLASS_PRINTER",
                8 => "USB_CLASS_MASS_STORAGE",
                9 => "USB_CLASS_HUB",
                10 => "USB_CLASS_CDC_DATA",
                11 => "USB_CLASS_CSCID",
                13 => "USB_CLASS_CONTENT_SEC",
                14 => "USB_CLASS_VIDEO",
                15 => "USB_CLASS_PERSONAL_HEALTHCARE",
                16 => "USB_CLASS_AUDIO_VIDEO",
                17 => "USB_CLASS_BILLBOARD",
                18 => "USB_CLASS_TYPE_C_BRIDGE",
                220 => "USB_CLASS_DIAGNOSTIC",
                224 => "USB_CLASS_WIRELESS_CONTROLLER",
                239 => "USB_CLASS_MISC",
                254 => "USB_CLASS_APP_SPEC",
                255 => "USB_CLASS_VENDOR_SPEC",
                _ => "UNKNOWN",
            };
            jstr(vm, text)?
        }
        (P57_USB_CONSTANTS, "usbSubclassName", "(I)Ljava/lang/String;") => {
            let text = match iarg(args, 0) {
                1 => "USB_INTERFACE_SUBCLASS_BOOT",
                _ => "UNKNOWN",
            };
            jstr(vm, text)?
        }
        (
            P57_MEDIA_SIZE,
            "getStandardMediaSizeById",
            "(Ljava/lang/String;)Landroid/print/PrintAttributes$MediaSize;",
        ) => match opt_str_arg(vm, args, 0) {
            Some(id) => p57_media_size_standard(vm, &id)?.unwrap_or(Value::Null),
            None => Value::Null,
        },
        (P57_KEY_PROPERTIES, "namespaceToLegacyUid", "(I)I") => match iarg(args, 0) {
            -1 => Value::Int(-1),
            102 => Value::Int(1010),
            ns => return Err(iae(format!("No UID corresponding to namespace {ns}"))),
        },
        (P57_KEY_PROPERTIES, "legacyUidToNamespace", "(I)I") => match iarg(args, 0) {
            -1 => Value::Int(-1),
            1010 => Value::Int(102),
            uid => return Err(iae(format!("No namespace corresponding to uid {uid}"))),
        },
        (P57_MTP_CONSTANTS, "isAbstractObject", "(I)Z") => bool_value(matches!(
            iarg(args, 0),
            0xba01 | 0xba02 | 0xba03 | 0xba04 | 0xba05 | 0xba09 | 0xba0a | 0xba0b | 0xba81
        )),
        _ => return Ok(None),
    }))
}

pub(crate) fn partial57_host_static(
    vm: &mut Engine,
    class: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    {
        let value = match (class, field) {
            (P57_PRINT_ATTRIBUTES, "COLOR_MODE_MONOCHROME") => Some(1),
            (P57_PRINT_ATTRIBUTES, "COLOR_MODE_COLOR") => Some(2),
            (P57_PRINT_ATTRIBUTES, "DUPLEX_MODE_NONE") => Some(1),
            (P57_PRINT_ATTRIBUTES, "DUPLEX_MODE_LONG_EDGE") => Some(2),
            (P57_PRINT_ATTRIBUTES, "DUPLEX_MODE_SHORT_EDGE") => Some(4),
            (P57_EUICC_MANAGER, "EUICC_OTA_STATUS_UNAVAILABLE") => Some(5),
            (P57_EUICC_MANAGER, "EMBEDDED_SUBSCRIPTION_RESULT_ERROR") => Some(2),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_EXEMPTED") => Some(5),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_ACTIVE") => Some(10),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_WORKING_SET") => Some(20),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_FREQUENT") => Some(30),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_RARE") => Some(40),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_RESTRICTED") => Some(45),
            (P57_USAGE_MANAGER, "STANDBY_BUCKET_NEVER") => Some(50),
            (P57_KEY_PROPERTIES, "AUTH_DEVICE_CREDENTIAL") => Some(1),
            (P57_KEY_PROPERTIES, "AUTH_BIOMETRIC_STRONG") => Some(2),
            (P57_KEY_PROPERTIES, "PURPOSE_ENCRYPT") => Some(1),
            (P57_KEY_PROPERTIES, "PURPOSE_DECRYPT") => Some(2),
            (P57_KEY_PROPERTIES, "PURPOSE_SIGN") => Some(4),
            (P57_KEY_PROPERTIES, "PURPOSE_VERIFY") => Some(8),
            (P57_KEY_PROPERTIES, "PURPOSE_WRAP_KEY") => Some(32),
            (P57_KEY_PROPERTIES, "PURPOSE_AGREE_KEY") => Some(64),
            (P57_KEY_PROPERTIES, "PURPOSE_ATTEST_KEY") => Some(128),
            (P57_KEY_PROPERTIES, "NAMESPACE_APPLICATION") => Some(-1),
            (P57_KEY_PROPERTIES, "NAMESPACE_WIFI") => Some(102),
            (P57_KEY_PROPERTIES, "NAMESPACE_LOCKSETTINGS") => Some(103),
            (P57_KEY_PROPERTIES, "UNRESTRICTED_USAGE_COUNT") => Some(-1),
            _ => None,
        };
        if let Some(v) = value {
            return Ok(Some(Value::Int(v)));
        }
    }

    if class == P57_USB_MANAGER {
        let value = match field {
            "FUNCTION_NONE" => Some(0),
            "FUNCTION_MTP" => Some(1 << 2),
            "FUNCTION_PTP" => Some(1 << 4),
            "FUNCTION_RNDIS" => Some(1 << 5),
            "FUNCTION_MIDI" => Some(1 << 3),
            "FUNCTION_ACCESSORY" => Some(1 << 1),
            "FUNCTION_AUDIO_SOURCE" => Some(1 << 6),
            "FUNCTION_ADB" => Some(1),
            "FUNCTION_NCM" => Some(1 << 10),
            "FUNCTION_UVC" => Some(1 << 7),
            _ => None,
        };
        if let Some(v) = value {
            return Ok(Some(Value::Long(v)));
        }
    }

    if class == P57_MEDIA_SIZE {
        if let Some(v) = p57_media_size_standard(vm, field)? {
            return Ok(Some(v));
        }
    }
    if class == P57_PAGE_RANGE && field == "ALL_PAGES" {
        let r = alloc_obj(vm, P57_PAGE_RANGE, Vec::new())?;
        put(vm, r, "mStart", Value::Int(0))?;
        put(vm, r, "mEnd", Value::Int(i32::MAX))?;
        return Ok(Some(Value::Obj(r)));
    }
    if class == P57_MARGINS && field == "NO_MARGINS" {
        let r = alloc_obj(vm, P57_MARGINS, Vec::new())?;
        for f in ["mLeftMils", "mTopMils", "mRightMils", "mBottomMils"] {
            put(vm, r, f, Value::Int(0))?;
        }
        return Ok(Some(Value::Obj(r)));
    }
    Ok(None)
}
