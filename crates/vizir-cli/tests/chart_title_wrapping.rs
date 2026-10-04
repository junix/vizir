use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vizir_compiler::{
    SemanticTextLayoutTarget, TEXT_LAYOUT_ENGINE, TEXT_LAYOUT_PROFILE,
    TEXT_LAYOUT_SEMANTIC_PROFILE, TextLayoutContext, TextLayoutRole, TextLayoutTarget,
    parse_text_layout_context_json,
};

const TITLE: &str = "Measured chart title wraps across lines.\n\n中文测试\n";

fn run(args: &[&str], fonts: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    command.args(args);
    for font in fonts {
        command.args(["--font", font]);
    }
    command.output().unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn rejected(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn write(p: &Path, value: &Value) {
    fs::write(p, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn read(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn parse(value: &Value) -> vizir_core::VizResult<TextLayoutContext> {
    parse_text_layout_context_json(&serde_json::to_vec(value).unwrap())
}
fn policy() -> Value {
    json!({"profile":TEXT_LAYOUT_SEMANTIC_PROFILE,"engine":TEXT_LAYOUT_ENGINE,"targets":[],
        "semantic_targets":[{"view_id":"chart","role":"chart.title","max_width":210.0,"max_lines":12,"line_height":32.0}]})
}
fn geometry_policy() -> Value {
    json!({"profile":TEXT_LAYOUT_PROFILE,"engine":TEXT_LAYOUT_ENGINE,"targets":[
        {"view_id":"labels","node_id":"body","max_width":140.0,"max_lines":12,"line_height":32.0}]})
}
fn source(dir: &Path, kind: &str, title: Option<&str>) -> PathBuf {
    let mut chart = json!({"kind":kind,"id":"chart","dataset":"data",
        "frame":{"x":0,"y":0,"width":640,"height":640}});
    if let Some(title) = title {
        chart["title"] = title.into();
    }
    if kind == "chart.bar" {
        chart["category"] = json!({"field":"category"});
        chart["value"] = json!({"field":"y"});
    } else {
        chart["x"] = json!({"field":"x"});
        chart["y"] = json!({"field":"y"});
    }
    let p = dir.join("source.json");
    write(
        &p,
        &json!({"version":"0.2","id":"title-wrapping-cli","width":640,"height":640,
        "datasets":{"data":{"key":"id","rows":[
            {"id":"a","category":"A","x":1,"y":2},
            {"id":"b","category":"B","x":2,"y":3}]}},"views":[chart]}),
    );
    p
}
fn measured(dir: &Path) -> (PathBuf, Vec<String>) {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vizir-compiler/tests/fixtures/wrapping-fonts");
    let manifest = read(&fixture_dir.join("manifest.json"));
    let mut faces = serde_json::Map::new();
    let mut mappings = Vec::new();
    for font in manifest["fonts"].as_array().unwrap() {
        faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(), json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
        mappings.push(format!(
            "{}={}",
            font["sha256"].as_str().unwrap(),
            fixture_dir.join(font["file"].as_str().unwrap()).display()
        ));
    }
    let p = dir.join("profile.json");
    write(
        &p,
        &json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,
        "declared_locale":"zh-CN","shaping_language":"default","faces":faces}),
    );
    (p, mappings)
}

#[test]
fn public_constructors_preserve_v1_bytes_and_opt_into_v2_explicitly() {
    let geometry = TextLayoutTarget::new("labels", "body", 140.0, 12, 32.0);
    let original = TextLayoutContext::new(vec![geometry]);
    assert_eq!(serde_json::to_value(&original).unwrap(), geometry_policy());
    assert_eq!(
        serde_json::to_string(&original).unwrap(),
        format!(
            "{{\"profile\":\"{TEXT_LAYOUT_PROFILE}\",\"engine\":\"{TEXT_LAYOUT_ENGINE}\",\"targets\":[{{\"view_id\":\"labels\",\"node_id\":\"body\",\"max_width\":140.0,\"max_lines\":12,\"line_height\":32.0}}]}}"
        )
    );
    assert!(original.semantic_targets.is_none());
    let semantic = SemanticTextLayoutTarget::chart_title("chart", 210.0, 12, 32.0);
    assert_eq!(semantic.role, TextLayoutRole::ChartTitle);
    let combined = original.clone().with_semantic_targets(vec![semantic]);
    assert_eq!(combined.profile, TEXT_LAYOUT_SEMANTIC_PROFILE);
    assert_eq!(combined.targets, original.targets);
    assert_eq!(combined.semantic_targets.as_ref().unwrap().len(), 1);
    combined.validate().unwrap();
    assert_eq!(
        parse(&serde_json::to_value(combined.clone()).unwrap()).unwrap(),
        combined
    );
}

#[test]
fn strict_profiles_reject_all_cross_version_semantic_spellings() {
    parse(&geometry_policy()).unwrap();
    parse(&policy()).unwrap();
    for value in [json!([]), Value::Null, policy()["semantic_targets"].clone()] {
        let mut v1 = geometry_policy();
        v1["semantic_targets"] = value;
        assert!(parse(&v1).is_err(), "v1 accepted {v1}");
        assert!(
            serde_json::from_value::<TextLayoutContext>(v1.clone()).is_err(),
            "generic serde accepted {v1}"
        );
    }
    for field in ["semantic_targets", "targets"] {
        let mut missing = policy();
        missing.as_object_mut().unwrap().remove(field);
        assert!(parse(&missing).is_err(), "{missing}");
        let mut null = policy();
        null[field] = Value::Null;
        assert!(parse(&null).is_err(), "{null}");
    }
    let mut empty = policy();
    empty["semantic_targets"] = json!([]);
    assert!(parse(&empty).is_err());
    let mut role_in_geometry = geometry_policy();
    role_in_geometry["targets"][0]["role"] = "chart.title".into();
    assert!(parse(&role_in_geometry).is_err());
    // Explicit null rejects even in generic serde, rather than normalizing into
    // an absent field before profile validation or strict envelope checks.
    let raw = serde_json::to_string(&policy()).unwrap().replace(
        &serde_json::to_string(&policy()["semantic_targets"]).unwrap(),
        "null",
    );
    assert!(serde_json::from_str::<TextLayoutContext>(&raw).is_err());
}

#[test]
fn semantic_policy_checks_numeric_limits_id_bytes_roles_and_duplicates() {
    for (field, value) in [
        ("max_width", json!(0)),
        ("max_width", json!(1_000_001)),
        ("line_height", json!(0)),
        ("line_height", json!(1_000_001)),
        ("max_lines", json!(0)),
        ("max_lines", json!(257)),
        ("max_lines", json!(1.25)),
        ("max_lines", json!(-1)),
        ("max_lines", json!(4294967296_u64)),
        ("max_lines", json!("12")),
        ("view_id", json!("")),
        ("view_id", json!("a".repeat(257))),
        ("view_id", json!("中".repeat(86))),
        ("role", json!("chart.category")),
        ("role", json!("diagram.title")),
        ("role", json!("title")),
        ("role", Value::Null),
        ("node_id", json!("title")),
        ("path", json!("never-open.ttf")),
    ] {
        let mut invalid = policy();
        invalid["semantic_targets"][0][field] = value;
        assert!(parse(&invalid).is_err(), "{invalid}");
    }
    for field in ["profile", "engine"] {
        let mut invalid = policy();
        invalid[field] = "unknown".into();
        assert!(parse(&invalid).is_err());
    }
    let mut duplicate = policy();
    let mut second = duplicate["semantic_targets"][0].clone();
    second["max_width"] = 300.into();
    duplicate["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert!(
        parse(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let encoded = serde_json::to_string(&policy()).unwrap();
    for (old, replacement) in [
        (
            "\"role\":\"chart.title\"",
            "\"role\":\"chart.title\",\"role\":\"chart.title\"",
        ),
        ("\"targets\":[]", "\"targets\":[],\"targets\":[]"),
        ("\"max_lines\":12", "\"max_lines\":12,\"max_lines\":12"),
    ] {
        let raw = encoded.replace(old, replacement);
        assert_ne!(raw, encoded);
        assert!(
            parse_text_layout_context_json(raw.as_bytes())
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
    for spelling in ["12.0", "12e0", "1.2e1"] {
        let raw = encoded.replace("\"max_lines\":12", &format!("\"max_lines\":{spelling}"));
        assert_eq!(
            parse_text_layout_context_json(raw.as_bytes()).unwrap(),
            parse(&policy()).unwrap()
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut typed = TextLayoutContext::new(vec![]).with_semantic_targets(vec![
            SemanticTextLayoutTarget::chart_title("chart", value, 12, 32.0),
        ]);
        assert!(typed.validate().is_err());
        let target = &mut typed.semantic_targets.as_mut().unwrap()[0];
        target.max_width = 210.0;
        target.line_height = value;
        assert!(typed.validate().is_err());
    }
}

#[test]
fn geometry_and_semantic_targets_share_one_combined_budget() {
    let mut mixed = policy();
    mixed["targets"] = geometry_policy()["targets"].clone();
    parse(&mixed).unwrap();
    let semantic = mixed["semantic_targets"][0].clone();
    mixed["semantic_targets"] = (0..255)
        .map(|i| {
            let mut target = semantic.clone();
            target["view_id"] = format!("chart-{i}").into();
            target
        })
        .collect::<Vec<_>>()
        .into();
    parse(&mixed).unwrap();
    let mut extra = semantic;
    extra["view_id"] = "chart-extra".into();
    mixed["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(parse(&mixed).unwrap_err().to_string().contains("256"));
    mixed["targets"] = json!([]);
    parse(&mixed).unwrap();
}

#[test]
fn compiled_envelope_uses_the_same_strict_profile_specific_wire_contract() {
    let legacy: Value =
        serde_json::from_str(include_str!("fixtures/legacy-theme.mir.json")).unwrap();
    let text: Value = serde_json::from_str(include_str!(
        "../../../examples/text/wrapping-font-profile.json"
    ))
    .unwrap();
    let mut compiled = json!({
        "format": vizir_compiler::COMPILED_MIR_FORMAT,
        "context": {"text": text, "text_layout": policy()},
        "mir": legacy
    });
    let parsed =
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(parsed).unwrap()["context"]["text_layout"],
        policy()
    );
    for value in [json!([]), Value::Null, policy()["semantic_targets"].clone()] {
        let mut v1 = geometry_policy();
        v1["semantic_targets"] = value;
        compiled["context"]["text_layout"] = v1;
        assert!(
            vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
                .is_err()
        );
        assert!(serde_json::from_value::<vizir_compiler::CompiledMir>(compiled.clone()).is_err());
    }
    for value in [json!([]), Value::Null] {
        compiled["context"]["text_layout"] = policy();
        compiled["context"]["text_layout"]["semantic_targets"] = value;
        assert!(
            vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
                .is_err()
        );
    }
    compiled["context"]["text_layout"] = policy();
    compiled["context"]["text_layout"]["semantic_targets"][0]["discarded"] = true.into();
    assert!(
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
    compiled["context"]["text_layout"] = policy();
    let raw = serde_json::to_string(&compiled).unwrap().replace(
        "\"role\":\"chart.title\"",
        "\"role\":\"chart.title\",\"role\":\"chart.title\"",
    );
    assert!(
        vizir_compiler::parse_compiled_mir_json(raw.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn semantic_schema_branches_keep_v1_closed_and_v2_explicit() {
    let schema = vizir_compiler::compiled_mir_schema();
    assert_eq!(
        schema,
        serde_json::from_str::<Value>(include_str!("../../../schemas/compiled-mir.schema.json"))
            .unwrap()
    );
    let v1 = &schema["$defs"]["GeometryTextLayoutContext"];
    let v2 = &schema["$defs"]["SemanticTextLayoutContext"];
    assert_eq!(v1["properties"]["profile"]["const"], TEXT_LAYOUT_PROFILE);
    assert_eq!(v1["additionalProperties"], false);
    assert!(v1["properties"].get("semantic_targets").is_none());
    assert_eq!(v1["properties"]["targets"]["minItems"], 1);
    assert_eq!(
        v2["properties"]["profile"]["const"],
        TEXT_LAYOUT_SEMANTIC_PROFILE
    );
    assert_eq!(v2["additionalProperties"], false);
    assert_eq!(v2["properties"]["targets"]["minItems"], 0);
    assert_eq!(v2["properties"]["semantic_targets"]["type"], "array");
    assert_eq!(v2["properties"]["semantic_targets"]["minItems"], 1);
    assert!(
        v2["required"]
            .as_array()
            .unwrap()
            .contains(&json!("semantic_targets"))
    );
    assert_eq!(
        schema["$defs"]["TextLayoutRole"]["enum"],
        json!(["chart.title"])
    );
}

#[test]
fn chart_title_roundtrips_every_cli_command_for_bar_line_and_scatter() {
    for kind in ["chart.bar", "chart.line", "chart.scatter"] {
        let t = tempfile::tempdir().unwrap();
        let input = source(t.path(), kind, Some(TITLE));
        let layout = t.path().join("layout.json");
        write(&layout, &policy());
        let (profile, fonts) = measured(t.path());
        let saved = t.path().join("compiled.json");
        let refreshed = t.path().join("refreshed.json");
        let svg = t.path().join("title.svg");
        let manifest = t.path().join("title.render.json");
        success(run(
            &[
                "validate",
                path(&input),
                "--theme",
                "azure",
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
            ],
            &fonts,
        ));
        success(run(
            &[
                "normalize",
                path(&input),
                "--theme",
                "azure",
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
                "-o",
                path(&saved),
            ],
            &fonts,
        ));
        let compiled = read(&saved);
        assert_eq!(compiled["context"]["text_layout"], policy());
        assert_eq!(compiled["mir"]["views"][0]["title"], TITLE);
        let encoded = fs::read_to_string(&saved).unwrap();
        assert!(!encoded.contains(path(t.path())));
        assert!(!encoded.contains(".otf"));
        let direct = success(run(
            &[
                "lower",
                path(&input),
                "--theme",
                "azure",
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
            ],
            &fonts,
        ));
        let replay = success(run(
            &["lower", path(&saved), "--text-layout", path(&layout)],
            &fonts,
        ));
        assert_eq!(direct, replay, "{kind}");
        success(run(
            &[
                "normalize",
                path(&saved),
                "--text-layout",
                path(&layout),
                "-o",
                path(&refreshed),
            ],
            &fonts,
        ));
        assert_eq!(read(&refreshed), compiled);
        success(run(
            &[
                "render",
                path(&saved),
                "--text-layout",
                path(&layout),
                "--format",
                "svg",
                "-o",
                path(&svg),
                "--manifest",
                path(&manifest),
            ],
            &fonts,
        ));
        let rendered = fs::read_to_string(&svg).unwrap();
        assert!(rendered.contains("<path"));
        assert!(!rendered.contains("<text"));
        assert_eq!(read(&manifest)["compilation_context"], compiled["context"]);
        let explanation = success(run(
            &[
                "explain",
                path(&saved),
                "--text-layout",
                path(&layout),
                "--node",
                "chart/title",
            ],
            &fonts,
        ));
        assert!(explanation.contains("chart/title"), "{explanation}");
        assert!(explanation.contains(TITLE), "{explanation}");
    }
}

#[test]
fn replay_requires_equal_policy_and_changes_require_original_hir_before_font_reads() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "chart.scatter", Some("Chart title"));
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.json");
    success(run(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    let invalid_font = vec![format!(
        "{}={}",
        "0".repeat(64),
        t.path().join("never-open.ttf").display()
    )];
    for (field, value) in [
        ("max_width", json!(240.0)),
        ("line_height", json!(34.0)),
        ("max_lines", json!(11)),
        ("view_id", json!("other")),
    ] {
        let mut changed = policy();
        changed["semantic_targets"][0][field] = value;
        write(&layout, &changed);
        rejected(
            run(
                &["normalize", path(&saved), "--text-layout", path(&layout)],
                &invalid_font,
            ),
            "original HIR",
        );
    }
    let mut changed = policy();
    changed["semantic_targets"][0]["max_width"] = 240.0.into();
    write(&layout, &changed);
    let fresh = success(run(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ],
        &fonts,
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&fresh).unwrap()["context"]["text_layout"],
        changed
    );
    success(run(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    rejected(
        run(
            &["validate", path(&saved), "--text-layout", path(&layout)],
            &invalid_font,
        ),
        "original HIR",
    );
}

#[test]
fn missing_title_rejects_and_empty_title_is_a_valid_logical_block() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "chart.line", None);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    rejected(
        run(
            &[
                "validate",
                path(&input),
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
            ],
            &fonts,
        ),
        "VIZ-TEXT-0007",
    );
    source(t.path(), "chart.line", Some(""));
    success(run(
        &[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ],
        &fonts,
    ));
}

#[test]
fn invalid_chart_title_preserves_prior_render_artifacts_and_policy() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "chart.line", Some("Chart title"));
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let policy_before = fs::read(&layout).unwrap();
    let (profile, fonts) = measured(t.path());
    let svg = t.path().join("title.svg");
    let manifest = t.path().join("title.render.json");
    let args = [
        "render",
        path(&input),
        "--text-profile",
        path(&profile),
        "--text-layout",
        path(&layout),
        "--format",
        "svg",
        "-o",
        path(&svg),
        "--manifest",
        path(&manifest),
    ];
    success(run(&args, &fonts));
    let svg_before = fs::read(&svg).unwrap();
    let manifest_before = fs::read(&manifest).unwrap();
    let entries = || {
        fs::read_dir(t.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let entries_before = entries();
    source(t.path(), "chart.line", Some("中文 ab\u{00ad}cd"));
    rejected(run(&args, &fonts), "soft hyphen");
    assert_eq!(fs::read(&svg).unwrap(), svg_before);
    assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
    assert_eq!(fs::read(&layout).unwrap(), policy_before);
    assert_eq!(entries(), entries_before);
}
