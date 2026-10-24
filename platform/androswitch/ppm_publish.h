/* Complete diagnostic-image publication. Only a successful close followed by
 * rename exposes a .ppm filename to external evidence readers. */
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

enum {
  AS_PPM_OK = 0,
  AS_PPM_NAME = -1,
  AS_PPM_OPEN = -2,
  AS_PPM_HEADER = -3,
  AS_PPM_ROW = -4,
  AS_PPM_WRITE = -5,
  AS_PPM_CLOSE = -6,
  AS_PPM_RENAME = -7
};
typedef int (*AsPpmRow)(void *context, unsigned y, uint8_t *rgb,
                        size_t capacity);
/* Same contracts as stdio; supplied only to exercise actual error paths in
 * host tests. Device calls use the normal stdio implementation. */
typedef struct {
  FILE *(*open)(const char *, const char *);
  size_t (*write)(const void *, size_t, size_t, FILE *);
  int (*close)(FILE *);
  int (*rename_file)(const char *, const char *);
  int (*remove_file)(const char *);
} AsPpmIo;
static inline int as_ppm_publish(const char *path, unsigned width,
                                 unsigned height, AsPpmRow make_row,
                                 void *context, const AsPpmIo *provided) {
  static const AsPpmIo standard = {fopen, fwrite, fclose, rename, remove};
  const AsPpmIo *io = provided ? provided : &standard;
  char temporary[256], header[64];
  uint8_t row[1280 * 3];
  if (!path || !make_row || !width || !height || width > 1280 || height > 1280)
    return AS_PPM_NAME;
  int n = snprintf(temporary, sizeof(temporary), "%s.tmp", path);
  if (n < 0 || (size_t)n >= sizeof(temporary))
    return AS_PPM_NAME;
  FILE *file = io->open(temporary, "wb");
  if (!file) {
    io->remove_file(temporary); /* also removes a previous interrupted temp */
    return AS_PPM_OPEN;
  }
  n = snprintf(header, sizeof(header), "P6\n%u %u\n255\n", width, height);
  int error = 0;
  if (n <= 0 || (size_t)n >= sizeof(header) ||
      io->write(header, 1, (size_t)n, file) != (size_t)n)
    error = AS_PPM_HEADER;
  for (unsigned y = 0; !error && y < height; y++) {
    if (make_row(context, y, row, sizeof(row)))
      error = AS_PPM_ROW;
    else if (io->write(row, 1, (size_t)width * 3, file) != (size_t)width * 3)
      error = AS_PPM_WRITE;
  }
  int closed = io->close(file);
  if (!error && closed)
    error = AS_PPM_CLOSE;
  if (!error && io->rename_file(temporary, path))
    error = AS_PPM_RENAME;
  if (error)
    io->remove_file(temporary);
  /* No incomplete final file is removed or replaced on a failed attempt.
   * A process crash can leave .tmp debris, which is never a published .ppm. */
  return error;
}
