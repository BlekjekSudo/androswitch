/* A failed JNI_OnLoad must retain its VM/mappings while a worker is alive. */
#include <jni.h>
#include <pthread.h>
#include <unistd.h>
static void *forever(void *unused) {
    (void)unused;
    for (;;) usleep(1000);
    return NULL;
}
JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void)vm;
    (void)reserved;
    pthread_t worker;
    if (pthread_create(&worker, NULL, forever, NULL)) return -1;
    return 0; /* Invalid JNI version, after starting an uncooperative worker. */
}
