"""Trusted GitHub publishing and maintainer-command handling.

This module is intended to run only from a default-branch workflow. It never
imports or executes candidate code; it consumes a validated JSON report and
re-checks the PR identity and changed-file list through the GitHub API.
"""

from __future__ import annotations

import base64
import gzip
import json
import os
import re
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Mapping, Sequence

from .model import (
    BOT_VERSION,
    REPORT_MARKER,
    match_any,
    redact,
    render_markdown,
    render_sarif,
    validate_report,
)

API_ROOT = "https://api.github.com"
COMMAND_PREFIX = "/rd-sentinel"


class GitHubError(RuntimeError):
    pass


@dataclass(slots=True)
class Command:
    action: str
    profile: str = "auto"
    fuzz_seconds: int | None = None
    mutation_minutes: int | None = None
    raw: str = ""


def parse_command(comment: str) -> Command | None:
    for raw_line in str(comment).splitlines():
        line = raw_line.strip()
        parts = line.split()
        if not parts or parts[0] != COMMAND_PREFIX:
            continue
        if len(parts) == 1 or parts[1] in {"help", "--help", "-h"}:
            return Command("help", raw=line)
        action = parts[1].lower()
        if action == "rerun":
            profile = parts[2] if len(parts) > 2 and parts[2] in {"auto", "deep", "nightly", "weekly", "perf", "fuzz", "deps", "determinism", "mutants", "soak"} else "auto"
            return Command("rerun", profile=profile, raw=line)
        if action == "deep":
            return Command("rerun", profile="deep", raw=line)
        if action in {"perf", "deps", "determinism", "soak"}:
            return Command("rerun", profile=action, raw=line)
        if action == "fuzz":
            seconds = 900
            if len(parts) > 2:
                try:
                    seconds = int(parts[2])
                except ValueError:
                    return Command("invalid", raw=line)
            if seconds < 0 or seconds > 86400:
                return Command("invalid", raw=line)
            return Command("rerun", profile="fuzz", fuzz_seconds=seconds, raw=line)
        if action == "mutants":
            minutes = 45
            if len(parts) > 2:
                try:
                    minutes = int(parts[2])
                except ValueError:
                    return Command("invalid", raw=line)
            if minutes < 1 or minutes > 240:
                return Command("invalid", raw=line)
            return Command(
                "rerun",
                profile="mutants",
                mutation_minutes=minutes,
                raw=line,
            )
        if action == "baseline" and len(parts) > 2 and parts[2] == "propose":
            return Command("baseline-propose", profile="nightly", raw=line)
        if action == "status":
            return Command("status", raw=line)
        return Command("invalid", raw=line)
    return None


def command_authorized(event: Mapping[str, Any], policy: Mapping[str, Any]) -> bool:
    approvals = policy.get("approvals", {}) if isinstance(policy.get("approvals"), Mapping) else {}
    allowed = {
        str(x)
        for x in approvals.get(
            "authorized_comment_associations", ["OWNER", "MEMBER", "COLLABORATOR"]
        )
    }
    comment = event.get("comment", {}) if isinstance(event.get("comment"), Mapping) else {}
    association = str(comment.get("author_association", ""))
    return association in allowed


def help_text() -> str:
    return """### 🛡️ RD Sentinel commands

Maintainers (OWNER/MEMBER/COLLABORATOR) may use:

- `/rd-sentinel rerun` — rerun the risk-selected PR audit
- `/rd-sentinel deep` — run the deep PR campaign
- `/rd-sentinel perf` — run A/B + real-APK performance evidence
- `/rd-sentinel fuzz 900` — run fuzz targets for 900s per target
- `/rd-sentinel deps` — run the dependency-vetting campaign
- `/rd-sentinel determinism` — repeat tests and byte-stability probes
- `/rd-sentinel mutants 45` — run changed-code mutation testing for up to 45 minutes
- `/rd-sentinel baseline propose` — produce fresh baseline evidence for a human-reviewed PR
- `/rd-sentinel status` — link the latest trusted Sentinel report on this issue/PR

Commands cannot approve policy, alter a baseline, merge, or push code.
"""


class GitHubClient:
    def __init__(self, repo: str, token: str | None = None, *, dry_run: bool = False) -> None:
        if "/" not in repo:
            raise ValueError("repository must be owner/name")
        self.repo = repo
        self.token = token if token is not None else os.environ.get("GITHUB_TOKEN", "")
        self.dry_run = dry_run
        self.requests: list[dict[str, Any]] = []

    def request(self, method: str, path: str, body: Any | None = None) -> Any:
        method = method.upper()
        url = path if path.startswith("https://") else API_ROOT + path
        record = {"method": method, "url": url, "body": body}
        self.requests.append(record)
        if self.dry_run and method not in {"GET", "HEAD"}:
            return {"dry_run": True, **record}
        if not self.token:
            raise GitHubError("GITHUB_TOKEN is not set")
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=data,
            method=method,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {self.token}",
                "Content-Type": "application/json",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": f"rd-sentinel/{BOT_VERSION}",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                raw = response.read().decode("utf-8", "replace")
                return json.loads(raw) if raw.strip() else {}
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", "replace") if exc.fp else ""
            raise GitHubError(f"GitHub {method} {path} failed ({exc.code}): {redact(raw, limit=1000)}") from exc
        except urllib.error.URLError as exc:
            raise GitHubError(f"GitHub {method} {path} failed: {exc}") from exc

    def paged(self, path: str) -> list[Any]:
        results: list[Any] = []
        page = 1
        separator = "&" if "?" in path else "?"
        while True:
            chunk = self.request("GET", f"{path}{separator}per_page=100&page={page}")
            if isinstance(chunk, Mapping) and isinstance(chunk.get("items"), list):
                values = chunk["items"]
            elif isinstance(chunk, list):
                values = chunk
            else:
                values = []
            results.extend(values)
            if len(values) < 100:
                return results
            page += 1
            if page > 50:
                raise GitHubError(f"pagination limit exceeded for {path}")

    def get_pr(self, number: int) -> Mapping[str, Any]:
        value = self.request("GET", f"/repos/{self.repo}/pulls/{number}")
        if not isinstance(value, Mapping):
            raise GitHubError("unexpected pull request response")
        return value

    def pr_files(self, number: int) -> list[str]:
        files = self.paged(f"/repos/{self.repo}/pulls/{number}/files")
        return [str(f.get("filename")) for f in files if isinstance(f, Mapping) and f.get("filename")]

    def upsert_issue_comment(self, issue_number: int, body: str, marker: str = REPORT_MARKER) -> Any:
        comments = self.paged(f"/repos/{self.repo}/issues/{issue_number}/comments")
        for comment in reversed(comments):
            if isinstance(comment, Mapping) and marker in str(comment.get("body", "")):
                user = comment.get("user", {}) if isinstance(comment.get("user"), Mapping) else {}
                # Never try to rewrite a marker planted by another account.
                if str(user.get("login", "")) not in {"github-actions[bot]"}:
                    continue
                comment_id = comment.get("id")
                if comment_id:
                    return self.request("PATCH", f"/repos/{self.repo}/issues/comments/{comment_id}", {"body": body})
        return self.request("POST", f"/repos/{self.repo}/issues/{issue_number}/comments", {"body": body})

    def comment(self, issue_number: int, body: str) -> Any:
        return self.request("POST", f"/repos/{self.repo}/issues/{issue_number}/comments", {"body": body})

    def ensure_label(self, name: str, color: str, description: str) -> None:
        encoded = urllib.parse.quote(name, safe="")
        try:
            self.request("GET", f"/repos/{self.repo}/labels/{encoded}")
            return
        except GitHubError:
            pass
        self.request(
            "POST",
            f"/repos/{self.repo}/labels",
            {"name": name, "color": color.lstrip("#"), "description": description[:100]},
        )

    def add_labels(self, issue_number: int, labels: Sequence[str]) -> Any:
        if not labels:
            return {}
        return self.request("POST", f"/repos/{self.repo}/issues/{issue_number}/labels", {"labels": list(labels)})

    def remove_label(self, issue_number: int, label: str) -> Any:
        encoded = urllib.parse.quote(label, safe="")
        try:
            return self.request("DELETE", f"/repos/{self.repo}/issues/{issue_number}/labels/{encoded}")
        except GitHubError:
            return {}

    def create_issue(self, title: str, body: str, labels: Sequence[str]) -> Any:
        return self.request("POST", f"/repos/{self.repo}/issues", {"title": title, "body": body, "labels": list(labels)})

    def close_issue(self, issue_number: int) -> Any:
        return self.request("PATCH", f"/repos/{self.repo}/issues/{issue_number}", {"state": "closed"})

    def dispatch_workflow(self, workflow: str, ref: str, inputs: Mapping[str, str]) -> Any:
        return self.request(
            "POST",
            f"/repos/{self.repo}/actions/workflows/{urllib.parse.quote(workflow, safe='')}/dispatches",
            {"ref": ref, "inputs": dict(inputs)},
        )

    def create_commit_status(self, sha: str, state: str, *, context: str, description: str, target_url: str = "") -> Any:
        body: dict[str, Any] = {"state": state, "context": context, "description": description[:140]}
        if target_url:
            body["target_url"] = target_url
        return self.request("POST", f"/repos/{self.repo}/statuses/{sha}", body)

    def create_check_run(self, payload: Mapping[str, Any]) -> Any:
        return self.request("POST", f"/repos/{self.repo}/check-runs", dict(payload))

    def update_check_run(self, check_run_id: int, payload: Mapping[str, Any]) -> Any:
        return self.request(
            "PATCH", f"/repos/{self.repo}/check-runs/{check_run_id}", dict(payload)
        )

    def upload_code_scanning_sarif(
        self, *, sarif: Mapping[str, Any], commit_sha: str, ref: str
    ) -> Any:
        compressed = gzip.compress(
            json.dumps(sarif, separators=(",", ":"), sort_keys=True).encode("utf-8"),
            mtime=0,
        )
        if len(compressed) > 10 * 1024 * 1024:
            raise GitHubError("SARIF payload exceeds GitHub code-scanning 10 MB gzip limit")
        body = {
            "commit_sha": commit_sha,
            "ref": ref,
            "sarif": base64.b64encode(compressed).decode("ascii"),
        }
        return self.request("POST", f"/repos/{self.repo}/code-scanning/sarifs", body)

    def repository_info(self) -> Mapping[str, Any]:
        value = self.request("GET", f"/repos/{self.repo}")
        if not isinstance(value, Mapping):
            raise GitHubError("unexpected repository response")
        return value


def _labels_for_report(report: Mapping[str, Any], policy: Mapping[str, Any], *, override_blocked: bool = False) -> list[str]:
    labels_cfg = policy.get("labels", {}) if isinstance(policy.get("labels"), Mapping) else {}

    def name(key: str, fallback: str) -> str:
        cfg = labels_cfg.get(key)
        return str(cfg.get("name")) if isinstance(cfg, Mapping) and cfg.get("name") else fallback

    verdict = "blocked" if override_blocked else str(report.get("verdict"))
    labels: list[str] = []
    if verdict == "pass":
        labels.append(name("pass", "sentinel:pass"))
    elif verdict == "blocked":
        labels.append(name("blocked", "sentinel:blocked"))
    else:
        # Scheduled deduplication searches the blocked label; incomplete
        # evidence is still a non-passing gate and must remain discoverable.
        labels.append(name("blocked", "sentinel:blocked"))
        labels.append(name("deep_required", "sentinel:deep-required"))

    risk_score = report.get("risk_score")
    if isinstance(risk_score, (int, float)) and risk_score >= 75:
        labels.append(name("risk_critical", "sentinel:risk-critical"))
    elif isinstance(risk_score, (int, float)) and risk_score >= 50:
        labels.append(name("risk_high", "sentinel:risk-high"))

    findings = [f for f in report.get("findings", []) if isinstance(f, Mapping)]
    if any(f.get("severity") == "P0" for f in findings):
        labels.append(name("p0", "sentinel:P0"))
    if any(f.get("area") in {"performance", "render-ui"} for f in findings):
        labels.append(name("perf_risk", "sentinel:perf-risk"))
    if any(f.get("area") in {"native-security", "ci-security", "parsers"} for f in findings):
        labels.append(name("security_risk", "sentinel:security-risk"))
    metrics = report.get("metrics", {}) if isinstance(report.get("metrics"), Mapping) else {}
    if any(f.get("area") == "dependencies-build" for f in findings) or metrics.get("dependency_changes"):
        labels.append(name("dependency_risk", "sentinel:dependency-risk"))
    test_gap_ids = {
        "flaky-repeated-command",
        "mutation-survivor",
        "mutation-timeout",
        "test-inventory-below-baseline",
        "test-removed",
        "static-test-attributes-shrank",
        "golden-vector-count-shrank",
        "fuzz-target-count-shrank",
        "fuzz-target-removed",
        "high-risk-change-without-test-evidence",
        "test-ignored",
    }
    if (
        any(f.get("id") in test_gap_ids for f in findings)
        or metrics.get("flaky_tests")
        or (isinstance(metrics.get("mutation_missed"), (int, float)) and metrics["mutation_missed"] > 0)
    ):
        labels.append(name("test_gap", "sentinel:test-gap"))
    if verdict == "incomplete":
        labels.append(name("deep_required", "sentinel:deep-required"))
    return list(dict.fromkeys(labels))


def _policy_bool(policy: Mapping[str, Any], key: str, default: bool = True) -> bool:
    bot = policy.get("bot", {}) if isinstance(policy.get("bot"), Mapping) else {}
    value = bot.get(key, default)
    return value if isinstance(value, bool) else default


def _check_annotations_for_report(report: Mapping[str, Any]) -> list[dict[str, Any]]:
    annotations: list[dict[str, Any]] = []
    for finding in report.get("findings", []):
        if not isinstance(finding, Mapping):
            continue
        path = finding.get("path")
        line = finding.get("line")
        if (
            not isinstance(path, str)
            or not path
            or path.startswith(("/", "$"))
            or path.endswith("/")
            or not isinstance(line, int)
            or isinstance(line, bool)
            or line <= 0
        ):
            continue
        severity = str(finding.get("severity") or "")
        level = {
            "P0": "failure",
            "P1": "failure",
            "P2": "warning",
            "P3": "notice",
        }.get(severity, "notice")
        title = redact(
            f"[{finding.get('id') or 'rd-sentinel'}] {finding.get('title') or 'RD Sentinel finding'}",
            limit=255,
        )
        message_parts = [title]
        evidence = str(finding.get("evidence") or "").strip()
        remediation = str(finding.get("remediation") or "").strip()
        if evidence:
            message_parts.append(f"Evidence: {evidence}")
        if remediation:
            message_parts.append(f"Remediation: {remediation}")
        annotations.append(
            {
                "path": path.replace("\\", "/"),
                "start_line": line,
                "end_line": line,
                "annotation_level": level,
                "message": redact("\n\n".join(message_parts), limit=5000),
                "title": title,
            }
        )
    return annotations


def _check_run_payload(
    report: Mapping[str, Any],
    policy: Mapping[str, Any],
    *,
    verdict: str,
    annotations: Sequence[Mapping[str, Any]],
    details_url: str = "",
) -> dict[str, Any]:
    bot_cfg = policy.get("bot", {}) if isinstance(policy.get("bot"), Mapping) else {}
    name = str(
        bot_cfg.get("check_run_name")
        or bot_cfg.get("commit_status_context")
        or "rd-sentinel/gate"
    )
    conclusion = {
        "pass": "success",
        "blocked": "failure",
        "incomplete": "action_required",
    }.get(verdict, "action_required")
    findings = [f for f in report.get("findings", []) if isinstance(f, Mapping)]
    blocking = sum(1 for f in findings if f.get("severity") in {"P0", "P1"})
    checks = [c for c in report.get("checks", []) if isinstance(c, Mapping)]
    failed_checks = sum(1 for c in checks if c.get("status") in {"fail", "error", "planned"})
    summary = "\n".join(
        [
            f"RD Sentinel verdict: **{verdict.upper()}**",
            "",
            f"- Risk: {report.get('risk_score', 0)}/100",
            f"- Complete: {bool(report.get('complete'))}",
            f"- Fingerprint: `{report.get('fingerprint') or 'unknown'}`",
            f"- Findings: {len(findings)} ({blocking} blocking P0/P1)",
            f"- Non-pass/planned checks: {failed_checks} of {len(checks)}",
            "",
            "The fingerprinted JSON report and trusted PR comment remain authoritative; "
            "this Check Run carries line annotations for findings that have a concrete file and line.",
        ]
    )
    payload: dict[str, Any] = {
        "name": name,
        "head_sha": str(report.get("head_sha") or ""),
        "status": "completed",
        "conclusion": conclusion,
        "output": {
            "title": f"RD Sentinel {verdict.upper()}",
            "summary": summary,
            "annotations": [dict(item) for item in annotations],
        },
    }
    fingerprint = str(report.get("fingerprint") or "")
    if fingerprint:
        payload["external_id"] = fingerprint[:255]
    if details_url:
        payload["details_url"] = details_url
    return payload


def _publish_check_run(
    client: GitHubClient,
    report: Mapping[str, Any],
    policy: Mapping[str, Any],
    *,
    verdict: str,
    details_url: str = "",
) -> dict[str, Any]:
    annotations = _check_annotations_for_report(report)
    payload = _check_run_payload(
        report,
        policy,
        verdict=verdict,
        annotations=annotations[:50],
        details_url=details_url,
    )
    created = client.create_check_run(payload)
    result: dict[str, Any] = {"created": created, "annotation_count": len(annotations)}
    check_run_id = created.get("id") if isinstance(created, Mapping) else None
    if isinstance(check_run_id, int) and len(annotations) > 50:
        output = payload.get("output", {}) if isinstance(payload.get("output"), Mapping) else {}
        updates: list[Any] = []
        for start in range(50, len(annotations), 50):
            updates.append(
                client.update_check_run(
                    check_run_id,
                    {
                        "output": {
                            "title": output.get("title", "RD Sentinel"),
                            "summary": output.get("summary", ""),
                            "annotations": annotations[start : start + 50],
                        }
                    },
                )
            )
        result["annotation_updates"] = updates
    return result


def _sarif_ref_for_report(report: Mapping[str, Any]) -> str | None:
    pr_number = report.get("pr_number")
    if isinstance(pr_number, int):
        return f"refs/pull/{pr_number}/head"
    head_ref = str(report.get("head_ref") or "").strip()
    if not head_ref or re.fullmatch(r"[0-9a-fA-F]{40}", head_ref):
        return None
    if head_ref.startswith("refs/"):
        return head_ref
    return f"refs/heads/{head_ref}"


def _ensure_policy_labels(client: GitHubClient, policy: Mapping[str, Any], wanted: Sequence[str]) -> None:
    labels_cfg = policy.get("labels", {}) if isinstance(policy.get("labels"), Mapping) else {}
    by_name: dict[str, Mapping[str, Any]] = {}
    for cfg in labels_cfg.values():
        if isinstance(cfg, Mapping) and cfg.get("name"):
            by_name[str(cfg["name"])] = cfg
    for label in wanted:
        cfg = by_name.get(label, {})
        try:
            client.ensure_label(label, str(cfg.get("color", "ededed")), str(cfg.get("description", "RD Sentinel")))
        except GitHubError:
            # Existing labels may race with another publisher; label creation is
            # best-effort, while the comment/status remains authoritative.
            continue


def _remove_stale_result_labels(
    client: GitHubClient,
    issue_number: int,
    policy: Mapping[str, Any],
    desired: Sequence[str],
) -> None:
    known: set[str] = {"sentinel:risk-high", "sentinel:risk-critical", "sentinel:test-gap"}
    labels_cfg = policy.get("labels", {}) if isinstance(policy.get("labels"), Mapping) else {}
    for cfg in labels_cfg.values():
        if isinstance(cfg, Mapping) and cfg.get("name"):
            known.add(str(cfg["name"]))
    for stale in sorted(known - set(desired)):
        client.remove_label(issue_number, stale)


def verify_pr_report(
    client: GitHubClient, report: Mapping[str, Any], policy: Mapping[str, Any]
) -> tuple[list[str], Mapping[str, Any] | None]:
    errors = validate_report(report)
    pr_number = report.get("pr_number")
    if not isinstance(pr_number, int):
        errors.append("report has no integer pr_number")
        return errors, None
    try:
        pr = client.get_pr(pr_number)
    except GitHubError as exc:
        errors.append(str(exc))
        return errors, None
    head = pr.get("head", {}) if isinstance(pr.get("head"), Mapping) else {}
    base = pr.get("base", {}) if isinstance(pr.get("base"), Mapping) else {}
    if report.get("head_sha") != head.get("sha"):
        errors.append(f"report head {report.get('head_sha')} does not match live PR head {head.get('sha')}")
    if report.get("base_sha") != base.get("sha"):
        errors.append(f"report base {report.get('base_sha')} does not match live PR base {base.get('sha')}")
    files = client.pr_files(pr_number)
    report_files = {
        str(item.get("path"))
        for item in report.get("changed_files", [])
        if isinstance(item, Mapping) and item.get("path")
    }
    # Both lists use post-rename paths for the same base/head pair. A material
    # mismatch means the artifact does not describe the live PR, even if its
    # SHAs were copied correctly. GitHub's PR-files endpoint caps responses
    # at 3,000 files, so only enforce exact equality below that cap.
    live_files = set(files)
    if report_files and len(live_files) < 3000 and len(report_files) < 3000 and live_files != report_files:
        missing = sorted(live_files - report_files)[:20]
        extra = sorted(report_files - live_files)[:20]
        errors.append(
            "report changed-file list does not match live PR: "
            f"missing_from_report={missing}; not_in_live_pr={extra}"
        )
    protected = policy.get("protected_paths", [])
    touched = [path for path in files if match_any(path, protected)]
    labels = {
        str(label.get("name"))
        for label in pr.get("labels", [])
        if isinstance(label, Mapping) and label.get("name")
    }
    approvals = policy.get("approvals", {}) if isinstance(policy.get("approvals"), Mapping) else {}
    policy_label = str(approvals.get("policy_label", "sentinel:policy-approved"))
    if touched and policy_label not in labels and report.get("verdict") == "pass":
        errors.append(
            "protected Sentinel paths changed without approval, but report claims pass: " + ", ".join(touched[:20])
        )
    return errors, pr


def publish_report(
    *,
    client: GitHubClient,
    report: dict[str, Any],
    policy: Mapping[str, Any],
    commit_status: bool = True,
    expected_head_sha: str | None = None,
    workflow_conclusion: str | None = None,
    workflow_run_url: str | None = None,
) -> dict[str, Any]:
    errors = validate_report(report)
    if errors:
        raise GitHubError("invalid RD Sentinel report: " + "; ".join(errors))

    pr_number = report.get("pr_number")
    override_blocked = False
    override_reasons: list[str] = []
    if expected_head_sha and report.get("head_sha") != expected_head_sha:
        override_blocked = True
        override_reasons.append(
            f"report head {report.get('head_sha')} does not match workflow_run head {expected_head_sha}"
        )
    if workflow_conclusion and workflow_conclusion != "success" and report.get("verdict") == "pass":
        override_blocked = True
        override_reasons.append(
            f"upstream workflow concluded {workflow_conclusion}, but the report claims pass"
        )
    if isinstance(pr_number, int):
        verify_errors, _pr = verify_pr_report(client, report, policy)
        material = [e for e in verify_errors if "fingerprint" not in e]
        if material:
            override_blocked = True
            override_reasons.extend(material)

    body_report = dict(report)
    if workflow_run_url:
        # run_url is deliberately outside the fingerprint payload; the trusted
        # workflow_run event, not the artifact, supplies the displayed link.
        body_report["run_url"] = workflow_run_url
    if override_blocked:
        body_report["verdict"] = "blocked"
        findings = list(body_report.get("findings", []))
        findings.append(
            {
                "id": "publisher-report-verification-failed",
                "severity": "P0",
                "area": "sentinel",
                "title": "Trusted publisher rejected the analysis identity/gate claim",
                "evidence": "; ".join(override_reasons),
                "path": None,
                "line": None,
                "remediation": "Rerun RD Sentinel from the current PR head; do not trust the stale or inconsistent artifact.",
                "excepted_by": None,
                "fingerprint": "publisher-verification",
            }
        )
        body_report["findings"] = findings
    body = render_markdown(body_report)
    labels = _labels_for_report(report, policy, override_blocked=override_blocked)
    _ensure_policy_labels(client, policy, labels)

    result: dict[str, Any] = {"labels": labels, "override_blocked": override_blocked}
    if isinstance(pr_number, int):
        result["comment"] = client.upsert_issue_comment(pr_number, body)
        result["label_update"] = client.add_labels(pr_number, labels)
        _remove_stale_result_labels(client, pr_number, policy, labels)
        verdict = "blocked" if override_blocked else str(report.get("verdict"))
        if commit_status and report.get("head_sha"):
            state = {"pass": "success", "blocked": "failure", "incomplete": "error"}.get(verdict, "error")
            bot_cfg = policy.get("bot", {}) if isinstance(policy.get("bot"), Mapping) else {}
            context = str(bot_cfg.get("commit_status_context", "rd-sentinel/gate"))
            try:
                result["commit_status"] = client.create_commit_status(
                    str(report["head_sha"]),
                    state,
                    context=context,
                    description=f"RD Sentinel {verdict}; risk {report.get('risk_score', 0)}/100",
                    target_url=str(body_report.get("run_url") or ""),
                )
            except GitHubError as exc:
                # The analysis workflow check and PR comment remain available
                # even if GitHub cannot attach a classic status to a fork SHA.
                result["commit_status_error"] = str(exc)
        if _policy_bool(policy, "create_check_run", True) and report.get("head_sha"):
            try:
                result["check_run"] = _publish_check_run(
                    client,
                    body_report,
                    policy,
                    verdict=verdict,
                    details_url=str(body_report.get("run_url") or ""),
                )
            except GitHubError as exc:
                # Check annotations are a presentation layer over the same
                # fingerprinted report; never fail the trusted comment/status
                # publication just because Checks API rejects a fork/head/ref.
                result["check_run_error"] = str(exc)
        if _policy_bool(policy, "upload_sarif_to_code_scanning", True) and report.get("head_sha"):
            sarif_ref = _sarif_ref_for_report(body_report)
            if sarif_ref and body_report.get("findings"):
                try:
                    result["code_scanning_sarif"] = client.upload_code_scanning_sarif(
                        sarif=render_sarif(body_report),
                        commit_sha=str(report["head_sha"]),
                        ref=sarif_ref,
                    )
                except GitHubError as exc:
                    result["code_scanning_sarif_error"] = str(exc)
        return result

    campaign = str(report.get("campaign") or "scheduled")
    marker = f"rd-sentinel-campaign:{campaign}"
    issues = client.paged(f"/repos/{client.repo}/issues?state=open&labels={urllib.parse.quote('sentinel:blocked', safe='')}")
    matching = [
        issue
        for issue in issues
        if isinstance(issue, Mapping) and marker in str(issue.get("body", "")) and "pull_request" not in issue
    ]
    verdict = "blocked" if override_blocked else str(report.get("verdict"))
    issue_body = body + f"\n<!-- {marker} -->\n"
    if verdict == "pass" and report.get("complete"):
        closed: list[Any] = []
        for issue in matching:
            number = issue.get("number")
            if isinstance(number, int):
                client.comment(number, "✅ A later complete RD Sentinel campaign passed; closing this regression issue. See the run linked in the latest report artifact.")
                closed.append(client.close_issue(number))
        result["closed_issues"] = closed
        return result

    if matching:
        number = matching[0].get("number")
        if isinstance(number, int):
            result["issue_comment"] = client.upsert_issue_comment(number, issue_body)
            result["issue_number"] = number
            result["label_update"] = client.add_labels(number, labels)
            _remove_stale_result_labels(client, number, policy, labels)
            return result

    title = f"[RD Sentinel] {campaign} regression at {str(report.get('head_sha') or '')[:8]}"
    created = client.create_issue(title, issue_body, labels)
    result["issue"] = created
    if isinstance(created, Mapping):
        result["issue_number"] = created.get("number")
    return result


def _latest_report_status(client: GitHubClient, issue_number: int) -> tuple[str, bool]:
    try:
        comments = client.paged(f"/repos/{client.repo}/issues/{issue_number}/comments")
    except GitHubError as exc:
        return f"🛡️ I could not read prior RD Sentinel reports on this thread: {exc}", False
    for comment in reversed(comments):
        if not isinstance(comment, Mapping):
            continue
        user = comment.get("user", {}) if isinstance(comment.get("user"), Mapping) else {}
        if str(user.get("login", "")) != "github-actions[bot]":
            continue
        body = str(comment.get("body", ""))
        if REPORT_MARKER not in body:
            continue
        verdict_line = next(
            (
                line.strip()
                for line in body.splitlines()
                if "**PASS**" in line or "**BLOCKED**" in line or "**INCOMPLETE**" in line
            ),
            "Latest RD Sentinel report found.",
        )
        fingerprint_match = re.search(r"rd-sentinel-fingerprint:([0-9a-fA-F]{8,64})", body)
        fingerprint = fingerprint_match.group(1)[:16] if fingerprint_match else "unknown"
        url = str(comment.get("html_url") or "")
        link = f"[open the trusted report comment]({url})" if url else "see the pinned Sentinel comment above"
        return (
            f"🛡️ Latest RD Sentinel status: {verdict_line}\n\n"
            f"Fingerprint: `{fingerprint}` — {link}. "
            "Use `/rd-sentinel rerun` for fresh evidence at the current head."
        ), True
    return (
        "🛡️ No trusted RD Sentinel report comment exists on this thread yet. "
        "Use `/rd-sentinel rerun` or `/rd-sentinel deep` to request evidence."
    ), False


def execute_comment_command(
    *,
    client: GitHubClient,
    event: Mapping[str, Any],
    policy: Mapping[str, Any],
) -> dict[str, Any]:
    comment_obj = event.get("comment", {}) if isinstance(event.get("comment"), Mapping) else {}
    issue = event.get("issue", {}) if isinstance(event.get("issue"), Mapping) else {}
    issue_number = issue.get("number")
    if not isinstance(issue_number, int):
        raise GitHubError("comment event has no issue number")
    command = parse_command(str(comment_obj.get("body", "")))
    if command is None:
        return {"ignored": True, "reason": "no RD Sentinel command"}
    if not command_authorized(event, policy):
        body = "🔒 RD Sentinel commands are restricted to repository OWNER/MEMBER/COLLABORATOR associations."
        result = client.comment(issue_number, body)
        return {"authorized": False, "comment": result}
    if command.action == "help":
        return {"authorized": True, "comment": client.comment(issue_number, help_text())}
    if command.action == "invalid":
        return {"authorized": True, "comment": client.comment(issue_number, "⚠️ I could not parse that RD Sentinel command.\n\n" + help_text())}
    if command.action == "status":
        status_body, found = _latest_report_status(client, issue_number)
        return {
            "authorized": True,
            "latest_report_found": found,
            "comment": client.comment(issue_number, status_body),
        }

    is_pr = isinstance(issue.get("pull_request"), Mapping)
    repo_info = client.repository_info()
    default_branch = str(repo_info.get("default_branch") or "main")
    workflow = "rd-sentinel.yml" if is_pr else "rd-sentinel-deep.yml"
    # Input names are matched exactly to each workflow. In particular, the
    # workflows spell the duration input Fuzz_SECONDS, and the deep workflow
    # has no PR input; sending unknown inputs can make GitHub reject a
    # dispatch even when the command itself was authorized.
    if is_pr:
        inputs = {
            "profile": command.profile,
            "campaign": f"command-{command.profile}",
            "pr_number": str(issue_number),
            "Fuzz_SECONDS": str(command.fuzz_seconds if command.fuzz_seconds is not None else ""),
            "mutation_minutes": str(command.mutation_minutes if command.mutation_minutes is not None else ""),
            "baseline_proposal": "true" if command.action == "baseline-propose" else "false",
        }
    else:
        inputs = {
            "profile": "nightly" if command.profile == "auto" else command.profile,
            "campaign": "manual",
            "Fuzz_SECONDS": str(command.fuzz_seconds if command.fuzz_seconds is not None else ""),
            "mutation_minutes": str(command.mutation_minutes if command.mutation_minutes is not None else ""),
            "baseline_proposal": "true" if command.action == "baseline-propose" else "false",
        }
    dispatch = client.dispatch_workflow(workflow, default_branch, inputs)
    message = (
        f"🛡️ RD Sentinel `{command.profile}` campaign dispatched"
        + (f" with fuzz duration {command.fuzz_seconds}s/target." if command.fuzz_seconds is not None else ".")
        + " The trusted publisher will update the fingerprinted report when it completes."
    )
    if command.mutation_minutes is not None:
        message += f"\n\nChanged-code mutation budget: {command.mutation_minutes} minutes."
    if command.action == "baseline-propose":
        message += "\n\nThis produces evidence only; a human must open a separately reviewed baseline PR."
    comment = client.comment(issue_number, message)
    return {"authorized": True, "dispatch": dispatch, "comment": comment, "workflow": workflow, "inputs": inputs}
