"""Freeze matching-NRO startup, console and fail-closed Ryujinx receipts."""
import argparse
import datetime
import hashlib
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--implementation-commit", required=True)
parser.add_argument("--unrun-normal-boundary-reason", default="")
options = parser.parse_args()
if not re.fullmatch(r"[0-9a-f]{40}", options.implementation_commit):
    raise SystemExit("Supply the full reviewed implementation commit")


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


nro_hash = sha(root / "local-artifacts/product/androswitch.nro")
runs = {}
for path in sorted((root / "local-artifacts/m7/runs").glob("*/verification.json"),
                   key=lambda p: p.stat().st_mtime):
    receipt = read(path)
    if receipt.get("status") == "passed" and receipt.get("nro_sha256") == nro_hash:
        runs[receipt["scenario"]] = (path.parent, receipt)
scenarios = ("startup-failure", "startup-quiescence", "workflow", "quiescence")
if any(name not in runs for name in scenarios[:3]):
    raise SystemExit("Startup recovery, failed-onload worker and console workflow must pass against the current NRO")
if "quiescence" not in runs and not options.unrun_normal_boundary_reason:
    raise SystemExit("Normal worker boundary must pass, or carry an explicit not-run reason")
startup_dir, startup = runs["startup-failure"]
if startup.get("external_apk_sha256") != "9b8a5ef80de69095769c679bf65d5b339564478736f78b24700a4f0826bfec01":
    raise SystemExit("The external Mekorama reproduction is required")
sessions = read(startup_dir / "emulator.json")["sessions"]
failed = [s for s in sessions if s["status"] == "failed"]
completed = [s for s in sessions if s["status"] == "passed_execution"]
if len(failed) != 4 or len(completed) != 3:
    raise SystemExit("Expected four recovered startup failures and three completed sessions")
for session in failed:
    phases = {p["phase"] for p in session["teardown"]}
    if not {"java_startup_abort", "runtime_closed", "native_resources_released"} <= phases:
        raise SystemExit("A failed startup did not complete owned cleanup")
worker = read(runs["startup-quiescence"][0] / "expected-stop-failure.json")
if not all(text in worker["error"] for text in ("JNI_OnLoad returned 0x0", "NATIVE_NOT_QUIESCENT")):
    raise SystemExit("Failed JNI_OnLoad worker must preserve both causes and fail closed")

host = {}
for name, expected in (("framework", 4), ("core", 6), ("launcher", 1)):
    path = root / f"target/evidence/startup-recovery-{name}.log"
    content = path.read_text(encoding="utf-8-sig")
    results = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", content)
    passed = sum(int(row[0]) for row in results)
    if passed != expected or any(int(row[1]) for row in results):
        raise SystemExit(f"Unexpected host test result in {path}")
    host[name] = {"passed": passed, "ignored": sum(int(row[2]) for row in results),
                  "log_sha256": sha(path)}
clippy = root / "target/evidence/startup-recovery-clippy.log"
if clippy.read_text(encoding="utf-8-sig").count("Finished `dev`") != 3:
    raise SystemExit("Three host clippy commands must pass")

sources = [
    "crates/as-framework/src/android.rs", "crates/as-framework/tests/lifecycle.rs",
    "crates/as-switch-smoke/src/application.rs", "platform/androswitch/main.c",
    "scripts/test-ryujinx-launcher.ps1", "scripts/build-startup-failure-fixtures.ps1",
    "scripts/record-startup-recovery-evidence.py",
    "scripts/validate-launcher-report.ps1", "scripts/verify-saved-launcher-run.ps1",
    "scripts/package-androswitch.ps1",
]
sources += [p.relative_to(root).as_posix() for p in sorted((root / "fixtures/startup-failure").glob("*"))]
report = {
    "schema_version": 1, "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "stage": "M7 alpha owned startup-failure recovery; Mekorama remains unsupported",
    "implementation_commit": options.implementation_commit, "product_nro_sha256": nro_hash,
    "verification": {
        "host": "11 focused tests passed; two unchanged oracle tests ignored; three clippy commands passed",
        "ryujinx": "passed four startup errors including unmodified Mekorama, Java/native/Java recovery, full console workflow validated from saved SD, and failed-onload live-worker exit/reboot boundary; see normal_boundary for separate status",
        "physical_device": "startup recovery pending retest; prior display fix confirmed by user for bundled native sample",
        "android_reference_device": "not_run",
    },
    "runs": {name: receipt for name, (_, receipt) in runs.items() if name in scenarios},
    "normal_boundary": runs["quiescence"][1] if "quiescence" in runs else {
        "status": "not_run_on_current_nro", "reason": options.unrun_normal_boundary_reason,
        "prior_evidence": "reports/m7-display-handoff.json; different NRO hash, not a current-build pass",
    },
    "desktop_incident": {
        "evidence_type": "user_report", "affected_apps": "browser and Codex",
        "symptom": "windows disappeared while emulator testing; processes initially remained",
        "user_action": "user later closed the affected processes; reopening declined",
        "cause": "undetermined; no claim that Hidden, fullscreen, GPU or memory is the established cause",
        "response": "emulator stopped, original SD folder restored, saved workflow validated without app launch; desktop E2E now requires explicit opt-in",
    },
    "startup_failures": [{"session_id": s["session_id"], "package": s["package"],
        "error": s["error"], "teardown": s["teardown"]} for s in failed],
    "following_sessions": [{"session_id": s["session_id"], "package": s["package"],
        "status": s["status"], "native_frames": s["snapshots"][-1]["native_frames"],
        "audio_frames": s["snapshots"][-1]["audio_frames"]} for s in completed],
    "failed_onload_live_worker": worker,
    "host": host, "clippy_log_sha256": sha(clippy),
    "device_build_log_sha256": sha(root / "target/evidence/startup-recovery-build.log"),
    "device_clippy": "not_run: nightly cargo-clippy component unavailable; device release build passed",
    "fixture_build": read(root / "local-artifacts/startup-failure/fixture/build-receipt.json"),
    "samples": [{"path": f"local-artifacts/{p}", "sha256": sha(root / "local-artifacts" / p)}
        for p in ("m5/fixture/java-ui.apk", "m5/fixture/kotlin-ui.apk",
                  "m6/fixture/mixed-media.apk", "m6/fixture/native-media.apk")],
    "sources": [{"path": p, "sha256": sha(root / p)} for p in sources],
    "supplied_hardware_and_static_evidence": {"path": "reports/mekorama-startup-20261024.json",
        "sha256": sha(root / "reports/mekorama-startup-20261024.json")},
    "reproduction_commands": [
        "scripts/build-startup-failure-fixtures.ps1", "scripts/build-androswitch.ps1",
        "scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario startup-failure -ExternalApk D:/Download/mekorama-1-7-2.apk",
        "scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario startup-quiescence",
        "scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator", "scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario quiescence",
        "scripts/verify-saved-launcher-run.ps1 -RunId 9f6be525dd8f41d69bd640cf574dd7c0",
        "cargo test -p as-framework --features host --test lifecycle --locked -- --include-ignored",
        "cargo test -p as-package -p as-native --locked",
        "cargo test -p as-launcher --features host --test workflow --locked -- --include-ignored",
        "cargo clippy -p as-framework --features host --all-targets --locked -- -D warnings",
        "cargo clippy -p as-launcher -p as-package --features as-launcher/host --all-targets --locked -- -D warnings",
        "cargo clippy -p as-switch-smoke --features host-cli --all-targets --locked -- -D warnings",
        "cargo fmt --all -- --check", "python scripts/check_md_links.py", "git diff --check",
    ],
    "limitations": [
        "Mekorama stops before graphics; no gameplay compatibility is claimed",
        "The 64 MiB APK budget is unchanged; no large-APK memory validation is claimed",
        "Host lifecycle tests cover cancellation; native ownership/thread boundaries require the NRO scenarios",
        "No proprietary APK or extracted libraries are distributed",
        "Synthetic Plus dispatch and PCM counters are emulator evidence, not physical controls/acoustic measurement",
        "Full upstream VM/JVM regression corpus was not rerun; prior milestone reports retain that evidence",
        "Supplied teardown/display traces lack session identity and are treated as earlier sample traces",
        "Desktop window loss was reported during emulator testing; cause remains undetermined; no further emulator run was started",
    ],
}
(root / "reports/m7-startup-recovery.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print("reports/m7-startup-recovery.json")
