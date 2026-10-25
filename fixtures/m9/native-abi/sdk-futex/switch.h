#pragma once
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <time.h>
typedef uint64_t u64;
typedef unsigned Result;
typedef struct {
  pthread_mutex_t mutex;
  int ready;
} Mutex;
typedef struct {
  pthread_cond_t condition;
  int ready;
} CondVar;
#define R_FAILED(v) ((v) != 0)
#define R_VALUE(v) (v)
#define KERNELRESULT(name) 1u
static pthread_mutex_t init_lock = PTHREAD_MUTEX_INITIALIZER;
static inline void mutexLock(Mutex *m) {
  assert(!pthread_mutex_lock(&init_lock));
  if (!m->ready) {
    assert(!pthread_mutex_init(&m->mutex, NULL));
    m->ready = 1;
  }
  assert(!pthread_mutex_unlock(&init_lock));
  assert(!pthread_mutex_lock(&m->mutex));
}
static inline void mutexUnlock(Mutex *m) {
  assert(!pthread_mutex_unlock(&m->mutex));
}
static inline void initialize_condition(CondVar *c) {
  if (!c->ready) {
    assert(!pthread_cond_init(&c->condition, NULL));
    c->ready = 1;
  }
}
static inline Result condvarWakeAll(CondVar *c) {
  initialize_condition(c);
  return pthread_cond_broadcast(&c->condition) ? 2 : 0;
}
static inline Result condvarWaitTimeout(CondVar *c, Mutex *m, u64 ns) {
  initialize_condition(c);
  int result;
  if (ns == UINT64_MAX)
    result = pthread_cond_wait(&c->condition, &m->mutex);
  else {
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    uint64_t n = (uint64_t)t.tv_nsec + ns;
    t.tv_sec += n / 1000000000;
    t.tv_nsec = n % 1000000000;
    result = pthread_cond_timedwait(&c->condition, &m->mutex, &t);
  }
  return result == ETIMEDOUT ? 1 : result ? 2 : 0;
}
