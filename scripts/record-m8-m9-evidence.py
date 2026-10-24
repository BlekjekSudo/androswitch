"""Record measured candidate evidence; missing Horizon gates remain missing."""
import datetime
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", default="reports/m8-m9-candidate.json", help="New report path relative to repository root")
parser.add_argument("--physical-test-first", action="store_true", help="Record an explicit request to test this candidate on physical Switch next")
args = parser.parse_args()
output = (root / args.output).resolve()
if not output.is_relative_to(root):
    parser.error("output must stay inside the repository")
if args.output != "reports/m8-m9-candidate.json" and output.exists():
    parser.error("choose a fresh report path; existing observations will not be replaced")
git = os.environ.get("GIT_EXECUTABLE", "C:/Program Files/Git/cmd/git.exe" if os.name == "nt" else "git")
def command(*args):
    return subprocess.check_output([git, *args], cwd=root).decode().strip()
def read(path):
    return json.loads((root / path).read_text(encoding="utf-8-sig"))
def sha(path):
    with (root / path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()
def relative(path):
    return Path(path).resolve().relative_to(root).as_posix()
def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered)-1, int((len(ordered)-1)*fraction))]

source_roots = ["Cargo.toml", "Cargo.lock", "crates", "platform", "fixtures", "scripts", "third_party"]
if subprocess.call([git, "diff", "--quiet", "HEAD", "--", *source_roots], cwd=root):
    raise SystemExit("Commit implementation changes before freezing evidence")
untracked = command("ls-files", "--others", "--exclude-standard", "--", *source_roots)
if untracked:
    raise SystemExit("Untracked implementation files must be committed before freezing evidence")
checks = read("target/evidence/m9-checks.json")
if not checks["completed"] or any(c["exit_code"] != 0 or sha(c["log"]) != c["log_sha256"] for c in checks["checks"]):
    raise SystemExit("Incomplete, changed or failing host checks")
latest = read("local-artifacts/m9/latest-host.json")
run = Path(latest["run"])
verification = read(relative(run / "verification.json"))
corpus = read(relative(run / "corpus.json"))
if verification["exit_code"] or verification["corpus_sha256"] != sha(relative(run / "corpus.json")) or corpus["status"] != "passed_required_host_cases" or corpus["cycles"] < 12:
    raise SystemExit("Required repeated host corpus is missing or failed")
rows = []
for case in corpus["cases"]:
    row = {k:v for k,v in case.items() if k != "sessions"}
    sessions = case.get("sessions", [])
    if case.get("expected") == "run" and (len(sessions) != corpus["cycles"] or case["status"] != "passed_host_execution"):
        raise SystemExit("Incomplete required case: " + case["id"])
    row["completed_sessions"] = len(sessions)
    if sessions:
        frames = [value for session in sessions for value in session["frame_us"]]
        row["measurements"] = {
            "load_apk_ms_median": statistics.median(s["load_apk_ms"] for s in sessions),
            "vm_startup_ms_median": statistics.median(s["vm_startup_ms"] for s in sessions),
            "vm_startup_ms_p95": percentile([s["vm_startup_ms"] for s in sessions], .95),
            "software_frame_us_median": statistics.median(frames),
            "software_frame_us_p95": percentile(frames, .95),
            "managed_heap_bytes_after_gc_min": min(s["managed_heap_bytes_after_gc"] for s in sessions),
            "managed_heap_bytes_after_gc_max": max(s["managed_heap_bytes_after_gc"] for s in sessions),
        }
        row["last_labels"] = sessions[-1]["labels"]
        row["last_frame_hash"] = sessions[-1]["frame_hash"]
    rows.append(row)
sources = command("ls-files", "--", *source_roots).splitlines()
fixtures = []
for path in ["local-artifacts/m8/fixture/build-receipt.json", "local-artifacts/m9/aosp-hello/build-receipt.json", "local-artifacts/m7-improvements/fixture/build-receipt.json", "local-artifacts/dependencies/sqlite/source-receipt.json"]:
    fixtures.append({"path": path, "sha256": sha(path), "receipt": read(path)})
for fixture in read("fixtures/m9/host-corpus.json"):
    fixtures.append({"id": fixture["id"], "apk": fixture["apk"], "sha256": sha(fixture["apk"]), "bytes": (root / fixture["apk"]).stat().st_size})
test_count = 0
for check in checks["checks"]:
    text = (root / check["log"]).read_text(encoding="utf-8-sig", errors="replace")
    counts = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
    check["passed_tests"] = sum(int(c[0]) for c in counts)
    check["ignored_tests"] = sum(int(c[2]) for c in counts)
    test_count += check["passed_tests"]
build = root / "target/evidence/m9-device-build.log"
build_text = build.read_text(encoding="utf-8-sig", errors="replace")
nro_hash = sha("local-artifacts/product/androswitch.nro")
build_receipt = read("target/evidence/product-build.json")
if build_receipt["status"] != "built" or build_receipt["nro_sha256"] != nro_hash or "Finished `release`" not in build_text or nro_hash.upper() not in build_text.upper():
    raise SystemExit("Final successful NRO build receipt is missing/mismatched")
report = {
    "schema_version": 1,
    "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "stage": "M9 validation candidate; release gates incomplete",
    "candidate_only": True,
    "implementation_commit": command("rev-parse", "HEAD"),
    "upstream": {"project":"RustyDroid", "authors":"nadabgfccvd and contributors", "revision":"6388281f66d254c8e01a0fdc7a5a9a30239bc762"},
    "product_nro_sha256": nro_hash,
    "verification": {"host":"passed focused shared contracts and required repeated corpus; see commands/counts", "ryujinx":"not_run_for_this_build; desktop opt-in required after window-disappearance incident", "physical_device":"not_run_for_this_build; M1-M7 user-confirmed baseline is separate", "android_reference_device":"not_run; JVM differential and AOSP contract references only"},
    "host_checks": checks,
    "passed_test_executions": test_count,
    "host_corpus": {"status":corpus["status"], "cycles_per_required_case":corpus["cycles"], "completed_sessions":sum(row["completed_sessions"] for row in rows), "cases":rows, "measurement_scope":corpus["measurement_scope"], "receipt":verification, "artifact_root":relative(run), "raw_corpus_sha256":sha(relative(run / "corpus.json"))},
    "device_build": {"command":"./scripts/build-androswitch.ps1", "log":"target/evidence/m9-device-build.log", "log_sha256":sha("target/evidence/m9-device-build.log"), "receipt":build_receipt, "executed":False},
    "fixtures": fixtures,
    "source_inputs": [{"path":path,"sha256":sha(path)} for path in sources],
    "open_gates": ["Execute new NRO Horizon regression: workflow, startup, quiescence and m9 24-session corpus", "Record Horizon startup/frame/total-memory measurements", "Test new preferences/database/large-container/return behavior on physical Switch", "Independent NDK sample integration and wider Android-reference semantics", "Resolve Vector Pinball's observed API blocker and any subsequent failures before claiming gameplay"],
}
report["external_first_screen_gate"] = {"state":"not_verified", "fixture_lock":"fixtures/m9/startup-apks.lock.json", "required":["Mekorama recognizable first screen and safe Plus return", "Candy Crush recognizable first screen and safe Plus return"], "emulator":"not_run_by_this_host_evidence_collector", "requested_next_backend":"physical_switch" if args.physical_test_first else None}
output.write_text(json.dumps(report, indent=2, ensure_ascii=False)+"\n", encoding="utf-8")
print(output.relative_to(root).as_posix())
