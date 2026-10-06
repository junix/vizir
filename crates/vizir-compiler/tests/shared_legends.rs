#[path = "support/wrapping_fixtures.rs"]
mod fixtures;

use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, FontResources, MaterializationLimits, TextLimits, build_compiled_scene,
    build_scene, build_scene_with_limits, compile, compile_with_context,
    compile_with_context_with_limits, compile_with_theme, parse_compiled_mir_json,
    rematerialize_compiled_mir, rematerialize_mir, rematerialize_mir_with_limits,
};
use vizir_core::{
    ChartMark, Composition, Document, GuideKind, MirChart, MirDataOperator, MirScale, MirView,
    Rect, Scene2D, SceneNode, VizMir, compose_versioned, find_scene_node,
};

const OWNER: &str = "series-key";
const DOMAIN: [&str; 3] = ["Beta", "Alpha", "Reserved"];

fn encoding(kind: &str) -> &'static str {
    if matches!(kind, "line" | "area") {
        "series"
    } else {
        "color"
    }
}

fn panel(kind: &str, id: &str, dataset: &str) -> Value {
    let mut panel = json!({"kind":format!("chart.{kind}"),"id":id,"dataset":dataset});
    if kind == "bar" {
        panel["category"] = json!({"field":"id"});
        panel["value"] = json!({"field":"y"});
    } else {
        panel["x"] = json!({"field":"x"});
        panel["y"] = json!({"field":"y"});
    }
    if kind == "area" {
        panel["baseline"] = json!(0);
        panel["order"] = json!("x-ascending");
    }
    panel[encoding(kind)] = json!({"field":"group","domain":DOMAIN});
    panel
}

fn source(left: &str, right: &str) -> Value {
    json!({
        "schema":"vizir-composition/0.7", "id":"shared-colors", "width":1100, "height":620,
        "layout":{"kind":"grid","columns":2,"gap":24,"padding":24},
        "datasets":{
            "left-data":{"key":"id","rows":[
                {"id":"a0","x":0,"y":2,"group":"Alpha"},
                {"id":"a1","x":1,"y":3,"group":"Alpha"}
            ]},
            "right-data":{"key":"id","rows":[
                {"id":"b0","x":0,"y":4,"group":"Beta"},
                {"id":"b1","x":1,"y":5,"group":"Beta"}
            ]}
        },
        "panels":[panel(left,"left","left-data"),panel(right,"right","right-data")],
        "shared_legend":{"id":OWNER,"members":["left","right"],"placement":"bottom","height":100,"gap":18}
    })
}

fn document(source: Value) -> Document {
    let input: Composition = serde_json::from_value(source).expect("composition parses");
    compose_versioned(&input).expect("composition lowers to validated HIR")
}

fn chart(mir: &VizMir, index: usize) -> &MirChart {
    let MirView::Chart(chart) = &mir.views[index] else {
        panic!("expected chart")
    };
    chart
}

fn color_scale(mir: &VizMir, index: usize) -> &MirScale {
    chart(mir, index)
        .scales
        .iter()
        .find(|scale| matches!(scale, MirScale::OrdinalColor { .. }))
        .expect("bound color scale is retained")
}

fn mapping(mir: &VizMir, index: usize) -> (&[String], Vec<&str>) {
    let MirScale::OrdinalColor { domain, range, .. } = color_scale(mir, index) else {
        unreachable!()
    };
    (domain, range.iter().map(|color| color.0.as_str()).collect())
}

fn root_id(owner: &str) -> String {
    format!("shared-legend:{}:{owner}", owner.len())
}

fn entry<'a>(scene: &'a Scene2D, owner: &str, index: usize, role: &str) -> &'a SceneNode {
    find_scene_node(
        &scene.nodes,
        &format!("{}/entry/{index}/{role}", root_id(owner)),
    )
    .unwrap_or_else(|| panic!("missing shared legend entry {index} {role}"))
}

fn bounds(node: &SceneNode) -> Rect {
    match node {
        SceneNode::Group { bounds, .. }
        | SceneNode::Rect { bounds, .. }
        | SceneNode::Circle { bounds, .. }
        | SceneNode::Line { bounds, .. }
        | SceneNode::Path { bounds, .. }
        | SceneNode::Text { bounds, .. } => *bounds,
    }
}

fn painted_keys(mir: &VizMir, index: usize) -> Vec<&str> {
    match &chart(mir, index).mark {
        ChartMark::Line { series, .. } | ChartMark::Area { series, .. } => {
            series.iter().map(|series| series.key.as_str()).collect()
        }
        ChartMark::Symbol { instances, .. } => {
            instances.iter().map(|point| point.key.as_str()).collect()
        }
        ChartMark::Bar { instances, .. } => instances.iter().map(|bar| bar.key.as_str()).collect(),
        ChartMark::Heatmap { .. } => panic!("heatmaps are not shared-legend members"),
    }
}

fn set_domain(source: &mut Value, domain: &[String]) {
    for panel in source["panels"].as_array_mut().unwrap() {
        let channel = if panel.get("series").is_some() {
            "series"
        } else {
            "color"
        };
        panel[channel]["domain"] = json!(domain);
    }
}

fn reject_source(value: Value) -> String {
    let result = serde_json::from_value::<Composition>(value)
        .map_err(|error| error.to_string())
        .and_then(|source| compose_versioned(&source).map_err(|error| error.to_string()))
        .and_then(|document| compile(&document).map_err(|error| error.to_string()));
    result.expect_err("invalid shared legend must fail closed")
}

fn reject_mir(value: Value) -> String {
    serde_json::from_value::<VizMir>(value)
        .map_err(|error| error.to_string())
        .and_then(|mir| build_scene(&mir).map_err(|error| error.to_string()))
        .expect_err("forged shared legend must fail at decode or replay")
}

#[test]
fn four_kinds_and_mixed_panels_share_one_full_ordered_legend_without_phantom_marks() {
    for [left, right] in [
        ["line", "area"],
        ["scatter", "bar"],
        ["line", "bar"],
        ["scatter", "area"],
    ] {
        let d = document(source(left, right));
        let before = serde_json::to_vec(&d).unwrap();
        let c = compile(&d).unwrap();
        assert_eq!(d.version, "0.8");
        assert_eq!(c.mir.version, "0.8");
        assert_eq!(c.mir.source_hir_version, "0.8");
        let hir = serde_json::to_value(&d).unwrap();
        assert_eq!(hir["shared_legend"]["members"], json!(["left", "right"]));
        assert_eq!(
            hir["shared_legend"]["frame"],
            json!({"x":24.0,"y":496.0,"width":1052.0,"height":100.0})
        );
        assert_eq!(
            hir["views"][0]["frame"],
            json!({"x":24.0,"y":24.0,"width":514.0,"height":454.0})
        );
        assert_eq!(mapping(&c.mir, 0), mapping(&c.mir, 1));
        assert_eq!(mapping(&c.mir, 0).0, DOMAIN);
        for (index, (id, kind, group, keys)) in [
            ("left", left, "Alpha", vec!["a0", "a1"]),
            ("right", right, "Beta", vec!["b0", "b1"]),
        ]
        .into_iter()
        .enumerate()
        {
            let chart = chart(&c.mir, index);
            assert!(
                chart
                    .guides
                    .iter()
                    .all(|guide| guide.kind != GuideKind::Legend)
            );
            assert!(find_scene_node(&c.scene.nodes, &format!("{id}/legend/0/label")).is_none());
            assert_eq!(
                painted_keys(&c.mir, index),
                if matches!(kind, "line" | "area") {
                    vec![group]
                } else {
                    keys
                }
            );
            let member = &serde_json::to_value(&c.mir).unwrap()["shared_legend"]["members"][index];
            assert_eq!(member["view"], id);
            assert_eq!(member["scale"], color_scale(&c.mir, index).id());
        }
        for (index, expected) in DOMAIN.into_iter().enumerate() {
            let label = entry(&c.scene, OWNER, index, "label");
            assert!(matches!(label, SceneNode::Text { text, .. } if text == expected));
            assert_eq!(label.origin().hir_node, OWNER);
            assert_eq!(label.origin().mir_node, OWNER);
            assert!(label.origin().data_key.is_none());
            assert_eq!(
                label.origin().data_lineage,
                ["data/left-data", "data/right-data"]
            );
            let swatch = entry(&c.scene, OWNER, index, "swatch");
            assert!(
                matches!(swatch, SceneNode::Rect { style, .. } | SceneNode::Circle { style, .. } if style.fill.0 == mapping(&c.mir, 0).1[index])
            );
        }
        assert!(
            find_scene_node(&c.scene.nodes, &format!("{}/entry/3/label", root_id(OWNER))).is_none()
        );
        assert_eq!(serde_json::to_vec(&d).unwrap(), before);
        assert_eq!(compile(&d).unwrap().scene, c.scene);
    }
}

#[test]
fn only_members_lose_local_legends_and_bound_scales_are_unchanged() {
    for kind in ["line", "area", "scatter", "bar"] {
        let mut input = source("line", "scatter");
        input["height"] = json!(1000);
        input["panels"]
            .as_array_mut()
            .unwrap()
            .push(panel(kind, "other", "left-data"));
        let c = compile(&document(input.clone())).unwrap();
        assert!(
            find_scene_node(&c.scene.nodes, "other/legend/0/label").is_some(),
            "{kind}"
        );
        input.as_object_mut().unwrap().remove("shared_legend");
        let local = compile(&document(input)).unwrap();
        assert_eq!(chart(&c.mir, 2).guides, chart(&local.mir, 2).guides);
        for index in 0..3 {
            assert_eq!(color_scale(&c.mir, index), color_scale(&local.mir, index));
            assert_eq!(painted_keys(&c.mir, index), painted_keys(&local.mir, index));
        }
        assert_eq!(c.mir.data, local.mir.data);
        assert_eq!(c.mir.expressions, local.mir.expressions);
    }
}

#[test]
fn palette_equivalence_uses_resolved_domain_mapping_including_cycles() {
    let mut input = source("line", "bar");
    input["panels"][0]["series"]["palette"] = json!(["#112233", "#445566"]);
    input["panels"][1]["color"]["palette"] = json!(["#112233", "#445566", "#112233"]);
    let d = document(input.clone());
    let c = compile(&d).unwrap();
    assert_eq!(mapping(&c.mir, 0).1, ["#112233", "#445566", "#112233"]);
    assert_eq!(mapping(&c.mir, 0), mapping(&c.mir, 1));
    for theme in vizir_compiler::THEME_NAMES {
        let themed = compile_with_theme(&d, theme).unwrap();
        assert_eq!(mapping(&themed.mir.mir, 0), mapping(&themed.mir.mir, 1));
    }
    // Only the globally absent category changes: comparing observed marks is insufficient.
    input["panels"][1]["color"]["palette"][2] = json!("#778899");
    let error = reject_source(input);
    assert!(
        error.contains("legend") || error.contains("mapping"),
        "{error}"
    );
}

#[test]
fn every_member_requires_an_authored_byte_exact_ordered_domain() {
    for changed in [
        json!(["Alpha", "Beta", "Reserved"]),
        json!(["Beta", "Alpha", "reserved"]),
        json!(["Beta", "Alpha", "Reserved "]),
        json!(["Beta", "Alpha"]),
    ] {
        let mut input = source("area", "bar");
        input["panels"][1]["color"]["domain"] = changed;
        assert!(!reject_source(input).is_empty());
    }
    let mut input = source("line", "scatter");
    input["panels"][1]["color"]
        .as_object_mut()
        .unwrap()
        .remove("domain");
    assert!(!reject_source(input).is_empty());
    let mut input = source("line", "scatter");
    input["panels"][0]["series"]["domain"][2] = json!("Café");
    input["panels"][1]["color"]["domain"][2] = json!("Cafe\u{301}");
    assert!(!reject_source(input).is_empty());
}

#[test]
fn restored_mir_replays_and_data_refresh_keeps_the_complete_legend_mapping() {
    let d = document(source("area", "scatter"));
    let c = compile(&d).unwrap();
    let bytes = serde_json::to_vec(&c.mir).unwrap();
    let restored: VizMir = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, c.mir);
    assert_eq!(build_scene(&restored).unwrap(), c.scene);
    assert_eq!(rematerialize_mir(&restored).unwrap(), restored);
    let mut edited = restored.clone();
    let MirDataOperator::Inline { rows } =
        &mut edited.data.get_mut("data/left-data").unwrap().operator;
    rows[0].insert("y".into(), json!(3));
    assert!(
        build_scene(&edited).is_err(),
        "stale mark cache cannot become trusted input"
    );
    let refreshed = rematerialize_mir(&edited).unwrap();
    for index in 0..2 {
        assert_eq!(
            chart(&refreshed, index).scales,
            chart(&restored, index).scales
        );
    }
    assert_eq!(
        serde_json::to_value(&refreshed).unwrap()["shared_legend"],
        serde_json::to_value(&restored).unwrap()["shared_legend"]
    );
    assert!(build_scene(&refreshed).is_ok());
}

#[test]
fn shared_strip_fits_full_labels_that_could_not_fit_a_member_local_header() {
    let mut input = source("line", "area");
    let long = "W".repeat(70);
    set_domain(&mut input, &["Beta".into(), "Alpha".into(), long.clone()]);
    let c = compile(&document(input)).unwrap();
    assert!(
        matches!(entry(&c.scene, OWNER, 2, "label"), SceneNode::Text { text, font_size, .. } if text == &long && *font_size == 10.5)
    );
}

#[test]
fn fixed_shared_strip_rejects_overwide_entry_title_and_exhausted_height() {
    for case in ["entry", "title", "height"] {
        let mut input = source("line", "bar");
        match case {
            "entry" => set_domain(
                &mut input,
                &["Beta".into(), "Alpha".into(), "W".repeat(400)],
            ),
            "title" => input["shared_legend"]["title"] = json!("W".repeat(400)),
            _ => input["shared_legend"]["height"] = json!(12),
        }
        let error = reject_source(input);
        assert!(
            error.contains("legend") || error.contains("layout") || error.contains("LAYOUT"),
            "{case}: {error}"
        );
    }
}

#[test]
fn measured_latin_cjk_entries_pack_in_domain_order_inside_the_fixed_frame() {
    let labels = [
        "Beta",
        "Alpha",
        "AV office ffi Café Å A\u{30a}",
        "中文字体测量，保留标点。",
        "Temperature service legend",
        "AV office ffi Café Å A\u{30a} AV office",
        "中文字体测量，保留标点。中文字体",
    ]
    .map(str::to_owned);
    let mut input = source("line", "scatter");
    input["width"] = json!(760);
    input["shared_legend"]["id"] = json!("shared/series");
    input["shared_legend"]["title"] = json!("服务 Temperature");
    set_domain(&mut input, &labels);
    let d = document(input);
    let (context, resources) = fixtures::profile();
    let c = compile_with_context(&d, &context, &resources).unwrap();
    let frame: Rect =
        serde_json::from_value(serde_json::to_value(&d).unwrap()["shared_legend"]["frame"].clone())
            .unwrap();
    let title = find_scene_node(
        &c.scene.nodes,
        &format!("{}/title", root_id("shared/series")),
    )
    .unwrap();
    assert!(matches!(title, SceneNode::Path { .. }));
    assert!(title.origin().explanation.contains("服务 Temperature"));
    let mut row_y = Vec::new();
    let mut previous: Option<Rect> = None;
    for (index, expected) in labels.iter().enumerate() {
        let label = entry(&c.scene, "shared/series", index, "label");
        let swatch = entry(&c.scene, "shared/series", index, "swatch");
        assert!(matches!(label, SceneNode::Path { .. }));
        assert!(
            label.origin().explanation.contains(expected),
            "full source label is retained"
        );
        assert_eq!(label.origin().hir_node, "shared/series");
        for node in [label, swatch] {
            let b = bounds(node);
            assert!(b.x >= frame.x && b.y >= frame.y, "{} {b:?}", node.id());
            assert!(
                b.x + b.width <= frame.x + frame.width + 0.0001,
                "{} {b:?}",
                node.id()
            );
            assert!(
                b.y + b.height <= frame.y + frame.height + 0.0001,
                "{} {b:?}",
                node.id()
            );
        }
        let b = bounds(swatch);
        match previous {
            None => row_y.push(b.y),
            Some(previous) if b.x < previous.x => {
                assert!(
                    b.y > previous.y + previous.height,
                    "packed rows cannot overlap"
                );
                row_y.push(b.y);
            }
            Some(previous) => {
                assert!(
                    b.x > previous.x + previous.width,
                    "entries stay in domain order within the row"
                );
            }
        }
        previous = Some(b);
    }
    assert!(row_y.len() >= 2, "fixture must exercise real row packing");
    assert!(row_y.windows(2).all(|rows| rows[1] > rows[0]));
    let restored = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(
        build_compiled_scene(&restored, &resources).unwrap(),
        c.scene
    );
    assert_eq!(
        rematerialize_compiled_mir(&restored, &resources).unwrap(),
        restored
    );
    assert_eq!(
        compile_with_context(&d, &context, &resources)
            .unwrap()
            .scene,
        c.scene
    );
}

#[test]
fn measured_replay_requires_fonts_and_never_silently_falls_back_for_legend_glyphs() {
    let (context, resources) = fixtures::profile();
    let c = compile_with_context(&document(source("line", "bar")), &context, &resources).unwrap();
    assert!(build_compiled_scene(&c.mir, &FontResources::new()).is_err());
    for case in ["entry", "title"] {
        let mut input = source("line", "bar");
        if case == "entry" {
            set_domain(
                &mut input,
                &["Beta".into(), "Alpha".into(), "Missing 😀".into()],
            );
        } else {
            input["shared_legend"]["title"] = json!("Missing 😀");
        }
        let error = compile_with_context(&document(input), &context, &resources)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("glyph") || error.contains("VIZ-TEXT"),
            "{case}: {error}"
        );
    }
}

#[test]
fn shared_legend_text_participates_in_whole_call_budgets() {
    let (context, resources) = fixtures::profile();
    let d = document(source("line", "scatter"));
    for field in 0..7 {
        let mut limits = TextLimits::default();
        match field {
            0 => limits.max_label_bytes = 0,
            1 => limits.max_text_bytes = 0,
            2 => limits.max_labels = 0,
            3 => limits.max_glyphs = 0,
            4 => limits.max_outline_commands = 0,
            5 => limits.max_cache_bytes = 0,
            _ => limits.max_output_bytes = 1,
        }
        let error = compile_with_context_with_limits(
            &d,
            &context,
            &resources,
            MaterializationLimits::default(),
            limits,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("VIZ-TEXT-0003"), "budget {field}: {error}");
    }
    // An absent category is only rendered by the shared legend, so this budget
    // specifically catches legend labels missed by preflight and layout work.
    let mut input = source("line", "scatter");
    set_domain(&mut input, &["Beta".into(), "Alpha".into(), "W".repeat(80)]);
    let mut limits = TextLimits::default();
    limits.max_label_bytes = 64;
    let error = compile_with_context_with_limits(
        &document(input),
        &context,
        &resources,
        MaterializationLimits::default(),
        limits,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("VIZ-TEXT-0003"), "{error}");
}

#[test]
fn forged_mapping_wrong_refs_and_local_shared_ownership_conflicts_are_rejected() {
    let compiled = compile(&document(source("line", "scatter"))).unwrap();
    let original = serde_json::to_value(&compiled.mir).unwrap();
    for case in [
        "color",
        "domain",
        "wrong-view",
        "wrong-scale",
        "numeric-scale",
        "other-view-scale",
        "unbound-scale",
        "local-conflict",
    ] {
        let mut forged = original.clone();
        match case {
            "color" | "domain" => {
                let scale = forged["views"][1]["scales"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|scale| scale["type"] == "ordinal-color")
                    .unwrap();
                if case == "color" {
                    scale["range"][2] = json!("#123456");
                } else {
                    scale["domain"][2] = json!("Forged");
                }
            }
            "wrong-view" => forged["shared_legend"]["members"][0]["view"] = json!("missing"),
            "wrong-scale" => forged["shared_legend"]["members"][0]["scale"] = json!("missing"),
            "numeric-scale" => forged["shared_legend"]["members"][0]["scale"] = json!("left/x"),
            "other-view-scale" => {
                forged["shared_legend"]["members"][0]["scale"] = json!("right/color")
            }
            "unbound-scale" => {
                let mut decoy = forged["views"][0]["scales"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|scale| scale["type"] == "ordinal-color")
                    .unwrap()
                    .clone();
                decoy["id"] = json!("left/decoy-color");
                forged["views"][0]["scales"]
                    .as_array_mut()
                    .unwrap()
                    .push(decoy);
                forged["shared_legend"]["members"][0]["scale"] = json!("left/decoy-color");
            }
            _ => {
                forged["views"][0]["guides"].as_array_mut().unwrap().push(json!({"id":"left/guides/forged-legend","kind":"legend","scale":"left/color","label":"group","orient":"right"}));
            }
        }
        assert!(!reject_mir(forged).is_empty(), "{case}");
    }
}

#[test]
fn shared_ownership_does_not_weaken_stale_range_or_mark_cache_checks() {
    let c = compile(&document(source("scatter", "bar"))).unwrap();
    for case in ["cache", "range"] {
        let mut value = serde_json::to_value(&c.mir).unwrap();
        if case == "cache" {
            value["views"][0]["mark"]["instances"][0]["y"] = json!(999);
        } else {
            value["views"][0]["scales"][1]["range"][1] = json!(123);
        }
        assert!(!reject_mir(value).is_empty(), "{case}");
    }
    let (context, resources) = fixtures::profile();
    let c =
        compile_with_context(&document(source("scatter", "bar")), &context, &resources).unwrap();
    let mut stale = c.mir.clone();
    let MirView::Chart(chart) = &mut stale.mir.views[0] else {
        panic!()
    };
    let MirScale::Linear { range, .. } = &mut chart.scales[1] else {
        panic!()
    };
    range[1] += 1.0;
    assert!(build_compiled_scene(&stale, &resources).is_err());
    assert!(rematerialize_compiled_mir(&stale, &resources).is_err());
}

#[test]
fn measured_font_envelopes_roundtrip_shared_owners_for_all_four_mark_kinds() {
    let (context, resources) = fixtures::profile();
    for [left, right] in [["line", "area"], ["scatter", "bar"]] {
        let mut input = source(left, right);
        input["shared_legend"]["title"] = json!("服务");
        let c = compile_with_context(&document(input), &context, &resources).unwrap();
        let restored = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
        assert_eq!(restored, c.mir);
        assert_eq!(
            build_compiled_scene(&restored, &resources).unwrap(),
            c.scene
        );
        assert_eq!(
            rematerialize_compiled_mir(&restored, &resources).unwrap(),
            restored
        );
        for index in 0..3 {
            assert!(matches!(
                entry(&c.scene, OWNER, index, "label"),
                SceneNode::Path { .. }
            ));
        }
        for index in 0..2 {
            assert_eq!(
                chart(&restored.mir, index).scales,
                chart(&c.mir.mir, index).scales
            );
        }
    }
}

#[test]
fn shared_owner_preflight_obeys_whole_call_materialization_work_limits() {
    let d = document(source("line", "scatter"));
    let c = compile(&d).unwrap();
    let limits = MaterializationLimits {
        max_evaluation_steps: 0,
        ..MaterializationLimits::default()
    };
    for error in [
        build_scene_with_limits(&c.mir, limits)
            .unwrap_err()
            .to_string(),
        rematerialize_mir_with_limits(&c.mir, limits)
            .unwrap_err()
            .to_string(),
        compile_with_context_with_limits(
            &d,
            &CompilationContext::new(),
            &FontResources::new(),
            limits,
            TextLimits::default(),
        )
        .unwrap_err()
        .to_string(),
    ] {
        assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    }
}

fn replace_string(value: &mut Value, old: &str, new: &str) {
    match value {
        Value::String(s) if s == old => *s = new.to_owned(),
        Value::Array(values) => {
            for value in values {
                replace_string(value, old, new);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace_string(value, old, new);
            }
        }
        _ => {}
    }
}

#[test]
fn repeated_owner_and_member_lineage_metadata_are_bounded_before_output_cloning() {
    let mut input = source("line", "scatter");
    input["width"] = json!(1800);
    input["height"] = json!(1200);
    input["shared_legend"]["height"] = json!(600);
    let mut domain = vec!["Beta".to_owned(), "Alpha".to_owned()];
    domain.extend((2..256).map(|index| format!("Category{index:03}")));
    set_domain(&mut input, &domain);
    let baseline = compile(&document(input.clone())).unwrap();
    assert!(
        entry(&baseline.scene, OWNER, 255, "label")
            .id()
            .ends_with("/255/label")
    );

    for case in ["owner", "lineage"] {
        let mut amplified_source = input.clone();
        let mut amplified_mir = serde_json::to_value(&baseline.mir).unwrap();
        match case {
            "owner" => {
                // This is a valid source ID under the 16 KiB identity limit,
                // but copying it into each node's ID and origin is not free.
                let large_id = "o".repeat(16_000);
                amplified_source["shared_legend"]["id"] = json!(large_id);
                amplified_mir["shared_legend"]["id"] = json!(large_id);
            }
            _ => {
                // Dataset names have no short fixed length limit. Every shared
                // entry retains every distinct member source in provenance.
                let large_name = format!("source-{}", "s".repeat(30_000));
                let data = amplified_source["datasets"]
                    .as_object_mut()
                    .unwrap()
                    .remove("left-data")
                    .unwrap();
                amplified_source["datasets"][&large_name] = data;
                amplified_source["panels"][0]["dataset"] = json!(large_name);
                let large_source = format!("data/{large_name}");
                let data = amplified_mir["data"]
                    .as_object_mut()
                    .unwrap()
                    .remove("data/left-data")
                    .unwrap();
                amplified_mir["data"][&large_source] = data;
                replace_string(&mut amplified_mir, "data/left-data", &large_source);
            }
        }
        let source = document(amplified_source);
        vizir_core::validate_document(&source).unwrap();
        let mir: VizMir = serde_json::from_value(amplified_mir).unwrap();
        vizir_core::validate_mir(&mir).unwrap();
        for error in [
            compile(&source).unwrap_err().to_string(),
            build_scene(&mir).unwrap_err().to_string(),
            rematerialize_mir(&mir).unwrap_err().to_string(),
        ] {
            assert!(error.contains("VIZ-MATERIALIZE-0001"), "{case}: {error}");
            assert!(
                error.contains("whole-call evaluation work limit"),
                "{case}: {error}"
            );
        }
    }
}
