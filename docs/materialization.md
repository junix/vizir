# Static MIR materialization

VizHIR remains the editable semantic source. MIR is an inspectable execution
plan: inline data, field schemas and supported typed expressions determine
chart marks. `instances` and `series` are checked materialization caches.
Editing a cache cannot override data or expressions.

## Execute and refresh

`vizir_compiler::build_scene` checks the executable input, recomputes chart
materialization and rejects a stale cache with `VIZ-MATERIALIZE-0002`. It builds
geometry from the recomputed values and validates the completed Scene2D before
returning it. No partial scene is returned on failure.

A Rust caller intentionally changing MIR data or expressions must call
`rematerialize_mir(&mir)`. This returns a new MIR with refreshed mark caches,
leaving the supplied MIR unchanged, including on failure. Repeated refreshes
are idempotent. All chart styles, references, versions, provenance, explicit
scale domains/ranges and other plan fields are retained.

Run the executable API example:

```sh
cargo run -p vizir-compiler --example materialize_mir -- \
  examples/chart/service-health.viz.yaml > refreshed.scene.json
```

The example multiplies the first chart's numeric value expression by 0.5,
refreshes materialization, and emits the new resolved scene. The resulting
`Scene2D` can be passed to `vizir_backend_svg::render` by a Rust consumer. The
CLI's normal `normalize`, `lower`, and `render` inputs remain HIR; this does not
add a command for importing arbitrary MIR/Scene JSON.

HIR normalization also uses this materializer. Numeric, band and color domains
are inferred from evaluated mark values rather than an independent row-to-mark
implementation. Direct MIR domains/ranges are already resolved, explicit
values: refreshing instances does not automatically re-infer them. Numeric
scales may intentionally extrapolate. Materialized category values absent from
explicit band/color domains are errors, as are duplicate category domains or
ordinal domain/range length mismatches. There is no silent blue fallback or
automatic expansion of categorical domains.

## Executable subset

The static chart executor accepts exactly:

- `Field` in the chart's declared row scope
- Finite `Int64`/`Float64`, `Bool`, and `String` literals
- Numeric `Add`, `Subtract`, and `Multiply`

Arithmetic is checked at every subtree. Int64 operations retain integer
precision and fail on overflow before coordinate conversion. Float64 and mixed
operations follow the expression's schema-inferred type, with ordinary IEEE-754
integer-to-float promotion. All Float64 results must remain finite. Numeric
coordinates are projected to f64 only at the existing chart boundary.

No string-code execution, environment/clock/network access, parameters,
signals, custom functions, filters, implicit null handling or conversions are
provided. `Divide`, comparisons, conditionals, calls, arrays/records and other
AST forms remain outside this execution subset. Their existing general type
rules have not been changed. Referenced unsupported expressions fail even when
the source has no rows. Numeric and scalar binding roles are likewise checked
independently of row count.

Inline chart sources must declare `deterministic: true` and
`update_mode: replace`; an incremental flag does not create an executing
incremental dataflow runtime. Only source inputs actually used by charts must
satisfy the static execution subset. All stored recursive values are still
preflighted for safe traversal/refresh.

Runtime source rows must conform to their declared schema. Extra fields,
wrong numeric types and missing required fields are diagnosed. Unused composite
metadata and missing/null `Option` fields remain valid. Referenced chart fields
must be supported non-null scalars; null does not become zero or drop a row.

## Identity and deterministic order

`key_expression` must directly reference the source schema's key field. Stable
keys retain the existing scalar stringification, including exact integer keys
above 2^53; they must be present and unique. Two source keys that stringify to
the same value are duplicates. There is no generated/rekeyed identity policy.

Scatter and bar caches retain source-row order. Bar categories must remain
unique. Line groups sort by group key; points sort by the evaluated numeric
`order_expression`, then datum key. Int64 ordering stays exact before f64
coordinate projection; Float64 ordering uses total ordering, including signed
zero. One group must have a uniform color category. Raw inline scalar category
spelling is preserved, including mixed numeric columns and signed zero.

Existing datum IDs, chart IDs and data lineage are preserved. For lines whose
order expression differs from x, scene provenance names that order expression
rather than claiming x order. Refresh preserves authored MIR provenance; it
does not rewrite narrative explanations supplied by the caller.

## Bounds and precision

`MaterializationLimits` is accepted by `rematerialize_mir_with_limits` and
`build_scene_with_limits`. Defaults are:

- `max_expression_depth: 64`
- `max_expression_nodes: 65_536`
- `max_evaluation_steps: 10_000_000`

The depth setting also limits recursively stored schemas, data values and
geometry. Callers can tighten it; 64 remains the hard recursive safety ceiling.
The expression-node limit covers all stored expressions, including unreferenced
ones that refresh would clone. Expression, schema, data and geometry preflight
is iterative and occurs before the existing recursive MIR typing/cloning.
Repeated schema clones and referenced typing are charged before they begin.
HIR schema inference is charged for the rectangular rows-times-unioned-fields
scan, including missing cells. Work accounting is shared across all views,
sources, expression evaluation and runtime row validation in a call.

Limits reject work; they never truncate, skip rows, or return partial caches.
They bound this static execution after parsing, not input-parser allocations,
string byte lengths, every rendering/layout operation, or total process memory.
`validate_mir` in `vizir-core` remains structural/reference/type validation. It
does not itself run materialization or impose the compiler's execution bounds.

Caches compare identities, group/point order, category values and cardinality
exactly. Finite numeric cache values compare by f64 bits, including signed zero.
There is no absolute, relative or ULP epsilon: a one-ULP source/cache change must
not be mistaken for serialization noise. The workspace enables serde_json's
`float_roundtrip` decoder so supported MIR JSON round trips retain f64 bits.
External consumers using other decoders must preserve the numeric values; an
explicit refresh repairs caches when they intentionally change those values.

## Compatibility and diagnostics

No HIR/MIR fields or versions are added. Ordinary coherent 0.1/0.2 artifacts
retain their wire structure and default rendering. Previously ignored source
changes, cache-only edits, and type-checkable but non-executable plans now fail
explicitly at the compiler boundary. This is a tighter executable contract,
not a claim that every type-checkable AST is supported.

Materialization diagnostics are stable categories with chart/source/row/key
and expression context where relevant:

- `0001`: resource bound
- `0002`: stale instance/series cache
- `0003`: source value/schema mismatch
- `0004`: execution scope, channel or scalar/numeric type
- `0005`: checked integer overflow or nonfinite floating-point arithmetic
- `0006`: unsupported static expression or source mode
- `0007`: key expression, invalid key or duplicate stable key
- `0008`: duplicate bar category or inconsistent line-group color
- `0009`: explicit categorical scale cannot represent the materialized values

All use the `VIZ-MATERIALIZE-` prefix. Existing structural, type, layout and
final Scene2D diagnostics remain applicable. A successful refresh does not
claim that a separately changed frame, guide or scale range satisfies layout;
`build_scene` still checks that boundary.

This prerequisite adds no HIR `calculate` or `filter` syntax. Those require
separately versioned executable operators and clear provenance/empty-result
semantics. It also does not turn ScenePatch into a dataflow update protocol.
