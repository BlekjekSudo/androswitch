#!/usr/bin/env python3
"""NDK caller and actual production Horizon JNI field entry points. No VM/OS claim."""
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM
from unicorn.arm64_const import UC_ARM64_REG_SP, UC_ARM64_REG_LR, UC_ARM64_REG_X0


def main():
    root = Path(__file__).resolve().parent.parent
    out = root / 'local-artifacts/m9/jni-static-fields-abi'
    out.mkdir(parents=True, exist_ok=True)
    ndk = Path.home() / 'AppData/Local/Android/Sdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
    cc = 'C:/devkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe'
    clang = str(ndk / 'bin/clang.exe')
    source = root / 'fixtures/m9/jni-objects'
    flags = ['-std=c11', '-Wall', '-Wextra', '-Werror', '-O1', '-ffreestanding', '-fno-builtin', '-fno-stack-protector', '-fno-pic', '-fno-pie', '-ffunction-sections', '-fdata-sections']
    subprocess.run([cc, *flags, '-c', str(source/'static_callee.c'), '-o', str(out/'callee.o')], check=True)
    subprocess.run([clang, '--target=aarch64-linux-android26', f'--sysroot={ndk/"sysroot"}', *flags, '-c', str(source/'static_caller.c'), '-o', str(out/'caller.o')], check=True)
    layout = out/'layout.ld'
    layout.write_text('ENTRY(oracle_case)\nSECTIONS { . = 0x100000; .text : { *(.text*) } .rodata : { *(.rodata*) } .data : { *(.data*) } .bss : { *(.bss*) *(COMMON) } /DISCARD/ : { *(.comment*) *(.eh_frame*) *(.note*) } }\n')
    subprocess.run([clang, '--target=aarch64-linux-android26', '-nostdlib', '-static', '-Wl,--gc-sections', '-Wl,--build-id=none', f'-Wl,-T,{layout}', str(out/'caller.o'), str(out/'callee.o'), '-o', str(out/'oracle.elf')], check=True)
    subprocess.run([str(ndk/'bin/llvm-objcopy.exe'), '-O', 'binary', str(out/'oracle.elf'), str(out/'oracle.bin')], check=True)
    symbols = {}
    for line in subprocess.check_output([str(ndk/'bin/llvm-nm.exe'), '-n', str(out/'oracle.elf')], text=True).splitlines():
        row = line.split()
        if len(row) == 3:
            symbols[row[2]] = int(row[0], 16)
    mask = (1 << 64)-1
    kinds = 'LZBCSIJFD'
    inputs = [0x3333, 1, 0x8f, 65530, (-23456)&0xffff, (-17)&0xffffffff, 0x8000000000001234, 0x7fc01234, 0xfff81234567890ab]
    outputs = [0x3333, 1, (-113)&mask, 65530, (-23456)&mask, (-17)&mask, *inputs[6:]]
    cases = []
    for mode in range(19):
        cpu = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
        cpu.mem_map(0x100000, 0x20000)
        cpu.mem_write(0x100000, (out/'oracle.bin').read_bytes())
        cpu.mem_map(0x300000, 0x10000)
        cpu.reg_write(UC_ARM64_REG_SP, 0x30f000)
        cpu.reg_write(UC_ARM64_REG_LR, 0x110000)
        cpu.reg_write(UC_ARM64_REG_X0, mode)
        if 1 <= mode <= 9:
            cpu.mem_write(symbols['oracle_result'], struct.pack('<Q', inputs[mode-1]))
        cpu.emu_start(symbols['oracle_case'], 0x110000, count=200000)
        actual = list(struct.unpack('<6Q', cpu.mem_read(symbols['oracle_observed'], 48)))
        expected = ([34, 0x1111, symbols['oracle_name'], symbols['oracle_signature'], 0, 1] if mode == 0 else
                    [35, 0x1111, 0x2222, ord(kinds[mode-1]), 0, 0] if mode <= 9 else
                    [36, 0x1111, 0x2222, ord(kinds[mode-10]), inputs[mode-10], 8])
        assert actual == expected, (mode, actual, expected)
        assert cpu.reg_read(UC_ARM64_REG_X0) == (0x5555 if mode == 0 else outputs[mode-1] if mode <= 9 else 0x5a), mode
        cases.append({'mode': mode, 'status': 'passed'})
    sources = ['platform/switch-smoke/jni_static_fields.inc', 'fixtures/m9/jni-objects/static_callee.c', 'fixtures/m9/jni-objects/static_caller.c']
    report = {'schema_version': 1, 'status': 'passed', 'backend': 'Unicorn ARM64 CPU oracle', 'scope': 'NDK caller and production Horizon-GCC field entry points. All nine typed getters/setters, narrow signs and exact NaN payload bits. Rust field storage/GC and Horizon execution require separate tests.', 'cases': cases, 'source_sha256': {p: hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sources}}
    (out/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print('JNI static-field cross-compiler ARM64 ABI oracle: PASS (19 cases)')


if __name__ == '__main__':
    main()
