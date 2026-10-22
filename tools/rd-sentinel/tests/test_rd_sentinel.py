from __future__ import annotations

import datetime as dt
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from sentinel.audit import (  # noqa: E402
    _path_dependency_escapes,
    AuditContext,
    ChangedFile,
    build_plan,
    build_static_report,
    classify_semver_change,
    collect_changed_files,
    collect_diff_lines,
    compare_cargo_locks,
    license_expression_allowed,
)
from sentinel.campaigns import parse_cold26_output  # noqa: E402
from sentinel.cli import main as cli_main  # noqa: E402
from sentinel.github_api import (  # noqa: E402
    GitHubClient,
    _labels_for_report,
    command_authorized,
    execute_comment_command,
    parse_command,
)
from sentinel.mcp_endurance import run_generic_apk_endurance  # noqa: E402
from sentinel.model import (  # noqa: E402
    Finding,
    apply_exceptions,
    compute_report_fingerprint,
    finalize_report,
    match_any,
    render_markdown,
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
    "protected_paths": [".github/rd-sentinel/**", "tools/rd-sentinel/**"],
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

    def test_command_prefix_must_be_a_whole_token(self):
        self.assertIsNone(parse_command("/rd-sentinel-please deep"))
        self.assertIsNone(parse_command("quote: /rd-sentinel deep"))

    def test_only_maintainer_associations_are_authorized(self):
        self.assertTrue(command_authorized({"comment": {"author_association": "MEMBER"}}, POLICY))
        self.assertFalse(command_authorized({"comment": {"author_association": "CONTRIBUTOR"}}, POLICY))

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

    def _write_merge_report(self, root: Path, name: str, status: str, findings: list[dict]) -> Path:
        report = {
            "base_sha": "a" * 40,
            "head_sha": "b" * 40,
            "campaign": "nightly",
            "profile": "nightly",
            "areas": [],
            "metrics": {},
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


if __name__ == "__main__":
    unittest.main()
