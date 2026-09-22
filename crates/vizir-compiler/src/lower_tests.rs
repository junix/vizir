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
