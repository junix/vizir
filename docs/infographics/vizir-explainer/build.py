#!/usr/bin/env python3
"""Assemble index.html from frozen data + panels. Built-in assertions:

  * panel count == expected
  * headline counts from data/*.json must appear in their rendered
    stat-tile form (">N</text>") — anchored, not bare substrings
  * zero self-pollution: no <script>, no src="http…", no @import, no fetch(
  * code-detail gate (2026-09-03 retrofit, fleet policy "code detail stays
    off the page"): six sweeps — file:line coords, engine source filenames,
    N–M line ranges, 第N行, source keywords, identifier call strings — plus
    a banned-identifier list, all zero on the page; new-form needles
    (pseudocode/diagram/text replacing retired verbatim cards) must appear

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

# Frozen snapshot of engine source-file basenames (`git -C <engine>
# ls-files`, code extensions only) plus this delivery's own generator
# filenames — the delivery tree is committed into the engine repo, so both
# classes hit the same sweep. Docs (*.md) and data/schema artifacts
# (*.json, *.yaml) are not code and stay citable.
ENGINE_CODE_FILES = [
    "Cargo.lock", "Cargo.toml", "build.py", "build_gallery.py",
    "capability.rs", "ci.yml", "cli.rs", "error.rs", "expression.rs",
    "hir.rs", "layout.rs", "lib.rs", "lower.rs", "main.rs", "mir.rs",
    "panels.py", "patch.rs", "pipeline.rs", "prep_data.py",
    "rustfmt.toml", "scene.rs", "scene_builder.rs", "shoot.js",
    "stitch.py", "svgkit.py", "validate.rs",
]
# Engine identifiers (function/type/variant/test names) and internal
# directory roots — banned from the page (fleet policy 2026-09-03); the
# audit anchors live in VERIFICATION.md and data/*.json.
BANNED_IDENTIFIERS = [
    "apply_scene_patch", "diff_scene", "negotiate_scene",
    "scene_capability_requirements", "VizError", "Diagnostic",
    "ScenePatch", "Revision(", "RemoveNode", "ReplaceNode", "InsertNode",
    "ReorderChildren", "BTreeSet", "struct Origin", "verify_png_alpha",
    "png_render_without_a_rasterizer", "schema_subcommand_emits",
    "diff_and_apply_match", "diff_rejects_cross", "apply_rejects_foreign",
    "apply_rejects_each", "revision_mismatch_rejects",
    "crates/", "src/", "python3 build",
]
RX_FILELINE = re.compile(
    r"\b[\w./-]+\.(?:rs|py|go|toml|swift|c|cpp|h|hpp|js|ts|java|cs):\d+")
RX_RANGE = re.compile(r":\d+\s*[-–—~]\s*\d+")
RX_NTHLINE = re.compile(r"第\s*\d+\s*行")
RX_KEYWORD = re.compile(
    r"\b(let|fn|impl|pub|use|match|struct|enum)\b|->|format!|self\."
    r"|assert_eq!|vec!")
RX_CALL = re.compile(r"\b[A-Za-z_]\w*\s*\(\s*[\d_\"']")
B64_URI = re.compile(r"data:image/svg\+xml;base64,[A-Za-z0-9+/=]+")


def code_detail_gate(svg_sources: str, html: str, failures: list) -> None:
    """Assert the page carries zero code-level detail. Base64 payloads are
    stripped from the html before sweeping (they are byte-identical to the
    svg sources, which are swept in full)."""
    page = svg_sources + B64_URI.sub("", html)

    def zero(rx, label):
        hits = rx.findall(page)
        if hits:
            failures.append(f"code-detail {label}: {len(hits)} hit(s), "
                            f"e.g. {hits[:3]}")

    zero(RX_FILELINE, "file:line coordinate")
    zero(RX_NTHLINE, "第N行")
    zero(RX_RANGE, "N–M line range")
    zero(RX_CALL, "identifier call string")
    zero(RX_KEYWORD, "source keyword")
    for name in ENGINE_CODE_FILES:
        n = len(re.findall(r"(?<![\w./-])" + re.escape(name)
                           + r"(?![\w.-])", page))
        if n:
            failures.append(f"code-detail engine filename {name}: "
                            f"{n} hit(s)")
    for ident in BANNED_IDENTIFIERS:
        n = page.count(ident)
        if n:
            failures.append(f"code-detail identifier {ident!r}: {n} hit(s)")

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
                    "vizir 技术长图 · 第五波 · 引擎已跟踪文件零改动 · 证据冻结于一次真实引擎运行",
                    size=12, fill=INK))
    out.append(code(48, 88, f"engine commit {commit}", size=10.5,
                    fill=MUTED))
    out.append(code(48, 106,
                    "重建命令与验收管线见 README / VERIFICATION · 本页声明 E1–E6 的证据链见 VERIFICATION.md",
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
                      "capability", "examples", "patch"]}
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

    # New-form needles (2026-09-03 retrofit): the two retired verbatim
    # source cards must survive as pseudocode / envelope diagram, and the
    # claim-ID citation layer (E1–E6 → VERIFICATION) must be on the page.
    new_forms = {
        "pseudocode card (fail-loud)": "谈判破裂的唯一出口（伪代码）",
        "pseudocode names the diagnostic":
            "一条稳定诊断 VIZ-CAP-0002",
        "envelope card title": "补丁信封六要素",
        "envelope base rule": "必须对上当前场景版本",
        "envelope target rule": "必须严格前进",
        "ops order law": "顺序铁律：先删 → 再改/插 → 最后排序",
        "equivalence claim strengthened": "严格相等",
        "suite label (derived from per_suite)":
            f'核心库 {d["tests"]["per_suite"]["vizir-core (lib)"]}',
        "patch test count (derived)": f'{d["patch"]["test_count"]} 个专项测试',
        "claim registry": "声明登记簿",
    }
    for label, needle in new_forms.items():
        if needle not in svg_sources:
            failures.append(f"new-form {label}: needle {needle!r} "
                            f"missing from panel svg sources")
    for eid in ["E1", "E2", "E3", "E4", "E5", "E6"]:
        if eid not in svg_sources:
            failures.append(f"claim id {eid}: missing from panel svg sources")
    if (svg_sources + B64_URI.sub("", html)).count("VERIFICATION") < 12:
        failures.append("claim pointers: fewer than 12 VERIFICATION "
                        "references on the page")

    everything = html + svg_sources
    for bad, label in [("<script", "script tag"), ('src="http', "remote src"),
                       ("@import", "css import"), ("fetch(", "fetch call")]:
        n = everything.count(bad)
        if n:
            failures.append(f"self-pollution {label}: {n} occurrence(s)")

    code_detail_gate(svg_sources, html, failures)

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
          f"{len(new_forms)} new-form needles, "
          f"4 pollution checks, code-detail gate: 6 sweeps + "
          f"{len(ENGINE_CODE_FILES)} filenames + "
          f"{len(BANNED_IDENTIFIERS)} identifiers all zero)")


if __name__ == "__main__":
    main()
