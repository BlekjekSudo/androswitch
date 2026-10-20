#include <jni.h>
#include <stdint.h>
static uint32_t feed(uint32_t h, uint32_t n) {
    for (unsigned i=0;i<4;i++) h=(h ^ ((n>>(8*i))&255u))*16777619u;
    return h;
}
JNIEXPORT jint JNICALL Java_org_rustydroid_nativeworkbench_MainActivity_digest(JNIEnv *e,jclass c,jint a,jint b) {
    (void)e;(void)c;
    return (jint)(feed(feed(2166136261u ^ 1u,(uint32_t)a),(uint32_t)b)&0x7fffffffu);
}
JNIEXPORT jint JNICALL Java_org_rustydroid_nativeworkbench_MainActivity_nativeFlavor(JNIEnv *e,jclass c) {(void)e;(void)c;return 1;}
