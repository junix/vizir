# VizHIR wire formats 0.1 and 0.2

A document declares `version`, stable `id`, output dimensions, named inline
datasets, and one or more views. Each view has an explicit frame so dashboard
composition remains semantic and deterministic.

Supported view tags:

```text
chart.scatter
chart.line
chart.bar
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
options even if its source values happen to be numbers. Color and series
legends do not accept numeric axis options. Unknown option fields and notation
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
