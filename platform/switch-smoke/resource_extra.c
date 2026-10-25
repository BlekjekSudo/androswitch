/* Read-only host observations, not Android build identity. MIT OR Apache-2.0.
 */
#include <errno.h>
#include <stdint.h>
#include <switch.h>

int as_platform_resource_query(uint32_t kind, uint64_t *output) {
  if (!output) return -EINVAL;
  if (kind == 0) {
    u64 mask = 0;
    Result rc = svcGetInfo(&mask, InfoType_CoreMask, CUR_PROCESS_HANDLE, 0);
    if (R_FAILED(rc)) return (int)rc;
    if (!mask) return -EINVAL;
    *output = mask;
    return 0;
  }
  if (kind == 4) {
    *output = armGetSystemTickFreq();
    return 0;
  }
  if (kind == 5) {
    *output = hosversionGet();
    return 0;
  }
  if (kind < 1 || kind > 3) return -EINVAL;
  PcvModule module = kind == 1   ? PcvModule_CpuBus
                     : kind == 2 ? PcvModule_GPU
                                 : PcvModule_EMC;
  u32 rate = 0;
  Result rc;
  if (hosversionAtLeast(8, 0, 0)) {
    rc = clkrstInitialize();
    if (R_FAILED(rc)) return (int)rc;
    ClkrstSession session;
    PcvModuleId id = kind == 1   ? PcvModuleId_CpuBus
                     : kind == 2 ? PcvModuleId_GPU
                                 : PcvModuleId_EMC;
    rc = clkrstOpenSession(&session, id, 3);
    if (R_SUCCEEDED(rc)) {
      rc = clkrstGetClockRate(&session, &rate);
      clkrstCloseSession(&session);
    }
    clkrstExit();
  } else {
    rc = pcvInitialize();
    if (R_FAILED(rc)) return (int)rc;
    rc = pcvGetClockRate(module, &rate);
    pcvExit();
  }
  if (R_FAILED(rc)) return (int)rc;
  /* Zero is retained if the platform reports a gated/zero clock. No setter
   * or overclock request is issued. This is the reported clock rate, not a
   * measurement of instructions executed per second. */
  *output = rate;
  return 0;
}
