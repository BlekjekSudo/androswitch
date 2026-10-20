# GC and runtime efficiency verification — 2026-10-20

## Scope and quality constraints

This change integrates the Appendix G registry and source-built C/C++/Rust
native-language work with measured GC/runtime improvements. The repository's
`main` at verification was `7ce8ba3a9849cdef09fee988a381dac591f5b8c4`;
registry work also overlaps the still-open PR #158. This PR does not close or
merge that PR. The other 25 adapters remain unimplemented; this is not full NDK
or physical-Android conformance.

The optimization-only baseline is imported delivered source
`6fdfaaf47b60f93d2aba5f499a2b7fa1a198008c`. Measurements used the Rust source
subsequently delivered in `f119cbd8143374f5de89053074502fbf5ba03d72`.
PR-publication follow-ups extend CI and shell-gate coverage, document evidence,
and scope CLI report serialization to its feature configuration so default and
native builds both pass strict Clippy. The CLI JSON behavior is preserved;
these follow-ups add no new VM/GC optimization.

No quality-damaging shortcut is accepted. Heap storage/API/limits, fuel,
safepoints, parser/ELF validation, barriers, pinning and the optimization
baseline's rendering/native/JIT algorithms are unchanged. The cumulative PR
also includes the earlier native UI/inheritance fixes documented in the native
language delivery; do not confuse the optimization-only diff with the full
main-to-PR diff.

## Retained changes

- Scan the exact conservative Debug root byte stream without temporary Strings.
  Preserve cross-write numeric tokens, saturation, order and byte counts.
- Remove a redundant minor-GC uniqueness set over unique arena indices.
- Borrow weak-class descriptors and only query WeakHashMap ancestry for Maps.
- Borrow method names instead of allocating per comparison.
- Use a checked sequential instruction cursor with checked binary fallback for
  branches, handlers and invalid PCs. Fuel and root publication remain intact.
- Borrow static built-in hierarchy/interface slices with preserved keys/edges.
- Avoid temporary String/UTF-16 allocations in length/hash/charAt paths; preserve
  surrogate units, invalid indices, messages and validation order.
- Resolve each subtype-walk class once, without a persistent mutable-DEX cache.

A compact vacant-slot representation passed tests but regressed sweep throughput
by about 40%; it was reverted. There is no optimization-only `heap.rs` change.
A slower scanner and a stale shared-Cargo-cache baseline attempt were also
rejected, not selectively reported as wins.

## Assertion-bearing paired measurements

25 seeded randomized-order pairs for each of nine workloads: 450 runs, matching
checksums and iteration counts. Original used a separate empty target directory;
final runtime dependencies were cleaned/rebuilt. No competing build/benchmark
was active. Linux GNU time measured child peak RSS.

| Workload | Original median ms | Final median ms | Ratio of medians | Median paired ratio [95% CI] | Median RSS KiB original -> final |
|---|---:|---:|---:|---:|---:|
| roots | 431.581 | 421.610 | 1.024x | 1.006x [0.972, 1.020] | 13400 -> 8144 |
| host | 427.035 | 414.853 | 1.029x | 1.039x [1.015, 1.062] | 13728 -> 9340 |
| sweep | 52.737 | 52.567 | 1.003x | 0.999x [0.990, 1.015] | 14632 -> 14468 |
| minor | 212.332 | 110.044 | 1.930x | 1.891x [1.872, 2.006] | 11988 -> 11904 |
| resolve | 689.405 | 126.124 | 5.466x | 5.503x [5.389, 5.531] | 4972 -> 4916 |
| interp | 146.115 | 19.315 | 7.565x | 7.649x [7.580, 7.677] | 6208 -> 6164 |
| strings | 39.004 | 16.131 | 2.418x | 2.418x [2.388, 2.460] | 5948 -> 5932 |
| subtype | 7.185 | 2.742 | 2.620x | 2.587x [2.546, 2.760] | 4200 -> 4200 |
| jit | 55.309 | 60.085 | 0.921x | 1.022x [1.000, 1.029] | 7620 -> 7680 |

Confidence bounds are a descriptive 10,000-resample percentile bootstrap of the
**median paired ratio**, not of the ratio of medians. They are not multiplicity
adjusted and cannot model all host noise. The fetch workload intentionally has
long sequential instruction runs; these are not whole-app multipliers.

The initial JIT row has different ratio-of-medians and paired estimates because
of noisy/mixed timing populations. Both the raw row and slower point estimate
remain included. An additional 100 unpinned and 100 fixed-vCPU pairs gave median
paired ratios 1.030 [1.022, 1.036] and 1.027 [1.025, 1.030]. No JIT algorithm was
changed and no JIT-specific speedup is a headline claim.

Raw pairs, generated summary, hierarchy proof and supplemental JIT summary are
in [the performance evidence directory](../tests/performance/20261020).

## Real-app and quality evidence

Recorded uncontended Comprix 1.6 typed replays, 20 cycles / 122 MCP steps each:

| Mode | Original seconds | Final seconds | Less elapsed time |
|---|---:|---:|---:|
| GC on, JIT off | 219.85 | 166.29 | 24.4% |
| GC on, JIT on | 225.70 | 172.06 | 23.8% |

These are one sequential replay per binary/mode, not a statistical app-wide
speedup guarantee. APK SHA-256 is
`39a4930c89f1a3670eca03cc6eb37114a0467c52687bf028dd633a6b60c6d6f4`.
The final typed item/quantity survived cycling and final collection. Six app
milestone PNG pairs and 36 native UI/widget pairs were byte-identical.

Original GC-on JIT-off runs already vary in non-time health metrics. The final
21-sample trace exactly matched the earlier original trace, while a second
original run differed. JIT-on matched its contemporaneous original trace.
Do not claim every paired intermediate fuel/GC counter was identical. No fuel
metering or safepoint was removed. GC root-phase timing now includes tokenization;
phase attribution before/after is not directly comparable.

A one-cycle GC-off typed control passed. Both versions separately reached the
same typed 256 MiB OOM at step 28 of the requested three-cycle GC-off boundary
control, after two cycles. Heap/fuel/clock samples matched before OOM. The
budget was not raised and GC was not secretly enabled.

Recorded gates: 570 all-feature / 558 default tests; strict Clippy/fmt; minimal
checks; Rust 1.76 minimal-native check and actual build; 150 JVM golden vectors;
18 native APK cases with all 41 tools positively covered in each, 1,683 calls;
nine malformed-ELF rejections; 15 legacy APK/ABI checks. Normally ignored real
source/OSS APK tests were explicitly exercised. Fresh PR verification and GitHub
checks are separately reported on the PR, not inferred from these earlier logs.

The JVM oracle is the repository CI's Temurin 21, compiling target 17. JVM 17
has two legacy float-string spellings; all 150 original/final raw VM records
were identical there. No comparator or runtime formatting was weakened.

## Reproduce

Use separate target directories for original/final source restores: restored
old mtimes can make a shared Cargo target reuse an unrelated optimized library.
Copy the identical final `efficiency_bench.rs` example into the baseline tree.
Build each with release/all-features/locked and one build job on a 2 GiB host.

```sh
python scripts/efficiency-benchmark.py --baseline /absolute/original-bench --candidate /absolute/final-bench --out /tmp/rd-perf/final-original-nine-paired.json --repetitions 25 --workloads roots host sweep minor resolve interp strings subtype jit
python scripts/summarize-efficiency-benchmarks.py --evidence /tmp/rd-perf
```

The checked instruction cursor has differential random-walk/bad-PC coverage;
UTF-16 has Unicode/surrogate/index differentials; the GC graph oracle uses 64
seeds with 192 objects each and independent buffered roots/manual marking.
These are regression evidence, not a proof over every APK or of a universal
performance optimum. Headless emulation is not physical-device validation.

The mandatory source-language CI gate rebuilds SHA-pinned signed APKs, executes
all three ABIs with JIT off/on, requires positive coverage of every MCP tool,
and rejects nine corrupted ELF inputs. It uploads provenance/evidence but not
the generated private debug signing key. See [native languages](native-languages.md).
