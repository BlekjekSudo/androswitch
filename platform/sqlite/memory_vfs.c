/* AndroSwitch bounded SQLite snapshot backend, MIT OR Apache-2.0.
 * SQLite itself is public domain; see third_party/sqlite.lock.json.
 * Only memory databases are opened. Persistence/atomic snapshots belong to the
 * shared storage adapter; no Linux syscalls or host filesystem VFS is linked.
 */
#if !defined(__SWITCH__) && !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif
#include "sqlite3.h"
#include <string.h>
#include <time.h>
#ifdef __SWITCH__
#include <switch.h>
#elif defined(_WIN32)
// clang-format off: Windows types must precede bcrypt declarations.
#include <windows.h>
#include <bcrypt.h>
// clang-format on
#else
#include <stdio.h>
#include <unistd.h>
#endif
static int memory_open(sqlite3_vfs *v, const char *p, sqlite3_file *f,
                       int flags, int *out) {
  (void)v;
  (void)p;
  (void)f;
  (void)flags;
  (void)out;
  return SQLITE_CANTOPEN;
}
static int memory_delete(sqlite3_vfs *v, const char *p, int sync) {
  (void)v;
  (void)p;
  (void)sync;
  return SQLITE_IOERR_DELETE;
}
static int memory_access(sqlite3_vfs *v, const char *p, int flags, int *out) {
  (void)v;
  (void)p;
  (void)flags;
  *out = 0;
  return SQLITE_OK;
}
static int memory_path(sqlite3_vfs *v, const char *p, int length, char *out) {
  (void)v;
  if ((int)strlen(p) >= length)
    return SQLITE_CANTOPEN;
  memcpy(out, p, strlen(p) + 1);
  return SQLITE_OK;
}
static int memory_random(sqlite3_vfs *v, int length, char *out) {
  (void)v;
#ifdef __SWITCH__
  randomGet(out, (size_t)length);
  return length;
#elif defined(_WIN32)
  return BCryptGenRandom(NULL, (PUCHAR)out, (ULONG)length,
                         BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0
             ? length
             : 0;
#else
  FILE *file = fopen("/dev/urandom", "rb");
  if (!file)
    return 0;
  int count = (int)fread(out, 1, (size_t)length, file);
  fclose(file);
  return count;
#endif
}
static int memory_sleep(sqlite3_vfs *v, int micros) {
  (void)v;
#ifdef __SWITCH__
  svcSleepThread((int64_t)micros * 1000);
#elif defined(_WIN32)
  Sleep((DWORD)((micros + 999) / 1000));
#else
  struct timespec delay = {micros / 1000000, (long)(micros % 1000000) * 1000};
  nanosleep(&delay, NULL);
#endif
  return micros;
}
static int memory_time64(sqlite3_vfs *v, sqlite3_int64 *out) {
  (void)v;
  *out = (sqlite3_int64)time(NULL) * 1000 + 210866760000000LL;
  return SQLITE_OK;
}
static int memory_time(sqlite3_vfs *v, double *out) {
  sqlite3_int64 value;
  int result = memory_time64(v, &value);
  *out = value / 86400000.0;
  return result;
}
static sqlite3_vfs memory_vfs = {.iVersion = 2,
                                 .szOsFile = sizeof(sqlite3_file),
                                 .mxPathname = 1024,
                                 .zName = "androswitch-memory",
                                 .xOpen = memory_open,
                                 .xDelete = memory_delete,
                                 .xAccess = memory_access,
                                 .xFullPathname = memory_path,
                                 .xRandomness = memory_random,
                                 .xSleep = memory_sleep,
                                 .xCurrentTime = memory_time,
                                 .xCurrentTimeInt64 = memory_time64};
int sqlite3_os_init(void) { return sqlite3_vfs_register(&memory_vfs, 1); }
int sqlite3_os_end(void) { return SQLITE_OK; }
