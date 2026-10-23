/* Ordinary Android NativeActivity APK. Build and run without AndroSwitch
 * headers, platform defines, package checks, or patched guest imports. */
#include <android/native_activity.h>
#include <android/asset_manager.h>
#include <android/input.h>
#include <android/looper.h>
#include <EGL/egl.h>
#include <GLES2/gl2.h>
#include <aaudio/AAudio.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static ANativeActivity *activity;
static ANativeWindow *window;
static AInputQueue *queue;
static pthread_t worker;
static atomic_int running, ready, input_count;
static int started, asset_ok, audio_ok, gl_ok, readback_ok;
static _Thread_local int thread_local_value;
static unsigned frames;
static char renderer[160];
static GLuint shader(GLenum type,const char *text){GLuint s=glCreateShader(type);glShaderSource(s,1,&text,NULL);glCompileShader(s);GLint ok;glGetShaderiv(s,GL_COMPILE_STATUS,&ok);return ok?s:0;}
static void report(void){
  char path[512];snprintf(path,sizeof(path),"%s/media.json",activity->internalDataPath);
  FILE *f=fopen(path,"w");if(f){fprintf(f,"{\"frames\":%u,\"input\":%d,\"asset_ok\":%d,\"audio_frames\":%d,\"gl_ok\":%d,\"readback_ok\":%d,\"tls\":%d,\"renderer\":\"%s\"}",frames,atomic_load(&input_count),asset_ok,audio_ok,gl_ok,readback_ok,thread_local_value,renderer);fclose(f);}
}
static void *render(void *unused){
  (void)unused;thread_local_value=73;
  ALooper *loop=ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS);
  AInputQueue_attachLooper(queue,loop,5,NULL,NULL);
  EGLDisplay d=eglGetDisplay(EGL_DEFAULT_DISPLAY);EGLint major,minor;
  if(!eglInitialize(d,&major,&minor))return NULL;
  const EGLint attributes[]={EGL_SURFACE_TYPE,EGL_WINDOW_BIT,EGL_RENDERABLE_TYPE,EGL_OPENGL_ES2_BIT,EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_ALPHA_SIZE,8,EGL_NONE};
  EGLConfig config;EGLint count;if(!eglChooseConfig(d,attributes,&config,1,&count)||count!=1)return NULL;
  EGLSurface surface=eglCreateWindowSurface(d,config,window,NULL);
  const EGLint context_attributes[]={EGL_CONTEXT_CLIENT_VERSION,2,EGL_NONE};
  EGLContext context=eglCreateContext(d,config,EGL_NO_CONTEXT,context_attributes);
  if(!surface||!context||!eglMakeCurrent(d,surface,surface,context))return NULL;
  snprintf(renderer,sizeof(renderer),"%s",glGetString(GL_RENDERER));
  GLuint vs=shader(GL_VERTEX_SHADER,"attribute vec2 position;void main(){gl_Position=vec4(position,0.,1.);}");
  GLuint fs=shader(GL_FRAGMENT_SHADER,"precision mediump float;void main(){gl_FragColor=vec4(1.,.8,.1,1.);}");
  GLuint program=glCreateProgram();glAttachShader(program,vs);glAttachShader(program,fs);glBindAttribLocation(program,0,"position");glLinkProgram(program);GLint linked=0;glGetProgramiv(program,GL_LINK_STATUS,&linked);glUseProgram(program);
  float triangle[]={-.7f,-.6f,.7f,-.6f,0.f,.7f};glVertexAttribPointer(0,2,GL_FLOAT,GL_FALSE,0,triangle);glEnableVertexAttribArray(0);
  AAudioStreamBuilder *builder=NULL;AAudioStream *audio=NULL;
  if(AAudio_createStreamBuilder(&builder)==AAUDIO_OK){AAudioStreamBuilder_setSampleRate(builder,48000);AAudioStreamBuilder_setChannelCount(builder,2);AAudioStreamBuilder_setFormat(builder,AAUDIO_FORMAT_PCM_I16);
    if(AAudioStreamBuilder_openStream(builder,&audio)==AAUDIO_OK && AAudioStream_requestStart(audio)==AAUDIO_OK){int16_t pcm[2400*2];for(int i=0;i<2400;i++)pcm[i*2]=pcm[i*2+1]=(i%96<48?1800:-1800);for(int n=0;n<5;n++){int wrote=AAudioStream_write(audio,pcm,2400,1000000000);if(wrote>0)audio_ok+=wrote;}}
    if(audio){AAudioStream_requestStop(audio);AAudioStream_close(audio);}AAudioStreamBuilder_delete(builder);}
  atomic_store(&ready,1);
  while(atomic_load(&running)){
    if(ALooper_pollOnce(0,NULL,NULL,NULL)==5){AInputEvent *event;while(AInputQueue_getEvent(queue,&event)>=0){if(!AInputQueue_preDispatchEvent(queue,event)){atomic_fetch_add(&input_count,1);AInputQueue_finishEvent(queue,event,1);}}}
    glViewport(0,0,1280,720);glClearColor(atomic_load(&input_count)?0.6f:0.05f,0.15f,0.35f,1.f);glClear(GL_COLOR_BUFFER_BIT);
    float drift=((int)(frames%120)-60)/500.f;triangle[0]=-.7f+drift;triangle[2]=.7f+drift;triangle[4]=drift;
    glDrawArrays(GL_TRIANGLES,0,3);
    if(frames==1){unsigned char *image=malloc(1280*720*4);if(image){glReadPixels(0,0,1280,720,GL_RGBA,GL_UNSIGNED_BYTE,image);size_t center=(360*1280+640)*4;readback_ok=image[center]>240&&image[center+1]>180&&image[center+2]<80&&image[(100*1280+1279)*4+2]>70;free(image);}}
    unsigned char pixel[4]={0};glReadPixels(640,360,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);gl_ok=linked&&readback_ok&&pixel[0]>240&&pixel[1]>180&&pixel[2]<80&&glGetError()==GL_NO_ERROR;
    if(!eglSwapBuffers(d,surface))break;
    frames++;if(frames%10==0)report();
  }
  report();glDeleteProgram(program);glDeleteShader(vs);glDeleteShader(fs);
  eglMakeCurrent(d,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);eglDestroyContext(d,context);eglDestroySurface(d,surface);eglTerminate(d);
  AInputQueue_detachLooper(queue);return NULL;
}
static void window_created(ANativeActivity *a,ANativeWindow *w){(void)a;window=w;ANativeWindow_acquire(w);atomic_store(&running,1);started=pthread_create(&worker,NULL,render,NULL)==0;}
static void window_destroyed(ANativeActivity *a,ANativeWindow *w){(void)a;atomic_store(&running,0);if(started)pthread_join(worker,NULL);started=0;ANativeWindow_release(w);}
static void queue_created(ANativeActivity *a,AInputQueue *q){(void)a;queue=q;}
void ANativeActivity_onCreate(ANativeActivity *a,void *saved,size_t size){
  (void)saved;(void)size;activity=a;thread_local_value=17;
  AAsset *asset=AAssetManager_open(a->assetManager,"stream.bin",AASSET_MODE_STREAMING);
  if(asset){unsigned char bytes[137];int n;unsigned total=0;int valid=1;while((n=AAsset_read(asset,bytes,sizeof(bytes)))>0){for(int i=0;i<n;i++)if(bytes[i]!=(total+i)%251)valid=0;total+=(unsigned)n;}
    unsigned char b=0;asset_ok=valid&&total==150000&&AAsset_getLength64(asset)==150000&&AAsset_seek64(asset,42,SEEK_SET)==42&&AAsset_read(asset,&b,1)==1&&b==42;AAsset_close(asset);}
  a->callbacks->onNativeWindowCreated=window_created;a->callbacks->onNativeWindowDestroyed=window_destroyed;a->callbacks->onInputQueueCreated=queue_created;
}
