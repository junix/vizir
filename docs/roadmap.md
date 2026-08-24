# Stabilized roadmap

The static Core2D slice is the compatibility base. Later phases extend it by
adding dialects and compiler services, not by making `SceneNode` understand
every domain concept.

## 0.1 — static Core2D (implemented)

- versioned YAML/JSON wire schema;
- chart, diagram, and geometry HIR dialects;
- unique IDs, stable data keys, field/reference/type validation;
- explicit scales, marks, guides, and layout requests in MIR;
- typed inline data nodes, pure expression ASTs, and explicit coordinate spaces;
- generated VizMIR, ScenePatch, and Capability JSON Schemas with drift tests;
- deterministic layered/manual layout service;
- concrete Scene2D with bounds, HIR/MIR origins, data keys/lineage, and pass provenance;
- revisioned Scene2D diff/apply with full-recompute equivalence tests;
- machine-readable backend profiles and per-node capability decisions;
- exact SVG target and alpha-verified transparent PNG;
- target-level rasterization entry in an optional artifact manifest;
- validate, normalize, lower, render, explain, and capabilities commands;
- complex executable gallery plus structural, deterministic, CLI, and alpha tests.

## 0.2 — output profiles and stronger layout

- explicit print/web output profile and units;
- intrinsic text measurement, bounded wrapping/reflow, and CJK policy;
- Graphviz provider behind the existing layout interface;
- label collision service and diagram ports;
- typed theme tokens resolved before Scene2D;
- source hashes and renderer versions in reproducibility manifests.

## 0.3 — dataflow and scale breadth

- typed filter, calculate, aggregate, bin, sort, and join operations;
- time and ordinal scales, facets, legends, heatmaps, and area marks;
- columnar `InstanceSet` storage for repeated marks;
- CSV/resource table with explicit hash and resolution policy.

## 0.4 — interactive web runtime

- expression evaluation in the browser using the already-stable typed AST;
- signals, point/interval selections, reducers, and named event spaces;
- immutable MIR plus transactional runtime state;
- browser transport for the existing ScenePatch protocol plus typed data/state patches;
- self-contained HTML runtime with SVG or Canvas target.

## 0.5 — timelines and document backends

- semantic transitions plus explicit sequence/parallel/keyframe timelines;
- native Canvas and resolved-geometry TikZ targets;
- fixed-time frame rendering and deterministic video manifests.

## Later — Scene3D

Scene3D begins only after resources, identities, capabilities, output profiles,
and runtime patches have held stable in 2D. It shares provenance and resources
but owns meshes, point clouds, cameras, materials, lights, and volume data in a
separate dialect.

## Promotion gates

A phase is promoted only when:

1. at least three materially different executable scenarios need the concept;
2. the concept has a name, owner, invariant, and explicit non-responsibility;
3. normalized and failure behavior are tested independently of pixels;
4. final artifacts are visually inspected at delivery size;
5. unsupported target behavior is diagnosed or entered in the loss ledger.
