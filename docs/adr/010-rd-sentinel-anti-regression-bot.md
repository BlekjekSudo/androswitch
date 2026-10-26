# ADR 010 — RD Sentinel: Dependabot-like anti-regression bot beyond CI

- **Status:** Accepted for implementation (repo-owner request, 2026-10-22)
- **Scope:** `.github/dependabot.yml`, `.github/CODEOWNERS`, `deny.toml`,
  `.github/rd-sentinel/`, `.github/workflows/rd-sentinel*.yml`,
  `tools/rd-sentinel/`, and the operator guide in `docs/rd-sentinel.md`
- **Current implementation:** RD Sentinel 1.2.0 (v1.2 amendment, 2026-10-23)
- **Identity (unchanged):** Rust-native Android runtime; no VM, KVM, or guest
  kernel; MSRV 1.76; minSdk 26; deterministic MCP/runtime behavior; no new
  `unsafe` outside `rd-jni`/`rd-ndk`.

## 1. Problem

The existing CI is intentionally a bounded pull-request gate. It already runs
formatting, Clippy, workspace tests, documentation integrity, feature checks,
150 JVM golden vectors, MSRV checks, fuzz-target **builds**, native ABI tests,
real-APK native gates, and signed native-language APK matrices.

That leaves a different class of regression risk that ordinary CI either
cannot afford on every pull request or cannot express as a single pass/fail
test at one revision:

1. **Time:** real fuzz execution, endurance/soak sessions, repeated flaky-test
   hunts, and many feature combinations need scheduled campaigns.
2. **History:** a change can keep every absolute test green while becoming
   slower, larger, less deterministic, or semantically different from the
   base revision.
3. **Environment drift:** beta Rust, future dependency releases, new advisories,
   license/source changes, and MSRV-incompatible transitive updates appear
   after a PR was green.
4. **Evidence shrinkage:** tests, fuzz targets, golden vectors, public
   contracts, or performance budgets can be weakened in ways that make CI
   greener by checking less.
5. **Real-world behavior:** process-cold launch, first frame, interactive
   materialization, native calls, UI trees, screenshots, RSS, and heap use
   need pinned real APKs and trend gates, not only unit tests.
6. **Triage:** CI can fail a job, but it does not maintain a deduplicated,
   fingerprinted findings stream, PR risk labels, maintainer commands, or
   scheduled regression issues.

## 2. Decision

Create **RD Sentinel**, a dedicated GitHub-native bot with four separated
parts:

1. **Dependabot proposes dependency and GitHub-Actions updates.** Sentinel
   does not reimplement update generation or auto-merge. It gives those PRs a
   stricter, RustyDroid-specific trial.
2. **A read-only analyzer** (`tools/rd-sentinel/`) computes explainable diff
   risk, enforces versioned invariants, runs risk-selected campaigns, and
   emits a fingerprinted JSON report plus Markdown and SARIF renderings.
3. **Scheduled deep campaigns** run work that is too long or broad for normal
   PR CI: differential A/B probes, actual fuzz execution, repeated
   determinism checks, real-APK soak/endurance, future-toolchain trials,
   all-feature tests, dependency/license/audit analysis, and size/resource
   trend checks.
4. **A trusted publisher** runs from the default branch after
   `workflow_run`, verifies the report against the live PR/base/head and file
   list, then comments, labels, opens or updates deduplicated issues, and sets
   the `rd-sentinel/gate` commit status. Candidate code never receives the
   publishing token.

The bot is implemented with Python's standard library so it has no additional
runtime dependency supply chain. It orchestrates the repository's existing
evidence tools (`cold26.py`, `mcp-session-soak.py`, `efficiency-benchmark.py`,
the fuzz campaign, native gates, and `rd perf`). The existing Comprix script
remains an application-specific journey and is run only for an explicitly
supplied Comprix APK; pinned launchable APKs use Sentinel's generic MCP
endurance client instead of being forced through another app's UI script.

## 3. Trust and anti-tamper model

- Analysis workflows use `contents: read` and `persist-credentials: false`.
- For a PR, policy and baseline are loaded from the **base checkout** whenever
  they exist there. A candidate cannot silently relax its own thresholds.
- The bot's workflows, policy, baseline, Dependabot configuration, and code
  are protected paths. Touching them requires the maintainer-applied
  `sentinel:policy-approved` label; otherwise the verdict is blocked.
- The publisher is a separate `workflow_run` workflow. It checks out only the
  default branch, downloads the report artifact, re-fetches the PR and its
  file list from GitHub, rejects stale/mismatched SHAs, and overrides a forged
  green report if protected paths changed without approval.
- Reports are schema-validated and content-fingerprinted. Command output is
  size-capped and redacted before it can appear in a report.
- The trusted policy and baseline are themselves schema/range-validated before
  use, so a typo cannot silently unblock P0/P1 findings or drop protection
  from Sentinel's own workflow/code.
- Sentinel workflows pin third-party actions to reviewed full commit SHAs and
  are statically checked for template injection, credential persistence,
  missing permissions, and candidate execution under privileged triggers.
- Maintainer chat commands are accepted only from OWNER, MEMBER, or
  COLLABORATOR associations. Commands can rerun campaigns; they cannot approve
  policy, edit a baseline, merge, or push code.

This is defense in depth, not a claim that a malicious workflow change is
impossible to detect in review. The repository must also protect `main` and
require both normal CI and `RD Sentinel / audit` (or the published
`rd-sentinel/gate` status) before merge.

## 4. Anti-regression policy

Sentinel is **fail-closed**:

- A required check that errors, times out, or is skipped in strict mode blocks.
- P0 and P1 findings block. P2/P3 findings are reported and count toward the
  risk budget.
- Removing tests, fuzz targets, golden vectors, MCP tools, or contracts blocks
  unless an explicit, dated policy exception says otherwise.
- Exceptions must name an owner, reason, path/finding scope, and expiry date.
  An expired exception becomes a blocking finding.
- New mutable process/thread-global guest state in `rd-vm`, new `unsafe`
  outside the approved crates, public API removals without an ADR, new
  `NOT_IMPLEMENTED` surface without compatibility evidence, dependency source
  changes, and workflow privilege escalation are treated as regressions until
  proven otherwise.
- Performance is compared both to absolute budgets and to the base revision.
  A result that is merely “still under budget” can still block if the A/B
  delta exceeds the configured ratio and a noise floor derived from sample
  count and median absolute deviation. Noise handling never relaxes the
  absolute <26 ms snapshot budget.
- Targeted test selection follows downstream workspace dependents, and public
  API evidence compares per-crate declaration inventories rather than every
  removed `pub` line in isolation.
- Determinism is evidence, not a hope: repeated tests must not alternate,
  repeated `inspect`/screenshot output must hash identically, and base and
  candidate cold-start frames for the launchable corpus must match unless an
  intentional rendering change is accompanied by updated golden evidence.

## 5. Campaign model

| Campaign | When | Work beyond ordinary PR CI |
|---|---|---|
| PR risk audit | pull request, merge queue, maintainer rerun | trusted-base policy, diff/evidence audit, targeted tests, feature submatrix, flaky repeats, dependency diff, optional A/B perf |
| Nightly deep | daily on `main` | all-features workspace tests, broad feature powerset, repeated determinism, deps/audit/licenses, beta + MSRV, bounded real fuzz, cold/perf trends |
| Weekly campaign | weekly on `main` | longer fuzz, native-language MCP soak, real-APK endurance, repeated golden run, base/candidate efficiency benchmarks, dependency-update trial |
| Manual deep | dispatch or `/rd-sentinel` command | maintainer-selected profile, fuzz duration, and changed-code mutation budget for a PR or `main` |
| Mutation trial | `sentinel:mutants`, deep PR, weekly | `cargo mutants --in-diff` asks whether tests catch faults injected into changed production Rust |

GitHub-hosted jobs still have a six-hour ceiling, so a literal 24-hour fuzz
campaign remains a self-hosted/manual campaign. Sentinel reports the executed
duration and never describes a shorter hosted run as 24 hours.

## 6. Dependency-update behavior

Dependabot opens grouped minor/patch Cargo updates, separate major updates,
fuzz-workspace updates, and GitHub-Actions updates. Version updates use
explicit release-age cooldowns, while security updates remain immediate.
Sentinel then:

- parses the exact `Cargo.lock` delta and classifies semver movement;
- blocks source changes inside a bot dependency PR;
- rejects new non-crates.io sources and unapproved licenses;
- checks every resolved package's declared Rust version against MSRV 1.76;
- forces deep feature/test campaigns for native, JIT, parser, crypto,
  compression, rendering, and build-script dependencies;
- computes direct and reachable workspace consumers for changed crates and
  calls out build/proc-macro/native-link blast radius;
- runs `cargo-deny` beside `cargo audit`, optionally queries OSV, and emits a
  deterministic CycloneDX SBOM for the resolved graph;
- requires maintainer approval for major-version movement; and
- never enables auto-merge.

## 7. Consequences

**Positive:** regressions are caught against history and over time, not only
against today's test expectations. Findings are reproducible, deduplicated,
and attached to a base/head pair. Scheduled work can be much heavier than PR
CI without slowing every contribution.

**Costs:** deep campaigns consume substantial runner time; A/B builds are
expensive; hosted performance numbers remain noisy; and a strict bot
occasionally blocks an intentional change until the baseline/exception change
is reviewed separately. Those costs are explicit and preferable to a bot
that silently learns to accept every regression.

**Non-goals:** Sentinel does not replace CI, code review, release management,
or Dependabot. It does not commit fixes, rewrite baselines, approve its own
policy changes, or auto-merge.

## 8. Rollout

1. Land this ADR, the bot, policy, baseline, workflows, and Dependabot config
   in one reviewed PR; apply `sentinel:policy-approved` to that bootstrap PR.
2. Create/confirm the Sentinel labels (the publisher can create missing
   labels when it first runs).
3. Require normal CI and RD Sentinel in branch protection/rulesets for
   `main`.
4. Run one manual nightly campaign and one weekly-profile campaign; fix any
   bot defects without weakening a threshold in the same change that exposes
   a product regression.
5. Add a self-hosted runner only if the project wants unattended 24-hour
   fuzzing; keep its token read-only for analysis.

## 9. v1.1 amendment — research-driven hardening

The 1.1 release incorporates current Dependabot cooldown guidance, GitHub
Actions hardening guidance, Renovate-style visibility/blast-radius thinking,
cargo-deny/OSV supply-chain checks, SARIF reporting, named flaky-test
extraction, downstream workspace test selection, and diff-scoped mutation
testing. It does not change the fundamental trust split or enable auto-merge.

Three rollout choices are deliberate:

1. **Mutation survivors are blocking in an explicit mutants/deep campaign and
   advisory in a trend context.** A repository-wide zero-survivor mandate
   should follow measured baseline evidence, not be imposed retroactively on
   unchanged code.
2. **Full-SHA action pins are adopted before a workflow lockfile.** GitHub's
   `actions.lock` support was still a preview-style adoption path at research
   time; hand-writing a fake lockfile would be worse than reviewed SHAs.
3. **CODEOWNERS routes review but is not yet a required approval source.**
   Requiring code-owner review while the owner is the sole author would create
   an unmergeable repository. Revisit when a second maintainer/team exists.

Cargo-vet, a blocking OpenSSF Scorecard gate, and rustdoc-based semver
checking remain future candidates. They should be adopted only with real
audit corpora, measured false-positive rates, and a clear owner — not as
decorative checks.

## 10. v1.2 amendment — named evidence and GitHub-native publication

The 1.2 release keeps the same trust split and no-auto-merge rule. It closes
three evidence/presentation gaps found after 1.1:

1. **No-shrink becomes identity-aware.** Counts remain useful floors, but the
   static audit also compares named Rust tests by `(scope, name)` (counting
   duplicate names in one crate) and fuzz targets by crate-qualified name.
   Same-crate moves are still tolerated; a same-count swap that deletes the
   meaningful entry is a finding.
2. **OSV pagination is completed, not sampled.** `querybatch` pagination is
   per query, so Sentinel resubmits only unfinished package queries with their
   own page tokens, bounded by `campaigns.osv_max_pages`, and records
   pages/pending/truncated metrics.
3. **Publication gains GitHub-native review surfaces without moving trust.**
   Only the default-branch publisher may create the annotated
   `rd-sentinel/gate` Check Run or upload SARIF to code scanning, and only
   after the fingerprinted report has been verified against the live PR. If
   GitHub rejects that presentation layer (for example a fork head or a
   disabled code-scanning surface), the error is recorded and the verified
   comment/labels/commit-status decision remains authoritative.

## Emenda 2026-10-25 — orçamento absoluto do `rd perf` já vermelho na base (issue #283)

O `rd perf` sai com código 1 quando um orçamento falha (PRF-06). O `_run_perf` usava esse código para duas coisas: "a medição rodou" e "o orçamento passou". Na campanha `compare`, a base roda o mesmo `_run_perf`. Com a `main` acima de 1500 ms emulados, o check obrigatório da **base** nascia `fail`, e assim qualquer PR ficava bloqueado, inclusive um que melhore o cold start (catch-22).

Decisão:
1. **A medição e o veredito ficam separados.** Os checks `perf-rd-perf-*` passam quando há números (p50/max). Sem JSON válido ou com timeout, eles falham e bloqueiam, como antes.
2. **Orçamento estourado na base** gera o finding P2 `perf-base-budget-preexisting`, que é advisory: a base é referência, não é a mudança em revisão.
3. **Orçamento estourado no candidato** gera o check obrigatório `perf-budget-candidate-<apk>`:
   - `fail` se a base passou no orçamento, porque a mudança regrediu;
   - `fail` se a base não foi medida nesta execução, porque nesse caso não dá para afirmar que é pré-existente;
   - `pass` (pré-existente) só quando a base também estoura. Nesse caso, quem bloqueia regressões são os gates pareados que já existiam (`perf-ab-p50-regression`: ratio > 1,10 e +25 ms; `perf-ab-max-regression`).

Nenhum limiar muda. `STARTUP_BUDGET_EMULATED_MS` continua em 1500 e `policy.json` não muda. Piorar continua bloqueado; melhorar deixa de ser bloqueado. O orçamento absoluto volta a valer sozinho assim que a `main` ficar abaixo dele.
