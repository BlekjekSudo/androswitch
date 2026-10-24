#ifndef ANDROSWITCH_MEMORY_POLICY_H
#define ANDROSWITCH_MEMORY_POLICY_H

#include <stdbool.h>
#include <stdint.h>

#define AS_MEMORY_MIB UINT64_C(1048576)
#define AS_HEAP_ALIGNMENT (UINT64_C(2) * AS_MEMORY_MIB)
#define AS_HEAP_MAXIMUM (UINT64_C(1536) * AS_MEMORY_MIB)
#define AS_HEAP_FALLBACK (UINT64_C(256) * AS_MEMORY_MIB)
#define AS_HEAP_OUTSIDE_RESERVE (UINT64_C(128) * AS_MEMORY_MIB)

enum AsHeapSource {
  AsHeapSource_ProcessLimit,
  AsHeapSource_LoaderOverride,
  AsHeapSource_QueryUnavailable,
};

struct AsHeapPlan {
  uint64_t heap_bytes;
  uint64_t outside_reserve_bytes;
  enum AsHeapSource source;
};

/* Only svcSetHeapSize sizes need 2 MiB alignment. A loader-owned override must
 * retain its supplied bounds: it may be a suballocation of a larger heap.
 * The normal allocation leaves room for kernel/service allocations outside
 * newlib; GPU buffers, VM and native allocations within newlib share its cap.
 * The 1536/128 MiB limits are conservative policy, not a RAM size guarantee. */
static inline struct AsHeapPlan as_heap_plan(bool have_process_limit,
                                             uint64_t total, uint64_t used,
                                             uint64_t override_bytes) {
  if (override_bytes)
    return (struct AsHeapPlan){override_bytes, 0, AsHeapSource_LoaderOverride};
  if (!have_process_limit)
    return (struct AsHeapPlan){AS_HEAP_FALLBACK, 0,
                               AsHeapSource_QueryUnavailable};
  uint64_t available = total > used ? total - used : 0;
  uint64_t reserve =
      available < AS_HEAP_OUTSIDE_RESERVE ? available : AS_HEAP_OUTSIDE_RESERVE;
  uint64_t heap = (available - reserve) & ~(AS_HEAP_ALIGNMENT - 1);
  if (heap > AS_HEAP_MAXIMUM)
    heap = AS_HEAP_MAXIMUM;
  return (struct AsHeapPlan){heap, available - heap, AsHeapSource_ProcessLimit};
}

/* Aggregate free space is an estimate, not a largest-contiguous-block claim.
 * Clamp malformed/inconsistent snapshots instead of wrapping their unsigned
 * sums. Newlib has already obtained arena bytes; the rest is still untouched.
 */
static inline uint64_t as_heap_free_estimate(uint64_t capacity, uint64_t arena,
                                             uint64_t allocator_free) {
  uint64_t committed = arena < capacity ? arena : capacity;
  uint64_t recycled = allocator_free < committed ? allocator_free : committed;
  return capacity - committed + recycled;
}

#endif
