# Explicit source-targeted text wrapping

`vizir-text-wrap/1` adds opt-in wrapping to the measured-outline compiler path.
It applies only to explicitly named text nodes in `geometry.scene` views,
including text nested in geometry groups. It does not change HIR, MIR, Scene2D,
the legacy themed envelope, or output from compilation without this policy.
Under v1, charts, chart titles, axes, legends, diagram labels, and generated
labels retain the existing measured single-line behavior. The separate v2
profile below opts supported chart titles into wrapping. V3 adds bar category
labels. Neither extension reinterprets or adds fields to the frozen v1 wire
shape, and v3 does not widen the published v2 chart-title contract.

## Policy and exact identities

Save an explicit policy as a JSON file:

```json
{
  "profile": "vizir-text-wrap/1",
  "engine": "unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)",
  "targets": [
    {
      "view_id": "labels",
      "node_id": "description",
      "max_width": 240,
      "max_lines": 6,
      "line_height": 32
    }
  ]
}
```

The policy identifies the source view ID and the source geometry text node ID.
Do not substitute a generated Scene2D path ID such as `labels/description` for
`node_id`. Targets must exist, be unique by `(view_id, node_id)`, and name text
inside geometry views. Missing, duplicate, non-text, chart, diagram, and unused
targets fail. The policy never selects nodes by matching their text content.

The exact profile and engine strings are required. Unicode line-break
opportunities use `unicode-linebreak` 0.1.5 with Unicode 15.0.0 data. Candidate
breaks must also be extended-grapheme boundaries according to
`unicode-segmentation` 1.13.3 with Unicode 17.0.0 data. These two data versions
are deliberately distinct and persisted, rather than described as an ambiguous
single “Unicode version.” Measured shaping retains its independent exact engine
and face identities in `context.text`.

Wrapping requires the existing measured text profile and exact explicit font
resources. `context.text_layout` supplements `context.text`; it cannot operate
with native SVG text or without the measured profile. Policy files contain no
font paths, resource URLs, fallback lists, or filesystem lookup instructions.
Unknown fields and duplicate JSON object keys at any depth are rejected.

## Semantic chart titles: `vizir-text-wrap/2`

V2 adds exactly one semantic role, `chart.title`, for the existing authored
`title` field on `chart.bar`, `chart.line`, and `chart.scatter` views. It does
not target chart categories, axes, legends, diagram labels, document titles,
geometry view titles, or arbitrary generated scene IDs. For example:

```json
{
  "profile": "vizir-text-wrap/2",
  "engine": "unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)",
  "targets": [],
  "semantic_targets": [
    {
      "view_id": "chart",
      "role": "chart.title",
      "max_width": 210,
      "max_lines": 12,
      "line_height": 32
    }
  ]
}
```

`view_id` is the original chart view ID. The role resolves its original title
field, never a matching string or a generated `chart/title` path name. An absent
title is an error; an explicitly present empty title is one valid empty logical
line. Each `(view_id, role)` must resolve exactly once. Missing views, unsupported
view kinds or roles, duplicate semantic targets, and unused targets fail.

V2 requires a present, non-null, nonempty `semantic_targets` array and a `targets`
array. `targets` can be empty or contain the same geometry targets supported by
v1. V1 rejects the `semantic_targets` field in every form, including `[]` and
`null`; omission alone retains the old contract. There is no implicit upgrade
based on a target's contents. Both profiles retain the exact same pinned
Unicode break engine, font identity requirements, source-byte preservation,
first-overflow greedy wrapping, grapheme safety, and failure behavior.

A wrapped chart title retains its existing 18px Bold face and first baseline at
`frame.y + 28`, with its start anchor at `frame.x + 18`. Subsequent baselines add
`line_height`; this value must cover the selected Bold face's actual scaled
ascent and descent. `max_width` bounds the union of advance and emitted ink,
including side bearings, rather than a count of characters. It is used exactly
as supplied, without being silently clamped to the frame. Both the wrapped
block and its actual ink must fit the chart and canvas; the existing header
width check also requires the title width to fit `frame.width - 36`.

The full title envelope includes face ascent/descent, actual ink, and all visual
lines, including blank and terminal empty lines. The initial header bottom is
`max(32.5, title_envelope_bottom - frame.y)`. Existing compact legends can remain
to the right only when every actual swatch and single-line label envelope fits
inside the frame and clears the complete title envelope by at least 8px.
Otherwise, source-order legend rows begin 8px below the reserved title block, using
measured ascent/descent, 16px horizontal gaps between entries, and 8px gaps
between rows. The final header bottom includes every title and legend footprint.

Without a y-axis title, the plot top is
`frame.y + max(header_bottom + 12, 50)`. With a y-axis title whose measured
logical/ink top relative to its baseline is `axis_top`, it is
`frame.y + max(header_bottom + 6 - axis_top + 12, 50)`. That title's baseline
remains `plot_top - 12`, and its descender must remain above the plot. At least
64px of plot width and height must remain. Existing numeric tick and minimum
plot-height checks run before final scale ranges are selected. Allocation
failure is a diagnostic, not permission to shrink the title, overlap text,
clip, or reflow persisted MIR. Untargeted titles retain the old layout branch.

The original UTF-8 title, including hard-break separators, remains unchanged in
HIR and MIR. The complete title outline retains the existing `view_id/title`
Scene2D identity, and its provenance includes original text and byte ranges for
lines and separators. SVG and PNG consume that same outline. There are no new
line IDs or renderer-native multiline text nodes.

The CLI flag, measured-text profile, and replay commands below are shared by
all versions. An equal repeated policy is accepted on replay; adding or
changing semantic targets, widths, line counts, or line heights requires
compiling the original HIR again. Refresh retains the persisted policy and
layout allocation.

## Bar category labels: `vizir-text-wrap/3`

V3 adds `bar.category_labels`, selected by the original `chart.bar` view ID:

```json
{
  "profile": "vizir-text-wrap/3",
  "engine": "unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)",
  "targets": [],
  "semantic_targets": [
    {
      "view_id": "sales",
      "role": "bar.category_labels",
      "max_width": 90,
      "max_lines": 12,
      "line_height": 16
    }
  ]
}
```

V3 requires a present, non-null, nonempty `semantic_targets` array containing
at least one `bar.category_labels` target. It can also include `chart.title`
targets and v1 geometry targets. V2 continues to accept only `chart.title`;
putting the new role into v2 fails in generic deserialization, validation, and
the compiled schema. Neither an existing constructor nor a role's contents
implicitly upgrades a policy. The published closed v1/v2 schema branches and
their referenced role/target definitions remain unchanged. V3 has a separate
closed branch with independently capped arrays; executable validation enforces
the combined 256-target cap without generating 256 schema branches.

One category target covers the complete, nonempty actual `Band` domain of its
source bar category binding, in its preserved order. It includes valid domain
values that currently have no rows. The unique bottom axis must reference that
same category binding. Missing/duplicate axes, mismatched bindings, invalid or
empty domains, wrong view kinds, and unresolved targets fail. Resolution uses
source semantics; generated IDs and string matching do not select labels.
The source field, domain order, row keys, and existing category node IDs remain
unchanged. Line breaks become legal only at that selected category use. The
same field used as a color legend or in another untargeted view retains the
single-line contract and rejects hard breaks.

Every targeted label retains a middle anchor at its existing band-cell center,
with the exact Regular face at 10px regardless of category count. Every line's
actual advance-plus-ink envelope must fit both the explicit `max_width` and its
own cell with 4px clearance on each side. The compiler does not silently clamp
width, shrink fonts, drop labels, rotate them, or iteratively retry layout.


For `bar.category_labels`, `max_width` is a maximum, not a promised line width.
Legal breakpoints are selected within both the authored maximum and the usable
Band cell after its 4px side gaps. Thus a maximum larger than the cell still
permits ordinary available-space wrapping; a smaller maximum wraps earlier.
The persisted authored number is never clamped or rewritten. Actual projected
advance-plus-ink intervals must also fit both sides of each cell, including
asymmetric bearings under the Middle anchor. An unbreakable segment that cannot
fit either constraint fails; the compiler never shrinks, truncates, or splits
it at an emergency glyph boundary.

Header and numeric-axis allocation establish the horizontal plot range first.
The full category blocks then reserve bottom space before final vertical
ranges are selected. Blank and terminal lines count in this reservation.
Category blocks have an 8px gap below the plot and an 8px gap above the actual
logical/ink top of the existing x-axis title envelope, including an empty
logical title. The first category baseline uses measured maximum ascent, and
the existing 62px bottom allocation remains a floor. Font metrics and complete
emitted ink participate in allocation;
`line_height` must cover the Regular face's scaled ascent and descent. At least
64px of plot width and height must remain, and existing numeric tick, frame,
and canvas checks still apply.

Let `T` and `B` be the minimum top and maximum bottom of the complete raw
category blocks relative to their first baseline, and let `H = B - T`.
Let `A` be the x-axis title's actual logical/ink top at its unchanged baseline
`frame.y + frame.height - 16`, and let `C = 0.0001`.
The final plot bottom is
`min(frame.y + frame.height - 62, A - 8 - H - 8 - 2*C)`.
The first category baseline is `plot_bottom + 8 - T + C`.

Final lines are reprojected from their original shaped runs into the allocated
coordinates. The 0.0001px vertical serialized-coordinate clearance protects
each boundary projection, followed by exact final fit and collision checks.
This is not a fitting tolerance or permission for overlap. SVG and PNG consume the
same complete outline under each existing category ID; provenance retains the
original UTF-8 source, visual-line and separator byte ranges, and baselines.
The first-overflow greedy rule, grapheme boundaries, engine identity, and all
other wrapping limits below are shared with v1/v2.

Replay and materialization refresh retain the persisted category policy and
scale ranges. A refreshed complete domain must still fit those ranges; refresh
cannot reflow or enlarge the plot. Adding targets, changing the policy or
requesting new layout allocation requires the original HIR.

## CLI and durable replay

With a measured-text profile and each distinct font resource hash prepared as
shown in [compiled-context.md](compiled-context.md):

```sh
vizir normalize source.viz.yaml \
  --text-profile text-profile.json --text-layout text-layout.json \
  --font "$REGULAR_SHA=fonts/regular.ttf" \
  --font "$MEDIUM_SHA=fonts/medium.ttf" \
  --font "$BOLD_SHA=fonts/bold.ttf" \
  -o wrapped.mir.json

vizir render wrapped.mir.json --format svg \
  --font "$REGULAR_SHA=fonts/regular.ttf" \
  --font "$MEDIUM_SHA=fonts/medium.ttf" \
  --font "$BOLD_SHA=fonts/bold.ttf" \
  -o wrapped.svg --manifest wrapped.render.json
```

`validate`, `normalize`, `lower`, `render`, and `explain` all accept
`--text-layout PATH`. The flag takes one explicit strict JSON file; it is not a
path persisted into the compiled artifact. An optional `--theme` works with the
same combined context. `compose` remains a separate source step producing HIR.

Normalization from HIR persists the policy as optional `context.text_layout`
in `vizir-compiled-mir/1`, alongside measured text and any theme. Replay uses
that persisted policy without reading the original policy file. The exact font
bytes must still be supplied explicitly. A repeated `--text-layout` is accepted
only when its decoded policy is equal to the persisted policy, including target
order and all numeric values. Adding or changing a policy on persisted MIR
fails: compile the original HIR instead. This also applies to theme-only and
measured-but-unwrapped envelopes. Refreshing materialized data does not grant
permission to change the persisted layout policy.

Absent or JSON `null` layout context means no wrapping; canonical serialization
omits it. The compiled schema grows additively. The existing themed-MIR, raw MIR,
HIR, and Scene2D contracts are unchanged. Render manifests retain the complete
compilation context and the existing outline-text fidelity losses.

## Text and break behavior

This profile supports horizontal left-to-right Latin and CJK text. It rejects
RTL content and directional controls, tabs, and unsupported control characters.
It does not introduce bidi layout, vertical text, font fallback, hyphenation,
automatic font shrinking, ellipsis, or emergency splitting of an unbreakable
span. Soft hyphen U+00AD is explicitly rejected with its source UTF-8 byte index:
this no-hyphenation profile cannot preserve its discretionary-hyphen semantics
and will not silently render it invisible. Missing glyphs retain the
measured-text diagnostic behavior.

LF, CRLF, Unicode line separator U+2028, and paragraph separator U+2029 are
explicit hard breaks. Bare CR is rejected. Consecutive separators preserve blank
lines; a trailing separator preserves a final blank line. Whitespace is never
trimmed or collapsed. Original UTF-8 text and hard-break bytes remain unchanged
in HIR/MIR and are accounted for in source ranges; there is no normalization of
combining sequences or line endings. Hard-break separators affect layout and
are not shaped as glyphs.

Within each hard-break paragraph, wrapping uses legal Unicode line-break
opportunities intersected with extended-grapheme boundaries. It tests candidate
ends in source order and chooses the last fitting candidate before the first
overflow. This deterministic first-overflow greedy rule does not assume that
all independently shaped prefix widths are monotone or seek a later narrower
candidate after an overflow. Each final line is shaped independently with the
same pinned font identity; there is no shaping through a chosen line break. A span that cannot fit at any supported break is
an error rather than permission to split a grapheme or silently overflow.

## Geometry, metrics, and provenance

Each line retains the authored text anchor (`start`, `middle`, or `end`) and its
advance-origin interpretation at the original `x`. `max_width` bounds the
horizontal union of the line's advance interval and actual glyph ink, including
negative side bearings. Candidate fits and final line fits are checked after
four-decimal outline-coordinate projection, using emitted geometry rather than
an unrounded or advance-only approximation.

The first baseline remains the authored `y`. Subsequent baselines are
`y + line_index * line_height`. `line_height` must be at least the selected
face's scaled ascent minus descent, with finite ordered ascent ≥ 0 and
descent ≤ 0. It is not a font-size multiplier or a request to stretch glyphs.
For example, Noto CJK's face metrics can require about 1.45 times the font size;
use a value that covers the actual selected face. Adjacent lines' actual ink
must also remain vertically non-overlapping.

The complete logical block uses the face ascent/descent and all visual lines,
including blank and terminal empty lines. Its bounds are checked under the
serialized transforms against the view and canvas. Actual outline ink is
checked too. Text that cannot satisfy these constraints fails rather than
moving the anchor, clipping, shrinking, or spilling outside its frame.

One existing Scene2D `Path` carries the complete wrapped outline under the
original semantic node ID. Its path bounds cover actual ink; logical blank-line
space is validated separately. The original provenance explanation is retained
and extended with original text bytes, line byte ranges, separator byte ranges,
and baselines. No generated line IDs are substituted for the stable source
identity. SVG and PNG consume the same resolved outline geometry, retaining the
existing loss of selectable/searchable/editable SVG text.

## Limits and failure behavior

A policy must contain 1–256 targets in total across geometry and semantic
arrays. V1 requires at least one geometry target; v2 requires at least one
semantic target, while v3 requires at least one bar-category target. Each source ID contains 1–256 UTF-8 bytes.
`max_width` and `line_height` must be finite values in 0.25–1,000,000;
`max_lines` is an integer in 1–256. JSON Schema can bound ID character length,
and independently bound each target array; executable validation additionally
enforces the combined 256-target cap, UTF-8 byte limit, and source-ID resolution.
It also checks duplicate target identities and actual text fitting.

The default `TextLimits.max_layout_lines` and `max_wrap_candidates` cap whole-call
visual lines and candidate shaping attempts at 4,096 each. Existing measured-text budgets independently cap input label bytes,
aggregate text work, glyphs, outline commands, cached outlines, collision checks,
and serialized output. Candidate shaping consumes the same whole-call budgets,
so unsuccessfully tested breaks are not unmetered work. Exceeding any limit is
an error, with no partial successful scene or output artifact.

Layout JSON is limited to 32 MiB before parsing, using a bounded regular-file
read. The same explicit-path, symlink/hard-link alias, and nonblocking FIFO
safeguards used for profiles and fonts apply to layout files. Layout files are
protected from overwrite by output or manifest destinations. No policy content
authorizes implicit filesystem or network access. Existing staged transactional
publication preserves previous outputs when compilation fails. These limits
bound specific input and compilation work; they are not a general process-memory
or filesystem-race isolation guarantee.

## Rust API

`TextLayoutTarget::new(view_id, node_id, max_width, max_lines, line_height)` builds
an explicit geometry source target. `TextLayoutContext::new(targets)` continues
to pin v1 and the Unicode engine, with `semantic_targets: None` omitted from
serialization. `TEXT_LAYOUT_PROFILE` remains `vizir-text-wrap/1`.

`SemanticTextLayoutTarget::chart_title(view_id, max_width, max_lines, line_height)`
constructs a semantic target with `TextLayoutRole::ChartTitle`.
`TextLayoutContext::new(vec![]).with_semantic_targets(targets)` explicitly pins
`TEXT_LAYOUT_SEMANTIC_PROFILE` (`vizir-text-wrap/2`); using this builder on a
geometry policy retains its geometry targets. `semantic_targets` is
`Option<Vec<SemanticTextLayoutTarget>>`; only `None` is omitted, and a present
JSON null is rejected. The public structs and role enum are non-exhaustive and
retain `PartialEq`; the existing `TextContext` keeps its `Eq` contract.

`SemanticTextLayoutTarget::bar_category_labels(view_id, max_width, max_lines, line_height)`
constructs a target with `TextLayoutRole::BarCategoryLabels` (wire spelling
`bar.category_labels`). `TextLayoutContext::new(vec![]).with_category_labels(targets)`
explicitly selects `TEXT_LAYOUT_CATEGORY_PROFILE` (`vizir-text-wrap/3`) and
retains any geometry targets. The supplied vector can mix category and title
roles but must contain a category role. `with_semantic_targets` always selects
v2, even when called on a v3 context, and category roles then fail validation.

Add the policy with
`CompilationContext::new().with_text(text).with_text_layout(layout)`, then use
the existing context compilation and replay APIs. `parse_text_layout_context_json`
provides the bounded strict JSON reader. Generic serde callers must supply their
own byte-input cap and use compiler APIs for executable validation. Fonts and
paths remain external resources.

## Executable three-panel composition

The checked-in example uses the separately renamed, OFL-licensed wrapping test
fonts. Their deliberately limited coverage is listed in the fixture manifest;
ordinary user documents should supply their own exact licensed faces. These
commands run from the repository root with the built `vizir` on PATH and a
supported native PNG renderer available:

```sh
mkdir -p /tmp/vizir-wrapping
REGULAR='fac61120f81f6bcd298dfa2f1f5c83860dd15112b27ef529ec99aa13800137c2=crates/vizir-compiler/tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Regular.otf'
MEDIUM='2296e72a0fae6aba328f0a460f4c18fcf41b805113f8facfafab2bdefda935bc=crates/vizir-compiler/tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Medium.otf'
BOLD='406a4747bd8a46debd27422748fbd931bcdcd868cf6aef6f5fa50049ca114fd0=crates/vizir-compiler/tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Bold.otf'
vizir compose examples/composition/wrapped-text.compose.yaml -o /tmp/vizir-wrapping/source.json
vizir normalize /tmp/vizir-wrapping/source.json --theme azure \
  --text-profile examples/text/wrapping-font-profile.json \
  --text-layout examples/text/wrapped-text-layout.json \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-wrapping/compiled.json
vizir render /tmp/vizir-wrapping/compiled.json --format svg \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-wrapping/wrapped.svg
vizir render /tmp/vizir-wrapping/compiled.json --format png --background transparent \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-wrapping/wrapped.png
vizir explain /tmp/vizir-wrapping/compiled.json --node chinese/body \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD"
```

This demonstrates three explicit source targets, narrow Latin and Chinese
paragraphs, ligatures and combining marks, real Chinese punctuation, blank and
terminal lines, all three exact weights, a centered anchor, and transparent
raster output. Source strings and their source byte ranges remain inspectable
in the compiled artifact and explanations.

## Executable narrow chart-title dashboard

[wrapped-chart-titles.compose.yaml](../examples/composition/wrapped-chart-titles.compose.yaml)
uses the same exact wrapping fixture fonts as the geometry example above. The
[chart-title policy](../examples/text/chart-title-layout.json) selects three
source views by role, demonstrating bar/line/scatter, CJK punctuation, Latin
ligatures and accents, blank/terminal title lines, a compact side legend and
legends moved below complete title blocks. With the REGULAR, MEDIUM and BOLD
explicit font mappings from the geometry example:

```sh
mkdir -p /tmp/vizir-chart-titles
vizir compose examples/composition/wrapped-chart-titles.compose.yaml \
  -o /tmp/vizir-chart-titles/source.json
vizir normalize /tmp/vizir-chart-titles/source.json --theme azure \
  --text-profile examples/text/wrapping-font-profile.json \
  --text-layout examples/text/chart-title-layout.json \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-chart-titles/compiled.json
vizir render /tmp/vizir-chart-titles/compiled.json --format svg \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-chart-titles/titles.svg
vizir render /tmp/vizir-chart-titles/compiled.json --format png --background transparent \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-chart-titles/titles.png
```

## Executable category dashboard

[wrapped-bar-categories.compose.yaml](../examples/composition/wrapped-bar-categories.compose.yaml)
uses [bar-category-layout.json](../examples/text/bar-category-layout.json) and the
same supplied wrapping font profile as the examples above. It shows ten
categories at fixed10px, separate title blocks, Chinese punctuation, Latin
ligatures/accents, retained whitespace and blank/terminal category lines.
With the REGULAR, MEDIUM and BOLD explicit font mappings defined above:

```sh
mkdir -p /tmp/vizir-bar-categories
vizir compose examples/composition/wrapped-bar-categories.compose.yaml \
  -o /tmp/vizir-bar-categories/source.json
vizir normalize /tmp/vizir-bar-categories/source.json --theme azure \
  --text-profile examples/text/wrapping-font-profile.json \
  --text-layout examples/text/bar-category-layout.json \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-bar-categories/compiled.json
vizir render /tmp/vizir-bar-categories/compiled.json --format svg \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-bar-categories/categories.svg
vizir render /tmp/vizir-bar-categories/compiled.json --format png --background transparent \
  --font "$REGULAR" --font "$MEDIUM" --font "$BOLD" \
  -o /tmp/vizir-bar-categories/categories.png
```
