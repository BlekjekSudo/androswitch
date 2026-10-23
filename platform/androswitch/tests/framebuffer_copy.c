/* Independent byte-address checks for the hardware layout, including guards
 * around a complete 720p allocation. No Horizon or GPU emulation is involved.
 */
#include "../framebuffer_copy.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
static void check(size_t stride, size_t height) {
  size_t input_size = stride * ((height + 7) & ~(size_t)7);
  size_t output_size = stride * ((height + 127) & ~(size_t)127);
  uint8_t *input = malloc(input_size);
  uint8_t *allocation = malloc(output_size + 128);
  assert(input && allocation);
  memset(allocation, 0xe3, output_size + 128);
  for (size_t i = 0; i < input_size; i++)
    input[i] = (uint8_t)((i * 13 + i / stride * 17) % 251);
  uint8_t *output = allocation + 64;
  as_framebuffer_copy(output, input, stride, height);
  for (size_t y = 0; y < height; y++) {
    for (size_t x = 0; x < stride; x++) {
      /* Byte address from Tegra GOB/block coordinates, independent of the
       * copy's destination-sector traversal. */
      size_t address = (y / 128 * (stride / 64) + x / 64) * 8192;
      address += (y % 128 / 8) * 512 + (x % 64 / 32) * 256;
      address += (y % 8 / 2) * 64 + (x % 32 / 16) * 32;
      address += (y % 2) * 16 + x % 16;
      assert(address < output_size);
      assert(output[address] == input[y * stride + x]);
    }
  }
  for (size_t i = 0; i < 64; i++) {
    assert(allocation[i] == 0xe3);
    assert(allocation[output_size + 64 + i] == 0xe3);
  }
  free(input);
  free(allocation);
}
int main(void) {
  check(64, 8);
  check(128, 128);
  check(256, 136);
  check(5120, 720);
  puts("framebuffer copy: 4 address/guard cases passed");
}
