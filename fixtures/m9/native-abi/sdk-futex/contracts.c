/* Production futex adapter with real host pthread scheduling, not GPU evidence.
 */
#include <assert.h>
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <time.h>

#include "sdk_futex.h"
uint64_t as_sdk_now_ns(int realtime) {
  struct timespec t;
  assert(!clock_gettime(realtime ? CLOCK_REALTIME : CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}
static void pause_ns(long ns) {
  struct timespec t = {0, ns};
  nanosleep(&t, NULL);
}
struct Context {
  uint32_t *address, mask;
  int result, error;
  atomic_int ready;
};
static void *wait_thread(void *opaque) {
  struct Context *c = opaque;
  uint64_t deadline = as_sdk_now_ns(0) + 2000000000;
  struct timespec t = {deadline / 1000000000, deadline % 1000000000};
  atomic_store(&c->ready, 1);
  c->result = as_sdk_futex(c->address, 9 | 128, *c->address, &t, c->mask);
  c->error = errno;
  return NULL;
}
int main(void) {
  uint32_t address = 7, other = 9;
  errno = 0;
  assert(as_sdk_futex(&address, 0, 8, NULL, 0) == -1 && errno == EAGAIN);
  struct timespec zero = {0, 0};
  assert(as_sdk_futex(&address, 0, 7, &zero, 0) == -1 && errno == ETIMEDOUT);
  assert(as_sdk_futex((void *)((char *)&address + 1), 0, 7, NULL, 0) == -1 &&
         errno == EINVAL);
  assert(as_sdk_futex(&address, 9, 7, NULL, 0) == -1 && errno == EINVAL);
  assert(as_sdk_futex(&address, 127, 7, NULL, 1) == -1 && errno == ENOSYS);
  struct Context a = {.address = &address, .mask = 1},
                 b = {.address = &address, .mask = 2},
                 c = {.address = &other, .mask = 1};
  pthread_t ta, tb, tc;
  assert(!pthread_create(&ta, NULL, wait_thread, &a));
  assert(!pthread_create(&tb, NULL, wait_thread, &b));
  assert(!pthread_create(&tc, NULL, wait_thread, &c));
  while (!atomic_load(&a.ready) || !atomic_load(&b.ready) ||
         !atomic_load(&c.ready))
    pause_ns(1000000);
  int wakes = 0;
  uint64_t until = as_sdk_now_ns(0) + 1000000000;
  while (!wakes && as_sdk_now_ns(0) < until) {
    wakes = as_sdk_futex(&address, 10, INT_MAX, NULL, 1);
    assert(wakes >= 0);
    if (!wakes) pause_ns(1000000);
  }
  assert(wakes == 1);
  assert(!pthread_join(ta, NULL));
  assert(a.result == 0);
  // Address and bitset selection must leave both other waiters pending.
  wakes = 0;
  while (!wakes && as_sdk_now_ns(0) < until) {
    wakes = as_sdk_futex(&address, 10, INT_MAX, NULL, 2);
    if (!wakes) pause_ns(1000000);
  }
  assert(wakes == 1);
  assert(!pthread_join(tb, NULL));
  assert(b.result == 0);
  wakes = 0;
  while (!wakes && as_sdk_now_ns(0) < until) {
    wakes = as_sdk_futex(&other, 1, 1, NULL, 0);
    if (!wakes) pause_ns(1000000);
  }
  assert(wakes == 1);
  assert(!pthread_join(tc, NULL));
  assert(c.result == 0);
  assert(as_sdk_futex(&address, 1, INT_MAX, NULL, 0) == 0);
  puts(
      "Rust SDK futex adapter: real waits/wakes, count, bitset/address "
      "isolation, timeout, invalid/error contracts PASS");
  return 0;
}
