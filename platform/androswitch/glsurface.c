/* Real EGL render thread for the shared GLSurfaceView Renderer contract.
 * Original implementation, MIT OR Apache-2.0; no application identities.
 */
#include "glsurface.h"
#include "media.h"
#include <errno.h>
#include <string.h>
#include <switch.h>
extern int as_application_gl_call(uint64_t, uint64_t, uint32_t, int, int, int,
                                  uint64_t, uint64_t);
extern int as_application_gl_poll(uint64_t, uint64_t, uint32_t);
extern uint64_t as_application_gl_choose(uint64_t, uint64_t, uint32_t,
                                         uint64_t);
extern void as_native_thread_cleanup(void);
static struct {
  Mutex mutex;
  Thread thread;
  AsGlSurfaceRequest request;
  int active, started, joining, stopping, close_error;
} renderer;
static int stopping(void) {
  mutexLock(&renderer.mutex);
  int value = renderer.stopping;
  mutexUnlock(&renderer.mutex);
  return value;
}
static int callback(const AsGlSurfaceRequest *r, int event,
                    const AsEglSurface *s) {
  return as_application_gl_call(r->vm, r->android, r->view, event, s->width,
                                s->height, s->context, s->config);
}
static int close_surface(const AsGlSurfaceRequest *r, AsEglSurface *s) {
  if (!s->display)
    return 0;
  int had_context = s->context != 0;
  int error = as_media_egl_close(s);
  if (error) {
    /* Ownership is uncertain after an EGL failure. Do not retry from another
     * thread, accept another renderer, or let a second quiesce hide it. */
    mutexLock(&renderer.mutex);
    renderer.close_error = error;
    mutexUnlock(&renderer.mutex);
    return error;
  }
  return had_context ? callback(r, 6, s) : 0;
}
static void render_thread(void *unused) {
  (void)unused;
  AsGlSurfaceRequest r = renderer.request;
  AsEglSurface s = {0};
  as_native_set_session((void *)(uintptr_t)r.session);
  int error = callback(&r, 0, &s);
  int attached = error == 0, close_failed = 0;
  while (!error && !stopping()) {
    int state = as_application_gl_poll(r.vm, r.android, r.view);
    if (state < 0) {
      error = state;
      break;
    }
    if (!(state & 1))
      break;
    /* AOSP GLThread drains queued Runnables before requiring a current EGL
     * context. A queueEvent does not promise GL resources are available. */
    if (state & 8) {
      error = callback(&r, 4, &s);
      if (error)
        break;
    }
    if (state & 2) {
      if ((state & 16) && s.surface) {
        error = as_media_egl_suspend(&s);
        if (error) {
          mutexLock(&renderer.mutex);
          renderer.close_error = error;
          mutexUnlock(&renderer.mutex);
          close_failed = 1;
          break;
        }
      } else if (!(state & 16) && s.display) {
        error = close_surface(&r, &s);
        if (error) {
          close_failed = s.display != 0;
          break;
        }
      }
      svcSleepThread(5000000);
      continue;
    }
    if (!s.context) {
      error = as_media_egl_initialize(&s);
      uint64_t config =
          error ? 0
                : as_application_gl_choose(r.vm, r.android, r.view, s.display);
      if (!error && !config)
        error = -2302;
      if (!error)
        error = as_media_egl_create(&s, config, r.client_version);
      if (!error)
        error = callback(&r, 1, &s);
      if (!error)
        error = callback(&r, 2, &s);
      if (error)
        break;
    } else if (!s.surface) {
      error = as_media_egl_resume(&s);
      if (!error)
        error = callback(&r, 2, &s);
      if (error)
        break;
    }
    if (state & 4) {
      error = callback(&r, 3, &s);
      if (!error)
        error = as_media_egl_swap(&s);
    } else
      svcSleepThread(1000000);
  }
  if (!close_failed) {
    int closed = close_surface(&r, &s);
    if (closed && !error)
      error = closed;
  }
  as_native_thread_cleanup(); // TLS destructors may still enter DEX.
  if (attached) {
    /* Guest TLS cleanup clears its session too. Restore only this live owner
     * for the framework detach callback, after guest destructors have run. */
    as_native_set_session((void *)(uintptr_t)r.session);
    int detached = callback(&r, 5, &s);
    if (detached && !error)
      error = detached;
  }
  as_native_set_session(NULL);
  if (error)
    as_media_gl_fail(error);
}
int as_glsurface_start(const AsGlSurfaceRequest *r) {
  if (!r || !r->vm || !r->android || !r->session || !r->view ||
      r->client_version != 2)
    return -EINVAL;
  int admission = as_worker_admission_enter();
  if (admission)
    return admission;
  mutexLock(&renderer.mutex);
  if (renderer.active) {
    mutexUnlock(&renderer.mutex);
    as_worker_admission_leave();
    return -EBUSY;
  }
  if (renderer.close_error) {
    mutexUnlock(&renderer.mutex);
    as_worker_admission_leave();
    return -EIO;
  }
  renderer.request = *r;
  renderer.started = renderer.joining = renderer.stopping = 0;
  Result rc = threadCreate(&renderer.thread, render_thread, NULL, NULL,
                           1024 * 1024, 0x2c, -2);
  if (R_SUCCEEDED(rc)) {
    renderer.active = 1;
    rc = threadStart(&renderer.thread);
    if (R_FAILED(rc)) {
      if (R_SUCCEEDED(threadClose(&renderer.thread)))
        renderer.active = 0;
    } else
      renderer.started = 1;
  }
  if (!renderer.active)
    memset(&renderer.request, 0, sizeof(renderer.request));
  mutexUnlock(&renderer.mutex);
  as_worker_admission_leave();
  return R_SUCCEEDED(rc) ? 0 : -EIO;
}
int as_glsurface_running(void) {
  mutexLock(&renderer.mutex);
  int active = renderer.active;
  mutexUnlock(&renderer.mutex);
  return active;
}
int as_glsurface_quiesce(void) {
  mutexLock(&renderer.mutex);
  if (!renderer.active) {
    int error = renderer.close_error;
    mutexUnlock(&renderer.mutex);
    return error;
  }
  if (renderer.joining) {
    mutexUnlock(&renderer.mutex);
    return -EBUSY;
  }
  if (renderer.thread.handle == threadGetCurHandle()) {
    mutexUnlock(&renderer.mutex);
    return -EBUSY;
  }
  renderer.stopping = 1;
  renderer.joining = 1;
  Handle handle = renderer.thread.handle;
  int started = renderer.started;
  mutexUnlock(&renderer.mutex);
  as_teardown_phase("glsurface_join");
  int32_t index = -1;
  int join_error = 0;
  /* Driver destruction may retire GPU queues and shader workers after the
   * final guest callback. Bound the complete owned worker, not only callbacks.
   * The process-wide stop watchdog is still the ultimate deadline. */
  as_media_stop_event("glsurface_wait_begin", started, handle);
  if (started) {
    Result waited = svcWaitSynchronization(&index, &handle, 1, 6000000000);
    as_media_stop_event("glsurface_wait_end", R_FAILED(waited) ? -EBUSY : 0,
                        waited);
    if (R_FAILED(waited))
      join_error = -EBUSY;
  }
  if (!join_error && started) {
    Result waited = threadWaitForExit(&renderer.thread);
    as_media_stop_event("glsurface_thread_wait_end",
                        R_FAILED(waited) ? -EIO : 0, waited);
    if (R_FAILED(waited))
      join_error = -EIO;
  }
  if (!join_error) {
    Result closed = threadClose(&renderer.thread);
    as_media_stop_event("glsurface_thread_close_end",
                        R_FAILED(closed) ? -EIO : 0, closed);
    if (R_FAILED(closed))
      join_error = -EIO;
  }
  mutexLock(&renderer.mutex);
  renderer.joining = 0;
  if (join_error) {
    mutexUnlock(&renderer.mutex);
    return join_error;
  }
  int error = renderer.close_error;
  renderer.active = renderer.started = renderer.stopping = 0;
  memset(&renderer.request, 0, sizeof(renderer.request));
  mutexUnlock(&renderer.mutex);
  return error;
}
