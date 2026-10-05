use serde_json::{Value, json};
use vizir_core::{
    Composition, Document, View, compose_versioned, composition_schema, mir_schema,
    validate_document,
};
fn source(kind: &str) -> Value {
    let mut view = json!({"kind":format!("chart.{kind}"),"id":"chart","frame":{"x":0,"y":0,"width":800,"height":480},"dataset":"d"});
    if kind == "bar" {
        view["category"] = json!({"field":"id"});
        view["value"] = json!({"field":"y","domain":[0,10]});
    } else {
        view["x"] = json!({"field":"x","domain":[-1,2]});
        view["y"] = json!({"field":"y","domain":[0,10]});
    }
    if kind == "area" {
        view["baseline"] = json!(0);
        view["order"] = json!("x-ascending");
    }
    json!({"version":"0.7","id":"domains","width":800,"height":480,"datasets":{"d":{"key":"id","rows":[{"id":"a","x":0,"y":2},{"id":"b","x":1,"y":8}]}},"views":[view]})
}
fn field(kind: &str) -> &str {
    if kind == "bar" { "value" } else { "y" }
}
#[test]
fn authored_domains_require_new_version_in_json_yaml_and_typed_api() {
    for kind in ["scatter", "line", "area", "bar"] {
        let value = source(kind);
        let d: Document = serde_json::from_value(value.clone()).unwrap();
        validate_document(&d).unwrap();
        for v in ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "future"] {
            let mut bad = value.clone();
            bad["version"] = v.into();
            assert!(serde_json::from_value::<Document>(bad.clone()).is_err());
            assert!(
                serde_yaml::from_str::<Document>(&serde_yaml::to_string(&bad).unwrap()).is_err()
            );
            let mut typed = d.clone();
            typed.version = v.into();
            assert!(validate_document(&typed).is_err());
        }
        let mut panel = value["views"][0].clone();
        panel.as_object_mut().unwrap().remove("frame");
        let mut c = json!({"schema":"vizir-composition/0.6","id":"grid","width":800,"height":480,"layout":{"kind":"grid","columns":1},"datasets":value["datasets"],"panels":[panel]});
        let typed: Composition = serde_json::from_value(c.clone()).unwrap();
        assert_eq!(compose_versioned(&typed).unwrap().version, "0.7");
        for v in ["0.1", "0.2", "0.3", "0.4", "0.5"] {
            c["schema"] = format!("vizir-composition/{v}").into();
            assert!(serde_json::from_value::<Composition>(c.clone()).is_err());
            assert!(
                serde_yaml::from_str::<Composition>(&serde_yaml::to_string(&c).unwrap()).is_err()
            );
        }
    }
}
#[test]
fn invalid_shape_order_span_and_non_numbers_reject_without_coercion() {
    for domain in [
        Value::Null,
        json!([]),
        json!([1]),
        json!([0, 1, 2]),
        json!("0,1"),
        json!(["0", "10"]),
        json!([true, 10]),
        json!([null, 10]),
        json!([0, {}]),
        json!([10, 0]),
        json!([2, 2]),
        json!([-0.0, 0.0]),
        json!([-1e308, 1e308]),
    ] {
        let mut v = source("scatter");
        v["views"][0]["y"]["domain"] = domain;
        assert!(
            serde_json::from_value::<Document>(v.clone()).is_err(),
            "{v}"
        );
        assert!(serde_yaml::from_str::<Document>(&serde_yaml::to_string(&v).unwrap()).is_err());
    }
    let mut d: Document = serde_json::from_value(source("scatter")).unwrap();
    for domain in [
        [f64::NAN, 10.],
        [0., f64::INFINITY],
        [0., 0.],
        [-1e308, 1e308],
        [10., 0.],
    ] {
        let View::Scatter(c) = &mut d.views[0] else {
            panic!()
        };
        c.y.domain = Some(domain);
        assert!(validate_document(&d).is_err());
    }
    let v = serde_json::to_string(&source("scatter")).unwrap();
    assert!(
        serde_json::from_str::<Document>(
            &v.replace("\"domain\":", "\"domain\":[0,10],\"domain\":")
        )
        .is_err()
    );
}
#[test]
fn membership_baselines_missing_values_and_nonapplicable_slots_are_strict() {
    for kind in ["scatter", "line", "area", "bar"] {
        for value in [json!(-1), json!(11), Value::Null, json!("5")] {
            let mut v = source(kind);
            v["datasets"]["d"]["rows"][0]["y"] = value;
            assert!(validate_document(&serde_json::from_value(v).unwrap()).is_err());
        }
        let mut v = source(kind);
        v["datasets"]["d"]["rows"][0]
            .as_object_mut()
            .unwrap()
            .remove("y");
        assert!(validate_document(&serde_json::from_value(v).unwrap()).is_err());
        for domain in [json!([2, 8]), json!([0, 8])] {
            let mut v = source(kind);
            v["views"][0][field(kind)]["domain"] = domain;
            let d: Document = serde_json::from_value(v).unwrap();
            let lower = serde_json::to_value(&d).unwrap()["views"][0][field(kind)]["domain"][0]
                .as_f64()
                .unwrap();
            assert_eq!(
                validate_document(&d).is_ok(),
                !(matches!(kind, "area" | "bar") && lower > 0.0)
            );
        }
    }
    let mut bar = source("bar");
    bar["views"][0]["category"]["domain"] = json!([0, 10]);
    assert!(serde_json::from_value::<Document>(bar).is_err());
    for stacking in ["stack", "stacked", "stacking"] {
        let mut area = source("area");
        area["views"][0][stacking] = true.into();
        assert!(serde_json::from_value::<Document>(area).is_err());
    }
}
#[test]
fn endpoints_retain_exact_float_bits_and_positive_subnormal_spans() {
    for domain in [
        [-0.0, 10.0],
        [0.0, f64::from_bits(1)],
        [-f64::from_bits(1), 0.0],
        [-1e308, 0.0],
        [1., f64::from_bits(1.0f64.to_bits() + 1)],
    ] {
        let mut v = source("scatter");
        v["views"][0]["y"]["domain"] = json!(domain);
        for (i, row) in v["datasets"]["d"]["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            row["y"] = json!(domain[i]);
        }
        let d: Document = serde_json::from_value(v).unwrap();
        validate_document(&d).unwrap();
        let View::Scatter(c) = &d.views[0] else {
            panic!()
        };
        assert_eq!(
            c.y.domain.unwrap().map(f64::to_bits),
            domain.map(f64::to_bits)
        );
    }
}
#[test]
fn complete_published_schema_closures_and_version_branches_are_frozen() {
    let frozen: Value =
        serde_json::from_str(include_str!("fixtures/numeric-frozen-definitions.json")).unwrap();
    for (kind, schema) in [("mir", mir_schema()), ("composition", composition_schema())] {
        for (name, definition) in frozen[kind]["$defs"].as_object().unwrap() {
            assert_eq!(&schema["$defs"][name], definition, "{kind}::{name}");
        }
        for (i, b) in frozen[kind]["oneOf"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&schema["oneOf"][i], b);
        }
    }
    assert!(
        composition_schema()["$defs"]["FieldEncoding"]["properties"]
            .get("domain")
            .is_none()
    );
    assert_eq!(
        composition_schema()["$defs"]["FieldEncodingV07"]["properties"]["domain"]["minItems"],
        2
    );
}
#[test]
fn nonzero_area_baseline_is_checked_against_exact_closed_bounds() {
    let mut v = source("area");
    v["views"][0]["y"]["domain"] = json!([2, 8]);
    for baseline in [2.0, 4.0, 8.0] {
        v["views"][0]["baseline"] = json!(baseline);
        validate_document(&serde_json::from_value(v.clone()).unwrap()).unwrap();
    }
    for baseline in [
        f64::from_bits(2.0f64.to_bits() - 1),
        f64::from_bits(8.0f64.to_bits() + 1),
    ] {
        v["views"][0]["baseline"] = json!(baseline);
        assert!(validate_document(&serde_json::from_value(v.clone()).unwrap()).is_err());
    }
}
