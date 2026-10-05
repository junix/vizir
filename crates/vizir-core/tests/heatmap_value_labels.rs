use serde_json::{Value, json};
use vizir_core::{
    Composition, Document, HeatmapValueLabels, NumberFormat, NumberNotation, View,
    compose_versioned, composition_schema, mir_schema, validate_document,
};

fn source() -> Value {
    json!({"version":"0.5","id":"values","width":800,"height":500,
    "datasets":{"d":{"key":"id","rows":[{"id":"a","x":"X","y":"Y","v":0}]}},
    "views":[{"kind":"chart.heatmap","id":"h","frame":{"x":0,"y":0,"width":800,"height":500},
    "dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"v"},"value_labels":{}}]})
}

#[test]
fn old_schema_definitions_and_complete_reference_closures_are_frozen() {
    let frozen: Value =
        serde_json::from_str(include_str!("fixtures/heatmap-frozen-definitions.json")).unwrap();
    for (kind, schema) in [("mir", mir_schema()), ("composition", composition_schema())] {
        for (name, definition) in frozen[kind].as_object().unwrap() {
            assert_eq!(&schema["$defs"][name], definition, "{kind}::{name}");
        }
    }
}

#[test]
fn labels_require_new_versions_in_json_yaml_and_typed_api() {
    let current: Document = serde_json::from_value(source()).unwrap();
    validate_document(&current).unwrap();
    for version in ["0.1", "0.2", "0.3", "0.4"] {
        let mut old = source();
        old["version"] = version.into();
        assert!(serde_json::from_value::<Document>(old.clone()).is_err());
        assert!(serde_yaml::from_str::<Document>(&serde_yaml::to_string(&old).unwrap()).is_err());
        let mut typed = current.clone();
        typed.version = version.into();
        assert!(validate_document(&typed).is_err());
    }
    let mut panel = source()["views"][0].clone();
    panel.as_object_mut().unwrap().remove("frame");
    let mut composition = json!({"schema":"vizir-composition/0.4","id":"grid","width":800,"height":500,"layout":{"kind":"grid","columns":1},"datasets":source()["datasets"],"panels":[panel]});
    let typed: Composition = serde_json::from_value(composition.clone()).unwrap();
    assert_eq!(compose_versioned(&typed).unwrap().version, "0.5");
    for version in ["0.1", "0.2", "0.3"] {
        composition["schema"] = format!("vizir-composition/{version}").into();
        assert!(serde_json::from_value::<Composition>(composition.clone()).is_err());
    }
}

#[test]
fn closed_optional_object_rejects_null_unknowns_and_duplicate_fields() {
    for labels in [
        Value::Null,
        json!(true),
        json!("auto"),
        json!({"show_labels":true}),
        json!({"color":null}),
        json!({"number_format":null}),
        json!({"number_format":{"notation":"fixed","precision":2.5}}),
    ] {
        let mut value = source();
        value["views"][0]["value_labels"] = labels;
        assert!(serde_json::from_value::<Document>(value).is_err());
    }
    let wire = serde_json::to_string(&source()).unwrap();
    assert!(
        serde_json::from_str::<Document>(&wire.replace(
            "\"value_labels\":{}",
            "\"value_labels\":{},\"value_labels\":{}"
        ))
        .is_err()
    );
    assert!(
        serde_json::from_str::<HeatmapValueLabels>(r##"{"color":"#000000","color":"#FFFFFF"}"##)
            .is_err()
    );
    let mut absent = source();
    absent["views"][0]
        .as_object_mut()
        .unwrap()
        .remove("value_labels");
    let roundtrip =
        serde_json::to_value(serde_json::from_value::<Document>(absent).unwrap()).unwrap();
    assert!(roundtrip["views"][0].get("value_labels").is_none());
    for precision in [0, 12] {
        let mut value = source();
        value["views"][0]["value_labels"]["number_format"] =
            json!({"notation":"fixed","precision":f64::from(precision)});
        validate_document(&serde_json::from_value::<Document>(value).unwrap()).unwrap();
    }
    let mut document: Document = serde_json::from_value(source()).unwrap();
    let View::Heatmap(chart) = &mut document.views[0] else {
        unreachable!()
    };
    chart.value_labels.as_mut().unwrap().number_format = Some(NumberFormat {
        notation: NumberNotation::Fixed,
        precision: 13,
    });
    assert!(validate_document(&document).is_err());
}

#[test]
fn opaque_colors_and_checked_resource_boundaries_are_exact() {
    for color in ["#000000", "#aBcDeF", "#112233FF", "#112233ff"] {
        assert!(vizir_core::heatmap_opaque_rgb(&vizir_core::Color::hex(color)).is_some());
    }
    for color in [
        "transparent",
        "#112233FE",
        "#fff",
        "red",
        "#ééé",
        "#€0000",
        "#zz0000",
    ] {
        assert!(vizir_core::heatmap_opaque_rgb(&vizir_core::Color::hex(color)).is_none());
        let mut value = source();
        value["views"][0]["value_labels"]["color"] = color.into();
        assert!(validate_document(&serde_json::from_value::<Document>(value).unwrap()).is_err());
    }
    // Source-level automatic contrast is palette-wide, even for an unused bin.
    let mut unused_alpha = source();
    unused_alpha["views"][0]["color"]["domain"] = json!([0, 10]);
    unused_alpha["views"][0]["color"]["palette"] = json!(["#000000", "#FFFFFF80"]);
    assert!(validate_document(&serde_json::from_value::<Document>(unused_alpha).unwrap()).is_err());
    let mut count = 0;
    vizir_core::reserve_heatmap_value_labels(&mut count, 4096).unwrap();
    assert!(vizir_core::reserve_heatmap_value_labels(&mut count, 1).is_err());
    let mut overflow = usize::MAX;
    assert!(vizir_core::reserve_heatmap_value_labels(&mut overflow, 1).is_err());
    let mut bytes = 0;
    for _ in 0..2048 {
        vizir_core::reserve_heatmap_value_label_bytes(&mut bytes, &"0".repeat(512)).unwrap();
    }
    assert_eq!(bytes, 1048576);
    assert!(vizir_core::reserve_heatmap_value_label_bytes(&mut bytes, "0").is_err());
    assert!(vizir_core::reserve_heatmap_value_label_bytes(&mut 0, &"0".repeat(513)).is_err());
}
