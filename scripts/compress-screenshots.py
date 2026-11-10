#!/usr/bin/env python3
"""Converts site/screenshots/*.png (from examples/screenshots.rs) to WebP for the site.

Needs Pillow. Run by `mise run screenshots` after the renderer. The PNGs are removed once
converted; only the .webp files are committed.
"""

import pathlib

from PIL import Image

DIR = pathlib.Path(__file__).resolve().parent.parent / "site" / "screenshots"

for png in sorted(DIR.glob("*.png")):
    out = png.with_suffix(".webp")
    Image.open(png).convert("RGB").save(out, "WEBP", quality=84, method=6)
    print(f"{out.name}: {out.stat().st_size // 1024} KB")
    png.unlink()
