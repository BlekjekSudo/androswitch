/* Android pthread keys/once use guest identities and real guest callbacks.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "bionic_errno.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#define KEY_COUNT 128
#define KEY_GENERATION_MAX (0x7fffffffu / KEY_COUNT)
typedef struct {
  unsigned generation;
  int active;
  uint64_t destructor;
} Key;
typedef struct {
  unsigned generation;
  void *value;
} Value;
typedef struct Once {
  int *address;
  uint64_t owner;
  struct Once *next;
} Once;
static Key keys[KEY_COUNT];
static __thread Value values[KEY_COUNT];
static Once *initializers;
static int key_create(unsigned *out, uint64_t destructor) {
  if (!out)
    return EINVAL;
  as_cxx_lock();
  for (unsigned i = 0; i < KEY_COUNT; ++i) {
    Key *key = &keys[i];
    if (!key->active && key->generation < KEY_GENERATION_MAX) {
      ++key->generation;
      key->active = 1;
      key->destructor = destructor;
      *out = key->generation * KEY_COUNT + i;
      as_cxx_unlock();
      return 0;
    }
  }
  as_cxx_unlock();
  return EAGAIN;
}
static Key *key_lookup(unsigned handle) {
  Key *key = &keys[handle % KEY_COUNT];
  return key->active && key->generation == handle / KEY_COUNT ? key : NULL;
}
static int key_delete(unsigned handle) {
  as_cxx_lock();
  Key *key = key_lookup(handle);
  int result = key ? 0 : EINVAL;
  if (key) {
    key->active = 0;
    key->destructor = 0;
  }
  as_cxx_unlock();
  return result;
}
static int key_set(unsigned handle, const void *value) {
  as_cxx_lock();
  Key *key = key_lookup(handle);
  int result = key ? 0 : EINVAL;
  if (key)
    values[handle % KEY_COUNT] = (Value){key->generation, (void *)value};
  as_cxx_unlock();
  return result;
}
static void *key_get(unsigned handle) {
  as_cxx_lock();
  Key *key = key_lookup(handle);
  Value *value = &values[handle % KEY_COUNT];
  void *result =
      key && key->generation == value->generation ? value->value : NULL;
  as_cxx_unlock();
  return result;
}
int as_bionic_keys_thread_finalize(void) {
  /* Android/POSIX allows four rounds when a destructor reassigns its key. */
  for (unsigned pass = 0; pass < 4; ++pass) {
    int called = 0;
    for (unsigned i = 0; i < KEY_COUNT; ++i) {
      as_cxx_lock();
      uint64_t function =
          keys[i].active && keys[i].generation == values[i].generation
              ? keys[i].destructor
              : 0;
      void *value = values[i].value;
      values[i].value = NULL;
      as_cxx_unlock();
      if (function && value) {
        int rc = as_cxx_invoke(function, (uint64_t)value);
        if (rc)
          return rc;
        called = 1;
      }
    }
    if (!called)
      break;
  }
  memset(values, 0, sizeof(values));
  return 0;
}
static int once(int *control, uint64_t function) {
  if (!control || !function)
    return EINVAL;
  as_cxx_lock();
  for (;;) {
    int state = __atomic_load_n(control, __ATOMIC_ACQUIRE);
    if (state == 2) {
      as_cxx_unlock();
      return 0;
    }
    if (state == 0) {
      Once *entry = malloc(sizeof(*entry));
      if (!entry) {
        as_cxx_unlock();
        return ENOMEM;
      }
      *entry = (Once){control, as_cxx_thread(), initializers};
      initializers = entry;
      __atomic_store_n(control, 1, __ATOMIC_RELEASE);
      as_cxx_unlock();
      int rc = as_cxx_invoke(function, 0);
      as_cxx_lock();
      Once **link = &initializers;
      while (*link && *link != entry)
        link = &(*link)->next;
      if (*link)
        *link = entry->next;
      free(entry);
      __atomic_store_n(control, rc ? 0 : 2, __ATOMIC_RELEASE);
      as_cxx_wake();
      as_cxx_unlock();
      if (rc)
        as_cxx_trap(rc);
      return 0;
    }
    Once *entry = initializers;
    while (entry && entry->address != control)
      entry = entry->next;
    if (state != 1 || !entry) {
      as_cxx_unlock();
      return EINVAL;
    }
    if (entry->owner == as_cxx_thread()) {
      as_cxx_unlock();
      return as_errno_to_android(EDEADLK);
    }
    as_cxx_wait();
  }
}
static int equal(uint64_t left, uint64_t right) { return left == right; }
unsigned as_bionic_keys_pending(void) {
  int saved = errno;
  unsigned pending = 0;
  as_cxx_lock();
  for (unsigned i = 0; i < KEY_COUNT; ++i)
    if (keys[i].active && keys[i].generation == values[i].generation &&
        keys[i].destructor && values[i].value) {
      pending = 1;
      break;
    }
  as_cxx_unlock();
  errno = saved;
  return pending;
}
int as_bionic_keys_reset(void) {
  as_cxx_lock();
  if (initializers) {
    as_cxx_unlock();
    return -EBUSY;
  }
  for (unsigned i = 0; i < KEY_COUNT; ++i) {
    keys[i].active = 0;
    keys[i].destructor = 0;
  }
  memset(values, 0, sizeof(values));
  as_cxx_unlock();
  return 0;
}
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
uint64_t as_bionic_keys_import(const char *name) {
  API("pthread_key_create", key_create);
  API("pthread_key_delete", key_delete);
  API("pthread_getspecific", key_get);
  API("pthread_setspecific", key_set);
  API("pthread_once", once);
  API("pthread_equal", equal);
  return 0;
}
