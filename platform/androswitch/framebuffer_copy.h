/* Tegra block-linear copy adapted from libnx v4.12.0 framebuffer.c by fincs.
 * Copyright libnx Authors. ISC license: third_party/libnx-framebuffer.LICENSE.
 * Keep the same 16-byte sector order as libnx; queue errors are handled by our
 * caller rather than framebufferEnd's fatal abort. Input rows are aligned to
 * 8 pixels, stride to 64 bytes, output height to 128 pixels (block height 16).
 */
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <string.h>
static inline void as_framebuffer_copy(uint8_t *output, const uint8_t *input,
                                       size_t stride, size_t height) {
  for (size_t block_y = 0; block_y < (height + 127) / 128; block_y++) {
    for (size_t block_x = 0; block_x < stride / 64; block_x++) {
      for (size_t gob_y = 0; gob_y < 16; gob_y++, output += 512) {
        size_t y = block_y * 128 + gob_y * 8;
        if (y >= height)
          continue;
        const uint8_t *gob = input + y * stride + block_x * 64;
        for (size_t sector = 0; sector < 32; sector++) {
          size_t row = ((sector >> 1) & 6) | (sector & 1);
          size_t column = ((sector << 3) & 16) | ((sector << 1) & 32);
          memcpy(output + sector * 16, gob + row * stride + column, 16);
        }
      }
    }
  }
}
