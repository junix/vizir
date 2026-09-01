use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vizir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn checked_in_schema(name: &str) -> serde_json::Value {
    let path = workspace().join("schemas").join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn validates_and_explains_a_stable_data_node() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let validate = vizir().arg("validate").arg(&input).output().unwrap();
    assert!(validate.status.success());
    assert_eq!(
        stdout_of(&validate),
        "valid: service-health-dashboard (VizHIR 0.1, 2 views)\n"
    );

    let explain = vizir()
        .arg("explain")
        .arg(&input)
        .args(["--node", "latency-risk/point/gateway"])
        .output()
        .unwrap();
    assert!(explain.status.success());
    assert_eq!(
        stdout_of(&explain),
        "node: latency-risk/point/gateway\n\
         hir-node: latency-risk\n\
         mir-node: latency-risk/marks/points\n\
         data-key: gateway\n\
         data-lineage: data/services\n\
         generated-by: build-symbol-scene\n\
         reason: datum gateway mapped through scales latency-risk/x and latency-risk/y\n"
    );
}

#[test]
fn render_rejects_an_unportable_background_without_partial_output() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("unwritten.svg");
    let result = vizir()
        .arg("render")
        .arg(&input)
        .args([
            "--format",
            "svg",
            "--background",
            "rebeccapurple",
            "--output",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        stderr_of(&result),
        "VIZ-TYPE-0004: invalid background \"rebeccapurple\"; use transparent, #RRGGBB, or #RRGGBBAA\n"
    );
    assert!(!output.exists());
}

#[test]
fn explain_reports_unknown_nodes_as_diagnostics() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let result = vizir()
        .arg("explain")
        .arg(&input)
        .args(["--node", "nope/missing"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        stderr_of(&result),
        "VIZ-EXPLAIN-0001: no Scene2D node named \"nope/missing\"\n"
    );
    assert_eq!(stdout_of(&result), "");
}

#[test]
fn explain_omits_data_lines_for_non_datum_nodes() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let result = vizir()
        .arg("explain")
        .arg(&input)
        .args(["--node", "latency-risk/title"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let stdout = stdout_of(&result);
    assert_eq!(
        stdout,
        "node: latency-risk/title\n\
         hir-node: latency-risk\n\
         mir-node: latency-risk\n\
         generated-by: shape-native-text\n\
         reason: view title retained from HIR\n"
    );
    assert!(!stdout.contains("data-key:"));
    assert!(!stdout.contains("data-lineage:"));
}

#[test]
fn missing_input_is_reported_as_a_read_error() {
    let input = workspace().join("examples/chart/absent.viz.yaml");
    let result = vizir().arg("validate").arg(&input).output().unwrap();
    assert!(!result.status.success());
    let stderr = stderr_of(&result);
    assert!(stderr.contains("failed to read"));
    assert!(stderr.contains(input.to_str().unwrap()));
    assert!(stderr.contains("No such file or directory"));
    assert_eq!(stdout_of(&result), "");
}

#[test]
fn validate_reports_each_diagnostic_with_its_source_and_help() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("invalid.viz.yaml");
    std::fs::write(
        &input,
        "version: \"0.9\"\nid: bad-document\nwidth: 400\nheight: 300\nviews: []\n",
    )
    .unwrap();
    let result = vizir().arg("validate").arg(&input).output().unwrap();
    assert!(!result.status.success());
    let stderr = stderr_of(&result);
    assert!(stderr.contains("VIZ-SCHEMA-0001: unsupported VizHIR version \"0.9\" at version"));
    assert!(stderr.contains("help: use version \"0.1\" or run a schema migration"));
    assert!(stderr.contains("VIZ-VALIDATE-0003: document has no views at views"));
    assert_eq!(stdout_of(&result), "");
}

#[test]
fn renders_exact_svg_without_an_opaque_background() {
    let input = workspace().join("examples/geometry/visual-grammar.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("grammar.svg");
    let manifest = temporary.path().join("grammar.manifest.json");
    let result = vizir()
        .arg("render")
        .arg(input)
        .args(["--format", "svg", "--background", "transparent"])
        .arg("--output")
        .arg(&output)
        .arg("--manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr_of(&result));
    assert_eq!(
        stdout_of(&result),
        format!(
            "emitted: {}\nrendered: visual-grammar-map -> {} (svg, 0 loss records)\n",
            manifest.display(),
            output.display()
        )
    );
    let svg = std::fs::read_to_string(&output).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(!svg.contains("width=\"100%\" height=\"100%\""));
    assert!(svg.contains("data-hir-node=\"center\""));
    assert!(svg.contains("data-mir-node=\"grammar-map/center\""));

    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(
        report["compiler"],
        format!("vizir/{}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(report["output"], output.display().to_string());
    assert_eq!(report["document_id"], "visual-grammar-map");
    assert_eq!(report["source_ir_version"], "0.1");
    assert_eq!(report["format"], "svg");
    assert_eq!(report["background"], "transparent");
    assert_eq!(report["losses"], serde_json::json!([]));
    assert_eq!(report["rasterizer"], serde_json::Value::Null);
    let decisions = report["capability_report"]["decisions"].as_array().unwrap();
    assert!(!decisions.is_empty());
    assert!(
        decisions
            .iter()
            .all(|decision| decision["status"] == "exact")
    );
}

#[test]
fn png_has_an_alpha_capable_color_type_when_rasterizer_is_available() {
    if Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let input = workspace().join("examples/diagram/data-platform.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("platform.png");
    let manifest = temporary.path().join("platform.manifest.json");
    let result = vizir()
        .arg("render")
        .arg(input)
        .args(["--format", "png", "--background", "transparent", "--output"])
        .arg(&output)
        .arg("--manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let png = std::fs::read(output).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(
        matches!(png[25], 4 | 6),
        "PNG color type {} has no alpha",
        png[25]
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert_eq!(report["losses"][0]["fidelity"], "rasterized");
    assert_eq!(report["rasterizer"], "rsvg-convert");
    assert_eq!(report["capability_report"]["backend"], "png");
    let decisions = report["capability_report"]["decisions"].as_array().unwrap();
    assert!(
        decisions
            .iter()
            .all(|decision| decision["status"] != "error")
    );
    assert!(
        decisions
            .iter()
            .any(|decision| decision["status"] == "rasterized")
    );
}

#[test]
fn capability_profile_is_machine_readable_and_names_unsupported_features() {
    let result = vizir().args(["capabilities", "svg"]).output().unwrap();
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["backend"], "svg");
    assert_eq!(report["version"], "1");
    assert_eq!(report["accepted_ir"], "scene2d");
    assert_eq!(report["unsupported_policy"], "error");
    assert_eq!(report["limits"]["max-clip-depth"], 32);
    assert!(
        report["supports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "scene.2d.path")
    );
    assert!(
        report["unsupported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "scene.3d.mesh")
    );
    assert!(
        !report["unsupported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "target.vector")
    );
}

#[test]
fn png_capability_profile_declares_the_rasterization_pipeline() {
    let result = vizir().args(["capabilities", "png"]).output().unwrap();
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["backend"], "png");
    assert_eq!(report["accepted_ir"], "scene2d-through-svg");
    assert_eq!(report["unsupported_policy"], "error");
    assert!(
        report["supports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "scene.2d.path")
    );
    let unsupported = report["unsupported"].as_array().unwrap();
    assert!(unsupported.iter().any(|feature| feature == "target.vector"));
    assert!(unsupported.iter().any(|feature| feature == "scene.3d.mesh"));
    let lowering = report["lowering"].as_object().unwrap();
    assert_eq!(lowering.len(), 8);
    assert!(lowering.values().all(|status| status == "rasterized"));
}

#[test]
fn schema_subcommand_emits_the_checked_in_canonical_schemas() {
    let temporary = tempfile::tempdir().unwrap();
    for (ir, checked_in) in [
        ("mir", "viz-mir.schema.json"),
        ("scene-patch", "scene-patch.schema.json"),
        ("capability", "capability.schema.json"),
    ] {
        let output: PathBuf = temporary.path().join(format!("{ir}.schema.json"));
        let result = vizir()
            .args(["schema", ir, "--output"])
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success(), "{ir}: {}", stderr_of(&result));
        assert_eq!(
            stdout_of(&result),
            format!("emitted: {}\n", output.display())
        );
        let emitted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            emitted,
            checked_in_schema(checked_in),
            "{ir} schema drifted"
        );
    }
}

#[test]
fn normalize_writes_canonical_mir_and_confirms_the_emitted_path() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("mir.json");
    let result = vizir()
        .args(["normalize", "--output"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr_of(&result));
    assert_eq!(
        stdout_of(&result),
        format!("emitted: {}\n", output.display())
    );

    let mir: serde_json::Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(mir["version"], "0.1");
    assert_eq!(mir["source_hir_version"], "0.1");
    assert_eq!(mir["document_id"], "service-health-dashboard");
    assert_eq!(mir["width"], 1280.0);
    assert_eq!(mir["height"], 720.0);
    assert_eq!(mir["background"], "transparent");
    assert!(
        mir["spaces"]
            .as_object()
            .unwrap()
            .contains_key("space/document")
    );
    assert_eq!(mir["views"].as_array().unwrap().len(), 2);

    let to_stdout = vizir().arg("normalize").arg(&input).output().unwrap();
    assert!(to_stdout.status.success());
    let stdout_mir: serde_json::Value = serde_json::from_slice(&to_stdout.stdout).unwrap();
    assert_eq!(stdout_mir, mir);
}

#[test]
fn lower_writes_the_resolved_scene_and_stdout_matches_the_file() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("scene.json");
    let result = vizir()
        .args(["lower", "--output"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr_of(&result));
    assert_eq!(
        stdout_of(&result),
        format!("emitted: {}\n", output.display())
    );

    let scene: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(scene["document_id"], "service-health-dashboard");
    assert_eq!(scene["width"], 1280.0);
    assert_eq!(scene["height"], 720.0);
    assert_eq!(scene["background"], "transparent");
    assert_eq!(scene["losses"], serde_json::json!([]));
    let view_groups: Vec<&str> = scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(view_groups, ["latency-risk", "availability-ranking"]);

    let to_stdout = vizir().arg("lower").arg(&input).output().unwrap();
    assert!(to_stdout.status.success());
    let stdout_scene: serde_json::Value = serde_json::from_slice(&to_stdout.stdout).unwrap();
    assert_eq!(stdout_scene, scene);
}

#[test]
fn png_render_without_a_rasterizer_fails_without_partial_output() {
    let input = workspace().join("examples/geometry/visual-grammar.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("unrasterized.png");
    let manifest = temporary.path().join("unrasterized.manifest.json");
    let result = vizir()
        .arg("render")
        .arg(input)
        .args(["--format", "png", "--background", "transparent", "--output"])
        .arg(&output)
        .arg("--manifest")
        .arg(&manifest)
        // An empty search path hides rsvg-convert and magick so the PNG path
        // must fail on capability grounds instead of silently degrading.
        .env("PATH", temporary.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        stderr_of(&result),
        "VIZ-CAP-0001: PNG output needs rsvg-convert or ImageMagick; SVG output remains available\n"
    );
    assert!(!output.exists());
    assert!(!manifest.exists());
}

#[test]
fn background_accepts_exact_hex_lengths_and_rejects_off_by_one() {
    let input = workspace().join("examples/geometry/visual-grammar.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    for (index, (background, accepted)) in [
        ("#112233", true),
        ("#11223344", true),
        // Uppercase hex digits stay portable; only the literal "transparent"
        // is accepted, not its case variants.
        ("#A1B2C3", true),
        ("#A1B2C3D4", true),
        ("Transparent", false),
        ("#11223", false),
        ("#1122334", false),
        ("#11223G", false),
        ("1122334", false),
    ]
    .into_iter()
    .enumerate()
    {
        // Accepted backgrounds render into a nested path, proving missing
        // parent directories are created; rejected ones must leave nothing.
        let output = if accepted {
            temporary
                .path()
                .join("nested")
                .join(format!("bg-{index}.svg"))
        } else {
            temporary.path().join(format!("bg-{index}.svg"))
        };
        let manifest = temporary.path().join(format!("bg-{index}.manifest.json"));
        let result = vizir()
            .arg("render")
            .arg(&input)
            .args(["--format", "svg", "--background"])
            .arg(background)
            .arg("--output")
            .arg(&output)
            .arg("--manifest")
            .arg(&manifest)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            accepted,
            "{background}: {}",
            stderr_of(&result)
        );
        if accepted {
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
            assert_eq!(report["background"], background, "{background}");
            assert!(output.exists(), "{background}");
        } else {
            assert_eq!(
                stderr_of(&result),
                format!(
                    "VIZ-TYPE-0004: invalid background {background:?}; \
                     use transparent, #RRGGBB, or #RRGGBBAA\n"
                ),
                "{background}"
            );
            assert!(!output.exists(), "{background}");
            assert!(!manifest.exists(), "{background}");
        }
    }
}
