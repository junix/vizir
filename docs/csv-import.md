# Explicit CSV import

`vizir import-csv` turns one local CSV file into an ordinary inline dataset in
one HIR or composition template. Column types and the stable key come from an
explicit `vizir-csv-import/1` JSON specification. Import is a separate authoring
step: the compiler, MIR, Scene2D and renderers do not read CSV files or follow
resource paths.

```text
CSV bytes + strict type spec + HIR template
  -> import-csv -> ordinary HIR JSON + import provenance
  -> normalize -> ordinary MIR -> Scene2D -> SVG / PNG

CSV bytes + strict type spec + composition template
  -> import-csv -> ordinary composition JSON + import provenance
  -> compose -> ordinary HIR -> the same compiler pipeline
```

Import does not introduce a new HIR, MIR, composition, Scene2D, theme or compiled
context version. It does not change old documents, their schema branches, or
outputs from commands that do not use this feature.

## Run the examples

From the repository root, with `vizir` built or installed:

```sh
vizir import-csv examples/import-csv/area.csv \
  --template examples/import-csv/area.template.yaml --template-kind hir \
  --dataset signals --spec examples/import-csv/area.spec.json \
  --output out/import-csv/area.viz.json \
  --provenance out/import-csv/area.provenance.json
vizir validate out/import-csv/area.viz.json
vizir normalize out/import-csv/area.viz.json --theme azure -o out/import-csv/area.mir.json
vizir lower out/import-csv/area.mir.json -o out/import-csv/area.scene.json
vizir render out/import-csv/area.mir.json --format svg -o out/import-csv/area.svg
vizir explain out/import-csv/area.mir.json --node signals/area/Alpha
```

The [area template](../examples/import-csv/area.template.yaml) is HIR 0.3. Its
unsorted source rows become a grouped area chart without changing row order,
keys, unused typed columns, quoted text or empty strings. Area materialization
sorts its own point cache by x; import never sorts the rows.

The [dashboard template](../examples/import-csv/dashboard.template.yaml) uses
composition 0.3. One imported dataset drives a sparse heatmap, grouped area and
line chart, alongside a semantic notes panel:

```sh
vizir import-csv examples/import-csv/checks.csv \
  --template examples/import-csv/dashboard.template.yaml \
  --template-kind composition --dataset checks \
  --spec examples/import-csv/checks.spec.json \
  --output out/import-csv/dashboard.compose.json \
  --provenance out/import-csv/dashboard.provenance.json
vizir compose out/import-csv/dashboard.compose.json -o out/import-csv/dashboard.viz.json
vizir normalize out/import-csv/dashboard.viz.json --theme azure -o out/import-csv/dashboard.mir.json
vizir render out/import-csv/dashboard.mir.json --format png --background transparent \
  -o out/import-csv/dashboard.png
vizir explain out/import-csv/dashboard.mir.json --node coverage/cell/jobs-mon
```

Seven observed CSV rows produce seven heatmap cells. Jobs / Mon is an observed
zero and keeps its keyed cell. Five category pairs, including every Thursday
pair, remain empty. The int64 `batch`, bool `verified`, and string `note` columns
remain in the dataset even though the charts do not bind them. No aggregation,
missing-pair filling or type inference is performed.

CLI replay uses an ordinary persisted themed or compiled MIR envelope. The
examples select `--theme azure` when normalizing for replay; bare MIR is an
inspectable artifact and a core API input, not a separate CLI input format.

The executable [run.sh](../examples/import-csv/run.sh) runs both examples through
validation, normalization, exact direct-versus-MIR Scene2D replay, `explain`,
and measured chart-title wrapping with both `azure` and `azure-dark` themes. It
renders SVG and native transparent PNG, using the existing checked-in test font
resources and ordinary render manifests:

```sh
cargo build -p vizir-cli
VIZIR="$PWD/target/debug/vizir" examples/import-csv/run.sh out/import-csv
```

The script requires Bash, Python 3 and an existing supported PNG rasterizer. If
using a nondefault Cargo target directory, set `VIZIR` to that binary instead.
It does not install software or fetch fonts. Measured text remains a separate,
explicit compilation option; neither fonts nor title policies belong in the
CSV specification or import provenance. This example selects only the existing
`chart.title` role, not category or legend wrapping.

## Command and template semantics

```text
vizir import-csv CSV
  --template PATH
  --template-kind hir|composition
  --dataset NAME
  --spec PATH
  --output PATH
  --provenance PATH
  [--replace-dataset]
```

All named arguments except `--replace-dataset` are required. Both output files
are mandatory; import does not emit the document to stdout. `--output` also
accepts `-o`. The dataset name must contain 1–256 ASCII letters, digits, hyphens,
underscores or slashes, following the existing stable-ID convention.

A template with a `.json` extension, case-insensitively, is decoded as JSON;
other extensions use YAML, matching existing source-reading conventions. The
explicit template kind controls the dialect. A filename cannot silently select
HIR, composition or a newer version.

- With no replacement flag, the target dataset must be absent
- With `--replace-dataset`, the target must already exist; the entire dataset,
  including its key and all rows, is replaced
- All other datasets, views and panel definitions retain their typed semantics
- Output is canonical pretty JSON in the template's source kind and version
- HIR 0.1–0.4 and composition 0.1–0.3 retain their ordinary version rules

The bounded reader decodes template shape first. Dataset insertion happens
before ordinary HIR semantic validation, or `compose_versioned` validation for
a composition. A template may therefore refer to the dataset being inserted,
or provide a replaceable `{key: id, rows: []}` placeholder. An unrelated missing
dataset, empty dataset, invalid field binding, invalid diagram reference, or
other ordinary semantic error still rejects the import. Shape-invalid fields
cannot be repaired by replacing a dataset. The template itself need not pass
`vizir validate` before import.

Import never normalizes, lowers or renders. It does not test the final chart's
render-time layout. Run the existing validation/normalization/render pipeline
on the emitted document. Composition output still needs the explicit `compose`
step; it is not a disguised HIR document.

## Type specification

The specification is strict UTF-8 JSON, without a BOM. Every member shown below
is required. Unknown, duplicate, missing or `null` members are rejected at both
the root and column level:

```json
{
  "format": "vizir-csv-import/1",
  "columns": [
    {"name": "id", "type": "string"},
    {"name": "time", "type": "int64"},
    {"name": "value", "type": "float64"},
    {"name": "verified", "type": "bool"},
    {"name": "note", "type": "string"}
  ],
  "key": "id"
}
```

`columns` is an ordered, nonempty array. Each name is unique, nonempty and at
most 256 UTF-8 bytes. C0/C1 controls and Unicode line/paragraph separators are
forbidden in names. The CSV header must match these names exactly in this exact
order. Names are not trimmed, folded or renamed. The key names one declared
`string` or `int64` column. `float64` and `bool` columns cannot be stable keys.
There is no nullable declaration, default, null sentinel, inferred type, path,
URL, delimiter selector or schema evolution option in this profile. Declared
types are enforced during conversion and represented by non-null scalar values.
The existing `Dataset` retains no authored type-schema declaration, so
header-only data, all-null columns and nullable type declarations are
intentionally unavailable.

Values use these exact rules:

- `string`: preserve the decoded CSV text, including empty strings, spaces,
  Unicode, commas and quoted line breaks. Key strings alone must be nonempty
- `int64`: match `-?(0|[1-9][0-9]*)`, then fit exactly in signed 64 bits
- `float64`: match JSON number grammar, then produce a finite IEEE 754 binary64
  value. A nonzero mantissa that rounds to zero is rejected; representable
  subnormal values are accepted. Positive and negative floating zero remain
  distinct in the stored number representation
- `bool`: exactly `true` or `false`, in lowercase

Blank numeric or boolean cells fail. No whitespace is stripped before parsing
numeric or boolean cells. `+1`, `01`, `.5`, `1.`, `NaN`, `Infinity` and decimal
commas are not numbers in this profile. An int64 cell cannot use `1.0` or `1e0`.
Quoted cells obey the same conversion rules after CSV decoding. Literal text
such as `null` is a string only in a declared string column; it never creates a
JSON null.

Keys must be unique after typed conversion, using the existing dataset key
representation. In an int64 key column, `-0` and `0` collide. In a string key
column they remain different strings. Import preserves data row order and
string-key spelling. Numeric lexemes can canonicalize (`1e1` becomes a number,
not the original source text). Row maps use the existing lexical field order;
only the provenance header and column array preserve the original header order.

## CSV byte and record profile

Input is UTF-8, with at most one optional BOM at the very start. The BOM is
removed from the decoded header and recorded in provenance; the raw CSV hash
still includes its bytes. Later or repeated U+FEFF is literal content; only
exactly one leading BOM is consumed. A second leading BOM therefore normally
causes a header mismatch. Comma is the only delimiter. LF and CRLF record
endings may be mixed, and the final record may omit its line ending.

A quote begins a quoted cell only at its beginning. Inside a quoted cell,
`""` decodes to one double quote; commas, LF and CRLF are literal cell content.
After the closing quote, only a comma, record ending or end of input is valid.
Bare quotes, unclosed quotes, trailing characters after a closing quote and
bare CR are rejected. NUL is forbidden, including inside a quoted cell.

The mandatory first record is the header. At least one data record is required.
Blank records and ragged rows are errors. A single final line ending is allowed;
an extra empty record is not. There are no comments, ignored rows, automatic
trimming, dropped fields or recovery from malformed records.

## Resource bounds

The CLI uses regular-file reads that enforce the byte cap while reading, not
only an initial metadata-size check. An explicitly named symlink may resolve
to a regular file. Directories, FIFOs, sockets and devices are not CSV, spec or
template inputs.

The fixed profile limits are:

- CSV source: 8 MiB; JSON specification: 256 KiB; template: 8 MiB
- Data rows: 65,536; columns: 128; data cells: 1,048,576
- Each decoded cell: 64 KiB, including quoted content; names: 256 UTF-8 bytes
- Retained import content: 32 MiB
- Template tree: depth 64, 262,144 nodes, 16 MiB of expanded string content
- YAML preflight: 1,048,592 scanner tokens (`4 * 262,144 + 16`)
- Serialized output document: 64 MiB; serialized provenance: 1 MiB

The mandatory header is separately bounded by column and cell-size limits;
`data cells` counts only data rows. Retained content is a peak content-byte
charge covering spec strings, returned header and dataset key, repeated field
names in every row, string values and canonical keys retained for duplicate
checking. It excludes caller-owned raw inputs, scalar/container metadata and
allocator overhead; the other caps bound those resources independently.

Template JSON and YAML require string mapping keys, with no implicit
scalar-to-string coercion, and reject duplicate mapping keys. The root has depth
zero; each array item or mapping value adds one level. Containers, scalar values
and mapping keys each consume a node. Mapping-key bytes count toward the string
budget without adding a value-nesting level. YAML accepts one document,
rejects explicit tags, and bounds alias expansion before a tree is materialized.
YAML merge expansion is not performed: `<<` stays a literal property subject
to the ordinary typed template shape. Limits apply to expanded content, so a short alias-heavy file
cannot bypass them. The underlying YAML decoder may reject repeated aliases
sooner through its own repetition guard. Tag rejection uses the already-locked
`unsafe-libyaml` token scanner, including standard tags that `serde_yaml` would
otherwise resolve before visiting values; there is no custom YAML lexer.
This strict template boundary belongs to `import-csv`; it
does not retrofit changed parsing rules onto legacy commands.

The pure Rust API can tighten these caps, never raise them. Serialization is
bounded before either output is published. A failure must be handled as a
rejected import, not as a partially usable dataset.

## Deterministic provenance

The required JSON receipt has `format: "vizir-csv-provenance/1"` and contains:

- `importer_profile`: `vizir-csv-import/1`
- `dataset`, `key`, `row_count`, ordered `header`, ordered `columns`, and
  `had_utf8_bom`
- `replacement`: `policy` (`require_absent` or `require_present`) and
  `replaced_existing`
- `csv` and `spec`: `{sha256, bytes}` for the exact raw input files
- `template` and `output`: `{sha256, bytes, kind, source_version}`

`kind` is `hir` or `composition`. `source_version` is the HIR version string
(such as `0.3`) or the full composition discriminator (such as
`vizir-composition/0.3`). The output stamp describes the ordinary imported source,
not a subsequently composed HIR or rendered artifact. The output hash covers
the exact emitted JSON bytes. File outputs have no trailing newline, consistent
with the ordinary JSON writers.

The receipt contains no timestamps, absolute or relative paths, machine names
or resource references. Repeating the same import into different destination
paths produces the same document and receipt bytes. Formatting or BOM changes
to source files change their raw hashes even if the decoded data is equal.
Provenance is a deterministic content receipt, not a signature or proof that an
external dataset is trustworthy.

## Path safety and publication

Both destinations are checked against the CSV, specification and template,
and against each other. Canonical paths, symlink resolution and existing file
identity catch relative spellings, symlink aliases and hardlinks. A rejected
alias cannot overwrite any input. Source files are never edited.

Existing document destinations must also fit the 64 MiB output cap, and existing
provenance destinations the 1 MiB receipt cap. This bounds rollback backups,
including hardlinked files and resolved symlink targets. Oversized or nonregular
destinations are rejected before staging.

The document and receipt are serialized and staged before publication, using
the existing [paired-output publication](../README.md#output-publication)
mechanism. Validation, read, alias, serialization or staging failure preserves
old contents in both destinations, or leaves new destinations absent. A
publication failure attempts best-effort rollback; a rollback failure reports
retained recovery information. Success is reported only after both files have
been published. Normal handled failures clean their staging files; parent
directories created for output may remain.

This is not a crash-atomic, crash-durable or race-proof transaction. Concurrent
filesystem mutations, process termination and power loss are outside its
guarantee, and readers can briefly observe a mixed pair. Use separate immutable
output names when coordinating external readers that need their own atomic
version switch.

## Rust boundary and downstream workflows

The pure core API operates only on caller-supplied bytes and values:

```rust
use vizir_core::{CsvImportLimits, import_csv, parse_csv_import_spec};

let limits = CsvImportLimits::default();
let spec = parse_csv_import_spec(spec_bytes, &limits)?;
let result = import_csv(csv_bytes, &spec, &limits)?;
let dataset = result.dataset;
```

`CsvImportSpec`, `CsvImportColumn`, `CsvColumnType`, `CsvImportResult`,
`validate_csv_import_spec`, and the independent `csv_import_spec_schema()`
function are also public. Inspect that separate contract with
`vizir schema csv-import-spec`; its JSON Schema describes wire structure, while
import also enforces UTF-8 byte bounds, unique column names and key references. The core performs no filesystem I/O, renderer calls,
HTTP access or environmental type inference. File selection, template insertion,
source validation, provenance and paired publication belong to the CLI.

Once imported, ordinary HIR/compose, theme, measured-title, normalize, lower,
SVG/PNG, `explain` and ScenePatch workflows apply unchanged. Reimport with the
same row keys retains downstream stable identity. For example, changing one
observed heatmap value can replace its existing `<view>/cell/<key>` node;
`diff_scene` and `apply_scene_patch` remain the existing APIs. The importer does
not create a new patch command or persist a CSV reload operation in MIR. To
change data later, import again explicitly, or intentionally edit ordinary MIR
data and use its existing refresh rules.
