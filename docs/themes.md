# Canonical theme compilation context

Theme selection is explicit. Existing HIR 0.1/0.2, MIR, Scene2D, Rust public
struct literals and APIs keep their legacy defaults and wire format. With no
`--theme`, the CLI keeps the same MIR, scene and SVG bytes. There is no inferred
host mode and no theme injection into composition or request cards.

## Connected CLI workflow

```sh
vizir themes
vizir normalize examples/mixed/theme-defaults.viz.yaml --theme sage-dark -o themed.mir.json
vizir validate themed.mir.json
vizir lower themed.mir.json -o themed.scene.json
vizir render themed.mir.json --format svg -o themed.svg
vizir render themed.mir.json --format png -o themed.png --manifest themed.render.json
vizir explain themed.mir.json --node health/point/gateway
```

`normalize`, `lower`, `render`, `validate` and `explain` also accept `--theme`
with ordinary HIR inputs. Exact names are `azure`, `mist-blue`, `sage`,
`stone-teal`, `dusty-violet`, `warm-sand`, `olive-paper`, and each `-dark` variant.
Aliases, case changes and a separate dark switch are not accepted. No theme is
selected by default. Selecting a dark theme asserts a dark host background;
transparent output cannot automatically adapt to its eventual host.

Normalize with a theme writes a JSON durable compilation-context envelope:

```json
{
  "format": "vizir-themed-mir/1",
  "theme": {
    "name": "sage-dark",
    "registry_spec": "diagram-theme/v1",
    "registry_version": "1.0.0",
    "registry_revision": "1cc4e6667aa86444a7e9055549aa85cac293dc07",
    "defaults": {}
  },
  "mir": {}
}
```

The example elides required defaults and inner MIR. The executable output
contains the full resolved fields. Use a `.json` filename for persisted
context; the CLI's YAML input contract remains source HIR. `schema themed-mir`
emits the separate context schema, including the fourteen exact theme contexts.
`schema mir` continues to describe only the unchanged legacy MIR.

The envelope is one shared compiler's durable context, not a second renderer
or parallel IR implementation. It avoids adding required Rust struct fields to
`Document`, `VizMir` or `Scene2D`. Its inner MIR retains the matching source HIR
version, data, expressions, stable IDs and explicit scale ranges. Never extract
the inner MIR and claim the theme is retained: legacy `build_scene` deliberately
has legacy defaults and does not infer themes from colors.

### Edit data and refresh

HIR remains the primary editable source. An intentional MIR data/expression
edit uses the same existing command to refresh materialized caches:

```sh
# After editing authoritative data/expressions in themed.mir.json:
vizir normalize themed.mir.json -o refreshed.mir.json
vizir render refreshed.mir.json --format svg -o refreshed.svg
```

`validate`, `lower`, `render` and `explain` reject stale caches; they do not
silently refresh them. `normalize` on context explicitly rematerializes and
checks the resulting scene. Theme identity/defaults, authored styles and
palettes, explicit scales, versions and other plan fields survive unchanged.
Neither data changes nor theme choice re-infers explicit MIR domains.
A conflicting `--theme` on persisted context is an error. The same theme is
accepted as an assertion. To change theme, normalize the HIR again with the new
name. Keep the selected name alongside the source build command; HIR itself
has not acquired a hidden theme field.

## Defaults and authorship

The single source is the immutable pinned `diagram-theme-rs` dependency above.
A context's name, registry spec/version/revision and every resolved default
must exactly match that pin before execution or refresh. A modified, unknown
or future context fails; upgrades cannot silently recolor old artifacts.

- Narrative text and geometry default strokes use `ink`; ticks, legends and
  connector labels use `muted`; guide rules use `grid`
- Unencoded chart marks use `s1`; omitted/empty categorical palettes use the
  exact `s1` through `s8` sequence, cycling in existing stable category order
- Scatter outlines and line point interiors use `paper` as semantic knockouts
- Ungrouped diagram nodes use `panel`/`line`; grouped node fills use the shared
  `soften(sN, panel)` derivation, retaining category distinctions; these are
  semantic node fills, not automatic canvas/card backgrounds
- Connector defaults use `edge`; arrow triangles inherit the resolved connector
  stroke color, width and opacity, including authored transparent strokes

Authorship is represented by existing `Option<Color>` fields: only `None`
gets a default. An explicit color wins even if it equals an old default,
`paper`, `panel` or `transparent`. Nonempty authored chart palettes win in
full. Explicit numeric geometry, style opacity and widths are retained.
No value-based recoloring or post-emission SVG mutation is performed.

The source root background is never changed by theme selection. Existing
`render --background` is still an explicit root-only override. Semantic node
fills and point interiors remain visible; theme selection is not a blanket
transparency operation.

### Arrows and limits

For themed graphs, the existing cubic connector route resolves one three-vertex
closed arrow triangle into ordinary Scene2D path geometry, before target
emission. It uses the existing 7×7 marker/stroke-width geometry with a 10-unit
viewBox and refX=9. The original connector keeps its stable ID; the additional
path has the deterministic `/arrow` suffix and edge provenance. No new scene
node kind, SVG-only theme state or recursive geometry expansion is added.
Legacy graph output retains the original SVG marker unchanged.

Each arrow contributes exactly four path commands. Before normalization, scene
execution or refresh, these commands are reserved from the existing global
expression-node/evaluation-work budgets; the remaining budget is shared with
ordinary MIR execution. Finite tangents and resulting coordinates are checked before returning
a scene. Degenerate or overflowing arrow routes fail explicitly.
The themed HIR entrypoint also preflights geometry depth/node counts before
expanding defaults. The existing materializer still bounds recursive
expressions, schemas, values and geometry before execution/cloning.
The CLI requires regular JSON input files and imposes a 32 MiB input bound before
parsing, including legacy JSON input. Unix opens are nonblocking and recheck the
opened file type, so a FIFO cannot wait indefinitely for a writer. These are
intentional new input restrictions, not a blanket input-acceptance compatibility
claim. serde_json's recursion limit remains enabled.
These are parsing/static-work safeguards, not a claim of a universal process
memory or graph-layout time limit.

## Additive Rust APIs

`lower_to_themed_mir`, `compile_with_theme`, `build_themed_scene` and
`rematerialize_themed_mir` share the existing lowering, materializer and scene
builder. Build/refresh variants with `MaterializationLimits` are also exposed.
`ThemeContext::resolve` and `THEME_NAMES` supply the canonical selector.
All previous functions and public struct literals remain valid. The resulting
Scene2D has fully resolved colors/arrow geometry and works with the existing
SVG backend and native PNG rasterization path.

## Strict envelope decoding

`parse_themed_mir_json` is the bounded Rust byte-reader used by the CLI. It checks
the 32 MiB cap, duplicate keys at every JSON depth, fields discarded by legacy
nested MIR enum decoders, and pinned context identity/default consistency. The
new schema likewise closes declared record/enum properties without closing
arbitrary inline data metadata maps. Optional coordinate-space `parent: null`,
defaultable styles, and integral numeric spellings such as precision `2.0`
remain valid. Raw rows keep supported 64-bit integer precision, signed zero and representable
subnormal values. This does not preserve original numeric spellings or provide
arbitrary precision: float spellings retain serde_json's existing f64 rounding
and underflow behavior, while overflowing/nonfinite JSON numbers are rejected.
For example, `1e-400` decodes to `0.0`, and `9007199254740993.0` rounds to
`9007199254740992.0`; the integer spelling `9007199254740993` remains exact.

`ThemedMir` deserialization itself also rejects duplicate/discarded fields;
callers using a generic serde reader must impose their own byte-input bound.
Context identity and executable MIR semantics are still checked by the byte
reader/build/refresh APIs as appropriate. Parsing does not execute materialization
or repair stale caches, and no parsing or file-open check promises a universal
I/O deadline or process-memory bound. Legacy MIR deserializers are unchanged.
