/* Android FILE is opaque to NDK callers. All bytes go through the VM VFS.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "bionic_cxx.h"
#include "bionic_errno.h"
#include "bionic_vfs.h"
#include "native.h"
#include <errno.h>
#include <limits.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern int64_t as_guest_fs(uint32_t, const char *, uint64_t, uint8_t *, size_t,
                           uint32_t);
typedef struct GuestFile {
  char *path;
  uint64_t position;
  int readable, writable, eof, error, descriptor, append;
  struct GuestFile *next;
} GuestFile;
static GuestFile *guest_fopen(const char *, const char *);
static GuestFile *guest_freopen(const char *, const char *, GuestFile *);
static int guest_fclose(GuestFile *);
static size_t guest_fread(void *, size_t, size_t, GuestFile *);
static size_t guest_fwrite(const void *, size_t, size_t, GuestFile *);
static int guest_fseek(GuestFile *, int64_t, int);
static int64_t guest_ftell(GuestFile *);
static int next_descriptor = 3;
/* API 26 Bionic's opaque LP64 FILE stride is 152 bytes. Legacy NDK headers
 * form &__sF[1/2] directly, while API 23+ imports pointer-valued stdout/stderr.
 * These are identity tokens only: no Bionic FILE is cast to a newlib FILE.
 * ABI reference: platform/bionic android-8.0.0_r1 libc/stdio/local.h and NDK
 * bits/struct_file.h. No implementation code is copied from Bionic. */
static _Alignas(8) unsigned char standard_tokens[3][152];
static void *standard_pointers[3] = {standard_tokens[0], standard_tokens[1],
                                     standard_tokens[2]};
static int standard_closed[3], standard_errors[3];
static GuestFile *standard_redirect[3];
static int standard_index(const void *file) {
  for (int i = 0; i < 3; ++i)
    if (file == standard_tokens[i])
      return i;
  return -1;
}
static GuestFile *redirected(GuestFile *file) {
  int standard = standard_index(file);
  return standard >= 0 && standard_redirect[standard]
             ? standard_redirect[standard]
             : file;
}
static GuestFile *files;
static int64_t operation(uint32_t op, const char *path, uint64_t offset,
                         void *bytes, size_t length, uint32_t flags) {
  as_jni_vm_lock();
  int64_t rc = as_guest_fs(op, path, offset, bytes, length, flags);
  as_jni_vm_unlock();
  if (rc < 0)
    errno = as_errno_from_android((int)-rc);
  return rc;
}
static GuestFile *guest_fopen_impl(const char *path, const char *mode) {
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
  file->append = mode[0] == 'a';
  as_cxx_lock();
  if (next_descriptor == INT_MAX) {
    as_cxx_unlock();
    free(file->path);
    free(file);
    errno = EMFILE;
    return NULL;
  }
  file->descriptor = next_descriptor++;
  file->next = files;
  files = file;
  as_cxx_unlock();
  return file;
}
static size_t guest_fread_impl(void *bytes, size_t size, size_t count,
                          GuestFile *file) {
  file = redirected(file);
  if (!size || !count)
    return 0;
  int standard = standard_index(file);
  if (standard >= 0) {
    /* No terminal/IME byte input is implemented. Do not fabricate bytes. */
    standard_errors[standard] = 1;
    errno = standard_closed[standard] || standard ? EBADF : ENOTSUP;
    return 0;
  }
  if (file->descriptor < 0 || !file->readable || count > SIZE_MAX / size) {
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
static size_t guest_fwrite_impl(const void *bytes, size_t size, size_t count,
                           GuestFile *file) {
  file = redirected(file);
  if (!size || !count)
    return 0;
  int standard = standard_index(file);
  if (standard >= 0) {
    if (!standard || standard_closed[standard] || count > SIZE_MAX / size) {
      standard_errors[standard] = 1;
      errno = EBADF;
      return 0;
    }
    size_t wrote = fwrite(bytes, size, count, standard == 1 ? stdout : stderr);
    if (wrote < count)
      standard_errors[standard] = 1;
    return wrote;
  }
  if (file->descriptor < 0 || !file->writable || count > SIZE_MAX / size) {
    file->error = 1;
    errno = EBADF;
    return 0;
  }
  if (file->append) {
    int64_t end = operation(103, file->path, 0, NULL, 0, 0);
    if (end < 0) { file->error = 1; return 0; }
    file->position = (uint64_t)end;
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
static int guest_fseek_impl(GuestFile *file, int64_t offset, int origin) {
  file = redirected(file);
  if (standard_index(file) >= 0) {
    errno = ESPIPE;
    return -1;
  }
  if (file->descriptor < 0) { errno = EBADF; return -1; }
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
  if ((offset < 0 && (uint64_t)base < (uint64_t)(-(offset + 1)) + 1) ||
      (offset > 0 && base > INT64_MAX - offset)) {
    errno = EINVAL;
    return -1;
  }
  file->position = (uint64_t)(base + offset);
  file->eof = 0;
  return 0;
}
static int64_t guest_ftell_impl(GuestFile *file) {
  file = redirected(file);
  if (standard_index(file) >= 0) {
    errno = ESPIPE;
    return -1;
  }
  if (file->descriptor < 0) { errno = EBADF; return -1; }
  return (int64_t)file->position;
}
static void guest_rewind(GuestFile *file) {
  file = redirected(file);
  guest_fseek(file, 0, SEEK_SET);
  int standard = standard_index(file);
  if (standard >= 0)
    standard_errors[standard] = 0;
  else
    file->error = 0;
}
static int guest_fflush(GuestFile *file) {
  file = redirected(file);
  int standard = standard_index(file);
  if (!file)
    return fflush(NULL);
  if (standard >= 0)
    return standard_closed[standard] ? (errno = EBADF, EOF)
                                     : fflush(standard == 1 ? stdout : stderr);
  return 0;
} /* VFS writes commit synchronously. */
static int guest_fclose_impl(GuestFile *file) {
  if (!file) {
    errno = EINVAL;
    return EOF;
  }
  int standard = standard_index(file);
  if (standard >= 0) {
    if (standard_closed[standard]) {
      errno = EBADF;
      return EOF;
    }
    int rc = standard_redirect[standard]
                 ? guest_fclose(standard_redirect[standard])
                 : guest_fflush(file);
    standard_redirect[standard] = NULL;
    standard_closed[standard] = 1;
    return rc;
  }
  as_cxx_lock();
  GuestFile **link = &files;
  while (*link && *link != file)
    link = &(*link)->next;
  if (*link)
    *link = file->next;
  as_cxx_unlock();
  int closed = file->descriptor < 0;
  free(file->path);
  free(file);
  if (closed) { errno = EBADF; return EOF; }
  return 0;
}
static GuestFile *guest_freopen_impl(const char *path, const char *mode,
                                GuestFile *file) {
  if (!file) {
    errno = EINVAL;
    return NULL;
  }
  if (!path) {
    errno = ENOSYS;
    return NULL;
  }
  GuestFile *opened = guest_fopen(path, mode);
  if (!opened) {
    guest_fclose(file);
    return NULL;
  }
  int standard = standard_index(file);
  if (standard >= 0) {
    if (standard_redirect[standard])
      guest_fclose(standard_redirect[standard]);
    standard_redirect[standard] = opened;
    standard_closed[standard] = standard_errors[standard] = 0;
    return file;
  }
  as_cxx_lock();
  GuestFile **link = &files;
  while (*link && *link != opened)
    link = &(*link)->next;
  if (*link)
    *link = opened->next;
  char *old_path = file->path;
  GuestFile *next = file->next;
  *file = *opened;
  file->next = next;
  as_cxx_unlock();
  free(old_path);
  free(opened);
  return file;
}
void as_bionic_io_reset(void) {
  next_descriptor = 3;
  memset(standard_redirect, 0, sizeof(standard_redirect));
  memset(standard_closed, 0, sizeof(standard_closed));
  memset(standard_errors, 0, sizeof(standard_errors));
  while (files) {
    GuestFile *file = files;
    files = file->next;
    free(file->path);
    free(file);
  }
}
static int guest_feof(GuestFile *file) {
  file = redirected(file);
  return standard_index(file) >= 0 ? 0 : file->eof;
}
static int guest_ferror(GuestFile *file) {
  file = redirected(file);
  int standard = standard_index(file);
  return standard >= 0 ? standard_errors[standard] : file->error;
}
static void guest_clearerr(GuestFile *file) {
  file = redirected(file);
  int standard = standard_index(file);
  if (standard >= 0)
    standard_errors[standard] = 0;
  else
    file->eof = file->error = 0;
}
static int guest_vfprintf(GuestFile *file, const char *format, va_list args) {
  va_list copy;
  va_copy(copy, args);
  int length = vsnprintf(NULL, 0, format, copy);
  va_end(copy);
  if (length < 0 || length > 1 << 20) {
    errno = EOVERFLOW;
    return -1;
  }
  char *bytes = malloc((size_t)length + 1);
  if (!bytes) {
    errno = ENOMEM;
    return -1;
  }
  vsnprintf(bytes, (size_t)length + 1, format, args);
  size_t wrote = guest_fwrite(bytes, 1, length, file);
  free(bytes);
  return wrote == (size_t)length ? length : -1;
}
static int guest_fprintf(GuestFile *file, const char *format, ...) {
  va_list args;
  va_start(args, format);
  int n = guest_vfprintf(file, format, args);
  va_end(args);
  return n;
}
static int guest_printf(const char *format, ...) {
  va_list args;
  va_start(args, format);
  int n = guest_vfprintf(standard_pointers[1], format, args);
  va_end(args);
  return n;
}
static int guest_vprintf(const char *format, va_list args) {
  return guest_vfprintf(standard_pointers[1], format, args);
}
static int guest_fputs(const char *text, GuestFile *file) {
  size_t length = strlen(text);
  return guest_fwrite(text, 1, length, file) == length ? 0 : EOF;
}
static int guest_puts(const char *text) {
  return guest_fputs(text, standard_pointers[1]) == EOF ||
                 guest_fputc('\n', standard_pointers[1]) == EOF
             ? EOF
             : 0;
}
static int guest_putchar(int byte) {
  return guest_fputc(byte, standard_pointers[1]);
}
uint64_t as_bionic_io_data(const char *name) {
  if (!strcmp(name, "__sF"))
    return (uint64_t)standard_tokens;
  for (int i = 0; i < 3; ++i)
    if (!strcmp(name, i == 0 ? "stdin" : i == 1 ? "stdout" : "stderr"))
      return (uint64_t)&standard_pointers[i];
  return 0;
}
#include "bionic_descriptors.inc"
uint64_t as_bionic_io_import(const char *name) {
  struct {
    const char *name;
    void *function;
  } imports[] = {{"fileno", guest_fileno}, {"fileno_unlocked", guest_fileno},
                 {"read", guest_read}, {"write", guest_write},
                 {"pread", guest_pread}, {"pread64", guest_pread},
                 {"pwrite", guest_pwrite}, {"pwrite64", guest_pwrite},
                 {"lseek", guest_lseek}, {"lseek64", guest_lseek},
                 {"fstat", guest_fstat}, {"fstat64", guest_fstat},
                 {"close", guest_close},
                 {"fopen", guest_fopen},     {"fopen64", guest_fopen},
                 {"fclose", guest_fclose},   {"freopen", guest_freopen},
                 {"fread", guest_fread},     {"fwrite", guest_fwrite},
                 {"fgetc", guest_fgetc},     {"getc", guest_fgetc},
                 {"fputc", guest_fputc},     {"putc", guest_fputc},
                 {"fseek", guest_fseek},     {"fseeko", guest_fseek},
                 {"fseeko64", guest_fseek},  {"ftell", guest_ftell},
                 {"ftello", guest_ftell},    {"ftello64", guest_ftell},
                 {"rewind", guest_rewind},   {"fflush", guest_fflush},
                 {"feof", guest_feof},       {"ferror", guest_ferror},
                 {"fprintf", guest_fprintf}, {"vfprintf", guest_vfprintf},
                 {"printf", guest_printf},   {"vprintf", guest_vprintf},
                 {"puts", guest_puts},       {"putchar", guest_putchar},
                 {"fputs", guest_fputs},     {"clearerr", guest_clearerr}};
  for (size_t i = 0; i < sizeof(imports) / sizeof(imports[0]); i++)
    if (!strcmp(name, imports[i].name))
      return (uint64_t)imports[i].function;
  return 0;
}
