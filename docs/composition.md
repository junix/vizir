# Grid composition source 0.1

`CompositionV1` is a small authoring wrapper that allocates equal grid cells to
frame-free panels. It emits an ordinary, validated VizHIR 0.2 document. It is
not a new HIR dialect or a claim that VizHIR 0.3 exists.

The pipeline is explicit:

```text
CompositionV1 (vizir-composition/0.1)
  -> compose -> VizHIR 0.2 with explicit frames
  -> normalize -> VizMIR 0.2
  -> lower -> Scene2D
  -> existing SVG / PNG targets
```

The emitted MIR truthfully records `source_hir_version: "0.2"`, because the
compiler receives the composed HIR document. It does not record the composition
wrapper version in that field. The wrapper does not change MIR, Scene2D, target
capabilities, or the meaning of an existing HIR frame.

## Run the mixed example

From the repository root:

```sh
cargo run -p vizir-cli -- compose examples/composition/service-grid.compose.yaml \
  --output out/service-grid.viz.json
cargo run -p vizir-cli -- validate out/service-grid.viz.json
cargo run -p vizir-cli -- normalize out/service-grid.viz.json \
  --output out/service-grid.mir.json
cargo run -p vizir-cli -- lower out/service-grid.viz.json \
  --output out/service-grid.scene.json
cargo run -p vizir-cli -- render out/service-grid.viz.json \
  --format svg --output out/service-grid.svg
cargo run -p vizir-cli -- render out/service-grid.viz.json \
  --format png --background transparent --output out/service-grid.png
```

PNG uses the existing rasterizer requirements. The authored source remains
`examples/composition/service-grid.compose.yaml`; generated HIR, MIR, Scene2D,
SVG, and PNG are outputs, not new canonical sources to edit by hand.

`compose INPUT` writes canonical HIR JSON to stdout. `compose INPUT --output
FILE` publishes the same JSON to a file and prints the emitted path. YAML and
JSON composition inputs are supported. Output paths retain the CLI's source
alias protection and staged-publication behavior: a rejected source does not
replace an existing output or leave a partial new output. Input/output aliases,
including hardlinks and symlinks, cannot overwrite the authored source.

The existing `validate`, `normalize`, `lower`, `render`, and `explain` commands
continue to consume HIR. Pass them the emitted HIR file, rather than a
composition source. Existing HIR 0.1 and 0.2 files do not need this wrapper.

Inspect the machine-readable authoring contract with:

```sh
cargo run -p vizir-cli -- schema composition
cargo run -p vizir-cli -- schema composition --output out/composition.schema.json
```

The same generated contract is checked in at `schemas/composition.schema.json`.

## Source contract

The required root fields are `schema`, `id`, `width`, `height`, `layout`, and
`panels`. The schema discriminator is exactly `vizir-composition/0.1`.
`background` defaults to `transparent`, `title` is optional, and `datasets`
defaults to an empty map. Dataset keys, rows, stable identity, field encodings,
styles, and diagram references use the existing HIR semantics.

```yaml
schema: vizir-composition/0.1
id: two-panels
width: 1440
height: 600
layout:
  kind: grid
  columns: 2
  padding: 24
  gap: 24
panels:
  - kind: geometry.scene
    id: left
    children:
      - {type: rect, id: box, x: 10, y: 20, width: 120, height: 80}
  - kind: diagram.graph
    id: right
    nodes:
      - {id: start, label: Start}
      - {id: finish, label: Finish}
    edges:
      - {from: start, to: finish}
```

`panels` is a nonempty ordered array. A panel has the same fields and defaults
as its corresponding HIR view, except that `frame` is forbidden:

- `chart.scatter`: dataset, x/y encodings, optional color, default point size 7
- `chart.line`: dataset, x/y encodings, optional series, default line width 2.5,
  and points enabled by default
- `chart.bar`: dataset, category/value encodings, optional color
- `diagram.graph`: nodes, optional edges, default layered layout; manual layout
  retains view-local node positions
- `geometry.scene`: typed children in panel-local coordinates

All variants retain their stable `id` and optional `title`. Chart numeric axis
options follow [VizHIR 0.2's format contract](wire-format.md#numeric-axis-formats-in-02).
For example, `axis.number_format: {notation: fixed, precision: 2}` remains a
typed display option when the composition becomes HIR.

Unknown fields, unsupported panel kinds, and author-supplied panel frames are
rejected. Required typed fields cannot be replaced with `null`; omitted fields
with concrete defaults, such as grid gap or chart point size, cannot be set to
`null` either. Optional HIR fields retain their existing individual rules.
The generated schema describes the wire structure; `compose` additionally
checks allocation and ordinary HIR validation, including duplicate IDs and
missing data or diagram references.

## Deterministic allocation

For `N` panels and `C` columns, `1 <= C <= N` and the row count is
`R = ceil(N / C)`. Panels fill cells in source order, left to right and then
top to bottom. An incomplete final row leaves its remaining cells unused;
existing panels are not stretched to fill them.

`padding` is the same inset on all four canvas edges. `gap` is the same space
between adjacent columns and rows. Both default to zero and must be finite
and nonnegative. Canvas dimensions must be finite and positive. Cell sizes are:

```text
cell_width  = (width  - 2 * padding - (C - 1) * gap) / C
cell_height = (height - 2 * padding - (R - 1) * gap) / R
x = padding + column * (cell_width  + gap)
y = padding + row    * (cell_height + gap)
```

Every allocated frame must be finite, positive-sized, and inside the padded
canvas. Numeric overflow or an allocation that cannot satisfy these conditions
is rejected instead of publishing invalid HIR.

The 1440 by 600 mixed demo has two columns, padding 24, and gap 24. Its exact
frames are `(x: 24, y: 24, width: 684, height: 552)` and
`(x: 732, y: 24, width: 684, height: 552)`. Its scatter chart and layered graph
otherwise retain their ordinary HIR behavior. Repeated composition is
deterministic and does not mutate the source.

## Coordinate ownership and limits

Composition allocates frames only. It does not auto-fit panel contents, scale
children, or clip overflow to the cell. A chart still performs its own existing
axis/header layout and may reject a cell too small for its labels. Layered
diagrams still use the existing topology layout provider; a dense diagram may
need a larger cell or different authored content.

Geometry coordinates stay local. In Scene2D the panel group translates by the
allocated frame origin while its child rectangle coordinates stay unchanged.
Existing nested geometry-group transforms also stay unchanged. Manual diagram
positions stay local in HIR and are converted to document coordinates exactly
once by the existing diagram layout provider. Composition does not pre-offset
either kind of child.

This version has no nested composition layouts, weighted columns, spans,
responsive breakpoints, or content-based sizing. Nested geometry groups are
still supported as ordinary geometry; they are not nested composition grids.
If a design requires unequal or individually placed view frames, author an
explicit HIR document.

## Rust API

```rust
use vizir_core::{compose, composition_schema, parse_composition};
use vizir_compiler::compile;

let source = parse_composition("examples/composition/service-grid.compose.yaml")?;
let hir = compose(&source)?;
let compilation = compile(&hir)?;
let schema = composition_schema();
```

`CompositionV1` is the typed, serializable source model. `compose(&CompositionV1)`
returns `VizResult<Document>` and performs allocation plus HIR validation.
`parse_document` and `compile(&Document)` retain their existing HIR-only
contracts. Integration tests compare the composed document, MIR, and Scene2D
with equivalent explicit HIR, and cover all five panel kinds, defaults,
coordinate ownership, repeatability, and safe CLI publication failures.
