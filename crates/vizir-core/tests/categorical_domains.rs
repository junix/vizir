use serde_json::{Value, json};
use vizir_core::{
    Composition, Document, View, compose_versioned, composition_schema, mir_schema,
    validate_document,
};

fn source() -> Value {
    json!({"version":"0.6","id":"colors","width":800,"height":480,
        "datasets":{"d":{"key":"id","rows":[{"id":"a","x":1,"y":2,"group":"Beta"}]}},
        "views":[{"kind":"chart.scatter","id":"chart","frame":{"x":0,"y":0,"width":800,"height":480},
        "dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"group","domain":["Beta","Alpha"]}}]})
}

#[test]
fn domain_is_opt_in_in_json_yaml_and_typed_api() {
    let current: Document = serde_json::from_value(source()).unwrap();
    validate_document(&current).unwrap();
    assert_eq!(
        serde_json::to_value(&current).unwrap()["views"][0]["color"]["domain"],
        json!(["Beta", "Alpha"])
    );
    for version in ["0.1", "0.2", "0.3", "0.4", "0.5", "future"] {
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
    let mut composition = json!({"schema":"vizir-composition/0.5","id":"grid","width":800,"height":480,"layout":{"kind":"grid","columns":1},"datasets":source()["datasets"],"panels":[panel]});
    for yaml in [false, true] {
        let typed: Composition = if yaml {
            serde_yaml::from_str(&serde_yaml::to_string(&composition).unwrap()).unwrap()
        } else {
            serde_json::from_value(composition.clone()).unwrap()
        };
        assert_eq!(
            compose_versioned(&typed).unwrap(),
            Document {
                id: "grid".into(),
                ..current.clone()
            }
        );
    }
    for version in ["0.1", "0.2", "0.3", "0.4"] {
        composition["schema"] = format!("vizir-composition/{version}").into();
        assert!(serde_json::from_value::<Composition>(composition.clone()).is_err());
        assert!(
            serde_yaml::from_str::<Composition>(&serde_yaml::to_string(&composition).unwrap())
                .is_err()
        );
    }
}

#[test]
fn strict_domain_shapes_duplicates_and_bounds_reject_early() {
    for domain in [
        Value::Null,
        json!([]),
        json!(["Beta", "Beta"]),
        json!([1]),
        json!([true]),
        json!([null]),
        json!([{}]),
        json!([["Beta"]]),
        json!("Beta"),
        json!([""]),
        json!(["a\nb"]),
        json!(["a\u{2028}b"]),
        json!(["x".repeat(16385)]),
        json!((0..257).map(|i| i.to_string()).collect::<Vec<_>>()),
        json!(
            (0..65)
                .map(|i| format!("{i:02}{}", "x".repeat(16382)))
                .collect::<Vec<_>>()
        ),
    ] {
        let mut value = source();
        value["views"][0]["color"]["domain"] = domain;
        assert!(serde_json::from_value::<Document>(value.clone()).is_err());
        assert!(serde_yaml::from_str::<Document>(&serde_yaml::to_string(&value).unwrap()).is_err());
    }
    let input = serde_json::to_string(&source()).unwrap();
    assert!(
        serde_json::from_str::<Document>(
            &input.replace("\"domain\":", "\"domain\":[\"Beta\"],\"domain\":")
        )
        .is_err()
    );
    let mut typed: Document = serde_json::from_value(source()).unwrap();
    let View::Scatter(chart) = &mut typed.views[0] else {
        panic!()
    };
    chart.color.as_mut().unwrap().domain = Some(vec![]);
    assert!(validate_document(&typed).is_err());
}

#[test]
fn membership_uses_exact_existing_scalar_key_semantics() {
    for (observed, key) in [
        (json!(true), "true"),
        (json!(42), "42"),
        (json!(1.5), "1.5"),
        (json!(" Beta "), " Beta "),
        (json!("β"), "β"),
    ] {
        let mut value = source();
        value["datasets"]["d"]["rows"][0]["group"] = observed;
        value["views"][0]["color"]["domain"] = json!([key, "reserved"]);
        validate_document(&serde_json::from_value(value.clone()).unwrap()).unwrap();
        value["views"][0]["color"]["domain"] = json!(["other"]);
        assert!(
            validate_document(&serde_json::from_value(value).unwrap())
                .unwrap_err()
                .iter()
                .any(|d| d.code == "VIZ-COLOR-0002")
        );
    }
    for observed in [Value::Null, json!([]), json!({})] {
        let mut value = source();
        value["datasets"]["d"]["rows"][0]["group"] = observed;
        assert!(validate_document(&serde_json::from_value(value).unwrap()).is_err());
    }
    // Legacy keys, including empty strings, are unaffected when domain is absent.
    for key in ["", "a\nb"] {
        let mut value = source();
        value["version"] = "0.1".into();
        value["views"][0]["color"]
            .as_object_mut()
            .unwrap()
            .remove("domain");
        value["datasets"]["d"]["rows"][0]["group"] = key.into();
        let typed: Document = serde_json::from_value(value).unwrap();
        validate_document(&typed).unwrap();
        assert!(
            serde_json::to_value(typed).unwrap()["views"][0]["color"]
                .get("domain")
                .is_none()
        );
    }
}

#[test]
fn every_old_schema_definition_and_version_branch_is_frozen() {
    let frozen: Value =
        serde_json::from_str(include_str!("fixtures/categorical-frozen-definitions.json")).unwrap();
    for (kind, schema) in [("mir", mir_schema()), ("composition", composition_schema())] {
        for (name, definition) in frozen[kind]["$defs"].as_object().unwrap() {
            assert_eq!(&schema["$defs"][name], definition, "{kind}::{name}");
        }
        for (i, branch) in frozen[kind]["oneOf"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&schema["oneOf"][i], branch, "{kind} branch {i}");
        }
    }
    let schema = composition_schema();
    assert!(
        schema["$defs"]["ColorEncoding"]["properties"]
            .get("domain")
            .is_none()
    );
    assert_eq!(
        schema["$defs"]["ColorEncodingV06"]["properties"]["domain"]["uniqueItems"],
        true
    );
}

#[test]
fn declared_domain_bounds_are_inclusive_and_only_count_utf8_bytes() {
    for domain in [
        (0..256).map(|i| i.to_string()).collect::<Vec<_>>(),
        vec!["界".repeat(5461) + "a"],
        (0..64)
            .map(|i| format!("{i:02}{}", "x".repeat(16382)))
            .collect::<Vec<_>>(),
    ] {
        let mut value = source();
        value["datasets"]["d"]["rows"][0]["group"] = domain[0].clone().into();
        value["views"][0]["color"]["domain"] = json!(domain);
        validate_document(&serde_json::from_value(value).unwrap()).unwrap();
    }
}
