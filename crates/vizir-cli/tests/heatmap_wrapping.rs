use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use vizir_compiler::{SemanticTextLayoutTarget, TextLayoutContext};
fn run(args: &[&str], fonts: &[String]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_vizir"));
    c.args(args);
    for f in fonts {
        c.args(["--font", f]);
    }
    c.output().unwrap()
}
fn ok(o: Output) -> Vec<u8> {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}
fn p(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn write(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn read(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn fixture(dir: &Path) -> (PathBuf, PathBuf, PathBuf, Vec<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile = root.join("examples/text/wrapping-font-profile.json");
    let manifest =
        read(&root.join("crates/vizir-compiler/tests/fixtures/wrapping-fonts/manifest.json"));
    let fonts = manifest["fonts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            format!(
                "{}={}",
                f["sha256"].as_str().unwrap(),
                root.join("crates/vizir-compiler/tests/fixtures/wrapping-fonts")
                    .join(f["file"].as_str().unwrap())
                    .display()
            )
        })
        .collect();
    let input = dir.join("input.json");
    write(
        &input,
        &json!({"version":"0.5","id":"wrapped","width":720,"height":400,"datasets":{"d":{"key":"id","rows":[{"id":"a","x":"中文字体测量测试","y":"测试","v":3},{"id":"b","x":"office e\u{301} office e\u{301}","y":"测试","v":5}]}},"views":[{"kind":"chart.heatmap","id":"h","frame":{"x":0,"y":0,"width":720,"height":400},"dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"v","domain":[0,10]}}]}),
    );
    let layout = dir.join("layout.json");
    write(
        &layout,
        &serde_json::to_value(TextLayoutContext::new(vec![]).with_heatmap_x_labels(vec![
            SemanticTextLayoutTarget::heatmap_x_category_labels("h", 60., 4, 16.),
        ]))
        .unwrap(),
    );
    (input, profile, layout, fonts)
}
#[test]
fn all_native_workflows_replay_exactly_without_paths_or_policy_file() {
    let d = tempfile::tempdir().unwrap();
    let (input, profile, layout, fonts) = fixture(d.path());
    let saved = d.path().join("compiled.json");
    let svg = d.path().join("direct.svg");
    let replay = d.path().join("replay.svg");
    for command in ["validate", "normalize", "lower"] {
        let o = ok(run(
            &[
                command,
                p(&input),
                "--text-profile",
                p(&profile),
                "--text-layout",
                p(&layout),
            ],
            &fonts,
        ));
        if command == "normalize" {
            fs::write(&saved, o).unwrap();
        }
    }
    let before = fs::read(&saved).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains(p(d.path())));
    assert_eq!(read(&saved)["context"]["text_layout"], read(&layout));
    ok(run(
        &[
            "render",
            p(&input),
            "--format",
            "svg",
            "--text-profile",
            p(&profile),
            "--text-layout",
            p(&layout),
            "-o",
            p(&svg),
        ],
        &fonts,
    ));
    fs::remove_file(&layout).unwrap();
    ok(run(
        &["render", p(&saved), "--format", "svg", "-o", p(&replay)],
        &fonts,
    ));
    assert_eq!(fs::read(&svg).unwrap(), fs::read(&replay).unwrap());
    assert_eq!(ok(run(&["normalize", p(&saved)], &fonts)), before);
    let explanation = ok(run(
        &["explain", p(&saved), "--node", "h/axis/x/category/0"],
        &fonts,
    ));
    assert!(String::from_utf8_lossy(&explanation).contains("heatmap.x_category_labels"));
}
#[test]
fn invalid_width_line_glyph_source_and_profile_never_publish_outputs() {
    let d = tempfile::tempdir().unwrap();
    let (input, profile, layout, fonts) = fixture(d.path());
    let original = read(&input);
    let policy = read(&layout);
    let output = d.path().join("old.svg");
    let manifest = d.path().join("old.manifest.json");
    for case in [
        "width",
        "lines",
        "height",
        "word",
        "glyph",
        "hard_break",
        "old_profile",
        "missing_target",
    ] {
        let mut source = original.clone();
        let mut policy = policy.clone();
        match case {
            "width" => policy["semantic_targets"][0]["max_width"] = 1.into(),
            "lines" => policy["semantic_targets"][0]["max_lines"] = 1.into(),
            "height" => policy["semantic_targets"][0]["line_height"] = 0.25.into(),
            "word" => {
                source["datasets"]["d"]["rows"][0]["x"] = "unbreakablewordthatcannotfit".into()
            }
            "glyph" => source["datasets"]["d"]["rows"][0]["x"] = "🦀".into(),
            "hard_break" => source["datasets"]["d"]["rows"][0]["x"] = "A\nB".into(),
            "old_profile" => policy["profile"] = "vizir-text-wrap/4".into(),
            _ => policy["semantic_targets"][0]["view_id"] = "missing".into(),
        };
        write(&input, &source);
        write(&layout, &policy);
        fs::write(&output, b"old SVG").unwrap();
        fs::write(&manifest, b"old manifest").unwrap();
        let o = run(
            &[
                "render",
                p(&input),
                "--text-profile",
                p(&profile),
                "--text-layout",
                p(&layout),
                "--format",
                "svg",
                "-o",
                p(&output),
                "--manifest",
                p(&manifest),
            ],
            &fonts,
        );
        assert!(!o.status.success(), "{case}");
        assert!(o.stdout.is_empty());
        assert_eq!(fs::read(&output).unwrap(), b"old SVG");
        assert_eq!(fs::read(&manifest).unwrap(), b"old manifest");
        assert!(fs::read_dir(d.path()).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vizir-")
        }));
    }
}
#[test]
fn schema_gates_new_role_without_widening_old_profiles() {
    let s = vizir_compiler::compiled_mir_schema();
    assert_eq!(
        s,
        serde_json::from_str::<Value>(include_str!("../../../schemas/compiled-mir.schema.json"))
            .unwrap()
    );
    assert_eq!(
        s["$defs"]["CategoryTextLayoutRole"]["enum"],
        json!(["chart.title", "bar.category_labels"])
    );
    assert_eq!(
        s["$defs"]["HeatmapTextLayoutRole"]["enum"],
        json!([
            "chart.title",
            "bar.category_labels",
            "heatmap.x_category_labels"
        ])
    );
    let h = &s["$defs"]["HeatmapTextLayoutContext"];
    assert_eq!(h["properties"]["profile"]["const"], "vizir-text-wrap/5");
    assert_eq!(
        h["properties"]["semantic_targets"]["contains"]["properties"]["role"]["const"],
        "heatmap.x_category_labels"
    );
    assert_eq!(h["additionalProperties"], false);
}
