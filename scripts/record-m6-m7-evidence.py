"""Freeze observed runs and source/fixture hashes; never manufacture test passes."""
import argparse
import hashlib
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[1]
args = argparse.ArgumentParser()
args.add_argument("--implementation-commit", default=None)
options = args.parse_args()

def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def relative(path):
    return path.relative_to(root).as_posix()

nro = root / "local-artifacts/product/androswitch.nro"
nro_hash = sha(nro)
runs = []
for path in (root / "local-artifacts/m7/runs").glob("*/verification.json"):
    value = read(path)
    if value.get("status") == "passed":
        runs.append((path, value))

def latest(scenario):
    candidates = [(p,v) for p,v in runs if v.get("scenario") == scenario and v.get("nro_sha256") == nro_hash]
    if not candidates:
        raise SystemExit(f"No passing {scenario} evidence for product NRO {nro_hash}")
    return max(candidates, key=lambda item: item[0].stat().st_mtime)

def compact(path, value):
    value = {k:v for k,v in value.items() if k not in ("sd_result", "pid", "inputs")}
    value.update(artifact_root=relative(path.parent), receipt_sha256=sha(path))
    return value

workflow_path, workflow_value = latest("workflow")
boundary_path, boundary_value = latest("quiescence")
workflow = read(workflow_path.parent / "emulator.json")
media = read(workflow_path.parent / "guest-media.json")
if workflow_value["completed_sessions"] != 10 or media.get("readback_ok") != 1:
    raise SystemExit("Incomplete console/media assertions")
native_inputs = []
for name in ("java-jni.apk", "java-jni-emutls.apk", "kotlin-jni.apk"):
    candidates = [(p,v) for p,v in runs if any(pathlib.PureWindowsPath(i["file"]).name == name for i in v.get("inputs", []))]
    if not candidates:
        raise SystemExit(f"No passing JNI regression: {name}")
    path, value = max(candidates, key=lambda item: item[0].stat().st_mtime)
    frozen = compact(path, value)
    frozen["inputs"] = [{"file":pathlib.PureWindowsPath(i["file"]).name,"sha256":i["sha256"]} for i in value["inputs"]]
    native_inputs.append(frozen)

logs = []
passed = 0
for name in ("m7-apk-tests.log", "m7-shell-tests.log", "m7-framework-tests.log", "m7-vm-tests.log", "m7-core-tests.log", "m7-loader-tests.log"):
    path = root / "target/evidence" / name
    content = path.read_text(encoding="utf-8-sig")
    results = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed", content)
    if not results or any(int(f) for _,f in results):
        raise SystemExit(f"Missing/nonpassing host summary: {name}")
    count = sum(int(n) for n,_ in results)
    passed += count
    logs.append({"file":relative(path),"sha256":sha(path),"passed_test_executions":count})

sources = sorted((root / "fixtures/m6").rglob("*"))
sources += [root / "scripts/build-m6-fixture.ps1",root / "scripts/make-fixture-icon.py"]
apks = sorted((root / "local-artifacts/m6/fixture").glob("*.apk"))
report = {
    "schema_version":1,"implementation_commit":options.implementation_commit,
    "stage":"M7 development alpha; supported proof corpus, not universal APK compatibility",
    "product_nro_sha256":nro_hash,
    "verification":{"host":"passed stated contracts", "ryujinx":"passed supported corpus and negative lifecycle boundary", "physical_device":"not_run", "android_reference_device":"not_run"},
    "console":compact(workflow_path,workflow_value),
    "boundary":compact(boundary_path,boundary_value),
    "boundary_error":read(boundary_path.parent / "expected-stop-failure.json"),
    "native_media":media,
    "captures":{"console_framebuffer_sha256":sha(workflow_path.parent/"launcher.ppm"),"documentation_png_sha256":sha(root/"docs/images/androswitch-m7.png")},
    "sessions":[{"session_id":s.get("session_id"),"package":s.get("package"),"status":s["status"],"error":s.get("error"),"snapshots":len(s.get("snapshots",[])),"last_native_frames":s.get("snapshots",[{}])[-1].get("native_frames"),"last_audio_frames":s.get("snapshots",[{}])[-1].get("audio_frames")} for s in workflow["sessions"]],
    "jni_regressions":{"runs":native_inputs,"total_cases":sum(v["dex_calls"] for v in native_inputs),"positive_reference":"real JVM/native fixture build", "expected_api_errors":2,"stack_callback_expected":83.25,"synchronization_expected":619,"old_nro_regression_failure":"local-artifacts/m6/runs/2c2e0a841c2246e7b867847e3b81962f"},
    "host":{"passed_test_executions":passed,"note":"Executions across commands; the loader scalar case is intentionally rerun with its fixture-dependent suite.","logs":logs},
    "commands":["scripts/build-m5-fixture.ps1", "scripts/build-m6-fixture.ps1", "scripts/build-m4-fixture.ps1", "scripts/build-androswitch.ps1", "scripts/test-ryujinx-launcher.ps1", "scripts/test-ryujinx-launcher.ps1 -Scenario quiescence", "scripts/build-switch-smoke.ps1", "scripts/test-ryujinx-probe.ps1 (Java/classic TLS, Java/emutls, Kotlin; -SkipHost, JVM expected values)", "cargo clippy -p as-launcher --features host --all-targets -- -D warnings", "cargo clippy -p as-switch-smoke --features host-cli --all-targets -- -D warnings", "cargo fmt --all -- --check", "python scripts/check_md_links.py"],
    "fixtures":{"sources":[{"path":relative(p),"sha256":sha(p)} for p in sources if p.is_file()],"apks":[{"path":relative(p),"sha256":sha(p),"bytes":p.stat().st_size} for p in apks],"ndk":"28.2.13676358","build_tools":"35.0.0","compile_api":26},
    "limitations":["Physical Switch and Android reference-device media/framework runs not performed", "Synthetic inputs use real dispatchers; physical Joy-Con/touchscreen unverified", "Audio proof checks released PCM buffers, not acoustic quality", "Console captures are CPU submissions; GLES captures are GPU readback", "External NDK samples and Vector Pinball remain independent pending targets", "Whole device APK container limited to 64 MiB; OBB/assets use bounded reads"]
}
path = root / "reports/m6-m7.json"
path.write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
print(path)
