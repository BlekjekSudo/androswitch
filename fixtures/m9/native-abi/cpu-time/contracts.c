#include <errno.h>
#include <stdio.h>

#include "cpu_time.h"
#include "switch.h"
static uint32_t version = 3 << 16;
static uint64_t frequency = 100;
static uint64_t counts[1024];
static uint32_t last_kind;
static int create_failure, close_failure, query_failure;
static unsigned next = 10;
uint32_t hosversionGet(void) { return version; }
int hosversionAtLeast(int major, int minor, int micro) {
  (void)minor;
  (void)micro;
  return (version >> 16) >= (unsigned)major;
}
Handle threadGetCurHandle(void) { return 3; }
u64 armGetSystemTickFreq(void) { return frequency; }
Result svcGetInfo(u64 *out, uint32_t kind, Handle handle, u64 sub) {
  assert(sub == UINT64_MAX);
  last_kind = kind;
  if (query_failure) return 1;
  *out = counts[handle == CUR_THREAD_HANDLE ? 3 : handle];
  return 0;
}
Result __real_threadCreate(Thread *t, ThreadFunc f, void *a, void *s, size_t n,
                           int p, int c) {
  (void)f;
  (void)a;
  (void)s;
  (void)n;
  (void)p;
  (void)c;
  if (create_failure) return 1;
  t->handle = next++;
  return 0;
}
Result __real_threadClose(Thread *t) {
  (void)t;
  return close_failure ? 1 : 0;
}
Result __wrap_threadCreate(Thread *, ThreadFunc, void *, void *, size_t, int,
                           int);
Result __wrap_threadClose(Thread *);
int main(void) {
  uint64_t out = 99;
  assert(as_cpu_time_query(0, &out) == -ENOSYS && out == 99);
  assert(!as_cpu_time_init());
  counts[3] = 100;
  assert(!as_cpu_time_query(0, &out) && out == 1000000000);
  assert(last_kind == InfoType_ThreadTickCountDeprecated);
  version = 13 << 16;
  assert(!as_cpu_time_query(1, &out) && out == 1000000000);
  assert(last_kind == 25);
  Thread t;
  assert(!__wrap_threadCreate(&t, NULL, NULL, NULL, 0, 0, 0));
  counts[t.handle] = 50;
  assert(!as_cpu_time_query(0, &out) && out == 1500000000);
  close_failure = 1;
  assert(__wrap_threadClose(&t));
  close_failure = 0;
  assert(!as_cpu_time_query(0, &out) && out == 1500000000);
  counts[t.handle] = 70;
  assert(!__wrap_threadClose(&t));
  counts[t.handle] = 900;
  assert(!as_cpu_time_query(0, &out) &&
         out == 1700000000);  // No dropped/double-counted retired time.
  create_failure = 1;
  assert(__wrap_threadCreate(&t, NULL, NULL, NULL, 0, 0, 0));
  create_failure = 0;
  assert(!as_cpu_time_query(0, &out) && out == 1700000000);
  query_failure = 1;
  out = 47;
  assert(as_cpu_time_query(0, &out) == -EIO && out == 47);
  query_failure = 0;
  version = 0;
  assert(as_cpu_time_query(0, &out) == -ENOSYS);
  version = 13 << 16;
  frequency = 0;
  assert(as_cpu_time_query(0, &out) == -EOVERFLOW);
  frequency = 100;
  assert(!__wrap_threadCreate(&t, NULL, NULL, NULL, 0, 0, 0));
  query_failure = 1;
  assert(!__wrap_threadClose(&t));
  query_failure = 0;
  assert(as_cpu_time_query(0, &out) ==
         -ENOSYS);  // Lost retirement cannot be invented as zero.
  puts(
      "kernel CPU tick accounting: live/retired/main/thread/error/version "
      "contracts PASS");
  return 0;
}
