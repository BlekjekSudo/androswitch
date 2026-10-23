#pragma once
#include <stddef.h>
#include <stdint.h>
typedef struct {
  uint64_t integer[8], floating[8], stack[32], result_integer[2],
      result_float[2], function, guest_tls;
} AsNativeCall;
int as_native_call(AsNativeCall *frame);
uint64_t as_native_veneer(uint64_t function);
uint64_t as_native_guest_tls(void);
void *as_native_session(void);
void as_native_set_session(void *session);
int as_native_last_error(void);
void as_native_fail(int code);
void as_native_trap(int code);
void as_jni_vm_lock(void);
void as_jni_vm_unlock(void);
