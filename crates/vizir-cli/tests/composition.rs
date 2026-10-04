use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use vizir_core::{CompositionV1, Document, compose, composition_schema};

const SENTINEL: &[u8] = b"existing output must survive a rejected composition\n";

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vizir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}

fn accepted(result: &Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn rejected(result: &Output) -> String {
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    assert_eq!(result.status.code(), Some(1), "{stderr}");
    assert!(result.stdout.is_empty(), "{:?}", result.stdout);
    assert!(!stderr.is_empty(), "failure needs a diagnostic");
    stderr
}

fn run_compose(input: &Path, output: Option<&Path>) -> Output {
    let mut command = vizir();
    command.arg("compose").arg(input);
    if let Some(output) = output {
        command.arg("--output").arg(output);
    }
    command.output().unwrap()
}

fn minimal_source() -> Value {
    json!({
        "schema": "vizir-composition/0.1",
        "id": "cli-composition",
        "width": 400,
        "height": 300,
        "layout": {"kind": "grid", "columns": 1},
        "panels": [{"kind": "geometry.scene", "id": "geometry", "children": [
            {"type": "rect", "id": "box", "x": 10, "y": 20, "width": 30, "height": 40}
        ]}]
    })
}

fn write_source(directory: &Path, source: &Value) -> PathBuf {
    let input = directory.join("source.compose.json");
    fs::write(&input, serde_json::to_vec_pretty(source).unwrap()).unwrap();
    input
}

fn assert_clean(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vizir-"),
            "publication left staging debris"
        );
    }
}

#[test]
fn compose_stdout_expands_defaults_into_plain_hir_02() {
    let temporary = tempfile::tempdir().unwrap();
    let source = minimal_source();
    let input = write_source(temporary.path(), &source);
    let result = run_compose(&input, None);
    accepted(&result);
    let document: Document = serde_json::from_slice(&result.stdout).unwrap();
    let typed: CompositionV1 = serde_json::from_value(source).unwrap();
    assert_eq!(document, compose(&typed).unwrap());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["version"], "0.2");
    assert_eq!(value["background"], "transparent");
    assert_eq!(value["datasets"], json!({}));
    assert_eq!(value["title"], Value::Null);
    assert_eq!(
        value["views"][0]["frame"],
        json!({"x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0})
    );
    for wrapper_field in ["schema", "layout", "panels"] {
        assert!(value.get(wrapper_field).is_none());
    }
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
}

#[test]
fn yaml_demo_round_trips_through_existing_commands() {
    let input = workspace().join("examples/composition/service-grid.compose.yaml");
    let original = fs::read(&input).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("nested/service-grid.viz.json");
    let result = run_compose(&input, Some(&output));
    accepted(&result);
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!("emitted: {}\n", output.display())
    );
    let bytes = fs::read(&output).unwrap();
    let hir: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hir["version"], "0.2");
    assert_eq!(hir["width"], 1440.0);
    assert_eq!(hir["height"], 600.0);
    assert_eq!(
        hir["views"][0]["frame"],
        json!({"x": 24.0, "y": 24.0, "width": 684.0, "height": 552.0})
    );
    assert_eq!(
        hir["views"][1]["frame"],
        json!({"x": 732.0, "y": 24.0, "width": 684.0, "height": 552.0})
    );
    for _ in 0..2 {
        let repeated = run_compose(&input, None);
        accepted(&repeated);
        assert_eq!(
            repeated.stdout,
            [bytes.as_slice(), b"\n"].concat(),
            "stdout adds only its customary trailing newline to canonical JSON"
        );
    }

    let valid = vizir().arg("validate").arg(&output).output().unwrap();
    accepted(&valid);
    assert_eq!(
        String::from_utf8(valid.stdout).unwrap(),
        "valid: service-grid (VizHIR 0.2, 2 views)\n"
    );
    let normalized = vizir().arg("normalize").arg(&output).output().unwrap();
    accepted(&normalized);
    let mir: Value = serde_json::from_slice(&normalized.stdout).unwrap();
    assert_eq!(mir["source_hir_version"], "0.2");
    assert_eq!(mir["version"], "0.2");
    assert_eq!(mir["document_id"], "service-grid");
    let lowered = vizir().arg("lower").arg(&output).output().unwrap();
    accepted(&lowered);
    let scene: Value = serde_json::from_slice(&lowered.stdout).unwrap();
    assert_eq!(scene["document_id"], "service-grid");
    assert_eq!(scene["nodes"].as_array().unwrap().len(), 2);
    let svg = temporary.path().join("service-grid.svg");
    let rendered = vizir()
        .arg("render")
        .arg(&output)
        .args(["--format", "svg", "--output"])
        .arg(&svg)
        .output()
        .unwrap();
    accepted(&rendered);
    assert!(fs::read_to_string(svg).unwrap().starts_with("<svg"));
    assert_eq!(fs::read(input).unwrap(), original);
    assert_clean(temporary.path());
    assert_clean(output.parent().unwrap());
}

#[test]
fn composition_schema_is_canonical_on_stdout_and_file() {
    let result = vizir().args(["schema", "composition"]).output().unwrap();
    accepted(&result);
    let schema: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(schema, composition_schema());
    let checked_in: Value = serde_json::from_slice(
        &fs::read(workspace().join("schemas/composition.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(schema, checked_in, "checked-in composition schema drifted");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("nested/composition.schema.json");
    let written = vizir()
        .args(["schema", "composition", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    accepted(&written);
    assert_eq!(
        [fs::read(&output).unwrap().as_slice(), b"\n"].concat(),
        result.stdout
    );
    assert_eq!(
        String::from_utf8(written.stdout).unwrap(),
        format!("emitted: {}\n", output.display())
    );
    assert_clean(output.parent().unwrap());
}

#[test]
fn successful_compose_replaces_existing_output_and_cleans_staging() {
    let temporary = tempfile::tempdir().unwrap();
    let input = write_source(temporary.path(), &minimal_source());
    let output = temporary.path().join("composed.viz.json");
    fs::write(&output, SENTINEL).unwrap();
    accepted(&run_compose(&input, Some(&output)));
    let document: Document = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(document.id, "cli-composition");
    assert_clean(temporary.path());
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 2);
}

#[test]
fn malformed_and_semantically_invalid_sources_never_publish_output() {
    let base = minimal_source();
    let mutations = [
        ("/schema", json!("vizir-composition/0.2")),
        ("/width", json!("wide")),
        ("/height", Value::Null),
        ("/layout", Value::Null),
        ("/layout/columns", json!(0)),
        ("/layout/columns", json!(2)),
        ("/layout/columns", json!(1.5)),
        ("/panels", Value::Null),
        ("/panels", json!([])),
        ("/panels/0/kind", json!("chart.unknown")),
        ("/panels/0/children", Value::Null),
        ("/panels/0/children/0/width", json!(-1)),
    ];
    let mut cases = Vec::new();
    for (pointer, value) in mutations {
        let mut changed = base.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        cases.push(changed);
    }
    for (field, value) in [
        ("padding", Value::Null),
        ("gap", Value::Null),
        ("padding", json!(-1)),
        ("gap", json!(-1)),
        ("padding", json!(200)),
        ("padding", json!(1e308)),
        ("weights", json!([1])),
    ] {
        let mut changed = base.clone();
        changed["layout"][field] = value;
        cases.push(changed);
    }
    for (field, value) in [
        (
            "frame",
            json!({"x": 0, "y": 0, "width": 400, "height": 300}),
        ),
        ("point_size", json!(7)),
        ("layout", json!({"kind": "grid", "columns": 1})),
    ] {
        let mut changed = base.clone();
        changed["panels"][0][field] = value;
        cases.push(changed);
    }
    for field in ["background", "datasets"] {
        let mut changed = base.clone();
        changed[field] = Value::Null;
        cases.push(changed);
    }
    let mut missing_schema = base.clone();
    missing_schema.as_object_mut().unwrap().remove("schema");
    cases.push(missing_schema);
    let mut duplicate = base.clone();
    duplicate["panels"]
        .as_array_mut()
        .unwrap()
        .push(base["panels"][0].clone());
    cases.push(duplicate);
    let mut impossible_gap = base.clone();
    let mut other = base["panels"][0].clone();
    other["id"] = json!("other");
    impossible_gap["panels"].as_array_mut().unwrap().push(other);
    impossible_gap["layout"]["columns"] = json!(2);
    impossible_gap["layout"]["gap"] = json!(1e308);
    cases.push(impossible_gap);
    let mut bad_dataset = base.clone();
    bad_dataset["panels"] = json!([
        {"kind": "chart.scatter", "id": "points", "dataset": "missing",
         "x": {"field": "x"}, "y": {"field": "y"}}
    ]);
    cases.push(bad_dataset);
    let mut bad_edge = base;
    bad_edge["panels"] = json!([
        {"kind": "diagram.graph", "id": "graph", "nodes": [{"id": "n", "label": "Node"}],
         "edges": [{"from": "n", "to": "missing"}]}
    ]);
    cases.push(bad_edge);

    for (index, source) in cases.into_iter().enumerate() {
        let temporary = tempfile::tempdir().unwrap();
        let input = write_source(temporary.path(), &source);
        let original = fs::read(&input).unwrap();
        rejected(&run_compose(&input, None));
        let output = temporary.path().join("composed.viz.json");
        rejected(&run_compose(&input, Some(&output)));
        assert!(!output.exists(), "invalid case {index} created an output");
        fs::write(&output, SENTINEL).unwrap();
        rejected(&run_compose(&input, Some(&output)));
        assert_eq!(fs::read(&output).unwrap(), SENTINEL, "invalid case {index}");
        assert_eq!(fs::read(&input).unwrap(), original, "invalid case {index}");
        assert_clean(temporary.path());
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 2);
    }
}

#[test]
fn nonfinite_yaml_allocation_fails_before_publication() {
    for numeric in [".nan", ".inf", "-.inf"] {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("nonfinite.compose.yaml");
        fs::write(
            &input,
            format!(
                "schema: vizir-composition/0.1\nid: invalid\nwidth: {numeric}\nheight: 300\n\
                 layout: {{kind: grid, columns: 1}}\n\
                 panels: [{{kind: geometry.scene, id: local, children: []}}]\n"
            ),
        )
        .unwrap();
        let output = temporary.path().join("composed.viz.json");
        fs::write(&output, SENTINEL).unwrap();
        rejected(&run_compose(&input, Some(&output)));
        assert_eq!(fs::read(&output).unwrap(), SENTINEL);
        assert_clean(temporary.path());
    }
}

#[test]
fn compose_cannot_overwrite_its_source_through_path_aliases() {
    for spelling in [
        "source.compose.json",
        "./source.compose.json",
        "sub/../source.compose.json",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let input = write_source(temporary.path(), &minimal_source());
        let original = fs::read(&input).unwrap();
        fs::create_dir(temporary.path().join("sub")).unwrap();
        let error = rejected(&run_compose(&input, Some(&temporary.path().join(spelling))));
        assert!(error.contains("VIZ-PATH-0001"), "{error}");
        assert!(error.contains("input path"), "{error}");
        assert!(error.contains("output path"), "{error}");
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_clean(temporary.path());
    }
}

#[test]
fn compose_cannot_overwrite_a_hardlinked_source() {
    let temporary = tempfile::tempdir().unwrap();
    let input = write_source(temporary.path(), &minimal_source());
    let original = fs::read(&input).unwrap();
    let alias = temporary.path().join("alias.json");
    fs::hard_link(&input, &alias).unwrap();
    let error = rejected(&run_compose(&input, Some(&alias)));
    assert!(error.contains("VIZ-PATH-0001"), "{error}");
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read(alias).unwrap(), original);
    assert_clean(temporary.path());
}

#[cfg(unix)]
#[test]
fn compose_cannot_overwrite_a_symlinked_source() {
    let temporary = tempfile::tempdir().unwrap();
    let input = write_source(temporary.path(), &minimal_source());
    let original = fs::read(&input).unwrap();
    let alias = temporary.path().join("alias.json");
    std::os::unix::fs::symlink(&input, &alias).unwrap();
    let error = rejected(&run_compose(&input, Some(&alias)));
    assert!(error.contains("VIZ-PATH-0001"), "{error}");
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read_link(alias).unwrap(), input);
    assert_clean(temporary.path());
}

#[test]
fn directory_destinations_are_not_replaced_or_partially_written() {
    let temporary = tempfile::tempdir().unwrap();
    let input = write_source(temporary.path(), &minimal_source());
    let output = temporary.path().join("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep"), SENTINEL).unwrap();
    rejected(&run_compose(&input, Some(&output)));
    assert_eq!(fs::read(output.join("keep")).unwrap(), SENTINEL);
    assert_clean(temporary.path());
    assert_clean(&output);
}

#[test]
fn legacy_commands_keep_their_hir_input_contract() {
    let source = workspace().join("examples/chart/service-health.viz.yaml");
    let result = vizir().arg("normalize").arg(&source).output().unwrap();
    accepted(&result);
    let mir: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(mir["version"], "0.1");
    assert_eq!(mir["source_hir_version"], "0.1");
    assert_eq!(mir["document_id"], "service-health-dashboard");
    rejected(&run_compose(&source, None));
    let wrapper = workspace().join("examples/composition/service-grid.compose.yaml");
    for command in ["validate", "normalize", "lower"] {
        rejected(&vizir().arg(command).arg(&wrapper).output().unwrap());
    }
}
