use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn run(args: &[&str], fonts: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    command.args(args);
    for font in fonts {
        command.args(["--font", font]);
    }
    command.output().unwrap()
}
fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn rejected(output: Output) {
    assert!(!output.status.success(), "invalid heatmap was accepted");
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty(), "failure needs a diagnostic");
}
fn value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}
fn write(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn source() -> Value {
    json!({
        "version":"0.4","id":"heatmap-cli","width":800,"height":520,
        "background":"transparent",
        "datasets":{"samples":{"key":"id","rows":[
            {"id":"api-tue","day":"Tue","service":"API","value":10},
            {"id":"jobs-mon","day":"Mon","service":"Jobs","value":0},
            {"id":"api-mon","day":"Mon","service":"API","value":20}
        ]}},
        "views":[{"kind":"chart.heatmap","id":"chart","title":"Observed service counts",
            "frame":{"x":0,"y":0,"width":800,"height":520},"dataset":"samples",
            "x":{"field":"day","label":"day"},
            "y":{"field":"service","label":"service"},
            "color":{"field":"value","label":"count","domain":[0,20],
                "palette":["#11223380","#AABBCC"]}}]
    })
}
fn flatten<'a>(nodes: &'a [Value], output: &mut Vec<&'a Value>) {
    for node in nodes {
        output.push(node);
        if let Some(children) = node["children"].as_array() {
            flatten(children, output);
        }
    }
}
fn cells(scene: &Value) -> Vec<&Value> {
    let mut all = Vec::new();
    flatten(scene["nodes"].as_array().unwrap(), &mut all);
    all.into_iter()
        .filter(|n| n["id"].as_str().is_some_and(|id| id.contains("/cell/")))
        .collect()
}
fn scale<'a>(mir: &'a Value, binding: &str) -> &'a Value {
    let chart = &mir["views"][0];
    let id = &chart["mark"][binding]["scale"];
    chart["scales"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| &s["id"] == id)
        .unwrap()
}
fn assert_clean(dir: &Path) {
    assert!(fs::read_dir(dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vizir-")
    }));
}
fn measured(dir: &Path) -> (PathBuf, Vec<String>) {
    let profile = dir.join("text-profile.json");
    fs::copy(
        root().join("examples/text/wrapping-font-profile.json"),
        &profile,
    )
    .unwrap();
    let fixtures = root().join("crates/vizir-compiler/tests/fixtures/wrapping-fonts");
    let manifest = value(&fs::read(fixtures.join("manifest.json")).unwrap());
    let fonts = manifest["fonts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|font| {
            format!(
                "{}={}",
                font["sha256"].as_str().unwrap(),
                fixtures.join(font["file"].as_str().unwrap()).display()
            )
        })
        .collect();
    (profile, fonts)
}

#[test]
fn sparse_heatmap_runs_normal_commands_and_preserves_rows_keys_and_zero() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let authored = source();
    write(&input, &authored);
    success(run(&["validate", path(&input)], &[]));
    let normalized = success(run(&["normalize", path(&input)], &[]));
    let mir = value(&normalized);
    assert_eq!(mir["version"], "0.4");
    assert_eq!(mir["source_hir_version"], "0.4");
    assert_eq!(
        mir["data"]["data/samples"]["operator"]["rows"],
        authored["datasets"]["samples"]["rows"]
    );
    let mark = &mir["views"][0]["mark"];
    assert_eq!(mark["type"], "heatmap");
    assert_eq!(
        mark["instances"],
        json!([
            {"key":"api-tue","x":"Tue","y":"API","value":10.0},
            {"key":"jobs-mon","x":"Mon","y":"Jobs","value":0.0},
            {"key":"api-mon","x":"Mon","y":"API","value":20.0}
        ])
    );
    assert_eq!(scale(&mir, "x")["domain"], json!(["Tue", "Mon"]));
    assert_eq!(scale(&mir, "y")["domain"], json!(["API", "Jobs"]));
    assert_eq!(scale(&mir, "x")["padding"].as_f64(), Some(0.));
    assert_eq!(scale(&mir, "y")["padding"].as_f64(), Some(0.));
    assert_eq!(scale(&mir, "color")["type"], "quantize-color");
    assert_eq!(scale(&mir, "color")["thresholds"], json!([10.0]));
    assert_eq!(
        scale(&mir, "color")["range"],
        authored["views"][0]["color"]["palette"]
    );
    let persisted = dir.path().join("mir.json");
    fs::write(
        &persisted,
        success(run(&["normalize", path(&input), "--theme", "azure"], &[])),
    )
    .unwrap();
    let direct = success(run(&["lower", path(&input), "--theme", "azure"], &[]));
    assert_eq!(direct, success(run(&["lower", path(&persisted)], &[])));
    let scene = value(&direct);
    let rects = cells(&scene);
    assert_eq!(rects.len(), 3, "missing Tue/Jobs must not create a cell");
    let expected = [
        ("chart/cell/api-tue", "#AABBCC"),
        ("chart/cell/jobs-mon", "#11223380"),
        ("chart/cell/api-mon", "#AABBCC"),
    ];
    for (id, fill) in expected {
        let rect = rects.iter().find(|r| r["id"] == id).unwrap();
        assert_eq!(rect["type"], "rect");
        assert_eq!(rect["style"]["fill"], fill);
        assert!(rect["bounds"]["width"].as_f64().unwrap() > 0.);
        assert!(rect["bounds"]["height"].as_f64().unwrap() > 0.);
    }
    let svg = dir.path().join("heatmap.svg");
    success(run(
        &[
            "render",
            path(&persisted),
            "--format",
            "svg",
            "-o",
            path(&svg),
        ],
        &[],
    ));
    let xml = fs::read_to_string(svg).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.has_tag_name("rect")
                && n.attribute("id").is_some_and(|id| id.contains("/cell/")))
            .count(),
        3
    );
    let explanation = String::from_utf8(success(run(
        &["explain", path(&persisted), "--node", "chart/cell/jobs-mon"],
        &[],
    )))
    .unwrap();
    assert!(explanation.contains("chart/cell/jobs-mon"));
    assert_eq!(value(&fs::read(&input).unwrap()), authored);
    assert_clean(dir.path());
}

#[test]
fn heatmap_requires_exactly_three_scales_and_matching_explicit_guides() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    write(&input, &source());
    let envelope = value(&success(run(
        &["normalize", path(&input), "--theme", "azure"],
        &[],
    )));
    let mir = &envelope["mir"];
    assert_eq!(mir["views"][0]["scales"].as_array().unwrap().len(), 3);
    let guides = mir["views"][0]["guides"].as_array().unwrap();
    assert_eq!(guides.len(), 3);
    for (binding, kind, orient) in [
        ("x", "axis", "bottom"),
        ("y", "axis", "left"),
        ("color", "legend", "right"),
    ] {
        let guide = guides.iter().find(|g| g["orient"] == orient).unwrap();
        assert_eq!(guide["kind"], kind);
        assert_eq!(guide["scale"], mir["views"][0]["mark"][binding]["scale"]);
    }
    for index in 0..guides.len() {
        let mut bad = envelope.clone();
        bad["mir"]["views"][0]["guides"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        write(&input, &bad);
        rejected(run(&["lower", path(&input)], &[]));
    }
    let mut bad = envelope.clone();
    bad["mir"]["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .push(guides[0].clone());
    write(&input, &bad);
    rejected(run(&["lower", path(&input)], &[]));
    let mut bad = envelope.clone();
    let y_scale = bad["mir"]["views"][0]["mark"]["y"]["scale"].clone();
    let bottom = bad["mir"]["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["orient"] == "bottom")
        .unwrap();
    bottom["scale"] = y_scale;
    write(&input, &bad);
    rejected(run(&["lower", path(&input)], &[]));
    let mut bad = envelope.clone();
    bad["mir"]["views"][0]["scales"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type":"quantize-color","id":"unused-scale","domain":[0.0,20.0],
            "thresholds":[10.0],"range":["#000000","#FFFFFF"]
        }));
    write(&input, &bad);
    rejected(run(&["lower", path(&input)], &[]));
}

#[test]
fn explicit_domains_reserve_empty_bands_and_reordering_preserves_cell_identity() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    authored["views"][0]["x"]["domain"] = json!(["Mon", "Tue", "Wed"]);
    authored["views"][0]["y"]["domain"] = json!(["API", "Jobs", "Cache"]);
    write(&input, &authored);
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    assert_eq!(scale(&mir, "x")["domain"], json!(["Mon", "Tue", "Wed"]));
    assert_eq!(scale(&mir, "y")["domain"], json!(["API", "Jobs", "Cache"]));
    let before = value(&success(run(&["lower", path(&input)], &[])));
    let first_cells = cells(&before);
    let a = first_cells
        .iter()
        .find(|r| r["id"] == "chart/cell/api-mon")
        .unwrap();
    let b = first_cells
        .iter()
        .find(|r| r["id"] == "chart/cell/api-tue")
        .unwrap();
    assert!(
        (a["bounds"]["x"].as_f64().unwrap() + a["bounds"]["width"].as_f64().unwrap()
            - b["bounds"]["x"].as_f64().unwrap())
        .abs()
            < 0.000_11
    );
    let jobs = first_cells
        .iter()
        .find(|r| r["id"] == "chart/cell/jobs-mon")
        .unwrap();
    assert!(a["bounds"]["y"].as_f64().unwrap() < jobs["bounds"]["y"].as_f64().unwrap());
    authored["views"][0]["x"]["domain"] = json!(["Wed", "Tue", "Mon"]);
    authored["views"][0]["y"]["domain"] = json!(["Cache", "Jobs", "API"]);
    write(&input, &authored);
    let after = value(&success(run(&["lower", path(&input)], &[])));
    let second_cells = cells(&after);
    let ids = |rects: &[&Value]| -> BTreeSet<String> {
        rects
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&first_cells), ids(&second_cells));
    assert_eq!(second_cells.len(), 3);
    let moved = second_cells
        .iter()
        .find(|r| r["id"] == "chart/cell/api-mon")
        .unwrap();
    assert_ne!(a["bounds"]["x"], moved["bounds"]["x"]);
    assert_ne!(a["bounds"]["y"], moved["bounds"]["y"]);
    let reordered = value(&success(run(&["normalize", path(&input)], &[])));
    assert_eq!(
        reordered["views"][0]["mark"]["instances"],
        mir["views"][0]["mark"]["instances"]
    );
}

#[test]
fn constant_domain_retains_full_palette_and_uses_lower_middle_color() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    for row in authored["datasets"]["samples"]["rows"]
        .as_array_mut()
        .unwrap()
    {
        row["value"] = json!(7);
    }
    authored["views"][0]["color"]
        .as_object_mut()
        .unwrap()
        .remove("domain");
    authored["views"][0]["color"]["palette"] = json!(["#010203", "#112233", "#445566", "#AABBCC"]);
    write(&input, &authored);
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    assert_eq!(scale(&mir, "color")["domain"], json!([7.0, 7.0]));
    assert_eq!(scale(&mir, "color")["thresholds"], json!([]));
    assert_eq!(scale(&mir, "color")["range"].as_array().unwrap().len(), 4);
    let scene = value(&success(run(&["lower", path(&input)], &[])));
    assert!(
        cells(&scene)
            .iter()
            .all(|r| r["style"]["fill"] == "#112233")
    );
    let mut all = Vec::new();
    flatten(scene["nodes"].as_array().unwrap(), &mut all);
    assert!(
        all.iter()
            .any(|n| n["text"].as_str().is_some_and(|s| s == "= 7"))
    );
    authored["views"][0]["color"]["domain"] = json!([7.0, 7.0]);
    authored["datasets"]["samples"]["rows"][0]["value"] = json!(7.1);
    write(&input, &authored);
    rejected(run(&["lower", path(&input)], &[]));
}

#[test]
fn inferred_domain_does_not_include_zero_and_unthemed_palette_matches_azure() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    authored["views"][0]["color"]
        .as_object_mut()
        .unwrap()
        .remove("domain");
    authored["views"][0]["color"]
        .as_object_mut()
        .unwrap()
        .remove("palette");
    for (row, number) in authored["datasets"]["samples"]["rows"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip([11, 12, 13])
    {
        row["value"] = json!(number);
    }
    write(&input, &authored);
    let plain = value(&success(run(&["normalize", path(&input)], &[])));
    let themed = value(&success(run(
        &["normalize", path(&input), "--theme", "azure"],
        &[],
    )));
    assert!(plain.get("format").is_none());
    assert_eq!(scale(&plain, "color")["domain"], json!([11.0, 13.0]));
    assert_eq!(scale(&plain, "color")["range"].as_array().unwrap().len(), 5);
    assert_eq!(
        scale(&plain, "color")["range"],
        scale(&themed["mir"], "color")["range"]
    );
    assert_eq!(themed["format"], "vizir-themed-mir/1");
    assert_eq!(themed["mir"]["version"], "0.4");
    assert_eq!(plain["background"], "transparent");
}

#[test]
fn heatmap_is_rejected_by_old_hir_mir_and_composition_versions() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let base = source();
    write(&input, &base);
    let envelope = value(&success(run(
        &["normalize", path(&input), "--theme", "azure"],
        &[],
    )));
    for version in ["0.1", "0.2", "0.3"] {
        let mut old = base.clone();
        old["version"] = version.into();
        write(&input, &old);
        rejected(run(&["lower", path(&input)], &[]));
        let mut old = envelope.clone();
        old["mir"]["version"] = version.into();
        old["mir"]["source_hir_version"] = version.into();
        write(&input, &old);
        rejected(run(&["lower", path(&input)], &[]));
        let mut mismatch = envelope.clone();
        mismatch["mir"]["source_hir_version"] = version.into();
        write(&input, &mismatch);
        rejected(run(&["lower", path(&input)], &[]));
    }
    let mut panel = base["views"][0].clone();
    panel.as_object_mut().unwrap().remove("frame");
    for version in ["vizir-composition/0.1", "vizir-composition/0.2"] {
        let composition = json!({"schema":version,"id":"old-heatmap","width":800,"height":520,
            "layout":{"kind":"grid","columns":1},"datasets":base["datasets"],"panels":[panel]});
        write(&input, &composition);
        rejected(run(&["compose", path(&input)], &[]));
    }
}

#[test]
fn null_new_options_and_unsupported_heatmap_fields_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    for (encoding, field) in [
        ("x", "label"),
        ("x", "domain"),
        ("y", "label"),
        ("y", "domain"),
        ("color", "label"),
        ("color", "domain"),
        ("color", "palette"),
        ("color", "number_format"),
    ] {
        let mut bad = source();
        bad["views"][0][encoding][field] = Value::Null;
        write(&input, &bad);
        rejected(run(&["lower", path(&input)], &[]));
    }
    for (field, option) in [
        ("aggregate", json!("sum")),
        ("show_labels", json!(true)),
        ("interpolation", json!("linear")),
        ("padding", json!(0.1)),
    ] {
        let mut bad = source();
        bad["views"][0][field] = option;
        write(&input, &bad);
        rejected(run(&["lower", path(&input)], &[]));
    }
    for (encoding, field, option) in [
        ("x", "axis", json!({})),
        (
            "y",
            "number_format",
            json!({"notation":"fixed","precision":0}),
        ),
        ("color", "axis", json!({})),
        ("color", "palette", json!(["#000000"])),
        ("color", "palette", json!(vec!["#000000"; 10])),
        (
            "color",
            "number_format",
            json!({"notation":"fixed","precision":13}),
        ),
        (
            "color",
            "number_format",
            json!({"notation":"automatic","precision":2}),
        ),
    ] {
        let mut bad = source();
        bad["views"][0][encoding][field] = option;
        write(&input, &bad);
        rejected(run(&["lower", path(&input)], &[]));
    }
}

#[test]
fn invalid_data_and_domains_preserve_paired_outputs_and_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let svg = dir.path().join("heatmap.svg");
    let manifest = dir.path().join("heatmap.manifest.json");
    write(&input, &source());
    let args = [
        "render",
        path(&input),
        "--format",
        "svg",
        "-o",
        path(&svg),
        "--manifest",
        path(&manifest),
    ];
    success(run(&args, &[]));
    let prior_svg = fs::read(&svg).unwrap();
    let prior_manifest = fs::read(&manifest).unwrap();
    let mut invalid = Vec::new();
    for rows in [
        json!([]),
        json!([{"id":"a","day":"Mon","service":"API","value":1},{"id":"b","day":"Mon","service":"API","value":2}]),
        json!([{"id":"a","day":"Mon","service":"API","value":1},{"id":"b","day":"Mon","service":"API","value":1}]),
        json!([{"id":"a","day":null,"service":"API","value":1}]),
        json!([{"id":"a","day":"Mon","service":5,"value":1}]),
        json!([{"id":"a","day":"Mon","service":"API","value":null}]),
        json!([{"id":"a","day":"Mon","service":"API","value":"1"}]),
        json!([{"id":"a","day":"Mon","service":"API","value":true}]),
        json!([{"id":"a","day":"Mon","service":"API"}]),
        json!([{"id":"a","day":"Mon","service":"API","value":-1}]),
        json!([{"id":"a","day":"Mon","service":"API","value":21}]),
        json!([{"id":"a","day":"Mon","service":"API","value":1},{"id":"a","day":"Tue","service":"API","value":2}]),
    ] {
        let mut bad = source();
        bad["datasets"]["samples"]["rows"] = rows;
        invalid.push(bad);
    }
    for category in [
        "",
        "a\nb",
        "a\tb",
        "a\u{7f}b",
        "a\u{85}b",
        "a\u{2028}b",
        "a\u{2029}b",
    ] {
        let mut bad = source();
        bad["datasets"]["samples"]["rows"][0]["day"] = category.into();
        invalid.push(bad);
        let mut bad = source();
        bad["views"][0]["x"]["domain"] = json!(["Mon", "Tue", category]);
        invalid.push(bad);
    }
    for domain in [
        json!([]),
        json!(["Mon"]),
        json!(["Mon", "Tue", "Mon"]),
        json!([1, 2]),
    ] {
        let mut bad = source();
        bad["views"][0]["x"]["domain"] = domain;
        invalid.push(bad);
    }
    for domain in [
        json!([20, 0]),
        json!([0]),
        json!([0, 20, 30]),
        json!([-1e308, 1e308]),
    ] {
        let mut bad = source();
        bad["views"][0]["color"]["domain"] = domain;
        invalid.push(bad);
    }
    let mut crowded = source();
    crowded["views"][0]["frame"]["width"] = json!(90);
    invalid.push(crowded);
    let mut too_many = source();
    too_many["views"][0]["x"]["domain"] =
        json!((0..257).map(|i| format!("column-{i}")).collect::<Vec<_>>());
    invalid.push(too_many);
    let mut too_long = source();
    too_long["datasets"]["samples"]["rows"][0]["day"] = "x".repeat(16 * 1024 + 1).into();
    invalid.push(too_long);
    for bad in invalid {
        write(&input, &bad);
        let original = fs::read(&input).unwrap();
        rejected(run(&args, &[]));
        assert_eq!(fs::read(&svg).unwrap(), prior_svg);
        assert_eq!(fs::read(&manifest).unwrap(), prior_manifest);
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_clean(dir.path());
    }
}

#[test]
fn category_spaces_and_case_are_literal_not_normalized() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    authored["datasets"]["samples"]["rows"] = json!([
        {"id":"a","day":" Mon","service":"API","value":1},
        {"id":"b","day":"Mon","service":"API","value":2},
        {"id":"c","day":"mon","service":"API","value":3},
        {"id":"d","day":" ","service":"API","value":4}
    ]);
    write(&input, &authored);
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    assert_eq!(
        scale(&mir, "x")["domain"],
        json!([" Mon", "Mon", "mon", " "])
    );
    assert_eq!(
        cells(&value(&success(run(&["lower", path(&input)], &[])))).len(),
        4
    );
}

#[test]
fn stale_mir_replay_fails_and_refresh_preserves_resolved_plan() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    write(&input, &source());
    let original = value(&success(run(
        &["normalize", path(&input), "--theme", "azure"],
        &[],
    )));
    let persisted = dir.path().join("edited.mir.json");
    let mut edited = original.clone();
    edited["mir"]["data"]["data/samples"]["operator"]["rows"][0]["value"] = json!(15);
    write(&persisted, &edited);
    rejected(run(&["lower", path(&persisted)], &[]));
    let svg = dir.path().join("stale.svg");
    let manifest = dir.path().join("stale.manifest.json");
    rejected(run(
        &[
            "render",
            path(&persisted),
            "--format",
            "svg",
            "-o",
            path(&svg),
            "--manifest",
            path(&manifest),
        ],
        &[],
    ));
    assert!(!svg.exists());
    assert!(!manifest.exists());
    let refreshed = value(&success(run(&["normalize", path(&persisted)], &[])));
    assert_eq!(
        refreshed["mir"]["views"][0]["scales"],
        original["mir"]["views"][0]["scales"]
    );
    assert_eq!(
        refreshed["mir"]["views"][0]["guides"],
        original["mir"]["views"][0]["guides"]
    );
    assert_eq!(refreshed["mir"]["spaces"], original["mir"]["spaces"]);
    assert_eq!(refreshed["theme"], original["theme"]);
    assert_eq!(refreshed["mir"]["data"], edited["mir"]["data"]);
    assert_eq!(
        refreshed["mir"]["views"][0]["mark"]["instances"][0]["key"],
        "api-tue"
    );
    assert_eq!(
        refreshed["mir"]["views"][0]["mark"]["instances"][0]["value"].as_f64(),
        Some(15.)
    );
    write(&persisted, &refreshed);
    success(run(&["lower", path(&persisted)], &[]));
    assert_eq!(
        value(&success(run(&["normalize", path(&persisted)], &[]))),
        refreshed
    );
    let mut out_of_range = refreshed;
    out_of_range["mir"]["data"]["data/samples"]["operator"]["rows"][0]["value"] = json!(21);
    write(&persisted, &out_of_range);
    rejected(run(&["normalize", path(&persisted)], &[]));
    assert_clean(dir.path());
}

#[test]
fn numeric_legend_formats_are_explicit_and_rounded_ambiguity_is_diagnosed() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    authored["views"][0]["color"]["number_format"] = json!({"notation":"fixed","precision":2});
    write(&input, &authored);
    let scene = value(&success(run(&["lower", path(&input)], &[])));
    let mut all = Vec::new();
    flatten(scene["nodes"].as_array().unwrap(), &mut all);
    let texts: Vec<_> = all.iter().filter_map(|n| n["text"].as_str()).collect();
    assert!(texts.contains(&"[0.00, 10.00)"));
    assert!(texts.contains(&"[10.00, 20.00]"));
    authored["views"][0]["color"]["number_format"] = json!({"notation":"scientific","precision":1});
    write(&input, &authored);
    let scene = value(&success(run(&["lower", path(&input)], &[])));
    let mut all = Vec::new();
    flatten(scene["nodes"].as_array().unwrap(), &mut all);
    assert!(
        all.iter()
            .any(|n| n["text"].as_str().is_some_and(|s| s.contains("1.0e1")))
    );
    authored["views"][0]["color"]["domain"] = json!([0.0, 0.1]);
    authored["views"][0]["color"]["number_format"] = json!({"notation":"fixed","precision":0});
    for row in authored["datasets"]["samples"]["rows"]
        .as_array_mut()
        .unwrap()
    {
        row["value"] = json!(0.05);
    }
    write(&input, &authored);
    rejected(run(&["lower", path(&input)], &[]));
}

#[test]
fn composition_03_runs_mixed_heatmaps_and_area_through_the_normal_cli() {
    let dir = tempfile::tempdir().unwrap();
    let example = root().join("examples/composition/heatmap-dashboard.compose.yaml");
    let original = fs::read(&example).unwrap();
    let hir_file = dir.path().join("dashboard.json");
    success(run(
        &["compose", path(&example), "-o", path(&hir_file)],
        &[],
    ));
    let hir = value(&fs::read(&hir_file).unwrap());
    assert_eq!(hir["version"], "0.4");
    assert_eq!(hir["views"].as_array().unwrap().len(), 4);
    success(run(&["validate", path(&hir_file)], &[]));
    let mir = value(&success(run(&["normalize", path(&hir_file)], &[])));
    assert_eq!(mir["version"], "0.4");
    assert_eq!(mir["source_hir_version"], "0.4");
    let scene = value(&success(run(&["lower", path(&hir_file)], &[])));
    assert_eq!(cells(&scene).len(), 17);
    let svg = dir.path().join("dashboard.svg");
    success(run(
        &[
            "render",
            path(&hir_file),
            "--format",
            "svg",
            "-o",
            path(&svg),
        ],
        &[],
    ));
    let xml = fs::read_to_string(svg).unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        parsed
            .descendants()
            .filter(|n| n.has_tag_name("path")
                && n.attribute("id")
                    .is_some_and(|id| id.starts_with("comparison/area/")))
            .count(),
        2
    );
    success(run(
        &[
            "explain",
            path(&hir_file),
            "--node",
            "coverage/cell/api-tue",
        ],
        &[],
    ));
    assert_eq!(fs::read(example).unwrap(), original);
    assert_clean(dir.path());
}

#[test]
fn measured_heatmap_titles_replay_under_existing_wrap_profiles_2_3_and_4() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source();
    authored["views"][0]["title"] = "中文（字体测量），保留标点。\nAV office".into();
    authored["width"] = json!(1320);
    authored["height"] = json!(780);
    authored["datasets"]["labels"] = json!({"key":"id","rows":[
        {"id":"label","category":"A complete category label","value":3}
    ]});
    authored["views"].as_array_mut().unwrap().extend([
        json!({"kind":"chart.bar","id":"bars","dataset":"labels",
            "frame":{"x":820,"y":0,"width":500,"height":520},
            "category":{"field":"category"},"value":{"field":"value"}}),
        json!({"kind":"diagram.graph","id":"notes","layout":"manual",
            "frame":{"x":820,"y":540,"width":500,"height":220},
            "nodes":[{"id":"note","label":"AV office","position":{"x":150,"y":100}}],
            "edges":[]}),
    ]);
    write(&input, &authored);
    let (profile, fonts) = measured(dir.path());
    let layout = dir.path().join("layout.json");
    let persisted = dir.path().join("compiled.json");
    for wrap_profile in [
        "vizir-text-wrap/2",
        "vizir-text-wrap/3",
        "vizir-text-wrap/4",
    ] {
        let mut policy = json!({"profile":wrap_profile,"engine":vizir_compiler::TEXT_LAYOUT_ENGINE,
            "targets":[],"semantic_targets":[{"view_id":"chart","role":"chart.title",
                "max_width":240.0,"max_lines":4,"line_height":32.0}]});
        if wrap_profile != "vizir-text-wrap/2" {
            policy["semantic_targets"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "view_id":"bars","role":"bar.category_labels","max_width":120.0,
                    "max_lines":3,"line_height":16.0
                }));
        }
        if wrap_profile == "vizir-text-wrap/4" {
            policy["diagram_targets"] = json!([{"view_id":"notes","node_id":"note",
                "max_width":132.0,"max_lines":2,"line_height":20.0}]);
        }
        write(&layout, &policy);
        let common = [
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ];
        let mut args = vec!["normalize", path(&input)];
        args.extend(common);
        let compiled_bytes = success(run(&args, &fonts));
        let compiled = value(&compiled_bytes);
        assert_eq!(compiled["format"], "vizir-compiled-mir/1");
        assert_eq!(compiled["mir"]["version"], "0.4");
        assert_eq!(compiled["mir"]["source_hir_version"], "0.4");
        assert_eq!(compiled["context"]["text_layout"], policy);
        let typed: vizir_compiler::CompiledMir = serde_json::from_value(compiled.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), compiled);
        fs::write(&persisted, compiled_bytes).unwrap();
        let mut args = vec!["lower", path(&input)];
        args.extend(common);
        let direct = success(run(&args, &fonts));
        let replay = success(run(&["lower", path(&persisted)], &fonts));
        assert_eq!(direct, replay);
        let scene = value(&replay);
        assert_eq!(cells(&scene).len(), 3);
        let mut nodes = Vec::new();
        flatten(scene["nodes"].as_array().unwrap(), &mut nodes);
        let title = nodes.iter().find(|n| n["id"] == "chart/title").unwrap();
        assert_eq!(title["type"], "path");
        assert!(
            title["origin"]["explanation"]
                .as_str()
                .unwrap()
                .ends_with(authored["views"][0]["title"].as_str().unwrap())
        );
        assert_eq!(
            value(&success(run(&["normalize", path(&persisted)], &fonts))),
            compiled
        );
        let mut wrong = compiled;
        wrong["mir"]["source_hir_version"] = "0.3".into();
        write(&persisted, &wrong);
        rejected(run(&["lower", path(&persisted)], &fonts));
    }
}

#[test]
fn executable_heatmap_examples_need_no_external_resources() {
    for name in [
        "sparse-heatmap.viz.yaml",
        "dense-heatmap.viz.yaml",
        "constant-heatmap.viz.yaml",
    ] {
        let input = root().join("examples/chart").join(name);
        success(run(&["validate", path(&input)], &[]));
        success(run(&["normalize", path(&input)], &[]));
        success(run(&["lower", path(&input)], &[]));
    }
}

#[test]
fn native_heatmap_png_has_transparent_and_visible_pixels_when_renderer_is_available() {
    if !Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("rsvg-convert unavailable; native PNG smoke is skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("examples/chart/dense-heatmap.viz.yaml");
    let output = dir.path().join("heatmap.png");
    success(run(
        &[
            "render",
            path(&input),
            "--format",
            "png",
            "--background",
            "transparent",
            "-o",
            path(&output),
        ],
        &[],
    ));
    let file = std::io::BufReader::new(fs::File::open(output).unwrap());
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).unwrap();
    assert_eq!((info.width, info.height), (960, 560));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let pixels = &bytes[..info.buffer_size()];
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
    assert_clean(dir.path());
}
