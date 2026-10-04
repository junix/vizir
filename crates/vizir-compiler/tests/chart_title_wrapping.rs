#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    CompilationContext, MaterializationLimits, SemanticTextLayoutTarget, TextLayoutContext,
    TextLimits, build_compiled_scene, compile_with_context, compile_with_context_with_limits,
    parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{
    Document, MirScale, MirView, Revision, SceneNode, apply_scene_patch, diff_scene,
    find_scene_node,
};
fn document(kind: &str, title: Option<&str>, legend: bool) -> Document {
    let mut view = json!({"kind":kind,"id":"c","dataset":"data","frame":{"x":10,"y":10,"width":620,"height":600}});
    if let Some(title) = title {
        view["title"] = title.into();
    }
    if kind == "chart.bar" {
        view["category"] = json!({"field":"city","label":"城市"});
        view["value"] = json!({"field":"n","label":"value"});
    } else {
        view["x"] = json!({"field":"x","label":"x"});
        view["y"] = json!({"field":"n","label":"value"});
    }
    if legend {
        view[if kind == "chart.line" {
            "series"
        } else {
            "color"
        }] = json!({"field":"group"});
    }
    serde_json::from_value(json!({"version":"0.2","id":"title-wrap","width":650,"height":640,"datasets":{"data":{"key":"id","rows":[{"id":"z","city":"北京","x":0,"n":2,"group":"Alpha"},{"id":"a","city":"上海","x":1,"n":4,"group":"Alpha"},{"id":"y","city":"深圳","x":0,"n":3,"group":"Beta"},{"id":"b","city":"广州","x":1,"n":5,"group":"Beta"}]}},"views":[view]})).unwrap()
}
fn layout(width: f64) -> TextLayoutContext {
    TextLayoutContext::new(vec![]).with_semantic_targets(vec![
        SemanticTextLayoutTarget::chart_title("c", width, 12, 30.),
    ])
}
fn context(width: f64) -> (CompilationContext, vizir_compiler::FontResources) {
    let (c, r) = fixtures::profile();
    (c.with_text_layout(layout(width)), r)
}
fn change(d: &Document, f: impl FnOnce(&mut Value)) -> Document {
    let mut v = serde_json::to_value(d).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
fn path<'a>(
    c: &'a vizir_core::Scene2D,
    id: &str,
) -> (&'a vizir_core::Rect, &'a vizir_core::Origin) {
    let SceneNode::Path { bounds, origin, .. } = find_scene_node(&c.nodes, id).unwrap() else {
        panic!()
    };
    (bounds, origin)
}
#[test]
fn all_chart_roles_wrap_complete_source_at_existing_baseline_and_weight() {
    let (ctx, res) = context(180.);
    for kind in ["chart.bar", "chart.line", "chart.scatter"] {
        let title = "中文（字体测量），保留标点。\n\nAV office ffi Café Å A\u{30a}\n";
        let c = compile_with_context(&document(kind, Some(title), true), &ctx, &res).unwrap();
        let (_, origin) = path(&c.scene, "c/title");
        assert_eq!(origin.hir_node, "c");
        assert!(origin.explanation.contains("chart.title"));
        assert!(origin.explanation.contains("baseline 38 "));
        assert!(origin.explanation.ends_with(title));
        let MirView::Chart(chart) = &c.mir.mir.views[0] else {
            panic!()
        };
        assert_eq!(chart.title.as_deref(), Some(title));
        assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
    }
}
#[test]
fn measured_block_changes_ranges_before_materialization_without_data_or_domain_changes() {
    let (ctx, res) = context(150.);
    let mut old = ctx.clone();
    old.text_layout = None;
    let source = document(
        "chart.bar",
        Some("Measured chart title with several words"),
        true,
    );
    let a = compile_with_context(&source, &old, &res).unwrap();
    let b = compile_with_context(&source, &ctx, &res).unwrap();
    assert_eq!(a.mir.mir.data, b.mir.mir.data);
    assert_eq!(a.mir.mir.expressions, b.mir.mir.expressions);
    let (MirView::Chart(a), MirView::Chart(b)) = (&a.mir.mir.views[0], &b.mir.mir.views[0]) else {
        panic!()
    };
    assert_eq!(a.mark, b.mark);
    for (x, y) in a.scales.iter().zip(&b.scales) {
        if let (
            MirScale::Linear {
                domain: a,
                range: ar,
                ..
            },
            MirScale::Linear {
                domain: b,
                range: br,
                ..
            },
        ) = (x, y)
        {
            assert_eq!(a, b);
            assert_eq!(ar[0], br[0]);
            assert!(br[1] > ar[1]);
        }
    }
}
#[test]
fn title_and_legend_can_share_row_only_when_full_envelopes_are_separated() {
    let (ctx, res) = context(220.);
    let compact =
        compile_with_context(&document("chart.bar", Some("AV"), true), &ctx, &res).unwrap();
    let (legend, _) = path(&compact.scene, "c/legend/0/label");
    let (title, _) = path(&compact.scene, "c/title");
    assert!(legend.x > title.x + title.width + 8.);
    assert!(legend.y < title.y + title.height);
    let rows = compile_with_context(
        &change(
            &document(
                "chart.bar",
                Some("Measured chart title words\n\nCafé Å"),
                true,
            ),
            |v| v["views"][0]["frame"]["width"] = 340.into(),
        ),
        &ctx,
        &res,
    )
    .unwrap();
    let (legend, _) = path(&rows.scene, "c/legend/0/label");
    let (title, _) = path(&rows.scene, "c/title");
    assert!(legend.y > title.y + title.height + 7.);
}
#[test]
fn empty_title_is_real_block_but_missing_title_or_wrong_dialect_is_rejected() {
    let (ctx, res) = context(180.);
    for title in ["", "\n", "\n\n"] {
        let c =
            compile_with_context(&document("chart.bar", Some(title), false), &ctx, &res).unwrap();
        assert!(
            path(&c.scene, "c/title")
                .1
                .explanation
                .contains("source byte coverage")
        );
    }
    assert!(
        compile_with_context(&document("chart.bar", None, false), &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("existing source title")
    );
    let d = change(
        &document("chart.bar", None, false),
        |v| v["views"][0] = json!({"kind":"geometry.scene","id":"c","title":"AV","frame":{"x":10,"y":10,"width":620,"height":600},"children":[]}),
    );
    assert!(
        compile_with_context(&d, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("bar, line or scatter")
    );
}
#[test]
fn newlines_are_scoped_to_title_not_legend_axis_or_other_view() {
    let (ctx, res) = context(180.);
    let d = document("chart.bar", Some("AV\n中文"), true);
    for kind in ["axis", "legend", "other"] {
        let d = change(&d, |v| match kind {
            "axis" => v["views"][0]["value"]["label"] = "AV\n中文".into(),
            "legend" => v["datasets"]["data"]["rows"][0]["group"] = "AV\n中文".into(),
            _ => {
                let mut other = v["views"][0].clone();
                other["id"] = "other".into();
                v["views"].as_array_mut().unwrap().push(other);
            }
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
fn cached_plan_does_not_repeat_candidates_or_logical_lines_across_phases() {
    let (ctx, res) = context(180.);
    let mut limits = TextLimits::new();
    limits.max_wrap_candidates = 1;
    limits.max_layout_lines = 1;
    let c = compile_with_context_with_limits(
        &document("chart.bar", Some("AV"), true),
        &ctx,
        &res,
        MaterializationLimits::default(),
        limits,
    )
    .unwrap();
    assert!(path(&c.scene, "c/title").0.width > 0.);
    limits.max_wrap_candidates = 0;
    assert!(
        compile_with_context_with_limits(
            &document("chart.bar", Some("AV"), true),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits
        )
        .is_err()
    );
}
#[test]
fn replay_refresh_preserve_ranges_and_reflow_requires_original_hir() {
    let (ctx, res) = context(180.);
    let c = compile_with_context(&document("chart.bar", Some("AV"), false), &ctx, &res).unwrap();
    let decoded = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
    assert_eq!(c.scene, build_compiled_scene(&decoded, &res).unwrap());
    assert_eq!(decoded, rematerialize_compiled_mir(&decoded, &res).unwrap());
    let mut changed = decoded.clone();
    let MirView::Chart(chart) = &mut changed.mir.views[0] else {
        panic!()
    };
    chart.title = Some("AV\n\nAV\n中文".into());
    let ranges = chart.scales.clone();
    let e = rematerialize_compiled_mir(&changed, &res)
        .unwrap_err()
        .to_string();
    assert!(e.contains("VizHIR") || e.contains("original HIR"), "{e}");
    let MirView::Chart(chart) = &changed.mir.views[0] else {
        panic!()
    };
    assert_eq!(ranges, chart.scales);
    let b =
        compile_with_context(&document("chart.bar", Some("AV\n中文"), false), &ctx, &res).unwrap();
    let patch = diff_scene(&c.scene, &b.scene, Revision(1), Revision(2), "title").unwrap();
    assert_eq!(
        apply_scene_patch(&c.scene, Revision(1), &patch).unwrap().0,
        b.scene
    );
}
#[test]
fn tiny_remaining_plot_and_final_fractional_frame_are_checked() {
    let (ctx, res) = context(180.);
    let d = document("chart.line", Some("AV\n\nCafé Å A\u{30a}\n中文\n"), true);
    let small = change(&d, |v| v["views"][0]["frame"]["height"] = 250.into());
    assert!(compile_with_context(&small, &ctx, &res).is_err());
    let fractional = change(&d, |v| {
        v["views"][0]["frame"]["x"] = 10.123456.into();
        v["views"][0]["frame"]["y"] = 10.123456.into();
        v["views"][0]["frame"]["width"] = 620.123456.into();
        v["views"][0]["frame"]["height"] = 600.123456.into();
    });
    let c = compile_with_context(&fractional, &ctx, &res).unwrap();
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
    assert!(
        path(&c.scene, "c/title")
            .1
            .explanation
            .contains("baseline 38.1235 ")
    );
}
#[test]
fn title_metadata_and_candidate_cache_remain_whole_call_bounded() {
    let (ctx, res) = context(180.);
    let d = document("chart.bar", Some("AV office ffi words\n中文"), true);
    for field in ["bytes", "labels", "cache", "commands"] {
        let mut l = TextLimits::new();
        match field {
            "bytes" => l.max_text_bytes = 40,
            "labels" => l.max_labels = 3,
            "cache" => l.max_cache_bytes = 100,
            _ => l.max_outline_commands = 50,
        };
        let e =
            compile_with_context_with_limits(&d, &ctx, &res, MaterializationLimits::default(), l)
                .unwrap_err()
                .to_string();
        assert!(e.contains("VIZ-TEXT-0003"), "{field}: {e}");
    }
}

#[test]
fn semantic_title_and_geometry_targets_execute_together_without_identity_parsing() {
    let (mut ctx, res) = context(180.);
    ctx.text_layout
        .as_mut()
        .unwrap()
        .semantic_targets
        .as_mut()
        .unwrap()[0]
        .view_id = "c/one".into();
    ctx.text_layout
        .as_mut()
        .unwrap()
        .targets
        .push(vizir_compiler::TextLayoutTarget::new(
            "notes", "a/b", 160., 5, 24.,
        ));
    let d = change(&document("chart.bar", Some("AV\n中文"), false), |v| {
        v["width"] = 980.into();
        v["views"][0]["id"] = "c/one".into();
        v["views"].as_array_mut().unwrap().push(json!({"kind":"geometry.scene","id":"notes","frame":{"x":650,"y":10,"width":310,"height":300},"children":[{"type":"text","id":"a/b","x":20,"y":40,"font_size":16,"anchor":"start","text":"中文测试\nAV office"}]}));
    });
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert!(
        path(&c.scene, "c/one/title")
            .1
            .explanation
            .contains("chart.title")
    );
    assert!(
        path(&c.scene, "notes/a/b")
            .1
            .explanation
            .contains("vizir-text-wrap/1")
    );
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}
