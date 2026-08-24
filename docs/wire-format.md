# VizHIR wire format 0.1

A document declares `version`, stable `id`, output dimensions, named inline
datasets, and one or more views. Each view has an explicit frame so dashboard
composition remains semantic and deterministic.

Supported view tags:

```text
chart.scatter
chart.line
chart.bar
diagram.graph
geometry.scene
```

Charts require a dataset, stable key field, field encodings, and an explicit
frame. Diagram edges reference declared node IDs. Geometry paths use typed
commands (`move`, `line`, `cubic`, `close`) instead of SVG path strings.

Colors are portable sRGB hex values (`#RRGGBB` or `#RRGGBBAA`) plus
`transparent`. Measurements in the MVP are abstract Scene2D units mapped
one-to-one to the SVG viewport.

See `examples/` for complete executable documents. `vizir normalize` is the
canonical way to inspect expanded scales, marks, guides, layout requests, and
loss records.
