#!/usr/bin/env python3
"""Generates the app icon set in packaging/icons/ (needs Pillow and numpy).

The icon is the app in miniature: two offset sets of rings whose interference makes a
moiré, colored with the "Prism" palette, on a dark rounded square. Run from anywhere:

    python3 scripts/make-icons.py

Writes PNGs at 32-1024 px, icon.ico (Windows) and, on macOS, icon.icns.
"""

import pathlib
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "packaging" / "icons"
SIZE = 1024
SS = 2  # supersampling for smooth edges


def palette(t):
    """Prism: a + b * cos(2π(c·t + d)) with a = b = 0.5, c = 1, d = (0, .33, .67)."""
    d = np.array([0.0, 0.33, 0.67])
    return 0.5 + 0.5 * np.cos(2 * np.pi * (t[..., None] + d))


def render(n):
    y, x = np.mgrid[0:n, 0:n].astype(np.float64)
    u = (x + 0.5) / n * 2 - 1
    v = (y + 0.5) / n * 2 - 1

    # Two ring sets, offset left and right; their product makes the moiré.
    off = 0.16
    r1 = np.hypot(u - off, v + 0.04)
    r2 = np.hypot(u + off, v - 0.04)
    freq = 22.0
    w1 = 0.5 + 0.5 * np.cos(r1 * freq * np.pi)
    w2 = 0.5 + 0.5 * np.cos(r2 * freq * np.pi)
    moire = w1 * w2

    r = np.hypot(u, v)
    ang = np.arctan2(v, u) / (2 * np.pi)
    col = palette(ang + r * 0.35 + 0.05)

    # Rings glow in the palette over a deep base; fade toward the edge.
    base = np.array([0.05, 0.04, 0.09])
    glow = np.clip(1.15 - r * 0.85, 0.0, 1.0) ** 1.4
    rgb = base + (col * (0.25 + 0.95 * moire[..., None]) - base) * glow[..., None]
    rgb = np.clip(rgb, 0.0, 1.0)

    # Rounded square (macOS-style ~22% corner radius) with a small margin.
    m = 0.09
    half = 1 - m
    rad = 0.22 * 2 * half
    qx = np.maximum(np.abs(u) - (half - rad), 0)
    qy = np.maximum(np.abs(v) - (half - rad), 0)
    inside = np.hypot(qx, qy) <= rad
    alpha = inside.astype(np.float64)

    img = np.dstack([rgb, alpha])
    return Image.fromarray((img * 255).round().astype(np.uint8), "RGBA")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    big = render(SIZE * SS).resize((SIZE, SIZE), Image.LANCZOS)
    sizes = [32, 64, 128, 256, 512, 1024]
    for s in sizes:
        big.resize((s, s), Image.LANCZOS).save(OUT / f"{s}x{s}.png")
    big.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

    if shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = pathlib.Path(tmp) / "icon.iconset"
            iconset.mkdir()
            for s in [16, 32, 128, 256, 512]:
                big.resize((s, s), Image.LANCZOS).save(iconset / f"icon_{s}x{s}.png")
                big.resize((s * 2, s * 2), Image.LANCZOS).save(iconset / f"icon_{s}x{s}@2x.png")
            subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / "icon.icns")], check=True)
    else:
        print("iconutil not found (macOS only): icon.icns not regenerated", file=sys.stderr)
    print(f"wrote icons to {OUT}")


if __name__ == "__main__":
    main()
