/* Original AndroSwitch SDK/NDK contract probes. MIT OR Apache-2.0. */
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <errno.h>
#include <jni.h>
#include <locale.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

extern jstring as_probe_graphics(JNIEnv *, int);
extern jstring as_probe_vulkan(JNIEnv *, int);
static JavaVM *machine;
static jstring text(JNIEnv *e, const char *s) {
  return (*e)->NewStringUTF(e, s);
}
static jint reenter(JNIEnv *e, jclass c, jint v) {
  jmethodID m = (*e)->GetStaticMethodID(e, c, "reenter", "(I)I");
  if (!m) return 0;
  return (*e)->CallStaticIntMethod(e, c, m, v);
}
struct work {
  jclass cls;
  jint value;
};
static void *worker(void *arg) {
  struct work *w = arg;
  JNIEnv *e = NULL;
  if ((*machine)->AttachCurrentThread(machine, &e, NULL) != JNI_OK) return NULL;
  w->value = reenter(e, w->cls, 3);
  if ((*e)->ExceptionCheck(e)) w->value = -1;
  (*machine)->DetachCurrentThread(machine);
  return NULL;
}
static jlong integer(JNIEnv *e, jclass c, jint mode) {
  if (mode == 0) return sizeof(void *);
  if (mode == 1) {
    if ((*e)->PushLocalFrame(e, 8) != 0) return 0;
    jstring s = text(e, "local");
    jobject retained = (*e)->PopLocalFrame(e, s);
    return retained && (*e)->GetStringLength(e, retained) == 5;
  }
  struct work *w = calloc(1, sizeof(*w));
  if (!w) return 0;
  w->cls = (*e)->NewGlobalRef(e, c);
  if (!w->cls) {
    free(w);
    return 0;
  }
  pthread_t thread;
  int rc = pthread_create(&thread, NULL, worker, w);
  if (!rc) {
    rc = pthread_join(thread, NULL);
    /* Unknown completion retains the argument/global ref until the owning
     * runtime proves guest-thread quiescence. Never return a stack pointer. */
    if (rc) return -rc;
  }
  jint value = w->value;
  (*e)->DeleteGlobalRef(e, w->cls);
  free(w);
  return rc ? -rc : value;
}
static jdouble mixed(JNIEnv *e, jclass c, jdouble a, jfloat b, jint i,
                     jlong l) {
  (void)e;
  (void)c;
  return a + b + i + l;
}
static jstring echo(JNIEnv *e, jclass c, jstring input) {
  (void)c;
  const char *bytes = (*e)->GetStringUTFChars(e, input, NULL);
  if (!bytes) return NULL;
  jstring result = text(e, bytes);
  (*e)->ReleaseStringUTFChars(e, input, bytes);
  return result;
}
static jintArray array(JNIEnv *e, jclass c, jintArray input) {
  (void)c;
  jsize n = (*e)->GetArrayLength(e, input);
  jint *values = (*e)->GetIntArrayElements(e, input, NULL);
  if (!values) return NULL;
  for (jsize i = 0; i < n; i++) values[i] *= 2;
  (*e)->ReleaseIntArrayElements(e, input, values, 0);
  return input;
}
static void raise_java(JNIEnv *e, jclass c) {
  (void)c;
  jclass type = (*e)->FindClass(e, "java/lang/IllegalStateException");
  if (type) (*e)->ThrowNew(e, type, "native-exception");
}
static jint callback(JNIEnv *e, jclass c, jint value) {
  return reenter(e, c, value);
}
static jstring shared_file(JNIEnv *e, jclass c, jstring path) {
  (void)c;
  const char *p = (*e)->GetStringUTFChars(e, path, NULL);
  if (!p) return NULL;
  FILE *f = fopen(p, "rb");
  int value = f ? fgetc(f) : -1;
  if (f) fclose(f);
  if (value != 47) {
    (*e)->ReleaseStringUTFChars(e, path, p);
    return text(e, "FAIL:Java write not visible to native");
  }
  f = fopen(p, "wb");
  int ok = f && fputc(83, f) != EOF;
  if (f && fclose(f)) ok = 0;
  (*e)->ReleaseStringUTFChars(e, path, p);
  return text(e, ok ? "Java/native file shared" : "FAIL:native file write");
}
static jstring facts(JNIEnv *e, jclass c, jint mode, jstring missing_path) {
  (void)c;
  char out[256];
  struct timespec a, b;
  if (mode == 0) {
    if (clock_gettime(CLOCK_MONOTONIC, &a) ||
        clock_gettime(CLOCK_MONOTONIC, &b))
      return text(e, "FAIL:clock_gettime");
    if (b.tv_sec < a.tv_sec || (b.tv_sec == a.tv_sec && b.tv_nsec < a.tv_nsec))
      return text(e, "FAIL:clock moved backwards");
    return text(e, "monotonic");
  }
  if (mode == 1) {
    if (clock_getres(CLOCK_MONOTONIC, &a)) return text(e, "FAIL:clock_getres");
    snprintf(out, sizeof(out), "resolution=%lld:%ld", (long long)a.tv_sec,
             a.tv_nsec);
    return text(e, out);
  }
  if (mode == 2) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    if (n < 1) return text(e, "FAIL:sysconf CPU count");
    snprintf(out, sizeof(out), "nativeOnlineProcessors=%ld", n);
    return text(e, out);
  }
  if (mode == 3) {
    const char *path = (*e)->GetStringUTFChars(e, missing_path, NULL);
    if (!path) return text(e, "FAIL:missing-file path");
    errno = 0;
    FILE *f = fopen(path, "rb");
    int saved_errno = errno;
    (*e)->ReleaseStringUTFChars(e, missing_path, path);
    if (f) {
      fclose(f);
      return text(e, "FAIL:unexpected file");
    }
    return text(e, saved_errno == ENOENT ? "ENOENT" : "FAIL:wrong errno");
  }
  if (mode == 4) {
    pthread_key_t key;
    if (pthread_key_create(&key, NULL)) return text(e, "FAIL:TLS key");
    int value = 47;
    int rc = pthread_setspecific(key, &value);
    void *actual = pthread_getspecific(key);
    pthread_key_delete(key);
    return text(e,
                !rc && actual == &value ? "TLS roundtrip" : "FAIL:TLS value");
  }
  if (mode == 5) {
    unsigned char *p = malloc(32);
    if (!p) return text(e, "FAIL:malloc");
    memset(p, 0x47, 32);
    unsigned char *q = realloc(p, 64);
    if (!q) {
      free(p);
      return text(e, "FAIL:realloc");
    }
    int ok = q[0] == 0x47 && q[31] == 0x47;
    free(q);
    return text(e, ok ? "allocation preserved" : "FAIL:realloc contents");
  }
  if (mode == 6) {
    struct lconv *l = localeconv();
    return text(e, l && l->decimal_point && strcmp(l->decimal_point, ".") == 0
                       ? "decimal point ."
                       : "FAIL:locale contract");
  }
  if (mode == 7) {
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &a))
      return text(e, "UNSUPPORTED:process CPU clock unavailable");
    snprintf(out, sizeof(out), "processCPU=%lld:%ld", (long long)a.tv_sec,
             a.tv_nsec);
    return text(e, out);
  }
  if (mode == 8) {
    snprintf(out, sizeof(out), "pointer=%zu long=%zu wchar=%zu", sizeof(void *),
             sizeof(long), sizeof(wchar_t));
    return text(e, out);
  }
  /* Shared-file path is provided by ordinary Java in a separate test. */
  return text(
      e, "NOT_RUN:cross-language file path supplied by integration harness");
}
static jstring graphics(JNIEnv *e, jclass c, jint mode) {
  (void)c;
  return as_probe_graphics(e, mode);
}
static jstring vulkan(JNIEnv *e, jclass c, jint mode) {
  (void)c;
  return as_probe_vulkan(e, mode);
}
JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved) {
  (void)reserved;
  machine = vm;
  JNIEnv *e = NULL;
  if ((*vm)->GetEnv(vm, (void **)&e, JNI_VERSION_1_6) != JNI_OK) return JNI_ERR;
  jclass c = (*e)->FindClass(e, "org/androswitch/conformance/NativeCases");
  if (!c) return JNI_ERR;
  JNINativeMethod methods[] = {
      {"integer", "(I)J", (void *)integer},
      {"mixed", "(DFIJ)D", (void *)mixed},
      {"echo", "(Ljava/lang/String;)Ljava/lang/String;", (void *)echo},
      {"array", "([I)[I", (void *)array},
      {"raise", "()V", (void *)raise_java},
      {"callback", "(I)I", (void *)callback},
      {"facts", "(ILjava/lang/String;)Ljava/lang/String;", (void *)facts},
      {"sharedFile", "(Ljava/lang/String;)Ljava/lang/String;",
       (void *)shared_file},
      {"graphics", "(I)Ljava/lang/String;", (void *)graphics},
      {"vulkan", "(I)Ljava/lang/String;", (void *)vulkan}};
  return (*e)->RegisterNatives(e, c, methods,
                               sizeof(methods) / sizeof(methods[0])) == 0
             ? JNI_VERSION_1_6
             : JNI_ERR;
}
