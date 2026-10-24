/* AAPCS64 scalar call bridge. Host TP is restored before every host callback.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
.text
.align 4
.global as_native_call_asm
.type as_native_call_asm,%function
as_native_call_asm:
    stp x29,x30,[sp,#-48]!
    stp x19,x20,[sp,#16]
    mrs x20,tpidr_el0
    str x20,[sp,#32]
    mov x19,x0
    sub sp,sp,#256
    add x9,x19,#128
    mov x10,sp
    mov x11,#16
1:  ldp x12,x13,[x9],#16
    stp x12,x13,[x10],#16
    subs x11,x11,#1
    b.ne 1b
    ldr x9,[x19,#424]
    ldr x10,[x9,#-16]
    str x20,[x10]
    msr tpidr_el0,x9
    ldr x16,[x19,#416]
    ldp d0,d1,[x19,#64]
    ldp d2,d3,[x19,#80]
    ldp d4,d5,[x19,#96]
    ldp d6,d7,[x19,#112]
    ldp x0,x1,[x19]
    ldp x2,x3,[x19,#16]
    ldp x4,x5,[x19,#32]
    ldp x6,x7,[x19,#48]
    blr x16
    msr tpidr_el0,x20
    stp x0,x1,[x19,#384]
    stp d0,d1,[x19,#400]
    add sp,sp,#256
    ldp x19,x20,[sp,#16]
    ldp x29,x30,[sp],#48
    ret
.size as_native_call_asm,.-as_native_call_asm
.global as_native_import_bridge
.type as_native_import_bridge,%function
as_native_import_bridge:
    mrs x17,tpidr_el0
    ldr x17,[x17,#-16]
    ldr x15,[x17,#16]
    cmp x15,#128
    b.hs 3f
    add x14,x17,#24
    str x30,[x14,x15,lsl#3]
    add x14,x17,#1048
    str x19,[x14,x15,lsl#3]
    add x14,x17,#2584
    str x18,[x14,x15,lsl#3]
    add x15,x15,#1
    str x15,[x17,#16]
    mov x19,x17
    ldr x14,[x17]
    msr tpidr_el0,x14
    // Convert thread-local errno without changing the argument ABI. Restore
    // SP before the imported call so stack and variadic arguments are intact.
    sub sp,sp,#208
    stp x0,x1,[sp]
    stp x2,x3,[sp,#16]
    stp x4,x5,[sp,#32]
    stp x6,x7,[sp,#48]
    stp x8,x16,[sp,#64]
    stp q0,q1,[sp,#80]
    stp q2,q3,[sp,#112]
    stp q4,q5,[sp,#144]
    stp q6,q7,[sp,#176]
    mov x0,x19
    bl as_native_errno_enter
    ldp q0,q1,[sp,#80]
    ldp q2,q3,[sp,#112]
    ldp q4,q5,[sp,#144]
    ldp q6,q7,[sp,#176]
    ldp x0,x1,[sp]
    ldp x2,x3,[sp,#16]
    ldp x4,x5,[sp,#32]
    ldp x6,x7,[sp,#48]
    ldp x8,x16,[sp,#64]
    add sp,sp,#208
    adr x30,2f
    br x16
2:  sub sp,sp,#80
    stp x0,x1,[sp]
    stp q0,q1,[sp,#16]
    stp q2,q3,[sp,#48]
    mov x0,x19
    bl as_native_errno_leave
    ldp x0,x1,[sp]
    ldp q0,q1,[sp,#16]
    ldp q2,q3,[sp,#48]
    add sp,sp,#80
    mov x17,x19
    ldr x15,[x17,#16]
    sub x15,x15,#1
    str x15,[x17,#16]
    add x14,x17,#24
    ldr x30,[x14,x15,lsl#3]
    add x14,x17,#1048
    ldr x19,[x14,x15,lsl#3]
    add x14,x17,#2584
    ldr x18,[x14,x15,lsl#3]
    ldr x16,[x17,#8]
    msr tpidr_el0,x16
    ret
3:  ldr x14,[x17]
    msr tpidr_el0,x14
    mov x0,#1008
    neg x0,x0
    b as_native_trap
.size as_native_import_bridge,.-as_native_import_bridge
/* TLSDESC resolver ABI preserves every register except x0 and NZCV. */
.global as_native_tlsdesc
.type as_native_tlsdesc,%function
as_native_tlsdesc:
    ldr x0,[x0,#8]
    ret
.size as_native_tlsdesc,.-as_native_tlsdesc
