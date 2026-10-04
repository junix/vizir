#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    MaterializationLimits, SemanticTextLayoutTarget, THEME_NAMES, TextLayoutContext, TextLimits,
    ThemeContext, build_compiled_scene, build_scene, compile, compile_with_context,
    compile_with_context_with_limits, compile_with_theme, parse_compiled_mir_json,
};
use vizir_core::{
    ChartMark, Document, MirChart, MirScale, MirView, Rect, Revision, SceneNode, View, VizMir,
    apply_scene_patch, diff_scene, find_scene_node,
};
fn document(rows: Value) -> Document {
    serde_json::from_value(json!({"version":"0.4","id":"heatmap-document","width":1100,"height":700,
        "datasets":{"d":{"key":"id","rows":rows}},"views":[{"kind":"chart.heatmap","id":"h","title":"Heatmap",
        "frame":{"x":20,"y":20,"width":1000,"height":640},"dataset":"d",
        "x":{"field":"x"},"y":{"field":"y"},"color":{"field":"v","domain":[0,10],"palette":["#112233","#aabbcc"]}}]})).unwrap()
}
fn simple() -> Document {
    document(json!([
        {"id":"b2","x":"B","y":"second","v":0},
        {"id":"a1","x":"A","y":"first","v":10},
        {"id":"b1","x":"B","y":"first","v":5}
    ]))
}
fn chart(mir: &VizMir) -> &MirChart {
    let MirView::Chart(c) = &mir.views[0] else {
        panic!()
    };
    c
}
fn heatmap(d: &mut Document) -> &mut vizir_core::HeatmapChart {
    let View::Heatmap(c) = &mut d.views[0] else {
        panic!()
    };
    c
}
fn rect(scene: &vizir_core::Scene2D, id: &str) -> Rect {
    let SceneNode::Rect { bounds, .. } = find_scene_node(&scene.nodes, id).unwrap() else {
        panic!()
    };
    *bounds
}
fn fill(scene: &vizir_core::Scene2D, id: &str) -> String {
    let SceneNode::Rect { style, .. } = find_scene_node(&scene.nodes, id).unwrap() else {
        panic!()
    };
    style.fill.0.clone()
}
fn text(scene: &vizir_core::Scene2D, id: &str) -> String {
    let SceneNode::Text { text, .. } = find_scene_node(&scene.nodes, id).unwrap() else {
        panic!()
    };
    text.clone()
}
#[test]
fn first_occurrence_domains_put_first_y_at_top_and_keep_zero_as_a_cell() {
    let d = simple();
    let c = compile(&d).unwrap();
    assert_eq!(c.scene.background.0, "transparent");
    let zero = rect(&c.scene, "h/cell/b2");
    let lower = rect(&c.scene, "h/cell/b1");
    let right = rect(&c.scene, "h/cell/a1");
    assert!(zero.y < lower.y);
    assert!(zero.x < right.x);
    assert_eq!(zero.x, lower.x);
    assert_eq!(zero.width, lower.width);
    assert_eq!(text(&c.scene, "h/axis/x/category/0"), "B");
    assert_eq!(text(&c.scene, "h/axis/y/category/0"), "second");
    assert_eq!(fill(&c.scene, "h/cell/b2"), "#112233");
    assert!(find_scene_node(&c.scene.nodes, "h/cell/a2").is_none());
    let ChartMark::Heatmap { instances, .. } = &chart(&c.mir).mark else {
        panic!()
    };
    assert_eq!(
        instances.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
        vec!["b2", "a1", "b1"]
    );
    for key in ["b2", "a1", "b1"] {
        let node = find_scene_node(&c.scene.nodes, &format!("h/cell/{key}")).unwrap();
        assert_eq!(node.origin().data_key.as_deref(), Some(key));
    }
}
#[test]
fn explicit_domain_permutations_and_empty_bands_preserve_cell_ids_and_patch() {
    let d = simple();
    let old = compile(&d).unwrap().scene;
    let mut reordered = d.clone();
    let h = heatmap(&mut reordered);
    h.x.domain = Some(vec!["A".into(), "empty".into(), "B".into()]);
    h.y.domain = Some(vec!["first".into(), "second".into()]);
    let new = compile(&reordered).unwrap().scene;
    assert!(rect(&new, "h/cell/a1").x < rect(&new, "h/cell/b1").x);
    assert!(rect(&new, "h/cell/a1").y < rect(&new, "h/cell/b2").y);
    let patch = diff_scene(&old, &new, Revision(1), Revision(2), "domain-order").unwrap();
    assert_eq!(
        apply_scene_patch(&old, Revision(1), &patch).unwrap(),
        (new, Revision(2))
    );
}
#[test]
fn exact_threshold_ties_go_up_and_interval_legend_has_closed_final_endpoint() {
    let threshold = 5.0f64;
    let d = document(json!([
        {"id":"lo","x":"lo","y":"row","v":0.0},
        {"id":"below","x":"below","y":"row","v":f64::from_bits(threshold.to_bits()-1)},
        {"id":"tie","x":"tie","y":"row","v":threshold},
        {"id":"above","x":"above","y":"row","v":f64::from_bits(threshold.to_bits()+1)},
        {"id":"hi","x":"hi","y":"row","v":10.0}
    ]));
    let c = compile(&d).unwrap();
    for id in ["lo", "below"] {
        assert_eq!(fill(&c.scene, &format!("h/cell/{id}")), "#112233");
    }
    for id in ["tie", "above", "hi"] {
        assert_eq!(fill(&c.scene, &format!("h/cell/{id}")), "#aabbcc");
    }
    assert_eq!(text(&c.scene, "h/legend/0/label"), "[0, 5)");
    assert_eq!(text(&c.scene, "h/legend/1/label"), "[5, 10]");
}
#[test]
fn constant_domain_keeps_full_even_palette_and_selects_lower_middle() {
    let mut d = document(json!([{"id":"a","x":"A","y":"Y","v":7}]));
    let h = heatmap(&mut d);
    h.color.domain = None;
    h.color.palette = Some(vec![
        vizir_core::Color::hex("#111111"),
        vizir_core::Color::hex("#222222"),
        vizir_core::Color::hex("#333333"),
        vizir_core::Color::hex("#444444"),
    ]);
    let c = compile(&d).unwrap();
    let MirScale::QuantizeColor {
        domain,
        range,
        thresholds,
        ..
    } = &chart(&c.mir).scales[2]
    else {
        panic!()
    };
    assert_eq!(*domain, [7., 7.]);
    assert_eq!(range.len(), 4);
    assert!(thresholds.is_empty());
    assert_eq!(fill(&c.scene, "h/cell/a"), "#222222");
    assert_eq!(text(&c.scene, "h/legend/0/label"), "= 7");
    assert!(find_scene_node(&c.scene.nodes, "h/legend/1/swatch").is_none());
}
#[test]
fn all_canonical_themes_use_pinned_ramp_endpoints_without_canvas_paint() {
    let mut d = simple();
    heatmap(&mut d).color.palette = None;
    for name in THEME_NAMES {
        let c = compile_with_theme(&d, name).unwrap();
        let MirScale::QuantizeColor { range, .. } = &chart(&c.mir.mir).scales[2] else {
            panic!()
        };
        assert_eq!(range.len(), 5);
        assert_eq!(range[0], c.mir.theme.defaults.group_fills[0]);
        assert_eq!(range[4], c.mir.theme.defaults.mark);
        assert_eq!(c.scene.background.0, "transparent");
    }
    let unthemed = compile(&d).unwrap();
    let themed = compile_with_theme(&d, "azure").unwrap();
    assert_eq!(
        chart(&unthemed.mir).scales[2],
        chart(&themed.mir.mir).scales[2]
    );
}
#[test]
fn explicit_palette_alpha_survives_theme_selection_and_native_scene_semantics() {
    let mut d = simple();
    heatmap(&mut d).color.palette = Some(vec![
        vizir_core::Color::hex("#11223380"),
        vizir_core::Color::transparent(),
    ]);
    let c = compile_with_theme(&d, "sage-dark").unwrap();
    assert_eq!(fill(&c.scene, "h/cell/b2"), "#11223380");
    assert_eq!(fill(&c.scene, "h/cell/a1"), "transparent");
    assert!(find_scene_node(&c.scene.nodes, "h/cell/a1").is_some());
}
#[test]
fn opaque_scale_guide_ids_and_order_do_not_change_scene_geometry() {
    let c = compile(&simple()).unwrap();
    let mut mir = c.mir.clone();
    let MirView::Chart(h) = &mut mir.views[0] else {
        panic!()
    };
    for scale in &mut h.scales {
        match scale {
            MirScale::Band { id, .. } | MirScale::QuantizeColor { id, .. } => {
                *id = format!("opaque-{id}")
            }
            _ => panic!(),
        }
    }
    for guide in &mut h.guides {
        guide.scale = format!("opaque-{}", guide.scale);
        guide.id = format!("opaque-{}", guide.id);
    }
    let ChartMark::Heatmap { x, y, color, .. } = &mut h.mark else {
        panic!()
    };
    for b in [x, y, color] {
        b.scale = format!("opaque-{}", b.scale);
    }
    h.scales.reverse();
    h.guides.reverse();
    let replay = build_scene(&mir).unwrap();
    for id in ["b2", "a1", "b1"] {
        assert_eq!(
            rect(&replay, &format!("h/cell/{id}")),
            rect(&c.scene, &format!("h/cell/{id}"))
        );
        assert_eq!(
            fill(&replay, &format!("h/cell/{id}")),
            fill(&c.scene, &format!("h/cell/{id}"))
        );
    }
}
#[test]
fn adjoining_cells_use_the_same_serialized_edges_at_fractional_frames() {
    let mut d = document(json!([
        {"id":"a","x":"A","y":"Y","v":1},{"id":"b","x":"B","y":"Y","v":2},{"id":"c","x":"C","y":"Y","v":3}
    ]));
    let h = heatmap(&mut d);
    h.frame.x = 20.12345;
    h.frame.width = 999.43219;
    let c = compile(&d).unwrap();
    let units = |v: f64| (v * 10000.).round() as i64;
    for pair in [["a", "b"], ["b", "c"]] {
        let a = rect(&c.scene, &format!("h/cell/{}", pair[0]));
        let b = rect(&c.scene, &format!("h/cell/{}", pair[1]));
        assert_eq!(units(a.x) + units(a.width), units(b.x));
        assert!(a.width > 0. && a.height > 0.);
    }
}
#[test]
fn single_line_layout_rejects_crowding_controls_and_rounded_legend_collisions() {
    let mut narrow = simple();
    heatmap(&mut narrow).frame.width = 180.;
    assert!(compile(&narrow).is_err());
    let mut long = simple();
    heatmap(&mut long).x.domain = Some(vec!["B".into(), "A".into(), "W".repeat(200)]);
    assert!(compile(&long).is_err());
    let mut bad = simple();
    heatmap(&mut bad).color.label = Some("a\nb".into());
    assert!(compile(&bad).is_err());
    let mut rounded = simple();
    let h = heatmap(&mut rounded);
    h.color.domain = Some([0., 0.01]);
    h.color.number_format = Some(vizir_core::NumberFormat {
        notation: vizir_core::NumberNotation::Fixed,
        precision: 0,
    });
    for r in &mut rounded.datasets.get_mut("d").unwrap().rows {
        r.insert("v".into(), json!(0.));
    }
    assert!(compile(&rounded).is_err());
}
#[test]
fn measured_cjk_labels_and_existing_title_wrap_replay_through_compiled_envelope() {
    let (base, resources) = fixtures::profile();
    let mut d = document(json!([
        {"id":"a","x":"北京","y":"甲","v":0},{"id":"b","x":"上海","y":"乙","v":10}
    ]));
    heatmap(&mut d).title = Some("Heatmap 中文标题 café with complete source".into());
    let ctx = base
        .with_theme(ThemeContext::resolve("azure").unwrap())
        .with_text_layout(TextLayoutContext::new(vec![]).with_semantic_targets(vec![
            SemanticTextLayoutTarget::chart_title("h", 200., 6, 30.),
        ]));
    let c = compile_with_context(&d, &ctx, &resources).unwrap();
    assert!(matches!(
        find_scene_node(&c.scene.nodes, "h/title"),
        Some(SceneNode::Path { .. })
    ));
    let replay = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(build_compiled_scene(&replay, &resources).unwrap(), c.scene);
    let mut limits = TextLimits::default();
    limits.max_collision_checks = 1;
    assert!(
        compile_with_context_with_limits(
            &d,
            &ctx,
            &resources,
            MaterializationLimits::default(),
            limits
        )
        .is_err()
    );
    for version in ["0.1", "0.2", "0.3"] {
        let mut old = d.clone();
        old.version = version.into();
        assert!(compile_with_context(&old, &ctx, &resources).is_err());
    }
}
#[test]
fn version_four_area_retains_version_three_scene_geometry() {
    let d = vizir_core::parse_document(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chart/baseline-area.viz.yaml"),
    )
    .unwrap();
    let old = compile(&d).unwrap();
    let mut new = d;
    new.version = "0.4".into();
    assert_eq!(compile(&new).unwrap().scene, old.scene);
}
#[test]
fn cell_insertion_removal_and_color_change_have_scene_patch_roundtrips() {
    let d = simple();
    let old = compile(&d).unwrap().scene;
    let mut next = d;
    let data = next.datasets.get_mut("d").unwrap();
    data.rows.remove(0);
    data.rows[0].insert("v".into(), json!(0));
    data.rows
        .push(serde_json::from_value(json!({"id":"new","x":"C","y":"first","v":8})).unwrap());
    let new = compile(&next).unwrap().scene;
    let patch = diff_scene(&old, &new, Revision(7), Revision(8), "cell-change").unwrap();
    assert_eq!(
        apply_scene_patch(&old, Revision(7), &patch).unwrap(),
        (new, Revision(8))
    );
}

#[test]
fn heatmap_rejects_unexecuted_coordinate_space_semantics_in_replay_and_refresh() {
    let original = compile(&simple()).unwrap().mir;
    for mutation in 0..9 {
        let mut mir = original.clone();
        let id = chart(&mir).space.clone();
        match mutation {
            0 => {
                mir.spaces
                    .get_mut(&id)
                    .unwrap()
                    .transform_to_parent
                    .translate
                    .x = 100.
            }
            1 => {
                mir.spaces
                    .get_mut(&id)
                    .unwrap()
                    .transform_to_parent
                    .rotate_degrees = 90.
            }
            2 => mir.spaces.get_mut(&id).unwrap().transform_to_parent.scale.x = 2.,
            3 => mir.spaces.get_mut(&id).unwrap().kind = vizir_core::CoordinateSpaceKind::Plot,
            4 => mir.spaces.get_mut(&id).unwrap().unit = vizir_core::SpatialUnit::Pixel,
            5 => mir.spaces.get_mut(&id).unwrap().parent = Some(id.clone()),
            6 => {
                mir.spaces
                    .get_mut(&id)
                    .unwrap()
                    .transform_to_parent
                    .translate
                    .x = f64::NAN
            }
            7 | 8 => {
                let mut space = mir.spaces[&id].clone();
                space.id = "other-space".into();
                mir.spaces.insert(space.id.clone(), space);
                let MirView::Chart(h) = &mut mir.views[0] else {
                    panic!()
                };
                if mutation == 7 {
                    h.space = "other-space".into();
                } else if let MirScale::Band { range_space, .. } = &mut h.scales[0] {
                    *range_space = "other-space".into();
                }
            }
            _ => unreachable!(),
        }
        assert!(build_scene(&mir).is_err(), "mutation{mutation}");
        assert!(
            vizir_compiler::rematerialize_mir(&mir).is_err(),
            "mutation{mutation}"
        );
    }
}
#[test]
fn heatmap_identity_document_space_id_remains_opaque() {
    let c = compile(&simple()).unwrap();
    let mut mir = c.mir;
    let old = chart(&mir).space.clone();
    let mut space = mir.spaces.remove(&old).unwrap();
    space.id = "opaque-document".into();
    mir.spaces.insert(space.id.clone(), space);
    let MirView::Chart(h) = &mut mir.views[0] else {
        panic!()
    };
    h.space = "opaque-document".into();
    for scale in &mut h.scales {
        if let MirScale::Band { range_space, .. } = scale {
            *range_space = "opaque-document".into();
        }
    }
    assert_eq!(build_scene(&mir).unwrap(), c.scene);
    assert_eq!(vizir_compiler::rematerialize_mir(&mir).unwrap(), mir);
}
