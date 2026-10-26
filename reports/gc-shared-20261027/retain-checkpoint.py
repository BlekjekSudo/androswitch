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
OUTPUT = ROOT / "reports/gc-shared-20261027.json"
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
        "shipping-contracts": "cargo test -p as-vm --features portable-test-support --test shipping_gc_generations --locked -- --test-threads=1",
        "upstream-tests": "cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1",
        "product-tests": "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored",
        "portable-lib": "cargo test -p as-vm --lib --locked -- --test-threads=1",
    }
    for name, command in commands.items():
        path = LOGS / f"gc-shared-{name}.log"
        record = PARITY.log_evidence(path, command)
        assert record["test_run_completed_successfully"] and record["totals"]["passed"] > 0
        assert not any(record["totals"][key] for key in ("failed", "ignored", "filtered"))
        tests[name] = record
        keep(path, f"tests/{name}.log")
    manifest = tomllib.loads((ROOT / "crates/as-vm/Cargo.toml").read_text())
    assert not PARITY.verify_reference_routes(tests["upstream-tests"]["suites"], manifest)
    for name in ("workspace-clippy", "upstream-clippy", "core-check"):
        path = LOGS / f"gc-shared-{name}.log"
        text = path.read_text(encoding="utf-8-sig")
        assert "Finished `dev` profile" in text and "error:" not in text
        keep(path, f"checks/{name}.log")
    for name in ("python-tests", "format", "source-audit", "device-gate"):
        suffix = "json" if name in ("source-audit", "device-gate") else "log"
        keep(LOGS / f"gc-shared-{name}.{suffix}", f"checks/{name}.{suffix}")
    audit = read(LOGS / "gc-shared-source-audit.json")
    assert audit["source_sync_pass"] and not audit["device_behavior_parity_pass"]
    assert not read(LOGS / "gc-shared-device-gate.json")["device_behavior_parity_pass"]
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
    dex = read(LOGS / "gc-shared-135-report.json")
    expected = dict((int(line.split(chr(9), 1)[0]), line.split(chr(9), 1)[1])
                    for line in (FIXTURE / "jvm-oracle.tsv").read_text().splitlines())
    assert [r["id"] for r in dex["cases"]] == [r["id"] for r in pure]
    assert all(r["status"] == "PASS" and r["detail"] == expected[pure[i]["index"]]
               for i, r in enumerate(dex["cases"]))
    keep(LOGS / "gc-shared-135-report.json", "jvm-dex/report.json")
    keep(LOGS / "gc-shared-135-dex.log", "jvm-dex/execution.log")
    keep(LOGS / "collections-ui-request.json", "host-ui/request.json")
    host = []
    for prefix, group in (("gc-shared", "Collections"), ("gc-shared-text", "Text")):
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
    for name in ("contract-provenance.json", "host-semantics.json", "harness-validation.json"):
        keep(LOGS / ("gc-shared-" + name), "contracts/" + name)
    build = read(build_dir / "product-build.json")
    assert build["status"] == "built" and build["source_commit"] == commit
    assert sha(build_dir / "androswitch.nro") == build["nro_sha256"]
    for name in ("product-build.json", "product-source.json", "product-std-platform.json", "product-mesa-link.json"):
        keep(build_dir / name, "horizon/" + name)
    keep(LOGS / "gc-shared-nro-build.log", "horizon/build.log")
    emulator = []
    declared_receipts = {str((run / "verification.json").resolve()).casefold(): run for run in runs}
    for run in runs:
        receipt, summary = read(run / "verification.json"), read(run / "conformance-summary.json")
        assert receipt["nro_sha256"] == build["nro_sha256"] and receipt["sd_original_restore"] == "restored"
        assert summary["apk_sha256"] == apk_sha and summary["catalog_sha256"] == catalog_sha
        journal = Path(receipt["sd_result"]) / "userdata/org.androswitch.conformance/files/conformance/report.json"
        rows = read(journal)
        assert [r["id"] for r in rows["cases"]] == ids
        selected = [r for r in rows["cases"] if r["status"] != "NOT_RUN"]
        groups = {by_id[r["id"]]["group"] for r in selected}
        controls = {r["relative"].rsplit("/", 1)[-1]: r for r in receipt.get("userdata_seed", [])}
        resumed_from, seed_cases = None, 0
        if "resume-evidence.json" in controls:
            seed = controls["resume-evidence.json"]
            resume_path = Path(seed["frozen"])
            assert sha(resume_path) == seed["sha256"]
            resume = read(resume_path)
            parent_path = Path(resume["receipt"])
            parent_run = declared_receipts[str(parent_path.resolve()).casefold()]
            assert sha(parent_path) == resume["receipt_sha256"]
            assert resume["nro_sha256"] == build["nro_sha256"] and resume["apk_sha256"] == apk_sha
            parent = read(parent_path)
            assert parent["sd_original_restore"] == "restored"
            prior_journal = read(Path(parent["sd_result"]) / "userdata/org.androswitch.conformance/files/conformance/report.json")
            prior_rows = {row["id"]: row for row in prior_journal["cases"]}
            for item in receipt["userdata_seed"]:
                name = item["relative"].rsplit("/", 1)[-1]
                if name.startswith("case-") and name.endswith(".json"):
                    path = Path(item["frozen"])
                    assert sha(path) == item["sha256"]
                    row = read(path)
                    assert row == prior_rows[row["id"]]
                    seed_cases += 1
            resumed_from = parent_run.name
            keep(resume_path, f"emulator/{run.name}/controls/resume-evidence.json")
        requested_group = "All"
        excluded = set()
        for name in ("filter.txt", "exclude.txt"):
            if name not in controls:
                continue
            seed = controls[name]
            raw = Path(seed["frozen"]).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == seed["sha256"]
            keep(Path(seed["frozen"]), f"emulator/{run.name}/controls/{name}")
            if name == "filter.txt":
                requested_group = raw.decode().strip() or "All"
            else:
                excluded = set(raw.decode().splitlines())
                assert excluded == {"host.cpu.clock_hz", "host.gpu.clock_hz", "host.memory.clock_hz"}
        expected = [r for r in catalog["cases"] if (requested_group == "All" or r["group"] == requested_group) and r["id"] not in excluded]
        passed = bool(selected) and [r["id"] for r in selected] == [r["id"] for r in expected] and all(r["status"] == "PASS" for r in selected)
        assert all(r["status"] == "NOT_RUN" for r in rows["cases"] if r["id"] in excluded)
        qualifies = passed and summary["launcher_completed"] and receipt["status"] == "execution_verified_visual_review_required"
        emulator.append(dict(run=run.name, requested_group=requested_group, observed_groups=sorted(groups),
                             explicit_exclusions=sorted(excluded), expected_available_cases=len(expected),
                             selected_passed=sum(r["status"] == "PASS" for r in selected),
                             status=receipt["status"], qualifies_available_emulator_cases=qualifies,
                             launcher_completed=summary["launcher_completed"], sd_restore=receipt["sd_original_restore"],
                             resumed_from_run=resumed_from, verified_seed_case_results=seed_cases,
                             visual_review="agent_reviewed_actual_captured_framebuffer" if visual_reviewed else "not_reviewed",
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
    package = read(LOGS / "gc-shared-package.json")
    assert package["manifest"]["source_commit"] == commit
    assert package["manifest"]["nro_sha256"] == build["nro_sha256"]
    assert package["manifest"]["apk_sha256"] == apk_sha
    assert sha(Path(package["archive"])) == package["sha256"]
    assert package["manifest"]["diagnostic"] and package["manifest"]["hardware"] == "not_run"
    keep(LOGS / "gc-shared-package.json", "package.json")
    report = dict(schema_version=1, date="2026-10-27", status="shared_original_collector_checkpoint",
                  implementation_commit=commit, upstream_revision=audit["upstream"],
                  attribution="RustyDroid by nadabgfccvd and contributors; history and licenses retained",
                  all_upstream_apis_1_to_1_on_horizon=False, api_percentage=None,
                  source_sync_pass=True, shipping_excluded_originals=audit["host_only_originals"],
                  source_inventory_scope="Source selection only; not an API denominator or behavior verification",
                  prior_pressure_checkpoint="reports/gc-pressure-20261027.json",
                  collector="Original crates/rd-vm/src/gc.rs on product and reference profiles; state adapters remain explicit",
                  reference_api_scope="Collector/heap contracts do not establish every public java.lang.ref overload",
                  tests=tests, jvm_dex=dict(passed=len(pure), actual_values_match_frozen_jvm=True),
                  ordinary_host_ui=host, horizon_build=build, emulator=emulator,
                  physical_device="not_run_for_this_implementation",
                  diagnostic_bundle=dict(archive=package["archive"], sha256=package["sha256"],
                                         hardware="not_run", full_catalog_acceptance=False),
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
    parser.add_argument("--visual-reviewed", action="store_true", help="Explicit agent assertion after viewing each captured framebuffer; catalog completeness remains a separate gate")
    args = parser.parse_args()
    generate(args.commit, args.build_dir, args.emulator_run, args.visual_reviewed)
