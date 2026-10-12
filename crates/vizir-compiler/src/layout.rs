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

/// Allocate separate side attachments before routing. Incoming and outgoing
/// endpoints share the same pool so reciprocal edges cannot reuse a port.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EdgeCurve {
    pub start: Point,
    pub end: Point,
    pub control1: Point,
    pub control2: Point,
}

pub(crate) fn route_edges(
    positions: &BTreeMap<String, Point>,
    edges: &[DiagramEdge],
) -> VizResult<Vec<EdgeCurve>> {
    for edge in edges {
        for (id, code) in [(&edge.from, "0002"), (&edge.to, "0003")] {
            if !positions.contains_key(id) {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-LAYOUT-{code}: missing position for {id:?}"
                )));
            }
        }
    }
    // Sides are right, bottom, left, top. Sort by the opposite node's
    // transverse coordinate, then source order, for deterministic fan-outs.
    type SideAttachments<'a> = BTreeMap<(&'a str, usize), Vec<(usize, bool, f64)>>;
    let mut sides: SideAttachments<'_> = BTreeMap::new();
    for (i, edge) in edges.iter().enumerate() {
        let from = positions[&edge.from];
        let to = positions[&edge.to];
        let (source_side, target_side) = if edge.from == edge.to {
            (0, 3)
        } else if (to.x - from.x).abs() >= (to.y - from.y).abs() {
            if to.x >= from.x { (0, 2) } else { (2, 0) }
        } else if to.y >= from.y {
            (1, 3)
        } else {
            (3, 1)
        };
        sides.entry((&edge.from, source_side)).or_default().push((
            i,
            true,
            if source_side % 2 == 0 { to.y } else { to.x },
        ));
        sides.entry((&edge.to, target_side)).or_default().push((
            i,
            false,
            if target_side % 2 == 0 { from.y } else { from.x },
        ));
    }
    let zero = Point { x: 0.0, y: 0.0 };
    let mut ports = vec![[(zero, zero); 2]; edges.len()];
    for ((id, side), mut ends) in sides {
        ends.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        let center = positions[id];
        let count = ends.len();
        for (slot, (index, source, _)) in ends.into_iter().enumerate() {
            let span: f64 = if side % 2 == 0 { 42.0 } else { 130.0 };
            let offset = if count == 1 {
                0.0
            } else {
                (slot as f64 / (count - 1) as f64 - 0.5) * span.min((count - 1) as f64 * 14.0)
            };
            let (point, normal) = match side {
                0 => (
                    Point {
                        x: center.x + 78.0,
                        y: center.y + offset,
                    },
                    Point { x: 1.0, y: 0.0 },
                ),
                1 => (
                    Point {
                        x: center.x + offset,
                        y: center.y + 34.0,
                    },
                    Point { x: 0.0, y: 1.0 },
                ),
                2 => (
                    Point {
                        x: center.x - 78.0,
                        y: center.y + offset,
                    },
                    Point { x: -1.0, y: 0.0 },
                ),
                _ => (
                    Point {
                        x: center.x + offset,
                        y: center.y - 34.0,
                    },
                    Point { x: 0.0, y: -1.0 },
                ),
            };
            ports[index][usize::from(!source)] = (point, normal);
        }
    }
    Ok(ports
        .into_iter()
        .enumerate()
        .map(|(i, [(start, sn), (end, tn)])| {
            let bend = if edges[i].from == edges[i].to {
                60.0
            } else {
                ((end.x - start.x).abs().max((end.y - start.y).abs()) * 0.45).max(28.0)
            };
            EdgeCurve {
                start,
                end,
                control1: Point {
                    x: start.x + sn.x * bend,
                    y: start.y + sn.y * bend,
                },
                control2: Point {
                    x: end.x + tn.x * bend,
                    y: end.y + tn.y * bend,
                },
            }
        })
        .collect())
}

#[cfg(test)]
mod connector_tests {
    use super::*;
    #[test]
    fn fan_in_parallel_reciprocal_and_loop_ports_are_distinct() {
        let positions = BTreeMap::from([
            ("a".into(), Point { x: 100.0, y: 200.0 }),
            ("b".into(), Point { x: 400.0, y: 100.0 }),
            ("c".into(), Point { x: 400.0, y: 300.0 }),
        ]);
        let edges: Vec<DiagramEdge> = serde_json::from_str(r#"[{"from":"a","to":"b"},{"from":"a","to":"b"},{"from":"b","to":"a"},{"from":"c","to":"a"},{"from":"a","to":"a"}]"#).unwrap();
        let routes = route_edges(&positions, &edges).unwrap();
        let ends = [
            routes[0].start,
            routes[1].start,
            routes[2].end,
            routes[3].end,
            routes[4].start,
            routes[4].end,
        ];
        for (i, a) in ends.iter().enumerate() {
            for b in &ends[i + 1..] {
                assert!((a.x - b.x).hypot(a.y - b.y) > 1.0, "coincident ports");
            }
        }
        for route in routes {
            assert!((route.end.x - route.control2.x).hypot(route.end.y - route.control2.y) > 7.0);
        }
    }
    #[test]
    fn parallel_edges_do_not_trigger_false_cycle_fallback() {
        let nodes: Vec<DiagramNode> =
            serde_json::from_str(r#"[{"id":"a","label":"A"},{"id":"b","label":"B"}]"#).unwrap();
        let edges: Vec<DiagramEdge> =
            serde_json::from_str(r#"[{"from":"a","to":"b"},{"from":"a","to":"b"}]"#).unwrap();
        let result = layered_layout(
            Frame {
                x: 0.0,
                y: 0.0,
                width: 600.0,
                height: 400.0,
            },
            &nodes,
            &edges,
        )
        .unwrap();
        assert!(!result.explanation.contains("fallback"));
    }
}
