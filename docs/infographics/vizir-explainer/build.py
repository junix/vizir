#!/usr/bin/env python3
"""Assemble index.html from frozen data + panels. Built-in assertions:

  * panel count == expected
  * headline counts from data/*.json must appear in their rendered
    stat-tile form (">N</text>") — anchored, not bare substrings
  * zero self-pollution: no <script>, no src="http…", no @import, no fetch(

Run:  python3 build.py   (twice in a row must produce byte-identical output)
"""
from __future__ import annotations

import base64
import json
import re
import sys
from pathlib import Path

import panels

HERE = Path(__file__).resolve().parent
SVG_DIR = HERE / "svg"
EXPECTED_PANELS = 12

HTML_HEAD = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=1200">
<title>vizir 技术长图 · 编译器问责对象</title>
<style>
  html, body { margin: 0; padding: 0; background: #F7F4EE; }
  body { width: 1200px; margin: 0 auto; font-family: 'Source Han Sans SC',
         'PingFang SC', sans-serif; color: #17212B; }
  header, footer { display: block; }
  section.chapter { display: block; }
  img.panel { display: block; width: 1200px; height: auto; border: 0; }
</style>
</head>
<body>
<header id="top"></header>
"""

HTML_FOOT = """<footer id="colophon">
<img class="panel" alt="" src="{footer_b64}">
</footer>
</body>
</html>
"""


def footer_svg(commit: str, total_h: int) -> str:
    from svgkit import (FLOW, FLOW_LT, FONT_MONO, INK, MUTED, PANEL, PAPER,
                        RULE, code, line, rect, svg, text)
    h = 150
    w = 1200
    out = [rect(0, 0, w, h, fill=PANEL)]
    out.append(line(48, 36, 1152, 36, stroke=RULE, sw=1))
    out.append(text(48, 66,
                    "vizir 技术长图 · 第五波 · 引擎已跟踪文件零改动 · 数据冻结于 prep_data.py 一次运行",
                    size=12, fill=INK))
    out.append(code(48, 88, f"engine commit {commit}", size=10.5,
                    fill=MUTED))
    out.append(code(48, 106,
                    "重建：python3 build.py（连续两次输出 byte 级一致）· 门禁：svg-linter check --plain 全 0 findings",
                    size=10.5, fill=FLOW_LT))
    out.append(code(48, 128,
                    "vizir explain / capabilities / render --manifest —— 问责三机制的日常入口",
                    size=10.5, fill=FLOW))
    return svg(w, h, *out)


def data_uri(svg_text: str) -> str:
    b64 = base64.b64encode(svg_text.encode("utf-8")).decode("ascii")
    return f"data:image/svg+xml;base64,{b64}"


def main() -> None:
    SVG_DIR.mkdir(exist_ok=True)
    rendered = panels.render_all()
    assert len(rendered) == EXPECTED_PANELS, \
        f"expected {EXPECTED_PANELS} panels, got {len(rendered)}"

    eng = json.loads((HERE / "data" / "engine.json").read_text())
    total_h = 0
    parts = [HTML_HEAD]
    for pid, svg_text in rendered:
        (SVG_DIR / f"{pid}.svg").write_text(svg_text)
        total_h += int(re.search(r'height="(\d+)"', svg_text).group(1))
        parts.append(
            f'<section class="chapter" id="{pid}">\n'
            f'<img class="panel" alt="{pid}" src="{data_uri(svg_text)}">\n'
            f'</section>\n')
    footer = footer_svg(eng["commit"], total_h)
    (SVG_DIR / "99-footer.svg").write_text(footer)
    parts.append(HTML_FOOT.format(footer_b64=data_uri(footer)))
    html = "".join(parts)
    (HERE / "index.html").write_text(html)

    # ---- assertions ------------------------------------------------------
    failures = []
    d = {name: json.loads((HERE / "data" / f"{name}.json").read_text())
         for name in ["cli", "tests", "diag_codes", "scene_nodes",
                      "capability", "examples"]}
    # Headline counts are asserted in their *rendered stat-tile form*
    # (">VALUE</text>"), never as bare substrings: a bare "7" also matches
    # inside "174" and a bare "11" inside "110", which structurally masked
    # tampering (2026-09 R11 fix). Seven numbers across six anchored forms
    # (diag codes and families share one combined tile value).
    must_appear = {
        "subcommand_count": f'>{d["cli"]["subcommand_count"]}</text>',
        "total tests": f'>{d["tests"]["total_passed"]}</text>',
        "diag codes × families":
            f'>{d["diag_codes"]["total_codes"]}×'
            f'{d["diag_codes"]["family_count"]}</text>',
        "scene nodes": f'>{d["scene_nodes"]["total_nodes"]}</text>',
        "svg decisions":
            f'>{d["capability"]["svg_manifest_decisions"]["total"]}</text>',
        "examples": f'>{d["examples"]["count"]}</text>',
    }
    svg_sources = "".join(p.read_text() for p in sorted(SVG_DIR.glob("*.svg")))
    # SVGs are base64-embedded into the html, so headline counts live in the
    # panel sources themselves.
    for label, needle in must_appear.items():
        if needle not in svg_sources:
            failures.append(f"headline {label}: anchored form {needle} "
                            f"missing from panel svg sources")

    everything = html + svg_sources
    for bad, label in [("<script", "script tag"), ('src="http', "remote src"),
                       ("@import", "css import"), ("fetch(", "fetch call")]:
        n = everything.count(bad)
        if n:
            failures.append(f"self-pollution {label}: {n} occurrence(s)")

    if failures:
        for f in failures:
            print("FAIL:", f, file=sys.stderr)
        sys.exit(1)
    heights = [int(re.search(r'height="(\d+)"', s).group(1))
               for _, s in rendered]
    print(f"built index.html: {EXPECTED_PANELS} panels + footer, "
          f"total panel height {sum(heights)}px, "
          f"all assertions green "
          f"({len(must_appear)} anchored headline-count forms, "
          f"4 pollution checks)")


if __name__ == "__main__":
    main()
