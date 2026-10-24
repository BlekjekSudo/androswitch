/* Host test boundary: actual pthreads, elapsed waits and event wakeups. */
#pragma once
#include <pthread.h>
#include <stdbool.h>
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
typedef struct {
  int signal;
} UEvent;
typedef struct {
  UEvent *event;
} Waiter;
#define R_SUCCEEDED(result) ((result) == 0)
#define R_FAILED(result) ((result) != 0)
#define R_VALUE(result) (result)
#define KERNELRESULT_TimedOut 1u
#define KERNELRESULT(name) KERNELRESULT_##name
void mutexLock(Mutex *);
void mutexUnlock(Mutex *);
Result threadCreate(Thread *, void (*)(void *), void *, void *, size_t, int,
                    int);
Result threadStart(Thread *);
Result threadWaitForExit(Thread *);
Result threadClose(Thread *);
Handle threadGetCurHandle(void);
Result svcWaitSynchronization(int32_t *, const Handle *, int, int64_t);
void ueventCreate(UEvent *, bool);
void ueventClear(UEvent *);
void ueventSignal(UEvent *);
static inline Waiter waiterForUEvent(UEvent *event) {
  Waiter waiter = {event};
  return waiter;
}
Result waitSingle(Waiter, uint64_t);
uint64_t armGetSystemTick(void);
static inline uint64_t armTicksToNs(uint64_t value) { return value; }
