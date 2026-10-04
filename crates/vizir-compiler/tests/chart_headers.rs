use std::path::PathBuf;

use vizir_compiler::{Compilation, build_scene, compile};
use vizir_core::{
    Document, MirScale, MirView, Rect, SceneNode, View, find_scene_node, parse_document,
};

fn example(name: &str) -> Document {
    parse_document(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name),
    )
    .unwrap()
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

fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn assert_headers_fit(compilation: &Compilation) {
    for view in &compilation.mir.views {
        let MirView::Chart(chart) = view else {
            continue;
        };
        let SceneNode::Group { children, .. } =
            find_scene_node(&compilation.scene.nodes, &chart.id).unwrap()
        else {
            panic!("chart group")
        };
        let header = children
            .iter()
            .filter(|node| {
                node.id() == format!("{}/title", chart.id)
                    || node.id().contains("/legend/")
                    || node.id() == format!("{}/axis/y/title", chart.id)
            })
            .collect::<Vec<_>>();
        let top_grid =
            bounds(find_scene_node(children, &format!("{}/grid/y/0", chart.id)).unwrap()).y;
        for (index, a) in header.iter().enumerate() {
            let a_bounds = bounds(a);
            assert!(
                a_bounds.x >= chart.frame.x
                    && a_bounds.x + a_bounds.width <= chart.frame.x + chart.frame.width,
                "{} clips horizontally: {a_bounds:?}",
                a.id()
            );
            assert!(
                a_bounds.y >= chart.frame.y && a_bounds.y + a_bounds.height < top_grid,
                "{} clips or enters plot: {a_bounds:?}",
                a.id()
            );
            for b in &header[index + 1..] {
                assert!(
                    !intersects(a_bounds, bounds(b)),
                    "{} overlaps {}",
                    a.id(),
                    b.id()
                );
            }
        }
        for scale in &chart.scales {
            if let MirScale::Linear { id, range, .. } = scale
                && (id.ends_with("/y") || id.ends_with("/value"))
            {
                assert_eq!(range[1], top_grid, "MIR scale and scene header disagree");
            }
        }
    }
}

#[test]
fn service_health_headers_do_not_overlap() {
    let result = compile(&example("chart/service-health.viz.yaml")).unwrap();
    let right_title = find_scene_node(&result.scene.nodes, "availability-ranking/title").unwrap();
    let first_label =
        find_scene_node(&result.scene.nodes, "availability-ranking/legend/0/label").unwrap();
    assert!(
        !intersects(bounds(right_title), bounds(first_label)),
        "right chart title overlaps its first legend label"
    );
    assert_headers_fit(&result);
    let title = find_scene_node(&result.scene.nodes, "availability-ranking/title").unwrap();
    assert!(
        matches!(title, SceneNode::Text { text, .. } if text == "30-day error-budget consumption")
    );
    for chart in ["latency-risk", "availability-ranking"] {
        for index in 0..5 {
            assert!(
                find_scene_node(
                    &result.scene.nodes,
                    &format!("{chart}/legend/{index}/label")
                )
                .is_some()
            );
        }
    }
    assert_eq!(build_scene(&result.mir).unwrap(), result.scene);
}

#[test]
fn gallery_chart_headers_and_scale_ranges_share_one_layout() {
    for file in [
        "chart/service-health.viz.yaml",
        "chart/incident-latency.viz.yaml",
        "chart/sales-regions.viz.yaml",
        "mixed/reliability-brief.viz.yaml",
        "mixed/capacity-planning.viz.yaml",
        "chart/model-evaluation.viz.yaml",
    ] {
        assert_headers_fit(&compile(&example(file)).unwrap());
    }
}

#[test]
fn legend_rows_keep_long_mixed_width_unicode_and_combining_labels() {
    let mut document = example("chart/service-health.viz.yaml");
    let labels = [
        "WWWWWW Wide infrastructure",
        "iiiiii narrow",
        "東京データ部",
        "Cafe\u{301} operations",
    ];
    for (index, row) in document
        .datasets
        .get_mut("services")
        .unwrap()
        .rows
        .iter_mut()
        .enumerate()
    {
        row.insert(
            "team".to_owned(),
            serde_json::json!(labels[index % labels.len()]),
        );
    }
    let result = compile(&document).unwrap();
    assert_headers_fit(&result);
    let encoded = serde_json::to_string(&result.scene).unwrap();
    for label in labels {
        assert!(encoded.contains(label));
    }
    assert_eq!(compile(&document).unwrap().scene, result.scene);
}

#[test]
fn header_layout_rejects_overwide_text_and_too_small_frames() {
    for (case, expected) in [
        ("title", "title needs"),
        ("legend", "legend label"),
        ("x-axis", "x-axis title needs"),
        ("y-axis", "y-axis title needs"),
        ("width", "64px by 64px plot"),
        ("height", "64px by 64px plot"),
    ] {
        let mut document = example("chart/service-health.viz.yaml");
        document.views.truncate(1);
        let View::Scatter(chart) = &mut document.views[0] else {
            panic!("scatter")
        };
        match case {
            "title" => chart.title = Some("W".repeat(100)),
            "legend" => {
                document.datasets.get_mut("services").unwrap().rows[0]
                    .insert("team".to_owned(), serde_json::json!("東京".repeat(100)));
            }
            "x-axis" => chart.x.label = Some("W".repeat(100)),
            "y-axis" => chart.y.label = Some("W".repeat(100)),
            "width" => {
                chart.frame.width = 150.0;
                chart.title = None;
                chart.color = None;
                chart.x.label = Some("x".to_owned());
                chart.y.label = Some("y".to_owned());
            }
            "height" => chart.frame.height = 150.0,
            _ => unreachable!(),
        }
        let error = compile(&document).unwrap_err().to_string();
        assert!(
            error.contains("VIZ-LAYOUT-0004") && error.contains(expected),
            "{case}: {error}"
        );
    }
}

#[test]
fn compact_legend_positions_and_headerless_plot_ranges_are_retained() {
    let mut document = example("chart/service-health.viz.yaml");
    document.views.truncate(1);
    let View::Scatter(chart) = &mut document.views[0] else {
        panic!("scatter")
    };
    chart.title = Some("Short".to_owned());
    for row in &mut document.datasets.get_mut("services").unwrap().rows {
        row.insert("team".to_owned(), serde_json::json!("A"));
    }
    let result = compile(&document).unwrap();
    let label = find_scene_node(&result.scene.nodes, "latency-risk/legend/0/label").unwrap();
    assert!(
        matches!(label, SceneNode::Text { position, .. } if position.x == 677.0 && position.y == 21.0)
    );
    assert_headers_fit(&result);
    let View::Scatter(chart) = &mut document.views[0] else {
        unreachable!()
    };
    chart.title = None;
    chart.color = None;
    let result = compile(&document).unwrap();
    let MirView::Chart(chart) = &result.mir.views[0] else {
        unreachable!()
    };
    assert!(matches!(&chart.scales[1], MirScale::Linear { range, .. } if *range == [658.0, 50.0]));
}

#[test]
fn stale_mir_ranges_are_diagnosed_instead_of_misaligning_marks() {
    let mut result = compile(&example("chart/service-health.viz.yaml")).unwrap();
    let MirView::Chart(chart) = &mut result.mir.views[0] else {
        unreachable!()
    };
    let MirScale::Linear { range, .. } = &mut chart.scales[1] else {
        unreachable!()
    };
    range[1] = 50.0;
    assert!(
        build_scene(&result.mir)
            .unwrap_err()
            .to_string()
            .contains("VIZ-LAYOUT-0005")
    );
}

#[test]
fn translated_narrow_frames_wrap_all_legend_cardinalities() {
    for count in [0, 1, 2, 3, 4, 7, 12] {
        let mut document = example("chart/service-health.viz.yaml");
        document.views.truncate(1);
        let View::Scatter(chart) = &mut document.views[0] else {
            unreachable!()
        };
        chart.title = Some("WWW iii 東京".to_owned());
        chart.frame.x = 75.0;
        chart.frame.y = 40.0;
        chart.frame.width = 320.0;
        chart.frame.height = 640.0;
        if count == 0 {
            chart.color = None;
        }
        for (index, row) in document
            .datasets
            .get_mut("services")
            .unwrap()
            .rows
            .iter_mut()
            .enumerate()
        {
            row.insert(
                "team".to_owned(),
                serde_json::json!(format!("Team {} wide WWW", index % count.max(1))),
            );
        }
        let result = compile(&document).unwrap();
        assert_headers_fit(&result);
        let SceneNode::Group { children, .. } = &result.scene.nodes[0] else {
            unreachable!()
        };
        assert_eq!(
            children
                .iter()
                .filter(|node| node.id().contains("/legend/") && node.id().ends_with("/label"))
                .count(),
            count
        );
    }
}

#[test]
fn fractional_frame_mir_json_round_trips_preserve_guide_and_mark_alignment() {
    for source in [
        "chart/service-health.viz.yaml",
        "chart/incident-latency.viz.yaml",
    ] {
        let base = example(source);
        for index in 1..=1000 {
            let mut document = base.clone();
            for view in &mut document.views {
                let frame = match view {
                    View::Scatter(chart) => &mut chart.frame,
                    View::Line(chart) => &mut chart.frame,
                    View::Bar(chart) => &mut chart.frame,
                    _ => unreachable!(),
                };
                frame.x = index as f64 / 13.0;
                frame.y = index as f64 / 17.0;
                frame.width = 760.0 + index as f64 / 19.0;
                frame.height = 720.0 + index as f64 / 23.0;
            }
            let mut result = compile(&document).unwrap();
            result.mir = serde_json::from_slice(&serde_json::to_vec(&result.mir).unwrap()).unwrap();
            result.scene = build_scene(&result.mir)
                .unwrap_or_else(|error| panic!("{source} case {index}: {error}"));
            assert_headers_fit(&result);
        }
    }
}

#[test]
fn translated_mir_rounding_does_not_mask_stale_endpoint_changes() {
    for (x, y) in [
        (0.0, 0.0),
        (-64.00000000000001, -84.12500000000001),
        (-1000.0 / 13.0, -1000.0 / 17.0),
    ] {
        let mut document = example("chart/service-health.viz.yaml");
        document.views.truncate(1);
        let View::Scatter(chart) = &mut document.views[0] else {
            unreachable!()
        };
        chart.frame.x = x;
        chart.frame.y = y;
        let result = compile(&document).unwrap();
        let mir: vizir_core::VizMir =
            serde_json::from_slice(&serde_json::to_vec(&result.mir).unwrap()).unwrap();
        build_scene(&mir).unwrap();
        for scale_index in 0..2 {
            for endpoint in 0..2 {
                for delta in [-1.0, -0.000001, 0.000001, 1.0] {
                    let mut changed = mir.clone();
                    let MirView::Chart(chart) = &mut changed.views[0] else {
                        unreachable!()
                    };
                    let MirScale::Linear { range, .. } = &mut chart.scales[scale_index] else {
                        unreachable!()
                    };
                    range[endpoint] += delta;
                    assert!(
                        build_scene(&changed)
                            .unwrap_err()
                            .to_string()
                            .contains("VIZ-LAYOUT-0005"),
                        "x={x}, y={y}, scale={scale_index}, endpoint={endpoint}, delta={delta}"
                    );
                }
            }
        }
    }
}
