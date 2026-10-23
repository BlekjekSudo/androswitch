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

$(OUTPUT_DIR)/main.o: main.c | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/fs.o: fs.c | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native.o: native.c native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/native_call.o: native_call.S | $(OUTPUT_DIR)
	$(CC) $(ARCH) -c "$<" -o "$@"

$(OUTPUT_DIR)/jni.o: jni.c jni_traps.inc native.h ../../third_party/android/jni.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -fno-strict-aliasing -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/bionic_io.o: bionic_io.c native.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/androswitch-smoke.elf: $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/fs.o $(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/native_call.o $(OUTPUT_DIR)/jni.o $(OUTPUT_DIR)/bionic_io.o $(RUST_LIBRARY)
	$(CC) $(ARCH) -specs=$(LIBNX)/switch.specs -L$(LIBNX)/lib $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/fs.o $(OUTPUT_DIR)/native.o $(OUTPUT_DIR)/native_call.o $(OUTPUT_DIR)/jni.o $(OUTPUT_DIR)/bionic_io.o -Wl,--start-group "$(RUST_LIBRARY)" -lnx -lm -Wl,--end-group -o "$@"

$(OUTPUT_DIR)/androswitch-smoke.nacp: Makefile | $(OUTPUT_DIR)
	$(TOOLS)/nacptool --create "AndroSwitch Runtime Proof" "AndroSwitch" "0.1.0" "$@"

$(OUTPUT_DIR)/androswitch-smoke.nro: $(OUTPUT_DIR)/androswitch-smoke.elf $(OUTPUT_DIR)/androswitch-smoke.nacp
	$(TOOLS)/elf2nro "$<" "$@" --nacp=$(OUTPUT_DIR)/androswitch-smoke.nacp
