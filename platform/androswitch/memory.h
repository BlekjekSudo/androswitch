#ifndef ANDROSWITCH_MEMORY_H
#define ANDROSWITCH_MEMORY_H

#include <stdbool.h>
#include <stdint.h>

/* Snapshot estimates include all users of the process newlib heap, not just
 * Rust or Android's managed VM. Concurrent allocations may consume the result.
 * Zero means no available space; it must not be interpreted as unlimited. */
uint64_t as_host_heap_capacity(void);
uint64_t as_host_memory_available(void);
void as_memory_log(const char *stage);
void as_memory_stage(const char *stage);
bool as_memory_runtime_ready(void);

#endif
