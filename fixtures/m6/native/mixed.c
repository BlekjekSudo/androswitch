#include <jni.h>
#include <android/asset_manager_jni.h>
#include <aaudio/AAudio.h>
#include <stdint.h>
#include <SLES/OpenSLES.h>
#include <SLES/OpenSLES_Android.h>
#include <stdatomic.h>
#include <pthread.h>
#include <unistd.h>
static atomic_int completed;
static pthread_t main_thread;
static _Thread_local int callback_tls;
static int16_t queued_pcm[4800];
static void queue_complete(SLAndroidSimpleBufferQueueItf queue,void *context){
    (void)context;
    if(pthread_self()==main_thread){atomic_store(&completed,-1);return;}
    if(callback_tls!=0&&callback_tls!=97){atomic_store(&completed,-2);return;}
    callback_tls=97;int count=atomic_fetch_add(&completed,1)+1;
    if(count==1&&(*queue)->Enqueue(queue,queued_pcm,sizeof(queued_pcm))!=SL_RESULT_SUCCESS)atomic_store(&completed,-3);
}
JNIEXPORT jint JNICALL Java_org_androswitch_media_MainActivity_nativeMedia(JNIEnv *env,jclass cls,jobject manager){
    (void)cls;AAssetManager *assets=AAssetManager_fromJava(env,manager);if(!assets)return -1;
    AAsset *asset=AAssetManager_open(assets,"stream.bin",AASSET_MODE_STREAMING);if(!asset)return -2;
    unsigned char byte=0;int valid=AAsset_seek64(asset,42,0)==42&&AAsset_read(asset,&byte,1)==1&&byte==42;AAsset_close(asset);if(!valid)return -3;
    AAudioStreamBuilder *builder;AAudioStream *stream=NULL;if(AAudio_createStreamBuilder(&builder))return -4;
    AAudioStreamBuilder_setSampleRate(builder,48000);AAudioStreamBuilder_setChannelCount(builder,2);AAudioStreamBuilder_setFormat(builder,AAUDIO_FORMAT_PCM_I16);
    int result=-5;if(!AAudioStreamBuilder_openStream(builder,&stream)&&!AAudioStream_requestStart(stream)){
        int16_t samples[4800];for(int i=0;i<4800;i++)samples[i]=(i%96<48?1000:-1000);result=AAudioStream_write(stream,samples,2400,1000000000);
    }
    if(stream){AAudioStream_requestStop(stream);AAudioStream_close(stream);}AAudioStreamBuilder_delete(builder);return result;
}
JNIEXPORT jint JNICALL Java_org_androswitch_media_MainActivity_queuedAudio(JNIEnv *env,jclass cls){
    (void)env;(void)cls;SLObjectItf engine=NULL,mix=NULL,player=NULL;SLEngineItf interface=NULL;SLPlayItf play=NULL;SLAndroidSimpleBufferQueueItf queue=NULL;
    int result=-1;main_thread=pthread_self();atomic_store(&completed,0);
    for(int i=0;i<4800;i++)queued_pcm[i]=i%96<48?1300:-1300;
    if(slCreateEngine(&engine,0,NULL,0,NULL,NULL)||(*engine)->Realize(engine,SL_BOOLEAN_FALSE)||(*engine)->GetInterface(engine,SL_IID_ENGINE,&interface))goto done;
    if((*interface)->CreateOutputMix(interface,&mix,0,NULL,NULL)||(*mix)->Realize(mix,SL_BOOLEAN_FALSE))goto done;
    SLDataLocator_AndroidSimpleBufferQueue source_locator={SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE,2};
    SLDataFormat_PCM format={SL_DATAFORMAT_PCM,2,SL_SAMPLINGRATE_48,16,16,SL_SPEAKER_FRONT_LEFT|SL_SPEAKER_FRONT_RIGHT,SL_BYTEORDER_LITTLEENDIAN};
    SLDataSource source={&source_locator,&format};SLDataLocator_OutputMix sink_locator={SL_DATALOCATOR_OUTPUTMIX,mix};SLDataSink sink={&sink_locator,NULL};
    const SLInterfaceID ids[]={SL_IID_ANDROIDSIMPLEBUFFERQUEUE};const SLboolean required[]={SL_BOOLEAN_TRUE};
    if((*interface)->CreateAudioPlayer(interface,&player,&source,&sink,1,ids,required)||(*player)->Realize(player,SL_BOOLEAN_FALSE)||(*player)->GetInterface(player,SL_IID_PLAY,&play)||(*player)->GetInterface(player,SL_IID_ANDROIDSIMPLEBUFFERQUEUE,&queue))goto done;
    if((*queue)->RegisterCallback(queue,queue_complete,NULL)||(*queue)->Enqueue(queue,queued_pcm,sizeof(queued_pcm))||(*play)->SetPlayState(play,SL_PLAYSTATE_PLAYING))goto done;
    for(int i=0;i<3000&&atomic_load(&completed)>=0&&atomic_load(&completed)<2;i++)usleep(1000);
    result=atomic_load(&completed)==2?4800:-10;
done:
    if(player)(*player)->Destroy(player);if(mix)(*mix)->Destroy(mix);if(engine)(*engine)->Destroy(engine);return result;
}
