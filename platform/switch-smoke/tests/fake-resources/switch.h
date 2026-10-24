#pragma once
#include <stdint.h>
typedef uint64_t u64;
typedef uint32_t Result;
typedef uint32_t Handle;
enum { InfoType_CoreMask = 0 };
#define CUR_PROCESS_HANDLE UINT32_C(0xffff8001)
#define R_FAILED(value) ((value) != 0)
Result svcGetInfo(u64 *output, uint32_t type, Handle handle, u64 sub_id);
