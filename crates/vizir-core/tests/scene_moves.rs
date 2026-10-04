use std::collections::BTreeSet;

use vizir_core::{
    Color, Origin, Rect, ResolvedStyle, Revision, Scene2D, SceneNode, SceneOp, SceneParent,
    ScenePatch, Transform2D, apply_scene_patch, diff_scene, validate_scene,
};

fn origin() -> Origin {
    Origin {
        hir_node: "source".to_owned(),
        mir_node: "resolved".to_owned(),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: "test".to_owned(),
        explanation: "scene movement".to_owned(),
    }
}

fn rect(id: &str) -> SceneNode {
    SceneNode::Rect {
        id: id.to_owned(),
        bounds: Rect::default(),
        origin: origin(),
        radius: 0.0,
        style: ResolvedStyle {
            fill: Color::hex("#123456"),
            stroke: Color::transparent(),
            stroke_width: 0.0,
            opacity: 1.0,
        },
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

fn assert_round_trip(previous: &Scene2D, next: &Scene2D) -> ScenePatch {
    validate_scene(previous).unwrap();
    validate_scene(next).unwrap();
    let untouched = previous.clone();
    let patch = diff_scene(previous, next, Revision(1), Revision(2), "move").unwrap();
    let actual = apply_scene_patch(previous, Revision(1), &patch).unwrap_or_else(|error| {
        panic!("{error}\nprevious: {previous:?}\nnext: {next:?}\npatch: {patch:?}")
    });
    assert_eq!(actual, (next.clone(), Revision(2)));
    assert_eq!(*previous, untouched);
    let repeated = diff_scene(previous, next, Revision(1), Revision(2), "move").unwrap();
    assert_eq!(
        serde_json::to_vec(&patch).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    let decoded: ScenePatch = serde_json::from_slice(&serde_json::to_vec(&patch).unwrap()).unwrap();
    assert_eq!(decoded, patch);

    // Every prefix must resolve unambiguous IDs, not just the final transaction.
    for count in 1..=patch.operations.len() {
        let mut prefix = patch.clone();
        prefix.operations.truncate(count);
        let (partial, _) = apply_scene_patch(previous, Revision(1), &prefix).unwrap();
        validate_scene(&partial).unwrap();
    }
    patch
}

#[test]
fn moving_to_an_earlier_parent_detaches_before_inserting() {
    let previous = scene(vec![
        group("left", vec![]),
        group("right", vec![rect("child")]),
    ]);
    let next = scene(vec![
        group("left", vec![rect("child")]),
        group("right", vec![]),
    ]);
    let patch = assert_round_trip(&previous, &next);
    assert_eq!(
        patch.operations,
        vec![
            SceneOp::RemoveNode {
                id: "child".to_owned()
            },
            SceneOp::InsertNode {
                parent: SceneParent::Node {
                    id: "left".to_owned()
                },
                index: 0,
                node: rect("child"),
            },
            SceneOp::ReorderChildren {
                parent: SceneParent::Node {
                    id: "left".to_owned()
                },
                order: vec!["child".to_owned()],
            },
        ]
    );
    assert_round_trip(&next, &previous);
}

#[test]
fn moving_subtrees_and_descendants_does_not_remove_an_id_twice() {
    let previous = scene(vec![
        group("left", vec![rect("stay-left")]),
        group(
            "right",
            vec![group("moved", vec![rect("child"), rect("stays-in-moved")])],
        ),
    ]);
    let next = scene(vec![
        group(
            "left",
            vec![
                rect("stay-left"),
                group("moved", vec![rect("stays-in-moved")]),
            ],
        ),
        group("right", vec![rect("child")]),
    ]);
    let patch = assert_round_trip(&previous, &next);
    let removals = patch
        .operations
        .iter()
        .filter_map(|op| match op {
            SceneOp::RemoveNode { id } => Some(id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(removals, ["moved"]);
    assert_round_trip(&next, &previous);
}

#[test]
fn descendants_survive_removed_replaced_and_new_ancestors() {
    let previous = scene(vec![
        group("left", vec![]),
        group("right", vec![group("old", vec![rect("child")])]),
    ]);
    let variants = [
        scene(vec![group("left", vec![rect("child")])]),
        scene(vec![group("left", vec![rect("child")]), rect("right")]),
        scene(vec![
            group("new", vec![group("left", vec![rect("child")])]),
            rect("right"),
        ]),
    ];
    for next in variants {
        assert_round_trip(&previous, &next);
        assert_round_trip(&next, &previous);
    }

    // Property changes replace an entire group, so moving a descendant into or
    // out of that group must release IDs before either replacement runs.
    let previous = scene(vec![
        group("left", vec![]),
        group("right", vec![rect("child")]),
    ]);
    let mut next = scene(vec![
        group("left", vec![rect("child")]),
        group("right", vec![]),
    ]);
    for node in &mut next.nodes {
        if let SceneNode::Group { opacity, .. } = node {
            *opacity = 0.5;
        }
    }
    let patch = assert_round_trip(&previous, &next);
    assert!(matches!(patch.operations[0], SceneOp::RemoveNode { .. }));
    assert_eq!(
        patch
            .operations
            .iter()
            .filter(|op| matches!(op, SceneOp::ReplaceNode { .. }))
            .count(),
        2
    );
    assert_round_trip(&next, &previous);
}

#[test]
fn root_parent_inversion_and_reorders_use_the_detached_state() {
    let previous = scene(vec![
        group("a", vec![group("b", vec![rect("x")]), rect("y"), rect("z")]),
        rect("root"),
    ]);
    let next = scene(vec![
        rect("z"),
        rect("root"),
        group("b", vec![rect("y"), group("a", vec![rect("x")])]),
    ]);
    let patch = assert_round_trip(&previous, &next);
    assert!(matches!(&patch.operations[0], SceneOp::RemoveNode { id } if id == "a"));
    assert_round_trip(&next, &previous);
}

#[test]
fn moving_between_groups_with_insertions_removals_and_reorders() {
    let previous = scene(vec![
        group("left", vec![rect("a"), rect("b"), rect("c")]),
        group("right", vec![rect("x"), rect("y"), rect("z")]),
        rect("deleted"),
    ]);
    let mut next = scene(vec![
        group(
            "right",
            vec![rect("new"), rect("b"), rect("y"), rect("c"), rect("x")],
        ),
        group("left", vec![rect("z"), rect("a")]),
    ]);
    next.width = 120.0;
    let patch = assert_round_trip(&previous, &next);
    assert!(matches!(
        patch.operations[0],
        SceneOp::SetSceneProperties { .. }
    ));
    assert_round_trip(&next, &previous);
}

#[test]
fn ordinary_diff_preserves_existing_operation_order() {
    let previous = scene(vec![
        group("left", vec![rect("a"), rect("b"), rect("c")]),
        group("right", vec![rect("deleted")]),
    ]);
    let next = scene(vec![
        group("left", vec![rect("b"), rect("new"), rect("a")]),
        group("right", vec![]),
    ]);
    let patch = assert_round_trip(&previous, &next);
    assert_eq!(
        patch.operations,
        vec![
            SceneOp::RemoveNode { id: "c".to_owned() },
            SceneOp::InsertNode {
                parent: SceneParent::Node {
                    id: "left".to_owned()
                },
                index: 1,
                node: rect("new"),
            },
            SceneOp::ReorderChildren {
                parent: SceneParent::Node {
                    id: "left".to_owned()
                },
                order: vec!["b".to_owned(), "new".to_owned(), "a".to_owned()],
            },
            SceneOp::RemoveNode {
                id: "deleted".to_owned()
            },
        ]
    );
}

#[test]
fn failed_operation_after_a_move_leaves_the_original_scene_untouched() {
    let previous = scene(vec![
        group("left", vec![]),
        group("right", vec![rect("child")]),
    ]);
    let next = scene(vec![
        group("left", vec![rect("child")]),
        group("right", vec![]),
    ]);
    let mut patch = assert_round_trip(&previous, &next);
    patch.operations.push(SceneOp::RemoveNode {
        id: "missing".to_owned(),
    });
    let untouched = previous.clone();
    let error = apply_scene_patch(&previous, Revision(1), &patch)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "VIZ-PATCH-0008: cannot remove missing scene node \"missing\""
    );
    assert_eq!(previous, untouched);
}

// Enumerate every ordered forest on three named groups, including parent/root
// inversions. Some leaves become rects, and other variants force subtree
// replacement, deletion, insertion, and group-property changes.
fn ordered_forests() -> Vec<Scene2D> {
    const N: usize = 3;
    const ORDERS: [[usize; N]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    fn children(parent: usize, parents: &[usize; N], order: &[usize; N]) -> Vec<SceneNode> {
        order
            .iter()
            .filter(|&&id| parents[id] == parent)
            .map(|&id| group(&format!("n{id}"), children(id, parents, order)))
            .collect()
    }
    let mut unique = BTreeSet::new();
    let mut scenes = Vec::new();
    for code in 0..(N + 1).pow(N as u32) {
        let mut value = code;
        let mut parents = [N; N];
        for parent in &mut parents {
            *parent = value % (N + 1);
            value /= N + 1;
        }
        let acyclic = (0..N).all(|id| {
            let mut cursor = id;
            for _ in 0..N {
                cursor = parents[cursor];
                if cursor == N {
                    return true;
                }
            }
            false
        });
        if !acyclic {
            continue;
        }
        for order in ORDERS {
            let candidate = scene(children(N, &parents, &order));
            if unique.insert(serde_json::to_string(&candidate).unwrap()) {
                scenes.push(candidate);
            }
        }
    }
    assert_eq!(scenes.len(), 30);
    scenes
}

fn vary_nodes(nodes: &mut Vec<SceneNode>, variant: usize) {
    if variant == 3 {
        nodes.retain(|node| node.id() != "n1");
    }
    for node in nodes {
        if let SceneNode::Group {
            id,
            opacity,
            children,
            ..
        } = node
        {
            vary_nodes(children, variant);
            if variant == 1 && children.is_empty() {
                *node = rect(id);
            } else if variant == 2 {
                *opacity = 0.5;
            } else if variant == 4 && id == "n1" {
                children.insert(0, rect("new"));
            }
        }
    }
}

#[test]
fn every_small_forest_transition_and_mixed_variant_round_trips() {
    let forests = ordered_forests();
    for previous in &forests {
        for next in &forests {
            for variant in 0..5 {
                let mut next = next.clone();
                vary_nodes(&mut next.nodes, variant);
                assert_round_trip(previous, &next);
                // Mixed variants also exercise recreation of missing IDs and
                // primitive-to-group replacement in the reverse direction.
                if variant != 0 {
                    assert_round_trip(&next, previous);
                }
            }
        }
    }
}
