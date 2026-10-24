/* liblog/syslog share the VM's persistent Android Log sink.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "native.h"
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern int as_guest_log(int priority, const char *tag, const char *message);
static char log_ident[256];
static int write_log(int priority, const char *tag, const char *message) {
  if (!message || priority < 0 || priority > 8) {
    errno = EINVAL;
    return -1;
  }
  as_jni_vm_lock();
  int result = as_guest_log(priority, tag ? tag : "", message);
  as_jni_vm_unlock();
  return result;
}
static int print_log_va(int priority, const char *tag, const char *format,
                        va_list args) {
  va_list copy;
  va_copy(copy, args);
  int length = vsnprintf(NULL, 0, format, copy);
  va_end(copy);
  if (length < 0 || length > 63 << 10) {
    errno = EOVERFLOW;
    return -1;
  }
  char *message = malloc((size_t)length + 1);
  if (!message) {
    errno = ENOMEM;
    return -1;
  }
  vsnprintf(message, (size_t)length + 1, format, args);
  int result = write_log(priority, tag, message);
  free(message);
  return result;
}
static int print_log(int priority, const char *tag, const char *format, ...) {
  va_list args;
  va_start(args, format);
  int result = print_log_va(priority, tag, format, args);
  va_end(args);
  return result;
}
static int write_log_buffer(int buffer, int priority, const char *tag,
                            const char *message) {
  /* Buffer IDs identify Android's log channels. The runtime records all
   * supported channels in its shared bounded sink instead of a log daemon. */
  if (buffer < 0 || buffer > 7) {
    errno = EINVAL;
    return -1;
  }
  return write_log(priority, tag, message);
}
static void open_log(const char *ident, int options, int facility) {
  (void)options;
  (void)facility;
  as_cxx_lock();
  size_t length = ident ? strnlen(ident, sizeof(log_ident) - 1) : 0;
  if (length)
    memcpy(log_ident, ident, length);
  log_ident[length] = 0;
  as_cxx_unlock();
}
static void close_log(void) { open_log(NULL, 0, 0); }
static void sys_log(int priority, const char *format, ...) {
  char tag[sizeof(log_ident)];
  as_cxx_lock();
  memcpy(tag, log_ident, sizeof(tag));
  as_cxx_unlock();
  int level = priority & 7;
  int android = level <= 3 ? 6 : level == 4 ? 5 : level <= 6 ? 4 : 3;
  va_list args;
  va_start(args, format);
  print_log_va(android, tag, format, args);
  va_end(args);
}
void as_bionic_log_reset(void) { close_log(); }
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
uint64_t as_bionic_log_import(const char *name) {
  API("__android_log_write", write_log);
  API("__android_log_buf_write", write_log_buffer);
  API("__android_log_print", print_log);
  API("__android_log_vprint", print_log_va);
  API("openlog", open_log);
  API("closelog", close_log);
  API("syslog", sys_log);
  return 0;
}
