#include <switch.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <inttypes.h>
#include <malloc.h>
size_t __nx_heap_size=256*1024*1024;
void *as_host_alloc(size_t alignment,size_t size){if(alignment<sizeof(void*))alignment=sizeof(void*);return memalign(alignment,size);}
void as_host_free(void *p){free(p);}
void as_host_panic(void){fprintf(stderr,"AndroSwitch Rust panic\n");abort();}
uint64_t as_platform_micros(void){return armTicksToNs(armGetSystemTick())/1000;}
extern uint64_t as_runtime_open(void);
extern int as_runtime_step(uint64_t);
extern const uint32_t *as_runtime_pixels(uint64_t);
extern void as_runtime_presented(uint64_t,uint64_t);
extern bool as_runtime_capture(uint64_t);
extern int as_runtime_input(uint64_t,int,float,float);
extern void as_runtime_close(uint64_t);
static uint32_t rgba(uint32_t p){return (p&0xff00ff00)|((p>>16)&255)|((p&255)<<16);}
static uint64_t present(Framebuffer *fb,const uint32_t *pixels,bool capture){
    u32 stride;uint8_t *memory=framebufferBegin(fb,&stride);uint64_t checksum=UINT64_C(0xcbf29ce484222325);
    for(unsigned y=0;y<720;y++){uint32_t *row=(void*)(memory+y*stride);for(unsigned x=0;x<1280;x++){row[x]=rgba(pixels[(y/2)*640+x/2]);if(!(x&1)&&!(y&1)){uint32_t readback=rgba(row[x]);checksum=(checksum^readback)*UINT64_C(0x100000001b3);}}}
    if(capture){FILE *file=fopen("sdmc:/switch/androswitch/logs/last-frame.ppm","wb");if(file){fprintf(file,"P6\n1280 720\n255\n");for(unsigned y=0;y<720;y++){uint32_t *row=(void*)(memory+y*stride);for(unsigned x=0;x<1280;x++){uint32_t p=row[x];uint8_t rgb[3]={p,p>>8,p>>16};fwrite(rgb,1,3,file);}}fclose(file);}}
    framebufferEnd(fb);return checksum;
}
int main(int argc,char **argv){
    (void)argc;(void)argv;uint64_t app=as_runtime_open();if(!app){consoleInit(NULL);printf("AndroSwitch: launch failed. See logs/session-report.json\n");while(appletMainLoop()){consoleUpdate(NULL);svcSleepThread(10000000);}consoleExit(NULL);return 1;}
    Framebuffer fb;Result rc=framebufferCreate(&fb,nwindowGetDefault(),1280,720,PIXEL_FORMAT_RGBA_8888,2);if(R_FAILED(rc)){as_runtime_close(app);return 2;}rc=framebufferMakeLinear(&fb);if(R_FAILED(rc)){framebufferClose(&fb);as_runtime_close(app);return 3;}
    padConfigureInput(1,HidNpadStyleSet_NpadStandard);PadState pad;padInitializeDefault(&pad);hidInitializeTouchScreen();bool touching=false;int last_x=0,last_y=0;
    while(appletMainLoop()){
        int state=as_runtime_step(app);if(state<0||state==1)break;
        padUpdate(&pad);u64 down=padGetButtonsDown(&pad);if(down&HidNpadButton_Plus)break;
        const struct {u64 button;int key;} keys[]={{HidNpadButton_A,23},{HidNpadButton_B,4},{HidNpadButton_AnyDown,20},{HidNpadButton_AnyUp,19},{HidNpadButton_AnyLeft,21},{HidNpadButton_AnyRight,22}};
        for(unsigned i=0;i<sizeof(keys)/sizeof(keys[0]);i++)if(down&keys[i].button)as_runtime_input(app,4,(float)keys[i].key,0);
        HidTouchScreenState touch={0};hidGetTouchScreenStates(&touch,1);if(touch.count){last_x=touch.touches[0].x/2;last_y=touch.touches[0].y/2;as_runtime_input(app,touching?2:0,last_x,last_y);touching=true;}else if(touching){as_runtime_input(app,1,last_x,last_y);touching=false;}
        uint64_t checksum=present(&fb,as_runtime_pixels(app),as_runtime_capture(app));as_runtime_presented(app,checksum);
    }
    framebufferClose(&fb);as_runtime_close(app);return 0;
}
