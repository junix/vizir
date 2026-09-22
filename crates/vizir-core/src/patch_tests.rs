use super::*;
use crate::{Color, Origin, Rect, ResolvedStyle};

fn rect(id: &str, width: f64) -> SceneNode {
    SceneNode::Rect {
        id: id.to_owned(),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: 1.0,
        },
        origin: Origin {
            hir_node: id.to_owned(),
            mir_node: id.to_owned(),
            data_key: None,
            data_lineage: Vec::new(),
            generated_by: "test".to_owned(),
            explanation: "test".to_owned(),
        },
        radius: 0.0,
        style: ResolvedStyle {
            fill: Color::hex("#000000"),
            stroke: Color::transparent(),
            stroke_width: 0.0,
            opacity: 1.0,
        },
    }
}

fn scene(nodes: Vec<SceneNode>) -> Scene2D {
    Scene2D {
        document_id: "doc".to_owned(),
        width: 10.0,
        height: 10.0,
        background: Color::transparent(),
        nodes,
        losses: Vec::new(),
    }
}

#[test]
fn diff_and_apply_match_full_scene_semantics() {
    let previous = scene(vec![rect("a", 1.0), rect("b", 2.0)]);
    let next = scene(vec![rect("b", 3.0), rect("c", 4.0)]);
    let patch = diff_scene(
        &previous,
        &next,
        Revision(7),
        Revision(8),
        "transaction/test",
    )
    .expect("diff should succeed");
    assert_eq!(patch.protocol_version, "0.1");
    assert_eq!(patch.document_id, "doc");
    assert_eq!(patch.transaction_id, "transaction/test");
    assert_eq!(patch.base_revision, Revision(7));
    assert_eq!(patch.target_revision, Revision(8));
    // Removals first, then in next-order replace/insert, then the reorder
    // that any insertion triggers for the surviving child sequence.
    assert_eq!(
        patch.operations,
        vec![
            SceneOp::RemoveNode { id: "a".to_owned() },
            SceneOp::ReplaceNode {
                id: "b".to_owned(),
                node: rect("b", 3.0),
            },
            SceneOp::InsertNode {
                parent: SceneParent::Root,
                index: 1,
                node: rect("c", 4.0),
            },
            SceneOp::ReorderChildren {
                parent: SceneParent::Root,
                order: vec!["b".to_owned(), "c".to_owned()],
            },
        ]
    );
    let (actual, revision) =
        apply_scene_patch(&previous, Revision(7), &patch).expect("patch should apply atomically");
    assert_eq!(revision, Revision(8));
    assert_eq!(actual, next);
}

#[test]
fn diff_rejects_cross_document_and_non_advancing_revisions() {
    let previous = scene(vec![rect("a", 1.0)]);
    let mut foreign = scene(vec![rect("a", 2.0)]);
    foreign.document_id = "other-doc".to_owned();
    let error = diff_scene(&previous, &foreign, Revision(7), Revision(8), "t")
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "VIZ-PATCH-0001: cannot diff documents \"doc\" and \"other-doc\""
    );

    let next = scene(vec![rect("a", 2.0)]);
    // Boundaries: a target equal to or behind the base is not an advance.
    for target in [Revision(7), Revision(6)] {
        let error = diff_scene(&previous, &next, Revision(7), target, "t")
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!(
                "VIZ-PATCH-0002: target revision {} must be greater than base revision 7",
                target.0
            )
        );
    }
}

#[test]
fn apply_rejects_foreign_patches_and_non_advancing_revisions() {
    let previous = scene(vec![rect("a", 1.0)]);
    let next = scene(vec![rect("a", 2.0), rect("b", 3.0)]);
    let mut patch = diff_scene(
        &previous,
        &next,
        Revision(1),
        Revision(2),
        "transaction/test",
    )
    .unwrap();

    patch.protocol_version = "0.9".to_owned();
    let error = apply_scene_patch(&previous, Revision(1), &patch)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "VIZ-PATCH-0003: unsupported ScenePatch version \"0.9\""
    );

    patch.protocol_version = "0.1".to_owned();
    patch.document_id = "other-doc".to_owned();
    let error = apply_scene_patch(&previous, Revision(1), &patch)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "VIZ-PATCH-0004: patch document \"other-doc\" does not match scene \"doc\""
    );

    patch.document_id = "doc".to_owned();
    patch.target_revision = Revision(1);
    let error = apply_scene_patch(&previous, Revision(1), &patch)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "VIZ-PATCH-0002: target revision 1 must be greater than base revision 1"
    );
}

#[test]
fn apply_rejects_each_malformed_operation_with_its_exact_diagnostic() {
    fn single_op_patch(operations: Vec<SceneOp>) -> ScenePatch {
        ScenePatch {
            protocol_version: "0.1".to_owned(),
            document_id: "doc".to_owned(),
            transaction_id: "transaction/test".to_owned(),
            base_revision: Revision(1),
            target_revision: Revision(2),
            operations,
        }
    }

    let cases = [
        (
            "insert beyond child count",
            SceneOp::InsertNode {
                parent: SceneParent::Root,
                index: 2,
                node: rect("b", 2.0),
            },
            "VIZ-PATCH-0006: insert index 2 exceeds child count 1",
        ),
        (
            "insert a duplicate id",
            SceneOp::InsertNode {
                parent: SceneParent::Root,
                index: 1,
                node: rect("a", 2.0),
            },
            "VIZ-PATCH-0007: inserted node id \"a\" already exists",
        ),
        (
            "remove a missing node",
            SceneOp::RemoveNode {
                id: "missing".to_owned(),
            },
            "VIZ-PATCH-0008: cannot remove missing scene node \"missing\"",
        ),
        (
            "replace that renames the stable id",
            SceneOp::ReplaceNode {
                id: "a".to_owned(),
                node: rect("renamed", 2.0),
            },
            "VIZ-PATCH-0009: replacement changes stable id \"a\" to \"renamed\"",
        ),
        (
            "replace a missing node",
            SceneOp::ReplaceNode {
                id: "missing".to_owned(),
                node: rect("missing", 2.0),
            },
            "VIZ-PATCH-0010: cannot replace missing scene node \"missing\"",
        ),
        (
            "reorder that drops a child",
            SceneOp::ReorderChildren {
                parent: SceneParent::Root,
                order: vec![],
            },
            "VIZ-PATCH-0011: reorder must name every child exactly once",
        ),
        (
            "insert under a missing parent",
            SceneOp::InsertNode {
                parent: SceneParent::Node {
                    id: "ghost".to_owned(),
                },
                index: 0,
                node: rect("b", 2.0),
            },
            "VIZ-PATCH-0012: missing parent node \"ghost\"",
        ),
        (
            "insert under a non-group parent",
            SceneOp::InsertNode {
                parent: SceneParent::Node { id: "a".to_owned() },
                index: 0,
                node: rect("b", 2.0),
            },
            "VIZ-PATCH-0013: parent node \"a\" is not a group",
        ),
    ];
    for (name, operation, expected) in cases {
        let previous = scene(vec![rect("a", 1.0)]);
        let patch = single_op_patch(vec![operation]);
        let error = apply_scene_patch(&previous, Revision(1), &patch)
            .unwrap_err()
            .to_string();
        assert_eq!(error, expected, "{name}");
    }
}

#[test]
fn revision_mismatch_rejects_patch() {
    let previous = scene(vec![rect("a", 1.0)]);
    let next = scene(vec![rect("a", 2.0)]);
    let patch = diff_scene(
        &previous,
        &next,
        Revision(1),
        Revision(2),
        "transaction/test",
    )
    .unwrap();
    let error = apply_scene_patch(&previous, Revision(0), &patch).unwrap_err();
    assert!(error.to_string().contains("expected base revision 1"));
}
