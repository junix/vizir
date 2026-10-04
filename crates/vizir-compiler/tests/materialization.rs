use serde_json::json;
use vizir_compiler::{
    MaterializationLimits, build_scene, build_scene_with_limits, lower_to_mir, rematerialize_mir,
    rematerialize_mir_with_limits,
};
use vizir_core::{
    ChartMark, Document, Expression, LiteralValue, MirChart, MirDataOperator, MirScale, MirView,
    PathCommand, PureFunction, SceneNode, TypedExpression, UpdateMode, ValueType, VizMir,
    find_scene_node,
};

fn document(kind: &str) -> Document {
    let mut view = json!({
        "kind": kind, "id": "chart", "dataset": "values",
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
        "version": "0.1", "id": "materialization", "width": 640, "height": 400,
        "background": "transparent",
        "datasets": {"values": {"key": "id", "rows": [
            {"id": "row-b", "category": "B", "x": 2.0, "y": 3.0, "group": "G", "rank": 1},
            {"id": "row-a", "category": "A", "x": 1.0, "y": 2.0, "group": "G", "rank": 1},
            {"id": "row-c", "category": "C", "x": 3.0, "y": 4.0, "group": "H", "rank": 0}
        ]}},
        "views": [view]
    }))
    .unwrap()
}

fn mir(kind: &str) -> VizMir {
    lower_to_mir(&document(kind)).unwrap()
}

fn chart(mir: &VizMir) -> &MirChart {
    let MirView::Chart(chart) = &mir.views[0] else {
        panic!("expected chart")
    };
    chart
}

fn chart_mut(mir: &mut VizMir) -> &mut MirChart {
    let MirView::Chart(chart) = &mut mir.views[0] else {
        panic!("expected chart")
    };
    chart
}

#[test]
fn stale_source_rows_are_rejected_at_scene_boundary() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        let source = chart(&input).source.clone();
        let MirDataOperator::Inline { rows } = &mut input.data.get_mut(&source).unwrap().operator;
        rows[0].insert("y".into(), json!(3.5));
        let before = serde_json::to_vec(&input).unwrap();
        assert!(
            build_scene(&input).is_err(),
            "{kind} trusted a stale cache after a source edit"
        );
        assert_eq!(serde_json::to_vec(&input).unwrap(), before);
    }
}

#[test]
fn cache_only_numeric_edits_are_rejected_at_scene_boundary() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        match &mut chart_mut(&mut input).mark {
            ChartMark::Symbol { instances, .. } => instances[0].y += 0.25,
            ChartMark::Line { series, .. } => series[0].points[0].y += 0.25,
            ChartMark::Bar { instances, .. } => instances[0].value += 0.25,
        }
        assert!(
            build_scene(&input).is_err(),
            "{kind} trusted a cache-only edit"
        );
    }
}

fn rows_mut(mir: &mut VizMir) -> &mut Vec<std::collections::BTreeMap<String, serde_json::Value>> {
    let source = chart(mir).source.clone();
    let MirDataOperator::Inline { rows } = &mut mir.data.get_mut(&source).unwrap().operator;
    rows
}

fn numeric_expression_id(mir: &VizMir) -> String {
    match &chart(mir).mark {
        ChartMark::Symbol { y, .. } | ChartMark::Line { y, .. } => y.expression.clone(),
        ChartMark::Bar { value, .. } => value.expression.clone(),
    }
}

fn set_numeric_expression(mir: &mut VizMir, expression: Expression, result_type: ValueType) {
    let id = numeric_expression_id(mir);
    mir.expressions.insert(
        id,
        TypedExpression {
            result_type,
            expression,
        },
    );
}

fn field(mir: &VizMir, name: &str) -> Expression {
    Expression::Field {
        row: chart(mir).row_variable.clone(),
        field: name.into(),
    }
}

fn int(value: i64) -> Expression {
    Expression::Literal {
        value: LiteralValue::Int64(value),
    }
}

fn float(value: f64) -> Expression {
    Expression::Literal {
        value: LiteralValue::Float64(value),
    }
}

fn cached_values(mir: &VizMir) -> Vec<(String, f64)> {
    match &chart(mir).mark {
        ChartMark::Symbol { instances, .. } => instances
            .iter()
            .map(|item| (item.key.clone(), item.y))
            .collect(),
        ChartMark::Line { series, .. } => series
            .iter()
            .flat_map(|series| series.points.iter().map(|item| (item.key.clone(), item.y)))
            .collect(),
        ChartMark::Bar { instances, .. } => instances
            .iter()
            .map(|item| (item.key.clone(), item.value))
            .collect(),
    }
}

fn mutate_first_cached_value(mir: &mut VizMir, change: impl FnOnce(f64) -> f64) {
    match &mut chart_mut(mir).mark {
        ChartMark::Symbol { instances, .. } => instances[0].y = change(instances[0].y),
        ChartMark::Line { series, .. } => series[0].points[0].y = change(series[0].points[0].y),
        ChartMark::Bar { instances, .. } => instances[0].value = change(instances[0].value),
    }
}

fn assert_failed_without_mutation(input: &VizMir) {
    let before = format!("{input:?}");
    assert!(rematerialize_mir(input).is_err());
    assert_eq!(format!("{input:?}"), before);
    assert!(build_scene(input).is_err());
    assert_eq!(format!("{input:?}"), before);
}

#[test]
fn ordinary_lowering_is_already_materialized_byte_for_byte() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let input = mir(kind);
        let bytes = serde_json::to_vec(&input).unwrap();
        let refreshed = rematerialize_mir(&input).unwrap();
        assert_eq!(serde_json::to_vec(&refreshed).unwrap(), bytes, "{kind}");
        assert_eq!(
            build_scene(&refreshed).unwrap(),
            build_scene(&input).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&rematerialize_mir(&refreshed).unwrap()).unwrap(),
            bytes
        );
    }
}

#[test]
fn source_edits_refresh_values_without_changing_explicit_scales_or_metadata() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        rows_mut(&mut input)[0].insert("y".into(), json!(3.5));
        let before = serde_json::to_vec(&input).unwrap();
        let refreshed = rematerialize_mir(&input).unwrap();
        assert_eq!(
            cached_values(&refreshed)
                .into_iter()
                .find(|(key, _)| key == "row-b")
                .unwrap()
                .1,
            3.5
        );
        assert_eq!(serde_json::to_vec(&input).unwrap(), before);
        assert_eq!(chart(&refreshed).scales, chart(&input).scales);
        assert_eq!(chart(&refreshed).guides, chart(&input).guides);
        assert_eq!(chart(&refreshed).provenance, chart(&input).provenance);
        assert_eq!(refreshed.data, input.data);
        assert_eq!(refreshed.expressions, input.expressions);
        assert_eq!(refreshed.spaces, input.spaces);
        assert_eq!(refreshed.losses, input.losses);
        build_scene(&refreshed).unwrap();
        assert_eq!(
            serde_json::to_vec(&rematerialize_mir(&refreshed).unwrap()).unwrap(),
            serde_json::to_vec(&refreshed).unwrap()
        );
    }
}

#[test]
fn explicit_numeric_and_categorical_domains_and_ranges_are_never_reinferred() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        for scale in &mut chart_mut(&mut input).scales {
            match scale {
                MirScale::Linear { domain, range, .. } => {
                    *domain = [-100.0, 100.0];
                    *range = [11.0, 91.0];
                }
                MirScale::Band { domain, range, .. } => {
                    domain.reverse();
                    *range = [15.0, 95.0];
                }
                MirScale::OrdinalColor { .. } => {}
            }
        }
        let scales = chart(&input).scales.clone();
        rows_mut(&mut input)[0].insert("y".into(), json!(5.0));
        let refreshed = rematerialize_mir(&input).unwrap();
        assert_eq!(chart(&refreshed).scales, scales);
    }
}

#[test]
fn refresh_repairs_cache_only_changes_for_every_chart_mark() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let original = mir(kind);
        let mut input = original.clone();
        mutate_first_cached_value(&mut input, |_| 123.0);
        let refreshed = rematerialize_mir(&input).unwrap();
        assert_eq!(
            serde_json::to_vec(&refreshed).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
    }
}

#[test]
fn exact_one_ulp_cache_edits_are_stale() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        mutate_first_cached_value(&mut input, |value| f64::from_bits(value.to_bits() + 1));
        assert!(
            build_scene(&input).is_err(),
            "{kind} accepted a one-ULP cache edit"
        );
        let refreshed = rematerialize_mir(&input).unwrap();
        build_scene(&refreshed).unwrap();
    }
}

#[test]
fn exact_one_ulp_source_edits_are_stale() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        rows_mut(&mut input)[0].insert("y".into(), json!(f64::from_bits(3.0_f64.to_bits() + 1)));
        assert!(
            build_scene(&input).is_err(),
            "{kind} accepted a one-ULP source edit"
        );
        let refreshed = rematerialize_mir(&input).unwrap();
        assert_eq!(
            cached_values(&refreshed)
                .into_iter()
                .find(|(key, _)| key == "row-b")
                .unwrap()
                .1
                .to_bits(),
            3.0_f64.to_bits() + 1
        );
        build_scene(&refreshed).unwrap();
    }
}

#[test]
fn signed_zero_is_preserved_and_cache_sign_edits_are_stale() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        set_numeric_expression(&mut input, float(-0.0), ValueType::Float64);
        let mut refreshed = rematerialize_mir(&input).unwrap();
        for (_, value) in cached_values(&refreshed) {
            assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
        }
        mutate_first_cached_value(&mut refreshed, |_| 0.0);
        assert!(
            build_scene(&refreshed).is_err(),
            "{kind} silently equated signed-zero cache bits"
        );
    }
}

#[test]
fn supported_arithmetic_changes_cached_values_and_real_geometry() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let original = mir(kind);
        let original_scene = build_scene(&original).unwrap();
        let mut input = original.clone();
        // (y + 1) * 0.5 - 0.25 uses field, both numeric literal types,
        // promotion, and every supported arithmetic operation.
        let expression = Expression::Subtract {
            left: Box::new(Expression::Multiply {
                left: Box::new(Expression::Add {
                    left: Box::new(field(&input, "y")),
                    right: Box::new(int(1)),
                }),
                right: Box::new(float(0.5)),
            }),
            right: Box::new(float(0.25)),
        };
        set_numeric_expression(&mut input, expression, ValueType::Float64);
        assert!(build_scene(&input).is_err());
        let refreshed = rematerialize_mir(&input).unwrap();
        for (key, value) in cached_values(&refreshed) {
            let source_value = cached_values(&original)
                .into_iter()
                .find(|(candidate, _)| candidate == &key)
                .unwrap()
                .1;
            assert_eq!(value, (source_value + 1.0) * 0.5 - 0.25);
        }
        let scene = build_scene(&refreshed).unwrap();
        let node_id = if kind == "chart.bar" {
            "chart/bar/row-b"
        } else {
            "chart/point/row-b"
        };
        match (
            find_scene_node(&original_scene.nodes, node_id).unwrap(),
            find_scene_node(&scene.nodes, node_id).unwrap(),
        ) {
            (SceneNode::Circle { center: old, .. }, SceneNode::Circle { center: new, .. }) => {
                assert_eq!(old.x, new.x);
                assert_ne!(old.y, new.y);
            }
            (SceneNode::Rect { bounds: old, .. }, SceneNode::Rect { bounds: new, .. }) => {
                assert_eq!(old.x, new.x);
                assert_ne!(old.height, new.height);
            }
            _ => panic!("unexpected mark geometry"),
        }
    }
}

#[test]
fn checked_integer_arithmetic_detects_each_overflow_before_float_conversion() {
    let expressions = [
        Expression::Add {
            left: Box::new(int(i64::MAX)),
            right: Box::new(int(1)),
        },
        Expression::Subtract {
            left: Box::new(int(i64::MIN)),
            right: Box::new(int(1)),
        },
        Expression::Multiply {
            left: Box::new(int(i64::MAX)),
            right: Box::new(int(2)),
        },
    ];
    for expression in expressions {
        let mut input = mir("chart.scatter");
        set_numeric_expression(&mut input, expression, ValueType::Int64);
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn finite_float_inputs_cannot_produce_infinite_results() {
    for expression in [
        Expression::Add {
            left: Box::new(float(f64::MAX)),
            right: Box::new(float(f64::MAX)),
        },
        Expression::Subtract {
            left: Box::new(float(-f64::MAX)),
            right: Box::new(float(f64::MAX)),
        },
        Expression::Multiply {
            left: Box::new(float(f64::MAX)),
            right: Box::new(float(2.0)),
        },
    ] {
        let mut input = mir("chart.scatter");
        set_numeric_expression(&mut input, expression, ValueType::Float64);
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn nonfinite_literals_fail_without_mutating_the_input() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = mir("chart.scatter");
        set_numeric_expression(&mut input, float(value), ValueType::Float64);
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn unsupported_expressions_fail_even_when_the_old_cache_is_valid() {
    let unsupported = [
        Expression::Divide {
            left: Box::new(float(3.0)),
            right: Box::new(float(1.0)),
        },
        Expression::If {
            condition: Box::new(Expression::Literal {
                value: LiteralValue::Bool(true),
            }),
            then_value: Box::new(float(3.0)),
            else_value: Box::new(float(3.0)),
        },
        Expression::Call {
            function: PureFunction::Abs,
            args: vec![float(3.0)],
        },
        Expression::Convert {
            value: Box::new(int(3)),
            to: ValueType::Float64,
        },
        Expression::Parameter {
            id: "parameter".into(),
        },
        Expression::Signal {
            id: "signal".into(),
        },
    ];
    for expression in unsupported {
        let mut input = mir("chart.scatter");
        set_numeric_expression(&mut input, expression, ValueType::Float64);
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn stable_key_expression_must_be_the_direct_declared_source_key_field() {
    let mut input = mir("chart.scatter");
    let key_id = chart(&input).key_expression.clone();
    for expression in [
        Expression::Literal {
            value: LiteralValue::String("row-b".into()),
        },
        field(&input, "category"),
        Expression::Field {
            row: "other-row".into(),
            field: "id".into(),
        },
    ] {
        input.expressions.insert(
            key_id.clone(),
            TypedExpression {
                result_type: ValueType::String,
                expression,
            },
        );
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn missing_duplicate_and_wrongly_typed_source_keys_fail() {
    for mutation in 0..3 {
        let mut input = mir("chart.scatter");
        match mutation {
            0 => {
                rows_mut(&mut input)[0].remove("id");
            }
            1 => {
                rows_mut(&mut input)[0].insert("id".into(), json!("row-a"));
            }
            2 => {
                rows_mut(&mut input)[0].insert("id".into(), json!(["row-b"]));
            }
            _ => unreachable!(),
        }
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn every_source_row_is_checked_against_its_declared_schema() {
    for mutation in 0..4 {
        let mut input = mir("chart.scatter");
        match mutation {
            0 => {
                rows_mut(&mut input)[0].remove("y");
            }
            1 => {
                rows_mut(&mut input)[0].insert("y".into(), json!("3"));
            }
            2 => {
                rows_mut(&mut input)[0].insert("rank".into(), json!(1.5));
            }
            3 => {
                rows_mut(&mut input)[0].insert("group".into(), serde_json::Value::Null);
            }
            _ => unreachable!(),
        }
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn line_order_expression_is_executed_and_ties_are_broken_by_stable_key() {
    let mut input = mir("chart.line");
    input.expressions.insert(
        "chart/order".into(),
        TypedExpression {
            result_type: ValueType::Int64,
            expression: Expression::Multiply {
                left: Box::new(field(&input, "rank")),
                right: Box::new(int(-1)),
            },
        },
    );
    let ChartMark::Line {
        order_expression, ..
    } = &mut chart_mut(&mut input).mark
    else {
        panic!("line")
    };
    *order_expression = "chart/order".into();
    let refreshed = rematerialize_mir(&input).unwrap();
    let ChartMark::Line { series, .. } = &chart(&refreshed).mark else {
        panic!("line")
    };
    assert_eq!(
        series[0]
            .points
            .iter()
            .map(|point| point.key.as_str())
            .collect::<Vec<_>>(),
        ["row-a", "row-b", "row-c"]
    );
    // Use positive ranks to deliberately disagree with ascending x as well.
    input.expressions.get_mut("chart/order").unwrap().expression = field(&input, "rank");
    let refreshed = rematerialize_mir(&input).unwrap();
    let ChartMark::Line { series, .. } = &chart(&refreshed).mark else {
        panic!("line")
    };
    assert_eq!(
        series[0]
            .points
            .iter()
            .map(|point| point.key.as_str())
            .collect::<Vec<_>>(),
        ["row-c", "row-a", "row-b"]
    );
    let scene = build_scene(&refreshed).unwrap();
    let SceneNode::Path { commands, .. } =
        find_scene_node(&scene.nodes, "chart/series/series").unwrap()
    else {
        panic!("line path")
    };
    for (command, key) in commands.iter().zip(["row-c", "row-a", "row-b"]) {
        let (PathCommand::Move { to } | PathCommand::Line { to }) = command else {
            panic!("line command")
        };
        let SceneNode::Circle { center, .. } =
            find_scene_node(&scene.nodes, &format!("chart/point/{key}")).unwrap()
        else {
            panic!("point")
        };
        assert_eq!(to, center);
    }
}

#[test]
fn line_order_must_be_numeric() {
    let mut input = mir("chart.line");
    let key_id = chart(&input).key_expression.clone();
    let ChartMark::Line {
        order_expression, ..
    } = &mut chart_mut(&mut input).mark
    else {
        panic!("line")
    };
    *order_expression = key_id;
    assert_failed_without_mutation(&input);
}

#[test]
fn grouped_lines_are_deterministic_and_reject_mixed_color_within_one_group() {
    let mut source = document("chart.line");
    let mut json = serde_json::to_value(&source).unwrap();
    json["views"][0]["series"] = json!({"field": "group"});
    source = serde_json::from_value(json).unwrap();
    let mut input = lower_to_mir(&source).unwrap();
    let refreshed = rematerialize_mir(&input).unwrap();
    let ChartMark::Line { series, .. } = &chart(&refreshed).mark else {
        panic!("line")
    };
    assert_eq!(
        series
            .iter()
            .map(|series| series.key.as_str())
            .collect::<Vec<_>>(),
        ["G", "H"]
    );
    assert_eq!(
        series[0]
            .points
            .iter()
            .map(|point| point.key.as_str())
            .collect::<Vec<_>>(),
        ["row-a", "row-b"]
    );
    input.expressions.insert(
        "chart/single-group".into(),
        TypedExpression {
            result_type: ValueType::String,
            expression: Expression::Literal {
                value: LiteralValue::String("all".into()),
            },
        },
    );
    let ChartMark::Line {
        group_expression, ..
    } = &mut chart_mut(&mut input).mark
    else {
        panic!("line")
    };
    *group_expression = Some("chart/single-group".into());
    assert_failed_without_mutation(&input);
}

#[test]
fn json_roundtrip_retains_extreme_float_and_signed_zero_cache_bits() {
    for value in [
        -0.0,
        0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        1e-120,
        1.2345678901234567,
        1e120,
        f64::MAX,
    ] {
        let mut input = mir("chart.scatter");
        set_numeric_expression(&mut input, float(value), ValueType::Float64);
        let refreshed = rematerialize_mir(&input).unwrap();
        let restored: VizMir =
            serde_json::from_slice(&serde_json::to_vec(&refreshed).unwrap()).unwrap();
        for (_, actual) in cached_values(&restored) {
            assert_eq!(
                actual.to_bits(),
                value.to_bits(),
                "float {value:?} changed in JSON roundtrip"
            );
        }
        let again = rematerialize_mir(&restored).unwrap();
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&restored).unwrap()
        );
    }
}

#[test]
fn json_roundtrip_of_tiny_domain_mir_stays_materialized_and_renderable() {
    for values in [
        [-0.0, f64::from_bits(1), f64::from_bits(2)],
        [1e-120, 2e-120, 3e-120],
    ] {
        for kind in ["chart.scatter", "chart.line", "chart.bar"] {
            let mut source = document(kind);
            for (row, value) in source
                .datasets
                .get_mut("values")
                .unwrap()
                .rows
                .iter_mut()
                .zip(values)
            {
                row.insert("x".into(), json!(value));
                row.insert("y".into(), json!(value));
            }
            let input = lower_to_mir(&source).unwrap();
            let restored: VizMir =
                serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
            build_scene(&restored).unwrap();
            assert_eq!(
                serde_json::to_vec(&rematerialize_mir(&restored).unwrap()).unwrap(),
                serde_json::to_vec(&restored).unwrap()
            );
        }
    }
}

fn limits(depth: usize, nodes: usize, steps: u64) -> MaterializationLimits {
    MaterializationLimits {
        max_expression_depth: depth,
        max_expression_nodes: nodes,
        max_evaluation_steps: steps,
    }
}

#[test]
fn expression_depth_and_node_limits_are_independent_and_checked_before_validation() {
    let mut input = mir("chart.scatter");
    let mut expression = field(&input, "not-in-schema");
    for _ in 0..20 {
        expression = Expression::Add {
            left: Box::new(expression),
            right: Box::new(float(1.0)),
        };
    }
    set_numeric_expression(&mut input, expression, ValueType::Float64);
    let before = format!("{input:?}");
    for budget in [limits(8, 1000, 1000), limits(1000, 8, 1000)] {
        let error = rematerialize_mir_with_limits(&input, budget)
            .unwrap_err()
            .to_string();
        assert!(
            error.to_lowercase().contains("limit"),
            "recursive type validation ran before AST budget preflight: {error}"
        );
        let error = build_scene_with_limits(&input, budget)
            .unwrap_err()
            .to_string();
        assert!(
            error.to_lowercase().contains("limit"),
            "scene entry skipped AST budget preflight: {error}"
        );
    }
    assert_eq!(format!("{input:?}"), before);
}

#[test]
fn evaluation_steps_are_bounded_and_failure_leaves_input_unchanged() {
    let input = mir("chart.scatter");
    let before = serde_json::to_vec(&input).unwrap();
    let small = limits(64, 1000, 1);
    assert!(rematerialize_mir_with_limits(&input, small).is_err());
    assert!(build_scene_with_limits(&input, small).is_err());
    assert_eq!(serde_json::to_vec(&input).unwrap(), before);
    let sufficient = limits(64, 1000, 1000);
    assert_eq!(
        rematerialize_mir_with_limits(&input, sufficient).unwrap(),
        input
    );
    build_scene_with_limits(&input, sufficient).unwrap();
}

#[test]
fn stale_cache_identity_color_cardinality_and_order_are_rejected_then_repaired() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let original = mir(kind);
        for mutation in 0..5 {
            let mut input = original.clone();
            match &mut chart_mut(&mut input).mark {
                ChartMark::Symbol { instances, .. } => match mutation {
                    0 => instances[0].key = "invented-key".into(),
                    1 => instances[0].color_category = Some("invented-color".into()),
                    2 => {
                        instances.pop();
                    }
                    3 => instances.reverse(),
                    4 => instances.push(instances[0].clone()),
                    _ => unreachable!(),
                },
                ChartMark::Line { series, .. } => match mutation {
                    0 => series[0].key = "invented-group".into(),
                    1 => series[0].color_category = Some("invented-color".into()),
                    2 => {
                        series[0].points.pop();
                    }
                    3 => series[0].points.reverse(),
                    4 => series.push(series[0].clone()),
                    _ => unreachable!(),
                },
                ChartMark::Bar { instances, .. } => match mutation {
                    0 => instances[0].category = "invented-category".into(),
                    1 => instances[0].color_category = Some("invented-color".into()),
                    2 => {
                        instances.pop();
                    }
                    3 => instances.reverse(),
                    4 => instances.push(instances[0].clone()),
                    _ => unreachable!(),
                },
            }
            let error = build_scene(&input).unwrap_err().to_string();
            assert!(
                error.contains("VIZ-MATERIALIZE-0002"),
                "{kind} mutation {mutation}: {error}"
            );
            assert_eq!(
                serde_json::to_vec(&rematerialize_mir(&input).unwrap()).unwrap(),
                serde_json::to_vec(&original).unwrap()
            );
        }
    }
}

#[test]
fn source_row_add_remove_and_reorder_are_materialized_from_source_order() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = mir(kind);
        let mut removed = rows_mut(&mut input).remove(0);
        rows_mut(&mut input).reverse();
        assert!(build_scene(&input).is_err());
        let refreshed = rematerialize_mir(&input).unwrap();
        let keys = cached_values(&refreshed)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        if kind == "chart.line" {
            assert_eq!(keys, ["row-a", "row-c"]);
        } else {
            assert_eq!(keys, ["row-c", "row-a"]);
        }
        build_scene(&refreshed).unwrap();
        removed.insert("id".into(), json!("row-d"));
        rows_mut(&mut input).push(removed);
        let refreshed = rematerialize_mir(&input).unwrap();
        let keys = cached_values(&refreshed)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        if kind == "chart.line" {
            assert_eq!(keys, ["row-a", "row-d", "row-c"]);
        } else {
            assert_eq!(keys, ["row-c", "row-a", "row-d"]);
        }
        build_scene(&refreshed).unwrap();
    }
}

#[test]
fn undeclared_fields_and_invalid_nested_unused_fields_are_schema_errors() {
    for mutation in 0..2 {
        let mut input = mir("chart.scatter");
        if mutation == 0 {
            rows_mut(&mut input)[0].insert("undeclared".into(), json!(42));
        } else {
            let source = chart(&input).source.clone();
            input.data.get_mut(&source).unwrap().schema.fields.insert(
                "extra".into(),
                ValueType::Array {
                    items: Box::new(ValueType::Int64),
                },
            );
            for row in rows_mut(&mut input) {
                row.insert("extra".into(), json!([1, 2]));
            }
            rows_mut(&mut input)[2].insert("extra".into(), json!([1, "bad"]));
        }
        let error = rematerialize_mir(&input).unwrap_err().to_string();
        assert!(error.contains("VIZ-MATERIALIZE-0003"), "{error}");
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn missing_and_null_unused_optional_fields_are_valid() {
    let mut input = mir("chart.scatter");
    let source = chart(&input).source.clone();
    input
        .data
        .get_mut(&source)
        .unwrap()
        .schema
        .fields
        .insert("optional".into(), ValueType::option(ValueType::Int64));
    rows_mut(&mut input)[0].insert("optional".into(), json!(7));
    rows_mut(&mut input)[1].insert("optional".into(), serde_json::Value::Null);
    let refreshed = rematerialize_mir(&input).unwrap();
    assert_eq!(refreshed, input);
    build_scene(&refreshed).unwrap();
}

#[test]
fn nondeterministic_and_incremental_sources_fail_closed() {
    for mutation in 0..2 {
        let mut input = mir("chart.scatter");
        let source = chart(&input).source.clone();
        let data = input.data.get_mut(&source).unwrap();
        if mutation == 0 {
            data.deterministic = false;
        } else {
            data.update_mode = UpdateMode::Incremental;
        }
        let error = rematerialize_mir(&input).unwrap_err().to_string();
        assert!(error.contains("VIZ-MATERIALIZE-0006"), "{error}");
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn stable_numeric_keys_preserve_source_integer_identity_above_float_precision() {
    let mut source = document("chart.scatter");
    let values = [
        9_007_199_254_740_992_i64,
        9_007_199_254_740_993_i64,
        i64::MAX,
    ];
    for (row, key) in source
        .datasets
        .get_mut("values")
        .unwrap()
        .rows
        .iter_mut()
        .zip(values)
    {
        row.insert("id".into(), json!(key));
    }
    let input = lower_to_mir(&source).unwrap();
    let refreshed = rematerialize_mir(&input).unwrap();
    assert_eq!(
        cached_values(&refreshed)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        values.map(|key| key.to_string())
    );
    assert_eq!(refreshed, input);
    let restored: VizMir =
        serde_json::from_slice(&serde_json::to_vec(&refreshed).unwrap()).unwrap();
    build_scene(&restored).unwrap();
}

#[test]
fn fractional_and_signed_zero_categories_keep_their_source_spelling() {
    let mut source = document("chart.bar");
    let values = [-0.0, 0.0, 1.0];
    for (row, value) in source
        .datasets
        .get_mut("values")
        .unwrap()
        .rows
        .iter_mut()
        .zip(values)
    {
        row.insert("category".into(), json!(value));
    }
    let input = lower_to_mir(&source).unwrap();
    let refreshed = rematerialize_mir(&input).unwrap();
    let ChartMark::Bar { instances, .. } = &chart(&refreshed).mark else {
        panic!("bar")
    };
    assert_eq!(
        instances
            .iter()
            .map(|item| item.category.as_str())
            .collect::<Vec<_>>(),
        ["-0.0", "0.0", "1.0"]
    );
    assert_eq!(refreshed, input);
    build_scene(&refreshed).unwrap();
}

#[test]
fn duplicate_bar_categories_are_not_silently_overplotted() {
    let mut input = mir("chart.bar");
    rows_mut(&mut input)[0].insert("category".into(), json!("A"));
    let error = rematerialize_mir(&input).unwrap_err().to_string();
    assert!(error.contains("VIZ-MATERIALIZE-0008"), "{error}");
    assert_failed_without_mutation(&input);
}

#[test]
fn unreferenced_expression_trees_also_receive_preflight_limits() {
    let mut input = mir("chart.scatter");
    let mut expression = float(1.0);
    for _ in 0..80 {
        expression = Expression::Add {
            left: Box::new(expression),
            right: Box::new(float(1.0)),
        };
    }
    input.expressions.insert(
        "unused".into(),
        TypedExpression {
            result_type: ValueType::Float64,
            expression,
        },
    );
    // Even permissive caller limits cannot remove the implementation's safe
    // recursive depth ceiling before cloning or validating the MIR.
    for result in [
        rematerialize_mir_with_limits(&input, limits(1000, 1000, 10000)).map(|_| ()),
        build_scene_with_limits(&input, limits(1000, 1000, 10000)).map(|_| ()),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    }
}

#[test]
fn declared_expression_result_types_and_row_scope_are_verified() {
    for mutation in 0..2 {
        let mut input = mir("chart.scatter");
        let expression = if mutation == 0 {
            field(&input, "y")
        } else {
            Expression::Field {
                row: "wrong-scope".into(),
                field: "y".into(),
            }
        };
        set_numeric_expression(
            &mut input,
            expression,
            if mutation == 0 {
                ValueType::Int64
            } else {
                ValueType::Float64
            },
        );
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn integer_line_order_retains_precision_above_float_exact_integer_range() {
    let mut input = mir("chart.line");
    for (row, rank) in rows_mut(&mut input).iter_mut().zip([
        9_007_199_254_740_992_i64,
        9_007_199_254_740_993_i64,
        i64::MAX,
    ]) {
        row.insert("rank".into(), json!(rank));
    }
    let expression = field(&input, "rank");
    input.expressions.insert(
        "chart/order".into(),
        TypedExpression {
            result_type: ValueType::Int64,
            expression,
        },
    );
    let ChartMark::Line {
        order_expression, ..
    } = &mut chart_mut(&mut input).mark
    else {
        panic!("line")
    };
    *order_expression = "chart/order".into();
    let refreshed = rematerialize_mir(&input).unwrap();
    assert_eq!(
        cached_values(&refreshed)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        ["row-b", "row-a", "row-c"]
    );
    build_scene(&refreshed).unwrap();
}

#[test]
fn unsupported_expressions_are_rejected_even_when_the_source_has_no_rows() {
    for expression in [
        Expression::Divide {
            left: Box::new(float(3.0)),
            right: Box::new(float(1.0)),
        },
        Expression::Call {
            function: PureFunction::Abs,
            args: vec![float(3.0)],
        },
    ] {
        let mut input = mir("chart.scatter");
        rows_mut(&mut input).clear();
        set_numeric_expression(&mut input, expression, ValueType::Float64);
        assert_failed_without_mutation(&input);
    }
}

#[test]
fn nonfinite_stored_cache_values_can_be_repaired_from_finite_authoritative_data() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let original = mir(kind);
            let mut input = original.clone();
            mutate_first_cached_value(&mut input, |_| value);
            assert!(build_scene(&input).is_err());
            let refreshed = rematerialize_mir(&input).unwrap();
            assert_eq!(refreshed, original);
            build_scene(&refreshed).unwrap();
        }
    }
}

fn assert_materialization_error(input: &VizMir, code: &str) {
    let before = format!("{input:?}");
    let error = rematerialize_mir(input).unwrap_err().to_string();
    assert!(error.contains(code), "expected {code}: {error}");
    let error = build_scene(input).unwrap_err().to_string();
    assert!(error.contains(code), "expected {code}: {error}");
    assert_eq!(format!("{input:?}"), before);
}

fn colored_mir(kind: &str) -> VizMir {
    let mut source = serde_json::to_value(document(kind)).unwrap();
    let channel = if kind == "chart.line" {
        "series"
    } else {
        "color"
    };
    source["views"][0][channel] = json!({"field": "group"});
    lower_to_mir(&serde_json::from_value(source).unwrap()).unwrap()
}

#[test]
fn empty_line_data_still_requires_a_numeric_order_expression() {
    let mut input = mir("chart.line");
    rows_mut(&mut input).clear();
    let key = chart(&input).key_expression.clone();
    let ChartMark::Line {
        order_expression, ..
    } = &mut chart_mut(&mut input).mark
    else {
        panic!("line")
    };
    *order_expression = key;
    assert_materialization_error(&input, "VIZ-MATERIALIZE-0004");
}

#[test]
fn empty_line_data_still_requires_a_non_nullable_scalar_group_expression() {
    for result_type in [
        ValueType::option(ValueType::String),
        ValueType::Array {
            items: Box::new(ValueType::String),
        },
    ] {
        let mut input = mir("chart.line");
        rows_mut(&mut input).clear();
        let source = chart(&input).source.clone();
        input
            .data
            .get_mut(&source)
            .unwrap()
            .schema
            .fields
            .insert("group".into(), result_type.clone());
        let expression = field(&input, "group");
        input.expressions.insert(
            "chart/group".into(),
            TypedExpression {
                result_type,
                expression,
            },
        );
        let ChartMark::Line {
            group_expression, ..
        } = &mut chart_mut(&mut input).mark
        else {
            panic!("line")
        };
        *group_expression = Some("chart/group".into());
        assert_materialization_error(&input, "VIZ-MATERIALIZE-0004");
    }
}

#[test]
fn empty_sources_still_require_supported_stable_key_scalar_types() {
    for result_type in [
        ValueType::option(ValueType::String),
        ValueType::Array {
            items: Box::new(ValueType::String),
        },
        ValueType::Record {
            fields: Default::default(),
        },
        ValueType::Color,
        ValueType::Null,
    ] {
        let mut input = mir("chart.line");
        rows_mut(&mut input).clear();
        let source = chart(&input).source.clone();
        let key = chart(&input).key_expression.clone();
        input
            .data
            .get_mut(&source)
            .unwrap()
            .schema
            .fields
            .insert("id".into(), result_type.clone());
        input.expressions.get_mut(&key).unwrap().result_type = result_type;
        assert_materialization_error(&input, "VIZ-MATERIALIZE-0004");
    }
}

#[test]
fn empty_sources_still_require_deterministic_replace_mode() {
    for mutation in 0..2 {
        let mut input = mir("chart.line");
        rows_mut(&mut input).clear();
        let source = chart(&input).source.clone();
        let data = input.data.get_mut(&source).unwrap();
        if mutation == 0 {
            data.deterministic = false;
        } else {
            data.update_mode = UpdateMode::Incremental;
        }
        assert_materialization_error(&input, "VIZ-MATERIALIZE-0006");
    }
}

#[test]
fn changed_colors_outside_explicit_ordinal_domain_fail_without_widening_it() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut input = colored_mir(kind);
        let scales = chart(&input).scales.clone();
        rows_mut(&mut input)[0].insert("group".into(), json!("new-group"));
        assert_materialization_error(&input, "VIZ-MATERIALIZE-0009");
        assert_eq!(chart(&input).scales, scales);
    }
}

#[test]
fn ordinal_domain_and_range_must_have_equal_lengths_even_with_empty_data() {
    for remove_color in [true, false] {
        for empty in [false, true] {
            let mut input = colored_mir("chart.scatter");
            if empty {
                rows_mut(&mut input).clear();
            }
            let scale = chart_mut(&mut input)
                .scales
                .iter_mut()
                .find(|scale| matches!(scale, MirScale::OrdinalColor { .. }))
                .unwrap();
            let MirScale::OrdinalColor { range, .. } = scale else {
                unreachable!()
            };
            if remove_color {
                range.pop();
            } else {
                range.push(range[0].clone());
            }
            assert_materialization_error(&input, "VIZ-MATERIALIZE-0009");
        }
    }
}

#[test]
fn duplicate_ordinal_and_band_domains_fail_even_with_empty_data() {
    for kind in ["chart.scatter", "chart.bar"] {
        for empty in [false, true] {
            let mut input = if kind == "chart.scatter" {
                colored_mir(kind)
            } else {
                mir(kind)
            };
            if empty {
                rows_mut(&mut input).clear();
            }
            for scale in &mut chart_mut(&mut input).scales {
                if let MirScale::OrdinalColor { domain, .. } | MirScale::Band { domain, .. } = scale
                {
                    domain[1] = domain[0].clone();
                }
            }
            assert_materialization_error(&input, "VIZ-MATERIALIZE-0009");
        }
    }
}

#[test]
fn absent_bar_band_category_fails_refresh_without_widening_domain() {
    let mut input = mir("chart.bar");
    let scales = chart(&input).scales.clone();
    rows_mut(&mut input)[0].insert("category".into(), json!("new-category"));
    assert_materialization_error(&input, "VIZ-MATERIALIZE-0009");
    assert_eq!(chart(&input).scales, scales);
}

fn add_second_scatter_chart(input: &mut VizMir, independent_expressions: bool) {
    let mut second = chart(input).clone();
    second.id = "chart-two".into();
    let ChartMark::Symbol { id, x, y, .. } = &mut second.mark else {
        panic!("scatter")
    };
    *id = "chart-two/marks/points".into();
    if independent_expressions {
        for reference in [
            &mut second.key_expression,
            &mut x.expression,
            &mut y.expression,
        ] {
            let new_id = format!("second/{reference}");
            input.expressions.insert(
                new_id.clone(),
                input.expressions.get(reference.as_str()).unwrap().clone(),
            );
            *reference = new_id;
        }
    }
    input.views.push(MirView::Chart(Box::new(second)));
}

#[test]
fn evaluation_work_limit_is_shared_across_all_charts_in_one_call() {
    let single = mir("chart.scatter");
    let mut low = 0;
    let mut high = MaterializationLimits::default().max_evaluation_steps;
    while low < high {
        let middle = low + (high - low) / 2;
        let result = rematerialize_mir_with_limits(&single, limits(64, 65_536, middle));
        if let Err(error) = result {
            assert!(error.to_string().contains("VIZ-MATERIALIZE-0001"));
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    assert!(low > 0);
    let minimal = limits(64, 65_536, low);
    rematerialize_mir_with_limits(&single, minimal).unwrap();
    build_scene_with_limits(&single, minimal).unwrap();
    let mut multiple = single.clone();
    add_second_scatter_chart(&mut multiple, false);
    rematerialize_mir(&multiple).unwrap();
    build_scene(&multiple).unwrap();
    for result in [
        rematerialize_mir_with_limits(&multiple, minimal).map(|_| ()),
        build_scene_with_limits(&multiple, minimal).map(|_| ()),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    }
}

#[test]
fn expression_node_limit_is_shared_across_all_chart_expression_entries() {
    let single = mir("chart.scatter");
    let exact_single_nodes = single.expressions.len(); // Three single-node fields.
    rematerialize_mir_with_limits(&single, limits(64, exact_single_nodes, 10_000)).unwrap();
    let mut multiple = single.clone();
    add_second_scatter_chart(&mut multiple, true);
    rematerialize_mir(&multiple).unwrap();
    build_scene(&multiple).unwrap();
    for result in [
        rematerialize_mir_with_limits(&multiple, limits(64, exact_single_nodes, 10_000))
            .map(|_| ()),
        build_scene_with_limits(&multiple, limits(64, exact_single_nodes, 10_000)).map(|_| ()),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
    }
}

#[test]
fn sparse_hir_schema_inference_is_charged_for_the_full_row_field_rectangle() {
    // Only 12,800 stored cells, but inference would inspect more than ten
    // million cells after taking the union of these distinct sparse fields.
    let mut source = document("chart.scatter");
    let row_count = 3_200usize;
    let rows = (0..row_count)
        .map(|index| {
            std::collections::BTreeMap::from([
                ("id".into(), json!(format!("row-{index}"))),
                ("x".into(), json!(index)),
                ("y".into(), json!(index)),
                (format!("sparse-{index}"), json!(true)),
            ])
        })
        .collect();
    source.datasets.get_mut("values").unwrap().rows = rows;
    assert!(
        row_count as u64 * (row_count as u64 + 3)
            > MaterializationLimits::default().max_evaluation_steps
    );
    let error = lower_to_mir(&source).unwrap_err().to_string();
    assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
}

#[test]
fn nested_schema_and_value_depths_are_preflighted_before_validation_or_clone() {
    for schema_nesting in [true, false] {
        let mut input = mir("chart.scatter");
        let source = chart(&input).source.clone();
        if schema_nesting {
            let mut value_type = ValueType::String;
            for _ in 0..80 {
                value_type = ValueType::option(value_type);
            }
            input
                .data
                .get_mut(&source)
                .unwrap()
                .schema
                .fields
                .insert("deep".into(), value_type);
        } else {
            let mut value = json!(1);
            for _ in 0..80 {
                value = json!([value]);
            }
            // The deliberately mismatched shallow schema must not run before
            // the value depth budget, and cloning must not precede preflight.
            input
                .data
                .get_mut(&source)
                .unwrap()
                .schema
                .fields
                .insert("deep".into(), ValueType::String);
            rows_mut(&mut input)[0].insert("deep".into(), value);
        }
        assert_materialization_error(&input, "VIZ-MATERIALIZE-0001");
    }
}

#[test]
fn nested_hir_values_are_preflighted_before_recursive_schema_inference() {
    let mut source = document("chart.scatter");
    let mut value = json!(1);
    for _ in 0..80 {
        value = json!([value]);
    }
    source.datasets.get_mut("values").unwrap().rows[0].insert("deep".into(), value);
    let error = lower_to_mir(&source).unwrap_err().to_string();
    assert!(error.contains("VIZ-MATERIALIZE-0001"), "{error}");
}
