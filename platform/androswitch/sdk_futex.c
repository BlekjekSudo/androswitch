/* Linux Rust std futex ABI over real Horizon condition variables.
 * Ticket assignment provides an exact wake count and resolves timeout/wake
 * races. No Linux syscall is executed and no success is fabricated for unknown
 * ops. MIT OR Apache-2.0, AndroSwitch contributors. */
#include "sdk_futex.h"

#include <errno.h>
#include <limits.h>
#include <stddef.h>
#include <switch.h>
typedef struct SdkWaiter {
  uint32_t *address, mask;
  int notified;
  struct SdkWaiter *next;
} SdkWaiter;
static Mutex lock;
static CondVar condition;
static SdkWaiter *waiters;
static unsigned waiter_count;
static int fail(int code) {
  errno = code;
  return -1;
}
int as_sdk_futex(uint32_t *address, int operation, uint32_t value,
                 const struct timespec *timeout, uint32_t mask) {
  if (!address) return fail(EFAULT);
  if ((uintptr_t)address % 4) return fail(EINVAL);
  int mode = operation & 127;
  if ((operation & ~(127 | 128 | 256)) ||
      (mode != 0 && mode != 1 && mode != 9 && mode != 10))
    return fail(ENOSYS);
  if ((operation & 256) && mode != 9) return fail(EINVAL);
  if (mode == 0 || mode == 1) mask = UINT32_MAX;
  if (!mask) return fail(EINVAL);
  mutexLock(&lock);
  if (mode == 1 || mode == 10) {
    if (value > INT_MAX) {
      mutexUnlock(&lock);
      return fail(EINVAL);
    }
    unsigned selected = 0;
    for (SdkWaiter *w = waiters; w && selected < value; w = w->next)
      if (w->address == address && (w->mask & mask) && !w->notified) {
        w->notified = 2;
        selected++;
      }
    Result rc = selected ? condvarWakeAll(&condition) : 0;
    for (SdkWaiter *w = waiters; w; w = w->next)
      if (w->notified == 2) w->notified = R_FAILED(rc) ? 0 : 1;
    mutexUnlock(&lock);
    return R_FAILED(rc) ? fail(EIO) : (int)selected;
  }
  if (__atomic_load_n(address, __ATOMIC_ACQUIRE) != value) {
    mutexUnlock(&lock);
    return fail(EAGAIN);
  }
  uint64_t deadline = UINT64_MAX;
  int realtime = !!(operation & 256);
  if (timeout) {
    if (timeout->tv_sec < 0 || timeout->tv_nsec < 0 ||
        timeout->tv_nsec >= 1000000000 ||
        (uint64_t)timeout->tv_sec > INT64_MAX / 1000000000) {
      mutexUnlock(&lock);
      return fail(EINVAL);
    }
    deadline =
        (uint64_t)timeout->tv_sec * 1000000000 + (uint64_t)timeout->tv_nsec;
    if (mode == 0) {
      uint64_t now = as_sdk_now_ns(0);
      if (now == UINT64_MAX || UINT64_MAX - now < deadline) {
        mutexUnlock(&lock);
        return fail(EOVERFLOW);
      }
      deadline += now;
    }
  }
  if (waiter_count == 512) {
    mutexUnlock(&lock);
    return fail(EAGAIN);
  }
  SdkWaiter waiter = {address, mask, 0, waiters};
  waiters = &waiter;
  waiter_count++;
  int error = 0;
  while (!waiter.notified) {
    uint64_t remaining = UINT64_MAX;
    if (deadline != UINT64_MAX) {
      uint64_t now = as_sdk_now_ns(realtime);
      if (now == UINT64_MAX) {
        error = EIO;
        break;
      }
      if (now >= deadline) {
        error = ETIMEDOUT;
        break;
      }
      remaining = deadline - now;
    }
    Result rc = condvarWaitTimeout(&condition, &lock, remaining);
    if (R_FAILED(rc) && R_VALUE(rc) != KERNELRESULT(TimedOut)) {
      error = EINTR;
      break;
    }
  }
  // A wake ticket wins a simultaneous timeout after the mutex is reacquired.
  if (waiter.notified) error = 0;
  SdkWaiter **link = &waiters;
  while (*link && *link != &waiter) link = &(*link)->next;
  if (*link) *link = waiter.next;
  waiter_count--;
  mutexUnlock(&lock);
  return error ? fail(error) : 0;
}
