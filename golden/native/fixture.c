/* MIT OR Apache-2.0. ABI fixtures, NOT a substitute for five real APKs.
 * No host jni.h dependency: these declarations are the JNI C ABI subset. */
typedef int jint;
typedef long long jlong;
typedef float jfloat;
typedef double jdouble;
typedef void *jobject;
typedef void *jclass;
typedef void *jstring;
typedef void *jintArray;
typedef void **JNIEnv;
typedef void **JavaVM;
typedef union { jint i; jlong j; jfloat f; jdouble d; jobject l; } jvalue;
typedef struct { const char *name; const char *signature; void *fnPtr; } JNINativeMethod;
#define JCALL(e,n,t) ((t)((*e)[n]))
static jint seed;
__attribute__((constructor)) static void initialise(void) { seed = 7; }
jint Java_test_Native_add(JNIEnv *e,jclass c,jint a,jint b) {(void)e;(void)c;return a+b+seed;}
jlong Java_test_Native_wide(JNIEnv *e,jclass c,jint a,jlong b,jint d) {(void)e;(void)c;return b+a+d;}
jdouble Java_test_Native_floating(JNIEnv *e,jclass c,jfloat a,jdouble b) {(void)e;(void)c;return a+b;}
jint Java_test_Native_many(JNIEnv *e,jclass c,jint a,jint b,jint d,jint f,jint g,jint h,jint i,jint j) {(void)e;(void)c;return a+b+d+f+g+h+i+j;}
jstring Java_test_Native_greeting(JNIEnv *e,jclass c) {(void)c;return JCALL(e,167,jstring(*)(JNIEnv*,const char*))(e,"hello\xc0\x80world \xed\xa0\xbd\xed\xb8\x80");}
jstring Java_test_Native_echo(JNIEnv *e,jclass c,jstring s) {(void)c;const char *p=JCALL(e,169,const char*(*)(JNIEnv*,jstring,void*))(e,s,0);jstring result=JCALL(e,167,jstring(*)(JNIEnv*,const char*))(e,p);JCALL(e,170,void(*)(JNIEnv*,jstring,const char*))(e,s,p);return result;}
jint Java_test_Native_sum(JNIEnv *e,jclass c,jintArray a) {(void)c;jint len=JCALL(e,171,jint(*)(JNIEnv*,jobject))(e,a);jint *p=JCALL(e,187,jint*(*)(JNIEnv*,jintArray,void*))(e,a,0);jint sum=0;for(jint i=0;i<len;i++)sum+=p[i];JCALL(e,195,void(*)(JNIEnv*,jintArray,jint*,jint))(e,a,p,2);return sum;}
void Java_test_Native_mutate(JNIEnv *e,jclass c,jintArray a) {(void)c;jint values[2]={19,23};JCALL(e,211,void(*)(JNIEnv*,jintArray,jint,jint,const jint*))(e,a,1,2,values);}
void Java_test_Native_throws(JNIEnv *e,jclass c) {(void)c;jclass ex=JCALL(e,6,jclass(*)(JNIEnv*,const char*))(e,"java/lang/IllegalArgumentException");JCALL(e,14,jint(*)(JNIEnv*,jclass,const char*))(e,ex,"from native");}
jint Java_test_Native_callback(JNIEnv *e,jclass c,jint value) {void *m=JCALL(e,113,void*(*)(JNIEnv*,jclass,const char*,const char*))(e,c,"twice","(I)I");jvalue args[1];args[0].i=value;return JCALL(e,131,jint(*)(JNIEnv*,jclass,void*,const jvalue*))(e,c,m,args);}
static jint registered(JNIEnv *e,jclass c,jint x) {(void)e;(void)c;return x*3;}
jint JNI_OnLoad(JavaVM *v,void *reserved) {(void)reserved;JNIEnv *env=0;if(((jint(*)(JavaVM*,void**,jint))((*v)[6]))(v,(void**)&env,0x10006)!=0)return -1;jclass c=JCALL(env,6,jclass(*)(JNIEnv*,const char*))(env,"test/Native");JNINativeMethod m={"registered","(I)I",(void*)registered};return JCALL(env,215,jint(*)(JNIEnv*,jclass,const JNINativeMethod*,jint))(env,c,&m,1)==0?0x10006:-1;}
jint Java_test_Native_spin(JNIEnv *e,jclass c) {(void)e;(void)c;volatile jint n=0;for(;;)n++;}
/* Lazily bound libc imports: implemented formatter and an absent import. */
int snprintf(char *, unsigned long, const char *, ...);
int getaddrinfo(const char *, const char *, const void *, void **);
jstring Java_test_Native_format(JNIEnv *e,jclass c,jint v) {(void)c;char buf[64];snprintf(buf,sizeof buf,"%s|%d|%05x|%-3c|%lld","rd",v,0xab,'z',-1234567890123LL);return JCALL(e,167,jstring(*)(JNIEnv*,const char*))(e,buf);}
jint Java_test_Native_missing(JNIEnv *e,jclass c) {(void)e;(void)c;void *r=0;return getaddrinfo("localhost",0,0,&r);}
/* M9 additions: string regions, ExceptionCheck (slot 228), float varargs, libm. */
float fmodf(float, float);
double floor(double);
jstring Java_test_Native_extras(JNIEnv *e,jclass c,jstring s,jfloat x) {(void)c;char sub[16];unsigned short u[2];char buf[128];
  JCALL(e,221,void(*)(JNIEnv*,jstring,jint,jint,char*))(e,s,1,3,sub);
  JCALL(e,220,void(*)(JNIEnv*,jstring,jint,jint,unsigned short*))(e,s,0,2,u);
  jint pending=JCALL(e,228,unsigned char(*)(JNIEnv*))(e);
  snprintf(buf,sizeof buf,"%s|%u,%u|%d|%.3f|%g|%.2e",sub,(unsigned)u[0],(unsigned)u[1],pending,(double)fmodf(x,2.0f),floor((double)x)+0.25,12345.678);
  return JCALL(e,167,jstring(*)(JNIEnv*,const char*))(e,buf);}
