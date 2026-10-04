# Measured single-line text compilation context

Measured outline text is an explicit, opt-in compiler profile. Default HIR,
MIR, Scene2D and SVG artifacts stay unchanged. `--theme` by itself retains the
existing `vizir-themed-mir/1` schema and APIs. Combining measured text with an
optional theme uses one reusable envelope instead of stacking feature wrappers:

```json
{
  "format": "vizir-compiled-mir/1",
  "context": {
    "theme": { "name": "azure", "...": "canonical resolved theme identity and defaults" },
    "text": {
      "profile": "vizir-text-outlines/1",
      "engine": "cosmic-text/0.19.0;harfrust/0.5.2;skrifa/0.40.0",
      "declared_locale": "en-US", "shaping_language": "default",
      "faces": {
        "regular": { "sha256": "<64 lowercase hex digits>", "face_index": 0, "weight": 400 },
        "medium": { "sha256": "<64 lowercase hex digits>", "face_index": 0, "weight": 500 },
        "bold": { "sha256": "<64 lowercase hex digits>", "face_index": 0, "weight": 700 }
      }
    }
  },
  "mir": {}
}
```

This illustrative envelope elides the required MIR and full theme defaults;
actual output includes both. The theme and text entries are independently
optional. `schema compiled-mir` emits the complete schema. Every persisted
theme is checked against the canonical pinned registry, including its defaults.
`schema themed-mir` and `schema mir` retain their previous contracts.

## CLI resources and replay

Save just the `text` object above as a strict JSON profile, filling in the
SHA-256 of your actual static regular, medium and bold font bytes. Paths are
operator-supplied resources, never profile or MIR fields. For example, with
shell variables containing each actual hash:

```sh
vizir normalize source.viz.yaml --theme azure --text-profile text-profile.json \
  --font "$REGULAR_SHA=fonts/regular.ttf" \
  --font "$MEDIUM_SHA=fonts/medium.ttf" \
  --font "$BOLD_SHA=fonts/bold.ttf" -o measured.mir.json

vizir render measured.mir.json --format svg -o measured.svg --manifest measured.render.json \
  --font "$REGULAR_SHA=fonts/regular.ttf" \
  --font "$MEDIUM_SHA=fonts/medium.ttf" \
  --font "$BOLD_SHA=fonts/bold.ttf"
```

`validate`, `normalize`, `lower`, `render` and `explain` all accept the profile
and repeated resource flags. The profile stores exact engine, locale, font
hash, collection face index and weight identities. `declared_locale` is declared font/content
provenance in this first profile; shaping uses the pinned library’s default
language behavior, not locale-selected OpenType language substitution. The
required persisted `shaping_language: "default"` makes this fixed behavior explicit;
other shaping-language values are rejected. Replay requires the exact
font bytes again; there is no system-font discovery, network loading, fallback,
font synthesis, profile-relative lookup, or persisted path. A resource may be
mapped from an explicitly supplied path outside the profile directory. Symlinks
are resolved and the opened resource must be a regular file. Each distinct
resource hash is supplied once, even if multiple faces refer to one collection.

A font hash mismatch, missing resource, incorrect face weight/index, unsupported
font, missing glyph or exceeded budget is an error. `--font` without measured
context is also an error. The first unit requires static normal-width upright
faces with exact 400/500/700 weights. Single-line shaping rejects line breaks,
tabs and control characters. Overflow/colliding labels are diagnosed; edit the
original HIR frame or strings to fix them. Wrapping, automatic fallback and
font synthesis are outside this profile.

A persisted envelope has already made its layout decisions. A repeated
`--text-profile` is accepted only if it exactly matches the persisted identity.
Adding measured text to a themed MIR, changing its profile, or adding/changing
its theme fails with an instruction to compile the original HIR. A legacy
`ThemedMir` remains a theme-only adapter, not a feature container.

Composition remains an independent source step: `compose` produces normal HIR,
then the resulting HIR accepts the same text/profile flags as any other source.

## Refresh, fidelity and resource limits

`normalize measured.mir.json` refreshes materialized data using the same
persisted context and explicit domains/ranges, without reflow. The refreshed
scene is checked with exact resources. Other commands reject stale caches.
Changing data may make preserved layout no longer fit; that is a diagnostic,
not permission to silently re-layout the persisted plan.

Font-independent outlines preserve each original text node’s stable semantic ID and provenance.
Original strings remain in HIR/MIR. Render manifests include
`compilation_context`, `source_context_format`, and scene/target loss records,
including the loss of SVG selectable/searchable/editable text. The existing
SVG and alpha-preserving PNG paths consume the same resolved Scene2D.

JSON inputs/profiles are limited to 32 MiB before parsing. Font files are
limited to 32 MiB each, with the compiler additionally bounding total bytes and
resource count. Whole-call limits independently bound label bytes and operations,
shaped/emitted glyphs, outline commands, cached outlines, collision checks and
serialized output. Measured Scene2D, compiled MIR JSON and SVG outputs are
limited to 32 MiB each; limits are not a total process-memory guarantee. Reads are capped, regular files are checked before and after
open, and Unix nonblocking opens prevent a swapped-in FIFO from waiting for a
writer. Output/manifest collision checks also protect profile and font sources,
including symlink and hard-link aliases. Existing staged transactional
publication remains in use. These safeguards do not promise universal I/O
latency or process-memory bounds, or confinement against hostile filesystem races.
Only explicit operator font mappings can authorize resource paths; profile and
MIR contents cannot expand filesystem or network access.

Both compiled envelopes and profiles reject duplicate JSON keys at every
depth and unknown fields. Compiled-envelope parsing also rejects fields that
legacy nested MIR deserializers would discard. Arbitrary supported inline data
metadata remains allowed. Optional context entries can be absent or null;
canonical serialization omits absent entries. JSON's recursion limit stays
enabled, and compiler materialization/text work budgets remain independent.

## Rust APIs

`CompilationContext::new().with_theme(...).with_text(...)` creates a reusable
identity-only context. `FontResources::new()` and `insert(hash, bytes)` supply
verified resources separately. `compile_with_context`,
`lower_to_compiled_mir`, `build_compiled_scene`, and
`rematerialize_compiled_mir` are additive APIs; their `_with_limits` variants
accept both materialization and text limits. `compile_compiled_mir` validates
or refreshes a persisted envelope and builds its scene in one call.

HIR compilation and persisted refresh each use one text session across their
phases, so text budgets are whole-call budgets. Existing theme-arrow reservations
and materializer preflight occur before large MIR clones. New public context
structs are non-exhaustive and offer constructors. `CompiledMir::from_themed`
provides a theme-only legacy adapter without changing existing APIs. `parse_compiled_mir_json`
and `parse_text_context_json` are the bounded byte readers; generic serde
callers must impose their own byte-input cap, then use compiler APIs to check
identity and executable semantics. Resources and paths never serialize into
compilation context.
