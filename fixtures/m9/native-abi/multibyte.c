/* General Android multibyte contracts, MIT OR Apache-2.0. */
#include "bionic_locale.h"
#include "bionic_multibyte.h"
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static pthread_mutex_t registry = PTHREAD_MUTEX_INITIALIZER;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&registry)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&registry)); }
void *as_cxx_allocate(size_t n) { return malloc(n); }
void as_cxx_deallocate(void *p) { free(p); }
#define GET(type, name, symbol)                                                \
  __typeof__((type)0) name =                                                   \
      (void *)(uintptr_t)as_bionic_multibyte_import(symbol);                   \
  assert(name)
#define BAD ((size_t)-1)
#define PART ((size_t)-2)
static size_t (*read_wide)(uint32_t *, const char *, size_t, AsGuestMbstate *);
static pthread_barrier_t barrier;
static void wait_barrier(void) {
  int result = pthread_barrier_wait(&barrier);
  assert(!result || result == PTHREAD_BARRIER_SERIAL_THREAD);
}
static void *worker(void *opaque) {
  unsigned id = (unsigned)(uintptr_t)opaque;
  const char *lead = id ? "\xe2\x82" : "\xc3";
  const char *tail = id ? "\xac" : "\xa9";
  uint32_t output = 0;
  errno = EDOM;
  assert(read_wide(&output, lead, id ? 2 : 1, NULL) == PART);
  wait_barrier();
  assert(read_wide(&output, tail, 1, NULL) == 1);
  assert(output == (id ? 0x20ac : 0xe9) && errno == EDOM);
  /* A new session/thread cleanup must not inherit a partial decoder. */
  assert(read_wide(NULL, "\xf0\x9f", 2, NULL) == PART);
  as_bionic_locale_thread_finalize();
  assert(read_wide(&output, "A", 1, NULL) == 1 && output == 'A');
  return NULL;
}
int main(void) {
  GET(int (*)(const AsGuestMbstate *), initial, "mbsinit");
  GET(size_t (*)(uint32_t *, const char *, size_t, AsGuestMbstate *), read32,
      "mbrtoc32");
  GET(size_t (*)(const char *, size_t, AsGuestMbstate *), length, "mbrlen");
  GET(int (*)(uint32_t *, const char *, size_t), legacy, "mbtowc");
  GET(int (*)(const char *, size_t), legacy_length, "mblen");
  GET(size_t (*)(char *, uint32_t, AsGuestMbstate *), write, "wcrtomb");
  GET(size_t (*)(char *, uint32_t, AsGuestMbstate *), write32, "c32rtomb");
  GET(int (*)(char *, uint32_t), legacy_write, "wctomb");
  GET(uint32_t (*)(int), one_wide, "btowc");
  GET(int (*)(uint32_t), one_byte, "wctob");
  GET(size_t (*)(uint32_t *, const char **, size_t, size_t, AsGuestMbstate *),
      read_string, "mbsnrtowcs");
  GET(size_t (*)(uint32_t *, const char **, size_t, AsGuestMbstate *),
      read_terminated, "mbsrtowcs");
  GET(size_t (*)(char *, const uint32_t **, size_t, size_t, AsGuestMbstate *),
      write_string, "wcsnrtombs");
  GET(size_t (*)(char *, const uint32_t **, size_t, AsGuestMbstate *),
      write_terminated, "wcsrtombs");
  GET(size_t (*)(uint32_t *, const char *, size_t), read_legacy_string,
      "mbstowcs");
  GET(size_t (*)(char *, const uint32_t *, size_t), write_legacy_string,
      "wcstombs");
  read_wide = (void *)as_bionic_multibyte_import("mbrtowc");
  assert(read_wide && !as_bionic_multibyte_import("mbrtoc16"));
  AsGuestMbstate state = {{0}, {0xa1, 0xb2, 0xc3, 0xd4}};
  uint32_t output = 0xabcdef;
  assert(sizeof(state) == 8 && initial(&state) && initial(NULL));
  errno = EDOM;
  assert(read_wide(&output, "Q", 1, &state) == 1 && output == 'Q');
  assert(read_wide(&output, "", 1, &state) == 0 && output == 0);
  output = 0xabcdef;
  assert(read_wide(&output, NULL, 0, &state) == 0 && output == 0xabcdef);
  assert(read_wide(&output, "", 0, &state) == PART && initial(&state));
  const char *examples[] = {"\xc2\x80", "\xc3\xa9", "\xe2\x82\xac",
                            "\xf0\x9f\x98\x80", "\xf4\x8f\xbf\xbf"};
  const uint32_t points[] = {0x80, 0xe9, 0x20ac, 0x1f600, 0x10ffff};
  for (unsigned i = 0; i < sizeof(points) / sizeof(points[0]); ++i) {
    size_t bytes = strlen(examples[i]);
    assert(read32(&output, examples[i], bytes, &state) == bytes &&
           output == points[i] && initial(&state));
    for (size_t split = 1; split < bytes; ++split) {
      assert(read_wide(NULL, examples[i], split, &state) == PART &&
             !initial(&state));
      assert(read_wide(&output, examples[i] + split, bytes - split, &state) ==
                 bytes - split &&
             output == points[i] && initial(&state));
    }
    char encoded[5] = {0x55, 0x55, 0x55, 0x55, 0x55};
    assert(write32(encoded, points[i], &state) == bytes &&
           !memcmp(encoded, examples[i], bytes) && encoded[bytes] == 0x55);
  }
  assert(errno == EDOM && !memcmp(state.reserved, "\xa1\xb2\xc3\xd4", 4));
  const char *bad[] = {"\x80",
                       "\xc0\x80",
                       "\xe0\x80\x80",
                       "\xed\xa0\x80",
                       "\xf4\x90\x80\x80",
                       "\xf5\x80\x80\x80",
                       "\xe2"
                       "A",
                       "\xef\xbf\xbe",
                       "\xef\xbf\xbf"};
  for (unsigned i = 0; i < sizeof(bad) / sizeof(bad[0]); ++i) {
    errno = 0;
    output = 0xabcdef;
    assert(read_wide(&output, bad[i], strlen(bad[i]), &state) == BAD &&
           errno == EILSEQ && output == 0xabcdef && initial(&state));
  }
  state.sequence[3] = 1;
  errno = 0;
  assert(read_wide(NULL, "A", 1, &state) == BAD && errno == EINVAL &&
         initial(&state));
  assert(read_wide(NULL, "\xe2", 1, &state) == PART);
  errno = 0;
  assert(read_wide(NULL, NULL, 0, &state) == BAD && errno == EILSEQ &&
         initial(&state));
  errno = EDOM;
  assert(length("\xc3", 1, NULL) == PART);
  assert(length("\xa9", 1, NULL) == 1 && errno == EDOM);
  assert(legacy(NULL, NULL, 0) == 0);
  assert(legacy(&output, "\xe2\x82", 2) == -1 && errno == EDOM);
  assert(legacy(&output, "\xac", 1) == 1 && output == 0x20ac);
  assert(legacy_length("\xc3", 1) == -1);
  assert(legacy_length("A", 1) == 1 && legacy_length(NULL, 5) == 0);
  char bytes[32];
  memset(bytes, 0x55, sizeof(bytes));
  assert(write(bytes, 0, &state) == 1 && bytes[0] == 0);
  state.sequence[0] = 0xc3;
  assert(write(NULL, 123, &state) == 1 && initial(&state));
  assert(legacy_write(NULL, 12) == 0 && legacy_write(bytes, 0x20ac) == 3);
  const uint32_t invalid_points[] = {0xd800, 0xdfff, 0x110000, UINT32_MAX};
  for (unsigned i = 0; i < sizeof(invalid_points) / sizeof(invalid_points[0]);
       ++i) {
    errno = 0;
    bytes[0] = 0x55;
    assert(write(bytes, invalid_points[i], &state) == BAD && errno == EILSEQ &&
           bytes[0] == 0x55 && initial(&state));
  }
  assert(one_wide(-1) == UINT32_MAX && one_wide('A') == 'A' &&
         one_wide(0xc3) == UINT32_MAX && one_byte(0xe9) == -1 &&
         one_byte('A') == 'A');
  const char text[] = "A\xc3\xa9\xf0\x9f\x98\x80Z";
  const char *source = text;
  uint32_t wide[8];
  for (unsigned i = 0; i < 8; ++i)
    wide[i] = 0xabcdef;
  errno = EDOM;
  assert(read_terminated(NULL, &source, 0, &state) == 4 && source == text &&
         initial(&state) && errno == EDOM);
  assert(read_terminated(wide, &source, 2, &state) == 2 && source == text + 3 &&
         wide[0] == 'A' && wide[1] == 0xe9 && wide[2] == 0xabcdef);
  assert(read_terminated(wide + 2, &source, 6, &state) == 2 && !source &&
         wide[2] == 0x1f600 && wide[3] == 'Z' && wide[4] == 0);
  source = text;
  assert(read_string(wide, &source, 2, 8, &state) == 1 && source == text + 2 &&
         !initial(&state));
  assert(read_string(wide + 1, &source, 1, 7, &state) == 1 &&
         source == text + 3 && wide[1] == 0xe9 && initial(&state));
  const char broken[] = "A\xe2\x80Z";
  source = broken;
  errno = 0;
  assert(read_terminated(wide, &source, 8, &state) == BAD &&
         source == broken + 1 && errno == EILSEQ && initial(&state));
  source = text;
  assert(read_string(wide, &source, 0, 8, &state) == 0 && source == text);
  assert(read_string(wide, &source, 8, 0, &state) == 0 && source == text);
  const uint32_t input[] = {'A', 0xe9, 0x1f600, 'Z', 0};
  const uint32_t *ws = input;
  assert(write_terminated(NULL, &ws, 0, &state) == 8 && ws == input);
  memset(bytes, 0x55, sizeof(bytes));
  assert(write_string(bytes, &ws, 5, 2, &state) == 1 && ws == input + 1 &&
         bytes[0] == 'A' && bytes[1] == 0x55);
  assert(write_terminated(bytes + 1, &ws, sizeof(bytes) - 1, &state) == 7 &&
         !ws && !strcmp(bytes, text) && bytes[9] == 0x55);
  const uint32_t invalid_string[] = {'A', 0xd800, 0};
  ws = invalid_string;
  errno = 0;
  assert(write_terminated(bytes, &ws, sizeof(bytes), &state) == BAD &&
         ws == invalid_string + 1 && errno == EILSEQ);
  assert(read_legacy_string(wide, text, 8) == 4 && wide[2] == 0x1f600 &&
         wide[4] == 0);
  assert(write_legacy_string(bytes, input, sizeof(bytes)) == 8 &&
         !strcmp(bytes, text));
  /* Android performs UTF-8 conversion even while C reports MB_CUR_MAX=1. */
  char *(*set)(int, const char *) =
      (void *)as_bionic_locale_import("setlocale");
  size_t (*maximum)(void) =
      (void *)as_bionic_locale_import("__ctype_get_mb_cur_max");
  assert(set && maximum && set(AS_LC_ALL, "C") && maximum() == 1);
  assert(legacy(&output, "\xc3\xa9", 2) == 2 && output == 0xe9);
  assert(set(AS_LC_ALL, "C.UTF-8") && maximum() == 4);
  assert(!pthread_barrier_init(&barrier, NULL, 2));
  pthread_t a, b;
  assert(!pthread_create(&a, NULL, worker, (void *)0) &&
         !pthread_create(&b, NULL, worker, (void *)1));
  assert(!pthread_join(a, NULL) && !pthread_join(b, NULL));
  assert(!pthread_barrier_destroy(&barrier));
  assert(!as_bionic_locale_reset());
  puts("native multibyte contracts: PASS (UTF-8/UTF-32, LP64 state, restart, "
       "strings, TLS, errors)");
  return 0;
}
