# Explicit uniform numeric plot alignment

Use composition `vizir-composition/0.8` to emit matching HIR/MIR `0.9`.
An optional root group aligns the *plot rectangles* of explicitly named equal-cell
line, scatter, and area charts:

```yaml
plot_alignment:
  id: comparison-plots
  mode: uniform
  members: [trend, measurements, area]
```

The group requires 2–64 distinct member IDs. It is absent by default; explicit
null, unknown fields/modes, numeric or boolean IDs, unsupported members, and
version downgrades are rejected. A document has at most one group. Every member
must have a finite, positive, equal-sized cell, a bottom numeric x axis, and a
left numeric y axis. Member cells must not overlap. Equality permits only four
floating-point rounding units relative to the relevant extents and origins, covering anchored grid
edge arithmetic; it is not a visual pixel tolerance.

## What is aligned

Composition first reserves any shared legend strip, then allocates its ordinary
grid cells. Each member's existing chart title, explicit title-wrapping policy,
local legend (unless it is shared-owned), axis titles, and numeric tick labels
establish required left, top, right, and bottom insets. The compiler takes the
maximum on each side and applies those four insets to every member. Same-row
plots share top/bottom edges; same-column plots share left/right edges. Equal
cell dimensions have equal plot spans.

After combining insets, the compiler checks minimum 64×64 plotting room, numeric
tick separation and its stronger height/width requirements, and x-axis-title
fit around the new plot center. A set of individually fitting charts can fail
this final check. Failure is explicit: labels are never hidden, clipped, shrunk,
truncated, or automatically wrapped to make alignment fit. Enlarge every cell,
shorten the authored labels, or choose a more compact number format.

Headers and local legend entries retain their individual allocations. The shared
legend's frame and entries are unchanged. Alignment membership and shared-legend
membership are independent; one group can mix local and shared legend charts.
Panels outside the group retain their original frames, layout, scales, and marks.

Alignment opts members into numeric tick measurement even without authored
formats or domains, using the same conservative default text estimates or the
persisted measured-font context. A `0.9` document without a group retains ordinary
layout behavior.

## Domains are independent

Alignment does not infer, union, or change numeric or categorical domains, color
palettes, data, baselines, or formatting. Equal numeric domains plus equal plot
spans give equal pixel-per-unit mappings. Different domains remain valid and give
aligned geometry only; they do not imply comparable units or pixel scaling.
Author explicit matching domains when that is the intended comparison.

## Portable semantic intent and replay

HIR retains the authored group. MIR retains its ID and mode with explicit member
references to the actual view and bound x/y scales:

```json
{"id":"comparison-plots","mode":"uniform","members":[
  {"view":"trend","x_scale":"trend/x","y_scale":"trend/y"},
  {"view":"area","x_scale":"area/x","y_scale":"area/y"}
]}
```

These are opaque references, not naming conventions. Each aligned chart uses an
identity, unparented document coordinate space in scene units; both bound numeric
scales reference that same space. Transformed or mismatched spaces are rejected.
The x/y linear ranges are
resolved results; there is no second authoritative plot-rectangle cache. MIR,
themed MIR, and compiled MIR replay recompute local requirements and the common
solution under the saved context, then verify stored ranges before building axes,
grids, or marks. Stale or forged ranges are rejected rather than repaired.
`rematerialize_mir` and compiled equivalents refresh data-coordinate mark caches;
they preserve group intent and explicit ranges. Intentionally changing layout
requires normalization again from HIR.

Measured-text replay needs the exact font bytes identified by the persisted
context. Copying those resources to new paths is supported. Reference traversal,
additional layout work, text shaping, and output remain bounded.

## Version and feature boundary

Published composition 0.1–0.7 and HIR/MIR 0.1–0.8 schema branches and outputs are
unchanged. Themed and compiled envelope formats remain `/1`. Provider V1–V3,
request-card profiles, and external provider routes are unchanged; use the native
composition/normalize/render commands for this feature.

Bars and heatmaps are excluded from the group because their categorical labels
can depend on the final plotting width. Shared axes, log scales, category
alignment, automatic domain unions, and automatic wrapping are separate work.
Unaligned bar/heatmap/diagram/geometry panels can coexist in the document.

Executable example: [aligned numeric grid](../examples/composition/aligned-numeric-grid.compose.yaml)

```sh
vizir compose examples/composition/aligned-numeric-grid.compose.yaml -o aligned.viz.json
vizir normalize aligned.viz.json -o aligned.mir.json
vizir render aligned.viz.json --format svg -o aligned.svg
vizir render aligned.viz.json --format png --background transparent -o aligned.png
```
