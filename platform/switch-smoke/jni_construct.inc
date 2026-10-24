/* Shared production NewObject/A/V entry points; helpers provided by jni.c. */
static jobject new_object_a(JNIEnv *env, jclass klass, jmethodID method,
                            const jvalue *values) {
  if (exception_check(env)) return NULL;
  char types[128] = {0};
  size_t count = argument_types(method, types);
  uint64_t bits[128] = {0};
  as_jni_arguments_a(types, count, values, bits);
  return (jobject)dispatch(CONSTRUCT, (uint64_t)klass, (uint64_t)method, 0,
                          bits, count);
}
static jobject new_object_v(JNIEnv *env, jclass klass, jmethodID method,
                            va_list values) {
  if (exception_check(env)) return NULL;
  char types[128] = {0};
  size_t count = argument_types(method, types);
  uint64_t bits[128] = {0};
  as_jni_arguments_v(types, count, values, bits);
  return (jobject)dispatch(CONSTRUCT, (uint64_t)klass, (uint64_t)method, 0,
                          bits, count);
}
static jobject new_object(JNIEnv *env, jclass klass, jmethodID method, ...) {
  va_list values;
  va_start(values, method);
  jobject result = new_object_v(env, klass, method, values);
  va_end(values);
  return result;
}
