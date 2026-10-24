/* Owned serial VM callback workers. Tokens are opaque, never object pointers.
 */
#pragma once
#include <stdint.h>

typedef struct {
  uint64_t vm, android, session, token;
} AsWorkerRequest;
/* Start creates a real thread immediately. The caller retains every request
 * owner until successful global quiescence. At most32 retained handles. */
int as_workers_start(const AsWorkerRequest *request);
int as_workers_wake(uint64_t token);
/* Signals only: never joins or interrupts an executing guest callback. */
int as_workers_cancel(uint64_t token);
/* One total nanosecond deadline for all workers. Failure preserves handles;
 * owners must remain live and the application VM cannot be reused. */
int as_workers_quiesce(uint64_t timeout_ns);
int as_platform_wall_millis(int64_t *value);
