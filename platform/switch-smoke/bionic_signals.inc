/* Android LP64 signal-set ABI, independent of the host's sigset_t layout.
 * Set operations are ordinary memory operations. Horizon has no guest Linux
 * signal delivery backend: registration/mask queries return ENOSYS and leave
 * output buffers untouched. Never install handlers on the host process.
 * ABI reference: Android NDK signal.h / bits/signal_types.h (NDK 28.2).
 * Original AndroSwitch implementation, MIT OR Apache-2.0. */
static int signal_unavailable(const char *api) {
  if (as_native_trace_metadata)
    as_native_trace_metadata("signal_unavailable", (const uint8_t *)api,
                             strlen(api));
  errno = ENOSYS;
  return -1;
}
static int guest_sigaltstack(const void *input, void *output) {
  (void)input;
  (void)output;
  return signal_unavailable("sigaltstack");
}
static int guest_sigaction(int signal_number, const void *input, void *output) {
  (void)signal_number;
  (void)input;
  (void)output;
  return signal_unavailable("sigaction");
}
typedef void (*guest_signal_handler)(int);
static guest_signal_handler guest_signal(int number,
                                         guest_signal_handler handler) {
  (void)number;
  (void)handler;
  signal_unavailable("signal");
  return (guest_signal_handler)(intptr_t)-1;
}
static int guest_sigprocmask(int how, const void *input, void *output) {
  (void)how;
  (void)input;
  (void)output;
  return signal_unavailable("sigprocmask");
}
static int guest_pthread_sigmask(int how, const void *input, void *output) {
  int saved = errno;
  guest_sigprocmask(how, input, output);
  errno = saved;
  return 38; /* pthread API returns Android ENOSYS, without changing errno. */
}
static int guest_sigpending(void *output) {
  (void)output;
  return signal_unavailable("sigpending");
}
static int guest_sigemptyset(void *set) {
  if (!set) {
    errno = EINVAL;
    return -1;
  }
  memset(set, 0, sizeof(uint64_t));
  return 0;
}
static int guest_sigfillset(void *set) {
  if (!set) {
    errno = EINVAL;
    return -1;
  }
  memset(set, 0xff, sizeof(uint64_t));
  return 0;
}
static int guest_signal_bit(const void *set, int number, uint64_t *mask) {
  if (!set || number < 1 || number > 64) {
    errno = EINVAL;
    return -1;
  }
  *mask = UINT64_C(1) << (number - 1);
  return 0;
}
static int guest_sigaddset(void *set, int number) {
  uint64_t bits, mask;
  if (guest_signal_bit(set, number, &mask)) return -1;
  memcpy(&bits, set, sizeof(bits));
  bits |= mask;
  memcpy(set, &bits, sizeof(bits));
  return 0;
}
static int guest_sigdelset(void *set, int number) {
  uint64_t bits, mask;
  if (guest_signal_bit(set, number, &mask)) return -1;
  memcpy(&bits, set, sizeof(bits));
  bits &= ~mask;
  memcpy(set, &bits, sizeof(bits));
  return 0;
}
static int guest_sigismember(const void *set, int number) {
  uint64_t bits, mask;
  if (guest_signal_bit(set, number, &mask)) return -1;
  memcpy(&bits, set, sizeof(bits));
  return !!(bits & mask);
}
static uint64_t guest_signal_import(const char *name) {
#define SIGNAL_API(n, f) \
  if (!strcmp(name, n)) return (uint64_t)(f)
  SIGNAL_API("sigaltstack", guest_sigaltstack);
  SIGNAL_API("signal", guest_signal);
  SIGNAL_API("sigaction", guest_sigaction);
  SIGNAL_API("sigaction64", guest_sigaction);
  SIGNAL_API("sigprocmask", guest_sigprocmask);
  SIGNAL_API("sigprocmask64", guest_sigprocmask);
  SIGNAL_API("pthread_sigmask", guest_pthread_sigmask);
  SIGNAL_API("pthread_sigmask64", guest_pthread_sigmask);
  SIGNAL_API("sigpending", guest_sigpending);
  SIGNAL_API("sigpending64", guest_sigpending);
  SIGNAL_API("sigemptyset", guest_sigemptyset);
  SIGNAL_API("sigemptyset64", guest_sigemptyset);
  SIGNAL_API("sigfillset", guest_sigfillset);
  SIGNAL_API("sigfillset64", guest_sigfillset);
  SIGNAL_API("sigaddset", guest_sigaddset);
  SIGNAL_API("sigaddset64", guest_sigaddset);
  SIGNAL_API("sigdelset", guest_sigdelset);
  SIGNAL_API("sigdelset64", guest_sigdelset);
  SIGNAL_API("sigismember", guest_sigismember);
  SIGNAL_API("sigismember64", guest_sigismember);
#undef SIGNAL_API
  return 0;
}
