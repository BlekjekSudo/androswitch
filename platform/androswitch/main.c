#include "display.h"
#include "display_geometry.h"
#include "framebuffer_copy.h"
#include "glsurface.h"
#include "media.h"
#include "memory.h"
#include "ppm_publish.h"
#include "rgba_ppm.h"
#include <inttypes.h>
#include <malloc.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
static u32 pinned_map_fd, pinned_fence_fd;
static unsigned display_generation, display_frames;
static bool display_monitor;
static void close_display_services(unsigned *owned) {
  if (*owned & 8)
    nvGpuExit();
  if (*owned & 4)
    nvFenceExit();
  if (*owned & 2)
    nvMapExit();
  if (*owned & 1)
    nvExit();
  *owned = 0;
}
static Result open_display_services(unsigned *owned) {
  /* Both framebufferClose and Nouveau's device destructor release these
   * service guards. Keep a process owner across every renderer handoff. */
  Result rc = nvInitialize();
  if (R_FAILED(rc))
    return rc;
  *owned |= 1;
  rc = nvMapInit();
  if (R_FAILED(rc))
    return rc;
  *owned |= 2;
  rc = nvFenceInit();
  if (R_FAILED(rc))
    return rc;
  *owned |= 4;
  rc = nvGpuInit();
  if (R_FAILED(rc))
    return rc;
  *owned |= 8;
  pinned_map_fd = nvMapGetFd();
  pinned_fence_fd = nvFenceGetFd();
  return 0;
}
static bool display_error(const char *operation, Result rc, int slot) {
  FILE *f = fopen("sdmc:/switch/androswitch/logs/display-error.json", "wb");
  if (f) {
    fprintf(f,
            "{\"status\":\"failed\",\"subsystem\":\"display\","
            "\"operation\":\"%s\",\"result\":\"0x%08x\",\"slot\":%d,"
            "\"generation\":%u,\"completed_frames\":%u,\"map_fd\":%u,"
            "\"fence_fd\":%u}",
            operation, rc, slot, display_generation, display_frames,
            nvMapGetFd(), nvFenceGetFd());
    fclose(f);
  }
  return false;
}
static int tracing_teardown;
static Mutex trace_mutex;
static const char *last_phase;
/* Each phase is closed/flushed independently, before executing the operation.
 * This survives faults that never reach the Rust session report. */
void as_teardown_phase(const char *phase) {
  if (!__atomic_load_n(&tracing_teardown, __ATOMIC_ACQUIRE))
    return;
  mutexLock(&trace_mutex);
  if (last_phase && !strcmp(last_phase, phase)) {
    mutexUnlock(&trace_mutex);
    return;
  }
  last_phase = phase;
  FILE *file = fopen("sdmc:/switch/androswitch/logs/teardown.log", "ab");
  if (file) {
    fprintf(file, "{\"phase\":\"%s\",\"tick\":%" PRIu64 "}\n", phase,
            armGetSystemTick());
    fclose(file);
  }
  mutexUnlock(&trace_mutex);
}
static void begin_teardown(void) {
  FILE *file = fopen("sdmc:/switch/androswitch/logs/teardown.log", "wb");
  if (file)
    fclose(file);
  last_phase = NULL;
  __atomic_store_n(&tracing_teardown, 1, __ATOMIC_RELEASE);
  as_teardown_phase("stop_requested");
}
static void end_teardown(const char *phase) {
  as_teardown_phase(phase);
  __atomic_store_n(&tracing_teardown, 0, __ATOMIC_RELEASE);
}
void *as_host_alloc(size_t alignment, size_t size) {
  if (alignment < sizeof(void *))
    alignment = sizeof(void *);
  return memalign(alignment, size);
}
void as_host_free(void *p) { free(p); }
void as_host_panic(void) {
  fprintf(stderr, "AndroSwitch Rust panic\n");
  abort();
}
uint64_t as_platform_micros(void) {
  return armTicksToNs(armGetSystemTick()) / 1000;
}
extern uint64_t as_runtime_open(void);
extern bool as_runtime_failed(uint64_t);
extern int as_runtime_step(uint64_t);
typedef struct {
  const uint32_t *pixels;
  size_t length;
  uint32_t width, height;
} AsCpuFrame;
extern bool as_runtime_frame(uint64_t, AsCpuFrame *);
extern void as_runtime_presented(uint64_t, uint64_t);
extern bool as_runtime_capture(uint64_t);
extern int as_runtime_input(uint64_t, int, float, float);
extern int as_runtime_close(uint64_t);
extern uint64_t as_runtime_native(uint64_t);
extern uint64_t as_shell_open(void);
extern int as_shell_step(uint64_t);
extern const uint32_t *as_shell_pixels(uint64_t);
extern void as_shell_presented(uint64_t, uint64_t);
extern void as_shell_input(uint64_t, int, int, int);
extern void as_shell_returned(uint64_t, int);
extern void as_shell_close(uint64_t);
extern bool as_shell_capture(uint64_t);
static uint32_t rgba(uint32_t p) {
  return (p & 0xff00ff00) | ((p >> 16) & 255) | ((p & 255) << 16);
}
static bool present(Framebuffer *fb, const AsCpuFrame *frame, int rotation,
                    bool capture, uint64_t *submitted_hash) {
  if (nvMapGetFd() != pinned_map_fd || nvFenceGetFd() != pinned_fence_fd)
    return display_error("service_identity",
                         MAKERESULT(Module_Libnx, LibnxError_BadInput), -1);
  s32 slot = -1;
  NvMultiFence fence = {0};
  Result rc = nwindowDequeueBuffer(fb->win, &slot, &fence);
  if (R_FAILED(rc))
    return display_error("dequeue", rc, slot);
  /* Never let a stale slot from the EGL triple-buffer queue index a smaller
   * CPU allocation. Validate before any write or block-linear conversion. */
  if (slot < 0 || (u32)slot >= fb->num_fbs ||
      !(fb->win->slots_configured & (UINT64_C(1) << slot))) {
    nwindowCancelBuffer(fb->win, slot, &fence);
    return display_error("slot_bounds",
                         MAKERESULT(Module_Libnx, LibnxError_BadInput), slot);
  }
  rc = nvMultiFenceWait(&fence, 2000000);
  if (R_FAILED(rc)) {
    nwindowCancelBuffer(fb->win, slot, &fence);
    return display_error("compositor_fence", rc, slot);
  }
  u32 stride = fb->stride;
  uint8_t *memory = fb->buf_linear;
  if (as_display_blit(memory, (size_t)stride * AS_PHYSICAL_HEIGHT, stride,
                      frame->pixels, frame->length, frame->width, frame->height,
                      rotation)) {
    nwindowCancelBuffer(fb->win, slot, &fence);
    return display_error("frame_geometry",
                         MAKERESULT(Module_Libnx, LibnxError_BadInput), slot);
  }
  uint64_t checksum = UINT64_C(0xcbf29ce484222325);
  for (unsigned y = 0; y < 720; y += 2) {
    uint32_t *row = (void *)(memory + y * stride);
    for (unsigned x = 0; x < 1280; x += 2) {
      uint32_t readback = rgba(row[x]);
      checksum = (checksum ^ readback) * UINT64_C(0x100000001b3);
    }
  }
  void *destination = (uint8_t *)fb->buf + (size_t)slot * fb->fb_size;
  as_framebuffer_copy(destination, fb->buf_linear, fb->stride, fb->win->height);
  armDCacheFlush(destination, fb->fb_size);
  rc = nwindowQueueBuffer(fb->win, slot, NULL);
  if (R_FAILED(rc)) {
    nwindowCancelBuffer(fb->win, slot, NULL);
    return display_error("queue", rc, slot);
  }
  *submitted_hash = checksum;
  if (capture) {
    /* Present first: diagnostic SD I/O must not delay this buffer's queue
     * submission. The linear shadow stays CPU-owned and unchanged until the
     * next present; read it rather than the block-linear buffer just queued. */
    AsRgbaFrame image = {memory, (size_t)stride * AS_PHYSICAL_HEIGHT, stride,
                         AS_PHYSICAL_WIDTH, AS_PHYSICAL_HEIGHT};
    int status = as_ppm_publish("sdmc:/switch/androswitch/logs/last-frame.ppm",
                                image.width, image.height, as_rgba_ppm_row,
                                &image, NULL);
    as_media_stop_event("cpu_capture_publish_result", status, 0);
  }
  if (display_monitor && display_frames < 8) {
    display_frames++;
    FILE *f = fopen("sdmc:/switch/androswitch/logs/display-handoffs.log", "ab");
    if (f) {
      fprintf(f,
              "{\"generation\":%u,\"frame\":%u,\"slot\":%d,"
              "\"buffers\":%u,\"map_fd\":%u,\"fence_fd\":%u,\"result\":0}\n",
              display_generation, display_frames, slot, fb->num_fbs,
              nvMapGetFd(), nvFenceGetFd());
      fclose(f);
    }
  }
  return true;
}
static int open_framebuffer(Framebuffer *fb, int application) {
  if (as_display_begin_cpu(application)) {
    display_error("framebuffer_geometry",
                  MAKERESULT(Module_Libnx, LibnxError_BadInput), -1);
    return -1;
  }
  Result rc = framebufferCreate(fb, nwindowGetDefault(), 1280, 720,
                                PIXEL_FORMAT_RGBA_8888, 3);
  if (R_FAILED(rc)) {
    display_error("framebuffer_create", rc, -1);
    return -1;
  }
  rc = framebufferMakeLinear(fb);
  if (R_FAILED(rc)) {
    display_error("framebuffer_linear", rc, -1);
    framebufferClose(fb);
    return -1;
  }
  return 0;
}
static void teardown_timeout(void *argument) {
  UEvent *done = argument;
  /* Ultimate stop deadline, starting before Rust lifecycle callbacks. A GL
   * worker may spend up to six seconds retiring Mesa/GPU state; actual joins
   * and successful cleanup remain mandatory before launcher reuse. */
  if (R_FAILED(waitSingle(waiterForUEvent(done), 15000000000))) {
    FILE *f =
        fopen("sdmc:/switch/androswitch/logs/teardown-failure.json", "wb");
    if (f) {
      fputs("{\"status\":\"failed\",\"error\":\"NATIVE_STOP_TIMEOUT: reopen "
            "homebrew\"}",
            f);
      fclose(f);
    }
    svcExitProcess();
  }
}
static int close_application(uint64_t app) {
  begin_teardown();
  UEvent done;
  ueventCreate(&done, true);
  Thread watchdog;
  if (R_FAILED(threadCreate(&watchdog, teardown_timeout, &done, NULL, 16384,
                            0x2b, -2))) {
    svcExitProcess();
  }
  if (R_FAILED(threadStart(&watchdog))) {
    threadClose(&watchdog);
    svcExitProcess();
  }
  int result = as_runtime_close(app);
  if (!result) {
    as_teardown_phase("display_geometry_reset");
    result = as_display_reset();
  }
  ueventSignal(&done);
  threadWaitForExit(&watchdog);
  threadClose(&watchdog);
  as_teardown_phase(result ? "stop_failed" : "runtime_closed");
  return result;
}
int main(int argc, char **argv) {
  (void)argc;
  (void)argv;
  as_memory_log("process_start");
  if (!as_memory_runtime_ready()) {
    as_memory_log("application_override_required");
    consoleInit(NULL);
    printf("AndroSwitch: MEMORY_LAUNCH_MODE\n\n"
           "Use application/title override to launch AndroSwitch.\n"
           "The runtime requires an application process and at least 256 MiB "
           "of heap.\n"
           "Current heap capacity: %" PRIu64 " MiB.\n\nPress + to exit.\n",
           as_host_heap_capacity() / UINT64_C(1048576));
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    PadState error_pad;
    padInitializeDefault(&error_pad);
    while (appletMainLoop()) {
      padUpdate(&error_pad);
      if (padGetButtonsDown(&error_pad) & HidNpadButton_Plus)
        break;
      consoleUpdate(NULL);
    }
    consoleExit(NULL);
    return 1;
  }
  FILE *request = fopen("sdmc:/switch/androswitch/runtime/launch.json", "rb");
  bool direct = request != NULL;
  if (request)
    fclose(request);
  uint64_t shell = direct ? 0 : as_shell_open(), app = 0;
  as_memory_log("launcher_open");
  if (!direct && !shell)
    return 1;
  unsigned display_services __attribute__((cleanup(close_display_services))) =
      0;
  Result display_rc = open_display_services(&display_services);
  if (R_FAILED(display_rc)) {
    display_error("initialize", display_rc, -1);
    if (shell)
      as_shell_close(shell);
    return 2;
  }
  AsActivity *native = NULL;
  Framebuffer fb;
  bool framebuffer_open = false, launch = direct;
  if (!direct) {
    if (open_framebuffer(&fb, 0))
      return 2;
    framebuffer_open = true;
  }
  padConfigureInput(1, HidNpadStyleSet_NpadStandard);
  PadState pad;
  padInitializeDefault(&pad);
  hidInitializeTouchScreen();
  bool touching = false;
  float last_x = 0, last_y = 0;
  while (appletMainLoop()) {
    if (!app && !direct && as_shell_step(shell) == 1)
      launch = true;
    if (launch) {
      if (framebuffer_open) {
        framebufferClose(&fb);
        framebuffer_open = false;
      }
      FILE *media_trace =
          fopen("sdmc:/switch/androswitch/logs/media-stop.log", "wb");
      if (media_trace)
        fclose(media_trace);
      as_memory_log("application_open_begin");
      app = as_runtime_open();
      as_memory_log("application_open_end");
      launch = false;
      if (app && as_runtime_failed(app)) {
        if (close_application(app))
          svcExitProcess();
        app = 0;
        as_memory_log("application_start_failed_closed");
      }
      if (!app) {
        if (as_native_session())
          svcExitProcess();
        if (direct)
          break;
        if (as_display_reset())
          svcExitProcess();
        as_shell_returned(shell, 0);
        if (open_framebuffer(&fb, 0))
          break;
        framebuffer_open = true;
        continue;
      }
      native = (void *)as_runtime_native(app);
      if (native) {
        if (as_activity_start(native)) {
          if (close_application(app))
            svcExitProcess();
          app = 0;
          native = NULL;
          if (direct)
            break;
          as_shell_returned(shell, 0);
          if (open_framebuffer(&fb, 0))
            break;
          framebuffer_open = true;
          touching = false;
          continue;
        }
      } else if (!as_glsurface_running()) {
        if (open_framebuffer(&fb, 1)) {
          close_application(app);
          app = 0;
          break;
        }
        framebuffer_open = true;
      }
      touching = false;
    }
    int state = app ? as_runtime_step(app) : 0;
    padUpdate(&pad);
    u64 down = padGetButtonsDown(&pad);
    /* Scripted E2E return requests exercise the same Plus dispatch as HID. */
    if (state == 3)
      down |= HidNpadButton_Plus;
    bool stop = app && (state < 0 || state == 1 || (!direct && state == 2) ||
                        (down & HidNpadButton_Plus));
    if (stop) {
      bool from_egl = native != NULL || as_glsurface_running();
      if (framebuffer_open) {
        framebufferClose(&fb);
        framebuffer_open = false;
      }
      if (close_application(app))
        svcExitProcess();
      app = 0;
      native = NULL;
      as_memory_log("application_closed");
      if (direct)
        break;
      display_monitor = from_egl;
      if (from_egl) {
        display_generation++;
        display_frames = 0;
      }
      as_teardown_phase("launcher_refresh");
      as_shell_returned(shell, 1);
      as_teardown_phase("launcher_framebuffer_open");
      if (open_framebuffer(&fb, 0))
        break;
      framebuffer_open = true;
      touching = false;
      continue;
    }
    if (!app && (down & HidNpadButton_Plus))
      break;
    const struct {
      u64 button;
      int key;
    } keys[] = {{HidNpadButton_A, 23},       {HidNpadButton_B, 4},
                {HidNpadButton_AnyDown, 20}, {HidNpadButton_AnyUp, 19},
                {HidNpadButton_AnyLeft, 21}, {HidNpadButton_AnyRight, 22}};
    for (unsigned i = 0; app && i < sizeof(keys) / sizeof(keys[0]); i++)
      if (down & keys[i].button)
        as_runtime_input(app, 4, (float)keys[i].key, 0);
    if (!app) {
      const struct {
        u64 button;
        int key;
      } menu_keys[] = {{HidNpadButton_AnyUp, 0}, {HidNpadButton_AnyDown, 1},
                       {HidNpadButton_L, 2},     {HidNpadButton_R, 3},
                       {HidNpadButton_A, 4},     {HidNpadButton_B, 5},
                       {HidNpadButton_X, 6},     {HidNpadButton_Y, 7},
                       {HidNpadButton_ZR, 8},    {HidNpadButton_Minus, 9}};
      for (unsigned i = 0; i < sizeof(menu_keys) / sizeof(menu_keys[0]); i++)
        if (down & menu_keys[i].button)
          as_shell_input(shell, 0, menu_keys[i].key, 0);
    }
    HidTouchScreenState touch = {0};
    hidGetTouchScreenStates(&touch, 1);
    if (touch.count) {
      as_display_touch(app ? as_display_rotation() : 0, touch.touches[0].x,
                       touch.touches[0].y, &last_x, &last_y);
      if (app)
        as_runtime_input(app, touching ? 2 : 0, last_x, last_y);
      touching = true;
    } else if (touching) {
      if (app)
        as_runtime_input(app, 1, last_x, last_y);
      else
        as_shell_input(shell, 1, last_x, last_y);
      touching = false;
    }
    uint64_t checksum;
    if (native || (app && as_glsurface_running())) {
      checksum = as_media_frames();
      svcSleepThread(1000000);
    } else {
      AsCpuFrame frame = {app ? NULL : as_shell_pixels(shell), 640 * 360, 640,
                          360};
      if (app && !as_runtime_frame(app, &frame)) {
        display_error("runtime_frame_metadata",
                      MAKERESULT(Module_Libnx, LibnxError_BadInput), -1);
        break;
      }
      if (!present(&fb, &frame, app ? as_display_rotation() : 0,
                   app ? as_runtime_capture(app) : as_shell_capture(shell),
                   &checksum))
        break;
    }
    if (app)
      as_runtime_presented(app, checksum);
    else {
      as_shell_presented(shell, checksum);
      if (__atomic_load_n(&tracing_teardown, __ATOMIC_ACQUIRE)) {
        if (!display_monitor)
          end_teardown("launcher_ready");
        else if (display_frames == 1)
          as_teardown_phase("launcher_ready");
        else if (display_frames == 8)
          end_teardown("launcher_stable");
      }
    }
  }
  if (framebuffer_open)
    framebufferClose(&fb);
  if (app && close_application(app))
    svcExitProcess();
  if (shell)
    as_shell_close(shell);
  as_memory_log("process_exit");
  return 0;
}
