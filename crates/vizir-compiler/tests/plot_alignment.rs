#[path = "support/wrapping_fixtures.rs"]
mod fixtures;

use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, FontResources, MaterializationLimits, SemanticTextLayoutTarget,
    TextLayoutContext, TextLimits, build_compiled_scene, build_compiled_scene_with_limits,
    build_scene, build_scene_with_limits, build_themed_scene, compile, compile_with_context,
    compile_with_context_with_limits, compile_with_theme, parse_compiled_mir_json,
    rematerialize_compiled_mir, rematerialize_mir, rematerialize_mir_with_limits,
};
use vizir_core::{
    ChartMark, Composition, Document, GuideKind, MirChart, MirDataOperator, MirScale, MirView,
    Rect, SceneNode, VizMir, compose_versioned, find_scene_node,
};

const MEMBERS: [&str; 3] = ["left", "middle", "right"];

fn panel(kind: &str, id: &str) -> Value {
    let mut panel = json!({
        "kind":format!("chart.{kind}"), "id":id, "dataset":"samples",
        "x":{"field":"x","label":"Elapsed time","domain":[0,10]},
        "y":{"field":"y","label":"Value","domain":[0,10]}
    });
    panel[if kind == "scatter" { "color" } else { "series" }] =
        json!({"field":"group","domain":["Beta","Alpha","Reserved"]});
    if kind == "area" {
        panel["baseline"] = json!(0);
        panel["order"] = json!("x-ascending");
    }
    panel
}

fn source(shared: bool) -> Value {
    let mut source = json!({
        "schema":"vizir-composition/0.8", "id":"aligned-numeric-plots",
        "width":2448, "height":680,
        "layout":{"kind":"grid","columns":4,"padding":24,"gap":24},
        "datasets":{"samples":{"key":"id","rows":[
            {"id":"a0","x":0,"y":2,"group":"Alpha"},
            {"id":"a1","x":10,"y":8,"group":"Alpha"},
            {"id":"b0","x":0,"y":4,"group":"Beta"},
            {"id":"b1","x":10,"y":6,"group":"Beta"}
        ]}},
        "panels":[panel("line","left"),panel("scatter","middle"),
            panel("area","right"),panel("scatter","other")],
        "plot_alignment":{"id":"comparison","mode":"uniform","members":MEMBERS}
    });
    source["panels"][0]["title"] = json!("Temperature over elapsed time");
    source["panels"][0]["y"]["label"] = json!("Long descriptive temperature measurement");
    source["panels"][0]["y"]["axis"] = json!({"number_format":{"notation":"fixed","precision":8}});
    source["panels"][1]["x"]["axis"] = json!({"number_format":{"notation":"fixed","precision":4}});
    if shared {
        // Deliberately different membership: alignment cannot infer legend ownership.
        source["shared_legend"] = json!({"id":"series-key","members":["left","middle"],
            "placement":"bottom","height":70,"gap":18});
    }
    source
}

fn document(source: Value) -> Document {
    let source: Composition = serde_json::from_value(source).expect("composition parses");
    compose_versioned(&source).expect("composition lowers")
}

fn without_alignment(mut source: Value) -> Value {
    source.as_object_mut().unwrap().remove("plot_alignment");
    source
}

fn chart<'a>(mir: &'a VizMir, id: &str) -> &'a MirChart {
    let MirView::Chart(chart) = mir.views.iter().find(|view| view.id() == id).unwrap() else {
        panic!("expected chart {id}")
    };
    chart
}

fn chart_mut<'a>(mir: &'a mut VizMir, id: &str) -> &'a mut MirChart {
    let MirView::Chart(chart) = mir.views.iter_mut().find(|view| view.id() == id).unwrap() else {
        panic!("expected chart {id}")
    };
    chart
}

fn numeric_scale<'a>(chart: &'a MirChart, axis: &str) -> &'a MirScale {
    let (x, y) = match &chart.mark {
        ChartMark::Line { x, y, .. }
        | ChartMark::Symbol { x, y, .. }
        | ChartMark::Area { x, y, .. } => (x, y),
        _ => panic!("only numeric-position chart members"),
    };
    let id = if axis == "x" { &x.scale } else { &y.scale };
    chart.scales.iter().find(|scale| scale.id() == id).unwrap()
}

fn range(chart: &MirChart, axis: &str) -> [f64; 2] {
    let MirScale::Linear { range, .. } = numeric_scale(chart, axis) else {
        panic!("linear position scale")
    };
    *range
}

fn insets(chart: &MirChart) -> [f64; 4] {
    let [left, right] = range(chart, "x");
    let [bottom, top] = range(chart, "y");
    let frame = chart.frame;
    [
        left - frame.x,
        top - frame.y,
        frame.x + frame.width - right,
        frame.y + frame.height - bottom,
    ]
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

fn assert_uniform(mir: &VizMir, members: &[&str]) {
    let first = insets(chart(mir, members[0]));
    for member in members {
        for (actual, expected) in insets(chart(mir, member)).into_iter().zip(first) {
            close(actual, expected);
        }
    }
}

fn assert_max_insets(local: &VizMir, aligned: &VizMir, members: &[&str]) {
    let mut maxima = [0.0_f64; 4];
    for member in members {
        for (maximum, inset) in maxima.iter_mut().zip(insets(chart(local, member))) {
            *maximum = maximum.max(inset);
        }
    }
    for member in members {
        for (actual, expected) in insets(chart(aligned, member)).into_iter().zip(maxima) {
            close(actual, expected);
        }
    }
}

fn semantic_chart(chart: &MirChart) -> Value {
    let mut value = serde_json::to_value(chart).unwrap();
    for scale in value["scales"].as_array_mut().unwrap() {
        if scale["type"] == "linear" {
            scale.as_object_mut().unwrap().remove("range");
        }
    }
    value
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

fn reject_mir(value: Value) -> String {
    serde_json::from_value::<VizMir>(value)
        .map_err(|error| error.to_string())
        .and_then(|mir| build_scene(&mir).map_err(|error| error.to_string()))
        .expect_err("forged alignment must fail at decode or replay")
}

#[test]
fn mixed_numeric_charts_share_maximum_insets_without_changing_semantics_or_nonmembers() {
    for shared in [false, true] {
        let input = source(shared);
        let doc = document(input.clone());
        let before = serde_json::to_vec(&doc).unwrap();
        let local = compile(&document(without_alignment(input))).unwrap();
        let aligned = compile(&doc).unwrap();
        assert_eq!(doc.version, "0.9");
        assert_eq!(aligned.mir.version, "0.9");
        assert_eq!(aligned.mir.source_hir_version, "0.9");
        let encoded = serde_json::to_value(&aligned.mir).unwrap();
        assert_eq!(encoded["plot_alignment"]["id"], "comparison");
        assert_eq!(encoded["plot_alignment"]["mode"], "uniform");
        for (index, member) in MEMBERS.into_iter().enumerate() {
            let member_chart = chart(&aligned.mir, member);
            assert_eq!(
                encoded["plot_alignment"]["members"][index],
                json!({"view":member,"x_scale":numeric_scale(member_chart,"x").id(),
                    "y_scale":numeric_scale(member_chart,"y").id()})
            );
            assert_eq!(
                semantic_chart(member_chart),
                semantic_chart(chart(&local.mir, member))
            );
        }
        assert_ne!(
            insets(chart(&local.mir, "left")),
            insets(chart(&local.mir, "middle"))
        );
        assert_max_insets(&local.mir, &aligned.mir, &MEMBERS);
        assert_uniform(&aligned.mir, &MEMBERS);
        assert_eq!(aligned.mir.data, local.mir.data);
        assert_eq!(aligned.mir.expressions, local.mir.expressions);
        assert_eq!(aligned.mir.spaces, local.mir.spaces);
        assert_eq!(chart(&aligned.mir, "other"), chart(&local.mir, "other"));
        assert_eq!(
            find_scene_node(&aligned.scene.nodes, "other"),
            find_scene_node(&local.scene.nodes, "other")
        );
        for member in MEMBERS {
            let c = chart(&aligned.mir, member);
            let top = bounds(
                find_scene_node(&aligned.scene.nodes, &format!("{member}/grid/y/0")).unwrap(),
            )
            .y;
            close(top, range(c, "y")[1]);
            let owns_legend = !shared || member == "right";
            if !owns_legend {
                assert!(c.guides.iter().all(|guide| guide.kind != GuideKind::Legend));
            }
            assert_eq!(
                find_scene_node(&aligned.scene.nodes, &format!("{member}/legend/0/label"))
                    .is_some(),
                owns_legend
            );
        }
        assert_eq!(serde_json::to_vec(&doc).unwrap(), before);
        assert_eq!(compile(&doc).unwrap().scene, aligned.scene);
        assert_eq!(build_scene(&aligned.mir).unwrap(), aligned.scene);
    }
}

#[test]
fn membership_order_does_not_change_geometry_and_omission_keeps_independent_layout() {
    for shared in [false, true] {
        let mut input = source(shared);
        let forward = compile(&document(input.clone())).unwrap();
        input["plot_alignment"]["members"] = json!(["right", "middle", "left"]);
        let reverse = compile(&document(input.clone())).unwrap();
        assert_eq!(reverse.mir.views, forward.mir.views);
        assert_eq!(reverse.scene, forward.scene);
        let independent = compile(&document(without_alignment(input))).unwrap();
        assert!(
            serde_json::to_value(&independent.mir)
                .unwrap()
                .get("plot_alignment")
                .is_none()
        );
        assert_ne!(
            insets(chart(&independent.mir, "left")),
            insets(chart(&independent.mir, "middle"))
        );
    }
}

#[test]
fn equal_domains_align_equal_values_but_unequal_domains_remain_authored_and_unioned_by_nobody() {
    let mut input = source(false);
    input["panels"][0] = panel("scatter", "left");
    input["panels"][0]["title"] = json!("A taller header");
    let equal = compile(&document(input.clone())).unwrap();
    for key in ["a0", "a1", "b0", "b1"] {
        let mut relative = None;
        for member in ["left", "middle"] {
            let SceneNode::Circle { center, .. } =
                find_scene_node(&equal.scene.nodes, &format!("{member}/point/{key}")).unwrap()
            else {
                panic!("scatter point")
            };
            let frame = chart(&equal.mir, member).frame;
            let point = [center.x - frame.x, center.y - frame.y];
            if let Some(expected) = relative {
                for (actual, expected) in point.into_iter().zip(expected) {
                    close(actual, expected);
                }
            } else {
                relative = Some(point);
            }
        }
    }
    input["panels"][1]["x"]["domain"] = json!([-5, 20]);
    input["panels"][1]["y"]["domain"] = json!([-100, 100]);
    let unequal = compile(&document(input)).unwrap();
    assert_uniform(&unequal.mir, &MEMBERS);
    for (member, x, y) in [
        ("left", [0., 10.], [0., 10.]),
        ("middle", [-5., 20.], [-100., 100.]),
        ("right", [0., 10.], [0., 10.]),
    ] {
        for (axis, expected) in [("x", x), ("y", y)] {
            let MirScale::Linear { domain, .. } = numeric_scale(chart(&unequal.mir, member), axis)
            else {
                panic!()
            };
            assert_eq!(
                *domain, expected,
                "alignment must not infer a common numeric domain"
            );
        }
    }
    assert_eq!(unequal.mir.data, equal.mir.data);
}

#[test]
fn inferred_domains_stay_independent_when_plot_alignment_is_explicit() {
    let mut input = source(false);
    for panel in input["panels"].as_array_mut().unwrap() {
        panel["x"].as_object_mut().unwrap().remove("domain");
        panel["y"].as_object_mut().unwrap().remove("domain");
    }
    input["datasets"]["different"] = json!({"key":"id","rows":[
        {"id":"d0","x":100,"y":1000,"group":"Alpha"},
        {"id":"d1","x":200,"y":2000,"group":"Alpha"}]});
    input["panels"][1]["dataset"] = json!("different");
    let aligned = compile(&document(input.clone())).unwrap();
    let local = compile(&document(without_alignment(input))).unwrap();
    assert_uniform(&aligned.mir, &MEMBERS);
    for member in MEMBERS {
        assert_eq!(
            semantic_chart(chart(&aligned.mir, member)),
            semantic_chart(chart(&local.mir, member))
        );
    }
    let MirScale::Linear { domain: left, .. } = numeric_scale(chart(&aligned.mir, "left"), "x")
    else {
        panic!()
    };
    let MirScale::Linear { domain: middle, .. } = numeric_scale(chart(&aligned.mir, "middle"), "x")
    else {
        panic!()
    };
    assert_ne!(left, middle);
}

#[test]
fn fractional_cells_origins_and_canvas_roundtrip_without_geometry_drift() {
    let mut input = source(true);
    input["width"] = json!(2448.375);
    input["height"] = json!(680.625);
    input["layout"]["padding"] = json!(23.125);
    input["layout"]["gap"] = json!(17.375);
    input["shared_legend"]["height"] = json!(70.25);
    input["shared_legend"]["gap"] = json!(18.125);
    let compiled = compile(&document(input)).unwrap();
    assert_uniform(&compiled.mir, &MEMBERS);
    let restored: VizMir =
        serde_json::from_slice(&serde_json::to_vec(&compiled.mir).unwrap()).unwrap();
    assert_eq!(restored, compiled.mir);
    assert_eq!(build_scene(&restored).unwrap(), compiled.scene);
    assert_eq!(rematerialize_mir(&restored).unwrap(), restored);
}

fn replace_string(value: &mut Value, old: &str, new: &str) {
    match value {
        Value::String(value) if value == old => *value = new.to_owned(),
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
fn explicit_refs_support_nonconventional_scale_ids_instead_of_suffix_guessing() {
    let original = compile(&document(source(true))).unwrap();
    let mut value = serde_json::to_value(&original.mir).unwrap();
    for member in MEMBERS {
        replace_string(
            &mut value,
            &format!("{member}/x"),
            &format!("position/{member}/horizontal"),
        );
        replace_string(
            &mut value,
            &format!("{member}/y"),
            &format!("position/{member}/vertical"),
        );
    }
    let renamed: VizMir = serde_json::from_value(value).unwrap();
    assert_uniform(&renamed, &MEMBERS);
    let mut expected = serde_json::to_string(&original.scene).unwrap();
    for member in MEMBERS {
        // Mark provenance names its explicit scales; geometry and every other
        // semantic field must remain exactly identical after those IDs change.
        expected = expected.replace(
            &format!("{member}/x"),
            &format!("position/{member}/horizontal"),
        );
        expected = expected.replace(
            &format!("{member}/y"),
            &format!("position/{member}/vertical"),
        );
    }
    assert_eq!(
        serde_json::to_value(build_scene(&renamed).unwrap()).unwrap(),
        serde_json::from_str::<Value>(&expected).unwrap()
    );
    assert_eq!(rematerialize_mir(&renamed).unwrap(), renamed);
}

#[test]
fn forged_missing_cross_view_swapped_and_unbound_scale_references_fail_closed() {
    let value = serde_json::to_value(compile(&document(source(false))).unwrap().mir).unwrap();
    for case in [
        "missing-view",
        "missing-x",
        "missing-y",
        "other-view",
        "swapped",
        "color",
        "unbound",
        "duplicate",
        "single",
        "missing-ref",
        "unknown-field",
    ] {
        let mut bad = value.clone();
        match case {
            "missing-view" => bad["plot_alignment"]["members"][0]["view"] = json!("missing"),
            "missing-x" => bad["plot_alignment"]["members"][0]["x_scale"] = json!("missing"),
            "missing-y" => bad["plot_alignment"]["members"][0]["y_scale"] = json!("missing"),
            "other-view" => bad["plot_alignment"]["members"][0]["x_scale"] = json!("middle/x"),
            "swapped" => {
                bad["plot_alignment"]["members"][0]["x_scale"] = json!("left/y");
                bad["plot_alignment"]["members"][0]["y_scale"] = json!("left/x");
            }
            "color" => bad["plot_alignment"]["members"][0]["x_scale"] = json!("left/color"),
            "unbound" => {
                let mut decoy = bad["views"][0]["scales"][0].clone();
                decoy["id"] = json!("unused-linear");
                bad["views"][0]["scales"]
                    .as_array_mut()
                    .unwrap()
                    .push(decoy);
                bad["plot_alignment"]["members"][0]["x_scale"] = json!("unused-linear");
            }
            "duplicate" => {
                bad["plot_alignment"]["members"][1] = bad["plot_alignment"]["members"][0].clone()
            }
            "single" => bad["plot_alignment"]["members"]
                .as_array_mut()
                .unwrap()
                .truncate(1),
            "missing-ref" => {
                bad["plot_alignment"]["members"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("x_scale");
            }
            _ => bad["plot_alignment"]["members"][0]["range"] = json!([0, 100]),
        }
        assert!(!reject_mir(bad).is_empty(), "{case}");
    }
}

#[test]
fn replay_recomputes_all_geometry_instead_of_trusting_self_consistent_forged_ranges() {
    let compiled = compile(&document(source(false))).unwrap();
    let original = serde_json::to_value(&compiled.mir).unwrap();
    for case in [
        "x-start",
        "x-end",
        "y-start",
        "y-end",
        "all-members",
        "format",
        "header",
        "remove-owner",
    ] {
        let mut bad = original.clone();
        match case {
            "x-start" | "x-end" | "y-start" | "y-end" => {
                let scale = if case.starts_with('x') { 0 } else { 1 };
                let endpoint = usize::from(case.ends_with("end"));
                let old = bad["views"][0]["scales"][scale]["range"][endpoint]
                    .as_f64()
                    .unwrap();
                bad["views"][0]["scales"][scale]["range"][endpoint] = json!(old + 1.0);
            }
            "all-members" => {
                // Equal but forged geometry must not be accepted just because members agree.
                for view in bad["views"].as_array_mut().unwrap().iter_mut().take(3) {
                    for scale in view["scales"].as_array_mut().unwrap().iter_mut().take(2) {
                        for endpoint in scale["range"].as_array_mut().unwrap() {
                            *endpoint = json!(endpoint.as_f64().unwrap() + 1.0);
                        }
                    }
                }
            }
            "format" => bad["views"][0]["guides"][1]["number_format"]["precision"] = json!(10),
            "header" => bad["views"][0]["title"] = Value::Null,
            _ => {
                bad.as_object_mut().unwrap().remove("plot_alignment");
            }
        }
        assert!(!reject_mir(bad.clone()).is_empty(), "{case}");
        if let Ok(mir) = serde_json::from_value::<VizMir>(bad) {
            let refreshed = rematerialize_mir(&mir).unwrap();
            assert_eq!(
                refreshed, mir,
                "bare refresh preserves all explicit geometry: {case}"
            );
            assert!(
                build_scene(&refreshed).is_err(),
                "refresh cannot make stale geometry executable: {case}"
            );
        }
    }
}

#[test]
fn refresh_preserves_alignment_ranges_and_semantics_but_rebuilds_only_stale_marks() {
    let compiled = compile(&document(source(true))).unwrap();
    let mut edited = compiled.mir.clone();
    let MirDataOperator::Inline { rows } =
        &mut edited.data.get_mut("data/samples").unwrap().operator;
    rows[0].insert("y".into(), json!(3));
    assert!(build_scene(&edited).is_err());
    let refreshed = rematerialize_mir(&edited).unwrap();
    assert_eq!(refreshed.plot_alignment, compiled.mir.plot_alignment);
    for member in ["left", "middle", "right", "other"] {
        assert_eq!(
            chart(&refreshed, member).scales,
            chart(&compiled.mir, member).scales
        );
        assert_ne!(
            chart(&refreshed, member).mark,
            chart(&compiled.mir, member).mark
        );
    }
    assert!(build_scene(&refreshed).is_ok());
    let mut stale = edited;
    let MirScale::Linear { range, .. } = &mut chart_mut(&mut stale, "left").scales[0] else {
        panic!()
    };
    range[0] += 1.;
    let before = serde_json::to_vec(&stale).unwrap();
    let refreshed_stale = rematerialize_mir(&stale).unwrap();
    assert_eq!(refreshed_stale.plot_alignment, stale.plot_alignment);
    for member in MEMBERS {
        assert_eq!(
            chart(&refreshed_stale, member).scales,
            chart(&stale, member).scales
        );
    }
    assert!(build_scene(&refreshed_stale).is_err());
    assert_eq!(
        serde_json::to_vec(&stale).unwrap(),
        before,
        "refresh never mutates its input"
    );
}

fn aggregate_source() -> Value {
    let mut value = source(false);
    value["width"] = json!(1128);
    value["height"] = json!(400);
    value["layout"] = json!({"kind":"grid","columns":2,"padding":16,"gap":16});
    value["panels"] = json!([panel("scatter", "left"), panel("scatter", "middle")]);
    for panel in value["panels"].as_array_mut().unwrap() {
        panel.as_object_mut().unwrap().remove("color");
    }
    value["panels"][0]["y"]["axis"] = json!({"number_format":{"notation":"fixed","precision":12}});
    value["plot_alignment"]["members"] = json!(["left", "middle"]);
    value
}

#[test]
fn locally_valid_charts_reject_when_aggregate_insets_break_final_numeric_tick_fit() {
    let mut input = aggregate_source();
    input["panels"][1]["x"]["axis"] = json!({"number_format":{"notation":"fixed","precision":6}});
    let local = compile(&document(without_alignment(input.clone()))).unwrap();
    assert_eq!(chart(&local.mir, "left").frame.width, 540.);
    let left = insets(chart(&local.mir, "left"));
    let middle = insets(chart(&local.mir, "middle"));
    assert!(left[0] > middle[0] && middle[2] > left[2]);
    let error = compile(&document(input)).unwrap_err().to_string();
    assert!(
        error.contains("VIZ-ALIGN") && error.contains("plot"),
        "{error}"
    );
}

#[test]
fn locally_valid_x_title_is_rechecked_after_asymmetric_uniform_insets() {
    let mut input = aggregate_source();
    input["panels"][1]["x"]["label"] = json!("W".repeat(31));
    let local = compile(&document(without_alignment(input.clone()))).unwrap();
    assert!(insets(chart(&local.mir, "left"))[0] > insets(chart(&local.mir, "middle"))[0]);
    let error = compile(&document(input)).unwrap_err().to_string();
    assert!(
        error.contains("VIZ-ALIGN") && error.contains("x-axis title"),
        "{error}"
    );
}

#[test]
fn measured_wrapped_latin_cjk_titles_persist_context_and_align_after_legend_ownership() {
    let title = "中文（字体测量），保留标点。\nAV office ffi Café Å A\u{30a}";
    let (context, resources) = fixtures::profile();
    let context =
        context.with_text_layout(TextLayoutContext::new(vec![]).with_semantic_targets(vec![
            SemanticTextLayoutTarget::chart_title("left", 180., 12, 30.),
        ]));
    for shared in [false, true] {
        let mut input = source(shared);
        input["panels"][0]["title"] = json!(title);
        input["panels"][0]["y"]["label"] = json!("Temperature 服务");
        let local = compile_with_context(
            &document(without_alignment(input.clone())),
            &context,
            &resources,
        )
        .unwrap();
        let compiled = compile_with_context(&document(input), &context, &resources).unwrap();
        assert_max_insets(&local.mir.mir, &compiled.mir.mir, &MEMBERS);
        assert_uniform(&compiled.mir.mir, &MEMBERS);
        let node = find_scene_node(&compiled.scene.nodes, "left/title").unwrap();
        assert!(matches!(node, SceneNode::Path { .. }));
        assert!(node.origin().explanation.ends_with(title));
        assert!(bounds(node).height > 30., "fixture must actually wrap");
        assert!(
            bounds(node).y + bounds(node).height < range(chart(&compiled.mir.mir, "left"), "y")[1]
        );
        assert_eq!(
            find_scene_node(&local.scene.nodes, "other"),
            find_scene_node(&compiled.scene.nodes, "other")
        );
        let persisted = serde_json::to_vec(&compiled.mir).unwrap();
        let restored = parse_compiled_mir_json(&persisted).unwrap();
        let (_, copied_resources) = fixtures::profile();
        assert_eq!(
            build_compiled_scene(&restored, &copied_resources).unwrap(),
            compiled.scene
        );
        assert_eq!(restored.context, context);
        assert_eq!(restored, compiled.mir);
        assert_eq!(
            build_compiled_scene(&restored, &resources).unwrap(),
            compiled.scene
        );
        assert_eq!(
            rematerialize_compiled_mir(&restored, &resources).unwrap(),
            restored
        );
        assert!(build_compiled_scene(&restored, &FontResources::new()).is_err());
        let mut stale = restored.clone();
        let MirScale::Linear { range, .. } = &mut chart_mut(&mut stale.mir, "middle").scales[1]
        else {
            panic!()
        };
        range[1] += 1.;
        assert!(build_compiled_scene(&stale, &resources).is_err());
        assert!(rematerialize_compiled_mir(&stale, &resources).is_err());
        let mut changed_context = restored;
        changed_context.context.text_layout = None;
        assert!(
            build_compiled_scene(&changed_context, &resources).is_err(),
            "persisted ranges cannot stand in for missing wrapping context"
        );
    }
}

#[test]
fn themes_replay_the_same_uniform_contract_without_mutating_domain_or_data() {
    let doc = document(source(true));
    for theme in ["azure", "sage-dark"] {
        let compiled = compile_with_theme(&doc, theme).unwrap();
        assert_uniform(&compiled.mir.mir, &MEMBERS);
        assert_eq!(build_themed_scene(&compiled.mir).unwrap(), compiled.scene);
        assert_eq!(compiled.mir.mir.data, compile(&doc).unwrap().mir.data);
    }
}

#[test]
fn alignment_preflight_limits_win_before_member_lookup_and_text_shaping() {
    let doc = document(source(false));
    let compiled = compile(&doc).unwrap();
    let limits = MaterializationLimits {
        max_evaluation_steps: 0,
        ..MaterializationLimits::default()
    };
    let mut invalid = compiled.mir.clone();
    invalid.plot_alignment.as_mut().unwrap().members[0].x_scale = "missing".into();
    let mut missing = doc.clone();
    missing.plot_alignment.as_mut().unwrap().members[0] = "missing".into();
    for error in [
        build_scene_with_limits(&invalid, limits)
            .unwrap_err()
            .to_string(),
        rematerialize_mir_with_limits(&invalid, limits)
            .unwrap_err()
            .to_string(),
        compile_with_context_with_limits(
            &missing,
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
    let (context, resources) = fixtures::profile();
    let mut unsupported = source(false);
    unsupported["panels"][0]["title"] = json!("Missing glyph 😀");
    let error = compile_with_context_with_limits(
        &document(unsupported),
        &context,
        &resources,
        limits,
        TextLimits::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    let measured = compile_with_context(&doc, &context, &resources).unwrap();
    let error =
        build_compiled_scene_with_limits(&measured.mir, &resources, limits, TextLimits::default())
            .unwrap_err()
            .to_string();
    assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    let mut text_limits = TextLimits::default();
    text_limits.max_labels = 0;
    let error = compile_with_context_with_limits(
        &doc,
        &context,
        &resources,
        MaterializationLimits::default(),
        text_limits,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("VIZ-TEXT-0003"), "{error}");
}
