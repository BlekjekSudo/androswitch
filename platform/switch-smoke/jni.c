/* Real JNI ABI table. Every unimplemented function has a named failure trap.
 * AndroSwitch contributors, MIT OR Apache-2.0; jni.h is AOSP Apache-2.0. */
#include "../../third_party/android/jni.h"
#include "native.h"
#include "jni_arguments.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>

extern uint64_t as_jni_dispatch(uint32_t, uint64_t, uint64_t, uint64_t,
                                const uint8_t *, size_t);
extern void as_native_trap(int code);
static struct JNINativeInterface table;
static struct JNIInvokeInterface vm_table;
static const struct JNIInvokeInterface *vm = &vm_table;
static __thread const struct JNINativeInterface *environment;
static __thread int attached;
static Mutex vm_mutex;
static __thread unsigned lease_depth;
static int initialized;
/* Called only after all guest callbacks/threads are quiescent. JNI tables own
 * executable import veneers, so they cannot survive native mapping teardown. */
void as_jni_reset(void) {
  initialized = 0;
  environment = NULL;
  attached = 0;
  memset(&table, 0, sizeof(table));
  memset(&vm_table, 0, sizeof(vm_table));
}
enum {
  FIND_CLASS = 1,
  METHOD = 2,
  REGISTER = 3,
  INVOKE = 4,
  OBJECT_CLASS = 5,
  NEW_UTF = 6,
  UTF_LENGTH = 7,
  UTF_COPY = 8,
  ARRAY_LENGTH = 9,
  NEW_ARRAY = 10,
  ARRAY_GET = 11,
  ARRAY_SET = 12,
  NEW_LOCAL = 13,
  NEW_GLOBAL = 14,
  DELETE_LOCAL = 15,
  DELETE_GLOBAL = 16,
  SAME = 17,
  PUSH = 18,
  POP = 19,
  EXCEPTION_CHECK = 20,
  EXCEPTION_CLEAR = 21,
  THROW_NEW = 22,
  EXCEPTION_OBJECT = 23,
  THROW_OBJECT = 24,
  METHOD_TYPES = 25,
  ATTACH = 26,
  DETACH = 27,
  NEW_UTF16 = 28,
  UTF16_LENGTH = 29,
  UTF16_COPY = 30,
  REFERENCE_KIND = 31,
  ALLOC_OBJECT = 32,
  INSTANCE_OF = 33,
  FIELD = 34,
  GET_FIELD = 35,
  SET_FIELD = 36,
  UNREGISTER = 37,
  CONSTRUCT = 38,
  EXCEPTION_DESCRIBE = 39
};
static uint64_t dispatch(uint32_t op, uint64_t a, uint64_t b, uint64_t c,
                         const void *data, size_t length) {
  if (!attached && op != ATTACH)
    as_native_trap(-1004);
  as_jni_vm_lock();
  uint64_t result = as_jni_dispatch(op, a, b, c, data, length);
  as_jni_vm_unlock();
  if (as_native_last_error())
    as_native_trap(as_native_last_error());
  return result;
}
static void as_jni_missing(int slot, const char *name) {
  as_jni_dispatch(UINT32_MAX, (uint64_t)slot, 0, 0, (const uint8_t *)name,
                  strlen(name));
  as_native_trap(-1002);
}
#include "jni_traps.inc"
uint64_t as_jni_thread(void) { return (uint64_t)threadGetCurHandle(); }
void as_jni_vm_lock(void) {
  if (!lease_depth++)
    mutexLock(&vm_mutex);
}
void as_jni_vm_unlock(void) {
  if (lease_depth && !--lease_depth)
    mutexUnlock(&vm_mutex);
}
unsigned as_jni_vm_release(void) {
  unsigned depth = lease_depth;
  if (depth) {
    lease_depth = 0;
    mutexUnlock(&vm_mutex);
  }
  return depth;
}
void as_jni_vm_acquire(unsigned depth) {
  if (depth) {
    mutexLock(&vm_mutex);
    lease_depth = depth;
  }
}
static jint version(JNIEnv *env) {
  (void)env;
  return JNI_VERSION_1_6;
}
static jclass find_class(JNIEnv *env, const char *name) {
  (void)env;
  return (jclass)dispatch(FIND_CLASS, 0, 0, 0, name, strlen(name));
}
static jclass object_class(JNIEnv *env, jobject object) {
  (void)env;
  return (jclass)dispatch(OBJECT_CLASS, (uint64_t)object, 0, 0, NULL, 0);
}
static jmethodID method(JNIEnv *env, jclass klass, const char *name,
                        const char *sig) {
  (void)env;
  return (jmethodID)dispatch(METHOD, (uint64_t)klass, (uint64_t)name,
                             (uint64_t)sig, NULL, 0);
}
static jmethodID static_method(JNIEnv *env, jclass klass, const char *name,
                               const char *sig) {
  (void)env;
  return (jmethodID)dispatch(METHOD, (uint64_t)klass, (uint64_t)name,
                             (uint64_t)sig, NULL, 1);
}
static jint register_natives(JNIEnv *env, jclass klass,
                             const JNINativeMethod *methods, jint count) {
  (void)env;
  if (count < 0 || count > 1024)
    return JNI_ERR;
  return (jint)dispatch(REGISTER, (uint64_t)klass, 0, 0, methods,
                        (size_t)count);
}
static jint unregister_natives(JNIEnv *env, jclass klass) {
  (void)env;
  return (jint)dispatch(UNREGISTER, (uint64_t)klass, 0, 0, NULL, 0);
}
static jobject local(JNIEnv *env, jobject object) {
  (void)env;
  return (jobject)dispatch(NEW_LOCAL, (uint64_t)object, 0, 0, NULL, 0);
}
static jobject global(JNIEnv *env, jobject object) {
  (void)env;
  return (jobject)dispatch(NEW_GLOBAL, (uint64_t)object, 0, 0, NULL, 0);
}
static void delete_local(JNIEnv *env, jobject object) {
  (void)env;
  dispatch(DELETE_LOCAL, (uint64_t)object, 0, 0, NULL, 0);
}
static void delete_global(JNIEnv *env, jobject object) {
  (void)env;
  dispatch(DELETE_GLOBAL, (uint64_t)object, 0, 0, NULL, 0);
}
static jboolean same(JNIEnv *env, jobject a, jobject b) {
  (void)env;
  return dispatch(SAME, (uint64_t)a, (uint64_t)b, 0, NULL, 0);
}
static jint push(JNIEnv *env, jint capacity) {
  (void)env;
  if (capacity < 0 || capacity > 65536)
    return JNI_ERR;
  dispatch(PUSH, 0, 0, 0, NULL, 0);
  return JNI_OK;
}
static jobject pop(JNIEnv *env, jobject result) {
  (void)env;
  return (jobject)dispatch(POP, (uint64_t)result, 0, 0, NULL, 0);
}
static jint capacity(JNIEnv *env, jint count) {
  (void)env;
  return count >= 0 && count <= 65536 ? JNI_OK : JNI_ERR;
}
static jboolean exception_check(JNIEnv *env) {
  (void)env;
  return dispatch(EXCEPTION_CHECK, 0, 0, 0, NULL, 0);
}
static void exception_clear(JNIEnv *env) {
  (void)env;
  dispatch(EXCEPTION_CLEAR, 0, 0, 0, NULL, 0);
}
static void exception_describe(JNIEnv *env) {
  (void)env;
  dispatch(EXCEPTION_DESCRIBE, 0, 0, 0, NULL, 0);
}
static jthrowable exception_object(JNIEnv *env) {
  (void)env;
  return (jthrowable)dispatch(EXCEPTION_OBJECT, 0, 0, 0, NULL, 0);
}
static jint throw_new(JNIEnv *env, jclass klass, const char *message) {
  (void)env;
  return (jint)dispatch(THROW_NEW, (uint64_t)klass, 0, 0, message,
                        message ? strlen(message) : 0);
}
static jint throw_object(JNIEnv *env, jthrowable object) {
  (void)env;
  return (jint)dispatch(THROW_OBJECT, (uint64_t)object, 0, 0, NULL, 0);
}
static jstring new_utf(JNIEnv *env, const char *text) {
  (void)env;
  return text ? (jstring)dispatch(NEW_UTF, 0, 0, 0, text, strlen(text)) : NULL;
}
static jsize utf_length(JNIEnv *env, jstring text) {
  (void)env;
  return (jsize)dispatch(UTF_LENGTH, (uint64_t)text, 0, 0, NULL, 0);
}
static const char *utf_chars(JNIEnv *env, jstring text, jboolean *copy) {
  (void)env;
  size_t length = (size_t)utf_length(env, text);
  char *bytes = malloc(length + 1);
  if (!bytes)
    as_native_trap(-12);
  dispatch(UTF_COPY, (uint64_t)text, 0, 0, bytes, length);
  bytes[length] = 0;
  if (copy)
    *copy = JNI_TRUE;
  return bytes;
}
static void utf_release(JNIEnv *env, jstring text, const char *bytes) {
  (void)env;
  (void)text;
  free((void *)bytes);
}
static jstring new_utf16(JNIEnv *env, const jchar *text, jsize length) {
  (void)env;
  if (length < 0)
    as_native_trap(-22);
  return (jstring)dispatch(NEW_UTF16, 0, 0, 0, text, (size_t)length);
}
static jsize utf16_length(JNIEnv *env, jstring text) {
  (void)env;
  return (jsize)dispatch(UTF16_LENGTH, (uint64_t)text, 0, 0, NULL, 0);
}
static const jchar *utf16_chars(JNIEnv *env, jstring text, jboolean *copy) {
  (void)env;
  size_t length = (size_t)utf16_length(env, text);
  jchar *bytes = malloc((length + 1) * 2);
  if (!bytes)
    as_native_trap(-12);
  dispatch(UTF16_COPY, (uint64_t)text, 0, 0, bytes, length);
  bytes[length] = 0;
  if (copy)
    *copy = JNI_TRUE;
  return bytes;
}
static void utf16_release(JNIEnv *env, jstring text, const jchar *bytes) {
  (void)env;
  (void)text;
  free((void *)bytes);
}
static jsize array_length(JNIEnv *env, jarray array) {
  (void)env;
  return (jsize)dispatch(ARRAY_LENGTH, (uint64_t)array, 0, 0, NULL, 0);
}
static jobjectRefType ref_kind(JNIEnv *env, jobject object) {
  (void)env;
  return object ? (jobjectRefType)dispatch(REFERENCE_KIND, (uint64_t)object, 0,
                                           0, NULL, 0)
                : JNIInvalidRefType;
}
static jobject alloc_object(JNIEnv *env, jclass klass) {
  (void)env;
  return (jobject)dispatch(ALLOC_OBJECT, (uint64_t)klass, 0, 0, NULL, 0);
}
static jboolean instance_of(JNIEnv *env, jobject object, jclass klass) {
  (void)env;
  return (jboolean)dispatch(INSTANCE_OF, (uint64_t)object, (uint64_t)klass, 0,
                            NULL, 0);
}

#include "jni_static_fields.inc"

static size_t argument_types(jmethodID method, char *types) {
  size_t length =
      (size_t)dispatch(METHOD_TYPES, (uint64_t)method, 0, 0, types, 128);
  if (length > 128)
    as_native_trap(-22);
  return length;
}
static uint64_t invoke_a(jobject receiver, jmethodID method,
                         const jvalue *values, int is_static) {
  char types[128] = {0};
  size_t count = argument_types(method, types);
  uint64_t bits[128] = {0};
  as_jni_arguments_a(types, count, values, bits);
  return dispatch(INVOKE, (uint64_t)receiver, (uint64_t)method,
                  (uint64_t)is_static, bits, count);
}
static uint64_t invoke_v(jobject receiver, jmethodID method, va_list values,
                         int is_static) {
  char types[128] = {0};
  size_t count = argument_types(method, types);
  uint64_t bits[128] = {0};
  as_jni_arguments_v(types, count, values, bits);
  return dispatch(INVOKE, (uint64_t)receiver, (uint64_t)method,
                  (uint64_t)is_static, bits, count);
}
#include "jni_construct.inc"
#define METHODS(Name, Type, Member)                                            \
  static Type call_##Name##_a(JNIEnv *e, jobject o, jmethodID m,               \
                              const jvalue *a) {                               \
    (void)e;                                                                   \
    uint64_t bits = invoke_a(o, m, a, 0);                                      \
    jvalue v = {0};                                                            \
    memcpy(&v, &bits, 8);                                                      \
    return v.Member;                                                           \
  }                                                                            \
  static Type call_##Name##_v(JNIEnv *e, jobject o, jmethodID m, va_list a) {  \
    (void)e;                                                                   \
    uint64_t bits = invoke_v(o, m, a, 0);                                      \
    jvalue v = {0};                                                            \
    memcpy(&v, &bits, 8);                                                      \
    return v.Member;                                                           \
  }                                                                            \
  static Type call_##Name(JNIEnv *e, jobject o, jmethodID m, ...) {            \
    va_list a;                                                                 \
    va_start(a, m);                                                            \
    Type v = call_##Name##_v(e, o, m, a);                                      \
    va_end(a);                                                                 \
    return v;                                                                  \
  }                                                                            \
  static Type static_##Name##_a(JNIEnv *e, jclass o, jmethodID m,              \
                                const jvalue *a) {                             \
    (void)e;                                                                   \
    uint64_t bits = invoke_a(o, m, a, 1);                                      \
    jvalue v = {0};                                                            \
    memcpy(&v, &bits, 8);                                                      \
    return v.Member;                                                           \
  }                                                                            \
  static Type static_##Name##_v(JNIEnv *e, jclass o, jmethodID m, va_list a) { \
    (void)e;                                                                   \
    uint64_t bits = invoke_v(o, m, a, 1);                                      \
    jvalue v = {0};                                                            \
    memcpy(&v, &bits, 8);                                                      \
    return v.Member;                                                           \
  }                                                                            \
  static Type static_##Name(JNIEnv *e, jclass o, jmethodID m, ...) {           \
    va_list a;                                                                 \
    va_start(a, m);                                                            \
    Type v = static_##Name##_v(e, o, m, a);                                    \
    va_end(a);                                                                 \
    return v;                                                                  \
  }
METHODS(Object, jobject, l)
METHODS(Boolean, jboolean, z)
METHODS(Byte, jbyte, b)
METHODS(Char, jchar, c)
METHODS(Short, jshort, s)
METHODS(Int, jint, i)
METHODS(Long, jlong, j)
METHODS(Float, jfloat, f)
METHODS(Double, jdouble, d)
static void call_Void_a(JNIEnv *e, jobject o, jmethodID m, const jvalue *a) {
  (void)e;
  invoke_a(o, m, a, 0);
}
static void call_Void_v(JNIEnv *e, jobject o, jmethodID m, va_list a) {
  (void)e;
  invoke_v(o, m, a, 0);
}
static void call_Void(JNIEnv *e, jobject o, jmethodID m, ...) {
  va_list a;
  va_start(a, m);
  call_Void_v(e, o, m, a);
  va_end(a);
}
static void static_Void_a(JNIEnv *e, jclass o, jmethodID m, const jvalue *a) {
  (void)e;
  invoke_a(o, m, a, 1);
}
static void static_Void_v(JNIEnv *e, jclass o, jmethodID m, va_list a) {
  (void)e;
  invoke_v(o, m, a, 1);
}
static void static_Void(JNIEnv *e, jclass o, jmethodID m, ...) {
  va_list a;
  va_start(a, m);
  static_Void_v(e, o, m, a);
  va_end(a);
}

#define ARRAYS(Name, Type, Kind)                                               \
  static Type##Array new_##Name(JNIEnv *e, jsize n) {                          \
    (void)e;                                                                   \
    if (n < 0)                                                                 \
      as_native_trap(-22);                                                     \
    return (Type##Array)dispatch(NEW_ARRAY, Kind, (uint64_t)n, 0, NULL, 0);    \
  }                                                                            \
  static void get_##Name(JNIEnv *e, Type##Array a, jsize start, jsize n,       \
                         Type *out) {                                          \
    (void)e;                                                                   \
    dispatch(ARRAY_GET, (uint64_t)a, (uint64_t)start, Kind, out, (size_t)n);   \
  }                                                                            \
  static void set_##Name(JNIEnv *e, Type##Array a, jsize start, jsize n,       \
                         const Type *in) {                                     \
    (void)e;                                                                   \
    dispatch(ARRAY_SET, (uint64_t)a, (uint64_t)start, Kind, in, (size_t)n);    \
  }                                                                            \
  static Type *elements_##Name(JNIEnv *e, Type##Array a, jboolean *copy) {     \
    jsize n = array_length(e, (jarray)a);                                      \
    Type *out = malloc((size_t)n * sizeof(Type) + 1);                          \
    if (!out)                                                                  \
      as_native_trap(-12);                                                     \
    get_##Name(e, a, 0, n, out);                                               \
    if (copy)                                                                  \
      *copy = JNI_TRUE;                                                        \
    return out;                                                                \
  }                                                                            \
  static void release_##Name(JNIEnv *e, Type##Array a, Type *in, jint mode) {  \
    if (mode != JNI_ABORT)                                                     \
      set_##Name(e, a, 0, array_length(e, (jarray)a), in);                     \
    if (mode != JNI_COMMIT)                                                    \
      free(in);                                                                \
  }
ARRAYS(Boolean, jboolean, 'Z')
ARRAYS(Byte, jbyte, 'B')
ARRAYS(Char, jchar, 'C')
ARRAYS(Short, jshort, 'S')
ARRAYS(Int, jint, 'I')
ARRAYS(Long, jlong, 'J')
ARRAYS(Float, jfloat, 'F')
ARRAYS(Double, jdouble, 'D')

static jint get_env(JavaVM *jvm, void **out, jint wanted) {
  (void)jvm;
  if (wanted != JNI_VERSION_1_6 && wanted != JNI_VERSION_1_4 &&
      wanted != JNI_VERSION_1_2)
    return JNI_EVERSION;
  if (!attached) {
    *out = NULL;
    return JNI_EDETACHED;
  }
  *out = &environment;
  return JNI_OK;
}
static jint attach(JavaVM *jvm, JNIEnv **out, void *args) {
  (void)jvm;
  if (!out) return JNI_ERR;
  if (!attached) {
    JavaVMAttachArgs *options=args;
    const char *name=NULL;
    size_t length=0;
    if (options) {
      if (options->version!=JNI_VERSION_1_2 && options->version!=JNI_VERSION_1_4 && options->version!=JNI_VERSION_1_6) {
        *out=NULL;return JNI_EVERSION;
      }
      /* ThreadGroup association has no implementation. Fail before changing
       * attachment state rather than claiming the supplied group was used. */
      if (options->group) { *out=NULL;return JNI_ERR; }
      name=options->name;
      if (name) {
        while (length<=4096 && name[length]) ++length;
        if (length>4096) { *out=NULL;return JNI_ERR; }
      }
    }
    as_jni_vm_lock();
    dispatch(ATTACH, 0, 0, 0, name, length);
    as_jni_vm_unlock();
    environment = &table;
    attached = 1;
  }
  *out = &environment;
  return JNI_OK;
}
static jint detach(JavaVM *jvm) {
  (void)jvm;
  if (!attached)
    return JNI_EDETACHED;
  as_jni_vm_lock();
  dispatch(DETACH, 0, 0, 0, NULL, 0);
  as_jni_vm_unlock();
  attached = 0;
  environment = NULL;
  return JNI_OK;
}
static jint get_vm(JNIEnv *env, JavaVM **out) {
  (void)env;
  *out = &vm;
  return JNI_OK;
}
#define BIND(Name, Function)                                                   \
  do {                                                                         \
    table.Name = (__typeof__(table.Name))(uintptr_t)as_native_veneer(          \
        (uint64_t)(Function));                                                 \
    if (!table.Name)                                                           \
      abort();                                                                 \
  } while (0)
uint64_t as_jni_initialize(void) {
  if (!initialized) {
    /* The pinned ABI has only reserved/data words then function pointers. */
    _Static_assert(sizeof(table) == sizeof(missing_functions),
                   "JNI ABI table size");
    void **slots = (void **)&table;
    for (size_t i = 4; i < sizeof(table) / sizeof(void *); i++) {
      slots[i] =
          (void *)(uintptr_t)as_native_veneer((uint64_t)missing_functions[i]);
      if (!slots[i])
        abort();
    }
    BIND(GetVersion, version);
    BIND(FindClass, find_class);
    BIND(GetObjectClass, object_class);
    BIND(GetMethodID, method);
    BIND(GetStaticMethodID, static_method);
    BIND(RegisterNatives, register_natives);
    BIND(UnregisterNatives, unregister_natives);
    BIND(NewLocalRef, local);
    BIND(NewGlobalRef, global);
    BIND(DeleteLocalRef, delete_local);
    BIND(DeleteGlobalRef, delete_global);
    BIND(IsSameObject, same);
    BIND(PushLocalFrame, push);
    BIND(PopLocalFrame, pop);
    BIND(EnsureLocalCapacity, capacity);
    BIND(ExceptionCheck, exception_check);
    BIND(ExceptionClear, exception_clear);
    BIND(ExceptionDescribe, exception_describe);
    BIND(ExceptionOccurred, exception_object);
    BIND(ThrowNew, throw_new);
    BIND(Throw, throw_object);
    BIND(NewStringUTF, new_utf);
    BIND(GetStringUTFLength, utf_length);
    BIND(GetStringUTFChars, utf_chars);
    BIND(ReleaseStringUTFChars, utf_release);
    BIND(NewString, new_utf16);
    BIND(GetStringLength, utf16_length);
    BIND(GetStringChars, utf16_chars);
    BIND(ReleaseStringChars, utf16_release);
    BIND(GetArrayLength, array_length);
    BIND(GetObjectRefType, ref_kind);
    BIND(AllocObject, alloc_object);
    BIND(NewObject, new_object);
    BIND(NewObjectV, new_object_v);
    BIND(NewObjectA, new_object_a);
    BIND(IsInstanceOf, instance_of);
    BIND(GetStaticFieldID, static_field_id);
#define BIND_STATIC_FIELDS(Name)                                               \
    BIND(GetStatic##Name##Field, get_static_##Name##_field);                     \
    BIND(SetStatic##Name##Field, set_static_##Name##_field)
    BIND_STATIC_FIELDS(Object);
    BIND_STATIC_FIELDS(Boolean);
    BIND_STATIC_FIELDS(Byte);
    BIND_STATIC_FIELDS(Char);
    BIND_STATIC_FIELDS(Short);
    BIND_STATIC_FIELDS(Int);
    BIND_STATIC_FIELDS(Long);
    BIND_STATIC_FIELDS(Float);
    BIND_STATIC_FIELDS(Double);
#undef BIND_STATIC_FIELDS
    BIND(GetJavaVM, get_vm);
#define BIND_METHODS(Name)                                                     \
  BIND(Call##Name##Method, call_##Name);                                       \
  BIND(Call##Name##MethodV, call_##Name##_v);                                  \
  BIND(Call##Name##MethodA, call_##Name##_a);                                  \
  BIND(CallStatic##Name##Method, static_##Name);                               \
  BIND(CallStatic##Name##MethodV, static_##Name##_v);                          \
  BIND(CallStatic##Name##MethodA, static_##Name##_a)
    BIND_METHODS(Object);
    BIND_METHODS(Boolean);
    BIND_METHODS(Byte);
    BIND_METHODS(Char);
    BIND_METHODS(Short);
    BIND_METHODS(Int);
    BIND_METHODS(Long);
    BIND_METHODS(Float);
    BIND_METHODS(Double);
    BIND_METHODS(Void);
#define BIND_ARRAYS(Name)                                                      \
  BIND(New##Name##Array, new_##Name);                                          \
  BIND(Get##Name##ArrayRegion, get_##Name);                                    \
  BIND(Set##Name##ArrayRegion, set_##Name);                                    \
  BIND(Get##Name##ArrayElements, elements_##Name);                             \
  BIND(Release##Name##ArrayElements, release_##Name)
    BIND_ARRAYS(Boolean);
    BIND_ARRAYS(Byte);
    BIND_ARRAYS(Char);
    BIND_ARRAYS(Short);
    BIND_ARRAYS(Int);
    BIND_ARRAYS(Long);
    BIND_ARRAYS(Float);
    BIND_ARRAYS(Double);
    vm_table.GetEnv = (__typeof__(vm_table.GetEnv))(uintptr_t)as_native_veneer(
        (uint64_t)get_env);
    vm_table.AttachCurrentThread =
        (__typeof__(vm_table.AttachCurrentThread))(uintptr_t)as_native_veneer(
            (uint64_t)attach);
    vm_table.AttachCurrentThreadAsDaemon = vm_table.AttachCurrentThread;
    vm_table.DetachCurrentThread =
        (__typeof__(vm_table.DetachCurrentThread))(uintptr_t)as_native_veneer(
            (uint64_t)detach);
    initialized = 1;
  }
  environment = &table;
  attached = 1;
  return (uint64_t)&environment;
}
uint64_t as_jni_java_vm(void) { return (uint64_t)&vm; }
uint64_t as_jni_environment(void) {
  return attached ? (uint64_t)&environment : 0;
}
