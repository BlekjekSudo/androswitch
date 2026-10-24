/* Original Horizon background callback transport, MIT OR Apache-2.0.
 * Scheduling policy and GC roots belong to the shared framework. */
#include "workers.h"
#include "media.h"
#include <errno.h>
#include <limits.h>
#include <string.h>
#include <switch.h>
#include <time.h>

/* Event0 attaches,1 polls/runs at most one serial framework task,2 detaches.
 * Step >=0 is relative wait milliseconds; -1 is graceful termination, <=-2
 * is an actual callback error already recorded by the VM adapter. */
extern int64_t as_application_worker_call(uint64_t, uint64_t, uint64_t, int,
                                          int64_t);
extern void as_native_thread_cleanup(void);
#define AS_WORKER_CAPACITY 32
typedef struct {
  Thread thread;
  UEvent wake;
  AsWorkerRequest request;
  int active, started, stopping, done;
} Worker;
static struct {
  Mutex mutex;
  Worker workers[AS_WORKER_CAPACITY];
  int joining;
} registry;

static int stopped(Worker *worker) {
  mutexLock(&registry.mutex);
  int value = worker->stopping;
  mutexUnlock(&registry.mutex);
  return value;
}
static void worker_main(void *argument) {
  Worker *worker = argument;
  AsWorkerRequest request = worker->request;
  as_native_set_session((void *)(uintptr_t)request.session);
  int64_t result = as_application_worker_call(request.vm, request.android,
                                              request.token, 0, 0);
  int attached = result == 0;
  while (!result && !stopped(worker)) {
    /* Clear before polling: a concurrent schedule/cancel signal during the
     * callback remains pending and cannot be lost before the wait. */
    ueventClear(&worker->wake);
    if (stopped(worker))
      break;
    result = as_application_worker_call(request.vm, request.android,
                                        request.token, 1, 0);
    if (result < 0)
      break;
    uint64_t delay = result > 1000 ? 1000 : (uint64_t)result;
    result = 0;
    if (delay && !stopped(worker)) {
      /* Relative sleeping uses the kernel's elapsed-time wait. Re-poll wall
       * time at least once a second, including after system-clock changes. */
      Result waited =
          waitSingle(waiterForUEvent(&worker->wake), (int64_t)delay * 1000000);
      if (R_FAILED(waited) && R_VALUE(waited) != KERNELRESULT(TimedOut)) {
        as_media_stop_event("worker_sleep_result", -EIO, waited);
        /* Distinguish an actual transport failure from ordinary timeout. */
        as_application_worker_call(request.vm, request.android, request.token,
                                   3, waited);
        break;
      }
    }
  }
  as_native_thread_cleanup(); /* guest TLS destructors may still enter DEX */
  if (attached) {
    as_native_set_session((void *)(uintptr_t)request.session);
    int64_t detached = as_application_worker_call(request.vm, request.android,
                                                  request.token, 2, 0);
    if (detached)
      as_media_stop_event("worker_detach_result", (int)detached, request.token);
  }
  as_native_set_session(NULL);
  mutexLock(&registry.mutex);
  worker->done = 1;
  mutexUnlock(&registry.mutex);
}
/* Called under the registry lock only after the entry point has finished all
 * owner access. Nonblocking kernel confirmation still precedes handle close. */
static int reap(Worker *worker) {
  if (!worker->active || !worker->done)
    return 0;
  int32_t index = -1;
  Handle handle = worker->thread.handle;
  if (R_FAILED(svcWaitSynchronization(&index, &handle, 1, 0)))
    return 0;
  if (R_FAILED(threadWaitForExit(&worker->thread)) ||
      R_FAILED(threadClose(&worker->thread)))
    return -EIO;
  memset(worker, 0, sizeof(*worker));
  return 0;
}
int as_workers_start(const AsWorkerRequest *request) {
  if (!request || !request->vm || !request->android || !request->session ||
      !request->token)
    return -EINVAL;
  int admission = as_worker_admission_enter();
  if (admission)
    return admission;
  mutexLock(&registry.mutex);
  if (registry.joining) {
    mutexUnlock(&registry.mutex);
    as_worker_admission_leave();
    return -EBUSY;
  }
  Worker *available = NULL;
  for (unsigned i = 0; i < AS_WORKER_CAPACITY; i++) {
    Worker *worker = &registry.workers[i];
    if (worker->active && worker->request.token == request->token) {
      mutexUnlock(&registry.mutex);
      as_worker_admission_leave();
      return -EEXIST;
    }
    int error = reap(worker);
    if (error) {
      mutexUnlock(&registry.mutex);
      as_worker_admission_leave();
      return error;
    }
    if (!worker->active && !available)
      available = worker;
  }
  if (!available) {
    mutexUnlock(&registry.mutex);
    as_worker_admission_leave();
    return -EAGAIN;
  }
  available->request = *request;
  ueventCreate(&available->wake, false);
  Result rc = threadCreate(&available->thread, worker_main, available, NULL,
                           1024 * 1024, 0x2c, -2);
  if (R_SUCCEEDED(rc)) {
    available->active = 1;
    rc = threadStart(&available->thread);
    if (R_SUCCEEDED(rc))
      available->started = 1;
    else if (R_SUCCEEDED(threadClose(&available->thread)))
      memset(available, 0, sizeof(*available));
  } else
    memset(available, 0, sizeof(*available));
  mutexUnlock(&registry.mutex);
  as_worker_admission_leave();
  return R_SUCCEEDED(rc) ? 0 : -EIO;
}
static int signal_worker(uint64_t token, int cancel) {
  if (!token)
    return -EINVAL;
  mutexLock(&registry.mutex);
  for (unsigned i = 0; i < AS_WORKER_CAPACITY; i++) {
    Worker *worker = &registry.workers[i];
    if (worker->active && worker->request.token == token) {
      if (!cancel && (!worker->started || worker->stopping || worker->done)) {
        mutexUnlock(&registry.mutex);
        return -ECANCELED;
      }
      if (cancel)
        worker->stopping = 1;
      ueventSignal(&worker->wake);
      mutexUnlock(&registry.mutex);
      return 0;
    }
  }
  mutexUnlock(&registry.mutex);
  return -ENOENT;
}
int as_workers_wake(uint64_t token) { return signal_worker(token, 0); }
int as_workers_cancel(uint64_t token) { return signal_worker(token, 1); }
int as_workers_quiesce(uint64_t timeout_ns) {
  if (!timeout_ns || timeout_ns > INT64_MAX)
    return -EINVAL;
  uint64_t start = armGetSystemTick();
  mutexLock(&registry.mutex);
  if (registry.joining) {
    mutexUnlock(&registry.mutex);
    return -EBUSY;
  }
  for (unsigned i = 0; i < AS_WORKER_CAPACITY; i++) {
    Worker *worker = &registry.workers[i];
    if (worker->active && worker->thread.handle == threadGetCurHandle()) {
      mutexUnlock(&registry.mutex);
      return -EBUSY;
    }
  }
  registry.joining = 1;
  for (unsigned i = 0; i < AS_WORKER_CAPACITY; i++) {
    Worker *worker = &registry.workers[i];
    if (worker->active) {
      worker->stopping = 1;
      ueventSignal(&worker->wake);
    }
  }
  mutexUnlock(&registry.mutex);
  int error = 0;
  for (unsigned i = 0; i < AS_WORKER_CAPACITY; i++) {
    Worker *worker = &registry.workers[i];
    if (!worker->active)
      continue;
    if (worker->started) {
      uint64_t elapsed = armTicksToNs(armGetSystemTick() - start);
      uint64_t remaining = elapsed < timeout_ns ? timeout_ns - elapsed : 0;
      Handle handle = worker->thread.handle;
      int32_t index = -1;
      Result waited =
          svcWaitSynchronization(&index, &handle, 1, (int64_t)remaining);
      as_media_stop_event("worker_wait_end", R_FAILED(waited) ? -EBUSY : 0,
                          waited);
      if (R_FAILED(waited)) {
        error = -EBUSY;
        break;
      }
      if (R_FAILED(threadWaitForExit(&worker->thread))) {
        error = -EIO;
        break;
      }
    }
    if (R_FAILED(threadClose(&worker->thread))) {
      error = -EIO;
      break;
    }
    mutexLock(&registry.mutex);
    memset(worker, 0, sizeof(*worker));
    mutexUnlock(&registry.mutex);
  }
  mutexLock(&registry.mutex);
  registry.joining = 0;
  mutexUnlock(&registry.mutex);
  return error;
}
int as_platform_wall_millis(int64_t *value) {
  if (!value)
    return -EINVAL;
  struct timespec now;
  if (clock_gettime(CLOCK_REALTIME, &now))
    return -EIO;
  if (now.tv_sec < 0 || (uint64_t)now.tv_sec > (uint64_t)INT64_MAX / 1000 ||
      now.tv_nsec < 0 || now.tv_nsec >= 1000000000)
    return -ERANGE;
  int64_t milliseconds = (int64_t)now.tv_sec * 1000;
  int64_t fraction = now.tv_nsec / 1000000;
  if (milliseconds > INT64_MAX - fraction)
    return -ERANGE;
  *value = milliseconds + fraction;
  return 0;
}
