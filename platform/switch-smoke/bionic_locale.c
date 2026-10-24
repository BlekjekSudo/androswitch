/* Private Android locale handles and C/UTF-8 services. No guest handle is
 * passed to the host libc. The native import bridge restores host TLS before
 * these functions run. Original AndroSwitch code: MIT OR Apache-2.0.
 * Behavior reference: AOSP android-8.0.0_r1
 * libc/bionic/{locale,ctype,wctype}.cpp.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include "bionic_locale.h"
#include "bionic_cxx.h"
#include "bionic_errno.h"
#include "bionic_multibyte.h"
#include <ctype.h>
#include <errno.h>
#include <float.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wchar.h>
#include <wctype.h>
_Static_assert(sizeof(AsGuestLconv) == 96, "Android LP64 lconv");
_Static_assert(offsetof(AsGuestLconv, int_n_sign_posn) == 93,
               "lconv field order");
_Static_assert(offsetof(AsLocaleTm, gmtoff) == 40, "Android LP64 tm");
#ifdef __SWITCH__
_Static_assert(sizeof(long double) == 16 && LDBL_MANT_DIG == 113,
               "Android ARM64 long double ABI");
#endif
typedef struct Locale {
  struct Locale *next;
  unsigned users;
  int utf8;
} Locale;
static Locale *locales;
static __thread void *selected;
static int global_utf8 = 1;
static locale_t host_locales[2];
static AsGuestLconv conventions = {".", "",  "",  "",  "",  "",  "",  "",
                                   "",  "",  255, 255, 255, 255, 255, 255,
                                   255, 255, 255, 255, 255, 255, 255, 255};
static Locale *find_locale(void *handle) {
  for (Locale *p = locales; p; p = p->next)
    if (p == handle)
      return p;
  return NULL;
}
static int locale_kind(void *handle, int *utf8) {
  as_cxx_lock();
  Locale *p = find_locale(handle);
  int valid = handle == AS_LC_GLOBAL || p;
  if (valid)
    *utf8 = p ? p->utf8 : global_utf8;
  as_cxx_unlock();
  if (!valid)
    errno = EINVAL;
  return valid;
}
static locale_t host_locale(int utf8) {
  int saved = errno;
  as_cxx_lock();
  if (!host_locales[utf8])
    host_locales[utf8] = newlocale(LC_ALL_MASK, utf8 ? "C.UTF-8" : "C", NULL);
  locale_t value = host_locales[utf8];
  as_cxx_unlock();
  if (value)
    errno = saved;
  return value;
}
static int supported(const char *name, int *utf8) {
  if (!strcmp(name, "C") || !strcmp(name, "POSIX")) {
    *utf8 = 0;
    return 1;
  }
  if (!*name || !strcmp(name, "C.UTF-8") || !strcmp(name, "en_US.UTF-8")) {
    *utf8 = 1;
    return 1;
  }
  errno = ENOENT;
  return 0;
}
static void *allocate_locale(int utf8) {
  int saved = errno;
  if (!host_locale(utf8))
    return NULL;
  Locale *p = as_cxx_allocate(sizeof(*p));
  if (!p) {
    errno = ENOMEM;
    return NULL;
  }
  *p = (Locale){.utf8 = utf8};
  as_cxx_lock();
  p->next = locales;
  locales = p;
  as_cxx_unlock();
  errno = saved;
  return p;
}
static void *guest_newlocale(int mask, const char *name, void *base) {
  int unused, utf8;
  if ((mask & ~AS_LC_ALL_MASK) || !name || base == AS_LC_GLOBAL) {
    errno = EINVAL;
    return NULL;
  }
  if (base && !locale_kind(base, &unused))
    return NULL;
  if (!supported(name, &utf8))
    return NULL;
  /* API26 Bionic ignores base and category selection after validating mask.
   * It returns a distinct handle; base remains owned by its caller. */
  return allocate_locale(utf8);
}
static void *guest_duplocale(void *handle) {
  int utf8;
  return locale_kind(handle, &utf8) ? allocate_locale(utf8) : NULL;
}
static void *guest_uselocale(void *handle) {
  as_cxx_lock();
  Locale *next = find_locale(handle);
  if (handle && handle != AS_LC_GLOBAL && !next) {
    as_cxx_unlock();
    errno = EINVAL;
    return NULL;
  }
  void *old = selected ? selected : AS_LC_GLOBAL;
  if (handle) {
    Locale *before = find_locale(selected);
    if (before)
      --before->users;
    if (next)
      ++next->users;
    selected = handle;
  }
  as_cxx_unlock();
  return old;
}
static void guest_freelocale(void *handle) {
  int saved = errno;
  as_cxx_lock();
  Locale **link = &locales;
  while (*link && *link != handle)
    link = &(*link)->next;
  Locale *p = *link;
  if (!p || p->users) {
    as_cxx_unlock();
    errno = p ? EBUSY : EINVAL;
    return;
  }
  *link = p->next;
  as_cxx_unlock();
  as_cxx_deallocate(p);
  errno = saved;
}
static char *guest_setlocale(int category, const char *name) {
  int utf8;
  if (category < 0 || category > 12) {
    errno = EINVAL;
    return NULL;
  }
  if (name) {
    if (!supported(name, &utf8) || !host_locale(utf8))
      return NULL;
    as_cxx_lock();
    global_utf8 = utf8;
    as_cxx_unlock();
  }
  as_cxx_lock();
  utf8 = global_utf8;
  as_cxx_unlock();
  return utf8 ? "C.UTF-8" : "C";
}
static size_t guest_mb_cur_max(void) {
  int utf8;
  return locale_kind(selected ? selected : AS_LC_GLOBAL, &utf8) ? (utf8 ? 4 : 1)
                                                                : 0;
}
static AsGuestLconv *guest_localeconv(void) { return &conventions; }
void as_bionic_locale_thread_finalize(void) {
  int saved = errno;
  as_bionic_multibyte_thread_finalize();
  guest_uselocale(AS_LC_GLOBAL);
  selected = NULL;
  errno = saved;
}
int as_bionic_locale_reset(void) {
  as_bionic_locale_thread_finalize();
  as_cxx_lock();
  for (Locale *p = locales; p; p = p->next)
    if (p->users) {
      as_cxx_unlock();
      return -EBUSY;
    }
  Locale *head = locales;
  locale_t c = host_locales[0], utf8 = host_locales[1];
  locales = NULL;
  host_locales[0] = host_locales[1] = NULL;
  global_utf8 = 1;
  as_cxx_unlock();
  while (head) {
    Locale *next = head->next;
    as_cxx_deallocate(head);
    head = next;
  }
  if (c)
    freelocale(c);
  if (utf8)
    freelocale(utf8);
  return 0;
}
/* Both supported Android locales use C numeric/collation conventions. */
static locale_t numeric_locale(void *handle) {
  int unused;
  return locale_kind(handle, &unused) ? host_locale(0) : NULL;
}
#define NUMBER(name, type)                                                     \
  static type guest_##name(const char *s, char **end, void *handle) {          \
    locale_t host = numeric_locale(handle);                                    \
    if (!host) {                                                               \
      if (end)                                                                 \
        *end = (char *)s;                                                      \
      return 0;                                                                \
    }                                                                          \
    return name(s, end, host);                                                 \
  }
#define INTEGER(name, type)                                                    \
  static type guest_##name(const char *s, char **end, int base,                \
                           void *handle) {                                     \
    locale_t host = numeric_locale(handle);                                    \
    if (!host) {                                                               \
      if (end)                                                                 \
        *end = (char *)s;                                                      \
      return 0;                                                                \
    }                                                                          \
    return name(s, end, base, host);                                           \
  }
NUMBER(strtod_l, double)
NUMBER(strtof_l, float)
NUMBER(strtold_l, long double)
INTEGER(strtol_l, long)
INTEGER(strtoll_l, long long)
INTEGER(strtoul_l, unsigned long)
INTEGER(strtoull_l, unsigned long long)
#define CLASSIFY(name)                                                         \
  static int guest_##name(int c, void *handle) {                               \
    locale_t host = numeric_locale(handle);                                    \
    return host ? name(c, host) : 0;                                           \
  }
CLASSIFY(isalnum_l)
CLASSIFY(isalpha_l)
CLASSIFY(isblank_l)
CLASSIFY(iscntrl_l)
CLASSIFY(isdigit_l)
CLASSIFY(isgraph_l)
CLASSIFY(islower_l)
CLASSIFY(isprint_l)
CLASSIFY(ispunct_l)
CLASSIFY(isspace_l)
CLASSIFY(isupper_l)
CLASSIFY(isxdigit_l)
CLASSIFY(tolower_l)
CLASSIFY(toupper_l)
/* Android's wide classification is Unicode, including in its C locale.
 * Use the real host UTF-8 classifier. Its Unicode database is a platform
 * capability; exact ICU-version equivalence is not claimed. */
#define WIDE(name, type)                                                       \
  static type guest_##name(uint32_t c, void *handle) {                         \
    int unused;                                                                \
    if (!locale_kind(handle, &unused))                                         \
      return 0;                                                                \
    locale_t host = host_locale(1);                                            \
    return host ? (type)name((wint_t)c, host) : 0;                             \
  }
WIDE(iswalnum_l, int)
WIDE(iswalpha_l, int)
WIDE(iswblank_l, int)
WIDE(iswcntrl_l, int)
WIDE(iswdigit_l, int)
WIDE(iswgraph_l, int)
WIDE(iswlower_l, int)
WIDE(iswprint_l, int)
WIDE(iswpunct_l, int)
WIDE(iswspace_l, int)
WIDE(iswupper_l, int)
WIDE(iswxdigit_l, int)
WIDE(towlower_l, uint32_t)
WIDE(towupper_l, uint32_t)
static int guest_strcoll_l(const char *a, const char *b, void *handle) {
  return numeric_locale(handle) ? strcmp(a, b) : 0;
}
static int guest_strcoll(const char *a, const char *b) { return strcmp(a, b); }
static size_t guest_strxfrm_l(char *out, const char *s, size_t n,
                              void *handle) {
  if (!numeric_locale(handle))
    return 0;
  size_t length = strlen(s);
  if (n)
    memcpy(out, s, length < n ? length + 1 : n);
  return length;
}
static int guest_wcscoll_l(const uint32_t *a, const uint32_t *b, void *handle) {
  if (!numeric_locale(handle))
    return 0;
  while (*a && *a == *b) {
    ++a;
    ++b;
  }
  return (*a > *b) - (*a < *b);
}
static size_t guest_wcsxfrm_l(uint32_t *out, const uint32_t *s, size_t n,
                              void *handle) {
  if (!numeric_locale(handle))
    return 0;
  size_t length = 0;
  while (s[length])
    ++length;
  size_t count = length < n ? length + 1 : n;
  if (count > SIZE_MAX / sizeof(*s)) {
    errno = EOVERFLOW;
    return 0;
  }
  if (n)
    memcpy(out, s, count * sizeof(*s));
  return length;
}
static int expand_time_format(const char *format, size_t length,
                              const AsLocaleTm *value, char *expanded,
                              size_t *required) {
  size_t written = 0;
#define EMIT(c)                                                                \
  do {                                                                         \
    if (expanded)                                                              \
      expanded[written] = (c);                                                 \
    ++written;                                                                 \
  } while (0)
  for (size_t i = 0; i < length; ++i) {
    if (format[i] != '%' || i + 1 == length) {
      EMIT(format[i]);
      continue;
    }
    char spec = format[i + 1];
    if (spec == '%') {
      EMIT('%');
      EMIT('%');
      ++i;
      continue;
    }
    size_t next = i + 1;
    if ((spec == 'E' || spec == 'O') && next + 1 < length)
      spec = format[++next];
    if (spec == 's') {
      errno = ENOTSUP;
      return 0;
    }
    if (spec != 'z' && spec != 'Z') {
      EMIT('%');
      continue;
    }
    i = next;
    char offset[32];
    const char *text = value->zone ? value->zone : "";
    if (spec == 'z') {
      long seconds = value->gmtoff;
      if (seconds <= -86400 || seconds >= 86400) {
        errno = EINVAL;
        return 0;
      }
      unsigned magnitude = seconds < 0 ? (unsigned)-seconds : (unsigned)seconds;
      snprintf(offset, sizeof(offset), "%c%02u%02u", seconds < 0 ? '-' : '+',
               magnitude / 3600, magnitude / 60 % 60);
      text = offset;
    }
    size_t n = strnlen(text, 256);
    if (n == 256) {
      errno = EOVERFLOW;
      return 0;
    }
    for (size_t j = 0; j < n; ++j) {
      if (text[j] == '%')
        EMIT('%');
      EMIT(text[j]);
    }
  }
  if (expanded)
    expanded[written] = 0;
  *required = written;
#undef EMIT
  return 1;
}
/* Convert the Android tm layout and substitute explicit offset/zone before
 * calling the C formatter, since newlib tm lacks Android's extension fields. */
static size_t guest_strftime_l(char *out, size_t size, const char *format,
                               const AsLocaleTm *value, void *handle) {
  locale_t host = numeric_locale(handle);
  if (!host)
    return 0;
  if (!value || !format) {
    errno = EINVAL;
    return 0;
  }
  size_t length = strnlen(format, 65537);
  if (length > 65536) {
    errno = EOVERFLOW;
    return 0;
  }
  size_t required;
  if (!expand_time_format(format, length, value, NULL, &required))
    return 0;
  char *expanded = as_cxx_allocate(required + 1);
  if (!expanded) {
    errno = ENOMEM;
    return 0;
  }
  if (!expand_time_format(format, length, value, expanded, &required)) {
    as_cxx_deallocate(expanded);
    return 0;
  }
  struct tm tm = {0};
  tm.tm_sec = value->sec;
  tm.tm_min = value->min;
  tm.tm_hour = value->hour;
  tm.tm_mday = value->mday;
  tm.tm_mon = value->mon;
  tm.tm_year = value->year;
  tm.tm_wday = value->wday;
  tm.tm_yday = value->yday;
  tm.tm_isdst = value->isdst;
  size_t result = strftime_l(out, size, expanded, &tm, host);
  as_cxx_deallocate(expanded);
  return result;
}
#define API(f)                                                                 \
  if (!strcmp(name, #f))                                                       \
  return (uint64_t)guest_##f
uint64_t as_bionic_locale_import(const char *name) {
  API(newlocale);
  API(duplocale);
  API(uselocale);
  API(freelocale);
  API(setlocale);
  API(localeconv);
  if (!strcmp(name, "__ctype_get_mb_cur_max"))
    return (uint64_t)guest_mb_cur_max;
  API(strtod_l);
  API(strtof_l);
  API(strtold_l);
  API(strtol_l);
  API(strtoll_l);
  API(strtoul_l);
  API(strtoull_l);
  API(strcoll);
  API(strcoll_l);
  API(strxfrm_l);
  API(wcscoll_l);
  API(wcsxfrm_l);
  API(strftime_l);
  API(isalnum_l);
  API(isalpha_l);
  API(isblank_l);
  API(iscntrl_l);
  API(isdigit_l);
  API(isgraph_l);
  API(islower_l);
  API(isprint_l);
  API(ispunct_l);
  API(isspace_l);
  API(isupper_l);
  API(isxdigit_l);
  API(tolower_l);
  API(toupper_l);
  API(iswalnum_l);
  API(iswalpha_l);
  API(iswblank_l);
  API(iswcntrl_l);
  API(iswdigit_l);
  API(iswgraph_l);
  API(iswlower_l);
  API(iswprint_l);
  API(iswpunct_l);
  API(iswspace_l);
  API(iswupper_l);
  API(iswxdigit_l);
  API(towlower_l);
  API(towupper_l);
  return 0;
}
