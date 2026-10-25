/* General Vulkan discovery/device bridge, MIT OR Apache-2.0. */
#pragma once
#include <stdint.h>
uint64_t as_vulkan_import(const char *name);
int as_vulkan_reset(void);
