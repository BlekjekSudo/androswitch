# Numeric and String checkpoint, 28 October 2026

The [retained report](../reports/shared-jdk-20261028.json) identifies runtime
`7b59cdbc56e45fe9113e73a7c6e3f034064b391c`, the corrected product regression
test commit, exact commands, hashes and raw evidence. It covers the shared
[RustyDroid numeric family](UPSTREAM_NUMERICS.md) and
[String allocation/lifetime integration](UPSTREAM_STRING_STORAGE.md).

The host product suite passes 1,017 tests; the reference suite passes 730 with
all 50 original suite routes registered and executed. Default shipping VM tests
pass 70, with five additional GC contracts and two fixture-serializer tests.
Strict workspace/reference Clippy, formatting, core-only compilation and the
source audit pass. The device-parity gate still fails as expected. Zero-case
native/JIT targets remain unverified. Counts are tests, not percentages of APIs.

The frozen JVM numeric oracle drives 233 comparisons across 218 exact inherited
declarations. The same 299-case APK executes 173 Java/DEX values matching the JVM.
Its direct host execution reports 256 PASS and 43 explicitly unsupported host
services. Those omissions are not passed tests.

The exact NRO hash is
`81ba7f73b2096f10a540c5128de0d155826416df41c2e76cad3608b8ef3697f1`.
Pinned headless Ryujinx completes the available baseline with **296 PASS and
3 NOT_RUN**. The latter are its missing PCV clock commands. The original run
saved 275 passes; the completed run resumed that actual journal with the same
APK/NRO. A failed intervening attempt exhausted the Windows paging-file limit
before the app started. All recorded attempts restored the test SD. The final
workflow returned to the launcher; its real frame was visually reviewed.

Diagnostics also retain a stale product test that incorrectly required dynamic
Strings to be permanently interned. The corrected test checks distinct identity,
actual roots and reclamation, alongside the unchanged DEX literal tests. An
initial source-audit failure caught an unreviewed new report attribute; that
evidence-policy change was deferred until after the frozen-source qualification.
The final source audit uses the matching pinned inventory. New report policy is
reviewed separately when retained evidence is committed.

PowerShell can deliver the next Cargo stderr target header before the previous
binary's stdout summary. The reporting parser now retains FIFO target ownership;
its regression test preserves failures and incomplete targets. The original raw
logs are retained unchanged. All 27 reporting/build Python tests pass.

This is **not complete upstream 1:1 parity**, hardware verification of this
binary, Android certification, or proof of Mekorama/Candy Crush first frames.
It is pinned to upstream `3761815`; the subsequently observed `4489813` only
deletes the desktop rd-sentinel tool and does not change runtime API source.
The later 309-case tokenizer build has separate evidence and receives none of
this binary's device qualification automatically.
