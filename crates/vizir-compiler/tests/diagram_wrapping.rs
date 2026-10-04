#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, DiagramTextLayoutTarget, FontResources, MaterializationLimits,
    TextLayoutContext, TextLimits, build_compiled_scene, compile_with_context,
    compile_with_context_with_limits, parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{
    Document, MirView, Revision, Scene2D, SceneNode, apply_scene_patch, diff_scene, find_scene_node,
};
fn document(label: &str) -> Document {
    serde_json::from_value(json!({"version":"0.2","id":"diagram-wrap","width":800,"height":500,"views":[{"kind":"diagram.graph","id":"d","frame":{"x":20,"y":20,"width":760,"height":460},"layout":"manual","nodes":[{"id":"n","label":label,"position":{"x":200,"y":180}},{"id":"other","label":"Beta","position":{"x":550,"y":180}}],"edges":[{"from":"n","to":"other","label":"Data"}]}]})).unwrap()
}
fn context(width: f64) -> (CompilationContext, FontResources) {
    let (c, r) = fixtures::profile();
    (
        c.with_text_layout(
            TextLayoutContext::new(vec![])
                .with_diagram_targets(vec![DiagramTextLayoutTarget::new("d", "n", width, 8, 20.)]),
        ),
        r,
    )
}
fn change(d: &Document, f: impl FnOnce(&mut Value)) -> Document {
    let mut v = serde_json::to_value(d).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
fn label(s: &Scene2D) -> &SceneNode {
    find_scene_node(&s.nodes, "d/node/n/label").unwrap()
}
fn explanation(s: &Scene2D) -> &str {
    let SceneNode::Path { origin, .. } = label(s) else {
        panic!()
    };
    &origin.explanation
}
fn ranges(s: &Scene2D) -> Vec<(usize, usize, usize, usize, f64, f64)> {
    explanation(s)
        .split("source byte coverage [")
        .nth(1)
        .unwrap()
        .split("]; original text:")
        .next()
        .unwrap()
        .split("; ")
        .map(|l| {
            let p: Vec<_> = l.split(' ').collect();
            let a: Vec<usize> = p[0].split("..").map(|x| x.parse().unwrap()).collect();
            let b: Vec<usize> = p[2].split("..").map(|x| x.parse().unwrap()).collect();
            (
                a[0],
                a[1],
                b[0],
                b[1],
                p[4].parse().unwrap(),
                p[6].parse().unwrap(),
            )
        })
        .collect()
}
#[test]
fn exact_source_bytes_hard_breaks_and_empty_lines_are_preserved() {
    let (c, r) = context(132.);
    for source in [
        "Café Å\n中文测试",
        "A\r\nB",
        "A\u{2028}B",
        "A\u{2029}B",
        " A  B ",
        "\nA",
        "A\n",
        "\n",
        "",
        "  ",
    ] {
        let result = compile_with_context(&document(source), &c, &r).unwrap();
        let spans = ranges(&result.scene);
        let mut output = String::new();
        for (a, b, c, d, _, _) in spans {
            output.push_str(&source[a..b]);
            output.push_str(&source[c..d]);
        }
        assert_eq!(output, source);
        assert!(explanation(&result.scene).ends_with(source));
        assert!(explanation(&result.scene).contains("fixed 13px Medium"));
        assert_eq!(result.scene, build_compiled_scene(&result.mir, &r).unwrap());
    }
}
#[test]
fn node_shapes_edges_and_layout_request_do_not_change() {
    let (ctx, r) = context(132.);
    let mut single = ctx.clone();
    single.text_layout = None;
    let d = document("Alpha Beta");
    let a = compile_with_context(&d, &single, &r).unwrap();
    let b = compile_with_context(&d, &ctx, &r).unwrap();
    assert_eq!(a.mir.mir, b.mir.mir);
    for id in [
        "d/node/n/shape",
        "d/node/other/shape",
        "d/node/other/label",
        "d/edge/0-n-other",
        "d/edge/0/label",
    ] {
        assert_eq!(
            find_scene_node(&a.scene.nodes, id),
            find_scene_node(&b.scene.nodes, id)
        );
    }
}
#[test]
fn fixed_medium_size_bypasses_only_selected_long_label_shrink() {
    let (ctx, r) = context(132.);
    let a = compile_with_context(&document("AV"), &ctx, &r).unwrap();
    let b = compile_with_context(&document("AV\n                    "), &ctx, &r).unwrap();
    let SceneNode::Path { bounds: a, .. } = label(&a.scene) else {
        panic!()
    };
    let SceneNode::Path { bounds: b, .. } = label(&b.scene) else {
        panic!()
    };
    assert!((a.width - b.width).abs() < 0.0002);
    assert!((a.height - b.height).abs() < 0.0002);
}
#[test]
fn full_raw_allocation_is_centered_at_fractional_node_position() {
    let (ctx, r) = context(132.);
    let d = change(&document("Å A\u{30a} gypq\n中文"), |v| {
        v["views"][0]["nodes"][0]["position"] = json!({"x":200.123456,"y":180.654321});
        v["views"][0]["frame"]["x"] = 20.123456.into();
    });
    let c = compile_with_context(&d, &ctx, &r).unwrap();
    let SceneNode::Path { bounds, .. } = label(&c.scene) else {
        panic!()
    };
    let SceneNode::Rect { bounds: node, .. } =
        find_scene_node(&c.scene.nodes, "d/node/n/shape").unwrap()
    else {
        panic!()
    };
    let round = |x: f64| format!("{x:.4}").parse::<f64>().unwrap();
    assert!(bounds.x >= round(node.x) + 8.);
    assert!(bounds.x + bounds.width <= round(node.x) + 142.);
    assert!(bounds.y >= round(node.y) + 4.);
    assert!(bounds.y + bounds.height <= round(node.y) + 58.);
    let lines = ranges(&c.scene);
    assert_eq!(lines.len(), 2);
    assert!((lines[1].4 - lines[0].4 - 20.).abs() < 0.00011);
    // Noto fixture metrics ascent1.16 and descent−0.288; source ink stays
    // within this logical envelope. Its complete allocation is centered.
    let top = lines[0].4 - 13. * 1.16;
    let bottom = lines[1].4 + 13. * 0.288;
    assert!(((top + bottom) / 2. - (180.654321 + 20.)).abs() < 0.000051);
}
#[test]
fn authored_width_and_actual_interior_both_select_legal_breaks() {
    let (large, r) = context(10000.);
    let (small, _) = context(65.);
    let d = document("Alpha Beta Gamma Delta");
    let a = compile_with_context(&d, &large, &r).unwrap();
    let b = compile_with_context(&d, &small, &r);
    assert!(ranges(&a.scene).len() >= 2);
    assert!(b.is_err()); // Three lines cannot fit this fixed-height node.
    for width in [10000., 10.] {
        let (c, _) = context(width);
        assert!(compile_with_context(&document("WWWWWWWWWWWWWWWWWWWWWWWW"), &c, &r).is_err());
    }
}
#[test]
fn three_lines_blank_height_spacing_and_unsupported_sources_fail() {
    let (c, r) = context(132.);
    for source in [
        "A\n\n",
        "\n\n",
        "A\tB",
        "A\rB",
        "foo\u{ad}bar",
        "العربية",
        "😀",
    ] {
        assert!(
            compile_with_context(&document(source), &c, &r).is_err(),
            "{source:?}"
        );
    }
    let mut c = c;
    c.text_layout
        .as_mut()
        .unwrap()
        .diagram_targets
        .as_mut()
        .unwrap()[0]
        .line_height = 10.;
    assert!(
        compile_with_context(&document("A\nB"), &c, &r)
            .unwrap_err()
            .to_string()
            .contains("line_height")
    );
}
#[test]
fn node_permission_does_not_leak_to_edges_titles_or_other_views() {
    let (c, r) = context(132.);
    for field in ["edge", "title", "node"] {
        let d = change(&document("A\nB"), |v| match field {
            "edge" => v["views"][0]["edges"][0]["label"] = "A\nB".into(),
            "title" => v["views"][0]["title"] = "A\nB".into(),
            _ => v["views"][0]["nodes"][1]["label"] = "A\nB".into(),
        });
        assert!(compile_with_context(&d, &c, &r).is_err());
    }
    let d = change(&document("A\nB"), |v| {
        let mut other = v["views"][0].clone();
        other["id"] = "other-view".into();
        v["views"].as_array_mut().unwrap().push(other);
    });
    assert!(compile_with_context(&d, &c, &r).is_err());
}
#[test]
fn slash_ids_and_same_node_ids_across_views_resolve_independently() {
    let (mut c, r) = context(132.);
    let d = change(&document("A\nB"), |v| {
        v["views"][0]["id"] = "d/one".into();
        v["views"][0]["nodes"][0]["id"] = "n/label".into();
        v["views"][0]["edges"][0]["from"] = "n/label".into();
        let mut other = v["views"][0].clone();
        other["id"] = "d/two".into();
        other["nodes"][0]["label"] = "中文\n测试".into();
        v["views"].as_array_mut().unwrap().push(other);
    });
    c.text_layout = Some(TextLayoutContext::new(vec![]).with_diagram_targets(vec![
        DiagramTextLayoutTarget::new("d/one", "n/label", 132., 2, 20.),
        DiagramTextLayoutTarget::new("d/two", "n/label", 132., 2, 20.),
    ]));
    let result = compile_with_context(&d, &c, &r).unwrap();
    assert!(find_scene_node(&result.scene.nodes, "d/one/node/n/label/label").is_some());
    assert!(find_scene_node(&result.scene.nodes, "d/two/node/n/label/label").is_some());
}
#[test]
fn missing_or_wrong_source_targets_reject_in_hir_and_mir() {
    let (mut c, r) = context(132.);
    let d = document("Alpha");
    let compiled = compile_with_context(&d, &c, &r).unwrap();
    for (view, node) in [("missing", "n"), ("d", "missing")] {
        c.text_layout
            .as_mut()
            .unwrap()
            .diagram_targets
            .as_mut()
            .unwrap()[0]
            .view_id = view.into();
        c.text_layout
            .as_mut()
            .unwrap()
            .diagram_targets
            .as_mut()
            .unwrap()[0]
            .node_id = node.into();
        assert!(compile_with_context(&d, &c, &r).is_err());
        let mut m = compiled.mir.clone();
        m.context = c.clone();
        assert!(build_compiled_scene(&m, &r).is_err());
    }
}
#[test]
fn edited_fitting_label_replays_refreshes_and_patches_without_graph_relayout() {
    let (c, r) = context(132.);
    let a = compile_with_context(&document("Alpha\nBeta"), &c, &r).unwrap();
    let bytes = serde_json::to_string(&a.mir).unwrap();
    let mut replay = parse_compiled_mir_json(bytes.as_bytes()).unwrap();
    let MirView::Diagram(diagram) = &mut replay.mir.views[0] else {
        panic!()
    };
    diagram.nodes[0].label = "中文\n测试".into();
    let b = compile_with_context(&document("中文\n测试"), &c, &r).unwrap();
    assert_eq!(build_compiled_scene(&replay, &r).unwrap(), b.scene);
    assert_eq!(rematerialize_compiled_mir(&replay, &r).unwrap(), b.mir);
    let patch = diff_scene(
        &a.scene,
        &b.scene,
        Revision(1),
        Revision(2),
        "diagram-label",
    )
    .unwrap();
    assert_eq!(
        apply_scene_patch(&a.scene, Revision(1), &patch).unwrap().0,
        b.scene
    );
    for id in ["d/node/n/shape", "d/node/other/shape", "d/edge/0-n-other"] {
        assert_eq!(
            find_scene_node(&a.scene.nodes, id),
            find_scene_node(&b.scene.nodes, id)
        );
    }
    let MirView::Diagram(diagram) = &mut replay.mir.views[0] else {
        panic!()
    };
    diagram.nodes[0].label = "A\nB\nC".into();
    assert!(build_compiled_scene(&replay, &r).is_err());
}
#[test]
fn layered_positions_are_independent_of_wrapped_label_contents() {
    let (c, r) = context(132.);
    let make = |s| {
        change(&document(s), |v| {
            v["views"][0]["layout"] = "layered".into();
            for n in v["views"][0]["nodes"].as_array_mut().unwrap() {
                n.as_object_mut().unwrap().remove("position");
            }
        })
    };
    let a = compile_with_context(&make("Alpha\nBeta"), &c, &r).unwrap();
    let b = compile_with_context(&make("中文\n测试"), &c, &r).unwrap();
    for id in ["d/node/n/shape", "d/node/other/shape", "d/edge/0-n-other"] {
        assert_eq!(
            find_scene_node(&a.scene.nodes, id),
            find_scene_node(&b.scene.nodes, id)
        );
    }
}
#[test]
fn candidate_line_cache_and_output_limits_remain_whole_call() {
    let (c, r) = context(132.);
    let d = document("Alpha\nBeta");
    for which in 0..4 {
        let mut l = TextLimits::new();
        match which {
            0 => l.max_wrap_candidates = 1,
            1 => l.max_layout_lines = 1,
            2 => l.max_cache_bytes = 128,
            _ => l.max_output_bytes = 128,
        };
        let error =
            compile_with_context_with_limits(&d, &c, &r, MaterializationLimits::default(), l)
                .unwrap_err()
                .to_string();
        assert!(error.contains("limit"), "{error}");
    }
}
