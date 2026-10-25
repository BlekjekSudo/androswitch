//! End-to-end Dalvik bytecode tests for `DisplayManager`,
//! `DisplayManager.DisplayListener`, `Display`, and `Display.Mode`
//! (unblocking Flutter's `VsyncWaiter` and AOSP display listener contracts).

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::HeapObj;
use rd_vm::intrinsics::{
    dispatch_display_event, EVENT_FLAG_DISPLAY_ADDED, EVENT_FLAG_DISPLAY_CHANGED,
    EVENT_FLAG_DISPLAY_REMOVED,
};
use rd_vm::value::Value;

const ACC_CONSTRUCTOR: u32 = 0x1_0000;
const OBJECT: &str = "Ljava/lang/Object;";
const ACTIVITY: &str = "Landroid/app/Activity;";
const HANDLER: &str = "Landroid/os/Handler;";
const EXECUTOR: &str = "Ljava/util/concurrent/Executor;";
const DISPLAY: &str = "Landroid/view/Display;";
const DISPLAY_MODE: &str = "Landroid/view/Display$Mode;";
const DISPLAY_MANAGER: &str = "Landroid/hardware/display/DisplayManager;";
const DISPLAY_LISTENER: &str = "Landroid/hardware/display/DisplayManager$DisplayListener;";
const TEST_LISTENER: &str = "LVsyncListener;";
const TEST_HARNESS: &str = "LDisplayHarness;";

fn op_if_eqz(reg: u8, off: i16) -> Vec<u16> {
    vec![0x38u16 | ((reg as u16) << 8), off as u16]
}

fn op_const32(reg: u8, val: i32) -> Vec<u16> {
    op31t(0x14, reg, val)
}

fn build_harness_dex() -> DexBuilder {
    let mut b = DexBuilder::new();

    let super_listener_init = {
        let p = b.proto_idx("V", vec![]);
        b.method_idx(DISPLAY_LISTENER, p, "<init>")
    };

    // ── LVsyncListener; extends DisplayManager$DisplayListener ──────────────
    let lc = b.class(TEST_LISTENER, DISPLAY_LISTENER);
    b.instance_field(lc, "addedCount", "I");
    b.instance_field(lc, "removedCount", "I");
    b.instance_field(lc, "changedCount", "I");
    b.instance_field(lc, "lastDisplayId", "I");
    let f_added = b.field_idx(TEST_LISTENER, "I", "addedCount");
    let f_removed = b.field_idx(TEST_LISTENER, "I", "removedCount");
    let f_changed = b.field_idx(TEST_LISTENER, "I", "changedCount");
    let f_last_id = b.field_idx(TEST_LISTENER, "I", "lastDisplayId");

    b.direct(
        lc,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut c = op35c(0x70, 1, super_listener_init, [0, 0, 0, 0, 0]);
            c.extend(op10x(0x0e));
            c
        })),
    );

    let make_cb = |counter_field: u16| -> Vec<u16> {
        // (I)V: this=v2, displayId=v3
        let mut c = op22c(0x52, 0, 2, counter_field); // iget v0, v2, counter
        c.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
        c.extend(op12x(0xb0, 0, 1)); // add-int/2addr v0, v1
        c.extend(op22c(0x59, 0, 2, counter_field)); // iput v0, v2, counter
        c.extend(op22c(0x59, 3, 2, f_last_id)); // iput v3, v2, lastDisplayId
        c.extend(op10x(0x0e)); // return-void
        c
    };
    let cb_add = b.code(4, 2, 0, make_cb(f_added));
    b.r#virtual(
        lc,
        "onDisplayAdded",
        "V",
        vec!["I"],
        ACC_PUBLIC,
        Some(cb_add),
    );
    let cb_rem = b.code(4, 2, 0, make_cb(f_removed));
    b.r#virtual(
        lc,
        "onDisplayRemoved",
        "V",
        vec!["I"],
        ACC_PUBLIC,
        Some(cb_rem),
    );
    let cb_chg = b.code(4, 2, 0, make_cb(f_changed));
    b.r#virtual(
        lc,
        "onDisplayChanged",
        "V",
        vec!["I"],
        ACC_PUBLIC,
        Some(cb_chg),
    );

    // ── LDisplayHarness; static helpers driving DisplayManager via DEX ──────
    let hc = b.class(TEST_HARNESS, OBJECT);

    let m_get_service = {
        let p = b.proto_idx("Ljava/lang/Object;", vec!["Ljava/lang/String;".into()]);
        b.method_idx(ACTIVITY, p, "getSystemService")
    };
    let m_listener_init = {
        let p = b.proto_idx("V", vec![]);
        b.method_idx(TEST_LISTENER, p, "<init>")
    };
    let m_reg2 = {
        let p = b.proto_idx("V", vec![DISPLAY_LISTENER.into(), HANDLER.into()]);
        b.method_idx(DISPLAY_MANAGER, p, "registerDisplayListener")
    };
    let m_reg3 = {
        let p = b.proto_idx(
            "V",
            vec![DISPLAY_LISTENER.into(), HANDLER.into(), "J".into()],
        );
        b.method_idx(DISPLAY_MANAGER, p, "registerDisplayListener")
    };
    let m_reg_exec = {
        let p = b.proto_idx(
            "V",
            vec![EXECUTOR.into(), "J".into(), DISPLAY_LISTENER.into()],
        );
        b.method_idx(DISPLAY_MANAGER, p, "registerDisplayListener")
    };
    let m_unreg = {
        let p = b.proto_idx("V", vec![DISPLAY_LISTENER.into()]);
        b.method_idx(DISPLAY_MANAGER, p, "unregisterDisplayListener")
    };
    let m_get_display = {
        let p = b.proto_idx(DISPLAY, vec!["I".into()]);
        b.method_idx(DISPLAY_MANAGER, p, "getDisplay")
    };
    let m_get_displays0 = {
        let p = b.proto_idx(&format!("[{DISPLAY}"), vec![]);
        b.method_idx(DISPLAY_MANAGER, p, "getDisplays")
    };
    let m_get_displays1 = {
        let p = b.proto_idx(&format!("[{DISPLAY}"), vec!["Ljava/lang/String;".into()]);
        b.method_idx(DISPLAY_MANAGER, p, "getDisplays")
    };
    let m_disp_fps = {
        let p = b.proto_idx("F", vec![]);
        b.method_idx(DISPLAY, p, "getRefreshRate")
    };
    let m_disp_mode = {
        let p = b.proto_idx(DISPLAY_MODE, vec![]);
        b.method_idx(DISPLAY, p, "getMode")
    };
    let m_disp_modes = {
        let p = b.proto_idx(&format!("[{DISPLAY_MODE}"), vec![]);
        b.method_idx(DISPLAY, p, "getSupportedModes")
    };
    let m_mode_id = {
        let p = b.proto_idx("I", vec![]);
        b.method_idx(DISPLAY_MODE, p, "getModeId")
    };
    let m_mode_w = {
        let p = b.proto_idx("I", vec![]);
        b.method_idx(DISPLAY_MODE, p, "getPhysicalWidth")
    };
    let m_mode_h = {
        let p = b.proto_idx("I", vec![]);
        b.method_idx(DISPLAY_MODE, p, "getPhysicalHeight")
    };
    let m_mode_fps = {
        let p = b.proto_idx("F", vec![]);
        b.method_idx(DISPLAY_MODE, p, "getRefreshRate")
    };
    let m_obj_eq = {
        let p = b.proto_idx("Z", vec![OBJECT.into()]);
        b.method_idx(OBJECT, p, "equals")
    };

    let s_display = b.intern("display") as u16;
    let s_pres = b.intern("android.hardware.display.category.PRESENTATION") as u16;
    let t_dm = b.type_idx(DISPLAY_MANAGER);
    let t_listener = b.type_idx(TEST_LISTENER);

    // flutterBootProbe(Activity ctx) -> int (registers: v0..v7 locals, v8 = ctx):
    let mut probe = Vec::new();
    probe.extend(op21c(0x1a, 0, s_display)); // const-string v0, "display"
    probe.extend(op35c(0x6e, 2, m_get_service, [8, 0, 0, 0, 0])); // ctx.getSystemService("display")
    probe.extend(op11x(0x0c, 1)); // v1 = dm
    probe.extend(op21c(0x1f, 1, t_dm)); // check-cast v1, DisplayManager

    probe.extend(op21c(0x22, 2, t_listener)); // new-instance v2, VsyncListener
    probe.extend(op35c(0x70, 1, m_listener_init, [2, 0, 0, 0, 0]));
    probe.extend(op11n(0x12, 3, 0)); // v3 = null
    probe.extend(op35c(0x6e, 3, m_reg2, [1, 2, 3, 0, 0])); // dm.registerDisplayListener(l, null)

    probe.extend(op11n(0x12, 0, 0)); // v0 = 0
    probe.extend(op35c(0x6e, 2, m_get_display, [1, 0, 0, 0, 0])); // dm.getDisplay(0)
    probe.extend(op11x(0x0c, 4)); // v4 = d0

    probe.extend(op21s(0x13, 0, 99)); // v0 = 99
    probe.extend(op35c(0x6e, 2, m_get_display, [1, 0, 0, 0, 0])); // dm.getDisplay(99)
    probe.extend(op11x(0x0c, 5)); // v5 = d9 (null)

    probe.extend(op35c(0x6e, 1, m_disp_fps, [4, 0, 0, 0, 0])); // d0.getRefreshRate()
    probe.extend(op11x(0x0a, 6)); // v6 = fps (float)
    probe.extend(op12x(0x87, 6, 6)); // float-to-int v6 -> 60

    probe.extend(op35c(0x6e, 1, m_disp_mode, [4, 0, 0, 0, 0])); // d0.getMode()
    probe.extend(op11x(0x0c, 3)); // v3 = mode
    probe.extend(op35c(0x6e, 1, m_disp_modes, [4, 0, 0, 0, 0])); // d0.getSupportedModes()
    probe.extend(op11x(0x0c, 7)); // v7 = modes[]
    probe.extend(op11n(0x12, 0, 0));
    probe.extend(op23x(0x46, 0, 7, 0)); // aget-object v0, v7, v0
    probe.extend(op35c(0x6e, 2, m_obj_eq, [0, 3, 0, 0, 0])); // modes[0].equals(mode)
    probe.extend(op11x(0x0a, 0)); // v0 = 1
    probe.extend(op12x(0xb0, 6, 0)); // v6 += 1

    probe.extend(op35c(0x6e, 1, m_mode_id, [3, 0, 0, 0, 0])); // mode.getModeId() -> 1
    probe.extend(op11x(0x0a, 0));
    probe.extend(op_const32(7, 1_000_000));
    probe.extend(op12x(0xb2, 0, 7)); // v0 *= 1_000_000
    probe.extend(op12x(0xb0, 6, 0)); // v6 += 1_000_000

    probe.extend(op35c(0x6e, 1, m_mode_w, [3, 0, 0, 0, 0])); // mode.getPhysicalWidth() -> 720
    probe.extend(op11x(0x0a, 0));
    probe.extend(op22b(0xda, 0, 0, 100)); // * 100 -> 72_000
    probe.extend(op12x(0xb0, 6, 0));

    probe.extend(op35c(0x6e, 1, m_mode_h, [3, 0, 0, 0, 0])); // mode.getPhysicalHeight() -> 1440
    probe.extend(op11x(0x0a, 0));
    probe.extend(op35c(0x6e, 1, m_mode_fps, [3, 0, 0, 0, 0])); // mode.getRefreshRate() -> 60.0f
    probe.extend(op11x(0x0a, 7));
    probe.extend(op12x(0x87, 7, 7)); // -> 60
    probe.extend(op23x(0x93, 0, 0, 7)); // 1440 / 60 = 24
    probe.extend(op12x(0xb0, 6, 0));

    probe.extend(op35c(0x6e, 1, m_get_displays0, [1, 0, 0, 0, 0])); // dm.getDisplays()
    probe.extend(op11x(0x0c, 0));
    probe.extend(op12x(0x21, 0, 0)); // array-length -> 1
    probe.extend(op22b(0xda, 0, 0, 10)); // * 10 -> 10
    probe.extend(op12x(0xb0, 6, 0));

    probe.extend(op21c(0x1a, 0, s_pres));
    probe.extend(op35c(0x6e, 2, m_get_displays1, [1, 0, 0, 0, 0])); // dm.getDisplays(category)
    probe.extend(op11x(0x0c, 0));
    probe.extend(op12x(0x21, 0, 0)); // array-length -> 0
    probe.extend(op12x(0xb0, 6, 0));

    probe.extend(op_if_eqz(5, 4)); // if-eqz v5 (d9 == null), +4 -> skip add 999
    probe.extend(op22s(0xd0, 6, 6, 999));
    probe.extend(op35c(0x6e, 2, m_unreg, [1, 2, 0, 0, 0])); // dm.unregisterDisplayListener(l)
    probe.extend(op11x(0x0f, 6)); // return v6

    b.direct(
        hc,
        "flutterBootProbe",
        "I",
        vec![ACTIVITY],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(9, 1, 3, probe)),
    );

    b.direct(
        hc,
        "callReg2",
        "V",
        vec![DISPLAY_MANAGER, DISPLAY_LISTENER, HANDLER],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 3, 3, {
            let mut c = op35c(0x6e, 3, m_reg2, [0, 1, 2, 0, 0]);
            c.extend(op10x(0x0e));
            c
        })),
    );

    b.direct(
        hc,
        "callReg3",
        "V",
        vec![DISPLAY_MANAGER, DISPLAY_LISTENER, HANDLER, "J"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 5, 5, {
            let mut c = op35c(0x6e, 5, m_reg3, [0, 1, 2, 3, 4]);
            c.extend(op10x(0x0e));
            c
        })),
    );

    b.direct(
        hc,
        "callRegExec",
        "V",
        vec![DISPLAY_MANAGER, EXECUTOR, "J", DISPLAY_LISTENER],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 5, 5, {
            let mut c = op35c(0x6e, 5, m_reg_exec, [0, 1, 2, 3, 4]);
            c.extend(op10x(0x0e));
            c
        })),
    );

    b.direct(
        hc,
        "newListener",
        TEST_LISTENER,
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 1, {
            let mut c = op21c(0x22, 0, t_listener);
            c.extend(op35c(0x70, 1, m_listener_init, [0, 0, 0, 0, 0]));
            c.extend(op11x(0x11, 0));
            c
        })),
    );

    b.direct(
        hc,
        "callUnreg",
        "V",
        vec![DISPLAY_MANAGER, DISPLAY_LISTENER],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 2, 2, {
            let mut c = op35c(0x6e, 2, m_unreg, [0, 1, 0, 0, 0]);
            c.extend(op10x(0x0e));
            c
        })),
    );

    b
}

#[test]
fn flutter_vsync_waiter_boot_registers_listener_and_queries_display_mode() {
    let b = build_harness_dex();
    let mut vm = engine_of(&b);
    let act = vm
        .heap
        .alloc_instance(ACTIVITY.to_string(), Vec::new())
        .unwrap();

    let res = vm
        .invoke_static(
            TEST_HARNESS,
            "flutterBootProbe",
            "(Landroid/app/Activity;)I",
            &[Value::Obj(act)],
        )
        .unwrap();
    // 1_000_000 (modeId=1) + 72_000 (720*100) + 60 (fps) + 1 (equals) + 24 (1440/60) + 10 (displays.len*10) + 0 = 1_072_095
    assert_eq!(res, Value::Int(1_072_095));
}

#[test]
fn display_listener_callbacks_event_masks_dedup_and_gc_reachability() {
    let b = build_harness_dex();
    let mut vm = engine_of(&b);
    let act = vm
        .heap
        .alloc_instance(ACTIVITY.to_string(), Vec::new())
        .unwrap();

    let dm_str = vm.heap.alloc_string("display".to_string()).unwrap();
    let Value::Obj(dm) = vm
        .invoke_instance(
            act,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[Value::Obj(dm_str)],
        )
        .unwrap()
    else {
        panic!("expected DisplayManager instance");
    };

    // Pad early small ObjRef slots so conservative `Debug` integers in empty
    // `HostState` (`0..=16`) cannot coincidentally pin our listener slot.
    for _ in 0..24 {
        let _ = vm.heap.alloc_instance(OBJECT.to_string(), Vec::new());
    }
    let mk_listener = |vm: &mut rd_vm::Engine| -> u32 {
        match vm
            .invoke_static(TEST_HARNESS, "newListener", "()LVsyncListener;", &[])
            .unwrap()
        {
            Value::Obj(r) => r,
            other => panic!("expected listener ObjRef, got {other:?}"),
        }
    };
    let l_all = mk_listener(&mut vm);
    let l_changed_only = mk_listener(&mut vm);
    let l_exec = mk_listener(&mut vm);
    let exec_obj = vm
        .heap
        .alloc_instance(OBJECT.to_string(), Vec::new())
        .unwrap();

    // Register `l_all` twice -> must deduplicate to a single registration!
    for _ in 0..2 {
        vm.invoke_static(
            TEST_HARNESS,
            "callReg2",
            "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;)V",
            &[Value::Obj(dm), Value::Obj(l_all), Value::Null],
        )
        .unwrap();
    }

    // Register `l_changed_only` with mask = EVENT_FLAG_DISPLAY_CHANGED only.
    vm.invoke_static(
        TEST_HARNESS,
        "callReg3",
        "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;J)V",
        &[
            Value::Obj(dm),
            Value::Obj(l_changed_only),
            Value::Null,
            Value::Long(EVENT_FLAG_DISPLAY_CHANGED),
        ],
    )
    .unwrap();

    // Register `l_exec` via API 34 Executor overload for EVENT_FLAG_DISPLAY_REMOVED only.
    vm.invoke_static(
        TEST_HARNESS,
        "callRegExec",
        "(Landroid/hardware/display/DisplayManager;Ljava/util/concurrent/Executor;JLandroid/hardware/display/DisplayManager$DisplayListener;)V",
        &[
            Value::Obj(dm),
            Value::Obj(exec_obj),
            Value::Long(EVENT_FLAG_DISPLAY_REMOVED),
            Value::Obj(l_exec),
        ],
    )
    .unwrap();

    // Force a full GC: registered listeners live only inside
    // `DisplayManager.mDisplayListeners` (via `vm.statics`) and MUST survive!
    vm.gc();

    let field_i32 = |vm: &rd_vm::Engine, r: u32, f: &str| -> i32 {
        vm.heap
            .get_field(r, f)
            .unwrap_or(Value::Int(0))
            .as_int()
            .unwrap()
    };

    assert_eq!(
        dispatch_display_event(&mut vm, 0, EVENT_FLAG_DISPLAY_CHANGED).unwrap(),
        2
    );
    assert_eq!(field_i32(&vm, l_all, "changedCount"), 1);
    assert_eq!(field_i32(&vm, l_changed_only, "changedCount"), 1);
    assert_eq!(field_i32(&vm, l_exec, "changedCount"), 0);

    assert_eq!(
        dispatch_display_event(&mut vm, 7, EVENT_FLAG_DISPLAY_ADDED).unwrap(),
        1
    );
    assert_eq!(field_i32(&vm, l_all, "addedCount"), 1);
    assert_eq!(field_i32(&vm, l_all, "lastDisplayId"), 7);
    assert_eq!(field_i32(&vm, l_changed_only, "addedCount"), 0);

    // Unregister `l_all`, force GC -> `l_all` is unrooted and freed, and
    // subsequent REMOVED delivers only to `l_exec`!
    vm.invoke_static(
        TEST_HARNESS,
        "callUnreg",
        "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;)V",
        &[Value::Obj(dm), Value::Obj(l_all)],
    )
    .unwrap();
    vm.gc();
    assert!(
        matches!(vm.heap.get(l_all), Ok(HeapObj::Instance { class, .. }) if class == rd_vm::gc::FREED_CLASS)
    );

    assert_eq!(
        dispatch_display_event(&mut vm, 7, EVENT_FLAG_DISPLAY_REMOVED).unwrap(),
        1
    );
    assert_eq!(field_i32(&vm, l_exec, "removedCount"), 1);
    assert_eq!(field_i32(&vm, l_exec, "lastDisplayId"), 7);
}

#[test]
fn display_manager_null_and_empty_mask_throw_aosp_illegal_argument_exceptions() {
    let b = build_harness_dex();
    let mut vm = engine_of(&b);
    let act = vm
        .heap
        .alloc_instance(ACTIVITY.to_string(), Vec::new())
        .unwrap();

    let dm_str = vm.heap.alloc_string("display".to_string()).unwrap();
    let Value::Obj(dm) = vm
        .invoke_instance(
            act,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[Value::Obj(dm_str)],
        )
        .unwrap()
    else {
        panic!("expected DisplayManager instance");
    };
    let listener = vm
        .heap
        .alloc_instance(TEST_LISTENER.to_string(), Vec::new())
        .unwrap();
    let exec_obj = vm
        .heap
        .alloc_instance(OBJECT.to_string(), Vec::new())
        .unwrap();

    let assert_iae = |res: Result<Value, VmExit>, expected_substr: &str| match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;");
            let msg = t.message.unwrap_or_default();
            assert!(
                msg.contains(expected_substr),
                "expected {expected_substr:?} in {msg:?}"
            );
        }
        other => panic!("expected IllegalArgumentException, got {other:?}"),
    };

    assert_iae(
        vm.invoke_static(
            TEST_HARNESS,
            "callReg2",
            "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;)V",
            &[Value::Obj(dm), Value::Null, Value::Null],
        ),
        "listener must not be null",
    );

    assert_iae(
        vm.invoke_static(
            TEST_HARNESS,
            "callReg3",
            "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;Landroid/os/Handler;J)V",
            &[Value::Obj(dm), Value::Obj(listener), Value::Null, Value::Long(0)],
        ),
        "must not be empty",
    );

    assert_iae(
        vm.invoke_static(
            TEST_HARNESS,
            "callRegExec",
            "(Landroid/hardware/display/DisplayManager;Ljava/util/concurrent/Executor;JLandroid/hardware/display/DisplayManager$DisplayListener;)V",
            &[Value::Obj(dm), Value::Null, Value::Long(EVENT_FLAG_DISPLAY_CHANGED), Value::Obj(listener)],
        ),
        "executor must not be null",
    );

    assert_iae(
        vm.invoke_static(
            TEST_HARNESS,
            "callRegExec",
            "(Landroid/hardware/display/DisplayManager;Ljava/util/concurrent/Executor;JLandroid/hardware/display/DisplayManager$DisplayListener;)V",
            &[Value::Obj(dm), Value::Obj(exec_obj), Value::Long(EVENT_FLAG_DISPLAY_CHANGED), Value::Null],
        ),
        "listener must not be null",
    );

    assert_iae(
        vm.invoke_static(
            TEST_HARNESS,
            "callUnreg",
            "(Landroid/hardware/display/DisplayManager;Landroid/hardware/display/DisplayManager$DisplayListener;)V",
            &[Value::Obj(dm), Value::Null],
        ),
        "listener must not be null",
    );
}
