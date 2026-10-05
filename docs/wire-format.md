# VizHIR wire formats 0.1, 0.2, 0.3 and 0.4

A document declares `version`, stable `id`, output dimensions, named inline
datasets, and one or more views. Each view has an explicit frame so dashboard
composition remains semantic and deterministic.

Supported view tags:

```text
chart.scatter
chart.line
chart.bar
chart.area (0.3 and 0.4)
chart.heatmap (0.4 only)
diagram.graph
geometry.scene
```

Charts require a dataset, stable key field, field encodings, and an explicit
frame. Diagram edges reference declared node IDs. Geometry paths use typed
commands (`move`, `line`, `cubic`, `close`) instead of SVG path strings.

Colors are portable sRGB hex values (`#RRGGBB` or `#RRGGBBAA`) plus
`transparent`. Measurements in the MVP are abstract Scene2D units mapped
one-to-one to the SVG viewport.

See `examples/` for complete executable documents. `vizir normalize` is the
canonical way to inspect typed data sources, expression ASTs, coordinate spaces,
expanded scales, marks, guides, layout requests, and loss records.

VizMIR, ScenePatch, and Capability are strict generated JSON contracts. Unknown
fields are rejected by their Rust model, and their JSON Schemas are checked in
under `schemas/`. YAML remains an authoring convenience for VizHIR only.

## Importing CSV without changing the wire format

The independent [`vizir-csv-import/1` authoring boundary](csv-import.md) converts
explicitly typed local CSV into the existing inline `Dataset { key, rows }`.
`vizir import-csv` writes ordinary HIR in the template's original version, or an
ordinary composition source when `--template-kind composition` is selected.
Its required provenance receipt is separate from HIR and MIR. There are no
file paths, reload operators, type declarations or CSV resources in the emitted
dataset. Existing wire versions, schema branches and inline-data behavior are
unchanged; subsequent compilation follows the normal contracts below.

## Numeric axis formats in 0.2

Numeric axis formatting is an explicit, versioned opt-in. Declare
`version: "0.2"` and add `axis.number_format` to a scatter or line chart's `x` or `y`
field encoding, or a bar chart's `value` encoding:

```yaml
version: "0.2"
# ...document and chart fields...
x:
  field: energy
  label: energy (J)
  axis:
    number_format: {notation: scientific, precision: 2}
y:
  field: reading
  axis:
    number_format: {notation: fixed, precision: 3}
```

`number_format` requires both fields. `notation` is the closed enum
`scientific` or `fixed`. `precision` is an integer-valued number from 0 through
12, inclusive, and means digits after the decimal point (not significant
digits). Integral numeric syntax such as `2.0` or `2e0` is accepted, as is
`-0.0` for zero; canonical serialization emits integer values such as `2` and
`0`. Fractions, strings, and booleans are rejected.
Scientific notation uses a lowercase `e` and an unpadded exponent without a
positive sign: `1.00e150`, `1.00e-120`, and `0.00e0` at precision 2. Fixed
notation retains trailing zeros: `0.123`, `12.000`, and `0.000` at precision 3.
Rounded negative zero is rendered without a minus sign. If adjacent ticks
would have identical labels at the requested precision, compilation reports
`VIZ-FORMAT-0001`; increase precision rather than relying on an implicit
notation or precision change. These are display options; the dataset, scale
domain, and mark values remain numeric. Changing notation or precision does
not change domain inference or materialized mark values.

The format applies only to numeric axes. A bar `category` axis rejects these
options even if its source values happen to be numbers. Categorical color and series
legends do not accept numeric axis options. The separate numeric heatmap legend
in 0.4 accepts `color.number_format`; it does not use `axis.number_format`. Unknown option fields and notation
values are rejected; precision outside 0–12 is diagnosed.

When at least one axis opts in, all numeric tick labels in that chart use the
same deterministic text-envelope calculation during normalization and scene
construction. Insets widen to fit full labels, including endpoint labels.
If the required labels cannot fit without overlap while retaining the minimum
plot area, compilation reports a layout diagnostic. Labels are not silently
truncated, omitted, or shrunk. The envelopes are conservative advance estimates
for the default sans-serif stack, not a font-shaping guarantee for every font.

### Version and source compatibility

VizHIR 0.1 remains accepted. Optional `axis` and `number_format` fields
must be omitted rather than set to `null`. Missing fields remain absent when
serialized, and documents without explicit formats retain their legacy tick
text and layout. Even an explicit empty `axis: {}` is a 0.2 option;
remove it when producing a 0.1 document. VizHIR 0.2 normalizes to VizMIR 0.2,
while 0.1 continues to normalize to 0.1. `source_hir_version` records the
original HIR version independently of the MIR version.

The Rust API adds `FieldEncoding.axis: Option<AxisOptions>` and
`MirGuide.number_format: Option<NumberFormat>`. This is wire-compatible for
old documents, but existing Rust struct literals must add `axis: None` and
`number_format: None`, respectively. `AxisOptions.number_format` is optional;
`NumberFormat` has `notation: NumberNotation` and `precision: u8`. The notation
variants are `NumberNotation::Scientific` and `NumberNotation::Fixed`.

Executable examples:

- `examples/chart/scientific-magnitudes.viz.yaml`
- `examples/chart/measurement-precision.viz.yaml`
- `examples/chart/mixed-axis-formats.viz.yaml`

## Area charts in 0.3

VizHIR 0.3 adds `chart.area` with numeric `x`/`y` field encodings, optional
`series`, required finite `baseline`, and required `order: x-ascending`.
The first contract is linear and unstacked, with fixed 0.35 fill opacity.
Each group needs at least two strictly increasing representable x values;
source rows and stable keys remain intact. Missing/null values, duplicate x
coordinates and unsupported options fail. See [area charts](area-charts.md)
for materialization, baseline, replay and rendering details.

Existing view kinds and numeric-axis options are valid in 0.3. A 0.3 document
normalizes to a matching VizMIR 0.3 / `source_hir_version: "0.3"` pair.
Versions 0.1 and 0.2 retain their existing contracts and reject area marks.
The published old schema branches retain their closed dependency definitions;
0.3 uses its own branch. The themed and compiled envelope names stay at version
1, with a matching 0.3 MIR/source pair admitted by their new branches.

## Categorical heatmaps in 0.4

VizHIR 0.4 adds `chart.heatmap` with required dataset, frame, string-category
`x`/`y` encodings, and a finite numeric `color` encoding. Each category encoding
accepts `field`, optional `label`, and optional ordered string `domain`. Color
accepts `field`, optional `label`, optional numeric `domain: [lo, hi]`, an optional
2–9-color `palette`, and optional `number_format`. New optional fields must be
absent rather than `null`.

Observed category pairs must be unique. Missing pairs stay empty and present
zero values create keyed rectangles. Explicit domains cover every observation
and may reserve empty bands. No aggregation, trimming, case folding, dropping,
or imputation occurs. Color uses explicit quantization thresholds and an honest
interval legend; out-of-range values are rejected. See [the complete heatmap
contract](heatmaps.md) for arithmetic, constant domains, layout, and limits.

A 0.4 document normalizes to VizMIR 0.4 with `source_hir_version: "0.4"`.
Existing view kinds, including areas, remain available. The heatmap mark stores
x/y/color bindings and source-ordered `{key, x, y, value}` instances. Its three
scales are zero-padding x/y bands and `quantize-color`, with explicit domain,
thresholds, and the full resolved palette. Exactly two bound axes and one
right quantitative legend are required. Refresh preserves that resolved plan;
stale instances fail ordinary replay.

Versions 0.1–0.3 reject heatmaps and retain their old schema closures. New 0.4
branches extend the existing themed/compiled envelope names without a new
context format. Runtime enum additions may break exhaustive Rust matches.
Standalone `ChartMark` and `Panel` schemas intentionally remain legacy; use
versioned MIR and composition root schemas for heatmaps.

## Authored numeric position domains (HIR/MIR 0.7)

Numeric `FieldEncoding.domain` is an optional, non-null two-number interval
under HIR 0.7. The compiler stores exact endpoints and
`out_of_domain: "reject"` on the resulting linear scale. Only scatter, line,
and unstacked-area x/y and bar value accept the feature; observations and
applicable baselines must be within the closed interval. Omitted fields
preserve old serialized bytes and inference/extrapolation behavior.
MIR 0.7 requires matching `source_hir_version: "0.7"`; older source, MIR and
composition branches remain closed. See [numeric domains](numeric-domains.md).
