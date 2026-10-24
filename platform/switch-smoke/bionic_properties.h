/* Read-only Bionic property ABI, backed by explicit runtime facts.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#pragma once
#include <errno.h>
#include <stddef.h>
#include <string.h>

enum { AS_PROP_VALUE_MAX = 92 };
#define AS_RUNTIME_PROPERTY(key, value)                                        \
  _Static_assert(sizeof(value) <= AS_PROP_VALUE_MAX,                           \
                 "Android property value exceeds ABI limit");
#include "runtime_properties.inc"
#undef AS_RUNTIME_PROPERTY

static inline int as_bionic_property_get(const char *name, char *value) {
  static const struct {
    const char *name, *value;
    size_t length;
  } properties[] = {
#define AS_RUNTIME_PROPERTY(key, value) {key, value, sizeof(value) - 1},
#include "runtime_properties.inc"
#undef AS_RUNTIME_PROPERTY
  };
  int saved_errno = errno;
  for (size_t i = 0; i < sizeof(properties) / sizeof(*properties); ++i) {
    if (!strcmp(name, properties[i].name)) {
      memcpy(value, properties[i].value, properties[i].length + 1);
      errno = saved_errno;
      return (int)properties[i].length;
    }
  }
  value[0] = '\0';
  errno = saved_errno;
  return 0;
}
