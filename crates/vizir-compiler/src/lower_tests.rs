use super::*;

#[test]
fn nice_domain_is_deterministic_and_expands() {
    assert_eq!(nice_domain([46.0, 230.0], false), [40.0, 240.0]);
    assert_eq!(nice_domain([2.0, 9.0], true), [0.0, 9.0]);
}

#[test]
fn nice_domain_covers_each_step_bucket_and_degenerate_domains() {
    // normalized in [2, 5) picks half-power steps; normalized >= 5 keeps
    // whole powers; degenerate spans expand symmetrically; include_zero
    // keeps a negative minimum.
    assert_eq!(nice_domain([12.0, 38.0], false), [10.0, 40.0]);
    assert_eq!(nice_domain([0.3, 6.8], false), [0.0, 7.0]);
    assert_eq!(nice_domain([5.0, 5.0], false), [4.5, 5.5]);
    assert_eq!(nice_domain([0.0, 0.0], false), [-0.1, 0.1]);
    assert_eq!(nice_domain([-3.0, 9.0], true), [-4.0, 10.0]);
}

#[test]
fn nullable_numeric_fields_promote_without_nested_options() {
    let rows = vec![
        BTreeMap::from([("value".to_owned(), Value::Null)]),
        BTreeMap::from([("value".to_owned(), serde_json::json!(1))]),
        BTreeMap::from([("value".to_owned(), serde_json::json!(2.5))]),
    ];
    let fields = infer_dataset_fields(&rows).unwrap();
    assert_eq!(fields["value"], ValueType::option(ValueType::Float64));
}

#[test]
fn infer_dataset_fields_rejects_incompatible_columns_with_exact_messages() {
    let rows = vec![
        BTreeMap::from([("value".to_owned(), serde_json::json!(true))]),
        BTreeMap::from([("value".to_owned(), serde_json::json!(1))]),
    ];
    assert_eq!(
        infer_dataset_fields(&rows).unwrap_err(),
        "field \"value\" has incompatible inferred types Bool and Int64"
    );

    // Incompatible items inside one array are reported against the merged
    // item type, not the raw JSON types.
    let mixed = vec![BTreeMap::from([(
        "mixed".to_owned(),
        serde_json::json!([true, 1]),
    )])];
    assert_eq!(
        infer_dataset_fields(&mixed).unwrap_err(),
        "array contains incompatible types Option { item: Bool } and Int64"
    );
}

#[test]
fn infer_dataset_fields_treats_sparse_columns_as_optional() {
    let rows = vec![
        BTreeMap::from([
            ("a".to_owned(), serde_json::json!(1)),
            ("kept".to_owned(), serde_json::json!("x")),
        ]),
        BTreeMap::from([("b".to_owned(), serde_json::json!(2.5))]),
    ];
    let fields = infer_dataset_fields(&rows).unwrap();
    assert_eq!(
        fields,
        BTreeMap::from([
            ("a".to_owned(), ValueType::option(ValueType::Int64)),
            ("b".to_owned(), ValueType::option(ValueType::Float64)),
            ("kept".to_owned(), ValueType::option(ValueType::String)),
        ])
    );

    // No rows means no inferred fields, and the i64/u64 boundary must not
    // claim Int64 for numbers that overflow it.
    assert!(infer_dataset_fields(&[]).unwrap().is_empty());
    let boundaries = vec![
        BTreeMap::from([("n".to_owned(), serde_json::json!(i64::MAX))]),
        BTreeMap::from([("n".to_owned(), serde_json::json!(u64::MAX))]),
    ];
    let fields = infer_dataset_fields(&boundaries).unwrap();
    assert_eq!(fields["n"], ValueType::Float64);
}

#[test]
fn tiny_nice_domains_preserve_relative_scale_and_cover_input() {
    for raw in [[1e-120, 3e-120], [-3e-120, -1e-120], [-1e-120, 1e-120]] {
        for include_zero in [false, true] {
            let domain = nice_domain(raw, include_zero);
            assert!(domain[0].is_finite() && domain[1].is_finite());
            assert!(
                domain[0] <= raw[0] && domain[1] >= raw[1],
                "raw={raw:?} domain={domain:?} zero={include_zero}"
            );
            let expected_span = if include_zero {
                raw[1].max(0.0) - raw[0].min(0.0)
            } else {
                raw[1] - raw[0]
            };
            assert!(domain[1] - domain[0] <= expected_span * 2.0, "{domain:?}");
            assert_eq!(domain, nice_domain(raw, include_zero));
        }
    }
}

#[test]
fn tiny_nice_step_underflow_preserves_finite_nonconstant_domain() {
    let smallest = f64::from_bits(1);
    for raw in [
        [0.0, smallest],
        [smallest, 2.0 * smallest],
        [smallest, 3.0 * smallest],
        [-3.0 * smallest, -smallest],
        [-smallest, smallest],
    ] {
        // Either the decimal power or power/5 step is unrepresentable.
        assert_eq!(nice_domain(raw, false), raw);
    }
    assert_eq!(
        nice_domain([smallest, 2.0 * smallest], true),
        [0.0, 2.0 * smallest]
    );
}

#[test]
fn tiny_representable_subnormal_steps_still_cover_input() {
    let smallest = f64::from_bits(1);
    for raw in [[0.0, 4.0 * smallest], [1e-320, 3e-320], [-3e-320, -1e-320]] {
        let domain = nice_domain(raw, false);
        assert!(domain[0].is_finite() && domain[1].is_finite());
        assert!(domain[0] <= raw[0] && domain[1] >= raw[1]);
        assert!(domain[0] < domain[1]);
        assert!((domain[1] - domain[0]) / (raw[1] - raw[0]) <= 2.0);
    }
}
