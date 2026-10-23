#include <stdint.h>
static volatile int initialized;
__attribute__((constructor)) static void initialize(void) { initialized = 73; }
int64_t dependency_value(int64_t value) { return value * 7 + initialized; }
