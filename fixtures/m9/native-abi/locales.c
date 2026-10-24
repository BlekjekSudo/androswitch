/* Android locale contracts; no package-specific input. MIT OR Apache-2.0. */
#include "bionic_locale.h"
#include <assert.h>
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static pthread_mutex_t registry = PTHREAD_MUTEX_INITIALIZER;
static int fail_allocation;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&registry)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&registry)); }
void *as_cxx_allocate(size_t n) { return fail_allocation ? NULL : malloc(n); }
void as_cxx_deallocate(void *p) { free(p); }
#define GET(type, name, symbol)                                                \
  __typeof__((type)0) name =                                                   \
      (void *)(uintptr_t)as_bionic_locale_import(symbol);                      \
  assert(name)
static void *(*use_locale)(void *);
static size_t (*mb_max)(void);
static pthread_barrier_t barrier;
typedef struct {
  void *handle;
  size_t expected;
} Worker;
static void *worker(void *opaque) {
  Worker *w = opaque;
  errno = EDOM;
  assert(use_locale(w->handle) == AS_LC_GLOBAL && errno == EDOM);
  assert(mb_max() == w->expected);
  int rc = pthread_barrier_wait(&barrier);
  assert(!rc || rc == PTHREAD_BARRIER_SERIAL_THREAD);
  rc = pthread_barrier_wait(&barrier);
  assert(!rc || rc == PTHREAD_BARRIER_SERIAL_THREAD);
  assert(mb_max() == w->expected);
  as_bionic_locale_thread_finalize();
  assert(use_locale(NULL) == AS_LC_GLOBAL && mb_max() == 4);
  return NULL;
}
int main(void) {
  GET(void *(*)(int, const char *, void *), create, "newlocale");
  GET(void *(*)(void *), duplicate, "duplocale");
  GET(void (*)(void *), release, "freelocale");
  GET(char *(*)(int, const char *), set, "setlocale");
  GET(AsGuestLconv * (*)(void), conventions, "localeconv");
  use_locale = (void *)as_bionic_locale_import("uselocale");
  mb_max = (void *)as_bionic_locale_import("__ctype_get_mb_cur_max");
  assert(use_locale && mb_max &&
         !as_bionic_locale_import("unsupported_locale_api"));
  assert(use_locale(NULL) == AS_LC_GLOBAL && mb_max() == 4);
  errno = E2BIG;
  void *c = create(AS_LC_ALL_MASK, "C", NULL);
  void *utf8 = create(AS_LC_ALL_MASK, "C.UTF-8", NULL);
  assert(c && utf8 && c != utf8 && errno == E2BIG);
  void *posix = create(0, "POSIX", c);
  void *alias = create(1, "en_US.UTF-8", NULL);
  void *empty = create(AS_LC_ALL_MASK, "", NULL);
  assert(posix && alias && empty);
  assert(use_locale(c) == AS_LC_GLOBAL && mb_max() == 1);
  assert(use_locale(utf8) == c && mb_max() == 4);
  assert(use_locale(alias) == utf8 && mb_max() == 4);
  assert(use_locale(posix) == alias && mb_max() == 1);
  /* API26 Bionic's whole-locale/base behavior is preserved. */
  void *with_base = create(0, "C.UTF-8", c);
  assert(with_base && with_base != c);
  assert(use_locale(with_base) == posix && mb_max() == 4);
  assert(use_locale(c) == with_base && mb_max() == 1);
  assert(use_locale(AS_LC_GLOBAL) == c);
  release(with_base);
  release(posix);
  release(alias);
  release(empty);
  errno = 0;
  assert(!create(1 << 6, "C", NULL) && errno == EINVAL);
  errno = 0;
  assert(!create(-1, "C", NULL) && errno == EINVAL);
  errno = 0;
  assert(!create(1, NULL, NULL) && errno == EINVAL);
  errno = 0;
  assert(!create(1, "de_DE.UTF-8", NULL) && errno == ENOENT);
  errno = 0;
  assert(!create(1, "C.utf8", NULL) && errno == ENOENT);
  errno = 0;
  assert(!create(1, "C", AS_LC_GLOBAL) && errno == EINVAL);
  errno = 0;
  assert(!use_locale((void *)1234) && errno == EINVAL);
  errno = 0;
  assert(!duplicate(NULL) && errno == EINVAL);
  assert(use_locale(NULL) == AS_LC_GLOBAL);
  fail_allocation = 1;
  errno = 0;
  assert(!create(1, "C", c) && errno == ENOMEM);
  errno = 0;
  assert(!duplicate(c) && errno == ENOMEM);
  fail_allocation = 0;
  assert(!strcmp(set(AS_LC_ALL, "C"), "C") && mb_max() == 1);
  void *snapshot = duplicate(AS_LC_GLOBAL);
  assert(snapshot);
  assert(!strcmp(set(AS_LC_ALL, ""), "C.UTF-8") && mb_max() == 4);
  assert(use_locale(snapshot) == AS_LC_GLOBAL && mb_max() == 1);
  assert(use_locale(AS_LC_GLOBAL) == snapshot);
  release(snapshot);
  errno = 0;
  assert(!set(13, "C") && errno == EINVAL);
  errno = 0;
  assert(!set(0, "fr_FR") && errno == ENOENT);
  assert(!strcmp(set(0, NULL), "C.UTF-8"));
  AsGuestLconv *lc = conventions();
  assert(!strcmp(lc->decimal_point, ".") && !*lc->thousands_sep &&
         !*lc->grouping);
  const unsigned char *markers = &lc->int_frac_digits;
  for (size_t i = 0; i < 14; i++)
    assert(markers[i] == 255);
  GET(long long (*)(const char *, char **, int, void *), integer, "strtoll_l");
  GET(unsigned long long (*)(const char *, char **, int, void *), uinteger,
      "strtoull_l");
  GET(double (*)(const char *, char **, void *), number, "strtod_l");
  GET(long double (*)(const char *, char **, void *), wide_number, "strtold_l");
  char *end;
  errno = E2BIG;
  assert(integer(" -0x2a!", &end, 0, c) == -42 && *end == '!' &&
         errno == E2BIG);
  errno = 0;
  assert(integer("999999999999999999999999999", &end, 10, c) == LLONG_MAX &&
         errno == ERANGE);
  assert(uinteger("18446744073709551615!", &end, 10, utf8) == ULLONG_MAX &&
         *end == '!');
  assert(number("1.25tail", &end, c) == 1.25 && !strcmp(end, "tail"));
  assert(number("1,25", &end, utf8) == 1 && *end == ',');
  assert(wide_number("0x1.8p+2!", &end, c) == 6.0L && *end == '!');
  GET(int (*)(int, void *), alpha, "isalpha_l");
  GET(int (*)(int, void *), lower, "tolower_l");
  GET(int (*)(uint32_t, void *), walpha, "iswalpha_l");
  GET(uint32_t (*)(uint32_t, void *), wlower, "towlower_l");
  assert(alpha('A', c) && !alpha(0xe9, utf8) && !alpha(-1, c));
  assert(lower('Q', utf8) == 'q' && lower(-1, c) == -1);
  assert(walpha(0xe9, c) && walpha(0x391, utf8));
  assert(wlower(0xc9, c) == 0xe9 && wlower(0x391, utf8) == 0x3b1 &&
         wlower(UINT32_MAX, c) == UINT32_MAX);
  GET(int (*)(const char *, const char *, void *), collate, "strcoll_l");
  GET(size_t (*)(char *, const char *, size_t, void *), transform, "strxfrm_l");
  GET(int (*)(const uint32_t *, const uint32_t *, void *), wcollate,
      "wcscoll_l");
  GET(size_t (*)(uint32_t *, const uint32_t *, size_t, void *), wtransform,
      "wcsxfrm_l");
  char out[128];
  memset(out, 0x5a, sizeof(out));
  assert(collate("Z", "a", utf8) < 0 && transform(NULL, "abcd", 0, c) == 4);
  assert(transform(out, "abcd", 3, c) == 4 && !memcmp(out, "abc", 3) &&
         out[3] == 0x5a);
  uint32_t wa[] = {0xe9, 0}, wb[] = {0x100, 0}, wo[3] = {0, 0, 0xabcdef};
  assert(wcollate(wa, wb, c) < 0 && wtransform(wo, wa, 3, utf8) == 1 &&
         wo[0] == 0xe9 && wo[1] == 0 && wo[2] == 0xabcdef);
  GET(size_t (*)(char *, size_t, const char *, const AsLocaleTm *, void *),
      format, "strftime_l");
  AsLocaleTm tm = {.sec = 5,
                   .min = 4,
                   .hour = 3,
                   .mday = 24,
                   .mon = 9,
                   .year = 126,
                   .wday = 6,
                   .yday = 296,
                   .gmtoff = 19800,
                   .zone = "IST"};
  assert(format(out, sizeof(out), "%Y-%m-%d %H:%M:%S %z %Z %%", &tm, c));
  assert(!strcmp(out, "2026-10-24 03:04:05 +0530 IST %"));
  tm.zone = "E%U";
  tm.gmtoff = -18000;
  assert(format(out, sizeof(out), "%z %Z", &tm, utf8) &&
         !strcmp(out, "-0500 E%U"));
  errno = 0;
  assert(!format(out, sizeof(out), "%s", &tm, c) && errno == ENOTSUP);
  fail_allocation = 1;
  errno = 0;
  assert(!format(out, sizeof(out), "%Y", &tm, c) && errno == ENOMEM);
  fail_allocation = 0;
  assert(!pthread_barrier_init(&barrier, NULL, 3));
  Worker first = {c, 1}, second = {utf8, 4};
  pthread_t a, b;
  assert(!pthread_create(&a, NULL, worker, &first) &&
         !pthread_create(&b, NULL, worker, &second));
  int rc = pthread_barrier_wait(&barrier);
  assert(!rc || rc == PTHREAD_BARRIER_SERIAL_THREAD);
  errno = 0;
  release(c);
  assert(errno == EBUSY);
  assert(as_bionic_locale_reset() == -EBUSY && mb_max() == 4);
  rc = pthread_barrier_wait(&barrier);
  assert(!rc || rc == PTHREAD_BARRIER_SERIAL_THREAD);
  assert(!pthread_join(a, NULL) && !pthread_join(b, NULL));
  assert(!pthread_barrier_destroy(&barrier));
  errno = E2BIG;
  release(c);
  release(utf8);
  assert(errno == E2BIG);
  assert(!as_bionic_locale_reset() && mb_max() == 4);
  puts("native locale contracts: PASS (ABI, names/masks, ownership/TLS, "
       "numeric, Unicode, collation, time)");
  return 0;
}
