use serde_json::{Value, json};
use vizir_core::{
    ChartMark, Composition, CompositionV1, CompositionVersion, Document, MirScale, MirView, VizMir,
    canonical_quantize_thresholds, compose_versioned, composition_schema, mir_schema,
    quantize_color_index, validate_document, validate_document_capabilities,
    validate_heatmap_category, validate_mir, validate_mir_capabilities,
};

fn hir() -> Value {
    json!({"version":"0.4","id":"heatmap","width":640,"height":400,
        "datasets":{"data":{"key":"id","rows":[
            {"id":"a","x":"A","y":"North","value":0},
            {"id":"b","x":"B","y":"South","value":10}]}},
        "views":[{"kind":"chart.heatmap","id":"map","frame":{"x":0,"y":0,"width":640,"height":400},
            "dataset":"data","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"value"}}]})
}

fn mir() -> Value {
    json!({"version":"0.4","source_hir_version":"0.4","document_id":"heatmap",
        "width":640,"height":400,"background":"transparent",
        "spaces":{"plot":{"id":"plot","kind":"document","unit":"scene-unit","transform_to_parent":{}}},
        "data":{"data":{"id":"data","schema":{"key":"id","fields":{
            "id":{"type":"string"},"x":{"type":"string"},"y":{"type":"string"},"value":{"type":"float64"}}},
            "operator":{"kind":"inline","rows":[{"id":"a","x":"A","y":"North","value":0},{"id":"b","x":"B","y":"South","value":10}]},
            "update_mode":"replace","deterministic":true}},
        "expressions":{
            "key":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"id"}},
            "x":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"x"}},
            "y":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"y"}},
            "value":{"result_type":{"type":"float64"},"expression":{"op":"field","row":"row","field":"value"}}},
        "views":[{"dialect":"chart","id":"map","title":null,"frame":{"x":0,"y":0,"width":640,"height":400},
            "space":"plot","source":"data","row_variable":"row","key_expression":"key",
            "scales":[
                {"type":"band","id":"x","domain":["A","B"],"range":[0,600],"range_space":"plot","padding":0},
                {"type":"band","id":"y","domain":["North","South"],"range":[0,340],"range_space":"plot","padding":0},
                {"type":"quantize-color","id":"color","domain":[0,10],"thresholds":[2,4,6,8],"range":["#000000","#222222","#555555","#AAAAAA","#FFFFFF"]}],
            "guides":[
                {"id":"x-axis","kind":"axis","scale":"x","label":"X","orient":"bottom"},
                {"id":"y-axis","kind":"axis","scale":"y","label":"Y","orient":"left"},
                {"id":"legend","kind":"legend","scale":"color","label":"Value","orient":"right","number_format":{"notation":"fixed","precision":2}}],
            "mark":{"type":"heatmap","id":"cells","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},
                "color":{"scale":"color","expression":"value"},"instances":[
                    {"key":"a","x":"A","y":"North","value":0},{"key":"b","x":"B","y":"South","value":10}]},
            "provenance":[]}],"losses":[]})
}

fn composition() -> Value {
    let source = hir();
    let mut panel = source["views"][0].clone();
    panel.as_object_mut().unwrap().remove("frame");
    json!({"schema":"vizir-composition/0.3","id":"grid","width":640,"height":400,
        "datasets":source["datasets"],"layout":{"kind":"grid","columns":1},"panels":[panel]})
}

fn valid_hir(source: Value) -> bool {
    validate_document(&serde_json::from_value::<Document>(source).unwrap()).is_ok()
}

fn valid_mir(source: Value) -> bool {
    validate_mir(&serde_json::from_value::<VizMir>(source).unwrap()).is_ok()
}

#[test]
fn canonical_quantize_arithmetic_edges_ties_constants_and_exact_bits() {
    for bins in 2..=9 {
        let domain = [-5., 15.];
        let thresholds = canonical_quantize_thresholds(domain, bins).unwrap();
        assert_eq!(thresholds.len(), bins - 1);
        assert_eq!(
            quantize_color_index(-5., domain, &thresholds, bins).unwrap(),
            0
        );
        assert_eq!(
            quantize_color_index(15., domain, &thresholds, bins).unwrap(),
            bins - 1
        );
        for (i, &threshold) in thresholds.iter().enumerate() {
            assert_eq!(
                quantize_color_index(threshold, domain, &thresholds, bins).unwrap(),
                i + 1
            );
            let fraction = ((i + 1) as f64) / (bins as f64);
            let offset = 20. * fraction;
            let expected = -5. + offset;
            assert_eq!(
                threshold.to_bits(),
                if expected == 0. {
                    0.0f64.to_bits()
                } else {
                    expected.to_bits()
                }
            );
        }
        assert!(quantize_color_index(-6., domain, &thresholds, bins).is_err());
        assert!(quantize_color_index(16., domain, &thresholds, bins).is_err());
        assert!(quantize_color_index(f64::NAN, domain, &thresholds, bins).is_err());
        for value in [0., -0., 42., -42.] {
            assert!(
                canonical_quantize_thresholds([value, value], bins)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                quantize_color_index(value, [value, value], &[], bins).unwrap(),
                (bins - 1) / 2
            );
            assert!(quantize_color_index(value + 1., [value, value], &[], bins).is_err());
        }
    }
    for bins in [0, 1, 10, usize::MAX] {
        assert!(canonical_quantize_thresholds([0., 1.], bins).is_err());
    }
    for domain in [
        [1., 0.],
        [-f64::MAX, f64::MAX],
        [0., f64::INFINITY],
        [f64::NAN, 0.],
        [0., f64::from_bits(1)],
        [1., f64::from_bits(1.0f64.to_bits() + 1)],
    ] {
        assert!(
            canonical_quantize_thresholds(domain, 2).is_err(),
            "{domain:?}"
        );
    }
    let tiny = f64::from_bits(1);
    assert_eq!(
        canonical_quantize_thresholds([tiny, 3. * tiny], 2).unwrap(),
        vec![2. * tiny]
    );
    assert_eq!(
        canonical_quantize_thresholds([-1., 1.], 2).unwrap()[0].to_bits(),
        0.0f64.to_bits()
    );
    assert!(quantize_color_index(0., [-1., 1.], &[-0.], 2).is_err());
    assert!(quantize_color_index(0., [-1., 1.], &[f64::from_bits(1)], 2).is_err());
    assert!(quantize_color_index(0., [0., 0.], &[0.], 2).is_err());
}

#[test]
fn new_options_omit_cleanly_and_reject_explicit_null_and_unknown_fields() {
    let typed: Document = serde_json::from_value(hir()).unwrap();
    let wire = serde_json::to_value(&typed).unwrap();
    for encoding in ["x", "y", "color"] {
        assert!(wire["views"][0][encoding].get("label").is_none());
        assert!(wire["views"][0][encoding].get("domain").is_none());
    }
    assert!(wire["views"][0]["color"].get("palette").is_none());
    assert!(wire["views"][0]["color"].get("number_format").is_none());
    for (encoding, fields) in [
        ("x", vec!["label", "domain"]),
        ("y", vec!["label", "domain"]),
        ("color", vec!["label", "domain", "palette", "number_format"]),
    ] {
        for field in fields {
            let mut source = hir();
            source["views"][0][encoding][field] = Value::Null;
            assert!(
                serde_json::from_value::<Document>(source.clone()).is_err(),
                "{encoding}.{field}"
            );
            assert!(
                serde_yaml::from_str::<Document>(&serde_yaml::to_string(&source).unwrap()).is_err()
            );
        }
    }
    for (encoding, field, value) in [
        ("x", "axis", json!({})),
        ("y", "sort", json!("ascending")),
        ("color", "clamp", json!(true)),
    ] {
        let mut source = hir();
        source["views"][0][encoding][field] = value;
        assert!(serde_json::from_value::<Document>(source).is_err());
    }
    let mut source = hir();
    source["views"][0]["interpolation"] = "linear".into();
    assert!(serde_json::from_value::<Document>(source).is_err());
}

#[test]
fn hir_json_yaml_typed_capabilities_require_04_without_legacy_tightening() {
    let current: Document = serde_json::from_value(hir()).unwrap();
    validate_document(&current).unwrap();
    for version in ["0.1", "0.2", "0.3", "0.9", "future"] {
        let mut source = hir();
        source["version"] = version.into();
        assert!(serde_json::from_value::<Document>(source.clone()).is_err());
        assert!(
            serde_yaml::from_str::<Document>(&serde_yaml::to_string(&source).unwrap()).is_err()
        );
        let mut typed = current.clone();
        typed.version = version.into();
        assert!(validate_document_capabilities(&typed).is_err());
    }
    assert!(
        serde_json::from_value::<Document>(
            json!({"version":"future","id":"","width":-1,"height":0,"views":[]})
        )
        .is_ok()
    );
}

#[test]
fn source_categories_are_exact_strings_and_domains_cover_data() {
    for value in [
        Value::Null,
        json!(4),
        json!(false),
        json!([]),
        json!({}),
        json!(""),
        json!("a\n"),
        json!("a\u{0085}"),
        json!("a\u{2028}"),
        json!("a\u{2029}"),
    ] {
        let mut source = hir();
        source["datasets"]["data"]["rows"][0]["x"] = value.clone();
        assert!(!valid_hir(source), "{value}");
    }
    for value in ["a\0", "\u{007f}", "\u{009f}"] {
        assert!(validate_heatmap_category(value).is_err());
    }
    for value in [" ", " A ", "é", "e\u{0301}", "東京", "🦀"] {
        validate_heatmap_category(value).unwrap();
    }
    for values in [
        json!([]),
        json!(["A", "A", "B"]),
        json!(["A"]),
        json!(["A", "B", "\n"]),
    ] {
        let mut source = hir();
        source["views"][0]["x"]["domain"] = values;
        assert!(!valid_hir(source));
    }
    let mut source = hir();
    source["views"][0]["x"]["domain"] = json!(["B", "extra", "A"]);
    source["views"][0]["y"]["domain"] = json!(["South", "North", "unused"]);
    assert!(valid_hir(source));
    let mut source = hir();
    source["datasets"]["data"]["rows"][1]["x"] = "A".into();
    source["datasets"]["data"]["rows"][1]["y"] = "North".into();
    assert!(!valid_hir(source));
    // Delimiter-concatenation would collide, but real tuples do not.
    let mut source = hir();
    source["datasets"]["data"]["rows"][0]["x"] = "a|b".into();
    source["datasets"]["data"]["rows"][0]["y"] = "c".into();
    source["datasets"]["data"]["rows"][1]["x"] = "a".into();
    source["datasets"]["data"]["rows"][1]["y"] = "b|c".into();
    assert!(valid_hir(source));
}

#[test]
fn source_numeric_domains_palettes_and_formats_are_bounded() {
    for value in [Value::Null, json!("2"), json!(true), json!([])] {
        let mut source = hir();
        source["datasets"]["data"]["rows"][0]["value"] = value;
        assert!(!valid_hir(source));
    }
    for domain in [
        json!([10, 0]),
        json!([1, 10]),
        json!([-1e308, 1e308]),
        json!([0, 0]),
    ] {
        let mut source = hir();
        source["views"][0]["color"]["domain"] = domain;
        assert!(!valid_hir(source));
    }
    for palette in [
        json!([]),
        json!(["#FFFFFF"]),
        json!(["red", "blue"]),
        json!(vec!["#FFFFFF"; 10]),
    ] {
        let mut source = hir();
        source["views"][0]["color"]["palette"] = palette;
        assert!(!valid_hir(source));
    }
    for bins in 2..=9 {
        let mut source = hir();
        source["views"][0]["color"]["palette"] = json!(vec!["#FFFFFF"; bins]);
        source["views"][0]["color"]["number_format"] =
            json!({"notation":"scientific","precision":12});
        assert!(valid_hir(source));
    }
    let mut source = hir();
    source["views"][0]["color"]["number_format"] = json!({"notation":"fixed","precision":13});
    assert!(!valid_hir(source));
}

#[test]
fn category_row_and_label_resource_boundaries_fail_before_general_work() {
    let mut source = hir();
    source["views"][0]["x"]["domain"] = json!(
        (0..254)
            .map(|i| format!("extra{i}"))
            .chain(["A".into(), "B".into()])
            .collect::<Vec<_>>()
    );
    assert!(valid_hir(source.clone()));
    source["views"][0]["x"]["domain"]
        .as_array_mut()
        .unwrap()
        .push(json!("257th"));
    assert!(!valid_hir(source));
    let label = "x".repeat(vizir_core::MAX_HEATMAP_LABEL_BYTES);
    validate_heatmap_category(&label).unwrap();
    assert!(validate_heatmap_category(&(label.clone() + "x")).is_err());
    let mut source = hir();
    source["views"][0]["color"]["label"] = (label + "x").into();
    assert!(!valid_hir(source));
    let mut source = hir();
    source["datasets"]["data"]["rows"] = json!(vec![
        source["datasets"]["data"]["rows"][0].clone();
        vizir_core::MAX_HEATMAP_CELLS + 1
    ]);
    let diagnostics =
        validate_document(&serde_json::from_value::<Document>(source).unwrap()).unwrap_err();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("16384"));
    // Exact 1 MiB combined domain label budget, then one extra domain entry.
    let values: Vec<_> = (0..32)
        .map(|i| format!("{i:02}{}", "x".repeat(16382)))
        .collect();
    let mut source = hir();
    source["datasets"]["data"]["rows"][0]["x"] = json!(values[0]);
    source["datasets"]["data"]["rows"][1]["x"] = json!(values[1]);
    source["datasets"]["data"]["rows"][0]["y"] = json!(values[0]);
    source["datasets"]["data"]["rows"][1]["y"] = json!(values[1]);
    source["views"][0]["x"]["domain"] = json!(values);
    source["views"][0]["y"]["domain"] = json!(values);
    assert!(valid_hir(source.clone()));
    source["views"][0]["y"]["domain"]
        .as_array_mut()
        .unwrap()
        .push(json!("extra"));
    assert!(!valid_hir(source));
}

#[test]
fn mir_capability_gates_all_quantize_occurrences_and_preserves_old_source_strings() {
    let current: VizMir = serde_json::from_value(mir()).unwrap();
    validate_mir(&current).unwrap();
    for (version, source_version) in [
        ("0.1", "0.1"),
        ("0.2", "0.2"),
        ("0.3", "0.3"),
        ("0.4", "0.3"),
        ("0.3", "0.4"),
        ("future", "future"),
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
        source["views"][0]["mark"] = json!({"type":"symbol","id":"legacy","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},"color":null,"size":7,"instances":[]});
        assert!(
            serde_json::from_value::<VizMir>(source).is_err(),
            "unused new scale must be gated"
        );
    }
    for version in ["0.1", "0.2"] {
        let mut source = mir();
        source["views"] = json!([]);
        source["version"] = version.into();
        source["source_hir_version"] = "arbitrary-future-source".into();
        let typed: VizMir = serde_json::from_value(source).unwrap();
        validate_mir(&typed).unwrap();
    }
}

#[test]
fn direct_mir_contract_rejects_malformed_bindings_guides_scales_and_cells() {
    for (pointer, value) in [
        ("/views/0/scales/0/padding", json!(0.1)),
        ("/views/0/scales/0/range", json!([600, 0])),
        ("/views/0/scales/0/domain", json!(["A", "A"])),
        ("/views/0/scales/0/domain", json!([])),
        ("/views/0/scales/2/thresholds", json!([2, 4, 6, 7])),
        ("/views/0/scales/2/thresholds", json!([2, 4, 6])),
        ("/views/0/scales/2/domain", json!([-1e308, 1e308])),
        ("/views/0/scales/2/range", json!(["#FFFFFF"])),
        ("/views/0/scales/2/range/0", json!("red")),
        ("/views/0/scales/2/id", json!("x")),
        ("/views/0/mark/x/scale", json!("color")),
        ("/views/0/mark/y/scale", json!("x")),
        ("/views/0/mark/x/expression", json!("value")),
        ("/views/0/mark/color/expression", json!("x")),
        ("/views/0/mark/color/scale", json!("y")),
        ("/views/0/guides", json!([])),
        ("/views/0/guides/2/orient", json!("left")),
        ("/views/0/guides/0/orient", json!("right")),
        ("/views/0/guides/1/id", json!("x-axis")),
        ("/data/data/operator/rows", json!([])),
    ] {
        let mut source = mir();
        *source.pointer_mut(pointer).unwrap() = value;
        assert!(!valid_mir(source), "{pointer}");
    }
    let mut source = mir();
    source["views"][0]["guides"][0]["number_format"] = json!({"notation":"fixed","precision":2});
    assert!(!valid_mir(source));
    // Bounded cached fields are assertions. Refresh must be able to repair
    // duplicate tuples/keys, stale domains, and typed non-finite cache values.
    let mut source = mir();
    source["views"][0]["mark"]["instances"][1] = source["views"][0]["mark"]["instances"][0].clone();
    source["views"][0]["mark"]["instances"][0]["x"] = "stale".into();
    source["views"][0]["mark"]["instances"][0]["y"] = "\n".into();
    source["views"][0]["mark"]["instances"][0]["value"] = json!(-1);
    assert!(valid_mir(source));
    let mut typed: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(chart) = &mut typed.views[0] else {
        unreachable!()
    };
    let ChartMark::Heatmap { instances, .. } = &mut chart.mark else {
        unreachable!()
    };
    instances[0].value = f64::NAN;
    assert!(validate_mir(&typed).is_ok());
}

#[test]
fn unbound_quantize_scales_are_validated_and_forbidden_on_other_marks() {
    let mut source = mir();
    source["views"][0]["mark"] = json!({"type":"symbol","id":"legacy","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},"color":null,"size":7,"instances":[]});
    assert!(!valid_mir(source.clone()));
    source["views"][0]["source"] = "missing".into();
    source["views"][0]["scales"][2]["thresholds"] = json!([123]);
    let diagnostics = validate_mir(&serde_json::from_value::<VizMir>(source).unwrap()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|error| error.source.as_deref() == Some("views[0].scales[2]"))
    );
}

#[test]
fn direct_mir_constants_require_empty_thresholds_and_exact_threshold_bits() {
    let mut source = mir();
    source["views"][0]["scales"][2]["domain"] = json!([3, 3]);
    source["views"][0]["scales"][2]["thresholds"] = json!([]);
    for cell in source["views"][0]["mark"]["instances"]
        .as_array_mut()
        .unwrap()
    {
        cell["value"] = json!(3);
    }
    assert!(valid_mir(source.clone()));
    source["views"][0]["scales"][2]["thresholds"] = json!([3]);
    assert!(!valid_mir(source));
    let mut typed: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(chart) = &mut typed.views[0] else {
        unreachable!()
    };
    let MirScale::QuantizeColor { thresholds, .. } = &mut chart.scales[2] else {
        unreachable!()
    };
    thresholds[0] = f64::from_bits(thresholds[0].to_bits() + 1);
    assert!(validate_mir(&typed).is_err());
}

#[test]
fn composition_v3_is_frame_free_and_maps_to_hir04() {
    let current: Composition = serde_json::from_value(composition()).unwrap();
    assert_eq!(current.schema, CompositionVersion::V3);
    assert_eq!(compose_versioned(&current).unwrap().version, "0.4");
    assert!(serde_json::from_value::<CompositionV1>(composition()).is_err());
    for version in [
        "vizir-composition/0.1",
        "vizir-composition/0.2",
        "vizir-composition/0.8",
    ] {
        let mut source = composition();
        source["schema"] = version.into();
        assert!(serde_json::from_value::<Composition>(source).is_err());
    }
    let mut source = composition();
    source["panels"][0]["frame"] = json!({"x":0,"y":0,"width":1,"height":1});
    assert!(serde_json::from_value::<Composition>(source).is_err());
    let mut typed = current;
    typed.schema = CompositionVersion::V2;
    assert!(compose_versioned(&typed).is_err());
}

#[test]
fn versioned_schema_paths_keep_old_marks_scales_and_axis_only_formats_exact() {
    let schema = mir_schema();
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 7);
    for name in [
        "ChartMark",
        "ChartMarkV03",
        "MirScale",
        "MirGuide",
        "MirView",
        "MirViewV03",
        "VizMirV03",
    ] {
        assert!(
            !schema["$defs"][name].to_string().contains("heatmap"),
            "{name}"
        );
        assert!(!schema["$defs"][name].to_string().contains("V04"), "{name}");
        assert!(
            !schema["$defs"][name].to_string().contains("quantize"),
            "{name}"
        );
    }
    assert_eq!(
        schema["$defs"]["MirGuide"]["allOf"],
        json!([{"if":{"required":["number_format"]},"then":{"properties":{"kind":{"const":"axis"}}}}])
    );
    assert!(schema["$defs"]["MirGuideV04"].get("allOf").is_none());
    assert_eq!(
        schema["$defs"]["VizMirV04"]["properties"]["version"]["const"],
        "0.4"
    );
    assert_eq!(
        schema["$defs"]["VizMirV04"]["properties"]["source_hir_version"]["const"],
        "0.4"
    );
    assert_eq!(
        schema["$defs"]["ChartMarkV04"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        schema["$defs"]["MirScale"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        schema["$defs"]["MirScaleV04"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let schema = composition_schema();
    assert_eq!(
        schema["$defs"]["CompositionVersion"]["enum"],
        json!(["vizir-composition/0.1", "vizir-composition/0.2"])
    );
    assert_eq!(
        schema["$defs"]["PanelV03"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        schema["$defs"]["CompositionV3"]["properties"]["schema"]["const"],
        "vizir-composition/0.3"
    );
    for name in ["CategoryEncoding", "QuantizeColorEncoding"] {
        assert_eq!(schema["$defs"][name]["additionalProperties"], false);
        assert_eq!(
            schema["$defs"][name]["properties"]["label"]["type"],
            "string"
        );
        assert_eq!(schema["$defs"][name]["required"], json!(["field"]));
    }
}

#[test]
fn mir_source_and_cache_call_budgets_are_independent_and_inclusive() {
    let mut typed: VizMir = serde_json::from_value(mir()).unwrap();
    let mut small = typed.data["data"].clone();
    small.id = "small".into();
    let vizir_core::MirDataOperator::Inline { rows } = &mut small.operator;
    rows.truncate(1);
    typed.data.insert("small".into(), small);
    let vizir_core::MirDataOperator::Inline { rows } =
        &mut typed.data.get_mut("data").unwrap().operator;
    rows.resize(vizir_core::MAX_HEATMAP_CELLS - 1, rows[0].clone());
    let original = typed.views[0].clone();
    typed.views.clear();
    for index in 0..8 {
        let mut view = original.clone();
        let MirView::Chart(chart) = &mut view else {
            unreachable!()
        };
        chart.id = format!("map{index}");
        let ChartMark::Heatmap { instances, .. } = &mut chart.mark else {
            unreachable!()
        };
        if index < 4 {
            instances.clear();
        } else {
            chart.source = "small".into();
            instances.resize(vizir_core::MAX_HEATMAP_CELLS, instances[0].clone());
        }
        typed.views.push(view);
    }
    validate_mir(&typed).unwrap();
    let mut excess_rows = typed.clone();
    let vizir_core::MirDataOperator::Inline { rows } =
        &mut excess_rows.data.get_mut("data").unwrap().operator;
    rows.push(rows[0].clone());
    assert!(
        validate_mir(&excess_rows)
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("65536"))
    );
    let MirView::Chart(chart) = &mut typed.views[0] else {
        unreachable!()
    };
    let ChartMark::Heatmap { instances, .. } = &mut chart.mark else {
        unreachable!()
    };
    instances.push(vizir_core::MirHeatmapCell {
        key: "stale".into(),
        x: "".into(),
        y: "".into(),
        value: 0.,
    });
    assert!(
        validate_mir(&typed)
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("65536"))
    );
}

#[test]
fn duplicate_heatmap_wire_fields_reject_before_preprocessing() {
    let source = serde_json::to_string(&hir()).unwrap();
    let source = source.replace(
        "\"x\":{\"field\":\"x\"}",
        "\"x\":{\"field\":\"x\",\"field\":\"x\"}",
    );
    assert!(serde_json::from_str::<Document>(&source).is_err());
    let source = serde_json::to_string(&mir()).unwrap();
    let source = source.replace(
        "\"thresholds\":[2,4,6,8]",
        "\"thresholds\":[2,4,6,8],\"thresholds\":[2,4,6,8]",
    );
    assert!(serde_json::from_str::<VizMir>(&source).is_err());
}

#[test]
fn heatmap_requires_one_identity_document_space_with_opaque_ids() {
    let mut renamed = mir();
    let id = "opaque/λ : canvas";
    let mut space = renamed["spaces"]
        .as_object_mut()
        .unwrap()
        .remove("plot")
        .unwrap();
    space["id"] = id.into();
    renamed["spaces"][id] = space;
    renamed["views"][0]["space"] = id.into();
    for index in 0..2 {
        renamed["views"][0]["scales"][index]["range_space"] = id.into();
    }
    assert!(valid_mir(renamed));

    for (pointer, replacement) in [
        (
            "/spaces/plot/transform_to_parent",
            json!({"translate":{"x":100,"y":0}}),
        ),
        (
            "/spaces/plot/transform_to_parent",
            json!({"translate":{"x":0,"y":1}}),
        ),
        (
            "/spaces/plot/transform_to_parent",
            json!({"scale":{"x":2,"y":1}}),
        ),
        (
            "/spaces/plot/transform_to_parent",
            json!({"scale":{"x":1,"y":0.5}}),
        ),
        (
            "/spaces/plot/transform_to_parent",
            json!({"rotate_degrees":90}),
        ),
        ("/spaces/plot/unit", json!("pixel")),
        ("/spaces/plot/unit", json!("point")),
        ("/spaces/plot/unit", json!("normalized-view-width")),
        ("/spaces/plot/kind", json!("plot")),
        ("/spaces/plot/kind", json!("view-local")),
        ("/views/0/space", json!("other")),
        ("/views/0/scales/0/range_space", json!("other")),
        ("/views/0/scales/1/range_space", json!("other")),
        ("/views/0/scales/0/range_space", json!("missing")),
    ] {
        let mut source = mir();
        source["spaces"]["other"] =
            json!({"id":"other","kind":"document","unit":"scene-unit","transform_to_parent":{}});
        *source.pointer_mut(pointer).unwrap() = replacement;
        // Generic decoding remains a narrow capability check; full validation
        // rejects coordinate semantics that this heatmap slice cannot execute.
        let typed: VizMir = serde_json::from_value(source).unwrap();
        validate_mir_capabilities(&typed).unwrap();
        let errors = validate_mir(&typed).unwrap_err();
        assert!(
            errors.iter().any(|e| e.code == "VIZ-HEATMAP-0001"),
            "{pointer}: {errors:?}"
        );
    }
    let mut parented = mir();
    parented["spaces"]["other"] =
        json!({"id":"other","kind":"document","unit":"scene-unit","transform_to_parent":{}});
    parented["spaces"]["plot"]["parent"] = "other".into();
    assert!(!valid_mir(parented));
    let mut typed: VizMir = serde_json::from_value(mir()).unwrap();
    typed
        .spaces
        .get_mut("plot")
        .unwrap()
        .transform_to_parent
        .translate
        .x = f64::NAN;
    assert!(validate_mir(&typed).is_err());
}

#[test]
fn heatmap_space_guard_does_not_tighten_other_chart_marks() {
    let mut source = mir();
    source["views"][0]["mark"] = json!({"type":"symbol","id":"legacy","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},"color":null,"size":7,"instances":[]});
    source["views"][0]["scales"].as_array_mut().unwrap().pop();
    source["views"][0]["guides"].as_array_mut().unwrap().pop();
    source["spaces"]["plot"]["kind"] = "plot".into();
    source["spaces"]["plot"]["unit"] = "pixel".into();
    source["spaces"]["plot"]["transform_to_parent"] = json!({"translate":{"x":100,"y":0}});
    assert!(valid_mir(source));
}
