# Explicit categorical color domains

VizHIR/MIR 0.6 and `vizir-composition/0.5` add optional ordered categorical
`domain` on scatter `color`, line `series`, area `series`, and bar `color`.
Heatmap color remains its independent numeric quantize contract.

```yaml
series:
  field: service
  domain: [Alpha, Beta, Reserved]
  palette: ["#3B6EF5", "#EB5E55", "#18A999"]
```

## Semantics

- Keys retain authored order and exact spelling, including significant spaces
  and Unicode. No sorting, trimming, normalization, or union across panels occurs
- Domain index chooses the palette index, cycling a shorter palette as before.
  An omitted or empty palette still uses the selected theme or native default
- Every observed key must be declared. Missing, null, array, and object values
  remain invalid color/series data. Ordinary scalar data keeps existing
  `value_as_key` semantics: strings are exact; numbers and booleans use their
  canonical JSON string representation (for example, `42` uses domain `"42"`)
- Declared categories absent from a panel reserve their color and appear in its
  legend, without manufacturing data rows or marks
- Existing stable IDs, keys, row order, and data lineage remain unchanged.
  Domain order is color/legend order, not series drawing order or x sorting
- MIR stores the authoritative ordered ordinal-color domain/range. Replay and
  refresh preserve it. A refresh introducing an undeclared observed key fails
- Omitting the field retains legacy sorted-observed inference exactly, including
  legacy handling of empty/control-containing category strings

## Strict new-field validation

The optional field must be absent or a non-null array of 1–256 unique strings.
Each key contains 1–16,384 UTF-8 bytes; total domain content is at most 1,048,576
UTF-8 bytes per encoding. Empty strings, C0/C1 controls, and Unicode line
separators U+2028/U+2029 are rejected only for the new explicit-domain contract.
JSON and YAML reject nonstring domain entries rather than coercing them.
Duplicate fields, duplicate keys, malformed arrays, and undeclared observations
fail before output publication. Domains may still fail ordinary legend-layout
or measured-text budgets when they cannot fit the requested frame.

`VIZ-COLOR-0001` diagnoses a typed domain's invalid shape or bounds;
`VIZ-COLOR-0002` identifies an observed key outside the declared domain.
Wire shape failures may be reported directly by deserialization.

## Compatibility and use

Old HIR 0.1–0.5 and composition 0.1–0.4 reject the new field. Their schema
branches and complete definition closures are frozen. MIR 0.6 reuses existing
ordinal-color machinery under a matching HIR/MIR version pair. The native
`compose`, `validate`, `normalize`, `lower`, `render`, and compiled
context replay/refresh paths support the feature. The CLI consumes persisted
compiled/themed envelopes for MIR replay; bare normalized MIR is replayed or
refreshed through Rust APIs, not passed directly back to the CLI.

Published provider v1 and v2 remain restricted to their original MIR 0.4 and
0.5 inputs. No provider descriptor, receipt contract, or acceptance profile is
expanded by this native feature.

```sh
vizir compose examples/composition/stable-panel-colors.compose.yaml --output stable.viz.json
vizir render stable.viz.json --format svg --output stable.svg
vizir render stable.viz.json --format png --background transparent --output stable.png
```

The example shows Beta red in both panels. The Beta-only panel has an Alpha
legend entry, but no Alpha line or points.
