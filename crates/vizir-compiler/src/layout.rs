use std::collections::{BTreeMap, BTreeSet, VecDeque};

use vizir_core::{DiagramEdge, DiagramLayout, DiagramNode, Frame, Point, VizError, VizResult};

#[derive(Debug, Clone)]
pub struct LayoutResult {
    pub positions: BTreeMap<String, Point>,
    pub explanation: String,
}

pub trait LayoutProvider {
    fn layout(
        &self,
        algorithm: &DiagramLayout,
        frame: Frame,
        nodes: &[DiagramNode],
        edges: &[DiagramEdge],
    ) -> VizResult<LayoutResult>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayeredLayoutProvider;

impl LayoutProvider for LayeredLayoutProvider {
    fn layout(
        &self,
        algorithm: &DiagramLayout,
        frame: Frame,
        nodes: &[DiagramNode],
        edges: &[DiagramEdge],
    ) -> VizResult<LayoutResult> {
        match algorithm {
            DiagramLayout::Manual => manual_layout(frame, nodes),
            DiagramLayout::Layered => layered_layout(frame, nodes, edges),
        }
    }
}

fn manual_layout(frame: Frame, nodes: &[DiagramNode]) -> VizResult<LayoutResult> {
    let mut positions = BTreeMap::new();
    for node in nodes {
        let position = node.position.ok_or_else(|| {
            VizError::Diagnostic(format!(
                "VIZ-LAYOUT-0001: node {:?} has no position for manual layout",
                node.id
            ))
        })?;
        positions.insert(
            node.id.clone(),
            Point {
                x: frame.x + position.x,
                y: frame.y + position.y,
            },
        );
    }
    Ok(LayoutResult {
        positions,
        explanation: "manual positions resolved from view-local to Scene2D coordinates".to_owned(),
    })
}

fn layered_layout(
    frame: Frame,
    nodes: &[DiagramNode],
    edges: &[DiagramEdge],
) -> VizResult<LayoutResult> {
    let mut indegree = nodes
        .iter()
        .map(|node| (node.id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in edges {
        *indegree.entry(edge.to.clone()).or_default() += 1;
        outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
    }
    for targets in outgoing.values_mut() {
        targets.sort();
        targets.dedup();
    }

    let mut queue = VecDeque::from(
        indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
            .collect::<Vec<_>>(),
    );
    let mut rank = nodes
        .iter()
        .map(|node| (node.id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        visited.insert(node.clone());
        let node_rank = rank[&node];
        if let Some(targets) = outgoing.get(&node) {
            for target in targets {
                rank.entry(target.clone())
                    .and_modify(|value| *value = (*value).max(node_rank + 1));
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(target.clone());
                    }
                }
            }
        }
    }

    // A deterministic cycle fallback keeps the layout total while preserving a
    // diagnostic-quality explanation. SCC condensation is a later provider concern.
    let max_rank = rank.values().copied().max().unwrap_or(0);
    for (offset, node) in nodes
        .iter()
        .filter(|node| !visited.contains(&node.id))
        .map(|node| node.id.clone())
        .enumerate()
    {
        rank.insert(node, max_rank + 1 + offset);
    }

    let mut layers: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for node in nodes {
        layers
            .entry(rank[&node.id])
            .or_default()
            .push(node.id.clone());
    }
    for layer in layers.values_mut() {
        layer.sort();
    }

    let left = frame.x + 95.0;
    let right = frame.x + frame.width - 95.0;
    let top = frame.y + 80.0;
    let bottom = frame.y + frame.height - 55.0;
    let layer_count = layers.len().max(1);
    let mut positions = BTreeMap::new();
    for (column_index, (_, layer)) in layers.iter().enumerate() {
        let x = if layer_count == 1 {
            (left + right) / 2.0
        } else {
            left + (right - left) * column_index as f64 / (layer_count - 1) as f64
        };
        for (row_index, node) in layer.iter().enumerate() {
            let y = if layer.len() == 1 {
                (top + bottom) / 2.0
            } else {
                top + (bottom - top) * row_index as f64 / (layer.len() - 1) as f64
            };
            positions.insert(node.clone(), Point { x, y });
        }
    }

    let cycle_note = if visited.len() == nodes.len() {
        "directed acyclic topology ranked with deterministic Kahn traversal"
    } else {
        "cycles were placed in deterministic fallback layers after acyclic ranking"
    };
    Ok(LayoutResult {
        positions,
        explanation: cycle_note.to_owned(),
    })
}
