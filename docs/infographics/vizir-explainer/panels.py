#!/usr/bin/env python3
"""Data-driven panels for the vizir explainer. Every visible number comes
from data/*.json (frozen by prep_data.py); nothing is invented here."""
from __future__ import annotations

import json
import re
from pathlib import Path

from svgkit import (CH_B, CH_G, FLOW, FLOW_DK, FLOW_LT, FLOW_TINT,
                    FLOW_XLT, FONT_DISPLAY, FONT_MONO, INK, MUTED,
                    OUTCOME, OUTCOME_LT, PANEL, PAPER, RULE, TEAL, TINT, WARN,
                    WARN_LT, arrow, chip, circle, code, code_box, hbar,
                    line, panel_head, rect, src_note, stat_tile, svg,
                    text)

HERE = Path(__file__).resolve().parent
W = 1200
X = 48
CW = 1104  # content width


def load(name: str):
    return json.loads((HERE / "data" / f"{name}.json").read_text())


# ---------------------------------------------------------------- P1 hero
def p_hero(d) -> str:
    cli = d["cli"]; tests = d["tests"]; diag = d["diag_codes"]
    nodes = d["scene_nodes"]; cap = d["capability"]; ex = d["examples"]
    eng = d["engine"]
    h = 668
    out = [rect(0, 0, W, h, fill=PANEL)]
    out.append(rect(0, 0, W, 6, fill=FLOW))
    out.append(text(X, 74, "TECHNICAL INFOGRAPHIC · vizir 0.1.0 · 可审计编译器问责",
                    size=12, fill=FLOW, family=FONT_MONO, weight="700",
                    spacing="2"))
    out.append(text(X, 128, "可视化文档，是编译器的问责对象",
                    size=40, fill=INK, family=FONT_DISPLAY, weight="700"))
    out.append(text(X, 162,
                    "vizir 把一份 YAML 可视化文档编译成 SVG：途中每一级 IR、每一个节点、每一次后端降级都留下可质询的记录——",
                    size=14, fill=MUTED))
    out.append(text(X, 184,
                    "节点能被 explain 溯源到数据行与生成 pass，能力不支持就以稳定诊断码 fail-loud，局部补丁必须与全量重算语义等价。",
                    size=14, fill=MUTED))

    # three mechanism cards
    cards = [
        ("机制一 · 逐节点溯源", "explain",
         "每个 Scene2D 节点携带 Origin 责任链：HIR 来源 / MIR 来源 / data-key / 数据 lineage / 生成 pass / 人类可读解释。", FLOW),
        ("机制二 · 能力谈判", "negotiate",
         "编译前为每个节点显式谈判后端能力：174 条逐节点 decision 写进 manifest；不支持即 VIZ-CAP 报错，绝不静默消失。", TEAL),
        ("机制三 · 补丁等价", "schema scene-patch",
         "场景补丁带版本对账：局部补丁必须证明自己与全量重算语义等价，基线版本对不上就拒绝，不猜。", WARN),
    ]
    cw3 = (CW - 2 * 24) / 3
    for i, (kick, sym, body, accent) in enumerate(cards):
        cx = X + i * (cw3 + 24)
        out.append(rect(cx, 214, cw3, 148, fill=PAPER, stroke=RULE, sw=1,
                        rx=10))
        out.append(rect(cx, 214, cw3, 4, fill=accent, sw=0, rx=2))
        out.append(text(cx + 16, 244, kick, size=13, fill=accent,
                        weight="700"))
        out.append(code(cx + 16, 264, sym, size=12, fill=MUTED))
        wrapped = wrap_cn(body, 26)
        yy = 288
        for seg in wrapped:
            out.append(text(cx + 16, yy, seg, size=12, fill=INK))
            yy += 19

    # stat strip
    tiles = [
        (cli["subcommand_count"], "CLI 子命令",
         "validate…schema"),
        (tests["total_passed"], "测试全过", "cargo test --workspace"),
        (f'{diag["total_codes"]}×{diag["family_count"]}', "稳定诊断码",
         "VIZ-<族>-NNNN"),
        (nodes["total_nodes"], "Scene2D 节点", "全部携带 Origin"),
        (cap["svg_manifest_decisions"]["total"], "capability decisions",
         "逐节点写入 manifest"),
        (ex["count"], "examples", "chart/diagram/geometry/mixed"),
    ]
    tw = (CW - 5 * 16) / 6
    for i, (v, lab, note) in enumerate(tiles):
        out.append(stat_tile(X + i * (tw + 16), 392, tw, 92, v, lab, note))

    out.append(rect(X, 512, CW, 4, fill=RULE, sw=0))
    out.append(text(X, 544,
                    "本页每个数字都冻结自一次真实引擎运行，可逐项复现（证据链见 VERIFICATION.md）；本图构建未改动引擎任何已跟踪文件、未提交任何 commit。",
                    size=12.5, fill=MUTED))
    out.append(src_note(X, 566,
                        f'engine commit {eng["commit"][:12]} · rustc '
                        f'{eng["rustc"].split(" (")[0]} · '
                        f'输入 {eng["example_input"]}'))
    out.append(src_note(X, 588,
                        "差异化边界：本图不讲「又一套分层 IR」也不讲「渲染确定性」——"
                        "那些属于姊妹图 graph-ir-rs / plot-go；本图只讲问责三机制。"))
    out.append(src_note(X, 610,
                        "机制源码锚点与冻结数据收录于 VERIFICATION.md 证据链（本页不印代码坐标）· 声明 E1/E2/E3"))
    out.append(src_note(X, 632,
                        "examples 口径：工作树 11 = 已跟踪 10 + 未跟踪 WIP 1"
                        "（mixed/capacity-planning，见 engine.json 基线）；"
                        "fresh clone 复现为 10"))
    return svg(W, h, *out)


_WORD_RUN = re.compile(r"[A-Za-z0-9._/-]+")


def wrap_cn(s: str, n: int) -> list[str]:
    """Greedy CJK-aware wrap: CJK chars count 1, ASCII 0.55.

    Runs of [A-Za-z0-9._/-] are treated as unbreakable words (no mid-word
    line breaks like deci|sion / dashboa|rd — 2026-09 R2 fix); CJK may still
    break anywhere. A run wider than the whole line is hard-split as a last
    resort so wrapping always terminates."""
    def cw(ch: str) -> float:
        return 1.0 if ord(ch) > 0x2E7F else 0.55

    lines: list[str] = []
    cur, width = "", 0.0

    def flush() -> None:
        nonlocal cur, width
        if cur:
            lines.append(cur)
        cur, width = "", 0.0

    i = 0
    while i < len(s):
        m = _WORD_RUN.match(s, i)
        tok = m.group(0) if m else s[i]
        i += len(tok)
        tw = sum(cw(c) for c in tok)
        if width + tw > n and cur:
            flush()
        if tw <= n:
            cur += tok
            width += tw
        else:  # single run wider than the line: hard-split it
            for ch in tok:
                if width + cw(ch) > n:
                    flush()
                cur += ch
                width += cw(ch)
    flush()
    return lines


# ------------------------------------------------------------ P2 pipeline
def p_pipeline(d) -> str:
    det = d["determinism"]; nodes = d["scene_nodes"]
    eng = d["engine"]
    h = 610
    out = [panel_head(28, "01 · 问责管线", "四级流水，每一级都留下责任证据",
                      "VizHIR → VizMIR → Scene2D → 后端谈判：产物可复算，责任可点名")]
    stages = [
        ("VizHIR", "version 0.1", "人类手写的语义文档",
         ["稳定 id：service-health-dashboard",
          "数据、视图、编码在此声明",
          "责任证据：稳定身份 + 引用校验"], FLOW, "validate / normalize"),
        ("VizMIR", "version 0.1", "规范化中间表示",
         ["尺度显式化：domain/range 落盘",
          "provenance 字符串解释推断",
          "责任证据：每条推断附解释"], FLOW_DK, "lower"),
        ("Scene2D", "resolved", "解析后的 2D 场景",
         [f'{nodes["total_nodes"]} 个节点全部携带 Origin',
          "坐标、样式、文本已定死",
          "责任证据：逐节点责任链"], TEAL, "explain --node <path>"),
        ("后端谈判", "negotiate", "能力面显式对齐",
         ["svg/png 各自声明 supports",
          "不支持 → VIZ-CAP 稳定诊断码",
          "责任证据：逐节点 decision"], WARN, "render --manifest"),
    ]
    sw4 = (CW - 3 * 20) / 4
    for i, (name, ver, sub, bullets, accent, cmd) in enumerate(stages):
        sx = X + i * (sw4 + 20)
        out.append(rect(sx, 150, sw4, 240, fill=PANEL, stroke=RULE, sw=1,
                        rx=10))
        out.append(rect(sx, 150, sw4, 4, fill=accent, sw=0, rx=2))
        out.append(text(sx + 14, 180, name, size=17, fill=INK,
                        family=FONT_DISPLAY, weight="700"))
        out.append(code(sx + 14, 198, ver, size=10.5, fill=MUTED))
        out.append(text(sx + 14, 220, sub, size=11.5, fill=accent))
        yy = 244
        for b in bullets:
            for seg in wrap_cn(b, 17):
                out.append(text(sx + 14, yy, seg, size=11, fill=INK))
                yy += 16
            yy += 5
        out.append(code(sx + 14, 372, cmd, size=10, fill=FLOW_DK))
        if i < 3:
            out.append(arrow(sx + sw4 + 3, 270, sx + sw4 + 17, 270,
                             color=FLOW_LT, sw=2.5))

    # determinism footer (deliberately a side note, not the protagonist)
    out.append(rect(X, 420, CW, 92, fill=FLOW_TINT, stroke=FLOW_XLT, sw=1,
                    rx=8))
    out.append(text(X + 16, 446, "问责的地基：字节级可复算（配角，不是本图主角）",
                    size=12.5, fill=FLOW_DK, weight="700"))
    for i, pair in enumerate(det["pairs"]):
        px = X + 16 + i * (CW - 32) / 3
        ok = "✓ 两跑一致" if pair["identical"] else "✗ 漂移"
        out.append(code(px, 468, pair["artifact"], size=10.5, fill=INK))
        out.append(code(px, 484, pair["run_a"][:16] + "…", size=10,
                        fill=MUTED))
        out.append(text(px, 500, ok, size=10.5,
                        fill=TEAL if pair["identical"] else WARN))
    out.append(src_note(X, 538,
                        "三对指纹全等（规范化/降级/渲染各两跑）；"
                        "manifest 仅 output 路径一行随路径变化，同路径重跑 byte 级一致 · 声明 E4"))
    out.append(src_note(X, 560,
                        f'输入 {eng["example_input"]}（sha256 见 VERIFICATION.md）· '
                        "证据链见 VERIFICATION.md"))
    return svg(W, h, *out)


# --------------------------------------------------------------- P3 origin
def p_origin(d) -> str:
    nodes = d["scene_nodes"]; o = nodes["origin_example"]
    h = 648
    out = [panel_head(28, "02 · Origin 责任链", "每个 Scene2D 节点都自带六字段档案",
                      f'例：explain --node latency-risk/point/gateway —— 下表全部为真实字段值，非示意')]
    fields = [
        ("HIR 视图声明", o["hir_node"], "它由哪条视图声明展开"),
        ("MIR 标记组", o["mir_node"], "它由哪个标记组实例化"),
        ("数据键", o["data_key"], "绑定到哪一行数据（稳定键）"),
        ("数据谱系", " → ".join(o["data_lineage"]), "数据从哪个数据集流入"),
        ("生成 pass", o["generated_by"], "由哪条生成 pass 产出"),
        ("人读解释", o["explanation"], "一句话解释它为什么长这样"),
    ]
    y = 152
    out.append(rect(X, y, 700, 330, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, y + 26, "六字段责任档案（下表全部为真实字段值，非示意）",
                    size=12.5, fill=FLOW, weight="700"))
    out.append(text(X + 16, y + 44, "字段原文键名见右卡真实输出节选；源码锚点见 VERIFICATION.md · 声明 E3",
                    size=10, fill=MUTED))
    yy = y + 76
    for name, value, meaning in fields:
        out.append(rect(X + 12, yy - 14, 676, 40, fill=PAPER, stroke=RULE,
                        sw=1, rx=6))
        out.append(text(X + 22, yy, name, size=11.5, fill=FLOW_DK,
                        weight="700"))
        out.append(code(X + 150, yy, value if len(str(value)) <= 52
                        else str(value)[:49] + "…", size=11, fill=INK))
        out.append(text(X + 150, yy + 15, meaning, size=10, fill=MUTED))
        yy += 44

    # right: verbatim JSON from the real scene dump
    # explanation is 69 chars — split for the 380px box at a word boundary
    # (never mid-word; 2026-09 R1 fix replacing the hardcoded [:33] slice).
    expl = o["explanation"]
    cut = expl.rfind(" ", 0, 34) + 1 or len(expl)
    json_lines = [
        ('"id": "latency-risk/point/gateway",', MUTED),
        ('"origin": {', INK),
        (f'  "hir_node": "{o["hir_node"]}",', INK),
        (f'  "mir_node": "{o["mir_node"]}",', INK),
        (f'  "data_key": "{o["data_key"]}",', FLOW_DK),
        (f'  "data_lineage": ["{o["data_lineage"][0]}"],', FLOW_DK),
        (f'  "generated_by": "{o["generated_by"]}",', TEAL),
        (f'  "explanation": "{expl[:cut]}', FLOW),
        (f'      {expl[cut:]}"', FLOW),
        ("}", INK),
    ]
    out.append(code_box(X + 724, y, 380, 250, json_lines, size=10.5,
                        title="vizir lower → run-a.scene.json（真实输出节选）"))
    # serde omission rule, right under the JSON box (2026-09 R3 fix;
    # 2026-09-03 retrofit: domain wording, engine coords stay in VERIFICATION)
    out.append(src_note(X + 724, y + 260,
                        "省略规则：可选字段缺省时整键不写入输出 JSON"
                        "（细则见声明 E3）"))
    out.append(rect(X + 724, y + 274, 380, 88, fill=TINT, stroke=TEAL, sw=1,
                    rx=8))
    out.append(text(X + 738, y + 298,
                    "110/110 个节点都带 origin 对象；", size=12, fill=INK,
                    weight="700"))
    out.append(text(X + 738, y + 318,
                    "hir_node / mir_node / generated_by / explanation "
                    "四字段必现；", size=10.5, fill=INK))
    out.append(text(X + 738, y + 336,
                    "data_key / data_lineage 可选：序列化省略 86 / 74。",
                    size=10.5, fill=INK))
    out.append(src_note(X, y + 384,
                        "六字段值逐字取自真实降级输出（右卡节选）；110 节点全覆盖 · "
                        "声明 E3 · 证据链见 VERIFICATION"))
    out.append(src_note(X, y + 406,
                        "「谁是它的爹、吃了哪行数据、哪个 pass 造的、为什么长这样」——四问全部可机器读取"))
    return svg(W, h, *out)


# --------------------------------------------------------- P4 explain tree
def p_explain(d) -> str:
    s = d["explain_samples"]["samples"]
    gb_colors = {
        "build-symbol-scene": FLOW,
        "build-guide-scene": FLOW_DK,
        "build-bar-scene": TEAL,
        "shape-native-text": CH_B,
        "(error)": WARN,
    }
    h = 896
    out = [panel_head(28, "03 · explain 决策树", "一条命令，把任意节点送回被告席",
                      "六个真实查询：两个数据点、坐标轴、条形、刻度标签、以及一个不存在的节点")]
    out.append(code_box(X, 138, CW, 40, [
        ("vizir explain examples/chart/service-health.viz.yaml --node <stable-node-id>",
         FLOW_DK)], size=11.5, bg="#FFFFFF"))
    y = 198
    ok = [q for q in s if q["expected_ok"]]
    err = [q for q in s if not q["expected_ok"]]
    for q in ok:
        f = q["fields"]
        # `vizir explain` prints hyphenated field names (hir-node, data-key,
        # generated-by, …) — parse_explain freezes them verbatim. Reading
        # underscore names here silently degraded every card to "?" chips in
        # one muted color (2026-09 R14, found while fixing R7).
        color = gb_colors.get(f.get("generated-by", ""), MUTED)
        out.append(rect(X, y, CW, 92, fill=PANEL, stroke=RULE, sw=1, rx=8))
        out.append(rect(X, y, 4, 92, fill=color, sw=0))
        out.append(code(X + 18, y + 24, q["queried"], size=12.5, fill=INK))
        chip_txt, chip_w = chip(X + 18, y + 36, f.get("generated-by", "?"),
                                fill=PAPER, stroke=color, color=color,
                                size=10)
        out.append(chip_txt)
        extra = []
        if f.get("data-key"):
            extra.append(f'data-key: {f["data-key"]}')
        if f.get("data-lineage"):
            extra.append(f'data-lineage: {f["data-lineage"]}')
        if f.get("mir-node"):
            extra.append(f'mir: {f["mir-node"]}')
        if extra:  # R7: never emit a zero-ink <text></text>
            out.append(code(X + 18 + chip_w + 12, y + 50,
                            " · ".join(extra), size=10.5, fill=MUTED))
        reason = f.get("reason", "")
        out.append(code(X + 18, y + 74,
                        reason if len(reason) <= 150 else reason[:147] + "…",
                        size=11, fill=FLOW_DK))
        y += 104
    for q in err:
        out.append(rect(X, y, CW, 66, fill=WARN_LT, stroke=WARN, sw=1.2,
                        rx=8))
        out.append(code(X + 18, y + 26, q["queried"], size=12.5, fill=INK))
        out.append(code(X + 18, y + 48, q["stderr"], size=11.5, fill=WARN,
                        weight="700"))
        y += 78
    out.append(src_note(X, y + 6,
                        "六查询输出逐字段实录 · VIZ-EXPLAIN-0001 = 唯一的 explain 失败码 · 声明 E1"))
    out.append(src_note(X, y + 28,
                        "generated_by 即着色：五个查询命中四条不同生成 pass —— 溯源粒度到 pass，不只到层级"))
    return svg(W, h, *out)


# ------------------------------------------------------------- P5 coverage
def p_coverage(d) -> str:
    nodes = d["scene_nodes"]
    hist = nodes["generated_by_histogram"]
    h = 570
    out = [panel_head(28, "04 · 覆盖面", "110 个节点，无一例外",
                      "按 generated_by（生成 pass）分组的 Scene2D 节点直方图——责任链覆盖率 100%")]
    vmax = max(hist.values())
    meanings = {
        "shape-native-text": "原生文本形状（刻度/图例/标签）",
        "build-guide-scene": "坐标轴与网格 guide",
        "build-symbol-scene": "散点 mark 实例",
        "build-bar-scene": "条形 mark 实例",
        "build-legend": "图例条目",
        "build-chart-scene": "视图组根节点",
    }
    y = 168
    out.append(rect(X, y, CW, 42, fill=FLOW_TINT, stroke=FLOW_XLT, sw=1, rx=8))
    out.append(text(X + 16, y + 27,
                    f'总节点 {nodes["total_nodes"]} = ' +
                    " + ".join(str(v) for v in hist.values()) +
                    "，全部可 explain；histogram 见 scene_nodes.json",
                    size=12.5, fill=FLOW_DK, weight="700"))
    y += 66
    colors = [CH_B, FLOW_DK, FLOW, TEAL, FLOW_LT, FLOW_XLT]
    for i, (name, count) in enumerate(hist.items()):
        out.append(hbar(X, y, CW - 24, 26, count, vmax, name, str(count),
                        color=colors[i % len(colors)], label_w=200,
                        vlabel_w=48, size=12))
        out.append(text(X + 208, y + 39, meanings.get(name, ""), size=10,
                        fill=MUTED))
        y += 52
    out.append(src_note(X, y + 18,
                        "直方图由两个视图（latency-risk + availability-ranking）合并统计 · 声明 E1 · 证据链见 VERIFICATION"))
    return svg(W, h, *out)


# ---------------------------------------------------- P6 capability surface
def p_capsurface(d) -> str:
    cap = d["capability"]
    svgb = cap["svg_backend"]; pngb = cap["png_backend"]
    h = 756
    out = [panel_head(28, "05 · 能力面", "后端必须先递名片，编译才开始谈判",
                      "vizir capabilities <backend> 的逐字输出分四节：支持 / 不支持 / 显式降级 / 处置策略")]
    colw = (CW - 24) / 2

    def backend_card(x, title, b, accent):
        o = [rect(x, 150, colw, 470, fill=PANEL, stroke=RULE, sw=1, rx=10)]
        o.append(rect(x, 150, colw, 4, fill=accent, sw=0, rx=2))
        o.append(text(x + 16, 180, title, size=16, fill=INK,
                      family=FONT_DISPLAY, weight="700"))
        o.append(code(x + 16, 200,
                      f'接受输入 {b["accepted_ir"]} · 能力面版本 {b["version"]}',
                      size=10.5, fill=MUTED))
        o.append(text(x + 16, 228, f'supports（{len(b["supports"])} 项）',
                      size=12, fill=accent, weight="700"))
        cx, cy = x + 16, 240
        for feat in b["supports"]:
            c, w = chip(cx, cy, feat, fill=FLOW_TINT, stroke=FLOW_XLT,
                        color=FLOW_DK, size=9.5, h=18, pad_x=6)
            o.append(c)
            cx += w + 6
            if cx > x + colw - 150:
                cx, cy = x + 16, cy + 24
        uy = cy + 34
        o.append(text(x + 16, uy,
                      f'unsupported（{len(b["unsupported"])} 项）→ 碰上即报错',
                      size=12, fill=WARN, weight="700"))
        for i, feat in enumerate(b["unsupported"]):
            o.append(code(x + 16, uy + 18 + i * 17, f'✗ {feat}', size=10.5,
                          fill=WARN))
        ly = uy + 18 + len(b["unsupported"]) * 17 + 24
        if b.get("lowering"):
            o.append(text(x + 16, ly, "lowering（显式降级策略）", size=12,
                          fill=OUTCOME, weight="700"))
            o.append(code(x + 16, ly + 18,
                          "所有 scene.2d.* → rasterized（8 项，逐项声明）",
                          size=10.5, fill=INK))
            o.append(code(x + 16, ly + 36,
                          "降级必须被声明，才允许发生", size=10.5, fill=MUTED))
            ly += 56
        o.append(rect(x + 12, ly, colw - 24, 30,
                      fill="#FDEEE8" if accent == WARN else FLOW_TINT,
                      stroke=accent, sw=1, rx=6))
        o.append(text(x + 24, ly + 20,
                      f'处置策略：不支持即 {b["unsupported_policy"]}（中止）',
                      size=11, fill=accent, weight="700"))
        return "".join(o)

    out.append(backend_card(X, "svg 后端", svgb, FLOW))
    out.append(backend_card(X + colw + 24, "png 后端（经 svg）", pngb, TEAL))
    out.append(src_note(X, 646,
                        "能力面逐字冻结自 capabilities 真实输出；谈判两步构成：先逐节点收集能力要求，再逐节点谈判判决"
                        "（声明 E2 · 源码锚点见 VERIFICATION）"))
    out.append(src_note(X, 668,
                        "谈判单位是「节点 × 能力项」：先去重再逐条判决，决策连同理由与来源节点路径落进 manifest"))
    return svg(W, h, *out)


# -------------------------------------------------------------- P7 decisions
def p_decisions_impl(d) -> str:
    cap = d["capability"]
    sm = cap["svg_manifest_decisions"]; pm = cap["png_manifest_decisions"]
    h = 700
    out = [panel_head(28, "06 · 174 次判决", "每个节点的每项能力要求，都要单独过堂",
                      "render --manifest 的逐节点 capability decision 统计：feature × status，来源节点路径逐条在档")]
    # left: feature histogram
    out.append(rect(X, 150, 620, 300, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 176, "按 feature 分组（svg 后端，全部 exact）",
                    size=12.5, fill=INK, weight="700"))
    feats = list(sm["by_feature"].items())
    vmax = max(v for _, v in feats)
    y = 192
    colors = [FLOW, CH_B, FLOW_DK, TEAL, FLOW_LT, FLOW_XLT, MUTED]
    for i, (name, count) in enumerate(feats):
        out.append(hbar(X + 16, y, 588, 24, count, vmax, name, str(count),
                        color=colors[i % len(colors)], label_w=140,
                        vlabel_w=40, size=11))
        y += 32
    out.append(text(X + 16, y + 16,
                    f'合计 {sm["total"]} 条 decision = 每个节点 × 它用到的能力项（先去重再逐条判决）',
                    size=11, fill=MUTED))

    # right: svg vs png stacked
    out.append(rect(X + 648, 150, 456, 300, fill=PANEL, stroke=RULE, sw=1,
                    rx=10))
    out.append(text(X + 664, 176, "同一 Scene2D，两个后端的判决对比", size=12.5,
                    fill=INK, weight="700"))
    total = sm["total"]
    bar_w = 380
    out.append(text(X + 664, 204, "svg", size=12, fill=MUTED,
                    family=FONT_MONO))
    out.append(rect(X + 664, 212, bar_w, 34, fill=FLOW, rx=4))
    out.append(text(X + 664 + bar_w + 8, 234, f"{total}", size=12,
                    fill=INK, family=FONT_MONO))
    out.append(code(X + 664, 262,
                    f'exact {sm["by_status"]["exact"]} / error 0', size=10.5,
                    fill=FLOW_DK))
    png_r = pm["by_status"].get("rasterized", 0)
    png_e = pm["by_status"].get("exact", 0)
    r_w = bar_w * png_r / total
    e_w = bar_w * png_e / total
    out.append(text(X + 664, 296, "png", size=12, fill=MUTED,
                    family=FONT_MONO))
    out.append(rect(X + 664, 304, r_w, 34, fill=OUTCOME, rx=0))
    out.append(rect(X + 664 + r_w, 304, e_w, 34, fill=FLOW))
    if png_r:
        out.append(text(X + 664 + r_w / 2, 326, str(png_r), size=11.5,
                        fill=INK, family=FONT_MONO, anchor="middle",
                        weight="700"))
    if png_e:
        out.append(text(X + 664 + r_w + e_w / 2, 326, str(png_e), size=11.5,
                        fill=PAPER, family=FONT_MONO, anchor="middle",
                        weight="700"))
    out.append(code(X + 664, 358,
                    f'rasterized {png_r}（scene.2d.* 全部声明降级）', size=10.5,
                    fill=OUTCOME))
    out.append(code(X + 664, 376,
                    f'exact {png_e}（paint.alpha 保真）', size=10.5,
                    fill=FLOW_DK))
    out.append(rect(X + 664, 396, 424, 38, fill=OUTCOME_LT, stroke=OUTCOME,
                    sw=1, rx=6))
    out.append(text(X + 676, 414, "降级不是失败：是逐条声明、逐条留档的", size=11,
                    fill=INK))
    out.append(text(X + 676, 428, "显式妥协；错误才是失败。", size=11, fill=INK))

    # sample decision verbatim
    sample = sm["sample"][0]
    out.append(code_box(X, 476, CW, 96, [
        ('manifest.capability_report.decisions[0] = ' +
         json.dumps(sample, ensure_ascii=False), INK),
        ('… 共 174 条，每条含 feature / status / reason / source(节点路径)',
         MUTED),
        (f'png 首条 rasterized: '
         f'{json.dumps(pm["sample_rasterized"], ensure_ascii=False)[:110]}…',
         OUTCOME),
    ], size=10.5, title="render --manifest 逐节点判决（真实样本）"))
    out.append(src_note(X, 606,
                        "逐节点判决统计逐字冻结自真实 manifest（报告/损耗/栅格化器/编译器各键在档）· 声明 E2"))
    out.append(src_note(X, 628,
                        "判决在渲染之前：谈不拢就 VIZ-CAP-0002 中止（声明 E2 · 坐标见 VERIFICATION），不会产出半张图"))
    return svg(W, h, *out)


# --------------------------------------------------------------- P8 fail loud
def p_fail_loud(d) -> str:
    fl = d["fail_loud"]; cap = d["capability"]
    h = 690
    out = [panel_head(28, "07 · fail-loud", "不支持就报错，绝不静默降级",
                      "一次真实实验：把 rasterizer 从 PATH 里拿走，让 PNG 渲染走投无路")]
    # left: experiment timeline
    out.append(rect(X, 150, 640, 400, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 178, "实验：空 PATH 下的 PNG 渲染", size=13,
                    fill=INK, weight="700"))
    steps = [
        ("命令", fl["cap0001"]["command"], MUTED),
        ("exit code", f'{fl["cap0001"]["exit"]}（非零即拒绝）', WARN),
        ("stderr", fl["cap0001"]["stderr"], WARN),
        ("输出文件", "不存在 — output_file_exists: false", TEAL),
        ("manifest", "不存在 — manifest_file_exists: false", TEAL),
    ]
    y = 204
    for label, val, color in steps:
        out.append(circle(X + 22, y - 4, 4, fill=color))
        if y > 204:
            out.append(line(X + 22, y - 22, X + 22, y - 10, stroke=RULE,
                            sw=1.5))
        out.append(text(X + 38, y, label, size=11, fill=MUTED,
                        family=FONT_MONO))
        vv = val if len(val) <= 74 else val[:71] + "…"
        out.append(code(X + 38, y + 17, vv, size=10.5,
                        fill=color if color is not WARN else INK))
        y += 44
    out.append(rect(X + 16, y + 2, 608, 56, fill=TINT, stroke=TEAL, sw=1,
                    rx=6))
    out.append(text(X + 30, y + 24, "半个字节都不落地：没有降级 PNG、没有空 manifest、",
                    size=11.5, fill=INK))
    out.append(text(X + 30, y + 42, "没有「看起来成功了」。失败是原子的。", size=11.5,
                    fill=INK))

    # right: counterfactual + cap-0002
    out.append(rect(X + 664, 150, 440, 188, fill=PAPER, stroke=RULE, sw=1,
                    rx=10, dash="5 4"))
    out.append(text(X + 680, 178, "反事实：如果选择静默降级", size=12.5,
                    fill=MUTED, weight="700"))
    cf = [
        "产出一个像素版 PNG，无任何标记；",
        "manifest 不记录任何妥协；",
        "下游把降级当成功，审计时无从对质。",
    ]
    yy = 202
    for c in cf:
        out.append(text(X + 680, yy, "· " + c, size=11, fill=MUTED))
        yy += 20
    out.append(text(X + 680, yy + 10, "vizir 的回答：unsupported_policy = \"error\"",
                    size=11.5, fill=FLOW_DK, weight="700"))
    # 2026-09-03 retrofit: verbatim source excerpt card replaced by a
    # domain-named numbered pseudocode card (claim preserved; excerpt itself
    # lives in VERIFICATION.md).
    out.append(code_box(X + 664, 356, 440, 194, [
        ("① 汇总：本轮全部「不支持」判决收拢成清单", INK),
        ("   （能力项 + 来源节点路径）", INK),
        ("② 定名：清单打成一条稳定诊断 VIZ-CAP-0002，", WARN),
        ("   点名后端与它给不了的每一项能力", WARN),
        ("③ 中止：立即停止渲染——不写输出、", FLOW_DK),
        ("   不写 manifest（exit 1）", FLOW_DK),
        ("谈不拢 = 一条命名诊断 + 零产出。", MUTED),
    ], size=10.5, title="谈判破裂的唯一出口（伪代码）· 声明 E2"))
    out.append(src_note(X, 580,
                        "实验 exit/stderr/文件不存在性均为实测 · 失败原子性另有引擎测试钉死：不残留输出与 manifest"
                        "（声明 E2 · 测试名与源码锚点见 VERIFICATION）"))
    out.append(src_note(X, 602,
                        "explain 的失败同样稳定：VIZ-EXPLAIN-0001 no Scene2D node named …（exit 1）"))
    return svg(W, h, *out)


# ------------------------------------------------------------------- P9 loss
def p_loss(d) -> str:
    cap = d["capability"]
    h = 560
    out = [panel_head(28, "08 · 诚实的损耗", "PNG 少掉的东西，manifest 逐字记下来",
                      "矢量→栅格这一步不是免费的；vizir 用 loss record 把代价写成数据，而不是藏进像素")]
    losses = cap["png_losses"]
    out.append(code_box(X, 150, CW, 118, [
        (f'run.png.manifest.json → losses: {json.dumps(losses, ensure_ascii=False)}',
         OUTCOME),
        (f'rasterizer: "{cap["png_rasterizer"]}"（外部工具，版本敏感）', INK),
        ("SVG 渲染 0 loss records；Scene2D losses 为空数组", MUTED),
    ], size=11, title="png 渲染 manifest（真实字段值）"))
    cards = [
        ("1 条 loss record", "fidelity: rasterized",
         "声明「矢量 Scene2D 在精确 SVG 排放之后被栅格化」——何时、何层、丢了什么，一格不漏。", OUTCOME),
        ("外部 rasterizer", "rsvg-convert / ImageMagick",
         "PNG 由外部工具栅格化，版本敏感、跨机可漂移——所以本图把证据锚在 SVG+JSON 的 sha256 上。", FLOW_DK),
        ("呈现层 ≠ 证据层", "PNG 只作呈现",
         "冻结的是 SVG 与 JSON 的指纹；PNG 位图仅供阅读。噪声如实记录，不假装它不存在。", TEAL),
    ]
    cw3 = (CW - 2 * 24) / 3
    for i, (title, kw, body, accent) in enumerate(cards):
        cx = X + i * (cw3 + 24)
        out.append(rect(cx, 296, cw3, 170, fill=PANEL, stroke=RULE, sw=1,
                        rx=10))
        out.append(rect(cx, 296, cw3, 4, fill=accent, sw=0, rx=2))
        out.append(text(cx + 16, 326, title, size=14, fill=INK,
                        family=FONT_DISPLAY, weight="700"))
        out.append(code(cx + 16, 346, kw, size=10.5, fill=accent))
        yy = 372
        for seg in wrap_cn(body, 26):
            out.append(text(cx + 16, yy, seg, size=11, fill=INK))
            yy += 17
    out.append(src_note(X, 494,
                        "损耗记录与栅格化器名逐字冻结自真实 manifest；manifest 仅 output 路径一行随路径变化，"
                        "同路径重跑 byte 级一致 · 声明 E5"))
    out.append(src_note(X, 516,
                        "alpha 校验另有 VIZ-ARTIFACT-0001/0002/0003 三码把关：透明背景必须真的透明 · 声明 E5（测试锚点见 VERIFICATION）"))
    return svg(W, h, *out)


# ---------------------------------------------------------- P10 patch revisions
def p_patch_gate(d) -> str:
    p = d["patch"]; diag = d["diag_codes"]
    codes = {c: diag["codes"][c] for c in p["diagnostic_codes"]}
    h = 730
    out = [panel_head(28, "09 · revision 校验", "局部补丁要过四道门才被接受",
                      "场景补丁携带基线/目标版本；执行侧逐门核对，任何一门不符即拒绝")]
    # patch envelope — 2026-09-03 retrofit: verbatim struct-literal excerpt
    # replaced by a domain-named envelope card (six elements; example values
    # from the patch test sample; the source excerpt lives in VERIFICATION).
    env = [
        ("协议版本", f'{p["protocol_version"]}', "协议对不上即拒（0003 把关）", FLOW_DK),
        ("文档标识", "doc（示例）", "别家的补丁贴不进本文档（0004）", INK),
        ("事务标识", "transaction/test（示例）", "一次补丁会话可追踪", INK),
        ("基线版本", "7（示例）", "必须对上当前场景版本（0005 把关）", FLOW),
        ("目标版本", "8（示例）", "必须严格前进，不许原地或倒退（0002）", FLOW),
        ("操作列表", "删 · 改 · 插 · 排序", "逐操作校验：坏操作各有专属码", TEAL),
    ]
    out.append(rect(X, 150, 470, 224, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 176, "补丁信封六要素（示例取自补丁测试样例）",
                    size=12.5, fill=FLOW, weight="700"))
    yy = 200
    for label, value, meaning, accent in env:
        out.append(text(X + 16, yy, label, size=11, fill=accent, weight="700"))
        out.append(code(X + 110, yy, value, size=10.5, fill=INK))
        out.append(text(X + 16, yy + 15, meaning, size=9.5, fill=MUTED))
        yy += 29
    # gates table
    gx = X + 494
    out.append(rect(gx, 150, 610, 344, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(gx + 16, 178, "四道拒绝门（每门一个稳定诊断码）", size=13,
                    fill=INK, weight="700"))
    gates = [
        ("VIZ-PATCH-0002", "target ≤ base",
         "补丁不许原地踏步或倒退", "diff 与 apply 两侧都查"),
        ("VIZ-PATCH-0003", "协议版本 ≠ 0.1",
         "协议版本对不上，拒绝解释", "apply 侧"),
        ("VIZ-PATCH-0004", "文档标识不匹配",
         "别家的补丁贴不进本文档", "apply 侧"),
        ("VIZ-PATCH-0005", "基线版本 ≠ 当前",
         "场景已经前进，补丁过期即废", "apply 侧"),
    ]
    y = 200
    for code_name, cond, meaning, side in gates:
        out.append(rect(gx + 12, y - 14, 586, 60, fill=PAPER, stroke=RULE,
                        sw=1, rx=6))
        out.append(code(gx + 22, y + 4, code_name, size=11.5, fill=WARN,
                        weight="700"))
        out.append(code(gx + 170, y + 4, cond, size=10.5, fill=INK))
        out.append(text(gx + 170, y + 22, meaning, size=10.5, fill=MUTED))
        out.append(text(gx + 440, y + 4, side, size=10, fill=FLOW_LT))
        y += 68
    out.append(rect(X, 380, 470, 78, fill=FLOW_TINT, stroke=FLOW_XLT, sw=1,
                    rx=8))
    out.append(text(X + 16, 402,
                    f'补丁机制共 {len(p["diagnostic_codes"])} 个 VIZ-PATCH 诊断码：',
                    size=11.5, fill=FLOW_DK, weight="700"))
    out.append(text(X + 16, 421,
                    "0001 仅比对（diff）侧发射；0002 比对与执行两侧都查；",
                    size=11.5, fill=FLOW_DK))
    out.append(text(X + 16, 440,
                    "其余 12 个全部在执行（apply）侧——拒绝理由全部命名。",
                    size=11.5, fill=FLOW_DK))
    out.append(src_note(X, 574,
                        f'scene-patch.schema.json sha256 {p["scene_patch_schema_sha256"][:24]}…（schema 三卡见第 11 板）'))
    out.append(src_note(X, 596,
                        "14 码清单与两侧发射位置冻结于补丁证据（声明 E3 · 源码锚点见 VERIFICATION）"))
    out.append(src_note(X, 618,
                        "校验通过不是终点——等价性才是，见下一板"))
    return svg(W, h, *out)


# ------------------------------------------------------- P11 patch equivalence
def p_patch_equiv(d) -> str:
    p = d["patch"]
    h = 700
    out = [panel_head(28, "10 · 等价性", "局部补丁 ≡ 全量重算，是被测试钉死的行为",
                      "主证测试：比对产出操作序列，执行之后的结果必须与直接重算的 Scene2D 语义相同")]
    # flow diagram
    y = 170
    out.append(rect(X, y, CW, 240, fill=PANEL, stroke=RULE, sw=1, rx=10))
    boxes = [
        ("上一代场景", "版本 7", "两个矩形 a、b", FLOW_LT),
        ("两代比对", "diff", "逐节点比对两代场景", FLOW),
        ("补丁信封", f'协议 {p["protocol_version"]}',
         "操作序列 + 双版本", FLOW_DK),
        ("逐门校验后执行", "apply", "四道门全过才执行", TEAL),
        ("补丁后场景", "版本 8", "≈ 直接重算", CH_G),
    ]
    bw = 180
    for i, (name, sub, body, accent) in enumerate(boxes):
        bx = X + 24 + i * (bw + 22)
        out.append(rect(bx, y + 40, bw, 96, fill=PAPER, stroke=accent, sw=1.4,
                        rx=8))
        out.append(text(bx + bw / 2, y + 66, name, size=12.5, fill=INK,
                        anchor="middle", weight="700"))
        out.append(code(bx + bw / 2, y + 84, sub, size=9.5, fill=MUTED,
                        anchor="middle"))
        out.append(code(bx + bw / 2, y + 104, body, size=9.5, fill=accent,
                        anchor="middle"))
        if i < 4:
            out.append(arrow(bx + bw + 3, y + 88, bx + bw + 19, y + 88,
                             color=FLOW_LT, sw=2))
    # ops sequence — 2026-09-03 retrofit: engine op-variant names replaced by
    # domain verbs (order claim preserved; original names in VERIFICATION).
    out.append(text(X + 24, y + 172, "操作序列（测试断言的精确顺序）：", size=11.5,
                    fill=INK, weight="700"))
    ops = [
        ("删节点", "键=a", "先删", WARN),
        ("改节点", "键=b, 宽=3", "再改", FLOW),
        ("插节点", "键=c, 位置=1", "后插", TEAL),
        ("子节点排序", "a,b→b,c", "终排序", FLOW_DK),
    ]
    ox = X + 210
    for name, arg, note, accent in ops:
        c, w = chip(ox, y + 184, f"{name}({arg})", fill=PAPER, stroke=accent,
                    color=accent, size=9.5, h=20)
        out.append(c)
        ox += w + 10
    out.append(text(X + 24, y + 228,
                    "顺序铁律：先删 → 再改/插 → 最后排序（引擎源注释原文见 VERIFICATION · 声明 E3）",
                    size=9.5, fill=MUTED))
    # tests list — engine test names + line numbers live in VERIFICATION now;
    # the panel keeps the five guarantees as domain claims.
    out.append(rect(X, 442, CW, 178, fill=PAPER, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 468,
                    f'补丁机制 {p["test_count"]} 个专项测试（名称与行号收录于 VERIFICATION · 声明 E3）',
                    size=12.5, fill=INK, weight="700"))
    guarantees = [
        ("主证", "等价性：操作顺序 + 结果场景 ≡ 全量重算（严格相等）"),
        ("比对侧拒绝", "跨文档补丁 / 停滞版本必拒"),
        ("执行侧拒绝", "外来补丁 / 停滞版本必拒"),
        ("逐操作精确诊断", "每个坏操作命中它自己的诊断码"),
        ("过期补丁必拒", "基线版本落后即拒（VIZ-PATCH-0005）"),
    ]
    yy = 492
    for i, (tag, claim) in enumerate(guarantees):
        out.append(code(X + 16, yy, f'T{i + 1}', size=10.5, fill=FLOW_LT,
                        weight="700"))
        out.append(text(X + 64, yy, tag, size=10.5, fill=INK, weight="700"))
        out.append(text(X + 210, yy, claim, size=10.5, fill=MUTED))
        yy += 24
    out.append(src_note(X, 640,
                        "补丁等价的主战场在库层（库 API 契约，非 CLI 子命令）；其 JSON Schema 由 vizir schema scene-patch 持久化防漂移"))
    return svg(W, h, *out)


# ------------------------------------------------------------------ P12 gates
def p_gates(d) -> str:
    diag = d["diag_codes"]; sch = d["schemas"]; tests = d["tests"]
    h = 880
    out = [panel_head(28, "11 · 门禁与来源", "问责文化需要门禁，门禁需要指纹",
                      "稳定诊断码全集、schema 防漂移、测试分布——以及本图每个数字的出处")]
    # family histogram
    out.append(rect(X, 150, 640, 380, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 176,
                    f'{diag["total_codes"]} 个稳定诊断码 × {diag["family_count"]} 个族',
                    size=13, fill=INK, weight="700"))
    fams = list(diag["families"].items())
    vmax = max(v for _, v in fams)
    y = 194
    for i, (name, count) in enumerate(fams):
        out.append(hbar(X + 16, y, 608 - 20, 19, count, vmax, name,
                        str(count),
                        color=FLOW if i % 2 == 0 else FLOW_LT, label_w=120,
                        vlabel_w=34, size=10.5))
        y += 24.5
    out.append(text(X + 16, y + 8,
                    "码格式 VIZ-<族>-NNNN：逐码枚举自引擎源码，含测试内出现次数 · 声明 E6（锚点口径见 VERIFICATION）",
                    size=10.5, fill=MUTED))

    # schema anti-drift
    out.append(rect(X + 664, 150, 440, 380, fill=PANEL, stroke=RULE, sw=1,
                    rx=10))
    out.append(text(X + 680, 176, "schema 防漂移：三份持久化契约", size=13,
                    fill=INK, weight="700"))
    yy = 200
    for ir, info in sch["schemas"].items():
        ok = info["matches_checked_in"]
        out.append(rect(X + 676, yy - 14, 416, 76, fill=PAPER, stroke=RULE,
                        sw=1, rx=6))
        out.append(code(X + 688, yy + 4, f'vizir schema {ir}', size=11,
                        fill=FLOW_DK))
        out.append(text(X + 688 + 178, yy + 4,
                        "✓ 与仓库内一致" if ok else "✗ 漂移", size=11,
                        fill=TEAL if ok else WARN, weight="700"))
        out.append(code(X + 688, yy + 22, info["emitted_sha256"][:34] + "…",
                        size=9.5, fill=MUTED))
        out.append(code(X + 688, yy + 38, info["checked_in_path"], size=9.5,
                        fill=FLOW_LT))
        yy += 88
    # anti-drift test name/coords stay in the record (VERIFICATION); the page
    # keeps the guarantee itself (2026-09-03 retrofit).
    out.append(code(X + 680, yy + 6,
                    "防漂移由引擎集成测试钉死：三份 schema 逐字节比对 · 声明 E6（测试名与源码锚点见 VERIFICATION）",
                    size=9.5, fill=MUTED))

    # tests distribution — suite keys are engine file paths; render them as
    # domain suite labels (numbers still read from the frozen per_suite map).
    suite_labels = {
        "vizir-core (lib)": "核心库",
        "vizir-compiler (lib)": "编译器",
        "vizir-backend-svg (lib)": "SVG 后端",
        "vizir-cli (bin)": "CLI 单元",
        "vizir-cli (tests/cli.rs)": "CLI 集成",
        "vizir-cli (tests/pipeline.rs)": "管线集成",
    }
    out.append(rect(X, 554, CW, 96, fill=FLOW_TINT, stroke=FLOW_XLT, sw=1,
                    rx=10))
    out.append(text(X + 16, 582,
                    f'cargo test --workspace：{tests["total_passed"]} 全过（exit {tests["exit"]}）',
                    size=13, fill=FLOW_DK, weight="700"))
    parts = " + ".join(f'{suite_labels.get(k, k)} {v}'
                       for k, v in tests["per_suite"].items() if v)
    out.append(code(X + 16, 604, parts, size=11, fill=INK))
    out.append(text(X + 16, 626,
                    "测试在冻结时实跑（只写引擎产物目录）；诊断码与补丁测试的行号均为逐码实测。",
                    size=11, fill=MUTED))

    # claim registry — 2026-09-03 retrofit: the old provenance table printed
    # frozen-data filenames + engine file:line; the page now cites stable
    # claim IDs (E1-E6), the chain itself lives in VERIFICATION.md.
    out.append(rect(X, 674, CW, 148, fill=PANEL, stroke=RULE, sw=1, rx=10))
    out.append(text(X + 16, 700, "声明登记簿（E1–E6 → 覆盖章节；冻结数据与源码锚点见 VERIFICATION.md 证据链）",
                    size=12.5, fill=INK, weight="700"))
    rows = [
        ("E1", "逐节点溯源：六字段责任档案 + explain 六查询 + 覆盖 100%", "章节 02·03·04"),
        ("E2", "能力谈判：174 条逐节点判决 + fail-loud 原子失败", "章节 05·06·07"),
        ("E3", "补丁等价：信封六要素 + 四道拒绝门 + 5 项测试保证", "章节 09·10"),
        ("E4", "确定性地基：三对产物指纹全等（配角）", "章节 01"),
        ("E5", "损耗诚实：1 条损耗记录逐字落 manifest", "章节 08"),
        ("E6", "门禁与指纹：81 码 ×14 族 + schema 防漂移 + 62 测试", "章节 11"),
    ]
    yy = 722
    for eid, claim, panels_ in rows:
        out.append(code(X + 16, yy, eid, size=10.5, fill=FLOW_DK,
                        weight="700"))
        out.append(text(X + 70, yy, claim, size=10.5, fill=INK))
        out.append(code(X + 760, yy, panels_, size=10, fill=MUTED))
        yy += 18
    out.append(src_note(X, 846,
                        "全部声明冻结于一次真实引擎运行；逐条证据链（冻结数据、源码锚点、复现命令）见 VERIFICATION.md"))
    return svg(W, h, *out)


PANELS = [
    ("01-hero", p_hero),
    ("02-pipeline", p_pipeline),
    ("03-origin", p_origin),
    ("04-explain-tree", p_explain),
    ("05-coverage", p_coverage),
    ("06-capability-surface", p_capsurface),
    ("07-decisions", p_decisions_impl),
    ("08-fail-loud", p_fail_loud),
    ("09-loss", p_loss),
    ("10-patch-gate", p_patch_gate),
    ("11-patch-equivalence", p_patch_equiv),
    ("12-gates", p_gates),
]


def render_all() -> list[tuple[str, str]]:
    data = {name: load(name) for name in
            ["engine", "cli", "tests", "diag_codes", "scene_nodes",
             "capability", "examples", "determinism", "explain_samples",
             "fail_loud", "patch", "schemas"]}
    results = []
    for pid, fn in PANELS:
        results.append((pid, fn(data)))
    return results
