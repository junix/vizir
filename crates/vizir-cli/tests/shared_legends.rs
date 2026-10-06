use serde_json::{Value, json};
use std::{
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const OWNER: &str = "shared-legend:10:series-key";
const SENTINEL: &[u8] = b"existing artifact must survive rejected shared legend\n";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
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
fn read(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn write(p: &Path, value: &Value) {
    fs::write(p, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn example() -> PathBuf {
    workspace().join("examples/composition/shared-legend-grid.compose.yaml")
}
fn composition() -> Value {
    serde_yaml::from_slice(&fs::read(example()).unwrap()).unwrap()
}
fn hir() -> Value {
    serde_json::from_slice(&success(run(&["compose", path(&example())]))).unwrap()
}
fn flattened<'a>(nodes: &'a Value, result: &mut Vec<&'a Value>) {
    if let Some(nodes) = nodes.as_array() {
        for node in nodes {
            result.push(node);
            flattened(&node["children"], result);
        }
    }
}
fn node<'a>(nodes: &[&'a Value], id: &str) -> &'a Value {
    nodes
        .iter()
        .copied()
        .find(|n| n["id"] == id)
        .unwrap_or_else(|| panic!("missing scene node {id}"))
}
fn clean(dir: &Path) {
    assert!(fs::read_dir(dir).unwrap().all(|p| {
        !p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vizir-")
    }));
}
fn rejected(output: Output) {
    assert!(
        !output.status.success(),
        "rejected input unexpectedly succeeded"
    );
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn native_compose_normalize_svg_and_compiled_replay_preserve_one_owner() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("shared.viz.json");
    let normalized = t.path().join("shared.mir.json");
    let compiled = t.path().join("shared.compiled.json");
    let direct_svg = t.path().join("direct.svg");
    let replay_svg = t.path().join("replay.svg");
    success(run(&["compose", path(&example()), "-o", path(&input)]));
    let source = read(&input);
    assert_eq!(source["version"], "0.8");
    let legend_frame = json!({"x":24.0,"y":392.0,"width":1072.0,"height":64.0});
    assert_eq!(source["shared_legend"]["frame"], legend_frame);
    assert_eq!(
        source["shared_legend"]["members"],
        json!(["all-series", "beta-only"])
    );
    for (i, x) in [24.0, 572.0].into_iter().enumerate() {
        assert_eq!(
            source["views"][i]["frame"],
            json!({"x":x,"y":24.0,"width":524.0,"height":356.0})
        );
    }
    let json_source = t.path().join("shared.compose.json");
    write(&json_source, &composition());
    assert_eq!(
        source,
        serde_json::from_slice::<Value>(&success(run(&["compose", path(&json_source)]))).unwrap()
    );
    success(run(&["validate", path(&input)]));
    success(run(&["normalize", path(&input), "-o", path(&normalized)]));
    let mir = read(&normalized);
    assert_eq!(mir["version"], "0.8");
    assert_eq!(mir["source_hir_version"], "0.8");
    assert_eq!(mir["shared_legend"]["frame"], legend_frame);
    for (index, chart) in mir["views"].as_array().unwrap().iter().enumerate() {
        let scale = chart["scales"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["type"] == "ordinal-color")
            .unwrap();
        assert_eq!(scale["domain"], json!(["Alpha", "Beta", "Reserved"]));
        assert_eq!(mir["shared_legend"]["members"][index]["view"], chart["id"]);
        assert_eq!(mir["shared_legend"]["members"][index]["scale"], scale["id"]);
        assert!(
            !chart["guides"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g["kind"] == "legend")
        );
    }
    write(
        &compiled,
        &json!({"format":"vizir-compiled-mir/1","context":{},"mir":mir}),
    );
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
        path(&compiled),
        "--format",
        "svg",
        "-o",
        path(&replay_svg),
    ]));
    assert_eq!(
        fs::read(&direct_svg).unwrap(),
        fs::read(&replay_svg).unwrap()
    );
    let scene: Value = serde_json::from_slice(&success(run(&["lower", path(&input)]))).unwrap();
    let replay_scene: Value =
        serde_json::from_slice(&success(run(&["lower", path(&compiled)]))).unwrap();
    assert_eq!(scene, replay_scene);
    let mut nodes = Vec::new();
    flattened(&scene["nodes"], &mut nodes);
    assert_eq!(node(&nodes, OWNER)["origin"]["hir_node"], "series-key");
    assert_eq!(node(&nodes, OWNER)["origin"]["mir_node"], "series-key");
    assert_eq!(
        node(&nodes, OWNER)["origin"]["data_lineage"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !nodes
            .iter()
            .any(|n| n["id"].as_str().unwrap().contains("/legend/"))
    );
    for (index, label) in ["Alpha", "Beta", "Reserved"].into_iter().enumerate() {
        assert_eq!(
            node(&nodes, &format!("{OWNER}/entry/{index}/label"))["text"],
            label
        );
    }
    assert_eq!(
        node(&nodes, "all-series/series/Beta")["style"]["stroke"],
        node(&nodes, "beta-only/series/Beta")["style"]["stroke"]
    );
    assert!(
        !nodes
            .iter()
            .any(|n| n["id"].as_str().unwrap().contains("/series/Reserved"))
    );
    assert!(!nodes.iter().any(|n| n["id"] == "beta-only/series/Alpha"));
    let svg = fs::read_to_string(&direct_svg).unwrap();
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert_eq!(
        xml.descendants()
            .filter(|n| n.attribute("id") == Some(OWNER))
            .count(),
        1
    );
    for label in ["Alpha", "Beta", "Reserved"] {
        assert_eq!(
            xml.descendants()
                .filter(|n| n.has_tag_name("text") && n.text() == Some(label))
                .count(),
            1
        );
    }
    clean(t.path());
}

#[test]
fn omission_keeps_old_versions_and_their_local_legends() {
    let original = workspace().join("examples/composition/shared-numeric-domains.compose.yaml");
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("old.json");
    let old: Value = serde_json::from_slice(&success(run(&["compose", path(&original)]))).unwrap();
    assert_eq!(old["version"], "0.7");
    assert!(old.get("shared_legend").is_none());
    assert_eq!(old["views"][0]["frame"]["height"], 432.0);
    write(&input, &old);
    let mir: Value = serde_json::from_slice(&success(run(&["normalize", path(&input)]))).unwrap();
    assert!(mir.get("shared_legend").is_none());
    let scene: Value = serde_json::from_slice(&success(run(&["lower", path(&input)]))).unwrap();
    let mut nodes = Vec::new();
    flattened(&scene["nodes"], &mut nodes);
    for id in ["all-series/legend/0/swatch", "beta-only/legend/0/swatch"] {
        node(&nodes, id);
    }
}

#[test]
fn rejected_composition_hir_and_replay_never_replace_outputs() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("input.json");
    let output = t.path().join("existing.svg");
    let manifest = t.path().join("existing.manifest.json");
    let mut invalid_compositions = Vec::new();
    for value in [Value::Null, json!({}), json!([])] {
        let mut source = composition();
        source["shared_legend"] = value;
        invalid_compositions.push(source);
    }
    for (field, value) in [
        ("placement", json!("top")),
        ("placement", Value::Null),
        ("height", json!(0)),
        ("gap", json!(-1)),
        ("members", json!(["all-series"])),
        ("members", json!(["all-series", "all-series"])),
        ("members", json!(["all-series", "unknown"])),
        ("unknown", json!(true)),
        ("title", json!("two\nlines")),
        ("title", Value::Null),
        ("members", Value::Null),
        ("height", Value::Null),
        ("gap", Value::Null),
    ] {
        let mut source = composition();
        source["shared_legend"][field] = value;
        invalid_compositions.push(source);
    }
    for version in 1..=6 {
        let mut source = composition();
        source["schema"] = json!(format!("vizir-composition/0.{version}"));
        invalid_compositions.push(source);
    }
    for source in invalid_compositions {
        write(&input, &source);
        fs::write(&output, SENTINEL).unwrap();
        rejected(run(&["compose", path(&input), "-o", path(&output)]));
        assert_eq!(fs::read(&output).unwrap(), SENTINEL);
        clean(t.path());
    }
    let original = hir();
    let mut invalid_inputs = Vec::new();
    for version in 1..=7 {
        let mut source = original.clone();
        source["version"] = json!(format!("0.{version}"));
        invalid_inputs.push(source);
    }
    for value in [Value::Null, json!({})] {
        let mut source = original.clone();
        source["shared_legend"] = value;
        invalid_inputs.push(source);
    }
    for (field, value) in [("placement", json!("left")), ("unknown", json!(true))] {
        let mut source = original.clone();
        source["shared_legend"][field] = value;
        invalid_inputs.push(source);
    }
    write(&input, &original);
    let original_mir: Value =
        serde_json::from_slice(&success(run(&["normalize", path(&input)]))).unwrap();
    for (version, source_version) in [("0.7", "0.7"), ("0.8", "0.7"), ("0.7", "0.8")] {
        let mut mir = original_mir.clone();
        mir["version"] = json!(version);
        mir["source_hir_version"] = json!(source_version);
        invalid_inputs.push(json!({"format":"vizir-compiled-mir/1","context":{},"mir":mir}));
    }
    let mut null_mir = original_mir;
    null_mir["shared_legend"] = Value::Null;
    invalid_inputs.push(json!({"format":"vizir-compiled-mir/1","context":{},"mir":null_mir}));
    for source in invalid_inputs {
        write(&input, &source);
        fs::write(&output, SENTINEL).unwrap();
        fs::write(&manifest, SENTINEL).unwrap();
        rejected(run(&[
            "render",
            path(&input),
            "--format",
            "svg",
            "-o",
            path(&output),
            "--manifest",
            path(&manifest),
        ]));
        assert_eq!(fs::read(&output).unwrap(), SENTINEL);
        assert_eq!(fs::read(&manifest).unwrap(), SENTINEL);
        clean(t.path());
    }
}

fn measured_profile(dir: &Path) -> (PathBuf, Vec<String>) {
    let fixtures = workspace().join("crates/vizir-compiler/tests/fixtures/fonts");
    let manifest = read(&fixtures.join("manifest.json"));
    let mut faces = serde_json::Map::new();
    let mut mappings = Vec::new();
    for (index, font) in manifest["fonts"].as_array().unwrap().iter().enumerate() {
        let resource = dir.join(format!("resource-{index}.otf"));
        fs::copy(fixtures.join(font["file"].as_str().unwrap()), &resource).unwrap();
        faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(), json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
        mappings.push(format!(
            "{}={}",
            font["sha256"].as_str().unwrap(),
            resource.display()
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
fn measured_cjk_legend_replays_from_copied_resources_without_original_paths() {
    let compile_dir = tempfile::tempdir().unwrap();
    let replay_dir = tempfile::tempdir().unwrap();
    let (profile, fonts) = measured_profile(compile_dir.path());
    let input = compile_dir.path().join("source.json");
    let mut source = hir();
    source["shared_legend"]["title"] = json!("图例");
    for chart in source["views"].as_array_mut().unwrap() {
        chart["series"]["domain"] = json!(["甲", "乙", "丙"]);
    }
    for dataset in source["datasets"].as_object_mut().unwrap().values_mut() {
        for row in dataset["rows"].as_array_mut().unwrap() {
            row["series"] = json!(if row["series"] == "Alpha" {
                "甲"
            } else {
                "乙"
            });
        }
    }
    write(&input, &source);
    let compiled = replay_dir.path().join("compiled.json");
    success(with_fonts(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
            "-o",
            path(&compiled),
        ],
        &fonts,
    ));
    let persisted = read(&compiled);
    assert_eq!(persisted["format"], "vizir-compiled-mir/1");
    assert_eq!(persisted["mir"]["version"], "0.8");
    assert_eq!(persisted["mir"]["shared_legend"]["title"], "图例");
    let compiled_text = fs::read_to_string(&compiled).unwrap();
    assert!(!compiled_text.contains(path(compile_dir.path())));
    assert!(!compiled_text.contains(".otf"));
    let direct_svg = compile_dir.path().join("direct.svg");
    success(with_fonts(
        &[
            "render",
            path(&input),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
            "--format",
            "svg",
            "-o",
            path(&direct_svg),
        ],
        &fonts,
    ));
    let direct_bytes = fs::read(&direct_svg).unwrap();
    let direct_scene = success(with_fonts(
        &[
            "lower",
            path(&input),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
        ],
        &fonts,
    ));
    let copied_fonts: Vec<String> = fonts
        .iter()
        .enumerate()
        .map(|(index, mapping)| {
            let (hash, original) = mapping.split_once('=').unwrap();
            let copied = replay_dir.path().join(format!("copied-{index}.otf"));
            fs::copy(original, &copied).unwrap();
            format!("{hash}={}", copied.display())
        })
        .collect();
    drop(compile_dir);
    success(with_fonts(&["validate", path(&compiled)], &copied_fonts));
    assert_eq!(
        direct_scene,
        success(with_fonts(&["lower", path(&compiled)], &copied_fonts))
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
        &copied_fonts,
    ));
    assert_eq!(direct_bytes, fs::read(&replay_svg).unwrap());
    let svg = fs::read_to_string(&replay_svg).unwrap();
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert!(!xml.descendants().any(|n| n.has_tag_name("text")));
    assert_eq!(
        xml.descendants()
            .filter(|n| n.attribute("id") == Some(OWNER))
            .count(),
        1
    );
    assert!(xml.descendants().any(|n| n.has_tag_name("path")));
    assert_eq!(read(&manifest)["compilation_context"], persisted["context"]);
    let refreshed = replay_dir.path().join("refreshed.json");
    success(with_fonts(
        &["normalize", path(&compiled), "-o", path(&refreshed)],
        &copied_fonts,
    ));
    assert_eq!(persisted, read(&refreshed));
    rejected(run(&[
        "render",
        path(&compiled),
        "--format",
        "svg",
        "-o",
        path(&replay_svg),
        "--manifest",
        path(&manifest),
    ]));
    assert_eq!(direct_bytes, fs::read(&replay_svg).unwrap());
    clean(replay_dir.path());
}

fn renderer_available() -> bool {
    ["rsvg-convert", "magick"].iter().any(|binary| {
        Command::new(binary)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

#[test]
fn real_transparent_png_contains_visible_shared_strip_when_renderer_available() {
    if !renderer_available() {
        eprintln!("shared legend PNG test skipped: no native renderer available");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("source.json");
    let png = t.path().join("shared.png");
    write(&input, &hir());
    success(run(&[
        "render",
        path(&input),
        "--format",
        "png",
        "--background",
        "transparent",
        "-o",
        path(&png),
    ]));
    let mut decoder = png::Decoder::new(BufReader::new(fs::File::open(png).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).unwrap();
    assert_eq!((info.width, info.height), (1120, 480));
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        other => panic!("expected decoded alpha channel, got {other:?}"),
    };
    let alpha: Vec<u8> = buffer[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|p| p[channels - 1])
        .collect();
    assert!(alpha.contains(&0));
    assert!(alpha.iter().any(|a| *a > 0));
    assert!(
        alpha[392 * 1120..456 * 1120].iter().any(|a| *a > 0),
        "legend strip must contain visible pixels"
    );
    assert!(
        alpha[..24 * 1120].iter().all(|a| *a == 0),
        "outer padding must remain transparent"
    );
}

#[test]
fn csv_import_preserves_shared_legend_source_versions_and_owner() {
    let t = tempfile::tempdir().unwrap();
    let input_csv = t.path().join("replacement.csv");
    let spec = t.path().join("spec.json");
    let template = t.path().join("template.json");
    let output = t.path().join("imported.json");
    let receipt = t.path().join("receipt.json");
    fs::write(&input_csv, "id,x,y,series\nb0,0,6,Beta\nb1,1,9,Beta\n").unwrap();
    write(
        &spec,
        &json!({"format":"vizir-csv-import/1","key":"id","columns":[
            {"name":"id","type":"string"},{"name":"x","type":"float64"},
            {"name":"y","type":"float64"},{"name":"series","type":"string"}
        ]}),
    );
    for (kind, source, version) in [
        ("composition", composition(), "vizir-composition/0.7"),
        ("hir", hir(), "0.8"),
    ] {
        write(&template, &source);
        success(run(&[
            "import-csv",
            path(&input_csv),
            "--template",
            path(&template),
            "--template-kind",
            kind,
            "--dataset",
            "subset",
            "--replace-dataset",
            "--spec",
            path(&spec),
            "--output",
            path(&output),
            "--provenance",
            path(&receipt),
        ]));
        let imported = read(&output);
        if kind == "composition" {
            let expected: vizir_core::CompositionSharedLegend =
                serde_json::from_value(source["shared_legend"].clone()).unwrap();
            let actual: vizir_core::CompositionSharedLegend =
                serde_json::from_value(imported["shared_legend"].clone()).unwrap();
            assert_eq!(actual, expected);
        } else {
            assert_eq!(imported["shared_legend"], source["shared_legend"]);
        }
        assert_eq!(
            imported[if kind == "composition" {
                "schema"
            } else {
                "version"
            }],
            version
        );
        assert_eq!(imported["datasets"]["subset"]["rows"][0]["y"], 6.0);
        assert_eq!(read(&receipt)["template"]["source_version"], version);
        assert_eq!(read(&receipt)["output"]["source_version"], version);
        if kind == "composition" {
            let imported_hir: Value =
                serde_json::from_slice(&success(run(&["compose", path(&output)]))).unwrap();
            assert_eq!(imported_hir["version"], "0.8");
        } else {
            success(run(&["validate", path(&output)]));
        }
    }
    clean(t.path());
}
