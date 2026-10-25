"""Executable RD Sentinel campaigns.

These campaigns intentionally reuse RustyDroid's existing harnesses. The
value added here is risk selection, repetition, base-versus-head comparison,
strict handling of missing evidence, and one common report format.
"""

from __future__ import annotations

import itertools
import json
import math
import os
import re
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable, Mapping, MutableMapping, Sequence
from urllib import error as urllib_error
from urllib import request as urllib_request

from .audit import compare_cargo_locks, license_expression_allowed
from .mcp_endurance import run_generic_apk_endurance
from .model import CheckResult, Finding, canonical_json, redact, sha256_file, sha256_text

PACKAGE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]*$")
FEATURE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]*$")
OSV_QUERYBATCH_URL = "https://api.osv.dev/v1/querybatch"


@dataclass(slots=True)
class CampaignOptions:
    execute: bool = False
    strict_skips: bool = False
    components: list[str] = field(default_factory=list)
    apk_paths: list[Path] = field(default_factory=list)
    fuzz_seconds: int | None = None
    fuzz_workers: int | None = None
    repeat: int | None = None
    perf_runs: int | None = None
    cold_runs: int | None = None
    mutation_minutes: int | None = None
    allow_network: bool = False
    output_dir: Path = Path("target/rd-sentinel")


@dataclass(slots=True)
class RawResult:
    argv: list[str]
    cwd: Path
    exit_code: int | None
    stdout: str
    stderr: str
    duration_ms: float
    timed_out: bool = False
    missing: bool = False


def _slug(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", value).strip("-").lower()
    return slug or "check"


def _percentile(values: Sequence[float], p: float) -> float:
    if not values:
        return math.nan
    ordered = sorted(values)
    rank = max(0, min(len(ordered) - 1, math.ceil(p / 100.0 * len(ordered)) - 1))
    return float(ordered[rank])


def _median(values: Sequence[float]) -> float:
    return float(statistics.median(values)) if values else math.nan


def _mad(values: Sequence[float]) -> float:
    """Median absolute deviation, a robust spread estimate for small samples."""

    if not values:
        return math.nan
    center = statistics.median(values)
    return float(statistics.median([abs(v - center) for v in values]))


def _distribution(prefix: str, values: Sequence[float]) -> dict[str, float]:
    if not values:
        return {f"{prefix}_samples": 0}
    return {
        f"{prefix}_samples": len(values),
        f"{prefix}_median_ms": round(_median(values), 3),
        f"{prefix}_mean_ms": round(statistics.fmean(values), 3),
        f"{prefix}_mad_ms": round(_mad(values), 3),
        f"{prefix}_p95_ms": round(_percentile(values, 95), 3),
        f"{prefix}_min_ms": round(min(values), 3),
        f"{prefix}_max_ms": round(max(values), 3),
    }


def _noise_floor_ms(base_values: Sequence[float], head_values: Sequence[float]) -> float:
    """Three robust-sigma equivalents from the noisier side of an A/B pair."""

    if not base_values or not head_values:
        return 0.0
    return 3.0 * 1.4826 * max(_mad(base_values), _mad(head_values))


def parse_cold26_output(text: str) -> dict[str, Any]:
    """Parse the real ``scripts/cold26.py`` JSON-lines contract.

    The harness prints its one capture row as ``capture: {...}`` and each
    timed row as a bare JSON object. Timed rows do not carry a top-level
    ``snapshot`` field; the snapshot disclosure, when present, is inside the
    launch text. Sentinel always invokes the harness with ``--snapshot on``
    and ``--capture``, so successful non-capture rows are the timed snapshot
    attempts. A snapshot miss still appears there and is caught by the tight
    absolute/A-B timing gates rather than being silently discarded.
    """

    rows: list[dict[str, Any]] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if line.startswith("capture:"):
            line = line[len("capture:") :].strip()
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            rows.append(value)

    def is_snapshot_hit(row: Mapping[str, Any]) -> bool:
        if row.get("capture") is True or "error" in row:
            return False
        if isinstance(row.get("snapshot"), bool):
            return bool(row["snapshot"])
        launch = str(row.get("launch", ""))
        compact_launch = re.sub(r"\s+", "", launch).lower()
        if '"snapshot":true' in compact_launch or "snapshot=true" in compact_launch:
            return True
        if '"snapshot":false' in compact_launch or "snapshot=false" in compact_launch:
            return False
        return isinstance(row.get("t_frame_ms"), (int, float))

    hits = [r for r in rows if is_snapshot_hit(r)]
    captures = [r for r in rows if r.get("capture") is True or r.get("snapshot") is False]
    frame_times = [float(r["t_frame_ms"]) for r in hits if isinstance(r.get("t_frame_ms"), (int, float))]
    interactive_times = [
        float(r["t_interactive_ms"]) for r in hits if isinstance(r.get("t_interactive_ms"), (int, float))
    ]
    frame_hashes = sorted({str(r.get("frame_md5")) for r in hits if r.get("frame_md5")})
    return {
        "rows": rows,
        "hits": hits,
        "captures": captures,
        "frame_times_ms": frame_times,
        "median_ms": statistics.median(frame_times) if frame_times else None,
        "p95_ms": _percentile(frame_times, 95) if frame_times else None,
        "max_ms": max(frame_times) if frame_times else None,
        "interactive_times_ms": interactive_times,
        "interactive_median_ms": statistics.median(interactive_times) if interactive_times else None,
        "interactive_p95_ms": _percentile(interactive_times, 95) if interactive_times else None,
        "interactive_max_ms": max(interactive_times) if interactive_times else None,
        "sample_count": len(frame_times),
        "mad_ms": _mad(frame_times) if frame_times else None,
        "interactive_sample_count": len(interactive_times),
        "interactive_mad_ms": _mad(interactive_times) if interactive_times else None,
        "tap_successes": sum(1 for r in hits if r.get("tap_ok") is True),
        "tap_failures": sum(1 for r in hits if r.get("tap_ok") is False),
        "frame_hashes": frame_hashes,
    }


def extract_json(text: str) -> Any | None:
    text = text.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    starts = [idx for idx, char in enumerate(text) if char in "[{"]
    for start in starts[:20]:
        try:
            decoder = json.JSONDecoder()
            value, _ = decoder.raw_decode(text[start:])
            return value
        except json.JSONDecodeError:
            continue
    return None


def _version_tuple(value: str | None) -> tuple[int, ...] | None:
    if not value:
        return None
    parts: list[int] = []
    for part in str(value).split("-", 1)[0].split("."):
        if not part.isdigit():
            return None
        parts.append(int(part))
    while len(parts) < 3:
        parts.append(0)
    return tuple(parts)


_TEST_OUTCOME_RE = re.compile(
    r"^test\s+([A-Za-z0-9_:.\/-]+)\s+\.\.\.\s+(ok|FAILED|ignored)\s*$", re.MULTILINE
)


def parse_test_outcomes(text: str) -> dict[str, set[str]]:
    """Extract libtest outcomes from one or more cargo-test output blocks."""

    outcomes: dict[str, set[str]] = {}
    for name, raw_status in _TEST_OUTCOME_RE.findall(text):
        status = {"ok": "pass", "FAILED": "fail", "ignored": "ignored"}[raw_status]
        outcomes.setdefault(name, set()).add(status)
    return outcomes


def summarize_repeated_test_outcomes(
    outputs: Sequence[str],
) -> dict[str, list[str]]:
    merged: dict[str, set[str]] = {}
    for output in outputs:
        for name, statuses in parse_test_outcomes(output).items():
            merged.setdefault(name, set()).update(statuses)
    flaky = sorted(
        name for name, statuses in merged.items() if {"pass", "fail"} <= statuses
    )
    consistent_failures = sorted(
        name for name, statuses in merged.items() if statuses == {"fail"}
    )
    return {
        "tests_seen": sorted(merged),
        "flaky_tests": flaky,
        "consistent_failures": consistent_failures,
    }


_MUTANT_SUMMARY_RE = re.compile(
    r"(?P<count>\d+)\s+(?:mutants?\s+)?(?P<outcome>caught|missed|unviable|timed?\s*out|timeouts?)",
    re.IGNORECASE,
)


def parse_mutants_output(text: str) -> dict[str, Any]:
    """Parse cargo-mutants human output.

    Current cargo-mutants writes the final line as, for example,
    ``5 mutants tested in 1s: 3 missed, 2 caught``. Older/other summaries use
    ``12 mutants caught``. Accept both, while preferring the structured
    ``outcomes.json`` parser below whenever that file is available.
    """

    counts = {"caught": 0, "missed": 0, "unviable": 0, "timeout": 0}
    for count, raw_outcome in _MUTANT_SUMMARY_RE.findall(text):
        lowered = raw_outcome.lower()
        outcome = "timeout" if lowered.startswith("time") else lowered
        counts[outcome] += int(count)
    missed_lines = [
        line.strip()
        for line in text.splitlines()
        if "MISSED" in line or line.strip().lower().endswith("... missed")
    ]
    if not any(counts.values()) and missed_lines:
        counts["missed"] = len(missed_lines)
    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    return {
        **counts,
        "viable": viable,
        "kill_rate": (counts["caught"] / viable) if viable else None,
        "missed_mutants": missed_lines[:100],
        "outcome_source": "text",
    }


def parse_mutants_outcomes(data: Any) -> dict[str, Any] | None:
    """Normalize cargo-mutants ``mutants.out/outcomes.json``.

    The top-level counters are the stable part used here. Missed mutant names
    are recovered from the current nested scenario representation when present,
    but a future rename there must not discard the authoritative counters.
    """

    if not isinstance(data, Mapping):
        return None
    counts: dict[str, int] = {}
    for source_key, target_key in (
        ("caught", "caught"),
        ("missed", "missed"),
        ("unviable", "unviable"),
        ("timeout", "timeout"),
    ):
        value = data.get(source_key, 0)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            return None
        counts[target_key] = value

    missed_mutants: list[str] = []
    outcomes = data.get("outcomes", [])
    if isinstance(outcomes, list):
        for outcome in outcomes:
            if not isinstance(outcome, Mapping) or outcome.get("summary") != "MissedMutant":
                continue
            scenario = outcome.get("scenario")
            if isinstance(scenario, Mapping):
                mutant = scenario.get("Mutant")
                if isinstance(mutant, Mapping) and mutant.get("name"):
                    missed_mutants.append(str(mutant["name"]))

    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    total = data.get("total_mutants")
    return {
        **counts,
        "viable": viable,
        "total": total if isinstance(total, int) and total >= 0 else viable + counts["unviable"],
        "kill_rate": (counts["caught"] / viable) if viable else None,
        "missed_mutants": missed_mutants[:100],
        "outcome_source": "outcomes.json",
        "cargo_mutants_version": data.get("cargo_mutants_version"),
    }


def mutation_check_status(
    *,
    exit_code: int,
    missed: int,
    timeouts: int,
    blocking_context: bool,
    max_missed: int = 0,
    missing: bool = False,
    timed_out: bool = False,
) -> str:
    """Map cargo-mutants' process result to Sentinel's verdict semantics.

    cargo-mutants uses a non-zero exit for ordinary missed/timeout outcomes.
    Those outcomes are blocking only in an explicit mutants/deep context (and,
    for survivors, only above the policy limit); in a trend campaign they are
    visible warnings with P2 findings rather than accidental hard failures.
    Baseline/usage failures have no usable mutant outcome and stay failures.
    """

    if missing or timed_out:
        return "error"
    if exit_code == 0:
        return "pass"
    over_limit = missed > max(0, int(max_missed))
    if blocking_context and (over_limit or timeouts > 0):
        return "fail"
    if missed > 0 or timeouts > 0:
        return "warn"
    return "fail"


class CampaignRunner:
    def __init__(
        self,
        *,
        repo: Path,
        report: MutableMapping[str, Any],
        policy: Mapping[str, Any],
        baseline: Mapping[str, Any],
        options: CampaignOptions,
    ) -> None:
        self.repo = repo.resolve()
        self.report = report
        self.policy = policy
        self.baseline = baseline
        self.options = options
        self.output_dir = options.output_dir
        if not self.output_dir.is_absolute():
            self.output_dir = self.repo / self.output_dir
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self._release_binary: Path | None = None
        self._debug_binary: Path | None = None
        self._native_fixtures_ensured = False
        # issue #283: the absolute `rd perf` budget verdict is resolved at
        # the end of run(), once the base reference (compare) is known.
        self._perf_base_budget_ok: dict[str, bool] = {}
        self._perf_base_budget_metrics: dict[str, dict[str, Any]] = {}
        self._perf_candidate_budget_failures: list[tuple[str, dict[str, Any]]] = []
        self._used_check_ids = {str(c.get("id")) for c in report.get("checks", []) if isinstance(c, Mapping)}

    # ------------------------------------------------------------------ util
    def _unique_id(self, wanted: str) -> str:
        base = _slug(wanted)
        candidate = base
        idx = 2
        while candidate in self._used_check_ids:
            candidate = f"{base}-{idx}"
            idx += 1
        self._used_check_ids.add(candidate)
        return candidate

    def _add_check(self, check: CheckResult) -> None:
        check.id = self._unique_id(check.id)
        check.stdout_tail = redact(check.stdout_tail, repo=self.repo)
        check.stderr_tail = redact(check.stderr_tail, repo=self.repo)
        self.report.setdefault("checks", []).append(check.to_dict())

    def _add_finding(self, finding: Finding) -> None:
        self.report.setdefault("findings", []).append(finding.to_dict())

    def _metric(self, key: str, value: Any) -> None:
        self.report.setdefault("metrics", {})[key] = value

    def _cfg(self, key: str, default: Any) -> Any:
        campaigns = self.policy.get("campaigns", {})
        return campaigns.get(key, default) if isinstance(campaigns, Mapping) else default

    def _threshold(self, key: str, default: float) -> float:
        thresholds = self.policy.get("thresholds", {})
        value = thresholds.get(key, default) if isinstance(thresholds, Mapping) else default
        try:
            return float(value)
        except (TypeError, ValueError):
            return default

    def _env(self, extra: Mapping[str, str] | None = None) -> dict[str, str]:
        env = dict(os.environ)
        env["CARGO_TERM_COLOR"] = "never"
        env.setdefault("CARGO_BUILD_JOBS", str(self._cfg("cargo_jobs", 2)))
        env.setdefault("RUST_BACKTRACE", "1")
        if extra:
            env.update({str(k): str(v) for k, v in extra.items()})
        return env

    def _run_raw(
        self,
        argv: Sequence[str],
        *,
        cwd: Path | None = None,
        timeout: int | None = None,
        env_extra: Mapping[str, str] | None = None,
    ) -> RawResult:
        cwd = (cwd or self.repo).resolve()
        timeout = timeout or int(self._cfg("short_command_timeout_seconds", 1800))
        start = time.monotonic()
        argv_list = [str(x) for x in argv]
        try:
            proc = subprocess.Popen(
                argv_list,
                cwd=str(cwd),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
                errors="replace",
                env=self._env(env_extra),
                start_new_session=True,
            )
        except OSError as exc:
            return RawResult(argv_list, cwd, None, "", str(exc), 0.0, missing=True)
        try:
            stdout, stderr = proc.communicate(timeout=timeout)
            return RawResult(
                argv_list,
                cwd,
                proc.returncode,
                stdout or "",
                stderr or "",
                (time.monotonic() - start) * 1000.0,
            )
        except subprocess.TimeoutExpired as exc:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except (ProcessLookupError, PermissionError):
                pass
            try:
                stdout, stderr = proc.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError):
                    pass
                stdout, stderr = proc.communicate()
            return RawResult(
                argv_list,
                cwd,
                None,
                str(exc.stdout or stdout or ""),
                str(exc.stderr or stderr or "") + f"\nRD Sentinel timeout after {timeout}s",
                (time.monotonic() - start) * 1000.0,
                timed_out=True,
            )

    def _record_raw(
        self,
        raw: RawResult,
        *,
        check_id: str,
        title: str,
        component: str,
        required: bool = True,
        status: str | None = None,
        summary: str | None = None,
        metrics: Mapping[str, Any] | None = None,
        allow_missing: bool = False,
    ) -> RawResult:
        if status is None:
            if raw.missing:
                status = "skip" if allow_missing or not required else "error"
            elif raw.timed_out:
                status = "error"
            else:
                status = "pass" if raw.exit_code == 0 else "fail"
        if summary is None:
            if raw.missing:
                summary = f"Executable not found: {raw.argv[0] if raw.argv else ''}"
            elif raw.timed_out:
                summary = "Command timed out"
            else:
                summary = f"exit {raw.exit_code}"
        self._add_check(
            CheckResult(
                id=check_id,
                title=title,
                status=status,
                required=required,
                component=component,
                command=raw.argv,
                exit_code=raw.exit_code,
                duration_ms=round(raw.duration_ms, 3),
                summary=summary,
                metrics=dict(metrics or {}),
                stdout_tail=raw.stdout,
                stderr_tail=raw.stderr,
            )
        )
        return raw

    def _run_check(
        self,
        argv: Sequence[str],
        *,
        check_id: str,
        title: str,
        component: str,
        cwd: Path | None = None,
        timeout: int | None = None,
        required: bool = True,
        env_extra: Mapping[str, str] | None = None,
        allow_missing: bool = False,
    ) -> RawResult:
        raw = self._run_raw(argv, cwd=cwd, timeout=timeout, env_extra=env_extra)
        return self._record_raw(
            raw,
            check_id=check_id,
            title=title,
            component=component,
            required=required,
            allow_missing=allow_missing,
        )

    def _skip(self, check_id: str, title: str, component: str, summary: str, *, required: bool = True) -> None:
        self._add_check(
            CheckResult(
                id=check_id,
                title=title,
                status="skip",
                required=required,
                component=component,
                summary=summary,
            )
        )

    def _profile(self) -> str:
        return str(self.report.get("profile", "auto"))

    def _areas(self) -> set[str]:
        return {str(x) for x in self.report.get("areas", [])}

    def _targeted_packages(self) -> list[str]:
        metrics = self.report.get("metrics", {})
        packages = metrics.get("targeted_packages", []) if isinstance(metrics, Mapping) else []
        return [str(p) for p in packages] if isinstance(packages, list) else []

    def _changed_paths(self) -> list[str]:
        out: list[str] = []
        for item in self.report.get("changed_files", []):
            if isinstance(item, Mapping) and item.get("path"):
                out.append(str(item["path"]))
        return out

    def _apk_candidates(self, *, launchable_only: bool = True) -> list[Path]:
        paths: list[Path] = []
        for raw in self.options.apk_paths:
            target = raw if raw.is_absolute() else self.repo / raw
            if target.is_file() and target not in paths:
                paths.append(target)
        if paths:
            return paths
        corpus = self.baseline.get("corpus", []) if isinstance(self.baseline.get("corpus"), list) else []
        for entry in corpus:
            if not isinstance(entry, Mapping):
                continue
            if launchable_only and entry.get("expected") != "launchable":
                continue
            if entry.get("path_env"):
                env_value = os.environ.get(str(entry["path_env"]))
                if env_value:
                    candidate = Path(env_value)
                    if candidate.is_file():
                        paths.append(candidate)
                continue
            if entry.get("path"):
                candidate = self.repo / str(entry["path"])
                if candidate.is_file():
                    paths.append(candidate)
        return paths

    def _record_missing_env_corpus(self, component: str) -> None:
        """Make an absent external corpus visible instead of silently passing."""

        corpus = self.baseline.get("corpus", []) if isinstance(self.baseline.get("corpus"), list) else []
        for entry in corpus:
            if not isinstance(entry, Mapping) or not entry.get("path_env"):
                continue
            env_name = str(entry["path_env"])
            value = os.environ.get(env_name, "")
            present = bool(value and Path(value).is_file())
            entry_id = str(entry.get("id") or env_name.lower())
            self._metric(f"corpus_{entry_id}_present", present)
            if present:
                continue
            explicit = os.environ.get("RD_SENTINEL_REQUIRE_HEAVY_APK") == "1"
            github_hosted_optional = bool(entry.get("optional_on_github_hosted")) and os.environ.get(
                "GITHUB_ACTIONS"
            ) == "true"
            required = explicit or (self._profile() == "weekly" and not github_hosted_optional)
            self._skip(
                f"{component}-missing-corpus-{entry_id}",
                f"External corpus evidence: {entry_id}",
                component,
                f"{env_name} is not set to an existing APK; Sentinel cannot claim this corpus passed.",
                required=required,
            )

    # --------------------------------------------------------------- dispatch
    def run(self, components: Sequence[str]) -> None:
        self.report["strict_skips"] = self.options.strict_skips
        self.report["executed_components"] = []
        if not self.options.execute:
            for component in components:
                if component == "static":
                    continue
                self._add_check(
                    CheckResult(
                        id=f"planned-{component}",
                        title=f"Planned campaign: {component}",
                        status="planned",
                        required=True,
                        component=component,
                        summary="Execution was not requested; planned evidence is incomplete, not passing.",
                    )
                )
            return

        handlers: dict[str, Callable[[], None]] = {
            "core": self._component_core,
            "features": self._component_features,
            "determinism": self._component_determinism,
            "mutation": self._component_mutation,
            "deps": self._component_deps,
            "future": self._component_future,
            "perf": self._component_perf,
            "real-apk": self._component_real_apk,
            "compare": self._component_compare,
            "fuzz": self._component_fuzz,
            "soak": self._component_soak,
            "golden": self._component_golden,
            "dep-trial": self._component_dep_trial,
        }
        for component in components:
            if component == "static":
                continue
            handler = handlers.get(component)
            if handler is None:
                self._add_finding(
                    Finding(
                        id="unknown-sentinel-component",
                        severity="P1",
                        area="sentinel",
                        title=f"Unknown Sentinel component: {component}",
                        evidence=component,
                        remediation="Fix the workflow/profile component name; unknown evidence cannot pass.",
                    )
                )
                self._add_check(
                    CheckResult(
                        id=f"component-{component}-unknown",
                        title=f"Campaign component failed to run: {component}",
                        status="error",
                        required=True,
                        component=component,
                        summary="unknown component",
                    )
                )
                continue
            self.report["executed_components"].append(component)
            try:
                handler()
            except Exception as exc:  # The report must survive campaign defects.
                self._add_check(
                    CheckResult(
                        id=f"component-{component}-error",
                        title=f"Campaign component failed to run: {component}",
                        status="error",
                        required=True,
                        component=component,
                        summary=redact(str(exc), repo=self.repo, limit=1000),
                    )
                )
        self._resolve_perf_budgets()

    # ------------------------------------------------------------------- core
    def _test_commands_for_packages(self, packages: Sequence[str]) -> list[tuple[str, list[str]]]:
        commands: list[tuple[str, list[str]]] = []
        paths = self._changed_paths()
        jit_relevant = any("jit" in p.lower() or p.startswith("crates/rd-jni/") for p in paths)
        native_relevant = any("native" in p.lower() or p.startswith(("crates/rd-ndk/", "crates/rd-jni/")) for p in paths)
        profile = self._profile()
        if profile in {"deep", "nightly", "weekly"} or (not packages and "dependencies-build" in self._areas()):
            commands.append(
                (
                    "cargo test --workspace --all-features",
                    ["cargo", "test", "--workspace", "--all-features", "--locked"],
                )
            )
            return commands
        for package in packages:
            commands.append((f"cargo test -p {package}", ["cargo", "test", "-p", package, "--locked"]))
            if package in {"rd-vm", "rd-agent", "rd-cli", "rd-jni"} and jit_relevant:
                commands.append(
                    (
                        f"cargo test -p {package} --features jit",
                        ["cargo", "test", "-p", package, "--features", "jit", "--locked"],
                    )
                )
            if package in {"rd-vm", "rd-agent", "rd-cli", "rd-ndk"} and native_relevant:
                commands.append(
                    (
                        f"cargo test -p {package} --features native",
                        ["cargo", "test", "-p", package, "--features", "native", "--locked"],
                    )
                )
        return commands

    @staticmethod
    def _commands_may_run_native(commands: Sequence[tuple[str, list[str]]]) -> bool:
        for _title, argv in commands:
            if "--all-features" in argv:
                return True
            for index, arg in enumerate(argv):
                features: str | None = None
                if arg == "--features" and index + 1 < len(argv):
                    features = argv[index + 1]
                elif arg.startswith("--features="):
                    features = arg.split("=", 1)[1]
                if features and "native" in features.split(","):
                    return True
        return False

    def _ensure_native_test_fixtures(
        self, component: str, commands: Sequence[tuple[str, list[str]]]
    ) -> None:
        """Build the golden/native .so fixtures before native tests run.

        Issue #193: deep/nightly profiles run
        ``cargo test --workspace --all-features``, which enables the
        ``rd-ndk`` native tests. Those tests read the three-ABI fixtures
        under ``golden/native/artifacts/`` (gitignored, built from source by
        ``golden/native/build.sh`` — ordinary CI builds them, Sentinel
        never did), so every Sentinel run failed 3 tests on missing files.
        Build them once, as a recorded required check: if the cross
        compilers are missing the check fails visibly (fail closed) instead
        of silently skipping native evidence. Workflows are responsible
        for installing gcc-aarch64-linux-gnu/gcc-arm-linux-gnueabihf and
        unicorn; this helper is the fail-closed backstop for any other
        way the campaign gets launched.
        """

        if self._native_fixtures_ensured or not self._commands_may_run_native(commands):
            return
        self._native_fixtures_ensured = True
        artifacts = self.repo / "golden/native/artifacts"
        if all(
            (artifacts / name).is_file()
            for name in ("x86_64.so", "arm64-v8a.so", "armeabi-v7a.so")
        ):
            return
        self._run_check(
            ["bash", "golden/native/build.sh"],
            check_id=f"{component}-native-fixtures",
            title="Build golden/native three-ABI fixtures for rd-ndk native tests",
            component=component,
            timeout=1800,
        )

    def _component_core(self) -> None:
        paths = self._changed_paths()
        if any(p.startswith("tools/rd-sentinel/") for p in paths) or self._profile() in {"nightly", "weekly", "deep"}:
            self._run_check(
                [sys.executable, "-m", "unittest", "discover", "-s", "tools/rd-sentinel/tests", "-v"],
                check_id="core-sentinel-unit-tests",
                title="RD Sentinel unit tests",
                component="core",
            )
        if any(p.endswith(".md") or p.startswith("docs/") for p in paths):
            self._run_check(
                [sys.executable, "scripts/check_md_links.py"],
                check_id="core-doc-links",
                title="Internal documentation links",
                component="core",
            )

        raw = self._run_raw(["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"])
        metadata = extract_json(raw.stdout)
        metrics: dict[str, Any] = {}
        if isinstance(metadata, Mapping):
            packages = metadata.get("packages", [])
            metrics["workspace_packages"] = len(packages) if isinstance(packages, list) else None
            self._metric("workspace_packages", metrics["workspace_packages"])
        self._record_raw(
            raw,
            check_id="core-cargo-metadata",
            title="Cargo workspace metadata resolves under --locked",
            component="core",
            metrics=metrics,
        )

        commands = self._test_commands_for_packages(self._targeted_packages())
        if not commands:
            self._skip(
                "core-targeted-tests",
                "Targeted Rust tests",
                "core",
                "No Rust package was selected by the diff; ordinary CI remains authoritative for the full PR suite.",
                required=False,
            )
            return
        self._ensure_native_test_fixtures("core", commands)
        for title, argv in commands:
            self._run_check(
                argv,
                check_id=f"core-{title}",
                title=title,
                component="core",
                timeout=max(3600, int(self._cfg("short_command_timeout_seconds", 1800))),
            )

    # --------------------------------------------------------------- features
    @staticmethod
    def _feature_sets_from_names(
        feature_names: Sequence[str], full: bool
    ) -> list[tuple[str, ...]]:
        names = sorted(
            {
                str(name)
                for name in feature_names
                if FEATURE_NAME_RE.fullmatch(str(name)) and str(name) != "default"
            }
        )
        sets: list[tuple[str, ...]] = []

        def add(values: Iterable[str]) -> None:
            item = tuple(dict.fromkeys(str(v) for v in values if str(v) in names))
            if item not in sets:
                sets.append(item)

        add(())
        if not names:
            return sets
        for name in names:
            add((name,))
        if full and len(names) <= 5:
            for size in range(2, len(names) + 1):
                for combo in itertools.combinations(names, size):
                    add(combo)
        else:
            for combo in itertools.combinations(names, 2):
                add(combo)
            add(tuple(names))
        # Deep profiles keep the matrix bounded even if a crate gains many
        # fan-in feature names; singleton/pair coverage catches most cfgs.
        return sets[:96]

    def _manifest_feature_names(self, package: str) -> list[str] | None:
        manifest_path = self.repo / "crates" / package / "Cargo.toml"
        if not manifest_path.is_file():
            return None
        try:
            import tomllib

            parsed = tomllib.loads(manifest_path.read_text(encoding="utf-8", errors="ignore"))
        except Exception:
            return None
        features = parsed.get("features", {}) if isinstance(parsed, Mapping) else {}
        if not isinstance(features, Mapping):
            return []
        return sorted(str(name) for name in features)

    def _feature_sets(self, package: str, full: bool) -> list[tuple[str, ...]]:
        dynamic = self._manifest_feature_names(package)
        if dynamic is not None:
            return self._feature_sets_from_names(dynamic, full)

        # Fallback for an unreadable manifest: preserve the v1.0 known matrix
        # rather than silently shrinking feature coverage.
        sets: list[tuple[str, ...]] = []

        def add(values: Iterable[str]) -> None:
            item = tuple(dict.fromkeys(values))
            if item not in sets:
                sets.append(item)

        add(())
        if package == "rd-vm":
            flags = ["advanced-components", "native", "jit"]
            combos = itertools.chain.from_iterable(itertools.combinations(flags, r) for r in range(len(flags) + 1)) if full else [(), ("advanced-components",), ("native",), ("jit",), tuple(flags)]
            for combo in combos:
                add(combo)
            return sets
        if package in {"rd-agent", "rd-cli"}:
            base_flags = ["advanced-components", "native", "jit"]
            combos = itertools.chain.from_iterable(itertools.combinations(base_flags, r) for r in range(len(base_flags) + 1)) if full else [(), ("advanced-components",), ("native",), ("jit",), tuple(base_flags)]
            for combo in combos:
                add(combo)
            add(("native-engines",))
            add(("advanced-components", "native-engines", "jit"))
            langs = [f"lang-{n:02d}" for n in range(1, 13)]
            if full:
                for lang in langs:
                    add((lang,))
                add(tuple(langs))
            else:
                add(("lang-01",))
                add(("lang-12",))
            return sets
        if package == "rd-engine-registry":
            langs = [f"lang-{n:02d}" for n in range(1, 13)]
            if full:
                for lang in langs:
                    add((lang,))
                add(tuple(langs))
            else:
                add(("lang-01",))
                add(("lang-12",))
            add(("native-execution",))
            return sets
        if package == "rd-jni":
            add(("jit",))
            return sets
        if package == "rd-ndk":
            add(("native",))
            return sets
        return sets

    def _component_features(self) -> None:
        profile = self._profile()
        full = profile in {"deep", "nightly", "weekly"}
        if full:
            packages = ["rd-vm", "rd-agent", "rd-cli", "rd-engine-registry", "rd-jni", "rd-ndk"]
        else:
            selected = self._targeted_packages()
            packages = [p for p in selected if p in {"rd-vm", "rd-agent", "rd-cli", "rd-engine-registry", "rd-jni", "rd-ndk"}]
            if "dependencies-build" in self._areas() and not packages:
                packages = ["rd-vm", "rd-agent", "rd-cli"]
        if not packages:
            self._skip("features-matrix", "Feature combination matrix", "features", "Diff did not select a feature-bearing package.", required=False)
            return
        for package in packages:
            for feature_set in self._feature_sets(package, full):
                argv = ["cargo", "check", "-p", package, "--no-default-features", "--locked"]
                label = ",".join(feature_set) if feature_set else "none"
                if feature_set:
                    argv.extend(["--features", ",".join(feature_set)])
                self._run_check(
                    argv,
                    check_id=f"features-{package}-{label}",
                    title=f"cargo check {package} [{label}]",
                    component="features",
                    timeout=1800,
                )
            self._run_check(
                ["cargo", "check", "-p", package, "--all-features", "--locked"],
                check_id=f"features-{package}-all",
                title=f"cargo check {package} [all features]",
                component="features",
                timeout=1800,
            )

    # ------------------------------------------------------------ determinism
    def _repeat_command(
        self,
        argv: Sequence[str],
        *,
        check_id: str,
        title: str,
        repetitions: int,
        timeout: int = 1800,
        component: str = "determinism",
    ) -> None:
        if repetitions < 1:
            self._add_check(
                CheckResult(
                    id=f"{check_id}-invalid-repetitions",
                    title=title,
                    status="error",
                    required=True,
                    component=component,
                    summary=f"repetitions must be >= 1, got {repetitions}",
                )
            )
            return
        results: list[RawResult] = []
        for _ in range(repetitions):
            results.append(self._run_raw(argv, timeout=timeout))
        passes = sum(1 for r in results if r.exit_code == 0)
        outcome_summary = summarize_repeated_test_outcomes(
            [f"{r.stdout}\n{r.stderr}" for r in results]
        )
        flaky_tests = outcome_summary["flaky_tests"]
        consistent_failures = outcome_summary["consistent_failures"]
        if passes == repetitions and not flaky_tests:
            status = "pass"
            summary = f"{passes}/{repetitions} repetitions passed"
        elif passes == 0 and not flaky_tests:
            status = "fail"
            summary = f"0/{repetitions} repetitions passed"
            if consistent_failures:
                summary += f"; consistent failures: {', '.join(consistent_failures[:10])}"
        else:
            status = "fail"
            summary = f"FLAKY: {passes}/{repetitions} repetitions passed"
            named_flaky = ", ".join(flaky_tests[:10]) if flaky_tests else "no individual libtest names parsed"
            self._add_finding(
                Finding(
                    id="flaky-repeated-command",
                    severity="P0",
                    area="tests",
                    title="Repeated command alternated between pass and fail",
                    evidence=f"{title}: {passes}/{repetitions} passed; flaky tests: {named_flaky}",
                    remediation="Quarantine is not a fix: capture the failing seed/order/state, remove shared global state, and add a deterministic regression.",
                )
            )
        last = results[-1]
        self._record_raw(
            last,
            check_id=check_id,
            title=title,
            component=component,
            status=status,
            summary=summary,
            metrics={
                "repetitions": repetitions,
                "passes": passes,
                "exit_codes": [r.exit_code for r in results],
                "tests_seen": outcome_summary["tests_seen"][:200],
                "flaky_tests": flaky_tests,
                "consistent_failures": consistent_failures,
            },
        )
        if flaky_tests:
            self._metric("flaky_tests", flaky_tests)

    def _debug_rd(self) -> Path | None:
        if self._debug_binary and self._debug_binary.is_file():
            return self._debug_binary
        raw = self._run_check(
            ["cargo", "build", "-p", "rd-cli", "--all-features", "--locked"],
            check_id="determinism-build-rd",
            title="Build all-features rd for determinism probes",
            component="determinism",
            timeout=3600,
        )
        candidate = self.repo / "target/debug/rd"
        self._debug_binary = candidate if raw.exit_code == 0 and candidate.is_file() else None
        return self._debug_binary

    def _component_determinism(self) -> None:
        if self.options.repeat is not None:
            repetitions = int(self.options.repeat)
        elif self._profile() in {"deep", "nightly", "weekly"}:
            repetitions = int(self._cfg("flaky_hunt_repetitions", 5))
        else:
            repetitions = int(self._cfg("repeat_targeted_tests", 3))
        commands = self._test_commands_for_packages(self._targeted_packages())
        repeat_commands = commands[:6]
        self._ensure_native_test_fixtures("determinism", repeat_commands)
        if repeat_commands:
            for title, argv in repeat_commands:
                self._repeat_command(
                    argv,
                    check_id=f"determinism-repeat-{title}",
                    title=f"Repeat {repetitions}×: {title}",
                    repetitions=repetitions,
                    timeout=3600,
                )
        else:
            self._skip("determinism-repeat-tests", "Repeated targeted tests", "determinism", "No targeted test command selected.", required=False)

        apks = self._apk_candidates(launchable_only=True)
        if not apks:
            self._skip("determinism-real-apk", "Real-APK output determinism", "determinism", "No launchable pinned APK is present.")
            return
        binary = self._debug_rd()
        if binary is None:
            return
        out = self.output_dir / "determinism"
        out.mkdir(parents=True, exist_ok=True)
        apk = apks[0]
        inspect_hashes: list[str] = []
        for idx in range(2):
            raw = self._run_raw([str(binary), "inspect", "--json", str(apk)], timeout=300)
            parsed = extract_json(raw.stdout)
            inspect_hashes.append(sha256_text(canonical_json(parsed)) if parsed is not None else sha256_text(raw.stdout))
            if idx == 1:
                status = "pass" if len(set(inspect_hashes)) == 1 and raw.exit_code == 0 else "fail"
                self._record_raw(
                    raw,
                    check_id="determinism-inspect-json",
                    title="Repeated rd inspect --json is byte-stable after canonicalization",
                    component="determinism",
                    status=status,
                    summary=f"hashes={inspect_hashes}",
                    metrics={"inspect_hashes": inspect_hashes},
                )
        shot_hashes: list[str] = []
        last_raw: RawResult | None = None
        for idx in range(2):
            target = out / f"frame-{idx}.png"
            last_raw = self._run_raw([str(binary), "app", "shot", "--out", str(target), str(apk)], timeout=600)
            if target.is_file():
                shot_hashes.append(sha256_file(target))
        if last_raw is not None:
            status = "pass" if len(shot_hashes) == 2 and len(set(shot_hashes)) == 1 else "fail"
            self._record_raw(
                last_raw,
                check_id="determinism-screenshot",
                title="Repeated rd app shot PNG is byte-identical",
                component="determinism",
                status=status,
                summary=f"frame hashes={shot_hashes}",
                metrics={"frame_sha256": shot_hashes},
            )
            if status != "pass":
                self._add_finding(
                    Finding(
                        id="nondeterministic-frame",
                        severity="P0",
                        area="render-ui",
                        title="Repeated first frame is not byte-identical",
                        evidence=f"apk={apk.name}, hashes={shot_hashes}",
                        remediation="Find time/random/global-state leakage; Law 4 determinism is a runtime contract.",
                    )
                )

    # ------------------------------------------------------------------- deps
    @staticmethod
    def _cargo_purl(name: str, version: str) -> str:
        encoded_name = re.sub(r"[^A-Za-z0-9._-]", "-", name)
        encoded_version = re.sub(r"[^A-Za-z0-9.+_-]", "-", version)
        return f"pkg:cargo/{encoded_name}@{encoded_version}"

    @staticmethod
    def _dependency_blast_radius(
        metadata: Mapping[str, Any],
    ) -> dict[tuple[str, str], dict[str, Any]]:
        packages = metadata.get("packages", []) if isinstance(metadata.get("packages"), list) else []
        by_id: dict[str, Mapping[str, Any]] = {}
        for package in packages:
            if isinstance(package, Mapping) and package.get("id"):
                by_id[str(package["id"])] = package
        workspace_ids = {
            str(x) for x in metadata.get("workspace_members", []) if isinstance(x, str)
        }
        resolve = metadata.get("resolve", {}) if isinstance(metadata.get("resolve"), Mapping) else {}
        nodes = resolve.get("nodes", []) if isinstance(resolve.get("nodes"), list) else []

        direct_workspace_consumers: dict[str, set[str]] = {pid: set() for pid in by_id}
        reverse_edges: dict[str, set[str]] = {pid: set() for pid in by_id}
        kinds: dict[str, set[str]] = {pid: set() for pid in by_id}
        for node in nodes:
            if not isinstance(node, Mapping):
                continue
            consumer_id = str(node.get("id") or "")
            if consumer_id not in by_id:
                continue
            deps = node.get("deps", []) if isinstance(node.get("deps"), list) else []
            for dep in deps:
                if not isinstance(dep, Mapping):
                    continue
                dep_id = str(dep.get("pkg") or "")
                if dep_id not in by_id:
                    continue
                reverse_edges.setdefault(dep_id, set()).add(consumer_id)
                consumer = by_id[consumer_id]
                consumer_name = str(consumer.get("name") or consumer_id)
                if consumer_id in workspace_ids:
                    direct_workspace_consumers.setdefault(dep_id, set()).add(consumer_name)
                dep_kinds = dep.get("dep_kinds", [])
                if isinstance(dep_kinds, list):
                    for kind_info in dep_kinds:
                        kind = "normal"
                        if isinstance(kind_info, Mapping) and kind_info.get("kind"):
                            kind = str(kind_info["kind"])
                        kinds.setdefault(dep_id, set()).add(kind)

        def reachable_workspace_consumers(start: str) -> set[str]:
            seen: set[str] = set()
            frontier = [start]
            consumers: set[str] = set()
            while frontier:
                current = frontier.pop()
                for consumer_id in reverse_edges.get(current, ()):
                    if consumer_id in seen:
                        continue
                    seen.add(consumer_id)
                    consumer = by_id.get(consumer_id, {})
                    if consumer_id in workspace_ids:
                        consumers.add(str(consumer.get("name") or consumer_id))
                    frontier.append(consumer_id)
            return consumers

        out: dict[tuple[str, str], dict[str, Any]] = {}
        for package_id, package in by_id.items():
            name = str(package.get("name") or "")
            version = str(package.get("version") or "")
            if not name or not version:
                continue
            targets = package.get("targets", []) if isinstance(package.get("targets"), list) else []
            target_kinds: set[str] = set()
            for target in targets:
                if isinstance(target, Mapping):
                    target_kinds.update(str(x) for x in target.get("kind", []) if isinstance(x, str))
                    target_kinds.update(str(x) for x in target.get("crate_types", []) if isinstance(x, str))
            out[(name, version)] = {
                "direct_workspace_consumers": sorted(
                    direct_workspace_consumers.get(package_id, ())
                ),
                "reachable_workspace_consumers": sorted(
                    reachable_workspace_consumers(package_id)
                ),
                "dependency_kinds": sorted(kinds.get(package_id, ())),
                "build_dependency": "build" in kinds.get(package_id, set()),
                "proc_macro": "proc-macro" in target_kinds,
                "has_build_script": "custom-build" in target_kinds,
                "links": package.get("links"),
                "source": package.get("source"),
                "license": package.get("license"),
            }
        return out

    def _write_cyclonedx_sbom(
        self, metadata: Mapping[str, Any]
    ) -> tuple[Path, int] | None:
        packages = metadata.get("packages", []) if isinstance(metadata.get("packages"), list) else []
        components: list[dict[str, Any]] = []
        for package in sorted(
            (p for p in packages if isinstance(p, Mapping)),
            key=lambda p: (str(p.get("name")), str(p.get("version")), str(p.get("source"))),
        ):
            name = str(package.get("name") or "")
            version = str(package.get("version") or "")
            if not name or not version:
                continue
            component: dict[str, Any] = {
                "type": "library",
                "bom-ref": self._cargo_purl(name, version),
                "name": name,
                "version": version,
                "purl": self._cargo_purl(name, version),
                "properties": [
                    {"name": "rd-sentinel:source", "value": str(package.get("source") or "workspace/path")},
                    {"name": "rd-sentinel:rust-version", "value": str(package.get("rust_version") or "")},
                ],
            }
            license_text = package.get("license")
            if license_text:
                component["licenses"] = [{"expression": str(license_text)}]
            components.append(component)

        serial = sha256_text(canonical_json(components))[:32]
        document = {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "serialNumber": f"urn:uuid:{serial[:8]}-{serial[8:12]}-{serial[12:16]}-{serial[16:20]}-{serial[20:32]}",
            "version": 1,
            "metadata": {
                "tools": [
                    {
                        "vendor": "RustyDroid",
                        "name": "RD Sentinel",
                        "version": str(self.report.get("bot_version") or ""),
                    }
                ],
                "properties": [
                    {"name": "rd-sentinel:head-sha", "value": str(self.report.get("head_sha") or "")},
                    {"name": "rd-sentinel:profile", "value": self._profile()},
                ],
            },
            "components": components,
        }
        target = self.output_dir / "sbom.cyclonedx.json"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(canonical_json(document) + "\n", encoding="utf-8")
        return target, len(components)

    @staticmethod
    def _query_osv(
        packages: Sequence[tuple[str, str]], *, timeout: float = 30.0, max_pages: int = 10
    ) -> dict[str, Any]:
        if not packages:
            return {
                "results": [],
                "queries": 0,
                "truncated": False,
                "pages": 0,
                "pending_queries": 0,
            }
        unique = sorted(set(packages))
        try:
            page_limit = max(1, min(100, int(max_pages)))
        except (TypeError, ValueError):
            page_limit = 10
        ids_by_package: dict[tuple[str, str], list[str]] = {pkg: [] for pkg in unique}
        seen_by_package: dict[tuple[str, str], set[str]] = {pkg: set() for pkg in unique}
        pending: list[tuple[tuple[str, str], str | None]] = [(pkg, None) for pkg in unique]
        pages = 0
        while pending and pages < page_limit:
            pages += 1
            queries: list[dict[str, Any]] = []
            for (name, version), page_token in pending:
                query: dict[str, Any] = {
                    "package": {"purl": CampaignRunner._cargo_purl(name, version)}
                }
                if page_token:
                    query["page_token"] = page_token
                queries.append(query)
            request = urllib_request.Request(
                OSV_QUERYBATCH_URL,
                data=json.dumps({"queries": queries}, separators=(",", ":")).encode("utf-8"),
                headers={
                    "Content-Type": "application/json",
                    "User-Agent": "RD-Sentinel",
                },
                method="POST",
            )
            with urllib_request.urlopen(request, timeout=timeout) as response:
                document = json.loads(response.read().decode("utf-8"))
            raw_results = document.get("results", []) if isinstance(document, Mapping) else []
            next_pending: list[tuple[tuple[str, str], str | None]] = []
            # OSV querybatch pagination is per-query: each result may carry its
            # own next_page_token. Follow-up batches must contain only those
            # unfinished queries, each with its own token, while this map keeps
            # the original package identity stable across subset responses.
            for (package_key, _old_token), result in zip(pending, raw_results):
                vulns = result.get("vulns", []) if isinstance(result, Mapping) else []
                for vuln in vulns:
                    if not isinstance(vuln, Mapping) or not vuln.get("id"):
                        continue
                    vuln_id = str(vuln["id"])
                    if vuln_id not in seen_by_package[package_key]:
                        seen_by_package[package_key].add(vuln_id)
                        ids_by_package[package_key].append(vuln_id)
                if isinstance(result, Mapping) and result.get("next_page_token"):
                    next_pending.append((package_key, str(result["next_page_token"])))
            pending = next_pending
        results = [
            {
                "package": name,
                "version": version,
                "vulnerability_ids": ids_by_package[(name, version)],
            }
            for name, version in unique
            if ids_by_package[(name, version)]
        ]
        return {
            "results": results,
            "queries": len(unique),
            "truncated": bool(pending),
            "pages": pages,
            "pending_queries": len(pending),
        }

    def _component_deps(self) -> None:
        raw = self._run_raw(["cargo", "metadata", "--locked", "--format-version", "1"], timeout=600)
        metadata = extract_json(raw.stdout)
        metrics: dict[str, Any] = {}
        status = "pass" if raw.exit_code == 0 and isinstance(metadata, Mapping) else "fail"
        if isinstance(metadata, Mapping):
            packages = metadata.get("packages", []) if isinstance(metadata.get("packages"), list) else []
            deps_cfg = self.policy.get("dependencies", {}) if isinstance(self.policy.get("dependencies"), Mapping) else {}
            allowed_licenses = deps_cfg.get("allowed_license_tokens", [])
            registry_prefix = str(deps_cfg.get("allowed_registry_source_prefix", ""))
            metrics_report = self.report.get("metrics", {}) if isinstance(self.report.get("metrics"), Mapping) else {}
            msrv = _version_tuple(str(metrics_report.get("rust_version") or "1.76"))
            license_failures: list[str] = []
            source_failures: list[str] = []
            msrv_failures: list[str] = []
            duplicates: dict[str, set[str]] = {}
            for package in packages:
                if not isinstance(package, Mapping):
                    continue
                name = str(package.get("name"))
                version = str(package.get("version"))
                duplicates.setdefault(name, set()).add(version)
                if not license_expression_allowed(package.get("license"), allowed_licenses):
                    # Local path packages are usually covered by the workspace
                    # license; cargo metadata still reports it. Keep the rule
                    # strict for resolved registry packages and any package
                    # that declares a license expression.
                    if package.get("source") or package.get("license"):
                        license_failures.append(f"{name} {version}: {package.get('license')!r}")
                source = package.get("source")
                if source and registry_prefix and not str(source).startswith(registry_prefix):
                    source_failures.append(f"{name} {version}: {source}")
                rust_version = _version_tuple(package.get("rust_version"))
                if rust_version and msrv and rust_version > msrv:
                    msrv_failures.append(f"{name} {version}: rust {package.get('rust_version')} > MSRV {'.'.join(map(str, msrv))}")
            duplicate_names = {k: sorted(v) for k, v in duplicates.items() if len(v) > 1}
            blast_radius = self._dependency_blast_radius(metadata)
            changed_dependencies: list[dict[str, Any]] = []
            raw_changes = self.report.get("dependency_changes", [])
            if isinstance(raw_changes, list):
                for change in raw_changes:
                    if not isinstance(change, Mapping):
                        continue
                    name = str(change.get("name") or "")
                    version = str(change.get("new") or "")
                    detail = blast_radius.get((name, version), {})
                    changed_dependencies.append(
                        {
                            "name": name,
                            "old": change.get("old"),
                            "new": change.get("new"),
                            "kind": change.get("kind"),
                            **detail,
                        }
                    )
            high_blast = [
                item
                for item in changed_dependencies
                if len(item.get("reachable_workspace_consumers", [])) >= 3
                or item.get("build_dependency")
                or item.get("proc_macro")
                or item.get("has_build_script")
                or item.get("links")
            ]
            for item in high_blast[:20]:
                self._add_finding(
                    Finding(
                        "dependency-high-blast-radius",
                        "P2",
                        "dependencies-build",
                        f"Changed dependency has a wide blast radius: {item.get('name')}",
                        canonical_json(item)[:1000],
                        "Cargo.lock",
                        remediation="Exercise every reachable workspace consumer plus feature/A-B evidence; build/proc-macro/native-link crates execute at build time or change linking.",
                    )
                )
            sbom_result = self._write_cyclonedx_sbom(metadata)
            metrics = {
                "resolved_packages": len(packages),
                "duplicate_package_names": duplicate_names,
                "license_failures": license_failures,
                "source_failures": source_failures,
                "msrv_failures": msrv_failures,
                "changed_dependency_blast_radius": changed_dependencies,
                "high_blast_radius_changes": high_blast,
                "sbom_path": (
                    str(sbom_result[0].relative_to(self.repo))
                    if sbom_result and sbom_result[0].is_relative_to(self.repo)
                    else (str(sbom_result[0]) if sbom_result else None)
                ),
                "sbom_packages": sbom_result[1] if sbom_result else 0,
            }
            self._metric("resolved_packages", len(packages))
            self._metric("duplicate_dependency_names", len(duplicate_names))
            self._metric("sbom_packages", sbom_result[1] if sbom_result else 0)
            if sbom_result:
                self._metric("sbom_sha256", sha256_file(sbom_result[0]))
            if license_failures:
                status = "fail"
                self._add_finding(Finding("dependency-license-not-allowed", "P1", "dependencies-build", "Dependency license is outside the approved set", "; ".join(license_failures[:20]), "Cargo.lock", remediation="Replace the dependency or approve its license deliberately in policy/ADR."))
            if source_failures:
                status = "fail"
                self._add_finding(Finding("dependency-source-not-allowed", "P0", "dependencies-build", "Dependency resolves outside crates.io", "; ".join(source_failures[:20]), "Cargo.lock", remediation="Restore the canonical crates.io source."))
            if msrv_failures:
                status = "fail"
                self._add_finding(Finding("dependency-exceeds-msrv", "P1", "dependencies-build", "Dependency requires a Rust newer than MSRV", "; ".join(msrv_failures[:20]), "Cargo.lock", remediation="Pin/select an MSRV-compatible release or change MSRV only through an ADR."))
        self._record_raw(raw, check_id="deps-metadata-license-msrv", title="Resolved graph license/source/MSRV audit", component="deps", status=status, metrics=metrics)

        audit_bin = shutil.which("cargo-audit")
        if audit_bin:
            raw_audit = self._run_raw(["cargo", "audit", "--json"], timeout=900)
            parsed = extract_json(raw_audit.stdout)
            vuln_count = None
            if isinstance(parsed, Mapping):
                vulns = parsed.get("vulnerabilities", {})
                if isinstance(vulns, Mapping) and isinstance(vulns.get("list"), list):
                    vuln_count = len(vulns["list"])
                elif isinstance(vulns, list):
                    vuln_count = len(vulns)
            if vuln_count:
                self._add_finding(Finding("cargo-audit-vulnerability", "P0", "dependencies-build", "cargo audit reported vulnerabilities", f"count={vuln_count}", "Cargo.lock", remediation="Update or replace affected crates; do not add an audit ignore without a dated exception."))
            self._record_raw(raw_audit, check_id="deps-cargo-audit", title="cargo audit advisory scan", component="deps", status="fail" if vuln_count else ("pass" if raw_audit.exit_code == 0 else "warn"), required=self._profile() in {"nightly", "weekly", "deps", "deep"}, metrics={"vulnerabilities": vuln_count})
        else:
            required = self._profile() in {"nightly", "weekly", "deps", "deep"}
            self._skip("deps-cargo-audit", "cargo audit advisory scan", "deps", "cargo-audit is not installed on this runner.", required=required)
            if required:
                self._add_finding(Finding("cargo-audit-missing", "P2", "dependencies-build", "cargo-audit missing from deep runner", "cargo-audit executable not found", remediation="Install cargo-audit in the deep workflow or on the self-hosted runner."))

        deny_bin = shutil.which("cargo-deny")
        deny_cfg = self.repo / "deny.toml"
        supply_profile = self._profile() in {"nightly", "weekly", "deps", "deep", "auto", "static", "plan"} or "deps" in self.report.get("executed_components", [])
        if deny_bin and deny_cfg.is_file():
            deny_argv = ["cargo", "deny", "--locked"]
            if not self.options.allow_network:
                # Still consult an already-fetched RustSec DB; maximum-db-
                # staleness in deny.toml prevents silently ancient evidence.
                deny_argv.append("--offline")
            # `cargo deny check` performs all checks unless targets are named.
            deny_argv.append("check")
            self._run_check(
                deny_argv,
                check_id="deps-cargo-deny",
                title="cargo-deny advisories/licenses/bans/sources policy",
                component="deps",
                timeout=1800,
                required=True,
            )
        else:
            self._skip(
                "deps-cargo-deny",
                "cargo-deny advisories/licenses/bans/sources policy",
                "deps",
                "cargo-deny or deny.toml is not present on this runner.",
                required=bool(supply_profile),
            )

        machete_bin = shutil.which("cargo-machete")
        if machete_bin:
            # Do not use --with-metadata here: upstream documents that it may
            # modify Cargo.lock, and an evidence campaign must not mutate the
            # candidate tree it is judging.
            self._run_check(
                ["cargo", "machete"],
                check_id="deps-cargo-machete",
                title="cargo-machete unused-dependency scan",
                component="deps",
                timeout=900,
                required=False,
            )
        else:
            self._skip(
                "deps-cargo-machete",
                "cargo-machete unused-dependency scan",
                "deps",
                "cargo-machete is not installed; unused-dependency detection is advisory trend evidence.",
                required=False,
            )

        if isinstance(metadata, Mapping):
            osv_packages: list[tuple[str, str]] = []
            for package in metadata.get("packages", []) if isinstance(metadata.get("packages"), list) else []:
                if not isinstance(package, Mapping) or not package.get("source"):
                    continue
                name = str(package.get("name") or "")
                version = str(package.get("version") or "")
                if name and version:
                    osv_packages.append((name, version))
            if self.options.allow_network:
                try:
                    try:
                        osv_timeout = max(
                            1.0,
                            min(300.0, float(self._cfg("osv_timeout_seconds", 30))),
                        )
                    except (TypeError, ValueError):
                        osv_timeout = 30.0
                    try:
                        osv_max_pages = max(1, min(100, int(self._cfg("osv_max_pages", 10))))
                    except (TypeError, ValueError):
                        osv_max_pages = 10
                    osv = self._query_osv(
                        osv_packages, timeout=osv_timeout, max_pages=osv_max_pages
                    )
                    vuln_total = sum(
                        len(item.get("vulnerability_ids", []))
                        for item in osv.get("results", [])
                        if isinstance(item, Mapping)
                    )
                    self._metric("osv_vulnerability_count", vuln_total)
                    self._metric("osv_packages_queried", osv.get("queries"))
                    self._metric("osv_pages", osv.get("pages"))
                    self._metric("osv_pagination_truncated", bool(osv.get("truncated")))
                    if vuln_total:
                        self._add_finding(
                            Finding(
                                "osv-vulnerability",
                                "P1",
                                "dependencies-build",
                                "OSV reports vulnerabilities in the resolved graph",
                                canonical_json(osv.get("results", []))[:1500],
                                "Cargo.lock",
                                remediation="Update or replace affected crates; use RustSec/cargo-audit detail for exploitability and do not hide this in an undated ignore.",
                            )
                        )
                    self._add_check(
                        CheckResult(
                            id="deps-osv-querybatch",
                            title="OSV vulnerability batch query",
                            status="fail" if vuln_total else "pass",
                            required=True,
                            component="deps",
                            summary=(
                                f"queried {osv.get('queries')} crates, {vuln_total} vulnerability IDs"
                                + (
                                    f"; pagination incomplete after {osv.get('pages')} page(s), "
                                    f"{osv.get('pending_queries')} unfinished queries"
                                    if osv.get("truncated")
                                    else f"; pagination complete in {osv.get('pages')} page(s)"
                                )
                            ),
                            metrics={
                                "packages_queried": osv.get("queries"),
                                "vulnerabilities": vuln_total,
                                "affected": osv.get("results", []),
                                "truncated": osv.get("truncated"),
                                "pages": osv.get("pages"),
                                "pending_queries": osv.get("pending_queries"),
                            },
                        )
                    )
                except (OSError, ValueError, urllib_error.URLError) as exc:
                    self._add_check(
                        CheckResult(
                            id="deps-osv-querybatch",
                            title="OSV vulnerability batch query",
                            status="error",
                            required=True,
                            component="deps",
                            summary=f"OSV query failed: {redact(str(exc), repo=self.repo, limit=300)}",
                        )
                    )
            else:
                self._skip(
                    "deps-osv-querybatch",
                    "OSV vulnerability batch query",
                    "deps",
                    "Network not allowed for this campaign; OSV results are not claimed.",
                    required=False,
                )

        if self._profile() in {"nightly", "weekly", "deps"}:
            if self.options.allow_network:
                raw_update = self._run_raw(["cargo", "update", "--dry-run"], timeout=900)
                updates = [line.strip() for line in (raw_update.stdout + raw_update.stderr).splitlines() if "Updating" in line or "Adding" in line or "Removing" in line]
                self._record_raw(raw_update, check_id="deps-update-dry-run", title="cargo update --dry-run visibility", component="deps", required=False, status="pass" if raw_update.exit_code == 0 else "warn", metrics={"update_lines": updates[:100]})
            else:
                self._skip("deps-update-dry-run", "cargo update --dry-run visibility", "deps", "Network not allowed for this campaign.", required=False)

    # ----------------------------------------------------------------- future
    def _component_future(self) -> None:
        list_raw = self._run_raw(["rustup", "toolchain", "list"], timeout=120)
        installed = list_raw.stdout + list_raw.stderr
        self._record_raw(list_raw, check_id="future-toolchain-list", title="Installed Rust toolchains", component="future", required=False)
        trials = [
            ("beta", ["cargo", "+beta", "check", "--workspace", "--all-targets", "--locked"]),
            ("1.76.0", ["cargo", "+1.76.0", "check", "--workspace", "--all-features", "--locked"]),
        ]
        for name, argv in trials:
            if name not in installed and not (name == "beta" and "beta" in installed):
                required = self._profile() in {"deep", "nightly", "weekly"}
                self._skip(f"future-{name}", f"Rust {name} trial", "future", f"Rust toolchain {name} is not installed.", required=required)
                continue
            self._run_check(argv, check_id=f"future-{name}", title=f"Rust {name} compatibility trial", component="future", timeout=3600)

    # -------------------------------------------------------------- perf tools
    def _build_release(self, repo: Path, *, check_prefix: str = "perf") -> Path | None:
        if repo == self.repo and self._release_binary and self._release_binary.is_file():
            return self._release_binary
        raw = self._run_check(
            ["cargo", "build", "--release", "-p", "rd-cli", "--all-features", "--locked"],
            check_id=f"{check_prefix}-build-release-{repo.name}",
            title="Build all-features release rd",
            component="perf",
            cwd=repo,
            timeout=int(self._cfg("command_timeout_seconds", 7200)),
        )
        candidate = repo / "target/release/rd"
        if raw.exit_code != 0 or not candidate.is_file():
            return None
        if repo == self.repo:
            self._release_binary = candidate
        return candidate

    def _run_perf(self, binary: Path, apk: Path, *, label: str) -> dict[str, Any] | None:
        runs = int(self.options.perf_runs or self._cfg("perf_runs", 3))
        raw = self._run_raw(
            [str(binary), "perf", "--json", "--device", "moto-e5", "--runs", str(runs), str(apk)],
            timeout=1800,
        )
        parsed = extract_json(raw.stdout)
        metrics: dict[str, Any] = {}
        status = "fail"
        if isinstance(parsed, Mapping):
            metrics = {
                "pass": parsed.get("pass"),
                "cold_p50_emulated_ms": parsed.get("cold_p50_emulated_ms"),
                "cold_max_emulated_ms": parsed.get("cold_max_emulated_ms"),
                "peak_rss_mb": parsed.get("peak_rss_mb"),
                "app_heap_peak_mb": parsed.get("app_heap_peak_mb"),
            }
        # issue #283: "the measurement ran" and "the budget passed" are two
        # different facts. `rd perf` exits 1 when a budget fails (PRF-06), so
        # using exit_code for both made a red base block every candidate,
        # including one that improves cold start. The measurement check
        # passes only when it produced numbers; the budget verdict is a
        # metric, resolved against the base in _resolve_perf_budgets().
        measured = (
            isinstance(parsed, Mapping)
            and not raw.timed_out
            and isinstance(parsed.get("cold_p50_emulated_ms"), (int, float))
            and isinstance(parsed.get("cold_max_emulated_ms"), (int, float))
        )
        summary = None
        if measured:
            status = "pass"
            budget_ok = parsed.get("pass") is True and raw.exit_code == 0
            metrics["budget_ok"] = budget_ok
            summary = f"exit {raw.exit_code}; measured; absolute budget {'pass' if budget_ok else 'FAIL'}"
            if label == "base":
                self._perf_base_budget_ok[apk.stem] = budget_ok
                self._perf_base_budget_metrics[apk.stem] = dict(metrics)
            elif not budget_ok:
                self._perf_candidate_budget_failures.append((apk.stem, dict(metrics)))
        self._record_raw(raw, check_id=f"perf-rd-perf-{label}-{apk.stem}", title=f"rd perf ({label}, {apk.name})", component="perf", status=status, summary=summary, metrics=metrics)
        return dict(parsed) if isinstance(parsed, Mapping) else None

    def _resolve_perf_budgets(self) -> None:
        """issue #283: decide the absolute `rd perf` budget once per run.

        - Base over budget: pre-existing, reported as a P2 advisory finding.
          The base is the reference, not the change under review.
        - Candidate over budget and base within budget (or base not
          measured in this run): blocking, exactly as before.
        - Candidate over budget and base also over budget: not blocking by
          itself. Regressions stay blocked by the paired A/B gates
          (perf-ab-p50-regression / perf-ab-max-regression), so getting
          worse is still impossible and getting better is no longer blocked.
        """
        for stem, base_ok in sorted(self._perf_base_budget_ok.items()):
            if not base_ok:
                m = self._perf_base_budget_metrics.get(stem, {})
                self._add_finding(Finding("perf-base-budget-preexisting", "P2", "performance", "Absolute rd perf budget already fails on the base", f"apk={stem}, base cold_p50={m.get('cold_p50_emulated_ms')}, cold_max={m.get('cold_max_emulated_ms')}, peak_rss_mb={m.get('peak_rss_mb')}", remediation="Pre-existing on main: fix it in a dedicated performance PR (see issue #271). Candidates are gated by the A/B ratio meanwhile."))
        for stem, metrics in self._perf_candidate_budget_failures:
            base_ok = self._perf_base_budget_ok.get(stem)
            preexisting = base_ok is False
            if preexisting:
                summary = "absolute budget fails on base and candidate (pre-existing); the A/B ratio gates regressions"
            elif base_ok is None:
                summary = "absolute budget failed; base not measured in this run, so it cannot be pre-existing"
            else:
                summary = "absolute budget failed; the base passed it, so this change regressed it"
            self._add_check(
                CheckResult(
                    id=f"perf-budget-candidate-{stem}",
                    title=f"Absolute rd perf budget (candidate, {stem})",
                    status="pass" if preexisting else "fail",
                    required=True,
                    component="perf",
                    summary=summary,
                    metrics={**metrics, "base_budget_ok": base_ok},
                )
            )

    def _run_cold26(
        self,
        binary: Path,
        apk: Path,
        *,
        label: str,
        snapshot_name: str,
        script_path: Path | None = None,
    ) -> dict[str, Any]:
        runs = int(self.options.cold_runs or self._cfg("cold_runs", 5))
        snapshot_dir = self.output_dir / "snapshots" / snapshot_name
        snapshot_dir.mkdir(parents=True, exist_ok=True)
        apk_path = apk.resolve()
        sandbox = os.pathsep.join([str(self.repo), str(apk_path.parent), str(snapshot_dir.resolve())])
        raw = self._run_raw(
            [
                sys.executable,
                str(script_path or (self.repo / "scripts/cold26.py")),
                "--apk",
                str(apk_path),
                "--bin",
                str(binary),
                "--runs",
                str(runs),
                "--snapshot",
                "on",
                "--capture",
                "--budget",
                str(self._threshold("cold_snapshot_median_ms", 26.0)),
            ],
            timeout=1800,
            env_extra={
                "RD_SNAPSHOT_DIR": str(snapshot_dir),
                "RD_MCP_SANDBOX": sandbox,
            },
        )
        parsed = parse_cold26_output(raw.stdout)
        metrics = {
            "median_ms": parsed["median_ms"],
            "p95_ms": parsed["p95_ms"],
            "max_ms": parsed["max_ms"],
            "interactive_median_ms": parsed["interactive_median_ms"],
            "interactive_p95_ms": parsed["interactive_p95_ms"],
            "interactive_max_ms": parsed["interactive_max_ms"],
            "tap_successes": parsed["tap_successes"],
            "tap_failures": parsed["tap_failures"],
            "frame_hashes": parsed["frame_hashes"],
            "hits": len(parsed["hits"]),
            "captures": len(parsed["captures"]),
            "frame_samples": parsed.get("sample_count"),
            "frame_mad_ms": parsed.get("mad_ms"),
            "interactive_samples": parsed.get("interactive_sample_count"),
            "interactive_mad_ms": parsed.get("interactive_mad_ms"),
            **_distribution("frame", parsed.get("frame_times_ms", [])),
            **_distribution("interactive", parsed.get("interactive_times_ms", [])),
        }
        status = "pass" if raw.exit_code == 0 and metrics["median_ms"] is not None else "fail"
        self._record_raw(raw, check_id=f"cold26-{label}-{apk.stem}", title=f"Process-cold snapshot campaign ({label}, {apk.name})", component="perf", status=status, metrics=metrics)
        parsed["raw_exit_code"] = raw.exit_code
        return parsed

    def _component_perf(self) -> None:
        binary = self._build_release(self.repo)
        if binary is None:
            return
        size = binary.stat().st_size
        self._metric("release_binary_bytes", size)
        max_bytes = self.baseline.get("release_binary", {}).get("max_bytes_without_ab_review") if isinstance(self.baseline.get("release_binary"), Mapping) else None
        if isinstance(max_bytes, int) and size > max_bytes:
            self._add_finding(Finding("release-binary-over-baseline", "P1", "performance", "Release binary exceeded the no-review size ceiling", f"{size} bytes > {max_bytes}", remediation="Run A/B size analysis and remove accidental dependency/feature/codegen bloat."))
        self._add_check(CheckResult(id="perf-binary-size", title="All-features release binary size", status="fail" if isinstance(max_bytes, int) and size > max_bytes else "pass", component="perf", summary=f"{size} bytes", metrics={"bytes": size, "max_bytes_without_ab_review": max_bytes}))
        self._record_missing_env_corpus("perf")

        apks = self._apk_candidates(launchable_only=True)
        if not apks:
            self._skip("perf-real-apk", "Real-APK perf and cold26", "perf", "No launchable pinned APK present.")
            return
        for apk in apks:
            perf = self._run_perf(binary, apk, label="candidate")
            if perf:
                self._metric("perf_cold_p50_emulated_ms", perf.get("cold_p50_emulated_ms"))
                self._metric("perf_cold_max_emulated_ms", perf.get("cold_max_emulated_ms"))
                self._metric("perf_peak_rss_mb", perf.get("peak_rss_mb"))
                self._metric("perf_app_heap_peak_mb", perf.get("app_heap_peak_mb"))
            cold = self._run_cold26(binary, apk, label="candidate", snapshot_name=f"candidate-{apk.stem}")
            median = cold.get("median_ms")
            p95 = cold.get("p95_ms")
            maximum = cold.get("max_ms")
            if median is not None:
                self._metric("cold_snapshot_median_ms", median)
                self._metric("cold_snapshot_p95_ms", p95)
                self._metric("cold_snapshot_max_ms", maximum)
                self._metric("cold_first_interactive_median_ms", cold.get("interactive_median_ms"))
                self._metric("cold_first_interactive_p95_ms", cold.get("interactive_p95_ms"))
                self._metric("cold_tap_successes", cold.get("tap_successes"))
                self._metric("cold_tap_failures", cold.get("tap_failures"))
                self._metric("cold_snapshot_samples", cold.get("sample_count"))
                self._metric("cold_snapshot_mad_ms", cold.get("mad_ms"))
                self._metric("cold_first_interactive_samples", cold.get("interactive_sample_count"))
                self._metric("cold_first_interactive_mad_ms", cold.get("interactive_mad_ms"))
            if median is None or median >= self._threshold("cold_snapshot_median_ms", 26.0):
                self._add_finding(Finding("cold26-median-regression", "P0", "performance", "Process-cold snapshot median is not below 26 ms", f"apk={apk.name}, median={median}", remediation="Profile exec/paged install/snapshot presentation with RD_PHASES=1; do not snapshot-time the real rd perf gate."))
            if p95 is not None and p95 >= self._threshold("cold_snapshot_p95_ms", 26.0):
                self._add_finding(Finding("cold26-p95-regression", "P1", "performance", "Process-cold snapshot p95 is not below 26 ms", f"apk={apk.name}, p95={p95}", remediation="Reduce tail work in process start, APK CD parsing, manifest verification, and frame publication."))
            if maximum is not None and maximum >= self._threshold("cold_snapshot_max_warn_ms", 50.0):
                self._add_finding(Finding("cold26-tail-regression", "P1", "performance", "Process-cold snapshot tail exceeded warning ceiling", f"apk={apk.name}, max={maximum}", remediation="Investigate scheduler/IO tail and separate one-off host noise from deterministic path work."))

    # --------------------------------------------------------------- real APK
    def _component_real_apk(self) -> None:
        binary = self._build_release(self.repo, check_prefix="real-apk")
        if binary is None:
            return

        weekly = self._profile() == "weekly"
        generic_cycles = int(
            self._cfg(
                "real_apk_cycles_weekly" if weekly else "real_apk_cycles_pr",
                100 if weekly else 10,
            )
        )
        apks = self._apk_candidates(launchable_only=True)
        if not apks:
            self._skip("real-apk-endurance", "Generic real-APK MCP endurance", "real-apk", "No launchable pinned APK present.")
        for apk in apks:
            out_dir = self.output_dir / "real-apk" / apk.stem
            result = run_generic_apk_endurance(
                binary=binary,
                apk=apk,
                repo=self.repo,
                output_dir=out_dir,
                cycles=generic_cycles,
            )
            metrics = result.metrics()
            heap_growth = None
            if result.initial_heap_bytes is not None and result.final_heap_bytes is not None:
                heap_growth = result.final_heap_bytes - result.initial_heap_bytes
            metrics["heap_growth_bytes"] = heap_growth
            heap_warning = bool(
                heap_growth is not None
                and result.initial_heap_bytes
                and heap_growth > 16 * 1024 * 1024
                and result.final_heap_bytes is not None
                and result.final_heap_bytes > 2 * result.initial_heap_bytes
            )
            status = "pass" if result.ok and not heap_warning else "warn" if result.ok else "fail"
            summary = (
                f"{result.cycles_completed}/{generic_cycles} persistent-session cycles; "
                f"calls={sum(result.calls.values())}; heap_growth={heap_growth}"
            )
            if result.error:
                summary += f"; error={result.error}"
            self._add_check(
                CheckResult(
                    id=f"real-apk-generic-endurance-{apk.stem}",
                    title=f"Generic real-APK MCP endurance ({apk.name}, {generic_cycles} cycles)",
                    status=status,
                    required=True,
                    component="real-apk",
                    summary=redact(summary, repo=self.repo, limit=2000),
                    metrics=metrics,
                )
            )
            if not result.ok:
                self._add_finding(
                    Finding(
                        "real-apk-endurance-failed",
                        "P1",
                        "runtime",
                        "Persistent real-APK MCP session failed",
                        f"apk={apk.name}; cycles={result.cycles_completed}/{generic_cycles}; error={result.error}",
                        remediation="Reproduce with the preserved MCP stderr log, add the smallest failing tool sequence as a regression, and fix the runtime/protocol failure.",
                    )
                )
            elif heap_warning:
                self._add_finding(
                    Finding(
                        "real-apk-heap-growth",
                        "P2",
                        "performance",
                        "Real-APK heap grew sharply during a persistent session",
                        f"apk={apk.name}; initial={result.initial_heap_bytes}; final={result.final_heap_bytes}; growth={heap_growth}",
                        remediation="Compare base/candidate heap traces and GC stats; distinguish legitimate app caches from Engine/session leakage.",
                    )
                )

        # The Comprix runner encodes one application's Portuguese UI journey.
        # It must never be pointed at an arbitrary APK; run it only for an
        # explicitly supplied Comprix corpus.
        comprix_env = os.environ.get("RD_SENTINEL_COMPRIX_APK", "")
        comprix_apk = Path(comprix_env) if comprix_env else None
        if comprix_apk is not None and comprix_apk.is_file():
            comprix_cycles = int(
                self._cfg("comprix_cycles_weekly" if weekly else "comprix_cycles_pr", 20 if weekly else 3)
            )
            out_dir = self.output_dir / "real-apk" / "comprix"
            out_dir.mkdir(parents=True, exist_ok=True)
            self._run_check(
                [
                    sys.executable,
                    "scripts/comprix-regression.py",
                    "--rd",
                    str(binary),
                    "--apk",
                    str(comprix_apk.resolve()),
                    "--out",
                    str(out_dir),
                    "--cycles",
                    str(comprix_cycles),
                    "--gc",
                    "on",
                    "--jit",
                    "on",
                    "--typed",
                ],
                check_id="real-apk-comprix-specific",
                title=f"Comprix-specific MCP endurance ({comprix_cycles} cycles)",
                component="real-apk",
                timeout=3600,
            )
        elif comprix_apk is not None:
            self._skip(
                "real-apk-comprix-specific",
                "Comprix-specific MCP endurance",
                "real-apk",
                "RD_SENTINEL_COMPRIX_APK is set but does not point to an existing APK.",
            )

        if "native-security" in self._areas() or self._profile() in {"weekly", "deep"}:
            self._run_check(
                ["bash", "golden-apks/native-gate.sh"],
                check_id="real-apk-native-gate",
                title="Pinned real-APK native gate",
                component="real-apk",
                timeout=3600,
            )

    # ---------------------------------------------------------------- compare
    def _inspect_hash(self, binary: Path, apk: Path, label: str) -> str | None:
        raw = self._run_raw([str(binary), "inspect", "--json", str(apk)], timeout=600)
        parsed = extract_json(raw.stdout)
        digest = sha256_text(canonical_json(parsed)) if parsed is not None else None
        self._record_raw(raw, check_id=f"compare-inspect-{label}", title=f"Differential inspect ({label})", component="compare", status="pass" if raw.exit_code == 0 and digest else "fail", metrics={"canonical_sha256": digest})
        return digest

    def _component_compare(self) -> None:
        base_sha = str(self.report.get("base_sha") or "")
        head_sha = str(self.report.get("head_sha") or "")
        if not base_sha or base_sha == head_sha:
            self._skip("compare-ab", "Base-versus-head differential campaign", "compare", "No distinct base revision is available.")
            return
        candidate_binary = self._build_release(self.repo, check_prefix="compare-candidate")
        if candidate_binary is None:
            return
        candidate_copy = self.output_dir / "bin" / "rd-candidate"
        candidate_copy.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(candidate_binary, candidate_copy)

        temp_root = Path(tempfile.mkdtemp(prefix="rd-sentinel-base-"))
        base_repo = temp_root / "repo"
        try:
            add_raw = self._run_raw(["git", "worktree", "add", "--detach", str(base_repo), base_sha], cwd=self.repo, timeout=600)
            self._record_raw(add_raw, check_id="compare-base-worktree", title="Create isolated base worktree", component="compare")
            if add_raw.exit_code != 0:
                return
            base_binary = self._build_release(base_repo, check_prefix="compare-base")
            if base_binary is None:
                return
            base_copy = self.output_dir / "bin" / "rd-base"
            shutil.copy2(base_binary, base_copy)

            base_size = base_copy.stat().st_size
            candidate_size = candidate_copy.stat().st_size
            self._metric("base_release_binary_bytes", base_size)
            self._metric("candidate_release_binary_bytes", candidate_size)
            ratio = candidate_size / base_size if base_size else None
            if ratio and ratio > self._threshold("binary_size_ratio", 1.05) and (candidate_size - base_size) > self._threshold("binary_size_absolute_bytes", 262144):
                self._add_finding(Finding("binary-size-ab-regression", "P1", "performance", "Release binary grew beyond A/B budget", f"base={base_size}, candidate={candidate_size}, ratio={ratio:.3f}", remediation="Use cargo tree/bloat analysis; feature-gate or remove the new code/dependency."))

            bench_binaries: dict[str, Path] = {}
            efficiency_workloads = [
                "roots",
                "host",
                "sweep",
                "minor",
                "resolve",
                "interp",
                "strings",
                "subtype",
                "jit",
            ]
            for label, source_repo in (("base", base_repo), ("candidate", self.repo)):
                raw_bench = self._run_check(
                    [
                        "cargo",
                        "build",
                        "--release",
                        "-p",
                        "rd-vm",
                        "--example",
                        "efficiency_bench",
                        "--features",
                        "jit",
                        "--locked",
                    ],
                    check_id=f"compare-bench-build-{label}",
                    title=f"Build efficiency bench ({label})",
                    component="compare",
                    cwd=source_repo,
                    timeout=3600,
                )
                bench = source_repo / "target/release/examples/efficiency_bench"
                if raw_bench.exit_code == 0 and bench.is_file():
                    dest = self.output_dir / "bin" / f"efficiency-{label}"
                    shutil.copy2(bench, dest)
                    bench_binaries[label] = dest
            if bench_binaries.keys() == {"base", "candidate"}:
                evidence = self.output_dir / "efficiency"
                evidence.mkdir(parents=True, exist_ok=True)
                bench_out = evidence / "benchmarks.json"
                self._run_check(
                    [
                        sys.executable,
                        "scripts/efficiency-benchmark.py",
                        "--baseline",
                        str(bench_binaries["base"]),
                        "--candidate",
                        str(bench_binaries["candidate"]),
                        "--out",
                        str(bench_out),
                        "--repetitions",
                        "5",
                        "--workloads",
                        *efficiency_workloads,
                    ],
                    check_id="compare-efficiency-benchmark",
                    title="Paired semantic efficiency benchmarks",
                    component="compare",
                    timeout=3600,
                )
                if bench_out.is_file():
                    self._run_check(
                        [
                            sys.executable,
                            "scripts/summarize-efficiency-benchmarks.py",
                            "--evidence",
                            str(evidence),
                            "--input",
                            str(bench_out),
                            "--expected-workloads",
                            *efficiency_workloads,
                        ],
                        check_id="compare-efficiency-summary",
                        title="Paired efficiency summary",
                        component="compare",
                        timeout=600,
                    )
                    summary_file = evidence / "final-benchmark-summary.json"
                    if summary_file.is_file():
                        summary_data = extract_json(summary_file.read_text(encoding="utf-8", errors="ignore"))
                        if isinstance(summary_data, list):
                            floor = self._threshold("efficiency_paired_speedup_floor", 0.9)
                            slow = [r for r in summary_data if isinstance(r, Mapping) and isinstance(r.get("median_paired_speedup"), (int, float)) and float(r["median_paired_speedup"]) < floor]
                            if slow:
                                self._add_finding(Finding("efficiency-ab-regression", "P1", "performance", "Paired efficiency workload regressed", canonical_json(slow)[:1000], remediation="Profile the named workload against base; microbenchmark evidence is workload-specific, not an app-wide percentage claim."))

            apks = self._apk_candidates(launchable_only=True)
            if not apks:
                self._skip("compare-real-apk", "Base-versus-head real-APK probes", "compare", "No launchable pinned APK present.")
                return
            for apk in apks:
                base_hash = self._inspect_hash(base_copy, apk, "base")
                candidate_hash = self._inspect_hash(candidate_copy, apk, "candidate")
                if base_hash and candidate_hash and base_hash != candidate_hash:
                    self._add_finding(Finding("inspect-ab-drift", "P1", "cli-contract", "Canonical inspect output changed versus base", f"apk={apk.name}, base={base_hash[:12]}, candidate={candidate_hash[:12]}", remediation="Review the JSON diff. Contract changes must be additive or documented as breaking."))
                base_perf = self._run_perf(base_copy, apk, label="base")
                candidate_perf = self._run_perf(candidate_copy, apk, label="candidate")
                if base_perf and candidate_perf:
                    bp = base_perf.get("cold_p50_emulated_ms")
                    cp = candidate_perf.get("cold_p50_emulated_ms")
                    bm = base_perf.get("cold_max_emulated_ms")
                    cm = candidate_perf.get("cold_max_emulated_ms")
                    if isinstance(bp, (int, float)) and isinstance(cp, (int, float)) and bp > 0:
                        ratio_p50 = cp / bp
                        self._metric("perf_ab_p50_ratio", ratio_p50)
                        if ratio_p50 > self._threshold("perf_ab_p50_ratio", 1.1) and (cp - bp) > self._threshold("perf_ab_p50_absolute_ms", 25.0):
                            self._add_finding(Finding("perf-ab-p50-regression", "P1", "performance", "Real-execution perf p50 regressed versus base", f"apk={apk.name}, base={bp}, candidate={cp}, ratio={ratio_p50:.3f}", remediation="Use RD_PHASES=1 base/candidate traces to isolate mount, launch, draw, or raster regression."))
                    if isinstance(bm, (int, float)) and isinstance(cm, (int, float)) and bm > 0 and (cm / bm) > self._threshold("perf_ab_max_ratio", 1.15) and (cm - bm) > 25:
                        self._add_finding(Finding("perf-ab-max-regression", "P1", "performance", "Real-execution perf max regressed versus base", f"apk={apk.name}, base={bm}, candidate={cm}, ratio={cm / bm:.3f}", remediation="Investigate tail behavior and deterministic path growth before accepting a new baseline."))

                base_script = base_repo / "scripts/cold26.py"
                base_cold = self._run_cold26(
                    base_copy,
                    apk,
                    label="base",
                    snapshot_name=f"base-{apk.stem}",
                    script_path=base_script if base_script.is_file() else None,
                )
                candidate_cold = self._run_cold26(candidate_copy, apk, label="candidate", snapshot_name=f"candidate-compare-{apk.stem}")
                b_med = base_cold.get("median_ms")
                c_med = candidate_cold.get("median_ms")
                if isinstance(b_med, (int, float)) and isinstance(c_med, (int, float)) and b_med > 0:
                    cold_ratio = c_med / b_med
                    cold_delta = c_med - b_med
                    cold_absolute_floor = self._threshold("cold_ab_median_absolute_ms", 2.0)
                    cold_noise_floor = _noise_floor_ms(
                        [float(v) for v in base_cold.get("frame_times_ms", []) if isinstance(v, (int, float))],
                        [float(v) for v in candidate_cold.get("frame_times_ms", []) if isinstance(v, (int, float))],
                    )
                    cold_threshold_ms = max(cold_absolute_floor, cold_noise_floor)
                    self._metric("cold_ab_median_ratio", cold_ratio)
                    self._metric("cold_ab_median_delta_ms", cold_delta)
                    self._metric("cold_ab_noise_floor_ms", round(cold_noise_floor, 3))
                    self._metric("cold_ab_median_threshold_ms", round(cold_threshold_ms, 3))
                    self._metric("cold_ab_base_mad_ms", base_cold.get("mad_ms"))
                    self._metric("cold_ab_candidate_mad_ms", candidate_cold.get("mad_ms"))
                    if cold_ratio > self._threshold("cold_ab_median_ratio", 1.25) and cold_delta > cold_threshold_ms:
                        self._add_finding(Finding("cold-ab-median-regression", "P1", "performance", "Process-cold snapshot median regressed versus base", f"apk={apk.name}, base={b_med}, candidate={c_med}, ratio={cold_ratio:.3f}, delta={cold_delta:.3f}ms, noise-aware threshold={cold_threshold_ms:.3f}ms", remediation="Profile process start, paged install fingerprinting, and snapshot publication."))
                b_interactive = base_cold.get("interactive_median_ms")
                c_interactive = candidate_cold.get("interactive_median_ms")
                if isinstance(b_interactive, (int, float)) and isinstance(c_interactive, (int, float)) and b_interactive > 0:
                    interactive_ratio = c_interactive / b_interactive
                    interactive_delta = c_interactive - b_interactive
                    interactive_absolute_floor = self._threshold("cold_ab_interactive_absolute_ms", 50.0)
                    interactive_noise_floor = _noise_floor_ms(
                        [float(v) for v in base_cold.get("interactive_times_ms", []) if isinstance(v, (int, float))],
                        [float(v) for v in candidate_cold.get("interactive_times_ms", []) if isinstance(v, (int, float))],
                    )
                    interactive_threshold_ms = max(interactive_absolute_floor, interactive_noise_floor)
                    self._metric("cold_ab_interactive_median_ratio", interactive_ratio)
                    self._metric("cold_ab_interactive_delta_ms", interactive_delta)
                    self._metric("cold_ab_interactive_noise_floor_ms", round(interactive_noise_floor, 3))
                    self._metric("cold_ab_interactive_threshold_ms", round(interactive_threshold_ms, 3))
                    if interactive_ratio > self._threshold("cold_ab_interactive_ratio", 1.25) and interactive_delta > interactive_threshold_ms:
                        self._add_finding(Finding("cold-ab-interactive-regression", "P1", "performance", "Process-cold first-interaction median regressed versus base", f"apk={apk.name}, base={b_interactive}, candidate={c_interactive}, ratio={interactive_ratio:.3f}, delta={interactive_delta:.3f}ms, noise-aware threshold={interactive_threshold_ms:.3f}ms", remediation="Profile snapshot materialization separately from first-frame presentation; do not hide a slower interactive path behind a fast stored frame."))
                if int(base_cold.get("tap_successes") or 0) > 0 and int(candidate_cold.get("tap_successes") or 0) == 0:
                    self._add_finding(Finding("cold-ab-tap-regression", "P1", "agent-protocol", "First tap after a snapshot hit stopped succeeding", f"apk={apk.name}, base_tap_successes={base_cold.get('tap_successes')}, candidate_tap_successes={candidate_cold.get('tap_successes')}", remediation="Reproduce the first materializing tap and add an Engine-handoff regression test."))
                bf = base_cold.get("frame_hashes") or []
                cf = candidate_cold.get("frame_hashes") or []
                if bf and cf and bf != cf:
                    severity = "P2" if "sentinel:baseline-approved" in [str(x) for x in self.report.get("labels", [])] else "P1"
                    self._add_finding(Finding("cold-frame-ab-drift", severity, "render-ui", "Snapshot first frame changed versus base", f"apk={apk.name}, base={bf}, candidate={cf}", remediation="If the render change is intentional, update golden/frame evidence and obtain baseline approval; otherwise treat as a UI regression."))
        finally:
            if base_repo.exists():
                subprocess.run(["git", "-C", str(self.repo), "worktree", "remove", "--force", str(base_repo)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            shutil.rmtree(temp_root, ignore_errors=True)

    # ---------------------------------------------------------------- mutation
    def _component_mutation(self) -> None:
        base_sha = str(self.report.get("base_sha") or "")
        head_sha = str(self.report.get("head_sha") or "")
        if (
            not re.fullmatch(r"[0-9a-fA-F]{40}", base_sha)
            or not re.fullmatch(r"[0-9a-fA-F]{40}", head_sha)
            or base_sha == head_sha
            or self.report.get("head_is_worktree") is True
            or self.report.get("worktree_dirty") is True
        ):
            self._skip(
                "mutation-in-diff",
                "Changed-code mutation testing",
                "mutation",
                "A clean committed base/head pair is required; cargo-mutants must start from a trustworthy diff.",
                required=True,
            )
            return

        changed_rs = [
            path
            for path in self._changed_paths()
            if path.endswith(".rs") and not path.startswith("tools/rd-sentinel/")
        ]
        rust_sources = [path for path in changed_rs if "/src/" in path and not path.endswith("_test.rs")]
        if not rust_sources:
            self._skip(
                "mutation-in-diff",
                "Changed-code mutation testing",
                "mutation",
                "No changed production Rust source can generate mutants.",
                required=False,
            )
            return

        mutants_bin = shutil.which("cargo-mutants")
        if not mutants_bin:
            self._skip(
                "mutation-in-diff",
                "Changed-code mutation testing",
                "mutation",
                "cargo-mutants is not installed on this runner.",
                required=True,
            )
            return

        diff_raw = self._run_raw(
            ["git", "diff", "--unified=0", f"{base_sha}...{head_sha}", "--", "*.rs"],
            cwd=self.repo,
            timeout=600,
        )
        self._record_raw(
            diff_raw,
            check_id="mutation-diff",
            title="Export changed Rust diff for cargo-mutants",
            component="mutation",
        )
        if diff_raw.exit_code != 0 or not diff_raw.stdout.strip():
            if diff_raw.exit_code == 0:
                self._skip(
                    "mutation-in-diff",
                    "Changed-code mutation testing",
                    "mutation",
                    "Git produced no Rust diff for the committed range.",
                    required=False,
                )
            return

        mutation_dir = self.output_dir / "mutation"
        mutation_dir.mkdir(parents=True, exist_ok=True)
        diff_path = mutation_dir / "changed.rs.diff"
        diff_path.write_text(diff_raw.stdout, encoding="utf-8")

        minutes = int(self.options.mutation_minutes or self._cfg("mutation_minutes", 45))
        minutes = max(1, min(240, minutes))
        try:
            test_timeout_seconds = max(
                30,
                min(3600, int(self._cfg("mutation_test_timeout_seconds", 600))),
            )
        except (TypeError, ValueError):
            test_timeout_seconds = 600
        argv = [
            "cargo",
            "mutants",
            "--cargo-arg=--locked",
            "--in-diff",
            str(diff_path),
            "--timeout",
            str(test_timeout_seconds),
            "--output",
            str(mutation_dir),
        ]
        targeted = [
            package
            for package in self._targeted_packages()
            if PACKAGE_RE.fullmatch(package)
        ]
        if 0 < len(targeted) <= 4:
            for package in targeted:
                argv.extend(["--package", package])

        raw = self._run_raw(argv, timeout=minutes * 60 + 600)
        parsed = parse_mutants_output(f"{raw.stdout}\n{raw.stderr}")
        outcomes_path = mutation_dir / "mutants.out" / "outcomes.json"
        if outcomes_path.is_file():
            try:
                structured = parse_mutants_outcomes(
                    json.loads(outcomes_path.read_text(encoding="utf-8"))
                )
            except (OSError, UnicodeDecodeError, json.JSONDecodeError):
                structured = None
            if structured is not None:
                # Structured counters are authoritative; retain readable MISSED
                # lines only if the JSON representation provides no names.
                if not structured.get("missed_mutants"):
                    structured["missed_mutants"] = parsed.get("missed_mutants", [])
                parsed = structured
        missed = int(parsed.get("missed") or 0)
        timeout_count = int(parsed.get("timeout") or 0)
        caught = int(parsed.get("caught") or 0)
        try:
            max_missed = max(0, int(self._cfg("mutation_max_missed", 0)))
        except (TypeError, ValueError):
            max_missed = 0
        self._metric("mutation_caught", caught)
        self._metric("mutation_missed", missed)
        self._metric("mutation_max_missed", max_missed)
        self._metric("mutation_unviable", parsed.get("unviable"))
        self._metric("mutation_timeouts", timeout_count)
        self._metric("mutation_kill_rate", parsed.get("kill_rate"))

        profile = self._profile()
        explicit_or_deep = profile in {"mutants", "deep"} or "sentinel:mutants" in [
            str(x) for x in self.report.get("labels", [])
        ]
        over_limit = missed > max_missed
        if missed:
            self._add_finding(
                Finding(
                    "mutation-survivor",
                    "P1" if explicit_or_deep and over_limit else "P2",
                    "tests",
                    "Changed-code mutants survived the test suite",
                    (
                        f"missed={missed}, max_missed={max_missed}, caught={caught}, "
                        f"timeouts={timeout_count}; "
                        + "; ".join(parsed.get("missed_mutants", [])[:10])
                    )[:1500],
                    remediation="Add tests for the public behavior the surviving mutant changes, or document a genuinely equivalent mutant with a dated evidence exception.",
                )
            )
        if timeout_count:
            self._add_finding(
                Finding(
                    "mutation-timeout",
                    "P2",
                    "tests",
                    "Mutation campaign produced timed-out mutants",
                    f"timeouts={timeout_count}",
                    remediation="Find the slow/pathological test path; a timeout is not evidence that a mutant was caught.",
                )
            )
        status = mutation_check_status(
            exit_code=raw.exit_code,
            missed=missed,
            timeouts=timeout_count,
            blocking_context=explicit_or_deep,
            max_missed=max_missed,
            missing=raw.missing,
            timed_out=raw.timed_out,
        )
        self._record_raw(
            raw,
            check_id="mutation-in-diff",
            title="Changed-code mutation testing (cargo-mutants --in-diff)",
            component="mutation",
            status=status,
            summary=(
                f"caught={caught}, missed={missed}, max_missed={max_missed}, "
                f"unviable={parsed.get('unviable')}, timeouts={timeout_count}, "
                f"packages={targeted or ['workspace-diff']}"
            ),
            metrics={
                **parsed,
                "minutes_budget": minutes,
                "test_timeout_seconds": test_timeout_seconds,
                "max_missed": max_missed,
                "blocking_context": explicit_or_deep,
                "packages": targeted,
            },
        )

    # ------------------------------------------------------------------- fuzz
    def _component_fuzz(self) -> None:
        if self.options.fuzz_seconds is not None:
            seconds = int(self.options.fuzz_seconds)
        elif self._profile() == "weekly":
            seconds = int(self._cfg("fuzz_seconds_weekly", 7200))
        elif self._profile() in {"nightly", "deep"}:
            seconds = int(self._cfg("fuzz_seconds_nightly", 900))
        else:
            seconds = int(self._cfg("fuzz_seconds_pr_parser", 120))
        workers = int(self.options.fuzz_workers or self._cfg("fuzz_workers", 1))
        if seconds < 0 or workers < 1:
            self._add_check(
                CheckResult(
                    id="fuzz-campaign-invalid-config",
                    title="Execute fuzz campaign",
                    status="error",
                    required=True,
                    component="fuzz",
                    summary=f"fuzz seconds/workers must be >= 0/1, got {seconds}/{workers}",
                )
            )
            return
        before = {str(p.relative_to(self.repo)) for p in self.repo.glob("crates/*/fuzz/artifacts/**/*") if p.is_file()}
        if seconds == 0:
            argv = ["bash", "scripts/fuzz-campaign.sh", "--seeds-only"]
            timeout = 1800
        else:
            argv = ["bash", "scripts/fuzz-campaign.sh", str(seconds), str(workers)]
            timeout = seconds + 3600
        raw = self._run_raw(argv, timeout=timeout)
        after = {str(p.relative_to(self.repo)) for p in self.repo.glob("crates/*/fuzz/artifacts/**/*") if p.is_file()}
        new_artifacts = sorted(after - before)
        crashes = [p for p in new_artifacts if "crash-" in Path(p).name]
        leaks = [p for p in new_artifacts if "leak-" in Path(p).name]
        others = [p for p in new_artifacts if p not in crashes and p not in leaks]
        for artifact in crashes:
            self._add_finding(Finding("fuzz-crash", "P0", "parsers", "Fuzz campaign found a crash", artifact, artifact, remediation="Preserve and minimize the artifact, add it as a regression seed/test, and fix the parser panic/UB."))
        for artifact in leaks:
            self._add_finding(Finding("fuzz-leak", "P1", "parsers", "Fuzz campaign found a leak artifact", artifact, artifact, remediation="Minimize the leak and add a bounded-resource regression."))
        for artifact in others:
            self._add_finding(Finding("fuzz-other-artifact", "P2", "parsers", "Fuzz campaign produced a new non-crash artifact", artifact, artifact, remediation="Inspect OOM/timeout artifacts; they can hide algorithmic-complexity regressions."))
        status = "fail" if crashes or raw.exit_code != 0 else "pass"
        self._record_raw(raw, check_id="fuzz-campaign", title=f"Execute fuzz campaign ({seconds}s/target)", component="fuzz", status=status, metrics={"seconds_per_target": seconds, "workers_per_target": workers, "new_artifacts": new_artifacts, "crashes": crashes, "leaks": leaks})
        self._metric("fuzz_seconds_per_target", seconds)
        self._metric("fuzz_new_artifacts", len(new_artifacts))

    # ------------------------------------------------------------------- soak
    def _component_soak(self) -> None:
        binary = self._build_release(self.repo, check_prefix="soak")
        if binary is None:
            return
        apks_dir = self.repo / "target/native-languages/apks"
        build_script = self.repo / "golden/native-languages/build.py"
        if (not apks_dir.is_dir() or not any(apks_dir.glob("*.apk"))) and build_script.is_file():
            self._run_check(
                [sys.executable, "golden/native-languages/build.py", "--out", str(apks_dir)],
                check_id="soak-build-native-apks",
                title="Build signed native-language APK corpus",
                component="soak",
                timeout=int(self._cfg("command_timeout_seconds", 7200)),
            )
        if not apks_dir.is_dir() or not any(apks_dir.glob("*.apk")):
            self._skip("soak-mcp-session", "Native-language MCP session soak", "soak", "Native-language APK corpus is unavailable; Android SDK/NDK prerequisites may be missing.")
            return
        seconds = int(self._cfg("mcp_soak_seconds_weekly", 1800))
        out_dir = self.output_dir / "mcp-soak"
        out_dir.mkdir(parents=True, exist_ok=True)
        self._run_check(
            [sys.executable, "scripts/mcp-session-soak.py", "--rd", str(binary), "--apks", str(apks_dir), "--out", str(out_dir), "--seconds", str(seconds)],
            check_id="soak-mcp-session",
            title=f"Native-language MCP session soak ({seconds}s)",
            component="soak",
            timeout=seconds + 3600,
        )

    # ----------------------------------------------------------------- golden
    def _component_golden(self) -> None:
        binary = self._build_release(self.repo, check_prefix="golden")
        if binary is None:
            return
        if not (self.repo / "golden/vm/run-golden.sh").is_file():
            self._skip("golden-vm-repeated", "Repeated JVM golden comparison", "golden", "golden/vm/run-golden.sh is not present in this checkout.")
            return
        repetitions = int(self.options.repeat) if self.options.repeat is not None else 3
        self._repeat_command(
            ["bash", "golden/vm/run-golden.sh", str(binary)],
            check_id="golden-vm-repeated",
            title="Repeated JVM golden comparison",
            repetitions=repetitions,
            timeout=3600,
            component="golden",
        )

    # -------------------------------------------------------------- dep trial
    def _component_dep_trial(self) -> None:
        if not self.options.allow_network:
            self._skip("dep-trial-update", "Isolated dependency-update trial", "dep-trial", "Network not allowed for this campaign.", required=self._profile() == "weekly")
            return
        head_sha = str(self.report.get("head_sha") or "")
        if not head_sha:
            self._skip("dep-trial-update", "Isolated dependency-update trial", "dep-trial", "Head SHA unavailable.")
            return
        temp_root = Path(tempfile.mkdtemp(prefix="rd-sentinel-dep-trial-"))
        trial_repo = temp_root / "repo"
        try:
            add_raw = self._run_raw(["git", "worktree", "add", "--detach", str(trial_repo), head_sha], cwd=self.repo, timeout=600)
            self._record_raw(add_raw, check_id="dep-trial-worktree", title="Create isolated dependency-trial worktree", component="dep-trial")
            if add_raw.exit_code != 0:
                return
            before = (trial_repo / "Cargo.lock").read_text(encoding="utf-8", errors="ignore") if (trial_repo / "Cargo.lock").is_file() else ""
            raw_update = self._run_check(["cargo", "update"], check_id="dep-trial-cargo-update", title="Trial cargo update in isolated worktree", component="dep-trial", cwd=trial_repo, timeout=1800)
            if raw_update.exit_code != 0:
                return
            after = (trial_repo / "Cargo.lock").read_text(encoding="utf-8", errors="ignore") if (trial_repo / "Cargo.lock").is_file() else ""
            changes, metrics = compare_cargo_locks(before, after)
            self._metric("dep_trial_changes", len(changes))
            self._metric("dep_trial_major_changes", metrics.get("dependency_major_changes"))
            if not changes:
                self._add_check(CheckResult(id="dep-trial-current", title="Dependency graph already current in trial", status="pass", component="dep-trial", summary="cargo update produced no lockfile delta"))
                return
            raw_check = self._run_check(["cargo", "check", "--workspace", "--all-features"], check_id="dep-trial-check", title="Build isolated updated dependency graph", component="dep-trial", cwd=trial_repo, timeout=int(self._cfg("command_timeout_seconds", 7200)))
            if raw_check.exit_code != 0:
                self._add_finding(Finding("dependency-update-trial-fails", "P1", "dependencies-build", "Semver-compatible dependency update trial does not build", canonical_json(changes)[:1200], "Cargo.lock", remediation="Investigate before Dependabot opens/merges the update; pin or prepare a code migration."))
        finally:
            if trial_repo.exists():
                subprocess.run(["git", "-C", str(self.repo), "worktree", "remove", "--force", str(trial_repo)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            shutil.rmtree(temp_root, ignore_errors=True)
