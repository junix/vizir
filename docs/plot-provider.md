# Native compiled SVG provider

The second binary in `vizir-cli`, `plot-provider-vizir`, offers two disjoint local
capabilities through the same existing compiler, SVG backend and bounded I/O:

- `render-compiled-svg`: capability `visualization.vizir.render-compiled-svg-v1`,
  profile `vizir-compiled-svg/1`, matching MIR/HIR **0.4 only**, receipt
  `vizir.render-receipt/v1`
- `render-compiled-svg-v2`: capability `visualization.vizir.render-compiled-svg-v2`,
  profile `vizir-compiled-svg/2`, matching MIR/HIR **0.5 only**, receipt
  `vizir.render-receipt/v2`

The v1 command descriptor, input/output schemas, receipt schema, and rendering
behavior remain unchanged. The new command explicitly adds 0.5 replay; neither
command guesses a version or accepts future versions. The original `vizir` CLI
and omitted-label behavior are unchanged.
It does not import data, discover fonts, download resources, use a browser,
rewrite source, rematerialize stale caches, or change layout policy.

This is the **standalone provider** boundary. Its self-checked receipt does not
by itself establish independent Hub verification. Accepted plot-acme integration
additionally needs that Hub's fixed receipt-core publication gate and pinned
execution identity. No old Hub is assumed to enforce unknown `x-*` metadata.

## Build and metadata

`cargo build -p vizir-cli --bins` builds both `vizir` and `plot-provider-vizir`.
`just build` builds both release binaries; `just install` copies each binary with
the existing atomic-copy and macOS signing steps. `default-run = "vizir"` preserves
existing `cargo run -p vizir-cli -- ...` commands. To select the provider, use
`cargo run -p vizir-cli --bin plot-provider-vizir -- ...`.

```sh
plot-provider-vizir describe --json
plot-provider-vizir doctor --json
```

Metadata never reads fonts, scans directories, starts a renderer, or accesses
the network. The version is the real package version, optionally suffixed with
the existing build's `PM_BUILD_SHA`. An unstamped build does not invent a commit
identity. The required describe `source.local_code_path` records the truthful
compile-time repository root as build provenance; it never authorizes resource
reads, need not exist on a deployment machine, and is absent from receipts.
Doctor reports only readiness of the built-in renderer; it cannot
verify resources that have not been supplied.

The immutable v1 command contract is
[`compiled-svg-command-v1.json`](../crates/vizir-cli/assets/compiled-svg-command-v1.json).
The separate 0.5 command contract is
[`compiled-svg-command-v2.json`](../crates/vizir-cli/assets/compiled-svg-command-v2.json).
Every actual source file is an explicit, top-level `x-acme-role: input-file`.
The receipt output declares the fixed `x-acme-receipt-core` relation metadata.
These fields do not imply that an arbitrary consumer already supports them.

## Explicit bundle contract

```text
plot-provider-vizir render-compiled-svg INPUT \
  --text-profile FILE [--text-layout FILE] \
  --font-1 FILE [--font-2 FILE] [--font-3 FILE] \
  --resource-pins JSON --output SVG --receipt JSON
```

- Input must be a complete `vizir-compiled-mir/1` envelope with both MIR and
  source HIR version `0.4` for v1 or `0.5` for v2, a valid canonical theme, and exact measured text
  `vizir-text-outlines/1`. HIR, bare MIR, theme-only context and null context
  placeholders are rejected
- `text_profile` is required and its strictly parsed typed value must equal
  persisted `context.text`. An explicit `text_layout` file is required if and
  only if persisted `context.text_layout` exists, with full typed equality.
  Native wrapping profiles `vizir-text-wrap/1` through `/4` are supported by both commands
- One to three contiguous font slots have a distinct SHA256 set exactly equal
  to the profile's face SHA256 set. A collection used for multiple faces is
  supplied once. Missing, duplicate, surplus, malformed or wrong-weight faces
  fail; there is no system fallback
- `resource_pins` is a closed JSON object mapping exactly the supplied roles
  (`input`, `text_profile`, optional `text_layout`, `font_1`–`font_3`) to
  lowercase 64-character SHA256 strings. Unknown/duplicate keys, null values,
  missing/extra roles and incorrect raw-byte hashes fail. Pins include original
  whitespace, rather than only a normalized JSON value
- Explicit absolute and relative local paths are accepted. The canonical
  directory of the compiled input's **original parent** is the bundle root;
  the canonical regular-file target of each input must be directly inside it.
  A leaf symlink cannot silently choose an outside bundle. Explicit in-bundle
  symlinks work. Paths keep native symlink/`..` semantics; they are not first
  lexically cleaned. URLs, stdin, nonregular files and source aliases fail
- Each input is checked against both output destinations for same-path,
  symlink and hard-link aliases. The two destinations must also be distinct.
  Outputs may be elsewhere, including explicit Hub stage paths
- Compiler and renderer consume verified immutable buffers. Source directory
  identity, file identities, raw bytes and destination aliases are rechecked
  before publication. This is ordinary-filesystem validation, not a race-free
  sandbox or a security boundary against concurrent malicious writers

Budgets: compiled input 32 MiB; profile and layout each 256 KiB; pins JSON 4 KiB;
each font 32 MiB and total fonts 96 MiB; SVG 32 MiB; receipt 8 MiB. The existing
native traversal, materialization, shaping, outline, collision and text-fit
limits still apply. Output loss records are never truncated to fit a budget.

## Exact 0.5 label replay

Use `render-compiled-svg-v2` with the same explicit argument names above for a
prepared MIR/HIR 0.5 bundle. Optional heatmap `value_labels` follow the existing
[native label contract](heatmaps.md#optional-present-cell-value-labels-in-05).
No new label engine or layout policy is introduced. `compile_compiled_mir` runs
with refresh disabled, so authoritative typed values, label keys/text/order/count,
measured fonts, exact wrapping policies and native fit/contrast/resource limits
are checked before either output is published. Stale or invalid caches fail;
the provider never repairs them.

CSV is imported and normalized by the existing CLI before provider execution;
the provider reads only the prepared compiled document and its declared text
resources. Exact Int64 label digits, present zero cells, and missing category
pairs retain the native engine's semantics. There is no CSV path discovery.

## Path-free receipt and publication

Both SVG and receipt are mandatory. They are completely rendered, typed,
self-checked, written to fresh stages and byte-verified before publication.
Existing destinations are preserved on a normal validation/staging failure.
The shared publication layer uses same-directory rename for ordinary files and
best-effort rollback across both outputs. It is not a crash-atomic transaction;
concurrent path changes, writer interference and failed rollback remain outside
its guarantee. Existing multiply linked outputs retain shared identity via the
same established publication semantics as the main CLI.

[`render-receipt.schema.json`](../schemas/render-receipt.schema.json) describes
`vizir.render-receipt/v1`, closed to source 0.4. The separate
[`render-receipt-v2.schema.json`](../schemas/render-receipt-v2.schema.json)
describes `vizir.render-receipt/v2`, closed to source 0.5 and profile
`vizir-compiled-svg/2`. Its native context, capability and loss payloads remain
complete. The generic `plot.artifact-receipt-core/v1` relation is unchanged in
both receipts. Each typed receipt includes:

- Exact provider ID/version and implementation profile
- `artifact_receipt` with fixed schema `plot.artifact-receipt-core/v1`, sorted
  input records `{role, sha256, bytes}`, and primary record
  `{artifact_id:"figure", role:"primary", argument:"output", kind:"svg", sha256, bytes}`
- Document/source versions, full native compilation context, source-owned
  background, full native capability decisions, and ordered native loss records

It contains no CLI input/output paths, temporary filenames, working directory,
timestamp, or self-referential executable hash. Source-owned IDs are retained
unchanged, including slashes. Text outlines carry the native loss that text is
not selectable/searchable/editable; there is no invented SVG rasterization loss.
A bounded stderr summary refers to the declared receipt; stdout is not an
artifact transport or result schema.

Replay is scoped to the same provider build and exact compiled/resource bytes.
No cross-build, cross-platform or cross-provider byte/pixel equivalence is
promised. The separate execution system is responsible for pinning executable
identity; schema validation alone does not prove input/primary hash relations.

## Verification

`just check` covers Rust format/lint/tests and the existing Node runtime tests.
Provider tests use explicitly illustrative source and the checked-in small,
licensed test font subsets. They verify raw pins, typed context, direct-native
SVG byte parity, complete context/loss/capability equality, path-free copied-only
replay, resource/destination alias rejection, limits and preserved old outputs.
Both profiles run the same resource, alias, limits, copied-bundle, wrapping
and failure-preservation suite. The 0.5 cases additionally cover inline and
explicit typed CSV labels, sparse zeros, Int64 extrema, contrast, and stale
label-cache mutations. Byte-pinned tests protect the old descriptor and receipt
schema, while cross-version and future-version cases verify disjoint admission.
The fixtures do not claim to be production fonts or the user's own data.
