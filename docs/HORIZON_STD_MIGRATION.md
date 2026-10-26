# Horizon std migration — 26 October 2026

AndroSwitch now builds its device runtime with the real Rust standard library.
This replaces the product's core/alloc-only build profile. It preserves the
existing embedded execution state, JNI object identities, guest/host TLS
boundary, persistent Java/native filesystem and console/media runner.

RustyDroid by **nadabgfccvd and the RustyDroid contributors** remains the runtime
foundation. The isolated `as-vm` Cargo package compiles `rd-vm/src/lib.rs`
directly; it contains no copied VM. See [credits](../CREDITS.md).

## Library support and Android execution are separate

`std` supplies Rust filesystem, clocks, collections, synchronization and threads.
`desktop-runtime` selects the inherited desktop execution backend. The latter
exists only in `rd-vm`; `as-vm` cannot enable it through Cargo feature unification.
Both packages compile the same original source tree. Default AndroSwitch
framework, storage, native, package, launcher and runner features enable std.

The full inherited `upstream-runtime` reference graph remains an explicit
feature. Migrating Rust library support does not connect that graph to every
live Android object/platform service. Complete upstream Android API behavior
parity and Mekorama/Candy Crush first frames remain separate work. The
[integration ledger](UPSTREAM_PARITY_PLAN.md#platform-integration-ledger)
retains those requirements.

## Pinned platform adaptation

- Compiler/source: `787af2b8c80638c51a4fc8e44f84e6891f243ec7`, distributed as
  nightly-2026-08-26; custom `aarch64-androswitch-newlib` target, panic=abort.
- `prepare-horizon-std-toolchain.py` validates the original source hashes and
  creates a task-local sysroot. Compiler artifacts are immutable hardlinks;
  Rust sources are independent copies. No global default or installed source
  is changed.
- Two std files change: `sys/fs/unix.rs` copies file contents without claiming
  Unix chmod exists on SD, and `sys/thread/unix.rs` queries the actual process's
  allowed core mask through the PAL. Explicit chmod remains unsupported.
- Pinned libc 0.2.189 is independently extracted and its newlib layouts adapted
  to the SDK. The probe compares 33 sizes, alignments, offsets and constants
  against C compiled with the actual SDK.
- `platform/horizon-std/support.c` uses real libnx/newlib services. It repairs
  SDK default joinability, reclaims detached pthreads with a dedicated reaper,
  queries `InfoType_CoreMask`, and supplies real CSRNG entropy. Creation is
  bounded at 256 live tracked pthreads and returns EAGAIN on exhaustion.
- Reclamation depends on the pinned libnx `__pthread_t` having its public
  `Thread` as its first member, as in
  [libnx newlib.c](https://github.com/switchbrew/libnx/blob/master/nx/source/runtime/newlib.c).
  SDK archive hashes accompany the probe build. Revalidate this private layout
  and the public ABI tests on every SDK update.
- Mesa's pinned shader compiler retains its separately versioned GNU/Linux
  Rust runtime and existing bounded SDK adapters. `prepare-mesa-std-link.py`
  namespaces its exception personality and DWARF references in copied
  archives. It does not ignore duplicate symbols or mix Rust crate versions.

There is no Linux guest kernel, Python, Unicorn or desktop process backend in
the NRO. Android files still pass through the common guest VFS; raw Rust std
paths do not establish Android permissions, storage mounts or lifecycle.
Networking and panic unwinding are outside this offline qualification.

## Reproduce

Use an already installed matching compiler and rust-src; `-Toolchain` accepts
its local selector. These scripts do not install global components.

```powershell
./scripts/build-horizon-std.ps1
./scripts/test-horizon-std-ryujinx.ps1 `
  -Nro local-artifacts/horizon-std/candidate/androswitch-std-probe.nro

./scripts/build-androswitch.ps1 -Toolchain nightly -LinkBackend Windows `
  -MesaSdk local-artifacts/dependencies/mesa-switch-0ff52079/sdk/opt/devkitpro/portlibs/switch

cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked -- --test-threads=1
```

The unified Mesa build also requires LLVM objcopy (default: Android NDK
28.2.13676358); use `-MesaObjcopy` for an explicit path. Build receipts retain
the compiler, copied source patches, SDK inputs and executed artifact hashes.
The historical parser smoke builder delegates its Rust build to this same
std profile. It remains a developer proof, separate from the product NRO.

## Verification

The evidence record is being completed alongside this migration. Host,
Ryujinx and physical-device results are recorded separately. A standalone
32-contract probe is finite smoke coverage, not complete std certification;
the product regression corpus is an additional requirement. The earlier
29/32 [experiment report](HORIZON_STD_REPORT.md) remains historical evidence.
The user's earlier 219/219 physical Switch result applies to its original NRO,
not to this new std build.
