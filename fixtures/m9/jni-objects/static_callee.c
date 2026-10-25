#include <stddef.h>
#include <stdint.h>

#include "../../../third_party/android/jni.h"
void *memcpy(void *out, const void *in, size_t length) {
  unsigned char *a = out;
  const unsigned char *b = in;
  while (length--) *a++ = *b++;
  return out;
}
enum { FIELD = 34, GET_FIELD = 35, SET_FIELD = 36 };
uint64_t oracle_observed[6], oracle_result;
static uint64_t dispatch(uint32_t op, uint64_t a, uint64_t b, uint64_t c,
                         const void *data, size_t length) {
  oracle_observed[0] = op;
  oracle_observed[1] = a;
  oracle_observed[2] = b;
  oracle_observed[3] = c;
  oracle_observed[4] = data ? *(const uint64_t *)data : 0;
  oracle_observed[5] = length;
  return op == FIELD ? 0x5555 : oracle_result;
}
#include "../../../platform/switch-smoke/jni_static_fields.inc"
static struct JNINativeInterface table;
static const struct JNINativeInterface *environment = &table;
JNIEnv *oracle_environment(void) {
  table.GetStaticFieldID = static_field_id;
#define BIND(Name)                                          \
  table.GetStatic##Name##Field = get_static_##Name##_field; \
  table.SetStatic##Name##Field = set_static_##Name##_field
  BIND(Object);
  BIND(Boolean);
  BIND(Byte);
  BIND(Char);
  BIND(Short);
  BIND(Int);
  BIND(Long);
  BIND(Float);
  BIND(Double);
#undef BIND
  return &environment;
}
