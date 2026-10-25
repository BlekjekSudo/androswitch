/* Host SDK synchronization adapter; unrelated to guest Linux syscall dispatch.
 */
#pragma once
#include <stdint.h>
#include <time.h>
int as_sdk_futex(uint32_t *, int, uint32_t, const struct timespec *, uint32_t);
uint64_t as_sdk_now_ns(int realtime);
