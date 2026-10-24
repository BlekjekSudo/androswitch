/* Android API26 locale ABI. Original AndroSwitch implementation (MIT OR
 * Apache-2.0); ABI constants/layout follow AOSP bionic locale.h. */
#pragma once
#include <stddef.h>
#include <stdint.h>
#define AS_LC_ALL 6
#define AS_LC_ALL_MASK 0x1fbf
#define AS_LC_GLOBAL ((void *)(intptr_t)-1)
typedef struct {
  char *decimal_point, *thousands_sep, *grouping, *int_curr_symbol;
  char *currency_symbol, *mon_decimal_point, *mon_thousands_sep, *mon_grouping;
  char *positive_sign, *negative_sign;
  unsigned char int_frac_digits, frac_digits, p_cs_precedes, p_sep_by_space;
  unsigned char n_cs_precedes, n_sep_by_space, p_sign_posn, n_sign_posn;
  unsigned char int_p_cs_precedes, int_p_sep_by_space, int_n_cs_precedes;
  unsigned char int_n_sep_by_space, int_p_sign_posn, int_n_sign_posn;
} AsGuestLconv;
typedef struct {
  int sec, min, hour, mday, mon, year, wday, yday, isdst;
  long gmtoff;
  const char *zone;
} AsLocaleTm;
uint64_t as_bionic_locale_import(const char *name);
void as_bionic_locale_thread_finalize(void);
int as_bionic_locale_reset(void);
