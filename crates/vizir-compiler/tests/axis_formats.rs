use std::path::PathBuf;

use serde_json::{Value, json};
use vizir_compiler::{Compilation, build_scene, compile, lower_to_mir};
use vizir_core::{
    AxisOptions, Document, GuideKind, GuideOrient, MirScale, MirView, NumberFormat, NumberNotation,
    Rect, Scene2D, SceneNode, View, VizMir, find_scene_node, mir_schema, parse_document,
    validate_document, validate_mir,
};

fn example(name: &str) -> Document {
    parse_document(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chart")
            .join(name),
    )
    .unwrap()
}

fn fixture(version: &str) -> Value {
    json!({
        "version": version,
        "id": "axis-format-contract",
        "width": 800,
        "height": 480,
        "datasets": {
            "samples": {"key": "id", "rows": [
                {"id": "a", "x": 0.0, "y": 0.0, "group": "A"},
                {"id": "b", "x": 1.0, "y": 1.0, "group": "B"}
            ]}
        },
        "views": [{
            "kind": "chart.scatter",
            "id": "format-test",
            "frame": {"x": 0.0, "y": 0.0, "width": 800.0, "height": 480.0},
            "dataset": "samples",
            "x": {"field": "x", "label": "x"},
            "y": {"field": "y", "label": "y"}
        }]
    })
}

fn document(value: Value) -> Document {
    serde_json::from_value(value).unwrap()
}

fn format(notation: &str, precision: u8) -> Value {
    json!({"number_format": {"notation": notation, "precision": precision}})
}

fn formatted_fixture() -> Value {
    let mut source = fixture("0.2");
    source["views"][0]["x"]["axis"] = format("scientific", 2);
    source["views"][0]["y"]["axis"] = format("fixed", 3);
    source
}

fn tick<'a>(scene: &'a Scene2D, chart: &str, axis: &str, index: usize) -> &'a str {
    let node = find_scene_node(&scene.nodes, &format!("{chart}/axis/{axis}/label/{index}"))
        .expect("numeric tick is present");
    let SceneNode::Text { text, .. } = node else {
        panic!("numeric tick must be text")
    };
    text
}

fn numeric_tick_texts(scene: &Scene2D) -> Vec<(String, String)> {
    fn collect(nodes: &[SceneNode], output: &mut Vec<(String, String)>) {
        for node in nodes {
            match node {
                SceneNode::Group { children, .. } => collect(children, output),
                SceneNode::Text { id, text, .. }
                    if id.contains("/axis/") && id.contains("/label/") =>
                {
                    output.push((id.clone(), text.clone()));
                }
                _ => {}
            }
        }
    }
    let mut output = Vec::new();
    collect(&scene.nodes, &mut output);
    output
}

fn guide_policies(mir: &VizMir) -> Vec<Value> {
    mir.views
        .iter()
        .filter_map(|view| {
            let MirView::Chart(chart) = view else {
                return None;
            };
            Some(serde_json::to_value(&chart.guides).unwrap())
        })
        .collect()
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

fn assert_numeric_ticks_fit(compilation: &Compilation) {
    for view in &compilation.mir.views {
        let MirView::Chart(chart) = view else {
            continue;
        };
        let SceneNode::Group { children, .. } =
            find_scene_node(&compilation.scene.nodes, &chart.id).unwrap()
        else {
            panic!("chart group")
        };
        let text_nodes = children
            .iter()
            .filter(|node| matches!(node, SceneNode::Text { .. }))
            .collect::<Vec<_>>();
        let numeric_ticks = text_nodes
            .iter()
            .copied()
            .filter(|node| {
                node.id().starts_with(&format!("{}/axis/", chart.id))
                    && node.id().contains("/label/")
            })
            .collect::<Vec<_>>();
        assert!(!numeric_ticks.is_empty());
        for node in numeric_ticks {
            let envelope = bounds(node);
            let tolerance = 16.0 * f64::EPSILON * chart.frame.width.max(chart.frame.height);
            assert!(
                envelope.x >= chart.frame.x - tolerance
                    && envelope.x + envelope.width <= chart.frame.x + chart.frame.width + tolerance
                    && envelope.y >= chart.frame.y - tolerance
                    && envelope.y + envelope.height
                        <= chart.frame.y + chart.frame.height + tolerance,
                "{} escapes {:?}: {envelope:?}",
                node.id(),
                chart.frame,
            );
            if let SceneNode::Text {
                font_size, text, ..
            } = node
            {
                assert_eq!(*font_size, 11.0, "tick text must not be shrunk");
                assert!(!text.contains('…'), "tick text must not be truncated");
                assert!(!text.contains("inf") && !text.contains("NaN"));
            }
            for other in &text_nodes {
                if node.id() != other.id() {
                    assert!(
                        !intersects(envelope, bounds(other)),
                        "{} overlaps {}: {:?} / {:?}",
                        node.id(),
                        other.id(),
                        envelope,
                        bounds(other)
                    );
                }
            }
        }
    }
}

#[test]
fn old_documents_omit_new_fields_and_preserve_legacy_text_and_scene() {
    let mut source = fixture("0.1");
    source["datasets"]["samples"]["rows"][1]["x"] = json!(5000.0);
    let old = document(source.clone());
    let encoded = serde_json::to_value(&old).unwrap();
    assert!(encoded["views"][0]["x"].get("axis").is_none());
    assert!(encoded["views"][0]["y"].get("axis").is_none());
    let old_result = compile(&old).unwrap();
    assert_eq!(old_result.mir.version, "0.1");
    assert_eq!(old_result.mir.source_hir_version, "0.1");
    let encoded_mir = serde_json::to_value(&old_result.mir).unwrap();
    for guide in encoded_mir["views"][0]["guides"].as_array().unwrap() {
        assert!(guide.get("number_format").is_none());
    }
    let expected = ["0", "1.0k", "2.0k", "3.0k", "4.0k", "5.0k"];
    for (index, expected) in expected.into_iter().enumerate() {
        assert_eq!(tick(&old_result.scene, "format-test", "x", index), expected);
    }
    source["version"] = json!("0.2");
    let upgraded = compile(&document(source)).unwrap();
    assert_eq!(upgraded.mir.version, "0.2");
    assert_eq!(upgraded.mir.source_hir_version, "0.2");
    assert_eq!(upgraded.scene, old_result.scene);
    let decoded: VizMir = serde_json::from_value(encoded_mir).unwrap();
    assert_eq!(build_scene(&decoded).unwrap(), old_result.scene);
}

#[test]
fn omitted_hir_version_still_defaults_to_01_and_explicit_axis_requires_02() {
    let mut source = fixture("0.1");
    source.as_object_mut().unwrap().remove("version");
    assert_eq!(document(source.clone()).version, "0.1");
    for axis in [json!({}), format("scientific", 2)] {
        source["views"][0]["x"]["axis"] = axis;
        let invalid = document(source.clone());
        let error = compile(&invalid).unwrap_err().to_string();
        assert!(error.contains("VIZ-SCHEMA-0002"), "{error}");
        assert!(error.contains("views[0].x.axis"), "{error}");
        assert!(
            lower_to_mir(&invalid).is_err(),
            "public lowering must validate"
        );
    }
}

#[test]
fn typed_hir_format_options_lower_losslessly_to_mir_02_guides() {
    let source = document(formatted_fixture());
    let View::Scatter(chart) = &source.views[0] else {
        unreachable!()
    };
    assert_eq!(
        chart.x.axis,
        Some(AxisOptions {
            number_format: Some(NumberFormat {
                notation: NumberNotation::Scientific,
                precision: 2,
            }),
        })
    );
    let result = compile(&source).unwrap();
    assert_eq!(result.mir.version, "0.2");
    assert_eq!(result.mir.source_hir_version, "0.2");
    let MirView::Chart(chart) = &result.mir.views[0] else {
        unreachable!()
    };
    for (orient, notation, precision) in [
        (GuideOrient::Bottom, NumberNotation::Scientific, 2),
        (GuideOrient::Left, NumberNotation::Fixed, 3),
    ] {
        let guide = chart
            .guides
            .iter()
            .find(|guide| guide.orient == orient)
            .unwrap();
        assert_eq!(
            guide.number_format,
            Some(NumberFormat {
                notation,
                precision
            })
        );
    }
    assert_eq!(tick(&result.scene, "format-test", "x", 5), "1.00e0");
    assert_eq!(tick(&result.scene, "format-test", "y", 0), "1.000");
    assert_numeric_ticks_fit(&result);
}

#[test]
fn unsupported_versions_are_rejected_at_public_entry_points() {
    for version in ["0.0", "0.10", "1.0", "latest"] {
        let source = document(fixture(version));
        assert!(
            compile(&source)
                .unwrap_err()
                .to_string()
                .contains("version")
        );
        assert!(lower_to_mir(&source).is_err());
        let mut mir = compile(&document(fixture("0.1"))).unwrap().mir;
        mir.version = version.to_owned();
        assert!(validate_mir(&mir).is_err());
        assert!(build_scene(&mir).is_err());
    }
}

#[test]
fn hir_number_format_is_a_strict_closed_contract() {
    for invalid in [
        json!({"notation": "engineering", "precision": 2}),
        json!({"notation": "Scientific", "precision": 2}),
        json!({"notation": "scientific"}),
        json!({"precision": 2}),
        json!({"notation": "fixed", "precision": -1}),
        json!({"notation": "fixed", "precision": 2.5}),
        json!({"notation": "fixed", "precision": 256}),
        json!({"notation": "fixed", "precision": "3"}),
        json!({"notation": "fixed", "precision": 3, "suffix": "V"}),
    ] {
        let mut source = formatted_fixture();
        source["views"][0]["x"]["axis"]["number_format"] = invalid.clone();
        assert!(
            serde_json::from_value::<Document>(source).is_err(),
            "{invalid}"
        );
    }
    let mut unknown_axis = formatted_fixture();
    unknown_axis["views"][0]["x"]["axis"]["tick_count"] = json!(3);
    assert!(serde_json::from_value::<Document>(unknown_axis).is_err());
    for precision in [13, 255] {
        let mut source = formatted_fixture();
        source["views"][0]["x"]["axis"]["number_format"]["precision"] = json!(precision);
        let invalid = document(source);
        let error = compile(&invalid).unwrap_err().to_string();
        assert!(
            error.contains("VIZ-TYPE-0107") && error.contains("precision"),
            "{error}"
        );
        assert!(validate_document(&invalid).is_err());
    }
}

fn replace_precision_token(serialized: &str, current: u8, replacement: &str) -> String {
    let field = format!("\"precision\":{current}");
    assert!(
        serialized.contains(&field),
        "precision fixture token is present"
    );
    serialized.replacen(&field, &format!("\"precision\":{replacement}"), 1)
}

#[test]
fn integer_valued_json_precision_syntax_is_accepted_and_canonicalized() {
    let mut source = formatted_fixture();
    source["width"] = json!(1600.0);
    source["views"][0]["frame"]["width"] = json!(1600.0);
    source["datasets"]["samples"]["rows"][1]["x"] = json!(10.0);
    let encoded_hir = serde_json::to_string(&source).unwrap();
    for (token, expected) in [
        ("2", 2),
        ("2.0", 2),
        ("2e0", 2),
        ("2E+0", 2),
        ("0.0", 0),
        ("-0.0", 0),
        ("0e0", 0),
        ("12.0", 12),
    ] {
        let hir_json = replace_precision_token(&encoded_hir, 2, token);
        let parsed: Document = serde_json::from_str(&hir_json)
            .unwrap_or_else(|error| panic!("HIR precision {token}: {error}"));
        validate_document(&parsed).unwrap();
        let canonical_hir = serde_json::to_value(&parsed).unwrap();
        let precision = &canonical_hir["views"][0]["x"]["axis"]["number_format"]["precision"];
        assert_eq!(precision.as_u64(), Some(expected as u64), "HIR {token}");
        assert!(
            !precision.is_f64(),
            "HIR {token} must serialize as an integer"
        );
        let compiled = compile(&parsed).unwrap();
        let mir_json = replace_precision_token(
            &serde_json::to_string(&compiled.mir).unwrap(),
            expected,
            token,
        );
        let parsed_mir: VizMir = serde_json::from_str(&mir_json)
            .unwrap_or_else(|error| panic!("MIR precision {token}: {error}"));
        validate_mir(&parsed_mir).unwrap();
        let canonical_mir = serde_json::to_value(&parsed_mir).unwrap();
        let precision = &canonical_mir["views"][0]["guides"][0]["number_format"]["precision"];
        assert_eq!(precision.as_u64(), Some(expected as u64), "MIR {token}");
        assert!(
            !precision.is_f64(),
            "MIR {token} must serialize as an integer"
        );
        let rebuilt = build_scene(&parsed_mir).unwrap();
        assert_eq!(
            numeric_tick_texts(&rebuilt),
            numeric_tick_texts(&compiled.scene),
            "MIR {token}"
        );
        assert_numeric_ticks_fit(&Compilation {
            mir: parsed_mir,
            scene: rebuilt,
        });
    }
}

#[test]
fn json_precision_rejects_fractional_nonnumeric_and_out_of_storage_range_values() {
    let source = document(formatted_fixture());
    let hir_json = serde_json::to_string(&source).unwrap();
    let mir_json = serde_json::to_string(&compile(&source).unwrap().mir).unwrap();
    for token in [
        "2.5", "2.1e0", "-0.5", "-1", "-1.0", "-2e0", "256", "256.0", "2.56e2", "1e308", "\"2\"",
        "true", "false", "null",
    ] {
        assert!(
            serde_json::from_str::<Document>(&replace_precision_token(&hir_json, 2, token))
                .is_err(),
            "HIR precision {token} must fail parsing"
        );
        assert!(
            serde_json::from_str::<VizMir>(&replace_precision_token(&mir_json, 2, token)).is_err(),
            "MIR precision {token} must fail parsing"
        );
    }
}

#[test]
fn integer_valued_precision_outside_format_range_gets_semantic_diagnostics() {
    let source = document(formatted_fixture());
    let hir_json = serde_json::to_string(&source).unwrap();
    let mir_json = serde_json::to_string(&compile(&source).unwrap().mir).unwrap();
    for token in ["13", "13.0", "1.3e1", "255", "255.0", "2.55e2"] {
        let parsed: Document =
            serde_json::from_str(&replace_precision_token(&hir_json, 2, token)).unwrap();
        let diagnostics = validate_document(&parsed).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "VIZ-TYPE-0107"),
            "HIR {token}"
        );
        assert!(
            compile(&parsed)
                .unwrap_err()
                .to_string()
                .contains("VIZ-TYPE-0107")
        );
        let parsed_mir: VizMir =
            serde_json::from_str(&replace_precision_token(&mir_json, 2, token)).unwrap();
        let diagnostics = validate_mir(&parsed_mir).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "VIZ-TYPE-0107"),
            "MIR {token}"
        );
        assert!(
            build_scene(&parsed_mir)
                .unwrap_err()
                .to_string()
                .contains("VIZ-TYPE-0107")
        );
    }
}

#[test]
fn optional_format_fields_must_be_omitted_rather_than_explicit_null() {
    for version in ["0.1", "0.2"] {
        let mut source = fixture(version);
        source["views"][0]["x"]["axis"] = Value::Null;
        assert!(serde_json::from_value::<Document>(source).is_err());
        let mut source = fixture(version);
        source["views"][0]["x"]["axis"] = json!({"number_format": null});
        assert!(serde_json::from_value::<Document>(source).is_err());
        let result = compile(&document(fixture(version))).unwrap();
        let mut encoded = serde_json::to_value(result.mir).unwrap();
        encoded["views"][0]["guides"][0]["number_format"] = Value::Null;
        assert!(serde_json::from_value::<VizMir>(encoded).is_err());
    }
}

#[test]
fn precision_zero_and_twelve_are_inclusive_valid_boundaries() {
    for precision in [0, 12] {
        for notation in ["fixed", "scientific"] {
            let mut source = fixture("0.2");
            source["width"] = json!(1600.0);
            source["views"][0]["frame"]["width"] = json!(1600.0);
            source["views"][0]["x"]["axis"] = format(notation, precision);
            source["views"][0]["y"]["axis"] = format(notation, precision);
            source["datasets"]["samples"]["rows"][1]["x"] = json!(10.0);
            source["datasets"]["samples"]["rows"][1]["y"] = json!(10.0);
            let result = compile(&document(source)).unwrap();
            let expected = match (notation, precision) {
                ("fixed", 0) => "10",
                ("fixed", 12) => "10.000000000000",
                ("scientific", 0) => "1e1",
                ("scientific", 12) => "1.000000000000e1",
                _ => unreachable!(),
            };
            assert_eq!(tick(&result.scene, "format-test", "x", 5), expected);
            assert_numeric_ticks_fit(&result);
        }
    }
}

#[test]
fn duplicate_rounded_ticks_are_diagnosed_without_changing_requested_precision() {
    for (notation, minimum, maximum) in [("fixed", 0.0, 0.1), ("scientific", 100.0, 101.0)] {
        let mut source = fixture("0.2");
        source["views"][0]["x"]["axis"] = format(notation, 0);
        source["datasets"]["samples"]["rows"][0]["x"] = json!(minimum);
        source["datasets"]["samples"]["rows"][1]["x"] = json!(maximum);
        let error = compile(&document(source)).unwrap_err().to_string();
        assert!(error.contains("VIZ-FORMAT-0001"), "{notation}: {error}");
        assert!(
            error.contains("precision") || error.contains("duplicate"),
            "{error}"
        );
    }
}

#[test]
fn old_mir_rejects_explicit_number_format_even_with_new_source_hir_version() {
    let mut mir = compile(&document(formatted_fixture())).unwrap().mir;
    mir.version = "0.1".to_owned();
    assert_eq!(mir.source_hir_version, "0.2");
    let diagnostics = validate_mir(&mir).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "VIZ-MIR-0007")
    );
    let error = build_scene(&mir).unwrap_err().to_string();
    assert!(
        error.contains("VIZ-MIR-0007") && error.contains("number_format"),
        "{error}"
    );
}

#[test]
fn generated_mir_schema_exposes_the_versioned_closed_format_contract() {
    let schema = mir_schema();
    assert_eq!(
        schema["oneOf"][0]["properties"]["version"]["enum"],
        json!(["0.1", "0.2"])
    );
    assert_eq!(
        schema["$defs"]["NumberNotation"]["enum"],
        json!(["scientific", "fixed"])
    );
    let format_schema = &schema["$defs"]["NumberFormat"];
    assert_eq!(format_schema["additionalProperties"], false);
    assert_eq!(format_schema["properties"]["precision"]["type"], "integer");
    assert_eq!(format_schema["properties"]["precision"]["minimum"], 0);
    assert_eq!(format_schema["properties"]["precision"]["maximum"], 12);
    assert_eq!(
        schema["oneOf"][0]["allOf"][0]["if"]["properties"]["version"]["const"],
        "0.1"
    );
    assert_eq!(
        schema["oneOf"][0]["allOf"][0]["then"]["properties"]["views"]["items"]["properties"]["guides"]
            ["items"]["not"]["required"],
        json!(["number_format"])
    );
    assert_eq!(
        schema["$defs"]["MirGuide"]["allOf"][0]["then"]["properties"]["kind"]["const"],
        "axis"
    );
}

#[test]
fn mir_number_format_rejects_unknown_fields_notation_and_out_of_range_precision() {
    let result = compile(&document(formatted_fixture())).unwrap();
    let encoded = serde_json::to_value(&result.mir).unwrap();
    for invalid in [
        json!({"notation": "compact", "precision": 2}),
        json!({"notation": "fixed", "precision": 3, "locale": "en-US"}),
        json!({"notation": "fixed", "precision": -1}),
        json!({"notation": "fixed"}),
    ] {
        let mut changed = encoded.clone();
        changed["views"][0]["guides"][0]["number_format"] = invalid.clone();
        assert!(
            serde_json::from_value::<VizMir>(changed).is_err(),
            "{invalid}"
        );
    }
    let mut changed = encoded;
    changed["views"][0]["guides"][0]["number_format"]["precision"] = json!(13);
    let invalid: VizMir = serde_json::from_value(changed).unwrap();
    assert!(validate_mir(&invalid).is_err());
    let error = build_scene(&invalid).unwrap_err().to_string();
    assert!(error.contains("VIZ-TYPE-0107"), "{error}");
}

#[test]
fn category_axes_reject_formats_even_when_category_values_are_numeric() {
    let mut source = fixture("0.2");
    source["views"][0] = json!({
        "kind": "chart.bar", "id": "format-test", "dataset": "samples",
        "frame": {"x": 0, "y": 0, "width": 800, "height": 480},
        "category": {"field": "x", "axis": format("fixed", 2)},
        "value": {"field": "y"}
    });
    let error = compile(&document(source)).unwrap_err().to_string();
    assert!(
        error.contains("VIZ-TYPE-0108") && error.contains("category"),
        "{error}"
    );
}

#[test]
fn color_and_series_encodings_do_not_accept_axis_options() {
    for (kind, channel) in [("chart.scatter", "color"), ("chart.line", "series")] {
        let mut source = fixture("0.2");
        source["views"][0]["kind"] = json!(kind);
        source["views"][0][channel] = json!({"field": "group", "axis": format("fixed", 2)});
        assert!(serde_json::from_value::<Document>(source).is_err());
    }
}

#[test]
fn direct_mir_rejects_formats_on_legends_and_band_axes() {
    let mut source = fixture("0.2");
    source["views"][0] = json!({
        "kind": "chart.bar", "id": "format-test", "dataset": "samples",
        "frame": {"x": 0, "y": 0, "width": 800, "height": 480},
        "category": {"field": "group"}, "value": {"field": "y"},
        "color": {"field": "group"}
    });
    let bar = compile(&document(source)).unwrap().mir;
    let mut source = fixture("0.2");
    source["views"][0]["color"] = json!({"field": "group"});
    let scatter = compile(&document(source)).unwrap().mir;
    for (kind, mut invalid) in [(GuideKind::Axis, bar), (GuideKind::Legend, scatter)] {
        let MirView::Chart(chart) = &mut invalid.views[0] else {
            unreachable!()
        };
        let guide = chart
            .guides
            .iter_mut()
            .find(|guide| guide.kind == kind && guide.orient != GuideOrient::Left)
            .unwrap();
        guide.number_format = Some(NumberFormat {
            notation: NumberNotation::Fixed,
            precision: 2,
        });
        let error = build_scene(&invalid).unwrap_err().to_string();
        assert!(error.contains("VIZ-TYPE-0108"), "{kind:?}: {error}");
    }
}

#[test]
fn direct_mir_format_is_honored_without_reopening_hir() {
    let mut mir = compile(&document(fixture("0.2"))).unwrap().mir;
    let MirView::Chart(chart) = &mut mir.views[0] else {
        unreachable!()
    };
    let x_guide = chart
        .guides
        .iter_mut()
        .find(|guide| guide.orient == GuideOrient::Bottom)
        .unwrap();
    x_guide.number_format = Some(NumberFormat {
        notation: NumberNotation::Fixed,
        precision: 3,
    });
    let round_trip: VizMir = serde_json::from_slice(&serde_json::to_vec(&mir).unwrap()).unwrap();
    let scene = build_scene(&round_trip).unwrap();
    assert_eq!(tick(&scene, "format-test", "x", 0), "0.000");
    assert_eq!(tick(&scene, "format-test", "x", 5), "1.000");
    assert_numeric_ticks_fit(&Compilation {
        mir: round_trip,
        scene,
    });
}

#[test]
fn scientific_ticks_keep_huge_tiny_negative_and_zero_values_readable() {
    for (minimum, maximum, expected_minimum, expected_maximum) in [
        (0.0, 1.0e150, "0.00e0", "1.00e150"),
        (0.0, 1.0e-120, "0.00e0", "1.00e-120"),
        (-1.0e150, 0.0, "-1.00e150", "0.00e0"),
        (-1.0e-120, 0.0, "-1.00e-120", "0.00e0"),
    ] {
        let mut source = fixture("0.2");
        for axis in ["x", "y"] {
            source["views"][0][axis]["axis"] = format("scientific", 2);
            source["datasets"]["samples"]["rows"][0][axis] = json!(minimum);
            source["datasets"]["samples"]["rows"][1][axis] = json!(maximum);
        }
        let result = compile(&document(source)).unwrap();
        assert_eq!(tick(&result.scene, "format-test", "x", 0), expected_minimum);
        assert_eq!(tick(&result.scene, "format-test", "x", 5), expected_maximum);
        assert_eq!(tick(&result.scene, "format-test", "y", 0), expected_maximum);
        assert_eq!(tick(&result.scene, "format-test", "y", 5), expected_minimum);
        assert_numeric_ticks_fit(&result);
    }
}

#[test]
fn fixed_precision_retains_digits_and_suppresses_rounded_negative_zero() {
    let mut source = fixture("0.2");
    source["views"][0]["x"]["axis"] = format("fixed", 3);
    source["views"][0]["y"]["axis"] = format("fixed", 3);
    source["datasets"]["samples"]["rows"][0]["x"] = json!(-0.0001);
    source["datasets"]["samples"]["rows"][1]["x"] = json!(0.0051);
    source["datasets"]["samples"]["rows"][1]["y"] = json!(0.123);
    let mut result = compile(&document(source)).unwrap();
    let MirView::Chart(chart) = &mut result.mir.views[0] else {
        unreachable!()
    };
    // Direct MIR domains are authoritative. This keeps the layout width while
    // testing a decimal endpoint not selected by the HIR nice-domain policy.
    let y_id = chart
        .guides
        .iter()
        .find(|guide| guide.orient == GuideOrient::Left)
        .unwrap()
        .scale
        .clone();
    let MirScale::Linear { domain, .. } = chart
        .scales
        .iter_mut()
        .find(|scale| scale.id() == y_id)
        .unwrap()
    else {
        unreachable!()
    };
    *domain = [0.0, 0.123];
    let x_id = chart
        .guides
        .iter()
        .find(|guide| guide.orient == GuideOrient::Bottom)
        .unwrap()
        .scale
        .clone();
    let MirScale::Linear { domain, .. } = chart
        .scales
        .iter_mut()
        .find(|scale| scale.id() == x_id)
        .unwrap()
    else {
        unreachable!()
    };
    *domain = [-0.0001, 0.005];
    result.scene = build_scene(&result.mir).unwrap();
    assert_eq!(tick(&result.scene, "format-test", "y", 0), "0.123");
    assert_eq!(tick(&result.scene, "format-test", "x", 0), "0.000");
    for index in 0..=5 {
        assert!(!tick(&result.scene, "format-test", "x", index).starts_with("-0.000"));
    }
    assert_numeric_ticks_fit(&result);
}

#[test]
fn all_three_chart_dialects_lower_numeric_formats() {
    for kind in ["chart.scatter", "chart.line", "chart.bar"] {
        let mut source = fixture("0.2");
        source["views"][0]["kind"] = json!(kind);
        if kind == "chart.bar" {
            let view = source["views"][0].as_object_mut().unwrap();
            view.remove("x");
            view.remove("y");
            view.insert("category".to_owned(), json!({"field": "group"}));
            view.insert(
                "value".to_owned(),
                json!({"field": "y", "axis": format("fixed", 3)}),
            );
        } else {
            source["views"][0]["x"]["axis"] = format("scientific", 2);
            source["views"][0]["y"]["axis"] = format("fixed", 3);
        }
        let result = compile(&document(source)).unwrap();
        assert_eq!(
            tick(&result.scene, "format-test", "y", 0),
            "1.000",
            "{kind}"
        );
        assert_numeric_ticks_fit(&result);
    }
}

#[test]
fn formatting_changes_presentation_without_changing_domains_or_materialized_marks() {
    let mut source = fixture("0.2");
    source["datasets"]["samples"]["rows"][0]["x"] = json!(0.123);
    source["datasets"]["samples"]["rows"][1]["x"] = json!(0.987);
    source["datasets"]["samples"]["rows"][0]["y"] = json!(-13.25);
    source["datasets"]["samples"]["rows"][1]["y"] = json!(42.5);
    let baseline = compile(&document(source.clone())).unwrap();
    let MirView::Chart(baseline_chart) = &baseline.mir.views[0] else {
        unreachable!()
    };
    for (x_notation, y_notation) in [
        ("fixed", "fixed"),
        ("scientific", "scientific"),
        ("fixed", "scientific"),
    ] {
        let mut changed = source.clone();
        changed["views"][0]["x"]["axis"] = format(x_notation, 3);
        changed["views"][0]["y"]["axis"] = format(y_notation, 3);
        let result = compile(&document(changed)).unwrap();
        let MirView::Chart(chart) = &result.mir.views[0] else {
            unreachable!()
        };
        assert_eq!(result.mir.data, baseline.mir.data);
        assert_eq!(result.mir.expressions, baseline.mir.expressions);
        assert_eq!(chart.mark, baseline_chart.mark);
        for (actual, expected) in chart.scales.iter().zip(&baseline_chart.scales) {
            let (
                MirScale::Linear {
                    domain: actual_domain,
                    id: actual_id,
                    ..
                },
                MirScale::Linear {
                    domain: expected_domain,
                    id: expected_id,
                    ..
                },
            ) = (actual, expected)
            else {
                unreachable!()
            };
            assert_eq!(actual_id, expected_id);
            assert_eq!(actual_domain, expected_domain);
        }
        assert_numeric_ticks_fit(&result);
    }
}

#[test]
fn one_formatted_axis_also_reserves_unformatted_numeric_ticks() {
    let mut source = fixture("0.2");
    source["width"] = json!(1600.0);
    source["views"][0]["frame"]["width"] = json!(1600.0);
    source["datasets"]["samples"]["rows"][1]["x"] = json!(1.0e18);
    source["views"][0]["y"]["axis"] = format("fixed", 3);
    let result = compile(&document(source)).unwrap();
    assert_eq!(
        tick(&result.scene, "format-test", "x", 5),
        "1000000000000000.0k"
    );
    assert_numeric_ticks_fit(&result);
}

#[test]
fn impossible_numeric_tick_layout_is_diagnosed_instead_of_clipped() {
    let mut source = formatted_fixture();
    source["views"][0]["frame"]["width"] = json!(220.0);
    source["views"][0]["x"]["axis"] = format("scientific", 12);
    let error = compile(&document(source)).unwrap_err().to_string();
    assert!(error.contains("VIZ-LAYOUT-0006"), "{error}");
    assert!(
        error.contains("tick") || error.contains("number"),
        "{error}"
    );
}

#[test]
fn public_examples_have_complete_nonoverlapping_numeric_tick_envelopes() {
    for name in [
        "scientific-magnitudes.viz.yaml",
        "measurement-precision.viz.yaml",
        "mixed-axis-formats.viz.yaml",
    ] {
        let source = example(name);
        let first = compile(&source).unwrap();
        let second = compile(&source).unwrap();
        assert_eq!(first.mir, second.mir, "{name}");
        assert_eq!(first.scene, second.scene, "{name}");
        assert_numeric_ticks_fit(&first);
        assert_eq!(build_scene(&first.mir).unwrap(), first.scene);
        let decoded_mir: VizMir =
            serde_json::from_slice(&serde_json::to_vec(&first.mir).unwrap()).unwrap();
        let rebuilt = build_scene(&decoded_mir).unwrap();
        assert_eq!(
            guide_policies(&decoded_mir),
            guide_policies(&first.mir),
            "{name}"
        );
        assert_eq!(
            numeric_tick_texts(&rebuilt),
            numeric_tick_texts(&first.scene),
            "{name}"
        );
        assert_numeric_ticks_fit(&Compilation {
            mir: decoded_mir,
            scene: rebuilt,
        });
    }
}

#[test]
fn ordinary_formatted_hir_and_mir_json_round_trips_rebuild_identical_scene() {
    let source = document(formatted_fixture());
    let baseline = compile(&source).unwrap();
    let decoded_hir: Document =
        serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
    let normalized = lower_to_mir(&decoded_hir).unwrap();
    let decoded_mir: VizMir =
        serde_json::from_slice(&serde_json::to_vec(&normalized).unwrap()).unwrap();
    assert_eq!(guide_policies(&decoded_mir), guide_policies(&baseline.mir));
    assert_eq!(build_scene(&decoded_mir).unwrap(), baseline.scene);
}

#[test]
fn fractional_hir_and_mir_json_round_trips_keep_ticks_inside_frames() {
    for name in [
        "scientific-magnitudes.viz.yaml",
        "measurement-precision.viz.yaml",
        "mixed-axis-formats.viz.yaml",
    ] {
        let source = example(name);
        for index in 1..=100 {
            let mut changed = source.clone();
            for view in &mut changed.views {
                let frame = match view {
                    View::Scatter(chart) => &mut chart.frame,
                    View::Line(chart) => &mut chart.frame,
                    View::Bar(chart) => &mut chart.frame,
                    _ => unreachable!(),
                };
                frame.x = index as f64 / 13.0;
                frame.y = index as f64 / 17.0;
                frame.width = 800.0 + index as f64 / 19.0;
                frame.height = 480.0 + index as f64 / 23.0;
            }
            let hir_json = serde_json::to_vec(&changed).unwrap();
            let changed: Document = serde_json::from_slice(&hir_json).unwrap();
            let mut result = compile(&changed).unwrap();
            let expected_ticks = numeric_tick_texts(&result.scene);
            let expected_policies = guide_policies(&result.mir);
            result.mir = serde_json::from_slice(&serde_json::to_vec(&result.mir).unwrap()).unwrap();
            assert_eq!(
                guide_policies(&result.mir),
                expected_policies,
                "{name}, case {index}"
            );
            result.scene = build_scene(&result.mir)
                .unwrap_or_else(|error| panic!("{name}, case {index}: {error}"));
            assert_eq!(
                numeric_tick_texts(&result.scene),
                expected_ticks,
                "{name}, case {index}"
            );
            assert_numeric_ticks_fit(&result);
        }
    }
}

fn rename_references(value: &mut Value, previous: &str, replacement: &str) {
    match value {
        Value::String(text) if text == previous => *text = replacement.to_owned(),
        Value::Array(values) => {
            for value in values {
                rename_references(value, previous, replacement);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                rename_references(value, previous, replacement);
            }
        }
        _ => {}
    }
}

#[test]
fn explicit_guide_bindings_survive_arbitrary_scale_names_and_order() {
    let original = compile(&document(formatted_fixture())).unwrap();
    let mut encoded = serde_json::to_value(&original.mir).unwrap();
    rename_references(
        &mut encoded,
        "format-test/x",
        "scales/unrelated-horizontal-name",
    );
    rename_references(
        &mut encoded,
        "format-test/y",
        "scales/unrelated-vertical-name",
    );
    encoded["views"][0]["scales"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let mir: VizMir = serde_json::from_value(encoded).unwrap();
    validate_mir(&mir).unwrap();
    let scene = build_scene(&mir).unwrap();
    // Renamed bindings also appear in mark provenance. The geometry, guides,
    // and all other scene fields must remain exactly unchanged.
    let mut expected = original.scene;
    fn rename_provenance(nodes: &mut [SceneNode]) {
        for node in nodes {
            let origin = match node {
                SceneNode::Group {
                    children, origin, ..
                } => {
                    rename_provenance(children);
                    origin
                }
                SceneNode::Rect { origin, .. }
                | SceneNode::Circle { origin, .. }
                | SceneNode::Line { origin, .. }
                | SceneNode::Path { origin, .. }
                | SceneNode::Text { origin, .. } => origin,
            };
            origin.explanation = origin
                .explanation
                .replace("format-test/x", "scales/unrelated-horizontal-name")
                .replace("format-test/y", "scales/unrelated-vertical-name");
        }
    }
    rename_provenance(&mut expected.nodes);
    assert_eq!(scene, expected);
    assert_numeric_ticks_fit(&Compilation { mir, scene });
}
