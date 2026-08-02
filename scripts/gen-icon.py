#!/usr/bin/env python3
"""Generate the Hotwire app icon (a bent amber wire joining two nodes).

Pure-stdlib PNG writer; no Pillow required. Re-run after tweaking to refresh
apps/desktop/src-tauri/icons/icon.png. For a full platform icon set (icns, ico,
RGBA at all sizes), pipe this PNG into `pnpm tauri icon` from
apps/desktop/src-tauri.
"""

import math
import struct
import sys
import zlib
from pathlib import Path

SIZE = 128
AMBER = (241, 179, 91)
INK = (17, 17, 15)

# Geometry in unit coordinates (0..1), y grows downward.
NODES = [(0.26, 0.74), (0.74, 0.26)]
WIRE = [(0.26, 0.74), (0.50, 0.50), (0.74, 0.26)]
SPARK = (0.50, 0.50)
RADIUS = 0.085
STROKE = 0.045
SPARK_RADIUS = 0.030


def dist_to_segment(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    length_sq = vx * vx + vy * vy
    if length_sq == 0.0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / length_sq))
    cx, cy = ax + t * vx, ay + t * vy
    return math.hypot(px - cx, py - cy)


def coverage(px, py, cx, cy, radius):
    """Anti-aliased circle coverage at (px,py)."""
    return clamp(radius + 0.5 - math.hypot(px - cx, py - cy))


def coverage_stroke(px, py, ax, ay, bx, by, width):
    return clamp(width + 0.5 - dist_to_segment(px, py, ax, ay, bx, by))


def clamp(value):
    return max(0.0, min(1.0, value))


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("icon.png")
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)
        for x in range(SIZE):
            px = x + 0.5
            py = y + 0.5
            # Blend from ink (wire/nodes) over amber "current" background.
            r, g, b = AMBER
            alpha = 0.0
            for nx, ny in NODES:
                a = coverage(px / SIZE, py / SIZE, nx, ny, RADIUS)
                if a > 0.0:
                    alpha = max(alpha, a)
                    r, g, b = _lerp_color(INK, (r, g, b), a)
            for i in range(len(WIRE) - 1):
                ax, ay = WIRE[i]
                bx, by = WIRE[i + 1]
                a = coverage_stroke(px / SIZE, py / SIZE, ax, ay, bx, by, STROKE)
                if a > 0.0:
                    alpha = max(alpha, a)
                    r, g, b = _lerp_color(INK, (r, g, b), a)
            a = coverage(px / SIZE, py / SIZE, SPARK[0], SPARK[1], SPARK_RADIUS)
            if a > 0.0:
                alpha = max(alpha, a)
            raw.extend((int(r), int(g), int(b), int(clamp(alpha) * 255)))
    _write_png(out, raw)
    return 0


def _lerp_color(fg, bg, t):
    return tuple(int(f * t + b * (1.0 - t)) for f, b in zip(fg, bg))


def _write_png(path, raw):
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    chunk = b"\x89PNG\r\n\x1a\n"
    chunk += _chunk(b"IHDR", header)
    chunk += _chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    chunk += _chunk(b"IEND", b"")
    path.write_bytes(chunk)


def _chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(
        ">I", zlib.crc32(kind + data)
    )


if __name__ == "__main__":
    raise SystemExit(main())
