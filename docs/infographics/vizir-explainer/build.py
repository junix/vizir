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
  * inline-medium gate (2026-09-06 refine): panels are inline <svg> blocks
    byte-identical to svg/*.svg (zero <img>), headings live in HTML
    (1×h1 + 11×h2 + kicker/lede layers), and SVG text honors the fleet
    font floors — CJK never below 12px, everything >= 11px
  * digit sweep gate (2026-09-07 audit hardening): every digit sequence
    in the reader-visible projection must be backed by the frozen data
    layer (substring of data/*.json bytes) or sit on the reviewed
    DIGIT_EXEMPTIONS list below — hand-typed numbers cannot drift in

Run:  python3 build.py   (twice in a row must produce byte-identical output)
"""
from __future__ import annotations

import json
import re
import sys
from html import escape
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

# Reviewed digit-sweep exemption list (2026-09-07, audit-batteries §2
# "reverse number sweep"). A page digit token passes the sweep when it is
# a substring of the frozen data layer (the value literally exists in
# data/*.json — panel ordinals, VIZ-*-NNNN code numbers, sha prefixes and
# example values all resolve through that channel) or listed here with a
# justification. Extend this table only with reviewed entries; never by
# deleting page numbers.
DIGIT_EXEMPTIONS = {
    # 「覆盖率 100%」(05-coverage lede) and registry row E1 「覆盖 100%」:
    # 100% = 110/110 Scene2D nodes carrying an origin object. The 110 is
    # frozen (scene_nodes.json total_nodes == sum of generated_by_histogram)
    # but the nodes array itself is not in the frozen layer, so the 100 is
    # a reviewed derived value — per-node origin presence was hand-verified
    # at freeze time (VERIFICATION §3, §7/R3).
    "100": "origin 覆盖率 100% = 110/110（110 冻结于 scene_nodes.json；逐节点 origin 全present 为冻结时人工核验，VERIFICATION §7/R3）",
}
RX_FILELINE = re.compile(
    r"\b[\w./-]+\.(?:rs|py|go|toml|swift|c|cpp|h|hpp|js|ts|java|cs):\d+")
RX_RANGE = re.compile(r":\d+\s*[-–—~]\s*\d+")
RX_NTHLINE = re.compile(r"第\s*\d+\s*行")
RX_KEYWORD = re.compile(
    r"\b(let|fn|impl|pub|use|match|struct|enum)\b|->|format!|self\."
    r"|assert_eq!|vec!")
RX_CALL = re.compile(r"\b[A-Za-z_]\w*\s*\(\s*[\d_\"']")
# <text …>content</text> pairs (svgkit emits leaf text elements only)
RX_TEXT_EL = re.compile(r"<text ([^>]*)>([^<]*)</text>")
RX_INLINE_SVG = re.compile(r"<svg[\s\S]*?</svg>")
RX_STYLE = re.compile(r"<style[\s\S]*?</style>")
RX_TAG = re.compile(r"<[^>]+>")


def visible_text(svg_sources: str, html: str) -> str:
    """Reader-visible text projection: SVG <text> contents + HTML text
    nodes (kicker/h1/h2/lede/figcaption/title). Inline <svg> blocks are
    removed from the html first — their <text> pairs are already swept via
    svg_sources (byte-identical, inline-medium gate). Used by the
    call-string ban (attributes like transform="translate(0 …" are SVG
    syntax, not reader-visible source code) and by the digit sweep."""
    from html import unescape
    parts = [c for _, c in RX_TEXT_EL.findall(svg_sources)]
    h = RX_STYLE.sub("", RX_INLINE_SVG.sub("", html))
    parts.append(unescape(RX_TAG.sub("\n", h)))
    return "\n".join(parts)


def code_detail_gate(svg_sources: str, html: str, failures: list) -> None:
    """Assert the page carries zero code-level detail. Panels are inlined
    byte-identical into the html, so sweeping html covers the svg sources
    as well; svg_sources is swept too for belt and braces.

    (2026-09-07 audit-hardening fix: the 2026-09-06 font-floor insertion
    had accidentally moved this body to dead code after font_floor_gate's
    return — the gate reported "all zero" while sweeping nothing. A
    file:line poison pill sailed through; restored + now pill-proven, see
    VERIFICATION §13 and data/audit/pills.jsonl.)"""
    page = svg_sources + html
    # Call-string ban runs on the reader-visible projection: SVG transform
    # attributes ("translate(0 -174)" — wave-1 geometry, 24 raw hits) are
    # presentation syntax, not source code; a visible-text call pill still
    # bites (data/audit/pills.jsonl, pill class "identifier-call-string").
    visible = visible_text(svg_sources, html)

    def zero(rx, label, hay=None):
        hits = rx.findall(hay if hay is not None else page)
        if hits:
            failures.append(f"code-detail {label}: {len(hits)} hit(s), "
                            f"e.g. {hits[:3]}")

    zero(RX_FILELINE, "file:line coordinate")
    zero(RX_NTHLINE, "第N行")
    zero(RX_RANGE, "N–M line range")
    zero(RX_CALL, "identifier call string", visible)
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


def font_floor_gate(svg_sources: str, failures: list) -> float:
    """Fleet hard rule (2026-09-06 refine): no SVG text below 11px, no CJK
    text below 12px (enforces the >=90%-at-11px rule at 100%)."""
    worst = 99.0
    for attrs, content in RX_TEXT_EL.findall(svg_sources):
        m = re.search(r'font-size="([\d.]+)"', attrs)
        size = float(m.group(1))
        plain = (content.replace("&amp;", "&").replace("&lt;", "<")
                 .replace("&gt;", ">").replace("&quot;", '"'))
        if any(ord(c) > 0x2E7F for c in plain) and size < 12:
            failures.append(f"font floor: CJK {size}px < 12 "
                            f"({plain[:18]!r})")
        worst = min(worst, size)
        if size < 11:
            failures.append(f"font floor: {size}px < 11 ({plain[:18]!r})")
    return worst


def digit_sweep_gate(svg_sources: str, html: str, failures: list) -> int:
    """Reverse number sweep (audit-batteries §2): every digit sequence in
    the reader-visible projection must be claimed — either it occurs as a
    substring of the frozen data layer (the value literally exists in
    data/*.json: stat values, VIZ-*-NNNN codes, sha/engine-commit
    prefixes, example values, per-suite counts), or it sits on the
    reviewed DIGIT_EXEMPTIONS list. Returns the count of distinct claimed
    tokens so the build banner can report sweep coverage."""
    blob = "".join(p.read_text() for p in sorted((HERE / "data").glob("*.json")))
    vis = visible_text(svg_sources, html)
    tokens = set(re.findall(r"[0-9][0-9.]*", vis))
    unclaimed = sorted(t for t in tokens
                       if t not in blob and t.rstrip(".") not in blob
                       and t not in DIGIT_EXEMPTIONS)
    for t in unclaimed:
        why = ("not a substring of any data/*.json and not on the "
               "DIGIT_EXEMPTIONS list — hand-typed numbers are banned; "
               "interpolate from frozen data or add a reviewed exemption")
        failures.append(f"digit sweep: unclaimed token {t!r} ({why})")
    return len(tokens) - len(unclaimed)

HTML_HEAD = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=1200">
<title>vizir 技术长图 · 编译器问责对象</title>
<style>
  html, body { margin: 0; padding: 0; background: #F7F4EE; }
  body { max-width: 1200px; margin: 0 auto; font-family: 'Source Han Sans SC',
         'PingFang SC', sans-serif; color: #17212B; }
  header, footer { display: block; }
  section.chapter { display: block; padding: 22px 0 0; }
  section.chapter > p.kicker {
    margin: 0 48px 8px; font-family: '0xProto Nerd Font', 'SF Mono', Menlo,
    monospace; font-size: 12px; font-weight: 700; color: #356A79;
    letter-spacing: 2px; }
  section.chapter > h1 {
    margin: 0 48px 10px; font-family: 'Source Han Serif SC', 'PingFang SC',
    serif; font-size: 40px; line-height: 1.25; font-weight: 700;
    color: #17212B; }
  section.chapter > h2 {
    margin: 0 48px 8px; font-family: 'Source Han Serif SC', 'PingFang SC',
    serif; font-size: 25px; line-height: 1.3; font-weight: 700;
    color: #17212B; }
  section.chapter > p.lede {
    margin: 0 48px 12px; font-size: 13px; line-height: 1.7;
    color: #5D6873; }
  section#01-hero > p.lede { font-size: 14px; }
  figure.fig { margin: 0; }
  figure.fig figcaption {
    margin: 5px 48px 0; font-size: 12px; line-height: 1.5;
    color: #5D6873; }
  svg.panel { display: block; }
</style>
</head>
<body>
<header id="top"></header>
"""

HTML_FOOT = """</body>
</html>
"""

# figcaption claim pointers — derived from VERIFICATION §11.6 覆盖章节
# (hero cards carry E1/E2/E3; chapters map 1:1 to the registry rows).
FIGCAPTIONS = {
    "01-hero": "声明 E1·E2·E3（三机制卡与档案数字）· 证据链见 VERIFICATION.md",
    "02-pipeline": "声明 E4 · 证据链见 VERIFICATION.md",
    "03-origin": "声明 E1 · 证据链见 VERIFICATION.md",
    "04-explain-tree": "声明 E1 · 证据链见 VERIFICATION.md",
    "05-coverage": "声明 E1 · 证据链见 VERIFICATION.md",
    "06-capability-surface": "声明 E2 · 证据链见 VERIFICATION.md",
    "07-decisions": "声明 E2 · 证据链见 VERIFICATION.md",
    "08-fail-loud": "声明 E2 · 证据链见 VERIFICATION.md",
    "09-loss": "声明 E5 · 证据链见 VERIFICATION.md",
    "10-patch-gate": "声明 E3 · 证据链见 VERIFICATION.md",
    "11-patch-equivalence": "声明 E3 · 证据链见 VERIFICATION.md",
    "12-gates": "声明 E6 · 证据链见 VERIFICATION.md",
}


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
        kicker, title, lede = panels.HEADS[pid]
        heading = "h1" if pid == "01-hero" else "h2"
        parts.append(
            f'<section class="chapter" id="{pid}">\n'
            f'<p class="kicker">{escape(kicker)}</p>\n'
            f'<{heading}>{escape(title)}</{heading}>\n'
            f'<p class="lede">{escape(lede)}</p>\n'
            f'<figure class="fig">\n{svg_text}\n'
            f'<figcaption>{escape(FIGCAPTIONS[pid])}</figcaption>\n'
            f'</figure>\n</section>\n')
    footer = footer_svg(eng["commit"], total_h)
    (SVG_DIR / "99-footer.svg").write_text(footer)
    parts.append(f'<footer id="colophon">\n{footer}\n</footer>\n')
    parts.append(HTML_FOOT)
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
    # Panels are inlined byte-identical into the html, so headline counts
    # live in the panel sources and in index.html alike.
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
    # Claim-id coverage, anchored on the reader-visible projection with a
    # two-way binding (2026-09-07): a bare substring check over raw svg
    # sources was structurally masked — "E1" survives inside the hex
    # attribute #D9E1E3, so deleting every real E1 chip sailed through
    # (pill record: data/audit/pills.json). Registry ids must appear as
    # standalone tokens in page text, and no E-number may appear that is
    # not in the E1-E6 registry.
    vis_text = visible_text(svg_sources, html)
    vis_svg_text = visible_text(svg_sources, "")
    for eid in ["E1", "E2", "E3", "E4", "E5", "E6"]:
        if not re.search(rf"(?<![A-Za-z0-9]){eid}(?![0-9])", vis_svg_text):
            failures.append(f"claim id {eid}: missing from page text")
    for n in sorted(set(re.findall(r"(?<![A-Za-z0-9])E(\d+)(?![0-9])",
                                   vis_text))
                    - {"1", "2", "3", "4", "5", "6"}):
        failures.append(f"claim id E{n}: on the page but not in the "
                        "E1-E6 registry (two-way binding)")
    if (svg_sources + html).count("VERIFICATION") < 12:
        failures.append("claim pointers: fewer than 12 VERIFICATION "
                        "references on the page")

    # ---- inline-medium gate (2026-09-06 refine) --------------------------
    # zero <img>: every panel is an inline <svg> byte-identical to its
    # svg/*.svg file; headings are HTML (1 h1 + 11 h2 + 12 kickers).
    if "<img" in html:
        failures.append("inline-medium: <img> present (must be zero)")
    n_svg = html.count("<svg ")
    if n_svg != EXPECTED_PANELS + 1:
        failures.append(f"inline-medium: {n_svg} inline <svg> blocks, "
                        f"expected {EXPECTED_PANELS + 1}")
    for pid, svg_text in rendered:
        if html.count(svg_text) != 1:
            failures.append(f"inline-medium: svg/{pid}.svg bytes not "
                            "embedded exactly once")
    if html.count(footer) != 1:
        failures.append("inline-medium: svg/99-footer.svg bytes not "
                        "embedded exactly once")
    if html.count("<h1>") != 1 or html.count("<h2>") != EXPECTED_PANELS - 1:
        failures.append("inline-medium: heading layer must be 1 h1 + "
                        f"{EXPECTED_PANELS - 1} h2 in HTML")
    if html.count('<p class="kicker">') != EXPECTED_PANELS:
        failures.append("inline-medium: kicker layer incomplete")
    worst = font_floor_gate(svg_sources, failures)

    everything = html + svg_sources
    for bad, label in [("<script", "script tag"), ('src="http', "remote src"),
                       ("@import", "css import"), ("fetch(", "fetch call")]:
        n = everything.count(bad)
        if n:
            failures.append(f"self-pollution {label}: {n} occurrence(s)")

    code_detail_gate(svg_sources, html, failures)
    claimed = digit_sweep_gate(svg_sources, html, failures)

    if failures:
        for f in failures:
            print("FAIL:", f, file=sys.stderr)
        sys.exit(1)
    heights = [int(re.search(r'height="(\d+)"', s).group(1))
               for _, s in rendered]
    print(f"built index.html: {EXPECTED_PANELS} inline-svg panels + footer, "
          f"total panel height {sum(heights)}px, "
          f"all assertions green "
          f"({len(must_appear)} anchored headline-count forms, "
          f"{len(new_forms)} new-form needles, "
          f"4 pollution checks, code-detail gate: 6 sweeps + "
          f"{len(ENGINE_CODE_FILES)} filenames + "
          f"{len(BANNED_IDENTIFIERS)} identifiers all zero, "
          f"inline-medium + font floors (min text {worst}px), "
          f"digit sweep: {claimed} distinct tokens all claimed "
          f"({len(DIGIT_EXEMPTIONS)} reviewed exemption(s))")


if __name__ == "__main__":
    main()
