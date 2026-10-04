//! One closed, bounded resource bundle. Paths exist only at this explicit I/O
//! boundary; compiler and renderer consume verified immutable byte buffers.
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use vizir_compiler::{
    COMPILED_MIR_FORMAT, FontResources, MAX_COMPILED_MIR_JSON_BYTES, TEXT_MAX_FONT_BYTES,
    TEXT_MAX_OUTPUT_BYTES, compile_compiled_mir, parse_compiled_mir_json, parse_text_context_json,
    parse_text_layout_context_json,
};
use vizir_core::{VizError, VizResult, negotiate_scene};

use super::provider_receipt::{InputReceipt, digest};

const MAX_PROFILE_BYTES: usize = 256 * 1024;
const MAX_PINS_BYTES: usize = 4 * 1024;
const MAX_FONT_TOTAL_BYTES: usize = 96 * 1024 * 1024;

#[derive(Debug, clap::Args)]
pub(crate) struct Options {
    pub input: PathBuf,
    #[arg(long)]
    pub text_profile: PathBuf,
    #[arg(long)]
    pub text_layout: Option<PathBuf>,
    #[arg(long = "font-1")]
    pub font_1: PathBuf,
    #[arg(long = "font-2")]
    pub font_2: Option<PathBuf>,
    #[arg(long = "font-3")]
    pub font_3: Option<PathBuf>,
    /// Closed role-to-SHA256 JSON object; every provided file requires a pin.
    #[arg(long)]
    pub resource_pins: String,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub receipt: PathBuf,
}

pub(crate) fn error(detail: impl fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-PROVIDER-0001: {detail}"))
}

#[derive(Debug)]
struct Pins(BTreeMap<String, String>);
impl<'de> Deserialize<'de> for Pins {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct PinsVisitor;
        impl<'de> Visitor<'de> for PinsVisitor {
            type Value = Pins;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a closed role-to-lowercase-SHA256 object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Pins, M::Error> {
                let mut pins = BTreeMap::new();
                while let Some((role, hash)) = map.next_entry::<String, String>()? {
                    if !matches!(
                        role.as_str(),
                        "input" | "text_profile" | "text_layout" | "font_1" | "font_2" | "font_3"
                    ) || hash.len() != 64
                        || !hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        || pins.insert(role, hash).is_some()
                    {
                        return Err(serde::de::Error::custom(
                            "unknown/duplicate role or invalid lowercase SHA256",
                        ));
                    }
                }
                Ok(Pins(pins))
            }
        }
        d.deserialize_map(PinsVisitor)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    handle: same_file::Handle,
}
impl Identity {
    fn read(path: &Path) -> VizResult<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata =
                fs::metadata(path).map_err(|e| error(format!("cannot inspect resource: {e}")))?;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                handle: same_file::Handle::from_path(path)
                    .map_err(|e| error(format!("cannot inspect resource: {e}")))?,
            })
        }
    }
}

struct Resource {
    role: &'static str,
    original: PathBuf,
    canonical: PathBuf,
    identity: Identity,
    bytes: Vec<u8>,
    sha256: String,
    limit: usize,
}
impl Resource {
    fn read(
        role: &'static str,
        path: &Path,
        directory: &Path,
        limit: usize,
        pin: &str,
    ) -> VizResult<Self> {
        let canonical = canonical_resource(path, directory)?;
        let identity = Identity::read(&canonical)?;
        let bytes = super::resource_io::read_bounded_regular_with_prefix(
            path,
            limit,
            role,
            "VIZ-PROVIDER",
        )?;
        if canonical_resource(path, directory)? != canonical || Identity::read(path)? != identity {
            return Err(error(format!("{role} identity changed during read")));
        }
        let sha256 = digest(&bytes);
        if sha256 != pin {
            return Err(error(format!(
                "{role} raw byte SHA256 does not match resource_pins"
            )));
        }
        Ok(Self {
            role,
            original: path.into(),
            canonical,
            identity,
            bytes,
            sha256,
            limit,
        })
    }

    fn recheck(&self, directory: &Path) -> VizResult<()> {
        let current = Self::read(
            self.role,
            &self.original,
            directory,
            self.limit,
            &self.sha256,
        )?;
        if current.canonical != self.canonical
            || current.identity != self.identity
            || current.bytes != self.bytes
        {
            return Err(error(format!("{} changed before publication", self.role)));
        }
        Ok(())
    }

    fn receipt(&self) -> InputReceipt {
        InputReceipt {
            role: self.role.into(),
            sha256: self.sha256.clone(),
            bytes: self.bytes.len() as u64,
        }
    }
}

fn local_path(path: &Path) -> VizResult<()> {
    let value = path.to_str().ok_or_else(|| error("paths must be UTF-8"))?;
    if value.is_empty()
        || value.len() > 4096
        || value == "-"
        || value.contains('\0')
        || value.contains("://")
    {
        return Err(error(
            "require an explicit local path of 1..4096 bytes; no URL or stdin",
        ));
    }
    Ok(())
}
fn canonical_resource(path: &Path, directory: &Path) -> VizResult<PathBuf> {
    local_path(path)?;
    let resolved = fs::canonicalize(path)
        .map_err(|e| error(format!("cannot resolve explicit resource: {e}")))?;
    if resolved.parent() != Some(directory)
        || !fs::metadata(&resolved)
            .map_err(|e| error(e.to_string()))?
            .is_file()
    {
        return Err(error(
            "every input must be a regular file in the compiled input bundle directory",
        ));
    }
    Ok(resolved)
}
fn input_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}
fn check_output_size(bytes: usize) -> VizResult<()> {
    if bytes > TEXT_MAX_OUTPUT_BYTES {
        return Err(error("SVG exceeds the 32 MiB output budget"));
    }
    Ok(())
}

pub(crate) fn run(options: Options) -> VizResult<()> {
    if options.font_3.is_some() && options.font_2.is_none() {
        return Err(error("font slots must be contiguous from font_1"));
    }
    local_path(&options.input)?;
    local_path(&options.output)?;
    local_path(&options.receipt)?;
    if options.resource_pins.len() > MAX_PINS_BYTES {
        return Err(error("resource_pins exceeds the 4 KiB budget"));
    }
    let pins: Pins = serde_json::from_str(&options.resource_pins)?;
    // Anchor the original parent's actual semantics BEFORE resolving the leaf.
    // An input symlink cannot silently choose a different resource directory.
    let parent = input_parent(&options.input);
    let directory =
        fs::canonicalize(parent).map_err(|e| error(format!("cannot resolve input parent: {e}")))?;
    let directory_identity = Identity::read(&directory)?;
    let mut files = BTreeMap::from([
        ("input", (&options.input, MAX_COMPILED_MIR_JSON_BYTES)),
        ("text_profile", (&options.text_profile, MAX_PROFILE_BYTES)),
        ("font_1", (&options.font_1, TEXT_MAX_FONT_BYTES)),
    ]);
    for (role, path, limit) in [
        ("text_layout", &options.text_layout, MAX_PROFILE_BYTES),
        ("font_2", &options.font_2, TEXT_MAX_FONT_BYTES),
        ("font_3", &options.font_3, TEXT_MAX_FONT_BYTES),
    ] {
        if let Some(path) = path {
            files.insert(role, (path, limit));
        }
    }
    if files.keys().copied().collect::<BTreeSet<_>>() != pins.0.keys().map(String::as_str).collect()
    {
        return Err(error(
            "resource_pins roles must exactly equal provided input-file roles",
        ));
    }
    let mut resources: Vec<Resource> = Vec::new();
    for (role, (path, limit)) in files {
        super::paths::check_destinations(path, Some(&options.output), Some(&options.receipt))?;
        for previous in &resources {
            super::paths::check_distinct_sources(path, &previous.original)?;
        }
        resources.push(Resource::read(
            role,
            path,
            &directory,
            limit,
            &pins.0[role],
        )?);
    }
    let resource = |role: &str| {
        resources
            .iter()
            .find(|r| r.role == role)
            .expect("provided role")
    };
    // Native strict decoding runs first, preserving its duplicate/unknown-field
    // protections. Its nullable contexts are narrower in this provider profile.
    let mir = parse_compiled_mir_json(&resource("input").bytes)?;
    let raw: serde_json::Value = serde_json::from_slice(&resource("input").bytes)?;
    for field in ["theme", "text", "text_layout"] {
        if raw["context"]
            .get(field)
            .is_some_and(serde_json::Value::is_null)
        {
            return Err(error("null compilation context fields are unsupported"));
        }
    }
    if mir.format != COMPILED_MIR_FORMAT
        || mir.mir.version != "0.4"
        || mir.mir.source_hir_version != "0.4"
        || mir.context.theme.is_none()
        || mir.context.text.is_none()
    {
        return Err(error(
            "require compiled-MIR/1 with matching MIR/HIR 0.4, canonical theme and measured text",
        ));
    }
    let text = parse_text_context_json(&resource("text_profile").bytes)?;
    if mir.context.text.as_ref() != Some(&text) {
        return Err(error("text_profile differs from persisted context.text"));
    }
    let layout = options
        .text_layout
        .as_ref()
        .map(|_| parse_text_layout_context_json(&resource("text_layout").bytes))
        .transpose()?;
    if layout != mir.context.text_layout {
        return Err(error(
            "text_layout presence/value differs from persisted context.text_layout",
        ));
    }
    let expected_fonts: BTreeSet<_> = [&text.faces.regular, &text.faces.medium, &text.faces.bold]
        .into_iter()
        .map(|f| f.sha256.as_str())
        .collect();
    let fonts: Vec<_> = resources
        .iter()
        .filter(|r| r.role.starts_with("font_"))
        .collect();
    let actual_fonts: BTreeSet<_> = fonts.iter().map(|f| f.sha256.as_str()).collect();
    if actual_fonts != expected_fonts || actual_fonts.len() != fonts.len() {
        return Err(error(
            "distinct font slots must exactly equal the profile face SHA256 set",
        ));
    }
    if fonts.iter().map(|r| r.bytes.len()).sum::<usize>() > MAX_FONT_TOTAL_BYTES {
        return Err(error("font resources exceed the 96 MiB total budget"));
    }
    let mut font_resources = FontResources::new();
    for font in fonts {
        font_resources.insert(&font.sha256, font.bytes.clone())?;
    }
    let compilation = compile_compiled_mir(&mir, &font_resources, false)?;
    let capability_report =
        negotiate_scene(&compilation.scene, &vizir_backend_svg::capabilities())?;
    capability_report.require_accepted()?;
    let svg = vizir_backend_svg::render(&compilation.scene)?;
    check_output_size(svg.len())?;
    let receipt = super::provider_receipt::build(
        resources.iter().map(Resource::receipt).collect(),
        &compilation.mir,
        &compilation.scene,
        capability_report,
        svg.as_bytes(),
    )?;
    let output_stage = super::publication::StagedFile::new(&options.output)?;
    let receipt_stage = super::publication::StagedFile::new(&options.receipt)?;
    output_stage.write(svg.as_bytes())?;
    receipt_stage.write(&receipt)?;
    // Verify the written stages before publishing either destination.
    for (stage, expected, limit) in [
        (&output_stage, svg.as_bytes(), TEXT_MAX_OUTPUT_BYTES),
        (
            &receipt_stage,
            receipt.as_slice(),
            super::provider_receipt::MAX_RECEIPT_BYTES,
        ),
    ] {
        let actual = super::resource_io::read_bounded_regular_with_prefix(
            stage.path(),
            limit,
            "staged artifact",
            "VIZ-PROVIDER",
        )?;
        if actual != expected {
            return Err(error("staged artifact differs from verified bytes"));
        }
    }
    if fs::canonicalize(parent).map_err(|e| error(e.to_string()))? != directory
        || Identity::read(&directory)? != directory_identity
    {
        return Err(error(
            "compiled input bundle directory changed before publication",
        ));
    }
    for resource in &resources {
        resource.recheck(&directory)?;
        super::paths::check_destinations(
            &resource.original,
            Some(&options.output),
            Some(&options.receipt),
        )?;
    }
    super::publication::publish(vec![output_stage, receipt_stage])?;
    eprintln!(
        "rendered SVG with {} native loss records; full evidence is in the declared receipt",
        compilation.scene.losses.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_are_duplicate_unknown_null_and_type_strict() {
        for text in [
            "null",
            "[]",
            r#"{"input":null}"#,
            r#"{"input":12}"#,
            r#"{"input":"a","input":"b"}"#,
            r#"{"path":"/tmp/font"}"#,
        ] {
            assert!(serde_json::from_str::<Pins>(text).is_err());
        }
        let h = "a".repeat(64);
        assert!(
            serde_json::from_str::<Pins>(&format!(r#"{{"input":"{h}","input":"{h}"}}"#)).is_err()
        );
        assert!(serde_json::from_str::<Pins>(&format!(r#"{{"input":"{h}"}}"#)).is_ok());
    }
    #[test]
    fn resource_recheck_detects_raw_whitespace_and_identity_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.json");
        fs::write(&p, b"{}").unwrap();
        let r = Resource::read("input", &p, dir.path(), 20, &digest(b"{}")).unwrap();
        fs::write(&p, b"{ }").unwrap();
        assert!(r.recheck(dir.path()).is_err());
        fs::write(&p, b"{}").unwrap();
        r.recheck(dir.path()).unwrap();
        fs::rename(&p, dir.path().join("old")).unwrap();
        fs::write(&p, b"{}").unwrap();
        assert!(r.recheck(dir.path()).is_err());
    }
    #[test]
    fn output_budget_is_enforced() {
        assert!(check_output_size(TEXT_MAX_OUTPUT_BYTES).is_ok());
        assert!(check_output_size(TEXT_MAX_OUTPUT_BYTES + 1).is_err());
    }
}
