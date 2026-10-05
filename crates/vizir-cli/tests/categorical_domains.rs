use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Vec<u8> {
    let out = run(args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn nodes<'a>(value: &'a Value, found: &mut Vec<&'a Value>) {
    if let Some(array) = value.as_array() {
        for item in array {
            found.push(item);
            nodes(&item["children"], found);
        }
    }
}
#[test]
fn two_panel_composition_json_yaml_and_native_replay_keep_beta_red() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let hir = dir.path().join("source.json");
    let mir = dir.path().join("mir.json");
    let source = root.join("examples/composition/stable-panel-colors.compose.yaml");
    ok(&["compose", path(&source), "--output", path(&hir)]);
    ok(&["validate", path(&hir)]);
    ok(&["normalize", path(&hir), "--output", path(&mir)]);
    let v: Value = serde_json::from_slice(&fs::read(&mir).unwrap()).unwrap();
    assert_eq!(v["version"], "0.6");
    for panel in v["views"].as_array().unwrap() {
        let scale = panel["scales"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["type"] == "ordinal-color")
            .unwrap();
        assert_eq!(scale["domain"], json!(["Alpha", "Beta"]));
        assert_eq!(scale["range"], json!(["#3B6EF5", "#EB5E55"]));
    }
    assert_eq!(v["views"][1]["mark"]["series"].as_array().unwrap().len(), 1);
    let svg = dir.path().join("source.svg");
    let replay = dir.path().join("replay.svg");
    let bundle = dir.path().join("compiled.json");
    fs::write(
        &bundle,
        serde_json::to_vec(&json!({"format":"vizir-compiled-mir/1","context":{},"mir":v})).unwrap(),
    )
    .unwrap();
    ok(&[
        "render",
        path(&hir),
        "--format",
        "svg",
        "--output",
        path(&svg),
    ]);
    ok(&[
        "render",
        path(&bundle),
        "--format",
        "svg",
        "--output",
        path(&replay),
    ]);
    assert_eq!(fs::read(svg).unwrap(), fs::read(replay).unwrap());
    let scene: Value = serde_json::from_slice(&ok(&["lower", path(&hir)])).unwrap();
    let mut all = Vec::new();
    nodes(&scene["nodes"], &mut all);
    for id in ["all-series/series/Beta", "beta-only/series/Beta"] {
        assert_eq!(
            all.iter().find(|n| n["id"] == id).unwrap()["style"]["stroke"],
            "#EB5E55"
        );
    }
    for id in ["all-series/legend/1/swatch", "beta-only/legend/1/swatch"] {
        assert_eq!(
            all.iter().find(|n| n["id"] == id).unwrap()["style"]["fill"],
            "#EB5E55"
        );
    }
    assert!(!all.iter().any(|n| n["id"] == "beta-only/series/Alpha"));
    let yaml: Value = serde_yaml::from_str(&fs::read_to_string(source).unwrap()).unwrap();
    let json = dir.path().join("source.compose.json");
    fs::write(&json, serde_json::to_vec(&yaml).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&hir).unwrap()).unwrap(),
        serde_json::from_slice::<Value>(&ok(&["compose", path(&json)])).unwrap()
    );
}
#[test]
fn invalid_domains_leave_existing_output_untouched() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let output = dir.path().join("output.svg");
    let original: Value = serde_json::from_slice(&ok(&[
        "compose",
        path(&root.join("examples/composition/stable-panel-colors.compose.yaml")),
    ]))
    .unwrap();
    for domain in [
        Value::Null,
        json!([]),
        json!(["Alpha", "Alpha"]),
        json!(["Alpha"]),
    ] {
        let mut invalid = original.clone();
        invalid["views"][0]["series"]["domain"] = domain;
        fs::write(&input, serde_json::to_vec(&invalid).unwrap()).unwrap();
        fs::write(&output, b"existing artifact").unwrap();
        let result = run(&[
            "render",
            path(&input),
            "--format",
            "svg",
            "--output",
            path(&output),
        ]);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
    }
}
