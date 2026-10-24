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
