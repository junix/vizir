# VizIR repository conventions

## Responsibility

VizIR is the executable compiler contract for semantic visualization documents:
validated VizHIR, normalized VizMIR, resolved Scene2D, and explicit targets.
The `create-plot` skill owns natural-language intent and renderer routing.

## Invariants

- Edit `.viz.yaml` or Rust source; never hand-edit MIR, Scene2D, SVG, or PNG as
  canonical source.
- Preserve stable semantic IDs, data keys, provenance, and deterministic output.
- Resolve data, scales, and layout before target emission.
- Never silently omit unsupported target behavior; diagnose or record loss.
- Transparent PNG is the primary raster contract. Verify both transparent and
  visible alpha values from the real artifact.
- Keep interaction, animation, and Scene3D in separate dialect/runtime work;
  do not expand the static `SceneNode` enum to fake support.

## Gate

Run `just check`, render the affected complex example, and inspect the final
artifact at delivery size. Update the gallery only from executable examples.
