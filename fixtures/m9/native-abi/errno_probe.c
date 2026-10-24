/* Compile against the actual Switch/newlib headers; run pure conversion code
 * in the host ARM64 CPU oracle. MIT OR Apache-2.0. */
#include "bionic_errno.h"
int oracle_errno_to_android(int value) { return as_errno_to_android(value); }
int oracle_errno_from_android(int value) {
  return as_errno_from_android(value);
}
int oracle_errno_host_value(int index) {
  switch (index) {
  case 0:
    return ENOSYS;
  case 1:
    return EDEADLK;
  case 2:
    return ENOTSUP;
  case 3:
    return ETIMEDOUT;
  case 4:
    return EOVERFLOW;
  case 5:
    return ELOOP;
  case 6:
    return EAGAIN;
  case 7:
    return ENOMEM;
  case 8:
    return EINVAL;
  default:
    return EIO;
  }
}
