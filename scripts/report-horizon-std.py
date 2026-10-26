"""Review fresh std probe journals, including negative and shutdown contracts."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

EXPECTED = (
    "abi.sdk_layouts", "alloc.vec_string", "alloc.aligned", "hashmap.entropy",
    "io.write_read", "io.seek_metadata", "io.c_shared_file", "io.rename", "io.copy",
    "io.directory", "io.not_found", "io.exclusive_create", "io.sync_all",
    "time.monotonic_sleep", "time.system_clock", "thread.spawn_join",
    "thread.tls_destructors", "thread.repeated_join_tls", "thread.detached_handle",
    "sync.mutex_contention", "sync.condvar_notify", "sync.condvar_timeout",
    "sync.rwlock_once", "sync.channel_timeout", "sync.park_unpark",
    "env.set_get_remove", "thread.available_parallelism", "process.unsupported",
    "io.fresh_process_persistence",
)


def parse_cases(text):
    cases = {}
    for line in text.splitlines():
        key, status, detail = line.split("\t", 2)
        if key not in EXPECTED or key in cases or status not in ("pass", "fail"):
            raise ValueError(f"Unknown, duplicate or malformed case: {key}")
        cases[key] = {"id": key, "status": status, "detail": detail}
    return [cases.get(key, {"id": key, "status": "not_run", "detail": "missing journal entry"}) for key in EXPECTED]


def analyze(run, build_dir):
    run = Path(run)
    receipt = json.loads((run / "verification.json").read_text(encoding="utf-8-sig"))
    session = receipt["session_id"]
    if not session.isalnum():
        raise ValueError("Invalid run/session identity")
    build = json.loads((build_dir / "build.json").read_text(encoding="utf-8-sig"))
    frozen_nro = run / "androswitch-std-probe.nro"
    digest = hashlib.sha256(frozen_nro.read_bytes()).hexdigest()
    if digest != receipt["nro_sha256"] or digest != build["nro_sha256"]:
        raise ValueError("Build and executed artifact hashes do not match")
    if hashlib.sha256((build_dir / "source.json").read_bytes()).hexdigest() != build["source_manifest_sha256"]:
        raise ValueError("Source manifest hash mismatch")
    base = run / "sd-result"
    data = base / session
    path = data / "results.tsv"
    cases = parse_cases(path.read_text(encoding="utf-8") if path.exists() else "")
    for mode in ("suite", "restart"):
        expected_failures = sum(case["status"] == "fail" for case in cases if (case["id"] == "io.fresh_process_persistence") == (mode == "restart"))
        complete = data / f"complete-{mode}.txt"
        returned = data / f"returned-{mode}.txt"
        shutdown = data / f"shutdown-{mode}.txt"
        ok = complete.exists() and complete.read_text().splitlines() == [session, mode, str(expected_failures)]
        ok = ok and returned.exists() and returned.read_text().strip() == str(expected_failures)
        ok = ok and shutdown.exists() and shutdown.read_text().strip() == "C atexit reached after Rust returned"
        cases.append({"id": f"shutdown.{mode}", "status": "pass" if ok else "fail", "detail": "fresh completion, actual Rust return and C atexit must agree"})
    panic = data / "panic.txt"
    aborted = panic.exists() and "intentional std probe panic: abort policy" in panic.read_text()
    aborted = aborted and not (data / "returned-panic.txt").exists() and not (data / "shutdown-panic.txt").exists()
    # Exit recorded by the actual guest OS path, not just a file written before panic.
    log = run / "panic.stdout.log"
    aborted = aborted and log.exists() and "ServiceAm Exit:" in log.read_text(encoding="utf-8", errors="replace")
    cases.append({"id": "panic.abort", "status": "pass" if aborted else "fail", "detail": "hook recorded intentional panic, guest exited, C return/atexit not reached"})
    counts = Counter(case["status"] for case in cases)
    phases = receipt.get("phases", [])
    phases_ok = len(phases) == 3 and [p.get("mode") for p in phases] == ["suite", "restart", "panic"] and all(p.get("marker_observed") is True for p in phases)
    complete = phases_ok and receipt.get("sd_restore") == "restored" and not counts["not_run"]
    return {"schema_version": 1, "session_id": session, "status": "complete_with_gaps" if complete and counts["fail"] else "passed_probe_contracts" if complete else "incomplete",
            "summary": {key: counts[key] for key in ("pass", "fail", "not_run")},
            "cases": cases, "build": build, "execution": receipt,
            "physical_device": "not_run", "product_runner_changed": False,
            "android_api_parity": "not_established_by_this_probe",
            "scope": "real std standalone libnx NRO; finite smoke contracts, not complete std certification",
            "recommendation": "qualify_std_product_regressions_separately" if complete and not counts["fail"]
                              else "resolve_reported_std_failures_before_product_migration"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--require-pass", action="store_true", help="Fail unless every finite probe contract passed")
    args = parser.parse_args()
    report = analyze(args.run, args.build)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], **report["summary"]}))
    if args.require_pass and (report["status"] != "passed_probe_contracts"
                              or report["summary"]["fail"] or report["summary"]["not_run"]):
        raise SystemExit("std probe acceptance failed; preserve the reported failures")


if __name__ == "__main__":
    main()
