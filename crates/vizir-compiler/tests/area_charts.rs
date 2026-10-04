#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    MaterializationLimits, SemanticTextLayoutTarget, THEME_NAMES, TextLayoutContext, ThemeContext,
    build_compiled_scene, build_scene, compile, compile_with_context, compile_with_theme,
    parse_compiled_mir_json, rematerialize_compiled_mir, rematerialize_mir,
    rematerialize_mir_with_limits,
};
use vizir_core::{
    ChartMark, Document, GuideKind, GuideOrient, MirChart, MirDataOperator, MirScale, MirView,
    PathCommand, Revision, SceneNode, View, VizMir, apply_scene_patch, diff_scene, find_scene_node,
    map_linear,
};
fn document(rows: Value, baseline: f64, grouped: bool) -> Document {
    let mut view = json!({"kind":"chart.area","id":"a","dataset":"d","title":"Area",
        "frame":{"x":20,"y":20,"width":900,"height":550},
        "x":{"field":"x"},"y":{"field":"y"},"baseline":baseline,"order":"x-ascending"});
    if grouped {
        view["series"] = json!({"field":"g"});
    }
    serde_json::from_value(
        json!({"version":"0.3","id":"areas","width":980,"height":650,
        "datasets":{"d":{"key":"id","rows":rows}},"views":[view]}),
    )
    .unwrap()
}
fn simple() -> Document {
    document(
        json!([{"id":"right","x":2,"y":4},{"id":"left","x":0,"y":-2},{"id":"middle","x":1,"y":3}]),
        1.0,
        false,
    )
}
fn chart(mir: &VizMir) -> &MirChart {
    let MirView::Chart(c) = &mir.views[0] else {
        panic!()
    };
    c
}
fn chart_mut(mir: &mut VizMir) -> &mut MirChart {
    let MirView::Chart(c) = &mut mir.views[0] else {
        panic!()
    };
    c
}
fn area(mir: &VizMir) -> &[vizir_core::MirSeries] {
    let ChartMark::Area { series, .. } = &chart(mir).mark else {
        panic!()
    };
    series
}
fn rows_mut(mir: &mut VizMir) -> &mut Vec<std::collections::BTreeMap<String, Value>> {
    let MirDataOperator::Inline { rows } = &mut mir.data.get_mut("data/d").unwrap().operator;
    rows
}
fn domain(mir: &VizMir, suffix: &str) -> ([f64; 2], [f64; 2]) {
    let MirScale::Linear { domain, range, .. } = chart(mir)
        .scales
        .iter()
        .find(|s| s.id().ends_with(suffix))
        .unwrap()
    else {
        panic!()
    };
    (*domain, *range)
}
#[test]
fn source_rows_keys_and_explicit_baseline_are_preserved_through_normal_pipeline() {
    let d = simple();
    let c = compile(&d).unwrap();
    assert_eq!(
        (c.mir.version.as_str(), c.mir.source_hir_version.as_str()),
        ("0.3", "0.3")
    );
    let MirDataOperator::Inline { rows } = &c.mir.data["data/d"].operator;
    assert_eq!(rows, &d.datasets["d"].rows);
    assert_eq!(
        area(&c.mir)[0]
            .points
            .iter()
            .map(|p| p.key.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "middle", "right"]
    );
    let (yd, yr) = domain(&c.mir, "/y");
    assert!(yd[0] <= -2.0 && yd[1] >= 4.0);
    let SceneNode::Path {
        commands,
        style,
        origin,
        marker_end,
        ..
    } = find_scene_node(&c.scene.nodes, "a/area/series").unwrap()
    else {
        panic!()
    };
    assert_eq!(style.opacity, 0.35);
    assert_eq!(style.stroke.0, "transparent");
    assert!(!marker_end);
    assert_eq!(origin.data_lineage, vec!["data/d"]);
    assert_eq!(origin.mir_node, "a/marks/areas");
    assert!(matches!(commands.last(), Some(PathCommand::Close)));
    let PathCommand::Move { to } = commands[0] else {
        panic!()
    };
    assert_eq!(to.y.to_bits(), map_linear(1.0, yd, yr).to_bits());
    assert_eq!(build_scene(&c.mir).unwrap(), c.scene);
    let replay: VizMir = serde_json::from_slice(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(build_scene(&replay).unwrap(), c.scene);
}
#[test]
fn grouped_areas_paint_in_sorted_series_order_with_explicit_real_legend() {
    let d = document(
        json!([{"id":"z2","x":2,"y":3,"g":"Z"},{"id":"a1","x":0,"y":-1,"g":"A"},{"id":"z1","x":0,"y":2,"g":"Z"},{"id":"a2","x":2,"y":4,"g":"A"}]),
        0.0,
        true,
    );
    let c = compile(&d).unwrap();
    assert_eq!(
        area(&c.mir)
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "Z"]
    );
    let guides = &chart(&c.mir).guides;
    assert!(guides.iter().any(|g| g.kind == GuideKind::Legend
        && g.orient == GuideOrient::Right
        && g.scale == "a/color"));
    let SceneNode::Group { children, .. } = &c.scene.nodes[0] else {
        panic!()
    };
    assert_eq!(
        children
            .iter()
            .filter(|n| n.id().starts_with("a/area/"))
            .map(|n| n.id())
            .collect::<Vec<_>>(),
        vec!["a/area/A", "a/area/Z"]
    );
    assert!(find_scene_node(&c.scene.nodes, "a/legend/0/label").is_some());
    let mut reordered = d.clone();
    reordered.datasets.get_mut("d").unwrap().rows.reverse();
    assert_eq!(compile(&reordered).unwrap().scene, c.scene);
}
#[test]
fn cardinality_duplicates_signed_zero_and_precision_collisions_are_errors() {
    for rows in [
        json!([{"id":"a","x":0,"y":1}]),
        json!([{"id":"a","x":0,"y":1},{"id":"b","x":0,"y":2}]),
        json!([{"id":"a","x":-0.0,"y":1},{"id":"b","x":0.0,"y":2}]),
        json!([{"id":"a","x":9007199254740992_i64,"y":1},{"id":"b","x":9007199254740993_i64,"y":2}]),
        json!([{"id":"a","x":i64::MAX-1,"y":1},{"id":"b","x":i64::MAX,"y":2}]),
        json!([{"id":"a","x":u64::MAX-1,"y":1},{"id":"b","x":u64::MAX,"y":2}]),
        json!([{"id":"a","x":9007199254740992_i64,"y":1},{"id":"b","x":9007199254740993_i64,"y":2},{"id":"c","x":1.5,"y":3}]),
    ] {
        assert!(compile(&document(rows, 0.0, false)).is_err());
    }
    let one_short = document(
        json!([{"id":"a","x":0,"y":1,"g":"A"},{"id":"b","x":2,"y":2,"g":"A"},{"id":"c","x":1,"y":3,"g":"B"}]),
        0.0,
        true,
    );
    assert!(compile(&one_short).is_err());
}
#[test]
fn source_projection_contract_allows_isolated_rounded_integers_with_distinct_coordinates() {
    let d = document(
        json!([{"id":"a","x":9007199254740993_i64,"y":1},{"id":"b","x":9007199254740996_i64,"y":2}]),
        0.0,
        false,
    );
    let c = compile(&d).unwrap();
    assert!(area(&c.mir)[0].points[0].x < area(&c.mir)[0].points[1].x);
    assert_eq!(
        c.mir.data["data/d"].operator,
        MirDataOperator::Inline {
            rows: d.datasets["d"].rows.clone()
        }
    );
}
#[test]
fn smallest_subnormal_domains_retain_distinct_positions_and_exact_baseline_bits() {
    let tiny = f64::from_bits(1);
    for baseline in [-tiny, -0.0, 0.0, tiny] {
        let c = compile(&document(
            json!([{"id":"a","x":0.0,"y":0.0},{"id":"b","x":tiny,"y":tiny}]),
            baseline,
            false,
        ))
        .unwrap();
        let ChartMark::Area { baseline: b, .. } = chart(&c.mir).mark else {
            panic!()
        };
        assert_eq!(b.to_bits(), baseline.to_bits());
        let (domain, _) = domain(&c.mir, "/y");
        assert!(domain[0] <= baseline && domain[1] >= baseline);
        let SceneNode::Path { commands, .. } =
            find_scene_node(&c.scene.nodes, "a/area/series").unwrap()
        else {
            panic!()
        };
        let PathCommand::Line { to: a } = commands[1] else {
            panic!()
        };
        let PathCommand::Line { to: b } = commands[2] else {
            panic!()
        };
        assert!(a.x < b.x);
        let re: VizMir = serde_json::from_slice(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
        assert_eq!(build_scene(&re).unwrap(), c.scene);
    }
}
#[test]
fn baseline_domain_inclusion_handles_negative_crossing_and_zero_height_areas() {
    for (base, ys) in [
        (5.0, [8.0, 10.0]),
        (-8.0, [-3.0, -6.0]),
        (2.0, [-2.0, 7.0]),
        (3.0, [3.0, 3.0]),
    ] {
        let c = compile(&document(
            json!([{"id":"a","x":0,"y":ys[0]},{"id":"b","x":1,"y":ys[1]}]),
            base,
            false,
        ))
        .unwrap();
        let (d, _) = domain(&c.mir, "/y");
        assert!(d[0] <= base && d[1] >= base);
        let SceneNode::Path { bounds, .. } =
            find_scene_node(&c.scene.nodes, "a/area/series").unwrap()
        else {
            panic!()
        };
        assert!(bounds.width > 0.0);
        if ys == [base, base] {
            assert_eq!(bounds.height, 0.0);
        }
    }
}
#[test]
fn nonfinite_combined_baseline_span_and_referenced_nulls_never_materialize() {
    assert!(
        compile(&document(
            json!([{"id":"a","x":0,"y":-1e308},{"id":"b","x":1,"y":-9e307}]),
            1e308,
            false
        ))
        .is_err()
    );
    for field in ["x", "y", "id"] {
        let mut d = simple();
        d.datasets.get_mut("d").unwrap().rows[0].insert(field.into(), Value::Null);
        assert!(compile(&d).is_err());
    }
    let mut d = simple();
    if let View::Area(a) = &mut d.views[0] {
        a.baseline = f64::INFINITY;
    }
    assert!(compile(&d).is_err());
}
#[test]
fn stale_caches_are_assertions_and_refresh_preserves_explicit_domains_and_plan() {
    let original = compile(&simple()).unwrap().mir;
    for which in 0..5 {
        let mut m = original.clone();
        let ChartMark::Area { series, .. } = &mut chart_mut(&mut m).mark else {
            panic!()
        };
        match which {
            0 => series[0].points[0].y = f64::from_bits(series[0].points[0].y.to_bits() + 1),
            1 => series[0].points.reverse(),
            2 => series[0].points[0].key = "wrong".into(),
            3 => series[0].points[0].color_category = Some("wrong".into()),
            _ => {
                series[0].points.pop();
            }
        }
        assert!(
            build_scene(&m)
                .unwrap_err()
                .to_string()
                .contains("VIZ-MATERIALIZE-0002")
        );
        assert_eq!(rematerialize_mir(&m).unwrap(), original);
    }
    let mut edited = original.clone();
    rows_mut(&mut edited)[0].insert("y".into(), json!(3));
    assert!(build_scene(&edited).is_err());
    let refreshed = rematerialize_mir(&edited).unwrap();
    assert_eq!(chart(&refreshed).scales, chart(&original).scales);
    assert_eq!(refreshed, rematerialize_mir(&refreshed).unwrap());
    assert_ne!(
        build_scene(&refreshed).unwrap(),
        build_scene(&original).unwrap()
    );
}
#[test]
fn explicit_mir_extrapolation_is_preserved_and_projected_x_collisions_are_diagnosed() {
    let mut m = compile(&simple()).unwrap().mir;
    let scales = chart(&m).scales.clone();
    if let ChartMark::Area { baseline, .. } = &mut chart_mut(&mut m).mark {
        *baseline = 100.0;
    }
    let r = rematerialize_mir(&m).unwrap();
    assert_eq!(chart(&r).scales, scales);
    build_scene(&r).unwrap();
    let mut collapsed = m;
    for s in &mut chart_mut(&mut collapsed).scales {
        if let MirScale::Linear { id, domain, .. } = s
            && id == "a/x"
        {
            *domain = [-1e300, 1e300];
        }
    }
    assert!(
        build_scene(&collapsed)
            .unwrap_err()
            .to_string()
            .contains("strictly increasing projected x")
    );
}
#[test]
fn area_requires_explicit_guides_and_honors_scale_renaming() {
    let m = compile(&simple()).unwrap().mir;
    for index in [0, 1] {
        let mut bad = m.clone();
        chart_mut(&mut bad).guides.remove(index);
        assert!(build_scene(&bad).is_err());
    }
    let mut renamed = m;
    let c = chart_mut(&mut renamed);
    for s in &mut c.scales {
        if let MirScale::Linear { id, .. } = s {
            *id = format!("renamed-{id}");
        }
    }
    for g in &mut c.guides {
        g.scale = format!("renamed-{}", g.scale);
    }
    if let ChartMark::Area { x, y, .. } = &mut c.mark {
        x.scale = format!("renamed-{}", x.scale);
        y.scale = format!("renamed-{}", y.scale);
    }
    build_scene(&renamed).unwrap();
    let mut bad = renamed;
    if let ChartMark::Area {
        order_expression,
        y,
        ..
    } = &mut chart_mut(&mut bad).mark
    {
        *order_expression = y.expression.clone();
    }
    assert!(build_scene(&bad).is_err());
}
#[test]
fn semantic_paths_and_patch_roundtrip_survive_data_updates_insertion_and_removal() {
    let d = simple();
    let old = compile(&d).unwrap().scene;
    let mut changed = d.clone();
    changed.datasets.get_mut("d").unwrap().rows[0].insert("y".into(), json!(3));
    changed
        .datasets
        .get_mut("d")
        .unwrap()
        .rows
        .push(serde_json::from_value(json!({"id":"new","x":3,"y":2})).unwrap());
    changed.datasets.get_mut("d").unwrap().rows.remove(1);
    let new = compile(&changed).unwrap().scene;
    assert!(find_scene_node(&new.nodes, "a/area/series").is_some());
    let patch = diff_scene(&old, &new, Revision(1), Revision(2), "area-update").unwrap();
    assert_eq!(
        apply_scene_patch(&old, Revision(1), &patch).unwrap(),
        (new, Revision(2))
    );
}
#[test]
fn canonical_themes_do_not_paint_the_canvas_and_all_use_semantic_area_fill() {
    for name in THEME_NAMES {
        let c = compile_with_theme(&simple(), name).unwrap();
        assert_eq!(c.scene.background.0, "transparent");
        let SceneNode::Path { style, .. } =
            find_scene_node(&c.scene.nodes, "a/area/series").unwrap()
        else {
            panic!()
        };
        assert_eq!(style.fill, c.mir.theme.defaults.mark);
        assert_eq!(style.opacity, 0.35);
    }
}
#[test]
fn area_uses_existing_title_service_only_under_new_source_version_and_replays() {
    let (base, res) = fixtures::profile();
    let mut d = simple();
    if let View::Area(a) = &mut d.views[0] {
        a.title = Some("Area title with several words and complete source".into());
    }
    let layout = TextLayoutContext::new(vec![]).with_semantic_targets(vec![
        SemanticTextLayoutTarget::chart_title("a", 180.0, 8, 30.0),
    ]);
    let ctx = base
        .with_theme(ThemeContext::resolve("azure").unwrap())
        .with_text_layout(layout);
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(c.mir.format, "vizir-compiled-mir/1");
    assert!(matches!(
        find_scene_node(&c.scene.nodes, "a/title"),
        Some(SceneNode::Path { .. })
    ));
    let replay = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(build_compiled_scene(&replay, &res).unwrap(), c.scene);
    assert_eq!(rematerialize_compiled_mir(&replay, &res).unwrap(), replay);
    for version in ["0.1", "0.2"] {
        let mut old = d.clone();
        old.version = version.into();
        assert!(compile_with_context(&old, &ctx, &res).is_err());
        let mut bad = c.mir.clone();
        bad.mir.version = version.into();
        bad.mir.source_hir_version = version.into();
        assert!(build_compiled_scene(&bad, &res).is_err());
    }
    let mut narrow = ctx;
    narrow
        .text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .max_width = 0.25;
    assert!(compile_with_context(&d, &narrow, &res).is_err());
}
#[test]
fn limits_and_empty_explicit_mir_sources_fail_without_partial_refresh() {
    let m = compile(&simple()).unwrap().mir;
    let saved = m.clone();
    let limits = MaterializationLimits {
        max_evaluation_steps: 1,
        ..MaterializationLimits::default()
    };
    assert!(rematerialize_mir_with_limits(&m, limits).is_err());
    assert_eq!(m, saved);
    let mut empty = m;
    rows_mut(&mut empty).clear();
    assert!(rematerialize_mir(&empty).is_err());
}

#[test]
fn area_title_support_in_mixed_wrap_three_and_four_keeps_existing_profile_shapes() {
    let (base, resources) = fixtures::profile();
    let mut value = serde_json::to_value(simple()).unwrap();
    value["width"] = json!(1540);
    value["height"] = json!(1080);
    value["views"][0]["title"] = json!("Area title with several words and complete source");
    value["views"].as_array_mut().unwrap().extend([
        json!({"kind":"chart.bar","id":"bar","dataset":"d","frame":{"x":960,"y":20,"width":540,"height":550},"category":{"field":"id"},"value":{"field":"y"}}),
        json!({"kind":"diagram.graph","id":"diagram","frame":{"x":20,"y":620,"width":900,"height":420},"nodes":[{"id":"node","label":"Diagram label"}]}),
    ]);
    let d: Document = serde_json::from_value(value).unwrap();
    let title = SemanticTextLayoutTarget::chart_title("a", 180., 8, 30.);
    let profiles = [
        TextLayoutContext::new(vec![]).with_category_labels(vec![
            title.clone(),
            SemanticTextLayoutTarget::bar_category_labels("bar", 130., 5, 18.),
        ]),
        TextLayoutContext::new(vec![])
            .with_semantic_targets(vec![title])
            .with_diagram_targets(vec![vizir_compiler::DiagramTextLayoutTarget::new(
                "diagram", "node", 70., 5, 22.,
            )]),
    ];
    for layout in profiles {
        let profile = layout.profile.clone();
        let ctx = base.clone().with_text_layout(layout);
        let result = compile_with_context(&d, &ctx, &resources).unwrap();
        assert_eq!(
            result.mir.context.text_layout.as_ref().unwrap().profile,
            profile
        );
        assert!(matches!(
            find_scene_node(&result.scene.nodes, "a/title"),
            Some(SceneNode::Path { .. })
        ));
        assert_eq!(
            build_compiled_scene(&result.mir, &resources).unwrap(),
            result.scene
        );
        let mut downgraded = d.clone();
        downgraded.version = "0.2".into();
        assert!(compile_with_context(&downgraded, &ctx, &resources).is_err());
    }
}

#[test]
fn standalone_lower_and_refresh_reject_invalid_domains_and_projection_collapse() {
    let d = document(
        json!([
            {"id":"a1","x":0.,"y":1,"g":"A"},{"id":"a2","x":1.,"y":2,"g":"A"},
            {"id":"z1","x":1e300,"y":1,"g":"Z"},{"id":"z2","x":2e300,"y":2,"g":"Z"}
        ]),
        0.,
        true,
    );
    assert!(vizir_compiler::lower_to_mir(&d).is_err());
    for bad in [f64::INFINITY, f64::NAN] {
        let mut m = compile(&simple()).unwrap().mir;
        for scale in &mut chart_mut(&mut m).scales {
            if let MirScale::Linear { id, domain, .. } = scale
                && id == "a/y"
            {
                domain[1] = bad;
            }
        }
        assert!(rematerialize_mir(&m).is_err());
    }
    // Nice expansion near Float64::MAX must be diagnosed, not emitted as null in MIR JSON.
    let huge = document(
        json!([{"id":"a","x":0.,"y":f64::MAX},{"id":"b","x":1.,"y":f64::MAX}]),
        f64::MAX,
        false,
    );
    assert!(vizir_compiler::lower_to_mir(&huge).is_err());
}
