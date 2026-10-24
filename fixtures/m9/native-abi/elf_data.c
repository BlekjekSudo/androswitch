/* Executable PT_LOAD may include read-only data shaped like instructions.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include <stdint.h>
__attribute__((visibility("default")))
const uint32_t instruction_shaped_data[] = {0xd4000001, 0xd4000021};
extern unsigned data_object;
extern int function_import(int);
unsigned import_kinds(void) { return data_object + function_import(2); }

__asm__(".type data_object,%object\n.type function_import,%function");

static volatile unsigned cleanup_state;
__attribute__((destructor(101))) void cleanup_first(void) { cleanup_state = 1; }
__attribute__((destructor(102))) void cleanup_second(void) {
  cleanup_state = 2;
}
