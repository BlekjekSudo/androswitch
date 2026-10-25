/* Horizon code-memory mapping follows the generic so_util technique by
 * Copyright (C) 2021 Andy Nguyen, fgsfds, under MIT
 * (third_party/so_util.LICENSE). Program-header planning/relocations are
 * implemented in Rust, without game hooks. Remaining AndroSwitch code: MIT OR
 * Apache-2.0. */
#include "native.h"
#include "bionic_cxx.h"
#include "bionic_dlfcn.h"
#include "bionic_errno.h"
#include "bionic_finalize.h"
#include "bionic_locale.h"
#include "bionic_memory.h"
#include <errno.h>
#include <malloc.h>
#include <math.h>
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
extern uint64_t as_bionic_keys_import(const char *);
extern int as_bionic_keys_thread_finalize(void);
extern int as_bionic_keys_reset(void);
extern void as_native_record_worker_error(int error);

typedef struct Mapping {
  void *backing, *address;
  size_t size;
  VirtmemReservation *reservation;
  int mapped;
  uint8_t *executable;
  char *library_name;
  void *program_headers;
  size_t program_header_count;
  struct Mapping *next;
} Mapping;
static Handle own_process;
static Mapping *mappings;
static Mutex resource_mutex;
static unsigned module_log_entries;
static Mutex diagnostics_mutex;
static int diagnostics_ready, native_trace_enabled;
static unsigned native_trace_calls, jni_failure_entries;
static unsigned native_trace_metadata_entries;
static int native_trace_started, jni_failures_started;

static void diagnostic_text(FILE *file, const uint8_t *bytes, size_t length) {
  for (size_t i = 0; i < length && i < 4096; ++i) {
    unsigned char byte = bytes[i];
    if (byte >= 0x20 && byte < 0x7f && byte != '"' && byte != '\\')
      fputc(byte, file);
    else
      fprintf(file, "\\x%02x", byte);
  }
  if (length > 4096)
    fputs("[truncated]", file);
}
/* Called under diagnostics_mutex, on host TLS. Failed diagnostics must never
 *
 * alter guest errno, pending exceptions, or the native call's return value. */
static void diagnostics_begin(void) {
  if (diagnostics_ready)
    return;
  diagnostics_ready = 1;
  FILE *marker =
      fopen("sdmc:/switch/androswitch/logs/native-trace.enabled", "rb");
  native_trace_enabled = marker != NULL;
  if (marker)
    fclose(marker);
  FILE *failures =
      fopen("sdmc:/switch/androswitch/logs/jni-failures.log", "wb");
  if (failures) {
    fclose(failures);
    jni_failures_started = 1;
  }
  FILE *calls = fopen("sdmc:/switch/androswitch/logs/native-calls.log", "wb");
  if (calls) {
    fclose(calls);
    native_trace_started = 1;
  }
}
int as_native_trace_active(void) {
  int saved_errno = errno;
  mutexLock(&diagnostics_mutex);
  diagnostics_begin();
  int active = native_trace_enabled;
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
  return active;
}
void as_native_trace_metadata(const char *source, const uint8_t *detail,
                              size_t length) {
  int saved_errno = errno;
  mutexLock(&diagnostics_mutex);
  diagnostics_begin();
  if (native_trace_enabled && native_trace_metadata_entries < 32) {
    ++native_trace_metadata_entries;
    FILE *file = fopen("sdmc:/switch/androswitch/logs/native-calls.log",
                       native_trace_started ? "ab" : "wb");
    if (file) {
      native_trace_started = 1;
      fprintf(file, "metadata thread=0x%llx source=\"",
              (unsigned long long)threadGetCurHandle());
      diagnostic_text(file, (const uint8_t *)source, strnlen(source, 128));
      fputs("\" detail=\"", file);
      diagnostic_text(file, detail, length);
      fprintf(file, "\" detail_bytes=%zu\n", length);
      fclose(file);
    }
  }
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
}
void as_native_jni_failure(uint32_t operation, uint64_t a, uint64_t b,
                           uint64_t c, const uint8_t *detail, size_t length) {
  int saved_errno = errno;
  mutexLock(&diagnostics_mutex);
  diagnostics_begin();
  if (jni_failure_entries < 256) {
    ++jni_failure_entries;
    FILE *file = fopen("sdmc:/switch/androswitch/logs/jni-failures.log",
                       jni_failures_started ? "ab" : "wb");
    if (file) {
      jni_failures_started = 1;
      fprintf(file,
              "jni_failure op=%u thread=0x%llx a=0x%llx b=0x%llx "
              "c=0x%llx detail=\"",
              operation, (unsigned long long)threadGetCurHandle(),
              (unsigned long long)a, (unsigned long long)b,
              (unsigned long long)c);
      diagnostic_text(file, detail, length);
      fprintf(file, "\" detail_bytes=%zu\n", length);
      fclose(file);
    }
  }
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
}
/* Keep diagnostic temporaries outside the guest call's setjmp scope. */
static __attribute__((noinline)) unsigned
record_native_enter(const AsNativeCall *frame) {
  int saved_errno = errno;
  unsigned ticket = 0;
  mutexLock(&diagnostics_mutex);
  diagnostics_begin();
  if (native_trace_enabled && native_trace_calls < 1024) {
    ticket = ++native_trace_calls;
    FILE *file = fopen("sdmc:/switch/androswitch/logs/native-calls.log",
                       native_trace_started ? "ab" : "wb");
    if (file) {
      native_trace_started = 1;
      fprintf(file,
              "enter call=%u thread=0x%llx function=0x%llx "
              "x0=0x%llx x1=0x%llx x2=0x%llx x3=0x%llx\n",
              ticket, (unsigned long long)threadGetCurHandle(),
              (unsigned long long)frame->function,
              (unsigned long long)frame->integer[0],
              (unsigned long long)frame->integer[1],
              (unsigned long long)frame->integer[2],
              (unsigned long long)frame->integer[3]);
      fclose(file);
    }
  }
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
  return ticket;
}
static void record_native_leave(unsigned ticket, const AsNativeCall *frame,
                                int error) {
  if (!ticket)
    return;
  int saved_errno = errno;
  mutexLock(&diagnostics_mutex);
  FILE *file = fopen("sdmc:/switch/androswitch/logs/native-calls.log",
                     native_trace_started ? "ab" : "wb");
  if (file) {
    native_trace_started = 1;
    fprintf(file, "leave call=%u thread=0x%llx error=%d", ticket,
            (unsigned long long)threadGetCurHandle(), error);
    if (!error)
      fprintf(file, " x0=0x%llx x1=0x%llx",
              (unsigned long long)frame->result_integer[0],
              (unsigned long long)frame->result_integer[1]);
    fputc('\n', file);
    fclose(file);
  }
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
}
/* Crash diagnostics use actual loader allocations, never inferred ELF bases.
 *
 * Keep this bounded and escape names so one library cannot forge log records.

 * * The next native session truncates the previous log at its first module. */
static void record_module_mapping(const Mapping *mapping) {
  int saved_errno = errno;
  if (module_log_entries >= 256)
    return;
  FILE *file = fopen("sdmc:/switch/androswitch/logs/native-modules.log",
                     module_log_entries ? "ab" : "wb");
  if (file) {
    fprintf(file, "module base=0x%llx length=0x%zx phdrs=%zu name=\"",
            (unsigned long long)(uintptr_t)mapping->address, mapping->size,
            mapping->program_header_count);
    size_t length = strlen(mapping->library_name);
    for (size_t i = 0; i < length && i < 1024; ++i) {
      unsigned char byte = (unsigned char)mapping->library_name[i];
      if (byte >= 0x20 && byte < 0x7f && byte != '"' && byte != '\\')
        fputc(byte, file);
      else
        fprintf(file, "\\x%02x", byte);
    }
    fprintf(file, "\" name_bytes=%zu%s\n", length,
            length > 1024 ? " name_truncated=true" : "");
    fclose(file);
    ++module_log_entries;
  }
  errno = saved_errno;
}
__attribute__((weak)) void as_teardown_phase(const char *phase) { (void)phase; }
typedef struct GuestAllocation {
  void *pointer;
  size_t size;
  struct GuestAllocation *next;
} GuestAllocation;
static GuestAllocation *guest_allocations;
static void *guest_aligned_allocate(size_t alignment, size_t size) {
  if (size > (128u << 20)) {
    errno = ENOMEM;
    return NULL;
  }
  void *p = alignment > 16 ? memalign(alignment, size ? size : 1)
                           : malloc(size ? size : 1);
  if (!p)
    return NULL;
  GuestAllocation *entry = malloc(sizeof(*entry));
  if (!entry) {
    free(p);
    return NULL;
  }
  entry->pointer = p;
  entry->size = size;
  mutexLock(&resource_mutex);
  entry->next = guest_allocations;
  guest_allocations = entry;
  mutexUnlock(&resource_mutex);
  return p;
}
static void *guest_malloc(size_t size) {
  return guest_aligned_allocate(16, size);
}
static int guest_posix_memalign(void **output, size_t alignment, size_t size) {
  if (!output || alignment < sizeof(void *) || (alignment & (alignment - 1)))
    return EINVAL;
  int saved = errno;
  void *pointer = guest_aligned_allocate(alignment, size);
  errno = saved;
  if (!pointer)
    return ENOMEM;
  *output = pointer;
  return 0;
}
static void guest_free(void *p) {
  if (!p)
    return;
  mutexLock(&resource_mutex);
  GuestAllocation **link = &guest_allocations;
  while (*link && (*link)->pointer != p)
    link = &(*link)->next;
  if (*link) {
    GuestAllocation *entry = *link;
    *link = entry->next;
    free(entry->pointer);
    free(entry);
  } else
    errno = EINVAL;
  mutexUnlock(&resource_mutex);
}
static void *guest_calloc(size_t count, size_t size) {
  if (size && count > SIZE_MAX / size) {
    errno = ENOMEM;
    return NULL;
  }
  void *p = guest_malloc(count * size);
  if (p)
    memset(p, 0, count * size);
  return p;
}
static void *guest_realloc(void *p, size_t size) {
  if (!p)
    return guest_malloc(size);
  if (!size) {
    guest_free(p);
    return NULL;
  }
  if (size > (128u << 20)) {
    errno = ENOMEM;
    return NULL;
  }
  mutexLock(&resource_mutex);
  GuestAllocation *entry = guest_allocations;
  while (entry && entry->pointer != p)
    entry = entry->next;
  void *result = NULL;
  if (entry) {
    result = realloc(p, size);
    if (result) {
      entry->pointer = result;
      entry->size = size;
    }
  } else
    errno = EINVAL;
  mutexUnlock(&resource_mutex);
  return result;
}
static char *guest_strdup(const char *s) {
  size_t n = strlen(s) + 1;
  char *p = guest_malloc(n);
  if (p)
    memcpy(p, s, n);
  return p;
}
typedef struct {
  Handle server, received;
  Result result;
} Duplicate;
static void receive_process(void *argument) {
  Duplicate *d = argument;
  s32 index;
  memset(armGetTls(), 0, 0x100);
  d->result =
      svcReplyAndReceive(&index, &d->server, 1, INVALID_HANDLE, UINT64_MAX);
  if (R_SUCCEEDED(d->result)) {
    HipcParsedRequest request = hipcParseRequest(armGetTls());
    if (request.meta.num_copy_handles == 1)
      d->received = request.data.copy_handles[0];
    hipcMakeRequestInline(armGetTls(), .type = 0);
    svcReplyAndReceive(&index, &d->server, 0, d->server, 0);
  }
}
static Handle process_handle(void) {
  Handle h = envGetOwnProcessHandle();
  if (h != INVALID_HANDLE)
    return h;
  if (own_process)
    return own_process;
  /* Homebrew loaders normally supply a real process handle. A local IPC copy
   * converts the pseudo handle when a loader (including Ryujinx) omits it. */
  Duplicate d = {0};
  Handle client;
  Thread receiver;
  if (R_FAILED(svcCreateSession(&d.server, &client, false, 0)))
    return INVALID_HANDLE;
  Result rc =
      threadCreate(&receiver, receive_process, &d, NULL, 16384, 0x2c, -2);
  if (R_SUCCEEDED(rc)) {
    rc = threadStart(&receiver);
    if (R_SUCCEEDED(rc)) {
      HipcRequest request =
          hipcMakeRequestInline(armGetTls(), .type = 4, .num_copy_handles = 1);
      request.copy_handles[0] = CUR_PROCESS_HANDLE;
      Result sent = svcSendSyncRequest(client);
      if (R_FAILED(sent)) {
        svcCloseHandle(client);
        client = INVALID_HANDLE;
      }
      threadWaitForExit(&receiver);
      own_process = d.received;
    }
    threadClose(&receiver);
  }
  if (client != INVALID_HANDLE) {
    svcCloseHandle(client);
  }
  svcCloseHandle(d.server);
  return own_process;
}
int as_native_allocate(size_t size, uint64_t *address, uint64_t *token) {
  Mapping *m = calloc(1, sizeof(*m));
  if (!m)
    return -ENOMEM;
  m->size = size;
  m->backing = memalign(4096, size);
  if (!m->backing) {
    free(m);
    return -ENOMEM;
  }
  memset(m->backing, 0, size);
  virtmemLock();
  m->address = virtmemFindCodeMemory(size, 4096);
  m->reservation = m->address ? virtmemAddReservation(m->address, size) : NULL;
  virtmemUnlock();
  if (!m->address || !m->reservation) {
    free(m->backing);
    free(m);
    return -ENOMEM;
  }
  *address = (uint64_t)m->address;
  *token = (uint64_t)m;
  mutexLock(&resource_mutex);
  m->next = mappings;
  mappings = m;
  mutexUnlock(&resource_mutex);
  return 0;
}
int as_native_write(uint64_t token, size_t offset, const uint8_t *bytes,
                    size_t length) {
  Mapping *m = (void *)token;
  if (m->mapped || offset > m->size || length > m->size - offset)
    return -EINVAL;
  memcpy((char *)m->backing + offset, bytes, length);
  return 0;
}
int as_native_finalize(uint64_t token, const uint8_t *executable,
                       size_t pages) {
  Mapping *m = (void *)token;
  if (pages != m->size / 4096 || m->mapped)
    return -EINVAL;
  m->executable = malloc(pages);
  if (!m->executable)
    return -ENOMEM;
  memcpy(m->executable, executable, pages);
  Result rc = svcMapProcessCodeMemory(process_handle(), (u64)m->address,
                                      (u64)m->backing, m->size);
  if (R_FAILED(rc))
    return (int)rc;
  m->mapped = 1;
  /* Kernel forbids W->X. Set executable runs before assigning writable pages.
   */
  for (int want_x = 1; want_x >= 0; want_x--) {
    size_t at = 0;
    while (at < pages) {
      if (executable[at] != (uint8_t)want_x) {
        at++;
        continue;
      }
      size_t end = at + 1;
      while (end < pages && executable[end] == (uint8_t)want_x)
        end++;
      rc = svcSetProcessMemoryPermission(
          process_handle(), (u64)m->address + at * 4096, (end - at) * 4096,
          want_x ? Perm_Rx : Perm_Rw);
      if (R_FAILED(rc))
        return (int)rc;
      at = end;
    }
  }
  armDCacheFlush(m->address, m->size);
  armICacheInvalidate(m->address, m->size);
  return 0;
}
void as_native_release(uint64_t token) {
  Mapping *m = (void *)token;
  if (!m)
    return;
  as_teardown_phase("native_mapping_release");
  if (m->mapped && m->executable) {
    size_t pages = m->size / 4096, at = 0;
    while (at < pages) {
      if (!m->executable[at]) {
        at++;
        continue;
      }
      size_t end = at + 1;
      while (end < pages && m->executable[end])
        end++;
      if (R_FAILED(svcSetProcessMemoryPermission(process_handle(),
                                                 (u64)m->address + at * 4096,
                                                 (end - at) * 4096, Perm_Rw)))
        return;
      at = end;
    }
  }
  if (m->mapped &&
      R_FAILED(svcUnmapProcessCodeMemory(process_handle(), (u64)m->address,
                                         (u64)m->backing, m->size)))
    return;
  virtmemLock();
  virtmemRemoveReservation(m->reservation);
  virtmemUnlock();
  mutexLock(&resource_mutex);
  Mapping **link = &mappings;
  while (*link && *link != m)
    link = &(*link)->next;
  if (*link)
    *link = m->next;
  mutexUnlock(&resource_mutex);
  free(m->backing);
  free(m->executable);
  free(m->library_name);
  free(m->program_headers);
  free(m);
}

int as_native_register_module(uint64_t token, const char *name,
                              const uint8_t *headers, size_t length) {
  Mapping *mapping = (void *)token;
  if (!mapping || !mapping->mapped || !length || length % 56 ||
      length / 56 > 4096 || mapping->library_name || strlen(name) > 65536)
    return -EINVAL;
  char *owned_name = strdup(name);
  void *owned_headers = malloc(length);
  if (!owned_name || !owned_headers) {
    free(owned_name);
    free(owned_headers);
    return -ENOMEM;
  }
  memcpy(owned_headers, headers, length);
  mutexLock(&resource_mutex);
  mapping->library_name = owned_name;
  mapping->program_headers = owned_headers;
  mapping->program_header_count = length / 56;
  record_module_mapping(mapping);
  mutexUnlock(&resource_mutex);
  return 0;
}
typedef struct {
  uint64_t address;
  const char *name;
  const void *headers;
  uint16_t header_count;
} GuestPhdrInfo;
_Static_assert(sizeof(GuestPhdrInfo) == 32, "Android API 26 dl_phdr_info ABI");
static int guest_iterate_phdr(uint64_t callback, uint64_t argument) {
  if (!callback) {
    errno = EINVAL;
    return -1;
  }
  mutexLock(&resource_mutex);
  size_t count = 0;
  for (Mapping *m = mappings; m; m = m->next)
    if (m->library_name)
      ++count;
  GuestPhdrInfo *snapshot = calloc(count ? count : 1, sizeof(*snapshot));
  if (!snapshot) {
    mutexUnlock(&resource_mutex);
    errno = ENOMEM;
    return -1;
  }
  size_t at = 0;
  for (Mapping *m = mappings; m; m = m->next)
    if (m->library_name)
      snapshot[at++] =
          (GuestPhdrInfo){(uint64_t)m->address, m->library_name,
                          m->program_headers, m->program_header_count};
  mutexUnlock(&resource_mutex);
  int result = 0;
  for (size_t i = 0; i < count; ++i) {
    AsNativeCall frame = {0};
    frame.function = callback;
    frame.integer[0] = (uint64_t)&snapshot[i];
    frame.integer[1] = sizeof(GuestPhdrInfo);
    frame.integer[2] = argument;
    int rc = as_native_call(&frame);
    if (rc) {
      free(snapshot);
      as_native_trap(rc);
      return -1;
    }
    result = (int)frame.result_integer[0];
    if (result)
      break;
  }
  free(snapshot);
  return result;
}
extern void as_native_import_bridge(void);
extern void as_native_tlsdesc(void);
extern void as_native_call_asm(AsNativeCall *);
/* Private host TLS: all host callbacks run with the libnx TP, never the guest
 * TP. */
typedef struct Context {
  void *session;
  uint8_t *guest;
  int error, android_errno;
  jmp_buf *escape;
  uint64_t thread_identity;
  struct EmulatedTls *emulated;
} Context;
static __thread Context context;
/* The native-bridge TLS slot points to private bridge state. Keeping saved
 * return addresses here leaves SP unchanged for imported stack/variadic args.
 * Layout is shared with native_call.S; this fits in the reserved prefix page.
 */
typedef struct {
  uint64_t host_tp, guest_tp, depth, returns[128], saved_x19[128];
  int saved_errno[128];
  uint64_t saved_x18[128];
} ImportBridge;
_Static_assert(sizeof(ImportBridge) < 4096 - 32, "bridge prefix capacity");
_Static_assert(offsetof(ImportBridge, returns) == 24,
               "assembly bridge returns");
_Static_assert(offsetof(ImportBridge, saved_x19) == 1048,
               "assembly bridge x19");
_Static_assert(offsetof(ImportBridge, saved_errno) == 2072,
               "assembly bridge errno");
_Static_assert(offsetof(ImportBridge, saved_x18) == 2584,
               "assembly bridge x18");
_Static_assert(offsetof(AsNativeCall, function) == 416,
               "assembly function ABI");
_Static_assert(offsetof(AsNativeCall, guest_tls) == 424, "assembly TLS ABI");
void as_native_errno_enter(ImportBridge *bridge) {
  bridge->saved_errno[bridge->depth - 1] = errno;
  errno = as_errno_from_android(context.android_errno);
}
void as_native_errno_leave(ImportBridge *bridge) {
  context.android_errno = as_errno_to_android(errno);
  errno = bridge->saved_errno[bridge->depth - 1];
}
static uint8_t tls_template[65536];
static size_t tls_used = 256;
typedef struct EmulatedTls {
  const void *control;
  void *bytes;
  struct EmulatedTls *next;
} EmulatedTls;
typedef struct {
  size_t size, alignment;
  uintptr_t index;
  const void *initial;
} EmulatedControl;
static uint64_t tls_modules[256];
static void *guest_emutls(EmulatedControl *control) {
  for (EmulatedTls *entry = context.emulated; entry; entry = entry->next)
    if (entry->control == control)
      return entry->bytes;
  if (!control || control->size > (1 << 20) || control->alignment > (1 << 20) ||
      !control->alignment || (control->alignment & (control->alignment - 1)))
    as_native_trap(-1007);
  EmulatedTls *entry = calloc(1, sizeof(*entry));
  if (!entry)
    as_native_trap(-12);
  entry->bytes = memalign(
      control->alignment < sizeof(void *) ? sizeof(void *) : control->alignment,
      control->size ? control->size : 1);
  if (!entry->bytes) {
    free(entry);
    as_native_trap(-12);
  }
  if (control->initial)
    memcpy(entry->bytes, control->initial, control->size);
  else
    memset(entry->bytes, 0, control->size);
  entry->control = control;
  entry->next = context.emulated;
  context.emulated = entry;
  return entry->bytes;
}
static void *guest_tls_address(const uint64_t *index) {
  if (!index || index[0] >= 256 || !tls_modules[index[0]])
    as_native_trap(-1007);
  return (void *)(as_native_guest_tls() + tls_modules[index[0]] + index[1]);
}
void as_native_thread_cleanup(void) {
  int cxx_error = as_bionic_cxx_thread_finalize();
  if (!cxx_error)
    cxx_error = as_bionic_keys_thread_finalize();
  as_bionic_locale_thread_finalize();
  if (cxx_error) {
    as_native_fail(cxx_error);
    as_native_record_worker_error(cxx_error);
  }
  while (context.emulated) {
    EmulatedTls *entry = context.emulated;
    context.emulated = entry->next;
    free(entry->bytes);
    free(entry);
  }
  free(context.guest);
  memset(&context, 0, sizeof(context));
}
typedef struct GuestThread {
  Thread thread;
  uint64_t function, argument, result;
  void *session;
  struct GuestThread *next;
  int joining;
} GuestThread;
static GuestThread *guest_threads;
static void guest_thread_main(void *argument) {
  GuestThread *thread = argument;
  context.session = thread->session;
  context.thread_identity = (uint64_t)thread;
  AsNativeCall call = {0};
  call.function = thread->function;
  call.integer[0] = thread->argument;
  int call_error = as_native_call(&call);
  if (call_error)
    as_native_record_worker_error(call_error);
  thread->result = call.result_integer[0];
  int cxx_error = as_bionic_cxx_thread_finalize();
  if (!cxx_error)
    cxx_error = as_bionic_keys_thread_finalize();
  as_bionic_locale_thread_finalize();
  if (cxx_error) {
    as_native_fail(cxx_error);
    as_native_record_worker_error(cxx_error);
  }
  while (context.emulated) {
    EmulatedTls *entry = context.emulated;
    context.emulated = entry->next;
    free(entry->bytes);
    free(entry);
  }
  free(context.guest);
  context.guest = NULL;
}
static int guest_pthread_create(uint64_t *out, const void *attributes,
                                void *(*function)(void *), void *argument) {
  if (attributes) {
    return as_errno_to_android(ENOTSUP);
  }
  int admission = as_worker_admission_enter();
  if (admission)
    return as_errno_to_android(-admission);
  GuestThread *thread = calloc(1, sizeof(*thread));
  if (!thread) {
    as_worker_admission_leave();
    return ENOMEM;
  }
  thread->function = (uint64_t)function;
  thread->argument = (uint64_t)argument;
  thread->session = context.session;
  Result rc = threadCreate(&thread->thread, guest_thread_main, thread, NULL,
                           1024 * 1024, 0x2c, -2);
  if (R_FAILED(rc)) {
    free(thread);
    as_worker_admission_leave();
    return EAGAIN;
  }
  rc = threadStart(&thread->thread);
  if (R_FAILED(rc)) {
    threadClose(&thread->thread);
    free(thread);
    as_worker_admission_leave();
    return EAGAIN;
  }
  *out = (uint64_t)thread;
  mutexLock(&resource_mutex);
  thread->next = guest_threads;
  guest_threads = thread;
  mutexUnlock(&resource_mutex);
  as_worker_admission_leave();
  return 0;
}
static int guest_pthread_join(uint64_t handle, void **result) {
  GuestThread *thread = (void *)handle;
  mutexLock(&resource_mutex);
  GuestThread *found = guest_threads;
  while (found && found != thread)
    found = found->next;
  if (!found || found->joining) {
    mutexUnlock(&resource_mutex);
    return ESRCH;
  }
  if (handle == context.thread_identity) {
    mutexUnlock(&resource_mutex);
    return as_errno_to_android(EDEADLK);
  }
  found->joining = 1;
  mutexUnlock(&resource_mutex);
  as_teardown_phase("guest_thread_join");
  Result rc = threadWaitForExit(&thread->thread);
  if (R_FAILED(rc))
    return EINVAL;
  if (result)
    *result = (void *)thread->result;
  threadClose(&thread->thread);
  mutexLock(&resource_mutex);
  GuestThread **link = &guest_threads;
  while (*link && *link != thread)
    link = &(*link)->next;
  if (*link)
    *link = thread->next;
  mutexUnlock(&resource_mutex);
  free(thread);
  as_teardown_phase("guest_thread_joined");
  return 0;
}
static uint64_t guest_pthread_self(void) {
  return context.thread_identity ? context.thread_identity
                                 : (uint64_t)threadGetCurHandle();
}
/* C++ ABI registry synchronization uses host TLS, including when waiting on
 * another guest thread's static initialization. */
static Mutex cxx_mutex;
static CondVar cxx_condition;
void as_cxx_lock(void) { mutexLock(&cxx_mutex); }
void as_cxx_unlock(void) { mutexUnlock(&cxx_mutex); }
void as_cxx_wait(void) { condvarWait(&cxx_condition, &cxx_mutex); }
void as_cxx_wake(void) { condvarWakeAll(&cxx_condition); }
uint64_t as_cxx_thread(void) { return guest_pthread_self(); }
void *as_cxx_allocate(size_t size) { return guest_malloc(size); }
void as_cxx_deallocate(void *pointer) { guest_free(pointer); }
void *as_guest_memory_allocate(size_t length) { return memalign(4096, length); }
int as_guest_memory_permission(void *address, size_t length, int protection) {
  Permission permission = protection == 3 ? Perm_Rw : protection == 1 ? Perm_R : Perm_None;
  Result rc = svcSetMemoryPermission(address, length, permission);
  if (R_FAILED(rc)) { errno = EACCES; return -1; }
  return 0;
}
void as_guest_memory_release(void *address, size_t length) {
  (void)length;
  free(address);
}
void as_cxx_trap(int error) { as_native_trap(error); }
int as_cxx_invoke(uint64_t function, uint64_t argument) {
  AsNativeCall frame = {0};
  frame.function = function;
  frame.integer[0] = argument;
  int rc = as_native_call(&frame);
  errno = as_errno_from_android(context.android_errno);
  return rc;
}
int as_native_finalize_thread_callbacks(void) {
  return as_bionic_finalize_thread();
}
int as_native_finalize_callbacks(void) { return as_bionic_finalize_session(); }
unsigned as_native_pending_callbacks(void) {
  return as_bionic_finalization_pending();
}
static uint64_t guard = 0x6173577463684a4e;
uint64_t as_native_guest_tls(void) {
  if (!context.guest) {
    context.guest = memalign(4096, sizeof(tls_template) + 4096);
    if (!context.guest)
      return 0;
    memset(context.guest, 0, sizeof(tls_template) + 4096);
    memcpy(context.guest + 4096, tls_template, sizeof(tls_template));
    ImportBridge *bridge = (void *)context.guest;
    bridge->guest_tp = (uint64_t)(context.guest + 4096);
    *(ImportBridge **)(context.guest + 4096 - 16) = bridge;
  }
  return (uint64_t)(context.guest + 4096);
}
void *as_native_session(void) { return context.session; }
void as_native_set_session(void *session) {
  context.session = session;
  int saved_errno = errno;
  mutexLock(&diagnostics_mutex);
  diagnostics_begin();
  mutexUnlock(&diagnostics_mutex);
  errno = saved_errno;
}
int as_native_last_error(void) { return context.error; }
void as_native_fail(int code) { context.error = code; }
void as_native_trap(int code) {
  context.error = code;
  if (context.escape)
    longjmp(*context.escape, 1);
  abort();
}
uint64_t as_native_tls(size_t module, const uint8_t *bytes, size_t length,
                       size_t size, size_t alignment) {
  if (module >= 256 || alignment > 4096 || !alignment ||
      size > sizeof(tls_template))
    return 0;
  size_t offset = (tls_used + alignment - 1) & ~(alignment - 1);
  if (offset + size > sizeof(tls_template) || length > size)
    return 0;
  memcpy(tls_template + offset, bytes, length);
  tls_used = offset + size;
  tls_modules[module] = offset;
  if (context.guest)
    memcpy(context.guest + 4096 + offset, bytes, length);
  return offset;
}
uint64_t as_native_tlsdesc_address(void) { return (uint64_t)as_native_tlsdesc; }
int as_native_call(AsNativeCall *frame) {
  int host_errno = errno;
  unsigned trace_ticket = record_native_enter(frame);
  frame->guest_tls = as_native_guest_tls();
  if (!frame->guest_tls) {
    record_native_leave(trace_ticket, frame, -ENOMEM);
    return -ENOMEM;
  }
  context.error = 0;
  ImportBridge *bridge = (void *)context.guest;
  uint64_t bridge_depth = bridge->depth;
  jmp_buf escape;
  jmp_buf *previous = context.escape;
  context.escape = &escape;
  if (!setjmp(escape))
    as_native_call_asm(frame);
  context.escape = previous;
  bridge->depth = bridge_depth;
  record_native_leave(trace_ticket, frame, context.error);
  errno = host_errno;
  return context.error;
}

uint64_t as_native_veneer(uint64_t function) {
  if (!function)
    return 0;
  uint64_t address, token;
  if (as_native_allocate(4096, &address, &token))
    return 0;
  uint8_t code[32] = {0};
  uint32_t instructions[] = {0x58000090, 0x580000b1, 0xd61f0220, 0xd503201f};
  memcpy(code, instructions, 16);
  memcpy(code + 16, &function, 8);
  uint64_t bridge = (uint64_t)as_native_import_bridge;
  memcpy(code + 24, &bridge, 8);
  uint8_t page = 1;
  if (as_native_write(token, 0, code, sizeof(code)) ||
      as_native_finalize(token, &page, 1)) {
    as_native_release(token);
    return 0;
  }
  /* Owned by the process runner; importing an existing function reuses its
   * cached veneer. */
  return address;
}
static void stack_fail(void) {
  as_native_fail(-1001);
  abort();
}
static int guest_getpid(void) {
  u64 id;
  if (R_FAILED(svcGetProcessId(&id, CUR_PROCESS_HANDLE)) || id > INT32_MAX) {
    errno = EOVERFLOW;
    return -1;
  }
  return (int)id;
}
static int guest_gettid(void) {
  u64 id;
  if (R_FAILED(svcGetThreadId(&id, CUR_THREAD_HANDLE)) || id > INT32_MAX) {
    errno = EOVERFLOW;
    return -1;
  }
  return (int)id;
}
static long guest_syscall(long number, ...) {
  if (number == 172)
    return guest_getpid();
  if (number == 178)
    return guest_gettid();
  /* All other Linux calls require explicit adapters. Never issue a Horizon
   * SVC with Linux's syscall number or pretend an unsupported call succeeded.
   */
  errno = ENOSYS;
  return -1;
}
static int *guest_errno(void) { return &context.android_errno; }
extern uint64_t as_bionic_io_import(const char *name);
extern uint64_t as_media_import(const char *name) __attribute__((weak));
extern int as_media_library(const char *name) __attribute__((weak));
int as_native_library(const char *name) {
  const char *base[] = {"libc.so",   "libm.so",       "libdl.so",
                        "liblog.so", "libandroid.so", "libstdc++.so"};
  for (size_t i = 0; i < sizeof(base) / sizeof(base[0]); i++)
    if (!strcmp(name, base[i]))
      return 1;
  return as_media_library ? as_media_library(name) : 0;
}
/* Exit paths never unmap a live thread. Timeout is a hard boundary: caller
 * records the failure and exits the Horizon process before another app starts.
 */
int as_native_quiesce(uint64_t timeout_ns) {
  uint64_t deadline = armGetSystemTick() + armNsToTicks(timeout_ns);
  for (;;) {
    mutexLock(&resource_mutex);
    GuestThread *t = guest_threads;
    Handle handle = t ? t->thread.handle : INVALID_HANDLE;
    int joining = t ? t->joining : 0;
    if (t && !joining)
      t->joining = 2;
    mutexUnlock(&resource_mutex);
    if (!t)
      return 0;
    uint64_t now = armGetSystemTick();
    if (now >= deadline)
      return -ETIMEDOUT;
    if (joining) {
      svcSleepThread(1000000);
      continue;
    }
    int32_t index;
    Result rc = svcWaitSynchronization(&index, &handle, 1,
                                       armTicksToNs(deadline - now));
    if (R_FAILED(rc))
      return -ETIMEDOUT;
    threadWaitForExit(&t->thread);
    threadClose(&t->thread);
    mutexLock(&resource_mutex);
    GuestThread **p = &guest_threads;
    while (*p && *p != t)
      p = &(*p)->next;
    if (*p)
      *p = t->next;
    mutexUnlock(&resource_mutex);
    free(t);
  }
}
/* Fixed component names only, with at most nine additional phase records per
 *
 * attempt. Logging never changes admission/ownership or calls guest code. */
static void reset_phase(const char *phase) {
  int saved = errno;
  as_teardown_phase(phase);
  errno = saved;
}
int as_native_reset(void) {
  as_teardown_phase("native_resource_reset");
  reset_phase("native_reset_guest_threads");
  if (guest_threads) {
    reset_phase("native_reset_guest_threads_busy");
    return -EBUSY;
  }
  reset_phase("native_reset_cxx");
  if (as_bionic_cxx_reset()) {
    reset_phase("native_reset_cxx_busy");
    unsigned pending = as_bionic_cxx_pending();
    if (pending & AS_CXX_PENDING_GLOBAL)
      reset_phase("native_reset_cxx_global_destructors_pending");
    if (pending & AS_CXX_PENDING_THREAD)
      reset_phase("native_reset_cxx_thread_destructors_pending");
    return -EBUSY;
  }
  reset_phase("native_reset_keys");
  if (as_bionic_keys_reset()) {
    reset_phase("native_reset_keys_busy");
    return -EBUSY;
  }
  extern int as_bionic_sync_reset(void);
  reset_phase("native_reset_sync");
  if (as_bionic_sync_reset()) {
    reset_phase("native_reset_sync_busy");
    return -EBUSY;
  }
  reset_phase("native_reset_locale");
  if (as_bionic_locale_reset()) {
    reset_phase("native_reset_locale_busy");
    return -EBUSY;
  }
  reset_phase("native_reset_admitted");
  extern void as_jni_reset(void);
  as_jni_reset();
  as_bionic_dlfcn_reset();
  while (mappings) {
    Mapping *before = mappings;
    as_native_release((uint64_t)before);
    if (mappings == before)
      return -EIO;
  }
  while (guest_allocations)
    guest_free(guest_allocations->pointer);
  reset_phase("native_reset_anonymous_memory");
  if (as_bionic_memory_reset()) return -EIO;
  extern void as_bionic_io_reset(void);
  as_bionic_io_reset();
  extern void as_native_missing_reset(void);
  as_native_missing_reset();
  extern void as_bionic_libc_reset(void);
  as_bionic_libc_reset();
  extern void as_bionic_log_reset(void);
  as_bionic_log_reset();
  as_teardown_phase("native_tls_reset");
  while (context.emulated) {
    EmulatedTls *e = context.emulated;
    context.emulated = e->next;
    free(e->bytes);
    free(e);
  }
  free(context.guest);
  memset(&context, 0, sizeof(context));
  memset(tls_template, 0, sizeof(tls_template));
  memset(tls_modules, 0, sizeof(tls_modules));
  tls_used = 256;
  module_log_entries = 0;
  diagnostics_ready = native_trace_enabled = 0;
  native_trace_calls = jni_failure_entries = 0;
  native_trace_metadata_entries = 0;
  native_trace_started = jni_failures_started = 0;
  if (own_process) {
    svcCloseHandle(own_process);
    own_process = 0;
  }
  as_teardown_phase("native_resources_released");
  /* Every earlier failure keeps the common admission barrier closed. */
  as_worker_admission_reset();
  return 0;
}
uint64_t as_native_import(const char *name) {

  struct {
    const char *name;
    void *function;
  } imports[] = {{"malloc", guest_malloc},
                 {"calloc", guest_calloc},
                 {"posix_memalign", guest_posix_memalign},
                 {"dl_iterate_phdr", guest_iterate_phdr},
                 {"getpid", guest_getpid},
                 {"gettid", guest_gettid},
                 {"syscall", guest_syscall},
                 {"realloc", guest_realloc},
                 {"free", guest_free},
                 {"strdup", guest_strdup},
                 {"snprintf", snprintf},
                 {"vsnprintf", vsnprintf},
                 {"memcpy", memcpy},
                 {"memmove", memmove},
                 {"memset", memset},
                 {"memcmp", memcmp},
                 {"strlen", strlen},
                 {"strcmp", strcmp},
                 {"strncmp", strncmp},
                 {"strcpy", strcpy},
                 {"sqrt", sqrt},
                 {"sqrtf", sqrtf},
                 {"sin", sin},
                 {"cos", cos},
                 {"floor", floor},
                 {"ceil", ceil},
                 {"__errno", guest_errno},
                 {"__stack_chk_fail", stack_fail},
                 {"__emutls_get_address", guest_emutls},
                 {"__tls_get_addr", guest_tls_address},
                 {"pthread_create", guest_pthread_create},
                 {"pthread_join", guest_pthread_join},
                 {"pthread_self", guest_pthread_self}};
  for (size_t i = 0; i < sizeof(imports) / sizeof(imports[0]); i++)
    if (!strcmp(name, imports[i].name))
      return (uint64_t)imports[i].function;
  extern uint64_t as_bionic_vfs_import(const char *);
  extern uint64_t as_bionic_log_import(const char *);
  extern uint64_t as_bionic_libc_import(const char *);
  uint64_t address = as_bionic_dlfcn_import(name);
  if (!address)
    address = as_bionic_memory_import(name);
  if (!address)
    address = as_bionic_libc_import(name);
  if (!address)
    address = as_bionic_vfs_import(name);
  if (!address)
    address = as_bionic_log_import(name);
  if (!address)
    address = as_bionic_cxx_import(name);
  if (!address)
    address = as_bionic_io_import(name);
  extern uint64_t as_bionic_sync_import(const char *);
  if (!address)
    address = as_bionic_sync_import(name);
  if (!address)
    address = as_bionic_keys_import(name);
  return address ? address : (as_media_import ? as_media_import(name) : 0);
}

/* Data symbols never receive function veneers: their address is the value used
 * by ELF relocations, not a callable import bridge. */
uint64_t as_native_data_import(const char *name) {
  if (!strcmp(name, "__stack_chk_guard"))
    return (uint64_t)&guard;
  uint64_t address = as_bionic_cxx_data(name);
  extern uint64_t as_bionic_io_data(const char *);
  if (!address)
    address = as_bionic_io_data(name);
  if (!address && as_media_import && !strncmp(name, "SL_IID_", 7))
    address = as_media_import(name);
  return address;
}

/* Resolve only actual linked providers in the requested library namespace. */
uint64_t as_native_system_symbol(const char *library, const char *name, int *data) {
  *data = 0;
  if (!library) {
    uint64_t address = as_native_data_import(name);
    if (address) { *data=1; return address; }
    return as_native_import(name);
  }
  if (!strcmp(library,"libdl.so")) return as_bionic_dlfcn_import(name);
  if (as_media_library && as_media_library(library)) {
    int belongs = (!strcmp(library,"libvulkan.so") && !strncmp(name,"vk",2)) || (!strcmp(library,"libEGL.so") && !strncmp(name,"egl",3)) ||
      (!strcmp(library,"libGLESv2.so") && !strncmp(name,"gl",2)) ||
      (!strcmp(library,"libaaudio.so") && !strncmp(name,"AAudio",6)) ||
      (!strcmp(library,"libOpenSLES.so") && (!strncmp(name,"sl",2)||!strncmp(name,"SL_",3))) ||
      (!strcmp(library,"libmediandk.so") && !strncmp(name,"AMedia",6));
    if (!belongs) return 0;
    if (!strncmp(name,"SL_IID_",7)) *data=1;
    return as_media_import ? as_media_import(name) : 0;
  }
  if (!strcmp(library,"libandroid.so")) return name[0]=='A' && as_media_import ? as_media_import(name) : 0;
  if (!strcmp(library,"liblog.so")) { extern uint64_t as_bionic_log_import(const char *);return as_bionic_log_import(name); }
  if (!strcmp(library,"libstdc++.so")) {
    uint64_t address=as_bionic_cxx_data(name);
    if(address) { *data=1;return address; } return as_bionic_cxx_import(name);
  }
  if (!strcmp(library,"libc.so") || !strcmp(library,"libm.so")) {
    // Device/media and libdl symbols belong to their own system providers.
    if (!strncmp(name,"egl",3)||!strncmp(name,"gl",2)||!strncmp(name,"vk",2)||name[0]=='A'||!strncmp(name,"dl",2)||!strncmp(name,"__android_log",13)) return 0;
    uint64_t address=as_native_data_import(name);
    if(address) { *data=1;return address; }return as_native_import(name);
  }
  return 0;
}
