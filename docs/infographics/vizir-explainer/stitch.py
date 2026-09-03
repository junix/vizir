#!/usr/bin/env python3
"""Stitch fixed-height slices (from y=0, full width) into the final PNGs.

    node shoot.js "file://$PWD/index.html" render   # writes render/slice-*.png
    python3 stitch.py                                # writes render/*.png

Built-in assertion: stitched bitmap height must equal page CSS height x 2
(dpr 2). Also emits thumb, grayscale proof, and per-panel 1:1 crops cut from
the stitched bitmap at exact panel boundaries.
"""
from __future__ import annotations

import json
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
RENDER = HERE / "render"
PAPER = "#F7F4EE"
WIDTH_CSS = 1200
DPR = 2


def main() -> None:
    meta = json.loads((RENDER / "sections.json").read_text())
    page_h = meta["page"]["h"]
    page_w = meta["page"]["w"]
    assert page_w == WIDTH_CSS, f"page width {page_w} != {WIDTH_CSS}"

    slices = [Path(line) for line in
              (RENDER / "slices.txt").read_text().splitlines() if line]
    assert slices, "no slices found"
    imgs = [Image.open(p) for p in slices]
    for p, im in zip(slices, imgs):
        assert im.width == WIDTH_CSS * DPR, \
            f"{p.name} width {im.width} != {WIDTH_CSS * DPR}"
    width = max(im.width for im in imgs)
    height = sum(im.height for im in imgs)
    assert height == page_h * DPR, \
        f"stitched height {height} != page css height {page_h} x {DPR}"

    canvas = Image.new("RGB", (width, height), PAPER)
    y = 0
    for im in imgs:
        canvas.paste(im, (0, y))
        y += im.height
    canvas.save(RENDER / "full@2x.png")
    canvas.resize((width // 4, height // 4), Image.LANCZOS).save(
        RENDER / "thumb.png")
    canvas.convert("L").save(RENDER / "full@2x.gray.png")

    for s in meta["sections"]:
        sid, sy, sh = s["id"], s["y"], s["h"]
        if sh <= 0:
            continue
        crop = canvas.crop((0, sy * DPR, width, (sy + sh) * DPR))
        crop.save(RENDER / f"panel-{sid}@2x.png")

    print(f"full@2x {canvas.size} (page {page_w}x{page_h} css px, dpr {DPR}) "
          f"-> thumb {width // 4}x{height // 4}, gray + "
          f"{len(meta['sections'])} panel crops")


if __name__ == "__main__":
    main()
