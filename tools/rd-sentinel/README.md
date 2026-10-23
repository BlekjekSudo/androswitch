# RD Sentinel

Implementation of the anti-regression bot defined in
[`docs/adr/010-rd-sentinel-anti-regression-bot.md`](../../docs/adr/010-rd-sentinel-anti-regression-bot.md).
Operational guidance lives in [`docs/rd-sentinel.md`](../../docs/rd-sentinel.md).

## Layout

- `rd_sentinel.py` — executable wrapper
- `sentinel/model.py` — report schema, fingerprints, verdicts, policy/baseline
  validation, secret redaction, Markdown, and SARIF
- `sentinel/audit.py` — trusted diff/contract/dependency/workflow static audit
  with downstream workspace-package selection
- `sentinel/campaigns.py` — tests, feature, determinism, perf, A/B, fuzz,
  mutation, soak, future-toolchain, SBOM, OSV, and dependency campaigns
- `sentinel/mcp_endurance.py` — application-neutral persistent MCP session
  endurance for pinned launchable APKs
- `sentinel/github_api.py` — trusted publisher (comment, labels, status,
  Check Run annotations, code-scanning SARIF) and maintainer commands
- `sentinel/cli.py` — command-line entry points, including raw
  `pull_request`/`merge_group` event context
- `tests/` — standard-library unit tests

Only Python's standard library is used at runtime.

## Quick start

```sh
python3 tools/rd-sentinel/rd_sentinel.py plan --base origin/main --head HEAD
python3 tools/rd-sentinel/rd_sentinel.py audit \
  --base origin/main --head HEAD --execute --strict-skips
python3 -m unittest discover -s tools/rd-sentinel/tests -v
```

Without `--execute`, planned campaigns are recorded as **planned/incomplete**,
never as passes.
