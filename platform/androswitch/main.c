#include "media.h"
#include <inttypes.h>
#include <malloc.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <switch.h>
size_t __nx_heap_size = 256 * 1024 * 1024;
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
extern int as_runtime_step(uint64_t);
extern const uint32_t *as_runtime_pixels(uint64_t);
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
static uint64_t present(Framebuffer *fb, const uint32_t *pixels, bool capture) {
  u32 stride;
  uint8_t *memory = framebufferBegin(fb, &stride);
  uint64_t checksum = UINT64_C(0xcbf29ce484222325);
  for (unsigned y = 0; y < 720; y++) {
    uint32_t *row = (void *)(memory + y * stride);
    for (unsigned x = 0; x < 1280; x++) {
      row[x] = rgba(pixels[(y / 2) * 640 + x / 2]);
      if (!(x & 1) && !(y & 1)) {
        uint32_t readback = rgba(row[x]);
        checksum = (checksum ^ readback) * UINT64_C(0x100000001b3);
      }
    }
  }
  if (capture) {
    FILE *file = fopen("sdmc:/switch/androswitch/logs/last-frame.ppm", "wb");
    if (file) {
      fprintf(file, "P6\n1280 720\n255\n");
      for (unsigned y = 0; y < 720; y++) {
        uint32_t *row = (void *)(memory + y * stride);
        for (unsigned x = 0; x < 1280; x++) {
          uint32_t p = row[x];
          uint8_t rgb[3] = {p, p >> 8, p >> 16};
          fwrite(rgb, 1, 3, file);
        }
      }
      fclose(file);
    }
  }
  framebufferEnd(fb);
  return checksum;
}
static int open_framebuffer(Framebuffer *fb) {
  Result rc = framebufferCreate(fb, nwindowGetDefault(), 1280, 720,
                                PIXEL_FORMAT_RGBA_8888, 2);
  if (R_FAILED(rc))
    return -1;
  rc = framebufferMakeLinear(fb);
  if (R_FAILED(rc)) {
    framebufferClose(fb);
    return -1;
  }
  return 0;
}
static void teardown_timeout(void *argument) {
  UEvent *done = argument;
  if (R_FAILED(waitSingle(waiterForUEvent(done), 4000000000))) {
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
  ueventSignal(&done);
  threadWaitForExit(&watchdog);
  threadClose(&watchdog);
  return result;
}
int main(int argc, char **argv) {
  (void)argc;
  (void)argv;
  FILE *request = fopen("sdmc:/switch/androswitch/runtime/launch.json", "rb");
  bool direct = request != NULL;
  if (request)
    fclose(request);
  uint64_t shell = direct ? 0 : as_shell_open(), app = 0;
  if (!direct && !shell)
    return 1;
  AsActivity *native = NULL;
  Framebuffer fb;
  bool framebuffer_open = false, launch = direct;
  if (!direct) {
    if (open_framebuffer(&fb))
      return 2;
    framebuffer_open = true;
  }
  padConfigureInput(1, HidNpadStyleSet_NpadStandard);
  PadState pad;
  padInitializeDefault(&pad);
  hidInitializeTouchScreen();
  bool touching = false;
  int last_x = 0, last_y = 0;
  while (appletMainLoop()) {
    if (!app && !direct && as_shell_step(shell) == 1)
      launch = true;
    if (launch) {
      if (framebuffer_open) {
        framebufferClose(&fb);
        framebuffer_open = false;
      }
      app = as_runtime_open();
      launch = false;
      if (!app) {
        if (as_native_session())
          svcExitProcess();
        if (direct)
          break;
        as_shell_returned(shell, 0);
        if (open_framebuffer(&fb))
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
          if (direct)
            break;
          as_shell_returned(shell, 0);
        }
      } else {
        if (open_framebuffer(&fb)) {
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
    bool stop = app && (state < 0 || state == 1 || (!direct && state == 2) ||
                        (down & HidNpadButton_Plus));
    if (stop) {
      if (framebuffer_open) {
        framebufferClose(&fb);
        framebuffer_open = false;
      }
      if (close_application(app))
        svcExitProcess();
      app = 0;
      native = NULL;
      if (direct)
        break;
      as_shell_returned(shell, 1);
      if (open_framebuffer(&fb))
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
      last_x = touch.touches[0].x / 2;
      last_y = touch.touches[0].y / 2;
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
    if (native) {
      checksum = as_media_frames();
      svcSleepThread(1000000);
    } else
      checksum =
          present(&fb, app ? as_runtime_pixels(app) : as_shell_pixels(shell),
                  app ? as_runtime_capture(app) : as_shell_capture(shell));
    if (app)
      as_runtime_presented(app, checksum);
    else
      as_shell_presented(shell, checksum);
  }
  if (framebuffer_open)
    framebufferClose(&fb);
  if (app && close_application(app))
    svcExitProcess();
  if (shell)
    as_shell_close(shell);
  return 0;
}
