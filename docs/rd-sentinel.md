# RD Sentinel — anti-regression bot operator guide

RD Sentinel is RustyDroid's dedicated Dependabot-like guardian. Normal CI
remains the fast, bounded pull-request gate. Sentinel handles the evidence CI
cannot afford or cannot express at one revision: differential comparisons,
long campaigns, historic budgets, environment drift, evidence shrinkage, and
deduplicated GitHub triage.

The architecture and rationale are recorded in
[ADR 010](adr/010-rd-sentinel-anti-regression-bot.md).

## What changed in v1.1

- Risk is now explainable: every report lists the factors and points behind
  its 0–100 score instead of asking reviewers to trust one opaque number.
- Targeted package selection follows reverse workspace dependencies, so a
  parser change also exercises the workspace consumers above it.
- Public-API checks compare crate-level item inventories, reducing false
  alarms for same-crate moves while still blocking removals/signature changes.
- Workflow changes are linted for mutable action refs, shell template
  injection, persisted checkout credentials, missing permissions, and
  dangerous-trigger candidate checkouts.
- Dependency evidence now includes duplicate/mass-update analysis, reachable
  workspace blast radius, `cargo-deny`, optional `cargo-machete`, an OSV batch
  query when network is allowed, and a deterministic CycloneDX SBOM artifact.
- Reports are also emitted as SARIF 2.1.0. The fingerprinted JSON remains the
  authoritative completeness/verdict record.
- A `mutants` profile runs `cargo mutants --in-diff` on committed Rust
  changes, asking whether the tests actually catch injected faults.
- Cold26 A/B comparisons include sample counts and median absolute deviation;
  relative deltas must clear a noise-aware floor, while absolute <26 ms
  budgets remain strict.

## What runs where

| Bot part | Trigger | Permissions | Purpose |
|---|---|---|---|
| Dependabot | weekly/monthly schedule | GitHub-managed | proposes Cargo, fuzz-crate, and Actions updates |
| `RD Sentinel` | PR, merge queue, manual dispatch | contents/pull-requests **read** | trusted-base policy audit plus risk-selected campaigns |
| `RD Sentinel Deep` | nightly, weekly, manual dispatch | contents/pull-requests **read** | long and broad campaigns on `main` |
| `RD Sentinel Publish` | `workflow_run` completion | issues/PR/statuses **write** | verifies the artifact against GitHub, comments, labels, issues, verified status |
| `RD Sentinel Command` | issue comment | actions/issues/PR **write**, contents read | maintainer-only campaign dispatch |

Candidate-executing analysis jobs never receive a publishing token.

## The strict rules

Sentinel fails closed:

- **P0/P1 findings block.** Examples: forged/stale report identity, protected
  Sentinel paths without approval, secrets, `unsafe` outside approved crates,
  fuzz crashes, nondeterministic frames, cold-start median at/above 26 ms,
  incompatible dependency sources.
- **Required skipped evidence is not a pass.** In GitHub campaigns it blocks;
  locally it produces `incomplete` unless `--strict-skips` is also set.
- **Evidence cannot shrink silently.** Test attributes, JVM golden vectors,
  fuzz targets, and MCP tool count are compared to both the base revision and
  `.github/rd-sentinel/baseline.json`.
- **Performance budgets cannot be relaxed to hide a regression.** The
  1,500 ms real-execution startup budget and the <26 ms process-cold snapshot
  presentation budget are separate contracts and must stay separate.
- **A/B regressions block even when absolute budgets still pass.** The bot
  compares base and candidate `inspect`, `rd perf`, snapshot cold start, frame
  hashes, binary size, and paired efficiency workloads.
- **Exceptions expire.** Every exception in
  `.github/rd-sentinel/policy.json` needs an owner, precise scope, reason, and
  expiry date. Expired or incomplete exceptions block.
- **The trusted policy and baseline validate themselves.** Weakened blocking
  severities, malformed numeric limits, invalid deep-dependency regexes,
  missing protection for Sentinel's own policy/baseline/workflow/code,
  duplicate or over-broad exceptions, and malformed corpus contracts are
  findings rather than silent configuration drift.
- **Workflow privilege cannot drift quietly.** Sentinel workflow actions are
  pinned to full commit SHAs, and changed workflows are checked for unpinned
  actions, user-controlled expressions in `run:` scripts, credential
  persistence, and missing explicit permissions.

## Campaigns

### PR risk audit (`auto`)

The diff is classified into parsers, VM core, native/security, agent/protocol,
render/UI, CLI contract, dependencies/build, CI security, performance, fuzz,
tests, docs, and Sentinel itself. That classification selects:

- tests for directly changed packages plus their downstream workspace
  dependents;
- relevant Cargo feature combinations derived from the checked-out manifests
  (with the v1.0 matrix retained as an unreadable-manifest fallback);
- repeated tests with named flaky-test extraction and real-APK byte-stability
  probes;
- actual bounded fuzz execution for parser changes;
- `rd perf` and `scripts/cold26.py` for performance-sensitive changes;
- full A/B and real-APK campaigns for high-risk diffs or `sentinel:deep`.

Draft PRs receive the static report without executing expensive campaigns.
The verdict stays incomplete until a ready/synchronized revision executes
the plan.

Use `--profile mutants` or the `sentinel:mutants` label to add changed-code
mutation testing to a PR. Deep runs include it as a required component. A
test-only diff or an uncommitted worktree cannot provide a trustworthy
mutation result and is reported as such.

### Nightly deep

Runs Monday-Saturday on `main`:

- `cargo test --workspace --all-features`;
- broad feature matrix for VM/agent/CLI/registry/JNI/NDK;
- repeated determinism/flaky hunts;
- resolved dependency license/source/MSRV audit, `cargo audit`,
  `cargo-deny`, changed-dependency blast-radius analysis, an OSV query when
  network is enabled, and a CycloneDX SBOM artifact;
- beta Rust and Rust 1.76 trials;
- release binary size, `rd perf`, process-cold snapshot, and real-APK probes;
- all five fuzz targets for the configured bounded duration;
- isolated semver-compatible dependency-update trial in the weekly core job.

### Weekly campaign

Runs Sunday on `main` and adds:

- a longer hosted fuzz campaign;
- base-versus-head A/B performance/efficiency/frame comparisons;
- application-neutral persistent-session MCP endurance on pinned launchable
  APKs (install/launch, observe, UI tree, screenshots, virtual time, app info,
  and runtime health), plus `scripts/comprix-regression.py` only when a real
  Comprix APK is explicitly supplied;
- `scripts/mcp-session-soak.py` over the signed native-language APK matrix;
- three repetitions of the JVM golden comparison;
- changed-code mutation testing for the weekly base/head range;
- an isolated `cargo update` trial.

GitHub-hosted jobs have a six-hour ceiling. Therefore the weekly hosted fuzz
run is **not** called a 24-hour campaign. For an actual 24-hour run, dispatch
or run `scripts/fuzz-campaign.sh 86400` on a self-hosted/read-only runner or a
maintainer machine and retain the artifacts.

## Maintainer commands

Only GitHub author associations OWNER, MEMBER, and COLLABORATOR may use:

```text
/rd-sentinel rerun
/rd-sentinel deep
/rd-sentinel perf
/rd-sentinel fuzz 900
/rd-sentinel deps
/rd-sentinel determinism
/rd-sentinel mutants 45
/rd-sentinel baseline propose
/rd-sentinel status
/rd-sentinel help
```

Commands only dispatch evidence campaigns. They cannot approve policy, change
a baseline, push, dismiss reviews, or merge. `status` links the newest report
comment authored by `github-actions[bot]`; a marker planted by another account
is ignored.

Approval is deliberately label-based and human-applied:

| Label | Meaning |
|---|---|
| `sentinel:policy-approved` | maintainer reviewed bot code/policy/workflow changes |
| `sentinel:baseline-approved` | maintainer reviewed before/after evidence for a baseline change |
| `sentinel:dependency-major-approved` | maintainer reviewed a major dependency movement |
| `sentinel:deep` | force the deep PR campaign |
| `sentinel:mutants` | force changed-code mutation testing |

The publisher applies only result/risk labels such as `sentinel:pass`,
`sentinel:blocked`, `sentinel:P0`, `sentinel:perf-risk`,
`sentinel:security-risk`, `sentinel:dependency-risk`, `sentinel:test-gap`,
`sentinel:risk-high`, and `sentinel:risk-critical`. It removes stale Sentinel
result/risk labels
when publishing a newer report.

## Dependabot PR vetting

Dependabot groups Cargo minor/patch updates, separates majors, checks the two
fuzz workspaces monthly, and updates GitHub Actions weekly. Version updates
use explicit release-age cooldowns (14 days for Cargo majors, 7 for minors and
GitHub Actions, and 3 for Cargo patches); security updates are not delayed by
those cooldowns. Sentinel then:

1. parses the exact `Cargo.lock` delta;
2. blocks non-dependency files in a Dependabot PR;
3. classifies major/minor/patch/prerelease and 0.x minor-as-major movement;
4. checks licenses, crates.io-only sources, and dependency Rust versions
   against MSRV 1.76;
5. reports duplicate-version growth and mass updates, then runs feature,
   determinism, and A/B evidence for those wider movements;
6. computes direct/reachable workspace consumers and flags changed build,
   proc-macro, build-script, native-link, and widely consumed crates;
7. runs `cargo-deny`, `cargo audit`, and (with network) OSV, and writes a
   CycloneDX SBOM;
8. requires the major-approval label for major movement; and
9. never auto-merges.

If an update only fails on Rust beta or future dependencies, do not ignore it:
that is tomorrow's red `main`. Either pin deliberately in the product PR or
prepare the migration before the stable release arrives.

## Local use

Static plan (no campaigns executed, therefore not a passing audit):

```sh
python3 tools/rd-sentinel/rd_sentinel.py plan \
  --base origin/main --head HEAD
```

Executable risk-selected audit:

```sh
python3 tools/rd-sentinel/rd_sentinel.py audit \
  --base origin/main --head HEAD --execute --strict-skips
```

Audit uncommitted work against the current commit:

```sh
python3 tools/rd-sentinel/rd_sentinel.py audit \
  --base HEAD --head WORKTREE --execute
```

Bounded fuzz only:

```sh
python3 tools/rd-sentinel/rd_sentinel.py audit \
  --base origin/main --head HEAD --profile fuzz \
  --component static --component fuzz --Fuzz_SECONDS 600 --execute
```

Changed-code mutation testing only (requires a clean committed range and
`cargo-mutants`):

```sh
python3 tools/rd-sentinel/rd_sentinel.py audit \
  --base origin/main --head HEAD --profile mutants \
  --component static --component mutation \
  --mutation-minutes 45 --execute --strict-skips
```

Validate/render an artifact:

```sh
python3 tools/rd-sentinel/rd_sentinel.py validate \
  --report target/rd-sentinel/rd-sentinel-report.json
python3 tools/rd-sentinel/rd_sentinel.py render \
  --report target/rd-sentinel/rd-sentinel-report.json
python3 tools/rd-sentinel/rd_sentinel.py sarif \
  --report target/rd-sentinel/rd-sentinel-report.json \
  --out target/rd-sentinel/rd-sentinel-report.sarif
```

Generate baseline evidence without modifying the baseline:

```sh
python3 tools/rd-sentinel/rd_sentinel.py baseline-proposal \
  --base origin/main --head HEAD --execute
```

Exit codes are `0=pass`, `1=blocked`, `2=bot/config error`, and
`3=incomplete`. An incomplete local run still writes JSON, Markdown, and SARIF
reports, but it must not be quoted as a pass.

## Corpus policy

The pinned launchable corpus currently contains Primftpd 71
(`org.primftpd_71.apk`) with its SHA-256 in the baseline. The public F-Droid
client download is a moving latest build, so Sentinel treats it as a
structured-behavior/determinism probe, not a pinned launch baseline.

For the 100 MB+ process-cold gate, set:

```sh
RD_SENTINEL_HEAVY_APK=/path/to/launchable-heavy.apk
```

A missing heavy corpus is reported as missing evidence. Sentinel does not
manufacture or imply a heavyweight pass. Never commit the heavy APK.

The Comprix journey is application-specific, so it is never run against
Primftpd or another unrelated APK. To include it in a real-APK campaign, set
`RD_SENTINEL_COMPRIX_APK=/path/to/comprix.apk`; Sentinel then runs the
existing Portuguese UI script only against that explicit corpus.

## Responding to findings

### Fuzz crash

1. Preserve the uploaded artifact before rerunning.
2. Reproduce and minimize with the command printed by `fuzz-campaign.sh`.
3. Add the minimized case as a unit/integration/fuzz seed regression.
4. Fix the parser without adding a production panic path.

### Flaky or nondeterministic output

Treat process/thread globals, time, random seeds, filesystem ordering, and
shared stdout routing as prime suspects. A retry that turns green is evidence
of the bug, not evidence that it disappeared. Sentinel names tests that both
passed and failed across repetitions; start with those tests rather than the
last failing command alone.

### Surviving mutant

A surviving mutant means the changed behavior can be altered without any test
noticing. Add an assertion for the public behavior, not a test that merely
mirrors a private implementation detail. If the mutant is truly equivalent,
record why in a dated, narrowly scoped exception. A mutation timeout is not a
kill and needs its slow/pathological path investigated.

### Performance

Separate three numbers:

- `rd perf`: real guest execution against the emulated Moto E5 budget;
- `cold26`: new live process presenting a previously captured deterministic
  snapshot frame;
- first interaction/materialization: reported separately from first frame.

Do not use snapshot timing to claim the first-ever guest launch is <26 ms,
and never raise a budget in the same PR that introduces the slowdown.

### Baseline change

1. Run the same campaign on base and candidate.
2. Open a PR that changes only the baseline/ADR evidence, or clearly isolate
   that commit.
3. Link both reports and explain why the new behavior is correct.
4. Apply `sentinel:baseline-approved` after maintainer review.

## Required repository settings

Files alone cannot make a check required. After this bot lands, configure
branch protection or a repository ruleset for `main` to require at least:

- the existing CI jobs;
- `RD Sentinel / audit`;
- optionally the publisher's `rd-sentinel/gate` commit status;
- conversation resolution and no force-pushes.

Also restrict who can apply the Sentinel approval labels. The bot honors those
labels as maintainer decisions, so label permission is security-sensitive.

`.github/CODEOWNERS` routes Sentinel, supply-chain, native-boundary, and
cold-start-evidence changes to the repository owner. Do **not** enable
“Require review from Code Owners” while that owner is also the sole PR author:
GitHub does not let authors approve their own PRs, so the rule would deadlock
merges. Enable it when a second maintainer or team exists.

## Known limits

- Hosted performance runners are noisy. Sentinel therefore pairs absolute
  budgets with A/B ratios and absolute deltas, but a human still reviews
  borderline evidence.
- Static diff heuristics are intentionally conservative and can produce P2
  review findings. P0/P1 rules are reserved for contracts, security,
  determinism, evidence loss, and measured regressions.
- `cargo audit`, `cargo-deny`, `cargo-machete`, and `cargo-mutants` are
  installed by workflows that need them. On a local runner without one, the
  report marks that evidence skipped/incomplete rather than inventing it.
- OSV is queried only when network is explicitly allowed. Sentinel consumes
  the first batch page and discloses if OSV returns a pagination token;
  `cargo audit`/RustSec remains part of the evidence.
- The public-API inventory is a conservative source-level comparison, not a
  rustdoc/semver proof. It intentionally ignores same-crate file moves;
  associated items, re-exports, trait implementations, and generic bounds can
  still require human review or future `cargo-semver-checks` adoption.
- Diff-scoped mutation testing gives fast changed-code evidence. It does not
  prove the unchanged workspace has a high global mutation score.
- Sentinel workflows use reviewed full-SHA action pins. GitHub's newer
  workflow dependency lockfile remains a separate adoption decision; Sentinel
  does not fabricate an `actions.lock` file.
- The bot does not prove Android compatibility beyond the pinned real-APK,
  native, contract, and golden evidence it actually executes.
