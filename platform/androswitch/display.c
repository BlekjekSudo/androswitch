#include "display.h"
#include "display_geometry.h"
#include <errno.h>
#include <switch.h>

static Mutex display_mutex;
static int rotation, presentation; /* 0: none, 1: CPU, 2: EGL */
static unsigned transform(int r) {
  static const unsigned transforms[] = {0, NATIVE_WINDOW_TRANSFORM_ROT_90,
                                        NATIVE_WINDOW_TRANSFORM_ROT_180,
                                        NATIVE_WINDOW_TRANSFORM_ROT_270};
  return transforms[r];
}
/* No configured buffers may change shape. nwindowSetDimensions clears crop;
 * every accepted geometry here is the complete buffer without a crop. */
static int configure(unsigned width, unsigned height, unsigned mode) {
  NWindow *window = nwindowGetDefault();
  if (window->slots_configured)
    return window->width == width && window->height == height &&
                   window->transform == mode
               ? 0
               : -EBUSY;
  unsigned old_width = window->width, old_height = window->height;
  unsigned old_transform = window->transform;
  if (R_FAILED(nwindowSetDimensions(window, width, height)))
    return -EIO;
  if (R_FAILED(nwindowSetTransform(window, mode))) {
    /* No buffers are registered, so restoring scalar window geometry cannot
     * race presentation. A failed request never commits application metadata.
     */
    nwindowSetDimensions(window, old_width, old_height);
    nwindowSetTransform(window, old_transform);
    return -EIO;
  }
  return 0;
}
int as_display_rotation(void) {
  return __atomic_load_n(&rotation, __ATOMIC_ACQUIRE);
}
int as_display_width(void) {
  return (int)as_display_buffer_width(as_display_rotation());
}
int as_display_height(void) {
  return (int)as_display_buffer_height(as_display_rotation());
}
int as_display_set_rotation(int requested) {
  if (!as_display_valid_rotation(requested))
    return -EINVAL;
  mutexLock(&display_mutex);
  int error = 0;
  if (requested != rotation) {
    if (presentation == 1) {
      // Software presentation always retains the physical 1280x720 buffer.
      // Logical frames and input rotate through the existing shared mapping.
      __atomic_store_n(&rotation, requested, __ATOMIC_RELEASE);
    } else if (presentation)
      error = -ENOTSUP;
    else if (nwindowGetDefault()->slots_configured)
      error = -EBUSY;
    else {
      error =
          configure(as_display_buffer_width(requested),
                    as_display_buffer_height(requested), transform(requested));
      if (!error)
        __atomic_store_n(&rotation, requested, __ATOMIC_RELEASE);
    }
  }
  mutexUnlock(&display_mutex);
  return error;
}
int as_display_begin_gpu(void) {
  mutexLock(&display_mutex);
  int error = presentation == 1 ? -EBUSY
                                : configure(as_display_buffer_width(rotation),
                                            as_display_buffer_height(rotation),
                                            transform(rotation));
  if (!error)
    presentation = 2;
  mutexUnlock(&display_mutex);
  return error;
}
int as_display_begin_cpu(int application) {
  mutexLock(&display_mutex);
  int error = presentation == 2 || (!application && rotation)
                  ? -EBUSY
                  : configure(AS_PHYSICAL_WIDTH, AS_PHYSICAL_HEIGHT, 0);
  if (!error && application)
    presentation = 1;
  mutexUnlock(&display_mutex);
  return error;
}
int as_display_reset(void) {
  mutexLock(&display_mutex);
  /* Even identical geometry is not proof that a renderer released ownership. */
  int error = nwindowGetDefault()->slots_configured
                  ? -EBUSY
                  : configure(AS_PHYSICAL_WIDTH, AS_PHYSICAL_HEIGHT, 0);
  if (!error) {
    presentation = 0;
    __atomic_store_n(&rotation, 0, __ATOMIC_RELEASE);
  }
  mutexUnlock(&display_mutex);
  return error;
}
