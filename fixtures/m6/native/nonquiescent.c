/* Negative lifecycle fixture: an app that never stops its native worker must
 * cause a process exit, never an unsafe return to the launcher. */
#include <android/native_activity.h>
#include <pthread.h>
#include <unistd.h>
static void *forever(void *arg){(void)arg;for(;;)usleep(1000);return NULL;}
void ANativeActivity_onCreate(ANativeActivity *activity,void *state,size_t size){(void)activity;(void)state;(void)size;pthread_t thread;pthread_create(&thread,NULL,forever,NULL);}
