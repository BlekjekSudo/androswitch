/* Fixed Android ARM64 filesystem ABI records. MIT OR Apache-2.0. */
#pragma once
#include <stdint.h>
typedef struct {
  uint64_t device, inode;
  uint32_t mode, links, uid, gid;
  uint64_t rdev, pad1;
  int64_t size;
  int32_t block_size, pad2;
  int64_t blocks, access_time[2], modify_time[2], change_time[2];
  uint32_t unused[2];
} AsGuestStat;
int as_bionic_stat_path(const char *path, AsGuestStat *out);
typedef struct {
  uint64_t inode;
  int64_t offset;
  uint16_t record_size;
  uint8_t type;
  char name[256];
} AsGuestDirent;
