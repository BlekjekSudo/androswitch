/* AndroSwitch root-relative storage adapter, MIT OR Apache-2.0. */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
#include <sys/stat.h>
#include <unistd.h>

/* Reporting identity uses real libnx randomness plus the monotonic system tick;
 * the clock is an identity component, not a claim about available entropy. */
uint64_t as_reporting_nonce(void) { return randomGet64(); }
uint64_t as_reporting_tick(void) { return armGetSystemTick(); }

int as_storage_prepare(const char *root) {
  if (strncmp(root, "sdmc:/switch/androswitch", 23) != 0 || strlen(root) >= 768)
    return -EINVAL;
  char path[768];
  strcpy(path, root);
  for (char *part = path + 6; *part; part++)
    if (*part == '/') {
      *part = 0;
      if (mkdir(path, 0777) && errno != EEXIST) return -errno;
      *part = '/';
    }
  if (mkdir(path, 0777) && errno != EEXIST) return -errno;
  return 0;
}
int as_storage_stat(const char *path, uint64_t *length, uint32_t *directory) {
  struct stat st;
  if (stat(path, &st)) {
    return errno == ENOENT ? 1 : -errno;
  }
  *length = (uint64_t)st.st_size;
  *directory = S_ISDIR(st.st_mode);
  return 0;
}
int as_storage_create_file_new(const char *path) {
  int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0666);
  if (fd < 0) return errno == EEXIST ? 0 : -errno;
  if (close(fd)) return -errno;
  if (R_FAILED(fsdevCommitDevice("sdmc"))) return -EIO;
  return 1;
}
int as_storage_modified_millis(const char *path, uint64_t *millis) {
  struct stat st;
  if (stat(path, &st)) return -errno;
  if (st.st_mtime < 0) return -ERANGE;
  *millis = (uint64_t)st.st_mtime * 1000;
  return 0;
}
int64_t as_storage_read(const char *path, uint64_t offset, uint8_t *output,
                        size_t length) {
  FILE *file = fopen(path, "rb");
  if (!file) return -errno;
  if (fseeko(file, (off_t)offset, SEEK_SET)) {
    int e = errno;
    fclose(file);
    return -e;
  }
  size_t n = fread(output, 1, length, file);
  int e = ferror(file) ? EIO : 0;
  fclose(file);
  return e ? -e : (int64_t)n;
}
int64_t as_storage_write(const char *path, uint64_t offset,
                         const uint8_t *input, size_t length,
                         uint32_t truncate) {
  FILE *file = fopen(path, truncate ? "wb" : "r+b");
  if (!file && !truncate && errno == ENOENT) file = fopen(path, "w+b");
  if (!file) return -errno;
  if (fseeko(file, (off_t)offset, SEEK_SET)) {
    int e = errno;
    fclose(file);
    return -e;
  }
  size_t n = fwrite(input, 1, length, file);
  int e = (n != length || fflush(file)) ? EIO : 0;
  if (fclose(file)) e = EIO;
  if (!e && R_FAILED(fsdevCommitDevice("sdmc"))) e = EIO;
  return e ? -e : (int64_t)n;
}
int as_storage_sync_file(const char *path) {
  struct stat st;
  if (stat(path, &st)) return -errno;
  if (S_ISDIR(st.st_mode)) return -EISDIR;
  /* Positional writes already fflush/fclose before returning. Reuse the same
   * filesystem commit used by durable writes and installation publication. */
  return R_FAILED(fsdevCommitDevice("sdmc")) ? -EIO : 0;
}
uint64_t as_storage_writer_open(const char *path) {
  return (uint64_t)fopen(path, "wb");
}
int64_t as_storage_writer_write(uint64_t handle, const uint8_t *bytes,
                                size_t length) {
  FILE *file = (FILE *)handle;
  if (!file) return -EINVAL;
  size_t written = fwrite(bytes, 1, length, file);
  return written == length ? (int64_t)written : -EIO;
}
int as_storage_writer_finish(uint64_t handle) {
  FILE *file = (FILE *)handle;
  if (!file) return -EINVAL;
  int error = fflush(file) ? EIO : 0;
  if (fclose(file)) error = EIO;
  if (!error && R_FAILED(fsdevCommitDevice("sdmc"))) error = EIO;
  return error ? -error : 0;
}
void as_storage_writer_close(uint64_t handle) {
  if (handle) fclose((FILE *)handle);
}
int as_storage_mkdir(const char *path, uint32_t exclusive) {
  if (!mkdir(path, 0777)) return 1;
  if (errno == EEXIST) {
    struct stat st;
    if (!stat(path, &st) && S_ISDIR(st.st_mode)) return 0;
    if (exclusive) return 0;
  }
  return -errno;
}
int64_t as_storage_list(const char *path, uint8_t *output, size_t capacity) {
  DIR *dir = opendir(path);
  if (!dir) return -errno;
  size_t used = 0;
  struct dirent *e;
  while ((e = readdir(dir))) {
    if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
    size_t n = strlen(e->d_name);
    if (used + n + 2 > capacity) {
      closedir(dir);
      return -28;
    }
    char full[1024];
    if (snprintf(full, sizeof(full), "%s/%s", path, e->d_name) >=
        (int)sizeof(full)) {
      closedir(dir);
      return -ENAMETOOLONG;
    }
    struct stat st;
    if (stat(full, &st)) {
      int error = errno;
      closedir(dir);
      return -error;
    }
    output[used++] = S_ISDIR(st.st_mode) ? 'D' : 'F';
    memcpy(output + used, e->d_name, n + 1);
    used += n + 1;
  }
  closedir(dir);
  return (int64_t)used;
}
int as_storage_rename(const char *source, const char *destination) {
  struct stat st;
  if (!stat(destination, &st)) return -EEXIST;
  if (errno != ENOENT) return -errno;
  if (rename(source, destination)) return -errno;
  if (R_SUCCEEDED(fsdevCommitDevice("sdmc"))) return 0;
  /* Restore the old visible state if publication could not be committed. */
  if (!rename(destination, source)) {
    fsdevCommitDevice("sdmc");
    return -EIO;
  }
  return -EINPROGRESS; /* Publication outcome is indeterminate; caller must
                          reload. */
}
int as_storage_rename_replace(const char *source, const char *destination) {
  if (rename(source, destination)) return -errno;
  /* Replacement cannot be rolled back after a failed durability commit.
   * Report the indeterminate outcome instead of claiming durable success. */
  return R_FAILED(fsdevCommitDevice("sdmc")) ? -EINPROGRESS : 0;
}
int as_storage_remove(const char *path, uint32_t recursive) {
  struct stat st;
  if (stat(path, &st)) return -errno;
  if (!S_ISDIR(st.st_mode)) return unlink(path) ? -errno : 0;
  if (recursive) {
    DIR *dir = opendir(path);
    if (!dir) return -errno;
    struct dirent *entry;
    while ((entry = readdir(dir))) {
      if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) continue;
      char full[1024];
      if (snprintf(full, sizeof(full), "%s/%s", path, entry->d_name) >=
          (int)sizeof(full)) {
        closedir(dir);
        return -ENAMETOOLONG;
      }
      int rc = as_storage_remove(full, 1);
      if (rc) {
        closedir(dir);
        return rc;
      }
    }
    closedir(dir);
  }
  return rmdir(path) ? -errno : 0;
}
