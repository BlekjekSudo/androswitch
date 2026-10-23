"""Package an original UI fixture with a streamed STORED payload, without SDK edits."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import zipfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--size-mib", type=int, default=128)
args = parser.parse_args()
if not 65 <= args.size_mib <= 400:
    raise SystemExit("Payload fixture size must be 65..400 MiB")
source = root / "local-artifacts/m5/fixture/java-ui.apk"
output = root / "local-artifacts/m7-improvements/fixture"
output.mkdir(parents=True, exist_ok=True)
target = output / "large-ui.apk"
shutil.copyfile(source, target)
block = b"AndroSwitch archive streaming fixture\n".ljust(65536, b"\xa5")
with zipfile.ZipFile(target, "a", compression=zipfile.ZIP_STORED) as archive:
    with archive.open("assets/big-payload.bin", "w", force_zip64=True) as payload:
        for _ in range(args.size_mib * 16):
            payload.write(block)


def sha(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


receipt = {"schema_version": 1, "source": str(source.relative_to(root)),
           "source_sha256": sha(source), "apk": str(target.relative_to(root)),
           "apk_bytes": target.stat().st_size, "apk_sha256": sha(target),
           "payload_bytes": args.size_mib << 20, "method": "STORED", "zip64_local_header": True,
           "generator_sha256": sha(Path(__file__)),
           "claim": "Original UI corpus fixture with large data asset; not external APK compatibility"}
(output / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
print(output / "build-receipt.json")
