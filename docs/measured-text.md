# Opt-in measured single-line text

The existing native-text output remains the default for VizHIR/MIR 0.1 and 0.2,
including legacy `vizir-themed-mir/1` artifacts. Measured text is selected explicitly
through a [compilation context](compiled-context.md). No new text primitive or
parallel domain IR is introduced.

## Reproducible CLI/composition example

The repository includes approximately 200 KiB of renamed, licensed test fonts.
They have deliberately limited character coverage and are not a general font pack.
This example needs no system fonts, font installation, Python or network:

```sh
vizir compose examples/composition/measured-text.compose.yaml --output dashboard.viz.json
fonts=crates/vizir-compiler/tests/fixtures/fonts
regular=101ae6935d6af30a20a3a2bd67b5b01bef33e166e8426328f1623a001a843a8a
medium=3ea61821b5e90206f77117282166e7c31a47edce94dffadee657b4038b0a3681
bold=ed45dae3cb7e0984c405d027c5071301549f77c0bbff28cc0a0826f3445188b0
vizir normalize dashboard.viz.json --theme azure \
  --text-profile examples/text/measured-font-profile.json \
  --font "$regular=$fonts/VizIRFixtureSC-Regular.otf" \
  --font "$medium=$fonts/VizIRFixtureSC-Medium.otf" \
  --font "$bold=$fonts/VizIRFixtureSC-Bold.otf" --output dashboard.compiled.json
vizir render dashboard.compiled.json --format svg --output dashboard.svg \
  --font "$regular=$fonts/VizIRFixtureSC-Regular.otf" \
  --font "$medium=$fonts/VizIRFixtureSC-Medium.otf" \
  --font "$bold=$fonts/VizIRFixtureSC-Bold.otf" --manifest dashboard.render.json
```

The same profile/resource arguments apply to `validate`, `lower`, `render` and
`explain`. For PNG use `--format png --background transparent`; the existing native
SVG rasterizer and alpha-verification requirements apply. `lower` emits the normal
Scene2D contract. Its Paths and the SVG/PNG do not require fonts afterward. Replaying
the saved compiled MIR does require the exact resource bytes again.

## Profile and resource ownership

The strict JSON profile identifies `vizir-text-outlines/1`, the implementation pin,
`declared_locale`, `shaping_language: "default"`, and three exact faces. Each face
contains a lowercase SHA256, collection face index and actual weight (Regular 400,
Medium 500, Bold 700). The constructors are non-exhaustive public Rust APIs.

`declared_locale` records content/font-locale provenance. It is **not** an active
OpenType language selector: this bounded profile uses the library's fixed default
shaping language. Exact regional font faces supply the demonstrated CJK forms.
Changing declared locale does not request language-specific substitutions. Any
other `shaping_language` is rejected. This is not general locale-aware shaping.

Only explicit operator `--font SHA256=PATH` mappings authorize font-file reads.
Profile/context JSON has no paths, URLs, search directories or implicit IO rights.
Mapped files are checked as regular files, bounded, canonicalized and protected
against output aliases. A symlink names the operator-selected resolved file; it
does not expand into directory search. Library callers supply immutable bytes to
`FontResources`. No system-font discovery, nearest-weight substitution, synthetic
bolding or glyph fallback is allowed. Missing/mismatched resources or glyphs fail.
This first profile accepts normal, static TrueType/CFF1 outline faces with 16..16384 units per em; variable/color/bitmap font
capabilities are not implied. Color/bitmap/SVG font tables are rejected in this profile. A font name does not establish redistribution rights.

The supplied tiny fixtures retain OFL/copyright notices and renamed derivative
identities. Their source hashes and deterministic regeneration instructions are in
[the fixture directory](../crates/vizir-compiler/tests/fixtures/fonts/README.md).
Full official font files remain outside the repository.

## Compilation and preservation

Every emitted text node in an opted-in document is measured with the selected bytes
and lowered to one existing Path with its original semantic ID. Original strings
remain in HIR/MIR; HIR/MIR IDs, data values/order, explicit scales/ranges and existing
origin explanation/lineage are preserved. The selectable/searchable/editable SVG
text loss is recorded explicitly, together with the bounded geometry approximation.

Chart header/tick/category allocation uses the same exact-face service before MIR
ranges are fixed. Final emitted ink is checked for view/canvas containment, chart
label-to-label and label-to-plot collisions, and diagram node-label containment.
This is not a general layout/collision solver for arbitrary authored geometry or
diagram edge labels. Oversized labels, missing glyphs and unsupported newlines/tabs
fail with `VIZ-TEXT-*` diagnostics; there is no wrapping, ellipsis or omitted label.
Existing diagram/category font-size defaults are retained and measured at the actual
emitted size. Author-specified baselines remain authoritative; normal accent/descender
overshoot is checked against the real container, not an assumed one-em box.

The saved compilation context fixes layout intent. Adding/changing text context on a
persisted theme-only or compiled MIR requires returning to original HIR. Refreshing
mark caches preserves context and explicit ranges; it never silently reflows them.

## Target precision and limits

The existing SVG backend emits four decimal places. This profile resolves outlined
path coordinates, group-transform scalars and Scene viewport/frame bounds to that
precision before checking fit. Cubic derivative extrema determine path bounds.
Ordinary fractional positions, translations, rotations and scales are supported;
a scale or viewport collapsing during serialization is rejected. Nonempty full-label
ink collapse is rejected; detected disappearing sub-contours produce a loss record.
Per-coordinate/per-transform-scalar rounding is at most 0.00005 scene units/degrees;
world-space error depends on the applied transform. This does not promise identical
antialiasing across rasterizers. Legacy output formatting is unchanged.

Hard ceilings (callers may tighten them using `TextLimits`) are:

- Font bytes: 32 MiB per file, 128 MiB across at most 16 supplied resources;
  at most 32 faces and 256 tables per face; exactly three selected role faces
- Text: 16 KiB per label, 1 MiB whole-call text bytes, 4,096 label operations
- Glyphs: 65,536 shaped and emitted glyphs per call
- Outlines: 1,000,000 generated/projected commands; cached outlines at most 16 MiB
- Collision checks: 1,000,000 per call; geometry traversal depth at most 64
- Serialized measured Scene, compiled MIR and SVG: at most 32 MiB
- Font size: 0.25 through 4,096 scene units; coordinates/transforms and document
  dimensions are restricted to finite magnitudes no greater than 1,000,000

Input decoding, existing materialization limits, PNG pixel/decoded-byte limits and
native renderer timeouts remain separate checks. Budgets fail closed without partial
publication. These counters are not a total process-memory/CPU bound, a hostile-font
sandbox, or proof that all underlying shaping-library failure cases are recoverable.

The workspace requires Rust 1.89 or newer because of cosmic-text 0.19.0. The published
CLI/workspace Cargo.lock is the reproducibility reference, with geometry-affecting
font/Unicode libraries explicitly pinned. The engine string is a profile identity,
not an attestation of every dependency selected by an arbitrary downstream Cargo
workspace. Library consumers must preserve equivalent locked dependencies; cross-build
or cross-platform binary/pixel identity is not claimed.

Font validation checks all declared face/table directory spans and offsets, and selected-face required head/hhea/maxp/hmtx/cmap structure, horizontal metric counts, CFF glyph counts or TrueType loca/glyf bounds. Shaped glyph IDs must be below the declared count. Unused OpenType table contents are not exhaustively validated.

The pinned skrifa CFF unscaled extraction uses integer font-unit coordinates. Extremely thin fractional-font-unit features are outside the guaranteed slice: an extracted non-whitespace glyph with no filled two-dimensional ink is rejected. The 0.00005 figure above describes subsequent Scene/SVG coordinate serialization, not a universal error bound against arbitrary original font curves. Post-serialization collinearity is tested on the exact decimal grid.
