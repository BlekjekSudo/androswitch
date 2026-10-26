#include <switch.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "../../platform/horizon-std/support.h"

size_t __nx_heap_size = 128 * 1024 * 1024;
extern int as_std_probe_run(void);
static char session[64], phase[32];
static void shutdown_marker(void) {
    char path[256];
    snprintf(path, sizeof(path), "sdmc:/switch/androswitch-std-probe/%s/shutdown-%s.txt", session, phase);
    FILE *file = fopen(path, "w");
    if (file) { fputs("C atexit reached after Rust returned\n", file); fclose(file); }
}

int as_std_c_exchange(const char *path) {
    char bytes[4];
    FILE *file = fopen(path, "r+b");
    if (!file) return 1;
    int ok = fread(bytes, 1, 4, file) == 4 && !memcmp(bytes, "Rust", 4);
    if (ok) ok = fseek(file, 0, SEEK_END) == 0 && fwrite("+C", 1, 2, file) == 2;
    if (fclose(file)) ok = 0;
    return ok ? 0 : 2;
}

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;
    FILE *request = fopen("sdmc:/switch/androswitch-std-probe/request.txt", "r");
    if (!request) return 111;
    int valid = fgets(session, sizeof(session), request) && fgets(phase, sizeof(phase), request);
    fclose(request);
    if (!valid) return 112;
    session[strcspn(session, "\r\n")] = 0;
    phase[strcspn(phase, "\r\n")] = 0;
    if (strspn(session, "0123456789abcdef") != strlen(session)) return 113;
    if (strcmp(phase, "suite") && strcmp(phase, "restart") && strcmp(phase, "panic")) return 115;
    if (atexit(shutdown_marker)) return 114;
    if (as_horizon_std_init()) return 110;
    int result = as_std_probe_run();
    if (as_horizon_std_shutdown(5000)) result=120;
    char path[256];
    snprintf(path, sizeof(path), "sdmc:/switch/androswitch-std-probe/%s/returned-%s.txt", session, phase);
    FILE *file = fopen(path, "w");
    if (file) {
        fprintf(file, "%d\n", result);
        fclose(file);
    }
    return result;
}
