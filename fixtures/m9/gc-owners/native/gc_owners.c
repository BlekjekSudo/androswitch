/* Independent Android API26 JNI regression. No Java object references are
 * retained here: local-only payloads must be traced from their DEX frames. */
#include <jni.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <time.h>

#define JNI_NAME(name) Java_org_androswitch_contracts_GcOwnersActivity_##name
static char report_path[512];
static atomic_int entered_a, entered_b, release_b, a_result, aborted;
static atomic_uintptr_t thread_a, thread_b;

static int wait_for(atomic_int *condition) {
    struct timespec start, now;
    if (clock_gettime(CLOCK_MONOTONIC, &start) != 0) return -1;
    while (!atomic_load_explicit(condition, memory_order_acquire)) {
        if (atomic_load(&aborted)) return -10;
        if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -2;
        if (now.tv_sec - start.tv_sec >= 15) return -3;
        const struct timespec pause = {0, 1000000};
        if (nanosleep(&pause, NULL) != 0) return -4;
    }
    return atomic_load(&aborted) ? -10 : 0;
}

JNIEXPORT void JNICALL JNI_NAME(prepare)(JNIEnv *env, jclass cls, jstring directory) {
    (void)cls;
    const char *path = (*env)->GetStringUTFChars(env, directory, NULL);
    if (path == NULL) return;
    int length = snprintf(report_path, sizeof(report_path), "%s/gc-owners-proof.json", path);
    (*env)->ReleaseStringUTFChars(env, directory, path);
    if (length < 0 || (size_t)length >= sizeof(report_path)) report_path[0] = '\0';
    atomic_store(&entered_a, 0);
    atomic_store(&entered_b, 0);
    atomic_store(&release_b, 0);
    atomic_store(&a_result, 0);
    atomic_store(&aborted, 0);
    atomic_store(&thread_a, 0);
    atomic_store(&thread_b, 0);
}

JNIEXPORT jint JNICALL JNI_NAME(barrier)(JNIEnv *env, jclass cls, jint owner) {
    (void)env;
    (void)cls;
    if (owner == 0) {
        atomic_store(&thread_a, (uintptr_t)pthread_self());
        atomic_store_explicit(&entered_a, 1, memory_order_release);
        return wait_for(&entered_b);
    }
    if (owner == 1) {
        atomic_store(&thread_b, (uintptr_t)pthread_self());
        int result = wait_for(&entered_a);
        if (result != 0) return result;
        atomic_store_explicit(&entered_b, 1, memory_order_release);
        return wait_for(&release_b);
    }
    return -5;
}

JNIEXPORT jint JNICALL JNI_NAME(releaseB)(JNIEnv *env, jclass cls, jint result_a) {
    (void)env;
    (void)cls;
    if (atomic_load(&aborted) || !atomic_load(&entered_a) || !atomic_load(&entered_b) || result_a != 101) return -6;
    atomic_store(&a_result, result_a);
    atomic_store_explicit(&release_b, 1, memory_order_release);
    return 0;
}

JNIEXPORT jint JNICALL JNI_NAME(finish)(JNIEnv *env, jclass cls, jint result_b) {
    (void)env;
    (void)cls;
    uintptr_t a = atomic_load(&thread_a), b = atomic_load(&thread_b);
    if (atomic_load(&aborted) || !atomic_load(&release_b) || atomic_load(&a_result) != 101 || result_b != 202 ||
        a == 0 || b == 0 || a == b || report_path[0] == '\0') return -7;
    FILE *file = fopen(report_path, "w");
    if (file == NULL) return -8;
    int written = fprintf(file,
        "{\"schema_version\":1,\"status\":\"GC_OWNERS_PROOF_PASS\","
        "\"a_result\":101,\"b_result\":202,\"a_native_thread\":%llu,"
        "\"b_native_thread\":%llu,\"release_after_a_body_return\":true,"
        "\"local_objects_passed_to_jni\":false}",
        (unsigned long long)a, (unsigned long long)b);
    int closed = fclose(file);
    return written > 0 && closed == 0 ? 0 : -9;
}

JNIEXPORT void JNICALL JNI_NAME(abortWaits)(JNIEnv *env, jclass cls) {
    (void)env;
    (void)cls;
    atomic_store_explicit(&aborted, 1, memory_order_release);
}
