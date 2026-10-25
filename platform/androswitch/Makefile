DEVKITPRO ?= /opt/devkitpro
DEVKITA64 := $(DEVKITPRO)/devkitA64
export DEVKITPRO DEVKITA64
CC := $(DEVKITA64)/bin/aarch64-none-elf-gcc
TOOLS := $(DEVKITPRO)/tools/bin
LIBNX := $(DEVKITPRO)/libnx
OUTPUT_DIR ?= ../../local-artifacts/product
RUST_LIBRARY ?= ../../target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a
SQLITE_SOURCE ?= ../../local-artifacts/dependencies/sqlite
ARCH := -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE
CFLAGS := $(ARCH) -D__SWITCH__ -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -I$(DEVKITPRO)/portlibs/switch/include -I../switch-smoke -I../../third_party/android
OBJECTS := $(addprefix $(OUTPUT_DIR)/,main.o display.o memory.o media.o media_format.o glsurface.o workers.o opensl.o fs.o resource_info.o resource_extra.o native.o worker_admission.o native_call.o jni.o bionic_io.o bionic_sync.o bionic_cxx.o bionic_finalize.o bionic_libc.o bionic_locale.o bionic_multibyte.o bionic_random.o bionic_keys.o bionic_log.o bionic_vfs.o missing_import.o sqlite3.o sqlite_memory.o)
SQLITE_FLAGS := -DSQLITE_OS_OTHER=1 -DSQLITE_THREADSAFE=0 -DSQLITE_TEMP_STORE=3 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_WAL -DSQLITE_DEFAULT_MEMSTATUS=1 -DSQLITE_DEFAULT_CACHE_SIZE=-1024
.PHONY: all
all: $(OUTPUT_DIR)/androswitch.nro
$(OUTPUT_DIR):
	mkdir -p "$@"
$(OUTPUT_DIR)/main.o: main.c media.h glsurface.h framebuffer_copy.h memory.h display.h display_geometry.h ppm_publish.h rgba_ppm.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -c "$<" -o "$@"
$(OUTPUT_DIR)/display.o: display.c display.h display_geometry.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -c "$<" -o "$@"
$(OUTPUT_DIR)/memory.o: memory.c memory.h memory_policy.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -c "$<" -o "$@"
$(OUTPUT_DIR)/media.o: media.c media.h glsurface.h media_format.h gles2_imports.inc display.h display_geometry.h ppm_publish.h ../switch-smoke/bionic_errno.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/glsurface.o: glsurface.c glsurface.h media.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/workers.o: workers.c workers.h media.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/media_format.o: media_format.c media_format.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/opensl.o: opensl.c media.h pcm_volume.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/sqlite3.o: $(SQLITE_SOURCE)/sqlite3.c $(SQLITE_SOURCE)/sqlite3.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 $(SQLITE_FLAGS) -I$(SQLITE_SOURCE) -c "$<" -o "$@"
$(OUTPUT_DIR)/sqlite_memory.o: ../sqlite/memory_vfs.c $(SQLITE_SOURCE)/sqlite3.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) $(SQLITE_FLAGS) -I$(SQLITE_SOURCE) -c "$<" -o "$@"
$(OUTPUT_DIR)/%.o: ../switch-smoke/%.c ../switch-smoke/native.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/jni.o: ../switch-smoke/jni_arguments.h ../switch-smoke/jni_construct.inc ../switch-smoke/jni_traps.inc ../../third_party/android/jni.h
$(OUTPUT_DIR)/resource_info.o: ../switch-smoke/resource_info.h
$(addprefix $(OUTPUT_DIR)/,native.o bionic_io.o bionic_cxx.o bionic_keys.o bionic_libc.o bionic_log.o bionic_vfs.o bionic_sync.o): ../switch-smoke/bionic_cxx.h ../switch-smoke/bionic_errno.h ../switch-smoke/bionic_vfs.h
$(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/bionic_libc.o $(OUTPUT_DIR)/bionic_locale.o: ../switch-smoke/bionic_locale.h
$(OUTPUT_DIR)/bionic_locale.o: ../switch-smoke/bionic_cxx.h ../switch-smoke/bionic_errno.h
$(OUTPUT_DIR)/bionic_locale.o $(OUTPUT_DIR)/bionic_libc.o $(OUTPUT_DIR)/bionic_multibyte.o: ../switch-smoke/bionic_multibyte.h
$(OUTPUT_DIR)/bionic_libc.o $(OUTPUT_DIR)/bionic_random.o: ../switch-smoke/bionic_random.h
$(OUTPUT_DIR)/bionic_random.o: ../switch-smoke/bionic_cxx.h
$(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/bionic_finalize.o: ../switch-smoke/bionic_finalize.h
$(OUTPUT_DIR)/bionic_finalize.o: ../switch-smoke/bionic_cxx.h
$(OUTPUT_DIR)/bionic_libc.o: ../switch-smoke/bionic_auxv.h ../switch-smoke/bionic_properties.h ../switch-smoke/runtime_properties.inc
$(OUTPUT_DIR)/native_call.o: ../switch-smoke/native_call.S | $(OUTPUT_DIR)
	$(CC) $(ARCH) -c "$<" -o "$@"
$(OUTPUT_DIR)/androswitch.elf: $(OBJECTS) $(RUST_LIBRARY) Makefile
	$(CC) $(ARCH) -specs=$(LIBNX)/switch.specs -L$(LIBNX)/lib -L$(DEVKITPRO)/portlibs/switch/lib $(OBJECTS) -Wl,--start-group "$(RUST_LIBRARY)" -lEGL -lGLESv2 -lglapi -ldrm_nouveau -lstdc++ -lnx -lm -Wl,--end-group -o "$@"
$(OUTPUT_DIR)/androswitch.nacp: Makefile | $(OUTPUT_DIR)
	$(TOOLS)/nacptool --create "AndroSwitch" "AndroSwitch / RustyDroid contributors" "0.1.0-dev" "$@"
$(OUTPUT_DIR)/androswitch.nro: $(OUTPUT_DIR)/androswitch.elf $(OUTPUT_DIR)/androswitch.nacp
	$(TOOLS)/elf2nro "$<" "$@" --nacp=$(OUTPUT_DIR)/androswitch.nacp

$(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/worker_admission.o $(OUTPUT_DIR)/workers.o $(OUTPUT_DIR)/glsurface.o $(OUTPUT_DIR)/opensl.o: ../switch-smoke/worker_admission.h
