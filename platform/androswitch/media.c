/* General NDK assets, window/input, EGL/GLES2 and PCM backend.
 * AndroSwitch contributors, MIT OR Apache-2.0. Public Android ABI layouts are
 * derived from AOSP NDK r28c (Apache-2.0). No APK identity is consulted. */
#include "media.h"
#include "../../third_party/android/jni.h"
#include <EGL/egl.h>
#include <GLES2/gl2.h>
#include <errno.h>
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
#include <time.h>
extern int64_t as_guest_asset(uint32_t, uint64_t, const char *, int64_t,
                              int32_t, uint8_t *, size_t);
extern uint64_t as_jni_java_vm(void), as_jni_environment(void);
static int media_failure, finish_requested;
static uint64_t submitted_frames, played_frames;
static int diagnostic_capture;
void as_media_capture(int enabled) { diagnostic_capture = enabled; }
static int window_token, assets_token, queue_token;
static AsActivity *active_activity;
static void *owner;
static int failure(int code) {
  media_failure = code;
  return code;
}
int as_media_error(void) { return media_failure; }
int as_media_finished(void) { return finish_requested; }
uint64_t as_media_frames(void) {
  return __atomic_load_n(&submitted_frames, __ATOMIC_ACQUIRE);
}
uint64_t as_media_audio_frames(void) {
  return __atomic_load_n(&played_frames, __ATOMIC_ACQUIRE);
}

/* Asset handles own only an incremental ZIP reader ID. All Java/native readers
 * go through the same FrameworkHost and its validated archive. */
typedef struct Asset {
  uint64_t id;
  struct Asset *next;
} Asset;
static Asset *assets;
static Mutex asset_mutex;
static int64_t asset_op(uint32_t op, uint64_t id, const char *name,
                        int64_t offset, int origin, void *data, size_t length) {
  as_jni_vm_lock();
  int64_t rc = as_guest_asset(op, id, name, offset, origin, data, length);
  as_jni_vm_unlock();
  if (rc < 0)
    errno = (int)-rc;
  return rc;
}
static Asset *asset_open(void *manager, const char *name, int mode) {
  if (manager != &assets_token || mode < 0 || mode > 3) {
    errno = EINVAL;
    return NULL;
  }
  int64_t id = asset_op(0, 0, name, 0, 0, NULL, 0);
  if (id < 0) {
    errno = (int)-id;
    return NULL;
  }
  Asset *a = calloc(1, sizeof(*a));
  if (!a) {
    asset_op(4, id, NULL, 0, 0, NULL, 0);
    return NULL;
  }
  a->id = id;
  mutexLock(&asset_mutex);
  a->next = assets;
  assets = a;
  mutexUnlock(&asset_mutex);
  return a;
}
static void *asset_from_java(void *env, void *object) {
  if (!object || env != (void *)as_jni_environment())
    return NULL;
  return asset_op(5, (uint64_t)object, NULL, 0, 0, NULL, 0) == 1 ? &assets_token
                                                                 : NULL;
}
static int asset_read(Asset *a, void *out, size_t len) {
  return (int)asset_op(1, a->id, NULL, 0, 0, out, len);
}
static int64_t asset_seek(Asset *a, int64_t offset, int origin) {
  return asset_op(2, a->id, NULL, offset, origin, NULL, 0);
}
static int64_t asset_length(Asset *a) {
  return asset_op(3, a->id, NULL, 0, 0, NULL, 0);
}
static int64_t asset_remaining(Asset *a) {
  int64_t p = asset_seek(a, 0, 1);
  return p < 0 ? p : asset_length(a) - p;
}
static void asset_close(Asset *a) {
  mutexLock(&asset_mutex);
  Asset **p = &assets;
  while (*p && *p != a)
    p = &(*p)->next;
  if (!*p) {
    mutexUnlock(&asset_mutex);
    return;
  }
  *p = a->next;
  mutexUnlock(&asset_mutex);
  asset_op(4, a->id, NULL, 0, 0, NULL, 0);
  free(a);
}
static void *asset_buffer(Asset *a) {
  (void)a;
  errno = ENOTSUP;
  return NULL;
}

/* AInputQueue with one consumer and bounded event lifetime. ALooper_pollOnce
 * waits on actual clock/wake/input; arbitrary FD watchers remain explicit gaps.
 */
typedef struct {
  int type, action, code;
  float x, y;
  int64_t time;
} Input;
typedef struct {
  int ident;
  void *data;
  int wake;
  uint64_t callback;
} Looper;
static Input input[64], current_input;
static unsigned input_read, input_write;
static Mutex input_mutex;
static CondVar input_cond;
static Looper looper;
static Handle looper_owner;
static int looper_refs;
static void *input_data;
static int input_ident, input_attached, input_inflight;
void as_media_input(int kind, int code, float x, float y) {
  mutexLock(&input_mutex);
  if (input_write - input_read < 64) {
    Input *e = &input[input_write++ % 64];
    *e = (Input){kind == 4 ? 1 : 2,
                 kind == 4 ? 0 : kind,
                 code,
                 x,
                 y,
                 (int64_t)armTicksToNs(armGetSystemTick())};
    condvarWakeAll(&input_cond);
  }
  mutexUnlock(&input_mutex);
}
static Looper *looper_prepare(int options) {
  if (options & ~1) {
    failure(-2020);
    return NULL;
  }
  mutexLock(&input_mutex);
  if (looper_owner && looper_owner != threadGetCurHandle()) {
    mutexUnlock(&input_mutex);
    failure(-2020);
    return NULL;
  }
  if (!looper_owner) {
    looper_owner = threadGetCurHandle();
    looper_refs = 1;
  }
  mutexUnlock(&input_mutex);
  return &looper;
}
static Looper *looper_for_thread(void) {
  return looper_owner == threadGetCurHandle() ? &looper : NULL;
}
static void looper_acquire(Looper *l) {
  if (l != &looper)
    failure(-2020);
  else
    __atomic_add_fetch(&looper_refs, 1, __ATOMIC_RELAXED);
}
static void looper_release(Looper *l) {
  if (l != &looper || __atomic_sub_fetch(&looper_refs, 1, __ATOMIC_RELAXED) < 1)
    failure(-2020);
}
static void looper_wake(Looper *l) {
  if (l != &looper) {
    failure(-2020);
    return;
  }
  mutexLock(&input_mutex);
  l->wake = 1;
  condvarWakeAll(&input_cond);
  mutexUnlock(&input_mutex);
}
static void queue_attach(void *q, Looper *l, int ident, uint64_t callback,
                         void *data) {
  if (q != &queue_token || l != &looper || ident < 0) {
    failure(-2020);
    return;
  }
  mutexLock(&input_mutex);
  input_attached = 1;
  input_ident = ident;
  input_data = data;
  looper.callback = callback;
  condvarWakeAll(&input_cond);
  mutexUnlock(&input_mutex);
}
static void queue_detach(void *q) {
  if (q != &queue_token) {
    failure(-2020);
    return;
  }
  mutexLock(&input_mutex);
  input_attached = 0;
  mutexUnlock(&input_mutex);
}
static int looper_poll(int timeout, int *fd, int *events, void **data) {
  if (looper_owner != threadGetCurHandle())
    return -4;
  mutexLock(&input_mutex);
  if (!looper.wake && !(input_attached && input_read < input_write) &&
      timeout != 0)
    condvarWaitTimeout(&input_cond, &input_mutex,
                       timeout < 0 ? UINT64_MAX : (uint64_t)timeout * 1000000);
  int result = -3;
  if (looper.wake) {
    looper.wake = 0;
    result = -1;
  } else if (input_attached && input_read < input_write) {
    result = input_ident;
    if (fd)
      *fd = -1;
    if (events)
      *events = 1;
    if (data)
      *data = input_data;
  }
  uint64_t callback = looper.callback;
  void *callback_data = input_data;
  mutexUnlock(&input_mutex);
  if (result >= 0 && callback) {
    AsNativeCall c = {0};
    c.function = callback;
    c.integer[0] = (uint64_t)-1;
    c.integer[1] = 1;
    c.integer[2] = (uint64_t)callback_data;
    int rc = as_native_call(&c);
    if (rc)
      return failure(rc);
    if (!c.result_integer[0])
      queue_detach(&queue_token);
    return -2;
  }
  return result;
}
static int queue_has(void *q) {
  if (q != &queue_token)
    return -1;
  mutexLock(&input_mutex);
  int n = input_write != input_read;
  mutexUnlock(&input_mutex);
  return n;
}
static int queue_get(void *q, Input **event) {
  if (q != &queue_token || !event)
    return -1;
  mutexLock(&input_mutex);
  if (input_inflight || input_read == input_write) {
    mutexUnlock(&input_mutex);
    return -1;
  }
  current_input = input[input_read % 64];
  input_inflight = 1;
  *event = &current_input;
  mutexUnlock(&input_mutex);
  return 0;
}
static int queue_predispatch(void *q, Input *e) {
  return q == &queue_token && e == &current_input ? 0 : -1;
}
static void queue_finish(void *q, Input *e, int handled) {
  (void)handled;
  if (q != &queue_token || e != &current_input) {
    failure(-2021);
    return;
  }
  mutexLock(&input_mutex);
  if (input_inflight) {
    input_read++;
    input_inflight = 0;
  }
  mutexUnlock(&input_mutex);
}
static int event_type(Input *e) { return e->type; }
static int event_device(Input *e) {
  (void)e;
  return 0;
}
static int event_source(Input *e) { return e->type == 1 ? 0x401 : 0x1002; }
static int event_action(Input *e) { return e->action; }
static int key_code(Input *e) { return e->code; }
static size_t motion_count(Input *e) {
  (void)e;
  return 1;
}
static float motion_x(Input *e, size_t pointer) {
  return pointer == 0 ? e->x : 0;
}
static float motion_y(Input *e, size_t pointer) {
  return pointer == 0 ? e->y : 0;
}
static int64_t event_time(Input *e) { return e->time; }

/* The guest ANativeWindow is a token. Only this boundary translates it to a
 * real libnx native window, so APK pointers never masquerade as NWindow. */
static int window_refs;
static int window_width(void *w) { return w == &window_token ? 1280 : -EINVAL; }
static int window_height(void *w) { return w == &window_token ? 720 : -EINVAL; }
static int window_format(void *w) { return w == &window_token ? 1 : -EINVAL; }
static void window_acquire(void *w) {
  if (w == &window_token)
    __atomic_add_fetch(&window_refs, 1, __ATOMIC_RELAXED);
  else
    failure(-2030);
}
static void window_release(void *w) {
  if (w == &window_token)
    __atomic_sub_fetch(&window_refs, 1, __ATOMIC_RELAXED);
  else
    failure(-2030);
}
static int window_geometry(void *w, int width, int height, int format) {
  if (w != &window_token)
    return -EINVAL;
  return ((width == 0 && height == 0) || (width == 1280 && height == 720)) &&
                 (format == 0 || format == 1)
             ? 0
             : -ENOTSUP;
}
static EGLSurface surfaces[16];
static EGLContext contexts[16];
static EGLDisplay display;
static Mutex graphics_mutex;
static unsigned current_bindings;
static __thread int bound_context;
static EGLDisplay guest_display(EGLNativeDisplayType d) {
  if (d != EGL_DEFAULT_DISPLAY)
    return EGL_NO_DISPLAY;
  display = eglGetDisplay(d);
  return display;
}
static EGLSurface guest_surface(EGLDisplay d, EGLConfig config, void *window,
                                const EGLint *attrs) {
  if (window != &window_token) {
    failure(-2030);
    return EGL_NO_SURFACE;
  }
  mutexLock(&graphics_mutex);
  unsigned i = 0;
  while (i < 16 && surfaces[i])
    i++;
  EGLSurface s =
      i == 16 ? EGL_NO_SURFACE
              : eglCreateWindowSurface(
                    d, config, (EGLNativeWindowType)nwindowGetDefault(), attrs);
  if (s)
    surfaces[i] = s;
  mutexUnlock(&graphics_mutex);
  return s;
}
static EGLContext guest_context(EGLDisplay d, EGLConfig config,
                                EGLContext share, const EGLint *attrs) {
  mutexLock(&graphics_mutex);
  unsigned i = 0;
  while (i < 16 && contexts[i])
    i++;
  EGLContext c =
      i == 16 ? EGL_NO_CONTEXT : eglCreateContext(d, config, share, attrs);
  if (c)
    contexts[i] = c;
  mutexUnlock(&graphics_mutex);
  return c;
}
static EGLBoolean guest_make_current(EGLDisplay d, EGLSurface draw,
                                     EGLSurface read, EGLContext c) {
  if (bound_context && c == EGL_NO_CONTEXT) {
    /* An EGL swap submits asynchronously. Finish on the owning thread before
     * its context/surface and Nouveau channel can be destroyed. */
    as_teardown_phase("egl_wait_gpu");
    glFinish();
    as_teardown_phase("egl_unbind");
  }
  EGLBoolean ok = eglMakeCurrent(d, draw, read, c);
  if (ok) {
    mutexLock(&graphics_mutex);
    if (bound_context)
      current_bindings--;
    bound_context = c != EGL_NO_CONTEXT;
    if (bound_context)
      current_bindings++;
    mutexUnlock(&graphics_mutex);
  }
  return ok;
}
static EGLBoolean guest_destroy_surface(EGLDisplay d, EGLSurface s) {
  as_teardown_phase("egl_destroy_surface");
  EGLBoolean ok = eglDestroySurface(d, s);
  if (ok)
    for (unsigned i = 0; i < 16; i++)
      if (surfaces[i] == s)
        surfaces[i] = 0;
  return ok;
}
static EGLBoolean guest_destroy_context(EGLDisplay d, EGLContext c) {
  as_teardown_phase("egl_destroy_context");
  EGLBoolean ok = eglDestroyContext(d, c);
  if (ok)
    for (unsigned i = 0; i < 16; i++)
      if (contexts[i] == c)
        contexts[i] = 0;
  return ok;
}
/* The installed Mesa/NV staging path corrupts multi-row RGBA CPU transfers in
 * the recorded emulator. Preserve GLES2 PACK_ALIGNMENT while issuing bounded
 * scanline reads for applications too, not only for diagnostic captures. */
static void guest_read_pixels(GLint x, GLint y, GLsizei width, GLsizei height,
                              GLenum format, GLenum type, void *pixels) {
  if (width > 0 && height > 1 && format == GL_RGBA &&
      type == GL_UNSIGNED_BYTE && pixels) {
    GLint alignment = 4;
    glGetIntegerv(GL_PACK_ALIGNMENT, &alignment);
    size_t stride =
        ((size_t)width * 4 + (size_t)alignment - 1) & ~((size_t)alignment - 1);
    for (GLsizei row = 0; row < height; row++)
      glReadPixels(x, y + row, width, 1, format, type,
                   (uint8_t *)pixels + (size_t)row * stride);
  } else
    glReadPixels(x, y, width, height, format, type, pixels);
}
static EGLBoolean guest_swap(EGLDisplay d, EGLSurface s) {
  uint64_t frame = as_media_frames();
  if (diagnostic_capture && (frame == 2 || frame == 30)) {
    uint8_t *pixels = malloc(1280 * 720 * 4);
    if (pixels) {
      /* Read scanlines so the capture stays bounded and does not rely on a
       * large temporary transfer buffer in the Mesa/NV staging path. */
      memset(pixels, 0, 1280 * 720 * 4);
      glFinish();
      for (int y = 0; y < 720; y++)
        glReadPixels(0, y, 1280, 1, GL_RGBA, GL_UNSIGNED_BYTE,
                     pixels + y * 1280 * 4);
      if (glGetError() == GL_NO_ERROR) {
        char path[160];
        snprintf(path, sizeof(path),
                 "sdmc:/switch/androswitch/logs/gles-frame-%llu.ppm",
                 (unsigned long long)frame);
        FILE *f = fopen(path, "wb");
        if (f) {
          fprintf(f, "P6\n1280 720\n255\n");
          for (int y = 719; y >= 0; y--)
            for (int x = 0; x < 1280; x++)
              fwrite(pixels + (y * 1280 + x) * 4, 1, 3, f);
          fclose(f);
        }
      }
      free(pixels);
    }
  }
  EGLBoolean ok = eglSwapBuffers(d, s);
  if (ok)
    __atomic_add_fetch(&submitted_frames, 1, __ATOMIC_RELEASE);
  else
    failure(-2031);
  return ok;
}
static EGLBoolean guest_terminate(EGLDisplay d) {
  if (current_bindings)
    return EGL_FALSE;
  as_teardown_phase("egl_terminate");
  EGLBoolean ok = eglTerminate(d);
  if (ok)
    display = 0;
  as_teardown_phase(ok ? "egl_terminated" : "egl_terminate_failed");
  return ok;
}
static uint64_t media_lookup(const char *name);
static void *guest_proc(const char *name) {
  uint64_t fn = media_lookup(name);
  return fn ? (void *)as_native_veneer(fn) : NULL;
}

/* All exposed audio APIs use this real 48 kHz stereo PCM16 output. Requests
 * outside this format fail explicitly. Retain submitted memory until release.
 */
typedef struct {
  uint64_t id;
  int started, initialized;
  AudioOutBuffer buffer;
  void *bytes;
} Audio;
static Audio audio;
static Mutex audio_mutex;
static uint64_t audio_serial;
int64_t as_audio_open(int rate, int channels, int format) {
  mutexLock(&audio_mutex);
  if (audio.id) {
    mutexUnlock(&audio_mutex);
    return -EBUSY;
  }
  if (rate != 48000 || channels != 2 || format != 1) {
    mutexUnlock(&audio_mutex);
    return -ENOTSUP;
  }
  Result rc = audoutInitialize();
  if (R_FAILED(rc)) {
    mutexUnlock(&audio_mutex);
    return -(int64_t)rc;
  }
  if (audoutGetSampleRate() != 48000 || audoutGetChannelCount() != 2) {
    audoutExit();
    mutexUnlock(&audio_mutex);
    return -ENOTSUP;
  }
  audio.initialized = 1;
  audio.id = ++audio_serial;
  mutexUnlock(&audio_mutex);
  return audio.id;
}
int as_audio_start(uint64_t id) {
  mutexLock(&audio_mutex);
  int rc = -EINVAL;
  if (id == audio.id && id) {
    rc = audio.started ? 0 : (int)audoutStartAudioOut();
    if (!rc)
      audio.started = 1;
  }
  mutexUnlock(&audio_mutex);
  return rc;
}
int64_t as_audio_write(uint64_t id, const void *data, size_t frames,
                       int64_t timeout) {
  mutexLock(&audio_mutex);
  if (!id || audio.id != id || !audio.started || frames > 48000 || !data) {
    mutexUnlock(&audio_mutex);
    return -EINVAL;
  }
  if (frames == 0) {
    mutexUnlock(&audio_mutex);
    return 0;
  }
  if (audio.bytes) {
    mutexUnlock(&audio_mutex);
    return -EBUSY;
  }
  size_t bytes = (frames * 4 + 4095) & ~4095u;
  audio.bytes = memalign(4096, bytes);
  if (!audio.bytes) {
    mutexUnlock(&audio_mutex);
    return -ENOMEM;
  }
  memcpy(audio.bytes, data, frames * 4);
  audio.buffer = (AudioOutBuffer){
      .buffer = audio.bytes, .buffer_size = bytes, .data_size = frames * 4};
  armDCacheFlush(audio.bytes, bytes);
  Result rc = audoutAppendAudioOutBuffer(&audio.buffer);
  if (R_SUCCEEDED(rc)) {
    AudioOutBuffer *released = NULL;
    u32 count = 0;
    rc = audoutWaitPlayFinish(&released, &count,
                              timeout < 0 ? UINT64_MAX : (uint64_t)timeout);
    if (R_SUCCEEDED(rc) && released != &audio.buffer)
      rc = MAKERESULT(Module_Libnx, LibnxError_BadInput);
  }
  if (R_FAILED(rc)) { /* queued memory remains valid until stop/close confirms
                         release */
    mutexUnlock(&audio_mutex);
    return -(int64_t)rc;
  }
  free(audio.bytes);
  audio.bytes = NULL;
  __atomic_add_fetch(&played_frames, frames, __ATOMIC_RELEASE);
  mutexUnlock(&audio_mutex);
  return (int64_t)frames;
}
int as_audio_stop(uint64_t id) {
  mutexLock(&audio_mutex);
  if (!id || id != audio.id) {
    mutexUnlock(&audio_mutex);
    return -EINVAL;
  }
  Result rc = audio.started ? audoutStopAudioOut() : 0;
  if (R_SUCCEEDED(rc))
    audio.started = 0;
  mutexUnlock(&audio_mutex);
  return (int)rc;
}
int as_audio_close(uint64_t id) {
  if (as_audio_stop(id))
    return -EIO;
  mutexLock(&audio_mutex);
  audoutExit();
  free(audio.bytes);
  memset(&audio, 0, sizeof(audio));
  mutexUnlock(&audio_mutex);
  return 0;
}

/* AAudio builder/write subset; callback streams are an identified capability
 * gap until a dedicated realtime callback scheduler is implemented. */
typedef struct Builder {
  int rate, channels, format, direction, sharing, performance, invalid;
  uint64_t callback;
  struct Builder *next;
} Builder;
typedef struct Stream {
  uint64_t id;
  struct Stream *next;
} Stream;
static Builder *builders;
static Stream *streams;
static Mutex aa_mutex;
static int aa_create(Builder **b) {
  *b = calloc(1, sizeof(**b));
  if (!*b)
    return -896;
  (*b)->rate = 48000;
  (*b)->channels = 2;
  (*b)->format = 1;
  (*b)->sharing = 1;
  (*b)->performance = 10;
  mutexLock(&aa_mutex);
  (*b)->next = builders;
  builders = *b;
  mutexUnlock(&aa_mutex);
  return 0;
}
static int aa_delete(Builder *b) {
  mutexLock(&aa_mutex);
  Builder **p = &builders;
  while (*p && *p != b)
    p = &(*p)->next;
  if (!*p) {
    mutexUnlock(&aa_mutex);
    return -898;
  }
  *p = b->next;
  mutexUnlock(&aa_mutex);
  free(b);
  return 0;
}
static void aa_rate(Builder *b, int v) { b->rate = v ? v : 48000; }
static void aa_channels(Builder *b, int v) { b->channels = v ? v : 2; }
static void aa_format(Builder *b, int v) { b->format = v ? v : 1; }
static void aa_direction(Builder *b, int v) { b->direction = v; }
static void aa_sharing(Builder *b, int v) { b->sharing = v; }
static void aa_performance(Builder *b, int v) { b->performance = v; }
static void aa_callback(Builder *b, uint64_t fn, void *data) {
  (void)data;
  b->callback = fn;
}
static int aa_open(Builder *b, Stream **s) {
  if (!s || b->direction || b->sharing != 1 || b->performance != 10 ||
      b->callback || b->invalid)
    return -890;
  int64_t id = as_audio_open(b->rate, b->channels, b->format);
  if (id < 0)
    return -883;
  *s = calloc(1, sizeof(**s));
  if (!*s) {
    as_audio_close(id);
    return -896;
  }
  (*s)->id = id;
  mutexLock(&aa_mutex);
  (*s)->next = streams;
  streams = *s;
  mutexUnlock(&aa_mutex);
  return 0;
}
static int aa_start(Stream *s) { return as_audio_start(s->id) ? -899 : 0; }
static int aa_stop(Stream *s) { return as_audio_stop(s->id) ? -899 : 0; }
static int aa_close(Stream *s) {
  int rc = as_audio_close(s->id);
  if (!rc) {
    mutexLock(&aa_mutex);
    Stream **p = &streams;
    while (*p && *p != s)
      p = &(*p)->next;
    if (*p)
      *p = s->next;
    mutexUnlock(&aa_mutex);
    free(s);
  }
  return rc ? -899 : 0;
}
static int aa_write(Stream *s, const void *data, int frames, int64_t timeout) {
  if (frames < 0)
    return -898;
  int64_t n = as_audio_write(s->id, data, frames, timeout);
  return n < 0 ? -885 : (int)n;
}
static int aa_sample(Stream *s) {
  (void)s;
  return 48000;
}
static int aa_count(Stream *s) {
  (void)s;
  return 2;
}
static int aa_fmt(Stream *s) {
  (void)s;
  return 1;
}
static int aa_burst(Stream *s) {
  (void)s;
  return -890; /* No measured hardware burst is exposed by this backend. */
}

AsActivity *as_activity_create(const char *package, uint64_t object) {
  if (active_activity)
    return NULL;
  AsActivity *a = calloc(1, sizeof(*a));
  if (!a)
    return NULL;
  a->callbacks = calloc(1, sizeof(*a->callbacks));
  if (!a->callbacks) {
    free(a);
    return NULL;
  }
  char path[256];
  snprintf(path, sizeof(path), "/data/user/0/%s/files", package);
  a->internal_path = strdup(path);
  snprintf(path, sizeof(path), "/sdcard/Android/data/%s/files", package);
  a->external_path = strdup(path);
  snprintf(path, sizeof(path), "/sdcard/Android/obb/%s", package);
  a->obb_path = strdup(path);
  a->sdk = 26;
  a->vm = (void *)as_jni_java_vm();
  a->env = (void *)as_jni_environment();
  a->clazz = object;
  a->assets = &assets_token;
  owner = as_native_session();
  active_activity = a;
  return a;
}
static int activity_callback(AsActivity *a, unsigned slot, uint64_t arg) {
  if (!a->callbacks->callbacks[slot])
    return 0;
  AsNativeCall c = {0};
  c.function = a->callbacks->callbacks[slot];
  c.integer[0] = (uint64_t)a;
  c.integer[1] = arg;
  return as_native_call(&c);
}
int as_activity_start(AsActivity *a) {
  int rc;
  if ((rc = activity_callback(a, 0, 0)) || (rc = activity_callback(a, 1, 0)) ||
      (rc = activity_callback(a, 11, (uint64_t)&queue_token)) ||
      (rc = activity_callback(a, 7, (uint64_t)&window_token)) ||
      (rc = activity_callback(a, 6, 1)))
    return failure(rc);
  return 0;
}
int as_activity_stop(AsActivity *a) {
  const struct {
    unsigned slot;
    uint64_t argument;
    const char *phase;
  } callbacks[] = {{6, 0, "activity_focus_lost"},
                   {3, 0, "activity_pause"},
                   {10, (uint64_t)&window_token, "activity_window_destroy"},
                   {12, (uint64_t)&queue_token, "activity_input_destroy"},
                   {4, 0, "activity_stop"},
                   {5, 0, "activity_destroy"}};
  for (unsigned i = 0; i < sizeof(callbacks) / sizeof(callbacks[0]); i++) {
    as_teardown_phase(callbacks[i].phase);
    int rc = activity_callback(a, callbacks[i].slot, callbacks[i].argument);
    if (rc)
      return failure(rc);
  }
  return 0;
}
void as_activity_dispose(AsActivity *a) {
  if (!a)
    return;
  free((void *)a->internal_path);
  free((void *)a->external_path);
  free((void *)a->obb_path);
  free(a->callbacks);
  free(a);
  active_activity = NULL;
}
static void activity_finish(AsActivity *a) {
  if (a == active_activity)
    finish_requested = 1;
  else
    failure(-2040);
}
static int guest_clock(int id, struct timespec *out) {
  if (id != 0 && id != 1) {
    errno = EINVAL;
    return -1;
  }
  uint64_t ns = armTicksToNs(armGetSystemTick());
  if (id == 0)
    ns = (uint64_t)time(NULL) * 1000000000;
  out->tv_sec = ns / 1000000000;
  out->tv_nsec = ns % 1000000000;
  return 0;
}
static int guest_nanosleep(const struct timespec *t,
                           struct timespec *remaining) {
  if (!t || t->tv_sec < 0 || t->tv_nsec < 0 || t->tv_nsec >= 1000000000) {
    errno = EINVAL;
    return -1;
  }
  svcSleepThread((int64_t)t->tv_sec * 1000000000 + t->tv_nsec);
  if (remaining)
    memset(remaining, 0, sizeof(*remaining));
  return 0;
}
static int guest_usleep(unsigned micros) {
  svcSleepThread((int64_t)micros * 1000);
  return 0;
}
int as_media_reset(void) {
  as_teardown_phase("media_reset");
  extern void as_sl_reset(void);
  as_sl_reset();
  if (current_bindings || window_refs || looper_refs > 1)
    return -EBUSY;
  if (audio.id && as_audio_close(audio.id))
    return -EIO;
  while (streams) {
    Stream *s = streams;
    streams = s->next;
    free(s);
  }
  while (builders) {
    Builder *b = builders;
    builders = b->next;
    free(b);
  }
  for (unsigned i = 0; i < 16; i++) {
    if (contexts[i])
      eglDestroyContext(display, contexts[i]);
    if (surfaces[i])
      eglDestroySurface(display, surfaces[i]);
  }
  if (display)
    eglTerminate(display);
  display = 0;
  memset(contexts, 0, sizeof(contexts));
  memset(surfaces, 0, sizeof(surfaces));
  while (assets)
    asset_close(assets);
  memset(&looper, 0, sizeof(looper));
  looper_owner = 0;
  looper_refs = 0;
  input_read = input_write = 0;
  input_attached = input_inflight = 0;
  owner = NULL;
  media_failure = finish_requested = 0;
  submitted_frames = played_frames = 0;
  diagnostic_capture = 0;
  as_teardown_phase("media_released");
  return 0;
}
int as_media_library(const char *name) {
  return !strcmp(name, "libEGL.so") || !strcmp(name, "libGLESv2.so") ||
         !strcmp(name, "libaaudio.so") || !strcmp(name, "libOpenSLES.so");
}
int as_media_quiesce(void) {
  extern int as_sl_quiesce(void);
  return as_sl_quiesce();
}
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
static uint64_t media_lookup(const char *name) {
  API("AAssetManager_open", asset_open);
  API("AAssetManager_fromJava", asset_from_java);
  API("AAsset_read", asset_read);
  API("AAsset_seek", asset_seek);
  API("AAsset_seek64", asset_seek);
  API("AAsset_getLength", asset_length);
  API("AAsset_getLength64", asset_length);
  API("AAsset_getRemainingLength", asset_remaining);
  API("AAsset_getRemainingLength64", asset_remaining);
  API("AAsset_close", asset_close);
  API("AAsset_getBuffer", asset_buffer);
  API("ALooper_prepare", looper_prepare);
  API("ALooper_forThread", looper_for_thread);
  API("ALooper_acquire", looper_acquire);
  API("ALooper_release", looper_release);
  API("ALooper_wake", looper_wake);
  API("ALooper_pollOnce", looper_poll);
  API("AInputQueue_attachLooper", queue_attach);
  API("AInputQueue_detachLooper", queue_detach);
  API("AInputQueue_hasEvents", queue_has);
  API("AInputQueue_getEvent", queue_get);
  API("AInputQueue_preDispatchEvent", queue_predispatch);
  API("AInputQueue_finishEvent", queue_finish);
  API("AInputEvent_getType", event_type);
  API("AInputEvent_getDeviceId", event_device);
  API("AInputEvent_getSource", event_source);
  API("AKeyEvent_getAction", event_action);
  API("AKeyEvent_getKeyCode", key_code);
  API("AKeyEvent_getEventTime", event_time);
  API("AMotionEvent_getAction", event_action);
  API("AMotionEvent_getPointerCount", motion_count);
  API("AMotionEvent_getX", motion_x);
  API("AMotionEvent_getY", motion_y);
  API("AMotionEvent_getEventTime", event_time);
  API("ANativeWindow_getWidth", window_width);
  API("ANativeWindow_getHeight", window_height);
  API("ANativeWindow_getFormat", window_format);
  API("ANativeWindow_acquire", window_acquire);
  API("ANativeWindow_release", window_release);
  API("ANativeWindow_setBuffersGeometry", window_geometry);
  API("ANativeActivity_finish", activity_finish);
  API("clock_gettime", guest_clock);
  API("nanosleep", guest_nanosleep);
  API("usleep", guest_usleep);
  API("eglGetDisplay", guest_display);
  API("eglCreateWindowSurface", guest_surface);
  API("eglCreateContext", guest_context);
  API("eglMakeCurrent", guest_make_current);
  API("eglDestroySurface", guest_destroy_surface);
  API("eglDestroyContext", guest_destroy_context);
  API("eglSwapBuffers", guest_swap);
  API("eglTerminate", guest_terminate);
  API("eglGetProcAddress", guest_proc);
  API("eglGetError", eglGetError);
  API("eglInitialize", eglInitialize);
  API("eglChooseConfig", eglChooseConfig);
  API("eglGetConfigs", eglGetConfigs);
  API("eglGetConfigAttrib", eglGetConfigAttrib);
  API("eglQueryString", eglQueryString);
  API("eglBindAPI", eglBindAPI);
  API("eglQueryAPI", eglQueryAPI);
  API("eglSwapInterval", eglSwapInterval);
  API("eglQuerySurface", eglQuerySurface);
  API("eglGetCurrentDisplay", eglGetCurrentDisplay);
  API("eglGetCurrentSurface", eglGetCurrentSurface);
  API("eglGetCurrentContext", eglGetCurrentContext);
  API("eglWaitClient", eglWaitClient);
  API("eglWaitGL", eglWaitGL);
  API("AAudio_createStreamBuilder", aa_create);
  API("AAudioStreamBuilder_delete", aa_delete);
  API("AAudioStreamBuilder_setSampleRate", aa_rate);
  API("AAudioStreamBuilder_setChannelCount", aa_channels);
  API("AAudioStreamBuilder_setFormat", aa_format);
  API("AAudioStreamBuilder_setDirection", aa_direction);
  API("AAudioStreamBuilder_setSharingMode", aa_sharing);
  API("AAudioStreamBuilder_setPerformanceMode", aa_performance);
  API("AAudioStreamBuilder_setDataCallback", aa_callback);
  API("AAudioStreamBuilder_openStream", aa_open);
  API("AAudioStream_requestStart", aa_start);
  API("AAudioStream_requestStop", aa_stop);
  API("AAudioStream_close", aa_close);
  API("AAudioStream_write", aa_write);
  API("AAudioStream_getSampleRate", aa_sample);
  API("AAudioStream_getChannelCount", aa_count);
  API("AAudioStream_getFormat", aa_fmt);
  API("AAudioStream_getFramesPerBurst", aa_burst);
  API("glReadPixels", guest_read_pixels);
#include "gles2_imports.inc"
  return 0;
}
uint64_t as_media_import(const char *name) {
  extern uint64_t as_sl_import(const char *);
  uint64_t address = media_lookup(name);
  return address ? address : as_sl_import(name);
}
