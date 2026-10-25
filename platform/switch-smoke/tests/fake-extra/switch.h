#pragma once
#include <stdbool.h>
#include <stdint.h>
typedef uint64_t u64;
typedef uint32_t u32;
typedef uint32_t Result;
typedef int PcvModule;
typedef int PcvModuleId;
typedef struct {
  int token;
} ClkrstSession;
#define InfoType_CoreMask 0
#define CUR_PROCESS_HANDLE 1
#define R_FAILED(x) ((x) != 0)
#define R_SUCCEEDED(x) ((x) == 0)
#define PcvModule_CpuBus 0
#define PcvModule_GPU 1
#define PcvModule_EMC 56
#define PcvModuleId_CpuBus 0x40000001
#define PcvModuleId_GPU 0x40000002
#define PcvModuleId_EMC 0x40000039
Result svcGetInfo(u64*, int, int, u64);
u64 armGetSystemTickFreq(void);
u32 hosversionGet(void);
bool hosversionAtLeast(int, int, int);
Result clkrstInitialize(void);
void clkrstExit(void);
Result clkrstOpenSession(ClkrstSession*, PcvModuleId, u32);
Result clkrstGetClockRate(ClkrstSession*, u32*);
void clkrstCloseSession(ClkrstSession*);
Result pcvInitialize(void);
void pcvExit(void);
Result pcvGetClockRate(PcvModule, u32*);
