.text
.global direct_linux_syscall
.type direct_linux_syscall,%function
direct_linux_syscall:
    mov x8,#172
    svc #0
    ret
.size direct_linux_syscall,.-direct_linux_syscall
