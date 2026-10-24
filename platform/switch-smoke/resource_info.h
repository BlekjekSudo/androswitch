#pragma once
#include <errno.h>
#include <stdint.h>

/* Count the actual process-allowed core mask, not all physical/online cores.
 * Do not write an output on invalid input or invent a one-core fallback. */
static inline int as_allowed_processor_count(uint64_t mask, uint32_t *output) {
    if (!mask || !output)
        return -EINVAL;
    uint32_t count = 0;
    while (mask) {
        count += (uint32_t)(mask & UINT64_C(1));
        mask >>= 1;
    }
    *output = count;
    return 0;
}

/* Return 0 only on a successful query and valid nonempty allowed mask.
 * Failed Horizon svc results are preserved; output is untouched on failure. */
int as_platform_available_processors(uint32_t *output);
