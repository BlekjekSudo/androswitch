/* Application presentation uses Android Surface rotations 0..3. The Switch
 * layer stays 1280x720; portrait EGL buffers are rotated by the compositor. */
#pragma once
int as_display_set_rotation(int rotation);
int as_display_rotation(void);
int as_display_width(void);
int as_display_height(void);
int as_display_begin_gpu(void);
int as_display_begin_cpu(int application);
/* Call only after CPU/GPU buffers and their owning workers are released. */
int as_display_reset(void);
