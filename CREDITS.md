# Credits and source provenance

## RustyDroid — the foundation

**AndroSwitch is built on RustyDroid, not a runtime written from scratch.**
The primary upstream credit belongs to
[nadabgfccvd](https://github.com/nadabgfccvd) and the
[RustyDroid contributors](https://github.com/nadabgfccvd/RustyDroid/graphs/contributors).

| Item | Provenance |
| --- | --- |
| Original repository | [nadabgfccvd/RustyDroid](https://github.com/nadabgfccvd/RustyDroid) |
| Fork base | [`6388281f66d254c8e01a0fdc7a5a9a30239bc762`](https://github.com/nadabgfccvd/RustyDroid/commit/6388281f66d254c8e01a0fdc7a5a9a30239bc762) |
| Latest synchronized upstream | [`3761815dc6a2a1427e9fc66b66e8471b78d1b170`](https://github.com/nadabgfccvd/RustyDroid/commit/3761815dc6a2a1427e9fc66b66e8471b78d1b170) |
| Base commit date | 22 October 2026 |
| Original copyright | Copyright (c) 2026 RustyDroid contributors |
| License | [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) |
| History | Original commits and authors retained as ancestors of this fork |

The inherited work includes APK ZIP/AXML/ARSC/manifest parsing, DEX parsing
and decoding, the Dalvik interpreter, Engine, classpath, heap, exception/value
models, host framework implementations, native/JNI research backends, tools,
tests and design documents. The `rd-*` names and upstream authorship remain.

M1 specifically ports and shares the existing interpreter and heap. It does
not substitute a second demonstration interpreter and present it as the
original runtime. The Switch-specific feature boundaries and adapters are
AndroSwitch changes on top of that foundation.

The original documents are preserved at the latest synchronized revision:

- [RustyDroid README](README.rustydroid.md).
- [RustyDroid contributing guide](CONTRIBUTING.rustydroid.md).
- [RustyDroid security policy](SECURITY.rustydroid.md).
- [RustyDroid specification](RustyDroid.md) and
  [upstream roadmap](docs/ROADMAP.md).

The [complete source manifest and porting audit](docs/UPSTREAM_PORTING.md) track
latest source integration separately from device behavior. Shared implementations
remain RustyDroid-derived code with the original authorship and license.

The original logo is retained with the upstream material; it is not presented
as AndroSwitch branding. The original license files are unchanged. Keep these
notices and source attribution when redistributing or extending the project.

## AndroSwitch additions

[BlekjekSudo/androswitch](https://github.com/BlekjekSudo/androswitch) maintains
the Horizon port and its own evidence. Current additions include:

- Shared portable interfaces, a Horizon Rust std PAL and a libnx C ABI boundary.
- Per-Engine portable state and a collector for that state graph.
- Host inspection tools, the bounded method-probe protocol and the Ryujinx
  harness with JVM comparisons and recorded failures.
- Persistent package revisions and one Java/native filesystem adapter.
- A shared ARM64 loader and libnx backend, with real VM-backed JNI and
  Java/Kotlin/C/C++/Rust execution fixtures.
- The Switch roadmap, compatibility inventory and source/fixture receipts.

Individual contributions are attributed through Git history. AndroSwitch
additions use the same **MIT OR Apache-2.0** license choice. Current Switch
compatibility claims belong to this fork's reports, rather than the upstream
Linux documentation.

## Optional Mesa Switch renderer

The optional unified graphics build uses [danfromtico/mesa-switch](https://github.com/danfromtico/mesa-switch)
at `0ff52079ca9e9e837ed27eb95a8d239f439a1864`, based on Mesa 26.2.4, by
Mesa, Nouveau, NVK, Collabora, NVIDIA and Mesa Switch contributors. It retains
their licenses and provenance. It provides the actual Horizon EGL/NVC0/NVK
driver implementations; AndroSwitch supplies Android ABI/lifetime/TLS bridges.
The SDK's shader compiler requires its matching Rust 1.101.0 nightly component.
This is separate from the RustyDroid VM's Horizon/newlib std. The Mesa archives'
exception personality is namespaced in build-local copies so compiler versions
do not accidentally share that ABI.
See [renderer port details](docs/MESA_SWITCH_PORT.md) for pinned artifacts,
platform adapters, limits and verification; a successful link is not a GPU pass.

## Rust standard library and Horizon PAL

Rust project contributors provide the actual `std`, `core` and `alloc`
implementations under MIT OR Apache-2.0. The device compiler and source are
pinned at `787af2b8c80638c51a4fc8e44f84e6891f243ec7`. AndroSwitch adapts two std
source files in an independent, task-local source overlay and adapts
rust-lang/libc 0.2.189's newlib ABI definitions. Installed toolchains and Cargo
registry sources are unchanged. devkitPro/newlib and switchbrew/libnx
contributors provide the SDK heap, pthread/TLS, clock and filesystem services.
AndroSwitch adds detached-thread reclamation, real process core queries and
CSRNG transport. See [the migration report](docs/HORIZON_STD_MIGRATION.md).

## Android SDK declaration metadata

The supplemental type declarations are derived from the locally installed
Android SDK API 35 `android.jar`, credited to the Android Open Source Project
and Android contributors. Only type names, inheritance, interfaces, modifiers
and inner-class names are generated; no executable SDK method bodies or JAR
are redistributed. [The input receipt](data/android-sdk-types.json) pins its
SHA-256 and [the generator](scripts/generate-android-types.py) is reproducible.
This declaration source does not change the API 26 behavior profile and does
not imply those classes' methods work. RustyDroid's original hierarchy remains
authoritative where SDK stubs hide internal parent classes.

## Platform, tooling and test sources

M8 adds native **SQLite 3.54.1**, by the SQLite authors/contributors (public
domain), compiled from its hash-pinned original amalgamation. See
[SQLite notice](third_party/SQLite-NOTICE.txt). XML preferences use
**xmlparser 0.13.6**, by Yevhenii Reizner and contributors, MIT OR Apache-2.0;
its original license files are included in packaged dependency notices.
SharedPreferences behavior is referenced against AOSP Android 8.0.0_r1
`SharedPreferencesImpl` (Android Open Source Project, Apache-2.0).
The shared broadcast/service and locale contracts also use Android 8.0.0_r1
framework/Bionic as behavior and ABI references. The new implementations are
AndroSwitch code; pinned reference links and tested limits accompany their
receipts. The task-local emulator diagnostic retains Ryujinx's MIT attribution;
its source patch and reproducible tooling are documented in
[tools/ryujinx-cfg](tools/ryujinx-cfg/README.md). Emulator binaries and keys are
excluded from the repository and release package.
The M9 external evaluation downloads an unmodified Vector Pinball APK from
F-Droid, credited to dozingcat/Vector Pinball contributors (GPL v3);
that APK is not bundled in the AndroSwitch distribution.

| Project / contributors | Use in AndroSwitch |
| --- | --- |
| [Rust](https://github.com/rust-lang/rust) and its contributors | Language, compiler, `core` and `alloc` |
| [libnx](https://github.com/switchbrew/libnx) / switchbrew contributors | Horizon services and the homebrew C platform boundary |
| [Autorun](https://github.com/autorunhq/autorun) / autorunhq contributors | Reference for loader-owned heaps, process allowance and allocator telemetry; independently implemented adapter, no Wine allocator source copied; [pinned provenance](docs/HORIZON_MEMORY.md) |
| [devkitPro](https://devkitpro.org/) / devkitA64 contributors | ARM64 C toolchain and Switch packaging tools |
| [Android Open Source Project](https://source.android.com/) and [R8/D8](https://r8.googlesource.com/r8/) contributors | Android build tooling and DEX fixture compilation |
| OpenJDK and JDK distribution contributors | Java compilation and the JVM reference execution |
| Ryujinx contributors | Emulator used for the recorded Horizon tests; tested binary hash/version are recorded separately |
| [Unicorn](https://github.com/unicorn-engine/unicorn) contributors | Optional host-only CPU oracle retained through RustyDroid's worker; absent from device builds |
| JetBrains / Kotlin contributors | Kotlin compiler, standard library and compiler dependency tooling for the independent JNI fixture |
| [F-Droid](https://f-droid.org/) contributors | Checksum-pinned real APK/DEX inspection fixture, downloaded separately |
| [Android NDK samples](https://github.com/android/ndk-samples) contributors | Frozen sources for later JNI/NDK milestones |
| [Vector Pinball](https://github.com/dozingcat/Vector-Pinball) / dozingcat contributors | Frozen independent integration target for later milestones |
| Cargo dependency authors | Parser, compression, serialization and other library dependencies recorded in `Cargo.lock` |
| DejaVu / Bitstream / Arev contributors | Glyphs in RustyDroid's retained text atlas; full notices in `third_party/fonts/DejaVu.LICENSE` |
| [Mesa](https://docs.mesa3d.org/) / [devkitPro Switch Mesa](https://github.com/devkitPro/mesa) / Nouveau contributors | Actual EGL/GLES2 driver and shader compiler linked into the product NRO |
| [Khronos](https://www.khronos.org/) / AOSP OpenSL ES contributors | Public OpenSL ES interface layouts and identifiers; original headers/notices retained |
| GCC / GNU libstdc++ / newlib contributors | Device C/C++ runtime; redistribution notices and the GCC Runtime Library Exception are included in packaged dependencies |
| Unicode / ICU / IBM contributors | Pinned API 26 charset-name recognition data; original [license and provenance](third_party/android-charset/SOURCE.md) retained and packaged. Encoder implementations are original AndroSwitch code. |

The isolated [Horizon std experiment](experiments/horizon-std/README.md) uses the
Rust project's real standard library and rust-lang/libc 0.2.189
(MIT OR Apache-2.0), plus devkitPro/newlib and switchbrew/libnx. The std Rust
sources are unmodified; the verified libc adaptation and SDK transport glue are
AndroSwitch experiment work. This does not establish device parity for
RustyDroid's Android APIs.

These acknowledgements do not change the components' individual licenses.
Tools, external APKs and proprietary game assets are not bundled in the
source repository. [The fixture lock](fixtures/corpus.lock.json) records the
actual revisions, hashes, source status and planned use of corpus entries.
The Java M1, storage M2, ELF M3 and Java/Kotlin/C/C++/Rust M4 fixtures are
included as AndroSwitch test source. The upstream parser/interpreter/JNI
descriptor work remains attributed to RustyDroid.

M3's generic Horizon code-memory mapping adapts the technique by **Andy Nguyen
and fgsfds** from `so_util.c`; its MIT license is retained. The JNI ABI header
is from AOSP under Apache-2.0. Exact source revisions and hashes are recorded
in [third-party notices](third_party/README.md). No PES-specific source,
offsets, game payloads or assets were imported.

See [NOTICE](NOTICE.md) for the retained attribution statement and
[the documentation index](docs/README.md) for current project behavior.
