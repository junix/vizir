use serde_json::{Value, json};
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
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty(), "failure needs a diagnostic");
}
fn value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}
fn write(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn source(grouped: bool) -> Value {
    let rows = if grouped {
        json!([
            {"id":"b2","x":2,"y":3,"group":"Beta"},
            {"id":"a1","x":1,"y":2,"group":"Alpha"},
            {"id":"b0","x":0,"y":5,"group":"Beta"},
            {"id":"a2","x":2,"y":4,"group":"Alpha"},
            {"id":"a0","x":0,"y":1,"group":"Alpha"}
        ])
    } else {
        json!([
            {"id":"late","x":2,"y":-2},
            {"id":"first","x":0,"y":4},
            {"id":"middle","x":1,"y":7}
        ])
    };
    let mut d = json!({
        "version":"0.3","id":"area-cli","width":640,"height":440,
        "datasets":{"samples":{"key":"id","rows":rows}},
        "views":[{"kind":"chart.area","id":"chart","title":"Area source",
            "frame":{"x":0,"y":0,"width":640,"height":440},"dataset":"samples",
            "x":{"field":"x","label":"time"},"y":{"field":"y","label":"value"},
            "baseline":0,"order":"x-ascending"}]
    });
    if grouped {
        d["views"][0]["series"] = json!({"field":"group","palette":["#3366CC","#009E73"]});
    }
    d
}
fn flatten<'a>(nodes: &'a [Value], output: &mut Vec<&'a Value>) {
    for node in nodes {
        output.push(node);
        if let Some(children) = node["children"].as_array() {
            flatten(children, output);
        }
    }
}
fn area_paths(scene: &Value) -> Vec<&Value> {
    let mut all = Vec::new();
    flatten(scene["nodes"].as_array().unwrap(), &mut all);
    all.into_iter()
        .filter(|n| n["id"].as_str().is_some_and(|id| id.contains("/area/")))
        .collect()
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
fn ungrouped_area_normalizes_sorts_cache_and_preserves_source_rows() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let authored = source(false);
    write(&input, &authored);
    success(run(&["validate", path(&input)], &[]));
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    assert_eq!(mir["version"], "0.3");
    assert_eq!(mir["source_hir_version"], "0.3");
    assert_eq!(
        mir["data"]["data/samples"]["operator"]["rows"],
        authored["datasets"]["samples"]["rows"]
    );
    let mark = &mir["views"][0]["mark"];
    assert_eq!(mark["type"], "area");
    assert_eq!(mark["baseline"].as_f64(), Some(0.));
    assert_eq!(mark["series"].as_array().unwrap().len(), 1);
    let points = mark["series"][0]["points"].as_array().unwrap();
    assert_eq!(
        points
            .iter()
            .map(|p| p["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "middle", "late"]
    );
    assert!(
        points
            .windows(2)
            .all(|p| p[0]["x"].as_f64().unwrap() < p[1]["x"].as_f64().unwrap())
    );
    let scene = value(&success(run(&["lower", path(&input)], &[])));
    let paths = area_paths(&scene);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0]["id"], "chart/area/series");
    assert_eq!(paths[0]["type"], "path");
    assert_eq!(paths[0]["style"]["opacity"].as_f64(), Some(0.35));
    assert_eq!(
        paths[0]["commands"].as_array().unwrap().last().unwrap()["op"],
        "close"
    );
    let explanation = String::from_utf8(success(run(
        &["explain", path(&input), "--node", "chart/area/series"],
        &[],
    )))
    .unwrap();
    assert!(explanation.contains("chart/area/series"));
    assert_eq!(value(&fs::read(&input).unwrap()), authored);
}

#[test]
fn grouped_area_emits_ordered_series_explicit_guides_and_closed_svg_paths() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    write(&input, &source(true));
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    let chart = &mir["views"][0];
    let series = chart["mark"]["series"].as_array().unwrap();
    assert_eq!(
        series
            .iter()
            .map(|s| s["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Alpha", "Beta"]
    );
    let guides = chart["guides"].as_array().unwrap();
    assert_eq!(guides.iter().filter(|g| g["kind"] == "axis").count(), 2);
    assert_eq!(guides.iter().filter(|g| g["kind"] == "legend").count(), 1);
    let svg = dir.path().join("area.svg");
    success(run(
        &["render", path(&input), "--format", "svg", "-o", path(&svg)],
        &[],
    ));
    let xml = fs::read_to_string(svg).unwrap();
    let xml = roxmltree::Document::parse(&xml).unwrap();
    let paths = xml
        .descendants()
        .filter(|n| {
            n.has_tag_name("path")
                && n.attribute("id")
                    .is_some_and(|id| id.starts_with("chart/area/"))
        })
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].attribute("id"), Some("chart/area/Alpha"));
    assert_eq!(paths[1].attribute("id"), Some("chart/area/Beta"));
    for p in paths {
        assert_eq!(p.attribute("opacity"), Some("0.35"));
        assert!(p.attribute("d").unwrap().trim_end().ends_with('Z'));
        assert!(p.attribute("fill").is_some_and(|fill| fill != "none"));
    }
}

#[test]
fn direct_mir_area_requires_explicit_axes_and_legend_bound_to_its_mark() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    write(&input, &source(true));
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    let guides = mir["views"][0]["guides"].as_array().unwrap();
    assert_eq!(guides.len(), 3);
    for index in 0..guides.len() {
        let mut bad = mir.clone();
        bad["views"][0]["guides"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        write(&input, &bad);
        rejected(run(&["lower", path(&input)], &[]));
    }
    let mut bad = mir.clone();
    bad["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .push(guides[0].clone());
    write(&input, &bad);
    rejected(run(&["lower", path(&input)], &[]));
    let mut bad = mir.clone();
    let y_scale = bad["views"][0]["mark"]["y"]["scale"].clone();
    let bottom = bad["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["orient"] == "bottom")
        .unwrap();
    bottom["scale"] = y_scale;
    write(&input, &bad);
    rejected(run(&["lower", path(&input)], &[]));
}

#[test]
fn area_is_rejected_by_old_hir_mir_and_composition_versions() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let base = source(false);
    write(&input, &base);
    let mir = value(&success(run(&["normalize", path(&input)], &[])));
    for version in ["0.1", "0.2"] {
        let mut old = base.clone();
        old["version"] = version.into();
        write(&input, &old);
        rejected(run(&["lower", path(&input)], &[]));
        let mut old = mir.clone();
        old["version"] = version.into();
        old["source_hir_version"] = version.into();
        write(&input, &old);
        rejected(run(&["lower", path(&input)], &[]));
    }
    let mut panel = base["views"][0].clone();
    panel.as_object_mut().unwrap().remove("frame");
    let composition = json!({"schema":"vizir-composition/0.1","id":"old-area","width":640,"height":440,"layout":{"kind":"grid","columns":1},"datasets":base["datasets"],"panels":[panel]});
    write(&input, &composition);
    rejected(run(&["compose", path(&input)], &[]));
}

#[test]
fn missing_and_unsupported_area_options_are_not_silently_defaulted() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    for field in ["baseline", "order"] {
        let mut d = source(false);
        d["views"][0].as_object_mut().unwrap().remove(field);
        write(&input, &d);
        rejected(run(&["lower", path(&input)], &[]));
        d["views"][0][field] = Value::Null;
        write(&input, &d);
        rejected(run(&["lower", path(&input)], &[]));
    }
    for (field, option) in [
        ("order", json!("source")),
        ("stacked", json!(true)),
        ("interpolation", json!("step")),
        ("show_points", json!(true)),
        ("opacity", json!(0.8)),
        ("line_width", json!(2)),
    ] {
        let mut d = source(false);
        d["views"][0][field] = option;
        write(&input, &d);
        rejected(run(&["lower", path(&input)], &[]));
    }
}

#[test]
fn invalid_samples_preserve_existing_svg_manifest_and_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let svg = dir.path().join("area.svg");
    let manifest = dir.path().join("area.manifest.json");
    write(&input, &source(false));
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
    let rows = [
        json!([{"id":"one","x":0,"y":1}]),
        json!([{"id":"a","x":0,"y":1},{"id":"b","x":0,"y":2}]),
        json!([{"id":"a","x":-0.0,"y":1},{"id":"b","x":0.0,"y":2}]),
        json!([{"id":"a","x":9007199254740992_i64,"y":1},{"id":"b","x":9007199254740993_i64,"y":2}]),
        json!([{"id":"a","x":0,"y":null},{"id":"b","x":1,"y":2}]),
        json!([{"id":"a","y":1},{"id":"b","x":1,"y":2}]),
        json!([{"id":"a","x":0,"y":1},{"id":"a","x":1,"y":2}]),
        json!([{"id":"a","x":0,"y":-1e308},{"id":"b","x":1,"y":1e308}]),
    ];
    for rows in rows {
        let mut bad = source(false);
        bad["datasets"]["samples"]["rows"] = rows;
        write(&input, &bad);
        let original = fs::read(&input).unwrap();
        rejected(run(&args, &[]));
        assert_eq!(fs::read(&svg).unwrap(), prior_svg);
        assert_eq!(fs::read(&manifest).unwrap(), prior_manifest);
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_clean(dir.path());
    }
    let mut bad = source(true);
    bad["datasets"]["samples"]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("group");
    write(&input, &bad);
    rejected(run(&args, &[]));
    assert_eq!(fs::read(&svg).unwrap(), prior_svg);
    assert_eq!(fs::read(&manifest).unwrap(), prior_manifest);
    assert_clean(dir.path());
}

#[test]
fn composition_02_runs_the_mixed_dashboard_through_normal_commands() {
    let dir = tempfile::tempdir().unwrap();
    let example = root().join("examples/composition/area-dashboard.compose.yaml");
    let original = fs::read(&example).unwrap();
    let hir_file = dir.path().join("dashboard.json");
    success(run(
        &["compose", path(&example), "-o", path(&hir_file)],
        &[],
    ));
    let hir = value(&fs::read(&hir_file).unwrap());
    assert_eq!(hir["version"], "0.3");
    assert_eq!(hir["views"].as_array().unwrap().len(), 4);
    success(run(&["validate", path(&hir_file)], &[]));
    let mir = value(&success(run(&["normalize", path(&hir_file)], &[])));
    assert_eq!(mir["version"], "0.3");
    assert_eq!(mir["source_hir_version"], "0.3");
    let scene = value(&success(run(&["lower", path(&hir_file)], &[])));
    assert_eq!(area_paths(&scene).len(), 3);
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
    roxmltree::Document::parse(&fs::read_to_string(svg).unwrap()).unwrap();
    success(run(
        &["explain", path(&hir_file), "--node", "signals/area/Alpha"],
        &[],
    ));
    assert_eq!(fs::read(example).unwrap(), original);
    assert_clean(dir.path());
}

#[test]
fn themed_measured_area_title_context_replays_as_matching_03() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let mut authored = source(true);
    authored["views"][0]["title"] = "中文（字体测量），保留标点。\nAV office".into();
    write(&input, &authored);
    let (profile, fonts) = measured(dir.path());
    let layout = dir.path().join("layout.json");
    let policy = json!({"profile":"vizir-text-wrap/2","engine":vizir_compiler::TEXT_LAYOUT_ENGINE,"targets":[],"semantic_targets":[{"view_id":"chart","role":"chart.title","max_width":160.0,"max_lines":4,"line_height":32.0}]});
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
    assert_eq!(compiled["mir"]["version"], "0.3");
    assert_eq!(compiled["mir"]["source_hir_version"], "0.3");
    assert_eq!(compiled["context"]["text_layout"], policy);
    let typed: vizir_compiler::CompiledMir = serde_json::from_value(compiled.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), compiled);
    let persisted = dir.path().join("compiled.json");
    fs::write(&persisted, compiled_bytes).unwrap();
    let mut args = vec!["lower", path(&input)];
    args.extend(common);
    let direct = success(run(&args, &fonts));
    let replay = success(run(&["lower", path(&persisted)], &fonts));
    assert_eq!(direct, replay);
    let scene = value(&replay);
    assert_eq!(area_paths(&scene).len(), 2);
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
    wrong["mir"]["source_hir_version"] = "0.2".into();
    write(&persisted, &wrong);
    rejected(run(&["lower", path(&persisted)], &fonts));
}

#[test]
fn authored_area_examples_run_without_external_resources() {
    for name in ["baseline-area.viz.yaml", "grouped-area.viz.yaml"] {
        let input = root().join("examples/chart").join(name);
        success(run(&["validate", path(&input)], &[]));
        success(run(&["normalize", path(&input)], &[]));
        success(run(&["lower", path(&input)], &[]));
    }
}

#[test]
fn native_area_png_has_transparent_and_visible_pixels_when_renderer_is_available() {
    if !Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("rsvg-convert unavailable; native PNG smoke is skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("examples/chart/grouped-area.viz.yaml");
    let output = dir.path().join("area.png");
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
    assert_eq!((info.width, info.height), (1000, 560));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let pixels = &bytes[..info.buffer_size()];
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
    assert_clean(dir.path());
}
