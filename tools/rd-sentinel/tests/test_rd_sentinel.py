from __future__ import annotations

import argparse
import base64
import datetime as dt
import gzip
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from sentinel import campaigns as campaigns_module  # noqa: E402
from sentinel.audit import (  # noqa: E402
    _action_ref_kind,
    _diff_rule_findings,
    _rust_test_lines,
    DiffLine,
    _check_inventory_regression,
    _fuzz_target_names,
    _named_test_findings,
    _path_dependency_escapes,
    _public_api_findings,
    AuditContext,
    ChangedFile,
    build_plan,
    build_static_report,
    classify_semver_change,
    collect_changed_files,
    collect_diff_lines,
    compare_cargo_locks,
    license_expression_allowed,
    named_test_inventory_from_sources,
    public_api_inventory_from_sources,
    risk_assessment,
    workflow_template_injections,
    _workflow_trigger_names,
    workspace_graph_from_manifests,
)
from sentinel.campaigns import (  # noqa: E402
    CampaignOptions,
    CampaignRunner,
    RawResult,
    _mad,
    _noise_floor_ms,
    mutation_check_status,
    parse_cold26_output,
    parse_mutants_outcomes,
    parse_mutants_output,
    summarize_repeated_test_outcomes,
)
from sentinel.cli import _context_from_args, main as cli_main  # noqa: E402
from sentinel.github_api import (  # noqa: E402
    GitHubClient,
    _check_annotations_for_report,
    _check_run_payload,
    _labels_for_report,
    _publish_check_run,
    _sarif_ref_for_report,
    command_authorized,
    execute_comment_command,
    parse_command,
    publish_report,
)
from sentinel.mcp_endurance import run_generic_apk_endurance  # noqa: E402
from sentinel.model import (  # noqa: E402
    Finding,
    apply_exceptions,
    compute_report_fingerprint,
    detect_secret_kind,
    finalize_report,
    match_any,
    redact,
    render_markdown,
    render_sarif,
    validate_baseline,
    validate_policy,
    validate_report,
)

POLICY = {
    "severity": {"blocking": ["P0", "P1"], "max_nonblocking_findings_before_block": 25},
    "approvals": {
        "policy_label": "sentinel:policy-approved",
        "baseline_label": "sentinel:baseline-approved",
        "dependency_major_label": "sentinel:dependency-major-approved",
        "authorized_comment_associations": ["OWNER", "MEMBER", "COLLABORATOR"],
    },
    "protected_paths": [
        ".github/rd-sentinel/**",
        ".github/workflows/rd-sentinel*.yml",
        "tools/rd-sentinel/**",
    ],
    "labels": {
        "pass": {"name": "sentinel:pass", "color": "0e8a16", "description": "passed"},
        "blocked": {"name": "sentinel:blocked", "color": "b60205", "description": "blocked"},
        "deep_required": {"name": "sentinel:deep-required", "color": "fef2c0", "description": "deep required"},
        "risk_critical": {"name": "sentinel:risk-critical", "color": "b60205", "description": "critical risk"},
    },
    "diff": {
        "large_file_bytes": 1000000,
        "very_large_diff_files": 100,
        "very_large_diff_lines": 10000,
        "binary_extensions_blocking": [".apk"],
        "unsafe_allowed_path_globs": ["crates/rd-jni/**", "crates/rd-ndk/**"],
        "test_path_globs": ["**/tests/**", "tests/**", "golden/**"],
        "adr_path_globs": ["docs/adr/**"],
        "compat_evidence_globs": ["docs/compat-matrix.md", "tests/**"],
        "evidence_harness_globs": ["scripts/cold26.py"],
    },
    "dependencies": {
        "allowed_registry_source_prefix": "registry+https://github.com/rust-lang/crates.io-index",
        "allowed_license_tokens": ["MIT", "Apache-2.0", "Unicode-3.0"],
        "deep_package_name_patterns": ["flate2"],
    },
    "exceptions": [],
}
BASELINE = {"contracts": {}, "corpus": []}


def run(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


class ModelTests(unittest.TestCase):
    def test_fingerprint_ignores_generated_at_and_detects_tamper(self):
        report = {"checks": [], "findings": [], "metrics": {"x": 1}}
        finalize_report(report, POLICY)  # type: ignore[arg-type]
        original = report["fingerprint"]
        report["generated_at"] = "2099-01-01T00:00:00Z"
        self.assertEqual(original, compute_report_fingerprint(report))
        report["metrics"]["x"] = 2
        self.assertIn("fingerprint", "; ".join(validate_report(report)))

    def test_markdown_escapes_table_mentions_and_links(self):
        report = {
            "verdict": "blocked",
            "campaign": "test --> injected",
            "profile": "auto",
            "fingerprint": "abc123",
            "checks": [],
            "findings": [
                {
                    "severity": "P1",
                    "area": "vm-core",
                    "title": "Ping @maintainer | now",
                    "evidence": "a|b [click](https://example.invalid) `code`",
                }
            ],
            "metrics": {},
        }
        markdown = render_markdown(report)
        self.assertIn("@\u200bmaintainer", markdown)
        self.assertIn("a\\|b", markdown)
        self.assertIn("\\[click\\]", markdown)
        self.assertNotIn("test --> injected", markdown.splitlines()[2])

    def test_dot_github_paths_are_not_normalized_away(self):
        self.assertTrue(match_any(".github/rd-sentinel/policy.json", [".github/rd-sentinel/**"]))
        self.assertTrue(match_any("./.github/workflows/rd-sentinel.yml", [".github/workflows/rd-sentinel.yml"]))

    def test_active_exception_downgrades_and_expired_one_blocks(self):
        finding = Finding(
            id="new-thread-local-state",
            severity="P1",
            area="vm-core",
            title="TLS",
            evidence="thread_local!",
            path="crates/rd-vm/src/jregex.rs",
        )
        policy = json.loads(json.dumps(POLICY))
        policy["exceptions"] = [
            {
                "id": "E1",
                "finding_id": "new-thread-local-state",
                "path_glob": "crates/rd-vm/src/jregex.rs",
                "owner": "vm",
                "reason": "pure cache",
                "expires": "2099-01-01",
            }
        ]
        out = apply_exceptions([finding], policy, today=dt.date(2026, 10, 22))
        self.assertEqual("P3", out[0].severity)
        self.assertEqual("E1", out[0].excepted_by)

        expired = json.loads(json.dumps(policy))
        expired["exceptions"][0]["expires"] = "2020-01-01"
        out2 = apply_exceptions([], expired, today=dt.date(2026, 10, 22))
        self.assertEqual("expired-policy-exception", out2[0].id)
        self.assertEqual("P1", out2[0].severity)

    def test_secret_detector_names_high_confidence_shapes_without_echoing_values(self):
        github_token = "gh" + "p_" + "A" * 30
        aws_key = "AK" + "IA" + "B" * 16
        self.assertIsNotNone(detect_secret_kind(github_token))
        self.assertIsNotNone(detect_secret_kind(aws_key))
        self.assertIsNotNone(detect_secret_kind("-----BEGIN " + "PRIVATE KEY-----"))
        self.assertIsNone(detect_secret_kind("token_budget = 42"))
        redacted = redact(f"before {github_token} after")
        self.assertNotIn(github_token, redacted)
        self.assertIn("[REDACTED]", redacted)

    def test_policy_validation_catches_weakening_and_overbroad_exceptions(self):
        policy = json.loads(json.dumps(POLICY))
        policy["severity"]["blocking"] = ["P0"]
        policy["exceptions"] = [
            {
                "id": "E-dup",
                "finding_id": "x",
                "path_glob": "crates/**",
                "owner": "team",
                "reason": "too broad",
                "expires": "2099-01-01",
            },
            {
                "id": "E-dup",
                "finding_id": "y",
                "path_glob": "crates/rd-vm/src/lib.rs",
                "owner": "team",
                "reason": "duplicate",
                "expires": "2099-01-01",
            },
        ]
        ids = {finding.id for finding in validate_policy(policy)}
        self.assertIn("blocking-severities-weakened", ids)
        self.assertIn("duplicate-policy-exception", ids)
        bad_baseline = {
            "contracts": {"mcp_tool_count": "47"},
            "corpus": [{"id": "x", "path": "a.apk", "path_env": "X", "sha256": "nope"}],
        }
        self.assertTrue(validate_baseline(bad_baseline))

    def test_sarif_carries_fingerprints_locations_and_severity_levels(self):
        report = {
            "bot_version": "1.1.0",
            "repository": "owner/repo",
            "verdict": "blocked",
            "campaign": "test",
            "profile": "auto",
            "checks": [],
            "metrics": {},
            "findings": [
                Finding(
                    id="example-finding",
                    severity="P1",
                    area="tests",
                    title="Example blocked",
                    evidence="evidence",
                    path="crates/rd-vm/src/lib.rs",
                    line=7,
                ).to_dict(),
                Finding(
                    id="directory-finding",
                    severity="P2",
                    area="tests",
                    title="Directory-scoped finding",
                    evidence="evidence",
                    path="crates/rd-vm/src",
                ).to_dict(),
            ],
        }
        finalize_report(report, POLICY)  # type: ignore[arg-type]
        sarif = render_sarif(report)
        self.assertEqual("2.1.0", sarif["version"])
        run = sarif["runs"][0]
        self.assertEqual("example-finding", run["results"][0]["ruleId"])
        self.assertEqual("error", run["results"][0]["level"])
        self.assertEqual(
            7,
            run["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
        )
        self.assertIn("rdSentinelFingerprint", run["results"][0]["partialFingerprints"])
        self.assertNotIn("locations", run["results"][1])


class AuditUnitTests(unittest.TestCase):
    def test_semver_and_lock_diff(self):
        self.assertEqual("major", classify_semver_change("0.9.0", "0.10.0"))
        self.assertEqual("minor", classify_semver_change("1.2.3", "1.3.0"))
        self.assertEqual("patch", classify_semver_change("1.2.3", "1.2.4"))
        old = 'version = 3\n[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n'
        new = 'version = 3\n[[package]]\nname="serde"\nversion="2.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n'
        changes, metrics = compare_cargo_locks(old, new)
        self.assertEqual("major", changes[0]["kind"])
        self.assertEqual(1, metrics["dependency_major_changes"])

    def test_license_expression(self):
        allowed = ["MIT", "Apache-2.0", "Unicode-3.0"]
        self.assertTrue(license_expression_allowed("MIT OR Apache-2.0", allowed))
        self.assertTrue(license_expression_allowed("(MIT OR Apache-2.0) AND Unicode-3.0", allowed))
        self.assertTrue(license_expression_allowed("MIT/Apache-2.0", allowed))
        self.assertFalse(license_expression_allowed("GPL-3.0-only", allowed))

    def test_workspace_path_dependencies_stay_inside_repo(self):
        self.assertFalse(_path_dependency_escapes("crates/rd-cli/Cargo.toml", "../rd-vm"))
        self.assertTrue(_path_dependency_escapes("crates/rd-cli/Cargo.toml", "../../../outside"))
        self.assertTrue(_path_dependency_escapes("Cargo.toml", "/tmp/crate"))

    def test_plan_adds_fuzz_for_parsers_and_ab_for_high_risk(self):
        files = [ChangedFile("M", "crates/rd-apk/src/zip.rs")]
        plan = build_plan(
            profile="auto",
            areas=["parsers"],
            files=files,
            labels=[],
            risk=90,
            dependency_bot=False,
        )
        components = {p["component"] for p in plan}
        self.assertIn("fuzz", components)
        self.assertIn("compare", components)
        self.assertIn("real-apk", components)

    def test_risk_assessment_is_explainable_and_counts_downstream_packages(self):
        files = [ChangedFile("M", "crates/rd-dex/src/lib.rs"), ChangedFile("M", "Cargo.lock")]
        without_downstream, base_factors = risk_assessment(files, ["parsers"], 10, 2)
        with_downstream, factors = risk_assessment(
            files, ["parsers"], 10, 2, downstream_packages=3
        )
        self.assertGreater(with_downstream, without_downstream)
        self.assertTrue(any(f["id"] == "downstream-blast-radius" for f in factors))
        self.assertTrue(any(f["id"] == "lockfile" for f in base_factors))

    def test_workspace_graph_from_manifests_keeps_workspace_edges_only(self):
        manifests = {
            "crates/rd-cli/Cargo.toml": {
                "package": {"name": "rd-cli"},
                "dependencies": {"rd-vm": {"path": "../rd-vm"}, "serde": "1"},
            },
            "crates/rd-vm/Cargo.toml": {
                "package": {"name": "rd-vm"},
                "dependencies": {"rd-dex": {"path": "../rd-dex"}},
            },
            "crates/rd-dex/Cargo.toml": {"package": {"name": "rd-dex"}},
        }
        graph = workspace_graph_from_manifests(manifests)
        self.assertEqual({"rd-vm"}, graph["rd-cli"])
        self.assertEqual({"rd-dex"}, graph["rd-vm"])
        self.assertEqual(set(), graph["rd-dex"])

    def test_public_api_inventory_ignores_same_crate_move_but_sees_signature_change(self):
        base = public_api_inventory_from_sources(
            {"crates/rd-dex/src/lib.rs": "pub fn parse(input: &[u8]) -> u8 { 1 }\n"}
        )
        moved = public_api_inventory_from_sources(
            {"crates/rd-dex/src/parser.rs": "pub fn parse(input: &[u8]) -> u8 { 1 }\n"}
        )
        findings, metrics = _public_api_findings(base, moved)
        self.assertEqual([], findings)
        self.assertEqual(0, metrics["public_api_removed_items"])

        changed = public_api_inventory_from_sources(
            {"crates/rd-dex/src/lib.rs": "pub fn parse(input: &[u8]) -> u16 { 1 }\n"}
        )
        findings, metrics = _public_api_findings(base, changed)
        self.assertEqual(["public-api-signature-changed"], [f.id for f in findings])
        self.assertEqual(1, metrics["public_api_changed_items"])

    def test_named_test_inventory_detects_swap_and_ignores_same_crate_move(self):
        source = "#[test]\nfn parses_header() {}\n#[tokio::test]\nasync fn parses_body() {}\n"
        base = named_test_inventory_from_sources({"crates/rd-dex/src/lib.rs": source})
        moved = named_test_inventory_from_sources({"crates/rd-dex/src/parser.rs": source})
        findings, metrics = _named_test_findings(base, moved)
        self.assertEqual([], findings)
        self.assertEqual(2, metrics["named_test_base_total"])
        self.assertEqual(2, metrics["named_test_head_total"])

        swapped = named_test_inventory_from_sources(
            {
                "crates/rd-dex/src/lib.rs": "#[test]\nfn smoke() {}\n#[test]\nfn parses_body() {}\n"
            }
        )
        findings, metrics = _named_test_findings(base, swapped)
        self.assertEqual(["test-removed"], [f.id for f in findings])
        self.assertEqual(1, metrics["named_test_removed_instances"])

        duplicated = named_test_inventory_from_sources(
            {
                "crates/rd-dex/src/a.rs": "#[test]\nfn same_name() {}\n",
                "crates/rd-dex/src/b.rs": "#[test]\nfn same_name() {}\n",
            }
        )
        self.assertEqual(2, duplicated[("rd-dex", "same_name")])
        findings, _metrics = _named_test_findings(
            duplicated, {("rd-dex", "same_name"): 1}
        )
        self.assertEqual(["test-removed"], [f.id for f in findings])

        self.assertEqual(
            ["rd-apk:zip", "rd-dex:dex"],
            _fuzz_target_names(
                [
                    "crates/rd-dex/fuzz/fuzz_targets/dex.rs",
                    "crates/rd-apk/fuzz/fuzz_targets/zip.rs",
                    "crates/rd-apk/src/lib.rs",
                ]
            ),
        )
        named_findings = _check_inventory_regression(
            {"fuzz_target_names": ["rd-apk:zip", "rd-dex:dex"]},
            {"fuzz_target_names": ["rd-apk:zip", "rd-dex:other"]},
        )
        self.assertEqual(["fuzz-target-removed"], [f.id for f in named_findings])

    def test_workflow_template_injection_and_action_pin_classification(self):
        workflow = """
steps:
  - run: |
      echo "${{ github.event.issue.title }}"
  - env:
      TITLE: ${{ github.event.issue.title }}
    run: echo "$TITLE"
"""
        injections = workflow_template_injections(workflow)
        self.assertEqual(1, len(injections))
        self.assertEqual("pinned", _action_ref_kind("actions/checkout@" + "a" * 40))
        self.assertEqual("mutable-tag", _action_ref_kind("actions/checkout@v4"))
        self.assertEqual("moving-branch", _action_ref_kind("owner/action@main"))
        triggers = _workflow_trigger_names(
            "# The trusted workflow_run publisher is separate.\n"
            "on:\n"
            "  pull_request:\n"
            "permissions:\n"
            "  contents: read\n"
        )
        self.assertIn("pull_request", triggers)
        self.assertNotIn("workflow_run", triggers)

    def test_lock_diff_reports_duplicate_version_growth(self):
        old = (
            'version = 3\n[[package]]\nname="hashbrown"\nversion="0.14.0"\n'
            'source="registry+https://github.com/rust-lang/crates.io-index"\n'
        )
        new = old + (
            '[[package]]\nname="hashbrown"\nversion="0.15.0"\n'
            'source="registry+https://github.com/rust-lang/crates.io-index"\n'
        )
        _changes, metrics = compare_cargo_locks(old, new)
        self.assertEqual(0, metrics["dependency_duplicate_names_base"])
        self.assertEqual(1, metrics["dependency_duplicate_names_head"])
        self.assertEqual(1, metrics["dependency_duplicate_names_delta"])

    def test_mutants_profile_is_explicitly_planned(self):
        plan = build_plan(
            profile="mutants",
            areas=[],
            files=[],
            labels=[],
            risk=0,
            dependency_bot=False,
        )
        self.assertEqual(
            ["static", "core", "mutation"], [item["component"] for item in plan]
        )
        for profile, expected in (("nightly", False), ("deep", True), ("weekly", True)):
            components = [
                item["component"]
                for item in build_plan(
                    profile=profile,
                    areas=[],
                    files=[],
                    labels=[],
                    risk=0,
                    dependency_bot=False,
                )
            ]
            self.assertEqual(expected, "mutation" in components, profile)


class Cold26Tests(unittest.TestCase):
    def test_parse_snapshot_hits(self):
        text = "\n".join(
            [
                json.dumps({"capture": True, "snapshot": False, "t_frame_ms": 400, "frame_md5": "x"}),
                json.dumps({"snapshot": True, "t_frame_ms": 7.0, "frame_md5": "a"}),
                json.dumps({"snapshot": True, "t_frame_ms": 9.0, "frame_md5": "a"}),
                "VERDICT ok",
            ]
        )
        parsed = parse_cold26_output(text)
        self.assertEqual(8.0, parsed["median_ms"])
        self.assertEqual(["a"], parsed["frame_hashes"])

    def test_parse_real_harness_capture_prefix_and_interactive_time(self):
        capture = json.dumps(
            {"capture": True, "t_frame_ms": 401.0, "t_interactive_ms": 700.0, "frame_md5": "z"}
        )
        rows = [
            {
                "t_frame_ms": 7.0,
                "t_interactive_ms": 350.0,
                "frame_md5": "a",
                "launch": '{"snapshot":true}',
                "tap_ok": True,
            },
            {
                "t_frame_ms": 9.0,
                "t_interactive_ms": 450.0,
                "frame_md5": "a",
                "launch": '{"snapshot":true}',
                "tap_ok": False,
            },
        ]
        text = "capture: " + capture + "\n" + "\n".join(json.dumps(r) for r in rows)
        parsed = parse_cold26_output(text)
        self.assertEqual(1, len(parsed["captures"]))
        self.assertEqual(8.0, parsed["median_ms"])
        self.assertEqual(400.0, parsed["interactive_median_ms"])
        self.assertEqual(1, parsed["tap_successes"])
        self.assertEqual(1, parsed["tap_failures"])
        self.assertEqual(["a"], parsed["frame_hashes"])


class CampaignUnitTests(unittest.TestCase):
    def test_robust_cold_statistics_ignore_one_outlier_better_than_mean(self):
        self.assertEqual(1.0, _mad([1.0, 2.0, 3.0, 100.0]))
        self.assertAlmostEqual(
            3.0 * 1.4826,
            _noise_floor_ms([1.0, 2.0, 3.0, 100.0], [2.0, 2.0, 2.0]),
        )

    def test_repeated_test_outcomes_name_flaky_and_consistent_failures(self):
        first = """
test vm::flaky_case ... FAILED
test vm::always_bad ... FAILED
test vm::stable ... ok
"""
        second = """
test vm::flaky_case ... ok
test vm::always_bad ... FAILED
test vm::stable ... ok
"""
        summary = summarize_repeated_test_outcomes([first, second])
        self.assertEqual(["vm::flaky_case"], summary["flaky_tests"])
        self.assertEqual(["vm::always_bad"], summary["consistent_failures"])

    def test_native_fixtures_built_only_for_native_bearing_test_commands(self):
        # issue #193: only commands that can execute the rd-ndk native
        # tests (workspace --all-features, or an explicit native feature)
        # may trigger the golden/native fixture build.
        self.assertTrue(
            CampaignRunner._commands_may_run_native(
                [("t", ["cargo", "test", "--workspace", "--all-features", "--locked"])]
            )
        )
        self.assertTrue(
            CampaignRunner._commands_may_run_native(
                [("t", ["cargo", "test", "-p", "rd-ndk", "--features", "native"])]
            )
        )
        self.assertTrue(
            CampaignRunner._commands_may_run_native(
                [("t", ["cargo", "test", "-p", "rd-vm", "--features=jit,native"])]
            )
        )
        self.assertFalse(
            CampaignRunner._commands_may_run_native(
                [("t", ["cargo", "test", "-p", "rd-vm", "--locked"])]
            )
        )
        self.assertFalse(
            CampaignRunner._commands_may_run_native(
                [("t", ["cargo", "test", "-p", "rd-vm", "--features", "jit"])]
            )
        )

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            report = {"bot_version": "1.1.0", "checks": [], "metrics": {}}
            runner = CampaignRunner(
                repo=root,
                report=report,  # type: ignore[arg-type]
                policy=POLICY,
                baseline=BASELINE,
                options=CampaignOptions(output_dir=root / "out"),
            )
            calls: list[list[str]] = []
            runner._run_check = lambda argv, **_kw: calls.append(list(argv))  # type: ignore[method-assign]
            all_features = [
                ("t", ["cargo", "test", "--workspace", "--all-features", "--locked"])
            ]
            # Missing artifacts -> exactly one recorded build.sh invocation,
            # even when both core and determinism ask.
            runner._ensure_native_test_fixtures("core", all_features)
            runner._ensure_native_test_fixtures("determinism", all_features)
            self.assertEqual([["bash", "golden/native/build.sh"]], calls)

            # Artifacts already present -> no build is requested at all.
            artifacts = root / "golden/native/artifacts"
            artifacts.mkdir(parents=True)
            for name in ("x86_64.so", "arm64-v8a.so", "armeabi-v7a.so"):
                (artifacts / name).write_bytes(b"\x7fELF")
            runner._native_fixtures_ensured = False
            runner._ensure_native_test_fixtures("core", all_features)
            self.assertEqual([["bash", "golden/native/build.sh"]], calls)

    # issue #283: a base already over the absolute `rd perf` budget must
    # not block every candidate (catch-22); regressions stay blocked.
    def _perf_runner(self, root: Path, runs: dict[str, tuple[int, dict | None]]):
        report = {"bot_version": "1.1.0", "checks": [], "metrics": {}}
        runner = CampaignRunner(
            repo=root,
            report=report,  # type: ignore[arg-type]
            policy=POLICY,
            baseline=BASELINE,
            options=CampaignOptions(output_dir=root / "out", execute=True),
        )

        def fake_raw(argv, **_kw):
            exit_code, payload = runs[Path(argv[0]).name]
            return RawResult(list(argv), root, exit_code, json.dumps(payload) if payload else "garbage", "", 1.0)

        runner._run_raw = fake_raw  # type: ignore[method-assign]
        return runner, report

    @staticmethod
    def _perf_json(p50: float, ok: bool) -> dict:
        return {"pass": ok, "cold_p50_emulated_ms": p50, "cold_max_emulated_ms": p50 + 40, "peak_rss_mb": 95.0, "app_heap_peak_mb": 3.7}

    def _perf_compare(self, base, cand):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            runner, report = self._perf_runner(root, {"rd-base": base, "rd-cand": cand})
            apk = root / "app.apk"
            runner._run_perf(root / "rd-base", apk, label="base")
            runner._run_perf(root / "rd-cand", apk, label="candidate")
            runner._resolve_perf_budgets()
            checks = {c["id"]: c for c in report["checks"]}
            findings = [f["id"] for f in report.get("findings", [])]
            return checks, findings

    def test_perf_red_base_does_not_block_improving_candidate(self):
        checks, findings = self._perf_compare((1, self._perf_json(1819.0, False)), (0, self._perf_json(900.0, True)))
        self.assertEqual("pass", checks["perf-rd-perf-base-app"]["status"])
        self.assertEqual("pass", checks["perf-rd-perf-candidate-app"]["status"])
        self.assertNotIn("perf-budget-candidate-app", checks)
        self.assertEqual(["perf-base-budget-preexisting"], findings)

    def test_perf_red_base_and_red_candidate_is_preexisting_not_blocking(self):
        checks, findings = self._perf_compare((1, self._perf_json(1819.0, False)), (1, self._perf_json(1840.0, False)))
        self.assertEqual("pass", checks["perf-budget-candidate-app"]["status"])
        self.assertIn("pre-existing", checks["perf-budget-candidate-app"]["summary"])
        self.assertIn("perf-base-budget-preexisting", findings)

    def test_perf_green_base_and_red_candidate_still_blocks(self):
        checks, findings = self._perf_compare((0, self._perf_json(1400.0, True)), (1, self._perf_json(1550.0, False)))
        self.assertEqual("fail", checks["perf-budget-candidate-app"]["status"])
        self.assertTrue(checks["perf-budget-candidate-app"]["required"])
        self.assertNotIn("perf-base-budget-preexisting", findings)

    def test_perf_missing_measurement_still_blocks(self):
        checks, _ = self._perf_compare((1, self._perf_json(1819.0, False)), (101, None))
        self.assertEqual("fail", checks["perf-rd-perf-candidate-app"]["status"])

    def test_perf_candidate_without_base_reference_still_blocks(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            runner, report = self._perf_runner(root, {"rd-cand": (1, self._perf_json(1840.0, False))})
            runner._run_perf(root / "rd-cand", root / "app.apk", label="candidate")
            runner._resolve_perf_budgets()
            checks = {c["id"]: c for c in report["checks"]}
            self.assertEqual("fail", checks["perf-budget-candidate-app"]["status"])
            self.assertIn("base not measured", checks["perf-budget-candidate-app"]["summary"])

    def test_dynamic_feature_sets_cover_singletons_pairs_and_all(self):
        sets = CampaignRunner._feature_sets_from_names(
            ["default", "jit", "native", "advanced-components"], full=False
        )
        self.assertIn((), sets)
        self.assertIn(("jit",), sets)
        self.assertIn(("jit", "native"), sets)
        self.assertIn(("advanced-components", "jit", "native"), sets)
        self.assertFalse(any("default" in item for item in sets))

    def test_parse_mutants_output_counts_outcomes_and_kill_rate(self):
        parsed = parse_mutants_output(
            "12 mutants caught\n3 mutants missed\n1 mutants unviable\n"
        )
        self.assertEqual(12, parsed["caught"])
        self.assertEqual(3, parsed["missed"])
        self.assertEqual(1, parsed["unviable"])
        self.assertAlmostEqual(12 / 15, parsed["kill_rate"])

        current = parse_mutants_output(
            "MISSED src/lib.rs:1:29: replace inc -> i32 with 1\n"
            "5 mutants tested in 1s: 3 missed, 2 caught\n"
        )
        self.assertEqual(2, current["caught"])
        self.assertEqual(3, current["missed"])
        self.assertAlmostEqual(2 / 5, current["kill_rate"])
        self.assertEqual("text", current["outcome_source"])

        structured = parse_mutants_outcomes(
            {
                "total_mutants": 6,
                "caught": 3,
                "missed": 1,
                "unviable": 1,
                "timeout": 1,
                "cargo_mutants_version": "27.1.0",
                "outcomes": [
                    {
                        "summary": "MissedMutant",
                        "scenario": {
                            "Mutant": {
                                "name": "src/lib.rs:1:29: replace inc -> i32 with 1"
                            }
                        },
                    }
                ],
            }
        )
        self.assertIsNotNone(structured)
        assert structured is not None
        self.assertEqual(6, structured["total"])
        self.assertEqual(5, structured["viable"])
        self.assertAlmostEqual(3 / 5, structured["kill_rate"])
        self.assertEqual("outcomes.json", structured["outcome_source"])
        self.assertEqual(
            ["src/lib.rs:1:29: replace inc -> i32 with 1"],
            structured["missed_mutants"],
        )
        self.assertIsNone(parse_mutants_outcomes({"caught": True}))
        self.assertEqual(
            "fail",
            mutation_check_status(
                exit_code=2,
                missed=1,
                timeouts=0,
                blocking_context=True,
            ),
        )
        self.assertEqual(
            "warn",
            mutation_check_status(
                exit_code=2,
                missed=1,
                timeouts=0,
                blocking_context=False,
            ),
        )
        self.assertEqual(
            "warn",
            mutation_check_status(
                exit_code=2,
                missed=2,
                timeouts=0,
                blocking_context=True,
                max_missed=2,
            ),
        )
        self.assertEqual(
            "fail",
            mutation_check_status(
                exit_code=4,
                missed=0,
                timeouts=0,
                blocking_context=False,
            ),
        )

    def test_dependency_blast_radius_and_deterministic_sbom(self):
        metadata = {
            "workspace_members": ["workspace-id"],
            "packages": [
                {
                    "id": "workspace-id",
                    "name": "rd-cli",
                    "version": "0.9.0",
                    "license": "MIT OR Apache-2.0",
                    "source": None,
                    "targets": [],
                },
                {
                    "id": "serde-id",
                    "name": "serde",
                    "version": "1.0.0",
                    "license": "MIT OR Apache-2.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "rust_version": "1.56",
                    "targets": [{"kind": ["lib"], "crate_types": ["lib"]}],
                },
            ],
            "resolve": {
                "nodes": [
                    {
                        "id": "workspace-id",
                        "deps": [
                            {
                                "pkg": "serde-id",
                                "dep_kinds": [{"kind": None}],
                            }
                        ],
                    },
                    {"id": "serde-id", "deps": []},
                ]
            },
        }
        blast = CampaignRunner._dependency_blast_radius(metadata)
        self.assertEqual(["rd-cli"], blast[("serde", "1.0.0")]["direct_workspace_consumers"])
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            report = {"bot_version": "1.1.0", "checks": [], "metrics": {}}
            runner = CampaignRunner(
                repo=root,
                report=report,  # type: ignore[arg-type]
                policy=POLICY,
                baseline=BASELINE,
                options=CampaignOptions(output_dir=root / "out"),
            )
            result = runner._write_cyclonedx_sbom(metadata)
            self.assertIsNotNone(result)
            path, count = result  # type: ignore[misc]
            self.assertEqual(2, count)
            document = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual("CycloneDX", document["bomFormat"])
            self.assertEqual("1.5", document["specVersion"])

    def test_osv_querybatch_uses_purls_and_follows_per_query_pagination(self):
        captured_payloads: list[dict] = []
        captured_timeouts: list[float] = []
        responses = [
            {
                "results": [
                    {"vulns": [{"id": "GHSA-1"}], "next_page_token": "serde-more"},
                    {"vulns": [{"id": "RUSTSEC-1"}]},
                ]
            },
            {"results": [{"vulns": [{"id": "GHSA-2"}]}]},
        ]

        class Response:
            def __init__(self, payload):
                self.payload = payload

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            def read(self):
                return json.dumps(self.payload).encode()

        def fake_urlopen(request, timeout):
            captured_timeouts.append(timeout)
            captured_payloads.append(json.loads(request.data.decode()))
            return Response(responses.pop(0))

        with mock.patch.object(campaigns_module.urllib_request, "urlopen", fake_urlopen):
            result = CampaignRunner._query_osv(
                [("serde", "1.0.0"), ("toml", "0.8.0")], timeout=7
            )
        self.assertEqual([7, 7], captured_timeouts)
        self.assertEqual(
            ["pkg:cargo/serde@1.0.0", "pkg:cargo/toml@0.8.0"],
            [q["package"]["purl"] for q in captured_payloads[0]["queries"]],
        )
        # Only the unfinished serde query is resent, with its own page token;
        # the completed toml query is not included in the follow-up batch.
        self.assertEqual(1, len(captured_payloads[1]["queries"]))
        self.assertEqual(
            "pkg:cargo/serde@1.0.0",
            captured_payloads[1]["queries"][0]["package"]["purl"],
        )
        self.assertEqual("serde-more", captured_payloads[1]["queries"][0]["page_token"])
        self.assertFalse(result["truncated"])
        self.assertEqual(2, result["pages"])
        self.assertEqual(0, result["pending_queries"])
        by_package = {item["package"]: item["vulnerability_ids"] for item in result["results"]}
        self.assertEqual(["GHSA-1", "GHSA-2"], by_package["serde"])
        self.assertEqual(["RUSTSEC-1"], by_package["toml"])

    def test_efficiency_summary_accepts_legacy_sentinel_six_and_rejects_mismatch(self):
        script = ROOT.parents[1] / "scripts" / "summarize-efficiency-benchmarks.py"
        six = ["roots", "host", "sweep", "minor", "resolve", "interp"]
        nine = [*six, "strings", "subtype", "jit"]
        rows = []
        for workload in six:
            for label, elapsed in (("baseline", 10.0), ("candidate", 9.8)):
                for repetition in range(3):
                    rows.append(
                        {
                            "workload": workload,
                            "label": label,
                            "repetition": repetition,
                            "checksum": 123,
                            "iterations": 100,
                            "rss_source": "gnu-time-child",
                            "elapsed_ms": elapsed + repetition * 0.01,
                            "max_rss_kib": 6000,
                        }
                    )
        with tempfile.TemporaryDirectory() as raw:
            evidence = Path(raw)
            bench = evidence / "benchmarks.json"
            bench.write_text(json.dumps(rows), encoding="utf-8")
            legacy = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "--evidence",
                    str(evidence),
                    "--input",
                    str(bench),
                    "--resamples",
                    "1000",
                ],
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(0, legacy.returncode, legacy.stderr)
            summary = json.loads((evidence / "final-benchmark-summary.json").read_text())
            self.assertEqual(set(six), {row["workload"] for row in summary})
            mismatch = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "--evidence",
                    str(evidence),
                    "--input",
                    str(bench),
                    "--expected-workloads",
                    *nine,
                    "--resamples",
                    "1000",
                ],
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertNotEqual(0, mismatch.returncode)
            self.assertIn("workload set mismatch", mismatch.stderr)
        campaign_source = (ROOT / "sentinel" / "campaigns.py").read_text(encoding="utf-8")
        self.assertIn('"--expected-workloads"', campaign_source)
        self.assertIn('"--features"', campaign_source)
        for workload in nine:
            self.assertIn(f'"{workload}"', campaign_source)


class GenericMcpEnduranceTests(unittest.TestCase):
    def test_fake_stdio_server_completes_cycles(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            server = root / "fake-rd"
            server.write_text(
                """#!/usr/bin/env python3
import json, sys
TOOLS = ["install", "launch", "observe", "ui_tree", "screenshot", "runtime_health", "wait", "app_info"]
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if "id" not in msg:
        continue
    method = msg.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05"}
    elif method == "tools/list":
        result = {"tools": [{"name": n} for n in TOOLS]}
    elif method == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "runtime_health":
            content = [{"type": "text", "text": json.dumps({"running": True, "heap_used_bytes": 100, "heap_peak_bytes": 120})}]
        elif name == "screenshot":
            content = [{"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]
        elif name == "observe":
            content = [{"type": "text", "text": json.dumps({"texts": ["ok"]})}]
        elif name == "ui_tree":
            content = [{"type": "text", "text": "<hierarchy />"}]
        else:
            content = [{"type": "text", "text": "ok"}]
        result = {"content": content, "isError": False}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
""",
                encoding="utf-8",
            )
            server.chmod(0o755)
            apk = root / "fake.apk"
            apk.write_bytes(b"not really an apk; the fake server does not read it")
            result = run_generic_apk_endurance(
                binary=server,
                apk=apk,
                repo=root,
                output_dir=root / "out",
                cycles=2,
                timeout_seconds=10,
            )
            self.assertTrue(result.ok, result.error)
            self.assertEqual(2, result.cycles_completed)
            self.assertEqual(8, len(result.tool_names))
            self.assertEqual(2, result.calls["observe"])
            self.assertEqual(100, result.final_heap_bytes)


class CommandTests(unittest.TestCase):
    def test_parse_commands(self):
        self.assertEqual("deep", parse_command("/rd-sentinel deep").profile)
        cmd = parse_command("hello\n/rd-sentinel fuzz 123")
        self.assertEqual(123, cmd.fuzz_seconds)
        self.assertEqual("invalid", parse_command("/rd-sentinel fuzz nope").action)
        mutants = parse_command("/rd-sentinel mutants 45")
        self.assertEqual("mutants", mutants.profile)
        self.assertEqual(45, mutants.mutation_minutes)
        self.assertEqual(45, parse_command("/rd-sentinel mutants").mutation_minutes)
        self.assertEqual("invalid", parse_command("/rd-sentinel mutants 241").action)
        self.assertEqual("mutants", parse_command("/rd-sentinel rerun mutants").profile)

    def test_command_prefix_must_be_a_whole_token(self):
        self.assertIsNone(parse_command("/rd-sentinel-please deep"))
        self.assertIsNone(parse_command("quote: /rd-sentinel deep"))

    def test_merge_group_event_context_uses_group_shas_without_inventing_pr_number(self):
        with tempfile.TemporaryDirectory() as raw:
            event_path = Path(raw) / "event.json"
            event_path.write_text(
                json.dumps(
                    {
                        "merge_group": {
                            "base_sha": "a" * 40,
                            "head_sha": "b" * 40,
                            "head_ref": "refs/heads/gh-readonly-queue/main/pr-42-" + "c" * 8,
                        },
                        "sender": {"login": "queue-bot"},
                    }
                ),
                encoding="utf-8",
            )
            args = argparse.Namespace(
                event=str(event_path),
                labels=None,
                base=None,
                head=None,
                event_name=None,
                campaign=None,
                profile="auto",
                actor=None,
                author=None,
                head_ref=None,
                pr_number=None,
                repository="owner/repo",
                run_url=None,
            )
            context, base_ref, head_ref = _context_from_args(args, Path(raw))
        self.assertEqual("a" * 40, base_ref)
        self.assertEqual("b" * 40, head_ref)
        self.assertEqual("merge_group", context.event_name)
        self.assertIsNone(context.pr_number)
        self.assertIn("gh-readonly-queue", context.head_ref)

    def test_only_maintainer_associations_are_authorized(self):
        self.assertTrue(command_authorized({"comment": {"author_association": "MEMBER"}}, POLICY))
        self.assertFalse(command_authorized({"comment": {"author_association": "CONTRIBUTOR"}}, POLICY))

    def test_mutants_command_dispatches_pr_workflow_with_budget(self):
        class FakeClient(GitHubClient):
            def __init__(self):
                super().__init__("owner/repo", token="test", dry_run=False)
                self.dispatch = None
                self.comments = []

            def repository_info(self):
                return {"default_branch": "main"}

            def dispatch_workflow(self, workflow, ref, inputs):
                self.dispatch = (workflow, ref, inputs)
                return {"ok": True}

            def comment(self, issue_number, body):
                self.comments.append((issue_number, body))
                return {"id": 2}

        client = FakeClient()
        event = {
            "comment": {
                "body": "/rd-sentinel mutants 37",
                "author_association": "OWNER",
            },
            "issue": {"number": 44, "pull_request": {"url": "https://example.invalid/pr"}},
        }
        result = execute_comment_command(client=client, event=event, policy=POLICY)
        self.assertEqual("rd-sentinel.yml", result["workflow"])
        self.assertEqual("mutants", result["inputs"]["profile"])
        self.assertEqual("37", result["inputs"]["mutation_minutes"])
        self.assertEqual("44", result["inputs"]["pr_number"])
        self.assertIn("37 minutes", client.comments[0][1])

    def test_status_quotes_latest_bot_report_not_a_forged_marker(self):
        class FakeClient(GitHubClient):
            def __init__(self):
                super().__init__("owner/repo", token="test", dry_run=False)
                self.comments = []

            def paged(self, path):
                return [
                    {
                        "body": "<!-- rd-sentinel-report -->\n<!-- rd-sentinel-fingerprint:abcdef0123456789 -->\n✅ **PASS**",
                        "user": {"login": "github-actions[bot]"},
                        "html_url": "https://example.invalid/real",
                    },
                    {
                        "body": "<!-- rd-sentinel-report -->\n❌ **BLOCKED**",
                        "user": {"login": "attacker"},
                        "html_url": "https://example.invalid/forged",
                    },
                ]

            def comment(self, issue_number, body):
                self.comments.append((issue_number, body))
                return {"id": 1}

        client = FakeClient()
        event = {
            "comment": {"body": "/rd-sentinel status", "author_association": "MEMBER"},
            "issue": {"number": 12},
        }
        result = execute_comment_command(client=client, event=event, policy=POLICY)
        self.assertTrue(result["latest_report_found"])
        self.assertIn("abcdef0123456789", client.comments[0][1])
        self.assertIn("real", client.comments[0][1])


class PublisherAndMergeTests(unittest.TestCase):
    def test_incomplete_report_gets_discoverable_blocked_label(self):
        labels = _labels_for_report({"verdict": "incomplete", "findings": [], "metrics": {}}, POLICY)
        self.assertIn("sentinel:blocked", labels)
        self.assertIn("sentinel:deep-required", labels)

    def test_risk_score_gets_a_label(self):
        labels = _labels_for_report(
            {"verdict": "blocked", "findings": [], "metrics": {}, "risk_score": 91}, POLICY
        )
        self.assertIn("sentinel:risk-critical", labels)

    def test_test_gap_label_uses_real_finding_ids(self):
        for finding_id in (
            "test-inventory-below-baseline",
            "test-removed",
            "static-test-attributes-shrank",
            "golden-vector-count-shrank",
            "fuzz-target-count-shrank",
            "fuzz-target-removed",
            "high-risk-change-without-test-evidence",
        ):
            labels = _labels_for_report(
                {
                    "verdict": "blocked",
                    "findings": [{"id": finding_id, "area": "tests"}],
                    "metrics": {},
                },
                POLICY,
            )
            self.assertIn("sentinel:test-gap", labels, finding_id)

    def test_check_annotations_payload_and_annotation_batching(self):
        report = {
            "head_sha": "b" * 40,
            "fingerprint": "f" * 64,
            "risk_score": 80,
            "complete": True,
            "checks": [{"status": "pass"}],
            "findings": [
                {
                    "id": "blocked-example",
                    "severity": "P1",
                    "title": "Blocked",
                    "evidence": "evidence",
                    "remediation": "fix it",
                    "path": "crates/rd-vm/src/lib.rs",
                    "line": 12,
                },
                {
                    "id": "warning-example",
                    "severity": "P2",
                    "title": "Warning",
                    "evidence": "evidence",
                    "remediation": "review it",
                    "path": "crates/rd-vm/src/lib.rs",
                    "line": 3,
                },
                {
                    "id": "no-line",
                    "severity": "P1",
                    "title": "No line",
                    "path": "Cargo.lock",
                    "line": None,
                },
                {
                    "id": "directory",
                    "severity": "P1",
                    "title": "Directory",
                    "path": "crates/rd-vm/src/",
                    "line": 1,
                },
            ],
        }
        annotations = _check_annotations_for_report(report)
        self.assertEqual(["failure", "warning"], [a["annotation_level"] for a in annotations])
        payload = _check_run_payload(
            report,
            POLICY,
            verdict="blocked",
            annotations=annotations,
            details_url="https://example.invalid/run",
        )
        self.assertEqual("rd-sentinel/gate", payload["name"])
        self.assertEqual("failure", payload["conclusion"])
        self.assertEqual(2, len(payload["output"]["annotations"]))

        class FakeClient(GitHubClient):
            def __init__(self):
                super().__init__("owner/repo", token="test", dry_run=False)
                self.created = None
                self.updates = []

            def create_check_run(self, payload):
                self.created = payload
                return {"id": 77}

            def update_check_run(self, check_run_id, payload):
                self.updates.append((check_run_id, payload))
                return {"id": check_run_id}

        many_report = dict(report)
        many_report["findings"] = [
            {
                "id": f"finding-{index}",
                "severity": "P2",
                "title": "Warning",
                "path": "crates/rd-vm/src/lib.rs",
                "line": index + 1,
            }
            for index in range(55)
        ]
        client = FakeClient()
        result = _publish_check_run(client, many_report, POLICY, verdict="blocked")
        self.assertEqual(55, result["annotation_count"])
        self.assertEqual(50, len(client.created["output"]["annotations"]))
        self.assertEqual(1, len(client.updates))
        self.assertEqual(5, len(client.updates[0][1]["output"]["annotations"]))

    def test_sarif_upload_payload_is_gzip_base64_and_pr_ref(self):
        client = GitHubClient("owner/repo", token="test", dry_run=True)
        sarif = {"version": "2.1.0", "runs": []}
        ref = _sarif_ref_for_report({"pr_number": 170, "head_ref": "feature"})
        self.assertEqual("refs/pull/170/head", ref)
        client.upload_code_scanning_sarif(sarif=sarif, commit_sha="b" * 40, ref=ref)
        body = client.requests[-1]["body"]
        decoded = json.loads(gzip.decompress(base64.b64decode(body["sarif"])).decode())
        self.assertEqual(sarif, decoded)
        self.assertEqual("b" * 40, body["commit_sha"])

    def test_publish_report_creates_check_run_and_uploads_sarif(self):
        base = "a" * 40
        head = "b" * 40

        class FakeClient(GitHubClient):
            def __init__(self):
                super().__init__("owner/repo", token="test", dry_run=False)
                self.check_payload = None
                self.sarif_upload = None

            def get_pr(self, number):
                return {"head": {"sha": head}, "base": {"sha": base}, "labels": []}

            def pr_files(self, number):
                return ["crates/rd-vm/src/lib.rs"]

            def ensure_label(self, name, color, description):
                return None

            def upsert_issue_comment(self, issue_number, body, marker="<!-- rd-sentinel-report -->"):
                return {"id": 1}

            def add_labels(self, issue_number, labels):
                return {}

            def remove_label(self, issue_number, label):
                return {}

            def create_commit_status(self, sha, state, *, context, description, target_url=""):
                return {"state": state, "context": context}

            def create_check_run(self, payload):
                self.check_payload = payload
                return {"id": 9}

            def upload_code_scanning_sarif(self, *, sarif, commit_sha, ref):
                self.sarif_upload = (sarif, commit_sha, ref)
                return {"id": "sarif-1"}

        report = {
            "repository": "owner/repo",
            "pr_number": 170,
            "base_sha": base,
            "head_sha": head,
            "head_ref": "feature",
            "campaign": "test",
            "profile": "auto",
            "areas": [],
            "risk_score": 70,
            "risk_factors": [],
            "changed_files": [{"path": "crates/rd-vm/src/lib.rs"}],
            "dependency_changes": [],
            "plan": [],
            "metrics": {},
            "checks": [],
            "findings": [
                Finding(
                    id="example-finding",
                    severity="P1",
                    area="vm-core",
                    title="Example blocked",
                    evidence="evidence",
                    path="crates/rd-vm/src/lib.rs",
                    line=7,
                    remediation="fix it",
                ).to_dict()
            ],
        }
        finalize_report(report, POLICY)  # type: ignore[arg-type]
        client = FakeClient()
        result = publish_report(
            client=client,
            report=report,  # type: ignore[arg-type]
            policy=POLICY,
            expected_head_sha=head,
            workflow_conclusion="success",
            workflow_run_url="https://example.invalid/run",
        )
        self.assertIn("check_run", result)
        self.assertEqual(1, len(client.check_payload["output"]["annotations"]))
        self.assertEqual("sarif-1", result["code_scanning_sarif"]["id"])
        self.assertEqual("refs/pull/170/head", client.sarif_upload[2])
        self.assertEqual(
            "example-finding",
            client.sarif_upload[0]["runs"][0]["results"][0]["ruleId"],
        )

    def test_upsert_ignores_marker_from_another_user(self):
        class FakeClient(GitHubClient):
            def __init__(self):
                super().__init__("owner/repo", token="test", dry_run=False)
                self.calls = []

            def paged(self, path):
                return [
                    {
                        "id": 99,
                        "body": "<!-- rd-sentinel-report -->\nforged",
                        "user": {"login": "someone-else"},
                    }
                ]

            def request(self, method, path, body=None):
                self.calls.append((method, path, body))
                return {"id": 100}

        client = FakeClient()
        client.upsert_issue_comment(7, "fresh")
        self.assertEqual("POST", client.calls[0][0])
        self.assertIn("/issues/7/comments", client.calls[0][1])

    def _write_merge_report(
        self,
        root: Path,
        name: str,
        status: str,
        findings: list[dict],
        plan: list[dict] | None = None,
    ) -> Path:
        report = {
            "base_sha": "a" * 40,
            "head_sha": "b" * 40,
            "campaign": "nightly",
            "profile": "nightly",
            "areas": [],
            "metrics": {},
            "plan": plan or [],
            "executed_components": ["core"] if status != "planned" else [],
            "requested_components": ["core"],
            "checks": [
                {
                    "id": "planned-core",
                    "title": "core",
                    "status": status,
                    "required": True,
                    "component": "core",
                    "command": [],
                    "exit_code": 0 if status == "pass" else 1 if status == "fail" else None,
                    "duration_ms": 1,
                    "summary": status,
                    "metrics": {},
                    "stdout_tail": "",
                    "stderr_tail": "",
                }
            ],
            "findings": findings,
        }
        finalize_report(report, POLICY)  # type: ignore[arg-type]
        folder = root / name
        folder.mkdir()
        (folder / "rd-sentinel-report.json").write_text(json.dumps(report), encoding="utf-8")
        return folder

    def _merge_root(self, root: Path) -> None:
        config = root / ".github/rd-sentinel"
        config.mkdir(parents=True)
        (config / "policy.json").write_text(json.dumps(POLICY), encoding="utf-8")
        (config / "baseline.json").write_text(json.dumps(BASELINE), encoding="utf-8")

    def test_merge_cli_keeps_worst_partial_verdict(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self._merge_root(root)
            finding = Finding("x", "P1", "tests", "blocked", "evidence")
            good = self._write_merge_report(root, "good", "pass", [])
            bad = self._write_merge_report(root, "bad", "fail", [finding.to_dict()])
            out = root / "merged.json"
            code = cli_main(["merge", str(good), str(bad), "--repo", str(root), "--out", str(out)])
            self.assertEqual(1, code)
            merged = json.loads(out.read_text())
            self.assertEqual("blocked", merged["verdict"])
            self.assertFalse(validate_report(merged))
            self.assertTrue((root / "merged.sarif").is_file())

    def test_merge_executed_check_wins_over_planned_placeholder(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self._merge_root(root)
            executed = self._write_merge_report(root, "a-executed", "pass", [])
            planned = self._write_merge_report(root, "z-planned", "planned", [])
            out = root / "merged.json"
            code = cli_main(["merge", str(executed), str(planned), "--repo", str(root), "--out", str(out)])
            self.assertEqual(0, code)
            merged = json.loads(out.read_text())
            self.assertEqual("pass", merged["checks"][0]["status"])
            self.assertEqual("pass", merged["verdict"])

    def test_merge_marks_unexecuted_planned_component_incomplete(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self._merge_root(root)
            partial = self._write_merge_report(
                root,
                "core-only",
                "pass",
                [],
                plan=[
                    {"component": "core", "reason": "test", "required": True},
                    {"component": "mutation", "reason": "test", "required": True},
                ],
            )
            out = root / "merged.json"
            code = cli_main(["merge", str(partial), "--repo", str(root), "--out", str(out)])
            self.assertEqual(3, code)
            merged = json.loads(out.read_text())
            self.assertEqual("incomplete", merged["verdict"])
            planned = [c for c in merged["checks"] if c["id"] == "planned-mutation"]
            self.assertEqual(1, len(planned))
            self.assertEqual("planned", planned[0]["status"])


class TinyRepoTests(unittest.TestCase):
    def _repo(self) -> tuple[tempfile.TemporaryDirectory[str], Path, str]:
        td = tempfile.TemporaryDirectory()
        repo = Path(td.name)
        subprocess.check_call(["git", "init", "-q", str(repo)])
        run(repo, "config", "user.email", "sentinel@example.invalid")
        run(repo, "config", "user.name", "RD Sentinel Test")
        (repo / "Cargo.toml").write_text('[workspace.package]\nrust-version = "1.76"\n', encoding="utf-8")
        (repo / "Cargo.lock").write_text("version = 3\n", encoding="utf-8")
        (repo / "crates/rd-vm/src").mkdir(parents=True)
        (repo / "crates/rd-vm/src/lib.rs").write_text("pub fn answer() -> u8 { 1 }\n#[test] fn t() {}\n", encoding="utf-8")
        run(repo, "add", ".")
        run(repo, "commit", "-qm", "base")
        return td, repo, run(repo, "rev-parse", "HEAD")

    def test_new_thread_local_blocks_but_exception_path_can_downgrade(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        lib = repo / "crates/rd-vm/src/lib.rs"
        lib.write_text(lib.read_text() + "thread_local! { static X: u8 = 1; }\n", encoding="utf-8")
        (repo / "crates/rd-vm/src/new_test.rs").write_text("#[test] fn regression() {}\n", encoding="utf-8")
        (repo / "docs/adr").mkdir(parents=True)
        (repo / "docs/adr/001.md").write_text("# test\n", encoding="utf-8")
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto"),
        )
        self.assertIn("new-thread-local-state", {f["id"] for f in report["findings"]})

    def test_removed_line_keeps_deleted_file_path_after_another_file(self):
        td, repo, _base = self._repo()
        self.addCleanup(td.cleanup)
        src = repo / "crates/rd-vm/src"
        (src / "a_first.rs").write_text("pub fn keep() {}\n", encoding="utf-8")
        (src / "z_deleted.rs").write_text("pub fn gone() {}\n", encoding="utf-8")
        run(repo, "add", ".")
        run(repo, "commit", "-qm", "add files")
        base = run(repo, "rev-parse", "HEAD")
        (src / "a_first.rs").write_text("pub fn keep() {}\n// touched\n", encoding="utf-8")
        (src / "z_deleted.rs").unlink()
        files = collect_changed_files(repo, base, "WORKTREE")
        lines = collect_diff_lines(repo, base, "WORKTREE", files)
        removed = [line for line in lines if line.kind == "removed" and "gone" in line.text]
        self.assertEqual(1, len(removed))
        self.assertTrue(removed[0].path.endswith("z_deleted.rs"))

    def test_public_api_same_crate_move_is_not_a_removal(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        src = repo / "crates/rd-vm/src"
        (src / "lib.rs").write_text("pub mod moved;\n#[test] fn t() {}\n", encoding="utf-8")
        (src / "moved.rs").write_text("pub fn answer() -> u8 { 1 }\n", encoding="utf-8")
        (repo / "docs/adr").mkdir(parents=True)
        (repo / "docs/adr/001.md").write_text("# move only\n", encoding="utf-8")
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto"),
        )
        ids = {f["id"] for f in report["findings"]}
        self.assertNotIn("public-api-removed", ids)
        self.assertEqual(0, report["metrics"]["public_api_removed_items"])
        self.assertTrue(report["risk_factors"])

    def test_workflow_change_scans_pinning_and_shell_injection(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        workflow = repo / ".github/workflows/test.yml"
        workflow.parent.mkdir(parents=True)
        workflow.write_text(
            "name: test\n"
            "on: [pull_request]\n"
            "permissions:\n"
            "  contents: read\n"
            "jobs:\n"
            "  test:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - uses: actions/checkout@v4\n"
            "      - run: echo \"${{ github.event.pull_request.title }}\"\n",
            encoding="utf-8",
        )
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto"),
        )
        ids = {f["id"] for f in report["findings"]}
        self.assertIn("workflow-action-not-sha-pinned", ids)
        self.assertIn("workflow-template-injection", ids)
        self.assertIn("workflow-checkout-persists-credentials", ids)

    def test_evidence_harness_change_requires_policy_label(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        scripts = repo / "scripts"
        scripts.mkdir()
        (scripts / "cold26.py").write_text("print('changed meter')\n", encoding="utf-8")
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto", labels=[]),
        )
        self.assertIn("evidence-harness-changed", {f["id"] for f in report["findings"]})
        approved = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto", labels=["sentinel:policy-approved"]),
        )
        self.assertNotIn("evidence-harness-changed", {f["id"] for f in approved["findings"]})

    def test_baseline_label_uses_candidate_contracts_for_inventory(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        trusted_baseline = {"contracts": {"minimum_static_test_attributes": 99}, "corpus": []}
        blocked = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=trusted_baseline,
            context=AuditContext(profile="auto", labels=[]),
        )
        self.assertIn("test-inventory-below-baseline", {f["id"] for f in blocked["findings"]})

        candidate_dir = repo / ".github/rd-sentinel"
        candidate_dir.mkdir(parents=True)
        (candidate_dir / "baseline.json").write_text(
            json.dumps({"contracts": {"minimum_static_test_attributes": 1}, "corpus": []}),
            encoding="utf-8",
        )
        approved = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=trusted_baseline,
            context=AuditContext(
                profile="auto",
                labels=["sentinel:baseline-approved", "sentinel:policy-approved"],
            ),
        )
        self.assertNotIn("test-inventory-below-baseline", {f["id"] for f in approved["findings"]})
        self.assertTrue(approved["metrics"]["baseline_approval_applied"])

    def test_version_move_to_nonregistry_source_blocks_and_forces_deep_plan(self):
        td, repo, _base = self._repo()
        self.addCleanup(td.cleanup)
        lock = repo / "Cargo.lock"
        lock.write_text(
            'version = 3\n\n[[package]]\nname = "flate2"\nversion = "1.0.0"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "abc"\n',
            encoding="utf-8",
        )
        run(repo, "add", "Cargo.lock")
        run(repo, "commit", "-qm", "lock flate2")
        base = run(repo, "rev-parse", "HEAD")
        lock.write_text(
            'version = 3\n\n[[package]]\nname = "flate2"\nversion = "1.1.0"\n'
            'source = "git+https://example.invalid/flate2#abc"\nchecksum = "def"\n',
            encoding="utf-8",
        )
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto", labels=[]),
        )
        ids = {f["id"] for f in report["findings"]}
        self.assertIn("dependency-source-changed", ids)
        self.assertIn("deep-dependency-changed", ids)
        self.assertEqual(["flate2"], report["metrics"]["deep_dependency_changes"])
        self.assertIn("compare", {item["component"] for item in report["plan"]})

    def test_protected_path_requires_label(self):
        td, repo, base = self._repo()
        self.addCleanup(td.cleanup)
        target = repo / "tools/rd-sentinel/sentinel"
        target.mkdir(parents=True)
        (target / "x.py").write_text("x = 1\n", encoding="utf-8")
        report = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto", labels=[]),
        )
        self.assertIn("protected-sentinel-path-changed", {f["id"] for f in report["findings"]})
        approved = build_static_report(
            repo=repo,
            base_ref=base,
            head_ref="WORKTREE",
            policy=POLICY,
            baseline=BASELINE,
            context=AuditContext(profile="auto", labels=["sentinel:policy-approved"]),
        )
        self.assertNotIn("protected-sentinel-path-changed", {f["id"] for f in approved["findings"]})


class RustScopeRuleTests(unittest.TestCase):
    """Inline `mod tests` scoping and global-vs-instance collection cells."""

    SRC = """pub fn prod() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first() {
        assert_eq!(prod(), 1);
    }

    fn helper() -> std::path::PathBuf {
        std::fs::create_dir_all("x").unwrap();
        std::path::PathBuf::from("x")
    }

    #[test]
    fn second() {
        let _ = helper();
    }
}

pub fn after() -> Option<u32> {
    Some(2).map(|v| v + 1)
}
"""

    def _repo(self, tmp: str, rel: str, src: str) -> Path:
        repo = Path(tmp)
        (repo / rel).parent.mkdir(parents=True, exist_ok=True)
        (repo / rel).write_text(src, encoding="utf-8")
        return repo

    def test_helper_between_nested_tests_is_test_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._repo(tmp, "crates/rd-apk/src/zip.rs", self.SRC)
            lines = _rust_test_lines(repo, "crates/rd-apk/src/zip.rs")
        src_lines = self.SRC.splitlines()
        helper_unwrap = next(i for i, l in enumerate(src_lines, 1) if "create_dir_all" in l)
        blank_between = helper_unwrap - 2
        self.assertIn(helper_unwrap, lines)
        self.assertIn(blank_between, lines)
        # production code before and after the test module stays production
        self.assertNotIn(1, lines)
        after = next(i for i, l in enumerate(src_lines, 1) if "pub fn after" in l)
        self.assertNotIn(after, lines)
        self.assertNotIn(after + 1, lines)

    def test_unwrap_in_inline_test_helper_is_not_production_finding(self) -> None:
        rel = "crates/rd-apk/src/zip.rs"
        src_lines = self.SRC.splitlines()
        n = next(i for i, l in enumerate(src_lines, 1) if "create_dir_all" in l)
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._repo(tmp, rel, self.SRC)
            findings, _ = _diff_rule_findings(
                repo,
                [ChangedFile("M", rel)],
                [DiffLine(rel, n, src_lines[n - 1], "added")],
                {},
            )
        self.assertNotIn("new-unwrap-expect", {f.id for f in findings})

    def test_unwrap_after_test_module_is_still_flagged(self) -> None:
        rel = "crates/rd-apk/src/zip.rs"
        src = self.SRC + "pub fn late(v: Option<u8>) -> u8 {\n    v.unwrap()\n}\n"
        src_lines = src.splitlines()
        n = len(src_lines) - 1
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._repo(tmp, rel, src)
            findings, _ = _diff_rule_findings(
                repo,
                [ChangedFile("M", rel)],
                [DiffLine(rel, n, src_lines[n - 1], "added")],
                {},
            )
        self.assertIn("new-unwrap-expect", {f.id for f in findings})

    def _rule_ids(self, text: str) -> dict[str, str]:
        rel = "crates/rd-vm/src/x.rs"
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._repo(tmp, rel, text + "\n")
            findings, _ = _diff_rule_findings(
                repo, [ChangedFile("M", rel)], [DiffLine(rel, 1, text, "added")], {}
            )
        return {f.id: f.severity for f in findings}

    def test_static_collection_globals_block(self) -> None:
        for text in (
            "static CACHE: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();",
            "pub(crate) static TABLE: OnceLock<Vec<u8>> = OnceLock::new();",
            "static REG: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());",
            "static SEEN: LazyLock<RwLock<HashSet<u64>>> = LazyLock::new(Default::default);",
        ):
            with self.subTest(text=text):
                self.assertEqual(self._rule_ids(text).get("new-global-collection"), "P1")

    def test_instance_field_collection_cell_is_reported_non_blocking(self) -> None:
        for text in (
            "    cache: OnceLock<Mutex<BTreeMap<u32, Arc<Vec<u8>>>>>,",
            "    slots: OnceLock<Vec<OnceLock<Option<String>>>>,",
        ):
            with self.subTest(text=text):
                ids = self._rule_ids(text)
                self.assertNotIn("new-global-collection", ids)
                self.assertEqual(ids.get("new-instance-collection-cell"), "P2")


if __name__ == "__main__":
    unittest.main()
