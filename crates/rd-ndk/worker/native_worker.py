#!/usr/bin/env python3
"""Private native CPU worker protocol v1. See ADR 004; no guest syscalls.

The Rust client embeds this script. Only stdin/stdout JSON-lines are protocol;
tracebacks and dependency errors go to stderr. Unicorn is an optional dependency.
"""
import json
import struct
import sys
import time

MAX_MESSAGE = 64 * 1024 * 1024
MAX_MEMORY = 256 * 1024 * 1024
PAGE = 4096
STACK = 0x60000000
STACK_SIZE = 2 * 1024 * 1024
TRAPS = 0x70000000
TLS = 0x58000000
TLS_SIZE = 0x10000
STOP = TRAPS + 0x10000


def main():
    from unicorn import Uc, UcError, UC_ARCH_X86, UC_ARCH_ARM64, UC_ARCH_ARM, UC_MODE_64, UC_MODE_ARM, UC_PROT_ALL, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED
    from unicorn.x86_const import UC_X86_REG_FS_BASE, UC_X86_REG_RIP, UC_X86_REG_RSP, UC_X86_REG_RAX, UC_X86_REG_RDI, UC_X86_REG_RSI, UC_X86_REG_RDX, UC_X86_REG_RCX, UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_XMM0
    from unicorn.arm64_const import UC_ARM64_REG_TPIDR_EL0, UC_ARM64_REG_PC, UC_ARM64_REG_SP, UC_ARM64_REG_X0, UC_ARM64_REG_X30, UC_ARM64_REG_Q0
    from unicorn.arm_const import UC_ARM_REG_C13_C0_3, UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_R0, UC_ARM_REG_LR, UC_ARM_REG_C1_C0_2, UC_ARM_REG_FPEXC
    uc = None
    abi = None
    mapped = 0
    event = None
    deadline = 0
    remaining = 0
    total = 0
    pc = sp = ret = 0
    regs = []
    fpregs = []
    fault = {}

    def send(value):
        sys.stdout.write(json.dumps(value, separators=(',', ':')) + '\n')
        sys.stdout.flush()

    def on_code(emu, address, size, data):
        nonlocal event, remaining, total
        remaining -= 1
        total += 1
        if address == STOP:
            event = {'event': 'return'}
            emu.emu_stop()
        elif TRAPS <= address < STOP:
            event = {'event': 'trap', 'slot': (address - TRAPS) // 16,
                     'args': [emu.reg_read(r) & 0xffffffffffffffff for r in regs],
                     'fp': [emu.reg_read(r) & 0xffffffffffffffff for r in fpregs],
                     'sp': emu.reg_read(sp)}
            emu.emu_stop()
        elif remaining < 0 or (total % 1024 == 0 and time.monotonic() > deadline):
            event = {'event': 'error', 'code': 'NATIVE_FUEL', 'cause': 'native instruction/time budget exhausted'}
            emu.emu_stop()
        else:
            # Fail closed on x86 SYSCALL/SYSENTER/int, ARM SVC and AArch64 SVC.
            b = bytes(emu.mem_read(address, size))
            if (abi == 'x86_64' and (b[:2] in (b'\x0f\x05', b'\x0f\x34') or b[:1] == b'\xcd')) or (abi == 'arm64-v8a' and len(b) == 4 and (int.from_bytes(b, 'little') & 0xffe0001f) == 0xd4000001) or (abi == 'armeabi-v7a' and ((len(b) == 2 and b[1] == 0xdf) or (len(b) == 4 and b[3] & 0x0f == 0x0f))):
                event = {'event': 'error', 'code': 'NOT_IMPLEMENTED', 'cause': 'guest syscall/SVC is not permitted'}
                emu.emu_stop()

    def on_unmapped(emu, access, address, size, value, data):
        fault['address'] = address
        fault['pc'] = emu.reg_read(pc)
        return False

    def run(address):
        nonlocal event
        event = None
        fault.clear()
        try:
            uc.emu_start(address, STOP + 4, timeout=5_000_000, count=max(1, remaining + 1))
        except UcError as exc:
            where = ' at pc=%#x addr=%#x' % (fault.get('pc', uc.reg_read(pc)), fault.get('address', 0)) if fault else ' at pc=%#x' % uc.reg_read(pc)
            return {'event': 'error', 'code': 'NATIVE_FAULT', 'cause': str(exc) + where, 'instructions': total}
        if event is None:
            event = {'event': 'error', 'code': 'NATIVE_FUEL', 'cause': 'native execution did not return within budget'}
        event['instructions'] = total
        if event['event'] == 'return':
            event['value'] = uc.reg_read(ret) & 0xffffffffffffffff
            if abi == 'armeabi-v7a':
                event['value'] |= uc.reg_read(regs[1]) << 32
            event['fp_value'] = (uc.reg_read(fpregs[0]) & 0xffffffffffffffff) if fpregs else event['value']
        return event

    while True:
        line = sys.stdin.buffer.readline(MAX_MESSAGE + 1)
        if not line:
            return
        if len(line) > MAX_MESSAGE:
            send({'event': 'error', 'code': 'LIMIT_EXCEEDED', 'cause': 'protocol message too large'})
            return
        try:
            q = json.loads(line)
            op = q['op']
            if op == 'init':
                abi = q['abi']
                if abi == 'x86_64':
                    uc = Uc(UC_ARCH_X86, UC_MODE_64)
                    pc, sp, ret = UC_X86_REG_RIP, UC_X86_REG_RSP, UC_X86_REG_RAX
                    regs = [UC_X86_REG_RDI, UC_X86_REG_RSI, UC_X86_REG_RDX, UC_X86_REG_RCX, UC_X86_REG_R8, UC_X86_REG_R9]
                    fpregs = [UC_X86_REG_XMM0 + i for i in range(8)]
                elif abi == 'arm64-v8a':
                    uc = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
                    pc, sp, ret = UC_ARM64_REG_PC, UC_ARM64_REG_SP, UC_ARM64_REG_X0
                    regs = [UC_ARM64_REG_X0 + i for i in range(8)]
                    fpregs = [UC_ARM64_REG_Q0 + i for i in range(8)]
                elif abi == 'armeabi-v7a':
                    uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
                    uc.reg_write(UC_ARM_REG_C1_C0_2, uc.reg_read(UC_ARM_REG_C1_C0_2) | (0xf << 20))
                    uc.reg_write(UC_ARM_REG_FPEXC, 0x40000000)
                    pc, sp, ret = UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_R0
                    regs = [UC_ARM_REG_R0 + i for i in range(4)]
                    fpregs = []
                else:
                    raise ValueError('unsupported ABI')
                uc.mem_map(STACK, STACK_SIZE)
                uc.mem_map(TRAPS, 0x11000)
                uc.mem_map(TLS, TLS_SIZE)
                tp = TLS + TLS_SIZE // 2
                # bionic stack-protector canary lives in TLS slot 5 (fs:0x28 / tp+0x28).
                uc.mem_write(tp, tp.to_bytes(8, 'little'))
                uc.mem_write(tp + 0x28, (0x5244444e4b4d3921).to_bytes(8, 'little'))
                if abi == 'x86_64':
                    uc.reg_write(UC_X86_REG_FS_BASE, tp)
                elif abi == 'arm64-v8a':
                    uc.reg_write(UC_ARM64_REG_TPIDR_EL0, tp)
                else:
                    uc.reg_write(UC_ARM_REG_C13_C0_3, tp)
                mapped = STACK_SIZE + 0x11000 + TLS_SIZE
                uc.hook_add(UC_HOOK_MEM_UNMAPPED, on_unmapped)
                uc.hook_add(UC_HOOK_CODE, on_code)
                send({'event': 'ok', 'version': 1})
            elif op == 'map':
                n = q['size']
                if n < 0 or mapped + n > MAX_MEMORY:
                    raise ValueError('native mapped-memory budget exceeded')
                uc.mem_map(q['address'], n, UC_PROT_ALL)
                mapped += n
                send({'event': 'ok'})
            elif op == 'write':
                uc.mem_write(q['address'], bytes.fromhex(q['hex']))
                send({'event': 'ok'})
            elif op == 'read':
                n = q['size']
                if n < 0 or n > 16 * 1024 * 1024:
                    raise ValueError('read exceeds limit')
                send({'event': 'ok', 'hex': bytes(uc.mem_read(q['address'], n)).hex()})
            elif op == 'call':
                remaining = min(q.get('fuel', 1000000), 20000000)
                total = 0
                deadline = time.monotonic() + 5
                stack = STACK + STACK_SIZE - 256
                for r in regs + fpregs:
                    uc.reg_write(r, 0)
                ints = floats = 0
                spilled = []
                for a in q['args']:
                    kind, value = a['kind'], a['bits']
                    if fpregs and kind in ('F', 'D'):
                        if floats < len(fpregs):
                            uc.reg_write(fpregs[floats], value)
                            floats += 1
                        else:
                            spilled.append(value)
                    elif abi == 'armeabi-v7a' and kind in ('J', 'D'):
                        ints = (ints + 1) & ~1
                        if ints + 1 < len(regs):
                            uc.reg_write(regs[ints], value & 0xffffffff)
                            uc.reg_write(regs[ints + 1], value >> 32)
                            ints += 2
                        else:
                            if len(spilled) % 2:
                                spilled.append(0)
                            spilled.extend([value & 0xffffffff, value >> 32])
                            ints = len(regs)
                    elif ints < len(regs):
                        uc.reg_write(regs[ints], value & (0xffffffff if abi == 'armeabi-v7a' else 0xffffffffffffffff))
                        ints += 1
                    else:
                        spilled.append(value)
                word = 4 if abi == 'armeabi-v7a' else 8
                stack -= ((len(spilled) * word + 15) // 16) * 16
                for i, v in enumerate(spilled):
                    uc.mem_write(stack + i * word, (v & ((1 << (word * 8)) - 1)).to_bytes(word, 'little'))
                if abi == 'x86_64':
                    stack -= 8
                    uc.mem_write(stack, STOP.to_bytes(8, 'little'))
                elif abi == 'arm64-v8a':
                    uc.reg_write(UC_ARM64_REG_X30, STOP)
                else:
                    uc.reg_write(UC_ARM_REG_LR, STOP)
                uc.reg_write(sp, stack)
                send(run(q['address']))
            elif op == 'resume':
                value = q.get('value', 0)
                uc.reg_write(ret, value & (0xffffffff if abi == 'armeabi-v7a' else 0xffffffffffffffff))
                if abi == 'armeabi-v7a':
                    uc.reg_write(regs[1], value >> 32)
                if 'fp_value' in q and fpregs:
                    uc.reg_write(fpregs[0], q['fp_value'])
                if abi == 'x86_64':
                    stack = uc.reg_read(sp)
                    address = int.from_bytes(uc.mem_read(stack, 8), 'little')
                    uc.reg_write(sp, stack + 8)
                elif abi == 'arm64-v8a':
                    address = uc.reg_read(UC_ARM64_REG_X30)
                else:
                    address = uc.reg_read(UC_ARM_REG_LR)
                send(run(address))
            elif op == 'quit':
                send({'event': 'ok'})
                return
            else:
                raise ValueError('unknown protocol operation')
        except Exception as exc:
            send({'event': 'error', 'code': 'NATIVE_FAULT', 'cause': str(exc)})


if __name__ == '__main__':
    try:
        main()
    except ImportError as exc:
        sys.stderr.write('Native worker dependency missing: install unicorn==2.1.4: ' + str(exc) + '\n')
        sys.exit(2)
