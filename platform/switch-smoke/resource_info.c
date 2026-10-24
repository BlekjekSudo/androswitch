#include "resource_info.h"
#include <switch.h>

int as_platform_available_processors(uint32_t *output) {
    if (!output)
        return -EINVAL;
    u64 mask = 0;
    Result result = svcGetInfo(&mask, InfoType_CoreMask, CUR_PROCESS_HANDLE, 0);
    if (R_FAILED(result))
        return (int)result;
    return as_allowed_processor_count(mask, output);
}
