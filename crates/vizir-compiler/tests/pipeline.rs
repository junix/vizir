use std::collections::BTreeMap;
use std::path::PathBuf;

use vizir_compiler::compile;
use vizir_core::{
    Color, Document, FieldEncoding, Frame, MirScale, MirView, Revision, ScatterChart, Scene2D,
    SceneNode, SceneOp, ValueType, View, VizMir, apply_scene_patch, capability_schema, diff_scene,
    find_scene_node, mir_schema, parse_document, scene_patch_schema,
};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn chart_normalization_is_deterministic_and_explicit() {
    let path = workspace().join("examples/chart/service-health.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let first = compile(&document).expect("example compiles");
    let second = compile(&document).expect("example recompiles");

    assert_eq!(
        serde_json::to_vec(&first.mir).unwrap(),
        serde_json::to_vec(&second.mir).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&first.scene).unwrap(),
        serde_json::to_vec(&second.scene).unwrap()
    );

    let MirView::Chart(scatter) = &first.mir.views[0] else {
        panic!("first view should be a chart")
    };
    assert!(
        scatter
            .scales
            .iter()
            .any(|scale| matches!(scale, MirScale::Linear { id, .. } if id == "latency-risk/x"))
    );
    assert_eq!(scatter.guides.len(), 3);
    assert_eq!(scatter.source, "data/services");
    assert_eq!(scatter.row_variable, "row/latency-risk");
    assert_eq!(
        first.mir.expressions["expr/latency-risk/x"].result_type,
        ValueType::Int64
    );
    assert_eq!(scatter.scales[0].range_space(), Some("space/document"));

    let point = find_scene_node(&first.scene.nodes, "latency-risk/point/gateway")
        .expect("stable datum scene node exists");
    assert_eq!(point.origin().data_key.as_deref(), Some("gateway"));
    assert_eq!(point.origin().hir_node, "latency-risk");
}

#[test]
fn checked_in_mir_schema_matches_rust_contract() {
    let path = workspace().join("schemas/viz-mir.schema.json");
    let checked_in: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("schema should be checked in"))
            .expect("schema should be valid JSON");
    assert_eq!(
        checked_in["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(checked_in, mir_schema());
}

#[test]
fn canonical_mir_round_trips_and_rejects_unknown_core_fields() {
    let path = workspace().join("examples/chart/sales-regions.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let mir = compile(&document).expect("example compiles").mir;
    let encoded = serde_json::to_value(&mir).unwrap();
    let decoded: VizMir = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, mir);

    let mut unknown = encoded.clone();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("backend_object".to_owned(), serde_json::json!({}));
    assert!(serde_json::from_value::<VizMir>(unknown).is_err());

    let mut unknown_variant = encoded;
    unknown_variant["views"][0]
        .as_object_mut()
        .unwrap()
        .insert("dom_node".to_owned(), serde_json::json!("forbidden"));
    assert!(serde_json::from_value::<VizMir>(unknown_variant).is_err());
}

#[test]
fn checked_in_protocol_schemas_match_rust_contracts() {
    for (name, generated) in [
        ("scene-patch.schema.json", scene_patch_schema()),
        ("capability.schema.json", capability_schema()),
    ] {
        let path = workspace().join("schemas").join(name);
        let checked_in: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("schema should be checked in"))
                .expect("schema should be valid JSON");
        assert_eq!(checked_in, generated, "schema drifted: {name}");
    }
}

#[test]
fn incremental_scene_patch_matches_full_recompute() {
    let path = workspace().join("examples/chart/service-health.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let previous = compile(&document).expect("baseline compiles").scene;

    let mut changed = document;
    changed.datasets.get_mut("services").unwrap().rows[0]
        .insert("error_rate".to_owned(), serde_json::json!(0.88));
    let recomputed = compile(&changed).expect("changed input compiles").scene;
    let patch = diff_scene(
        &previous,
        &recomputed,
        Revision(41),
        Revision(42),
        "transaction/service-health-update",
    )
    .expect("scene diff should succeed");
    let (incremental, revision) =
        apply_scene_patch(&previous, Revision(41), &patch).expect("scene patch should apply");

    assert_eq!(revision, Revision(42));
    assert_eq!(incremental, recomputed);

    assert_eq!(patch.protocol_version, "0.1");
    assert_eq!(patch.document_id, "service-health-dashboard");
    assert_eq!(patch.transaction_id, "transaction/service-health-update");
    assert_eq!(patch.base_revision, Revision(41));
    assert_eq!(patch.target_revision, Revision(42));
    assert_eq!(
        patch.operations.len(),
        1,
        "a non-extremal datum change must not rescale guides or sibling marks"
    );
    let SceneOp::ReplaceNode { id, node } = &patch.operations[0] else {
        panic!("changed datum should lower to a single replace-node operation")
    };
    assert_eq!(id, "latency-risk/point/gateway");
    assert_eq!(node.id(), "latency-risk/point/gateway");
    assert_eq!(node.origin().data_key.as_deref(), Some("gateway"));
}

#[test]
fn layered_layout_is_resolved_before_backend_emission() {
    let path = workspace().join("examples/diagram/dialect-lowering.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let compilation = compile(&document).expect("example compiles");

    let shape = find_scene_node(
        &compilation.scene.nodes,
        "lowering-topology/node/normalize/shape",
    )
    .expect("resolved diagram node exists");
    let SceneNode::Rect { bounds, .. } = shape else {
        panic!("diagram node should lower to a concrete rect")
    };
    assert!(bounds.x.is_finite() && bounds.y.is_finite());
    assert!(shape.origin().explanation.contains("Kahn"));
}

#[test]
fn geometry_path_remains_typed_until_scene_construction() {
    let path = workspace().join("examples/geometry/compiler-pipeline.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let compilation = compile(&document).expect("example compiles");
    let path = find_scene_node(&compilation.scene.nodes, "compiler-poster/hir-to-mir")
        .expect("typed path lowers to scene");
    let SceneNode::Path { commands, .. } = path else {
        panic!("geometry path should remain a typed path")
    };
    assert_eq!(commands.len(), 2);
}

#[test]
fn resolved_scene_round_trips_and_rejects_unknown_node_fields() {
    let path = workspace().join("examples/chart/sales-regions.viz.yaml");
    let document = parse_document(path).expect("example parses");
    let scene = compile(&document).expect("example compiles").scene;
    let encoded = serde_json::to_value(&scene).unwrap();
    let decoded: Scene2D = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, scene);

    let mut unknown = encoded.clone();
    unknown["nodes"][0]
        .as_object_mut()
        .unwrap()
        .insert("backend_node".to_owned(), serde_json::json!("forbidden"));
    assert!(serde_json::from_value::<Scene2D>(unknown).is_err());

    let mut unknown_variant = encoded;
    unknown_variant["nodes"][0]
        .as_object_mut()
        .unwrap()
        .insert("type".to_owned(), serde_json::json!("dom-node"));
    assert!(serde_json::from_value::<Scene2D>(unknown_variant).is_err());
}

#[test]
fn compile_rejects_documents_with_unknown_datasets() {
    let invalid = Document {
        version: "0.1".to_owned(),
        id: "unknown-dataset".to_owned(),
        width: 400.0,
        height: 300.0,
        background: Color::transparent(),
        title: None,
        datasets: BTreeMap::new(),
        views: vec![View::Scatter(ScatterChart {
            id: "points".to_owned(),
            title: None,
            frame: Frame {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
            },
            dataset: "missing".to_owned(),
            x: FieldEncoding {
                field: "x".to_owned(),
                label: None,
            },
            y: FieldEncoding {
                field: "y".to_owned(),
                label: None,
            },
            color: None,
            point_size: 7.0,
        })],
    };
    let message = compile(&invalid).unwrap_err().to_string();
    assert!(message.contains("VIZ-VALIDATE-0101"));
    assert!(message.contains("unknown dataset \"missing\""));
    assert!(message.contains("views[0].dataset"));
}
