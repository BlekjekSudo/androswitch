/* Standalone std PAL gaps. All other OS calls use the SDK implementations. */
#include <switch.h>
#include <errno.h>
#include <reent.h>
#include <unistd.h>
#include <pthread.h>

/* Observe the SDK result that release std's JoinHandle::drop cannot report.
 * Reap an unsupported detach only after the probe worker completed, so this
 * experiment itself does not retain an unjoined native thread. */
static Mutex detach_lock;
static pthread_t failed_detach;
static int detach_result = -999;
extern int __real_pthread_detach(pthread_t thread);
int __wrap_pthread_detach(pthread_t thread) {
    int result = __real_pthread_detach(thread);
    mutexLock(&detach_lock);
    detach_result = result;
    if (result) failed_detach = thread;
    mutexUnlock(&detach_lock);
    return result;
}
int as_std_detach_result(void) {
    mutexLock(&detach_lock);
    int result = detach_result;
    mutexUnlock(&detach_lock);
    return result;
}
int as_std_reap_failed_detach(void) {
    mutexLock(&detach_lock);
    pthread_t thread = failed_detach;
    failed_detach = NULL;
    mutexUnlock(&detach_lock);
    return thread ? pthread_join(thread, NULL) : 0;
}

long sysconf(int name) {
    /* Horizon's AArch64 VM page granule, also required by libnx thread stacks. */
    if (name == _SC_PAGESIZE) return 0x1000;
    errno = EINVAL;
    return -1;
}

int _getentropy_r(struct _reent *reent, void *buffer, size_t length) {
    if (length > 256) { reent->_errno = EIO; return -1; }
    Result rc = csrngGetRandomBytes(buffer, length);
    if (R_FAILED(rc)) { reent->_errno = EIO; return -1; }
    return 0;
}
