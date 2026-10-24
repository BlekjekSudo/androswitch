/* A process-wide transport barrier, shared by all guest/platform workers. */
#pragma once
/* Success holds the admission lock until leave, across OS start AND owner
 * registry insertion. Rejection returns negative host ECANCELED unlocked. */
int as_worker_admission_enter(void);
void as_worker_admission_leave(void);
/* Wait for every previously admitted start to be registered, then reject all
 * future starts through joins/TLS/global destructors and resource cleanup. */
void as_worker_admission_begin_shutdown(void);
/* Called only at the successful end of complete application resource reset. */
void as_worker_admission_reset(void);
