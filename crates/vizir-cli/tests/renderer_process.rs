#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Fixture {
    directory: tempfile::TempDir,
    png: PathBuf,
    output: PathBuf,
    manifest: PathBuf,
    calls: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let png = directory.path().join("valid.png");
        let mut encoder = png::Encoder::new(fs::File::create(&png).unwrap(), 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[0, 0, 0, 0, 10, 20, 30, 255])
            .unwrap();
        writer.finish().unwrap();
        Self {
            output: directory.path().join("plot.png"),
            manifest: directory.path().join("manifest.json"),
            calls: directory.path().join("calls.log"),
            directory,
            png,
        }
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.directory.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_vizir"))
            .args(["render", "--format", "png", "--background", "transparent"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/chart/service-health.viz.yaml"),
            )
            .arg("--output")
            .arg(&self.output)
            .arg("--manifest")
            .arg(&self.manifest)
            .env("PATH", self.directory.path())
            .env("VIZIR_FIXTURE_PNG", &self.png)
            .env("VIZIR_FIXTURE_CALLS", &self.calls)
            .output()
            .unwrap()
    }

    fn assert_failure(&self, output: Output, message: &str) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
        assert!(output.stdout.is_empty());
        assert!(
            !self.manifest.exists(),
            "a failed render must not emit a success manifest"
        );
    }
}

const RSVG_SUCCESS: &str = r#"
if [ "$1" = --version ]; then
    echo rsvg-probe >> "$VIZIR_FIXTURE_CALLS"
    echo 'rsvg-convert version test'
    exit 0
fi
echo rsvg-render >> "$VIZIR_FIXTURE_CALLS"
/bin/cp "$VIZIR_FIXTURE_PNG" "$4"
"#;
const MAGICK_SUCCESS: &str = r#"
if [ "$1" = --version ]; then
    echo magick-probe >> "$VIZIR_FIXTURE_CALLS"
    exit 0
fi
echo magick-render >> "$VIZIR_FIXTURE_CALLS"
/bin/cp "$VIZIR_FIXTURE_PNG" "${2#png:}"
"#;

#[test]
fn preferred_renderer_is_probed_once_and_manifest_uses_actual_renderer() {
    let fixture = Fixture::new();
    fixture.script("rsvg-convert", RSVG_SUCCESS);
    fixture.script("magick", MAGICK_SUCCESS);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        "rsvg-probe\nrsvg-render\n"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.manifest).unwrap()).unwrap();
    assert_eq!(manifest["rasterizer"], "rsvg-convert");
    assert_eq!(
        fs::read(fixture.output).unwrap(),
        fs::read(fixture.png).unwrap()
    );
}

#[test]
fn unavailable_preferred_renderer_preserves_imagemagick_fallback() {
    let fixture = Fixture::new();
    fixture.script(
        "rsvg-convert",
        "echo rsvg-probe >> \"$VIZIR_FIXTURE_CALLS\"; exit 9",
    );
    fixture.script("magick", MAGICK_SUCCESS);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        "rsvg-probe\nmagick-probe\nmagick-render\n"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.manifest).unwrap()).unwrap();
    assert_eq!(manifest["rasterizer"], "imagemagick");
}

#[test]
fn missing_preferred_binary_preserves_imagemagick_fallback() {
    let fixture = Fixture::new();
    fixture.script("magick", MAGICK_SUCCESS);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        "magick-probe\nmagick-render\n"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.manifest).unwrap()).unwrap();
    assert_eq!(manifest["rasterizer"], "imagemagick");
}

#[test]
fn hung_probe_is_bounded_and_reports_actionable_failure() {
    let fixture = Fixture::new();
    fixture.script("rsvg-convert", "exec /bin/sleep 10");
    let started = Instant::now();
    fixture.assert_failure(
        fixture.run(),
        "rsvg-convert probe failed: timed out after 2000 ms",
    );
    assert!(started.elapsed() < Duration::from_secs(6));
    assert!(!fixture.output.exists());
}

#[test]
fn hung_probe_can_fall_back_to_the_next_renderer() {
    let fixture = Fixture::new();
    fixture.script("rsvg-convert", "exec /bin/sleep 10");
    fixture.script("magick", MAGICK_SUCCESS);
    let started = Instant::now();
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(started.elapsed() < Duration::from_secs(6));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.manifest).unwrap()).unwrap();
    assert_eq!(manifest["rasterizer"], "imagemagick");
}

#[test]
fn failed_renderer_does_not_accept_a_valid_or_partial_png() {
    for complete in [true, false] {
        let fixture = Fixture::new();
        let write = if complete {
            "/bin/cp \"$VIZIR_FIXTURE_PNG\" \"$4\""
        } else {
            "printf partial > \"$4\""
        };
        fixture.script("rsvg-convert", &format!("if [ \"$1\" = --version ]; then exit 0; fi\n{write}\necho renderer-error >&2\nexit 17"));
        fixture.script("magick", MAGICK_SUCCESS);
        fixture.assert_failure(fixture.run(), "VIZ-BACKEND-0002: rsvg-convert failed");
        assert!(
            !fixture.calls.exists(),
            "a failed selected renderer must not fall back"
        );
    }
}

#[test]
fn successful_exit_with_missing_partial_or_truncated_png_is_rejected() {
    for mode in ["missing", "partial", "truncated"] {
        let fixture = Fixture::new();
        let body = match mode {
            "missing" => "if [ \"$1\" = --version ]; then exit 0; fi\nexit 0",
            "partial" => "if [ \"$1\" = --version ]; then exit 0; fi\nprintf partial > \"$4\"",
            "truncated" => {
                let mut bytes = fs::read(&fixture.png).unwrap();
                bytes.truncate(bytes.len() - 4);
                fs::write(&fixture.png, bytes).unwrap();
                RSVG_SUCCESS
            }
            _ => unreachable!(),
        };
        fixture.script("rsvg-convert", body);
        let output = fixture.run();
        assert_eq!(output.status.code(), Some(1), "mode: {mode}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.manifest.exists());
    }
}

#[test]
fn renderer_timeout_rejects_partial_output_and_does_not_emit_manifest() {
    let fixture = Fixture::new();
    fixture.script(
        "rsvg-convert",
        "if [ \"$1\" = --version ]; then exit 0; fi\nprintf partial > \"$4\"\nexec /bin/sleep 40",
    );
    let started = Instant::now();
    fixture.assert_failure(fixture.run(), "rsvg-convert: timed out after 30000 ms");
    assert!(started.elapsed() < Duration::from_secs(35));
    // Direct writes are intentionally not staged or rolled back by this feature.
    assert_eq!(fs::read(&fixture.output).unwrap(), b"partial");
}
