#pragma once
#include <stdint.h>
uint64_t armGetSystemTick(void);
uint64_t armGetSystemTickFreq(void);
uint64_t armTicksToNs(uint64_t);
