/* Android LP64 libdl contracts. MIT OR Apache-2.0. */
#pragma once
#include <stdint.h>
uint64_t as_bionic_dlfcn_import(const char *name);
void as_bionic_dlfcn_reset(void);
/* Platform symbol lookup returns raw host functions or direct data addresses.
 * It must never advertise a library without a real provider. */
int as_native_library(const char *name);
uint64_t as_native_system_symbol(const char *library, const char *name,
                                 int *data);
uint64_t as_bionic_function_veneer(uint64_t target);
