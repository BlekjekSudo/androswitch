#!/usr/bin/env python3
"""Record execution evidence without converting source/test counts into API parity.

The inherited API catalog documents subsets, not every Android overload. Keep
its scope and each unverified device surface visible, including zero-case tests.
This report is a checkpoint; it cannot certify device parity from host logs.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tomllib
from collections import deque
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUN = re.compile(r"^\s*Running (.+) \(([^\n]+)\)$")
RESULT = re.compile(
    r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out;"
)


def parse_cargo_log(text: str) -> list[dict]:
    suites, current = [], None
    pending = deque()
    for line in text.splitlines():
        candidate = None
        doc = re.match(r"^\s*Doc-tests (\S+)$", line)
        if doc:
            candidate = {"target": f"doc_{doc[1]}", "source": "doctests",
                       "status": "incomplete", "passed": 0, "failed": 0,
                       "ignored": 0, "measured": 0, "filtered": 0}
        start = RUN.match(line)
        if start:
            label, executable = start.groups()
            filename = executable.replace("\\", "/").rsplit("/", 1)[-1]
            target = re.sub(r"-[0-9a-f]{16}(?:\.exe)?$", "", filename)
            candidate = {"target": target, "source": label.replace("\\", "/"),
                       "status": "incomplete", "passed": 0, "failed": 0,
                       "ignored": 0, "measured": 0, "filtered": 0}
        if candidate is not None:
            # Cargo runs test binaries sequentially, but PowerShell's merged
            # stdout/stderr can expose the next stderr header before the prior
            # binary's stdout summary. Keep FIFO ownership; never attribute the
            # previous summary to the new header or edit the raw capture.
            if current is None:
                current = candidate
            else:
                pending.append(candidate)
        result = RESULT.match(line)
        if result:
            if current is None:
                raise ValueError("test summary without a Running target")
            status, *numbers = result.groups()
            current.update(zip(("passed", "failed", "ignored", "measured", "filtered"),
                               map(int, numbers)))
            current["status"] = (
                "failed" if status != "ok" or current["failed"] else
                "partial" if current["ignored"] or current["filtered"] else
                "zero_cases_unverified" if not current["passed"] else "passed"
            )
            suites.append(current)
            current = pending.popleft() if pending else None
    if current is not None:
        suites.append(current)
    suites.extend(pending)
    if not suites:
        raise ValueError("no test execution evidence")
    return suites


def verify_reference_routes(suites: list[dict], manifest: dict) -> list[str]:
    expected = {test["name"] for test in manifest.get("test", [])
                if "upstream-parity-tests" in test.get("required-features", [])}
    actual = [suite["target"] for suite in suites if suite["target"].startswith("upstream_")]
    errors = [f"missing upstream execution: {name}" for name in sorted(expected - set(actual))]
    errors += [f"unexpected upstream execution: {name}" for name in sorted(set(actual) - expected)]
    errors += [f"duplicated upstream execution: {name}" for name in sorted(set(actual))
               if actual.count(name) != 1]
    return errors


def log_evidence(path: Path, command: str) -> dict:
    raw = path.read_bytes()
    text = raw.decode("utf-8-sig")
    suites = parse_cargo_log(text)
    errors = [line for line in text.splitlines() if re.match(r"^error(?:\[|:)", line)]
    return {"log": path.as_posix(), "log_sha256": hashlib.sha256(raw).hexdigest(),
            "command": command, "suites": suites,
            "cargo_errors": errors,
            "test_run_completed_successfully": not errors and not any(s["status"] == "incomplete" for s in suites),
            "totals": {key: sum(suite[key] for suite in suites)
                       for key in ("passed", "failed", "ignored", "measured", "filtered")},
            "zero_case_targets": [s["target"] for s in suites
                                  if s["status"] == "zero_cases_unverified"],
            "incomplete_targets": [s["target"] for s in suites if s["status"] == "incomplete"]}


def report(root: Path, portable: dict, desktop: dict | None, horizon_log: Path,
           implementation_commit: str, source_audit: dict) -> dict:
    lock = json.loads((root / "data/rustydroid-upstream.json").read_text(encoding="utf-8"))
    manifest = tomllib.loads((root / "crates/as-vm/Cargo.toml").read_text(encoding="utf-8"))
    catalog = tomllib.loads((root / "data/api-coverage.toml").read_text(encoding="utf-8"))
    route_errors = verify_reference_routes(portable["suites"], manifest)
    raw = horizon_log.read_bytes()
    # A check log is retained as diagnostic evidence, never an NRO/runtime pass.
    horizon_check = b"Finished `dev` profile" in raw and b"error:" not in raw
    return {
        "schema": 1,
        "status": "not_yet_1_to_1",
        "all_upstream_apis_1_to_1_on_horizon": False,
        "upstream_repository": lock["repository"],
        "upstream_revision": lock["revision"],
        "implementation_commit": implementation_commit,
        "source_inventory": {key: source_audit[key] for key in (
            "source_sync_pass", "source_files", "identical_files", "adapted_files",
            "relocated_files", "errors", "host_only_originals_scope")},
        "original_vm_modules": source_audit["vm_modules"],
        "scope": "Complete pinned VM corpus on the portable reference path; product integration remains incomplete.",
        "portable_reference": {**portable, "route_errors": route_errors},
        "desktop_and_existing_runner_regressions": desktop,
        "horizon": {"verification": "core_compile_only" if horizon_check else "check_not_confirmed",
                    "log": horizon_log.as_posix(), "log_sha256": hashlib.sha256(raw).hexdigest(),
                    "command": "cargo +nightly check -p as-vm --features upstream-runtime,advanced-components --target aarch64-nintendo-switch-freestanding -Zbuild-std=core,alloc",
                    "nro_from_this_implementation": "not_built",
                    "emulator": "not_run_for_this_implementation",
                    "physical_device": "not_run_for_this_implementation"},
        "upstream_catalog_scope": "Original subset catalog; key_apis are examples, not an exhaustive overload inventory or execution coverage.",
        "upstream_api_surfaces": [
            {"id": s["id"], "prefix": s["prefix"], "upstream_status": s["status"],
             "upstream_notes": s["notes"], "upstream_sources": s.get("code", []),
             "key_apis": s.get("key_apis", []),
             "complete_horizon_behavior_parity": "unverified"}
            for s in catalog["surface"]
        ],
        "remaining_gaps": [
            "The shipping runner does not enable upstream-runtime. A passing standalone reference corpus does not prove the bound Horizon dispatch path.",
            "Original framework state and the Horizon adapter must be unified across lifecycle, views, collections, class/enum identities, callbacks and GC.",
            "The upstream reference filesystem remains in memory; Java/JNI/native must use the same persistent GuestFileSystem before enabling it on device.",
            "Native SQLite replaces the Python transport in the portable path; device persistence and resource-limit parity still need integration evidence.",
            "Cooperative reference threads, virtual clock/device models and desktop native/JIT backends are not physical Horizon services.",
            "Complete JNI overload metadata and native backend contract comparison remain unverified; zero-case native/JIT suites do not count as passes.",
            "Mekorama and Candy Crush have no confirmed first frame. Earlier NRO47 attempts and old hardware 219/219 apply only to their recorded binaries.",
            "SDK type declarations are metadata only and do not establish implementation of the declared APIs.",
        ],
        "attribution": "RustyDroid by nadabgfccvd and contributors; original history and MIT OR Apache-2.0 notices retained. See CREDITS.md and NOTICE.md.",
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--portable-log", type=Path, required=True)
    parser.add_argument("--regression-log", type=Path)
    parser.add_argument("--horizon-log", type=Path, required=True)
    parser.add_argument("--implementation-commit", required=True)
    parser.add_argument("--source-audit", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    subprocess.run(["git", "-C", str(ROOT), "cat-file", "-e", args.implementation_commit], check=True)
    portable = log_evidence(args.portable_log,
        "cargo test -p as-vm --features upstream-parity-tests --lib --tests --no-fail-fast -- --test-threads=1")
    desktop = (log_evidence(args.regression_log,
        "cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --no-fail-fast -- --test-threads=1 --include-ignored")
        if args.regression_log else None)
    source_audit = json.loads(args.source_audit.read_text(encoding="utf-8"))
    result = report(ROOT, portable, desktop, args.horizon_log, args.implementation_commit, source_audit)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    print(f"{result['status']}; portable: {portable['totals']}; missing routes: {len(result['portable_reference']['route_errors'])}")
