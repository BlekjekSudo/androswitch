#!/usr/bin/env python3
"""Retain common-owner port evidence without claiming whole upstream parity."""
import argparse
import hashlib
import importlib.util
import json
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("parity", ROOT / "scripts/report-upstream-parity.py")
PARITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARITY)


def generate(logs, fixture, output, commit, target_build=None, emulator_runs=()):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Full implementation commit required")
    kind = subprocess.check_output(["git", "-C", str(ROOT), "cat-file", "-t", commit], text=True).strip()
    if kind != "commit":
        raise ValueError("Implementation object is not a commit")
    retained = output.with_suffix("")
    retained.mkdir(parents=True, exist_ok=True)
    evidence = []

    def keep(path, name):
        data = path.read_bytes()
        dest = retained / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        evidence.append(dict(path=dest.relative_to(ROOT).as_posix(), source=str(path),
                             sha256=hashlib.sha256(data).hexdigest(), bytes=len(data)))

    commands = {
        "upstream-tests": "cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1",
        "product-tests": "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored",
    }
    tests = {}
    for label, command in commands.items():
        path = logs / f"collection-text-final-{label}.log"
        record = PARITY.log_evidence(path, command)
        if (not record["test_run_completed_successfully"] or record["totals"]["failed"]
                or record["totals"]["ignored"] or record["totals"]["filtered"]):
            raise ValueError(f"Incomplete/failing corpus: {label}")
        tests[label] = record
        keep(path, path.name)
    manifest = tomllib.loads((ROOT / "crates/as-vm/Cargo.toml").read_text(encoding="utf-8"))
    errors = PARITY.verify_reference_routes(tests["upstream-tests"]["suites"], manifest)
    if errors:
        raise ValueError(errors)
    product_cases = [s for s in tests["product-tests"]["suites"] if s["target"] == "collection_factories"]
    if len(product_cases) != 1 or product_cases[0]["passed"] != 11:
        raise ValueError("Eleven actual product state/GC/unwind contracts required")
    for label in ("workspace-clippy", "upstream-clippy", "core-check"):
        path = logs / f"collection-text-final-{label}.log"
        text = path.read_text(encoding="utf-8-sig")
        if "Finished `dev` profile" not in text or re.search(r"^error(?:\[|:)", text, re.M):
            raise ValueError(f"Check did not pass: {label}")
        keep(path, path.name)
    audit = json.loads((logs / "collection-text-final-source-audit.json").read_bytes())
    if not audit["source_sync_pass"]:
        raise ValueError("Reviewed source gate required")
    keep(logs / "collection-text-final-source-audit.json", "source-audit.json")
    keep(ROOT / "data/rustydroid-upstream.json", "source-inventory.json")
    catalog = json.loads((fixture / "catalog.json").read_bytes())
    pure = [row for row in catalog["cases"] if row["family"] == "jvm"]
    host = json.loads((logs / "collection-text-135-report.json").read_bytes())
    if ([r["id"] for r in host["cases"]] != [r["id"] for r in pure]
            or not all(r["status"] == "PASS" for r in host["cases"])):
        raise ValueError("Every actual JVM/DEX case must pass with matching identities")
    for name in ("catalog.json", "build-receipt.json", "jvm-oracle.tsv", "signature-verification.log"):
        keep(fixture / name, "fixture/" + name)
    for name in ("collection-text-135-report.json", "collection-text-final-135-dex.log",
                 "collection-text-final-python.log", "collection-text-format.log"):
        keep(logs / name, name)
    # Retain observed failures; later passing evidence must not erase them.
    for name in ("collection-text-upstream-tests.log", "collection-family-host-report.json",
                 "collection-text-pure-report.json", "collection-text-complete-report.json",
                 "collection-text-cycles-focused.log", "collection-text-product-regression-before.log"):
        path = logs / name
        if path.exists():
            keep(path, "before/" + name)
    apk_hash = hashlib.sha256((fixture / "AndroSwitchConformance.apk").read_bytes()).hexdigest()
    target = None
    if target_build:
        target = json.loads(target_build.read_bytes())
        if target["source_commit"] != commit:
            raise ValueError("Target receipt is from a different implementation")
        keep(target_build, "horizon/product-build.json")
        for name in ("product-source.json", "product-std-platform.json", "product-mesa-link.json"):
            path = target_build.parent / name
            if path.exists():
                keep(path, "horizon/" + name)
    emulator = []
    for run in emulator_runs:
        receipt = json.loads((run / "verification.json").read_bytes())
        summary = json.loads((run / "conformance-summary.json").read_bytes())
        if (target is None or receipt["nro_sha256"] != target["nro_sha256"]
                or receipt["sd_original_restore"] != "restored"
                or summary["apk_sha256"] != apk_hash):
            raise ValueError("Emulator must match NRO/APK and restore original SD")
        journal = Path(receipt["sd_result"]) / "userdata/org.androswitch.conformance/files/conformance/report.json"
        rows = json.loads(journal.read_bytes())["cases"]
        selected = [r for r in rows if r["status"] != "NOT_RUN"]
        expected = [r for r in catalog["cases"] if r["group"] in {s["group"] for s in selected}]
        if (not selected or not all(s["status"] == "PASS" for s in selected)
                or [r["id"] for r in selected] != [r["id"] for r in expected]):
            raise ValueError("Selected emulator groups are incomplete or not all passing")
        emulator.append(dict(run=run.name, groups=sorted({s["group"] for s in selected}),
                             selected_passed=len(selected), total_catalog=len(rows),
                             full_catalog_acceptance=summary["acceptance_passed"]))
        keep(journal, f"emulator/{run.name}/report.json")
        for name in ("verification.json", "conformance-summary.json", "emulator.json", "launcher-test.json",
                     "ryujinx.stdout.log", "ryujinx.stderr.log"):
            path = run / name
            if path.exists(): keep(path, f"emulator/{run.name}/{name}")
        for path in sorted((run / "sessions").rglob("*")):
            if path.is_file() and path.suffix in {".json", ".log"}:
                keep(path, f"emulator/{run.name}/sessions/" + path.relative_to(run / "sessions").as_posix())
    report = dict(schema_version=1, date="2026-10-26", status="scoped_integration_checkpoint",
        implementation_commit=commit, upstream_revision=audit["upstream"],
        upstream_credit="RustyDroid by nadabgfccvd and contributors; history/licenses retained",
        all_upstream_apis_1_to_1_on_horizon=False, api_percentage=None,
        shipping_excluded_originals=audit["host_only_originals"],
        shipping_excluded_scope="Source selection, not API denominator; every original module still behavior-unverified in the source audit",
        shared_original_modules=["jcollections", "jiterator", "collections", "jlstring", "jsb", "jregex"],
        tests=tests, jvm_dex=dict(passed=len(pure), apk_sha256=apk_hash, total_catalog=len(catalog["cases"])),
        horizon_build=target, emulator=emulator, physical_device="not_run_for_this_implementation",
        prior_hardware_report="reports/conformance-std-hardware-20261026.json; old std candidate only",
        independent_games="deferred_until_complete_upstream_integration_per_user_request",
        limitations_document="docs/UPSTREAM_COLLECTIONS_TEXT.md", evidence=evidence)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--logs", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--implementation-commit", required=True)
    parser.add_argument("--target-build", type=Path)
    parser.add_argument("--emulator-run", type=Path, action="append", default=[])
    args = parser.parse_args()
    result = generate(args.logs, args.fixture, args.output, args.implementation_commit,
                      args.target_build, args.emulator_run)
    print(f"Retained {len(result['evidence'])} inputs/outputs; complete upstream API parity remains false")
