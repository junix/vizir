#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, MaterializationLimits, SemanticTextLayoutTarget, TextLayoutContext,
    TextLimits, build_compiled_scene, compile_with_context, compile_with_context_with_limits,
    parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{Document, MirDataOperator, MirScale, MirView, SceneNode, find_scene_node};
const LABELS: [&str; 6] = [
    "中文字体测量测试",
    "中文测试字体测量",
    "字体中文测量测试",
    "测试中文字体测量",
    "测量字体中文测试",
    "字体测量中文测试",
];
fn document(labels: &[&str]) -> Document {
    let rows: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, x)| json!({"id":format!("key-{i}"),"x":x,"y":"测试","v":i+1}))
        .collect();
    serde_json::from_value(json!({"version":"0.5","id":"heatmap-wrap","width":720,"height":400,"datasets":{"d":{"key":"id","rows":rows}},"views":[{"kind":"chart.heatmap","id":"h","title":"中文测试","dataset":"d","frame":{"x":0,"y":0,"width":720,"height":400},"x":{"field":"x","label":"字体"},"y":{"field":"y","label":"测试"},"color":{"field":"v","label":"测量","domain":[0,10]}}]})).unwrap()
}
fn context(width: f64) -> (CompilationContext, vizir_compiler::FontResources) {
    let (c, r) = fixtures::profile();
    (
        c.with_text_layout(TextLayoutContext::new(vec![]).with_heatmap_x_labels(vec![
            SemanticTextLayoutTarget::heatmap_x_category_labels("h", width, 4, 16.),
        ])),
        r,
    )
}
fn chart(c: &vizir_compiler::CompiledMir) -> &vizir_core::MirChart {
    let MirView::Chart(c) = &c.mir.views[0] else {
        panic!()
    };
    c
}
fn change(d: &Document, f: impl FnOnce(&mut Value)) -> Document {
    let mut v = serde_json::to_value(d).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
fn path<'a>(
    s: &'a vizir_core::Scene2D,
    id: &str,
) -> (&'a vizir_core::Rect, &'a vizir_core::Origin) {
    let SceneNode::Path { bounds, origin, .. } = find_scene_node(&s.nodes, id).unwrap() else {
        panic!()
    };
    (bounds, origin)
}
fn spans(origin: &vizir_core::Origin) -> Vec<(std::ops::Range<usize>, f64)> {
    origin
        .explanation
        .split("source byte coverage [")
        .nth(1)
        .unwrap()
        .split("]; original text:")
        .next()
        .unwrap()
        .split("; ")
        .map(|s| {
            let p: Vec<_> = s.split(' ').collect();
            let r: Vec<usize> = p[0].split("..").map(|n| n.parse().unwrap()).collect();
            (r[0]..r[1], p[4].parse().unwrap())
        })
        .collect()
}
#[test]
fn narrow_cjk_wraps_with_exact_bands_source_coverage_and_reserved_bottom() {
    let d = document(&LABELS);
    let (ctx, res) = context(60.);
    let (old, _) = fixtures::profile();
    let unwrapped = compile_with_context(&d, &old, &res).unwrap();
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(unwrapped.mir.mir.data, c.mir.mir.data);
    assert_eq!(chart(&unwrapped.mir).mark, chart(&c.mir).mark);
    assert_eq!(chart(&unwrapped.mir).scales[0], chart(&c.mir).scales[0]);
    assert_eq!(chart(&unwrapped.mir).scales[2], chart(&c.mir).scales[2]);
    let MirScale::Band { range, .. } = &chart(&c.mir).scales[0] else {
        panic!()
    };
    let MirScale::Band { range: yrange, .. } = &chart(&c.mir).scales[1] else {
        panic!()
    };
    let title = path(&c.scene, "h/axis/x/title").0;
    let MirScale::Band { range: old_y, .. } = &chart(&unwrapped.mir).scales[1] else {
        panic!()
    };
    assert!(old_y[1] > yrange[1]);
    for (i, source) in LABELS.iter().enumerate() {
        let (b, o) = path(&c.scene, &format!("h/axis/x/category/{i}"));
        let spans = spans(o);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].1 - spans[0].1, 16.);
        let rebuilt: String = spans.iter().map(|(r, _)| &source[r.clone()]).collect();
        assert_eq!(&rebuilt, source);
        assert!(
            o.explanation
                .contains("heatmap.x_category_labels wrapping vizir-text-wrap/5")
        );
        assert_eq!(o.hir_node, "h");
        assert_eq!(o.mir_node, "h/guides/x-axis");
        assert!(!o.data_lineage.is_empty());
        let round = |x: f64| format!("{x:.4}").parse::<f64>().unwrap();
        assert!(b.x >= round(range[0] + (range[1] - range[0]) * i as f64 / 6.) + 4.);
        assert!(
            b.x + b.width <= round(range[0] + (range[1] - range[0]) * (i + 1) as f64 / 6.) - 4.
        );
        assert!(b.y >= round(yrange[1]) + 8.);
        assert!(b.y + b.height <= title.y - 8.);
    }
    let decoded = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(c.scene, build_compiled_scene(&decoded, &res).unwrap());
    assert_eq!(decoded, rematerialize_compiled_mir(&decoded, &res).unwrap());
    assert_eq!(c.scene.background.0, "transparent");
}
#[test]
fn latin_combining_graphemes_spaces_and_punctuation_keep_all_bytes() {
    let labels = [
        "office e\u{301} office e\u{301}",
        "AV office ffi Café Å",
        "中文（字体测量），测试。",
        " A  B  A  B ",
    ];
    let (ctx, res) = context(60.);
    let c = compile_with_context(&document(&labels), &ctx, &res).unwrap();
    for (i, source) in labels.iter().enumerate() {
        let (_, o) = path(&c.scene, &format!("h/axis/x/category/{i}"));
        let ranges = spans(o);
        let rebuilt: String = ranges.iter().map(|(r, _)| &source[r.clone()]).collect();
        assert_eq!(&rebuilt, source);
        for (r, _) in ranges {
            assert!(!source[r.end..].starts_with('\u{301}'));
        }
    }
}
#[test]
fn old_profiles_reject_heatmap_role_and_v5_requires_it() {
    let (ctx, _) = context(60.);
    let layout = ctx.text_layout.unwrap();
    for profile in 1..=4 {
        let mut v = serde_json::to_value(&layout).unwrap();
        v["profile"] = format!("vizir-text-wrap/{profile}").into();
        assert!(serde_json::from_value::<TextLayoutContext>(v).is_err());
    }
    let mut v = serde_json::to_value(&layout).unwrap();
    v["semantic_targets"] = json!([]);
    assert!(serde_json::from_value::<TextLayoutContext>(v).is_err());
    for field in ["semantic_targets", "diagram_targets"] {
        let mut v = serde_json::to_value(&layout).unwrap();
        v[field] = Value::Null;
        assert!(serde_json::from_value::<TextLayoutContext>(v).is_err());
    }
}
#[test]
fn no_shrink_no_emergency_split_and_explicit_budgets_fail() {
    let (ctx, res) = context(60.);
    for label in [
        "unbreakablelongwordthatcannotpossiblyfit",
        "A\nB",
        "A\r\nB",
        "A\u{2028}B",
        "A\u{2029}B",
        "A\tB",
        "A\u{00ad}B",
        "missing 🦀",
    ] {
        assert!(
            compile_with_context(&document(&[label]), &ctx, &res).is_err(),
            "{label}"
        );
    }
    for (width, lines, height) in [
        (1., 4, 16.),
        (60., 1, 16.),
        (60., 4, 0.25),
        (10., 256, 100.),
    ] {
        let mut ctx = ctx.clone();
        let t = &mut ctx
            .text_layout
            .as_mut()
            .unwrap()
            .semantic_targets
            .as_mut()
            .unwrap()[0];
        t.max_width = width;
        t.max_lines = lines;
        t.line_height = height;
        assert!(compile_with_context(&document(&LABELS), &ctx, &res).is_err());
    }
    for name in [
        "bytes",
        "labels",
        "cache",
        "lines",
        "candidates",
        "glyphs",
        "commands",
        "collisions",
        "output",
    ] {
        let mut l = TextLimits::new();
        match name {
            "bytes" => l.max_text_bytes = 10,
            "labels" => l.max_labels = 2,
            "cache" => l.max_cache_bytes = 10,
            "lines" => l.max_layout_lines = 2,
            "candidates" => l.max_wrap_candidates = 1,
            "glyphs" => l.max_glyphs = 1,
            "commands" => l.max_outline_commands = 1,
            "collisions" => l.max_collision_checks = 1,
            _ => l.max_output_bytes = 10,
        };
        assert!(
            compile_with_context_with_limits(
                &document(&LABELS),
                &ctx,
                &res,
                MaterializationLimits::default(),
                l
            )
            .is_err(),
            "{name}"
        );
    }
}
#[test]
fn actual_band_limits_width_and_fractional_frames_are_safe() {
    let (ctx, res) = context(500.);
    let d = change(&document(&LABELS), |v| {
        let f = &mut v["views"][0]["frame"];
        f["x"] = 0.123456.into();
        f["y"] = 0.123456.into();
        f["width"] = 599.654321.into();
        f["height"] = 399.654321.into();
    });
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
    for i in 0..6 {
        assert_eq!(
            spans(path(&c.scene, &format!("h/axis/x/category/{i}")).1).len(),
            2
        );
    }
    let small = change(&d, |v| v["views"][0]["frame"]["height"] = 120.into());
    assert!(compile_with_context(&small, &ctx, &res).is_err());
}
#[test]
fn domain_order_absent_entries_and_opaque_bindings_remain_authoritative() {
    let (ctx, res) = context(60.);
    let d = change(&document(&["A A A A", "B B B B"]), |v| {
        v["views"][0]["x"]["domain"] = json!(["B B B B", "Empty Empty", "A A A A"])
    });
    let mut c = compile_with_context(&d, &ctx, &res).unwrap();
    assert!(
        path(&c.scene, "h/axis/x/category/0")
            .1
            .explanation
            .ends_with("B B B B")
    );
    assert!(
        path(&c.scene, "h/axis/x/category/1")
            .1
            .explanation
            .ends_with("Empty Empty")
    );
    let MirView::Chart(h) = &mut c.mir.mir.views[0] else {
        panic!()
    };
    for s in &mut h.scales {
        if let MirScale::Band { id, .. } = s
            && id == "h/x"
        {
            *id = "opaque/x".into();
        }
    }
    if let vizir_core::ChartMark::Heatmap { x, .. } = &mut h.mark {
        x.scale = "opaque/x".into();
    }
    for g in &mut h.guides {
        if g.scale == "h/x" {
            g.scale = "opaque/x".into();
            g.id = "opaque/guide".into();
        }
    }
    let replay = build_compiled_scene(&c.mir, &res).unwrap();
    assert_eq!(
        path(&replay, "h/axis/x/category/1").0,
        path(&c.scene, "h/axis/x/category/1").0
    );
}
#[test]
fn stale_layout_replay_and_refresh_fail_without_mutating_compiled_input() {
    let (ctx, res) = context(60.);
    let c = compile_with_context(&document(&["A", "B"]), &ctx, &res).unwrap();
    let mut changed = c.mir.clone();
    for data in changed.mir.data.values_mut() {
        let MirDataOperator::Inline { rows } = &mut data.operator;
        rows[0].insert("x".into(), "A A A A A A A A".into());
    }
    let MirView::Chart(h) = &mut changed.mir.views[0] else {
        panic!()
    };
    if let MirScale::Band { domain, .. } = &mut h.scales[0] {
        domain[0] = "A A A A A A A A".into();
    }
    let before = serde_json::to_vec(&changed).unwrap();
    assert!(rematerialize_compiled_mir(&changed, &res).is_err());
    assert_eq!(before, serde_json::to_vec(&changed).unwrap());
    let mut changed = c.mir.clone();
    changed
        .context
        .text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .line_height = 100.;
    // One-line labels do not use line spacing; changing to tall multi-line sources above must reflow.
    assert!(build_compiled_scene(&changed, &res).is_ok());
}
#[test]
fn roles_are_source_scoped_and_conflicting_or_missing_targets_fail() {
    let (mut ctx, res) = context(60.);
    let d = document(&LABELS);
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()
        .push(SemanticTextLayoutTarget::bar_category_labels(
            "h", 60., 4, 16.,
        ));
    assert!(compile_with_context(&d, &ctx, &res).is_err());
    let (mut ctx, _) = context(60.);
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .view_id = "missing".into();
    assert!(compile_with_context(&d, &ctx, &res).is_err());
    let (ctx, _) = context(60.);
    let bad = change(&d, |v| {
        v["datasets"]["d"]["rows"][0]["y"] = "verylong".repeat(150).into()
    });
    assert!(compile_with_context(&bad, &ctx, &res).is_err());
}

#[test]
fn twelve_cjk_characters_require_wrap_even_with_exact_face_metrics() {
    let labels: Vec<String> = LABELS.iter().map(|s| format!("{s}字体测量")).collect();
    let refs: Vec<_> = labels.iter().map(String::as_str).collect();
    let d = document(&refs);
    let (ctx, res) = context(60.);
    let (baseline, _) = fixtures::profile();
    assert!(
        compile_with_context(&d, &baseline, &res)
            .unwrap_err()
            .to_string()
            .contains("does not fit its band")
    );
    let mut ctx = ctx;
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .max_lines = 2;
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    for x in [0.123456, 10.123456, 1000.123456] {
        let translated = change(&d, |v| {
            v["width"] = (x + 721.).into();
            v["views"][0]["frame"]["x"] = x.into();
        });
        let c = compile_with_context(&translated, &ctx, &res).unwrap();
        for i in 0..6 {
            assert_eq!(
                spans(path(&c.scene, &format!("h/axis/x/category/{i}")).1).len(),
                2
            );
        }
    }
    let mut tight = ctx.clone();
    tight
        .text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .max_width = 60. - 1e-12;
    assert!(compile_with_context(&d, &tight, &res).is_err());
    for i in 0..6 {
        assert_eq!(
            spans(path(&c.scene, &format!("h/axis/x/category/{i}")).1).len(),
            2
        );
    }
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}

#[test]
fn title_and_heatmap_targets_share_header_and_bottom_budgets() {
    let (mut ctx, res) = context(60.);
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()
        .push(SemanticTextLayoutTarget::chart_title("h", 140., 4, 30.));
    let d = change(&document(&LABELS), |v| {
        v["views"][0]["title"] = "Measured title words 中文测试".into()
    });
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert!(
        path(&c.scene, "h/title")
            .1
            .explanation
            .contains("chart.title wrapping vizir-text-wrap/5")
    );
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}
