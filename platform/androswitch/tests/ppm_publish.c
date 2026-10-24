/* Production publisher against real host files, with injected I/O failures.
 * Verify final-name visibility and content, not just mock call counts. */
#include "../ppm_publish.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>

enum {
  NORMAL,
  OPEN_FAIL,
  HEADER_SHORT,
  ROW_SHORT,
  ROW_FAIL,
  CLOSE_FAIL,
  RENAME_FAIL
};
static struct {
  const char *path;
  char temporary[256];
  int mode, opens, writes, closes, renames;
  unsigned width, height;
  int previous;
} test;
static void unchanged(void) {
  FILE *file = fopen(test.path, "rb");
  if (!test.previous) {
    assert(!file);
    return;
  }
  assert(file);
  char data[8] = {0};
  assert(fread(data, 1, sizeof(data), file) == 7);
  assert(!memcmp(data, "previous", 7));
  assert(!fclose(file));
}
static FILE *open_file(const char *path, const char *mode) {
  test.opens++;
  assert(!strcmp(path, test.temporary) && !strcmp(mode, "wb"));
  unchanged();
  return test.mode == OPEN_FAIL ? NULL : fopen(path, mode);
}
static size_t write_file(const void *data, size_t size, size_t count,
                         FILE *file) {
  test.writes++;
  unchanged();
  assert(size == 1 && count > 1);
  int partial = (test.mode == HEADER_SHORT && test.writes == 1) ||
                (test.mode == ROW_SHORT && test.writes == 2);
  return fwrite(data, size, partial ? count - 1 : count, file);
}
static int close_file(FILE *file) {
  test.closes++;
  unchanged();
  assert(!fclose(file));
  return test.mode == CLOSE_FAIL ? EOF : 0;
}
static int rename_file(const char *source, const char *destination) {
  test.renames++;
  assert(test.closes == 1);
  assert(test.writes == (int)test.height + 1);
  assert(!strcmp(source, test.temporary) && !strcmp(destination, test.path));
  unchanged();
  return test.mode == RENAME_FAIL ? -1 : rename(source, destination);
}
static const AsPpmIo injected = {open_file, write_file, close_file, rename_file,
                                 remove};
static int row(void *context, unsigned y, uint8_t *rgb, size_t capacity) {
  assert(context == &test && y < test.height);
  assert(capacity >= (size_t)test.width * 3 && capacity == 3840);
  if (test.mode == ROW_FAIL && y == 1)
    return -1;
  for (unsigned x = 0; x < test.width * 3; x++)
    rgb[x] = (uint8_t)(x * 7 + y * 17);
  return 0;
}
static void reset(const char *path, int mode, int previous, unsigned width,
                  unsigned height) {
  memset(&test, 0, sizeof(test));
  test.path = path;
  test.mode = mode;
  test.previous = previous;
  test.width = width;
  test.height = height;
  assert(snprintf(test.temporary, sizeof(test.temporary), "%s.tmp", path) > 0);
  remove(path);
  remove(test.temporary);
  if (previous) {
    FILE *file = fopen(path, "wb");
    assert(file);
    assert(fwrite("previous", 1, 7, file) == 7 && !fclose(file));
  }
}
static void verify_complete(void) {
  FILE *file = fopen(test.path, "rb");
  assert(file);
  char expected[64], actual[64];
  int n = snprintf(expected, sizeof(expected), "P6\n%u %u\n255\n", test.width,
                   test.height);
  assert(n > 0 && fread(actual, 1, (size_t)n, file) == (size_t)n &&
         !memcmp(actual, expected, (size_t)n));
  uint8_t bytes[3840];
  for (unsigned y = 0; y < test.height; y++) {
    assert(fread(bytes, 1, (size_t)test.width * 3, file) ==
           (size_t)test.width * 3);
    for (unsigned x = 0; x < test.width * 3; x++)
      assert(bytes[x] == (uint8_t)(x * 7 + y * 17));
  }
  assert(fgetc(file) == EOF && !ferror(file));
  assert(!fclose(file));
  assert(!fopen(test.temporary, "rb"));
}
int main(int argc, char **argv) {
  assert(argc == 2);
  const char *path = argv[1];
  assert(strlen(path) + 4 < sizeof(test.temporary));
  reset(path, NORMAL, 0, 5, 3);
  assert(as_ppm_publish(path, 5, 3, row, &test, NULL) == AS_PPM_OK);
  verify_complete();
  reset(path, NORMAL, 0, 1280, 720);
  assert(as_ppm_publish(path, 1280, 720, row, &test, &injected) == AS_PPM_OK);
  assert(test.writes == 721 && test.closes == 1 && test.renames == 1);
  verify_complete();
  const int modes[] = {OPEN_FAIL, HEADER_SHORT, ROW_SHORT,
                       ROW_FAIL,  CLOSE_FAIL,   RENAME_FAIL};
  const int errors[] = {AS_PPM_OPEN, AS_PPM_HEADER, AS_PPM_WRITE,
                        AS_PPM_ROW,  AS_PPM_CLOSE,  AS_PPM_RENAME};
  for (unsigned i = 0; i < sizeof(modes) / sizeof(*modes); i++) {
    reset(path, modes[i], 1, 5, 3);
    if (modes[i] == OPEN_FAIL) {
      FILE *stale = fopen(test.temporary, "wb");
      assert(stale);
      assert(fwrite("stale", 1, 5, stale) == 5 && !fclose(stale));
    }
    assert(as_ppm_publish(path, 5, 3, row, &test, &injected) == errors[i]);
    unchanged();
    assert(!fopen(test.temporary, "rb"));
    if (modes[i] != OPEN_FAIL)
      assert(test.closes == 1);
    if (modes[i] != RENAME_FAIL)
      assert(!test.renames);
  }
  reset(path, NORMAL, 0, 5, 3);
  char too_long[300];
  memset(too_long, 'x', sizeof(too_long));
  too_long[sizeof(too_long) - 1] = 0;
  assert(as_ppm_publish(path, 1281, 3, row, &test, &injected) == AS_PPM_NAME);
  assert(as_ppm_publish(too_long, 5, 3, row, &test, &injected) == AS_PPM_NAME);
  assert(as_ppm_publish(NULL, 5, 3, row, &test, &injected) == AS_PPM_NAME);
  assert(as_ppm_publish(path, 5, 3, NULL, &test, &injected) == AS_PPM_NAME);
  assert(!test.opens);
  remove(path);
  remove(test.temporary);
  puts("PPM publication: 12 complete/atomic/error/guard cases passed;720p "
       "uses721 writes");
  return 0;
}
