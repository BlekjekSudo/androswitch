#include "../memory_policy.h"
#include <assert.h>
#include <stdio.h>

int main(void) {
  const uint64_t mib = AS_MEMORY_MIB;
  struct AsHeapPlan plan = as_heap_plan(true, 3200 * mib, 48 * mib, 0);
  assert(plan.heap_bytes == 1536 * mib);
  assert(plan.outside_reserve_bytes == (3200 - 48 - 1536) * mib);
  plan = as_heap_plan(true, 1024 * mib + 17, 80 * mib, 0);
  assert(plan.heap_bytes == 816 * mib);
  assert(plan.outside_reserve_bytes >= 128 * mib);
  assert(plan.heap_bytes % AS_HEAP_ALIGNMENT == 0);
  assert(as_heap_plan(true, 128 * mib, 0, 0).heap_bytes == 0);
  assert(as_heap_plan(true, 0, UINT64_MAX, 0).heap_bytes == 0);
  assert(as_heap_plan(true, UINT64_MAX, UINT64_MAX, 0).heap_bytes == 0);
  plan = as_heap_plan(true, UINT64_MAX, 0, 0);
  assert(plan.heap_bytes == AS_HEAP_MAXIMUM);
  plan = as_heap_plan(false, 0, 0, 0);
  assert(plan.source == AsHeapSource_QueryUnavailable);
  assert(plan.heap_bytes == AS_HEAP_FALLBACK);
  plan = as_heap_plan(true, 3200 * mib, 0, 96 * mib + 17);
  assert(plan.source == AsHeapSource_LoaderOverride);
  assert(plan.heap_bytes == 96 * mib + 17);
  assert(plan.outside_reserve_bytes == 0);
  assert(as_heap_free_estimate(512 * mib, 200 * mib, 30 * mib) == 342 * mib);
  assert(as_heap_free_estimate(512 * mib, 0, 0) == 512 * mib);
  assert(as_heap_free_estimate(512 * mib, 600 * mib, 20 * mib) == 20 * mib);
  assert(as_heap_free_estimate(512 * mib, 200 * mib, 900 * mib) == 512 * mib);
  assert(as_heap_free_estimate(UINT64_MAX, UINT64_MAX, UINT64_MAX) ==
         UINT64_MAX);
  assert(as_heap_free_estimate(0, 100, 100) == 0);
  puts("memory policy: limits, overrides, reserve, alignment and snapshots "
       "pass");
  return 0;
}
