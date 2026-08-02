#!/usr/bin/env python3
"""Construct a geometric 'S' matching the i3ml wordmark's DNA.

Measured from the source logo (units = SVG user units):
  stroke width  57.45   (the `l` bar: 1800.84 - 1743.4; the `i` stem agrees)
  baseline      589.8
  round-glyph vertical extent 228.7 .. 595.8  (the `3`, incl. overshoot)

The S is built as two vertically-stacked ellipse arcs that meet with a shared
horizontal tangent — the classic geometric construction — then offset by
±stroke/2 into an outline with perpendicular (butt) terminals.
"""
from math import cos, sin, pi, hypot

STROKE = 57.45
TOP = 228.7
BOTTOM = 595.8
HEIGHT = BOTTOM - TOP            # 367.1
DEFAULT_WIDTH = 262.0            # outer width; the `3` is 255.9 wide

# Terminal angles, degrees, on each ellipse (0 = right, -90 = top, +90 = bottom)
UPPER_START = -42.0
LOWER_END = 138.0


def centerline(width=DEFAULT_WIDTH, samples=180):
    rx = (width - STROKE) / 2
    ry = (HEIGHT - STROKE) / 4
    cx = width / 2
    cy_upper = TOP + STROKE / 2 + ry
    cy_lower = BOTTOM - STROKE / 2 - ry

    points = []
    # Upper bowl: right terminal -> over the top -> left -> junction (bottom).
    start, end = UPPER_START, -270.0
    for i in range(samples + 1):
        theta = (start + (end - start) * i / samples) * pi / 180
        points.append((cx + rx * cos(theta), cy_upper + ry * sin(theta)))
    # Lower bowl: junction (top) -> right -> bottom -> left terminal.
    start, end = -90.0, LOWER_END
    for i in range(1, samples + 1):
        theta = (start + (end - start) * i / samples) * pi / 180
        points.append((cx + rx * cos(theta), cy_lower + ry * sin(theta)))
    return points


def outline(points, stroke=STROKE):
    """Offset a polyline by ±stroke/2 into a closed outline with butt caps."""
    half = stroke / 2
    normals = []
    for index, (x, y) in enumerate(points):
        px, py = points[max(0, index - 1)]
        nx, ny = points[min(len(points) - 1, index + 1)]
        dx, dy = nx - px, ny - py
        length = hypot(dx, dy) or 1.0
        normals.append((-dy / length, dx / length))

    left = [(x + nx * half, y + ny * half) for (x, y), (nx, ny) in zip(points, normals)]
    right = [(x - nx * half, y - ny * half) for (x, y), (nx, ny) in zip(points, normals)]
    return left + right[::-1]


def to_path_data(polygon, precision=2):
    head = f"M{polygon[0][0]:.{precision}f} {polygon[0][1]:.{precision}f}"
    rest = "".join(f"L{x:.{precision}f} {y:.{precision}f}" for x, y in polygon[1:])
    return head + rest + "Z"


def glyph_s(width=DEFAULT_WIDTH):
    """-> (svg path data, outer width)"""
    return to_path_data(outline(centerline(width))), width


if __name__ == "__main__":
    d, width = glyph_s()
    print(f"width={width} chars={len(d)}")
    open("glyph_s.txt", "w").write(d)
