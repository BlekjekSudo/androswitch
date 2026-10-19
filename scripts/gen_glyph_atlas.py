#!/usr/bin/env python3
"""Generate crates/rd-vm/assets/glyphs.bin — the software-text atlas.

DejaVu Sans (Bitstream Vera licence, permissive) rendered at 32 px:
ASCII, Latin-1 Supplement and common typographic symbols.

Format (little endian):
  b"RDGL" u16 px_size, i16 ascent, i16 descent, u16 count
  per glyph: u32 codepoint, u16 advance*64, i16 left, i16 top(from baseline, up+),
             u16 w, u16 h, w*h bytes alpha
"""
import struct, sys
from PIL import Image, ImageDraw, ImageFont
SIZE = 32
font = ImageFont.truetype(sys.argv[1] if len(sys.argv) > 1 else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", SIZE)
asc, desc = font.getmetrics()
cps = list(range(0x20, 0x7F)) + list(range(0xA0, 0x180)) + [0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026, 0x20AC, 0x2190, 0x2192, 0x2191, 0x2193, 0x2713, 0x2715, 0x2212, 0x00D7, 0x25CF, 0x25CB, 0x2605, 0x2606, 0xFFFD]
out = bytearray()
n = 0
for cp in cps:
    ch = chr(cp)
    adv = font.getlength(ch)
    l, t, r, b = font.getbbox(ch, anchor="ls")
    w, h = max(0, r - l), max(0, b - t)
    img = Image.new("L", (max(w, 1), max(h, 1)), 0)
    if w and h:
        ImageDraw.Draw(img).text((-l, -t), ch, font=font, fill=255, anchor="ls")
    out += struct.pack("<IHhhHH", cp, int(round(adv * 64)), l, -t, w, h)
    if w and h:
        out += img.tobytes()
    n += 1
sys.stdout.buffer.write(b"RDGL" + struct.pack("<HhhH", SIZE, asc, desc, n) + out)
