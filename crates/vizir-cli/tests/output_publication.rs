use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const OLD_MANIFEST: &[u8] = b"previous successful manifest\n";

fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chart/service-health.viz.yaml")
}

fn render(format: &str, output: &Path, manifest: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    command
        .args(["render", "--format", format])
        .arg(source())
        .arg("--output")
        .arg(output)
        .arg("--manifest")
        .arg(manifest);
    command
}

fn assert_failed(output: Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    stderr
}

fn assert_clean(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vizir-")
        );
    }
}

#[test]
fn invalid_manifest_destination_preserves_old_svg_and_directory_contents() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("plot.svg");
    let manifest = temporary.path().join("report.json");
    fs::write(&output, b"old svg").unwrap();
    fs::create_dir(&manifest).unwrap();
    fs::write(manifest.join("keep"), OLD_MANIFEST).unwrap();
    assert_failed(render("svg", &output, &manifest).output().unwrap());
    assert_eq!(fs::read(output).unwrap(), b"old svg");
    assert_eq!(fs::read(manifest.join("keep")).unwrap(), OLD_MANIFEST);
    assert_clean(temporary.path());
}

#[test]
fn svg_and_json_writers_publish_fresh_contents_and_clean_staging() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("plot.svg");
    let manifest = temporary.path().join("report.json");
    fs::write(&output, b"old svg").unwrap();
    fs::write(&manifest, OLD_MANIFEST).unwrap();
    let result = render("svg", &output, &manifest).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::read_to_string(&output).unwrap().starts_with("<svg"));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(report["output"], output.to_str().unwrap());
    assert!(!fs::read_to_string(&manifest).unwrap().contains(".vizir-"));
    for command in ["normalize", "lower", "schema"] {
        fs::write(&output, b"old content").unwrap();
        let mut process = Command::new(env!("CARGO_BIN_EXE_vizir"));
        process.arg(command);
        if command == "schema" {
            process.arg("mir");
        } else {
            process.arg(source());
        }
        let result = process.arg("--output").arg(&output).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&fs::read(&output).unwrap()).unwrap();
    }
    assert_clean(temporary.path());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    fn valid_png() -> Vec<u8> {
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

    fn script(directory: &Path, name: &str, body: &str) {
        let path = directory.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn missing_noop_failed_and_invalid_renderers_preserve_old_artifact_and_manifest() {
        for mode in [
            "unavailable",
            "noop",
            "failed-partial",
            "failed-valid",
            "invalid",
            "truncated",
        ] {
            for existing in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let output = temporary.path().join("plot.png");
                let manifest = temporary.path().join("report.json");
                let old_png = valid_png();
                if existing {
                    fs::write(&output, &old_png).unwrap();
                    fs::write(&manifest, OLD_MANIFEST).unwrap();
                }
                let png = temporary.path().join("renderer-source.png");
                let mut bytes = old_png.clone();
                if mode == "truncated" {
                    bytes.truncate(bytes.len() - 4);
                }
                fs::write(&png, &bytes).unwrap();
                let body = match mode {
                    "unavailable" => None,
                    "noop" => Some("exit 0"),
                    "failed-partial" => Some("printf partial > \"$4\"; exit 7"),
                    "failed-valid" => Some("/bin/cp \"$PNG\" \"$4\"; exit 7"),
                    "invalid" => Some("printf invalid > \"$4\""),
                    "truncated" => Some("/bin/cp \"$PNG\" \"$4\""),
                    _ => unreachable!(),
                };
                if let Some(body) = body {
                    script(
                        temporary.path(),
                        "rsvg-convert",
                        &format!("if [ \"$1\" = --version ]; then exit 0; fi\n{body}"),
                    );
                }
                let result = render("png", &output, &manifest)
                    .env("PATH", temporary.path())
                    .env("PNG", &png)
                    .output()
                    .unwrap();
                assert_failed(result);
                if existing {
                    assert_eq!(fs::read(&output).unwrap(), old_png, "{mode}");
                    assert_eq!(fs::read(&manifest).unwrap(), OLD_MANIFEST, "{mode}");
                } else {
                    assert!(!output.exists(), "{mode}");
                    assert!(!manifest.exists(), "{mode}");
                }
                assert_clean(temporary.path());
            }
        }
    }

    #[test]
    fn timed_out_renderer_preserves_existing_output_and_manifest() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("plot.png");
        let manifest = temporary.path().join("report.json");
        let old_png = valid_png();
        fs::write(&output, &old_png).unwrap();
        fs::write(&manifest, OLD_MANIFEST).unwrap();
        script(
            temporary.path(),
            "rsvg-convert",
            "if [ \"$1\" = --version ]; then exit 0; fi\nprintf partial > \"$4\"\nexec /bin/sleep 40",
        );
        let result = render("png", &output, &manifest)
            .env("PATH", temporary.path())
            .output()
            .unwrap();
        let stderr = assert_failed(result);
        assert!(stderr.contains("timed out after 30000 ms"), "{stderr}");
        assert_eq!(fs::read(&output).unwrap(), old_png);
        assert_eq!(fs::read(&manifest).unwrap(), OLD_MANIFEST);
        assert_clean(temporary.path());
    }

    #[test]
    fn both_renderer_routes_publish_to_the_requested_path_without_temporary_metadata() {
        for renderer in ["rsvg-convert", "magick"] {
            let temporary = tempfile::tempdir().unwrap();
            let output = temporary.path().join("plot.png");
            let manifest = temporary.path().join("report.json");
            let png = temporary.path().join("renderer-source.png");
            fs::write(&png, valid_png()).unwrap();
            fs::write(&output, b"old").unwrap();
            fs::write(&manifest, OLD_MANIFEST).unwrap();
            let destination = if renderer == "rsvg-convert" {
                "$4"
            } else {
                "${2#png:}"
            };
            script(
                temporary.path(),
                renderer,
                &format!(
                    "if [ \"$1\" = --version ]; then exit 0; fi\n/bin/cp \"$PNG\" \"{destination}\""
                ),
            );
            let result = render("png", &output, &manifest)
                .env("PATH", temporary.path())
                .env("PNG", &png)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(fs::read(&output).unwrap(), valid_png());
            let text = fs::read_to_string(&manifest).unwrap();
            let report: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(report["output"], output.to_str().unwrap());
            assert!(!text.contains(".vizir-"));
            assert_clean(temporary.path());
        }
    }

    #[test]
    fn output_and_manifest_symlinks_hardlinks_and_modes_keep_their_contract() {
        for hardlink in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let output = temporary.path().join("plot.svg");
            let manifest = temporary.path().join("report.json");
            let output_target = temporary.path().join("plot-target");
            let manifest_target = temporary.path().join("report-target");
            for (alias, target) in [(&output, &output_target), (&manifest, &manifest_target)] {
                fs::write(target, b"old").unwrap();
                fs::set_permissions(target, fs::Permissions::from_mode(0o640)).unwrap();
                if hardlink {
                    fs::hard_link(target, alias).unwrap();
                } else {
                    symlink(target, alias).unwrap();
                }
            }
            let output_inode = fs::metadata(&output).unwrap().ino();
            let manifest_inode = fs::metadata(&manifest).unwrap().ino();
            let result = render("svg", &output, &manifest).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                fs::read_to_string(&output_target)
                    .unwrap()
                    .starts_with("<svg")
            );
            serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest_target).unwrap())
                .unwrap();
            for path in [&output, &manifest, &output_target, &manifest_target] {
                assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o640);
            }
            if hardlink {
                assert_eq!(fs::metadata(&output).unwrap().ino(), output_inode);
                assert_eq!(fs::metadata(&manifest).unwrap().ino(), manifest_inode);
                assert_eq!(fs::metadata(&output).unwrap().nlink(), 2);
                assert_eq!(fs::metadata(&manifest).unwrap().nlink(), 2);
            } else {
                assert_eq!(fs::read_link(&output).unwrap(), output_target);
                assert_eq!(fs::read_link(&manifest).unwrap(), manifest_target);
            }
            assert_clean(temporary.path());
        }
    }

    #[test]
    fn unreadable_hardlink_backup_rejects_before_either_destination_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("plot.svg");
        let manifest = temporary.path().join("report.json");
        let alias = temporary.path().join("alias");
        fs::write(&output, b"old plot").unwrap();
        fs::write(&manifest, OLD_MANIFEST).unwrap();
        fs::hard_link(&manifest, &alias).unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o200)).unwrap();
        let unreadable = fs::File::open(&manifest).is_err();
        let result = render("svg", &output, &manifest).output().unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
        if unreadable {
            assert_failed(result);
            assert_eq!(fs::read(&output).unwrap(), b"old plot");
            assert_eq!(fs::read(&manifest).unwrap(), OLD_MANIFEST);
            assert_eq!(fs::read(&alias).unwrap(), OLD_MANIFEST);
        }
        assert_clean(temporary.path());
    }

    #[test]
    fn unwritable_destination_or_parent_fails_without_changing_the_pair() {
        for blocked_parent in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let output = temporary.path().join("plot.svg");
            let report_dir = temporary.path().join("reports");
            fs::create_dir(&report_dir).unwrap();
            let manifest = report_dir.join("report.json");
            fs::write(&output, b"old plot").unwrap();
            fs::write(&manifest, OLD_MANIFEST).unwrap();
            let blocked = if blocked_parent {
                &report_dir
            } else {
                &manifest
            };
            let mode = if blocked_parent { 0o500 } else { 0o400 };
            fs::set_permissions(blocked, fs::Permissions::from_mode(mode)).unwrap();
            let permission_enforced = if blocked_parent {
                fs::File::create(report_dir.join("probe")).is_err()
            } else {
                fs::OpenOptions::new().write(true).open(&manifest).is_err()
            };
            let result = render("svg", &output, &manifest).output().unwrap();
            assert_eq!(fs::metadata(blocked).unwrap().mode() & 0o777, mode);
            fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
            if permission_enforced {
                assert_failed(result);
                assert_eq!(fs::read(output).unwrap(), b"old plot");
                assert_eq!(fs::read(manifest).unwrap(), OLD_MANIFEST);
            }
            assert_clean(temporary.path());
            assert_clean(&report_dir);
        }
    }

    #[test]
    fn manifest_serialization_failure_preserves_a_non_utf8_output() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary
            .path()
            .join(OsString::from_vec(b"plot-\xff.svg".to_vec()));
        let manifest = temporary.path().join("report.json");
        fs::write(&output, b"old plot").unwrap();
        fs::write(&manifest, OLD_MANIFEST).unwrap();
        assert_failed(render("svg", &output, &manifest).output().unwrap());
        assert_eq!(fs::read(output).unwrap(), b"old plot");
        assert_eq!(fs::read(manifest).unwrap(), OLD_MANIFEST);
        assert_clean(temporary.path());
    }
}
