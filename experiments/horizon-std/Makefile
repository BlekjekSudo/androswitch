DEVKITPRO ?= C:/devkitPro
CC := $(DEVKITPRO)/devkitA64/bin/aarch64-none-elf-gcc
LIBNX := $(DEVKITPRO)/libnx
TOOLS := $(DEVKITPRO)/tools/bin
OUTPUT_DIR ?= ../../local-artifacts/horizon-std/build
ARCH := -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE

.PHONY: all
all: $(OUTPUT_DIR)/androswitch-std-probe.nro

$(OUTPUT_DIR):
	mkdir -p "$@"

$(OUTPUT_DIR)/main.o: main.c | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/abi.o: abi.c | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -c "$<" -o "$@"

$(OUTPUT_DIR)/support.o: ../../platform/horizon-std/support.c ../../platform/horizon-std/support.h | $(OUTPUT_DIR)
	$(CC) $(ARCH) -O2 -Wall -Wextra -Werror -I$(LIBNX)/include -c "$<" -o "$@"

$(OUTPUT_DIR)/androswitch-std-probe.elf: $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/abi.o $(OUTPUT_DIR)/support.o $(RUST_LIBRARY) Makefile
	$(CC) $(ARCH) -specs=$(LIBNX)/switch.specs -L$(LIBNX)/lib $(OUTPUT_DIR)/main.o $(OUTPUT_DIR)/abi.o $(OUTPUT_DIR)/support.o -Wl,--wrap=pthread_create,--wrap=pthread_join,--wrap=pthread_detach,--wrap=pthread_attr_init -Wl,--start-group "$(RUST_LIBRARY)" -lnx -lm -Wl,--end-group -Wl,-Map,$(OUTPUT_DIR)/probe.map -o "$@"

$(OUTPUT_DIR)/androswitch-std-probe.nacp: Makefile | $(OUTPUT_DIR)
	$(TOOLS)/nacptool --create "AndroSwitch std proof" "AndroSwitch" "0.1.0" "$@"

$(OUTPUT_DIR)/androswitch-std-probe.nro: $(OUTPUT_DIR)/androswitch-std-probe.elf $(OUTPUT_DIR)/androswitch-std-probe.nacp
	$(TOOLS)/elf2nro "$<" "$@" --nacp=$(OUTPUT_DIR)/androswitch-std-probe.nacp
