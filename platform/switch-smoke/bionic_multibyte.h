/* Android LP64 multibyte ABI. Original AndroSwitch code, MIT OR Apache-2.0. */
#pragma once
#include <stddef.h>
#include <stdint.h>
typedef struct {
  unsigned char sequence[4];
  unsigned char reserved[4];
} AsGuestMbstate;
uint64_t as_bionic_multibyte_import(const char *name);
void as_bionic_multibyte_thread_finalize(void);
