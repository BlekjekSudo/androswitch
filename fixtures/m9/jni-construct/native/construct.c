#include <jni.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <pthread.h>
#include <string.h>
#define SIGNATURE "(IZBCSJFDLjava/lang/Object;[IDDDDDDDDI)V"
static jobject forward_v(JNIEnv *env,jclass klass,jmethodID method,...){
  va_list args;va_start(args,method);
  jobject result=(*env)->NewObjectV(env,klass,method,args);va_end(args);return result;
}
static jobject make(JNIEnv *env,int form,int mode,jobject identity,jintArray array){
  jclass klass=(*env)->FindClass(env,"org/androswitch/contracts/Payload");
  jmethodID ctor=(*env)->GetMethodID(env,klass,"<init>",SIGNATURE);
  if((*env)->ExceptionCheck(env))return NULL;
  if(form==0)return (*env)->NewObject(env,klass,ctor,(jint)-17,(jint)1,(jint)-113,(jint)65530,(jint)-23456,
    (jlong)INT64_C(0x123456789abcdef),(jdouble)1.25,(jdouble)-3.5,identity,array,
    (jdouble)0.5,(jdouble)1.5,(jdouble)2.5,(jdouble)3.5,(jdouble)4.5,(jdouble)5.5,(jdouble)6.5,(jdouble)7.5,(jint)mode);
  if(form==1)return forward_v(env,klass,ctor,(jint)-17,(jint)1,(jint)-113,(jint)65530,(jint)-23456,
    (jlong)INT64_C(0x123456789abcdef),(jdouble)1.25,(jdouble)-3.5,identity,array,
    (jdouble)0.5,(jdouble)1.5,(jdouble)2.5,(jdouble)3.5,(jdouble)4.5,(jdouble)5.5,(jdouble)6.5,(jdouble)7.5,(jint)mode);
  jvalue args[19]={0};args[0].i=-17;args[1].z=1;args[2].b=-113;args[3].c=65530;args[4].s=-23456;
  args[5].j=INT64_C(0x123456789abcdef);args[6].f=1.25f;args[7].d=-3.5;args[8].l=identity;args[9].l=array;
  for(int i=0;i<8;i++){args[10+i].d=0.5+i;}
  args[18].i=mode;
  return (*env)->NewObjectA(env,klass,ctor,args);
}
JNIEXPORT jobject JNICALL Java_org_androswitch_contracts_ConstructorCases_create(
  JNIEnv *env,jclass klass,jint form,jint mode,jobject identity,jintArray array){
  (void)klass;return make(env,form,mode,identity,array);
}
JNIEXPORT jint JNICALL Java_org_androswitch_contracts_ConstructorCases_nested(JNIEnv *env,jclass root){
  (void)root;jclass klass=(*env)->FindClass(env,"org/androswitch/contracts/Leaf");
  jmethodID ctor=(*env)->GetMethodID(env,klass,"<init>","(I)V");
  jobject leaf=forward_v(env,klass,ctor,(jint)37);
  if(!leaf||(*env)->ExceptionCheck(env))return -1;
  jmethodID value=(*env)->GetMethodID(env,klass,"value","()I");
  return (*env)->CallIntMethod(env,leaf,value);
}
JNIEXPORT jint JNICALL Java_org_androswitch_contracts_ConstructorCases_exceptionProbe(JNIEnv *env,jclass root,jint form){
  (void)root;jobject object=make(env,form,1,NULL,NULL);
  if(object||!(*env)->ExceptionCheck(env))return -1;
  jthrowable thrown=(*env)->ExceptionOccurred(env);(*env)->ExceptionClear(env);
  jclass expected=(*env)->FindClass(env,"java/lang/IllegalStateException");
  return (*env)->IsInstanceOf(env,thrown,expected)?71:-2;
}
static int thread_name(JNIEnv *env,jobject object,jmethodID name,const char *expected){
  jstring text=(*env)->CallObjectMethod(env,object,name);
  if(!text||(*env)->ExceptionCheck(env))return 0;
  const char *bytes=(*env)->GetStringUTFChars(env,text,NULL);
  if(!bytes)return 0;
  int matches=strcmp(bytes,expected)==0;(*env)->ReleaseStringUTFChars(env,text,bytes);return matches;
}
JNIEXPORT jint JNICALL Java_org_androswitch_contracts_ConstructorCases_threadIdentity(JNIEnv *env,jclass root){
  (void)root;jclass klass=(*env)->FindClass(env,"java/lang/Thread");
  jmethodID current=(*env)->GetStaticMethodID(env,klass,"currentThread","()Ljava/lang/Thread;");
  jmethodID name=(*env)->GetMethodID(env,klass,"getName","()Ljava/lang/String;");
  jmethodID id=(*env)->GetMethodID(env,klass,"getId","()J");
  jmethodID alive=(*env)->GetMethodID(env,klass,"isAlive","()Z");
  if((*env)->ExceptionCheck(env))return -1;
  jobject first=(*env)->CallStaticObjectMethod(env,klass,current);
  jobject second=(*env)->CallStaticObjectMethod(env,klass,current);
  if(!first||!(*env)->IsSameObject(env,first,second)||(*env)->CallLongMethod(env,first,id)<=0
      ||!(*env)->CallBooleanMethod(env,first,alive)||!thread_name(env,first,name,"main"))return -2;
  jmethodID ctor=(*env)->GetMethodID(env,klass,"<init>","(Ljava/lang/String;)V");
  jstring label=(*env)->NewStringUTF(env,"JNI constructed thread");
  jobject created=forward_v(env,klass,ctor,label);
  if(!created||(*env)->CallBooleanMethod(env,created,alive)||!thread_name(env,created,name,"JNI constructed thread"))return -3;
  return 89;
}
JNIEXPORT jint JNICALL Java_org_androswitch_contracts_ConstructorCases_contextLoader(JNIEnv *env,jclass root){
  jclass thread_class=(*env)->FindClass(env,"java/lang/Thread");
  jclass class_class=(*env)->FindClass(env,"java/lang/Class");
  jmethodID current=(*env)->GetStaticMethodID(env,thread_class,"currentThread","()Ljava/lang/Thread;");
  jmethodID getter=(*env)->GetMethodID(env,thread_class,"getContextClassLoader","()Ljava/lang/ClassLoader;");
  jmethodID setter=(*env)->GetMethodID(env,thread_class,"setContextClassLoader","(Ljava/lang/ClassLoader;)V");
  jmethodID defining=(*env)->GetMethodID(env,class_class,"getClassLoader","()Ljava/lang/ClassLoader;");
  jmethodID ctor=(*env)->GetMethodID(env,thread_class,"<init>","(Ljava/lang/String;)V");
  if((*env)->ExceptionCheck(env))return -1;
  jobject main=(*env)->CallStaticObjectMethod(env,thread_class,current);
  jobject app_loader=(*env)->CallObjectMethod(env,root,defining);
  jobject inherited=(*env)->CallObjectMethod(env,main,getter);
  if(!app_loader||!(*env)->IsSameObject(env,app_loader,inherited))return -2;
  jstring label=(*env)->NewStringUTF(env,"context child");
  jobject child=forward_v(env,thread_class,ctor,label);
  if(!child||!(*env)->IsSameObject(env,(*env)->CallObjectMethod(env,child,getter),app_loader))return -3;
  (*env)->CallVoidMethod(env,main,setter,NULL);
  jobject bootstrap_child=forward_v(env,thread_class,ctor,label);
  jobject bootstrap=bootstrap_child?(*env)->CallObjectMethod(env,bootstrap_child,getter):app_loader;
  (*env)->CallVoidMethod(env,main,setter,app_loader);
  if(!bootstrap_child||bootstrap||(*env)->ExceptionCheck(env))return -4;
  if(!(*env)->IsSameObject(env,(*env)->CallObjectMethod(env,child,getter),app_loader))return -5;
  return 103;
}
typedef struct {JavaVM *vm;jobject thread;jint result;} Attached;
static void *attached_entry(void *argument){
  Attached *work=argument;JNIEnv *env=NULL;
  JavaVMAttachArgs args={JNI_VERSION_1_6,"JNI named worker",NULL};
  if((*work->vm)->AttachCurrentThread(work->vm,&env,&args)!=JNI_OK){work->result=-4;return NULL;}
  jclass klass=(*env)->FindClass(env,"java/lang/Thread");
  jmethodID current=(*env)->GetStaticMethodID(env,klass,"currentThread","()Ljava/lang/Thread;");
  jmethodID name=(*env)->GetMethodID(env,klass,"getName","()Ljava/lang/String;");
  jmethodID alive=(*env)->GetMethodID(env,klass,"isAlive","()Z");
  jmethodID context=(*env)->GetMethodID(env,klass,"getContextClassLoader","()Ljava/lang/ClassLoader;");
  if((*env)->ExceptionCheck(env)){work->result=-5;return NULL;}
  jobject thread=(*env)->CallStaticObjectMethod(env,klass,current);
  if(!thread||!thread_name(env,thread,name,"JNI named worker")||!(*env)->CallBooleanMethod(env,thread,alive)){work->result=-6;return NULL;}
  if((*env)->CallObjectMethod(env,thread,context)||(*env)->ExceptionCheck(env)){work->result=-9;return NULL;}
  work->thread=(*env)->NewGlobalRef(env,thread);
  work->result=(*work->vm)->DetachCurrentThread(work->vm)==JNI_OK?97:-7;
  return NULL;
}
JNIEXPORT jint JNICALL Java_org_androswitch_contracts_ConstructorCases_attachedThread(JNIEnv *env,jclass root){
  Attached work={0};if((*env)->GetJavaVM(env,&work.vm)!=JNI_OK)return -1;
  pthread_t thread;if(pthread_create(&thread,NULL,attached_entry,&work))return -2;
  if(pthread_join(thread,NULL))return -3;
  if(work.result!=97)return work.result;
  jclass klass=(*env)->FindClass(env,"java/lang/Thread");
  jmethodID alive=(*env)->GetMethodID(env,klass,"isAlive","()Z");
  jmethodID name=(*env)->GetMethodID(env,klass,"getName","()Ljava/lang/String;");
  if(!work.thread||(*env)->CallBooleanMethod(env,work.thread,alive)||!thread_name(env,work.thread,name,"JNI named worker"))return -8;
  (*env)->DeleteGlobalRef(env,work.thread);
  jmethodID collect=(*env)->GetStaticMethodID(env,root,"collect","()V");
  (*env)->CallStaticVoidMethod(env,root,collect);return 97;
}
