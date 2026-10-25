/* Real host backing/protection plus failure injection for guest adapters.
 * Host contract only; Horizon permissions require device/emulator evidence. */
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#ifdef __MSYS__
#include <windows.h>
#endif

#include "bionic_memory.h"
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static int permission_failure;
static unsigned allocations, releases;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&lock)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&lock)); }
void *as_guest_memory_allocate(size_t size) {
#ifdef __MSYS__
  void *p = VirtualAlloc(NULL, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
  if (!p) return NULL;
#else
  void *p = mmap(NULL, size, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
  if (p == MAP_FAILED) return NULL;
#endif
  ++allocations;
  return p;
}
int as_guest_memory_permission(void *p, size_t size, int protection) {
  if (permission_failure) {
    errno = EACCES;
    return -1;
  }
#ifdef __MSYS__
  DWORD before;
  DWORD mode = protection == 3   ? PAGE_READWRITE
               : protection == 1 ? PAGE_READONLY
                                 : PAGE_NOACCESS;
  if (!VirtualProtect(p, size, mode, &before)) {
    errno = EACCES;
    return -1;
  }
  return 0;
#else
  return mprotect(p, size, protection);
#endif
}
void as_guest_memory_release(void *p, size_t size) {
#ifdef __MSYS__
  (void)size;
  assert(VirtualFree(p, 0, MEM_RELEASE));
#else
  assert(!munmap(p, size));
#endif
  ++releases;
}
#define LOAD(type, name) ((type)(uintptr_t)as_bionic_memory_import(name))
typedef void *(*Map)(void *, size_t, int, int, int, int64_t);
typedef int (*Unmap)(void *, size_t);
typedef int (*Protect)(void *, size_t, int);
typedef int (*Advise)(void *, size_t, int);
int main(void) {
  Map map = LOAD(Map, "mmap"), map64 = LOAD(Map, "mmap64");
  Unmap unmap = LOAD(Unmap, "munmap");
  Protect protect = LOAD(Protect, "mprotect");
  Advise advise = LOAD(Advise, "madvise");
  assert(map && map == map64 && unmap && protect && advise);
  assert(!as_bionic_memory_import("mremap"));
  assert(map(NULL, 0, 3, 0x22, -1, 0) == MAP_FAILED && errno == EINVAL);
  assert(map(NULL, 4096, 7, 0x22, -1, 0) == MAP_FAILED && errno == ENOTSUP);
  assert(map(NULL, 4096, 3, 0x21, -1, 0) == MAP_FAILED && errno == ENOTSUP);
  assert(map(NULL, 4096, 3, 2, 5, 0) == MAP_FAILED && errno == ENOTSUP);
  assert(map(NULL, 4096, 3, 0x32, -1, 0) == MAP_FAILED && errno == ENOTSUP);
  assert(map(NULL, 4096, 3, 0x22, -1, 1) == MAP_FAILED && errno == EINVAL);
  unsigned char *p = map(NULL, 8193, 3, 0x22, -1, 0);
  assert(p != MAP_FAILED && !((uintptr_t)p % 4096));
  for (unsigned i = 0; i < 12288; i++) assert(!p[i]);
  memset(p, 0x73, 12288);
  assert(!protect(p, 4096, 1) && p[0] == 0x73);
  assert(!protect(p, 4096, 3));
  assert(!advise(p, 4096, 4));
  for (unsigned i = 0; i < 4096; i++) assert(!p[i]);
  assert(p[4096] == 0x73 && p[8192] == 0x73);
  assert(!advise(p, 12288, 0));
  assert(!advise(p, 12288, 1));
  assert(!advise(p, 12288, 2));
  assert(!advise(p, 12288, 3));
  assert(advise(p, 12288, 8) == -1 && errno == ENOTSUP);
  assert(protect(p + 1, 4096, 1) == -1 && errno == EINVAL);
  assert(unmap(p, 0) == -1 && errno == EINVAL);
  permission_failure = 1;
  assert(unmap(p, 4096) == -1 && errno == EACCES && p[0] == 0);
  permission_failure = 0;
  int partial = unmap(p + 4096, 1);
  if (partial) perror("partial unmap permission");
  assert(!partial);
  assert(protect(p, 12288, 3) == -1 && errno == ENOMEM);
  assert(p[8192] == 0x73);
  assert(!unmap(p, 4096));
  assert(releases == 0); /* Partial unmaps retain isolated backing. */
  assert(!unmap(p + 8192, 4096) && releases == 1);
  assert(unmap((void *)4096, 4096) == -1 && errno == ENOTSUP);
  unsigned char *none = map(NULL, 4096, 0, 0x22, -1, 0);
  assert(none != MAP_FAILED && !protect(none, 4096, 3) && !none[0]);
  unsigned char *ro = map(NULL, 4096, 1, 0x22, -1, 0);
  assert(ro != MAP_FAILED && !ro[0]);
  assert(advise(ro, 4096, 4) == -1 && errno == ENOTSUP);
  permission_failure = 1;
  assert(as_bionic_memory_reset() == -1 && allocations != releases);
  permission_failure = 0;
  assert(!as_bionic_memory_reset() && allocations == releases);
  assert(!as_bionic_memory_reset());
  permission_failure = 1;
  assert(map(NULL, 4096, 1, 0x22, -1, 0) == MAP_FAILED && errno == EACCES);
  permission_failure = 0;
  assert(allocations == releases);
  puts(
      "anonymous mmap: zeroing, permissions, partial unmap, advice, ownership, "
      "failures and reset PASS");
}
