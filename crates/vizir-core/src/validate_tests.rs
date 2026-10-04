use super::*;

#[test]
fn color_validation_accepts_portable_values() {
    let mut diagnostics = Vec::new();
    validate_color(&Color("#12aBcDff".to_owned()), "color", &mut diagnostics);
    validate_color(&Color::transparent(), "color", &mut diagnostics);
    assert!(diagnostics.is_empty());
}

#[test]
fn color_validation_rejects_css_names() {
    let mut diagnostics = Vec::new();
    validate_color(
        &Color("rebeccapurple".to_owned()),
        "color",
        &mut diagnostics,
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.pop().unwrap();
    assert_eq!(diagnostic.code, "VIZ-TYPE-0004");
    assert_eq!(
        diagnostic.message,
        "invalid portable color \"rebeccapurple\""
    );
    assert_eq!(diagnostic.source.as_deref(), Some("color"));
    assert_eq!(
        diagnostic.help.as_deref(),
        Some("use transparent, #RRGGBB, or #RRGGBBAA")
    );
}

#[test]
fn color_validation_enforces_exact_hex_lengths() {
    for (value, accepted) in [
        ("#12aBcD", true),     // #RRGGBB
        ("#12aBcDff", true),   // #RRGGBBAA
        ("#12aBc", false),     // one short of #RRGGBB
        ("#12aBcDf", false),   // between the two portable lengths
        ("#12aBcDff0", false), // one past #RRGGBBAA
        ("12aBcD", false),     // right shape, missing the '#' prefix
        ("#12aBGcD", false),   // right length, non-hex digit
    ] {
        let mut diagnostics = Vec::new();
        validate_color(&Color(value.to_owned()), "color", &mut diagnostics);
        assert_eq!(diagnostics.len(), usize::from(!accepted), "{value}");
        if !accepted {
            assert_eq!(diagnostics[0].code, "VIZ-TYPE-0004", "{value}");
        }
    }
}

#[test]
fn unit_validation_enforces_the_inclusive_zero_to_one_range() {
    for value in [0.0, 1.0, 0.5] {
        let mut diagnostics = Vec::new();
        validate_unit(value, "opacity", &mut diagnostics);
        assert!(diagnostics.is_empty(), "{value}");
    }
    for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        let mut diagnostics = Vec::new();
        validate_unit(value, "opacity", &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "{value}");
        assert_eq!(diagnostics[0].code, "VIZ-TYPE-0005", "{value}");
        assert_eq!(diagnostics[0].source.as_deref(), Some("opacity"), "{value}");
    }
}

#[test]
fn finite_validation_rejects_nan_and_infinities_only() {
    for value in [0.0, -1.5, 1e300] {
        let mut diagnostics = Vec::new();
        validate_finite(value, "width", &mut diagnostics);
        assert!(diagnostics.is_empty(), "{value}");
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut diagnostics = Vec::new();
        validate_finite(value, "width", &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "{value}");
        assert_eq!(diagnostics[0].code, "VIZ-TYPE-0006", "{value}");
    }
}

#[test]
fn validation_rejects_duplicate_data_keys() {
    let document: Document = serde_yaml::from_str(
        r##"
version: "0.1"
id: duplicate-keys
width: 400
height: 300
datasets:
  points:
    key: id
    rows:
      - {id: same, x: 1, y: 2}
      - {id: same, x: 2, y: 3}
views:
  - kind: chart.scatter
    id: points
    frame: {x: 0, y: 0, width: 400, height: 300}
    dataset: points
    x: {field: x}
    y: {field: y}
"##,
    )
    .unwrap();
    let diagnostics = validate_document(&document).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|value| value.code == "VIZ-VALIDATE-0102")
    );
}

#[test]
fn validation_rejects_dangling_diagram_edges() {
    let document: Document = serde_yaml::from_str(
        r##"
version: "0.1"
id: dangling-edge
width: 400
height: 300
views:
  - kind: diagram.graph
    id: graph
    frame: {x: 0, y: 0, width: 400, height: 300}
    nodes:
      - {id: source, label: Source}
    edges:
      - {from: source, to: missing}
"##,
    )
    .unwrap();
    let diagnostics = validate_document(&document).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|value| value.code == "VIZ-VALIDATE-0204")
    );
}

#[test]
fn numeric_chart_span_validation_checks_each_axis_without_changing_finite_controls() {
    for kind in ["chart.line", "chart.scatter", "chart.bar"] {
        for field in ["x", "y"] {
            let chart_fields = if kind == "chart.bar" {
                format!("    category: {{field: category}}\n    value: {{field: {field}}}\n")
            } else {
                "    x: {field: x}\n    y: {field: y}\n".to_owned()
            };
            for (minimum, maximum, rejected) in [
                (-1e308, 1e308, true),
                (-3.0, 5.0, false),
                (0.0, 0.0, false),
                (1e150, 3e150, false),
            ] {
                let rows = [minimum, maximum]
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let x = if field == "x" { *value } else { index as f64 };
                        let y = if field == "y" { *value } else { index as f64 };
                        format!("      - {{id: row-{index}, category: row-{index}, x: {x:e}, y: {y:e}}}\n")
                    })
                    .collect::<String>();
                let source = format!(
                    "version: \"0.1\"\nid: numeric-span\nwidth: 640\nheight: 400\ndatasets:\n  values:\n    key: id\n    rows:\n{rows}views:\n  - kind: {kind}\n    id: chart\n    frame: {{x: 0, y: 0, width: 640, height: 400}}\n    dataset: values\n{chart_fields}"
                );
                let document: Document = serde_yaml::from_str(&source).unwrap();
                let result = validate_document(&document);
                if rejected {
                    let diagnostics = result.unwrap_err();
                    assert_eq!(diagnostics.len(), 1, "{kind} {field}");
                    assert_eq!(diagnostics[0].code, "VIZ-TYPE-0106");
                    assert_eq!(
                        diagnostics[0].source.as_deref(),
                        Some(format!("datasets.values.rows.{field}").as_str())
                    );
                } else {
                    assert!(result.is_ok(), "{kind} {field}: {result:?}");
                }
            }
        }
    }
}
