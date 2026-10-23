"""Fetch the unmodified, pinned independent APK. Does not launch an emulator."""
import hashlib
import json
from pathlib import Path
import urllib.request
root = Path(__file__).resolve().parent.parent
path = root / "local-artifacts/m9/external/vector-pinball-43.apk"
url = "https://f-droid.org/repo/com.dozingcatsoftware.bouncy_43.apk"
expected = "ffda0d9cb0b1b2aa58be9559dda891c4fa24391bc481d297a8e3d96c31f62721"
path.parent.mkdir(parents=True, exist_ok=True)
if not path.exists():
    with urllib.request.urlopen(url, timeout=45) as reply:
        data = reply.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
        raise SystemExit("Independent APK digest/size mismatch")
    path.write_bytes(data)
if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
    raise SystemExit("Cached independent APK digest mismatch")
print(path)
lock = json.loads((root / "fixtures/m9/external.lock.json").read_text())
source = root / "local-artifacts/m9/aosp-source"
for relative, digest in lock["aosp_hello_activity"]["files"].items():
    target = source / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    if not target.exists():
        with urllib.request.urlopen(lock["aosp_hello_activity"]["source_base"] + "/" + relative, timeout=30) as reply:
            data = reply.read(1024 * 1024 + 1)
        if len(data) > 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
            raise SystemExit("AOSP source digest/size mismatch: " + relative)
        target.write_bytes(data)
    if hashlib.sha256(target.read_bytes()).hexdigest() != digest:
        raise SystemExit("Cached AOSP source digest mismatch: " + relative)
print(source)
