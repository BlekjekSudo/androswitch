# Shared String allocation and literal lifetime

RustyDroid by nadabgfccvd and contributors is the source of the common dynamic
String allocator and its catchable OutOfMemoryError contract. The original
allocation body now lives in `java_values`; both dispatchers delegate to it.
The product's former cache retained every dynamically generated String and
returned the same identity for equal results. That cache is removed.

Only loaded DEX literals enter the Engine's literal table. Both const-string
opcodes, including jumbo, and encoded static String values use it. The existing
collector traces those references on both profiles. Runtime-created strings
remain ordinary Java heap objects retained by actual frames, fields, JNI roots
and other owners. Literal identity is shared across DEX files within one Engine;
different Engines have separate tables. No application name chooses a policy.

Four shared unit contracts cover distinct dynamic identities, literal retention
through minor/major GC, a 512-allocation churn under a 1 KiB heap budget, and
catchable failed allocations without publishing a literal entry. Two actual DEX
contracts exercise normal/jumbo constant loads across DEX files/Engines and
512 Integer.toString calls under a 4 KiB heap budget. These measure VM heap
stability, not all native process overhead or RSS.

The synthetic upstream DEX builder previously wrote plain UTF-8 despite declaring
UTF-16 lengths. Supplementary literals therefore produced invalid DEX string_data.
It now writes modified UTF-8 over UTF-16 units, including C0 80 for NUL and the
two surrogate encodings for a supplementary scalar. A fixture-writer regression
checks the bytes and parsed value. Runtime decoding is not changed to accept
the invalid fixture, and external APKs are not modified.

The central SDK source adds three independent JVM scenarios: dynamic builder
identity, encoded/reflected literal identity after GC, and dynamic numeric-string
churn with GC. It contains 299 cases with 173 JVM rows. Initial ordinary case
execution on the product host has 256 PASS and 43 explicit host omissions; the
three new cases agree with the actual JVM. Each artifact's later host, emulator
and hardware qualification is recorded separately.

The broader upstream integration remains incomplete. Actual isolated-surrogate
String values still require a UTF-16 heap representation. Character-stream side
state still needs aggregate memory accounting and incremental decoding. The
String allocation change does not complete filesystem descriptor, preference,
database, UI, scheduling or native ABI contracts in the main parity ledger.
