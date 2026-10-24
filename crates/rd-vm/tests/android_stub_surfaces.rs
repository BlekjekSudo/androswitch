//! Guest-visible proofs for the bounded partial slices promoted from the
//! former `stub` rows: WebKit state/history, DocumentsContract URI values,
//! Camera2 metadata/params on explicit data, no-key security defaults, and
//! TextClassifier values plus its NO_OP/default classifier behavior.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const CONTEXT: &str = "Landroid/content/Context;";
const WEBVIEW: &str = "Landroid/webkit/WebView;";
const DOCUMENTS: &str = "Landroid/provider/DocumentsContract;";
const DOCUMENTS_PATH: &str = "Landroid/provider/DocumentsContract$Path;";
const POLICY: &str = "Landroid/security/NetworkSecurityPolicy;";
const KEY_CHAIN: &str = "Landroid/security/KeyChain;";
const CAMERA_MANAGER: &str = "Landroid/hardware/camera2/CameraManager;";
const CAMERA_ACCESS_EXCEPTION: &str = "Landroid/hardware/camera2/CameraAccessException;";
const METADATA_NATIVE: &str = "Landroid/hardware/camera2/impl/CameraMetadataNative;";
const NATIVE_KEY: &str = "Landroid/hardware/camera2/impl/CameraMetadataNative$Key;";
const CHARACTERISTICS: &str = "Landroid/hardware/camera2/CameraCharacteristics;";
const CHARACTERISTICS_KEY: &str = "Landroid/hardware/camera2/CameraCharacteristics$Key;";
const CAPTURE_REQUEST: &str = "Landroid/hardware/camera2/CaptureRequest;";
const CAPTURE_REQUEST_KEY: &str = "Landroid/hardware/camera2/CaptureRequest$Key;";
const CAPTURE_REQUEST_BUILDER: &str = "Landroid/hardware/camera2/CaptureRequest$Builder;";
const CAPTURE_RESULT: &str = "Landroid/hardware/camera2/CaptureResult;";
const CAPTURE_RESULT_KEY: &str = "Landroid/hardware/camera2/CaptureResult$Key;";
const TOTAL_CAPTURE_RESULT: &str = "Landroid/hardware/camera2/TotalCaptureResult;";
const STREAM_CONFIG: &str = "Landroid/hardware/camera2/params/StreamConfiguration;";
const STREAM_DURATION: &str = "Landroid/hardware/camera2/params/StreamConfigurationDuration;";
const STREAM_MAP: &str = "Landroid/hardware/camera2/params/StreamConfigurationMap;";
const HIGH_SPEED: &str = "Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;";
const REPROCESS: &str = "Landroid/hardware/camera2/params/ReprocessFormatsMap;";
const METERING: &str = "Landroid/hardware/camera2/params/MeteringRectangle;";
const RGGB: &str = "Landroid/hardware/camera2/params/RggbChannelVector;";
const BLACK: &str = "Landroid/hardware/camera2/params/BlackLevelPattern;";
const COLOR_TRANSFORM: &str = "Landroid/hardware/camera2/params/ColorSpaceTransform;";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}
fn object(value: Value) -> ObjRef {
    let Value::Obj(reference) = value else {
        panic!("expected object, got {value:?}");
    };
    reference
}
fn text(engine: &mut Engine, value: &str) -> Value {
    Value::Obj(engine.heap.alloc_string(value.to_string()).unwrap())
}
fn text_of(engine: &Engine, value: &Value) -> String {
    match value {
        Value::Obj(reference) => engine.heap.as_str(*reference).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}
fn call(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_instance(recv, name, sig, args).unwrap()
}
fn call_text(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> String {
    let value = call(engine, recv, name, sig, args);
    text_of(engine, &value)
}
fn call_static(engine: &mut Engine, class: &str, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_static(class, name, sig, args).unwrap()
}
fn call_static_text(
    engine: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> String {
    let value = call_static(engine, class, name, sig, args);
    text_of(engine, &value)
}
fn assert_throws(result: Result<Value, VmExit>, expected: &str) {
    match result {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, expected),
        other => panic!("expected {expected}, got {other:?}"),
    }
}
fn field(engine: &Engine, recv: ObjRef, name: &str) -> Value {
    engine.heap.get_field(recv, name).unwrap()
}
fn int_array(engine: &mut Engine, values: &[i32]) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Int, values.len(), "[I")
        .unwrap();
    *engine.heap.array_elems_mut(array).unwrap() = values.iter().copied().map(Value::Int).collect();
    Value::Obj(array)
}
fn float_array(engine: &mut Engine, len: usize) -> Value {
    Value::Obj(
        engine
            .heap
            .alloc_array_zeroed(ElemKind::Float, len, "[F")
            .unwrap(),
    )
}
fn object_array(engine: &mut Engine, descriptor: &str, values: Vec<Value>) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Obj, values.len(), descriptor)
        .unwrap();
    *engine.heap.array_elems_mut(array).unwrap() = values;
    Value::Obj(array)
}
fn array_values(engine: &Engine, value: &Value) -> Vec<Value> {
    engine
        .heap
        .as_array_elems(object(value.clone()))
        .unwrap()
        .0
        .to_vec()
}
fn int_values(engine: &Engine, value: &Value) -> Vec<i32> {
    array_values(engine, value)
        .iter()
        .map(|v| match v {
            Value::Int(i) => *i,
            other => panic!("expected int array element, got {other:?}"),
        })
        .collect()
}
fn list_of(engine: &mut Engine, values: &[Value]) -> ObjRef {
    let list = instance(engine, "Ljava/util/ArrayList;");
    call(engine, list, "<init>", "()V", &[]);
    for value in values {
        call(
            engine,
            list,
            "add",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(value),
        );
    }
    list
}
fn uri_text(engine: &mut Engine, value: Value) -> String {
    let uri = object(value);
    call_text(engine, uri, "toString", "()Ljava/lang/String;", &[])
}

#[test]
fn webview_history_and_websettings_state_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let view = instance(&mut engine, WEBVIEW);
    call(
        &mut engine,
        view,
        "<init>",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );

    let settings = object(call(
        &mut engine,
        view,
        "getSettings",
        "()Landroid/webkit/WebSettings;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, settings, "supportZoom", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, settings, "getJavaScriptEnabled", "()Z", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        settings,
        "setJavaScriptEnabled",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, settings, "getJavaScriptEnabled", "()Z", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        settings,
        "setTextZoom",
        "(I)V",
        &[Value::Int(125)],
    );
    assert_eq!(
        call(&mut engine, settings, "getTextZoom", "()I", &[]),
        Value::Int(125)
    );
    call(
        &mut engine,
        settings,
        "setMinimumFontSize",
        "(I)V",
        &[Value::Int(100)],
    );
    assert_eq!(
        call(&mut engine, settings, "getMinimumFontSize", "()I", &[]),
        Value::Int(72),
        "AOSP pins font sizes to 1..72"
    );
    assert_eq!(
        call_text(
            &mut engine,
            settings,
            "getDefaultTextEncodingName",
            "()Ljava/lang/String;",
            &[]
        ),
        "UTF-8"
    );

    for url in ["https://example.test/one", "https://example.test/two"] {
        let value = text(&mut engine, url);
        call(
            &mut engine,
            view,
            "loadUrl",
            "(Ljava/lang/String;)V",
            &[value],
        );
    }
    assert_eq!(
        call_text(&mut engine, view, "getUrl", "()Ljava/lang/String;", &[]),
        "https://example.test/two"
    );
    assert_eq!(
        call(&mut engine, view, "getProgress", "()I", &[]),
        Value::Int(100)
    );
    assert_eq!(
        call(&mut engine, view, "canGoBack", "()Z", &[]),
        Value::Int(1)
    );
    call(&mut engine, view, "goBack", "()V", &[]);
    assert_eq!(
        call_text(&mut engine, view, "getUrl", "()Ljava/lang/String;", &[]),
        "https://example.test/one"
    );
    assert_eq!(
        call(&mut engine, view, "canGoForward", "()Z", &[]),
        Value::Int(1)
    );
    call(&mut engine, view, "goForward", "()V", &[]);
    call(&mut engine, view, "onPause", "()V", &[]);
    assert_eq!(
        call(&mut engine, view, "isPaused", "()Z", &[]),
        Value::Int(1)
    );
    call(&mut engine, view, "destroy", "()V", &[]);
    assert_throws(
        engine.invoke_instance(view, "getUrl", "()Ljava/lang/String;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
}

#[test]
fn documents_contract_builds_parses_and_models_paths_without_a_provider() {
    let mut engine = engine_of(&DexBuilder::new());
    let authority = text(&mut engine, "com.example.docs");
    let document_id = text(&mut engine, "root:folder/file.txt");
    let document_uri = call_static(
        &mut engine,
        DOCUMENTS,
        "buildDocumentUri",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
        &[authority.clone(), document_id],
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            DOCUMENTS,
            "getDocumentId",
            "(Landroid/net/Uri;)Ljava/lang/String;",
            std::slice::from_ref(&document_uri),
        ),
        "root:folder/file.txt"
    );

    let tree_id = text(&mut engine, "tree-root");
    let tree_uri = call_static(
        &mut engine,
        DOCUMENTS,
        "buildTreeDocumentUri",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
        &[authority, tree_id],
    );
    assert_eq!(
        call_static(
            &mut engine,
            DOCUMENTS,
            "isTreeUri",
            "(Landroid/net/Uri;)Z",
            std::slice::from_ref(&tree_uri),
        ),
        Value::Int(1)
    );
    let child_id = text(&mut engine, "child");
    let child_uri = call_static(
        &mut engine,
        DOCUMENTS,
        "buildDocumentUriUsingTree",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
        &[tree_uri, child_id],
    );
    assert_eq!(
        uri_text(&mut engine, child_uri.clone()),
        "content://com.example.docs/tree/tree-root/document/child"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            DOCUMENTS,
            "getTreeDocumentId",
            "(Landroid/net/Uri;)Ljava/lang/String;",
            &[child_uri],
        ),
        "tree-root"
    );

    // No installed DocumentsProvider answers PROVIDER_INTERFACE in this
    // profile, so AOSP's provider-backed predicates are legitimately false.
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    assert_eq!(
        call_static(
            &mut engine,
            DOCUMENTS,
            "isDocumentUri",
            "(Landroid/content/Context;Landroid/net/Uri;)Z",
            &[context, document_uri],
        ),
        Value::Int(0)
    );

    let root = text(&mut engine, "root");
    let first = text(&mut engine, "a");
    let second = text(&mut engine, "b");
    let ids = list_of(&mut engine, &[first, second]);
    let path = instance(&mut engine, DOCUMENTS_PATH);
    call(
        &mut engine,
        path,
        "<init>",
        "(Ljava/lang/String;Ljava/util/List;)V",
        &[root.clone(), Value::Obj(ids)],
    );
    // Path retains the caller's List reference rather than copying it.
    assert_eq!(
        object(field(&engine, path, "mPath")),
        ids,
        "AOSP Path retains the caller's list"
    );
    assert_eq!(
        call_text(&mut engine, path, "getRootId", "()Ljava/lang/String;", &[]),
        "root"
    );
    let same_root = text(&mut engine, "root");
    let same_ids = {
        let a = text(&mut engine, "a");
        let b = text(&mut engine, "b");
        list_of(&mut engine, &[a, b])
    };
    let equal_path = instance(&mut engine, DOCUMENTS_PATH);
    call(
        &mut engine,
        equal_path,
        "<init>",
        "(Ljava/lang/String;Ljava/util/List;)V",
        &[same_root, Value::Obj(same_ids)],
    );
    assert_eq!(
        call(
            &mut engine,
            path,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(equal_path)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, path, "hashCode", "()I", &[]),
        call(&mut engine, equal_path, "hashCode", "()I", &[])
    );
}

#[test]
fn no_keystore_security_defaults_and_keychain_results_match_aosp() {
    let mut engine = engine_of(&DexBuilder::new());
    let policy = object(call_static(
        &mut engine,
        POLICY,
        "getInstance",
        "()Landroid/security/NetworkSecurityPolicy;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            policy,
            "isCleartextTrafficPermitted",
            "()Z",
            &[]
        ),
        Value::Int(1)
    );
    let host = text(&mut engine, "example.test");
    assert_eq!(
        call(
            &mut engine,
            policy,
            "isCleartextTrafficPermitted",
            "(Ljava/lang/String;)Z",
            &[host]
        ),
        Value::Int(1)
    );

    for (algorithm, supported) in [("EC", 1), ("rsa", 1), ("DSA", 0)] {
        let value = text(&mut engine, algorithm);
        assert_eq!(
            call_static(
                &mut engine,
                KEY_CHAIN,
                "isKeyAlgorithmSupported",
                "(Ljava/lang/String;)Z",
                &[value],
            ),
            Value::Int(supported)
        );
    }
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    let alias = text(&mut engine, "missing");
    assert_eq!(
        call_static(
            &mut engine,
            KEY_CHAIN,
            "getPrivateKey",
            "(Landroid/content/Context;Ljava/lang/String;)Ljava/security/PrivateKey;",
            &[context.clone(), alias.clone()],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            KEY_CHAIN,
            "getCertificateChain",
            "(Landroid/content/Context;Ljava/lang/String;)[Ljava/security/cert/X509Certificate;",
            &[context, alias],
        ),
        Value::Null
    );

    let intent = object(call_static(
        &mut engine,
        KEY_CHAIN,
        "createInstallIntent",
        "()Landroid/content/Intent;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            intent,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "android.credentials.INSTALL"
    );

    let activity = Value::Obj(instance(&mut engine, "Landroid/app/Activity;"));
    let callback = instance(&mut engine, "Landroid/security/KeyChainAliasCallback;");
    call_static(
        &mut engine,
        KEY_CHAIN,
        "choosePrivateKeyAlias",
        "(Landroid/app/Activity;Landroid/security/KeyChainAliasCallback;[Ljava/lang/String;[Ljava/security/Principal;Ljava/lang/String;ILjava/lang/String;)V",
        &[
            activity,
            Value::Obj(callback),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(-1),
            Value::Null,
        ],
    );
    assert_eq!(field(&engine, callback, "rd_alias_called"), Value::Int(1));
    assert_eq!(field(&engine, callback, "rd_alias"), Value::Null);
}

fn list_size(engine: &mut Engine, value: Value) -> i32 {
    let list = object(value);
    match call(engine, list, "size", "()I", &[]) {
        Value::Int(size) => size,
        other => panic!("expected list size, got {other:?}"),
    }
}
fn new_builder(engine: &mut Engine, class: &str, sig: &str, args: &[Value]) -> ObjRef {
    let builder = instance(engine, class);
    call(engine, builder, "<init>", sig, args);
    builder
}

#[test]
fn textclassifier_values_normalize_entities_and_no_op_returns_defaults() {
    let mut engine = engine_of(&DexBuilder::new());

    let selection_builder = new_builder(
        &mut engine,
        "Landroid/view/textclassifier/TextSelection$Builder;",
        "(II)V",
        &[Value::Int(2), Value::Int(9)],
    );
    for (entity, score) in [("email", 0.4), ("url", 2.0), ("other", 0.0)] {
        let value = text(&mut engine, entity);
        call(
            &mut engine,
            selection_builder,
            "setEntityType",
            "(Ljava/lang/String;F)Landroid/view/textclassifier/TextSelection$Builder;",
            &[value, Value::Float(score)],
        );
    }
    let selection = object(call(
        &mut engine,
        selection_builder,
        "build",
        "()Landroid/view/textclassifier/TextSelection;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, selection, "getSelectionStartIndex", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, selection, "getSelectionEndIndex", "()I", &[]),
        Value::Int(9)
    );
    // Scores <= 0 are dropped, scores > 1 clamp to 1, and entities are
    // sorted by descending confidence.
    assert_eq!(
        call(&mut engine, selection, "getEntityCount", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_text(
            &mut engine,
            selection,
            "getEntity",
            "(I)Ljava/lang/String;",
            &[Value::Int(0)]
        ),
        "url"
    );
    let url = text(&mut engine, "url");
    assert_eq!(
        call(
            &mut engine,
            selection,
            "getConfidenceScore",
            "(Ljava/lang/String;)F",
            &[url]
        ),
        Value::Float(1.0)
    );

    let invalid_builder = instance(
        &mut engine,
        "Landroid/view/textclassifier/TextSelection$Builder;",
    );
    assert_throws(
        engine.invoke_instance(
            invalid_builder,
            "<init>",
            "(II)V",
            &[Value::Int(4), Value::Int(4)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let classification_builder = new_builder(
        &mut engine,
        "Landroid/view/textclassifier/TextClassification$Builder;",
        "()V",
        &[],
    );
    let classified_text = text(&mut engine, "call me");
    call(
        &mut engine,
        classification_builder,
        "setText",
        "(Ljava/lang/String;)Landroid/view/textclassifier/TextClassification$Builder;",
        &[classified_text],
    );
    let entity = text(&mut engine, "phone");
    call(
        &mut engine,
        classification_builder,
        "setEntityType",
        "(Ljava/lang/String;F)Landroid/view/textclassifier/TextClassification$Builder;",
        &[entity, Value::Float(0.75)],
    );
    let classification = object(call(
        &mut engine,
        classification_builder,
        "build",
        "()Landroid/view/textclassifier/TextClassification;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            classification,
            "getText",
            "()Ljava/lang/String;",
            &[]
        ),
        "call me"
    );
    let actions = call(
        &mut engine,
        classification,
        "getActions",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(list_size(&mut engine, actions), 0);
    assert_eq!(
        call_text(
            &mut engine,
            classification,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "TextClassification {text=call me, entities={phone=0.75}, actions=[], id=null, extras=Bundle[0 keys]}"
    );

    // ULocale/TextLanguage values.
    let language_builder = new_builder(
        &mut engine,
        "Landroid/view/textclassifier/TextLanguage$Builder;",
        "()V",
        &[],
    );
    let locale = {
        let language = text(&mut engine, "en");
        let country = text(&mut engine, "US");
        new_builder(
            &mut engine,
            "Landroid/icu/util/ULocale;",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[language, country],
        )
    };
    call(
        &mut engine,
        language_builder,
        "putLocale",
        "(Landroid/icu/util/ULocale;F)Landroid/view/textclassifier/TextLanguage$Builder;",
        &[Value::Obj(locale), Value::Float(0.9)],
    );
    let language = object(call(
        &mut engine,
        language_builder,
        "build",
        "()Landroid/view/textclassifier/TextLanguage;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            language,
            "getLocaleHypothesisCount",
            "()I",
            &[]
        ),
        Value::Int(1)
    );
    let detected = object(call(
        &mut engine,
        language,
        "getLocale",
        "(I)Landroid/icu/util/ULocale;",
        &[Value::Int(0)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            detected,
            "toLanguageTag",
            "()Ljava/lang/String;",
            &[]
        ),
        "en-US"
    );

    // Manager -> default classifier: no model, but exact NO_OP behavior.
    let manager = instance(
        &mut engine,
        "Landroid/view/textclassifier/TextClassificationManager;",
    );
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    call(
        &mut engine,
        manager,
        "<init>",
        "(Landroid/content/Context;)V",
        &[context],
    );
    let classifier = object(call(
        &mut engine,
        manager,
        "getTextClassifier",
        "()Landroid/view/textclassifier/TextClassifier;",
        &[],
    ));
    let request_builder = {
        let input = text(&mut engine, "hello world");
        new_builder(
            &mut engine,
            "Landroid/view/textclassifier/TextSelection$Request$Builder;",
            "(Ljava/lang/CharSequence;II)V",
            &[input, Value::Int(6), Value::Int(11)],
        )
    };
    let request = object(call(
        &mut engine,
        request_builder,
        "build",
        "()Landroid/view/textclassifier/TextSelection$Request;",
        &[],
    ));
    let suggested = object(call(
        &mut engine,
        classifier,
        "suggestSelection",
        "(Landroid/view/textclassifier/TextSelection$Request;)Landroid/view/textclassifier/TextSelection;",
        &[Value::Obj(request)],
    ));
    assert_eq!(
        call(&mut engine, suggested, "getSelectionStartIndex", "()I", &[]),
        Value::Int(6)
    );
    assert_eq!(
        call(&mut engine, suggested, "getEntityCount", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            classifier,
            "getMaxGenerateLinksTextLength",
            "()I",
            &[]
        ),
        Value::Int(i32::MAX)
    );

    // Explicit EntityConfig resolution excludes classifier defaults.
    let included = {
        let email = text(&mut engine, "email");
        list_of(&mut engine, &[email])
    };
    let config = object(call_static(
        &mut engine,
        "Landroid/view/textclassifier/TextClassifier$EntityConfig;",
        "createWithExplicitEntityList",
        "(Ljava/util/Collection;)Landroid/view/textclassifier/TextClassifier$EntityConfig;",
        &[Value::Obj(included)],
    ));
    assert_eq!(
        call(
            &mut engine,
            config,
            "shouldIncludeTypesFromTextClassifier",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    let defaults = {
        let url = text(&mut engine, "url");
        let phone = text(&mut engine, "phone");
        list_of(&mut engine, &[url, phone])
    };
    let resolved = call(
        &mut engine,
        config,
        "resolveEntityListModifications",
        "(Ljava/util/Collection;)Ljava/util/Collection;",
        &[Value::Obj(defaults)],
    );
    assert_eq!(list_size(&mut engine, resolved), 1);
}

fn class_object(engine: &mut Engine, descriptor: &str) -> Value {
    let class = instance(engine, "Ljava/lang/Class;");
    let name = text(engine, descriptor);
    engine.heap.put_field(class, "class_name", name).unwrap();
    Value::Obj(class)
}
fn boxed_int(engine: &mut Engine, value: i32) -> Value {
    let boxed = instance(engine, "Ljava/lang/Integer;");
    engine
        .heap
        .put_field(boxed, "value", Value::Int(value))
        .unwrap();
    Value::Obj(boxed)
}
fn native_key(engine: &mut Engine, wrapper: Option<&str>, name: &str) -> ObjRef {
    let class = class_object(engine, "Ljava/lang/Integer;");
    native_key_with_class(engine, wrapper, name, class)
}
fn native_key_with_class(
    engine: &mut Engine,
    wrapper: Option<&str>,
    name: &str,
    class: Value,
) -> ObjRef {
    let key_name = text(engine, name);
    new_builder(
        engine,
        wrapper.unwrap_or(NATIVE_KEY),
        "(Ljava/lang/String;Ljava/lang/Class;)V",
        &[key_name, class],
    )
}
fn empty_metadata(engine: &mut Engine) -> ObjRef {
    new_builder(engine, METADATA_NATIVE, "()V", &[])
}
fn metadata_set(engine: &mut Engine, metadata: ObjRef, key: ObjRef, value: Value) {
    // Erasure makes the useful host-level signature Object/Object; the
    // implementation dispatches on the actual key object's class.
    call(
        engine,
        metadata,
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        &[Value::Obj(key), value],
    );
}
fn metadata_get(engine: &mut Engine, metadata: ObjRef, key: ObjRef) -> Value {
    call(
        engine,
        metadata,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Obj(key)],
    )
}

#[test]
fn camera_metadata_keys_characteristics_requests_and_results_use_explicit_data() {
    let mut engine = engine_of(&DexBuilder::new());
    // Guest code receives one canonical Class object per descriptor.
    let integer_class = class_object(&mut engine, "Ljava/lang/Integer;");
    let native = native_key_with_class(
        &mut engine,
        None,
        "android.control.aeMode",
        integer_class.clone(),
    );
    let wrapper = native_key_with_class(
        &mut engine,
        Some(CAPTURE_REQUEST_KEY),
        "android.control.aeMode",
        integer_class,
    );
    assert_eq!(
        call_text(&mut engine, wrapper, "getName", "()Ljava/lang/String;", &[]),
        "android.control.aeMode"
    );
    // AOSP native-key equality unwraps wrappers, but wrapper equality is
    // deliberately one-way and requires the same wrapper class.
    assert_eq!(
        call(
            &mut engine,
            native,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(wrapper)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            wrapper,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(native)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, native, "hashCode", "()I", &[]),
        call(&mut engine, wrapper, "hashCode", "()I", &[])
    );

    let metadata = empty_metadata(&mut engine);
    let value = boxed_int(&mut engine, 7);
    metadata_set(&mut engine, metadata, wrapper, value.clone());
    assert_eq!(metadata_get(&mut engine, metadata, native), value);
    metadata_set(&mut engine, metadata, wrapper, Value::Null);
    assert_eq!(
        call(&mut engine, metadata, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );

    // CameraCharacteristics takes ownership by moving the supplied metadata.
    let characteristics_key = native_key(
        &mut engine,
        Some(CHARACTERISTICS_KEY),
        "android.info.supportedHardwareLevel",
    );
    let level = boxed_int(&mut engine, 3);
    metadata_set(&mut engine, metadata, characteristics_key, level.clone());
    let characteristics = new_builder(
        &mut engine,
        CHARACTERISTICS,
        "(Landroid/hardware/camera2/impl/CameraMetadataNative;)V",
        &[Value::Obj(metadata)],
    );
    assert_eq!(
        call(&mut engine, metadata, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            characteristics,
            "get",
            "(Landroid/hardware/camera2/CameraCharacteristics$Key;)Ljava/lang/Object;",
            &[Value::Obj(characteristics_key)]
        ),
        level
    );
    let keys = call(
        &mut engine,
        characteristics,
        "getKeys",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(list_size(&mut engine, keys), 1);

    // Build a request from a template, exercise a physical-camera entry,
    // target set, tag, and immutable build copy.
    let template = empty_metadata(&mut engine);
    let template_value = boxed_int(&mut engine, 1);
    metadata_set(&mut engine, template, wrapper, template_value.clone());
    let logical_id = text(&mut engine, "0");
    let physical_id = text(&mut engine, "1");
    let physical_ids = list_of(&mut engine, &[physical_id]);
    let builder = new_builder(
        &mut engine,
        CAPTURE_REQUEST_BUILDER,
        "(Landroid/hardware/camera2/impl/CameraMetadataNative;ZILjava/lang/String;Ljava/util/Set;)V",
        &[
            Value::Obj(template),
            Value::Int(0),
            Value::Int(-1),
            logical_id,
            Value::Obj(physical_ids),
        ],
    );
    assert_eq!(
        call(&mut engine, template, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );
    let changed = boxed_int(&mut engine, 9);
    call(
        &mut engine,
        builder,
        "set",
        "(Landroid/hardware/camera2/CaptureRequest$Key;Ljava/lang/Object;)V",
        &[Value::Obj(wrapper), changed.clone()],
    );
    let physical_query = text(&mut engine, "1");
    assert_eq!(
        call(
            &mut engine,
            builder,
            "getPhysicalCameraKey",
            "(Landroid/hardware/camera2/CaptureRequest$Key;Ljava/lang/String;)Ljava/lang/Object;",
            &[Value::Obj(wrapper), physical_query]
        ),
        template_value,
        "physical settings start as a copy of the logical template"
    );
    let unknown_id = text(&mut engine, "missing");
    assert_throws(
        engine.invoke_instance(
            builder,
            "getPhysicalCameraKey",
            "(Landroid/hardware/camera2/CaptureRequest$Key;Ljava/lang/String;)Ljava/lang/Object;",
            &[Value::Obj(wrapper), unknown_id],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let surface = Value::Obj(instance(&mut engine, "Landroid/view/Surface;"));
    call(
        &mut engine,
        builder,
        "addTarget",
        "(Landroid/view/Surface;)V",
        std::slice::from_ref(&surface),
    );
    let tag = text(&mut engine, "preview");
    call(
        &mut engine,
        builder,
        "setTag",
        "(Ljava/lang/Object;)V",
        &[tag],
    );
    let request = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/hardware/camera2/CaptureRequest;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            request,
            "get",
            "(Landroid/hardware/camera2/CaptureRequest$Key;)Ljava/lang/Object;",
            &[Value::Obj(wrapper)]
        ),
        changed
    );
    assert_eq!(
        call_text(&mut engine, request, "getTag", "()Ljava/lang/Object;", &[]),
        "preview"
    );
    assert_eq!(
        call(
            &mut engine,
            request,
            "containsTarget",
            "(Landroid/view/Surface;)Z",
            &[surface]
        ),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(request, "getReprocessableSessionId", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    // A second build from the same builder is a distinct immutable copy with
    // the same AOSP equality fields, so equals/hashCode must agree.
    let same_request = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/hardware/camera2/CaptureRequest;",
        &[],
    ));
    assert_ne!(request, same_request);
    assert_eq!(
        call(
            &mut engine,
            request,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_request)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, request, "hashCode", "()I", &[]),
        call(&mut engine, same_request, "hashCode", "()I", &[])
    );

    // Reprocess validation and session propagation.
    let reprocess_template = empty_metadata(&mut engine);
    let logical = text(&mut engine, "0");
    let bad_builder = instance(&mut engine, CAPTURE_REQUEST_BUILDER);
    assert_throws(
        engine.invoke_instance(
            bad_builder,
            "<init>",
            "(Landroid/hardware/camera2/impl/CameraMetadataNative;ZILjava/lang/String;Ljava/util/Set;)V",
            &[
                Value::Obj(reprocess_template),
                Value::Int(1),
                Value::Int(-1),
                logical.clone(),
                Value::Null,
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let valid_template = empty_metadata(&mut engine);
    let reprocess_builder = new_builder(
        &mut engine,
        CAPTURE_REQUEST_BUILDER,
        "(Landroid/hardware/camera2/impl/CameraMetadataNative;ZILjava/lang/String;Ljava/util/Set;)V",
        &[
            Value::Obj(valid_template),
            Value::Int(1),
            Value::Int(7),
            logical,
            Value::Null,
        ],
    );
    let reprocess = object(call(
        &mut engine,
        reprocess_builder,
        "build",
        "()Landroid/hardware/camera2/CaptureRequest;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            reprocess,
            "getReprocessableSessionId",
            "()I",
            &[]
        ),
        Value::Int(7)
    );

    // A request-less result requires non-empty moved metadata.
    let result_key = native_key(
        &mut engine,
        Some(CAPTURE_RESULT_KEY),
        "android.sensor.timestamp",
    );
    let result_metadata = empty_metadata(&mut engine);
    let timestamp = boxed_int(&mut engine, 1234);
    metadata_set(&mut engine, result_metadata, result_key, timestamp.clone());
    let total = new_builder(
        &mut engine,
        TOTAL_CAPTURE_RESULT,
        "(Landroid/hardware/camera2/impl/CameraMetadataNative;I)V",
        &[Value::Obj(result_metadata), Value::Int(42)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            total,
            "getCameraId",
            "()Ljava/lang/String;",
            &[]
        ),
        "none"
    );
    assert_eq!(
        call(&mut engine, total, "getSequenceId", "()I", &[]),
        Value::Int(42)
    );
    assert_eq!(
        call(&mut engine, total, "getFrameNumber", "()J", &[]),
        Value::Long(-1)
    );
    assert_eq!(
        call(
            &mut engine,
            total,
            "get",
            "(Landroid/hardware/camera2/CaptureResult$Key;)Ljava/lang/Object;",
            &[Value::Obj(result_key)]
        ),
        timestamp
    );
    let partials = call(
        &mut engine,
        total,
        "getPartialResults",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(list_size(&mut engine, partials), 0);
}

#[test]
fn camera_manager_has_no_cameras_and_access_exception_messages_are_exact() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = instance(&mut engine, CAMERA_MANAGER);
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    call(
        &mut engine,
        manager,
        "<init>",
        "(Landroid/content/Context;)V",
        &[context],
    );
    let ids = call(
        &mut engine,
        manager,
        "getCameraIdList",
        "()[Ljava/lang/String;",
        &[],
    );
    assert!(array_values(&engine, &ids).is_empty());
    let concurrent = call(
        &mut engine,
        manager,
        "getConcurrentCameraIds",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(list_size(&mut engine, concurrent), 0);
    let camera_id = text(&mut engine, "0");
    assert_throws(
        engine.invoke_instance(
            manager,
            "getCameraCharacteristics",
            "(Ljava/lang/String;)Landroid/hardware/camera2/CameraCharacteristics;",
            &[camera_id],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "openCamera",
            "(Ljava/lang/String;Landroid/hardware/camera2/CameraDevice$StateCallback;Landroid/os/Handler;)V",
            &[Value::Null, Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let callback = Value::Obj(instance(
        &mut engine,
        "Landroid/hardware/camera2/CameraDevice$StateCallback;",
    ));
    let camera_id = text(&mut engine, "0");
    assert_throws(
        engine.invoke_instance(
            manager,
            "openCamera",
            "(Ljava/lang/String;Landroid/hardware/camera2/CameraDevice$StateCallback;Landroid/os/Handler;)V",
            &[camera_id, callback, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let message = text(&mut engine, "busy");
    let exception = new_builder(
        &mut engine,
        CAMERA_ACCESS_EXCEPTION,
        "(ILjava/lang/String;)V",
        &[Value::Int(4), message],
    );
    assert_eq!(
        call(&mut engine, exception, "getReason", "()I", &[]),
        Value::Int(4)
    );
    assert_eq!(
        call_text(
            &mut engine,
            exception,
            "getMessage",
            "()Ljava/lang/String;",
            &[]
        ),
        "CAMERA_IN_USE (4): busy"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            CAMERA_ACCESS_EXCEPTION,
            "getDefaultMessage",
            "(I)Ljava/lang/String;",
            &[Value::Int(1)]
        ),
        "The camera is disabled due to a device policy, and cannot be opened."
    );
    assert_eq!(
        call_static(
            &mut engine,
            CAMERA_ACCESS_EXCEPTION,
            "getDefaultMessage",
            "(I)Ljava/lang/String;",
            &[Value::Int(999)]
        ),
        Value::Null
    );
}

fn stream_config(engine: &mut Engine, format: i32, width: i32, height: i32, input: bool) -> ObjRef {
    new_builder(
        engine,
        STREAM_CONFIG,
        "(IIIZ)V",
        &[
            Value::Int(format),
            Value::Int(width),
            Value::Int(height),
            Value::Int(input as i32),
        ],
    )
}
fn stream_duration(engine: &mut Engine, format: i32, width: i32, height: i32, ns: i64) -> ObjRef {
    new_builder(
        engine,
        STREAM_DURATION,
        "(IIIJ)V",
        &[
            Value::Int(format),
            Value::Int(width),
            Value::Int(height),
            Value::Long(ns),
        ],
    )
}
fn size_pairs(engine: &Engine, value: Value) -> Vec<(i32, i32)> {
    array_values(engine, &value)
        .iter()
        .map(|size| {
            let size = object(size.clone());
            (
                match field(engine, size, "rd_w") {
                    Value::Int(v) => v,
                    _ => 0,
                },
                match field(engine, size, "rd_h") {
                    Value::Int(v) => v,
                    _ => 0,
                },
            )
        })
        .collect()
}
fn size_object(engine: &mut Engine, width: i32, height: i32) -> Value {
    let size = new_builder(
        engine,
        "Landroid/util/Size;",
        "(II)V",
        &[Value::Int(width), Value::Int(height)],
    );
    Value::Obj(size)
}

#[test]
fn camera_params_values_and_stream_map_compute_from_explicit_configs() {
    let mut engine = engine_of(&DexBuilder::new());

    let rectangle = new_builder(
        &mut engine,
        METERING,
        "(IIIII)V",
        &[
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
            Value::Int(500),
        ],
    );
    let rect = object(call(
        &mut engine,
        rectangle,
        "getRect",
        "()Landroid/graphics/Rect;",
        &[],
    ));
    assert_eq!(field(&engine, rect, "right"), Value::Int(40));
    assert_eq!(field(&engine, rect, "bottom"), Value::Int(60));
    assert_eq!(
        call_text(
            &mut engine,
            rectangle,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "(x:10, y:20, w:30, h:40, wt:500)"
    );
    let invalid_rectangle = instance(&mut engine, METERING);
    assert_throws(
        engine.invoke_instance(
            invalid_rectangle,
            "<init>",
            "(IIIII)V",
            &[
                Value::Int(0),
                Value::Int(0),
                Value::Int(1),
                Value::Int(1),
                Value::Int(1001),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let offsets = int_array(&mut engine, &[10, 20, 30, 40, 999]);
    let black = new_builder(&mut engine, BLACK, "([I)V", &[offsets]);
    assert_eq!(
        call(
            &mut engine,
            black,
            "getOffsetForIndex",
            "(II)I",
            &[Value::Int(3), Value::Int(2)]
        ),
        Value::Int(20)
    );
    let destination = int_array(&mut engine, &[0; 6]);
    call(
        &mut engine,
        black,
        "copyTo",
        "([II)V",
        &[destination.clone(), Value::Int(1)],
    );
    assert_eq!(int_values(&engine, &destination), [0, 10, 20, 30, 40, 0]);

    let vector = new_builder(
        &mut engine,
        RGGB,
        "(FFFF)V",
        &[
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            vector,
            "getComponent",
            "(I)F",
            &[Value::Int(2)]
        ),
        Value::Float(3.0)
    );
    let out = float_array(&mut engine, 4);
    call(
        &mut engine,
        vector,
        "copyTo",
        "([FI)V",
        &[out.clone(), Value::Int(0)],
    );
    assert_eq!(
        array_values(&engine, &out),
        [
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0)
        ]
    );

    let mut matrix = [0; 18];
    for i in 0..9 {
        matrix[i * 2] = (i + 1) as i32;
        matrix[i * 2 + 1] = 1;
    }
    let matrix_array = int_array(&mut engine, &matrix);
    let transform = new_builder(&mut engine, COLOR_TRANSFORM, "([I)V", &[matrix_array]);
    let element = object(call(
        &mut engine,
        transform,
        "getElement",
        "(II)Landroid/util/Rational;",
        &[Value::Int(1), Value::Int(2)],
    ));
    assert_eq!(field(&engine, element, "rd_num"), Value::Int(8));

    // Internal formats: IMPLEMENTATION_DEFINED/PRIVATE, YUV, and BLOB.
    let configs = vec![
        Value::Obj(stream_config(&mut engine, 0x22, 1920, 1080, false)),
        Value::Obj(stream_config(&mut engine, 0x23, 1280, 720, false)),
        Value::Obj(stream_config(&mut engine, 0x21, 4000, 3000, false)),
        Value::Obj(stream_config(&mut engine, 0x23, 640, 480, true)),
    ];
    let min_durations = vec![
        Value::Obj(stream_duration(&mut engine, 0x22, 1920, 1080, 33_000_000)),
        Value::Obj(stream_duration(&mut engine, 0x23, 1280, 720, 16_000_000)),
        Value::Obj(stream_duration(&mut engine, 0x21, 4000, 3000, 100_000_000)),
    ];
    let stall_durations = vec![Value::Obj(stream_duration(
        &mut engine,
        0x21,
        4000,
        3000,
        200_000_000,
    ))];
    let config_array = object_array(
        &mut engine,
        "[Landroid/hardware/camera2/params/StreamConfiguration;",
        configs,
    );
    let min_array = object_array(
        &mut engine,
        "[Landroid/hardware/camera2/params/StreamConfigurationDuration;",
        min_durations,
    );
    let stall_array = object_array(
        &mut engine,
        "[Landroid/hardware/camera2/params/StreamConfigurationDuration;",
        stall_durations,
    );
    let high_speed = new_builder(
        &mut engine,
        HIGH_SPEED,
        "(IIIII)V",
        &[
            Value::Int(1280),
            Value::Int(720),
            Value::Int(30),
            Value::Int(120),
            Value::Int(2),
        ],
    );
    let high_speed_array = object_array(
        &mut engine,
        "[Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;",
        vec![Value::Obj(high_speed)],
    );
    let mut args: Vec<Value> = vec![config_array, min_array, stall_array];
    args.extend(std::iter::repeat_n(Value::Null, 12));
    args.push(high_speed_array);
    args.push(Value::Null);
    args.push(Value::Int(1));
    let map = new_builder(
        &mut engine,
        STREAM_MAP,
        "([Landroid/hardware/camera2/params/StreamConfiguration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfiguration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfiguration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfiguration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfiguration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/StreamConfigurationDuration;[Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;Landroid/hardware/camera2/params/ReprocessFormatsMap;Z)V",
        &args,
    );
    // Internal keys sort as BLOB, PRIVATE, YUV; BLOB publishes as JPEG.
    let formats = call(&mut engine, map, "getOutputFormats", "()[I", &[]);
    assert_eq!(int_values(&engine, &formats), [0x100, 0x22, 0x23]);
    let private_sizes = call(
        &mut engine,
        map,
        "getOutputSizes",
        "(I)[Landroid/util/Size;",
        &[Value::Int(0x22)],
    );
    assert_eq!(size_pairs(&engine, private_sizes), [(1920, 1080)]);
    let jpeg_sizes = call(
        &mut engine,
        map,
        "getOutputSizes",
        "(I)[Landroid/util/Size;",
        &[Value::Int(0x100)],
    );
    assert!(size_pairs(&engine, jpeg_sizes).is_empty());
    let high_res_sizes = call(
        &mut engine,
        map,
        "getHighResolutionOutputSizes",
        "(I)[Landroid/util/Size;",
        &[Value::Int(0x100)],
    );
    assert_eq!(size_pairs(&engine, high_res_sizes), [(4000, 3000)]);
    let input_sizes = call(
        &mut engine,
        map,
        "getInputSizes",
        "(I)[Landroid/util/Size;",
        &[Value::Int(0x23)],
    );
    assert_eq!(size_pairs(&engine, input_sizes), [(640, 480)]);
    let jpeg_size = size_object(&mut engine, 4000, 3000);
    assert_eq!(
        call(
            &mut engine,
            map,
            "getOutputMinFrameDuration",
            "(ILandroid/util/Size;)J",
            &[Value::Int(0x100), jpeg_size]
        ),
        Value::Long(100_000_000)
    );
    let high_speed_sizes = call(
        &mut engine,
        map,
        "getHighSpeedVideoSizes",
        "()[Landroid/util/Size;",
        &[],
    );
    assert_eq!(size_pairs(&engine, high_speed_sizes), [(1280, 720)]);

    let entry = int_array(&mut engine, &[0x23, 2, 0x21, 0x23, 0x21, 1, 0x23]);
    let reprocess = new_builder(&mut engine, REPROCESS, "([I)V", &[entry]);
    let inputs = call(&mut engine, reprocess, "getInputs", "()[I", &[]);
    assert_eq!(int_values(&engine, &inputs), [0x23, 0x100]);
    let outputs = call(
        &mut engine,
        reprocess,
        "getOutputs",
        "(I)[I",
        &[Value::Int(0x23)],
    );
    assert_eq!(int_values(&engine, &outputs), [0x100, 0x23]);
    // This preserves AOSP's direct internal-format comparison: BLOB is an
    // input internally even though getInputs publishes it as JPEG.
    assert_throws(
        engine.invoke_instance(reprocess, "getOutputs", "(I)[I", &[Value::Int(0x100)]),
        "Ljava/lang/IllegalArgumentException;",
    );
    let outputs = call(
        &mut engine,
        reprocess,
        "getOutputs",
        "(I)[I",
        &[Value::Int(0x21)],
    );
    assert_eq!(int_values(&engine, &outputs), [0x23]);
}

#[test]
fn drafted_stub_surface_classpath_subtypes_are_visible() {
    let engine = engine_of(&DexBuilder::new());
    let camera_metadata = "Landroid/hardware/camera2/CameraMetadata;";
    let parcelable = "Landroid/os/Parcelable;";
    assert!(engine.cp.is_subtype(CHARACTERISTICS, camera_metadata));
    assert!(engine.cp.is_subtype(CAPTURE_REQUEST, camera_metadata));
    assert!(engine.cp.is_subtype(CAPTURE_REQUEST, parcelable));
    assert!(engine.cp.is_subtype(CAPTURE_RESULT, camera_metadata));
    assert!(engine.cp.is_subtype(TOTAL_CAPTURE_RESULT, CAPTURE_RESULT));
    assert!(engine.cp.is_subtype(TOTAL_CAPTURE_RESULT, camera_metadata));
    assert!(engine.cp.is_subtype(METADATA_NATIVE, parcelable));
    assert!(engine
        .cp
        .is_subtype(CAMERA_ACCESS_EXCEPTION, "Landroid/util/AndroidException;"));
    assert!(engine
        .cp
        .is_subtype(CAMERA_ACCESS_EXCEPTION, "Ljava/lang/Exception;"));
    assert!(engine
        .cp
        .is_subtype(CAMERA_ACCESS_EXCEPTION, "Ljava/lang/Throwable;"));
    assert!(engine
        .cp
        .is_subtype(CAMERA_ACCESS_EXCEPTION, "Ljava/lang/Object;"));
    assert!(engine.cp.is_subtype(
        "Landroid/view/textclassifier/TextClassification;",
        parcelable
    ));
    assert!(engine
        .cp
        .is_subtype("Landroid/view/textclassifier/TextSelection;", parcelable));
    assert!(engine
        .cp
        .is_subtype("Landroid/icu/util/ULocale;", "Ljava/lang/Cloneable;"));
    assert!(engine
        .cp
        .is_subtype("Landroid/icu/util/ULocale;", "Ljava/io/Serializable;"));
    assert!(engine
        .cp
        .is_subtype("Landroid/icu/util/ULocale;", "Ljava/lang/Comparable;"));
}

const ANIMATOR: &str = "Landroid/animation/Animator;";
const VALUE_ANIMATOR: &str = "Landroid/animation/ValueAnimator;";
const OBJECT_ANIMATOR: &str = "Landroid/animation/ObjectAnimator;";
const PROPERTY_VALUES_HOLDER: &str = "Landroid/animation/PropertyValuesHolder;";
const KEYFRAME: &str = "Landroid/animation/Keyframe;";

fn float_value_array(engine: &mut Engine, values: &[f32]) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Float, values.len(), "[F")
        .unwrap();
    *engine.heap.array_elems_mut(array).unwrap() =
        values.iter().copied().map(Value::Float).collect();
    Value::Obj(array)
}
fn boxed_float(engine: &mut Engine, value: f32) -> Value {
    let boxed = instance(engine, "Ljava/lang/Float;");
    engine
        .heap
        .put_field(boxed, "value", Value::Float(value))
        .unwrap();
    Value::Obj(boxed)
}
fn boxed_int_value(engine: &Engine, value: &Value) -> i32 {
    match field(engine, object(value.clone()), "value") {
        Value::Int(v) => v,
        other => panic!("expected boxed int value, got {other:?}"),
    }
}
fn boxed_float_value(engine: &Engine, value: &Value) -> f32 {
    match field(engine, object(value.clone()), "value") {
        Value::Float(v) => v,
        other => panic!("expected boxed float value, got {other:?}"),
    }
}
fn set_linear_interpolator(engine: &mut Engine, animator: ObjRef) {
    let linear = instance(engine, "Landroid/view/animation/LinearInterpolator;");
    call(engine, linear, "<init>", "()V", &[]);
    call(
        engine,
        animator,
        "setInterpolator",
        "(Landroid/animation/TimeInterpolator;)V",
        &[Value::Obj(linear)],
    );
}

#[test]
fn value_animator_runs_on_virtual_clock_and_seeks_with_repeats() {
    let mut engine = engine_of(&DexBuilder::new());
    let values = int_array(&mut engine, &[0, 100]);
    let animator = object(call_static(
        &mut engine,
        VALUE_ANIMATOR,
        "ofInt",
        "([I)Landroid/animation/ValueAnimator;",
        std::slice::from_ref(&values),
    ));
    assert_eq!(
        call(&mut engine, animator, "getDuration", "()J", &[]),
        Value::Long(300)
    );
    assert_throws(
        engine.invoke_instance(
            animator,
            "setDuration",
            "(J)Landroid/animation/ValueAnimator;",
            &[Value::Long(-1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_eq!(
        call(
            &mut engine,
            animator,
            "setDuration",
            "(J)Landroid/animation/ValueAnimator;",
            &[Value::Long(160)]
        ),
        Value::Obj(animator)
    );
    set_linear_interpolator(&mut engine, animator);
    let listener = instance(&mut engine, "Landroid/animation/AnimatorListenerAdapter;");
    call(&mut engine, listener, "<init>", "()V", &[]);
    call(
        &mut engine,
        animator,
        "addListener",
        "(Landroid/animation/Animator$AnimatorListener;)V",
        &[Value::Obj(listener)],
    );
    let update_listener = instance(
        &mut engine,
        "Landroid/animation/ValueAnimator$AnimatorUpdateListener;",
    );
    call(
        &mut engine,
        animator,
        "addUpdateListener",
        "(Landroid/animation/ValueAnimator$AnimatorUpdateListener;)V",
        &[Value::Obj(update_listener)],
    );
    call(&mut engine, animator, "start", "()V", &[]);
    assert_eq!(
        call(&mut engine, animator, "isStarted", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, animator, "isRunning", "()Z", &[]),
        Value::Int(1)
    );
    let initial = call(
        &mut engine,
        animator,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_int_value(&engine, &initial), 0);

    // The first Choreographer pulse establishes the AOSP start time.
    engine.advance_clock(16).unwrap();
    engine.advance_clock(80).unwrap();
    let halfway = call(
        &mut engine,
        animator,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_int_value(&engine, &halfway), 50);
    assert_eq!(
        call(&mut engine, animator, "getAnimatedFraction", "()F", &[]),
        Value::Float(0.5)
    );
    engine.advance_clock(80).unwrap();
    let finished = call(
        &mut engine,
        animator,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_int_value(&engine, &finished), 100);
    assert_eq!(
        call(&mut engine, animator, "isRunning", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(field(&engine, listener, "rd_on_start_count"), Value::Int(1));
    assert_eq!(field(&engine, listener, "rd_on_end_count"), Value::Int(1));
    assert!(matches!(
        field(&engine, update_listener, "rd_on_update_count"),
        Value::Int(n) if n > 0
    ));

    let float_values = float_value_array(&mut engine, &[0.0, 10.0]);
    let repeating = object(call_static(
        &mut engine,
        VALUE_ANIMATOR,
        "ofFloat",
        "([F)Landroid/animation/ValueAnimator;",
        std::slice::from_ref(&float_values),
    ));
    call(
        &mut engine,
        repeating,
        "setDuration",
        "(J)Landroid/animation/ValueAnimator;",
        &[Value::Long(100)],
    );
    set_linear_interpolator(&mut engine, repeating);
    call(
        &mut engine,
        repeating,
        "setRepeatCount",
        "(I)V",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        repeating,
        "setRepeatMode",
        "(I)V",
        &[Value::Int(2)],
    );
    assert_eq!(
        call(&mut engine, repeating, "getTotalDuration", "()J", &[]),
        Value::Long(200)
    );
    call(
        &mut engine,
        repeating,
        "setCurrentFraction",
        "(F)V",
        &[Value::Float(1.25)],
    );
    let reversed = call(
        &mut engine,
        repeating,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_float_value(&engine, &reversed), 7.5);
    assert_eq!(
        call(&mut engine, repeating, "getCurrentPlayTime", "()J", &[]),
        Value::Long(125)
    );
    call(
        &mut engine,
        repeating,
        "setCurrentPlayTime",
        "(J)V",
        &[Value::Long(50)],
    );
    let sought = call(
        &mut engine,
        repeating,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_float_value(&engine, &sought), 5.0);

    let clone = object(call(
        &mut engine,
        repeating,
        "clone",
        "()Landroid/animation/ValueAnimator;",
        &[],
    ));
    assert_ne!(clone, repeating);
    assert_eq!(
        call(&mut engine, clone, "getRepeatCount", "()I", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        clone,
        "setCurrentFraction",
        "(F)V",
        &[Value::Float(0.25)],
    );
    let cloned_value = call(
        &mut engine,
        clone,
        "getAnimatedValue",
        "()Ljava/lang/Object;",
        &[],
    );
    assert_eq!(boxed_float_value(&engine, &cloned_value), 2.5);
}

#[test]
fn keyframes_property_holders_and_evaluators_compute_values() {
    let mut engine = engine_of(&DexBuilder::new());
    let first = call_static(
        &mut engine,
        KEYFRAME,
        "ofFloat",
        "(FF)Landroid/animation/Keyframe;",
        &[Value::Float(0.0), Value::Float(0.0)],
    );
    let middle = call_static(
        &mut engine,
        KEYFRAME,
        "ofFloat",
        "(FF)Landroid/animation/Keyframe;",
        &[Value::Float(0.5), Value::Float(100.0)],
    );
    let last = call_static(
        &mut engine,
        KEYFRAME,
        "ofFloat",
        "(FF)Landroid/animation/Keyframe;",
        &[Value::Float(1.0), Value::Float(0.0)],
    );
    let keyframes = object_array(
        &mut engine,
        "[Landroid/animation/Keyframe;",
        vec![first, middle, last],
    );
    let property_name = text(&mut engine, "value");
    let holder = object(call_static(
        &mut engine,
        PROPERTY_VALUES_HOLDER,
        "ofKeyframe",
        "(Ljava/lang/String;[Landroid/animation/Keyframe;)Landroid/animation/PropertyValuesHolder;",
        &[property_name, keyframes],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            holder,
            "getPropertyName",
            "()Ljava/lang/String;",
            &[]
        ),
        "value"
    );
    let holders = object_array(
        &mut engine,
        "[Landroid/animation/PropertyValuesHolder;",
        vec![Value::Obj(holder)],
    );
    let animator = object(call_static(
        &mut engine,
        VALUE_ANIMATOR,
        "ofPropertyValuesHolder",
        "([Landroid/animation/PropertyValuesHolder;)Landroid/animation/ValueAnimator;",
        std::slice::from_ref(&holders),
    ));
    set_linear_interpolator(&mut engine, animator);
    call(
        &mut engine,
        animator,
        "setCurrentFraction",
        "(F)V",
        &[Value::Float(0.25)],
    );
    let name = text(&mut engine, "value");
    let value = call(
        &mut engine,
        animator,
        "getAnimatedValue",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        std::slice::from_ref(&name),
    );
    assert_eq!(boxed_float_value(&engine, &value), 50.0);
    let returned_holders = call(
        &mut engine,
        animator,
        "getValues",
        "()[Landroid/animation/PropertyValuesHolder;",
        &[],
    );
    assert_eq!(array_values(&engine, &returned_holders).len(), 1);

    let int_evaluator = instance(&mut engine, "Landroid/animation/IntEvaluator;");
    call(&mut engine, int_evaluator, "<init>", "()V", &[]);
    let start = boxed_int(&mut engine, 10);
    let end = boxed_int(&mut engine, 20);
    let evaluated = call(
        &mut engine,
        int_evaluator,
        "evaluate",
        "(FLjava/lang/Integer;Ljava/lang/Integer;)Ljava/lang/Integer;",
        &[Value::Float(0.25), start, end],
    );
    assert_eq!(boxed_int_value(&engine, &evaluated), 12);

    let float_evaluator = instance(&mut engine, "Landroid/animation/FloatEvaluator;");
    call(&mut engine, float_evaluator, "<init>", "()V", &[]);
    let start = boxed_float(&mut engine, 2.0);
    let end = boxed_float(&mut engine, 6.0);
    let evaluated = call(
        &mut engine,
        float_evaluator,
        "evaluate",
        "(FLjava/lang/Number;Ljava/lang/Number;)Ljava/lang/Float;",
        &[Value::Float(0.5), start, end],
    );
    assert_eq!(boxed_float_value(&engine, &evaluated), 4.0);

    let argb = object(call_static(
        &mut engine,
        "Landroid/animation/ArgbEvaluator;",
        "getInstance",
        "()Landroid/animation/ArgbEvaluator;",
        &[],
    ));
    let again = call_static(
        &mut engine,
        "Landroid/animation/ArgbEvaluator;",
        "getInstance",
        "()Landroid/animation/ArgbEvaluator;",
        &[],
    );
    assert_eq!(again, Value::Obj(argb));
    let black = boxed_int(&mut engine, -16_777_216);
    let white = boxed_int(&mut engine, -1);
    let color = call(
        &mut engine,
        argb,
        "evaluate",
        "(FLjava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Float(0.5), black, white],
    );
    assert_eq!(boxed_int_value(&engine, &color), -4_539_718);
}

#[test]
fn object_animator_applies_values_and_pauses_resumes_on_virtual_frames() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let view = instance(&mut engine, "Landroid/view/View;");
    engine
        .attach_host_state("Landroid/view/View;", view)
        .unwrap();
    call(
        &mut engine,
        view,
        "<init>",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    call(&mut engine, view, "setAlpha", "(F)V", &[Value::Float(0.25)]);
    let property = text(&mut engine, "alpha");
    let values = float_value_array(&mut engine, &[0.75]);
    let animator = object(call_static(
        &mut engine,
        OBJECT_ANIMATOR,
        "ofFloat",
        "(Ljava/lang/Object;Ljava/lang/String;[F)Landroid/animation/ObjectAnimator;",
        &[Value::Obj(view), property, values],
    ));
    assert_eq!(
        call(
            &mut engine,
            animator,
            "getTarget",
            "()Ljava/lang/Object;",
            &[]
        ),
        Value::Obj(view)
    );
    assert_eq!(
        call_text(
            &mut engine,
            animator,
            "getPropertyName",
            "()Ljava/lang/String;",
            &[]
        ),
        "alpha"
    );
    call(
        &mut engine,
        animator,
        "setDuration",
        "(J)Landroid/animation/ObjectAnimator;",
        &[Value::Long(160)],
    );
    set_linear_interpolator(&mut engine, animator);
    call(&mut engine, animator, "start", "()V", &[]);
    assert_eq!(
        call(&mut engine, view, "getAlpha", "()F", &[]),
        Value::Float(0.25),
        "the single-value start is read from the target getter"
    );
    engine.advance_clock(16).unwrap();
    engine.advance_clock(80).unwrap();
    assert_eq!(
        call(&mut engine, view, "getAlpha", "()F", &[]),
        Value::Float(0.5)
    );
    engine.advance_clock(80).unwrap();
    assert_eq!(
        call(&mut engine, view, "getAlpha", "()F", &[]),
        Value::Float(0.75)
    );
    assert_eq!(
        call(&mut engine, animator, "isStarted", "()Z", &[]),
        Value::Int(0)
    );

    let property = text(&mut engine, "translationX");
    let values = float_value_array(&mut engine, &[0.0, 40.0]);
    let pausing = object(call_static(
        &mut engine,
        OBJECT_ANIMATOR,
        "ofFloat",
        "(Ljava/lang/Object;Ljava/lang/String;[F)Landroid/animation/ObjectAnimator;",
        &[Value::Obj(view), property, values],
    ));
    call(
        &mut engine,
        pausing,
        "setDuration",
        "(J)Landroid/animation/ObjectAnimator;",
        &[Value::Long(160)],
    );
    set_linear_interpolator(&mut engine, pausing);
    call(&mut engine, pausing, "start", "()V", &[]);
    engine.advance_clock(16).unwrap();
    engine.advance_clock(80).unwrap();
    assert_eq!(
        call(&mut engine, view, "getTranslationX", "()F", &[]),
        Value::Float(20.0)
    );
    call(&mut engine, pausing, "pause", "()V", &[]);
    assert_eq!(
        call(&mut engine, pausing, "isPaused", "()Z", &[]),
        Value::Int(1)
    );
    engine.advance_clock(160).unwrap();
    assert_eq!(
        call(&mut engine, view, "getTranslationX", "()F", &[]),
        Value::Float(20.0)
    );
    call(&mut engine, pausing, "resume", "()V", &[]);
    engine.advance_clock(80).unwrap();
    assert_eq!(
        call(&mut engine, view, "getTranslationX", "()F", &[]),
        Value::Float(40.0)
    );

    assert!(engine.cp.is_subtype(OBJECT_ANIMATOR, VALUE_ANIMATOR));
    assert!(engine.cp.is_subtype(VALUE_ANIMATOR, ANIMATOR));
    assert!(engine
        .cp
        .is_subtype("Landroid/animation/Keyframe$FloatKeyframe;", KEYFRAME));
    assert!(engine.cp.is_subtype(
        "Landroid/animation/ArgbEvaluator;",
        "Landroid/animation/TypeEvaluator;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/animation/AnimatorListenerAdapter;",
        "Landroid/animation/Animator$AnimatorListener;"
    ));
}
