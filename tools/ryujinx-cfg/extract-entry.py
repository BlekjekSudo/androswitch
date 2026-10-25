"""Extract one exact diagnostic bundle assembly without running the emulator."""
import argparse
import hashlib
import io
from pathlib import Path
import struct
import zlib
from rebundle import SIGNATURE, readstr

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("bundle", type=Path)
p.add_argument("output", type=Path)
p.add_argument("--expected-sha", required=True)
p.add_argument("--entry", choices=["LibHac.dll", "Ryujinx.HLE.dll"], required=True)
a = p.parse_args()
b = a.bundle.read_bytes()
if hashlib.sha256(b).hexdigest() != a.expected_sha or b.count(SIGNATURE) != 1:
    raise SystemExit("Input bundle identity/signature differs")
f = io.BytesIO(b)
f.seek(struct.unpack_from("<Q", b, b.index(SIGNATURE) - 8)[0])
major, minor, count = struct.unpack("<III", f.read(12))
if (major, minor) != (6, 0) or count > 10000:
    raise SystemExit("Unsupported bundle version/count")
readstr(f)
f.read(40)
entries = []
for _ in range(count):
    offset, size, compressed, kind = struct.unpack("<QQQB", f.read(25))
    name = readstr(f)
    if name == a.entry:
        if size > 128 * 1024 * 1024 or offset + (compressed or size) > len(b):
            raise SystemExit("Invalid assembly range")
        data = b[offset:offset + (compressed or size)]
        data = zlib.decompress(data, -15) if compressed else data
        if len(data) != size or not data.startswith(b"MZ"):
            raise SystemExit("Invalid assembly payload")
        entries.append(data)
if len(entries) != 1 or a.output.exists():
    raise SystemExit("Exactly one assembly and a new output path required")
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_bytes(entries[0])
print(a.entry, hashlib.sha256(entries[0]).hexdigest())
