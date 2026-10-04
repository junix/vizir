use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vizir_compiler::{
    DiagramTextLayoutTarget, SemanticTextLayoutTarget, TEXT_LAYOUT_CATEGORY_PROFILE,
    TEXT_LAYOUT_DIAGRAM_PROFILE, TEXT_LAYOUT_ENGINE, TEXT_LAYOUT_PROFILE,
    TEXT_LAYOUT_SEMANTIC_PROFILE, TextLayoutContext, TextLayoutTarget,
    parse_text_layout_context_json,
};

const LABEL: &str = "office e\u{301}\n中文测试";

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
    json!({"profile":TEXT_LAYOUT_DIAGRAM_PROFILE,"engine":TEXT_LAYOUT_ENGINE,"targets":[],
        "diagram_targets":[{"view_id":"diagram","node_id":"first","max_width":120.0,"max_lines":3,"line_height":20.0}]})
}
fn source(dir: &Path, label: &str) -> PathBuf {
    let p = dir.join("source.json");
    write(
        &p,
        &json!({"version":"0.2","id":"diagram-wrapping-cli","width":640,"height":360,
        "views":[{"kind":"diagram.graph","id":"diagram","title":"Diagram title",
            "frame":{"x":0,"y":0,"width":640,"height":360},"layout":"manual",
            "nodes":[{"id":"first","label":label,"position":{"x":160,"y":160}},
                {"id":"second","label":"Second node","position":{"x":460,"y":160}}],
            "edges":[{"from":"first","to":"second","label":"connects"}]}]}),
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
fn prior_policies() -> Vec<TextLayoutContext> {
    let geometry = TextLayoutContext::new(vec![TextLayoutTarget::new(
        "labels", "body", 140.0, 12, 32.0,
    )]);
    let title =
        geometry
            .clone()
            .with_semantic_targets(vec![SemanticTextLayoutTarget::chart_title(
                "chart", 210.0, 12, 32.0,
            )]);
    let category =
        geometry
            .clone()
            .with_category_labels(vec![SemanticTextLayoutTarget::bar_category_labels(
                "chart", 90.0, 12, 16.0,
            )]);
    vec![geometry, title, category]
}

#[test]
fn public_diagram_api_is_explicit_and_preserves_all_prior_profile_bytes() {
    let target = DiagramTextLayoutTarget::new("diagram", "first", 120.0, 3, 20.0);
    assert_eq!(
        serde_json::to_value(&target).unwrap(),
        policy()["diagram_targets"][0]
    );
    let v4 = TextLayoutContext::new(vec![]).with_diagram_targets(vec![target.clone()]);
    assert_eq!(serde_json::to_value(&v4).unwrap(), policy());
    v4.validate().unwrap();
    assert_eq!(parse(&policy()).unwrap(), v4);
    assert_eq!(
        serde_json::from_value::<TextLayoutContext>(policy()).unwrap(),
        v4
    );
    for original in prior_policies() {
        let before = serde_json::to_string(&original).unwrap();
        let semantic = match original.profile.as_str() {
            TEXT_LAYOUT_SEMANTIC_PROFILE => {
                ",\"semantic_targets\":[{\"view_id\":\"chart\",\"role\":\"chart.title\",\"max_width\":210.0,\"max_lines\":12,\"line_height\":32.0}]"
            }
            TEXT_LAYOUT_CATEGORY_PROFILE => {
                ",\"semantic_targets\":[{\"view_id\":\"chart\",\"role\":\"bar.category_labels\",\"max_width\":90.0,\"max_lines\":12,\"line_height\":16.0}]"
            }
            _ => "",
        };
        assert_eq!(
            before,
            format!(
                "{{\"profile\":\"{}\",\"engine\":\"{TEXT_LAYOUT_ENGINE}\",\"targets\":[{{\"view_id\":\"labels\",\"node_id\":\"body\",\"max_width\":140.0,\"max_lines\":12,\"line_height\":32.0}}]{semantic}}}",
                original.profile
            )
        );
        let upgraded = original.clone().with_diagram_targets(vec![target.clone()]);
        assert_eq!(upgraded.profile, TEXT_LAYOUT_DIAGRAM_PROFILE);
        assert_eq!(upgraded.targets, original.targets);
        assert_eq!(upgraded.semantic_targets, original.semantic_targets);
        upgraded.validate().unwrap();
        let mut downgraded = upgraded;
        downgraded.diagram_targets = None;
        downgraded.profile = original.profile.clone();
        assert_eq!(serde_json::to_string(&downgraded).unwrap(), before);
    }
    assert!(
        TextLayoutContext::new(vec![])
            .with_diagram_targets(vec![])
            .validate()
            .is_err()
    );
    for downgraded in [
        v4.clone()
            .with_semantic_targets(vec![SemanticTextLayoutTarget::chart_title(
                "chart", 120.0, 3, 16.0,
            )]),
        v4.with_category_labels(vec![SemanticTextLayoutTarget::bar_category_labels(
            "chart", 120.0, 3, 16.0,
        )]),
    ] {
        assert!(downgraded.validate().is_err());
        assert!(
            serde_json::from_value::<TextLayoutContext>(serde_json::to_value(downgraded).unwrap())
                .is_err()
        );
    }
}

#[test]
fn diagram_presence_is_rejected_by_every_prior_profile_in_both_readers_and_validation() {
    for original in prior_policies() {
        for value in [json!([]), Value::Null, policy()["diagram_targets"].clone()] {
            let mut invalid = serde_json::to_value(&original).unwrap();
            invalid["diagram_targets"] = value;
            assert!(parse(&invalid).is_err(), "strict reader accepted {invalid}");
            assert!(
                serde_json::from_value::<TextLayoutContext>(invalid.clone()).is_err(),
                "serde accepted {invalid}"
            );
        }
        for targets in [
            vec![],
            vec![DiagramTextLayoutTarget::new(
                "diagram", "first", 120.0, 3, 20.0,
            )],
        ] {
            let mut invalid = original.clone();
            invalid.diagram_targets = Some(targets);
            assert!(invalid.validate().is_err());
        }
    }
    for field in ["targets", "diagram_targets"] {
        let mut missing = policy();
        missing.as_object_mut().unwrap().remove(field);
        assert!(parse(&missing).is_err());
        assert!(serde_json::from_value::<TextLayoutContext>(missing).is_err());
        let mut null = policy();
        null[field] = Value::Null;
        assert!(parse(&null).is_err());
        assert!(serde_json::from_value::<TextLayoutContext>(null).is_err());
    }
    let mut empty = policy();
    empty["diagram_targets"] = json!([]);
    assert!(parse(&empty).is_err());
    assert!(serde_json::from_value::<TextLayoutContext>(empty).is_err());
    let mut optional_semantic = policy();
    optional_semantic["semantic_targets"] = json!([]);
    parse(&optional_semantic).unwrap();
    optional_semantic["semantic_targets"] = Value::Null;
    assert!(parse(&optional_semantic).is_err());
}

#[test]
fn diagram_policy_checks_numbers_ids_fields_duplicates_and_the_shared_target_budget() {
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
        ("max_lines", json!("3")),
        ("view_id", json!("")),
        ("view_id", json!("a".repeat(257))),
        ("view_id", json!("中".repeat(86))),
        ("node_id", json!("")),
        ("node_id", json!("a".repeat(257))),
        ("node_id", json!("中".repeat(86))),
        ("role", json!("diagram.node")),
        ("path", json!("never-open.otf")),
    ] {
        let mut invalid = policy();
        invalid["diagram_targets"][0][field] = value;
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
            "\"node_id\":\"first\"",
            "\"node_id\":\"first\",\"node_id\":\"first\"",
        ),
        ("\"targets\":[]", "\"targets\":[],\"targets\":[]"),
        ("\"max_lines\":3", "\"max_lines\":3,\"max_lines\":3"),
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
    for spelling in ["3.0", "3e0", "0.3e1"] {
        let raw = encoded.replace("\"max_lines\":3", &format!("\"max_lines\":{spelling}"));
        assert_eq!(
            parse_text_layout_context_json(raw.as_bytes()).unwrap(),
            parse(&policy()).unwrap()
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut typed = TextLayoutContext::new(vec![]).with_diagram_targets(vec![
            DiagramTextLayoutTarget::new("diagram", "first", value, 3, 20.0),
        ]);
        assert!(typed.validate().is_err());
        let target = &mut typed.diagram_targets.as_mut().unwrap()[0];
        target.max_width = 120.0;
        target.line_height = value;
        assert!(typed.validate().is_err());
    }
    let mut duplicate = policy();
    let mut repeated = duplicate["diagram_targets"][0].clone();
    repeated["max_width"] = 121.into();
    duplicate["diagram_targets"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert!(
        parse(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let mut combined =
        prior_policies()
            .pop()
            .unwrap()
            .with_diagram_targets(vec![DiagramTextLayoutTarget::new(
                "diagram", "first", 120.0, 3, 20.0,
            )]);
    combined
        .semantic_targets
        .as_mut()
        .unwrap()
        .push(SemanticTextLayoutTarget::chart_title(
            "chart", 120.0, 3, 16.0,
        ));
    for index in 0..252 {
        combined.targets.push(TextLayoutTarget::new(
            "labels",
            format!("body-{index}"),
            120.0,
            3,
            16.0,
        ));
    }
    combined.validate().unwrap();
    assert_eq!(
        parse(&serde_json::to_value(&combined).unwrap()).unwrap(),
        combined
    );
    combined
        .diagram_targets
        .as_mut()
        .unwrap()
        .push(DiagramTextLayoutTarget::new(
            "diagram", "second", 120.0, 3, 20.0,
        ));
    assert!(combined.validate().unwrap_err().to_string().contains("256"));
}

#[test]
fn compiled_envelope_retains_diagram_strictness_and_legacy_profile_fences() {
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
    for original in prior_policies() {
        for field in [json!([]), Value::Null, policy()["diagram_targets"].clone()] {
            compiled["context"]["text_layout"] = serde_json::to_value(&original).unwrap();
            compiled["context"]["text_layout"]["diagram_targets"] = field;
            assert!(
                vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
                    .is_err()
            );
            assert!(
                serde_json::from_value::<vizir_compiler::CompiledMir>(compiled.clone()).is_err()
            );
        }
    }
    compiled["context"]["text_layout"] = policy();
    compiled["context"]["text_layout"]["diagram_targets"][0]["discarded"] = true.into();
    assert!(
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
    compiled["context"]["text_layout"] = policy();
    let raw = serde_json::to_string(&compiled).unwrap().replace(
        "\"node_id\":\"first\"",
        "\"node_id\":\"first\",\"node_id\":\"first\"",
    );
    assert!(
        vizir_compiler::parse_compiled_mir_json(raw.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn schema_preserves_the_complete_prior_definition_closures_and_adds_a_closed_v4_branch() {
    let schema = vizir_compiler::compiled_mir_schema();
    assert_eq!(
        schema,
        serde_json::from_str::<Value>(include_str!("../../../schemas/compiled-mir.schema.json"))
            .unwrap()
    );
    let published: Value = serde_json::from_str(r##"{"GeometryTextLayoutContext":{"additionalProperties":false,"properties":{"engine":{"const":"unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)","type":"string"},"profile":{"const":"vizir-text-wrap/1","type":"string"},"targets":{"items":{"$ref":"#/$defs/TextLayoutTarget"},"maxItems":256,"minItems":1,"type":"array","uniqueItems":true}},"required":["profile","engine","targets"],"type":"object"},"SemanticTextLayoutContext":{"additionalProperties":false,"properties":{"engine":{"const":"unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)","type":"string"},"profile":{"const":"vizir-text-wrap/2","type":"string"},"semantic_targets":{"items":{"$ref":"#/$defs/SemanticTextLayoutTarget"},"maxItems":256,"minItems":1,"type":"array","uniqueItems":true},"targets":{"items":{"$ref":"#/$defs/TextLayoutTarget"},"maxItems":256,"minItems":0,"type":"array","uniqueItems":true}},"required":["profile","engine","targets","semantic_targets"],"type":"object"},"CategoryTextLayoutContext":{"additionalProperties":false,"properties":{"engine":{"const":"unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)","type":"string"},"profile":{"const":"vizir-text-wrap/3","type":"string"},"semantic_targets":{"contains":{"additionalProperties":true,"properties":{"role":{"const":"bar.category_labels"}},"required":["role"],"type":"object"},"items":{"$ref":"#/$defs/CategorySemanticTextLayoutTarget"},"maxItems":256,"minItems":1,"type":"array","uniqueItems":true},"targets":{"items":{"$ref":"#/$defs/TextLayoutTarget"},"maxItems":256,"minItems":0,"type":"array","uniqueItems":true}},"required":["profile","engine","targets","semantic_targets"],"type":"object"},"TextLayoutTarget":{"additionalProperties":false,"properties":{"line_height":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"max_lines":{"format":"uint32","maximum":256,"minimum":1,"type":"integer"},"max_width":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"node_id":{"maxLength":256,"minLength":1,"type":"string"},"view_id":{"maxLength":256,"minLength":1,"type":"string"}},"required":["view_id","node_id","max_width","max_lines","line_height"],"type":"object"},"SemanticTextLayoutTarget":{"additionalProperties":false,"properties":{"line_height":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"max_lines":{"format":"uint32","maximum":256,"minimum":1,"type":"integer"},"max_width":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"role":{"$ref":"#/$defs/TextLayoutRole"},"view_id":{"maxLength":256,"minLength":1,"type":"string"}},"required":["view_id","role","max_width","max_lines","line_height"],"type":"object"},"CategorySemanticTextLayoutTarget":{"additionalProperties":false,"properties":{"line_height":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"max_lines":{"format":"uint32","maximum":256,"minimum":1,"type":"integer"},"max_width":{"format":"double","maximum":1000000,"minimum":0.25,"type":"number"},"role":{"$ref":"#/$defs/CategoryTextLayoutRole"},"view_id":{"maxLength":256,"minLength":1,"type":"string"}},"required":["view_id","role","max_width","max_lines","line_height"],"type":"object"},"TextLayoutRole":{"description":"A supported semantic text slot, resolved against original source fields.","enum":["chart.title"],"type":"string"},"CategoryTextLayoutRole":{"description":"A supported semantic text slot, resolved against original source fields.","enum":["chart.title","bar.category_labels"],"type":"string"}}"##).unwrap();
    for (name, definition) in published.as_object().unwrap() {
        assert_eq!(
            &schema["$defs"][name], definition,
            "changed published definition {name}"
        );
    }
    for (name, profile) in [
        ("GeometryTextLayoutContext", TEXT_LAYOUT_PROFILE),
        ("SemanticTextLayoutContext", TEXT_LAYOUT_SEMANTIC_PROFILE),
        ("CategoryTextLayoutContext", TEXT_LAYOUT_CATEGORY_PROFILE),
    ] {
        let branch = &schema["$defs"][name];
        assert_eq!(branch["properties"]["profile"]["const"], profile);
        assert_eq!(branch["additionalProperties"], false);
        assert!(branch["properties"].get("diagram_targets").is_none());
    }
    let v4 = &schema["$defs"]["DiagramTextLayoutContext"];
    assert_eq!(v4["additionalProperties"], false);
    assert_eq!(
        v4["properties"]["profile"]["const"],
        TEXT_LAYOUT_DIAGRAM_PROFILE
    );
    assert_eq!(
        v4["required"],
        json!(["profile", "engine", "targets", "diagram_targets"])
    );
    assert_eq!(v4["properties"]["diagram_targets"]["minItems"], 1);
    assert_eq!(v4["properties"]["diagram_targets"]["maxItems"], 256);
    assert_eq!(v4["properties"]["diagram_targets"]["type"], "array");
    assert_eq!(
        v4["properties"]["diagram_targets"]["items"]["$ref"],
        "#/$defs/DiagramTextLayoutTarget"
    );
    assert_eq!(v4["properties"]["semantic_targets"]["type"], "array");
    assert_eq!(v4["properties"]["semantic_targets"]["minItems"], 0);
    assert_eq!(
        v4["properties"]["semantic_targets"]["items"]["$ref"],
        "#/$defs/CategorySemanticTextLayoutTarget"
    );
    assert!(
        v4["properties"]["semantic_targets"]
            .get("contains")
            .is_none()
    );
    let mut diagram_target = schema["$defs"]["DiagramTextLayoutTarget"].clone();
    diagram_target
        .as_object_mut()
        .unwrap()
        .remove("description");
    assert_eq!(diagram_target, schema["$defs"]["TextLayoutTarget"]);
    assert_eq!(
        schema["$defs"]["TextLayoutContext"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(serde_json::to_vec(&schema).unwrap().len() < 100_000);
}

#[test]
fn diagram_labels_roundtrip_all_five_cli_workflows_and_replay_without_the_policy_file() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), LABEL);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.mir.json");
    let refreshed = t.path().join("refreshed.json");
    let svg = t.path().join("diagram.svg");
    let manifest = t.path().join("diagram.render.json");
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
    assert_eq!(compiled["mir"]["views"][0]["nodes"][0]["label"], LABEL);
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
    assert_eq!(
        direct,
        success(run(
            &["lower", path(&saved), "--text-layout", path(&layout)],
            &fonts
        ))
    );
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
            "diagram/node/first/label",
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
            "diagram/node/first/label",
        ],
        &fonts,
    ));
    assert_eq!(direct_explanation, replay_explanation);
    assert!(replay_explanation.contains(LABEL), "{replay_explanation}");
    fs::remove_file(&layout).unwrap();
    assert_eq!(success(run(&["lower", path(&saved)], &fonts)), direct);
    success(run(&["validate", path(&saved)], &fonts));
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
    assert_eq!(
        success(run(
            &[
                "explain",
                path(&saved),
                "--node",
                "diagram/node/first/label"
            ],
            &fonts
        )),
        direct_explanation
    );
}

#[test]
fn diagram_policy_changes_are_source_only_in_every_workflow_before_font_reads() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "First node");
    let layout = t.path().join("layout.json");
    let mut combined = policy();
    let mut second = combined["diagram_targets"][0].clone();
    second["node_id"] = "second".into();
    combined["diagram_targets"]
        .as_array_mut()
        .unwrap()
        .push(second);
    write(&layout, &combined);
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.mir.json");
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
        ("max_width", json!(121)),
        ("line_height", json!(21)),
        ("max_lines", json!(2)),
        ("view_id", json!("other")),
        ("node_id", json!("other")),
    ] {
        let mut changed = combined.clone();
        changed["diagram_targets"][0][field] = value;
        changes.push(changed);
    }
    let mut reordered = combined.clone();
    reordered["diagram_targets"]
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
                args.extend(["--node", "diagram/node/first/label"]);
            }
            rejected(run(&args, &invalid_font), "original HIR");
        }
    }
    assert!(!render_output.exists());
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
    for mode in ["themed", "measured"] {
        let mut args = vec!["normalize", path(&input), "-o", path(&saved)];
        if mode == "themed" {
            args.extend(["--theme", "azure"]);
        }
        if mode == "measured" {
            args.extend(["--text-profile", path(&profile)]);
        }
        success(run(&args, if mode == "measured" { &fonts } else { &[] }));
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
fn target_errors_and_late_overflow_preserve_output_manifest_policy_and_directory() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), LABEL);
    let layout = t.path().join("layout.json");
    write(&layout, &policy());
    let (profile, fonts) = measured(t.path());
    let svg = t.path().join("diagram.svg");
    let manifest = t.path().join("diagram.render.json");
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
    let assert_unchanged = || {
        assert_eq!(fs::read(&svg).unwrap(), svg_before);
        assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
        assert_eq!(entries(), entries_before);
    };
    source(t.path(), "First node");
    for (field, value) in [
        ("view_id", "missing"),
        ("node_id", "missing"),
        ("node_id", "node/first/label"),
        ("node_id", "edge/0-first-second/label"),
        ("node_id", "title"),
    ] {
        let mut invalid = policy();
        invalid["diagram_targets"][0][field] = value.into();
        write(&layout, &invalid);
        let policy_before = fs::read(&layout).unwrap();
        rejected(run(&args, &fonts), "VIZ-TEXT-0007");
        assert_eq!(fs::read(&layout).unwrap(), policy_before);
        assert_unchanged();
    }
    write(&layout, &policy());
    let policy_before = fs::read(&layout).unwrap();
    for label in [
        "中文 ab\u{00ad}cd",
        "AnUnbreakableDiagramLabelThatCannotFit",
        "A\nA\nA\nA",
        "A\tB",
    ] {
        source(t.path(), label);
        rejected(run(&args, &fonts), "VIZ-TEXT-0007");
        assert_eq!(fs::read(&layout).unwrap(), policy_before);
        assert_unchanged();
    }
    source(t.path(), "A\nB\nC");
    let mut tall = policy();
    tall["diagram_targets"][0]["line_height"] = 24.into();
    write(&layout, &tall);
    rejected(run(&args, &fonts), "VIZ-TEXT-0007");
    assert_unchanged();
}

#[test]
fn diagram_newlines_are_scoped_to_the_exact_node_and_need_measured_context() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), LABEL);
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
    let original = read(&input);
    for field in ["title", "edge", "other-node"] {
        let mut changed = original.clone();
        match field {
            "title" => changed["views"][0]["title"] = "Diagram\ntitle".into(),
            "edge" => changed["views"][0]["edges"][0]["label"] = "Edge\nlabel".into(),
            _ => changed["views"][0]["nodes"][1]["label"] = "Other\nnode".into(),
        }
        write(&input, &changed);
        assert!(
            !run(&args, &fonts).status.success(),
            "accepted unselected {field}"
        );
    }
    let mut changed = original;
    let mut other = changed["views"][0].clone();
    other["id"] = "unrelated".into();
    other["frame"]["y"] = 360.into();
    changed["height"] = 720.into();
    changed["views"].as_array_mut().unwrap().push(other);
    write(&input, &changed);
    assert!(!run(&args, &fonts).status.success());
}

#[test]
fn one_v4_policy_combines_diagram_geometry_titles_and_categories_without_reflow_on_replay() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), LABEL);
    let mut document = read(&input);
    document["width"] = 720.into();
    document["height"] = 1160.into();
    document["datasets"] = json!({"data":{"key":"id","rows":[
        {"id":"a","category":"First\ncategory","value":2},
        {"id":"b","category":"中文测试","value":3}]}});
    document["views"].as_array_mut().unwrap().extend([
        json!({"kind":"chart.bar","id":"chart","dataset":"data","title":"Chart title\nSecond line",
            "frame":{"x":0,"y":360,"width":720,"height":640},
            "category":{"field":"category"},"value":{"field":"value"}}),
        json!({"kind":"geometry.scene","id":"labels","frame":{"x":0,"y":1000,"width":720,"height":160},
            "children":[{"type":"text","id":"body","x":20,"y":40,"font_size":16,"anchor":"start","text":"Geometry label\n中文"}]}),
    ]);
    write(&input, &document);
    let layout = t.path().join("layout.json");
    let combined = TextLayoutContext::new(vec![TextLayoutTarget::new(
        "labels", "body", 140.0, 3, 32.0,
    )])
    .with_category_labels(vec![
        SemanticTextLayoutTarget::chart_title("chart", 210.0, 3, 32.0),
        SemanticTextLayoutTarget::bar_category_labels("chart", 90.0, 3, 16.0),
    ])
    .with_diagram_targets(vec![DiagramTextLayoutTarget::new(
        "diagram", "first", 120.0, 3, 20.0,
    )]);
    write(&layout, &serde_json::to_value(combined).unwrap());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.mir.json");
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
    assert_eq!(read(&saved)["context"]["text_layout"], read(&layout));
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
    // V4 can also carry title-only semantic targets; it does not inherit v3's
    // category-presence requirement just because both roles are permitted.
    let mut title_only = read(&layout);
    title_only["semantic_targets"].as_array_mut().unwrap().pop();
    document["datasets"]["data"]["rows"][0]["category"] = "First category".into();
    write(&input, &document);
    write(&layout, &title_only);
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
