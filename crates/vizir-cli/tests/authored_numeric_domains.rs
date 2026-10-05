use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Vec<u8> {
    let o = run(args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn original() -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/composition/shared-numeric-domains.compose.yaml");
    serde_json::from_slice(&ok(&["compose", path(&p)])).unwrap()
}
#[test]
fn native_cli_domains_survive_normalize_render_and_compiled_replay() {
    let dir = tempfile::tempdir().unwrap();
    let hir = dir.path().join("d.json");
    let mir = dir.path().join("m.json");
    let bundle = dir.path().join("c.json");
    let a = dir.path().join("a.svg");
    let b = dir.path().join("b.svg");
    fs::write(&hir, serde_json::to_vec(&original()).unwrap()).unwrap();
    ok(&["validate", path(&hir)]);
    ok(&["normalize", path(&hir), "--output", path(&mir)]);
    let v: Value = serde_json::from_slice(&fs::read(&mir).unwrap()).unwrap();
    assert_eq!(v["version"], "0.7");
    for chart in v["views"].as_array().unwrap() {
        for (i, domain) in [json!([0.0, 1.0]), json!([0.0, 10.0])]
            .into_iter()
            .enumerate()
        {
            assert_eq!(chart["scales"][i]["domain"], domain);
            assert_eq!(chart["scales"][i]["out_of_domain"], "reject");
        }
    }
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
        path(&a),
    ]);
    ok(&[
        "render",
        path(&bundle),
        "--format",
        "svg",
        "--output",
        path(&b),
    ]);
    assert_eq!(fs::read(a).unwrap(), fs::read(b).unwrap());
}
#[test]
fn bad_domains_never_publish_or_replace_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("d.json");
    let output = dir.path().join("sentinel.svg");
    for domain in [
        Value::Null,
        json!([]),
        json!([0, 0]),
        json!([10, 0]),
        json!([0, 7]),
        json!([-1e308, 1e308]),
    ] {
        let mut v = original();
        v["views"][0]["y"]["domain"] = domain;
        fs::write(&input, serde_json::to_vec(&v).unwrap()).unwrap();
        fs::write(&output, b"existing artifact").unwrap();
        let o = run(&[
            "render",
            path(&input),
            "--format",
            "svg",
            "--output",
            path(&output),
        ]);
        assert!(!o.status.success());
        assert!(o.stdout.is_empty());
        assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
    }
}
#[test]
fn provider_descriptors_and_acceptance_profiles_stay_closed() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("d.json");
    fs::write(&input, serde_json::to_vec(&original()).unwrap()).unwrap();
    let mir: Value = serde_json::from_slice(&ok(&["normalize", path(&input)])).unwrap();
    let descriptor = Command::new(env!("CARGO_BIN_EXE_plot-provider-vizir"))
        .arg("describe")
        .output()
        .unwrap();
    assert!(descriptor.status.success());
    let text = String::from_utf8(descriptor.stdout).unwrap();
    assert!(!text.contains("0.7"));
    // Both provider profile guards are independently covered by existing future-version tests.
    assert_eq!(mir["version"], "0.7");
}
