"""Reproduce the scoped GC checkpoint from retained local command outputs."""
import argparse
import hashlib
import importlib.util
import json
import shutil
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOGS = ROOT / "local-artifacts/horizon-std"
FIXTURE = ROOT / "local-artifacts/conformance-collections-text-final"
OUTPUT = ROOT / "reports/gc-pressure-20261027.json"
DEST = OUTPUT.with_suffix("")
SPEC = importlib.util.spec_from_file_location("parity", ROOT / "scripts/report-upstream-parity.py")
PARITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARITY)
GIT = shutil.which("git.exe") or "git"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def generate(commit, build_dir, runs, visual_reviewed=False):
    assert subprocess.check_output([GIT, "-C", str(ROOT), "cat-file", "-t", commit]).strip() == b"commit"
    evidence = []

    def keep(path, relative):
        data = path.read_bytes()
        target = DEST / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        evidence.append(dict(path=target.relative_to(ROOT).as_posix(), source=str(path),
                             sha256=hashlib.sha256(data).hexdigest(), bytes=len(data)))

    tests = {}
    commands = {
        "upstream-tests": "cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1",
        "product-tests": "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored",
        "portable-lib": "cargo test -p as-vm --lib --locked -- --test-threads=1",
    }
    for name, command in commands.items():
        path = LOGS / f"gc-unpinned-{name}.log"
        record = PARITY.log_evidence(path, command)
        assert record["test_run_completed_successfully"] and record["totals"]["passed"] > 0
        assert not any(record["totals"][key] for key in ("failed", "ignored", "filtered"))
        tests[name] = record
        keep(path, f"tests/{name}.log")
    manifest = tomllib.loads((ROOT / "crates/as-vm/Cargo.toml").read_text())
    assert not PARITY.verify_reference_routes(tests["upstream-tests"]["suites"], manifest)
    for name in ("workspace-clippy", "upstream-clippy", "core-check"):
        path = LOGS / f"gc-unpinned-{name}.log"
        text = path.read_text(encoding="utf-8-sig")
        assert "Finished `dev` profile" in text and "error:" not in text
        keep(path, f"checks/{name}.log")
    for name in ("python-tests", "format", "source-audit", "device-gate"):
        suffix = "json" if name in ("source-audit", "device-gate") else "log"
        keep(LOGS / f"gc-unpinned-{name}.{suffix}", f"checks/{name}.{suffix}")
    audit = read(LOGS / "gc-unpinned-source-audit.json")
    assert audit["source_sync_pass"] and not audit["device_behavior_parity_pass"]
    assert not read(LOGS / "gc-unpinned-device-gate.json")["device_behavior_parity_pass"]
    inventory = build_dir / "source-inventory.json"
    committed_inventory = subprocess.check_output([GIT, "-C", str(ROOT), "show", commit + ":data/rustydroid-upstream.json"])
    assert inventory.read_bytes().replace(b"\r\n", b"\n") == committed_inventory
    keep(inventory, "source-inventory.json")
    catalog = read(FIXTURE / "catalog.json")
    ids = [row["id"] for row in catalog["cases"]]
    by_id = {row["id"]: row for row in catalog["cases"]}
    catalog_sha = sha(FIXTURE / "catalog.json")
    apk_sha = sha(FIXTURE / "AndroSwitchConformance.apk")
    keep(FIXTURE / "catalog.json", "fixture/catalog.json")
    keep(FIXTURE / "build-receipt.json", "fixture/build-receipt.json")
    keep(FIXTURE / "jvm-oracle.tsv", "fixture/jvm-oracle.tsv")
    keep(FIXTURE / "signature-verification.log", "fixture/signature-verification.log")
    pure = [row for row in catalog["cases"] if row["family"] == "jvm"]
    dex = read(LOGS / "gc-unpinned-135-report.json")
    expected = dict((int(line.split(chr(9), 1)[0]), line.split(chr(9), 1)[1])
                    for line in (FIXTURE / "jvm-oracle.tsv").read_text().splitlines())
    assert [r["id"] for r in dex["cases"]] == [r["id"] for r in pure]
    assert all(r["status"] == "PASS" and r["detail"] == expected[pure[i]["index"]]
               for i, r in enumerate(dex["cases"]))
    keep(LOGS / "gc-unpinned-135-report.json", "jvm-dex/report.json")
    keep(LOGS / "gc-unpinned-135-dex.log", "jvm-dex/execution.log")
    keep(LOGS / "collections-ui-request.json", "host-ui/request.json")
    host = []
    for prefix, group in (("gc-unpinned", "Collections"), ("gc-unpinned-text", "Text")):
        journal = LOGS / f"{prefix}-ui-storage/userdata/org.androswitch.conformance/files/conformance/report.json"
        rows = read(journal)
        assert rows["catalog_sha256"] == catalog_sha and [r["id"] for r in rows["cases"]] == ids
        selected = [r for r in rows["cases"] if by_id[r["id"]]["group"] == group]
        assert selected and all(r["status"] == "PASS" for r in selected)
        output = LOGS / f"{prefix}-host-ui.log"
        snapshots = read(output)["snapshots"]
        assert snapshots and any(v["text"] == "Selected run complete; other categories may be NOT_RUN"
                                 for v in snapshots[-1]["views"])
        timing_path = LOGS / f"{prefix}-host-ui-timing.json"
        timing = read(timing_path)
        assert timing["exit_code"] == 0 and timing["apk_sha256"] == apk_sha
        assert timing["request_sha256"] == sha(LOGS / "collections-ui-request.json")
        host.append(dict(group=group, passed=len(selected), complete=True,
                         elapsed_ms=timing["elapsed_ms"], heap_budget_bytes=64 << 20,
                         full_catalog_acceptance=False, horizon_evidence=False))
        for path, name in ((journal, "journal.json"), (output, "execution.log"), (timing_path, "timing.json")):
            keep(path, f"host-ui/{group}/{name}")
    for name in ("gc-pressure-before.log", "gc-looper-before.log", "gc-watermark-before.log",
                 "gc-pressure-host-ui.log", "gc-pressure-host-ui-timing.json",
                 "gc-looper-host-ui.log", "gc-looper-host-ui-timing.json"):
        keep(LOGS / name, "before/" + name)
    for prefix in ("gc-pressure", "gc-looper"):
        keep(LOGS / f"{prefix}-ui-storage/userdata/org.androswitch.conformance/files/conformance/report.json",
             f"before/{prefix}-journal.json")
    build = read(build_dir / "product-build.json")
    assert build["status"] == "built" and build["source_commit"] == commit
    assert sha(build_dir / "androswitch.nro") == build["nro_sha256"]
    for name in ("product-build.json", "product-source.json", "product-std-platform.json", "product-mesa-link.json"):
        keep(build_dir / name, "horizon/" + name)
    keep(LOGS / "gc-unpinned-nro-build.log", "horizon/build.log")
    emulator = []
    for run in runs:
        receipt, summary = read(run / "verification.json"), read(run / "conformance-summary.json")
        assert receipt["nro_sha256"] == build["nro_sha256"] and receipt["sd_original_restore"] == "restored"
        assert summary["apk_sha256"] == apk_sha and summary["catalog_sha256"] == catalog_sha
        journal = Path(receipt["sd_result"]) / "userdata/org.androswitch.conformance/files/conformance/report.json"
        rows = read(journal)
        assert [r["id"] for r in rows["cases"]] == ids
        selected = [r for r in rows["cases"] if r["status"] != "NOT_RUN"]
        groups = {by_id[r["id"]]["group"] for r in selected}
        expected = [r for r in catalog["cases"] if r["group"] in groups]
        passed = bool(selected) and [r["id"] for r in selected] == [r["id"] for r in expected] and all(r["status"] == "PASS" for r in selected)
        qualifies = passed and summary["launcher_completed"] and receipt["status"] == "execution_verified_visual_review_required"
        emulator.append(dict(run=run.name, groups=sorted(groups), selected_passed=sum(r["status"] == "PASS" for r in selected),
                             status=receipt["status"], qualifies_scoped_e2e_pass=qualifies,
                             launcher_completed=summary["launcher_completed"], sd_restore=receipt["sd_original_restore"],
                             visual_review="agent_reviewed_actual_completion_framebuffer" if visual_reviewed else "not_reviewed",
                             full_catalog_acceptance=summary["acceptance_passed"]))
        for name in ("verification.json", "conformance-summary.json", "conformance-report.json", "emulator.json", "launcher-test.json", "ryujinx.stdout.log", "ryujinx.stderr.log"):
            path = run / name
            if path.exists(): keep(path, f"emulator/{run.name}/{name}")
        for path in sorted((run / "sessions").rglob("*")):
            if path.is_file() and path.suffix in {".json", ".log", ".png"}:
                keep(path, f"emulator/{run.name}/sessions/" + path.relative_to(run / "sessions").as_posix())
        if visual_reviewed:
            assert list((run / "sessions").rglob("last-frame.png"))
    keep(Path(__file__), "retain-checkpoint.py")
    report = dict(schema_version=1, date="2026-10-27", status="scoped_gc_pressure_checkpoint",
                  implementation_commit=commit, upstream_revision=audit["upstream"],
                  attribution="RustyDroid by nadabgfccvd and contributors; history and licenses retained",
                  all_upstream_apis_1_to_1_on_horizon=False, api_percentage=None,
                  source_sync_pass=True, shipping_excluded_originals=audit["host_only_originals"],
                  source_inventory_scope="Source selection only; not an API denominator or behavior verification",
                  tests=tests, jvm_dex=dict(passed=len(pure), actual_values_match_frozen_jvm=True),
                  ordinary_host_ui=host, horizon_build=build, emulator=emulator,
                  physical_device="not_run_for_this_implementation",
                  prior_physical_device="219/219 user-reported for the earlier std candidate only; reports/conformance-std-hardware-20261026.json",
                  independent_games="deferred_until_complete_upstream_integration_per_user_request",
                  remaining_work="docs/UPSTREAM_PARITY_PLAN.md", implementation_notes="docs/UPSTREAM_GC_PRESSURE.md",
                  apk_sha256=apk_sha, catalog_sha256=catalog_sha, evidence=evidence)
    OUTPUT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Retained {len(evidence)} evidence files; full upstream parity remains false")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--emulator-run", type=Path, action="append", default=[])
    parser.add_argument("--visual-reviewed", action="store_true", help="Explicit agent assertion after viewing each captured completion framebuffer")
    args = parser.parse_args()
    generate(args.commit, args.build_dir, args.emulator_run, args.visual_reviewed)
