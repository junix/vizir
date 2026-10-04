use std::path::PathBuf;

use serde_json::{Value, json};
use vizir_compiler::compile;
use vizir_core::{
    Color, CompositionV1, DiagramLayout, Document, Frame, GeometryNode, Point, Rect, SceneNode,
    ShapeStyle, Transform2D, View, compose, find_scene_node, parse_composition, parse_document,
    validate_document,
};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(panels: Value) -> Value {
    json!({
        "schema": "vizir-composition/0.1",
        "id": "composition-contract",
        "width": 1440,
        "height": 600,
        "layout": {"kind": "grid", "columns": 2, "gap": 24, "padding": 24},
        "panels": panels
    })
}

fn composition(source: &Value) -> CompositionV1 {
    serde_json::from_value(source.clone()).expect("typed composition parses")
}

// The reference is ordinary explicit HIR. Callers supply independently known
// frames, rather than repeating compose's allocation algorithm in this helper.
fn explicit_hir(source: &Value, frames: &[Frame]) -> Document {
    let mut value = source.clone();
    let root = value.as_object_mut().unwrap();
    root.remove("schema");
    root.remove("layout");
    root.insert("version".to_owned(), json!("0.2"));
    let mut views = root.remove("panels").unwrap();
    assert_eq!(views.as_array().unwrap().len(), frames.len());
    for (view, frame) in views.as_array_mut().unwrap().iter_mut().zip(frames) {
        view["frame"] = serde_json::to_value(frame).unwrap();
    }
    root.insert("views".to_owned(), views);
    serde_json::from_value(value).expect("explicit reference HIR parses")
}

fn two_frames() -> [Frame; 2] {
    [
        Frame {
            x: 24.0,
            y: 24.0,
            width: 684.0,
            height: 552.0,
        },
        Frame {
            x: 732.0,
            y: 24.0,
            width: 684.0,
            height: 552.0,
        },
    ]
}

#[test]
fn mixed_demo_is_identical_to_explicit_hir_through_mir_and_scene() {
    let input =
        parse_composition(workspace().join("examples/composition/service-grid.compose.yaml"))
            .expect("mixed demo parses");
    let document = compose(&input).expect("mixed demo composes");
    let explicit = explicit_hir(&serde_json::to_value(&input).unwrap(), &two_frames());
    assert_eq!(document, explicit);
    assert_eq!(document.version, "0.2");
    assert_eq!(document.background, Color::transparent());
    validate_document(&document).expect("composition returns validated ordinary HIR");

    let compiled = compile(&document).expect("composed HIR compiles");
    let reference = compile(&explicit).expect("explicit HIR compiles");
    assert_eq!(compiled.mir, reference.mir);
    assert_eq!(compiled.scene, reference.scene);
    assert_eq!(compiled.mir.source_hir_version, "0.2");
    assert_eq!(compiled.mir.version, "0.2");

    let datum = find_scene_node(&compiled.scene.nodes, "latency-risk/point/gateway")
        .expect("stable dataset key is preserved");
    assert_eq!(datum.origin().data_key.as_deref(), Some("gateway"));
    assert_eq!(datum.origin().hir_node, "latency-risk");
    assert!(
        find_scene_node(&compiled.scene.nodes, "service-topology/node/gateway/shape").is_some()
    );
}

#[test]
fn composition_and_compilation_are_repeatable_without_mutating_source() {
    let input =
        parse_composition(workspace().join("examples/composition/service-grid.compose.yaml"))
            .unwrap();
    let before = serde_json::to_vec(&input).unwrap();
    let first_document = compose(&input).unwrap();
    let first = compile(&first_document).unwrap();
    for _ in 0..3 {
        let document = compose(&input).unwrap();
        let compiled = compile(&document).unwrap();
        assert_eq!(
            serde_json::to_vec(&document).unwrap(),
            serde_json::to_vec(&first_document).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&compiled.mir).unwrap(),
            serde_json::to_vec(&first.mir).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&compiled.scene).unwrap(),
            serde_json::to_vec(&first.scene).unwrap()
        );
    }
    assert_eq!(serde_json::to_vec(&input).unwrap(), before);
}

#[test]
fn all_five_panel_variants_keep_hir_defaults_and_row_major_order() {
    let mut source = source(json!([
        {"kind": "chart.scatter", "id": "scatter", "dataset": "readings",
         "x": {"field": "x"}, "y": {"field": "y"}},
        {"kind": "chart.line", "id": "line", "dataset": "readings",
         "x": {"field": "x"}, "y": {"field": "y"}},
        {"kind": "chart.bar", "id": "bar", "dataset": "readings",
         "category": {"field": "name"}, "value": {"field": "y"}},
        {"kind": "diagram.graph", "id": "diagram", "nodes": [{"id": "n", "label": "Node"}]},
        {"kind": "geometry.scene", "id": "geometry", "children": [
            {"type": "rect", "id": "box", "x": 10, "y": 20, "width": 30, "height": 40}
        ]}
    ]));
    source["height"] = json!(1200);
    source["datasets"] = json!({"readings": {"key": "id", "rows": [
        {"id": "a", "name": "A", "x": 1, "y": 2},
        {"id": "b", "name": "B", "x": 2, "y": 4}
    ]}});
    let frames = [
        (24.0, 24.0),
        (732.0, 24.0),
        (24.0, 416.0),
        (732.0, 416.0),
        (24.0, 808.0),
    ]
    .map(|(x, y)| Frame {
        x,
        y,
        width: 684.0,
        height: 368.0,
    });
    let document = compose(&composition(&source)).unwrap();
    let explicit = explicit_hir(&source, &frames);
    assert_eq!(document, explicit);
    assert_eq!(document.background, Color::transparent());
    assert!(document.title.is_none());
    assert_eq!(
        document.views.iter().map(View::id).collect::<Vec<_>>(),
        ["scatter", "line", "bar", "diagram", "geometry"]
    );

    let View::Scatter(scatter) = &document.views[0] else {
        panic!("scatter kind changed")
    };
    assert_eq!(scatter.point_size, 7.0);
    assert!(scatter.color.is_none());
    assert!(scatter.x.axis.is_none());
    let View::Line(line) = &document.views[1] else {
        panic!("line kind changed")
    };
    assert_eq!(line.line_width, 2.5);
    assert!(line.show_points);
    assert!(line.series.is_none());
    let View::Bar(bar) = &document.views[2] else {
        panic!("bar kind changed")
    };
    assert!(bar.color.is_none());
    let View::Diagram(diagram) = &document.views[3] else {
        panic!("diagram kind changed")
    };
    assert_eq!(diagram.layout, DiagramLayout::Layered);
    assert!(diagram.edges.is_empty());
    assert_eq!(diagram.nodes[0].style, ShapeStyle::default());
    let View::Geometry(geometry) = &document.views[4] else {
        panic!("geometry kind changed")
    };
    let GeometryNode::Rect { radius, style, .. } = &geometry.children[0] else {
        panic!("typed geometry was not preserved")
    };
    assert_eq!(*radius, 0.0);
    assert_eq!(*style, ShapeStyle::default());

    let compiled = compile(&document).unwrap();
    let reference = compile(&explicit).unwrap();
    assert_eq!(compiled.mir, reference.mir);
    assert_eq!(compiled.scene, reference.scene);
}

#[test]
fn panel_coordinates_stay_local_until_the_existing_lowering_steps() {
    let source = source(json!([
        {"kind": "diagram.graph", "id": "manual", "layout": "manual", "nodes": [
            {"id": "node", "label": "Pinned", "position": {"x": 100, "y": 120}}
        ]},
        {"kind": "geometry.scene", "id": "local", "children": [
            {"type": "rect", "id": "box", "x": 10, "y": 20, "width": 30, "height": 40},
            {"type": "group", "id": "nested", "transform": {"translate": {"x": 7, "y": 11}},
             "children": [{"type": "rect", "id": "nested-box", "x": 3, "y": 5,
                           "width": 13, "height": 17}]}
        ]}
    ]));
    let document = compose(&composition(&source)).unwrap();
    assert_eq!(document, explicit_hir(&source, &two_frames()));
    let View::Diagram(manual) = &document.views[0] else {
        panic!("expected diagram")
    };
    assert_eq!(manual.nodes[0].position, Some(Point { x: 100.0, y: 120.0 }));
    let compiled = compile(&document).unwrap();
    let SceneNode::Group { transform, .. } =
        find_scene_node(&compiled.scene.nodes, "manual").unwrap()
    else {
        panic!("diagram should emit a group")
    };
    assert_eq!(*transform, Transform2D::default());
    let SceneNode::Rect { bounds, .. } =
        find_scene_node(&compiled.scene.nodes, "manual/node/node/shape").unwrap()
    else {
        panic!("manual node should emit a rectangle")
    };
    // Manual coordinates become document coordinates exactly once. The node's
    // center is (24 + 100, 24 + 120), with the standard 150 by 62 node size.
    assert_eq!(
        *bounds,
        Rect {
            x: 49.0,
            y: 113.0,
            width: 150.0,
            height: 62.0
        }
    );
    let SceneNode::Group { transform, .. } =
        find_scene_node(&compiled.scene.nodes, "local").unwrap()
    else {
        panic!("geometry should emit a group")
    };
    assert_eq!(transform.translate, Point { x: 732.0, y: 24.0 });
    assert_eq!(transform.scale, Point { x: 1.0, y: 1.0 });
    let SceneNode::Rect { bounds, .. } =
        find_scene_node(&compiled.scene.nodes, "local/box").unwrap()
    else {
        panic!("geometry child should emit a rectangle")
    };
    assert_eq!(
        *bounds,
        Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0
        }
    );
    let SceneNode::Group { transform, .. } =
        find_scene_node(&compiled.scene.nodes, "local/nested").unwrap()
    else {
        panic!("nested geometry group should remain a group")
    };
    assert_eq!(transform.translate, Point { x: 7.0, y: 11.0 });
    let SceneNode::Rect { bounds, .. } =
        find_scene_node(&compiled.scene.nodes, "local/nested-box").unwrap()
    else {
        panic!("nested child should remain a rectangle")
    };
    assert_eq!(
        *bounds,
        Rect {
            x: 3.0,
            y: 5.0,
            width: 13.0,
            height: 17.0
        }
    );
}

#[test]
fn frame_allocation_does_not_resize_or_clip_geometry() {
    let input = composition(&json!({
        "schema": "vizir-composition/0.1", "id": "overflow", "width": 100, "height": 100,
        "layout": {"kind": "grid", "columns": 1, "padding": 10},
        "panels": [{"kind": "geometry.scene", "id": "local", "children": [
            {"type": "rect", "id": "oversized", "x": 5, "y": 7, "width": 200, "height": 180}
        ]}]
    }));
    let document = compose(&input).unwrap();
    assert_eq!(
        *document.views[0].frame(),
        Frame {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 80.0
        }
    );
    let compiled = compile(&document).unwrap();
    let SceneNode::Group {
        transform,
        children,
        ..
    } = &compiled.scene.nodes[0]
    else {
        panic!("geometry root should be a group")
    };
    assert_eq!(transform.scale, Point { x: 1.0, y: 1.0 });
    assert_eq!(
        children.len(),
        1,
        "compose must not inject a clipping wrapper"
    );
    let SceneNode::Rect { bounds, .. } = &children[0] else {
        panic!("geometry child should remain the original rectangle")
    };
    assert_eq!(
        *bounds,
        Rect {
            x: 5.0,
            y: 7.0,
            width: 200.0,
            height: 180.0
        }
    );
}

#[test]
fn legacy_hir_readers_and_version_defaults_remain_unchanged() {
    let legacy = parse_document(workspace().join("examples/chart/service-health.viz.yaml"))
        .expect("legacy HIR remains accepted");
    assert_eq!(legacy.version, "0.1");
    let compiled = compile(&legacy).unwrap();
    assert_eq!(compiled.mir.version, "0.1");
    assert_eq!(compiled.mir.source_hir_version, "0.1");

    let mut value = serde_json::to_value(&legacy).unwrap();
    value.as_object_mut().unwrap().remove("version");
    let defaulted: Document = serde_json::from_value(value).unwrap();
    assert_eq!(defaulted, legacy);
    let again = compile(&defaulted).unwrap();
    assert_eq!(again.mir, compiled.mir);
    assert_eq!(again.scene, compiled.scene);
    assert!(
        parse_document(workspace().join("examples/composition/service-grid.compose.yaml")).is_err(),
        "a composition wrapper must not silently become HIR"
    );
}
