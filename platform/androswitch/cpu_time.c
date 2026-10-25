/* Account every libnx-created thread, including retired threads. Link wrapping
 * covers product, newlib pthreads and statically linked driver workers. Values
 * come only from kernel ThreadTickCount, never wall time or a guessed clock.
 * Scope is the actual Horizon NRO process (launcher plus current guest).
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "cpu_time.h"

#include <errno.h>
#include <stdint.h>
#include <switch.h>
#define LIMIT 512
static Mutex lock;
static struct {
  Handle handle;
  int state;
} threads[LIMIT];
static uint64_t retired;
static int initialized, lost_accounting;
static int ticks(Handle handle, uint64_t *value) {
  if (!hosversionGet()) return -ENOSYS;
  u64 count = 0;
  Result rc = svcGetInfo(&count,
                         hosversionAtLeast(13, 0, 0)
                             ? InfoType_ThreadTickCount
                             : InfoType_ThreadTickCountDeprecated,
                         handle, TickCountInfo_Total);
  if (R_FAILED(rc)) return -EIO;
  *value = count;
  return 0;
}
int as_cpu_time_init(void) {
  // Called on the process main thread before product/session worker creation.
  mutexLock(&lock);
  if (!initialized) {
    Handle main = threadGetCurHandle();
    size_t i = 0;
    while (i < LIMIT && threads[i].state) i++;
    if (!main || main == CUR_THREAD_HANDLE || i == LIMIT) {
      mutexUnlock(&lock);
      return -ENOSYS;
    }
    threads[i].handle = main;
    threads[i].state = 1;
    initialized = 1;
  }
  mutexUnlock(&lock);
  return 0;
}
Result __real_threadCreate(Thread *, ThreadFunc, void *, void *, size_t, int,
                           int);
Result __wrap_threadCreate(Thread *thread, ThreadFunc entry, void *argument,
                           void *stack, size_t size, int priority, int core) {
  mutexLock(&lock);
  size_t i = 0;
  while (i < LIMIT && threads[i].state) i++;
  if (i == LIMIT) {
    mutexUnlock(&lock);
    return MAKERESULT(Module_Libnx, LibnxError_OutOfMemory);
  }
  threads[i].state =
      2;  // Reserved: created threads cannot execute until threadStart.
  mutexUnlock(&lock);
  Result rc =
      __real_threadCreate(thread, entry, argument, stack, size, priority, core);
  mutexLock(&lock);
  if (R_SUCCEEDED(rc)) {
    threads[i].handle = thread->handle;
    threads[i].state = 1;
  } else
    threads[i].state = 0;
  mutexUnlock(&lock);
  return rc;
}
Result __real_threadClose(Thread *);
Result __wrap_threadClose(Thread *thread) {
  mutexLock(&lock);
  size_t i = 0;
  while (i < LIMIT &&
         (threads[i].state != 1 || threads[i].handle != thread->handle))
    i++;
  uint64_t final = 0;
  int error = i == LIMIT ? -ENOENT : ticks(thread->handle, &final);
  Result rc = __real_threadClose(thread);
  if (R_SUCCEEDED(rc)) {
    if (error || UINT64_MAX - retired < final)
      lost_accounting = 1;
    else
      retired += final;
    if (i < LIMIT) threads[i].state = 0;
  }
  mutexUnlock(&lock);
  return rc;
}
int as_cpu_time_query(int thread_only, uint64_t *nanoseconds) {
  if (!nanoseconds || (thread_only != 0 && thread_only != 1)) return -EINVAL;
  uint64_t total = 0;
  if (thread_only) {
    int error = ticks(CUR_THREAD_HANDLE, &total);
    if (error) return error;
  } else {
    mutexLock(&lock);
    if (!initialized || lost_accounting) {
      mutexUnlock(&lock);
      return -ENOSYS;
    }
    total = retired;
    for (size_t i = 0; i < LIMIT; i++)
      if (threads[i].state == 1) {
        uint64_t current = 0;
        int error = ticks(threads[i].handle, &current);
        if (error || UINT64_MAX - total < current) {
          mutexUnlock(&lock);
          return error ? error : -EOVERFLOW;
        }
        total += current;
      }
    mutexUnlock(&lock);
  }
  uint64_t frequency = armGetSystemTickFreq();
  if (!frequency || total / frequency > UINT64_MAX / 1000000000 ||
      total % frequency > UINT64_MAX / 1000000000)
    return -EOVERFLOW;
  uint64_t seconds = total / frequency * 1000000000;
  uint64_t fraction = total % frequency * 1000000000 / frequency;
  if (UINT64_MAX - seconds < fraction) return -EOVERFLOW;
  *nanoseconds = seconds + fraction;
  return 0;
}
