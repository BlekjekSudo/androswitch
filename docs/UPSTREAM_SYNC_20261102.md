# RustyDroid integration at 33e7645

AndroSwitch preserves the full history and attribution of RustyDroid by
nadabgfccvd and its contributors. This update integrates
`33e76456643e157458d97087d2626045498725db`, checked on 2 November 2026,
over the previous `4489813` source checkpoint. It does not establish complete
Android API behavior on Horizon.

The source gate accounts for all **499 upstream files**: **346 identical**,
**153 reviewed adaptations**, and **10 relocated** repository-administration
files. Fifty additional core helpers are audited separately. All **95 original
VM integration test files** are registered directly, including the optional
WASM tests; zero-case native/JIT targets remain unverified. The shipping profile
still excludes **83 original VM source files**. These are source counts, not an
API coverage percentage.

## Integration decisions

- Keep the original lazy ARSC/string-pool algorithms, bounded caches, typed GC
  ephemerons, interpreter frame splitting, static-provider chain, lifecycle,
  dispatch, I/O, regex, XML and SQL updates. Cache access uses standard locks on
  the std build and a parser-only spin adapter on the retained core profile.
- Preserve the port's resource names/type-id offsets, actual execution-owner
  roots, shared Class identity, native/JNI bridge, persistent storage and bound
  framework dispatch. A missing bound API cannot fall back to disconnected
  reference state.
- The original stack byte-distance/depth guard now follows each execution
  owner. Tests cover overflow, interleaved suspended owners, unwind and invalid
  identity. The fallback stack budget does not yet constitute verification of
  physical Horizon thread-stack bounds.
- Bring the upstream SQL ownership policy to the native SQLite backend using
  the [SQLite authorizer](https://www.sqlite.org/c3ref/set_authorizer.html).
  `writable_schema`, ATTACH and DETACH are rejected during statement compilation,
  including after snapshot restore; ordinary pragmas/queries remain usable.
- Retain lossless managed UTF-16 builder storage and the original upstream
  String materialization algorithm. An isolated surrogate becomes U+FFFD at
  `toString`/substring conversion into the current UTF-8 String heap. This
  supersedes the earlier explicit rejection described in
  [the I/O checkpoint](UPSTREAM_SHARED_IO.md); it is upstream compatibility,
  **not full Android UTF-16 String semantics**. JNI and full String storage still
  require a unified UTF-16 representation.
- Update the upstream MSRV/dependencies and retain the WASM engine as an
  optional reference feature with its required resource/dispatch closure.
  It is not enabled in the shipping runner. New upstream publishing/bot
  workflows are preserved under `docs/upstream/github/`, without activating
  upstream repository administration in this fork.

## Verification and open work

The complete source inventory and the omission/drift gate pass. The strict
device-parity gate intentionally fails. The initial reference run exposed SQL
authorization and String conversion differences; both were fixed and passed
their original focused tests. The first combined product run also recorded
missing generated fixtures in the isolated worktree. Those input files are
copied from the frozen development corpus and SHA-256 verified before retry.
Failed attempts remain diagnostic evidence rather than being counted as passes.

The full reference run passes **911 cases**, with all 95 original suite files
invoked. Four native/JIT targets execute zero cases and remain unverified. The
combined product/desktop run records 1,183 passes and one stale builder
expectation; after updating that expectation, its complete 14-case suite passes.
Together these qualify 1,184 product/desktop cases through a full run plus one
targeted rerun, rather than an unbroken all-green run. Shipping-profile tests
pass **84 cases**; parser, native-loader, SQLite and WASM tests pass **244 cases**.
These groups overlap in scope and must not be added into an API coverage score.
The unchanged 309-case conformance APK completes on the host with **266 PASS**
and **43 UNSUPPORTED**. Native/graphics/platform cases unavailable on that host
remain explicit gaps, pending separate Horizon execution.

Bundle provenance now reads the original base and synchronized upstream
revision from the runtime build's Git commit, replacing a stale hard-coded pin.

The [retained qualification report](../reports/upstream-merge-20261102.json)
names source `5d12467e573f38419e3ced627ec3f3cf9be4dd5d`, the real Horizon std
NRO, raw test logs, fixture hashes and individual emulator attempts. The NRO
SHA-256 is `0ee266432eddc85762080464ee2624ce742056f7c8ba946dc75a45daab32f908`.
The first emulator attempt passed 60 cases but hit the 15-second stop watchdog
during Java lifecycle cleanup. An isolated 60-second diagnostic build returned
successfully, with Java cleanup taking about 1.5 seconds; it does not establish
that increasing the production deadline is a fix. The original unchanged NRO
subsequently completed a resumed launcher workflow with 120 passing cases and
Java cleanup taking about 0.86 seconds. The earlier timeout remains unexplained.
Diagnostic and product results are kept separate, and each session's SD was
restored.

The final product resume (`0aabee1d29584648922d22aeea853429`) completes the
declared baseline with **306 PASS and 3 NOT_RUN**, a completed launcher workflow,
restored SD and visual inspection of the conformance screen. The three explicit
exclusions are CPU, GPU and memory clock queries. Full catalog acceptance is
therefore false. These observations were accumulated through the same NRO and
APK's ordinary persistent journal, not across the diagnostic NRO.

An earlier portrait session returned safely but failed the frame-hash gate:
the C readback is hashed in physical scan order while the VM hashes its logical
raster. Reconstructing the preserved image in rotation 1 yields the VM's exact
hash (`95663f1ba4872ac2`); physical scan order yields the recorded readback hash
(`73bbe1bf8bc0bd24`). The original reports remain unchanged. Normalizing this
verification contract remains open; the final unrotated session does not close
that gap. The development package includes these qualification limits and is
explicitly diagnostic, not a v0.1 or whole-Android compatibility release.

The [bundle verification receipt](../reports/upstream-merge-20261102/package.json)
records the ZIP hash and all 136 packaged file checks. It includes the exact NRO,
unchanged conformance APK, runtime qualification report and RustyDroid credits.

The earlier 219/219 physical-device result and older emulator runs do not
qualify this update. Mekorama and Candy Crush first frames remain unproven.

Remaining work includes canonical framework/VFS/database/lifecycle state,
thread/TLS ownership across all inherited families, metadata/reflection,
aggregate side-state memory pressure, remaining feature exclusions, and
Horizon behavior evidence for the full corpus. See
[the complete parity plan](UPSTREAM_PARITY_PLAN.md).
