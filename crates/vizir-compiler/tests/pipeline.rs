use std::path::PathBuf;

use vizir_compiler::compile;
use vizir_core::{MirScale, MirView, SceneNode, find_scene_node, parse_document};

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

    let point = find_scene_node(&first.scene.nodes, "latency-risk/point/gateway")
        .expect("stable datum scene node exists");
    assert_eq!(point.origin().data_key.as_deref(), Some("gateway"));
    assert_eq!(point.origin().hir_node, "latency-risk");
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
