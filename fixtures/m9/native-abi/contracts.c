/* General ABI contracts, no package or commercial payload. MIT OR Apache-2.0.
 */
#include "bionic_cxx.h"
#include "bionic_vfs.h"
#include "native.h"
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t condition = PTHREAD_COND_INITIALIZER;
static __thread jmp_buf *escape;
static __thread int trapped;
static int allocation_failure;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&lock)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&lock)); }
void as_cxx_wait(void) { assert(!pthread_cond_wait(&condition, &lock)); }
void as_cxx_wake(void) { assert(!pthread_cond_broadcast(&condition)); }
uint64_t as_cxx_thread(void) { return (uint64_t)pthread_self(); }
void *as_cxx_allocate(size_t n) {
  return allocation_failure ? NULL : malloc(n);
}
void as_cxx_deallocate(void *p) { free(p); }
int as_cxx_invoke(uint64_t f, uint64_t arg) {
  ((void (*)(uint64_t))f)(arg);
  return 0;
}
void as_cxx_trap(int error) {
  trapped = error;
  assert(escape);
  longjmp(*escape, 1);
}
void as_native_trap(int error) { as_cxx_trap(error); }
extern uint64_t as_bionic_libc_import(const char *name);
extern uint64_t as_bionic_keys_import(const char *name);
extern uint64_t as_bionic_vfs_import(const char *name);
extern uint64_t as_bionic_log_import(const char *name);
extern int as_bionic_keys_thread_finalize(void);
extern int as_bionic_keys_reset(void);
extern uint64_t as_bionic_io_import(const char *name);
extern uint64_t as_bionic_io_data(const char *name);
extern void as_bionic_io_reset(void);
void as_jni_vm_lock(void) {}
void as_jni_vm_unlock(void) {}
static unsigned char file_content[256];
static size_t file_length;
int64_t as_guest_fs(uint32_t op, const char *path, uint64_t offset,
                    uint8_t *bytes, size_t length, uint32_t flags) {
  if (op == 104) {
    uint64_t metadata[3] = {file_length,
                            !strcmp(path, "/sdcard/testdir") ? 1 : 0, 123};
    memcpy(bytes, metadata, sizeof(metadata));
    return 0;
  }
  if (op == 106) {
    assert(!strcmp(path, "/sdcard/testdir"));
    if (flags >= 2)
      return 0;
    memset(bytes, 0, length);
    strcpy((char *)bytes, flags ? "alpha" : "zebra");
    return 1;
  }
  if (op == 105) {
    file_length = 0;
    return 0;
  }
  if (op == 107)
    return !strcmp((char *)bytes, "/sdcard/destination.txt") ? 0 : -38;
  assert(!strcmp(path, "/sdcard/shared.txt"));
  if (op == 100) {
    if (flags & 4)
      file_length = 0;
    return flags & 8 ? file_length : 0;
  }
  if (op == 103)
    return file_length;
  if (offset > sizeof(file_content) || length > sizeof(file_content) - offset)
    return -EFBIG;
  if (op == 101) {
    if (offset >= file_length)
      return 0;
    if (length > file_length - offset)
      length = file_length - offset;
    memcpy(bytes, file_content + offset, length);
    return length;
  }
  if (op == 102) {
    memcpy(file_content + offset, bytes, length);
    if (file_length < offset + length)
      file_length = offset + length;
    return length;
  }
  return -ENOSYS;
}
static char logged_tag[256], logged_message[4096];
static int logged_priority;
int as_guest_log(int priority, const char *tag, const char *message) {
  logged_priority = priority;
  snprintf(logged_tag, sizeof(logged_tag), "%s", tag);
  snprintf(logged_message, sizeof(logged_message), "%s", message);
  return (int)strlen(message);
}
static int directory_filter(const AsGuestDirent *entry) {
  return entry->name[0] != '.';
}
int as_native_call(AsNativeCall *frame) {
  if (frame->function == (uint64_t)directory_filter)
    frame->result_integer[0] =
        directory_filter((void *)(uintptr_t)frame->integer[0]);
  else {
    assert(frame->function == as_bionic_vfs_import("alphasort"));
    int (*compare)(const AsGuestDirent **, const AsGuestDirent **) =
        (void *)(uintptr_t)frame->function;
    frame->result_integer[0] =
        (uint64_t)compare((void *)(uintptr_t)frame->integer[0],
                          (void *)(uintptr_t)frame->integer[1]);
  }
  return 0;
}
extern uint64_t as_native_missing_veneer(const char *, const char *);
extern const char *as_native_missing_library(void);
extern const char *as_native_missing_symbol(void);
extern void as_native_missing_reset(void);
typedef struct TestMapping {
  void *bytes;
  struct TestMapping *next;
} TestMapping;
static TestMapping *test_mappings;
int as_native_allocate(size_t size, uint64_t *address, uint64_t *token) {
  TestMapping *mapping = calloc(1, sizeof(*mapping));
  assert(mapping);
  mapping->bytes = calloc(1, size);
  assert(mapping->bytes);
  mapping->next = test_mappings;
  test_mappings = mapping;
  *address = (uint64_t)mapping->bytes;
  *token = (uint64_t)mapping;
  return 0;
}
int as_native_write(uint64_t token, size_t offset, const uint8_t *bytes,
                    size_t length) {
  TestMapping *mapping = (void *)(uintptr_t)token;
  assert(offset + length <= 4096);
  memcpy((char *)mapping->bytes + offset, bytes, length);
  return 0;
}
int as_native_finalize(uint64_t token, const uint8_t *pages, size_t count) {
  (void)token;
  assert(count == 1 && pages[0] == 1);
  return 0;
}
void as_native_release(uint64_t token) {
  TestMapping *mapping = (void *)(uintptr_t)token;
  TestMapping **link = &test_mappings;
  while (*link && *link != mapping)
    link = &(*link)->next;
  assert(*link);
  *link = mapping->next;
  free(mapping->bytes);
  free(mapping);
}
void as_native_import_bridge(void) {}
static int (*register_dtor)(uint64_t, uint64_t, uint64_t);
static void (*finalize)(uint64_t);
static int (*acquire)(uint64_t *);
static void (*release)(uint64_t *), (*abort_guard)(uint64_t *);
static unsigned results[64], result_count;
static void record(uint64_t n) { results[result_count++] = (unsigned)n; }
static void register_more(uint64_t n) {
  record(n);
  assert(register_dtor((uint64_t)record, n + 1, 9) == 0);
  finalize(9);
}
static uint64_t shared_guard;
static unsigned initialized;
static int payload;
static void *worker(void *unused) {
  (void)unused;
  if (acquire(&shared_guard)) {
    ++initialized;
    struct timespec pause = {.tv_nsec = 1000000};
    nanosleep(&pause, NULL);
    payload = 42;
    release(&shared_guard);
  }
  assert(payload == 42);
  return NULL;
}
static void *thread_destructor_worker(void *unused) {
  (void)unused;
  int (*register_tls)(uint64_t, uint64_t, uint64_t) =
      (void *)as_bionic_cxx_import("__cxa_thread_atexit_impl");
  assert(register_tls((uint64_t)record, 71, 0) == 0);
  assert(register_tls((uint64_t)record, 72, 0) == 0);
  assert(!as_bionic_cxx_thread_finalize());
  assert(!as_bionic_cxx_thread_finalize());
  return NULL;
}
static int (*key_create_fn)(unsigned *, uint64_t);
static int (*key_delete_fn)(unsigned);
static int (*key_set_fn)(unsigned, const void *);
static void *(*key_get_fn)(unsigned);
static int (*once_fn)(int *, uint64_t);
static unsigned destructor_key, destructor_calls;
static void repeat_destructor(uint64_t value) {
  ++destructor_calls;
  assert(!key_get_fn(destructor_key));
  assert(!key_set_fn(destructor_key, (void *)(uintptr_t)value));
}
static int once_control;
static unsigned once_calls;
static void once_initialize(uint64_t unused) {
  (void)unused;
  ++once_calls;
  struct timespec pause = {.tv_nsec = 1000000};
  nanosleep(&pause, NULL);
}
static void *once_worker(void *unused) {
  (void)unused;
  assert(!once_fn(&once_control, (uint64_t)once_initialize));
  assert(once_calls == 1);
  assert(!key_get_fn(destructor_key));
  assert(!key_set_fn(destructor_key, (void *)7));
  assert(key_get_fn(destructor_key) == (void *)7);
  assert(!as_bionic_keys_thread_finalize());
  return NULL;
}
int main(void) {
  /* ARM64 getauxval uses unsigned long for both argument and result. Known
   *
   * zero values preserve errno; absent types must be distinguishable. */
  _Static_assert(sizeof(unsigned long) == 8, "Android ARM64 getauxval ABI");
  unsigned long (*auxiliary)(unsigned long) =
      (void *)as_bionic_libc_import("getauxval");
  assert(auxiliary);
  errno = E2BIG;
  assert(auxiliary(6) == 4096 && errno == E2BIG);
  assert(auxiliary(16) == 3 && errno == E2BIG);
  assert(auxiliary(26) == 0 && errno == E2BIG);
  assert(auxiliary(23) == 0 && errno == E2BIG);
  const unsigned long absent_auxiliary[] = {0, 3, 7, 9, 25, ~0UL};
  for (unsigned i = 0; i < sizeof(absent_auxiliary) / sizeof(*absent_auxiliary);
       ++i) {
    errno = 0;
    assert(auxiliary(absent_auxiliary[i]) == 0 && errno == ENOENT);
  }
  int (*property)(const char *, char *) =
      (void *)as_bionic_libc_import("__system_property_get");
  assert(property);
  char property_value[94];
  const char *property_names[] = {"ro.build.version.sdk", "ro.product.cpu.abi", "ro.product.cpu.abilist", "ro.product.cpu.abilist64", "ro.hardware", "ro.product.model", "ro.build.version.release", "ro.build.fingerprint", "ro.build.version.security_patch", "ro.build.date.utc"};
  const char *property_expected[] = {"26", "arm64-v8a", "arm64-v8a", "arm64-v8a", "horizon", "AndroSwitch", "8.0.0", "androswitch/horizon/androswitch:8.0.0/AS26/0.1.0:eng/test-keys", "", "0"};
  for (unsigned i = 0; i < sizeof(property_names) / sizeof(*property_names);
       ++i) {
    memset(property_value, 0x5a, sizeof(property_value));
    errno = E2BIG;
    const char *expected = property_expected[i];
    assert(property(property_names[i], property_value + 1) ==
           (int)strlen(expected));
    assert(!strcmp(property_value + 1, expected) && errno == E2BIG);
    assert(property_value[0] == 0x5a && property_value[93] == 0x5a);
    assert(property_value[strlen(expected) + 2] == 0x5a);
  }
  const char *missing_properties[] = {"",
                                      "PATH",
                                      "ro.serialno",
                                      "RO.BUILD.VERSION.SDK"};
  for (unsigned i = 0;
       i < sizeof(missing_properties) / sizeof(*missing_properties); ++i) {
    memset(property_value, 0x5a, sizeof(property_value));
    errno = EACCES;
    assert(property(missing_properties[i], property_value + 1) == 0);
    assert(property_value[1] == 0 && property_value[2] == 0x5a &&
           errno == EACCES);
    assert(property_value[0] == 0x5a && property_value[93] == 0x5a);
  }
  register_dtor = (void *)as_bionic_cxx_import("__cxa_atexit");
  finalize = (void *)as_bionic_cxx_import("__cxa_finalize");
  acquire = (void *)as_bionic_cxx_import("__cxa_guard_acquire");
  release = (void *)as_bionic_cxx_import("__cxa_guard_release");
  abort_guard = (void *)as_bionic_cxx_import("__cxa_guard_abort");
  assert(register_dtor && finalize && acquire && release && abort_guard);
  assert(register_dtor((uint64_t)record, 1, 1) == 0);
  assert(register_dtor((uint64_t)record, 2, 2) == 0);
  assert(register_dtor((uint64_t)record, 3, 1) == 0);
  finalize(1);
  finalize(1);
  finalize(0);
  assert(result_count == 3 && results[0] == 3 && results[1] == 1 &&
         results[2] == 2);
  assert(register_dtor((uint64_t)register_more, 10, 9) == 0);
  finalize(9);
  assert(result_count == 5 && results[3] == 10 && results[4] == 11);
  pthread_t threads[16];
  for (int i = 0; i < 16; ++i)
    assert(!pthread_create(&threads[i], NULL, worker, NULL));
  for (int i = 0; i < 16; ++i)
    assert(!pthread_join(threads[i], NULL));
  assert(initialized == 1 && shared_guard == 1);
  uint64_t retry = 0;
  assert(acquire(&retry));
  abort_guard(&retry);
  assert(acquire(&retry));
  release(&retry);
  assert(!acquire(&retry));
  uint64_t recursive = 0;
  assert(acquire(&recursive));
  jmp_buf caught;
  escape = &caught;
  if (!setjmp(caught)) {
    acquire(&recursive);
    assert(0);
  }
  assert(trapped == -1204);
  abort_guard(&recursive);
  void *(*make)(size_t) = (void *)as_bionic_cxx_import("_Znwm");
  void (*destroy)(void *) = (void *)as_bionic_cxx_import("_ZdlPv");
  void *(*make_nothrow)(size_t, void *) =
      (void *)as_bionic_cxx_import("_ZnamRKSt9nothrow_t");
  void *p = make(0);
  assert(p);
  destroy(p);
  allocation_failure = 1;
  assert(!make_nothrow(8, NULL));
  if (!setjmp(caught)) {
    make(8);
    assert(0);
  }
  assert(trapped == -1201);
  allocation_failure = 0;
  assert(!pthread_create(&threads[0], NULL, thread_destructor_worker, NULL));
  assert(!pthread_join(threads[0], NULL));
  assert(result_count == 7 && results[5] == 72 && results[6] == 71);
  char out[16] = {0};
  void *(*checked_copy)(void *, const void *, size_t, size_t) =
      (void *)as_bionic_libc_import("__memcpy_chk");
  assert(checked_copy(out, "hello", 6, sizeof(out)) == out);
  assert(!strcmp(out, "hello"));
  if (!setjmp(caught)) {
    checked_copy(out, "hello", 6, 4);
    assert(0);
  }
  assert(trapped == -1210);
  size_t (*checked_length)(const char *, size_t) =
      (void *)as_bionic_libc_import("__strlen_chk");
  assert(checked_length(out, sizeof(out)) == 5);
  if (!setjmp(caught)) {
    checked_length("abcdef", 3);
    assert(0);
  }
  assert(trapped == -1210);
  char *(*checked_strncpy2)(char *, const char *, size_t, size_t, size_t) =
      (void *)as_bionic_libc_import("__strncpy_chk2");
  assert(checked_strncpy2(out, "a", 8, sizeof(out), 2) == out);
  assert(out[0] == 'a' && out[7] == 0);
  if (!setjmp(caught)) {
    checked_strncpy2(out, "abcdef", 8, sizeof(out), 3);
    assert(0);
  }
  assert(trapped == -1210);
  void *(*open_file)(const char *, const char *) =
      (void *)as_bionic_io_import("fopen");
  int (*close_file)(void *) = (void *)as_bionic_io_import("fclose");
  size_t (*write_file)(const void *, size_t, size_t, void *) =
      (void *)as_bionic_io_import("fwrite");
  size_t (*read_file)(void *, size_t, size_t, void *) =
      (void *)as_bionic_io_import("fread");
  int (*seek_file)(void *, int64_t, int) = (void *)as_bionic_io_import("fseek");
  void *file = open_file("/sdcard/shared.txt", "w+");
  assert(file && write_file("persistent", 1, 10, file) == 10);
  assert(!seek_file(file, 0, SEEK_SET));
  memset(out, 0, sizeof(out));
  assert(read_file(out, 1, 10, file) == 10 && !strcmp(out, "persistent"));
  assert(!close_file(file));
  file = open_file("/sdcard/shared.txt", "r");
  assert(read_file(out, 1, 10, file) == 10 && !strcmp(out, "persistent"));
  assert(!close_file(file));
  void *standard_out = *(void **)(uintptr_t)as_bionic_io_data("stdout");
  void *legacy_out = (void *)(uintptr_t)(as_bionic_io_data("__sF") + 152);
  void *legacy_err = (void *)(uintptr_t)(as_bionic_io_data("__sF") + 304);
  assert(standard_out == legacy_out);
  assert(*(void **)(uintptr_t)as_bionic_io_data("stderr") == legacy_err);
  assert(write_file("stdio identity bridge\n", 1, 22, legacy_out) == 22);
  assert(seek_file(standard_out, 0, SEEK_SET) == -1 && errno == ESPIPE);
  assert(read_file(out, 1, 1, (void *)(uintptr_t)as_bionic_io_data("__sF")) ==
         0);
  assert(errno == ENOTSUP);
  as_bionic_io_reset();
  typedef struct {
    int sec, min, hour, mday, mon, year, wday, yday, isdst;
    long gmtoff;
    const char *zone;
  } AndroidTm;
  AndroidTm *(*utc_time)(const time_t *) =
      (void *)as_bionic_libc_import("gmtime");
  time_t epoch = 0;
  AndroidTm *utc = utc_time(&epoch);
  assert(utc && utc->year == 70 && utc->mon == 0 && utc->mday == 1);
  assert(utc->gmtoff == 0 && !strcmp(utc->zone, "GMT"));
  key_create_fn = (void *)as_bionic_keys_import("pthread_key_create");
  key_delete_fn = (void *)as_bionic_keys_import("pthread_key_delete");
  key_set_fn = (void *)as_bionic_keys_import("pthread_setspecific");
  key_get_fn = (void *)as_bionic_keys_import("pthread_getspecific");
  once_fn = (void *)as_bionic_keys_import("pthread_once");
  assert(!key_create_fn(&destructor_key, 0));
  assert(!key_set_fn(destructor_key, (void *)123));
  assert(!pthread_create(&threads[0], NULL, once_worker, NULL));
  assert(!pthread_join(threads[0], NULL));
  assert(key_get_fn(destructor_key) == (void *)123);
  once_control = 0;
  once_calls = 0;
  for (int i = 0; i < 16; ++i)
    assert(!pthread_create(&threads[i], NULL, once_worker, NULL));
  for (int i = 0; i < 16; ++i)
    assert(!pthread_join(threads[i], NULL));
  assert(once_calls == 1);
  unsigned stale_key = destructor_key;
  assert(!key_delete_fn(destructor_key));
  assert(!key_create_fn(&destructor_key, (uint64_t)repeat_destructor));
  assert(destructor_key != stale_key && !key_get_fn(destructor_key));
  assert(key_set_fn(stale_key, (void *)3) == EINVAL);
  assert(!key_set_fn(destructor_key, (void *)4));
  assert(!as_bionic_keys_thread_finalize());
  assert(destructor_calls == 4);
  assert(!key_get_fn(destructor_key));
  assert(!key_delete_fn(destructor_key));
  assert(!as_bionic_keys_reset());
  char *formatted = NULL;
  int (*asprintf_fn)(char **, const char *, ...) =
      (void *)as_bionic_libc_import("asprintf");
  assert(asprintf_fn(&formatted, "value %d %.2f", 7, 1.25) == 12);
  assert(!strcmp(formatted, "value 7 1.25"));
  as_cxx_deallocate(formatted);
  int (*native_log)(int, const char *, const char *, ...) =
      (void *)as_bionic_log_import("__android_log_print");
  assert(native_log(4, "ABI", "value %d %.2f", 7, 1.25) == 12);
  assert(logged_priority == 4 && !strcmp(logged_tag, "ABI") &&
         !strcmp(logged_message, "value 7 1.25"));
  void (*open_log_fn)(const char *, int, int) =
      (void *)as_bionic_log_import("openlog");
  void (*syslog_fn)(int, const char *, ...) =
      (void *)as_bionic_log_import("syslog");
  open_log_fn("native", 0, 0);
  syslog_fn(3, "failure %d", 9);
  assert(logged_priority == 6 && !strcmp(logged_tag, "native") &&
         !strcmp(logged_message, "failure 9"));
  AsGuestStat metadata;
  int (*stat_fn)(const char *, AsGuestStat *) =
      (void *)as_bionic_vfs_import("stat");
  int (*access_fn)(const char *, int) = (void *)as_bionic_vfs_import("access");
  assert(!stat_fn("/sdcard/shared.txt", &metadata) && metadata.size == 10 &&
         metadata.mode == 0100600);
  assert(!access_fn("/sdcard/shared.txt", 6));
  assert(access_fn("/sdcard/shared.txt", 1) == -1 && errno == EACCES);
  int (*scan_fn)(const char *, AsGuestDirent ***, uint64_t, uint64_t) =
      (void *)as_bionic_vfs_import("scandir");
  AsGuestDirent **entries = NULL;
  assert(scan_fn("/sdcard/testdir", &entries, (uint64_t)directory_filter,
                 as_bionic_vfs_import("alphasort")) == 2);
  assert(!strcmp(entries[0]->name, "alpha") &&
         !strcmp(entries[1]->name, "zebra"));
  assert(entries[0]->record_size == 280 && entries[0]->type == 8);
  as_cxx_deallocate(entries[0]);
  as_cxx_deallocate(entries[1]);
  as_cxx_deallocate(entries);
  int (*rename_fn)(const char *, const char *) =
      (void *)as_bionic_vfs_import("rename");
  assert(!rename_fn("/sdcard/shared.txt", "/sdcard/destination.txt"));
  assert(rename_fn("/sdcard/shared.txt", "/sdcard/existing.txt") == -1 &&
         errno == ENOSYS);
  void *(*reopen_file)(const char *, const char *, void *) =
      (void *)as_bionic_io_import("freopen");
  file = open_file("/sdcard/shared.txt", "r");
  assert(reopen_file("/sdcard/shared.txt", "w+", file) == file);
  assert(write_file("again", 1, 5, file) == 5);
  assert(!seek_file(file, 0, SEEK_SET));
  memset(out, 0, sizeof(out));
  assert(read_file(out, 1, 5, file) == 5 && !strcmp(out, "again"));
  assert(!close_file(file));
  assert(reopen_file("/sdcard/shared.txt", "w", standard_out) == standard_out);
  assert(write_file("redirect", 1, 8, standard_out) == 8 && file_length == 8);
  assert(!close_file(standard_out));
  as_bionic_io_reset();
  char library_name[] = "libportable.so",
       unavailable_name[] = "unsupported_shared_api";
  uint64_t missing_address =
      as_native_missing_veneer(library_name, unavailable_name);
  assert(missing_address &&
         missing_address ==
             as_native_missing_veneer(library_name, unavailable_name));
  const uint32_t *instructions = (void *)(uintptr_t)missing_address;
  assert(instructions[0] == 0x58000080 && instructions[1] == 0x580000b0 &&
         instructions[2] == 0x580000d1 && instructions[3] == 0xd61f0220);
  typedef struct Missing Missing;
  Missing *identity;
  void (*fail_import)(Missing *);
  memcpy(&identity, (char *)(uintptr_t)missing_address + 16, sizeof(identity));
  memcpy(&fail_import, (char *)(uintptr_t)missing_address + 24,
         sizeof(fail_import));
  library_name[0] = 'x';
  unavailable_name[0] = 'x';
  if (!setjmp(caught)) {
    fail_import(identity);
    assert(0);
  }
  assert(trapped == -1211);
  assert(!strcmp(as_native_missing_library(), "libportable.so"));
  assert(!strcmp(as_native_missing_symbol(), "unsupported_shared_api"));
  while (test_mappings)
    as_native_release((uint64_t)test_mappings);
  as_native_missing_reset();
  assert(!as_native_missing_library() && !as_native_missing_symbol());
  assert(!as_bionic_cxx_import("__cxa_throw"));
  assert(!as_bionic_cxx_import("unknown_import"));
  assert(as_bionic_cxx_data("_ZSt7nothrow"));
  assert(!as_bionic_cxx_finalize());
  assert(!as_bionic_cxx_reset());
  uint64_t abandoned_guard = 0;
  assert(acquire(&abandoned_guard));
  assert(!as_bionic_cxx_reset()); /* All test workers joined: abandon failed
                                     startup guard. */
  assert(acquire(&abandoned_guard));
  abort_guard(&abandoned_guard);
  /* Diagnostics must distinguish both pending registries without consuming
   * callbacks or making a busy reset succeed. */
  int (*register_tls_probe)(uint64_t, uint64_t, uint64_t) =
      (void *)as_bionic_cxx_import("__cxa_thread_atexit_impl");
  assert(!as_bionic_cxx_pending());
  assert(!register_dtor((uint64_t)record, 90, 0));
  assert(!register_tls_probe((uint64_t)record, 91, 0));
  errno = E2BIG;
  assert(as_bionic_cxx_pending() ==
             (AS_CXX_PENDING_GLOBAL | AS_CXX_PENDING_THREAD) &&
         errno == E2BIG);
  assert(as_bionic_cxx_reset() == -EBUSY);
  assert(as_bionic_cxx_pending() ==
         (AS_CXX_PENDING_GLOBAL | AS_CXX_PENDING_THREAD));
  assert(!as_bionic_cxx_finalize());
  assert(as_bionic_cxx_pending() == AS_CXX_PENDING_THREAD);
  assert(as_bionic_cxx_reset() == -EBUSY);
  assert(!as_bionic_cxx_thread_finalize());
  assert(!as_bionic_cxx_pending() && !as_bionic_cxx_reset());
  extern void as_bionic_libc_reset(void);
  int (*random_draw)(void) = (void *)as_bionic_libc_import("rand");
  void (*random_seed)(unsigned) = (void *)as_bionic_libc_import("srand");
  assert(random_draw && random_seed);
  random_seed(42);
  assert(random_draw() == 71876166);
  errno = E2BIG;
  as_bionic_libc_reset();
  assert(random_draw() == 1804289383 && errno == E2BIG);
  puts("native ABI contracts: PASS (C++ lifetimes, guards/once/keys, "
       "allocation, "
       "fortify, stdio/VFS, logging)");
  return 0;
}
