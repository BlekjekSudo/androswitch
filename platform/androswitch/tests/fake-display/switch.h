/* Host-only libnx boundary for production display ownership policy tests. */
#pragma once
#include <stdint.h>
typedef uint32_t Result;
typedef struct {
  int locked;
} Mutex;
typedef struct {
  uint64_t slots_configured;
  unsigned width, height, transform;
} NWindow;
#define R_FAILED(result) ((result) != 0)
#define NATIVE_WINDOW_TRANSFORM_ROT_90 4u
#define NATIVE_WINDOW_TRANSFORM_ROT_180 3u
#define NATIVE_WINDOW_TRANSFORM_ROT_270 7u
void mutexLock(Mutex *);
void mutexUnlock(Mutex *);
NWindow *nwindowGetDefault(void);
Result nwindowSetDimensions(NWindow *, unsigned, unsigned);
Result nwindowSetTransform(NWindow *, unsigned);
