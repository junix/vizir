//! Bounded static provider, directly linked to the existing compiler and SVG backend.
use clap::{Parser, Subcommand};
use serde_json::json;
use vizir_core::VizResult;

mod paths;
mod provider_receipt;
mod provider_render;
mod publication;
mod resource_io;

const PROVIDER_ID: &str = "plot-provider-vizir";
const PROFILE: &str = "vizir-compiled-svg/1";
const PROFILE_V2: &str = "vizir-compiled-svg/2";
const PROFILE_V3: &str = "vizir-compiled-svg/3";
const PROFILE_V4: &str = "vizir-compiled-svg/4";

#[derive(Parser)]
#[command(name = PROVIDER_ID, version = version(), about = "Render exact prepared VizIR bundles")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Describe the fixed local provider contract without inspecting resources.
    Describe {
        #[arg(long)]
        json: bool,
    },
    /// Report built-in readiness only; does not validate any unprovided bundle.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Replay compiled MIR 0.4 with exact explicit resources to SVG and receipt.
    RenderCompiledSvg(Box<provider_render::Options>),
    /// Replay compiled MIR 0.5 locally, including exact optional heatmap labels.
    RenderCompiledSvgV2(Box<provider_render::Options>),
    /// Replay compiled MIR 0.7 with authored domains and measured heatmap wrapping.
    RenderCompiledSvgV3(Box<provider_render::Options>),
    /// Replay compiled MIR 0.9 with shared legends and explicit numeric plot alignment.
    RenderCompiledSvgV4(Box<provider_render::Options>),
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
        Commands::Describe { json: machine } => {
            let command: serde_json::Value =
                serde_json::from_str(include_str!("../assets/compiled-svg-command-v1.json"))?;
            let command_v2: serde_json::Value =
                serde_json::from_str(include_str!("../assets/compiled-svg-command-v2.json"))?;
            let command_v3: serde_json::Value =
                serde_json::from_str(include_str!("../assets/compiled-svg-command-v3.json"))?;
            let command_v4: serde_json::Value =
                serde_json::from_str(include_str!("../assets/compiled-svg-command-v4.json"))?;
            let description = json!({
                "schema_version":"plot-provider-vizir.describe/v1",
                "provider":{"id":PROVIDER_ID,"version":version(),"protocol_versions":[1]},
                "source":{"local_code_path":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(std::path::Path::parent).expect("workspace source root")},
                "operations":["render-compiled-svg", "render-compiled-svg-v2", "render-compiled-svg-v3", "render-compiled-svg-v4"],
                "commands":[command, command_v2,
                    {"name":"describe", "description":"Describe both fixed local compiled SVG capabilities without inspecting resources."},
                    {"name":"doctor", "description":"Check built-in local SVG readiness; validate explicit bundle resources only during rendering."}, command_v3, command_v4]
            });
            if machine {
                println!("{}", serde_json::to_string(&description)?);
            } else {
                println!(
                    "{PROVIDER_ID} {} ({PROFILE}, {PROFILE_V2}, {PROFILE_V3}, {PROFILE_V4}): render-compiled-svg, render-compiled-svg-v2, render-compiled-svg-v3, render-compiled-svg-v4",
                    version()
                );
            }
        }
        Commands::Doctor { json: machine } => {
            if machine {
                println!(
                    "{}",
                    json!({
                        "schema_version":"plot-provider-vizir.doctor/v1",
                        "provider":{"id":PROVIDER_ID,"version":version()},
                        "ok":true,
                    "available":true,
                        "checks":[{"name":"builtin-svg-renderer","ok":true}],
                        "notes":["Bundle profiles, exact fonts and inputs are validated only on render. No discovery, download or network access."]
                    })
                );
            } else {
                println!(
                    "built-in SVG renderer ready; bundle resources require render-time validation"
                );
            }
        }
        Commands::RenderCompiledSvg(options) => provider_render::run(*options)?,
        Commands::RenderCompiledSvgV2(options) => provider_render::run_v2(*options)?,
        Commands::RenderCompiledSvgV3(options) => provider_render::run_v3(*options)?,
        Commands::RenderCompiledSvgV4(options) => provider_render::run_v4(*options)?,
    }
    Ok(())
}
