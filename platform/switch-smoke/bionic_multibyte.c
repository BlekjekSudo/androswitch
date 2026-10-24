/* Explicit UTF-8 conversion for Android's UTF-32 wchar_t / LP64 mbstate_t.
 * No host locale or host mbstate_t crosses this boundary. The import bridge
 * restores host TLS before entry, so implicit states belong to the real thread.
 * Like API26 Bionic, UTF-8 conversion is independent of MB_CUR_MAX in C locale.
 * Original implementation, MIT OR Apache-2.0. ABI/reference behavior:
 * AOSP android-8.0.0_r1 libc/{bionic/wchar.cpp,bionic/mbrtoc32.cpp,
 * bionic/c32rtomb.cpp,include/bits/mbstate_t.h}.
 * Historical non-scalar UTF-8 acceptance and bounded-conversion bugs are not
 * reproduced; strict Unicode scalars and restartable partial-input semantics
 * are deliberate. This is not byte-for-byte parity with every API26 edge bug.
 */
#include "bionic_multibyte.h"
#include <errno.h>
#include <string.h>
_Static_assert(sizeof(AsGuestMbstate) == 8, "Android ARM64 mbstate_t");
_Static_assert(offsetof(AsGuestMbstate, reserved) == 4, "reserved state ABI");
#define ILLEGAL ((size_t)-1)
#define INCOMPLETE ((size_t)-2)
enum {
  READ_WIDE,
  READ_LENGTH,
  READ_LEGACY,
  READ_STRING,
  WRITE_WIDE,
  WRITE_LEGACY,
  WRITE_STRING,
  READ_C32,
  WRITE_C32,
  STATE_COUNT
};
static __thread AsGuestMbstate implicit[STATE_COUNT];
void as_bionic_multibyte_thread_finalize(void) {
  memset(implicit, 0, sizeof(implicit));
}
static void clear(AsGuestMbstate *state) {
  memset(state->sequence, 0, sizeof(state->sequence));
}
static int guest_mbsinit(const AsGuestMbstate *state) {
  return !state || !(state->sequence[0] | state->sequence[1] |
                     state->sequence[2] | state->sequence[3]);
}
static size_t error(AsGuestMbstate *state, int number) {
  clear(state);
  errno = number;
  return ILLEGAL;
}
static unsigned width(unsigned char first) {
  if (first < 0x80)
    return 1;
  if (first >= 0xc2 && first <= 0xdf)
    return 2;
  if (first >= 0xe0 && first <= 0xef)
    return 3;
  if (first >= 0xf0 && first <= 0xf4)
    return 4;
  return 0;
}
static int scalar(uint32_t value) {
  return value <= 0x10ffff && !(value >= 0xd800 && value <= 0xdfff);
}
static size_t decode(uint32_t *out, const char *input, size_t limit,
                     AsGuestMbstate *state) {
  unsigned have = 0;
  while (have < 4 && state->sequence[have])
    ++have;
  for (unsigned i = have; i < 4; ++i)
    if (state->sequence[i])
      return error(state, EINVAL);
  if (have == 4)
    return error(state, EINVAL);
  if (!input) {
    input = "";
    limit = 1;
    out = NULL;
  }
  if (!limit)
    return INCOMPLETE;
  unsigned expected = width(have ? state->sequence[0] : (unsigned char)*input);
  if (!expected)
    return error(state, EILSEQ);
  if (have >= expected)
    return error(state, EINVAL);
  for (unsigned i = 1; i < have; ++i)
    if ((state->sequence[i] & 0xc0) != 0x80)
      return error(state, EINVAL);
  size_t used = 0;
  while (have < expected && used < limit) {
    unsigned char byte = (unsigned char)input[used++];
    if (have && (byte & 0xc0) != 0x80)
      return error(state, EILSEQ);
    state->sequence[have++] = byte;
  }
  if (have != expected)
    return INCOMPLETE;
  static const unsigned char masks[] = {0, 0x7f, 0x1f, 0x0f, 0x07};
  static const uint32_t minimum[] = {0, 0, 0x80, 0x800, 0x10000};
  uint32_t value = state->sequence[0] & masks[expected];
  for (unsigned i = 1; i < expected; ++i)
    value = (value << 6) | (state->sequence[i] & 0x3f);
  if (value < minimum[expected] || !scalar(value) || value == 0xfffe ||
      value == 0xffff)
    return error(state, EILSEQ);
  clear(state);
  if (out)
    *out = value;
  return value ? used : 0;
}
static size_t encode(char *out, uint32_t value, AsGuestMbstate *state) {
  if (!out) {
    clear(state);
    return 1;
  }
  if (!value) {
    clear(state);
    *out = 0;
    return 1;
  }
  if (!guest_mbsinit(state) || !scalar(value))
    return error(state, EILSEQ);
  unsigned count = value < 0x80      ? 1
                   : value < 0x800   ? 2
                   : value < 0x10000 ? 3
                                     : 4;
  static const unsigned char leads[] = {0, 0, 0xc0, 0xe0, 0xf0};
  for (unsigned i = count - 1; i; --i) {
    out[i] = (char)(0x80 | (value & 0x3f));
    value >>= 6;
  }
  out[0] = (char)(leads[count] | value);
  return count;
}
static size_t guest_mbrtowc(uint32_t *out, const char *input, size_t limit,
                            AsGuestMbstate *state) {
  return decode(out, input, limit, state ? state : &implicit[READ_WIDE]);
}
static size_t guest_mbrtoc32(uint32_t *out, const char *input, size_t limit,
                             AsGuestMbstate *state) {
  return decode(out, input, limit, state ? state : &implicit[READ_C32]);
}
static size_t guest_mbrlen(const char *input, size_t limit,
                           AsGuestMbstate *state) {
  return decode(NULL, input, limit, state ? state : &implicit[READ_LENGTH]);
}
static int guest_mbtowc(uint32_t *out, const char *input, size_t limit) {
  AsGuestMbstate *state = &implicit[READ_LEGACY];
  if (!input) {
    clear(state);
    return 0;
  }
  size_t count = decode(out, input, limit, state);
  return count == ILLEGAL || count == INCOMPLETE ? -1 : (int)count;
}
static int guest_mblen(const char *input, size_t limit) {
  AsGuestMbstate state = {0};
  if (!input)
    return 0;
  size_t count = decode(NULL, input, limit, &state);
  return count == ILLEGAL || count == INCOMPLETE ? -1 : (int)count;
}
static size_t guest_wcrtomb(char *out, uint32_t value, AsGuestMbstate *state) {
  return encode(out, value, state ? state : &implicit[WRITE_WIDE]);
}
static size_t guest_c32rtomb(char *out, uint32_t value, AsGuestMbstate *state) {
  return encode(out, value, state ? state : &implicit[WRITE_C32]);
}
static int guest_wctomb(char *out, uint32_t value) {
  AsGuestMbstate *state = &implicit[WRITE_LEGACY];
  if (!out) {
    clear(state);
    return 0;
  }
  size_t count = encode(out, value, state);
  return count == ILLEGAL ? -1 : (int)count;
}
static uint32_t guest_btowc(int byte) {
  if (byte == -1)
    return UINT32_MAX;
  char value = (char)(unsigned char)byte;
  uint32_t wide;
  AsGuestMbstate state = {0};
  size_t result = decode(&wide, &value, 1, &state);
  return result > 1 ? UINT32_MAX : wide;
}
static int guest_wctob(uint32_t value) {
  if (value == UINT32_MAX)
    return -1;
  AsGuestMbstate state = {0};
  char encoded[4];
  return encode(encoded, value, &state) == 1 ? (unsigned char)encoded[0] : -1;
}
static size_t guest_mbsnrtowcs(uint32_t *out, const char **source, size_t bytes,
                               size_t capacity, AsGuestMbstate *provided) {
  AsGuestMbstate *state = provided ? provided : &implicit[READ_STRING];
  AsGuestMbstate measured;
  if (!source || !*source) {
    errno = EINVAL;
    return ILLEGAL;
  }
  if (!out) {
    measured = *state;
    state = &measured;
  }
  const char *input = *source;
  size_t used = 0, count = 0;
  while (used < bytes && (!out || count < capacity)) {
    uint32_t value;
    size_t result = decode(&value, input + used, bytes - used, state);
    if (result == ILLEGAL) {
      if (out)
        *source = input + used;
      return ILLEGAL;
    }
    if (result == INCOMPLETE) {
      if (out)
        *source = input + bytes;
      return count;
    }
    if (out)
      out[count] = value;
    if (!result) {
      if (out)
        *source = NULL;
      return count;
    }
    used += result;
    ++count;
  }
  if (out)
    *source = input + used;
  return count;
}
static size_t guest_mbsrtowcs(uint32_t *out, const char **source,
                              size_t capacity, AsGuestMbstate *state) {
  return guest_mbsnrtowcs(out, source, SIZE_MAX, capacity, state);
}
static size_t guest_wcsnrtombs(char *out, const uint32_t **source,
                               size_t values, size_t capacity,
                               AsGuestMbstate *provided) {
  AsGuestMbstate *state = provided ? provided : &implicit[WRITE_STRING];
  AsGuestMbstate measured;
  if (!source || !*source) {
    errno = EINVAL;
    return ILLEGAL;
  }
  if (!out) {
    measured = *state;
    state = &measured;
  }
  if (!guest_mbsinit(state))
    return error(state, EILSEQ);
  const uint32_t *input = *source;
  size_t consumed = 0, bytes = 0;
  while (consumed < values && (!out || bytes < capacity)) {
    char encoded[4];
    size_t result = encode(encoded, input[consumed], state);
    if (result == ILLEGAL) {
      if (out)
        *source = input + consumed;
      return ILLEGAL;
    }
    if (out && result > capacity - bytes)
      break;
    if (bytes > SIZE_MAX - result) {
      errno = EOVERFLOW;
      return ILLEGAL;
    }
    if (out)
      memcpy(out + bytes, encoded, result);
    if (!input[consumed]) {
      if (out)
        *source = NULL;
      return bytes;
    }
    bytes += result;
    ++consumed;
  }
  if (out)
    *source = input + consumed;
  return bytes;
}
static size_t guest_wcsrtombs(char *out, const uint32_t **source,
                              size_t capacity, AsGuestMbstate *state) {
  return guest_wcsnrtombs(out, source, SIZE_MAX, capacity, state);
}
static size_t guest_mbstowcs(uint32_t *out, const char *input,
                             size_t capacity) {
  AsGuestMbstate state = {0};
  return guest_mbsrtowcs(out, &input, capacity, &state);
}
static size_t guest_wcstombs(char *out, const uint32_t *input,
                             size_t capacity) {
  AsGuestMbstate state = {0};
  return guest_wcsrtombs(out, &input, capacity, &state);
}
#define API(name)                                                              \
  if (!strcmp(symbol, #name))                                                  \
  return (uint64_t)guest_##name
uint64_t as_bionic_multibyte_import(const char *symbol) {
  API(mbsinit);
  API(mbrtowc);
  API(mbrtoc32);
  API(mbrlen);
  API(mbtowc);
  API(mblen);
  API(wcrtomb);
  API(c32rtomb);
  API(wctomb);
  API(btowc);
  API(wctob);
  API(mbsnrtowcs);
  API(mbsrtowcs);
  API(wcsnrtombs);
  API(wcsrtombs);
  API(mbstowcs);
  API(wcstombs);
  return 0;
}
