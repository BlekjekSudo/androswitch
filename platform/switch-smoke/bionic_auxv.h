/* Android ARM64 auxiliary-vector facts exposed by the Horizon backend.
 * Numeric ABI definitions: AOSP linux/auxvec.h and asm-arm64/asm/hwcap.h.
 * No Linux process stack, kernel pointer or host auxiliary vector is exposed.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#pragma once

static inline int as_bionic_auxv(unsigned long type, unsigned long *value) {
  switch (type) {
  case 6: /* AT_PAGESZ: the loader/libnx mapping granule is 4096 bytes. */
    *value = 4096;
    return 1;
  case 16: /* AT_HWCAP: FP | ASIMD, used by the ARM64 runtime itself. */
    /* Do not advertise optional crypto, CRC, LSE, SVE or EL0 ID-register access
     * without platform detection. Callers retain their baseline fallbacks. */
    *value = (1UL << 0) | (1UL << 1);
    return 1;
  case 23: /* AT_SECURE: this runtime never performs credential-changing exec.
            */
    *value = 0;
    return 1;
  case 26: /* AT_HWCAP2: no extension in this bank is exposed by the backend. */
    *value = 0;
    return 1;
  default:
    return 0;
  }
}
