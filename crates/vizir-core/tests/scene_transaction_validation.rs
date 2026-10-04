use vizir_core::{
    Color, Origin, Rect, ResolvedStyle, Revision, Scene2D, SceneNode, SceneOp, SceneParent,
    ScenePatch, Transform2D, apply_scene_patch, diff_scene,
};

fn rect(id: &str) -> SceneNode {
    SceneNode::Rect {
        id: id.to_owned(),
        bounds: Rect {
            x: -2.0,
            y: -3.0,
            width: 4.0,
            height: 5.0,
        },
        origin: origin(),
        radius: 0.0,
        style: ResolvedStyle {
            fill: Color::hex("#12345678"),
            stroke: Color::transparent(),
            stroke_width: 0.0,
            opacity: 1.0,
        },
    }
}

fn origin() -> Origin {
    Origin {
        hir_node: "source".to_owned(),
        mir_node: "resolved".to_owned(),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: "test".to_owned(),
        explanation: "validation boundary".to_owned(),
    }
}

fn group(id: &str, children: Vec<SceneNode>) -> SceneNode {
    SceneNode::Group {
        id: id.to_owned(),
        bounds: Rect::default(),
        origin: origin(),
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    }
}

fn scene(nodes: Vec<SceneNode>) -> Scene2D {
    Scene2D {
        document_id: "document".to_owned(),
        width: 100.0,
        height: 80.0,
        background: Color::transparent(),
        nodes,
        losses: Vec::new(),
    }
}

fn patch(operations: Vec<SceneOp>) -> ScenePatch {
    ScenePatch {
        protocol_version: "0.1".to_owned(),
        document_id: "document".to_owned(),
        transaction_id: "test/validation".to_owned(),
        base_revision: Revision(3),
        target_revision: Revision(4),
        operations,
    }
}

fn properties(width: f64, height: f64, background: &str) -> SceneOp {
    SceneOp::SetSceneProperties {
        width,
        height,
        background: Color(background.to_owned()),
        losses: Vec::new(),
    }
}

fn insert(node: SceneNode, index: usize) -> SceneOp {
    SceneOp::InsertNode {
        parent: SceneParent::Root,
        index,
        node,
    }
}

fn replace(node: SceneNode) -> SceneOp {
    SceneOp::ReplaceNode {
        id: node.id().to_owned(),
        node,
    }
}

fn invalid_scenes() -> Vec<(&'static str, Scene2D)> {
    let mut width = scene(vec![]);
    width.width = f64::NAN;
    let mut height = scene(vec![]);
    height.height = 0.0;
    let mut background = scene(vec![]);
    background.background = Color("url(https://example.invalid/paint)".to_owned());
    let mut document = scene(vec![]);
    document.document_id = " \t ".to_owned();
    vec![
        ("nonfinite viewport", width),
        ("zero viewport", height),
        ("unsupported background", background),
        ("blank document id", document),
        (
            "blank node id",
            scene(vec![group("root", vec![rect(" \t ")])]),
        ),
        (
            "duplicate descendants",
            scene(vec![group("root", vec![rect("child"), rect("child")])]),
        ),
        (
            "cross-subtree duplicate",
            scene(vec![
                group("a", vec![rect("child")]),
                group("b", vec![rect("child")]),
            ]),
        ),
    ]
}

#[test]
fn diff_validates_previous_scene_even_when_next_repairs_it() {
    for (name, previous) in invalid_scenes() {
        let mut next = scene(vec![]);
        // Match the envelope so it cannot mask an invalid scene body.
        next.document_id = previous.document_id.clone();
        assert!(
            diff_scene(&previous, &next, Revision(3), Revision(4), "t").is_err(),
            "{name}"
        );
    }
}

#[test]
fn diff_validates_next_scene_before_emitting_operations() {
    for (name, next) in invalid_scenes() {
        let mut previous = scene(vec![]);
        previous.document_id = next.document_id.clone();
        assert!(
            diff_scene(&previous, &next, Revision(3), Revision(4), "t").is_err(),
            "{name}"
        );
    }
}

#[test]
fn diff_preserves_envelope_diagnostic_precedence() {
    let mut invalid = scene(vec![]);
    invalid.width = f64::NAN;
    let mut foreign = invalid.clone();
    foreign.document_id = "other".to_owned();
    let error = diff_scene(&invalid, &foreign, Revision(3), Revision(3), "t").unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0001:"));
    let error = diff_scene(&invalid, &invalid, Revision(3), Revision(3), "t").unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0002:"));
}

#[test]
fn apply_validates_base_before_a_patch_can_repair_it() {
    let mut invalid = scene(vec![rect("a")]);
    invalid.width = -1.0;
    let repair = patch(vec![properties(100.0, 80.0, "transparent")]);
    assert!(apply_scene_patch(&invalid, Revision(3), &repair).is_err());
    assert_eq!(invalid.width, -1.0);
}

#[test]
fn apply_rejects_invalid_base_even_for_empty_transaction() {
    for (name, invalid) in invalid_scenes() {
        let mut transaction = patch(vec![]);
        transaction.document_id = invalid.document_id.clone();
        assert!(
            apply_scene_patch(&invalid, Revision(3), &transaction).is_err(),
            "{name}"
        );
    }
}

#[test]
fn apply_rejects_duplicate_base_even_when_removal_would_repair_it() {
    let invalid = scene(vec![rect("a"), group("b", vec![rect("a")])]);
    let transaction = patch(vec![SceneOp::RemoveNode { id: "b".to_owned() }]);
    assert!(apply_scene_patch(&invalid, Revision(3), &transaction).is_err());
}

#[test]
fn apply_preserves_envelope_diagnostic_precedence_over_invalid_base() {
    let mut invalid = scene(vec![]);
    invalid.width = f64::INFINITY;
    let mut transaction = patch(vec![]);
    transaction.protocol_version = "unsupported".to_owned();
    transaction.document_id = "foreign".to_owned();
    transaction.target_revision = Revision(3);
    let error = apply_scene_patch(&invalid, Revision(2), &transaction).unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0003:"));
    transaction.protocol_version = "0.1".to_owned();
    let error = apply_scene_patch(&invalid, Revision(2), &transaction).unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0004:"));
    transaction.document_id = invalid.document_id.clone();
    let error = apply_scene_patch(&invalid, Revision(2), &transaction).unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0005:"));
    let error = apply_scene_patch(&invalid, Revision(3), &transaction).unwrap_err();
    assert!(error.to_string().starts_with("VIZ-PATCH-0002:"));
}

#[test]
fn set_scene_properties_cannot_commit_invalid_viewport_or_background() {
    let base = scene(vec![rect("a")]);
    for operation in [
        properties(0.0, 80.0, "transparent"),
        properties(-1.0, 80.0, "transparent"),
        properties(f64::NAN, 80.0, "transparent"),
        properties(100.0, f64::INFINITY, "transparent"),
        properties(100.0, -1.0, "transparent"),
        properties(100.0, 80.0, "red"),
        properties(100.0, 80.0, "#xyzxyz"),
    ] {
        assert!(apply_scene_patch(&base, Revision(3), &patch(vec![operation])).is_err());
    }
}

#[test]
fn insert_cannot_commit_invalid_descendant_geometry() {
    let base = scene(vec![rect("existing")]);
    let mut invalid = rect("new-child");
    if let SceneNode::Rect { radius, .. } = &mut invalid {
        *radius = -1.0;
    }
    let transaction = patch(vec![insert(group("new-group", vec![invalid]), 1)]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn replacement_cannot_commit_invalid_descendant_style() {
    let base = scene(vec![group("root", vec![rect("child")])]);
    let mut invalid = rect("child");
    if let SceneNode::Rect { style, .. } = &mut invalid {
        style.opacity = 1.1;
    }
    let transaction = patch(vec![replace(group("root", vec![invalid]))]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn late_final_validation_failure_leaves_input_scene_unchanged() {
    let base = scene(vec![rect("a")]);
    let before = base.clone();
    let transaction = patch(vec![
        insert(rect("b"), 1),
        properties(200.0, 160.0, "#112233"),
        properties(-1.0, 160.0, "#112233"),
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
    assert_eq!(base, before);
    let (unchanged, revision) = apply_scene_patch(&base, Revision(3), &patch(vec![])).unwrap();
    assert_eq!(unchanged, before);
    assert_eq!(revision, Revision(4));
}

#[test]
fn late_operation_failure_leaves_input_scene_unchanged() {
    let base = scene(vec![rect("a")]);
    let before = base.clone();
    let transaction = patch(vec![
        insert(rect("b"), 1),
        SceneOp::RemoveNode {
            id: "missing".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
    assert_eq!(base, before);
}

#[test]
fn insertion_rejects_transient_descendant_collision_with_existing_node() {
    let base = scene(vec![rect("existing")]);
    let transaction = patch(vec![
        insert(group("new-group", vec![rect("existing")]), 1),
        SceneOp::RemoveNode {
            id: "new-group".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn insertion_rejects_transient_duplicates_inside_inserted_subtree() {
    let base = scene(vec![]);
    let transaction = patch(vec![
        insert(
            group(
                "new-group",
                vec![
                    group("left", vec![rect("same")]),
                    group("right", vec![rect("same")]),
                ],
            ),
            0,
        ),
        SceneOp::RemoveNode {
            id: "new-group".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn insertion_rejects_transient_descendant_collision_with_its_own_root() {
    let base = scene(vec![]);
    let transaction = patch(vec![
        insert(group("same", vec![rect("same")]), 0),
        SceneOp::RemoveNode {
            id: "same".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn replacement_rejects_transient_descendant_collision_with_external_node() {
    let base = scene(vec![group("root", vec![rect("old")]), rect("outside")]);
    let transaction = patch(vec![
        replace(group("root", vec![rect("outside")])),
        SceneOp::RemoveNode {
            id: "root".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn replacement_rejects_transient_duplicate_descendants_even_if_replaced_again() {
    let base = scene(vec![group("root", vec![rect("old")])]);
    let transaction = patch(vec![
        replace(group(
            "root",
            vec![rect("same"), group("nested", vec![rect("same")])],
        )),
        replace(group("root", vec![rect("new")])),
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn replacement_rejects_transient_descendant_collision_with_ancestor() {
    let base = scene(vec![group("root", vec![group("nested", vec![])])]);
    let transaction = patch(vec![
        replace(group("nested", vec![rect("root")])),
        SceneOp::RemoveNode {
            id: "nested".to_owned(),
        },
    ]);
    assert!(apply_scene_patch(&base, Revision(3), &transaction).is_err());
}

#[test]
fn replacement_can_reuse_ids_from_the_subtree_it_replaces() {
    let base = scene(vec![group(
        "root",
        vec![rect("child"), group("nested", vec![rect("leaf")])],
    )]);
    let expected = scene(vec![group(
        "root",
        vec![rect("leaf"), group("nested", vec![rect("child")])],
    )]);
    let transaction = patch(vec![replace(expected.nodes[0].clone())]);
    let (actual, revision) = apply_scene_patch(&base, Revision(3), &transaction).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(revision, Revision(4));
}

#[test]
fn transaction_can_fix_transient_invalid_numeric_and_style_properties() {
    let base = scene(vec![rect("a")]);
    let mut invalid = rect("a");
    if let SceneNode::Rect { radius, style, .. } = &mut invalid {
        *radius = f64::NAN;
        style.fill = Color("invalid".to_owned());
        style.opacity = -1.0;
    }
    let transaction = patch(vec![
        properties(-1.0, f64::INFINITY, "invalid"),
        replace(invalid),
        properties(100.0, 80.0, "transparent"),
        replace(rect("a")),
    ]);
    let (actual, _) = apply_scene_patch(&base, Revision(3), &transaction).unwrap();
    assert_eq!(actual, base);
}

#[test]
fn transaction_can_remove_transient_invalid_geometry() {
    let base = scene(vec![]);
    let mut invalid = rect("temporary");
    if let SceneNode::Rect { radius, .. } = &mut invalid {
        *radius = f64::INFINITY;
    }
    let transaction = patch(vec![
        insert(invalid, 0),
        SceneOp::RemoveNode {
            id: "temporary".to_owned(),
        },
    ]);
    let (actual, _) = apply_scene_patch(&base, Revision(3), &transaction).unwrap();
    assert_eq!(actual, base);
}

#[test]
fn opaque_ids_zero_bounds_and_reflected_transforms_round_trip() {
    let mut opaque = group(
        "group with spaces / 雪 \"quoted\"",
        vec![rect(" child α 'quoted' ")],
    );
    if let SceneNode::Group {
        transform, opacity, ..
    } = &mut opaque
    {
        transform.scale.x = -1.0;
        transform.scale.y = 0.0;
        transform.translate.x = -50.0;
        *opacity = 0.0;
    }
    let previous = scene(vec![]);
    let next = scene(vec![opaque]);
    let transaction = diff_scene(&previous, &next, Revision(3), Revision(4), "t").unwrap();
    let (actual, _) = apply_scene_patch(&previous, Revision(3), &transaction).unwrap();
    assert_eq!(actual, next);
}
