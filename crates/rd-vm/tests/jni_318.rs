//! Regression tests for Issue #318 (`rd-vm/jni` soundness):
//! 1. Array type safety in `NewObjectArray`, `GetObjectArrayElement`,
//!    `SetObjectArrayElement` (`ArrayStoreException` on element mismatch,
//!    `JNI_ERROR` on primitive arrays), and `array_bytes`/`write_array`
//!    (`JNI_ERROR` on object or mismatched primitive arrays).
//! 2. 32-bit `jsize` ABI truncation (`as i32`) across 64-bit registers and
//!    catchable pending `ArrayIndexOutOfBoundsException` /
//!    `StringIndexOutOfBoundsException` / `OutOfMemoryError` instead of
//!    aborting with `VmExit::Error("JNI_ERROR")`.
//! 3. DEX superclass hierarchy default field population in `AllocObject` and
//!    `NewObject` (plus `Get<Type>Field` default fallback).
//! 4. Preserving and GC-rooting the original `Throwable` `ObjRef` across
//!    `Throw`, `ThrowNew`, `ExceptionOccurred`, JNI callback unwinding, and
//!    returning pending exceptions to Java.

#![cfg(all(feature = "native", target_arch = "x86_64"))]

use rd_testsupport::{apkfix::*, vm::*};
use rd_vm::gc::FREED_CLASS;
use rd_vm::heap::ElemKind;
use rd_vm::{Engine, Value, VmConfig, VmExit};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

const CLS_HOST: &str = "Ltest/Jni318;";
const CLS_BASE: &str = "Ltest/BaseModel;";
const CLS_CHILD: &str = "Ltest/ChildModel;";
const CLS_EX: &str = "Ltest/CustomJniException;";

const C_SRC: &str = r#"
typedef int jint;
typedef jint jsize;
typedef unsigned char jboolean;
typedef unsigned short jchar;
typedef long long jlong;
typedef double jdouble;
typedef unsigned long long uint64_t;
typedef void *jobject;
typedef void *jclass;
typedef void *jstring;
typedef void *jarray;
typedef void *jobjectArray;
typedef void *jintArray;
typedef void *jthrowable;
typedef void **JNIEnv;
typedef union { jint i; jlong j; jdouble d; jobject l; } jvalue;
#define JCALL(e,n,t) ((t)((*e)[n]))

static int check_and_clear_exc(JNIEnv *e, const char *want_cls_slash) {
    if (!JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return 0;
    }
    jthrowable ex = JCALL(e, 15, jthrowable(*)(JNIEnv*))(e);
    JCALL(e, 17, void(*)(JNIEnv*))(e);
    if (!ex) {
        return 0;
    }
    jclass want = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, want_cls_slash);
    if (!want) {
        JCALL(e, 17, void(*)(JNIEnv*))(e);
        return 0;
    }
    jboolean ok = JCALL(e, 32, jboolean(*)(JNIEnv*, jobject, jclass))(e, ex, want);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, ex);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, want);
    return ok ? 1 : 0;
}

/* Bug 1a: SetObjectArrayElement checks component subtyping + NewObjectArray nested "[I" -> "[[I". */
jint Java_test_Jni318_exerciseObjectArraySafety(JNIEnv *e, jclass c) {
    (void)c;
    jclass str_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/String");
    jstring init_s = JCALL(e, 167, jstring(*)(JNIEnv*, const char*))(e, "keep-me");
    jobjectArray sarr = JCALL(e, 172, jobjectArray(*)(JNIEnv*, jsize, jclass, jobject))(
        e, 2, str_cls, init_s);
    if (!sarr) return -1;

    /* Construct a StringBuilder (NOT a String!) and try storing it into String[]. */
    jclass sb_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/StringBuilder");
    void *sb_init = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, sb_cls, "<init>", "()V");
    jobject sb = JCALL(e, 30, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
        e, sb_cls, sb_init, 0);

    JCALL(e, 174, void(*)(JNIEnv*, jobjectArray, jsize, jobject))(e, sarr, 0, sb);
    if (!check_and_clear_exc(e, "java/lang/ArrayStoreException")) {
        return -2;
    }

    /* Slot 0 must remain untouched ("keep-me"), never corrupted to StringBuilder. */
    jobject elem0 = JCALL(e, 173, jobject(*)(JNIEnv*, jobjectArray, jsize))(e, sarr, 0);
    if (!JCALL(e, 32, jboolean(*)(JNIEnv*, jobject, jclass))(e, elem0, str_cls)) {
        return -3;
    }

    /* NewObjectArray withIncompatible init must also raise ArrayStoreException and return NULL. */
    jobjectArray bad_arr = JCALL(e, 172, jobjectArray(*)(JNIEnv*, jsize, jclass, jobject))(
        e, 1, str_cls, sb);
    if (bad_arr != 0 || !check_and_clear_exc(e, "java/lang/ArrayStoreException")) {
        return -4;
    }

    /* Subtype store: CustomJniException into Throwable[] must succeed! */
    jclass thr_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/Throwable");
    jclass cex_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "test/CustomJniException");
    jobject cex = JCALL(e, 27, jobject(*)(JNIEnv*, jclass))(e, cex_cls);
    jobjectArray tarr = JCALL(e, 172, jobjectArray(*)(JNIEnv*, jsize, jclass, jobject))(
        e, 1, thr_cls, 0);
    JCALL(e, 174, void(*)(JNIEnv*, jobjectArray, jsize, jobject))(e, tarr, 0, cex);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return -5;
    }

    /* Nested array descriptor: NewObjectArray(2, "[I", row) must produce "[[I" so IsInstanceOf("[[I") is true! */
    jclass int_arr_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "[I");
    jclass int2_arr_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "[[I");
    jintArray row = JCALL(e, 179, jintArray(*)(JNIEnv*, jsize))(e, 3);
    jobjectArray mat = JCALL(e, 172, jobjectArray(*)(JNIEnv*, jsize, jclass, jobject))(
        e, 2, int_arr_cls, row);
    if (!mat || !JCALL(e, 32, jboolean(*)(JNIEnv*, jobject, jclass))(e, mat, int2_arr_cls)) {
        return -6;
    }

    return 100;
}

/* Bug 1b: Calling SetObjectArrayElement on int[] must fail with JNI_ERROR. */
void Java_test_Jni318_setObjectOnIntArray(JNIEnv *e, jclass c, jintArray arr, jobject val) {
    (void)c;
    JCALL(e, 174, void(*)(JNIEnv*, jobjectArray, jsize, jobject))(e, (jobjectArray)arr, 0, val);
}

/* Bug 1c: Calling GetIntArrayRegion on String[] must fail with JNI_ERROR. */
void Java_test_Jni318_getIntRegionOnObjectArray(JNIEnv *e, jclass c, jobjectArray arr) {
    (void)c;
    jint buf[1] = {0};
    JCALL(e, 203, void(*)(JNIEnv*, jintArray, jsize, jsize, jint*))(e, (jintArray)arr, 0, 1, buf);
}

/* Bug 1d: Calling GetLongArrayElements on int[] must fail with JNI_ERROR. */
void Java_test_Jni318_getLongElemsOnIntArray(JNIEnv *e, jclass c, jintArray arr) {
    (void)c;
    (void)JCALL(e, 188, jlong*(*)(JNIEnv*, jobject, jboolean*))(e, arr, 0);
}

/* Bug 2: 64-bit upper-garbage jsize truncation + pending bounds exceptions. */
jint Java_test_Jni318_exerciseJsizeAndBounds(JNIEnv *e, jclass c) {
    (void)c;
    const uint64_t HI = 0xDEADBEEF00000000ULL;

    /* 1. Dirty upper 32 bits with valid positive low 32-bit jsize values MUST succeed! */
    if (JCALL(e, 26, jint(*)(JNIEnv*, uint64_t))(e, HI | 16ULL) != 0) {
        return -10;
    }

    jchar uchars[3] = { 'A', 'B', 'C' };
    jstring js = JCALL(e, 163, jstring(*)(JNIEnv*, const jchar*, uint64_t))(
        e, uchars, HI | 3ULL);
    if (!js || JCALL(e, 164, jsize(*)(JNIEnv*, jstring))(e, js) != 3) {
        return -11;
    }

    jchar out_u[2] = { 0, 0 };
    JCALL(e, 220, void(*)(JNIEnv*, jstring, uint64_t, uint64_t, jchar*))(
        e, js, HI | 1ULL, HI | 2ULL, out_u);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e) || out_u[0] != 'B' || out_u[1] != 'C') {
        return -12;
    }

    char out_utf[8] = { 0 };
    JCALL(e, 221, void(*)(JNIEnv*, jstring, uint64_t, uint64_t, char*))(
        e, js, HI | 0ULL, HI | 2ULL, out_utf);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e) || out_utf[0] != 'A' || out_utf[1] != 'B' || out_utf[2] != 0) {
        return -13;
    }

    jintArray ia = JCALL(e, 179, jintArray(*)(JNIEnv*, jsize))(e, 3);
    jint src[2] = { 42, 99 };
    JCALL(e, 211, void(*)(JNIEnv*, jintArray, uint64_t, uint64_t, const jint*))(
        e, ia, HI | 1ULL, HI | 2ULL, src);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return -14;
    }
    jint dst[2] = { 0, 0 };
    JCALL(e, 203, void(*)(JNIEnv*, jintArray, uint64_t, uint64_t, jint*))(
        e, ia, HI | 1ULL, HI | 2ULL, dst);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e) || dst[0] != 42 || dst[1] != 99) {
        return -15;
    }

    jclass str_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/String");
    jobjectArray oa = JCALL(e, 172, jobjectArray(*)(JNIEnv*, jsize, jclass, jobject))(
        e, 2, str_cls, 0);
    JCALL(e, 174, void(*)(JNIEnv*, jobjectArray, uint64_t, jobject))(e, oa, HI | 1ULL, js);
    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return -16;
    }
    jobject got = JCALL(e, 173, jobject(*)(JNIEnv*, jobjectArray, uint64_t))(e, oa, HI | 1ULL);
    if (!got || !JCALL(e, 24, jboolean(*)(JNIEnv*, jobject, jobject))(e, got, js)) {
        return -17;
    }

    /* 2. Negative / out-of-bounds indices MUST set pending Java exceptions, NOT abort with JNI_ERROR! */
    if (JCALL(e, 26, jint(*)(JNIEnv*, jint))(e, -5) >= 0
        || !check_and_clear_exc(e, "java/lang/OutOfMemoryError")) {
        return -20;
    }

    JCALL(e, 173, jobject(*)(JNIEnv*, jobjectArray, jint))(e, oa, -1);
    if (!check_and_clear_exc(e, "java/lang/ArrayIndexOutOfBoundsException")) {
        return -21;
    }
    JCALL(e, 174, void(*)(JNIEnv*, jobjectArray, jint, jobject))(e, oa, 5, js);
    if (!check_and_clear_exc(e, "java/lang/ArrayIndexOutOfBoundsException")) {
        return -22;
    }
    JCALL(e, 203, void(*)(JNIEnv*, jintArray, jint, jint, jint*))(e, ia, -1, 1, dst);
    if (!check_and_clear_exc(e, "java/lang/ArrayIndexOutOfBoundsException")) {
        return -23;
    }
    JCALL(e, 211, void(*)(JNIEnv*, jintArray, jint, jint, const jint*))(e, ia, 2, 2, src);
    if (!check_and_clear_exc(e, "java/lang/ArrayIndexOutOfBoundsException")) {
        return -24;
    }
    JCALL(e, 220, void(*)(JNIEnv*, jstring, jint, jint, jchar*))(e, js, -1, 1, out_u);
    if (!check_and_clear_exc(e, "java/lang/StringIndexOutOfBoundsException")) {
        return -25;
    }
    JCALL(e, 221, void(*)(JNIEnv*, jstring, jint, jint, char*))(e, js, 2, 5, out_utf);
    if (!check_and_clear_exc(e, "java/lang/StringIndexOutOfBoundsException")) {
        return -26;
    }

    return 200;
}

/* Bug 3: AllocObject and NewObject populate DEX superclass + subclass default fields. */
jobject Java_test_Jni318_allocAndVerifyDefaults(JNIEnv *e, jclass c, jboolean use_new_object) {
    (void)c;
    jclass base_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "test/BaseModel");
    jclass child_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "test/ChildModel");

    jobject obj = 0;
    if (use_new_object) {
        void *init = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
            e, child_cls, "<init>", "()V");
        obj = JCALL(e, 30, jobject(*)(JNIEnv*, jclass, void*, const jvalue*))(
            e, child_cls, init, 0);
    } else {
        obj = JCALL(e, 27, jobject(*)(JNIEnv*, jclass))(e, child_cls);
    }
    if (!obj || JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return 0;
    }

    void *f_base_count = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, base_cls, "baseCount", "I");
    void *f_base_stamp = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, base_cls, "baseStamp", "J");
    void *f_base_tag = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, base_cls, "baseTag", "Ljava/lang/String;");
    void *f_child_code = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, child_cls, "childCode", "I");
    void *f_ratio = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, child_cls, "ratio", "D");
    void *f_payload = JCALL(e, 94, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, child_cls, "payload", "Ljava/lang/Object;");

    jint bc = JCALL(e, 99, jint(*)(JNIEnv*, jobject, void*))(e, obj, f_base_count);
    jlong bs = JCALL(e, 100, jlong(*)(JNIEnv*, jobject, void*))(e, obj, f_base_stamp);
    jobject bt = JCALL(e, 95, jobject(*)(JNIEnv*, jobject, void*))(e, obj, f_base_tag);
    jint cc = JCALL(e, 99, jint(*)(JNIEnv*, jobject, void*))(e, obj, f_child_code);
    jdouble rt = JCALL(e, 102, jdouble(*)(JNIEnv*, jobject, void*))(e, obj, f_ratio);
    jobject pl = JCALL(e, 95, jobject(*)(JNIEnv*, jobject, void*))(e, obj, f_payload);

    if (JCALL(e, 228, jboolean(*)(JNIEnv*))(e)) {
        return 0;
    }
    if (bc != 0 || bs != 0 || bt != 0 || cc != 0 || rt != 0.0 || pl != 0) {
        return 0;
    }
    return obj;
}

/* Bug 4: Pending Throwable preserves custom ObjRef, fields, cause, identity, and survives GC! */
void Java_test_Jni318_throwCustomAndSurviveGc(JNIEnv *e, jclass c) {
    /* First verify ThrowNew populates Throwable.getMessage() AND ExceptionOccurred returns same ObjRef! */
    jclass iae_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/IllegalArgumentException");
    JCALL(e, 14, jint(*)(JNIEnv*, jclass, const char*))(e, iae_cls, "msg-from-thrownew");
    jthrowable t1 = JCALL(e, 15, jthrowable(*)(JNIEnv*))(e);
    jthrowable t2 = JCALL(e, 15, jthrowable(*)(JNIEnv*))(e);
    jboolean same_tn = JCALL(e, 24, jboolean(*)(JNIEnv*, jobject, jobject))(e, t1, t2);
    JCALL(e, 17, void(*)(JNIEnv*))(e);
    if (!t1 || !same_tn) {
        JCALL(e, 14, jint(*)(JNIEnv*, jclass, const char*))(e, iae_cls, "ERR_THROWNEW_IDENTITY");
        return;
    }
    jclass thr_cls = JCALL(e, 6, jclass(*)(JNIEnv*, const char*))(e, "java/lang/Throwable");
    void *get_msg = JCALL(e, 33, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, thr_cls, "getMessage", "()Ljava/lang/String;");
    jstring tn_msg = (jstring)JCALL(e, 36, jobject(*)(JNIEnv*, jobject, void*, const jvalue*))(
        e, t1, get_msg, 0);
    if (!tn_msg || JCALL(e, 164, jsize(*)(JNIEnv*, jstring))(e, tn_msg) != 17) {
        JCALL(e, 14, jint(*)(JNIEnv*, jclass, const char*))(e, iae_cls, "ERR_THROWNEW_GETMESSAGE");
        return;
    }
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, tn_msg);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, t1);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, t2);

    /* Invoke Java helper that throws a CustomJniException(errorCode=777, cause=ioEx, payload="live-payload"). */
    void *java_thrower = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, c, "throwCustomFromJava", "()V");
    JCALL(e, 143, void(*)(JNIEnv*, jclass, void*, const jvalue*))(e, c, java_thrower, 0);

    jthrowable e1 = JCALL(e, 15, jthrowable(*)(JNIEnv*))(e);
    jthrowable e2 = JCALL(e, 15, jthrowable(*)(JNIEnv*))(e);
    if (!e1 || !JCALL(e, 24, jboolean(*)(JNIEnv*, jobject, jobject))(e, e1, e2)) {
        JCALL(e, 17, void(*)(JNIEnv*))(e);
        JCALL(e, 14, jint(*)(JNIEnv*, jclass, const char*))(e, iae_cls, "ERR_JAVA_CALLBACK_IDENTITY");
        return;
    }

    /* Clear, re-throw via env->Throw(e1), then DELETE ALL LOCAL REFS to e1 & e2 so
       refs.pending.obj is the SOLE root in the entire VM, then trigger full GC! */
    JCALL(e, 17, void(*)(JNIEnv*))(e);
    if (JCALL(e, 13, jint(*)(JNIEnv*, jthrowable))(e, e1) != 0) {
        JCALL(e, 14, jint(*)(JNIEnv*, jclass, const char*))(e, iae_cls, "ERR_THROW_RET");
        return;
    }
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, e1);
    JCALL(e, 23, void(*)(JNIEnv*, jobject))(e, e2);

    void *churn = JCALL(e, 113, void*(*)(JNIEnv*, jclass, const char*, const char*))(
        e, c, "churnGc", "()V");
    JCALL(e, 143, void(*)(JNIEnv*, jclass, void*, const jvalue*))(e, c, churn, 0);
}
"#;

fn compile_so() -> Vec<u8> {
    let id = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let src_path = dir.join(format!("rd_jni_318_{pid}_{id}.c"));
    let so_path = dir.join(format!("rd_jni_318_{pid}_{id}.so"));
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
        .expect("invoke gcc");
    let _ = std::fs::remove_file(&src_path);
    assert!(status.success(), "gcc failed compiling jni_318 fixture");
    let bytes = std::fs::read(&so_path).expect("read compiled .so");
    let _ = std::fs::remove_file(&so_path);
    bytes
}

fn build_engine() -> Engine {
    let mut b = DexBuilder::new();

    // BaseModel extends Object { int baseCount; long baseStamp; String baseTag; <init>() { super(); } }
    let base_c = b.class(CLS_BASE, "Ljava/lang/Object;");
    b.instance_field(base_c, "baseCount", "I");
    b.instance_field(base_c, "baseStamp", "J");
    b.instance_field(base_c, "baseTag", "Ljava/lang/String;");
    let f_bc = b.field_idx(CLS_BASE, "I", "baseCount");
    let _ = b.field_idx(CLS_BASE, "J", "baseStamp");
    let _ = b.field_idx(CLS_BASE, "Ljava/lang/String;", "baseTag");
    let obj_init_p = b.proto_idx("V", vec![]);
    let obj_init_m = b.method_idx("Ljava/lang/Object;", obj_init_p, "<init>");
    let mut u = op35c(0x70, 1, obj_init_m, [0, 0, 0, 0, 0]);
    u.extend(op10x(0x0e));
    let base_init = b.code(1, 1, 1, u);
    b.direct(base_c, "<init>", "V", vec![], 0x10001, Some(base_init));

    // ChildModel extends BaseModel { int childCode; double ratio; Object payload; <init>() { super(); } }
    let child_c = b.class(CLS_CHILD, CLS_BASE);
    b.instance_field(child_c, "childCode", "I");
    b.instance_field(child_c, "ratio", "D");
    b.instance_field(child_c, "payload", "Ljava/lang/Object;");
    let _ = b.field_idx(CLS_CHILD, "I", "childCode");
    let _ = b.field_idx(CLS_CHILD, "D", "ratio");
    let _ = b.field_idx(CLS_CHILD, "Ljava/lang/Object;", "payload");
    let base_init_m = b.method_idx(CLS_BASE, obj_init_p, "<init>");
    let mut u = op35c(0x70, 1, base_init_m, [0, 0, 0, 0, 0]);
    u.extend(op10x(0x0e));
    let child_init = b.code(1, 1, 1, u);
    b.direct(child_c, "<init>", "V", vec![], 0x10001, Some(child_init));
    // ChildModel.readBaseCount(): iget v0, p0, BaseModel->baseCount; return v0
    let mut u = op22c(0x52, 0, 1, f_bc);
    u.extend(op11x(0x0f, 0));
    let read_bc = b.code(2, 1, 0, u);
    b.r#virtual(child_c, "readBaseCount", "I", vec![], 0x1, Some(read_bc));

    // CustomJniException extends RuntimeException { int errorCode; Object detailPayload; }
    let ex_c = b.class(CLS_EX, "Ljava/lang/RuntimeException;");
    b.instance_field(ex_c, "errorCode", "I");
    b.instance_field(ex_c, "detailPayload", "Ljava/lang/Object;");
    let re_str_p = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
    let re_init_m = b.method_idx("Ljava/lang/RuntimeException;", re_str_p, "<init>");
    // CustomJniException.<init>(String msg, int code, Object payload, Throwable cause)
    // regs=6, ins=5 => p0=v1(this), p1=v2(msg), p2=v3(code), p3=v4(payload), p4=v5(cause)
    let f_code = b.field_idx(CLS_EX, "I", "errorCode");
    let f_payload = b.field_idx(CLS_EX, "Ljava/lang/Object;", "detailPayload");
    let init_cause_p = b.proto_idx(
        "Ljava/lang/Throwable;",
        vec!["Ljava/lang/Throwable;".into()],
    );
    let init_cause_m = b.method_idx("Ljava/lang/Throwable;", init_cause_p, "initCause");
    let mut u = op35c(0x70, 2, re_init_m, [1, 2, 0, 0, 0]); // super(msg)
    u.extend(op22c(0x59, 3, 1, f_code)); // iput v3, v1, errorCode
    u.extend(op22c(0x5b, 4, 1, f_payload)); // iput-object v4, v1, detailPayload
    u.extend(op35c(0x6e, 2, init_cause_m, [1, 5, 0, 0, 0])); // invoke-virtual initCause(v5)
    u.extend(op10x(0x0e));
    let ex_init = b.code(6, 5, 2, u);
    b.direct(
        ex_c,
        "<init>",
        "V",
        vec![
            "Ljava/lang/String;",
            "I",
            "Ljava/lang/Object;",
            "Ljava/lang/Throwable;",
        ],
        0x10001,
        Some(ex_init),
    );

    // Host class Ltest/Jni318;
    let host_c = b.class(CLS_HOST, "Ljava/lang/Object;");
    for (name, ret, params) in [
        ("exerciseObjectArraySafety", "I", vec![]),
        ("setObjectOnIntArray", "V", vec!["[I", "Ljava/lang/Object;"]),
        (
            "getIntRegionOnObjectArray",
            "V",
            vec!["[Ljava/lang/String;"],
        ),
        ("getLongElemsOnIntArray", "V", vec!["[I"]),
        ("exerciseJsizeAndBounds", "I", vec![]),
        ("allocAndVerifyDefaults", CLS_CHILD, vec!["Z"]),
        ("throwCustomAndSurviveGc", "V", vec![]),
    ] {
        b.direct(host_c, name, ret, params, 0x109, None);
    }

    // static void throwCustomFromJava():
    //   v0 = new IllegalStateException("root-cause")
    //   v1 = new StringBuilder(); v1.append("payload-318")
    //   v2 = new CustomJniException("boom-318", 777, v1, v0)
    //   throw v2
    let ise_ty = b.type_idx("Ljava/lang/IllegalStateException;");
    let ise_init = b.method_idx("Ljava/lang/IllegalStateException;", re_str_p, "<init>");
    let sb_ty = b.type_idx("Ljava/lang/StringBuilder;");
    let sb_init = b.method_idx("Ljava/lang/StringBuilder;", obj_init_p, "<init>");
    let sb_app_p = b.proto_idx(
        "Ljava/lang/StringBuilder;",
        vec!["Ljava/lang/String;".into()],
    );
    let sb_app = b.method_idx("Ljava/lang/StringBuilder;", sb_app_p, "append");
    let ex_ty = b.type_idx(CLS_EX);
    let ex_ctor_p = b.proto_idx(
        "V",
        vec![
            "Ljava/lang/String;".into(),
            "I".into(),
            "Ljava/lang/Object;".into(),
            "Ljava/lang/Throwable;".into(),
        ],
    );
    let ex_ctor = b.method_idx(CLS_EX, ex_ctor_p, "<init>");
    let s_cause = b.intern("root-cause") as u16;
    let s_payload = b.intern("payload-318") as u16;
    let s_boom = b.intern("boom-318") as u16;

    let mut u = Vec::new();
    u.extend(op21c(0x22, 0, ise_ty)); // new-instance v0, IllegalStateException
    u.extend(op21c(0x1a, 3, s_cause)); // const-string v3, "root-cause"
    u.extend(op35c(0x70, 2, ise_init, [0, 3, 0, 0, 0]));
    u.extend(op21c(0x22, 1, sb_ty)); // new-instance v1, StringBuilder
    u.extend(op35c(0x70, 1, sb_init, [1, 0, 0, 0, 0]));
    u.extend(op21c(0x1a, 3, s_payload));
    u.extend(op35c(0x6e, 2, sb_app, [1, 3, 0, 0, 0]));
    u.extend(op21c(0x22, 2, ex_ty)); // new-instance v2, CustomJniException
    u.extend(op21c(0x1a, 3, s_boom)); // v3 = "boom-318"
    u.extend(op21s(0x13, 4, 777)); // v4 = 777
    u.extend(op35c(0x70, 5, ex_ctor, [2, 3, 4, 1, 0])); // <init>(v2, v3, v4, v1, v0)
    u.extend(op11x(0x27, 2)); // throw v2
    let thrower_code = b.code(5, 0, 5, u);
    b.direct(
        host_c,
        "throwCustomFromJava",
        "V",
        vec![],
        0x9,
        Some(thrower_code),
    );

    // static void churnGc(): allocate 32 x 8192 `[B` arrays (~6.2 MB > 2 MB budget) to force GC.
    let byte_arr_ty = b.type_idx("[B");
    let mut u = Vec::new();
    u.extend(op21s(0x13, 1, 32)); // v1 = 32
    u.extend(op31t(0x14, 2, 8_192)); // v2 = 8192
    let loop_pc = u.len();
    u.extend(op22c(0x23, 0, 2, byte_arr_ty)); // new-array v0, v2, [B
    u.extend(op22b(0xd8, 1, 1, -1)); // add-int/lit8 v1, v1, -1
    let branch_pc = u.len();
    let rel = (loop_pc as isize - branch_pc as isize) as i16;
    u.extend(op21s(0x3b, 1, rel)); // if-gtz v1, loop_pc
    u.extend(op11n(0x12, 0, 0)); // const/4 v0, 0
    u.extend(op10x(0x0e)); // return-void
    let churn_code = b.code(3, 0, 0, u);
    b.direct(host_c, "churnGc", "V", vec![], 0x9, Some(churn_code));

    let dex = b.finish();
    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", build_manifest_axml("test", CLS_HOST));
    z.add("classes.dex", dex.clone());
    z.add("lib/x86_64/libjni318.so", compile_so());
    let apk = rd_apk::Apk::from_bytes(z.build()).expect("build apk");
    let cfg = VmConfig {
        heap_budget: 2 * 1024 * 1024,
        ..VmConfig::default()
    };
    let mut vm = Engine::new(vec![rd_dex::Dex::parse(dex).expect("parse dex")], cfg);
    vm.set_resources(apk);
    rd_vm::native::load(&mut vm, "jni318").expect("load libjni318.so");
    vm
}

#[test]
fn jni_array_type_safety_and_nested_descriptors() {
    let mut vm = build_engine();
    assert_eq!(
        vm.invoke_static(CLS_HOST, "exerciseObjectArraySafety", "()I", &[])
            .expect("exerciseObjectArraySafety"),
        Value::Int(100),
    );

    // Primitive array passed to SetObjectArrayElement must fail with JNI_ERROR.
    let int_arr = vm
        .heap
        .alloc_array(ElemKind::Int, 2, "[I")
        .expect("alloc [I");
    vm.heap
        .array_elems_mut(int_arr)
        .expect("elems")
        .resize(2, Value::Int(7));
    let s = vm.heap.alloc_string("bad".into()).expect("alloc str");
    let err1 = vm.invoke_static(
        CLS_HOST,
        "setObjectOnIntArray",
        "([ILjava/lang/Object;)V",
        &[Value::Obj(int_arr), Value::Obj(s)],
    );
    assert!(
        matches!(err1, Err(VmExit::Error(ref e)) if e.code == "JNI_ERROR"),
        "SetObjectArrayElement on [I must return JNI_ERROR, got {err1:?}"
    );

    // Object array passed to GetIntArrayRegion must fail with JNI_ERROR.
    let str_arr = vm
        .heap
        .alloc_array(ElemKind::Obj, 1, "[Ljava/lang/String;")
        .expect("alloc [String");
    vm.heap
        .array_elems_mut(str_arr)
        .expect("elems")
        .resize(1, Value::Obj(s));
    let err2 = vm.invoke_static(
        CLS_HOST,
        "getIntRegionOnObjectArray",
        "([Ljava/lang/String;)V",
        &[Value::Obj(str_arr)],
    );
    assert!(
        matches!(err2, Err(VmExit::Error(ref e)) if e.code == "JNI_ERROR"),
        "GetIntArrayRegion on [Ljava/lang/String; must return JNI_ERROR, got {err2:?}"
    );

    // Mismatched primitive array (int[] passed to GetLongArrayElements) must fail with JNI_ERROR.
    let err3 = vm.invoke_static(
        CLS_HOST,
        "getLongElemsOnIntArray",
        "([I)V",
        &[Value::Obj(int_arr)],
    );
    assert!(
        matches!(err3, Err(VmExit::Error(ref e)) if e.code == "JNI_ERROR"),
        "GetLongArrayElements on [I must return JNI_ERROR, got {err3:?}"
    );
}

#[test]
fn jni_64bit_jsize_truncation_and_bounds_exceptions() {
    let mut vm = build_engine();
    assert_eq!(
        vm.invoke_static(CLS_HOST, "exerciseJsizeAndBounds", "()I", &[])
            .expect("exerciseJsizeAndBounds"),
        Value::Int(200),
    );
}

#[test]
fn jni_alloc_object_and_new_object_default_fields() {
    let mut vm = build_engine();
    for use_new in [0, 1] {
        let res = vm.invoke_static(
            CLS_HOST,
            "allocAndVerifyDefaults",
            "(Z)Ltest/ChildModel;",
            &[Value::Int(use_new)],
        );
        let Ok(Value::Obj(r)) = res else {
            panic!("expected non-null ChildModel for use_new={use_new}, got {res:?}");
        };
        assert_ne!(r, 0, "ChildModel ref must be non-null (use_new={use_new})");
        assert_eq!(vm.heap.get_field(r, "baseCount"), Ok(Value::Int(0)));
        assert_eq!(vm.heap.get_field(r, "baseStamp"), Ok(Value::Long(0)));
        assert_eq!(vm.heap.get_field(r, "baseTag"), Ok(Value::Null));
        assert_eq!(vm.heap.get_field(r, "childCode"), Ok(Value::Int(0)));
        assert_eq!(vm.heap.get_field(r, "ratio"), Ok(Value::Double(0.0)));
        assert_eq!(vm.heap.get_field(r, "payload"), Ok(Value::Null));
        assert_eq!(
            vm.invoke_instance(r, "readBaseCount", "()I", &[])
                .expect("readBaseCount"),
            Value::Int(0)
        );
    }
}

#[test]
fn jni_pending_throwable_identity_fields_cause_and_gc_root() {
    let mut vm = build_engine();
    let gc_before = vm.gc_stats().collections;
    let exit = vm.invoke_static(CLS_HOST, "throwCustomAndSurviveGc", "()V", &[]);
    let gc_after = vm.gc_stats().collections;
    assert!(
        gc_after > gc_before,
        "expected churnGc() to run at least one GC while pending Throwable was the sole root"
    );

    let Err(VmExit::Exception(t)) = exit else {
        panic!("expected VmExit::Exception(CustomJniException), got {exit:?}");
    };
    assert_eq!(t.class, CLS_EX);
    assert_eq!(t.message.as_deref(), Some("boom-318"));

    let ex_ref = t
        .obj
        .expect("pending Throwable ObjRef must be preserved on VmExit::Exception");
    assert_ne!(vm.heap.class_of(ex_ref), Ok(FREED_CLASS));
    assert_eq!(vm.heap.class_of(ex_ref), Ok(CLS_EX));
    assert_eq!(vm.heap.get_field(ex_ref, "errorCode"), Ok(Value::Int(777)));

    // Transitive custom field (`detailPayload` StringBuilder) must also survive GC.
    let Ok(Value::Obj(payload_ref)) = vm.heap.get_field(ex_ref, "detailPayload") else {
        panic!("expected detailPayload ObjRef on CustomJniException");
    };
    assert_ne!(vm.heap.class_of(payload_ref), Ok(FREED_CLASS));
    let Ok(Value::Obj(payload_str)) =
        vm.invoke_instance(payload_ref, "toString", "()Ljava/lang/String;", &[])
    else {
        panic!("expected StringBuilder.toString()");
    };
    assert_eq!(vm.heap.as_str(payload_str), Ok("payload-318"));

    // Transitive `cause` (`IllegalStateException("root-cause")`) must also survive GC.
    let Ok(Value::Obj(cause_ref)) =
        vm.invoke_instance(ex_ref, "getCause", "()Ljava/lang/Throwable;", &[])
    else {
        panic!("expected getCause() to return IllegalStateException");
    };
    assert_eq!(
        vm.heap.class_of(cause_ref),
        Ok("Ljava/lang/IllegalStateException;")
    );
    let Ok(Value::Obj(cause_msg)) =
        vm.invoke_instance(cause_ref, "getMessage", "()Ljava/lang/String;", &[])
    else {
        panic!("expected cause.getMessage()");
    };
    assert_eq!(vm.heap.as_str(cause_msg), Ok("root-cause"));
}
