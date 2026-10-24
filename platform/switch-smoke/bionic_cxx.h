/* Shared Android legacy C++ ABI boundary. MIT OR Apache-2.0. */
#pragma once
#include <stddef.h>
#include <stdint.h>
uint64_t as_bionic_cxx_import(const char *name);
uint64_t as_bionic_cxx_data(const char *name);
int as_bionic_cxx_finalize(void);
int as_bionic_cxx_thread_finalize(void);
int as_bionic_cxx_reset(void);
/* Read-only post-quiescence diagnostics: never consumes a guest callback. */
#define AS_CXX_PENDING_GLOBAL 1u
#define AS_CXX_PENDING_THREAD 2u
unsigned as_bionic_cxx_pending(void);
/* Platform adapters hold host TLS; callbacks explicitly restore guest TLS. */
void as_cxx_lock(void);
void as_cxx_unlock(void);
void as_cxx_wait(void);
void as_cxx_wake(void);
uint64_t as_cxx_thread(void);
void *as_cxx_allocate(size_t size);
void as_cxx_deallocate(void *pointer);
int as_cxx_invoke(uint64_t function, uint64_t argument);
void as_cxx_trap(int error);
