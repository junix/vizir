use std::process::Command;

use vizir_core::{BackendCapabilities, CapabilityStatus, Scene2D, negotiate_scene};

fn cli_json(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn exported_profiles_enforce_identity_and_limits_on_cli_lowered_scenes() {
    let input = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chart/service-health.viz.yaml");
    let scene: Scene2D =
        serde_json::from_value(cli_json(&["lower", input.to_str().unwrap()])).unwrap();
    for backend in ["svg", "png"] {
        let profile_json = cli_json(&["capabilities", backend]);
        let mut profile: BackendCapabilities =
            serde_json::from_value(profile_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&profile).unwrap(), profile_json);
        let valid_report = negotiate_scene(&scene, &profile).unwrap();
        valid_report.require_accepted().unwrap();

        profile.accepted_ir = "scene3d".into();
        profile.limits.insert("max-nodes".into(), 0);
        let report = negotiate_scene(&scene, &profile).unwrap();
        assert!(
            !report.is_accepted(),
            "{backend} accepted the wrong IR and zero node budget"
        );
        for feature in ["accepted_ir", "limit.max-nodes"] {
            assert!(
                report
                    .decisions
                    .iter()
                    .any(|d| d.feature == feature && d.status == CapabilityStatus::Error)
            );
        }
        assert!(report.require_accepted().is_err());
    }
}

#[test]
fn svg_render_publishes_the_same_valid_negotiation_report() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let input = root.join("examples/chart/service-health.viz.yaml");
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("chart.svg");
    let manifest = temp.path().join("chart.json");
    let result = Command::new(env!("CARGO_BIN_EXE_vizir"))
        .arg("render")
        .arg(&input)
        .args(["--format", "svg", "--output"])
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
    let scene: Scene2D =
        serde_json::from_value(cli_json(&["lower", input.to_str().unwrap()])).unwrap();
    let profile: BackendCapabilities =
        serde_json::from_value(cli_json(&["capabilities", "svg"])).unwrap();
    let report = negotiate_scene(&scene, &profile).unwrap();
    let published: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert_eq!(
        published["capability_report"],
        serde_json::to_value(&report).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(output).unwrap(),
        vizir_backend_svg::render(&scene).unwrap()
    );
}
