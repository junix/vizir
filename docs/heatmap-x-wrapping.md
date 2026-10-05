# Measured heatmap x-category wrapping

`vizir-text-wrap/5` adds one semantic role: `heatmap.x_category_labels`.
It is an explicit native compiler/CLI context policy, with no new HIR or MIR
fields or versions. Native heatmap versions 0.4–0.7 can use it. Prior wrapping
profiles 1–4 reject the role, and compiled-SVG provider profiles V1/V2
remain closed to wrapping profiles 1–4. Their descriptors and receipts are unchanged.
The separate [provider V3](plot-provider.md) accepts wrapping profiles 1–5 with
matching compiled MIR/HIR 0.7, exact measured fonts and a versioned V3 receipt.

```json
{
  "profile": "vizir-text-wrap/5",
  "engine": "unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)",
  "targets": [],
  "semantic_targets": [{
    "view_id": "scores",
    "role": "heatmap.x_category_labels",
    "max_width": 60,
    "max_lines": 2,
    "line_height": 16
  }]
}
```

A v5 policy must contain at least one heatmap x-category target. Existing
geometry, chart-title, bar-category, and nonempty diagram-target lists can be
combined with it, under the same whole-call budgets. A target must resolve to
exactly one matching source heatmap and its bottom-axis Band binding. A single
view cannot be both a bar-category and heatmap-category target.

## Layout and source contract

- Font size remains 10 Scene2D units, Regular weight, with no automatic shrinking,
  rotation, truncation, substitution, or emergency splitting of long words
- UAX14 line opportunities are intersected with extended-grapheme boundaries;
  the declared exact face shapes every candidate and missing glyphs fail
- Each line is independently centered by its advance. Its full advance/ink
  envelope must fit both `max_width` and its actual serialized Band cell,
  with 4 units of clearance at each side
  Serialized advance spans use integer-grid subtraction before scaling, so an
  exact 60-unit span remains 60 even at translated anchors. Analytic ink extrema
  and overhangs remain unrounded; no epsilon loosens the authored width budget
- Line count and line height are explicit. Adjacent line ink cannot overlap
- The union of logical and ink extents reserves the bottom of the plot before
  emitting MIR ranges. Plot-to-label and label-to-axis-title gaps are at least
  8 units. The x title uses its measured fixed baseline at frame bottom minus 16
- The existing heatmap minimum 64×64 plot, y-label, legend, frame containment,
  and pairwise collision checks still apply. Only explicitly targeted x labels
  wrap. Y categories, chart/axis/legend titles, and legend labels are unchanged
- Categories are still canonical data keys: LF, CRLF, Unicode line/paragraph
  separators, and C0/C1 controls remain prohibited by HIR/MIR heatmap rules.
  This role provides soft wrapping, not multiline authored category keys
- Original bytes, ordered domain entries (including absent declared categories),
  cell keys, values, guide identity, and data lineage remain authoritative.
  Outline provenance records the semantic role, domain index, source byte spans,
  baselines, and original text. Wrap layout is marked explicitly; no text is lost

Plans cache exact fonts/text identity, role, source strings, axis title, frame,
plot bounds, wrapping profile, and authored budgets. Lowering and Scene building
reuse the same plan without reshaping or consuming line budgets again; cache
lookups themselves remain bounded. Fonts are supplied as exact SHA-256 resources,
never persisted filesystem paths. Font-independent Scene paths and SVG bytes
replay exactly from the compiled envelope plus those font resources.

Changing source, width, or line spacing after normalization may require new
plot ranges. Replay and refresh reject stale ranges instead of silently reflowing;
normalize again from the original HIR. Failed CLI operations leave existing
outputs and manifests intact. Untargeted output follows the original path.

## Runnable 720×400 Chinese example

The example has six 12-character Chinese category names. At this width exact-face
single-line layout fails, while the explicit 60-unit, two-line, 16-unit policy fits.
The earlier eight-character audit fixture already fits with exact-face metrics;
its unmeasured heuristic failure is a separate baseline and is not claimed to
require wrapping.

Use [`wrapped-heatmap-x.compose.yaml`](../examples/composition/wrapped-heatmap-x.compose.yaml),
[`heatmap-x-wrapping-policy.json`](../examples/text/heatmap-x-wrapping-policy.json),
and [`heatmap-x-full-font-profile.json`](../examples/text/heatmap-x-full-font-profile.json).
The profile pins full Noto Sans CJK SC Regular/Medium/Bold fonts, not the small
repository test subsets. Supply originals from the pinned upstream commit
`notofonts/noto-cjk@165c01b46ea533872e002e0785ff17e44f6d97d8`, under
`Sans/OTF/SimplifiedChinese/NotoSansCJKsc-{Regular,Medium,Bold}.otf`.
Their digests and source URLs are recorded in the existing
[`wrapping-fonts/manifest.json`](../crates/vizir-compiler/tests/fixtures/wrapping-fonts/manifest.json)
under `source_sha256` and `source_url`. Do not substitute test-subset digests.

```sh
vizir compose examples/composition/wrapped-heatmap-x.compose.yaml -o heatmap-x.viz.json
vizir render heatmap-x.viz.json \
  --text-profile examples/text/heatmap-x-full-font-profile.json \
  --text-layout examples/text/heatmap-x-wrapping-policy.json \
  --font "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b=$FONTS/NotoSansCJKsc-Regular.otf" \
  --font "ca094f6b0001fb048ca39ddd797a0cdb0179e1e55c6561e111c49c3e6a61d7b7=$FONTS/NotoSansCJKsc-Medium.otf" \
  --font "b5f0d1a190a7f9b43c310a8850630af12553df32c4c050543f9059732d9b4c0a=$FONTS/NotoSansCJKsc-Bold.otf" \
  --format png --background transparent -o heatmap-x-wrapping.png
```

For a compiled envelope use the same options with `normalize`, then render the
saved envelope with just the font resources. The layout policy is persisted.
