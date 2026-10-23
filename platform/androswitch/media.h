/* Android ARM64 ABI declarations derived from NDK r28c public headers.
 * Android contracts: AOSP Apache-2.0; implementation: MIT OR Apache-2.0.
 * These are wire layouts, not host Android objects. */
#pragma once
#include "native.h"
#include <stddef.h>
#include <stdint.h>
typedef struct {
  uint64_t callbacks[16];
} AsActivityCallbacks;
typedef struct {
  AsActivityCallbacks *callbacks;
  void *vm, *env;
  uint64_t clazz;
  const char *internal_path, *external_path;
  int32_t sdk;
  void *instance, *assets;
  const char *obb_path;
} AsActivity;
AsActivity *as_activity_create(const char *package, uint64_t object);
int as_activity_start(AsActivity *activity);
int as_activity_stop(AsActivity *activity);
void as_activity_dispose(AsActivity *activity);
void as_media_input(int kind, int code, float x, float y);
int as_media_finished(void);
int as_media_reset(void);
int as_media_quiesce(void);
int as_media_error(void);
void as_media_capture(int enabled);
uint64_t as_media_frames(void);
uint64_t as_media_audio_frames(void);
int64_t as_audio_open(int rate, int channels, int format);
int as_audio_start(uint64_t id);
int64_t as_audio_write(uint64_t id, const void *data, size_t frames,
                       int64_t timeout);
int as_audio_stop(uint64_t id);
int as_audio_close(uint64_t id);
