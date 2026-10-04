use std::fmt::{Debug, Display};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use vizir_core::{Origin, Rect, ResolvedStyle, Scene2D, SceneNode, ScenePatch, scene_patch_schema};

const VARIANTS: [&str; 6] = ["group", "rect", "circle", "line", "path", "text"];
const UNKNOWN: &str = "unexpected_wire_field";

fn bounds() -> Value {
    json!({"x": -2.0, "y": -3.0, "width": 4.0, "height": 5.0})
}

fn style() -> Value {
    json!({"fill": "#12345678", "stroke": "transparent", "stroke_width": 0.0, "opacity": 1.0})
}

fn origin() -> Value {
    json!({
        "hir_node": "source / 数据",
        "mir_node": "resolved & <reference>",
        "data_key": " row / α\n<&> ",
        "data_lineage": ["", "same", "same", "opaque\n值"],
        "generated_by": "test",
        "explanation": ""
    })
}

fn node(kind: &str) -> Value {
    let mut value = json!({
        "type": kind,
        "id": format!("{kind}-id"),
        "bounds": bounds(),
        "origin": origin()
    });
    let fields = match kind {
        "group" => json!({
            "transform": {"translate": {"x": 0.0, "y": 0.0}, "rotate_degrees": 0.0,
                          "scale": {"x": 1.0, "y": 1.0}},
            "opacity": 1.0,
            "children": []
        }),
        "rect" => json!({"radius": 0.0, "style": style()}),
        "circle" => json!({"center": {"x": 0.0, "y": 0.0}, "radius": 2.0, "style": style()}),
        "line" => json!({"from": {"x": 0.0, "y": 0.0}, "to": {"x": 1.0, "y": 1.0},
                         "style": style(), "marker_end": false}),
        "path" => json!({"commands": [{"op": "move", "to": {"x": 0.0, "y": 0.0}},
                                     {"op": "line", "to": {"x": 1.0, "y": 1.0}}],
                         "style": style(), "marker_end": true}),
        "text" => json!({"position": {"x": 0.0, "y": 0.0}, "text": "opaque <文字>\n",
                         "font_size": 12.0, "anchor": "middle", "color": "#123456",
                         "weight": "regular"}),
        _ => unreachable!(),
    };
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value
}

fn group(children: Vec<Value>) -> Value {
    let mut value = node("group");
    value["children"] = children.into();
    value
}

fn scene(nodes: Vec<Value>) -> Value {
    json!({"document_id": "document", "width": 100.0, "height": 80.0,
           "background": "transparent", "nodes": nodes, "losses": []})
}

fn patch(operation: &str, node: Value) -> Value {
    let operation = match operation {
        "insert-node" => {
            json!({"op": operation, "parent": {"kind": "root"}, "index": 0, "node": node})
        }
        "replace-node" => json!({"op": operation, "id": "existing", "node": node}),
        _ => unreachable!(),
    };
    json!({"protocol_version": "0.1", "document_id": "document", "transaction_id": "test/wire",
           "base_revision": 3, "target_revision": 4, "operations": [operation]})
}

fn with_unknown(mut value: Value) -> Value {
    value[UNKNOWN] = json!({"must_not_disappear": true});
    value
}

fn assert_error<T, E: Display>(result: Result<T, E>, expected: &str, context: &str) {
    match result {
        Ok(_) => panic!("{context}: accepted invalid wire input"),
        Err(error) => assert!(
            error.to_string().contains(expected),
            "{context}: expected {expected:?}, got {error}"
        ),
    }
}

fn assert_unknown<T: DeserializeOwned>(value: Value, context: &str) {
    let json = serde_json::to_string(&value).unwrap();
    let yaml = serde_yaml::to_string(&value).unwrap();
    assert_error(serde_json::from_str::<T>(&json), UNKNOWN, context);
    assert_error(serde_json::from_value::<T>(value.clone()), UNKNOWN, context);
    assert_error(serde_yaml::from_str::<T>(&yaml), UNKNOWN, context);
    assert_error(
        serde_yaml::from_value::<T>(serde_yaml::to_value(value).unwrap()),
        UNKNOWN,
        context,
    );
}

fn assert_round_trip<T>(value: Value)
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let json = serde_json::to_string(&value).unwrap();
    let decoded: T = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
    assert_eq!(serde_json::from_value::<T>(value.clone()).unwrap(), decoded);
    let yaml = serde_yaml::to_string(&decoded).unwrap();
    assert_eq!(serde_yaml::from_str::<T>(&yaml).unwrap(), decoded);
    assert_eq!(
        serde_yaml::from_value::<T>(serde_yaml::to_value(value).unwrap()).unwrap(),
        decoded
    );
}

// Construct the repeated key in raw JSON. Never decode it through Value, which
// would collapse repeated keys before the actual wire decoder sees them.
fn duplicate(value: &Value, field: &str, second: Value) -> String {
    assert!(
        value.get(field).is_some(),
        "missing duplicate target {field}"
    );
    let json = serde_json::to_string(value).unwrap();
    format!("{{\"{field}\":{second},{}", &json[1..])
}

fn assert_duplicate<T: DeserializeOwned>(json: &str, field: &str) {
    assert_error(
        serde_json::from_str::<T>(json),
        &format!("duplicate field `{field}`"),
        json,
    );
}

fn insert_raw_node(template: Value, raw_node: &str) -> String {
    let json = serde_json::to_string(&template).unwrap();
    assert!(json.contains("\"__raw_node__\""));
    json.replace("\"__raw_node__\"", raw_node)
}

#[test]
fn scene_envelope_rejects_unknown_json_and_yaml_fields() {
    assert_unknown::<Scene2D>(with_unknown(scene(vec![])), "Scene2D");
}

#[test]
fn bounds_reject_unknown_json_and_yaml_fields() {
    assert_unknown::<Rect>(with_unknown(bounds()), "Rect");
}

#[test]
fn resolved_style_rejects_unknown_json_and_yaml_fields() {
    assert_unknown::<ResolvedStyle>(with_unknown(style()), "ResolvedStyle");
}

#[test]
fn origin_rejects_unknown_json_and_yaml_fields() {
    assert_unknown::<Origin>(with_unknown(origin()), "Origin");
}

#[test]
fn every_scene_variant_rejects_unknown_nested_fields() {
    for kind in VARIANTS {
        let fixture = node(kind);
        // The enum itself was already strict; nested structs must be equally strict.
        assert_unknown::<SceneNode>(with_unknown(fixture.clone()), kind);
        for field in ["bounds", "origin", "style"] {
            if fixture.get(field).is_none() {
                continue;
            }
            let mut invalid = fixture.clone();
            invalid[field] = with_unknown(invalid[field].clone());
            assert_unknown::<SceneNode>(invalid.clone(), &format!("{kind}.{field}"));
            assert_unknown::<Scene2D>(
                scene(vec![group(vec![group(vec![invalid])])]),
                &format!("nested scene {kind}.{field}"),
            );
        }
    }
}

#[test]
fn insert_and_replace_reject_unknown_fields_in_recursive_node_payloads() {
    for operation in ["insert-node", "replace-node"] {
        for kind in VARIANTS {
            let fixture = node(kind);
            for field in ["bounds", "origin", "style"] {
                if fixture.get(field).is_none() {
                    continue;
                }
                let mut invalid = fixture.clone();
                invalid[field] = with_unknown(invalid[field].clone());
                for payload in [invalid.clone(), group(vec![group(vec![invalid])])] {
                    assert_unknown::<ScenePatch>(
                        patch(operation, payload),
                        &format!("{operation} {kind}.{field}"),
                    );
                }
            }
        }
    }
}

#[test]
fn unknown_keys_cannot_be_silently_removed_by_a_wire_round_trip() {
    let mut invalid = scene(vec![group(vec![node("rect")])]);
    invalid["nodes"][0]["children"][0]["origin"][UNKNOWN] = json!("preserve or reject");
    let decoded = serde_json::from_value::<Scene2D>(invalid.clone());
    if let Ok(decoded) = decoded {
        panic!(
            "wire round trip accepted and discarded an unknown key: before {invalid}, after {}",
            serde_json::to_value(decoded).unwrap()
        );
    }
}

#[test]
fn recognized_struct_fields_reject_raw_json_duplicates() {
    let fixture = scene(vec![]);
    for (field, value) in fixture.as_object().unwrap() {
        assert_duplicate::<Scene2D>(&duplicate(&fixture, field, value.clone()), field);
    }
    let fixture = bounds();
    for (field, value) in fixture.as_object().unwrap() {
        assert_duplicate::<Rect>(&duplicate(&fixture, field, value.clone()), field);
    }
    let fixture = style();
    for (field, value) in fixture.as_object().unwrap() {
        assert_duplicate::<ResolvedStyle>(&duplicate(&fixture, field, value.clone()), field);
    }
    let fixture = origin();
    for (field, value) in fixture.as_object().unwrap() {
        assert_duplicate::<Origin>(&duplicate(&fixture, field, value.clone()), field);
    }
}

#[test]
fn optional_origin_fields_reject_duplicates_even_when_null_or_empty() {
    for first in [Value::Null, json!(""), json!("opaque")] {
        for second in [Value::Null, json!(""), json!("different")] {
            let mut fixture = origin();
            fixture["data_key"] = first.clone();
            assert_duplicate::<Origin>(&duplicate(&fixture, "data_key", second), "data_key");
        }
    }
    for first in [json!([]), json!(["row"])] {
        for second in [json!([]), json!(["other"])] {
            let mut fixture = origin();
            fixture["data_lineage"] = first.clone();
            assert_duplicate::<Origin>(
                &duplicate(&fixture, "data_lineage", second),
                "data_lineage",
            );
        }
    }
}

#[test]
fn raw_duplicate_nested_fields_are_rejected_through_scenes_and_patches() {
    for (object, field, second) in [
        ("bounds", "x", json!(99.0)),
        ("style", "opacity", json!(0.5)),
        ("origin", "hir_node", json!("different")),
        ("origin", "data_key", Value::Null),
        ("origin", "data_lineage", json!([])),
    ] {
        for kind in VARIANTS {
            let mut fixture = node(kind);
            if fixture.get(object).is_none() {
                continue;
            }
            if field == "data_key" {
                fixture[object][field] = Value::Null;
            }
            let raw_object = duplicate(&fixture[object], field, second.clone());
            let original = serde_json::to_string(&fixture[object]).unwrap();
            let raw_node =
                serde_json::to_string(&fixture)
                    .unwrap()
                    .replacen(&original, &raw_object, 1);
            assert_duplicate::<SceneNode>(&raw_node, field);
            let nested = insert_raw_node(group(vec![json!("__raw_node__")]), &raw_node);
            assert_duplicate::<Scene2D>(
                &insert_raw_node(scene(vec![json!("__raw_node__")]), &nested),
                field,
            );
            for operation in ["insert-node", "replace-node"] {
                assert_duplicate::<ScenePatch>(
                    &insert_raw_node(patch(operation, json!("__raw_node__")), &nested),
                    field,
                );
            }
        }
    }
}

#[test]
fn valid_struct_scene_and_patch_fixtures_keep_the_same_wire_values() {
    assert_round_trip::<Rect>(bounds());
    assert_round_trip::<ResolvedStyle>(style());
    assert_round_trip::<Origin>(origin());
    for kind in VARIANTS {
        assert_round_trip::<SceneNode>(node(kind));
    }
    let nested = group(vec![group(VARIANTS.into_iter().map(node).collect())]);
    assert_round_trip::<Scene2D>(scene(vec![nested.clone()]));
    for operation in ["insert-node", "replace-node"] {
        assert_round_trip::<ScenePatch>(patch(operation, nested.clone()));
    }
}

#[test]
fn omitted_optional_origin_fields_keep_defaults_and_stay_omitted() {
    let mut fixture = origin();
    fixture.as_object_mut().unwrap().remove("data_key");
    fixture.as_object_mut().unwrap().remove("data_lineage");
    assert_round_trip::<Origin>(fixture.clone());
    let decoded: Origin = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(decoded.data_key, None);
    assert!(decoded.data_lineage.is_empty());
    let mut explicit = fixture.clone();
    explicit["data_key"] = Value::Null;
    explicit["data_lineage"] = json!([]);
    let decoded: Origin = serde_yaml::from_str(&serde_yaml::to_string(&explicit).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), fixture);
    let mut nested = node("rect");
    nested["origin"] = fixture;
    assert_round_trip::<Scene2D>(scene(vec![group(vec![nested.clone()])]));
    for operation in ["insert-node", "replace-node"] {
        assert_round_trip::<ScenePatch>(patch(operation, nested.clone()));
    }
}

#[test]
fn decoding_preserves_empty_strings_and_opaque_data_without_semantic_validation() {
    let fixture = json!({"hir_node": "", "mir_node": "", "data_key": "",
                         "data_lineage": ["", "row", "row", " <数据>\n "],
                         "generated_by": "", "explanation": ""});
    assert_round_trip::<Origin>(fixture.clone());
    let mut nested = node("text");
    nested["id"] = json!("");
    nested["text"] = json!("");
    nested["origin"] = fixture;
    let mut value = scene(vec![nested.clone()]);
    value["document_id"] = json!("");
    assert_round_trip::<Scene2D>(value);
    for operation in ["insert-node", "replace-node"] {
        assert_round_trip::<ScenePatch>(patch(operation, nested.clone()));
    }
}

#[test]
fn scene_patch_schema_forbids_unknown_nested_struct_properties() {
    let generated = scene_patch_schema();
    let checked_in: Value =
        serde_json::from_str(include_str!("../../../schemas/scene-patch.schema.json")).unwrap();
    for schema in [&generated, &checked_in] {
        for name in ["Origin", "Rect", "ResolvedStyle"] {
            assert_eq!(
                schema["$defs"][name]["additionalProperties"],
                json!(false),
                "{name} must reject unknown properties"
            );
        }
        assert!(schema["$defs"].get("Scene2D").is_none());
    }
    assert_eq!(generated, checked_in);
}
