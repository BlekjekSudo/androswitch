/* Horizon PAL for Rust std. SDK pthread creation/join and TLS remain real.
 * Detach adds resource reclamation missing from libnx/newlib. The first field
 * of the pinned libnx __pthread_t is its public Thread (newlib.c); that SDK
 * layout is used only to poll completion before performing real pthread_join.
 * SDK hashes and ABI checks accompany builds. No guest Bionic pointers enter
 * this host runtime. */
#include "support.h"
#include <switch.h>
#include <errno.h>
#include <pthread.h>
#include <reent.h>
#include <stdlib.h>
#include <unistd.h>

#define STD_THREADS 256
typedef struct {
    pthread_t id;
    void *(*entry)(void *);
    void *arg;
    bool used, published, detached, joining;
} StdThread;
static StdThread threads[STD_THREADS];
static Mutex lock;
static CondVar changed;
static Thread reaper;
static bool initialized, stopping;
static int detach_result = -999, reap_error;
static unsigned reaped;
extern int __real_pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
extern int __real_pthread_join(pthread_t, void **);
extern int __real_pthread_attr_init(pthread_attr_t *);
int __wrap_pthread_attr_init(pthread_attr_t *attr) {
    int rc=__real_pthread_attr_init(attr);
    /* SDK init zeroes detachstate, but this SDK's DETACHED constant is zero.
     * POSIX requires the default attribute to create a joinable thread. */
    if(!rc) attr->detachstate=PTHREAD_CREATE_JOINABLE;
    return rc;
}

static unsigned pending_locked(bool detached_only) {
    unsigned count = 0;
    for (unsigned i=0; i<STD_THREADS; i++)
        if (threads[i].used && (!detached_only || threads[i].detached)) count++;
    return count;
}
static void reap_main(void *unused) {
    (void)unused;
    mutexLock(&lock);
    for (;;) {
        StdThread *finished = NULL;
        for (unsigned i=0; i<STD_THREADS; i++) {
            StdThread *item = &threads[i];
            if (!item->used || !item->published || !item->detached || item->joining) continue;
            /* SDK pthread_t contains Thread as its first member. Never close
             * the running thread's own stack; only the reaper joins it. */
            Thread *native = (Thread *)item->id;
            s32 index = 0;
            Result rc = svcWaitSynchronization(&index, &native->handle, 1, 0);
            if (R_SUCCEEDED(rc)) { item->joining=true; finished=item; break; }
            if (rc != KERNELRESULT(TimedOut)) reap_error=EIO;
        }
        if (finished) {
            pthread_t id = finished->id;
            mutexUnlock(&lock);
            int result = __real_pthread_join(id, NULL);
            mutexLock(&lock);
            if (!result) { finished->used=false; reaped++; }
            else { finished->joining=false; reap_error=result; }
            condvarWakeAll(&changed);
            continue;
        }
        unsigned pending = pending_locked(true);
        if (stopping && !pending) { mutexUnlock(&lock); return; }
        condvarWaitTimeout(&changed, &lock, pending ? 2000000 : UINT64_MAX);
    }
}
int as_horizon_std_init(void) {
    mutexLock(&lock);
    if (initialized) { int rc=stopping ? EBUSY : 0; mutexUnlock(&lock); return rc; }
    Result rc = csrngInitialize();
    if (R_FAILED(rc)) { mutexUnlock(&lock); return EIO; }
    rc = threadCreate(&reaper, reap_main, NULL, NULL, 64*1024, 0x3B, -2);
    if (R_FAILED(rc)) { csrngExit(); mutexUnlock(&lock); return EAGAIN; }
    rc = threadStart(&reaper);
    if (R_FAILED(rc)) { threadClose(&reaper); csrngExit(); mutexUnlock(&lock); return EAGAIN; }
    initialized=true;
    mutexUnlock(&lock);
    return 0;
}
static void *std_entry(void *arg) {
    StdThread *item = arg;
    mutexLock(&lock);
    item->id=pthread_self();
    void *(*entry)(void *)=item->entry;
    void *parameter=item->arg;
    mutexUnlock(&lock);
    return entry(parameter);
}
int __wrap_pthread_create(pthread_t *id, const pthread_attr_t *attr, void *(*entry)(void *), void *arg) {
    if (!id || !entry) return EINVAL;
    int rc=as_horizon_std_init();
    if (rc) return rc;
    pthread_attr_t actual;
    if (attr) actual=*attr;
    else if ((rc=pthread_attr_init(&actual))) return rc;
    bool detached=actual.detachstate==PTHREAD_CREATE_DETACHED;
    actual.detachstate=PTHREAD_CREATE_JOINABLE;
    mutexLock(&lock);
    StdThread *item=NULL;
    if (!stopping) for (unsigned i=0;i<STD_THREADS;i++) if (!threads[i].used) {item=&threads[i];break;}
    if (!item) {mutexUnlock(&lock);return EAGAIN;}
    *item=(StdThread){.entry=entry,.arg=arg,.used=true,.detached=detached};
    mutexUnlock(&lock);
    rc=__real_pthread_create(id,&actual,std_entry,item);
    mutexLock(&lock);
    if (rc) item->used=false;
    else {item->id=*id;item->published=true;}
    condvarWakeAll(&changed);
    mutexUnlock(&lock);
    return rc;
}
int __wrap_pthread_join(pthread_t id, void **value) {
    if (id==pthread_self()) return EDEADLK;
    mutexLock(&lock);
    StdThread *item=NULL;
    for(unsigned i=0;i<STD_THREADS;i++) if(threads[i].used && threads[i].id==id){item=&threads[i];break;}
    if (!item || item->detached || item->joining) {mutexUnlock(&lock);return EINVAL;}
    item->joining=true;
    mutexUnlock(&lock);
    int rc=__real_pthread_join(id,value);
    mutexLock(&lock);
    if(!rc) item->used=false;
    else item->joining=false;
    condvarWakeAll(&changed);
    mutexUnlock(&lock);
    return rc;
}
int __wrap_pthread_detach(pthread_t id) {
    mutexLock(&lock);
    StdThread *item=NULL;
    for(unsigned i=0;i<STD_THREADS;i++) if(threads[i].used && threads[i].id==id){item=&threads[i];break;}
    int rc=(!item || item->detached || item->joining) ? EINVAL : 0;
    if(!rc) item->detached=true;
    detach_result=rc;
    condvarWakeAll(&changed);
    mutexUnlock(&lock);
    return rc;
}
int as_std_reaper_drain(unsigned timeout_ms) {
    u64 start=armGetSystemTick();
    mutexLock(&lock);
    while (pending_locked(true)) {
        if (reap_error) {int rc=reap_error;mutexUnlock(&lock);return rc;}
        if (armTicksToNs(armGetSystemTick()-start)/1000000 >= timeout_ms) {mutexUnlock(&lock);return ETIMEDOUT;}
        condvarWaitTimeout(&changed,&lock,2000000);
    }
    mutexUnlock(&lock);return 0;
}
int as_horizon_std_shutdown(unsigned timeout_ms) {
    int rc=as_std_reaper_drain(timeout_ms);
    if(rc) return rc;
    mutexLock(&lock);
    if(!initialized){mutexUnlock(&lock);return 0;}
    if(pending_locked(false)){mutexUnlock(&lock);return EBUSY;}
    stopping=true;condvarWakeAll(&changed);mutexUnlock(&lock);
    if(R_FAILED(threadWaitForExit(&reaper))) return EIO;
    if(R_FAILED(threadClose(&reaper))) return EIO;
    csrngExit();mutexLock(&lock);initialized=false;stopping=false;mutexUnlock(&lock);return 0;
}
int as_std_detach_result(void) {mutexLock(&lock);int rc=detach_result;mutexUnlock(&lock);return rc;}
unsigned as_std_reaped_threads(void) {mutexLock(&lock);unsigned n=reaped;mutexUnlock(&lock);return n;}
unsigned as_std_live_threads(void) {mutexLock(&lock);unsigned n=pending_locked(false);mutexUnlock(&lock);return n;}
long as_std_process_core_count(void) {
    u64 mask=0;
    if(R_FAILED(svcGetInfo(&mask,InfoType_CoreMask,CUR_PROCESS_HANDLE,0)) || !mask){errno=EIO;return -1;}
    return __builtin_popcountll(mask);
}
long sysconf(int name) {
    if(name==_SC_PAGESIZE) return 0x1000;
    if(name==_SC_NPROCESSORS_ONLN || name==_SC_NPROCESSORS_CONF) return as_std_process_core_count();
    errno=EINVAL;return -1;
}
int _getentropy_r(struct _reent *reent,void *buffer,size_t length) {
    if(length>256){reent->_errno=EIO;return -1;}
    if(R_FAILED(csrngGetRandomBytes(buffer,length))){reent->_errno=EIO;return -1;}
    return 0;
}
