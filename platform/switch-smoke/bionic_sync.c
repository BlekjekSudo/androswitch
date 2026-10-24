/* Opaque Android pthread mutex/cond addresses map to owned libnx objects.
 * Never reinterpret Bionic pthread layouts as newlib layouts.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_errno.h"
#include "native.h"
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
#include <time.h>
typedef struct Sync {
  const void *key;
  Mutex mutex;
  CondVar condition;
  int kind, type, users, clock;
  uint64_t owner;
  unsigned depth;
  struct Sync *next;
} Sync;
static Sync *objects;
static Mutex registry;
/* Registry mutex is held by all callers. Instances stay alive while a thread
 * owns or waits on them; POSIX destruction returns EBUSY in that state. */
static Sync *get(const void *key, int kind) {
  for (Sync *s = objects; s; s = s->next)
    if (s->key == key)
      return s->kind == kind ? s : NULL;
  if (!key)
    return NULL;
  unsigned count = 0;
  for (Sync *s = objects; s; s = s->next)
    count++;
  if (count >= 256)
    return NULL;
  uint32_t initial = *(const uint32_t *)key;
  if ((kind == 0 && (initial & ~0xc000u)) || (kind == 1 && initial))
    return NULL;
  Sync *s = calloc(1, sizeof(*s));
  if (!s)
    return NULL;
  s->key = key;
  s->kind = kind;
  s->type = initial & 0x4000 ? 1 : initial & 0x8000 ? 2 : 0;
  s->next = objects;
  objects = s;
  return s;
}
static int mutex_init(void *key, const int *attr) {
  if (!key)
    return EINVAL;
  if (attr && (*attr < 0 || *attr > 2))
    return as_errno_to_android(ENOTSUP);
  mutexLock(&registry);
  *(uint32_t *)key = 0;
  Sync *s = get(key, 0);
  int rc = !s ? EINVAL : s->users ? EBUSY : 0;
  if (!rc)
    s->type = attr ? *attr : 0;
  mutexUnlock(&registry);
  return rc;
}
static int mutex_lock(void *key) {
  mutexLock(&registry);
  Sync *s = get(key, 0);
  if (!s) {
    mutexUnlock(&registry);
    return EINVAL;
  }
  uint64_t self = threadGetCurHandle();
  if (s->owner == self) {
    if (s->type == 1) {
      s->depth++;
      mutexUnlock(&registry);
      return 0;
    }
    if (s->type == 2) {
      mutexUnlock(&registry);
      return as_errno_to_android(EDEADLK);
    }
  }
  s->users++;
  mutexUnlock(&registry);
  mutexLock(&s->mutex);
  mutexLock(&registry);
  s->owner = self;
  s->depth = 1;
  mutexUnlock(&registry);
  return 0;
}
static int mutex_trylock(void *key) {
  mutexLock(&registry);
  Sync *s = get(key, 0);
  if (!s) {
    mutexUnlock(&registry);
    return EINVAL;
  }
  uint64_t self = threadGetCurHandle();
  if (s->owner == self && s->type == 1) {
    s->depth++;
    mutexUnlock(&registry);
    return 0;
  }
  if (!mutexTryLock(&s->mutex)) {
    mutexUnlock(&registry);
    return EBUSY;
  }
  s->owner = self;
  s->depth = 1;
  s->users++;
  mutexUnlock(&registry);
  return 0;
}
static int mutex_unlock(void *key) {
  mutexLock(&registry);
  Sync *s = get(key, 0);
  if (!s || s->owner != threadGetCurHandle()) {
    mutexUnlock(&registry);
    return EPERM;
  }
  if (--s->depth == 0) {
    s->owner = 0;
    s->users--;
    mutexUnlock(&s->mutex);
  }
  mutexUnlock(&registry);
  return 0;
}
static int destroy(void *key, int kind) {
  mutexLock(&registry);
  Sync **p = &objects;
  while (*p && ((*p)->key != key || (*p)->kind != kind))
    p = &(*p)->next;
  int rc = 0;
  if (*p) {
    Sync *s = *p;
    if (s->users)
      rc = EBUSY;
    else {
      *p = s->next;
      free(s);
    }
  }
  mutexUnlock(&registry);
  return rc;
}
static int mutex_destroy(void *key) { return destroy(key, 0); }
static int cond_destroy(void *key) { return destroy(key, 1); }
static int attr_init(int *attr) {
  if (!attr)
    return EINVAL;
  *attr = 0;
  return 0;
}
static int attr_destroy(int *attr) { return attr ? 0 : EINVAL; }
static int attr_type(int *attr, int type) {
  if (!attr || type < 0 || type > 2)
    return EINVAL;
  *attr = type;
  return 0;
}
static int cond_init(void *key, const int *attr) {
  if (!key)
    return EINVAL;
  if (attr && (*attr < 0 || *attr > 1))
    return as_errno_to_android(ENOTSUP);
  mutexLock(&registry);
  *(uint32_t *)key = 0;
  Sync *s = get(key, 1);
  int rc = !s ? EINVAL : s->users ? EBUSY : 0;
  if (!rc)
    s->clock = attr ? *attr : 0;
  mutexUnlock(&registry);
  return rc;
}
static int attr_clock(int *attr, int clock) {
  if (!attr || clock < 0 || clock > 1)
    return EINVAL;
  *attr = clock;
  return 0;
}
static int cond_signal(void *key) {
  mutexLock(&registry);
  Sync *s = get(key, 1);
  if (s)
    condvarWakeOne(&s->condition);
  mutexUnlock(&registry);
  return s ? 0 : EINVAL;
}
static int cond_broadcast(void *key) {
  mutexLock(&registry);
  Sync *s = get(key, 1);
  if (s)
    condvarWakeAll(&s->condition);
  mutexUnlock(&registry);
  return s ? 0 : EINVAL;
}
static int cond_wait_internal(void *key, void *mutex,
                              const struct timespec *deadline) {
  mutexLock(&registry);
  Sync *c = get(key, 1), *m = get(mutex, 0);
  if (!c || !m || m->owner != threadGetCurHandle() || m->depth != 1) {
    mutexUnlock(&registry);
    return EINVAL;
  }
  uint64_t timeout = UINT64_MAX;
  if (deadline) {
    if (deadline->tv_sec < 0 || deadline->tv_nsec < 0 ||
        deadline->tv_nsec >= 1000000000) {
      mutexUnlock(&registry);
      return EINVAL;
    }
    struct timespec now;
    clock_gettime(c->clock == 1 ? CLOCK_MONOTONIC : CLOCK_REALTIME, &now);
    int64_t seconds = (int64_t)deadline->tv_sec - now.tv_sec;
    if (seconds > INT64_MAX / 1000000000) {
      mutexUnlock(&registry);
      return as_errno_to_android(EOVERFLOW);
    }
    int64_t delta = seconds * 1000000000 + deadline->tv_nsec - now.tv_nsec;
    timeout = delta > 0 ? (uint64_t)delta : 0;
  }
  c->users++;
  m->owner = 0;
  m->depth = 0;
  mutexUnlock(&registry);
  Result rc = condvarWaitTimeout(&c->condition, &m->mutex, timeout);
  mutexLock(&registry);
  c->users--;
  m->owner = threadGetCurHandle();
  m->depth = 1;
  mutexUnlock(&registry);
  return R_SUCCEEDED(rc) ? 0
         : R_DESCRIPTION(rc) == KernelError_TimedOut
             ? as_errno_to_android(ETIMEDOUT)
             : EINVAL;
}
static int cond_wait(void *c, void *m) {
  return cond_wait_internal(c, m, NULL);
}
static int cond_timedwait(void *c, void *m, const struct timespec *t) {
  return t ? cond_wait_internal(c, m, t) : EINVAL;
}
int as_bionic_sync_reset(void) {
  mutexLock(&registry);
  for (Sync *s = objects; s; s = s->next)
    if (s->users) {
      mutexUnlock(&registry);
      return EBUSY;
    }
  while (objects) {
    Sync *s = objects;
    objects = s->next;
    free(s);
  }
  mutexUnlock(&registry);
  return 0;
}
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
uint64_t as_bionic_sync_import(const char *name) {
  API("pthread_mutex_init", mutex_init);
  API("pthread_mutex_destroy", mutex_destroy);
  API("pthread_mutex_lock", mutex_lock);
  API("pthread_mutex_trylock", mutex_trylock);
  API("pthread_mutex_unlock", mutex_unlock);
  API("pthread_mutexattr_init", attr_init);
  API("pthread_mutexattr_destroy", attr_destroy);
  API("pthread_mutexattr_settype", attr_type);
  API("pthread_cond_init", cond_init);
  API("pthread_cond_destroy", cond_destroy);
  API("pthread_cond_signal", cond_signal);
  API("pthread_cond_broadcast", cond_broadcast);
  API("pthread_cond_wait", cond_wait);
  API("pthread_cond_timedwait", cond_timedwait);
  API("pthread_condattr_init", attr_init);
  API("pthread_condattr_destroy", attr_destroy);
  API("pthread_condattr_setclock", attr_clock);
  return 0;
}
