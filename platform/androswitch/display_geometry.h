/* Pure geometry shared by real framebuffer, input and capture paths. No GPU
 * or Android state is fabricated here. Rotation 1 is clockwise 90 degrees,
 * matching libnx NATIVE_WINDOW_TRANSFORM_ROT_90. */
#pragma once
#include <stddef.h>
#include <stdint.h>

#define AS_PHYSICAL_WIDTH 1280u
#define AS_PHYSICAL_HEIGHT 720u
static inline int as_display_valid_rotation(int rotation) {
  return rotation >= 0 && rotation <= 3;
}
static inline unsigned as_display_buffer_width(int rotation) {
  return rotation & 1 ? AS_PHYSICAL_HEIGHT : AS_PHYSICAL_WIDTH;
}
static inline unsigned as_display_buffer_height(int rotation) {
  return rotation & 1 ? AS_PHYSICAL_WIDTH : AS_PHYSICAL_HEIGHT;
}
/* Inverse of the compositor rotation, for an output pixel on the fixed layer.
 * scale=1 is a native EGL buffer; scale=2 is the CPU dp-sized raster. */
static inline void as_display_source_xy(int rotation, unsigned x, unsigned y,
                                        unsigned scale, unsigned *sx,
                                        unsigned *sy) {
  unsigned w = AS_PHYSICAL_WIDTH / scale, h = AS_PHYSICAL_HEIGHT / scale;
  x /= scale;
  y /= scale;
  switch (rotation) {
  case 1:
    *sx = y;
    *sy = w - 1 - x;
    break;
  case 2:
    *sx = w - 1 - x;
    *sy = h - 1 - y;
    break;
  case 3:
    *sx = h - 1 - y;
    *sy = x;
    break;
  default:
    *sx = x;
    *sy = y;
    break;
  }
}
/* HID coordinates are physical pixels. Preserve sub-dp precision; the runtime
 * converts these logical coordinates to full-resolution pixels for GL/NDK. */
static inline void as_display_touch(int rotation, unsigned px, unsigned py,
                                    float *x, float *y) {
  if (px >= AS_PHYSICAL_WIDTH)
    px = AS_PHYSICAL_WIDTH - 1;
  if (py >= AS_PHYSICAL_HEIGHT)
    py = AS_PHYSICAL_HEIGHT - 1;
  switch (rotation) {
  case 1:
    *x = py / 2.f;
    *y = (AS_PHYSICAL_WIDTH - 1 - px) / 2.f;
    break;
  case 2:
    *x = (AS_PHYSICAL_WIDTH - 1 - px) / 2.f;
    *y = (AS_PHYSICAL_HEIGHT - 1 - py) / 2.f;
    break;
  case 3:
    *x = (AS_PHYSICAL_HEIGHT - 1 - py) / 2.f;
    *y = px / 2.f;
    break;
  default:
    *x = px / 2.f;
    *y = py / 2.f;
    break;
  }
}
static inline uint32_t as_display_rgba(uint32_t p) {
  return (p & 0xff00ff00) | ((p >> 16) & 255) | ((p & 255) << 16);
}
/* Output is physical RGBA, input is the actual logical ARGB Frame. Reject
 * inconsistent metadata before touching any pixels, including short buffers. */
static inline int as_display_blit(uint8_t *output, size_t capacity,
                                  size_t stride, const uint32_t *pixels,
                                  size_t length, unsigned width,
                                  unsigned height, int rotation) {
  if (!output || !pixels || !as_display_valid_rotation(rotation) ||
      width != as_display_buffer_width(rotation) / 2 ||
      height != as_display_buffer_height(rotation) / 2 ||
      length < (size_t)width * height || ((uintptr_t)output & 3) ||
      (stride & 3) || stride < AS_PHYSICAL_WIDTH * 4 ||
      stride > SIZE_MAX / AS_PHYSICAL_HEIGHT ||
      capacity < stride * AS_PHYSICAL_HEIGHT)
    return -1;
  for (unsigned y = 0; y < AS_PHYSICAL_HEIGHT; y++) {
    uint32_t *row = (void *)(output + y * stride);
    for (unsigned x = 0; x < AS_PHYSICAL_WIDTH; x++) {
      unsigned sx, sy;
      as_display_source_xy(rotation, x, y, 2, &sx, &sy);
      row[x] = as_display_rgba(pixels[(size_t)sy * width + sx]);
    }
  }
  return 0;
}
