use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn vizir(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn source() -> Value {
    json!({"version":"0.2","id":"theme-roundtrip","width":640,"height":400,
    "datasets":{"data":{"key":"id","rows":[{"id":"b","x":2.0,"y":4.0,"group":"G"},{"id":"a","x":1.0,"y":2.0,"group":"G"}]}},
    "views":[{"kind":"chart.line","id":"chart","title":"Theme round trip","dataset":"data",
        "frame":{"x":0,"y":0,"width":640,"height":400},"x":{"field":"x"},"y":{"field":"y"},"series":{"field":"group"}}]})
}

#[test]
fn cli_themed_normalize_reload_edit_refresh_and_render_are_one_workflow() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("source.json");
    write(&input, &source());
    let saved = t.path().join("saved.json");
    let refreshed = t.path().join("refreshed.json");
    let svg = t.path().join("output.svg");
    let manifest = t.path().join("manifest.json");
    success(vizir(&[
        "normalize",
        path(&input),
        "--theme",
        "sage-dark",
        "-o",
        path(&saved),
    ]));
    let original = json_file(&saved);
    assert_eq!(original["format"], "vizir-themed-mir/1");
    assert_eq!(original["mir"]["version"], "0.2");
    let direct = success(vizir(&["lower", path(&input), "--theme", "sage-dark"]));
    let loaded = success(vizir(&["lower", path(&saved)]));
    assert_eq!(direct, loaded);
    success(vizir(&["validate", path(&saved)]));
    let mut changed = original.clone();
    changed["mir"]["data"]["data/data"]["operator"]["rows"][0]["y"] = json!(3.5);
    write(&saved, &changed);
    let stale = vizir(&["render", path(&saved), "--format", "svg", "-o", path(&svg)]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("VIZ-MATERIALIZE-0002"));
    assert!(!svg.exists());
    success(vizir(&["normalize", path(&saved), "-o", path(&refreshed)]));
    let after = json_file(&refreshed);
    assert_eq!(after["theme"], original["theme"]);
    assert_eq!(
        after["mir"]["views"][0]["scales"],
        original["mir"]["views"][0]["scales"]
    );
    let points = &after["mir"]["views"][0]["mark"]["series"][0]["points"];
    assert_eq!(points[0]["key"], "a");
    assert_eq!(points[1]["key"], "b");
    assert_eq!(points[1]["y"], 3.5);
    let again = success(vizir(&["normalize", path(&refreshed)]));
    assert_eq!(serde_json::from_str::<Value>(&again).unwrap(), after);
    success(vizir(&[
        "render",
        path(&refreshed),
        "--format",
        "svg",
        "-o",
        path(&svg),
        "--manifest",
        path(&manifest),
    ]));
    let report = json_file(&manifest);
    assert_eq!(report["theme"], original["theme"]);
    assert_eq!(report["source_context_format"], "vizir-themed-mir/1");
    let svg = fs::read_to_string(svg).unwrap();
    assert!(svg.contains(original["theme"]["defaults"]["ink"].as_str().unwrap()));
    assert!(!svg.contains("width=\"100%\" height=\"100%\""));
    success(vizir(&[
        "explain",
        path(&refreshed),
        "--node",
        "chart/point/b",
    ]));
    success(vizir(&[
        "validate",
        path(&refreshed),
        "--theme",
        "sage-dark",
    ]));
}

#[test]
fn cli_rejects_ambiguous_malformed_or_rethemed_context_without_output() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("source.json");
    write(&input, &source());
    let original: Value = serde_json::from_str(&success(vizir(&[
        "normalize",
        path(&input),
        "--theme",
        "azure",
    ])))
    .unwrap();
    let bad = t.path().join("bad.json");
    let out = t.path().join("out.svg");
    for mutation in ["format", "theme", "pin", "defaults", "unknown", "mixed"] {
        let mut value = original.clone();
        match mutation {
            "format" => value["format"] = json!("vizir-themed-mir/99"),
            "theme" => value["theme"]["name"] = json!("not-a-theme"),
            "pin" => value["theme"]["registry_revision"] = json!("0000000"),
            "defaults" => value["theme"]["defaults"]["series"][0] = json!("#FFFFFF"),
            "unknown" => value["theme"]["ignored"] = json!(true),
            _ => value["views"] = json!([]),
        }
        write(&bad, &value);
        for command in ["validate", "normalize", "lower"] {
            assert!(
                !vizir(&[command, path(&bad)]).status.success(),
                "{mutation} {command}"
            );
        }
        assert!(
            !vizir(&["render", path(&bad), "--format", "svg", "-o", path(&out)])
                .status
                .success()
        );
        assert!(!out.exists());
    }
    write(&bad, &original);
    let conflict = vizir(&["lower", path(&bad), "--theme", "azure-dark"]);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("VIZ-THEME-0006"));
    assert!(
        !vizir(&["lower", path(&input), "--theme", "default"])
            .status
            .success()
    );
    let duplicate = serde_json::to_string(&original).unwrap().replacen(
        '{',
        "{\"format\":\"vizir-themed-mir/1\",",
        1,
    );
    fs::write(&bad, duplicate).unwrap();
    assert!(!vizir(&["validate", path(&bad)]).status.success());
}

#[test]
fn legacy_cli_stays_unwrapped_and_theme_schema_is_separate() {
    let input = workspace().join("examples/chart/service-health.viz.yaml");
    let mir: Value = serde_json::from_str(&success(vizir(&["normalize", path(&input)]))).unwrap();
    assert!(mir.get("format").is_none());
    assert!(mir.get("theme").is_none());
    assert_eq!(mir["version"], "0.1");
    let schema: Value = serde_json::from_str(&success(vizir(&["schema", "themed-mir"]))).unwrap();
    assert_eq!(
        schema["properties"]["format"]["const"],
        "vizir-themed-mir/1"
    );
    assert_eq!(
        schema,
        json_file(&workspace().join("schemas/themed-mir.schema.json"))
    );
    let names = success(vizir(&["themes"]));
    assert_eq!(names.lines().count(), 14);
    assert!(names.lines().any(|n| n == "olive-paper-dark"));
}

#[test]
fn oversized_json_and_deep_envelopes_are_rejected_before_execution() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("oversized.json");
    let file = fs::File::create(&input).unwrap();
    file.set_len(32 * 1024 * 1024 + 1).unwrap();
    let output = vizir(&["normalize", path(&input)]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("VIZ-THEME-0004"));
    fs::write(
        &input,
        format!(
            "{{\"format\":\"vizir-themed-mir/1\",\"x\":{}0{}}}",
            "[".repeat(130),
            "]".repeat(130)
        ),
    )
    .unwrap();
    assert!(!vizir(&["normalize", path(&input)]).status.success());
}

#[test]
fn unselected_theme_preserves_exact_legacy_mir_scene_and_svg_bytes() {
    let input =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-theme-source.json");
    let t = tempfile::tempdir().unwrap();
    let mir = t.path().join("legacy.mir.json");
    let scene = t.path().join("legacy.scene.json");
    let output = t.path().join("legacy.svg");
    success(vizir(&["normalize", path(&input), "-o", path(&mir)]));
    success(vizir(&["lower", path(&input), "-o", path(&scene)]));
    assert_eq!(
        fs::read_to_string(mir).unwrap(),
        include_str!("fixtures/legacy-theme.mir.json")
    );
    assert_eq!(
        fs::read_to_string(scene).unwrap(),
        include_str!("fixtures/legacy-theme.scene.json")
    );
    success(vizir(&[
        "render",
        path(&input),
        "--format",
        "svg",
        "-o",
        path(&output),
    ]));
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        include_str!("fixtures/legacy-theme.svg")
    );
}

#[cfg(unix)]
#[test]
fn json_fifo_is_rejected_without_waiting_for_a_writer() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let t = tempfile::tempdir().unwrap();
    let fifo = t.path().join("input.json");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(["normalize", path(&fifo)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("JSON FIFO read blocked waiting for a writer");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("VIZ-THEME-0008"));
}
