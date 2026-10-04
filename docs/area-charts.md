# Linear area charts in VizHIR 0.3

`chart.area` fills the region between numeric samples and an explicit numeric
baseline. Each series becomes one closed Scene2D path. The first area contract
supports unstacked linear interpolation with fixed fill opacity 0.35. A grouped
chart overlays its series in deterministic key order; it does not compute a
stacked total.

## Author a source

Area charts require VizHIR `version: "0.3"`. They use the existing dataset key,
`x` and `y` field encodings, optional `series` color encoding, title and frame.
Two fields are required: a finite `baseline` and `order: x-ascending`.

```yaml
version: "0.3"
id: baseline-example
width: 800
height: 480
datasets:
  samples:
    key: id
    rows:
      - {id: later, x: 2, y: 8}
      - {id: first, x: 0, y: 3}
      - {id: middle, x: 1, y: 6}
views:
  - kind: chart.area
    id: signal
    frame: {x: 0, y: 0, width: 800, height: 480}
    dataset: samples
    x: {field: x, label: time}
    y: {field: y, label: value}
    baseline: 4
    order: x-ascending
```

The compiler preserves authored rows, their order and stable keys. It sorts the
materialized points by numeric x within each series. Every series needs at
least two distinct, strictly increasing representable x coordinates. Duplicate
x values, positive/negative zero at the same x, and distinct integers that
collapse to the same floating-point coordinate fail instead of being merged.
The check applies after the existing numeric conversion and again to the
resolved x positions; it does not require each integer to be individually
exact in floating point. A very wide chart domain can collapse a small local
x difference during projection, which also fails with a diagnostic. Source
keys must remain unique under the existing dataset key rules.

Both x and y must be present, non-null finite numbers. A selected series field
must be present and non-null. Missing values do not create implicit gaps or
zero-valued samples. Invalid rows are not discarded. The inferred y domain
includes the authored baseline and all sample values; the full combined span
must be finite. Negative values and crossings of the baseline are preserved.
Numeric axis formatting from VizHIR 0.2 remains available in 0.3.

`order` accepts only `x-ascending`. The first contract has no stacking,
interpolation selector, point markers, outline, opacity or style options.
Unsupported fields fail rather than being ignored. To choose series colors,
use the existing optional `series.palette` or an explicit theme.

## Resolve and inspect

```sh
vizir validate examples/chart/baseline-area.viz.yaml
vizir normalize examples/chart/baseline-area.viz.yaml -o /tmp/area.mir.json
vizir lower /tmp/area.mir.json -o /tmp/area.scene.json
vizir render /tmp/area.mir.json --format svg -o /tmp/area.svg
vizir render /tmp/area.mir.json --format png --background transparent -o /tmp/area.png
vizir explain /tmp/area.mir.json --node readings/area/series
```

Normalization emits a VizMIR 0.3 area mark with explicit x/y bindings, grouping
and ordering expressions, baseline, and keyed point caches. Ungrouped data
uses the series key `series`. Grouped caches and their paint order use the same
deterministic series-key ordering as line charts. Exactly one bottom axis must
reference the area x binding, and exactly one left axis must reference its y binding. When a color binding is present, an
explicit right legend must reference that same color scale. Direct MIR cannot
omit these guides, substitute another scale, duplicate a guide slot or rely
on a legacy implicit legend.

A resolved path runs from the first x at the baseline through each sorted
sample, then returns from the last x to the baseline and closes. Its stable
Scene ID is `<view-id>/area/<series-key>`. Data-point keys remain in the MIR
cache; path provenance identifies the source dataset and series. Vertices do
not become separate Scene nodes. Updating a series changes its existing path
through the usual ScenePatch workflow.

Direct MIR retains its explicitly authored domains and ranges, including the
existing linear extrapolation semantics. Refresh recomputes keyed samples and
checks them against the declared plan; it does not infer new domains, reorder
source rows or reflow chart layout. A stale materialized cache fails ordinary
replay. Use an intentional refresh for data edits that satisfy the existing
plan, or compile the original HIR for a new layout/domain allocation.

The existing theme and compiled-context envelope names remain unchanged. Their
new 0.3 branches require a matching `version`/`source_hir_version` pair. Measured
text and source-targeted `chart.title` wrapping work with a 0.3 area chart using
the same explicit font resources and text-layout policies as other charts.
No new text profile is required. Area remains unavailable in HIR/MIR 0.1 or 0.2.

### Rust and schema compatibility

Adding `View::Area`, `ChartMark::Area`, and `Panel::Area` can require updates to
exhaustive Rust matches. Preserving old wire documents does not imply Rust
source compatibility. Existing `CompositionV1`, `parse_composition`, and
`compose` remain 0.1-to-HIR-0.2 adapters; use `Composition`,
`parse_versioned_composition`, and `compose_versioned` for the new source version.

Standalone `schema_for!(ChartMark)` and `schema_for!(Panel)` retain their legacy,
unversioned schemas. Area callers should use the versioned VizMIR and composition
root schema APIs (or the CLI `schema mir` and `schema composition` commands).
Those roots dispatch to isolated new definitions while retaining the complete
published legacy definition closures.

## Compose a mixed dashboard

Use `schema: vizir-composition/0.2` for frame-free area panels. This composition
version emits VizHIR 0.3; the existing composition 0.1 still emits HIR 0.2 and
rejects area panels.

```sh
vizir compose examples/composition/area-dashboard.compose.yaml -o /tmp/area-dashboard.viz.json
vizir normalize /tmp/area-dashboard.viz.json --theme azure -o /tmp/area-dashboard.mir.json
vizir render /tmp/area-dashboard.mir.json --format svg -o /tmp/area-dashboard.svg
vizir render /tmp/area-dashboard.mir.json --format png --background transparent -o /tmp/area-dashboard.png
```

The executable dashboard combines a nonzero-baseline area, two overlapping
series, an ordinary line chart of those same samples, and semantic notes.
The standalone [baseline example](../examples/chart/baseline-area.viz.yaml) and
[grouped example](../examples/chart/grouped-area.viz.yaml) also run through the
normal CLI pipeline. PNG uses the existing native rasterizer and transparent
alpha validation; SVG and PNG consume the same resolved scene.
