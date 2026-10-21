//! D8 may declare an inherited Android method on an app subclass.
#[macro_use]
mod common;
use common::*;
use rd_vm::{framework::HostObj, heap::HeapObj, Value, VmExit};
const ACC_CONSTRUCTOR: u32 = 0x1_0000;
fn fixture(override_id: bool) -> rd_vm::Engine {
    let mut b = DexBuilder::new();
    let custom = b.class("LCustomEdit;", "Landroid/widget/EditText;");
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    let pi = b.proto_idx("V", vec!["I".into()]);
    let pt = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".into()]);
    let setid = b.method_idx(
        if override_id {
            "Landroid/view/View;"
        } else {
            "LCustomEdit;"
        },
        pi,
        "setId",
    );
    let settext = b.method_idx("LCustomEdit;", pt, "setText");
    if override_id {
        let super_id = b.method_idx("Landroid/view/View;", pi, "setId");
        let mut code = op22b(0xd8, 2, 2, 1);
        code.extend(op35c(0x6f, 2, super_id, [1, 2, 0, 0, 0]));
        code.extend(op10x(0x0e));
        b.r#virtual(
            custom,
            "setId",
            "V",
            vec!["I"],
            ACC_PUBLIC,
            Some(b.code(3, 2, 2, code)),
        );
    }
    let text = b.intern("actual text") as u16;
    let mut code = op11n(0x12, 0, 7);
    code.extend(op35c(0x6e, 2, setid, [2, 0, 0, 0, 0]));
    code.extend(op21c(0x1a, 1, text));
    code.extend(op35c(0x6e, 2, settext, [2, 1, 0, 0, 0]));
    code.extend(op10x(0x0e));
    b.direct(
        caller,
        "run",
        "V",
        vec!["LCustomEdit;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 1, 2, code)),
    );
    engine_of(&b)
}
fn run(override_id: bool) {
    let mut e = fixture(override_id);
    let view = e
        .heap
        .alloc_instance("LCustomEdit;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCustomEdit;", view).unwrap();
    e.invoke_static("LCaso;", "run", "(LCustomEdit;)V", &[Value::Obj(view)])
        .unwrap();
    let expected = if override_id { 8 } else { 7 };
    assert!(matches!(e.fw.get(view),Some(HostObj::View{id,..}) if *id==expected));
    assert_eq!(
        e.invoke_instance(view, "getId", "()I", &[]).unwrap(),
        Value::Int(expected)
    );
    let text = e
        .invoke_instance(view, "getText", "()Ljava/lang/CharSequence;", &[])
        .unwrap();
    assert!(
        matches!(text,Value::Obj(r) if matches!(e.heap.get(r),Ok(HeapObj::Str(s)) if s=="actual text"))
    );
    let error = e
        .invoke_instance(view, "unrecognizedCall", "()V", &[])
        .unwrap_err();
    assert!(matches!(error,VmExit::Error(e) if e.code=="NOT_FOUND"));
}
#[test]
fn inherited_ids_and_text_use_concrete_models_not_generic_properties() {
    run(false);
}
#[test]
fn virtual_dex_override_wins_and_invoke_super_does_not_recurse() {
    run(true);
}
#[test]
fn inherited_android_view_nullable_property_defaults_apply_to_dex_subclasses() {
    let mut e = fixture(false);
    let view = e
        .heap
        .alloc_instance("LCustomEdit;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCustomEdit;", view).unwrap();
    assert_eq!(
        e.invoke_instance(
            view,
            "getForeground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        )
        .unwrap(),
        Value::Null
    );
}

#[test]
fn link_movement_method_is_canonical_and_round_trips_through_textview() {
    let mut e = engine_of(&DexBuilder::new());
    let class = "Landroid/text/method/LinkMovementMethod;";
    let sig = "()Landroid/text/method/MovementMethod;";
    let first = e.invoke_static(class, "getInstance", sig, &[]).unwrap();
    let second = e.invoke_static(class, "getInstance", sig, &[]).unwrap();
    assert_eq!(
        first, second,
        "getInstance returns a canonical platform object"
    );
    assert!(e
        .cp
        .is_subtype(class, "Landroid/text/method/MovementMethod;"));

    let view = e
        .heap
        .alloc_instance("Landroid/widget/TextView;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/TextView;", view)
        .unwrap();
    assert_eq!(
        e.invoke_instance(
            view,
            "setMovementMethod",
            "(Landroid/text/method/MovementMethod;)V",
            std::slice::from_ref(&first),
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(view, "getMovementMethod", sig, &[])
            .unwrap(),
        first
    );
}

#[test]
fn platform_underline_span_constructor_is_available_and_has_character_style_type() {
    let mut e = engine_of(&DexBuilder::new());
    let class = "Landroid/text/style/UnderlineSpan;";
    let span = e.heap.alloc_instance(class.into(), vec![]).unwrap();
    assert_eq!(
        e.invoke_instance(span, "<init>", "()V", &[]).unwrap(),
        Value::Null
    );
    assert!(e
        .cp
        .is_subtype(class, "Landroid/text/style/CharacterStyle;"));
}

#[test]
fn textview_wrap_content_respects_at_most_measure_specs() {
    let mut e = engine_of(&DexBuilder::new());
    let view = e
        .heap
        .alloc_instance("Landroid/widget/TextView;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/TextView;", view)
        .unwrap();
    let text = Value::Obj(e.heap.alloc_string("first\nsecond".into()).unwrap());
    e.invoke_instance(view, "setText", "(Ljava/lang/CharSequence;)V", &[text])
        .unwrap();
    e.invoke_instance(
        view,
        "measure",
        "(II)V",
        &[
            Value::Int(0x4000_0000 | 720),
            Value::Int(0x8000_0000u32 as i32 | 1000),
        ],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(96),
        "two explicit lines at wrap-content height must not consume the AT_MOST bound"
    );
}

#[test]
fn progressbar_wrap_content_respects_at_most_measure_specs() {
    let mut e = engine_of(&DexBuilder::new());
    let view = e
        .heap
        .alloc_instance("Landroid/widget/ProgressBar;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/ProgressBar;", view)
        .unwrap();
    e.invoke_instance(
        view,
        "measure",
        "(II)V",
        &[
            Value::Int(0x4000_0000 | 720),
            Value::Int(0x8000_0000u32 as i32 | 1000),
        ],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(48),
        "a wrap-content ProgressBar uses its compact intrinsic height"
    );
}

#[test]
fn inherited_activity_layout_inflater_uses_context_service() {
    let mut b = DexBuilder::new();
    b.class("LMyActivity;", "Landroid/app/Activity;");
    let mut e = engine_of(&b);
    let activity = e
        .heap
        .alloc_instance("LMyActivity;".into(), vec![])
        .unwrap();

    let inflater = e
        .invoke_instance(
            activity,
            "getLayoutInflater",
            "()Landroid/view/LayoutInflater;",
            &[],
        )
        .unwrap();
    let Value::Obj(inflater) = inflater else {
        panic!("Activity.getLayoutInflater must return a LayoutInflater")
    };
    assert_eq!(
        e.heap.get_field(inflater, "mContext").unwrap(),
        Value::Obj(activity),
        "the inflater must be associated with the requesting Activity"
    );
}

#[test]
fn reflect_modifier_predicates_and_string_use_java_flag_bits() {
    let mut e = engine_of(&DexBuilder::new());
    let class = "Ljava/lang/reflect/Modifier;";
    for (method, flags) in [
        ("isPublic", 0x0001),
        ("isPrivate", 0x0002),
        ("isProtected", 0x0004),
        ("isStatic", 0x0008),
        ("isFinal", 0x0010),
        ("isSynchronized", 0x0020),
        ("isVolatile", 0x0040),
        ("isTransient", 0x0080),
        ("isNative", 0x0100),
        ("isInterface", 0x0200),
        ("isAbstract", 0x0400),
        ("isStrict", 0x0800),
        ("isSynthetic", 0x1000),
        ("isAnnotation", 0x2000),
        ("isEnum", 0x4000),
        ("isMandated", 0x8000),
    ] {
        assert_eq!(
            e.invoke_static(class, method, "(I)Z", &[Value::Int(flags)])
                .unwrap(),
            Value::Int(1),
            "{method} must recognize {flags:#x}"
        );
        assert_eq!(
            e.invoke_static(class, method, "(I)Z", &[Value::Int(0)])
                .unwrap(),
            Value::Int(0),
            "{method} must reject zero flags"
        );
    }
    let text = e
        .invoke_static(
            class,
            "toString",
            "(I)Ljava/lang/String;",
            &[Value::Int(0x0411)],
        )
        .unwrap();
    let Value::Obj(text) = text else {
        panic!("Modifier.toString must return a String")
    };
    assert_eq!(e.heap.as_str(text).unwrap(), "public abstract final");
}

#[test]
fn string_tokenizer_handles_returned_delimiters_and_enumeration() {
    let mut e = engine_of(&DexBuilder::new());
    let tokenizer = e
        .heap
        .alloc_instance("Ljava/util/StringTokenizer;".into(), Vec::new())
        .unwrap();
    e.invoke_instance(
        tokenizer,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;Z)V",
        &[
            Value::StrPlaceholder("1,2-4".into()),
            Value::StrPlaceholder(",;-".into()),
            Value::Int(1),
        ],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(tokenizer, "countTokens", "()I", &[])
            .unwrap(),
        Value::Int(5)
    );

    for (method, expected, signature) in [
        ("nextToken", "1", "()Ljava/lang/String;"),
        ("nextToken", ",", "()Ljava/lang/String;"),
        ("nextElement", "2", "()Ljava/lang/Object;"),
        ("nextToken", "-", "()Ljava/lang/String;"),
        ("nextToken", "4", "()Ljava/lang/String;"),
    ] {
        let Value::Obj(token) = e
            .invoke_instance(tokenizer, method, signature, &[])
            .unwrap()
        else {
            panic!("{method} should return a String token");
        };
        assert_eq!(e.heap.as_str(token).unwrap(), expected);
    }
    assert_eq!(
        e.invoke_instance(tokenizer, "hasMoreElements", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(tokenizer, "countTokens", "()I", &[])
            .unwrap(),
        Value::Int(0)
    );

    let changing = e
        .heap
        .alloc_instance("Ljava/util/StringTokenizer;".into(), Vec::new())
        .unwrap();
    e.invoke_instance(
        changing,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[
            Value::StrPlaceholder("a,b;c".into()),
            Value::StrPlaceholder(",".into()),
        ],
    )
    .unwrap();
    let Value::Obj(first) = e
        .invoke_instance(changing, "nextToken", "()Ljava/lang/String;", &[])
        .unwrap()
    else {
        panic!("nextToken should return a String");
    };
    assert_eq!(e.heap.as_str(first).unwrap(), "a");
    let Value::Obj(rest) = e
        .invoke_instance(
            changing,
            "nextToken",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::StrPlaceholder("|".into())],
        )
        .unwrap()
    else {
        panic!("nextToken(delimiters) should return a String");
    };
    assert_eq!(e.heap.as_str(rest).unwrap(), ",b;c");
}

#[test]
fn broadcast_receiver_subclass_calls_platform_super_constructor() {
    let mut b = DexBuilder::new();
    let receiver_class = b.class("LCustomReceiver;", "Landroid/content/BroadcastReceiver;");
    let proto = b.proto_idx("V", vec![]);
    let super_ctor = b.method_idx("Landroid/content/BroadcastReceiver;", proto, "<init>");
    let mut code = op35c(0x70, 1, super_ctor, [0, 0, 0, 0, 0]);
    code.extend(op10x(0x0e));
    b.direct(
        receiver_class,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 0, code)),
    );

    let mut e = engine_of(&b);
    let receiver = e
        .heap
        .alloc_instance("LCustomReceiver;".into(), Vec::new())
        .unwrap();
    e.invoke_instance(receiver, "<init>", "()V", &[]).unwrap();
}

#[test]
fn androidx_fragment_super_constructor_executes_dex_initialization() {
    let mut b = DexBuilder::new();
    let fragment = b.class("Landroidx/fragment/app/Fragment;", "Ljava/lang/Object;");
    b.instance_field(fragment, "constructorMarker", "I");
    let marker = b.field_idx("Landroidx/fragment/app/Fragment;", "I", "constructorMarker");
    let mut fragment_ctor = op11n(0x12, 0, 7);
    fragment_ctor.extend(op22c(0x59, 0, 1, marker));
    fragment_ctor.extend(op10x(0x0e));
    b.direct(
        fragment,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 1, 0, fragment_ctor)),
    );

    let subclass = b.class("LCustomFragment;", "Landroidx/fragment/app/Fragment;");
    let proto = b.proto_idx("V", vec![]);
    let super_ctor = b.method_idx("Landroidx/fragment/app/Fragment;", proto, "<init>");
    let mut subclass_ctor = op35c(0x70, 1, super_ctor, [0, 0, 0, 0, 0]);
    subclass_ctor.extend(op10x(0x0e));
    b.direct(
        subclass,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 0, subclass_ctor)),
    );

    let mut e = engine_of(&b);
    let fragment = e
        .heap
        .alloc_instance("LCustomFragment;".into(), Vec::new())
        .unwrap();
    e.invoke_instance(fragment, "<init>", "()V", &[]).unwrap();
    assert_eq!(
        e.heap.get_field(fragment, "constructorMarker").unwrap(),
        Value::Int(7),
        "AndroidX Fragment's real DEX constructor must initialize its fields"
    );
}

#[test]
fn view_clickable_state_round_trips_independently_of_listeners() {
    let mut b = DexBuilder::new();
    b.class("LClickableView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LClickableView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LClickableView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "isClickable", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setClickable", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isClickable", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert!(matches!(
        e.fw.get(view),
        Some(HostObj::View {
            clickable: true,
            ..
        })
    ));

    e.invoke_instance(view, "setClickable", "(Z)V", &[Value::Int(0)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isClickable", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn pointer_icon_get_system_icon_returns_a_platform_object() {
    let mut e = engine_of(&DexBuilder::new());
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), Vec::new())
        .unwrap();
    let icon = rd_vm::framework::call_host_static(
        &mut e,
        "Landroid/view/PointerIcon;",
        "getSystemIcon",
        "(Landroid/content/Context;I)Landroid/view/PointerIcon;",
        &[Value::Obj(context), Value::Int(1000)],
    )
    .unwrap()
    .expect("PointerIcon factory should be handled by the Android host");
    let Value::Obj(icon) = icon else {
        panic!("PointerIcon.getSystemIcon should return a platform object");
    };
    assert_eq!(e.heap.class_of(icon).unwrap(), "Landroid/view/PointerIcon;");
}

#[test]
fn collections_empty_list_field_is_non_null_and_iterable() {
    let mut b = DexBuilder::new();
    let caller = b.class("LEmptyCollectionsField;", "Ljava/lang/Object;");
    let empty_list_field = b.field_idx("Ljava/util/Collections;", "Ljava/util/List;", "EMPTY_LIST");
    b.direct(
        caller,
        "emptyList",
        "Ljava/util/List;",
        vec![],
        ACC_PUBLIC | 0x0008,
        Some(b.code(1, 0, 0, {
            let mut code = op21c(0x62, 0, empty_list_field); // sget-object EMPTY_LIST
            code.extend(op11x(0x11, 0)); // return-object
            code
        })),
    );
    let mut e = engine_of(&b);
    let Value::Obj(empty_list) = e
        .invoke_static(
            "LEmptyCollectionsField;",
            "emptyList",
            "()Ljava/util/List;",
            &[],
        )
        .unwrap()
    else {
        panic!("Collections.EMPTY_LIST should be materialized as a List");
    };
    assert_eq!(
        e.invoke_instance(empty_list, "isEmpty", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    let Value::Obj(iterator) = e
        .invoke_instance(empty_list, "iterator", "()Ljava/util/Iterator;", &[])
        .unwrap()
    else {
        panic!("EMPTY_LIST.iterator should return an Iterator");
    };
    assert_eq!(
        e.invoke_instance(iterator, "hasNext", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn dex_abstract_list_iterator_dispatches_through_iterable_interface() {
    let mut b = DexBuilder::new();
    let list = b.class("LTestAbstractList;", "Ljava/util/AbstractList;");
    let caller = b.class("LIterableInvoke;", "Ljava/lang/Object;");
    let p_iterator = b.proto_idx("Ljava/util/Iterator;", vec![]);
    let queued = b.intern("queued") as u16;

    b.r#virtual(
        list,
        "size",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut code = op11n(0x12, 0, 1); // const/4 v0, 1
            code.extend(op11x(0x0f, 0)); // return v0
            code
        })),
    );
    b.r#virtual(
        list,
        "get",
        "Ljava/lang/Object;",
        vec!["I"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, {
            let mut code = op21c(0x1a, 0, queued); // const-string v0, "queued"
            code.extend(op11x(0x11, 0)); // return-object v0
            code
        })),
    );

    let iterable_iterator = b.method_idx("Ljava/lang/Iterable;", p_iterator, "iterator");
    b.direct(
        caller,
        "iteratorViaInterface",
        "Ljava/util/Iterator;",
        vec!["Ljava/lang/Iterable;"],
        ACC_PUBLIC | 0x0008,
        Some(b.code(1, 1, 1, {
            let mut code = op35c(0x72, 1, iterable_iterator, [0, 0, 0, 0, 0]);
            code.extend(op11x(0x0c, 0)); // move-result-object v0
            code.extend(op11x(0x11, 0)); // return-object v0
            code
        })),
    );

    let mut e = engine_of(&b);
    let list = e
        .heap
        .alloc_instance("LTestAbstractList;".into(), vec![])
        .unwrap();
    let Value::Obj(iterator) = e
        .invoke_static(
            "LIterableInvoke;",
            "iteratorViaInterface",
            "(Ljava/lang/Iterable;)Ljava/util/Iterator;",
            &[Value::Obj(list)],
        )
        .expect("AbstractList template should handle Iterable.iterator")
    else {
        panic!("Iterable.iterator should return a host iterator");
    };
    assert_eq!(
        e.invoke_instance(iterator, "hasNext", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    let Value::Obj(item) = e
        .invoke_instance(iterator, "next", "()Ljava/lang/Object;", &[])
        .unwrap()
    else {
        panic!("iterator should contain the DEX list item");
    };
    assert!(matches!(e.heap.get(item), Ok(HeapObj::Str(value)) if value == "queued"));
    assert_eq!(
        e.invoke_instance(iterator, "hasNext", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn view_group_default_check_layout_params_accepts_only_non_null() {
    let mut b = DexBuilder::new();
    b.class("LTestGroup;", "Landroid/view/ViewGroup;");
    let caller = b.class("LCheckLayoutParamsInvoke;", "Ljava/lang/Object;");
    let check_proto = b.proto_idx("Z", vec!["Landroid/view/ViewGroup$LayoutParams;".into()]);
    let check_method = b.method_idx("Landroid/view/ViewGroup;", check_proto, "checkLayoutParams");
    b.direct(
        caller,
        "isValid",
        "Z",
        vec![
            "Landroid/view/ViewGroup;",
            "Landroid/view/ViewGroup$LayoutParams;",
        ],
        ACC_PUBLIC | 0x0008,
        Some(b.code(2, 2, 2, {
            let mut code = op35c(0x6e, 2, check_method, [0, 1, 0, 0, 0]);
            code.extend(op11x(0x0a, 0)); // move-result v0
            code.extend(op11x(0x0f, 0)); // return v0
            code
        })),
    );

    let mut e = engine_of(&b);
    let group = e.heap.alloc_instance("LTestGroup;".into(), vec![]).unwrap();
    let params = e
        .heap
        .alloc_instance("Landroid/view/ViewGroup$LayoutParams;".into(), vec![])
        .unwrap();
    let call = |e: &mut rd_vm::Engine, params| {
        e.invoke_static(
            "LCheckLayoutParamsInvoke;",
            "isValid",
            "(Landroid/view/ViewGroup;Landroid/view/ViewGroup$LayoutParams;)Z",
            &[Value::Obj(group), params],
        )
        .unwrap()
    };
    assert_eq!(call(&mut e, Value::Obj(params)), Value::Int(1));
    assert_eq!(call(&mut e, Value::Null), Value::Int(0));
}

#[test]
fn view_group_add_view_generates_default_and_preserves_explicit_layout_params() {
    let mut b = DexBuilder::new();
    b.class("LTestAddGroup;", "Landroid/view/ViewGroup;");
    let caller = b.class("LAddViewInvoker;", "Ljava/lang/Object;");
    let add_proto = b.proto_idx("V", vec!["Landroid/view/View;".into()]);
    let add_method = b.method_idx("Landroid/view/ViewGroup;", add_proto, "addView");
    b.direct(
        caller,
        "add",
        "V",
        vec!["LTestAddGroup;", "Landroid/view/View;"],
        ACC_PUBLIC | 0x0008,
        Some(b.code(2, 2, 2, {
            let mut code = op35c(0x6e, 2, add_method, [0, 1, 0, 0, 0]);
            code.extend(op10x(0x0e));
            code
        })),
    );

    let mut e = engine_of(&b);
    let parent = e
        .heap
        .alloc_instance("LTestAddGroup;".into(), vec![])
        .unwrap();
    let child = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    e.attach_host_state("LTestAddGroup;", parent).unwrap();
    e.attach_host_state("Landroid/view/View;", child).unwrap();
    e.invoke_static(
        "LAddViewInvoker;",
        "add",
        "(LTestAddGroup;Landroid/view/View;)V",
        &[Value::Obj(parent), Value::Obj(child)],
    )
    .unwrap();
    let Value::Obj(default_params) = e
        .invoke_instance(
            child,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .unwrap()
    else {
        panic!("addView(child) should generate default LayoutParams");
    };
    assert_eq!(
        e.heap.class_of(default_params).unwrap(),
        "Landroid/view/ViewGroup$LayoutParams;"
    );

    let explicit_child = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    let explicit_params = e
        .heap
        .alloc_instance("Landroid/view/ViewGroup$LayoutParams;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/view/View;", explicit_child)
        .unwrap();
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
        &[Value::Obj(explicit_child), Value::Obj(explicit_params)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(
            explicit_child,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .unwrap(),
        Value::Obj(explicit_params)
    );
}

#[test]
fn view_group_measure_child_with_margins_derives_android_measure_specs() {
    let mut b = DexBuilder::new();
    b.class("LTestMeasureGroup;", "Landroid/view/ViewGroup;");
    let caller = b.class("LMeasureChildInvoker;", "Ljava/lang/Object;");
    let measure_proto = b.proto_idx(
        "V",
        vec![
            "Landroid/view/View;".into(),
            "I".into(),
            "I".into(),
            "I".into(),
            "I".into(),
        ],
    );
    let measure_method = b.method_idx(
        "Landroid/view/ViewGroup;",
        measure_proto,
        "measureChildWithMargins",
    );
    b.direct(
        caller,
        "measure",
        "V",
        vec![
            "Landroid/view/ViewGroup;",
            "Landroid/view/View;",
            "I",
            "I",
            "I",
            "I",
        ],
        ACC_PUBLIC | 0x0008,
        Some(b.code(6, 6, 6, {
            // invoke-virtual/range {v0..v5}
            let mut code = vec![((6 << 8) | 0x74), measure_method as u16, 0];
            code.extend(op10x(0x0e));
            code
        })),
    );

    let mut e = engine_of(&b);
    let parent = e
        .heap
        .alloc_instance("LTestMeasureGroup;".into(), vec![])
        .unwrap();
    let child = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    let params = e
        .heap
        .alloc_instance("Landroid/view/ViewGroup$MarginLayoutParams;".into(), vec![])
        .unwrap();
    e.attach_host_state("LTestMeasureGroup;", parent).unwrap();
    e.attach_host_state("Landroid/view/View;", child).unwrap();
    e.invoke_instance(
        parent,
        "setPadding",
        "(IIII)V",
        &[Value::Int(10), Value::Int(3), Value::Int(14), Value::Int(5)],
    )
    .unwrap();
    for (field, value) in [
        ("width", -1),
        ("height", -2),
        ("leftMargin", 2),
        ("topMargin", 1),
        ("rightMargin", 4),
        ("bottomMargin", 3),
    ] {
        e.heap.put_field(params, field, Value::Int(value)).unwrap();
    }
    e.heap
        .put_field(child, "mLayoutParams", Value::Obj(params))
        .unwrap();

    e.invoke_static(
        "LMeasureChildInvoker;",
        "measure",
        "(Landroid/view/ViewGroup;Landroid/view/View;IIII)V",
        &[
            Value::Obj(parent),
            Value::Obj(child),
            Value::Int(0x4000_012c), // parent width: EXACTLY 300
            Value::Int(8),           // width already consumed by siblings
            Value::Int((0x8000_0000u32 | 200) as i32), // parent height: AT_MOST 200
            Value::Int(7),           // height already consumed
        ],
    )
    .unwrap();

    // Width: 300 - 10 - 14 - 2 - 4 - 8 = 262, MATCH_PARENT under EXACTLY.
    assert_eq!(
        e.invoke_instance(child, "getMeasuredWidth", "()I", &[])
            .unwrap(),
        Value::Int(262)
    );
    // Height: 200 - 3 - 5 - 1 - 3 - 7 = 181, WRAP_CONTENT under AT_MOST.
    assert_eq!(
        e.invoke_instance(child, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(181)
    );
}

#[test]
fn viewgroup_static_get_child_measure_spec_handles_inherited_owners() {
    let mut b = DexBuilder::new();
    b.class("LTestToolbar;", "Landroid/widget/FrameLayout;");
    let mut e = engine_of(&b);
    let at_most = (0x8000_0000u32 | 300) as i32;
    let cases = [
        (0x4000_012c, 20, -1, 0x4000_0118), // exact + match-parent
        (0x4000_012c, 20, -2, (0x8000_0000u32 | 280) as i32), // exact + wrap
        (at_most, 20, -1, (0x8000_0000u32 | 280) as i32), // at-most + match
        (0, 20, -1, 0),                     // unspecified + match-parent
        (at_most, 20, 42, 0x4000_002a),     // fixed child dimension
    ];
    for (parent_spec, padding, child_dimension, expected) in cases {
        assert_eq!(
            e.invoke_static(
                "LTestToolbar;",
                "getChildMeasureSpec",
                "(III)I",
                &[
                    Value::Int(parent_spec),
                    Value::Int(padding),
                    Value::Int(child_dimension),
                ],
            )
            .unwrap(),
            Value::Int(expected)
        );
    }
}

#[test]
fn view_static_measure_helpers_accept_custom_subclass_owners() {
    let mut b = DexBuilder::new();
    b.class("LTestView;", "Landroid/widget/FrameLayout;");
    let mut e = engine_of(&b);
    for (spec, expected) in [
        (0x4000_00c8, 200),
        ((0x8000_0000u32 | 200) as i32, 200),
        (0, 32),
    ] {
        assert_eq!(
            e.invoke_static(
                "LTestView;",
                "getDefaultSize",
                "(II)I",
                &[Value::Int(32), Value::Int(spec)],
            )
            .unwrap(),
            Value::Int(expected)
        );
    }
    assert_eq!(
        e.invoke_static(
            "LTestView;",
            "resolveSize",
            "(II)I",
            &[Value::Int(240), Value::Int((0x8000_0000u32 | 200) as i32)],
        )
        .unwrap(),
        Value::Int(200)
    );
    assert_eq!(
        e.invoke_static(
            "LTestView;",
            "combineMeasuredStates",
            "(II)I",
            &[Value::Int(0x100), Value::Int(0x200)],
        )
        .unwrap(),
        Value::Int(0x300)
    );
}

#[test]
fn fits_system_windows_defaults_false_and_round_trips_on_custom_views() {
    let mut b = DexBuilder::new();
    b.class("LTestFitsView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LTestFitsView;".into(), vec![])
        .unwrap();
    e.attach_host_state("LTestFitsView;", view).unwrap();
    assert_eq!(
        e.invoke_instance(view, "getFitsSystemWindows", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setFitsSystemWindows", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getFitsSystemWindows", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(view, "setFitsSystemWindows", "(Z)V", &[Value::Int(0)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getFitsSystemWindows", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
}

#[test]
fn typed_value_apply_dimension_uses_display_metrics_units() {
    let mut e = engine_of(&DexBuilder::new());
    let metrics = e
        .heap
        .alloc_instance("Landroid/util/DisplayMetrics;".into(), vec![])
        .unwrap();
    for (field, value) in [("density", 1.75), ("scaledDensity", 2.0), ("xdpi", 280.0)] {
        e.heap
            .put_field(metrics, field, Value::Float(value))
            .unwrap();
    }
    let cases = [
        (0, 12.0, 12.0),  // px
        (1, 12.0, 21.0),  // dp
        (2, 12.0, 24.0),  // sp
        (3, 72.0, 280.0), // points
        (4, 1.0, 280.0),  // inches
        (5, 25.4, 280.0), // millimetres
        (99, 12.0, 0.0),  // unknown unit
    ];
    for (unit, value, expected) in cases {
        let Value::Float(actual) = e
            .invoke_static(
                "Landroid/util/TypedValue;",
                "applyDimension",
                "(IFLandroid/util/DisplayMetrics;)F",
                &[Value::Int(unit), Value::Float(value), Value::Obj(metrics)],
            )
            .unwrap()
        else {
            panic!("applyDimension should return float");
        };
        assert!(
            (actual - expected).abs() < 0.001,
            "unit {unit}: {actual} != {expected}"
        );
    }
}

#[test]
fn gravity_get_absolute_gravity_resolves_start_and_end_for_layout_direction() {
    let mut e = engine_of(&DexBuilder::new());
    let cases = [
        (0x0080_0003, 0, 0x3),  // START in LTR -> LEFT
        (0x0080_0003, 1, 0x5),  // START in RTL -> RIGHT
        (0x0080_0005, 0, 0x5),  // END in LTR -> RIGHT
        (0x0080_0005, 1, 0x3),  // END in RTL -> LEFT
        (0x0080_0033, 1, 0x35), // preserve CENTER_VERTICAL
        (0x35, 1, 0x35),        // absolute gravity remains unchanged
    ];
    for (gravity, direction, expected) in cases {
        assert_eq!(
            e.invoke_static(
                "Landroid/view/Gravity;",
                "getAbsoluteGravity",
                "(II)I",
                &[Value::Int(gravity), Value::Int(direction)],
            )
            .unwrap(),
            Value::Int(expected)
        );
    }
}

#[test]
fn inherited_attach_queries_work_for_dex_owned_view_subclasses() {
    let mut b = DexBuilder::new();
    let main = b.class("LMain;", "Landroid/app/Activity;");
    let custom = b.class("LCustomView;", "Landroid/view/View;");
    let view_field = b.field_idx("LMain;", "LCustomView;", "view");
    b.instance_field(main, "view", "LCustomView;");
    let empty_proto = b.proto_idx("V", vec![]);
    let activity_view_proto = b.proto_idx("V", vec!["Landroid/view/View;".into()]);
    let view_ctor = b.method_idx("Landroid/view/View;", empty_proto, "<init>");
    let custom_ctor = b.method_idx("LCustomView;", empty_proto, "<init>");
    let set_content = b.method_idx(
        "Landroid/app/Activity;",
        activity_view_proto,
        "setContentView",
    );
    b.direct(
        custom,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut code = op35c(0x70, 1, view_ctor, [0, 0, 0, 0, 0]);
            code.extend(op10x(0x0E));
            code
        })),
    );
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut code = op21c(0x22, 0, b.type_idx_cached("LCustomView;"));
            code.extend(op35c(0x70, 1, custom_ctor, [0, 0, 0, 0, 0]));
            code.extend(op22c(0x5B, 0, 2, view_field));
            code.extend(op35c(0x6E, 2, set_content, [2, 0, 0, 0, 0]));
            code.extend(op10x(0x0E));
            code
        })),
    );
    let mut e = engine_of(&b);
    let activity = e.launch_app("LMain;", "org.example", Vec::new()).unwrap();
    let view = e
        .heap
        .get_field(activity, "view")
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isAttachedToWindow", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert!(matches!(
        e.invoke_instance(view, "getWindowToken", "()Landroid/os/IBinder;", &[])
            .unwrap(),
        Value::Obj(_)
    ));
}

#[test]
fn will_not_draw_uses_viewgroup_default_and_explicit_override() {
    let mut e = engine_of(&DexBuilder::new());
    let group = e
        .heap
        .alloc_instance("Landroid/widget/LinearLayout;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/LinearLayout;", group)
        .unwrap();
    assert_eq!(
        e.invoke_instance(group, "willNotDraw", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(group, "setWillNotDraw", "(Z)V", &[Value::Int(0)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(group, "willNotDraw", "()Z", &[]).unwrap(),
        Value::Int(0)
    );

    let view = e
        .heap
        .alloc_instance("Landroid/widget/TextView;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/TextView;", view)
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "willNotDraw", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn gravity_apply_positions_and_fills_rects_in_ltr_and_rtl() {
    let mut e = engine_of(&DexBuilder::new());
    let container = e
        .heap
        .alloc_instance("Landroid/graphics/Rect;".into(), vec![])
        .unwrap();
    let out = e
        .heap
        .alloc_instance("Landroid/graphics/Rect;".into(), vec![])
        .unwrap();
    for (field, value) in [("left", 10), ("top", 20), ("right", 110), ("bottom", 220)] {
        e.heap
            .put_field(container, field, Value::Int(value))
            .unwrap();
    }
    let cases = [
        (0x0080_0013, 0, [10, 100, 30, 140]), // START + CENTER_VERTICAL, LTR
        (0x0080_0013, 1, [90, 100, 110, 140]), // START + CENTER_VERTICAL, RTL
        (0x77, 0, [10, 20, 110, 220]),        // FILL
    ];
    for (gravity, direction, expected) in cases {
        e.invoke_static(
            "Landroid/view/Gravity;",
            "apply",
            "(IIILandroid/graphics/Rect;Landroid/graphics/Rect;I)V",
            &[
                Value::Int(gravity),
                Value::Int(20),
                Value::Int(40),
                Value::Obj(container),
                Value::Obj(out),
                Value::Int(direction),
            ],
        )
        .unwrap();
        let actual = ["left", "top", "right", "bottom"]
            .map(|field| e.heap.get_field(out, field).unwrap().as_int().unwrap());
        assert_eq!(actual, expected);
    }
}

#[test]
fn frame_layout_measures_and_positions_children_with_padding_and_margins() {
    let mut e = engine_of(&DexBuilder::new());
    let parent = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout;".into(), vec![])
        .unwrap();
    let child = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    let params = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout$LayoutParams;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/FrameLayout;", parent)
        .unwrap();
    e.attach_host_state("Landroid/view/View;", child).unwrap();
    e.invoke_instance(
        parent,
        "setPadding",
        "(IIII)V",
        &[
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
        ],
    )
    .unwrap();
    for (field, value) in [
        ("width", 100),
        ("height", 80),
        ("leftMargin", 5),
        ("topMargin", 6),
        ("rightMargin", 7),
        ("bottomMargin", 8),
    ] {
        e.heap.put_field(params, field, Value::Int(value)).unwrap();
    }
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
        &[Value::Obj(child), Value::Obj(params)],
    )
    .unwrap();

    e.invoke_instance(
        parent,
        "measure",
        "(II)V",
        &[Value::Int(0x4000_012c), Value::Int(0x4000_00c8)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(parent, "getMeasuredWidth", "()I", &[])
            .unwrap(),
        Value::Int(300)
    );
    assert_eq!(
        e.invoke_instance(child, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(80)
    );

    e.invoke_instance(
        parent,
        "layout",
        "(IIII)V",
        &[
            Value::Int(0),
            Value::Int(0),
            Value::Int(300),
            Value::Int(200),
        ],
    )
    .unwrap();
    assert!(matches!(
        e.fw.get(child),
        Some(HostObj::View {
            x: 15,
            y: 26,
            w: 100,
            h: 80,
            ..
        })
    ));
}

#[test]
fn scroll_view_measures_content_beyond_its_viewport() {
    let mut e = engine_of(&DexBuilder::new());
    let scroll = e
        .heap
        .alloc_instance("Landroid/widget/ScrollView;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/ScrollView;", scroll)
        .unwrap();
    let content = e
        .heap
        .alloc_instance("Landroid/widget/LinearLayout;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/LinearLayout;", content)
        .unwrap();
    e.invoke_instance(content, "setOrientation", "(I)V", &[Value::Int(1)])
        .unwrap();

    for index in 0..12 {
        let row = e
            .heap
            .alloc_instance("Landroid/widget/TextView;".into(), vec![])
            .unwrap();
        e.attach_host_state("Landroid/widget/TextView;", row)
            .unwrap();
        let text = Value::Obj(e.heap.alloc_string(format!("row {index}")).unwrap());
        e.invoke_instance(row, "setText", "(Ljava/lang/CharSequence;)V", &[text])
            .unwrap();
        let params = e
            .heap
            .alloc_instance("Landroid/widget/LinearLayout$LayoutParams;".into(), vec![])
            .unwrap();
        e.heap.put_field(params, "width", Value::Int(-1)).unwrap();
        e.heap.put_field(params, "height", Value::Int(-2)).unwrap();
        e.invoke_instance(
            content,
            "addView",
            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
            &[Value::Obj(row), Value::Obj(params)],
        )
        .unwrap();
    }
    let content_params = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout$LayoutParams;".into(), vec![])
        .unwrap();
    e.heap
        .put_field(content_params, "width", Value::Int(-1))
        .unwrap();
    e.heap
        .put_field(content_params, "height", Value::Int(-2))
        .unwrap();
    e.invoke_instance(
        scroll,
        "addView",
        "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
        &[Value::Obj(content), Value::Obj(content_params)],
    )
    .unwrap();

    e.invoke_instance(
        scroll,
        "measure",
        "(II)V",
        &[Value::Int(0x4000_0000 | 720), Value::Int(0x4000_0000 | 400)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(scroll, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(400),
        "the ScrollView itself stays at the viewport height"
    );
    assert_eq!(
        e.invoke_instance(content, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(12 * 48),
        "the scroll child must receive an unspecified height and retain all rows"
    );
}

#[test]
fn linear_layout_measures_and_stacks_children_on_its_orientation_axis() {
    let mut e = engine_of(&DexBuilder::new());
    let parent = e
        .heap
        .alloc_instance("Landroid/widget/LinearLayout;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/LinearLayout;", parent)
        .unwrap();
    e.invoke_instance(parent, "setOrientation", "(I)V", &[Value::Int(1)])
        .unwrap();
    e.invoke_instance(
        parent,
        "setPadding",
        "(IIII)V",
        &[Value::Int(10), Value::Int(2), Value::Int(20), Value::Int(3)],
    )
    .unwrap();

    let mut children = Vec::new();
    for top_margin in [1, 2] {
        let child = e
            .heap
            .alloc_instance("Landroid/view/View;".into(), vec![])
            .unwrap();
        let params = e
            .heap
            .alloc_instance("Landroid/widget/LinearLayout$LayoutParams;".into(), vec![])
            .unwrap();
        e.attach_host_state("Landroid/view/View;", child).unwrap();
        for (field, value) in [
            ("width", -1),
            ("height", 30),
            ("leftMargin", 0),
            ("topMargin", top_margin),
            ("rightMargin", 0),
            ("bottomMargin", 0),
        ] {
            e.heap.put_field(params, field, Value::Int(value)).unwrap();
        }
        e.invoke_instance(
            parent,
            "addView",
            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
            &[Value::Obj(child), Value::Obj(params)],
        )
        .unwrap();
        children.push(child);
    }

    e.invoke_instance(
        parent,
        "measure",
        "(II)V",
        &[Value::Int(0x4000_012c), Value::Int(0x4000_00c8)],
    )
    .unwrap();
    for child in &children {
        assert_eq!(
            e.invoke_instance(*child, "getMeasuredWidth", "()I", &[])
                .unwrap(),
            Value::Int(270)
        );
        assert_eq!(
            e.invoke_instance(*child, "getMeasuredHeight", "()I", &[])
                .unwrap(),
            Value::Int(30)
        );
    }

    e.invoke_instance(
        parent,
        "layout",
        "(IIII)V",
        &[
            Value::Int(0),
            Value::Int(0),
            Value::Int(300),
            Value::Int(200),
        ],
    )
    .unwrap();
    assert!(matches!(
        e.fw.get(children[0]),
        Some(HostObj::View { x: 10, y: 3, .. })
    ));
    assert!(matches!(
        e.fw.get(children[1]),
        Some(HostObj::View { x: 10, y: 35, .. })
    ));
}

#[test]
fn linear_layout_distributes_weights_before_measuring_zero_width_children() {
    let mut e = engine_of(&DexBuilder::new());
    let parent = e
        .heap
        .alloc_instance("Landroid/widget/LinearLayout;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/LinearLayout;", parent)
        .unwrap();
    e.invoke_instance(parent, "setOrientation", "(I)V", &[Value::Int(0)])
        .unwrap();

    let mut columns = Vec::new();
    for (weight, text) in [
        (1.0f32, "Username: user"),
        (2.0f32, "Public key auth: false"),
    ] {
        let column = e
            .heap
            .alloc_instance("Landroid/widget/LinearLayout;".into(), vec![])
            .unwrap();
        e.attach_host_state("Landroid/widget/LinearLayout;", column)
            .unwrap();
        e.invoke_instance(column, "setOrientation", "(I)V", &[Value::Int(1)])
            .unwrap();

        let label = e
            .heap
            .alloc_instance("Landroid/widget/TextView;".into(), vec![])
            .unwrap();
        e.attach_host_state("Landroid/widget/TextView;", label)
            .unwrap();
        let text = Value::Obj(e.heap.alloc_string(text.into()).unwrap());
        e.invoke_instance(label, "setText", "(Ljava/lang/CharSequence;)V", &[text])
            .unwrap();
        let label_params = e
            .heap
            .alloc_instance("Landroid/widget/LinearLayout$LayoutParams;".into(), vec![])
            .unwrap();
        e.heap
            .put_field(label_params, "width", Value::Int(-1))
            .unwrap();
        e.heap
            .put_field(label_params, "height", Value::Int(-2))
            .unwrap();
        e.invoke_instance(
            column,
            "addView",
            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
            &[Value::Obj(label), Value::Obj(label_params)],
        )
        .unwrap();

        let column_params = e
            .heap
            .alloc_instance("Landroid/widget/LinearLayout$LayoutParams;".into(), vec![])
            .unwrap();
        e.heap
            .put_field(column_params, "width", Value::Int(0))
            .unwrap();
        e.heap
            .put_field(column_params, "height", Value::Int(-1))
            .unwrap();
        e.heap
            .put_field(column_params, "weight", Value::Float(weight))
            .unwrap();
        e.invoke_instance(
            parent,
            "addView",
            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
            &[Value::Obj(column), Value::Obj(column_params)],
        )
        .unwrap();
        columns.push(column);
    }

    e.invoke_instance(
        parent,
        "measure",
        "(II)V",
        &[
            Value::Int(0x4000_0000 | 720),
            Value::Int(0x8000_0000u32 as i32 | 1000),
        ],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(parent, "getMeasuredHeight", "()I", &[])
            .unwrap(),
        Value::Int(48),
        "weighted columns must receive width before their wrap-content labels are measured"
    );
    assert_eq!(
        e.invoke_instance(columns[0], "getMeasuredWidth", "()I", &[])
            .unwrap(),
        Value::Int(240)
    );
    assert_eq!(
        e.invoke_instance(columns[1], "getMeasuredWidth", "()I", &[])
            .unwrap(),
        Value::Int(480)
    );

    e.invoke_instance(
        parent,
        "layout",
        "(IIII)V",
        &[
            Value::Int(0),
            Value::Int(0),
            Value::Int(720),
            Value::Int(48),
        ],
    )
    .unwrap();
    assert!(matches!(
        e.fw.get(columns[0]),
        Some(HostObj::View {
            x: 0,
            w: 240,
            h: 48,
            ..
        })
    ));
    assert!(matches!(
        e.fw.get(columns[1]),
        Some(HostObj::View {
            x: 240,
            w: 480,
            h: 48,
            ..
        })
    ));
}

#[test]
fn view_compute_system_window_insets_clears_local_rect_and_returns_consumed_insets() {
    let mut b = DexBuilder::new();
    b.class("LTestInsetsView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LTestInsetsView;".into(), vec![])
        .unwrap();
    let insets = e
        .heap
        .alloc_instance("Landroid/view/WindowInsets;".into(), vec![])
        .unwrap();
    let local = e
        .heap
        .alloc_instance("Landroid/graphics/Rect;".into(), vec![])
        .unwrap();
    e.attach_host_state("LTestInsetsView;", view).unwrap();
    assert_eq!(
        e.invoke_instance(
            view,
            "getRootWindowInsets",
            "()Landroid/view/WindowInsets;",
            &[]
        )
        .unwrap(),
        Value::Null
    );
    for field in ["left", "top", "right", "bottom"] {
        e.heap.put_field(local, field, Value::Int(9)).unwrap();
    }
    assert_eq!(
        e.invoke_instance(
            view,
            "dispatchApplyWindowInsets",
            "(Landroid/view/WindowInsets;)Landroid/view/WindowInsets;",
            &[Value::Obj(insets)],
        )
        .unwrap(),
        Value::Obj(insets)
    );

    assert_eq!(
        e.invoke_instance(
            view,
            "computeSystemWindowInsets",
            "(Landroid/view/WindowInsets;Landroid/graphics/Rect;)Landroid/view/WindowInsets;",
            &[Value::Obj(insets), Value::Obj(local)],
        )
        .unwrap(),
        Value::Obj(insets)
    );
    for field in ["left", "top", "right", "bottom"] {
        assert_eq!(e.heap.get_field(local, field).unwrap(), Value::Int(0));
    }
}

#[test]
fn objects_equals_uses_guest_equals_and_handles_null_references() {
    let mut b = DexBuilder::new();
    let equals_class = b.class("LGuestEqualsValue;", "Ljava/lang/Object;");
    b.r#virtual(
        equals_class,
        "equals",
        "Z",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 0, {
            let mut code = op11n(0x12, 0, 1); // const/4 v0, true
            code.extend(op11x(0x0f, 0)); // return v0
            code
        })),
    );
    let mut e = engine_of(&b);
    let left = e
        .heap
        .alloc_instance("LGuestEqualsValue;".into(), vec![])
        .unwrap();
    let right = e
        .heap
        .alloc_instance("LGuestEqualsValue;".into(), vec![])
        .unwrap();
    assert_eq!(
        e.invoke_static(
            "Ljava/util/Objects;",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            &[Value::Obj(left), Value::Obj(right)],
        )
        .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_static(
            "Ljava/util/Objects;",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            &[Value::Null, Value::Int(0)],
        )
        .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_static(
            "Ljava/util/Objects;",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            &[Value::Null, Value::Obj(right)],
        )
        .unwrap(),
        Value::Int(0)
    );
}

#[test]
fn objects_require_non_null_returns_identity_and_preserves_optional_message() {
    let mut e = engine_of(&DexBuilder::new());
    let object = e
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    assert_eq!(
        e.invoke_static(
            "Ljava/util/Objects;",
            "requireNonNull",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[Value::Obj(object)],
        )
        .unwrap(),
        Value::Obj(object)
    );

    let message = e.heap.alloc_string("required object".into()).unwrap();
    match e.invoke_static(
        "Ljava/util/Objects;",
        "requireNonNull",
        "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        &[Value::Null, Value::Obj(message)],
    ) {
        Err(VmExit::Exception(error)) => {
            assert_eq!(error.class, "Ljava/lang/NullPointerException;");
            assert_eq!(error.message.as_deref(), Some("required object"));
        }
        other => panic!("expected NullPointerException, got {other:?}"),
    }

    match e.invoke_static(
        "Ljava/util/Objects;",
        "requireNonNull",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Null],
    ) {
        Err(VmExit::Exception(error)) => {
            assert_eq!(error.class, "Ljava/lang/NullPointerException;");
            assert_eq!(error.message, None);
        }
        other => panic!("expected NullPointerException, got {other:?}"),
    }
}

#[test]
fn view_on_touch_listener_is_registered_dispatched_and_cleared() {
    let mut b = DexBuilder::new();
    let listener = b.class("LTestTouchListener;", "Ljava/lang/Object;");
    let registrar = b.class("LTouchListenerRegistrar;", "Ljava/lang/Object;");
    b.r#virtual(
        listener,
        "onTouch",
        "Z",
        vec!["Landroid/view/View;", "Landroid/view/MotionEvent;"],
        ACC_PUBLIC,
        Some(b.code(3, 3, 0, {
            let mut code = op11n(0x12, 0, 1); // const/4 v0, true
            code.extend(op11x(0x0f, 0)); // return v0
            code
        })),
    );
    let set_listener_proto = b.proto_idx("V", vec!["Landroid/view/View$OnTouchListener;".into()]);
    let set_listener = b.method_idx(
        "Landroid/view/View;",
        set_listener_proto,
        "setOnTouchListener",
    );
    b.direct(
        registrar,
        "register",
        "V",
        vec!["Landroid/view/View;", "Landroid/view/View$OnTouchListener;"],
        ACC_PUBLIC | 0x0008,
        Some(b.code(2, 2, 2, {
            let mut code = op35c(0x6e, 2, set_listener, [0, 1, 0, 0, 0]);
            code.extend(op10x(0x0e)); // return-void
            code
        })),
    );

    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/view/View;", view).unwrap();
    let listener = e
        .heap
        .alloc_instance("LTestTouchListener;".into(), vec![])
        .unwrap();
    let register = |e: &mut rd_vm::Engine, value| {
        e.invoke_static(
            "LTouchListenerRegistrar;",
            "register",
            "(Landroid/view/View;Landroid/view/View$OnTouchListener;)V",
            &[Value::Obj(view), value],
        )
        .unwrap();
    };
    register(&mut e, Value::Obj(listener));
    assert!(e.touch_sequence(view, &[(10, 10)]).unwrap());
    register(&mut e, Value::Null);
    assert!(!e.touch_sequence(view, &[(10, 10)]).unwrap());
}

#[test]
fn activity_window_callback_returns_default_null_panel_view() {
    let mut b = DexBuilder::new();
    let caller = b.class("LWindowCallbackInvoke;", "Ljava/lang/Object;");
    let panel_view = b.proto_idx("Landroid/view/View;", vec!["I".into()]);
    let callback_method = b.method_idx(
        "Landroid/view/Window$Callback;",
        panel_view,
        "onCreatePanelView",
    );
    b.direct(
        caller,
        "createPanelView",
        "Landroid/view/View;",
        vec!["Landroid/view/Window$Callback;", "I"],
        ACC_PUBLIC | 0x0008,
        Some(b.code(2, 2, 2, {
            let mut code = op35c(0x72, 2, callback_method, [0, 1, 0, 0, 0]);
            code.extend(op11x(0x0c, 0)); // move-result-object v0
            code.extend(op11x(0x11, 0)); // return-object v0
            code
        })),
    );

    let mut e = engine_of(&b);
    let activity = e
        .heap
        .alloc_instance("Landroid/app/Activity;".into(), vec![])
        .unwrap();
    assert_eq!(
        e.invoke_static(
            "LWindowCallbackInvoke;",
            "createPanelView",
            "(Landroid/view/Window$Callback;I)Landroid/view/View;",
            &[Value::Obj(activity), Value::Int(0)],
        )
        .unwrap(),
        Value::Null
    );
}

#[test]
fn reflection_invokes_platform_static_main_looper_method() {
    let mut b = DexBuilder::new();
    let looper_method = b.proto_idx("Landroid/os/Looper;", vec![]);
    b.method_idx("Landroid/os/Looper;", looper_method, "getMainLooper");
    let mut e = engine_of(&b);

    let Value::Obj(looper_class) = e
        .invoke_static(
            "Ljava/lang/Class;",
            "forName",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[Value::StrPlaceholder("android.os.Looper".into())],
        )
        .unwrap()
    else {
        panic!("Class.forName should return Looper's Class object");
    };
    let Value::Obj(method) = e
        .invoke_instance(
            looper_class,
            "getDeclaredMethod",
            "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            &[Value::StrPlaceholder("getMainLooper".into()), Value::Null],
        )
        .unwrap()
    else {
        panic!("getDeclaredMethod should return a Method");
    };
    let Value::Obj(main_looper) = e
        .invoke_instance(
            method,
            "invoke",
            "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
            &[Value::Null, Value::Null],
        )
        .unwrap()
    else {
        panic!("Method.invoke should return the main Looper");
    };
    assert_eq!(e.heap.class_of(main_looper).unwrap(), "Landroid/os/Looper;");
    assert_eq!(
        e.invoke_static(
            "Landroid/os/Looper;",
            "getMainLooper",
            "()Landroid/os/Looper;",
            &[],
        )
        .unwrap(),
        Value::Obj(main_looper),
        "direct and reflective calls should share the canonical Looper"
    );
}

#[test]
fn view_selected_state_round_trips_independently() {
    let mut b = DexBuilder::new();
    b.class("LSelectedView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LSelectedView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LSelectedView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "isSelected", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setSelected", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isSelected", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert!(matches!(
        e.fw.get(view),
        Some(HostObj::View { selected: true, .. })
    ));
    assert_eq!(
        e.invoke_instance(view, "isEnabled", "()Z", &[]).unwrap(),
        Value::Int(1),
        "selected state must not overwrite enabled state"
    );
}

#[test]
fn view_activated_state_round_trips_independently_of_selected_and_enabled() {
    let mut b = DexBuilder::new();
    b.class("LActivatedView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LActivatedView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LActivatedView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "isActivated", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setActivated", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isActivated", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(view, "isSelected", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(view, "isEnabled", "()Z", &[]).unwrap(),
        Value::Int(1)
    );

    e.invoke_instance(view, "setSelected", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isActivated", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(view, "setActivated", "(Z)V", &[Value::Int(0)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isSelected", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
}

#[test]
fn view_overscroll_mode_defaults_and_round_trips() {
    let mut b = DexBuilder::new();
    b.class("LOverscrollView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LOverscrollView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LOverscrollView;", view).unwrap();
    assert_eq!(
        e.invoke_instance(view, "getOverScrollMode", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(view, "setOverScrollMode", "(I)V", &[Value::Int(2)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getOverScrollMode", "()I", &[])
            .unwrap(),
        Value::Int(2)
    );
}

#[test]
fn view_minimum_dimensions_round_trip_without_changing_measured_bounds() {
    let mut b = DexBuilder::new();
    b.class("LMinimumSizeView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LMinimumSizeView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LMinimumSizeView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "getMinimumWidth", "()I", &[])
            .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(view, "getMinimumHeight", "()I", &[])
            .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(view, "getSuggestedMinimumWidth", "()I", &[])
            .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(view, "getSuggestedMinimumHeight", "()I", &[])
            .unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setMinimumWidth", "(I)V", &[Value::Int(42)])
        .unwrap();
    e.invoke_instance(view, "setMinimumHeight", "(I)V", &[Value::Int(17)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getMinimumWidth", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        e.invoke_instance(view, "getMinimumHeight", "()I", &[])
            .unwrap(),
        Value::Int(17)
    );
    assert_eq!(
        e.invoke_instance(view, "getSuggestedMinimumWidth", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        e.invoke_instance(view, "getSuggestedMinimumHeight", "()I", &[])
            .unwrap(),
        Value::Int(17)
    );
    assert!(matches!(
        e.fw.get(view),
        Some(HostObj::View { w: 0, h: 0, .. })
    ));
}

#[test]
fn view_layout_direction_defaults_to_ltr_and_round_trips() {
    let mut b = DexBuilder::new();
    b.class("LLayoutDirectionView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LLayoutDirectionView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LLayoutDirectionView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "getLayoutDirection", "()I", &[])
            .unwrap(),
        Value::Int(0),
        "headless display defaults to left-to-right"
    );
    e.invoke_instance(view, "setLayoutDirection", "(I)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getLayoutDirection", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn view_scroll_offsets_round_trip_through_scroll_to_and_by() {
    let mut b = DexBuilder::new();
    b.class("LScrollStateView;", "Landroid/view/View;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LScrollStateView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("LScrollStateView;", view).unwrap();

    assert_eq!(
        e.invoke_instance(view, "getScrollX", "()I", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "scrollTo", "(II)V", &[Value::Int(20), Value::Int(30)])
        .unwrap();
    e.invoke_instance(view, "scrollBy", "(II)V", &[Value::Int(-5), Value::Int(2)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getScrollX", "()I", &[]).unwrap(),
        Value::Int(15)
    );
    assert_eq!(
        e.invoke_instance(view, "getScrollY", "()I", &[]).unwrap(),
        Value::Int(32)
    );
}

#[test]
fn textview_charsequence_null_is_not_treated_as_resource_zero() {
    let mut e = engine_of(&DexBuilder::new());
    let text_view = e
        .heap
        .alloc_instance("Landroid/widget/TextView;".into(), Vec::new())
        .unwrap();
    e.attach_host_state("Landroid/widget/TextView;", text_view)
        .unwrap();

    e.invoke_instance(
        text_view,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[Value::Int(0)],
    )
    .unwrap();
    let Value::Obj(text) = e
        .invoke_instance(text_view, "getText", "()Ljava/lang/CharSequence;", &[])
        .unwrap()
    else {
        panic!("TextView.getText should return a String");
    };
    assert_eq!(e.heap.as_str(text).unwrap(), "");

    let error = e
        .invoke_instance(text_view, "setText", "(I)V", &[Value::Int(0)])
        .unwrap_err();
    assert!(matches!(error, VmExit::Error(ref error) if error.code == "RESOURCES_MISSING"));
}

#[test]
fn data_set_observable_calls_dex_observers_and_unregisters_them() {
    let mut b = DexBuilder::new();
    let observer_class = b.class("LCountingObserver;", "Landroid/database/DataSetObserver;");
    b.r#virtual(
        observer_class,
        "onChanged",
        "V",
        vec![],
        0x1,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    b.r#virtual(
        observer_class,
        "onInvalidated",
        "V",
        vec![],
        0x1,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    let mut e = engine_of(&b);
    let observable = e
        .heap
        .alloc_instance("Landroid/database/DataSetObservable;".into(), vec![])
        .unwrap();
    e.invoke_instance(observable, "<init>", "()V", &[]).unwrap();
    let observer = e
        .heap
        .alloc_instance("LCountingObserver;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        observable,
        "registerObserver",
        "(Landroid/database/DataSetObserver;)V",
        &[Value::Obj(observer)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(observable, "hasObservers", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(observable, "notifyChanged", "()V", &[])
        .unwrap();
    e.invoke_instance(observable, "notifyInvalidated", "()V", &[])
        .unwrap();
    let labels = e.executed_labels();
    assert!(labels
        .iter()
        .any(|label| label == "LCountingObserver;->onChanged()V"));
    assert!(labels
        .iter()
        .any(|label| label == "LCountingObserver;->onInvalidated()V"));
    e.invoke_instance(
        observable,
        "unregisterObserver",
        "(Landroid/database/DataSetObserver;)V",
        &[Value::Obj(observer)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(observable, "hasObservers", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
}

#[test]
fn virtual_platform_base_callback_dispatches_to_dex_override() {
    let mut b = DexBuilder::new();
    let observer_class = b.class("LTestDataObserver;", "Landroid/database/DataSetObserver;");
    let caller_class = b.class("LTestDataObserverCaller;", "Ljava/lang/Object;");
    let on_changed_proto = b.proto_idx("V", vec![]);
    let on_changed = b.method_idx(
        "Landroid/database/DataSetObserver;",
        on_changed_proto,
        "onChanged",
    );
    let mut observer_code = op35c(0x6f, 1, on_changed, [1, 0, 0, 0, 0]); // super.onChanged()
    observer_code.extend(op10x(0x0e));
    b.r#virtual(
        observer_class,
        "onChanged",
        "V",
        vec![],
        0x1,
        Some(b.code(2, 1, 1, observer_code)),
    );
    let mut caller_code = op35c(0x6e, 1, on_changed, [0, 0, 0, 0, 0]);
    caller_code.extend(op10x(0x0e));
    b.direct(
        caller_class,
        "dispatch",
        "V",
        vec!["Landroid/database/DataSetObserver;"],
        0x1 | 0x8,
        Some(b.code(1, 1, 1, caller_code)),
    );

    let mut e = engine_of(&b);
    let observer = e
        .heap
        .alloc_instance("LTestDataObserver;".into(), vec![])
        .unwrap();
    e.invoke_static(
        "LTestDataObserverCaller;",
        "dispatch",
        "(Landroid/database/DataSetObserver;)V",
        &[Value::Obj(observer)],
    )
    .unwrap();
    assert!(e
        .executed_labels()
        .iter()
        .any(|label| label == "LTestDataObserver;->onChanged()V"));
}

#[test]
fn linear_layout_gravity_is_stored_on_custom_subclasses() {
    let mut b = DexBuilder::new();
    b.class("LCompatLinear;", "Landroid/widget/LinearLayout;");
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LCompatLinear;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCompatLinear;", view).unwrap();
    e.invoke_instance(view, "setGravity", "(I)V", &[Value::Int(0x11)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getGravity", "()I", &[]).unwrap(),
        Value::Int(0x11)
    );
}

#[test]
fn app_custom_views_can_extend_scroll_container_platform_views() {
    let mut b = DexBuilder::new();
    b.class(
        "LCompatTabContainer;",
        "Landroid/widget/HorizontalScrollView;",
    );
    let mut e = engine_of(&b);
    assert!(e
        .cp
        .is_subtype("LCompatTabContainer;", "Landroid/view/View;"));
    let view = e
        .heap
        .alloc_instance("LCompatTabContainer;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCompatTabContainer;", view).unwrap();
    assert!(matches!(e.fw.get(view), Some(HostObj::View { .. })));
}

#[test]
fn platform_graphics_insets_factory_and_value_operations_work() {
    let mut e = engine_of(&DexBuilder::new());
    let descriptor = "Landroid/graphics/Insets;";
    let sig = "(IIII)Landroid/graphics/Insets;";
    let first = e
        .invoke_static(
            descriptor,
            "of",
            sig,
            &[Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
        )
        .unwrap();
    let second = e
        .invoke_static(
            descriptor,
            "of",
            sig,
            &[Value::Int(4), Value::Int(3), Value::Int(2), Value::Int(1)],
        )
        .unwrap();
    let Value::Obj(first_obj) = first else {
        panic!("Insets.of must return a platform value object")
    };
    let Value::Obj(second_obj) = second else {
        panic!("Insets.of must return a platform value object")
    };
    for (field, expected) in [("left", 1), ("top", 2), ("right", 3), ("bottom", 4)] {
        assert_eq!(
            e.heap.get_field(first_obj, field).unwrap(),
            Value::Int(expected)
        );
    }
    let max = e
        .invoke_static(
            descriptor,
            "max",
            "(Landroid/graphics/Insets;Landroid/graphics/Insets;)Landroid/graphics/Insets;",
            &[Value::Obj(first_obj), Value::Obj(second_obj)],
        )
        .unwrap();
    let Value::Obj(max_obj) = max else {
        panic!("Insets.max must return a platform value object")
    };
    for (field, expected) in [("left", 4), ("top", 3), ("right", 3), ("bottom", 4)] {
        assert_eq!(
            e.heap.get_field(max_obj, field).unwrap(),
            Value::Int(expected)
        );
    }
    assert_eq!(
        e.invoke_instance(
            first_obj,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(first_obj)],
        )
        .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn platform_scroller_constructor_and_start_scroll_are_modelled() {
    let mut e = engine_of(&DexBuilder::new());
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), vec![])
        .unwrap();
    let scroller = e
        .heap
        .alloc_instance("Landroid/widget/Scroller;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        scroller,
        "<init>",
        "(Landroid/content/Context;Landroid/view/animation/Interpolator;)V",
        &[Value::Obj(context), Value::Null],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(scroller, "isFinished", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(
        scroller,
        "startScroll",
        "(IIIII)V",
        &[
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
            Value::Int(250),
        ],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(scroller, "isFinished", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(scroller, "computeScrollOffset", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(scroller, "getCurrX", "()I", &[]).unwrap(),
        Value::Int(40)
    );
    assert_eq!(
        e.invoke_instance(scroller, "getCurrY", "()I", &[]).unwrap(),
        Value::Int(60)
    );
    assert_eq!(
        e.invoke_instance(scroller, "isFinished", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn linear_interpolator_returns_input_fraction() {
    let mut e = engine_of(&DexBuilder::new());
    let interpolator = e
        .heap
        .alloc_instance("Landroid/view/animation/LinearInterpolator;".into(), vec![])
        .unwrap();
    e.invoke_instance(interpolator, "<init>", "()V", &[])
        .unwrap();
    assert_eq!(
        e.invoke_instance(
            interpolator,
            "getInterpolation",
            "(F)F",
            &[Value::Float(0.375)],
        )
        .unwrap(),
        Value::Float(0.375)
    );
}

#[test]
fn decelerate_interpolator_uses_default_and_explicit_factor() {
    let mut e = engine_of(&DexBuilder::new());
    let default = e
        .heap
        .alloc_instance(
            "Landroid/view/animation/DecelerateInterpolator;".into(),
            vec![],
        )
        .unwrap();
    e.invoke_instance(default, "<init>", "()V", &[]).unwrap();
    assert_eq!(
        e.invoke_instance(default, "getInterpolation", "(F)F", &[Value::Float(0.5)],)
            .unwrap(),
        Value::Float(0.75)
    );

    let factor_two = e
        .heap
        .alloc_instance(
            "Landroid/view/animation/DecelerateInterpolator;".into(),
            vec![],
        )
        .unwrap();
    e.invoke_instance(factor_two, "<init>", "(F)V", &[Value::Float(2.0)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(factor_two, "getInterpolation", "(F)F", &[Value::Float(0.5)],)
            .unwrap(),
        Value::Float(0.9375)
    );
}

#[test]
fn platform_edge_effect_tracks_pull_and_release() {
    let mut e = engine_of(&DexBuilder::new());
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), vec![])
        .unwrap();
    let edge = e
        .heap
        .alloc_instance("Landroid/widget/EdgeEffect;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        edge,
        "<init>",
        "(Landroid/content/Context;)V",
        &[Value::Obj(context)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(edge, "isFinished", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(edge, "onPull", "(F)V", &[Value::Float(0.25)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(edge, "isFinished", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(edge, "getDistance", "()F", &[]).unwrap(),
        Value::Float(0.25)
    );
    e.invoke_instance(edge, "onRelease", "()V", &[]).unwrap();
    assert_eq!(
        e.invoke_instance(edge, "isFinished", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
}

#[test]
fn imagebutton_subclass_uses_view_host_and_platform_super_constructor() {
    use rd_vm::heap::HeapObj;

    let mut b = DexBuilder::new();
    let subclass = b.class("LCompatButton;", "Landroid/widget/ImageButton;");
    let sig = "(Landroid/content/Context;Landroid/util/AttributeSet;I)V";
    let proto = b.proto_idx(
        "V",
        vec![
            "Landroid/content/Context;".into(),
            "Landroid/util/AttributeSet;".into(),
            "I".into(),
        ],
    );
    let super_ctor = b.method_idx("Landroid/widget/ImageButton;", proto, "<init>");
    let mut code = op35c(0x70, 4, super_ctor, [0, 1, 2, 3, 0]);
    code.extend(op10x(0x0e));
    b.direct(
        subclass,
        "<init>",
        "V",
        vec![
            "Landroid/content/Context;",
            "Landroid/util/AttributeSet;",
            "I",
        ],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(4, 4, 0, code)),
    );
    let mut e = engine_of(&b);
    let view = e
        .heap
        .alloc_instance("LCompatButton;".into(), vec![])
        .unwrap();
    e.attach_host_state("LCompatButton;", view).unwrap();
    assert!(matches!(e.fw.get(view), Some(HostObj::View { .. })));

    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".into(), vec![])
        .unwrap();
    let attrs = e
        .heap
        .alloc_instance("Landroid/util/AttributeSet;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        view,
        "<init>",
        sig,
        &[Value::Obj(context), Value::Obj(attrs), Value::Int(0)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getContext", "()Landroid/content/Context;", &[])
            .unwrap(),
        Value::Obj(context)
    );
    assert_eq!(
        e.invoke_instance(view, "isLaidOut", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(view, "setFocusable", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isFocusable", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(view, "setFocusableInTouchMode", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isFocusableInTouchMode", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(
        view,
        "setHorizontalScrollBarEnabled",
        "(Z)V",
        &[Value::Int(0)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isHorizontalScrollBarEnabled", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(
        view,
        "setVerticalScrollBarEnabled",
        "(Z)V",
        &[Value::Int(0)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isVerticalScrollBarEnabled", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(
        view,
        "setPaddingRelative",
        "(IIII)V",
        &[Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getPaddingStart", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(view, "getPaddingEnd", "()I", &[])
            .unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        e.invoke_instance(view, "isPaddingRelative", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(view, "getImportantForAccessibility", "()I", &[],)
            .unwrap(),
        Value::Int(0)
    );
    e.invoke_instance(
        view,
        "setImportantForAccessibility",
        "(I)V",
        &[Value::Int(1)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getImportantForAccessibility", "()I", &[],)
            .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(
        view,
        "setDescendantFocusability",
        "(I)V",
        &[Value::Int(0x0004_0000)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "getDescendantFocusability", "()I", &[])
            .unwrap(),
        Value::Int(0x0004_0000)
    );
    e.invoke_instance(
        view,
        "layout",
        "(IIII)V",
        &[Value::Int(0), Value::Int(0), Value::Int(10), Value::Int(10)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isLaidOut", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(view, "requestLayout", "()V", &[])
        .unwrap();
    assert_eq!(
        e.invoke_instance(view, "isLayoutRequested", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(
            view,
            "getDrawable",
            "()Landroid/graphics/drawable/Drawable;",
            &[]
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(view, "setWillNotDraw", "(Z)V", &[Value::Int(1)])
            .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(view, "postInvalidateOnAnimation", "()V", &[])
            .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(
            view,
            "setBackground",
            "(Landroid/graphics/drawable/Drawable;)V",
            &[Value::Null],
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(
            view,
            "getBackground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        e.invoke_instance(
            view,
            "saveAttributeDataForStyleable",
            "(Landroid/content/Context;[ILandroid/util/AttributeSet;Landroid/content/res/TypedArray;II)V",
            &[
                Value::Obj(context),
                Value::Null,
                Value::Obj(attrs),
                Value::Null,
                Value::Int(0),
                Value::Int(0),
            ],
        )
        .unwrap(),
        Value::Null
    );
    assert!(matches!(e.heap.get(view), Ok(HeapObj::Instance { .. })));
}

#[test]
fn cyclic_nonplatform_hierarchy_fails_closed() {
    let mut b = DexBuilder::new();
    b.class("LA;", "LB;");
    b.class("LB;", "LA;");
    let mut e = engine_of(&b);
    let r = e.heap.alloc_instance("LA;".into(), vec![]).unwrap();
    assert!(
        matches!(e.invoke_instance(r,"unrecognizedCall","()V",&[]),Err(VmExit::Error(e)) if e.code=="NOT_FOUND")
    );
}
