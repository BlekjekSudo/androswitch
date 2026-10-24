/* Production entry points, compiled by the actual Horizon GCC in the CPU
 * oracle. Dispatch is modeled; this file does not pretend to execute DEX. */
#include "../../../platform/switch-smoke/jni_arguments.h"
#include <stddef.h>
#include <stdint.h>
#include <string.h>
enum { CONSTRUCT=38 };
uint64_t oracle_observed[22];
static int pending;
static jboolean exception_check(JNIEnv *env){(void)env;return pending;}
static size_t argument_types(jmethodID method,char *types){
  (void)method;const char *signature="IZBCSJFDL[DDDDDDDDI";
  memcpy(types,signature,19);return 19;
}
static uint64_t dispatch(uint32_t op,uint64_t klass,uint64_t method,uint64_t ignored,const void *data,size_t length){
  (void)ignored;oracle_observed[0]=op;oracle_observed[1]=klass;oracle_observed[2]=method;
  if(length!=19)return 0;
  memcpy(oracle_observed+3,data,19*8);return 0x5555;
}
#include "../../../platform/switch-smoke/jni_construct.inc"
static jclass find(JNIEnv *e,const char *n){(void)e;(void)n;return (jclass)(uintptr_t)0x1111;}
static jmethodID method(JNIEnv *e,jclass k,const char *n,const char *s){(void)e;(void)k;(void)n;(void)s;return (jmethodID)(uintptr_t)0x2222;}
JNIEnv *oracle_environment(void){
  static const struct JNINativeInterface table={.FindClass=find,.GetMethodID=method,
    .NewObject=new_object,.NewObjectV=new_object_v,.NewObjectA=new_object_a,.ExceptionCheck=exception_check};
  static JNIEnv env=&table;return &env;
}
void oracle_pending(int value){pending=value;}
