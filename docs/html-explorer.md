# Bounded HTML explorer

`explorer-v1` is an explicit, local browser runtime around VizIR's existing
Scene2D/SVG renderer. It adds a whole-document camera, recorded-source
inspection, and single Scene-node selection. It does not change chart domains,
filter data, link panels, edit geometry, evaluate expressions, apply ScenePatch,
or animate. HIR, MIR, Scene2D, CompilationContext and static SVG/PNG contracts
remain unchanged. PNG is still the default render format.

**Verification status:** native exporter/reducer and structural package checks
are separate from real-browser acceptance. Pointer/keyboard DOM behavior,
accessibility, browser CSP enforcement and visual browser output have not been
accepted in the current environment: browser access is policy-blocked. Native
checks do not substitute for that gate. This is a bounded implementation, not
promotion of the full interactive roadmap phase.

## CLI and Rust API

```sh
vizir render examples/mixed/reliability-brief.viz.yaml \
  --format html --interaction-profile explorer-v1 \
  --instance-key main --output brief.html --manifest brief.render.json
vizir capabilities html
vizir schema interaction --output schemas/interaction.schema.json
```

HTML requires the exact named profile. `--instance-key` defaults to `main` only
when omitted. Both new options are rejected for SVG/PNG. Existing theme, measured
text and input handling are reused. No renderer process or network resource is
required for HTML export.

`vizir_web::render_html(&Scene2D, &ExplorerOptions)` returns `HtmlExport { html,
manifest }`. `ExplorerOptions` contains `instance_key`. `render_fragment` returns
one generated section, without a document, stylesheet or executable script.
For multiple instances, render fragments with distinct keys into one host
HTML document, include `STYLESHEET` and the exact `runtime_script()` bytes once,
and use `content_security_policy()` (or an equivalent stricter host policy).
Do not concatenate complete standalone HTML documents. Raw `RUNTIME` is the
checked-in payload, not the configured executable module.

The shared SVG backend has `SvgRenderContext::new(instance_key)` and
`render_to_with_context(scene, context, writer)`. Both static and HTML outputs
use the same shape emission code. Legacy `render(scene)` keeps its formatting,
IDs and bytes. The opt-in path uses round-tripping root/background dimensions,
a fixed document-sized background and safe namespaced DOM identities. This
avoids rounding a fractional camera home extent differently from the SVG.

Output/manifest publication retains the existing source-alias protections and
staging with best-effort rollback. It is not a crash-atomic transaction.

## Versioned context and identity

The artifact carries strict `vizir-interaction/1` metadata with:

- `profile: "explorer-v1"`, document ID and instance key
- SHA-256 of compact `serde_json` serialization of the validated Scene2D
- `runtime_payload_sha256`, covering the exact checked-in `runtime.mjs` bytes
- exact home viewport, fixed camera policy and preorder node identity records
- each exact Scene-node ID, required nullable parent ID, derived DOM ID, and
  typed Origin (including the full lineage array)

The runtime configuration prefix is exactly
`const VIZIR_RUNTIME_PAYLOAD_SHA256 = "<payload SHA-256>";` followed by one LF
and the unchanged payload. The payload digest excludes that prefix to avoid a
self-hash cycle. The CSP and manifest's `runtime_sha256` cover the complete
executed module, including the prefix. The manifest also records the separate
payload and stylesheet digests. Metadata, mount and the document-level runtime
registry must agree on the payload revision; another payload using the same
wire profile cannot silently reuse the first runtime.

These hashes bind an immutable export snapshot and runtime revision. They do
not authenticate source authors, prove provenance truth, or authorize content.

Instance keys match `[a-z][a-z0-9-]{0,31}`. Node DOM IDs are
`vzi-<key>-node-<lowercase UTF-8 hex of Scene-node ID>`. Title, marker and control
IDs use disjoint tagged domains. Original IDs remain unmodified in metadata and
`data-vizir-scene-id`. The runtime never interpolates opaque IDs into selectors.

References qualify exact node identity by instance, document and scene digest.
No joins are inferred from `data-key`, and legacy comma-joined `data-lineage`
attributes are never parsed into identity. A line/area path may represent a
series, a point may represent a row, and a geometry element may have no data
key. Diagram edges retain the existing positional identity; recompiling after
edge reordering does not promise persistent selection. No source rows are
copied into the package by this runtime.

## Camera and selection semantics

The home view is `[0, 0, width, height]`. Camera zoom is clamped to 1–16, and its
viewBox stays inside the document. Buttons use a 1.25 zoom factor centered on
the current view. With the viewport focused, unmodified arrow keys pan by 10%
of the visible extent, `+`/`-` zoom and Home/0 resets the camera. Shift used to
type `+` is supported. Native picker/button keys and browser modifier shortcuts
are left alone. Reset changes only the camera.

Inspect is the default mode. A primary click selects one exact Scene node.
Hover or keyboard focus shows recorded provenance. The native source picker
provides the same selection without requiring every SVG primitive in tab order.
Nodes use programmatic focus and at most the selected node is a tab stop.
Selection has a text status and focus/adornment indication, not just color.

The Pan toggle enables single-primary-pointer dragging with a 4 CSS-pixel
threshold. A drag cannot produce a following selection click. A second pointer,
pointer cancel, lost capture, window blur or Escape cancels the gesture.
Escape on the viewport outside a gesture clears selection. Page wheel events
are not intercepted. There is no custom wheel/pinch zoom; explicit Pan mode
uses `touch-action: pinch-zoom` to allow user-agent pinch zoom while suppressing
native single-finger panning on that viewport. The adapter cancels local pan
on a second pointer. These touch/browser details still require real-device QA.

All updates are immediate, with no tweening or autoplay; reduced-motion users
receive the same motion-free behavior. There is no fit-to-node calculation from
approximate Scene bounds, no chart-domain zoom and no child transform rewrite.

## Lifecycle and embedding

The module installs `globalThis.VizIRExplorerV1` with mount access. Mount owns
only its generated section and validates metadata, runtime identity, complete
Scene-node mapping, ancestry and internal references before attaching handlers
or changing runtime-owned DOM state. Callers must not modify the generated
subtree while it is mounted.

Mount returns `getState`, `reset` and `dispose`. A repeated explicit mount on
an active root is an error. Repeated execution of the same configured module
is bootstrap-idempotent and retains the existing registry/listeners. Different
payload revisions reject. State is instance-local; there is no selection bus,
localStorage, URL state or network state.

Duplicate keys are rejected against all generated roots in the owner document,
including disposed roots whose SVG IDs still exist. Disposing one of two equal
keys does not make their DOM identities unique. Remounting the same sole root
is supported. Dispose is idempotent, removes handlers, releases capture,
cleans pending work and restores runtime-owned static state. Call it before
replacing/removing a mounted fragment. Failure leaves the static figure visible
and surfaces a plain-text error; it does not claim interactivity succeeded.

## Resource and security limits

All are opt-in HTML limits; static acceptance is unchanged:

- width and height: 1 through 1,000,000 Scene units
- 8,192 nodes, 64 levels, 262,144 path commands
- 4,096 UTF-8 bytes per source string, including text, identity, every Origin
  field, colors and loss strings; derived hex DOM IDs have their exact length
- 4 MiB aggregate source strings, 32,768 aggregate lineage entries, 8,192 losses
- 16 MiB compact Scene JSON through a streaming hash/count writer
- 4 MiB metadata JSON and HTML-safe metadata script text, including surrounding LF
- 32 MiB final HTML through a bounded writer

An iterative preflight bounds every traversed array/string before recursive
validation, hashing or rendering. It conservatively reserves escaped metadata,
SVG and final-package expansion before cloning metadata. This reserve may
reject a package below the final byte limit, especially many tiny nodes or
long repeated parent identities. Limits are simultaneous ceilings, not a
promise every individual ceiling is attainable. No content is truncated or
silently omitted. They bound export work and intermediate storage; they are
not a total-process-memory or measured-browser-responsiveness guarantee.

Scene hashing streams without constructing an unbounded JSON copy. Metadata
has a capped 4 MiB intermediate buffer, and SVG/path emission streams directly
into the bounded HTML sink. There is no complete intermediate SVG string.

Only compiler-generated markup is supported. Metadata uses typed JSON with
unknown/duplicate fields rejected at its typed native reader. HTML-safe
serialization escapes `<`, `&`, U+2028 and U+2029; the browser checks strict
metadata before mounting. Provenance is inserted using textContent. There is
no authored HTML/script, SVG import, eval/Function, external asset, remote font,
fetch, URL action or inline event handler. Changes use scoped classes, ARIA,
viewBox and presentation attributes, never style attributes/properties.

The standalone package has hash-only script/style CSP and denies default,
connect, image, font, object, base and form destinations. It uses no
`unsafe-inline` escape. Meta CSP does not supply frame-ancestors protection.
Host embedding must preserve the intended resource policy. This is an export
contract, not a sanitizer for arbitrary existing HTML or a compromised host.

## Verification

`just check` includes Rust formatting/lints/tests and the actual JavaScript
reducer suite under `node --test`. JavaScript uses no installed packages.
`tools/check_web_package.py FILE.html` parses the HTML natively and verifies
CSP hashes, allowed structural surfaces, identity correspondence and internal
references; it explicitly reports browser DOM acceptance as not run.

Required remaining browser acceptance covers pointer/keyboard hit targets,
focus and screen-reader behavior, touch cancellation, letterboxing/camera math,
CSP enforcement, multiple instances, repeated bootstrap, dispose/remount and
visual inspection at delivery size. None may be inferred from native tests,
string audits or static SVG/PNG comparisons.
