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
    args.extend(std::iter::repeat(Value::Null).take(12));
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
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/AutofillService;",
        "Landroid/app/Service;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/AutofillService;",
        "Landroid/content/ContextWrapper;"
    ));
    assert!(engine
        .cp
        .is_subtype("Landroid/service/autofill/Dataset;", parcelable));
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/RegexValidator;",
        "Landroid/service/autofill/InternalValidator;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/RegexValidator;",
        "Landroid/service/autofill/Validator;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/TextValueSanitizer;",
        "Landroid/service/autofill/InternalSanitizer;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/service/autofill/UserData;",
        "Landroid/service/autofill/FieldClassificationUserData;"
    ));
    assert!(engine
        .cp
        .is_subtype("Landroid/view/autofill/AutofillValue;", parcelable));
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

const AF_ID: &str = "Landroid/view/autofill/AutofillId;";
const AF_VALUE: &str = "Landroid/view/autofill/AutofillValue;";
const AF_DATASET_BUILDER: &str = "Landroid/service/autofill/Dataset$Builder;";
const AF_FIELD_BUILDER: &str = "Landroid/service/autofill/Field$Builder;";
const AF_PRESENTATIONS_BUILDER: &str = "Landroid/service/autofill/Presentations$Builder;";
const AF_FILL_RESPONSE_BUILDER: &str = "Landroid/service/autofill/FillResponse$Builder;";
const AF_SAVE_INFO_BUILDER: &str = "Landroid/service/autofill/SaveInfo$Builder;";
const AF_USER_DATA: &str = "Landroid/service/autofill/UserData;";
const AF_USER_DATA_BUILDER: &str = "Landroid/service/autofill/UserData$Builder;";
const AF_REGEX_VALIDATOR: &str = "Landroid/service/autofill/RegexValidator;";
const AF_TEXT_SANITIZER: &str = "Landroid/service/autofill/TextValueSanitizer;";
const AF_FILL_REQUEST: &str = "Landroid/service/autofill/FillRequest;";
const AF_SAVE_REQUEST: &str = "Landroid/service/autofill/SaveRequest;";
const AF_FILL_CONTEXT: &str = "Landroid/service/autofill/FillContext;";
const AF_FILL_CALLBACK: &str = "Landroid/service/autofill/FillCallback;";
const AF_SAVE_CALLBACK: &str = "Landroid/service/autofill/SaveCallback;";
const AF_FIELD_CLASSIFICATION: &str = "Landroid/service/autofill/FieldClassification;";
const AF_FIELD_CLASSIFICATION_MATCH: &str = "Landroid/service/autofill/FieldClassification$Match;";
const AF_SAVED_DATASETS_INFO: &str = "Landroid/service/autofill/SavedDatasetsInfo;";

fn af_id(engine: &mut Engine, id: i32) -> ObjRef {
    let value = instance(engine, AF_ID);
    call(engine, value, "<init>", "(I)V", &[Value::Int(id)]);
    value
}
fn af_pattern(engine: &mut Engine, regex: &str) -> Value {
    let regex = text(engine, regex);
    call_static(
        engine,
        "Ljava/util/regex/Pattern;",
        "compile",
        "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
        &[regex],
    )
}
fn af_text_value(engine: &mut Engine, value: &str) -> Value {
    let value = text(engine, value);
    call_static(
        engine,
        AF_VALUE,
        "forText",
        "(Ljava/lang/CharSequence;)Landroid/view/autofill/AutofillValue;",
        &[value],
    )
}
fn af_remote_views(engine: &mut Engine) -> ObjRef {
    let views = instance(engine, "Landroid/widget/RemoteViews;");
    let package = text(engine, "test.autofill");
    call(
        engine,
        views,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[package, Value::Int(7)],
    );
    views
}
fn af_list_get(engine: &mut Engine, list: Value, index: i32) -> Value {
    call(
        engine,
        object(list),
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(index)],
    )
}

#[test]
fn autofill_ids_and_values_have_aosp_value_semantics() {
    let mut engine = engine_of(&DexBuilder::new());

    let host = af_id(&mut engine, 42);
    let child = instance(&mut engine, AF_ID);
    call(
        &mut engine,
        child,
        "<init>",
        "(Landroid/view/autofill/AutofillId;I)V",
        &[Value::Obj(host), Value::Int(7)],
    );
    assert_eq!(
        call_text(&mut engine, child, "toString", "()Ljava/lang/String;", &[]),
        "42:7"
    );
    assert_eq!(
        call(&mut engine, child, "getViewId", "()I", &[]),
        Value::Int(42)
    );
    assert_eq!(
        call(&mut engine, child, "getVirtualChildIntId", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(&mut engine, child, "isVirtualInt", "()Z", &[]),
        Value::Int(1)
    );

    let child_copy = instance(&mut engine, AF_ID);
    call(
        &mut engine,
        child_copy,
        "<init>",
        "(II)V",
        &[Value::Int(42), Value::Int(7)],
    );
    assert_eq!(
        call(
            &mut engine,
            child,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(child_copy)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, child, "hashCode", "()I", &[]),
        call(&mut engine, child_copy, "hashCode", "()I", &[])
    );

    let session_id = instance(&mut engine, AF_ID);
    call(
        &mut engine,
        session_id,
        "<init>",
        "(Landroid/view/autofill/AutofillId;JI)V",
        &[Value::Obj(host), Value::Long(99), Value::Int(5)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            session_id,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "42:99@5"
    );
    let without_session = object(call_static(
        &mut engine,
        AF_ID,
        "withoutSession",
        "(Landroid/view/autofill/AutofillId;)Landroid/view/autofill/AutofillId;",
        &[Value::Obj(session_id)],
    ));
    assert_eq!(
        call(&mut engine, without_session, "hasSession", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            without_session,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "42:99"
    );

    assert_eq!(
        call_static(
            &mut engine,
            AF_VALUE,
            "forText",
            "(Ljava/lang/CharSequence;)Landroid/view/autofill/AutofillValue;",
            &[Value::Null]
        ),
        Value::Null
    );
    let text_value = object(af_text_value(&mut engine, "ada"));
    assert_eq!(
        call(&mut engine, text_value, "isText", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            text_value,
            "getTextValue",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "ada"
    );
    assert_throws(
        engine.invoke_instance(text_value, "getListValue", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    let same_text = object(af_text_value(&mut engine, "ada"));
    assert_eq!(
        call(
            &mut engine,
            text_value,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_text)]
        ),
        Value::Int(1)
    );

    let toggle = object(call_static(
        &mut engine,
        AF_VALUE,
        "forToggle",
        "(Z)Landroid/view/autofill/AutofillValue;",
        &[Value::Int(1)],
    ));
    assert_eq!(
        call(&mut engine, toggle, "getToggleValue", "()Z", &[]),
        Value::Int(1)
    );
    let list = object(call_static(
        &mut engine,
        AF_VALUE,
        "forList",
        "(I)Landroid/view/autofill/AutofillValue;",
        &[Value::Int(3)],
    ));
    assert_eq!(
        call(&mut engine, list, "getListValue", "()I", &[]),
        Value::Int(3)
    );
    let date = object(call_static(
        &mut engine,
        AF_VALUE,
        "forDate",
        "(J)Landroid/view/autofill/AutofillValue;",
        &[Value::Long(123_456)],
    ));
    assert_eq!(
        call(&mut engine, date, "getDateValue", "()J", &[]),
        Value::Long(123_456)
    );
}

#[test]
fn autofill_dataset_builder_replaces_ids_and_enforces_build_contract() {
    let mut engine = engine_of(&DexBuilder::new());
    let presentation = af_remote_views(&mut engine);

    let empty_builder = instance(&mut engine, AF_DATASET_BUILDER);
    call(
        &mut engine,
        empty_builder,
        "<init>",
        "(Landroid/widget/RemoteViews;)V",
        &[Value::Obj(presentation)],
    );
    assert_throws(
        engine.invoke_instance(
            empty_builder,
            "build",
            "()Landroid/service/autofill/Dataset;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let builder = instance(&mut engine, AF_DATASET_BUILDER);
    call(
        &mut engine,
        builder,
        "<init>",
        "(Landroid/widget/RemoteViews;)V",
        &[Value::Obj(presentation)],
    );
    let dataset_id = text(&mut engine, "cred-1");
    call(
        &mut engine,
        builder,
        "setId",
        "(Ljava/lang/String;)Landroid/service/autofill/Dataset$Builder;",
        &[dataset_id],
    );
    let user_id = af_id(&mut engine, 10);
    let first_value = af_text_value(&mut engine, "old-user");
    call(
        &mut engine,
        builder,
        "setValue",
        "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Dataset$Builder;",
        &[Value::Obj(user_id), first_value],
    );
    let replacement_id = af_id(&mut engine, 10);
    let replacement = af_text_value(&mut engine, "ada");
    call(
        &mut engine,
        builder,
        "setValue",
        "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Dataset$Builder;",
        &[Value::Obj(replacement_id), replacement],
    );
    let password_id = af_id(&mut engine, 11);
    let password = af_text_value(&mut engine, "secret");
    let pattern = af_pattern(&mut engine, "[a-z]+");
    call(
        &mut engine,
        builder,
        "setValue",
        "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;Ljava/util/regex/Pattern;)Landroid/service/autofill/Dataset$Builder;",
        &[Value::Obj(password_id), password, pattern.clone()],
    );
    let dataset = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/service/autofill/Dataset;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/service/autofill/Dataset;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let ids = call(
        &mut engine,
        dataset,
        "getFieldIds",
        "()Ljava/util/ArrayList;",
        &[],
    );
    assert_eq!(list_size(&mut engine, ids), 2);
    let values = call(
        &mut engine,
        dataset,
        "getFieldValues",
        "()Ljava/util/ArrayList;",
        &[],
    );
    let first = object(af_list_get(&mut engine, values.clone(), 0));
    assert_eq!(
        call_text(
            &mut engine,
            first,
            "getTextValue",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "ada"
    );
    assert_eq!(
        call(
            &mut engine,
            dataset,
            "getFieldPresentation",
            "(I)Landroid/widget/RemoteViews;",
            &[Value::Int(0)]
        ),
        Value::Obj(presentation)
    );
    assert_eq!(
        call(
            &mut engine,
            dataset,
            "getFilter",
            "(I)Landroid/service/autofill/Dataset$DatasetFieldFilter;",
            &[Value::Int(0)]
        ),
        Value::Null
    );
    let filter = object(call(
        &mut engine,
        dataset,
        "getFilter",
        "(I)Landroid/service/autofill/Dataset$DatasetFieldFilter;",
        &[Value::Int(1)],
    ));
    assert_eq!(
        call(
            &mut engine,
            filter,
            "getPattern",
            "()Ljava/util/regex/Pattern;",
            &[]
        ),
        pattern
    );
    assert_eq!(
        call_text(&mut engine, dataset, "getId", "()Ljava/lang/String;", &[]),
        "cred-1"
    );

    let empty_presentations = instance(&mut engine, AF_PRESENTATIONS_BUILDER);
    call(&mut engine, empty_presentations, "<init>", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            empty_presentations,
            "build",
            "()Landroid/service/autofill/Presentations;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let presentations_builder = instance(&mut engine, AF_PRESENTATIONS_BUILDER);
    call(&mut engine, presentations_builder, "<init>", "()V", &[]);
    call(
        &mut engine,
        presentations_builder,
        "setMenuPresentation",
        "(Landroid/widget/RemoteViews;)Landroid/service/autofill/Presentations$Builder;",
        &[Value::Obj(presentation)],
    );
    let presentations = call(
        &mut engine,
        presentations_builder,
        "build",
        "()Landroid/service/autofill/Presentations;",
        &[],
    );

    let field_builder = instance(&mut engine, AF_FIELD_BUILDER);
    call(&mut engine, field_builder, "<init>", "()V", &[]);
    let field_value = af_text_value(&mut engine, "grace");
    call(
        &mut engine,
        field_builder,
        "setValue",
        "(Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Field$Builder;",
        &[field_value],
    );
    call(
        &mut engine,
        field_builder,
        "setFilter",
        "(Ljava/util/regex/Pattern;)Landroid/service/autofill/Field$Builder;",
        &[Value::Null],
    );
    call(
        &mut engine,
        field_builder,
        "setPresentations",
        "(Landroid/service/autofill/Presentations;)Landroid/service/autofill/Field$Builder;",
        &[presentations],
    );
    let field = call(
        &mut engine,
        field_builder,
        "build",
        "()Landroid/service/autofill/Field;",
        &[],
    );
    assert_throws(
        engine.invoke_instance(
            field_builder,
            "build",
            "()Landroid/service/autofill/Field;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let typed_builder = instance(&mut engine, AF_DATASET_BUILDER);
    call(&mut engine, typed_builder, "<init>", "()V", &[]);
    let typed_id = af_id(&mut engine, 12);
    call(
        &mut engine,
        typed_builder,
        "setField",
        "(Landroid/view/autofill/AutofillId;Landroid/service/autofill/Field;)Landroid/service/autofill/Dataset$Builder;",
        &[Value::Obj(typed_id), field.clone()],
    );
    let hint = text(&mut engine, "username");
    call(
        &mut engine,
        typed_builder,
        "setField",
        "(Ljava/lang/String;Landroid/service/autofill/Field;)Landroid/service/autofill/Dataset$Builder;",
        &[hint, field],
    );
    let typed_dataset = object(call(
        &mut engine,
        typed_builder,
        "build",
        "()Landroid/service/autofill/Dataset;",
        &[],
    ));
    let datatypes = call(
        &mut engine,
        typed_dataset,
        "getAutofillDatatypes",
        "()Ljava/util/ArrayList;",
        &[],
    );
    assert_eq!(list_size(&mut engine, datatypes.clone()), 1);
    let datatype = af_list_get(&mut engine, datatypes, 0);
    assert_eq!(text_of(&engine, &datatype), "username");
    assert_eq!(
        call(
            &mut engine,
            typed_dataset,
            "getFieldPresentation",
            "(I)Landroid/widget/RemoteViews;",
            &[Value::Int(0)]
        ),
        Value::Obj(presentation)
    );
}

#[test]
fn autofill_response_saveinfo_and_user_data_builders_validate_state() {
    let mut engine = engine_of(&DexBuilder::new());
    let required_id = af_id(&mut engine, 20);
    let optional_id = af_id(&mut engine, 21);

    let invalid_user = instance(&mut engine, AF_USER_DATA_BUILDER);
    let invalid_args = [
        text(&mut engine, "data"),
        text(&mut engine, "ab"),
        text(&mut engine, "email"),
    ];
    assert_throws(
        engine.invoke_instance(
            invalid_user,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &invalid_args,
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let user_builder = instance(&mut engine, AF_USER_DATA_BUILDER);
    let user_id = text(&mut engine, "data");
    let user_value = text(&mut engine, "alice@example.test");
    let email_category = text(&mut engine, "email");
    call(
        &mut engine,
        user_builder,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[user_id, user_value.clone(), email_category.clone()],
    );
    call(
        &mut engine,
        user_builder,
        "add",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/service/autofill/UserData$Builder;",
        &[user_value, email_category],
    );
    let phone_value = text(&mut engine, "555-0100");
    let phone_category = text(&mut engine, "phone");
    call(
        &mut engine,
        user_builder,
        "add",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/service/autofill/UserData$Builder;",
        &[phone_value, phone_category],
    );
    let algorithm = text(&mut engine, "Levenshtein");
    let category = text(&mut engine, "email");
    call(
        &mut engine,
        user_builder,
        "setFieldClassificationAlgorithmForCategory",
        "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)Landroid/service/autofill/UserData$Builder;",
        &[category, algorithm, Value::Null],
    );
    let user_data = object(call(
        &mut engine,
        user_builder,
        "build",
        "()Landroid/service/autofill/UserData;",
        &[],
    ));
    let categories = call(
        &mut engine,
        user_data,
        "getCategoryIds",
        "()[Ljava/lang/String;",
        &[],
    );
    assert_eq!(
        array_values(&engine, &categories)
            .iter()
            .map(|value| text_of(&engine, value))
            .collect::<Vec<_>>(),
        ["email", "phone"]
    );
    let category = text(&mut engine, "email");
    assert_eq!(
        call_text(
            &mut engine,
            user_data,
            "getFieldClassificationAlgorithmForCategory",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[category]
        ),
        "Levenshtein"
    );
    let post_build_args = [
        text(&mut engine, "another-value"),
        text(&mut engine, "other"),
    ];
    assert_throws(
        engine.invoke_instance(
            user_builder,
            "add",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/service/autofill/UserData$Builder;",
            &post_build_args,
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_eq!(
        call_static(&mut engine, AF_USER_DATA, "getMaxUserDataSize", "()I", &[]),
        Value::Int(50)
    );

    let validator_pattern = af_pattern(&mut engine, ".{4,}");
    let validator = instance(&mut engine, AF_REGEX_VALIDATOR);
    call(
        &mut engine,
        validator,
        "<init>",
        "(Landroid/view/autofill/AutofillId;Ljava/util/regex/Pattern;)V",
        &[Value::Obj(required_id), validator_pattern],
    );
    let sanitizer_pattern = af_pattern(&mut engine, r"^(\d{4})\s(\d{4})$");
    let sanitizer_subst = text(&mut engine, "$1$2");
    let sanitizer = instance(&mut engine, AF_TEXT_SANITIZER);
    call(
        &mut engine,
        sanitizer,
        "<init>",
        "(Ljava/util/regex/Pattern;Ljava/lang/String;)V",
        &[sanitizer_pattern, sanitizer_subst],
    );
    let raw_card = af_text_value(&mut engine, "1234 5678");
    let sanitized = object(call(
        &mut engine,
        sanitizer,
        "sanitize",
        "(Landroid/view/autofill/AutofillValue;)Landroid/view/autofill/AutofillValue;",
        &[raw_card],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            sanitized,
            "getTextValue",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "12345678"
    );

    let save_builder = instance(&mut engine, AF_SAVE_INFO_BUILDER);
    let required_ids = object_array(
        &mut engine,
        "[Landroid/view/autofill/AutofillId;",
        vec![Value::Obj(required_id)],
    );
    call(
        &mut engine,
        save_builder,
        "<init>",
        "(I[Landroid/view/autofill/AutofillId;)V",
        &[Value::Int(0x1 | 0x8), required_ids.clone()],
    );
    let optional_ids = object_array(
        &mut engine,
        "[Landroid/view/autofill/AutofillId;",
        vec![Value::Obj(optional_id)],
    );
    call(
        &mut engine,
        save_builder,
        "setOptionalIds",
        "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
        &[optional_ids],
    );
    assert_throws(
        engine.invoke_instance(
            save_builder,
            "setFlags",
            "(I)Landroid/service/autofill/SaveInfo$Builder;",
            &[Value::Int(8)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        save_builder,
        "setFlags",
        "(I)Landroid/service/autofill/SaveInfo$Builder;",
        &[Value::Int(1)],
    );
    let description = text(&mut engine, "Save login?");
    call(
        &mut engine,
        save_builder,
        "setDescription",
        "(Ljava/lang/CharSequence;)Landroid/service/autofill/SaveInfo$Builder;",
        &[description],
    );
    let custom_description = instance(&mut engine, "Landroid/service/autofill/CustomDescription;");
    assert_throws(
        engine.invoke_instance(
            save_builder,
            "setCustomDescription",
            "(Landroid/service/autofill/CustomDescription;)Landroid/service/autofill/SaveInfo$Builder;",
            &[Value::Obj(custom_description)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            save_builder,
            "setNegativeAction",
            "(ILandroid/content/IntentSender;)Landroid/service/autofill/SaveInfo$Builder;",
            &[Value::Int(3), Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        save_builder,
        "setNegativeAction",
        "(ILandroid/content/IntentSender;)Landroid/service/autofill/SaveInfo$Builder;",
        &[Value::Int(2), Value::Null],
    );
    call(
        &mut engine,
        save_builder,
        "setValidator",
        "(Landroid/service/autofill/Validator;)Landroid/service/autofill/SaveInfo$Builder;",
        &[Value::Obj(validator)],
    );
    call(
        &mut engine,
        save_builder,
        "addSanitizer",
        "(Landroid/service/autofill/Sanitizer;[Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
        &[Value::Obj(sanitizer), required_ids.clone()],
    );
    assert_throws(
        engine.invoke_instance(
            save_builder,
            "addSanitizer",
            "(Landroid/service/autofill/Sanitizer;[Landroid/view/autofill/AutofillId;)Landroid/service/autofill/SaveInfo$Builder;",
            &[Value::Obj(sanitizer), required_ids.clone()],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let save_info = object(call(
        &mut engine,
        save_builder,
        "build",
        "()Landroid/service/autofill/SaveInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, save_info, "getType", "()I", &[]),
        Value::Int(9)
    );
    assert_eq!(
        call(&mut engine, save_info, "getFlags", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, save_info, "getNegativeActionStyle", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(
            &mut engine,
            save_info,
            "getValidator",
            "()Landroid/service/autofill/InternalValidator;",
            &[]
        ),
        Value::Obj(validator)
    );
    let sanitizer_keys = call(
        &mut engine,
        save_info,
        "getSanitizerKeys",
        "()[Landroid/service/autofill/InternalSanitizer;",
        &[],
    );
    assert_eq!(
        array_values(&engine, &sanitizer_keys),
        [Value::Obj(sanitizer)]
    );

    let presentation = af_remote_views(&mut engine);
    let dataset_builder = instance(&mut engine, AF_DATASET_BUILDER);
    call(
        &mut engine,
        dataset_builder,
        "<init>",
        "(Landroid/widget/RemoteViews;)V",
        &[Value::Obj(presentation)],
    );
    let value = af_text_value(&mut engine, "alice@example.test");
    call(
        &mut engine,
        dataset_builder,
        "setValue",
        "(Landroid/view/autofill/AutofillId;Landroid/view/autofill/AutofillValue;)Landroid/service/autofill/Dataset$Builder;",
        &[Value::Obj(required_id), value],
    );
    let dataset = call(
        &mut engine,
        dataset_builder,
        "build",
        "()Landroid/service/autofill/Dataset;",
        &[],
    );

    let empty_response_builder = instance(&mut engine, AF_FILL_RESPONSE_BUILDER);
    call(&mut engine, empty_response_builder, "<init>", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            empty_response_builder,
            "build",
            "()Landroid/service/autofill/FillResponse;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let response_builder = instance(&mut engine, AF_FILL_RESPONSE_BUILDER);
    call(&mut engine, response_builder, "<init>", "()V", &[]);
    call(
        &mut engine,
        response_builder,
        "addDataset",
        "(Landroid/service/autofill/Dataset;)Landroid/service/autofill/FillResponse$Builder;",
        &[Value::Null],
    );
    call(
        &mut engine,
        response_builder,
        "addDataset",
        "(Landroid/service/autofill/Dataset;)Landroid/service/autofill/FillResponse$Builder;",
        std::slice::from_ref(&dataset),
    );
    call(
        &mut engine,
        response_builder,
        "setSaveInfo",
        "(Landroid/service/autofill/SaveInfo;)Landroid/service/autofill/FillResponse$Builder;",
        &[Value::Obj(save_info)],
    );
    call(
        &mut engine,
        response_builder,
        "setUserData",
        "(Landroid/service/autofill/UserData;)Landroid/service/autofill/FillResponse$Builder;",
        &[Value::Obj(user_data)],
    );
    call(
        &mut engine,
        response_builder,
        "setFlags",
        "(I)Landroid/service/autofill/FillResponse$Builder;",
        &[Value::Int(2)],
    );
    let classification_ids = object_array(
        &mut engine,
        "[Landroid/view/autofill/AutofillId;",
        vec![Value::Obj(required_id)],
    );
    call(
        &mut engine,
        response_builder,
        "setFieldClassificationIds",
        "([Landroid/view/autofill/AutofillId;)Landroid/service/autofill/FillResponse$Builder;",
        &[classification_ids],
    );
    let response = object(call(
        &mut engine,
        response_builder,
        "build",
        "()Landroid/service/autofill/FillResponse;",
        &[],
    ));
    let datasets = call(
        &mut engine,
        response,
        "getDatasets",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(list_size(&mut engine, datasets), 1);
    assert_eq!(
        call(
            &mut engine,
            response,
            "getSaveInfo",
            "()Landroid/service/autofill/SaveInfo;",
            &[]
        ),
        Value::Obj(save_info)
    );
    assert_eq!(
        call(&mut engine, response, "getFlags", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, response, "getRequestId", "()I", &[]),
        Value::Int(i32::MIN)
    );
    assert_eq!(
        call(
            &mut engine,
            response,
            "supportsInlineSuggestions",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );

    let callback = instance(&mut engine, AF_FILL_CALLBACK);
    call(
        &mut engine,
        callback,
        "<init>",
        "(Landroid/service/autofill/IFillCallback;I)V",
        &[Value::Null, Value::Int(77)],
    );
    call(
        &mut engine,
        callback,
        "onSuccess",
        "(Landroid/service/autofill/FillResponse;)V",
        &[Value::Obj(response)],
    );
    assert_eq!(
        call(&mut engine, response, "getRequestId", "()I", &[]),
        Value::Int(77)
    );
    assert_throws(
        engine.invoke_instance(
            callback,
            "onFailure",
            "(Ljava/lang/CharSequence;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );
}

#[test]
fn autofill_requests_callbacks_classifications_and_service_base_are_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let focused = af_id(&mut engine, 30);
    let structure = instance(&mut engine, "Landroid/app/assist/AssistStructure;");
    let context = instance(&mut engine, AF_FILL_CONTEXT);
    call(
        &mut engine,
        context,
        "<init>",
        "(ILandroid/app/assist/AssistStructure;Landroid/view/autofill/AutofillId;)V",
        &[Value::Int(9), Value::Obj(structure), Value::Obj(focused)],
    );
    assert_eq!(
        call(&mut engine, context, "getRequestId", "()I", &[]),
        Value::Int(9)
    );
    assert_eq!(
        call(
            &mut engine,
            context,
            "getFocusedId",
            "()Landroid/view/autofill/AutofillId;",
            &[]
        ),
        Value::Obj(focused)
    );

    let contexts = Value::Obj(list_of(&mut engine, &[Value::Obj(context)]));
    let username_hint = text(&mut engine, "username");
    let hints = Value::Obj(list_of(&mut engine, std::slice::from_ref(&username_hint)));
    let invalid_request = instance(&mut engine, AF_FILL_REQUEST);
    assert_throws(
        engine.invoke_instance(
            invalid_request,
            "<init>",
            "(ILjava/util/List;Ljava/util/List;Landroid/os/Bundle;ILandroid/view/inputmethod/InlineSuggestionsRequest;Landroid/content/IntentSender;)V",
            &[
                Value::Int(9),
                contexts.clone(),
                hints.clone(),
                Value::Null,
                Value::Int(8),
                Value::Null,
                Value::Null,
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let request = instance(&mut engine, AF_FILL_REQUEST);
    call(
        &mut engine,
        request,
        "<init>",
        "(ILjava/util/List;Ljava/util/List;Landroid/os/Bundle;ILandroid/view/inputmethod/InlineSuggestionsRequest;Landroid/content/IntentSender;)V",
        &[
            Value::Int(9),
            contexts.clone(),
            hints.clone(),
            Value::Null,
            Value::Int(1),
            Value::Null,
            Value::Null,
        ],
    );
    assert_eq!(
        call(&mut engine, request, "getId", "()I", &[]),
        Value::Int(9)
    );
    assert_eq!(
        call(&mut engine, request, "getHints", "()Ljava/util/List;", &[]),
        hints
    );

    let context_list = match contexts {
        Value::Obj(list) => Value::Obj(list),
        other => panic!("context list: {other:?}"),
    };
    let dataset_id = text(&mut engine, "cred-1");
    let dataset_ids = Value::Obj(list_of(&mut engine, std::slice::from_ref(&dataset_id)));
    let save_request = instance(&mut engine, AF_SAVE_REQUEST);
    call(
        &mut engine,
        save_request,
        "<init>",
        "(Ljava/util/ArrayList;Landroid/os/Bundle;Ljava/util/ArrayList;)V",
        &[context_list, Value::Null, dataset_ids.clone()],
    );
    assert_eq!(
        call(
            &mut engine,
            save_request,
            "getDatasetIds",
            "()Ljava/util/List;",
            &[]
        ),
        dataset_ids
    );

    let save_callback = instance(&mut engine, AF_SAVE_CALLBACK);
    call(
        &mut engine,
        save_callback,
        "<init>",
        "(Landroid/service/autofill/ISaveCallback;)V",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            save_callback,
            "onSuccess",
            "(Landroid/content/IntentSender;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(&mut engine, save_callback, "onSuccess", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            save_callback,
            "onFailure",
            "(Ljava/lang/CharSequence;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let low_category = text(&mut engine, "low");
    let low = instance(&mut engine, AF_FIELD_CLASSIFICATION_MATCH);
    call(
        &mut engine,
        low,
        "<init>",
        "(Ljava/lang/String;F)V",
        &[low_category, Value::Float(0.25)],
    );
    let high_category = text(&mut engine, "high");
    let high = instance(&mut engine, AF_FIELD_CLASSIFICATION_MATCH);
    call(
        &mut engine,
        high,
        "<init>",
        "(Ljava/lang/String;F)V",
        &[high_category, Value::Float(0.75)],
    );
    let matches = Value::Obj(list_of(&mut engine, &[Value::Obj(low), Value::Obj(high)]));
    let classification = instance(&mut engine, AF_FIELD_CLASSIFICATION);
    call(
        &mut engine,
        classification,
        "<init>",
        "(Ljava/util/ArrayList;)V",
        std::slice::from_ref(&matches),
    );
    let sorted = call(
        &mut engine,
        classification,
        "getMatches",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(af_list_get(&mut engine, sorted, 0), Value::Obj(high));

    let passwords_type = text(&mut engine, "passwords");
    let saved = instance(&mut engine, AF_SAVED_DATASETS_INFO);
    call(
        &mut engine,
        saved,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[passwords_type, Value::Int(4)],
    );
    assert_eq!(
        call_text(&mut engine, saved, "toString", "()Ljava/lang/String;", &[]),
        "SavedDatasetsInfo { type = passwords, count = 4 }"
    );
    let invalid_saved_type = text(&mut engine, "cards");
    let invalid_saved = instance(&mut engine, AF_SAVED_DATASETS_INFO);
    assert_throws(
        engine.invoke_instance(
            invalid_saved,
            "<init>",
            "(Ljava/lang/String;I)V",
            &[invalid_saved_type, Value::Int(1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let service = instance(&mut engine, "Landroid/service/autofill/AutofillService;");
    call(&mut engine, service, "<init>", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            service,
            "getFillEventHistory",
            "()Landroid/service/autofill/FillEventHistory;",
            &[]
        ),
        Value::Null
    );
    let wrong_action = text(&mut engine, "example.WRONG");
    let wrong_intent = instance(&mut engine, "Landroid/content/Intent;");
    call(
        &mut engine,
        wrong_intent,
        "<init>",
        "(Ljava/lang/String;)V",
        &[wrong_action],
    );
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong_intent)]
        ),
        Value::Null
    );
    let service_action = text(&mut engine, "android.service.autofill.AutofillService");
    let intent = instance(&mut engine, "Landroid/content/Intent;");
    call(
        &mut engine,
        intent,
        "<init>",
        "(Ljava/lang/String;)V",
        &[service_action],
    );
    let binder = call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(intent)],
    );
    assert!(!matches!(binder, Value::Null));
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(intent)]
        ),
        binder
    );
}

const TRANSITION: &str = "Landroid/transition/Transition;";
const TRANSITION_SET: &str = "Landroid/transition/TransitionSet;";
const TRANSITION_AUTO: &str = "Landroid/transition/AutoTransition;";
const TRANSITION_VISIBILITY: &str = "Landroid/transition/Visibility;";
const TRANSITION_FADE: &str = "Landroid/transition/Fade;";
const TRANSITION_CHANGE_BOUNDS: &str = "Landroid/transition/ChangeBounds;";
const TRANSITION_VALUES: &str = "Landroid/transition/TransitionValues;";
const TRANSITION_SCENE: &str = "Landroid/transition/Scene;";
const TRANSITION_MANAGER: &str = "Landroid/transition/TransitionManager;";

fn transition_new(engine: &mut Engine, class: &str, sig: &str, args: &[Value]) -> ObjRef {
    let transition = instance(engine, class);
    call(engine, transition, "<init>", sig, args);
    transition
}
fn transition_host_view(engine: &mut Engine, class: &str) -> ObjRef {
    let context = instance(engine, CONTEXT);
    let view = instance(engine, class);
    engine.attach_host_state(class, view).unwrap();
    call(
        engine,
        view,
        "<init>",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    view
}
fn transition_layout(engine: &mut Engine, view: ObjRef, bounds: [i32; 4]) {
    call(
        engine,
        view,
        "layout",
        "(IIII)V",
        &[
            Value::Int(bounds[0]),
            Value::Int(bounds[1]),
            Value::Int(bounds[2]),
            Value::Int(bounds[3]),
        ],
    );
}
fn transition_linear(engine: &mut Engine, transition: ObjRef) {
    let linear = instance(engine, "Landroid/view/animation/LinearInterpolator;");
    call(engine, linear, "<init>", "()V", &[]);
    call(
        engine,
        transition,
        "setInterpolator",
        "(Landroid/animation/TimeInterpolator;)Landroid/transition/Transition;",
        &[Value::Obj(linear)],
    );
}

#[test]
fn transition_configuration_sets_and_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let fade = transition_new(&mut engine, TRANSITION_FADE, "()V", &[]);
    assert_eq!(
        call(&mut engine, fade, "getDuration", "()J", &[]),
        Value::Long(-1)
    );
    assert_eq!(
        call(&mut engine, fade, "getStartDelay", "()J", &[]),
        Value::Long(-1)
    );
    assert_eq!(
        call(&mut engine, fade, "getMode", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            fade,
            "setDuration",
            "(J)Landroid/transition/Transition;",
            &[Value::Long(250)]
        ),
        Value::Obj(fade)
    );
    assert_eq!(
        call(
            &mut engine,
            fade,
            "setMode",
            "(I)Landroid/transition/Visibility;",
            &[Value::Int(1)]
        ),
        Value::Obj(fade)
    );
    assert_throws(
        engine.invoke_instance(
            fade,
            "setMode",
            "(I)Landroid/transition/Visibility;",
            &[Value::Int(4)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let invalid_fade = instance(&mut engine, TRANSITION_FADE);
    assert_throws(
        engine.invoke_instance(invalid_fade, "<init>", "(I)V", &[Value::Int(4)]),
        "Ljava/lang/IllegalArgumentException;",
    );

    call(
        &mut engine,
        fade,
        "addTarget",
        "(I)Landroid/transition/Transition;",
        &[Value::Int(0)],
    );
    call(
        &mut engine,
        fade,
        "addTarget",
        "(I)Landroid/transition/Transition;",
        &[Value::Int(77)],
    );
    let target_ids = call(&mut engine, fade, "getTargetIds", "()Ljava/util/List;", &[]);
    assert_eq!(list_size(&mut engine, target_ids), 1);
    let target_name = text(&mut engine, "hero");
    call(
        &mut engine,
        fade,
        "addTarget",
        "(Ljava/lang/String;)Landroid/transition/Transition;",
        &[target_name],
    );
    let target_names = call(
        &mut engine,
        fade,
        "getTargetNames",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(list_size(&mut engine, target_names), 1);

    let duplicate_order = int_array(&mut engine, &[1, 1]);
    assert_throws(
        engine.invoke_instance(fade, "setMatchOrder", "([I)V", &[duplicate_order]),
        "Ljava/lang/IllegalArgumentException;",
    );
    let invalid_order = int_array(&mut engine, &[9]);
    assert_throws(
        engine.invoke_instance(fade, "setMatchOrder", "([I)V", &[invalid_order]),
        "Ljava/lang/IllegalArgumentException;",
    );
    let property_array = call(
        &mut engine,
        fade,
        "getTransitionProperties",
        "()[Ljava/lang/String;",
        &[],
    );
    let properties = array_values(&engine, &property_array);
    assert_eq!(
        properties
            .iter()
            .map(|value| text_of(&engine, value))
            .collect::<Vec<_>>(),
        ["android:visibility:visibility", "android:visibility:parent"]
    );

    let set = transition_new(&mut engine, TRANSITION_SET, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            set,
            "setOrdering",
            "(I)Landroid/transition/TransitionSet;",
            &[Value::Int(9)],
        ),
        "Landroid/util/AndroidRuntimeException;",
    );
    call(
        &mut engine,
        set,
        "setOrdering",
        "(I)Landroid/transition/TransitionSet;",
        &[Value::Int(1)],
    );
    let child_fade = transition_new(&mut engine, TRANSITION_FADE, "()V", &[]);
    let change_bounds = transition_new(&mut engine, TRANSITION_CHANGE_BOUNDS, "()V", &[]);
    for child in [child_fade, change_bounds] {
        call(
            &mut engine,
            set,
            "addTransition",
            "(Landroid/transition/Transition;)Landroid/transition/TransitionSet;",
            &[Value::Obj(child)],
        );
    }
    call(
        &mut engine,
        set,
        "setDuration",
        "(J)Landroid/transition/TransitionSet;",
        &[Value::Long(123)],
    );
    assert_eq!(
        call(&mut engine, child_fade, "getDuration", "()J", &[]),
        Value::Long(123),
        "TransitionSet duration propagates to already-added children"
    );
    assert_eq!(
        call(&mut engine, set, "getTransitionCount", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(
            &mut engine,
            set,
            "getTransitionAt",
            "(I)Landroid/transition/Transition;",
            &[Value::Int(1)]
        ),
        Value::Obj(change_bounds)
    );
    call(
        &mut engine,
        set,
        "removeTransition",
        "(Landroid/transition/Transition;)Landroid/transition/TransitionSet;",
        &[Value::Obj(change_bounds)],
    );
    assert_eq!(
        call(&mut engine, set, "getTransitionCount", "()I", &[]),
        Value::Int(1)
    );

    let auto = transition_new(&mut engine, TRANSITION_AUTO, "()V", &[]);
    assert_eq!(
        call(&mut engine, auto, "getOrdering", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, auto, "getTransitionCount", "()I", &[]),
        Value::Int(3)
    );
    let first = object(call(
        &mut engine,
        auto,
        "getTransitionAt",
        "(I)Landroid/transition/Transition;",
        &[Value::Int(0)],
    ));
    let second = object(call(
        &mut engine,
        auto,
        "getTransitionAt",
        "(I)Landroid/transition/Transition;",
        &[Value::Int(1)],
    ));
    let third = object(call(
        &mut engine,
        auto,
        "getTransitionAt",
        "(I)Landroid/transition/Transition;",
        &[Value::Int(2)],
    ));
    assert_eq!(engine.heap.class_of(first).unwrap(), TRANSITION_FADE);
    assert_eq!(
        engine.heap.class_of(second).unwrap(),
        TRANSITION_CHANGE_BOUNDS
    );
    assert_eq!(engine.heap.class_of(third).unwrap(), TRANSITION_FADE);
    assert_eq!(
        call(&mut engine, first, "getMode", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, third, "getMode", "()I", &[]),
        Value::Int(1)
    );

    let clone = object(call(
        &mut engine,
        fade,
        "clone",
        "()Landroid/transition/Transition;",
        &[],
    ));
    assert_ne!(clone, fade);
    assert_eq!(
        call(&mut engine, clone, "getDuration", "()J", &[]),
        Value::Long(250)
    );
    assert_eq!(
        call(&mut engine, clone, "getMode", "()I", &[]),
        Value::Int(1)
    );
}

#[test]
fn transition_values_scenes_and_manager_mappings_are_modeled() {
    let mut engine = engine_of(&DexBuilder::new());
    let root = transition_host_view(&mut engine, "Landroid/widget/FrameLayout;");
    let child = transition_host_view(&mut engine, "Landroid/view/View;");
    call(
        &mut engine,
        root,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );

    let first_values = transition_new(
        &mut engine,
        TRANSITION_VALUES,
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    let equal_values = transition_new(
        &mut engine,
        TRANSITION_VALUES,
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    assert_eq!(
        call(
            &mut engine,
            first_values,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(equal_values)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, first_values, "hashCode", "()I", &[]),
        call(&mut engine, equal_values, "hashCode", "()I", &[])
    );
    assert!(call_text(
        &mut engine,
        first_values,
        "toString",
        "()Ljava/lang/String;",
        &[]
    )
    .contains("TransitionValues@"));

    let scene = transition_new(
        &mut engine,
        TRANSITION_SCENE,
        "(Landroid/view/ViewGroup;)V",
        &[Value::Obj(root)],
    );
    assert_eq!(
        call(
            &mut engine,
            scene,
            "getSceneRoot",
            "()Landroid/view/ViewGroup;",
            &[]
        ),
        Value::Obj(root)
    );
    call(&mut engine, scene, "enter", "()V", &[]);
    assert_eq!(
        call_static(
            &mut engine,
            TRANSITION_SCENE,
            "getCurrentScene",
            "(Landroid/view/ViewGroup;)Landroid/transition/Scene;",
            &[Value::Obj(root)],
        ),
        Value::Obj(scene)
    );

    let default_transition = object(call_static(
        &mut engine,
        TRANSITION_MANAGER,
        "getDefaultTransition",
        "()Landroid/transition/Transition;",
        &[],
    ));
    assert_eq!(
        engine.heap.class_of(default_transition).unwrap(),
        TRANSITION_AUTO
    );
    let manager = transition_new(&mut engine, TRANSITION_MANAGER, "()V", &[]);
    let fade = transition_new(&mut engine, TRANSITION_FADE, "()V", &[]);
    call(
        &mut engine,
        manager,
        "setTransition",
        "(Landroid/transition/Scene;Landroid/transition/Transition;)V",
        &[Value::Obj(scene), Value::Obj(fade)],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getTransition",
            "(Landroid/transition/Scene;)Landroid/transition/Transition;",
            &[Value::Obj(scene)]
        ),
        Value::Obj(fade)
    );

    assert!(engine.cp.is_subtype(TRANSITION_FADE, TRANSITION_VISIBILITY));
    assert!(engine.cp.is_subtype(TRANSITION_VISIBILITY, TRANSITION));
    assert!(engine.cp.is_subtype(TRANSITION_AUTO, TRANSITION_SET));
    assert!(engine.cp.is_subtype(TRANSITION_SET, TRANSITION));
    assert!(engine.cp.is_subtype(
        "Landroid/transition/TransitionListenerAdapter;",
        "Landroid/transition/Transition$TransitionListener;"
    ));
}

#[test]
fn delayed_fade_transition_animates_transition_alpha_on_virtual_frames() {
    let mut engine = engine_of(&DexBuilder::new());
    let root = transition_host_view(&mut engine, "Landroid/widget/FrameLayout;");
    let child = transition_host_view(&mut engine, "Landroid/view/View;");
    call(
        &mut engine,
        root,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    transition_layout(&mut engine, root, [0, 0, 200, 200]);
    transition_layout(&mut engine, child, [0, 0, 50, 50]);
    call(
        &mut engine,
        child,
        "setVisibility",
        "(I)V",
        &[Value::Int(8)],
    );

    let fade = transition_new(&mut engine, TRANSITION_FADE, "()V", &[]);
    call(
        &mut engine,
        fade,
        "setDuration",
        "(J)Landroid/transition/Transition;",
        &[Value::Long(160)],
    );
    transition_linear(&mut engine, fade);
    let listener = transition_new(
        &mut engine,
        "Landroid/transition/TransitionListenerAdapter;",
        "()V",
        &[],
    );
    call(
        &mut engine,
        fade,
        "addListener",
        "(Landroid/transition/Transition$TransitionListener;)Landroid/transition/Transition;",
        &[Value::Obj(listener)],
    );
    call_static(
        &mut engine,
        TRANSITION_MANAGER,
        "beginDelayedTransition",
        "(Landroid/view/ViewGroup;Landroid/transition/Transition;)V",
        &[Value::Obj(root), Value::Obj(fade)],
    );
    call(
        &mut engine,
        child,
        "setVisibility",
        "(I)V",
        &[Value::Int(0)],
    );

    engine.advance_clock(16).unwrap();
    assert_eq!(
        call(&mut engine, child, "getTransitionAlpha", "()F", &[]),
        Value::Float(0.0)
    );
    engine.advance_clock(80).unwrap();
    let halfway = call(&mut engine, child, "getTransitionAlpha", "()F", &[]);
    assert!(
        matches!(halfway, Value::Float(value) if (value - 0.5).abs() < 0.001),
        "expected halfway transition alpha, got {halfway:?}"
    );
    engine.advance_clock(80).unwrap();
    assert_eq!(
        call(&mut engine, child, "getTransitionAlpha", "()F", &[]),
        Value::Float(1.0),
        "Fade restores transition alpha at the end"
    );
    assert_eq!(
        field(&engine, listener, "rd_on_transition_start_count"),
        Value::Int(1)
    );
    assert_eq!(
        field(&engine, listener, "rd_on_transition_end_count"),
        Value::Int(1)
    );
}

#[test]
fn change_bounds_interpolates_captured_view_bounds_linearly() {
    let mut engine = engine_of(&DexBuilder::new());
    let root = transition_host_view(&mut engine, "Landroid/widget/FrameLayout;");
    let child = transition_host_view(&mut engine, "Landroid/view/View;");
    call(
        &mut engine,
        root,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    transition_layout(&mut engine, root, [0, 0, 200, 200]);
    transition_layout(&mut engine, child, [0, 10, 40, 60]);

    let change_bounds = transition_new(&mut engine, TRANSITION_CHANGE_BOUNDS, "()V", &[]);
    let start = transition_new(
        &mut engine,
        TRANSITION_VALUES,
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    call(
        &mut engine,
        change_bounds,
        "captureStartValues",
        "(Landroid/transition/TransitionValues;)V",
        &[Value::Obj(start)],
    );
    transition_layout(&mut engine, child, [80, 30, 160, 110]);
    let end = transition_new(
        &mut engine,
        TRANSITION_VALUES,
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    );
    call(
        &mut engine,
        change_bounds,
        "captureEndValues",
        "(Landroid/transition/TransitionValues;)V",
        &[Value::Obj(end)],
    );
    let animator = object(call(
        &mut engine,
        change_bounds,
        "createAnimator",
        "(Landroid/view/ViewGroup;Landroid/transition/TransitionValues;Landroid/transition/TransitionValues;)Landroid/animation/Animator;",
        &[Value::Obj(root), Value::Obj(start), Value::Obj(end)],
    ));
    call(
        &mut engine,
        animator,
        "setCurrentFraction",
        "(F)V",
        &[Value::Float(0.5)],
    );
    assert_eq!(
        call(&mut engine, child, "getLeft", "()I", &[]),
        Value::Int(40)
    );
    assert_eq!(
        call(&mut engine, child, "getTop", "()I", &[]),
        Value::Int(20)
    );
    assert_eq!(
        call(&mut engine, child, "getRight", "()I", &[]),
        Value::Int(100)
    );
    assert_eq!(
        call(&mut engine, child, "getBottom", "()I", &[]),
        Value::Int(85)
    );
}

const ANDROID_OS: &str = "Landroid/system/Os;";
const ANDROID_OS_CONSTANTS: &str = "Landroid/system/OsConstants;";
const ANDROID_ERRNO: &str = "Landroid/system/ErrnoException;";
const ANDROID_STRUCT_STAT: &str = "Landroid/system/StructStat;";
const ANDROID_STRUCT_TIMESPEC: &str = "Landroid/system/StructTimespec;";

fn byte_array(engine: &mut Engine, values: &[u8]) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Int, values.len(), "[B")
        .unwrap();
    *engine.heap.array_elems_mut(array).unwrap() = values
        .iter()
        .map(|value| Value::Int(*value as i8 as i32))
        .collect();
    Value::Obj(array)
}
fn byte_array_values(engine: &Engine, value: &Value) -> Vec<u8> {
    array_values(engine, value)
        .iter()
        .map(|value| match value {
            Value::Int(v) => *v as u8,
            other => panic!("expected byte array element, got {other:?}"),
        })
        .collect()
}
fn assert_errno(result: Result<Value, VmExit>, errno: i32, function: &str) -> ObjRef {
    let Err(VmExit::Exception(throwable)) = result else {
        panic!("expected ErrnoException {errno} from {function}, got {result:?}");
    };
    assert_eq!(throwable.class, ANDROID_ERRNO);
    throwable
        .obj
        .expect("ErrnoException must retain its materialized object")
}
fn assert_errno_fields(engine: &Engine, object: ObjRef, errno: i32, function: &str) {
    assert_eq!(field(engine, object, "errno"), Value::Int(errno));
    assert_eq!(
        text_of(engine, &field(engine, object, "functionName")),
        function
    );
}
fn os_text_arg(engine: &mut Engine, value: &str) -> Value {
    text(engine, value)
}
fn os_open(engine: &mut Engine, path: &str, flags: i32, mode: i32) -> ObjRef {
    let path = os_text_arg(engine, path);
    object(call_static(
        engine,
        ANDROID_OS,
        "open",
        "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
        &[path, Value::Int(flags), Value::Int(mode)],
    ))
}
fn os_close(engine: &mut Engine, fd: ObjRef) {
    call_static(
        engine,
        ANDROID_OS,
        "close",
        "(Ljava/io/FileDescriptor;)V",
        &[Value::Obj(fd)],
    );
}
fn os_stat(engine: &mut Engine, path: &str) -> ObjRef {
    let path = os_text_arg(engine, path);
    object(call_static(
        engine,
        ANDROID_OS,
        "stat",
        "(Ljava/lang/String;)Landroid/system/StructStat;",
        &[path],
    ))
}

#[test]
fn android_system_constants_structs_and_errno_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert_eq!(
        call_static_text(
            &mut engine,
            ANDROID_OS_CONSTANTS,
            "errnoName",
            "(I)Ljava/lang/String;",
            &[Value::Int(2)]
        ),
        "ENOENT"
    );
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS_CONSTANTS,
            "S_ISDIR",
            "(I)Z",
            &[Value::Int(0o040755)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS_CONSTANTS,
            "S_ISREG",
            "(I)Z",
            &[Value::Int(0o040755)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            ANDROID_OS,
            "strerror",
            "(I)Ljava/lang/String;",
            &[Value::Int(13)]
        ),
        "Permission denied"
    );

    let function = text(&mut engine, "stat");
    let errno = instance(&mut engine, ANDROID_ERRNO);
    call(
        &mut engine,
        errno,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[function, Value::Int(2)],
    );
    assert_eq!(field(&engine, errno, "errno"), Value::Int(2));
    assert_eq!(
        call_text(
            &mut engine,
            errno,
            "getMessage",
            "()Ljava/lang/String;",
            &[]
        ),
        "stat failed: ENOENT (No such file or directory)"
    );
    let rethrown = engine.invoke_instance(
        errno,
        "rethrowAsIOException",
        "()Ljava/io/IOException;",
        &[],
    );
    let Err(VmExit::Exception(throwable)) = rethrown else {
        panic!("expected IOException, got {rethrown:?}");
    };
    assert_eq!(throwable.class, "Ljava/io/IOException;");
    let io = throwable.obj.expect("IOException object");
    assert_eq!(field(&engine, io, "cause"), Value::Obj(errno));

    let invalid = instance(&mut engine, ANDROID_STRUCT_TIMESPEC);
    assert_throws(
        engine.invoke_instance(
            invalid,
            "<init>",
            "(JJ)V",
            &[Value::Long(1), Value::Long(1_000_000_000)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let first = transition_new(
        &mut engine,
        ANDROID_STRUCT_TIMESPEC,
        "(JJ)V",
        &[Value::Long(10), Value::Long(20)],
    );
    let equal = transition_new(
        &mut engine,
        ANDROID_STRUCT_TIMESPEC,
        "(JJ)V",
        &[Value::Long(10), Value::Long(20)],
    );
    let later = transition_new(
        &mut engine,
        ANDROID_STRUCT_TIMESPEC,
        "(JJ)V",
        &[Value::Long(10), Value::Long(21)],
    );
    assert_eq!(
        call(
            &mut engine,
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(equal)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, first, "hashCode", "()I", &[]),
        call(&mut engine, equal, "hashCode", "()I", &[])
    );
    assert_eq!(
        call(
            &mut engine,
            first,
            "compareTo",
            "(Landroid/system/StructTimespec;)I",
            &[Value::Obj(later)]
        ),
        Value::Int(-1)
    );

    let stat = transition_new(
        &mut engine,
        ANDROID_STRUCT_STAT,
        "(JJIJIIJJJJJJJ)V",
        &[
            Value::Long(7),
            Value::Long(42),
            Value::Int(0o100644),
            Value::Long(1),
            Value::Int(10000),
            Value::Int(10000),
            Value::Long(0),
            Value::Long(123),
            Value::Long(11),
            Value::Long(12),
            Value::Long(13),
            Value::Long(4096),
            Value::Long(8),
        ],
    );
    assert_eq!(field(&engine, stat, "st_size"), Value::Long(123));
    let mtim = object(field(&engine, stat, "st_mtim"));
    assert_eq!(field(&engine, mtim, "tv_sec"), Value::Long(12));

    assert!(engine.cp.is_subtype(ANDROID_ERRNO, "Ljava/lang/Exception;"));
    assert!(engine
        .cp
        .is_subtype(ANDROID_STRUCT_TIMESPEC, "Ljava/lang/Comparable;"));
}

#[test]
fn android_system_os_files_descriptors_and_metadata_use_the_vfs() {
    let mut engine = engine_of(&DexBuilder::new());
    let root = "/rd-system-file";
    let root_arg = os_text_arg(&mut engine, root);
    call_static(
        &mut engine,
        ANDROID_OS,
        "mkdir",
        "(Ljava/lang/String;I)V",
        &[root_arg, Value::Int(0o755)],
    );
    let path = "/rd-system-file/data.bin";

    let missing_arg = os_text_arg(&mut engine, path);
    let missing = assert_errno(
        engine.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[missing_arg, Value::Int(0), Value::Int(0)],
        ),
        2,
        "open",
    );
    assert_errno_fields(&engine, missing, 2, "open");

    // O_WRONLY | O_CREAT | O_TRUNC, mode 0640.
    let writer = os_open(&mut engine, path, 1 | 0o100 | 0o1000, 0o640);
    assert!(matches!(
        field(&engine, writer, "descriptor"),
        Value::Int(fd) if fd >= 3
    ));
    let bytes = byte_array(&mut engine, b"hello system");
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "write",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[Value::Obj(writer), bytes, Value::Int(0), Value::Int(12)],
        ),
        Value::Int(12)
    );
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "lseek",
            "(Ljava/io/FileDescriptor;JI)J",
            &[Value::Obj(writer), Value::Long(0), Value::Int(1)],
        ),
        Value::Long(12)
    );
    os_close(&mut engine, writer);
    assert_eq!(field(&engine, writer, "descriptor"), Value::Int(-1));
    let closed = assert_errno(
        engine.invoke_static(
            ANDROID_OS,
            "close",
            "(Ljava/io/FileDescriptor;)V",
            &[Value::Obj(writer)],
        ),
        9,
        "close",
    );
    assert_errno_fields(&engine, closed, 9, "close");

    let stat = os_stat(&mut engine, path);
    assert_eq!(field(&engine, stat, "st_mode"), Value::Int(0o100640));
    assert_eq!(field(&engine, stat, "st_size"), Value::Long(12));
    assert_eq!(field(&engine, stat, "st_uid"), Value::Int(10000));

    let reader = os_open(&mut engine, path, 0, 0);
    let out = byte_array(&mut engine, &[0; 16]);
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "read",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[
                Value::Obj(reader),
                out.clone(),
                Value::Int(2),
                Value::Int(5)
            ],
        ),
        Value::Int(5)
    );
    assert_eq!(&byte_array_values(&engine, &out)[2..7], b"hello");
    let pread_out = byte_array(&mut engine, &[0; 6]);
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "pread",
            "(Ljava/io/FileDescriptor;[BIIJ)I",
            &[
                Value::Obj(reader),
                pread_out.clone(),
                Value::Int(0),
                Value::Int(6),
                Value::Long(6),
            ],
        ),
        Value::Int(6)
    );
    assert_eq!(byte_array_values(&engine, &pread_out), b"system");
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "lseek",
            "(Ljava/io/FileDescriptor;JI)J",
            &[Value::Obj(reader), Value::Long(0), Value::Int(1)],
        ),
        Value::Long(5),
        "pread does not move the shared descriptor offset"
    );
    let rest = byte_array(&mut engine, &[0; 16]);
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "read",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[
                Value::Obj(reader),
                rest.clone(),
                Value::Int(0),
                Value::Int(16)
            ],
        ),
        Value::Int(7)
    );
    assert_eq!(&byte_array_values(&engine, &rest)[..7], b" system");
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "read",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[Value::Obj(reader), rest, Value::Int(0), Value::Int(1)],
        ),
        Value::Int(-1)
    );
    let bad_bounds = byte_array(&mut engine, &[0; 2]);
    assert_throws(
        engine.invoke_static(
            ANDROID_OS,
            "read",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[Value::Obj(reader), bad_bounds, Value::Int(1), Value::Int(2)],
        ),
        "Ljava/lang/IndexOutOfBoundsException;",
    );
    let read_only_write = byte_array(&mut engine, b"x");
    let bad_write = assert_errno(
        engine.invoke_static(
            ANDROID_OS,
            "write",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[
                Value::Obj(reader),
                read_only_write,
                Value::Int(0),
                Value::Int(1),
            ],
        ),
        9,
        "write",
    );
    assert_errno_fields(&engine, bad_write, 9, "write");
    os_close(&mut engine, reader);

    let rdwr = os_open(&mut engine, path, 2, 0);
    let replacement = byte_array(&mut engine, b"HELLO");
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "pwrite",
            "(Ljava/io/FileDescriptor;[BIIJ)I",
            &[
                Value::Obj(rdwr),
                replacement,
                Value::Int(0),
                Value::Int(5),
                Value::Long(0),
            ],
        ),
        Value::Int(5)
    );
    call_static(
        &mut engine,
        ANDROID_OS,
        "ftruncate",
        "(Ljava/io/FileDescriptor;J)V",
        &[Value::Obj(rdwr), Value::Long(5)],
    );
    os_close(&mut engine, rdwr);

    let appender = os_open(&mut engine, path, 1 | 0o2000, 0);
    call_static(
        &mut engine,
        ANDROID_OS,
        "lseek",
        "(Ljava/io/FileDescriptor;JI)J",
        &[Value::Obj(appender), Value::Long(0), Value::Int(0)],
    );
    let bang = byte_array(&mut engine, b"!");
    call_static(
        &mut engine,
        ANDROID_OS,
        "write",
        "(Ljava/io/FileDescriptor;[BII)I",
        &[Value::Obj(appender), bang, Value::Int(0), Value::Int(1)],
    );
    os_close(&mut engine, appender);
    let stat = os_stat(&mut engine, path);
    assert_eq!(field(&engine, stat, "st_size"), Value::Long(6));

    let original = os_open(&mut engine, path, 0, 0);
    let duplicate = object(call_static(
        &mut engine,
        ANDROID_OS,
        "dup",
        "(Ljava/io/FileDescriptor;)Ljava/io/FileDescriptor;",
        &[Value::Obj(original)],
    ));
    let dup_out = byte_array(&mut engine, &[0; 5]);
    call_static(
        &mut engine,
        ANDROID_OS,
        "read",
        "(Ljava/io/FileDescriptor;[BII)I",
        &[
            Value::Obj(duplicate),
            dup_out.clone(),
            Value::Int(0),
            Value::Int(5),
        ],
    );
    assert_eq!(byte_array_values(&engine, &dup_out), b"HELLO");
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "lseek",
            "(Ljava/io/FileDescriptor;JI)J",
            &[Value::Obj(original), Value::Long(0), Value::Int(1)],
        ),
        Value::Long(5),
        "dup shares the open-file description offset"
    );
    os_close(&mut engine, duplicate);
    os_close(&mut engine, original);

    let path_arg = os_text_arg(&mut engine, path);
    call_static(
        &mut engine,
        ANDROID_OS,
        "chmod",
        "(Ljava/lang/String;I)V",
        &[path_arg, Value::Int(0o600)],
    );
    let stat = os_stat(&mut engine, path);
    assert_eq!(field(&engine, stat, "st_mode"), Value::Int(0o100600));
    for (mode, expected) in [(4, 1), (2, 1), (1, 0)] {
        let path_arg = os_text_arg(&mut engine, path);
        assert_eq!(
            call_static(
                &mut engine,
                ANDROID_OS,
                "access",
                "(Ljava/lang/String;I)Z",
                &[path_arg, Value::Int(mode)],
            ),
            Value::Int(expected)
        );
    }

    let root_arg = os_text_arg(&mut engine, root);
    let dir_write = assert_errno(
        engine.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[root_arg, Value::Int(2), Value::Int(0)],
        ),
        21,
        "open",
    );
    assert_errno_fields(&engine, dir_write, 21, "open");

    let renamed = "/rd-system-file/renamed.bin";
    let old_arg = os_text_arg(&mut engine, path);
    let new_arg = os_text_arg(&mut engine, renamed);
    call_static(
        &mut engine,
        ANDROID_OS,
        "rename",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[old_arg, new_arg],
    );
    let renamed_arg = os_text_arg(&mut engine, renamed);
    call_static(
        &mut engine,
        ANDROID_OS,
        "remove",
        "(Ljava/lang/String;)V",
        &[renamed_arg],
    );
    let removed_arg = os_text_arg(&mut engine, renamed);
    let removed = assert_errno(
        engine.invoke_static(
            ANDROID_OS,
            "stat",
            "(Ljava/lang/String;)Landroid/system/StructStat;",
            &[removed_arg],
        ),
        2,
        "stat",
    );
    assert_errno_fields(&engine, removed, 2, "stat");
}

#[test]
fn android_system_interoperates_with_java_io_environment_and_statvfs() {
    let mut engine = engine_of(&DexBuilder::new());
    let root_arg = os_text_arg(&mut engine, "/rd-system-interop");
    call_static(
        &mut engine,
        ANDROID_OS,
        "mkdir",
        "(Ljava/lang/String;I)V",
        &[root_arg, Value::Int(0o755)],
    );
    let path = "/rd-system-interop/from-java.bin";
    let output = instance(&mut engine, "Ljava/io/FileOutputStream;");
    let path_arg = os_text_arg(&mut engine, path);
    call(
        &mut engine,
        output,
        "<init>",
        "(Ljava/lang/String;)V",
        &[path_arg],
    );
    let payload = byte_array(&mut engine, b"java-io");
    call(
        &mut engine,
        output,
        "write",
        "([B)V",
        std::slice::from_ref(&payload),
    );
    call(&mut engine, output, "close", "()V", &[]);
    let stat = os_stat(&mut engine, path);
    assert_eq!(field(&engine, stat, "st_size"), Value::Long(7));

    let input = instance(&mut engine, "Ljava/io/FileInputStream;");
    let path_arg = os_text_arg(&mut engine, path);
    call(
        &mut engine,
        input,
        "<init>",
        "(Ljava/lang/String;)V",
        &[path_arg],
    );
    let fd = object(call(
        &mut engine,
        input,
        "getFD",
        "()Ljava/io/FileDescriptor;",
        &[],
    ));
    let fstat = object(call_static(
        &mut engine,
        ANDROID_OS,
        "fstat",
        "(Ljava/io/FileDescriptor;)Landroid/system/StructStat;",
        &[Value::Obj(fd)],
    ));
    assert_eq!(field(&engine, fstat, "st_size"), Value::Long(7));
    let out = byte_array(&mut engine, &[0; 7]);
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "read",
            "(Ljava/io/FileDescriptor;[BII)I",
            &[Value::Obj(fd), out.clone(), Value::Int(0), Value::Int(7)],
        ),
        Value::Int(7)
    );
    assert_eq!(byte_array_values(&engine, &out), b"java-io");

    let key = os_text_arg(&mut engine, "RD_SYSTEM_TEST");
    let value = os_text_arg(&mut engine, "vfs");
    call_static(
        &mut engine,
        ANDROID_OS,
        "setenv",
        "(Ljava/lang/String;Ljava/lang/String;Z)V",
        &[key.clone(), value, Value::Int(1)],
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            ANDROID_OS,
            "getenv",
            "(Ljava/lang/String;)Ljava/lang/String;",
            std::slice::from_ref(&key),
        ),
        "vfs"
    );
    let second = os_text_arg(&mut engine, "other");
    call_static(
        &mut engine,
        ANDROID_OS,
        "setenv",
        "(Ljava/lang/String;Ljava/lang/String;Z)V",
        &[key.clone(), second, Value::Int(0)],
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            ANDROID_OS,
            "getenv",
            "(Ljava/lang/String;)Ljava/lang/String;",
            std::slice::from_ref(&key),
        ),
        "vfs",
        "overwrite=false preserves an existing value"
    );
    let environ = call_static(
        &mut engine,
        ANDROID_OS,
        "environ",
        "()[Ljava/lang/String;",
        &[],
    );
    assert!(array_values(&engine, &environ)
        .iter()
        .map(|value| text_of(&engine, value))
        .any(|entry| entry == "RD_SYSTEM_TEST=vfs"));
    call_static(
        &mut engine,
        ANDROID_OS,
        "unsetenv",
        "(Ljava/lang/String;)V",
        std::slice::from_ref(&key),
    );
    assert_eq!(
        call_static(
            &mut engine,
            ANDROID_OS,
            "getenv",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[key],
        ),
        Value::Null
    );

    let path_arg = os_text_arg(&mut engine, path);
    let statvfs = object(call_static(
        &mut engine,
        ANDROID_OS,
        "statvfs",
        "(Ljava/lang/String;)Landroid/system/StructStatVfs;",
        &[path_arg],
    ));
    assert_eq!(field(&engine, statvfs, "f_bsize"), Value::Long(4096));
    assert!(matches!(
        field(&engine, statvfs, "f_blocks"),
        Value::Long(blocks) if blocks > 0
    ));
    let uname = object(call_static(
        &mut engine,
        ANDROID_OS,
        "uname",
        "()Landroid/system/StructUtsname;",
        &[],
    ));
    assert_eq!(text_of(&engine, &field(&engine, uname, "sysname")), "Linux");
    assert!(!text_of(&engine, &field(&engine, uname, "machine")).is_empty());
    assert_eq!(call(&mut engine, fd, "valid", "()Z", &[]), Value::Int(1));
    os_close(&mut engine, fd);
    assert_eq!(call(&mut engine, fd, "valid", "()Z", &[]), Value::Int(0));
}

// ---------------------------------------------------------------------------
// Batch 57-A: eight formerly-not_implemented surfaces promoted to bounded
// partial slices.
// ---------------------------------------------------------------------------

fn p57_system_service(engine: &mut Engine, name: &str) -> ObjRef {
    let context = instance(engine, CONTEXT);
    let service_name = text(engine, name);
    object(call(
        engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ))
}

fn p57_collection_size(engine: &mut Engine, value: Value) -> i32 {
    match call(engine, object(value), "size", "()I", &[]) {
        Value::Int(n) => n,
        other => panic!("expected collection size, got {other:?}"),
    }
}

fn p57_string_array(engine: &mut Engine, values: &[&str]) -> Value {
    let values = values
        .iter()
        .map(|value| text(engine, value))
        .collect::<Vec<_>>();
    object_array(engine, "[Ljava/lang/String;", values)
}

fn p57_string_values(engine: &Engine, value: &Value) -> Vec<String> {
    array_values(engine, value)
        .iter()
        .map(|value| text_of(engine, value))
        .collect()
}

fn p57_date_value(engine: &mut Engine, value: Value) -> i64 {
    match call(engine, object(value), "getTime", "()J", &[]) {
        Value::Long(ms) => ms,
        other => panic!("expected Date.getTime, got {other:?}"),
    }
}

#[test]
fn textservice_values_params_and_no_checker_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "textservices");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/view/textservice/TextServicesManager;"
    );
    assert_eq!(
        call(&mut engine, manager, "isSpellCheckerEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getCurrentSpellCheckerInfo",
            "()Landroid/view/textservice/SpellCheckerInfo;",
            &[],
        ),
        Value::Null
    );
    let enabled = call(
        &mut engine,
        manager,
        "getEnabledSpellCheckerInfos",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, enabled), 0);

    let info = instance(&mut engine, "Landroid/view/textservice/TextInfo;");
    let word = text(&mut engine, "helo");
    call(
        &mut engine,
        info,
        "<init>",
        "(Ljava/lang/String;II)V",
        &[word, Value::Int(7), Value::Int(11)],
    );
    assert_eq!(
        call_text(&mut engine, info, "getText", "()Ljava/lang/String;", &[]),
        "helo"
    );
    assert_eq!(
        call(&mut engine, info, "getCookie", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(&mut engine, info, "getSequence", "()I", &[]),
        Value::Int(11)
    );

    let suggestions = p57_string_array(&mut engine, &["hello", "halo"]);
    let suggestions_info = instance(&mut engine, "Landroid/view/textservice/SuggestionsInfo;");
    call(
        &mut engine,
        suggestions_info,
        "<init>",
        "(I[Ljava/lang/String;II)V",
        &[Value::Int(1), suggestions, Value::Int(7), Value::Int(11)],
    );
    assert_eq!(
        call(
            &mut engine,
            suggestions_info,
            "getSuggestionsCount",
            "()I",
            &[]
        ),
        Value::Int(2)
    );
    assert_eq!(
        call_text(
            &mut engine,
            suggestions_info,
            "getSuggestionAt",
            "(I)Ljava/lang/String;",
            &[Value::Int(0)],
        ),
        "hello"
    );

    let infos = object_array(
        &mut engine,
        "[Landroid/view/textservice/SuggestionsInfo;",
        vec![Value::Obj(suggestions_info)],
    );
    let offsets = int_array(&mut engine, &[3]);
    let lengths = int_array(&mut engine, &[4]);
    let sentence = instance(
        &mut engine,
        "Landroid/view/textservice/SentenceSuggestionsInfo;",
    );
    call(
        &mut engine,
        sentence,
        "<init>",
        "([Landroid/view/textservice/SuggestionsInfo;[I[I)V",
        &[infos, offsets, lengths],
    );
    assert_eq!(
        call(
            &mut engine,
            sentence,
            "getOffsetAt",
            "(I)I",
            &[Value::Int(0)]
        ),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            sentence,
            "getLengthAt",
            "(I)I",
            &[Value::Int(0)]
        ),
        Value::Int(4)
    );
    assert_eq!(
        call(
            &mut engine,
            sentence,
            "getOffsetAt",
            "(I)I",
            &[Value::Int(9)]
        ),
        Value::Int(-1)
    );

    let locale = text(&mut engine, "en_US");
    let extra = text(&mut engine, "SubtypeMode=keyboard,Language=en");
    let subtype = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSubtype;",
    );
    call(
        &mut engine,
        subtype,
        "<init>",
        "(ILjava/lang/String;Ljava/lang/String;)V",
        &[Value::Int(42), locale, extra],
    );
    let key = text(&mut engine, "SubtypeMode");
    assert_eq!(
        call(
            &mut engine,
            subtype,
            "containsExtraValueKey",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&key),
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            subtype,
            "getExtraValueOf",
            "(Ljava/lang/String;)Ljava/lang/String;",
            std::slice::from_ref(&key),
        ),
        "keyboard"
    );

    let builder = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
    );
    call(&mut engine, builder, "<init>", "()V", &[]);
    call(
        &mut engine,
        builder,
        "setShouldReferToSpellCheckerLanguageSettings",
        "(Z)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        builder,
        "setSupportedAttributes",
        "(I)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
        &[Value::Int(3)],
    );
    let params = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, params, "getSupportedAttributes", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            params,
            "shouldReferToSpellCheckerLanguageSettings",
            "()Z",
            &[],
        ),
        Value::Int(1)
    );

    let listener = instance(&mut engine, "Ljava/lang/Object;");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "newSpellCheckerSession",
            "(Landroid/os/Bundle;Ljava/util/Locale;Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;Z)Landroid/view/textservice/SpellCheckerSession;",
            &[Value::Null, Value::Null, Value::Obj(listener), Value::Int(0)],
        ),
        Value::Null,
        "no enabled spell checker means no session is fabricated"
    );

    let checker_info = instance(&mut engine, "Landroid/view/textservice/SpellCheckerInfo;");
    let session = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSession;",
    );
    call(
        &mut engine,
        session,
        "<init>",
        "(Landroid/view/textservice/SpellCheckerInfo;Landroid/view/textservice/TextServicesManager;Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;Ljava/util/concurrent/Executor;)V",
        &[
            Value::Obj(checker_info),
            Value::Obj(manager),
            Value::Obj(listener),
            Value::Null,
        ],
    );
    assert_eq!(
        call(&mut engine, session, "isSessionDisconnected", "()Z", &[]),
        Value::Int(1)
    );

    // Deep audit: subtype extra-value parsing follows String.split
    // semantics, so "a=" stores a null value, "b==x" stores "", a bare
    // "=" pair stores nothing, and an empty extra value still maps the
    // empty key to null.
    let split_locale = text(&mut engine, "en");
    let split_extra = text(&mut engine, "a=,b==x,=,c");
    let split_subtype = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSubtype;",
    );
    call(
        &mut engine,
        split_subtype,
        "<init>",
        "(ILjava/lang/String;Ljava/lang/String;)V",
        &[Value::Int(0), split_locale, split_extra],
    );
    let a_key = text(&mut engine, "a");
    assert_eq!(
        call(
            &mut engine,
            split_subtype,
            "getExtraValueOf",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[a_key],
        ),
        Value::Null
    );
    let b_key = text(&mut engine, "b");
    assert_eq!(
        call_text(
            &mut engine,
            split_subtype,
            "getExtraValueOf",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[b_key],
        ),
        ""
    );
    let empty_key = text(&mut engine, "");
    assert_eq!(
        call(
            &mut engine,
            split_subtype,
            "containsExtraValueKey",
            "(Ljava/lang/String;)Z",
            &[empty_key],
        ),
        Value::Int(0)
    );
    let empty_locale = text(&mut engine, "en");
    let empty_extra = text(&mut engine, "");
    let empty_subtype = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSubtype;",
    );
    call(
        &mut engine,
        empty_subtype,
        "<init>",
        "(ILjava/lang/String;Ljava/lang/String;)V",
        &[Value::Int(0), empty_locale, empty_extra],
    );
    let empty_key = text(&mut engine, "");
    assert_eq!(
        call(
            &mut engine,
            empty_subtype,
            "containsExtraValueKey",
            "(Ljava/lang/String;)Z",
            &[empty_key],
        ),
        Value::Int(1)
    );

    // Deep audit: getLocaleObject parses the legacy underscore locale
    // string (en_US) into language/country instead of treating it as a
    // single language, and maps the legacy "tl" language to "fil".
    let subtype_locale = object(call(
        &mut engine,
        subtype,
        "getLocaleObject",
        "()Ljava/util/Locale;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            subtype_locale,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        "en"
    );
    assert_eq!(
        call_text(
            &mut engine,
            subtype_locale,
            "getCountry",
            "()Ljava/lang/String;",
            &[]
        ),
        "US"
    );
    let tl_locale = text(&mut engine, "tl_PH");
    let tl_extra = text(&mut engine, "");
    let tl_subtype = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSubtype;",
    );
    call(
        &mut engine,
        tl_subtype,
        "<init>",
        "(ILjava/lang/String;Ljava/lang/String;)V",
        &[Value::Int(0), tl_locale, tl_extra],
    );
    let fil_locale = object(call(
        &mut engine,
        tl_subtype,
        "getLocaleObject",
        "()Ljava/util/Locale;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            fil_locale,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        "fil"
    );

    // Deep audit: SpellCheckerSessionParams.Builder.setExtras performs no
    // null check, and the null comes back from getExtras.
    let null_extras_builder = instance(
        &mut engine,
        "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
    );
    call(&mut engine, null_extras_builder, "<init>", "()V", &[]);
    call(
        &mut engine,
        null_extras_builder,
        "setShouldReferToSpellCheckerLanguageSettings",
        "(Z)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        null_extras_builder,
        "setExtras",
        "(Landroid/os/Bundle;)Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;",
        &[Value::Null],
    );
    let null_extras_params = object(call(
        &mut engine,
        null_extras_builder,
        "build",
        "()Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            null_extras_params,
            "getExtras",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Null
    );

    // Deep audit: with no enabled checkers the manager returns
    // Collections.emptyList(), whose mutation throws.
    let enabled_list = object(call(
        &mut engine,
        manager,
        "getEnabledSpellCheckerInfos",
        "()Ljava/util/List;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(
            enabled_list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(checker_info)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
}

#[test]
fn translation_values_context_and_no_service_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let source = instance(&mut engine, "Landroid/view/translation/TranslationSpec;");
    let en = text(&mut engine, "en");
    call(
        &mut engine,
        source,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[en, Value::Int(1)],
    );
    let target = instance(&mut engine, "Landroid/view/translation/TranslationSpec;");
    let pt = text(&mut engine, "pt");
    call(
        &mut engine,
        target,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[pt, Value::Int(1)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            source,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        "en"
    );
    assert!(matches!(
        call(
            &mut engine,
            source,
            "getLocale",
            "()Landroid/icu/util/ULocale;",
            &[],
        ),
        Value::Obj(_)
    ));
    assert_eq!(
        call(
            &mut engine,
            source,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(source)]
        ),
        Value::Int(1)
    );

    let capability = instance(
        &mut engine,
        "Landroid/view/translation/TranslationCapability;",
    );
    call(
        &mut engine,
        capability,
        "<init>",
        "(ILandroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;ZI)V",
        &[
            Value::Int(3),
            Value::Obj(source),
            Value::Obj(target),
            Value::Int(1),
            Value::Int(3),
        ],
    );
    assert_eq!(
        call(&mut engine, capability, "getState", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            capability,
            "isUiTranslationEnabled",
            "()Z",
            &[]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            capability,
            "getSupportedTranslationFlags",
            "()I",
            &[]
        ),
        Value::Int(3)
    );

    let context_builder = instance(
        &mut engine,
        "Landroid/view/translation/TranslationContext$Builder;",
    );
    call(
        &mut engine,
        context_builder,
        "<init>",
        "(Landroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;)V",
        &[Value::Obj(source), Value::Obj(target)],
    );
    call(
        &mut engine,
        context_builder,
        "setTranslationFlags",
        "(I)Landroid/view/translation/TranslationContext$Builder;",
        &[Value::Int(3)],
    );
    let translation_context = object(call(
        &mut engine,
        context_builder,
        "build",
        "()Landroid/view/translation/TranslationContext;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            translation_context,
            "getTranslationFlags",
            "()I",
            &[]
        ),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            translation_context,
            "getSourceSpec",
            "()Landroid/view/translation/TranslationSpec;",
            &[],
        ),
        Value::Obj(source)
    );

    let request_text = text(&mut engine, "Hello");
    let request_value = object(call_static(
        &mut engine,
        "Landroid/view/translation/TranslationRequestValue;",
        "forText",
        "(Ljava/lang/CharSequence;)Landroid/view/translation/TranslationRequestValue;",
        &[request_text],
    ));
    let autofill_id = instance(&mut engine, AF_ID);
    call(
        &mut engine,
        autofill_id,
        "<init>",
        "(I)V",
        &[Value::Int(17)],
    );
    let request_builder = instance(
        &mut engine,
        "Landroid/view/translation/ViewTranslationRequest$Builder;",
    );
    call(
        &mut engine,
        request_builder,
        "<init>",
        "(Landroid/view/autofill/AutofillId;)V",
        &[Value::Obj(autofill_id)],
    );
    let text_key = text(&mut engine, "android:text");
    call(
        &mut engine,
        request_builder,
        "setValue",
        "(Ljava/lang/String;Landroid/view/translation/TranslationRequestValue;)Landroid/view/translation/ViewTranslationRequest$Builder;",
        &[text_key.clone(), Value::Obj(request_value)],
    );
    let request = object(call(
        &mut engine,
        request_builder,
        "build",
        "()Landroid/view/translation/ViewTranslationRequest;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            request,
            "getValue",
            "(Ljava/lang/String;)Landroid/view/translation/TranslationRequestValue;",
            std::slice::from_ref(&text_key),
        ),
        Value::Obj(request_value)
    );
    let request_keys = call(&mut engine, request, "getKeys", "()Ljava/util/Set;", &[]);
    assert_eq!(p57_collection_size(&mut engine, request_keys), 1);

    let manager = p57_system_service(&mut engine, "translation");
    let capabilities = call(
        &mut engine,
        manager,
        "getOnDeviceTranslationCapabilities",
        "(II)Ljava/util/Set;",
        &[Value::Int(1), Value::Int(1)],
    );
    assert_eq!(p57_collection_size(&mut engine, capabilities), 0);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "createOnDeviceTranslator",
            "(Landroid/view/translation/TranslationContext;)Landroid/view/translation/Translator;",
            &[Value::Obj(translation_context)],
        ),
        Value::Null
    );

    // Deep audit: the deprecated TranslationSpec(String, int) constructor
    // performs no null check, so a null language is stored verbatim with
    // a root ULocale instead of throwing.
    let null_spec = instance(&mut engine, "Landroid/view/translation/TranslationSpec;");
    call(
        &mut engine,
        null_spec,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[Value::Null, Value::Int(1)],
    );
    assert_eq!(
        call(
            &mut engine,
            null_spec,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert!(matches!(
        call(
            &mut engine,
            null_spec,
            "getLocale",
            "()Landroid/icu/util/ULocale;",
            &[],
        ),
        Value::Obj(_)
    ));

    // Deep audit: ViewTranslationRequest.Builder.setValue accepts null
    // keys/values (ArrayMap), and because checkNotUsed only runs when the
    // map is first created, setValue after build() still mutates the map
    // that was handed to the built request.
    let null_key_builder = instance(
        &mut engine,
        "Landroid/view/translation/ViewTranslationRequest$Builder;",
    );
    call(
        &mut engine,
        null_key_builder,
        "<init>",
        "(Landroid/view/autofill/AutofillId;)V",
        &[Value::Obj(autofill_id)],
    );
    let k_key = text(&mut engine, "k");
    call(
        &mut engine,
        null_key_builder,
        "setValue",
        "(Ljava/lang/String;Landroid/view/translation/TranslationRequestValue;)Landroid/view/translation/ViewTranslationRequest$Builder;",
        &[k_key, Value::Null],
    );
    let null_key_request = object(call(
        &mut engine,
        null_key_builder,
        "build",
        "()Landroid/view/translation/ViewTranslationRequest;",
        &[],
    ));
    let k_lookup = text(&mut engine, "k");
    assert_eq!(
        call(
            &mut engine,
            null_key_request,
            "getValue",
            "(Ljava/lang/String;)Landroid/view/translation/TranslationRequestValue;",
            &[k_lookup],
        ),
        Value::Null
    );
    let late_key = text(&mut engine, "late");
    call(
        &mut engine,
        null_key_builder,
        "setValue",
        "(Ljava/lang/String;Landroid/view/translation/TranslationRequestValue;)Landroid/view/translation/ViewTranslationRequest$Builder;",
        &[late_key, Value::Obj(request_value)],
    );
    let late_lookup = text(&mut engine, "late");
    assert_eq!(
        call(
            &mut engine,
            null_key_request,
            "getValue",
            "(Ljava/lang/String;)Landroid/view/translation/TranslationRequestValue;",
            &[late_lookup],
        ),
        Value::Obj(request_value)
    );

    // Deep audit: TranslationContext.Builder.setActivityId performs no
    // null validation, and build() marks the builder used before the
    // constructor validates flags, so a flags failure still consumes it.
    let bad_context_builder = instance(
        &mut engine,
        "Landroid/view/translation/TranslationContext$Builder;",
    );
    call(
        &mut engine,
        bad_context_builder,
        "<init>",
        "(Landroid/view/translation/TranslationSpec;Landroid/view/translation/TranslationSpec;)V",
        &[Value::Obj(source), Value::Obj(target)],
    );
    call(
        &mut engine,
        bad_context_builder,
        "setActivityId",
        "(Landroid/app/assist/ActivityId;)Landroid/view/translation/TranslationContext$Builder;",
        &[Value::Null],
    );
    call(
        &mut engine,
        bad_context_builder,
        "setTranslationFlags",
        "(I)Landroid/view/translation/TranslationContext$Builder;",
        &[Value::Int(8)],
    );
    assert_throws(
        engine.invoke_instance(
            bad_context_builder,
            "build",
            "()Landroid/view/translation/TranslationContext;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            bad_context_builder,
            "build",
            "()Landroid/view/translation/TranslationContext;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    // Deep audit: the no-service capabilities result is
    // Collections.emptySet(), so adding to it throws.
    let capabilities_set = object(call(
        &mut engine,
        manager,
        "getOnDeviceTranslationCapabilities",
        "(II)Ljava/util/Set;",
        &[Value::Int(1), Value::Int(1)],
    ));
    assert_throws(
        engine.invoke_instance(
            capabilities_set,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(capability)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
}

#[test]
fn usb_explicit_descriptors_and_no_device_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let endpoint = instance(&mut engine, "Landroid/hardware/usb/UsbEndpoint;");
    call(
        &mut engine,
        endpoint,
        "<init>",
        "(IIII)V",
        &[
            Value::Int(0x81),
            Value::Int(2),
            Value::Int(64),
            Value::Int(0),
        ],
    );
    assert_eq!(
        call(&mut engine, endpoint, "getDirection", "()I", &[]),
        Value::Int(128)
    );
    assert_eq!(
        call(&mut engine, endpoint, "getEndpointNumber", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, endpoint, "getType", "()I", &[]),
        Value::Int(2)
    );

    let endpoints = object_array(
        &mut engine,
        "[Landroid/hardware/usb/UsbEndpoint;",
        vec![Value::Obj(endpoint)],
    );
    let usb_interface = instance(&mut engine, "Landroid/hardware/usb/UsbInterface;");
    let interface_name = text(&mut engine, "Bulk");
    call(
        &mut engine,
        usb_interface,
        "<init>",
        "(IIILjava/lang/String;II[Landroid/hardware/usb/UsbEndpoint;)V",
        &[
            Value::Int(0),
            Value::Int(0),
            Value::Int(8),
            interface_name,
            Value::Int(6),
            Value::Int(80),
            endpoints,
        ],
    );
    let interfaces = object_array(
        &mut engine,
        "[Landroid/hardware/usb/UsbInterface;",
        vec![Value::Obj(usb_interface)],
    );
    let configuration = instance(&mut engine, "Landroid/hardware/usb/UsbConfiguration;");
    call(
        &mut engine,
        configuration,
        "<init>",
        "(ILjava/lang/String;II[Landroid/hardware/usb/UsbInterface;)V",
        &[
            Value::Int(1),
            Value::Null,
            Value::Int(0x60),
            Value::Int(50),
            interfaces,
        ],
    );
    assert_eq!(
        call(&mut engine, configuration, "isSelfPowered", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, configuration, "isRemoteWakeup", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, configuration, "getMaxPower", "()I", &[]),
        Value::Int(100)
    );

    let configurations = object_array(
        &mut engine,
        "[Landroid/hardware/usb/UsbConfiguration;",
        vec![Value::Obj(configuration)],
    );
    let builder = instance(&mut engine, "Landroid/hardware/usb/UsbDevice$Builder;");
    let device_name = text(&mut engine, "/dev/bus/usb/001/002");
    let manufacturer = text(&mut engine, "Acme");
    let product = text(&mut engine, "Widget");
    let version = text(&mut engine, "2.0");
    let serial = text(&mut engine, "SER42");
    call(
        &mut engine,
        builder,
        "<init>",
        "(Ljava/lang/String;IIIIILjava/lang/String;Ljava/lang/String;Ljava/lang/String;[Landroid/hardware/usb/UsbConfiguration;Ljava/lang/String;ZZZZZ)V",
        &[
            device_name,
            Value::Int(0x1234),
            Value::Int(0x5678),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            manufacturer,
            product,
            version,
            configurations,
            serial,
            Value::Int(1),
            Value::Int(0),
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
        ],
    );
    let reader = instance(&mut engine, "Landroid/hardware/usb/IUsbSerialReader;");
    let device = object(call(
        &mut engine,
        builder,
        "build",
        "(Landroid/hardware/usb/IUsbSerialReader;)Landroid/hardware/usb/UsbDevice;",
        &[Value::Obj(reader)],
    ));
    assert_eq!(
        call(&mut engine, device, "getVendorId", "()I", &[]),
        Value::Int(0x1234)
    );
    assert_eq!(
        call(&mut engine, device, "getInterfaceCount", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getSerialNumber",
            "()Ljava/lang/String;",
            &[]
        ),
        "SER42"
    );
    assert_eq!(
        call(&mut engine, device, "getDeviceId", "()I", &[]),
        Value::Int(1002)
    );
    let static_name = text(&mut engine, "/dev/bus/usb/003/004");
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/hardware/usb/UsbDevice;",
            "getDeviceId",
            "(Ljava/lang/String;)I",
            std::slice::from_ref(&static_name),
        ),
        Value::Int(3004)
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            "Landroid/hardware/usb/UsbDevice;",
            "getDeviceName",
            "(I)Ljava/lang/String;",
            &[Value::Int(3004)],
        ),
        "/dev/bus/usb/003/004"
    );

    let manager = p57_system_service(&mut engine, "usb");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/hardware/usb/UsbManager;"
    );
    let devices = call(
        &mut engine,
        manager,
        "getDeviceList",
        "()Ljava/util/HashMap;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, devices), 0);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getAccessoryList",
            "()[Landroid/hardware/usb/UsbAccessory;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "hasPermission",
            "(Landroid/hardware/usb/UsbDevice;)Z",
            &[Value::Obj(device)],
        ),
        Value::Int(0)
    );

    let function_name = text(&mut engine, "mtp,adb");
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/hardware/usb/UsbManager;",
            "usbFunctionsFromString",
            "(Ljava/lang/String;)J",
            std::slice::from_ref(&function_name),
        ),
        Value::Long(5)
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            "Landroid/hardware/usb/UsbManager;",
            "usbFunctionsToString",
            "(J)Ljava/lang/String;",
            &[Value::Long(5)],
        ),
        "mtp,adb"
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/hardware/usb/UsbManager;",
            "areSettableFunctions",
            "(J)Z",
            &[Value::Long((1 << 5) | (1 << 10))],
        ),
        Value::Int(1)
    );

    // Deep audit: usbFunctionsFromString maps null/"none" to FUNCTION_NONE,
    // skips empty segments, and rejects unknown names with IAE.
    let usb_manager_class = "Landroid/hardware/usb/UsbManager;";
    assert_eq!(
        call_static(
            &mut engine,
            usb_manager_class,
            "usbFunctionsFromString",
            "(Ljava/lang/String;)J",
            &[Value::Null],
        ),
        Value::Long(0)
    );
    let none_functions = text(&mut engine, "none");
    assert_eq!(
        call_static(
            &mut engine,
            usb_manager_class,
            "usbFunctionsFromString",
            "(Ljava/lang/String;)J",
            &[none_functions],
        ),
        Value::Long(0)
    );
    let bogus_functions = text(&mut engine, "mtp,bogus");
    assert_throws(
        engine.invoke_static(
            usb_manager_class,
            "usbFunctionsFromString",
            "(Ljava/lang/String;)J",
            &[bogus_functions],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: only none, a single settable function, or the
    // rndis+ncm pair are settable (adb is not in the settable mask).
    assert_eq!(
        call_static(
            &mut engine,
            usb_manager_class,
            "areSettableFunctions",
            "(J)Z",
            &[Value::Long((1 << 2) | (1 << 4))],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            usb_manager_class,
            "areSettableFunctions",
            "(J)Z",
            &[Value::Long(1)],
        ),
        Value::Int(0)
    );

    // Deep audit: interface lookup is a raw array access past the end.
    assert_throws(
        engine.invoke_instance(
            device,
            "getInterface",
            "(I)Landroid/hardware/usb/UsbInterface;",
            &[Value::Int(99)],
        ),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );
}

#[test]
fn euicc_info_and_disabled_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "euicc");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/telephony/euicc/EuiccManager;"
    );
    assert_eq!(
        call(&mut engine, manager, "isEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, manager, "getEid", "()Ljava/lang/String;", &[]),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, manager, "getOtaStatus", "()I", &[]),
        Value::Int(5)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getEuiccInfo",
            "()Landroid/telephony/euicc/EuiccInfo;",
            &[],
        ),
        Value::Null
    );
    let countries = call(
        &mut engine,
        manager,
        "getSupportedCountries",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, countries), 0);
    let country = text(&mut engine, "BR");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isSupportedCountry",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&country),
        ),
        Value::Int(0)
    );
    // The disabled branch returns before touching its arguments: a null
    // country is still just "not supported" and a null country list is
    // silently ignored, matching AOSP's isEnabled()-first ordering.
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isSupportedCountry",
            "(Ljava/lang/String;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        manager,
        "setSupportedCountries",
        "(Ljava/util/List;)V",
        &[Value::Null],
    );
    // isSimPortAvailable is the exception: AOSP dereferences the absent
    // IEuiccController without an isEnabled() guard.
    assert_throws(
        engine.invoke_instance(manager, "isSimPortAvailable", "(I)Z", &[Value::Int(0)]),
        "Ljava/lang/NullPointerException;",
    );
    // The public constructor dereferences its Context immediately.
    let bare_manager = instance(&mut engine, "Landroid/telephony/euicc/EuiccManager;");
    assert_throws(
        engine.invoke_instance(
            bare_manager,
            "<init>",
            "(Landroid/content/Context;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let for_card = object(call(
        &mut engine,
        manager,
        "createForCardId",
        "(I)Landroid/telephony/euicc/EuiccManager;",
        &[Value::Int(9)],
    ));
    assert_ne!(for_card, manager);
    assert_eq!(
        call(&mut engine, for_card, "isEnabled", "()Z", &[]),
        Value::Int(0)
    );

    let pending = instance(&mut engine, "Landroid/app/PendingIntent;");
    call(
        &mut engine,
        manager,
        "deleteSubscription",
        "(ILandroid/app/PendingIntent;)V",
        &[Value::Int(4), Value::Obj(pending)],
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "deleteSubscription",
            "(ILandroid/app/PendingIntent;)V",
            &[Value::Int(4), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let info = instance(&mut engine, "Landroid/telephony/euicc/EuiccInfo;");
    let os_version = text(&mut engine, "OS 1.2");
    call(
        &mut engine,
        info,
        "<init>",
        "(Ljava/lang/String;)V",
        std::slice::from_ref(&os_version),
    );
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getOsVersion",
            "()Ljava/lang/String;",
            &[]
        ),
        "OS 1.2"
    );
}

#[test]
fn usage_empty_history_events_and_stats_merge_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "usagestats");
    let usage = call(
        &mut engine,
        manager,
        "queryUsageStats",
        "(IJJ)Ljava/util/List;",
        &[Value::Int(4), Value::Long(0), Value::Long(1000)],
    );
    assert_eq!(p57_collection_size(&mut engine, usage), 0);
    let events = object(call(
        &mut engine,
        manager,
        "queryEvents",
        "(JJ)Landroid/app/usage/UsageEvents;",
        &[Value::Long(0), Value::Long(1000)],
    ));
    assert_eq!(
        call(&mut engine, events, "hasNextEvent", "()Z", &[]),
        Value::Int(0)
    );
    let package = text(&mut engine, "com.example");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isAppInactive",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&package),
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, manager, "getAppStandbyBucket", "()I", &[]),
        Value::Int(10)
    );

    let set_stats =
        |engine: &mut Engine, stats: ObjRef, begin: i64, end: i64, last: i64, total: i64| {
            let package = text(engine, "com.example");
            engine
                .heap
                .put_field(stats, "mPackageName", package)
                .unwrap();
            engine
                .heap
                .put_field(stats, "mBeginTimeStamp", Value::Long(begin))
                .unwrap();
            engine
                .heap
                .put_field(stats, "mEndTimeStamp", Value::Long(end))
                .unwrap();
            engine
                .heap
                .put_field(stats, "mLastTimeUsed", Value::Long(last))
                .unwrap();
            engine
                .heap
                .put_field(stats, "mTotalTimeInForeground", Value::Long(total))
                .unwrap();
            engine
                .heap
                .put_field(stats, "mAppLaunchCount", Value::Int(1))
                .unwrap();
        };

    let left = instance(&mut engine, "Landroid/app/usage/UsageStats;");
    call(&mut engine, left, "<init>", "()V", &[]);
    set_stats(&mut engine, left, 10, 20, 15, 100);
    let right = instance(&mut engine, "Landroid/app/usage/UsageStats;");
    call(&mut engine, right, "<init>", "()V", &[]);
    set_stats(&mut engine, right, 5, 30, 99, 50);
    let copy = instance(&mut engine, "Landroid/app/usage/UsageStats;");
    call(
        &mut engine,
        copy,
        "<init>",
        "(Landroid/app/usage/UsageStats;)V",
        &[Value::Obj(left)],
    );
    assert_eq!(
        call(&mut engine, copy, "getTotalTimeInForeground", "()J", &[]),
        Value::Long(100)
    );

    call(
        &mut engine,
        left,
        "add",
        "(Landroid/app/usage/UsageStats;)V",
        &[Value::Obj(right)],
    );
    assert_eq!(
        call(&mut engine, left, "getFirstTimeStamp", "()J", &[]),
        Value::Long(5)
    );
    assert_eq!(
        call(&mut engine, left, "getLastTimeStamp", "()J", &[]),
        Value::Long(30)
    );
    assert_eq!(
        call(&mut engine, left, "getTotalTimeInForeground", "()J", &[]),
        Value::Long(150)
    );
    // AOSP only maxes last-used fields when right begins after left.
    assert_eq!(
        call(&mut engine, left, "getLastTimeUsed", "()J", &[]),
        Value::Long(15)
    );
    assert_eq!(
        call(&mut engine, left, "getAppLaunchCount", "()I", &[]),
        Value::Int(2)
    );

    let newer = instance(&mut engine, "Landroid/app/usage/UsageStats;");
    call(&mut engine, newer, "<init>", "()V", &[]);
    set_stats(&mut engine, newer, 20, 40, 99, 0);
    call(
        &mut engine,
        left,
        "add",
        "(Landroid/app/usage/UsageStats;)V",
        &[Value::Obj(newer)],
    );
    assert_eq!(
        call(&mut engine, left, "getLastTimeUsed", "()J", &[]),
        Value::Long(99)
    );

    let event = instance(&mut engine, "Landroid/app/usage/UsageEvents$Event;");
    call(
        &mut engine,
        event,
        "<init>",
        "(IJ)V",
        &[Value::Int(11), Value::Long(1234)],
    );
    let event_package = text(&mut engine, "com.example");
    engine
        .heap
        .put_field(event, "mPackage", event_package)
        .unwrap();
    engine
        .heap
        .put_field(event, "mBucketAndReason", Value::Int((30 << 16) | 7))
        .unwrap();
    let event_copy = instance(&mut engine, "Landroid/app/usage/UsageEvents$Event;");
    call(
        &mut engine,
        event_copy,
        "<init>",
        "(Landroid/app/usage/UsageEvents$Event;)V",
        &[Value::Obj(event)],
    );
    assert_eq!(
        call(&mut engine, event_copy, "getAppStandbyBucket", "()I", &[]),
        Value::Int(30)
    );
    assert_eq!(
        call(&mut engine, event_copy, "getStandbyReason", "()I", &[]),
        Value::Int(7)
    );

    let list = list_of(&mut engine, &[Value::Obj(event)]);
    let empty_pool = p57_string_array(&mut engine, &[]);
    let iterable = instance(&mut engine, "Landroid/app/usage/UsageEvents;");
    call(
        &mut engine,
        iterable,
        "<init>",
        "(Ljava/util/List;[Ljava/lang/String;)V",
        &[Value::Obj(list), empty_pool],
    );
    let out = instance(&mut engine, "Landroid/app/usage/UsageEvents$Event;");
    call(&mut engine, out, "<init>", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            iterable,
            "getNextEvent",
            "(Landroid/app/usage/UsageEvents$Event;)Z",
            &[Value::Obj(out)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            out,
            "getPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example"
    );
    assert_eq!(
        call(&mut engine, iterable, "hasNextEvent", "()Z", &[]),
        Value::Int(0)
    );

    // Deep audit: getNextEvent rejects a null output event with IAE, and
    // resetToStart rewinds the cursor so the sequence can be read again.
    assert_throws(
        engine.invoke_instance(
            iterable,
            "getNextEvent",
            "(Landroid/app/usage/UsageEvents$Event;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(&mut engine, iterable, "resetToStart", "()V", &[]);
    assert_eq!(
        call(&mut engine, iterable, "hasNextEvent", "()Z", &[]),
        Value::Int(1)
    );
    let second_pass = instance(&mut engine, "Landroid/app/usage/UsageEvents$Event;");
    call(&mut engine, second_pass, "<init>", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            iterable,
            "getNextEvent",
            "(Landroid/app/usage/UsageEvents$Event;)Z",
            &[Value::Obj(second_pass)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            second_pass,
            "getPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example"
    );
}

#[test]
fn print_values_rotation_and_no_service_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let media = instance(&mut engine, "Landroid/print/PrintAttributes$MediaSize;");
    let media_id = text(&mut engine, "CUSTOM");
    let media_label = text(&mut engine, "Custom");
    call(
        &mut engine,
        media,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;II)V",
        &[media_id, media_label, Value::Int(8500), Value::Int(11000)],
    );
    assert_eq!(
        call(&mut engine, media, "isPortrait", "()Z", &[]),
        Value::Int(1)
    );
    let landscape_media = object(call(
        &mut engine,
        media,
        "asLandscape",
        "()Landroid/print/PrintAttributes$MediaSize;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, landscape_media, "getWidthMils", "()I", &[]),
        Value::Int(11000)
    );

    let resolution = instance(&mut engine, "Landroid/print/PrintAttributes$Resolution;");
    let resolution_id = text(&mut engine, "res");
    let resolution_label = text(&mut engine, "300x600");
    call(
        &mut engine,
        resolution,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;II)V",
        &[
            resolution_id,
            resolution_label,
            Value::Int(300),
            Value::Int(600),
        ],
    );
    let margins = instance(&mut engine, "Landroid/print/PrintAttributes$Margins;");
    call(
        &mut engine,
        margins,
        "<init>",
        "(IIII)V",
        &[
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
        ],
    );

    let builder = instance(&mut engine, "Landroid/print/PrintAttributes$Builder;");
    call(&mut engine, builder, "<init>", "()V", &[]);
    call(
        &mut engine,
        builder,
        "setMediaSize",
        "(Landroid/print/PrintAttributes$MediaSize;)Landroid/print/PrintAttributes$Builder;",
        &[Value::Obj(media)],
    );
    call(
        &mut engine,
        builder,
        "setResolution",
        "(Landroid/print/PrintAttributes$Resolution;)Landroid/print/PrintAttributes$Builder;",
        &[Value::Obj(resolution)],
    );
    call(
        &mut engine,
        builder,
        "setMinMargins",
        "(Landroid/print/PrintAttributes$Margins;)Landroid/print/PrintAttributes$Builder;",
        &[Value::Obj(margins)],
    );
    call(
        &mut engine,
        builder,
        "setColorMode",
        "(I)Landroid/print/PrintAttributes$Builder;",
        &[Value::Int(2)],
    );
    call(
        &mut engine,
        builder,
        "setDuplexMode",
        "(I)Landroid/print/PrintAttributes$Builder;",
        &[Value::Int(2)],
    );
    let attrs = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/print/PrintAttributes;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, attrs, "getColorMode", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, attrs, "isPortrait", "()Z", &[]),
        Value::Int(1)
    );
    let landscape = object(call(
        &mut engine,
        attrs,
        "asLandscape",
        "()Landroid/print/PrintAttributes;",
        &[],
    ));
    let landscape_resolution = object(call(
        &mut engine,
        landscape,
        "getResolution",
        "()Landroid/print/PrintAttributes$Resolution;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            landscape_resolution,
            "getHorizontalDpi",
            "()I",
            &[]
        ),
        Value::Int(600)
    );
    assert_throws(
        engine.invoke_instance(attrs, "setColorMode", "(I)V", &[Value::Int(3)]),
        "Ljava/lang/IllegalArgumentException;",
    );

    let range = instance(&mut engine, "Landroid/print/PageRange;");
    call(
        &mut engine,
        range,
        "<init>",
        "(II)V",
        &[Value::Int(2), Value::Int(4)],
    );
    assert_eq!(
        call(&mut engine, range, "contains", "(I)Z", &[Value::Int(3)]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, range, "getSize", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call_text(&mut engine, range, "toString", "()Ljava/lang/String;", &[]),
        "PageRange[2 - 4]"
    );

    let component = instance(&mut engine, "Landroid/content/ComponentName;");
    let package = text(&mut engine, "com.example.print");
    let cls = text(&mut engine, "com.example.print.Service");
    call(
        &mut engine,
        component,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[package, cls],
    );
    let printer = instance(&mut engine, "Landroid/print/PrinterId;");
    let local = text(&mut engine, "printer-1");
    call(
        &mut engine,
        printer,
        "<init>",
        "(Landroid/content/ComponentName;Ljava/lang/String;)V",
        &[Value::Obj(component), local],
    );
    assert_eq!(
        call_text(
            &mut engine,
            printer,
            "getLocalId",
            "()Ljava/lang/String;",
            &[]
        ),
        "printer-1"
    );
    assert!(call_text(
        &mut engine,
        printer,
        "toString",
        "()Ljava/lang/String;",
        &[]
    )
    .contains("com.example.print/com.example.print.Service"));

    let manager = p57_system_service(&mut engine, "print");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/print/PrintManager;"
    );
    let jobs = call(
        &mut engine,
        manager,
        "getPrintJobs",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, jobs), 0);
    let empty_name = text(&mut engine, "");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "print",
            "(Ljava/lang/String;Landroid/print/PrintDocumentAdapter;Landroid/print/PrintAttributes;)Landroid/print/PrintJob;",
            &[empty_name, Value::Null, Value::Null],
        ),
        Value::Null,
        "PrintManager checks the absent print service before other validation"
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "getPrintServices",
            "(I)Ljava/util/List;",
            &[Value::Int(8)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: the Builder' mAttributes is the object build() returns,
    // so setter calls after build() mutate the already-returned instance,
    // and repeated build() calls return the same instance.
    let built = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/print/PrintAttributes;",
        &[],
    ));
    let built_again = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/print/PrintAttributes;",
        &[],
    ));
    assert_eq!(built, built_again);
    call(
        &mut engine,
        builder,
        "setColorMode",
        "(I)Landroid/print/PrintAttributes$Builder;",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, built, "getColorMode", "()I", &[]),
        Value::Int(1)
    );
}

#[test]
fn keystore_specs_protection_and_key_info_preserve_explicit_values() {
    let mut engine = engine_of(&DexBuilder::new());
    let builder = instance(
        &mut engine,
        "Landroid/security/keystore/KeyGenParameterSpec$Builder;",
    );
    let alias = text(&mut engine, "signing");
    call(
        &mut engine,
        builder,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[alias, Value::Int(4 | 8)],
    );
    call(
        &mut engine,
        builder,
        "setKeySize",
        "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        &[Value::Int(256)],
    );
    let digests = p57_string_array(&mut engine, &["SHA-256", "SHA-512"]);
    call(
        &mut engine,
        builder,
        "setDigests",
        "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        std::slice::from_ref(&digests),
    );
    let blocks = p57_string_array(&mut engine, &["GCM"]);
    call(
        &mut engine,
        builder,
        "setBlockModes",
        "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        std::slice::from_ref(&blocks),
    );
    call(
        &mut engine,
        builder,
        "setUserAuthenticationParameters",
        "(II)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        &[Value::Int(30), Value::Int(3)],
    );
    let challenge = byte_array(&mut engine, &[9, 8, 7]);
    call(
        &mut engine,
        builder,
        "setAttestationChallenge",
        "([B)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        std::slice::from_ref(&challenge),
    );
    let spec = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/security/keystore/KeyGenParameterSpec;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            spec,
            "getKeystoreAlias",
            "()Ljava/lang/String;",
            &[]
        ),
        "signing"
    );
    assert_eq!(
        call(&mut engine, spec, "getKeySize", "()I", &[]),
        Value::Int(256)
    );
    assert_eq!(
        call(&mut engine, spec, "getPurposes", "()I", &[]),
        Value::Int(12)
    );
    let spec_digests = call(
        &mut engine,
        spec,
        "getDigests",
        "()[Ljava/lang/String;",
        &[],
    );
    assert_eq!(
        p57_string_values(&engine, &spec_digests),
        ["SHA-256", "SHA-512"]
    );
    let returned_challenge = call(&mut engine, spec, "getAttestationChallenge", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &returned_challenge), [9, 8, 7]);
    let not_before = call(
        &mut engine,
        spec,
        "getCertificateNotBefore",
        "()Ljava/util/Date;",
        &[],
    );
    assert_eq!(p57_date_value(&mut engine, not_before), 0);
    let not_after = call(
        &mut engine,
        spec,
        "getCertificateNotAfter",
        "()Ljava/util/Date;",
        &[],
    );
    assert_eq!(p57_date_value(&mut engine, not_after), 2_461_449_600_000);
    assert!(matches!(
        call(
            &mut engine,
            spec,
            "getCertificateSubject",
            "()Ljavax/security/auth/x500/X500Principal;",
            &[],
        ),
        Value::Obj(_)
    ));
    assert_eq!(
        call(
            &mut engine,
            spec,
            "getUserAuthenticationValidityDurationSeconds",
            "()I",
            &[]
        ),
        Value::Int(30)
    );

    let plain_builder = instance(
        &mut engine,
        "Landroid/security/keystore/KeyGenParameterSpec$Builder;",
    );
    let plain_alias = text(&mut engine, "plain");
    call(
        &mut engine,
        plain_builder,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[plain_alias, Value::Int(1)],
    );
    let plain = object(call(
        &mut engine,
        plain_builder,
        "build",
        "()Landroid/security/keystore/KeyGenParameterSpec;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, plain, "isDigestsSpecified", "()Z", &[]),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_instance(plain, "getDigests", "()[Ljava/lang/String;", &[]),
        "Ljava/lang/IllegalStateException;",
    );

    let protection_builder = instance(
        &mut engine,
        "Landroid/security/keystore/KeyProtection$Builder;",
    );
    call(
        &mut engine,
        protection_builder,
        "<init>",
        "(I)V",
        &[Value::Int(1 | 2)],
    );
    let paddings = p57_string_array(&mut engine, &["NoPadding"]);
    call(
        &mut engine,
        protection_builder,
        "setEncryptionPaddings",
        "([Ljava/lang/String;)Landroid/security/keystore/KeyProtection$Builder;",
        std::slice::from_ref(&paddings),
    );
    call(
        &mut engine,
        protection_builder,
        "setRandomizedEncryptionRequired",
        "(Z)Landroid/security/keystore/KeyProtection$Builder;",
        &[Value::Int(0)],
    );
    let protection = object(call(
        &mut engine,
        protection_builder,
        "build",
        "()Landroid/security/keystore/KeyProtection;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, protection, "getPurposes", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            protection,
            "isRandomizedEncryptionRequired",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    let protection_paddings = call(
        &mut engine,
        protection,
        "getEncryptionPaddings",
        "()[Ljava/lang/String;",
        &[],
    );
    assert_eq!(
        p57_string_values(&engine, &protection_paddings),
        ["NoPadding"]
    );

    let empty_strings = p57_string_array(&mut engine, &[]);
    let sig_paddings = p57_string_array(&mut engine, &["PSS"]);
    let key_digests = p57_string_array(&mut engine, &["SHA-256"]);
    let empty_blocks = p57_string_array(&mut engine, &[]);
    let key_info = instance(&mut engine, "Landroid/security/keystore/KeyInfo;");
    let info_alias = text(&mut engine, "info-key");
    call(
        &mut engine,
        key_info,
        "<init>",
        "(Ljava/lang/String;ZIILjava/util/Date;Ljava/util/Date;Ljava/util/Date;I[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;ZIIZZZZZII)V",
        &[
            info_alias,
            Value::Int(1),
            Value::Int(1),
            Value::Int(2048),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(4),
            empty_strings,
            sig_paddings,
            key_digests,
            empty_blocks,
            Value::Int(1),
            Value::Int(15),
            Value::Int(3),
            Value::Int(1),
            Value::Int(0),
            Value::Int(1),
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(9),
        ],
    );
    assert_eq!(
        call(&mut engine, key_info, "getKeySize", "()I", &[]),
        Value::Int(2048)
    );
    assert_eq!(
        call(&mut engine, key_info, "isInsideSecureHardware", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, key_info, "getSecurityLevel", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, key_info, "getRemainingUsageCount", "()I", &[]),
        Value::Int(9)
    );
    let info_digests = call(
        &mut engine,
        key_info,
        "getDigests",
        "()[Ljava/lang/String;",
        &[],
    );
    assert_eq!(p57_string_values(&engine, &info_digests), ["SHA-256"]);
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/security/keystore/KeyProperties;",
            "namespaceToLegacyUid",
            "(I)I",
            &[Value::Int(102)],
        ),
        Value::Int(1010)
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/security/keystore/KeyProperties;",
            "legacyUidToNamespace",
            "(I)I",
            &[Value::Int(1010)],
        ),
        Value::Int(102)
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/security/keystore/KeyProperties;",
            "namespaceToLegacyUid",
            "(I)I",
            &[Value::Int(-1)],
        ),
        Value::Int(-1)
    );

    // Deep audit: KeyGenParameterSpec.getUid wraps KeyProperties'
    // IllegalArgumentException for non-legacy namespaces into an
    // IllegalStateException (the static conversion itself throws IAE).
    assert_throws(
        engine.invoke_static(
            "Landroid/security/keystore/KeyProperties;",
            "namespaceToLegacyUid",
            "(I)I",
            &[Value::Int(103)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let namespaced_builder = instance(
        &mut engine,
        "Landroid/security/keystore/KeyGenParameterSpec$Builder;",
    );
    let namespaced_alias = text(&mut engine, "namespaced-key");
    call(
        &mut engine,
        namespaced_builder,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[namespaced_alias, Value::Int(3)],
    );
    call(
        &mut engine,
        namespaced_builder,
        "setNamespace",
        "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
        &[Value::Int(103)],
    );
    let namespaced_spec = object(call(
        &mut engine,
        namespaced_builder,
        "build",
        "()Landroid/security/keystore/KeyGenParameterSpec;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(namespaced_spec, "getUid", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
}

#[test]
fn mtp_object_info_and_unopened_device_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let builder = instance(&mut engine, "Landroid/mtp/MtpObjectInfo$Builder;");
    call(&mut engine, builder, "<init>", "()V", &[]);
    let object_name = text(&mut engine, "photo.jpg");
    call(
        &mut engine,
        builder,
        "setName",
        "(Ljava/lang/String;)Landroid/mtp/MtpObjectInfo$Builder;",
        std::slice::from_ref(&object_name),
    );
    call(
        &mut engine,
        builder,
        "setStorageId",
        "(I)Landroid/mtp/MtpObjectInfo$Builder;",
        &[Value::Int(0x10001)],
    );
    call(
        &mut engine,
        builder,
        "setFormat",
        "(I)Landroid/mtp/MtpObjectInfo$Builder;",
        &[Value::Int(0x3801)],
    );
    call(
        &mut engine,
        builder,
        "setCompressedSize",
        "(J)Landroid/mtp/MtpObjectInfo$Builder;",
        &[Value::Long(0xffff_ffff)],
    );
    call(
        &mut engine,
        builder,
        "setImagePixWidth",
        "(J)Landroid/mtp/MtpObjectInfo$Builder;",
        &[Value::Long(4000)],
    );
    let info = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/mtp/MtpObjectInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, info, "getObjectHandle", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call_text(&mut engine, info, "getName", "()Ljava/lang/String;", &[]),
        "photo.jpg"
    );
    assert_eq!(
        call(&mut engine, info, "getCompressedSizeLong", "()J", &[]),
        Value::Long(0xffff_ffff)
    );
    // AOSP's 32-bit getter runs Preconditions.checkState(size >= 0): the
    // unsigned value above Integer.MAX_VALUE is only visible via the Long
    // variant, and the int getter must fail instead of returning -1.
    assert_throws(
        engine.invoke_instance(info, "getCompressedSize", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    // longToUint32 validates on the RHS before the (already-consumed)
    // builder's null mObjectInfo is stored through, so a negative value is
    // an IllegalArgumentException even after build().
    assert_throws(
        engine.invoke_instance(
            builder,
            "setCompressedSize",
            "(J)Landroid/mtp/MtpObjectInfo$Builder;",
            &[Value::Long(-1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_eq!(
        call(
            &mut engine,
            builder,
            "build",
            "()Landroid/mtp/MtpObjectInfo;",
            &[]
        ),
        Value::Null
    );

    let copy_builder = instance(&mut engine, "Landroid/mtp/MtpObjectInfo$Builder;");
    call(
        &mut engine,
        copy_builder,
        "<init>",
        "(Landroid/mtp/MtpObjectInfo;)V",
        &[Value::Obj(info)],
    );
    let copied = object(call(
        &mut engine,
        copy_builder,
        "build",
        "()Landroid/mtp/MtpObjectInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, copied, "getObjectHandle", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, copied, "getImagePixWidth", "()I", &[]),
        Value::Int(4000)
    );

    let device_info = instance(&mut engine, "Landroid/mtp/MtpDeviceInfo;");
    call(&mut engine, device_info, "<init>", "()V", &[]);
    let manufacturer = text(&mut engine, "CameraCo");
    engine
        .heap
        .put_field(device_info, "mManufacturer", manufacturer)
        .unwrap();
    let operations = int_array(&mut engine, &[0x1001, 0x1002]);
    engine
        .heap
        .put_field(device_info, "mOperationsSupported", operations)
        .unwrap();
    assert_eq!(
        call_text(
            &mut engine,
            device_info,
            "getManufacturer",
            "()Ljava/lang/String;",
            &[]
        ),
        "CameraCo"
    );
    assert_eq!(
        call(
            &mut engine,
            device_info,
            "isOperationSupported",
            "(I)Z",
            &[Value::Int(0x1002)]
        ),
        Value::Int(1)
    );

    let storage_info = instance(&mut engine, "Landroid/mtp/MtpStorageInfo;");
    call(&mut engine, storage_info, "<init>", "()V", &[]);
    engine
        .heap
        .put_field(storage_info, "mStorageId", Value::Int(7))
        .unwrap();
    engine
        .heap
        .put_field(storage_info, "mMaxCapacity", Value::Long(1000))
        .unwrap();
    engine
        .heap
        .put_field(storage_info, "mFreeSpace", Value::Long(250))
        .unwrap();
    assert_eq!(
        call(&mut engine, storage_info, "getMaxCapacity", "()J", &[]),
        Value::Long(1000)
    );
    assert_eq!(
        call(&mut engine, storage_info, "getFreeSpace", "()J", &[]),
        Value::Long(250)
    );

    let usb_builder = instance(&mut engine, "Landroid/hardware/usb/UsbDevice$Builder;");
    let usb_name = text(&mut engine, "/dev/bus/usb/002/003");
    let usb_manufacturer = text(&mut engine, "CameraCo");
    let usb_product = text(&mut engine, "Camera");
    let usb_version = text(&mut engine, "1.0");
    let configurations = object_array(
        &mut engine,
        "[Landroid/hardware/usb/UsbConfiguration;",
        Vec::new(),
    );
    call(
        &mut engine,
        usb_builder,
        "<init>",
        "(Ljava/lang/String;IIIIILjava/lang/String;Ljava/lang/String;Ljava/lang/String;[Landroid/hardware/usb/UsbConfiguration;Ljava/lang/String;ZZZZZ)V",
        &[
            usb_name,
            Value::Int(1),
            Value::Int(2),
            Value::Int(6),
            Value::Int(1),
            Value::Int(1),
            usb_manufacturer,
            usb_product,
            usb_version,
            configurations,
            Value::Null,
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
        ],
    );
    let reader = instance(&mut engine, "Landroid/hardware/usb/IUsbSerialReader;");
    let usb_device = object(call(
        &mut engine,
        usb_builder,
        "build",
        "(Landroid/hardware/usb/IUsbSerialReader;)Landroid/hardware/usb/UsbDevice;",
        &[Value::Obj(reader)],
    ));

    let mtp = instance(&mut engine, "Landroid/mtp/MtpDevice;");
    call(
        &mut engine,
        mtp,
        "<init>",
        "(Landroid/hardware/usb/UsbDevice;)V",
        &[Value::Obj(usb_device)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            mtp,
            "getDeviceName",
            "()Ljava/lang/String;",
            &[]
        ),
        "/dev/bus/usb/002/003"
    );
    assert_eq!(
        call(&mut engine, mtp, "getDeviceId", "()I", &[]),
        Value::Int(2003)
    );
    let connection = instance(&mut engine, "Landroid/hardware/usb/UsbDeviceConnection;");
    assert_eq!(
        call(
            &mut engine,
            mtp,
            "open",
            "(Landroid/hardware/usb/UsbDeviceConnection;)Z",
            &[Value::Obj(connection)],
        ),
        Value::Int(0),
        "without a native MTP transport, open fails rather than lying"
    );
    assert_eq!(
        call(&mut engine, mtp, "getStorageIds", "()[I", &[]),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            mtp,
            "readEvent",
            "(Landroid/os/CancellationSignal;)Landroid/mtp/MtpEvent;",
            &[Value::Null],
        ),
        "Ljava/io/IOException;",
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/mtp/MtpConstants;",
            "isAbstractObject",
            "(I)Z",
            &[Value::Int(0xba02)],
        ),
        Value::Int(1)
    );
}

// ---------------------------------------------------------------------------
// Batch 49-A: the first eight of the remaining 49 formerly-not_implemented
// surfaces, promoted only to the bounded partial profiles described in the
// registry notes.
// ---------------------------------------------------------------------------

fn p49_new(engine: &mut Engine, class: &str, sig: &str, args: &[Value]) -> ObjRef {
    let value = instance(engine, class);
    call(engine, value, "<init>", sig, args);
    value
}

fn p49_field_or_null(engine: &Engine, recv: ObjRef, name: &str) -> Value {
    engine.heap.get_field(recv, name).unwrap_or(Value::Null)
}

fn p49_list_get(engine: &mut Engine, list: Value, index: i32) -> Value {
    call(
        engine,
        object(list),
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(index)],
    )
}

fn p49_uuid(engine: &mut Engine, value: &str) -> Value {
    let text_value = text(engine, value);
    call_static(
        engine,
        "Ljava/util/UUID;",
        "fromString",
        "(Ljava/lang/String;)Ljava/util/UUID;",
        &[text_value],
    )
}

fn p49_locale(engine: &mut Engine, language: &str) -> ObjRef {
    let language = text(engine, language);
    p49_new(
        engine,
        "Ljava/util/Locale;",
        "(Ljava/lang/String;)V",
        &[language],
    )
}

#[test]
fn fingerprint_absent_service_and_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "fingerprint");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/hardware/fingerprint/FingerprintManager;"
    );
    assert_eq!(
        call(&mut engine, manager, "isHardwareDetected", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, manager, "hasEnrolledFingerprints", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getEnrolledFingerprints",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null,
        "AOSP returns null when the fingerprint service is absent"
    );
    let sensors = call(
        &mut engine,
        manager,
        "getSensorProperties",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, sensors), 0);

    let callback_class = "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationCallback;";
    let callback = p49_new(&mut engine, callback_class, "()V", &[]);
    let authenticate_sig = "(Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;Landroid/os/CancellationSignal;ILandroid/hardware/fingerprint/FingerprintManager$AuthenticationCallback;Landroid/os/Handler;)V";
    assert_throws(
        engine.invoke_instance(
            manager,
            "authenticate",
            authenticate_sig,
            &[
                Value::Null,
                Value::Null,
                Value::Int(0),
                Value::Null,
                Value::Null,
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        manager,
        "authenticate",
        authenticate_sig,
        &[
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Obj(callback),
            Value::Null,
        ],
    );
    assert_eq!(
        p49_field_or_null(&engine, callback, "rd_result"),
        Value::Null
    );
    assert_eq!(
        p49_field_or_null(&engine, callback, "rd_error_code"),
        Value::Null
    );

    let name = text(&mut engine, "right index");
    let fingerprint = p49_new(
        &mut engine,
        "Landroid/hardware/fingerprint/Fingerprint;",
        "(Ljava/lang/CharSequence;IIJ)V",
        &[name, Value::Int(3), Value::Int(7), Value::Long(99)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            fingerprint,
            "getName",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "right index"
    );
    assert_eq!(
        call(&mut engine, fingerprint, "getGroupId", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, fingerprint, "getBiometricId", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(&mut engine, fingerprint, "getDeviceId", "()J", &[]),
        Value::Long(99)
    );

    let cipher = instance(&mut engine, "Ljavax/crypto/Cipher;");
    let crypto = p49_new(
        &mut engine,
        "Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;",
        "(Ljavax/crypto/Cipher;)V",
        &[Value::Obj(cipher)],
    );
    assert_eq!(
        call(
            &mut engine,
            crypto,
            "getCipher",
            "()Ljavax/crypto/Cipher;",
            &[]
        ),
        Value::Obj(cipher)
    );
    assert_eq!(
        call(
            &mut engine,
            crypto,
            "getSignature",
            "()Ljava/security/Signature;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, crypto, "getOpId", "()J", &[]),
        Value::Long(0)
    );

    let result = p49_new(
        &mut engine,
        "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationResult;",
        "(Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;Landroid/hardware/fingerprint/Fingerprint;IZ)V",
        &[
            Value::Obj(crypto),
            Value::Obj(fingerprint),
            Value::Int(10),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            result,
            "getFingerprint",
            "()Landroid/hardware/fingerprint/Fingerprint;",
            &[]
        ),
        Value::Obj(fingerprint)
    );
    assert_eq!(
        call(&mut engine, result, "getUserId", "()I", &[]),
        Value::Int(10)
    );
    assert_eq!(
        call(&mut engine, result, "isStrongBiometric", "()Z", &[]),
        Value::Int(1)
    );

    // The three-argument Fingerprint constructor leaves the group id at
    // its field default, identifier setters mutate in place.
    let plain_name = text(&mut engine, "left thumb");
    let plain = p49_new(
        &mut engine,
        "Landroid/hardware/fingerprint/Fingerprint;",
        "(Ljava/lang/CharSequence;IJ)V",
        &[plain_name, Value::Int(5), Value::Long(6)],
    );
    assert_eq!(
        call(&mut engine, plain, "getGroupId", "()I", &[]),
        Value::Int(0)
    );
    let renamed = text(&mut engine, "renamed");
    call(
        &mut engine,
        plain,
        "setName",
        "(Ljava/lang/CharSequence;)V",
        std::slice::from_ref(&renamed),
    );
    call(
        &mut engine,
        plain,
        "setDeviceId",
        "(J)V",
        &[Value::Long(77)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            plain,
            "getName",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "renamed"
    );
    assert_eq!(
        call(&mut engine, plain, "getDeviceId", "()J", &[]),
        Value::Long(77)
    );
    // AOSP's CryptoObject constructors store the wrapper unchecked, so a
    // null cipher yields null typed getters and opId 0, not an exception.
    let null_crypto = p49_new(
        &mut engine,
        "Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;",
        "(Ljavax/crypto/Cipher;)V",
        &[Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            null_crypto,
            "getCipher",
            "()Ljavax/crypto/Cipher;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, null_crypto, "getOpId", "()J", &[]),
        Value::Long(0)
    );
}

#[test]
fn biometrics_absent_service_prompt_and_crypto_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "biometric");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/hardware/biometrics/BiometricManager;"
    );
    assert_eq!(
        call(&mut engine, manager, "canAuthenticate", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "canAuthenticate",
            "(I)I",
            &[Value::Int(0x800f)]
        ),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(manager, "getSensorProperties", "()Ljava/util/List;", &[]),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            "Landroid/hardware/biometrics/BiometricManager;",
            "authenticatorToStr",
            "(I)Ljava/lang/String;",
            &[Value::Int(0x000f)],
        ),
        "BIOMETRIC_STRONG"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            "Landroid/hardware/biometrics/BiometricManager;",
            "authenticatorToStr",
            "(I)Ljava/lang/String;",
            &[Value::Int(123)],
        ),
        "Unknown authenticator type: 123"
    );

    let strings = object(call(
        &mut engine,
        manager,
        "getStrings",
        "(I)Landroid/hardware/biometrics/BiometricManager$Strings;",
        &[Value::Int(0x00ff)],
    ));
    assert_throws(
        engine.invoke_instance(strings, "getButtonLabel", "()Ljava/lang/CharSequence;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    let context = instance(&mut engine, CONTEXT);
    let missing_title_builder = p49_new(
        &mut engine,
        "Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert_throws(
        engine.invoke_instance(
            missing_title_builder,
            "build",
            "()Landroid/hardware/biometrics/BiometricPrompt;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let builder = p49_new(
        &mut engine,
        "Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    let title = text(&mut engine, "Unlock");
    call(
        &mut engine,
        builder,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[title],
    );
    let subtitle = text(&mut engine, "Use your fingerprint");
    call(
        &mut engine,
        builder,
        "setSubtitle",
        "(Ljava/lang/CharSequence;)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[subtitle],
    );
    let negative = text(&mut engine, "Cancel");
    let executor = instance(&mut engine, "Ljava/util/concurrent/Executor;");
    let listener = instance(
        &mut engine,
        "Landroid/content/DialogInterface$OnClickListener;",
    );
    call(
        &mut engine,
        builder,
        "setNegativeButton",
        "(Ljava/lang/CharSequence;Ljava/util/concurrent/Executor;Landroid/content/DialogInterface$OnClickListener;)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[negative, Value::Obj(executor), Value::Obj(listener)],
    );
    call(
        &mut engine,
        builder,
        "setAllowedAuthenticators",
        "(I)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[Value::Int(0x000f)],
    );
    // Deep audit: PromptInfo.setAllowedSensorIds clears the current ids
    // before addAll dereferences the argument, so the null call throws
    // after emptying the list.
    let sensor_id = instance(&mut engine, "Ljava/lang/Integer;");
    let sensor_ids = list_of(&mut engine, &[Value::Obj(sensor_id)]);
    call(
        &mut engine,
        builder,
        "setAllowedSensorIds",
        "(Ljava/util/List;)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[Value::Obj(sensor_ids)],
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "setAllowedSensorIds",
            "(Ljava/util/List;)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        builder,
        "setConfirmationRequired",
        "(Z)Landroid/hardware/biometrics/BiometricPrompt$Builder;",
        &[Value::Int(0)],
    );
    let prompt = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/hardware/biometrics/BiometricPrompt;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            prompt,
            "getTitle",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Unlock"
    );
    assert_eq!(
        call_text(
            &mut engine,
            prompt,
            "getSubtitle",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Use your fingerprint"
    );
    assert_eq!(
        call(&mut engine, prompt, "getAllowedAuthenticators", "()I", &[]),
        Value::Int(0x000f)
    );
    assert_eq!(
        call(&mut engine, prompt, "isConfirmationRequired", "()Z", &[]),
        Value::Int(0)
    );
    let cleared_ids = call(
        &mut engine,
        prompt,
        "getAllowedSensorIds",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, cleared_ids), 0);

    let callback = p49_new(
        &mut engine,
        "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;",
        "()V",
        &[],
    );
    let authenticate_sig = "(Landroid/os/CancellationSignal;Ljava/util/concurrent/Executor;Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;)V";
    assert_throws(
        engine.invoke_instance(
            prompt,
            "authenticate",
            authenticate_sig,
            &[Value::Null, Value::Obj(executor), Value::Obj(callback)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let signal = instance(&mut engine, "Landroid/os/CancellationSignal;");
    assert_throws(
        engine.invoke_instance(
            prompt,
            "authenticate",
            authenticate_sig,
            &[
                Value::Obj(signal),
                Value::Obj(executor),
                Value::Obj(callback),
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        p49_field_or_null(&engine, callback, "rd_result"),
        Value::Null
    );

    let signature = instance(&mut engine, "Ljava/security/Signature;");
    let crypto = p49_new(
        &mut engine,
        "Landroid/hardware/biometrics/BiometricPrompt$CryptoObject;",
        "(Ljava/security/Signature;)V",
        &[Value::Obj(signature)],
    );
    assert_eq!(
        call(
            &mut engine,
            crypto,
            "getSignature",
            "()Ljava/security/Signature;",
            &[]
        ),
        Value::Obj(signature)
    );
    assert_eq!(
        call(
            &mut engine,
            crypto,
            "getCipher",
            "()Ljavax/crypto/Cipher;",
            &[]
        ),
        Value::Null
    );
    let result = p49_new(
        &mut engine,
        "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationResult;",
        "(Landroid/hardware/biometrics/BiometricPrompt$CryptoObject;I)V",
        &[Value::Obj(crypto), Value::Int(2)],
    );
    // Deep audit: the AOSP getter is getAuthenticationType (the
    // @AuthenticationResultType annotation is not a method), and the
    // BiometricPrompt result carries a null identifier and userId 0.
    assert_eq!(
        call(&mut engine, result, "getAuthenticationType", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(
            &mut engine,
            result,
            "getId",
            "()Landroid/hardware/biometrics/BiometricAuthenticator$Identifier;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, result, "getUserId", "()I", &[]),
        Value::Int(0)
    );
}

#[test]
fn hdmi_no_device_profile_and_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "hdmi_control");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/hardware/hdmi/HdmiControlManager;"
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getPlaybackClient",
            "()Landroid/hardware/hdmi/HdmiPlaybackClient;",
            &[]
        ),
        Value::Null
    );
    let connected = call(
        &mut engine,
        manager,
        "getConnectedDevices",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, connected), 0);
    let ports = call(
        &mut engine,
        manager,
        "getPortInfo",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, ports), 0);
    assert_eq!(
        call(&mut engine, manager, "getPhysicalAddress", "()I", &[]),
        Value::Int(0xffff)
    );

    let operand = byte_array(&mut engine, &[0x55]);
    let features = object(call_static(
        &mut engine,
        "Landroid/hardware/hdmi/DeviceFeatures;",
        "fromOperand",
        "([B)Landroid/hardware/hdmi/DeviceFeatures;",
        &[operand],
    ));
    for (method, expected) in [
        ("getRecordTvScreenSupport", 1),
        ("getSetOsdStringSupport", 0),
        ("getDeckControlSupport", 1),
        ("getSetAudioRateSupport", 0),
        ("getArcTxSupport", 1),
        ("getArcRxSupport", 0),
        ("getSetAudioVolumeLevelSupport", 1),
    ] {
        assert_eq!(
            call(&mut engine, features, method, "()I", &[]),
            Value::Int(expected)
        );
    }
    let round_trip = call(&mut engine, features, "toOperand", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &round_trip), vec![0x55]);

    let unknown_builder = p49_new(
        &mut engine,
        "Landroid/hardware/hdmi/DeviceFeatures$Builder;",
        "(I)V",
        &[Value::Int(2)],
    );
    let unknown = object(call(
        &mut engine,
        unknown_builder,
        "build",
        "()Landroid/hardware/hdmi/DeviceFeatures;",
        &[],
    ));
    let feature_builder = p49_new(
        &mut engine,
        "Landroid/hardware/hdmi/DeviceFeatures$Builder;",
        "(Landroid/hardware/hdmi/DeviceFeatures;)V",
        &[Value::Obj(features)],
    );
    call(
        &mut engine,
        feature_builder,
        "update",
        "(Landroid/hardware/hdmi/DeviceFeatures;)Landroid/hardware/hdmi/DeviceFeatures$Builder;",
        &[Value::Obj(unknown)],
    );
    let preserved = object(call(
        &mut engine,
        feature_builder,
        "build",
        "()Landroid/hardware/hdmi/DeviceFeatures;",
        &[],
    ));
    let preserved_operand = call(&mut engine, preserved, "toOperand", "()[B", &[]);
    assert_eq!(
        byte_array_values(&engine, &preserved_operand),
        vec![0x55],
        "update must ignore unknown (2) feature statuses"
    );

    let device_builder = p49_new(
        &mut engine,
        "Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;",
        "(I)V",
        &[Value::Int(0)],
    );
    call(
        &mut engine,
        device_builder,
        "setLogicalAddress",
        "(I)Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        device_builder,
        "setDeviceType",
        "(I)Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;",
        &[Value::Int(4)],
    );
    call(
        &mut engine,
        device_builder,
        "setVendorId",
        "(I)Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;",
        &[Value::Int(0x123456)],
    );
    let osd = text(&mut engine, "Player");
    call(
        &mut engine,
        device_builder,
        "setDisplayName",
        "(Ljava/lang/String;)Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;",
        &[osd],
    );
    let device = object(call(
        &mut engine,
        device_builder,
        "build",
        "()Landroid/hardware/hdmi/HdmiDeviceInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, device, "getDeviceType", "()I", &[]),
        Value::Int(4)
    );
    assert_eq!(
        call(&mut engine, device, "getLogicalAddress", "()I", &[]),
        Value::Int(5)
    );
    assert_eq!(
        call(&mut engine, device, "getId", "()I", &[]),
        Value::Int(5),
        "a CEC device ID is its logical address"
    );
    assert_eq!(
        call(&mut engine, device, "isSourceType", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, device, "isCecDevice", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, device, "getVendorId", "()I", &[]),
        Value::Int(0x123456)
    );
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getDisplayName",
            "()Ljava/lang/String;",
            &[]
        ),
        "Player"
    );

    let port = p49_new(
        &mut engine,
        "Landroid/hardware/hdmi/HdmiPortInfo$Builder;",
        "(III)V",
        &[Value::Int(3), Value::Int(1), Value::Int(0x1200)],
    );
    let port = object(call(
        &mut engine,
        port,
        "build",
        "()Landroid/hardware/hdmi/HdmiPortInfo;",
        &[],
    ));
    assert_eq!(call(&mut engine, port, "getId", "()I", &[]), Value::Int(3));
    assert_eq!(
        call(&mut engine, port, "getType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, port, "getAddress", "()I", &[]),
        Value::Int(0x1200)
    );

    let utils = "Landroid/hardware/hdmi/HdmiUtils;";
    assert_eq!(
        call_static(
            &mut engine,
            utils,
            "getLocalPortFromPhysicalAddress",
            "(II)I",
            &[Value::Int(0x1200), Value::Int(0x1000)],
        ),
        Value::Int(2)
    );
    assert_eq!(
        call_static(
            &mut engine,
            utils,
            "isValidPhysicalAddress",
            "(I)Z",
            &[Value::Int(0x1012)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            utils,
            "getHdmiAddressRelativePosition",
            "(II)I",
            &[Value::Int(0x1200), Value::Int(0x1200)],
        ),
        Value::Int(3)
    );
    assert_eq!(
        call_static(
            &mut engine,
            utils,
            "getHdmiAddressRelativePosition",
            "(II)I",
            &[Value::Int(0x1000), Value::Int(0x1200)],
        ),
        Value::Int(4)
    );
    assert_eq!(
        call_static(
            &mut engine,
            utils,
            "getHdmiAddressRelativePosition",
            "(II)I",
            &[Value::Int(0x1100), Value::Int(0x1200)],
        ),
        Value::Int(6)
    );

    // Deep audit: remaining relative-position branches (directly below,
    // below, above, different branch, and invalid/unassigned unknown).
    for (src, dest, expected) in [
        (0x1200, 0x1000, 1), // directly below
        (0x1230, 0x1000, 2), // below
        (0x1000, 0x1230, 5), // above
        (0x1110, 0x1210, 7), // different branch
        (0x1010, 0x1200, 0), // invalid physical address -> unknown
        (0xffff, 0x1200, 0), // unassigned -> unknown
    ] {
        assert_eq!(
            call_static(
                &mut engine,
                utils,
                "getHdmiAddressRelativePosition",
                "(II)I",
                &[Value::Int(src), Value::Int(dest)],
            ),
            Value::Int(expected),
            "relative position {src:#06x} vs {dest:#06x}"
        );
    }

    // Deep audit: an empty DeviceFeatures operand keeps every status at
    // FEATURE_SUPPORT_UNKNOWN (2) instead of clearing them.
    let empty_operand = byte_array(&mut engine, &[]);
    let unknown_features = object(call_static(
        &mut engine,
        "Landroid/hardware/hdmi/DeviceFeatures;",
        "fromOperand",
        "([B)Landroid/hardware/hdmi/DeviceFeatures;",
        &[empty_operand],
    ));
    assert_eq!(
        call(
            &mut engine,
            unknown_features,
            "getRecordTvScreenSupport",
            "()I",
            &[]
        ),
        Value::Int(2)
    );

    // Deep audit: HdmiPortInfo.Builder rejects an invalid port type and a
    // negative address in its constructor.
    let bad_type_builder = instance(&mut engine, "Landroid/hardware/hdmi/HdmiPortInfo$Builder;");
    assert_throws(
        engine.invoke_instance(
            bad_type_builder,
            "<init>",
            "(III)V",
            &[Value::Int(1), Value::Int(7), Value::Int(0x1000)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let bad_address_builder = instance(&mut engine, "Landroid/hardware/hdmi/HdmiPortInfo$Builder;");
    assert_throws(
        engine.invoke_instance(
            bad_address_builder,
            "<init>",
            "(III)V",
            &[Value::Int(1), Value::Int(0), Value::Int(-1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: isDeviceConnected null-checks its argument before the
    // invalid-local-address early return of this profile.
    assert_throws(
        engine.invoke_instance(
            manager,
            "isDeviceConnected",
            "(Landroid/hardware/hdmi/HdmiDeviceInfo;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn input_no_device_profile_and_device_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "input");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/hardware/input/InputManager;"
    );
    let devices = call(&mut engine, manager, "getInputDeviceIds", "()[I", &[]);
    assert!(array_values(&engine, &devices).is_empty());
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getInputDevice",
            "(I)Landroid/view/InputDevice;",
            &[Value::Int(-1)]
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/view/InputDevice;",
            "getDevice",
            "(I)Landroid/view/InputDevice;",
            &[Value::Int(1)],
        ),
        Value::Null
    );
    let static_ids = call_static(
        &mut engine,
        "Landroid/view/InputDevice;",
        "getDeviceIds",
        "()[I",
        &[],
    );
    assert!(array_values(&engine, &static_ids).is_empty());
    let keys = int_array(&mut engine, &[4, 82]);
    let has_keys = call(&mut engine, manager, "deviceHasKeys", "([I)[Z", &[keys]);
    assert_eq!(
        array_values(&engine, &has_keys),
        vec![Value::Int(0), Value::Int(0)]
    );

    let listener = instance(
        &mut engine,
        "Landroid/hardware/input/InputManager$InputDeviceListener;",
    );
    let register_sig =
        "(Landroid/hardware/input/InputManager$InputDeviceListener;Landroid/os/Handler;)V";
    assert_throws(
        engine.invoke_instance(
            manager,
            "registerInputDeviceListener",
            register_sig,
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        manager,
        "registerInputDeviceListener",
        register_sig,
        &[Value::Obj(listener), Value::Null],
    );
    call(
        &mut engine,
        manager,
        "registerInputDeviceListener",
        register_sig,
        &[Value::Obj(listener), Value::Null],
    );
    let stored = field(&engine, manager, "rd_input_listeners");
    assert_eq!(p57_collection_size(&mut engine, stored), 1);
    let unregister_sig = "(Landroid/hardware/input/InputManager$InputDeviceListener;)V";
    assert_throws(
        engine.invoke_instance(
            manager,
            "unregisterInputDeviceListener",
            unregister_sig,
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        manager,
        "unregisterInputDeviceListener",
        unregister_sig,
        &[Value::Obj(listener)],
    );
    let stored = field(&engine, manager, "rd_input_listeners");
    assert_eq!(p57_collection_size(&mut engine, stored), 0);

    let identifier_descriptor = text(&mut engine, "descriptor-1");
    let identifier = p49_new(
        &mut engine,
        "Landroid/hardware/input/InputDeviceIdentifier;",
        "(Ljava/lang/String;II)V",
        &[identifier_descriptor, Value::Int(11), Value::Int(22)],
    );
    let builder = p49_new(
        &mut engine,
        "Landroid/view/InputDevice$Builder;",
        "()V",
        &[],
    );
    let builder_class = "Landroid/view/InputDevice$Builder;";
    for (method, sig, argument) in [
        (
            "setId",
            "(I)Landroid/view/InputDevice$Builder;",
            Value::Int(-1),
        ),
        (
            "setGeneration",
            "(I)Landroid/view/InputDevice$Builder;",
            Value::Int(4),
        ),
        (
            "setSources",
            "(I)Landroid/view/InputDevice$Builder;",
            Value::Int(0x101),
        ),
        (
            "setKeyboardType",
            "(I)Landroid/view/InputDevice$Builder;",
            Value::Int(2),
        ),
    ] {
        call(&mut engine, builder, method, sig, &[argument]);
        let _ = builder_class;
    }
    assert_eq!(
        call_text(
            &mut engine,
            identifier,
            "getDescriptor",
            "()Ljava/lang/String;",
            &[]
        ),
        "descriptor-1"
    );
    assert_eq!(
        call(&mut engine, identifier, "getVendorId", "()I", &[]),
        Value::Int(11)
    );
    assert_eq!(
        call(&mut engine, identifier, "getProductId", "()I", &[]),
        Value::Int(22)
    );
    let name = text(&mut engine, "Virtual keyboard");
    call(
        &mut engine,
        builder,
        "setName",
        "(Ljava/lang/String;)Landroid/view/InputDevice$Builder;",
        &[name],
    );
    call(
        &mut engine,
        builder,
        "setVendorId",
        "(I)Landroid/view/InputDevice$Builder;",
        &[Value::Int(11)],
    );
    call(
        &mut engine,
        builder,
        "setProductId",
        "(I)Landroid/view/InputDevice$Builder;",
        &[Value::Int(22)],
    );
    let descriptor = text(&mut engine, "descriptor-1");
    call(
        &mut engine,
        builder,
        "setDescriptor",
        "(Ljava/lang/String;)Landroid/view/InputDevice$Builder;",
        &[descriptor],
    );
    let language = text(&mut engine, "pt");
    call(
        &mut engine,
        builder,
        "setKeyboardLanguageTag",
        "(Ljava/lang/String;)Landroid/view/InputDevice$Builder;",
        &[language],
    );
    let layout = text(&mut engine, "abnt2");
    call(
        &mut engine,
        builder,
        "setKeyboardLayoutType",
        "(Ljava/lang/String;)Landroid/view/InputDevice$Builder;",
        &[layout],
    );
    call(
        &mut engine,
        builder,
        "addMotionRange",
        "(IIFFFFF)Landroid/view/InputDevice$Builder;",
        &[
            Value::Int(0),
            Value::Int(0x1002),
            Value::Float(0.0),
            Value::Float(1080.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(1.0),
        ],
    );
    let device = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/view/InputDevice;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, device, "getId", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, device, "isVirtual", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(&mut engine, device, "getName", "()Ljava/lang/String;", &[]),
        "Virtual keyboard"
    );
    assert_eq!(
        call(&mut engine, device, "getVendorId", "()I", &[]),
        Value::Int(11)
    );
    assert_eq!(
        call(&mut engine, device, "getProductId", "()I", &[]),
        Value::Int(22)
    );
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getDescriptor",
            "()Ljava/lang/String;",
            &[]
        ),
        "descriptor-1"
    );
    assert_eq!(
        call(
            &mut engine,
            device,
            "supportsSource",
            "(I)Z",
            &[Value::Int(0x100)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, device, "isFullKeyboard", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getKeyboardLanguageTag",
            "()Ljava/lang/String;",
            &[]
        ),
        "pt"
    );
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getKeyboardLayoutType",
            "()Ljava/lang/String;",
            &[]
        ),
        "abnt2"
    );
    let ranges = call(
        &mut engine,
        device,
        "getMotionRanges",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, ranges.clone()), 1);
    let range = object(p49_list_get(&mut engine, ranges, 0));
    assert_eq!(
        call(&mut engine, range, "getRange", "()F", &[]),
        Value::Float(1080.0)
    );
    assert_eq!(
        call(
            &mut engine,
            range,
            "isFromSource",
            "(I)Z",
            &[Value::Int(0x1002)]
        ),
        Value::Int(1)
    );
    let context = instance(&mut engine, CONTEXT);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "areTouchpadGesturesAvailable",
            "(Landroid/content/Context;)Z",
            &[Value::Obj(context)],
        ),
        Value::Int(1)
    );

    // Deep audit: getMotionRange(axis, source) compares the source exactly
    // (unlike MotionRange.isFromSource, which is a mask test).
    assert_eq!(
        call(
            &mut engine,
            device,
            "getMotionRange",
            "(II)Landroid/view/InputDevice$MotionRange;",
            &[Value::Int(0), Value::Int(0x2)]
        ),
        Value::Null
    );
    let by_axis_source = object(call(
        &mut engine,
        device,
        "getMotionRange",
        "(II)Landroid/view/InputDevice$MotionRange;",
        &[Value::Int(0), Value::Int(0x1002)],
    ));
    assert_eq!(
        call(&mut engine, by_axis_source, "getMax", "()F", &[]),
        Value::Float(1080.0)
    );

    // Deep audit: getKeyboardLayout rejects a null descriptor with IAE.
    assert_throws(
        engine.invoke_instance(
            manager,
            "getKeyboardLayout",
            "(Ljava/lang/String;)Landroid/hardware/input/KeyboardLayout;",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
}

#[test]
fn soundtrigger_models_and_metadata_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let uuid = p49_uuid(&mut engine, "123e4567-e89b-12d3-a456-426614174000");
    let vendor_uuid = p49_uuid(&mut engine, "123e4567-e89b-12d3-a456-426614174999");
    let generic_class = "Landroid/hardware/soundtrigger/SoundTrigger$GenericSoundModel;";

    let generic = p49_new(
        &mut engine,
        generic_class,
        "(Ljava/util/UUID;Ljava/util/UUID;[B)V",
        &[uuid.clone(), Value::Null, Value::Null],
    );
    assert_eq!(
        call(&mut engine, generic, "getType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, generic, "getVersion", "()I", &[]),
        Value::Int(-1)
    );
    let data = call(&mut engine, generic, "getData", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &data), Vec::<u8>::new());
    let normalized_vendor = object(call(
        &mut engine,
        generic,
        "getVendorUuid",
        "()Ljava/util/UUID;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            normalized_vendor,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "00000000-0000-0000-0000-000000000000"
    );

    let generic_data = byte_array(&mut engine, &[1, 2, 3]);
    let generic_versioned = p49_new(
        &mut engine,
        generic_class,
        "(Ljava/util/UUID;Ljava/util/UUID;[BI)V",
        &[
            uuid.clone(),
            vendor_uuid.clone(),
            generic_data,
            Value::Int(7),
        ],
    );
    let generic_data_copy = byte_array(&mut engine, &[1, 2, 3]);
    let generic_equal = p49_new(
        &mut engine,
        generic_class,
        "(Ljava/util/UUID;Ljava/util/UUID;[BI)V",
        &[
            uuid.clone(),
            vendor_uuid.clone(),
            generic_data_copy,
            Value::Int(7),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            generic_versioned,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(generic_equal)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, generic_versioned, "hashCode", "()I", &[]),
        call(&mut engine, generic_equal, "hashCode", "()I", &[])
    );

    let locale = p49_locale(&mut engine, "pt");
    let phrase = text(&mut engine, "Ok Rusty");
    let keyphrase_class = "Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;";
    let bad_keyphrase = instance(&mut engine, keyphrase_class);
    assert_throws(
        engine.invoke_instance(
            bad_keyphrase,
            "<init>",
            "(IILjava/util/Locale;Ljava/lang/String;[I)V",
            &[
                Value::Int(1),
                Value::Int(1),
                Value::Null,
                phrase.clone(),
                Value::Null,
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let keyphrase = p49_new(
        &mut engine,
        keyphrase_class,
        "(IILjava/util/Locale;Ljava/lang/String;[I)V",
        &[
            Value::Int(4),
            Value::Int(5),
            Value::Obj(locale),
            phrase.clone(),
            Value::Null,
        ],
    );
    let users = call(&mut engine, keyphrase, "getUsers", "()[I", &[]);
    assert!(array_values(&engine, &users).is_empty());
    let phrase_copy = text(&mut engine, "Ok Rusty");
    let keyphrase_equal = p49_new(
        &mut engine,
        keyphrase_class,
        "(IILjava/util/Locale;Ljava/lang/String;[I)V",
        &[
            Value::Int(4),
            Value::Int(5),
            Value::Obj(locale),
            phrase_copy,
            Value::Null,
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            keyphrase,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(keyphrase_equal)]
        ),
        Value::Int(1)
    );

    let keyphrases = object_array(
        &mut engine,
        "[Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;",
        vec![Value::Obj(keyphrase)],
    );
    let keyphrases_equal = object_array(
        &mut engine,
        "[Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;",
        vec![Value::Obj(keyphrase_equal)],
    );
    let model_data = byte_array(&mut engine, &[9]);
    let key_model_class = "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseSoundModel;";
    let key_model_sig = "(Ljava/util/UUID;Ljava/util/UUID;[B[Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;)V";
    let key_model = p49_new(
        &mut engine,
        key_model_class,
        key_model_sig,
        &[uuid.clone(), vendor_uuid.clone(), model_data, keyphrases],
    );
    let model_data_copy = byte_array(&mut engine, &[9]);
    let key_model_equal = p49_new(
        &mut engine,
        key_model_class,
        key_model_sig,
        &[
            uuid.clone(),
            vendor_uuid.clone(),
            model_data_copy,
            keyphrases_equal,
        ],
    );
    assert_eq!(
        call(&mut engine, key_model, "getType", "()I", &[]),
        Value::Int(0)
    );
    let returned_keyphrases = call(
        &mut engine,
        key_model,
        "getKeyphrases",
        "()[Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;",
        &[],
    );
    assert_eq!(
        array_values(&engine, &returned_keyphrases),
        vec![Value::Obj(keyphrase)]
    );
    assert_eq!(
        call(
            &mut engine,
            key_model,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(key_model_equal)]
        ),
        Value::Int(1)
    );

    let range = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$ModelParamRange;",
        "(II)V",
        &[Value::Int(-2), Value::Int(9)],
    );
    let range_equal = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$ModelParamRange;",
        "(II)V",
        &[Value::Int(-2), Value::Int(9)],
    );
    assert_eq!(
        call(&mut engine, range, "getStart", "()I", &[]),
        Value::Int(-2)
    );
    assert_eq!(
        call(&mut engine, range, "getEnd", "()I", &[]),
        Value::Int(9)
    );
    assert_eq!(
        call(
            &mut engine,
            range,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(range_equal)]
        ),
        Value::Int(1)
    );

    let confidence = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$ConfidenceLevel;",
        "(II)V",
        &[Value::Int(7), Value::Int(80)],
    );
    assert_eq!(field(&engine, confidence, "userId"), Value::Int(7));
    assert_eq!(
        field(&engine, confidence, "confidenceLevel"),
        Value::Int(80)
    );
    let levels = object_array(
        &mut engine,
        "[Landroid/hardware/soundtrigger/SoundTrigger$ConfidenceLevel;",
        vec![Value::Obj(confidence)],
    );
    let extra = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseRecognitionExtra;",
        "(III[Landroid/hardware/soundtrigger/SoundTrigger$ConfidenceLevel;)V",
        &[Value::Int(3), Value::Int(5), Value::Int(66), levels],
    );
    let returned_levels = call(
        &mut engine,
        extra,
        "getConfidenceLevels",
        "()Ljava/util/Collection;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, returned_levels.clone()), 1);
    assert_eq!(
        p49_list_get(&mut engine, returned_levels, 0),
        Value::Obj(confidence)
    );
    let extras = object_array(
        &mut engine,
        "[Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseRecognitionExtra;",
        vec![Value::Obj(extra)],
    );
    let config_data = byte_array(&mut engine, &[4, 5]);
    let config = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$RecognitionConfig;",
        "(ZZ[Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseRecognitionExtra;[BI)V",
        &[
            Value::Int(1),
            Value::Int(0),
            extras,
            config_data,
            Value::Int(3),
        ],
    );
    assert_eq!(field(&engine, config, "captureRequested"), Value::Int(1));
    assert_eq!(
        field(&engine, config, "allowMultipleTriggers"),
        Value::Int(0)
    );
    assert_eq!(field(&engine, config, "audioCapabilities"), Value::Int(3));

    let locales = list_of(&mut engine, &[Value::Obj(locale)]);
    let metadata_phrase = text(&mut engine, "Ok Rusty");
    let metadata = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/KeyphraseMetadata;",
        "(ILjava/lang/String;Ljava/util/Set;I)V",
        &[
            Value::Int(9),
            metadata_phrase,
            Value::Obj(locales),
            Value::Int(5),
        ],
    );
    let lower_phrase = text(&mut engine, "ok rusty");
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "supportsPhrase",
            "(Ljava/lang/String;)Z",
            &[lower_phrase]
        ),
        Value::Int(1)
    );
    let english = p49_locale(&mut engine, "en");
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "supportsLocale",
            "(Ljava/util/Locale;)Z",
            &[Value::Obj(english)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "supportsLocale",
            "(Ljava/util/Locale;)Z",
            &[Value::Obj(locale)]
        ),
        Value::Int(1)
    );

    // Deep audit: Keyphrase requireNonNulls its locale and text, while a
    // null users array normalizes to an empty array.
    let keyphrase_class = "Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;";
    let keyphrase_sig = "(IILjava/util/Locale;Ljava/lang/String;[I)V";
    let phrase_text = text(&mut engine, "Ok Rusty");
    let null_locale_phrase = instance(&mut engine, keyphrase_class);
    assert_throws(
        engine.invoke_instance(
            null_locale_phrase,
            "<init>",
            keyphrase_sig,
            &[
                Value::Int(1),
                Value::Int(1),
                Value::Null,
                phrase_text.clone(),
                Value::Null,
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let null_text_phrase = instance(&mut engine, keyphrase_class);
    assert_throws(
        engine.invoke_instance(
            null_text_phrase,
            "<init>",
            keyphrase_sig,
            &[
                Value::Int(1),
                Value::Int(1),
                Value::Obj(locale),
                Value::Null,
                Value::Null,
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let no_users_phrase = p49_new(
        &mut engine,
        keyphrase_class,
        keyphrase_sig,
        &[
            Value::Int(1),
            Value::Int(1),
            Value::Obj(locale),
            phrase_text,
            Value::Null,
        ],
    );
    let users = call(&mut engine, no_users_phrase, "getUsers", "()[I", &[]);
    assert_eq!(int_values(&engine, &users), Vec::<i32>::new());

    // Deep audit: SoundModel.equals uses instanceof, so a plain SoundModel
    // can equal a KeyphraseSoundModel with the same base fields while the
    // reverse comparison is false (asymmetric, as in AOSP).
    let base_model = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$SoundModel;",
        "(Ljava/util/UUID;Ljava/util/UUID;I[BI)V",
        &[
            uuid.clone(),
            vendor_uuid.clone(),
            Value::Int(0),
            Value::Null,
            Value::Int(-1),
        ],
    );
    let keyphrase_model = p49_new(
        &mut engine,
        "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseSoundModel;",
        "(Ljava/util/UUID;Ljava/util/UUID;[B[Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;)V",
        &[uuid.clone(), vendor_uuid, Value::Null, Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            base_model,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(keyphrase_model)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            keyphrase_model,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(base_model)]
        ),
        Value::Int(0)
    );
}

#[test]
fn midi_no_device_profile_values_and_receiver_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "midi");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/media/midi/MidiManager;"
    );
    let devices = call(
        &mut engine,
        manager,
        "getDevices",
        "()[Landroid/media/midi/MidiDeviceInfo;",
        &[],
    );
    assert!(array_values(&engine, &devices).is_empty());
    let transport_devices = call(
        &mut engine,
        manager,
        "getDevicesForTransport",
        "(I)Ljava/util/Set;",
        &[Value::Int(0)],
    );
    assert_eq!(p57_collection_size(&mut engine, transport_devices), 0);

    let callback = p49_new(
        &mut engine,
        "Landroid/media/midi/MidiManager$DeviceCallback;",
        "()V",
        &[],
    );
    let register_sig = "(Landroid/media/midi/MidiManager$DeviceCallback;Landroid/os/Handler;)V";
    assert_throws(
        engine.invoke_instance(
            manager,
            "registerDeviceCallback",
            register_sig,
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        manager,
        "registerDeviceCallback",
        register_sig,
        &[Value::Obj(callback), Value::Null],
    );
    call(
        &mut engine,
        manager,
        "registerDeviceCallback",
        register_sig,
        &[Value::Obj(callback), Value::Null],
    );
    let stored = field(&engine, manager, "rd_device_callbacks");
    assert_eq!(p57_collection_size(&mut engine, stored), 1);
    call(
        &mut engine,
        manager,
        "unregisterDeviceCallback",
        "(Landroid/media/midi/MidiManager$DeviceCallback;)V",
        &[Value::Obj(callback)],
    );
    let stored = field(&engine, manager, "rd_device_callbacks");
    assert_eq!(p57_collection_size(&mut engine, stored), 0);
    // AOSP stores listeners in a ConcurrentHashMap keyed by callback, so a
    // null callback fails on both unregister (remove) and the transport
    // overload's Objects.requireNonNull(executor) fires before anything else.
    assert_throws(
        engine.invoke_instance(
            manager,
            "unregisterDeviceCallback",
            "(Landroid/media/midi/MidiManager$DeviceCallback;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "registerDeviceCallback",
            "(ILjava/util/concurrent/Executor;Landroid/media/midi/MidiManager$DeviceCallback;)V",
            &[Value::Int(1), Value::Null, Value::Obj(callback)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let bundle = p49_new(&mut engine, "Landroid/os/Bundle;", "()V", &[]);
    let input_names = p57_string_array(&mut engine, &["in-a", "in-b"]);
    let output_names = p57_string_array(&mut engine, &["out-a"]);
    let info_class = "Landroid/media/midi/MidiDeviceInfo;";
    let info_sig = "(IIII[Ljava/lang/String;[Ljava/lang/String;Landroid/os/Bundle;ZI)V";
    let bad_info = instance(&mut engine, info_class);
    assert_throws(
        engine.invoke_instance(
            bad_info,
            "<init>",
            info_sig,
            &[
                Value::Int(1),
                Value::Int(1),
                Value::Int(257),
                Value::Int(0),
                Value::Null,
                Value::Null,
                Value::Obj(bundle),
                Value::Int(0),
                Value::Int(0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let info = p49_new(
        &mut engine,
        info_class,
        info_sig,
        &[
            Value::Int(2),
            Value::Int(42),
            Value::Int(2),
            Value::Int(1),
            input_names,
            output_names,
            Value::Obj(bundle),
            Value::Int(1),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(&mut engine, info, "getType", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(call(&mut engine, info, "getId", "()I", &[]), Value::Int(42));
    assert_eq!(
        call(&mut engine, info, "isPrivate", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, info, "getDefaultProtocol", "()I", &[]),
        Value::Int(1)
    );
    let ports = call(
        &mut engine,
        info,
        "getPorts",
        "()[Landroid/media/midi/MidiDeviceInfo$PortInfo;",
        &[],
    );
    let port_values = array_values(&engine, &ports);
    assert_eq!(port_values.len(), 3);
    let first_port = object(port_values[0].clone());
    assert_eq!(
        call(&mut engine, first_port, "getType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            first_port,
            "getName",
            "()Ljava/lang/String;",
            &[]
        ),
        "in-a"
    );
    let output_port = object(port_values[2].clone());
    assert_eq!(
        call(&mut engine, output_port, "getType", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, output_port, "getPortNumber", "()I", &[]),
        Value::Int(0)
    );
    let same_id = p49_new(
        &mut engine,
        info_class,
        info_sig,
        &[
            Value::Int(9),
            Value::Int(42),
            Value::Int(0),
            Value::Int(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Int(0),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            info,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_id)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, info, "hashCode", "()I", &[]),
        Value::Int(42)
    );
    assert!(
        call_text(&mut engine, info, "toString", "()Ljava/lang/String;", &[])
            .starts_with("MidiDeviceInfo[mType=2,mInputPortCount=2,mOutputPortCount=1,")
    );
    // AOSP's toString first calls mProperties.getString(...) to force
    // unparcelling, so a device info built with null properties fails
    // instead of rendering.
    assert_throws(
        engine.invoke_instance(same_id, "toString", "()Ljava/lang/String;", &[]),
        "Ljava/lang/NullPointerException;",
    );
    // getPorts indexes the caller-supplied names arrays directly, so a
    // names array shorter than the declared port count is out of bounds.
    let short_input_names = p57_string_array(&mut engine, &["only-one"]);
    let short_names_info = p49_new(
        &mut engine,
        info_class,
        info_sig,
        &[
            Value::Int(1),
            Value::Int(7),
            Value::Int(2),
            Value::Int(0),
            short_input_names,
            Value::Null,
            Value::Obj(bundle),
            Value::Int(0),
            Value::Int(0),
        ],
    );
    assert_throws(
        engine.invoke_instance(
            short_names_info,
            "getPorts",
            "()[Landroid/media/midi/MidiDeviceInfo$PortInfo;",
            &[],
        ),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );

    call(
        &mut engine,
        callback,
        "onDeviceAdded",
        "(Landroid/media/midi/MidiDeviceInfo;)V",
        &[Value::Obj(info)],
    );
    assert_eq!(field(&engine, callback, "rd_last_added"), Value::Obj(info));

    let receiver = p49_new(
        &mut engine,
        "Landroid/media/midi/MidiReceiver;",
        "(I)V",
        &[Value::Int(2)],
    );
    assert_eq!(
        call(&mut engine, receiver, "getMaxMessageSize", "()I", &[]),
        Value::Int(2)
    );
    let message = byte_array(&mut engine, &[0x90, 1, 2, 3, 4]);
    call(
        &mut engine,
        receiver,
        "send",
        "([BIIJ)V",
        &[message, Value::Int(1), Value::Int(3), Value::Long(99)],
    );
    assert_eq!(field(&engine, receiver, "rd_send_calls"), Value::Int(2));
    let received = field(&engine, receiver, "rd_received");
    assert_eq!(byte_array_values(&engine, &received), vec![1, 2, 3]);
    assert_eq!(field(&engine, receiver, "rd_last_offset"), Value::Int(3));
    assert_eq!(field(&engine, receiver, "rd_last_count"), Value::Int(1));
    assert_eq!(
        field(&engine, receiver, "rd_last_timestamp"),
        Value::Long(99)
    );
    // A zero-count send never enters AOSP's chunking loop.
    let empty_message = byte_array(&mut engine, &[0x80]);
    call(
        &mut engine,
        receiver,
        "send",
        "([BIIJ)V",
        &[empty_message, Value::Int(0), Value::Int(0), Value::Long(7)],
    );
    assert_eq!(field(&engine, receiver, "rd_send_calls"), Value::Int(2));
    call(&mut engine, receiver, "flush", "()V", &[]);
    assert_eq!(field(&engine, receiver, "rd_flushed"), Value::Int(1));
}

#[test]
fn tv_no_input_profile_ratings_and_tracks_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = p57_system_service(&mut engine, "tv_input");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/media/tv/TvInputManager;"
    );
    let inputs = call(
        &mut engine,
        manager,
        "getTvInputList",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, inputs), 0);
    assert_throws(
        engine.invoke_instance(
            manager,
            "getTvInputInfo",
            "(Ljava/lang/String;)Landroid/media/tv/TvInputInfo;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let input_id = text(&mut engine, "input-1");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getTvInputInfo",
            "(Ljava/lang/String;)Landroid/media/tv/TvInputInfo;",
            std::slice::from_ref(&input_id)
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getInputState",
            "(Ljava/lang/String;)I",
            &[input_id]
        ),
        Value::Int(2)
    );
    let systems = call(
        &mut engine,
        manager,
        "getTvContentRatingSystemList",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, systems), 0);

    let callback = p49_new(
        &mut engine,
        "Landroid/media/tv/TvInputManager$TvInputCallback;",
        "()V",
        &[],
    );
    let handler = instance(&mut engine, "Landroid/os/Handler;");
    let register_sig = "(Landroid/media/tv/TvInputManager$TvInputCallback;Landroid/os/Handler;)V";
    call(
        &mut engine,
        manager,
        "registerCallback",
        register_sig,
        &[Value::Obj(callback), Value::Obj(handler)],
    );
    call(
        &mut engine,
        manager,
        "registerCallback",
        register_sig,
        &[Value::Obj(callback), Value::Obj(handler)],
    );
    let callbacks = p49_field_or_null(&engine, manager, "rd_callbacks");
    assert_eq!(p57_collection_size(&mut engine, callbacks), 2);
    call(
        &mut engine,
        manager,
        "unregisterCallback",
        "(Landroid/media/tv/TvInputManager$TvInputCallback;)V",
        &[Value::Obj(callback)],
    );
    let callbacks = field(&engine, manager, "rd_callbacks");
    assert_eq!(p57_collection_size(&mut engine, callbacks), 1);

    let rating_class = "Landroid/media/tv/TvContentRating;";
    let create_sig = "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)Landroid/media/tv/TvContentRating;";
    let empty = text(&mut engine, "");
    let domain = text(&mut engine, "example.org");
    let system = text(&mut engine, "US");
    let main_rating = text(&mut engine, "TV-14");
    assert_throws(
        engine.invoke_static(
            rating_class,
            "createRating",
            create_sig,
            &[empty, system.clone(), main_rating.clone(), Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let subratings = p57_string_array(&mut engine, &["V", "FV"]);
    let candidate = object(call_static(
        &mut engine,
        rating_class,
        "createRating",
        create_sig,
        &[
            domain.clone(),
            system.clone(),
            main_rating.clone(),
            subratings.clone(),
        ],
    ));
    assert_eq!(p57_string_values(&engine, &subratings), ["FV", "V"]);
    assert_eq!(
        call_text(
            &mut engine,
            candidate,
            "flattenToString",
            "()Ljava/lang/String;",
            &[]
        ),
        "example.org/US/TV-14/FV/V"
    );
    let subrating_list = call(
        &mut engine,
        candidate,
        "getSubRatings",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, subrating_list.clone()), 2);
    let first_subrating = p49_list_get(&mut engine, subrating_list, 0);
    assert_eq!(text_of(&engine, &first_subrating), "FV");

    let no_subratings = p57_string_array(&mut engine, &[]);
    let base = object(call_static(
        &mut engine,
        rating_class,
        "createRating",
        create_sig,
        &[domain, system, main_rating, no_subratings],
    ));
    assert_eq!(
        call(
            &mut engine,
            base,
            "getSubRatings",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            candidate,
            "contains",
            "(Landroid/media/tv/TvContentRating;)Z",
            &[Value::Obj(base)]
        ),
        Value::Int(1)
    );

    call(
        &mut engine,
        manager,
        "addBlockedRating",
        "(Landroid/media/tv/TvContentRating;)V",
        &[Value::Obj(base)],
    );
    call(
        &mut engine,
        manager,
        "addBlockedRating",
        "(Landroid/media/tv/TvContentRating;)V",
        &[Value::Obj(base)],
    );
    let blocked = call(
        &mut engine,
        manager,
        "getBlockedRatings",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, blocked.clone()), 1);
    let blocked_first = object(p49_list_get(&mut engine, blocked, 0));
    assert_ne!(blocked_first, base);
    assert_eq!(
        call(
            &mut engine,
            blocked_first,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(base)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isRatingBlocked",
            "(Landroid/media/tv/TvContentRating;)Z",
            &[Value::Obj(candidate)]
        ),
        Value::Int(1)
    );
    call(
        &mut engine,
        manager,
        "removeBlockedRating",
        "(Landroid/media/tv/TvContentRating;)V",
        &[Value::Obj(base)],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isRatingBlocked",
            "(Landroid/media/tv/TvContentRating;)Z",
            &[Value::Obj(candidate)]
        ),
        Value::Int(0)
    );
    let flattened = text(&mut engine, "example.org/US/TV-14/FV/V");
    let round_trip = object(call_static(
        &mut engine,
        rating_class,
        "unflattenFromString",
        "(Ljava/lang/String;)Landroid/media/tv/TvContentRating;",
        &[flattened],
    ));
    assert_eq!(
        call(
            &mut engine,
            round_trip,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(candidate)]
        ),
        Value::Int(1)
    );

    assert_eq!(
        call(
            &mut engine,
            manager,
            "isParentalControlsEnabled",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        manager,
        "setParentalControlsEnabled",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isParentalControlsEnabled",
            "()Z",
            &[]
        ),
        Value::Int(1)
    );

    let builder_class = "Landroid/media/tv/TvTrackInfo$Builder;";
    let bad_builder = instance(&mut engine, builder_class);
    assert_throws(
        engine.invoke_instance(
            bad_builder,
            "<init>",
            "(ILjava/lang/String;)V",
            &[Value::Int(3), Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let audio_id = text(&mut engine, "audio-1");
    let audio_builder = p49_new(
        &mut engine,
        builder_class,
        "(ILjava/lang/String;)V",
        &[Value::Int(0), audio_id],
    );
    let language = text(&mut engine, "pt");
    call(
        &mut engine,
        audio_builder,
        "setLanguage",
        "(Ljava/lang/String;)Landroid/media/tv/TvTrackInfo$Builder;",
        &[language],
    );
    let description = text(&mut engine, "Portuguese");
    call(
        &mut engine,
        audio_builder,
        "setDescription",
        "(Ljava/lang/CharSequence;)Landroid/media/tv/TvTrackInfo$Builder;",
        &[description],
    );
    call(
        &mut engine,
        audio_builder,
        "setAudioChannelCount",
        "(I)Landroid/media/tv/TvTrackInfo$Builder;",
        &[Value::Int(2)],
    );
    call(
        &mut engine,
        audio_builder,
        "setAudioSampleRate",
        "(I)Landroid/media/tv/TvTrackInfo$Builder;",
        &[Value::Int(48_000)],
    );
    assert_throws(
        engine.invoke_instance(
            audio_builder,
            "setVideoWidth",
            "(I)Landroid/media/tv/TvTrackInfo$Builder;",
            &[Value::Int(1920)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let extra = p49_new(&mut engine, "Landroid/os/Bundle;", "()V", &[]);
    call(
        &mut engine,
        audio_builder,
        "setExtra",
        "(Landroid/os/Bundle;)Landroid/media/tv/TvTrackInfo$Builder;",
        &[Value::Obj(extra)],
    );
    let audio = object(call(
        &mut engine,
        audio_builder,
        "build",
        "()Landroid/media/tv/TvTrackInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, audio, "getType", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            audio,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        "pt"
    );
    assert_eq!(
        call(&mut engine, audio, "getAudioChannelCount", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, audio, "getAudioSampleRate", "()I", &[]),
        Value::Int(48_000)
    );
    let copied_extra = object(call(
        &mut engine,
        audio,
        "getExtra",
        "()Landroid/os/Bundle;",
        &[],
    ));
    assert_ne!(copied_extra, extra);

    let audio_id_copy = text(&mut engine, "audio-1");
    let audio_builder_copy = p49_new(
        &mut engine,
        builder_class,
        "(ILjava/lang/String;)V",
        &[Value::Int(0), audio_id_copy],
    );
    let language_copy = text(&mut engine, "pt");
    call(
        &mut engine,
        audio_builder_copy,
        "setLanguage",
        "(Ljava/lang/String;)Landroid/media/tv/TvTrackInfo$Builder;",
        &[language_copy],
    );
    let description_copy = text(&mut engine, "Portuguese");
    call(
        &mut engine,
        audio_builder_copy,
        "setDescription",
        "(Ljava/lang/CharSequence;)Landroid/media/tv/TvTrackInfo$Builder;",
        &[description_copy],
    );
    call(
        &mut engine,
        audio_builder_copy,
        "setAudioChannelCount",
        "(I)Landroid/media/tv/TvTrackInfo$Builder;",
        &[Value::Int(2)],
    );
    call(
        &mut engine,
        audio_builder_copy,
        "setAudioSampleRate",
        "(I)Landroid/media/tv/TvTrackInfo$Builder;",
        &[Value::Int(48_000)],
    );
    let audio_equal = object(call(
        &mut engine,
        audio_builder_copy,
        "build",
        "()Landroid/media/tv/TvTrackInfo;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            audio,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(audio_equal)]
        ),
        Value::Int(1),
        "TvTrackInfo equality ignores mExtra"
    );
    assert_eq!(
        call(&mut engine, audio, "hashCode", "()I", &[]),
        call(&mut engine, audio_equal, "hashCode", "()I", &[])
    );

    let video_id = text(&mut engine, "video-1");
    let video_builder = p49_new(
        &mut engine,
        builder_class,
        "(ILjava/lang/String;)V",
        &[Value::Int(1), video_id],
    );
    let video = object(call(
        &mut engine,
        video_builder,
        "build",
        "()Landroid/media/tv/TvTrackInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, video, "getVideoPixelAspectRatio", "()F", &[]),
        Value::Float(1.0)
    );
}

#[test]
fn companion_absent_service_values_filters_and_mac_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let mac_class = "Landroid/net/MacAddress;";
    let mac_sig = "(Ljava/lang/String;)Landroid/net/MacAddress;";
    let mac_text = text(&mut engine, "02:11:22:AA:BB:CC");
    let mac_value = call_static(&mut engine, mac_class, "fromString", mac_sig, &[mac_text]);
    let mac = object(mac_value.clone());
    assert_eq!(
        call_text(&mut engine, mac, "toString", "()Ljava/lang/String;", &[]),
        "02:11:22:aa:bb:cc"
    );
    assert_eq!(
        call(&mut engine, mac, "getAddressType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, mac, "isLocallyAssigned", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(&mut engine, mac, "toOuiString", "()Ljava/lang/String;", &[]),
        "02:11:22"
    );
    let mac_bytes = call(&mut engine, mac, "toByteArray", "()[B", &[]);
    assert_eq!(
        byte_array_values(&engine, &mac_bytes),
        vec![0x02, 0x11, 0x22, 0xaa, 0xbb, 0xcc]
    );
    let mask_text = text(&mut engine, "ff:ff:ff:00:00:00");
    let mask = object(call_static(
        &mut engine,
        mac_class,
        "fromString",
        mac_sig,
        &[mask_text],
    ));
    let base_text = text(&mut engine, "02:11:22:00:00:99");
    let base = object(call_static(
        &mut engine,
        mac_class,
        "fromString",
        mac_sig,
        &[base_text],
    ));
    assert_eq!(
        call(
            &mut engine,
            mac,
            "matches",
            "(Landroid/net/MacAddress;Landroid/net/MacAddress;)Z",
            &[Value::Obj(base), Value::Obj(mask)]
        ),
        Value::Int(1)
    );
    let short_mac = text(&mut engine, "1:2:3");
    assert_throws(
        engine.invoke_static(mac_class, "fromString", mac_sig, &[short_mac]),
        "Ljava/lang/IllegalArgumentException;",
    );
    let invalid_mac = text(&mut engine, "GG:11:22:33:44:55");
    assert_throws(
        engine.invoke_static(mac_class, "fromString", mac_sig, &[invalid_mac]),
        "Ljava/lang/NumberFormatException;",
    );

    let pattern = af_pattern(&mut engine, "Rusty.*");
    let bt_builder = p49_new(
        &mut engine,
        "Landroid/companion/BluetoothDeviceFilter$Builder;",
        "()V",
        &[],
    );
    call(
        &mut engine,
        bt_builder,
        "setNamePattern",
        "(Ljava/util/regex/Pattern;)Landroid/companion/BluetoothDeviceFilter$Builder;",
        std::slice::from_ref(&pattern),
    );
    let bt_address = text(&mut engine, "02:11:22:AA:BB:CC");
    call(
        &mut engine,
        bt_builder,
        "setAddress",
        "(Ljava/lang/String;)Landroid/companion/BluetoothDeviceFilter$Builder;",
        &[bt_address],
    );
    call(
        &mut engine,
        bt_builder,
        "addServiceUuid",
        "(Landroid/os/ParcelUuid;Landroid/os/ParcelUuid;)Landroid/companion/BluetoothDeviceFilter$Builder;",
        &[Value::Null, Value::Null],
    );
    let bt_filter = object(call(
        &mut engine,
        bt_builder,
        "build",
        "()Landroid/companion/BluetoothDeviceFilter;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, bt_filter, "getMediumType", "()I", &[]),
        Value::Int(0)
    );
    let service_uuids = call(
        &mut engine,
        bt_filter,
        "getServiceUuids",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, service_uuids), 1);
    assert_throws(
        engine.invoke_instance(
            bt_builder,
            "build",
            "()Landroid/companion/BluetoothDeviceFilter;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let ble_builder = p49_new(
        &mut engine,
        "Landroid/companion/BluetoothLeDeviceFilter$Builder;",
        "()V",
        &[],
    );
    assert_throws(
        engine.invoke_instance(
            ble_builder,
            "setRawDataFilter",
            "([B[B)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let raw = byte_array(&mut engine, &[1, 2]);
    let short_mask = byte_array(&mut engine, &[1]);
    assert_throws(
        engine.invoke_instance(
            ble_builder,
            "setRawDataFilter",
            "([B[B)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
            &[raw, short_mask],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let raw = byte_array(&mut engine, &[1, 2]);
    let raw_mask = byte_array(&mut engine, &[0xff, 0xf0]);
    call(
        &mut engine,
        ble_builder,
        "setRawDataFilter",
        "([B[B)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
        &[raw, raw_mask],
    );
    let long_prefix = text(&mut engine, "ThisPrefixIsTooLong");
    let suffix = text(&mut engine, "!");
    assert_throws(
        engine.invoke_instance(
            ble_builder,
            "setRenameFromName",
            "(Ljava/lang/String;Ljava/lang/String;II)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
            &[long_prefix, suffix.clone(), Value::Int(0), Value::Int(3)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let prefix = text(&mut engine, "Watch");
    call(
        &mut engine,
        ble_builder,
        "setRenameFromName",
        "(Ljava/lang/String;Ljava/lang/String;II)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
        &[prefix, suffix, Value::Int(0), Value::Int(3)],
    );
    let second_prefix = text(&mut engine, "Other");
    let second_suffix = text(&mut engine, "?");
    assert_throws(
        engine.invoke_instance(
            ble_builder,
            "setRenameFromName",
            "(Ljava/lang/String;Ljava/lang/String;II)Landroid/companion/BluetoothLeDeviceFilter$Builder;",
            &[
                second_prefix,
                second_suffix,
                Value::Int(0),
                Value::Int(1),
            ],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let ble_filter = object(call(
        &mut engine,
        ble_builder,
        "build",
        "()Landroid/companion/BluetoothLeDeviceFilter;",
        &[],
    ));
    let scan_filter = call(
        &mut engine,
        ble_filter,
        "getScanFilter",
        "()Landroid/bluetooth/le/ScanFilter;",
        &[],
    );
    assert_eq!(
        engine.heap.class_of(object(scan_filter)).unwrap(),
        "Landroid/bluetooth/le/ScanFilter;"
    );
    assert_eq!(
        call_text(
            &mut engine,
            ble_filter,
            "getRenamePrefix",
            "()Ljava/lang/String;",
            &[]
        ),
        "Watch"
    );
    assert_eq!(
        call(&mut engine, ble_filter, "getMediumType", "()I", &[]),
        Value::Int(1)
    );

    let wifi_builder = p49_new(
        &mut engine,
        "Landroid/companion/WifiDeviceFilter$Builder;",
        "()V",
        &[],
    );
    call(
        &mut engine,
        wifi_builder,
        "setBssid",
        "(Landroid/net/MacAddress;)Landroid/companion/WifiDeviceFilter$Builder;",
        std::slice::from_ref(&mac_value),
    );
    let wifi_filter = object(call(
        &mut engine,
        wifi_builder,
        "build",
        "()Landroid/companion/WifiDeviceFilter;",
        &[],
    ));
    let default_mask = object(call(
        &mut engine,
        wifi_filter,
        "getBssidMask",
        "()Landroid/net/MacAddress;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            default_mask,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "ff:ff:ff:ff:ff:ff"
    );
    assert_eq!(
        call(&mut engine, wifi_filter, "getMediumType", "()I", &[]),
        Value::Int(2)
    );
    let bad_wifi_builder = p49_new(
        &mut engine,
        "Landroid/companion/WifiDeviceFilter$Builder;",
        "()V",
        &[],
    );
    call(
        &mut engine,
        bad_wifi_builder,
        "setBssidMask",
        "(Landroid/net/MacAddress;)Landroid/companion/WifiDeviceFilter$Builder;",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            bad_wifi_builder,
            "build",
            "()Landroid/companion/WifiDeviceFilter;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            bad_wifi_builder,
            "build",
            "()Landroid/companion/WifiDeviceFilter;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let request_builder = p49_new(
        &mut engine,
        "Landroid/companion/AssociationRequest$Builder;",
        "()V",
        &[],
    );
    assert_throws(
        engine.invoke_instance(
            request_builder,
            "setDisplayName",
            "(Ljava/lang/CharSequence;)Landroid/companion/AssociationRequest$Builder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        request_builder,
        "addDeviceFilter",
        "(Landroid/companion/DeviceFilter;)Landroid/companion/AssociationRequest$Builder;",
        &[Value::Null],
    );
    call(
        &mut engine,
        request_builder,
        "addDeviceFilter",
        "(Landroid/companion/DeviceFilter;)Landroid/companion/AssociationRequest$Builder;",
        &[Value::Obj(bt_filter)],
    );
    call(
        &mut engine,
        request_builder,
        "setSingleDevice",
        "(Z)Landroid/companion/AssociationRequest$Builder;",
        &[Value::Int(1)],
    );
    let profile = text(&mut engine, "android.app.role.COMPANION_DEVICE_WATCH");
    call(
        &mut engine,
        request_builder,
        "setDeviceProfile",
        "(Ljava/lang/String;)Landroid/companion/AssociationRequest$Builder;",
        &[profile],
    );
    call(
        &mut engine,
        request_builder,
        "setSelfManaged",
        "(Z)Landroid/companion/AssociationRequest$Builder;",
        &[Value::Int(1)],
    );
    let display_name = text(&mut engine, "Rusty Watch");
    call(
        &mut engine,
        request_builder,
        "setDisplayName",
        "(Ljava/lang/CharSequence;)Landroid/companion/AssociationRequest$Builder;",
        &[display_name],
    );
    let request = object(call(
        &mut engine,
        request_builder,
        "build",
        "()Landroid/companion/AssociationRequest;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, request, "isSingleDevice", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, request, "isSelfManaged", "()Z", &[]),
        Value::Int(1)
    );
    let request_filters = call(
        &mut engine,
        request,
        "getDeviceFilters",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, request_filters.clone()), 1);
    assert_eq!(
        p49_list_get(&mut engine, request_filters, 0),
        Value::Obj(bt_filter)
    );
    assert!(matches!(
        call(&mut engine, request, "getCreationTime", "()J", &[]),
        Value::Long(_)
    ));

    let info_class = "Landroid/companion/AssociationInfo;";
    let info_sig = "(IILjava/lang/String;Landroid/net/MacAddress;Ljava/lang/CharSequence;Ljava/lang/String;Landroid/companion/AssociatedDevice;ZZZJJI)V";
    let package_name = text(&mut engine, "dev.RustyDroid.test");
    let info_display = text(&mut engine, "Rusty Watch");
    let info_profile = text(&mut engine, "android.app.role.COMPANION_DEVICE_WATCH");
    let bad_info = instance(&mut engine, info_class);
    assert_throws(
        engine.invoke_instance(
            bad_info,
            "<init>",
            info_sig,
            &[
                Value::Int(0),
                Value::Int(10),
                package_name.clone(),
                mac_value.clone(),
                info_display.clone(),
                info_profile.clone(),
                Value::Null,
                Value::Int(0),
                Value::Int(1),
                Value::Int(0),
                Value::Long(123),
                Value::Long(456),
                Value::Int(3),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let info = p49_new(
        &mut engine,
        info_class,
        info_sig,
        &[
            Value::Int(7),
            Value::Int(10),
            package_name,
            mac_value,
            info_display,
            info_profile,
            Value::Null,
            Value::Int(0),
            Value::Int(1),
            Value::Int(0),
            Value::Long(123),
            Value::Long(456),
            Value::Int(3),
        ],
    );
    assert_eq!(call(&mut engine, info, "getId", "()I", &[]), Value::Int(7));
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getDeviceMacAddressAsString",
            "()Ljava/lang/String;",
            &[]
        ),
        "02:11:22:AA:BB:CC"
    );
    let last_connected = object(call(
        &mut engine,
        info,
        "getLastTimeConnectedMs",
        "()Ljava/lang/Long;",
        &[],
    ));
    assert_eq!(field(&engine, last_connected, "value"), Value::Long(456));
    let lower_mac_text = text(&mut engine, "02:11:22:aa:bb:cc");
    assert_eq!(
        call(
            &mut engine,
            info,
            "isLinkedTo",
            "(Ljava/lang/String;)Z",
            &[lower_mac_text]
        ),
        Value::Int(1)
    );

    let info_builder = p49_new(
        &mut engine,
        "Landroid/companion/AssociationInfo$Builder;",
        "(Landroid/companion/AssociationInfo;)V",
        &[Value::Obj(info)],
    );
    assert_throws(
        engine.invoke_instance(
            info_builder,
            "setLastTimeConnected",
            "(J)Landroid/companion/AssociationInfo$Builder;",
            &[Value::Long(-1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        info_builder,
        "setLastTimeConnected",
        "(J)Landroid/companion/AssociationInfo$Builder;",
        &[Value::Long(789)],
    );
    call(
        &mut engine,
        info_builder,
        "setRevoked",
        "(Z)Landroid/companion/AssociationInfo$Builder;",
        &[Value::Int(1)],
    );
    let updated = object(call(
        &mut engine,
        info_builder,
        "build",
        "()Landroid/companion/AssociationInfo;",
        &[],
    ));
    assert_ne!(updated, info);
    assert_eq!(
        call(&mut engine, updated, "getId", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(&mut engine, updated, "isRevoked", "()Z", &[]),
        Value::Int(1)
    );

    let manager = p57_system_service(&mut engine, "companiondevice");
    assert_eq!(
        engine.heap.class_of(manager).unwrap(),
        "Landroid/companion/CompanionDeviceManager;"
    );
    assert_eq!(
        call(&mut engine, manager, "checkFeaturePresent", "()Z", &[]),
        Value::Int(0)
    );
    let associations = call(
        &mut engine,
        manager,
        "getMyAssociations",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, associations), 0);
    let component = instance(&mut engine, "Landroid/content/ComponentName;");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "hasNotificationAccess",
            "(Landroid/content/ComponentName;)Z",
            &[Value::Obj(component)]
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        manager,
        "associate",
        "(Landroid/companion/AssociationRequest;Landroid/companion/CompanionDeviceManager$Callback;Landroid/os/Handler;)V",
        &[Value::Null, Value::Null, Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "buildAssociationCancellationIntent",
            "()Landroid/content/IntentSender;",
            &[]
        ),
        Value::Null
    );

    let callback = p49_new(
        &mut engine,
        "Landroid/companion/CompanionDeviceManager$Callback;",
        "()V",
        &[],
    );
    let sender = instance(&mut engine, "Landroid/content/IntentSender;");
    call(
        &mut engine,
        callback,
        "onAssociationPending",
        "(Landroid/content/IntentSender;)V",
        &[Value::Obj(sender)],
    );
    assert_eq!(
        field(&engine, callback, "rd_pending_sender"),
        Value::Obj(sender)
    );
    assert_eq!(
        field(&engine, callback, "rd_found_sender"),
        Value::Obj(sender)
    );

    // Deep audit: the absent-service getMyAssociations result is
    // Collections.emptyList(), so adding to it throws (the deprecated
    // getAssociations maps into a fresh list and stays empty here).
    let my_associations = object(call(
        &mut engine,
        manager,
        "getMyAssociations",
        "()Ljava/util/List;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(
            my_associations,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    let legacy_associations = call(
        &mut engine,
        manager,
        "getAssociations",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, legacy_associations), 0);
}

// ---------------------------------------------------------------------------
// Batch 41-A: six android.service.* surfaces in their bounded
// unbound/unattached profiles (Quick Settings, wallpaper, dreams, Chooser,
// carrier and media-browser service bases/values).
// ---------------------------------------------------------------------------

const P41_TILE: &str = "Landroid/service/quicksettings/Tile;";
const P41_TILE_SERVICE: &str = "Landroid/service/quicksettings/TileService;";
const P41_WALLPAPER_SERVICE: &str = "Landroid/service/wallpaper/WallpaperService;";
const P41_WALLPAPER_ENGINE: &str = "Landroid/service/wallpaper/WallpaperService$Engine;";
const P41_DREAM_SERVICE: &str = "Landroid/service/dreams/DreamService;";
const P41_CHOOSER_TARGET: &str = "Landroid/service/chooser/ChooserTarget;";
const P41_CHOOSER_SERVICE: &str = "Landroid/service/chooser/ChooserTargetService;";
const P41_CHOOSER_RESULT: &str = "Landroid/service/chooser/IChooserTargetResult;";
const P41_CARRIER_IDENTIFIER: &str = "Landroid/service/carrier/CarrierIdentifier;";
const P41_CARRIER_SERVICE: &str = "Landroid/service/carrier/CarrierService;";
const P41_MEDIA_SERVICE: &str = "Landroid/service/media/MediaBrowserService;";
const P41_MEDIA_ROOT: &str = "Landroid/service/media/MediaBrowserService$BrowserRoot;";
const P41_MEDIA_RESULT: &str = "Landroid/service/media/MediaBrowserService$Result;";

fn p41_new(engine: &mut Engine, class: &str, sig: &str, args: &[Value]) -> ObjRef {
    p49_new(engine, class, sig, args)
}

fn p41_intent(engine: &mut Engine, action: &str) -> ObjRef {
    let action = text(engine, action);
    p41_new(
        engine,
        "Landroid/content/Intent;",
        "(Ljava/lang/String;)V",
        &[action],
    )
}

fn p41_component(engine: &mut Engine) -> ObjRef {
    let package = text(engine, "dev.RustyDroid.test");
    let cls = text(engine, "dev.RustyDroid.test.ShareActivity");
    p41_new(
        engine,
        "Landroid/content/ComponentName;",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[package, cls],
    )
}

fn p41_bundle(engine: &mut Engine) -> ObjRef {
    p41_new(engine, "Landroid/os/Bundle;", "()V", &[])
}

fn p41_pending(engine: &mut Engine, method: &str, request: i32) -> Value {
    let context = instance(engine, CONTEXT);
    let intent = p41_intent(engine, "dev.RustyDroid.test.PENDING");
    call_static(
        engine,
        "Landroid/app/PendingIntent;",
        method,
        "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;",
        &[
            Value::Obj(context),
            Value::Int(request),
            Value::Obj(intent),
            Value::Int(0x0400_0000),
        ],
    )
}

fn p41_byte_array(engine: &mut Engine, values: &[i32]) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Int, values.len(), "[B")
        .unwrap();
    *engine.heap.array_elems_mut(array).unwrap() = values.iter().copied().map(Value::Int).collect();
    Value::Obj(array)
}

#[test]
fn quicksettings_tile_and_unbound_service_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let tile = p41_new(&mut engine, P41_TILE, "()V", &[]);
    assert_eq!(
        call(&mut engine, tile, "getState", "()I", &[]),
        Value::Int(1),
        "Tile defaults to STATE_INACTIVE"
    );
    assert_eq!(
        call(
            &mut engine,
            tile,
            "getIcon",
            "()Landroid/graphics/drawable/Icon;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            tile,
            "getLabel",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        Value::Null
    );

    call(&mut engine, tile, "setState", "(I)V", &[Value::Int(2)]);
    let label = text(&mut engine, "Wi-Fi");
    call(
        &mut engine,
        tile,
        "setLabel",
        "(Ljava/lang/CharSequence;)V",
        &[label],
    );
    let subtitle = text(&mut engine, "Connected");
    call(
        &mut engine,
        tile,
        "setSubtitle",
        "(Ljava/lang/CharSequence;)V",
        &[subtitle],
    );
    let content = text(&mut engine, "Toggle Wi-Fi");
    call(
        &mut engine,
        tile,
        "setContentDescription",
        "(Ljava/lang/CharSequence;)V",
        &[content],
    );
    let state_description = text(&mut engine, "On");
    call(
        &mut engine,
        tile,
        "setStateDescription",
        "(Ljava/lang/CharSequence;)V",
        &[state_description],
    );
    let icon = instance(&mut engine, "Landroid/graphics/drawable/Icon;");
    call(
        &mut engine,
        tile,
        "setIcon",
        "(Landroid/graphics/drawable/Icon;)V",
        &[Value::Obj(icon)],
    );
    assert_eq!(
        call(&mut engine, tile, "getState", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_text(
            &mut engine,
            tile,
            "getLabel",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Wi-Fi"
    );
    assert_eq!(
        call_text(
            &mut engine,
            tile,
            "getSubtitle",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Connected"
    );
    assert_eq!(
        call(
            &mut engine,
            tile,
            "getIcon",
            "()Landroid/graphics/drawable/Icon;",
            &[]
        ),
        Value::Obj(icon)
    );
    assert_eq!(
        call(&mut engine, tile, "describeContents", "()I", &[]),
        Value::Int(0)
    );

    let broadcast = p41_pending(&mut engine, "getBroadcast", 41);
    assert_throws(
        engine.invoke_instance(
            tile,
            "setActivityLaunchForClick",
            "(Landroid/app/PendingIntent;)V",
            &[broadcast],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let activity = p41_pending(&mut engine, "getActivity", 42);
    call(
        &mut engine,
        tile,
        "setActivityLaunchForClick",
        "(Landroid/app/PendingIntent;)V",
        std::slice::from_ref(&activity),
    );
    assert_eq!(
        call(
            &mut engine,
            tile,
            "getActivityLaunchForClick",
            "()Landroid/app/PendingIntent;",
            &[]
        ),
        activity
    );
    assert_throws(
        engine.invoke_instance(tile, "updateTile", "()V", &[]),
        "Ljava/lang/NullPointerException;",
    );

    let service = p41_new(&mut engine, P41_TILE_SERVICE, "()V", &[]);
    for method in [
        "onTileAdded",
        "onTileRemoved",
        "onStartListening",
        "onStopListening",
        "onClick",
        "onDestroy",
    ] {
        call(&mut engine, service, method, "()V", &[]);
    }
    assert_eq!(
        call(
            &mut engine,
            service,
            "getQsTile",
            "()Landroid/service/quicksettings/Tile;",
            &[]
        ),
        Value::Null,
        "no Tile exists before SystemUI binds the service"
    );
    call(
        &mut engine,
        service,
        "setStatusIcon",
        "(Landroid/graphics/drawable/Icon;Ljava/lang/String;)V",
        &[Value::Null, Value::Null],
    );
    assert_throws(
        engine.invoke_instance(service, "isSecure", "()Z", &[]),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(service, "isLocked", "()Z", &[]),
        "Ljava/lang/NullPointerException;",
    );
    let runnable = instance(&mut engine, "Ljava/lang/Runnable;");
    assert_throws(
        engine.invoke_instance(
            service,
            "unlockAndRun",
            "(Ljava/lang/Runnable;)V",
            &[Value::Obj(runnable)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        field(&engine, service, "mUnlockRunnable"),
        Value::Obj(runnable),
        "AOSP stores the Runnable before dereferencing the absent QS service"
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "startActivityAndCollapse",
            "(Landroid/app/PendingIntent;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let bind_intent = p41_intent(&mut engine, "android.service.quicksettings.action.QS_TILE");
    assert_throws(
        engine.invoke_instance(
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(bind_intent)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(field(&engine, service, "mService"), Value::Null);
    assert_eq!(field(&engine, service, "mTileToken"), Value::Null);
}

#[test]
fn wallpaper_engine_unattached_state_and_defaults_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let service = p41_new(&mut engine, P41_WALLPAPER_SERVICE, "()V", &[]);
    let wallpaper = p41_new(
        &mut engine,
        P41_WALLPAPER_ENGINE,
        "(Landroid/service/wallpaper/WallpaperService;)V",
        &[Value::Obj(service)],
    );

    let holder = call(
        &mut engine,
        wallpaper,
        "getSurfaceHolder",
        "()Landroid/view/SurfaceHolder;",
        &[],
    );
    assert!(!matches!(holder, Value::Null));
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "getSurfaceHolder",
            "()Landroid/view/SurfaceHolder;",
            &[]
        ),
        holder
    );
    assert_eq!(
        call(&mut engine, wallpaper, "isVisible", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, wallpaper, "isInAmbientMode", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "supportsLocalColorExtraction",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, wallpaper, "getZoom", "()F", &[]),
        Value::Float(0.0)
    );
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "getDisplayContext",
            "()Landroid/content/Context;",
            &[]
        ),
        Value::Null
    );
    for (method, sig) in [
        ("getWallpaperFlags", "()I"),
        ("getDesiredMinimumWidth", "()I"),
        ("getDesiredMinimumHeight", "()I"),
        ("isPreview", "()Z"),
    ] {
        assert_throws(
            engine.invoke_instance(wallpaper, method, sig, &[]),
            "Ljava/lang/NullPointerException;",
        );
    }

    call(
        &mut engine,
        wallpaper,
        "setTouchEventsEnabled",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(field(&engine, wallpaper, "mWindowFlags"), Value::Int(0));
    call(
        &mut engine,
        wallpaper,
        "setTouchEventsEnabled",
        "(Z)V",
        &[Value::Int(0)],
    );
    assert_eq!(field(&engine, wallpaper, "mWindowFlags"), Value::Int(0x10));
    call(
        &mut engine,
        wallpaper,
        "setOffsetNotificationsEnabled",
        "(Z)V",
        &[Value::Int(0)],
    );
    assert_eq!(
        field(&engine, wallpaper, "mWindowPrivateFlags"),
        Value::Int(1 << 25)
    );
    call(
        &mut engine,
        wallpaper,
        "setShowForAllUsers",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        field(&engine, wallpaper, "mWindowPrivateFlags"),
        Value::Int((1 << 25) | (1 << 4))
    );

    call(
        &mut engine,
        wallpaper,
        "setZoom",
        "(F)V",
        &[Value::Float(0.25)],
    );
    assert_eq!(
        call(&mut engine, wallpaper, "getZoom", "()F", &[]),
        Value::Float(0.25)
    );
    call(
        &mut engine,
        wallpaper,
        "doAmbientModeChanged",
        "(ZJ)V",
        &[Value::Int(1), Value::Long(0)],
    );
    assert_eq!(
        call(&mut engine, wallpaper, "isInAmbientMode", "()Z", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        wallpaper,
        "setZoom",
        "(F)V",
        &[Value::Float(0.0)],
    );
    assert_eq!(
        call(&mut engine, wallpaper, "getZoom", "()F", &[]),
        Value::Float(0.0),
        "ambient setZoom(0) silently resets the previous zoom"
    );
    call(
        &mut engine,
        wallpaper,
        "setZoom",
        "(F)V",
        &[Value::Float(0.5)],
    );
    assert_eq!(
        call(&mut engine, wallpaper, "getZoom", "()F", &[]),
        Value::Float(0.5)
    );
    assert_eq!(
        call(&mut engine, wallpaper, "shouldZoomOutWallpaper", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "shouldWaitForEngineShown",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        wallpaper,
        "onCreate",
        "(Landroid/view/SurfaceHolder;)V",
        std::slice::from_ref(&holder),
    );
    let action = text(&mut engine, "android.wallpaper.tap");
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "onCommand",
            "(Ljava/lang/String;IIILandroid/os/Bundle;Z)Landroid/os/Bundle;",
            &[
                action,
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Null,
                Value::Int(1),
            ]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            wallpaper,
            "onComputeColors",
            "()Landroid/app/WallpaperColors;",
            &[]
        ),
        Value::Null
    );
    call(
        &mut engine,
        wallpaper,
        "onSurfaceDestroyed",
        "(Landroid/view/SurfaceHolder;)V",
        &[holder],
    );
    call(&mut engine, wallpaper, "onDestroy", "()V", &[]);
}

#[test]
fn dream_service_unattached_state_and_finish_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let dream = p41_new(&mut engine, P41_DREAM_SERVICE, "()V", &[]);
    assert_eq!(
        call(&mut engine, dream, "isInteractive", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "isFullscreen", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "isScreenBright", "()Z", &[]),
        Value::Int(1),
        "DreamService.mScreenBright defaults to true"
    );
    assert_eq!(
        call(&mut engine, dream, "isWindowless", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "canDoze", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "isDozing", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "getDozeScreenState", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "getDozeScreenBrightness", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(
            &mut engine,
            dream,
            "getWindow",
            "()Landroid/view/Window;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            dream,
            "getWindowManager",
            "()Landroid/view/WindowManager;",
            &[]
        ),
        Value::Null
    );

    call(
        &mut engine,
        dream,
        "setInteractive",
        "(Z)V",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        dream,
        "setFullscreen",
        "(Z)V",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        dream,
        "setScreenBright",
        "(Z)V",
        &[Value::Int(0)],
    );
    call(
        &mut engine,
        dream,
        "setWindowless",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, dream, "isInteractive", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, dream, "isFullscreen", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, dream, "isScreenBright", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, dream, "isWindowless", "()Z", &[]),
        Value::Int(1)
    );

    call(
        &mut engine,
        dream,
        "setDozeScreenBrightness",
        "(I)V",
        &[Value::Int(999)],
    );
    assert_eq!(
        call(&mut engine, dream, "getDozeScreenBrightness", "()I", &[]),
        Value::Int(255)
    );
    call(
        &mut engine,
        dream,
        "setDozeScreenBrightness",
        "(I)V",
        &[Value::Int(-50)],
    );
    assert_eq!(
        call(&mut engine, dream, "getDozeScreenBrightness", "()I", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        dream,
        "setDozeScreenBrightness",
        "(I)V",
        &[Value::Int(-1)],
    );
    call(
        &mut engine,
        dream,
        "setDozeScreenState",
        "(I)V",
        &[Value::Int(4)],
    );
    assert_eq!(
        call(&mut engine, dream, "getDozeScreenState", "()I", &[]),
        Value::Int(4)
    );
    call(&mut engine, dream, "startDozing", "()V", &[]);
    assert_eq!(
        call(&mut engine, dream, "isDozing", "()Z", &[]),
        Value::Int(0),
        "startDozing is a no-op when the dream cannot doze"
    );
    call(&mut engine, dream, "onDreamingStarted", "()V", &[]);
    call(&mut engine, dream, "wakeUp", "()V", &[]);
    assert_eq!(field(&engine, dream, "mWaking"), Value::Int(1));
    assert_eq!(
        field(&engine, dream, "mFinished"),
        Value::Int(1),
        "the default onWakeUp calls finish()"
    );
    call(&mut engine, dream, "wakeUp", "()V", &[]);
    call(&mut engine, dream, "finish", "()V", &[]);
    call(&mut engine, dream, "onDreamingStopped", "()V", &[]);

    let doze_dream = p41_new(&mut engine, P41_DREAM_SERVICE, "()V", &[]);
    engine
        .heap
        .put_field(doze_dream, "mCanDoze", Value::Int(1))
        .unwrap();
    call(&mut engine, doze_dream, "startDozing", "()V", &[]);
    assert_eq!(
        call(&mut engine, doze_dream, "isDozing", "()Z", &[]),
        Value::Int(1),
        "updateDoze returns early without a dream token"
    );
    assert_throws(
        engine.invoke_instance(doze_dream, "stopDozing", "()V", &[]),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call(&mut engine, doze_dream, "isDozing", "()Z", &[]),
        Value::Int(0),
        "AOSP clears mDozing before dereferencing the absent IDreamManager"
    );

    assert_eq!(
        call_static(
            &mut engine,
            P41_DREAM_SERVICE,
            "getDreamMetadata",
            "(Landroid/content/Context;Landroid/content/pm/ServiceInfo;)Landroid/service/dreams/DreamService$DreamMetadata;",
            &[Value::Null, Value::Null]
        ),
        Value::Null,
        "AOSP returns null immediately for a null ServiceInfo"
    );
    let intent = p41_intent(&mut engine, "android.service.dreams.DreamService");
    assert_eq!(
        call(
            &mut engine,
            dream,
            "onUnbind",
            "(Landroid/content/Intent;)Z",
            &[Value::Obj(intent)]
        ),
        Value::Int(0)
    );
}

#[test]
fn chooser_target_validation_and_service_binding_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let component = p41_component(&mut engine);
    let title = text(&mut engine, "Alice");
    let icon = instance(&mut engine, "Landroid/graphics/drawable/Icon;");
    let extras = p41_bundle(&mut engine);
    let target_sig =
        "(Ljava/lang/CharSequence;Landroid/graphics/drawable/Icon;FLandroid/content/ComponentName;Landroid/os/Bundle;)V";
    let bad = instance(&mut engine, P41_CHOOSER_TARGET);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            target_sig,
            &[
                title.clone(),
                Value::Obj(icon),
                Value::Float(1.1),
                Value::Obj(component),
                Value::Obj(extras),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let target = p41_new(
        &mut engine,
        P41_CHOOSER_TARGET,
        target_sig,
        &[
            title,
            Value::Obj(icon),
            Value::Float(0.75),
            Value::Obj(component),
            Value::Obj(extras),
        ],
    );
    assert_eq!(
        call_text(
            &mut engine,
            target,
            "getTitle",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Alice"
    );
    assert_eq!(
        call(&mut engine, target, "getScore", "()F", &[]),
        Value::Float(0.75)
    );
    assert_eq!(
        call(
            &mut engine,
            target,
            "getComponentName",
            "()Landroid/content/ComponentName;",
            &[]
        ),
        Value::Obj(component)
    );
    assert_eq!(
        call(
            &mut engine,
            target,
            "getIntentExtras",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Obj(extras)
    );
    assert_eq!(
        call(&mut engine, target, "describeContents", "()I", &[]),
        Value::Int(0)
    );

    let plain_title = text(&mut engine, "Share");
    let plain = p41_new(
        &mut engine,
        P41_CHOOSER_TARGET,
        target_sig,
        &[
            plain_title,
            Value::Null,
            Value::Float(0.5),
            Value::Null,
            Value::Null,
        ],
    );
    assert_eq!(
        call_text(&mut engine, plain, "toString", "()Ljava/lang/String;", &[]),
        "ChooserTarget{null, null, 'Share', 0.5}"
    );
    let nan_title = text(&mut engine, "NaN");
    let nan_target = p41_new(
        &mut engine,
        P41_CHOOSER_TARGET,
        target_sig,
        &[
            nan_title,
            Value::Null,
            Value::Float(f32::NAN),
            Value::Null,
            Value::Null,
        ],
    );
    assert!(matches!(
        call(&mut engine, nan_target, "getScore", "()F", &[]),
        Value::Float(score) if score.is_nan()
    ));

    let service = p41_new(&mut engine, P41_CHOOSER_SERVICE, "()V", &[]);
    let wrong = p41_intent(&mut engine, "example.WRONG");
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong)]
        ),
        Value::Null
    );
    let correct = p41_intent(&mut engine, "android.service.chooser.ChooserTargetService");
    let wrapper = call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(correct)],
    );
    let wrapper_ref = object(wrapper.clone());
    assert_eq!(
        engine.heap.class_of(wrapper_ref).unwrap(),
        "Landroid/service/chooser/ChooserTargetService$IChooserTargetServiceWrapper;"
    );
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(correct)]
        ),
        wrapper
    );

    let result = instance(&mut engine, P41_CHOOSER_RESULT);
    let filter = instance(&mut engine, "Landroid/content/IntentFilter;");
    assert!(
        engine
            .invoke_instance(
                wrapper_ref,
                "getChooserTargets",
                "(Landroid/content/ComponentName;Landroid/content/IntentFilter;Landroid/service/chooser/IChooserTargetResult;)V",
                &[Value::Obj(component), Value::Obj(filter), Value::Obj(result)],
            )
            .is_err(),
        "the exact abstract service has no onGetChooserTargets implementation"
    );
    assert_eq!(
        p49_field_or_null(&engine, result, "rd_targets"),
        Value::Null
    );
    assert_eq!(field(&engine, result, "rd_send_count"), Value::Int(1));
}

#[test]
fn carrier_identifier_and_service_binding_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let mcc = text(&mut engine, "724");
    let mnc = text(&mut engine, "05");
    let spn = text(&mut engine, "Rusty Mobile");
    let imsi = text(&mut engine, "724051234567890");
    let gid1 = text(&mut engine, "01");
    let gid2 = text(&mut engine, "02");
    let full_sig =
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;II)V";
    let identifier = p41_new(
        &mut engine,
        P41_CARRIER_IDENTIFIER,
        full_sig,
        &[
            mcc.clone(),
            mnc.clone(),
            spn.clone(),
            imsi.clone(),
            gid1.clone(),
            gid2.clone(),
            Value::Int(1234),
            Value::Int(5678),
        ],
    );
    assert_eq!(
        call_text(
            &mut engine,
            identifier,
            "getMcc",
            "()Ljava/lang/String;",
            &[]
        ),
        "724"
    );
    assert_eq!(
        call_text(
            &mut engine,
            identifier,
            "getMnc",
            "()Ljava/lang/String;",
            &[]
        ),
        "05"
    );
    assert_eq!(
        call_text(
            &mut engine,
            identifier,
            "getSpn",
            "()Ljava/lang/String;",
            &[]
        ),
        "Rusty Mobile"
    );
    assert_eq!(
        call(&mut engine, identifier, "getCarrierId", "()I", &[]),
        Value::Int(1234)
    );
    assert_eq!(
        call(&mut engine, identifier, "getSpecificCarrierId", "()I", &[]),
        Value::Int(5678)
    );
    assert_eq!(
        call_text(
            &mut engine,
            identifier,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "CarrierIdentifier{mcc=724,mnc=05,spn=Rusty Mobile,imsi=[****],gid1=01,gid2=02,carrierid=1234,specificCarrierId=5678}"
    );

    let same = p41_new(
        &mut engine,
        P41_CARRIER_IDENTIFIER,
        full_sig,
        &[
            mcc.clone(),
            mnc.clone(),
            spn,
            imsi,
            gid1.clone(),
            gid2.clone(),
            Value::Int(1234),
            Value::Int(5678),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            identifier,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, identifier, "hashCode", "()I", &[]),
        call(&mut engine, same, "hashCode", "()I", &[])
    );
    let different = p41_new(
        &mut engine,
        P41_CARRIER_IDENTIFIER,
        full_sig,
        &[
            mcc,
            mnc,
            Value::Null,
            Value::Null,
            gid1,
            gid2,
            Value::Int(1234),
            Value::Int(9999),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            identifier,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(different)]
        ),
        Value::Int(0)
    );

    let unknown_mcc = text(&mut engine, "310");
    let unknown_mnc = text(&mut engine, "260");
    let unknown = p41_new(
        &mut engine,
        P41_CARRIER_IDENTIFIER,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            unknown_mcc,
            unknown_mnc,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ],
    );
    assert_eq!(
        call(&mut engine, unknown, "getCarrierId", "()I", &[]),
        Value::Int(-1)
    );

    let bytes = p41_byte_array(&mut engine, &[0x27, -12, 0x50]);
    let from_bytes = p41_new(
        &mut engine,
        P41_CARRIER_IDENTIFIER,
        "([BLjava/lang/String;Ljava/lang/String;)V",
        &[bytes, Value::Null, Value::Null],
    );
    assert_eq!(
        call_text(
            &mut engine,
            from_bytes,
            "getMcc",
            "()Ljava/lang/String;",
            &[]
        ),
        "724"
    );
    assert_eq!(
        call_text(
            &mut engine,
            from_bytes,
            "getMnc",
            "()Ljava/lang/String;",
            &[]
        ),
        "05"
    );
    let bad_bytes = p41_byte_array(&mut engine, &[1, 2]);
    let bad = instance(&mut engine, P41_CARRIER_IDENTIFIER);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            "([BLjava/lang/String;Ljava/lang/String;)V",
            &[bad_bytes, Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let service = p41_new(&mut engine, P41_CARRIER_SERVICE, "()V", &[]);
    let binder = call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Null],
    );
    let binder_ref = object(binder.clone());
    assert_eq!(
        engine.heap.class_of(binder_ref).unwrap(),
        "Landroid/service/carrier/CarrierService$ICarrierServiceWrapper;"
    );
    let intent = p41_intent(&mut engine, "example.IGNORED");
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(intent)]
        ),
        binder
    );
    assert!(
        engine
            .invoke_instance(
                service,
                "onLoadConfig",
                "(ILandroid/service/carrier/CarrierIdentifier;)Landroid/os/PersistableBundle;",
                &[Value::Int(3), Value::Obj(identifier)],
            )
            .is_err(),
        "the exact abstract service has no one-argument onLoadConfig implementation"
    );
}

#[test]
fn media_browser_root_result_and_unconnected_service_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let bad_root = instance(&mut engine, P41_MEDIA_ROOT);
    assert_throws(
        engine.invoke_instance(
            bad_root,
            "<init>",
            "(Ljava/lang/String;Landroid/os/Bundle;)V",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let root_id = text(&mut engine, "library-root");
    let extras = p41_bundle(&mut engine);
    let root = p41_new(
        &mut engine,
        P41_MEDIA_ROOT,
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        &[root_id, Value::Obj(extras)],
    );
    assert_eq!(
        call_text(&mut engine, root, "getRootId", "()Ljava/lang/String;", &[]),
        "library-root"
    );
    assert_eq!(
        call(&mut engine, root, "getExtras", "()Landroid/os/Bundle;", &[]),
        Value::Obj(extras)
    );

    let service = p41_new(&mut engine, P41_MEDIA_SERVICE, "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            service,
            "getSessionToken",
            "()Landroid/media/session/MediaSession$Token;",
            &[]
        ),
        Value::Null
    );
    let correct = p41_intent(&mut engine, "android.media.browse.MediaBrowserService");
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(correct)]
        ),
        Value::Null,
        "onCreate has not created the ServiceBinder yet"
    );
    call(&mut engine, service, "onCreate", "()V", &[]);
    let wrong = p41_intent(&mut engine, "example.WRONG");
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong)]
        ),
        Value::Null
    );
    let binder = call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(correct)],
    );
    assert!(!matches!(binder, Value::Null));
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(correct)]
        ),
        binder
    );

    let debug = text(&mut engine, "load-children");
    let result = p41_new(
        &mut engine,
        P41_MEDIA_RESULT,
        "(Landroid/service/media/MediaBrowserService;Ljava/lang/Object;)V",
        &[Value::Obj(service), debug],
    );
    assert_eq!(
        call(&mut engine, result, "isDone", "()Z", &[]),
        Value::Int(0)
    );
    call(&mut engine, result, "detach", "()V", &[]);
    assert_eq!(
        call(&mut engine, result, "isDone", "()Z", &[]),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(result, "detach", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    call(
        &mut engine,
        result,
        "sendResult",
        "(Ljava/lang/Object;)V",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            result,
            "sendResult",
            "(Ljava/lang/Object;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    let item_id = text(&mut engine, "missing-item");
    let item_debug = text(&mut engine, "load-item");
    let item_result = p41_new(
        &mut engine,
        P41_MEDIA_RESULT,
        "(Landroid/service/media/MediaBrowserService;Ljava/lang/Object;)V",
        &[Value::Obj(service), item_debug],
    );
    call(
        &mut engine,
        service,
        "onLoadItem",
        "(Ljava/lang/String;Landroid/service/media/MediaBrowserService$Result;)V",
        &[item_id, Value::Obj(item_result)],
    );
    assert_eq!(field(&engine, item_result, "mFlags"), Value::Int(2));
    assert_eq!(
        call(&mut engine, item_result, "isDone", "()Z", &[]),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(item_result, "detach", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );

    let children_parent = text(&mut engine, "library-root");
    let children_options = p41_bundle(&mut engine);
    let children_debug = text(&mut engine, "load-children-options");
    let children_result = p41_new(
        &mut engine,
        P41_MEDIA_RESULT,
        "(Landroid/service/media/MediaBrowserService;Ljava/lang/Object;)V",
        &[Value::Obj(service), children_debug],
    );
    assert!(
        engine
            .invoke_instance(
                service,
                "onLoadChildren",
                "(Ljava/lang/String;Landroid/service/media/MediaBrowserService$Result;Landroid/os/Bundle;)V",
                &[
                    children_parent,
                    Value::Obj(children_result),
                    Value::Obj(children_options),
                ],
            )
            .is_err(),
        "the exact abstract service has no two-argument onLoadChildren implementation"
    );
    assert_eq!(field(&engine, children_result, "mFlags"), Value::Int(1));
    assert_eq!(
        call(&mut engine, children_result, "isDone", "()Z", &[]),
        Value::Int(0)
    );

    let parent = text(&mut engine, "library-root");
    call(
        &mut engine,
        service,
        "notifyChildrenChanged",
        "(Ljava/lang/String;)V",
        std::slice::from_ref(&parent),
    );
    let options = p41_bundle(&mut engine);
    call(
        &mut engine,
        service,
        "notifyChildrenChanged",
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        &[parent, Value::Obj(options)],
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "notifyChildrenChanged",
            "(Ljava/lang/String;Landroid/os/Bundle;)V",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(service, "getBrowserRootHints", "()Landroid/os/Bundle;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "getCurrentBrowserInfo",
            "()Landroid/media/session/MediaSessionManager$RemoteUserInfo;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    assert_throws(
        engine.invoke_instance(
            service,
            "setSessionToken",
            "(Landroid/media/session/MediaSession$Token;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let token = instance(&mut engine, "Landroid/media/session/MediaSession$Token;");
    call(
        &mut engine,
        service,
        "setSessionToken",
        "(Landroid/media/session/MediaSession$Token;)V",
        &[Value::Obj(token)],
    );
    assert_eq!(
        call(
            &mut engine,
            service,
            "getSessionToken",
            "()Landroid/media/session/MediaSession$Token;",
            &[]
        ),
        Value::Obj(token)
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "setSessionToken",
            "(Landroid/media/session/MediaSession$Token;)V",
            &[Value::Obj(token)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
}

// ---------------------------------------------------------------------------
// Appendix C Batch 35-A: android.speech + android.service.textservice.
// ---------------------------------------------------------------------------

const P35_RECOGNIZER: &str = "Landroid/speech/SpeechRecognizer;";
const P35_RECOGNIZER_PROXY: &str = "Landroid/speech/SpeechRecognizerProxy;";
const P35_LISTENER: &str = "Landroid/speech/RecognitionListener;";
const P35_ILISTENER: &str = "Landroid/speech/IRecognitionListener;";
const P35_REC_SERVICE: &str = "Landroid/speech/RecognitionService;";
const P35_REC_BINDER: &str = "Landroid/speech/RecognitionService$RecognitionServiceBinder;";
const P35_REC_CALLBACK: &str = "Landroid/speech/RecognitionService$Callback;";
const P35_ISUPPORT: &str = "Landroid/speech/IRecognitionSupportCallback;";
const P35_SUPPORT_CB: &str = "Landroid/speech/RecognitionSupportCallback;";
const P35_IMDL: &str = "Landroid/speech/IModelDownloadListener;";
const P35_SUPPORT_BUILDER: &str = "Landroid/speech/RecognitionSupport$Builder;";
const P35_PART: &str = "Landroid/speech/RecognitionPart;";
const P35_PART_BUILDER: &str = "Landroid/speech/RecognitionPart$Builder;";
const P35_SPAN: &str = "Landroid/speech/AlternativeSpan;";
const P35_RINTENT: &str = "Landroid/speech/RecognizerIntent;";
const P35_SPELL: &str = "Landroid/service/textservice/SpellCheckerService;";
const P35_SPELL_BINDER: &str =
    "Landroid/service/textservice/SpellCheckerService$SpellCheckerServiceBinder;";
const P35_SPELL_SESSION: &str = "Landroid/service/textservice/SpellCheckerService$Session;";
const P35_SPELL_INTERNAL: &str =
    "Landroid/service/textservice/SpellCheckerService$InternalISpellCheckerSession;";
const P35_SPELL_LISTENER: &str = "Lcom/android/internal/textservice/ISpellCheckerSessionListener;";

fn p35_string_list(engine: &mut Engine, items: &[&str]) -> Value {
    let list = p49_new(engine, "Ljava/util/ArrayList;", "()V", &[]);
    for item in items {
        let value = text(engine, item);
        call(engine, list, "add", "(Ljava/lang/Object;)Z", &[value]);
    }
    Value::Obj(list)
}

fn p35_array_len(engine: &Engine, value: &Value) -> usize {
    match value {
        Value::Obj(r) => engine
            .heap
            .as_array_elems(*r)
            .map(|(elems, _)| elems.len())
            .unwrap_or(0),
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn speech_recognizer_and_recognition_service_without_a_recognizer_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);

    // No RecognitionService is installed in this profile.
    assert_eq!(
        call_static(
            &mut engine,
            P35_RECOGNIZER,
            "isRecognitionAvailable",
            "(Landroid/content/Context;)Z",
            &[Value::Obj(context)],
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P35_RECOGNIZER,
            "isRecognitionAvailable",
            "(Landroid/content/Context;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call_static(
            &mut engine,
            P35_RECOGNIZER,
            "isOnDeviceRecognitionAvailable",
            "(Landroid/content/Context;)Z",
            &[Value::Obj(context)],
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P35_RECOGNIZER,
            "createOnDeviceSpeechRecognizer",
            "(Landroid/content/Context;)Landroid/speech/SpeechRecognizer;",
            &[Value::Obj(context)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    assert_throws(
        engine.invoke_static(
            P35_RECOGNIZER,
            "createSpeechRecognizer",
            "(Landroid/content/Context;)Landroid/speech/SpeechRecognizer;",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let recognizer = object(call_static(
        &mut engine,
        P35_RECOGNIZER,
        "createSpeechRecognizer",
        "(Landroid/content/Context;)Landroid/speech/SpeechRecognizer;",
        &[Value::Obj(context)],
    ));
    assert_eq!(
        engine.heap.class_of(recognizer).unwrap(),
        P35_RECOGNIZER_PROXY
    );
    assert!(engine.cp.is_subtype(P35_RECOGNIZER_PROXY, P35_RECOGNIZER));

    let listener = instance(&mut engine, P35_LISTENER);
    call(
        &mut engine,
        recognizer,
        "setRecognitionListener",
        "(Landroid/speech/RecognitionListener;)V",
        &[Value::Obj(listener)],
    );
    assert_throws(
        engine.invoke_instance(
            recognizer,
            "startListening",
            "(Landroid/content/Intent;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let intent = p41_intent(&mut engine, "android.speech.action.RECOGNIZE_SPEECH");
    call(
        &mut engine,
        recognizer,
        "startListening",
        "(Landroid/content/Intent;)V",
        &[Value::Obj(intent)],
    );
    // ERROR_CLIENT: the recognition manager service does not exist.
    assert_eq!(field(&engine, listener, "rd_error"), Value::Int(5));
    assert_eq!(field(&engine, listener, "rd_count"), Value::Int(1));
    assert_eq!(
        text_of(&engine, &field(&engine, listener, "rd_last")),
        "onError"
    );

    // stop/cancel only queue pending messages in AOSP: no new callbacks.
    call(&mut engine, recognizer, "stopListening", "()V", &[]);
    call(&mut engine, recognizer, "cancel", "()V", &[]);
    assert_eq!(field(&engine, listener, "rd_count"), Value::Int(1));

    // checkRecognitionSupport never reaches the support callback without a
    // manager; the recognition listener gets another ERROR_CLIENT.
    let support_callback = instance(&mut engine, P35_SUPPORT_CB);
    call(
        &mut engine,
        recognizer,
        "checkRecognitionSupport",
        "(Landroid/content/Intent;Ljava/util/concurrent/Executor;Landroid/speech/RecognitionSupportCallback;)V",
        &[Value::Obj(intent), Value::Null, Value::Obj(support_callback)],
    );
    assert_eq!(
        p49_field_or_null(&engine, support_callback, "rd_count"),
        Value::Null
    );
    assert_eq!(field(&engine, listener, "rd_count"), Value::Int(2));

    call(
        &mut engine,
        recognizer,
        "triggerModelDownload",
        "(Landroid/content/Intent;)V",
        &[Value::Obj(intent)],
    );
    assert_eq!(field(&engine, listener, "rd_count"), Value::Int(3));

    // destroy() is also queued behind the missing connection, so the
    // listener is still registered for the next failed connect attempt.
    call(&mut engine, recognizer, "destroy", "()V", &[]);
    call(
        &mut engine,
        recognizer,
        "startListening",
        "(Landroid/content/Intent;)V",
        &[Value::Obj(intent)],
    );
    assert_eq!(field(&engine, listener, "rd_count"), Value::Int(4));

    // The exact public base class throws UnsupportedOperationException.
    let base = p49_new(&mut engine, P35_RECOGNIZER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            base,
            "startListening",
            "(Landroid/content/Intent;)V",
            &[Value::Obj(intent)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );

    // RecognitionService base-class defaults.
    let service = p49_new(&mut engine, P35_REC_SERVICE, "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            service,
            "getMaxConcurrentSessionsCount",
            "()I",
            &[]
        ),
        Value::Int(1)
    );
    let binder = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(intent)],
    ));
    assert_eq!(engine.heap.class_of(binder).unwrap(), P35_REC_BINDER);
    assert!(engine.cp.is_subtype(P35_REC_BINDER, "Landroid/os/IBinder;"));
    let binder_again = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(intent)],
    ));
    assert_eq!(binder_again, binder);

    // Default support check reports ERROR_CANNOT_CHECK_SUPPORT (14).
    let support_recorder = instance(&mut engine, P35_ISUPPORT);
    call(
        &mut engine,
        binder,
        "checkRecognitionSupport",
        "(Landroid/content/Intent;Landroid/content/AttributionSource;Landroid/speech/IRecognitionSupportCallback;)V",
        &[Value::Obj(intent), Value::Null, Value::Obj(support_recorder)],
    );
    assert_eq!(field(&engine, support_recorder, "rd_error"), Value::Int(14));

    // Default download-with-listener reports
    // ERROR_CANNOT_LISTEN_TO_DOWNLOAD_EVENTS (15) through the wrapper.
    let download_recorder = instance(&mut engine, P35_IMDL);
    call(
        &mut engine,
        binder,
        "triggerModelDownload",
        "(Landroid/content/Intent;Landroid/content/AttributionSource;Landroid/speech/IModelDownloadListener;)V",
        &[Value::Obj(intent), Value::Null, Value::Obj(download_recorder)],
    );
    assert_eq!(
        field(&engine, download_recorder, "rd_error"),
        Value::Int(15)
    );

    // stopListening for a listener with no session reports ERROR_CLIENT.
    let session_listener = instance(&mut engine, P35_ILISTENER);
    call(
        &mut engine,
        binder,
        "stopListening",
        "(Landroid/speech/IRecognitionListener;)V",
        &[Value::Obj(session_listener)],
    );
    assert_eq!(field(&engine, session_listener, "rd_error"), Value::Int(5));

    // Callback forwards to the IRecognitionListener.
    let forwarded = instance(&mut engine, P35_ILISTENER);
    let callback = p49_new(
        &mut engine,
        P35_REC_CALLBACK,
        "(Landroid/speech/RecognitionService;Landroid/speech/IRecognitionListener;Landroid/content/AttributionSource;)V",
        &[Value::Obj(service), Value::Obj(forwarded), Value::Null],
    );
    let bundle = p41_bundle(&mut engine);
    call(
        &mut engine,
        callback,
        "readyForSpeech",
        "(Landroid/os/Bundle;)V",
        &[Value::Obj(bundle)],
    );
    assert_eq!(
        text_of(&engine, &field(&engine, forwarded, "rd_last")),
        "onReadyForSpeech"
    );
    assert_eq!(field(&engine, forwarded, "rd_bundle"), Value::Obj(bundle));
    call(
        &mut engine,
        callback,
        "rmsChanged",
        "(F)V",
        &[Value::Float(2.5)],
    );
    assert_eq!(field(&engine, forwarded, "rd_rms"), Value::Float(2.5));
    call(&mut engine, callback, "error", "(I)V", &[Value::Int(7)]);
    assert_eq!(field(&engine, forwarded, "rd_error"), Value::Int(7));
    assert_eq!(
        call(
            &mut engine,
            callback,
            "getCallingAttributionSource",
            "()Landroid/content/AttributionSource;",
            &[],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(callback, "getCallingUid", "()I", &[]),
        "Ljava/lang/NullPointerException;",
    );

    // After onDestroy the binder drops its service reference: further
    // stopListening calls are ignored.
    call(&mut engine, service, "onDestroy", "()V", &[]);
    let before = field(&engine, session_listener, "rd_count");
    call(
        &mut engine,
        binder,
        "stopListening",
        "(Landroid/speech/IRecognitionListener;)V",
        &[Value::Obj(session_listener)],
    );
    assert_eq!(field(&engine, session_listener, "rd_count"), before);
}

#[test]
fn speech_recognition_value_builders_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());

    // RecognitionSupport.Builder.
    let builder = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    let en_us = text(&mut engine, "en-US");
    let returned = call(
        &mut engine,
        builder,
        "addInstalledOnDeviceLanguage",
        "(Ljava/lang/String;)Landroid/speech/RecognitionSupport$Builder;",
        &[en_us],
    );
    assert_eq!(returned, Value::Obj(builder));
    let online = p35_string_list(&mut engine, &["en", "de"]);
    call(
        &mut engine,
        builder,
        "setOnlineLanguages",
        "(Ljava/util/List;)Landroid/speech/RecognitionSupport$Builder;",
        &[online],
    );
    let support = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/speech/RecognitionSupport;",
        &[],
    ));
    let installed = call(
        &mut engine,
        support,
        "getInstalledOnDeviceLanguages",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, installed.clone()), 1);
    assert_eq!(
        call_text(
            &mut engine,
            object(installed),
            "get",
            "(I)Ljava/lang/Object;",
            &[Value::Int(0)],
        ),
        "en-US"
    );
    let pending = call(
        &mut engine,
        support,
        "getPendingOnDeviceLanguages",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, pending), 0);
    assert_eq!(
        call_text(&mut engine, support, "toString", "()Ljava/lang/String;", &[]),
        "RecognitionSupport { installedOnDeviceLanguages = [en-US], pendingOnDeviceLanguages = [], supportedOnDeviceLanguages = [], onlineLanguages = [en, de] }"
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/speech/RecognitionSupport;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // Builders are one-shot, and explicitly-set null lists fail at build.
    let null_builder = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    call(
        &mut engine,
        null_builder,
        "setOnlineLanguages",
        "(Ljava/util/List;)Landroid/speech/RecognitionSupport$Builder;",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            null_builder,
            "build",
            "()Landroid/speech/RecognitionSupport;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // An identically-built instance is equal with the same hash.
    let builder2 = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    let en_us2 = text(&mut engine, "en-US");
    call(
        &mut engine,
        builder2,
        "addInstalledOnDeviceLanguage",
        "(Ljava/lang/String;)Landroid/speech/RecognitionSupport$Builder;",
        &[en_us2],
    );
    let online2 = p35_string_list(&mut engine, &["en", "de"]);
    call(
        &mut engine,
        builder2,
        "setOnlineLanguages",
        "(Ljava/util/List;)Landroid/speech/RecognitionSupport$Builder;",
        &[online2],
    );
    let support2 = object(call(
        &mut engine,
        builder2,
        "build",
        "()Landroid/speech/RecognitionSupport;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            support,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(support2)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, support, "hashCode", "()I", &[]),
        call(&mut engine, support2, "hashCode", "()I", &[])
    );
    assert_eq!(
        call(&mut engine, support, "describeContents", "()I", &[]),
        Value::Int(0)
    );

    // RecognitionPart.Builder.
    let raw = text(&mut engine, "hello");
    let part_builder = p49_new(
        &mut engine,
        P35_PART_BUILDER,
        "(Ljava/lang/String;)V",
        &[raw],
    );
    let part = object(call(
        &mut engine,
        part_builder,
        "build",
        "()Landroid/speech/RecognitionPart;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, part, "getRawText", "()Ljava/lang/String;", &[]),
        "hello"
    );
    assert_eq!(
        call(
            &mut engine,
            part,
            "getFormattedText",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, part, "getTimestampMillis", "()J", &[]),
        Value::Long(0)
    );
    assert_eq!(
        call(&mut engine, part, "getConfidenceLevel", "()I", &[]),
        Value::Int(0)
    );

    let raw2 = text(&mut engine, "word");
    let part_builder2 = p49_new(
        &mut engine,
        P35_PART_BUILDER,
        "(Ljava/lang/String;)V",
        &[raw2],
    );
    let formatted = text(&mut engine, "Word");
    call(
        &mut engine,
        part_builder2,
        "setFormattedText",
        "(Ljava/lang/String;)Landroid/speech/RecognitionPart$Builder;",
        &[formatted],
    );
    call(
        &mut engine,
        part_builder2,
        "setTimestampMillis",
        "(J)Landroid/speech/RecognitionPart$Builder;",
        &[Value::Long(1500)],
    );
    call(
        &mut engine,
        part_builder2,
        "setConfidenceLevel",
        "(I)Landroid/speech/RecognitionPart$Builder;",
        &[Value::Int(5)],
    );
    let part2 = object(call(
        &mut engine,
        part_builder2,
        "build",
        "()Landroid/speech/RecognitionPart;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, part2, "toString", "()Ljava/lang/String;", &[]),
        "RecognitionPart { rawText = word, formattedText = Word, timestampMillis = 1500, confidenceLevel = CONFIDENCE_LEVEL_HIGH }"
    );

    let null_part = instance(&mut engine, P35_PART_BUILDER);
    assert_throws(
        engine.invoke_instance(null_part, "<init>", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    let raw3 = text(&mut engine, "x");
    let part_builder3 = p49_new(
        &mut engine,
        P35_PART_BUILDER,
        "(Ljava/lang/String;)V",
        &[raw3],
    );
    assert_throws(
        engine.invoke_instance(
            part_builder3,
            "setFormattedText",
            "(Ljava/lang/String;)Landroid/speech/RecognitionPart$Builder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        part_builder3,
        "setConfidenceLevel",
        "(I)Landroid/speech/RecognitionPart$Builder;",
        &[Value::Int(6)],
    );
    assert_throws(
        engine.invoke_instance(
            part_builder3,
            "build",
            "()Landroid/speech/RecognitionPart;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let raw4 = text(&mut engine, "y");
    let part_builder4 = p49_new(
        &mut engine,
        P35_PART_BUILDER,
        "(Ljava/lang/String;)V",
        &[raw4],
    );
    call(
        &mut engine,
        part_builder4,
        "setTimestampMillis",
        "(J)Landroid/speech/RecognitionPart$Builder;",
        &[Value::Long(-1)],
    );
    assert_throws(
        engine.invoke_instance(
            part_builder4,
            "build",
            "()Landroid/speech/RecognitionPart;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P35_PART,
            "confidenceLevelToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(3)],
        ),
        "CONFIDENCE_LEVEL_MEDIUM"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P35_PART,
            "confidenceLevelToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(99)],
        ),
        "63"
    );

    // AlternativeSpan.
    let alternatives = p35_string_list(&mut engine, &["hello", "hallo"]);
    let span = p49_new(
        &mut engine,
        P35_SPAN,
        "(IILjava/util/List;)V",
        &[Value::Int(0), Value::Int(4), alternatives],
    );
    assert_eq!(
        call(&mut engine, span, "getStartPosition", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(&mut engine, span, "toString", "()Ljava/lang/String;", &[]),
        "AlternativeSpan { startPosition = 0, endPosition = 4, alternatives = [hello, hallo] }"
    );
    let bad_start = instance(&mut engine, P35_SPAN);
    let alternatives2 = p35_string_list(&mut engine, &["a"]);
    assert_throws(
        engine.invoke_instance(
            bad_start,
            "<init>",
            "(IILjava/util/List;)V",
            &[Value::Int(-1), Value::Int(2), alternatives2.clone()],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let bad_range = instance(&mut engine, P35_SPAN);
    assert_throws(
        engine.invoke_instance(
            bad_range,
            "<init>",
            "(IILjava/util/List;)V",
            &[Value::Int(2), Value::Int(2), alternatives2],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let empty_span = instance(&mut engine, P35_SPAN);
    let no_alternatives = p35_string_list(&mut engine, &[]);
    assert_throws(
        engine.invoke_instance(
            empty_span,
            "<init>",
            "(IILjava/util/List;)V",
            &[Value::Int(0), Value::Int(1), no_alternatives],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let null_span = instance(&mut engine, P35_SPAN);
    assert_throws(
        engine.invoke_instance(
            null_span,
            "<init>",
            "(IILjava/util/List;)V",
            &[Value::Int(0), Value::Int(1), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // No web-search activity exists, so there is no voice-details intent.
    let context = instance(&mut engine, CONTEXT);
    assert_eq!(
        call_static(
            &mut engine,
            P35_RINTENT,
            "getVoiceDetailsIntent",
            "(Landroid/content/Context;)Landroid/content/Intent;",
            &[Value::Obj(context)],
        ),
        Value::Null
    );
}

#[test]
fn spell_checker_service_session_defaults_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let service = p49_new(&mut engine, P35_SPELL, "()V", &[]);
    let binder = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Null],
    ));
    assert_eq!(engine.heap.class_of(binder).unwrap(), P35_SPELL_BINDER);
    assert!(engine
        .cp
        .is_subtype(P35_SPELL_BINDER, "Landroid/os/IBinder;"));
    let binder_again = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Null],
    ));
    assert_eq!(binder_again, binder);
    // createSession is abstract on the exact base: session creation fails.
    assert!(engine
        .invoke_instance(
            binder,
            "getISpellCheckerSession",
            "(Ljava/lang/String;Lcom/android/internal/textservice/ISpellCheckerSessionListener;Landroid/os/Bundle;ILcom/android/internal/textservice/ISpellCheckerServiceCallback;)V",
            &[Value::Null, Value::Null, Value::Null, Value::Int(0), Value::Null],
        )
        .is_err());

    let session = p49_new(&mut engine, P35_SPELL_SESSION, "()V", &[]);
    assert_throws(
        engine.invoke_instance(session, "getLocale", "()Ljava/lang/String;", &[]),
        "Ljava/lang/NullPointerException;",
    );
    call(&mut engine, session, "onCancel", "()V", &[]);
    call(&mut engine, session, "onClose", "()V", &[]);

    let empty_infos = object_array(&mut engine, "[Landroid/view/textservice/TextInfo;", vec![]);
    let multiple = call(
        &mut engine,
        session,
        "onGetSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;IZ)[Landroid/view/textservice/SuggestionsInfo;",
        &[empty_infos.clone(), Value::Int(5), Value::Int(0)],
    );
    assert_eq!(p35_array_len(&engine, &multiple), 0);
    assert_throws(
        engine.invoke_instance(
            session,
            "onGetSuggestionsMultiple",
            "([Landroid/view/textservice/TextInfo;IZ)[Landroid/view/textservice/SuggestionsInfo;",
            &[Value::Null, Value::Int(5), Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let sentences = call(
        &mut engine,
        session,
        "onGetSentenceSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;I)[Landroid/view/textservice/SentenceSuggestionsInfo;",
        &[Value::Null, Value::Int(5)],
    );
    assert_eq!(p35_array_len(&engine, &sentences), 0);

    // Installing an internal session with an empty locale: getters work and
    // sentence-level requests return the empty result.
    let listener = instance(&mut engine, P35_SPELL_LISTENER);
    let bundle = p41_bundle(&mut engine);
    let empty_locale = text(&mut engine, "");
    let internal = p49_new(
        &mut engine,
        P35_SPELL_INTERNAL,
        "(Ljava/lang/String;Lcom/android/internal/textservice/ISpellCheckerSessionListener;Landroid/os/Bundle;Landroid/service/textservice/SpellCheckerService$Session;I)V",
        &[
            empty_locale,
            Value::Obj(listener),
            Value::Obj(bundle),
            Value::Obj(session),
            Value::Int(3),
        ],
    );
    assert_eq!(
        call_text(
            &mut engine,
            session,
            "getLocale",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    assert_eq!(
        call(
            &mut engine,
            session,
            "getBundle",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Obj(bundle)
    );
    assert_eq!(
        call(&mut engine, session, "getSupportedAttributes", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, internal, "getSupportedAttributes", "()I", &[]),
        Value::Int(3)
    );

    call(
        &mut engine,
        internal,
        "onGetSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;IZ)V",
        &[empty_infos.clone(), Value::Int(5), Value::Int(1)],
    );
    assert_eq!(
        text_of(&engine, &field(&engine, listener, "rd_last")),
        "onGetSuggestions"
    );
    assert_eq!(
        p35_array_len(&engine, &field(&engine, listener, "rd_suggestions")),
        0
    );

    let word = text(&mut engine, "helo");
    let info = p49_new(
        &mut engine,
        "Landroid/view/textservice/TextInfo;",
        "(Ljava/lang/String;II)V",
        &[word, Value::Int(7), Value::Int(11)],
    );
    let infos = object_array(
        &mut engine,
        "[Landroid/view/textservice/TextInfo;",
        vec![Value::Obj(info)],
    );
    call(
        &mut engine,
        internal,
        "onGetSentenceSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;I)V",
        &[infos, Value::Int(5)],
    );
    assert_eq!(
        text_of(&engine, &field(&engine, listener, "rd_last")),
        "onGetSentenceSuggestions"
    );
    assert_eq!(
        p35_array_len(
            &engine,
            &field(&engine, listener, "rd_sentence_suggestions")
        ),
        0
    );

    call(&mut engine, internal, "onCancel", "()V", &[]);
    call(&mut engine, internal, "onClose", "()V", &[]);
    // onClose nulls the listener, so a later delivery dereferences null.
    assert_throws(
        engine.invoke_instance(
            internal,
            "onGetSuggestionsMultiple",
            "([Landroid/view/textservice/TextInfo;IZ)V",
            &[empty_infos, Value::Int(5), Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn speech_and_textservice_deep_audit_fixes_are_guest_visible() {
    // A guest listener/callback whose delivery methods throw
    // RemoteException, which AOSP's spell-checker IPC wrappers swallow.
    let mut dex = DexBuilder::new();
    let throwing = dex.class("LDeepAuditSpellListener;", "Ljava/lang/Object;");
    let remote_type = dex.type_idx("Landroid/os/RemoteException;");
    let make_throw = |dex: &DexBuilder| {
        // The platform RemoteException constructor is outside the modeled
        // classpath; throwing the freshly allocated instance still carries
        // the exact throwable class through the VM.
        let mut code = op21c(0x22, 0, remote_type);
        code.extend(op11x(0x27, 0));
        dex.code(3, 2, 1, code)
    };
    let throw_suggestions = make_throw(&dex);
    dex.r#virtual(
        throwing,
        "onGetSuggestions",
        "V",
        vec!["[Landroid/view/textservice/SuggestionsInfo;"],
        ACC_PUBLIC,
        Some(throw_suggestions),
    );
    let throw_created = make_throw(&dex);
    dex.r#virtual(
        throwing,
        "onSessionCreated",
        "V",
        vec!["Lcom/android/internal/textservice/ISpellCheckerSession;"],
        ACC_PUBLIC,
        Some(throw_created),
    );

    // Minimal Context/PackageManager overrides returning test fields, used
    // to reach RecognizerIntent's resolved-activity path.
    let audit_context = dex.class("LDeepAuditContext;", "Landroid/content/Context;");
    dex.instance_field(
        audit_context,
        "rd_pm",
        "Landroid/content/pm/PackageManager;",
    );
    let pm_field = dex.field_idx(
        "LDeepAuditContext;",
        "Landroid/content/pm/PackageManager;",
        "rd_pm",
    );
    let mut get_pm_code = op22c(0x54, 0, 1, pm_field); // iget-object v0, v1(this)
    get_pm_code.extend(op11x(0x11, 0)); // return-object v0
    let get_pm_blob = dex.code(2, 1, 0, get_pm_code);
    dex.r#virtual(
        audit_context,
        "getPackageManager",
        "Landroid/content/pm/PackageManager;",
        vec![],
        ACC_PUBLIC,
        Some(get_pm_blob),
    );
    // The host path only needs virtual dispatch to resolveActivity; use a
    // plain DEX test double rather than modelling PackageManager's full
    // abstract platform hierarchy in this fixture.
    let audit_pm = dex.class("LDeepAuditPackageManager;", "Ljava/lang/Object;");
    dex.instance_field(audit_pm, "rd_result", "Landroid/content/ContentResolver;");
    let result_field = dex.field_idx(
        "LDeepAuditPackageManager;",
        "Landroid/content/ContentResolver;",
        "rd_result",
    );
    let mut resolve_code = op22c(0x54, 0, 1, result_field); // iget-object v0, v1(this)
    resolve_code.extend(op11x(0x11, 0)); // return-object v0
    let resolve_blob = dex.code(4, 3, 0, resolve_code);
    dex.r#virtual(
        audit_pm,
        "resolveActivity",
        "Landroid/content/ContentResolver;",
        vec!["Landroid/content/Intent;", "I"],
        ACC_PUBLIC,
        Some(resolve_blob),
    );
    let mut engine = engine_of(&dex);
    // RecognitionSupport defaults are List.of(): immutable on the built
    // object and assigned back into the builder, so a post-build add*
    // hits UnsupportedOperationException rather than checkNotUsed.
    let builder = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    let support = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/speech/RecognitionSupport;",
        &[],
    ));
    let pending = object(call(
        &mut engine,
        support,
        "getPendingOnDeviceLanguages",
        "()Ljava/util/List;",
        &[],
    ));
    let de = text(&mut engine, "de");
    assert_throws(
        engine.invoke_instance(
            pending,
            "add",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(&de),
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "addPendingOnDeviceLanguage",
            "(Ljava/lang/String;)Landroid/speech/RecognitionSupport$Builder;",
            &[de],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );

    // Defaults for every unset field are assigned before the generated
    // constructor rejects an explicitly-set null list.
    let failed_builder = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    call(
        &mut engine,
        failed_builder,
        "setInstalledOnDeviceLanguages",
        "(Ljava/util/List;)Landroid/speech/RecognitionSupport$Builder;",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            failed_builder,
            "build",
            "()Landroid/speech/RecognitionSupport;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let fr = text(&mut engine, "fr");
    assert_throws(
        engine.invoke_instance(
            failed_builder,
            "addPendingOnDeviceLanguage",
            "(Ljava/lang/String;)Landroid/speech/RecognitionSupport$Builder;",
            &[fr],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );

    // The generated add* methods only call the checked setter when the
    // current list is null; adding to an explicitly-set list after build
    // therefore still mutates the built object's list.
    let explicit_builder = p49_new(&mut engine, P35_SUPPORT_BUILDER, "()V", &[]);
    let online = p35_string_list(&mut engine, &["en"]);
    call(
        &mut engine,
        explicit_builder,
        "setOnlineLanguages",
        "(Ljava/util/List;)Landroid/speech/RecognitionSupport$Builder;",
        &[online],
    );
    let explicit_support = object(call(
        &mut engine,
        explicit_builder,
        "build",
        "()Landroid/speech/RecognitionSupport;",
        &[],
    ));
    let es = text(&mut engine, "es");
    call(
        &mut engine,
        explicit_builder,
        "addOnlineLanguage",
        "(Ljava/lang/String;)Landroid/speech/RecognitionSupport$Builder;",
        &[es],
    );
    let online_after = call(
        &mut engine,
        explicit_support,
        "getOnlineLanguages",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, online_after), 2);

    // AlternativeSpan passes its sentence as the *name* argument to
    // Preconditions.checkCollectionNotEmpty, yielding this exact text.
    let empty_span = instance(&mut engine, P35_SPAN);
    let no_alternatives = p35_string_list(&mut engine, &[]);
    match engine.invoke_instance(
        empty_span,
        "<init>",
        "(IILjava/util/List;)V",
        &[Value::Int(0), Value::Int(1), no_alternatives],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;");
            assert_eq!(
                t.message.as_deref(),
                Some("List of alternative strings must not be empty. is empty")
            );
        }
        other => panic!("expected empty-alternatives failure, got {other:?}"),
    }

    // A resolved voice-search activity with details metadata but a null
    // packageName reaches new ComponentName(null, ...) in AOSP and NPEs,
    // rather than making getVoiceDetailsIntent return null.
    let details_bundle = p41_bundle(&mut engine);
    let details_key = text(&mut engine, "android.speech.DETAILS");
    let details_class = text(&mut engine, "com.example.Details");
    call(
        &mut engine,
        details_bundle,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[details_key, details_class],
    );
    let activity_info = instance(&mut engine, "Landroid/content/pm/ActivityInfo;");
    engine
        .heap
        .put_field(activity_info, "metaData", Value::Obj(details_bundle))
        .unwrap();
    engine
        .heap
        .put_field(activity_info, "packageName", Value::Null)
        .unwrap();
    let resolve_info = instance(&mut engine, "Landroid/content/ContentResolver;");
    engine
        .heap
        .put_field(resolve_info, "activityInfo", Value::Obj(activity_info))
        .unwrap();
    let audit_pm = instance(&mut engine, "LDeepAuditPackageManager;");
    engine
        .heap
        .put_field(audit_pm, "rd_result", Value::Obj(resolve_info))
        .unwrap();
    let audit_context = instance(&mut engine, "LDeepAuditContext;");
    engine
        .heap
        .put_field(audit_context, "rd_pm", Value::Obj(audit_pm))
        .unwrap();
    match engine.invoke_static(
        P35_RINTENT,
        "getVoiceDetailsIntent",
        "(Landroid/content/Context;)Landroid/content/Intent;",
        &[Value::Obj(audit_context)],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;");
            assert_eq!(t.message.as_deref(), Some("package name is null"));
        }
        other => panic!("expected null-package ComponentName failure, got {other:?}"),
    }

    // Sentence-level empty results are the one shared
    // EMPTY_SENTENCE_SUGGESTIONS_INFOS array instance.
    let session = p49_new(&mut engine, P35_SPELL_SESSION, "()V", &[]);
    let empty_once = call(
        &mut engine,
        session,
        "onGetSentenceSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;I)[Landroid/view/textservice/SentenceSuggestionsInfo;",
        &[Value::Null, Value::Int(5)],
    );
    let empty_infos = object_array(&mut engine, "[Landroid/view/textservice/TextInfo;", vec![]);
    let empty_twice = call(
        &mut engine,
        session,
        "onGetSentenceSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;I)[Landroid/view/textservice/SentenceSuggestionsInfo;",
        &[empty_infos.clone(), Value::Int(5)],
    );
    assert_eq!(empty_once, empty_twice);

    // InternalISpellCheckerSession swallows only RemoteException from
    // result delivery.
    let throwing_listener = instance(&mut engine, "LDeepAuditSpellListener;");
    let bundle = p41_bundle(&mut engine);
    let empty_locale = text(&mut engine, "");
    let internal = p49_new(
        &mut engine,
        P35_SPELL_INTERNAL,
        "(Ljava/lang/String;Lcom/android/internal/textservice/ISpellCheckerSessionListener;Landroid/os/Bundle;Landroid/service/textservice/SpellCheckerService$Session;I)V",
        &[
            empty_locale,
            Value::Obj(throwing_listener),
            Value::Obj(bundle),
            Value::Obj(session),
            Value::Int(0),
        ],
    );
    call(
        &mut engine,
        internal,
        "onGetSuggestionsMultiple",
        "([Landroid/view/textservice/TextInfo;IZ)V",
        &[empty_infos, Value::Int(5), Value::Int(0)],
    );

    // The binder likewise swallows RemoteException from onSessionCreated
    // (here the service reference is gone, so the session is null).
    let binder = p49_new(
        &mut engine,
        P35_SPELL_BINDER,
        "(Landroid/service/textservice/SpellCheckerService;)V",
        &[Value::Null],
    );
    let throwing_callback = instance(&mut engine, "LDeepAuditSpellListener;");
    call(
        &mut engine,
        binder,
        "getISpellCheckerSession",
        "(Ljava/lang/String;Lcom/android/internal/textservice/ISpellCheckerSessionListener;Landroid/os/Bundle;ILcom/android/internal/textservice/ISpellCheckerServiceCallback;)V",
        &[
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Obj(throwing_callback),
        ],
    );
}

// ---------------------------------------------------------------------------
// Appendix C Batch 35-B: android.speech.tts + android.service.voice.
// ---------------------------------------------------------------------------

const P35B_TTS: &str = "Landroid/speech/tts/TextToSpeech;";
const P35B_TTS_ON_INIT: &str = "Landroid/speech/tts/TextToSpeech$OnInitListener;";
const P35B_ENGINE_INFO: &str = "Landroid/speech/tts/TextToSpeech$EngineInfo;";
const P35B_VOICE: &str = "Landroid/speech/tts/Voice;";
const P35B_SYNTHESIS_REQUEST: &str = "Landroid/speech/tts/SynthesisRequest;";
const P35B_UPL: &str = "Landroid/speech/tts/UtteranceProgressListener;";
const P35B_TTS_SERVICE: &str = "Landroid/speech/tts/TextToSpeechService;";
const P35B_VIS: &str = "Landroid/service/voice/VoiceInteractionService;";
const P35B_VIS_STUB: &str = "Landroid/service/voice/IVoiceInteractionService$Stub;";
const P35B_SESSION: &str = "Landroid/service/voice/VoiceInteractionSession;";
const P35B_ASSIST_STATE: &str = "Landroid/service/voice/VoiceInteractionSession$AssistState;";
const P35B_ACTIVITY_ID: &str = "Landroid/service/voice/VoiceInteractionSession$ActivityId;";
const P35B_DETECTED: &str = "Landroid/service/voice/HotwordDetectedResult;";
const P35B_DETECTED_BUILDER: &str = "Landroid/service/voice/HotwordDetectedResult$Builder;";
const P35B_REJECTED_BUILDER: &str = "Landroid/service/voice/HotwordRejectedResult$Builder;";

fn p35b_locale(engine: &mut Engine, language: &str, country: &str) -> ObjRef {
    let language = text(engine, language);
    let country = text(engine, country);
    p49_new(
        engine,
        "Ljava/util/Locale;",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[language, country],
    )
}

#[test]
fn text_to_speech_without_an_engine_and_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);

    // A null context is dereferenced while looking for engines.
    let null_tts = instance(&mut engine, P35B_TTS);
    assert_throws(
        engine.invoke_instance(
            null_tts,
            "<init>",
            "(Landroid/content/Context;Landroid/speech/tts/TextToSpeech$OnInitListener;)V",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // No engine is installed: onInit(ERROR) fires exactly once.
    let init_listener = instance(&mut engine, P35B_TTS_ON_INIT);
    let tts = p49_new(
        &mut engine,
        P35B_TTS,
        "(Landroid/content/Context;Landroid/speech/tts/TextToSpeech$OnInitListener;)V",
        &[Value::Obj(context), Value::Obj(init_listener)],
    );
    assert_eq!(field(&engine, init_listener, "rd_status"), Value::Int(-1));
    assert_eq!(field(&engine, init_listener, "rd_count"), Value::Int(1));
    assert_eq!(
        call(
            &mut engine,
            tts,
            "getCurrentEngine",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "getDefaultEngine",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    let engines = call(&mut engine, tts, "getEngines", "()Ljava/util/List;", &[]);
    assert_eq!(p57_collection_size(&mut engine, engines), 0);

    // Every service-backed call returns its AOSP no-connection result.
    let utterance = text(&mut engine, "hello");
    assert_eq!(
        call(
            &mut engine,
            tts,
            "speak",
            "(Ljava/lang/CharSequence;ILandroid/os/Bundle;Ljava/lang/String;)I",
            &[utterance.clone(), Value::Int(0), Value::Null, Value::Null],
        ),
        Value::Int(-1)
    );
    assert_eq!(call(&mut engine, tts, "stop", "()I", &[]), Value::Int(-1));
    assert_eq!(
        call(&mut engine, tts, "isSpeaking", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "synthesizeToFile",
            "(Ljava/lang/CharSequence;Landroid/os/Bundle;Landroid/os/ParcelFileDescriptor;Ljava/lang/String;)I",
            &[utterance, Value::Null, Value::Null, Value::Null],
        ),
        Value::Int(-1)
    );

    let locale = p35b_locale(&mut engine, "en", "US");
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setLanguage",
            "(Ljava/util/Locale;)I",
            &[Value::Obj(locale)]
        ),
        Value::Int(-2)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "isLanguageAvailable",
            "(Ljava/util/Locale;)I",
            &[Value::Obj(locale)],
        ),
        Value::Int(-2)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "getVoice",
            "()Landroid/speech/tts/Voice;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, tts, "getVoices", "()Ljava/util/Set;", &[]),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "getFeatures",
            "(Ljava/util/Locale;)Ljava/util/Set;",
            &[Value::Obj(locale)]
        ),
        Value::Null
    );

    // Local parameter bookkeeping still works without an engine.
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setSpeechRate",
            "(F)I",
            &[Value::Float(1.5)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setSpeechRate",
            "(F)I",
            &[Value::Float(0.0)]
        ),
        Value::Int(-1)
    );
    // (int)(0.001f * 100) == 0, so AOSP rejects it.
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setSpeechRate",
            "(F)I",
            &[Value::Float(0.001)]
        ),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, tts, "setPitch", "(F)I", &[Value::Float(2.0)]),
        Value::Int(0)
    );
    let params = object(field(&engine, tts, "mParams"));
    let rate_key = text(&mut engine, "rate");
    assert_eq!(
        call(
            &mut engine,
            params,
            "getInt",
            "(Ljava/lang/String;)I",
            &[rate_key]
        ),
        Value::Int(150)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setAudioAttributes",
            "(Landroid/media/AudioAttributes;)I",
            &[Value::Null],
        ),
        Value::Int(-1)
    );

    let spoken = text(&mut engine, "beep");
    let package = text(&mut engine, "com.example.app");
    assert_eq!(
        call(
            &mut engine,
            tts,
            "addSpeech",
            "(Ljava/lang/String;Ljava/lang/String;I)I",
            &[spoken, package, Value::Int(7)],
        ),
        Value::Int(0)
    );
    let earcon = text(&mut engine, "ding");
    let uri_text = text(&mut engine, "file:///ding.wav");
    let uri = call_static(
        &mut engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[uri_text],
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "addEarcon",
            "(Ljava/lang/String;Landroid/net/Uri;)I",
            &[earcon, uri],
        ),
        Value::Int(0)
    );

    assert_eq!(
        call(&mut engine, tts, "areDefaultsEnforced", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_static(&mut engine, P35B_TTS, "getMaxSpeechInputLength", "()I", &[]),
        Value::Int(4000)
    );
    // Re-running init fails again but the consumed listener is not called.
    let other_engine = text(&mut engine, "com.example.tts");
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setEngineByPackageName",
            "(Ljava/lang/String;)I",
            &[other_engine]
        ),
        Value::Int(-1)
    );
    assert_eq!(field(&engine, init_listener, "rd_count"), Value::Int(1));
    call(&mut engine, tts, "shutdown", "()V", &[]);

    // Voice value object.
    let features = p49_new(&mut engine, "Ljava/util/HashSet;", "()V", &[]);
    let feature = text(&mut engine, "networkSynthesis");
    call(
        &mut engine,
        features,
        "add",
        "(Ljava/lang/Object;)Z",
        &[feature],
    );
    let voice_name = text(&mut engine, "en-US");
    let voice = p49_new(
        &mut engine,
        P35B_VOICE,
        "(Ljava/lang/String;Ljava/util/Locale;IIZLjava/util/Set;)V",
        &[
            voice_name,
            Value::Obj(locale),
            Value::Int(300),
            Value::Int(200),
            Value::Int(1),
            Value::Obj(features),
        ],
    );
    assert_eq!(
        call_text(&mut engine, voice, "getName", "()Ljava/lang/String;", &[]),
        "en-US"
    );
    assert_eq!(
        call(&mut engine, voice, "getQuality", "()I", &[]),
        Value::Int(300)
    );
    assert_eq!(
        call(
            &mut engine,
            voice,
            "isNetworkConnectionRequired",
            "()Z",
            &[]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            tts,
            "setVoice",
            "(Landroid/speech/tts/Voice;)I",
            &[Value::Obj(voice)]
        ),
        Value::Int(-2)
    );
    let voice_name2 = text(&mut engine, "en-US");
    let features2 = p49_new(&mut engine, "Ljava/util/HashSet;", "()V", &[]);
    let feature2 = text(&mut engine, "networkSynthesis");
    call(
        &mut engine,
        features2,
        "add",
        "(Ljava/lang/Object;)Z",
        &[feature2],
    );
    let voice2 = p49_new(
        &mut engine,
        P35B_VOICE,
        "(Ljava/lang/String;Ljava/util/Locale;IIZLjava/util/Set;)V",
        &[
            voice_name2,
            Value::Obj(locale),
            Value::Int(300),
            Value::Int(200),
            Value::Int(1),
            Value::Obj(features2),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            voice,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(voice2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, voice, "hashCode", "()I", &[]),
        call(&mut engine, voice2, "hashCode", "()I", &[])
    );
    // toString() dereferences mFeatures directly.
    let no_features = p49_new(
        &mut engine,
        P35B_VOICE,
        "(Ljava/lang/String;Ljava/util/Locale;IIZLjava/util/Set;)V",
        &[
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Null,
        ],
    );
    assert_throws(
        engine.invoke_instance(no_features, "toString", "()Ljava/lang/String;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    // EngineInfo defaults.
    let engine_info = p49_new(&mut engine, P35B_ENGINE_INFO, "()V", &[]);
    assert_eq!(
        call_text(
            &mut engine,
            engine_info,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "EngineInfo{name=null}"
    );

    // SynthesisRequest copies its params bundle.
    let bundle = p41_bundle(&mut engine);
    let request_text = text(&mut engine, "say this");
    let request = p49_new(
        &mut engine,
        P35B_SYNTHESIS_REQUEST,
        "(Ljava/lang/CharSequence;Landroid/os/Bundle;)V",
        &[request_text, Value::Obj(bundle)],
    );
    assert_eq!(
        call_text(&mut engine, request, "getText", "()Ljava/lang/String;", &[]),
        "say this"
    );
    let copied_params = object(call(
        &mut engine,
        request,
        "getParams",
        "()Landroid/os/Bundle;",
        &[],
    ));
    assert_ne!(copied_params, bundle);
    assert_eq!(
        call(&mut engine, request, "getSpeechRate", "()I", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        request,
        "setSpeechRate",
        "(I)V",
        &[Value::Int(150)],
    );
    assert_eq!(
        call(&mut engine, request, "getSpeechRate", "()I", &[]),
        Value::Int(150)
    );
    let language = text(&mut engine, "eng");
    let country = text(&mut engine, "USA");
    let variant = text(&mut engine, "");
    call(
        &mut engine,
        request,
        "setLanguage",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[language, country, variant],
    );
    assert_eq!(
        call_text(
            &mut engine,
            request,
            "getLanguage",
            "()Ljava/lang/String;",
            &[]
        ),
        "eng"
    );
    assert_eq!(
        call(&mut engine, request, "getCallerUid", "()I", &[]),
        Value::Int(0)
    );
    let null_request = instance(&mut engine, P35B_SYNTHESIS_REQUEST);
    let some_text = text(&mut engine, "x");
    assert_throws(
        engine.invoke_instance(
            null_request,
            "<init>",
            "(Ljava/lang/CharSequence;Landroid/os/Bundle;)V",
            &[some_text, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // UtteranceProgressListener base-class defaults.
    let progress = p49_new(&mut engine, P35B_UPL, "()V", &[]);
    let id = text(&mut engine, "utt");
    call(
        &mut engine,
        progress,
        "onStop",
        "(Ljava/lang/String;Z)V",
        &[id.clone(), Value::Int(1)],
    );
    call(
        &mut engine,
        progress,
        "onBeginSynthesis",
        "(Ljava/lang/String;III)V",
        &[id.clone(), Value::Int(22050), Value::Int(2), Value::Int(1)],
    );
    // The coded-error overload delegates to the abstract onError(String).
    assert!(engine
        .invoke_instance(
            progress,
            "onError",
            "(Ljava/lang/String;I)V",
            &[id, Value::Int(1)]
        )
        .is_err());

    // TextToSpeechService base defaults.
    let service = p49_new(&mut engine, P35B_TTS_SERVICE, "()V", &[]);
    let tts_intent = p41_intent(&mut engine, "android.intent.action.TTS_SERVICE");
    let binder = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(tts_intent)],
    ));
    assert_eq!(
        engine.heap.class_of(binder).unwrap(),
        "Landroid/speech/tts/ITextToSpeechService$Stub;"
    );
    assert!(engine.cp.is_subtype(
        "Landroid/speech/tts/ITextToSpeechService$Stub;",
        "Landroid/os/IBinder;"
    ));
    let binder_again = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(tts_intent)],
    ));
    assert_eq!(binder_again, binder);
    let features = call(
        &mut engine,
        service,
        "onGetFeaturesForLanguage",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/util/Set;",
        &[Value::Null, Value::Null, Value::Null],
    );
    assert_eq!(p57_collection_size(&mut engine, features), 0);
}

#[test]
fn voice_interaction_service_and_session_before_ready_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);

    let service = p49_new(&mut engine, P35B_VIS, "()V", &[]);
    let wrong_intent = p41_intent(&mut engine, "android.intent.action.MAIN");
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong_intent)],
        ),
        Value::Null
    );
    let service_intent = p41_intent(&mut engine, "android.service.voice.VoiceInteractionService");
    let binder = object(call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(service_intent)],
    ));
    assert_eq!(engine.heap.class_of(binder).unwrap(), P35B_VIS_STUB);
    assert_throws(
        engine.invoke_instance(
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // There is no voice interaction manager: onReady() fails and every
    // guarded call keeps throwing the pre-ready exception.
    assert_throws(
        engine.invoke_instance(service, "onReady", "()V", &[]),
        "Ljava/lang/NullPointerException;",
    );
    let bundle = p41_bundle(&mut engine);
    assert_throws(
        engine.invoke_instance(
            service,
            "showSession",
            "(Landroid/os/Bundle;I)V",
            &[Value::Obj(bundle), Value::Int(0)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_eq!(
        call(
            &mut engine,
            service,
            "getKeyphraseEnrollmentInfo",
            "()Landroid/hardware/soundtrigger/KeyphraseEnrollmentInfo;",
            &[],
        ),
        Value::Null
    );
    let locale = p35b_locale(&mut engine, "en", "US");
    let keyphrase = text(&mut engine, "hey assistant");
    assert_eq!(
        call(
            &mut engine,
            service,
            "isKeyphraseAndLocaleSupportedForHotword",
            "(Ljava/lang/String;Ljava/util/Locale;)Z",
            &[keyphrase.clone(), Value::Obj(locale)],
        ),
        Value::Int(0)
    );
    let actions = call(
        &mut engine,
        service,
        "onGetSupportedVoiceActions",
        "(Ljava/util/Set;)Ljava/util/Set;",
        &[Value::Null],
    );
    assert_eq!(p57_collection_size(&mut engine, actions), 0);
    assert_throws(
        engine.invoke_instance(
            service,
            "setUiHints",
            "(Landroid/os/Bundle;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "setUiHints",
            "(Landroid/os/Bundle;)V",
            &[Value::Obj(bundle)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(service, "getDisabledShowContext", "()I", &[]),
        "Ljava/lang/NullPointerException;",
    );

    assert_throws(
        engine.invoke_instance(
            service,
            "createAlwaysOnHotwordDetector",
            "(Ljava/lang/String;Ljava/util/Locale;Landroid/service/voice/AlwaysOnHotwordDetector$Callback;)Landroid/service/voice/AlwaysOnHotwordDetector;",
            &[keyphrase.clone(), Value::Obj(locale), Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // The Executor overload null-checks its arguments first.
    assert_throws(
        engine.invoke_instance(
            service,
            "createAlwaysOnHotwordDetector",
            "(Ljava/lang/String;Ljava/util/Locale;Ljava/util/concurrent/Executor;Landroid/service/voice/AlwaysOnHotwordDetector$Callback;)Landroid/service/voice/AlwaysOnHotwordDetector;",
            &[Value::Null, Value::Obj(locale), Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let executor = instance(&mut engine, "Ljava/util/concurrent/Executor;");
    let callback = instance(
        &mut engine,
        "Landroid/service/voice/AlwaysOnHotwordDetector$Callback;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "createAlwaysOnHotwordDetector",
            "(Ljava/lang/String;Ljava/util/Locale;Ljava/util/concurrent/Executor;Landroid/service/voice/AlwaysOnHotwordDetector$Callback;)Landroid/service/voice/AlwaysOnHotwordDetector;",
            &[keyphrase, Value::Obj(locale), Value::Obj(executor), Value::Obj(callback)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "createHotwordDetector",
            "(Landroid/os/PersistableBundle;Landroid/os/SharedMemory;Landroid/service/voice/HotwordDetector$Callback;)Landroid/service/voice/HotwordDetector;",
            &[Value::Null, Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "createKeyphraseModelManager",
            "()Landroid/media/voice/KeyphraseModelManager;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // No voice interaction service is configured in Settings.Secure.
    assert_eq!(
        call_static(
            &mut engine,
            P35B_VIS,
            "isActiveService",
            "(Landroid/content/Context;Landroid/content/ComponentName;)Z",
            &[Value::Obj(context), Value::Null],
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P35B_VIS,
            "isActiveService",
            "(Landroid/content/Context;Landroid/content/ComponentName;)Z",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // An unattached session: show/hide/finish are illegal until onCreate.
    let session = p49_new(
        &mut engine,
        P35B_SESSION,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert!(matches!(
        call(
            &mut engine,
            session,
            "getContext",
            "()Landroid/content/Context;",
            &[]
        ),
        Value::Obj(_)
    ));
    assert_throws(
        engine.invoke_instance(session, "hide", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            session,
            "show",
            "(Landroid/os/Bundle;I)V",
            &[Value::Obj(bundle), Value::Int(1)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(session, "finish", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    // onBackPressed() delegates to hide().
    assert_throws(
        engine.invoke_instance(session, "onBackPressed", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(session, "getDisabledShowContext", "()I", &[]),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call(
            &mut engine,
            session,
            "onCreateContentView",
            "()Landroid/view/View;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            session,
            "onKeyDown",
            "(ILandroid/view/KeyEvent;)Z",
            &[Value::Int(4), Value::Null]
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        session,
        "setUiEnabled",
        "(Z)V",
        &[Value::Int(0)],
    );
    call(&mut engine, session, "setTheme", "(I)V", &[Value::Int(7)]);
    call(&mut engine, session, "onCreate", "()V", &[]);
    call(&mut engine, session, "onHide", "()V", &[]);

    let command_a = text(&mut engine, "com.example.A");
    let command_b = text(&mut engine, "com.example.B");
    let commands = object_array(
        &mut engine,
        "[Ljava/lang/String;",
        vec![command_a, command_b],
    );
    let supported = call(
        &mut engine,
        session,
        "onGetSupportedCommands",
        "([Ljava/lang/String;)[Z",
        &[commands],
    );
    assert_eq!(p35_array_len(&engine, &supported), 2);
    assert_throws(
        engine.invoke_instance(
            session,
            "onGetSupportedCommands",
            "([Ljava/lang/String;)[Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // AssistState/ActivityId plumbing used by onHandleAssist.
    let activity_id = p49_new(
        &mut engine,
        P35B_ACTIVITY_ID,
        "(ILandroid/os/IBinder;)V",
        &[Value::Int(42), Value::Obj(binder)],
    );
    assert_eq!(
        call(&mut engine, activity_id, "getTaskId", "()I", &[]),
        Value::Int(42)
    );
    assert_eq!(
        call(
            &mut engine,
            activity_id,
            "getAssistToken",
            "()Landroid/os/IBinder;",
            &[]
        ),
        Value::Obj(binder)
    );
    let activity_id2 = p49_new(
        &mut engine,
        P35B_ACTIVITY_ID,
        "(ILandroid/os/IBinder;)V",
        &[Value::Int(42), Value::Obj(binder)],
    );
    assert_eq!(
        call(
            &mut engine,
            activity_id,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(activity_id2)]
        ),
        Value::Int(1)
    );
    let state_sig =
        "(Landroid/service/voice/VoiceInteractionSession$ActivityId;Landroid/os/Bundle;Landroid/app/assist/AssistStructure;Landroid/app/assist/AssistContent;II)V";
    let empty_state = p49_new(
        &mut engine,
        P35B_ASSIST_STATE,
        state_sig,
        &[
            Value::Obj(activity_id),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(&mut engine, empty_state, "isFocused", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            empty_state,
            "getAssistData",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Null
    );
    // All-null assist data returns without delegating.
    call(
        &mut engine,
        session,
        "onHandleAssist",
        "(Landroid/service/voice/VoiceInteractionSession$AssistState;)V",
        &[Value::Obj(empty_state)],
    );
    let focused_state = p49_new(
        &mut engine,
        P35B_ASSIST_STATE,
        state_sig,
        &[
            Value::Obj(activity_id),
            Value::Obj(bundle),
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Int(2),
        ],
    );
    call(
        &mut engine,
        session,
        "onHandleAssist",
        "(Landroid/service/voice/VoiceInteractionSession$AssistState;)V",
        &[Value::Obj(focused_state)],
    );
    let secondary_state = p49_new(
        &mut engine,
        P35B_ASSIST_STATE,
        state_sig,
        &[
            Value::Obj(activity_id),
            Value::Obj(bundle),
            Value::Null,
            Value::Null,
            Value::Int(1),
            Value::Int(2),
        ],
    );
    assert_eq!(
        call(&mut engine, secondary_state, "isFocused", "()Z", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        session,
        "onHandleAssist",
        "(Landroid/service/voice/VoiceInteractionSession$AssistState;)V",
        &[Value::Obj(secondary_state)],
    );
}

#[test]
fn hotword_result_builders_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());

    // HotwordRejectedResult.
    let builder = p49_new(&mut engine, P35B_REJECTED_BUILDER, "()V", &[]);
    let rejected = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/service/voice/HotwordRejectedResult;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, rejected, "getConfidenceLevel", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            rejected,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "HotwordRejectedResult { confidenceLevel = 0 }"
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/service/voice/HotwordRejectedResult;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let builder2 = p49_new(&mut engine, P35B_REJECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        builder2,
        "setConfidenceLevel",
        "(I)Landroid/service/voice/HotwordRejectedResult$Builder;",
        &[Value::Int(2)],
    );
    let rejected2 = object(call(
        &mut engine,
        builder2,
        "build",
        "()Landroid/service/voice/HotwordRejectedResult;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            rejected2,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "HotwordRejectedResult { confidenceLevel = 2 }"
    );
    let builder3 = p49_new(&mut engine, P35B_REJECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        builder3,
        "setConfidenceLevel",
        "(I)Landroid/service/voice/HotwordRejectedResult$Builder;",
        &[Value::Int(4)],
    );
    // The @IntDef confidence is not runtime-validated in AOSP.
    let rejected3 = object(call(
        &mut engine,
        builder3,
        "build",
        "()Landroid/service/voice/HotwordRejectedResult;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, rejected3, "getConfidenceLevel", "()I", &[]),
        Value::Int(4)
    );

    // HotwordDetectedResult defaults.
    let detected_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    let detected = object(call(
        &mut engine,
        detected_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, detected, "getConfidenceLevel", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, detected, "getHotwordOffsetMillis", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, detected, "getAudioChannel", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, detected, "getScore", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, detected, "getBackgroundAudioPower", "()I", &[]),
        Value::Int(-1)
    );
    let streams = call(
        &mut engine,
        detected,
        "getAudioStreams",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, streams), 0);
    assert!(matches!(
        call(
            &mut engine,
            detected,
            "getExtras",
            "()Landroid/os/PersistableBundle;",
            &[]
        ),
        Value::Obj(_)
    ));
    assert_eq!(
        call(&mut engine, detected, "getProximity", "()I", &[]),
        Value::Int(-1)
    );
    call(
        &mut engine,
        detected,
        "setProximity",
        "(D)V",
        &[Value::Double(1.5)],
    );
    assert_eq!(
        call(&mut engine, detected, "getProximity", "()I", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        detected,
        "setProximity",
        "(D)V",
        &[Value::Double(9.0)],
    );
    assert_eq!(
        call(&mut engine, detected, "getProximity", "()I", &[]),
        Value::Int(2)
    );
    // Unknown proximity is never written back into the extras.
    call(
        &mut engine,
        detected,
        "setProximity",
        "(D)V",
        &[Value::Double(-1.0)],
    );
    assert_eq!(
        call(&mut engine, detected, "getProximity", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_static(&mut engine, P35B_DETECTED, "getMaxScore", "()I", &[]),
        Value::Int(255)
    );

    // Range validation from onConstructed().
    let bad_score = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        bad_score,
        "setScore",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(256)],
    );
    assert_throws(
        engine.invoke_instance(
            bad_score,
            "build",
            "()Landroid/service/voice/HotwordDetectedResult;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let bad_channel = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        bad_channel,
        "setAudioChannel",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(64)],
    );
    assert_throws(
        engine.invoke_instance(
            bad_channel,
            "build",
            "()Landroid/service/voice/HotwordDetectedResult;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let good = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        good,
        "setScore",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(255)],
    );
    call(
        &mut engine,
        good,
        "setConfidenceLevel",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        good,
        "setAudioChannel",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(63)],
    );
    let good_result = object(call(
        &mut engine,
        good,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, good_result, "getScore", "()I", &[]),
        Value::Int(255)
    );
    assert_eq!(
        call(&mut engine, good_result, "getConfidenceLevel", "()I", &[]),
        Value::Int(5)
    );

    // Audio streams are defensively copied on the way in and out.
    let null_stream_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            null_stream_builder,
            "setAudioStreams",
            "(Ljava/util/List;)Landroid/service/voice/HotwordDetectedResult$Builder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let stream = instance(&mut engine, "Landroid/service/voice/HotwordAudioStream;");
    let source = p49_new(&mut engine, "Ljava/util/ArrayList;", "()V", &[]);
    call(
        &mut engine,
        source,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(stream)],
    );
    let stream_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        stream_builder,
        "setAudioStreams",
        "(Ljava/util/List;)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Obj(source)],
    );
    call(
        &mut engine,
        source,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(stream)],
    );
    let with_streams = object(call(
        &mut engine,
        stream_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    let out = call(
        &mut engine,
        with_streams,
        "getAudioStreams",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, out.clone()), 1);
    let out_again = call(
        &mut engine,
        with_streams,
        "getAudioStreams",
        "()Ljava/util/List;",
        &[],
    );
    assert_ne!(object(out_again.clone()), object(out.clone()));
    // buildUpon round-trips the values.
    let rebuilt_builder = object(call(
        &mut engine,
        with_streams,
        "buildUpon",
        "()Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[],
    ));
    let rebuilt = object(call(
        &mut engine,
        rebuilt_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            with_streams,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(rebuilt)]
        ),
        Value::Int(1)
    );
}

#[test]
fn tts_and_voice_deep_audit_fixes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);

    // EngineInfo has no describeContents in AOSP (it is not Parcelable).
    let engine_info = p49_new(&mut engine, P35B_ENGINE_INFO, "()V", &[]);
    assert!(engine
        .invoke_instance(engine_info, "describeContents", "()I", &[])
        .is_err());

    // TextToSpeechService.onBind is gated on the TTS_SERVICE action.
    let tts_service = p49_new(&mut engine, P35B_TTS_SERVICE, "()V", &[]);
    let wrong_intent = p41_intent(&mut engine, "android.intent.action.MAIN");
    assert_eq!(
        call(
            &mut engine,
            tts_service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong_intent)],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            tts_service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // onGetSupportedVoiceActions returns the immutable empty set.
    let vis = p49_new(&mut engine, P35B_VIS, "()V", &[]);
    let actions = object(call(
        &mut engine,
        vis,
        "onGetSupportedVoiceActions",
        "(Ljava/util/Set;)Ljava/util/Set;",
        &[Value::Null],
    ));
    let sample = text(&mut engine, "com.example.ACTION");
    assert_throws(
        engine.invoke_instance(actions, "add", "(Ljava/lang/Object;)Z", &[sample]),
        "Ljava/lang/UnsupportedOperationException;",
    );
    let vis_intent = p41_intent(&mut engine, "android.service.voice.VoiceInteractionService");
    let binder = object(call(
        &mut engine,
        vis,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(vis_intent)],
    ));

    // Session lifecycle signatures and pre-create guard ordering.
    let session = p49_new(
        &mut engine,
        P35B_SESSION,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    let bundle = p41_bundle(&mut engine);
    call(
        &mut engine,
        session,
        "onPrepareShow",
        "(Landroid/os/Bundle;I)V",
        &[Value::Obj(bundle), Value::Int(1)],
    );
    call(
        &mut engine,
        session,
        "onTaskStarted",
        "(Landroid/content/Intent;I)V",
        &[Value::Null, Value::Int(7)],
    );
    assert_throws(
        engine.invoke_instance(
            session,
            "onTaskFinished",
            "(Landroid/content/Intent;I)V",
            &[Value::Null, Value::Int(7)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // requireNonNull(bundle) precedes the token check.
    assert_throws(
        engine.invoke_instance(
            session,
            "startAssistantActivity",
            "(Landroid/content/Intent;Landroid/os/Bundle;)V",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            session,
            "startAssistantActivity",
            "(Landroid/content/Intent;Landroid/os/Bundle;)V",
            &[Value::Null, Value::Obj(bundle)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let activity_id = p49_new(
        &mut engine,
        P35B_ACTIVITY_ID,
        "(ILandroid/os/IBinder;)V",
        &[Value::Int(3), Value::Obj(binder)],
    );
    let executor = instance(&mut engine, "Ljava/util/concurrent/Executor;");
    let consumer = instance(&mut engine, "Ljava/util/function/Consumer;");
    assert_throws(
        engine.invoke_instance(
            session,
            "requestDirectActions",
            "(Landroid/service/voice/VoiceInteractionSession$ActivityId;Landroid/os/CancellationSignal;Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V",
            &[Value::Null, Value::Null, Value::Obj(executor), Value::Obj(consumer)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            session,
            "requestDirectActions",
            "(Landroid/service/voice/VoiceInteractionSession$ActivityId;Landroid/os/CancellationSignal;Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V",
            &[Value::Obj(activity_id), Value::Null, Value::Obj(executor), Value::Obj(consumer)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // performDirectAction checks the token before its null checks.
    assert_throws(
        engine.invoke_instance(
            session,
            "performDirectAction",
            "(Landroid/app/DirectAction;Landroid/os/Bundle;Landroid/os/CancellationSignal;Ljava/util/concurrent/Executor;Ljava/util/function/Consumer;)V",
            &[Value::Null, Value::Null, Value::Null, Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    // HotwordDetectedResult: List.copyOf semantics for audio streams.
    let stream = instance(&mut engine, "Landroid/service/voice/HotwordAudioStream;");
    let detected_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    let detected = object(call(
        &mut engine,
        detected_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    let out_streams = object(call(
        &mut engine,
        detected,
        "getAudioStreams",
        "()Ljava/util/List;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(
            out_streams,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(stream)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    // A failing List.copyOf leaves the builder bit set and the old
    // value in place.
    let good_list = p49_new(&mut engine, "Ljava/util/ArrayList;", "()V", &[]);
    call(
        &mut engine,
        good_list,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Obj(stream)],
    );
    let null_list = p49_new(&mut engine, "Ljava/util/ArrayList;", "()V", &[]);
    call(
        &mut engine,
        null_list,
        "add",
        "(Ljava/lang/Object;)Z",
        &[Value::Null],
    );
    let replace_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        replace_builder,
        "setAudioStreams",
        "(Ljava/util/List;)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Obj(good_list)],
    );
    assert_throws(
        engine.invoke_instance(
            replace_builder,
            "setAudioStreams",
            "(Ljava/util/List;)Landroid/service/voice/HotwordDetectedResult$Builder;",
            &[Value::Obj(null_list)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let replaced = object(call(
        &mut engine,
        replace_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    let kept = call(
        &mut engine,
        replaced,
        "getAudioStreams",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, kept), 1);
    // With no previous value, the marked bit makes build() hit the
    // constructor's @NonNull validation instead of the default.
    let fresh_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            fresh_builder,
            "setAudioStreams",
            "(Ljava/util/List;)Landroid/service/voice/HotwordDetectedResult$Builder;",
            &[Value::Obj(null_list)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            fresh_builder,
            "build",
            "()Landroid/service/voice/HotwordDetectedResult;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // @NonNull extras validation precedes the score range check.
    let order_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        order_builder,
        "setScore",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(256)],
    );
    call(
        &mut engine,
        order_builder,
        "setExtras",
        "(Landroid/os/PersistableBundle;)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Null],
    );
    assert_throws(
        engine.invoke_instance(
            order_builder,
            "build",
            "()Landroid/service/voice/HotwordDetectedResult;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // Generated toString prints the raw confidence int; the symbolic
    // name comes from the static confidenceLevelToString helper.
    let named_builder = p49_new(&mut engine, P35B_DETECTED_BUILDER, "()V", &[]);
    call(
        &mut engine,
        named_builder,
        "setConfidenceLevel",
        "(I)Landroid/service/voice/HotwordDetectedResult$Builder;",
        &[Value::Int(5)],
    );
    let named = object(call(
        &mut engine,
        named_builder,
        "build",
        "()Landroid/service/voice/HotwordDetectedResult;",
        &[],
    ));
    let named_text = call_text(&mut engine, named, "toString", "()Ljava/lang/String;", &[]);
    assert!(named_text.starts_with("HotwordDetectedResult { confidenceLevel = 5,"));
    assert_eq!(
        call_static_text(
            &mut engine,
            P35B_DETECTED,
            "confidenceLevelToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(6)],
        ),
        "CONFIDENCE_LEVEL_VERY_HIGH"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P35B_DETECTED,
            "proximityToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(2)],
        ),
        "PROXIMITY_FAR"
    );

    // HotwordRejectedResult has no buildUpon in AOSP.
    let rejected_builder = p49_new(&mut engine, P35B_REJECTED_BUILDER, "()V", &[]);
    let rejected = object(call(
        &mut engine,
        rejected_builder,
        "build",
        "()Landroid/service/voice/HotwordRejectedResult;",
        &[],
    ));
    assert!(engine
        .invoke_instance(
            rejected,
            "buildUpon",
            "()Landroid/service/voice/HotwordRejectedResult$Builder;",
            &[],
        )
        .is_err());
}

// ---------------------------------------------------------------------------
// Appendix C Batch 35-C: android.location + android.app.slice.
// ---------------------------------------------------------------------------

const P35C_LOCATION: &str = "Landroid/location/Location;";
const P35C_CRITERIA: &str = "Landroid/location/Criteria;";
const P35C_SLICE_BUILDER: &str = "Landroid/app/slice/Slice$Builder;";
const P35C_SLICE_ITEM: &str = "Landroid/app/slice/SliceItem;";
const P35C_SLICE_SPEC: &str = "Landroid/app/slice/SliceSpec;";
const P35C_SLICE_MANAGER: &str = "Landroid/app/slice/SliceManager;";

fn p35c_location_at(engine: &mut Engine, latitude: f64, longitude: f64) -> ObjRef {
    let provider = text(engine, "gps");
    let location = p49_new(engine, P35C_LOCATION, "(Ljava/lang/String;)V", &[provider]);
    call(
        engine,
        location,
        "setLatitude",
        "(D)V",
        &[Value::Double(latitude)],
    );
    call(
        engine,
        location,
        "setLongitude",
        "(D)V",
        &[Value::Double(longitude)],
    );
    location
}

fn p35c_as_double(value: Value) -> f64 {
    match value {
        Value::Double(d) => d,
        Value::Float(f) => f as f64,
        other => panic!("expected floating value, got {other:?}"),
    }
}

#[test]
fn location_and_criteria_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let location = p35c_location_at(&mut engine, 37.4215, -122.084);
    assert_eq!(
        call_text(
            &mut engine,
            location,
            "getProvider",
            "()Ljava/lang/String;",
            &[]
        ),
        "gps"
    );
    assert_eq!(
        call(&mut engine, location, "getLatitude", "()D", &[]),
        Value::Double(37.4215)
    );
    assert_eq!(
        call(&mut engine, location, "getLongitude", "()D", &[]),
        Value::Double(-122.084)
    );
    assert_eq!(
        call(&mut engine, location, "hasAccuracy", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, location, "isComplete", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, location, "isMock", "()Z", &[]),
        Value::Int(0)
    );

    call(
        &mut engine,
        location,
        "setAccuracy",
        "(F)V",
        &[Value::Float(5.0)],
    );
    assert_eq!(
        call(&mut engine, location, "hasAccuracy", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, location, "getAccuracy", "()F", &[]),
        Value::Float(5.0)
    );
    call(&mut engine, location, "removeAccuracy", "()V", &[]);
    // Removing clears only the presence bit; the value is retained.
    assert_eq!(
        call(&mut engine, location, "hasAccuracy", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, location, "getAccuracy", "()F", &[]),
        Value::Float(5.0)
    );
    call(
        &mut engine,
        location,
        "setAccuracy",
        "(F)V",
        &[Value::Float(5.0)],
    );

    call(
        &mut engine,
        location,
        "setBearing",
        "(F)V",
        &[Value::Float(370.0)],
    );
    assert_eq!(
        call(&mut engine, location, "getBearing", "()F", &[]),
        Value::Float(10.0)
    );
    call(
        &mut engine,
        location,
        "setBearing",
        "(F)V",
        &[Value::Float(-10.0)],
    );
    assert_eq!(
        call(&mut engine, location, "getBearing", "()F", &[]),
        Value::Float(350.0)
    );
    assert_throws(
        engine.invoke_instance(location, "setBearing", "(F)V", &[Value::Float(f32::NAN)]),
        "Ljava/lang/IllegalArgumentException;",
    );

    call(
        &mut engine,
        location,
        "setTime",
        "(J)V",
        &[Value::Long(123_456_789)],
    );
    assert_eq!(
        call(&mut engine, location, "getTime", "()J", &[]),
        Value::Long(123_456_789)
    );
    call(
        &mut engine,
        location,
        "setElapsedRealtimeNanos",
        "(J)V",
        &[Value::Long(1_999_999)],
    );
    // NANOSECONDS.toMillis truncates.
    assert_eq!(
        call(
            &mut engine,
            location,
            "getElapsedRealtimeMillis",
            "()J",
            &[]
        ),
        Value::Long(1)
    );
    assert_eq!(
        call(
            &mut engine,
            location,
            "getElapsedRealtimeAgeMillis",
            "(J)J",
            &[Value::Long(2000)],
        ),
        Value::Long(1999)
    );
    assert_eq!(
        call(&mut engine, location, "isComplete", "()Z", &[]),
        Value::Int(1)
    );

    // WGS84 distance/bearing: one degree of longitude at the equator.
    let origin = p35c_location_at(&mut engine, 0.0, 0.0);
    let east = p35c_location_at(&mut engine, 0.0, 1.0);
    let distance = p35c_as_double(call(
        &mut engine,
        origin,
        "distanceTo",
        "(Landroid/location/Location;)F",
        &[Value::Obj(east)],
    ));
    assert!(
        (111_000.0..111_700.0).contains(&distance),
        "distance={distance}"
    );
    let bearing = p35c_as_double(call(
        &mut engine,
        origin,
        "bearingTo",
        "(Landroid/location/Location;)F",
        &[Value::Obj(east)],
    ));
    assert!((89.9..90.1).contains(&bearing), "bearing={bearing}");
    assert_eq!(
        call(
            &mut engine,
            origin,
            "distanceTo",
            "(Landroid/location/Location;)F",
            &[Value::Obj(origin)],
        ),
        Value::Float(0.0)
    );
    let results = float_array(&mut engine, 3);
    call_static(
        &mut engine,
        P35C_LOCATION,
        "distanceBetween",
        "(DDDD[F)V",
        &[
            Value::Double(0.0),
            Value::Double(0.0),
            Value::Double(0.0),
            Value::Double(1.0),
            results.clone(),
        ],
    );
    let results_obj = object(results);
    let (elems, _) = engine.heap.as_array_elems(results_obj).unwrap();
    let elems = elems.to_vec();
    assert!((111_000.0..111_700.0).contains(&p35c_as_double(elems[0].clone())));
    assert!((89.9..90.1).contains(&p35c_as_double(elems[1].clone())));
    assert!((89.9..90.1).contains(&p35c_as_double(elems[2].clone())));
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "distanceBetween",
            "(DDDD[F)V",
            &[
                Value::Double(0.0),
                Value::Double(0.0),
                Value::Double(0.0),
                Value::Double(1.0),
                Value::Null,
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Coordinate conversion both ways.
    assert_eq!(
        call_static_text(
            &mut engine,
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(37.4215), Value::Int(0)],
        ),
        "37.4215"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(-122.084), Value::Int(1)],
        ),
        "-122:5.04"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(37.4215), Value::Int(2)],
        ),
        "37:25:17.4"
    );
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(200.0), Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(10.0), Value::Int(7)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let parsed = |engine: &mut Engine, input: &str| -> f64 {
        let value = text(engine, input);
        p35c_as_double(call_static(
            engine,
            P35C_LOCATION,
            "convert",
            "(Ljava/lang/String;)D",
            &[value],
        ))
    };
    assert!((parsed(&mut engine, "37.4215") - 37.4215).abs() < 1e-9);
    assert!((parsed(&mut engine, "-122:5.04") + 122.084).abs() < 1e-9);
    assert!((parsed(&mut engine, "37:25:17.4") - 37.4215).abs() < 1e-9);
    assert!((parsed(&mut engine, "-180:0") + 180.0).abs() < 1e-12);
    let empty = text(&mut engine, "");
    assert_throws(
        engine.invoke_static(P35C_LOCATION, "convert", "(Ljava/lang/String;)D", &[empty]),
        "Ljava/lang/StringIndexOutOfBoundsException;",
    );
    let bad_degrees = text(&mut engine, "200:0");
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(Ljava/lang/String;)D",
            &[bad_degrees],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(Ljava/lang/String;)D",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // Extras are defensively copied.
    let extras = p41_bundle(&mut engine);
    let satellites = text(&mut engine, "satellites");
    call(
        &mut engine,
        extras,
        "putInt",
        "(Ljava/lang/String;I)V",
        &[satellites.clone(), Value::Int(7)],
    );
    call(
        &mut engine,
        location,
        "setExtras",
        "(Landroid/os/Bundle;)V",
        &[Value::Obj(extras)],
    );
    let stored = object(call(
        &mut engine,
        location,
        "getExtras",
        "()Landroid/os/Bundle;",
        &[],
    ));
    assert_ne!(stored, extras);
    let satellites_key = text(&mut engine, "satellites");
    assert_eq!(
        call(
            &mut engine,
            stored,
            "getInt",
            "(Ljava/lang/String;)I",
            &[satellites_key]
        ),
        Value::Int(7)
    );
    call(
        &mut engine,
        extras,
        "putInt",
        "(Ljava/lang/String;I)V",
        &[satellites, Value::Int(9)],
    );
    let satellites2 = text(&mut engine, "satellites");
    assert_eq!(
        call(
            &mut engine,
            stored,
            "getInt",
            "(Ljava/lang/String;)I",
            &[satellites2]
        ),
        Value::Int(7)
    );

    // Copy semantics and equality.
    let copy = p49_new(
        &mut engine,
        P35C_LOCATION,
        "(Landroid/location/Location;)V",
        &[Value::Obj(location)],
    );
    assert_eq!(
        call(
            &mut engine,
            location,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(copy)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, location, "hashCode", "()I", &[]),
        call(&mut engine, copy, "hashCode", "()I", &[])
    );
    call(&mut engine, copy, "removeBearing", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            location,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(copy)]
        ),
        Value::Int(0)
    );
    call(&mut engine, copy, "reset", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            copy,
            "getProvider",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, copy, "getTime", "()J", &[]),
        Value::Long(0)
    );

    // makeComplete fills provider and accuracy.
    let incomplete = p35c_location_at(&mut engine, 1.0, 2.0);
    call(
        &mut engine,
        incomplete,
        "setProvider",
        "(Ljava/lang/String;)V",
        &[Value::Null],
    );
    call(
        &mut engine,
        incomplete,
        "setTime",
        "(J)V",
        &[Value::Long(5)],
    );
    call(
        &mut engine,
        incomplete,
        "setElapsedRealtimeNanos",
        "(J)V",
        &[Value::Long(5)],
    );
    call(&mut engine, incomplete, "makeComplete", "()V", &[]);
    assert_eq!(
        call_text(
            &mut engine,
            incomplete,
            "getProvider",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    assert_eq!(
        call(&mut engine, incomplete, "getAccuracy", "()F", &[]),
        Value::Float(100.0)
    );
    assert_eq!(
        call(&mut engine, incomplete, "isComplete", "()Z", &[]),
        Value::Int(1)
    );
    call(&mut engine, incomplete, "setMock", "(Z)V", &[Value::Int(1)]);
    assert_eq!(
        call(&mut engine, incomplete, "isMock", "()Z", &[]),
        Value::Int(1)
    );

    // Criteria.
    let criteria = p49_new(&mut engine, P35C_CRITERIA, "()V", &[]);
    assert_eq!(
        call(&mut engine, criteria, "getHorizontalAccuracy", "()I", &[]),
        Value::Int(0)
    );
    // Less than ACCURACY_HIGH maps back to ACCURACY_COARSE.
    assert_eq!(
        call(&mut engine, criteria, "getAccuracy", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_text(
            &mut engine,
            criteria,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "Criteria[power=None, accuracy=None]"
    );
    call(
        &mut engine,
        criteria,
        "setAccuracy",
        "(I)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, criteria, "getHorizontalAccuracy", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, criteria, "getAccuracy", "()I", &[]),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(criteria, "setAccuracy", "(I)V", &[Value::Int(3)]),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(criteria, "setHorizontalAccuracy", "(I)V", &[Value::Int(4)]),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        criteria,
        "setPowerRequirement",
        "(I)V",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        criteria,
        "setVerticalAccuracy",
        "(I)V",
        &[Value::Int(2)],
    );
    call(
        &mut engine,
        criteria,
        "setAltitudeRequired",
        "(Z)V",
        &[Value::Int(1)],
    );
    call(
        &mut engine,
        criteria,
        "setCostAllowed",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call_text(&mut engine, criteria, "toString", "()Ljava/lang/String;", &[]),
        "Criteria[power=Low, accuracy=High, verticalAccuracy=Medium, required=[altitude], costAllowed]"
    );
    let criteria_copy = p49_new(
        &mut engine,
        P35C_CRITERIA,
        "(Landroid/location/Criteria;)V",
        &[Value::Obj(criteria)],
    );
    assert_eq!(
        call(
            &mut engine,
            criteria_copy,
            "getPowerRequirement",
            "()I",
            &[]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, criteria_copy, "isAltitudeRequired", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, criteria_copy, "isSpeedRequired", "()Z", &[]),
        Value::Int(0)
    );
}

#[test]
fn location_deep_audit_fixes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let location = p35c_location_at(&mut engine, 1.0, 2.0);

    // The two MSL getters throw IllegalStateException while unset, even
    // though every other optional getter returns its retained field.
    assert_throws(
        engine.invoke_instance(location, "getMslAltitudeMeters", "()D", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(location, "getMslAltitudeAccuracyMeters", "()F", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    call(
        &mut engine,
        location,
        "setMslAltitudeMeters",
        "(D)V",
        &[Value::Double(123.5)],
    );
    call(
        &mut engine,
        location,
        "setMslAltitudeAccuracyMeters",
        "(F)V",
        &[Value::Float(4.5)],
    );
    assert_eq!(
        call(&mut engine, location, "getMslAltitudeMeters", "()D", &[]),
        Value::Double(123.5)
    );
    assert_eq!(
        call(
            &mut engine,
            location,
            "getMslAltitudeAccuracyMeters",
            "()F",
            &[]
        ),
        Value::Float(4.5)
    );
    call(&mut engine, location, "removeMslAltitude", "()V", &[]);
    assert_throws(
        engine.invoke_instance(location, "getMslAltitudeMeters", "()D", &[]),
        "Ljava/lang/IllegalStateException;",
    );

    // Location.equals uses Double.compare/Float.compare: distinct NaN
    // payloads are equal (and hash alike), but -0.0 differs from 0.0.
    let nan_a = f64::from_bits(0x7ff8_0000_0000_0001);
    let nan_b = f64::from_bits(0xfff8_0000_0000_0002);
    let left = p35c_location_at(&mut engine, nan_a, -0.0);
    let right = p35c_location_at(&mut engine, nan_b, -0.0);
    let locations_equal = |engine: &mut Engine, a: ObjRef, b: ObjRef| {
        call(
            engine,
            a,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(b)],
        )
    };
    assert_eq!(locations_equal(&mut engine, left, right), Value::Int(1));
    assert_eq!(
        call(&mut engine, left, "hashCode", "()I", &[]),
        call(&mut engine, right, "hashCode", "()I", &[])
    );
    call(
        &mut engine,
        right,
        "setLongitude",
        "(D)V",
        &[Value::Double(0.0)],
    );
    assert_eq!(locations_equal(&mut engine, left, right), Value::Int(0));
    call(
        &mut engine,
        right,
        "setLongitude",
        "(D)V",
        &[Value::Double(-0.0)],
    );
    call(
        &mut engine,
        left,
        "setSpeed",
        "(F)V",
        &[Value::Float(f32::NAN)],
    );
    call(
        &mut engine,
        right,
        "setSpeed",
        "(F)V",
        &[Value::Float(f32::from_bits(0xffc0_0001))],
    );
    assert_eq!(locations_equal(&mut engine, left, right), Value::Int(1));
    call(&mut engine, left, "setSpeed", "(F)V", &[Value::Float(-0.0)]);
    call(&mut engine, right, "setSpeed", "(F)V", &[Value::Float(0.0)]);
    assert_eq!(locations_equal(&mut engine, left, right), Value::Int(0));

    // convert(String) delegates its floating-point tokens to
    // Double.parseDouble, and its range checks use ordinary comparisons,
    // so NaN seconds slip through to a NaN result.
    let parsed = |engine: &mut Engine, input: &str| -> f64 {
        let value = text(engine, input);
        p35c_as_double(call_static(
            engine,
            P35C_LOCATION,
            "convert",
            "(Ljava/lang/String;)D",
            &[value],
        ))
    };
    assert_eq!(parsed(&mut engine, " 12.5f "), 12.5);
    assert_eq!(parsed(&mut engine, "1: 30.0f "), 1.5);
    assert!(parsed(&mut engine, "1:2:NaN").is_nan());
    assert_eq!(parsed(&mut engine, "Infinity"), f64::INFINITY);
    let rust_spelling = text(&mut engine, "inf");
    assert_throws(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(Ljava/lang/String;)D",
            &[rust_spelling],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Internal Preconditions range failures use the bounds, not the
    // rejected value, and spell out which side failed.
    let assert_message = |result: Result<Value, VmExit>, expected: &str| match result {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;");
            assert_eq!(t.message.as_deref(), Some(expected));
        }
        other => panic!("expected IllegalArgumentException({expected:?}), got {other:?}"),
    };
    assert_message(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(f64::NAN), Value::Int(0)],
        ),
        "coordinate must not be NaN",
    );
    assert_message(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(-180.5), Value::Int(0)],
        ),
        "coordinate is out of range of [-180.000000, 180.000000] (too low)",
    );
    assert_message(
        engine.invoke_static(
            P35C_LOCATION,
            "convert",
            "(DI)Ljava/lang/String;",
            &[Value::Double(180.5), Value::Int(0)],
        ),
        "coordinate is out of range of [-180.000000, 180.000000] (too high)",
    );
    let criteria = p49_new(&mut engine, P35C_CRITERIA, "()V", &[]);
    assert_message(
        engine.invoke_instance(criteria, "setHorizontalAccuracy", "(I)V", &[Value::Int(-1)]),
        "accuracy is out of range of [0, 3] (too low)",
    );
    assert_message(
        engine.invoke_instance(criteria, "setPowerRequirement", "(I)V", &[Value::Int(4)]),
        "powerRequirement is out of range of [0, 3] (too high)",
    );
}

#[test]
fn slice_values_builder_and_missing_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());

    let spec_type = text(&mut engine, "list");
    let spec_v2 = p49_new(
        &mut engine,
        P35C_SLICE_SPEC,
        "(Ljava/lang/String;I)V",
        &[spec_type, Value::Int(2)],
    );
    assert_eq!(
        call_text(&mut engine, spec_v2, "getType", "()Ljava/lang/String;", &[]),
        "list"
    );
    assert_eq!(
        call(&mut engine, spec_v2, "getRevision", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_text(
            &mut engine,
            spec_v2,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "SliceSpec{list,2}"
    );
    let spec_type3 = text(&mut engine, "list");
    let spec_v3 = p49_new(
        &mut engine,
        P35C_SLICE_SPEC,
        "(Ljava/lang/String;I)V",
        &[spec_type3, Value::Int(3)],
    );
    assert_eq!(
        call(
            &mut engine,
            spec_v3,
            "canRender",
            "(Landroid/app/slice/SliceSpec;)Z",
            &[Value::Obj(spec_v2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            spec_v2,
            "canRender",
            "(Landroid/app/slice/SliceSpec;)Z",
            &[Value::Obj(spec_v3)]
        ),
        Value::Int(0)
    );
    let spec_type2b = text(&mut engine, "list");
    let spec_v2b = p49_new(
        &mut engine,
        P35C_SLICE_SPEC,
        "(Ljava/lang/String;I)V",
        &[spec_type2b, Value::Int(2)],
    );
    assert_eq!(
        call(
            &mut engine,
            spec_v2,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(spec_v2b)]
        ),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_instance(
            spec_v2,
            "canRender",
            "(Landroid/app/slice/SliceSpec;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let uri_text = text(&mut engine, "content://com.example/slice");
    let uri = call_static(
        &mut engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[uri_text],
    );
    let builder = p49_new(
        &mut engine,
        P35C_SLICE_BUILDER,
        "(Landroid/net/Uri;Landroid/app/slice/SliceSpec;)V",
        &[uri.clone(), Value::Obj(spec_v2)],
    );
    let hello = text(&mut engine, "Hello");
    let no_hints = p35_string_list(&mut engine, &[]);
    let returned = call(
        &mut engine,
        builder,
        "addText",
        "(Ljava/lang/CharSequence;Ljava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
        &[hello, Value::Null, no_hints],
    );
    assert_eq!(returned, Value::Obj(builder));
    let ttl_hints = p35_string_list(&mut engine, &["ttl"]);
    call(
        &mut engine,
        builder,
        "addInt",
        "(ILjava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
        &[Value::Int(42), Value::Null, ttl_hints],
    );
    let long_hints = p35_string_list(&mut engine, &[]);
    call(
        &mut engine,
        builder,
        "addLong",
        "(JLjava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
        &[Value::Long(1234), Value::Null, long_hints],
    );
    call(
        &mut engine,
        builder,
        "setCallerNeeded",
        "(Z)Landroid/app/slice/Slice$Builder;",
        &[Value::Int(1)],
    );
    let slice = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/app/slice/Slice;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, slice, "getUri", "()Landroid/net/Uri;", &[]),
        uri
    );
    assert_eq!(
        call(
            &mut engine,
            slice,
            "getSpec",
            "()Landroid/app/slice/SliceSpec;",
            &[]
        ),
        Value::Obj(spec_v2)
    );
    assert_eq!(
        call(&mut engine, slice, "isCallerNeeded", "()Z", &[]),
        Value::Int(1)
    );
    let items = call(&mut engine, slice, "getItems", "()Ljava/util/List;", &[]);
    assert_eq!(p57_collection_size(&mut engine, items.clone()), 3);
    let items_obj = object(items);
    let item0 = object(call(
        &mut engine,
        items_obj,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(0)],
    ));
    assert_eq!(
        call_text(&mut engine, item0, "getFormat", "()Ljava/lang/String;", &[]),
        "text"
    );
    assert_eq!(
        call_text(
            &mut engine,
            item0,
            "getText",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Hello"
    );
    let item1 = object(call(
        &mut engine,
        items_obj,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(1)],
    ));
    assert_eq!(
        call(&mut engine, item1, "getInt", "()I", &[]),
        Value::Int(42)
    );
    let ttl = text(&mut engine, "ttl");
    assert_eq!(
        call(
            &mut engine,
            item1,
            "hasHint",
            "(Ljava/lang/String;)Z",
            &[ttl]
        ),
        Value::Int(1)
    );
    let item2 = object(call(
        &mut engine,
        items_obj,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(2)],
    ));
    assert_eq!(
        call(&mut engine, item2, "getLong", "()J", &[]),
        Value::Long(1234)
    );
    // Unboxing the wrong type is a ClassCastException.
    assert_throws(
        engine.invoke_instance(item0, "getInt", "()I", &[]),
        "Ljava/lang/ClassCastException;",
    );

    // SliceItem constructor null-handling differs per hints overload.
    let payload = text(&mut engine, "payload");
    let format = text(&mut engine, "text");
    let null_hints_item = instance(&mut engine, P35C_SLICE_ITEM);
    assert_throws(
        engine.invoke_instance(
            null_hints_item,
            "<init>",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;)V",
            &[payload.clone(), format.clone(), Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let array_item = p49_new(
        &mut engine,
        P35C_SLICE_ITEM,
        "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)V",
        &[payload, format, Value::Null, Value::Null],
    );
    let any_hint = text(&mut engine, "anything");
    assert_eq!(
        call(
            &mut engine,
            array_item,
            "hasHint",
            "(Ljava/lang/String;)Z",
            &[any_hint]
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_instance(array_item, "getHints", "()Ljava/util/List;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    // Sub-slices and actions.
    let sub_builder = p49_new(
        &mut engine,
        P35C_SLICE_BUILDER,
        "(Landroid/app/slice/Slice$Builder;)V",
        &[Value::Obj(builder)],
    );
    let sub_slice = object(call(
        &mut engine,
        sub_builder,
        "build",
        "()Landroid/app/slice/Slice;",
        &[],
    ));
    let sub_uri = object(call(
        &mut engine,
        sub_slice,
        "getUri",
        "()Landroid/net/Uri;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            sub_uri,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "content://com.example/slice/_gen/0"
    );

    let parent_uri_text = text(&mut engine, "content://com.example/parent");
    let parent_uri = call_static(
        &mut engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[parent_uri_text],
    );
    let parent_builder = p49_new(
        &mut engine,
        P35C_SLICE_BUILDER,
        "(Landroid/net/Uri;Landroid/app/slice/SliceSpec;)V",
        &[parent_uri, Value::Obj(spec_v2)],
    );
    call(
        &mut engine,
        parent_builder,
        "addSubSlice",
        "(Landroid/app/slice/Slice;Ljava/lang/String;)Landroid/app/slice/Slice$Builder;",
        &[Value::Obj(sub_slice), Value::Null],
    );
    let action_slice = {
        let uri_value = {
            let t = text(&mut engine, "content://com.example/action");
            call_static(
                &mut engine,
                "Landroid/net/Uri;",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[t],
            )
        };
        let b = p49_new(
            &mut engine,
            P35C_SLICE_BUILDER,
            "(Landroid/net/Uri;Landroid/app/slice/SliceSpec;)V",
            &[uri_value, Value::Obj(spec_v2)],
        );
        object(call(
            &mut engine,
            b,
            "build",
            "()Landroid/app/slice/Slice;",
            &[],
        ))
    };
    let pending = p41_pending(&mut engine, "getActivity", 0);
    call(
        &mut engine,
        parent_builder,
        "addAction",
        "(Landroid/app/PendingIntent;Landroid/app/slice/Slice;Ljava/lang/String;)Landroid/app/slice/Slice$Builder;",
        &[pending.clone(), Value::Obj(action_slice), Value::Null],
    );
    // addAction clears the child slice's spec, as in AOSP.
    assert_eq!(
        call(
            &mut engine,
            action_slice,
            "getSpec",
            "()Landroid/app/slice/SliceSpec;",
            &[]
        ),
        Value::Null
    );
    let parent = object(call(
        &mut engine,
        parent_builder,
        "build",
        "()Landroid/app/slice/Slice;",
        &[],
    ));
    let parent_items = object(call(
        &mut engine,
        parent,
        "getItems",
        "()Ljava/util/List;",
        &[],
    ));
    let sub_item = object(call(
        &mut engine,
        parent_items,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(0)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            sub_item,
            "getFormat",
            "()Ljava/lang/String;",
            &[]
        ),
        "slice"
    );
    assert_eq!(
        call(
            &mut engine,
            sub_item,
            "getSlice",
            "()Landroid/app/slice/Slice;",
            &[]
        ),
        Value::Obj(sub_slice)
    );
    let action_item = object(call(
        &mut engine,
        parent_items,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(1)],
    ));
    assert_eq!(
        call(
            &mut engine,
            action_item,
            "getAction",
            "()Landroid/app/PendingIntent;",
            &[]
        ),
        pending
    );
    assert_eq!(
        call(
            &mut engine,
            action_item,
            "getSlice",
            "()Landroid/app/slice/Slice;",
            &[]
        ),
        Value::Obj(action_slice)
    );

    // There is no "slice" system service to construct a manager from.
    let context = instance(&mut engine, CONTEXT);
    let manager = instance(&mut engine, P35C_SLICE_MANAGER);
    assert_throws(
        engine.invoke_instance(
            manager,
            "<init>",
            "(Landroid/content/Context;Landroid/os/Handler;)V",
            &[Value::Obj(context), Value::Null],
        ),
        "Landroid/os/ServiceManager$ServiceNotFoundException;",
    );
    assert_throws(
        engine.invoke_instance(manager, "hasSliceAccess", "()Z", &[]),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn slice_deep_audit_fixes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let item_sig = "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;)V";

    // Typed getters really cast the payload: a String is a CharSequence
    // but not a Bundle, Slice, or the Pair behind getAction.
    let payload = text(&mut engine, "words");
    let bundle_format = text(&mut engine, "bundle");
    let no_hints = p35_string_list(&mut engine, &[]);
    let wrong = p49_new(
        &mut engine,
        P35C_SLICE_ITEM,
        item_sig,
        &[payload, bundle_format, Value::Null, no_hints],
    );
    assert_eq!(
        call_text(
            &mut engine,
            wrong,
            "getText",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "words"
    );
    assert_throws(
        engine.invoke_instance(wrong, "getBundle", "()Landroid/os/Bundle;", &[]),
        "Ljava/lang/ClassCastException;",
    );
    assert_throws(
        engine.invoke_instance(wrong, "getSlice", "()Landroid/app/slice/Slice;", &[]),
        "Ljava/lang/ClassCastException;",
    );
    assert_throws(
        engine.invoke_instance(wrong, "getAction", "()Landroid/app/PendingIntent;", &[]),
        "Ljava/lang/ClassCastException;",
    );

    // A null payload casts cleanly for reference getters, but getAction
    // dereferences the Pair and therefore NPEs.
    let text_format = text(&mut engine, "text");
    let no_hints = p35_string_list(&mut engine, &[]);
    let empty = p49_new(
        &mut engine,
        P35C_SLICE_ITEM,
        item_sig,
        &[Value::Null, text_format, Value::Null, no_hints],
    );
    assert_eq!(
        call(
            &mut engine,
            empty,
            "getText",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            empty,
            "getBundle",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(empty, "getAction", "()Landroid/app/PendingIntent;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    // Build parent/child slices for the Pair and toString checks.
    let make_slice = |engine: &mut Engine, uri_text: &str, label: &str| -> ObjRef {
        let raw_uri = text(engine, uri_text);
        let uri = call_static(
            engine,
            "Landroid/net/Uri;",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[raw_uri],
        );
        let builder = p49_new(
            engine,
            P35C_SLICE_BUILDER,
            "(Landroid/net/Uri;Landroid/app/slice/SliceSpec;)V",
            &[uri, Value::Null],
        );
        let words = text(engine, label);
        let hints = p35_string_list(engine, &[]);
        call(
            engine,
            builder,
            "addText",
            "(Ljava/lang/CharSequence;Ljava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
            &[words, Value::Null, hints],
        );
        let hints = p35_string_list(engine, &[]);
        call(
            engine,
            builder,
            "addInt",
            "(ILjava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
            &[Value::Int(9), Value::Null, hints],
        );
        object(call(
            engine,
            builder,
            "build",
            "()Landroid/app/slice/Slice;",
            &[],
        ))
    };
    let child = make_slice(&mut engine, "content://example/child", "child");

    // An action item created directly from a Pair exposes both sides.
    let pending = p41_pending(&mut engine, "getActivity", 7);
    let pair = instance(&mut engine, "Landroid/util/Pair;");
    engine
        .heap
        .put_field(pair, "first", pending.clone())
        .unwrap();
    engine
        .heap
        .put_field(pair, "second", Value::Obj(child))
        .unwrap();
    let action_format = text(&mut engine, "action");
    let no_hints = p35_string_list(&mut engine, &[]);
    let action_item = p49_new(
        &mut engine,
        P35C_SLICE_ITEM,
        item_sig,
        &[Value::Obj(pair), action_format, Value::Null, no_hints],
    );
    assert_eq!(
        call(
            &mut engine,
            action_item,
            "getAction",
            "()Landroid/app/PendingIntent;",
            &[]
        ),
        pending
    );
    assert_eq!(
        call(
            &mut engine,
            action_item,
            "getSlice",
            "()Landroid/app/slice/Slice;",
            &[]
        ),
        Value::Obj(child)
    );

    // Slice.toString prints one line per item and indents sub-slices.
    let parent_uri_text = text(&mut engine, "content://example/parent");
    let parent_uri = call_static(
        &mut engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[parent_uri_text],
    );
    let parent_builder = p49_new(
        &mut engine,
        P35C_SLICE_BUILDER,
        "(Landroid/net/Uri;Landroid/app/slice/SliceSpec;)V",
        &[parent_uri, Value::Null],
    );
    let root = text(&mut engine, "root");
    let hints = p35_string_list(&mut engine, &[]);
    call(
        &mut engine,
        parent_builder,
        "addText",
        "(Ljava/lang/CharSequence;Ljava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
        &[root, Value::Null, hints],
    );
    call(
        &mut engine,
        parent_builder,
        "addSubSlice",
        "(Landroid/app/slice/Slice;Ljava/lang/String;)Landroid/app/slice/Slice$Builder;",
        &[Value::Obj(child), Value::Null],
    );
    let hints = p35_string_list(&mut engine, &[]);
    call(
        &mut engine,
        parent_builder,
        "addLong",
        "(JLjava/lang/String;Ljava/util/List;)Landroid/app/slice/Slice$Builder;",
        &[Value::Long(3), Value::Null, hints],
    );
    let parent = object(call(
        &mut engine,
        parent_builder,
        "build",
        "()Landroid/app/slice/Slice;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, parent, "toString", "()Ljava/lang/String;", &[]),
        "text: root\nslice:\n   text: child\n   int\nlong\n"
    );
}

// ---------------------------------------------------------------------------
// Appendix C Batch 35-D: android.app.backup + android.telephony.gsm.
// ---------------------------------------------------------------------------

const P35D_BACKUP_MANAGER: &str = "Landroid/app/backup/BackupManager;";
const P35D_BACKUP_AGENT: &str = "Landroid/app/backup/BackupAgent;";
const P35D_RESTORE_OBSERVER: &str = "Landroid/app/backup/RestoreObserver;";
const P35D_GSM: &str = "Landroid/telephony/gsm/GsmCellLocation;";

#[test]
fn backup_without_a_backup_service_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let manager = p49_new(
        &mut engine,
        P35D_BACKUP_MANAGER,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );

    // All calls take the no-service path: writes drop, reads default.
    call(&mut engine, manager, "dataChanged", "()V", &[]);
    let package = text(&mut engine, "com.example.app");
    call_static(
        &mut engine,
        P35D_BACKUP_MANAGER,
        "dataChanged",
        "(Ljava/lang/String;)V",
        std::slice::from_ref(&package),
    );
    assert_eq!(
        call(&mut engine, manager, "isBackupEnabled", "()Z", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        manager,
        "setBackupEnabled",
        "(Z)V",
        &[Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, manager, "isBackupEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getCurrentTransport",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "listAllTransports",
            "()[Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isAppEligibleForBackup",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&package),
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getAvailableRestoreToken",
            "(Ljava/lang/String;)J",
            &[package],
        ),
        Value::Long(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "beginRestoreSession",
            "()Landroid/app/backup/RestoreSession;",
            &[]
        ),
        Value::Null
    );
    let packages = object_array(&mut engine, "[Ljava/lang/String;", vec![]);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "requestBackup",
            "([Ljava/lang/String;Landroid/app/backup/BackupObserver;)I",
            &[packages, Value::Null],
        ),
        Value::Int(-1)
    );
    // requestRestore has returned -1 unconditionally since Android P.
    let observer = p49_new(&mut engine, P35D_RESTORE_OBSERVER, "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "requestRestore",
            "(Landroid/app/backup/RestoreObserver;)I",
            &[Value::Obj(observer)],
        ),
        Value::Int(-1)
    );
    call(&mut engine, manager, "backupNow", "()V", &[]);
    call(&mut engine, manager, "cancelBackups", "()V", &[]);

    // Observer base-class callbacks are no-ops.
    call(
        &mut engine,
        observer,
        "restoreStarting",
        "(I)V",
        &[Value::Int(3)],
    );
    let current = text(&mut engine, "com.example.app");
    call(
        &mut engine,
        observer,
        "onUpdate",
        "(ILjava/lang/String;)V",
        &[Value::Int(1), current],
    );
    call(
        &mut engine,
        observer,
        "restoreFinished",
        "(I)V",
        &[Value::Int(0)],
    );

    // BackupAgent lifecycle and logger wiring.
    let agent = p49_new(&mut engine, P35D_BACKUP_AGENT, "()V", &[]);
    assert!(engine
        .cp
        .is_subtype(P35D_BACKUP_AGENT, "Landroid/content/Context;"));
    let binder = object(call(
        &mut engine,
        agent,
        "onBind",
        "()Landroid/os/IBinder;",
        &[],
    ));
    assert_eq!(
        engine.heap.class_of(binder).unwrap(),
        "Landroid/app/backup/BackupAgent$BackupServiceBinder;"
    );
    assert!(engine.cp.is_subtype(
        "Landroid/app/backup/BackupAgent$BackupServiceBinder;",
        "Landroid/os/IBinder;"
    ));
    let binder_again = object(call(
        &mut engine,
        agent,
        "onBind",
        "()Landroid/os/IBinder;",
        &[],
    ));
    assert_eq!(binder_again, binder);
    // The logger only exists after the three-argument onCreate.
    assert_eq!(
        call(
            &mut engine,
            agent,
            "getBackupRestoreEventLogger",
            "()Landroid/app/backup/BackupRestoreEventLogger;",
            &[],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "getBackupRestoreEventLogger",
            "(Landroid/app/backup/BackupAgent;)Landroid/app/backup/BackupRestoreEventLogger;",
            &[Value::Obj(agent)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    call(
        &mut engine,
        agent,
        "onCreate",
        "(Landroid/os/UserHandle;II)V",
        &[Value::Null, Value::Int(0), Value::Int(1)],
    );
    let logger = call(
        &mut engine,
        agent,
        "getBackupRestoreEventLogger",
        "()Landroid/app/backup/BackupRestoreEventLogger;",
        &[],
    );
    assert!(matches!(logger, Value::Obj(_)));
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getBackupRestoreEventLogger",
            "(Landroid/app/backup/BackupAgent;)Landroid/app/backup/BackupRestoreEventLogger;",
            &[Value::Obj(agent)],
        ),
        logger
    );
    let logger = object(logger);
    let results = call(
        &mut engine,
        logger,
        "getLoggingResults",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, results), 0);
    assert_eq!(
        call(&mut engine, logger, "getOperationType", "()I", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        agent,
        "clearBackupRestoreEventLogger",
        "()V",
        &[],
    );
    let results = call(
        &mut engine,
        logger,
        "getLoggingResults",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p57_collection_size(&mut engine, results), 0);
    call(
        &mut engine,
        agent,
        "onQuotaExceeded",
        "(JJ)V",
        &[Value::Long(10), Value::Long(5)],
    );
    call(&mut engine, agent, "onDestroy", "()V", &[]);
}

#[test]
fn gsm_cell_location_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let location = p49_new(&mut engine, P35D_GSM, "()V", &[]);
    assert!(engine
        .cp
        .is_subtype(P35D_GSM, "Landroid/telephony/CellLocation;"));
    assert_eq!(
        call(&mut engine, location, "getLac", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, location, "getCid", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, location, "getPsc", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, location, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            location,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "[-1,-1,-1]"
    );

    call(
        &mut engine,
        location,
        "setLacAndCid",
        "(II)V",
        &[Value::Int(410), Value::Int(234567)],
    );
    call(&mut engine, location, "setPsc", "(I)V", &[Value::Int(99)]);
    assert_eq!(
        call(&mut engine, location, "getLac", "()I", &[]),
        Value::Int(410)
    );
    assert_eq!(
        call(&mut engine, location, "getCid", "()I", &[]),
        Value::Int(234567)
    );
    assert_eq!(
        call(&mut engine, location, "isEmpty", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            location,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "[410,234567,99]"
    );
    assert_eq!(
        call(&mut engine, location, "hashCode", "()I", &[]),
        Value::Int(410 ^ 234567)
    );

    // Round-trip through the notifier bundle and the Bundle constructor.
    let bundle = p41_bundle(&mut engine);
    call(
        &mut engine,
        location,
        "fillInNotifierBundle",
        "(Landroid/os/Bundle;)V",
        &[Value::Obj(bundle)],
    );
    let from_bundle = p49_new(
        &mut engine,
        P35D_GSM,
        "(Landroid/os/Bundle;)V",
        &[Value::Obj(bundle)],
    );
    assert_eq!(
        call(
            &mut engine,
            location,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(from_bundle)]
        ),
        Value::Int(1)
    );
    // The Bundle constructor dereferences its argument.
    let null_bundle = instance(&mut engine, P35D_GSM);
    assert_throws(
        engine.invoke_instance(
            null_bundle,
            "<init>",
            "(Landroid/os/Bundle;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // A non-GsmCellLocation argument simply compares unequal (the cast
    // failure is caught inside AOSP's equals).
    assert_eq!(
        call(
            &mut engine,
            location,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(bundle)]
        ),
        Value::Int(0)
    );
    call(&mut engine, location, "setStateInvalid", "()V", &[]);
    assert_eq!(
        call(&mut engine, location, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );

    // With no telephony service the static update request is dropped.
    call_static(
        &mut engine,
        "Landroid/telephony/CellLocation;",
        "requestLocationUpdate",
        "()V",
        &[],
    );
}

// ---- Appendix C Batch 35-E: android.net.http + android.se.omapi ----

const P35E_SSL_ERROR: &str = "Landroid/net/http/SslError;";
const P35E_SSL_CERTIFICATE: &str = "Landroid/net/http/SslCertificate;";
const P35E_SE_SERVICE: &str = "Landroid/se/omapi/SEService;";
const P35E_READER: &str = "Landroid/se/omapi/Reader;";

#[test]
fn ssl_certificate_and_error_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let millis = 1_704_067_200_000i64; // whole seconds: format keeps s precision
    let not_before = p49_new(
        &mut engine,
        "Ljava/util/Date;",
        "(J)V",
        &[Value::Long(millis)],
    );
    let not_after = p49_new(
        &mut engine,
        "Ljava/util/Date;",
        "(J)V",
        &[Value::Long(millis + 86_400_000)],
    );
    let issued_to = text(&mut engine, "CN=example.com, O=Example Inc, OU=IT, C=US");
    let issued_by = text(&mut engine, "CN=Doe\\, John, O=Root CA");
    let cert = p49_new(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/util/Date;Ljava/util/Date;)V",
        &[
            issued_to,
            issued_by,
            Value::Obj(not_before),
            Value::Obj(not_after),
        ],
    );
    let to = object(call(
        &mut engine,
        cert,
        "getIssuedTo",
        "()Landroid/net/http/SslCertificate$DName;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, to, "getDName", "()Ljava/lang/String;", &[]),
        "CN=example.com, O=Example Inc, OU=IT, C=US"
    );
    assert_eq!(
        call_text(&mut engine, to, "getCName", "()Ljava/lang/String;", &[]),
        "example.com"
    );
    assert_eq!(
        call_text(&mut engine, to, "getOName", "()Ljava/lang/String;", &[]),
        "Example Inc"
    );
    assert_eq!(
        call_text(&mut engine, to, "getUName", "()Ljava/lang/String;", &[]),
        "IT"
    );
    let by = object(call(
        &mut engine,
        cert,
        "getIssuedBy",
        "()Landroid/net/http/SslCertificate$DName;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, by, "getCName", "()Ljava/lang/String;", &[]),
        "Doe, John"
    );
    assert_eq!(
        call_text(&mut engine, by, "getUName", "()Ljava/lang/String;", &[]),
        ""
    );
    // Date getters clone: same time, different object.
    let before_date = object(call(
        &mut engine,
        cert,
        "getValidNotBeforeDate",
        "()Ljava/util/Date;",
        &[],
    ));
    assert_ne!(before_date, not_before);
    assert_eq!(
        call(&mut engine, before_date, "getTime", "()J", &[]),
        Value::Long(millis)
    );
    assert_eq!(
        call(
            &mut engine,
            cert,
            "getX509Certificate",
            "()Ljava/security/cert/X509Certificate;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call_text(&mut engine, cert, "toString", "()Ljava/lang/String;", &[]),
        "Issued to: CN=example.com, O=Example Inc, OU=IT, C=US;\nIssued by: CN=Doe\\, John, O=Root CA;\n"
    );
    // saveState/restoreState round trip preserves names and date millis.
    let bundle = object(call_static(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "saveState",
        "(Landroid/net/http/SslCertificate;)Landroid/os/Bundle;",
        &[Value::Obj(cert)],
    ));
    let key = text(&mut engine, "issued-to");
    assert_eq!(
        call_text(
            &mut engine,
            bundle,
            "getString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[key]
        ),
        "CN=example.com, O=Example Inc, OU=IT, C=US"
    );
    let restored = object(call_static(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "restoreState",
        "(Landroid/os/Bundle;)Landroid/net/http/SslCertificate;",
        &[Value::Obj(bundle)],
    ));
    let restored_before = object(call(
        &mut engine,
        restored,
        "getValidNotBeforeDate",
        "()Ljava/util/Date;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, restored_before, "getTime", "()J", &[]),
        Value::Long(millis)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P35E_SSL_CERTIFICATE,
            "saveState",
            "(Landroid/net/http/SslCertificate;)Landroid/os/Bundle;",
            &[Value::Null],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P35E_SSL_CERTIFICATE,
            "restoreState",
            "(Landroid/os/Bundle;)Landroid/net/http/SslCertificate;",
            &[Value::Null],
        ),
        Value::Null
    );
    // Unparseable date strings degrade to null dates ("" formatted).
    let bad_to = text(&mut engine, "CN=x");
    let bad_by = text(&mut engine, "CN=y");
    let bad_date = text(&mut engine, "not a date");
    let bad_date2 = text(&mut engine, "also bad");
    let bad = p49_new(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[bad_to, bad_by, bad_date, bad_date2],
    );
    assert_eq!(
        call(
            &mut engine,
            bad,
            "getValidNotBeforeDate",
            "()Ljava/util/Date;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call_text(
            &mut engine,
            bad,
            "getValidNotBefore",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );

    // SslError bitmask semantics.
    let url = text(&mut engine, "https://example.com/");
    let error = p49_new(
        &mut engine,
        P35E_SSL_ERROR,
        "(ILandroid/net/http/SslCertificate;Ljava/lang/String;)V",
        &[Value::Int(1), Value::Obj(cert), url],
    );
    assert_eq!(
        call(&mut engine, error, "getPrimaryError", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, error, "hasError", "(I)Z", &[Value::Int(1)]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, error, "hasError", "(I)Z", &[Value::Int(3)]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, error, "addError", "(I)Z", &[Value::Int(3)]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, error, "getPrimaryError", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, error, "addError", "(I)Z", &[Value::Int(6)]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, error, "hasError", "(I)Z", &[Value::Int(-1)]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            error,
            "getCertificate",
            "()Landroid/net/http/SslCertificate;",
            &[]
        ),
        Value::Obj(cert)
    );
    let shown = call_text(&mut engine, error, "toString", "()Ljava/lang/String;", &[]);
    assert!(
        shown.starts_with("primary error: 3 certificate: Issued to: CN=example.com"),
        "{shown}"
    );
    assert!(shown.ends_with("on URL: https://example.com/"), "{shown}");
    let chromium_url = text(&mut engine, "https://example.com/");
    let chromium = object(call_static(
        &mut engine,
        P35E_SSL_ERROR,
        "SslErrorFromChromiumErrorCode",
        "(ILandroid/net/http/SslCertificate;Ljava/lang/String;)Landroid/net/http/SslError;",
        &[Value::Int(-200), Value::Obj(cert), chromium_url],
    ));
    assert_eq!(
        call(&mut engine, chromium, "getPrimaryError", "()I", &[]),
        Value::Int(2)
    );
}

#[test]
fn ssl_certificate_deep_audit_fixes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let millis = 1_704_067_200_000i64;

    // cloneDate is `(Date) date.clone()`: a Date subclass instance
    // keeps its dynamic type and its own fields through SslCertificate.
    let stamp = instance(&mut engine, "Ljava/sql/Timestamp;");
    engine
        .heap
        .put_field(stamp, "rd_ms", Value::Long(millis))
        .unwrap();
    engine
        .heap
        .put_field(stamp, "nanos", Value::Int(123_456_789))
        .unwrap();
    let stamp_after = instance(&mut engine, "Ljava/sql/Timestamp;");
    engine
        .heap
        .put_field(stamp_after, "rd_ms", Value::Long(millis + 1_000))
        .unwrap();
    engine
        .heap
        .put_field(stamp_after, "nanos", Value::Int(987_654_321))
        .unwrap();
    let issued_to = text(&mut engine, "CN=example.com");
    let issued_by = text(&mut engine, "CN=Root CA");
    let cert = p49_new(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/util/Date;Ljava/util/Date;)V",
        &[
            issued_to,
            issued_by,
            Value::Obj(stamp),
            Value::Obj(stamp_after),
        ],
    );
    let returned = object(call(
        &mut engine,
        cert,
        "getValidNotBeforeDate",
        "()Ljava/util/Date;",
        &[],
    ));
    assert_ne!(returned, stamp);
    assert_eq!(
        engine.heap.class_of(returned).unwrap(),
        "Ljava/sql/Timestamp;"
    );
    assert_eq!(field(&engine, returned, "rd_ms"), Value::Long(millis));
    assert_eq!(field(&engine, returned, "nanos"), Value::Int(123_456_789));
    // Mutating the returned clone does not reach the certificate.
    engine
        .heap
        .put_field(returned, "nanos", Value::Int(1))
        .unwrap();
    let returned_again = object(call(
        &mut engine,
        cert,
        "getValidNotBeforeDate",
        "()Ljava/util/Date;",
        &[],
    ));
    assert_eq!(
        field(&engine, returned_again, "nanos"),
        Value::Int(123_456_789)
    );

    // Undecodable x509 bytes in restoreState degrade to a null X.509
    // certificate instead of failing the restore.
    let bundle = p41_bundle(&mut engine);
    for (key, value) in [
        ("issued-to", "CN=example.com"),
        ("issued-by", "CN=Root CA"),
        ("valid-not-before", "2024-01-01 00:00:00+0000"),
        ("valid-not-after", "2025-01-01 00:00:00+0000"),
    ] {
        let key = text(&mut engine, key);
        let value = text(&mut engine, value);
        call(
            &mut engine,
            bundle,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[key, value],
        );
    }
    let key = text(&mut engine, "x509-certificate");
    let garbage = byte_array(&mut engine, &[1, 2, 3]);
    call(
        &mut engine,
        bundle,
        "putByteArray",
        "(Ljava/lang/String;[B)V",
        &[key, garbage],
    );
    let restored = object(call_static(
        &mut engine,
        P35E_SSL_CERTIFICATE,
        "restoreState",
        "(Landroid/os/Bundle;)Landroid/net/http/SslCertificate;",
        &[Value::Obj(bundle)],
    ));
    assert_eq!(
        call(
            &mut engine,
            restored,
            "getX509Certificate",
            "()Ljava/security/cert/X509Certificate;",
            &[]
        ),
        Value::Null
    );
    let before = object(call(
        &mut engine,
        restored,
        "getValidNotBeforeDate",
        "()Ljava/util/Date;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, before, "getTime", "()J", &[]),
        Value::Long(millis)
    );
}

#[test]
fn omapi_without_a_secure_element_service_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, "Landroid/content/ContextWrapper;");
    let executor = instance(&mut engine, "Ljava/util/concurrent/Executor;");
    let listener = instance(
        &mut engine,
        "Landroid/se/omapi/SEService$OnConnectedListener;",
    );
    // Null arguments are rejected before any binding attempt.
    let rejected = instance(&mut engine, P35E_SE_SERVICE);
    assert_throws(
        engine.invoke_instance(
            rejected,
            "<init>",
            "(Landroid/content/Context;Ljava/util/concurrent/Executor;Landroid/se/omapi/SEService$OnConnectedListener;)V",
            &[Value::Null, Value::Obj(executor), Value::Obj(listener)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let service = p49_new(
        &mut engine,
        P35E_SE_SERVICE,
        "(Landroid/content/Context;Ljava/util/concurrent/Executor;Landroid/se/omapi/SEService$OnConnectedListener;)V",
        &[Value::Obj(context), Value::Obj(executor), Value::Obj(listener)],
    );
    assert_eq!(
        call(&mut engine, service, "isConnected", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            service,
            "getVersion",
            "()Ljava/lang/String;",
            &[]
        ),
        "3.3"
    );
    // The connected callback never fires without the SE service.
    assert_eq!(
        p49_field_or_null(&engine, listener, "rd_count"),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(service, "getReaders", "()[Landroid/se/omapi/Reader;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "getUiccReader",
            "(I)Landroid/se/omapi/Reader;",
            &[Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "getUiccReader",
            "(I)Landroid/se/omapi/Reader;",
            &[Value::Int(1)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    // Reader obeys its disconnected-service contract.
    let reader_iface = instance(&mut engine, "Landroid/se/omapi/ISecureElementReader;");
    let reader_name = text(&mut engine, "SIM1");
    let rejected_reader = instance(&mut engine, P35E_READER);
    assert_throws(
        engine.invoke_instance(
            rejected_reader,
            "<init>",
            "(Landroid/se/omapi/SEService;Ljava/lang/String;Landroid/se/omapi/ISecureElementReader;)V",
            &[Value::Obj(service), Value::Null, Value::Obj(reader_iface)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let reader = p49_new(
        &mut engine,
        P35E_READER,
        "(Landroid/se/omapi/SEService;Ljava/lang/String;Landroid/se/omapi/ISecureElementReader;)V",
        &[Value::Obj(service), reader_name, Value::Obj(reader_iface)],
    );
    assert_eq!(
        call_text(&mut engine, reader, "getName", "()Ljava/lang/String;", &[]),
        "SIM1"
    );
    assert_eq!(
        call(
            &mut engine,
            reader,
            "getSEService",
            "()Landroid/se/omapi/SEService;",
            &[]
        ),
        Value::Obj(service)
    );
    assert_throws(
        engine.invoke_instance(reader, "openSession", "()Landroid/se/omapi/Session;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(reader, "isSecureElementPresent", "()Z", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_eq!(
        call(&mut engine, reader, "reset", "()Z", &[]),
        Value::Int(0)
    );
    call(&mut engine, reader, "closeSessions", "()V", &[]);
    call(&mut engine, service, "shutdown", "()V", &[]);
    assert_eq!(
        call(&mut engine, service, "isConnected", "()Z", &[]),
        Value::Int(0)
    );
}

// ---- Appendix C Batch 36-A: android.opengl + android.security.identity ----

const P36A_MATRIX: &str = "Landroid/opengl/Matrix;";
const P36A_IDENTITY_STORE: &str = "Landroid/security/identity/IdentityCredentialStore;";

fn p36a_float_values(engine: &Engine, value: &Value) -> Vec<f32> {
    array_values(engine, value)
        .iter()
        .map(|value| match value {
            Value::Float(f) => *f,
            other => panic!("expected float array element, got {other:?}"),
        })
        .collect()
}

fn p36a_assert_near(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() < 0.00001,
            "index {index}: {actual} != {expected} in {actual:?}"
        );
    }
}

#[test]
fn opengl_matrix_math_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine.cp.is_subtype(P36A_MATRIX, "Ljava/lang/Object;"));

    // Identity can be written at an offset without disturbing neighbors.
    let offset_result = float_value_array(&mut engine, &[7.0; 20]);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "setIdentityM",
        "([FI)V",
        &[offset_result.clone(), Value::Int(2)],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &offset_result),
        &[
            7.0, 7.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0, 7.0, 7.0,
        ],
    );

    assert_eq!(
        call_static(
            &mut engine,
            P36A_MATRIX,
            "length",
            "(FFF)F",
            &[Value::Float(3.0), Value::Float(4.0), Value::Float(12.0)],
        ),
        Value::Float(13.0)
    );

    let translation = float_value_array(
        &mut engine,
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0,
        ],
    );
    let scale = float_value_array(
        &mut engine,
        &[
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    let product = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "multiplyMM",
        "([FI[FI[FI)V",
        &[
            product.clone(),
            Value::Int(0),
            translation,
            Value::Int(0),
            scale.clone(),
            Value::Int(0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &product),
        &[
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 1.0, 2.0, 3.0, 1.0,
        ],
    );

    let vector = float_value_array(&mut engine, &[1.0, 1.0, 1.0, 1.0]);
    let transformed = float_array(&mut engine, 4);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "multiplyMV",
        "([FI[FI[FI)V",
        &[
            transformed.clone(),
            Value::Int(0),
            product.clone(),
            Value::Int(0),
            vector,
            Value::Int(0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &transformed),
        &[3.0, 5.0, 7.0, 1.0],
    );

    // The inverse undoes both scale and translation.
    let inverse = float_array(&mut engine, 16);
    assert_eq!(
        call_static(
            &mut engine,
            P36A_MATRIX,
            "invertM",
            "([FI[FI)Z",
            &[
                inverse.clone(),
                Value::Int(0),
                product.clone(),
                Value::Int(0)
            ],
        ),
        Value::Int(1)
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &inverse),
        &[
            0.5,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / 3.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.25,
            0.0,
            -0.5,
            -2.0 / 3.0,
            -0.75,
            1.0,
        ],
    );
    let singular = float_array(&mut engine, 16);
    let untouched = float_value_array(&mut engine, &[9.0; 16]);
    assert_eq!(
        call_static(
            &mut engine,
            P36A_MATRIX,
            "invertM",
            "([FI[FI)Z",
            &[untouched.clone(), Value::Int(0), singular, Value::Int(0)],
        ),
        Value::Int(0)
    );
    p36a_assert_near(&p36a_float_values(&engine, &untouched), &[9.0; 16]);

    let sequence: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let sequence_array = float_value_array(&mut engine, &sequence);
    let transposed = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "transposeM",
        "([FI[FI)V",
        &[
            transposed.clone(),
            Value::Int(0),
            sequence_array,
            Value::Int(0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &transposed),
        &[
            0.0, 4.0, 8.0, 12.0, 1.0, 5.0, 9.0, 13.0, 2.0, 6.0, 10.0, 14.0, 3.0, 7.0, 11.0, 15.0,
        ],
    );

    // Out-of-place translation reads the source columns before adding
    // the scaled translation column.
    let translated = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "translateM",
        "([FI[FIFFF)V",
        &[
            translated.clone(),
            Value::Int(0),
            scale,
            Value::Int(0),
            Value::Float(1.0),
            Value::Float(1.0),
            Value::Float(1.0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &translated),
        &[
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 2.0, 3.0, 4.0, 1.0,
        ],
    );

    let ortho = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "orthoM",
        "([FIFFFFFF)V",
        &[
            ortho.clone(),
            Value::Int(0),
            Value::Float(-1.0),
            Value::Float(1.0),
            Value::Float(-2.0),
            Value::Float(2.0),
            Value::Float(1.0),
            Value::Float(3.0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &ortho),
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -2.0, 1.0,
        ],
    );

    let perspective = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "perspectiveM",
        "([FIFFFF)V",
        &[
            perspective.clone(),
            Value::Int(0),
            Value::Float(90.0),
            Value::Float(2.0),
            Value::Float(1.0),
            Value::Float(3.0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &perspective),
        &[
            0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -2.0, -1.0, 0.0, 0.0, -3.0, 0.0,
        ],
    );

    let rotation = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "setRotateM",
        "([FIFFFF)V",
        &[
            rotation.clone(),
            Value::Int(0),
            Value::Float(90.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(1.0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &rotation),
        &[
            0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );

    // Deprecated and corrected Euler conversions intentionally differ
    // around Y, exactly as in AOSP.
    let deprecated = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "setRotateEulerM",
        "([FIFFF)V",
        &[
            deprecated.clone(),
            Value::Int(0),
            Value::Float(0.0),
            Value::Float(90.0),
            Value::Float(0.0),
        ],
    );
    let corrected = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "setRotateEulerM2",
        "([FIFFF)V",
        &[
            corrected.clone(),
            Value::Int(0),
            Value::Float(0.0),
            Value::Float(90.0),
            Value::Float(0.0),
        ],
    );
    let deprecated = p36a_float_values(&engine, &deprecated);
    let corrected = p36a_float_values(&engine, &corrected);
    assert!((deprecated[4] - 1.0).abs() < 0.00001);
    assert!(deprecated[8].abs() < 0.00001);
    assert!(corrected[4].abs() < 0.00001);
    assert!((corrected[8] + 1.0).abs() < 0.00001);

    let look_at = float_array(&mut engine, 16);
    call_static(
        &mut engine,
        P36A_MATRIX,
        "setLookAtM",
        "([FIFFFFFFFFF)V",
        &[
            look_at.clone(),
            Value::Int(0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(5.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(1.0),
            Value::Float(0.0),
        ],
    );
    p36a_assert_near(
        &p36a_float_values(&engine, &look_at),
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -5.0, 1.0,
        ],
    );

    // Explicit and implicit AOSP argument validation remains distinct.
    let valid = float_array(&mut engine, 16);
    assert_throws(
        engine.invoke_static(
            P36A_MATRIX,
            "multiplyMM",
            "([FI[FI[FI)V",
            &[
                Value::Null,
                Value::Int(0),
                valid.clone(),
                Value::Int(0),
                valid.clone(),
                Value::Int(0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P36A_MATRIX,
            "transposeM",
            "([FI[FI)V",
            &[valid.clone(), Value::Int(0), Value::Null, Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_static(
            P36A_MATRIX,
            "orthoM",
            "([FIFFFFFF)V",
            &[
                valid.clone(),
                Value::Int(0),
                Value::Float(1.0),
                Value::Float(1.0),
                Value::Float(-1.0),
                Value::Float(1.0),
                Value::Float(1.0),
                Value::Float(2.0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P36A_MATRIX,
            "setRotateEulerM2",
            "([FIFFF)V",
            &[
                Value::Null,
                Value::Int(0),
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Float(0.0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P36A_MATRIX,
            "frustumM",
            "([FIFFFFFF)V",
            &[
                valid,
                Value::Int(0),
                Value::Float(-1.0),
                Value::Float(1.0),
                Value::Float(-1.0),
                Value::Float(1.0),
                Value::Float(0.0),
                Value::Float(2.0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
}

#[test]
fn identity_without_credstore_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine.cp.is_subtype(
        "Landroid/security/identity/CredstoreIdentityCredentialStore;",
        P36A_IDENTITY_STORE,
    ));
    let context = instance(&mut engine, CONTEXT);
    for method in ["getInstance", "getDirectAccessInstance"] {
        assert_eq!(
            call_static(
                &mut engine,
                P36A_IDENTITY_STORE,
                method,
                "(Landroid/content/Context;)Landroid/security/identity/IdentityCredentialStore;",
                &[Value::Obj(context)],
            ),
            Value::Null
        );
    }
    // AOSP looks up the missing binder before it ever dereferences the
    // Context, so even a null Context returns the documented null. Both
    // factories and the Credstore subclass entry points share that path.
    for class in [
        P36A_IDENTITY_STORE,
        "Landroid/security/identity/CredstoreIdentityCredentialStore;",
    ] {
        for method in ["getInstance", "getDirectAccessInstance"] {
            assert_eq!(
                call_static(
                    &mut engine,
                    class,
                    method,
                    "(Landroid/content/Context;)Landroid/security/identity/IdentityCredentialStore;",
                    &[Value::Null],
                ),
                Value::Null
            );
        }
    }

    // The public base-class presentation-session implementation is the
    // documented UnsupportedOperationException.
    let store = instance(&mut engine, P36A_IDENTITY_STORE);
    call(&mut engine, store, "<init>", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            store,
            "createPresentationSession",
            "(I)Landroid/security/identity/PresentationSession;",
            &[Value::Int(1)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
}

// ---- Appendix C Batch 36-B: android.hardware.Camera + android.nfc ----

const P36B_CAMERA: &str = "Landroid/hardware/Camera;";
const P36B_CAMERA_INFO: &str = "Landroid/hardware/Camera$CameraInfo;";
const P36B_CAMERA_SIZE: &str = "Landroid/hardware/Camera$Size;";
const P36B_CAMERA_AREA: &str = "Landroid/hardware/Camera$Area;";
const P36B_NFC_ADAPTER: &str = "Landroid/nfc/NfcAdapter;";
const P36B_NFC_MANAGER: &str = "Landroid/nfc/NfcManager;";

#[test]
fn legacy_camera_without_a_camera_and_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert_eq!(
        call_static(&mut engine, P36B_CAMERA, "getNumberOfCameras", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36B_CAMERA,
            "open",
            "()Landroid/hardware/Camera;",
            &[],
        ),
        Value::Null
    );

    let info = p49_new(&mut engine, P36B_CAMERA_INFO, "()V", &[]);
    assert_eq!(field(&engine, info, "facing"), Value::Int(0));
    assert_eq!(field(&engine, info, "orientation"), Value::Int(0));
    assert_eq!(
        field(&engine, info, "canDisableShutterSound"),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P36B_CAMERA,
            "getCameraInfo",
            "(ILandroid/hardware/Camera$CameraInfo;)V",
            &[Value::Int(0), Value::Obj(info)],
        ),
        "Ljava/lang/RuntimeException;",
    );
    assert_throws(
        engine.invoke_static(
            P36B_CAMERA,
            "open",
            "(I)Landroid/hardware/Camera;",
            &[Value::Int(0)],
        ),
        "Ljava/lang/RuntimeException;",
    );
    assert_throws(
        engine.invoke_static(
            P36B_CAMERA,
            "openLegacy",
            "(II)Landroid/hardware/Camera;",
            &[Value::Int(0), Value::Int(0x100)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_static(
            P36B_CAMERA,
            "openLegacy",
            "(II)Landroid/hardware/Camera;",
            &[Value::Int(0), Value::Int(0x300)],
        ),
        "Ljava/lang/RuntimeException;",
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36B_CAMERA,
            "checkInitErrors",
            "(I)Z",
            &[Value::Int(0)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36B_CAMERA,
            "checkInitErrors",
            "(I)Z",
            &[Value::Int(-13)],
        ),
        Value::Int(1)
    );

    // The hidden testing constructor does not connect to a device, but
    // supplies the enclosing instance required by Camera.Size.
    let camera = object(call_static(
        &mut engine,
        P36B_CAMERA,
        "openUninitialized",
        "()Landroid/hardware/Camera;",
        &[],
    ));
    let size = p49_new(
        &mut engine,
        P36B_CAMERA_SIZE,
        "(Landroid/hardware/Camera;II)V",
        &[Value::Obj(camera), Value::Int(640), Value::Int(480)],
    );
    let same_size = p49_new(
        &mut engine,
        P36B_CAMERA_SIZE,
        "(Landroid/hardware/Camera;II)V",
        &[Value::Obj(camera), Value::Int(640), Value::Int(480)],
    );
    let other_size = p49_new(
        &mut engine,
        P36B_CAMERA_SIZE,
        "(Landroid/hardware/Camera;II)V",
        &[Value::Obj(camera), Value::Int(800), Value::Int(480)],
    );
    assert_eq!(field(&engine, size, "width"), Value::Int(640));
    assert_eq!(field(&engine, size, "height"), Value::Int(480));
    assert_eq!(
        call(
            &mut engine,
            size,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_size)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            size,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(other_size)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            size,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, size, "hashCode", "()I", &[]),
        Value::Int(640 * 32713 + 480)
    );

    let area = p49_new(
        &mut engine,
        P36B_CAMERA_AREA,
        "(Landroid/graphics/Rect;I)V",
        &[Value::Null, Value::Int(37)],
    );
    let same_area = p49_new(
        &mut engine,
        P36B_CAMERA_AREA,
        "(Landroid/graphics/Rect;I)V",
        &[Value::Null, Value::Int(37)],
    );
    assert_eq!(field(&engine, area, "rect"), Value::Null);
    assert_eq!(field(&engine, area, "weight"), Value::Int(37));
    assert_eq!(
        call(
            &mut engine,
            area,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_area)],
        ),
        Value::Int(1)
    );

    // Size.equals compares only width/height; the synthetic enclosing
    // Camera of the non-static inner class is ignored, so sizes from two
    // different (uninitialized) cameras can still be equal.
    let other_camera = object(call_static(
        &mut engine,
        P36B_CAMERA,
        "openUninitialized",
        "()Landroid/hardware/Camera;",
        &[],
    ));
    let cross_size = p49_new(
        &mut engine,
        P36B_CAMERA_SIZE,
        "(Landroid/hardware/Camera;II)V",
        &[Value::Obj(other_camera), Value::Int(640), Value::Int(480)],
    );
    assert_eq!(
        call(
            &mut engine,
            size,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(cross_size)],
        ),
        Value::Int(1)
    );
    // Area.equals short-circuits a null rect on either side.
    let rect = p49_new(&mut engine, "Landroid/graphics/Rect;", "()V", &[]);
    let rect_area = p49_new(
        &mut engine,
        P36B_CAMERA_AREA,
        "(Landroid/graphics/Rect;I)V",
        &[Value::Obj(rect), Value::Int(37)],
    );
    assert_eq!(
        call(
            &mut engine,
            area,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(rect_area)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            rect_area,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(area)],
        ),
        Value::Int(0)
    );
}

#[test]
fn nfc_without_nfc_feature_or_service_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let service_name = text(&mut engine, "nfc");
    let service = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    assert!(engine.cp.is_subtype(P36B_NFC_MANAGER, "Ljava/lang/Object;"));
    assert_eq!(
        call(
            &mut engine,
            service,
            "getDefaultAdapter",
            "()Landroid/nfc/NfcAdapter;",
            &[],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36B_NFC_ADAPTER,
            "getDefaultAdapter",
            "(Landroid/content/Context;)Landroid/nfc/NfcAdapter;",
            &[Value::Obj(context)],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_static(
            P36B_NFC_ADAPTER,
            "getDefaultAdapter",
            "(Landroid/content/Context;)Landroid/nfc/NfcAdapter;",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let manager = p49_new(
        &mut engine,
        P36B_NFC_MANAGER,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert_eq!(field(&engine, manager, "mAdapter"), Value::Null);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getDefaultAdapter",
            "()Landroid/nfc/NfcAdapter;",
            &[],
        ),
        Value::Null
    );
    let rejected = instance(&mut engine, P36B_NFC_MANAGER);
    assert_throws(
        engine.invoke_instance(
            rejected,
            "<init>",
            "(Landroid/content/Context;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // The deprecated no-arg overload takes getNfcAdapter(null)'s first
    // branch: a cached null-context adapter even with no NFC hardware.
    let legacy = call_static(
        &mut engine,
        P36B_NFC_ADAPTER,
        "getDefaultAdapter",
        "()Landroid/nfc/NfcAdapter;",
        &[],
    );
    let legacy_adapter = object(legacy.clone());
    assert_eq!(
        call(
            &mut engine,
            legacy_adapter,
            "getContext",
            "()Landroid/content/Context;",
            &[],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36B_NFC_ADAPTER,
            "getDefaultAdapter",
            "()Landroid/nfc/NfcAdapter;",
            &[],
        ),
        legacy,
        "the null-context adapter is a process singleton in AOSP"
    );
}

// ---- Appendix C Batch 36-C: android.net.nsd + android.net.sip ----

const P36C_NSD_INFO: &str = "Landroid/net/nsd/NsdServiceInfo;";
const P36C_NSD_MANAGER: &str = "Landroid/net/nsd/NsdManager;";
const P36C_SIP_MANAGER: &str = "Landroid/net/sip/SipManager;";
const P36C_SIP_EXCEPTION: &str = "Landroid/net/sip/SipException;";

#[test]
fn nsd_service_info_txt_records_and_unpublished_service_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let service_name = text(&mut engine, "nsd");
    assert_throws(
        engine.invoke_instance(
            context,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[service_name],
        ),
        "Landroid/os/ServiceManager$ServiceNotFoundException;",
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36C_NSD_MANAGER,
            "nameOf",
            "(I)Ljava/lang/String;",
            &[Value::Int(1)],
        ),
        "DISCOVER_SERVICES"
    );
    // AOSP's EVENT_NAMES map intentionally has no entry for code 30.
    assert_eq!(
        call_static_text(
            &mut engine,
            P36C_NSD_MANAGER,
            "nameOf",
            "(I)Ljava/lang/String;",
            &[Value::Int(30)],
        ),
        "30"
    );
    assert!(engine
        .cp
        .is_subtype(P36C_NSD_INFO, "Landroid/os/Parcelable;"));

    let info = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let name = text(&mut engine, "Printer");
    call(
        &mut engine,
        info,
        "setServiceName",
        "(Ljava/lang/String;)V",
        &[name],
    );
    let service_type = text(&mut engine, "_ipp._tcp");
    call(
        &mut engine,
        info,
        "setServiceType",
        "(Ljava/lang/String;)V",
        &[service_type],
    );
    call(&mut engine, info, "setPort", "(I)V", &[Value::Int(631)]);
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getServiceName",
            "()Ljava/lang/String;",
            &[]
        ),
        "Printer"
    );
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getServiceType",
            "()Ljava/lang/String;",
            &[]
        ),
        "_ipp._tcp"
    );
    assert_eq!(
        call(&mut engine, info, "getPort", "()I", &[]),
        Value::Int(631)
    );

    let address = instance(&mut engine, "Ljava/net/InetAddress;");
    let addresses = list_of(&mut engine, &[Value::Obj(address)]);
    call(
        &mut engine,
        info,
        "setHostAddresses",
        "(Ljava/util/List;)V",
        &[Value::Obj(addresses)],
    );
    assert_eq!(
        call(
            &mut engine,
            info,
            "getHost",
            "()Ljava/net/InetAddress;",
            &[]
        ),
        Value::Obj(address)
    );
    let returned_addresses = object(call(
        &mut engine,
        info,
        "getHostAddresses",
        "()Ljava/util/List;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, returned_addresses, "size", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            returned_addresses,
            "get",
            "(I)Ljava/lang/Object;",
            &[Value::Int(0)],
        ),
        Value::Obj(address)
    );
    let empty_addresses = list_of(&mut engine, &[]);
    call(
        &mut engine,
        info,
        "setHostAddresses",
        "(Ljava/util/List;)V",
        &[Value::Obj(empty_addresses)],
    );

    let path_key = text(&mut engine, "path");
    let path_value = text(&mut engine, "/x");
    call(
        &mut engine,
        info,
        "setAttribute",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[path_key, path_value],
    );
    let flag_key = text(&mut engine, "flag");
    call(
        &mut engine,
        info,
        "setAttribute",
        "(Ljava/lang/String;[B)V",
        &[flag_key, Value::Null],
    );
    let attributes = object(call(
        &mut engine,
        info,
        "getAttributes",
        "()Ljava/util/Map;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, attributes, "size", "()I", &[]),
        Value::Int(2)
    );
    let path_lookup = text(&mut engine, "path");
    let stored_path = call(
        &mut engine,
        attributes,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[path_lookup],
    );
    assert_eq!(byte_array_values(&engine, &stored_path), b"/x");
    let txt_record = call(&mut engine, info, "getTxtRecord", "()[B", &[]);
    assert_eq!(
        byte_array_values(&engine, &txt_record),
        vec![7, b'p', b'a', b't', b'h', b'=', b'/', b'x', 5, b'f', b'l', b'a', b'g', b'=']
    );
    let bad_key = text(&mut engine, "bad=key");
    let bad_value = text(&mut engine, "x");
    assert_throws(
        engine.invoke_instance(
            info,
            "setAttribute",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[bad_key, bad_value],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: AOSP wraps the live map in
    // Collections.unmodifiableMap, so mutating the returned map throws
    // instead of silently mutating a copy.
    let extra_key = text(&mut engine, "extra");
    assert_throws(
        engine.invoke_instance(
            attributes,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[extra_key, Value::Null],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );

    // Deep audit: the 1300-byte accounting double-counts an overwritten
    // entry (AOSP sizes the future record from the current size plus
    // the new entry), and a single key+value pair is capped below 255.
    let sized = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let k_key = text(&mut engine, "k");
    let long_value = "v".repeat(254);
    let long_value = text(&mut engine, &long_value);
    assert_throws(
        engine.invoke_instance(
            sized,
            "setAttribute",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[k_key, long_value],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    // Five entries of 254 counted bytes each reach 1270 of the 1300
    // budget (each entry stays under the per-entry 255 cap).
    for index in 0..5 {
        let key = format!("k{index}");
        let key = text(&mut engine, &key);
        let value = "x".repeat(250);
        let value = text(&mut engine, &value);
        call(
            &mut engine,
            sized,
            "setAttribute",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[key, value],
        );
    }
    // Overwriting k0 with a 100-byte value would shrink the record,
    // but AOSP adds the new entry to the current size without
    // subtracting the replaced one: 1270 + 104 > 1300, so it throws.
    let a_key = text(&mut engine, "k0");
    let small_value = "y".repeat(100);
    let small_value = text(&mut engine, &small_value);
    assert_throws(
        engine.invoke_instance(
            sized,
            "setAttribute",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[a_key, small_value],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: a wire record "key=" (equals sign as the last byte)
    // stores a null value, exactly like a boolean record without '='.
    let equals_only = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let equals_raw = byte_array(&mut engine, &[4, b'k', b'e', b'y', b'=']);
    call(
        &mut engine,
        equals_only,
        "setTxtRecords",
        "([B)V",
        &[equals_raw],
    );
    let equals_attributes = object(call(
        &mut engine,
        equals_only,
        "getAttributes",
        "()Ljava/util/Map;",
        &[],
    ));
    let key_lookup = text(&mut engine, "key");
    assert_eq!(
        call(
            &mut engine,
            equals_attributes,
            "containsKey",
            "(Ljava/lang/Object;)Z",
            &[key_lookup],
        ),
        Value::Int(1)
    );
    let key_lookup = text(&mut engine, "key");
    assert_eq!(
        call(
            &mut engine,
            equals_attributes,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[key_lookup],
        ),
        Value::Null
    );

    let parsed = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let mut raw = Vec::new();
    raw.extend([12]);
    raw.extend(b"name=printer");
    raw.extend([6]);
    raw.extend(b"secure");
    raw.extend([10]);
    raw.extend(b"name=other");
    raw.extend([0, 9]);
    raw.extend(b"path=/ipp");
    raw.extend([4, 1, b'x', b'=', b'v']);
    let raw = byte_array(&mut engine, &raw);
    call(&mut engine, parsed, "setTxtRecords", "([B)V", &[raw]);
    let parsed_txt_record = call(&mut engine, parsed, "getTxtRecord", "()[B", &[]);
    assert_eq!(
        byte_array_values(&engine, &parsed_txt_record),
        b"\x0cname=printer\x07secure=\x09path=/ipp".to_vec()
    );
    let secure = text(&mut engine, "secure");
    call(
        &mut engine,
        parsed,
        "removeAttribute",
        "(Ljava/lang/String;)V",
        &[secure],
    );
    let parsed_attributes = object(call(
        &mut engine,
        parsed,
        "getAttributes",
        "()Ljava/util/Map;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, parsed_attributes, "size", "()I", &[]),
        Value::Int(2)
    );

    let shown = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let shown_name = text(&mut engine, "Printer");
    call(
        &mut engine,
        shown,
        "setServiceName",
        "(Ljava/lang/String;)V",
        &[shown_name],
    );
    let shown_type = text(&mut engine, "_ipp._tcp");
    call(
        &mut engine,
        shown,
        "setServiceType",
        "(Ljava/lang/String;)V",
        &[shown_type],
    );
    call(&mut engine, shown, "setPort", "(I)V", &[Value::Int(631)]);
    call(
        &mut engine,
        shown,
        "setInterfaceIndex",
        "(I)V",
        &[Value::Int(7)],
    );
    assert_eq!(
        call(&mut engine, shown, "getInterfaceIndex", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call_text(&mut engine, shown, "toString", "()Ljava/lang/String;", &[]),
        "name: Printer, type: _ipp._tcp, hostAddresses: , port: 631, network: null, txtRecord: "
    );
}

#[test]
fn nsd_manager_validation_and_listener_bookkeeping_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = instance(&mut engine, P36C_NSD_MANAGER);
    let listener = instance(
        &mut engine,
        "Landroid/net/nsd/NsdManager$DiscoveryListener;",
    );
    let empty_type = text(&mut engine, "");
    assert_throws(
        engine.invoke_instance(
            manager,
            "discoverServices",
            "(Ljava/lang/String;ILandroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[empty_type, Value::Int(1), Value::Obj(listener)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let service_type = text(&mut engine, "_http._tcp");
    assert_throws(
        engine.invoke_instance(
            manager,
            "discoverServices",
            "(Ljava/lang/String;ILandroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[service_type, Value::Int(9), Value::Obj(listener)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let service_type = text(&mut engine, "_http._tcp");
    assert_throws(
        engine.invoke_instance(
            manager,
            "discoverServices",
            "(Ljava/lang/String;ILandroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[service_type, Value::Int(1), Value::Obj(listener)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // putListener ran before the absent-connector dereference, so the
    // same listener is now in use and can be found for stop requests.
    let service_type = text(&mut engine, "_http._tcp");
    assert_throws(
        engine.invoke_instance(
            manager,
            "discoverServices",
            "(Ljava/lang/String;ILandroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[service_type, Value::Int(1), Value::Obj(listener)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let other_listener = instance(
        &mut engine,
        "Landroid/net/nsd/NsdManager$DiscoveryListener;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "stopServiceDiscovery",
            "(Landroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[Value::Obj(other_listener)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "stopServiceDiscovery",
            "(Landroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[Value::Obj(listener)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let info = p49_new(&mut engine, P36C_NSD_INFO, "()V", &[]);
    let registration_listener = instance(
        &mut engine,
        "Landroid/net/nsd/NsdManager$RegistrationListener;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "registerService",
            "(Landroid/net/nsd/NsdServiceInfo;ILandroid/net/nsd/NsdManager$RegistrationListener;)V",
            &[
                Value::Obj(info),
                Value::Int(1),
                Value::Obj(registration_listener),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let resolve_listener = instance(&mut engine, "Landroid/net/nsd/NsdManager$ResolveListener;");
    assert_throws(
        engine.invoke_instance(
            manager,
            "resolveService",
            "(Landroid/net/nsd/NsdServiceInfo;Landroid/net/nsd/NsdManager$ResolveListener;)V",
            &[Value::Obj(info), Value::Obj(resolve_listener)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // Deep audit: checkListener runs inside putListener after the
    // type/protocol validation, so a null listener NPEs; registerService
    // dereferences the service info's port before its null check.
    let service_type = text(&mut engine, "_http._tcp");
    assert_throws(
        engine.invoke_instance(
            manager,
            "discoverServices",
            "(Ljava/lang/String;ILandroid/net/nsd/NsdManager$DiscoveryListener;)V",
            &[service_type, Value::Int(1), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "registerService",
            "(Landroid/net/nsd/NsdServiceInfo;ILandroid/net/nsd/NsdManager$RegistrationListener;)V",
            &[
                Value::Null,
                Value::Int(1),
                Value::Obj(registration_listener),
            ],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn sip_without_sip_features_or_service_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    assert_eq!(
        call_static(
            &mut engine,
            P36C_SIP_MANAGER,
            "isApiSupported",
            "(Landroid/content/Context;)Z",
            &[Value::Obj(context)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36C_SIP_MANAGER,
            "isVoipSupported",
            "(Landroid/content/Context;)Z",
            &[Value::Obj(context)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36C_SIP_MANAGER,
            "newInstance",
            "(Landroid/content/Context;)Landroid/net/sip/SipManager;",
            &[Value::Obj(context)],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_static(
            P36C_SIP_MANAGER,
            "isApiSupported",
            "(Landroid/content/Context;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // The same Context dereference drives isVoipSupported/newInstance, and
    // the Intent helpers dereference their Intent (unlike
    // isIncomingCallIntent's explicit null guard below).
    assert_throws(
        engine.invoke_static(
            P36C_SIP_MANAGER,
            "isVoipSupported",
            "(Landroid/content/Context;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_static(
            P36C_SIP_MANAGER,
            "newInstance",
            "(Landroid/content/Context;)Landroid/net/sip/SipManager;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_static(
            P36C_SIP_MANAGER,
            "getCallId",
            "(Landroid/content/Intent;)Ljava/lang/String;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36C_SIP_EXCEPTION, "Ljava/lang/Exception;"));

    let call_id = text(&mut engine, "call-17");
    let offer = text(&mut engine, "v=0");
    let intent = object(call_static(
        &mut engine,
        P36C_SIP_MANAGER,
        "createIncomingCallBroadcast",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[call_id, offer],
    ));
    assert_eq!(
        call_static_text(
            &mut engine,
            P36C_SIP_MANAGER,
            "getCallId",
            "(Landroid/content/Intent;)Ljava/lang/String;",
            &[Value::Obj(intent)],
        ),
        "call-17"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36C_SIP_MANAGER,
            "getOfferSessionDescription",
            "(Landroid/content/Intent;)Ljava/lang/String;",
            &[Value::Obj(intent)],
        ),
        "v=0"
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36C_SIP_MANAGER,
            "isIncomingCallIntent",
            "(Landroid/content/Intent;)Z",
            &[Value::Obj(intent)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36C_SIP_MANAGER,
            "isIncomingCallIntent",
            "(Landroid/content/Intent;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );

    let manager = p49_new(
        &mut engine,
        P36C_SIP_MANAGER,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert_eq!(field(&engine, manager, "mContext"), Value::Obj(context));
    assert_eq!(field(&engine, manager, "mSipService"), Value::Null);
    assert_throws(
        engine.invoke_instance(
            manager,
            "open",
            "(Landroid/net/sip/SipProfile;)V",
            &[Value::Null],
        ),
        P36C_SIP_EXCEPTION,
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "open",
            "(Landroid/net/sip/SipProfile;Landroid/app/PendingIntent;Landroid/net/sip/SipRegistrationListener;)V",
            &[Value::Null, Value::Null, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let pending = instance(&mut engine, "Landroid/app/PendingIntent;");
    assert_throws(
        engine.invoke_instance(
            manager,
            "open",
            "(Landroid/net/sip/SipProfile;Landroid/app/PendingIntent;Landroid/net/sip/SipRegistrationListener;)V",
            &[Value::Null, Value::Obj(pending), Value::Null],
        ),
        P36C_SIP_EXCEPTION,
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "makeAudioCall",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/net/sip/SipAudioCall$Listener;I)Landroid/net/sip/SipAudioCall;",
            &[Value::Null, Value::Null, Value::Null, Value::Int(30)],
        ),
        P36C_SIP_EXCEPTION,
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "takeAudioCall",
            "(Landroid/content/Intent;Landroid/net/sip/SipAudioCall$Listener;)Landroid/net/sip/SipAudioCall;",
            &[Value::Null, Value::Null],
        ),
        P36C_SIP_EXCEPTION,
    );
    let empty_intent = p49_new(&mut engine, "Landroid/content/Intent;", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            manager,
            "takeAudioCall",
            "(Landroid/content/Intent;Landroid/net/sip/SipAudioCall$Listener;)Landroid/net/sip/SipAudioCall;",
            &[Value::Obj(empty_intent), Value::Null],
        ),
        P36C_SIP_EXCEPTION,
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "takeAudioCall",
            "(Landroid/content/Intent;Landroid/net/sip/SipAudioCall$Listener;)Landroid/net/sip/SipAudioCall;",
            &[Value::Obj(intent), Value::Null],
        ),
        P36C_SIP_EXCEPTION,
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "getSessionFor",
            "(Landroid/content/Intent;)Landroid/net/sip/SipSession;",
            &[Value::Obj(intent)],
        ),
        P36C_SIP_EXCEPTION,
    );
}

// ---- Appendix C Batch 36-D: Bluetooth / LE + NFC CE/tech ----

const P36D_BT_ADAPTER: &str = "Landroid/bluetooth/BluetoothAdapter;";
const P36D_BT_MANAGER: &str = "Landroid/bluetooth/BluetoothManager;";
const P36D_BT_DEVICE: &str = "Landroid/bluetooth/BluetoothDevice;";
const P36D_SCAN_SETTINGS: &str = "Landroid/bluetooth/le/ScanSettings;";
const P36D_SCAN_SETTINGS_BUILDER: &str = "Landroid/bluetooth/le/ScanSettings$Builder;";
const P36D_SCAN_FILTER: &str = "Landroid/bluetooth/le/ScanFilter;";
const P36D_SCAN_FILTER_BUILDER: &str = "Landroid/bluetooth/le/ScanFilter$Builder;";
const P36D_TAG: &str = "Landroid/nfc/Tag;";
const P36D_NFC_A: &str = "Landroid/nfc/tech/NfcA;";
const P36D_CARD_EMULATION: &str = "Landroid/nfc/cardemulation/CardEmulation;";
const P36D_AID_GROUP: &str = "Landroid/nfc/cardemulation/AidGroup;";

#[test]
fn bluetooth_without_a_service_and_device_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    assert_eq!(
        call_static(
            &mut engine,
            P36D_BT_ADAPTER,
            "getDefaultAdapter",
            "()Landroid/bluetooth/BluetoothAdapter;",
            &[],
        ),
        Value::Null
    );
    let valid = text(&mut engine, "00:11:22:33:AA:BB");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_BT_ADAPTER,
            "checkBluetoothAddress",
            "(Ljava/lang/String;)Z",
            &[valid],
        ),
        Value::Int(1)
    );
    let lowercase = text(&mut engine, "00:11:22:33:aa:bb");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_BT_ADAPTER,
            "checkBluetoothAddress",
            "(Ljava/lang/String;)Z",
            &[lowercase],
        ),
        Value::Int(0)
    );
    let random_static = text(&mut engine, "C0:11:22:33:AA:BB");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_BT_ADAPTER,
            "isAddressRandomStatic",
            "(Ljava/lang/String;)Z",
            &[random_static],
        ),
        Value::Int(1)
    );
    assert_throws(
        engine.invoke_static(
            P36D_BT_ADAPTER,
            "isAddressRandomStatic",
            "(Ljava/lang/String;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36D_BT_ADAPTER,
            "nameForState",
            "(I)Ljava/lang/String;",
            &[Value::Int(99)],
        ),
        "?!?!? (99)"
    );

    let service_name = text(&mut engine, "bluetooth_manager");
    let system_manager = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    assert_eq!(
        call(
            &mut engine,
            system_manager,
            "getAdapter",
            "()Landroid/bluetooth/BluetoothAdapter;",
            &[],
        ),
        Value::Null
    );
    let manager = p49_new(
        &mut engine,
        P36D_BT_MANAGER,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert_eq!(field(&engine, manager, "mAdapter"), Value::Null);
    let states = int_array(&mut engine, &[2]);
    assert_throws(
        engine.invoke_instance(
            manager,
            "getDevicesMatchingConnectionStates",
            "(I[I)Ljava/util/List;",
            &[Value::Int(99), states.clone()],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "getDevicesMatchingConnectionStates",
            "(I[I)Ljava/util/List;",
            &[Value::Int(7), states],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "openGattServer",
            "(Landroid/content/Context;Landroid/bluetooth/BluetoothGattServerCallback;)Landroid/bluetooth/BluetoothGattServer;",
            &[Value::Null, Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let adapter = instance(&mut engine, P36D_BT_ADAPTER);
    assert_eq!(
        call(&mut engine, adapter, "getState", "()I", &[]),
        Value::Int(10)
    );
    assert_eq!(
        call(&mut engine, adapter, "isEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            adapter,
            "getBluetoothLeScanner",
            "()Landroid/bluetooth/le/BluetoothLeScanner;",
            &[],
        ),
        Value::Null
    );
    let address = text(&mut engine, "00:11:22:33:AA:BB");
    let device = object(call(
        &mut engine,
        adapter,
        "getRemoteDevice",
        "(Ljava/lang/String;)Landroid/bluetooth/BluetoothDevice;",
        &[address],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            device,
            "getAddress",
            "()Ljava/lang/String;",
            &[]
        ),
        "00:11:22:33:AA:BB"
    );
    let bytes = byte_array(&mut engine, &[0, 0x11, 0x22, 0x33, 0xaa, 0xbb]);
    let same_device = object(call(
        &mut engine,
        adapter,
        "getRemoteDevice",
        "([B)Landroid/bluetooth/BluetoothDevice;",
        &[bytes],
    ));
    assert_eq!(
        call(
            &mut engine,
            device,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_device)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, device, "hashCode", "()I", &[]),
        call(&mut engine, same_device, "hashCode", "()I", &[])
    );
    let le_address = text(&mut engine, "C0:11:22:33:AA:BB");
    let le_device = object(call(
        &mut engine,
        adapter,
        "getRemoteLeDevice",
        "(Ljava/lang/String;I)Landroid/bluetooth/BluetoothDevice;",
        &[le_address, Value::Int(1)],
    ));
    assert_eq!(
        call(&mut engine, le_device, "getAddressType", "()I", &[]),
        Value::Int(1)
    );
    let bad_address = text(&mut engine, "00-11-22-33-AA-BB");
    assert_throws(
        engine.invoke_instance(
            adapter,
            "getRemoteDevice",
            "(Ljava/lang/String;)Landroid/bluetooth/BluetoothDevice;",
            &[bad_address],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36D_BT_DEVICE, "Landroid/os/Parcelable;"));
}

#[test]
fn bluetooth_le_scan_settings_and_filters_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let builder = p49_new(&mut engine, P36D_SCAN_SETTINGS_BUILDER, "()V", &[]);
    let settings = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/bluetooth/le/ScanSettings;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, settings, "getScanMode", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, settings, "getCallbackType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, settings, "getReportDelayMillis", "()J", &[]),
        Value::Long(0)
    );
    assert_eq!(
        call(&mut engine, settings, "getLegacy", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, settings, "getPhy", "()I", &[]),
        Value::Int(255)
    );
    call(
        &mut engine,
        builder,
        "setScanMode",
        "(I)Landroid/bluetooth/le/ScanSettings$Builder;",
        &[Value::Int(2)],
    );
    call(
        &mut engine,
        builder,
        "setCallbackType",
        "(I)Landroid/bluetooth/le/ScanSettings$Builder;",
        &[Value::Int(6)],
    );
    call(
        &mut engine,
        builder,
        "setReportDelay",
        "(J)Landroid/bluetooth/le/ScanSettings$Builder;",
        &[Value::Long(2500)],
    );
    call(
        &mut engine,
        builder,
        "setLegacy",
        "(Z)Landroid/bluetooth/le/ScanSettings$Builder;",
        &[Value::Int(0)],
    );
    let configured = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/bluetooth/le/ScanSettings;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, configured, "getScanMode", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, configured, "getCallbackType", "()I", &[]),
        Value::Int(6)
    );
    assert_eq!(
        call(&mut engine, configured, "getReportDelayMillis", "()J", &[]),
        Value::Long(2500)
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "setScanMode",
            "(I)Landroid/bluetooth/le/ScanSettings$Builder;",
            &[Value::Int(99)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        builder,
        "setCallbackType",
        "(I)Landroid/bluetooth/le/ScanSettings$Builder;",
        &[Value::Int(8)],
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/bluetooth/le/ScanSettings;",
            &[],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let filter_builder = p49_new(&mut engine, P36D_SCAN_FILTER_BUILDER, "()V", &[]);
    let empty = object(call(
        &mut engine,
        filter_builder,
        "build",
        "()Landroid/bluetooth/le/ScanFilter;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, empty, "isAllFieldsEmpty", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, empty, "getManufacturerId", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(
            &mut engine,
            empty,
            "matches",
            "(Landroid/bluetooth/le/ScanResult;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );
    let name = text(&mut engine, "beacon");
    call(
        &mut engine,
        filter_builder,
        "setDeviceName",
        "(Ljava/lang/String;)Landroid/bluetooth/le/ScanFilter$Builder;",
        &[name],
    );
    let address = text(&mut engine, "00:11:22:33:AA:BB");
    call(
        &mut engine,
        filter_builder,
        "setDeviceAddress",
        "(Ljava/lang/String;)Landroid/bluetooth/le/ScanFilter$Builder;",
        &[address],
    );
    let manufacturer_data = byte_array(&mut engine, &[1, 2, 3]);
    call(
        &mut engine,
        filter_builder,
        "setManufacturerData",
        "(I[B)Landroid/bluetooth/le/ScanFilter$Builder;",
        &[Value::Int(76), manufacturer_data],
    );
    let first = object(call(
        &mut engine,
        filter_builder,
        "build",
        "()Landroid/bluetooth/le/ScanFilter;",
        &[],
    ));
    let second = object(call(
        &mut engine,
        filter_builder,
        "build",
        "()Landroid/bluetooth/le/ScanFilter;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            first,
            "getDeviceName",
            "()Ljava/lang/String;",
            &[]
        ),
        "beacon"
    );
    assert_eq!(
        call(
            &mut engine,
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(second)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, first, "hashCode", "()I", &[]),
        call(&mut engine, second, "hashCode", "()I", &[])
    );
    let bad_address = text(&mut engine, "invalid");
    assert_throws(
        engine.invoke_instance(
            filter_builder,
            "setDeviceAddress",
            "(Ljava/lang/String;)Landroid/bluetooth/le/ScanFilter$Builder;",
            &[bad_address],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36D_SCAN_SETTINGS, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36D_SCAN_FILTER, "Landroid/os/Parcelable;"));

    // Deep audit: setManufacturerData validates the *existing* stored
    // data/mask pair, not the pair being set — so a mismatched pair is
    // accepted on the first call and only rejected by the second call
    // (whatever it tries to store). Builder state after the throw keeps
    // the first pair.
    let trap_builder = p49_new(
        &mut engine,
        "Landroid/bluetooth/le/ScanFilter$Builder;",
        "()V",
        &[],
    );
    let short_data = byte_array(&mut engine, &[1, 2, 3]);
    let long_mask = byte_array(&mut engine, &[1, 2, 3, 4, 5]);
    call(
        &mut engine,
        trap_builder,
        "setManufacturerData",
        "(I[B[B)Landroid/bluetooth/le/ScanFilter$Builder;",
        &[Value::Int(7), short_data.clone(), long_mask],
    );
    let fine_data = byte_array(&mut engine, &[9]);
    let fine_mask = byte_array(&mut engine, &[9]);
    assert_throws(
        engine.invoke_instance(
            trap_builder,
            "setManufacturerData",
            "(I[B[B)Landroid/bluetooth/le/ScanFilter$Builder;",
            &[Value::Int(7), fine_data, fine_mask],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let trapped = object(call(
        &mut engine,
        trap_builder,
        "build",
        "()Landroid/bluetooth/le/ScanFilter;",
        &[],
    ));
    let stored = call(&mut engine, trapped, "getManufacturerData", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &stored), vec![1u8, 2, 3]);
}

#[test]
fn nfc_mock_tag_and_nfc_a_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let extras = p49_new(&mut engine, "Landroid/os/Bundle;", "()V", &[]);
    let sak_key = text(&mut engine, "sak");
    call(
        &mut engine,
        extras,
        "putShort",
        "(Ljava/lang/String;S)V",
        &[sak_key, Value::Int(8)],
    );
    let atqa_key = text(&mut engine, "atqa");
    let atqa = byte_array(&mut engine, &[4, 0]);
    call(
        &mut engine,
        extras,
        "putByteArray",
        "(Ljava/lang/String;[B)V",
        &[atqa_key, atqa],
    );
    let id = byte_array(&mut engine, &[1, 2, 3, 4]);
    let tech = int_array(&mut engine, &[1, 6]);
    let extras_array = object_array(
        &mut engine,
        "[Landroid/os/Bundle;",
        vec![Value::Obj(extras)],
    );
    let tag = object(call_static(
        &mut engine,
        P36D_TAG,
        "createMockTag",
        "([B[I[Landroid/os/Bundle;J)Landroid/nfc/Tag;",
        &[id, tech, extras_array, Value::Long(99)],
    ));
    let returned_id = call(&mut engine, tag, "getId", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &returned_id), [1, 2, 3, 4]);
    let tech_list = call(
        &mut engine,
        tag,
        "getTechList",
        "()[Ljava/lang/String;",
        &[],
    );
    let tech_names = array_values(&engine, &tech_list)
        .iter()
        .map(|value| text_of(&engine, value))
        .collect::<Vec<_>>();
    assert_eq!(
        tech_names,
        ["android.nfc.tech.NfcA", "android.nfc.tech.Ndef"]
    );
    assert_eq!(
        call(&mut engine, tag, "hasTech", "(I)Z", &[Value::Int(1)]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, tag, "hasTech", "(I)Z", &[Value::Int(2)]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(&mut engine, tag, "toString", "()Ljava/lang/String;", &[]),
        "TAG: Tech [android.nfc.tech.NfcA, android.nfc.tech.Ndef]"
    );
    assert_throws(
        engine.invoke_instance(tag, "rediscover", "()Landroid/nfc/Tag;", &[]),
        "Ljava/io/IOException;",
    );

    let technology = object(call_static(
        &mut engine,
        P36D_NFC_A,
        "get",
        "(Landroid/nfc/Tag;)Landroid/nfc/tech/NfcA;",
        &[Value::Obj(tag)],
    ));
    assert_eq!(
        call(&mut engine, technology, "getSak", "()S", &[]),
        Value::Int(8)
    );
    let returned_atqa = call(&mut engine, technology, "getAtqa", "()[B", &[]);
    assert_eq!(byte_array_values(&engine, &returned_atqa), [4, 0]);
    assert_eq!(
        call(
            &mut engine,
            technology,
            "getTag",
            "()Landroid/nfc/Tag;",
            &[]
        ),
        Value::Obj(tag)
    );
    assert_eq!(
        call(&mut engine, technology, "isConnected", "()Z", &[]),
        Value::Int(0)
    );
    let payload = byte_array(&mut engine, &[0x30, 0]);
    assert_throws(
        engine.invoke_instance(technology, "transceive", "([B)[B", &[payload]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(technology, "connect", "()V", &[]),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        tag,
        "setConnectedTechnology",
        "(I)Z",
        &[Value::Int(1)],
    );
    assert_throws(
        engine.invoke_instance(technology, "close", "()V", &[]),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call(&mut engine, tag, "getConnectedTechnology", "()I", &[]),
        Value::Int(-1)
    );
    assert!(engine.cp.is_subtype(P36D_TAG, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36D_NFC_A, "Landroid/nfc/tech/TagTechnology;"));
}

#[test]
fn nfc_card_emulation_without_hce_and_aid_groups_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let adapter = instance(&mut engine, "Landroid/nfc/NfcAdapter;");
    assert_throws(
        engine.invoke_static(
            P36D_CARD_EMULATION,
            "getInstance",
            "(Landroid/nfc/NfcAdapter;)Landroid/nfc/cardemulation/CardEmulation;",
            &[Value::Obj(adapter)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    engine
        .heap
        .put_field(adapter, "mContext", Value::Obj(context))
        .unwrap();
    assert_throws(
        engine.invoke_static(
            P36D_CARD_EMULATION,
            "getInstance",
            "(Landroid/nfc/NfcAdapter;)Landroid/nfc/cardemulation/CardEmulation;",
            &[Value::Obj(adapter)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    assert_throws(
        engine.invoke_static(
            P36D_CARD_EMULATION,
            "getInstance",
            "(Landroid/nfc/NfcAdapter;)Landroid/nfc/cardemulation/CardEmulation;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    let valid = text(&mut engine, "F014811481");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_CARD_EMULATION,
            "isValidAid",
            "(Ljava/lang/String;)Z",
            &[valid],
        ),
        Value::Int(1)
    );
    let prefix = text(&mut engine, "a000000004*");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_CARD_EMULATION,
            "isValidAid",
            "(Ljava/lang/String;)Z",
            &[prefix],
        ),
        Value::Int(1)
    );
    let invalid = text(&mut engine, "F0148");
    assert_eq!(
        call_static(
            &mut engine,
            P36D_CARD_EMULATION,
            "isValidAid",
            "(Ljava/lang/String;)Z",
            &[invalid],
        ),
        Value::Int(0)
    );

    let first = text(&mut engine, "f014811481");
    let second = text(&mut engine, "a000000004*");
    let aids = list_of(&mut engine, &[first, second]);
    let category = text(&mut engine, "custom");
    let group = p49_new(
        &mut engine,
        P36D_AID_GROUP,
        "(Ljava/util/List;Ljava/lang/String;)V",
        &[Value::Obj(aids), category],
    );
    assert_eq!(
        call_text(
            &mut engine,
            group,
            "getCategory",
            "()Ljava/lang/String;",
            &[]
        ),
        "other"
    );
    assert_eq!(
        call_text(&mut engine, group, "toString", "()Ljava/lang/String;", &[]),
        "Category: other, AIDs:F014811481, A000000004*, "
    );
    let empty = list_of(&mut engine, &[]);
    let rejected = instance(&mut engine, P36D_AID_GROUP);
    let payment = text(&mut engine, "payment");
    assert_throws(
        engine.invoke_instance(
            rejected,
            "<init>",
            "(Ljava/util/List;Ljava/lang/String;)V",
            &[Value::Obj(empty), payment],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let card_emulation = instance(&mut engine, P36D_CARD_EMULATION);
    let other = text(&mut engine, "other");
    assert_eq!(
        call(
            &mut engine,
            card_emulation,
            "categoryAllowsForegroundPreference",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&other),
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            card_emulation,
            "getSelectionModeForCategory",
            "(Ljava/lang/String;)I",
            &[other],
        ),
        Value::Int(2)
    );
    let registered_aid = text(&mut engine, "F014811481");
    let valid_aids = list_of(&mut engine, &[registered_aid]);
    let category = text(&mut engine, "other");
    assert_throws(
        engine.invoke_instance(
            card_emulation,
            "registerAidsForService",
            "(Landroid/content/ComponentName;Ljava/lang/String;Ljava/util/List;)Z",
            &[Value::Null, category, Value::Obj(valid_aids)],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

// ---- Appendix C Batch 36-E: media / session / browse / projection / audiofx ----

const P36E_MEDIA_METADATA: &str = "Landroid/media/MediaMetadata;";
const P36E_MEDIA_METADATA_BUILDER: &str = "Landroid/media/MediaMetadata$Builder;";
const P36E_MEDIA_DESCRIPTION: &str = "Landroid/media/MediaDescription;";
const P36E_MEDIA_DESCRIPTION_BUILDER: &str = "Landroid/media/MediaDescription$Builder;";
const P36E_RATING: &str = "Landroid/media/Rating;";
const P36E_PLAYBACK_STATE: &str = "Landroid/media/session/PlaybackState;";
const P36E_PLAYBACK_STATE_BUILDER: &str = "Landroid/media/session/PlaybackState$Builder;";
const P36E_CUSTOM_ACTION: &str = "Landroid/media/session/PlaybackState$CustomAction;";
const P36E_CUSTOM_ACTION_BUILDER: &str =
    "Landroid/media/session/PlaybackState$CustomAction$Builder;";
const P36E_QUEUE_ITEM: &str = "Landroid/media/session/MediaSession$QueueItem;";
const P36E_MEDIA_SESSION: &str = "Landroid/media/session/MediaSession;";
const P36E_MEDIA_BROWSER: &str = "Landroid/media/browse/MediaBrowser;";
const P36E_MEDIA_ITEM: &str = "Landroid/media/browse/MediaBrowser$MediaItem;";
const P36E_CONNECTION_CALLBACK: &str = "Landroid/media/browse/MediaBrowser$ConnectionCallback;";
const P36E_SUBSCRIPTION_CALLBACK: &str = "Landroid/media/browse/MediaBrowser$SubscriptionCallback;";
const P36E_ITEM_CALLBACK: &str = "Landroid/media/browse/MediaBrowser$ItemCallback;";
const P36E_PROJECTION_MANAGER: &str = "Landroid/media/projection/MediaProjectionManager;";
const P36E_PROJECTION: &str = "Landroid/media/projection/MediaProjection;";
const P36E_AUDIO_EFFECT: &str = "Landroid/media/audiofx/AudioEffect;";
const P36E_AUDIO_EFFECT_DESCRIPTOR: &str = "Landroid/media/audiofx/AudioEffect$Descriptor;";

#[test]
fn media_metadata_description_and_rating_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let heart = object(call_static(
        &mut engine,
        P36E_RATING,
        "newHeartRating",
        "(Z)Landroid/media/Rating;",
        &[Value::Int(1)],
    ));
    assert_eq!(
        call(&mut engine, heart, "isRated", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, heart, "getRatingStyle", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, heart, "hasHeart", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, heart, "getStarRating", "()F", &[]),
        Value::Float(-1.0)
    );
    assert_eq!(
        call_text(&mut engine, heart, "toString", "()Ljava/lang/String;", &[]),
        "Rating:style=1 rating=1.0"
    );
    let stars = object(call_static(
        &mut engine,
        P36E_RATING,
        "newStarRating",
        "(IF)Landroid/media/Rating;",
        &[Value::Int(5), Value::Float(4.5)],
    ));
    assert_eq!(
        call(&mut engine, stars, "getStarRating", "()F", &[]),
        Value::Float(4.5)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36E_RATING,
            "newStarRating",
            "(IF)Landroid/media/Rating;",
            &[Value::Int(5), Value::Float(5.5)],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36E_RATING,
            "newPercentageRating",
            "(F)Landroid/media/Rating;",
            &[Value::Float(101.0)],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36E_RATING,
            "newUnratedRating",
            "(I)Landroid/media/Rating;",
            &[Value::Int(0)],
        ),
        Value::Null
    );

    let description_builder = p49_new(&mut engine, P36E_MEDIA_DESCRIPTION_BUILDER, "()V", &[]);
    let media_id = text(&mut engine, "song-1");
    call(
        &mut engine,
        description_builder,
        "setMediaId",
        "(Ljava/lang/String;)Landroid/media/MediaDescription$Builder;",
        &[media_id],
    );
    let title = text(&mut engine, "Blue in Green");
    call(
        &mut engine,
        description_builder,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroid/media/MediaDescription$Builder;",
        &[title],
    );
    let subtitle = text(&mut engine, "Miles Davis");
    call(
        &mut engine,
        description_builder,
        "setSubtitle",
        "(Ljava/lang/CharSequence;)Landroid/media/MediaDescription$Builder;",
        &[subtitle],
    );
    let uri_text = text(&mut engine, "content://media/external/audio/media/1");
    let media_uri = call_static(
        &mut engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[uri_text],
    );
    call(
        &mut engine,
        description_builder,
        "setMediaUri",
        "(Landroid/net/Uri;)Landroid/media/MediaDescription$Builder;",
        &[media_uri],
    );
    let description = object(call(
        &mut engine,
        description_builder,
        "build",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            description,
            "getMediaId",
            "()Ljava/lang/String;",
            &[]
        ),
        "song-1"
    );
    assert_eq!(
        call_text(
            &mut engine,
            description,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "Blue in Green, Miles Davis, null"
    );
    let same_description = object(call(
        &mut engine,
        description_builder,
        "build",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            description,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_description)],
        ),
        Value::Int(1)
    );

    let builder = p49_new(&mut engine, P36E_MEDIA_METADATA_BUILDER, "()V", &[]);
    let title_key = text(&mut engine, "android.media.metadata.TITLE");
    let title_value = text(&mut engine, "Blue in Green");
    call(
        &mut engine,
        builder,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/media/MediaMetadata$Builder;",
        &[title_key, title_value],
    );
    let artist_key = text(&mut engine, "android.media.metadata.ARTIST");
    let artist_value = text(&mut engine, "Miles Davis");
    call(
        &mut engine,
        builder,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/media/MediaMetadata$Builder;",
        &[artist_key, artist_value],
    );
    let album_key = text(&mut engine, "android.media.metadata.ALBUM");
    let album_value = text(&mut engine, "Kind of Blue");
    call(
        &mut engine,
        builder,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/media/MediaMetadata$Builder;",
        &[album_key, album_value],
    );
    let id_key = text(&mut engine, "android.media.metadata.MEDIA_ID");
    let id_value = text(&mut engine, "song-1");
    call(
        &mut engine,
        builder,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/media/MediaMetadata$Builder;",
        &[id_key, id_value],
    );
    let duration_key = text(&mut engine, "android.media.metadata.DURATION");
    call(
        &mut engine,
        builder,
        "putLong",
        "(Ljava/lang/String;J)Landroid/media/MediaMetadata$Builder;",
        &[duration_key, Value::Long(327_000)],
    );
    let folder_key = text(&mut engine, "android.media.metadata.BT_FOLDER_TYPE");
    call(
        &mut engine,
        builder,
        "putLong",
        "(Ljava/lang/String;J)Landroid/media/MediaMetadata$Builder;",
        &[folder_key, Value::Long(2)],
    );
    let rating_key = text(&mut engine, "android.media.metadata.USER_RATING");
    call(
        &mut engine,
        builder,
        "putRating",
        "(Ljava/lang/String;Landroid/media/Rating;)Landroid/media/MediaMetadata$Builder;",
        &[rating_key, Value::Obj(stars)],
    );
    let metadata = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/media/MediaMetadata;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, metadata, "size", "()I", &[]),
        Value::Int(7)
    );
    let query_title = text(&mut engine, "android.media.metadata.TITLE");
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "containsKey",
            "(Ljava/lang/String;)Z",
            std::slice::from_ref(&query_title),
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_text(
            &mut engine,
            metadata,
            "getString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            std::slice::from_ref(&query_title),
        ),
        "Blue in Green"
    );
    let query_duration = text(&mut engine, "android.media.metadata.DURATION");
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "getLong",
            "(Ljava/lang/String;)J",
            std::slice::from_ref(&query_duration),
        ),
        Value::Long(327_000)
    );
    let query_rating = text(&mut engine, "android.media.metadata.USER_RATING");
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "getRating",
            "(Ljava/lang/String;)Landroid/media/Rating;",
            std::slice::from_ref(&query_rating),
        ),
        Value::Obj(stars)
    );
    let derived = object(call(
        &mut engine,
        metadata,
        "getDescription",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    let derived_again = call(
        &mut engine,
        metadata,
        "getDescription",
        "()Landroid/media/MediaDescription;",
        &[],
    );
    assert_eq!(derived_again, Value::Obj(derived));
    assert_eq!(
        call_text(
            &mut engine,
            derived,
            "getMediaId",
            "()Ljava/lang/String;",
            &[]
        ),
        "song-1"
    );
    assert_eq!(
        call_text(
            &mut engine,
            derived,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "Blue in Green, Miles Davis, Kind of Blue"
    );
    let extras = object(call(
        &mut engine,
        derived,
        "getExtras",
        "()Landroid/os/Bundle;",
        &[],
    ));
    let extra_key = text(&mut engine, "android.media.extra.BT_FOLDER_TYPE");
    assert_eq!(
        call(
            &mut engine,
            extras,
            "getLong",
            "(Ljava/lang/String;J)J",
            &[extra_key, Value::Long(-1)],
        ),
        Value::Long(2)
    );
    let same_metadata = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/media/MediaMetadata;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            metadata,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_metadata)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, metadata, "hashCode", "()I", &[]),
        call(&mut engine, same_metadata, "hashCode", "()I", &[])
    );
    let wrong_key = text(&mut engine, "android.media.metadata.DURATION");
    let wrong_value = text(&mut engine, "not-a-long");
    assert_throws(
        engine.invoke_instance(
            builder,
            "putText",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Landroid/media/MediaMetadata$Builder;",
            &[wrong_key, wrong_value],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36E_MEDIA_METADATA, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36E_MEDIA_DESCRIPTION, "Landroid/os/Parcelable;"));
    assert!(engine.cp.is_subtype(P36E_RATING, "Landroid/os/Parcelable;"));
}

#[test]
fn playback_state_queue_and_empty_session_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let action_name = text(&mut engine, "com.example.FAVORITE");
    let action_label = text(&mut engine, "Favorite");
    let custom_builder = p49_new(
        &mut engine,
        P36E_CUSTOM_ACTION_BUILDER,
        "(Ljava/lang/String;Ljava/lang/CharSequence;I)V",
        &[action_name, action_label, Value::Int(7)],
    );
    let custom = object(call(
        &mut engine,
        custom_builder,
        "build",
        "()Landroid/media/session/PlaybackState$CustomAction;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            custom,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.FAVORITE"
    );
    assert_eq!(
        call_text(&mut engine, custom, "toString", "()Ljava/lang/String;", &[]),
        "Action:mName='Favorite, mIcon=7, mExtras=null"
    );
    let rejected = instance(&mut engine, P36E_CUSTOM_ACTION_BUILDER);
    let empty = text(&mut engine, "");
    let label = text(&mut engine, "Nope");
    assert_throws(
        engine.invoke_instance(
            rejected,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/CharSequence;I)V",
            &[empty, label, Value::Int(1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let builder = p49_new(&mut engine, P36E_PLAYBACK_STATE_BUILDER, "()V", &[]);
    let initial = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/media/session/PlaybackState;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, initial, "getActiveQueueItemId", "()J", &[]),
        Value::Long(-1)
    );
    assert_eq!(
        call(&mut engine, initial, "isActive", "()Z", &[]),
        Value::Int(0)
    );
    call(
        &mut engine,
        builder,
        "setState",
        "(IJFJ)Landroid/media/session/PlaybackState$Builder;",
        &[
            Value::Int(3),
            Value::Long(1234),
            Value::Float(1.0),
            Value::Long(9999),
        ],
    );
    call(
        &mut engine,
        builder,
        "setActions",
        "(J)Landroid/media/session/PlaybackState$Builder;",
        &[Value::Long(6)],
    );
    call(
        &mut engine,
        builder,
        "setBufferedPosition",
        "(J)Landroid/media/session/PlaybackState$Builder;",
        &[Value::Long(5000)],
    );
    call(
        &mut engine,
        builder,
        "setActiveQueueItemId",
        "(J)Landroid/media/session/PlaybackState$Builder;",
        &[Value::Long(42)],
    );
    call(
        &mut engine,
        builder,
        "addCustomAction",
        "(Landroid/media/session/PlaybackState$CustomAction;)Landroid/media/session/PlaybackState$Builder;",
        &[Value::Obj(custom)],
    );
    let second_action = text(&mut engine, "com.example.SKIP30");
    let second_label = text(&mut engine, "Skip 30");
    call(
        &mut engine,
        builder,
        "addCustomAction",
        "(Ljava/lang/String;Ljava/lang/String;I)Landroid/media/session/PlaybackState$Builder;",
        &[second_action, second_label, Value::Int(9)],
    );
    let state = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/media/session/PlaybackState;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, state, "getState", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call(&mut engine, state, "getPosition", "()J", &[]),
        Value::Long(1234)
    );
    assert_eq!(
        call(&mut engine, state, "getPlaybackSpeed", "()F", &[]),
        Value::Float(1.0)
    );
    assert_eq!(
        call(&mut engine, state, "isActive", "()Z", &[]),
        Value::Int(1)
    );
    let actions = object(call(
        &mut engine,
        state,
        "getCustomActions",
        "()Ljava/util/List;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, actions, "size", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(
            &mut engine,
            actions,
            "get",
            "(I)Ljava/lang/Object;",
            &[Value::Int(0)],
        ),
        Value::Obj(custom)
    );
    let rendered = call_text(&mut engine, state, "toString", "()Ljava/lang/String;", &[]);
    assert!(rendered.contains("state=PLAYING(3)"), "{rendered}");
    assert!(rendered.contains("active item id=42"), "{rendered}");

    let description_builder = p49_new(&mut engine, P36E_MEDIA_DESCRIPTION_BUILDER, "()V", &[]);
    let queue_title = text(&mut engine, "Queued");
    call(
        &mut engine,
        description_builder,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroid/media/MediaDescription$Builder;",
        &[queue_title],
    );
    let queue_description = object(call(
        &mut engine,
        description_builder,
        "build",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    let queue_item = p49_new(
        &mut engine,
        P36E_QUEUE_ITEM,
        "(Landroid/media/MediaDescription;J)V",
        &[Value::Obj(queue_description), Value::Long(42)],
    );
    assert_eq!(
        call(&mut engine, queue_item, "getQueueId", "()J", &[]),
        Value::Long(42)
    );
    assert_eq!(
        call_text(
            &mut engine,
            queue_item,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "MediaSession.QueueItem {Description=Queued, null, null, Id=42 }"
    );
    let rejected_item = instance(&mut engine, P36E_QUEUE_ITEM);
    assert_throws(
        engine.invoke_instance(
            rejected_item,
            "<init>",
            "(Landroid/media/MediaDescription;J)V",
            &[Value::Obj(queue_description), Value::Long(-1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let session = instance(&mut engine, P36E_MEDIA_SESSION);
    let tag = text(&mut engine, "test-session");
    assert_throws(
        engine.invoke_instance(
            session,
            "<init>",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[Value::Obj(context), tag],
        ),
        "Ljava/lang/RuntimeException;",
    );
    let service_name = text(&mut engine, "media_session");
    let manager = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    let sessions = object(call(
        &mut engine,
        manager,
        "getActiveSessions",
        "(Landroid/content/ComponentName;)Ljava/util/List;",
        &[Value::Null],
    ));
    assert_eq!(
        call(&mut engine, sessions, "size", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getMediaKeyEventSession",
            "()Landroid/media/session/MediaSession$Token;",
            &[],
        ),
        Value::Null
    );
    assert_eq!(
        call_text(
            &mut engine,
            manager,
            "getMediaKeyEventSessionPackageName",
            "()Ljava/lang/String;",
            &[],
        ),
        ""
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "removeOnActiveSessionsChangedListener",
            "(Landroid/media/session/MediaSessionManager$OnActiveSessionsChangedListener;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36E_PLAYBACK_STATE, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36E_CUSTOM_ACTION, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36E_QUEUE_ITEM, "Landroid/os/Parcelable;"));
}

#[test]
fn media_browser_disconnected_state_and_media_items_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let component = instance(&mut engine, "Landroid/content/ComponentName;");
    let connection = p49_new(&mut engine, P36E_CONNECTION_CALLBACK, "()V", &[]);
    let browser = p49_new(
        &mut engine,
        P36E_MEDIA_BROWSER,
        "(Landroid/content/Context;Landroid/content/ComponentName;Landroid/media/browse/MediaBrowser$ConnectionCallback;Landroid/os/Bundle;)V",
        &[
            Value::Obj(context),
            Value::Obj(component),
            Value::Obj(connection),
            Value::Null,
        ],
    );
    assert_eq!(
        call(&mut engine, browser, "isConnected", "()Z", &[]),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_instance(browser, "getRoot", "()Ljava/lang/String;", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(
            browser,
            "getSessionToken",
            "()Landroid/media/session/MediaSession$Token;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    let subscription = p49_new(&mut engine, P36E_SUBSCRIPTION_CALLBACK, "()V", &[]);
    let root = text(&mut engine, "root");
    call(
        &mut engine,
        browser,
        "subscribe",
        "(Ljava/lang/String;Landroid/media/browse/MediaBrowser$SubscriptionCallback;)V",
        &[root.clone(), Value::Obj(subscription)],
    );
    let empty = text(&mut engine, "");
    assert_throws(
        engine.invoke_instance(
            browser,
            "subscribe",
            "(Ljava/lang/String;Landroid/media/browse/MediaBrowser$SubscriptionCallback;)V",
            &[empty, Value::Obj(subscription)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    call(
        &mut engine,
        browser,
        "unsubscribe",
        "(Ljava/lang/String;Landroid/media/browse/MediaBrowser$SubscriptionCallback;)V",
        &[root.clone(), Value::Obj(subscription)],
    );
    let item_callback = p49_new(&mut engine, P36E_ITEM_CALLBACK, "()V", &[]);
    let item_id = text(&mut engine, "song-1");
    call(
        &mut engine,
        browser,
        "getItem",
        "(Ljava/lang/String;Landroid/media/browse/MediaBrowser$ItemCallback;)V",
        &[item_id, Value::Obj(item_callback)],
    );
    call(&mut engine, browser, "connect", "()V", &[]);
    assert_throws(
        engine.invoke_instance(browser, "connect", "()V", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    call(&mut engine, browser, "disconnect", "()V", &[]);
    call(&mut engine, browser, "connect", "()V", &[]);
    call(&mut engine, browser, "disconnect", "()V", &[]);

    let description_builder = p49_new(&mut engine, P36E_MEDIA_DESCRIPTION_BUILDER, "()V", &[]);
    let media_id = text(&mut engine, "song-1");
    call(
        &mut engine,
        description_builder,
        "setMediaId",
        "(Ljava/lang/String;)Landroid/media/MediaDescription$Builder;",
        &[media_id],
    );
    let title = text(&mut engine, "Song");
    call(
        &mut engine,
        description_builder,
        "setTitle",
        "(Ljava/lang/CharSequence;)Landroid/media/MediaDescription$Builder;",
        &[title],
    );
    let description = object(call(
        &mut engine,
        description_builder,
        "build",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    let item = p49_new(
        &mut engine,
        P36E_MEDIA_ITEM,
        "(Landroid/media/MediaDescription;I)V",
        &[Value::Obj(description), Value::Int(3)],
    );
    assert_eq!(
        call(&mut engine, item, "isBrowsable", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, item, "isPlayable", "()Z", &[]),
        Value::Int(1)
    );
    // Deep audit: FLAG_BROWSABLE is 1<<0 and FLAG_PLAYABLE is 1<<1 —
    // pin each bit independently (a flags=3 fixture cannot detect a swap).
    let browsable_only = p49_new(
        &mut engine,
        P36E_MEDIA_ITEM,
        "(Landroid/media/MediaDescription;I)V",
        &[Value::Obj(description), Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, browsable_only, "isBrowsable", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, browsable_only, "isPlayable", "()Z", &[]),
        Value::Int(0)
    );
    let playable_only = p49_new(
        &mut engine,
        P36E_MEDIA_ITEM,
        "(Landroid/media/MediaDescription;I)V",
        &[Value::Obj(description), Value::Int(2)],
    );
    assert_eq!(
        call(&mut engine, playable_only, "isBrowsable", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, playable_only, "isPlayable", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(&mut engine, item, "getMediaId", "()Ljava/lang/String;", &[]),
        "song-1"
    );
    assert_eq!(
        call_text(&mut engine, item, "toString", "()Ljava/lang/String;", &[]),
        "MediaItem{mFlags=3, mDescription=Song, null, null}"
    );
    let no_id_builder = p49_new(&mut engine, P36E_MEDIA_DESCRIPTION_BUILDER, "()V", &[]);
    let no_id = object(call(
        &mut engine,
        no_id_builder,
        "build",
        "()Landroid/media/MediaDescription;",
        &[],
    ));
    let rejected = instance(&mut engine, P36E_MEDIA_ITEM);
    assert_throws(
        engine.invoke_instance(
            rejected,
            "<init>",
            "(Landroid/media/MediaDescription;I)V",
            &[Value::Obj(no_id), Value::Int(2)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert!(engine
        .cp
        .is_subtype(P36E_MEDIA_ITEM, "Landroid/os/Parcelable;"));
}

#[test]
fn media_projection_without_a_token_or_service_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let service_name = text(&mut engine, "media_projection");
    let manager = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    let intent = p49_new(&mut engine, "Landroid/content/Intent;", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getMediaProjection",
            "(ILandroid/content/Intent;)Landroid/media/projection/MediaProjection;",
            &[Value::Int(0), Value::Obj(intent)],
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getMediaProjection",
            "(ILandroid/content/Intent;)Landroid/media/projection/MediaProjection;",
            &[Value::Int(-1), Value::Obj(intent)],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "getActiveProjectionInfo",
            "()Landroid/media/projection/MediaProjectionInfo;",
            &[],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let callback = instance(
        &mut engine,
        "Landroid/media/projection/MediaProjectionManager$Callback;",
    );
    call(
        &mut engine,
        manager,
        "removeCallback",
        "(Landroid/media/projection/MediaProjectionManager$Callback;)V",
        &[Value::Obj(callback)],
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "removeCallback",
            "(Landroid/media/projection/MediaProjectionManager$Callback;)V",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let handler = instance(&mut engine, "Landroid/os/Handler;");
    assert_throws(
        engine.invoke_instance(
            manager,
            "addCallback",
            "(Landroid/media/projection/MediaProjectionManager$Callback;Landroid/os/Handler;)V",
            &[Value::Obj(callback), Value::Obj(handler)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    // Deep audit: addCallback stores the delegate before the failing
    // service call, so removing that callback reaches the service (and the
    // same NPE) — but the local removal happens first, making a second
    // removal an unknown-callback no-op.
    assert_throws(
        engine.invoke_instance(
            manager,
            "removeCallback",
            "(Landroid/media/projection/MediaProjectionManager$Callback;)V",
            &[Value::Obj(callback)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    call(
        &mut engine,
        manager,
        "removeCallback",
        "(Landroid/media/projection/MediaProjectionManager$Callback;)V",
        &[Value::Obj(callback)],
    );
    let built_manager = p49_new(
        &mut engine,
        P36E_PROJECTION_MANAGER,
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    );
    assert_eq!(
        field(&engine, built_manager, "mContext"),
        Value::Obj(context)
    );
    assert_eq!(field(&engine, built_manager, "mService"), Value::Null);

    let projection_impl = instance(&mut engine, "Landroid/media/projection/IMediaProjection;");
    let projection = p49_new(
        &mut engine,
        P36E_PROJECTION,
        "(Landroid/content/Context;Landroid/media/projection/IMediaProjection;)V",
        &[Value::Obj(context), Value::Obj(projection_impl)],
    );
    assert_eq!(
        call(
            &mut engine,
            projection,
            "getProjection",
            "()Landroid/media/projection/IMediaProjection;",
            &[],
        ),
        Value::Obj(projection_impl)
    );
    let projection_callback = instance(
        &mut engine,
        "Landroid/media/projection/MediaProjection$Callback;",
    );
    call(
        &mut engine,
        projection,
        "registerCallback",
        "(Landroid/media/projection/MediaProjection$Callback;Landroid/os/Handler;)V",
        &[Value::Obj(projection_callback), Value::Obj(handler)],
    );
    call(
        &mut engine,
        projection,
        "unregisterCallback",
        "(Landroid/media/projection/MediaProjection$Callback;)V",
        &[Value::Obj(projection_callback)],
    );
}

#[test]
fn audiofx_descriptors_and_no_effect_engine_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let effects = call_static(
        &mut engine,
        P36E_AUDIO_EFFECT,
        "queryEffects",
        "()[Landroid/media/audiofx/AudioEffect$Descriptor;",
        &[],
    );
    assert_eq!(array_values(&engine, &effects).len(), 0);
    let equalizer_text = text(&mut engine, "0bed4300-ddd6-11db-8f34-0002a5d5c51b");
    let equalizer = call_static(
        &mut engine,
        "Ljava/util/UUID;",
        "fromString",
        "(Ljava/lang/String;)Ljava/util/UUID;",
        &[equalizer_text],
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36E_AUDIO_EFFECT,
            "isEffectTypeAvailable",
            "(Ljava/util/UUID;)Z",
            std::slice::from_ref(&equalizer),
        ),
        Value::Int(0)
    );
    let type_text = text(&mut engine, "0bed4300-ddd6-11db-8f34-0002a5d5c51b");
    let uuid_text = text(&mut engine, "123e4567-e89b-12d3-a456-426614174000");
    let mode = text(&mut engine, "Insert");
    let name = text(&mut engine, "Equalizer");
    let implementor = text(&mut engine, "AOSP");
    let descriptor = p49_new(
        &mut engine,
        P36E_AUDIO_EFFECT_DESCRIPTOR,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[type_text, uuid_text, mode, name, implementor],
    );
    let type_value = field(&engine, descriptor, "type");
    let type_object = object(type_value);
    assert_eq!(
        call_text(
            &mut engine,
            type_object,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "0bed4300-ddd6-11db-8f34-0002a5d5c51b"
    );
    let type_text2 = text(&mut engine, "0bed4300-ddd6-11db-8f34-0002a5d5c51b");
    let uuid_text2 = text(&mut engine, "123e4567-e89b-12d3-a456-426614174000");
    let mode2 = text(&mut engine, "Insert");
    let name2 = text(&mut engine, "Equalizer");
    let implementor2 = text(&mut engine, "AOSP");
    let same_descriptor = p49_new(
        &mut engine,
        P36E_AUDIO_EFFECT_DESCRIPTOR,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[type_text2, uuid_text2, mode2, name2, implementor2],
    );
    assert_eq!(
        call(
            &mut engine,
            descriptor,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same_descriptor)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, descriptor, "hashCode", "()I", &[]),
        call(&mut engine, same_descriptor, "hashCode", "()I", &[])
    );
    let effect = instance(&mut engine, P36E_AUDIO_EFFECT);
    let uuid = equalizer.clone();
    assert_throws(
        engine.invoke_instance(
            effect,
            "<init>",
            "(Ljava/util/UUID;Ljava/util/UUID;II)V",
            &[uuid.clone(), uuid, Value::Int(0), Value::Int(0)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
    assert_throws(
        engine.invoke_instance(effect, "getId", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    assert_throws(
        engine.invoke_instance(effect, "setEnabled", "(Z)I", &[Value::Int(1)]),
        "Ljava/lang/IllegalStateException;",
    );
    call(&mut engine, effect, "release", "()V", &[]);
    assert_eq!(field(&engine, effect, "mState"), Value::Int(0));

    // Deep audit: the string Descriptor constructor parses through
    // UUID.fromString, so a malformed UUID is an IllegalArgumentException;
    // equals dereferences the fields directly, so two default Descriptors
    // (all-null fields) throw NPE; and a null type probe is simply false
    // when no effects are installed.
    let descriptor_class = "Landroid/media/audiofx/AudioEffect$Descriptor;";
    let descriptor_sig =
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V";
    let bad_uuid = text(&mut engine, "not-a-uuid");
    let mode = text(&mut engine, "Insert");
    let name = text(&mut engine, "Fx");
    let implementor = text(&mut engine, "Rusty");
    let good_uuid = text(&mut engine, "123e4567-e89b-12d3-a456-426614174000");
    let malformed = instance(&mut engine, descriptor_class);
    assert_throws(
        engine.invoke_instance(
            malformed,
            "<init>",
            descriptor_sig,
            &[bad_uuid, good_uuid, mode, name, implementor],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let empty_one = p49_new(&mut engine, descriptor_class, "()V", &[]);
    let empty_two = p49_new(&mut engine, descriptor_class, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            empty_one,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(empty_two)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call_static(
            &mut engine,
            "Landroid/media/audiofx/AudioEffect;",
            "isEffectTypeAvailable",
            "(Ljava/util/UUID;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );
}

// ---- Appendix C Batch 36-F: graphics.pdf / renderscript ----

const P36F_PDF_DOCUMENT: &str = "Landroid/graphics/pdf/PdfDocument;";
const P36F_PDF_PAGE_INFO_BUILDER: &str = "Landroid/graphics/pdf/PdfDocument$PageInfo$Builder;";
const P36F_PDF_RENDERER: &str = "Landroid/graphics/pdf/PdfRenderer;";
const P36F_PDF_RENDERER_PAGE: &str = "Landroid/graphics/pdf/PdfRenderer$Page;";
const P36F_RS: &str = "Landroid/renderscript/RenderScript;";
const P36F_RS_ELEMENT: &str = "Landroid/renderscript/Element;";
const P36F_RS_FLOAT3: &str = "Landroid/renderscript/Float3;";
const P36F_RS_FLOAT4: &str = "Landroid/renderscript/Float4;";
const P36F_RS_MATRIX2: &str = "Landroid/renderscript/Matrix2f;";
const P36F_RS_MATRIX3: &str = "Landroid/renderscript/Matrix3f;";
const P36F_RS_MATRIX4: &str = "Landroid/renderscript/Matrix4f;";
const P36F_IAE: &str = "Ljava/lang/IllegalArgumentException;";
const P36F_ISE: &str = "Ljava/lang/IllegalStateException;";
const P36F_NPE: &str = "Ljava/lang/NullPointerException;";

fn p36f_pdf_bytes(engine: &mut Engine, document: ObjRef) -> Vec<u8> {
    let sink = p49_new(engine, "Ljava/io/ByteArrayOutputStream;", "()V", &[]);
    call(
        engine,
        document,
        "writeTo",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(sink)],
    );
    let bytes = call(engine, sink, "toByteArray", "()[B", &[]);
    array_values(engine, &bytes)
        .iter()
        .map(|v| v.as_int().unwrap_or(0) as u8)
        .collect()
}

#[test]
fn pdf_document_lifecycle_and_page_info_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    for (w, h, number) in [(0, 100, 0), (100, 0, 0), (100, 100, -1)] {
        let builder = instance(&mut engine, P36F_PDF_PAGE_INFO_BUILDER);
        assert_throws(
            engine.invoke_instance(
                builder,
                "<init>",
                "(III)V",
                &[Value::Int(w), Value::Int(h), Value::Int(number)],
            ),
            P36F_IAE,
        );
    }
    let builder = p49_new(
        &mut engine,
        P36F_PDF_PAGE_INFO_BUILDER,
        "(III)V",
        &[Value::Int(612), Value::Int(792), Value::Int(0)],
    );
    let too_big = p49_new(
        &mut engine,
        "Landroid/graphics/Rect;",
        "(IIII)V",
        &[
            Value::Int(0),
            Value::Int(0),
            Value::Int(613),
            Value::Int(792),
        ],
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "setContentRect",
            "(Landroid/graphics/Rect;)Landroid/graphics/pdf/PdfDocument$PageInfo$Builder;",
            &[Value::Obj(too_big)],
        ),
        P36F_IAE,
    );
    let content = p49_new(
        &mut engine,
        "Landroid/graphics/Rect;",
        "(IIII)V",
        &[
            Value::Int(36),
            Value::Int(72),
            Value::Int(576),
            Value::Int(756),
        ],
    );
    let returned = call(
        &mut engine,
        builder,
        "setContentRect",
        "(Landroid/graphics/Rect;)Landroid/graphics/pdf/PdfDocument$PageInfo$Builder;",
        &[Value::Obj(content)],
    );
    assert_eq!(returned, Value::Obj(builder));
    let info = object(call(
        &mut engine,
        builder,
        "create",
        "()Landroid/graphics/pdf/PdfDocument$PageInfo;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, info, "getPageWidth", "()I", &[]),
        Value::Int(612)
    );
    assert_eq!(
        call(&mut engine, info, "getPageHeight", "()I", &[]),
        Value::Int(792)
    );
    assert_eq!(
        call(&mut engine, info, "getPageNumber", "()I", &[]),
        Value::Int(0)
    );
    let rect = object(call(
        &mut engine,
        info,
        "getContentRect",
        "()Landroid/graphics/Rect;",
        &[],
    ));
    assert_eq!(field(&engine, rect, "left"), Value::Int(36));
    assert_eq!(field(&engine, rect, "bottom"), Value::Int(756));

    let builder2 = p49_new(
        &mut engine,
        P36F_PDF_PAGE_INFO_BUILDER,
        "(III)V",
        &[Value::Int(100), Value::Int(100), Value::Int(1)],
    );
    let info2 = object(call(
        &mut engine,
        builder2,
        "create",
        "()Landroid/graphics/pdf/PdfDocument$PageInfo;",
        &[],
    ));
    let default_rect = object(call(
        &mut engine,
        info2,
        "getContentRect",
        "()Landroid/graphics/Rect;",
        &[],
    ));
    assert_eq!(field(&engine, default_rect, "right"), Value::Int(100));

    let document = p49_new(&mut engine, P36F_PDF_DOCUMENT, "()V", &[]);
    let pages = object(call(
        &mut engine,
        document,
        "getPages",
        "()Ljava/util/List;",
        &[],
    ));
    assert_eq!(call(&mut engine, pages, "size", "()I", &[]), Value::Int(0));
    assert_throws(
        engine.invoke_instance(
            document,
            "startPage",
            "(Landroid/graphics/pdf/PdfDocument$PageInfo;)Landroid/graphics/pdf/PdfDocument$Page;",
            &[Value::Null],
        ),
        P36F_IAE,
    );
    let page = object(call(
        &mut engine,
        document,
        "startPage",
        "(Landroid/graphics/pdf/PdfDocument$PageInfo;)Landroid/graphics/pdf/PdfDocument$Page;",
        &[Value::Obj(info)],
    ));
    assert_eq!(
        call(
            &mut engine,
            page,
            "getInfo",
            "()Landroid/graphics/pdf/PdfDocument$PageInfo;",
            &[]
        ),
        Value::Obj(info)
    );
    let canvas = object(call(
        &mut engine,
        page,
        "getCanvas",
        "()Landroid/graphics/Canvas;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, canvas, "getWidth", "()I", &[]),
        Value::Int(612)
    );
    assert_eq!(
        call(&mut engine, canvas, "getHeight", "()I", &[]),
        Value::Int(792)
    );
    assert_throws(
        engine.invoke_instance(
            document,
            "startPage",
            "(Landroid/graphics/pdf/PdfDocument$PageInfo;)Landroid/graphics/pdf/PdfDocument$Page;",
            &[Value::Obj(info2)],
        ),
        P36F_ISE,
    );
    assert_throws(
        engine.invoke_instance(document, "close", "()V", &[]),
        P36F_ISE,
    );
    let other_page = instance(&mut engine, "Landroid/graphics/pdf/PdfDocument$Page;");
    assert_throws(
        engine.invoke_instance(
            document,
            "finishPage",
            "(Landroid/graphics/pdf/PdfDocument$Page;)V",
            &[Value::Obj(other_page)],
        ),
        P36F_ISE,
    );
    call(
        &mut engine,
        document,
        "finishPage",
        "(Landroid/graphics/pdf/PdfDocument$Page;)V",
        &[Value::Obj(page)],
    );
    assert_eq!(
        call(
            &mut engine,
            page,
            "getCanvas",
            "()Landroid/graphics/Canvas;",
            &[]
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            document,
            "writeTo",
            "(Ljava/io/OutputStream;)V",
            &[Value::Null],
        ),
        P36F_IAE,
    );
    let bytes = p36f_pdf_bytes(&mut engine, document);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.starts_with("%PDF-1.4"), "{text}");
    assert!(text.contains("/Count 1"), "{text}");
    assert!(text.contains("/MediaBox [0 0 612 792]"), "{text}");
    assert!(text.contains("/CropBox [36 36 576 720]"), "{text}");
    assert!(text.contains("%%EOF"), "{text}");
    let pages = call(&mut engine, document, "getPages", "()Ljava/util/List;", &[]);
    assert_eq!(
        call(&mut engine, object(pages.clone()), "size", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(p49_list_get(&mut engine, pages, 0), Value::Obj(info));

    let page2 = object(call(
        &mut engine,
        document,
        "startPage",
        "(Landroid/graphics/pdf/PdfDocument$PageInfo;)Landroid/graphics/pdf/PdfDocument$Page;",
        &[Value::Obj(info2)],
    ));
    call(
        &mut engine,
        document,
        "finishPage",
        "(Landroid/graphics/pdf/PdfDocument$Page;)V",
        &[Value::Obj(page2)],
    );
    let bytes = p36f_pdf_bytes(&mut engine, document);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Count 2"), "{text}");
    assert!(text.contains("/MediaBox [0 0 100 100]"), "{text}");

    call(&mut engine, document, "close", "()V", &[]);
    call(&mut engine, document, "close", "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            document,
            "startPage",
            "(Landroid/graphics/pdf/PdfDocument$PageInfo;)Landroid/graphics/pdf/PdfDocument$Page;",
            &[Value::Obj(info)],
        ),
        P36F_ISE,
    );
    assert_throws(
        engine.invoke_instance(
            document,
            "writeTo",
            "(Ljava/io/OutputStream;)V",
            &[Value::Null],
        ),
        P36F_ISE,
    );
    let pages = object(call(
        &mut engine,
        document,
        "getPages",
        "()Ljava/util/List;",
        &[],
    ));
    assert_eq!(call(&mut engine, pages, "size", "()I", &[]), Value::Int(2));

    // Deep audit: getPages exposes Collections.unmodifiableList(mPages),
    // so mutating the returned list is rejected.
    let first_page_info = object(call(
        &mut engine,
        pages,
        "get",
        "(I)Ljava/lang/Object;",
        &[Value::Int(0)],
    ));
    assert_throws(
        engine.invoke_instance(
            pages,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(first_page_info)],
        ),
        "Ljava/lang/UnsupportedOperationException;",
    );
}

#[test]
fn pdf_renderer_without_pdfium_fails_before_any_document_exists() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine
        .cp
        .is_subtype(P36F_PDF_RENDERER, "Ljava/lang/AutoCloseable;"));
    assert!(engine
        .cp
        .is_subtype(P36F_PDF_RENDERER_PAGE, "Ljava/lang/AutoCloseable;"));
    let renderer = instance(&mut engine, P36F_PDF_RENDERER);
    assert_throws(
        engine.invoke_instance(
            renderer,
            "<init>",
            "(Landroid/os/ParcelFileDescriptor;)V",
            &[Value::Null],
        ),
        P36F_NPE,
    );
    let descriptor = instance(&mut engine, "Landroid/os/ParcelFileDescriptor;");
    match engine.invoke_instance(
        renderer,
        "<init>",
        "(Landroid/os/ParcelFileDescriptor;)V",
        &[Value::Obj(descriptor)],
    ) {
        Err(VmExit::Exception(t)) => {
            assert!(
                t.class == P36F_IAE || t.class == "Ljava/io/IOException;",
                "{}",
                t.class
            )
        }
        other => panic!("expected construction failure, got {other:?}"),
    }
    for (name, sig, args) in [
        ("getPageCount", "()I", Vec::new()),
        ("shouldScaleForPrinting", "()Z", Vec::new()),
        (
            "openPage",
            "(I)Landroid/graphics/pdf/PdfRenderer$Page;",
            vec![Value::Int(0)],
        ),
        ("close", "()V", Vec::new()),
    ] {
        assert_throws(engine.invoke_instance(renderer, name, sig, &args), P36F_ISE);
    }
    let page = instance(&mut engine, P36F_PDF_RENDERER_PAGE);
    assert_eq!(
        call(&mut engine, page, "getIndex", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, page, "getWidth", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, page, "getHeight", "()I", &[]),
        Value::Int(0)
    );
    assert_throws(engine.invoke_instance(page, "close", "()V", &[]), P36F_ISE);
    assert_throws(
        engine.invoke_instance(
            page,
            "render",
            "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Matrix;I)V",
            &[Value::Null, Value::Null, Value::Null, Value::Int(1)],
        ),
        P36F_NPE,
    );
}

fn p36f_floats(engine: &mut Engine, values: &[f32]) -> Value {
    let array = float_array(engine, values.len());
    *engine.heap.array_elems_mut(object(array.clone())).unwrap() =
        values.iter().copied().map(Value::Float).collect();
    array
}

fn p36f_float_values(engine: &Engine, value: &Value) -> Vec<f32> {
    array_values(engine, value)
        .iter()
        .map(|v| match v {
            Value::Float(f) => *f,
            other => panic!("expected float element, got {other:?}"),
        })
        .collect()
}

#[test]
fn renderscript_vectors_and_matrices_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let a = p49_new(
        &mut engine,
        P36F_RS_FLOAT4,
        "(FFFF)V",
        &[
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
        ],
    );
    assert_eq!(field(&engine, a, "x"), Value::Float(1.0));
    assert_eq!(field(&engine, a, "w"), Value::Float(4.0));
    let b = p49_new(
        &mut engine,
        P36F_RS_FLOAT4,
        "(FFFF)V",
        &[
            Value::Float(2.0),
            Value::Float(0.0),
            Value::Float(1.0),
            Value::Float(0.0),
        ],
    );
    let sum = object(call_static(
        &mut engine,
        P36F_RS_FLOAT4,
        "add",
        "(Landroid/renderscript/Float4;Landroid/renderscript/Float4;)Landroid/renderscript/Float4;",
        &[Value::Obj(a), Value::Obj(b)],
    ));
    assert_eq!(field(&engine, sum, "x"), Value::Float(3.0));
    assert_eq!(field(&engine, sum, "z"), Value::Float(4.0));
    let scaled = object(call_static(
        &mut engine,
        P36F_RS_FLOAT4,
        "mul",
        "(Landroid/renderscript/Float4;F)Landroid/renderscript/Float4;",
        &[Value::Obj(a), Value::Float(2.0)],
    ));
    assert_eq!(field(&engine, scaled, "w"), Value::Float(8.0));
    assert_eq!(
        call(
            &mut engine,
            a,
            "dotProduct",
            "(Landroid/renderscript/Float4;)F",
            &[Value::Obj(b)]
        ),
        Value::Float(5.0)
    );
    assert_eq!(
        call(&mut engine, a, "elementSum", "()F", &[]),
        Value::Float(10.0)
    );
    assert_eq!(call(&mut engine, a, "length", "()I", &[]), Value::Int(4));
    assert_eq!(
        call(&mut engine, a, "get", "(I)F", &[Value::Int(2)]),
        Value::Float(3.0)
    );
    assert_throws(
        engine.invoke_instance(a, "get", "(I)F", &[Value::Int(4)]),
        "Ljava/lang/IndexOutOfBoundsException;",
    );
    call(
        &mut engine,
        a,
        "setAt",
        "(IF)V",
        &[Value::Int(0), Value::Float(9.0)],
    );
    call(&mut engine, a, "negate", "()V", &[]);
    assert_eq!(field(&engine, a, "x"), Value::Float(-9.0));
    let target = float_array(&mut engine, 6);
    call(
        &mut engine,
        a,
        "copyTo",
        "([FI)V",
        &[target.clone(), Value::Int(1)],
    );
    assert_eq!(
        p36f_float_values(&engine, &target),
        vec![0.0, -9.0, -2.0, -3.0, -4.0, 0.0]
    );

    let v3 = p49_new(
        &mut engine,
        P36F_RS_FLOAT3,
        "(FFF)V",
        &[Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
    );
    let dot = object(call(
        &mut engine,
        v3,
        "dotProduct",
        "(Landroid/renderscript/Float3;)Ljava/lang/Float;",
        &[Value::Obj(v3)],
    ));
    assert_eq!(
        call(&mut engine, dot, "floatValue", "()F", &[]),
        Value::Float(14.0)
    );
    let sum3 = object(call(
        &mut engine,
        v3,
        "elementSum",
        "()Ljava/lang/Float;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, sum3, "floatValue", "()F", &[]),
        Value::Float(6.0)
    );

    let matrix = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            matrix,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(0)]
        ),
        Value::Float(1.0)
    );
    let array = call(&mut engine, matrix, "getArray", "()[F", &[]);
    assert_eq!(array, field(&engine, matrix, "mMat"));
    call(
        &mut engine,
        matrix,
        "loadScale",
        "(FFF)V",
        &[Value::Float(2.0), Value::Float(3.0), Value::Float(4.0)],
    );
    call(
        &mut engine,
        matrix,
        "translate",
        "(FFF)V",
        &[Value::Float(10.0), Value::Float(20.0), Value::Float(30.0)],
    );
    assert_eq!(
        call(
            &mut engine,
            matrix,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(0)]
        ),
        Value::Float(20.0)
    );
    assert_eq!(
        call(
            &mut engine,
            matrix,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(1)]
        ),
        Value::Float(60.0)
    );
    assert_eq!(
        call(
            &mut engine,
            matrix,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(2)]
        ),
        Value::Float(120.0)
    );
    let invertible = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    call(
        &mut engine,
        invertible,
        "loadScale",
        "(FFF)V",
        &[Value::Float(2.0), Value::Float(4.0), Value::Float(5.0)],
    );
    assert_eq!(
        call(&mut engine, invertible, "inverse", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            invertible,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(0)]
        ),
        Value::Float(0.5)
    );
    call(
        &mut engine,
        invertible,
        "loadScale",
        "(FFF)V",
        &[Value::Float(0.0), Value::Float(1.0), Value::Float(1.0)],
    );
    assert_eq!(
        call(&mut engine, invertible, "inverse", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            invertible,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(0)]
        ),
        Value::Float(0.0)
    );
    call(
        &mut engine,
        invertible,
        "loadOrtho",
        "(FFFFFF)V",
        &[
            Value::Float(0.0),
            Value::Float(2.0),
            Value::Float(0.0),
            Value::Float(2.0),
            Value::Float(-1.0),
            Value::Float(1.0),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            invertible,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(0)]
        ),
        Value::Float(-1.0)
    );
    assert_eq!(
        call(
            &mut engine,
            invertible,
            "get",
            "(II)F",
            &[Value::Int(2), Value::Int(2)]
        ),
        Value::Float(-1.0)
    );

    let small = p36f_floats(&mut engine, &[1.0, 2.0, 3.0, 4.0]);
    let matrix2 = p49_new(&mut engine, P36F_RS_MATRIX2, "([F)V", &[small]);
    call(&mut engine, matrix2, "transpose", "()V", &[]);
    assert_eq!(
        call(
            &mut engine,
            matrix2,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(1)]
        ),
        Value::Float(3.0)
    );
    let matrix3 = p49_new(&mut engine, P36F_RS_MATRIX3, "()V", &[]);
    call(
        &mut engine,
        matrix3,
        "loadScale",
        "(FF)V",
        &[Value::Float(2.0), Value::Float(3.0)],
    );
    let widened = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    call(
        &mut engine,
        widened,
        "load",
        "(Landroid/renderscript/Matrix3f;)V",
        &[Value::Obj(matrix3)],
    );
    assert_eq!(
        call(
            &mut engine,
            widened,
            "get",
            "(II)F",
            &[Value::Int(1), Value::Int(1)]
        ),
        Value::Float(3.0)
    );
    assert_eq!(
        call(
            &mut engine,
            widened,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(3)]
        ),
        Value::Float(1.0)
    );
}

#[test]
fn renderscript_disabled_context_and_element_state_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine
        .cp
        .is_subtype(P36F_RS_ELEMENT, "Landroid/renderscript/BaseObj;"));
    assert!(engine.cp.is_subtype(
        "Landroid/renderscript/RSIllegalArgumentException;",
        "Landroid/renderscript/RSRuntimeException;"
    ));
    assert!(engine.cp.is_subtype(
        "Landroid/renderscript/RSRuntimeException;",
        "Ljava/lang/RuntimeException;"
    ));
    let context = instance(&mut engine, CONTEXT);
    assert_eq!(
        call_static(
            &mut engine,
            P36F_RS,
            "createMultiContext",
            "(Landroid/content/Context;Landroid/renderscript/RenderScript$ContextType;II)Landroid/renderscript/RenderScript;",
            &[Value::Obj(context), Value::Null, Value::Int(0), Value::Int(34)],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36F_RS,
            "create",
            "(Landroid/content/Context;I)Landroid/renderscript/RenderScript;",
            &[Value::Obj(context), Value::Int(22)],
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_static(
            P36F_RS,
            "create",
            "(Landroid/content/Context;I)Landroid/renderscript/RenderScript;",
            &[Value::Obj(context), Value::Int(34)],
        ),
        P36F_NPE,
    );
    assert_throws(
        engine.invoke_static(
            P36F_RS,
            "create",
            "(Landroid/content/Context;)Landroid/renderscript/RenderScript;",
            &[Value::Null],
        ),
        P36F_NPE,
    );
    // The bare host Context stand-in reports the no-APK fallback
    // targetSdkVersion (1), so the disabled internalCreate null is
    // returned directly instead of dereferenced by the sdk>=23 wrapper.
    assert_eq!(
        call_static(
            &mut engine,
            P36F_RS,
            "create",
            "(Landroid/content/Context;)Landroid/renderscript/RenderScript;",
            &[Value::Obj(context)],
        ),
        Value::Null
    );
    assert_eq!(
        call_static(&mut engine, P36F_RS, "releaseAllContexts", "()V", &[]),
        Value::Null
    );

    let element = instance(&mut engine, P36F_RS_ELEMENT);
    assert_eq!(
        call(&mut engine, element, "isComplex", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, element, "getSubElementCount", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            element,
            "getDataType",
            "()Landroid/renderscript/Element$DataType;",
            &[]
        ),
        Value::Null
    );
    assert_throws(
        engine.invoke_instance(
            element,
            "getSubElement",
            "(I)Landroid/renderscript/Element;",
            &[Value::Int(0)],
        ),
        "Landroid/renderscript/RSIllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            element,
            "getSubElementName",
            "(I)Ljava/lang/String;",
            &[Value::Int(0)],
        ),
        "Landroid/renderscript/RSIllegalArgumentException;",
    );
}

// ---- Appendix C Batch 36-G: service.credentials / app.admin ----

const P36G_CALLING_APP_INFO: &str = "Landroid/service/credentials/CallingAppInfo;";
const P36G_BEGIN_GET_OPTION: &str = "Landroid/service/credentials/BeginGetCredentialOption;";
const P36G_BEGIN_GET_REQ_BUILDER: &str =
    "Landroid/service/credentials/BeginGetCredentialRequest$Builder;";
const P36G_GET_REQ: &str = "Landroid/service/credentials/GetCredentialRequest;";
const P36G_CREATE_REQ: &str = "Landroid/service/credentials/CreateCredentialRequest;";
const P36G_BEGIN_CREATE_REQ: &str = "Landroid/service/credentials/BeginCreateCredentialRequest;";
const P36G_CLEAR_REQ: &str = "Landroid/service/credentials/ClearCredentialStateRequest;";
const P36G_CREDENTIAL_OPTION_BUILDER: &str = "Landroid/credentials/CredentialOption$Builder;";
const P36G_ACTION: &str = "Landroid/service/credentials/Action;";
const P36G_CREATE_ENTRY: &str = "Landroid/service/credentials/CreateEntry;";
const P36G_REMOTE_ENTRY: &str = "Landroid/service/credentials/RemoteEntry;";
const P36G_CREDENTIAL_ENTRY: &str = "Landroid/service/credentials/CredentialEntry;";
const P36G_BEGIN_GET_RESP: &str = "Landroid/service/credentials/BeginGetCredentialResponse;";
const P36G_BEGIN_GET_RESP_BUILDER: &str =
    "Landroid/service/credentials/BeginGetCredentialResponse$Builder;";
const P36G_BEGIN_CREATE_RESP: &str = "Landroid/service/credentials/BeginCreateCredentialResponse;";
const P36G_BEGIN_CREATE_RESP_BUILDER: &str =
    "Landroid/service/credentials/BeginCreateCredentialResponse$Builder;";
const P36G_PROVIDER_SERVICE: &str = "Landroid/service/credentials/CredentialProviderService;";
const P36G_DPM: &str = "Landroid/app/admin/DevicePolicyManager;";
const P36G_UPDATE_POLICY: &str = "Landroid/app/admin/SystemUpdatePolicy;";
const P36G_IAE: &str = "Ljava/lang/IllegalArgumentException;";
const P36G_NPE: &str = "Ljava/lang/NullPointerException;";

fn p36g_calling_app_info(engine: &mut Engine) -> ObjRef {
    let signing = instance(engine, "Landroid/content/pm/SigningInfo;");
    let package = text(engine, "dev.RustyDroid.caller");
    let origin = text(engine, "https://example.com");
    p49_new(
        engine,
        P36G_CALLING_APP_INFO,
        "(Ljava/lang/String;Landroid/content/pm/SigningInfo;Ljava/lang/String;)V",
        &[package, Value::Obj(signing), origin],
    )
}

fn p36g_bundle_with(engine: &mut Engine, key: &str, value: &str) -> ObjRef {
    let bundle = p41_bundle(engine);
    let key = text(engine, key);
    let value = text(engine, value);
    call(
        engine,
        bundle,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[key, value],
    );
    bundle
}

fn p36g_bundle_string(engine: &mut Engine, bundle: ObjRef, key: &str) -> Value {
    let key = text(engine, key);
    call(
        engine,
        bundle,
        "getString",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[key],
    )
}

fn p36g_list_size(engine: &mut Engine, list: Value) -> Value {
    call(engine, object(list), "size", "()I", &[])
}

fn p36g_list_size_of(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str) -> Value {
    let list = call(engine, recv, name, sig, &[]);
    p36g_list_size(engine, list)
}

#[test]
fn credentials_request_and_option_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine
        .cp
        .is_subtype(P36G_BEGIN_GET_OPTION, "Landroid/os/Parcelable;"));
    assert!(engine
        .cp
        .is_subtype(P36G_PROVIDER_SERVICE, "Landroid/app/Service;"));

    let signing = instance(&mut engine, "Landroid/content/pm/SigningInfo;");
    let empty_package = text(&mut engine, "");
    let bad_info = instance(&mut engine, P36G_CALLING_APP_INFO);
    assert_throws(
        engine.invoke_instance(
            bad_info,
            "<init>",
            "(Ljava/lang/String;Landroid/content/pm/SigningInfo;)V",
            &[empty_package, Value::Obj(signing)],
        ),
        P36G_IAE,
    );
    let package = text(&mut engine, "dev.RustyDroid.caller");
    assert_throws(
        engine.invoke_instance(
            bad_info,
            "<init>",
            "(Ljava/lang/String;Landroid/content/pm/SigningInfo;)V",
            &[package, Value::Null],
        ),
        P36G_NPE,
    );
    let info = p36g_calling_app_info(&mut engine);
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        "dev.RustyDroid.caller"
    );
    assert_eq!(
        call_text(&mut engine, info, "getOrigin", "()Ljava/lang/String;", &[]),
        "https://example.com"
    );
    assert!(!matches!(
        call(
            &mut engine,
            info,
            "getSigningInfo",
            "()Landroid/content/pm/SigningInfo;",
            &[]
        ),
        Value::Null
    ));

    let bundle = p36g_bundle_with(&mut engine, "k", "v");
    let id = text(&mut engine, "id1");
    let kind = text(&mut engine, "android.credentials.TYPE_PASSWORD_CREDENTIAL");
    let option = p49_new(
        &mut engine,
        P36G_BEGIN_GET_OPTION,
        "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)V",
        &[id, kind.clone(), Value::Obj(bundle)],
    );
    assert_eq!(
        call_text(&mut engine, option, "getId", "()Ljava/lang/String;", &[]),
        "id1"
    );
    assert_eq!(
        call_text(&mut engine, option, "getType", "()Ljava/lang/String;", &[]),
        "android.credentials.TYPE_PASSWORD_CREDENTIAL"
    );
    let candidate = object(call(
        &mut engine,
        option,
        "getCandidateQueryData",
        "()Landroid/os/Bundle;",
        &[],
    ));
    assert_ne!(candidate, bundle);
    let bundle_id = p36g_bundle_string(
        &mut engine,
        candidate,
        "android.service.credentials.BeginGetCredentialOption.BUNDLE_ID_KEY",
    );
    assert_eq!(text_of(&engine, &bundle_id), "id1");
    let bundle_value = p36g_bundle_string(&mut engine, candidate, "k");
    assert_eq!(text_of(&engine, &bundle_value), "v");
    let key2 = text(&mut engine, "k2");
    let value2 = text(&mut engine, "later");
    call(
        &mut engine,
        bundle,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[key2, value2],
    );
    assert_eq!(
        p36g_bundle_string(&mut engine, candidate, "k2"),
        Value::Null
    );
    let bad_option = instance(&mut engine, P36G_BEGIN_GET_OPTION);
    let empty_id = text(&mut engine, "");
    assert_throws(
        engine.invoke_instance(
            bad_option,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)V",
            &[empty_id, kind, Value::Obj(bundle)],
        ),
        P36G_IAE,
    );
    let good_id = text(&mut engine, "id2");
    assert_throws(
        engine.invoke_instance(
            bad_option,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)V",
            &[good_id, Value::Null, Value::Obj(bundle)],
        ),
        P36G_IAE,
    );

    let builder = p49_new(&mut engine, P36G_BEGIN_GET_REQ_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/service/credentials/BeginGetCredentialRequest;",
            &[],
        ),
        P36G_IAE,
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "addBeginGetCredentialOption",
            "(Landroid/service/credentials/BeginGetCredentialOption;)Landroid/service/credentials/BeginGetCredentialRequest$Builder;",
            &[Value::Null],
        ),
        P36G_NPE,
    );
    let empty = list_of(&mut engine, &[]);
    assert_throws(
        engine.invoke_instance(
            builder,
            "setBeginGetCredentialOptions",
            "(Ljava/util/List;)Landroid/service/credentials/BeginGetCredentialRequest$Builder;",
            &[Value::Obj(empty)],
        ),
        P36G_IAE,
    );
    let with_null = list_of(&mut engine, &[Value::Null]);
    assert_throws(
        engine.invoke_instance(
            builder,
            "setBeginGetCredentialOptions",
            "(Ljava/util/List;)Landroid/service/credentials/BeginGetCredentialRequest$Builder;",
            &[Value::Obj(with_null)],
        ),
        P36G_NPE,
    );
    call(
        &mut engine,
        builder,
        "setCallingAppInfo",
        "(Landroid/service/credentials/CallingAppInfo;)Landroid/service/credentials/BeginGetCredentialRequest$Builder;",
        &[Value::Obj(info)],
    );
    call(
        &mut engine,
        builder,
        "addBeginGetCredentialOption",
        "(Landroid/service/credentials/BeginGetCredentialOption;)Landroid/service/credentials/BeginGetCredentialRequest$Builder;",
        &[Value::Obj(option)],
    );
    let request = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/service/credentials/BeginGetCredentialRequest;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            request,
            "getCallingAppInfo",
            "()Landroid/service/credentials/CallingAppInfo;",
            &[]
        ),
        Value::Obj(info)
    );
    let options = call(
        &mut engine,
        request,
        "getBeginGetCredentialOptions",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36g_list_size(&mut engine, options.clone()), Value::Int(1));
    assert_eq!(p49_list_get(&mut engine, options, 0), Value::Obj(option));

    let retrieval = p41_bundle(&mut engine);
    let query = p41_bundle(&mut engine);
    let option_builder = instance(&mut engine, P36G_CREDENTIAL_OPTION_BUILDER);
    let kind2 = text(
        &mut engine,
        "android.credentials.TYPE_PUBLIC_KEY_CREDENTIAL",
    );
    assert_throws(
        engine.invoke_instance(
            option_builder,
            "<init>",
            "(Ljava/lang/String;Landroid/os/Bundle;Landroid/os/Bundle;)V",
            &[kind2.clone(), Value::Null, Value::Obj(query)],
        ),
        P36G_NPE,
    );
    call(
        &mut engine,
        option_builder,
        "<init>",
        "(Ljava/lang/String;Landroid/os/Bundle;Landroid/os/Bundle;)V",
        &[kind2, Value::Obj(retrieval), Value::Obj(query)],
    );
    call(
        &mut engine,
        option_builder,
        "setIsSystemProviderRequired",
        "(Z)Landroid/credentials/CredentialOption$Builder;",
        &[Value::Int(1)],
    );
    let provider = instance(&mut engine, "Landroid/content/ComponentName;");
    call(
        &mut engine,
        option_builder,
        "addAllowedProvider",
        "(Landroid/content/ComponentName;)Landroid/credentials/CredentialOption$Builder;",
        &[Value::Obj(provider)],
    );
    let credential_option = object(call(
        &mut engine,
        option_builder,
        "build",
        "()Landroid/credentials/CredentialOption;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            credential_option,
            "getType",
            "()Ljava/lang/String;",
            &[]
        ),
        "android.credentials.TYPE_PUBLIC_KEY_CREDENTIAL"
    );
    assert_eq!(
        call(
            &mut engine,
            credential_option,
            "getCredentialRetrievalData",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Obj(retrieval)
    );
    assert_eq!(
        call(
            &mut engine,
            credential_option,
            "isSystemProviderRequired",
            "()Z",
            &[]
        ),
        Value::Int(1)
    );
    let allowed = call(
        &mut engine,
        credential_option,
        "getAllowedProviders",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(p36g_list_size(&mut engine, allowed), Value::Int(1));

    let get_request = instance(&mut engine, P36G_GET_REQ);
    assert_throws(
        engine.invoke_instance(
            get_request,
            "<init>",
            "(Landroid/service/credentials/CallingAppInfo;Ljava/util/List;)V",
            &[Value::Null, Value::Obj(empty)],
        ),
        P36G_NPE,
    );
    let option_list = list_of(&mut engine, &[Value::Obj(credential_option)]);
    call(
        &mut engine,
        get_request,
        "<init>",
        "(Landroid/service/credentials/CallingAppInfo;Ljava/util/List;)V",
        &[Value::Obj(info), Value::Obj(option_list)],
    );
    assert_eq!(
        call(
            &mut engine,
            get_request,
            "getCredentialOptions",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Obj(option_list)
    );

    let create_kind = text(&mut engine, "android.credentials.TYPE_PASSWORD_CREDENTIAL");
    let create_request = p49_new(
        &mut engine,
        P36G_CREATE_REQ,
        "(Landroid/service/credentials/CallingAppInfo;Ljava/lang/String;Landroid/os/Bundle;)V",
        &[Value::Obj(info), create_kind, Value::Obj(bundle)],
    );
    assert_eq!(
        call(
            &mut engine,
            create_request,
            "getData",
            "()Landroid/os/Bundle;",
            &[]
        ),
        Value::Obj(bundle)
    );
    let begin_create_kind = text(&mut engine, "android.credentials.TYPE_PASSWORD_CREDENTIAL");
    let begin_create = p49_new(
        &mut engine,
        P36G_BEGIN_CREATE_REQ,
        "(Ljava/lang/String;Landroid/os/Bundle;)V",
        &[begin_create_kind, Value::Obj(bundle)],
    );
    let copied = object(call(
        &mut engine,
        begin_create,
        "getData",
        "()Landroid/os/Bundle;",
        &[],
    ));
    assert_ne!(copied, bundle);
    assert_eq!(
        call(
            &mut engine,
            begin_create,
            "getCallingAppInfo",
            "()Landroid/service/credentials/CallingAppInfo;",
            &[]
        ),
        Value::Null
    );
    let clear = instance(&mut engine, P36G_CLEAR_REQ);
    assert_throws(
        engine.invoke_instance(
            clear,
            "<init>",
            "(Landroid/service/credentials/CallingAppInfo;Landroid/os/Bundle;)V",
            &[Value::Obj(info), Value::Null],
        ),
        P36G_NPE,
    );
    call(
        &mut engine,
        clear,
        "<init>",
        "(Landroid/service/credentials/CallingAppInfo;Landroid/os/Bundle;)V",
        &[Value::Obj(info), Value::Obj(bundle)],
    );
    assert_eq!(
        call(&mut engine, clear, "getData", "()Landroid/os/Bundle;", &[]),
        Value::Obj(bundle)
    );
}

#[test]
fn credentials_entries_responses_and_service_binding_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let slice = instance(&mut engine, "Landroid/app/slice/Slice;");
    let action = instance(&mut engine, P36G_ACTION);
    assert_throws(
        engine.invoke_instance(
            action,
            "<init>",
            "(Landroid/app/slice/Slice;)V",
            &[Value::Null],
        ),
        P36G_NPE,
    );
    call(
        &mut engine,
        action,
        "<init>",
        "(Landroid/app/slice/Slice;)V",
        &[Value::Obj(slice)],
    );
    assert_eq!(
        call(
            &mut engine,
            action,
            "getSlice",
            "()Landroid/app/slice/Slice;",
            &[]
        ),
        Value::Obj(slice)
    );
    let create_entry = p49_new(
        &mut engine,
        P36G_CREATE_ENTRY,
        "(Landroid/app/slice/Slice;)V",
        &[Value::Obj(slice)],
    );
    let remote_entry = p49_new(
        &mut engine,
        P36G_REMOTE_ENTRY,
        "(Landroid/app/slice/Slice;)V",
        &[Value::Obj(slice)],
    );

    let bundle = p41_bundle(&mut engine);
    let id = text(&mut engine, "opt-1");
    let kind = text(&mut engine, "android.credentials.TYPE_PASSWORD_CREDENTIAL");
    let option = p49_new(
        &mut engine,
        P36G_BEGIN_GET_OPTION,
        "(Ljava/lang/String;Ljava/lang/String;Landroid/os/Bundle;)V",
        &[id, kind, Value::Obj(bundle)],
    );
    let entry = p49_new(
        &mut engine,
        P36G_CREDENTIAL_ENTRY,
        "(Landroid/service/credentials/BeginGetCredentialOption;Landroid/app/slice/Slice;)V",
        &[Value::Obj(option), Value::Obj(slice)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            entry,
            "getBeginGetCredentialOptionId",
            "()Ljava/lang/String;",
            &[]
        ),
        "opt-1"
    );
    assert_eq!(
        call_text(&mut engine, entry, "getType", "()Ljava/lang/String;", &[]),
        "android.credentials.TYPE_PASSWORD_CREDENTIAL"
    );
    let empty_kind = text(&mut engine, "");
    let unbound_entry = p49_new(
        &mut engine,
        P36G_CREDENTIAL_ENTRY,
        "(Ljava/lang/String;Landroid/app/slice/Slice;)V",
        &[empty_kind, Value::Obj(slice)],
    );
    assert_eq!(
        call(
            &mut engine,
            unbound_entry,
            "getBeginGetCredentialOptionId",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call_text(
            &mut engine,
            unbound_entry,
            "getType",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    let bad_entry = instance(&mut engine, P36G_CREDENTIAL_ENTRY);
    let entry_id = text(&mut engine, "");
    let entry_kind = text(&mut engine, "t");
    assert_throws(
        engine.invoke_instance(
            bad_entry,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;Landroid/app/slice/Slice;)V",
            &[entry_id, entry_kind, Value::Obj(slice)],
        ),
        P36G_IAE,
    );

    let empty_response = p49_new(&mut engine, P36G_BEGIN_GET_RESP, "()V", &[]);
    assert_eq!(
        p36g_list_size_of(
            &mut engine,
            empty_response,
            "getCredentialEntries",
            "()Ljava/util/List;"
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            empty_response,
            "getRemoteCredentialEntry",
            "()Landroid/service/credentials/RemoteEntry;",
            &[]
        ),
        Value::Null
    );
    let builder = p49_new(&mut engine, P36G_BEGIN_GET_RESP_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            builder,
            "addCredentialEntry",
            "(Landroid/service/credentials/CredentialEntry;)Landroid/service/credentials/BeginGetCredentialResponse$Builder;",
            &[Value::Null],
        ),
        P36G_NPE,
    );
    call(
        &mut engine,
        builder,
        "addCredentialEntry",
        "(Landroid/service/credentials/CredentialEntry;)Landroid/service/credentials/BeginGetCredentialResponse$Builder;",
        &[Value::Obj(entry)],
    );
    call(
        &mut engine,
        builder,
        "addAction",
        "(Landroid/service/credentials/Action;)Landroid/service/credentials/BeginGetCredentialResponse$Builder;",
        &[Value::Obj(action)],
    );
    call(
        &mut engine,
        builder,
        "addAuthenticationAction",
        "(Landroid/service/credentials/Action;)Landroid/service/credentials/BeginGetCredentialResponse$Builder;",
        &[Value::Obj(action)],
    );
    call(
        &mut engine,
        builder,
        "setRemoteCredentialEntry",
        "(Landroid/service/credentials/RemoteEntry;)Landroid/service/credentials/BeginGetCredentialResponse$Builder;",
        &[Value::Obj(remote_entry)],
    );
    let response = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/service/credentials/BeginGetCredentialResponse;",
        &[],
    ));
    let entries = call(
        &mut engine,
        response,
        "getCredentialEntries",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36g_list_size(&mut engine, entries.clone()), Value::Int(1));
    assert_eq!(p49_list_get(&mut engine, entries, 0), Value::Obj(entry));
    assert_eq!(
        p36g_list_size_of(&mut engine, response, "getActions", "()Ljava/util/List;"),
        Value::Int(1)
    );
    assert_eq!(
        p36g_list_size_of(
            &mut engine,
            response,
            "getAuthenticationActions",
            "()Ljava/util/List;"
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            response,
            "getRemoteCredentialEntry",
            "()Landroid/service/credentials/RemoteEntry;",
            &[]
        ),
        Value::Obj(remote_entry)
    );

    let create_builder = p49_new(&mut engine, P36G_BEGIN_CREATE_RESP_BUILDER, "()V", &[]);
    let empty = list_of(&mut engine, &[]);
    assert_throws(
        engine.invoke_instance(
            create_builder,
            "setCreateEntries",
            "(Ljava/util/List;)Landroid/service/credentials/BeginCreateCredentialResponse$Builder;",
            &[Value::Obj(empty)],
        ),
        P36G_IAE,
    );
    call(
        &mut engine,
        create_builder,
        "addCreateEntry",
        "(Landroid/service/credentials/CreateEntry;)Landroid/service/credentials/BeginCreateCredentialResponse$Builder;",
        &[Value::Obj(create_entry)],
    );
    let create_response = object(call(
        &mut engine,
        create_builder,
        "build",
        "()Landroid/service/credentials/BeginCreateCredentialResponse;",
        &[],
    ));
    assert_eq!(
        p36g_list_size_of(
            &mut engine,
            create_response,
            "getCreateEntries",
            "()Ljava/util/List;"
        ),
        Value::Int(1)
    );
    let empty_create = p49_new(&mut engine, P36G_BEGIN_CREATE_RESP, "()V", &[]);
    assert_eq!(
        p36g_list_size_of(
            &mut engine,
            empty_create,
            "getCreateEntries",
            "()Ljava/util/List;"
        ),
        Value::Int(0)
    );

    let service = instance(&mut engine, P36G_PROVIDER_SERVICE);
    let wrong_action = text(&mut engine, "example.WRONG");
    let wrong_intent = instance(&mut engine, "Landroid/content/Intent;");
    call(
        &mut engine,
        wrong_intent,
        "<init>",
        "(Ljava/lang/String;)V",
        &[wrong_action],
    );
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(wrong_intent)]
        ),
        Value::Null
    );
    let action_text = text(
        &mut engine,
        "android.service.credentials.CredentialProviderService",
    );
    let intent = instance(&mut engine, "Landroid/content/Intent;");
    call(
        &mut engine,
        intent,
        "<init>",
        "(Ljava/lang/String;)V",
        &[action_text],
    );
    let binder = call(
        &mut engine,
        service,
        "onBind",
        "(Landroid/content/Intent;)Landroid/os/IBinder;",
        &[Value::Obj(intent)],
    );
    assert!(!matches!(binder, Value::Null));
    assert_eq!(
        call(
            &mut engine,
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Obj(intent)]
        ),
        binder
    );
    assert_throws(
        engine.invoke_instance(
            service,
            "onBind",
            "(Landroid/content/Intent;)Landroid/os/IBinder;",
            &[Value::Null],
        ),
        P36G_NPE,
    );

    // Deep audit: the (String, Slice) CredentialEntry constructor uses
    // Objects.requireNonNull for the type (empty accepted, null NPE),
    // unlike the other constructors' checkStringNotEmpty (IAE).
    let entry_class = "Landroid/service/credentials/CredentialEntry;";
    let empty_type = text(&mut engine, "");
    let empty_typed = p49_new(
        &mut engine,
        entry_class,
        "(Ljava/lang/String;Landroid/app/slice/Slice;)V",
        &[empty_type, Value::Obj(slice)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            empty_typed,
            "getType",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    let null_typed = instance(&mut engine, entry_class);
    assert_throws(
        engine.invoke_instance(
            null_typed,
            "<init>",
            "(Ljava/lang/String;Landroid/app/slice/Slice;)V",
            &[Value::Null, Value::Obj(slice)],
        ),
        P36G_NPE,
    );
}

#[test]
fn device_policy_no_service_and_system_update_policy_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine.cp.is_subtype(
        "Landroid/app/admin/SystemUpdatePolicy$ValidationFailedException;",
        "Ljava/lang/IllegalArgumentException;"
    ));
    let context = instance(&mut engine, CONTEXT);
    let admin = instance(&mut engine, "Landroid/content/ComponentName;");
    let service_name = text(&mut engine, "device_policy");
    let from_context = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    assert_eq!(
        call(
            &mut engine,
            from_context,
            "isAdminActive",
            "(Landroid/content/ComponentName;)Z",
            &[Value::Obj(admin)]
        ),
        Value::Int(0)
    );

    let manager = p49_new(
        &mut engine,
        P36G_DPM,
        "(Landroid/content/Context;Landroid/app/admin/IDevicePolicyManager;)V",
        &[Value::Obj(context), Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isAdminActive",
            "(Landroid/content/ComponentName;)Z",
            &[Value::Obj(admin)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getActiveAdmins",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "hasGrantedPolicy",
            "(Landroid/content/ComponentName;I)Z",
            &[Value::Obj(admin), Value::Int(1)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getPasswordQuality",
            "(Landroid/content/ComponentName;)I",
            &[Value::Null]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getPasswordMinimumLength",
            "(Landroid/content/ComponentName;)I",
            &[Value::Obj(admin)]
        ),
        Value::Int(0)
    );
    call(
        &mut engine,
        manager,
        "setPasswordQuality",
        "(Landroid/content/ComponentName;I)V",
        &[Value::Obj(admin), Value::Int(0x60000)],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getPasswordQuality",
            "(Landroid/content/ComponentName;)I",
            &[Value::Obj(admin)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getStorageEncryptionStatus",
            "()I",
            &[]
        ),
        Value::Int(0)
    );
    let package = text(&mut engine, "dev.RustyDroid.owner");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isDeviceOwnerApp",
            "(Ljava/lang/String;)Z",
            &[package]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isDeviceOwnerApp",
            "(Ljava/lang/String;)Z",
            &[Value::Null]
        ),
        Value::Int(0)
    );
    let profile_package = text(&mut engine, "dev.RustyDroid.owner");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isProfileOwnerApp",
            "(Ljava/lang/String;)Z",
            &[profile_package]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getDeviceOwnerComponentOnCallingUser",
            "()Landroid/content/ComponentName;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, manager, "getDeviceOwnerUserId", "()I", &[]),
        Value::Int(-10000)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getRequiredStrongAuthTimeout",
            "(Landroid/content/ComponentName;)J",
            &[Value::Null]
        ),
        Value::Long(259_200_000)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getSystemUpdatePolicy",
            "()Landroid/app/admin/SystemUpdatePolicy;",
            &[]
        ),
        Value::Null
    );
    call(
        &mut engine,
        manager,
        "removeActiveAdmin",
        "(Landroid/content/ComponentName;)V",
        &[Value::Obj(admin)],
    );
    let provision_action = text(&mut engine, "android.app.action.PROVISION_MANAGED_PROFILE");
    assert_throws(
        engine.invoke_instance(
            manager,
            "isProvisioningAllowed",
            "(Ljava/lang/String;)Z",
            &[provision_action],
        ),
        P36G_NPE,
    );

    let parent = p49_new(
        &mut engine,
        P36G_DPM,
        "(Landroid/content/Context;Landroid/app/admin/IDevicePolicyManager;Z)V",
        &[Value::Obj(context), Value::Null, Value::Int(1)],
    );
    assert_throws(
        engine.invoke_instance(
            parent,
            "isAdminActive",
            "(Landroid/content/ComponentName;)Z",
            &[Value::Obj(admin)],
        ),
        "Ljava/lang/SecurityException;",
    );
    assert_eq!(
        call(
            &mut engine,
            parent,
            "getPasswordQuality",
            "(Landroid/content/ComponentName;)I",
            &[Value::Null]
        ),
        Value::Int(0)
    );

    let windowed = object(call_static(
        &mut engine,
        P36G_UPDATE_POLICY,
        "createWindowedInstallPolicy",
        "(II)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Int(1380), Value::Int(120)],
    ));
    assert_eq!(
        call(&mut engine, windowed, "getPolicyType", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, windowed, "getInstallWindowStart", "()I", &[]),
        Value::Int(1380)
    );
    assert_eq!(
        call(&mut engine, windowed, "getInstallWindowEnd", "()I", &[]),
        Value::Int(120)
    );
    assert_eq!(
        call_text(
            &mut engine,
            windowed,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "SystemUpdatePolicy (type: 2, windowStart: 1380, windowEnd: 120, freezes: [])"
    );
    assert_eq!(
        p36g_list_size_of(
            &mut engine,
            windowed,
            "getFreezePeriods",
            "()Ljava/util/List;"
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P36G_UPDATE_POLICY,
            "createWindowedInstallPolicy",
            "(II)Landroid/app/admin/SystemUpdatePolicy;",
            &[Value::Int(-1), Value::Int(0)],
        ),
        P36G_IAE,
    );
    assert_throws(
        engine.invoke_static(
            P36G_UPDATE_POLICY,
            "createWindowedInstallPolicy",
            "(II)Landroid/app/admin/SystemUpdatePolicy;",
            &[Value::Int(0), Value::Int(1440)],
        ),
        P36G_IAE,
    );
    let automatic = object(call_static(
        &mut engine,
        P36G_UPDATE_POLICY,
        "createAutomaticInstallPolicy",
        "()Landroid/app/admin/SystemUpdatePolicy;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, automatic, "getPolicyType", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, automatic, "getInstallWindowStart", "()I", &[]),
        Value::Int(-1)
    );
    let postponed = object(call_static(
        &mut engine,
        P36G_UPDATE_POLICY,
        "createPostponeInstallPolicy",
        "()Landroid/app/admin/SystemUpdatePolicy;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, postponed, "getPolicyType", "()I", &[]),
        Value::Int(3)
    );
    call(
        &mut engine,
        manager,
        "setSystemUpdatePolicy",
        "(Landroid/content/ComponentName;Landroid/app/admin/SystemUpdatePolicy;)V",
        &[Value::Obj(admin), Value::Obj(windowed)],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getSystemUpdatePolicy",
            "()Landroid/app/admin/SystemUpdatePolicy;",
            &[]
        ),
        Value::Null
    );
}

// ---- Appendix C Batch 36-H: android.net.wifi + android.net.ipsec.ike ----

const P36H_WIFI_MANAGER: &str = "Landroid/net/wifi/WifiManager;";
const P36H_SCAN_RESULT: &str = "Landroid/net/wifi/ScanResult;";
const P36H_WIFI_SSID: &str = "Landroid/net/wifi/WifiSsid;";
const P36H_SUPPLICANT: &str = "Landroid/net/wifi/SupplicantState;";
const P36H_SA_PROPOSAL: &str = "Landroid/net/ipsec/ike/SaProposal;";
const P36H_IKE_PROPOSAL: &str = "Landroid/net/ipsec/ike/IkeSaProposal;";
const P36H_IKE_BUILDER: &str = "Landroid/net/ipsec/ike/IkeSaProposal$Builder;";
const P36H_CHILD_PROPOSAL: &str = "Landroid/net/ipsec/ike/ChildSaProposal;";
const P36H_CHILD_BUILDER: &str = "Landroid/net/ipsec/ike/ChildSaProposal$Builder;";
const P36H_TS: &str = "Landroid/net/ipsec/ike/IkeTrafficSelector;";
const P36H_ID_FQDN: &str = "Landroid/net/ipsec/ike/IkeFqdnIdentification;";
const P36H_ID_RFC822: &str = "Landroid/net/ipsec/ike/IkeRfc822AddrIdentification;";
const P36H_ID_KEYID: &str = "Landroid/net/ipsec/ike/IkeKeyIdIdentification;";
const P36H_ID_IPV4: &str = "Landroid/net/ipsec/ike/IkeIpv4AddrIdentification;";
const P36H_TRANSPORT_BUILDER: &str =
    "Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;";
const P36H_TUNNEL_BUILDER: &str = "Landroid/net/ipsec/ike/TunnelModeChildSessionParams$Builder;";
const P36H_IAE: &str = "Ljava/lang/IllegalArgumentException;";
const P36H_NPE: &str = "Ljava/lang/NullPointerException;";
const P36H_INTS2: &str = "(II)I";

fn p36h_size(engine: &mut Engine, value: Value) -> Value {
    let list = object(value);
    call(engine, list, "size", "()I", &[])
}

fn p36h_int_value(engine: &mut Engine, value: Value) -> Value {
    let boxed = object(value);
    call(engine, boxed, "intValue", "()I", &[])
}

fn p36h_supplicant(engine: &mut Engine, name: &str) -> Value {
    let state = instance(engine, P36H_SUPPLICANT);
    let label = text(engine, name);
    engine.heap.put_field(state, "rd_name", label).unwrap();
    Value::Obj(state)
}

#[test]
fn wifi_disabled_profile_and_value_classes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = instance(&mut engine, CONTEXT);
    let service_name = text(&mut engine, "wifi");
    let manager = object(call(
        &mut engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ));
    assert_eq!(
        call(&mut engine, manager, "getWifiState", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, manager, "isWifiEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "setWifiEnabled",
            "(Z)Z",
            &[Value::Int(1)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, manager, "startScan", "()Z", &[]),
        Value::Int(0)
    );
    let scans = call(
        &mut engine,
        manager,
        "getScanResults",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, scans), Value::Int(0));
    let configured = call(
        &mut engine,
        manager,
        "getConfiguredNetworks",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, configured), Value::Int(0));
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getMaxNumberOfNetworkSuggestionsPerApp",
            "()I",
            &[]
        ),
        Value::Int(1024)
    );

    // Pure WifiManager statics.
    assert_eq!(
        call_static(
            &mut engine,
            P36H_WIFI_MANAGER,
            "calculateSignalLevel",
            P36H_INTS2,
            &[Value::Int(-100), Value::Int(5)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_WIFI_MANAGER,
            "calculateSignalLevel",
            P36H_INTS2,
            &[Value::Int(-55), Value::Int(5)]
        ),
        Value::Int(4)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_WIFI_MANAGER,
            "calculateSignalLevel",
            P36H_INTS2,
            &[Value::Int(-78), Value::Int(5)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_WIFI_MANAGER,
            "compareSignalLevel",
            P36H_INTS2,
            &[Value::Int(3), Value::Int(7)]
        ),
        Value::Int(-4)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_WIFI_MANAGER,
            "getMaxNumberOfNetworkSuggestionsPerApp",
            "(Z)I",
            &[Value::Int(1)]
        ),
        Value::Int(256)
    );

    // Default (not-connected) WifiInfo.
    let info = object(call(
        &mut engine,
        manager,
        "getConnectionInfo",
        "()Landroid/net/wifi/WifiInfo;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, info, "getSSID", "()Ljava/lang/String;", &[]),
        "<unknown ssid>"
    );
    assert_eq!(
        call(&mut engine, info, "getBSSID", "()Ljava/lang/String;", &[]),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, info, "getNetworkId", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, info, "getRssi", "()I", &[]),
        Value::Int(-127)
    );
    assert_eq!(
        call(&mut engine, info, "getLinkSpeed", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, info, "getFrequency", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, info, "getTxLinkSpeedMbps", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getMacAddress",
            "()Ljava/lang/String;",
            &[]
        ),
        "02:00:00:00:00:00"
    );
    assert_eq!(
        call(&mut engine, info, "getIpAddress", "()I", &[]),
        Value::Int(0)
    );
    let supplicant = object(call(
        &mut engine,
        info,
        "getSupplicantState",
        "()Landroid/net/wifi/SupplicantState;",
        &[],
    ));
    assert_eq!(
        text_of(&engine, &field(&engine, supplicant, "rd_name")),
        "UNINITIALIZED"
    );
    let detailed = object(call(
        &mut engine,
        info,
        "getDetailedState",
        "()Landroid/net/NetworkInfo$DetailedState;",
        &[],
    ));
    assert_eq!(
        text_of(&engine, &field(&engine, detailed, "rd_name")),
        "IDLE"
    );

    // WifiSsid value semantics.
    let ssid_text = text(&mut engine, "RustyDroid");
    let ssid = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromUtf8Text",
        "(Ljava/lang/CharSequence;)Landroid/net/wifi/WifiSsid;",
        &[ssid_text],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            ssid,
            "getUtf8Text",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "RustyDroid"
    );
    assert_eq!(
        call_text(&mut engine, ssid, "toString", "()Ljava/lang/String;", &[]),
        "\"RustyDroid\""
    );
    let ssid_bytes = call(&mut engine, ssid, "getBytes", "()[B", &[]);
    assert_eq!(
        byte_array_values(&engine, &ssid_bytes),
        b"RustyDroid".to_vec()
    );
    let raw = byte_array(&mut engine, &[0xff, 0x00]);
    let binary_ssid = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromBytes",
        "([B)Landroid/net/wifi/WifiSsid;",
        &[raw],
    ));
    assert_eq!(
        call(
            &mut engine,
            binary_ssid,
            "getUtf8Text",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call_text(
            &mut engine,
            binary_ssid,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "ff00"
    );
    let quoted = text(&mut engine, "\"abc\"");
    let from_string = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromString",
        "(Ljava/lang/String;)Landroid/net/wifi/WifiSsid;",
        &[quoted],
    ));
    let abc_text = text(&mut engine, "abc");
    let from_utf8 = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromUtf8Text",
        "(Ljava/lang/CharSequence;)Landroid/net/wifi/WifiSsid;",
        &[abc_text],
    ));
    assert_eq!(
        call(
            &mut engine,
            from_string,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(from_utf8)]
        ),
        Value::Int(1)
    );
    let bad_hex = text(&mut engine, "zz");
    assert_throws(
        engine.invoke_static(
            P36H_WIFI_SSID,
            "fromString",
            "(Ljava/lang/String;)Landroid/net/wifi/WifiSsid;",
            &[bad_hex],
        ),
        P36H_IAE,
    );

    // SupplicantState predicates.
    let completed = p36h_supplicant(&mut engine, "COMPLETED");
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isValidState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[completed]
        ),
        Value::Int(1)
    );
    let uninitialized = p36h_supplicant(&mut engine, "UNINITIALIZED");
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isValidState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[uninitialized]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isValidState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[Value::Null]
        ),
        Value::Int(1)
    );
    let associated = p36h_supplicant(&mut engine, "ASSOCIATED");
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isHandshakeState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[associated]
        ),
        Value::Int(1)
    );
    let completed2 = p36h_supplicant(&mut engine, "COMPLETED");
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isHandshakeState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[completed2]
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_static(
            P36H_SUPPLICANT,
            "isHandshakeState",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[Value::Null],
        ),
        P36H_NPE,
    );
    let iface_disabled = p36h_supplicant(&mut engine, "INTERFACE_DISABLED");
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SUPPLICANT,
            "isDriverActive",
            "(Landroid/net/wifi/SupplicantState;)Z",
            &[iface_disabled]
        ),
        Value::Int(0)
    );

    // ScanResult band/channel conversions.
    let conv = |engine: &mut Engine, channel: i32, band: i32| {
        call_static(
            engine,
            P36H_SCAN_RESULT,
            "convertChannelToFrequencyMhzIfSupported",
            P36H_INTS2,
            &[Value::Int(channel), Value::Int(band)],
        )
    };
    assert_eq!(conv(&mut engine, 6, 1), Value::Int(2437));
    assert_eq!(conv(&mut engine, 14, 1), Value::Int(2484));
    assert_eq!(conv(&mut engine, 36, 2), Value::Int(5180));
    assert_eq!(conv(&mut engine, 2, 8), Value::Int(5935));
    assert_eq!(conv(&mut engine, 1, 16), Value::Int(58320));
    assert_eq!(conv(&mut engine, 99, 1), Value::Int(-1));
    let to_channel = |engine: &mut Engine, freq: i32| {
        call_static(
            engine,
            P36H_SCAN_RESULT,
            "convertFrequencyMhzToChannelIfSupported",
            "(I)I",
            &[Value::Int(freq)],
        )
    };
    assert_eq!(to_channel(&mut engine, 2437), Value::Int(6));
    assert_eq!(to_channel(&mut engine, 2484), Value::Int(14));
    assert_eq!(to_channel(&mut engine, 5180), Value::Int(36));
    assert_eq!(to_channel(&mut engine, 5935), Value::Int(2));
    assert_eq!(to_channel(&mut engine, 999), Value::Int(-1));
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SCAN_RESULT,
            "is6GHzPsc",
            "(I)Z",
            &[Value::Int(5975)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SCAN_RESULT,
            "is6GHzPsc",
            "(I)Z",
            &[Value::Int(5980)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SCAN_RESULT,
            "toBand",
            "(I)I",
            &[Value::Int(5955)]
        ),
        Value::Int(8)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SCAN_RESULT,
            "toBand",
            "(I)I",
            &[Value::Int(100)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36H_SCAN_RESULT,
            "getBandFromOpClass",
            P36H_INTS2,
            &[Value::Int(131), Value::Int(1)]
        ),
        Value::Int(8)
    );
}

fn p36h_ike_builder(engine: &mut Engine) -> ObjRef {
    p49_new(engine, P36H_IKE_BUILDER, "()V", &[])
}

fn p36h_child_proposal(engine: &mut Engine) -> ObjRef {
    let builder = p49_new(engine, P36H_CHILD_BUILDER, "()V", &[]);
    call(
        engine,
        builder,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/ChildSaProposal$Builder;",
        &[Value::Int(20), Value::Int(256)],
    );
    object(call(
        engine,
        builder,
        "build",
        "()Landroid/net/ipsec/ike/ChildSaProposal;",
        &[],
    ))
}

#[test]
fn ike_sa_proposal_builders_validate_like_aosp() {
    let mut engine = engine_of(&DexBuilder::new());

    // Happy-path IKE proposal: AES-CBC-256 + HMAC-SHA2-256 + PRF SHA2-256 + DH 14.
    let builder = p36h_ike_builder(&mut engine);
    let chained = call(
        &mut engine,
        builder,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12), Value::Int(256)],
    );
    assert_eq!(chained, Value::Obj(builder));
    call(
        &mut engine,
        builder,
        "addIntegrityAlgorithm",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12)],
    );
    call(
        &mut engine,
        builder,
        "addPseudorandomFunction",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        builder,
        "addDhGroup",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(14)],
    );
    let proposal = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/net/ipsec/ike/IkeSaProposal;",
        &[],
    ));
    let enc = call(
        &mut engine,
        proposal,
        "getEncryptionAlgorithms",
        "()Ljava/util/List;",
        &[],
    );
    let pair = object(p49_list_get(&mut engine, enc, 0));
    let first = field(&engine, pair, "first");
    assert_eq!(p36h_int_value(&mut engine, first), Value::Int(12));
    let second = field(&engine, pair, "second");
    assert_eq!(p36h_int_value(&mut engine, second), Value::Int(256));
    let integ = call(
        &mut engine,
        proposal,
        "getIntegrityAlgorithms",
        "()Ljava/util/List;",
        &[],
    );
    let integ0 = p49_list_get(&mut engine, integ, 0);
    assert_eq!(p36h_int_value(&mut engine, integ0), Value::Int(12));
    let prfs = call(
        &mut engine,
        proposal,
        "getPseudorandomFunctions",
        "()Ljava/util/List;",
        &[],
    );
    let prf0 = p49_list_get(&mut engine, prfs, 0);
    assert_eq!(p36h_int_value(&mut engine, prf0), Value::Int(5));
    let dhs = call(
        &mut engine,
        proposal,
        "getDhGroups",
        "()Ljava/util/List;",
        &[],
    );
    let dh0 = p49_list_get(&mut engine, dhs, 0);
    assert_eq!(p36h_int_value(&mut engine, dh0), Value::Int(14));

    // Key-length and algorithm-id validation.
    let b = p36h_ike_builder(&mut engine);
    assert_throws(
        engine.invoke_instance(
            b,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(12), Value::Int(64)],
        ),
        P36H_IAE,
    );
    assert_throws(
        engine.invoke_instance(
            b,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(3), Value::Int(128)],
        ),
        P36H_IAE,
    );
    assert_throws(
        engine.invoke_instance(
            b,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(99), Value::Int(0)],
        ),
        P36H_IAE,
    );
    assert_throws(
        engine.invoke_instance(
            b,
            "addIntegrityAlgorithm",
            "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(99)],
        ),
        P36H_IAE,
    );
    // Mixing AEAD with a normal cipher is rejected when the second is added.
    call(
        &mut engine,
        b,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12), Value::Int(128)],
    );
    assert_throws(
        engine.invoke_instance(
            b,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(20), Value::Int(256)],
        ),
        P36H_IAE,
    );

    // IKE build requirements.
    let no_prf = p36h_ike_builder(&mut engine);
    call(
        &mut engine,
        no_prf,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12), Value::Int(128)],
    );
    assert_throws(
        engine.invoke_instance(
            no_prf,
            "build",
            "()Landroid/net/ipsec/ike/IkeSaProposal;",
            &[],
        ),
        P36H_IAE,
    );
    // AEAD requires INTEGRITY_ALGORITHM_NONE (or none recorded).
    let aead = p36h_ike_builder(&mut engine);
    call(
        &mut engine,
        aead,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(20), Value::Int(256)],
    );
    call(
        &mut engine,
        aead,
        "addIntegrityAlgorithm",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12)],
    );
    call(
        &mut engine,
        aead,
        "addPseudorandomFunction",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        aead,
        "addDhGroup",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(14)],
    );
    assert_throws(
        engine.invoke_instance(
            aead,
            "build",
            "()Landroid/net/ipsec/ike/IkeSaProposal;",
            &[],
        ),
        P36H_IAE,
    );
    // DH_GROUP_NONE is invalid for IKE proposals.
    let dh_none = p36h_ike_builder(&mut engine);
    call(
        &mut engine,
        dh_none,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(20), Value::Int(128)],
    );
    call(
        &mut engine,
        dh_none,
        "addPseudorandomFunction",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        dh_none,
        "addDhGroup",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(0)],
    );
    assert_throws(
        engine.invoke_instance(
            dh_none,
            "build",
            "()Landroid/net/ipsec/ike/IkeSaProposal;",
            &[],
        ),
        P36H_IAE,
    );

    // Child proposals: 3DES has no IPsec transform; AEAD child needs no DH.
    let child = p49_new(&mut engine, P36H_CHILD_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            child,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/ChildSaProposal$Builder;",
            &[Value::Int(3), Value::Int(0)],
        ),
        P36H_IAE,
    );
    let child_proposal = p36h_child_proposal(&mut engine);
    let child_enc = call(
        &mut engine,
        child_proposal,
        "getEncryptionAlgorithms",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, child_enc), Value::Int(1));

    // Supported-algorithm sets and string forms.
    let ike_supported = call_static(
        &mut engine,
        P36H_IKE_PROPOSAL,
        "getSupportedEncryptionAlgorithms",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, ike_supported), Value::Int(7));
    let child_supported = call_static(
        &mut engine,
        P36H_CHILD_PROPOSAL,
        "getSupportedEncryptionAlgorithms",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, child_supported), Value::Int(6));
    let dh_supported = call_static(
        &mut engine,
        P36H_SA_PROPOSAL,
        "getSupportedDhGroups",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, dh_supported), Value::Int(7));
    assert_eq!(
        call_static_text(
            &mut engine,
            P36H_SA_PROPOSAL,
            "getEncryptionAlgorithmString",
            "(I)Ljava/lang/String;",
            &[Value::Int(12)]
        ),
        "ENCR_AES_CBC"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36H_SA_PROPOSAL,
            "getEncryptionAlgorithmString",
            "(I)Ljava/lang/String;",
            &[Value::Int(77)]
        ),
        "ENC_Unknown_77"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36H_SA_PROPOSAL,
            "getDhGroupString",
            "(I)Ljava/lang/String;",
            &[Value::Int(31)]
        ),
        "DH_GROUP_CURVE_25519"
    );

    // Deep audit: the Child builder rejects IKE-valid ciphers that the
    // IPsec stack does not support (3DES), while wholly unknown algorithm
    // ids fail transform validation; both surface as IAE at add time.
    let child_rejecting = p49_new(&mut engine, P36H_CHILD_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            child_rejecting,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/ChildSaProposal$Builder;",
            &[Value::Int(3), Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_throws(
        engine.invoke_instance(
            child_rejecting,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/ChildSaProposal$Builder;",
            &[Value::Int(999), Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
}

#[test]
fn ike_traffic_selectors_and_identifications_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    assert!(engine
        .cp
        .is_subtype("Ljava/net/Inet4Address;", "Ljava/net/InetAddress;"));
    let v4_start = instance(&mut engine, "Ljava/net/Inet4Address;");
    let v4_end = instance(&mut engine, "Ljava/net/Inet4Address;");
    let ts = p49_new(
        &mut engine,
        P36H_TS,
        "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
        &[
            Value::Int(0),
            Value::Int(65535),
            Value::Obj(v4_start),
            Value::Obj(v4_end),
        ],
    );
    assert_eq!(field(&engine, ts, "tsType"), Value::Int(7));
    assert_eq!(field(&engine, ts, "selectorLength"), Value::Int(16));
    assert_eq!(field(&engine, ts, "startPort"), Value::Int(0));
    assert_eq!(field(&engine, ts, "endPort"), Value::Int(65535));

    let bad_ports = instance(&mut engine, P36H_TS);
    assert_throws(
        engine.invoke_instance(
            bad_ports,
            "<init>",
            "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
            &[
                Value::Int(100),
                Value::Int(50),
                Value::Obj(v4_start),
                Value::Obj(v4_end),
            ],
        ),
        P36H_IAE,
    );
    let v6_start = instance(&mut engine, "Ljava/net/Inet6Address;");
    let v6_end = instance(&mut engine, "Ljava/net/Inet6Address;");
    let mixed = instance(&mut engine, P36H_TS);
    assert_throws(
        engine.invoke_instance(
            mixed,
            "<init>",
            "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
            &[
                Value::Int(0),
                Value::Int(10),
                Value::Obj(v4_start),
                Value::Obj(v6_start),
            ],
        ),
        P36H_IAE,
    );
    let v6_ts = p49_new(
        &mut engine,
        P36H_TS,
        "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
        &[
            Value::Int(443),
            Value::Int(443),
            Value::Obj(v6_start),
            Value::Obj(v6_end),
        ],
    );
    assert_eq!(field(&engine, v6_ts, "tsType"), Value::Int(8));
    assert_eq!(field(&engine, v6_ts, "selectorLength"), Value::Int(40));

    let ts_copy = p49_new(
        &mut engine,
        P36H_TS,
        "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
        &[
            Value::Int(0),
            Value::Int(65535),
            Value::Obj(v4_start),
            Value::Obj(v4_end),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            ts,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(ts_copy)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            ts,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(v6_ts)]
        ),
        Value::Int(0)
    );

    // Identifications.
    let fqdn_name = text(&mut engine, "vpn.example.com");
    let fqdn = p49_new(
        &mut engine,
        P36H_ID_FQDN,
        "(Ljava/lang/String;)V",
        &[fqdn_name],
    );
    assert_eq!(
        text_of(&engine, &field(&engine, fqdn, "fqdn")),
        "vpn.example.com"
    );
    assert_eq!(field(&engine, fqdn, "idType"), Value::Int(2));
    let unicode_name = text(&mut engine, "exämple.com");
    let bad_fqdn = instance(&mut engine, P36H_ID_FQDN);
    assert_throws(
        engine.invoke_instance(bad_fqdn, "<init>", "(Ljava/lang/String;)V", &[unicode_name]),
        P36H_IAE,
    );
    let fqdn_bytes = byte_array(&mut engine, b"ike.example.org");
    let fqdn2 = p49_new(&mut engine, P36H_ID_FQDN, "([B)V", &[fqdn_bytes]);
    assert_eq!(
        text_of(&engine, &field(&engine, fqdn2, "fqdn")),
        "ike.example.org"
    );

    let key_bytes = byte_array(&mut engine, &[1, 2, 3]);
    let key_id = p49_new(&mut engine, P36H_ID_KEYID, "([B)V", &[key_bytes]);
    assert_eq!(field(&engine, key_id, "idType"), Value::Int(11));
    assert_eq!(
        byte_array_values(&engine, &field(&engine, key_id, "keyId")),
        vec![1, 2, 3]
    );
    let key_bytes2 = byte_array(&mut engine, &[1, 2, 3]);
    let key_id2 = p49_new(&mut engine, P36H_ID_KEYID, "([B)V", &[key_bytes2]);
    assert_eq!(
        call(
            &mut engine,
            key_id,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(key_id2)]
        ),
        Value::Int(1)
    );

    let mail = text(&mut engine, "user@example.com");
    let rfc822 = p49_new(
        &mut engine,
        P36H_ID_RFC822,
        "(Ljava/lang/String;)V",
        &[mail],
    );
    assert_eq!(
        text_of(&engine, &field(&engine, rfc822, "rfc822Name")),
        "user@example.com"
    );
    let ipv4_id = p49_new(
        &mut engine,
        P36H_ID_IPV4,
        "(Ljava/net/Inet4Address;)V",
        &[Value::Obj(v4_start)],
    );
    assert_eq!(field(&engine, ipv4_id, "ipv4Address"), Value::Obj(v4_start));
}

#[test]
fn child_session_params_builders_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let proposal = p36h_child_proposal(&mut engine);

    let builder = p49_new(&mut engine, P36H_TRANSPORT_BUILDER, "()V", &[]);
    assert_throws(
        engine.invoke_instance(
            builder,
            "build",
            "()Landroid/net/ipsec/ike/TransportModeChildSessionParams;",
            &[],
        ),
        P36H_IAE,
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "addChildSaProposal",
            "(Landroid/net/ipsec/ike/ChildSaProposal;)Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;",
            &[Value::Null],
        ),
        P36H_NPE,
    );
    call(
        &mut engine,
        builder,
        "addChildSaProposal",
        "(Landroid/net/ipsec/ike/ChildSaProposal;)Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;",
        &[Value::Obj(proposal)],
    );
    assert_throws(
        engine.invoke_instance(
            builder,
            "setLifetimeSeconds",
            "(II)Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;",
            &[Value::Int(100), Value::Int(50)],
        ),
        P36H_IAE,
    );
    call(
        &mut engine,
        builder,
        "setLifetimeSeconds",
        "(II)Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;",
        &[Value::Int(3600), Value::Int(1800)],
    );
    let params = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/net/ipsec/ike/TransportModeChildSessionParams;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, params, "getHardLifetimeSeconds", "()I", &[]),
        Value::Int(3600)
    );
    assert_eq!(
        call(&mut engine, params, "getSoftLifetimeSeconds", "()I", &[]),
        Value::Int(1800)
    );
    let proposals = call(
        &mut engine,
        params,
        "getChildSaProposals",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, proposals), Value::Int(1));
    // Default traffic selectors (IPv4 + IPv6 full ranges) were inserted.
    let inbound = call(
        &mut engine,
        params,
        "getInboundTrafficSelectors",
        "()Ljava/util/List;",
        &[],
    );
    let inbound0 = object(p49_list_get(&mut engine, inbound.clone(), 0));
    assert_eq!(field(&engine, inbound0, "tsType"), Value::Int(7));
    let inbound1 = object(p49_list_get(&mut engine, inbound, 1));
    assert_eq!(field(&engine, inbound1, "tsType"), Value::Int(8));

    // Tunnel builder keeps an explicitly configured selector and defaults.
    let v4_start = instance(&mut engine, "Ljava/net/Inet4Address;");
    let v4_end = instance(&mut engine, "Ljava/net/Inet4Address;");
    let selector = p49_new(
        &mut engine,
        P36H_TS,
        "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
        &[
            Value::Int(443),
            Value::Int(443),
            Value::Obj(v4_start),
            Value::Obj(v4_end),
        ],
    );
    let tunnel = p49_new(&mut engine, P36H_TUNNEL_BUILDER, "()V", &[]);
    call(
        &mut engine,
        tunnel,
        "addChildSaProposal",
        "(Landroid/net/ipsec/ike/ChildSaProposal;)Landroid/net/ipsec/ike/TunnelModeChildSessionParams$Builder;",
        &[Value::Obj(proposal)],
    );
    call(
        &mut engine,
        tunnel,
        "addInboundTrafficSelectors",
        "(Landroid/net/ipsec/ike/IkeTrafficSelector;)Landroid/net/ipsec/ike/TunnelModeChildSessionParams$Builder;",
        &[Value::Obj(selector)],
    );
    let tunnel_params = object(call(
        &mut engine,
        tunnel,
        "build",
        "()Landroid/net/ipsec/ike/TunnelModeChildSessionParams;",
        &[],
    ));
    let tunnel_in = call(
        &mut engine,
        tunnel_params,
        "getInboundTrafficSelectors",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, tunnel_in), Value::Int(1));
    assert_eq!(
        call(
            &mut engine,
            tunnel_params,
            "getHardLifetimeSeconds",
            "()I",
            &[]
        ),
        Value::Int(7200)
    );
    assert_eq!(
        call(
            &mut engine,
            tunnel_params,
            "getSoftLifetimeSeconds",
            "()I",
            &[]
        ),
        Value::Int(3600)
    );

    // Copy constructor carries proposals and lifetimes.
    let copied_builder = p49_new(
        &mut engine,
        P36H_TRANSPORT_BUILDER,
        "(Landroid/net/ipsec/ike/TransportModeChildSessionParams;)V",
        &[Value::Obj(params)],
    );
    let copied = object(call(
        &mut engine,
        copied_builder,
        "build",
        "()Landroid/net/ipsec/ike/TransportModeChildSessionParams;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, copied, "getHardLifetimeSeconds", "()I", &[]),
        Value::Int(3600)
    );
    let copied_proposals = call(
        &mut engine,
        copied,
        "getChildSaProposals",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, copied_proposals), Value::Int(1));
}

// ---- Appendix C Batch 36-I: telephony + mbms + cellbroadcast + telecom ----

const P36I_PHONE_UTILS: &str = "Landroid/telephony/PhoneNumberUtils;";
const P36I_TM: &str = "Landroid/telephony/TelephonyManager;";
const P36I_SM: &str = "Landroid/telephony/SubscriptionManager;";
const P36I_SERVICE_STATE: &str = "Landroid/telephony/ServiceState;";
const P36I_CB_RANGE: &str = "Landroid/telephony/CellBroadcastIdRange;";
const P36I_DOWNLOAD_REQ: &str = "Landroid/telephony/mbms/DownloadRequest;";
const P36I_DOWNLOAD_BUILDER: &str = "Landroid/telephony/mbms/DownloadRequest$Builder;";
const P36I_FILE_INFO: &str = "Landroid/telephony/mbms/FileInfo;";
const P36I_HANDLE: &str = "Landroid/telecom/PhoneAccountHandle;";
const P36I_ACCOUNT_BUILDER: &str = "Landroid/telecom/PhoneAccount$Builder;";
const P36I_CAUSE: &str = "Landroid/telecom/DisconnectCause;";
const P36I_AUDIO_STATE: &str = "Landroid/telecom/CallAudioState;";
const P36I_VIDEO: &str = "Landroid/telecom/VideoProfile;";
const P36I_CAPS: &str = "Landroid/telecom/VideoProfile$CameraCapabilities;";
const P36I_HINTS: &str = "Landroid/telecom/StatusHints;";
const P36I_COMPONENT: &str = "Landroid/content/ComponentName;";

fn p36i_system_service(engine: &mut Engine, name: &str) -> ObjRef {
    let context = instance(engine, CONTEXT);
    let service_name = text(engine, name);
    object(call(
        engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ))
}

fn p36i_uri(engine: &mut Engine, uri: &str) -> Value {
    let text_value = text(engine, uri);
    call_static(
        engine,
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[text_value],
    )
}

fn p36i_component(engine: &mut Engine, pkg: &str, cls: &str) -> ObjRef {
    let pkg_value = text(engine, pkg);
    let cls_value = text(engine, cls);
    p49_new(
        engine,
        P36I_COMPONENT,
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[pkg_value, cls_value],
    )
}

#[test]
fn telephony_no_radio_profile_and_phone_number_utils_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let tm = p36i_system_service(&mut engine, "telephony");
    assert_eq!(
        call(&mut engine, tm, "getSimState", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, tm, "getCallState", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, tm, "getDataState", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, tm, "getDataActivity", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, tm, "hasIccCard", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, tm, "getNetworkType", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            tm,
            "getDataNetworkType",
            "(I)I",
            &[Value::Int(3)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            tm,
            "getSimOperator",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    assert_eq!(
        call_text(
            &mut engine,
            tm,
            "getNetworkOperatorName",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );
    assert_eq!(
        call(&mut engine, tm, "isNetworkRoaming", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_TM,
            "getSimStateForSlotIndex",
            "(I)I",
            &[Value::Int(0)]
        ),
        Value::Int(0)
    );

    let sm = p36i_system_service(&mut engine, "telephony_subscription_service");
    assert_eq!(
        call(
            &mut engine,
            sm,
            "getActiveSubscriptionInfoList",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );
    let complete = call(
        &mut engine,
        sm,
        "getCompleteActiveSubscriptionInfoList",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, complete), Value::Int(0));
    assert_eq!(
        call(
            &mut engine,
            sm,
            "getActiveSubscriptionInfoCount",
            "()I",
            &[]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(&mut engine, P36I_SM, "getDefaultSubscriptionId", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "getDefaultVoiceSubscriptionId",
            "()I",
            &[]
        ),
        Value::Int(-1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "isValidSubscriptionId",
            "(I)Z",
            &[Value::Int(0)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "isValidSubscriptionId",
            "(I)Z",
            &[Value::Int(-1)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "isUsableSubscriptionId",
            "(I)Z",
            &[Value::Int(i32::MAX)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "isUsableSubscriptionId",
            "(I)Z",
            &[Value::Int(5)]
        ),
        Value::Int(1)
    );

    // PhoneNumberUtils pure string algorithms.
    let keypad = |engine: &mut Engine, input: &str| {
        let value = text(engine, input);
        call_static_text(
            engine,
            P36I_PHONE_UTILS,
            "convertKeypadLettersToDigits",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value],
        )
    };
    assert_eq!(keypad(&mut engine, "1-800-GOOG-411"), "1-800-4664-411");
    let value = text(&mut engine, "1-800-GOOG-411");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "convertAndStrip",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        "18004664411"
    );
    let value = text(&mut engine, "+1 (555) 010-1234,99");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "extractNetworkPortion",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        "+15550101234"
    );
    let value = text(&mut engine, "*31#+123");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "extractNetworkPortion",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        "*31#+123"
    );
    let value = text(&mut engine, "++123+45");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "extractNetworkPortionAlt",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        "+12345"
    );
    let value = text(&mut engine, "555,12;34");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "extractPostDialPortion",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        ",12;34"
    );
    let value = text(&mut engine, "5551234");
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "extractPostDialPortion",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[value]
        ),
        ""
    );
    let sms = |engine: &mut Engine, input: &str| {
        let value = text(engine, input);
        call_static(
            engine,
            P36I_PHONE_UTILS,
            "isWellFormedSmsAddress",
            "(Ljava/lang/String;)Z",
            &[value],
        )
    };
    assert_eq!(sms(&mut engine, "+1555"), Value::Int(1));
    assert_eq!(sms(&mut engine, "+"), Value::Int(0));
    assert_eq!(sms(&mut engine, ""), Value::Int(0));
    let global = |engine: &mut Engine, input: Value| {
        call_static(
            engine,
            P36I_PHONE_UTILS,
            "isGlobalPhoneNumber",
            "(Ljava/lang/String;)Z",
            &[input],
        )
    };
    let v = text(&mut engine, "+1.555-010");
    assert_eq!(global(&mut engine, v), Value::Int(1));
    let v = text(&mut engine, "555-12a");
    assert_eq!(global(&mut engine, v), Value::Int(0));
    assert_eq!(global(&mut engine, Value::Null), Value::Int(0));
    assert_eq!(
        call_static(
            &mut engine,
            P36I_PHONE_UTILS,
            "isDialable",
            "(C)Z",
            &[Value::Int(78)]
        ),
        Value::Int(1) // 'N' is the WILD character
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_PHONE_UTILS,
            "isReallyDialable",
            "(C)Z",
            &[Value::Int(78)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_PHONE_UTILS,
            "isStartsPostDial",
            "(C)Z",
            &[Value::Int(59)]
        ),
        Value::Int(1) // ';' is WAIT
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_PHONE_UTILS,
            "isISODigit",
            "(C)Z",
            &[Value::Int(53)]
        ),
        Value::Int(1)
    );
}

#[test]
fn service_state_and_cell_broadcast_id_range_values_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let state = p49_new(&mut engine, P36I_SERVICE_STATE, "()V", &[]);
    assert_eq!(
        call(&mut engine, state, "getState", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, state, "getDataRegState", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, state, "getRoaming", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            state,
            "getOperatorAlphaLong",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, state, "getChannelNumber", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, state, "isEmergencyOnly", "()Z", &[]),
        Value::Int(0)
    );
    let bandwidths = call(&mut engine, state, "getCellBandwidths", "()[I", &[]);
    assert_eq!(array_values(&engine, &bandwidths).len(), 0);

    let long = text(&mut engine, "Rusty Mobile");
    let short = text(&mut engine, "Rusty");
    let numeric = text(&mut engine, "72499");
    call(
        &mut engine,
        state,
        "setOperatorName",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[long, short, numeric],
    );
    assert_eq!(
        call_text(
            &mut engine,
            state,
            "getOperatorNumeric",
            "()Ljava/lang/String;",
            &[]
        ),
        "72499"
    );
    call(
        &mut engine,
        state,
        "setVoiceRegState",
        "(I)V",
        &[Value::Int(0)],
    );
    assert_eq!(
        call(&mut engine, state, "getState", "()I", &[]),
        Value::Int(0)
    );
    call(&mut engine, state, "setRoaming", "(Z)V", &[Value::Int(1)]);
    assert_eq!(
        call(&mut engine, state, "getRoaming", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, state, "getVoiceRoamingType", "()I", &[]),
        Value::Int(1)
    );
    let copy = p49_new(
        &mut engine,
        P36I_SERVICE_STATE,
        "(Landroid/telephony/ServiceState;)V",
        &[Value::Obj(state)],
    );
    assert_eq!(
        call_text(
            &mut engine,
            copy,
            "getOperatorAlphaShort",
            "()Ljava/lang/String;",
            &[]
        ),
        "Rusty"
    );
    call(&mut engine, state, "setStateOutOfService", "()V", &[]);
    assert_eq!(
        call(&mut engine, state, "getState", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, state, "getChannelNumber", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(
            &mut engine,
            state,
            "getOperatorAlphaLong",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
    call(&mut engine, state, "setStateOff", "()V", &[]);
    assert_eq!(
        call(&mut engine, state, "getState", "()I", &[]),
        Value::Int(3)
    );

    // CellBroadcastIdRange (android.telephony).
    let range = p49_new(
        &mut engine,
        P36I_CB_RANGE,
        "(IIIZ)V",
        &[
            Value::Int(4370),
            Value::Int(4399),
            Value::Int(1),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(&mut engine, range, "getStartId", "()I", &[]),
        Value::Int(4370)
    );
    assert_eq!(
        call(&mut engine, range, "getEndId", "()I", &[]),
        Value::Int(4399)
    );
    assert_eq!(
        call(&mut engine, range, "isEnabled", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_text(&mut engine, range, "toString", "()Ljava/lang/String;", &[]),
        "CellBroadcastIdRange[4370, 4399, 1, true]"
    );
    let same = p49_new(
        &mut engine,
        P36I_CB_RANGE,
        "(IIIZ)V",
        &[
            Value::Int(4370),
            Value::Int(4399),
            Value::Int(1),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call(
            &mut engine,
            range,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(same)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, range, "hashCode", "()I", &[]),
        call(&mut engine, same, "hashCode", "()I", &[])
    );
    let bad = instance(&mut engine, P36I_CB_RANGE);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            "(IIIZ)V",
            &[Value::Int(-1), Value::Int(5), Value::Int(1), Value::Int(1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let bad = instance(&mut engine, P36I_CB_RANGE);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            "(IIIZ)V",
            &[
                Value::Int(100),
                Value::Int(99),
                Value::Int(1),
                Value::Int(0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let bad = instance(&mut engine, P36I_CB_RANGE);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            "(IIIZ)V",
            &[
                Value::Int(0),
                Value::Int(65536),
                Value::Int(1),
                Value::Int(0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    // Boundary: a single-id range at the top of the 16-bit id space is
    // valid, and equality is exact-class/field based only.
    let single = p49_new(
        &mut engine,
        P36I_CB_RANGE,
        "(IIIZ)V",
        &[
            Value::Int(65535),
            Value::Int(65535),
            Value::Int(2),
            Value::Int(0),
        ],
    );
    assert_eq!(
        call(&mut engine, single, "getType", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call_text(&mut engine, single, "toString", "()Ljava/lang/String;", &[]),
        "CellBroadcastIdRange[65535, 65535, 2, false]"
    );
    assert_eq!(
        call(
            &mut engine,
            range,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Null],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            range,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(single)],
        ),
        Value::Int(0)
    );
}

#[test]
fn mbms_download_request_and_file_info_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let source = p36i_uri(&mut engine, "http://example.com/file.bin");
    let dest = p36i_uri(&mut engine, "file:///data/local/file.bin");

    let bad = instance(&mut engine, P36I_DOWNLOAD_BUILDER);
    assert_throws(
        engine.invoke_instance(
            bad,
            "<init>",
            "(Landroid/net/Uri;Landroid/net/Uri;)V",
            &[Value::Null, dest.clone()],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let builder = p49_new(
        &mut engine,
        P36I_DOWNLOAD_BUILDER,
        "(Landroid/net/Uri;Landroid/net/Uri;)V",
        &[source.clone(), dest.clone()],
    );
    let service_id = text(&mut engine, "svc-1");
    call(
        &mut engine,
        builder,
        "setServiceId",
        "(Ljava/lang/String;)Landroid/telephony/mbms/DownloadRequest$Builder;",
        &[service_id],
    );
    call(
        &mut engine,
        builder,
        "setSubscriptionId",
        "(I)Landroid/telephony/mbms/DownloadRequest$Builder;",
        &[Value::Int(7)],
    );
    let request = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            request,
            "getFileServiceId",
            "()Ljava/lang/String;",
            &[]
        ),
        "svc-1"
    );
    assert_eq!(
        call(&mut engine, request, "getSubscriptionId", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(
            &mut engine,
            request,
            "getSourceUri",
            "()Landroid/net/Uri;",
            &[]
        ),
        source
    );
    assert_eq!(
        call(&mut engine, request, "getVersion", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_DOWNLOAD_REQ,
            "getMaxAppIntentSize",
            "()I",
            &[]
        ),
        Value::Int(50000)
    );

    let copied_builder = object(call_static(
        &mut engine,
        P36I_DOWNLOAD_BUILDER,
        "fromDownloadRequest",
        "(Landroid/telephony/mbms/DownloadRequest;)Landroid/telephony/mbms/DownloadRequest$Builder;",
        &[Value::Obj(request)],
    ));
    let copied = object(call(
        &mut engine,
        copied_builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            request,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(copied)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, request, "hashCode", "()I", &[]),
        call(&mut engine, copied, "hashCode", "()I", &[])
    );

    let mime = text(&mut engine, "video/mp4");
    let info = p49_new(
        &mut engine,
        P36I_FILE_INFO,
        "(Landroid/net/Uri;Ljava/lang/String;)V",
        &[source.clone(), mime],
    );
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getMimeType",
            "()Ljava/lang/String;",
            &[]
        ),
        "video/mp4"
    );
    assert_eq!(
        call(&mut engine, info, "getUri", "()Landroid/net/Uri;", &[]),
        source
    );
    let mime2 = text(&mut engine, "audio/mpeg");
    let other_info = p49_new(
        &mut engine,
        P36I_FILE_INFO,
        "(Landroid/net/Uri;Ljava/lang/String;)V",
        &[source.clone(), mime2],
    );
    assert_eq!(
        call(
            &mut engine,
            info,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(other_info)]
        ),
        Value::Int(0)
    );
    // FileInfo stores its fields unchecked; two all-null instances are
    // equal through Objects.equals semantics.
    let null_info = p49_new(
        &mut engine,
        P36I_FILE_INFO,
        "(Landroid/net/Uri;Ljava/lang/String;)V",
        &[Value::Null, Value::Null],
    );
    let null_info2 = p49_new(
        &mut engine,
        P36I_FILE_INFO,
        "(Landroid/net/Uri;Ljava/lang/String;)V",
        &[Value::Null, Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            null_info,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(null_info2)]
        ),
        Value::Int(1)
    );

    // The app intent round-trips through Intent.toUri/parseUri.
    let done_action = text(&mut engine, "com.example.DOWNLOAD_DONE");
    let done_intent = p49_new(
        &mut engine,
        "Landroid/content/Intent;",
        "(Ljava/lang/String;)V",
        &[done_action],
    );
    let intent_builder = p49_new(
        &mut engine,
        P36I_DOWNLOAD_BUILDER,
        "(Landroid/net/Uri;Landroid/net/Uri;)V",
        &[source.clone(), dest.clone()],
    );
    call(
        &mut engine,
        intent_builder,
        "setAppIntent",
        "(Landroid/content/Intent;)Landroid/telephony/mbms/DownloadRequest$Builder;",
        &[Value::Obj(done_intent)],
    );
    let with_intent = object(call(
        &mut engine,
        intent_builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    let restored = object(call(
        &mut engine,
        with_intent,
        "getIntentForApp",
        "()Landroid/content/Intent;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            restored,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.DOWNLOAD_DONE"
    );

    // AOSP assigns the serialized intent before validating its length, so
    // the oversized value remains in the builder when setAppIntent throws
    // and is carried into the next build().
    let long_action = "a".repeat(50_001);
    let long_action_value = text(&mut engine, &long_action);
    let big_intent = p49_new(
        &mut engine,
        "Landroid/content/Intent;",
        "(Ljava/lang/String;)V",
        &[long_action_value],
    );
    let oversized_builder = p49_new(
        &mut engine,
        P36I_DOWNLOAD_BUILDER,
        "(Landroid/net/Uri;Landroid/net/Uri;)V",
        &[source, dest],
    );
    assert_throws(
        engine.invoke_instance(
            oversized_builder,
            "setAppIntent",
            "(Landroid/content/Intent;)Landroid/telephony/mbms/DownloadRequest$Builder;",
            &[Value::Obj(big_intent)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let retained = object(call(
        &mut engine,
        oversized_builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    let retained_intent = object(call(
        &mut engine,
        retained,
        "getIntentForApp",
        "()Landroid/content/Intent;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            retained_intent,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        long_action
    );
}

#[test]
fn telecom_values_and_no_service_manager_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let telecom = p36i_system_service(&mut engine, "telecom");
    assert_eq!(
        call(&mut engine, telecom, "getCallState", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, telecom, "isInCall", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, telecom, "isRinging", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            telecom,
            "getDefaultDialerPackage",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );

    // ComponentName value model (needed by PhoneAccountHandle).
    let component = p36i_component(
        &mut engine,
        "com.example.dialer",
        "com.example.dialer.ConnService",
    );
    assert_eq!(
        call_text(
            &mut engine,
            component,
            "getPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.dialer"
    );
    assert_eq!(
        call_text(
            &mut engine,
            component,
            "getShortClassName",
            "()Ljava/lang/String;",
            &[]
        ),
        ".ConnService"
    );
    assert_eq!(
        call_text(
            &mut engine,
            component,
            "flattenToShortString",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.dialer/.ConnService"
    );
    let flat = text(&mut engine, "com.example.dialer/.ConnService");
    let unflattened = object(call_static(
        &mut engine,
        P36I_COMPONENT,
        "unflattenFromString",
        "(Ljava/lang/String;)Landroid/content/ComponentName;",
        &[flat],
    ));
    assert_eq!(
        call(
            &mut engine,
            component,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(unflattened)]
        ),
        Value::Int(1)
    );
    let no_sep = text(&mut engine, "no-separator");
    assert_eq!(
        call_static(
            &mut engine,
            P36I_COMPONENT,
            "unflattenFromString",
            "(Ljava/lang/String;)Landroid/content/ComponentName;",
            &[no_sep]
        ),
        Value::Null
    );
    // The existing ComponentName host model (jtext_format) treats a
    // non-String first argument as the Context overload, so only the
    // null class name is an NPE here.
    let bad_component = instance(&mut engine, P36I_COMPONENT);
    let some_pkg = text(&mut engine, "com.example");
    assert_throws(
        engine.invoke_instance(
            bad_component,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[some_pkg, Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // PhoneAccountHandle + PhoneAccount.
    let user = instance(&mut engine, "Landroid/os/UserHandle;");
    let id = text(&mut engine, "acc-1");
    let handle = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;Landroid/os/UserHandle;)V",
        &[Value::Obj(component), id, Value::Obj(user)],
    );
    assert_eq!(
        call_text(&mut engine, handle, "getId", "()Ljava/lang/String;", &[]),
        "acc-1"
    );
    let id2 = text(&mut engine, "acc-1");
    let handle2 = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;Landroid/os/UserHandle;)V",
        &[Value::Obj(unflattened), id2, Value::Obj(user)],
    );
    assert_eq!(
        call(
            &mut engine,
            handle,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(handle2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_HANDLE,
            "areFromSamePackage",
            "(Landroid/telecom/PhoneAccountHandle;Landroid/telecom/PhoneAccountHandle;)Z",
            &[Value::Obj(handle), Value::Obj(handle2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_HANDLE,
            "areFromSamePackage",
            "(Landroid/telecom/PhoneAccountHandle;Landroid/telecom/PhoneAccountHandle;)Z",
            &[Value::Null, Value::Null]
        ),
        Value::Int(1)
    );

    let label = text(&mut engine, "Example");
    let builder = p49_new(
        &mut engine,
        P36I_ACCOUNT_BUILDER,
        "(Landroid/telecom/PhoneAccountHandle;Ljava/lang/CharSequence;)V",
        &[Value::Obj(handle), label],
    );
    let account = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/telecom/PhoneAccount;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            account,
            "getLabel",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "Example"
    );
    let schemes = call(
        &mut engine,
        account,
        "getSupportedUriSchemes",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, schemes), Value::Int(1));
    let tel = text(&mut engine, "tel");
    assert_eq!(
        call(
            &mut engine,
            account,
            "supportsUriScheme",
            "(Ljava/lang/String;)Z",
            &[tel]
        ),
        Value::Int(1)
    );
    let sip = text(&mut engine, "sip");
    assert_eq!(
        call(
            &mut engine,
            account,
            "supportsUriScheme",
            "(Ljava/lang/String;)Z",
            &[sip]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, account, "getSupportedAudioRoutes", "()I", &[]),
        Value::Int(31)
    );
    assert_eq!(
        call(&mut engine, account, "isEnabled", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call_text(
            &mut engine,
            account,
            "getGroupId",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );

    let label2 = text(&mut engine, "Second");
    let builder2 = p49_new(
        &mut engine,
        P36I_ACCOUNT_BUILDER,
        "(Landroid/telecom/PhoneAccountHandle;Ljava/lang/CharSequence;)V",
        &[Value::Obj(handle), label2],
    );
    call(
        &mut engine,
        builder2,
        "setCapabilities",
        "(I)Landroid/telecom/PhoneAccount$Builder;",
        &[Value::Int(0x802)],
    );
    let sip1 = text(&mut engine, "sip");
    call(
        &mut engine,
        builder2,
        "addSupportedUriScheme",
        "(Ljava/lang/String;)Landroid/telecom/PhoneAccount$Builder;",
        &[sip1],
    );
    let sip2 = text(&mut engine, "sip");
    call(
        &mut engine,
        builder2,
        "addSupportedUriScheme",
        "(Ljava/lang/String;)Landroid/telecom/PhoneAccount$Builder;",
        &[sip2],
    );
    let account2 = object(call(
        &mut engine,
        builder2,
        "build",
        "()Landroid/telecom/PhoneAccount;",
        &[],
    ));
    let schemes2 = call(
        &mut engine,
        account2,
        "getSupportedUriSchemes",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, schemes2), Value::Int(1));
    let rebuilt_builder = object(call(
        &mut engine,
        account2,
        "toBuilder",
        "()Landroid/telecom/PhoneAccount$Builder;",
        &[],
    ));
    let rebuilt = object(call(
        &mut engine,
        rebuilt_builder,
        "build",
        "()Landroid/telecom/PhoneAccount;",
        &[],
    ));
    assert_eq!(
        call(&mut engine, rebuilt, "getCapabilities", "()I", &[]),
        Value::Int(0x802)
    );

    // DisconnectCause.
    let busy = p49_new(&mut engine, P36I_CAUSE, "(I)V", &[Value::Int(7)]);
    assert_eq!(
        call(&mut engine, busy, "getCode", "()I", &[]),
        Value::Int(7)
    );
    assert_eq!(
        call(&mut engine, busy, "getTone", "()I", &[]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut engine, busy, "getTelephonyDisconnectCause", "()I", &[]),
        Value::Int(36)
    );
    assert_eq!(
        call(
            &mut engine,
            busy,
            "getTelephonyPreciseDisconnectCause",
            "()I",
            &[]
        ),
        Value::Int(65535)
    );
    assert_eq!(
        call_text(&mut engine, busy, "toString", "()Ljava/lang/String;", &[]),
        "DisconnectCause [ Code: (BUSY) Label: () Description: () Reason: () Tone: (-1) TelephonyCause: 36/65535 ImsReasonInfo: null]"
    );
    let invalid = p49_new(&mut engine, P36I_CAUSE, "(I)V", &[Value::Int(99)]);
    assert_eq!(
        call_text(&mut engine, invalid, "toString", "()Ljava/lang/String;", &[]),
        "DisconnectCause [ Code: (invalid code: 99) Label: () Description: () Reason: () Tone: (-1) TelephonyCause: 36/65535 ImsReasonInfo: null]"
    );
    let reason = text(&mut engine, "REASON_TEST");
    let with_reason = p49_new(
        &mut engine,
        P36I_CAUSE,
        "(ILjava/lang/String;)V",
        &[Value::Int(1), reason],
    );
    assert_eq!(
        call_text(
            &mut engine,
            with_reason,
            "getReason",
            "()Ljava/lang/String;",
            &[]
        ),
        "REASON_TEST"
    );

    // CallAudioState + VideoProfile + StatusHints.
    let audio = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(ZII)V",
        &[Value::Int(0), Value::Int(8), Value::Int(31)],
    );
    assert_eq!(
        call(&mut engine, audio, "getRoute", "()I", &[]),
        Value::Int(8)
    );
    assert_eq!(
        call(&mut engine, audio, "getSupportedRouteMask", "()I", &[]),
        Value::Int(31)
    );
    assert_eq!(
        call_text(&mut engine, audio, "toString", "()Ljava/lang/String;", &[]),
        "[AudioState isMuted: false, route: SPEAKER, supportedRouteMask: EARPIECE, BLUETOOTH, WIRED_HEADSET, SPEAKER, STREAMING, activeBluetoothDevice: [null], supportedBluetoothDevices: []]"
    );
    let streaming = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(ZII)V",
        &[Value::Int(1), Value::Int(16), Value::Int(31)],
    );
    assert_eq!(
        call(&mut engine, streaming, "getSupportedRouteMask", "()I", &[]),
        Value::Int(16)
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_AUDIO_STATE,
            "audioRouteToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(9)]
        ),
        "EARPIECE, SPEAKER"
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_AUDIO_STATE,
            "audioRouteToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(32)]
        ),
        "UNKNOWN"
    );
    let audio_copy = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(Landroid/telecom/CallAudioState;)V",
        &[Value::Obj(audio)],
    );
    assert_eq!(
        call(
            &mut engine,
            audio,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(audio_copy)]
        ),
        Value::Int(1)
    );

    let video = p49_new(
        &mut engine,
        P36I_VIDEO,
        "(II)V",
        &[Value::Int(3), Value::Int(1)],
    );
    assert_eq!(
        call(&mut engine, video, "getVideoState", "()I", &[]),
        Value::Int(3)
    );
    assert_eq!(
        call_text(&mut engine, video, "toString", "()Ljava/lang/String;", &[]),
        "[VideoProfile videoState = Audio Tx Rx videoQuality = 1]"
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_VIDEO,
            "isBidirectional",
            "(I)Z",
            &[Value::Int(3)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_VIDEO,
            "isAudioOnly",
            "(I)Z",
            &[Value::Int(4)]
        ),
        Value::Int(1) // paused-only has neither TX nor RX
    );
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_VIDEO,
            "videoStateToString",
            "(I)Ljava/lang/String;",
            &[Value::Int(7)]
        ),
        "Audio Tx Rx Pause"
    );
    let caps = p49_new(
        &mut engine,
        P36I_CAPS,
        "(IIZF)V",
        &[
            Value::Int(1920),
            Value::Int(1080),
            Value::Int(1),
            Value::Float(2.5),
        ],
    );
    assert_eq!(
        call(&mut engine, caps, "getWidth", "()I", &[]),
        Value::Int(1920)
    );
    assert_eq!(
        call(&mut engine, caps, "isZoomSupported", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, caps, "getMaxZoom", "()F", &[]),
        Value::Float(2.5)
    );

    let hint_label = text(&mut engine, "In call");
    let hints = p49_new(
        &mut engine,
        P36I_HINTS,
        "(Ljava/lang/CharSequence;Landroid/graphics/drawable/Icon;Landroid/os/Bundle;)V",
        &[hint_label, Value::Null, Value::Null],
    );
    assert_eq!(
        call_text(
            &mut engine,
            hints,
            "getLabel",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        "In call"
    );
    assert_eq!(
        call(
            &mut engine,
            hints,
            "getIcon",
            "()Landroid/graphics/drawable/Icon;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, hints, "getIconResId", "()I", &[]),
        Value::Int(0)
    );
    let shim_pkg = call(
        &mut engine,
        hints,
        "getPackageName",
        "()Landroid/content/ComponentName;",
        &[],
    );
    let shim_component = object(shim_pkg);
    assert_eq!(
        call_text(
            &mut engine,
            shim_component,
            "getPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        ""
    );

    // Deep audit: PhoneAccountHandle.checkParameters only logs — null
    // component, id and user handle are all stored without rejection,
    // and equality/hash tolerate the null fields.
    let null_handle_a = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;Landroid/os/UserHandle;)V",
        &[Value::Null, Value::Null, Value::Null],
    );
    let null_handle_b = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;Landroid/os/UserHandle;)V",
        &[Value::Null, Value::Null, Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            null_handle_a,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(null_handle_b)]
        ),
        Value::Int(1)
    );
    let hash_a = call(&mut engine, null_handle_a, "hashCode", "()I", &[]);
    let hash_b = call(&mut engine, null_handle_b, "hashCode", "()I", &[]);
    assert_eq!(hash_a, hash_b);
    assert_eq!(
        call(
            &mut engine,
            null_handle_a,
            "getId",
            "()Ljava/lang/String;",
            &[]
        ),
        Value::Null
    );
}

#[test]
fn batch36i_source_audit_edge_semantics_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());

    // CallAudioState: equals and the copy constructor use the
    // ROUTE_STREAMING-adjusted supportedRouteMask (AOSP getSupportedRouteMask).
    let streaming_wide = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(ZII)V",
        &[Value::Int(0), Value::Int(16), Value::Int(31)],
    );
    let streaming_narrow = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(ZII)V",
        &[Value::Int(0), Value::Int(16), Value::Int(16)],
    );
    assert_eq!(
        call(
            &mut engine,
            streaming_wide,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(streaming_narrow)]
        ),
        Value::Int(1)
    );
    let streaming_copy = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(Landroid/telecom/CallAudioState;)V",
        &[Value::Obj(streaming_wide)],
    );
    assert_eq!(
        call(
            &mut engine,
            streaming_copy,
            "getSupportedRouteMask",
            "()I",
            &[]
        ),
        Value::Int(16)
    );
    // The 5-argument @TestApi constructor stores explicit bluetooth state.
    let no_devices = call_static(
        &mut engine,
        "Ljava/util/Collections;",
        "emptyList",
        "()Ljava/util/List;",
        &[],
    );
    let five_arg = p49_new(
        &mut engine,
        P36I_AUDIO_STATE,
        "(ZIILandroid/bluetooth/BluetoothDevice;Ljava/util/Collection;)V",
        &[
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Null,
            no_devices,
        ],
    );
    assert_eq!(
        call(&mut engine, five_arg, "isMuted", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, five_arg, "getSupportedRouteMask", "()I", &[]),
        Value::Int(3)
    );

    // VideoProfile.isVideo + ServiceState network-type getters.
    assert_eq!(
        call_static(&mut engine, P36I_VIDEO, "isVideo", "(I)Z", &[Value::Int(4)]),
        Value::Int(0)
    );
    assert_eq!(
        call_static(&mut engine, P36I_VIDEO, "isVideo", "(I)Z", &[Value::Int(2)]),
        Value::Int(1)
    );
    let state = p49_new(&mut engine, P36I_SERVICE_STATE, "()V", &[]);
    assert_eq!(
        call(&mut engine, state, "getDataNetworkType", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, state, "getRilVoiceRadioTechnology", "()I", &[]),
        Value::Int(0)
    );

    // PhoneAccount: capability predicates, enabled mutation, equality, and
    // the AOSP builder/account list aliasing.
    let component = p36i_component(&mut engine, "com.example.voip", "com.example.voip.Svc");
    let user = instance(&mut engine, "Landroid/os/UserHandle;");
    let id = text(&mut engine, "acc-9");
    let handle = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;Landroid/os/UserHandle;)V",
        &[Value::Obj(component), id, Value::Obj(user)],
    );
    let label = text(&mut engine, "VoIP");
    let builder = p49_new(
        &mut engine,
        P36I_ACCOUNT_BUILDER,
        "(Landroid/telecom/PhoneAccountHandle;Ljava/lang/CharSequence;)V",
        &[Value::Obj(handle), label],
    );
    call(
        &mut engine,
        builder,
        "setCapabilities",
        "(I)Landroid/telecom/PhoneAccount$Builder;",
        &[Value::Int(0x802)],
    );
    call(
        &mut engine,
        builder,
        "setIsEnabled",
        "(Z)Landroid/telecom/PhoneAccount$Builder;",
        &[Value::Int(1)],
    );
    let account = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/telecom/PhoneAccount;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            account,
            "hasCapabilities",
            "(I)Z",
            &[Value::Int(0x800)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            account,
            "hasCapabilities",
            "(I)Z",
            &[Value::Int(0x1)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, account, "isSelfManaged", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, account, "isEnabled", "()Z", &[]),
        Value::Int(1)
    );
    call(
        &mut engine,
        account,
        "setIsEnabled",
        "(Z)V",
        &[Value::Int(0)],
    );
    assert_eq!(
        call(&mut engine, account, "isEnabled", "()Z", &[]),
        Value::Int(0)
    );
    // build() aliases the builder's scheme list (AOSP behaviour): adding a
    // scheme to the builder afterwards becomes visible on the account.
    let sip = text(&mut engine, "sip");
    call(
        &mut engine,
        builder,
        "addSupportedUriScheme",
        "(Ljava/lang/String;)Landroid/telecom/PhoneAccount$Builder;",
        &[sip],
    );
    let sip2 = text(&mut engine, "sip");
    assert_eq!(
        call(
            &mut engine,
            account,
            "supportsUriScheme",
            "(Ljava/lang/String;)Z",
            &[sip2]
        ),
        Value::Int(1)
    );
    let rebuilt_builder = object(call(
        &mut engine,
        account,
        "toBuilder",
        "()Landroid/telecom/PhoneAccount$Builder;",
        &[],
    ));
    let rebuilt = object(call(
        &mut engine,
        rebuilt_builder,
        "build",
        "()Landroid/telecom/PhoneAccount;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            account,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(rebuilt)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, account, "hashCode", "()I", &[]),
        call(&mut engine, rebuilt, "hashCode", "()I", &[])
    );

    // StatusHints equality + hash sum.
    let label1 = text(&mut engine, "Ringing");
    let hints1 = p49_new(
        &mut engine,
        P36I_HINTS,
        "(Ljava/lang/CharSequence;Landroid/graphics/drawable/Icon;Landroid/os/Bundle;)V",
        &[label1, Value::Null, Value::Null],
    );
    let label2 = text(&mut engine, "Ringing");
    let hints2 = p49_new(
        &mut engine,
        P36I_HINTS,
        "(Ljava/lang/CharSequence;Landroid/graphics/drawable/Icon;Landroid/os/Bundle;)V",
        &[label2, Value::Null, Value::Null],
    );
    assert_eq!(
        call(
            &mut engine,
            hints1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(hints2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, hints1, "hashCode", "()I", &[]),
        call(&mut engine, hints2, "hashCode", "()I", &[])
    );

    // PhoneNumberUtils BCD codec, including MMI plus placement.
    let encode = |engine: &mut Engine, number: &str| {
        let value = text(engine, number);
        call_static(
            engine,
            P36I_PHONE_UTILS,
            "numberToCalledPartyBCD",
            "(Ljava/lang/String;)[B",
            &[value],
        )
    };
    let intl = encode(&mut engine, "+1555010");
    assert_eq!(
        byte_array_values(&engine, &intl),
        vec![0x91u8, 0x51, 0x55, 0x10, 0xF0]
    );
    let decode = |engine: &mut Engine, bytes: Value, len: i32| {
        call_static_text(
            engine,
            P36I_PHONE_UTILS,
            "calledPartyBCDToString",
            "([BII)Ljava/lang/String;",
            &[bytes, Value::Int(0), Value::Int(len)],
        )
    };
    assert_eq!(decode(&mut engine, intl.clone(), 5), "+1555010");
    let local = encode(&mut engine, "555");
    assert_eq!(byte_array_values(&engine, &local), vec![0x81u8, 0x55, 0xF5]);
    assert_eq!(decode(&mut engine, local, 3), "555");
    let mmi = encode(&mut engine, "+*31#123");
    let mmi_len = byte_array_values(&engine, &mmi).len() as i32;
    assert_eq!(decode(&mut engine, mmi, mmi_len), "*31#+123");
    let oddball = encode(&mut engine, "+**21#");
    let odd_len = byte_array_values(&engine, &oddball).len() as i32;
    assert_eq!(decode(&mut engine, oddball, odd_len), "**21#+");
    assert_eq!(decode(&mut engine, intl, 1), "");
    let fragment_bytes = byte_array(&mut engine, &[0x21]);
    assert_eq!(
        call_static_text(
            &mut engine,
            P36I_PHONE_UTILS,
            "calledPartyBCDFragmentToString",
            "([BII)Ljava/lang/String;",
            &[fragment_bytes, Value::Int(0), Value::Int(1)]
        ),
        "12"
    );
    let empty_number = text(&mut engine, "+");
    assert_eq!(
        call_static(
            &mut engine,
            P36I_PHONE_UTILS,
            "numberToCalledPartyBCD",
            "(Ljava/lang/String;)[B",
            &[empty_number]
        ),
        Value::Null
    );
    let bad_char = text(&mut engine, "12a");
    assert_throws(
        engine.invoke_static(
            P36I_PHONE_UTILS,
            "numberToCalledPartyBCD",
            "(Ljava/lang/String;)[B",
            &[bad_char],
        ),
        "Ljava/lang/RuntimeException;",
    );
    // networkPortionToCalledPartyBCD strips separators/post-dial first.
    let messy = text(&mut engine, "+1 (555) 010,99");
    let network_bcd = call_static(
        &mut engine,
        P36I_PHONE_UTILS,
        "networkPortionToCalledPartyBCD",
        "(Ljava/lang/String;)[B",
        &[messy],
    );
    let network_len = byte_array_values(&engine, &network_bcd).len() as i32;
    assert_eq!(decode(&mut engine, network_bcd, network_len), "+1555010");
}

#[test]
fn batch36i_manager_no_service_lists_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let sm = p36i_system_service(&mut engine, "telephony_subscription_service");
    let ids = call(&mut engine, sm, "getActiveSubscriptionIdList", "()[I", &[]);
    assert_eq!(array_values(&engine, &ids).len(), 0);
    let all = call(
        &mut engine,
        sm,
        "getAllSubscriptionInfoList",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, all), Value::Int(0));
    assert_eq!(
        call(
            &mut engine,
            sm,
            "getAvailableSubscriptionInfoList",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            sm,
            "getAccessibleSubscriptionInfoList",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );

    let telecom = p36i_system_service(&mut engine, "telecom");
    let capable = call(
        &mut engine,
        telecom,
        "getCallCapablePhoneAccounts",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, capable), Value::Int(0));
    let all_accounts = call(
        &mut engine,
        telecom,
        "getAllPhoneAccounts",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36h_size(&mut engine, all_accounts), Value::Int(0));
    assert_eq!(
        call(&mut engine, telecom, "getAllPhoneAccountsCount", "()I", &[]),
        Value::Int(0)
    );
    // getPhoneAccountsForPackage is the odd one out in AOSP: null, not empty.
    assert_eq!(
        call(
            &mut engine,
            telecom,
            "getPhoneAccountsForPackage",
            "()Ljava/util/List;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            telecom,
            "getUserSelectedOutgoingPhoneAccount",
            "()Landroid/telecom/PhoneAccountHandle;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, telecom, "getCurrentTtyMode", "()I", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, telecom, "isTtySupported", "()Z", &[]),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_instance(
            telecom,
            "getOwnSelfManagedPhoneAccounts",
            "()Ljava/util/List;",
            &[],
        ),
        "Ljava/lang/IllegalStateException;",
    );
}

#[test]
fn gateway_info_value_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let pkg = text(&mut engine, "com.example.gateway");
    let gateway = p36i_uri(&mut engine, "sip:gateway@example.com");
    let original = p36i_uri(&mut engine, "tel:+1555010");
    let info = p49_new(
        &mut engine,
        "Landroid/telecom/GatewayInfo;",
        "(Ljava/lang/String;Landroid/net/Uri;Landroid/net/Uri;)V",
        &[pkg, gateway.clone(), original.clone()],
    );
    assert_eq!(
        call_text(
            &mut engine,
            info,
            "getGatewayProviderPackageName",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.gateway"
    );
    assert_eq!(
        call(
            &mut engine,
            info,
            "getGatewayAddress",
            "()Landroid/net/Uri;",
            &[]
        ),
        gateway
    );
    assert_eq!(
        call(
            &mut engine,
            info,
            "getOriginalAddress",
            "()Landroid/net/Uri;",
            &[]
        ),
        original
    );
    assert_eq!(
        call(&mut engine, info, "isEmpty", "()Z", &[]),
        Value::Int(0)
    );
    let empty_pkg = text(&mut engine, "");
    let empty_info = p49_new(
        &mut engine,
        "Landroid/telecom/GatewayInfo;",
        "(Ljava/lang/String;Landroid/net/Uri;Landroid/net/Uri;)V",
        &[empty_pkg, gateway, Value::Null],
    );
    assert_eq!(
        call(&mut engine, empty_info, "isEmpty", "()Z", &[]),
        Value::Int(1)
    );
}

#[test]
fn phone_account_handle_two_arg_ctor_and_tm_service_state_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let component = p36i_component(&mut engine, "com.example.dialer", "com.example.dialer.Svc");
    let id = text(&mut engine, "acc-2");
    let handle = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;)V",
        &[Value::Obj(component), id],
    );
    let user = call(
        &mut engine,
        handle,
        "getUserHandle",
        "()Landroid/os/UserHandle;",
        &[],
    );
    assert!(
        matches!(user, Value::Obj(_)),
        "owner user handle must be present"
    );
    let id_b = text(&mut engine, "acc-2");
    let handle_b = p49_new(
        &mut engine,
        P36I_HANDLE,
        "(Landroid/content/ComponentName;Ljava/lang/String;)V",
        &[Value::Obj(component), id_b],
    );
    assert_eq!(
        call(
            &mut engine,
            handle,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(handle_b)]
        ),
        Value::Int(1)
    );

    let tm = p36i_system_service(&mut engine, "telephony");
    assert_eq!(
        call(
            &mut engine,
            tm,
            "getServiceState",
            "()Landroid/telephony/ServiceState;",
            &[]
        ),
        Value::Null
    );
}

#[test]
fn mbms_download_request_app_intent_round_trip_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let source = p36i_uri(&mut engine, "http://example.com/movie.mp4");
    let dest = p36i_uri(&mut engine, "file:///data/local/movie.mp4");
    let builder = p49_new(
        &mut engine,
        P36I_DOWNLOAD_BUILDER,
        "(Landroid/net/Uri;Landroid/net/Uri;)V",
        &[source, dest],
    );
    // Fresh request: no app intent recorded yet. AOSP getIntentForApp calls
    // Intent.parseUri(null, 0), which throws NullPointerException (only
    // URISyntaxException is caught), so the NPE propagates.
    let plain = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    assert_throws(
        engine.invoke_instance(plain, "getIntentForApp", "()Landroid/content/Intent;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    let action = text(&mut engine, "com.example.DOWNLOAD_DONE");
    let intent = p49_new(
        &mut engine,
        "Landroid/content/Intent;",
        "(Ljava/lang/String;)V",
        &[action],
    );
    call(
        &mut engine,
        builder,
        "setAppIntent",
        "(Landroid/content/Intent;)Landroid/telephony/mbms/DownloadRequest$Builder;",
        &[Value::Obj(intent)],
    );
    let request = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/telephony/mbms/DownloadRequest;",
        &[],
    ));
    let round_tripped = call(
        &mut engine,
        request,
        "getIntentForApp",
        "()Landroid/content/Intent;",
        &[],
    );
    let recovered = object(round_tripped);
    assert_eq!(
        call_text(
            &mut engine,
            recovered,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.DOWNLOAD_DONE"
    );
}

#[test]
fn intent_to_uri_and_parse_uri_round_trip_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let action = text(&mut engine, "com.example.PLAY");
    let intent = p49_new(
        &mut engine,
        "Landroid/content/Intent;",
        "(Ljava/lang/String;)V",
        &[action],
    );
    let uri = call_text(
        &mut engine,
        intent,
        "toUri",
        "(I)Ljava/lang/String;",
        &[Value::Int(0)],
    );
    assert_eq!(uri, "#Intent;action=com.example.PLAY;end");

    // Extras + launch flags survive the round trip; immutable grant flags
    // are masked out by parseUri unless URI_ALLOW_UNSAFE is passed.
    let key = text(&mut engine, "track");
    call(
        &mut engine,
        intent,
        "putExtra",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[key, Value::Int(42)],
    );
    call(
        &mut engine,
        intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[Value::Int(0x101)],
    );
    let uri_text = call_text(
        &mut engine,
        intent,
        "toUri",
        "(I)Ljava/lang/String;",
        &[Value::Int(0)],
    );
    let uri_value = text(&mut engine, &uri_text);
    assert!(text_of(&engine, &uri_value).contains("launchFlags=0x101"));
    assert!(text_of(&engine, &uri_value).contains("i.track=42"));
    let parsed = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[uri_value, Value::Int(0)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            parsed,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.PLAY"
    );
    assert_eq!(
        call(&mut engine, parsed, "getFlags", "()I", &[]),
        Value::Int(0x100) // FLAG_GRANT_READ_URI_PERMISSION (0x1) masked out
    );
    let key2 = text(&mut engine, "track");
    assert_eq!(
        call(
            &mut engine,
            parsed,
            "getIntExtra",
            "(Ljava/lang/String;I)I",
            &[key2, Value::Int(-1)]
        ),
        Value::Int(42)
    );

    // A bare URI parses to ACTION_VIEW with that data.
    let bare = text(&mut engine, "https://example.com/watch");
    let view = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[bare, Value::Int(0)],
    ));
    assert_eq!(
        call_text(&mut engine, view, "getAction", "()Ljava/lang/String;", &[]),
        "android.intent.action.VIEW"
    );
    let data = call(&mut engine, view, "getData", "()Landroid/net/Uri;", &[]);
    let data_obj = object(data);
    assert_eq!(
        call_text(
            &mut engine,
            data_obj,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "https://example.com/watch"
    );

    // Unknown EXTRA types are URISyntaxException, not a silent default.
    let malformed = text(&mut engine, "#Intent;X.key=1;end");
    assert_throws(
        engine.invoke_static(
            "Landroid/content/Intent;",
            "parseUri",
            "(Ljava/lang/String;I)Landroid/content/Intent;",
            &[malformed, Value::Int(0)],
        ),
        "Ljava/net/URISyntaxException;",
    );
}

#[test]
fn intent_parse_uri_selector_and_android_app_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    // Selector: base intent has no package, so the SEL block is retained.
    let sel_uri = text(
        &mut engine,
        "#Intent;action=com.example.BASE;SEL;action=com.example.SEL;end",
    );
    let parsed = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[sel_uri, Value::Int(0)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            parsed,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.BASE"
    );
    let selector = object(call(
        &mut engine,
        parsed,
        "getSelector",
        "()Landroid/content/Intent;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            selector,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.SEL"
    );
    let back = call_text(
        &mut engine,
        parsed,
        "toUri",
        "(I)Ljava/lang/String;",
        &[Value::Int(0)],
    );
    assert!(back.contains("SEL;action=com.example.SEL"), "toUri: {back}");

    // android-app: package + scheme + authority + path decomposition.
    let app_uri = text(
        &mut engine,
        "android-app://com.example.app/https/example.com/watch#Intent;end",
    );
    let app = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[app_uri, Value::Int(0)],
    ));
    assert_eq!(
        call_text(&mut engine, app, "getPackage", "()Ljava/lang/String;", &[]),
        "com.example.app"
    );
    assert_eq!(
        call_text(&mut engine, app, "getAction", "()Ljava/lang/String;", &[]),
        "android.intent.action.VIEW"
    );
    let data = object(call(
        &mut engine,
        app,
        "getData",
        "()Landroid/net/Uri;",
        &[],
    ));
    assert_eq!(
        call_text(&mut engine, data, "toString", "()Ljava/lang/String;", &[]),
        "https://example.com/watch"
    );

    // Package-only android-app targets the package with ACTION_MAIN.
    let pkg_uri = text(&mut engine, "android-app://com.example.only#Intent;end");
    let pkg_intent = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[pkg_uri, Value::Int(0)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            pkg_intent,
            "getAction",
            "()Ljava/lang/String;",
            &[]
        ),
        "android.intent.action.MAIN"
    );
    assert_eq!(
        call(
            &mut engine,
            pkg_intent,
            "getData",
            "()Landroid/net/Uri;",
            &[]
        ),
        Value::Null
    );

    // Old (pre-ICS) format: #action(...);categories(...!...).
    let old_uri = text(
        &mut engine,
        "https://example.com/x#action(com.example.OLD)categories(android.intent.category.BROWSABLE!com.example.CAT)",
    );
    let old = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[old_uri, Value::Int(0)],
    ));
    assert_eq!(
        call_text(&mut engine, old, "getAction", "()Ljava/lang/String;", &[]),
        "com.example.OLD"
    );
    let old_data = object(call(
        &mut engine,
        old,
        "getData",
        "()Landroid/net/Uri;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            old_data,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "https://example.com/x"
    );
}

#[test]
fn telecom_call_control_no_service_behaviour_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let telecom = p36i_system_service(&mut engine, "telecom");
    assert_eq!(
        call(&mut engine, telecom, "endCall", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, telecom, "isInEmergencyCall", "()Z", &[]),
        Value::Int(0)
    );
    call(&mut engine, telecom, "acceptRingingCall", "()V", &[]);
    call(&mut engine, telecom, "silenceRinger", "()V", &[]);
    call(
        &mut engine,
        telecom,
        "cancelMissedCallsNotification",
        "()V",
        &[],
    );
    let dial = text(&mut engine, "*#06#");
    assert_eq!(
        call(
            &mut engine,
            telecom,
            "handlePinMmi",
            "(Ljava/lang/String;)Z",
            &[dial]
        ),
        Value::Int(0)
    );
    let adn = call(
        &mut engine,
        telecom,
        "getAdnUriForPhoneAccount",
        "(Landroid/telecom/PhoneAccountHandle;)Landroid/net/Uri;",
        &[Value::Null],
    );
    let adn_uri = object(adn);
    assert_eq!(
        call_text(
            &mut engine,
            adn_uri,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "content://icc/adn"
    );
}

#[test]
fn telephony_cell_defaults_and_subscription_id_caches_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let tm = p36i_system_service(&mut engine, "telephony");
    assert_eq!(
        call(
            &mut engine,
            tm,
            "getCellLocation",
            "()Landroid/telephony/CellLocation;",
            &[]
        ),
        Value::Null
    );
    assert_eq!(
        call(&mut engine, tm, "getAllCellInfo", "()Ljava/util/List;", &[]),
        Value::Null
    );
    assert_eq!(
        call_static(&mut engine, P36I_SM, "getPhoneId", "(I)I", &[Value::Int(3)]),
        Value::Int(-1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            P36I_SM,
            "getSlotIndex",
            "(I)I",
            &[Value::Int(3)]
        ),
        Value::Int(-1)
    );
}

#[test]
fn intent_to_uri_with_data_and_intent_scheme_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let intent = p49_new(&mut engine, "Landroid/content/Intent;", "()V", &[]);
    let data = p36i_uri(&mut engine, "https://example.com/play?id=7");
    call(
        &mut engine,
        intent,
        "setData",
        "(Landroid/net/Uri;)Landroid/content/Intent;",
        &[data],
    );
    // flags=0: raw data string, no fragment (nothing else is set).
    assert_eq!(
        call_text(
            &mut engine,
            intent,
            "toUri",
            "(I)Ljava/lang/String;",
            &[Value::Int(0)]
        ),
        "https://example.com/play?id=7"
    );
    // URI_INTENT_SCHEME: scheme moves into the fragment.
    let with_scheme = call_text(
        &mut engine,
        intent,
        "toUri",
        "(I)Ljava/lang/String;",
        &[Value::Int(1)],
    );
    assert_eq!(
        with_scheme,
        "intent://example.com/play?id=7#Intent;scheme=https;end"
    );
    let value = text(&mut engine, &with_scheme);
    let parsed = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[value, Value::Int(1)],
    ));
    let parsed_data = object(call(
        &mut engine,
        parsed,
        "getData",
        "()Landroid/net/Uri;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            parsed_data,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "https://example.com/play?id=7"
    );

    // Component survives the round trip via flattenToShortString.
    let component = p36i_component(&mut engine, "com.example.app", "com.example.app.Player");
    call(
        &mut engine,
        intent,
        "setComponent",
        "(Landroid/content/ComponentName;)Landroid/content/Intent;",
        &[Value::Obj(component)],
    );
    let uri_text = call_text(
        &mut engine,
        intent,
        "toUri",
        "(I)Ljava/lang/String;",
        &[Value::Int(0)],
    );
    assert!(
        uri_text.contains("component=com.example.app/.Player"),
        "{uri_text}"
    );
    let value = text(&mut engine, &uri_text);
    let parsed = object(call_static(
        &mut engine,
        "Landroid/content/Intent;",
        "parseUri",
        "(Ljava/lang/String;I)Landroid/content/Intent;",
        &[value, Value::Int(0)],
    ));
    let back = object(call(
        &mut engine,
        parsed,
        "getComponent",
        "()Landroid/content/ComponentName;",
        &[],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            back,
            "getClassName",
            "()Ljava/lang/String;",
            &[]
        ),
        "com.example.app.Player"
    );
}

#[test]
fn wifi_ipsec_value_identity_audit_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());

    // WifiSsid.fromBytes keeps the caller's array reference (AOSP does not
    // copy); getBytes still clones.
    let raw = byte_array(&mut engine, b"AliasNet");
    let ssid = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromBytes",
        "([B)Landroid/net/wifi/WifiSsid;",
        std::slice::from_ref(&raw),
    ));
    assert_eq!(field(&engine, ssid, "mBytes"), raw);
    let cloned = call(&mut engine, ssid, "getBytes", "()[B", &[]);
    assert_ne!(cloned, raw);

    // decodeSsid overflow: >32 UTF-16 units truncate instead of failing,
    // and a malformed byte past the buffer is never examined.
    let mut long = Vec::new();
    long.extend_from_slice(&[b'a'; 40]);
    long.push(0xff);
    let long_array = byte_array(&mut engine, &long);
    let long_ssid = object(call_static(
        &mut engine,
        P36H_WIFI_SSID,
        "fromBytes",
        "([B)Landroid/net/wifi/WifiSsid;",
        &[long_array],
    ));
    let decoded = call(
        &mut engine,
        long_ssid,
        "getUtf8Text",
        "()Ljava/lang/CharSequence;",
        &[],
    );
    assert_eq!(text_of(&engine, &decoded), "a".repeat(32));

    // IkeTrafficSelector equals/hashCode contract.
    let v4a = instance(&mut engine, "Ljava/net/Inet4Address;");
    let v4b = instance(&mut engine, "Ljava/net/Inet4Address;");
    let make_ts = |engine: &mut Engine| {
        p49_new(
            engine,
            P36H_TS,
            "(IILjava/net/InetAddress;Ljava/net/InetAddress;)V",
            &[
                Value::Int(0),
                Value::Int(65535),
                Value::Obj(v4a),
                Value::Obj(v4b),
            ],
        )
    };
    let ts1 = make_ts(&mut engine);
    let ts2 = make_ts(&mut engine);
    assert_eq!(
        call(
            &mut engine,
            ts1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(ts2)]
        ),
        Value::Int(1)
    );
    let h1 = call(&mut engine, ts1, "hashCode", "()I", &[]);
    let h2 = call(&mut engine, ts2, "hashCode", "()I", &[]);
    assert_eq!(h1, h2);
    assert_ne!(h1, Value::Int(ts1 as i32));

    // SaProposal equals/hashCode contract (IKE + PRFs, Child + default ESN).
    let build_ike = |engine: &mut Engine| {
        let builder = p36h_ike_builder(engine);
        call(
            engine,
            builder,
            "addEncryptionAlgorithm",
            "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(12), Value::Int(256)],
        );
        call(
            engine,
            builder,
            "addIntegrityAlgorithm",
            "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(12)],
        );
        call(
            engine,
            builder,
            "addPseudorandomFunction",
            "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(5)],
        );
        call(
            engine,
            builder,
            "addDhGroup",
            "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
            &[Value::Int(14)],
        );
        object(call(
            engine,
            builder,
            "build",
            "()Landroid/net/ipsec/ike/IkeSaProposal;",
            &[],
        ))
    };
    let p1 = build_ike(&mut engine);
    let p2 = build_ike(&mut engine);
    assert_eq!(
        call(
            &mut engine,
            p1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(p2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(&mut engine, p1, "hashCode", "()I", &[]),
        call(&mut engine, p2, "hashCode", "()I", &[])
    );
    // Different key length -> not equal.
    let builder = p36h_ike_builder(&mut engine);
    call(
        &mut engine,
        builder,
        "addEncryptionAlgorithm",
        "(II)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12), Value::Int(128)],
    );
    call(
        &mut engine,
        builder,
        "addIntegrityAlgorithm",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(12)],
    );
    call(
        &mut engine,
        builder,
        "addPseudorandomFunction",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(5)],
    );
    call(
        &mut engine,
        builder,
        "addDhGroup",
        "(I)Landroid/net/ipsec/ike/IkeSaProposal$Builder;",
        &[Value::Int(14)],
    );
    let p3 = object(call(
        &mut engine,
        builder,
        "build",
        "()Landroid/net/ipsec/ike/IkeSaProposal;",
        &[],
    ));
    assert_eq!(
        call(
            &mut engine,
            p1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(p3)]
        ),
        Value::Int(0)
    );
}

#[test]
fn freeze_periods_and_update_policy_validation_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let month_day = |engine: &mut Engine, month: i32, day: i32| {
        object(call_static(
            engine,
            "Ljava/time/MonthDay;",
            "of",
            "(II)Ljava/time/MonthDay;",
            &[Value::Int(month), Value::Int(day)],
        ))
    };
    let feb29 = month_day(&mut engine, 2, 29);
    assert_eq!(
        call_text(&mut engine, feb29, "toString", "()Ljava/lang/String;", &[]),
        "--02-29"
    );
    // Feb 29 in a non-leap year adjusts to Feb 28 (MonthDay.atYear).
    let at_2023 = object(call(
        &mut engine,
        feb29,
        "atYear",
        "(I)Ljava/time/LocalDate;",
        &[Value::Int(2023)],
    ));
    assert_eq!(
        call_text(
            &mut engine,
            at_2023,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "2023-02-28"
    );
    assert_throws(
        engine.invoke_static(
            "Ljava/time/MonthDay;",
            "of",
            "(II)Ljava/time/MonthDay;",
            &[Value::Int(2), Value::Int(30)],
        ),
        "Ljava/time/DateTimeException;",
    );

    let freeze = |engine: &mut Engine, sm: i32, sd: i32, em: i32, ed: i32| {
        let start = month_day(engine, sm, sd);
        let end = month_day(engine, em, ed);
        p49_new(
            engine,
            "Landroid/app/admin/FreezePeriod;",
            "(Ljava/time/MonthDay;Ljava/time/MonthDay;)V",
            &[Value::Obj(start), Value::Obj(end)],
        )
    };
    let policy = object(call_static(
        &mut engine,
        P36G_UPDATE_POLICY,
        "createWindowedInstallPolicy",
        "(II)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Int(60), Value::Int(180)],
    ));
    let winter = freeze(&mut engine, 12, 20, 1, 10);
    assert_eq!(
        call_text(&mut engine, winter, "toString", "()Ljava/lang/String;", &[]),
        "Dec 20 - Jan 10"
    );
    let periods = list_of(&mut engine, &[Value::Obj(winter)]);
    let chained = call(
        &mut engine,
        policy,
        "setFreezePeriods",
        "(Ljava/util/List;)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Obj(periods)],
    );
    assert_eq!(chained, Value::Obj(policy));
    assert_eq!(
        call(&mut engine, policy, "isValid", "()Z", &[]),
        Value::Int(1)
    );
    let stored = call(
        &mut engine,
        policy,
        "getFreezePeriods",
        "()Ljava/util/List;",
        &[],
    );
    assert_eq!(p36g_list_size(&mut engine, stored), Value::Int(1));

    // Overlapping periods: ValidationFailedException with error code 2.
    let a = freeze(&mut engine, 1, 1, 1, 20);
    let b = freeze(&mut engine, 1, 15, 2, 1);
    let overlapping = list_of(&mut engine, &[Value::Obj(a), Value::Obj(b)]);
    match engine.invoke_instance(
        policy,
        "setFreezePeriods",
        "(Ljava/util/List;)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Obj(overlapping)],
    ) {
        Err(rd_vm::err::VmExit::Exception(thrown)) => {
            assert_eq!(
                thrown.class,
                "Landroid/app/admin/SystemUpdatePolicy$ValidationFailedException;"
            );
            let obj = thrown.obj.expect("exception object");
            assert_eq!(
                engine
                    .invoke_instance(obj, "getErrorCode", "()I", &[])
                    .expect("getErrorCode"),
                Value::Int(2)
            );
        }
        other => panic!("expected ValidationFailedException, got {other:?}"),
    }

    // A 100-day period exceeds the 90-day maximum (error code 3).
    let long = freeze(&mut engine, 1, 1, 4, 10);
    let too_long = list_of(&mut engine, &[Value::Obj(long)]);
    match engine.invoke_instance(
        policy,
        "setFreezePeriods",
        "(Ljava/util/List;)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Obj(too_long)],
    ) {
        Err(rd_vm::err::VmExit::Exception(thrown)) => {
            let obj = thrown.obj.expect("exception object");
            assert_eq!(
                engine
                    .invoke_instance(obj, "getErrorCode", "()I", &[])
                    .expect("getErrorCode"),
                Value::Int(3)
            );
        }
        other => panic!("expected freezePeriodTooLong, got {other:?}"),
    }

    // Two short periods less than 60 days apart (error code 4).
    let x = freeze(&mut engine, 1, 1, 1, 10);
    let y = freeze(&mut engine, 2, 1, 2, 10);
    let close = list_of(&mut engine, &[Value::Obj(x), Value::Obj(y)]);
    match engine.invoke_instance(
        policy,
        "setFreezePeriods",
        "(Ljava/util/List;)Landroid/app/admin/SystemUpdatePolicy;",
        &[Value::Obj(close)],
    ) {
        Err(rd_vm::err::VmExit::Exception(thrown)) => {
            let obj = thrown.obj.expect("exception object");
            assert_eq!(
                engine
                    .invoke_instance(obj, "getErrorCode", "()I", &[])
                    .expect("getErrorCode"),
                Value::Int(4)
            );
        }
        other => panic!("expected freezePeriodTooClose, got {other:?}"),
    }

    // Deep audit: AOSP derives the freeze day-of-year from
    // MonthDay.atYear(2001); 2001 is not a leap year, so a period
    // starting February 29 is dated from the 28th.
    let leap_start = freeze(&mut engine, 2, 29, 3, 5);
    assert_eq!(
        call_text(
            &mut engine,
            leap_start,
            "toString",
            "()Ljava/lang/String;",
            &[]
        ),
        "Feb 28 - Mar 05"
    );
}

#[test]
fn credential_option_public_constructor_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    let kind = text(&mut engine, "android.credentials.TYPE_PASSWORD_CREDENTIAL");
    let retrieval = p36g_bundle_with(&mut engine, "key", "value");
    let candidate = p36g_bundle_with(&mut engine, "key", "value");
    let option = p49_new(
        &mut engine,
        "Landroid/credentials/CredentialOption;",
        "(Ljava/lang/String;Landroid/os/Bundle;Landroid/os/Bundle;Z)V",
        &[
            kind,
            Value::Obj(retrieval),
            Value::Obj(candidate),
            Value::Int(1),
        ],
    );
    assert_eq!(
        call_text(&mut engine, option, "getType", "()Ljava/lang/String;", &[]),
        "android.credentials.TYPE_PASSWORD_CREDENTIAL"
    );
    assert_eq!(
        call(&mut engine, option, "isSystemProviderRequired", "()Z", &[]),
        Value::Int(1)
    );
    let providers = call(
        &mut engine,
        option,
        "getAllowedProviders",
        "()Ljava/util/Set;",
        &[],
    );
    assert_eq!(p36g_list_size(&mut engine, providers), Value::Int(0));
    assert_eq!(
        call(&mut engine, option, "describeContents", "()I", &[]),
        Value::Int(0)
    );

    // Empty type is IAE; null bundles are NPEs (private-ctor validations).
    let empty = text(&mut engine, "");
    let unused = p36g_bundle_with(&mut engine, "k", "v");
    let unused2 = p36g_bundle_with(&mut engine, "k", "v");
    let target = instance(&mut engine, "Landroid/credentials/CredentialOption;");
    assert_throws(
        engine.invoke_instance(
            target,
            "<init>",
            "(Ljava/lang/String;Landroid/os/Bundle;Landroid/os/Bundle;Z)V",
            &[
                empty,
                Value::Obj(unused),
                Value::Obj(unused2),
                Value::Int(0),
            ],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let kind = text(&mut engine, "type");
    let target = instance(&mut engine, "Landroid/credentials/CredentialOption;");
    assert_throws(
        engine.invoke_instance(
            target,
            "<init>",
            "(Ljava/lang/String;Landroid/os/Bundle;Landroid/os/Bundle;Z)V",
            &[kind, Value::Null, Value::Obj(unused2), Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn renderscript_matrix4f_multiply_and_rotate_audit_is_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    // Column-major storage: get(x, y) reads mMat[x*4 + y]. A translation by
    // (1, 2, 3) sits in column 3.
    let m = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    call(
        &mut engine,
        m,
        "loadTranslate",
        "(FFF)V",
        &[Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
    );
    assert_eq!(
        call(
            &mut engine,
            m,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(0)]
        ),
        Value::Float(1.0)
    );
    assert_eq!(
        call(
            &mut engine,
            m,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(3)]
        ),
        Value::Float(0.0)
    );
    // multiply: translate * scale applies scale first, so the translation
    // column is untouched (result(3,*) = lhs(3,*)).
    let scale = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    call(
        &mut engine,
        scale,
        "loadScale",
        "(FFF)V",
        &[Value::Float(2.0), Value::Float(4.0), Value::Float(8.0)],
    );
    call(
        &mut engine,
        m,
        "multiply",
        "(Landroid/renderscript/Matrix4f;)V",
        &[Value::Obj(scale)],
    );
    assert_eq!(
        call(
            &mut engine,
            m,
            "get",
            "(II)F",
            &[Value::Int(0), Value::Int(0)]
        ),
        Value::Float(2.0)
    );
    assert_eq!(
        call(
            &mut engine,
            m,
            "get",
            "(II)F",
            &[Value::Int(3), Value::Int(1)]
        ),
        Value::Float(2.0)
    );
    // loadRotate 90 degrees about Z maps +X toward +Y in the AOSP sign
    // convention: mMat[1] (x=0,y=1) = sin = 1, mMat[4] (x=1,y=0) = -sin.
    let rot = p49_new(&mut engine, P36F_RS_MATRIX4, "()V", &[]);
    call(
        &mut engine,
        rot,
        "loadRotate",
        "(FFFF)V",
        &[
            Value::Float(90.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(1.0),
        ],
    );
    let xy = match call(
        &mut engine,
        rot,
        "get",
        "(II)F",
        &[Value::Int(0), Value::Int(1)],
    ) {
        Value::Float(v) => v,
        other => panic!("{other:?}"),
    };
    let yx = match call(
        &mut engine,
        rot,
        "get",
        "(II)F",
        &[Value::Int(1), Value::Int(0)],
    ) {
        Value::Float(v) => v,
        other => panic!("{other:?}"),
    };
    assert!((xy - 1.0).abs() < 1e-6, "get(0,1) = {xy}");
    assert!((yx + 1.0).abs() < 1e-6, "get(1,0) = {yx}");

    // Deep audit: AOSP loadRotate "normalizes" only an already-unit
    // axis (`if (!(len != 1))`), so a non-unit axis is used verbatim.
    // Rotating 90 degrees about (0, 0, 2) therefore yields
    // m[4] = xy*nc - z*s = -2 instead of the proper rotation's -1.
    let nonunit = p49_new(&mut engine, "Landroid/renderscript/Matrix4f;", "()V", &[]);
    call(
        &mut engine,
        nonunit,
        "loadRotate",
        "(FFFF)V",
        &[
            Value::Float(90.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(2.0),
        ],
    );
    let scaled = match call(
        &mut engine,
        nonunit,
        "get",
        "(II)F",
        &[Value::Int(1), Value::Int(0)],
    ) {
        Value::Float(v) => v,
        other => panic!("{other:?}"),
    };
    assert!((scaled + 2.0).abs() < 1e-6, "get(1,0) = {scaled}");
}

#[test]
fn graphics_matrix_audit_fixes_are_guest_visible() {
    let mut engine = engine_of(&DexBuilder::new());
    const MATRIX: &str = "Landroid/graphics/Matrix;";
    let m = p49_new(&mut engine, MATRIX, "()V", &[]);
    // AOSP Matrix.hashCode() is literally `return 44`.
    assert_eq!(call(&mut engine, m, "hashCode", "()I", &[]), Value::Int(44));

    let rectf = |engine: &mut Engine, l: f32, t: f32, r: f32, b: f32| {
        let rect = instance(engine, "Landroid/graphics/RectF;");
        engine
            .heap
            .put_field(rect, "left", Value::Float(l))
            .unwrap();
        engine.heap.put_field(rect, "top", Value::Float(t)).unwrap();
        engine
            .heap
            .put_field(rect, "right", Value::Float(r))
            .unwrap();
        engine
            .heap
            .put_field(rect, "bottom", Value::Float(b))
            .unwrap();
        rect
    };
    let fit = |engine: &mut Engine, ordinal: i32| {
        let e = instance(engine, "Landroid/graphics/Matrix$ScaleToFit;");
        engine
            .heap
            .put_field(e, "rd_ord", Value::Int(ordinal))
            .unwrap();
        e
    };
    let values = |engine: &mut Engine, matrix: ObjRef| -> Vec<f32> {
        let array = float_array(engine, 9);
        call(
            engine,
            matrix,
            "getValues",
            "([F)V",
            std::slice::from_ref(&array),
        );
        match &array {
            Value::Obj(r) => engine
                .heap
                .as_array_elems(*r)
                .unwrap()
                .0
                .iter()
                .map(|v| match v {
                    Value::Float(f) => *f,
                    _ => 0.0,
                })
                .collect(),
            _ => unreachable!(),
        }
    };

    // CENTER: src 10x10 into dst 100x50 -> uniform scale 5, x off by 25.
    let src = rectf(&mut engine, 0.0, 0.0, 10.0, 10.0);
    let dst = rectf(&mut engine, 0.0, 0.0, 100.0, 50.0);
    let center = fit(&mut engine, 2);
    assert_eq!(
        call(
            &mut engine,
            m,
            "setRectToRect",
            "(Landroid/graphics/RectF;Landroid/graphics/RectF;Landroid/graphics/Matrix$ScaleToFit;)Z",
            &[Value::Obj(src), Value::Obj(dst), Value::Obj(center)],
        ),
        Value::Int(1)
    );
    let v = values(&mut engine, m);
    assert_eq!(v[0], 5.0); // MSCALE_X
    assert_eq!(v[4], 5.0); // MSCALE_Y
    assert_eq!(v[2], 25.0); // MTRANS_X (centered)
    assert_eq!(v[5], 0.0); // MTRANS_Y

    // END aligns the far edge: MTRANS_X = 50.
    let end = fit(&mut engine, 3);
    call(
        &mut engine,
        m,
        "setRectToRect",
        "(Landroid/graphics/RectF;Landroid/graphics/RectF;Landroid/graphics/Matrix$ScaleToFit;)Z",
        &[Value::Obj(src), Value::Obj(dst), Value::Obj(end)],
    );
    let v = values(&mut engine, m);
    assert_eq!(v[2], 50.0);

    // Empty src -> identity + false.
    let empty = rectf(&mut engine, 5.0, 5.0, 5.0, 9.0);
    assert_eq!(
        call(
            &mut engine,
            m,
            "setRectToRect",
            "(Landroid/graphics/RectF;Landroid/graphics/RectF;Landroid/graphics/Matrix$ScaleToFit;)Z",
            &[Value::Obj(empty), Value::Obj(dst), Value::Obj(center)],
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(&mut engine, m, "isIdentity", "()Z", &[]),
        Value::Int(1)
    );

    // mapPoints bounds: mismatched dst/src lengths are AIOOBE (AOSP).
    let two = float_value_array(&mut engine, &[1.0, 2.0]);
    let four = float_value_array(&mut engine, &[0.0; 4]);
    assert_throws(
        engine.invoke_instance(m, "mapPoints", "([F[F)V", &[two, four.clone()]),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );
    // setValues with fewer than 9 floats is AIOOBE.
    let short = float_value_array(&mut engine, &[1.0; 4]);
    assert_throws(
        engine.invoke_instance(m, "setValues", "([F)V", &[short]),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );
}
