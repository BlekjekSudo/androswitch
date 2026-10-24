#include "media_format.h"
#include <stdatomic.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void as_native_trap(int);
enum Kind { INT32, INT64, FLOAT, STRING, BUFFER };
typedef struct Entry {
  struct Entry *next;
  char *name;
  enum Kind kind;
  size_t length;
  union {
    int32_t i32;
    int64_t i64;
    float f;
    void *data;
  } value;
} Entry;
typedef struct Format {
  Entry *entries;
  size_t count, bytes;
  char *description;
  struct Format *next;
} Format;
static Format *formats;
static atomic_flag registry_lock = ATOMIC_FLAG_INIT;
static void lock(void) {
  while (
      atomic_flag_test_and_set_explicit(&registry_lock, memory_order_acquire)) {
  }
}
static void unlock(void) {
  atomic_flag_clear_explicit(&registry_lock, memory_order_release);
}
_Noreturn static void fail(void) {
  /* Void setters cannot return OOM. Escape through the native-call boundary;
   * never silently discard the requested mutation. */
  as_native_trap(-2401);
  abort();
}
static size_t checked_name(const char *s) {
  if (!s)
    fail();
  size_t n = 0;
  while (n < 1024 && s[n])
    n++;
  if (n == 1024)
    fail();
  return n;
}
static Entry *find(Format *f, const char *name) {
  if (!f || !name)
    return NULL;
  for (Entry *e = f->entries; e; e = e->next)
    if (!strcmp(e->name, name))
      return e;
  return NULL;
}
static Format *format_new(void) {
  Format *f = calloc(1, sizeof(Format));
  if (f) {
    lock();
    f->next = formats;
    formats = f;
    unlock();
  }
  return f;
}
static int32_t format_delete(Format *f) {
  if (!f)
    return -10004; /* AMEDIA_ERROR_INVALID_PARAMETER */
  lock();
  Format **p = &formats;
  while (*p && *p != f)
    p = &(*p)->next;
  if (!*p) {
    unlock();
    return -10004;
  }
  *p = f->next;
  unlock();
  while (f->entries) {
    Entry *e = f->entries;
    f->entries = e->next;
    if (e->kind == STRING || e->kind == BUFFER)
      free(e->value.data);
    free(e->name);
    free(e);
  }
  free(f->description);
  free(f);
  return 0;
}
/* Only called after every media/guest worker has been joined. */
void as_media_format_reset(void) {
  while (formats)
    format_delete(formats);
}
static void put(Format *f, const char *name, enum Kind kind, const void *value,
                size_t length) {
  if (!f || (!value && length))
    fail();
  size_t name_length = checked_name(name);
  Entry *e = find(f, name);
  size_t old = e ? e->length : 0;
  if (length > 16 * 1024 * 1024 || f->bytes - old > 16 * 1024 * 1024 - length ||
      (!e && f->count == 1024))
    fail();
  void *copy = NULL;
  if (kind == STRING || kind == BUFFER) {
    copy = malloc(length ? length : 1);
    if (!copy)
      fail();
    if (length)
      memcpy(copy, value, length);
  }
  if (!e) {
    e = calloc(1, sizeof(*e));
    if (!e) {
      free(copy);
      fail();
    }
    e->name = malloc(name_length + 1);
    if (!e->name) {
      free(copy);
      free(e);
      fail();
    }
    memcpy(e->name, name, name_length + 1);
    e->next = f->entries;
    f->entries = e;
    f->count++;
  } else if (e->kind == STRING || e->kind == BUFFER)
    free(e->value.data);
  memset(&e->value, 0, sizeof(e->value));
  e->kind = kind;
  e->length = length;
  f->bytes = f->bytes - old + length;
  if (kind == STRING || kind == BUFFER)
    e->value.data = copy;
  else
    memcpy(&e->value, value, length);
}
#define SCALAR(Name, KindName, Type, Field)                                    \
  static void set_##Name(Format *f, const char *name, Type value) {            \
    put(f, name, KindName, &value, sizeof(value));                             \
  }                                                                            \
  static bool get_##Name(Format *f, const char *name, Type *out) {             \
    Entry *e = find(f, name);                                                  \
    if (!e || e->kind != KindName || !out)                                     \
      return false;                                                            \
    *out = e->value.Field;                                                     \
    return true;                                                               \
  }
SCALAR(Int32, INT32, int32_t, i32)
SCALAR(Int64, INT64, int64_t, i64)
SCALAR(Float, FLOAT, float, f)
static void set_String(Format *f, const char *name, const char *value) {
  if (!value)
    fail();
  size_t length = 0;
  while (length < 16 * 1024 * 1024 && value[length])
    length++;
  if (length == 16 * 1024 * 1024)
    fail();
  put(f, name, STRING, value, length + 1);
}
static void set_Buffer(Format *f, const char *name, const void *value,
                       size_t length) {
  put(f, name, BUFFER, value, length);
}
static bool get_String(Format *f, const char *name, const char **out) {
  Entry *e = find(f, name);
  if (!e || e->kind != STRING || !out)
    return false;
  *out = e->value.data;
  return true;
}
static bool get_Buffer(Format *f, const char *name, void **data,
                       size_t *length) {
  Entry *e = find(f, name);
  if (!e || e->kind != BUFFER || !data || !length)
    return false;
  *data = e->value.data;
  *length = e->length;
  return true;
}
static const char *format_string(Format *f) {
  if (!f)
    return NULL;
  size_t size = 3;
  for (Entry *e = f->entries; e; e = e->next)
    size += strlen(e->name) + (e->kind == STRING ? e->length : 96) + 8;
  char *text = malloc(size);
  if (!text)
    fail();
  size_t used = 0;
  text[used++] = '{';
  for (Entry *e = f->entries; e; e = e->next) {
    used += (size_t)snprintf(text + used, size - used, "%s: ", e->name);
    int count = 0;
    switch (e->kind) {
    case INT32:
      count = snprintf(text + used, size - used, "%ld", (long)e->value.i32);
      break;
    case INT64:
      count =
          snprintf(text + used, size - used, "%lld", (long long)e->value.i64);
      break;
    case FLOAT:
      count = snprintf(text + used, size - used, "%.9g", (double)e->value.f);
      break;
    case STRING:
      count =
          snprintf(text + used, size - used, "\"%s\"", (char *)e->value.data);
      break;
    case BUFFER:
      count = snprintf(text + used, size - used, "buffer[%zu]", e->length);
      break;
    }
    if (count < 0 || (size_t)count >= size - used) {
      free(text);
      fail();
    }
    used += (size_t)count;
    if (e->next) {
      text[used++] = ',';
      text[used++] = ' ';
    }
  }
  text[used++] = '}';
  text[used] = 0;
  free(f->description);
  f->description = text;
  return text;
}
uint64_t as_media_format_import(const char *name) {
#define API(n, f)                                                              \
  if (!strcmp(name, "AMediaFormat_" n))                                        \
  return (uint64_t)(uintptr_t)(f)
  API("new", format_new);
  API("delete", format_delete);
  API("toString", format_string);
  API("getInt32", get_Int32);
  API("setInt32", set_Int32);
  API("getInt64", get_Int64);
  API("setInt64", set_Int64);
  API("getFloat", get_Float);
  API("setFloat", set_Float);
  API("getString", get_String);
  API("setString", set_String);
  API("getBuffer", get_Buffer);
  API("setBuffer", set_Buffer);
#undef API
  return 0;
}
