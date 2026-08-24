# Architecture and invariants

VizIR is a multi-level compiler IR with domain dialects.

```text
Agent / DSL
    -> VizHIR (semantic and editable)
    -> VizMIR (typed data/expressions/spaces, scales, marks, guides, layout requests)
    -> Scene2D (resolved coordinates, styles, bounds, provenance)
    -> capability negotiation
    -> target backend (SVG, then PNG rasterization)
```

The semantic document is the source of truth. MIR and Scene2D are deterministic
caches that may be regenerated.

## MVP invariants

1. Document, view, data item, and diagram node identifiers are stable and
   unique in their scopes.
2. Every data-driven mark has an explicit stable key field.
3. Dataset, field, node, and edge references resolve before lowering.
4. Resolved geometry contains only finite values.
5. Scene nodes retain HIR origin, data key when applicable, generating pass,
   MIR origin, data lineage, and a human-readable explanation.
6. Layout is a compiler service and completes before backend emission.
7. SVG emission never performs data lookup, scale inference, or graph layout.
8. Unsupported capabilities fail with a diagnostic instead of disappearing.
9. Transparent PNG is verified from the emitted artifact, not inferred from
   the filename.
10. Serialization order and generated IDs are deterministic.
11. Canonical expressions are pure typed ASTs; strings never carry executable
    MIR semantics.
12. Coordinate spaces and units are explicit in MIR and incompatible spatial
    types do not unify.
13. Scene patches require matching document and base revision, and patch/full
    recompute produce equal normalized Scene2D semantics.

## Non-goals for 0.1

- arbitrary JavaScript, Python, CSS, or backend code inside portable IR;
- interactive runtime state or scene patches;
- animation timelines;
- Scene3D;
- pixel-identical native typography across unrelated renderers;
- one object allocation per datum at large-data scale.

Those capabilities require explicit dialects and runtime contracts after the
static Core2D boundary has been proven. See `ir-family.md` for the complete
ownership and promotion model.
