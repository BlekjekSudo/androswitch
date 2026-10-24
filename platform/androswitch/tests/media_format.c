/* Contract tests use only public imported entry points, not Format layout. */
#include "../media_format.h"
#include <assert.h>
#include <setjmp.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
static jmp_buf trapped;
static int trap_code;
void as_native_trap(int code) {
  trap_code = code;
  longjmp(trapped, 1);
}
#define FN(name, type)                                                         \
  ((type)(uintptr_t)as_media_format_import("AMediaFormat_" name))
typedef void *(*New)(void);
typedef int32_t (*Delete)(void *);
typedef void (*Set32)(void *, const char *, int32_t);
typedef bool (*Get32)(void *, const char *, int32_t *);
typedef void (*Set64)(void *, const char *, int64_t);
typedef bool (*Get64)(void *, const char *, int64_t *);
typedef void (*SetFloat)(void *, const char *, float);
typedef bool (*GetFloat)(void *, const char *, float *);
typedef void (*SetString)(void *, const char *, const char *);
typedef bool (*GetString)(void *, const char *, const char **);
typedef void (*SetBuffer)(void *, const char *, const void *, size_t);
typedef bool (*GetBuffer)(void *, const char *, void **, size_t *);
typedef const char *(*ToString)(void *);
int main(void) {
  void *f = FN("new", New)();
  assert(f);
  int32_t i32 = 777;
  assert(!FN("getInt32", Get32)(f, "missing", &i32) && i32 == 777);
  FN("setInt32", Set32)(f, "width", 1280);
  assert(FN("getInt32", Get32)(f, "width", &i32) && i32 == 1280);
  int64_t i64 = -1;
  assert(!FN("getInt64", Get64)(f, "width", &i64) && i64 == -1);
  FN("setInt64", Set64)(f, "durationUs", INT64_C(1234567890123));
  assert(FN("getInt64", Get64)(f, "durationUs", &i64) &&
         i64 == INT64_C(1234567890123));
  FN("setFloat", SetFloat)(f, "frame-rate", 29.5f);
  float rate = 0;
  assert(FN("getFloat", GetFloat)(f, "frame-rate", &rate) && rate == 29.5f);
  char mime[] = "audio/raw";
  FN("setString", SetString)(f, "mime", mime);
  mime[0] = 'X';
  const char *str = NULL;
  assert(FN("getString", GetString)(f, "mime", &str) &&
         !strcmp(str, "audio/raw"));
  unsigned char bytes[] = {0, 1, 2, 0xff};
  FN("setBuffer", SetBuffer)(f, "csd-0", bytes, sizeof(bytes));
  bytes[1] = 99;
  void *data = NULL;
  size_t length = 0;
  assert(FN("getBuffer", GetBuffer)(f, "csd-0", &data, &length) &&
         length == 4 && ((unsigned char *)data)[1] == 1);
  FN("setInt32", Set32)(f, "mime", 42);
  assert(!FN("getString", GetString)(f, "mime", &str));
  assert(FN("getInt32", Get32)(f, "mime", &i32) && i32 == 42);
  const char *description = FN("toString", ToString)(f);
  assert(strstr(description, "width: 1280") &&
         strstr(description, "buffer[4]"));
  FN("setInt32", Set32)(f, "width", 720);
  assert(strstr(description,
                "width: 1280")); /* valid until next toString/delete */
  assert(strstr(FN("toString", ToString)(f), "width: 720"));
  if (!setjmp(trapped)) {
    FN("setBuffer", SetBuffer)(f, "oversized", bytes, 16 * 1024 * 1024 + 1);
    assert(0);
  }
  assert(trap_code == -2401);
  assert(FN("delete", Delete)(f) == 0);
  assert(FN("new", New)());
  as_media_format_reset();
  as_media_format_reset();
  assert(as_media_format_import("AMediaCodec_createDecoderByType") == 0);
  puts("AMediaFormat typed values, ownership, replacement, budget and reset "
       "contracts passed");
}
