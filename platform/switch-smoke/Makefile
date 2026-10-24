DEVKITPRO ?= /opt/devkitpro
DEVKITA64 := $(DEVKITPRO)/devkitA64
export DEVKITPRO DEVKITA64
CC := $(DEVKITA64)/bin/aarch64-none-elf-gcc
TOOLS := $(DEVKITPRO)/tools/bin
LIBNX := $(DEVKITPRO)/libnx
OUTPUT_DIR ?= ../../local-artifacts/m1
RUST_LIBRARY ?= ../../target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a
ARCH := -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE

.PHONY: all
all: $(OUTPUT_DIR)/androswitch-smoke.nro

$(OUTPUT_DIR):
	mkdir -p "$@"

$(OUTPUT_DIR)/main.o: main.c ../androswitch/memory_policy.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/fs.o: fs.c | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native.o: native.c native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/worker_admission.o: worker_admission.c worker_admission.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native_call.o: native_call.S | $(OUTPUT_DIR)
	$(CC) $(ARCH) -c "$<" -o "$@"

$(OUTPUT_DIR)/jni.o: jni.c jni_arguments.h jni_construct.inc jni_traps.inc native.h ../../third_party/android/jni.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -fno-strict-aliasing -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_io.o: bionic_io.c native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_sync.o: bionic_sync.c native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_cxx.o: bionic_cxx.c bionic_cxx.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native.o: bionic_finalize.h

$(OUTPUT_DIR)/bionic_finalize.o: bionic_finalize.c bionic_finalize.h bionic_cxx.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/bionic_libc.o: bionic_locale.h

$(OUTPUT_DIR)/bionic_libc.o: bionic_libc.c bionic_auxv.h bionic_properties.h runtime_properties.inc native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_locale.o: bionic_locale.c bionic_locale.h bionic_cxx.h bionic_errno.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_locale.o $(OUTPUT_DIR)/bionic_libc.o: bionic_multibyte.h

$(OUTPUT_DIR)/bionic_multibyte.o: bionic_multibyte.c bionic_multibyte.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_libc.o: bionic_random.h

$(OUTPUT_DIR)/bionic_random.o: bionic_random.c bionic_random.h bionic_cxx.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_keys.o: bionic_keys.c bionic_cxx.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_log.o: bionic_log.c bionic_cxx.h native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_vfs.o: bionic_vfs.c bionic_vfs.h bionic_cxx.h native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(addprefix $(OUTPUT_DIR)/,native.o bionic_io.o bionic_cxx.o bionic_keys.o bionic_libc.o bionic_log.o bionic_vfs.o bionic_sync.o): bionic_cxx.h bionic_errno.h bionic_vfs.h

$(OUTPUT_DIR)/missing_import.o: missing_import.c bionic_cxx.h native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/androswitch-smoke.elf: $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/fs.o $(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/worker_admission.o $(OUTPUT_DIR)/native_call.o $(OUTPUT_DIR)/jni.o $(OUTPUT_DIR)/bionic_io.o $(OUTPUT_DIR)/bionic_sync.o $(OUTPUT_DIR)/bionic_cxx.o $(OUTPUT_DIR)/bionic_finalize.o $(OUTPUT_DIR)/bionic_libc.o $(OUTPUT_DIR)/bionic_locale.o $(OUTPUT_DIR)/bionic_multibyte.o $(OUTPUT_DIR)/bionic_random.o $(OUTPUT_DIR)/bionic_keys.o $(OUTPUT_DIR)/bionic_log.o $(OUTPUT_DIR)/bionic_vfs.o $(OUTPUT_DIR)/missing_import.o $(RUST_LIBRARY) Makefile
	$(CC) $(ARCH) -specs=$(LIBNX)/switch.specs -L$(LIBNX)/lib $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/fs.o $(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/worker_admission.o $(OUTPUT_DIR)/native_call.o $(OUTPUT_DIR)/jni.o $(OUTPUT_DIR)/bionic_io.o $(OUTPUT_DIR)/bionic_sync.o $(OUTPUT_DIR)/bionic_cxx.o $(OUTPUT_DIR)/bionic_finalize.o $(OUTPUT_DIR)/bionic_libc.o $(OUTPUT_DIR)/bionic_locale.o $(OUTPUT_DIR)/bionic_multibyte.o $(OUTPUT_DIR)/bionic_random.o $(OUTPUT_DIR)/bionic_keys.o $(OUTPUT_DIR)/bionic_log.o $(OUTPUT_DIR)/bionic_vfs.o $(OUTPUT_DIR)/missing_import.o -Wl,--start-group "$(RUST_LIBRARY)" -lnx -lm -Wl,--end-group -o "$@"

$(OUTPUT_DIR)/androswitch-smoke.nacp: Makefile | $(OUTPUT_DIR)
	$(TOOLS)/nacptool --create "AndroSwitch Runtime Proof" "AndroSwitch" "0.1.0" "$@"

$(OUTPUT_DIR)/androswitch-smoke.nro: $(OUTPUT_DIR)/androswitch-smoke.elf $(OUTPUT_DIR)/androswitch-smoke.nacp
	$(TOOLS)/elf2nro "$<" "$@" --nacp=$(OUTPUT_DIR)/androswitch-smoke.nacp

$(OUTPUT_DIR)/native.o: worker_admission.h
