# Android API 26 charset alias data

`crates/rd-vm/src/portable/charset_catalog.rs` contains data derived from the ICU
converter alias table shipped in AOSP tag `android-8.0.0_r1`. The encoder and
bounded lookup implementation in `string_encoding.rs` is original AndroSwitch
code. No ICU converter implementation is incorporated.

The full original `icu4c/LICENSE` is retained as [LICENSE](LICENSE), including
Unicode/IBM copyright and third-party notices. Its SHA-256 is
`2023a2d68dfcdb1f7b7c5dccb96f88aabba112f63f250cb9e533cc64625eff07`.

The data source is
[ICU convrtrs.txt](https://android.googlesource.com/platform/external/icu/+/android-8.0.0_r1/icu4c/source/data/mappings/convrtrs.txt),
SHA-256 `446d9587e344c9cc55af1c878f439e1c96efae3487b02b7f561ba21527b33227`.
It carries Copyright (C) 2016 and later Unicode, Inc. and others, and
Copyright (C) 1995–2014 International Business Machines Corporation and others.

Regenerate from an unmodified local copy of that source with:

```powershell
python scripts/generate-android26-charsets.py path/to/convrtrs.txt
python scripts/generate-android26-charsets.py path/to/convrtrs.txt --check
```

The generator rejects a different input checksum. It extracts 365 converter
groups, 1,548 normalized recognition keys, and 95 explicitly implemented aliases.
The six implemented groups are UTF-8, UTF-16, UTF-16BE, UTF-16LE, ISO-8859-1,
and US-ASCII. Extra common spellings are explicitly listed in the generator.
Versioned/BOM variants remain distinct. Recognition of another converter does
not claim that its conversion or all original ICU data packages are available.

The bounded recognition comparison follows the behavior documented in
[ICU ucnv.h](https://android.googlesource.com/platform/external/icu/+/android-8.0.0_r1/icu4c/source/common/unicode/ucnv.h)
and inspected in
[ICU ucnv_io.cpp](https://android.googlesource.com/platform/external/icu/+/android-8.0.0_r1/icu4c/source/common/ucnv_io.cpp).
That comparison is used to identify missing API coverage; supported conversion
uses the explicit case-insensitive alias list. Unlisted equivalent spellings
produce an explicit coverage error, rather than selecting an unverified encoder.

Encoding semantics were checked against the API 26
[String.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/String.java),
[Charset.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/nio/charset/Charset.java),
[NativeConverter.cpp](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/luni/src/main/native/libcore_icu_NativeConverter.cpp),
and ART's
[CharsetUtils.cc](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/native/libcore_util_CharsetUtils.cc).
These are references, not copied implementation code. API 26 ART's ASCII and
Latin-1 fast paths replace each out-of-range UTF-16 code unit with `?`;
a supplementary character therefore produces two replacement bytes, unlike
OpenJDK's single replacement for a surrogate pair. This behavior is verified
against source and an explicit expected byte vector, not Android-device execution.

The VM's existing Rust-string representation cannot retain unpaired Java UTF-16
surrogates. This change does not remove that limitation and does not claim full
`java.nio.charset` coverage, decoding, charset providers, or ICU converter options.
