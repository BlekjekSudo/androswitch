/* Inspect the SDK actually used by the standalone probe, not Android/Bionic. */
#include <sys/types.h>
#include <sys/stat.h>
#include <dirent.h>
#include <fcntl.h>
#include <time.h>
#include <stddef.h>
#include <stdint.h>
#include <pthread.h>

const uint64_t as_sdk_abi[] = {
    sizeof(dev_t), sizeof(ino_t), sizeof(uid_t), sizeof(gid_t),
    sizeof(blksize_t), sizeof(blkcnt_t), sizeof(clockid_t),
    sizeof(struct stat), _Alignof(struct stat),
    offsetof(struct stat, st_size), offsetof(struct stat, st_atim),
    offsetof(struct stat, st_blksize), sizeof(struct dirent),
    offsetof(struct dirent, d_name), CLOCK_REALTIME, CLOCK_MONOTONIC,
    O_CREAT, O_EXCL, O_TRUNC, O_APPEND, AT_FDCWD,
    sizeof(pthread_attr_t), _Alignof(pthread_attr_t),
    sizeof(pthread_mutex_t), _Alignof(pthread_mutex_t),
    sizeof(pthread_mutexattr_t), _Alignof(pthread_mutexattr_t),
    sizeof(pthread_cond_t), _Alignof(pthread_cond_t),
    sizeof(pthread_condattr_t), _Alignof(pthread_condattr_t),
    sizeof(pthread_t), _Alignof(pthread_t)
};
