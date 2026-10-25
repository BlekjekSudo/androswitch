/* Private anonymous memory with actual page permissions and session ownership.
 * File/shared/fixed/executable mappings are explicit unsupported boundaries.
 * Partial unmaps revoke access immediately; backing is retained until the last
 * page of an allocation is unmapped (no demand paging or overcommit promise).
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_memory.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "bionic_cxx.h"

#define PAGE 4096u
typedef struct AnonymousMapping {
  unsigned char *base;
  size_t length;
  /* 0 = unmapped; otherwise protection + 1 (NONE=1, READ=2, RW=4). */
  unsigned char *pages;
  struct AnonymousMapping *next;
} AnonymousMapping;
static AnonymousMapping *mappings;
static int fail(int error) {
  errno = error;
  return -1;
}
static int rounded(size_t length, size_t *out) {
  if (!length) return fail(EINVAL);
  if (length > SIZE_MAX - (PAGE - 1)) return fail(ENOMEM);
  *out = (length + PAGE - 1) & ~(size_t)(PAGE - 1);
  return 0;
}
static int supported_protection(int protection) {
  if (protection & ~7) return fail(EINVAL);
  if (protection != 0 && protection != 1 && protection != 3)
    return fail(ENOTSUP);
  return 0;
}
/* Caller holds the shared native metadata lock. Never searches host mappings.
 */
static AnonymousMapping **find(void *address, size_t length) {
  uintptr_t start = (uintptr_t)address;
  for (AnonymousMapping **p = &mappings; *p; p = &(*p)->next) {
    uintptr_t base = (uintptr_t)(*p)->base;
    if (start >= base && start - base <= (*p)->length &&
        length <= (*p)->length - (start - base))
      return p;
  }
  return NULL;
}
static int range(void *address, size_t length, size_t *size) {
  if ((uintptr_t)address % PAGE || rounded(length, size)) return fail(EINVAL);
  if (*size > UINTPTR_MAX - (uintptr_t)address) return fail(EINVAL);
  return 0;
}
static void *guest_mmap(void *hint, size_t length, int protection, int flags,
                        int fd, int64_t offset) {
  (void)hint; /* Non-fixed addresses are advisory. */
  (void)fd;   /* Anonymous mappings ignore fd. */
  size_t size;
  if (rounded(length, &size) || supported_protection(protection))
    return (void *)-1;
  if (offset < 0 || (uint64_t)offset % PAGE) {
    errno = EINVAL;
    return (void *)-1;
  }
  if ((flags & 3) != 1 && (flags & 3) != 2) {
    errno = EINVAL;
    return (void *)-1;
  }
  /* MAP_NORESERVE is permitted to reserve eagerly; MAP_STACK is a Linux no-op.
   * The guest never receives an address with weaker protection than requested.
   */
  if ((flags & 3) != 2 || !(flags & 0x20) ||
      (flags & ~(2 | 0x20 | 0x4000 | 0x20000))) {
    errno = ENOTSUP;
    return (void *)-1;
  }
  AnonymousMapping *m = calloc(1, sizeof(*m));
  if (!m) {
    errno = ENOMEM;
    return (void *)-1;
  }
  m->pages = malloc(size / PAGE);
  m->base = m->pages ? as_guest_memory_allocate(size) : NULL;
  if (!m->base) {
    free(m->pages);
    free(m);
    errno = ENOMEM;
    return (void *)-1;
  }
  memset(m->base, 0, size);
  if (as_guest_memory_permission(m->base, size, protection)) {
    int saved = errno;
    as_guest_memory_release(m->base, size);
    free(m->pages);
    free(m);
    errno = saved;
    return (void *)-1;
  }
  memset(m->pages, protection + 1, size / PAGE);
  m->length = size;
  as_cxx_lock();
  m->next = mappings;
  mappings = m;
  as_cxx_unlock();
  return m->base;
}
static int guest_mprotect(void *address, size_t length, int protection) {
  if ((uintptr_t)address % PAGE) return fail(EINVAL);
  if (supported_protection(protection)) return -1;
  if (!length) return 0;
  size_t size;
  if (range(address, length, &size) || supported_protection(protection))
    return -1;
  as_cxx_lock();
  AnonymousMapping **p = find(address, size);
  if (!p) {
    as_cxx_unlock();
    return fail(ENOMEM);
  }
  AnonymousMapping *m = *p;
  size_t first = ((uintptr_t)address - (uintptr_t)m->base) / PAGE;
  for (size_t i = 0; i < size / PAGE; ++i)
    if (!m->pages[first + i]) {
      as_cxx_unlock();
      return fail(ENOMEM);
    }
  int rc = as_guest_memory_permission(address, size, protection);
  if (!rc) memset(m->pages + first, protection + 1, size / PAGE);
  as_cxx_unlock();
  return rc;
}
static int guest_munmap(void *address, size_t length) {
  size_t size;
  if (range(address, length, &size)) return -1;
  as_cxx_lock();
  AnonymousMapping **p = find(address, size);
  if (!p) {
    as_cxx_unlock();
    return fail(ENOTSUP);
  }
  AnonymousMapping *m = *p;
  size_t first = ((uintptr_t)address - (uintptr_t)m->base) / PAGE;
  /* Change permissions first; a failed kernel operation retains ownership. */
  int rc = as_guest_memory_permission(address, size, 0);
  if (!rc) {
    memset(m->pages + first, 0, size / PAGE);
    int live = 0;
    for (size_t i = 0; i < m->length / PAGE; ++i) live |= m->pages[i];
    if (!live) {
      rc = as_guest_memory_permission(m->base, m->length, 3);
      if (!rc) {
        *p = m->next;
        as_guest_memory_release(m->base, m->length);
        free(m->pages);
        free(m);
      }
    }
  }
  as_cxx_unlock();
  return rc;
}
static int guest_madvise(void *address, size_t length, int advice) {
  if ((uintptr_t)address % PAGE) return fail(EINVAL);
  if (advice < 0 || advice > 4) return fail(ENOTSUP);
  if (!length) return 0;
  size_t size;
  if (range(address, length, &size)) return -1;
  if (advice < 0 || advice > 4) return fail(ENOTSUP);
  as_cxx_lock();
  AnonymousMapping **p = find(address, size);
  if (!p) {
    as_cxx_unlock();
    return fail(ENOMEM);
  }
  AnonymousMapping *m = *p;
  size_t first = ((uintptr_t)address - (uintptr_t)m->base) / PAGE;
  for (size_t i = 0; i < size / PAGE; ++i) {
    unsigned state = m->pages[first + i];
    if (!state || (advice == 4 && state != 4)) {
      as_cxx_unlock();
      return fail(state ? ENOTSUP : ENOMEM);
    }
  }
  /* Private anonymous DONTNEED really restores zero bytes. Other supported
   * values are advisory access patterns over already resident backing. */
  if (advice == 4) memset(address, 0, size);
  as_cxx_unlock();
  return 0;
}
int as_bionic_memory_reset(void) {
  as_cxx_lock();
  while (mappings) {
    AnonymousMapping *m = mappings;
    if (as_guest_memory_permission(m->base, m->length, 3)) {
      as_cxx_unlock();
      return -1;
    }
    mappings = m->next;
    as_guest_memory_release(m->base, m->length);
    free(m->pages);
    free(m);
  }
  as_cxx_unlock();
  return 0;
}
extern int as_native_trace_active(void) __attribute__((weak));
extern void as_native_trace_metadata(const char *, const uint8_t *, size_t)
    __attribute__((weak));
static void *traced_mmap(void *hint, size_t length, int protection, int flags,
                         int fd, int64_t offset) {
  void *out = guest_mmap(hint, length, protection, flags, fd, offset);
  int saved = errno;
  if (as_native_trace_active && as_native_trace_active() &&
      as_native_trace_metadata) {
    char text[256];
    int n = snprintf(
        text, sizeof(text),
        "length=%zu prot=%d flags=0x%x fd=%d offset=%lld result=%p errno=%d",
        length, protection, flags, fd, (long long)offset, out,
        out == (void *)-1 ? saved : 0);
    if (n > 0 && (size_t)n < sizeof(text))
      as_native_trace_metadata("mmap", (const uint8_t *)text, (size_t)n);
  }
  errno = saved;
  return out;
}
uint64_t as_bionic_memory_import(const char *name) {
#define API(n, f) \
  if (!strcmp(name, n)) return (uint64_t)(uintptr_t)(f)
  API("mmap", traced_mmap);
  API("mmap64", traced_mmap);
  API("munmap", guest_munmap);
  API("mprotect", guest_mprotect);
  API("madvise", guest_madvise);
#undef API
  return 0;
}
