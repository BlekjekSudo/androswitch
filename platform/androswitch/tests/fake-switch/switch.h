/* Host-only libnx stand-in for the GLSurface worker contract tests. */
#pragma once
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>

typedef uint32_t Result;
typedef uint32_t Handle;
typedef struct {
  int locked;
} Mutex;
typedef struct {
  Handle handle;
  pthread_t native;
  void (*entry)(void *);
  void *argument;
  int started, joined, done;
} Thread;
#define R_SUCCEEDED(result) ((result) == 0)
#define R_FAILED(result) ((result) != 0)
void mutexLock(Mutex *mutex);
void mutexUnlock(Mutex *mutex);
Result threadCreate(Thread *, void (*)(void *), void *, void *, size_t, int,
                    int);
Result threadStart(Thread *);
Result threadWaitForExit(Thread *);
Result threadClose(Thread *);
Handle threadGetCurHandle(void);
Result svcWaitSynchronization(int32_t *, const Handle *, int, int64_t);
Result svcSleepThread(int64_t);
