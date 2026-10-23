/* Bounded OpenSL ES engine/output-mix/PCM simple-buffer-queue contracts.
 * Implementation MIT OR Apache-2.0. Khronos/AOSP headers and IID source retain
 * their original notices under third_party/android/opensles. */
#define SL_API_DEPRECATED(level)
#include "../../third_party/android/opensles/OpenSLES_Android.h"
#include "media.h"
#include <errno.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <switch.h>
static const struct SLInterfaceID_ engine_id = {
    0x8d97c260, 0xddd4, 0x11db, 0x958f, {0, 2, 0xa5, 0xd5, 0xc5, 0x1b}};
static const struct SLInterfaceID_ play_id = {
    0xef0bd9c0, 0xddd7, 0x11db, 0xbf49, {0, 2, 0xa5, 0xd5, 0xc5, 0x1b}};
static const struct SLInterfaceID_ queue_id = {
    0x198e4940, 0xc5d7, 0x11df, 0xa2a6, {0, 2, 0xa5, 0xd5, 0xc5, 0x1b}};
static const struct SLInterfaceID_ buffer_id = {
    0x2bc99cc0, 0xddd4, 0x11db, 0x8d99, {0, 2, 0xa5, 0xd5, 0xc5, 0x1b}};
const SLInterfaceID SL_IID_ENGINE = &engine_id, SL_IID_PLAY = &play_id,
                    SL_IID_ANDROIDSIMPLEBUFFERQUEUE = &queue_id,
                    SL_IID_BUFFERQUEUE = &buffer_id;
typedef struct Object Object;
typedef struct {
  uint64_t *table;
  Object *owner;
} Interface;
struct Object {
  Interface object, engine, play, queue;
  int kind, realized, state, started, stopping, inflight;
  uint64_t audio, callback, context, completed, played;
  void *session;
  Thread thread;
  Mutex mutex;
  CondVar condition;
  struct {
    void *data;
    size_t bytes;
  } buffers[4];
  unsigned read, write, capacity;
  Object *next, *parent;
};
static Object *objects;
static uint64_t object_table[sizeof(struct SLObjectItf_) / 8],
    engine_table[sizeof(struct SLEngineItf_) / 8],
    play_table[sizeof(struct SLPlayItf_) / 8],
    queue_table[sizeof(struct SLAndroidSimpleBufferQueueItf_) / 8];
static int tables_initialized;
static Object *object(const void *self) {
  return ((const Interface *)self)->owner;
}
static int equal(SLInterfaceID a, SLInterfaceID b) {
  return a && b && !memcmp(a, b, sizeof(*a));
}
static SLresult unsupported(void) { return SL_RESULT_FEATURE_UNSUPPORTED; }
static void worker(void *arg) {
  Object *o = arg;
  as_native_set_session(o->session);
  for (;;) {
    mutexLock(&o->mutex);
    while (!o->stopping &&
           (o->state != SL_PLAYSTATE_PLAYING || o->read == o->write))
      condvarWait(&o->condition, &o->mutex);
    if (o->stopping) {
      mutexUnlock(&o->mutex);
      break;
    }
    unsigned at = o->read % 4;
    void *data = o->buffers[at].data;
    size_t bytes = o->buffers[at].bytes;
    o->inflight = 1;
    mutexUnlock(&o->mutex);
    int64_t played = as_audio_write(o->audio, data, bytes / 4, 1000000000);
    mutexLock(&o->mutex);
    free(data);
    o->buffers[at].data = NULL;
    o->read++;
    o->completed++;
    o->inflight = 0;
    if (played > 0)
      o->played += (uint64_t)played;
    else
      o->stopping = 1;
    uint64_t callback = o->callback, context = o->context;
    condvarWakeAll(&o->condition);
    mutexUnlock(&o->mutex);
    if (callback && played >= 0) {
      AsNativeCall c = {0};
      c.function = callback;
      c.integer[0] = (uint64_t)&o->queue;
      c.integer[1] = context;
      if (as_native_call(&c)) {
        mutexLock(&o->mutex);
        o->stopping = 1;
        mutexUnlock(&o->mutex);
      }
    }
  }
  extern void as_native_thread_cleanup(void);
  as_native_thread_cleanup();
}
static SLresult realize(SLObjectItf self, SLboolean async) {
  Object *o = object(self);
  if (async)
    return SL_RESULT_FEATURE_UNSUPPORTED;
  if (o->realized)
    return SL_RESULT_PRECONDITIONS_VIOLATED;
  if (o->kind == 3) {
    Result rc = threadCreate(&o->thread, worker, o, NULL, 256 << 10, 0x2c, -2);
    if (R_FAILED(rc))
      return SL_RESULT_RESOURCE_ERROR;
    rc = threadStart(&o->thread);
    if (R_FAILED(rc)) {
      threadClose(&o->thread);
      return SL_RESULT_RESOURCE_ERROR;
    }
    o->started = 1;
  }
  o->realized = 1;
  return SL_RESULT_SUCCESS;
}
static SLresult get_state(SLObjectItf self, SLuint32 *state) {
  if (!state)
    return SL_RESULT_PARAMETER_INVALID;
  *state = object(self)->realized ? SL_OBJECT_STATE_REALIZED
                                  : SL_OBJECT_STATE_UNREALIZED;
  return SL_RESULT_SUCCESS;
}
static SLresult get_interface(SLObjectItf self, SLInterfaceID id,
                              void *result) {
  Object *o = object(self);
  if (!result || !id)
    return SL_RESULT_PARAMETER_INVALID;
  *(void **)result = NULL;
  if (!o->realized)
    return SL_RESULT_PRECONDITIONS_VIOLATED;
  if (o->kind == 1 && equal(id, SL_IID_ENGINE))
    *(void **)result = &o->engine;
  else if (o->kind == 3 && equal(id, SL_IID_PLAY))
    *(void **)result = &o->play;
  else if (o->kind == 3 && (equal(id, SL_IID_ANDROIDSIMPLEBUFFERQUEUE) ||
                            equal(id, SL_IID_BUFFERQUEUE)))
    *(void **)result = &o->queue;
  else
    return SL_RESULT_FEATURE_UNSUPPORTED;
  return SL_RESULT_SUCCESS;
}
static SLresult set_play(SLPlayItf self, SLuint32 state) {
  Object *o = object(self);
  if (state != SL_PLAYSTATE_PLAYING && state != SL_PLAYSTATE_STOPPED)
    return SL_RESULT_FEATURE_UNSUPPORTED;
  if (state == SL_PLAYSTATE_PLAYING && as_audio_start(o->audio))
    return SL_RESULT_RESOURCE_ERROR;
  mutexLock(&o->mutex);
  o->state = state;
  condvarWakeAll(&o->condition);
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult get_play(SLPlayItf self, SLuint32 *state) {
  if (!state)
    return SL_RESULT_PARAMETER_INVALID;
  Object *o = object(self);
  mutexLock(&o->mutex);
  *state = o->state;
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult get_position(SLPlayItf self, SLmillisecond *position) {
  if (!position)
    return SL_RESULT_PARAMETER_INVALID;
  Object *o = object(self);
  mutexLock(&o->mutex);
  *position = (SLmillisecond)(o->played * 1000 / 48000);
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult enqueue(SLAndroidSimpleBufferQueueItf self, const void *data,
                        SLuint32 size) {
  Object *o = object(self);
  if (!data || !size || size % 4 || size > 192000)
    return SL_RESULT_PARAMETER_INVALID;
  mutexLock(&o->mutex);
  if (o->stopping || o->write - o->read >= o->capacity) {
    mutexUnlock(&o->mutex);
    return SL_RESULT_BUFFER_INSUFFICIENT;
  }
  void *copy = malloc(size);
  if (!copy) {
    mutexUnlock(&o->mutex);
    return SL_RESULT_MEMORY_FAILURE;
  }
  memcpy(copy, data, size);
  unsigned at = o->write++ % 4;
  o->buffers[at].data = copy;
  o->buffers[at].bytes = size;
  condvarWakeAll(&o->condition);
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult queue_state(SLAndroidSimpleBufferQueueItf self,
                            SLAndroidSimpleBufferQueueState *state) {
  if (!state)
    return SL_RESULT_PARAMETER_INVALID;
  Object *o = object(self);
  mutexLock(&o->mutex);
  state->count = o->write - o->read;
  state->index = (SLuint32)o->completed;
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult queue_callback(SLAndroidSimpleBufferQueueItf self,
                               slAndroidSimpleBufferQueueCallback callback,
                               void *context) {
  Object *o = object(self);
  mutexLock(&o->mutex);
  o->callback = (uint64_t)callback;
  o->context = (uint64_t)context;
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static SLresult clear_queue(SLAndroidSimpleBufferQueueItf self) {
  Object *o = object(self);
  mutexLock(&o->mutex);
  if (o->inflight) {
    mutexUnlock(&o->mutex);
    return SL_RESULT_PRECONDITIONS_VIOLATED;
  }
  while (o->read < o->write) {
    free(o->buffers[o->read % 4].data);
    o->buffers[o->read++ % 4].data = NULL;
  }
  o->completed = 0;
  mutexUnlock(&o->mutex);
  return SL_RESULT_SUCCESS;
}
static int stop_worker(Object *o) {
  if (!o->started)
    return 0;
  if (o->thread.handle == threadGetCurHandle())
    return -EBUSY;
  mutexLock(&o->mutex);
  o->stopping = 1;
  condvarWakeAll(&o->condition);
  mutexUnlock(&o->mutex);
  Handle handle = o->thread.handle;
  int32_t index;
  Result rc = svcWaitSynchronization(&index, &handle, 1, 2000000000);
  if (R_FAILED(rc))
    return -EBUSY;
  threadWaitForExit(&o->thread);
  threadClose(&o->thread);
  o->started = 0;
  return 0;
}
static void destroy(SLObjectItf self) {
  Object *o = object(self);
  for (Object *child = objects, *next; child; child = next) {
    next = child->next;
    if (child->parent == o)
      destroy((SLObjectItf)&child->object);
  }
  if (stop_worker(o)) {
    as_native_fail(-2062);
    return;
  }
  if (o->audio) {
    as_audio_close(o->audio);
    o->audio = 0;
  }
  for (unsigned i = 0; i < 4; i++)
    free(o->buffers[i].data);
  Object **link = &objects;
  while (*link && *link != o)
    link = &(*link)->next;
  if (*link)
    *link = o->next;
  free(o);
}
static Object *new_object(int kind, Object *parent) {
  Object *o = calloc(1, sizeof(*o));
  if (!o)
    return NULL;
  o->kind = kind;
  o->parent = parent;
  o->state = SL_PLAYSTATE_STOPPED;
  o->object = (Interface){object_table, o};
  o->engine = (Interface){engine_table, o};
  o->play = (Interface){play_table, o};
  o->queue = (Interface){queue_table, o};
  o->session = as_native_session();
  o->next = objects;
  objects = o;
  return o;
}
static int required_ok(unsigned count, const SLInterfaceID *ids,
                       const SLboolean *required, int player) {
  if (count > 32 || (count && (!ids || !required)))
    return 0;
  for (unsigned i = 0; i < count; i++)
    if (required[i] &&
        !(player && (equal(ids[i], SL_IID_PLAY) ||
                     equal(ids[i], SL_IID_ANDROIDSIMPLEBUFFERQUEUE) ||
                     equal(ids[i], SL_IID_BUFFERQUEUE))))
      return 0;
  return 1;
}
static SLresult create_mix(SLEngineItf self, SLObjectItf *mix, SLuint32 count,
                           const SLInterfaceID *ids,
                           const SLboolean *required) {
  if (!mix || !required_ok(count, ids, required, 0))
    return SL_RESULT_FEATURE_UNSUPPORTED;
  Object *o = new_object(2, object(self));
  if (!o)
    return SL_RESULT_MEMORY_FAILURE;
  *mix = (SLObjectItf)&o->object;
  return SL_RESULT_SUCCESS;
}
static SLresult create_player(SLEngineItf self, SLObjectItf *player,
                              SLDataSource *source, SLDataSink *sink,
                              SLuint32 count, const SLInterfaceID *ids,
                              const SLboolean *required) {
  if (!player || !source || !sink || !source->pLocator || !source->pFormat ||
      !sink->pLocator)
    return SL_RESULT_PARAMETER_INVALID;
  if (!required_ok(count, ids, required, 1))
    return SL_RESULT_FEATURE_UNSUPPORTED;
  SLDataLocator_AndroidSimpleBufferQueue *locator = source->pLocator;
  SLDataFormat_PCM *format = source->pFormat;
  SLDataLocator_OutputMix *output = sink->pLocator;
  if ((locator->locatorType != SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE &&
       locator->locatorType != SL_DATALOCATOR_BUFFERQUEUE) ||
      locator->numBuffers < 1 || locator->numBuffers > 4 ||
      format->formatType != SL_DATAFORMAT_PCM || format->numChannels != 2 ||
      format->samplesPerSec != SL_SAMPLINGRATE_48 ||
      format->bitsPerSample != 16 || format->containerSize != 16 ||
      format->endianness != SL_BYTEORDER_LITTLEENDIAN ||
      format->channelMask != (SL_SPEAKER_FRONT_LEFT | SL_SPEAKER_FRONT_RIGHT) ||
      output->locatorType != SL_DATALOCATOR_OUTPUTMIX || !output->outputMix)
    return SL_RESULT_CONTENT_UNSUPPORTED;
  Object *mix = object(output->outputMix);
  if (mix->kind != 2 || !mix->realized)
    return SL_RESULT_PRECONDITIONS_VIOLATED;
  int64_t audio = as_audio_open(48000, 2, 1);
  if (audio < 0)
    return SL_RESULT_RESOURCE_ERROR;
  Object *o = new_object(3, object(self));
  if (!o) {
    as_audio_close(audio);
    return SL_RESULT_MEMORY_FAILURE;
  }
  o->audio = audio;
  o->capacity = locator->numBuffers;
  *player = (SLObjectItf)&o->object;
  return SL_RESULT_SUCCESS;
}
#define SET(table, type, field, fn)                                            \
  table[offsetof(struct type, field) / 8] = as_native_veneer((uint64_t)(fn))
static void initialize_tables(void) {
  if (tables_initialized)
    return;
  uint64_t missing = as_native_veneer((uint64_t)unsupported);
  for (size_t i = 0; i < sizeof(object_table) / 8; i++)
    object_table[i] = missing;
  for (size_t i = 0; i < sizeof(engine_table) / 8; i++)
    engine_table[i] = missing;
  for (size_t i = 0; i < sizeof(play_table) / 8; i++)
    play_table[i] = missing;
  for (size_t i = 0; i < sizeof(queue_table) / 8; i++)
    queue_table[i] = missing;
  SET(object_table, SLObjectItf_, Realize, realize);
  SET(object_table, SLObjectItf_, GetState, get_state);
  SET(object_table, SLObjectItf_, GetInterface, get_interface);
  SET(object_table, SLObjectItf_, Destroy, destroy);
  SET(engine_table, SLEngineItf_, CreateOutputMix, create_mix);
  SET(engine_table, SLEngineItf_, CreateAudioPlayer, create_player);
  SET(play_table, SLPlayItf_, SetPlayState, set_play);
  SET(play_table, SLPlayItf_, GetPlayState, get_play);
  SET(play_table, SLPlayItf_, GetPosition, get_position);
  SET(queue_table, SLAndroidSimpleBufferQueueItf_, Enqueue, enqueue);
  SET(queue_table, SLAndroidSimpleBufferQueueItf_, Clear, clear_queue);
  SET(queue_table, SLAndroidSimpleBufferQueueItf_, GetState, queue_state);
  SET(queue_table, SLAndroidSimpleBufferQueueItf_, RegisterCallback,
      queue_callback);
  tables_initialized = 1;
}
static SLresult create_engine(SLObjectItf *engine, SLuint32 options,
                              const SLEngineOption *values, SLuint32 count,
                              const SLInterfaceID *ids,
                              const SLboolean *required) {
  if (!engine || options > 4 || !required_ok(count, ids, required, 0))
    return SL_RESULT_FEATURE_UNSUPPORTED;
  for (unsigned i = 0; i < options; i++)
    if (!values || values[i].feature != SL_ENGINEOPTION_THREADSAFE ||
        values[i].data != SL_BOOLEAN_TRUE)
      return SL_RESULT_FEATURE_UNSUPPORTED;
  initialize_tables();
  Object *o = new_object(1, NULL);
  if (!o)
    return SL_RESULT_MEMORY_FAILURE;
  *engine = (SLObjectItf)&o->object;
  return SL_RESULT_SUCCESS;
}
int as_sl_quiesce(void) {
  for (Object *o = objects; o; o = o->next)
    if (stop_worker(o))
      return -EBUSY;
  return 0;
}
void as_sl_reset(void) {
  while (objects)
    destroy((SLObjectItf)&objects->object);
  tables_initialized = 0;
}
uint64_t as_sl_import(const char *name) {
  if (!strcmp(name, "slCreateEngine"))
    return (uint64_t)create_engine;
  if (!strcmp(name, "SL_IID_ENGINE"))
    return (uint64_t)&SL_IID_ENGINE;
  if (!strcmp(name, "SL_IID_PLAY"))
    return (uint64_t)&SL_IID_PLAY;
  if (!strcmp(name, "SL_IID_ANDROIDSIMPLEBUFFERQUEUE"))
    return (uint64_t)&SL_IID_ANDROIDSIMPLEBUFFERQUEUE;
  if (!strcmp(name, "SL_IID_BUFFERQUEUE"))
    return (uint64_t)&SL_IID_BUFFERQUEUE;
  return 0;
}
