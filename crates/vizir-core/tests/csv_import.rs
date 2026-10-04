use serde_json::{Value, json};
use vizir_core::{
    CSV_IMPORT_FORMAT, CSV_IMPORT_MAX_CELL_BYTES, CSV_IMPORT_MAX_COLUMNS, CSV_IMPORT_MAX_CSV_BYTES,
    CSV_IMPORT_MAX_NAME_BYTES, CSV_IMPORT_MAX_RETAINED_BYTES, CsvColumnType, CsvImportColumn,
    CsvImportLimits, CsvImportSpec, csv_import_spec_schema, import_csv, parse_csv_import_spec,
    validate_csv_import_spec,
};

fn spec(columns: &[(&str, CsvColumnType)], key: &str) -> CsvImportSpec {
    CsvImportSpec {
        format: CSV_IMPORT_FORMAT.to_owned(),
        columns: columns
            .iter()
            .map(|(name, column_type)| CsvImportColumn {
                name: (*name).to_owned(),
                column_type: *column_type,
            })
            .collect(),
        key: key.to_owned(),
    }
}
fn string_spec() -> CsvImportSpec {
    spec(&[("id", CsvColumnType::String)], "id")
}
fn typed_spec(kind: CsvColumnType) -> CsvImportSpec {
    spec(&[("id", CsvColumnType::String), ("v", kind)], "id")
}
fn assert_bad(csv: &[u8], spec: &CsvImportSpec, code: &str) {
    let error = import_csv(csv, spec, &CsvImportLimits::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains(code), "{error}");
    assert!(error.len() < 400, "diagnostic must be bounded");
}
fn wire() -> Value {
    json!({"format":CSV_IMPORT_FORMAT,"columns":[{"name":"id","type":"string"}],"key":"id"})
}

#[test]
fn preserves_order_types_whitespace_empty_strings_and_exact_line_endings() {
    let spec = spec(
        &[
            ("id", CsvColumnType::Int64),
            ("z", CsvColumnType::String),
            ("a", CsvColumnType::Float64),
            ("ok", CsvColumnType::Bool),
        ],
        "id",
    );
    let result = import_csv(
        b"id,z,a,ok\r\n2,\" a,\"\"b\"\"\nline\r\n \",1,true\n1,,-0,false",
        &spec,
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert_eq!(result.header, ["id", "z", "a", "ok"]);
    assert!(!result.had_utf8_bom);
    assert_eq!(result.row_count, 2);
    assert_eq!(result.dataset.rows[0]["id"], json!(2));
    assert_eq!(result.dataset.rows[1]["id"], json!(1));
    assert_eq!(result.dataset.rows[0]["z"], json!(" a,\"b\"\nline\r\n "));
    assert_eq!(result.dataset.rows[1]["z"], json!(""));
    assert!(result.dataset.rows[0]["a"].as_number().unwrap().is_f64());
    assert_eq!(
        result.dataset.rows[1]["a"].as_f64().unwrap().to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        result.dataset.rows[0]
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "id", "ok", "z"]
    );
}

#[test]
fn eof_and_record_terminator_rules_are_strict() {
    for valid in [
        "id\na",
        "id\na\n",
        "id\r\na\r\n",
        "id\n\"a\"",
        "id\n\"a\"\r\n",
        "id\na\r\nb\nc",
    ] {
        import_csv(
            valid.as_bytes(),
            &string_spec(),
            &CsvImportLimits::default(),
        )
        .unwrap();
    }
    for invalid in [
        "id\n\"a",
        "id\n\"a\"\"",
        "id\n\"a\"x",
        "id\n\"a\" ",
        "id\na\"",
        "id\n a\"",
        "id\na\r",
        "id\n\"a\rb\"",
        "id\na\r\rb",
        "id\na\n\n",
        "id\n\n",
        "\nid\na",
    ] {
        assert_bad(invalid.as_bytes(), &string_spec(), "VIZ-CSV-0005");
    }
    assert_bad(b"id\n\"a\"\r", &string_spec(), "VIZ-CSV-0005");
    assert_bad(b"id\na\0", &string_spec(), "VIZ-CSV-0005");
    assert_bad(b"id\n\"a\0\"", &string_spec(), "VIZ-CSV-0005");
    assert_bad(b"i\0d\na", &string_spec(), "VIZ-CSV-0005");
}

#[test]
fn escaped_quote_at_eof_and_quoted_empty_field_are_records() {
    let imported =
        import_csv(b"id\n\"\"\"\"", &string_spec(), &CsvImportLimits::default()).unwrap();
    assert_eq!(imported.dataset.rows[0]["id"], json!("\""));
    let pair = typed_spec(CsvColumnType::String);
    for csv in ["id,v\na,\"\"", "id,v\na,", "id,v\na,\n"] {
        let imported = import_csv(csv.as_bytes(), &pair, &CsvImportLimits::default()).unwrap();
        assert_eq!(imported.dataset.rows[0]["v"], json!(""));
    }
    assert_bad(b"id\n\"\"", &string_spec(), "VIZ-CSV-0009");
}

#[test]
fn headers_are_mandatory_exact_ordered_and_nonempty() {
    let pair = typed_spec(CsvColumnType::String);
    for invalid in ["", "id", "id\n", "id\r\n"] {
        assert!(
            import_csv(
                invalid.as_bytes(),
                &string_spec(),
                &CsvImportLimits::default()
            )
            .is_err()
        );
    }
    for invalid in [
        "v,id\na,b",
        "id, v\na,b",
        "id,id\na,b",
        "id,\na,b",
        "id\na,b",
        "id,v,extra\na,b,c",
    ] {
        assert_bad(invalid.as_bytes(), &pair, "VIZ-CSV-0006");
    }
    for invalid in ["id,v\na", "id,v\na,b,c", "id,v\na,b,"] {
        assert_bad(invalid.as_bytes(), &pair, "VIZ-CSV-0007");
    }
    import_csv(b"\"id\",\"v\"\na,b", &pair, &CsvImportLimits::default()).unwrap();
}

#[test]
fn initial_bom_is_optional_and_only_one_is_removed() {
    let result = import_csv(
        "\u{feff}id\r\n\u{feff}abc".as_bytes(),
        &string_spec(),
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert!(result.had_utf8_bom);
    assert_eq!(result.dataset.rows[0]["id"], json!("\u{feff}abc"));
    assert_bad(
        "\u{feff}\u{feff}id\na".as_bytes(),
        &string_spec(),
        "VIZ-CSV-0006",
    );
    assert_bad(b"id\n\xff", &string_spec(), "VIZ-CSV-0004");
    assert_bad(b"\xef\xbb", &string_spec(), "VIZ-CSV-0004");
    assert_bad(b"\xef\xbb\xbf", &string_spec(), "VIZ-CSV-0006");
}

#[test]
fn unicode_names_and_values_are_byte_exact_without_normalization() {
    let unicode_spec = spec(
        &[
            ("clé", CsvColumnType::String),
            ("日本", CsvColumnType::String),
        ],
        "clé",
    );
    let result = import_csv(
        "clé,日本\n é ,語\né,\" 🦀 \"".as_bytes(),
        &unicode_spec,
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert_eq!(result.dataset.rows[0]["clé"], json!(" é "));
    assert_eq!(result.dataset.rows[1]["日本"], json!(" 🦀 "));
    assert_bad("clé,日本\na,b".as_bytes(), &unicode_spec, "VIZ-CSV-0006");
    let quoted_name = spec(&[("a\"b", CsvColumnType::String)], "a\"b");
    import_csv(b"\"a\"\"b\"\nx", &quoted_name, &CsvImportLimits::default()).unwrap();
}

#[test]
fn int64_grammar_range_and_canonical_key_rules() {
    let typed = typed_spec(CsvColumnType::Int64);
    for valid in [
        "0",
        "-0",
        "9223372036854775807",
        "-9223372036854775808",
        "7",
        "-18",
    ] {
        let csv = format!("id,v\na,{valid}");
        let result = import_csv(csv.as_bytes(), &typed, &CsvImportLimits::default()).unwrap();
        assert!(result.dataset.rows[0]["v"].as_number().unwrap().is_i64());
    }
    for invalid in [
        "",
        " ",
        "+1",
        "01",
        "-01",
        "-",
        "1.0",
        "1e0",
        " 1",
        "1 ",
        "9223372036854775808",
        "-9223372036854775809",
        "１２",
    ] {
        assert_bad(
            format!("id,v\na,{invalid}").as_bytes(),
            &typed,
            "VIZ-CSV-0008",
        );
    }
    let key = spec(&[("id", CsvColumnType::Int64)], "id");
    assert_bad(b"id\n-0\n0", &key, "VIZ-CSV-0009");
    let result = import_csv(
        b"id\n-9223372036854775808\n9223372036854775807",
        &key,
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert_eq!(result.row_count, 2);
}

#[test]
fn float64_json_grammar_finiteness_underflow_and_signed_zero() {
    let typed = typed_spec(CsvColumnType::Float64);
    for valid in [
        "0",
        "-0",
        "0.0",
        "-0.0e-9999",
        "1",
        "-1.25",
        "1e+2",
        "1E-2",
        "1.7976931348623157e308",
        "5e-324",
        "2.4703282292062328e-324",
        "9007199254740993",
    ] {
        let result = import_csv(
            format!("id,v\na,{valid}").as_bytes(),
            &typed,
            &CsvImportLimits::default(),
        )
        .unwrap();
        assert!(
            result.dataset.rows[0]["v"].as_number().unwrap().is_f64(),
            "{valid}"
        );
        if valid.starts_with("-0") {
            assert!(
                result.dataset.rows[0]["v"]
                    .as_f64()
                    .unwrap()
                    .is_sign_negative()
            );
        }
    }
    for invalid in [
        "",
        "NaN",
        "nan",
        "inf",
        "Infinity",
        "-Infinity",
        "+1",
        "01",
        "-01",
        ".1",
        "1.",
        "1e",
        "1e+",
        "1e-",
        "--1",
        "1_000",
        " 1",
        "1 ",
        "0x10",
        "1e309",
        "-1e309",
        "1e-9999",
        "-1e-9999",
        "2e-324",
    ] {
        assert_bad(
            format!("id,v\na,{invalid}").as_bytes(),
            &typed,
            "VIZ-CSV-0008",
        );
    }
    let result = import_csv(b"id,v\na,5e-324", &typed, &CsvImportLimits::default()).unwrap();
    assert_eq!(result.dataset.rows[0]["v"].as_f64().unwrap().to_bits(), 1);
}

#[test]
fn bool_is_exact_and_there_is_no_na_or_comment_inference() {
    let typed = typed_spec(CsvColumnType::Bool);
    for valid in ["true", "false", "\"true\""] {
        import_csv(
            format!("id,v\na,{valid}").as_bytes(),
            &typed,
            &CsvImportLimits::default(),
        )
        .unwrap();
    }
    for invalid in [
        "", "True", "FALSE", "1", "0", "yes", " true", "false ", "null", "NA",
    ] {
        assert_bad(
            format!("id,v\na,{invalid}").as_bytes(),
            &typed,
            "VIZ-CSV-0008",
        );
    }
    let result = import_csv(
        b"id,v\n#comment,NA\nnull,null\ntrue,",
        &typed_spec(CsvColumnType::String),
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert_eq!(result.row_count, 3);
    assert_eq!(result.dataset.rows[0]["v"], json!("NA"));
    assert_eq!(result.dataset.rows[1]["v"], json!("null"));
}

#[test]
fn string_keys_use_exact_nonempty_canonical_values() {
    assert_bad(b"id\na\na", &string_spec(), "VIZ-CSV-0009");
    assert_bad(b"id\na\n\"a\"", &string_spec(), "VIZ-CSV-0009");
    let result = import_csv(
        "id\na\n a\n \né\né".as_bytes(),
        &string_spec(),
        &CsvImportLimits::default(),
    )
    .unwrap();
    assert_eq!(result.row_count, 5);
}

#[test]
fn strict_spec_rejects_unknown_duplicate_missing_null_and_extension_members() {
    for object in [
        json!({}),
        json!({"format":null,"columns":[],"key":"id"}),
        json!({"format":CSV_IMPORT_FORMAT,"columns":null,"key":"id"}),
        json!({"format":CSV_IMPORT_FORMAT,"columns":[],"key":null}),
    ] {
        assert!(serde_json::from_value::<CsvImportSpec>(object).is_err());
    }
    for member in ["path", "url", "nullable", "default", "unknown"] {
        let mut value = wire();
        value[member] = json!(false);
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
        let mut value = wire();
        value["columns"][0][member] = json!(false);
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
    }
    for member in ["format", "columns", "key"] {
        let mut value = wire();
        value[member] = Value::Null;
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
        let mut value = wire();
        value.as_object_mut().unwrap().remove(member);
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
    }
    for member in ["name", "type"] {
        let mut value = wire();
        value["columns"][0][member] = Value::Null;
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
        let mut value = wire();
        value["columns"][0].as_object_mut().unwrap().remove(member);
        assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
    }
    for raw in [
        r#"{"format":"vizir-csv-import/1","format":"vizir-csv-import/1","columns":[{"name":"id","type":"string"}],"key":"id"}"#,
        r#"{"format":"vizir-csv-import/1","columns":[{"name":"id","name":"id","type":"string"}],"key":"id"}"#,
        r#"{"format":"vizir-csv-import/1","columns":[{"name":"id","type":"string","type":"string"}],"key":"id"}"#,
        r#"{"format":"vizir-csv-import/1","columns":[{"name":"id","type":"string"}],"key":"id","key":"id"}"#,
        r#"{"format":"vizir-csv-import/1","columns":[{"name":"id","type":"string"}],"columns":[],"key":"id"}"#,
    ] {
        assert!(serde_json::from_str::<CsvImportSpec>(raw).is_err());
        assert!(parse_csv_import_spec(raw.as_bytes(), &CsvImportLimits::default()).is_err());
    }
}

#[test]
fn spec_semantics_apply_to_generic_serde_and_direct_structs() {
    let mut invalid_specs = Vec::new();
    let mut bad = string_spec();
    bad.format = "vizir-csv-import/2".to_owned();
    invalid_specs.push(bad);
    let mut bad = string_spec();
    bad.columns.clear();
    invalid_specs.push(bad);
    let mut bad = string_spec();
    bad.columns.push(bad.columns[0].clone());
    invalid_specs.push(bad);
    let mut bad = string_spec();
    bad.key = "missing".to_owned();
    invalid_specs.push(bad);
    invalid_specs.push(spec(&[("id", CsvColumnType::Float64)], "id"));
    invalid_specs.push(spec(&[("id", CsvColumnType::Bool)], "id"));
    for name in [
        "",
        "a\0b",
        "a\tb",
        "a\nb",
        "a\rb",
        "a\u{7f}b",
        "a\u{80}b",
        "a\u{9f}b",
        "a\u{2028}b",
        "a\u{2029}b",
    ] {
        invalid_specs.push(spec(&[(name, CsvColumnType::String)], name));
    }
    invalid_specs.push(spec(
        &[(&"é".repeat(129), CsvColumnType::String)],
        &"é".repeat(129),
    ));
    for bad in invalid_specs {
        assert!(validate_csv_import_spec(&bad, &CsvImportLimits::default()).is_err());
        assert!(import_csv(b"id\na", &bad, &CsvImportLimits::default()).is_err());
        assert!(
            serde_json::from_value::<CsvImportSpec>(serde_json::to_value(&bad).unwrap()).is_err()
        );
    }
    let exact = "é".repeat(128);
    let valid = spec(&[(&exact, CsvColumnType::String)], &exact);
    assert_eq!(exact.len(), CSV_IMPORT_MAX_NAME_BYTES);
    validate_csv_import_spec(&valid, &CsvImportLimits::default()).unwrap();
    serde_json::from_value::<CsvImportSpec>(serde_json::to_value(valid).unwrap()).unwrap();
}

#[test]
fn spec_decode_is_bounded_before_column_growth_and_checks_trailing_input() {
    let columns = (0..=CSV_IMPORT_MAX_COLUMNS)
        .map(|index| json!({"name":format!("c{index}"),"type":"string"}))
        .collect::<Vec<_>>();
    let value = json!({"format":CSV_IMPORT_FORMAT,"columns":columns,"key":"c0"});
    assert!(serde_json::from_value::<CsvImportSpec>(value).is_err());
    let raw = serde_json::to_vec(&wire()).unwrap();
    let mut limits = CsvImportLimits {
        max_spec_bytes: raw.len(),
        ..CsvImportLimits::default()
    };
    parse_csv_import_spec(&raw, &limits).unwrap();
    limits.max_spec_bytes -= 1;
    assert!(
        parse_csv_import_spec(&raw, &limits)
            .unwrap_err()
            .to_string()
            .contains("VIZ-CSV-0010")
    );
    let limits = CsvImportLimits {
        max_columns: 0,
        ..CsvImportLimits::default()
    };
    assert!(parse_csv_import_spec(&raw, &limits).is_err());
    let limits = CsvImportLimits {
        max_columns: 1,
        ..CsvImportLimits::default()
    };
    parse_csv_import_spec(&raw, &limits).unwrap();
    let two = serde_json::to_vec(&typed_spec(CsvColumnType::String)).unwrap();
    assert!(parse_csv_import_spec(&two, &limits).is_err());
    let mut trailing = raw;
    trailing.extend_from_slice(b" true");
    assert!(parse_csv_import_spec(&trailing, &CsvImportLimits::default()).is_err());
}

#[test]
fn all_limits_only_tighten_and_checked_arithmetic_cannot_wrap() {
    let hard = CsvImportLimits::default();
    let invalid = [
        CsvImportLimits {
            max_csv_bytes: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_spec_bytes: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_rows: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_columns: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_cells: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_cell_bytes: usize::MAX,
            ..hard
        },
        CsvImportLimits {
            max_retained_bytes: usize::MAX,
            ..hard
        },
    ];
    for limits in invalid {
        assert!(limits.validate().is_err());
        assert!(
            import_csv(b"id\na", &string_spec(), &limits)
                .unwrap_err()
                .to_string()
                .contains("VIZ-CSV-0001")
        );
        assert!(parse_csv_import_spec(b"{}", &limits).is_err());
    }
    let zero = CsvImportLimits {
        max_csv_bytes: 0,
        max_spec_bytes: 0,
        max_rows: 0,
        max_columns: 0,
        max_cells: 0,
        max_cell_bytes: 0,
        max_retained_bytes: 0,
    };
    zero.validate().unwrap();
    assert!(import_csv(b"id\na", &string_spec(), &zero).is_err());
}

#[test]
fn tiny_byte_row_column_cell_limits_accept_boundary_and_reject_next_unit() {
    let csv = b"id,v\na,xy";
    let spec = typed_spec(CsvColumnType::String);
    let limits = CsvImportLimits {
        max_csv_bytes: csv.len(),
        max_rows: 1,
        max_columns: 2,
        max_cells: 2,
        max_cell_bytes: 2,
        ..CsvImportLimits::default()
    };
    import_csv(csv, &spec, &limits).unwrap();
    for lower in [
        CsvImportLimits {
            max_csv_bytes: csv.len() - 1,
            ..limits
        },
        CsvImportLimits {
            max_rows: 0,
            ..limits
        },
        CsvImportLimits {
            max_columns: 1,
            ..limits
        },
        CsvImportLimits {
            max_cells: 1,
            ..limits
        },
        CsvImportLimits {
            max_cell_bytes: 1,
            ..limits
        },
    ] {
        assert!(import_csv(csv, &spec, &lower).is_err());
    }
    let limits = CsvImportLimits {
        max_rows: 1,
        ..CsvImportLimits::default()
    };
    assert!(import_csv(b"id\na\nb", &string_spec(), &limits).is_err());
    let limits = CsvImportLimits {
        max_cells: 3,
        ..CsvImportLimits::default()
    };
    assert!(import_csv(b"id,v\na,x\nb,y", &spec, &limits).is_err());
}

#[test]
fn decoded_cell_limit_counts_unescaped_utf8_and_crlf_bytes() {
    let spec = typed_spec(CsvColumnType::String);
    let limits = CsvImportLimits {
        max_cell_bytes: 2,
        ..CsvImportLimits::default()
    };
    for csv in ["id,v\na,é", "id,v\na,\"\"\"\"\"\"", "id,v\na,\"\r\n\""] {
        import_csv(csv.as_bytes(), &spec, &limits).unwrap();
    }
    for csv in [
        "id,v\na,éx",
        "id,v\na,\"\r\nx\"",
        "id,v\na,\"\"\"\"\"\"\"\"\"",
    ] {
        assert!(
            import_csv(csv.as_bytes(), &spec, &limits)
                .unwrap_err()
                .to_string()
                .contains("VIZ-CSV-0010")
        );
    }
    let exact = format!("id,v\na,{}", "x".repeat(CSV_IMPORT_MAX_CELL_BYTES));
    import_csv(exact.as_bytes(), &spec, &CsvImportLimits::default()).unwrap();
    assert_bad(format!("{exact}x").as_bytes(), &spec, "VIZ-CSV-0010");
}

#[test]
fn retained_accounting_includes_spec_header_key_and_duplicate_set_before_allocations() {
    let string_key_spec = string_spec();
    let expected = CSV_IMPORT_FORMAT.len() + 2 + 2 + 2 + 2 + 2 + 1 + 1;
    let limits = CsvImportLimits {
        max_retained_bytes: expected,
        ..CsvImportLimits::default()
    };
    let result = import_csv(b"id\na", &string_key_spec, &limits).unwrap();
    assert_eq!(result.retained_content_bytes, expected);
    let limits = CsvImportLimits {
        max_retained_bytes: expected - 1,
        ..limits
    };
    assert!(
        import_csv(b"id\na", &string_key_spec, &limits)
            .unwrap_err()
            .to_string()
            .contains("VIZ-CSV-0010")
    );
    let int_spec = spec(&[("id", CsvColumnType::Int64)], "id");
    let result = import_csv(b"id\n-0", &int_spec, &CsvImportLimits::default()).unwrap();
    assert_eq!(
        result.retained_content_bytes,
        CSV_IMPORT_FORMAT.len() + 2 + 2 + 2 + 2 + 2 + 1
    );
    let spec_charge = CSV_IMPORT_FORMAT.len() + 2 + 2;
    let raw = serde_json::to_vec(&wire()).unwrap();
    let limits = CsvImportLimits {
        max_retained_bytes: spec_charge,
        ..CsvImportLimits::default()
    };
    parse_csv_import_spec(&raw, &limits).unwrap();
    let limits = CsvImportLimits {
        max_retained_bytes: spec_charge - 1,
        ..limits
    };
    assert!(parse_csv_import_spec(&raw, &limits).is_err());
}

#[test]
fn repeated_column_name_amplification_hits_retained_hardcap() {
    let mut columns = vec![("id".to_owned(), CsvColumnType::Int64)];
    for index in 1..128 {
        columns.push((
            format!("{}{:03}", "n".repeat(253), index),
            CsvColumnType::String,
        ));
    }
    let spec = CsvImportSpec {
        format: CSV_IMPORT_FORMAT.to_owned(),
        columns: columns
            .iter()
            .map(|(name, column_type)| CsvImportColumn {
                name: name.clone(),
                column_type: *column_type,
            })
            .collect(),
        key: "id".to_owned(),
    };
    let mut csv = columns
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    csv.push('\n');
    for row in 0..1100 {
        csv.push_str(&row.to_string());
        csv.push_str(&",".repeat(127));
        csv.push('\n');
    }
    assert!(csv.len() < 256 * 1024);
    assert!(CSV_IMPORT_MAX_RETAINED_BYTES > csv.len() * 100);
    assert_bad(csv.as_bytes(), &spec, "VIZ-CSV-0010");
}

#[test]
fn oversized_raw_input_rejected_and_diagnostics_never_echo_cells() {
    let oversized = vec![b'x'; CSV_IMPORT_MAX_CSV_BYTES + 1];
    assert_bad(&oversized, &string_spec(), "VIZ-CSV-0010");
    let huge = format!("id,v\na,{}", "9".repeat(64 * 1024));
    let diagnostic = import_csv(
        huge.as_bytes(),
        &typed_spec(CsvColumnType::Int64),
        &CsvImportLimits::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(diagnostic.contains("VIZ-CSV-0008"));
    assert!(diagnostic.contains("record 2 column 2 byte 7"));
    assert!(!diagnostic.contains("999999"));
    assert!(diagnostic.len() < 250);
}

#[test]
fn independent_schema_is_closed_required_and_exactly_typed() {
    let schema = csv_import_spec_schema();
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(
        schema["properties"]["format"]["const"],
        json!(CSV_IMPORT_FORMAT)
    );
    assert_eq!(schema["properties"]["columns"]["minItems"], json!(1));
    assert_eq!(schema["properties"]["columns"]["maxItems"], json!(128));
    assert_eq!(schema["required"], json!(["format", "columns", "key"]));
    assert_eq!(
        schema["$defs"]["CsvImportColumn"]["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        schema["$defs"]["CsvColumnType"]["enum"],
        json!(["string", "int64", "float64", "bool"])
    );
}

#[test]
fn generic_yaml_does_not_coerce_scalar_names_keys_or_types_to_strings() {
    for yaml in [
        "format: vizir-csv-import/1\ncolumns: [{name: 123, type: string}]\nkey: '123'",
        "format: vizir-csv-import/1\ncolumns: [{name: '123', type: string}]\nkey: 123",
        "format: vizir-csv-import/1\ncolumns: [{name: true, type: string}]\nkey: 'true'",
        "format: vizir-csv-import/1\ncolumns: [{name: id, type: true}]\nkey: id",
    ] {
        assert!(
            serde_yaml::from_str::<CsvImportSpec>(yaml).is_err(),
            "{yaml}"
        );
    }
    serde_yaml::from_str::<CsvImportSpec>(
        "format: vizir-csv-import/1\ncolumns: [{name: '123', type: string}]\nkey: '123'",
    )
    .unwrap();
}

#[test]
fn schema_names_require_absolute_end_and_runtime_rejects_every_control_suffix() {
    let schema = csv_import_spec_schema();
    // A bare `$` permits a match before a terminal LF in JSON Schema regex
    // engines. The lookahead requires actual end-of-input in both ECMA and
    // Python-compatible validators, without relying on a nonportable `\z`.
    for pattern in [
        &schema["properties"]["key"]["pattern"],
        &schema["$defs"]["CsvImportColumn"]["properties"]["name"]["pattern"],
    ] {
        assert!(pattern.as_str().unwrap().ends_with(r"$(?![\s\S])"));
    }
    let controls = (0..=0x1f).chain(0x7f..=0x9f).chain([0x2028, 0x2029]);
    for control in controls {
        let suffix = char::from_u32(control).unwrap();
        let mut invalid_column = typed_spec(CsvColumnType::String);
        invalid_column.columns[1].name = format!("value{suffix}");
        assert!(validate_csv_import_spec(&invalid_column, &CsvImportLimits::default()).is_err());
        let mut invalid_key = string_spec();
        invalid_key.key = format!("id{suffix}");
        invalid_key.columns[0].name.clone_from(&invalid_key.key);
        assert!(validate_csv_import_spec(&invalid_key, &CsvImportLimits::default()).is_err());
        assert!(
            serde_json::from_value::<CsvImportSpec>(serde_json::to_value(invalid_key).unwrap())
                .is_err()
        );
    }
    let mut trailing_crlf = string_spec();
    trailing_crlf.key = "id\r\n".to_owned();
    trailing_crlf.columns[0].name.clone_from(&trailing_crlf.key);
    assert!(validate_csv_import_spec(&trailing_crlf, &CsvImportLimits::default()).is_err());
}
