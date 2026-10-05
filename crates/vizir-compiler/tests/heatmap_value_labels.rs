#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    MaterializationLimits, TextLimits, build_compiled_scene, build_scene, compile,
    compile_with_context, compile_with_context_with_limits, lower_to_mir, rematerialize_mir,
};
use vizir_core::{
    ChartMark, Color, Document, HeatmapValueLabels, MirHeatmapValueLabels, MirView, NumberFormat,
    NumberNotation, Revision, SceneNode, View, VizMir, apply_scene_patch, diff_scene,
    find_scene_node,
};

fn source(rows: Value) -> Document {
    serde_json::from_value(json!({"version":"0.5","id":"values","width":1600,"height":800,
        "datasets":{"d":{"key":"id","rows":rows}},"views":[{"kind":"chart.heatmap","id":"h",
        "frame":{"x":0,"y":0,"width":1600,"height":800},"dataset":"d","x":{"field":"x"},"y":{"field":"y"},
        "color":{"field":"v","palette":["#000000","#FFFFFF"]},"value_labels":{}}]})).unwrap()
}
fn simple() -> Document {
    source(json!([{"id":"a","x":"A","y":"one","v":0},{"id":"b","x":"B","y":"two","v":10}]))
}
fn chart(d: &mut Document) -> &mut vizir_core::HeatmapChart {
    let View::Heatmap(chart) = &mut d.views[0] else {
        panic!()
    };
    chart
}
fn cache(mir: &VizMir) -> &MirHeatmapValueLabels {
    let MirView::Chart(c) = &mir.views[0] else {
        panic!()
    };
    let ChartMark::Heatmap {
        value_labels: Some(labels),
        ..
    } = &c.mark
    else {
        panic!()
    };
    labels
}
fn cache_mut(mir: &mut VizMir) -> &mut MirHeatmapValueLabels {
    let MirView::Chart(c) = &mut mir.views[0] else {
        panic!()
    };
    let ChartMark::Heatmap {
        value_labels: Some(labels),
        ..
    } = &mut c.mark
    else {
        panic!()
    };
    labels
}
fn text(scene: &vizir_core::Scene2D, key: &str) -> String {
    let SceneNode::Text { text, .. } =
        find_scene_node(&scene.nodes, &format!("h/cell-label/{key}")).unwrap()
    else {
        panic!()
    };
    text.clone()
}

#[test]
fn cells_and_guides_are_unchanged_and_missing_pair_is_not_zero() {
    let mut d = simple();
    chart(&mut d).x.domain = Some(vec!["A".into(), "B".into(), "reserved".into()]);
    let labeled = compile(&d).unwrap();
    let mut old = d.clone();
    old.version = "0.4".into();
    chart(&mut old).value_labels = None;
    let baseline = compile(&old).unwrap();
    let mut stripped = labeled.scene.clone();
    let SceneNode::Group { children, .. } = &mut stripped.nodes[0] else {
        panic!()
    };
    children.retain(|node| !node.id().starts_with("h/cell-label/"));
    assert_eq!(stripped, baseline.scene);
    assert_eq!(text(&labeled.scene, "a"), "0");
    assert!(find_scene_node(&labeled.scene.nodes, "h/cell/missing").is_none());
    assert_eq!(cache(&labeled.mir).instances.len(), 2);
    let SceneNode::Text { origin, color, .. } =
        find_scene_node(&labeled.scene.nodes, "h/cell-label/a").unwrap()
    else {
        panic!()
    };
    assert_eq!(origin.data_key.as_deref(), Some("a"));
    assert_eq!(origin.data_lineage, vec!["data/d"]);
    assert_eq!(origin.hir_node, "h");
    assert_eq!(origin.mir_node, "h/marks/cells");
    assert_eq!(color.0, "#FFFFFF");
    let mut newer = old.clone();
    newer.version = "0.5".into();
    assert_eq!(compile(&newer).unwrap().scene, baseline.scene);
}

#[test]
fn authoritative_integer_labels_survive_f64_paint_projection() {
    for v in [
        9007199254740991_i64,
        9007199254740992,
        9007199254740993,
        i64::MIN,
        i64::MAX,
    ] {
        let d = source(json!([{"id":"a","x":"A","y":"row","v":v}]));
        let c = compile(&d).unwrap();
        assert_eq!(text(&c.scene, "a"), v.to_string());
        assert_eq!(cache(&c.mir).instances[0].text, v.to_string());
        assert_eq!(build_scene(&c.mir).unwrap(), c.scene);
    }
    let mixed = source(
        json!([{"id":"a","x":"A","y":"row","v":9007199254740993_i64},{"id":"b","x":"B","y":"row","v":1.5}]),
    );
    let c = compile(&mixed).unwrap();
    // An inferred mixed column has authoritative Float64 semantics, including conversion.
    assert_eq!(text(&c.scene, "a"), "9.007199254740992e15");
    assert_eq!(text(&c.scene, "b"), "1.5");
}

#[test]
fn floats_signed_zero_extremes_and_requested_rounding() {
    for v in [
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -f64::MAX,
        1e-30,
    ] {
        let c = compile(&source(json!([{"id":"a","x":"A","y":"row","v":v}]))).unwrap();
        assert_eq!(
            text(&c.scene, "a").parse::<f64>().unwrap().to_bits(),
            v.to_bits()
        );
    }
    let d = source(json!([{"id":"a","x":"A","y":"row","v":-0.0}]));
    let c = compile(&d).unwrap();
    assert_eq!(text(&c.scene, "a"), "0");
    let MirView::Chart(h) = &c.mir.views[0] else {
        panic!()
    };
    let ChartMark::Heatmap { instances, .. } = &h.mark else {
        panic!()
    };
    assert_eq!(instances[0].value.to_bits(), (-0_f64).to_bits());
    let mut d = source(
        json!([{"id":"a","x":"A","y":"row","v":-0.01},{"id":"b","x":"B","y":"row","v":0.01}]),
    );
    chart(&mut d).value_labels.as_mut().unwrap().number_format = Some(NumberFormat {
        notation: NumberNotation::Fixed,
        precision: 0,
    });
    let c = compile(&d).unwrap();
    assert_eq!(text(&c.scene, "a"), "0");
    assert_eq!(text(&c.scene, "b"), "0");
}

#[test]
fn replay_checks_every_cache_component_and_refresh_preserves_plan() {
    let c = compile(&simple()).unwrap();
    for mutation in 0..4 {
        let mut mir = c.mir.clone();
        let labels = cache_mut(&mut mir);
        match mutation {
            0 => labels.instances[0].text = "wrong".into(),
            1 => labels.instances[0].key = "wrong".into(),
            2 => labels.instances.reverse(),
            _ => {
                labels.instances.pop();
            }
        }
        assert!(
            build_scene(&mir)
                .unwrap_err()
                .to_string()
                .contains("stored mark")
        );
        assert_eq!(rematerialize_mir(&mir).unwrap(), c.mir);
    }
    let mut mir = c.mir.clone();
    cache_mut(&mut mir).number_format = Some(NumberFormat {
        notation: NumberNotation::Fixed,
        precision: 2,
    });
    assert!(build_scene(&mir).is_err());
    let refreshed = rematerialize_mir(&mir).unwrap();
    assert_eq!(cache(&refreshed).instances[0].text, "0.00");
    let MirView::Chart(a) = &c.mir.views[0] else {
        panic!()
    };
    let MirView::Chart(b) = &refreshed.views[0] else {
        panic!()
    };
    assert_eq!(a.scales, b.scales);
    let mut changed = serde_json::to_value(&c.mir).unwrap();
    changed["data"]["data/d"]["operator"]["rows"][0]["v"] = 5.into();
    let changed: VizMir = serde_json::from_value(changed).unwrap();
    let fresh = rematerialize_mir(&changed).unwrap();
    assert_eq!(cache(&fresh).instances[0].text, "5");
}

#[test]
fn strict_per_cell_fit_reports_key_and_keeps_fractional_edges() {
    let mut d = simple();
    d.width = 701.1234;
    chart(&mut d).frame.width = 701.1234;
    let c = compile(&d).unwrap();
    for key in ["a", "b"] {
        let SceneNode::Rect { bounds: cell, .. } =
            find_scene_node(&c.scene.nodes, &format!("h/cell/{key}")).unwrap()
        else {
            panic!()
        };
        let SceneNode::Text { bounds: label, .. } =
            find_scene_node(&c.scene.nodes, &format!("h/cell-label/{key}")).unwrap()
        else {
            panic!()
        };
        assert!(label.x >= cell.x + 4. && label.x + label.width <= cell.x + cell.width - 4.);
        assert!(label.y >= cell.y + 4. && label.y + label.height <= cell.y + cell.height - 4.);
    }
    let mut d = source(json!([{"id":"long/key","x":"A","y":"row","v":9007199254740993_i64}]));
    chart(&mut d).x.domain = Some(
        (0..20)
            .map(|i| if i == 0 { "A".into() } else { format!("X{i}") })
            .collect(),
    );
    let err = compile(&d).unwrap_err().to_string();
    assert!(
        err.contains("long/key") && err.contains("chart \"h\"") && err.contains("does not fit"),
        "{err}"
    );
    chart(&mut d).x.domain = None;
    chart(&mut d).y.domain = Some(
        (0..40)
            .map(|i| {
                if i == 0 {
                    "row".into()
                } else {
                    format!("Y{i}")
                }
            })
            .collect(),
    );
    let err = compile(&d).unwrap_err().to_string();
    assert!(err.contains("does not fit"), "{err}");
}

#[test]
fn measured_labels_use_same_fonts_and_keep_escaped_key_provenance() {
    let d = source(
        json!([{"id":"nested/key&\"雪","x":"中","y":"row","v":0},{"id":"b","x":"文","y":"row","v":3}]),
    );
    let (context, resources) = fixtures::profile();
    let c = compile_with_context(&d, &context, &resources).unwrap();
    let node = find_scene_node(&c.scene.nodes, "h/cell-label/nested/key&\"雪").unwrap();
    assert!(matches!(node, SceneNode::Path { .. }));
    assert_eq!(node.origin().data_key.as_deref(), Some("nested/key&\"雪"));
    assert!(node.origin().explanation.contains("0"));
    assert_eq!(build_compiled_scene(&c.mir, &resources).unwrap(), c.scene);
    assert!(build_compiled_scene(&c.mir, &vizir_compiler::FontResources::new()).is_err());
    for setting in 0..3 {
        let mut limits = TextLimits::default();
        match setting {
            0 => limits.max_labels = 1,
            1 => limits.max_output_bytes = 100,
            _ => limits.max_text_bytes = 1,
        }
        assert!(
            compile_with_context_with_limits(
                &d,
                &context,
                &resources,
                MaterializationLimits::default(),
                limits
            )
            .is_err()
        );
    }
}

#[test]
fn transparent_fills_need_explicit_opaque_label_color_and_keep_canvas() {
    let mut d = simple();
    chart(&mut d).color.palette = Some(vec![Color::hex("#00000080"), Color::transparent()]);
    let err = compile(&d).unwrap_err().to_string();
    assert!(err.contains("value_labels.color"));
    chart(&mut d).value_labels.as_mut().unwrap().color = Some(Color::hex("#123456FF"));
    let c = compile(&d).unwrap();
    assert_eq!(c.scene.background.0, "transparent");
    for key in ["a", "b"] {
        let SceneNode::Text { color, .. } =
            find_scene_node(&c.scene.nodes, &format!("h/cell-label/{key}")).unwrap()
        else {
            panic!()
        };
        assert_eq!(color.0, "#123456FF");
    }
}

#[test]
fn insertion_removal_order_and_scene_patch_preserve_stable_ids() {
    let d = simple();
    let old = compile(&d).unwrap().scene;
    let mut next = d.clone();
    next.datasets.get_mut("d").unwrap().rows.reverse();
    next.datasets
        .get_mut("d")
        .unwrap()
        .rows
        .push(serde_json::from_value(json!({"id":"new","x":"A","y":"two","v":2})).unwrap());
    let new = compile(&next).unwrap().scene;
    let patch = diff_scene(&old, &new, Revision(1), Revision(2), "labels").unwrap();
    assert_eq!(
        apply_scene_patch(&old, Revision(1), &patch).unwrap(),
        (new, Revision(2))
    );
    next.datasets.get_mut("d").unwrap().rows.remove(0);
    let removed = compile(&next).unwrap().scene;
    assert!(find_scene_node(&removed.nodes, "h/cell/b").is_none());
    assert!(find_scene_node(&removed.nodes, "h/cell-label/b").is_none());
    assert_eq!(text(&removed, "a"), "0");
}

#[test]
fn source_and_stale_cache_limits_preflight_before_generation_or_clone() {
    let rows=Value::Array((0..4096).map(|i|json!({"id":format!("k{i}"),"x":format!("X{}",i%64),"y":format!("Y{}",i/64),"v":0})).collect());
    let mut d = source(rows);
    d.width = 10000.;
    d.height = 10000.;
    chart(&mut d).frame.width = 10000.;
    chart(&mut d).frame.height = 10000.;
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cache(&mir).instances.len(), 4096);
    let row = d.datasets["d"].rows[0].clone();
    d.datasets.get_mut("d").unwrap().rows.push(row);
    assert!(lower_to_mir(&d).unwrap_err().to_string().contains("4096"));
    let mut stale = mir.clone();
    let row = cache(&stale).instances[0].clone();
    cache_mut(&mut stale).instances.push(row);
    assert!(
        rematerialize_mir(&stale)
            .unwrap_err()
            .to_string()
            .contains("4096")
    );
    let mut stale = mir.clone();
    cache_mut(&mut stale).instances[0].text = "0".repeat(513);
    assert!(
        rematerialize_mir(&stale)
            .unwrap_err()
            .to_string()
            .contains("512")
    );
    let mut stale = mir;
    for label in &mut cache_mut(&mut stale).instances {
        label.text = "0".repeat(512);
    }
    assert!(
        rematerialize_mir(&stale)
            .unwrap_err()
            .to_string()
            .contains("1048576")
    );
    let mut d = simple();
    chart(&mut d).value_labels = Some(HeatmapValueLabels::default());
    let mut two = d.views[0].clone();
    if let View::Heatmap(c) = &mut two {
        c.id = "second".into();
    }
    d.views.push(two);
    assert!(compile(&d).is_ok());
}

#[test]
fn all_theme_ramp_bins_use_resolved_literal_contrast() {
    let mut d = source(json!([
        {"id":"0","x":"A","y":"row","v":0}, {"id":"1","x":"B","y":"row","v":1},
        {"id":"2","x":"C","y":"row","v":2}, {"id":"3","x":"D","y":"row","v":3},
        {"id":"4","x":"E","y":"row","v":4}
    ]));
    chart(&mut d).color.palette = None;
    for theme in vizir_compiler::THEME_NAMES {
        let c = vizir_compiler::compile_with_theme(&d, theme).unwrap();
        for bin in 0..5 {
            let SceneNode::Rect { style, .. } =
                find_scene_node(&c.scene.nodes, &format!("h/cell/{bin}")).unwrap()
            else {
                panic!()
            };
            let rgb = vizir_core::heatmap_opaque_rgb(&style.fill).unwrap();
            let channels = rgb.map(|v| {
                let c = f64::from(v) / 255.;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            });
            let luminance = channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
            // Equivalent contrast crossover, independent of the implementation's ratio comparison.
            let expected = if luminance >= (21_f64.sqrt() - 1.) / 20. {
                "#000000"
            } else {
                "#FFFFFF"
            };
            let SceneNode::Text { color, .. } =
                find_scene_node(&c.scene.nodes, &format!("h/cell-label/{bin}")).unwrap()
            else {
                panic!()
            };
            assert_eq!(color.0, expected, "{theme}, bin {bin}");
        }
    }
}
