use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand, ValueEnum};
use tempfile::Builder;
use vizir_compiler::compile;
use vizir_core::{
    BackendCapabilities, Color, LossRecord, LoweringFidelity, UnsupportedPolicy, VizError,
    VizResult, capability_schema, find_scene_node, mir_schema, negotiate_scene, parse_document,
    scene_patch_schema, validate_document,
};

mod paths;
mod process;
mod publication;

#[derive(Debug, Parser)]
#[command(name = "vizir", version = version(), about = "Compile semantic visualization IR")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Validate VizHIR structure, references, types, and stable identity.
    Validate { input: PathBuf },
    /// Emit canonical normalized VizMIR as JSON.
    Normalize {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Emit resolved Scene2D as JSON.
    Lower {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Render exact SVG or alpha-preserving PNG.
    Render {
        input: PathBuf,
        #[arg(long, value_enum, default_value = "png")]
        format: OutputFormat,
        #[arg(long)]
        background: Option<String>,
        #[arg(short, long)]
        output: PathBuf,
        /// Write target fidelity and renderer metadata as JSON.
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Explain the provenance of one stable Scene2D node.
    Explain {
        input: PathBuf,
        #[arg(long)]
        node: String,
    },
    /// Report a backend's supported capability surface.
    Capabilities { backend: Backend },
    /// Emit a canonical JSON Schema for a persisted IR contract.
    Schema {
        #[arg(value_enum)]
        ir: IrKind,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Svg,
    Png,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Svg,
    Png,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IrKind {
    Mir,
    ScenePatch,
    Capability,
}

fn version() -> &'static str {
    match option_env!("PM_BUILD_SHA") {
        Some(stamp) => format!("{}+{}", env!("CARGO_PKG_VERSION"), stamp).leak(),
        None => env!("CARGO_PKG_VERSION"),
    }
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> VizResult<()> {
    match cli.command {
        Commands::Validate { input } => {
            let document = parse_document(&input)?;
            validate_document(&document)
                .map_err(|diagnostics| VizError::validation(&diagnostics))?;
            println!(
                "valid: {} (VizHIR {}, {} views)",
                document.id,
                document.version,
                document.views.len()
            );
        }
        Commands::Normalize { input, output } => {
            let document = parse_document(&input)?;
            paths::check_destinations(&input, output.as_deref(), None)?;
            let compilation = compile(&document)?;
            emit_json(&compilation.mir, output.as_deref())?;
        }
        Commands::Lower { input, output } => {
            let document = parse_document(&input)?;
            paths::check_destinations(&input, output.as_deref(), None)?;
            let compilation = compile(&document)?;
            emit_json(&compilation.scene, output.as_deref())?;
        }
        Commands::Render {
            input,
            format,
            background,
            output,
            manifest,
        } => {
            let document = parse_document(&input)?;
            paths::check_destinations(&input, Some(&output), manifest.as_deref())?;
            let mut compilation = compile(&document)?;
            if let Some(background) = background {
                validate_cli_color(&background)?;
                compilation.scene.background = Color(background);
            }
            let capabilities = match format {
                OutputFormat::Svg => vizir_backend_svg::capabilities(),
                OutputFormat::Png => png_capabilities(),
            };
            let capability_report = negotiate_scene(&compilation.scene, &capabilities)?;
            capability_report.require_accepted()?;
            let svg = vizir_backend_svg::render(&compilation.scene)?;
            let staged_output = publication::StagedFile::new(&output)?;
            let staged_manifest = manifest
                .as_deref()
                .map(publication::StagedFile::new)
                .transpose()?;
            let mut target_losses = Vec::new();
            let mut rasterizer = None;
            match format {
                OutputFormat::Svg => staged_output.write(svg.as_bytes())?,
                OutputFormat::Png => {
                    rasterizer = Some(render_png(
                        &svg,
                        staged_output.path(),
                        compilation.scene.background.0 == "transparent",
                    )?);
                    target_losses.push(LossRecord {
                        source: "scene2d".to_owned(),
                        target: "png".to_owned(),
                        fidelity: LoweringFidelity::Rasterized,
                        reason: "vector Scene2D was rasterized after exact SVG emission".to_owned(),
                    });
                }
            }
            if let Some(staged_manifest) = &staged_manifest {
                let report = serde_json::json!({
                    "compiler": format!("vizir/{}", env!("CARGO_PKG_VERSION")),
                    "document_id": document.id,
                    "source_ir_version": document.version,
                    "format": format_name(format),
                    "background": compilation.scene.background,
                    "output": serde_json::to_value(&output)?,
                    "rasterizer": rasterizer,
                    "capability_report": capability_report,
                    "losses": target_losses,
                });
                staged_manifest.write(&serde_json::to_vec_pretty(&report)?)?;
            }
            let mut files = vec![staged_output];
            files.extend(staged_manifest);
            publication::publish(files)?;
            if let Some(manifest) = &manifest {
                println!("emitted: {}", manifest.display());
            }
            println!(
                "rendered: {} -> {} ({}, {} loss records)",
                document.id,
                output.display(),
                match format {
                    OutputFormat::Svg => "svg",
                    OutputFormat::Png => "png",
                },
                compilation.scene.losses.len() + target_losses.len()
            );
        }
        Commands::Explain { input, node } => {
            let document = parse_document(&input)?;
            let compilation = compile(&document)?;
            let found = find_scene_node(&compilation.scene.nodes, &node).ok_or_else(|| {
                VizError::Diagnostic(format!("VIZ-EXPLAIN-0001: no Scene2D node named {node:?}"))
            })?;
            let origin = found.origin();
            println!("node: {}", found.id());
            println!("hir-node: {}", origin.hir_node);
            println!("mir-node: {}", origin.mir_node);
            if let Some(key) = &origin.data_key {
                println!("data-key: {key}");
            }
            if !origin.data_lineage.is_empty() {
                println!("data-lineage: {}", origin.data_lineage.join(", "));
            }
            println!("generated-by: {}", origin.generated_by);
            println!("reason: {}", origin.explanation);
        }
        Commands::Capabilities { backend } => {
            let report = match backend {
                Backend::Svg => vizir_backend_svg::capabilities(),
                Backend::Png => png_capabilities(),
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Schema { ir, output } => {
            let schema = match ir {
                IrKind::Mir => mir_schema(),
                IrKind::ScenePatch => scene_patch_schema(),
                IrKind::Capability => capability_schema(),
            };
            emit_json(&schema, output.as_deref())?;
        }
    }
    Ok(())
}

fn png_capabilities() -> BackendCapabilities {
    let svg = vizir_backend_svg::capabilities();
    BackendCapabilities {
        backend: "png".to_owned(),
        version: "1".to_owned(),
        accepted_ir: "scene2d-through-svg".to_owned(),
        supports: svg.supports,
        unsupported: BTreeSet::from([
            "animation.timeline".to_owned(),
            "interaction.pointer".to_owned(),
            "scene.3d.mesh".to_owned(),
            "target.vector".to_owned(),
        ]),
        limits: BTreeMap::new(),
        lowering: BTreeMap::from([
            (
                "scene.2d".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.circle".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.group".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.line".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.path".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.rect".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.text".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
            (
                "scene.2d.transform".to_owned(),
                vizir_core::CapabilityStatus::Rasterized,
            ),
        ]),
        unsupported_policy: UnsupportedPolicy::Error,
    }
}

fn emit_json<T: serde::Serialize>(value: &T, output: Option<&Path>) -> VizResult<()> {
    let rendered = serde_json::to_vec_pretty(value)?;
    if let Some(output) = output {
        let staged = publication::StagedFile::new(output)?;
        staged.write(&rendered)?;
        publication::publish(vec![staged])?;
        println!("emitted: {}", output.display());
    } else {
        println!("{}", String::from_utf8_lossy(&rendered));
    }
    Ok(())
}

fn render_png(svg: &str, output: &Path, expect_transparency: bool) -> VizResult<&'static str> {
    let mut temporary = Builder::new()
        .prefix("vizir-")
        .suffix(".svg")
        .tempfile()
        .map_err(|source| VizError::Write {
            path: "temporary SVG".to_owned(),
            source,
        })?;
    std::io::Write::write_all(&mut temporary, svg.as_bytes()).map_err(|source| {
        VizError::Write {
            path: temporary.path().display().to_string(),
            source,
        }
    })?;

    let rasterizer = available_rasterizer()?;
    let mut command = Command::new(rasterizer.command());
    let (start_code, failure_code) = match rasterizer {
        Rasterizer::Rsvg => {
            command
                .args(["--format", "png", "--output"])
                .arg(output)
                .arg(temporary.path());
            ("VIZ-BACKEND-0001", "VIZ-BACKEND-0002")
        }
        Rasterizer::ImageMagick => {
            command
                .arg(temporary.path())
                .arg(format!("png:{}", output.display()));
            ("VIZ-BACKEND-0003", "VIZ-BACKEND-0004")
        }
    };
    let result = process::run(&mut command, process::RENDER_TIMEOUT).map_err(|error| {
        VizError::Diagnostic(format!("{start_code}: {}: {error}", rasterizer.command()))
    })?;
    if !result.status.success() {
        return Err(VizError::Diagnostic(format!(
            "{failure_code}: {} failed ({}): {}",
            rasterizer.command(),
            result.status,
            result.diagnostics()
        )));
    }
    verify_png_alpha(output, expect_transparency)?;
    Ok(rasterizer.name())
}

fn verify_png_alpha(path: &Path, expect_transparency: bool) -> VizResult<()> {
    let file = fs::File::open(path).map_err(|source| VizError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(|error| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0001: {} is not a decodable PNG: {error}",
            path.display()
        ))
    })?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|error| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0001: {} has invalid PNG pixels: {error}",
            path.display()
        ))
    })?;
    reader.finish().map_err(|error| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0001: {} has an incomplete or invalid PNG stream: {error}",
            path.display()
        ))
    })?;
    let pixels = &buffer[..info.buffer_size()];
    let alphas = match info.color_type {
        png::ColorType::Rgba => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .collect::<Vec<_>>(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pixel| pixel[1])
            .collect::<Vec<_>>(),
        color_type => {
            return Err(VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0002: {} uses {color_type:?} without an alpha channel",
                path.display()
            )));
        }
    };
    if expect_transparency {
        let min = alphas.iter().copied().min().unwrap_or(255);
        let max = alphas.iter().copied().max().unwrap_or(0);
        if min != 0 || max == 0 {
            return Err(VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0003: {} does not contain both transparent and visible pixels (alpha {min}..{max})",
                path.display()
            )));
        }
    }
    Ok(())
}

fn format_name(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Svg => "svg",
        OutputFormat::Png => "png",
    }
}

#[derive(Clone, Copy)]
enum Rasterizer {
    Rsvg,
    ImageMagick,
}

impl Rasterizer {
    fn command(self) -> &'static str {
        match self {
            Self::Rsvg => "rsvg-convert",
            Self::ImageMagick => "magick",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Rsvg => "rsvg-convert",
            Self::ImageMagick => "imagemagick",
        }
    }
}

fn available_rasterizer() -> VizResult<Rasterizer> {
    let mut failures = Vec::new();
    for rasterizer in [Rasterizer::Rsvg, Rasterizer::ImageMagick] {
        let result = process::run(
            Command::new(rasterizer.command()).arg("--version"),
            process::PROBE_TIMEOUT,
        );
        match result {
            Ok(output) if output.status.success() => return Ok(rasterizer),
            Ok(output) => failures.push(format!(
                "{} probe failed ({}): {}",
                rasterizer.command(),
                output.status,
                output.diagnostics()
            )),
            Err(error) if error.missing => {}
            Err(error) => failures.push(format!("{} probe failed: {error}", rasterizer.command())),
        }
    }
    let details = if failures.is_empty() {
        String::new()
    } else {
        format!("\n{}", failures.join("\n"))
    };
    Err(VizError::Diagnostic(format!(
        "VIZ-CAP-0001: PNG output needs rsvg-convert or ImageMagick; SVG output remains available{details}"
    )))
}

fn validate_cli_color(value: &str) -> VizResult<()> {
    let valid_hex = matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if value == "transparent" || valid_hex {
        Ok(())
    } else {
        Err(VizError::Diagnostic(format!(
            "VIZ-TYPE-0004: invalid background {value:?}; use transparent, #RRGGBB, or #RRGGBBAA"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[0, 0, 0, 0, 10, 20, 30, 255])
                .unwrap();
            writer.finish().unwrap();
        }
        bytes
    }

    fn verify_bytes(bytes: &[u8]) -> VizResult<()> {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("artifact.png");
        fs::write(&path, bytes).unwrap();
        verify_png_alpha(&path, true)
    }

    #[test]
    fn complete_png_stream_is_accepted() {
        verify_bytes(&complete_png()).unwrap();
    }

    #[test]
    fn png_with_pixels_but_truncated_iend_is_rejected() {
        let mut bytes = complete_png();
        assert_eq!(&bytes[bytes.len() - 8..bytes.len() - 4], b"IEND");
        bytes.truncate(bytes.len() - 4);
        let error = verify_bytes(&bytes).unwrap_err().to_string();
        assert!(error.contains("VIZ-ARTIFACT-0001"), "{error}");
        assert!(error.contains("PNG stream"), "{error}");
    }

    #[test]
    fn png_with_corrupt_iend_crc_is_rejected() {
        let mut bytes = complete_png();
        *bytes.last_mut().unwrap() ^= 1;
        let error = verify_bytes(&bytes).unwrap_err().to_string();
        assert!(error.contains("VIZ-ARTIFACT-0001"), "{error}");
    }
}
