/* Actual Horizon process/thread CPU accounting. MIT OR Apache-2.0. */
#pragma once
#include <stdint.h>
int as_cpu_time_init(void);
int as_cpu_time_query(int thread_only, uint64_t *nanoseconds);
