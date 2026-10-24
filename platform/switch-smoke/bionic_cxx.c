/* Android legacy libstdc++/Itanium ABI contracts; no host C++ object layout is
 * exposed to guests. Exception unwinding is deliberately not advertised.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>

typedef struct Destructor {
  uint64_t function, argument, dso;
  struct Destructor *next;
} Destructor;
typedef struct Guard {
  uint64_t *address, owner;
  struct Guard *next;
} Guard;
static Destructor *destructors;
static unsigned destructor_count;
static Guard *guards;
static unsigned guard_count;
static __thread Destructor *thread_destructors;
static __thread unsigned thread_destructor_count;
static const unsigned char nothrow_object;

static void *operator_new_nothrow(size_t size, const void *tag) {
  (void)tag;
  return as_cxx_allocate(size ? size : 1);
}
static void *operator_new(size_t size) {
  void *pointer = operator_new_nothrow(size, NULL);
  if (!pointer)
    as_cxx_trap(-1201); /* bad_alloc cannot unwind through the host bridge. */
  return pointer;
}
static void operator_delete(void *pointer) { as_cxx_deallocate(pointer); }
static void operator_delete_sized(void *pointer, size_t size) {
  (void)size;
  as_cxx_deallocate(pointer);
}
static void operator_delete_nothrow(void *pointer, const void *tag) {
  (void)tag;
  as_cxx_deallocate(pointer);
}
static void pure_virtual(void) { as_cxx_trap(-1202); }

static int register_destructor(Destructor **head, unsigned *count,
                               uint64_t function, uint64_t argument,
                               uint64_t dso) {
  if (!function || *count >= 4096) {
    errno = function ? ENOMEM : EINVAL;
    return -1;
  }
  Destructor *entry = malloc(sizeof(*entry));
  if (!entry) {
    errno = ENOMEM;
    return -1;
  }
  *entry = (Destructor){function, argument, dso, *head};
  *head = entry;
  ++*count;
  return 0;
}
static int cxa_atexit(uint64_t function, uint64_t argument, uint64_t dso) {
  as_cxx_lock();
  int rc = register_destructor(&destructors, &destructor_count, function,
                               argument, dso);
  as_cxx_unlock();
  return rc;
}
static int cxa_thread_atexit(uint64_t function, uint64_t argument,
                             uint64_t dso) {
  return register_destructor(&thread_destructors, &thread_destructor_count,
                             function, argument, dso);
}
static int finalize_dso(uint64_t dso) {
  /* Remove before entering guest code: repeated, concurrent or reentrant
   * finalization must not execute a registered destructor twice. */
  for (unsigned calls = 0;; ++calls) {
    if (calls >= 16384)
      return -ELOOP;
    as_cxx_lock();
    Destructor **link = &destructors;
    while (*link && dso && (*link)->dso != dso)
      link = &(*link)->next;
    Destructor *entry = *link;
    if (entry) {
      *link = entry->next;
      --destructor_count;
    }
    as_cxx_unlock();
    if (!entry)
      return 0;
    uint64_t function = entry->function, argument = entry->argument;
    free(entry);
    int rc = as_cxx_invoke(function, argument);
    if (rc)
      return rc;
  }
}
static void cxa_finalize(uint64_t dso) {
  int rc = finalize_dso(dso);
  if (rc)
    as_cxx_trap(rc);
}
int as_bionic_cxx_finalize(void) { return finalize_dso(0); }
int as_bionic_cxx_thread_finalize(void) {
  /* A destructor may register a new destructor; it is processed in LIFO order.
   * Bound runaway registration so cleanup can fail instead of looping forever.
   */
  for (unsigned calls = 0; thread_destructors && calls < 16384; ++calls) {
    Destructor *entry = thread_destructors;
    thread_destructors = entry->next;
    --thread_destructor_count;
    uint64_t function = entry->function, argument = entry->argument;
    free(entry);
    int rc = as_cxx_invoke(function, argument);
    if (rc)
      return rc;
  }
  return thread_destructors ? -ELOOP : 0;
}
static Guard **find_guard(uint64_t *address) {
  Guard **link = &guards;
  while (*link && (*link)->address != address)
    link = &(*link)->next;
  return link;
}
static int guard_acquire(uint64_t *address) {
  if (!address)
    as_cxx_trap(-1203);
  as_cxx_lock();
  for (;;) {
    if (__atomic_load_n((unsigned char *)address, __ATOMIC_ACQUIRE)) {
      as_cxx_unlock();
      return 0;
    }
    Guard **link = find_guard(address);
    if (!*link) {
      Guard *entry = guard_count < 256 ? malloc(sizeof(*entry)) : NULL;
      if (!entry) {
        as_cxx_unlock();
        as_cxx_trap(-ENOMEM);
        return 0;
      }
      *entry = (Guard){address, as_cxx_thread(), guards};
      guards = entry;
      ++guard_count;
      as_cxx_unlock();
      return 1;
    }
    if ((*link)->owner == as_cxx_thread()) {
      as_cxx_unlock();
      as_cxx_trap(-1204); /* Recursive local-static initialization. */
      return 0;
    }
    as_cxx_wait();
  }
}
static void guard_finish(uint64_t *address, int complete) {
  as_cxx_lock();
  Guard **link = find_guard(address);
  if (!*link || (*link)->owner != as_cxx_thread()) {
    as_cxx_unlock();
    as_cxx_trap(-1203);
    return;
  }
  Guard *entry = *link;
  *link = entry->next;
  --guard_count;
  if (complete)
    __atomic_store_n((unsigned char *)address, 1, __ATOMIC_RELEASE);
  free(entry);
  as_cxx_wake();
  as_cxx_unlock();
}
static void guard_release(uint64_t *address) { guard_finish(address, 1); }
static void guard_abort(uint64_t *address) { guard_finish(address, 0); }
unsigned as_bionic_cxx_pending(void) {
  int saved = errno;
  as_cxx_lock();
  unsigned pending = (destructors ? AS_CXX_PENDING_GLOBAL : 0) |
                     (thread_destructors ? AS_CXX_PENDING_THREAD : 0);
  as_cxx_unlock();
  errno = saved;
  return pending;
}
int as_bionic_cxx_reset(void) {
  /* Caller has proved every guest/platform thread quiescent. A controlled
   * startup trap can abandon an in-progress local-static guard; its code and
   * address are already being discarded. Reclaim registry nodes without
   * touching guard addresses, which may have been unmapped by Loader::drop. */
  as_cxx_lock();
  if (destructors || thread_destructors) {
    as_cxx_unlock();
    return -EBUSY;
  }
  while (guards) {
    Guard *entry = guards;
    guards = entry->next;
    free(entry);
  }
  guard_count = 0;
  as_cxx_unlock();
  return 0;
}

#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
uint64_t as_bionic_cxx_data(const char *name) {
  return !strcmp(name, "_ZSt7nothrow") ? (uint64_t)&nothrow_object : 0;
}
uint64_t as_bionic_cxx_import(const char *name) {
  API("_Znwm", operator_new);
  API("_Znam", operator_new);
  API("_ZnwmRKSt9nothrow_t", operator_new_nothrow);
  API("_ZnamRKSt9nothrow_t", operator_new_nothrow);
  API("_ZdlPv", operator_delete);
  API("_ZdaPv", operator_delete);
  API("_ZdlPvm", operator_delete_sized);
  API("_ZdaPvm", operator_delete_sized);
  API("_ZdlPvRKSt9nothrow_t", operator_delete_nothrow);
  API("_ZdaPvRKSt9nothrow_t", operator_delete_nothrow);
  API("__cxa_atexit", cxa_atexit);
  API("__cxa_finalize", cxa_finalize);
  API("__cxa_thread_atexit_impl", cxa_thread_atexit);
  API("__cxa_guard_acquire", guard_acquire);
  API("__cxa_guard_release", guard_release);
  API("__cxa_guard_abort", guard_abort);
  API("__cxa_pure_virtual", pure_virtual);
  API("__cxa_deleted_virtual", pure_virtual);
  return 0;
}
