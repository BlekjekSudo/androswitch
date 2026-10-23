"""Command-line entry point for RD Sentinel."""

from __future__ import annotations

import argparse
import json
import os
import sys
import traceback
from pathlib import Path
from typing import Any, Mapping, MutableMapping, Sequence

from .audit import AuditContext, build_static_report, resolve_rev
from .campaigns import CampaignOptions, CampaignRunner
from .github_api import GitHubClient, command_authorized, execute_comment_command, parse_command, publish_report
from .model import (
    CHECK_RANK,
    CheckResult,
    SEVERITY_RANK,
    finalize_report,
    load_json,
    render_markdown,
    render_sarif,
    sha256_file,
    validate_report,
)


def _default_repo(value: str | None) -> Path:
    return Path(value or ".").resolve()


def _resolve_under_repo(repo: Path, value: str | None, default: str) -> Path:
    raw = Path(value or default)
    return raw if raw.is_absolute() else repo / raw


def _load_policy_baseline(args: argparse.Namespace, repo: Path) -> tuple[dict[str, Any], dict[str, Any], Path, Path]:
    policy_path = _resolve_under_repo(repo, getattr(args, "policy", None), ".github/rd-sentinel/policy.json")
    baseline_path = _resolve_under_repo(repo, getattr(args, "baseline", None), ".github/rd-sentinel/baseline.json")
    return load_json(policy_path), load_json(baseline_path), policy_path, baseline_path


def _parse_labels(value: str | None) -> list[str]:
    if not value:
        return []
    return [item.strip() for item in value.split(",") if item.strip()]


def _event_data(path: str | None) -> dict[str, Any]:
    if not path:
        return {}
    value = json.loads(Path(path).read_text(encoding="utf-8"))
    return value if isinstance(value, dict) else {}


def _context_from_args(args: argparse.Namespace, repo: Path) -> tuple[AuditContext, str, str]:
    event = _event_data(getattr(args, "event", None))
    pr = event.get("pull_request", {}) if isinstance(event.get("pull_request"), Mapping) else {}
    merge_group = event.get("merge_group", {}) if isinstance(event.get("merge_group"), Mapping) else {}
    sender = event.get("sender", {}) if isinstance(event.get("sender"), Mapping) else {}

    labels = _parse_labels(getattr(args, "labels", None))
    if not labels and isinstance(pr.get("labels"), list):
        labels = [str(item.get("name")) for item in pr["labels"] if isinstance(item, Mapping) and item.get("name")]

    def nested(mapping: Mapping[str, Any], key: str, field: str) -> str:
        child = mapping.get(key, {})
        return str(child.get(field, "")) if isinstance(child, Mapping) else ""

    base_ref = (
        getattr(args, "base", None)
        or nested(pr, "base", "sha")
        or str(merge_group.get("base_sha") or "")
    )
    head_ref = (
        getattr(args, "head", None)
        or nested(pr, "head", "sha")
        or str(merge_group.get("head_sha") or "")
        or "HEAD"
    )
    explicit_event_name = getattr(args, "event_name", None)
    if explicit_event_name:
        event_name = explicit_event_name
    elif merge_group:
        # An explicit merge_group payload is stronger evidence than ambient
        # GITHUB_EVENT_NAME: the same code is unit-tested/reproduced while an
        # outer pull_request workflow is running.
        event_name = "merge_group"
    else:
        event_name = os.environ.get("GITHUB_EVENT_NAME", "local")
    if not base_ref:
        if event_name == "schedule":
            parent = resolve_rev(repo, "HEAD^")
            base_ref = parent or "HEAD"
        else:
            base_ref = "origin/main"

    repository = getattr(args, "repository", None) or os.environ.get("GITHUB_REPOSITORY", "")
    run_url = getattr(args, "run_url", None) or ""
    if not run_url and repository and os.environ.get("GITHUB_RUN_ID"):
        server = os.environ.get("GITHUB_SERVER_URL", "https://github.com").rstrip("/")
        run_url = f"{server}/{repository}/actions/runs/{os.environ['GITHUB_RUN_ID']}"

    pr_number = getattr(args, "pr_number", None)
    if pr_number is None and isinstance(pr.get("number"), int):
        pr_number = int(pr["number"])

    context = AuditContext(
        event_name=str(event_name),
        campaign=str(getattr(args, "campaign", None) or event_name),
        profile=str(getattr(args, "profile", "auto") or "auto"),
        actor=str(getattr(args, "actor", None) or sender.get("login") or nested(pr, "user", "login")),
        author=str(getattr(args, "author", None) or nested(pr, "user", "login")),
        head_ref=str(
            getattr(args, "head_ref", None)
            or nested(pr, "head", "ref")
            or str(merge_group.get("head_ref") or "")
        ),
        labels=labels,
        pr_number=pr_number,
        run_url=str(run_url),
        repository=str(repository),
    )
    return context, str(base_ref), str(head_ref)


def _dedupe_findings(report: MutableMapping[str, Any]) -> None:
    by_key: dict[str, Mapping[str, Any]] = {}
    for finding in report.get("findings", []):
        if not isinstance(finding, Mapping):
            continue
        key = str(finding.get("fingerprint") or finding.get("id"))
        existing = by_key.get(key)
        if existing is None or SEVERITY_RANK.get(str(finding.get("severity")), 99) < SEVERITY_RANK.get(str(existing.get("severity")), 99):
            by_key[key] = finding
    report["findings"] = list(by_key.values())


def _write_report(args: argparse.Namespace, report: Mapping[str, Any], repo: Path) -> tuple[Path, Path, Path]:
    output_dir = _resolve_under_repo(repo, getattr(args, "output_dir", None), "target/rd-sentinel")
    output_dir.mkdir(parents=True, exist_ok=True)
    json_path = _resolve_under_repo(repo, getattr(args, "json_out", None), str(output_dir / "rd-sentinel-report.json"))
    markdown_path = _resolve_under_repo(repo, getattr(args, "markdown_out", None), str(output_dir / "rd-sentinel-report.md"))
    sarif_path = _resolve_under_repo(repo, getattr(args, "sarif_out", None), str(output_dir / "rd-sentinel-report.sarif"))
    json_path.parent.mkdir(parents=True, exist_ok=True)
    markdown_path.parent.mkdir(parents=True, exist_ok=True)
    sarif_path.parent.mkdir(parents=True, exist_ok=True)
    rendered = render_markdown(report)
    json_path.write_text(json.dumps(report, indent=2, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")
    markdown_path.write_text(rendered, encoding="utf-8")
    sarif_path.write_text(
        json.dumps(render_sarif(report), indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    summary_path = getattr(args, "summary_out", None) or os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with Path(summary_path).open("a", encoding="utf-8") as fh:
            fh.write(rendered)
    github_output = getattr(args, "github_output", None) or os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as fh:
            fh.write(f"verdict={report.get('verdict')}\n")
            fh.write(f"fingerprint={report.get('fingerprint')}\n")
            fh.write(f"report_json={json_path}\n")
            fh.write(f"report_markdown={markdown_path}\n")
            fh.write(f"report_sarif={sarif_path}\n")
    return json_path, markdown_path, sarif_path


def _exit_for_verdict(verdict: str) -> int:
    return {"pass": 0, "blocked": 1, "incomplete": 3}.get(verdict, 2)


def cmd_audit(args: argparse.Namespace) -> int:
    repo = _default_repo(args.repo)
    policy, baseline, _, _ = _load_policy_baseline(args, repo)
    context, base_ref, head_ref = _context_from_args(args, repo)
    report: MutableMapping[str, Any] = build_static_report(
        repo=repo,
        base_ref=base_ref,
        head_ref=head_ref,
        policy=policy,
        baseline=baseline,
        context=context,
    )
    planned = [str(item.get("component")) for item in report.get("plan", []) if isinstance(item, Mapping)]
    components = list(dict.fromkeys(args.component or planned))
    report["requested_components"] = components
    output_dir = _resolve_under_repo(repo, args.output_dir, "target/rd-sentinel")
    options = CampaignOptions(
        execute=bool(args.execute),
        strict_skips=bool(args.strict_skips),
        components=components,
        apk_paths=[Path(p) for p in (args.apk or [])],
        fuzz_seconds=args.Fuzz_SECONDS,
        fuzz_workers=args.fuzz_workers,
        repeat=args.repeat,
        perf_runs=args.perf_runs,
        cold_runs=args.cold_runs,
        mutation_minutes=args.mutation_minutes,
        allow_network=bool(args.allow_network),
        output_dir=output_dir,
    )
    CampaignRunner(repo=repo, report=report, policy=policy, baseline=baseline, options=options).run(components)

    if args.baseline_proposal:
        metrics = report.get("metrics", {}) if isinstance(report.get("metrics"), Mapping) else {}
        proposal: dict[str, Any] = {
            "source_head": report.get("head_sha"),
            "contracts": {
                "rust_version": metrics.get("rust_version"),
                "cargo_lock_version": metrics.get("cargo_lock_version"),
                "floor_min_sdk": metrics.get("floor_min_sdk"),
                "mcp_tool_count": metrics.get("mcp_tool_count"),
                "golden_vector_count": metrics.get("golden_vector_count"),
                "fuzz_target_count": metrics.get("fuzz_target_count"),
                "minimum_static_test_attributes": metrics.get("static_test_attributes"),
                "startup_budget_emulated_ms_max": metrics.get("startup_budget_emulated_ms"),
            },
            "release_binary_bytes": metrics.get("release_binary_bytes"),
            "note": "Evidence only. A maintainer must turn this into a separately reviewed baseline PR.",
        }
        corpus: list[dict[str, Any]] = []
        for entry in baseline.get("corpus", []) if isinstance(baseline.get("corpus"), list) else []:
            if isinstance(entry, Mapping) and entry.get("path"):
                target = repo / str(entry["path"])
                corpus.append({"id": entry.get("id"), "path": entry.get("path"), "present": target.is_file(), "sha256": sha256_file(target) if target.is_file() else None})
        proposal["corpus"] = corpus
        report["baseline_proposal"] = proposal

    _dedupe_findings(report)
    finalize_report(report, policy)
    json_path, markdown_path, sarif_path = _write_report(args, report, repo)
    print(f"RD Sentinel verdict={report['verdict']} fingerprint={report['fingerprint']}")
    print(f"JSON: {json_path}")
    print(f"Markdown: {markdown_path}")
    print(f"SARIF: {sarif_path}")
    return _exit_for_verdict(str(report["verdict"]))


def cmd_plan(args: argparse.Namespace) -> int:
    repo = _default_repo(args.repo)
    policy, baseline, _, _ = _load_policy_baseline(args, repo)
    context, base_ref, head_ref = _context_from_args(args, repo)
    report = build_static_report(repo=repo, base_ref=base_ref, head_ref=head_ref, policy=policy, baseline=baseline, context=context)
    output = {
        "base_sha": report.get("base_sha"),
        "head_sha": report.get("head_sha"),
        "areas": report.get("areas"),
        "risk_score": report.get("risk_score"),
        "risk_factors": report.get("risk_factors"),
        "plan": report.get("plan"),
        "findings": report.get("findings"),
        "metrics": report.get("metrics"),
    }
    text = json.dumps(output, indent=2, ensure_ascii=False, sort_keys=True)
    if args.out:
        out_path = Path(args.out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 1 if any(f.get("severity") in {"P0", "P1"} for f in output["findings"]) else 0


def cmd_render(args: argparse.Namespace) -> int:
    report = load_json(Path(args.report))
    errors = validate_report(report, require_fingerprint=not args.allow_unverified)
    if errors:
        print("invalid report: " + "; ".join(errors), file=sys.stderr)
        return 2
    text = render_markdown(report)
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
    else:
        print(text, end="")
    return 0


def cmd_sarif(args: argparse.Namespace) -> int:
    report = load_json(Path(args.report))
    errors = validate_report(report, require_fingerprint=not args.allow_unverified)
    if errors:
        print("invalid report: " + "; ".join(errors), file=sys.stderr)
        return 2
    text = json.dumps(render_sarif(report), indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
    else:
        print(text, end="")
    return 0


def cmd_validate(args: argparse.Namespace) -> int:
    report = load_json(Path(args.report))
    errors = validate_report(report)
    if errors:
        print("invalid report: " + "; ".join(errors), file=sys.stderr)
        return 2
    print(f"valid RD Sentinel report: verdict={report.get('verdict')} fingerprint={report.get('fingerprint')}")
    if args.require_pass and report.get("verdict") != "pass":
        return _exit_for_verdict(str(report.get("verdict")))
    return 0


def _preferred_merged_check(
    old: Mapping[str, Any] | None, new: Mapping[str, Any]
) -> Mapping[str, Any]:
    """Resolve duplicate checks from partial reports without losing evidence.

    Every deep partial contains the same static plan. A merely *planned* or
    *skipped* placeholder in one partial must never overwrite an actually
    executed result from another partial; among executed duplicates, the
    worst result wins so merging remains fail-closed.
    """

    if old is None:
        return new
    actual = {"pass", "warn", "fail", "error"}
    old_status = str(old.get("status"))
    new_status = str(new.get("status"))
    if old_status in actual and new_status not in actual:
        return old
    if new_status in actual and old_status not in actual:
        return new
    return new if CHECK_RANK.get(new_status, 0) >= CHECK_RANK.get(old_status, 0) else old


def cmd_merge(args: argparse.Namespace) -> int:
    repo = _default_repo(args.repo)
    policy, _baseline, _, _ = _load_policy_baseline(args, repo)
    reports: list[dict[str, Any]] = []
    for raw in args.reports:
        path = Path(raw)
        if path.is_dir():
            candidates = sorted(path.rglob("rd-sentinel-report.json"))
        else:
            candidates = [path]
        for candidate in candidates:
            if candidate.is_file():
                reports.append(load_json(candidate))
    if not reports:
        print("no reports found to merge", file=sys.stderr)
        return 2
    for report in reports:
        errors = validate_report(report)
        if errors:
            print("invalid partial report: " + "; ".join(errors), file=sys.stderr)
            return 2
    identities = {(r.get("base_sha"), r.get("head_sha")) for r in reports}
    if len(identities) != 1:
        print(f"partial reports do not share one base/head identity: {identities}", file=sys.stderr)
        return 2

    merged: dict[str, Any] = dict(reports[0])
    checks: dict[str, Mapping[str, Any]] = {}
    findings: dict[str, Mapping[str, Any]] = {}
    metrics: dict[str, Any] = {}
    plans: dict[str, Mapping[str, Any]] = {}
    areas: set[str] = set()
    executed: set[str] = set()
    components: set[str] = set()
    for report in reports:
        for check in report.get("checks", []):
            if not isinstance(check, Mapping):
                continue
            key = str(check.get("id"))
            checks[key] = _preferred_merged_check(checks.get(key), check)
        for finding in report.get("findings", []):
            if not isinstance(finding, Mapping):
                continue
            key = str(finding.get("fingerprint") or finding.get("id"))
            old = findings.get(key)
            if old is None or SEVERITY_RANK.get(str(finding.get("severity")), 99) < SEVERITY_RANK.get(str(old.get("severity")), 99):
                findings[key] = finding
        if isinstance(report.get("metrics"), Mapping):
            for key, value in report["metrics"].items():
                if value is not None:
                    metrics[str(key)] = value
        for plan in report.get("plan", []):
            if isinstance(plan, Mapping) and plan.get("component"):
                plans[str(plan["component"])] = plan
        areas.update(str(x) for x in report.get("areas", []) if x)
        executed.update(str(x) for x in report.get("executed_components", []) if x)
        components.update(str(x) for x in report.get("requested_components", []) if x)

    covered_components = set(executed)
    covered_components.update(
        str(check.get("component"))
        for check in checks.values()
        if check.get("component") and check.get("status") != "planned"
    )
    for component, plan in sorted(plans.items()):
        if plan.get("required", True) and component not in covered_components:
            planned_id = f"planned-{component}"
            checks.setdefault(
                planned_id,
                CheckResult(
                    id=planned_id,
                    title=f"Planned campaign: {component}",
                    status="planned",
                    required=True,
                    component=component,
                    summary="No partial report executed this planned required component; merged evidence is incomplete.",
                ).to_dict(),
            )

    merged["checks"] = list(checks.values())
    merged["findings"] = list(findings.values())
    merged["metrics"] = metrics
    merged["plan"] = [plans[k] for k in sorted(plans)]
    merged["areas"] = sorted(areas)
    merged["executed_components"] = sorted(executed)
    merged["requested_components"] = sorted(components)
    merged["risk_score"] = max(int(r.get("risk_score") or 0) for r in reports)
    merged["risk_factors"] = max(
        reports,
        key=lambda r: int(r.get("risk_score") or 0),
    ).get("risk_factors", [])
    merged["strict_skips"] = any(bool(r.get("strict_skips")) for r in reports)
    merged["merged_report_fingerprints"] = [r.get("fingerprint") for r in reports]
    finalize_report(merged, policy)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(merged, indent=2, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")
    md_out = Path(args.markdown_out) if args.markdown_out else out.with_suffix(".md")
    md_out.write_text(render_markdown(merged), encoding="utf-8")
    sarif_value = getattr(args, "sarif_out", None)
    sarif_out = Path(sarif_value) if sarif_value else out.with_suffix(".sarif")
    sarif_out.write_text(
        json.dumps(render_sarif(merged), indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"merged RD Sentinel verdict={merged['verdict']} fingerprint={merged['fingerprint']} -> {out}")
    return _exit_for_verdict(str(merged["verdict"]))


def cmd_publish(args: argparse.Namespace) -> int:
    repo = _default_repo(args.repo)
    policy, _baseline, _, _ = _load_policy_baseline(args, repo)
    report = load_json(Path(args.report))
    repository = args.repository or report.get("repository") or os.environ.get("GITHUB_REPOSITORY", "")
    if not repository:
        print("--repository owner/name is required", file=sys.stderr)
        return 2
    client = GitHubClient(str(repository), dry_run=bool(args.dry_run))
    result = publish_report(
        client=client,
        report=report,
        policy=policy,
        commit_status=not args.no_commit_status,
        expected_head_sha=args.expected_head_sha,
        workflow_conclusion=args.workflow_conclusion,
        workflow_run_url=args.run_url,
    )
    printable = {
        "dry_run": bool(args.dry_run),
        "labels": result.get("labels"),
        "override_blocked": result.get("override_blocked"),
        "issue_number": result.get("issue_number"),
        "requests": client.requests if args.dry_run else len(client.requests),
    }
    print(json.dumps(printable, indent=2, ensure_ascii=False, sort_keys=True))
    return 0


def cmd_command(args: argparse.Namespace) -> int:
    repo = _default_repo(args.repo)
    policy, _baseline, _, _ = _load_policy_baseline(args, repo)
    event = _event_data(args.event)
    comment = event.get("comment", {}) if isinstance(event.get("comment"), Mapping) else {}
    parsed = parse_command(str(comment.get("body", "")))
    preview = {
        "command": vars(parsed) if parsed else None,
        "authorized": command_authorized(event, policy),
    }
    if not args.execute:
        print(json.dumps(preview, indent=2, sort_keys=True))
        return 0
    repository = args.repository or os.environ.get("GITHUB_REPOSITORY", "")
    if not repository:
        print("--repository owner/name is required", file=sys.stderr)
        return 2
    client = GitHubClient(str(repository), dry_run=bool(args.dry_run))
    result = execute_comment_command(client=client, event=event, policy=policy)
    print(json.dumps({"preview": preview, "result": result, "requests": client.requests if args.dry_run else len(client.requests)}, indent=2, ensure_ascii=False, sort_keys=True))
    return 0


def cmd_baseline_proposal(args: argparse.Namespace) -> int:
    args.baseline_proposal = True
    return cmd_audit(args)


def _add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--repo", default=".", help="Candidate repository path")
    parser.add_argument("--policy", default=None, help="Policy JSON path (relative to --repo unless absolute)")
    parser.add_argument("--baseline", default=None, help="Baseline JSON path (relative to --repo unless absolute)")


def _non_negative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("value must be >= 0")
    return parsed


def _positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("value must be >= 1")
    return parsed


def _add_audit_args(parser: argparse.ArgumentParser) -> None:
    _add_common(parser)
    parser.add_argument("--base", default=None, help="Trusted base ref/SHA (default: event base or origin/main)")
    parser.add_argument("--head", default=None, help="Head ref/SHA, or WORKTREE for a dirty local tree")
    parser.add_argument("--profile", default="auto", choices=["auto", "plan", "static", "deep", "nightly", "weekly", "perf", "fuzz", "deps", "determinism", "mutants", "soak"])
    parser.add_argument("--campaign", default=None)
    parser.add_argument("--event", default=None, help="GitHub event JSON path")
    parser.add_argument("--event-name", default=None)
    parser.add_argument("--pr-number", type=int, default=None)
    parser.add_argument("--actor", default=None)
    parser.add_argument("--author", default=None)
    parser.add_argument("--head-ref", default=None)
    parser.add_argument("--labels", default=None, help="Comma-separated PR labels")
    parser.add_argument("--repository", default=None)
    parser.add_argument("--run-url", default=None)
    parser.add_argument("--component", action="append", default=[], help="Component override; repeatable")
    parser.add_argument("--execute", action="store_true", help="Execute planned campaigns; without this they are marked planned/incomplete")
    parser.add_argument("--strict-skips", action="store_true", help="Treat a skipped required campaign as blocked, not merely incomplete")
    parser.add_argument("--apk", action="append", default=[], help="APK path override; repeatable")
    parser.add_argument("--Fuzz_SECONDS", type=_non_negative_int, default=None)
    parser.add_argument("--fuzz-workers", type=_positive_int, default=None)
    parser.add_argument("--repeat", type=_positive_int, default=None)
    parser.add_argument("--perf-runs", type=_positive_int, default=None)
    parser.add_argument("--cold-runs", type=_positive_int, default=None)
    parser.add_argument("--mutation-minutes", type=_positive_int, default=None)
    parser.add_argument("--allow-network", action="store_true")
    parser.add_argument("--baseline-proposal", action="store_true")
    parser.add_argument("--output-dir", default="target/rd-sentinel")
    parser.add_argument("--json-out", default=None)
    parser.add_argument("--markdown-out", default=None)
    parser.add_argument("--sarif-out", default=None)
    parser.add_argument("--summary-out", default=None)
    parser.add_argument("--github-output", default=None)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="rd-sentinel", description="RD Sentinel anti-regression bot")
    sub = parser.add_subparsers(dest="subcommand", required=True)

    audit = sub.add_parser("audit", help="Audit and optionally execute campaigns")
    _add_audit_args(audit)
    audit.set_defaults(func=cmd_audit)

    baseline = sub.add_parser("baseline-proposal", help="Generate baseline evidence without modifying the baseline")
    _add_audit_args(baseline)
    baseline.set_defaults(func=cmd_baseline_proposal)

    plan = sub.add_parser("plan", help="Print the static risk plan")
    _add_audit_args(plan)
    plan.add_argument("--out", default=None)
    plan.set_defaults(func=cmd_plan)

    render = sub.add_parser("render", help="Render a JSON report as Markdown")
    render.add_argument("--report", required=True)
    render.add_argument("--out", default=None)
    render.add_argument("--allow-unverified", action="store_true")
    render.set_defaults(func=cmd_render)

    sarif = sub.add_parser("sarif", help="Render a JSON report as SARIF 2.1.0")
    sarif.add_argument("--report", required=True)
    sarif.add_argument("--out", default=None)
    sarif.add_argument("--allow-unverified", action="store_true")
    sarif.set_defaults(func=cmd_sarif)

    validate = sub.add_parser("validate", help="Validate report schema/fingerprint")
    validate.add_argument("--report", required=True)
    validate.add_argument("--require-pass", action="store_true")
    validate.set_defaults(func=cmd_validate)

    merge = sub.add_parser("merge", help="Merge partial reports for one base/head")
    _add_common(merge)
    merge.add_argument("reports", nargs="+")
    merge.add_argument("--out", required=True)
    merge.add_argument("--markdown-out", default=None)
    merge.add_argument("--sarif-out", default=None)
    merge.set_defaults(func=cmd_merge)

    publish = sub.add_parser("publish", help="Publish a report through the trusted GitHub publisher")
    _add_common(publish)
    publish.add_argument("--report", required=True)
    publish.add_argument("--repository", default=None)
    publish.add_argument("--expected-head-sha", default=None, help="workflow_run head SHA the report must match")
    publish.add_argument("--workflow-conclusion", default=None, help="upstream workflow_run conclusion")
    publish.add_argument("--run-url", default=None, help="trusted upstream workflow_run URL")
    publish.add_argument("--dry-run", action="store_true")
    publish.add_argument("--no-commit-status", action="store_true")
    publish.set_defaults(func=cmd_publish)

    command = sub.add_parser("command", help="Parse or execute a maintainer issue-comment command")
    _add_common(command)
    command.add_argument("--event", required=True)
    command.add_argument("--repository", default=None)
    command.add_argument("--execute", action="store_true")
    command.add_argument("--dry-run", action="store_true")
    command.set_defaults(func=cmd_command)

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.func(args))
    except FileNotFoundError as exc:
        print(f"RD Sentinel error: file not found: {exc.filename or exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        if os.environ.get("RD_SENTINEL_DEBUG") == "1":
            traceback.print_exc()
        print(f"RD Sentinel error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
