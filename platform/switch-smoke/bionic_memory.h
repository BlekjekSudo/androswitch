/* Owned anonymous Android mappings, MIT OR Apache-2.0. */
#pragma once
#include <stddef.h>
#include <stdint.h>
uint64_t as_bionic_memory_import(const char *name);
int as_bionic_memory_reset(void);
/* Host TLS only. Permissions are Android PROT_NONE/READ/READ|WRITE (0/1/3).
 * Backing must be page isolated; release is called after restoring RW. */
void *as_guest_memory_allocate(size_t length);
int as_guest_memory_permission(void *address, size_t length, int protection);
void as_guest_memory_release(void *address, size_t length);
