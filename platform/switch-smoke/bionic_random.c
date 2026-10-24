/* API26 Bionic rand/srand share the BSD random/srandom TYPE_3 stream.
 * The 31-word additive generator has separation3 and 310 warm-up draws;
 * seeding uses signed 32-bit Park-Miller values. Seed0 remains distinct from1.
 * rand_r is the separate modulo-2^32 LCG. No host PRNG state is accessed.
 * Original implementation, MIT OR Apache-2.0, using algorithm contracts in
 * AOSP android-8.0.0_r1 libc/bionic/rand.cpp and upstream-netbsd
 * {common/lib/libc/stdlib/random.c,lib/libc/stdlib/rand_r.c}, pinned commit
 * f711b078b9656f7dbb52a046a7f992a450e8fa13. No upstream code/table copied.
 * This generator is deterministic and is not a cryptographic RNG.
 */
#include "bionic_random.h"
#include "bionic_cxx.h"
#include <errno.h>
#include <string.h>
_Static_assert(sizeof(unsigned) == 4 && sizeof(int) == 4 && sizeof(long) == 8,
               "Android LP64 PRNG ABI");
static uint32_t words[31];
static unsigned rear;
static int initialized;
static uint32_t next_unlocked(void) {
  unsigned front = rear + 3;
  if (front >= 31)
    front -= 31;
  words[front] += words[rear];
  uint32_t result = words[front] >> 1;
  if (++rear == 31)
    rear = 0;
  return result;
}
static void seed_unlocked(uint32_t seed) {
  words[0] = seed;
  int64_t previous =
      seed <= INT32_MAX ? (int64_t)seed : (int64_t)seed - 0x100000000LL;
  for (unsigned i = 1; i < 31; ++i) {
    int64_t value = 16807 * (previous % 127773) - 2836 * (previous / 127773);
    if (value <= 0)
      value += INT32_MAX;
    words[i] = (uint32_t)value;
    previous = value;
  }
  rear = 0;
  for (unsigned i = 0; i < 310; ++i)
    (void)next_unlocked();
  initialized = 1;
}
static int guest_rand(void) {
  int saved = errno;
  as_cxx_lock();
  if (!initialized)
    seed_unlocked(1);
  int result = (int)next_unlocked();
  as_cxx_unlock();
  errno = saved;
  return result;
}
static long guest_random(void) { return guest_rand(); }
static void guest_srand(unsigned seed) {
  int saved = errno;
  as_cxx_lock();
  seed_unlocked(seed);
  as_cxx_unlock();
  errno = saved;
}
static void guest_srandom(unsigned seed) { guest_srand(seed); }
static int guest_rand_r(unsigned *seed) {
  /* seed is caller-owned, non-NULL, and externally synchronized if shared. */
  uint32_t value = (uint32_t)*seed * UINT32_C(1103515245) + UINT32_C(12345);
  *seed = value;
  return (int)(value & INT32_MAX);
}
void as_bionic_random_reset(void) {
  int saved = errno;
  as_cxx_lock();
  memset(words, 0, sizeof(words));
  rear = 0;
  initialized = 0;
  as_cxx_unlock();
  errno = saved;
}
#define API(name)                                                              \
  if (!strcmp(name_string, #name))                                             \
  return (uint64_t)guest_##name
uint64_t as_bionic_random_import(const char *name_string) {
  API(rand);
  API(srand);
  API(rand_r);
  API(random);
  API(srandom);
  return 0;
}
