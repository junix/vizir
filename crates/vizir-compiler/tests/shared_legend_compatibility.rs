use serde_json::Value;
use vizir_compiler::{compiled_mir_schema, themed_mir_schema};

#[test]
fn complete_pre08_envelope_schema_closures_remain_unchanged() {
    let frozen: Value =
        serde_json::from_str(include_str!("fixtures/shared-legend-frozen-envelopes.json")).unwrap();
    for (kind, actual) in [
        ("compiled", compiled_mir_schema()),
        ("themed", themed_mir_schema()),
    ] {
        let old = &frozen[kind];
        for (name, definition) in old["$defs"].as_object().unwrap() {
            if name == "VizMir" {
                for (index, branch) in definition["oneOf"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        &actual["$defs"][name]["oneOf"][index], branch,
                        "{kind}::{name}::{index}"
                    );
                }
                assert_eq!(
                    actual["$defs"][name]["oneOf"].as_array().unwrap().len(),
                    definition["oneOf"].as_array().unwrap().len() + 1
                );
            } else {
                assert_eq!(&actual["$defs"][name], definition, "{kind}::{name}");
            }
        }
        for (key, value) in old.as_object().unwrap() {
            if key != "$defs" {
                assert_eq!(&actual[key], value, "{kind}::{key}");
            }
        }
    }
}
