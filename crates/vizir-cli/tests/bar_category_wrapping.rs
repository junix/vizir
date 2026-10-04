use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vizir_compiler::{
    SemanticTextLayoutTarget, TEXT_LAYOUT_CATEGORY_PROFILE, TEXT_LAYOUT_ENGINE,
    TEXT_LAYOUT_PROFILE, TEXT_LAYOUT_SEMANTIC_PROFILE, TextLayoutContext, TextLayoutRole,
    TextLayoutTarget, parse_text_layout_context_json,
};

const CATEGORY: &str = "Measured category labels wrap.\n\n中文测试\n";

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
    json!({"profile":TEXT_LAYOUT_CATEGORY_PROFILE,"engine":TEXT_LAYOUT_ENGINE,"targets":[],
        "semantic_targets":[{"view_id":"chart","role":"bar.category_labels","max_width":90.0,"max_lines":12,"line_height":16.0}]})
}
fn geometry_policy() -> Value {
    json!({"profile":TEXT_LAYOUT_PROFILE,"engine":TEXT_LAYOUT_ENGINE,"targets":[
        {"view_id":"labels","node_id":"body","max_width":140.0,"max_lines":12,"line_height":32.0}]})
}
fn source(dir: &Path, categories: &[&str]) -> PathBuf {
    let rows: Vec<Value> = categories
        .iter()
        .enumerate()
        .map(|(i, category)| json!({"id":format!("row-{i}"),"category":category,"value":i + 2}))
        .collect();
    let p = dir.join("source.json");
    write(
        &p,
        &json!({"version":"0.2","id":"category-wrapping-cli","width":720,"height":640,
        "datasets":{"data":{"key":"id","rows":rows}},"views":[{
            "kind":"chart.bar","id":"chart","dataset":"data","title":"Chart title",
            "frame":{"x":0,"y":0,"width":720,"height":640},
            "category":{"field":"category","label":"Category"},
            "value":{"field":"value","label":"Value"}}]}),
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
fn public_category_api_is_explicit_and_retains_exact_v1_v2_bytes() {
    let original = TextLayoutContext::new(vec![TextLayoutTarget::new(
        "labels", "body", 140.0, 12, 32.0,
    )]);
    let v1 = format!(
        "{{\"profile\":\"{TEXT_LAYOUT_PROFILE}\",\"engine\":\"{TEXT_LAYOUT_ENGINE}\",\"targets\":[{{\"view_id\":\"labels\",\"node_id\":\"body\",\"max_width\":140.0,\"max_lines\":12,\"line_height\":32.0}}]}}"
    );
    assert_eq!(serde_json::to_string(&original).unwrap(), v1);
    let title = SemanticTextLayoutTarget::chart_title("chart", 210.0, 12, 32.0);
    let v2 = original.clone().with_semantic_targets(vec![title.clone()]);
    assert_eq!(
        serde_json::to_string(&v2).unwrap(),
        format!(
            "{{\"profile\":\"{TEXT_LAYOUT_SEMANTIC_PROFILE}\",\"engine\":\"{TEXT_LAYOUT_ENGINE}\",\"targets\":[{{\"view_id\":\"labels\",\"node_id\":\"body\",\"max_width\":140.0,\"max_lines\":12,\"line_height\":32.0}}],\"semantic_targets\":[{{\"view_id\":\"chart\",\"role\":\"chart.title\",\"max_width\":210.0,\"max_lines\":12,\"line_height\":32.0}}]}}"
        )
    );
    let category = SemanticTextLayoutTarget::bar_category_labels("chart", 90.0, 12, 16.0);
    assert_eq!(category.role, TextLayoutRole::BarCategoryLabels);
    assert_eq!(
        serde_json::to_value(category.role).unwrap(),
        "bar.category_labels"
    );
    let v3 = original
        .clone()
        .with_category_labels(vec![title.clone(), category.clone()]);
    assert_eq!(v3.profile, TEXT_LAYOUT_CATEGORY_PROFILE);
    assert_eq!(v3.targets, original.targets);
    v3.validate().unwrap();
    assert_eq!(parse(&serde_json::to_value(&v3).unwrap()).unwrap(), v3);
    assert_eq!(
        serde_json::from_value::<TextLayoutContext>(serde_json::to_value(&v3).unwrap()).unwrap(),
        v3
    );
    // This older builder always selects v2, even when called on an existing v3 policy.
    let downgraded = v3.with_semantic_targets(vec![category]);
    assert_eq!(downgraded.profile, TEXT_LAYOUT_SEMANTIC_PROFILE);
    assert!(downgraded.validate().is_err());
    assert!(
        serde_json::from_value::<TextLayoutContext>(serde_json::to_value(downgraded).unwrap())
            .is_err()
    );
    assert!(
        original
            .clone()
            .with_category_labels(vec![title])
            .validate()
            .is_err()
    );
    assert!(original.with_category_labels(vec![]).validate().is_err());
}

#[test]
fn profile_shape_rejects_cross_version_roles_missing_null_and_empty_arrays() {
    parse(&policy()).unwrap();
    let mut invalid = Vec::new();
    for profile in [TEXT_LAYOUT_PROFILE, TEXT_LAYOUT_SEMANTIC_PROFILE] {
        let mut value = policy();
        value["profile"] = profile.into();
        invalid.push(value);
    }
    for field in ["targets", "semantic_targets"] {
        let mut missing = policy();
        missing.as_object_mut().unwrap().remove(field);
        invalid.push(missing);
        let mut null = policy();
        null[field] = Value::Null;
        invalid.push(null);
    }
    let mut empty = policy();
    empty["semantic_targets"] = json!([]);
    invalid.push(empty);
    let mut title_only = policy();
    title_only["semantic_targets"][0]["role"] = "chart.title".into();
    invalid.push(title_only);
    for value in [json!([]), Value::Null, policy()["semantic_targets"].clone()] {
        let mut v1 = geometry_policy();
        v1["semantic_targets"] = value;
        invalid.push(v1);
    }
    for value in invalid {
        assert!(parse(&value).is_err(), "strict parser accepted {value}");
        assert!(
            serde_json::from_value::<TextLayoutContext>(value.clone()).is_err(),
            "generic serde accepted {value}"
        );
    }
}

#[test]
fn category_policy_has_strict_numbers_fields_ids_and_duplicate_keys() {
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
        ("role", json!("bar.category")),
        ("role", json!("chart.category_labels")),
        ("role", json!("bar.category_labels/0")),
        ("role", Value::Null),
        ("node_id", json!("axis/x/category/0")),
        ("path", json!("never-open.otf")),
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
    let encoded = serde_json::to_string(&policy()).unwrap();
    for (old, replacement) in [
        (
            "\"role\":\"bar.category_labels\"",
            "\"role\":\"bar.category_labels\",\"role\":\"bar.category_labels\"",
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
        let mut typed = TextLayoutContext::new(vec![]).with_category_labels(vec![
            SemanticTextLayoutTarget::bar_category_labels("chart", value, 12, 16.0),
        ]);
        assert!(typed.validate().is_err());
        let target = &mut typed.semantic_targets.as_mut().unwrap()[0];
        target.max_width = 90.0;
        target.line_height = value;
        assert!(typed.validate().is_err());
    }
}

#[test]
fn category_title_and_geometry_share_the_combined_target_budget() {
    let mut mixed = policy();
    mixed["targets"] = geometry_policy()["targets"].clone();
    mixed["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "view_id":"chart","role":"chart.title","max_width":210.0,"max_lines":12,"line_height":32.0
        }));
    parse(&mixed).unwrap();
    let mut duplicate = mixed.clone();
    let mut repeated = duplicate["semantic_targets"][0].clone();
    repeated["max_width"] = 91.into();
    duplicate["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert!(
        parse(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
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
fn compiled_envelope_preserves_profile_specific_decoding_and_strict_fields() {
    let legacy: Value =
        serde_json::from_str(include_str!("fixtures/legacy-theme.mir.json")).unwrap();
    let text: Value = serde_json::from_str(include_str!(
        "../../../examples/text/wrapping-font-profile.json"
    ))
    .unwrap();
    let mut compiled = json!({"format":vizir_compiler::COMPILED_MIR_FORMAT,
        "context":{"text":text,"text_layout":policy()},"mir":legacy});
    let parsed =
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(parsed).unwrap()["context"]["text_layout"],
        policy()
    );
    for profile in [TEXT_LAYOUT_PROFILE, TEXT_LAYOUT_SEMANTIC_PROFILE] {
        compiled["context"]["text_layout"]["profile"] = profile.into();
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
        assert!(serde_json::from_value::<vizir_compiler::CompiledMir>(compiled.clone()).is_err());
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
        "\"role\":\"bar.category_labels\"",
        "\"role\":\"bar.category_labels\",\"role\":\"bar.category_labels\"",
    );
    assert!(
        vizir_compiler::parse_compiled_mir_json(raw.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn schema_preserves_published_branches_and_adds_one_closed_v3_branch() {
    let schema = vizir_compiler::compiled_mir_schema();
    assert_eq!(
        schema,
        serde_json::from_str::<Value>(include_str!("../../../schemas/compiled-mir.schema.json"))
            .unwrap()
    );
    // Exact pre-v3 published definitions, including the title-only role reference.
    let published: Value = serde_json::from_str(r##"{"GeometryTextLayoutContext":{"additionalProperties":false,"properties":{"engine":{"const":"unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)","type":"string"},"profile":{"const":"vizir-text-wrap/1","type":"string"},"targets":{"items":{"$ref":"#/$defs/TextLayoutTarget"},"maxItems":256,"minItems":1,"type":"array","uniqueItems":true}},"required":["profile","engine","targets"],"type":"object"},"SemanticTextLayoutContext":{"additionalProperties":false,"properties":{"engine":{"const":"unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)","type":"string"},"profile":{"const":"vizir-text-wrap/2","type":"string"},"semantic_targets":{"items":{"$ref":"#/$defs/SemanticTextLayoutTarget"},"maxItems":256,"minItems":1,"type":"array","uniqueItems":true},"targets":{"items":{"$ref":"#/$defs/TextLayoutTarget"},"maxItems":256,"minItems":0,"type":"array","uniqueItems":true}},"required":["profile","engine","targets","semantic_targets"],"type":"object"},"TextLayoutTarget":{"additionalProperties":false,"properties":{"line_height":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"max_lines":{"format":"uint32","maximum":256,"minimum":1,"type":"integer"},"max_width":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"node_id":{"maxLength":256,"minLength":1,"type":"string"},"view_id":{"maxLength":256,"minLength":1,"type":"string"}},"required":["view_id","node_id","max_width","max_lines","line_height"],"type":"object"},"SemanticTextLayoutTarget":{"additionalProperties":false,"properties":{"line_height":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"max_lines":{"format":"uint32","maximum":256,"minimum":1,"type":"integer"},"max_width":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"role":{"$ref":"#/$defs/TextLayoutRole"},"view_id":{"maxLength":256,"minLength":1,"type":"string"}},"required":["view_id","role","max_width","max_lines","line_height"],"type":"object"},"TextLayoutRole":{"description":"A supported semantic text slot, resolved against original source fields.","enum":["chart.title"],"type":"string"}}"##).unwrap();
    for (name, definition) in published.as_object().unwrap() {
        assert_eq!(
            &schema["$defs"][name], definition,
            "changed published definition {name}"
        );
    }
    let v3 = &schema["$defs"]["CategoryTextLayoutContext"];
    assert_eq!(v3["additionalProperties"], false);
    assert_eq!(
        v3["properties"]["profile"]["const"],
        TEXT_LAYOUT_CATEGORY_PROFILE
    );
    assert_eq!(v3["properties"]["targets"]["minItems"], 0);
    assert_eq!(v3["properties"]["targets"]["maxItems"], 256);
    let targets = &v3["properties"]["semantic_targets"];
    assert_eq!(targets["type"], "array");
    assert_eq!(targets["minItems"], 1);
    assert_eq!(targets["maxItems"], 256);
    assert_eq!(
        targets["contains"]["properties"]["role"]["const"],
        "bar.category_labels"
    );
    assert_eq!(targets["contains"]["additionalProperties"], true);
    assert!(
        v3["required"]
            .as_array()
            .unwrap()
            .contains(&json!("semantic_targets"))
    );
    assert_eq!(
        targets["items"]["$ref"],
        "#/$defs/CategorySemanticTextLayoutTarget"
    );
    assert_eq!(
        schema["$defs"]["CategorySemanticTextLayoutTarget"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["$defs"]["CategoryTextLayoutRole"]["enum"],
        json!(["chart.title", "bar.category_labels"])
    );
    assert_eq!(
        schema["$defs"]["TextLayoutContext"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(serde_json::to_vec(&schema).unwrap().len() < 100_000);
}

#[test]
fn category_labels_roundtrip_all_five_cli_workflows_and_replay_without_the_policy_file() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), &[CATEGORY, "Second category", "office e\u{301}"]);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.json");
    let refreshed = t.path().join("refreshed.json");
    let svg = t.path().join("categories.svg");
    let manifest = t.path().join("categories.render.json");
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
    assert_eq!(
        compiled["mir"]["data"]["data/data"]["operator"]["rows"],
        read(&input)["datasets"]["data"]["rows"]
    );
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
    assert_eq!(direct, replay);
    success(run(
        &["validate", path(&saved), "--text-layout", path(&layout)],
        &fonts,
    ));
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
            path(&input),
            "--theme",
            "azure",
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
        ],
        &fonts,
    ));
    let rendered = fs::read_to_string(&svg).unwrap();
    assert!(rendered.contains("<path"));
    assert!(!rendered.contains("<text"));
    assert_eq!(read(&manifest)["compilation_context"], compiled["context"]);
    let direct_explanation = success(run(
        &[
            "explain",
            path(&input),
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
            "--node",
            "chart/axis/x/category/0",
        ],
        &fonts,
    ));
    let replay_explanation = success(run(
        &[
            "explain",
            path(&saved),
            "--text-layout",
            path(&layout),
            "--node",
            "chart/axis/x/category/0",
        ],
        &fonts,
    ));
    assert_eq!(direct_explanation, replay_explanation);
    assert!(
        replay_explanation.contains(CATEGORY),
        "{replay_explanation}"
    );
    fs::remove_file(&layout).unwrap();
    assert_eq!(success(run(&["lower", path(&saved)], &fonts)), direct);
    success(run(
        &["render", path(&saved), "--format", "svg", "-o", path(&svg)],
        &fonts,
    ));
    assert_eq!(fs::read_to_string(&svg).unwrap(), rendered);
    success(run(
        &["normalize", path(&saved), "-o", path(&refreshed)],
        &fonts,
    ));
    assert_eq!(read(&refreshed), compiled);
}

#[test]
fn policy_changes_are_source_only_on_every_cli_workflow_and_precede_font_reads() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), &["First category", "Second category"]);
    let layout = t.path().join("layout.json");
    let mut combined = policy();
    combined["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "view_id":"chart","role":"chart.title","max_width":210.0,"max_lines":12,"line_height":32.0
        }));
    write(&layout, &combined);
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
        t.path().join("never-open.otf").display()
    )];
    let mut changes = Vec::new();
    for (field, value) in [
        ("max_width", json!(91)),
        ("line_height", json!(17)),
        ("max_lines", json!(11)),
        ("view_id", json!("other")),
    ] {
        let mut changed = combined.clone();
        changed["semantic_targets"][0][field] = value;
        changes.push(changed);
    }
    let mut reordered = combined.clone();
    reordered["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .reverse();
    changes.push(reordered);
    changes.push(policy());
    let render_output = t.path().join("should-not-publish.svg");
    for changed in changes {
        write(&layout, &changed);
        for command in ["validate", "normalize", "lower", "render", "explain"] {
            let mut args = vec![command, path(&saved), "--text-layout", path(&layout)];
            if command == "render" {
                args.extend(["--format", "svg", "-o", path(&render_output)]);
            }
            if command == "explain" {
                args.extend(["--node", "chart/axis/x/category/0"]);
            }
            rejected(run(&args, &invalid_font), "original HIR");
        }
    }
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
        policy()
    );
    // Source-only opt-in also applies to each pre-existing envelope kind.
    for mode in ["themed", "measured", "title"] {
        let mut args = vec!["normalize", path(&input), "-o", path(&saved)];
        if mode == "themed" {
            args.extend(["--theme", "azure"]);
        }
        if matches!(mode, "measured" | "title") {
            args.extend(["--text-profile", path(&profile)]);
        }
        if mode == "title" {
            let mut title_policy = combined.clone();
            title_policy["profile"] = TEXT_LAYOUT_SEMANTIC_PROFILE.into();
            title_policy["semantic_targets"]
                .as_array_mut()
                .unwrap()
                .remove(0);
            write(&layout, &title_policy);
            args.extend(["--text-layout", path(&layout)]);
        }
        success(run(
            &args,
            if matches!(mode, "measured" | "title") {
                &fonts
            } else {
                &[]
            },
        ));
        write(&layout, &policy());
        rejected(
            run(
                &["validate", path(&saved), "--text-layout", path(&layout)],
                &invalid_font,
            ),
            "original HIR",
        );
    }
}

#[test]
fn late_category_failure_preserves_existing_output_manifest_policy_and_directory() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), &["First category", "Second category"]);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let policy_before = fs::read(&layout).unwrap();
    let (profile, fonts) = measured(t.path());
    let svg = t.path().join("categories.svg");
    let manifest = t.path().join("categories.render.json");
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
    for category in [
        "中文 ab\u{00ad}cd",
        "AnUnbreakableCategoryThatCannotPossiblyFitTheExplicitWidth",
        "A\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA",
    ] {
        source(t.path(), &["First category", category]);
        rejected(run(&args, &fonts), "VIZ-TEXT-0007");
        assert_eq!(fs::read(&svg).unwrap(), svg_before);
        assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
        assert_eq!(fs::read(&layout).unwrap(), policy_before);
        assert_eq!(entries(), entries_before);
    }
}

#[test]
fn category_newlines_are_scoped_to_selected_use_and_need_measured_context() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), &["First\ncategory", "Second category"]);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    rejected(
        run(
            &["validate", path(&input), "--text-layout", path(&layout)],
            &[],
        ),
        "requires a measured",
    );
    let args = [
        "validate",
        path(&input),
        "--text-profile",
        path(&profile),
        "--text-layout",
        path(&layout),
    ];
    success(run(&args, &fonts));
    let mut same_field_legend = read(&input);
    same_field_legend["views"][0]["color"] = json!({"field":"category"});
    write(&input, &same_field_legend);
    assert!(!run(&args, &fonts).status.success());
    source(t.path(), &["First\ncategory", "Second category"]);
    let mut two_views = read(&input);
    two_views["width"] = 1440.into();
    let mut other = two_views["views"][0].clone();
    other["id"] = "unrelated".into();
    other["frame"]["x"] = 720.into();
    two_views["views"].as_array_mut().unwrap().push(other);
    write(&input, &two_views);
    assert!(!run(&args, &fonts).status.success());
}

#[test]
fn one_v3_policy_compiles_geometry_title_and_category_targets_together() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), &[CATEGORY, "Second category"]);
    let mut document = read(&input);
    document["height"] = 840.into();
    document["views"][0]["frame"]["y"] = 200.into();
    document["views"][0]["title"] = "Chart title\nSecond line".into();
    document["views"].as_array_mut().unwrap().push(json!({
        "kind":"geometry.scene","id":"labels","frame":{"x":0,"y":0,"width":720,"height":180},
        "children":[{"type":"text","id":"body","x":20,"y":40,"font_size":16,"anchor":"start","text":"Geometry label\n中文"}]
    }));
    write(&input, &document);
    let mut combined = policy();
    combined["targets"] = geometry_policy()["targets"].clone();
    combined["semantic_targets"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "view_id":"chart","role":"chart.title","max_width":210.0,"max_lines":12,"line_height":32.0
        }));
    let layout = t.path().join("layout.json");
    write(&layout, &combined);
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
    assert_eq!(read(&saved)["context"]["text_layout"], combined);
    let direct = success(run(
        &[
            "lower",
            path(&input),
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ],
        &fonts,
    ));
    assert_eq!(success(run(&["lower", path(&saved)], &fonts)), direct);
}
