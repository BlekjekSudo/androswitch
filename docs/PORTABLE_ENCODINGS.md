# Portable string encoding

The Horizon VM implements `String.getBytes()` with UTF-8 and
`String.getBytes(String)` with UTF-8, UTF-16, UTF-16BE, UTF-16LE, ISO-8859-1
and US-ASCII. UTF-16 uses a big-endian BOM for nonempty output; the explicit
BE/LE forms omit it. Each call returns a fresh, signed Java `byte[]`, including
empty arrays. The default overload and named encoders share one implementation.

The [host receipt](../reports/portable-encodings-host-20261024.json) records five
passing tests on the actual `no_std + alloc` interpreter, with 31 JVM-equal cases
executed before and after GC, explicit API 26 replacement vectors, Android alias
groups, exception inheritance, fresh array ownership, exact memory boundaries
and unchanged bytes written through Java to the shared VFS. It does not qualify
an APK's complete startup, emulator execution, or physical hardware.

Build and run the unchanged Java call sites with:

```powershell
./crates/rd-vm/tests/fixtures/portable-encoding/build.ps1
$env:CARGO_INCREMENTAL = '0'
cargo test -p as-switch-smoke --features host-cli,portable-tests --test portable_encoding -- --nocapture
cargo clippy -p as-switch-smoke --features host-cli,portable-tests --test portable_encoding -- -D warnings
```

The JVM oracle explicitly selects UTF-8 as its default charset. Each Rust test
asserts `rd_vm::PORTABLE_CORE`, preventing a legacy `std` feature from silently
substituting another interpreter.

Charset names use a fixed 60-byte stack buffer and the API 26 ICU name bound.
The implementation accepts 95 explicit case-insensitive aliases. The pinned
ICU data also supplies 1,548 recognition keys so a known missing encoder,
versioned/BOM converter, option, or unimplemented alias form reports
`API_UNAVAILABLE`. Unknown/invalid names throw `UnsupportedEncodingException`,
which has its actual `IOException` ancestry; a null charset name throws
`NullPointerException`. No missing encoder is replaced with UTF-8. Recognizing
an alias does not claim that every original ICU data package or Java provider is
present. The retained [ICU license and source notice](../third_party/android-charset/SOURCE.md)
describe provenance and the reproducible checksum-checked generator.

The encoder computes the output length with checked arithmetic and checks the
managed heap's real `Value` storage charge before allocation. It fallibly reserves
the final array backing vector and moves that vector into the heap; it does not
clone the source string or create another encoded scratch buffer. Budget denial
and host reserve failure throw `OutOfMemoryError`. Managed budget headroom is
not a claim about live Horizon physical memory availability.

API 26 ART's fast ASCII/Latin-1 paths replace every out-of-range UTF-16 code unit
with `?`. A supplementary character produces two replacement bytes. OpenJDK 21
replaces its surrogate pair once, so the two corresponding test cases deliberately
compare against the pinned ART source-derived result and record the JVM
difference. No Android-reference device was executed for that comparison.

The VM's existing Rust-string representation cannot retain unpaired Java UTF-16
surrogates; malformed surrogate behavior remains unverified. This implementation
does not provide arbitrary legacy encoders, `Charset` objects, decoding, provider
registration or ICU converter-option execution. These remain explicit API gaps.
