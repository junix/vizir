use std::path::Path;
use std::process::Command;

fn fixture(
    directory: &Path,
    name: &str,
    version: &str,
    width: f64,
    precision: u8,
) -> std::path::PathBuf {
    let source = serde_json::json!({
        "version": version, "id": "numeric-format", "width": width, "height": 400,
        "datasets": {"samples": {"key": "id", "rows": [
            {"id": "a", "x": 0.0, "y": 0.0}, {"id": "b", "x": 1.0, "y": 1.0}
        ]}},
        "views": [{"kind": "chart.scatter", "id": "chart", "dataset": "samples",
            "frame": {"x": 0, "y": 0, "width": width, "height": 400},
            "x": {"field": "x", "axis": {"number_format": {"notation": "fixed", "precision": precision}}},
            "y": {"field": "y"}
        }]
    });
    let path = directory.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec(&source).unwrap()).unwrap();
    path
}

#[test]
fn invalid_formats_fail_all_public_compile_paths_before_output_publication() {
    let temporary = tempfile::tempdir().unwrap();
    for (name, version, width, precision, diagnostic) in [
        ("old-version", "0.1", 800.0, 3, "VIZ-SCHEMA-0002"),
        ("precision", "0.2", 800.0, 13, "VIZ-TYPE-0107"),
        ("ambiguous", "0.2", 800.0, 0, "VIZ-FORMAT-0001"),
        ("narrow", "0.2", 220.0, 12, "VIZ-LAYOUT-0006"),
    ] {
        let input = fixture(temporary.path(), name, version, width, precision);
        for (operation, format) in [
            ("validate", None),
            ("normalize", None),
            ("lower", None),
            ("render", Some("svg")),
            ("render", Some("png")),
        ] {
            // `validate` checks HIR semantics; tick generation/layout belongs to
            // normalization and later compile paths.
            if operation == "validate" && matches!(name, "ambiguous" | "narrow") {
                continue;
            }
            for existing in [false, true] {
                let artifact = temporary.path().join("artifact");
                let manifest = temporary.path().join("manifest.json");
                if existing {
                    std::fs::write(&artifact, "previous artifact").unwrap();
                    std::fs::write(&manifest, "previous manifest").unwrap();
                }
                let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
                command.arg(operation).arg(&input);
                if operation != "validate" {
                    command.arg("--output").arg(&artifact);
                }
                if let Some(format) = format {
                    command
                        .args(["--format", format, "--background", "transparent"])
                        .arg("--manifest")
                        .arg(&manifest);
                }
                let output = command.output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!output.status.success(), "{name}: {operation} {format:?}");
                assert!(stderr.contains(diagnostic), "{name}: {operation}: {stderr}");
                assert!(output.stdout.is_empty());
                if existing {
                    assert_eq!(
                        std::fs::read_to_string(&artifact).unwrap(),
                        "previous artifact"
                    );
                    assert_eq!(
                        std::fs::read_to_string(&manifest).unwrap(),
                        "previous manifest"
                    );
                    std::fs::remove_file(&artifact).unwrap();
                    std::fs::remove_file(&manifest).unwrap();
                } else {
                    assert!(!artifact.exists());
                    assert!(!manifest.exists());
                }
            }
        }
    }
}
