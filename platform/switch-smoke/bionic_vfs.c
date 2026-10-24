/* Shared VM filesystem adapters for Bionic's ARM64 stat/directory ABI.
 * Virtual files expose sandbox modes and unknown timestamps as Unix epoch;
 * timestamps cannot be set until the platform storage API supports them.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_vfs.h"
#include "bionic_cxx.h"
#include "bionic_errno.h"
#include "native.h"
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
extern int64_t as_guest_fs(uint32_t, const char *, uint64_t, uint8_t *, size_t,
                           uint32_t);
_Static_assert(sizeof(AsGuestStat) == 128, "Bionic ARM64 stat ABI");
_Static_assert(sizeof(AsGuestDirent) == 280, "Bionic ARM64 dirent ABI");
static int64_t operation(uint32_t op, const char *path, void *bytes,
                         size_t length, unsigned flags) {
  as_jni_vm_lock();
  int64_t result = as_guest_fs(op, path, 0, bytes, length, flags);
  as_jni_vm_unlock();
  if (result < 0)
    errno = as_errno_from_android((int)-result);
  return result;
}
static uint64_t inode(const char *path) {
  uint64_t result = 14695981039346656037ull;
  for (; *path; ++path)
    result = (result ^ (unsigned char)*path) * 1099511628211ull;
  return result ? result : 1;
}
static int stat_path(const char *path, AsGuestStat *out) {
  uint64_t metadata[3];
  if (!out) {
    errno = EFAULT;
    return -1;
  }
  if (operation(104, path, metadata, sizeof(metadata), 0) < 0)
    return -1;
  memset(out, 0, sizeof(*out));
  out->device = 1;
  out->inode = metadata[2];
  out->mode = metadata[1] & 1   ? (metadata[1] & 2 ? 0040500 : 0040700)
              : metadata[1] & 2 ? 0100400
                                : 0100600;
  out->links = 1;
  out->uid = out->gid = 10000;
  if (metadata[0] > INT64_MAX) {
    errno = EOVERFLOW;
    return -1;
  }
  out->size = (int64_t)metadata[0];
  out->block_size = 4096;
  out->blocks = (int64_t)(metadata[0] / 512 + (metadata[0] % 512 != 0));
  return 0;
}
static int access_path(const char *path, int mode) {
  if (mode & ~7) {
    errno = EINVAL;
    return -1;
  }
  AsGuestStat metadata;
  if (stat_path(path, &metadata))
    return -1;
  unsigned permissions = mode << 6;
  if ((metadata.mode & permissions) != permissions) {
    errno = EACCES;
    return -1;
  }
  return 0;
}
static int remove_path(const char *path) {
  return operation(105, path, NULL, 0, 0) < 0 ? -1 : 0;
}
static int rename_path(const char *source, const char *destination) {
  return operation(107, source, (void *)destination, strlen(destination) + 1,
                   0) < 0
             ? -1
             : 0;
}
static int utime_path(const char *path, const void *times) {
  (void)path;
  (void)times;
  errno = ENOSYS;
  return -1;
}
static __thread uint64_t sort_function;
static __thread int sort_error;
static int compare_entries(const void *left, const void *right) {
  if (sort_error)
    return 0;
  AsNativeCall frame = {0};
  frame.function = sort_function;
  frame.integer[0] = (uint64_t)left;
  frame.integer[1] = (uint64_t)right;
  sort_error = as_native_call(&frame);
  return (int)frame.result_integer[0];
}
static int alpha_sort(const AsGuestDirent **left, const AsGuestDirent **right) {
  return strcoll((*left)->name, (*right)->name);
}
static int scan_directory(const char *path, AsGuestDirent ***out,
                          uint64_t filter, uint64_t comparator) {
  if (!out) {
    errno = EINVAL;
    return -1;
  }
  *out = NULL;
  AsGuestStat metadata;
  if (stat_path(path, &metadata))
    return -1;
  if ((metadata.mode & 0170000) != 0040000) {
    errno = ENOTDIR;
    return -1;
  }
  AsGuestDirent **entries = calloc(4098, sizeof(*entries));
  if (!entries) {
    errno = ENOMEM;
    return -1;
  }
  unsigned count = 0;
  int failed = 0, callback_error = 0;
  for (unsigned i = 0; i < 4098; ++i) {
    unsigned char data[257] = {0};
    int64_t more = 1;
    if (i >= 2)
      more = operation(106, path, data, sizeof(data), i - 2);
    else {
      strcpy((char *)data, i ? ".." : ".");
      data[256] = 1;
    }
    if (more < 0) {
      failed = 1;
      break;
    }
    if (!more)
      break;
    AsGuestDirent *entry = as_cxx_allocate(sizeof(*entry));
    if (!entry) {
      errno = ENOMEM;
      failed = 1;
      break;
    }
    memset(entry, 0, sizeof(*entry));
    entry->inode = inode((char *)data);
    entry->offset = i + 1;
    entry->record_size = sizeof(*entry);
    entry->type = data[256] ? 4 : 8;
    memcpy(entry->name, data, 256);
    if (filter) {
      AsNativeCall frame = {0};
      frame.function = filter;
      frame.integer[0] = (uint64_t)entry;
      callback_error = as_native_call(&frame);
      if (callback_error) {
        as_cxx_deallocate(entry);
        failed = 1;
        break;
      }
      if (!(int)frame.result_integer[0]) {
        as_cxx_deallocate(entry);
        continue;
      }
    }
    entries[count++] = entry;
  }
  if (!failed && comparator) {
    uint64_t previous_function = sort_function;
    int previous_error = sort_error;
    sort_function = comparator;
    sort_error = 0;
    qsort(entries, count, sizeof(*entries), compare_entries);
    callback_error = sort_error;
    failed = callback_error != 0;
    sort_function = previous_function;
    sort_error = previous_error;
  }
  AsGuestDirent **result =
      failed ? NULL : as_cxx_allocate((count ? count : 1) * sizeof(*result));
  if (!result && !failed) {
    errno = ENOMEM;
    failed = 1;
  }
  if (failed)
    for (unsigned i = 0; i < count; ++i)
      as_cxx_deallocate(entries[i]);
  else
    memcpy(result, entries, count * sizeof(*result));
  free(entries);
  if (callback_error)
    as_native_trap(callback_error);
  if (failed)
    return -1;
  *out = result;
  return (int)count;
}
#define API(n, f)                                                              \
  if (!strcmp(name, n))                                                        \
  return (uint64_t)(f)
uint64_t as_bionic_vfs_import(const char *name) {
  API("stat", stat_path);
  API("stat64", stat_path);
  API("lstat", stat_path);
  API("access", access_path);
  API("remove", remove_path);
  API("rename", rename_path);
  API("utime", utime_path);
  API("scandir", scan_directory);
  API("alphasort", alpha_sort);
  return 0;
}
