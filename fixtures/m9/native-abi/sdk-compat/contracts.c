/* Exercise production SDK POSIX adapters with scripted platform clock inputs.
 * Host pipe writes are real. These checks do not claim GPU/hardware execution.
 */
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int cpu_error;
uint64_t armGetSystemTick(void) { return 123; }
uint64_t armGetSystemTickFreq(void) { return 100; }
uint64_t armTicksToNs(uint64_t ticks) { return ticks * 10000000; }
int as_cpu_time_query(int thread, uint64_t *ns) {
  *ns = thread ? 700 : 900;
  return cpu_error;
}
int as_sdk_futex(uint32_t *p, int op, uint32_t v, const struct timespec *t,
                 uint32_t mask) {
  (void)p;
  (void)op;
  (void)v;
  (void)t;
  (void)mask;
  errno = ENOSYS;
  return -1;
}
long as_std_process_core_count(void) { return 3; }
long as_mesa_sysconf(int);
int as_mesa_clock_gettime(clockid_t, struct timespec *);
int as_mesa_clock_getres(clockid_t, struct timespec *);
int __wrap_posix_memalign(void **, size_t, size_t);
int __wrap_open64(const char *, int, ...);
long __wrap_syscall(long, ...);
struct SdkIovec {
  void *base;
  size_t length;
};
ssize_t __wrap_writev(int, const struct SdkIovec *, int);
int main(void) {
  struct timespec t;
  assert(as_mesa_sysconf(30)==4096 && as_mesa_sysconf(83)==3 && as_mesa_sysconf(84)==3);
  assert(as_mesa_sysconf(-1)==-1 && errno==EINVAL);
  assert(!as_mesa_clock_gettime(0, &t) && t.tv_sec>1600000000);
  errno = 0;
  assert(as_mesa_clock_getres(-1, &t) == -1 && errno == EINVAL);
  assert(as_mesa_clock_getres(5, &t) == -1 && errno == EINVAL);
  assert(!as_mesa_clock_getres(1, &t) && t.tv_sec == 0 && t.tv_nsec == 10000000);
  assert(!as_mesa_clock_getres(1, NULL));
  assert(!as_mesa_clock_gettime(1, &t) && t.tv_sec == 1 &&
         t.tv_nsec == 230000000);
  assert(!as_mesa_clock_gettime(4, &t) && t.tv_sec == 1 &&
         t.tv_nsec == 230000000);
  assert(!as_mesa_clock_gettime(2, &t) && t.tv_nsec == 900);
  assert(!as_mesa_clock_gettime(3, &t) && t.tv_nsec == 700);
  cpu_error = -ENOSYS;
  assert(as_mesa_clock_gettime(2, &t) == -1 && errno == ENOSYS);
  assert(as_mesa_clock_gettime(-1, &t) == -1 && errno == EINVAL);
  assert(as_mesa_clock_gettime(1, NULL) == -1 && errno == EFAULT);
  void *p = (void *)(uintptr_t)1;
  errno = EDOM;
  assert(__wrap_posix_memalign(&p, 3, 16) == EINVAL &&
         p == (void *)(uintptr_t)1 && errno == EDOM);
  assert(!__wrap_posix_memalign(&p, 64, 129) && (uintptr_t)p % 64 == 0 &&
         errno == EDOM);
  free(p);
  assert(__wrap_open64("must-not-be-created", 0) == -1 && errno == ENOSYS);
  assert(__wrap_syscall(99999) == -1 && errno == ENOSYS);
  int fds[2];
  assert(!pipe(fds));
  struct SdkIovec v[] = {{"abc", 3}, {"de", 2}};
  assert(__wrap_writev(fds[1], v, 2) == 5);
  char b[6] = {0};
  assert(read(fds[0], b, 5) == 5 && !strcmp(b, "abcde"));
  assert(!close(fds[0]) && !close(fds[1]));
  assert(__wrap_writev(-1, NULL, -1) == -1 && errno == EINVAL);
  puts(
      "SDK clock, allocation, unsupported syscall and real vector I/O "
      "contracts PASS");
}
