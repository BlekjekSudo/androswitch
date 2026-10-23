"""Record observed M7 hardware-feedback regressions without replacing the baseline."""
import argparse
import hashlib
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--implementation-commit", required=True)
parser.add_argument("--display-handoff", action="store_true")
options = parser.parse_args()


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def relative(path):
    return path.relative_to(root).as_posix()


nro_hash = sha(root / "local-artifacts/product/androswitch.nro")
runs = []
for path in (root / "local-artifacts/m7/runs").glob("*/verification.json"):
    value = read(path)
    if value.get("status") == "passed" and value.get("nro_sha256") == nro_hash:
        runs.append((path, value))


def latest(scenario):
    candidates = [(p, v) for p, v in runs if v.get("scenario") == scenario]
    if not candidates:
        raise SystemExit(f"No passing {scenario} evidence for {nro_hash}")
    path, value = max(candidates, key=lambda item: item[0].stat().st_mtime)
    compact = {k: v for k, v in value.items() if k not in ("sd_result", "pid")}
    compact.update(artifact_root=relative(path.parent), receipt_sha256=sha(path))
    return path.parent, compact


workflow_dir, workflow = latest("workflow")
boundary_dir, boundary = latest("quiescence")
execution = read(workflow_dir / "emulator.json")
native = [s for s in execution["sessions"] if s.get("package") == "org.fixture.nativemedia"]
required = {"egl_wait_gpu", "egl_terminated", "guest_thread_joined",
            "native_resources_released", "runtime_closed"}
if len(native) != 2 or any(not required.issubset({p["phase"] for p in s["teardown"]}) for s in native):
    raise SystemExit("Missing repeated native Plus/GPU retirement evidence")
traces = [json.loads(line) for line in (workflow_dir / "teardown.log").read_text().splitlines()]
if traces[-1]["phase"] != "launcher_ready":
    raise SystemExit("Launcher frame was not presented after cleanup")

logs = []
for name in ("package", "shell"):
    path = root / f"target/evidence/hardware-fix-{name}.log"
    results = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed", path.read_text(encoding="utf-8-sig"))
    if not results or any(int(f) for _, f in results):
        raise SystemExit(f"Nonpassing host log: {path}")
    logs.append({"file": relative(path), "sha256": sha(path),
                 "passed_tests": sum(int(p) for p, _ in results)})
clippy = root / ("target/evidence/display-handoff-clippy.log" if options.display_handoff
                 else "target/evidence/hardware-fix-clippy.log")
if clippy.read_text(encoding="utf-8-sig").count("Finished `dev`") != 2:
    raise SystemExit("Both clippy invocations must finish successfully")

sources = [
    "crates/as-package/src/lib.rs", "crates/as-package/tests/persistence.rs",
    "crates/as-launcher/src/lib.rs", "crates/as-launcher/tests/workflow.rs",
    "crates/as-switch-smoke/src/application.rs", "crates/as-switch-smoke/src/launcher.rs",
    "platform/androswitch/main.c", "platform/androswitch/media.c",
    "platform/switch-smoke/native.c", "platform/switch-smoke/native.h",
    "scripts/test-ryujinx-launcher.ps1", "scripts/package-androswitch.ps1",
    "scripts/record-hardware-feedback-evidence.py",
]
baseline = read(root / "reports/m6-m7.json")
report = {
    "schema_version": 1,
    "implementation_commit": options.implementation_commit,
    "stage": "M7 alpha hardware-feedback candidate; physical Plus fix unconfirmed",
    "product_nro_sha256": nro_hash,
    "verification": {
        "host": "passed package and launcher regressions",
        "ryujinx": "passed console workflow, repeated synthetic Plus dispatch and negative teardown/recovery",
        "physical_device": "pending_retest",
        "android_reference_device": "not_run",
    },
    "original_hardware_feedback": {
        "date": "2026-10-24", "evidence_type": "user_report",
        "original_nro_sha256": baseline["product_nro_sha256"],
        "sample_install_open": "reported working for four bundled APKs",
        "native_animation_beeps": "reported working",
        "native_plus_return": "reported Switch crash; session-report.json absent",
        "crash_dump": "not_available", "new_build_hardware": "pending_retest",
    },
    "console": workflow, "boundary": boundary,
    "native_plus_sessions": [{"session_id": s["session_id"], "revision": s["revision"],
        "native_frames": s["snapshots"][-1]["native_frames"],
        "audio_frames": s["snapshots"][-1]["audio_frames"],
        "teardown": s["teardown"]} for s in native],
    "native_media": read(workflow_dir / "guest-media.json"),
    "host": logs, "clippy_log_sha256": sha(clippy),
    "sources": [{"path": p, "sha256": sha(root / p)} for p in sources],
    "samples": [{"path": f"local-artifacts/{p}", "sha256": sha(root / "local-artifacts" / p)}
        for p in ("m5/fixture/java-ui.apk", "m5/fixture/kotlin-ui.apk",
                  "m6/fixture/mixed-media.apk", "m6/fixture/native-media.apk")],
    "commands": [
        "cargo test -p as-package --locked",
        "cargo test -p as-launcher --features host --test workflow --locked -- --include-ignored",
        "cargo clippy -p as-launcher -p as-package --features as-launcher/host --all-targets --locked -- -D warnings",
        "cargo clippy -p as-switch-smoke --features host-cli --all-targets --locked -- -D warnings",
        "scripts/build-androswitch.ps1", "scripts/test-ryujinx-launcher.ps1",
        "scripts/test-ryujinx-launcher.ps1 -Scenario quiescence",
        "cargo fmt --all -- --check", "python scripts/check_md_links.py", "git diff --check",
    ],
    "limitations": [
        "GPU retirement is a candidate fix; the hardware crash cause is not confirmed",
        "Synthetic return_to_launcher dispatches Plus in the C main loop; no physical HID event is claimed",
        "Original JVM/VM regression evidence remains in reports/m6-m7.json; those suites were not rerun for this fix",
        "Firmware, hardware model, launch mode and Atmosphere result code were not supplied",
    ],
}
if options.display_handoff:
    handoff_path = workflow_dir / "display-handoffs.log"
    handoffs = [json.loads(line) for line in handoff_path.read_text().splitlines()]
    generations = {h["generation"] for h in handoffs}
    if len(handoffs) != 16 or len(generations) != 2:
        raise SystemExit("Missing eight-frame monitoring after both native returns")
    for generation in generations:
        if [h["frame"] for h in handoffs if h["generation"] == generation] != list(range(1, 9)):
            raise SystemExit("Incomplete monitored frame sequence")
    if any(h["result"] or h["buffers"] != 3 or not 0 <= h["slot"] < 3 for h in handoffs):
        raise SystemExit("Invalid monitored queue result/slot")
    if len({h["map_fd"] for h in handoffs}) != 1 or len({h["fence_fd"] for h in handoffs}) != 1:
        raise SystemExit("Display services did not survive renderer handoffs")
    copy_log = root / "target/evidence/display-handoff-copy.log"
    if "4 address/guard cases passed" not in copy_log.read_text(encoding="utf-8-sig"):
        raise SystemExit("Missing hardware pixel-layout host test")
    report["prior_package_launcher_host_tests"] = report.pop("host")
    report["host"] = {"passed_layout_cases": 4, "log_sha256": sha(copy_log),
                      "note": "Package/launcher tests above are retained prior evidence; their code is unchanged"}
    report["stage"] = "M7 display-handoff candidate after supplied hardware crash logs"
    report["verification"]["host"] = "passed four independent framebuffer address/guard cases and clippy"
    report["verification"]["ryujinx"] = "passed console workflow, two 60-frame launcher waits after native Plus, eight-frame queue monitoring per return, negative teardown/recovery"
    report["hardware_feedback"] = {"path": "reports/hardware-native-plus-20261024.json",
                                  "sha256": sha(root / "reports/hardware-native-plus-20261024.json")}
    report["display_handoffs"] = handoffs
    report["display_handoff_receipt_sha256"] = sha(handoff_path)
    report["sources"] += [{"path": p, "sha256": sha(root / p)} for p in (
        "platform/androswitch/framebuffer_copy.h", "platform/androswitch/tests/framebuffer_copy.c",
        "platform/androswitch/Makefile", "platform/androswitch/media.h")]
    report["commands"] = report["commands"][2:]
    report["commands"].append("gcc -std=c11 -O2 -Wall -Wextra -Werror platform/androswitch/tests/framebuffer_copy.c -o <local output>; run output")
    report["limitations"][0] = "Physical crash occurred after launcher_ready on the preceding candidate; display-service/queue changes await hardware retest"
path = root / ("reports/m7-display-handoff.json" if options.display_handoff
               else "reports/m7-hardware-feedback.json")
path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(path)
