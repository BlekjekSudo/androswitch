"""Deterministic original RGBA PNG fixture, using only the Python stdlib."""
import pathlib
import struct
import sys
import zlib

def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

rows = bytearray()
for y in range(48):
    rows.append(0)
    for x in range(48):
        letter = 10 <= y < 39 and (abs(x - (24 - (y-10)//3)) <= 2 or abs(x - (24+(y-10)//3)) <= 2 or 27 <= y < 31 and 17 <= x <= 31)
        rows.extend((232, 255, 250, 255) if letter else (32, 141, 125, 255))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB",48,48,8,6,0,0,0)) + chunk(b"IDAT",zlib.compress(rows,9)) + chunk(b"IEND",b"")
path = pathlib.Path(sys.argv[1])
path.parent.mkdir(parents=True, exist_ok=True)
path.write_bytes(png)
