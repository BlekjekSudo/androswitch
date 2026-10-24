/* This runs the production GLSurface worker against real host threads and a
 * fake EGL/libnx boundary. It proves ownership/ordering, not Horizon rendering.
 * Every scenario is a separate process, including intentional fail-closed EGL
 * failures whose process state must never be reused. */
#define _POSIX_C_SOURCE 200809L
#include "../glsurface.h"
#include "worker_admission.h"
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
#include <time.h>

static struct {
  int states[12], next_state, events[64], event_count;
  int create_fail, start_fail, wait_fail, close_thread_fail, force_timeout;
  int initialize_fail, choose_fail, egl_create_fail, egl_close_fail;
  int egl_suspend_fail, egl_resume_fail, suspend_calls, resume_calls;
  int callback_fail, callback_error, poll_error, block_draw, release_draw;
  int draw_entered, wait_entered, worker_done, attached, cleared;
  int create_calls, initialize_calls, egl_close_calls, thread_close_calls;
  int thread_close_attempts, swaps, failure, cleanup_calls, self_join;
  int self_join_result;
  int width, height;
  int slow_termination, termination_entered;
  int wait_result, wait_raw_result;
  int64_t wait_timeout;
} test;
static Thread *worker;
static _Thread_local Handle current_handle;
static _Thread_local void *current_session;
static void delay(void) {
  struct timespec pause = {0, 1000000};
  nanosleep(&pause, NULL);
}
void mutexLock(Mutex *mutex) {
  while (__atomic_exchange_n(&mutex->locked, 1, __ATOMIC_ACQUIRE))
    delay();
}
void mutexUnlock(Mutex *mutex) {
  __atomic_store_n(&mutex->locked, 0, __ATOMIC_RELEASE);
}
static void *entry(void *argument) {
  Thread *thread = argument;
  current_handle = thread->handle;
  thread->entry(thread->argument);
  __atomic_store_n(&thread->done, 1, __ATOMIC_RELEASE);
  __atomic_store_n(&test.worker_done, 1, __ATOMIC_RELEASE);
  return NULL;
}
Result threadCreate(Thread *thread, void (*function)(void *), void *argument,
                    void *stack, size_t size, int priority, int core) {
  (void)stack;
  (void)size;
  (void)priority;
  (void)core;
  if (test.create_fail)
    return 1;
  memset(thread, 0, sizeof(*thread));
  thread->handle = 7;
  thread->entry = function;
  thread->argument = argument;
  worker = thread;
  return 0;
}
Result threadStart(Thread *thread) {
  if (test.start_fail)
    return 1;
  thread->started = 1;
  return pthread_create(&thread->native, NULL, entry, thread) == 0 ? 0 : 1;
}
Result threadWaitForExit(Thread *thread) {
  if (__atomic_exchange_n(&test.wait_fail, 0, __ATOMIC_ACQ_REL))
    return 1;
  if (thread->joined)
    return 0;
  assert(thread->started);
  if (pthread_join(thread->native, NULL))
    return 1;
  thread->joined = 1;
  return 0;
}
Result threadClose(Thread *thread) {
  test.thread_close_attempts++;
  if (__atomic_exchange_n(&test.close_thread_fail, 0, __ATOMIC_ACQ_REL))
    return 1;
  assert(thread->handle && (!thread->started || thread->joined));
  thread->handle = 0;
  test.thread_close_calls++;
  worker = NULL;
  return 0;
}
Handle threadGetCurHandle(void) { return current_handle ? current_handle : 1; }
Result svcWaitSynchronization(int32_t *index, const Handle *handles, int count,
                              int64_t timeout) {
  assert(count == 1 && worker && handles[0] == worker->handle && timeout > 0);
  __atomic_store_n(&test.wait_entered, 1, __ATOMIC_RELEASE);
  test.wait_timeout = timeout;
  if (__atomic_exchange_n(&test.force_timeout, 0, __ATOMIC_ACQ_REL))
    return 1;
  struct timespec start, now;
  assert(!clock_gettime(CLOCK_MONOTONIC, &start));
  for (;;) {
    if (__atomic_load_n(&worker->done, __ATOMIC_ACQUIRE)) {
      *index = 0;
      return 0;
    }
    assert(!clock_gettime(CLOCK_MONOTONIC, &now));
    int64_t elapsed = (int64_t)(now.tv_sec - start.tv_sec) * 1000000000 +
                      now.tv_nsec - start.tv_nsec;
    if (elapsed >= timeout)
      break;
    delay();
  }
  return 1;
}
Result svcSleepThread(int64_t timeout) {
  (void)timeout;
  delay();
  return 0;
}
void as_native_set_session(void *session) {
  current_session = session;
  if (!session)
    test.cleared = 1;
}
void as_native_thread_cleanup(void) {
  /* Actual native.c clears context.session after running guest destructors. */
  assert(current_session);
  if (test.attached)
    assert(test.events[test.event_count - 1] != 5);
  test.events[test.event_count++] = 10;
  test.cleanup_calls++;
  current_session = NULL;
}
void as_teardown_phase(const char *phase) {
  assert(!strcmp(phase, "glsurface_join"));
}
int as_application_gl_call(uint64_t vm, uint64_t android, uint32_t view,
                           int event, int width, int height, uint64_t context,
                           uint64_t config) {
  assert(vm == 1 && android == 2 && view == 4 && current_session == (void *)3);
  assert(test.event_count < 64);
  test.events[test.event_count++] = event;
  if (event == test.callback_fail)
    return test.callback_error;
  if (event == 0)
    test.attached = 1;
  if (event == 5) {
    assert(test.cleanup_calls == 1);
    test.attached = 0;
  }
  if (event >= 1 && event <= 3)
    assert(context == 42 && config == 77 && width == test.width &&
           height == test.height);
  if (event == 6)
    assert(!context && !config && !width && !height);
  if (event == 3) {
    if (test.self_join)
      test.self_join_result = as_glsurface_quiesce();
    __atomic_store_n(&test.draw_entered, 1, __ATOMIC_RELEASE);
    while (test.block_draw &&
           !__atomic_load_n(&test.release_draw, __ATOMIC_ACQUIRE))
      delay();
  }
  return 0;
}
int as_application_gl_poll(uint64_t vm, uint64_t android, uint32_t view) {
  assert(vm == 1 && android == 2 && view == 4 && test.attached);
  if (test.poll_error)
    return test.poll_error;
  assert(test.next_state < 12);
  return test.states[test.next_state++];
}
uint64_t as_application_gl_choose(uint64_t vm, uint64_t android, uint32_t view,
                                  uint64_t display) {
  assert(vm == 1 && android == 2 && view == 4 && display == 99);
  return test.choose_fail ? 0 : 77;
}
int as_media_egl_initialize(AsEglSurface *surface) {
  test.initialize_calls++;
  surface->display = 99;
  return test.initialize_fail ? -501 : 0;
}
int as_media_egl_create(AsEglSurface *surface, uint64_t config, int version) {
  assert(surface->display == 99 && config == 77 && version == 2);
  test.create_calls++;
  surface->context = 42;
  surface->surface = 43;
  surface->config = 77;
  surface->width = test.width;
  surface->height = test.height;
  return test.egl_create_fail ? -502 : 0;
}
int as_media_egl_swap(AsEglSurface *surface) {
  assert(surface->context == 42 && surface->surface == 43);
  test.swaps++;
  return 0;
}
int as_media_egl_suspend(AsEglSurface *surface) {
  assert(surface->context == 42 && surface->surface == 43);
  test.suspend_calls++;
  if (test.egl_suspend_fail)
    return -506;
  surface->surface = 0;
  return 0;
}
int as_media_egl_resume(AsEglSurface *surface) {
  assert(surface->context == 42 && !surface->surface && surface->display == 99);
  test.resume_calls++;
  surface->surface = 43;
  return test.egl_resume_fail ? -507 : 0;
}
int as_media_egl_close(AsEglSurface *surface) {
  assert(surface->display == 99);
  test.egl_close_calls++;
  if (test.egl_close_fail)
    return -503;
  if (test.slow_termination) {
    __atomic_store_n(&test.termination_entered, 1, __ATOMIC_RELEASE);
    /* A real host worker remains owned for longer than the old two-second
     * cutoff, including its EGL teardown and later TLS/detach callbacks. */
    struct timespec pause = {2, 200000000};
    assert(!nanosleep(&pause, NULL));
  }
  memset(surface, 0, sizeof(*surface));
  return 0;
}
void as_media_gl_fail(int code) { test.failure = code; }
void as_media_stop_event(const char *operation, int result, uint64_t detail) {
  if (!strcmp(operation, "glsurface_wait_end")) {
    test.wait_result = result;
    test.wait_raw_result = (int)detail;
  }
}
static void wait_for(int *flag) {
  for (int attempt = 0; attempt < 2000; attempt++) {
    if (__atomic_load_n(flag, __ATOMIC_ACQUIRE))
      return;
    delay();
  }
  assert(!"host worker did not reach the expected boundary");
}
static int count_event(int event) {
  int count = 0;
  for (int i = 0; i < test.event_count; i++)
    count += test.events[i] == event;
  return count;
}
static const AsGlSurfaceRequest request = {1, 2, 3, 4, 2};
static void *joiner(void *unused) {
  (void)unused;
  return (void *)(intptr_t)as_glsurface_quiesce();
}
int main(int argc, char **argv) {
  assert(argc == 2);
  const char *scenario = argv[1];
  test.width = !strncmp(scenario, "portrait", 8) ? 720 : 1280;
  test.height = !strncmp(scenario, "portrait", 8) ? 1280 : 720;
  int preserve = !strcmp(scenario, "pause-preserve") ||
                 !strcmp(scenario, "portrait-pause-preserve");
  test.callback_fail = -1;
  test.callback_error = -504;
  test.states[0] = 1 | 4;
  if (!strcmp(scenario, "admission-closed")) {
    as_worker_admission_begin_shutdown();
    assert(as_glsurface_start(&request) == -ECANCELED);
    assert(!as_glsurface_running() && !test.create_calls);
    assert(!as_glsurface_quiesce());
    assert(as_glsurface_start(&request) == -ECANCELED);
    as_worker_admission_reset();
    scenario = "normal"; /* normal complete lifecycle after admitted reset */
  }
  if (!strcmp(scenario, "invalid")) {
    AsGlSurfaceRequest invalid = request;
    invalid.client_version = 3;
    assert(as_glsurface_start(NULL) == -EINVAL);
    assert(as_glsurface_start(&invalid) == -EINVAL);
    assert(!as_glsurface_running());
    return 0;
  }
  if (!strcmp(scenario, "create-failure"))
    test.create_fail = 1;
  if (!strcmp(scenario, "start-failure") ||
      !strcmp(scenario, "start-close-failure"))
    test.start_fail = 1;
  if (!strcmp(scenario, "start-close-failure"))
    test.close_thread_fail = 1;
  if (test.create_fail || test.start_fail) {
    assert(as_glsurface_start(&request) == -EIO);
    assert(as_glsurface_running() == !strcmp(scenario, "start-close-failure"));
    assert(as_glsurface_quiesce() == 0);
    assert(test.thread_close_calls == !test.create_fail);
    assert(!as_glsurface_running() && !test.event_count);
    return 0;
  }
  if (preserve || !strcmp(scenario, "suspend-failure") ||
      !strcmp(scenario, "resume-failure")) {
    test.states[1] = 1 | 2 | 16;
    test.states[2] = 1 | 4;
    test.egl_suspend_fail = !strcmp(scenario, "suspend-failure");
    test.egl_resume_fail = !strcmp(scenario, "resume-failure");
  } else if (!strcmp(scenario, "pause-before-create")) {
    test.states[0] = 1 | 2 | 16;
    test.states[1] = 1 | 4;
  } else if (!strcmp(scenario, "pause-preserve-exit")) {
    test.states[1] = 1 | 2 | 16;
  } else if (!strcmp(scenario, "pause-toggle-discard")) {
    test.states[1] = 1 | 2 | 16;
    test.states[2] = 1 | 2;
    test.states[3] = 1 | 4;
  } else if (!strcmp(scenario, "pause-recreate")) {
    test.states[1] = 1 | 2;
    test.states[2] = 1 | 4;
  } else if (!strcmp(scenario, "pause-exit"))
    test.states[1] = 1 | 2;
  else if (!strcmp(scenario, "queue-before-context"))
    test.states[0] = 1 | 8 | 4;
  else if (!strcmp(scenario, "initialize-failure"))
    test.initialize_fail = 1;
  else if (!strcmp(scenario, "choose-failure"))
    test.choose_fail = 1;
  else if (!strcmp(scenario, "egl-create-failure"))
    test.egl_create_fail = 1;
  else if (!strcmp(scenario, "attach-failure"))
    test.callback_fail = 0;
  else if (!strcmp(scenario, "callback-failure"))
    test.callback_fail = 1;
  else if (!strcmp(scenario, "queue-failure")) {
    test.states[0] = 1 | 8;
    test.callback_fail = 4;
  } else if (!strcmp(scenario, "poll-failure"))
    test.poll_error = -505;
  else if (!strcmp(scenario, "close-failure"))
    test.egl_close_fail = 1;
  else if (!strcmp(scenario, "self-join"))
    test.self_join = 1;
  else if (!strcmp(scenario, "wait-failure"))
    test.wait_fail = 1;
  else if (!strcmp(scenario, "thread-close-failure"))
    test.close_thread_fail = 1;
  else if (!strcmp(scenario, "portrait-slow-termination"))
    test.slow_termination = 1;
  else if (!strcmp(scenario, "timeout") || !strcmp(scenario, "concurrent-join"))
    test.block_draw = 1;
  else
    assert(!strcmp(scenario, "normal") || !strcmp(scenario, "portrait"));
  assert(as_glsurface_start(&request) == 0);
  if (test.block_draw) {
    wait_for(&test.draw_entered);
    assert(as_glsurface_start(&request) == -EBUSY);
    if (!strcmp(scenario, "concurrent-join")) {
      pthread_t joining;
      void *result;
      assert(pthread_create(&joining, NULL, joiner, NULL) == 0);
      wait_for(&test.wait_entered);
      assert(as_glsurface_quiesce() == -EBUSY);
      __atomic_store_n(&test.release_draw, 1, __ATOMIC_RELEASE);
      assert(pthread_join(joining, &result) == 0 && (intptr_t)result == 0);
    } else {
      test.force_timeout = 1;
      assert(as_glsurface_quiesce() == -EBUSY);
      assert(test.wait_result == -EBUSY && test.wait_raw_result == 1);
      assert(as_glsurface_running() && as_glsurface_start(&request) == -EBUSY);
      __atomic_store_n(&test.release_draw, 1, __ATOMIC_RELEASE);
      assert(as_glsurface_quiesce() == 0);
    }
  } else if (test.slow_termination) {
    wait_for(&test.termination_entered);
    assert(as_glsurface_running());
    assert(as_glsurface_quiesce() == 0);
    assert(test.wait_timeout == 6000000000);
    assert(!test.wait_result && !test.wait_raw_result);
    assert(count_event(5) == 1 && count_event(6) == 1 && !test.failure);
  } else {
    wait_for(&test.worker_done);
    int result = as_glsurface_quiesce();
    if (test.egl_close_fail || test.egl_suspend_fail) {
      int expected = test.egl_suspend_fail ? -506 : -503;
      assert(result == expected && as_glsurface_quiesce() == expected);
      assert(as_glsurface_start(&request) == -EIO);
      assert(test.egl_close_calls == !test.egl_suspend_fail);
      assert(count_event(6) == 0);
    } else if (!strcmp(scenario, "wait-failure") ||
               !strcmp(scenario, "thread-close-failure")) {
      assert(result == -EIO && as_glsurface_running());
      assert(as_glsurface_start(&request) == -EBUSY);
      assert(as_glsurface_quiesce() == 0);
    } else
      assert(result == 0);
  }
  assert(!as_glsurface_running() && test.thread_close_calls == 1);
  assert(test.cleanup_calls == 1 && test.cleared && !test.attached);
  assert(count_event(0) == 1 && count_event(5) == (test.callback_fail != 0));
  if (!strcmp(scenario, "normal") || !strcmp(scenario, "portrait") ||
      test.self_join || test.block_draw) {
    assert(test.swaps == 1 && count_event(6) == 1 && !test.failure);
    assert(test.events[test.event_count - 2] == 10 &&
           test.events[test.event_count - 1] == 5);
  }
  if (test.self_join)
    assert(test.self_join_result == -EBUSY);
  if (preserve)
    assert(test.create_calls == 1 && count_event(6) == 1 && test.swaps == 2);
  if (preserve)
    assert(test.suspend_calls == 1 && test.resume_calls == 1 &&
           count_event(1) == 1 && count_event(2) == 2);
  if (!strcmp(scenario, "pause-before-create"))
    assert(test.create_calls == 1 && !test.suspend_calls &&
           !test.resume_calls && test.swaps == 1);
  if (!strcmp(scenario, "pause-preserve-exit"))
    assert(test.create_calls == 1 && test.suspend_calls == 1 &&
           !test.resume_calls && count_event(6) == 1);
  if (!strcmp(scenario, "pause-toggle-discard"))
    assert(test.create_calls == 2 && test.suspend_calls == 1 &&
           !test.resume_calls && count_event(6) == 2 && test.swaps == 2);
  if (test.egl_resume_fail)
    assert(test.suspend_calls == 1 && test.resume_calls == 1 &&
           test.failure == -507 && count_event(6) == 1 && test.swaps == 1);
  if (!strcmp(scenario, "pause-recreate"))
    assert(test.create_calls == 2 && count_event(6) == 2 && test.swaps == 2);
  if (!strcmp(scenario, "pause-exit"))
    assert(test.egl_close_calls == 1 && count_event(6) == 1);
  if (!strcmp(scenario, "queue-before-context"))
    assert(test.events[1] == 4 && test.events[2] == 1);
  if (test.initialize_fail || test.choose_fail)
    assert(!test.create_calls && count_event(6) == 0 &&
           test.egl_close_calls == 1 && test.failure);
  if (test.egl_create_fail || test.callback_fail == 1)
    assert(!test.swaps && count_event(6) == 1 && test.failure);
  if (test.callback_fail == 0 || test.callback_fail == 4 || test.poll_error)
    assert(!test.egl_close_calls && !test.create_calls && test.failure);
  printf("GLSurface worker %s: passed (host contract only)\n", scenario);
  return 0;
}
