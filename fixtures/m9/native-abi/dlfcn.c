/* Linked libdl provider contracts, independent of any APK. MIT OR Apache-2.0. */
#include "bionic_dlfcn.h"
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
static pthread_mutex_t mutex=PTHREAD_MUTEX_INITIALIZER;
void as_cxx_lock(void) { assert(!pthread_mutex_lock(&mutex)); }
void as_cxx_unlock(void) { assert(!pthread_mutex_unlock(&mutex)); }
static int allocations;
uint64_t as_native_veneer(uint64_t function) { allocations++;return function+0x1000; }
int as_native_library(const char *name) { return !strcmp(name,"libEGL.so") || !strcmp(name,"libc.so"); }
uint64_t as_native_system_symbol(const char *library,const char *name,int *data) {
  *data=0;
  if ((!library||!strcmp(library,"libEGL.so")) && !strcmp(name,"eglGetDisplay")) return 0x2000;
  if ((!library||!strcmp(library,"libc.so")) && !strcmp(name,"guard")) { *data=1;return 0x3000; }
  return 0;
}
static char *(*error)(void);
static void *(*symbol)(void *,const char *);
static void *other_thread(void *arg) { (void)arg; assert(!error());assert(!symbol(NULL,"absent-thread"));assert(strstr(error(),"absent-thread"));assert(!error());return NULL; }
int main(void) {
  void *(*open)(const char *,int)=(void *)as_bionic_dlfcn_import("dlopen");
  int (*close)(void *)=(void *)as_bionic_dlfcn_import("dlclose");
  symbol=(void *)as_bionic_dlfcn_import("dlsym");error=(void *)as_bionic_dlfcn_import("dlerror");
  assert(open&&symbol&&close&&error);
  assert(!open("not-a-library.so",2));assert(strstr(error(),"no linked system provider"));assert(!error());
  assert(!open("libc.so",0));assert(error());
  void *egl=open("libEGL.so",2);assert(egl);
  assert(open("/system/lib64/libEGL.so",2)==egl);
  assert(symbol(egl,"eglGetDisplay")== (void *)0x3000);
  assert(symbol(egl,"eglGetDisplay")== (void *)0x3000 && allocations==1);
  assert(!symbol(egl,"guard"));assert(error());
  void *c=open("libc.so",2);assert(c);
  assert(symbol(c,"guard")== (void *)0x3000 && allocations==1);
  assert(!symbol((void *)42,"guard"));assert(error());
  assert(!symbol((void *)(intptr_t)-1,"guard"));assert(strstr(error(),"RTLD_NEXT"));
  assert(!symbol(NULL,"main-error"));
  pthread_t thread;assert(!pthread_create(&thread,NULL,other_thread,NULL));assert(!pthread_join(thread,NULL));
  assert(strstr(error(),"main-error"));
  assert(!close(egl));assert(symbol(egl,"eglGetDisplay"));assert(!close(egl));
  assert(!symbol(egl,"eglGetDisplay"));assert(error());assert(close(egl)==-1);assert(error());
  assert(open("libEGL.so",2)!=egl); // Stale handle is never revived.
  assert(!close(c));
  as_bionic_dlfcn_reset();assert(!error());
  puts("libdl scoped function/data pointers, TLS errors, references, stale handles: PASS");return 0;
}
