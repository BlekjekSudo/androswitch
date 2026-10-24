# DEX field reflection

AndroSwitch implements a bounded field-read layer on the portable RustyDroid
VM. It uses real DEX declarations and stored values. Application names do not
select behavior. The initial scope is field discovery, metadata and reads;
setters and platform field catalogs remain unavailable.

`Class.getDeclaredField(String)` searches a class's own instance declarations
before its static declarations. `Class.getField(String)` searches public fields
on the class, then its interfaces recursively, then its superclass. A nonpublic
declaration does not hide an inherited public candidate from this lookup. A
bytecode field reference alone does not declare a field. Null names throw
NullPointerException; a proven missing declaration throws NoSuchFieldException.
The parser preserves malformed class-data errors rather than presenting an
empty declaration list. These rules follow the pinned
[API 26 Class implementation](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/native/java_lang_Class.cc).

Each returned Field has its own accessibility override and immutable identity
metadata: declaring class, name, resolved type and access flags. The runtime
validates canonical Class identities and its own Field provenance. It does not
populate a heap-shaped fake Field with guessed fields. Type resolution does not
initialize the declaring class.

Generic `Field.get(Object)` returns the actual reference/null or boxes the exact
declared primitive type. The eight primitive getters allow Java widening
conversions and reject narrowing, Boolean/numeric mixing, or reference fields.
An instance read checks null and assignability, then reads the actual stored
value or the declaration's genuine unwritten Java default. Static reads ignore
the supplied receiver and initialize the actual declaring class, including when
lookup began on a subclass. Required default-interface initialization that the
VM cannot schedule fails explicitly instead of being skipped.

Access checks use the actual declaring DEX class on the current execution
owner's stack. They distinguish public/private/package/protected access and the
cross-package protected receiver rule. Portable native DEX calls also retain
their real declaring class; diagnostic labels are never parsed to guess it.
Unknown native thread identity or a nonpublic read without a Java caller cannot
silently acquire permission. `setAccessible(true)` suppresses access checking on
that Field only; receiver and conversion checks still apply.

ART checks the receiver, or initializes a static owner, before checking field
access and primitive conversion. An initializer can therefore reenter Java and
change the same Field's accessibility before its read continues. The service
reads the current override after initialization and holds no service borrow
across callbacks. This ordering follows
[API 26 Field reads](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/native/java_lang_reflect_Field.cc)
and [AccessibleObject](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/reflect/AccessibleObject.java).

Metadata is a weak, budgeted side table: at most 4096 Field records, 4 MiB of
estimated metadata payload and 64 in-flight read scopes. Ticketed read roots
retain the Field, receiver and relevant Class objects across initialization and
GC. Collection removes unreachable records and restores the metadata budget.
These roots do not keep discarded Fields alive indefinitely.

The current VM has two storage limitations. Instance slots use a field name;
static slots use declaring owner plus name. Hidden same-name instance fields,
or same-name static fields with distinct descriptors, require richer storage
keys and fail explicitly. A static ambiguity is rejected before initializing
the owner. The original fixture's class-file transformation demonstrates that
the latter shape is legal on a JVM and in DEX; it is not treated as a malformed
APK. Arrays, primitives and Object have known empty declared-field catalogs.
Other platform field catalogs are not assumed empty.

The original SDK/JVM fixture is
[fixtures/m9/reflection-fields](../fixtures/m9/reflection-fields/README.md).
Its normal Activity displays `FIELD REFLECTION PASS` and writes a VFS transcript
at `/sdcard/Download/androswitch-field-reflection-proof.txt`. It uses standard
Android APIs without native test hooks. The builder produces another Java
namespace and manifest identity, and records source/APK hashes.

The host suite checks ordinary JVM results, all primitive widening pairs,
access and typed failures, initialization/failure poisoning, reference identity,
GC/reentrancy, metadata budget reclamation and explicit unsupported storage.
JVM comparisons cover shared behavior; ART-specific ordering is tested against
the pinned source contract separately. Field-specific native-host reentry is a
portable host test; it does not establish an AArch64 JNI Field roundtrip.

The initial source checkpoint and its review followups are recorded in
[the initial host receipt](../reports/m9-reflection-fields-host-initial-20261025.json).
The [post-review host receipt](../reports/m9-reflection-fields-host-20261025.json)
binds the static-storage guard, two legal duplicate-name DEX fixtures, and seven
focused tests rerun against the corrected owner-aware core. Its linked core
receipt records the separate native allocation and initialization-owner tests.
Host results are separate from Horizon emulator or physical-device execution.
Neither successful fixture tests nor recognizing the required APIs establishes
an external game's first screen.
