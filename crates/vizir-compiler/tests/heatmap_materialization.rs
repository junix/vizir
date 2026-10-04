use serde_json::{Value, json};
use vizir_compiler::{
    MaterializationLimits, build_scene, build_scene_with_limits, lower_to_mir, rematerialize_mir,
    rematerialize_mir_with_limits,
};
use vizir_core::{
    ChartMark, Document, Expression, LiteralValue, MAX_HEATMAP_CELLS, MAX_HEATMAP_LABEL_BYTES,
    MirChart, MirDataOperator, MirHeatmapCell, MirScale, MirView, TypedExpression, ValueType, View,
    VizMir, canonical_quantize_thresholds, quantize_color_index,
};

fn document(rows: Value) -> Document {
    serde_json::from_value(json!({
        "version":"0.4", "id":"heatmaps", "width":1200, "height":800,
        "datasets":{"d":{"key":"id","rows":rows}},
        "views":[{"kind":"chart.heatmap","id":"heat","dataset":"d",
            "frame":{"x":20,"y":20,"width":1120,"height":720},
            "x":{"field":"x"}, "y":{"field":"y"},
            "color":{"field":"v","domain":[0,10],
                "palette":["#111111","#777777","#eeeeee"]}}]
    }))
    .unwrap()
}

fn simple() -> Document {
    document(json!([
        {"id":"last","x":"B","y":"row2","v":9.0},
        {"id":"first","x":"A","y":"row1","v":1.0},
        {"id":"middle","x":"B","y":"row1","v":5.0}
    ]))
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

fn cells(mir: &VizMir) -> &[MirHeatmapCell] {
    let ChartMark::Heatmap { instances, .. } = &chart(mir).mark else {
        panic!("expected heatmap")
    };
    instances
}

fn cells_mut(mir: &mut VizMir) -> &mut Vec<MirHeatmapCell> {
    let ChartMark::Heatmap { instances, .. } = &mut chart_mut(mir).mark else {
        panic!("expected heatmap")
    };
    instances
}

fn rows_mut(mir: &mut VizMir) -> &mut Vec<std::collections::BTreeMap<String, Value>> {
    let source = chart(mir).source.clone();
    let MirDataOperator::Inline { rows } = &mut mir.data.get_mut(&source).unwrap().operator;
    rows
}

fn band_mut<'a>(mir: &'a mut VizMir, suffix: &str) -> &'a mut MirScale {
    chart_mut(mir)
        .scales
        .iter_mut()
        .find(|scale| scale.id().ends_with(suffix))
        .unwrap()
}

fn quantitative_mut(mir: &mut VizMir) -> &mut MirScale {
    chart_mut(mir)
        .scales
        .iter_mut()
        .find(|scale| matches!(scale, MirScale::QuantizeColor { .. }))
        .unwrap()
}

#[test]
fn rows_keys_values_and_source_order_survive_hir_and_mir_replay() {
    let document = simple();
    let mir = lower_to_mir(&document).unwrap();
    let MirDataOperator::Inline { rows } = &mir.data[&chart(&mir).source].operator;
    assert_eq!(rows, &document.datasets["d"].rows);
    assert_eq!(
        cells(&mir)
            .iter()
            .map(|cell| cell.key.as_str())
            .collect::<Vec<_>>(),
        vec!["last", "first", "middle"]
    );
    assert_eq!(
        cells(&mir)
            .iter()
            .map(|cell| cell.value)
            .collect::<Vec<_>>(),
        vec![9.0, 1.0, 5.0]
    );
    let scene = build_scene(&mir).unwrap();
    let replay: VizMir = serde_json::from_slice(&serde_json::to_vec(&mir).unwrap()).unwrap();
    assert_eq!(build_scene(&replay).unwrap(), scene);
    assert_eq!(rematerialize_mir(&mir).unwrap(), mir);
}

#[test]
fn category_identity_is_exact_and_pair_uniqueness_is_not_concatenation() {
    let d = document(json!([
        {"id":"a","x":"a|b","y":"c","v":1},
        {"id":"b","x":"a","y":"b|c","v":2},
        {"id":"c","x":"A","y":"c","v":3},
        {"id":"d","x":" a","y":"c","v":4},
        {"id":"e","x":"a ","y":"c","v":5},
        {"id":"f","x":"é","y":"c","v":6},
        {"id":"g","x":"e\u{301}","y":"c","v":7}
    ]));
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cells(&mir).len(), 7);
    assert_eq!(cells(&mir)[5].x, "é");
    assert_eq!(cells(&mir)[6].x, "e\u{301}");
    let mut duplicate = simple();
    let first = duplicate.datasets["d"].rows[0].clone();
    let row = &mut duplicate.datasets.get_mut("d").unwrap().rows[1];
    row.insert("x".into(), first["x"].clone());
    row.insert("y".into(), first["y"].clone());
    assert!(
        lower_to_mir(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn axes_require_nonempty_non_null_strings_and_reject_line_controls() {
    for value in [
        json!(0),
        json!(true),
        Value::Null,
        json!(""),
        json!("a\n"),
        json!("a\u{7f}"),
        json!("a\u{85}"),
        json!("a\u{2028}"),
        json!("a\u{2029}"),
    ] {
        for field in ["x", "y"] {
            let mut d = simple();
            d.datasets.get_mut("d").unwrap().rows[0].insert(field.into(), value.clone());
            assert!(lower_to_mir(&d).is_err(), "accepted {field} = {value:?}");
        }
    }
    for field in ["x", "y"] {
        let mut d = simple();
        d.datasets.get_mut("d").unwrap().rows[0].remove(field);
        assert!(lower_to_mir(&d).is_err());
    }
    let mut d = simple();
    d.datasets.get_mut("d").unwrap().rows[0]
        .insert("x".into(), json!("x".repeat(MAX_HEATMAP_LABEL_BYTES + 1)));
    assert!(lower_to_mir(&d).is_err());
}

#[test]
fn numeric_colors_do_not_coerce_strings_booleans_null_or_missing() {
    for value in [json!("3"), json!(true), Value::Null] {
        let mut d = simple();
        d.datasets.get_mut("d").unwrap().rows[0].insert("v".into(), value);
        assert!(lower_to_mir(&d).is_err());
    }
    let mut d = simple();
    d.datasets.get_mut("d").unwrap().rows[0].remove("v");
    assert!(lower_to_mir(&d).is_err());
}

#[test]
fn explicit_domains_can_preserve_unused_categories_without_dense_cells() {
    let mut d = simple();
    let View::Heatmap(view) = &mut d.views[0] else {
        panic!()
    };
    view.x.domain = Some(vec!["unused".into(), "A".into(), "B".into()]);
    view.y.domain = Some(vec!["row2".into(), "unused".into(), "row1".into()]);
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cells(&mir).len(), 3);
    let MirScale::Band { domain, .. } = &chart(&mir).scales[0] else {
        panic!()
    };
    assert_eq!(domain, &["unused", "A", "B"]);
    let mut altered = mir.clone();
    rows_mut(&mut altered)[0].insert("x".into(), json!("unused"));
    assert!(build_scene(&altered).is_err());
    let refreshed = rematerialize_mir(&altered).unwrap();
    assert_eq!(cells(&refreshed)[0].x, "unused");
    assert_eq!(chart(&refreshed).scales, chart(&mir).scales);
    rows_mut(&mut altered)[0].insert("x".into(), json!("not-in-domain"));
    assert!(rematerialize_mir(&altered).is_err());
}

#[test]
fn every_cached_field_length_and_order_are_assertions() {
    let original = lower_to_mir(&simple()).unwrap();
    for case in 0..8 {
        let mut altered = original.clone();
        let items = cells_mut(&mut altered);
        match case {
            0 => items[0].key = "different".into(),
            1 => items[0].x = "A".into(),
            2 => items[0].y = "row1".into(),
            3 => items[0].value = 8.0,
            4 => items.swap(0, 1),
            5 => {
                items.pop();
            }
            6 => items.push(items[0].clone()),
            _ => items.clear(),
        }
        let before = serde_json::to_vec(&altered).unwrap();
        assert!(
            build_scene(&altered).is_err(),
            "accepted cache mutation {case}"
        );
        assert_eq!(serde_json::to_vec(&altered).unwrap(), before);
        assert_eq!(rematerialize_mir(&altered).unwrap(), original);
    }
}

#[test]
fn refresh_ignores_arbitrary_bounded_cache_contents() {
    let original = lower_to_mir(&simple()).unwrap();
    let mut mir = original.clone();
    let cell = &mut cells_mut(&mut mir)[0];
    cell.key.clear();
    cell.x = "invalid\ncategory".into();
    cell.y.clear();
    cell.value = f64::NAN;
    assert!(build_scene(&mir).is_err());
    assert_eq!(rematerialize_mir(&mir).unwrap(), original);
}

#[test]
fn numeric_cache_comparison_preserves_signed_zero_bits() {
    let d = document(json!([{"id":"zero","x":"A","y":"r","v":-0.0}]));
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cells(&mir)[0].value.to_bits(), (-0.0_f64).to_bits());
    let mut changed = mir.clone();
    cells_mut(&mut changed)[0].value = 0.0;
    assert!(build_scene(&changed).is_err());
    assert_eq!(
        cells(&rematerialize_mir(&changed).unwrap())[0]
            .value
            .to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn direct_mir_uses_typed_arithmetic_and_the_same_numeric_conversion() {
    let original = lower_to_mir(&simple()).unwrap();
    let mut mir = original.clone();
    let ChartMark::Heatmap { color, .. } = &chart(&mir).mark else {
        panic!()
    };
    let expression_id = color.expression.clone();
    let source_expression = mir.expressions[&expression_id].expression.clone();
    mir.expressions.insert(
        expression_id,
        TypedExpression {
            result_type: ValueType::Float64,
            expression: Expression::Add {
                left: Box::new(source_expression),
                right: Box::new(Expression::Literal {
                    value: LiteralValue::Float64(0.25),
                }),
            },
        },
    );
    assert!(build_scene(&mir).is_err());
    let refreshed = rematerialize_mir(&mir).unwrap();
    assert_eq!(
        cells(&refreshed)
            .iter()
            .map(|cell| cell.value)
            .collect::<Vec<_>>(),
        vec![9.25, 1.25, 5.25]
    );
    assert_eq!(chart(&refreshed).scales, chart(&original).scales);
    build_scene(&refreshed).unwrap();

    let mut large = document(json!([
        {"id":"a","x":"A","y":"r","v":9007199254740993_i64},
        {"id":"b","x":"B","y":"r","v":9007199254740992_i64}
    ]));
    let View::Heatmap(view) = &mut large.views[0] else {
        panic!()
    };
    view.color.domain = None;
    let mir = lower_to_mir(&large).unwrap();
    assert_eq!(
        cells(&mir)[0].value.to_bits(),
        (9007199254740993_i64 as f64).to_bits()
    );
    assert_eq!(rematerialize_mir(&mir).unwrap(), mir);
    let MirDataOperator::Inline { rows } = &mir.data[&chart(&mir).source].operator;
    assert_eq!(rows[0]["v"], json!(9007199254740993_i64));
}

#[test]
fn direct_mir_string_literals_are_supported_but_numeric_axes_are_not() {
    let mut mir = lower_to_mir(&simple()).unwrap();
    let ChartMark::Heatmap { x, .. } = &chart(&mir).mark else {
        panic!()
    };
    let id = x.expression.clone();
    mir.expressions.insert(
        id.clone(),
        TypedExpression {
            result_type: ValueType::Int64,
            expression: Expression::Literal {
                value: LiteralValue::Int64(1),
            },
        },
    );
    assert!(rematerialize_mir(&mir).is_err());
    mir.expressions.insert(
        id,
        TypedExpression {
            result_type: ValueType::String,
            expression: Expression::Literal {
                value: LiteralValue::String("A".into()),
            },
        },
    );
    // All x values becoming A duplicates the two row1 cells.
    assert!(rematerialize_mir(&mir).is_err());
    rows_mut(&mut mir).pop();
    let refreshed = rematerialize_mir(&mir).unwrap();
    assert!(cells(&refreshed).iter().all(|cell| cell.x == "A"));
}

#[test]
fn refresh_preserves_explicit_color_plan_and_never_clamps() {
    let original = lower_to_mir(&simple()).unwrap();
    let mut mir = original.clone();
    rows_mut(&mut mir)[0].insert("v".into(), json!(8.5));
    assert!(build_scene(&mir).is_err());
    let refreshed = rematerialize_mir(&mir).unwrap();
    assert_eq!(chart(&refreshed).scales, chart(&original).scales);
    assert_eq!(cells(&refreshed)[0].value, 8.5);
    for outside in [-0.25, 10.25] {
        rows_mut(&mut mir)[0].insert("v".into(), json!(outside));
        assert!(rematerialize_mir(&mir).is_err());
    }
}

#[test]
fn quantize_ties_upper_endpoint_constant_palettes_and_canonical_thresholds() {
    let domain = [0.0, 10.0];
    let thresholds = canonical_quantize_thresholds(domain, 3).unwrap();
    assert_eq!(
        quantize_color_index(0.0, domain, &thresholds, 3).unwrap(),
        0
    );
    assert_eq!(
        quantize_color_index(thresholds[0], domain, &thresholds, 3).unwrap(),
        1
    );
    assert_eq!(
        quantize_color_index(thresholds[1], domain, &thresholds, 3).unwrap(),
        2
    );
    assert_eq!(
        quantize_color_index(10.0, domain, &thresholds, 3).unwrap(),
        2
    );
    assert!(quantize_color_index(10.1, domain, &thresholds, 3).is_err());
    for bins in 2..=9 {
        assert!(
            canonical_quantize_thresholds([7.0, 7.0], bins)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            quantize_color_index(7.0, [7.0, 7.0], &[], bins).unwrap(),
            (bins - 1) / 2
        );
        assert!(quantize_color_index(7.1, [7.0, 7.0], &[], bins).is_err());
    }
    let mut mir = lower_to_mir(&simple()).unwrap();
    let MirScale::QuantizeColor { thresholds, .. } = quantitative_mut(&mut mir) else {
        panic!()
    };
    thresholds[0] = f64::from_bits(thresholds[0].to_bits() + 1);
    assert!(build_scene(&mir).is_err());
    assert!(rematerialize_mir(&mir).is_err());
}

#[test]
fn heatmap_band_ranges_must_remain_positive_after_four_decimal_serialization() {
    let original = lower_to_mir(&simple()).unwrap();
    for range in [
        [0.0, 0.00001],
        [1.0, 1.0],
        [5.0, -5.0],
        [f64::NEG_INFINITY, 10.0],
        [-f64::MAX, f64::MAX],
        [0.0, 1_000_001.0],
    ] {
        let mut mir = original.clone();
        let MirScale::Band { range: target, .. } = band_mut(&mut mir, "/x") else {
            panic!()
        };
        *target = range;
        assert!(build_scene(&mir).is_err(), "accepted range {range:?}");
        assert!(
            rematerialize_mir(&mir).is_err(),
            "refreshed invalid range {range:?}"
        );
    }
    let mut mir = original;
    let MirScale::Band { padding, .. } = band_mut(&mut mir, "/y") else {
        panic!()
    };
    *padding = 0.1;
    assert!(rematerialize_mir(&mir).is_err());
}

#[test]
fn direct_mir_duplicate_categories_missing_values_and_nulls_are_rejected() {
    for field in ["x", "y", "v"] {
        let mut missing = lower_to_mir(&simple()).unwrap();
        rows_mut(&mut missing)[0].remove(field);
        assert!(rematerialize_mir(&missing).is_err());
        let mut null = lower_to_mir(&simple()).unwrap();
        rows_mut(&mut null)[0].insert(field.into(), Value::Null);
        assert!(rematerialize_mir(&null).is_err());
    }
    let mut duplicate = lower_to_mir(&simple()).unwrap();
    rows_mut(&mut duplicate)[0].insert("y".into(), json!("row1"));
    assert!(rematerialize_mir(&duplicate).is_err());
}

#[test]
fn hard_row_and_cache_limits_are_checked_before_generic_validation() {
    let mut d = simple();
    let row = d.datasets["d"].rows[0].clone();
    d.datasets.get_mut("d").unwrap().rows = vec![row; MAX_HEATMAP_CELLS + 1];
    assert!(lower_to_mir(&d).unwrap_err().to_string().contains("16384"));

    let original = lower_to_mir(&simple()).unwrap();
    let mut source = original.clone();
    let row = rows_mut(&mut source)[0].clone();
    *rows_mut(&mut source) = vec![row; MAX_HEATMAP_CELLS + 1];
    assert!(
        rematerialize_mir(&source)
            .unwrap_err()
            .to_string()
            .contains("16384")
    );

    let mut cache = original.clone();
    let cell = cells(&cache)[0].clone();
    *cells_mut(&mut cache) = vec![cell; MAX_HEATMAP_CELLS + 1];
    assert!(
        rematerialize_mir(&cache)
            .unwrap_err()
            .to_string()
            .contains("16384")
    );

    let mut source_total = original.clone();
    let row = rows_mut(&mut source_total)[0].clone();
    *rows_mut(&mut source_total) = vec![row; MAX_HEATMAP_CELLS];
    source_total.views = vec![source_total.views[0].clone(); 5];
    assert!(
        rematerialize_mir(&source_total)
            .unwrap_err()
            .to_string()
            .contains("65536")
    );

    let mut cache_total = original;
    let cell = cells(&cache_total)[0].clone();
    *cells_mut(&mut cache_total) = vec![cell; MAX_HEATMAP_CELLS];
    cache_total.views = vec![cache_total.views[0].clone(); 5];
    assert!(
        rematerialize_mir(&cache_total)
            .unwrap_err()
            .to_string()
            .contains("65536")
    );
}

#[test]
fn empty_sources_domain_limits_and_whole_call_work_budgets_are_enforced() {
    assert!(lower_to_mir(&document(json!([]))).is_err());
    let mut d = simple();
    let View::Heatmap(view) = &mut d.views[0] else {
        panic!()
    };
    view.x.domain = Some((0..257).map(|i| format!("cat-{i}")).collect());
    assert!(lower_to_mir(&d).is_err());
    let original = lower_to_mir(&simple()).unwrap();
    let mut mir = original.clone();
    let MirScale::Band { domain, .. } = band_mut(&mut mir, "/x") else {
        panic!()
    };
    *domain = (0..257).map(|i| format!("cat-{i}")).collect();
    assert!(rematerialize_mir(&mir).is_err());
    let mut bytes = original.clone();
    let MirScale::Band { domain, .. } = band_mut(&mut bytes, "/x") else {
        panic!()
    };
    *domain = (0..65)
        .map(|i| format!("{i:03}{}", "x".repeat(MAX_HEATMAP_LABEL_BYTES - 3)))
        .collect();
    assert!(rematerialize_mir(&bytes).is_err());
    let limits = MaterializationLimits {
        max_evaluation_steps: 1,
        ..MaterializationLimits::default()
    };
    assert!(build_scene_with_limits(&original, limits).is_err());
    assert!(rematerialize_mir_with_limits(&original, limits).is_err());
}

#[test]
fn source_reordering_refreshes_in_source_order_without_reordering_domains() {
    let original = lower_to_mir(&simple()).unwrap();
    let mut edited = original.clone();
    rows_mut(&mut edited).reverse();
    assert!(build_scene(&edited).is_err());
    let refreshed = rematerialize_mir(&edited).unwrap();
    assert_eq!(chart(&refreshed).scales, chart(&original).scales);
    assert_eq!(
        cells(&refreshed)
            .iter()
            .map(|cell| cell.key.as_str())
            .collect::<Vec<_>>(),
        vec!["middle", "first", "last"]
    );
}

#[test]
fn stable_keys_keep_existing_scalar_spelling_and_must_be_unique_and_present() {
    for (keys, expected) in [
        ([json!(12), json!(3)], ["12", "3"]),
        ([json!(false), json!(true)], ["false", "true"]),
        (
            [json!("key/with|delimiters"), json!(" key ")],
            ["key/with|delimiters", " key "],
        ),
    ] {
        let mut d = document(json!([
            {"id":"a","x":"A","y":"r","v":1},
            {"id":"b","x":"B","y":"r","v":2}
        ]));
        for (row, key) in d.datasets.get_mut("d").unwrap().rows.iter_mut().zip(keys) {
            row.insert("id".into(), key);
        }
        let mir = lower_to_mir(&d).unwrap();
        assert_eq!(
            cells(&mir)
                .iter()
                .map(|cell| cell.key.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    for missing in [false, true] {
        let mut mir = lower_to_mir(&simple()).unwrap();
        if missing {
            rows_mut(&mut mir)[1].remove("id");
        } else {
            rows_mut(&mut mir)[1].insert("id".into(), json!("last"));
        }
        assert!(rematerialize_mir(&mir).is_err());
    }
}

#[test]
fn typed_integer_overflow_and_nonfinite_literals_are_rejected() {
    let original = lower_to_mir(&simple()).unwrap();
    for expression in [
        Expression::Literal {
            value: LiteralValue::Float64(f64::INFINITY),
        },
        Expression::Multiply {
            left: Box::new(Expression::Literal {
                value: LiteralValue::Float64(f64::MAX),
            }),
            right: Box::new(Expression::Literal {
                value: LiteralValue::Float64(2.0),
            }),
        },
    ] {
        let mut mir = original.clone();
        let ChartMark::Heatmap { color, .. } = &chart(&mir).mark else {
            panic!()
        };
        let id = color.expression.clone();
        mir.expressions.insert(
            id,
            TypedExpression {
                result_type: ValueType::Float64,
                expression,
            },
        );
        assert!(rematerialize_mir(&mir).is_err());
    }
    let mut mir = original;
    let ChartMark::Heatmap { color, .. } = &chart(&mir).mark else {
        panic!()
    };
    let id = color.expression.clone();
    mir.expressions.insert(
        id,
        TypedExpression {
            result_type: ValueType::Int64,
            expression: Expression::Add {
                left: Box::new(Expression::Literal {
                    value: LiteralValue::Int64(i64::MAX),
                }),
                right: Box::new(Expression::Literal {
                    value: LiteralValue::Int64(1),
                }),
            },
        },
    );
    assert!(rematerialize_mir(&mir).is_err());
}

#[test]
fn maximum_axis_product_remains_sparse_and_does_not_materialize_missing_pairs() {
    let mut d = simple();
    d.width = 16000.0;
    d.height = 6500.0;
    let View::Heatmap(view) = &mut d.views[0] else {
        panic!()
    };
    view.frame.width = 15900.0;
    view.frame.height = 6400.0;
    view.x.domain = Some(
        std::iter::once("B".to_owned())
            .chain(std::iter::once("A".to_owned()))
            .chain((0..254).map(|i| format!("X{i:03}")))
            .collect(),
    );
    view.y.domain = Some(
        std::iter::once("row2".to_owned())
            .chain(std::iter::once("row1".to_owned()))
            .chain((0..254).map(|i| format!("Y{i:03}")))
            .collect(),
    );
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cells(&mir).len(), 3);
    assert_eq!(rematerialize_mir(&mir).unwrap(), mir);
}

#[test]
fn maximum_source_row_count_is_accepted_without_dense_domain_expansion() {
    let rows = (0..128)
        .flat_map(|y| {
            (0..128).map(move |x| {
                json!({
                    "id":format!("{x}:{y}"), "x":format!("X{x}"), "y":format!("Y{y}"), "v":5
                })
            })
        })
        .collect::<Vec<_>>();
    let mut d = document(Value::Array(rows));
    d.width = 10000.0;
    d.height = 5000.0;
    let View::Heatmap(view) = &mut d.views[0] else {
        panic!()
    };
    view.frame.width = 9900.0;
    view.frame.height = 4900.0;
    let mir = lower_to_mir(&d).unwrap();
    assert_eq!(cells(&mir).len(), MAX_HEATMAP_CELLS);
    assert_eq!(rematerialize_mir(&mir).unwrap(), mir);
}
