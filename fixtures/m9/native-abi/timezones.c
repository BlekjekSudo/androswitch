/* Real libc import-call timezone regression. MIT OR Apache-2.0.
 * Only the formatter metadata is wrapped: calendar calculation and import
 * resolution run the production adapter and the host C library. */
#include "native.h"
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct {
  int sec, min, hour, mday, mon, year, wday, yday, isdst;
  long gmtoff;
  const char *zone;
} AndroidTm;
extern uint64_t as_bionic_libc_import(const char *);
extern size_t __real_strftime(char *, size_t, const char *, const struct tm *);
enum { REAL_METADATA, EMPTY_ZONE, OFFSET_OVERRIDE, PARTIAL_ZONE };
static int mode;
static const char *override_offset;
static unsigned formatter_calls;
size_t __wrap_strftime(char *out, size_t capacity, const char *format,
                       const struct tm *value) {
  ++formatter_calls;
  if (mode == EMPTY_ZONE && !strcmp(format, "%Z")) {
    assert(capacity);
    out[0] = 0;
    return 0;
  }
  if (mode == PARTIAL_ZONE && !strcmp(format, "%Z")) {
    memset(out, 'x', capacity);
    return 0;
  }
  if (mode == OFFSET_OVERRIDE && !strcmp(format, "%z")) {
    size_t length = strlen(override_offset);
    assert(length < capacity);
    memcpy(out, override_offset, length + 1);
    return length;
  }
  return __real_strftime(out, capacity, format, value);
}

/* Other provider contracts are intentionally outside this focused executable.
 * Abort if the time import accidentally delegates into one of them. */
void as_cxx_lock(void) { abort(); }
void as_cxx_unlock(void) { abort(); }
void *as_cxx_allocate(size_t size) { (void)size; abort(); }
void as_cxx_deallocate(void *pointer) { (void)pointer; abort(); }
void as_native_trap(int code) { (void)code; abort(); }
uint64_t as_bionic_locale_import(const char *name) { (void)name; abort(); }
uint64_t as_bionic_multibyte_import(const char *name) { (void)name; abort(); }
uint64_t as_bionic_random_import(const char *name) { (void)name; abort(); }
void as_bionic_random_reset(void) { abort(); }

static void equal_calendar(const AndroidTm *guest, const struct tm *host) {
  assert(guest->sec == host->tm_sec && guest->min == host->tm_min);
  assert(guest->hour == host->tm_hour && guest->mday == host->tm_mday);
  assert(guest->mon == host->tm_mon && guest->year == host->tm_year);
  assert(guest->wday == host->tm_wday && guest->yday == host->tm_yday);
  assert(guest->isdst == host->tm_isdst);
}
static void set_test_timezone(const char *name) {
  assert(!setenv("TZ", name, 1));
  tzset();
}
int main(void) {
  AndroidTm *(*local)(const time_t *) =
      (void *)(uintptr_t)as_bionic_libc_import("localtime");
  AndroidTm *(*local_r)(const time_t *, AndroidTm *) =
      (void *)(uintptr_t)as_bionic_libc_import("localtime_r");
  AndroidTm *(*utc)(const time_t *) =
      (void *)(uintptr_t)as_bionic_libc_import("gmtime");
  AndroidTm *(*utc_r)(const time_t *, AndroidTm *) =
      (void *)(uintptr_t)as_bionic_libc_import("gmtime_r");
  time_t (*make_time)(AndroidTm *) =
      (void *)(uintptr_t)as_bionic_libc_import("mktime");
  assert(local && local_r && utc && utc_r && make_time);
  const time_t epoch = 1792843795; /* Immutable probe's actual calendar input. */
  struct tm expected;
  AndroidTm out;

  set_test_timezone("UTC0");
  mode = REAL_METADATA;
  assert(localtime_r(&epoch, &expected));
  AndroidTm *result = local(&epoch);
  assert(result && result->zone && result->zone[0]);
  equal_calendar(result, &expected);
  assert(result->gmtoff == 0);
  mode = EMPTY_ZONE;
  errno = EBUSY;
  result = local(&epoch);
  assert(result && result->zone && !result->zone[0] && errno == EBUSY);
  equal_calendar(result, &expected);
  assert(result->gmtoff == 0);
  memset(&out, 0x5a, sizeof(out));
  assert(local_r(&epoch, &out) == &out);
  equal_calendar(&out, &expected);
  assert(out.gmtoff == 0 && out.zone && !out.zone[0]);

  /* mktime must retain the actual epoch and normalized calendar with no zone
   * name, including a genuine normalization rather than a canned value. */
  out.min += 60;
  assert(make_time(&out) == epoch + 3600);
  time_t normalized = epoch + 3600;
  assert(localtime_r(&normalized, &expected));
  equal_calendar(&out, &expected);
  assert(out.gmtoff == 0 && out.zone && !out.zone[0]);

  unsigned before_utc = formatter_calls;
  assert(gmtime_r(&epoch, &expected));
  result = utc(&epoch);
  assert(result && result->gmtoff == 0 && !strcmp(result->zone, "GMT"));
  equal_calendar(result, &expected);
  assert(utc_r(&epoch, &out) == &out && out.gmtoff == 0);
  equal_calendar(&out, &expected);
  assert(formatter_calls == before_utc);

  /* Offset and calendar still come from the actual selected host timezone;
   * accepting an empty abbreviation must not silently force UTC. */
  set_test_timezone("NPT-5:45");
  assert(localtime_r(&epoch, &expected));
  assert(local_r(&epoch, &out) == &out);
  equal_calendar(&out, &expected);
  assert(out.gmtoff == 20700 && !out.zone[0]);
  assert(make_time(&out) == epoch);
  set_test_timezone("NST3:30");
  assert(localtime_r(&epoch, &expected));
  assert(local_r(&epoch, &out) == &out);
  equal_calendar(&out, &expected);
  assert(out.gmtoff == -12600 && !out.zone[0]);
  assert(make_time(&out) == epoch);

  static const char *invalid_offsets[] = {
      "", "+000", "+00000", "00000", "=0000", "+x000", "+0x00",
      "+00x0", "+000x", "-x000", "+00:00"};
  set_test_timezone("UTC0");
  mode = OFFSET_OVERRIDE;
  for (size_t i = 0; i < sizeof(invalid_offsets) / sizeof(*invalid_offsets); ++i) {
    override_offset = invalid_offsets[i];
    errno = 0;
    assert(local(&epoch) == NULL && errno == ENOTSUP);
    errno = 0;
    assert(local_r(&epoch, &out) == NULL && errno == ENOTSUP);
    assert(gmtime_r(&epoch, &expected));
    out = (AndroidTm){.sec = expected.tm_sec, .min = expected.tm_min,
                      .hour = expected.tm_hour, .mday = expected.tm_mday,
                      .mon = expected.tm_mon, .year = expected.tm_year,
                      .isdst = expected.tm_isdst};
    errno = 0;
    assert(make_time(&out) == (time_t)-1 && errno == ENOTSUP);
  }
  mode = PARTIAL_ZONE;
  errno = 0;
  assert(local(&epoch) == NULL && errno == ENOTSUP);
  mode = EMPTY_ZONE;
  assert(local(&epoch) && !local(&epoch)->zone[0]);
  puts("native time contracts: empty zone / offset preservation / mktime normalization / UTC / 11 invalid offsets / partial zone passed");
  return 0;
}
