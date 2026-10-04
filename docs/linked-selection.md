# Explicit linked selection

`explorer-linked-v1` is a separate opt-in HTML profile. A selection has one
primary Scene node and an explicitly authored set of linked Scene nodes in the
same immutable document/instance. It uses the existing mixed-scene SVG and the
shared explorer runtime. It does not join data, filter rows, change chart
scales, mutate source geometry, or communicate with another instance/document.

**Acceptance boundary:** native reducers, strict readers and package checks do
not establish real-browser DOM, focus, pointer, accessibility, CSP enforcement
or visual acceptance. That browser gate remains policy-blocked and unrun.

## Author links from an actual export

First export the exact source with the intended theme, font, background and
instance options using the existing `explorer-v1` profile:

```sh
vizir render examples/interaction/linked-services.viz.yaml \
  --format html --interaction-profile explorer-v1 \
  --instance-key main -o inspect.html --manifest inspect.render.json
```

Use the manifest's `document_id`, `interaction.scene_sha256` and
`interaction.instance_key`. Obtain exact `scene_node_id` values from the
artifact's typed metadata/source picker, or from `vizir lower` with the same
compilation options. Do not reimplement Scene JSON serialization or infer IDs
from a data key, visible label, HIR name or DOM identifier.

Save a strict local JSON file with this shape:

```json
{
  "format": "vizir-selection-links/1",
  "document_id": "linked-services",
  "scene_sha256": "93a5a7259655200c6e8b3d1e70bc61caeba677569c9844958546f1a569397065",
  "instance_key": "main",
  "groups": [{
    "id": "api",
    "members": [
      "latency/point/api",
      "topology/node/api/shape",
      "topology/node/api/label",
      "status/api-card"
    ]
  }]
}
```

The digest above belongs only to the checked-in example with its default
compilation options. A complete two-group specification is provided beside
that source. It deliberately lists members out of Scene order to demonstrate
normalization:

```sh
vizir render examples/interaction/linked-services.viz.yaml \
  --format html --interaction-profile explorer-linked-v1 \
  --selection-links examples/interaction/linked-services.links.json \
  --instance-key main -o linked.html --manifest linked.render.json
vizir schema selection-links
vizir schema interaction-linked
vizir capabilities html-linked
```

The linked profile requires `--selection-links`; other profiles and static
formats reject it. The file is read as a bounded regular local file. Its path
is never embedded or resolved from JSON. Input/spec/output/manifest aliases,
including hard links and symlinks, are rejected; the specification is never
rewritten. Existing staged publication and best-effort rollback remain in use.

Changing the compiled scene, document or instance requires an explicitly
updated specification. A stale digest, wrong identity or missing member is an
error; no reconciliation, dropping or rebinding is attempted. Runtime-only
updates do not change a Scene digest, but each artifact still binds its own
exact runtime payload revision.

## Selection policy

- Each group has at least two exact Scene-node IDs
- Groups must be disjoint: a node in two groups is an error
- Duplicate groups/members are errors, not silently merged
- There is no union, transitive closure, row matching or ancestry expansion
- Authoring group/member order may vary; exported groups are sorted by ASCII
  group ID, and each group's members follow Scene preorder
- Both native and JavaScript metadata readers require that canonical order

The primary is the existing `selected` reference. V2 state adds
`selected_group_id` and `linked`, with the primary excluded from `linked`.
References retain document, Scene digest, instance and exact Scene-node ID.
The immutable context signature also contains the canonical group definitions;
a state cannot be reused with different links under the same Scene digest.

Clicking a node or choosing it in the source picker makes it primary. The
remaining members of its group become linked. Selecting one of those linked
nodes rotates the primary within the same group. An ungrouped node has no
linked nodes, even when it shares a data key or name with grouped nodes.
Selecting the same primary again is idempotent.

Escape/clear removes primary and linked selection together, except that Escape
first cancels an active pan gesture as in v1. Reset affects only the camera.
Hover/focus continue inspecting the latest exact node and do not select or
propagate links. The inspector's Origin always belongs to that inspected
node, not to an inferred entity. Status and inspector text name the selected
group and linked count; the count excludes the primary. Linked IDs are bounded
and displayed as text. No selection is conveyed by color alone.

Only the primary gets the existing selected-node tab stop. Linked highlights
are classes on the exact member elements; there is no descendant selector or
additional focus stop for every linked node. An explicitly selected/linked SVG
group can geometrically outline its children without making those children
separate members. Ancestor and descendant may both be authored members, while
unlisted siblings remain unlinked.

Line/area path IDs can represent series; they do not become row selections.
Diagram edge IDs retain their existing positional component and are valid only
for the bound Scene snapshot. Geometry may have no data key. None of these
cases triggers special inference or a new data model.

## Wire and API compatibility

The new metadata format is `vizir-interaction/2`, profile
`explorer-linked-v1`. It preserves the shared identity/camera/node fields and
adds required, nonnull `link_groups`. Group objects have only `id` and
`members`; missing/null/unknown fields are rejected. The authoring spec remains
its own `vizir-selection-links/1` contract.

Rust adds `SelectionLinks::parse`, `render_linked_html(scene, options, links)`
and `render_linked_fragment(scene, options, links)`. The existing
`ExplorerOptions`, `HtmlExport`, v1 APIs and v1 metadata/schema shape are not
expanded. The existing `schema interaction` and `capabilities html` continue
to describe the narrow v1 profile. No static IR, compiler context or
ScenePatch field is added.

One shared JavaScript payload/stylesheet supports both profiles. V1 continues
to reject v2-only fields and produces the same reducer state shape, selection
results and no-op identity behavior. Its generated HTML bytes, runtime/style
hashes, CSP and manifest hashes can change when the shared runtime is revised.
This is an explicit artifact revision, not a promise of HTML byte identity.
Default HIR/MIR/Scene/SVG/PNG bytes and existing schema/capability contracts
remain the compatibility baseline.

Older self-contained v1 HTML keeps its original module, stylesheet and CSP.
Do not transplant its metadata into a different payload. For a page with v1
and v2 fragments, export both with the same current runtime, use distinct
instance keys, and include `runtime_script()`/`STYLESHEET` once. The shared
registry rejects conflicting payload revisions exactly as before. There is
no cross-instance selection bus. Mount/dispose, pointer capture, camera
controls and document-level duplicate-instance checks remain shared.

## Bounds and security

The original explorer's iterative scene, string, path, depth, metadata and HTML
limits still apply. Links add:

- Authoring specification: at most 1 MiB, strict JSON with duplicate keys rejected
- 1–256 groups, IDs matching `[a-z][a-z0-9-]{0,63}`
- 2–256 distinct members per group, at most 4,096 total memberships
- Each exact member ID retains the 4,096 UTF-8-byte source-string limit
- Compact serialized link groups: at most 1 MiB, also checked by both v2 readers
- Added group/member strings count toward the existing aggregate 4 MiB limit
- Raw and normalized metadata stay within 4 MiB; omitted lineage arrays and
  nullable data keys are accounted for before JavaScript normalization/copying

V2 also bounds qualified-reference expansion and retained state at 4 MiB.
Let B be the compact escaped UTF-8 JSON size of
`{document_id, scene_sha256, instance_key}`, and R(n) be
`B + 17 + size(JSON(scene_node_id))`. The sum of R over all Scene nodes must
fit. For each group, the conservative state reservation is
`B + 3 * max(R) + 2048 + sum(R(member) + 1)`, which must also fit. The three
extra references cover independent hover/focus/inspection; the group covers
primary plus linked members. Native export checks these before cloning groups,
and JavaScript checks before constructing derived references/state. V1 does
not gain these new limits.

All ceilings apply together. Conservative escaped-output reservations may
reject input below an individual ceiling. A limit failure never truncates the
linked set or silently omits requested behavior. These limits do not establish
browser responsiveness or total process-memory guarantees.

There is no authored code, expression, URL action, arbitrary HTML, dynamic
loader, network resource or permission expansion. Only the fixed shared
runtime consumes the closed link contract. CSP remains hash-based and the
DOM adapter uses textContent/classes/presentation attributes. Existing
metadata/Scene hashes bind content identities, not source authenticity.

## Verification

`just check` runs the Rust suite and the actual Node reducer suite. It includes
v1 compatibility traces, strict/canonical v2 failures, exact primary/linked
semantics, ancestor/descendant non-expansion, stale group/state rejection,
resource amplification, mixed-profile isolation and CLI path/publication
checks. The native package checker supports multiple generated fragments and
verifies shared payload/CSP identities and disjoint DOM IDs.

These are native/structural results. Real-browser input, lifecycle, touch,
focus/accessibility, CSS rendering and CSP enforcement remain a separate,
blocked acceptance gate.
