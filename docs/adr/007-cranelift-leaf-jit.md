# ADR 007 — opt-in Cranelift leaf JIT with a narrow native ABI

Status: initial implementation; opt-in, not a complete Dalvik JIT.

## Decision

Start with hot, straight-line, static integer leaf methods only. Reject any
method that allocates, accesses fields, calls out, branches, throws, uses
references/wide values, or has exception handlers. Unsupported methods continue
in the interpreter. This keeps GC and exception behavior unchanged.

`rd-vm` validates Dalvik operand shapes and lowers a bounded integer-only IR.
`rd-jni`, the existing native/unsafe boundary, validates the IR again and uses
Cranelift to compile a function with signature `extern C (const i32*) -> i32`.
Only the checked parameter count can be loaded; there are no guest-controlled
addresses, memory writes, or external calls. Integer add/sub/mul wrap and shift
counts are masked to Java's 0..31. Division/remainder are deliberately excluded.

Native entry and code-memory reclamation require small locally allowed unsafe
blocks in rd-jni, never rd-vm. Function addresses remain private and cannot
outlive the owning JIT module. The safe execute API validates handle/arity.
Methods pay their complete interpreter instruction fuel before native entry;
coverage still records the method. Compilation/code-cache growth is bounded.

## Feature and floor

Cargo feature `jit` is OFF by default. Enable an engine explicitly or set
`RD_JIT=on`. Cranelift 0.104 is chosen for the Rust 1.76 MSRV. Native execution
is supported on Cranelift host architectures (tested x86_64; ARM64 needs device
validation). This is not an ARMv7/Moto E5 JIT claim. The default interpreter
remains portable and is the Moto E5 compatibility baseline.

## Follow-up

Control flow, wider numeric lanes, deoptimization, safepoint maps, host calls,
exceptions and allocation remain interpreter-only. Adding any of those requires
a separate safety/GC review and differential real-APK benchmarks.
