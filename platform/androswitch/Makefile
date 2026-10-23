DEVKITPRO ?= /opt/devkitpro
DEVKITA64 := $(DEVKITPRO)/devkitA64
export DEVKITPRO DEVKITA64
CC := $(DEVKITA64)/bin/aarch64-none-elf-gcc
TOOLS := $(DEVKITPRO)/tools/bin
LIBNX := $(DEVKITPRO)/libnx
OUTPUT_DIR ?= ../../local-artifacts/product
RUST_LIBRARY ?= ../../target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a
ARCH := -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE
CFLAGS := $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -I../switch-smoke
OBJECTS := $(addprefix $(OUTPUT_DIR)/,main.o fs.o native.o native_call.o jni.o bionic_io.o)
.PHONY: all
all: $(OUTPUT_DIR)/androswitch.nro
$(OUTPUT_DIR):
	mkdir -p "$@"
$(OUTPUT_DIR)/main.o: main.c | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -c "$<" -o "$@"
$(OUTPUT_DIR)/%.o: ../switch-smoke/%.c ../switch-smoke/native.h | $(OUTPUT_DIR)
	$(CC) $(CFLAGS) -fno-strict-aliasing -c "$<" -o "$@"
$(OUTPUT_DIR)/native_call.o: ../switch-smoke/native_call.S | $(OUTPUT_DIR)
	$(CC) $(ARCH) -c "$<" -o "$@"
$(OUTPUT_DIR)/androswitch.elf: $(OBJECTS) $(RUST_LIBRARY)
	$(CC) $(ARCH) -specs=$(LIBNX)/switch.specs -L$(LIBNX)/lib $(OBJECTS) -Wl,--start-group "$(RUST_LIBRARY)" -lnx -lm -Wl,--end-group -o "$@"
$(OUTPUT_DIR)/androswitch.nacp: Makefile | $(OUTPUT_DIR)
	$(TOOLS)/nacptool --create "AndroSwitch" "AndroSwitch / RustyDroid contributors" "0.1.0-dev" "$@"
$(OUTPUT_DIR)/androswitch.nro: $(OUTPUT_DIR)/androswitch.elf $(OUTPUT_DIR)/androswitch.nacp
	$(TOOLS)/elf2nro "$<" "$@" --nacp=$(OUTPUT_DIR)/androswitch.nacp
