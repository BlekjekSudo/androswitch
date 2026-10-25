# RustyDroid reuse audit

Date: 2026-10-26. Scope: source-routing audit of the frozen RustyDroid base
`6388281f66d254c8e01a0fdc7a5a9a30239bc762` and the current AndroSwitch device
path. This is an initial family inventory, not an Android API coverage count or
an execution claim.

The latest upstream source was also inspected at
[`3761815dc6a2a1427e9fc66b66e8471b78d1b170`](https://github.com/nadabgfccvd/RustyDroid/commit/3761815dc6a2a1427e9fc66b66e8471b78d1b170).
It was fetched for comparison; the frozen fork base is unchanged. Neither that
revision nor the frozen base implements EnumMap, EnumSet or sigaltstack. The
latest tree contains one EnumMap comment, which is not an implementation.
Its much larger framework surface needs the same state/GC/platform audit before
integration; merging generated service surfaces does not establish behavior.

## Why an implemented upstream class can still fail on Switch

The shared interpreter, classpath, heap, DEX reader and APK readers are inherited
from RustyDroid. However, `rd-vm/src/lib.rs` selects different intrinsic dispatch
modules using `cfg(feature = "std")`: the inherited `intrinsics.rs` for desktop,
and `portable/intrinsics.rs` for the device. Many inherited Java/Android modules
and `Engine::fw` are also behind `std`. The device binds `FrameworkHost` to
`as-framework`, whereas the inherited modules use `framework::HostState` and
`HostObj` directly. See [ADR 012](adr/012-portable-interpreter.md) and
[ADR 015](adr/015-portable-framework-and-session-boundary.md).

Consequently, an unresolved descriptor can be a **porting/integration gap** even
when its upstream implementation is present in the checkout. `no_std` requires
separating OS dependencies; it does not require recreating every Java class.
The separate path helped establish the initial device build, but also left
reusable upstream behavior outside that build. Adding isolated replacements
without first inspecting upstream creates avoidable duplication.

## Initial family inventory

Paths below are relative to `crates/rd-vm/src` unless otherwise specified.
Presence means source exists; only the explicitly tested methods have verified
behavior. Constructors, overloads, reflection metadata and GC edges must also
reach the device path before an API is usable there.

| Family | Inherited source / assumptions | Device integration and reuse direction |
| --- | --- | --- |
| Interpreter and parsing | `engine.rs`, `interp.rs`, `classpath.rs`, `heap.rs`; `rd-dex` and `rd-apk` | Already shared. Keep fixes here when they are interpreter/parser semantics. Separate GC and some execution state still need their own evidence. |
| Atomic references and integer atomics | `intrinsics.rs` implements these using heap fields, under a single-threaded VM assumption | Device `portable/atomic*.rs` has GC/callback/thread contracts. Extract compatible field/value helpers rather than treating the class as absent upstream; preserve the device VM lease and memory ordering. |
| Primitive wrappers | `boxes.rs` already implements all eight wrapper families using heap fields | Now compiled by both VM feature paths; portable dispatch and framework adapters delegate to this same source. Duplicate device allocation/conversion/equality code has been removed. A bounded per-Engine valueOf cache is traced by both collectors; exact method/static-field metadata is shared with device JNI lookup. A single JVM/DEX corpus exercises both builds. Verification is tracked separately from source presence. |
| Android logging | `android_text::log_intrinsic` implements log levels, Throwable summary overloads and println under `std` | Extracted dispatch/formatting to shared `android_log`; desktop host_print and Horizon's actual bounded file sink are adapters. Exact argument pins survive callback GC and are released on errors. Tag-threshold policy remains adapter supplied. Throwable output retains upstream's summary scope; full stack frames, getStackTraceString on device and fatal wtf handlers are not implemented. |
| Math / numeric utilities | `numerics.rs`, intrinsic dispatch; formatting caches and helpers include `std` dependencies | Radix/decimal parsers were extracted to `primitive_parse.rs` for both builds. The rest of numerics and portable Math/Formatter are not yet unified. Audit and extract remaining pure algorithms; isolate caches and transcendental/platform dependencies. Do not infer full Math coverage from min/max tests. |
| Pair and other value objects | `android_util.rs` already contains Pair, Size, Range and other families | Device `as-framework/src/services/pair.rs` covers a bounded Pair subset. Reuse compatible value-object logic; descriptor metadata, Java callbacks and roots still require the device adapter. Other classes in the upstream module are not thereby supported. |
| Collections and concurrent maps | `intrinsics.rs`, `jcollections.rs`, `collections.rs`; ConcurrentHashMap reuses host Map state. Comments explicitly defer some methods and describe snapshot collection views | Device collections use separate state. Port by family, preserving callback/GC ownership. The current bounded device concurrent map is not evidence of lock-free/full Java concurrent-map behavior. Do not blindly copy sequential assumptions or snapshot views into an API that promises different semantics. |
| Copy-on-write list/set | Upstream `set_intrinsic` supplies constructor deduplication and base Set operations, using sequential HostObj state | Set-constructor deduplication is extracted into shared `collection_values.rs`, consumed by upstream and portable adapters. Device Set operations reuse the existing COW list's immutable generations, writer lock, iterator and GC engine; add forwards to addIfAbsent. No second backing store was introduced. Twenty additional JVM scenarios cover the supported Set subset; bulk methods and equality/hash APIs remain explicit gaps. |
| Executors and thread-local state | `intrinsics.rs`, `threads.rs`, `jlocks.rs`; executor tasks run inline and thread-local behavior assumes the inherited scheduler | Device executors and native workers use actual thread/callback admission. Reuse compatible dispatch/argument logic, then adapt scheduling and lifetime explicitly. Inline execution is not a general replacement for Android threading. |
| Reflection / class loading | `jlclass.rs`, `reflect.rs`, `jlsystem.rs` and intrinsic class-object helpers | Device `portable/classes.rs` and `as-framework/src/services/reflection` bind real DEX metadata and its own class/loader identities. Share descriptor and access algorithms after checking class initialization, JNI roots and loader scope. |
| Resources, Bundle and package metadata | `android_res.rs`, `bundle.rs`, `android_pm.rs` | Reuse shared parsers and compatible value semantics. Upstream `android_pm.rs` documents null provider resolution; the installed-registry provider query requires actual AndroSwitch package state and cannot be replaced with that result. |
| File APIs, SQLite, graphics and native ABI | `jfile.rs`, `jsqlite.rs`, `graphics.rs`, native/JNI crates and host adapters | Share parsing, API semantics and tests. Native fsync/fdatasync now use one VFS/FileSystem sync contract, mapped to host sync_all and the same libnx filesystem commit already used by durable writes. Unimplemented backends return an error. Desktop I/O, subprocesses or Linux calls cannot execute unchanged on Horizon. |

Upstream source also documents incomplete semantics: executors execute inline,
some collection views are snapshots, and ConcurrentHashMap uses sequential Map
storage. Those limitations are reasons to adapt the relevant behavior, not
evidence that RustyDroid implements the whole Android framework. They also do
not establish that every AndroSwitch replacement needed a new implementation.

## Required investigation before expanding a missing API

Two genuinely absent families are being added after this comparison. EnumMap
uses the existing Collections map storage, budget and collector hooks, with
enum key validation and three constructor forms layered on top. The implemented
subset is size/isEmpty/clear, get/remove/containsKey/containsValue, put and
putAll for implemented map sources. Live views, clone and serialization remain
unavailable. The same SDK fixture harness supplies 26 JVM observations in two
namespaces, before/after GC; this is not another standalone APK workflow.

Android LP64 signal-set bit operations use an explicit eight-byte layout.
Horizon cannot currently deliver Linux signals: sigaltstack, sigaction, signal,
sigprocmask, sigpending and pthread_sigmask return ENOSYS/SIG_ERR (pthread APIs
return the positive Android error without changing errno). Outputs stay intact;
no handler is registered in the host. This exposes a normal native ABI failure
path, not an implementation of Android signal delivery. Pure memory operations
and unavailable delivery are tested separately. ABI references are the pinned
NDK headers and [Bionic's signal source](https://android.googlesource.com/platform/bionic/%2B/main/libc/bionic/signal.cpp).

1. Search the inherited implementation and its tests for the exact descriptor
   and its class family. Record whether the failure is dispatch/metadata,
   excluded source, different object state, missing platform service, or missing
   behavior in upstream itself.
2. Prefer extracting compatible, OS-independent algorithms/state into shared
   `core`/`alloc` modules consumed by both dispatch paths. Keep actual device
   services behind platform interfaces. Avoid a fallback to the entire desktop
   framework: it uses a different object-state and GC contract.
3. Reuse upstream test vectors and add differential Android/JVM observations
   where behavior is ambiguous. Check constructors, overloads, exceptions,
   reentrant callbacks, GC and relevant threading, rather than only class-name
   presence. Preserve inherited host behavior until a separately reviewed
   semantic correction is supported by evidence.
4. Expand by coherent families and report their actual method coverage.
   Implement new behavior only where reuse/adaptation does not supply it.
   Keep unavailable operations explicit. Applications remain integration
   probes; package checks and game-specific hooks remain prohibited.

This audit does not establish a percentage of Android compatibility or the
remaining time to a first frame. The 219-case conformance corpus covers its
listed contracts only; it does not automatically exercise all inherited modules
or every API used by an external APK.
