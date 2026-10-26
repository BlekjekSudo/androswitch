#pragma once
#include <stdint.h>
int as_horizon_std_init(void);
int as_horizon_std_shutdown(unsigned timeout_ms);
int as_std_reaper_drain(unsigned timeout_ms);
int as_std_detach_result(void);
unsigned as_std_reaped_threads(void);
unsigned as_std_live_threads(void);
long as_std_process_core_count(void);
