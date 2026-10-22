"""RD Sentinel report model, policy handling, and rendering.

The module deliberately uses only the Python standard library. Reports are
plain JSON dictionaries so a trusted publisher can validate an artifact that
was produced by a read-only analysis job without importing candidate code.
"""

from __future__ import annotations

import datetime as _dt
import fnmatch
import hashlib
import html
import json
import re
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Iterable, Mapping, MutableMapping, Sequence

BOT_NAME = "RD Sentinel"
BOT_VERSION = "1.0.0"
SCHEMA_VERSION = 1
REPORT_MARKER = "<!-- rd-sentinel-report -->"
SEVERITY_RANK = {"P0": 0, "P1": 1, "P2": 2, "P3": 3}
CHECK_RANK = {
    "pass": 0,
    "planned": 1,
    "skip": 2,
    "warn": 3,
    "fail": 4,
    "error": 5,
}

_SECRET_PATTERNS = (
    re.compile(r"ghp_[A-Za-z0-9_]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(r"(?i)(authorization\s*:\s*(?:bearer|token)\s+)[^\s'\"]+"),
    re.compile(r"(?i)((?:api[_-]?key|access[_-]?token|secret|password)\s*[=:]\s*)[^\s'\"]+"),
)


def utc_now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def today_utc() -> _dt.date:
    return _dt.datetime.now(_dt.timezone.utc).date()


def canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8", "replace")).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path}: expected a JSON object")
    return value


def redact(text: str | None, *, repo: Path | None = None, limit: int = 6000) -> str:
    if not text:
        return ""
    out = str(text)
    for pattern in _SECRET_PATTERNS:
        out = pattern.sub(lambda m: (m.group(1) if m.lastindex else "") + "[REDACTED]", out)
    if repo is not None:
        try:
            out = out.replace(str(repo.resolve()), "$REPO")
        except OSError:
            out = out.replace(str(repo), "$REPO")
    out = out.replace("/home/user", "$HOME")
    if len(out) > limit:
        marker = f"\n... [truncated {len(out) - limit} chars] ...\n"
        keep = max(0, limit - len(marker))
        out = out[: keep // 2] + marker + out[-keep // 2 :]
    return out


def match_any(path: str, patterns: Iterable[str]) -> bool:
    normalized = path.replace("\\", "/")
    if normalized.startswith("./"):
        normalized = normalized[2:]
    for raw in patterns:
        pattern = str(raw).replace("\\", "/")
        if fnmatch.fnmatchcase(normalized, pattern):
            return True
        if pattern.endswith("/**") and normalized.startswith(pattern[:-3] + "/"):
            return True
        if pattern.startswith("**/") and fnmatch.fnmatchcase(normalized.rsplit("/", 1)[-1], pattern[3:]):
            return True
    return False


def _normalized_evidence(evidence: str) -> str:
    value = re.sub(r"0x[0-9a-fA-F]+", "0x…", str(evidence))
    value = re.sub(r"\b[0-9a-f]{40,64}\b", "<hash>", value)
    value = re.sub(r"\s+", " ", value).strip().lower()
    return value[:500]


@dataclass(slots=True)
class Finding:
    id: str
    severity: str
    area: str
    title: str
    evidence: str
    path: str | None = None
    line: int | None = None
    remediation: str | None = None
    excepted_by: str | None = None
    fingerprint: str = field(init=False)

    def __post_init__(self) -> None:
        if self.severity not in SEVERITY_RANK:
            raise ValueError(f"invalid severity {self.severity!r}")
        basis = {
            "id": self.id,
            "area": self.area,
            "path": self.path,
            "evidence": _normalized_evidence(self.evidence),
        }
        self.fingerprint = sha256_text(canonical_json(basis))[:20]

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(slots=True)
class CheckResult:
    id: str
    title: str
    status: str
    required: bool = True
    component: str = "static"
    command: list[str] = field(default_factory=list)
    exit_code: int | None = None
    duration_ms: float | None = None
    summary: str = ""
    metrics: dict[str, Any] = field(default_factory=dict)
    stdout_tail: str = ""
    stderr_tail: str = ""

    def __post_init__(self) -> None:
        if self.status not in CHECK_RANK:
            raise ValueError(f"invalid check status {self.status!r}")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def parse_date(value: Any) -> _dt.date | None:
    if not isinstance(value, str):
        return None
    try:
        return _dt.date.fromisoformat(value)
    except ValueError:
        return None


def apply_exceptions(
    findings: list[Finding], policy: Mapping[str, Any], *, today: _dt.date | None = None
) -> list[Finding]:
    """Apply dated exceptions and surface malformed/expired exceptions.

    An active exception never deletes evidence: it downgrades a matching
    finding to P3 and records the exception id. Expired or malformed
    exceptions become blocking findings of their own.
    """

    current = today or today_utc()
    exceptions = policy.get("exceptions", [])
    exception_findings: list[Finding] = []
    if not isinstance(exceptions, list):
        return findings + [
            Finding(
                id="invalid-policy-exceptions",
                severity="P0",
                area="policy",
                title="Policy exceptions are not a list",
                evidence=f"exceptions has type {type(exceptions).__name__}",
                remediation="Restore the versioned policy schema.",
            )
        ]

    for raw in exceptions:
        if not isinstance(raw, Mapping):
            exception_findings.append(
                Finding(
                    id="invalid-policy-exception",
                    severity="P0",
                    area="policy",
                    title="Policy exception is not an object",
                    evidence=repr(raw)[:300],
                    remediation="Every exception needs id, finding_id, path_glob, owner, reason, and expires.",
                )
            )
            continue
        ex_id = str(raw.get("id", ""))
        finding_id = str(raw.get("finding_id", ""))
        path_glob = str(raw.get("path_glob", ""))
        expires = parse_date(raw.get("expires"))
        missing = [
            name
            for name, value in (
                ("id", ex_id),
                ("finding_id", finding_id),
                ("path_glob", path_glob),
                ("owner", str(raw.get("owner", ""))),
                ("reason", str(raw.get("reason", ""))),
            )
            if not value.strip()
        ]
        if expires is None:
            missing.append("valid expires date")
        if missing:
            exception_findings.append(
                Finding(
                    id="invalid-policy-exception",
                    severity="P0",
                    area="policy",
                    title="Policy exception is incomplete",
                    evidence=f"exception {ex_id or '<unnamed>'} is missing: {', '.join(missing)}",
                    remediation="Complete or remove the exception in a separately approved policy change.",
                )
            )
            continue
        if expires < current:
            exception_findings.append(
                Finding(
                    id="expired-policy-exception",
                    severity="P1",
                    area="policy",
                    title=f"Policy exception {ex_id} has expired",
                    evidence=f"expires={expires.isoformat()} finding_id={finding_id} path={path_glob}",
                    remediation="Remove the exception or renew it with fresh evidence and policy approval.",
                )
            )
            continue
        for finding in findings:
            if finding.id != finding_id or finding.path is None:
                continue
            if match_any(finding.path, (path_glob,)):
                finding.severity = "P3"
                finding.excepted_by = ex_id
                finding.evidence += f" [excepted by {ex_id} until {expires.isoformat()}]"
    return findings + exception_findings


def _fingerprint_payload(report: Mapping[str, Any]) -> dict[str, Any]:
    checks = []
    for check in report.get("checks", []):
        if isinstance(check, Mapping):
            checks.append(
                {
                    "id": check.get("id"),
                    "status": check.get("status"),
                    "required": check.get("required"),
                    "exit_code": check.get("exit_code"),
                    "metrics": check.get("metrics", {}),
                }
            )
    findings = []
    for finding in report.get("findings", []):
        if isinstance(finding, Mapping):
            findings.append(
                {
                    "id": finding.get("id"),
                    "severity": finding.get("severity"),
                    "area": finding.get("area"),
                    "path": finding.get("path"),
                    "fingerprint": finding.get("fingerprint"),
                    "excepted_by": finding.get("excepted_by"),
                }
            )
    return {
        "schema": report.get("schema"),
        "bot_version": report.get("bot_version"),
        "base_sha": report.get("base_sha"),
        "head_sha": report.get("head_sha"),
        "campaign": report.get("campaign"),
        "profile": report.get("profile"),
        "areas": report.get("areas", []),
        "metrics": report.get("metrics", {}),
        "checks": sorted(checks, key=lambda c: str(c.get("id"))),
        "findings": sorted(findings, key=lambda f: str(f.get("fingerprint"))),
        "verdict": report.get("verdict"),
    }


def compute_report_fingerprint(report: Mapping[str, Any]) -> str:
    return sha256_text(canonical_json(_fingerprint_payload(report)))


def compute_verdict(report: MutableMapping[str, Any], policy: Mapping[str, Any]) -> str:
    severity_cfg = policy.get("severity", {}) if isinstance(policy.get("severity"), Mapping) else {}
    blocking = set(severity_cfg.get("blocking", ["P0", "P1"]))
    findings = [f for f in report.get("findings", []) if isinstance(f, Mapping)]
    if any(f.get("severity") in blocking for f in findings):
        return "blocked"

    max_nonblocking = severity_cfg.get("max_nonblocking_findings_before_block", 25)
    try:
        max_nonblocking = int(max_nonblocking)
    except (TypeError, ValueError):
        max_nonblocking = 25
    if len(findings) > max_nonblocking:
        return "blocked"

    checks = [c for c in report.get("checks", []) if isinstance(c, Mapping)]
    strict_skips = bool(report.get("strict_skips", False))
    for check in checks:
        if not bool(check.get("required", True)):
            continue
        status = check.get("status")
        if status in {"fail", "error"}:
            return "blocked"
        if status == "planned":
            return "incomplete"
        if status == "skip" and strict_skips:
            return "blocked"
    if any(c.get("status") == "skip" and bool(c.get("required", True)) for c in checks):
        return "incomplete"
    return "pass"


def finalize_report(report: MutableMapping[str, Any], policy: Mapping[str, Any]) -> MutableMapping[str, Any]:
    report["schema"] = SCHEMA_VERSION
    report["bot"] = BOT_NAME
    report["bot_version"] = BOT_VERSION
    report.setdefault("generated_at", utc_now())
    report["findings"] = sorted(
        report.get("findings", []),
        key=lambda f: (SEVERITY_RANK.get(str(f.get("severity")), 99), str(f.get("id")), str(f.get("path"))),
    )
    report["checks"] = sorted(report.get("checks", []), key=lambda c: str(c.get("id")))
    report["verdict"] = compute_verdict(report, policy)
    report["complete"] = report["verdict"] != "incomplete"
    report["fingerprint"] = compute_report_fingerprint(report)
    return report


def validate_report(report: Any, *, require_fingerprint: bool = True) -> list[str]:
    errors: list[str] = []
    if not isinstance(report, dict):
        return ["report is not a JSON object"]
    if report.get("schema") != SCHEMA_VERSION:
        errors.append(f"schema must be {SCHEMA_VERSION}")
    for key in ("bot", "bot_version", "generated_at", "verdict", "checks", "findings"):
        if key not in report:
            errors.append(f"missing required field: {key}")
    if report.get("verdict") not in {"pass", "blocked", "incomplete"}:
        errors.append("verdict must be pass, blocked, or incomplete")
    seen_checks: set[str] = set()
    for check in report.get("checks", []):
        if not isinstance(check, Mapping):
            errors.append("check is not an object")
            continue
        cid = str(check.get("id", ""))
        if not cid:
            errors.append("check without id")
        elif cid in seen_checks:
            errors.append(f"duplicate check id: {cid}")
        seen_checks.add(cid)
        if check.get("status") not in CHECK_RANK:
            errors.append(f"check {cid}: invalid status")
    for finding in report.get("findings", []):
        if not isinstance(finding, Mapping):
            errors.append("finding is not an object")
            continue
        if finding.get("severity") not in SEVERITY_RANK:
            errors.append(f"finding {finding.get('id')}: invalid severity")
        if not finding.get("fingerprint"):
            errors.append(f"finding {finding.get('id')}: missing fingerprint")
    if require_fingerprint and report.get("fingerprint") != compute_report_fingerprint(report):
        errors.append("report fingerprint does not match report contents")
    return errors


def md_escape(value: Any) -> str:
    text = str(value if value is not None else "")
    text = text.replace("@", "@\u200b")
    # Reports are rendered by a trusted publisher from data produced around
    # candidate code. Neutralize table breaks, code-span escapes, and inline
    # link syntax in addition to HTML and @-mentions.
    text = (
        text.replace("`", "'")
        .replace("[", "\\[")
        .replace("]", "\\]")
        .replace("|", "\\|")
        .replace("\r", " ")
        .replace("\n", "<br>")
    )
    return html.escape(text, quote=False)


def _status_icon(status: str) -> str:
    return {
        "pass": "✅",
        "fail": "❌",
        "error": "💥",
        "warn": "⚠️",
        "skip": "⏭️",
        "planned": "🗓️",
    }.get(status, "❔")


def render_markdown(report: Mapping[str, Any], *, artifact_note: str | None = None) -> str:
    verdict = str(report.get("verdict", "incomplete"))
    verdict_line = {
        "pass": "✅ **PASS** — no blocking anti-regression finding in the executed campaign.",
        "blocked": "❌ **BLOCKED** — at least one strict anti-regression gate failed.",
        "incomplete": "⚠️ **INCOMPLETE** — required evidence was planned or skipped; this is not a pass.",
    }.get(verdict, verdict)
    findings = [f for f in report.get("findings", []) if isinstance(f, Mapping)]
    checks = [c for c in report.get("checks", []) if isinstance(c, Mapping)]
    metrics = report.get("metrics", {}) if isinstance(report.get("metrics"), Mapping) else {}
    safe_fingerprint = re.sub(r"[^0-9a-fA-F]", "", str(report.get("fingerprint", "")))[:64]
    safe_campaign = re.sub(r"[^A-Za-z0-9._/-]+", "-", str(report.get("campaign", "")))[:80].strip("-") or "unknown"
    lines = [
        REPORT_MARKER,
        f"<!-- rd-sentinel-fingerprint:{safe_fingerprint} -->",
        f"<!-- rd-sentinel-campaign:{safe_campaign} -->",
        "## 🛡️ RD Sentinel — anti-regression report",
        "",
        verdict_line,
        "",
        "| Field | Value |",
        "|---|---|",
        f"| Campaign | {md_escape(report.get('campaign'))} |",
        f"| Profile | {md_escape(report.get('profile'))} |",
        f"| Base | `{md_escape(str(report.get('base_sha') or '')[:12])}` |",
        f"| Head | `{md_escape(str(report.get('head_sha') or '')[:12])}` |",
        f"| Risk score | {md_escape(report.get('risk_score', 0))}/100 |",
        f"| Areas | {md_escape(', '.join(str(a) for a in report.get('areas', [])) or '—')} |",
        f"| Report fingerprint | `{md_escape(str(report.get('fingerprint') or '')[:16])}` |",
    ]
    if report.get("run_url"):
        lines.append(f"| Evidence run | {md_escape(report.get('run_url'))} |")
    lines.append("")

    if metrics:
        lines.extend(["### Key metrics", "", "| Metric | Value |", "|---|---|"])
        interesting = (
            "changed_files",
            "added_lines",
            "deleted_lines",
            "static_test_attributes",
            "mcp_tool_count",
            "golden_vector_count",
            "fuzz_target_count",
            "dependency_changes",
            "dependency_major_changes",
            "release_binary_bytes",
            "cold_snapshot_median_ms",
            "cold_first_interactive_median_ms",
            "perf_cold_p50_emulated_ms",
        )
        shown = 0
        for key in interesting:
            if key in metrics:
                lines.append(f"| `{md_escape(key)}` | {md_escape(metrics[key])} |")
                shown += 1
        if not shown:
            for key in sorted(metrics)[:12]:
                value = metrics[key]
                if isinstance(value, (str, int, float, bool)) or value is None:
                    lines.append(f"| `{md_escape(key)}` | {md_escape(value)} |")
        lines.append("")

    lines.extend(["### Findings", ""])
    if findings:
        lines.extend(["| Severity | Area | Finding | Evidence |", "|---|---|---|---|"])
        for finding in findings[:80]:
            location = str(finding.get("path") or "")
            if finding.get("line"):
                location += f":{finding.get('line')}"
            evidence = md_escape(str(finding.get("evidence") or "")[:500])
            if location:
                evidence = f"`{md_escape(location)}` — {evidence}"
            lines.append(
                "| "
                + " | ".join(
                    [
                        md_escape(finding.get("severity")),
                        md_escape(finding.get("area")),
                        md_escape(finding.get("title")),
                        evidence,
                    ]
                )
                + " |"
            )
        if len(findings) > 80:
            lines.append("")
            lines.append(f"_Showing 80 of {len(findings)} findings; the JSON artifact contains all of them._")
    else:
        lines.append("No findings.")
    lines.append("")

    lines.extend(["### Checks", "", "| Result | Check | Required | Summary |", "|---|---|---:|---|"])
    for check in checks[:160]:
        status = str(check.get("status"))
        lines.append(
            "| "
            + " | ".join(
                [
                    f"{_status_icon(status)} {md_escape(status)}",
                    md_escape(check.get("title") or check.get("id")),
                    "yes" if check.get("required", True) else "no",
                    md_escape(str(check.get("summary") or "")[:400]),
                ]
            )
            + " |"
        )
    if len(checks) > 160:
        lines.append("")
        lines.append(f"_Showing 160 of {len(checks)} checks; the JSON artifact contains all of them._")
    lines.append("")

    plan = report.get("plan", [])
    if isinstance(plan, Sequence) and plan:
        lines.extend(["### Campaign plan", ""])
        for item in plan:
            if isinstance(item, Mapping):
                lines.append(
                    f"- **{md_escape(item.get('component'))}** — {md_escape(item.get('reason'))}"
                    + (" (required)" if item.get("required", True) else " (advisory)")
                )
        lines.append("")

    lines.extend(
        [
            "### How to respond",
            "",
            "- Fix P0/P1 findings in the product change; do not weaken Sentinel policy in the same PR.",
            "- A skipped required campaign is incomplete evidence, never a pass.",
            "- Maintainers can request `/rd-sentinel deep`, `/rd-sentinel perf`, `/rd-sentinel fuzz 900`, or `/rd-sentinel rerun`.",
            "- Baseline changes need fresh before/after evidence and the `sentinel:baseline-approved` label.",
        ]
    )
    if artifact_note:
        lines.extend(["", md_escape(artifact_note)])
    rendered = "\n".join(lines) + "\n"
    if len(rendered) > 60000:
        rendered = rendered[:59000] + "\n\n_Report truncated for GitHub; see the JSON artifact._\n"
    return rendered
