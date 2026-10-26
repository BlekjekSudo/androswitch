"""Retain exact shared-I/O evidence without granting whole-runtime parity."""
import argparse
import hashlib
import importlib.util
import json
import subprocess
import tomllib
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = Path(__file__).resolve().parent
LOGS = ROOT / "local-artifacts/upstream-file"
FIXTURE = ROOT / "local-artifacts/conformance-io-qualified"
GIT = "C:/Program Files/Git/cmd/git.exe"
SPEC = importlib.util.spec_from_file_location("parity", ROOT / "scripts/report-upstream-parity.py")
PARITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARITY)


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def retain(commit, build_dir, runs, visual_reviewed):
    evidence = []

    def keep(path, relative):
        raw = path.read_bytes()
        target = DEST / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
        evidence.append(dict(path=target.relative_to(ROOT).as_posix(), bytes=len(raw),
                             sha256=hashlib.sha256(raw).hexdigest()))

    tests = {}
    commands = {
        "builder-product": "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored",
        "builder-reference": "cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1",
        "builder-shipping": "cargo test -p as-vm --lib --test shipping_io_chars --locked -- --test-threads=1",
        "builder-gc": "cargo test -p as-vm --features portable-test-support --test shipping_gc_generations --locked -- --test-threads=1",
    }
    for name, command in commands.items():
        path = LOGS / (name + ".log")
        tests[name] = PARITY.log_evidence(path, command)
        row = tests[name]
        assert row["test_run_completed_successfully"] and row["totals"]["passed"] > 0
        assert not any(row["totals"][k] for k in ("failed", "ignored", "filtered"))
        keep(path, "tests/" + path.name)
    manifest = tomllib.loads((ROOT / "crates/as-vm/Cargo.toml").read_text())
    assert not PARITY.verify_reference_routes(tests["builder-reference"]["suites"], manifest)
    for name in ("builder-workspace-clippy", "builder-reference-clippy", "builder-core"):
        path = LOGS / (name + ".log")
        text = path.read_text(encoding="utf-8-sig")
        assert "Finished `dev` profile" in text and "error:" not in text
        keep(path, "checks/" + path.name)
    for name in ("builder-format.log", "builder-python.log", "builder-source.json", "builder-device-gate.json"):
        keep(LOGS / name, "checks/" + name)
    checks = read(LOGS / "builder-checks.json")
    assert len(checks) == 10 and all(row["exit_code"] == row["expected_exit"] for row in checks)
    assert all(sha(Path(row["log"])) == row["sha256"] for row in checks)
    keep(LOGS / "builder-checks.json", "checks/commands.json")
    source = read(LOGS / "builder-source.json")
    assert source["source_sync_pass"] and not source["device_behavior_parity_pass"]
    assert not read(LOGS / "builder-device-gate.json")["device_behavior_parity_pass"]
    build = read(build_dir / "product-build.json")
    assert build["source_commit"] == commit and build["status"] == "built"
    assert sha(build_dir / "androswitch.nro") == build["nro_sha256"]
    frozen = subprocess.check_output([GIT, "-C", str(ROOT), "show", commit + ":data/rustydroid-upstream.json"])
    assert (build_dir / "source-inventory.json").read_bytes().replace(b"\r\n", b"\n") == frozen
    for name in ("product-build.json", "product-source.json", "product-std-platform.json", "product-mesa-link.json", "source-inventory.json"):
        keep(build_dir / name, "horizon/" + name)
    keep(LOGS / "builder-nro-build.log", "horizon/build.log")
    for name in ("catalog.json", "build-receipt.json", "jvm-oracle.tsv", "signature-verification.log"):
        keep(FIXTURE / name, "fixture/" + name)
    catalog = read(FIXTURE / "catalog.json")
    ids = [r["id"] for r in catalog["cases"]]
    assert len(ids) == len(set(ids)) == 274
    pure = [r for r in catalog["cases"] if r["family"] == "jvm"]
    expected = dict((int(line.split("\t", 1)[0]), line.split("\t", 1)[1])
                    for line in (FIXTURE / "jvm-oracle.tsv").read_text(encoding="utf-8").splitlines())
    host = read(LOGS / "builder-sdk-report.json")
    actual = {r["id"]: r for r in host["cases"]}
    assert len(pure) == 148 and all(actual[r["id"]]["status"] == "PASS" and actual[r["id"]]["detail"] == expected[r["index"]] for r in pure)
    keep(LOGS / "builder-sdk-report.json", "host/report.json")
    keep(LOGS / "builder-sdk.log", "host/execution.log")
    journal_path = LOGS / "host-io-ui-storage/userdata/org.androswitch.conformance/files/conformance/report.json"
    journal = read(journal_path)
    assert journal["catalog_sha256"] == sha(FIXTURE / "catalog.json")
    assert Counter(r["status"] for r in journal["cases"]) == {"PASS": 17, "NOT_RUN": 257}
    snapshots = read(LOGS / "host-io-ui-snapshots.json")["snapshots"]
    assert any(v["text"] == "Selected run complete; other categories may be NOT_RUN" for v in snapshots[-1]["views"])
    keep(journal_path, "host-ui/journal.json")
    keep(LOGS / "host-io-ui-request.json", "host-ui/request.json")
    keep(LOGS / "host-io-ui.log", "host-ui/execution.log")
    for name in ("jvm.tsv", "receipt.json"):
        keep(ROOT / "local-artifacts/builder-utf16" / name, "builder-oracle/" + name)
    emulators = []
    for run in runs:
        receipt, summary = read(run / "verification.json"), read(run / "conformance-summary.json")
        assert receipt["nro_sha256"] == build["nro_sha256"]
        assert receipt["sd_original_restore"] == "restored"
        assert summary["apk_sha256"] == sha(FIXTURE / "AndroSwitchConformance.apk")
        assert summary["catalog_sha256"] == sha(FIXTURE / "catalog.json")
        device = read(run / "conformance-report.json")
        assert [r["id"] for r in device["cases"]] == ids
        counts = Counter(r["status"] for r in device["cases"])
        selected = counts == {"PASS": 17, "NOT_RUN": 257}
        full = counts == {"PASS": 271, "NOT_RUN": 3}
        assert selected or full, counts
        if full:
            assert {r["id"] for r in device["cases"] if r["status"] == "NOT_RUN"} == {
                "host.cpu.clock_hz", "host.gpu.clock_hz", "host.memory.clock_hz"}
        emulators.append(dict(run=run.name, counts=dict(counts), scope="I/O" if selected else "available SDK baseline", full_catalog_acceptance=False))
        for name in ("verification.json", "conformance-summary.json", "conformance-report.json", "emulator.json", "launcher-test.json"):
            keep(run / name, "emulator/" + run.name + "/" + name)
        for path in run.rglob("*"):
            if path.is_file() and path.suffix in (".log", ".png"):
                keep(path, "emulator/" + run.name + "/" + path.relative_to(run).as_posix())
        from PIL import Image
        for capture in receipt["cases"][0]["captures"]:
            ppm = Path(capture["file"])
            png = ppm.with_suffix(".png")
            assert sha(ppm) == capture["sha256"]
            with Image.open(ppm) as original, Image.open(png) as retained:
                assert original.size == retained.size and original.tobytes() == retained.tobytes()
    assert len(emulators) >= 2 and any(e["counts"].get("PASS") == 271 for e in emulators)
    keep(LOGS / "builder-product-before-oom-contract.log", "diagnostics/product-before-oom-contract.log")
    before = ROOT / "local-artifacts/m9/external-runs/b27b076cfe4545218ba6a95dc1121311"
    prior = read(before / "verification.json")
    assert prior["status"] == "blocked_runtime" and prior["sd_original_restore"] == "restored"
    for name in ("verification.json", "conformance-summary.json", "conformance-report.json"):
        keep(before / name, "diagnostics/before-builder-fix/" + name)
    report = dict(schema=1, status="not_yet_1_to_1", all_upstream_apis_1_to_1_on_horizon=False,
                  upstream_repository="https://github.com/nadabgfccvd/RustyDroid",
                  upstream_revision=read(ROOT / "data/rustydroid-upstream.json")["revision"],
                  implementation_commit=commit, tests=tests, source_audit=source,
                  horizon_build=build, emulator=emulators, visual_reviewed=visual_reviewed,
                  physical_device="not_run_for_this_binary", games_first_frame="not_proved",
                  diagnostics_scope="Retained failures predate the final binary and are not pass evidence.",
                  host_sdk_counts=dict(Counter(r["status"] for r in host["cases"])),
                  ordinary_host_io_ui=dict(passed=17, complete=True, implementation_commit="d8542f81669e1edf4db0c7485fe7814e74596920"),
                  open_boundaries_document="docs/UPSTREAM_PARITY_PLAN.md", evidence=evidence)
    output = DEST.with_suffix(".json")
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(dict(report=str(output), tests={k:v["totals"] for k,v in tests.items()}, emulator=emulators)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--run", type=Path, action="append", required=True)
    parser.add_argument("--visual-reviewed", action="store_true")
    args = parser.parse_args()
    retain(args.commit, args.build_dir, args.run, args.visual_reviewed)
