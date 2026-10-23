#include <jni.h>
extern "C" JNIEXPORT jint JNICALL
Java_org_androswitch_fixture_KotlinBridge_nativeValue(JNIEnv *env,
                                                      jobject receiver,
                                                      jint value) {
  jclass klass = env->GetObjectClass(receiver);
  jmethodID callback = env->GetMethodID(klass, "callback", "(I)I");
  jvalue arguments[1];
  arguments[0].i = value;
  jint result = env->CallIntMethodA(receiver, callback, arguments);
  env->DeleteLocalRef(klass);
  return result + 17;
}
