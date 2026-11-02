# ADR 007 — opt-in Cranelift integer & control-flow JIT (`MethodJit`)

Status: accepted and upgraded from straight-line leaf prototype to full
verified multi-block integer/long/switch/array JIT; opt-in (`RD_JIT=on` or
`Engine::enable_jit`).

## Context and Evolution

The initial ADR 007 prototype (`LeafJit`) compiled only straight-line static
32-bit `int` leaves (`<= 128` instructions, no branches, no loops, no 64-bit
`long`, no `switch`, no primitive arrays, `opt_level="none"`, and per-call heap
allocations in the dispatch path). Real Android compute hotspots — checksums,
hashes, PRNGs, parsers, state machines, sorting/searching, and pixel/byte-buffer
loops — all rely on control flow, 64-bit `long` arithmetic, `switch` tables, or
primitive arrays, causing the prototype to reject them (`unsupported`).

`MethodJit` graduates ADR 007 into a complete, RustyDroid-adapted integer and
control-flow JIT while preserving every safety, determinism, MSRV, and GC
invariant of the original boundary (`LeafJit` remains available for backward
compatibility).

## Architecture and Safety Invariants

1. **Dual-stage verification (`rd-vm` + `rd-jni`):**
   - `crates/rd-vm/src/jit.rs` (`#![forbid(unsafe_code)]`) discovers basic-block
     leaders across `CodeItem.instructions`, prunes unreachable dead blocks and
     trailing switch payloads via BFS from PC `0`, propagates Dalvik register
     types across CFG joins, and lowers 82 Dalvik opcodes into `JitProgram`.
   - `crates/rd-jni/src/jit.rs` independently re-verifies structural bounds
     (`<= 256` registers, `<= 32` parameters, `<= 256` reachable blocks,
     `<= 4,096` total IR ops, `<= 4,096` cached functions) and runs a monotone
     fixed-point dataflow verifier over the lattice
     `{Zero, Int32, LongLo, LongHi, Ref, ArrI32, ArrI64, Uninit}` before
     Cranelift (`opt_level="speed"`) emits machine code.
2. **Full Dalvik control flow and exact per-block fuel accounting:**
   - Supports `goto`/`goto/16`/`goto/32`, `if-eq..if-le` (both `int` and
     reference identity), `if-eqz..if-lez` (including `null` checks),
     `packed-switch` (Cranelift `JumpTableData`), and `sparse-switch`
     (Cranelift `Switch` search tree / jump tables, including negative keys).
   - Every basic block header adds its exact Dalvik instruction count to an SSA
     fuel accumulator and branches to a dedicated `FuelExhausted` exit if the
     window budget (`Engine::fuel_remaining()`) would be exceeded. Hostile
     infinite loops terminate deterministically with `VM_FUEL`, and normal
     returns advance `Engine::fuel_used` by the exact instruction count of the
     interpreter.
3. **64-bit `long` lane, `void`/instance methods, and safe literal division:**
   - Supports `const-wide/*`, `move-wide/*`, `add/sub/mul/and/or/xor-long`
     (3-reg and `/2addr`), `shl/shr/ushr-long` (32-bit `int` shift count masked
     with `& 63` per Dalvik specification), `neg/not-long`, `int-to-long`,
     `long-to-int`, `cmp-long`, `return-void`, and `return-wide`.
   - Supports non-zero literal `div-int/lit16`, `rem-int/lit16`, `div-int/lit8`,
     and `rem-int/lit8`, guarding `lit == ±1` in IR so `i32::MIN / -1` wraps to
     `i32::MIN` and `i32::MIN % -1 == 0` without hardware `#DE` traps. Variable
     register division/remainder (`div-int`, `rem-int`, `div-long`, `rem-long`)
     and `lit == 0` continue to fall back to the interpreter to materialize
     `ArithmeticException`.
4. **Opaque-handle transactional primitive arrays (zero raw guest pointers):**
   - Primitive array parameters (`[Z`, `[B`, `[S`, `[C`, `[I`, `[J`) never pass
     heap addresses into guest registers. Instead, guest registers hold 1-based
     opaque handles (`0` = `null`, `1..=K` = verified host slice table slot),
     deduplicated by `ObjRef` so aliased arguments (`f(a, a)`) share the same
     transactional buffer and preserve `a == b` reference identity.
   - Every `array-length`, `aget*`, and `aput*` checks `1 <= handle <= K` and
     `(idx as u32) < len` in hardware before indexing. On normal return,
     mutated buffers commit back to `Engine.heap`. On `NullPointerException`,
     `ArrayIndexOutOfBoundsException`, or mid-array `VM_FUEL`, uncommitted
     scratch buffers are dropped and `Engine::call` replays in the interpreter
     to produce exact partial writes and exception traces.
5. **Loop-aware tiering and zero-allocation hot dispatch:**
   - Methods containing backward branch edges (loops) compile immediately on
     their first invocation (`threshold = 1`), accelerating cold single-call
     compute kernels; straight-line leaves retain `threshold = 32`.
   - `Engine::invoke_static` maintains an inline cache (`StaticIcSlot`) and
     marshals scalar arguments onto a stack `[u64; 32]` buffer, eliminating
     per-call `String`/`Vec` allocations and DEX table scans.

## Feature and Floor

Cargo feature `jit` remains OFF by default. Enable an engine explicitly via
`Engine::enable_jit()` or `RD_JIT=on`. `cranelift-* =0.104.3` preserves the
workspace Rust `1.76.0` MSRV. Native execution targets 64-bit Cranelift hosts
(validated on `x86_64`); the portable interpreter remains the Moto E5 baseline.

## Paired Benchmark Results (`--release --features jit`, 5-run medians)

All five workloads assert bit-identical return checksums and instruction-exact
`Engine::fuel_used()` parity between interpreter and `MethodJit` (compilation
time is included inside the `MethodJit` wall-clock numbers for loop kernels):

| Workload | Dalvik Fuel | Interpreter (`RD_JIT=off`) | Prototype `LeafJit` (`main`) | **`MethodJit` (`RD_JIT=on`)** | **Speedup vs `main`** |
| --- | ---: | ---: | ---: | ---: | ---: |
| `efficiency_bench jit` (`100k` `sum(II)I` calls) | `200,128` | `103.78 ms` | `91.92 ms` | **`7.10 ms`** | **`12.95x`** (`14.62x` vs interp) |
| IEEE 802.3 CRC-32 (`byte[4096] x 40`, nested loops) | `11,796,760` | `212.45 ms` | `212.45 ms` (unsupported) | **`2.64 ms`** | **`80.46x`** |
| SplitMix64 `long` stream (`409,600` 64-bit rounds) | `3,277,300` | `59.17 ms` | `59.17 ms` (unsupported) | **`1.28 ms`** | **`46.37x`** |
| Bounded Collatz CFG (`4,000` calls, `if-*` + `div/rem`) | `3,016,750` | `43.98 ms` | `43.98 ms` (unsupported) | **`2.45 ms`** | **`17.94x`** |
| `packed-switch` VM state machine (`409,600` steps) | `2,867,500` | `40.28 ms` | `40.28 ms` (unsupported) | **`1.19 ms`** | **`33.71x`** |

## Remaining Interpreter-Only Scope

Floating-point (`float`/`double`) lanes, heap object allocation, field access
(`iget`/`iput`/`sget`/`sput`), nested method calls (`invoke-*` inside a JITted
body), and methods with `try`/`catch` handlers remain in the interpreter.
