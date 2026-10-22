# RD Sentinel — anti-regression bot operator guide

RD Sentinel is RustyDroid's dedicated Dependabot-like guardian. Normal CI
remains the fast, bounded pull-request gate. Sentinel handles the evidence CI
cannot afford or cannot express at one revision: differential comparisons,
long campaigns, historic budgets, environment drift, evidence shrinkage, and
deduplicated GitHub triage.

The architecture and rationale are recorded in
[ADR 010](adr/010-rd-sentinel-anti-regression-bot.md).

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

## Campaigns

### PR risk audit (`auto`)

The diff is classified into parsers, VM core, native/security, agent/protocol,
render/UI, CLI contract, dependencies/build, CI security, performance, fuzz,
tests, docs, and Sentinel itself. That classification selects:

- targeted package tests;
- relevant Cargo feature combinations;
- repeated tests and real-APK byte-stability probes;
- actual bounded fuzz execution for parser changes;
- `rd perf` and `scripts/cold26.py` for performance-sensitive changes;
- full A/B and real-APK campaigns for high-risk diffs or `sentinel:deep`.

Draft PRs receive the static report without executing expensive campaigns.
The verdict stays incomplete until a ready/synchronized revision executes
the plan.

### Nightly deep

Runs Monday-Saturday on `main`:

- `cargo test --workspace --all-features`;
- broad feature matrix for VM/agent/CLI/registry/JNI/NDK;
- repeated determinism/flaky hunts;
- resolved dependency license/source/MSRV audit plus `cargo audit` when the
  workflow has installed it;
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

The publisher applies only result/risk labels such as `sentinel:pass`,
`sentinel:blocked`, `sentinel:P0`, `sentinel:perf-risk`,
`sentinel:security-risk`, `sentinel:dependency-risk`, `sentinel:risk-high`,
and `sentinel:risk-critical`. It removes stale Sentinel result/risk labels
when publishing a newer report.

## Dependabot PR vetting

Dependabot groups Cargo minor/patch updates, separates majors, checks the two
fuzz workspaces monthly, and updates GitHub Actions weekly. Sentinel then:

1. parses the exact `Cargo.lock` delta;
2. blocks non-dependency files in a Dependabot PR;
3. classifies major/minor/patch/prerelease and 0.x minor-as-major movement;
4. checks licenses, crates.io-only sources, and dependency Rust versions
   against MSRV 1.76;
5. runs the dependency campaign and feature matrix;
6. requires the major-approval label for major movement; and
7. never auto-merges.

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

Validate/render an artifact:

```sh
python3 tools/rd-sentinel/rd_sentinel.py validate \
  --report target/rd-sentinel/rd-sentinel-report.json
python3 tools/rd-sentinel/rd_sentinel.py render \
  --report target/rd-sentinel/rd-sentinel-report.json
```

Generate baseline evidence without modifying the baseline:

```sh
python3 tools/rd-sentinel/rd_sentinel.py baseline-proposal \
  --base origin/main --head HEAD --execute
```

Exit codes are `0=pass`, `1=blocked`, `2=bot/config error`, and
`3=incomplete`. An incomplete local run still writes the JSON/Markdown report,
but it must not be quoted as a pass.

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
of the bug, not evidence that it disappeared.

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

## Known limits

- Hosted performance runners are noisy. Sentinel therefore pairs absolute
  budgets with A/B ratios and absolute deltas, but a human still reviews
  borderline evidence.
- Static diff heuristics are intentionally conservative and can produce P2
  review findings. P0/P1 rules are reserved for contracts, security,
  determinism, evidence loss, and measured regressions.
- `cargo audit` is installed by workflows that need it. On a local runner
  without it, the report marks that evidence skipped/incomplete.
- The bot does not prove Android compatibility beyond the pinned real-APK,
  native, contract, and golden evidence it actually executes.
