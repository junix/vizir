use serde_json::json;
use vizir_compiler::{Compilation, build_scene, compile};
use vizir_core::{Document, MirScale, MirView, PathCommand, SceneNode, find_scene_node};

fn document(kind: &str, values: &[f64]) -> Document {
    let rows = values.iter().enumerate().map(|(i, value)| {
        json!({"id": format!("row-{i}"), "category": format!("C{i}"), "x": value, "y": value})
    }).collect::<Vec<_>>();
    let mut view = json!({
        "kind": kind, "id": "chart", "dataset": "values", "title": "Tiny values",
        "frame": {"x": 0, "y": 0, "width": 640, "height": 400}
    });
    if kind == "chart.bar" {
        view["category"] = json!({"field": "category"});
        view["value"] = json!({"field": "y"});
    } else {
        view["x"] = json!({"field": "x"});
        view["y"] = json!({"field": "y"});
    }
    serde_json::from_value(json!({
        "version": "0.1", "id": "tiny-domains", "width": 640, "height": 400,
        "background": "transparent", "datasets": {"values": {"key": "id", "rows": rows}},
        "views": [view]
    }))
    .unwrap()
}

fn finite(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => fields.values().for_each(finite),
        serde_json::Value::Array(values) => values.iter().for_each(finite),
        serde_json::Value::Number(number) => assert!(number.as_f64().unwrap().is_finite()),
        serde_json::Value::Null => panic!("non-finite coordinate serialized as null"),
        _ => {}
    }
}

fn assert_guides_and_scales(result: &Compilation, raw: [f64; 2]) {
    let MirView::Chart(chart) = &result.mir.views[0] else {
        panic!("chart")
    };
    for scale in &chart.scales {
        if let MirScale::Linear {
            domain,
            range,
            zero,
            ..
        } = scale
        {
            assert!(domain[0].is_finite() && domain[1].is_finite());
            assert!(domain[0] <= raw[0] && domain[1] >= raw[1]);
            assert!(domain[0] < domain[1]);
            let raw_span = if *zero {
                raw[1].max(0.0) - raw[0].min(0.0)
            } else {
                raw[1] - raw[0]
            };
            assert!((domain[1] - domain[0]) / raw_span <= 2.0, "{domain:?}");
            assert!(range.iter().all(|value| value.is_finite()));
        }
    }
    // Ticks and grid positions must remain finite, spaced and aligned with the
    // stored ranges. Legacy label text intentionally remains a formatting policy.
    let MirScale::Linear { range: y_range, .. } = &chart.scales[1] else {
        panic!("y scale")
    };
    let mut previous = f64::NEG_INFINITY;
    for index in 0..=5 {
        let SceneNode::Line { from, to, .. } =
            find_scene_node(&result.scene.nodes, &format!("chart/grid/y/{index}")).unwrap()
        else {
            panic!("grid")
        };
        let expected = y_range[1] + (y_range[0] - y_range[1]) * (index as f64 / 5.0);
        assert_eq!(from.y, expected);
        assert_eq!(to.y, expected);
        assert!(from.y > previous);
        previous = from.y;
        let SceneNode::Text { position, text, .. } =
            find_scene_node(&result.scene.nodes, &format!("chart/axis/y/label/{index}")).unwrap()
        else {
            panic!("tick")
        };
        assert_eq!(position.y, expected + 4.0);
        assert!(!text.contains("NaN") && !text.contains("inf"));
    }
    finite(&serde_json::to_value(&result.scene).unwrap());
    // JSON input reconstruction can round independently; header range
    // validation must continue to accept this ordinary serialization path.
    let restored = serde_json::from_slice(&serde_json::to_vec(&result.mir).unwrap()).unwrap();
    let scene = build_scene(&restored).unwrap();
    finite(&serde_json::to_value(scene).unwrap());
}

fn assert_point_extent(kind: &str, values: &[f64]) {
    let result = compile(&document(kind, values)).unwrap();
    assert_guides_and_scales(&result, [values[0], *values.last().unwrap()]);
    let mut previous = None;
    let mut centers = Vec::new();
    for (index, _) in values.iter().enumerate() {
        let SceneNode::Circle { center, .. } =
            find_scene_node(&result.scene.nodes, &format!("chart/point/row-{index}")).unwrap()
        else {
            panic!("point")
        };
        assert!(center.x.is_finite() && center.y.is_finite());
        if let Some((x, y)) = previous {
            assert!(
                center.x > x && center.y < y,
                "distinct data collapsed for {kind}: {centers:?}"
            );
        }
        previous = Some((center.x, center.y));
        centers.push(*center);
    }
    assert!(centers.last().unwrap().x - centers[0].x > 200.0);
    assert!(centers[0].y - centers.last().unwrap().y > 100.0);
    if kind == "chart.line" {
        let SceneNode::Path {
            bounds, commands, ..
        } = find_scene_node(&result.scene.nodes, "chart/series/series").unwrap()
        else {
            panic!("line")
        };
        assert!(bounds.width > 200.0 && bounds.height > 100.0);
        for (command, expected) in commands.iter().zip(&centers) {
            let (PathCommand::Move { to } | PathCommand::Line { to }) = command else {
                panic!("command")
            };
            assert_eq!(to, expected);
        }
    }
}

#[test]
fn tiny_positive_negative_and_cross_zero_scatter_and_line_keep_visible_extent() {
    for kind in ["chart.scatter", "chart.line"] {
        for values in [
            [1e-120, 2e-120, 3e-120],
            [-3e-120, -2e-120, -1e-120],
            [-1e-120, 0.0, 1e-120],
        ] {
            assert_point_extent(kind, &values);
        }
    }
}

#[test]
fn tiny_subnormal_and_minimum_step_points_keep_visible_extent() {
    let smallest = f64::from_bits(1);
    for kind in ["chart.scatter", "chart.line"] {
        for values in [
            vec![smallest, 2.0 * smallest, 3.0 * smallest],
            vec![-3.0 * smallest, -2.0 * smallest, -smallest],
            vec![-smallest, 0.0, smallest],
            vec![0.0, smallest],
            vec![1e-320, 2e-320, 3e-320],
        ] {
            assert_point_extent(kind, &values);
        }
    }
}

#[test]
fn tiny_signed_and_subnormal_bars_keep_nonzero_finite_heights() {
    let smallest = f64::from_bits(1);
    for values in [
        [1e-120, 2e-120, 3e-120],
        [-3e-120, -2e-120, -1e-120],
        [-1e-120, 0.0, 1e-120],
        [smallest, 2.0 * smallest, 3.0 * smallest],
        [-smallest, 0.0, smallest],
    ] {
        let result = compile(&document("chart.bar", &values)).unwrap();
        assert_guides_and_scales(&result, [values[0], values[2]]);
        for (index, value) in values.into_iter().enumerate() {
            let SceneNode::Rect { bounds, .. } =
                find_scene_node(&result.scene.nodes, &format!("chart/bar/row-{index}")).unwrap()
            else {
                panic!("bar")
            };
            assert!(bounds.width > 0.0 && bounds.height.is_finite());
            if value == 0.0 {
                assert_eq!(bounds.height, 0.0);
            } else {
                assert!(bounds.height > 50.0, "{bounds:?}");
            }
        }
    }
}
