#!/usr/bin/env python3
"""Bake the three-petal mark (assets/icon.svg) into nawa/kernel/src/logo.rs.

Uses our own pure-stdlib SVG rasterizer (svgraster.py). items.pkl carries the
flattened source paths; regenerate it by parsing assets/icon.svg with
svgraster.parse_path if the mark ever changes. Host-only tooling — never in
the image.
"""
# The generation logic lives inline in the repo history (M0.5 commit); this
# file is the canonical entry point going forward. Run from this directory:
#   python3 logo_pack.py
import pickle
import svgraster as sr

items = pickle.load(open("items.pkl", "rb"))
MARK = [(0, (0x15, 0x2A, 0xC5)), (1, (0x00, 0xAE, 0xF2)), (2, (0x1A, 0x86, 0xEE))]
x0 = min(items[i]["bbox"][0] for i, _ in MARK)
y0 = min(items[i]["bbox"][1] for i, _ in MARK)
x1 = max(items[i]["bbox"][2] for i, _ in MARK)
y1 = max(items[i]["bbox"][3] for i, _ in MARK)

TARGET_H = 200
scale = TARGET_H / (y1 - y0)
W = int(round((x1 - x0) * scale))
H = TARGET_H
tf = lambda p: ((p[0] - x0) * scale, (p[1] - y0) * scale)

petals = []
for idx, color in MARK:
    cov = sr.rasterize(items[idx]["sub"], W, H, tf, items[idx]["evenodd"])
    mask = [1 if c > 0.5 else 0 for c in cov]
    xs = [x for y in range(H) for x in range(W) if mask[y * W + x]]
    ys = [y for y in range(H) for x in range(W) if mask[y * W + x]]
    bx0, bx1, by0, by1 = min(xs), max(xs) + 1, min(ys), max(ys) + 1
    bw, bh = bx1 - bx0, by1 - by0
    row_bytes = (bw + 7) // 8
    data = bytearray()
    for y in range(by0, by1):
        row = bytearray(row_bytes)
        for x in range(bx0, bx1):
            if mask[y * W + x]:
                row[(x - bx0) // 8] |= 0x80 >> ((x - bx0) % 8)
        data += row
    petals.append((bx0, by0, bw, bh, color, bytes(data)))

out = [
    "//! The i3mlOS mark — three petals orbiting a center, the O in i3mlOS —",
    "//! rasterized at build time from assets/icon.svg by our own pure-Python",
    "//! SVG rasterizer (host/banner/{svgraster.py,logo_pack.py}). Vector",
    "//! rendering in-kernel arrives with the Phase 3 compositor.",
    "",
    "use nawa_core::fb::Bitmap1Bpp;",
    "",
    f"pub const MARK_WIDTH: usize = {W};",
    f"pub const MARK_HEIGHT: usize = {H};",
    "",
    "pub struct Petal {",
    "    pub dx: usize,",
    "    pub dy: usize,",
    "    pub color: (u8, u8, u8),",
    "    pub bitmap: Bitmap1Bpp,",
    "}",
    "",
]
for n, (bx, by, bw, bh, color, data) in enumerate(petals):
    out.append(f"static PETAL_{n}: [u8; {len(data)}] = [")
    for i in range(0, len(data), 16):
        out.append("    " + ", ".join(f"0x{b:02x}" for b in data[i : i + 16]) + ",")
    out.append("];")
    out.append("")
out.append("pub fn mark() -> [Petal; 3] {")
out.append("    [")
for n, (bx, by, bw, bh, color, data) in enumerate(petals):
    r, g, b = color
    out.append(
        f"        Petal {{ dx: {bx}, dy: {by}, color: (0x{r:02x}, 0x{g:02x}, 0x{b:02x}),"
        f" bitmap: Bitmap1Bpp {{ width: {bw}, height: {bh}, data: &PETAL_{n} }} }},"
    )
out.append("    ]")
out.append("}")
with open("../../nawa/kernel/src/logo.rs", "w") as f:
    f.write("\n".join(out) + "\n")
print(f"emitted logo.rs: mark {W}x{H}, {len(petals)} petals")
