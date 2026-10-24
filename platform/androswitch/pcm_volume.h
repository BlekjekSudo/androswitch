/* Original PCM gain implementation, MIT OR Apache-2.0. */
#ifndef ANDROSWITCH_PCM_VOLUME_H
#define ANDROSWITCH_PCM_VOLUME_H
#include <math.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
static inline void as_pcm16_volume(void *data, size_t frames, int millibels,
                                   bool mute) {
  if (mute || millibels <= -32768) {
    memset(data, 0, frames * 4);
    return;
  }
  if (millibels >= 0)
    return;
  double gain = pow(10.0, millibels / 2000.0);
  /* Buffer addresses need not have int16_t alignment; copy each scalar. */
  unsigned char *bytes = data;
  for (size_t i = 0; i < frames * 2; i++) {
    int16_t sample;
    memcpy(&sample, bytes + i * 2, 2);
    sample = (int16_t)lround(sample * gain);
    memcpy(bytes + i * 2, &sample, 2);
  }
}
#endif
