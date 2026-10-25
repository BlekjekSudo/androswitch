# Real C, C++, and Rust APK execution

## What is implemented

The opt-in `native-engines` feature supplies executing `c-ndk`, `cpp-ndk`, and
`rust-ndk` registry adapters. They share the existing bounded Android DEX → JNI →
ELF → isolated Unicorn worker route; they are not new language interpreters.
MCP `launch.engine` explicitly prepares the VM through the same native adapter
parser. `native_state.selected_engine` identifies that selection. A filename or
ELF signature never identifies a source language or authorizes execution.

The other 25 registered adapters remain `NOT_IMPLEMENTED`. LANG features alone
still select contracts. Default advanced-components and the independent native
and JIT feature gates remain supported. Detection/listing is static and always
reports `execution_claimed=false`, even when an adapter is `AVAILABLE_BOUNDED`.
Only runtime evidence with positive machine-instruction counts claims execution.

## Source-built Android applications

`golden/native-languages/` contains three **project-owned demonstration APKs**,
not third-party applications. Each has a real binary Android manifest, compiled
resources, Java-to-DEX UI glue, and Android-targeted JNI native cores for x86_64,
arm64-v8a, and armeabi-v7a:

- **C:** Android NDK r27c clang, C11.
- **C++:** NDK r27c clang++, C++20 templates and `std::array` headers;
  exceptions and RTTI disabled, no dynamically linked C++ runtime.
- **Rust:** `rustc` Android targets, `no_std`, `panic=abort`, with the NDK linker;
  not an x86 Linux `.so` renamed as an Android library.

`digest(int,int)` consumes two signed integers as eight little-endian bytes,
using FNV-1a seeded by a language tag and returning the low 31 bits.
`nativeFlavor()` returns the language tag. There is **no Java implementation** of
these native methods or fallback arithmetic. The independent Python/Rust test
oracles are outside the APKs. Java provides the Android UI and component glue,
as in an ordinary JNI Android app; the native cores are the three languages
being demonstrated.

Each APK has inputs, compute/persisted result, notification, pinned shortcut,
detail activity, finish control, widget provider, accessibility service, input
method service, and notification listener. They are signed with a generated
**development-only** certificate; never use that key/password in production.

## Reproduce

Build prerequisites: Linux x86_64, JDK (`java`, `javac`, `keytool`), `aapt`,
`apksigner`, `zipalign`, `unzip`, Python 3, and Rust with these targets:

```sh
rustup target add x86_64-linux-android aarch64-linux-android armv7-linux-androideabi
python golden/native-languages/build.py --out /tmp/native-language-apks
python -m pip install unicorn==2.1.4
cargo build -p rd-cli --features native-engines
./target/debug/rd engine check rust-ndk --json
./target/debug/rd engine run rust-ndk /tmp/native-language-apks/native-rust.apk --json
```

The builder downloads SHA-256-pinned official platform 26, NDK r27c, and R8
8.5.35 inputs. It compiles with minSdk 26/targetSdk 34, verifies APK v2/v3
signatures with `apksigner`, and emits `provenance.json` containing source,
compiler, APK, library, and ELF-machine evidence. Downloads, private debug key,
classes, `.so` files, and APKs remain outside Git under cache/target/output.
The runtime's MSRV remains Rust 1.76; the builder records the actual compiler.

Run every MCP tool positively for each APK and each guest ABI:

```sh
for abi in x86_64 arm64-v8a armeabi-v7a; do
  python scripts/native-languages-mcp.py \
    --rd target/debug/rd --apks /tmp/native-language-apks \
    --out /tmp/native-language-evidence/$abi --abi $abi
done
```

This is a real newline-delimited JSON-RPC MCP stdio client, not direct handler
calls. It obtains the server's tool catalog, requires exactly 47 tools (the catalog of `rd mcp`, see docs/mcp-tools.md), checks
argument names/required fields, rejects unsuccessful assertions, and requires
**positive coverage of all 47 for every language/ABI**. Expected negative errors
are counted separately and cannot satisfy positive coverage. Outputs include
raw request/response JSONL, per-tool positive/negative counts, native instruction
counters/ABI, APK SHA-256, PNG screenshots, and machine-readable summaries.

The campaign independently checks signed integer extremes, input through
keyboard/IME/accessibility, actual compute callbacks and logs, screenshots,
service activation, widget bind/render/delete, notification post/click/cancel,
shortcut publication/launch, detail navigation, forced major GC while Main is
suspended, BACK restoration, another native computation after restoration,
warm launch with persisted result, finish, package clear/remove, permissions,
coverage, batching, and diagnostics. Unsupported engines, invalid arguments,
missing methods/views, and sandbox escape fail with typed errors. A failed
method lookup must not increment native instruction counts.

The public adapter parse/event/syscall contract has a separate real-APK gate:

```sh
RD_NATIVE_LANGUAGE_APKS=/tmp/native-language-apks \
  cargo test -p rd-engine-registry --features native-execution \
  --test native_execution -- --ignored --nocapture
```

Run it once per ABI using `RD_NATIVE_ABI`, or use the MCP matrix above. The
external-APK test is explicitly ignored by ordinary offline unit tests; the
availability/metadata tests and VM history/dispatch tests run without the SDK.

The gate also runs the public `engine run --json` CLI regression test for all
three languages on each ABI. It requires one clean JSON artifact on stdout,
guest output captured in `logs`, an executing native report, and the expected
UI text. Missing source APK fixtures fail the explicitly requested test rather
than silently skipping it. Ordinary unit tests independently cover invocation
validation at the unchanged 16 KiB payload and 16 signed-integer argument
boundaries, including malformed JSON, duplicate/unknown fields and overflow.

## MCP selection

After `install`, call:

```json
{"name":"launch","arguments":{"engine":"rust-ndk"}}
```

Use `c-ndk` or `cpp-ndk` for the other source-built packages. No selection means
the existing generic Android route. A warm launch preserves a selected backend
for the same running package; switching explicit backend rebuilds the process.
Unsupported adapters and builds without `native-engines` return
`NOT_IMPLEMENTED`; unknown IDs return `UNKNOWN_ENGINE`. They never fall back to
another language adapter. `native_state` is diagnostic; `invoke` performs method
calls. `runtime_health` with `gc=true` forces major collection.

## Runtime fixes supporting the proof

- D8-inherited Android method owners resolve through bounded/cycle-checked
  platform ancestors after DEX override resolution. Concrete View/TextView/
  Context handlers precede generic property storage, preserving IDs and text.
  Direct VM invocation shares that host ancestor resolver.
- Context navigation has bounded (64 suspended entries) single-task history,
  strong GC roots, lifecycle dispatch, focus/IME restoration, failed-launch
  recovery, and idempotent finish/BACK. Explicit MCP root relaunch replaces old
  history. Actual heap activity class drives MCP activity/health metadata.
- Snapshot and widget screenshots now use the VM Canvas embedded antialiased
  glyph atlas instead of checkerboard pseudo-text. They remain headless
  snapshots, not pixel-perfect Android theme rendering.
- CLI `native` forwards to the agent too. The previous missing forwarding could
  make an executing native VM appear disabled through MCP diagnostics.
- `engine run --json` captures app stdout instead of corrupting the JSON output;
  APK input is bounded to 256 MiB, including a growing-file read bound.
- Deliberately corrupted native payloads fail closed in all nine language/ABI
  combinations; a launch error cannot print success JSON or fallback output.

## Explicit limits

This demonstrates **bounded real execution**, not universal C/C++/Rust Android
compatibility. It is not a complete Android OS, NDK/libc/JNI implementation,
complete STL/C++ exception runtime, Rust `std`/async runtime, arbitrary host
syscall bridge, or graphical GPU implementation. The Java UI/components run in
the project's headless Android model. Guest machine code runs in Unicorn on
this host, not on a physical Android phone. Task history is single-task and does
not implement all Android launch modes, tasks, process restoration, or services.
Actual hardware installation and device testing are separate from this gate.
The explicit native adapter launcher currently rejects activity aliases; use the
generic MCP route for those APKs. The GitHub Actions job is added here; local
execution is not evidence that remote CI has run or passed.

Source attribution comes from the committed source/build provenance, **not**
from guessing a language from ELF. A successful demonstration APK does not
establish compatibility with every third-party APK written in that language.
