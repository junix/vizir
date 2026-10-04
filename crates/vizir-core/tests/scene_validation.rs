use vizir_core::{
    Color, Origin, Rect, ResolvedStyle, Scene2D, SceneNode, Transform2D, validate_scene,
};

fn origin() -> Origin {
    Origin {
        hir_node: "semantic source / 数据".to_owned(),
        mir_node: "normalized source & \"reference\"".to_owned(),
        data_key: Some(String::new()),
        data_lineage: vec![String::new(), "arbitrary key\n值".to_owned()],
        generated_by: "test pass".to_owned(),
        explanation: String::new(),
    }
}

fn scene() -> Scene2D {
    Scene2D {
        document_id: "document & 数据".to_owned(),
        width: 1.0,
        height: 1.0,
        background: Color::transparent(),
        nodes: vec![SceneNode::Group {
            id: "group with spaces".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            transform: Transform2D::default(),
            opacity: 1.0,
            children: vec![SceneNode::Rect {
                id: "opaque / <child> \"身份\"".to_owned(),
                bounds: Rect::default(),
                origin: origin(),
                radius: 5.0,
                style: ResolvedStyle {
                    fill: Color::hex("#aBcDeF00"),
                    stroke: Color::transparent(),
                    stroke_width: 0.0,
                    opacity: 0.0,
                },
            }],
        }],
        losses: Vec::new(),
    }
}

#[test]
fn scene_validation_is_read_only_and_keeps_opaque_identity_and_data_values() {
    let scene = scene();
    let before = serde_json::to_vec(&scene).unwrap();
    validate_scene(&scene).unwrap();
    assert_eq!(serde_json::to_vec(&scene).unwrap(), before);
    let decoded = serde_json::from_slice(&before).unwrap();
    validate_scene(&decoded).unwrap();
    assert_eq!(scene, decoded);
}

#[test]
fn scene_diagnostics_are_aggregated_deterministically_with_field_paths() {
    let mut scene = scene();
    scene.document_id = " \n\t".to_owned();
    scene.width = f64::NAN;
    scene.height = 0.0;
    scene.background = Color::hex("red");
    let SceneNode::Group {
        bounds,
        origin,
        transform,
        opacity,
        children,
        ..
    } = &mut scene.nodes[0]
    else {
        unreachable!()
    };
    origin.hir_node.clear();
    origin.mir_node = "\u{2003}".to_owned();
    origin.generated_by = "\n".to_owned();
    bounds.width = -1.0;
    transform.rotate_degrees = f64::INFINITY;
    *opacity = 2.0;
    let SceneNode::Rect { id, style, .. } = &mut children[0] else {
        unreachable!()
    };
    *id = "group with spaces".to_owned();
    style.stroke = Color::hex("#000");
    let first = validate_scene(&scene).unwrap_err();
    assert_eq!(first, validate_scene(&scene).unwrap_err());
    assert_eq!(
        first
            .iter()
            .map(|d| d.source.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "document_id",
            "width",
            "height",
            "background",
            "nodes[0].origin.hir_node",
            "nodes[0].origin.mir_node",
            "nodes[0].origin.generated_by",
            "nodes[0].bounds.width",
            "nodes[0].transform.rotate_degrees",
            "nodes[0].opacity",
            "nodes[0].children[0].id",
            "nodes[0].children[0].style.stroke",
        ]
    );
    assert_eq!(first[0].code, "VIZ-SCENE-0101");
    assert_eq!(first[10].code, "VIZ-SCENE-0102");
    assert!(scene.width.is_nan());
}

#[test]
fn duplicate_ids_are_global_and_opaque_without_normalization() {
    let mut scene = scene();
    let mut other = scene.nodes[0].clone();
    let SceneNode::Group { id, .. } = &mut other else {
        unreachable!()
    };
    *id = " group with spaces ".to_owned();
    // Whitespace is part of a nonblank ID, but duplicate descendants still clash.
    scene.nodes.push(other.clone());
    let diagnostics = validate_scene(&scene).unwrap_err();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].source.as_deref(),
        Some("nodes[1].children[0].id")
    );
    let SceneNode::Group { children, .. } = &mut other else {
        unreachable!()
    };
    children.clear();
    scene.nodes[1] = other;
    validate_scene(&scene).unwrap();
}
