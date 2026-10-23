# ADR 014: shared ELF loader, native TLS boundary and real VM JNI

Status: implemented for the M3/M4 proof; hardware validation pending.

The later session-isolation assumption is superseded by
[ADR 015](015-portable-framework-and-session-boundary.md): standard NRO
chainloading does not create a new Horizon process. M4's process-owned code
must gain verified quiescent teardown before M7 can switch guests safely.

Keep ELF parsing in RustyDroid's portable `rd-ndk` parser. Add `as-native` for
dependency/relocation planning and `NativeBackend` for mapping and imports.
The optional host backend retains the upstream isolated Unicorn worker; Horizon
uses libnx code memory and an AAPCS64 scalar call bridge. No guest Linux kernel,
Python or Unicorn is linked into the device package.

Guest Android TLS and host TLS are distinct. Native entry saves/restores host
TPIDR_EL0, imports use executable veneers to restore it around host callbacks,
and native pthread entry allocates a separate guest TLS instance. libnx's own
thread storage remains on its normal host path. A real process handle is used
for code-memory SVCs; a local IPC copy handles loaders that omit the handle.

JNI function tables use the unmodified AOSP header. Named unimplemented entries
trap only after Rust dispatch frames have returned, so C longjmp never crosses
Rust stack frames. Structured errors identify the required JNI API. Ordinary
Java exceptions use per-thread pending throwable state instead.

The no_std Engine binds an `Rc<dyn NativeHost>`. JNI local references belong to
their attached thread/frame, global references are shared, and method IDs have
session lifetime. Object references root the actual Engine heap. A reentrant C
lease serializes VM access, releasing it during foreign calls to permit joined
thread callbacks and reentry. A boxed Engine provides a stable address. Native
sessions/code stay process-owned, matching the planned fresh-process runner;
arbitrary native-library/thread teardown is not claimed.

The generic mapping technique is attributed to Andy Nguyen and fgsfds, with
its MIT text retained. JNI's header is AOSP Apache-2.0. See
[third-party provenance](../../third_party/README.md). The ABI reference is
[AAPCS64](https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst).
Tests and supported-subset limits are documented in [M2–M4](../M2_M4_E2E.md).
