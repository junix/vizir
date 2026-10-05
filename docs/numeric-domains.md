# Authored numeric position domains

VizHIR/MIR 0.7 and `vizir-composition/0.6` add optional numeric `domain` bounds
on scatter, line, and unstacked-area `x`/`y`, and bar `value` encodings.

```yaml
x: {field: time, domain: [0, 60]}
y: {field: measurement, domain: [0, 10]}
```

Use the same bounds in panels comparing the same units. Each panel retains its
own frame, labels and data. Equal bounds map equal values to equal relative
positions in equal plot ranges; equal plot geometry must also be maintained
for direct pixel alignment. There is no automatic union, shared-axis inference,
unit conversion, clipping or filtering.

## Strict bounded contract

- An authored domain has exactly two numeric finite endpoints, in strictly
  ascending order, with a finite nonzero difference. Reversed and equal bounds
  are rejected, including `[-0.0, 0.0]`. Positive subnormal spans are valid
- Bounds remain exact f64 values. The compiler does not pad, sort, round, or
  apply its inferred nice-domain policy to an authored interval
- Every numeric observation must be within the closed interval. Outliers fail
  with `VIZ-DOMAIN-0002`; nothing is clipped, extrapolated, dropped or hidden
- An area's y interval must contain its explicit baseline. A bar's value
  interval must contain zero. Violations produce `VIZ-DOMAIN-0003`
- Missing, null, nonnumeric and nonfinite data retain existing validation errors.
  Domains do not fill gaps or manufacture observations
- This applies to numeric position fields only. Bar `category` rejects numeric
  domains; heatmap category and color encodings retain their separate contracts
- Area is unstacked. Stacking options remain unsupported and rejected. Each
  series' actual y values and the baseline are checked, never a fictional total
- Default labels for authored axes preserve exact endpoints and use concise
  interior text with 6–17 significant digits as needed to distinguish distinct
  neighboring tick values, including across large magnitude differences.
  Existing `axis.number_format` remains available; ordinary label-fit and
  indistinguishable-label validation applies

The normalized linear scale stores `out_of_domain: reject`. This is the only
supported explicit policy. Replay and refresh check freshly materialized
coordinates and baselines against the stored bounds, including one-ULP
outliers. Stale caches still fail ordinary replay. A failed refresh is atomic;
recompile HIR to deliberately change a domain. Area's strictly increasing x
projection check continues to reject distinct samples that collapse after
projection through an excessively wide domain.

## Compatibility

Omission preserves inferred domains, tick labels, MIR/Scene/SVG output, and
legacy direct-MIR extrapolation. `out_of_domain` is omitted, never serialized as
null, on those scales. Older HIR 0.1–0.6 and composition 0.1–0.5 reject numeric
position domains. Older MIR versions reject the new policy; the 0.7 branch
requires a matching source HIR version. Complete published schema dependency
closures remain unchanged. The Rust FieldEncoding and Linear constructors gain
optional fields, so direct Rust struct literals need `domain: None` or
`out_of_domain: None` as applicable.

Provider v1 and v2 remain at their published 0.4/0.5 profiles and reject 0.7.
No wrapper descriptor, receipt, or execution profile is broadened.

```sh
vizir compose examples/composition/shared-numeric-domains.compose.yaml -o shared.viz.json
vizir normalize shared.viz.json -o shared.mir.json
vizir render shared.viz.json --format svg -o shared.svg
vizir render shared.viz.json --format png --background transparent -o shared.png
```

The example compares Alpha+Beta with Beta alone. Both use y=[0,10] and x=[0,1].
Beta's 7 and 8 samples appear at the same height, with the same categorical
color, while Alpha's absent series remains only in the subset's legend.
