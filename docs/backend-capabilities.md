# Scene2D backend negotiation

`negotiate_scene` checks a backend profile against the resolved `Scene2D` before
emission. Declaring the required feature names is necessary but not sufficient:
the input IR identity and every advertised limit must also be understood and met.

## IR identity and versions

The current exact `accepted_ir` spellings are `scene2d` and
`scene2d-through-svg`. Both accept the current static Scene2D 0.1 wire shape;
the latter names the existing PNG rasterization route. Matching is exact and
case-sensitive. Prefixes, whitespace, other dialects, and version-qualified
spellings such as `scene2d@0.2` are not aliases.

Scene2D has no serialized version field today. This check therefore enforces
the existing identity aliases, not a version negotiation protocol. HIR/MIR 0.2
numeric-guide formatting still lowers to the same Scene2D shape. The profile's
`version` is the backend implementation version, not an IR version. An explicit
versioned Scene2D envelope and accepted-version ranges require a separate future
wire/source migration.

Profiles for other IRs remain serializable and structurally valid, so discovery
tools can inspect them. Passing one to `negotiate_scene` produces an
`accepted_ir` error decision rather than accepting an unrelated IR based on
feature-name overlap.

## Advertised limits

The Scene2D negotiator recognizes exactly these keys:

| Key | Measurement |
| --- | --- |
| `max-nodes` | Total recursive `SceneNode` occurrences, including each group (even an empty group) and every drawable child |
| `max-clip-depth` | Maximum simultaneously active clip nesting; currently zero because static Scene2D has no clip construct |

Limits are inclusive. An absent key imposes no backend-profile restriction;
zero is a hard zero. Counts are for the resolved Scene2D, not for the HIR, MIR,
unique semantic IDs, capability decisions, or generated SVG/XML elements. A
background rectangle, SVG `<defs>`, marker resources, and document metadata are
not SceneNodes. Repeated node IDs do not reduce the occurrence count.

Ordinary groups and transforms do not create clips. In particular, 33 nested
groups fit a `max-clip-depth` of zero. The SVG profile's existing value of 32
does not impose a group-depth limit, and this change does not add clipping.
There is no representable 32- or 33-active-clip scene in the current model.
Future clip support must update both the scene model and this measurement.

Unknown or misspelled limit keys yield `limit.<key>` error decisions, even when
their value is zero. They remain allowed in inspectable profiles because other
IR negotiators may define different limits. They are never silently ignored by
Scene2D negotiation. Direct JSON deserialization into `BackendCapabilities`
rejects duplicate limit keys instead of using the last value, which could relax
a limit. Parsing into `serde_json::Value` first already collapses duplicates;
the profile decoder cannot recover declarations discarded by that earlier step.

The Rust limit values remain `u64`, preserving canonical integer values through
`u64::MAX`. Numeric deserialization is unchanged: the existing serde integer
parser does not accept floating-point JSON spellings such as `2.0`, even though
JSON Schema considers an integer-valued number valid. That pre-existing parser
and schema distinction is not a negotiation rule or a claim that `2.0` is
malformed JSON.

## Reports and emission

Feature requirements are still reported individually. Failed identity and limit
checks add deterministic error decisions, ordered by `(source, feature)` with
the existing feature decisions. Limit errors name the observed count and bound;
unsupported-limit errors name the unrecognized key. Passing checks do not add
synthetic decisions, preserving ordinary reports and emitted artifact bytes.

Both `unsupported_policy: error` and `unsupported_policy: report` return a
report containing the errors; neither authorizes dropping content or exceeding
a declared resource limit. `CapabilityReport::is_accepted()` is false when any
decision is an error, and `require_accepted()` returns `VIZ-CAP-0002`. Structural
profile failures still return their existing validation diagnostics directly.

The SVG backend and CLI require an accepted report before artifact publication.
The CLI uses its built-in SVG/PNG profiles; there is no custom-profile command
line override. Public API callers using a custom profile must likewise call
`require_accepted()` before emitting or publishing output. These checks run on
an already-built Scene2D; they neither perform complete Scene2D validation nor
bound total process memory. Negotiation does not replace document/MIR validation
or the CLI's independent PNG and process bounds.

## SVG XML string representability

SVG emission separately checks every emitted source string against the
[XML 1.0 character set](https://www.w3.org/TR/REC-xml/#charsets). Unrepresentable
characters fail with `VIZ-SVG-0001`, naming the field, code point, and UTF-8 byte
offset, without echoing source content. Node locations use zero-based preorder
ordinals (`nodes(preorder)[N]`), including groups, to keep diagnostics bounded.
This target check does not restrict generic Scene2D opaque IDs or inspect
metadata that SVG does not emit, such as origin explanations and loss records.

Attribute tabs, line feeds, and carriage returns are emitted as character
references; text carriage returns are also referenced. XML parsing therefore
preserves their exact values instead of normalizing them. XML-special characters
remain escaped, and ordinary legacy output bytes are unchanged. This guarantees
XML string round trips, not browser ID-selector syntax or text layout behavior.
The CLI performs this check before staging or publishing SVG, PNG, or manifests;
an XML error leaves any previous output files untouched.
