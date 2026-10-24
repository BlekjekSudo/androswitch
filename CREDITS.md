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

The original documents are preserved from the fork base:

- [RustyDroid README](README.rustydroid.md).
- [RustyDroid contributing guide](CONTRIBUTING.rustydroid.md).
- [RustyDroid security policy](SECURITY.rustydroid.md).
- [RustyDroid specification](RustyDroid.md) and
  [upstream roadmap](docs/ROADMAP.md).

The original logo is retained with the upstream material; it is not presented
as AndroSwitch branding. The original license files are unchanged. Keep these
notices and source attribution when redistributing or extending the project.

## AndroSwitch additions

[BlekjekSudo/androswitch](https://github.com/BlekjekSudo/androswitch) maintains
the Horizon port and its own evidence. Current additions include:

- Portable `no_std + alloc` paths and a libnx C ABI boundary.
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

## Platform, tooling and test sources

M8 adds native **SQLite 3.54.1**, by the SQLite authors/contributors (public
domain), compiled from its hash-pinned original amalgamation. See
[SQLite notice](third_party/SQLite-NOTICE.txt). XML preferences use
**xmlparser 0.13.6**, by Yevhenii Reizner and contributors, MIT OR Apache-2.0;
its original license files are included in packaged dependency notices.
SharedPreferences behavior is referenced against AOSP Android 8.0.0_r1
`SharedPreferencesImpl` (Android Open Source Project, Apache-2.0).
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
