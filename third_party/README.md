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
