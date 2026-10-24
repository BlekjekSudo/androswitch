#!/usr/bin/env python3
"""Actual NDK caller -> actual Horizon-GCC production JNI entry points.

CPU/ABI oracle only: the Rust VM dispatch is modeled and no Android/Horizon
kernel, constructors, GC, or display execution is asserted here.
"""
import argparse
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM
from unicorn.arm64_const import UC_ARM64_REG_SP, UC_ARM64_REG_LR, UC_ARM64_REG_X0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--ndk', default=str(Path.home()/'AppData/Local/Android/Sdk/ndk/28.2.13676358'))
    parser.add_argument('--horizon-cc', default='C:/devkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    source = root/'fixtures/m9/jni-construct'
    out = root/'local-artifacts/m9/jni-constructor-abi'
    out.mkdir(parents=True, exist_ok=True)
    tools = Path(args.ndk)/'toolchains/llvm/prebuilt/windows-x86_64'
    common = ['-std=c11', '-Wall', '-Wextra', '-Werror', '-O1', '-ffreestanding',
              '-fno-builtin', '-fno-stack-protector', '-fno-pic', '-fno-pie',
              '-ffunction-sections', '-fdata-sections']
    commands = [[args.horizon_cc, *common, '-c', str(source/'abi_callee.c'), '-o', str(out/'callee.o')],
                [str(tools/'bin/clang.exe'), '--target=aarch64-linux-android26',
                 f'--sysroot={tools / "sysroot"}', *common,
                 '-c', str(source/'abi_caller.c'), '-o', str(out/'caller.o')]]
    for cmd in commands:
        subprocess.run(cmd, check=True)
    layout = out/'layout.ld'
    layout.write_text('ENTRY(oracle_case)\nSECTIONS { . = 0x100000; .text : { *(.text*) } '
                      '.rodata : { *(.rodata*) } .data : { *(.data*) } .bss : { *(.bss*) *(COMMON) } '
                      '/DISCARD/ : { *(.comment*) *(.eh_frame*) *(.note*) } }\n')
    elf, binary = out/'oracle.elf', out/'oracle.bin'
    subprocess.run([str(tools/'bin/clang.exe'), '--target=aarch64-linux-android26', '-nostdlib',
                    '-static', '-Wl,--gc-sections', '-Wl,-u,oracle_pending_case', '-Wl,--build-id=none',
                    f'-Wl,-T,{layout}', str(out/'caller.o'), str(out/'callee.o'), '-o', str(elf)], check=True)
    subprocess.run([str(tools/'bin/llvm-objcopy.exe'), '-O', 'binary', str(elf), str(binary)], check=True)
    symbols = {}
    for line in subprocess.check_output([str(tools/'bin/llvm-nm.exe'), '-n', str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    mask = (1 << 64)-1
    double = lambda value: struct.unpack('<Q', struct.pack('<d', value))[0]
    f32 = struct.unpack('<I', struct.pack('<f', 1.25))[0]
    expected = [38, 0x1111, 0x2222, -17 & mask, 1, -113 & mask, 65530,
                -23456 & mask, 0x123456789abcdef, f32, double(-3.5), 0x3333, 0x4444,
                *[double(0.5+i) for i in range(8)], 9]
    cases = []
    for pending in [False, True]:
        for form, name in enumerate(['NewObject', 'NewObjectV', 'NewObjectA']):
            cpu = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
            cpu.mem_map(0x100000, 0x20000)
            cpu.mem_write(0x100000, binary.read_bytes())
            cpu.mem_map(0x300000, 0x10000)
            cpu.reg_write(UC_ARM64_REG_SP, 0x30f000)
            cpu.reg_write(UC_ARM64_REG_LR, 0x110000)
            cpu.reg_write(UC_ARM64_REG_X0, form)
            entry = symbols['oracle_pending_case' if pending else 'oracle_case']
            cpu.emu_start(entry, 0x110000, count=200000)
            actual = list(struct.unpack('<22Q', bytes(cpu.mem_read(symbols['oracle_observed'], 22*8))))
            assert cpu.reg_read(UC_ARM64_REG_X0) == (0 if pending else 0x5555), name
            assert actual == ([0]*22 if pending else expected), (name, pending, actual, expected)
            cases.append({'api': name, 'pending_exception': pending, 'status': 'passed'})
    sources = ['platform/switch-smoke/jni_arguments.h', 'platform/switch-smoke/jni_construct.inc',
               'fixtures/m9/jni-construct/abi_caller.c', 'fixtures/m9/jni-construct/abi_callee.c',
               'fixtures/m9/jni-construct/native/construct.c']
    report = {'schema_version': 1, 'status': 'passed', 'backend': 'Unicorn ARM64 CPU oracle',
              'scope': 'Android NDK Clang caller and actual Horizon GCC callee execute production NewObject/A/V argument paths. GP/FP register exhaustion and stack arguments, narrow promotions, wide/object/array arguments, and pending-exception refusal. Rust dispatch is modeled; no VM/OS/display proof.',
              'caller_compiler': subprocess.check_output([str(tools/'bin/clang.exe'), '--version'], text=True).splitlines()[0],
              'callee_compiler': subprocess.check_output([args.horizon_cc, '--version'], text=True).splitlines()[0],
              'cases': cases, 'source_sha256': {p: hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sources}}
    (out/'result.json').write_text(json.dumps(report, indent=2))
    print('JNI constructor cross-compiler ARM64 ABI oracle: PASS (6 cases)')


if __name__ == '__main__':
    main()
