#include "resource_info.h"
#include <assert.h>
#include <stdio.h>
#include <switch.h>

static Result query_result;
static u64 allowed_mask;
static uint32_t query_count;

Result svcGetInfo(u64 *output, uint32_t type, Handle handle, u64 sub_id) {
    assert(output);
    assert(type == InfoType_CoreMask);
    assert(handle == CUR_PROCESS_HANDLE);
    assert(sub_id == 0);
    ++query_count;
    *output = allowed_mask;
    return query_result;
}

int main(void) {
    uint32_t count = UINT32_C(0x12345678);
    assert(as_platform_available_processors(NULL) == -EINVAL);
    assert(query_count == 0);
    for (unsigned bit = 0; bit < 64; ++bit) {
        allowed_mask = UINT64_C(1) << bit;
        assert(as_platform_available_processors(&count) == 0);
        assert(count == 1);
    }
    allowed_mask = UINT64_C(0x5); /* Sparse permissions, not max core ID + 1. */
    assert(as_platform_available_processors(&count) == 0 && count == 2);
    allowed_mask = UINT64_C(0x7);
    assert(as_platform_available_processors(&count) == 0 && count == 3);
    allowed_mask = UINT64_MAX;
    assert(as_platform_available_processors(&count) == 0 && count == 64);
    allowed_mask = 0;
    count = 100;
    assert(as_platform_available_processors(&count) == -EINVAL && count == 100);
    allowed_mask = UINT64_C(0xf);
    query_result = UINT32_C(0x4321);
    assert(as_platform_available_processors(&count) == (int)query_result);
    assert(count == 100); /* Failed query cannot publish even plausible data. */
    query_result = 0;
    allowed_mask = UINT64_C(0x8000000000000001);
    assert(as_platform_available_processors(&count) == 0 && count == 2);
    assert(query_count == 70);
    puts("RESOURCE_INFO_PASS:64 single bits,sparse/full masks,fresh queries,zero/error/null failures");
    return 0;
}
