/* Independent Android JNI/GLES2 fixture. Colors are defined in GL's logical
 * bottom-left coordinates: red, green, blue, yellow. The moving white bar and
 * touch square make fresh frames and inverse input mapping visible. */
#include <jni.h>
#include <EGL/egl.h>
#include <GLES2/gl2.h>
#include <stdatomic.h>
#include <stdio.h>
static char report_path[512];
static int surface_width, surface_height, callback_width, callback_height;
static unsigned frames, readback_mask;
static atomic_int touches, last_action, last_x, last_y;
#define JNI_NAME(name) Java_org_androswitch_graphics_portrait_PortraitActivity_##name
JNIEXPORT void JNICALL JNI_NAME(prepare)(JNIEnv *env,jclass cls,jstring files) {
    (void)cls;
    const char *directory=(*env)->GetStringUTFChars(env,files,NULL);
    if(!directory)return;
    snprintf(report_path,sizeof(report_path),"%s/portrait-proof.json",directory);
    (*env)->ReleaseStringUTFChars(env,files,directory);
}
JNIEXPORT jint JNICALL JNI_NAME(created)(JNIEnv *env,jclass cls) {
    (void)env;(void)cls;
    return eglGetCurrentContext()!=EGL_NO_CONTEXT&&eglGetCurrentDisplay()!=EGL_NO_DISPLAY?0:-1;
}
JNIEXPORT jint JNICALL JNI_NAME(changed)(JNIEnv *env,jclass cls,jint width,jint height) {
    (void)env;(void)cls;
    EGLDisplay display=eglGetCurrentDisplay();
    EGLSurface surface=eglGetCurrentSurface(EGL_DRAW);
    EGLint actual_width=0,actual_height=0;
    if(!eglQuerySurface(display,surface,EGL_WIDTH,&actual_width)||
       !eglQuerySurface(display,surface,EGL_HEIGHT,&actual_height))return -1;
    callback_width=width;callback_height=height;
    surface_width=actual_width;surface_height=actual_height;
    return width==actual_width&&height==actual_height&&width>0&&height>width?0:-2;
}
static void quadrant(int x,int y,int width,int height,float red,float green,float blue) {
    glScissor(x,y,width,height);glClearColor(red,green,blue,1.f);glClear(GL_COLOR_BUFFER_BIT);
}
static int report(void) {
    FILE *file=fopen(report_path,"w");
    if(!file)return -4;
    int result=fprintf(file,"{\"schema_version\":1,\"surface_width\":%d,\"surface_height\":%d,\"callback_width\":%d,\"callback_height\":%d,\"frames\":%u,\"readback_mask\":%u,\"touches\":%d,\"last_action\":%d,\"touch_x\":%d,\"touch_y\":%d}",surface_width,surface_height,callback_width,callback_height,frames,readback_mask,atomic_load(&touches),atomic_load(&last_action),atomic_load(&last_x),atomic_load(&last_y));
    int closed=fclose(file);return result>0&&closed==0?0:-5;
}
JNIEXPORT jint JNICALL JNI_NAME(draw)(JNIEnv *env,jclass cls) {
    (void)env;(void)cls;
    int width=surface_width,height=surface_height;
    if(width<=0||height<=width)return -2;
    glViewport(0,0,width,height);glEnable(GL_SCISSOR_TEST);
    quadrant(0,0,width/2,height/2,1.f,0.f,0.f);
    quadrant(width/2,0,width-width/2,height/2,0.f,1.f,0.f);
    quadrant(0,height/2,width/2,height-height/2,0.f,0.f,1.f);
    quadrant(width/2,height/2,width-width/2,height-height/2,1.f,1.f,0.f);
    readback_mask=0;
    for(int i=0;i<4;i++) {
        unsigned char pixel[4]={0};
        glReadPixels(i%2?width*3/4:width/4,i/2?height*3/4:height/4,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
        int red=i==0||i==3,green=i==1||i==3,blue=i==2;
        if((pixel[0]>240)==red&&(pixel[1]>240)==green&&(pixel[2]>240)==blue&&pixel[3]>240)readback_mask|=1u<<i;
    }
    quadrant(0,(int)((frames*7u)%(unsigned)height),width,10,1.f,1.f,1.f);
    if(atomic_load(&touches)>0)quadrant(atomic_load(&last_x)-15,height-atomic_load(&last_y)-15,30,30,1.f,1.f,1.f);
    glDisable(GL_SCISSOR_TEST);
    if(glGetError()!=GL_NO_ERROR)return -3;
    if(readback_mask!=15)return -6;
    frames++;
    return frames==1||frames%30==0?report():0;
}
JNIEXPORT void JNICALL JNI_NAME(touch)(JNIEnv *env,jclass cls,jint action,jfloat x,jfloat y) {
    (void)env;(void)cls;
    atomic_store(&last_action,action);atomic_store(&last_x,(int)x);atomic_store(&last_y,(int)y);
    atomic_fetch_add(&touches,1);
}
