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
BOT_VERSION = "1.2.0"
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

# High-confidence token shapes. Detection intentionally returns only the kind;
# callers must never copy the matched value into report evidence.
_SECRET_DETECTORS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("github-fine-grained-pat", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{22,}\b")),
    ("github-token", re.compile(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{30,}\b")),
    ("private-key", re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----")),
    ("aws-access-key", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("slack-token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b")),
    ("google-api-key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("stripe-live-key", re.compile(r"\b(?:sk|rk)_live_[0-9A-Za-z]{16,}\b")),
    ("npm-token", re.compile(r"\bnpm_[A-Za-z0-9]{36,}\b")),
    ("pypi-token", re.compile(r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}\b")),
)

_GENERIC_SECRET_ASSIGNMENT = re.compile(
    r"(?i)\b(?:api[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret|private[_-]?key|password)\b"
    r"\s*[:=]\s*[\"']?([A-Za-z0-9_\-./+=]{24,})[\"']?"
)
_SECRET_PLACEHOLDERS = (
    "example",
    "placeholder",
    "redacted",
    "changeme",
    "change-me",
    "your-",
    "your_",
    "xxxx",
    "test-token",
    "dummy",
    "fake",
)


def _looks_like_secret_value(value: str) -> bool:
    lowered = value.lower()
    if any(marker in lowered for marker in _SECRET_PLACEHOLDERS):
        return False
    if lowered.startswith(("${", "$", "env.", "secrets.", "os.environ", "process.env")):
        return False
    classes = sum(
        bool(re.search(pattern, value))
        for pattern in (r"[a-z]", r"[A-Z]", r"[0-9]", r"[_\-./+=]")
    )
    # Unique-character count is a cheap guard against repeated filler strings
    # while still catching opaque generated credentials in assignments.
    return classes >= 3 and len(set(value)) >= 10


def detect_secret_kind(text: str) -> str | None:
    """Return the kind of credential in *text*, never the credential itself."""

    for kind, pattern in _SECRET_DETECTORS:
        if pattern.search(text):
            return kind
    match = _GENERIC_SECRET_ASSIGNMENT.search(text)
    if match and _looks_like_secret_value(match.group(1)):
        return "generic-secret-assignment"
    return None


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
    for _kind, pattern in _SECRET_DETECTORS:
        out = pattern.sub("[REDACTED]", out)
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


def _policy_finding(fid: str, severity: str, title: str, evidence: str) -> Finding:
    return Finding(
        id=fid,
        severity=severity,
        area="policy",
        title=title,
        evidence=evidence[:800],
        remediation="Fix the trusted RD Sentinel configuration in a separately reviewed policy change.",
    )


def _is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _validate_string_list(
    findings: list[Finding], owner: Mapping[str, Any], key: str, where: str
) -> list[str]:
    value = owner.get(key, [])
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        findings.append(
            _policy_finding(
                "invalid-policy-shape",
                "P0",
                f"Policy field {where}.{key} must be a list of strings",
                f"value type={type(value).__name__}",
            )
        )
        return []
    return list(value)


def validate_policy(
    policy: Mapping[str, Any], *, today: _dt.date | None = None
) -> list[Finding]:
    """Validate the trusted policy shape before it is used to judge a diff.

    This is deliberately stricter than ``json.loads``: a typo that makes a
    campaign threshold a string, removes P0 from the blocking set, or drops a
    critical protected path must be a visible P0 instead of silently changing
    the gate's meaning.
    """

    findings: list[Finding] = []
    current = today or today_utc()
    if policy.get("schema") not in (None, 1):
        findings.append(
            _policy_finding(
                "unsupported-policy-schema",
                "P0",
                "RD Sentinel policy schema is unsupported",
                f"schema={policy.get('schema')!r}",
            )
        )

    severity = policy.get("severity", {})
    if not isinstance(severity, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy severity must be an object", repr(severity)))
        severity = {}
    blocking = severity.get("blocking", ["P0", "P1"])
    if not isinstance(blocking, list) or not blocking or any(item not in SEVERITY_RANK for item in blocking):
        findings.append(
            _policy_finding(
                "invalid-blocking-severities",
                "P0",
                "Policy blocking severities are invalid",
                f"blocking={blocking!r}",
            )
        )
    elif not {"P0", "P1"}.issubset(set(blocking)):
        findings.append(
            _policy_finding(
                "blocking-severities-weakened",
                "P0",
                "Policy no longer blocks both P0 and P1 findings",
                f"blocking={blocking!r}",
            )
        )
    max_nonblocking = severity.get("max_nonblocking_findings_before_block", 25)
    if not isinstance(max_nonblocking, int) or isinstance(max_nonblocking, bool) or not (0 <= max_nonblocking <= 1000):
        findings.append(
            _policy_finding(
                "invalid-nonblocking-limit",
                "P0",
                "Policy non-blocking finding limit is invalid",
                f"max_nonblocking_findings_before_block={max_nonblocking!r}",
            )
        )

    approvals = policy.get("approvals", {})
    if not isinstance(approvals, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy approvals must be an object", repr(approvals)))
        approvals = {}
    for key in ("policy_label", "baseline_label", "dependency_major_label"):
        if key in approvals and (not isinstance(approvals[key], str) or not approvals[key].strip()):
            findings.append(
                _policy_finding(
                    "invalid-approval-label",
                    "P0",
                    "Policy approval label is invalid",
                    f"approvals.{key}={approvals[key]!r}",
                )
            )

    protected = _validate_string_list(findings, policy, "protected_paths", "policy")
    critical_paths = {
        ".github/rd-sentinel/policy.json": "Sentinel policy",
        ".github/rd-sentinel/baseline.json": "Sentinel baseline",
        ".github/workflows/rd-sentinel.yml": "Sentinel PR workflow",
        "tools/rd-sentinel/sentinel/audit.py": "Sentinel audit code",
    }
    for path, label in critical_paths.items():
        if protected and not match_any(path, protected):
            findings.append(
                _policy_finding(
                    "critical-path-not-protected",
                    "P0",
                    f"{label} is not covered by protected_paths",
                    path,
                )
            )

    labels = policy.get("labels", {})
    if labels is not None and not isinstance(labels, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy labels must be an object", repr(labels)))
        labels = {}
    if isinstance(labels, Mapping):
        for key, cfg in labels.items():
            if not isinstance(cfg, Mapping) or not cfg.get("name"):
                findings.append(
                    _policy_finding(
                        "invalid-result-label",
                        "P1",
                        "Policy result label is incomplete",
                        f"labels.{key}={cfg!r}",
                    )
                )
                continue
            color = str(cfg.get("color", ""))
            if color and not re.fullmatch(r"[0-9a-fA-F]{6}", color):
                findings.append(
                    _policy_finding(
                        "invalid-result-label-color",
                        "P2",
                        "Policy result label color is not six-digit hex",
                        f"labels.{key}.color={color!r}",
                    )
                )

    diff_cfg = policy.get("diff", {})
    if not isinstance(diff_cfg, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy diff must be an object", repr(diff_cfg)))
        diff_cfg = {}
    for key in ("large_file_bytes", "very_large_diff_files", "very_large_diff_lines"):
        value = diff_cfg.get(key)
        if value is not None and (not _is_number(value) or value <= 0):
            findings.append(
                _policy_finding(
                    "invalid-diff-limit",
                    "P0",
                    "Policy diff limit is invalid",
                    f"diff.{key}={value!r}",
                )
            )
    for key in (
        "binary_extensions_blocking",
        "unsafe_allowed_path_globs",
        "test_path_globs",
        "adr_path_globs",
        "compat_evidence_globs",
        "evidence_harness_globs",
    ):
        _validate_string_list(findings, diff_cfg, key, "diff")

    campaigns = policy.get("campaigns", {})
    if not isinstance(campaigns, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy campaigns must be an object", repr(campaigns)))
        campaigns = {}
    positive_campaign_fields = {
        "cargo_jobs",
        "repeat_targeted_tests",
        "flaky_hunt_repetitions",
        "perf_runs",
        "cold_runs",
        "real_apk_cycles_pr",
        "real_apk_cycles_weekly",
        "comprix_cycles_pr",
        "comprix_cycles_weekly",
        "mcp_soak_seconds_weekly",
        "fuzz_workers",
        "mutation_minutes",
        "command_timeout_seconds",
        "short_command_timeout_seconds",
        "mutation_test_timeout_seconds",
        "osv_timeout_seconds",
    }
    nonnegative_campaign_fields = {
        "fuzz_seconds_pr_parser",
        "fuzz_seconds_nightly",
        "fuzz_seconds_weekly",
        "mutation_max_missed",
    }
    for key in positive_campaign_fields:
        value = campaigns.get(key)
        if value is not None and (not _is_number(value) or value <= 0):
            findings.append(
                _policy_finding("invalid-campaign-limit", "P0", "Policy campaign limit is invalid", f"campaigns.{key}={value!r}")
            )
    for key in nonnegative_campaign_fields:
        value = campaigns.get(key)
        if value is not None and (not _is_number(value) or value < 0):
            findings.append(
                _policy_finding("invalid-campaign-limit", "P0", "Policy campaign limit is invalid", f"campaigns.{key}={value!r}")
            )

    thresholds = policy.get("thresholds", {})
    if not isinstance(thresholds, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy thresholds must be an object", repr(thresholds)))
        thresholds = {}
    for key, value in thresholds.items():
        if not _is_number(value) or value <= 0:
            findings.append(
                _policy_finding("invalid-threshold", "P0", "Policy threshold is invalid", f"thresholds.{key}={value!r}")
            )
        elif key.endswith("_ratio") and value < 1:
            findings.append(
                _policy_finding(
                    "invalid-threshold-ratio",
                    "P0",
                    "Policy A/B ratio threshold is below 1.0",
                    f"thresholds.{key}={value!r}",
                )
            )
        elif key == "efficiency_paired_speedup_floor" and value > 1:
            findings.append(
                _policy_finding(
                    "invalid-threshold-ratio",
                    "P0",
                    "Policy efficiency speedup floor is above 1.0",
                    f"thresholds.{key}={value!r}",
                )
            )

    dependencies = policy.get("dependencies", {})
    if not isinstance(dependencies, Mapping):
        findings.append(_policy_finding("invalid-policy-shape", "P0", "Policy dependencies must be an object", repr(dependencies)))
        dependencies = {}
    _validate_string_list(findings, dependencies, "allowed_license_tokens", "dependencies")
    for pattern in _validate_string_list(findings, dependencies, "deep_package_name_patterns", "dependencies"):
        try:
            re.compile(pattern)
        except re.error as exc:
            findings.append(
                _policy_finding(
                    "invalid-deep-dependency-pattern",
                    "P0",
                    "Deep dependency policy pattern is invalid",
                    f"{pattern}: {exc}",
                )
            )
    mass_threshold = dependencies.get("mass_update_change_threshold")
    if mass_threshold is not None and (not _is_number(mass_threshold) or mass_threshold < 1):
        findings.append(
            _policy_finding(
                "invalid-dependency-limit",
                "P0",
                "Policy mass-update threshold is invalid",
                f"dependencies.mass_update_change_threshold={mass_threshold!r}",
            )
        )

    exceptions = policy.get("exceptions", [])
    if isinstance(exceptions, list):
        seen_ids: set[str] = set()
        broad_globs = {"", "*", "**", "./**", ".github/**", "crates/**", "tools/**"}
        for raw in exceptions:
            if not isinstance(raw, Mapping):
                continue  # apply_exceptions reports the malformed object itself.
            ex_id = str(raw.get("id", ""))
            if ex_id and ex_id in seen_ids:
                findings.append(
                    _policy_finding(
                        "duplicate-policy-exception",
                        "P0",
                        "Policy exception id is duplicated",
                        ex_id,
                    )
                )
            seen_ids.add(ex_id)
            path_glob = str(raw.get("path_glob", "")).strip()
            if path_glob in broad_globs:
                findings.append(
                    _policy_finding(
                        "overbroad-policy-exception",
                        "P0",
                        "Policy exception scope is overbroad",
                        f"exception {ex_id or '<unnamed>'} path_glob={path_glob!r}",
                    )
                )
            expires = parse_date(raw.get("expires"))
            if expires and expires > current + _dt.timedelta(days=732):
                findings.append(
                    _policy_finding(
                        "policy-exception-too-long",
                        "P1",
                        "Policy exception lasts more than two years",
                        f"exception {ex_id or '<unnamed>'} expires={expires.isoformat()}",
                    )
                )
    return findings


def validate_baseline(baseline: Mapping[str, Any]) -> list[Finding]:
    """Validate baseline contracts/corpus without changing their meaning."""

    findings: list[Finding] = []
    if baseline.get("schema") not in (None, 1):
        findings.append(
            _policy_finding(
                "unsupported-baseline-schema",
                "P0",
                "RD Sentinel baseline schema is unsupported",
                f"schema={baseline.get('schema')!r}",
            )
        )
    contracts = baseline.get("contracts", {})
    if not isinstance(contracts, Mapping):
        findings.append(_policy_finding("invalid-baseline-shape", "P0", "Baseline contracts must be an object", repr(contracts)))
        contracts = {}
    int_contracts = {
        "cargo_lock_version",
        "floor_min_sdk",
        "mcp_tool_count",
        "golden_vector_count",
        "fuzz_target_count",
        "minimum_static_test_attributes",
    }
    for key in int_contracts:
        value = contracts.get(key)
        if value is not None and (not isinstance(value, int) or isinstance(value, bool) or value < 0):
            findings.append(
                _policy_finding("invalid-baseline-contract", "P0", "Baseline integer contract is invalid", f"contracts.{key}={value!r}")
            )
    if contracts.get("rust_version") is not None and not isinstance(contracts["rust_version"], str):
        findings.append(
            _policy_finding(
                "invalid-baseline-contract",
                "P0",
                "Baseline Rust version contract is invalid",
                f"contracts.rust_version={contracts['rust_version']!r}",
            )
        )
    budget = contracts.get("startup_budget_emulated_ms_max")
    if budget is not None and (not _is_number(budget) or budget <= 0):
        findings.append(
            _policy_finding(
                "invalid-baseline-contract",
                "P0",
                "Baseline startup budget contract is invalid",
                f"contracts.startup_budget_emulated_ms_max={budget!r}",
            )
        )

    corpus = baseline.get("corpus", [])
    if not isinstance(corpus, list):
        findings.append(_policy_finding("invalid-baseline-shape", "P0", "Baseline corpus must be a list", repr(corpus)))
        return findings
    seen_corpus: set[str] = set()
    for entry in corpus:
        if not isinstance(entry, Mapping):
            findings.append(_policy_finding("invalid-baseline-corpus", "P0", "Baseline corpus entry is not an object", repr(entry)))
            continue
        entry_id = str(entry.get("id", ""))
        if not entry_id:
            findings.append(_policy_finding("invalid-baseline-corpus", "P0", "Baseline corpus entry has no id", repr(entry)[:300]))
        elif entry_id in seen_corpus:
            findings.append(_policy_finding("duplicate-baseline-corpus", "P0", "Baseline corpus id is duplicated", entry_id))
        seen_corpus.add(entry_id)
        if bool(entry.get("path")) == bool(entry.get("path_env")):
            findings.append(
                _policy_finding(
                    "invalid-baseline-corpus-path",
                    "P0",
                    "Baseline corpus entry must set exactly one of path/path_env",
                    entry_id or repr(entry)[:300],
                )
            )
        sha = entry.get("sha256", entry.get("hash"))
        if sha is not None and not (isinstance(sha, str) and re.fullmatch(r"[0-9a-fA-F]{64}", sha)):
            findings.append(
                _policy_finding(
                    "invalid-baseline-corpus-hash",
                    "P0",
                    "Baseline corpus SHA-256 is invalid",
                    f"corpus {entry_id}: {sha!r}",
                )
            )
    return findings


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
        "risk_factors": report.get("risk_factors", []),
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


def render_sarif(report: Mapping[str, Any]) -> dict[str, Any]:
    """Render findings as SARIF 2.1.0 for code-scanning and editor tooling.

    The JSON report remains authoritative. SARIF is a presentation/export
    format: it carries the same fingerprinted finding identities, but a SARIF
    consumer must still use the JSON report for campaign completeness.
    """

    findings = [f for f in report.get("findings", []) if isinstance(f, Mapping)]
    level_by_severity = {"P0": "error", "P1": "error", "P2": "warning", "P3": "note"}
    rules_by_id: dict[str, dict[str, Any]] = {}
    results: list[dict[str, Any]] = []
    for finding in findings:
        rule_id = str(finding.get("id") or "rd-sentinel-finding")
        severity = str(finding.get("severity") or "P2")
        if rule_id not in rules_by_id:
            rules_by_id[rule_id] = {
                "id": rule_id,
                "name": re.sub(r"[^A-Za-z0-9]+", "", rule_id.title()) or "RdSentinelFinding",
                "shortDescription": {"text": str(finding.get("title") or rule_id)[:300]},
                "fullDescription": {
                    "text": redact(
                        str(finding.get("remediation") or finding.get("title") or rule_id),
                        limit=1000,
                    )
                },
                "defaultConfiguration": {"level": level_by_severity.get(severity, "warning")},
                "properties": {
                    "rdSentinelSeverity": severity,
                    "rdSentinelArea": str(finding.get("area") or ""),
                    "precision": "medium",
                },
            }
        message = str(finding.get("title") or rule_id)
        evidence = redact(str(finding.get("evidence") or ""), limit=1200)
        if evidence:
            message += f": {evidence}"
        result: dict[str, Any] = {
            "ruleId": rule_id,
            "level": level_by_severity.get(severity, "warning"),
            "message": {"text": message[:4000]},
            "partialFingerprints": {
                "rdSentinelFingerprint": str(finding.get("fingerprint") or rule_id)
            },
            "properties": {
                "rdSentinelSeverity": severity,
                "rdSentinelArea": str(finding.get("area") or ""),
                "exceptedBy": finding.get("excepted_by"),
            },
        }
        path = finding.get("path")
        path_basename = path.replace("\\", "/").rsplit("/", 1)[-1] if isinstance(path, str) else ""
        if (
            isinstance(path, str)
            and path
            and not path.startswith(("/", "$"))
            and not path.endswith("/")
            and "." in path_basename
        ):
            line = finding.get("line")
            result["locations"] = [
                {
                    "physicalLocation": {
                        "artifactLocation": {"uri": path.replace("\\", "/")},
                        "region": {
                            "startLine": int(line) if isinstance(line, int) and line > 0 else 1,
                            "startColumn": 1,
                        },
                    }
                }
            ]
        results.append(result)

    repository = str(report.get("repository") or "")
    driver: dict[str, Any] = {
        "name": BOT_NAME,
        "version": str(report.get("bot_version") or BOT_VERSION),
        "semanticVersion": str(report.get("bot_version") or BOT_VERSION),
        "rules": [rules_by_id[key] for key in sorted(rules_by_id)],
    }
    if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        driver["informationUri"] = f"https://github.com/{repository}/blob/main/docs/rd-sentinel.md"
    return {
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {"driver": driver},
                "invocations": [
                    {
                        "executionSuccessful": report.get("verdict") == "pass",
                        "properties": {
                            "campaign": report.get("campaign"),
                            "profile": report.get("profile"),
                        },
                    }
                ],
                "results": results,
                "properties": {
                    "rdSentinelReportFingerprint": report.get("fingerprint"),
                    "verdict": report.get("verdict"),
                    "baseSha": report.get("base_sha"),
                    "headSha": report.get("head_sha"),
                    "riskScore": report.get("risk_score"),
                },
            }
        ],
    }


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

    severity_counts = {severity: 0 for severity in ("P0", "P1", "P2", "P3")}
    for finding in findings:
        severity = str(finding.get("severity") or "")
        if severity in severity_counts:
            severity_counts[severity] += 1
    check_counts: dict[str, int] = {}
    for check in checks:
        status = str(check.get("status") or "unknown")
        check_counts[status] = check_counts.get(status, 0) + 1
    lines.extend(
        [
            "### Summary",
            "",
            "| Signal | Count |",
            "|---|---:|",
            f"| Blocking findings (P0+P1) | {severity_counts['P0'] + severity_counts['P1']} |",
            f"| Advisory findings (P2+P3) | {severity_counts['P2'] + severity_counts['P3']} |",
            f"| Checks passed | {check_counts.get('pass', 0)} |",
            f"| Checks failed/error | {check_counts.get('fail', 0) + check_counts.get('error', 0)} |",
            f"| Checks planned/skipped | {check_counts.get('planned', 0) + check_counts.get('skip', 0)} |",
            "",
        ]
    )

    risk_factors = report.get("risk_factors", [])
    if isinstance(risk_factors, Sequence) and risk_factors:
        lines.extend(
            [
                "### Why this risk score?",
                "",
                "| Points | Factor | Evidence |",
                "|---:|---|---|",
            ]
        )
        shown_factors = 0
        for factor in risk_factors:
            if not isinstance(factor, Mapping):
                continue
            lines.append(
                "| "
                + " | ".join(
                    [
                        md_escape(factor.get("points", 0)),
                        md_escape(factor.get("label") or factor.get("id")),
                        md_escape(str(factor.get("evidence") or "")[:300]),
                    ]
                )
                + " |"
            )
            shown_factors += 1
            if shown_factors >= 10:
                break
        lines.append("")

    dependency_changes = report.get("dependency_changes", [])
    if isinstance(dependency_changes, Sequence) and dependency_changes:
        lines.extend(
            [
                "### Dependency movement",
                "",
                "| Crate | Change | Kind | Lockfile |",
                "|---|---|---|---|",
            ]
        )
        shown_changes = 0
        for change in dependency_changes:
            if not isinstance(change, Mapping):
                continue
            old = change.get("old") or "—"
            new = change.get("new") or "—"
            lines.append(
                "| "
                + " | ".join(
                    [
                        f"`{md_escape(change.get('name'))}`",
                        f"{md_escape(old)} → {md_escape(new)}",
                        md_escape(change.get("kind")),
                        f"`{md_escape(change.get('lockfile') or 'Cargo.lock')}`",
                    ]
                )
                + " |"
            )
            shown_changes += 1
            if shown_changes >= 30:
                break
        if len(dependency_changes) > shown_changes:
            lines.append("")
            lines.append(
                f"_Showing {shown_changes} of {len(dependency_changes)} dependency changes; the JSON artifact contains all of them._"
            )
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
            "affected_package_count",
            "resolved_packages",
            "duplicate_dependency_names",
            "osv_vulnerability_count",
            "mutation_missed",
            "mutation_caught",
            "sbom_packages",
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
