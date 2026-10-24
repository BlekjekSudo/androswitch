/* ABI-compatible scalar C/fortify imports shared by all guest libraries.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "native.h"
#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/time.h>
#include <time.h>
static void bounds_fail(void) { as_native_trap(-1210); }
static void *memcpy_checked(void *out, const void *in, size_t count,
                            size_t size) {
  if (count > size)
    bounds_fail();
  return memcpy(out, in, count);
}
static void *memmove_checked(void *out, const void *in, size_t count,
                             size_t size) {
  if (count > size)
    bounds_fail();
  return memmove(out, in, count);
}
static void *memset_checked(void *out, int byte, size_t count, size_t size) {
  if (count > size)
    bounds_fail();
  return memset(out, byte, count);
}
static size_t strlen_checked(const char *text, size_t size) {
  const char *end = memchr(text, 0, size);
  if (!end) {
    bounds_fail();
    return 0;
  }
  return (size_t)(end - text);
}
static char *strcpy_checked(char *out, const char *in, size_t size) {
  size_t length = strlen(in);
  if (length >= size)
    bounds_fail();
  return memcpy(out, in, length + 1);
}
static char *strncpy_checked(char *out, const char *in, size_t count,
                             size_t size) {
  if (count > size)
    bounds_fail();
  return strncpy(out, in, count);
}
static char *strncpy_checked2(char *out, const char *in, size_t count,
                              size_t out_size, size_t in_size) {
  if (count > out_size || (count > in_size && !memchr(in, 0, in_size)))
    bounds_fail();
  return strncpy(out, in, count);
}
static char *strchr_checked(const char *text, int byte, size_t size) {
  for (size_t i = 0; i < size; ++i) {
    if ((unsigned char)text[i] == (unsigned char)byte)
      return (char *)text + i;
    if (!text[i])
      return NULL;
  }
  bounds_fail();
  return NULL;
}
static char *strncat_checked(char *out, const char *in, size_t count,
                             size_t size) {
  size_t length = strlen_checked(out, size), appended = strnlen(in, count);
  if (appended >= size - length)
    bounds_fail();
  memcpy(out + length, in, appended);
  out[length + appended] = 0;
  return out;
}
static int vsnprintf_checked(char *out, size_t count, int flags, size_t size,
                             const char *format, va_list args) {
  (void)flags;
  if (count > size)
    bounds_fail();
  return vsnprintf(out, count, format, args);
}
static int vsprintf_checked(char *out, int flags, size_t size,
                            const char *format, va_list args) {
  (void)flags;
  int result = vsnprintf(out, size, format, args);
  if (result >= 0 && (size_t)result >= size)
    bounds_fail();
  return result;
}
static int sprintf_checked(char *out, int flags, size_t size,
                           const char *format, ...) {
  va_list args;
  va_start(args, format);
  int result = vsprintf_checked(out, flags, size, format, args);
  va_end(args);
  return result;
}
/* Bionic's LP64 tm has UTC-offset/zone fields absent from newlib's tm.
 * Translate fields instead of exposing host layouts across the ABI. */
typedef struct {
  int sec, min, hour, mday, mon, year, wday, yday, isdst;
  long gmtoff;
  const char *zone;
} GuestTm;
static __thread GuestTm static_time;
static __thread char time_zone[64];
static GuestTm *from_host_time(const struct tm *host, GuestTm *guest, int utc) {
  guest->sec = host->tm_sec;
  guest->min = host->tm_min;
  guest->hour = host->tm_hour;
  guest->mday = host->tm_mday;
  guest->mon = host->tm_mon;
  guest->year = host->tm_year;
  guest->wday = host->tm_wday;
  guest->yday = host->tm_yday;
  guest->isdst = host->tm_isdst;
  if (utc) {
    guest->gmtoff = 0;
    guest->zone = "GMT";
  } else {
    char offset[16];
    if (strftime(offset, sizeof(offset), "%z", host) != 5 ||
        (offset[0] != '+' && offset[0] != '-') || offset[1] < '0' ||
        offset[1] > '9' || offset[2] < '0' || offset[2] > '9' ||
        offset[3] < '0' || offset[3] > '9' || offset[4] < '0' ||
        offset[4] > '9') {
      errno = ENOTSUP;
      return NULL;
    }
    guest->gmtoff = ((offset[1] - '0') * 10 + offset[2] - '0') * 3600 +
                    ((offset[3] - '0') * 10 + offset[4] - '0') * 60;
    if (offset[0] == '-')
      guest->gmtoff = -guest->gmtoff;
    if (!strftime(time_zone, sizeof(time_zone), "%Z", host)) {
      errno = ENOTSUP;
      return NULL;
    }
    guest->zone = time_zone;
  }
  return guest;
}
static GuestTm *guest_gmtime_r(const time_t *value, GuestTm *out) {
  struct tm host;
  return gmtime_r(value, &host) ? from_host_time(&host, out, 1) : NULL;
}
static GuestTm *guest_localtime_r(const time_t *value, GuestTm *out) {
  struct tm host;
  return localtime_r(value, &host) ? from_host_time(&host, out, 0) : NULL;
}
static GuestTm *guest_gmtime(const time_t *value) {
  return guest_gmtime_r(value, &static_time);
}
static GuestTm *guest_localtime(const time_t *value) {
  return guest_localtime_r(value, &static_time);
}
static time_t guest_mktime(GuestTm *value) {
  struct tm host = {0};
  host.tm_sec = value->sec;
  host.tm_min = value->min;
  host.tm_hour = value->hour;
  host.tm_mday = value->mday;
  host.tm_mon = value->mon;
  host.tm_year = value->year;
  host.tm_isdst = value->isdst;
  time_t result = mktime(&host);
  if (result != (time_t)-1 && !from_host_time(&host, value, 0))
    return (time_t)-1;
  return result;
}
static int64_t guest_clock(void) {
  clock_t value = clock();
  if (value == (clock_t)-1)
    return -1;
  return (int64_t)value / CLOCKS_PER_SEC * 1000000 +
         (int64_t)value % CLOCKS_PER_SEC * 1000000 / CLOCKS_PER_SEC;
}
static void guest_sincos(double angle, double *sine, double *cosine) {
  *sine = sin(angle);
  *cosine = cos(angle);
}
static void guest_sincosf(float angle, float *sine, float *cosine) {
  *sine = sinf(angle);
  *cosine = cosf(angle);
}
static char abort_message[1024];
static void set_abort_message(const char *message) {
  size_t length = message ? strnlen(message, sizeof(abort_message) - 1) : 0;
  as_cxx_lock();
  if (length)
    memcpy(abort_message, message, length);
  abort_message[length] = 0;
  as_cxx_unlock();
}
const char *as_bionic_abort_message(void) { return abort_message; }
void as_bionic_libc_reset(void) { set_abort_message(NULL); }
static int guest_vasprintf(char **out, const char *format, va_list args) {
  if (!out) {
    errno = EINVAL;
    return -1;
  }
  *out = NULL;
  va_list copy;
  va_copy(copy, args);
  int length = vsnprintf(NULL, 0, format, copy);
  va_end(copy);
  if (length < 0 || length > 1 << 20) {
    errno = EOVERFLOW;
    return -1;
  }
  char *bytes = as_cxx_allocate((size_t)length + 1);
  if (!bytes) {
    errno = ENOMEM;
    return -1;
  }
  int written = vsnprintf(bytes, (size_t)length + 1, format, args);
  if (written != length) {
    as_cxx_deallocate(bytes);
    errno = EIO;
    return -1;
  }
  *out = bytes;
  return written;
}
static int guest_asprintf(char **out, const char *format, ...) {
  va_list args;
  va_start(args, format);
  int result = guest_vasprintf(out, format, args);
  va_end(args);
  return result;
}
static void guest_exit(int status) {
  (void)status;
  as_native_trap(-1205);
}
static void guest_abort(void) { as_native_trap(-1206); }
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
#define SCALAR(f) API(#f, f)
uint64_t as_bionic_libc_import(const char *name) {
  API("__memcpy_chk", memcpy_checked);
  API("__memmove_chk", memmove_checked);
  API("__memset_chk", memset_checked);
  API("__strlen_chk", strlen_checked);
  API("__strcpy_chk", strcpy_checked);
  API("__strncpy_chk", strncpy_checked);
  API("__strncpy_chk2", strncpy_checked2);
  API("__strchr_chk", strchr_checked);
  API("__strncat_chk", strncat_checked);
  API("__vsnprintf_chk", vsnprintf_checked);
  API("__vsprintf_chk", vsprintf_checked);
  API("__sprintf_chk", sprintf_checked);
  API("android_set_abort_message", set_abort_message);
  API("vasprintf", guest_vasprintf);
  API("asprintf", guest_asprintf);
  SCALAR(gettimeofday);
  API("exit", guest_exit);
  API("_exit", guest_exit);
  API("abort", guest_abort);
  SCALAR(memchr);
  SCALAR(strcasecmp);
  SCALAR(strncasecmp);
  SCALAR(strcat);
  SCALAR(strncat);
  SCALAR(strncpy);
  SCALAR(strnlen);
  SCALAR(strchr);
  SCALAR(strrchr);
  SCALAR(strstr);
  SCALAR(strspn);
  SCALAR(strcspn);
  SCALAR(strpbrk);
  SCALAR(atoi);
  SCALAR(atol);
  SCALAR(atoll);
  SCALAR(atof);
  SCALAR(strtol);
  SCALAR(strtoll);
  SCALAR(strtoul);
  SCALAR(strtoull);
  SCALAR(strtod);
  SCALAR(strtof);
  SCALAR(qsort);
  SCALAR(sprintf);
  SCALAR(vsprintf);
  SCALAR(sscanf);
  SCALAR(vsscanf);
  SCALAR(time);
  SCALAR(difftime);
  API("clock", guest_clock);
  API("localtime", guest_localtime);
  API("localtime_r", guest_localtime_r);
  API("gmtime", guest_gmtime);
  API("gmtime_r", guest_gmtime_r);
  API("mktime", guest_mktime);
  SCALAR(acos);
  SCALAR(acosf);
  SCALAR(asin);
  SCALAR(asinf);
  SCALAR(atan);
  SCALAR(atanf);
  SCALAR(atan2);
  SCALAR(atan2f);
  API("sincos", guest_sincos);
  API("sincosf", guest_sincosf);
  SCALAR(sinf);
  SCALAR(cosf);
  SCALAR(tan);
  SCALAR(tanf);
  SCALAR(sinh);
  SCALAR(cosh);
  SCALAR(tanh);
  SCALAR(pow);
  SCALAR(powf);
  SCALAR(exp);
  SCALAR(expf);
  SCALAR(exp2);
  SCALAR(exp2f);
  SCALAR(log);
  SCALAR(logf);
  SCALAR(log2);
  SCALAR(log2f);
  SCALAR(log10);
  SCALAR(log10f);
  SCALAR(fmod);
  SCALAR(fmodf);
  SCALAR(fmin);
  SCALAR(fminf);
  SCALAR(fmax);
  SCALAR(fmaxf);
  SCALAR(floorf);
  SCALAR(ceilf);
  SCALAR(fabs);
  SCALAR(fabsf);
  SCALAR(ldexp);
  SCALAR(ldexpf);
  SCALAR(frexp);
  SCALAR(modf);
  SCALAR(modff);
  SCALAR(hypot);
  SCALAR(hypotf);
  return 0;
}
