# Attribution and provenance

AndroSwitch is a derivative of **RustyDroid**, created by **nadabgfccvd and
the RustyDroid contributors**:

- Original project: <https://github.com/nadabgfccvd/RustyDroid>
- Original copyright: **Copyright (c) 2026 RustyDroid contributors**
- Fork base: `6388281f66d254c8e01a0fdc7a5a9a30239bc762`, 22 October 2026
- License: **MIT OR Apache-2.0**

The original Git history, copyright notices and license texts are retained.
The upstream APK/DEX parsers, Dalvik interpreter, classpath, heap and broader
runtime implementation remain credited to their original authors. AndroSwitch
adds the Horizon portability work, libnx boundary, project tools and milestone
evidence. AndroSwitch-specific additions use the same MIT OR Apache-2.0 terms.

The original README, contributing guide and security policy are preserved as
`README.rustydroid.md`, `CONTRIBUTING.rustydroid.md` and
`SECURITY.rustydroid.md`. The RustyDroid specification and historical design
documents are also retained. Current AndroSwitch behavior and contribution
instructions are documented separately.

PES21 NX was consulted as a native Android-on-Horizon reference. No PES game
binary, assets, title-specific hooks, response payloads or source code were
imported in M0/M1. Any later component import must record its exact source,
revision and applicable attribution.

M3's generic code-memory mapping adapts the technique in `so_util.c`,
Copyright (C) 2021 Andy Nguyen, fgsfds, licensed under MIT. The original
license is preserved in `third_party/so_util.LICENSE`. The pinned AOSP JNI
header retains Copyright (C) 2006 The Android Open Source Project and its
Apache-2.0 notice. See [third-party provenance](third_party/README.md).

Rust, libnx, devkitPro, Android build tools, emulator tools, Cargo dependencies
and external fixtures retain their respective licenses and credits. The
project's license choice does not replace those terms. External APKs, generated
libraries, toolchains and emulator binaries are not committed to this repo.

See [CREDITS](CREDITS.md), [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).

Latest synchronized RustyDroid source: `3761815dc6a2a1427e9fc66b66e8471b78d1b170`.
The merge retains upstream commits, authors, runtime source and tests. See
[CREDITS.md](CREDITS.md) and [the source/porting audit](docs/UPSTREAM_PORTING.md).
