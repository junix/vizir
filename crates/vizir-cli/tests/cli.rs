use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vizir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}

#[test]
fn validates_and_explains_a_stable_data_node() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let validate = vizir().arg("validate").arg(&input).output().unwrap();
    assert!(validate.status.success());
    assert!(String::from_utf8_lossy(&validate.stdout).contains("service-health-dashboard"));

    let explain = vizir()
        .arg("explain")
        .arg(&input)
        .args(["--node", "latency-risk/point/gateway"])
        .output()
        .unwrap();
    assert!(explain.status.success());
    let stdout = String::from_utf8_lossy(&explain.stdout);
    assert!(stdout.contains("data-key: gateway"));
    assert!(stdout.contains("mir-node: latency-risk/marks/points"));
    assert!(stdout.contains("data-lineage: data/services"));
    assert!(stdout.contains("generated-by: build-symbol-scene"));
}

#[test]
fn renders_exact_svg_without_an_opaque_background() {
    let input = workspace().join("examples/geometry/visual-grammar.viz.yaml");
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("grammar.svg");
    let result = vizir()
        .arg("render")
        .arg(input)
        .args(["--format", "svg", "--background", "transparent", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let svg = std::fs::read_to_string(output).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(!svg.contains("width=\"100%\" height=\"100%\""));
    assert!(svg.contains("data-hir-node=\"center\""));
    assert!(svg.contains("data-mir-node=\"grammar-map/center\""));
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
    assert_eq!(report["accepted_ir"], "scene2d");
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
}
