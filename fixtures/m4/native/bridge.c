#include <jni.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static JavaVM *java_vm;
static __thread jint tls_value = 41;
typedef struct {pthread_mutex_t mutex;pthread_cond_t condition;int ready,go,value;} SyncWork;
static void *sync_worker(void *argument){SyncWork *work=argument;pthread_mutex_lock(&work->mutex);work->ready=1;pthread_cond_signal(&work->condition);while(!work->go)pthread_cond_wait(&work->condition,&work->mutex);work->value=work->value*5+4;pthread_mutex_unlock(&work->mutex);return NULL;}
JNIEXPORT jint JNICALL Java_org_androswitch_fixture_Bridge_synchronization(JNIEnv *env,jclass cls){
  (void)env;(void)cls;SyncWork work={.mutex=PTHREAD_MUTEX_INITIALIZER,.condition=PTHREAD_COND_INITIALIZER,.value=123};pthread_t worker;
  if(pthread_create(&worker,NULL,sync_worker,&work))return -1;
  pthread_mutex_lock(&work.mutex);while(!work.ready)pthread_cond_wait(&work.condition,&work.mutex);work.go=1;pthread_cond_signal(&work.condition);pthread_mutex_unlock(&work.mutex);
  if(pthread_join(worker,NULL))return -2;if(pthread_mutex_destroy(&work.mutex)||pthread_cond_destroy(&work.condition))return -3;
  pthread_mutex_t recursive;pthread_mutexattr_t attr;pthread_mutexattr_init(&attr);pthread_mutexattr_settype(&attr,PTHREAD_MUTEX_RECURSIVE);
  if(pthread_mutex_init(&recursive,&attr)||pthread_mutex_lock(&recursive)||pthread_mutex_trylock(&recursive)||pthread_mutex_unlock(&recursive)||pthread_mutex_unlock(&recursive)||pthread_mutex_destroy(&recursive)||pthread_mutexattr_destroy(&attr))return -4;
  return work.value;
}
JNIEXPORT jdouble JNICALL Java_org_androswitch_fixture_Bridge_many(JNIEnv *env,jclass klass,jint a,jint b,jint c,jint d,jint e,jint f,jint g,jint h,jint i,jdouble j,jdouble k,jdouble l,jdouble m,jdouble n,jdouble o,jdouble p,jdouble q,jdouble r){(void)env;(void)klass;return a+b*2+c*3+d*4+e*5+f*6+g*7+h*8+i*9+j+k*2+l*3+m*4+n*5+o*6+p*7+q*8+r*9;}
JNIEXPORT jdouble JNICALL Java_org_androswitch_fixture_Bridge_nativeFloatCallback(JNIEnv *env,jclass klass){jmethodID method=(*env)->GetStaticMethodID(env,klass,"floatCallback","(IDFJ)D");return (*env)->CallStaticDoubleMethod(env,klass,method,(jint)-11,(jdouble)1.75,(jdouble)-2.5,(jlong)12345);}
static jint registered(JNIEnv *env, jclass klass, jint value) {
  (void)env;
  (void)klass;
  return value * 3 + 5;
}
JNIEXPORT jint JNI_OnLoad(JavaVM *vm, void *reserved) {
  (void)reserved;
  java_vm = vm;
  JNIEnv *env = NULL;
  if ((*vm)->GetEnv(vm, (void **)&env, JNI_VERSION_1_6) != JNI_OK)
    return JNI_ERR;
  jclass klass = (*env)->FindClass(env, "org/androswitch/fixture/Bridge");
  JNINativeMethod methods[] = {{"registered", "(I)I", (void *)registered}};
  if (!klass || (*env)->RegisterNatives(env, klass, methods, 1) != JNI_OK)
    return JNI_ERR;
  (*env)->DeleteLocalRef(env, klass);
  return JNI_VERSION_1_6;
}
JNIEXPORT jdouble JNICALL Java_org_androswitch_fixture_Bridge_mixed(
    JNIEnv *env, jclass klass, jint a, jlong b, jfloat c, jdouble d) {
  (void)env;
  (void)klass;
  return (double)a + (double)b + (double)c * d;
}
JNIEXPORT jint JNICALL Java_org_androswitch_fixture_Bridge_nativeReenter(
    JNIEnv *env, jclass klass, jint value) {
  jmethodID method = (*env)->GetStaticMethodID(env, klass, "reentrant", "(I)I");
  return (*env)->CallStaticIntMethod(env, klass, method, value);
}
JNIEXPORT jstring JNICALL Java_org_androswitch_fixture_Bridge_roundtrip(
    JNIEnv *env, jclass klass, jstring value) {
  (void)klass;
  const char *text = (*env)->GetStringUTFChars(env, value, NULL);
  jstring copy = (*env)->NewStringUTF(env, text);
  (*env)->ReleaseStringUTFChars(env, value, text);
  return copy;
}
JNIEXPORT jint JNICALL
Java_org_androswitch_fixture_Bridge_arrays(JNIEnv *env, jclass klass) {
  (void)klass;
  jintArray values = (*env)->NewIntArray(env, 4);
  jint initial[] = {4, -7, 19, 31};
  (*env)->SetIntArrayRegion(env, values, 0, 4, initial);
  jint *copy = (*env)->GetIntArrayElements(env, values, NULL);
  copy[1] = 11;
  (*env)->ReleaseIntArrayElements(env, values, copy, JNI_COMMIT);
  copy[2] = 99;
  (*env)->ReleaseIntArrayElements(env, values, copy, JNI_ABORT);
  jint result[4];
  (*env)->GetIntArrayRegion(env, values, 0, 4, result);
  return result[0] + result[1] * 10 + result[2] * 100 + result[3] * 1000;
}
JNIEXPORT jint JNICALL
Java_org_androswitch_fixture_Bridge_references(JNIEnv *env, jclass klass) {
  if ((*env)->PushLocalFrame(env, 8) != JNI_OK)
    return -1;
  jbyteArray local = (*env)->NewByteArray(env, 3);
  jbyte bytes[] = {17, -2, 41};
  (*env)->SetByteArrayRegion(env, local, 0, 3, bytes);
  jobject global = (*env)->NewGlobalRef(env, local);
  (*env)->DeleteLocalRef(env, local);
  jmethodID collect = (*env)->GetStaticMethodID(env, klass, "collect", "()V");
  (*env)->CallStaticVoidMethod(env, klass, collect);
  jobject kept = (*env)->NewLocalRef(env, global);
  jobject returned = (*env)->PopLocalFrame(env, kept);
  (*env)->DeleteGlobalRef(env, global);
  (*env)->CallStaticVoidMethod(env, klass, collect);
  jbyte result[3];
  (*env)->GetByteArrayRegion(env, (jbyteArray)returned, 0, 3, result);
  jint answer = result[0] * 10000 + (result[1] & 255) * 100 + result[2];
  (*env)->DeleteLocalRef(env, returned);
  return answer;
}
JNIEXPORT void JNICALL
Java_org_androswitch_fixture_Bridge_throwing(JNIEnv *env, jclass klass) {
  (void)klass;
  jclass error = (*env)->FindClass(env, "java/lang/IllegalArgumentException");
  (*env)->ThrowNew(env, error, "native exception");
}
JNIEXPORT jint JNICALL Java_org_androswitch_fixture_Bridge_callbackException(
    JNIEnv *env, jclass klass) {
  jmethodID method = (*env)->GetStaticMethodID(env, klass, "raise", "()I");
  (*env)->CallStaticIntMethod(env, klass, method);
  if (!(*env)->ExceptionCheck(env))
    return -1;
  jthrowable error = (*env)->ExceptionOccurred(env);
  (*env)->ExceptionClear(env);
  jclass expected = (*env)->FindClass(env, "java/lang/ArithmeticException");
  jint result = (*env)->IsInstanceOf(env, error, expected) ? 89 : -2;
  (*env)->DeleteLocalRef(env, error);
  return result;
}
typedef struct {
  jclass klass;
  jmethodID method;
  jint value, result;
} Work;
static void *callback_thread(void *argument) {
  Work *work = argument;
  if(tls_value!=41){work->result=-8;return NULL;}
  tls_value=77;
  JNIEnv *env = NULL;
  if ((*java_vm)->GetEnv(java_vm, (void **)&env, JNI_VERSION_1_6) !=
      JNI_EDETACHED) {
    work->result = -3;
    return NULL;
  }
  if ((*java_vm)->AttachCurrentThread(java_vm, &env, NULL) != JNI_OK) {
    work->result = -4;
    return NULL;
  }
  work->result =
      (*env)->CallStaticIntMethod(env, work->klass, work->method, work->value);
  if(tls_value!=77)work->result=-9;
  if ((*java_vm)->DetachCurrentThread(java_vm) != JNI_OK)
    work->result = -5;
  return NULL;
}
JNIEXPORT jint JNICALL Java_org_androswitch_fixture_Bridge_callbackThread(
    JNIEnv *env, jclass klass, jint value) {
  Work work = {(jclass)(*env)->NewGlobalRef(env, klass),
               (*env)->GetStaticMethodID(env, klass, "callback", "(I)I"), value,
               0};
  pthread_t thread;
  if (pthread_create(&thread, NULL, callback_thread, &work))
    return -6;
  if (pthread_join(thread, NULL))
    return -7;
  (*env)->DeleteGlobalRef(env, work.klass);
  if(tls_value!=41)return -10;
  return work.result;
}
JNIEXPORT jint JNICALL Java_org_androswitch_fixture_Bridge_nativeStorage(
    JNIEnv *env, jclass klass, jstring value) {
  (void)klass;
  const char *path = (*env)->GetStringUTFChars(env, value, NULL);
  FILE *file = fopen(path, "r+b");
  (*env)->ReleaseStringUTFChars(env, value, path);
  if (!file)
    return -1;
  int result = fgetc(file);
  rewind(file);
  fputc(result + 5, file);
  fclose(file);
  return result;
}
JNIEXPORT jint JNICALL
Java_org_androswitch_fixture_Bridge_unsupported(JNIEnv *env, jclass klass) {
  return (*env)->MonitorEnter(env, klass);
}
JNIEXPORT jdouble JNICALL Java_org_androswitch_fixture_Bridge_nativeStackCallback(JNIEnv *env,jclass klass){
  jmethodID method=(*env)->GetStaticMethodID(env,klass,"stackCallback","(IIIIIIIIIDDDDDDDDD)D");
  return (*env)->CallStaticDoubleMethod(env,klass,method,1,2,3,4,5,6,7,8,9,0.25,1.25,2.25,3.25,4.25,5.25,6.25,7.25,8.25);
}
