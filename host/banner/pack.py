#!/usr/bin/env python3
"""Pack banner.pgm (from banner.swift) into nawa/kernel/src/banner.rs.

Host-only tooling (purity charter standing rule: third-party code — PIL —
may exist here, never in the image). Threshold 128, MSB-first bit packing.
"""
from PIL import Image

img = Image.open("banner.pgm").point(lambda p: 255 if p >= 128 else 0).convert("1")
w, h = img.size
pixels = img.load()
bytes_per_row = (w + 7) // 8
data = bytearray()
for y in range(h):
    row = bytearray(bytes_per_row)
    for x in range(w):
        if pixels[x, y]:
            row[x // 8] |= 0x80 >> (x % 8)
    data += row

out = [
    "//! Boot banner: the word **i3mel** — ‎«إعمل» — pre-shaped and rasterized at",
    "//! build time from the host's Geeza Pro Bold via CoreText (font output is",
    "//! Tier-3 *content* under the purity charter, like model weights; the live",
    "//! TTF -> bidi -> shaper pipeline that replaces this bake is the Phase 2/3",
    "//! flagship track). Regenerate with host/banner/{banner.swift,pack.py}.",
    "",
    "use nawa_core::fb::Bitmap1Bpp;",
    "",
    f"const WIDTH: usize = {w};",
    f"const HEIGHT: usize = {h};",
    "",
    f"static DATA: [u8; {len(data)}] = [",
]
for i in range(0, len(data), 16):
    out.append("    " + ", ".join(f"0x{b:02x}" for b in data[i:i+16]) + ",")
out += [
    "];",
    "",
    "pub fn banner() -> Bitmap1Bpp {",
    "    Bitmap1Bpp { width: WIDTH, height: HEIGHT, data: &DATA }",
    "}",
]
with open("../../nawa/kernel/src/banner.rs", "w") as f:
    f.write("\n".join(out) + "\n")
print(f"emitted banner.rs: {w}x{h}, {len(data)} bytes")
