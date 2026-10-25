//! Guest-visible proofs for the first reimplemented advance-all batch:
//! Formatter/DateFormat, Rfc822 address tokens, view animation
//! interpolators, and the in-memory AccountManager subset.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const CONTEXT: &str = "Landroid/content/Context;";
const FORMATTER: &str = "Landroid/text/format/Formatter;";
const DATE_FORMAT: &str = "Landroid/text/format/DateFormat;";
const RFC822_TOKENIZER: &str = "Landroid/text/util/Rfc822Tokenizer;";
const ACCOUNT: &str = "Landroid/accounts/Account;";
const ACCOUNT_MANAGER: &str = "Landroid/accounts/AccountManager;";
const BUNDLE: &str = "Landroid/os/Bundle;";

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
fn opt_text_of(engine: &Engine, value: &Value) -> Option<String> {
    match value {
        Value::Obj(reference) => Some(engine.heap.as_str(*reference).unwrap().to_string()),
        Value::Null => None,
        other => panic!("expected String or null, got {other:?}"),
    }
}
fn text_of(engine: &Engine, value: &Value) -> String {
    opt_text_of(engine, value).unwrap_or_else(|| panic!("expected String, got null"))
}
fn call(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_instance(recv, name, sig, args).unwrap()
}
fn call_static(engine: &mut Engine, class: &str, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_static(class, name, sig, args).unwrap()
}
fn call_static_string(
    engine: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> String {
    let value = call_static(engine, class, name, sig, args);
    text_of(engine, &value)
}
fn call_opt_text(
    engine: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Option<String> {
    let value = call(engine, recv, name, sig, args);
    opt_text_of(engine, &value)
}
fn call_text(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> String {
    let value = call(engine, recv, name, sig, args);
    text_of(engine, &value)
}
fn array_values(engine: &Engine, value: &Value) -> Vec<Value> {
    let array = object(value.clone());
    engine.heap.as_array_elems(array).unwrap().0.to_vec()
}

#[test]
fn formatter_and_android_date_format_use_aosp_rounding_and_patterns() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    let format_file_size = "(Landroid/content/Context;J)Ljava/lang/String;";
    let format_short_file_size = "(Landroid/content/Context;J)Ljava/lang/String;";

    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatFileSize",
            format_file_size,
            &[Value::Null, Value::Long(1536)],
        ),
        ""
    );
    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatFileSize",
            format_file_size,
            &[context.clone(), Value::Long(900)],
        ),
        "900 B"
    );
    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatFileSize",
            format_file_size,
            &[context.clone(), Value::Long(1000)],
        ),
        "1.00 kB"
    );
    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatFileSize",
            format_file_size,
            &[context.clone(), Value::Long(1536)],
        ),
        "1.54 kB"
    );
    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatShortFileSize",
            format_short_file_size,
            &[context.clone(), Value::Long(1536)],
        ),
        "1.5 kB"
    );

    let resources = Value::Obj(instance(&mut engine, "Landroid/content/res/Resources;"));
    let bytes = object(call_static(
        &mut engine,
        FORMATTER,
        "formatBytes",
        "(Landroid/content/res/Resources;JI)Landroid/text/format/Formatter$BytesResult;",
        &[resources, Value::Long(1536), Value::Int(2 | 4)],
    ));
    assert_eq!(
        text_of(&engine, &engine.heap.get_field(bytes, "value").unwrap()),
        "1.54"
    );
    assert_eq!(
        text_of(&engine, &engine.heap.get_field(bytes, "units").unwrap()),
        "kB"
    );
    assert_eq!(
        engine.heap.get_field(bytes, "roundedBytes").unwrap(),
        Value::Long(1540)
    );
    assert_eq!(
        call_static_string(
            &mut engine,
            FORMATTER,
            "formatIpAddress",
            "(I)Ljava/lang/String;",
            &[Value::Int(0x0102_0304)],
        ),
        "4.3.2.1"
    );

    let pattern = text(&mut engine, "yyyy-MM-dd HH:mm:ss");
    assert_eq!(
        call_static_string(
            &mut engine,
            DATE_FORMAT,
            "format",
            "(Ljava/lang/CharSequence;J)Ljava/lang/CharSequence;",
            &[pattern, Value::Long(0)],
        ),
        "1970-01-01 00:00:00"
    );
    let pattern = text(&mut engine, "EEE MMM d yyyy h:mm a z");
    assert_eq!(
        call_static_string(
            &mut engine,
            DATE_FORMAT,
            "format",
            "(Ljava/lang/CharSequence;J)Ljava/lang/CharSequence;",
            &[pattern, Value::Long(0)],
        ),
        "Thu Jan 1 1970 12:00 AM UTC"
    );
    let order_pattern = text(&mut engine, "dd/MM/yyyy");
    let order_value = call_static(
        &mut engine,
        DATE_FORMAT,
        "getDateFormatOrder",
        "(Ljava/lang/String;)[C",
        &[order_pattern],
    );
    let order = array_values(&engine, &order_value);
    assert_eq!(
        order,
        vec![
            Value::Int('d' as i32),
            Value::Int('M' as i32),
            Value::Int('y' as i32)
        ]
    );
    let quoted = text(&mut engine, "'s' mm");
    assert_eq!(
        call_static(
            &mut engine,
            DATE_FORMAT,
            "hasSeconds",
            "(Ljava/lang/CharSequence;)Z",
            &[quoted],
        ),
        Value::Int(0)
    );
    let with_seconds = text(&mut engine, "mm:ss");
    assert_eq!(
        call_static(
            &mut engine,
            DATE_FORMAT,
            "hasSeconds",
            "(Ljava/lang/CharSequence;)Z",
            &[with_seconds],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            DATE_FORMAT,
            "is24HourFormat",
            "(Landroid/content/Context;)Z",
            &[context],
        ),
        Value::Int(0)
    );
}

#[test]
fn rfc822_tokenizer_parses_names_addresses_and_comments() {
    let mut engine = engine_of(&DexBuilder::new());
    let input = text(
        &mut engine,
        "John Doe (Boss) <john@example.com>, jane@example.com (Friend), \"Doe, John\" <jd@example.com>",
    );
    let tokens_value = call_static(
        &mut engine,
        RFC822_TOKENIZER,
        "tokenize",
        "(Ljava/lang/CharSequence;)[Landroid/text/util/Rfc822Token;",
        &[input],
    );
    let tokens = array_values(&engine, &tokens_value);
    assert_eq!(tokens.len(), 3);

    let first = object(tokens[0].clone());
    assert_eq!(
        call_opt_text(&mut engine, first, "getName", "()Ljava/lang/String;", &[]),
        Some("John Doe".to_string())
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            first,
            "getAddress",
            "()Ljava/lang/String;",
            &[]
        ),
        Some("john@example.com".to_string())
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            first,
            "getComment",
            "()Ljava/lang/String;",
            &[]
        ),
        Some("Boss".to_string())
    );
    assert_eq!(
        call_text(&mut engine, first, "toString", "()Ljava/lang/String;", &[]),
        "John Doe (Boss) <john@example.com>"
    );

    let second = object(tokens[1].clone());
    assert_eq!(
        call_opt_text(&mut engine, second, "getName", "()Ljava/lang/String;", &[]),
        None
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            second,
            "getAddress",
            "()Ljava/lang/String;",
            &[]
        ),
        Some("jane@example.com".to_string())
    );
    assert_eq!(
        call_text(&mut engine, second, "toString", "()Ljava/lang/String;", &[]),
        "(Friend) <jane@example.com>"
    );

    let third = object(tokens[2].clone());
    assert_eq!(
        call_opt_text(&mut engine, third, "getName", "()Ljava/lang/String;", &[]),
        Some("Doe, John".to_string())
    );
    assert_eq!(
        call_text(&mut engine, third, "toString", "()Ljava/lang/String;", &[]),
        "\"Doe, John\" <jd@example.com>"
    );

    let list = instance(&mut engine, "Ljava/util/ArrayList;");
    call(&mut engine, list, "<init>", "()V", &[]);
    let input = text(&mut engine, "a@b, c@d");
    call_static(
        &mut engine,
        RFC822_TOKENIZER,
        "tokenize",
        "(Ljava/lang/CharSequence;Ljava/util/Collection;)V",
        &[input, Value::Obj(list)],
    );
    assert_eq!(call(&mut engine, list, "size", "()I", &[]), Value::Int(2));

    let tokenizer = instance(&mut engine, RFC822_TOKENIZER);
    call(&mut engine, tokenizer, "<init>", "()V", &[]);
    let token_text = text(&mut engine, "a@b, \"x,y\" <x@y>");
    assert_eq!(
        call(
            &mut engine,
            tokenizer,
            "findTokenEnd",
            "(Ljava/lang/CharSequence;I)I",
            &[token_text.clone(), Value::Int(0)],
        ),
        Value::Int(3)
    );
    let len = "a@b, \"x,y\" <x@y>".len() as i32;
    assert_eq!(
        call(
            &mut engine,
            tokenizer,
            "findTokenStart",
            "(Ljava/lang/CharSequence;I)I",
            &[token_text.clone(), Value::Int(len)],
        ),
        Value::Int(5)
    );

    // Deep audit: AOSP cursor bounds. findTokenStart past the end
    // walks into charAt(len + 1); findTokenEnd with a negative
    // cursor indexes charAt(-1); a past-the-end findTokenEnd cursor
    // is returned untouched, and a negative findTokenStart is 0.
    let short = text(&mut engine, "a@b");
    for (call_name, cursor) in [("findTokenStart", 99), ("findTokenEnd", -1)] {
        match engine.invoke_instance(
            tokenizer,
            call_name,
            "(Ljava/lang/CharSequence;I)I",
            &[short.clone(), Value::Int(cursor)],
        ) {
            Err(rd_vm::err::VmExit::Exception(t)) => {
                assert_eq!(t.class, "Ljava/lang/StringIndexOutOfBoundsException;")
            }
            other => panic!("expected StringIndexOutOfBoundsException, got {other:?}"),
        }
    }
    assert_eq!(
        call(
            &mut engine,
            tokenizer,
            "findTokenEnd",
            "(Ljava/lang/CharSequence;I)I",
            &[short.clone(), Value::Int(99)],
        ),
        Value::Int(99)
    );
    assert_eq!(
        call(
            &mut engine,
            tokenizer,
            "findTokenStart",
            "(Ljava/lang/CharSequence;I)I",
            &[short, Value::Int(-1)],
        ),
        Value::Int(0)
    );
    let _ = token_text;
}

fn interpolation(
    engine: &mut Engine,
    class: &str,
    init_sig: &str,
    init_args: &[Value],
    input: f32,
) -> f32 {
    let interpolator = instance(engine, class);
    call(engine, interpolator, "<init>", init_sig, init_args);
    match call(
        engine,
        interpolator,
        "getInterpolation",
        "(F)F",
        &[Value::Float(input)],
    ) {
        Value::Float(v) => v,
        other => panic!("expected float interpolation, got {other:?}"),
    }
}
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn view_animation_interpolators_match_aosp_formulas() {
    let mut engine = engine_of(&DexBuilder::new());
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/LinearInterpolator;",
            "()V",
            &[],
            0.37,
        ),
        0.37,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AccelerateInterpolator;",
            "()V",
            &[],
            0.5,
        ),
        0.25,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AccelerateInterpolator;",
            "(F)V",
            &[Value::Float(2.0)],
            0.5,
        ),
        0.0625,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/DecelerateInterpolator;",
            "()V",
            &[],
            0.5,
        ),
        0.75,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AccelerateDecelerateInterpolator;",
            "()V",
            &[],
            0.5,
        ),
        0.5,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AnticipateInterpolator;",
            "()V",
            &[],
            0.5,
        ),
        -0.125,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/OvershootInterpolator;",
            "()V",
            &[],
            0.5,
        ),
        1.125,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AnticipateOvershootInterpolator;",
            "()V",
            &[],
            0.25,
        ),
        -0.125,
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/AnticipateOvershootInterpolator;",
            "()V",
            &[],
            0.75,
        ),
        1.125,
    );
    let bounce_end = interpolation(
        &mut engine,
        "Landroid/view/animation/BounceInterpolator;",
        "()V",
        &[],
        1.0,
    );
    assert!(
        (bounce_end - 1.0).abs() < 0.001,
        "bounce end was {bounce_end}"
    );
    assert_close(
        interpolation(
            &mut engine,
            "Landroid/view/animation/CycleInterpolator;",
            "(F)V",
            &[Value::Float(1.0)],
            0.25,
        ),
        1.0,
    );

    // Deep audit: AccelerateDecelerate computes (input + 1) in float
    // before widening for cos; one ULP above 0.25 the float sum
    // rounds to exactly 1.25, changing the last bits of the result.
    let exact = interpolation(
        &mut engine,
        "Landroid/view/animation/AccelerateDecelerateInterpolator;",
        "()V",
        &[],
        f32::from_bits(0x3e80_0001),
    );
    assert_eq!(exact.to_bits(), 0x3e15_f61a);
}

fn account(engine: &mut Engine, name: &str, ty: &str) -> ObjRef {
    let account = instance(engine, ACCOUNT);
    let name = text(engine, name);
    let ty = text(engine, ty);
    call(
        engine,
        account,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[name, ty],
    );
    account
}

#[test]
fn account_manager_explicit_accounts_passwords_userdata_and_tokens() {
    let mut engine = engine_of(&DexBuilder::new());
    let context = Value::Obj(instance(&mut engine, CONTEXT));
    let manager = object(call_static(
        &mut engine,
        ACCOUNT_MANAGER,
        "get",
        "(Landroid/content/Context;)Landroid/accounts/AccountManager;",
        &[context],
    ));
    let empty = call(
        &mut engine,
        manager,
        "getAccounts",
        "()[Landroid/accounts/Account;",
        &[],
    );
    assert_eq!(array_values(&engine, &empty).len(), 0);

    let bad = instance(&mut engine, ACCOUNT);
    let empty_name = text(&mut engine, "");
    let ty = text(&mut engine, "com.example");
    assert!(engine
        .invoke_instance(
            bad,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[empty_name, ty],
        )
        .is_err());

    let bundle = instance(&mut engine, BUNDLE);
    call(&mut engine, bundle, "<init>", "()V", &[]);
    let bundle_key = text(&mut engine, "seed");
    let bundle_value = text(&mut engine, "from-bundle");
    call(
        &mut engine,
        bundle,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[bundle_key, bundle_value],
    );

    let acct = account(&mut engine, "alice@example.com", "com.example");
    assert_eq!(
        call_text(&mut engine, acct, "toString", "()Ljava/lang/String;", &[]),
        "Account {name=alice@example.com, type=com.example}"
    );
    let password = text(&mut engine, "secret");
    assert_eq!(
        call(
            &mut engine,
            manager,
            "addAccountExplicitly",
            "(Landroid/accounts/Account;Ljava/lang/String;Landroid/os/Bundle;)Z",
            &[Value::Obj(acct), password, Value::Obj(bundle)],
        ),
        Value::Int(1)
    );
    let duplicate = account(&mut engine, "alice@example.com", "com.example");
    assert_eq!(
        call(
            &mut engine,
            acct,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(duplicate)],
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "addAccountExplicitly",
            "(Landroid/accounts/Account;Ljava/lang/String;Landroid/os/Bundle;)Z",
            &[Value::Obj(duplicate), Value::Null, Value::Null],
        ),
        Value::Int(0)
    );

    let same_type = text(&mut engine, "com.example");
    let by_type = call(
        &mut engine,
        manager,
        "getAccountsByType",
        "(Ljava/lang/String;)[Landroid/accounts/Account;",
        &[same_type],
    );
    assert_eq!(array_values(&engine, &by_type), vec![Value::Obj(acct)]);
    let other_type = text(&mut engine, "com.other");
    let by_other_type = call(
        &mut engine,
        manager,
        "getAccountsByType",
        "(Ljava/lang/String;)[Landroid/accounts/Account;",
        &[other_type],
    );
    assert_eq!(array_values(&engine, &by_other_type).len(), 0);

    let lookup = account(&mut engine, "alice@example.com", "com.example");
    assert_eq!(
        call_opt_text(
            &mut engine,
            manager,
            "getPassword",
            "(Landroid/accounts/Account;)Ljava/lang/String;",
            &[Value::Obj(lookup)],
        ),
        Some("secret".to_string())
    );
    let seed = text(&mut engine, "seed");
    assert_eq!(
        call_opt_text(
            &mut engine,
            manager,
            "getUserData",
            "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Obj(lookup), seed],
        ),
        Some("from-bundle".to_string())
    );

    let key = text(&mut engine, "display");
    let value = text(&mut engine, "Alice A.");
    call(
        &mut engine,
        manager,
        "setUserData",
        "(Landroid/accounts/Account;Ljava/lang/String;Ljava/lang/String;)V",
        &[Value::Obj(acct), key.clone(), value],
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            manager,
            "getUserData",
            "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Obj(lookup), key],
        ),
        Some("Alice A.".to_string())
    );

    let token_type = text(&mut engine, "mail");
    let token = text(&mut engine, "token-123");
    call(
        &mut engine,
        manager,
        "setAuthToken",
        "(Landroid/accounts/Account;Ljava/lang/String;Ljava/lang/String;)V",
        &[Value::Obj(acct), token_type.clone(), token],
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            manager,
            "peekAuthToken",
            "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Obj(lookup), token_type],
        ),
        Some("token-123".to_string())
    );

    call(
        &mut engine,
        manager,
        "clearPassword",
        "(Landroid/accounts/Account;)V",
        &[Value::Obj(acct)],
    );
    assert_eq!(
        call_opt_text(
            &mut engine,
            manager,
            "getPassword",
            "(Landroid/accounts/Account;)Ljava/lang/String;",
            &[Value::Obj(lookup)],
        ),
        None
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "removeAccountExplicitly",
            "(Landroid/accounts/Account;)Z",
            &[Value::Obj(lookup)],
        ),
        Value::Int(1)
    );
    let after_remove = call(
        &mut engine,
        manager,
        "getAccounts",
        "()[Landroid/accounts/Account;",
        &[],
    );
    assert_eq!(array_values(&engine, &after_remove).len(), 0);

    // Deep audit: getUserData's cache recompute throws the service
    // null-check IAEs (it does not quietly return null). AOSP checks
    // the account first.
    match engine.invoke_instance(
        manager,
        "getUserData",
        "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;",
        &[Value::Null, Value::Null],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;")
        }
        other => panic!("expected IllegalArgumentException, got {other:?}"),
    }
    // setPasswordInternal deletes every cached auth token: after a
    // password change, peekAuthToken must observe nothing.
    let purge_account = account(&mut engine, "token-purge", "test.type");
    call(
        &mut engine,
        manager,
        "addAccountExplicitly",
        "(Landroid/accounts/Account;Ljava/lang/String;Landroid/os/Bundle;)Z",
        &[Value::Obj(purge_account), Value::Null, Value::Null],
    );
    let token_type = text(&mut engine, "oauth");
    let token = text(&mut engine, "tok-1");
    call(
        &mut engine,
        manager,
        "setAuthToken",
        "(Landroid/accounts/Account;Ljava/lang/String;Ljava/lang/String;)V",
        &[Value::Obj(purge_account), token_type.clone(), token],
    );
    let new_password = text(&mut engine, "s3cret");
    call(
        &mut engine,
        manager,
        "setPassword",
        "(Landroid/accounts/Account;Ljava/lang/String;)V",
        &[Value::Obj(purge_account), new_password],
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "peekAuthToken",
            "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Obj(purge_account), token_type],
        ),
        Value::Null
    );

    // Deep audit: toSafeName walks UTF-16 code units, so a
    // supplementary letter (a surrogate pair) is kept as-is instead
    // of being replaced like a BMP letter or digit.
    let bold_a = char::from_u32(0x1d56c).unwrap(); // supplementary letter
    let mixed_source = format!("a{bold_a}@example.com");
    let mixed_name = text(&mut engine, &mixed_source);
    assert_eq!(text_of(&engine, &mixed_name), mixed_source);
    let safe_name = call_static(
        &mut engine,
        ACCOUNT,
        "toSafeName",
        "(Ljava/lang/String;C)Ljava/lang/String;",
        &[mixed_name, Value::Int('x' as i32)],
    );
    assert_eq!(
        text_of(&engine, &safe_name),
        format!("x{bold_a}@xxxxxxx.xxx")
    );
}
