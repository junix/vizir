#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/png_fixtures.rs"]
mod png_fixtures;

fn render(directory: &Path, background: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(["render", "--format", "png", "--background", background])
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/chart/service-health.viz.yaml"),
        )
        .arg("--output")
        .arg(directory.join("output.png"))
        .arg("--manifest")
        .arg(directory.join("manifest.json"))
        .env("PATH", directory)
        .env("VIZIR_ALPHA_FIXTURE", directory.join("fixture.png"))
        .output()
        .unwrap()
}

fn assert_renderer(directory: &Path, expected: &str) {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["rasterizer"], expected);
}

#[test]
fn both_renderer_routes_validate_exact_decoded_alpha_fixtures() {
    for (command, renderer, target) in [
        ("rsvg-convert", "rsvg-convert", "\"$4\""),
        ("magick", "imagemagick", "\"${2#png:}\""),
    ] {
        for fixture in png_fixtures::fixtures() {
            for background in ["transparent", "#ffffff", "#11223380"] {
                let directory = tempfile::tempdir().unwrap();
                fs::write(directory.path().join("fixture.png"), &fixture.bytes).unwrap();
                let executable = directory.path().join(command);
                fs::write(&executable, format!(
                    "#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\n/bin/cp \"$VIZIR_ALPHA_FIXTURE\" {target}\n"
                )).unwrap();
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
                let result = render(directory.path(), background);
                let stderr = String::from_utf8_lossy(&result.stderr);
                let error = fixture
                    .transparent_error
                    .filter(|_| background == "transparent");
                if let Some(code) = error {
                    assert_eq!(
                        result.status.code(),
                        Some(1),
                        "{command}, {}, {background}: {stderr}",
                        fixture.name
                    );
                    assert!(
                        stderr.contains(code),
                        "{command}, {}: {stderr}",
                        fixture.name
                    );
                    assert!(!directory.path().join("manifest.json").exists());
                } else {
                    assert!(
                        result.status.success(),
                        "{command}, {}, {background}: {stderr}",
                        fixture.name
                    );
                    assert_renderer(directory.path(), renderer);
                    assert_eq!(
                        fs::read(directory.path().join("output.png")).unwrap(),
                        fixture.bytes
                    );
                }
            }
        }
    }
}

fn installed_binary(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|path| {
            Command::new(path)
                .arg("--version")
                .output()
                .is_ok_and(|result| result.status.success())
        })
        .map(|path| fs::canonicalize(path).unwrap())
}

#[test]
fn native_imagemagick_fallback_renders_transparent_and_hex_backgrounds_when_available() {
    let Some(magick) = installed_binary("magick") else {
        eprintln!("native ImageMagick test skipped: no successful magick --version probe");
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    symlink(magick, directory.path().join("magick")).unwrap();
    if let Some(rsvg) = installed_binary("rsvg-convert") {
        // Some ImageMagick installations require rsvg-convert as their SVG
        // delegate. Keep that real delegate available for conversion while
        // making VizIR's preferred-renderer probe unsuccessful. When only
        // ImageMagick is installed, its own configured SVG support is used.
        let wrapper = directory.path().join("rsvg-convert");
        let quoted = rsvg.to_str().unwrap().replace('\'', "'\\''");
        fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then exit 1; fi\nexec '{quoted}' \"$@\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    for background in ["transparent", "#ffffff", "#11223380"] {
        let result = render(directory.path(), background);
        assert!(
            result.status.success(),
            "{background}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_renderer(directory.path(), "imagemagick");
    }
}
