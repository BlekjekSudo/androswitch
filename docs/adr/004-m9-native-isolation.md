# ADR 004 — M9 isolated native execution and advanced components

## Status

Accepted for implementation; acceptance remains subject to the M9 evidence gate.

## Context

APK native libraries are untrusted ELF, commonly ARM rather than the host ISA.
Calling arbitrary guest code through host `dlopen` violates isolation, exposes the
host libc ABI instead of Android bionic, and makes deterministic fuel impossible.
The workspace forbids unsafe Rust outside the two native crates. Full CPU emulation
is not a reasonable home-grown instruction subset. Unicorn supports x86-64,
AArch64 and ARM/Thumb, but its GPL licensing must not silently change the licensing
of the MIT/Apache Rust binary.

## Decision

Use a separately installed, optional Python/Unicorn **worker process**, controlled
by bounded JSON-lines messages over private stdin/stdout. No host function pointers,
no executable guest mappings in the Rust process and no new unsafe Rust. The worker
receives only explicitly mapped guest memory; guest syscalls and unimplemented
imports fail closed. It is a CPU emulator, not a guest Android OS. Native support
is opt-in (`native` Cargo feature); minimal builds retain typed errors. The worker
requires Python 3 and `unicorn==2.1.4`; it is not linked into the Rust binary.
Unicorn retains its own license. Process separation is an engineering boundary,
not a claim of legal advice or a hardened OS sandbox: Python/Unicorn vulnerabilities
still require an external container/seccomp policy for hostile APKs.

ELF metadata and relocation processing live in safe Rust (`rd-ndk`), JNI identity,
modified UTF-8, descriptors and local-reference rules in `rd-jni`, and Java heap /
callback translation in `rd-vm`. Unsupported relocation/import/JNI operations must
return structured errors rather than fabricated success. Instruction and memory
budgets bound guest execution. The worker watchdog also bounds wall-clock time.

Advanced components use per-Engine deterministic state and the existing virtual
clock. Notifications are keyed by tag/id with channels, actions and listeners;
widgets retain RemoteViews operations; IME edits the focused view with UTF-16
selection semantics; accessibility operates on the existing view tree. Agent tools
use the same state, not a second mock implementation. Missing platform services
remain explicit compatibility gaps.

## Consequences

Native execution requires an extra optional runtime dependency and IPC has overhead.
No claim of complete Android bionic, C++ runtime, JNI or five real APK compatibility
is made without reproducible tests. Generated native fixtures prove ABI behavior;
they do not substitute for the five-real-APK exit gate. Golden evidence records
APK hashes, source/license and the native functions actually executed.
