//! Explicit local import boundary. Imported output is ordinary authored source.
//! Neither the CSV spec nor persisted IR contains a file-reading operation.
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use serde::Serialize;
use sha2::{Digest, Sha256};
use vizir_core::{
    Composition, CompositionVersion, CsvImportColumn, CsvImportLimits, Dataset, Document, VizError,
    VizResult, compose_versioned, import_csv, parse_csv_import_spec, validate_document,
};

const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROVENANCE_BYTES: usize = 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 2048;

#[derive(Debug, Args)]
pub(crate) struct Options {
    /// Local UTF-8 CSV with the exact header declared in --spec.
    csv: PathBuf,
    /// Existing HIR or composition source; JSON by .json suffix, otherwise YAML.
    #[arg(long)]
    template: PathBuf,
    #[arg(long, value_enum)]
    template_kind: TemplateKind,
    /// Stable name of the one dataset to insert or explicitly replace.
    #[arg(long)]
    dataset: String,
    /// Strict vizir-csv-import/1 JSON with ordered column types and stable key.
    #[arg(long)]
    spec: PathBuf,
    /// Ordinary JSON in the same source dialect and version as the template.
    #[arg(short, long)]
    output: PathBuf,
    /// Required deterministic import receipt, published together with output.
    #[arg(long)]
    provenance: PathBuf,
    /// Require the selected dataset to exist, then replace that entire dataset.
    #[arg(long)]
    replace_dataset: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
enum TemplateKind {
    Hir,
    Composition,
}

#[derive(Serialize)]
struct ContentHash {
    sha256: String,
    bytes: usize,
}

impl ContentHash {
    fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            bytes: bytes.len(),
        }
    }
}

#[derive(Serialize)]
struct DocumentHash<'a> {
    #[serde(flatten)]
    content: ContentHash,
    kind: TemplateKind,
    source_version: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ReplacementPolicy {
    RequireAbsent,
    RequirePresent,
}

#[derive(Serialize)]
struct Replacement {
    policy: ReplacementPolicy,
    replaced_existing: bool,
}

#[derive(Serialize)]
struct Provenance<'a> {
    format: &'static str,
    importer_profile: &'static str,
    dataset: &'a str,
    key: &'a str,
    row_count: usize,
    header: &'a [String],
    columns: &'a [CsvImportColumn],
    had_utf8_bom: bool,
    replacement: Replacement,
    csv: ContentHash,
    spec: ContentHash,
    template: DocumentHash<'a>,
    output: DocumentHash<'a>,
}

pub(crate) fn run(options: Options) -> VizResult<()> {
    run_inner(options).map_err(bounded_diagnostic)
}

fn run_inner(options: Options) -> VizResult<()> {
    // Reject a typo or oversized selector before copying it into data/receipts.
    if options.dataset.is_empty()
        || options.dataset.len() > 256
        || !options
            .dataset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/'))
    {
        return Err(error(
            "VIZ-CSV-0101",
            "--dataset requires a stable ID of 1..256 ASCII letters, digits, hyphens, underscores or slashes",
        ));
    }
    for source in [&options.csv, &options.spec, &options.template] {
        crate::paths::check_destinations(source, Some(&options.output), Some(&options.provenance))?;
    }
    check_existing_destination(&options.output, MAX_OUTPUT_BYTES, "output")?;
    check_existing_destination(&options.provenance, MAX_PROVENANCE_BYTES, "provenance")?;
    let limits = CsvImportLimits::default();
    let csv_bytes = read(&options.csv, limits.max_csv_bytes, "CSV input")?;
    let spec_bytes = read(&options.spec, limits.max_spec_bytes, "CSV specification")?;
    let template_bytes = read(
        &options.template,
        crate::import_template::MAX_TEMPLATE_BYTES,
        "import template",
    )?;
    let spec = parse_csv_import_spec(&spec_bytes, &limits)?;
    let imported = import_csv(&csv_bytes, &spec, &limits)?;
    let value = crate::import_template::parse_template(
        &template_bytes,
        options
            .template
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json")),
    )?;

    // Decode structure first, but validate data references only after insertion.
    // This accepts an intentional missing target or empty placeholder dataset.
    let (output_bytes, source_version) = match options.template_kind {
        TemplateKind::Hir => {
            let mut document: Document = serde_json::from_value(value).map_err(|source| {
                error(
                    "VIZ-CSV-0101",
                    &format!("invalid HIR template shape: {source}"),
                )
            })?;
            insert_dataset(&mut document.datasets, &options, imported.dataset)?;
            validate_document(&document).map_err(|diagnostics| {
                // Do not stringify an unbounded list of downstream diagnostics.
                VizError::validation(&diagnostics.into_iter().take(8).collect::<Vec<_>>())
            })?;
            (
                serialize_limited(&document, MAX_OUTPUT_BYTES, "imported document")?,
                document.version,
            )
        }
        TemplateKind::Composition => {
            let mut composition: Composition = serde_json::from_value(value).map_err(|source| {
                error(
                    "VIZ-CSV-0101",
                    &format!("invalid composition template shape: {source}"),
                )
            })?;
            insert_dataset(&mut composition.datasets, &options, imported.dataset)?;
            // The normal resolver validates references and the resolved HIR.
            compose_versioned(&composition)?;
            let version = match composition.schema {
                CompositionVersion::V1 => "vizir-composition/0.1",
                CompositionVersion::V2 => "vizir-composition/0.2",
                CompositionVersion::V3 => "vizir-composition/0.3",
                CompositionVersion::V4 => "vizir-composition/0.4",
                CompositionVersion::V5 => "vizir-composition/0.5",
                CompositionVersion::V6 => "vizir-composition/0.6",
                CompositionVersion::V7 => "vizir-composition/0.7",
                CompositionVersion::V8 => "vizir-composition/0.8",
            };
            (
                serialize_limited(&composition, MAX_OUTPUT_BYTES, "imported composition")?,
                version.to_owned(),
            )
        }
    };
    let provenance = Provenance {
        format: "vizir-csv-provenance/1",
        importer_profile: "vizir-csv-import/1",
        dataset: &options.dataset,
        key: &spec.key,
        row_count: imported.row_count,
        header: &imported.header,
        columns: &spec.columns,
        had_utf8_bom: imported.had_utf8_bom,
        replacement: Replacement {
            policy: if options.replace_dataset {
                ReplacementPolicy::RequirePresent
            } else {
                ReplacementPolicy::RequireAbsent
            },
            replaced_existing: options.replace_dataset,
        },
        csv: ContentHash::of(&csv_bytes),
        spec: ContentHash::of(&spec_bytes),
        template: DocumentHash {
            content: ContentHash::of(&template_bytes),
            kind: options.template_kind,
            source_version: &source_version,
        },
        output: DocumentHash {
            content: ContentHash::of(&output_bytes),
            kind: options.template_kind,
            source_version: &source_version,
        },
    };
    let provenance_bytes =
        serialize_limited(&provenance, MAX_PROVENANCE_BYTES, "import provenance")?;
    // Recheck all explicit sources before staging. This is a preflight, not a
    // security claim about concurrent filesystem mutation (see publication.rs).
    for source in [&options.csv, &options.spec, &options.template] {
        crate::paths::check_destinations(source, Some(&options.output), Some(&options.provenance))?;
    }
    check_existing_destination(&options.output, MAX_OUTPUT_BYTES, "output")?;
    check_existing_destination(&options.provenance, MAX_PROVENANCE_BYTES, "provenance")?;
    let output = crate::publication::StagedFile::new(&options.output)?;
    let provenance = crate::publication::StagedFile::new(&options.provenance)?;
    output.write(&output_bytes)?;
    provenance.write(&provenance_bytes)?;
    crate::publication::publish(vec![output, provenance])?;
    println!("emitted: {}", options.output.display());
    println!("provenance: {}", options.provenance.display());
    Ok(())
}

fn read(path: &Path, limit: usize, role: &str) -> VizResult<Vec<u8>> {
    crate::input::read_bounded_regular_with_prefix(path, limit, role, "VIZ-CSV")
}

// Bound rollback storage as well as the newly serialized bytes. Generic
// publication keeps its established behavior; this is local to CSV import.
fn check_existing_destination(path: &Path, limit: usize, role: &str) -> VizResult<()> {
    let resolved = crate::paths::destination_for_write(path)?;
    match std::fs::metadata(&resolved) {
        Ok(metadata) if !metadata.is_file() => Err(error(
            "VIZ-CSV-0104",
            &format!("existing {role} destination must be a regular file"),
        )),
        Ok(metadata) if metadata.len() > limit as u64 => Err(error(
            "VIZ-CSV-0104",
            &format!("existing {role} destination exceeds its {limit} byte rollback limit"),
        )),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(VizError::Read {
            path: path.display().to_string(),
            source,
        }),
    }
}

fn insert_dataset(
    datasets: &mut BTreeMap<String, Dataset>,
    options: &Options,
    dataset: Dataset,
) -> VizResult<()> {
    let exists = datasets.contains_key(&options.dataset);
    if exists != options.replace_dataset {
        return Err(error(
            "VIZ-CSV-0102",
            if exists {
                "the selected dataset already exists; --replace-dataset is required to replace it"
            } else {
                "--replace-dataset requires the selected dataset to exist"
            },
        ));
    }
    datasets.insert(options.dataset.clone(), dataset);
    Ok(())
}

fn error(code: &str, message: &str) -> VizError {
    VizError::Diagnostic(format!("{code}: {message}"))
}

fn bounded_diagnostic(error: VizError) -> VizError {
    let message = error.to_string();
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return error;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    VizError::Diagnostic(format!("{} [diagnostic truncated]", &message[..end]))
}

/// The serializer cannot allocate the complete oversized output before failing.
fn serialize_limited<T: Serialize>(value: &T, limit: usize, role: &str) -> VizResult<Vec<u8>> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer_pretty(&mut writer, value) {
        if writer.exceeded {
            return Err(self::error(
                "VIZ-CSV-0103",
                &format!("{role} exceeds its {limit} byte serialized limit"),
            ));
        }
        return Err(error.into());
    }
    Ok(writer.bytes)
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(length) = self.bytes.len().checked_add(bytes.len()) else {
            self.exceeded = true;
            return Err(io::Error::other("serialized byte limit"));
        };
        if length > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("serialized byte limit"));
        }
        if length > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .max(1024)
                .saturating_mul(2)
                .min(self.limit)
                .max(length);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("cannot reserve bounded serialized output"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_writer_accepts_exact_size_and_stops_before_growth() {
        let value = serde_json::json!({"text": "quotes: \"\"\" and tabs\t\t"});
        let expected = serde_json::to_vec_pretty(&value).unwrap();
        assert_eq!(
            serialize_limited(&value, expected.len(), "test").unwrap(),
            expected
        );
        assert!(serialize_limited(&value, expected.len() - 1, "test").is_err());
        let mut writer = LimitedWriter {
            bytes: Vec::new(),
            limit: 3,
            exceeded: false,
        };
        writer.write_all(b"abc").unwrap();
        assert!(writer.bytes.capacity() <= 3);
        assert!(writer.write_all(b"d").is_err());
        assert_eq!(writer.bytes, b"abc");
    }

    #[test]
    fn content_hash_is_exact_raw_bytes() {
        assert_eq!(
            ContentHash::of(b"abc").sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(
            ContentHash::of(b"abc").sha256,
            ContentHash::of(b"abc\n").sha256
        );
    }

    #[test]
    fn diagnostic_truncation_retains_utf8_boundaries() {
        let error = error("VIZ-CSV-0101", &"雪".repeat(2000));
        let message = bounded_diagnostic(error).to_string();
        assert!(message.len() < MAX_DIAGNOSTIC_BYTES + 40);
        assert!(message.ends_with("[diagnostic truncated]"));
    }
}
