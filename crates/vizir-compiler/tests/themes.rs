use serde_json::json;
use vizir_compiler::{
    MaterializationLimits, THEME_NAMES, THEME_REGISTRY_REVISION, THEMED_MIR_FORMAT, ThemeContext,
    ThemedMir, build_themed_scene, build_themed_scene_with_limits, compile, compile_with_theme,
    rematerialize_themed_mir,
};
use vizir_core::{
    ChartMark, Color, Document, MirDataOperator, MirScale, MirView, SceneNode, find_scene_node,
};

fn chart(kind: &str, palette: Option<Vec<&str>>) -> Document {
    let mut view = json!({"kind":kind,"id":"chart","dataset":"data", "title":"Values",
        "frame":{"x":0,"y":0,"width":640,"height":400},
        "x":{"field":"x"},"y":{"field":"y"}});
    if kind == "chart.bar" {
        view.as_object_mut().unwrap().remove("x");
        view.as_object_mut().unwrap().remove("y");
        view["category"] = json!({"field":"id"});
        view["value"] = json!({"field":"y"});
    }
    let encoding = if kind == "chart.line" {
        "series"
    } else {
        "color"
    };
    view[encoding] = json!({"field":"group"});
    if let Some(palette) = palette {
        view[encoding]["palette"] = json!(palette);
    }
    serde_json::from_value(
        json!({"version":"0.2","id":"themes","width":640,"height":400,
        "datasets":{"data":{"key":"id","rows":[
            {"id":"b","x":2.0,"y":4.0,"group":"G"},
            {"id":"a","x":1.0,"y":2.0,"group":"G"},
            {"id":"c","x":3.0,"y":3.0,"group":"H"}]}},"views":[view]}),
    )
    .unwrap()
}

fn diagram() -> Document {
    serde_json::from_value(json!({"version":"0.1","id":"diagram","width":800,"height":500,
        "background":"#FFFFFF", "views":[{"kind":"diagram.graph","id":"graph","title":"Graph",
        "frame":{"x":0,"y":0,"width":800,"height":350},"layout":"manual",
        "nodes":[{"id":"a","label":"A","position":{"x":120,"y":160},"style":{"fill":"#F7F9FC","stroke":"#B8C3D3"}},
        {"id":"b","label":"B","position":{"x":400,"y":160},"style":{"fill":"transparent"}},
        {"id":"c","label":"C","position":{"x":680,"y":160}}],
        "edges":[{"from":"a","to":"b","style":{"stroke":"#8793A5","opacity":0.4}},
        {"from":"b","to":"c"}]},
        {"kind":"geometry.scene","id":"geometry","frame":{"x":0,"y":350,"width":800,"height":150},
         "children":[{"type":"text","id":"default","x":0,"y":30,"text":"Default"},
         {"type":"text","id":"authored","x":0,"y":60,"text":"Authored","color":"#1C2736"}]}]})).unwrap()
}

#[test]
fn all_fourteen_names_match_pinned_registry_and_series_order() {
    let registry: serde_json::Value = serde_json::from_str(diagram_theme::REGISTRY_JSON).unwrap();
    assert_eq!(THEME_NAMES.len(), 14);
    assert!(
        include_str!("../Cargo.toml").contains(&format!("rev = \"{THEME_REGISTRY_REVISION}\""))
    );
    for name in THEME_NAMES {
        let context = ThemeContext::resolve(name).unwrap();
        context.validate().unwrap();
        let (family, mode) = name
            .strip_suffix("-dark")
            .map(|f| (f, "dark"))
            .unwrap_or((name, "light"));
        let tokens = &registry["themes"][family][mode];
        assert_eq!(context.registry_revision, THEME_REGISTRY_REVISION);
        for (i, color) in context.defaults.series.iter().enumerate() {
            assert_eq!(
                color.0,
                tokens[format!("--s{}", i + 1)]
                    .as_str()
                    .unwrap()
                    .to_ascii_lowercase()
            );
        }
        assert_eq!(
            context.defaults.ink.0,
            tokens["--ink"].as_str().unwrap().to_ascii_lowercase()
        );
        assert_eq!(
            context.defaults.point_interior.0,
            tokens["--paper"].as_str().unwrap().to_ascii_lowercase()
        );
        for kind in ["chart.scatter", "chart.line", "chart.bar"] {
            let compiled = compile_with_theme(&chart(kind, None), name).unwrap();
            assert_eq!(compiled.scene.background, Color::transparent());
            let MirView::Chart(c) = &compiled.mir.mir.views[0] else {
                panic!()
            };
            let range = c
                .scales
                .iter()
                .find_map(|scale| match scale {
                    MirScale::OrdinalColor { range, .. } => Some(range),
                    _ => None,
                })
                .unwrap();
            assert_eq!(range, &context.defaults.series[..2]);
        }
    }
    for bad in ["", "default", "dark", "Azure", "azure-light", "sage_dark"] {
        assert!(ThemeContext::resolve(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn author_colors_win_even_when_equal_to_legacy_defaults_or_transparent() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let compiled = compile_with_theme(
            &chart(kind, Some(vec!["#3B6EF5", "transparent"])),
            "sage-dark",
        )
        .unwrap();
        let MirView::Chart(c) = &compiled.mir.mir.views[0] else {
            panic!()
        };
        let range = c
            .scales
            .iter()
            .find_map(|scale| match scale {
                MirScale::OrdinalColor { range, .. } => Some(range),
                _ => None,
            })
            .unwrap();
        assert_eq!(range, &vec![Color::hex("#3B6EF5"), Color::transparent()]);
    }
    let compiled = compile_with_theme(&diagram(), "azure-dark").unwrap();
    let defaults = &compiled.mir.theme.defaults;
    assert_eq!(compiled.scene.background, Color::hex("#FFFFFF"));
    let get = |id| find_scene_node(&compiled.scene.nodes, id).unwrap();
    let SceneNode::Rect { style, .. } = get("graph/node/a/shape") else {
        panic!()
    };
    assert_eq!(style.fill, Color::hex("#F7F9FC"));
    assert_eq!(style.stroke, Color::hex("#B8C3D3"));
    let SceneNode::Rect { style, .. } = get("graph/node/b/shape") else {
        panic!()
    };
    assert_eq!(style.fill, Color::transparent());
    let SceneNode::Rect { style, .. } = get("graph/node/c/shape") else {
        panic!()
    };
    assert_eq!(style.fill, defaults.node_fill);
    let SceneNode::Text { color, .. } = get("geometry/default") else {
        panic!()
    };
    assert_eq!(color, &defaults.ink);
    let SceneNode::Text { color, .. } = get("geometry/authored") else {
        panic!()
    };
    assert_eq!(color, &Color::hex("#1C2736"));
    let SceneNode::Path {
        style,
        commands,
        marker_end,
        ..
    } = get("graph/edge/0-a-b/arrow")
    else {
        panic!()
    };
    assert_eq!(style.fill, Color::hex("#8793A5"));
    assert_eq!(style.opacity, 0.4);
    assert_eq!(commands.len(), 4);
    assert!(!marker_end);
    let SceneNode::Path { style, .. } = get("graph/edge/1-b-c/arrow") else {
        panic!()
    };
    assert_eq!(style.fill, defaults.edge);
    let SceneNode::Path { marker_end, .. } = get("graph/edge/1-b-c") else {
        panic!()
    };
    assert!(!marker_end);
    let legacy = compile(&diagram()).unwrap();
    assert!(find_scene_node(&legacy.scene.nodes, "graph/edge/1-b-c/arrow").is_none());
    let SceneNode::Path { marker_end, .. } =
        find_scene_node(&legacy.scene.nodes, "graph/edge/1-b-c").unwrap()
    else {
        panic!()
    };
    assert!(*marker_end);
}

#[test]
fn round_trip_and_rematerialization_keep_theme_identity_order_and_explicit_scales() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let original = compile_with_theme(&chart(kind, None), "stone-teal-dark").unwrap();
        let mut loaded: ThemedMir =
            serde_json::from_slice(&serde_json::to_vec(&original.mir).unwrap()).unwrap();
        assert_eq!(build_themed_scene(&loaded).unwrap(), original.scene);
        let MirDataOperator::Inline { rows } =
            &mut loaded.mir.data.get_mut("data/data").unwrap().operator;
        rows[0].insert("y".into(), json!(3.5));
        let saved = loaded.clone();
        assert!(
            build_themed_scene(&loaded)
                .unwrap_err()
                .to_string()
                .contains("VIZ-MATERIALIZE-0002")
        );
        let refreshed = rematerialize_themed_mir(&loaded).unwrap();
        assert_eq!(loaded, saved);
        assert_eq!(refreshed.theme, original.mir.theme);
        assert_eq!(rematerialize_themed_mir(&refreshed).unwrap(), refreshed);
        let MirView::Chart(c) = &refreshed.mir.views[0] else {
            panic!()
        };
        let MirView::Chart(old) = &original.mir.mir.views[0] else {
            panic!()
        };
        assert_eq!(c.scales, old.scales);
        match &c.mark {
            ChartMark::Symbol { instances, .. } => {
                assert_eq!((&instances[0].key, instances[0].y), (&"b".to_owned(), 3.5))
            }
            ChartMark::Bar { instances, .. } => assert_eq!(
                (&instances[0].key, instances[0].value),
                (&"b".to_owned(), 3.5)
            ),
            ChartMark::Line { series, .. } => assert_eq!(
                series[0]
                    .points
                    .iter()
                    .map(|p| (p.key.as_str(), p.y))
                    .collect::<Vec<_>>(),
                vec![("a", 2.0), ("b", 3.5)]
            ),
        }
        build_themed_scene(&refreshed).unwrap();
    }
}

#[test]
fn tampered_context_and_generated_arrow_work_fail_before_refresh() {
    let original = compile_with_theme(&diagram(), "warm-sand").unwrap().mir;
    for field in [
        "format",
        "name",
        "registry_spec",
        "registry_version",
        "registry_revision",
        "defaults",
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        if field == "format" {
            value[field] = json!("vizir-themed-mir/2");
        } else if field == "defaults" {
            value["theme"][field]["ink"] = json!("#ffffff");
        } else {
            value["theme"][field] = json!("wrong");
        }
        let bad: ThemedMir = serde_json::from_value(value).unwrap();
        assert!(build_themed_scene(&bad).is_err());
        assert!(rematerialize_themed_mir(&bad).is_err());
    }
    let limits = MaterializationLimits {
        max_expression_nodes: 7,
        ..MaterializationLimits::default()
    };
    assert!(
        build_themed_scene_with_limits(&original, limits)
            .unwrap_err()
            .to_string()
            .contains("VIZ-THEME-0004")
    );
    assert_eq!(original.format, THEMED_MIR_FORMAT);
}

#[test]
fn strict_context_reader_rejects_nested_discarded_fields_and_duplicate_map_keys() {
    let original = compile_with_theme(&chart("chart.scatter", None), "azure")
        .unwrap()
        .mir;
    let value = serde_json::to_value(&original).unwrap();
    let mut bad = value.clone();
    bad["mir"]["expressions"]["expr/chart/x"]["result_type"]["accidental_style"] = json!("#123456");
    assert!(vizir_compiler::parse_themed_mir_json(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = value.clone();
    bad["mir"]["expressions"]["expr/chart/x"]["expression"]["ignored"] = json!(null);
    assert!(serde_json::from_slice::<ThemedMir>(&serde_json::to_vec(&bad).unwrap()).is_err());
    let json = serde_json::to_string(&value).unwrap();
    for (needle, replacement) in [
        ("\"data/data\":", "\"data/data\":null,\"data/data\":"),
        ("\"y\":4.0", "\"y\":4.0,\"y\":4.0"),
    ] {
        assert!(json.contains(needle));
        let duplicate = json.replacen(needle, replacement, 1);
        assert!(
            vizir_compiler::parse_themed_mir_json(duplicate.as_bytes())
                .unwrap_err()
                .to_string()
                .contains("duplicate JSON object key")
        );
    }
}

#[test]
fn strict_reader_keeps_optional_null_defaults_and_numeric_fidelity() {
    let mut doc = serde_json::to_value(chart("chart.scatter", None)).unwrap();
    doc["views"][0]["y"]["axis"] = json!({"number_format":{"notation":"fixed","precision":2}});
    for (i, key) in [9007199254740993i64, 9007199254740994, 9007199254740995]
        .into_iter()
        .enumerate()
    {
        doc["datasets"]["data"]["rows"][i]["id"] = json!(key);
        doc["datasets"]["data"]["rows"][i]["metadata"] =
            json!({"nullable":null,"signed_zero":-0.0,"tiny":5e-324});
    }
    let original = compile_with_theme(&serde_json::from_value(doc).unwrap(), "sage-dark").unwrap();
    let mut value = serde_json::to_value(&original.mir).unwrap();
    value["mir"]["spaces"]["space/document"]["parent"] = json!(null);
    let encoded = serde_json::to_string(&value)
        .unwrap()
        .replace("\"precision\":2", "\"precision\":2.0");
    let parsed = vizir_compiler::parse_themed_mir_json(encoded.as_bytes()).unwrap();
    assert_eq!(build_themed_scene(&parsed).unwrap(), original.scene);
    assert_eq!(rematerialize_themed_mir(&parsed).unwrap(), original.mir);
    let MirDataOperator::Inline { rows } = &parsed.mir.data["data/data"].operator;
    assert_eq!(rows[0]["id"].as_i64(), Some(9007199254740993));
    assert_eq!(
        rows[0]["metadata"]["signed_zero"]
            .as_f64()
            .unwrap()
            .to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(rows[0]["metadata"]["tiny"].as_f64().unwrap().to_bits(), 1);
    let mut value =
        serde_json::to_value(compile_with_theme(&diagram(), "azure").unwrap().mir).unwrap();
    value["mir"]["views"][0]["nodes"][2]["style"] = json!({});
    vizir_compiler::parse_themed_mir_json(&serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn arrow_commands_share_existing_global_expression_and_evaluation_budgets() {
    let mut doc = chart("chart.scatter", None);
    doc.views.extend(diagram().views);
    let mir = compile_with_theme(&doc, "azure").unwrap().mir;
    let count = mir.mir.expressions.len(); // this fixture contains only single-node field ASTs
    let limits = MaterializationLimits {
        max_expression_nodes: count + 8,
        ..MaterializationLimits::default()
    };
    build_themed_scene_with_limits(&mir, limits).unwrap();
    assert!(
        build_themed_scene_with_limits(
            &mir,
            MaterializationLimits {
                max_expression_nodes: count + 7,
                ..limits
            }
        )
        .is_err()
    );
    // Find the exact existing executor work boundary independently of arrows.
    let (mut low, mut high) = (0, 10_000);
    while low < high {
        let mid = (low + high) / 2;
        if vizir_compiler::build_scene_with_limits(
            &mir.mir,
            MaterializationLimits {
                max_evaluation_steps: mid,
                ..MaterializationLimits::default()
            },
        )
        .is_ok()
        {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    assert!(low < 10_000);
    build_themed_scene_with_limits(
        &mir,
        MaterializationLimits {
            max_evaluation_steps: low + 8,
            ..MaterializationLimits::default()
        },
    )
    .unwrap();
    assert!(
        build_themed_scene_with_limits(
            &mir,
            MaterializationLimits {
                max_evaluation_steps: low + 7,
                ..MaterializationLimits::default()
            }
        )
        .is_err()
    );
}
