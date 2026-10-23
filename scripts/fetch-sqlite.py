"""Fetch pinned public SQLite sources; verify the upstream SHA3 before extraction."""
import hashlib
import json
from pathlib import Path
import urllib.request
import zipfile

root = Path(__file__).resolve().parents[1]
lock = json.loads((root / "third_party/sqlite.lock.json").read_text())
cache = root / lock["cache"]
cache.mkdir(parents=True, exist_ok=True)
archive = cache / "source.zip"
if not archive.exists():
    request = urllib.request.Request(lock["source_url"], headers={"User-Agent": "AndroSwitch-build/0.1"})
    with urllib.request.urlopen(request, timeout=45) as response:
        data = response.read(8 << 20)
    if hashlib.sha3_256(data).hexdigest() != lock["archive_sha3_256"]:
        raise SystemExit("SQLite source checksum does not match the pinned upstream digest")
    archive.write_bytes(data)
if hashlib.sha3_256(archive.read_bytes()).hexdigest() != lock["archive_sha3_256"]:
    raise SystemExit("Cached SQLite archive checksum mismatch")
with zipfile.ZipFile(archive) as source:
    for name in ("sqlite3.c", "sqlite3.h"):
        content = source.read(lock["source_directory"] + "/" + name)
        path = cache / name
        if not path.exists() or path.read_bytes() != content:
            path.write_bytes(content)
receipt = {"version": lock["version"], "archive_sha3_256": lock["archive_sha3_256"],
           "files": {name: hashlib.sha256((cache/name).read_bytes()).hexdigest()
                     for name in ("sqlite3.c", "sqlite3.h")}}
(cache / "source-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(cache / "source-receipt.json")
