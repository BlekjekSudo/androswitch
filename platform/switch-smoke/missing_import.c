/* Lazy unresolved *function* bindings fail on first invocation with the exact
 * ELF library/symbol. They never return a fabricated implementation result.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "native.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
extern int as_native_allocate(size_t, uint64_t *, uint64_t *);
extern int as_native_write(uint64_t, size_t, const uint8_t *, size_t);
extern int as_native_finalize(uint64_t, const uint8_t *, size_t);
extern void as_native_release(uint64_t);
extern void as_native_import_bridge(void);
typedef struct Missing {
  char *library, *symbol;
  size_t name_bytes;
  uint64_t address;
  struct Missing *next;
} Missing;
static Missing *entries;
static size_t count, metadata_bytes;
static __thread const Missing *last_missing;
static void missing_call(Missing *entry) {
  as_cxx_lock();
  Missing *known = entries;
  while (known && known != entry)
    known = known->next;
  if (known)
    last_missing = known;
  as_cxx_unlock();
  as_native_trap(known ? -1211 : -1002);
}
uint64_t as_native_missing_veneer(const char *library, const char *symbol) {
  size_t left = strnlen(library, 4097), right = strnlen(symbol, 4097);
  if (!left || !right || left > 4096 || right > 4096)
    return 0;
  as_cxx_lock();
  for (Missing *entry = entries; entry; entry = entry->next)
    if (!strcmp(entry->library, library) && !strcmp(entry->symbol, symbol)) {
      uint64_t address = entry->address;
      as_cxx_unlock();
      return address;
    }
  size_t bytes = left + right + 2;
  if (count >= 4096 || metadata_bytes + bytes > 4u << 20) {
    as_cxx_unlock();
    return 0;
  }
  Missing *entry = calloc(1, sizeof(*entry));
  if (!entry) {
    as_cxx_unlock();
    return 0;
  }
  entry->library = strdup(library);
  entry->symbol = strdup(symbol);
  uint64_t address = 0, token = 0;
  if (!entry->library || !entry->symbol ||
      as_native_allocate(4096, &address, &token)) {
    free(entry->library);
    free(entry->symbol);
    free(entry);
    as_cxx_unlock();
    return 0;
  }
  uint8_t code[40] = {0};
  const uint32_t instructions[] = {0x58000080, 0x580000b0, 0x580000d1,
                                   0xd61f0220};
  memcpy(code, instructions, 16);
  uint64_t identity = (uint64_t)entry, function = (uint64_t)missing_call;
  uint64_t bridge = (uint64_t)as_native_import_bridge;
  memcpy(code + 16, &identity, 8);
  memcpy(code + 24, &function, 8);
  memcpy(code + 32, &bridge, 8);
  uint8_t page = 1;
  if (as_native_write(token, 0, code, sizeof(code)) ||
      as_native_finalize(token, &page, 1)) {
    as_native_release(token);
    free(entry->library);
    free(entry->symbol);
    free(entry);
    as_cxx_unlock();
    return 0;
  }
  entry->address = address;
  entry->name_bytes = bytes;
  entry->next = entries;
  entries = entry;
  ++count;
  metadata_bytes += bytes;
  as_cxx_unlock();
  return address;
}
const char *as_native_missing_library(void) {
  return last_missing ? last_missing->library : NULL;
}
const char *as_native_missing_symbol(void) {
  return last_missing ? last_missing->symbol : NULL;
}
void as_native_missing_reset(void) {
  /* Called only after all workers quiesce and executable mappings are gone. */
  as_cxx_lock();
  while (entries) {
    Missing *entry = entries;
    entries = entry->next;
    free(entry->library);
    free(entry->symbol);
    free(entry);
  }
  count = metadata_bytes = 0;
  last_missing = NULL;
  as_cxx_unlock();
}
