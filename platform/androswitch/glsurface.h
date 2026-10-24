#pragma once
#include <stddef.h>
#include <stdint.h>
typedef struct {
  uint64_t display, context, surface, config;
  int32_t width, height;
} AsEglSurface;
typedef struct {
  uint64_t vm, android, session;
  uint32_t view;
  int32_t client_version;
} AsGlSurfaceRequest;
int as_media_egl_initialize(AsEglSurface *s);
int as_media_egl_create(AsEglSurface *s, uint64_t config, int version);
int as_media_egl_swap(AsEglSurface *s);
int as_media_egl_suspend(AsEglSurface *s);
int as_media_egl_resume(AsEglSurface *s);
int as_media_egl_close(AsEglSurface *s);
int as_media_egl_choose(uint64_t d, const int32_t *a, size_t n,
                        uint64_t *configs, size_t capacity, int32_t *count);
int as_media_egl_attrib(uint64_t d, uint64_t c, int32_t attribute,
                        int32_t *value);
int as_media_keep_screen_on(int enabled);
void as_media_gl_fail(int code);
/* vm/android/session allocations remain live until successful quiescence. A
 * failure requires preserving those owners and terminating the process. */
int as_glsurface_start(const AsGlSurfaceRequest *request);
/* Reports an owned worker/thread handle, including one awaiting a join after
 * its render loop has exited. It does not assert that a frame was rendered. */
int as_glsurface_running(void);
int as_glsurface_quiesce(void);
