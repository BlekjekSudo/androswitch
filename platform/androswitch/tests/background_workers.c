/* Production transport against real pthreads, clock waits and event signals.
 * Framework queue semantics/GC and actual Horizon execution are separate. */
#define _POSIX_C_SOURCE 200809L
#include "../workers.h"
#include "worker_admission.h"
#include <assert.h>
#include <errno.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
#include <time.h>

enum { DELAY, SERIAL, IDLE, BLOCK, FAIL, SELF_JOIN, SELF_CANCEL, WAKE_DURING };
typedef struct {
  int mode;
  atomic_int attached, steps, detached, cleanup, running, max_running;
  atomic_int release, finish, cleanup_ms, transport_error;
  atomic_int spawn_in_cleanup, cleanup_spawn_result;
  uint64_t first_tick, second_tick;
  Handle handle;
  int self_result;
} Task;
static Task tasks[80];
static Thread *handles[128];
static unsigned next_handle = 2;
static atomic_int create_fail, start_fail, close_fail, join_fail, wait_fail;
static atomic_int sleep_fail, create_calls, close_calls, wait_entries;
static atomic_int quiesce_done;
static atomic_int hold_start, start_waiting, release_start;
static atomic_int shutdown_entered, shutdown_done, starter_done;
static int quiesce_result;
static uint64_t join_timeouts[128];
static _Thread_local Handle this_handle;
static _Thread_local uint64_t this_token;
static _Thread_local void *this_session;
static void pause_ms(unsigned milliseconds) {
  struct timespec duration = {milliseconds / 1000,
                              (long)(milliseconds % 1000) * 1000000};
  nanosleep(&duration, NULL);
}
uint64_t armGetSystemTick(void) {
  struct timespec now;
  assert(!clock_gettime(CLOCK_MONOTONIC, &now));
  return (uint64_t)now.tv_sec * 1000000000 + now.tv_nsec;
}
void mutexLock(Mutex *mutex) {
  while (__atomic_exchange_n(&mutex->locked, 1, __ATOMIC_ACQUIRE))
    pause_ms(1);
}
void mutexUnlock(Mutex *mutex) {
  __atomic_store_n(&mutex->locked, 0, __ATOMIC_RELEASE);
}
static void *entry(void *argument) {
  Thread *thread = argument;
  this_handle = thread->handle;
  thread->entry(thread->argument);
  __atomic_store_n(&thread->done, 1, __ATOMIC_RELEASE);
  return NULL;
}
Result threadCreate(Thread *thread, void (*function)(void *), void *argument,
                    void *stack, size_t size, int priority, int core) {
  assert(!stack && size == 1024 * 1024 && priority == 0x2c && core == -2);
  atomic_fetch_add(&create_calls, 1);
  if (atomic_exchange(&create_fail, 0))
    return 2;
  assert(next_handle < 128);
  memset(thread, 0, sizeof(*thread));
  thread->handle = next_handle++;
  thread->entry = function;
  thread->argument = argument;
  handles[thread->handle] = thread;
  return 0;
}
Result threadStart(Thread *thread) {
  if (atomic_exchange(&start_fail, 0))
    return 2;
  if (pthread_create(&thread->native, NULL, entry, thread))
    return 2;
  thread->started = 1;
  if (atomic_load(&hold_start)) {
    atomic_store(&start_waiting, 1);
    while (!atomic_load(&release_start))
      pause_ms(1);
  }
  return 0;
}
Result threadWaitForExit(Thread *thread) {
  if (atomic_exchange(&join_fail, 0))
    return 2;
  assert(thread->started);
  if (!thread->joined) {
    assert(!pthread_join(thread->native, NULL));
    thread->joined = 1;
  }
  return 0;
}
Result threadClose(Thread *thread) {
  if (atomic_exchange(&close_fail, 0))
    return 2;
  assert(thread->handle && (!thread->started || thread->joined));
  handles[thread->handle] = NULL;
  thread->handle = 0;
  atomic_fetch_add(&close_calls, 1);
  return 0;
}
Handle threadGetCurHandle(void) { return this_handle ? this_handle : 1; }
Result svcWaitSynchronization(int32_t *index, const Handle *values, int count,
                              int64_t timeout) {
  assert(count == 1 && values[0] < 128 && timeout >= 0);
  Thread *thread = handles[values[0]];
  assert(thread);
  if (timeout)
    join_timeouts[values[0]] = (uint64_t)timeout;
  if (timeout && atomic_exchange(&wait_fail, 0))
    return 1;
  uint64_t begin = armGetSystemTick();
  for (;;) {
    if (__atomic_load_n(&thread->done, __ATOMIC_ACQUIRE)) {
      *index = 0;
      return 0;
    }
    if (armGetSystemTick() - begin >= (uint64_t)timeout)
      return 1;
    pause_ms(1);
  }
}
void ueventCreate(UEvent *event, bool auto_clear) {
  assert(!auto_clear);
  __atomic_store_n(&event->signal, 0, __ATOMIC_RELEASE);
}
void ueventClear(UEvent *event) {
  __atomic_store_n(&event->signal, 0, __ATOMIC_RELEASE);
}
void ueventSignal(UEvent *event) {
  __atomic_store_n(&event->signal, 1, __ATOMIC_RELEASE);
}
Result waitSingle(Waiter waiter, uint64_t timeout) {
  assert(timeout && timeout <= 1000000000);
  atomic_fetch_add(&wait_entries, 1);
  if (atomic_exchange(&sleep_fail, 0))
    return 2;
  uint64_t begin = armGetSystemTick();
  for (;;) {
    if (__atomic_load_n(&waiter.event->signal, __ATOMIC_ACQUIRE))
      return 0;
    if (armGetSystemTick() - begin >= timeout)
      return 1;
    pause_ms(1);
  }
}
void as_native_set_session(void *session) { this_session = session; }
void as_native_thread_cleanup(void) {
  assert(this_token && this_session == (void *)3);
  Task *task = &tasks[this_token];
  assert(atomic_load(&task->attached) && !atomic_load(&task->detached));
  pause_ms((unsigned)atomic_load(&task->cleanup_ms));
  if (atomic_load(&task->spawn_in_cleanup)) {
    AsWorkerRequest request = {1, 2, 3, 2};
    atomic_store(&task->cleanup_spawn_result, as_workers_start(&request));
  }
  atomic_fetch_add(&task->cleanup, 1);
  this_session = NULL;
}
void as_media_stop_event(const char *operation, int result, uint64_t detail) {
  (void)detail;
  assert(!strcmp(operation, "worker_wait_end") ||
         !strcmp(operation, "worker_sleep_result") ||
         !strcmp(operation, "worker_detach_result"));
  assert(!result || result < 0);
}
int64_t as_application_worker_call(uint64_t vm, uint64_t android,
                                   uint64_t token, int event, int64_t detail) {
  assert(vm == 1 && android == 2 && token > 0 && token < 80);
  assert(this_session == (void *)3 && threadGetCurHandle() != 1);
  Task *task = &tasks[token];
  if (event == 0) {
    assert(!atomic_exchange(&task->attached, 1));
    task->handle = threadGetCurHandle();
    this_token = token;
    return 0;
  }
  assert(atomic_load(&task->attached));
  if (event == 2) {
    assert(atomic_load(&task->cleanup) == 1);
    assert(!atomic_exchange(&task->detached, 1));
    return 0;
  }
  if (event == 3) {
    assert(detail == 2);
    atomic_store(&task->transport_error, (int)detail);
    return -2300;
  }
  assert(event == 1 && !atomic_load(&task->detached));
  int active = atomic_fetch_add(&task->running, 1) + 1;
  assert(active == 1);
  atomic_store(&task->max_running, active);
  int step = atomic_fetch_add(&task->steps, 1) + 1;
  int64_t result = 60000;
  if (task->mode == DELAY) {
    if (step == 1) {
      task->first_tick = armGetSystemTick();
      result = 40;
    } else {
      task->second_tick = armGetSystemTick();
      result = -1;
    }
  } else if (task->mode == SERIAL)
    result = step < 5 ? 0 : -1;
  else if (task->mode == BLOCK) {
    while (!atomic_load(&task->release))
      pause_ms(1);
  } else if (task->mode == WAKE_DURING) {
    if (step == 1) {
      while (!atomic_load(&task->release))
        pause_ms(1);
    } else
      result = -1;
  } else if (task->mode == FAIL)
    result = -2300;
  else if (task->mode == SELF_JOIN) {
    task->self_result = as_workers_quiesce(1000000000);
    result = -1;
  } else if (task->mode == SELF_CANCEL) {
    task->self_result = as_workers_cancel(token);
  } else if (atomic_load(&task->finish))
    result = -1;
  atomic_fetch_sub(&task->running, 1);
  return result;
}
static void await(atomic_int *value, int minimum) {
  uint64_t begin = armGetSystemTick();
  while (atomic_load(value) < minimum) {
    assert(armGetSystemTick() - begin < 3000000000);
    pause_ms(1);
  }
}
static void start(unsigned token, int mode) {
  tasks[token].mode = mode;
  AsWorkerRequest request = {1, 2, 3, token};
  assert(!as_workers_start(&request));
}
static void closed(unsigned token) {
  assert(atomic_load(&tasks[token].attached) == 1);
  assert(atomic_load(&tasks[token].cleanup) == 1);
  assert(atomic_load(&tasks[token].detached) == 1);
  assert(!atomic_load(&tasks[token].running));
}
static void *join_all(void *unused) {
  (void)unused;
  quiesce_result = as_workers_quiesce(1000000000);
  atomic_store(&quiesce_done, 1);
  return NULL;
}
static void *start_held(void *unused) {
  (void)unused;
  start(1, IDLE);
  atomic_store(&starter_done, 1);
  return NULL;
}
static void *begin_shutdown(void *unused) {
  (void)unused;
  atomic_store(&shutdown_entered, 1);
  as_worker_admission_begin_shutdown();
  atomic_store(&shutdown_done, 1);
  return NULL;
}
int main(int argc, char **argv) {
  assert(argc == 2);
  const char *scenario = argv[1];
  AsWorkerRequest request = {1, 2, 3, 1};
  if (!strcmp(scenario, "admission-race")) {
    atomic_store(&hold_start, 1);
    pthread_t starter, shutdown;
    assert(!pthread_create(&starter, NULL, start_held, NULL));
    await(&start_waiting,
          1); /* actual OS thread exists; registration incomplete */
    assert(!atomic_load(&starter_done));
    assert(!pthread_create(&shutdown, NULL, begin_shutdown, NULL));
    await(&shutdown_entered, 1);
    assert(!atomic_load(&shutdown_done));
    atomic_store(&release_start, 1);
    assert(!pthread_join(starter, NULL));
    assert(!pthread_join(shutdown, NULL));
    assert(atomic_load(&starter_done) && atomic_load(&shutdown_done));
    request.token = 2;
    assert(as_workers_start(&request) == -ECANCELED);
    assert(atomic_load(&create_calls) == 1);
    assert(!as_workers_quiesce(1000000000));
    closed(1);
    assert(atomic_load(&close_calls) == 1);
    assert(as_worker_admission_enter() == -ECANCELED);
  } else if (!strcmp(scenario, "admission-closed")) {
    tasks[1].spawn_in_cleanup = 1;
    start(1, IDLE);
    await(&tasks[1].steps, 1);
    as_worker_admission_begin_shutdown();
    as_worker_admission_begin_shutdown();
    request.token = 2;
    assert(as_workers_start(&request) == -ECANCELED);
    assert(atomic_load(&create_calls) == 1);
    assert(!as_workers_quiesce(1000000000));
    closed(1);
    assert(atomic_load(&tasks[1].cleanup_spawn_result) == -ECANCELED);
    /* Joining/destructors cannot implicitly reopen admission. Only the
     * successful resource-reset endpoint is permitted to call reset. */
    assert(as_workers_start(&request) == -ECANCELED);
    assert(atomic_load(&create_calls) == 1);
    as_worker_admission_reset();
    start(3, IDLE);
    assert(!as_workers_quiesce(1000000000));
    closed(3);
    assert(atomic_load(&create_calls) == 2 && atomic_load(&close_calls) == 2);
  } else if (!strcmp(scenario, "invalid")) {
    assert(as_workers_start(NULL) == -EINVAL);
    for (unsigned i = 0; i < 4; i++) {
      AsWorkerRequest bad = request;
      ((uint64_t *)&bad)[i] = 0;
      assert(as_workers_start(&bad) == -EINVAL);
    }
    assert(as_workers_wake(0) == -EINVAL);
    assert(as_workers_cancel(0) == -EINVAL);
    assert(as_workers_wake(1) == -ENOENT);
    assert(as_workers_cancel(1) == -ENOENT);
    assert(as_workers_quiesce(0) == -EINVAL);
    assert(as_workers_quiesce(UINT64_MAX) == -EINVAL);
    assert(!atomic_load(&create_calls));
  } else if (!strcmp(scenario, "wall-clock")) {
    int64_t before, after;
    assert(as_platform_wall_millis(NULL) == -EINVAL);
    assert(!as_platform_wall_millis(&before));
    assert(before > INT64_C(1700000000000));
    time_t seconds = time(NULL);
    assert(before / 1000 >= (int64_t)seconds - 1 &&
           before / 1000 <= (int64_t)seconds + 1);
    pause_ms(5);
    assert(!as_platform_wall_millis(&after) && after >= before);
  } else if (!strcmp(scenario, "create-failure")) {
    atomic_store(&create_fail, 1);
    assert(as_workers_start(&request) == -EIO);
    assert(as_workers_wake(1) == -ENOENT);
    assert(!as_workers_quiesce(1000000000));
    assert(!atomic_load(&close_calls));
  } else if (!strcmp(scenario, "start-failure") ||
             !strcmp(scenario, "start-close-failure")) {
    atomic_store(&start_fail, 1);
    if (!strcmp(scenario, "start-close-failure"))
      atomic_store(&close_fail, 1);
    assert(as_workers_start(&request) == -EIO);
    assert(as_workers_wake(1) ==
           (!strcmp(scenario, "start-close-failure") ? -ECANCELED : -ENOENT));
    assert(!as_workers_quiesce(1000000000));
    assert(atomic_load(&close_calls) == 1);
    assert(!atomic_load(&tasks[1].attached));
  } else if (!strcmp(scenario, "capacity")) {
    for (unsigned i = 1; i <= 32; i++)
      start(i, IDLE);
    request.token = 33;
    assert(as_workers_start(&request) == -EAGAIN);
    assert(!as_workers_quiesce(2000000000));
    for (unsigned i = 1; i <= 32; i++)
      closed(i);
    start(34, IDLE);
    assert(!as_workers_quiesce(1000000000));
    closed(34);
    assert(atomic_load(&close_calls) == 33);
  } else if (!strcmp(scenario, "reap")) {
    start(1, SERIAL);
    await(&tasks[1].detached, 1);
    while (!__atomic_load_n(&handles[tasks[1].handle]->done, __ATOMIC_ACQUIRE))
      pause_ms(1);
    assert(as_workers_wake(1) == -ECANCELED);
    start(2, IDLE);
    assert(atomic_load(&close_calls) == 1);
    assert(!as_workers_quiesce(1000000000));
    closed(1);
    closed(2);
  } else if (!strcmp(scenario, "total-deadline")) {
    tasks[1].cleanup_ms = 35;
    tasks[2].cleanup_ms = 110;
    start(1, IDLE);
    start(2, IDLE);
    await(&tasks[1].steps, 1);
    await(&tasks[2].steps, 1);
    assert(as_workers_quiesce(75000000) == -EBUSY);
    assert(join_timeouts[tasks[2].handle] < join_timeouts[tasks[1].handle]);
    assert(join_timeouts[tasks[2].handle] < 60000000);
    assert(!as_workers_quiesce(1000000000));
    closed(1);
    closed(2);
  } else if (!strcmp(scenario, "concurrent-quiesce")) {
    start(1, BLOCK);
    await(&tasks[1].steps, 1);
    pthread_t joiner;
    assert(!pthread_create(&joiner, NULL, join_all, NULL));
    /* Observe actual wait initiation through the owned worker stop signal. */
    pause_ms(15);
    assert(!atomic_load(&quiesce_done));
    assert(as_workers_quiesce(1000000000) == -EBUSY);
    request.token = 2;
    assert(as_workers_start(&request) == -EBUSY);
    atomic_store(&tasks[1].release, 1);
    assert(!pthread_join(joiner, NULL));
    assert(!quiesce_result);
    closed(1);
  } else {
    int mode = IDLE;
    if (!strcmp(scenario, "normal-delay"))
      mode = DELAY;
    else if (!strcmp(scenario, "serial"))
      mode = SERIAL;
    else if (!strcmp(scenario, "cancel-active") || !strcmp(scenario, "timeout"))
      mode = BLOCK;
    else if (!strcmp(scenario, "callback-failure"))
      mode = FAIL;
    else if (!strcmp(scenario, "self-quiesce"))
      mode = SELF_JOIN;
    else if (!strcmp(scenario, "self-cancel"))
      mode = SELF_CANCEL;
    else if (!strcmp(scenario, "wake-during-callback"))
      mode = WAKE_DURING;
    else
      assert(!strcmp(scenario, "wake") || !strcmp(scenario, "cancel-idle") ||
             !strcmp(scenario, "wait-error") ||
             !strcmp(scenario, "join-failure") ||
             !strcmp(scenario, "close-failure"));
    if (!strcmp(scenario, "wait-error"))
      atomic_store(&sleep_fail, 1);
    start(1, mode);
    assert(as_workers_start(&request) == -EEXIST);
    await(&tasks[1].steps, 1);
    if (!strcmp(scenario, "wake-during-callback")) {
      uint64_t began = armGetSystemTick();
      assert(!as_workers_wake(1));
      atomic_store(&tasks[1].release, 1);
      await(&tasks[1].detached, 1);
      assert(armGetSystemTick() - began < 800000000);
      assert(atomic_load(&tasks[1].steps) == 2);
    } else if (!strcmp(scenario, "wake")) {
      await(&wait_entries, 1);
      uint64_t began = armGetSystemTick();
      atomic_store(&tasks[1].finish, 1);
      assert(!as_workers_wake(1));
      await(&tasks[1].detached, 1);
      assert(armGetSystemTick() - began < 800000000);
      assert(atomic_load(&tasks[1].steps) == 2);
    } else if (!strcmp(scenario, "cancel-idle")) {
      await(&wait_entries, 1);
      assert(!as_workers_cancel(1));
      assert(!as_workers_cancel(1));
    } else if (!strcmp(scenario, "cancel-active")) {
      assert(!as_workers_cancel(1));
      assert(atomic_load(&tasks[1].running) == 1);
      assert(!atomic_load(&tasks[1].detached));
      assert(as_workers_wake(1) == -ECANCELED);
      atomic_store(&tasks[1].release, 1);
    } else if (!strcmp(scenario, "timeout")) {
      assert(as_workers_quiesce(20000000) == -EBUSY);
      assert(!atomic_load(&close_calls));
      assert(!atomic_load(&tasks[1].detached));
      assert(as_workers_wake(1) == -ECANCELED);
      atomic_store(&tasks[1].release, 1);
    } else if (!strcmp(scenario, "join-failure")) {
      atomic_store(&join_fail, 1);
      assert(as_workers_quiesce(1000000000) == -EIO);
      assert(!atomic_load(&close_calls));
    } else if (!strcmp(scenario, "close-failure")) {
      atomic_store(&close_fail, 1);
      assert(as_workers_quiesce(1000000000) == -EIO);
      assert(!atomic_load(&close_calls));
    } else
      await(&tasks[1].detached, 1);
    assert(!as_workers_quiesce(1000000000));
    closed(1);
    assert(atomic_load(&close_calls) == 1);
    if (mode == DELAY) {
      assert(atomic_load(&tasks[1].steps) == 2);
      assert(tasks[1].second_tick - tasks[1].first_tick >= 35000000);
    } else if (mode == SERIAL)
      assert(atomic_load(&tasks[1].steps) == 5 &&
             atomic_load(&tasks[1].max_running) == 1);
    else if (mode == SELF_JOIN)
      assert(tasks[1].self_result == -EBUSY);
    else if (mode == SELF_CANCEL)
      assert(!tasks[1].self_result);
    else if (!strcmp(scenario, "wait-error"))
      assert(atomic_load(&tasks[1].transport_error) == 2);
  }
  puts("background worker transport: passed (host thread/clock contract only)");
  return 0;
}
