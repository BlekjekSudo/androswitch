/* Descriptor-driven JNI arguments, independently implemented from the JNI
 * contract and AAPCS64. AndroSwitch contributors, MIT OR Apache-2.0.
 * Use the compiler's va_copy/va_arg, never a guessed register/stack layout. */
#pragma once
#include "../../third_party/android/jni.h"
#include <stdint.h>
#include <string.h>
#if defined(__aarch64__)
_Static_assert(sizeof(va_list) == 32, "AAPCS64 va_list size");
_Static_assert(_Alignof(va_list) == 8, "AAPCS64 va_list alignment");
#endif
static inline void as_jni_arguments_a(const char *types, size_t count,
                                     const jvalue *values, uint64_t *bits) {
  for (size_t i = 0; i < count; ++i) {
    bits[i] = 0;
    switch (types[i]) {
    case 'Z': bits[i] = values[i].z; break;
    case 'B': bits[i] = (uint64_t)(int64_t)values[i].b; break;
    case 'C': bits[i] = values[i].c; break;
    case 'S': bits[i] = (uint64_t)(int64_t)values[i].s; break;
    case 'I': bits[i] = (uint64_t)(int64_t)values[i].i; break;
    case 'J': bits[i] = (uint64_t)values[i].j; break;
    case 'F': memcpy(&bits[i], &values[i].f, sizeof(jfloat)); break;
    case 'D': memcpy(&bits[i], &values[i].d, sizeof(jdouble)); break;
    case 'L': case '[': bits[i] = (uintptr_t)values[i].l; break;
    }
  }
}
static inline void as_jni_arguments_v(const char *types, size_t count,
                                     va_list values, uint64_t *bits) {
  va_list copy;
  va_copy(copy, values);
  for (size_t i = 0; i < count; ++i) {
    bits[i] = 0;
    switch (types[i]) {
    /* Default C argument promotions apply to boolean/byte/char/short and
     * float. jlong and jobject keep their own ABI types. */
    case 'Z': bits[i] = (jboolean)va_arg(copy, jint); break;
    case 'B': bits[i] = (uint64_t)(int64_t)(jbyte)va_arg(copy, jint); break;
    case 'C': bits[i] = (jchar)va_arg(copy, jint); break;
    case 'S': bits[i] = (uint64_t)(int64_t)(jshort)va_arg(copy, jint); break;
    case 'I': bits[i] = (uint64_t)(int64_t)va_arg(copy, jint); break;
    case 'J': bits[i] = (uint64_t)va_arg(copy, jlong); break;
    case 'F': {
      jfloat v = (jfloat)va_arg(copy, jdouble);
      memcpy(&bits[i], &v, sizeof(v)); break;
    }
    case 'D': {
      jdouble v = va_arg(copy, jdouble);
      memcpy(&bits[i], &v, sizeof(v)); break;
    }
    case 'L': case '[': bits[i] = (uintptr_t)va_arg(copy, jobject); break;
    }
  }
  va_end(copy);
}
