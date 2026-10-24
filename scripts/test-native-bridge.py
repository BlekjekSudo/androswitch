#!/usr/bin/env python3
"""Execute production ARM64 import/call assembly using host-only Unicorn.

Host errno helpers are modeled as ABI-compliant worst-case register clobbers;
portable C tests separately exercise the implementation helpers. No Horizon or
physical-device execution is implied by this CPU/ABI test.
"""
import argparse
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm64_const import *


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--ndk', default=str(Path.home() / 'AppData/Local/Android/Sdk/ndk/28.2.13676358'))
    parser.add_argument('--errno-object', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = root / 'local-artifacts/m9/native-abi'
    out.mkdir(parents=True, exist_ok=True)
    tools = Path(args.ndk) / 'toolchains/llvm/prebuilt/windows-x86_64/bin'
    elf, binary = out / 'bridge-oracle.elf', out / 'bridge-oracle.bin'
    command = [str(tools / 'aarch64-linux-android26-clang.cmd'), '-nostdlib', '-static',
               '-Wl,-Ttext=0x100000', '-Wl,-e,as_native_import_bridge', '-Wl,--build-id=none',
               str(root / 'platform/switch-smoke/native_call.S'),
               str(root / 'fixtures/m9/native-abi/bridge_probe.S')]
    if args.errno_object:
        command.append(str(args.errno_object))
    subprocess.run(command + ['-o', str(elf)], check=True)
    subprocess.run([str(tools / 'llvm-objcopy.exe'), '-O', 'binary', '--only-section=.text',
                    str(elf), str(binary)], check=True)
    symbols = {}
    for line in subprocess.check_output([str(tools / 'llvm-nm.exe'), '-n', str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            try:
                symbols[fields[2]] = int(fields[0], 16)
            except ValueError:
                pass
    rows = []
    for nested in [False, True]:
        cpu = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
        for address, size in [(0x100000, 0x20000), (0x200000, 0x10000),
                              (0x300000, 0x10000), (0x600000, 0x10000),
                              (0x6ff000, 0x12000)]:
            cpu.mem_map(address, size)
        cpu.mem_write(0x100000, binary.read_bytes())
        host_tp, guest_tp, bridge, stop = 0x600000, 0x700000, 0x6ff000, 0x110000
        sp = 0x30f000
        cpu.mem_write(bridge, struct.pack('<QQQ', host_tp, guest_tp, 0))
        cpu.mem_write(guest_tp-16, struct.pack('<Q', bridge))
        cpu.mem_write(sp, struct.pack('<QQQ', 0xabcdef, 0x123456, 0x987654))
        cpu.reg_write(UC_ARM64_REG_SP, sp)
        cpu.reg_write(UC_ARM64_REG_TPIDR_EL0, guest_tp)
        cpu.reg_write(UC_ARM64_REG_LR, stop)
        cpu.reg_write(UC_ARM64_REG_X16, symbols['oracle_host_target'])
        cpu.reg_write(UC_ARM64_REG_X18, 0x18181818)
        cpu.reg_write(UC_ARM64_REG_X19, 0x19191919)
        for i in range(9):
            cpu.reg_write(UC_ARM64_REG_X0+i, 0x101+i)
        qvalues = [int.from_bytes(bytes([i+1])*16, 'little') for i in range(8)]
        for i, value in enumerate(qvalues):
            cpu.reg_write(UC_ARM64_REG_Q0+i, value)
        outer_return = [None]
        calls = []
        result_q = [int.from_bytes(bytes([0xa0+i])*16, 'little') for i in range(4)]

        def depth():
            return struct.unpack('<Q', bytes(cpu.mem_read(bridge+16, 8)))[0]

        def return_from_host():
            cpu.reg_write(UC_ARM64_REG_X0, 0x5555)
            cpu.reg_write(UC_ARM64_REG_X1, 0x6666)
            cpu.reg_write(UC_ARM64_REG_X18, 0xdead1818)
            for i, value in enumerate(result_q):
                cpu.reg_write(UC_ARM64_REG_Q0+i, value)
            cpu.reg_write(UC_ARM64_REG_PC, cpu.reg_read(UC_ARM64_REG_LR))

        def hook(uc, address, size, user):
            del size, user
            if address in [symbols['as_native_errno_enter'], symbols['as_native_errno_leave']]:
                assert uc.reg_read(UC_ARM64_REG_TPIDR_EL0) == host_tp
                assert uc.reg_read(UC_ARM64_REG_X0) == bridge
                calls.append(('enter' if address == symbols['as_native_errno_enter'] else 'leave', depth()))
                # Caller-saved argument/result registers may all change in C.
                for i in range(19):
                    uc.reg_write(UC_ARM64_REG_X0+i, 0xf000+i)
                for i in range(8):
                    uc.reg_write(UC_ARM64_REG_Q0+i, 0xf000+i)
                uc.reg_write(UC_ARM64_REG_PC, uc.reg_read(UC_ARM64_REG_LR))
            elif address == symbols['oracle_host_target']:
                assert depth() == 1 and uc.reg_read(UC_ARM64_REG_SP) == sp
                assert uc.reg_read(UC_ARM64_REG_TPIDR_EL0) == host_tp
                assert [uc.reg_read(UC_ARM64_REG_X0+i) for i in range(9)] == list(range(0x101, 0x10a))
                assert [uc.reg_read(UC_ARM64_REG_Q0+i) for i in range(8)] == qvalues
                assert bytes(uc.mem_read(sp, 24)) == struct.pack('<QQQ', 0xabcdef, 0x123456, 0x987654)
                if nested:
                    outer_return[0] = uc.reg_read(UC_ARM64_REG_LR)
                    frame = bytearray(432)
                    for i in range(8): struct.pack_into('<Q', frame, i*8, 0x201+i)
                    struct.pack_into('<Q', frame, 416, symbols['oracle_inner_guest'])
                    struct.pack_into('<Q', frame, 424, guest_tp)
                    uc.mem_write(0x200000, bytes(frame))
                    uc.reg_write(UC_ARM64_REG_X0, 0x200000)
                    uc.reg_write(UC_ARM64_REG_LR, symbols['oracle_nested_return'])
                    uc.reg_write(UC_ARM64_REG_PC, symbols['as_native_call_asm'])
                else:
                    return_from_host()
            elif address == symbols['oracle_inner_target']:
                assert depth() == 2 and uc.reg_read(UC_ARM64_REG_TPIDR_EL0) == host_tp
                assert uc.reg_read(UC_ARM64_REG_SP) == sp-320
                assert [uc.reg_read(UC_ARM64_REG_X0+i) for i in range(8)] == list(range(0x201, 0x209))
                uc.reg_write(UC_ARM64_REG_X0, 0x2222)
                uc.reg_write(UC_ARM64_REG_PC, uc.reg_read(UC_ARM64_REG_LR))
            elif address == symbols['oracle_nested_return']:
                assert depth() == 1 and uc.reg_read(UC_ARM64_REG_TPIDR_EL0) == host_tp
                assert uc.reg_read(UC_ARM64_REG_SP) == sp
                assert struct.unpack('<Q', bytes(uc.mem_read(0x200000+384, 8)))[0] == 0x2222
                uc.reg_write(UC_ARM64_REG_LR, outer_return[0])
                return_from_host()
            elif address == symbols['as_native_trap']:
                raise AssertionError('unexpected native trap')

        cpu.hook_add(UC_HOOK_CODE, hook)
        cpu.emu_start(symbols['as_native_import_bridge'], stop, count=10000)
        assert cpu.reg_read(UC_ARM64_REG_PC) == stop
        assert depth() == 0 and cpu.reg_read(UC_ARM64_REG_SP) == sp
        assert cpu.reg_read(UC_ARM64_REG_TPIDR_EL0) == guest_tp
        assert cpu.reg_read(UC_ARM64_REG_X18) == 0x18181818
        assert cpu.reg_read(UC_ARM64_REG_X19) == 0x19191919
        assert [cpu.reg_read(UC_ARM64_REG_X0+i) for i in range(2)] == [0x5555, 0x6666]
        assert [cpu.reg_read(UC_ARM64_REG_Q0+i) for i in range(4)] == result_q
        assert calls == ([('enter', 1), ('enter', 2), ('leave', 2), ('leave', 1)] if nested else [('enter', 1), ('leave', 1)])
        rows.append({'case': 'nested_callback' if nested else 'import_registers_stack_hfa_return', 'status': 'passed', 'errno_helper_calls': calls})
    if args.errno_object:
        errno_cpu = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
        errno_cpu.mem_map(0x100000, 0x20000)
        errno_cpu.mem_write(0x100000, binary.read_bytes())
        errno_cpu.mem_map(0x300000, 0x10000)
        def call_conversion(name, value):
            errno_cpu.reg_write(UC_ARM64_REG_SP, 0x30f000)
            errno_cpu.reg_write(UC_ARM64_REG_X0, value)
            errno_cpu.reg_write(UC_ARM64_REG_LR, 0x110000)
            errno_cpu.emu_start(symbols[name], 0x110000, count=10000)
            assert errno_cpu.reg_read(UC_ARM64_REG_PC) == 0x110000
            return errno_cpu.reg_read(UC_ARM64_REG_X0) & 0xffffffff
        android = [38, 35, 95, 110, 75, 40, 11, 12, 22, 5]
        hosts = [call_conversion('oracle_errno_host_value', i) for i in range(len(android))]
        for host, guest in zip(hosts, android):
            assert call_conversion('oracle_errno_to_android', host) == guest, (host, guest)
            assert call_conversion('oracle_errno_from_android', guest) == host, (host, guest)
        rows.append({'case': 'actual_switch_newlib_errno_translation', 'status': 'passed',
                     'host_codes': hosts, 'android_codes': android})
    report = {'schema_version': 1, 'backend': 'Unicorn ARM64 CPU oracle', 'status': 'passed',
              'scope': 'Production native_call.S with modeled host callback clobbers; optional pure errno conversion code compiled against actual Switch/newlib. Not libnx/emulator/device execution.',
              'errno_header_sha256': hashlib.sha256((root/'platform/switch-smoke/bionic_errno.h').read_bytes()).hexdigest(),
              'assembly_sha256': hashlib.sha256((root/'platform/switch-smoke/native_call.S').read_bytes()).hexdigest(), 'cases': rows}
    (out/'bridge-oracle.json').write_text(json.dumps(report, indent=2))
    print('native ARM64 bridge oracle: PASS (registers, stack, TLS, nested callbacks, x18, four-vector returns)')


if __name__ == '__main__':
    main()
