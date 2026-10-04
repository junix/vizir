use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn vizir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}
#[test]
fn html_is_explicit_and_static_defaults_remain_static() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.html");
    let input = root().join("examples/mixed/reliability-brief.viz.yaml");
    for args in [
        vec!["--format", "html"],
        vec!["--format", "html", "--interaction-profile", "unknown"],
        vec!["--format", "svg", "--interaction-profile", "explorer-v1"],
        vec!["--format", "svg", "--instance-key", "a"],
    ] {
        fs::write(&output, "original").unwrap();
        let result = vizir()
            .arg("render")
            .arg(&input)
            .args(args)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(&output).unwrap(), "original");
    }
    let result = vizir()
        .arg("render")
        .arg(&input)
        .args(["--format", "svg", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let svg = fs::read_to_string(&output).unwrap();
    assert!(svg.starts_with("<svg "));
    assert!(!svg.contains("data-vizir-scene-id"));
    assert!(!svg.contains("script"));
}
#[test]
fn mixed_scenarios_export_structurally_valid_packages_and_manifest() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["reliability-brief", "capacity-planning", "theme-defaults"] {
        let input = root().join(format!("examples/mixed/{name}.viz.yaml"));
        let output = dir.path().join(format!("{name}.html"));
        let manifest = dir.path().join(format!("{name}.json"));
        let result = vizir()
            .arg("render")
            .arg(&input)
            .args([
                "--format",
                "html",
                "--interaction-profile",
                "explorer-v1",
                "--instance-key",
                "example",
                "-o",
            ])
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
        let report: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(report["format"], "html");
        assert_eq!(report["interaction"]["profile"], "explorer-v1");
        assert_eq!(report["interaction"]["instance_key"], "example");
        let checked = Command::new("python3")
            .arg(root().join("tools/check_web_package.py"))
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        assert!(String::from_utf8_lossy(&checked.stdout).contains("not_run"));
        // Execute the actual metadata validator/reducer under Node, never a DOM mock.
        let native = Command::new("node").args(["--input-type=module", "-e", r#"
            import fs from 'node:fs';
            import { pathToFileURL } from 'node:url';
            const runtime = await import(pathToFileURL(process.argv[1]));
            const html = fs.readFileSync(process.argv[2], 'utf8');
            const match = html.match(/<script type="application\/json" data-vizir-metadata="">\n([\s\S]*?)\n<\/script>/);
            if (!match) throw new Error('missing exported metadata');
            const context = runtime.validateContext(JSON.parse(match[1]));
            const state = runtime.createState(context);
            if (state.camera.width !== context.home.width) throw new Error('home mismatch');
        "#]).arg(root().join("crates/vizir-web/runtime/runtime.mjs")).arg(&output).output().unwrap();
        assert!(
            native.status.success(),
            "{}",
            String::from_utf8_lossy(&native.stderr)
        );
    }
}
#[test]
fn generated_interaction_schema_matches_checked_in_contract() {
    let actual = vizir().args(["schema", "interaction"]).output().unwrap();
    assert!(actual.status.success());
    let actual: Value = serde_json::from_slice(&actual.stdout).unwrap();
    let expected: Value =
        serde_json::from_slice(&fs::read(root().join("schemas/interaction.schema.json")).unwrap())
            .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn html_failure_preserves_existing_output_and_manifest_and_cleans_staging() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.viz.yaml");
    let output = dir.path().join("out.html");
    let manifest = dir.path().join("manifest.json");
    fs::copy(
        root().join("examples/mixed/theme-defaults.viz.yaml"),
        &input,
    )
    .unwrap();
    let run = || {
        vizir()
            .arg("render")
            .arg(&input)
            .args([
                "--format",
                "html",
                "--interaction-profile",
                "explorer-v1",
                "-o",
            ])
            .arg(&output)
            .arg("--manifest")
            .arg(&manifest)
            .output()
            .unwrap()
    };
    fs::write(&output, "old output").unwrap();
    fs::create_dir(&manifest).unwrap();
    fs::write(manifest.join("keep"), "old manifest").unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_to_string(&output).unwrap(), "old output");
    assert_eq!(
        fs::read_to_string(manifest.join("keep")).unwrap(),
        "old manifest"
    );
    fs::remove_dir_all(&manifest).unwrap();
    fs::write(&manifest, "old manifest").unwrap();
    let source = format!(
        "version: '0.1'\nid: test\nwidth: 100\nheight: 100\nbackground: transparent\ndatasets: {{}}\nviews:\n  - kind: geometry.scene\n    id: g\n    frame: {{x: 0, y: 0, width: 100, height: 100}}\n    children:\n      - type: text\n        id: text\n        x: 0\n        y: 10\n        text: {}\n",
        "x".repeat(4097)
    );
    fs::write(&input, source).unwrap();
    let rejected = run();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("VIZ-WEB-0002"));
    assert_eq!(fs::read_to_string(&output).unwrap(), "old output");
    assert_eq!(fs::read_to_string(&manifest).unwrap(), "old manifest");
    assert!(fs::read_dir(dir.path()).unwrap().all(|p| {
        !p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vizir-")
    }));
}
