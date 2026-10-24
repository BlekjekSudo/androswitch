# DEX field reflection contracts

Build with `./fixtures/m9/reflection-fields/build.ps1`. The builder compiles the
same original Java sources for a JVM and Android DEX, under two different Java
namespaces and manifest identities. The fixture receipt records all input and
APK hashes. No game assets or application identifiers are used.

`reflection-fields.apk` is a normal SDK proof. It displays
`FIELD REFLECTION PASS` and writes
`/sdcard/Download/androswitch-field-reflection-proof.txt`. It checks real static
String/int constants, inherited instance data, declaring/type/modifier metadata,
access suppression, and a typed missing-field error. It uses no native hooks.

The host test target is `reflection_fields`:

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p as-framework --features host --test reflection_fields --locked -- --test-threads=1
```

The supported subset is declared/public DEX field discovery, Field metadata,
per-Field accessibility suppression, generic reads with exact primitive boxing,
and the eight typed primitive getters with Java widening conversions. Discovery
does not initialize the owner; a static read initializes its real declaring
class. Instance reads validate null/type and use real stored values or genuine
unwritten Java defaults. Access checks use the actual active DEX declaring class
for the current native execution owner. No guessed caller is used for a direct
host read of a nonpublic field.

The portable instance heap currently keys slots by name. Hidden same-name
instance fields are therefore explicit unsupported operations, rather than
confusing base and subclass values. Field writes, platform field catalogs,
annotations/generic signatures and volatile memory ordering outside the VM
lease are outside this subset. Unknown platform catalogs fail explicitly;
known empty primitive/array/Object declarations can throw NoSuchFieldException.
Field side-table entries are weak and budgeted; live Fields trace their actual
Class identities and in-flight reads retain their state across initialization
and GC. Setters are never reported as implemented.

API26 contracts are pinned to Android8 sources:
[Class lookup](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/native/java_lang_Class.cc),
[Field reads](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/native/java_lang_reflect_Field.cc),
[Field metadata](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/reflect/Field.java),
and [AccessibleObject](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/reflect/AccessibleObject.java).
ART validates a receiver, or initializes a static declaring class, before
checking access and primitive conversion. Host JVM comparisons cover shared
semantics; they do not establish Horizon execution or a game first screen.
