/* Original API-21 AMediaFormat implementation; MIT OR Apache-2.0. Public
 * signatures follow Android NDK's Apache-2.0 NdkMediaFormat.h. */
#ifndef AS_MEDIA_FORMAT_H
#define AS_MEDIA_FORMAT_H
#include <stdint.h>
uint64_t as_media_format_import(const char *name);
void as_media_format_reset(void);
#endif
