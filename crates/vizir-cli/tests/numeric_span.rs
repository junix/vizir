use std::path::Path;
use std::process::{Command, Output};

fn fixture(directory: &Path, name: &str, values: &[f64]) -> std::path::PathBuf {
    let path = directory.join(format!("{name}.viz.yaml"));
    let rows = values
        .iter()
        .enumerate()
        .map(|(index, value)| format!("      - {{id: row-{index}, x: {index}, y: {value:e}}}\n"))
        .collect::<String>();
    std::fs::write(
        &path,
        format!(
            "version: \"0.1\"\nid: numeric-span\nwidth: 640\nheight: 400\nbackground: transparent\ndatasets:\n  values:\n    key: id\n    rows:\n{rows}views:\n  - kind: chart.line\n    id: line\n    frame: {{x: 0, y: 0, width: 640, height: 400}}\n    dataset: values\n    x: {{field: x}}\n    y: {{field: y}}\n    show_points: true\n"
        ),
    )
    .unwrap();
    path
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_finite_geometry(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, child) in fields {
                if matches!(key.as_str(), "x" | "y" | "width" | "height" | "radius") {
                    assert!(child.as_f64().is_some_and(f64::is_finite), "{key}: {child}");
                }
                assert_finite_geometry(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                assert_finite_geometry(child);
            }
        }
        _ => {}
    }
}

#[test]
fn overflowing_finite_span_is_rejected_by_all_public_compile_paths_before_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let input = fixture(temporary.path(), "extreme", &[-1e308, 0.0, 1e308]);
    for (operation, format) in [
        ("validate", None),
        ("normalize", None),
        ("lower", None),
        ("render", Some("svg")),
        ("render", Some("png")),
    ] {
        for existing in [false, true] {
            let output = temporary.path().join("artifact");
            let manifest = temporary.path().join("manifest.json");
            if existing {
                std::fs::write(&output, "existing artifact").unwrap();
                std::fs::write(&manifest, "existing manifest").unwrap();
            }
            let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
            command.arg(operation).arg(&input);
            if operation != "validate" {
                command.arg("--output").arg(&output);
            }
            if let Some(format) = format {
                command
                    .args(["--format", format, "--background", "transparent"])
                    .arg("--manifest")
                    .arg(&manifest);
            }
            let result = command.output().unwrap();
            assert!(!result.status.success(), "{operation} {format:?}");
            assert!(
                stderr(&result).contains("VIZ-TYPE-0106: field \"y\" has a numeric span too large for a finite linear scale at datasets.values.rows.y"),
                "{}",
                stderr(&result)
            );
            assert!(result.stdout.is_empty());
            if existing {
                assert_eq!(
                    std::fs::read_to_string(&output).unwrap(),
                    "existing artifact"
                );
                assert_eq!(
                    std::fs::read_to_string(&manifest).unwrap(),
                    "existing manifest"
                );
                std::fs::remove_file(&output).unwrap();
                std::fs::remove_file(&manifest).unwrap();
            } else {
                assert!(!output.exists());
                assert!(!manifest.exists());
            }
        }
    }
}

#[test]
fn finite_zero_signed_and_large_controls_keep_public_domains_and_finite_svg() {
    let temporary = tempfile::tempdir().unwrap();
    for (name, values) in [
        ("zero", [0.0, 0.0, 0.0]),
        ("signed", [-3.0, 0.0, 5.0]),
        ("large", [1e150, 2e150, 3e150]),
    ] {
        let input = fixture(temporary.path(), name, &values);
        for operation in ["validate", "normalize", "lower", "render"] {
            let output = temporary.path().join(format!("{name}.{operation}"));
            let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
            command.arg(operation).arg(&input);
            if operation != "validate" {
                command.arg("--output").arg(&output);
            }
            if operation == "render" {
                command.args(["--format", "svg", "--background", "transparent"]);
            }
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{name} {operation}: {}",
                stderr(&result)
            );
            if operation != "validate" {
                let artifact = std::fs::read_to_string(&output).unwrap();
                assert!(!artifact.contains("NaN"), "{name} {operation}");
                assert!(!artifact.contains("Infinity"), "{name} {operation}");
                if operation == "lower" {
                    assert_finite_geometry(&serde_json::from_str(&artifact).unwrap());
                }
                if operation == "normalize" {
                    let mir: serde_json::Value = serde_json::from_str(&artifact).unwrap();
                    let domain = &mir["views"][0]["scales"][1]["domain"];
                    if name == "zero" {
                        assert_eq!(domain, &serde_json::json!([-0.1, 0.1]));
                    } else if name == "signed" {
                        assert_eq!(domain, &serde_json::json!([-3.0, 5.0]));
                    } else {
                        assert!(domain[0].as_f64().unwrap().is_finite());
                        assert!(domain[1].as_f64().unwrap().is_finite());
                    }
                }
            }
        }
    }
}
