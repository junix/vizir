use serde_json::{Value, json};
use vizir_core::{CompositionV1, Frame, PanelLayout, compose, composition_schema};

fn source(count: usize, columns: u32) -> CompositionV1 {
    serde_json::from_value(json!({
        "schema": "vizir-composition/0.1", "id": "grid", "width": 1000, "height": 800,
        "layout": {"kind": "grid", "columns": columns, "gap": 20, "padding": 30},
        "panels": (0..count).map(|index| json!({
            "kind": "geometry.scene", "id": format!("panel-{index}"), "children": []
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

fn frames(source: &CompositionV1) -> Vec<Frame> {
    compose(source)
        .unwrap()
        .views
        .iter()
        .map(|view| *view.frame())
        .collect()
}

#[test]
fn exact_row_major_frames_preserve_partial_final_row() {
    let expected = vec![
        Frame {
            x: 30.0,
            y: 30.0,
            width: 460.0,
            height: 360.0,
        },
        Frame {
            x: 510.0,
            y: 30.0,
            width: 460.0,
            height: 360.0,
        },
        Frame {
            x: 30.0,
            y: 410.0,
            width: 460.0,
            height: 360.0,
        },
        Frame {
            x: 510.0,
            y: 410.0,
            width: 460.0,
            height: 360.0,
        },
    ];
    assert_eq!(frames(&source(4, 2)), expected);
    assert_eq!(frames(&source(3, 2)), expected[..3]);
    assert_eq!(
        frames(&source(1, 1)),
        vec![Frame {
            x: 30.0,
            y: 30.0,
            width: 940.0,
            height: 740.0
        }]
    );
}

#[test]
fn one_column_and_one_row_supply_stacks_without_new_layout_semantics() {
    assert_eq!(
        frames(&source(2, 1)),
        vec![
            Frame {
                x: 30.0,
                y: 30.0,
                width: 940.0,
                height: 360.0
            },
            Frame {
                x: 30.0,
                y: 410.0,
                width: 940.0,
                height: 360.0
            },
        ]
    );
    assert_eq!(
        frames(&source(2, 2)),
        vec![
            Frame {
                x: 30.0,
                y: 30.0,
                width: 460.0,
                height: 740.0
            },
            Frame {
                x: 510.0,
                y: 30.0,
                width: 460.0,
                height: 740.0
            },
        ]
    );
}

#[test]
fn zero_gap_and_padding_can_fill_canvas() {
    let mut source = source(4, 2);
    source.layout = PanelLayout::Grid {
        columns: 2,
        gap: 0.0,
        padding: 0.0,
    };
    let frames = frames(&source);
    assert_eq!(
        frames[0],
        Frame {
            x: 0.0,
            y: 0.0,
            width: 500.0,
            height: 400.0
        }
    );
    assert_eq!(
        frames[3],
        Frame {
            x: 500.0,
            y: 400.0,
            width: 500.0,
            height: 400.0
        }
    );
}

#[test]
fn fractional_dimensions_keep_cells_within_padding_and_gap() {
    for index in 1..=300 {
        let mut source = source(7, 3);
        source.width = 300.0 + f64::from(index) / 7.0;
        source.height = 420.0 + f64::from(index) / 11.0;
        let gap = 1.25;
        let padding = 2.125;
        source.layout = PanelLayout::Grid {
            columns: 3,
            gap,
            padding,
        };
        let cells = frames(&source);
        for frame in &cells {
            assert!(frame.x >= padding && frame.y >= padding);
            assert!(frame.x + frame.width <= source.width - padding);
            assert!(frame.y + frame.height <= source.height - padding);
            assert!(frame.width > 0.0 && frame.height > 0.0);
        }
        assert!((cells[1].x - (cells[0].x + cells[0].width) - gap).abs() < 1e-12);
        assert!((cells[3].y - (cells[0].y + cells[0].height) - gap).abs() < 1e-12);
    }
}

#[test]
fn invalid_counts_are_diagnosed_before_arithmetic() {
    for (count, columns, code) in [
        (0, 0, "0002"),
        (2, 0, "0003"),
        (2, 3, "0003"),
        (1, u32::MAX, "0003"),
    ] {
        let error = compose(&source(count, columns)).unwrap_err().to_string();
        assert!(error.contains(&format!("VIZ-COMPOSE-{code}")), "{error}");
    }
}

#[test]
fn nonfinite_or_negative_layout_values_are_never_materialized() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        for field in ["width", "height", "gap", "padding"] {
            let mut source = source(2, 2);
            match field {
                "width" => source.width = value,
                "height" => source.height = value,
                "gap" => {
                    source.layout = PanelLayout::Grid {
                        columns: 2,
                        gap: value,
                        padding: 0.0,
                    }
                }
                _ => {
                    source.layout = PanelLayout::Grid {
                        columns: 2,
                        gap: 0.0,
                        padding: value,
                    }
                }
            }
            let error = compose(&source).unwrap_err().to_string();
            assert!(
                error.contains("VIZ-COMPOSE-0001") && error.contains(field),
                "{error}"
            );
        }
    }
    for value in [0.0, -0.0] {
        let mut source = source(1, 1);
        source.width = value;
        assert!(compose(&source).is_err());
    }
}

#[test]
fn exhausted_space_overflow_underflow_and_lost_gaps_are_errors() {
    let mut cases = Vec::new();
    let mut exhausted = source(2, 2);
    exhausted.layout = PanelLayout::Grid {
        columns: 2,
        gap: 940.0,
        padding: 30.0,
    };
    cases.push(exhausted);
    let mut overflow = source(3, 3);
    overflow.layout = PanelLayout::Grid {
        columns: 3,
        gap: f64::MAX,
        padding: 0.0,
    };
    cases.push(overflow);
    let mut padding = source(1, 1);
    padding.layout = PanelLayout::Grid {
        columns: 1,
        gap: 0.0,
        padding: 500.0,
    };
    cases.push(padding);
    let mut underflow = source(2, 2);
    underflow.width = f64::from_bits(1);
    underflow.layout = PanelLayout::Grid {
        columns: 2,
        gap: 0.0,
        padding: 0.0,
    };
    cases.push(underflow);
    let mut lost_gap = source(2, 2);
    lost_gap.width = 1e100;
    lost_gap.layout = PanelLayout::Grid {
        columns: 2,
        gap: 1.0,
        padding: 0.0,
    };
    cases.push(lost_gap);
    let mut lost_padding = source(1, 1);
    lost_padding.width = 1e100;
    lost_padding.layout = PanelLayout::Grid {
        columns: 1,
        gap: 0.0,
        padding: 1.0,
    };
    cases.push(lost_padding);
    for source in cases {
        let error = compose(&source).unwrap_err().to_string();
        assert!(error.contains("VIZ-COMPOSE-0004"), "{error}");
    }
}

#[test]
fn integer_columns_spelling_matches_generated_schema() {
    let template = serde_json::to_value(source(2, 2)).unwrap();
    for spelling in ["2", "2.0", "2e0"] {
        let mut source = template.clone();
        source["layout"]["columns"] = serde_json::from_str(spelling).unwrap();
        let parsed: CompositionV1 = serde_json::from_value(source).unwrap();
        assert_eq!(frames(&parsed).len(), 2);
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["layout"]["columns"],
            2
        );
    }
    for invalid in [
        json!(1.5),
        json!(-1),
        json!(4294967296_u64),
        json!("2"),
        json!(true),
        Value::Null,
    ] {
        let mut source = template.clone();
        source["layout"]["columns"] = invalid;
        assert!(serde_json::from_value::<CompositionV1>(source).is_err());
    }
}

#[test]
fn duplicate_wire_fields_are_rejected_without_value_preprocessing() {
    let source = r#"{"schema":"vizir-composition/0.1","id":"a","width":10,"height":10,"layout":{"kind":"grid","columns":1,"columns":2},"panels":[{"kind":"geometry.scene","id":"p","children":[]}]}"#;
    assert!(serde_json::from_str::<CompositionV1>(source).is_err());
}

#[test]
fn checked_in_schema_tracks_the_closed_versioned_composition_contract() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/composition.schema.json");
    let checked_in: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let generated = composition_schema();
    assert_eq!(checked_in, generated);
    assert_eq!(
        generated["$defs"]["CompositionV1"]["additionalProperties"],
        false
    );
    assert_eq!(
        generated["$defs"]["CompositionSchema"]["enum"],
        json!(["vizir-composition/0.1"])
    );
    assert_eq!(
        generated["$defs"]["CompositionV1"]["properties"]["panels"]["minItems"],
        1
    );
}
