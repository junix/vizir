use std::path::PathBuf;

use vizir_compiler::{build_scene, lower_to_mir};
use vizir_core::{
    Color, MirGeometryNode, MirShapeStyle, MirView, Point, VizMir, parse_document, validate_mir,
};

fn style() -> MirShapeStyle {
    MirShapeStyle {
        fill: Color::hex("#123456"),
        stroke: Color::transparent(),
        stroke_width: 0.0,
        opacity: 1.0,
    }
}

fn geometry_mir(children: Vec<MirGeometryNode>) -> VizMir {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/geometry/visual-grammar.viz.yaml");
    let document = parse_document(path).unwrap();
    let mut mir = lower_to_mir(&document).unwrap();
    let MirView::Geometry(geometry) = &mut mir.views[0] else {
        panic!("fixture must be a geometry scene");
    };
    geometry.title = None;
    geometry.children = children;
    mir
}

#[test]
fn build_scene_rejects_invalid_style_in_otherwise_resolved_geometry() {
    let mut invalid_style = style();
    invalid_style.opacity = 2.0;
    let mir = geometry_mir(vec![MirGeometryNode::Circle {
        id: "circle".to_owned(),
        cx: 10.0,
        cy: 10.0,
        radius: 1.0,
        style: invalid_style,
    }]);
    validate_mir(&mir).expect("MIR reference validation permits this geometry fixture");
    assert!(
        build_scene(&mir).is_err(),
        "invalid resolved style must not cross the Scene2D boundary"
    );
}

#[test]
fn build_scene_rejects_circle_bounds_overflow_from_finite_inputs() {
    let mir = geometry_mir(vec![MirGeometryNode::Circle {
        id: "circle".to_owned(),
        cx: 0.0,
        cy: 0.0,
        radius: f64::MAX,
        style: style(),
    }]);
    validate_mir(&mir).expect("all source values are finite and references resolve");
    assert!(
        build_scene(&mir).is_err(),
        "doubling a finite radius must not publish infinite bounds"
    );
}

#[test]
fn build_scene_rejects_line_bounds_overflow_from_finite_endpoints() {
    let mir = geometry_mir(vec![MirGeometryNode::Line {
        id: "line".to_owned(),
        from: Point {
            x: -f64::MAX,
            y: 0.0,
        },
        to: Point {
            x: f64::MAX,
            y: 0.0,
        },
        style: style(),
    }]);
    validate_mir(&mir).expect("all source values are finite and references resolve");
    assert!(
        build_scene(&mir).is_err(),
        "a finite endpoint span may overflow during lowering"
    );
}

#[test]
fn build_scene_rejects_duplicate_resolved_descendant_ids() {
    let node = MirGeometryNode::Circle {
        id: "same".to_owned(),
        cx: 10.0,
        cy: 10.0,
        radius: 1.0,
        style: style(),
    };
    let mir = geometry_mir(vec![node.clone(), node]);
    validate_mir(&mir).expect("MIR reference validation permits this geometry fixture");
    assert!(
        build_scene(&mir).is_err(),
        "lowering must not publish ambiguous Scene2D identities"
    );
}

#[test]
fn build_scene_keeps_empty_geometry_scenes_valid() {
    let mir = geometry_mir(vec![]);
    let scene = build_scene(&mir).unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(scene.nodes[0].children().is_empty());
}
