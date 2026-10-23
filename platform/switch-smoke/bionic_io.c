/* Android FILE is opaque to NDK callers. All bytes go through the VM VFS.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "native.h"
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern int64_t as_guest_fs(uint32_t, const char *, uint64_t, uint8_t *, size_t,
                           uint32_t);
typedef struct {
  char *path;
  uint64_t position;
  int readable, writable, eof, error;
} GuestFile;
static int64_t operation(uint32_t op, const char *path, uint64_t offset,
                         void *bytes, size_t length, uint32_t flags) {
  as_jni_vm_lock();
  int64_t rc = as_guest_fs(op, path, offset, bytes, length, flags);
  as_jni_vm_unlock();
  if (rc < 0)
    errno = (int)-rc;
  return rc;
}
static GuestFile *guest_fopen(const char *path, const char *mode) {
  int read = mode[0] == 'r' || strchr(mode, '+'),
      write = mode[0] == 'w' || mode[0] == 'a' || strchr(mode, '+');
  if (!strchr("rwa", mode[0])) {
    errno = EINVAL;
    return NULL;
  }
  uint32_t flags = (read ? 1 : 0) | (write ? 2 : 0) | (mode[0] == 'w' ? 4 : 0) |
                   (mode[0] == 'a' ? 8 : 0);
  int64_t position = operation(100, path, 0, NULL, 0, flags);
  if (position < 0)
    return NULL;
  GuestFile *file = calloc(1, sizeof(*file));
  if (!file) {
    errno = ENOMEM;
    return NULL;
  }
  file->path = strdup(path);
  if (!file->path) {
    free(file);
    errno = ENOMEM;
    return NULL;
  }
  file->position = (uint64_t)position;
  file->readable = read;
  file->writable = write;
  return file;
}
static size_t guest_fread(void *bytes, size_t size, size_t count,
                          GuestFile *file) {
  if (!size || !count)
    return 0;
  if (!file->readable || count > SIZE_MAX / size) {
    file->error = 1;
    errno = EBADF;
    return 0;
  }
  int64_t n =
      operation(101, file->path, file->position, bytes, size * count, 0);
  if (n < 0) {
    file->error = 1;
    return 0;
  }
  file->position += (uint64_t)n;
  if ((size_t)n < size * count)
    file->eof = 1;
  return (size_t)n / size;
}
static size_t guest_fwrite(const void *bytes, size_t size, size_t count,
                           GuestFile *file) {
  if (!size || !count)
    return 0;
  if (!file->writable || count > SIZE_MAX / size) {
    file->error = 1;
    errno = EBADF;
    return 0;
  }
  int64_t n = operation(102, file->path, file->position, (void *)bytes,
                        size * count, 0);
  if (n < 0) {
    file->error = 1;
    return 0;
  }
  file->position += (uint64_t)n;
  return (size_t)n / size;
}
static int guest_fgetc(GuestFile *file) {
  uint8_t byte;
  return guest_fread(&byte, 1, 1, file) == 1 ? byte : EOF;
}
static int guest_fputc(int byte, GuestFile *file) {
  uint8_t value = (uint8_t)byte;
  return guest_fwrite(&value, 1, 1, file) == 1 ? value : EOF;
}
static int guest_fseek(GuestFile *file, int64_t offset, int origin) {
  int64_t base = 0;
  if (origin == SEEK_CUR)
    base = (int64_t)file->position;
  else if (origin == SEEK_END) {
    base = operation(103, file->path, 0, NULL, 0, 0);
    if (base < 0)
      return -1;
  } else if (origin != SEEK_SET) {
    errno = EINVAL;
    return -1;
  }
  if ((offset < 0 && base < -offset) ||
      (offset > 0 && base > INT64_MAX - offset)) {
    errno = EINVAL;
    return -1;
  }
  file->position = (uint64_t)(base + offset);
  file->eof = 0;
  return 0;
}
static int64_t guest_ftell(GuestFile *file) { return (int64_t)file->position; }
static void guest_rewind(GuestFile *file) {
  guest_fseek(file, 0, SEEK_SET);
  file->error = 0;
}
static int guest_fflush(GuestFile *file) {
  (void)file;
  return 0;
} /* VFS writes commit synchronously. */
static int guest_fclose(GuestFile *file) {
  if (!file) {
    errno = EINVAL;
    return EOF;
  }
  free(file->path);
  free(file);
  return 0;
}
static int guest_feof(GuestFile *file) { return file->eof; }
static int guest_ferror(GuestFile *file) { return file->error; }
uint64_t as_bionic_io_import(const char *name) {
  struct {
    const char *name;
    void *function;
  } imports[] = {{"fopen", guest_fopen},    {"fopen64", guest_fopen},
                 {"fclose", guest_fclose},  {"fread", guest_fread},
                 {"fwrite", guest_fwrite},  {"fgetc", guest_fgetc},
                 {"getc", guest_fgetc},     {"fputc", guest_fputc},
                 {"putc", guest_fputc},     {"fseek", guest_fseek},
                 {"fseeko", guest_fseek},   {"fseeko64", guest_fseek},
                 {"ftell", guest_ftell},    {"ftello", guest_ftell},
                 {"ftello64", guest_ftell}, {"rewind", guest_rewind},
                 {"fflush", guest_fflush},  {"feof", guest_feof},
                 {"ferror", guest_ferror}};
  for (size_t i = 0; i < sizeof(imports) / sizeof(imports[0]); i++)
    if (!strcmp(name, imports[i].name))
      return (uint64_t)imports[i].function;
  return 0;
}
