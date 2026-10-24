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
