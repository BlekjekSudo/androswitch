/* Real policy/blit/input code against a fake libnx scalar configuration API.
 * Forward pixel placement is checked independently of the inverse mapper.
 * This is not proof of actual compositor/GPU orientation. */
#include "../display.h"
#include "../display_geometry.h"
#include "../rgba_ppm.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>

static NWindow window;
static int dimensions_fail, transform_fail, dimension_calls, transform_calls;
void mutexLock(Mutex *mutex) {
  assert(!mutex->locked);
  mutex->locked = 1;
}
void mutexUnlock(Mutex *mutex) {
  assert(mutex->locked);
  mutex->locked = 0;
}
NWindow *nwindowGetDefault(void) { return &window; }
Result nwindowSetDimensions(NWindow *w, unsigned width, unsigned height) {
  dimension_calls++;
  assert(!w->slots_configured);
  if (dimensions_fail) {
    dimensions_fail--;
    return 1;
  }
  w->width = width;
  w->height = height;
  return 0;
}
Result nwindowSetTransform(NWindow *w, unsigned transform) {
  transform_calls++;
  if (transform_fail) {
    transform_fail--;
    return 1;
  }
  w->transform = transform;
  return 0;
}
static void forward(int rotation, unsigned w, unsigned h, unsigned x,
                    unsigned y, unsigned *px, unsigned *py) {
  if (rotation == 0) {
    *px = x;
    *py = y;
  } else if (rotation == 1) {
    *px = h - 1 - y;
    *py = x;
  } else if (rotation == 2) {
    *px = w - 1 - x;
    *py = h - 1 - y;
  } else {
    *px = y;
    *py = w - 1 - x;
  }
}
static uint32_t color(unsigned x, unsigned y) {
  return 0xff000000u | ((x * 17u + y * 29u) & 255u) << 16 |
         ((x * 7u + y * 3u) & 255u) << 8 | ((x + y * 5u) & 255u);
}
static void check_geometry(int rotation) {
  unsigned w = rotation & 1 ? 360 : 640, h = rotation & 1 ? 640 : 360;
  size_t stride = 1280 * 4 + 64, capacity = stride * 720;
  uint8_t *allocation = malloc(capacity + 128), *saved = malloc(capacity);
  uint8_t *rgb = malloc((size_t)1280 * 720 * 3);
  uint32_t *pixels = malloc((size_t)w * h * sizeof(*pixels));
  assert(allocation && saved && rgb && pixels);
  memset(allocation, 0xa5, capacity + 128);
  uint8_t *output = allocation + 64;
  for (unsigned y = 0; y < h; y++)
    for (unsigned x = 0; x < w; x++)
      pixels[(size_t)y * w + x] = color(x, y);
  assert(!as_display_blit(output, capacity, stride, pixels, (size_t)w * h, w, h,
                          rotation));
  AsRgbaFrame image = {output, capacity, stride, 1280, 720};
  for (unsigned y = 0; y < 720; y++)
    assert(!as_rgba_ppm_row(&image, y, rgb + (size_t)y * 1280 * 3, 1280 * 3));
  for (unsigned y = 0; y < h; y++) {
    for (unsigned x = 0; x < w; x++) {
      unsigned px, py;
      forward(rotation, w, h, x, y, &px, &py);
      uint32_t p = color(x, y), expected = (p & 0xff00ff00u) |
                                           (p & 255u) << 16 | (p >> 16 & 255u);
      for (unsigned dy = 0; dy < 2; dy++) {
        for (unsigned dx = 0; dx < 2; dx++) {
          unsigned tx = px * 2 + dx, ty = py * 2 + dy;
          assert(((uint32_t *)(output + ty * stride))[tx] == expected);
          size_t byte = ((size_t)ty * 1280 + tx) * 3;
          assert(rgb[byte] == (uint8_t)(p >> 16));
          assert(rgb[byte + 1] == (uint8_t)(p >> 8));
          assert(rgb[byte + 2] == (uint8_t)p);
          float touch_x, touch_y;
          as_display_touch(rotation, tx, ty, &touch_x, &touch_y);
          assert((unsigned)touch_x == x && (unsigned)touch_y == y);
          assert(touch_x * 2 < w * 2 && touch_y * 2 < h * 2);
        }
      }
    }
  }
  /* Full-resolution EGL capture uses the exact same physical transform. */
  for (unsigned y = 0; y < h * 2; y++) {
    for (unsigned x = 0; x < w * 2; x++) {
      unsigned px, py, sx, sy;
      forward(rotation, w * 2, h * 2, x, y, &px, &py);
      as_display_source_xy(rotation, px, py, 1, &sx, &sy);
      assert(sx == x && sy == y);
    }
  }
  for (unsigned y = 0; y < 720; y++)
    for (size_t x = 5120; x < stride; x++)
      assert(output[y * stride + x] == 0xa5);
  for (size_t i = 0; i < 64; i++) {
    assert(allocation[i] == 0xa5 && output[capacity + i] == 0xa5);
  }
  memcpy(saved, output, capacity);
  assert(as_display_blit(output, capacity, stride, pixels, (size_t)w * h - 1, w,
                         h, rotation));
  assert(as_display_blit(output, capacity - 1, stride, pixels, (size_t)w * h, w,
                         h, rotation));
  assert(as_display_blit(output, capacity, 5116, pixels, (size_t)w * h, w, h,
                         rotation));
  assert(as_display_blit(output, capacity, stride + 1, pixels, (size_t)w * h, w,
                         h, rotation));
  assert(as_display_blit(output + 1, capacity - 1, stride, pixels,
                         (size_t)w * h, w, h, rotation));
  assert(as_display_blit(output, SIZE_MAX, SIZE_MAX / 720 + 1, pixels,
                         (size_t)w * h, w, h, rotation));
  assert(as_display_blit(output, capacity, stride, pixels, (size_t)w * h, w + 1,
                         h, rotation));
  assert(as_display_blit(output, capacity, stride, pixels, (size_t)w * h, w, h,
                         4));
  assert(as_display_blit(output, capacity, stride, pixels, (size_t)w * h, w, h,
                         -1));
  assert(as_display_blit(NULL, capacity, stride, pixels, (size_t)w * h, w, h,
                         rotation));
  assert(as_display_blit(output, capacity, stride, NULL, (size_t)w * h, w, h,
                         rotation));
  assert(!memcmp(saved, output, capacity));
  free(allocation);
  free(saved);
  free(pixels);
  free(rgb);
}
static void check_row_guards(void) {
  uint8_t source[] = {1, 2, 3, 91, 4,  5,  6,  92, 0xa5, 0xa5, 0xa5, 0xa5,
                      7, 8, 9, 93, 10, 11, 12, 94, 0xa5, 0xa5, 0xa5, 0xa5};
  uint8_t rgb[8];
  AsRgbaFrame good = {source, sizeof(source), 12, 2, 2};
  memset(rgb, 0xa5, sizeof(rgb));
  assert(!as_rgba_ppm_row(&good, 1, rgb, sizeof(rgb)));
  const uint8_t expected[] = {7, 8, 9, 10, 11, 12, 0xa5, 0xa5};
  assert(!memcmp(rgb, expected, sizeof(rgb)));
  memset(rgb, 0xa5, sizeof(rgb));
#define BAD_ROW(expression)                                                    \
  do {                                                                         \
    assert(expression);                                                        \
    for (size_t i = 0; i < sizeof(rgb); i++)                                   \
      assert(rgb[i] == 0xa5);                                                  \
  } while (0)
  BAD_ROW(as_rgba_ppm_row(NULL, 0, rgb, sizeof(rgb)));
  BAD_ROW(as_rgba_ppm_row(&good, 0, NULL, sizeof(rgb)));
  BAD_ROW(as_rgba_ppm_row(&good, 2, rgb, sizeof(rgb)));
  BAD_ROW(as_rgba_ppm_row(&good, 0, rgb, 5));
  AsRgbaFrame bad = good;
  bad.pixels = NULL;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad = good;
  bad.width = 0;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad.width = 1281;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad = good;
  bad.height = 0;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad.height = 1281;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad = good;
  bad.stride = 7;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad.stride = SIZE_MAX / 2 + 1;
  bad.capacity = SIZE_MAX;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
  bad = good;
  bad.capacity--;
  BAD_ROW(as_rgba_ppm_row(&bad, 0, rgb, sizeof(rgb)));
#undef BAD_ROW
}
static void check_policy(void) {
  static const unsigned transforms[] = {0, 4, 3, 7};
  assert(!as_display_reset());
  for (int r = 0; r < 4; r++) {
    assert(!as_display_set_rotation(r));
    assert(as_display_rotation() == r);
    if (r)
      assert(as_display_begin_cpu(0) == -EBUSY);
    assert(as_display_width() == (r & 1 ? 720 : 1280));
    assert(as_display_height() == (r & 1 ? 1280 : 720));
    assert(!as_display_begin_gpu());
    assert(window.width == (unsigned)as_display_width());
    assert(window.height == (unsigned)as_display_height());
    assert(window.transform == transforms[r]);
    window.slots_configured = 7;
    assert(!as_display_set_rotation(r));
    assert(as_display_set_rotation((r + 1) % 4) == -ENOTSUP);
    assert(as_display_rotation() == r && window.transform == transforms[r]);
    assert(as_display_begin_cpu(1) == -EBUSY);
    assert(as_display_reset() == -EBUSY);
    assert(as_display_rotation() == r);
    window.slots_configured = 0;
    /* Surface suspend/resume retains the locked context/orientation. */
    window.width = window.height = 0;
    assert(!as_display_begin_gpu());
    assert(window.width == (unsigned)as_display_width());
    assert(window.transform == transforms[r]);
    assert(!as_display_reset());
    assert(as_display_rotation() == 0 && window.transform == 0);
    assert(window.width == 1280 && window.height == 720);
    assert(!as_display_begin_cpu(0));
  }
  for (int r = 0; r < 4; r++) {
    assert(!as_display_set_rotation(r));
    assert(!as_display_begin_cpu(1));
    assert(window.width == 1280 && window.height == 720 && !window.transform);
    assert(as_display_rotation() == r);
    int calls_before=dimension_calls;
    window.slots_configured=7;
    assert(!as_display_set_rotation((r+1)%4));
    assert(as_display_rotation()==(r+1)%4);
    assert(dimension_calls==calls_before && window.width==1280 && window.height==720 && !window.transform);
    window.slots_configured=0;
    assert(as_display_begin_gpu() == -EBUSY);
    assert(!as_display_reset());
  }
  int before = dimension_calls;
  assert(as_display_set_rotation(-1) == -EINVAL);
  assert(as_display_set_rotation(4) == -EINVAL);
  assert(dimension_calls == before && as_display_rotation() == 0);
  dimensions_fail = 1;
  assert(as_display_set_rotation(1) == -EIO);
  assert(as_display_rotation() == 0 && window.width == 1280 &&
         !window.transform);
  transform_fail = 1;
  assert(as_display_set_rotation(1) == -EIO);
  assert(as_display_rotation() == 0 && window.width == 1280 &&
         window.height == 720 && !window.transform);
  window.slots_configured = 7;
  before = transform_calls;
  assert(as_display_set_rotation(1) == -EBUSY);
  assert(as_display_rotation() == 0 && transform_calls == before);
  window.slots_configured = 0;
  window.width = 720;
  window.height = 1280;
  window.transform = 4;
  window.slots_configured = 7;
  assert(as_display_set_rotation(1) == -EBUSY);
  assert(as_display_rotation() == 0);
  window.slots_configured = 0;
  assert(!as_display_reset());
}
int main(void) {
  for (int r = 0; r < 4; r++)
    check_geometry(r);
  check_row_guards();
  check_policy();
  puts("display rotation: all 4 complete frame/input/capture rotations and "
       "CPU RGB rows, 12 malformed row guards and lifecycle/failure policies "
       "passed");
  return 0;
}
