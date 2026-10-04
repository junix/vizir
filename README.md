# VizIR

VizIR is an experimental, deterministic visualization compiler. Its editable
source is a semantic `.viz.yaml` document; generated MIR and Scene2D are
inspectable build artifacts.

```text
VizHIR -> validated document -> VizMIR -> Scene2D -> capability report -> target
```

The MVP deliberately supports three dialect families instead of pretending one
renderer can understand every visual system:

- `chart.scatter`, `chart.line`, and `chart.bar`;
- `diagram.graph` with deterministic layered or manual layout;
- `geometry.scene` with typed groups, shapes, text, paths, and transforms.

PNG is a first-class delivery format and defaults to a transparent background.
SVG remains the exact, inspectable target. PNG rasterization uses
`rsvg-convert`, with ImageMagick as a fallback.

## Quick start

```bash
just install

cargo run -p vizir-cli -- validate examples/chart/service-health.viz.yaml

cargo run -p vizir-cli -- normalize \
  examples/chart/service-health.viz.yaml \
  --output out/service-health.mir.json

cargo run -p vizir-cli -- render \
  examples/chart/service-health.viz.yaml \
  --format png \
  --background transparent \
  --output out/service-health.png \
  --manifest out/service-health.manifest.json
```

Use `vizir explain <file> --node <stable-id>` to inspect why a Scene2D node
exists, and `vizir capabilities <backend>` to inspect output support.

### Output path safety

`normalize`, `lower`, and `render` reject an output that refers to their input.
`render --manifest` also rejects a manifest that refers to the input or rendered
artifact. The read-only preflight runs before creating destination directories
or writing any artifact, including before PNG rasterization. A collision reports
`VIZ-PATH-0001`; a path that cannot be inspected reports `VIZ-PATH-0002`.

Checks include relative/absolute spellings, existing symlinks and hard links,
symlinked parent directories, and dangling symlinks to new destinations. Missing
parents are projected without creating them, respecting symlinks before `..`.
Distinct destinations still support automatic parent creation and overwriting
an existing artifact; JSON output to stdout is unchanged.

This is protection against accidental path collisions, not a filesystem security
boundary. Concurrent path/link changes after the preflight can invalidate the
check. Checks rely on the filesystem's path resolution and file identities;
they do not predict filesystem-specific aliases between differently spelled
files that do not exist yet (for example, case folding on some volumes).

### Output publication

All file writers (`normalize`, `lower`, `schema`, SVG, PNG, and render manifests)
first write to fresh staging directories beside their destinations (mode `0700`
on Unix; inherited access control on other platforms).
PNG rasterizers receive an absent staging path, never the final output, and that
fresh artifact must pass the complete PNG and alpha checks before publication.
A renderer that exits successfully without writing cannot reuse an old PNG.
Compilation, provider selection, rendering, validation, or manifest serialization
failure leaves existing artifact and manifest contents untouched; absent outputs
remain absent. The manifest always records the requested output path.

After staging succeeds, every destination and required backup is prepared before
any final file changes. Ordinary files are replaced by same-directory rename;
existing leaf symlinks remain links and their resolved targets are updated.
Multiply hardlinked files are copied in place to preserve their shared inode,
with an independent readable backup for rollback. On Unix and Windows the link
count selects this behavior; other platforms conservatively copy in place.
Ordinary write-only files can use a hardlink backup without reading their contents.
An unreadable multiply linked file, or an ordinary file that cannot be backed up
by either hardlink or copy, is rejected before any final file changes. Existing
file write permission is required even when rename would otherwise be possible.
Existing Unix read/write/execute permission bits and platform permission flags
are preserved; special bits remain subject to OS write semantics. Ownership,
ACLs, and extended attributes are not promised across rename replacement.
Destination directories must permit
staging; their permissions are never changed. Newly created parent directories
may remain after a failed command.

If a publication operation fails, VizIR attempts to restore all destinations it
changed, including the partially copied hardlinked file. Success messages appear
only after the artifact and optional manifest are published. If rollback itself
fails (for example, because the filesystem becomes unwritable), `VIZ-OUTPUT-0001`
reports the failure and retained recovery directories; it does not claim that
old contents were restored. Normal success and handled failures clean staging.

This is not a multi-file atomic or crash-durable transaction: readers may observe
the new artifact before its manifest, and hardlink updates may expose partial
bytes. Crashes, forced termination, power loss, concurrent writers, path/link
changes, and external processes that keep modifying staged files are outside
the guarantee. A crash can leave mixed versions and staging/backup directories;
there is no automatic crash recovery or directory `fsync` durability guarantee.

### PNG alpha validation

The default `--background transparent` requires at least one fully transparent
pixel (alpha zero) and at least one visible pixel (nonzero alpha). Validation
uses decoded samples, including 8/16-bit RGBA and grayscale-alpha, low-bit
indexed palettes, and grayscale/RGB `tRNS` transparency. Full 16-bit precision
is preserved: alpha `0x0001` is visible. An alpha-capable file containing only
opaque pixels or only fully transparent pixels does not meet this contract.

Explicit hex backgrounds (`#RRGGBB` or `#RRGGBBAA`) do not require a transparent
pixel or an alpha channel; RGB, grayscale, and indexed PNGs without transparency
are valid opaque encodings. An eight-digit background may itself be translucent,
so specifying a hex background is not an assertion that every pixel is opaque.
Every mode must decode the pixels and finish the PNG stream, including IEND/CRC
validation. Both rasterizer routes use the same checks.

### PNG verification resource limits

PNG verification accepts at most **16,777,216 pixels** (`width × height`) and
**134,217,728 decoded bytes** (128 MiB) per image. The byte budget includes all
channels after palette, packed grayscale, and `tRNS` expansion, and preserves
16-bit samples: RGBA8 needs four bytes per pixel, RGBA16 needs eight. These
limits apply to both renderer routes and every background mode.

The PNG library reads the IHDR first; checked dimension arithmetic rejects
over-budget images before further metadata and pixel-buffer allocation. After
metadata determines the expanded sample format, checked byte arithmetic and a
fallible reservation guard the caller's decoded buffer. These failures report
`VIZ-ARTIFACT-0004` and leave existing output and manifest contents unchanged.
For a valid but over-budget IHDR, this resource diagnostic takes priority over
later missing/corrupt pixels or stream chunks. Within-budget invalid streams
still report `VIZ-ARTIFACT-0001`; complete decoding and alpha checks remain
required for success.

The decoder retains its separate 64 MiB internal allocation limit. The caps are
verification maxima, not a guarantee that every image shape below them can be
decoded or a total-process memory bound. Allocator overhead, decoder state,
and external rasterizer memory are separate; rasterization happens before
artifact verification. Scene/SVG dimensions are not newly capped, and do not
imply support for arbitrarily large PNGs.

### External renderer limits

PNG selection still prefers `rsvg-convert`, falling back to ImageMagick (`magick`)
when its probe is unavailable or unsuccessful. Each `--version` probe has a
2-second deadline, and the selected rasterizer has a 30-second deadline. The
manifest records that successful selection without launching another probe. A
failed selected renderer is an error, not a request to retry with another backend.

Each deadline covers both direct-child exit and completion of stdout/stderr
capture. Stdin is closed. At most 64 KiB of raw diagnostic bytes are retained
per stream; excess bytes are drained and discarded, with a truncation marker in
reported diagnostics. Both streams are drained in bounded chunks so a flood
cannot prevent deadline checks. Captures use nonblocking reads on Unix and
available-byte reads on Windows, without detached reader threads.

After direct-child exit, inherited diagnostic pipes get at most 250 ms to close,
within the original deadline. A timeout, capture failure, or held-open pipe is an
error even if a PNG was written. On cancellation, only the owned direct child is
killed, followed by up to 250 ms of nonblocking reap attempts; a cleanup failure
is reported. Descendants are not killed and may survive after their capture
handles close. No process groups, global signal handlers, or global environment
changes are used. Deadlines are polling bounds (5 ms interval), with cancellation
grace and normal OS scheduling/spawn overhead, not hard real-time guarantees.

Success also requires a zero exit status and a decodable, complete fresh PNG
passing the alpha contract. Failed and timed-out renders discard their staging
without changing existing output or manifest contents. See Output publication
above for publication rollback and crash/concurrency limits.

The executable contracts can be emitted directly from the Rust model:

```bash
vizir schema mir --output schemas/viz-mir.schema.json
vizir schema scene-patch --output schemas/scene-patch.schema.json
vizir schema capability --output schemas/capability.schema.json
```

### Numeric chart fields

Line, scatter, and bar numeric fields must contain finite numbers. A field
whose finite maximum minus minimum overflows is rejected before normalization
with `VIZ-TYPE-0106`; rescale the input values to a smaller magnitude. This
prevents that overflowing span from producing invalid domains or coordinates.
This check does not promise arbitrary-magnitude arithmetic or constrain every
possible derived-domain operation.

### Chart header layout

Chart titles, legends, and axis titles reserve space before scales are resolved.
Short legends retain the compact positions when they fit; otherwise all entries
flow onto width-aware rows below the title. Plot ranges and Scene2D use the same
layout, and stable label IDs, full text, category order, and colors are retained.

Header widths use conservative, deterministic advance estimates for the default
sans-serif stack, not measured glyph bounds or font shaping. Wide Latin letters
reserve more space than narrow ones; each non-ASCII code point, including a
combining mark, reserves 1.2 em. Font substitution can still affect rendered
metrics, so inspect the final artifact at delivery size. Text is never silently
truncated, hidden, or reduced to make a header fit.

An individual overwide title/legend/axis title, or a frame that cannot retain a
64-by-64 scene-unit plot after header and axis insets, fails with
`VIZ-LAYOUT-0004`. Enlarge the frame or edit the text. This happens before output
publication. Persisted MIR whose scale ranges no longer match this layout fails
with `VIZ-LAYOUT-0005`; normalize it again from its VizHIR source. Range checks
allow only floating-point roundoff from serialization and recomputation; guides
then use the same stored endpoints as marks.

## Current boundary

VizIR owns schema validation, normalization, lowering, layout, stable identity,
provenance, capability reporting, Scene2D construction, and artifact emission.
It does not own natural-language intent interpretation or backend routing; that
remains the responsibility of the `create-plot` skill.

Interaction, animation, Scene3D, external layout providers, native TikZ, and
large columnar instance buffers are planned dialect/runtime extensions, not
MVP placeholders hidden behind generic enums.

See [the IR family contract](docs/ir-family.md) for the ownership boundary,
stable 0.1 surface, and promotion rules.

## Executable reference gallery

Every gallery PNG is built from the adjacent semantic example and retains a
transparent alpha channel. These are scenario references, not isolated shape
smokes.

| Scenario | Semantic source | Rendered reference |
| --- | --- | --- |
| Service reliability dashboard | `examples/chart/service-health.viz.yaml` | `gallery/service-health.png` |
| Multi-series incident recovery | `examples/chart/incident-latency.viz.yaml` | `gallery/incident-latency.png` |
| Model evaluation comparison | `examples/chart/model-evaluation.viz.yaml` | `gallery/model-evaluation.png` |
| Regional revenue bindings | `examples/chart/sales-regions.viz.yaml` | `gallery/sales-regions.png` |
| Agent compiler runtime | `examples/diagram/agent-runtime.viz.yaml` | `gallery/agent-runtime.png` |
| Streaming data platform | `examples/diagram/data-platform.viz.yaml` | `gallery/data-platform.png` |
| Automatic dialect lowering layout | `examples/diagram/dialect-lowering.viz.yaml` | `gallery/dialect-lowering.png` |
| Compiler invariant poster | `examples/geometry/compiler-pipeline.viz.yaml` | `gallery/compiler-pipeline.png` |
| Visual grammar map | `examples/geometry/visual-grammar.viz.yaml` | `gallery/visual-grammar.png` |
| Mixed reliability brief | `examples/mixed/reliability-brief.viz.yaml` | `gallery/reliability-brief.png` |
| Mixed capacity planning brief | `examples/mixed/capacity-planning.viz.yaml` | `gallery/capacity-planning.png` |

Run `just gallery` to regenerate every PNG and the searchable, self-contained
[`gallery.html`](gallery.html); `just gallery-check` verifies it is current.

![Service health dashboard](gallery/service-health.png)

![Visualization compiler pipeline](gallery/compiler-pipeline.png)
