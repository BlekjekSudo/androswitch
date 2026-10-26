# Shared RustyDroid I/O integration

This is an integration checkpoint toward the pinned RustyDroid API port, not
complete upstream or Android compatibility. The implementation derives from
RustyDroid by nadabgfccvd and contributors at `3761815dc6a2a1427e9fc66b66e8471b78d1b170`.
Its history, notices and licenses remain intact.

## Common implementation and state

The original `jfile` File/path, file-stream and AtomicFile algorithms now live
in `java_file`, `java_stream` and `atomic_file`. Both reference dispatch and the
shipping std VM call these modules. The previous product File/stream algorithms
and their per-File side-table entries were removed. `RandomAccessFile` remains
an existing product adapter and is not claimed as a newly shared implementation.

File paths, stream positions, closure state and stream/channel ownership live
in actual Java heap fields. A channel holds a strong heap edge to its stream
owner; position and close operations share that owner. The object graph reaches
JNI through the VM's actual objects, rather than another reference object store.

The storage boundary explicitly chooses the bound persistent `GuestFileSystem`.
When a product framework is bound without storage it fails with an identified
error. Only the unbound reference profile can select `jfile::ReferenceIo`, which
uses the original memory VFS and its quotas. Product files are positional,
streamed operations on the same mounts used by native code; the memory oracle's
64 MiB file quota is not copied to the device filesystem.

Platform adapters add exclusive empty-file creation, observed modification
timestamps and rename replacement. Host creation uses `create_new`; the Horizon
C adapter uses `O_CREAT | O_EXCL`, closes the descriptor and commits the SD
filesystem. Replacement failure after publication/commit is reported, not
reinterpreted as a durable success. APK mounts and reserved/private roots retain
their write/rename guards.

AtomicFile retains the upstream backup protocol. Finishing a replacement must
cross the real file-sync boundary before deleting the previous committed backup.
The storage-failure contract exercises recovery with an injected sync failure.

## Character and byte streams

The original `jio_chars` implementation now compiles on the default std profile.
Reader, Writer and byte-stream tables belong to `Engine.io_tables` on both
profiles. No second set is allocated by the framework adapter. GC traces typed
Writer/Reader sink/source edges and purges dead keys; numeric charset names or
stream contents are not treated as object references. Existing generation
side-table traversal revisits old owners during minor collections.

The unchanged original six-case `io_chars_266` suite also runs through the
shipping VM in `as-vm/tests/shipping_io_chars.rs`. The original reference target
and all fifty upstream suite registrations remain separate.

SDK tests exposed and corrected three general integration defects:

- A superclass-template search could consume a DEX subclass constructor and
  skip its field initializers. Constructors now stay at their declared class.
- The earlier portable String handler intercepted byte/charset constructors
  before the already-shared original String implementation. The std dispatcher
  now selects the inherited implementation first.
- The wider regression corpus caught the inherited encoder's weaker charset
  aliases/BOM and allocation behavior shadowing the already verified API26
  `String.getBytes` adapter. Named/default byte encoding keeps that common
  product adapter; moving constructor dispatch does not replace its contracts.
- Flushing an encoding Writer discarded an incomplete surrogate pair; chaining
  Writers converted raw UTF-16 chars to a lossy Rust String. Encoding flush keeps
  the pending high surrogate, and Writer callbacks receive a real `char[]`.

The JVM oracle is now written directly to a UTF-8 file. The earlier Windows
console could turn an emoji into `?`; those initial mismatches are retained as
diagnostics, not promoted to API failures or passes by changing expectations.

## Verification scope

The SDK catalog grows from the frozen 257 cases to **274**: 148 pure Java/DEX,
93 Android/platform and 33 JNI/native/graphics. Seventeen I/O cases share the
existing APK and ordinary UI. The independent JVM produces the pure expected
values. Initial host execution has 148/148 matching pure results and 17/17 I/O
passes; its native/graphics/worker omissions are not device passes.

The full reference command passes 717 cases: the previous 710-case corpus,
one additional reference descriptor-constructor regression and the six-case
shared shipping wrapper. All fifty original source suites execute; native/JIT
zero-case targets remain unverified. Combined desktop/product regression passes
1,010 cases. The default product VM separately passes 47 unit cases plus the
six unchanged I/O cases, and all five original heap/GC contracts. Strict
workspace/reference Clippy, core-only checking, formatting, 26 Python tests and
the source audit pass. No ignored/filtered cases are counted as passes.

Reference regression, product regression, NRO build, emulator and hardware
qualification are recorded separately in the associated execution receipt.
The earlier 219-case physical result and GC-only NRO evidence do not qualify
this binary or the larger APK. No game first frame is established here.

## Open boundaries

This work does not close the complete I/O row or upstream parity:

- Java/native descriptor lifetime, duplicate descriptors, shared offsets,
  rename/unlink while open and native advisory locks still need one owner.
  Product calls that lack that bridge return `API_UNAVAILABLE`; inherited
  reference-only lock/descriptor assumptions cannot return fake device success.
- File permission/attribute mutation, space queries, native timestamp/stat
  transport and the rest of `android.system.Os` require common platform contracts.
- Inherited character Readers materialize input up to their explicit 16 MiB
  stream ceiling. Aggregate side-state allocation/pressure and incremental
  decoding remain memory work; this is not unlimited streaming support.
- Preferences and SQLite still have product services whose live objects and
  persistence must be reconciled with the inherited implementation.
- Shared class/enum/reflection metadata, framework/UI/lifecycle, scheduling and
  remaining Java/Android families retain their own ledger entries.

The source exclusion count is an inventory of selected original files, not an
API denominator or an estimate of completion percentage.
