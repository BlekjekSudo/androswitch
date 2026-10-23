#include <stdint.h>
#include <stdlib.h>
#include <string.h>
extern int64_t dependency_value(int64_t value);
static volatile int initialized;
__attribute__((constructor)) static void initialize(void) { initialized = 19; }
int64_t mixed_integer(int32_t value, int64_t wide) {
  return dependency_value(wide) + value + initialized;
}
double mixed_float(int32_t a, double b, float c, int64_t d) {
  return (double)a + b * (double)c + (double)d;
}
int32_t allocation(int32_t length) {
  if (length < 1 || length > 4096)
    return -1;
  char *buffer = malloc((size_t)length + 1);
  if (!buffer)
    return -2;
  memset(buffer, 'r', (size_t)length);
  buffer[length] = 0;
  int32_t result = (int32_t)strlen(buffer);
  free(buffer);
  return result;
}
