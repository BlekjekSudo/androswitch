# Third-party source notices

The runtime is a history-preserving fork of **RustyDroid by nadabgfccvd and
contributors**; see [CREDITS](../CREDITS.md) and [NOTICE](../NOTICE.md).

## Generic Horizon code-memory mapping

`platform/switch-smoke/native.c` adapts the generic process-code-memory mapping
technique in `source/so_util.c`, copyright **2021 Andy Nguyen, fgsfds**.
The source was consulted in the local PES21 NX reference checkout at commit
`e9b1b42f1035ca835f6a5229be294d4ca5ecce97`; source-file SHA-256:
`7e15e7ab3cd4e8b59c27d30f6ec99806df64af72944cf7ef57ed70b9437379c4`.
Its complete [MIT license](so_util.LICENSE) is retained. No gameplay code,
title offsets, proprietary binary or package-specific behavior is included.
ELF planning, dependency resolution and relocations live in portable Rust.

## JNI ABI header

[android/jni.h](android/jni.h) is the unmodified Android Open Source Project
header shipped with Android NDK `28.2.13676358`, copyright **2006 The Android
Open Source Project**, licensed under Apache-2.0. Its notice is retained in the
file; the [Apache-2.0 license](../LICENSE-APACHE) is included at repository root.
SHA-256: `5df9074457c041b372f2c68ff646d6e8600cd673a223885e8df21c44c94ec667`.
This defines the JNI ABI; it does not supply a JVM implementation.

## Inherited glyph atlas

The UI rasterizer reuses RustyDroid's `crates/rd-vm/assets/glyphs.bin` and the
atlas format documented by its generator. It contains DejaVu Sans glyphs,
based on Bitstream Vera; DejaVu changes are public domain, and some imported
glyphs have Arev notices. The complete [font license](fonts/DejaVu.LICENSE)
is retained from the [DejaVu project](https://github.com/dejavu-fonts/dejavu-fonts).
This does not identify a new font build or replace the inherited atlas.

## OpenSL ES ABI and interface identifiers

`android/opensles/OpenSLES*.h` are unmodified headers from Android NDK
`28.2.13676358`. Khronos permissive notices remain in `OpenSLES.h` and
`OpenSLES_Platform.h`; the Android extensions retain AOSP Apache-2.0 notices.
`OpenSLES_IID.c` is retained as the identifier reference, not compiled. Source:
https://android.googlesource.com/platform/system/media/+/gingerbread/opensles/libopensles/OpenSLES_IID.c
SHA-256: `6b7c2d6ea5b61ff695b95e929827900e5715dfbce0a7571f8ad9ff2916b7a627`.
The complete original notices are in these files. `platform/androswitch/opensl.c`
implements the bounded audio subset itself, using those public ABI definitions.

## Linked platform components

`platform/androswitch/framebuffer_copy.h` adapts the 16-byte sector copy from
fincs/libnx `v4.12.0`'s `nx/source/display/framebuffer.c` so queue/fence failures
can be returned instead of invoking libnx's fatal framebuffer convenience API.
The complete [libnx ISC license](libnx-framebuffer.LICENSE) is retained. Source:
https://github.com/switchbrew/libnx/blob/v4.12.0/nx/source/display/framebuffer.c

The tested product uses libnx `4.12.0-1`, Switch Mesa `20.1.0-5` (EGL metadata
`20.1.0-rc3`), and `switch-libdrm_nouveau 1.0.1-2`. Mesa, Nouveau, libnx,
libstdc++, newlib, Rust libraries and Cargo dependencies keep their individual
licenses. `scripts/collect-product-licenses.ps1` prepares the source-linked
notices used by `package-androswitch.ps1`; the package contains a license
inventory and full copied texts. This does not relicense dependencies under
AndroSwitch's own MIT/Apache-2.0 choice.
# M8 native storage additions

SQLite 3.54.1 comes from its official, hash-pinned amalgamation archive;
`scripts/fetch-sqlite.py` verifies SHA3-256 and the build verifies each source
SHA-256. See `sqlite.lock.json` and `SQLite-NOTICE.txt`. Generated source stays
in ignored local artifacts. The native C code is public domain. xmlparser
0.13.6 (MIT OR Apache-2.0) supplies the no_std XML tokenizer; its notices are
collected with the actual device Cargo dependency graph.
