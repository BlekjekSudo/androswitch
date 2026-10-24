#include "../pcm_volume.h"
#include <assert.h>
#include <stdlib.h>
int main(void) {
  int16_t unity[] = {-32768, 32767, -1000, 1000};
  as_pcm16_volume(unity, 2, 0, false);
  assert(unity[0] == -32768 && unity[1] == 32767);
  as_pcm16_volume(unity, 2, -600, false);
  assert(abs(unity[0] + 16423) <= 1 && abs(unity[1] - 16422) <= 1);
  assert(unity[2] == -501 && unity[3] == 501);
  as_pcm16_volume(unity, 2, 0, true);
  assert(unity[0] == 0 && unity[3] == 0);
  unsigned char unaligned[] = {99, 0xff, 0x7f, 0, 0x80, 88};
  as_pcm16_volume(unaligned + 1, 1, -32768, false);
  assert(unaligned[0] == 99 && unaligned[5] == 88);
  for (unsigned i = 1; i < 5; i++)
    assert(unaligned[i] == 0);
  as_pcm16_volume(NULL, 0, 0, false);
  return 0;
}
