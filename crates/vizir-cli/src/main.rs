use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand, ValueEnum};
use tempfile::Builder;
use vizir_compiler::{compiled_mir_schema, themed_mir_schema};
use vizir_core::{
    BackendCapabilities, Color, LossRecord, LoweringFidelity, UnsupportedPolicy, VizError,
    VizResult, capability_schema, compose_versioned, composition_schema, find_scene_node,
    mir_schema, negotiate_scene, parse_versioned_composition, scene_patch_schema,
};

mod input;
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
    /// Resolve a frame-free grid composition into ordinary versioned VizHIR JSON.
    Compose {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Validate VizHIR structure, references, types, and stable identity.
    Validate {
        input: PathBuf,
        /// Opt in to canonical defaults; use a listed family or family-dark.
        #[arg(long)]
        theme: Option<String>,
        #[command(flatten)]
        text: input::TextOptions,
    },
    /// Emit canonical normalized VizMIR as JSON.
    Normalize {
        input: PathBuf,
        /// Canonical defaults for HIR; persisted context cannot be re-themed.
        #[arg(long)]
        theme: Option<String>,
        #[command(flatten)]
        text: input::TextOptions,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Emit resolved Scene2D as JSON.
    Lower {
        input: PathBuf,
        /// Canonical defaults for HIR; persisted context cannot be re-themed.
        #[arg(long)]
        theme: Option<String>,
        #[command(flatten)]
        text: input::TextOptions,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Render exact SVG or alpha-preserving PNG.
    Render {
        input: PathBuf,
        /// Canonical defaults for HIR; persisted context cannot be re-themed.
        #[arg(long)]
        theme: Option<String>,
        #[command(flatten)]
        text: input::TextOptions,
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
        /// Canonical defaults for HIR; persisted context cannot be re-themed.
        #[arg(long)]
        theme: Option<String>,
        #[command(flatten)]
        text: input::TextOptions,
        #[arg(long)]
        node: String,
    },
    /// List the fourteen canonical opt-in theme names.
    Themes,
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
    Composition,
    Mir,
    ThemedMir,
    CompiledMir,
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
        Commands::Compose { input, output } => {
            let composition = parse_versioned_composition(&input)?;
            paths::check_destinations(&input, output.as_deref(), None)?;
            let document = compose_versioned(&composition)?;
            emit_json(&document, output.as_deref())?;
        }
        Commands::Validate { input, theme, text } => {
            println!("{}", input::read(&input, theme, text)?.validate()?);
        }
        Commands::Normalize {
            input,
            output,
            theme,
            text,
        } => {
            let document = input::read(&input, theme, text)?;
            document.check_destinations(&input, output.as_deref(), None)?;
            let compilation = document.compile(true)?;
            let limit = compilation
                .mir
                .context()
                .and_then(|context| context.text.as_ref())
                .map(|_| vizir_compiler::TEXT_MAX_OUTPUT_BYTES);
            emit_json_with_limit(&compilation.mir, output.as_deref(), limit)?;
        }
        Commands::Lower {
            input,
            output,
            theme,
            text,
        } => {
            let document = input::read(&input, theme, text)?;
            document.check_destinations(&input, output.as_deref(), None)?;
            let compilation = document.compile(false)?;
            emit_json(&compilation.scene, output.as_deref())?;
        }
        Commands::Render {
            input,
            theme,
            text,
            format,
            background,
            output,
            manifest,
        } => {
            let document = input::read(&input, theme, text)?;
            document.check_destinations(&input, Some(&output), manifest.as_deref())?;
            let mut compilation = document.compile(false)?;
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
            if compilation
                .mir
                .context()
                .is_some_and(|context| context.text.is_some())
            {
                check_output_size(svg.len(), vizir_compiler::TEXT_MAX_OUTPUT_BYTES)?;
            }
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
                let mut report = serde_json::json!({
                    "compiler": format!("vizir/{}", env!("CARGO_PKG_VERSION")),
                    "document_id": compilation.mir.inner().document_id,
                    "source_ir_version": compilation.mir.inner().source_hir_version,
                    "format": format_name(format),
                    "background": compilation.scene.background,
                    "output": serde_json::to_value(&output)?,
                    "rasterizer": rasterizer,
                    "capability_report": capability_report,
                    "losses": compilation.scene.losses.iter().chain(&target_losses).collect::<Vec<_>>(),
                });
                if let Some(theme) = compilation.mir.theme() {
                    report["theme"] = serde_json::to_value(theme)?;
                }
                if let Some(context) = compilation.mir.context() {
                    report["compilation_context"] = serde_json::to_value(context)?;
                }
                if let Some(format) = compilation.mir.context_format() {
                    report["source_context_format"] = format.into();
                }
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
                compilation.mir.inner().document_id,
                output.display(),
                match format {
                    OutputFormat::Svg => "svg",
                    OutputFormat::Png => "png",
                },
                compilation.scene.losses.len() + target_losses.len()
            );
        }
        Commands::Explain {
            input,
            node,
            theme,
            text,
        } => {
            let document = input::read(&input, theme, text)?;
            let compilation = document.compile(false)?;
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
        Commands::Themes => {
            println!("{}", vizir_compiler::THEME_NAMES.join("\n"));
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
                IrKind::Composition => composition_schema(),
                IrKind::Mir => mir_schema(),
                IrKind::ThemedMir => themed_mir_schema(),
                IrKind::CompiledMir => compiled_mir_schema(),
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
    emit_json_with_limit(value, output, None)
}

fn emit_json_with_limit<T: serde::Serialize>(
    value: &T,
    output: Option<&Path>,
    limit: Option<usize>,
) -> VizResult<()> {
    let rendered = serde_json::to_vec_pretty(value)?;
    if let Some(limit) = limit {
        check_output_size(rendered.len(), limit)?;
    }
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

fn check_output_size(bytes: usize, limit: usize) -> VizResult<()> {
    if bytes > limit {
        return Err(VizError::Diagnostic(format!(
            "VIZ-TEXT-0003: measured serialized output exceeds the {} MiB limit",
            limit / (1024 * 1024)
        )));
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

// PNG verification policy, independent of the decoder's internal allocation
// budget. Eight bytes per pixel retains full RGBA16 precision at the pixel cap.
const PNG_MAX_PIXELS: usize = 16_777_216;
const PNG_MAX_DECODED_BYTES: usize = 128 * 1024 * 1024;
const PNG_DECODER_BYTES: usize = 64 * 1024 * 1024;

fn png_pixel_count(path: &Path, width: u32, height: u32) -> VizResult<usize> {
    usize::try_from(width)
        .ok()
        .zip(usize::try_from(height).ok())
        .and_then(|(width, height)| width.checked_mul(height))
        .filter(|&pixels| pixels <= PNG_MAX_PIXELS)
        .ok_or_else(|| {
            VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0004: {} declares PNG dimensions {width}x{height} exceeding the {PNG_MAX_PIXELS} pixel verification limit",
                path.display()
            ))
        })
}

fn png_decoded_bytes(
    path: &Path,
    pixels: usize,
    color: png::ColorType,
    depth: png::BitDepth,
) -> VizResult<usize> {
    // EXPAND makes every output sample byte-aligned, including packed palette
    // and grayscale inputs and channels added by tRNS. Never size from IHDR's
    // original color type or bit depth, and never strip 16-bit precision.
    let sample_bytes = match depth {
        png::BitDepth::Eight => 1,
        png::BitDepth::Sixteen => 2,
        _ => {
            return Err(VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0001: {} has unexpected decoded PNG depth {depth:?}",
                path.display()
            )));
        }
    };
    pixels
        .checked_mul(color.samples())
        .and_then(|bytes| bytes.checked_mul(sample_bytes))
        .filter(|&bytes| bytes <= PNG_MAX_DECODED_BYTES)
        .ok_or_else(|| {
            VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0004: {} exceeds the {PNG_MAX_DECODED_BYTES} byte decoded PNG verification limit ({color:?}/{depth:?})",
                path.display()
            ))
        })
}

fn verify_png_alpha(path: &Path, expect_transparency: bool) -> VizResult<()> {
    let file = fs::File::open(path).map_err(|source| VizError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let mut decoder = png::Decoder::new_with_limits(
        file,
        png::Limits {
            bytes: PNG_DECODER_BYTES,
        },
    );
    // Expand palette/low-bit grayscale samples and tRNS to decoded channels,
    // but retain 16-bit precision: alpha 0x0001 is visible, not transparent.
    decoder.set_transformations(png::Transformations::EXPAND);
    // Reject oversized IHDRs before reading further metadata or preparing any
    // image buffers. Their resource error intentionally precedes later stream
    // errors, even when the over-budget PNG is also truncated or corrupt.
    let header = decoder.read_header_info().map_err(|error| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0001: {} is not a decodable PNG: {error}",
            path.display()
        ))
    })?;
    let pixels = png_pixel_count(path, header.width, header.height)?;
    let mut reader = decoder.read_info().map_err(|error| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0001: {} is not a decodable PNG: {error}",
            path.display()
        ))
    })?;
    let (color, depth) = reader.output_color_type();
    let buffer_size = png_decoded_bytes(path, pixels, color, depth)?;
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(buffer_size).map_err(|_| {
        VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0004: {} could not allocate {buffer_size} bytes for decoded PNG verification",
            path.display()
        ))
    })?;
    buffer.resize(buffer_size, 0);
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
    // A hex background may itself include alpha. It does not promise fully
    // opaque pixels, and rasterizers may legitimately omit an unused channel.
    // Complete decoding/finish above remains mandatory in either mode.
    if !expect_transparency {
        return Ok(());
    }
    let pixels = &buffer[..info.buffer_size()];
    let (min, max) = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => alpha_range(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| u16::from(pixel[3])),
        ),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => alpha_range(
            pixels
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pixel| u16::from(pixel[1])),
        ),
        (png::ColorType::Rgba, png::BitDepth::Sixteen) => alpha_range(
            pixels
                .as_chunks::<8>()
                .0
                .iter()
                .map(|pixel| u16::from_be_bytes([pixel[6], pixel[7]])),
        ),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen) => alpha_range(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| u16::from_be_bytes([pixel[2], pixel[3]])),
        ),
        (color_type, bit_depth) => {
            return Err(VizError::Diagnostic(format!(
                "VIZ-ARTIFACT-0002: {} uses {color_type:?}/{bit_depth:?} without a decoded alpha channel",
                path.display()
            )));
        }
    };
    if min != 0 || max == 0 {
        return Err(VizError::Diagnostic(format!(
            "VIZ-ARTIFACT-0003: {} does not contain both transparent and visible pixels (alpha {min}..{max})",
            path.display()
        )));
    }
    Ok(())
}

fn alpha_range(alphas: impl Iterator<Item = u16>) -> (u16, u16) {
    alphas.fold((u16::MAX, 0), |(min, max), alpha| {
        (min.min(alpha), max.max(alpha))
    })
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
#[path = "../tests/support/png_fixtures.rs"]
mod png_fixtures;

#[cfg(test)]
#[path = "../tests/support/png_budget_fixtures.rs"]
mod png_budget_fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_output_limits_fail_before_publication() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("artifact.json");
        fs::write(&output, b"existing artifact").unwrap();
        let value = serde_json::json!({"payload": "text"});
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        assert!(check_output_size(bytes.len(), bytes.len()).is_ok());
        assert!(check_output_size(bytes.len() + 1, bytes.len()).is_err());
        let error = emit_json_with_limit(&value, Some(&output), Some(bytes.len() - 1)).unwrap_err();
        assert!(error.to_string().contains("VIZ-TEXT-0003"));
        assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        emit_json_with_limit(&value, Some(&output), Some(bytes.len())).unwrap();
        assert_eq!(fs::read(&output).unwrap(), bytes);
    }

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

    fn verify_bytes_with_policy(bytes: &[u8], expect_transparency: bool) -> VizResult<()> {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("artifact.png");
        fs::write(&path, bytes).unwrap();
        verify_png_alpha(&path, expect_transparency)
    }

    fn verify_bytes(bytes: &[u8]) -> VizResult<()> {
        verify_bytes_with_policy(bytes, true)
    }

    #[test]
    fn png_resource_arithmetic_checks_boundaries_and_overflow() {
        let path = Path::new("fixture.png");
        for (width, height) in [(4096, 4096), (1, 16_777_216), (16_777_216, 1)] {
            assert_eq!(
                png_pixel_count(path, width, height).unwrap(),
                PNG_MAX_PIXELS
            );
        }
        assert_eq!(
            png_pixel_count(path, 1, 16_777_215).unwrap(),
            PNG_MAX_PIXELS - 1
        );
        for (width, height) in [(4096, 4097), (1, 16_777_217), (u32::MAX, u32::MAX)] {
            let error = png_pixel_count(path, width, height)
                .unwrap_err()
                .to_string();
            assert!(error.contains("VIZ-ARTIFACT-0004"), "{error}");
        }
        for color in [
            png::ColorType::Grayscale,
            png::ColorType::GrayscaleAlpha,
            png::ColorType::Rgb,
            png::ColorType::Rgba,
        ] {
            for (depth, sample_bytes) in [(png::BitDepth::Eight, 1), (png::BitDepth::Sixteen, 2)] {
                assert_eq!(
                    png_decoded_bytes(path, PNG_MAX_PIXELS, color, depth).unwrap(),
                    PNG_MAX_PIXELS * color.samples() * sample_bytes
                );
            }
        }
        assert_eq!(
            png_decoded_bytes(
                path,
                PNG_MAX_PIXELS,
                png::ColorType::Rgba,
                png::BitDepth::Sixteen
            )
            .unwrap(),
            PNG_MAX_DECODED_BYTES
        );
        // Exercise the byte guard independently of the tighter pixel guard,
        // including multiplication overflow on both 32- and 64-bit platforms.
        for pixels in [PNG_MAX_PIXELS + 1, usize::MAX] {
            let error =
                png_decoded_bytes(path, pixels, png::ColorType::Rgba, png::BitDepth::Sixteen)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("VIZ-ARTIFACT-0004"), "{error}");
        }
    }

    #[test]
    fn png_resource_budget_uses_expanded_decoded_formats() {
        let path = Path::new("fixture.png");
        for fixture in png_fixtures::fixtures() {
            let mut decoder = png::Decoder::new(fixture.bytes.as_slice());
            decoder.set_transformations(png::Transformations::EXPAND);
            let reader = decoder.read_info().unwrap();
            let (color, depth) = reader.output_color_type();
            let pixels = png_pixel_count(path, reader.info().width, reader.info().height).unwrap();
            let bytes = png_decoded_bytes(path, pixels, color, depth).unwrap();
            assert_eq!(bytes, reader.output_buffer_size(), "{}", fixture.name);
            if reader.info().color_type == png::ColorType::Indexed {
                assert_eq!(depth, png::BitDepth::Eight);
                assert_eq!(bytes, if reader.info().trns.is_some() { 8 } else { 6 });
            }
        }
    }

    #[test]
    fn oversized_png_headers_fail_before_metadata_or_pixel_allocation() {
        for bytes in png_budget_fixtures::oversized_pngs() {
            for expect_transparency in [true, false] {
                let error = verify_bytes_with_policy(&bytes, expect_transparency)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("VIZ-ARTIFACT-0004"), "{error}");
                assert!(
                    error.contains("16777216 pixel verification limit"),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn within_budget_truncated_pixels_still_report_corrupt_artifacts() {
        for header_only in [true, false] {
            let bytes = png_budget_fixtures::declared_png(
                2,
                1,
                png::ColorType::Rgba,
                png::BitDepth::Sixteen,
                header_only,
            );
            for expect_transparency in [true, false] {
                let error = verify_bytes_with_policy(&bytes, expect_transparency)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("VIZ-ARTIFACT-0001"), "{error}");
            }
        }
    }

    #[test]
    fn real_pngs_at_the_pixel_and_byte_limits_decode_and_validate_alpha() {
        use std::io::Write;

        // Stream-encode rows instead of allocating a second full-size image.
        // The cases run sequentially: at most one 128 MiB decoded buffer exists.
        for (color, depth, row_bytes) in [
            (png::ColorType::Rgba, png::BitDepth::Eight, 4096 * 4),
            (png::ColorType::Rgba, png::BitDepth::Sixteen, 4096 * 8),
            (png::ColorType::Indexed, png::BitDepth::One, 4096 / 8),
        ] {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 4096, 4096);
                encoder.set_color(color);
                encoder.set_depth(depth);
                encoder.set_compression(png::Compression::Fast);
                encoder.set_filter(png::FilterType::NoFilter);
                if color == png::ColorType::Indexed {
                    encoder.set_palette(&[0, 0, 0, 255, 255, 255][..]);
                    encoder.set_trns(&[0, 1][..]);
                }
                let mut writer = encoder.write_header().unwrap();
                {
                    let mut stream = writer.stream_writer().unwrap();
                    let mut row = vec![0; row_bytes];
                    // Includes both zero and nonzero alpha; RGBA16 uses 0x0001.
                    *row.last_mut().unwrap() = 1;
                    for _ in 0..4096 {
                        stream.write_all(&row).unwrap();
                    }
                    stream.finish().unwrap();
                }
                writer.finish().unwrap();
            }
            assert!(bytes.len() < 1024 * 1024);
            verify_bytes(&bytes).unwrap_or_else(|error| panic!("{color:?}/{depth:?}: {error}"));
        }
    }

    #[test]
    fn decoded_alpha_formats_follow_the_background_contract() {
        for fixture in png_fixtures::fixtures() {
            let result = verify_bytes(&fixture.bytes);
            match fixture.transparent_error {
                Some(code) => {
                    let error = result.expect_err(fixture.name).to_string();
                    assert!(error.contains(code), "{}: {error}", fixture.name);
                }
                None => result.unwrap_or_else(|error| panic!("{}: {error}", fixture.name)),
            }
            verify_bytes_with_policy(&fixture.bytes, false)
                .unwrap_or_else(|error| panic!("{} with hex background: {error}", fixture.name));
        }
    }

    #[test]
    fn alpha_policy_never_bypasses_complete_stream_validation() {
        for fixture in png_fixtures::fixtures() {
            for expect_transparency in [true, false] {
                for corrupt_crc in [false, true] {
                    let mut bytes = fixture.bytes.clone();
                    if corrupt_crc {
                        *bytes.last_mut().unwrap() ^= 1;
                    } else {
                        bytes.truncate(bytes.len() - 4);
                    }
                    let error = verify_bytes_with_policy(&bytes, expect_transparency)
                        .expect_err(fixture.name)
                        .to_string();
                    assert!(
                        error.contains("VIZ-ARTIFACT-0001"),
                        "{}: {error}",
                        fixture.name
                    );
                }
            }
        }
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
