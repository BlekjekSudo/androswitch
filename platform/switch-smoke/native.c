/* Horizon code-memory mapping follows the generic so_util technique by
 * Copyright (C) 2021 Andy Nguyen, fgsfds, under MIT
 * (third_party/so_util.LICENSE). Program-header planning/relocations are
 * implemented in Rust, without game hooks. Remaining AndroSwitch code: MIT OR
 * Apache-2.0. */
#include "native.h"
#include <errno.h>
#include <malloc.h>
#include <math.h>
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>

typedef struct Mapping {
  void *backing, *address;
  size_t size;
  VirtmemReservation *reservation;
  int mapped;
  uint8_t *executable;
  struct Mapping *next;
} Mapping;
static Handle own_process;
static Mapping *mappings;
static Mutex resource_mutex;
typedef struct GuestAllocation {
  void *pointer;
  size_t size;
  struct GuestAllocation *next;
} GuestAllocation;
static GuestAllocation *guest_allocations;
static void *guest_malloc(size_t size) {
  if (size > (128u << 20)) {
    errno = ENOMEM;
    return NULL;
  }
  void *p = malloc(size ? size : 1);
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
  free(m);
}

extern void as_native_import_bridge(void);
extern void as_native_tlsdesc(void);
extern void as_native_call_asm(AsNativeCall *);
/* Private host TLS: all host callbacks run with the libnx TP, never the guest
 * TP. */
typedef struct Context {
  void *session;
  uint8_t *guest;
  int error;
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
} ImportBridge;
_Static_assert(sizeof(ImportBridge) < 4096 - 32, "bridge prefix capacity");
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
  as_native_call(&call);
  thread->result = call.result_integer[0];
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
    return ENOTSUP;
  }
  GuestThread *thread = calloc(1, sizeof(*thread));
  if (!thread)
    return ENOMEM;
  thread->function = (uint64_t)function;
  thread->argument = (uint64_t)argument;
  thread->session = context.session;
  Result rc = threadCreate(&thread->thread, guest_thread_main, thread, NULL,
                           1024 * 1024, 0x2c, -2);
  if (R_FAILED(rc)) {
    free(thread);
    return EAGAIN;
  }
  rc = threadStart(&thread->thread);
  if (R_FAILED(rc)) {
    threadClose(&thread->thread);
    free(thread);
    return EAGAIN;
  }
  *out = (uint64_t)thread;
  mutexLock(&resource_mutex);
  thread->next = guest_threads;
  guest_threads = thread;
  mutexUnlock(&resource_mutex);
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
    return EDEADLK;
  }
  found->joining = 1;
  mutexUnlock(&resource_mutex);
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
  return 0;
}
static uint64_t guest_pthread_self(void) {
  return context.thread_identity ? context.thread_identity
                                 : (uint64_t)threadGetCurHandle();
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
void as_native_set_session(void *session) { context.session = session; }
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
  frame->guest_tls = as_native_guest_tls();
  if (!frame->guest_tls)
    return -ENOMEM;
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
static int *guest_errno(void) { return &errno; }
extern uint64_t as_bionic_io_import(const char *name);
extern uint64_t as_media_import(const char *name) __attribute__((weak));
extern int as_media_library(const char *name) __attribute__((weak));
int as_native_library(const char *name) {
  const char *base[] = {"libc.so", "libm.so", "libdl.so", "liblog.so",
                        "libandroid.so"};
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
int as_native_reset(void) {
  if (guest_threads)
    return -EBUSY;
  extern int as_bionic_sync_reset(void);
  if (as_bionic_sync_reset())
    return -EBUSY;
  extern void as_jni_reset(void);
  as_jni_reset();
  while (mappings) {
    Mapping *before = mappings;
    as_native_release((uint64_t)before);
    if (mappings == before)
      return -EIO;
  }
  while (guest_allocations)
    guest_free(guest_allocations->pointer);
  extern void as_bionic_io_reset(void);
  as_bionic_io_reset();
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
  if (own_process) {
    svcCloseHandle(own_process);
    own_process = 0;
  }
  return 0;
}
uint64_t as_native_import(const char *name) {
  if (!strcmp(name, "__stack_chk_guard"))
    return (uint64_t)&guard;
  struct {
    const char *name;
    void *function;
  } imports[] = {{"malloc", guest_malloc},
                 {"calloc", guest_calloc},
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
  uint64_t address = as_bionic_io_import(name);
  extern uint64_t as_bionic_sync_import(const char *);
  if (!address)
    address = as_bionic_sync_import(name);
  return address ? address : (as_media_import ? as_media_import(name) : 0);
}
