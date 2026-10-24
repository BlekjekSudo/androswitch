/* Original common thread-start admission barrier, MIT OR Apache-2.0. */
#include "worker_admission.h"
#include <errno.h>
#include <switch.h>

static Mutex admission_mutex;
static int shutdown_started;
int as_worker_admission_enter(void) {
  mutexLock(&admission_mutex);
  if (shutdown_started) {
    mutexUnlock(&admission_mutex);
    return -ECANCELED;
  }
  return 0;
}
void as_worker_admission_leave(void) { mutexUnlock(&admission_mutex); }
void as_worker_admission_begin_shutdown(void) {
  mutexLock(&admission_mutex);
  shutdown_started = 1;
  mutexUnlock(&admission_mutex);
}
void as_worker_admission_reset(void) {
  mutexLock(&admission_mutex);
  shutdown_started = 0;
  mutexUnlock(&admission_mutex);
}
