# Native shared categorical legends

VizHIR/MIR 0.8 and `vizir-composition/0.7` add one optional root
`shared_legend`. It owns the categorical color legend of 2–64 explicitly named
chart panels. The first version supports a fixed bottom strip.

```yaml
schema: vizir-composition/0.7
# ... document size, grid, datasets and panels ...
shared_legend:
  id: series-key
  members: [all-series, beta-only]
  placement: bottom
  height: 64
  gap: 12
  title: Series
```

## Membership and colors

- Members are distinct existing panel IDs. Supported encodings are line/area
  `series` and scatter/bar `color`
- Every member must explicitly author the same ordered string `domain`. Inferred
  domains are not eligible, even if their current observations happen to agree
- Each member must resolve the same color for every domain key under the active
  theme or explicit palette. Equal palette spelling is not required; equal
  resolved colors are. Color equality includes globally absent domain keys
- Domains retain exact strings and order. There is no automatic union, sorting,
  trimming, label rewriting, or color reconciliation
- The owner emits the complete domain once. Categories absent from one panel,
  or from all panels, create only legend entries. They never create observations
  or plotted marks
- Only members lose their local legends. Nonmembers retain ordinary local
  legends and their own categorical semantics
- A shared legend describes categorical colors across chart types, not mark
  geometry. It does not share axes, infer common units, align plot ranges, or
  introduce panel cards

## Fixed geometry

The composition helper reserves the bottom strip and its gap before allocating
the panel grid. Height must be finite and positive; gap must be finite and
nonnegative. The grid must still have usable dimensions. The optional title
contains 1–16,384 UTF-8 bytes without controls or line separators. The owner ID
uses the existing stable-ID syntax and must differ from every panel ID. Unknown
fields and unsupported placements are rejected.

For the [1120×480 example](../examples/composition/shared-legend-grid.compose.yaml),
24px outer padding leaves a 1072px-wide content area. A 64px legend strip and
12px gap leave a 356px-high grid. The exact frames are:

- Shared legend: `{x: 24, y: 392, width: 1072, height: 64}`
- `all-series`: `{x: 24, y: 24, width: 524, height: 356}`
- `beta-only`: `{x: 572, y: 24, width: 524, height: 356}`

Legend entries pack in authored order into complete horizontal rows. They do
not shrink, truncate, or wrap individual labels. Text and title must fit the
authored strip; an undersized strip fails rather than silently dropping entries
or changing panel dimensions. Measured text uses the pinned font resources and
continues to enforce glyph and layout checks. The fixed style uses 8px padding,
10px square swatches, a 6px swatch-label gap, 20px between entries, and 6px
between rows. Labels are 10.5px regular and the title is 12.5px medium, with a
6px gap below it. Row height includes the measured logical and ink envelopes.

Native HIR stores the resolved owner frame and member view IDs. MIR stores the
frame plus explicit member view/ordinal-color-scale references. The compiler
checks those references and color agreement before lowering or replaying.
Every panel, including nonmembers, must end above the owner frame in HIR/MIR;
the frame must fit the canvas. The layout does not widen the Scene2D node
vocabulary. The owner group ID is `shared-legend:<UTF-8-byte-length>:<id>`,
with `/title` and `/entry/<index>/swatch` or `/label` children. Its provenance
names the owner and retains the distinct member data-source lineage.

## CLI and replay

```sh
vizir compose examples/composition/shared-legend-grid.compose.yaml -o shared.viz.json
vizir validate shared.viz.json
vizir normalize shared.viz.json -o shared.mir.json
vizir render shared.viz.json --format svg -o shared.svg
vizir render shared.viz.json --format png --background transparent -o shared.png
```

The example compares Alpha+Beta with Beta alone; `Reserved` is absent globally.
Beta retains its color in both panels. Only the shared legend contains all three
keys. A real PNG renderer is required for PNG output.

With `--text-profile` and explicit `--font SHA256=PATH` resources, normalization
emits the existing compiled-MIR envelope. Copy that envelope and the exact
font bytes to another directory and replay with mappings to their new paths:

```sh
vizir normalize shared.viz.json --text-profile profile.json \
  --font SHA256=font.otf -o shared.compiled.json
vizir render shared.compiled.json --font SHA256=/new/location/font.otf \
  --format svg -o replay.svg
```

Supply a mapping for every font hash required by the profile. The envelope
contains resource hashes, not local font paths. Replay verifies resources and
retains shared-legend semantics. Bare normalized MIR remains a Rust API input;
CLI replay uses an existing compiled/themed envelope.

## Closed compatibility boundary

- Only composition 0.7 and matching HIR/MIR 0.8 support `shared_legend`
- Older versions reject the field; explicit null is not an omission
- Omitting it preserves existing local legends and layout behavior, including
  the output shape of old versions
- The new owner object is closed: no alternate placements, arbitrary per-entry
  styling, continuous heatmap legends, multiple owners, or automatic membership
- Published provider v1/v2/v3 descriptors, receipt schemas, and acceptance
  profiles are unchanged. Provider v3 remains MIR 0.7; shared legends are a
  native CLI/compiler feature
- Direct Rust root struct literals gain `shared_legend: None` when not used

Invalid authoring, incompatible colors, broken replay references, or layout
failure is diagnosed before publishing or replacing an output artifact.
