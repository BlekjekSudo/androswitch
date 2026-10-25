/* Production typed JNI static-field entry points. MIT OR Apache-2.0.
 * Values cross the Rust boundary as scalar bits, never host object pointers. */
static jfieldID static_field_id(JNIEnv *env, jclass klass, const char *name,
                                const char *signature) {
  (void)env;
  return (jfieldID)dispatch(FIELD, (uint64_t)klass, (uint64_t)name,
                            (uint64_t)signature, NULL, 1);
}
#define STATIC_FIELD(Name, Type, Kind)                                        \
  static Type get_static_##Name##_field(JNIEnv *env, jclass klass,            \
                                        jfieldID field) {                     \
    (void)env;                                                                \
    uint64_t bits =                                                           \
        dispatch(GET_FIELD, (uint64_t)klass, (uint64_t)field, Kind, NULL, 0); \
    Type value;                                                               \
    memcpy(&value, &bits, sizeof(value));                                     \
    return value;                                                             \
  }                                                                           \
  static void set_static_##Name##_field(JNIEnv *env, jclass klass,            \
                                        jfieldID field, Type value) {         \
    (void)env;                                                                \
    uint64_t bits = 0;                                                        \
    memcpy(&bits, &value, sizeof(value));                                     \
    dispatch(SET_FIELD, (uint64_t)klass, (uint64_t)field, Kind, &bits, 8);    \
  }
STATIC_FIELD(Object, jobject, 'L')
STATIC_FIELD(Boolean, jboolean, 'Z')
STATIC_FIELD(Byte, jbyte, 'B')
STATIC_FIELD(Char, jchar, 'C')
STATIC_FIELD(Short, jshort, 'S')
STATIC_FIELD(Int, jint, 'I')
STATIC_FIELD(Long, jlong, 'J')
STATIC_FIELD(Float, jfloat, 'F')
STATIC_FIELD(Double, jdouble, 'D')
#undef STATIC_FIELD
