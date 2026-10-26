#!/usr/bin/env python3
"""Retain scoped collection integration evidence without declaring API parity."""
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


def generate(logs, output, commit):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Full implementation commit required")
    subprocess.run(["git", "-C", str(ROOT), "cat-file", "-e", commit + "^{commit}"], check=True)
    retained = output.with_suffix("")
    retained.mkdir(parents=True, exist_ok=True)
    evidence = []

    def keep(source, name):
        raw = source.read_bytes()
        data = ("\n".join(s.rstrip() for s in raw.decode("utf-8-sig").splitlines()).rstrip() + "\n").encode()
        dest = retained / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        evidence.append(dict(path=dest.relative_to(ROOT).as_posix(), source=str(source),
                             raw_sha256=hashlib.sha256(raw).hexdigest(), sha256=hashlib.sha256(data).hexdigest()))

    upstream = PARITY.log_evidence(logs / "collections-upstream-bounded-tests.log",
        "cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1")
    product = PARITY.log_evidence(logs / "collections-product-bounded-tests.log",
        "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored")
    for record in (upstream, product):
        if not record["test_run_completed_successfully"] or record["totals"]["failed"] or record["totals"]["ignored"]:
            raise ValueError("Complete passing upstream/product logs are required")
    manifest = tomllib.loads((ROOT / "crates/as-vm/Cargo.toml").read_text())
    errors = PARITY.verify_reference_routes(upstream["suites"], manifest)
    if errors:
        raise ValueError(errors)
    factory = [s for s in product["suites"] if s["target"] == "collection_factories"]
    if len(factory) != 1 or factory[0]["passed"] != 4:
        raise ValueError("Actual four-case product collection integration target required")
    audit = json.loads((logs / "collections-upstream-source-audit.json").read_text(encoding="utf-8-sig"))
    if not audit["source_sync_pass"]:
        raise ValueError("Reviewed source synchronization required")
    for name in ("collections-workspace-bounded-clippy.log", "collections-upstream-bounded-clippy.log"):
        text = (logs / name).read_text(encoding="utf-8-sig")
        if "Finished `dev` profile" not in text or "\nerror" in text:
            raise ValueError(f"Clippy did not pass: {name}")
    for name in ("collections-upstream-bounded-tests.log", "collections-product-bounded-tests.log",
                 "collections-workspace-bounded-clippy.log", "collections-upstream-bounded-clippy.log",
                 "collections-upstream-source-audit.json", "collections-upstream-source-audit.log",
                 "collections-upstream-tests.log", "collections-bridge-check.log"):
        keep(logs / name, name)
    for name in ("CollectionsFactoryOracle.java", "results.txt", "java-version.txt", "compile.log"):
        keep(logs / "collections-oracle" / name, "jvm/" + name)
    oracle = dict(line.split("=", 1) for line in (logs / "collections-oracle/results.txt").read_text().splitlines())
    if len(oracle) != 14 or oracle["unmodifiableList.sizeAfterSourceClear"] != "0":
        raise ValueError("Complete JVM observation rows required")
    report = dict(schema_version=1, status="scoped_collection_factory_integration",
        implementation_commit=commit, upstream_revision=audit["upstream"],
        all_upstream_apis_1_to_1_on_horizon=False,
        scope=dict(device_factory_overloads=9, static_fields=3, original_factory_lines_reused=137,
                   backend="existing active collection store; no fallback to disconnected HostState",
                   complete_collections_family=False),
        host=dict(reference=upstream, product=product, original_route_errors=errors,
                  environment=dict(CARGO_BUILD_JOBS="1", CARGO_INCREMENTAL="0",
                                   CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0"),
                  jvm_observations=oracle),
        limitations=["Inherited unmodifiable wrappers are snapshots; JVM live-view behavior differs.",
                     "Sorted factories remain reference-only because their inherited return markers are plain Set/Map.",
                     "Synchronized no-op wrappers are not exposed as real Horizon synchronization.",
                     "singletonMap, other Collections utilities, remaining constructors/bulk operations/iterators and full framework state remain integration work.",
                     "Zero-case native/JIT targets remain unverified; test totals are executions, not API coverage percentages.",
                     "Initial default-debug full-reference compilation exhausted compiler memory; preserved as failure.",
                     "Initial metadata-only host check failed on a core metadata stub; preserved separately from execution results."],
        current_hardware_candidate=dict(changed=False, nro_sha256="a3a34bab32cf31e182fb72efe524bad1e06b5a69107853e9a0aba893aaef2bf8"),
        horizon="not_run_for_this_source_checkpoint", emulator="not_run_for_this_source_checkpoint",
        physical_device="not_run_for_this_source_checkpoint", evidence=evidence)
    output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--logs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--implementation-commit", required=True)
    args = parser.parse_args()
    result = generate(args.logs.resolve(), args.output.resolve(), args.implementation_commit)
    print(result["status"], result["host"]["reference"]["totals"], result["host"]["product"]["totals"])
