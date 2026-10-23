#include <switch.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <malloc.h>
#include <inttypes.h>

// This parser smoke needs a bounded heap, not all application RAM.
size_t __nx_heap_size = 256 * 1024 * 1024;

typedef struct {
    uint32_t version, file_size, classes, methods, strings, checksum_ok;
} AsDexSummary;

extern uint32_t as_smoke_parse_dex(const uint8_t *, size_t, AsDexSummary *);
extern uint64_t as_smoke_allocator_probe(void);
extern size_t as_smoke_run_apk(const uint8_t *, size_t, const uint8_t *, size_t, uint8_t *, size_t);

static uint8_t *read_bounded(const char *path, size_t limit, size_t *length) {
    *length = 0;
    FILE *file = fopen(path, "rb");
    if (!file) return NULL;
    uint8_t *data = NULL;
    if (fseek(file, 0, SEEK_END) == 0) {
        long size = ftell(file);
        if (size > 0 && (size_t)size <= limit && fseek(file, 0, SEEK_SET) == 0) {
            data = malloc((size_t)size);
            if (data && fread(data, 1, (size_t)size, file) == (size_t)size)
                *length = (size_t)size;
            else { free(data); data = NULL; }
        }
    }
    fclose(file);
    return data;
}

static int apk_probe(const char *base) {
    char path[256];
    size_t apk_size, request_size;
    snprintf(path, sizeof(path), "%s/app.apk", base);
    uint8_t *apk = read_bounded(path, 64 * 1024 * 1024, &apk_size);
    snprintf(path, sizeof(path), "%s/request.json", base);
    uint8_t *request = read_bounded(path, 64 * 1024, &request_size);
    if (!apk || !request) { free(apk); free(request); return 0; }
    uint8_t *output = malloc(64 * 1024);
    size_t length = output ? as_smoke_run_apk(apk, apk_size, request, request_size, output, 64 * 1024) : 0;
    free(apk); free(request);
    int written = 0;
    if (length) {
        snprintf(path, sizeof(path), "%s/probe-result.json", base);
        FILE *file = fopen(path, "wb");
        if (file) { written = fwrite(output, 1, length, file) == length; fclose(file); }
        printf("APK method probe: %u bytes, report %s\n", (unsigned)length, written ? "saved" : "write failed");
    }
    free(output);
    return written;
}

void *as_host_alloc(size_t alignment, size_t size) {
    if (alignment < sizeof(void *)) alignment = sizeof(void *);
    return memalign(alignment, size);
}
void as_host_free(void *memory) { free(memory); }
void as_host_panic(void) { fprintf(stderr, "AS_SMOKE Rust panic\n"); abort(); }

int main(int argc, char **argv) {
    consoleInit(NULL);
    const char *base = "sdmc:/switch/androswitch-smoke";
    (void)argc; (void)argv;
    char input[256], output[256];
    snprintf(input, sizeof(input), "%s/classes.dex", base);
    snprintf(output, sizeof(output), "%s/smoke.json", base);
    const uint64_t start = armTicksToNs(armGetSystemTick()) / 1000;
    const uint64_t allocation = as_smoke_allocator_probe();
    uint32_t result = 3;
    AsDexSummary summary = {0};
    FILE *file = fopen(input, "rb");
    if (file && fseek(file, 0, SEEK_END) == 0) {
        const long size = ftell(file);
        if (size > 0 && size <= 64 * 1024 * 1024 && fseek(file, 0, SEEK_SET) == 0) {
            uint8_t *bytes = malloc((size_t)size);
            if (bytes) {
                if (fread(bytes, 1, (size_t)size, file) == (size_t)size)
                    result = as_smoke_parse_dex(bytes, (size_t)size, &summary);
                free(bytes);
            }
        }
    }
    if (file) fclose(file);
    const uint64_t elapsed = armTicksToNs(armGetSystemTick()) / 1000 - start;
    char report[768];
    snprintf(report, sizeof(report),
        "{\"schema_version\":1,\"kind\":\"dex_parser_smoke\","
        "\"allocator_checksum\":%" PRIu64 ",\"parse_result\":%u,"
        "\"version\":%u,\"file_size\":%u,\"classes\":%u,\"methods\":%u,"
        "\"strings\":%u,\"checksum_ok\":%u,\"elapsed_micros\":%" PRIu64 ","
        "\"apk_execution\":false}\n",
        allocation, result, summary.version, summary.file_size, summary.classes,
        summary.methods, summary.strings, summary.checksum_ok, elapsed);
    printf("AndroSwitch M1 portable DEX parser smoke\n%s\nPress + to exit.\n", report);
    FILE *receipt = fopen(output, "wb");
    if (receipt) { fputs(report, receipt); fclose(receipt); }
    const int probe_written = apk_probe(base);
    printf("APK probe report written: %d\n", probe_written);
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    PadState pad;
    padInitializeDefault(&pad);
    while (appletMainLoop()) {
        padUpdate(&pad);
        if (padGetButtonsDown(&pad) & HidNpadButton_Plus) break;
        consoleUpdate(NULL);
    }
    consoleExit(NULL);
    return result == 0 && allocation == 32640 ? 0 : 1;
}
