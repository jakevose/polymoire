#!/usr/bin/env python3
"""Converts site/screenshots/*.png (from examples/screenshots.rs) to WebP for the site.

Needs Pillow. Run by `mise run screenshots` after the renderer. The PNGs are removed once
converted; only the .webp files are committed.

Also writes site/og.jpg, the 1200x630 link-preview image (Slack, social sites), cropped
from OG_SOURCE. Previewers want an absolute URL to a JPEG or PNG, not WebP.
"""

import pathlib

from PIL import Image

DIR = pathlib.Path(__file__).resolve().parent.parent / "site" / "screenshots"
OG_SOURCE = "glacier-cells"
OG_SIZE = (1200, 630)

for png in sorted(DIR.glob("*.png")):
    out = png.with_suffix(".webp")
    Image.open(png).convert("RGB").save(out, "WEBP", quality=84, method=6)
    print(f"{out.name}: {out.stat().st_size // 1024} KB")
    png.unlink()

src = Image.open(DIR / f"{OG_SOURCE}.webp").convert("RGB")
w, h = src.size
crop_h = round(w * OG_SIZE[1] / OG_SIZE[0])
top = (h - crop_h) // 2
og = src.crop((0, top, w, top + crop_h)).resize(OG_SIZE, Image.LANCZOS)
og.save(DIR.parent / "og.jpg", "JPEG", quality=86, optimize=True, progressive=True)
print(f"og.jpg: {(DIR.parent / 'og.jpg').stat().st_size // 1024} KB")
