/* Real shared C++/pthread callback coordination, no application-specific data.
 * Original AndroSwitch fixture, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "bionic_finalize.h"
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
extern uint64_t as_bionic_keys_import(const char *);
extern int as_bionic_keys_reset(void);
static pthread_mutex_t registry = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t condition = PTHREAD_COND_INITIALIZER;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&registry)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&registry)); }
void as_cxx_wait(void) { assert(!pthread_cond_wait(&condition, &registry)); }
void as_cxx_wake(void) { assert(!pthread_cond_broadcast(&condition)); }
uint64_t as_cxx_thread(void) { return (uint64_t)pthread_self(); }
void *as_cxx_allocate(size_t size) { return malloc(size); }
void as_cxx_deallocate(void *pointer) { free(pointer); }
void as_cxx_trap(int error) {
  (void)error;
  abort();
}
static int callback_error;
int as_cxx_invoke(uint64_t function, uint64_t argument) {
  ((void (*)(uint64_t))(uintptr_t)function)(argument);
  int result = callback_error;
  callback_error = 0;
  return result;
}
static int (*register_global)(uint64_t, uint64_t, uint64_t);
static int (*register_thread)(uint64_t, uint64_t, uint64_t);
static int (*key_create)(unsigned *, uint64_t);
static int (*key_set)(unsigned, const void *);
static unsigned events[512], event_count;
static int resources_live = 1;
static void record(uint64_t value) {
  assert(resources_live && event_count < sizeof(events) / sizeof(events[0]));
  events[event_count++] = (unsigned)value;
}
static void global_registers_thread(uint64_t value) {
  record(value);
  assert(!register_thread((uint64_t)record, value + 1, 0));
}
static unsigned chain_end;
static int runaway;
static void alternating(uint64_t value) {
  record(value);
  if (runaway || value < chain_end) {
    int (*registrar)(uint64_t, uint64_t, uint64_t) =
        (value + 1) & 1 ? register_thread : register_global;
    assert(!registrar((uint64_t)alternating, value + 1, 0));
  }
}
static void thread_registers_thread(uint64_t value) {
  record(value);
  assert(!register_thread((uint64_t)record, value + 1, 0));
  assert(!register_thread((uint64_t)record, value + 2, 0));
}
static void key_registers_thread(uint64_t value) {
  record(value);
  assert(!register_thread((uint64_t)record, value + 1, 0));
}
static unsigned key;
static void global_sets_key(uint64_t value) {
  record(value);
  assert(!key_set(key, (void *)(uintptr_t)(value + 1)));
}
static void reenter(uint64_t value) {
  record(value);
  assert(!as_bionic_finalize_session());
  record(value + 1);
}
static void fail(uint64_t value) {
  record(value);
  callback_error = -1234;
}
int main(void) {
  register_global = (void *)as_bionic_cxx_import("__cxa_atexit");
  register_thread = (void *)as_bionic_cxx_import("__cxa_thread_atexit_impl");
  key_create = (void *)as_bionic_keys_import("pthread_key_create");
  key_set = (void *)as_bionic_keys_import("pthread_setspecific");
  assert(register_global && register_thread && key_create && key_set);
  /* Reproduce the old ordering exactly: initial TLS drain, global dtors,
   * then reset. The late TLS callback must remain owned and block reset. */
  assert(!register_global((uint64_t)global_registers_thread, 10, 0));
  assert(!as_bionic_cxx_thread_finalize());
  assert(!as_bionic_cxx_finalize());
  assert(event_count == 1 && events[0] == 10);
  assert(as_bionic_cxx_pending() == AS_CXX_PENDING_THREAD);
  assert(as_bionic_cxx_reset() == -EBUSY);
  assert(!as_bionic_finalize_session());
  assert(event_count == 2 && events[1] == 11 &&
         !as_bionic_finalization_pending());
  assert(!as_bionic_cxx_reset());
  /* The runtime coordinator invokes both callbacks while their state lives. */
  event_count = 0;
  assert(!register_global((uint64_t)global_registers_thread, 20, 0));
  assert(!as_bionic_finalize_session());
  assert(event_count == 2 && events[0] == 20 && events[1] == 21);
  /* TLS can register globals, which can register more TLS. */
  event_count = 0;
  chain_end = 12;
  assert(!register_global((uint64_t)alternating, 0, 0));
  assert(!as_bionic_finalize_session() && event_count == 13);
  for (unsigned i = 0; i < 13; ++i)
    assert(events[i] == i);
  event_count = 0;
  assert(!register_thread((uint64_t)thread_registers_thread, 30, 0));
  assert(!as_bionic_finalize_thread());
  assert(event_count == 3 && events[0] == 30 && events[1] == 32 &&
         events[2] == 31);
  /* Key destructor creates another C++ thread destructor after its first
   * drain. Global-finalizer-created key data must also be finalized. */
  event_count = 0;
  assert(!key_create(&key, (uint64_t)key_registers_thread));
  assert(!register_global((uint64_t)global_sets_key, 40, 0));
  assert(!as_bionic_finalize_session());
  assert(event_count == 3 && events[0] == 40 && events[1] == 41 &&
         events[2] == 42);
  assert(!as_bionic_finalization_pending());
  /* A reentrant finalization call cannot execute a popped entry twice. */
  event_count = 0;
  assert(!register_thread((uint64_t)record, 53, 0));
  assert(!register_global((uint64_t)record, 52, 0));
  assert(!register_global((uint64_t)reenter, 50, 0));
  assert(!as_bionic_finalize_session());
  assert(event_count == 4 && events[0] == 50 && events[1] == 52 &&
         events[2] == 53 && events[3] == 51);
  /* Error propagation retains uncalled entries. No callback/owner clearing. */
  event_count = 0;
  assert(!register_global((uint64_t)record, 61, 0));
  assert(!register_global((uint64_t)fail, 60, 0));
  assert(as_bionic_finalize_session() == -1234);
  assert(event_count == 1 && events[0] == 60 &&
         as_bionic_cxx_pending() == AS_CXX_PENDING_GLOBAL);
  assert(as_bionic_cxx_reset() == -EBUSY);
  assert(!as_bionic_finalize_session() && event_count == 2 && events[1] == 61);
  /* Cross-registry runaway is bounded. Remaining work survives failure;
   * this test turns off its generator before deliberately resuming it. */
  event_count = 0;
  runaway = 1;
  assert(!register_global((uint64_t)alternating, 0, 0));
  assert(as_bionic_finalize_session() == -ELOOP);
  assert(event_count == 2 * AS_FINALIZE_PASSES);
  assert(as_bionic_cxx_pending() == AS_CXX_PENDING_GLOBAL &&
         as_bionic_cxx_reset() == -EBUSY);
  runaway = 0;
  chain_end = 0;
  assert(!as_bionic_finalize_session() && !as_bionic_finalization_pending());
  assert(!as_bionic_cxx_reset() && !as_bionic_keys_reset());
  resources_live = 0;
  puts("native finalization contracts: PASS (late TLS, cross-registration, "
       "keys, reentrancy, errors, bounds)");
}
