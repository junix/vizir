use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand, ValueEnum};
use tempfile::Builder;
use vizir_compiler::compile;
use vizir_core::{
    Color, LossRecord, LoweringFidelity, VizError, VizResult, find_scene_node, parse_document,
    validate_document,
};

#[derive(Debug, Parser)]
#[command(name = "vizir", version, about = "Compile semantic visualization IR")]
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
            let compilation = compile(&document)?;
            emit_json(&compilation.mir, output.as_deref())?;
        }
        Commands::Lower { input, output } => {
            let document = parse_document(&input)?;
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
            let mut compilation = compile(&document)?;
            if let Some(background) = background {
                validate_cli_color(&background)?;
                compilation.scene.background = Color(background);
            }
            let svg = vizir_backend_svg::render(&compilation.scene);
            ensure_parent(&output)?;
            let mut target_losses = Vec::new();
            match format {
                OutputFormat::Svg => write(&output, svg.as_bytes())?,
                OutputFormat::Png => {
                    render_png(
                        &svg,
                        &output,
                        compilation.scene.background.0 == "transparent",
                    )?;
                    target_losses.push(LossRecord {
                        source: "scene2d".to_owned(),
                        target: "png".to_owned(),
                        fidelity: LoweringFidelity::Rasterized,
                        reason: "vector Scene2D was rasterized after exact SVG emission".to_owned(),
                    });
                }
            }
            if let Some(manifest) = manifest {
                let report = serde_json::json!({
                    "compiler": format!("vizir/{}", env!("CARGO_PKG_VERSION")),
                    "document_id": document.id,
                    "source_ir_version": document.version,
                    "format": format_name(format),
                    "background": compilation.scene.background,
                    "output": output,
                    "rasterizer": matches!(format, OutputFormat::Png)
                        .then(available_rasterizer)
                        .flatten(),
                    "losses": target_losses,
                });
                emit_json(&report, Some(&manifest))?;
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
            if let Some(key) = &origin.data_key {
                println!("data-key: {key}");
            }
            println!("generated-by: {}", origin.generated_by);
            println!("reason: {}", origin.explanation);
        }
        Commands::Capabilities { backend } => {
            let report = match backend {
                Backend::Svg => serde_json::json!({
                    "backend": "svg",
                    "accepted_ir": "scene2d",
                    "vector_path": true,
                    "native_text": true,
                    "transparent_background": true,
                    "interaction": false,
                    "animation": false,
                    "scene3d": false,
                    "unsupported_policy": "error"
                }),
                Backend::Png => serde_json::json!({
                    "backend": "png",
                    "accepted_ir": "scene2d-through-svg",
                    "vector_path": false,
                    "native_text": false,
                    "transparent_background": true,
                    "interaction": false,
                    "animation": false,
                    "scene3d": false,
                    "fidelity": "rasterized-artifact",
                    "rasterizer": available_rasterizer()
                }),
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn emit_json<T: serde::Serialize>(value: &T, output: Option<&Path>) -> VizResult<()> {
    let rendered = serde_json::to_vec_pretty(value)?;
    if let Some(output) = output {
        ensure_parent(output)?;
        write(output, &rendered)?;
        println!("emitted: {}", output.display());
    } else {
        println!("{}", String::from_utf8_lossy(&rendered));
    }
    Ok(())
}

fn render_png(svg: &str, output: &Path, expect_transparency: bool) -> VizResult<()> {
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

    if command_exists("rsvg-convert") {
        let result = Command::new("rsvg-convert")
            .args(["--format", "png", "--output"])
            .arg(output)
            .arg(temporary.path())
            .output()
            .map_err(|source| VizError::Diagnostic(format!("VIZ-BACKEND-0001: {source}")))?;
        if result.status.success() {
            verify_png_alpha(output, expect_transparency)?;
            return Ok(());
        }
        return Err(VizError::Diagnostic(format!(
            "VIZ-BACKEND-0002: rsvg-convert failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }

    if command_exists("magick") {
        let result = Command::new("magick")
            .arg(temporary.path())
            .arg(format!("png:{}", output.display()))
            .output()
            .map_err(|source| VizError::Diagnostic(format!("VIZ-BACKEND-0003: {source}")))?;
        if result.status.success() {
            verify_png_alpha(output, expect_transparency)?;
            return Ok(());
        }
        return Err(VizError::Diagnostic(format!(
            "VIZ-BACKEND-0004: ImageMagick failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }

    Err(VizError::Diagnostic(
        "VIZ-CAP-0001: PNG output needs rsvg-convert or ImageMagick; SVG output remains available"
            .to_owned(),
    ))
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

fn available_rasterizer() -> Option<&'static str> {
    if command_exists("rsvg-convert") {
        Some("rsvg-convert")
    } else if command_exists("magick") {
        Some("imagemagick")
    } else {
        None
    }
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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

fn ensure_parent(path: &Path) -> VizResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| VizError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    Ok(())
}

fn write(path: &Path, content: &[u8]) -> VizResult<()> {
    fs::write(path, content).map_err(|source| VizError::Write {
        path: path.display().to_string(),
        source,
    })
}
