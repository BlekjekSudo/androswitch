//! Regression suite for Issue #315 (items 1, 3, and 4):
//! 1. `ClipboardManager` unified dispatch between `m9::ime` and `m10::sysconfig`
//!    (`addPrimaryClipChangedListener`, `removePrimaryClipChangedListener`,
//!    `setPrimaryClip`, `clearPrimaryClip`, and host `m10_set_clipboard`).
//! 3. Polymorphic DEX overrides on `recv_class` (and intermediate guest
//!    `invoke-super`) run before `m9`/`m10`/`m8`/`androidx`/`gms` host shims.
//! 4. `AutoCloseable` / `Closeable` interface subtyping and `invoke-interface`
//!    `close()V` dispatch on `CameraDevice`, `CameraCaptureSession`,
//!    `ImageReader`, and `Image`.

#![cfg(feature = "virtual-devices")]

use rd_testsupport::vm::*;
use rd_vm::{framework::HostObj, heap::HeapObj, Engine, ObjRef, Value};

const ACC_ABSTRACT: u32 = 0x0400;

fn dex_obj(e: &mut Engine, cls: &str) -> ObjRef {
    e.ensure_initialized(cls).expect("init cls");
    e.new_dex_instance_pub(cls).expect("alloc dex cls")
}

fn static_int(e: &Engine, cls: &str, field: &str) -> i32 {
    match e.statics.get(&(cls.to_string(), field.to_string())) {
        Some(Value::Int(n)) => *n,
        other => panic!("expected static int {cls}.{field}, got {other:?}"),
    }
}

fn str_of(e: &Engine, v: Value) -> String {
    let Value::Obj(r) = v else {
        panic!("expected String Obj, got {v:?}");
    };
    match e.heap.get(r).expect("heap get") {
        HeapObj::Str(s) => s.clone(),
        other => panic!("expected HeapObj::Str, got {other:?}"),
    }
}

#[test]
fn clipboard_manager_bytecode_listener_and_host_set_clipboard_are_unified() {
    let mut b = DexBuilder::new();
    let listener_cls = b.class("LClipCb;", "Ljava/lang/Object;");
    b.static_field(listener_cls, "changes", "I", SVal::Int(0));
    b.direct(
        listener_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    let changes_f = b.field_idx("LClipCb;", "I", "changes");
    let mut cb_code = op21c(0x60, 0, changes_f);
    cb_code.extend(op22b(0xd8, 0, 0, 1));
    cb_code.extend(op21c(0x67, 0, changes_f));
    cb_code.extend(op10x(0x0e));
    b.r#virtual(
        listener_cls,
        "onPrimaryClipChanged",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, cb_code)),
    );

    let driver = b.class("LClipDriver;", "Ljava/lang/Object;");
    let p_get_svc = b.proto_idx("Ljava/lang/Object;", vec!["Ljava/lang/String;".into()]);
    let m_get_svc = b.method_idx("Landroid/content/Context;", p_get_svc, "getSystemService");
    let t_cm = b.type_idx("Landroid/content/ClipboardManager;");
    let p_listener = b.proto_idx(
        "V",
        vec!["Landroid/content/ClipboardManager$OnPrimaryClipChangedListener;".into()],
    );
    let m_add = b.method_idx(
        "Landroid/content/ClipboardManager;",
        p_listener,
        "addPrimaryClipChangedListener",
    );
    let m_rem = b.method_idx(
        "Landroid/content/ClipboardManager;",
        p_listener,
        "removePrimaryClipChangedListener",
    );
    let p_new_plain = b.proto_idx(
        "Landroid/content/ClipData;",
        vec![
            "Ljava/lang/CharSequence;".into(),
            "Ljava/lang/CharSequence;".into(),
        ],
    );
    let m_new_plain = b.method_idx("Landroid/content/ClipData;", p_new_plain, "newPlainText");
    let p_set_clip = b.proto_idx("V", vec!["Landroid/content/ClipData;".into()]);
    let m_set_clip = b.method_idx(
        "Landroid/content/ClipboardManager;",
        p_set_clip,
        "setPrimaryClip",
    );
    let p_void = b.proto_idx("V", vec![]);
    let m_clear = b.method_idx(
        "Landroid/content/ClipboardManager;",
        p_void,
        "clearPrimaryClip",
    );
    let p_bool = b.proto_idx("Z", vec![]);
    let m_has = b.method_idx(
        "Landroid/content/ClipboardManager;",
        p_bool,
        "hasPrimaryClip",
    );

    let s_clip = b.intern("clipboard") as u16;
    let s_lbl = b.intern("otp") as u16;
    let s_txt = b.intern("111222") as u16;

    // registerAndSet(Context p0=v3, LClipCb p1=v4) -> ClipboardManager
    let mut reg_code = op21c(0x1a, 0, s_clip);
    reg_code.extend(op35c(0x6e, 2, m_get_svc, [3, 0, 0, 0, 0]));
    reg_code.extend(op11x(0x0c, 0));
    reg_code.extend(op21c(0x1f, 0, t_cm));
    reg_code.extend(op35c(0x6e, 2, m_add, [0, 4, 0, 0, 0]));
    reg_code.extend(op21c(0x1a, 1, s_lbl));
    reg_code.extend(op21c(0x1a, 2, s_txt));
    reg_code.extend(op35c(0x71, 2, m_new_plain, [1, 2, 0, 0, 0]));
    reg_code.extend(op11x(0x0c, 1));
    reg_code.extend(op35c(0x6e, 2, m_set_clip, [0, 1, 0, 0, 0]));
    reg_code.extend(op11x(0x11, 0));

    // clearAndUnregister(ClipboardManager p0=v2, LClipCb p1=v3) -> int (hasPrimaryClip after clear)
    let mut clr_code = op35c(0x6e, 1, m_clear, [2, 0, 0, 0, 0]);
    clr_code.extend(op35c(0x6e, 1, m_has, [2, 0, 0, 0, 0]));
    clr_code.extend(op11x(0x0a, 0));
    clr_code.extend(op35c(0x6e, 2, m_rem, [2, 3, 0, 0, 0]));
    clr_code.extend(op21c(0x1a, 1, s_lbl));
    clr_code.extend(op35c(0x71, 2, m_new_plain, [1, 1, 0, 0, 0]));
    clr_code.extend(op11x(0x0c, 1));
    clr_code.extend(op35c(0x6e, 2, m_set_clip, [2, 1, 0, 0, 0]));
    clr_code.extend(op11x(0x0f, 0));

    // Alphabetical order in directs: clearAndUnregister, registerAndSet
    b.direct(
        driver,
        "clearAndUnregister",
        "I",
        vec!["Landroid/content/ClipboardManager;", "LClipCb;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 2, 2, clr_code)),
    );
    b.direct(
        driver,
        "registerAndSet",
        "Landroid/content/ClipboardManager;",
        vec!["Landroid/content/Context;", "LClipCb;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 2, 2, reg_code)),
    );

    let mut e = engine_of(&b);
    let ctx = e
        .heap
        .alloc_instance("Landroid/app/Application;".into(), vec![])
        .expect("alloc ctx");
    e.attach_host_state("Landroid/app/Application;", ctx)
        .expect("attach ctx");
    let cb = dex_obj(&mut e, "LClipCb;");

    let Value::Obj(cm) = e
        .invoke_static(
            "LClipDriver;",
            "registerAndSet",
            "(Landroid/content/Context;LClipCb;)Landroid/content/ClipboardManager;",
            &[Value::Obj(ctx), Value::Obj(cb)],
        )
        .expect("registerAndSet")
    else {
        panic!("expected ClipboardManager instance");
    };
    assert_eq!(
        static_int(&e, "LClipCb;", "changes"),
        1,
        "bytecode setPrimaryClip must fire listener registered via bytecode"
    );
    assert_eq!(
        e.m10_get_clipboard().expect("get clip"),
        Some("111222".into())
    );

    // Host m10_set_clipboard must notify the listener registered via bytecode
    e.m10_set_clipboard("host", "999888")
        .expect("host set_clipboard");
    assert_eq!(
        static_int(&e, "LClipCb;", "changes"),
        2,
        "m10_set_clipboard must fire listener registered via ClipboardManager bytecode"
    );
    assert_eq!(
        e.m10_get_clipboard().expect("get clip"),
        Some("999888".into())
    );

    // clearPrimaryClip fires listener (2 -> 3), then removePrimaryClipChangedListener stops future callbacks
    let has_after_clear = e
        .invoke_static(
            "LClipDriver;",
            "clearAndUnregister",
            "(Landroid/content/ClipboardManager;LClipCb;)I",
            &[Value::Obj(cm), Value::Obj(cb)],
        )
        .expect("clearAndUnregister");
    assert_eq!(has_after_clear, Value::Int(0));
    assert_eq!(
        static_int(&e, "LClipCb;", "changes"),
        3,
        "clearPrimaryClip fires listener once, and unregistered listener is not called again"
    );
}

#[test]
fn polymorphic_dex_override_on_context_wrapper_and_view_runs_before_host_shims() {
    let mut b = DexBuilder::new();

    // LCustomCtx extends ContextWrapper and overrides getSystemService(String)
    let custom_ctx = b.class("LCustomCtx;", "Landroid/content/ContextWrapper;");
    b.static_field(custom_ctx, "intercepts", "I", SVal::Int(0));
    let f_ctx_hits = b.field_idx("LCustomCtx;", "I", "intercepts");
    let p_get_svc = b.proto_idx("Ljava/lang/Object;", vec!["Ljava/lang/String;".into()]);
    let m_super_svc = b.method_idx(
        "Landroid/content/ContextWrapper;",
        p_get_svc,
        "getSystemService",
    );
    let m_ctx_svc = b.method_idx("Landroid/content/Context;", p_get_svc, "getSystemService");
    let mut svc_code = op21c(0x60, 0, f_ctx_hits);
    svc_code.extend(op22b(0xd8, 0, 0, 1));
    svc_code.extend(op21c(0x67, 0, f_ctx_hits));
    svc_code.extend(op35c(0x6f, 2, m_super_svc, [1, 2, 0, 0, 0]));
    svc_code.extend(op11x(0x0c, 0));
    svc_code.extend(op11x(0x11, 0));
    b.r#virtual(
        custom_ctx,
        "getSystemService",
        "Ljava/lang/Object;",
        vec!["Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, svc_code)),
    );

    // LCustomView extends View and overrides setContentDescription(CharSequence)
    let custom_view = b.class("LCustomView;", "Landroid/view/View;");
    b.static_field(custom_view, "desc_hits", "I", SVal::Int(0));
    let f_desc_hits = b.field_idx("LCustomView;", "I", "desc_hits");
    let p_set_desc = b.proto_idx("V", vec!["Ljava/lang/CharSequence;".into()]);
    let m_view_set_desc = b.method_idx("Landroid/view/View;", p_set_desc, "setContentDescription");
    let mut desc_code = op21c(0x60, 0, f_desc_hits);
    desc_code.extend(op22b(0xd8, 0, 0, 1));
    desc_code.extend(op21c(0x67, 0, f_desc_hits));
    desc_code.extend(op35c(0x6f, 2, m_view_set_desc, [1, 2, 0, 0, 0]));
    desc_code.extend(op10x(0x0e));
    b.r#virtual(
        custom_view,
        "setContentDescription",
        "V",
        vec!["Ljava/lang/CharSequence;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, desc_code)),
    );

    let caller = b.class("LPolyCaller;", "Ljava/lang/Object;");
    let s_clip = b.intern("clipboard") as u16;
    let s_desc = b.intern("accessible-button") as u16;
    // exercise(Context p0=v2, View p1=v3) -> Object
    let mut call_code = op21c(0x1a, 0, s_desc);
    call_code.extend(op35c(0x6e, 2, m_view_set_desc, [3, 0, 0, 0, 0]));
    call_code.extend(op21c(0x1a, 0, s_clip));
    call_code.extend(op35c(0x6e, 2, m_ctx_svc, [2, 0, 0, 0, 0]));
    call_code.extend(op11x(0x0c, 0));
    call_code.extend(op11x(0x11, 0));
    b.direct(
        caller,
        "exercise",
        "Ljava/lang/Object;",
        vec!["Landroid/content/Context;", "Landroid/view/View;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 2, 2, call_code)),
    );

    let mut e = engine_of(&b);
    e.ensure_initialized("LCustomCtx;").expect("init CustomCtx");
    e.ensure_initialized("LCustomView;")
        .expect("init CustomView");
    let ctx = e
        .heap
        .alloc_instance("LCustomCtx;".into(), vec![])
        .expect("alloc CustomCtx");
    e.attach_host_state("LCustomCtx;", ctx)
        .expect("attach CustomCtx");
    let view = e
        .heap
        .alloc_instance("LCustomView;".into(), vec![])
        .expect("alloc CustomView");
    e.attach_host_state("LCustomView;", view)
        .expect("attach CustomView");

    let Value::Obj(svc) = e
        .invoke_static(
            "LPolyCaller;",
            "exercise",
            "(Landroid/content/Context;Landroid/view/View;)Ljava/lang/Object;",
            &[Value::Obj(ctx), Value::Obj(view)],
        )
        .expect("exercise")
    else {
        panic!("expected ClipboardManager object");
    };
    assert_eq!(
        e.heap.class_of(svc).expect("svc class"),
        "Landroid/content/ClipboardManager;"
    );
    assert_eq!(static_int(&e, "LCustomCtx;", "intercepts"), 1);
    assert_eq!(static_int(&e, "LCustomView;", "desc_hits"), 1);

    let got_desc = e
        .invoke_instance(
            view,
            "getContentDescription",
            "()Ljava/lang/CharSequence;",
            &[],
        )
        .expect("getContentDescription");
    assert_eq!(str_of(&e, got_desc), "accessible-button");
}

#[test]
fn polymorphic_dex_override_on_mutable_livedata_runs_before_androidx_shim() {
    let mut b = DexBuilder::new();

    // LAppLiveData extends host-shimmed Landroidx/lifecycle/MutableLiveData;
    // and overrides setValue(Object), incrementing `hooks` before chaining super.
    let app_ld = b.class("LAppLiveData;", "Landroidx/lifecycle/MutableLiveData;");
    b.static_field(app_ld, "hooks", "I", SVal::Int(0));
    let f_hooks = b.field_idx("LAppLiveData;", "I", "hooks");
    let p_ctor = b.proto_idx("V", vec![]);
    let m_mld_ctor = b.method_idx("Landroidx/lifecycle/MutableLiveData;", p_ctor, "<init>");
    let mut ctor_code = op35c(0x70, 1, m_mld_ctor, [0, 0, 0, 0, 0]);
    ctor_code.extend(op10x(0x0e));
    b.direct(
        app_ld,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 1, ctor_code)),
    );

    let p_set = b.proto_idx("V", vec!["Ljava/lang/Object;".into()]);
    let m_mld_set = b.method_idx("Landroidx/lifecycle/MutableLiveData;", p_set, "setValue");
    let mut set_code = op21c(0x60, 0, f_hooks);
    set_code.extend(op22b(0xd8, 0, 0, 10));
    set_code.extend(op21c(0x67, 0, f_hooks));
    set_code.extend(op35c(0x6f, 2, m_mld_set, [1, 2, 0, 0, 0]));
    set_code.extend(op10x(0x0e));
    b.r#virtual(
        app_ld,
        "setValue",
        "V",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, set_code)),
    );

    let caller = b.class("LLiveCaller;", "Ljava/lang/Object;");
    let p_get = b.proto_idx("Ljava/lang/Object;", vec![]);
    let m_mld_get = b.method_idx("Landroidx/lifecycle/MutableLiveData;", p_get, "getValue");
    let s_val = b.intern("live-42") as u16;
    // run(LAppLiveData p0=v1) -> Object
    let mut run_code = op21c(0x1a, 0, s_val);
    run_code.extend(op35c(0x6e, 2, m_mld_set, [1, 0, 0, 0, 0]));
    run_code.extend(op35c(0x6e, 1, m_mld_get, [1, 0, 0, 0, 0]));
    run_code.extend(op11x(0x0c, 0));
    run_code.extend(op11x(0x11, 0));
    b.direct(
        caller,
        "run",
        "Ljava/lang/Object;",
        vec!["LAppLiveData;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 1, 2, run_code)),
    );

    let mut e = engine_of(&b);
    let ld = dex_obj(&mut e, "LAppLiveData;");
    e.invoke_instance(ld, "<init>", "()V", &[])
        .expect("LAppLiveData.<init>");

    let out = e
        .invoke_static(
            "LLiveCaller;",
            "run",
            "(LAppLiveData;)Ljava/lang/Object;",
            &[Value::Obj(ld)],
        )
        .expect("LLiveCaller.run");
    assert_eq!(static_int(&e, "LAppLiveData;", "hooks"), 10);
    assert_eq!(str_of(&e, out), "live-42");
    // Also verify D8 subclass owner resolution (LAppLiveData->getValue)
    let inherited_out = e
        .invoke_instance(ld, "getValue", "()Ljava/lang/Object;", &[])
        .expect("LAppLiveData.getValue");
    assert_eq!(str_of(&e, inherited_out), "live-42");
}

#[test]
fn camera_and_image_autocloseable_try_with_resources_closes_host_resources() {
    let mut b = DexBuilder::new();

    // Bundled desugared AutoCloseable interface with abstract close()V
    let auto_cls = b.class("Ljava/lang/AutoCloseable;", "Ljava/lang/Object;");
    b.r#virtual(
        auto_cls,
        "close",
        "V",
        vec![],
        ACC_PUBLIC | ACC_ABSTRACT,
        None,
    );

    // CameraDevice$StateCallback tracking onOpened & onClosed
    let dev_cb_cls = b.class(
        "LCamDevCb;",
        "Landroid/hardware/camera2/CameraDevice$StateCallback;",
    );
    b.static_field(dev_cb_cls, "closed", "I", SVal::Int(0));
    b.static_field(dev_cb_cls, "opened", "I", SVal::Int(0));
    b.direct(
        dev_cb_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    let f_dev_closed = b.field_idx("LCamDevCb;", "I", "closed");
    let mut dev_closed_code = op21c(0x60, 0, f_dev_closed);
    dev_closed_code.extend(op22b(0xd8, 0, 0, 1));
    dev_closed_code.extend(op21c(0x67, 0, f_dev_closed));
    dev_closed_code.extend(op10x(0x0e));
    b.r#virtual(
        dev_cb_cls,
        "onClosed",
        "V",
        vec!["Landroid/hardware/camera2/CameraDevice;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, dev_closed_code)),
    );
    let f_dev_opened = b.field_idx("LCamDevCb;", "I", "opened");
    let mut dev_opened_code = op21c(0x60, 0, f_dev_opened);
    dev_opened_code.extend(op22b(0xd8, 0, 0, 1));
    dev_opened_code.extend(op21c(0x67, 0, f_dev_opened));
    dev_opened_code.extend(op10x(0x0e));
    b.r#virtual(
        dev_cb_cls,
        "onOpened",
        "V",
        vec!["Landroid/hardware/camera2/CameraDevice;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, dev_opened_code)),
    );

    // CameraCaptureSession$StateCallback tracking onClosed & onConfigured
    let sess_cb_cls = b.class(
        "LCamSessCb;",
        "Landroid/hardware/camera2/CameraCaptureSession$StateCallback;",
    );
    b.static_field(sess_cb_cls, "closed", "I", SVal::Int(0));
    b.static_field(sess_cb_cls, "configured", "I", SVal::Int(0));
    b.direct(
        sess_cb_cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    let f_sess_closed = b.field_idx("LCamSessCb;", "I", "closed");
    let mut sess_closed_code = op21c(0x60, 0, f_sess_closed);
    sess_closed_code.extend(op22b(0xd8, 0, 0, 1));
    sess_closed_code.extend(op21c(0x67, 0, f_sess_closed));
    sess_closed_code.extend(op10x(0x0e));
    b.r#virtual(
        sess_cb_cls,
        "onClosed",
        "V",
        vec!["Landroid/hardware/camera2/CameraCaptureSession;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, sess_closed_code)),
    );
    let f_sess_cfg = b.field_idx("LCamSessCb;", "I", "configured");
    let mut sess_cfg_code = op21c(0x60, 0, f_sess_cfg);
    sess_cfg_code.extend(op22b(0xd8, 0, 0, 1));
    sess_cfg_code.extend(op21c(0x67, 0, f_sess_cfg));
    sess_cfg_code.extend(op10x(0x0e));
    b.r#virtual(
        sess_cb_cls,
        "onConfigured",
        "V",
        vec!["Landroid/hardware/camera2/CameraCaptureSession;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, sess_cfg_code)),
    );

    // D8 try-with-resources helper: instance-of AutoCloseable + check-cast + invoke-interface close()V
    let twr = b.class("LTwrHelper;", "Ljava/lang/Object;");
    let t_auto = b.type_idx("Ljava/lang/AutoCloseable;");
    let p_close = b.proto_idx("V", vec![]);
    let m_close = b.method_idx("Ljava/lang/AutoCloseable;", p_close, "close");
    // closeResource(Object p0=v1) -> int (1 if instanceof AutoCloseable)
    let mut twr_code = op22c(0x20, 0, 1, t_auto);
    twr_code.extend(op21c(0x1f, 1, t_auto));
    twr_code.extend(op35c(0x72, 1, m_close, [1, 0, 0, 0, 0]));
    twr_code.extend(op11x(0x0f, 0));
    b.direct(
        twr,
        "closeResource",
        "I",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 1, 1, twr_code)),
    );

    let mut e = engine_of(&b);

    let Value::Obj(reader) = rd_vm::m10::call_static(
        &mut e,
        "Landroid/media/ImageReader;",
        "newInstance",
        "(IIII)Landroid/media/ImageReader;",
        &[
            Value::Int(64),
            Value::Int(64),
            Value::Int(256),
            Value::Int(2),
        ],
    )
    .expect("newInstance")
    .expect("some reader") else {
        panic!("expected ImageReader");
    };
    let Value::Obj(surface) = rd_vm::m10::call_instance(
        &mut e,
        "Landroid/media/ImageReader;",
        "getSurface",
        "()Landroid/view/Surface;",
        reader,
        &[],
    )
    .expect("getSurface")
    .expect("some surface") else {
        panic!("expected Surface");
    };

    let cm = e
        .heap
        .alloc_instance("Landroid/hardware/camera2/CameraManager;".into(), vec![])
        .expect("alloc CameraManager");
    let cam0 = e.heap.alloc_string("0".into()).expect("alloc '0'");
    let dev_cb = dex_obj(&mut e, "LCamDevCb;");
    rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CameraManager;",
        "openCamera",
        "(Ljava/lang/String;Landroid/hardware/camera2/CameraDevice$StateCallback;Landroid/os/Handler;)V",
        cm,
        &[Value::Obj(cam0), Value::Obj(dev_cb), Value::Null],
    )
    .expect("openCamera");
    assert_eq!(static_int(&e, "LCamDevCb;", "opened"), 1);
    let dev = *e.fw.vdev.camera_devices.keys().next().expect("camera dev");

    let surf_list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".into(), vec![])
        .expect("alloc ArrayList");
    e.fw.objects.insert(
        surf_list,
        HostObj::List {
            elems: vec![Value::Obj(surface)],
        },
    );
    let sess_cb = dex_obj(&mut e, "LCamSessCb;");
    rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CameraDevice;",
        "createCaptureSession",
        "(Ljava/util/List;Landroid/hardware/camera2/CameraCaptureSession$StateCallback;Landroid/os/Handler;)V",
        dev,
        &[Value::Obj(surf_list), Value::Obj(sess_cb), Value::Null],
    )
    .expect("createCaptureSession");
    assert_eq!(static_int(&e, "LCamSessCb;", "configured"), 1);
    let sess = *e
        .fw
        .vdev
        .camera_sessions
        .keys()
        .next()
        .expect("camera session");

    let Value::Obj(req_builder) = rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CameraDevice;",
        "createCaptureRequest",
        "(I)Landroid/hardware/camera2/CaptureRequest$Builder;",
        dev,
        &[Value::Int(1)],
    )
    .expect("createCaptureRequest")
    .expect("some builder") else {
        panic!("expected CaptureRequest$Builder");
    };
    rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CaptureRequest$Builder;",
        "addTarget",
        "(Landroid/view/Surface;)V",
        req_builder,
        &[Value::Obj(surface)],
    )
    .expect("addTarget");
    let Value::Obj(cap_req) = rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CaptureRequest$Builder;",
        "build",
        "()Landroid/hardware/camera2/CaptureRequest;",
        req_builder,
        &[],
    )
    .expect("build")
    .expect("some CaptureRequest") else {
        panic!("expected CaptureRequest");
    };
    rd_vm::m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CameraCaptureSession;",
        "setRepeatingRequest",
        "(Landroid/hardware/camera2/CaptureRequest;Landroid/hardware/camera2/CameraCaptureSession$CaptureCallback;Landroid/os/Handler;)I",
        sess,
        &[Value::Obj(cap_req), Value::Null, Value::Null],
    )
    .expect("setRepeatingRequest");

    e.m10_inject_camera_frame("0", 64, 64, 256, vec![0xFF, 0xD8, 0xFF, 0xD9])
        .expect("inject frame");
    let Value::Obj(img) = rd_vm::m10::call_instance(
        &mut e,
        "Landroid/media/ImageReader;",
        "acquireLatestImage",
        "()Landroid/media/Image;",
        reader,
        &[],
    )
    .expect("acquireLatestImage")
    .expect("some image") else {
        panic!("expected Image");
    };

    // Close all 4 resources via try-with-resources AutoCloseable bytecode!
    for res in [img, reader, sess, dev] {
        let is_auto = e
            .invoke_static(
                "LTwrHelper;",
                "closeResource",
                "(Ljava/lang/Object;)I",
                &[Value::Obj(res)],
            )
            .expect("AutoCloseable.close must succeed");
        assert_eq!(is_auto, Value::Int(1));
    }

    // Verify all 4 host state transitions actually ran:
    assert_eq!(
        e.heap.get_field(img, "rd_closed").expect("img rd_closed"),
        Value::Int(1)
    );
    assert!(!e.fw.vdev.image_readers.contains_key(&reader));
    assert!(!e.fw.vdev.camera_sessions.contains_key(&sess));
    assert!(!e.fw.vdev.camera_devices.contains_key(&dev));
    assert_eq!(static_int(&e, "LCamSessCb;", "closed"), 1);
    assert_eq!(static_int(&e, "LCamDevCb;", "closed"), 1);
}
