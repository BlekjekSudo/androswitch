/* Runtime teardown coordination, not a guest Android API. MIT OR Apache-2.0. */
#pragma once
#define AS_FINALIZE_PASSES 64u
#define AS_FINALIZE_PENDING_KEYS 4u
unsigned as_bionic_finalization_pending(void);
int as_bionic_finalize_thread(void);
int as_bionic_finalize_session(void);
