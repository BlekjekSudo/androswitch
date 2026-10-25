#include <jni.h>
#include <stdint.h>
#include <string.h>
extern JNIEnv *oracle_environment(void);
const char oracle_name[] = "counter", oracle_signature[] = "J";
uint64_t oracle_case(uint64_t mode) {
  JNIEnv *env = oracle_environment();
  jclass type = (jclass)0x1111;
  jfieldID field = (jfieldID)0x2222;
  uint32_t fbits = 0x7fc01234;
  uint64_t dbits = UINT64_C(0xfff81234567890ab);
  jfloat f;
  jdouble d;
  memcpy(&f, &fbits, 4);
  memcpy(&d, &dbits, 8);
  switch (mode) {
    case 0:
      return (uint64_t)(*env)->GetStaticFieldID(env, type, oracle_name,
                                                oracle_signature);
    case 1:
      return (uint64_t)(*env)->GetStaticObjectField(env, type, field);
    case 2:
      return (*env)->GetStaticBooleanField(env, type, field);
    case 3:
      return (uint64_t)(int64_t)(*env)->GetStaticByteField(env, type, field);
    case 4:
      return (*env)->GetStaticCharField(env, type, field);
    case 5:
      return (uint64_t)(int64_t)(*env)->GetStaticShortField(env, type, field);
    case 6:
      return (uint64_t)(int64_t)(*env)->GetStaticIntField(env, type, field);
    case 7:
      return (uint64_t)(*env)->GetStaticLongField(env, type, field);
    case 8:
      f = (*env)->GetStaticFloatField(env, type, field);
      memcpy(&fbits, &f, 4);
      return fbits;
    case 9:
      d = (*env)->GetStaticDoubleField(env, type, field);
      memcpy(&dbits, &d, 8);
      return dbits;
    case 10:
      (*env)->SetStaticObjectField(env, type, field, (jobject)0x3333);
      break;
    case 11:
      (*env)->SetStaticBooleanField(env, type, field, JNI_TRUE);
      break;
    case 12:
      (*env)->SetStaticByteField(env, type, field, -113);
      break;
    case 13:
      (*env)->SetStaticCharField(env, type, field, 65530);
      break;
    case 14:
      (*env)->SetStaticShortField(env, type, field, -23456);
      break;
    case 15:
      (*env)->SetStaticIntField(env, type, field, -17);
      break;
    case 16:
      (*env)->SetStaticLongField(env, type, field,
                                 (jlong)UINT64_C(0x8000000000001234));
      break;
    case 17:
      (*env)->SetStaticFloatField(env, type, field, f);
      break;
    case 18:
      (*env)->SetStaticDoubleField(env, type, field, d);
      break;
    default:
      return 0;
  }
  return 0x5a;
}
