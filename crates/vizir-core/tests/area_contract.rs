use serde_json::{Value, json};
use vizir_core::{
    ChartMark, Composition, CompositionV1, CompositionVersion, Document, MirView, View, VizMir,
    compose, compose_versioned, composition_schema, mir_schema, validate_document,
    validate_document_capabilities, validate_mir, validate_mir_capabilities,
};

fn hir() -> Value {
    json!({
        "version": "0.3", "id": "area-document", "width": 640, "height": 400,
        "datasets": {"data": {"key": "id", "rows": [
            {"id": "a", "x": 1, "y": 2, "group": "one"},
            {"id": "b", "x": 2, "y": 3, "group": "one"}
        ]}},
        "views": [{"kind": "chart.area", "id": "area", "frame": {"x": 0, "y": 0, "width": 640, "height": 400},
            "dataset": "data", "x": {"field": "x"}, "y": {"field": "y"},
            "series": {"field": "group"}, "baseline": 0, "order": "x-ascending"}]
    })
}

fn mir() -> Value {
    json!({
        "version": "0.3", "source_hir_version": "0.3", "document_id": "area-document",
        "width": 640, "height": 400, "background": "transparent",
        "spaces": {"plot": {"id": "plot", "kind": "plot", "unit": "scene-unit", "transform_to_parent": {}}},
        "data": {"data": {"id": "data", "schema": {"key": "id", "fields": {
            "id": {"type": "string"}, "x": {"type": "float64"}, "y": {"type": "float64"}, "group": {"type": "string"}
        }}, "operator": {"kind": "inline", "rows": []}, "update_mode": "replace", "deterministic": true}},
        "expressions": {
            "key": {"result_type": {"type": "string"}, "expression": {"op": "field", "row": "row", "field": "id"}},
            "x": {"result_type": {"type": "float64"}, "expression": {"op": "field", "row": "row", "field": "x"}},
            "y": {"result_type": {"type": "float64"}, "expression": {"op": "field", "row": "row", "field": "y"}},
            "group": {"result_type": {"type": "string"}, "expression": {"op": "field", "row": "row", "field": "group"}}
        },
        "views": [{"dialect": "chart", "id": "area", "title": null,
            "frame": {"x": 0, "y": 0, "width": 640, "height": 400}, "space": "plot",
            "source": "data", "row_variable": "row", "key_expression": "key",
            "scales": [
                {"type": "linear", "id": "x", "domain": [1, 2], "range": [0, 640], "range_space": "plot", "zero": false},
                {"type": "linear", "id": "y", "domain": [0, 3], "range": [400, 0], "range_space": "plot", "zero": true},
                {"type": "ordinal-color", "id": "color", "domain": ["one"], "range": ["#112233"]}
            ],
            "guides": [
                {"id": "x-axis", "kind": "axis", "scale": "x", "label": "X", "orient": "bottom"},
                {"id": "y-axis", "kind": "axis", "scale": "y", "label": "Y", "orient": "left"},
                {"id": "legend", "kind": "legend", "scale": "color", "label": "Series", "orient": "right"}
            ],
            "mark": {"type": "area", "id": "area-mark", "x": {"scale": "x", "expression": "x"},
                "y": {"scale": "y", "expression": "y"}, "color": {"scale": "color", "expression": "group"},
                "group_expression": "group", "order_expression": "x", "baseline": 0, "series": []},
            "provenance": []}], "losses": []
    })
}

fn composition(version: &str) -> Value {
    let source = hir();
    let mut panel = source["views"][0].clone();
    panel.as_object_mut().unwrap().remove("frame");
    json!({"schema": version, "id": "composition", "width": 640, "height": 400,
        "datasets": source["datasets"], "layout": {"kind": "grid", "columns": 1}, "panels": [panel]})
}

#[test]
fn generic_json_yaml_and_typed_hir_enforce_area_version_without_legacy_tightening() {
    let current: Document = serde_json::from_value(hir()).unwrap();
    validate_document(&current).unwrap();
    for version in ["0.1", "0.2", "0.6"] {
        let mut source = hir();
        source["version"] = version.into();
        assert!(serde_json::from_value::<Document>(source.clone()).is_err());
        assert!(
            serde_yaml::from_str::<Document>(&serde_yaml::to_string(&source).unwrap()).is_err()
        );
        let mut typed = current.clone();
        typed.version = version.to_owned();
        assert!(validate_document_capabilities(&typed).is_err());
        assert!(validate_document(&typed).is_err());
    }
    // Legacy serde did not validate dimensions, IDs, or unknown versions.
    let legacy = json!({"version": "future", "id": "", "width": -1, "height": 0, "views": []});
    assert!(serde_json::from_value::<Document>(legacy).is_ok());
}

#[test]
fn area_wire_requires_baseline_and_closed_order_and_rejects_style_knobs() {
    for field in ["baseline", "order"] {
        let mut source = hir();
        source["views"][0].as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Document>(source).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("order", json!("input")),
        ("order", json!("x-descending")),
        ("show_points", json!(true)),
        ("line_width", json!(2)),
        ("fill_opacity", json!(0.5)),
        ("stack", json!(true)),
        ("curve", json!("step")),
    ] {
        let mut source = hir();
        source["views"][0][field] = value;
        assert!(
            serde_json::from_value::<Document>(source).is_err(),
            "{field}"
        );
    }
}

#[test]
fn area_hir_checks_baseline_combined_span_scalar_series_and_axis_format() {
    let mut document: Document = serde_json::from_value(hir()).unwrap();
    let View::Area(chart) = &mut document.views[0] else {
        unreachable!()
    };
    chart.baseline = f64::INFINITY;
    assert!(validate_document(&document).is_err());
    let mut source = hir();
    source["views"][0]["baseline"] = json!(-1e308);
    source["datasets"]["data"]["rows"][0]["y"] = json!(1e308);
    let document: Document = serde_json::from_value(source).unwrap();
    assert!(
        validate_document(&document)
            .unwrap_err()
            .iter()
            .any(|error| error.code == "VIZ-AREA-0001")
    );
    for value in [Value::Null, json!([]), json!({})] {
        let mut source = hir();
        source["datasets"]["data"]["rows"][0]["group"] = value;
        assert!(validate_document(&serde_json::from_value(source).unwrap()).is_err());
    }
    let mut source = hir();
    source["views"][0]["x"]["axis"] =
        json!({"number_format": {"notation": "fixed", "precision": 2}});
    validate_document(&serde_json::from_value(source).unwrap()).unwrap();
}

#[test]
fn generic_json_yaml_and_typed_mir_enforce_area_pairs_but_preserve_legacy_source_values() {
    let current: VizMir = serde_json::from_value(mir()).unwrap();
    validate_mir(&current).unwrap();
    for (version, source_version) in [
        ("0.1", "0.1"),
        ("0.2", "0.2"),
        ("0.3", "0.2"),
        ("0.2", "0.3"),
    ] {
        let mut source = mir();
        source["version"] = version.into();
        source["source_hir_version"] = source_version.into();
        assert!(serde_json::from_value::<VizMir>(source.clone()).is_err());
        assert!(serde_yaml::from_str::<VizMir>(&serde_yaml::to_string(&source).unwrap()).is_err());
        let mut typed = current.clone();
        typed.version = version.into();
        typed.source_hir_version = source_version.into();
        assert!(validate_mir_capabilities(&typed).is_err());
        assert!(validate_mir(&typed).is_err());
    }
    for version in ["0.1", "0.2"] {
        let mut legacy = mir();
        legacy["views"] = json!([]);
        legacy["version"] = version.into();
        legacy["source_hir_version"] = "0.3".into();
        let typed: VizMir = serde_json::from_value(legacy).unwrap();
        validate_mir(&typed).unwrap();
    }
}

#[test]
fn direct_mir_area_requires_numeric_linear_axes_same_x_order_and_explicit_guides() {
    for (pointer, replacement) in [
        ("/views/0/mark/order_expression", json!("y")),
        ("/views/0/mark/x/expression", json!("group")),
        ("/views/0/mark/x/scale", json!("color")),
        ("/views/0/guides", json!([])),
        ("/views/0/guides/2/orient", json!("left")),
        ("/views/0/guides/0/orient", json!("right")),
        ("/views/0/mark/color/scale", json!("x")),
        ("/views/0/scales/1/domain", json!([-1e308, 1e308])),
    ] {
        let mut source = mir();
        *source.pointer_mut(pointer).unwrap() = replacement;
        let typed: VizMir = serde_json::from_value(source).unwrap();
        assert!(validate_mir(&typed).is_err(), "{pointer}");
    }
    let mut duplicate = mir();
    let guide = duplicate["views"][0]["guides"][0].clone();
    duplicate["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .push(guide);
    assert!(validate_mir(&serde_json::from_value(duplicate).unwrap()).is_err());
    let mut typed: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(chart) = &mut typed.views[0] else {
        unreachable!()
    };
    let ChartMark::Area { baseline, .. } = &mut chart.mark else {
        unreachable!()
    };
    *baseline = f64::NAN;
    assert!(validate_mir(&typed).is_err());
}

#[test]
fn composition_dispatch_preserves_v1_adapter_and_area_is_v2_only() {
    let source = composition("vizir-composition/0.2");
    let current: Composition = serde_json::from_value(source.clone()).unwrap();
    let document = compose_versioned(&current).unwrap();
    assert_eq!(document.version, "0.3");
    assert!(matches!(document.views[0], View::Area(_)));
    assert!(serde_json::from_value::<CompositionV1>(source).is_err());
    let old = composition("vizir-composition/0.1");
    assert!(serde_json::from_value::<Composition>(old.clone()).is_err());
    assert!(serde_json::from_value::<CompositionV1>(old).is_err());
    let mut typed = current.clone();
    typed.schema = CompositionVersion::V1;
    assert!(compose_versioned(&typed).is_err());

    let mut old = composition("vizir-composition/0.1");
    old["panels"] = json!([{"kind": "geometry.scene", "id": "geometry", "children": []}]);
    let legacy: CompositionV1 = serde_json::from_value(old.clone()).unwrap();
    let dispatcher: Composition = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(
        compose(&legacy).unwrap(),
        compose_versioned(&dispatcher).unwrap()
    );
    assert_eq!(compose(&legacy).unwrap().version, "0.2");
    let mut forged = legacy;
    forged.panels = current.panels;
    assert!(compose(&forged).is_err());
    old["schema"] = "vizir-composition/0.2".into();
    let current: Composition = serde_json::from_value(old).unwrap();
    assert_eq!(compose_versioned(&current).unwrap().version, "0.3");
}

#[test]
fn schema_version_paths_isolate_area_and_retain_legacy_contracts() {
    let schema = mir_schema();
    assert_eq!(
        schema["oneOf"][0]["properties"]["version"]["enum"],
        json!(["0.1", "0.2"])
    );
    assert_eq!(
        schema["$defs"]["VizMirV03"]["properties"]["version"]["const"],
        "0.3"
    );
    assert_eq!(
        schema["$defs"]["VizMirV03"]["properties"]["source_hir_version"]["const"],
        "0.3"
    );
    assert_eq!(
        schema["$defs"]["ChartMark"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        schema["$defs"]["ChartMarkV03"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(!schema["$defs"]["MirView"].to_string().contains("V03"));
    assert!(
        schema["$defs"]["MirViewV03"]
            .to_string()
            .contains("ChartMarkV03")
    );
    let schema = composition_schema();
    assert_eq!(
        schema["$defs"]["CompositionSchema"]["enum"],
        json!(["vizir-composition/0.1"])
    );
    assert_eq!(
        schema["$defs"]["Panel"]["oneOf"].as_array().unwrap().len(),
        5
    );
    assert_eq!(
        schema["$defs"]["PanelV02"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        schema["$defs"]["CompositionV2"]["properties"]["schema"]["const"],
        "vizir-composition/0.2"
    );
}
