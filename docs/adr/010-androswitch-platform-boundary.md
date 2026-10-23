# ADR 010 — AndroSwitch platform and verification boundaries

Date: 2026-10-23. Status: accepted in the user-approved implementation plan.

Use a history-preserving RustyDroid fork with a separate AndroSwitch facade/CLI.
Keep original tools/docs identifiable. Initial environment: API 26, native ARM64,
offline, one active app. No package-specific runtime exceptions or profiles.

The official freestanding Rust Switch target has no supported `std`. Use
`no_std + alloc` and explicit libnx adapters. Keep host configuration/process
dependencies out of the device graph. Platform contracts are portable; host
implementations are feature-gated. FFI crates may use locally documented unsafe
code under their own `deny` lint, without weakening safe workspace crates.

Unicorn/Python are host oracles, never device dependencies. Track host, Horizon
emulator and physical verification separately. M1 cannot pass on an arithmetic
demo alone: its gate also needs actual APK/DEX/parser/interpreter and
file/time/allocator paths through the device build, agreeing with host.
