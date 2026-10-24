/* Private Android non-cryptographic PRNG ABI. MIT OR Apache-2.0. */
#pragma once
#include <stdint.h>
uint64_t as_bionic_random_import(const char *name);
/* Called only after guest threads/destructors have quiesced. */
void as_bionic_random_reset(void);
