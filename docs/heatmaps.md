# Categorical heatmaps in VizHIR 0.4 and 0.5

`chart.heatmap` maps two string categories to a rectangle and one numeric value
to a quantized color. It is an explicit, keyed matrix contract: it does not
aggregate duplicate observations, invent missing cells, or substitute zero for
missing data. VizHIR 0.4 also accepts the previously supported chart, diagram,
and geometry views, including area charts.

## Author a source

```yaml
version: "0.4"
id: service-matrix
width: 800
height: 520
background: transparent
datasets:
  readings:
    key: id
    rows:
      - {id: api-tue, day: Tue, service: API, count: 10}
      - {id: jobs-mon, day: Mon, service: Jobs, count: 0}
      - {id: api-mon, day: Mon, service: API, count: 20}
views:
  - kind: chart.heatmap
    id: matrix
    title: Observed service counts
    frame: {x: 0, y: 0, width: 800, height: 520}
    dataset: readings
    x: {field: day, label: day, domain: [Mon, Tue, Wed]}
    y: {field: service, label: service, domain: [API, Jobs]}
    color:
      field: count
      label: count
      domain: [0, 20]
      palette: ["#DCEAF7", "#145DA0"]
      number_format: {notation: fixed, precision: 0}
```

`x` and `y` require `field`; each optionally accepts `label` and an ordered
string `domain`. `color` requires `field` and optionally accepts `label`, a
numeric two-endpoint `domain`, a `palette` of 2–9 portable colors, and
`number_format: {notation: fixed|scientific, precision: 0..12}`. Omit optional
fields rather than assigning `null`. Unknown options fail, including numeric
axis options on the category encodings and unsupported aggregation, 0.4 cell-label,
interpolation, and padding controls.

### Categories, values, and identity

The dataset must be nonempty. Every row must supply non-null string x/y values
and a finite Int64 or Float64 color value under the existing numeric conversion
rules. The original typed rows, row order, values, and dataset keys remain
intact. Every actual `(x, y)` pair must be unique, even when duplicate rows have
different stable keys or identical numeric values. Invalid rows are diagnosed,
never dropped or merged.

An omitted category domain follows the first occurrence of each string in
source order. An explicit domain must be unique, ordered, and cover every
observed category. Extra entries reserve empty bands. Categories are literal:
no trimming or case folding occurs, and normal spaces are preserved. Empty
strings, C0/C1 control characters, U+2028, and U+2029 are rejected in observed
categories and explicit domains.

A missing pair produces no rectangle. A present zero produces an ordinary
keyed rectangle. Every cell uses the stable Scene ID `<view-id>/cell/<row-key>`;
changing domain order moves cells without changing their IDs. MIR instances
remain in source order. The y domain is displayed from top to bottom.

### Quantized colors and numeric intervals

Without a color domain, the compiler uses the minimum and maximum of the raw
projected numeric values. It neither includes zero automatically nor expands
the domain to nice bounds. Explicit bounds must be finite, ordered `lo <= hi`,
and have a finite span. Out-of-domain values fail instead of being clamped.

For `k` colors and a nonconstant domain, the `k - 1` thresholds use this exact
sequence for each `i` from 1 through `k - 1`:

```text
span = hi - lo
fraction = i / k
offset = span * fraction
threshold = lo + offset
```

No fused or algebraically substituted arithmetic is used. Generated zero is
normalized to positive zero. Every threshold must be finite, strictly inside
the domain, and strictly increasing; unrepresentable subdivisions fail.
Threshold ties enter the upper bin, and the final upper endpoint is included.
The quantitative legend shows explicit `[a, b)` intervals, ending in `[a, b]`.

For a constant domain `lo == hi`, thresholds are empty and the scale retains
its full palette. All present cells use palette index `floor((k - 1) / 2)`;
only the exact domain value is accepted. The legend displays `= c`.

By default, numeric labels use the shortest decimal representation for zero or
`1e-4 <= abs(value) < 1e6`, and the shortest scientific representation otherwise.
An explicit format uses the requested notation and precision. If adjacent
boundaries become indistinguishable at that precision, compilation diagnoses
the ambiguity instead of silently changing the format.

An explicit palette preserves every authored portable color, including alpha.
Otherwise `quantize-ramp/1` derives five encoded-sRGB samples between the
resolved canonical theme's `group_fills[0]` and `mark` endpoints. Each channel
uses `low + (high - low) * (i / 4)` in that order, then nearest-integer half-up
rounding `floor(value + 0.5)`. This is encoded-sRGB interpolation, not linear-light
interpolation. An unthemed heatmap alone uses the pinned registry's azure ramp
as its fallback. This does not create a new theme context or paint a canvas
background, and it does not change legacy chart defaults.

## Resolve, refresh, and inspect

```sh
vizir validate examples/chart/sparse-heatmap.viz.yaml
vizir normalize examples/chart/sparse-heatmap.viz.yaml --theme azure -o /tmp/heatmap.mir.json
vizir lower /tmp/heatmap.mir.json -o /tmp/heatmap.scene.json
vizir render /tmp/heatmap.mir.json --format svg -o /tmp/heatmap.svg
vizir render /tmp/heatmap.mir.json --format png --background transparent -o /tmp/heatmap.png
vizir explain /tmp/heatmap.mir.json --node coverage/cell/api-tue
```

Normalization produces matching VizMIR `version: "0.4"` and
`source_hir_version: "0.4"`. The `heatmap` mark has x/y/color scale bindings and
`instances: [{key, x, y, value}]`. Its plan contains exactly three scales:
zero-padding x/y `band` scales and a `quantize-color` scale with explicit
`domain`, `thresholds`, and the full resolved color `range`. Exactly one bottom
x axis, one left y axis, and one right quantitative legend must reference those
bindings. Direct MIR cannot omit, duplicate, or substitute these guides.

For heatmaps, direct MIR requires `chart.space` and both band scales'
`range_space` references to name the same existing coordinate space. That space
must be parentless, have `Document` kind and `SceneUnit` units, and use an
identity transform. The space ID is opaque and may be renamed consistently.
Transformed or separately referenced heatmap spaces are rejected; this
restriction does not change legacy chart contracts.

Bare normalized MIR is an input to the public Rust compiler APIs, not a CLI
source document. For durable CLI replay, normalize with `--theme azure` as above,
or with an explicit measured-text context, and retain the existing
`vizir-themed-mir/1` or `vizir-compiled-mir/1` envelope. Unthemed normalization
still exposes the bare plan for inspection and API use.

Lowering checks the materialized cache against the source plan. A stale cache
fails replay. After an intentional data edit inside a saved CLI envelope,
`vizir normalize edited.mir.json -o refreshed.mir.json` refreshes instances
while preserving the declared
scales, domains, thresholds, colors, ranges, source row order, and layout.
Refresh does not infer a new domain or reflow the chart; recompile HIR when the
existing plan no longer fits the new data. Direct MIR remains an inspectable
artifact rather than a second canonical authoring source.

### Layout and measured text

The shared layout allocator reserves both category axes, the vertical numeric
legend, and chart/axis/legend titles before assigning plot ranges. Category
labels use fixed 10px text, and legend labels use fixed 10.5px text. Categories
use actual band centers and boundaries, not a synthetic set of numeric ticks.
No labels are pruned, hidden, shrunk, or clipped to force a fit. Plain text uses
conservative envelopes; explicit measured text checks shaped advance and ink
bounds, including final serialized containment.

The remaining plot must be at least 64 by 64 Scene2D units. Adjacent cells share
common rounded boundary arrays; every emitted rectangle must retain positive
width and height at four-decimal geometry precision. Shared serialized edges do
not guarantee seam-free rasterization: a native rasterizer may antialias
fractional-pixel edges. If the frame cannot fit the labels and cells, compilation
fails rather than silently losing content.

The existing supplied-font workflow and `vizir-text-wrap/2`, `/3`, and `/4`
policies support the `chart.title` role in HIR/MIR 0.4. Title blocks reserve
measured height before plot allocation. Heatmap category and quantitative-legend
wrapping are deferred; these roles remain single-line and do not inherit bar
category wrapping. See [measured text](measured-text.md) and
[title policy instructions](wrapping.md#semantic-chart-titles-vizir-text-wrap2).

### Resource limits

Each categorical axis permits at most 256 domain entries. Their checked product
is at most 65,536 positions; this includes explicit empty bands. There may be at
most 16,384 present cells per chart and 65,536 per compilation call. Individual
labels are limited to 16 KiB, with at most 1 MiB of domain-label bytes per chart.
That budget sums unique entries within each axis; a spelling shared by x and y
counts once on each axis. Existing text, evaluation, and CLI input/resource
limits still apply.
These are ceilings, not a promise that all permitted labels fit a given frame.

## Compose a mixed dashboard

`schema: vizir-composition/0.3` adds frame-free heatmap panels and emits VizHIR
0.4. Existing panels, including areas, remain available. The older composition
0.1 and 0.2 contracts still emit HIR 0.2 and 0.3 respectively and reject heatmaps.

```sh
vizir compose examples/composition/heatmap-dashboard.compose.yaml -o /tmp/heatmap-dashboard.viz.json
vizir validate /tmp/heatmap-dashboard.viz.json
vizir normalize /tmp/heatmap-dashboard.viz.json --theme azure -o /tmp/heatmap-dashboard.mir.json
vizir lower /tmp/heatmap-dashboard.mir.json -o /tmp/heatmap-dashboard.scene.json
vizir render /tmp/heatmap-dashboard.mir.json --format svg -o /tmp/heatmap-dashboard.svg
vizir render /tmp/heatmap-dashboard.mir.json --format png --background transparent -o /tmp/heatmap-dashboard.png
vizir explain /tmp/heatmap-dashboard.mir.json --node coverage/cell/api-tue
```

The executable [dashboard](../examples/composition/heatmap-dashboard.compose.yaml)
combines dense and sparse categorical matrices with a grouped area comparison
and semantic notes. Standalone [sparse](../examples/chart/sparse-heatmap.viz.yaml),
[dense](../examples/chart/dense-heatmap.viz.yaml), and
[constant-domain](../examples/chart/constant-heatmap.viz.yaml) sources use the same
CLI workflow. Transparent PNG is produced by the existing native rasterizer;
SVG and PNG consume the same resolved scene.

## Version and Rust source compatibility

Unlabeled heatmaps require HIR/MIR 0.4 or 0.5; old 0.1, 0.2, and 0.3 documents retain their exact
wire contracts and schema definition closures. The existing themed/compiled
context envelope names remain at version 1, with matching MIR/source pairs.
There is no new theme envelope or implicit canvas paint.

New runtime enum variants, including `View::Heatmap`, `ChartMark::Heatmap`,
`Panel::Heatmap`, and `MirScale::QuantizeColor`, can require updates to exhaustive
Rust matches. Old wire compatibility does not imply Rust source compatibility.
Standalone `schema_for!(ChartMark)`, `schema_for!(Panel)`, and
`schema_for!(MirScale)` remain legacy unversioned schemas; the standalone
`MirScale` schema excludes `QuantizeColor`. Use the versioned MIR/composition
root schema APIs, or
`vizir schema mir` and `vizir schema composition`, for heatmap branches. Use
`Composition`, `parse_versioned_composition`, and `compose_versioned` for the new
composition version; the existing V1 APIs remain 0.1-to-HIR-0.2 adapters.

This unit does not add CSV ingestion, request cards, aggregation, clustering,
continuous color interpolation, or category/legend wrapping.
Optional value labels below are a separate 0.5 capability; 0.4 still rejects them.


## Optional present-cell value labels in 0.5

Use HIR/MIR `0.5`, or composition `vizir-composition/0.4`, and add a closed
`value_labels` object to a heatmap chart/panel:

```yaml
value_labels: {}
# Or request exact decimal places and an explicit opaque text color:
# value_labels:
#   number_format: {notation: fixed, precision: 0}
#   color: "#000000"
```

Omitting the object preserves the existing scene and target bytes. Empty `{}`
enables automatic numeric formatting and contrast. `number_format` uses the
existing fixed/scientific notation and 0–12 decimal-place precision, independently
of `color.number_format` (the legend). Both fields are optional, but explicit
null, unknown properties, templates, `show_labels`, and arbitrary authored label
text are rejected. HIR/MIR 0.1–0.4 and composition 0.1–0.3 retain their closed
contracts and full schema reference closures.

Every present cell gets exactly one label from the value driving its color
expression, before quantization. Zero is `0`; a missing pair still has neither
rectangle nor label. The MIR keeps `MirHeatmapCell` unchanged and adds a mark-level
`value_labels` object with optional format/color plus ordered `instances` of
`{key, text}`. This is a checked derived cache: replay rejects text, key, order,
or count tampering. Refresh regenerates it from typed source values while keeping
explicit scales and plan options. Scene IDs are `<view-id>/cell-label/<row-key>`;
HIR/MIR identity, data key, lineage, and numeric text survive measured outlining.

Automatic Int64 labels retain every integer digit, including values above 2^53
and both i64 extrema. Fixed Int64 format appends the requested decimal zeros;
scientific Int64 format uses exact decimal round-to-nearest, ties-to-even, with
carry and sign handling. Default Float64 labels use shortest round-tripping
spelling, never abbreviations or rounding that hides a finite nonzero value.
Explicit Float64 format retains existing numeric formatter rounding. Displayed
negative zero is normalized; original source/cache numeric bits are retained.

The exact value is the evaluated typed number, not lexical JSON spelling. Mixed
Int64/Float64 columns infer Float64; digits already lost by that conversion are
not recoverable. Explicit CSV Float64 conversion is the same authoritative
boundary. Keep large exact integers in Int64 columns when their digits matter.

Text is centered at a fixed 12 Scene2D units, regular weight, single line, with
4 units of padding on each cell edge. The existing supplied-font pipeline
measures logical-plus-ink bounds and outlines that same face. Legacy text uses
the existing conservative envelope. Both check each actual four-decimal cell
rectangle and fail the whole compilation if any label cannot fit, identifying
the chart and row key. There is no shrinking, clipping, wrapping, pruning, or
silent disappearance. Axes and legend geometry stay unchanged.

Automatic contrast compares black and white using linearized sRGB relative
luminance of the exact resolved palette color (ties choose black). Opaque
`#RRGGBB` and `#RRGGBBFF` are supported. For automatic contrast all palette
entries must be opaque, including unused bins. Transparent/translucent entries require an
explicit opaque `value_labels.color`; no canvas or embedding background is
assumed. An explicit color is the author's choice, not a guarantee of contrast.

Labeled compilations additionally cap all charts together at 4,096 generated
labels, 512 UTF-8 bytes per label, and 1 MiB of label text. Source and stale-cache
counts are checked before cloning. Measured mode retains all existing text,
glyph, outline, cache, operation and output budgets; repeated measurements count
even on cache hits, so 4,096 is a ceiling rather than promised measured capacity.
MIR, Scene and SVG publication is bounded to 32 MiB in legacy labeled mode too.
SVG streams through the existing emitter into a bounded writer; PNG retains its
existing pixel/decoded-byte limits and transactional publication.

Run the existing commands, with the same optional text profile/font arguments:

```sh
vizir validate examples/chart/labeled-heatmap.viz.yaml
vizir normalize examples/chart/labeled-heatmap.viz.yaml --theme sage -o /tmp/labels.mir.json
vizir lower /tmp/labels.mir.json -o /tmp/labels.scene.json
vizir render /tmp/labels.mir.json --format svg -o /tmp/labels.svg
vizir render /tmp/labels.mir.json --format png --background transparent -o /tmp/labels.png
vizir explain /tmp/labels.mir.json --node coverage/cell-label/jobs-mon
```

The original provider descriptor/profile remains matching MIR/HIR 0.4 only
and rejects 0.5. The separate native `render-compiled-svg-v2` capability replays
matching MIR/HIR 0.5 with required exact measured-text resources and a closed
`vizir.render-receipt/v2`; see [provider contracts](plot-provider.md). Direct CLI
and Rust compilation retain their existing unmeasured and measured modes. A
provider capability alone does not establish request-card routing or independent
Hub verification.
