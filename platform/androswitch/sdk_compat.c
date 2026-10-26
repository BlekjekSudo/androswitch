/* Bounded native Rust SDK ABI adapters, MIT OR Apache-2.0.
 * AndroSwitch's Horizon std uses newlib. The optional Mesa shader compiler's
 * separate GNU/Linux Rust libraries use these bounded host ABI adapters.
 */
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <malloc.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <switch.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#include "bionic_auxv.h"
#include "cpu_time.h"
#include "sdk_futex.h"
#include "../horizon-std/support.h"
/* These names are imported only by the SDK's GNU/Linux Rust objects.
 * CLOCK_REALTIME is 0 there but 1 in newlib. Never wrap the process-global
 * clock_gettime: that would turn Horizon std's wall clock into uptime. */
int as_mesa_clock_gettime(clockid_t clock, struct timespec *out) {
  if (!out) {
    errno = EFAULT;
    return -1;
  }
  if (clock == 0) return clock_gettime(CLOCK_REALTIME, out);
  uint64_t ns;
  if (clock == 1 || clock == 4)
    ns = armTicksToNs(armGetSystemTick());
  else if (clock == 2 || clock == 3) {
    int rc = as_cpu_time_query(clock == 3, &ns);
    if (rc) {
      errno = -rc;
      return -1;
    }
  } else {
    errno = EINVAL;
    return -1;
  }
  out->tv_sec = ns / 1000000000;
  out->tv_nsec = ns % 1000000000;
  return 0;
}
uint64_t as_sdk_now_ns(int realtime) {
  if (!realtime) return armTicksToNs(armGetSystemTick());
  struct timespec t;
  if (clock_gettime(CLOCK_REALTIME, &t) || t.tv_sec < 0) return UINT64_MAX;
  return (uint64_t)t.tv_sec * 1000000000 + (uint64_t)t.tv_nsec;
}
int as_mesa_clock_getres(clockid_t clock, struct timespec *out) {
  if ((uint64_t)clock > 4) {
    errno = EINVAL;
    return -1;
  }
  uint64_t freq = armGetSystemTickFreq();
  if (!freq) {
    errno = ENOSYS;
    return -1;
  }
  uint64_t ns = (1000000000 + freq - 1) / freq;
  if (out) {
    out->tv_sec = ns / 1000000000;
    out->tv_nsec = ns % 1000000000;
  }
  return 0;
}
long as_mesa_sysconf(int name) {
  /* GNU/Linux constants from the compiler-matched libc ABI, not newlib's. */
  if(name==30) return 0x1000;
  if(name==83 || name==84) return as_std_process_core_count();
  errno=EINVAL;return -1;
}
long __wrap_syscall(long number, ...) {
  if (number != 98) {
    errno = ENOSYS;
    return -1;
  }
  va_list args;
  va_start(args, number);
  uint32_t *address = va_arg(args, uint32_t *);
  int op = va_arg(args, int);
  uint32_t value = va_arg(args, uint32_t);
  const struct timespec *timeout = NULL;
  uint32_t mask = UINT32_MAX;
  if ((op & 127) == 0 || (op & 127) == 9)
    timeout = va_arg(args, const struct timespec *);
  if ((op & 127) == 9 || (op & 127) == 10) {
    if ((op & 127) == 10) (void)va_arg(args, void *);
    (void)va_arg(args, void *);
    mask = va_arg(args, uint32_t);
  }
  va_end(args);
  return as_sdk_futex(address, op, value, timeout, mask);
}
unsigned long __wrap_getauxval(unsigned long kind) {
  unsigned long value = 0;
  if (as_bionic_auxv(kind, &value)) return value;
  errno = ENOENT;
  return 0;
}
int __wrap_posix_memalign(void **out, size_t alignment, size_t size) {
  if (!out || alignment < sizeof(void *) || alignment % sizeof(void *) ||
      (alignment & (alignment - 1)))
    return EINVAL;
  int saved = errno;
  void *p = memalign(alignment, size ? size : 1);
  int rc = p ? 0 : ENOMEM;
  if (p) *out = p;
  errno = saved;
  return rc;
}
int __wrap___xpg_strerror_r(int error, char *buffer, size_t length) {
  return strerror_r(error, buffer, length);
}
/* Linux stat64 layout/flags are not newlib's. Preserve explicit unsupported
 * boundaries rather than writing the wrong structure or creating wrong files.
 */
int __wrap_open64(const char *path, int flags, ...) {
  (void)path;
  (void)flags;
  errno = ENOSYS;
  return -1;
}
int __wrap_fstat64(int fd, void *buffer) {
  (void)fd;
  (void)buffer;
  errno = ENOSYS;
  return -1;
}
int __wrap_stat64(const char *path, void *buffer) {
  (void)path;
  (void)buffer;
  errno = ENOSYS;
  return -1;
}
void *__wrap_mmap64(void *address, size_t length, int protection, int flags,
                    int fd, long long offset) {
  (void)address;
  (void)length;
  (void)protection;
  (void)flags;
  (void)fd;
  (void)offset;
  errno = ENOSYS;
  return (void *)-1;
}
int __wrap_munmap(void *address, size_t length) {
  (void)address;
  (void)length;
  errno = ENOSYS;
  return -1;
}
int __wrap_dl_iterate_phdr(void *callback, void *data) {
  (void)callback;
  (void)data;
  errno = ENOSYS;
  return -1;
}
struct SdkIovec {
  void *base;
  size_t length;
};
ssize_t __wrap_writev(int fd, const struct SdkIovec *vectors, int count) {
  if (count < 0 || count > 1024 || (count && !vectors)) {
    errno = EINVAL;
    return -1;
  }
  size_t size = 0;
  for (int i = 0; i < count; i++) {
    if (vectors[i].length > (size_t)SSIZE_MAX - size) {
      errno = EINVAL;
      return -1;
    }
    size += vectors[i].length;
  }
  ssize_t total = 0;
  for (int i = 0; i < count; i++) {
    ssize_t n = write(fd, vectors[i].base, vectors[i].length);
    if (n < 0) return total ? total : -1;
    total += n;
    if ((size_t)n < vectors[i].length) break;
  }
  return total;
}
int __wrap_vk_drm_syncobj_copy_payloads(void *device, unsigned wait_count,
                                        const void *waits,
                                        unsigned signal_count,
                                        const void *signals) {
  (void)device;
  (void)wait_count;
  (void)waits;
  (void)signal_count;
  (void)signals;
  return -8; /* VK_ERROR_FEATURE_NOT_PRESENT; Horizon exposes no DRM syncobj
                interop. */
}
