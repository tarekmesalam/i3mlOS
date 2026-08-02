#!/usr/bin/env python3
"""Minimal SVG-path rasterizer: parse `d`, flatten beziers, scanline fill.

Pure stdlib. Supports M/L/H/V/C/S/Q/T/Z (absolute + relative), nonzero and
evenodd fill rules, 4x supersampling for antialiasing.
"""
import re
from math import ceil, floor

TOKEN = re.compile(r"[MmLlHhVvCcSsQqTtAaZz]|-?\d*\.?\d+(?:[eE][-+]?\d+)?")


def parse_path(d):
    """-> list of subpaths, each a list of (x, y) points (already flattened)."""
    tokens = TOKEN.findall(d)
    i = 0
    subpaths, current = [], []
    cx = cy = 0.0
    start_x = start_y = 0.0
    prev_ctrl = None
    command = None

    def num():
        nonlocal i
        value = float(tokens[i])
        i += 1
        return value

    def flatten_cubic(x0, y0, x1, y1, x2, y2, x3, y3, steps=None):
        if steps is None:
            # Chord-length heuristic: enough segments to stay sub-pixel.
            span = abs(x3 - x0) + abs(y3 - y0) + abs(x1 - x0) + abs(y1 - y0) + abs(x2 - x3) + abs(y2 - y3)
            steps = max(4, min(96, int(span / 3)))
        points = []
        for step in range(1, steps + 1):
            t = step / steps
            u = 1 - t
            points.append((
                u * u * u * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3,
                u * u * u * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3,
            ))
        return points

    while i < len(tokens):
        token = tokens[i]
        if token.isalpha():
            command = token
            i += 1
        elif command in ("M", "m"):
            command = "L" if command == "M" else "l"
        # implicit repeat: keep previous command

        relative = command.islower()
        op = command.upper()

        if op == "Z":
            if current:
                current.append((start_x, start_y))
                subpaths.append(current)
                current = []
            cx, cy = start_x, start_y
            prev_ctrl = None
            continue

        if op == "M":
            x, y = num(), num()
            if relative:
                x, y = cx + x, cy + y
            if current:
                subpaths.append(current)
            current = [(x, y)]
            cx = cy = None
            cx, cy = x, y
            start_x, start_y = x, y
            prev_ctrl = None
        elif op == "L":
            x, y = num(), num()
            if relative:
                x, y = cx + x, cy + y
            current.append((x, y))
            cx, cy = x, y
            prev_ctrl = None
        elif op == "H":
            x = num()
            x = cx + x if relative else x
            current.append((x, cy))
            cx = x
            prev_ctrl = None
        elif op == "V":
            y = num()
            y = cy + y if relative else y
            current.append((cx, y))
            cy = y
            prev_ctrl = None
        elif op in ("C", "S"):
            if op == "C":
                x1, y1 = num(), num()
                if relative:
                    x1, y1 = cx + x1, cy + y1
            else:
                x1, y1 = (2 * cx - prev_ctrl[0], 2 * cy - prev_ctrl[1]) if prev_ctrl else (cx, cy)
            x2, y2 = num(), num()
            x3, y3 = num(), num()
            if relative:
                x2, y2, x3, y3 = cx + x2, cy + y2, cx + x3, cy + y3
            current.extend(flatten_cubic(cx, cy, x1, y1, x2, y2, x3, y3))
            prev_ctrl = (x2, y2)
            cx, cy = x3, y3
        elif op in ("Q", "T"):
            if op == "Q":
                qx, qy = num(), num()
                if relative:
                    qx, qy = cx + qx, cy + qy
            else:
                qx, qy = (2 * cx - prev_ctrl[0], 2 * cy - prev_ctrl[1]) if prev_ctrl else (cx, cy)
            x3, y3 = num(), num()
            if relative:
                x3, y3 = cx + x3, cy + y3
            # quadratic -> cubic
            current.extend(flatten_cubic(
                cx, cy,
                cx + 2 / 3 * (qx - cx), cy + 2 / 3 * (qy - cy),
                x3 + 2 / 3 * (qx - x3), y3 + 2 / 3 * (qy - y3),
                x3, y3,
            ))
            prev_ctrl = (qx, qy)
            cx, cy = x3, y3
        else:
            raise ValueError(f"unsupported command {command!r}")

    if current:
        subpaths.append(current)
    return subpaths


def rasterize(subpaths, width, height, transform=lambda p: p, evenodd=False, ss=4):
    """-> width*height list of floats in 0..1 (coverage), antialiased."""
    edges = []
    for points in subpaths:
        pts = [transform(p) for p in points]
        if pts[0] != pts[-1]:
            pts.append(pts[0])
        for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
            if y0 != y1:
                edges.append((x0, y0 * ss, x1, y1 * ss, x0 * ss, x1 * ss))
    if not edges:
        return [0.0] * (width * height)

    acc = [0] * (width * height)
    sw = width * ss
    for sy in range(height * ss):
        y = sy + 0.5
        crossings = []
        for _, y0, _, y1, sx0, sx1 in edges:
            if (y0 <= y < y1) or (y1 <= y < y0):
                t = (y - y0) / (y1 - y0)
                crossings.append((sx0 + t * (sx1 - sx0), 1 if y1 > y0 else -1))
        if not crossings:
            continue
        crossings.sort()
        spans = []
        if evenodd:
            for index in range(0, len(crossings) - 1, 2):
                spans.append((crossings[index][0], crossings[index + 1][0]))
        else:
            winding = 0
            span_start = None
            for x, direction in crossings:
                was_inside = winding != 0
                winding += direction
                if not was_inside and winding != 0:
                    span_start = x
                elif was_inside and winding == 0 and span_start is not None:
                    spans.append((span_start, x))
                    span_start = None
        row = (sy // ss) * width
        for x_start, x_end in spans:
            px0 = max(0, int(floor(x_start)))
            px1 = min(sw, int(ceil(x_end)))
            for sx in range(px0, px1):
                # subpixel coverage of this sample column
                covered = min(sx + 1, x_end) - max(sx, x_start)
                if covered > 0.5:
                    acc[row + sx // ss] += 1
    scale = ss * ss
    return [min(1.0, value / scale) for value in acc]


def write_pgm(path, coverage, width, height):
    header = f"P5\n{width} {height}\n255\n".encode()
    body = bytes(int(round(value * 255)) for value in coverage)
    with open(path, "wb") as f:
        f.write(header + body)


def write_ppm(path, rgb, width, height):
    header = f"P6\n{width} {height}\n255\n".encode()
    with open(path, "wb") as f:
        f.write(header + bytes(rgb))
