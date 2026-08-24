# VizIR IR family

VizIR uses an IR family, not one universal visualization object model:

```text
authoring YAML / Rust API / Agent
              │
              ▼
       VizHIR semantic dialects
              │ validate / infer / normalize
              ▼
       VizMIR execution contract
              │ data / scale / layout resolution
              ▼
       Scene2D retained scene IR
              │ capability negotiation
              ▼
       backend-private emission plan
              │
           SVG / PNG

ScenePatch ── revisioned incremental Scene2D updates
Capability ─ machine-readable support and lowering decisions
```

`Scene2D` is the current 2D SIR. The name is deliberately dimension-specific:
a later `Scene3D` owns mesh, camera, material, and light semantics instead of
adding mostly-invalid fields to `SceneNode`.

## Ownership

| Contract | Owns | Does not own | Persistence |
| --- | --- | --- | --- |
| VizHIR | editable chart, diagram, and geometry intent; defaults | renderer routing, target objects | authoritative source |
| VizMIR | typed data sources and expressions, resolved references, coordinate spaces, scales, guides, layout requests, deterministic materialization | DOM, Canvas, SVG elements, interaction runtime | canonical interchange and inspectable build artifact |
| Scene2D | resolved 2D geometry, transforms, bounds, drawing order, provenance | chart intent, scale inference, graph layout | regenerable build artifact |
| Backend plan | target commands and target resource handles | portable semantics | ephemeral |
| ScenePatch | revision checks and atomic scene edits | dataflow or state updates not present in static 0.1 | transport contract |
| Capability | backend feature IDs, limits, unsupported features, and per-node decisions | renderer selection policy | discovery/report contract |

Natural-language intent and renderer routing remain owned by `create-plot`.

## Stable 0.1 contract

The first stable surface is intentionally narrower than the full VIR proposal:

- `VizMir` uses ID-keyed `data`, `expressions`, and `spaces` maps. `views` keeps
  explicit order. Its `version` is independent from `source_hir_version`.
- Inline datasets lower to typed `MirDataNode` sources with a stable key,
  inferred field schema, update mode, and determinism declaration.
- Chart channels reference canonical `TypedExpression` AST entries. Expressions
  are pure, closed enums and are re-type-checked during MIR validation.
- Spatial types carry both unit and coordinate-space identity. The current HIR
  numbers lower to explicit `scene-unit` spaces; there is no implicit conversion
  between view and world lengths.
- Geometry defaults are expanded while lowering to `MirGeometryNode`; HIR
  geometry is not reused as Scene2D.
- Every Scene2D origin records HIR ID, MIR ID, datum key when present, data
  lineage, and generating pass.
- Backends advertise namespaced feature IDs. Compilation produces an explicit
  decision for every feature required by every scene node; missing support is an
  error unless a future lowering pass deliberately selects a fallback.
- `ScenePatch` carries document, transaction, base revision, target revision,
  and typed operations. Applying a patch is atomic because it operates on a
  clone and publishes only the validated result.

The checked-in schemas are generated from the Rust types:

```text
schemas/viz-mir.schema.json
schemas/scene-patch.schema.json
schemas/capability.schema.json
```

Regenerate them with:

```bash
vizir schema mir --output schemas/viz-mir.schema.json
vizir schema scene-patch --output schemas/scene-patch.schema.json
vizir schema capability --output schemas/capability.schema.json
```

Tests reject schema drift.

## Deliberate compatibility seam

`MirView::{Chart, Diagram, Geometry}` and `ChartMark` remain a small static-2D
execution plan in 0.1. They are not declared to be universal visual primitives.
The current compiler has three chart forms and one Scene2D consumer; replacing
them now with an unproven `repeat + glyph + connector` algebra would merely move
the same cases behind generic names.

The promotion point is concrete: when dataflow transforms and a second retained
runtime need shared repeated-instance behavior, chart plans lower one step
further into a profile-neutral visual plan. At that point bar becomes
`repeat(rect template) + band/linear bindings`, graph nodes become
`repeat(node glyph)`, and line becomes ordered grouping plus path geometry.
That new plan must replace `ChartMark`, not coexist with it.

## Separate future dialects

The following do not enter static `SceneNode` or pretend to work through unused
fields:

- data/state/resource patches extend the Patch protocol only with an executing
  transactional runtime;
- signals, selections, event streams, and actions belong to an interaction
  dialect;
- clocks, timelines, transitions, and simulations belong to a time dialect;
- mesh, point cloud, material, camera, and light belong to `Scene3D`;
- glTF/GLB remains a managed 3D resource format, not a reimplemented binary
  format inside VizMIR.

Each promotion requires executable examples, normalized failure tests, explicit
capability behavior, and incremental/full semantic equivalence where applicable.
