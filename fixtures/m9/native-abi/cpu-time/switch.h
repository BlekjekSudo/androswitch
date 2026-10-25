#pragma once
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
typedef uint64_t u64;
typedef uint32_t Result;
typedef uint32_t Handle;
typedef void (*ThreadFunc)(void *);
typedef struct {
  Handle handle;
} Thread;
typedef struct {
  int held;
} Mutex;
static inline void mutexLock(Mutex *m) {
  assert(!m->held);
  m->held = 1;
}
static inline void mutexUnlock(Mutex *m) {
  assert(m->held);
  m->held = 0;
}
#define R_FAILED(rc) ((rc) != 0)
#define R_SUCCEEDED(rc) ((rc) == 0)
#define CUR_THREAD_HANDLE 0xffff8000u
#define TickCountInfo_Total UINT64_MAX
#define InfoType_ThreadTickCount 25
#define InfoType_ThreadTickCountDeprecated 0xf0000002u
#define Module_Libnx 345
#define LibnxError_OutOfMemory 2
#define MAKERESULT(m, d) ((Result)((m) | ((d) << 9)))
uint32_t hosversionGet(void);
int hosversionAtLeast(int, int, int);
Result svcGetInfo(u64 *, uint32_t, Handle, u64);
u64 armGetSystemTickFreq(void);
Handle threadGetCurHandle(void);
