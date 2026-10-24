/* Finalizers may create more current-thread state, including while a global
 * destructor runs. Settle real callbacks while code, VM/JNI and TLS still live.
 * Normal __cxa_finalize and pthread-key destructor semantics stay in their
 * providers; this helper coordinates whole-session termination only.
 * Original AndroSwitch implementation, MIT OR Apache-2.0. */
#include "bionic_finalize.h"
#include "bionic_cxx.h"
#include <errno.h>
extern int as_bionic_keys_thread_finalize(void);
extern unsigned as_bionic_keys_pending(void);
unsigned as_bionic_finalization_pending(void) {
  return as_bionic_cxx_pending() |
         (as_bionic_keys_pending() ? AS_FINALIZE_PENDING_KEYS : 0);
}
int as_bionic_finalize_thread(void) {
  for (unsigned pass = 0; pass < AS_FINALIZE_PASSES; ++pass) {
    int result = as_bionic_cxx_thread_finalize();
    if (result)
      return result;
    result = as_bionic_keys_thread_finalize();
    if (result)
      return result;
    /* A pthread-key destructor can register a C++ TLS destructor. */
    if (!(as_bionic_finalization_pending() &
          (AS_CXX_PENDING_THREAD | AS_FINALIZE_PENDING_KEYS)))
      return 0;
  }
  return -ELOOP;
}
int as_bionic_finalize_session(void) {
  for (unsigned pass = 0; pass < AS_FINALIZE_PASSES; ++pass) {
    int result = as_bionic_cxx_finalize();
    if (result)
      return result;
    result = as_bionic_finalize_thread();
    if (result)
      return result;
    /* Newly created TLS state can in turn register a global destructor. */
    if (!as_bionic_finalization_pending())
      return 0;
  }
  /* Retain pending entries; the caller keeps the session/mappings alive. */
  return -ELOOP;
}
