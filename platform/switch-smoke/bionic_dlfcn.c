/* Bounded Android libdl for linked system providers. Guest DSO loading and
 * RTLD_NEXT require loader ownership/caller scope and fail explicitly here.
 * All host function pointers cross the same TLS/errno veneer as ELF imports.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_dlfcn.h"

#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

#include "bionic_cxx.h"
#include "native.h"
#define MAX_HANDLES 256
#define MAX_VENEERS 1024
struct Library {
  char name[128];
  unsigned references;
  int global;
};
struct Veneer {
  uint64_t target, address;
};
static struct Library handles[MAX_HANDLES];
static struct Veneer veneers[MAX_VENEERS];
static size_t handle_count, veneer_count;
static __thread char error_message[256];
static __thread int error_pending;
static void fail(const char *operation, const char *name, const char *cause) {
  snprintf(error_message, sizeof(error_message), "%s(%.*s): %s", operation, 128,
           name ? name : "NULL", cause);
  error_pending = 1;
}
static char *guest_error(void) {
  if (!error_pending) return NULL;
  error_pending = 0;
  return error_message;
}
static int index_of(void *handle) {
  uintptr_t address = (uintptr_t)handle, base = (uintptr_t)handles;
  if (address < base || address - base >= handle_count * sizeof(handles[0]) ||
      (address - base) % sizeof(handles[0]))
    return -1;
  size_t index = (address - base) / sizeof(handles[0]);
  return handles[index].references ? (int)index : -1;
}
static void *guest_open(const char *filename, int flags) {
  /* Android LP64 values, independent of the host's dlfcn.h. */
  if ((flags & ~(1 | 2 | 4 | 0x100 | 0x1000)) || !(flags & 3)) {
    fail("dlopen", filename, "invalid Android RTLD flags");
    return NULL;
  }
  const char *name = filename;
  if (name && !strncmp(name, "/system/lib64/", 14)) name += 14;
  if (name && (strlen(name) >= sizeof(handles[0].name) || strchr(name, '/') ||
               !as_native_library(name))) {
    fail(
        "dlopen", filename,
        "library has no linked system provider; guest DSO loading unavailable");
    return NULL;
  }
  if (!name)
    name = "";  // A distinct ref-counted handle for default symbol scope.
  as_cxx_lock();
  for (size_t i = 0; i < handle_count; i++) {
    if (!strcmp(handles[i].name, name) && handles[i].references) {
      if (handles[i].references == UINT_MAX) {
        as_cxx_unlock();
        fail("dlopen", name, "reference count overflow");
        return NULL;
      }
      handles[i].references++;
      handles[i].global |= !!(flags & 0x100);
      as_cxx_unlock();
      return &handles[i];
    }
  }
  if (handle_count == MAX_HANDLES) {
    as_cxx_unlock();
    fail("dlopen", name, "256 handle lifetime budget");
    return NULL;
  }
  struct Library *library = &handles[handle_count++];
  strcpy(library->name, name);
  library->references = 1;
  library->global = !!(flags & 0x100);
  as_cxx_unlock();
  return library;
}
static uint64_t callable_locked(uint64_t target) {
  if (!target) return 0;
  for (size_t i = 0; i < veneer_count; i++)
    if (veneers[i].target == target) return veneers[i].address;
  if (veneer_count == MAX_VENEERS) return 0;
  uint64_t address = as_native_veneer(target);
  if (address) veneers[veneer_count++] = (struct Veneer){target, address};
  return address;
}
uint64_t as_bionic_function_veneer(uint64_t target) {
  as_cxx_lock();
  uint64_t address = callable_locked(target);
  as_cxx_unlock();
  return address;
}
static void *guest_symbol(void *handle, const char *name) {
  if (!name || !*name || strlen(name) > 65536) {
    fail("dlsym", name, "invalid symbol name");
    return NULL;
  }
  if (handle == (void *)(intptr_t)-1) {
    fail("dlsym", name, "RTLD_NEXT caller scope unavailable");
    return NULL;
  }
  as_cxx_lock();
  int index = handle ? index_of(handle) : -1;
  if (handle && index < 0) {
    as_cxx_unlock();
    fail("dlsym", name, "invalid or closed handle");
    return NULL;
  }
  const char *library =
      index < 0 || !handles[index].name[0] ? NULL : handles[index].name;
  int data = 0;
  uint64_t target = as_native_system_symbol(library, name, &data);
  if (!target) {
    as_cxx_unlock();
    fail("dlsym", name, "symbol is unavailable in this library scope");
    return NULL;
  }
  if (data) {
    as_cxx_unlock();
    return (void *)(uintptr_t)target;
  }
  uint64_t address = callable_locked(target);
  as_cxx_unlock();
  if (!address) fail("dlsym", name, "function veneer budget/allocation failed");
  return (void *)(uintptr_t)address;
}

static int guest_close(void *handle) {
  as_cxx_lock();
  int index = index_of(handle);
  if (index < 0) {
    as_cxx_unlock();
    fail("dlclose", NULL, "invalid or closed handle");
    return -1;
  }
  handles[index].references--;
  /* Linked providers are resident, as are veneers used by other references.
   * Handle slots are never reused within a session (stale pointers stay dead).
   */
  as_cxx_unlock();
  return 0;
}
uint64_t as_bionic_dlfcn_import(const char *name) {
  if (!strcmp(name, "dlopen")) return (uint64_t)guest_open;
  if (!strcmp(name, "dlsym")) return (uint64_t)guest_symbol;
  if (!strcmp(name, "dlclose")) return (uint64_t)guest_close;
  if (!strcmp(name, "dlerror")) return (uint64_t)guest_error;
  return 0;
}
void as_bionic_dlfcn_reset(void) {
  // Called only after worker quiescence, before executable mappings release.
  memset(handles, 0, sizeof(handles));
  memset(veneers, 0, sizeof(veneers));
  handle_count = veneer_count = 0;
  error_pending = 0;
  error_message[0] = 0;
}
