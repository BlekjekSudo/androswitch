//! Regression & integration tests for:
//! - Issue #231: Precise roots, ephemeron mark fixed-point, and sweep purging
//!   across `M8State`, `m9::State`, `AndroidxState`, and `GmsState` without
//!   scalar false-pinning or unbounded retention.
//! - Issue #240: Preserving `vm.native` across `load`, `.init_array`,
//!   `JNI_OnLoad`, and JNI → Java callbacks so active LocalRefs/GlobalRefs stay
//!   rooted, WeakGlobalRefs clear on sweep, and reentrant `System.loadLibrary`
//!   is idempotent.
//! - Issue #241: Resolving and dispatching host-backed JDK, Android framework,
//!   AndroidX, and GMS methods/constructors/static fields from JNI.
//! - Issue #242 (`native.rs`): Checked pointer arithmetic (`MEMORY_ACCESS`) on
//!   `RegisterNatives` and `Call*MethodA` overflow.

use rd_vm::framework::HostObj;
use rd_vm::gc::FREED_CLASS;
use rd_vm::gms::{Pending as GmsPending, TaskHost};
use rd_vm::heap::{ElemKind, ObjRef};
use rd_vm::{Engine, Value, VmConfig};

fn alloc_marker(vm: &mut Engine, tag: &str) -> ObjRef {
    let s = vm.heap.alloc_string(tag.to_string()).expect("alloc string");
    vm.heap
        .alloc_instance(
            "Ljava/lang/Object;".to_string(),
            vec![("tag".to_string(), Value::Obj(s))],
        )
        .expect("alloc marker")
}

fn is_freed(vm: &Engine, r: ObjRef) -> bool {
    vm.heap.class_of(r) == Ok(FREED_CLASS)
}

#[test]
fn host_subsystems_are_ephemerons_and_purged_on_gc() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    // Advance past small integer IDs (e.g. 1, 8, 160) that match fixed display
    // constants in `HostState` outside `m8`/`m9`/`androidx`/`gms`.
    for _ in 0..200 {
        let _ = vm
            .heap
            .alloc_instance("Ljava/lang/Object;".into(), vec![])
            .expect("warmup");
    }

    // ── Unreachable owners + children across M8, M9, AndroidX, and GMS ──
    let dead_cv = alloc_marker(&mut vm, "dead_cv");
    let dead_cv_child = alloc_marker(&mut vm, "dead_cv_child");
    vm.fw
        .m8
        .content_values
        .insert(dead_cv, vec![("k".to_string(), Value::Obj(dead_cv_child))]);

    let dead_intent = alloc_marker(&mut vm, "dead_intent");
    let dead_uri = alloc_marker(&mut vm, "dead_uri");
    let ist = rd_vm::m8::IntentState {
        data: Some(dead_uri),
        ..Default::default()
    };
    vm.fw.m8.intents.insert(dead_intent, ist);
    vm.fw
        .m8
        .uris
        .insert(dead_uri, "https://dead.invalid".into());

    #[cfg(feature = "advanced-components")]
    let (dead_builder, dead_pi) = {
        let b = alloc_marker(&mut vm, "dead_notif_builder");
        let pi = alloc_marker(&mut vm, "dead_pending_intent");
        let notif = rd_vm::m9::Notification {
            content_intent: Some(pi),
            ..Default::default()
        };
        vm.fw.m9.builders.insert(b, notif);
        (b, pi)
    };

    let dead_map = alloc_marker(&mut vm, "dead_array_map");
    let dead_map_val = alloc_marker(&mut vm, "dead_array_map_val");
    vm.fw
        .androidx
        .array_maps
        .insert(dead_map, vec![(Value::Int(1), Value::Obj(dead_map_val))]);

    let dead_tcs = alloc_marker(&mut vm, "dead_tcs");
    let dead_task = alloc_marker(&mut vm, "dead_task");
    let dead_listener = alloc_marker(&mut vm, "dead_listener");
    let dead_cont = alloc_marker(&mut vm, "dead_cont");
    let dead_out_task = alloc_marker(&mut vm, "dead_out_task");
    let mut th = TaskHost::default();
    th.success_listeners.push(dead_listener);
    vm.fw.gms.task_sources.insert(dead_tcs, dead_task);
    vm.fw.gms.tasks.insert(dead_task, th);
    vm.fw.gms.pending.insert(
        dead_task,
        vec![GmsPending::Cont {
            cont: dead_cont,
            success_only: false,
            flatten: false,
            out: dead_out_task,
        }],
    );

    // ── Live (rooted) owners + ephemeron-reachable children ─────────────
    let live_cv = alloc_marker(&mut vm, "live_cv");
    let live_cv_child = alloc_marker(&mut vm, "live_cv_child");
    vm.fw
        .m8
        .content_values
        .insert(live_cv, vec![("k".to_string(), Value::Obj(live_cv_child))]);

    #[cfg(feature = "advanced-components")]
    let (live_builder, live_pi) = {
        let b = alloc_marker(&mut vm, "live_notif_builder");
        let pi = alloc_marker(&mut vm, "live_pending_intent");
        let notif = rd_vm::m9::Notification {
            content_intent: Some(pi),
            ..Default::default()
        };
        vm.fw.m9.builders.insert(b, notif);
        vm.statics
            .insert(("LRoots;".into(), "b".into()), Value::Obj(b));
        (b, pi)
    };

    let live_map = alloc_marker(&mut vm, "live_array_map");
    let live_map_val = alloc_marker(&mut vm, "live_array_map_val");
    vm.fw
        .androidx
        .array_maps
        .insert(live_map, vec![(Value::Int(2), Value::Obj(live_map_val))]);

    let live_tcs = alloc_marker(&mut vm, "live_tcs");
    let live_task = alloc_marker(&mut vm, "live_task");
    let live_listener = alloc_marker(&mut vm, "live_listener");
    let live_cont = alloc_marker(&mut vm, "live_cont");
    let live_out_task = alloc_marker(&mut vm, "live_out_task");
    let mut live_th = TaskHost::default();
    live_th.success_listeners.push(live_listener);
    vm.fw.gms.task_sources.insert(live_tcs, live_task);
    vm.fw.gms.tasks.insert(live_task, live_th);
    vm.fw.gms.pending.insert(
        live_task,
        vec![GmsPending::Cont {
            cont: live_cont,
            success_only: false,
            flatten: false,
            out: live_out_task,
        }],
    );

    vm.statics
        .insert(("LRoots;".into(), "cv".into()), Value::Obj(live_cv));
    vm.statics
        .insert(("LRoots;".into(), "map".into()), Value::Obj(live_map));
    vm.statics
        .insert(("LRoots;".into(), "tcs".into()), Value::Obj(live_tcs));

    vm.gc();

    // Unreachable owners & children are reclaimed and purged from maps.
    for r in [
        dead_cv,
        dead_cv_child,
        dead_intent,
        dead_uri,
        dead_map,
        dead_map_val,
        dead_tcs,
        dead_task,
        dead_listener,
        dead_cont,
        dead_out_task,
    ] {
        assert!(is_freed(&vm, r), "expected ObjRef {r} to be collected");
    }
    assert!(!vm.fw.m8.content_values.contains_key(&dead_cv));
    assert!(!vm.fw.m8.intents.contains_key(&dead_intent));
    assert!(!vm.fw.m8.uris.contains_key(&dead_uri));
    assert!(!vm.fw.androidx.array_maps.contains_key(&dead_map));
    assert!(!vm.fw.gms.task_sources.contains_key(&dead_tcs));
    assert!(!vm.fw.gms.tasks.contains_key(&dead_task));
    assert!(!vm.fw.gms.pending.contains_key(&dead_task));

    #[cfg(feature = "advanced-components")]
    {
        assert!(is_freed(&vm, dead_builder));
        assert!(is_freed(&vm, dead_pi));
        assert!(!vm.fw.m9.builders.contains_key(&dead_builder));
        assert!(!is_freed(&vm, live_builder));
        assert!(!is_freed(&vm, live_pi));
        assert!(vm.fw.m9.builders.contains_key(&live_builder));
    }

    // Rooted owners & transitive ephemeron children survive intact.
    for r in [
        live_cv,
        live_cv_child,
        live_map,
        live_map_val,
        live_tcs,
        live_task,
        live_listener,
        live_cont,
        live_out_task,
    ] {
        assert!(!is_freed(&vm, r), "expected ObjRef {r} to stay live");
    }
    assert_eq!(vm.fw.m8.content_values.len(), 1);
    assert_eq!(vm.fw.androidx.array_maps.len(), 1);
    assert_eq!(vm.fw.gms.task_sources.len(), 1);
    assert_eq!(vm.fw.gms.tasks.len(), 1);
    assert_eq!(vm.fw.gms.pending.len(), 1);
}

#[test]
fn host_scalar_counters_do_not_false_pin_heap_objects() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    for _ in 0..200 {
        let _ = vm
            .heap
            .alloc_instance("Ljava/lang/Object;".into(), vec![])
            .expect("warmup");
    }
    let r1 = alloc_marker(&mut vm, "slot_m8_code");
    let r2 = alloc_marker(&mut vm, "slot_androidx_bio");
    let r3 = alloc_marker(&mut vm, "slot_gms_avail");
    let r4 = alloc_marker(&mut vm, "slot_gms_maps_counter");

    vm.fw.m8.http.code = r1 as i32;
    vm.fw.m8.scheduled_jobs.push(r1 as i32);
    vm.fw.androidx.biometric_can_authenticate = r2 as i32;
    vm.fw.gms.availability_code = r3 as i32;
    vm.fw.gms.maps_counter = r4 as u32;
    #[cfg(feature = "advanced-components")]
    let r5 = {
        let r5 = alloc_marker(&mut vm, "slot_m9_seq");
        vm.fw.m9.target_sdk = r5;
        vm.fw.m9.next_widget = r5 as i32;
        r5
    };

    vm.gc();

    for r in [r1, r2, r3, r4] {
        assert!(
            is_freed(&vm, r),
            "scalar integer equal to ObjRef({r}) must not false-pin heap slot"
        );
    }
    #[cfg(feature = "advanced-components")]
    assert!(
        is_freed(&vm, r5),
        "m9 scalar counter equal to ObjRef({r5}) must not false-pin heap slot"
    );
}

#[test]
fn repeated_gms_and_androidx_churn_stays_bounded_under_budget() {
    let cfg = VmConfig {
        heap_budget: 4 * 1024 * 1024,
        ..VmConfig::default()
    };
    let mut vm = Engine::new(vec![], cfg);
    let byte_kind = ElemKind::from_array_desc("[B").expect("byte array kind");

    for i in 0..1_500usize {
        let arr = vm
            .heap
            .alloc_array(byte_kind, 4096, "[B")
            .expect("4 KB byte array");
        vm.heap
            .array_elems_mut(arr)
            .expect("elems")
            .resize(4096, Value::Int((i & 0x7f) as i32));
        let map_obj = vm
            .heap
            .alloc_instance("Landroidx/collection/ArrayMap;".into(), vec![])
            .expect("map");
        vm.fw
            .androidx
            .array_maps
            .insert(map_obj, vec![(Value::Int(i as i32), Value::Obj(arr))]);

        let task_obj = vm
            .heap
            .alloc_instance("Lcom/google/android/gms/tasks/Task;".into(), vec![])
            .expect("task");
        let th = TaskHost {
            complete: true,
            result: Value::Obj(arr),
            ..Default::default()
        };
        vm.fw.gms.tasks.insert(task_obj, th);

        let cv_obj = vm
            .heap
            .alloc_instance("Landroid/content/ContentValues;".into(), vec![])
            .expect("cv");
        vm.fw
            .m8
            .content_values
            .insert(cv_obj, vec![("payload".into(), Value::Obj(arr))]);

        let _ = vm.maybe_gc();
    }

    let stats = vm.gc();
    assert!(stats.collections >= 2);
    // Out of 1,500 churned entries per subsystem, at most a handful of fixed
    // display constants in `HostState` can conservatively match a low slot ID.
    assert!(
        vm.fw.androidx.array_maps.len() <= 4,
        "array_maps leaked: {}",
        vm.fw.androidx.array_maps.len()
    );
    assert!(
        vm.fw.gms.tasks.len() <= 4,
        "gms.tasks leaked: {}",
        vm.fw.gms.tasks.len()
    );
    assert!(
        vm.fw.m8.content_values.len() <= 4,
        "m8.content_values leaked: {}",
        vm.fw.m8.content_values.len()
    );
    assert!(
        vm.heap.used() < 512 * 1024,
        "heap should stay well below 4 MB budget after churn, got {}",
        vm.heap.used()
    );
}

#[test]
fn minor_gc_preserves_young_children_of_old_ephemeron_owners() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    let old_map = alloc_marker(&mut vm, "old_map");
    let old_task = alloc_marker(&mut vm, "old_task");
    vm.statics
        .insert(("LGen;".into(), "map".into()), Value::Obj(old_map));
    vm.statics
        .insert(("LGen;".into(), "task".into()), Value::Obj(old_task));
    vm.fw.androidx.array_maps.insert(old_map, Vec::new());
    vm.fw.gms.tasks.insert(old_task, TaskHost::default());

    // Promote `old_map` and `old_task` to the old generation.
    vm.gc();

    // Mutate host ephemeron tables directly (no `Heap::get_mut` barrier) to
    // point at freshly allocated young objects.
    let young_map_child = alloc_marker(&mut vm, "young_map_child");
    let young_task_child = alloc_marker(&mut vm, "young_task_child");
    let young_garbage = alloc_marker(&mut vm, "young_garbage");
    vm.fw
        .androidx
        .array_maps
        .insert(old_map, vec![(Value::Int(1), Value::Obj(young_map_child))]);
    if let Some(th) = vm.fw.gms.tasks.get_mut(&old_task) {
        th.result = Value::Obj(young_task_child);
    }

    vm.gc_minor();

    assert!(!is_freed(&vm, young_map_child));
    assert!(!is_freed(&vm, young_task_child));
    assert!(is_freed(&vm, young_garbage));

    // Also verifyHostObj::Bundle remains untouched as a control.
    let b = alloc_marker(&mut vm, "bundle_ctrl");
    vm.fw.objects.insert(
        b,
        HostObj::Bundle {
            map: vec![("x".into(), Value::Int(1))],
        },
    );
    vm.gc();
    assert!(is_freed(&vm, b));
}

#[cfg(feature = "native")]
mod native_jni_tests {
    use rd_testsupport::{apkfix::*, vm::*};
    use rd_vm::{Engine, Value, VmConfig, VmExit};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    const C_SRC: &str = r#"
typedef int jint;
typedef unsigned char jboolean;
typedef long long jlong;
typedef unsigned long uintptr_t;
typedef void *jobject;
typedef void *jclass;
typedef void *jstring;
typedef void *jweak;
typedef void **JNIEnv;
typedef void **JavaVM;
typedef union { jint i; jlong j; jobject l; } jvalue;
typedef struct { const char *name; const char *signature; void *fnPtr; } JNINativeMethod;
#define JCALL(e,n,t) ((t)((*e)[n]))

static jobject g_onload_strong = 0;
static jweak g_onload_weak_dead = 0;

jint JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void)reserved;
    JNIEnv *e = 0;
    if (((jint(*)(JavaVM*, void**, jint))((*vm)[6]))(vm, (void**)&e, 0x10006) != 0) {
        return -1;
    }
    /* Strong GlobalRef allocated during JNI_OnLoad BEFORE Java callback triggers GC. */
    jstring s = JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, "onload-survived-gc");
    g_onload_strong = JCALL(e, 21, jobject(*)(JNIEnv*, jobject))(e, s);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, s);

    /* WeakGlobalRef to an unreferenced StringBuilder allocated during JNI_OnLoad. */
    jclass sb_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/StringBuilder");
    void *sb_init = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sb_cls, "<init>", "()V");
    jobject ephemeral = JCALL(e, 30, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, sb_cls, sb_init, 0);
    g_onload_weak_dead = JCALL(e, 226, jweak(*)(JNIEnv*, jobject))(e, ephemeral);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, ephemeral);

    /* Upcall into DEX that reentrantly calls System.loadLibrary("jnibridge")
       and churns heap allocations past the GC threshold! */
    jclass app_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "test/JniHost");
    void *churn = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, app_cls, "reloadAndChurnGc", "()V");
    JCALL(e, 143, void(*)(JNIEnv*, jclass, void*, const jvalue*))(e, app_cls, churn, 0);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return -1;
    }
    return 0x10006;
}

jstring Java_test_JniHost_verifyOnLoadAndLocalRoots(JNIEnv *e, jclass c, jobject passed_from_java) {
    /* 1. Verify g_onload_weak_dead was cleared by the GC inside JNI_OnLoad! */
    jint ref_ty = JCALL(e, 232, jint(*)(JNIEnv*, jobject))(e, g_onload_weak_dead);
    jboolean weak_is_null = JCALL(e, 24, jboolean(*)(JNIEnv*, jobject, jobject))(
        e, g_onload_weak_dead, 0);
    jobject promoted = JCALL(e, 25, jobject(*)(JNIEnv*, jobject))(e, g_onload_weak_dead);
    if (ref_ty != 3 || !weak_is_null || promoted != 0) {
        return JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, "ERR_WEAK_NOT_CLEARED");
    }

    /* 2. Create a WeakGlobalRef to `passed_from_java` (which IS strongly rooted
          by our active JNI LocalRef frame!) and trigger another GC callback. */
    jweak weak_live = JCALL(e, 226, jweak(*)(JNIEnv*, jobject))(e, passed_from_java);
    void *churn = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, c, "reloadAndChurnGc", "()V");
    JCALL(e, 143, void(*)(JNIEnv*, jclass, void*, const jvalue*))(e, c, churn, 0);

    jboolean still_same = JCALL(e, 24, jboolean(*)(JNIEnv*, jobject, jobject))(
        e, weak_live, passed_from_java);
    JCALL(e, 227, void(*)(JNIEnv*, jweak))(e, weak_live);
    if (!still_same) {
        return JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, "ERR_LIVE_WEAK_LOST");
    }

    /* 3. Concatenate g_onload_strong + ":" + passed_from_java.toString() via host StringBuilder! */
    jclass sb_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/StringBuilder");
    void *sb_init = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sb_cls, "<init>", "(Ljava/lang/String;)V");
    void *sb_append = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sb_cls, "append", "(Ljava/lang/String;)Ljava/lang/StringBuilder;");
    void *obj_tostr = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sb_cls, "toString", "()Ljava/lang/String;");

    jvalue a1[1];
    a1[0].l = g_onload_strong;
    jobject sb = JCALL(e, 30, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, sb_cls, sb_init, a1);

    a1[0].l = JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, ":");
    JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(e, sb, sb_append, a1);

    jclass obj_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/Object");
    void *any_tostr = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, obj_cls, "toString", "()Ljava/lang/String;");
    a1[0].l = JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, passed_from_java, any_tostr, 0);
    JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(e, sb, sb_append, a1);

    return (jstring)JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, sb, obj_tostr, 0);
}

jint Java_test_JniHost_exerciseHostDispatch(JNIEnv *e, jclass c) {
    /* 1. Pure DEX missing method MUST set NoSuchMethodError and return NULL. */
    void *bad = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, c, "noSuchMethodOnAppClass", "()V");
    if (bad != 0 || !JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return -10;
    }
    JCALL(e, 17, void(*)(JNIEnv*))(e); /* ExceptionClear */

    /* 2. AllocObject (slot 27) on android/os/Bundle attaches HostObj::Bundle! */
    jclass bundle_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "android/os/Bundle");
    jobject bundle = JCALL(e, 27, jobject(*)(JNIEnv*, jclass))(e, bundle_cls);
    void *put_int = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, bundle_cls, "putInt", "(Ljava/lang/String;I)V");
    void *get_int = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, bundle_cls, "getInt", "(Ljava/lang/String;)I");
    jstring key = JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, "answer");
    jvalue bargs[2];
    bargs[0].l = key;
    bargs[1].i = 42;
    JCALL(e, 63, void(*)(JNIEnv*, jobject, void*, const jvalue*))(e, bundle, put_int, bargs);
    jvalue gargs[1];
    gargs[0].l = key;
    jint got_bundle = JCALL(e, 51, jint(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, bundle, get_int, gargs);
    if (got_bundle != 42) {
        return -20;
    }

    /* 3. AndroidX ArrayMap via NewObjectA + put + size */
    jclass am_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(
        e, "androidx/collection/ArrayMap");
    void *am_init = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, am_cls, "<init>", "()V");
    jobject am = JCALL(e, 30, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, am_cls, am_init, 0);
    void *am_put = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, am_cls, "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;");
    void *am_size = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, am_cls, "size", "()I");
    bargs[0].l = key;
    bargs[1].l = key;
    JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(e, am, am_put, bargs);
    jint am_sz = JCALL(e, 51, jint(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, am, am_size, 0);
    if (am_sz != 1) {
        return -30;
    }

    /* 4. GMS Tasks.forResult(Object) + Task.isSuccessful() */
    jclass tasks_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(
        e, "com/google/android/gms/tasks/Tasks");
    void *for_res = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, tasks_cls, "forResult", "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;");
    gargs[0].l = key;
    jobject task = JCALL(e, 116, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, tasks_cls, for_res, gargs);
    jclass task_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(
        e, "com/google/android/gms/tasks/Task");
    void *is_succ = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, task_cls, "isSuccessful", "()Z");
    jboolean ok = JCALL(e, 39, jboolean(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, task, is_succ, 0);
    if (!ok) {
        return -40;
    }

    /* 5. Host static field: android/os/Build$VERSION.SDK_INT */
    jclass ver_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(
        e, "android/os/Build$VERSION");
    void *sdk_fid = JCALL(e, 144, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, ver_cls, "SDK_INT", "I");
    jint sdk_int = JCALL(e, 149, jint(*)(JNIEnv*, jclass, void*))(e, ver_cls, sdk_fid);

    return got_bundle + am_sz + (jint)ok + sdk_int; /* 42 + 1 + 1 + 34 = 78 */
}

jint Java_test_JniHost_overflowRegisterNatives(JNIEnv *e, jclass c) {
    const JNINativeMethod *bad_ptr = (const JNINativeMethod *)(uintptr_t)0xfffffffffffffff0ULL;
    return JCALL(e, 215, jint(*)(JNIEnv*, jclass, const JNINativeMethod*, jint))(
        e, c, bad_ptr, 2);
}

jint Java_test_JniHost_overflowCallMethodA(JNIEnv *e, jclass c) {
    (void)c;
    jclass sys = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/Math");
    void *mx = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sys, "max", "(II)I");
    const jvalue *bad_args = (const jvalue *)(uintptr_t)0xfffffffffffffffcULL;
    return JCALL(e, 131, jint(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, sys, mx, bad_args);
}

jobject Java_test_JniHost_makeNormalizedBooleans(JNIEnv *e, jclass c) {
    (void)c;
    /* NewBooleanArray (slot 175) + SetBooleanArrayRegion (slot 207) with raw {0, 2, 255} */
    jobject arr = JCALL(e, 175, jobject(*)(JNIEnv*, jint))(e, 3);
    unsigned char raw[3] = {0, 2, 255};
    JCALL(e, 207, void(*)(JNIEnv*, jobject, jint, jint, const unsigned char*))(e, arr, 0, 3, raw);
    return arr;
}
"#;

    fn compile_x86_64_so() -> Vec<u8> {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir();
        let src_path = dir.join(format!("rd_jni_host_{}_{}.c", std::process::id(), id));
        let so_path = dir.join(format!("rd_jni_host_{}_{}.so", std::process::id(), id));
        std::fs::write(&src_path, C_SRC).expect("write C fixture");
        // Pin the x86-64 baseline: CI runners (Ubuntu 26.04) default to x86-64-v3, whose
        // VEX code the native worker rejects (issue #400). Same pin as golden/native/build.sh.
        let status = Command::new("gcc")
            .args([
                "-shared",
                "-march=x86-64",
                "-fPIC",
                "-O1",
                "-nostdlib",
                "-fno-stack-protector",
                "-Wl,--hash-style=both",
                "-Wl,--build-id=none",
            ])
            .arg(&src_path)
            .arg("-o")
            .arg(&so_path)
            .status()
            .expect("invoke host gcc");
        let _ = std::fs::remove_file(&src_path);
        assert!(status.success(), "gcc failed compiling test fixture");
        let bytes = std::fs::read(&so_path).expect("read compiled .so");
        let _ = std::fs::remove_file(&so_path);
        bytes
    }

    fn build_jni_engine() -> Engine {
        const CLS: &str = "Ltest/JniHost;";
        let mut b = DexBuilder::new();
        let c = b.class(CLS, "Ljava/lang/Object;");
        for (name, ret, params) in [
            (
                "verifyOnLoadAndLocalRoots",
                "Ljava/lang/String;",
                vec!["Ljava/lang/Object;"],
            ),
            ("exerciseHostDispatch", "I", vec![]),
            ("overflowRegisterNatives", "I", vec![]),
            ("overflowCallMethodA", "I", vec![]),
            ("makeNormalizedBooleans", "[Z", vec![]),
        ] {
            b.direct(c, name, ret, params, 0x109, None);
        }

        // static void reloadAndChurnGc():
        //   1. System.loadLibrary("jnibridge")  (reentrant idempotent check!)
        //   2. Loop 8 times allocating 256 KB `[B` arrays (~2 MB > 1.5 MB threshold)
        //      so `interp` runs `vm.maybe_gc()` -> major GC!
        let lib_str = b.intern("jnibridge") as u16;
        let load_lib_p = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
        let load_lib_m = b.method_idx("Ljava/lang/System;", load_lib_p, "loadLibrary");
        let byte_arr_ty = b.type_idx("[B");
        let mut units = Vec::new();
        // v0 = "jnibridge"; invoke-static {v0}, System.loadLibrary
        units.extend(op21c(0x1a, 0, lib_str));
        units.extend(op35c(0x71, 1, load_lib_m, [0, 0, 0, 0, 0]));
        // v1 = 32 (loop counter), v2 = 8192 (~196 KB per array -> ~6.2 MB churn > 2 MB budget!)
        units.extend(op21s(0x13, 1, 32));
        units.extend(op31t(0x14, 2, 8_192));
        // loop: new-array v0, v2, [B; add-int/lit8 v1, v1, -1; if-gtz v1, rel
        let loop_pc = units.len();
        units.extend(op22c(0x23, 0, 2, byte_arr_ty));
        units.extend(op22b(0xd8, 1, 1, -1));
        let branch_pc = units.len();
        let rel = (loop_pc as isize - branch_pc as isize) as i16;
        units.extend(op21s(0x3b, 1, rel));
        // Drop last array ref from v0 before final safe point: const/4 v0, 0; return-void
        units.extend(op11n(0x12, 0, 0));
        units.extend(op10x(0x0e));
        let code = b.code(3, 0, 1, units);
        b.direct(c, "reloadAndChurnGc", "V", vec![], 0x9, Some(code));

        // static String runLocalRootTest():
        let sb_ty = b.type_idx("Ljava/lang/StringBuilder;");
        let sb_init_p = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
        let sb_init = b.method_idx("Ljava/lang/StringBuilder;", sb_init_p, "<init>");
        let arg_str = b.intern("local-arg-ok") as u16;
        let verify_p = b.proto_idx("Ljava/lang/String;", vec!["Ljava/lang/Object;".into()]);
        let verify_m = b.method_idx(CLS, verify_p, "verifyOnLoadAndLocalRoots");
        let mut u2 = Vec::new();
        u2.extend(op21c(0x22, 0, sb_ty)); // new-instance v0, StringBuilder
        u2.extend(op21c(0x1a, 1, arg_str)); // const-string v1, "local-arg-ok"
        u2.extend(op35c(0x70, 2, sb_init, [0, 1, 0, 0, 0])); // invoke-direct <init>
        u2.extend(op35c(0x71, 1, verify_m, [0, 0, 0, 0, 0])); // invoke-static verifyOnLoadAndLocalRoots
        u2.extend(op11x(0x0c, 0)); // move-result-object v0
        u2.extend(op11x(0x11, 0)); // return-object v0
        let code2 = b.code(2, 0, 2, u2);
        b.direct(
            c,
            "runLocalRootTest",
            "Ljava/lang/String;",
            vec![],
            0x9,
            Some(code2),
        );

        let dex = b.finish();
        let mut z = ZipFix::new();
        z.add("AndroidManifest.xml", build_manifest_axml("test", CLS));
        z.add("classes.dex", dex.clone());
        z.add("lib/x86_64/libjnibridge.so", compile_x86_64_so());

        let apk = rd_apk::Apk::from_bytes(z.build()).expect("parse apk");
        let cfg = VmConfig {
            heap_budget: 2 * 1024 * 1024,
            ..VmConfig::default()
        };
        let mut vm = Engine::new(vec![rd_dex::Dex::parse(dex).expect("parse dex")], cfg);
        for _ in 0..200 {
            let _ = vm
                .heap
                .alloc_instance("Ljava/lang/Object;".into(), vec![])
                .expect("warmup past low IDs");
        }
        vm.set_resources(apk);
        rd_vm::native::load(&mut vm, "jnibridge").expect("JNI_OnLoad + reentrant loadLibrary");
        vm
    }

    #[test]
    fn jni_onload_and_callback_gc_roots_and_weak_globals() {
        let mut vm = build_jni_engine();
        // `JNI_OnLoad` already triggered at least one GC while `vm.native` stayed active!
        assert!(
            vm.gc_stats().collections >= 1,
            "expected GC to run during JNI_OnLoad callback"
        );

        // Idempotent repeat load from Rust host side as well.
        rd_vm::native::load(&mut vm, "jnibridge").expect("repeat load");

        let out = vm
            .invoke_static(
                "Ltest/JniHost;",
                "runLocalRootTest",
                "()Ljava/lang/String;",
                &[],
            )
            .expect("verifyOnLoadAndLocalRoots");
        let Value::Obj(s_ref) = out else {
            panic!("expected String return, got {out:?}");
        };
        assert_eq!(
            vm.heap.as_str(s_ref).expect("as_str"),
            "onload-survived-gc:local-arg-ok"
        );
    }

    #[test]
    fn jni_resolves_and_dispatches_host_jdk_android_androidx_and_gms_methods() {
        let mut vm = build_jni_engine();
        let out = vm
            .invoke_static("Ltest/JniHost;", "exerciseHostDispatch", "()I", &[])
            .expect("exerciseHostDispatch");
        assert_eq!(out, Value::Int(78));
    }

    #[test]
    fn jni_pointer_overflow_in_register_natives_and_call_method_a_is_rejected() {
        let mut vm = build_jni_engine();

        let err1 = vm
            .invoke_static("Ltest/JniHost;", "overflowRegisterNatives", "()I", &[])
            .expect_err("RegisterNatives pointer overflow must fail");
        match err1 {
            VmExit::Error(e) => assert_eq!(e.code, "MEMORY_ACCESS", "got {e:?}"),
            other => panic!("expected VmExit::Error(MEMORY_ACCESS), got {other:?}"),
        }

        let err2 = vm
            .invoke_static("Ltest/JniHost;", "overflowCallMethodA", "()I", &[])
            .expect_err("Call*MethodA pointer overflow must fail");
        match err2 {
            VmExit::Error(e) => assert_eq!(e.code, "MEMORY_ACCESS", "got {e:?}"),
            other => panic!("expected VmExit::Error(MEMORY_ACCESS), got {other:?}"),
        }

        // Issue #250 (`native.rs` slice): jbooleanArray writes normalize non-zero bytes to 1.
        let bools = vm
            .invoke_static("Ltest/JniHost;", "makeNormalizedBooleans", "()[Z", &[])
            .expect("makeNormalizedBooleans");
        let Value::Obj(arr_ref) = bools else {
            panic!("expected [Z array, got {bools:?}");
        };
        let (elems, _) = vm.heap.as_array_elems(arr_ref).expect("bool elems");
        assert_eq!(elems, &[Value::Int(0), Value::Int(1), Value::Int(1)]);
    }
}
