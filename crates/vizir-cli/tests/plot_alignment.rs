use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SENTINEL: &[u8] = b"existing output survives rejected alignment\n";
const MEMBERS: [&str; 3] = ["line", "scatter", "area"];

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn run(args: &[&str]) -> Output {
    with_fonts(args, &[])
}
fn with_fonts(args: &[&str], fonts: &[String]) -> Output {
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
fn rejected(output: Output, label: &str) {
    assert!(
        !output.status.success(),
        "{label}: rejected input unexpectedly succeeded"
    );
    assert!(
        output.stdout.is_empty(),
        "{label}: failure published stdout"
    );
    assert!(
        !output.stderr.is_empty(),
        "{label}: failure omitted diagnostic"
    );
}
fn read(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn write(p: &Path, value: &Value) {
    fs::write(p, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plot-alignment-mixed.compose.yaml")
}
fn composition() -> Value {
    serde_yaml::from_slice(&fs::read(fixture()).unwrap()).unwrap()
}
fn hir() -> Value {
    serde_json::from_slice(&success(run(&["compose", path(&fixture())]))).unwrap()
}
fn clean(dir: &Path) {
    assert!(fs::read_dir(dir).unwrap().all(|p| {
        !p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vizir-")
    }));
}
fn mirrored(mir: Value) -> Value {
    json!({"format":"vizir-compiled-mir/1","context":{},"mir":mir})
}
fn normalize(source: &Value, dir: &Path) -> Value {
    let input = dir.join("normalize-source.json");
    write(&input, source);
    serde_json::from_slice(&success(run(&["normalize", path(&input)]))).unwrap()
}
fn shared(source: &mut Value) {
    source["shared_legend"] = json!({"id":"series-key","members":MEMBERS,"placement":"bottom","height":64,"gap":12,"title":"Series"});
}
fn view<'a>(mir: &'a Value, id: &str) -> &'a Value {
    mir["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
}
fn scale<'a>(chart: &'a Value, axis: &str) -> &'a Value {
    let id = chart["mark"][axis]["scale"].as_str().unwrap();
    chart["scales"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
}
fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn insets(chart: &Value) -> [f64; 4] {
    let frame = &chart["frame"];
    let x = &scale(chart, "x")["range"];
    let y = &scale(chart, "y")["range"];
    [
        number(&x[0]) - number(&frame["x"]),
        number(&frame["x"]) + number(&frame["width"]) - number(&x[1]),
        number(&y[1]) - number(&frame["y"]),
        number(&frame["y"]) + number(&frame["height"]) - number(&y[0]),
    ]
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}
fn check_alignment(mir: &Value, local: &Value) {
    assert_eq!(mir["version"], "0.9");
    assert_eq!(mir["source_hir_version"], "0.9");
    assert_eq!(mir["plot_alignment"]["id"], "aligned");
    assert_eq!(mir["plot_alignment"]["mode"], "uniform");
    let mut expected = [0.0_f64; 4];
    for id in MEMBERS {
        let chart = view(local, id);
        for (i, value) in insets(chart).into_iter().enumerate() {
            expected[i] = expected[i].max(value);
        }
    }
    for (index, id) in MEMBERS.into_iter().enumerate() {
        let aligned = view(mir, id);
        let original = view(local, id);
        assert_eq!(
            mir["plot_alignment"]["members"][index],
            json!({"view":id,"x_scale":scale(aligned,"x")["id"],"y_scale":scale(aligned,"y")["id"]})
        );
        for (i, actual) in insets(aligned).into_iter().enumerate() {
            near(actual, expected[i]);
        }
        for axis in ["x", "y"] {
            assert_eq!(
                scale(aligned, axis)["domain"],
                scale(original, axis)["domain"]
            );
            assert_eq!(scale(aligned, axis)["id"], scale(original, axis)["id"]);
        }
        for field in ["frame", "mark", "guides", "source", "key_expression"] {
            assert_eq!(aligned[field], original[field], "{id} {field}");
        }
    }
    assert_eq!(mir["data"], local["data"]);
    assert_eq!(mir["expressions"], local["expressions"]);
    assert_ne!(
        scale(view(mir, "line"), "x")["domain"],
        scale(view(mir, "scatter"), "x")["domain"]
    );
    assert_ne!(
        scale(view(mir, "line"), "y")["domain"],
        scale(view(mir, "scatter"), "y")["domain"]
    );
}
fn flattened<'a>(nodes: &'a Value, result: &mut Vec<&'a Value>) {
    if let Some(nodes) = nodes.as_array() {
        for n in nodes {
            result.push(n);
            flattened(&n["children"], result);
        }
    }
}

fn check_scene_ranges(mir: &Value, bytes: &[u8]) {
    let scene: Value = serde_json::from_slice(bytes).unwrap();
    let mut nodes = Vec::new();
    flattened(&scene["nodes"], &mut nodes);
    for id in MEMBERS {
        let chart = view(mir, id);
        let x = &scale(chart, "x")["range"];
        let y = &scale(chart, "y")["range"];
        for (axis, from, to) in [
            ("x", json!({"x":x[0],"y":y[0]}), json!({"x":x[1],"y":y[0]})),
            ("y", json!({"x":x[0],"y":y[1]}), json!({"x":x[0],"y":y[0]})),
        ] {
            let node_id = format!("{id}/axis/{axis}");
            let node = nodes.iter().find(|n| n["id"] == node_id).unwrap();
            assert_eq!(node["from"], from);
            assert_eq!(node["to"], to);
        }
    }
}

#[test]
fn mixed_charts_align_explicitly_with_local_or_shared_legends_and_exact_replay() {
    for shared_legend in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let input = t.path().join("source.json");
        let compose_input = t.path().join("source.compose.json");
        let mut source = composition();
        if shared_legend {
            shared(&mut source);
        }
        write(&compose_input, &source);
        success(run(&["compose", path(&compose_input), "-o", path(&input)]));
        let hir = read(&input);
        assert_eq!(hir["version"], "0.9");
        assert_eq!(hir["plot_alignment"], source["plot_alignment"]);
        for chart in hir["views"].as_array().unwrap() {
            assert_eq!(chart["frame"]["width"], hir["views"][0]["frame"]["width"]);
            assert_eq!(chart["frame"]["height"], hir["views"][0]["frame"]["height"]);
        }
        success(run(&["validate", path(&input)]));
        let mir = normalize(&hir, t.path());
        let mut local_hir = hir.clone();
        local_hir.as_object_mut().unwrap().remove("plot_alignment");
        let local = normalize(&local_hir, t.path());
        assert!(local.get("plot_alignment").is_none());
        assert_ne!(
            insets(view(&local, "line")),
            insets(view(&local, "area")),
            "omission must not infer alignment"
        );
        check_alignment(&mir, &local);
        if shared_legend {
            assert_eq!(mir["shared_legend"]["id"], "series-key");
            assert_eq!(mir["shared_legend"]["members"].as_array().unwrap().len(), 3);
        } else {
            assert!(mir.get("shared_legend").is_none());
        }
        let replay = t.path().join("compiled.json");
        write(&replay, &mirrored(mir.clone()));
        let direct_svg = t.path().join("direct.svg");
        let replay_svg = t.path().join("replay.svg");
        success(run(&[
            "render",
            path(&input),
            "--format",
            "svg",
            "-o",
            path(&direct_svg),
        ]));
        success(run(&[
            "render",
            path(&replay),
            "--format",
            "svg",
            "-o",
            path(&replay_svg),
        ]));
        assert_eq!(
            fs::read(&direct_svg).unwrap(),
            fs::read(&replay_svg).unwrap()
        );
        let direct_scene = success(run(&["lower", path(&input)]));
        check_scene_ranges(&mir, &direct_scene);
        assert_eq!(direct_scene, success(run(&["lower", path(&replay)])));
        let scene: Value = serde_json::from_slice(&direct_scene).unwrap();
        let mut nodes = Vec::new();
        flattened(&scene["nodes"], &mut nodes);
        let shared_owner = nodes
            .iter()
            .filter(|n| n["id"] == "shared-legend:10:series-key")
            .count();
        assert_eq!(shared_owner, usize::from(shared_legend));
        for id in MEMBERS {
            assert_eq!(
                nodes.iter().any(|n| n["id"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("{id}/legend/"))),
                !shared_legend
            );
        }
        assert!(
            !nodes.iter().any(|n| n["id"] == "aligned"),
            "alignment has no duplicate scene owner"
        );
        clean(t.path());
    }
}

#[test]
fn nonmembers_keep_their_local_layout_and_member_count_boundaries_are_explicit() {
    let t = tempfile::tempdir().unwrap();
    let mut source = hir();
    source["plot_alignment"]["members"] = json!(["line", "scatter"]);
    let aligned = normalize(&source, t.path());
    source.as_object_mut().unwrap().remove("plot_alignment");
    let local = normalize(&source, t.path());
    assert_eq!(view(&aligned, "area"), view(&local, "area"));
    for count in [64, 65] {
        let mut source = hir();
        let mut template = source["views"][0].clone();
        template.as_object_mut().unwrap().remove("title");
        template.as_object_mut().unwrap().remove("series");
        source["width"] = json!(4800);
        source["height"] = json!(5400);
        let mut views = Vec::new();
        let mut members = Vec::new();
        for index in 0..count {
            let mut chart = template.clone();
            let id = format!("panel-{index}");
            chart["id"] = json!(id);
            chart["frame"] = json!({"x":(index%8)*600,"y":(index/8)*600,"width":600,"height":600});
            views.push(chart);
            members.push(id);
        }
        source["views"] = json!(views);
        source["plot_alignment"]["members"] = json!(members);
        let input = t.path().join("boundary.json");
        write(&input, &source);
        let result = run(&["normalize", path(&input)]);
        if count == 64 {
            let mir: Value = serde_json::from_slice(&success(result)).unwrap();
            assert_eq!(
                mir["plot_alignment"]["members"].as_array().unwrap().len(),
                64
            );
        } else {
            rejected(result, "65 members");
        }
    }
}

#[test]
fn malformed_composition_hir_and_replay_are_rejected_before_publication() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("input.json");
    let output = t.path().join("existing.svg");
    let manifest = t.path().join("manifest.json");
    let mut compositions = Vec::new();
    for value in [Value::Null, json!({}), json!([])] {
        let mut source = composition();
        source["plot_alignment"] = value;
        compositions.push(("malformed group".to_owned(), source));
    }
    for (field, value) in [
        ("id", json!("")),
        ("id", Value::Null),
        ("mode", json!("shared-domains")),
        ("mode", Value::Null),
        ("members", Value::Null),
        ("members", json!([])),
        ("members", json!(["line"])),
        ("members", json!(["line", "line"])),
        ("members", json!(["line", "missing"])),
        ("members", json!(["line", 1])),
        ("unknown", json!(true)),
    ] {
        let mut source = composition();
        source["plot_alignment"][field] = value;
        compositions.push((format!("composition {field}"), source));
    }
    for version in 1..=7 {
        let mut source = composition();
        source["schema"] = json!(format!("vizir-composition/0.{version}"));
        compositions.push((format!("composition version {version}"), source));
    }
    for (label, source) in compositions {
        write(&input, &source);
        fs::write(&output, SENTINEL).unwrap();
        rejected(run(&["compose", path(&input), "-o", path(&output)]), &label);
        assert_eq!(fs::read(&output).unwrap(), SENTINEL);
        clean(t.path());
    }
    let original = hir();
    let original_mir = normalize(&original, t.path());
    let mut invalid = Vec::new();
    for version in 1..=8 {
        let mut source = original.clone();
        source["version"] = json!(format!("0.{version}"));
        invalid.push((format!("HIR version {version}"), source));
    }
    for value in [Value::Null, json!({}), json!([])] {
        let mut source = original.clone();
        source["plot_alignment"] = value;
        invalid.push(("HIR malformed group".to_owned(), source));
    }
    for (field, value) in [
        ("members", json!(["line", "line"])),
        ("members", json!(["line", "absent"])),
        ("members", json!(["line"])),
        ("mode", json!("independent")),
        ("unknown", json!(true)),
    ] {
        let mut source = original.clone();
        source["plot_alignment"][field] = value;
        invalid.push((format!("HIR {field}"), source));
    }
    for dimension in ["width", "height"] {
        let mut source = original.clone();
        source["views"][1]["frame"][dimension] =
            json!(number(&source["views"][1]["frame"][dimension]) - 1.0);
        invalid.push((format!("unequal HIR {dimension}"), source));
    }
    let mut unsupported = original.clone();
    unsupported["views"][1] = json!({"kind":"chart.bar","id":"scatter","frame":original["views"][1]["frame"],"dataset":"samples","category":{"field":"series"},"value":{"field":"y"}});
    invalid.push(("bar group member".to_owned(), unsupported));
    for (version, source_version) in [("0.8", "0.8"), ("0.9", "0.8"), ("0.8", "0.9")] {
        let mut mir = original_mir.clone();
        mir["version"] = json!(version);
        mir["source_hir_version"] = json!(source_version);
        invalid.push((
            format!("MIR versions {version}/{source_version}"),
            mirrored(mir),
        ));
    }
    for value in [Value::Null, json!({}), json!([])] {
        let mut mir = original_mir.clone();
        mir["plot_alignment"] = value;
        invalid.push(("MIR malformed group".to_owned(), mirrored(mir)));
    }
    for (field, value) in [
        ("view", json!("missing")),
        ("x_scale", json!("missing")),
        ("y_scale", json!("scatter/y")),
        ("x_scale", json!("line/y")),
        ("unknown", json!(true)),
    ] {
        let mut mir = original_mir.clone();
        mir["plot_alignment"]["members"][0][field] = value;
        invalid.push((format!("MIR member {field}"), mirrored(mir)));
    }
    let mut duplicate = original_mir.clone();
    duplicate["plot_alignment"]["members"][1] = duplicate["plot_alignment"]["members"][0].clone();
    invalid.push(("MIR duplicate member".to_owned(), mirrored(duplicate)));
    for dimension in ["width", "height"] {
        let mut mir = original_mir.clone();
        mir["views"][1]["frame"][dimension] =
            json!(number(&mir["views"][1]["frame"][dimension]) - 1.0);
        invalid.push((format!("unequal MIR {dimension}"), mirrored(mir)));
    }
    for all in [false, true] {
        for delta in [-1.0, 1.0] {
            let mut mir = original_mir.clone();
            for index in 0..if all { 3 } else { 1 } {
                let range = &mut mir["views"][index]["scales"][0]["range"];
                range[0] = json!(number(&range[0]) + delta);
            }
            invalid.push((
                format!("tampered range all={all} delta={delta}"),
                mirrored(mir),
            ));
        }
    }
    for (label, source) in invalid {
        write(&input, &source);
        fs::write(&output, SENTINEL).unwrap();
        fs::write(&manifest, SENTINEL).unwrap();
        rejected(
            run(&[
                "render",
                path(&input),
                "--format",
                "svg",
                "-o",
                path(&output),
                "--manifest",
                path(&manifest),
            ]),
            &label,
        );
        assert_eq!(fs::read(&output).unwrap(), SENTINEL, "{label}");
        assert_eq!(fs::read(&manifest).unwrap(), SENTINEL, "{label}");
        clean(t.path());
    }
}

fn measured_profile(dir: &Path) -> (PathBuf, Vec<String>) {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vizir-compiler/tests/fixtures/wrapping-fonts");
    let manifest = read(&fixture_dir.join("manifest.json"));
    let mut faces = serde_json::Map::new();
    let mut mappings = Vec::new();
    for (index, font) in manifest["fonts"].as_array().unwrap().iter().enumerate() {
        let copied = dir.join(format!("resource-{index}.otf"));
        fs::copy(fixture_dir.join(font["file"].as_str().unwrap()), &copied).unwrap();
        faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(),json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
        mappings.push(format!(
            "{}={}",
            font["sha256"].as_str().unwrap(),
            copied.display()
        ));
    }
    let profile = dir.join("profile.json");
    write(
        &profile,
        &json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,"declared_locale":"zh-CN","shaping_language":"default","faces":faces}),
    );
    (profile, mappings)
}

#[test]
fn measured_wrapped_titles_align_and_replay_from_copied_font_resources() {
    for shared_legend in [false, true] {
        let compile_dir = tempfile::tempdir().unwrap();
        let replay_dir = tempfile::tempdir().unwrap();
        let (profile, fonts) = measured_profile(compile_dir.path());
        let compose_input = compile_dir.path().join("source.compose.json");
        let input = compile_dir.path().join("source.json");
        let mut source = composition();
        if shared_legend {
            shared(&mut source);
            source["shared_legend"]["title"] = json!("图例");
        }
        write(&compose_input, &source);
        success(run(&["compose", path(&compose_input), "-o", path(&input)]));
        let mut source = read(&input);
        source["views"][0]["title"] = json!("Measured title wraps across lines.\n中文测试");
        source["views"][0]["y"]["label"] = json!("测量值");
        write(&input, &source);
        let policy = compile_dir.path().join("text-layout.json");
        write(
            &policy,
            &json!({"profile":vizir_compiler::TEXT_LAYOUT_SEMANTIC_PROFILE,"engine":vizir_compiler::TEXT_LAYOUT_ENGINE,"targets":[],"semantic_targets":[{"view_id":"line","role":"chart.title","max_width":210.0,"max_lines":8,"line_height":28.0}]}),
        );
        let opts = [
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&policy),
            "--theme",
            "azure",
        ];
        let compiled = replay_dir.path().join("compiled.json");
        let mut args = vec!["normalize", path(&input), "-o", path(&compiled)];
        args.extend(opts);
        success(with_fonts(&args, &fonts));
        let persisted = read(&compiled);
        assert_eq!(persisted["format"], "vizir-compiled-mir/1");
        assert_eq!(persisted["mir"]["version"], "0.9");
        let mut local = source.clone();
        local.as_object_mut().unwrap().remove("plot_alignment");
        let local_input = compile_dir.path().join("local.json");
        write(&local_input, &local);
        let mut args = vec!["normalize", path(&local_input)];
        args.extend(opts);
        let local: Value = serde_json::from_slice(&success(with_fonts(&args, &fonts))).unwrap();
        check_alignment(&persisted["mir"], &local["mir"]);
        assert!(
            insets(view(&persisted["mir"], "area"))[2] > insets(view(&local["mir"], "area"))[2]
        );
        let serialized = fs::read_to_string(&compiled).unwrap();
        assert!(!serialized.contains(path(compile_dir.path())));
        assert!(!serialized.contains(".otf"));
        let svg = compile_dir.path().join("direct.svg");
        let mut args = vec!["render", path(&input), "--format", "svg", "-o", path(&svg)];
        args.extend(opts);
        success(with_fonts(&args, &fonts));
        let direct_svg = fs::read(&svg).unwrap();
        let mut args = vec!["lower", path(&input)];
        args.extend(opts);
        let direct_scene = success(with_fonts(&args, &fonts));
        check_scene_ranges(&persisted["mir"], &direct_scene);
        let copied: Vec<String> = fonts
            .iter()
            .enumerate()
            .map(|(index, mapping)| {
                let (hash, original) = mapping.split_once('=').unwrap();
                let dest = replay_dir.path().join(format!("copied-{index}.otf"));
                fs::copy(original, &dest).unwrap();
                format!("{hash}={}", dest.display())
            })
            .collect();
        drop(compile_dir);
        success(with_fonts(&["validate", path(&compiled)], &copied));
        assert_eq!(
            direct_scene,
            success(with_fonts(&["lower", path(&compiled)], &copied))
        );
        let replay_svg = replay_dir.path().join("replay.svg");
        let manifest = replay_dir.path().join("manifest.json");
        success(with_fonts(
            &[
                "render",
                path(&compiled),
                "--format",
                "svg",
                "-o",
                path(&replay_svg),
                "--manifest",
                path(&manifest),
            ],
            &copied,
        ));
        assert_eq!(direct_svg, fs::read(&replay_svg).unwrap());
        assert_eq!(read(&manifest)["compilation_context"], persisted["context"]);
        let rendered = fs::read_to_string(&replay_svg).unwrap();
        let xml = roxmltree::Document::parse(&rendered).unwrap();
        assert!(!xml.descendants().any(|n| n.has_tag_name("text")));
        assert!(xml.descendants().any(|n| n.has_tag_name("path")));
        let refreshed = replay_dir.path().join("refreshed.json");
        success(with_fonts(
            &["normalize", path(&compiled), "-o", path(&refreshed)],
            &copied,
        ));
        assert_eq!(persisted, read(&refreshed));
        let manifest_bytes = fs::read(&manifest).unwrap();
        rejected(
            run(&[
                "render",
                path(&compiled),
                "--format",
                "svg",
                "-o",
                path(&replay_svg),
                "--manifest",
                path(&manifest),
            ]),
            "missing copied fonts",
        );
        assert_eq!(direct_svg, fs::read(&replay_svg).unwrap());
        assert_eq!(manifest_bytes, fs::read(&manifest).unwrap());
        clean(replay_dir.path());
    }
}

#[test]
fn csv_import_preserves_alignment_group_and_versions() {
    let t = tempfile::tempdir().unwrap();
    let csv = t.path().join("samples.csv");
    let spec = t.path().join("spec.json");
    let template = t.path().join("template.json");
    let output = t.path().join("imported.json");
    let receipt = t.path().join("receipt.json");
    fs::write(
        &csv,
        "id,x,y,series\na0,0,1,Alpha\na1,1,4,Alpha\nb0,0,6,Beta\nb1,1,9,Beta\n",
    )
    .unwrap();
    write(
        &spec,
        &json!({"format":"vizir-csv-import/1","key":"id","columns":[{"name":"id","type":"string"},{"name":"x","type":"float64"},{"name":"y","type":"float64"},{"name":"series","type":"string"}]}),
    );
    for (kind, source, version) in [
        ("composition", composition(), "vizir-composition/0.8"),
        ("hir", hir(), "0.9"),
    ] {
        write(&template, &source);
        success(run(&[
            "import-csv",
            path(&csv),
            "--template",
            path(&template),
            "--template-kind",
            kind,
            "--dataset",
            "samples",
            "--replace-dataset",
            "--spec",
            path(&spec),
            "--output",
            path(&output),
            "--provenance",
            path(&receipt),
        ]));
        let imported = read(&output);
        assert_eq!(imported["plot_alignment"], source["plot_alignment"]);
        assert_eq!(
            imported[if kind == "composition" {
                "schema"
            } else {
                "version"
            }],
            version
        );
        assert_eq!(read(&receipt)["template"]["source_version"], version);
        assert_eq!(read(&receipt)["output"]["source_version"], version);
        assert_eq!(imported["datasets"]["samples"]["rows"][0]["y"], 1.0);
        let hir = if kind == "composition" {
            serde_json::from_slice(&success(run(&["compose", path(&output)]))).unwrap()
        } else {
            imported
        };
        let mir = normalize(&hir, t.path());
        assert_eq!(mir["plot_alignment"]["id"], "aligned");
    }
    clean(t.path());
}
