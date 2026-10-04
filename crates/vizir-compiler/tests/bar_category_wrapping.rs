#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, MaterializationLimits, SemanticTextLayoutTarget, TextLayoutContext,
    TextLimits, build_compiled_scene, compile_with_context, compile_with_context_with_limits,
    parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{
    ChartMark, Document, GuideOrient, MirDataOperator, MirScale, MirView, Revision, SceneNode,
    apply_scene_patch, diff_scene, find_scene_node,
};
fn document(labels: &[&str]) -> Document {
    let rows:Vec<_>=labels.iter().enumerate().map(|(i,s)|json!({"id":format!("key-{}",labels.len()-i),"category":s,"value":i+1,"group":if i%2==0{"Alpha"}else{"Beta"}})).collect();
    serde_json::from_value(json!({"version":"0.2","id":"category-wrap","width":1240,"height":700,"datasets":{"data":{"key":"id","rows":rows}},"views":[{"kind":"chart.bar","id":"c","dataset":"data","frame":{"x":20,"y":20,"width":1200,"height":650},"category":{"field":"category","label":"category"},"value":{"field":"value","label":"value"}}]})).unwrap()
}
fn context(width: f64) -> (CompilationContext, vizir_compiler::FontResources) {
    let (c, r) = fixtures::profile();
    (
        c.with_text_layout(TextLayoutContext::new(vec![]).with_category_labels(vec![
            SemanticTextLayoutTarget::bar_category_labels("c", width, 12, 16.),
        ])),
        r,
    )
}
fn change(d: &Document, f: impl FnOnce(&mut Value)) -> Document {
    let mut v = serde_json::to_value(d).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
fn chart(c: &vizir_compiler::CompiledMir) -> &vizir_core::MirChart {
    let MirView::Chart(c) = &c.mir.views[0] else {
        panic!()
    };
    c
}
fn chart_mut(c: &mut vizir_compiler::CompiledMir) -> &mut vizir_core::MirChart {
    let MirView::Chart(c) = &mut c.mir.views[0] else {
        panic!()
    };
    c
}
fn category(
    s: &vizir_core::Scene2D,
    i: usize,
) -> (
    &vizir_core::Rect,
    &vizir_core::Origin,
    &Vec<vizir_core::PathCommand>,
) {
    let SceneNode::Path {
        bounds,
        origin,
        commands,
        ..
    } = find_scene_node(&s.nodes, &format!("c/axis/x/category/{i}")).unwrap()
    else {
        panic!()
    };
    (bounds, origin, commands)
}
fn band(c: &vizir_compiler::CompiledMir) -> (&Vec<String>, [f64; 2]) {
    let c = chart(c);
    let ChartMark::Bar { category, .. } = &c.mark else {
        panic!()
    };
    let MirScale::Band { domain, range, .. } =
        c.scales.iter().find(|s| s.id() == category.scale).unwrap()
    else {
        panic!()
    };
    (domain, *range)
}
fn bottom(c: &vizir_compiler::CompiledMir) -> f64 {
    let c = chart(c);
    let ChartMark::Bar { value, .. } = &c.mark else {
        panic!()
    };
    let MirScale::Linear { range, .. } = c.scales.iter().find(|s| s.id() == value.scale).unwrap()
    else {
        panic!()
    };
    range[0]
}
#[test]
fn complete_domain_order_source_bytes_and_baselines_are_preserved() {
    let labels = [
        "中文（字体测量），保留标点。",
        "AV office ffi Café Å",
        " A  B ",
        "A\n\nB\n",
        "\r\n中文\u{2028}测试\u{2029}",
    ];
    let (ctx, res) = context(100.);
    let d = document(&labels);
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(band(&c.mir).0, &labels.map(str::to_owned));
    for (i, source) in labels.iter().enumerate() {
        let (_, origin, _) = category(&c.scene, i);
        assert_eq!(origin.hir_node, "c");
        assert!(origin.explanation.contains(&format!("domain index {i}")));
        assert!(origin.explanation.ends_with(source));
        let spans = origin
            .explanation
            .split("source byte coverage [")
            .nth(1)
            .unwrap()
            .split("]; original text:")
            .next()
            .unwrap();
        let mut reconstructed = String::new();
        for line in spans.split("; ") {
            let p: Vec<_> = line.split(' ').collect();
            for range in [p[0], p[2]] {
                let r: Vec<usize> = range.split("..").map(|n| n.parse().unwrap()).collect();
                reconstructed.push_str(&source[r[0]..r[1]]);
            }
        }
        assert_eq!(&reconstructed, source);
    }
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}
#[test]
fn nine_plus_categories_keep_fixed_ten_pixel_weight_and_no_implicit_shrink() {
    let (ctx, res) = context(90.);
    let short = compile_with_context(&document(&["AV", "B"]), &ctx, &res).unwrap();
    let names: Vec<_> = (0..10)
        .map(|i| {
            if i == 0 {
                "AV".to_owned()
            } else {
                format!("Category {i}")
            }
        })
        .collect();
    let refs: Vec<_> = names.iter().map(String::as_str).collect();
    let many = compile_with_context(&document(&refs), &ctx, &res).unwrap();
    assert!((category(&short.scene, 0).0.width - category(&many.scene, 0).0.width).abs() < 0.0002);
    assert_eq!(band(&many.mir).0.len(), 10);
    for i in 0..10 {
        assert!(
            category(&many.scene, i)
                .1
                .explanation
                .contains("bar.category_labels")
        );
    }
}
#[test]
fn actual_bearing_envelopes_fit_own_fractional_band_cells_and_axis_gaps() {
    let (ctx, res) = context(500.);
    let source = change(
        &document(&["Å A\u{30a} gypq", "中文测试", "A\n\nB"]),
        |v| {
            let f = &mut v["views"][0]["frame"];
            f["x"] = 20.123456.into();
            f["y"] = 20.123456.into();
            f["width"] = 1199.654321.into();
            f["height"] = 649.654321.into();
        },
    );
    let c = compile_with_context(&source, &ctx, &res).unwrap();
    let (domain, range) = band(&c.mir);
    let step = (range[1] - range[0]) / domain.len() as f64;
    let round = |n: f64| format!("{n:.4}").parse::<f64>().unwrap();
    let SceneNode::Path { bounds: axis, .. } =
        find_scene_node(&c.scene.nodes, "c/axis/x/title").unwrap()
    else {
        panic!()
    };
    for i in 0..domain.len() {
        let (b, _, _) = category(&c.scene, i);
        assert!(b.x >= round(range[0] + i as f64 * step) + 4.);
        assert!(b.x + b.width <= round(range[0] + (i + 1) as f64 * step) - 4.);
        assert!(b.y >= round(bottom(&c.mir)) + 8.);
        assert!(b.y + b.height <= axis.y - 8.);
    }
}
#[test]
fn blank_and_terminal_lines_reserve_bottom_before_ranges_without_data_changes() {
    let (ctx, res) = context(100.);
    let one = compile_with_context(&document(&["A", "B"]), &ctx, &res).unwrap();
    let blank = compile_with_context(&document(&["A\n\n", "B"]), &ctx, &res).unwrap();
    assert!(bottom(&one.mir) - bottom(&blank.mir) > 30.9);
    let a = chart(&one.mir);
    let b = chart(&blank.mir);
    for (x, y) in a.scales.iter().zip(&b.scales) {
        if let (MirScale::Linear { domain: a, .. }, MirScale::Linear { domain: b, .. }) = (x, y) {
            assert_eq!(a, b);
        }
    }
    let mut old = ctx.clone();
    old.text_layout = None;
    let baseline = compile_with_context(&document(&["A", "B"]), &old, &res).unwrap();
    assert_eq!(baseline.mir.mir.data, one.mir.mir.data);
    assert_eq!(chart(&baseline.mir).mark, chart(&one.mir).mark);
}
#[test]
fn renamed_scale_and_guide_ids_and_unused_domain_items_are_resolved_by_references() {
    let (ctx, res) = context(100.);
    let mut c = compile_with_context(&document(&["A\nA", "B\nB"]), &ctx, &res).unwrap();
    let before = category(&c.scene, 0).2.clone();
    let chart = chart_mut(&mut c.mir);
    let ChartMark::Bar {
        category: binding, ..
    } = &mut chart.mark
    else {
        panic!()
    };
    let old = binding.scale.clone();
    binding.scale = "opaque/band".into();
    for s in &mut chart.scales {
        if let MirScale::Band { id, domain, .. } = s
            && *id == old
        {
            *id = "opaque/band".into();
            domain.push("Unused\n中文".into());
        }
    }
    for g in &mut chart.guides {
        if g.orient == GuideOrient::Bottom {
            g.id = "unrelated/axis".into();
            g.scale = "opaque/band".into();
        }
    }
    let scene = build_compiled_scene(&c.mir, &res).unwrap();
    assert!(category(&scene, 2).1.explanation.ends_with("Unused\n中文"));
    assert_ne!(category(&scene, 0).2, &before);
    assert_eq!(band(&c.mir).0.len(), 3);
}
#[test]
fn missing_mismatched_empty_domains_reject_only_under_targeted_policy() {
    let (ctx, res) = context(100.);
    let c = compile_with_context(&document(&["A", "B"]), &ctx, &res).unwrap();
    let mut mismatch = c.mir.clone();
    let chart = chart_mut(&mut mismatch);
    let mut extra = chart
        .scales
        .iter()
        .find(|s| matches!(s, MirScale::Band { .. }))
        .unwrap()
        .clone();
    let MirScale::Band { id, .. } = &mut extra else {
        panic!()
    };
    *id = "other".into();
    chart.scales.push(extra);
    chart
        .guides
        .iter_mut()
        .find(|g| g.orient == GuideOrient::Bottom)
        .unwrap()
        .scale = "other".into();
    assert!(
        build_compiled_scene(&mismatch, &res)
            .unwrap_err()
            .to_string()
            .contains("category binding")
    );
    mismatch.context.text_layout = None;
    assert!(build_compiled_scene(&mismatch, &res).is_ok());
    let mut missing = c.mir.clone();
    chart_mut(&mut missing)
        .guides
        .retain(|g| g.orient != GuideOrient::Bottom);
    assert!(build_compiled_scene(&missing, &res).is_err());
    let mut empty = c.mir.clone();
    for s in &mut chart_mut(&mut empty).scales {
        if let MirScale::Band { domain, .. } = s {
            domain.clear();
        }
    }
    assert!(
        build_compiled_scene(&empty, &res)
            .unwrap_err()
            .to_string()
            .contains("nonempty")
    );
}
#[test]
fn newline_permission_does_not_leak_to_legend_other_view_or_axis() {
    let (ctx, res) = context(100.);
    let d = document(&["A\nB", "C"]);
    for mode in ["legend", "other", "axis"] {
        let d = change(&d, |v| match mode {
            "legend" => v["views"][0]["color"] = json!({"field":"category"}),
            "other" => {
                let mut other = v["views"][0].clone();
                other["id"] = "other".into();
                v["views"].as_array_mut().unwrap().push(other);
            }
            _ => v["views"][0]["category"]["label"] = "A\nB".into(),
        });
        assert!(
            compile_with_context(&d, &ctx, &res)
                .unwrap_err()
                .to_string()
                .contains("single-line")
        );
    }
}
#[test]
fn plan_reuse_is_bounded_and_does_not_repeat_lines_or_candidates() {
    let (ctx, res) = context(100.);
    let mut l = TextLimits::new();
    l.max_layout_lines = 2;
    l.max_wrap_candidates = 2;
    let c = compile_with_context_with_limits(
        &document(&["A", "B"]),
        &ctx,
        &res,
        MaterializationLimits::default(),
        l,
    )
    .unwrap();
    assert!(category(&c.scene, 0).0.height > 0.);
    l.max_wrap_candidates = 1;
    assert!(
        compile_with_context_with_limits(
            &document(&["A", "B"]),
            &ctx,
            &res,
            MaterializationLimits::default(),
            l
        )
        .is_err()
    );
    for name in ["cache", "bytes", "labels"] {
        let mut l = TextLimits::new();
        match name {
            "cache" => l.max_cache_bytes = 100,
            "bytes" => l.max_text_bytes = 10,
            _ => l.max_labels = 3,
        };
        assert!(
            compile_with_context_with_limits(
                &document(&["A A A", "B B B"]),
                &ctx,
                &res,
                MaterializationLimits::default(),
                l
            )
            .is_err()
        );
    }
}
#[test]
fn replay_refresh_and_patches_preserve_explicit_ranges_and_need_hir_for_reflow() {
    let (ctx, res) = context(100.);
    let a = compile_with_context(&document(&["A", "B"]), &ctx, &res).unwrap();
    let decoded = parse_compiled_mir_json(&serde_json::to_vec(&a.mir).unwrap()).unwrap();
    assert_eq!(decoded, rematerialize_compiled_mir(&decoded, &res).unwrap());
    assert_eq!(a.scene, build_compiled_scene(&decoded, &res).unwrap());
    let mut changed = decoded.clone();
    for data in changed.mir.data.values_mut() {
        let MirDataOperator::Inline { rows } = &mut data.operator;
        rows[0].insert("category".into(), "A\nA\nA".into());
    }
    for s in &mut chart_mut(&mut changed).scales {
        if let MirScale::Band { domain, .. } = s {
            domain[0] = "A\nA\nA".into();
        }
    }
    let old_ranges = chart(&changed).scales.clone();
    let e = rematerialize_compiled_mir(&changed, &res)
        .unwrap_err()
        .to_string();
    assert!(e.contains("VizHIR") || e.contains("original HIR"), "{e}");
    assert_eq!(old_ranges, chart(&changed).scales);
    let b = compile_with_context(&document(&["A\nA\nA", "B"]), &ctx, &res).unwrap();
    let patch = diff_scene(&a.scene, &b.scene, Revision(1), Revision(2), "categories").unwrap();
    assert_eq!(
        apply_scene_patch(&a.scene, Revision(1), &patch).unwrap().0,
        b.scene
    );
}
#[test]
fn title_and_categories_share_the_same_context_without_scope_or_budget_escape() {
    let (mut ctx, res) = context(90.);
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()
        .push(SemanticTextLayoutTarget::chart_title("c", 240., 8, 30.));
    let d = change(
        &document(&["中文字体测量\n测试", "AV office ffi\nCafé Å"]),
        |v| {
            v["views"][0]["title"] = "Measured title words\n\n中文测试".into();
            v["views"][0]["color"] = json!({"field":"group"});
        },
    );
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert!(matches!(
        find_scene_node(&c.scene.nodes, "c/title"),
        Some(SceneNode::Path { .. })
    ));
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}
#[test]
fn long_unbreakable_or_overfull_cells_and_small_plots_fail_without_shrink() {
    let (ctx, res) = context(1000.);
    let d = change(
        &document(&["ABCDEFGHIJKLMNOPQRSTUVWXYZ", "B", "C", "D"]),
        |v| v["views"][0]["frame"]["width"] = 300.into(),
    );
    assert!(
        compile_with_context(&d, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("unbreakable")
    );
    let d = change(&document(&["A\nA\nA\nA\nA\nA", "B"]), |v| {
        v["views"][0]["frame"]["height"] = 230.into()
    });
    assert!(compile_with_context(&d, &ctx, &res).is_err());
    let (ctx, res) = context(30.);
    for source in ["missing 😀", "foo\u{ad}bar", "Aא", "A\tB"] {
        assert!(compile_with_context(&document(&[source, "B"]), &ctx, &res).is_err());
    }
}

#[test]
fn authored_width_is_preserved_while_available_cell_also_selects_legal_breaks() {
    let (mut ctx, res) = context(10000.);
    let d = change(&document(&["AV AV AV AV AV", "B"]), |v| {
        v["views"][0]["frame"]["width"] = 200.into()
    });
    let available = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(
        available
            .mir
            .context
            .text_layout
            .as_ref()
            .unwrap()
            .semantic_targets
            .as_ref()
            .unwrap()[0]
            .max_width,
        10000.
    );
    let a = category(&available.scene, 0)
        .1
        .explanation
        .split(" baseline ")
        .count();
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .max_width = 20.;
    let narrow = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(
        narrow
            .mir
            .context
            .text_layout
            .as_ref()
            .unwrap()
            .semantic_targets
            .as_ref()
            .unwrap()[0]
            .max_width,
        20.
    );
    assert!(
        category(&narrow.scene, 0)
            .1
            .explanation
            .split(" baseline ")
            .count()
            > a
    );
    let d = change(&document(&["ABCDEFGHIJKLMNOPQRSTUVWXYZ", "B"]), |v| {
        v["views"][0]["frame"]["width"] = 200.into()
    });
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .max_width = 10000.;
    assert!(
        compile_with_context(&d, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("unbreakable")
    );
}
