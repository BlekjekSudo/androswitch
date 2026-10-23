# ADR 011 — portable DEX parser and smoke FFI

Date: 2026-10-23. Status: implementation in progress (partial M1).

Port the actual upstream `rd-dex` parser to `no_std + alloc` while retaining
its default `std` host behavior. The device parser eagerly builds a BTreeMap
class index, avoiding host OnceLock/HashMap dependencies and thread-local state.
The host retains its measured lazy HashMap. Both preserve first-definition-wins.
I/O error conversion is host-only; parse errors remain available on-device.

`as-switch-smoke` is a small dedicated FFI boundary, with unsafe denied except
at documented exports, buffer conversions and the allocator callbacks. The C
caller owns valid input/output buffers; Rust clones input before parsing and
retains no raw pointers. A 64 MiB cap applies to smoke DEX inputs. Device panic
and allocator exhaustion terminate the smoke process instead of unwinding into C.

The smoke NRO tests allocation, real file input, monotonic timing and the real
DEX parser. It does not execute bytecode or APKs. It cannot complete M1 without
the remaining APK-parser/interpreter port and emulator output comparison.
