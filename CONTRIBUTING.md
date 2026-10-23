# Contributing to AndroSwitch

AndroSwitch is a Horizon adaptation of
[RustyDroid](https://github.com/nadabgfccvd/RustyDroid). Preserve the original
authors' credit, history and licenses. The unmodified upstream contribution
guide is retained in [CONTRIBUTING.rustydroid.md](CONTRIBUTING.rustydroid.md);
this file describes contributions to this fork.

## Before changing code

Read the [roadmap](docs/ANDROSWITCH_ROADMAP.md),
[architecture](docs/ARCHITECTURE.md), [compatibility inventory](docs/ANDROSWITCH_COMPATIBILITY.md)
and [workspace guide](AGENTS.md). Work follows M0 → M1 → M2 → M3 → M4 before
launcher work. An upstream feature is not a verified Horizon feature.

Fix shared Android API/ABI behavior. Do not add package-name checks, game
profiles, fixed title offsets, hard-coded game libraries or fabricated success
responses. Unsupported behavior must identify the failing operation/API and
subsystem. Test call descriptors belong in fixtures, outside runtime behavior.

The device core uses `no_std + alloc`. Route OS behavior through platform
adapters and keep Python, Unicorn, subprocesses and desktop globals out of
the device dependency graph. Host tools may use std and reference backends.
Keep unsafe code confined to documented FFI boundaries under local lint
allowances; do not weaken the workspace-wide safe-code rule.

Significant changes to the platform boundary, native loader, VM/JNI reference
model, storage or lifecycle should include an [architecture decision](docs/adr/).

## Validate the change

Use focused checks appropriate to the affected code, then the relevant gates:

```powershell
./scripts/check-m0.ps1
./scripts/check-m1.ps1
python -X utf8 scripts/check_md_links.py
git diff --check
```

M1 checks need a JDK/Android SDK fixture build; see [building](docs/BUILDING.md).
Changes to the device parser/interpreter or libnx bridge also need the
[Ryujinx E2E gate](docs/M1_E2E.md). Keep host, emulator and physical-device
results separate. A compile is not execution evidence; an emulator pass is
not physical Switch verification.

The [testing guide](docs/TESTING.md) explains Cargo feature isolation, the
known immutable-upstream baseline failures and which checks are unavailable
on some hosts. Record commands and unrun checks honestly.

## Fixtures, evidence and attribution

- Keep APKs, generated libraries, NROs, toolchains and verbose run output in
  ignored cache/artifact directories. Commit fixture source and compact evidence.
- Record external URLs/revisions/hashes in [the fixture lock](fixtures/corpus.lock.json).
  Do not modify an APK to make it pass and report it as unmodified compatibility.
- New platform side tables holding VM objects must participate in GC rooting
  before callbacks, threading or JNI references use them.
- Updates must preserve the [credits](CREDITS.md), [notices](NOTICE.md) and
  original license files. Source imports need origin, revision and license.

## Pull requests and issue reports

Describe the problem, resulting behavior and validation. Include fixture
identity, exact runtime commit, backend and reproduction commands for a
compatibility issue. State which API or ABI contract is missing; an engine
name alone is not enough evidence.

Conventional commit messages are preferred. AI-assisted contributions are
welcome; disclose their use and subject them to the same tests and maintainer
review as other changes. Update the roadmap/inventory when supported behavior
changes. Avoid applying the retained Linux/Moto E5 or MCP policies as new
Switch compatibility claims.

Report security-sensitive issues using [SECURITY.md](SECURITY.md), rather
than attaching exploit payloads to a public compatibility issue.
