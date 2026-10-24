#include "memory.h"
#include "memory_policy.h"
#include <inttypes.h>
#include <malloc.h>
#include <stdio.h>
#include <switch.h>
#include <sys/stat.h>

/* Original implementation using public libnx APIs. Autorun's native_heap.c and
 * runtime.c at 1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877 informed the
 * distinction between heap capacity, allocator usage and untouched/recycled
 * free space. No Wine allocator source or private newlib chunk layout is
 * incorporated. */
static struct AsHeapPlan heap_plan;
static Result total_query_result, used_query_result;
static u64 initial_total, initial_used;
extern char *fake_heap_start, *fake_heap_end;

/* libnx calls this before services, constructors or malloc are available.
 * Respect hbloader's existing heap; otherwise derive a checked bounded heap
 * from this process's limit, never from the machine's physical RAM total.
 * Reference: libnx nx/source/runtime/init.c v4.12.0 (__libnx_initheap). */
void __libnx_initheap(void) {
  void *address = NULL;
  total_query_result = svcGetInfo(&initial_total, InfoType_TotalMemorySize,
                                  CUR_PROCESS_HANDLE, 0);
  used_query_result =
      svcGetInfo(&initial_used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
  bool query_ok =
      R_SUCCEEDED(total_query_result) && R_SUCCEEDED(used_query_result);
  uint64_t override_bytes = 0;
  if (envHasHeapOverride()) {
    address = envGetHeapOverrideAddr();
    override_bytes = envGetHeapOverrideSize();
    if (!address || !override_bytes ||
        override_bytes > UINTPTR_MAX - (uintptr_t)address)
      diagAbortWithResult(MAKERESULT(Module_Libnx, LibnxError_HeapAllocFailed));
  }
  heap_plan =
      as_heap_plan(query_ok, initial_total, initial_used, override_bytes);
  if (!override_bytes &&
      (!heap_plan.heap_bytes ||
       R_FAILED(svcSetHeapSize(&address, heap_plan.heap_bytes))))
    diagAbortWithResult(MAKERESULT(Module_Libnx, LibnxError_HeapAllocFailed));
  fake_heap_start = address;
  fake_heap_end = (char *)address + heap_plan.heap_bytes;
}

uint64_t as_host_heap_capacity(void) { return heap_plan.heap_bytes; }

uint64_t as_host_memory_available(void) {
  struct mallinfo info = mallinfo();
  return as_heap_free_estimate(heap_plan.heap_bytes, info.arena, info.fordblks);
}

bool as_memory_runtime_ready(void) {
  AppletType type = appletGetAppletType();
  return (type == AppletType_Application ||
          type == AppletType_SystemApplication) &&
         heap_plan.heap_bytes >= AS_HEAP_FALLBACK;
}

void as_memory_log(const char *stage) {
  /* Logging is called only on the main lifecycle thread. Obtain a snapshot
   * before fopen allocates its own buffers; no allocator locks are held on I/O.
   * Kernel accounting includes the reserved process heap and is recorded
   * separately from newlib usage. It is not malloc's remaining capacity. */
  struct mallinfo info = mallinfo();
  u64 total = 0, used = 0;
  Result total_rc =
      svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
  Result used_rc =
      svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
  uint64_t free_estimate =
      as_heap_free_estimate(heap_plan.heap_bytes, info.arena, info.fordblks);
  mkdir("sdmc:/switch/androswitch", 0777);
  mkdir("sdmc:/switch/androswitch/logs", 0777);
  const char *path = "sdmc:/switch/androswitch/logs/memory.log";
  struct stat existing;
  /* Keep evidence bounded across repeated launcher/app handoffs. */
  FILE *file = fopen(
      path,
      stat(path, &existing) == 0 && existing.st_size < 262144 ? "ab" : "wb");
  if (!file)
    return;
  fprintf(file,
          "{\"stage\":\"%s\",\"tick\":%" PRIu64
          ",\"heap_source\":%u,\"heap_capacity\":%" PRIu64
          ",\"outside_heap_reserve\":%" PRIu64
          ",\"allocator_arena\":%zu,\"allocator_used\":%zu"
          ",\"allocator_free\":%zu,\"free_estimate\":%" PRIu64
          ",\"contiguous_free_known\":false,\"applet_type\":%u"
          ",\"initial_total\":%" PRIu64 ",\"initial_used\":%" PRIu64
          ",\"initial_total_result\":%u,\"initial_used_result\":%u"
          ",\"process_total\":%" PRIu64 ",\"process_used\":%" PRIu64
          ",\"total_result\":%u,\"used_result\":%u}\n",
          stage, armGetSystemTick(), (unsigned)heap_plan.source,
          heap_plan.heap_bytes, heap_plan.outside_reserve_bytes, info.arena,
          info.uordblks, info.fordblks, free_estimate,
          (unsigned)appletGetAppletType(), initial_total, initial_used,
          total_query_result, used_query_result, total, used, total_rc,
          used_rc);
  fclose(file);
}

void as_memory_stage(const char *stage) { as_memory_log(stage); }
