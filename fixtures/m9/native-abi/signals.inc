/* Android LP64 layout and unsupported-delivery contracts, no host signals. */
static void signal_contracts(void) {
  int (*empty)(void *) = (void *)as_bionic_libc_import("sigemptyset");
  int (*fill)(void *) = (void *)as_bionic_libc_import("sigfillset");
  int (*add)(void *, int) = (void *)as_bionic_libc_import("sigaddset");
  int (*del)(void *, int) = (void *)as_bionic_libc_import("sigdelset");
  int (*member)(const void *, int) =
      (void *)as_bionic_libc_import("sigismember");
  assert(empty && fill && add && del && member);
  assert(as_bionic_libc_import("sigemptyset64") == (uint64_t)empty);
  assert(as_bionic_libc_import("sigfillset64") == (uint64_t)fill);
  assert(as_bionic_libc_import("sigaddset64") == (uint64_t)add);
  assert(as_bionic_libc_import("sigdelset64") == (uint64_t)del);
  assert(as_bionic_libc_import("sigismember64") == (uint64_t)member);
  unsigned char storage[10];
  memset(storage, 0xa5, sizeof(storage));
  void *set =
      storage + 1; /* Check exact 8-byte ABI, including unaligned input. */
  errno = E2BIG;
  assert(empty(set) == 0 && errno == E2BIG);
  uint64_t bits;
  memcpy(&bits, set, sizeof(bits));
  assert(bits == 0 && storage[0] == 0xa5 && storage[9] == 0xa5);
  for (int number = 1; number <= 64; ++number) {
    assert(add(set, number) == 0 && member(set, number) == 1);
    memcpy(&bits, set, sizeof(bits));
    assert(bits == (number == 64 ? UINT64_MAX : (UINT64_C(1) << number) - 1));
  }
  assert(fill(set) == 0);
  for (int number = 1; number <= 64; ++number) {
    assert(del(set, number) == 0 && member(set, number) == 0);
  }
  memcpy(&bits, set, sizeof(bits));
  assert(bits == 0);
  const int invalid[] = {INT_MIN, -1, 0, 65, INT_MAX};
  for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) {
    errno = 0;
    assert(add(set, invalid[i]) == -1 && errno == EINVAL);
    assert(del(set, invalid[i]) == -1 && member(set, invalid[i]) == -1);
  }
  assert(empty(NULL) == -1 && fill(NULL) == -1);
  assert(add(NULL, 1) == -1 && del(NULL, 1) == -1 && member(NULL, 1) == -1);
  assert(storage[0] == 0xa5 && storage[9] == 0xa5);
  unsigned char output[64], original[64];
  memset(output, 0xa5, sizeof(output));
  memcpy(original, output, sizeof(output));
  int (*alt)(const void *, void *) =
      (void *)as_bionic_libc_import("sigaltstack");
  int (*action)(int, const void *, void *) =
      (void *)as_bionic_libc_import("sigaction");
  int (*mask)(int, const void *, void *) =
      (void *)as_bionic_libc_import("sigprocmask");
  int (*pmask)(int, const void *, void *) =
      (void *)as_bionic_libc_import("pthread_sigmask");
  int (*pending)(void *) = (void *)as_bionic_libc_import("sigpending");
  typedef void (*handler)(int);
  handler (*install)(int, handler) = (void *)as_bionic_libc_import("signal");
  assert(alt && action && mask && pmask && pending && install);
  assert(as_bionic_libc_import("sigaction64") == (uint64_t)action);
  assert(as_bionic_libc_import("sigprocmask64") == (uint64_t)mask);
  assert(as_bionic_libc_import("pthread_sigmask64") == (uint64_t)pmask);
  assert(as_bionic_libc_import("sigpending64") == (uint64_t)pending);
  assert(alt(NULL, output) == -1 && errno == ENOSYS);
  assert(alt(output, NULL) == -1 && errno == ENOSYS);
  assert(action(11, output, output) == -1 && errno == ENOSYS);
  assert(mask(0, set, output) == -1 && errno == ENOSYS);
  errno = E2BIG;
  assert(pmask(0, set, output) == 38 && errno == E2BIG);
  assert(pending(output) == -1 && errno == ENOSYS);
  assert(install(11, NULL) == (handler)(intptr_t)-1 && errno == ENOSYS);
  assert(!memcmp(original, output, sizeof(output)));
  puts(
      "native signal contracts: PASS (LP64 sets 1..64; ENOSYS delivery, "
      "untouched outputs, pthread errno)");
}
