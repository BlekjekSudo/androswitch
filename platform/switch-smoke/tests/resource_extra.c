#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <switch.h>
int as_platform_resource_query(uint32_t, uint64_t *);
static int modern = 1, exits, closes, opened, last_module;
static Result init_error, open_error, read_error;
static u32 clock_hz = 1020000000;
static u64 mask = 7;
static u32 version = 0x130000;
Result svcGetInfo(u64 *v, int t, int h, u64 n) {
  assert(t == InfoType_CoreMask && h == CUR_PROCESS_HANDLE && !n);
  if (read_error) return read_error;
  *v = mask;
  return 0;
}
u64 armGetSystemTickFreq(void) { return 19200000; }
u32 hosversionGet(void) { return version; }
bool hosversionAtLeast(int a, int b, int c) {
  assert(a == 8 && !b && !c);
  return modern;
}
Result clkrstInitialize(void) { return init_error; }
void clkrstExit(void) { exits++; }
Result clkrstOpenSession(ClkrstSession *s, PcvModuleId m, u32 n) {
  assert(n == 3);
  last_module = m;
  if (open_error) return open_error;
  s->token = 47;
  opened++;
  return 0;
}
Result clkrstGetClockRate(ClkrstSession *s, u32 *n) {
  assert(s->token == 47);
  if (read_error) return read_error;
  *n = clock_hz;
  return 0;
}
void clkrstCloseSession(ClkrstSession *s) {
  assert(s->token == 47);
  closes++;
}
Result pcvInitialize(void) { return init_error; }
void pcvExit(void) { exits++; }
Result pcvGetClockRate(PcvModule m, u32 *n) {
  last_module = m;
  if (read_error) return read_error;
  *n = clock_hz;
  return 0;
}
int main(void) {
  uint64_t out = 91;
  assert(as_platform_resource_query(0, NULL) == -EINVAL);
  assert(as_platform_resource_query(99, &out) == -EINVAL && out == 91);
  assert(!as_platform_resource_query(0, &out) && out == 7);
  mask = 0;
  out = 91;
  assert(as_platform_resource_query(0, &out) == -EINVAL && out == 91);
  mask = 7;
  assert(!as_platform_resource_query(4, &out) && out == 19200000);
  assert(!as_platform_resource_query(5, &out) && out == 0x130000);
  version = 0;
  out = 91;
  for (unsigned kind = 1; kind <= 5; kind++) {
    if (kind == 4) continue;
    assert(as_platform_resource_query(kind, &out) == -ENOSYS && out == 91);
  }
  assert(!opened && !closes && !exits);
  version = 0x130000;
  for (unsigned kind = 1; kind <= 3; kind++) {
    assert(!as_platform_resource_query(kind, &out) && out == clock_hz);
    assert(last_module == (kind == 1   ? PcvModuleId_CpuBus
                           : kind == 2 ? PcvModuleId_GPU
                                       : PcvModuleId_EMC));
  }
  assert(opened == 3 && closes == 3 && exits == 3);
  read_error = 0x123;
  out = 91;
  assert(as_platform_resource_query(1, &out) == 0x123 && out == 91);
  assert(closes == 4 && exits == 4);
  read_error = 0;
  open_error = 0x456;
  assert(as_platform_resource_query(2, &out) == 0x456 && out == 91);
  assert(closes == 4 && exits == 5);
  open_error = 0;
  init_error = 0x789;
  assert(as_platform_resource_query(3, &out) == 0x789 && out == 91);
  assert(exits == 5);
  init_error = 0;
  modern = 0;
  version = 0x070000;
  for (unsigned kind = 1; kind <= 3; kind++) {
    assert(!as_platform_resource_query(kind, &out));
    assert(last_module == (kind == 1   ? PcvModule_CpuBus
                           : kind == 2 ? PcvModule_GPU
                                       : PcvModule_EMC));
  }
  assert(exits == 8);
  read_error = 7;
  out = 91;
  assert(as_platform_resource_query(2, &out) == 7 && out == 91 && exits == 9);
  read_error = 0;
  clock_hz = 0;
  assert(!as_platform_resource_query(2, &out) && out == 0);
  puts(
      "RESOURCE_EXTRA_PASS: real wrapper, mask/clock/timer/version routing, "
      "modern+legacy, cleanup/error/zero preservation; no setters");
}
