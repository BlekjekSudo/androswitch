/* The same independent NDK fixture call sites used by the SDK proof. */
#include "native/construct.c"
extern JNIEnv *oracle_environment(void);
extern void oracle_pending(int);
uint64_t oracle_case(int form){
  return (uintptr_t)make(oracle_environment(),form,9,(jobject)(uintptr_t)0x3333,(jintArray)(uintptr_t)0x4444);
}
uint64_t oracle_pending_case(int form){
  JNIEnv *env=oracle_environment();oracle_pending(1);jobject result;
  if(form==0)result=(*env)->NewObject(env,(jclass)(uintptr_t)0x1111,(jmethodID)(uintptr_t)0x2222);
  else if(form==1)result=forward_v(env,(jclass)(uintptr_t)0x1111,(jmethodID)(uintptr_t)0x2222);
  else result=(*env)->NewObjectA(env,(jclass)(uintptr_t)0x1111,(jmethodID)(uintptr_t)0x2222,NULL);
  oracle_pending(0);return (uintptr_t)result;
}
/* Freestanding CPU oracle has no guest libc. */
void *memcpy(void *out,const void *in,size_t length){
  unsigned char *d=out;const unsigned char *s=in;for(size_t i=0;i<length;i++){d[i]=s[i];}return out;
}
void *memset(void *out,int value,size_t length){
  unsigned char *d=out;for(size_t i=0;i<length;i++){d[i]=(unsigned char)value;}return out;
}
