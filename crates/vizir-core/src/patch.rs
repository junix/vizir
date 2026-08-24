use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Color, LossRecord, Scene2D, SceneNode, VizError, VizResult};

fn patch_protocol_version() -> String {
    "0.1".to_owned()
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(transparent)]
pub struct Revision(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScenePatch {
    #[serde(default = "patch_protocol_version")]
    pub protocol_version: String,
    pub document_id: String,
    pub transaction_id: String,
    pub base_revision: Revision,
    pub target_revision: Revision,
    pub operations: Vec<SceneOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SceneParent {
    Root,
    Node { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SceneOp {
    SetSceneProperties {
        width: f64,
        height: f64,
        background: Color,
        losses: Vec<LossRecord>,
    },
    InsertNode {
        parent: SceneParent,
        index: usize,
        node: SceneNode,
    },
    RemoveNode {
        id: String,
    },
    ReplaceNode {
        id: String,
        node: SceneNode,
    },
    ReorderChildren {
        parent: SceneParent,
        order: Vec<String>,
    },
}

pub fn diff_scene(
    previous: &Scene2D,
    next: &Scene2D,
    base_revision: Revision,
    target_revision: Revision,
    transaction_id: impl Into<String>,
) -> VizResult<ScenePatch> {
    if previous.document_id != next.document_id {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0001: cannot diff documents {:?} and {:?}",
            previous.document_id, next.document_id
        )));
    }
    if target_revision <= base_revision {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0002: target revision {} must be greater than base revision {}",
            target_revision.0, base_revision.0
        )));
    }

    let mut operations = Vec::new();
    if previous.width != next.width
        || previous.height != next.height
        || previous.background != next.background
        || previous.losses != next.losses
    {
        operations.push(SceneOp::SetSceneProperties {
            width: next.width,
            height: next.height,
            background: next.background.clone(),
            losses: next.losses.clone(),
        });
    }
    diff_children(
        &previous.nodes,
        &next.nodes,
        SceneParent::Root,
        &mut operations,
    );
    Ok(ScenePatch {
        protocol_version: patch_protocol_version(),
        document_id: next.document_id.clone(),
        transaction_id: transaction_id.into(),
        base_revision,
        target_revision,
        operations,
    })
}

pub fn apply_scene_patch(
    scene: &Scene2D,
    current_revision: Revision,
    patch: &ScenePatch,
) -> VizResult<(Scene2D, Revision)> {
    if patch.protocol_version != patch_protocol_version() {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0003: unsupported ScenePatch version {:?}",
            patch.protocol_version
        )));
    }
    if scene.document_id != patch.document_id {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0004: patch document {:?} does not match scene {:?}",
            patch.document_id, scene.document_id
        )));
    }
    if current_revision != patch.base_revision {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0005: expected base revision {}, received {}",
            patch.base_revision.0, current_revision.0
        )));
    }
    if patch.target_revision <= patch.base_revision {
        return Err(VizError::Diagnostic(format!(
            "VIZ-PATCH-0002: target revision {} must be greater than base revision {}",
            patch.target_revision.0, patch.base_revision.0
        )));
    }

    let mut next = scene.clone();
    for operation in &patch.operations {
        apply_operation(&mut next, operation)?;
    }
    validate_scene_ids(&next.nodes)?;
    Ok((next, patch.target_revision))
}

pub fn scene_patch_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(ScenePatch))
        .expect("ScenePatch schema must serialize");
    schema
        .as_object_mut()
        .expect("root schema must be an object")
        .insert(
            "$schema".to_owned(),
            serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".to_owned()),
        );
    schema
}

fn diff_children(
    previous: &[SceneNode],
    next: &[SceneNode],
    parent: SceneParent,
    operations: &mut Vec<SceneOp>,
) {
    let previous_by_id = previous
        .iter()
        .map(|node| (node.id(), node))
        .collect::<BTreeMap<_, _>>();
    let next_by_id = next
        .iter()
        .map(|node| (node.id(), node))
        .collect::<BTreeMap<_, _>>();

    for node in previous {
        if !next_by_id.contains_key(node.id()) {
            operations.push(SceneOp::RemoveNode {
                id: node.id().to_owned(),
            });
        }
    }
    for (index, node) in next.iter().enumerate() {
        match previous_by_id.get(node.id()) {
            Some(previous_node) => diff_node(previous_node, node, operations),
            None => operations.push(SceneOp::InsertNode {
                parent: parent.clone(),
                index,
                node: node.clone(),
            }),
        }
    }

    let previous_order = previous
        .iter()
        .filter(|node| next_by_id.contains_key(node.id()))
        .map(|node| node.id())
        .collect::<Vec<_>>();
    let next_existing_order = next
        .iter()
        .filter(|node| previous_by_id.contains_key(node.id()))
        .map(|node| node.id())
        .collect::<Vec<_>>();
    let inserted = next
        .iter()
        .any(|node| !previous_by_id.contains_key(node.id()));
    if previous_order != next_existing_order || inserted {
        operations.push(SceneOp::ReorderChildren {
            parent,
            order: next.iter().map(|node| node.id().to_owned()).collect(),
        });
    }
}

fn diff_node(previous: &SceneNode, next: &SceneNode, operations: &mut Vec<SceneOp>) {
    match (previous, next) {
        (
            SceneNode::Group {
                id: previous_id,
                bounds: previous_bounds,
                origin: previous_origin,
                transform: previous_transform,
                opacity: previous_opacity,
                children: previous_children,
            },
            SceneNode::Group {
                id: next_id,
                bounds: next_bounds,
                origin: next_origin,
                transform: next_transform,
                opacity: next_opacity,
                children: next_children,
            },
        ) if previous_id == next_id
            && previous_bounds == next_bounds
            && previous_origin == next_origin
            && previous_transform == next_transform
            && previous_opacity == next_opacity =>
        {
            diff_children(
                previous_children,
                next_children,
                SceneParent::Node {
                    id: next_id.clone(),
                },
                operations,
            );
        }
        _ if previous != next => operations.push(SceneOp::ReplaceNode {
            id: previous.id().to_owned(),
            node: next.clone(),
        }),
        _ => {}
    }
}

fn apply_operation(scene: &mut Scene2D, operation: &SceneOp) -> VizResult<()> {
    match operation {
        SceneOp::SetSceneProperties {
            width,
            height,
            background,
            losses,
        } => {
            scene.width = *width;
            scene.height = *height;
            scene.background = background.clone();
            scene.losses = losses.clone();
            Ok(())
        }
        SceneOp::InsertNode {
            parent,
            index,
            node,
        } => {
            let children = children_mut(&mut scene.nodes, parent)?;
            if *index > children.len() {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0006: insert index {index} exceeds child count {}",
                    children.len()
                )));
            }
            if find_node_mut(&mut scene.nodes, node.id()).is_some() {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0007: inserted node id {:?} already exists",
                    node.id()
                )));
            }
            let children = children_mut(&mut scene.nodes, parent)?;
            children.insert(*index, node.clone());
            Ok(())
        }
        SceneOp::RemoveNode { id } => {
            if remove_node(&mut scene.nodes, id) {
                Ok(())
            } else {
                Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0008: cannot remove missing scene node {id:?}"
                )))
            }
        }
        SceneOp::ReplaceNode { id, node } => {
            if id != node.id() {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0009: replacement changes stable id {id:?} to {:?}",
                    node.id()
                )));
            }
            let Some(existing) = find_node_mut(&mut scene.nodes, id) else {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0010: cannot replace missing scene node {id:?}"
                )));
            };
            *existing = node.clone();
            Ok(())
        }
        SceneOp::ReorderChildren { parent, order } => {
            let children = children_mut(&mut scene.nodes, parent)?;
            let current = children
                .iter()
                .map(|node| node.id().to_owned())
                .collect::<BTreeSet<_>>();
            let requested = order.iter().cloned().collect::<BTreeSet<_>>();
            if current != requested || requested.len() != order.len() {
                return Err(VizError::Diagnostic(
                    "VIZ-PATCH-0011: reorder must name every child exactly once".to_owned(),
                ));
            }
            let mut by_id = std::mem::take(children)
                .into_iter()
                .map(|node| (node.id().to_owned(), node))
                .collect::<BTreeMap<_, _>>();
            *children = order
                .iter()
                .map(|id| by_id.remove(id).expect("validated reorder member"))
                .collect();
            Ok(())
        }
    }
}

fn children_mut<'a>(
    roots: &'a mut Vec<SceneNode>,
    parent: &SceneParent,
) -> VizResult<&'a mut Vec<SceneNode>> {
    match parent {
        SceneParent::Root => Ok(roots),
        SceneParent::Node { id } => {
            let Some(node) = find_node_mut(roots, id) else {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0012: missing parent node {id:?}"
                )));
            };
            match node {
                SceneNode::Group { children, .. } => Ok(children),
                _ => Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0013: parent node {id:?} is not a group"
                ))),
            }
        }
    }
}

fn find_node_mut<'a>(nodes: &'a mut [SceneNode], id: &str) -> Option<&'a mut SceneNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let SceneNode::Group { children, .. } = node
            && let Some(found) = find_node_mut(children, id)
        {
            return Some(found);
        }
    }
    None
}

fn remove_node(nodes: &mut Vec<SceneNode>, id: &str) -> bool {
    if let Some(index) = nodes.iter().position(|node| node.id() == id) {
        nodes.remove(index);
        return true;
    }
    for node in nodes {
        if let SceneNode::Group { children, .. } = node
            && remove_node(children, id)
        {
            return true;
        }
    }
    false
}

fn validate_scene_ids(nodes: &[SceneNode]) -> VizResult<()> {
    fn visit(nodes: &[SceneNode], ids: &mut BTreeSet<String>) -> VizResult<()> {
        for node in nodes {
            if !ids.insert(node.id().to_owned()) {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATCH-0014: duplicate scene node id {:?} after patch",
                    node.id()
                )));
            }
            visit(node.children(), ids)?;
        }
        Ok(())
    }

    visit(nodes, &mut BTreeSet::new())
}

#[cfg(test)]
mod tests {
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
        let (actual, revision) = apply_scene_patch(&previous, Revision(7), &patch)
            .expect("patch should apply atomically");
        assert_eq!(revision, Revision(8));
        assert_eq!(actual, next);
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
}
