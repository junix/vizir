#!/usr/bin/env python3
"""SVG primitives for the vizir explainer. Literal hex only (no CSS var() in
SVG presentation attributes — keeps svg-linter findings meaningful).

House style inherited from the lut-ops / graph-ir-rs explainer fleet:
light editorial paper, blue-dominant roles (FLOW ramp), warm accents for
fail-loud / loss semantics only.
"""
from __future__ import annotations

import math

# ---- bound role palette (design-guide section 6; user prefers blue) --------
PAPER = "#F7F4EE"
PANEL = "#FDFBF7"
INK = "#17212B"
MUTED = "#5D6873"
RULE = "#D9E1E3"
FLOW = "#356A79"      # primary blue
FLOW_DK = "#28505C"
FLOW_LT = "#7FA3AD"
FLOW_XLT = "#B9CDD3"
FLOW_TINT = "#E3EDF0"  # pale blue wash for emphasis zones
TEAL = "#2A9D8F"
TINT = "#DDF2EC"
WARN = "#E76F51"       # fail-loud / unsupported
WARN_LT = "#F4C7B8"
OUTCOME = "#E9C46A"    # losses (honest-degradation semantics)
OUTCOME_LT = "#F7E8C3"
# series (color-palette-rs sweetie-16; dataviz-validated)
CH_R = "#B13E53"
CH_G = "#38B764"
CH_B = "#3B5DC9"

FONT_DISPLAY = "'Source Han Serif SC','PingFang SC',serif"
FONT_BODY = "'Source Han Sans SC','PingFang SC',sans-serif"
FONT_MONO = "'0xProto Nerd Font','SF Mono',Menlo,monospace"


def esc(s: str) -> str:
    return (str(s).replace("&", "&amp;").replace("<", "&lt;")
            .replace(">", "&gt;").replace('"', "&quot;"))


def el(tag: str, attrs: dict | None = None, *children: str) -> str:
    a = " ".join(f'{k}="{esc(str(v)) if not str(v).startswith("url") else v}"'
                 for k, v in (attrs or {}).items())
    inner = "".join(children)
    if not children:
        return f"<{tag} {a}/>"
    return f"<{tag} {a}>{inner}</{tag}>"


def text(x, y, s, size=13, fill=INK, family=FONT_BODY, weight="400",
         anchor="start", style=None, spacing=None):
    # Empty strings emit nothing (a zero-ink <text></text> is dead markup that
    # makes svg-linter's text rules skip real pairs — 2026-09 R7 fix). Callers
    # that use "" as a blank spacer line still advance their own line cursor.
    if not s:
        return ""
    attrs = {"x": round(x, 2), "y": round(y, 2), "font-size": size,
             "fill": fill, "font-family": family, "font-weight": weight,
             "text-anchor": anchor}
    if style:
        attrs["font-style"] = style
    if spacing:
        attrs["letter-spacing"] = spacing
    return el("text", attrs, esc(s))


def rect(x, y, w, h, fill=PANEL, stroke=None, sw=1, rx=0, dash=None,
         opacity=None):
    attrs = {"x": round(x, 2), "y": round(y, 2), "width": round(w, 2),
             "height": round(h, 2), "fill": fill}
    if stroke:
        attrs["stroke"] = stroke
        attrs["stroke-width"] = sw
    if rx:
        attrs["rx"] = rx
    if dash:
        attrs["stroke-dasharray"] = dash
    if opacity is not None:
        attrs["opacity"] = opacity
    return el("rect", attrs)


def line(x1, y1, x2, y2, stroke=RULE, sw=1, dash=None):
    attrs = {"x1": round(x1, 2), "y1": round(y1, 2), "x2": round(x2, 2),
             "y2": round(y2, 2), "stroke": stroke, "stroke-width": sw}
    if dash:
        attrs["stroke-dasharray"] = dash
    return el("line", attrs)


def circle(cx, cy, r, fill=FLOW, stroke=None, sw=1):
    attrs = {"cx": round(cx, 2), "cy": round(cy, 2), "r": round(r, 2),
             "fill": fill}
    if stroke:
        attrs["stroke"] = stroke
        attrs["stroke-width"] = sw
    return el("circle", attrs)


def path(d, stroke=INK, sw=1.5, fill="none", dash=None):
    attrs = {"d": d, "stroke": stroke, "stroke-width": sw, "fill": fill}
    if dash:
        attrs["stroke-dasharray"] = dash
    return el("path", attrs)


def g(*children, transform=None, opacity=None):
    attrs = {}
    if transform:
        attrs["transform"] = transform
    if opacity is not None:
        attrs["opacity"] = opacity
    return el("g", attrs or None, *children)


def svg(w, h, *children, bg=PAPER):
    inner = rect(0, 0, w, h, fill=bg) if bg else ""
    return el("svg", {"xmlns": "http://www.w3.org/2000/svg",
                      "viewBox": f"0 0 {w} {h}", "width": w, "height": h},
              inner, *children)


def code(x, y, s, size=12, fill=FLOW_DK, anchor="start", weight="400"):
    """Monospace inline code text."""
    return text(x, y, s, size=size, fill=fill, family=FONT_MONO,
                anchor=anchor, weight=weight)


def code_box(x, y, w, h, lines, size=12, fill=INK, title=None,
             stroke=RULE, bg="#FFFFFF", lh=None, weight="400"):
    """Code block: white card + mono lines. lines = [(text, color?)]."""
    lh = lh or (size + 7)
    out = [rect(x, y, w, h, fill=bg, stroke=stroke, sw=1, rx=6)]
    ty = y + size + 10
    if title:
        out.append(text(x + 12, ty, title, size=11, fill=MUTED,
                        family=FONT_MONO, anchor="start"))
        ty += lh + 4
    for item in lines:
        s, col = item if isinstance(item, tuple) else (item, fill)
        out.append(code(x + 12, ty, s, size=size, fill=col))
        ty += lh
    return "".join(out)


def chip(x, y, label, fill=FLOW_TINT, stroke=FLOW_LT, color=FLOW_DK,
         size=11, pad_x=8, h=20):
    w = pad_x * 2 + sum(2 for _ in label) + size * 0.62 * len(label)
    w = max(w, size * 0.62 * len(label) + 2 * pad_x)
    out = [rect(x, y, w, h, fill=fill, stroke=stroke, sw=1, rx=10)]
    out.append(text(x + w / 2, y + h / 2 + size * 0.36, label, size=size,
                    fill=color, family=FONT_MONO, anchor="middle"))
    return "".join(out), w


def arrow(x1, y1, x2, y2, color=FLOW, sw=2):
    ang = math.atan2(y2 - y1, x2 - x1)
    head = 7
    a1 = (x2 - head * math.cos(ang - 0.45), y2 - head * math.sin(ang - 0.45))
    a2 = (x2 - head * math.cos(ang + 0.45), y2 - head * math.sin(ang + 0.45))
    return (line(x1, y1, x2, y2, stroke=color, sw=sw)
            + path(f"M {a1[0]:.1f} {a1[1]:.1f} L {x2:.1f} {y2:.1f} "
                   f"L {a2[0]:.1f} {a2[1]:.1f}", stroke=color, sw=sw))


def elbow(x1, y1, x2, y2, r=8, color=FLOW_LT, sw=2):
    """Orthogonal connector with rounded corner: horizontal-then-vertical."""
    sx = 1 if x2 >= x1 else -1
    d = (f"M {x1:.1f} {y1:.1f} H {x2 - r * sx:.1f} "
         f"Q {x2:.1f} {y1:.1f} {x2:.1f} {y1 + r if y2 >= y1 else y1 - r:.1f} "
         f"V {y2:.1f}")
    return path(d, stroke=color, sw=sw)


def panel_head(y, kicker, title, sub=None, x=48, w=1104):
    """Chapter header: kicker chip + display title + optional sub line."""
    out = [text(x, y + 12, kicker, size=12, fill=FLOW, family=FONT_MONO,
                weight="700", spacing="2")]
    out.append(text(x, y + 46, title, size=25, fill=INK,
                    family=FONT_DISPLAY, weight="700"))
    if sub:
        out.append(text(x, y + 70, sub, size=13, fill=MUTED))
    out.append(line(x, y + 84, x + w, y + 84, stroke=RULE, sw=1))
    return "".join(out)


def stat_tile(x, y, w, h, value, label, note=None, fill=PANEL,
              accent=FLOW, vsize=26):
    out = [rect(x, y, w, h, fill=fill, stroke=RULE, sw=1, rx=8)]
    out.append(rect(x, y, 4, h, fill=accent, sw=0))
    out.append(text(x + 16, y + vsize + 10, str(value), size=vsize,
                    fill=INK, family=FONT_DISPLAY, weight="700"))
    out.append(text(x + 16, y + vsize + 30, label, size=11.5, fill=MUTED))
    if note:
        out.append(text(x + 16, y + vsize + 47, note, size=10.5, fill=FLOW,
                        family=FONT_MONO))
    return "".join(out)


def hbar(x, y, w_full, h, value, vmax, label, value_label, color=FLOW,
         track="#EFEAE0", label_w=180, vlabel_w=64, size=12):
    """Horizontal bar row: label | track+bar | value."""
    bar_x = x + label_w
    bar_w = max(2, w_full - label_w - vlabel_w) * (value / vmax)
    out = [text(x, y + h / 2 + size * 0.36, label, size=size, fill=INK,
                family=FONT_MONO, anchor="start")]
    out.append(rect(bar_x, y, max(w_full - label_w - vlabel_w, 2), h,
                    fill=track, rx=3))
    out.append(rect(bar_x, y, bar_w, h, fill=color, rx=3))
    out.append(text(bar_x + max(w_full - label_w - vlabel_w, 2) + 8,
                    y + h / 2 + size * 0.36, value_label, size=size,
                    fill=MUTED, family=FONT_MONO))
    return "".join(out)


def src_note(x, y, s, w=None):
    """Source citation line (file:line), small mono, muted blue."""
    return text(x, y, s, size=10.5, fill=FLOW_LT, family=FONT_MONO)
