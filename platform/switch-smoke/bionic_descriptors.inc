/* FILE and descriptor calls share one unbuffered VFS position and lifetime.
 * All lookup/I/O/close is serialized by the recursive VM lease; native metadata
 * locks are never held while acquiring that lease. No newlib FD is exposed.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#define LEASE(ret, name, params, args) \
  static ret name params {             \
    as_jni_vm_lock();                  \
    ret as_result = name##_impl args;  \
    as_jni_vm_unlock();                \
    return as_result;                  \
  }
LEASE(GuestFile *, guest_fopen, (const char *path, const char *mode),
      (path, mode))
LEASE(GuestFile *, guest_freopen,
      (const char *path, const char *mode, GuestFile *file), (path, mode, file))
LEASE(int, guest_fclose, (GuestFile * file), (file))
LEASE(size_t, guest_fread,
      (void *bytes, size_t size, size_t count, GuestFile *file),
      (bytes, size, count, file))
LEASE(size_t, guest_fwrite,
      (const void *bytes, size_t size, size_t count, GuestFile *file),
      (bytes, size, count, file))
LEASE(int, guest_fseek, (GuestFile * file, int64_t offset, int origin),
      (file, offset, origin))
LEASE(int64_t, guest_ftell, (GuestFile * file), (file))

static GuestFile *descriptor_file(int fd) {
  if (fd >= 0 && fd < 3) {
    if (!standard_closed[fd])
      return redirected((GuestFile *)standard_tokens[fd]);
  } else if (fd >= 3) {
    for (GuestFile *file = files; file; file = file->next)
      if (file->descriptor == fd) return file;
  }
  errno = EBADF;
  return NULL;
}
static int guest_fileno_impl(GuestFile *file) {
  int standard = standard_index(file);
  if (standard >= 0 && !standard_closed[standard]) return standard;
  for (GuestFile *p = files; p; p = p->next)
    if (p == file && p->descriptor >= 0) return p->descriptor;
  errno = EBADF;
  return -1;
}
LEASE(int, guest_fileno, (GuestFile * file), (file))
static int guest_fsync_impl(int fd) {
  GuestFile *file = descriptor_file(fd);
  if (!file) return -1;
  if (standard_index(file) >= 0) {
    errno = EINVAL;
    return -1;
  }
  return operation(108, file->path, 0, NULL, 0, 0) < 0 ? -1 : 0;
}
LEASE(int, guest_fsync, (int fd), (fd))
static int64_t descriptor_io(int fd, void *bytes, size_t count, int write,
                             int positioned, int64_t offset) {
  GuestFile *file = descriptor_file(fd);
  if (!file) return -1;
  int standard = standard_index(file);
  if (standard >= 0) {
    if ((write && !standard) || (!write && standard)) {
      errno = EBADF;
      return -1;
    }
    if (positioned) {
      errno = ESPIPE;
      return -1;
    }
    if (!count) return 0;
    if (!bytes) {
      errno = EFAULT;
      return -1;
    }
    if (!write) {
      errno = ENOTSUP;
      return -1;
    }
    size_t n = fwrite(bytes, 1, count, standard == 1 ? stdout : stderr);
    return n ? (int64_t)n : -1;
  }
  if ((write && !file->writable) || (!write && !file->readable)) {
    errno = EBADF;
    return -1;
  }
  if (positioned && offset < 0) {
    errno = EINVAL;
    return -1;
  }
  if (!count) return 0;
  if (!bytes) {
    errno = EFAULT;
    return -1;
  }
  /* read/write may return a short count. Keep each VFS transfer bounded. */
  if (count > 16u << 20) count = 16u << 20;
  uint64_t position = positioned ? (uint64_t)offset : file->position;
  if (write && file->append) {
    /* Linux O_APPEND overrides pwrite's offset too. */
    int64_t end = operation(103, file->path, 0, NULL, 0, 0);
    if (end < 0) return -1;
    position = (uint64_t)end;
  }
  if (position > INT64_MAX || count > INT64_MAX - position) {
    errno = EOVERFLOW;
    return -1;
  }
  int64_t n =
      operation(write ? 102 : 101, file->path, position, bytes, count, 0);
  if (n < 0) return -1;
  if (!positioned) file->position = position + (uint64_t)n;
  return n;
}
static int64_t guest_read_impl(int fd, void *bytes, size_t count) {
  return descriptor_io(fd, bytes, count, 0, 0, 0);
}
static int64_t guest_write_impl(int fd, const void *bytes, size_t count) {
  return descriptor_io(fd, (void *)bytes, count, 1, 0, 0);
}
static int64_t guest_pread_impl(int fd, void *bytes, size_t count,
                                int64_t offset) {
  return descriptor_io(fd, bytes, count, 0, 1, offset);
}
static int64_t guest_pwrite_impl(int fd, const void *bytes, size_t count,
                                 int64_t offset) {
  return descriptor_io(fd, (void *)bytes, count, 1, 1, offset);
}
LEASE(int64_t, guest_read, (int fd, void *bytes, size_t count),
      (fd, bytes, count))
LEASE(int64_t, guest_write, (int fd, const void *bytes, size_t count),
      (fd, bytes, count))
LEASE(int64_t, guest_pread, (int fd, void *bytes, size_t count, int64_t offset),
      (fd, bytes, count, offset))
LEASE(int64_t, guest_pwrite,
      (int fd, const void *bytes, size_t count, int64_t offset),
      (fd, bytes, count, offset))
static int64_t guest_lseek_impl(int fd, int64_t offset, int origin) {
  GuestFile *file = descriptor_file(fd);
  if (!file) return -1;
  if (standard_index(file) >= 0) {
    errno = ESPIPE;
    return -1;
  }
  int eof = file->eof;
  int rc = guest_fseek_impl(file, offset, origin);
  file->eof = eof; /* Raw FD operations do not change stdio indicators. */
  return rc ? -1 : (int64_t)file->position;
}
LEASE(int64_t, guest_lseek, (int fd, int64_t offset, int origin),
      (fd, offset, origin))
static int guest_fstat_impl(int fd, AsGuestStat *out) {
  GuestFile *file = descriptor_file(fd);
  if (!file) return -1;
  if (standard_index(file) >= 0) {
    errno = ENOTSUP;
    return -1;
  }
  return as_bionic_stat_path(file->path, out);
}
LEASE(int, guest_fstat, (int fd, AsGuestStat *out), (fd, out))
static int guest_close_impl(int fd) {
  GuestFile *file = descriptor_file(fd);
  if (!file) return -1;
  if (fd < 3) return guest_fclose_impl((GuestFile *)standard_tokens[fd]);
  /* FILE still owns its stream object. fclose later frees it and reports EBADF;
   * a closed descriptor is never reused within the same session. */
  file->descriptor = -1;
  return 0;
}
LEASE(int, guest_close, (int fd), (fd))
#undef LEASE
