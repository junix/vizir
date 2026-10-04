use std::process::Command;

#[test]
fn tiny_and_subnormal_yaml_values_keep_visible_geometry_through_public_commands() {
    let temporary = tempfile::tempdir().unwrap();
    for (name, numbers) in [
        ("positive", ["1e-120", "2e-120", "3e-120"]),
        ("negative", ["-3e-120", "-2e-120", "-1e-120"]),
        ("signed", ["-1e-120", "0", "1e-120"]),
        ("subnormal", ["5e-324", "1e-323", "1.5e-323"]),
        ("subnormal-signed", ["-5e-324", "0", "5e-324"]),
        ("subnormal-step", ["1e-320", "2e-320", "3e-320"]),
    ] {
        let input = temporary.path().join(format!("{name}.viz.yaml"));
        let rows = numbers
            .iter()
            .enumerate()
            .map(|(i, n)| format!("      - {{id: row-{i}, x: {i}, y: {n}}}\n"))
            .collect::<String>();
        std::fs::write(&input, format!("version: \"0.1\"\nid: tiny-domains\nwidth: 640\nheight: 400\nbackground: transparent\ndatasets:\n  values:\n    key: id\n    rows:\n{rows}views:\n  - kind: chart.line\n    id: chart\n    frame: {{x: 0, y: 0, width: 640, height: 400}}\n    dataset: values\n    x: {{field: x}}\n    y: {{field: y}}\n    show_points: true\n")).unwrap();
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
                String::from_utf8_lossy(&result.stderr)
            );
            if operation == "validate" {
                continue;
            }
            let artifact = std::fs::read_to_string(output).unwrap();
            assert!(!artifact.contains("NaN") && !artifact.contains("Infinity"));
            if operation == "normalize" {
                let mir: serde_json::Value = serde_json::from_str(&artifact).unwrap();
                let domain = &mir["views"][0]["scales"][1]["domain"];
                let min = domain[0].as_f64().unwrap();
                let max = domain[1].as_f64().unwrap();
                let raw = numbers.map(|n| n.parse::<f64>().unwrap());
                assert!(min.is_finite() && max.is_finite() && min < max);
                assert!(min <= raw[0] && max >= raw[2]);
                assert!((max - min) / (raw[2] - raw[0]) <= 2.0, "{name}: {domain}");
            } else if operation == "lower" {
                let scene: serde_json::Value = serde_json::from_str(&artifact).unwrap();
                let children = scene["nodes"][0]["children"].as_array().unwrap();
                let ys = (0..3)
                    .map(|i| {
                        children
                            .iter()
                            .find(|node| node["id"] == format!("chart/point/row-{i}"))
                            .unwrap()["center"]["y"]
                            .as_f64()
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert!(ys.iter().all(|y| y.is_finite()));
                assert!(ys[0] > ys[1] && ys[1] > ys[2], "{name}: {ys:?}");
                assert!(ys[0] - ys[2] > 100.0);
                let line = children
                    .iter()
                    .find(|node| node["id"] == "chart/series/series")
                    .unwrap();
                assert!(line["bounds"]["height"].as_f64().unwrap() > 100.0);
            }
        }
    }
}
