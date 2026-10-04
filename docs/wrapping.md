# Explicit geometry text wrapping

`vizir-text-wrap/1` adds opt-in wrapping to the measured-outline compiler path.
It applies only to explicitly named text nodes in `geometry.scene` views,
including text nested in geometry groups. It does not change HIR, MIR, Scene2D,
the legacy themed envelope, or output from compilation without this policy.
Charts, chart titles, axes, legends, diagram labels, and generated labels retain
the existing measured single-line behavior.

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

A policy must contain 1–256 targets. Each source ID contains 1–256 UTF-8 bytes.
`max_width` and `line_height` must be finite values in 0.25–1,000,000;
`max_lines` is an integer in 1–256. JSON Schema can bound ID character length,
while executable validation also enforces the UTF-8 byte limit and source-ID
resolution. It also checks duplicate target identities and actual text fitting.

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
an explicit source target. `TextLayoutContext::new(targets)` pins the profile and
Unicode engine identity. The public structs are non-exhaustive and retain
`PartialEq`; the existing `TextContext` keeps its `Eq` contract.

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
