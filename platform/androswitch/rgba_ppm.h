/* Read the submitted CPU shadow image without touching the queued GPU buffer.
 * The source is physical RGBA bytes, including any framebuffer row padding. */
#pragma once
#include <stddef.h>
#include <stdint.h>

typedef struct {
  const uint8_t *pixels;
  size_t capacity, stride;
  unsigned width, height;
} AsRgbaFrame;

static inline int as_rgba_ppm_row(void *context, unsigned y, uint8_t *rgb,
                                  size_t capacity) {
  const AsRgbaFrame *frame = context;
  /* Check the entire declared frame before reading any pixel. Bound the row
   * dimensions to the same maximum as the diagnostic PPM publisher. */
  if (!frame || !frame->pixels || !rgb || !frame->width || !frame->height ||
      frame->width > 1280 || frame->height > 1280 || y >= frame->height ||
      frame->stride < (size_t)frame->width * 4 ||
      frame->stride > SIZE_MAX / frame->height ||
      frame->capacity < frame->stride * frame->height ||
      capacity < (size_t)frame->width * 3)
    return -1;
  const uint8_t *source = frame->pixels + (size_t)y * frame->stride;
  for (unsigned x = 0; x < frame->width; x++) {
    rgb[(size_t)x * 3] = source[(size_t)x * 4];
    rgb[(size_t)x * 3 + 1] = source[(size_t)x * 4 + 1];
    rgb[(size_t)x * 3 + 2] = source[(size_t)x * 4 + 2];
  }
  return 0;
}
