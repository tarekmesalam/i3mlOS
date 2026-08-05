#!/usr/bin/env python3
"""Pack font.pgm into nawa/kernel/src/font.rs — one byte per glyph row.

Eight pixels wide is one byte per row, which is why a terminal font is the
one kind of font a kernel can carry without a rasterizer.
"""
CELL_W, CELL_H, FIRST, LAST = 8, 16, 32, 126

with open("font.pgm", "rb") as handle:
    data = handle.read()
parts = data.split(b"\n", 3)
width, height = map(int, parts[1].split())
pixels = parts[3]

count = LAST - FIRST + 1
rows = []
for index in range(count):
    for y in range(CELL_H):
        byte = 0
        for x in range(CELL_W):
            if pixels[y * width + index * CELL_W + x] >= 128:
                byte |= 0x80 >> x
        rows.append(byte)

out = [
    "// An 8x16 bitmap font for ASCII 32..126, rendered at build time from",
    "// Menlo and packed one byte per row. A kernel that wants to say something",
    "// on screen needs a rasterizer or a grid; until the TTF pipeline exists,",
    "// this is the grid. Regenerate with host/font/{fontgen.swift,pack.py}.",
    "",
    "pub const CELL_WIDTH: usize = 8;",
    "pub const CELL_HEIGHT: usize = 16;",
    "pub const FIRST: u8 = 32;",
    "pub const LAST: u8 = 126;",
    "",
    f"pub static GLYPHS: [u8; {len(rows)}] = [",
]
for i in range(0, len(rows), 16):
    out.append("    " + ", ".join(f"0x{b:02x}" for b in rows[i:i+16]) + ",")
out += [
    "];",
    "",
    "/// The sixteen rows of one character, or spaces for anything outside the",
    "/// range — a kernel printing an unexpected byte should leave a gap, not a",
    "/// surprise.",
    "pub fn glyph(byte: u8) -> &'static [u8] {",
    "    if byte < FIRST || byte > LAST {",
    "        return &GLYPHS[0..CELL_HEIGHT];",
    "    }",
    "    let index = (byte - FIRST) as usize * CELL_HEIGHT;",
    "    &GLYPHS[index..index + CELL_HEIGHT]",
    "}",
]
with open("../../nawa/kernel/src/font.rs", "w") as handle:
    handle.write("\n".join(out) + "\n")
print(f"emitted font.rs: {count} glyphs, {len(rows)} bytes")
